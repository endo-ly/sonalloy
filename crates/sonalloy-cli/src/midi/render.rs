use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use sonalloy_core::{
    DEFAULT_TEMPO_BPM, Diagnostic, DiagnosticCode, MusicalTimeMap, ProcessEventKind, ScheduledEvent,
};

use crate::midi::parse::{RawMidiEventKind, RawMidiTempoChange, RawMidiTimeSignatureChange};
use crate::midi::parse_midi;
use crate::midi::{normalize_control, normalize_pitch_bend, note_id};
use crate::musical_time::{TempoPoint, TimeSignaturePoint, build_musical_time_map, tick_to_frame};
use crate::pattern::PatternTimeSignatureChange;

pub(crate) struct MidiRender {
    pub(crate) events: Vec<ScheduledEvent>,
    pub(crate) duration_frames: u64,
    pub(crate) musical_time_map: MusicalTimeMap,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy)]
struct ConvertedEvent {
    frame: u64,
    track: usize,
    index: usize,
    channel: u8,
    kind: ProcessEventKind,
}

#[derive(Debug)]
struct PendingRenderNote {
    note_id: u64,
    start_tick: u64,
    start_frame: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ControlKind {
    SustainPedal,
    PitchBend,
    ModWheel,
    Aftertouch,
}

impl ControlKind {
    const ALL: [Self; 4] = [
        Self::SustainPedal,
        Self::PitchBend,
        Self::ModWheel,
        Self::Aftertouch,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::SustainPedal => "sustain pedal",
            Self::PitchBend => "pitch bend",
            Self::ModWheel => "mod wheel",
            Self::Aftertouch => "aftertouch",
        }
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn read_midi(path: &Path, sample_rate: f64) -> Result<MidiRender, Vec<Diagnostic>> {
    let parsed = parse_midi(path)?;
    let mut diagnostics = parsed.diagnostics;
    let tempo_points = midi_tempo_points(&parsed.tempo_changes);
    let signature_changes = imported_time_signature_changes(&parsed.time_signature_changes)?;
    let signature_points = signature_changes
        .iter()
        .map(|change| TimeSignaturePoint {
            tick: change.tick,
            numerator: u16::from(change.numerator),
            denominator: u16::from(change.denominator),
        })
        .collect::<Vec<_>>();
    let musical_time_map = build_musical_time_map(
        parsed.ticks_per_beat,
        &tempo_points,
        &signature_points,
        sample_rate,
    )
    .map_err(|error| {
        vec![
            Diagnostic::error(DiagnosticCode::MidiError, "MIDI tempo map is invalid")
                .with_path(path.to_string_lossy())
                .with_detail(error.to_string()),
        ]
    })?;

    let mut active_notes: HashMap<(u8, u8), VecDeque<PendingRenderNote>> = HashMap::new();
    let mut serials: HashMap<(u8, u8), u32> = HashMap::new();
    let mut zero_length_note_ids = HashSet::new();
    let mut converted = Vec::new();
    for raw in &parsed.events {
        let frame = tick_to_frame(raw.tick, parsed.ticks_per_beat, &tempo_points, sample_rate)
            .map_err(|error| {
                vec![
                    Diagnostic::error(
                        DiagnosticCode::MidiError,
                        "MIDI event frame is outside the Core frame range",
                    )
                    .with_path(path.to_string_lossy())
                    .with_detail(error.to_string()),
                ]
            })?;
        match raw.kind {
            RawMidiEventKind::NoteOn {
                channel,
                note,
                velocity,
            } => {
                let key = (channel, note);
                let serial = serials.entry(key).or_default();
                let current_serial = *serial;
                let note_id = note_id(channel, note, current_serial);
                *serial = serial.checked_add(1).ok_or_else(|| {
                    vec![Diagnostic::error(
                        DiagnosticCode::MidiError,
                        "MIDI note serial overflow",
                    )]
                })?;
                active_notes
                    .entry(key)
                    .or_default()
                    .push_back(PendingRenderNote {
                        note_id,
                        start_tick: raw.tick,
                        start_frame: frame,
                    });
                converted.push(ConvertedEvent {
                    frame,
                    track: raw.track,
                    index: raw.index,
                    channel,
                    kind: ProcessEventKind::NoteOn {
                        note_id,
                        note_number: note,
                        velocity,
                    },
                });
            }
            RawMidiEventKind::NoteOff { channel, note } => {
                let key = (channel, note);
                let Some(pending) = active_notes.get_mut(&key).and_then(VecDeque::pop_front) else {
                    diagnostics.push(
                        Diagnostic::warning(
                            DiagnosticCode::MidiError,
                            "Note Off without a matching Note On was ignored",
                        )
                        .with_detail(format!("channel {channel}, note {note}")),
                    );
                    continue;
                };
                if pending.start_tick == raw.tick || pending.start_frame == frame {
                    zero_length_note_ids.insert(pending.note_id);
                } else {
                    converted.push(ConvertedEvent {
                        frame,
                        track: raw.track,
                        index: raw.index,
                        channel,
                        kind: ProcessEventKind::NoteOff {
                            note_id: pending.note_id,
                        },
                    });
                }
            }
            RawMidiEventKind::SustainPedal { channel, down } => {
                converted.push(ConvertedEvent {
                    frame,
                    track: raw.track,
                    index: raw.index,
                    channel,
                    kind: ProcessEventKind::SustainPedal { down },
                });
            }
            RawMidiEventKind::PitchBend { channel, value } => {
                converted.push(ConvertedEvent {
                    frame,
                    track: raw.track,
                    index: raw.index,
                    channel,
                    kind: ProcessEventKind::PitchBend {
                        value: normalize_pitch_bend(value),
                    },
                });
            }
            RawMidiEventKind::ModWheel { channel, value } => {
                converted.push(ConvertedEvent {
                    frame,
                    track: raw.track,
                    index: raw.index,
                    channel,
                    kind: ProcessEventKind::ModWheel {
                        value: normalize_control(value),
                    },
                });
            }
            RawMidiEventKind::Aftertouch { channel, value } => {
                converted.push(ConvertedEvent {
                    frame,
                    track: raw.track,
                    index: raw.index,
                    channel,
                    kind: ProcessEventKind::Aftertouch {
                        value: normalize_control(value),
                    },
                });
            }
        }
    }
    converted.retain(|event| match event.kind {
        ProcessEventKind::NoteOn { note_id, .. } | ProcessEventKind::NoteOff { note_id } => {
            !zero_length_note_ids.contains(&note_id)
        }
        _ => true,
    });
    if !converted
        .iter()
        .any(|event| matches!(event.kind, ProcessEventKind::NoteOn { .. }))
    {
        return Err(vec![
            Diagnostic::error(
                DiagnosticCode::MidiError,
                "MIDI file contains no note events",
            )
            .with_path(path.to_string_lossy()),
        ]);
    }
    let duration_frames = converted
        .iter()
        .map(|event| event.frame)
        .max()
        .and_then(|frame| frame.checked_add(1))
        .ok_or_else(|| {
            vec![Diagnostic::error(
                DiagnosticCode::MidiError,
                "MIDI duration overflow",
            )]
        })?;
    converted.sort_by_key(|event| (event.frame, event.kind.priority(), event.track, event.index));
    let mut note_channels = [false; 16];
    for event in &converted {
        if matches!(event.kind, ProcessEventKind::NoteOn { .. }) {
            note_channels[usize::from(event.channel)] = true;
        }
    }
    if note_channels.iter().filter(|channel| **channel).count() > 1 {
        diagnostics.push(Diagnostic::warning(
            DiagnosticCode::MidiError,
            "notes from multiple MIDI channels were merged into one instrument",
        ));
    }
    for kind in ControlKind::ALL {
        if control_warning_needed(kind, &converted) {
            diagnostics.push(Diagnostic::warning(
                DiagnosticCode::MidiError,
                format!(
                    "{} controls from MIDI channels were merged into one instrument",
                    kind.label()
                ),
            ));
        }
    }
    Ok(MidiRender {
        events: converted
            .into_iter()
            .map(|event| ScheduledEvent {
                absolute_frame: event.frame,
                kind: event.kind,
            })
            .collect(),
        duration_frames,
        musical_time_map,
        diagnostics,
    })
}

fn control_warning_needed(kind: ControlKind, events: &[ConvertedEvent]) -> bool {
    let mut active_notes = [0_u32; 16];
    let mut channel_values = [0.0_f32; 16];
    let mut merged_value = 0.0_f32;
    let mut index = 0;

    while index < events.len() {
        let frame = events[index].frame;
        while index < events.len() && events[index].frame == frame {
            let event = events[index];
            let channel = usize::from(event.channel);
            match event.kind {
                ProcessEventKind::NoteOff { .. } => {
                    active_notes[channel] = active_notes[channel].saturating_sub(1);
                }
                ProcessEventKind::NoteOn { .. } => {
                    active_notes[channel] = active_notes[channel].saturating_add(1);
                }
                ProcessEventKind::SustainPedal { down } if kind == ControlKind::SustainPedal => {
                    let value = if down { 1.0 } else { 0.0 };
                    channel_values[channel] = value;
                    merged_value = value;
                }
                ProcessEventKind::PitchBend { value } if kind == ControlKind::PitchBend => {
                    channel_values[channel] = value;
                    merged_value = value;
                }
                ProcessEventKind::ModWheel { value } if kind == ControlKind::ModWheel => {
                    channel_values[channel] = value;
                    merged_value = value;
                }
                ProcessEventKind::Aftertouch { value } if kind == ControlKind::Aftertouch => {
                    channel_values[channel] = value;
                    merged_value = value;
                }
                _ => {}
            }
            index += 1;
        }

        if (0..16).any(|channel| {
            active_notes[channel] > 0
                && channel_values[channel].total_cmp(&merged_value) != std::cmp::Ordering::Equal
        }) {
            return true;
        }
    }
    false
}

fn midi_tempo_points(changes: &[RawMidiTempoChange]) -> Vec<TempoPoint> {
    let mut points = vec![TempoPoint {
        tick: 0,
        bpm: DEFAULT_TEMPO_BPM,
    }];
    for change in changes {
        let bpm = 60_000_000.0 / f64::from(change.microseconds_per_beat);
        if let Some(previous) = points.last_mut().filter(|point| point.tick == change.tick) {
            previous.bpm = bpm;
        } else {
            points.push(TempoPoint {
                tick: change.tick,
                bpm,
            });
        }
    }
    points.sort_by_key(|point| point.tick);
    points
}

#[allow(clippy::too_many_lines)]
pub(crate) fn imported_time_signature_changes(
    changes: &[RawMidiTimeSignatureChange],
) -> Result<Vec<PatternTimeSignatureChange>, Vec<Diagnostic>> {
    let mut result = vec![PatternTimeSignatureChange {
        tick: 0,
        numerator: 4,
        denominator: 4,
    }];
    for change in changes {
        let Some(denominator) = 1_u16.checked_shl(u32::from(change.denominator_power)) else {
            return Err(vec![
                Diagnostic::error(
                    DiagnosticCode::MidiError,
                    "MIDI time signature denominator is not representable",
                )
                .with_detail(format!("track {}, event {}", change.track, change.index)),
            ]);
        };
        let Ok(denominator) = u8::try_from(denominator) else {
            return Err(vec![
                Diagnostic::error(
                    DiagnosticCode::MidiError,
                    "MIDI time signature denominator must be between 1 and 128",
                )
                .with_detail(format!("track {}, event {}", change.track, change.index)),
            ]);
        };
        let value = PatternTimeSignatureChange {
            tick: change.tick,
            numerator: change.numerator,
            denominator,
        };
        if let Some(previous) = result
            .last_mut()
            .filter(|previous| previous.tick == change.tick)
        {
            *previous = value;
        } else {
            result.push(value);
        }
    }
    result.sort_by_key(|change| change.tick);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::midi::{MOD_WHEEL_CONTROLLER, SUSTAIN_PEDAL_CONTROLLER};
    use midly::{
        Format, Header, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
        num::{u4, u7, u15, u24},
    };

    use super::*;

    fn midi_file(events: Vec<TrackEvent<'static>>) -> (tempfile::TempDir, PathBuf) {
        midi_file_with_ticks_per_beat(480, events)
    }

    fn midi_file_with_ticks_per_beat(
        ticks_per_beat: u16,
        events: Vec<TrackEvent<'static>>,
    ) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("test.mid");
        let mut smf = Smf::new(Header::new(
            Format::SingleTrack,
            Timing::Metrical(u15::new(ticks_per_beat)),
        ));
        smf.tracks.push(events);
        smf.save(&path).expect("MIDI fixture");
        (directory, path)
    }

    fn midi_event_with_delta(channel: u8, message: MidiMessage, delta: u32) -> TrackEvent<'static> {
        TrackEvent {
            delta: delta.into(),
            kind: TrackEventKind::Midi {
                channel: u4::new(channel),
                message,
            },
        }
    }

    fn note_on_with_delta(channel: u8, delta: u32) -> TrackEvent<'static> {
        midi_event_with_delta(
            channel,
            MidiMessage::NoteOn {
                key: u7::new(60),
                vel: u7::new(100),
            },
            delta,
        )
    }

    fn note_on(channel: u8) -> TrackEvent<'static> {
        note_on_with_delta(channel, 0)
    }

    fn note_off_with_delta(channel: u8, delta: u32) -> TrackEvent<'static> {
        midi_event_with_delta(
            channel,
            MidiMessage::NoteOff {
                key: u7::new(60),
                vel: u7::new(0),
            },
            delta,
        )
    }

    fn note_off(channel: u8) -> TrackEvent<'static> {
        note_off_with_delta(channel, 0)
    }

    fn pitch_bend_with_delta(channel: u8, value: i16, delta: u32) -> TrackEvent<'static> {
        midi_event_with_delta(
            channel,
            MidiMessage::PitchBend {
                bend: midly::PitchBend::from_int(value),
            },
            delta,
        )
    }

    fn pitch_bend(channel: u8, value: i16) -> TrackEvent<'static> {
        pitch_bend_with_delta(channel, value, 0)
    }

    fn controller_with_delta(
        channel: u8,
        controller: u8,
        value: u8,
        delta: u32,
    ) -> TrackEvent<'static> {
        midi_event_with_delta(
            channel,
            MidiMessage::Controller {
                controller: u7::new(controller),
                value: u7::new(value),
            },
            delta,
        )
    }

    fn aftertouch_with_delta(channel: u8, value: u8, delta: u32) -> TrackEvent<'static> {
        midi_event_with_delta(
            channel,
            MidiMessage::ChannelAftertouch {
                vel: u7::new(value),
            },
            delta,
        )
    }

    fn end_of_track() -> TrackEvent<'static> {
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
        }
    }

    #[test]
    fn pitch_bend_conversion_uses_the_asymmetric_midi_center() {
        assert!((normalize_pitch_bend(-8192) + 1.0).abs() < f32::EPSILON);
        assert!(normalize_pitch_bend(0).abs() < f32::EPSILON);
        assert!((normalize_pitch_bend(8191) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn supported_midi_controls_are_converted() {
        let (_directory, path) = midi_file(vec![
            note_on(0),
            controller_with_delta(0, MOD_WHEEL_CONTROLLER, 64, 0),
            controller_with_delta(0, SUSTAIN_PEDAL_CONTROLLER, 127, 0),
            aftertouch_with_delta(0, 32, 0),
            pitch_bend(0, 4096),
            note_off_with_delta(0, 480),
            controller_with_delta(0, SUSTAIN_PEDAL_CONTROLLER, 0, 0),
            end_of_track(),
        ]);

        let render = read_midi(&path, 48_000.0).expect("MIDI with supported controls");
        assert!(
            render
                .events
                .iter()
                .any(|event| matches!(event.kind, ProcessEventKind::SustainPedal { down: true }))
        );
        assert!(
            render
                .events
                .iter()
                .any(|event| matches!(event.kind, ProcessEventKind::SustainPedal { down: false }))
        );
        assert!(render.events.iter().any(|event| matches!(
            event.kind,
            ProcessEventKind::ModWheel { value }
                if (value - normalize_control(64)).abs() < f32::EPSILON
        )));
        assert!(render.events.iter().any(|event| matches!(
            event.kind,
            ProcessEventKind::Aftertouch { value }
                if (value - normalize_control(32)).abs() < f32::EPSILON
        )));
        assert!(render.events.iter().any(|event| matches!(
            event.kind,
            ProcessEventKind::PitchBend { value }
                if (value - normalize_pitch_bend(4096)).abs() < f32::EPSILON
        )));
    }

    #[test]
    fn tempo_events_become_absolute_frame_tempo_changes() {
        let (_directory, path) = midi_file(vec![
            TrackEvent {
                delta: 0.into(),
                kind: TrackEventKind::Meta(midly::MetaMessage::Tempo(u24::new(500_000))),
            },
            note_on_with_delta(0, 0),
            TrackEvent {
                delta: 480.into(),
                kind: TrackEventKind::Meta(midly::MetaMessage::Tempo(u24::new(1_000_000))),
            },
            note_off_with_delta(0, 480),
            end_of_track(),
        ]);

        let render = read_midi(&path, 48_000.0).expect("valid MIDI");
        assert_eq!(render.musical_time_map.changes().len(), 2);
        assert_eq!(render.musical_time_map.changes()[1].absolute_frame, 24_000);
        assert_eq!(render.events[0].absolute_frame, 0);
        assert_eq!(render.events[1].absolute_frame, 72_000);
    }

    #[test]
    fn control_only_midi_is_rejected() {
        let (_directory, path) = midi_file(vec![
            midi_event_with_delta(
                0,
                MidiMessage::PitchBend {
                    bend: midly::PitchBend::from_int(0),
                },
                0,
            ),
            end_of_track(),
        ]);
        let Err(diagnostics) = read_midi(&path, 48_000.0) else {
            panic!("control-only MIDI must fail");
        };
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message == "MIDI file contains no note events")
        );
    }

    #[test]
    fn control_from_a_channel_without_notes_emits_a_warning() {
        let (_directory, path) = midi_file(vec![
            note_on(0),
            pitch_bend(1, 4096),
            note_off_with_delta(0, 480),
            end_of_track(),
        ]);
        let render = read_midi(&path, 48_000.0).expect("MIDI with notes is valid");
        assert!(render.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("pitch bend controls from MIDI channels")
        }));
    }

    #[test]
    fn control_after_all_notes_end_needs_no_warning() {
        let (_directory, path) = midi_file(vec![
            note_on(0),
            note_off_with_delta(0, 480),
            pitch_bend_with_delta(1, 4096, 480),
            end_of_track(),
        ]);
        let render = read_midi(&path, 48_000.0).expect("MIDI with notes is valid");
        assert!(!render.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("pitch bend controls from MIDI channels")
        }));
    }

    #[test]
    fn control_from_a_later_note_channel_warns_during_another_channel_note() {
        let (_directory, path) = midi_file(vec![
            note_on(0),
            pitch_bend_with_delta(1, 4096, 480),
            note_off_with_delta(0, 480),
            note_on_with_delta(1, 480),
            note_off_with_delta(1, 480),
            end_of_track(),
        ]);
        let render = read_midi(&path, 48_000.0).expect("MIDI with notes is valid");
        assert!(render.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("pitch bend controls from MIDI channels")
        }));
    }

    #[test]
    fn same_channel_note_and_control_needs_no_warning() {
        let (_directory, path) = midi_file(vec![
            note_on(0),
            pitch_bend(0, 4096),
            note_off_with_delta(0, 480),
            end_of_track(),
        ]);
        let render = read_midi(&path, 48_000.0).expect("MIDI with notes is valid");
        assert!(!render.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("pitch bend controls from MIDI channels")
        }));
    }

    #[test]
    fn differing_control_values_across_channels_emit_a_warning() {
        let (_directory, path) = midi_file(vec![
            note_on(0),
            note_on(1),
            pitch_bend(0, 0),
            pitch_bend(1, 4096),
            note_off_with_delta(0, 480),
            note_off(1),
            end_of_track(),
        ]);
        let render = read_midi(&path, 48_000.0).expect("MIDI with notes is valid");
        assert!(render.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("pitch bend controls from MIDI channels")
        }));
    }

    #[test]
    fn differing_controls_in_non_overlapping_notes_need_no_warning() {
        let (_directory, path) = midi_file(vec![
            note_on(0),
            pitch_bend(0, 4096),
            note_off_with_delta(0, 480),
            note_on(1),
            pitch_bend(1, -4096),
            note_off_with_delta(1, 480),
            end_of_track(),
        ]);
        let render = read_midi(&path, 48_000.0).expect("MIDI with notes is valid");
        assert!(!render.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("pitch bend controls from MIDI channels")
        }));
    }

    #[test]
    fn equal_control_values_across_note_channels_need_no_control_warning() {
        let (_directory, path) = midi_file(vec![
            note_on(0),
            note_on(1),
            pitch_bend(0, 4096),
            pitch_bend(1, 4096),
            note_off_with_delta(0, 480),
            note_off(1),
            end_of_track(),
        ]);
        let render = read_midi(&path, 48_000.0).expect("MIDI with notes is valid");
        assert!(!render.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("pitch bend controls from MIDI channels")
        }));
        assert!(render.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("notes from multiple MIDI channels")
        }));
    }

    #[test]
    fn notes_from_multiple_channels_emit_a_warning() {
        let (_directory, path) = midi_file(vec![
            note_on(0),
            note_on(1),
            note_off_with_delta(0, 480),
            note_off(1),
            end_of_track(),
        ]);
        let render = read_midi(&path, 48_000.0).expect("MIDI with notes is valid");
        assert!(render.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("notes from multiple MIDI channels")
        }));
    }

    #[test]
    fn same_tick_note_is_removed_from_render_input() {
        let (_directory, path) = midi_file(vec![
            note_on_with_delta(0, 0),
            note_off_with_delta(0, 0),
            note_on_with_delta(0, 480),
            note_off_with_delta(0, 480),
            end_of_track(),
        ]);
        let render = read_midi(&path, 48_000.0).expect("non-zero note remains");
        assert_eq!(render.events.len(), 2);
    }

    #[test]
    fn notes_that_share_a_frame_but_not_a_tick_are_removed_from_render_input() {
        let (_directory, path) = midi_file_with_ticks_per_beat(
            32_767,
            vec![
                TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Meta(midly::MetaMessage::Tempo(u24::new(100_000))),
                },
                note_on(0),
                note_off_with_delta(0, 1),
                note_on_with_delta(0, 479),
                note_off_with_delta(0, 480),
                end_of_track(),
            ],
        );
        let render = read_midi(&path, 48_000.0).expect("the longer note remains");

        assert_eq!(render.events.len(), 2);
        assert!(render.events[0].absolute_frame < render.events[1].absolute_frame);
        assert!(render.events.iter().all(|event| match event.kind {
            ProcessEventKind::NoteOn {
                note_id: event_note_id,
                ..
            }
            | ProcessEventKind::NoteOff {
                note_id: event_note_id,
            } => {
                event_note_id == note_id(0, 60, 1)
            }
            _ => false,
        }));
    }
}
