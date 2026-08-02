use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use midly::{MidiMessage, Smf, Timing, TrackEventKind};
use sonalloy_core::{Diagnostic, DiagnosticCode, ProcessEventKind, ScheduledEvent};

/// MIDI events and duration prepared for the Core renderer.
pub(crate) struct MidiRender {
    /// Absolute-frame events in Core order.
    pub events: Vec<ScheduledEvent>,
    /// Minimum main duration that includes the final event.
    pub duration_frames: u64,
    /// Non-fatal MIDI conditions encountered during conversion.
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy)]
enum RawKind {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8 },
    PitchBend { channel: u8, value: i16 },
    ModWheel { channel: u8, value: u8 },
    Aftertouch { channel: u8, value: u8 },
    Tempo { microseconds_per_beat: u32 },
}

#[derive(Debug, Clone, Copy)]
struct RawEvent {
    tick: u64,
    track: usize,
    index: usize,
    kind: RawKind,
}

#[derive(Debug, Clone, Copy)]
struct ConvertedEvent {
    frame: u64,
    track: usize,
    index: usize,
    channel: u8,
    kind: ProcessEventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ControlKind {
    PitchBend,
    ModWheel,
    Aftertouch,
}

impl ControlKind {
    const ALL: [Self; 3] = [Self::PitchBend, Self::ModWheel, Self::Aftertouch];

    const fn label(self) -> &'static str {
        match self {
            Self::PitchBend => "pitch bend",
            Self::ModWheel => "mod wheel",
            Self::Aftertouch => "aftertouch",
        }
    }
}

/// Parse a Standard MIDI File and convert it to normalized absolute-frame events.
#[allow(clippy::too_many_lines)]
pub(crate) fn read_midi(path: &Path, sample_rate: f64) -> Result<MidiRender, Vec<Diagnostic>> {
    let bytes = std::fs::read(path).map_err(|error| {
        vec![
            Diagnostic::error(DiagnosticCode::MidiError, "could not read MIDI input")
                .with_path(path.to_string_lossy())
                .with_detail(error.to_string()),
        ]
    })?;
    let smf = Smf::parse(&bytes).map_err(|error| {
        vec![
            Diagnostic::error(DiagnosticCode::MidiError, "could not parse MIDI input")
                .with_path(path.to_string_lossy())
                .with_detail(error.to_string()),
        ]
    })?;
    let ticks_per_beat = match smf.header.timing {
        Timing::Metrical(value) => u64::from(value.as_int()),
        Timing::Timecode(_, _) => {
            return Err(vec![
                Diagnostic::error(
                    DiagnosticCode::MidiError,
                    "SMPTE timecode MIDI files are not supported",
                )
                .with_path(path.to_string_lossy()),
            ]);
        }
    };
    if ticks_per_beat == 0 {
        return Err(vec![
            Diagnostic::error(
                DiagnosticCode::MidiError,
                "MIDI ticks per beat must be positive",
            )
            .with_path(path.to_string_lossy()),
        ]);
    }

    let mut diagnostics = Vec::new();
    let mut raw_events = Vec::new();
    for (track_index, track) in smf.tracks.iter().enumerate() {
        let mut tick = 0_u64;
        for (event_index, event) in track.iter().enumerate() {
            tick = tick
                .checked_add(u64::from(event.delta.as_int()))
                .ok_or_else(|| {
                    vec![Diagnostic::error(
                        DiagnosticCode::MidiError,
                        "MIDI tick position overflow",
                    )]
                })?;
            match event.kind {
                TrackEventKind::Midi { channel, message } => {
                    let channel = channel.as_int();
                    match message {
                        MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                            raw_events.push(RawEvent {
                                tick,
                                track: track_index,
                                index: event_index,
                                kind: RawKind::NoteOn {
                                    channel,
                                    note: key.as_int(),
                                    velocity: vel.as_int(),
                                },
                            });
                        }
                        MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                            raw_events.push(RawEvent {
                                tick,
                                track: track_index,
                                index: event_index,
                                kind: RawKind::NoteOff {
                                    channel,
                                    note: key.as_int(),
                                },
                            });
                        }
                        MidiMessage::PitchBend { bend } => {
                            raw_events.push(RawEvent {
                                tick,
                                track: track_index,
                                index: event_index,
                                kind: RawKind::PitchBend {
                                    channel,
                                    value: bend.as_int(),
                                },
                            });
                        }
                        MidiMessage::Controller { controller, value }
                            if controller.as_int() == 1 =>
                        {
                            raw_events.push(RawEvent {
                                tick,
                                track: track_index,
                                index: event_index,
                                kind: RawKind::ModWheel {
                                    channel,
                                    value: value.as_int(),
                                },
                            });
                        }
                        MidiMessage::ChannelAftertouch { vel } => {
                            raw_events.push(RawEvent {
                                tick,
                                track: track_index,
                                index: event_index,
                                kind: RawKind::Aftertouch {
                                    channel,
                                    value: vel.as_int(),
                                },
                            });
                        }
                        MidiMessage::Controller { controller, .. } if controller.as_int() == 64 => {
                            diagnostics.push(
                                Diagnostic::warning(
                                    DiagnosticCode::MidiError,
                                    "sustain pedal event is not supported and was ignored",
                                )
                                .with_path(format!("track[{track_index}].event[{event_index}]")),
                            );
                        }
                        MidiMessage::Aftertouch { .. } => {
                            diagnostics.push(
                                Diagnostic::warning(
                                    DiagnosticCode::MidiError,
                                    "polyphonic aftertouch is not supported and was ignored",
                                )
                                .with_path(format!("track[{track_index}].event[{event_index}]")),
                            );
                        }
                        MidiMessage::Controller { .. } | MidiMessage::ProgramChange { .. } => {
                            diagnostics.push(
                                Diagnostic::warning(
                                    DiagnosticCode::MidiError,
                                    "unsupported MIDI event was ignored",
                                )
                                .with_path(format!("track[{track_index}].event[{event_index}]")),
                            );
                        }
                    }
                }
                TrackEventKind::Meta(midly::MetaMessage::Tempo(value)) => {
                    let microseconds_per_beat = value.as_int();
                    if microseconds_per_beat == 0 {
                        return Err(vec![
                            Diagnostic::error(
                                DiagnosticCode::MidiError,
                                "MIDI tempo must be greater than zero",
                            )
                            .with_path(format!("track[{track_index}].event[{event_index}]")),
                        ]);
                    }
                    raw_events.push(RawEvent {
                        tick,
                        track: track_index,
                        index: event_index,
                        kind: RawKind::Tempo {
                            microseconds_per_beat,
                        },
                    });
                }
                TrackEventKind::Meta(_) | TrackEventKind::SysEx(_) | TrackEventKind::Escape(_) => {}
            }
        }
    }
    raw_events.sort_by_key(|event| (event.tick, event.track, event.index));
    let tempo_changes: Vec<(u64, u32)> = raw_events
        .iter()
        .filter_map(|event| match event.kind {
            RawKind::Tempo {
                microseconds_per_beat,
            } => Some((event.tick, microseconds_per_beat)),
            _ => None,
        })
        .collect();

    let mut tempo_index = 0_usize;
    let mut tempo = 500_000_u32;
    let mut cursor_tick = 0_u64;
    let mut cursor_frames = 0.0_f64;
    let mut active_notes: HashMap<(u8, u8), VecDeque<(u64, u64)>> = HashMap::new();
    let mut serials: HashMap<(u8, u8), u32> = HashMap::new();
    let mut zero_length_note_ids = HashSet::new();
    let mut converted = Vec::new();
    for raw in raw_events {
        advance_tempo(
            raw.tick,
            ticks_per_beat,
            sample_rate,
            &tempo_changes,
            &mut tempo_index,
            &mut tempo,
            &mut cursor_tick,
            &mut cursor_frames,
        );
        let frame = round_frame(cursor_frames)?;
        match raw.kind {
            RawKind::Tempo { .. } => {}
            RawKind::NoteOn {
                channel,
                note,
                velocity,
            } => {
                let key = (channel, note);
                let serial = serials.entry(key).or_default();
                let note_id =
                    (u64::from(channel) << 56) | (u64::from(note) << 48) | u64::from(*serial);
                *serial = serial.checked_add(1).ok_or_else(|| {
                    vec![Diagnostic::error(
                        DiagnosticCode::MidiError,
                        "MIDI note serial overflow",
                    )]
                })?;
                active_notes
                    .entry(key)
                    .or_default()
                    .push_back((note_id, frame));
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
            RawKind::NoteOff { channel, note } => {
                let key = (channel, note);
                let Some((note_id, start_frame)) =
                    active_notes.get_mut(&key).and_then(VecDeque::pop_front)
                else {
                    diagnostics.push(
                        Diagnostic::warning(
                            DiagnosticCode::MidiError,
                            "Note Off without a matching Note On was ignored",
                        )
                        .with_detail(format!("channel {channel}, note {note}")),
                    );
                    continue;
                };
                if start_frame == frame {
                    zero_length_note_ids.insert(note_id);
                } else {
                    converted.push(ConvertedEvent {
                        frame,
                        track: raw.track,
                        index: raw.index,
                        channel,
                        kind: ProcessEventKind::NoteOff { note_id },
                    });
                }
            }
            RawKind::PitchBend { channel, value } => {
                let normalized = pitch_bend_value(value);
                converted.push(ConvertedEvent {
                    frame,
                    track: raw.track,
                    index: raw.index,
                    channel,
                    kind: ProcessEventKind::PitchBend { value: normalized },
                });
            }
            RawKind::ModWheel { channel, value } => {
                let normalized = f32::from(value) / 127.0;
                converted.push(ConvertedEvent {
                    frame,
                    track: raw.track,
                    index: raw.index,
                    channel,
                    kind: ProcessEventKind::ModWheel { value: normalized },
                });
            }
            RawKind::Aftertouch { channel, value } => {
                let normalized = f32::from(value) / 127.0;
                converted.push(ConvertedEvent {
                    frame,
                    track: raw.track,
                    index: raw.index,
                    channel,
                    kind: ProcessEventKind::Aftertouch { value: normalized },
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
        diagnostics,
    })
}

fn pitch_bend_value(value: i16) -> f32 {
    if value < 0 {
        f32::from(value) / 8192.0
    } else {
        f32::from(value) / 8191.0
    }
}

fn control_warning_needed(kind: ControlKind, events: &[ConvertedEvent]) -> bool {
    let mut active_notes = [0_u32; 16];
    let mut channel_values = [0.0_f32; 16];
    let mut merged_value = 0.0_f32;
    let mut control_channels = [false; 16];
    let mut note_channels = [false; 16];
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
                    note_channels[channel] = true;
                }
                ProcessEventKind::PitchBend { value } if kind == ControlKind::PitchBend => {
                    channel_values[channel] = value;
                    merged_value = value;
                    control_channels[channel] = true;
                }
                ProcessEventKind::ModWheel { value } if kind == ControlKind::ModWheel => {
                    channel_values[channel] = value;
                    merged_value = value;
                    control_channels[channel] = true;
                }
                ProcessEventKind::Aftertouch { value } if kind == ControlKind::Aftertouch => {
                    channel_values[channel] = value;
                    merged_value = value;
                    control_channels[channel] = true;
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

    control_channels
        .iter()
        .zip(note_channels)
        .any(|(has_control, has_note)| *has_control && !has_note)
}

#[allow(clippy::too_many_arguments)]
fn advance_tempo(
    target_tick: u64,
    ticks_per_beat: u64,
    sample_rate: f64,
    tempo_changes: &[(u64, u32)],
    tempo_index: &mut usize,
    tempo: &mut u32,
    cursor_tick: &mut u64,
    cursor_frames: &mut f64,
) {
    while *tempo_index < tempo_changes.len() && tempo_changes[*tempo_index].0 <= target_tick {
        let (change_tick, change_tempo) = tempo_changes[*tempo_index];
        if change_tick > *cursor_tick {
            *cursor_frames += ticks_to_frames(
                change_tick - *cursor_tick,
                *tempo,
                ticks_per_beat,
                sample_rate,
            );
            *cursor_tick = change_tick;
        }
        *tempo = change_tempo;
        *tempo_index += 1;
    }
    if target_tick > *cursor_tick {
        *cursor_frames += ticks_to_frames(
            target_tick - *cursor_tick,
            *tempo,
            ticks_per_beat,
            sample_rate,
        );
        *cursor_tick = target_tick;
    }
}

fn ticks_to_frames(
    ticks: u64,
    microseconds_per_beat: u32,
    ticks_per_beat: u64,
    sample_rate: f64,
) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let ticks_f64 = ticks as f64;
    #[allow(clippy::cast_precision_loss)]
    let ticks_per_beat_f64 = ticks_per_beat as f64;
    ticks_f64 * f64::from(microseconds_per_beat) * sample_rate / 1_000_000.0 / ticks_per_beat_f64
}

fn round_frame(frames: f64) -> Result<u64, Vec<Diagnostic>> {
    #[allow(clippy::cast_precision_loss)]
    let max_frame = u64::MAX as f64;
    if !frames.is_finite() || frames < 0.0 || frames >= max_frame {
        return Err(vec![Diagnostic::error(
            DiagnosticCode::MidiError,
            "MIDI event frame is outside the Core frame range",
        )]);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let frame = frames.round() as u64;
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use midly::{
        Format, Header, MetaMessage, MidiMessage, PitchBend, Smf, Timing, TrackEvent,
        TrackEventKind,
        num::{u4, u7},
    };

    use super::*;

    fn midi_file(events: Vec<TrackEvent<'static>>) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("test.mid");
        let mut smf = Smf::new(Header::new(
            Format::SingleTrack,
            Timing::Metrical(480.into()),
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

    fn note_on(channel: u8) -> TrackEvent<'static> {
        note_on_with_delta(channel, 0)
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

    fn note_off(channel: u8) -> TrackEvent<'static> {
        note_off_with_delta(channel, 0)
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

    fn pitch_bend(channel: u8, value: i16) -> TrackEvent<'static> {
        pitch_bend_with_delta(channel, value, 0)
    }

    fn pitch_bend_with_delta(channel: u8, value: i16, delta: u32) -> TrackEvent<'static> {
        midi_event_with_delta(
            channel,
            MidiMessage::PitchBend {
                bend: PitchBend::from_int(value),
            },
            delta,
        )
    }

    fn end_of_track() -> TrackEvent<'static> {
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        }
    }

    #[test]
    fn pitch_bend_conversion_uses_the_asymmetric_midi_center() {
        assert!((pitch_bend_value(-8192) + 1.0).abs() < f32::EPSILON);
        assert!(pitch_bend_value(0).abs() < f32::EPSILON);
        assert!((pitch_bend_value(8191) - 1.0).abs() < f32::EPSILON);
        assert!((-1.0..=1.0).contains(&pitch_bend_value(4096)));
    }

    #[test]
    fn event_priority_orders_note_off_before_controls_and_note_on() {
        let mut events = [
            ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
            ProcessEventKind::Aftertouch { value: 0.5 },
            ProcessEventKind::NoteOff { note_id: 1 },
            ProcessEventKind::PitchBend { value: 0.5 },
        ];
        events.sort_by_key(|event| event.priority());
        assert_eq!(events[0].priority(), 0);
        assert_eq!(events[1].priority(), 2);
        assert_eq!(events[2].priority(), 4);
        assert_eq!(events[3].priority(), 5);
    }

    #[test]
    fn control_only_midi_is_rejected_without_note_events() {
        let (_directory, path) = midi_file(vec![pitch_bend(0, 0), end_of_track()]);
        let Err(diagnostics) = read_midi(&path, 48_000.0) else {
            panic!("control-only MIDI is invalid");
        };
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.message == "MIDI file contains no note events" })
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
            midi_event_with_delta(
                0,
                MidiMessage::NoteOff {
                    key: u7::new(60),
                    vel: u7::new(0),
                },
                480,
            ),
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
            midi_event_with_delta(
                0,
                MidiMessage::NoteOff {
                    key: u7::new(60),
                    vel: u7::new(0),
                },
                480,
            ),
            note_on(1),
            pitch_bend(1, -4096),
            midi_event_with_delta(
                1,
                MidiMessage::NoteOff {
                    key: u7::new(60),
                    vel: u7::new(0),
                },
                480,
            ),
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
            midi_event_with_delta(
                0,
                MidiMessage::NoteOff {
                    key: u7::new(60),
                    vel: u7::new(0),
                },
                480,
            ),
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
    fn same_frame_note_on_and_note_off_are_removed_as_a_zero_length_note() {
        let (_directory, path) = midi_file(vec![
            note_on(0),
            note_off(0),
            note_on_with_delta(0, 480),
            note_off_with_delta(0, 480),
            end_of_track(),
        ]);
        let render = read_midi(&path, 48_000.0).expect("MIDI with a non-zero note is valid");
        assert_eq!(render.events.len(), 2);
        let surviving_note_id = (60_u64 << 48) | 1;
        assert!(render.events.iter().all(|event| match event.kind {
            ProcessEventKind::NoteOn { note_id, .. } | ProcessEventKind::NoteOff { note_id } =>
                note_id == surviving_note_id,
            _ => false,
        }));
    }
}
