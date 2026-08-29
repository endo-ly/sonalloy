use std::collections::{HashMap, VecDeque};
use std::path::Path;

use midly::{
    Format, Header, MidiMessage, Smf, TrackEvent, TrackEventKind,
    num::{u4, u7, u15, u24, u28},
};
use sonalloy_core::{DEFAULT_TEMPO_BPM, Diagnostic, DiagnosticCode, ProcessEventKind};

use crate::midi::parse::{ParsedMidi, RawMidiEventKind, RawMidiTempoChange};
use crate::midi::render::imported_time_signature_changes;
use crate::midi::{
    MOD_WHEEL_CONTROLLER, SUSTAIN_PEDAL_CONTROLLER, denormalize_control, denormalize_pitch_bend,
    normalize_control, normalize_pitch_bend, tempo_to_microseconds_per_beat,
};
use crate::pattern::{
    PATTERN_SCHEMA_VERSION, PatternDefinition, PatternEvent, PatternTempoChange, validate,
};
use midly::Timing;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PatternMidiEventKind {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
    SustainPedal { down: bool },
    PitchBend { value: f32 },
    ModWheel { value: f32 },
    Aftertouch { value: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PatternMidiEvent {
    pub(crate) tick: u64,
    pub(crate) source_index: usize,
    pub(crate) kind: PatternMidiEventKind,
}

impl PatternMidiEventKind {
    pub(crate) const fn priority(self) -> u8 {
        match self {
            Self::SustainPedal { down } => ProcessEventKind::SustainPedal { down }.priority(),
            Self::NoteOff { .. } => ProcessEventKind::NoteOff { note_id: 0 }.priority(),
            Self::PitchBend { value } => ProcessEventKind::PitchBend { value }.priority(),
            Self::ModWheel { value } => ProcessEventKind::ModWheel { value }.priority(),
            Self::Aftertouch { value } => ProcessEventKind::Aftertouch { value }.priority(),
            Self::NoteOn { note, velocity } => ProcessEventKind::NoteOn {
                note_id: 0,
                note_number: note,
                velocity,
            }
            .priority(),
        }
    }
}

struct PendingImportedNote {
    tick: u64,
    track: usize,
    index: usize,
    note: u8,
    velocity: u8,
}

#[derive(Debug)]
struct ImportedPatternEvent {
    tick: u64,
    track: usize,
    index: usize,
    event: PatternEvent,
}

pub(crate) fn midi_events(
    pattern: &PatternDefinition,
) -> Result<Vec<PatternMidiEvent>, Vec<Diagnostic>> {
    let diagnostics = validate(pattern);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let mut events = Vec::with_capacity(pattern.events.len().saturating_mul(2));
    let mut errors = midi_note_overlap_diagnostics(pattern);
    for (source_index, event) in pattern.events.iter().enumerate() {
        match event {
            PatternEvent::Note {
                tick,
                duration_ticks,
                note,
                velocity,
            } => {
                let end_tick = tick
                    .checked_add(*duration_ticks)
                    .expect("validated note duration fits the tick counter");
                events.push(PatternMidiEvent {
                    tick: *tick,
                    source_index,
                    kind: PatternMidiEventKind::NoteOn {
                        note: *note,
                        velocity: *velocity,
                    },
                });
                events.push(PatternMidiEvent {
                    tick: end_tick,
                    source_index,
                    kind: PatternMidiEventKind::NoteOff { note: *note },
                });
            }
            PatternEvent::SustainPedal { tick, down } => events.push(PatternMidiEvent {
                tick: *tick,
                source_index,
                kind: PatternMidiEventKind::SustainPedal { down: *down },
            }),
            PatternEvent::PitchBend { tick, value } => events.push(PatternMidiEvent {
                tick: *tick,
                source_index,
                kind: PatternMidiEventKind::PitchBend { value: *value },
            }),
            PatternEvent::ModWheel { tick, value } => events.push(PatternMidiEvent {
                tick: *tick,
                source_index,
                kind: PatternMidiEventKind::ModWheel { value: *value },
            }),
            PatternEvent::Aftertouch { tick, value } => events.push(PatternMidiEvent {
                tick: *tick,
                source_index,
                kind: PatternMidiEventKind::Aftertouch { value: *value },
            }),
            PatternEvent::ParameterChange { .. } => errors.push(
                Diagnostic::error(
                    DiagnosticCode::MidiError,
                    "Sonalloy parameter changes cannot be represented in Standard MIDI",
                )
                .with_path(format!("events[{source_index}]")),
            ),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    events.sort_by_key(|event| (event.tick, event.kind.priority(), event.source_index));
    Ok(events)
}

fn midi_note_overlap_diagnostics(pattern: &PatternDefinition) -> Vec<Diagnostic> {
    let mut notes = pattern
        .events
        .iter()
        .enumerate()
        .filter_map(|(source_index, event)| {
            let PatternEvent::Note {
                tick,
                duration_ticks,
                note,
                ..
            } = event
            else {
                return None;
            };
            let end_tick = tick
                .checked_add(*duration_ticks)
                .expect("validated note duration fits the tick counter");
            Some((source_index, *note, *tick, end_tick))
        })
        .collect::<Vec<_>>();
    notes.sort_unstable_by_key(|(_, note, tick, end_tick)| (*note, *tick, *end_tick));

    let mut latest_end_by_note = [None; 128];
    let mut diagnostics = Vec::new();
    for (source_index, note, start_tick, end_tick) in notes {
        if let Some((previous_index, previous_end_tick)) = latest_end_by_note[usize::from(note)]
            && start_tick < previous_end_tick
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::MidiError,
                    "notes with the same pitch cannot overlap in Standard MIDI",
                )
                .with_path(format!("events[{source_index}]"))
                .with_detail(format!(
                    "events[{source_index}] overlaps events[{previous_index}] for note {note}"
                )),
            );
        }
        if latest_end_by_note[usize::from(note)]
            .is_none_or(|(_, previous_end_tick)| end_tick > previous_end_tick)
        {
            latest_end_by_note[usize::from(note)] = Some((source_index, end_tick));
        }
    }
    diagnostics
}

pub(crate) fn time_signature_denominator_power(denominator: u8) -> u8 {
    u8::try_from(denominator.trailing_zeros()).expect("u8 denominator has a small exponent")
}

#[allow(clippy::too_many_lines)]
pub(crate) fn import_pattern(
    parsed: ParsedMidi,
    channel: Option<u8>,
) -> Result<(PatternDefinition, Vec<Diagnostic>), Vec<Diagnostic>> {
    let mut diagnostics = parsed.diagnostics;
    let mut available_channels = [false; 16];
    for event in &parsed.events {
        if let RawMidiEventKind::NoteOn { channel, .. } = event.kind {
            available_channels[usize::from(channel)] = true;
        }
    }
    let selected_channel = if let Some(channel) = channel {
        if !available_channels[usize::from(channel)] {
            return Err(vec![
                Diagnostic::error(
                    DiagnosticCode::MidiError,
                    "the selected MIDI channel contains no note events",
                )
                .with_detail(format!("channel {}", usize::from(channel) + 1)),
            ]);
        }
        channel
    } else {
        let channels = available_channels
            .iter()
            .enumerate()
            .filter_map(|(index, present)| present.then_some(index))
            .collect::<Vec<_>>();
        match channels.as_slice() {
            [channel] => u8::try_from(*channel).expect("MIDI channel fits u8"),
            [] => {
                return Err(vec![Diagnostic::error(
                    DiagnosticCode::MidiError,
                    "MIDI file contains no note events",
                )]);
            }
            _ => {
                let labels = channels
                    .iter()
                    .map(|channel| (channel + 1).to_string())
                    .collect::<Vec<_>>();
                return Err(vec![
                    Diagnostic::error(
                        DiagnosticCode::MidiError,
                        "MIDI file contains notes on multiple channels; specify --channel",
                    )
                    .with_detail(format!("available channels: {}", labels.join(", "))),
                ]);
            }
        }
    };

    let mut pending_notes: HashMap<u8, VecDeque<PendingImportedNote>> = HashMap::new();
    let mut imported = Vec::new();
    let mut note_end_tick = 0_u64;
    for raw in &parsed.events {
        let raw_channel = raw_channel(raw.kind);
        if raw_channel != selected_channel {
            continue;
        }
        match raw.kind {
            RawMidiEventKind::NoteOn { note, velocity, .. } => pending_notes
                .entry(note)
                .or_default()
                .push_back(PendingImportedNote {
                    tick: raw.tick,
                    track: raw.track,
                    index: raw.index,
                    note,
                    velocity,
                }),
            RawMidiEventKind::NoteOff { note, .. } => {
                let Some(pending) = pending_notes.get_mut(&note).and_then(VecDeque::pop_front)
                else {
                    diagnostics.push(
                        Diagnostic::warning(
                            DiagnosticCode::MidiError,
                            "Note Off without a matching Note On was ignored",
                        )
                        .with_detail(format!(
                            "channel {}, note {note}",
                            usize::from(selected_channel) + 1
                        )),
                    );
                    continue;
                };
                if pending.tick == raw.tick {
                    continue;
                }
                let duration_ticks = raw.tick.checked_sub(pending.tick).ok_or_else(|| {
                    vec![Diagnostic::error(
                        DiagnosticCode::MidiError,
                        "MIDI note duration underflow",
                    )]
                })?;
                note_end_tick = note_end_tick.max(raw.tick);
                imported.push(ImportedPatternEvent {
                    tick: pending.tick,
                    track: pending.track,
                    index: pending.index,
                    event: PatternEvent::Note {
                        tick: pending.tick,
                        duration_ticks,
                        note: pending.note,
                        velocity: pending.velocity,
                    },
                });
            }
            RawMidiEventKind::SustainPedal { down, .. } => imported.push(ImportedPatternEvent {
                tick: raw.tick,
                track: raw.track,
                index: raw.index,
                event: PatternEvent::SustainPedal {
                    tick: raw.tick,
                    down,
                },
            }),
            RawMidiEventKind::PitchBend { value, .. } => imported.push(ImportedPatternEvent {
                tick: raw.tick,
                track: raw.track,
                index: raw.index,
                event: PatternEvent::PitchBend {
                    tick: raw.tick,
                    value: normalize_pitch_bend(value),
                },
            }),
            RawMidiEventKind::ModWheel { value, .. } => imported.push(ImportedPatternEvent {
                tick: raw.tick,
                track: raw.track,
                index: raw.index,
                event: PatternEvent::ModWheel {
                    tick: raw.tick,
                    value: normalize_control(value),
                },
            }),
            RawMidiEventKind::Aftertouch { value, .. } => imported.push(ImportedPatternEvent {
                tick: raw.tick,
                track: raw.track,
                index: raw.index,
                event: PatternEvent::Aftertouch {
                    tick: raw.tick,
                    value: normalize_control(value),
                },
            }),
        }
    }
    let unmatched = pending_notes.values().map(VecDeque::len).sum::<usize>();
    if unmatched > 0 {
        return Err(vec![
            Diagnostic::error(
                DiagnosticCode::MidiError,
                "MIDI file contains Note On events without matching Note Off events",
            )
            .with_detail(format!("{unmatched} unmatched note(s)")),
        ]);
    }
    if imported.iter().all(|event| !event.event.is_note()) {
        return Err(vec![Diagnostic::error(
            DiagnosticCode::MidiError,
            "MIDI file contains no non-zero-length note events on the selected channel",
        )]);
    }

    let mut length_ticks = parsed.end_tick.max(note_end_tick);
    length_ticks = length_ticks.max(imported.iter().map(|event| event.tick).max().unwrap_or(0));
    let mut tempo_changes = imported_tempo_changes(&parsed.tempo_changes);
    let mut time_signature_changes =
        imported_time_signature_changes(&parsed.time_signature_changes)?;
    for change in &tempo_changes {
        if change.tick >= length_ticks {
            length_ticks = change.tick.checked_add(1).ok_or_else(|| {
                vec![Diagnostic::error(
                    DiagnosticCode::MidiError,
                    "MIDI pattern length overflows the tick counter",
                )]
            })?;
        }
    }
    for change in &time_signature_changes {
        if change.tick >= length_ticks {
            length_ticks = change.tick.checked_add(1).ok_or_else(|| {
                vec![Diagnostic::error(
                    DiagnosticCode::MidiError,
                    "MIDI pattern length overflows the tick counter",
                )]
            })?;
        }
    }
    if length_ticks == 0 {
        return Err(vec![Diagnostic::error(
            DiagnosticCode::MidiError,
            "MIDI pattern length must be greater than zero",
        )]);
    }
    tempo_changes.sort_by_key(|change| change.tick);
    time_signature_changes.sort_by_key(|change| change.tick);
    imported.sort_by_key(|event| (event.tick, event.track, event.index));
    let pattern = PatternDefinition {
        schema_version: PATTERN_SCHEMA_VERSION,
        name: None,
        ticks_per_beat: parsed.ticks_per_beat,
        length_ticks,
        tempo_changes,
        time_signature_changes,
        events: imported.into_iter().map(|event| event.event).collect(),
    };
    let pattern_diagnostics = crate::pattern::validate(&pattern);
    if !pattern_diagnostics.is_empty() {
        return Err(pattern_diagnostics);
    }
    Ok((pattern, std::mem::take(&mut diagnostics)))
}

fn raw_channel(kind: RawMidiEventKind) -> u8 {
    match kind {
        RawMidiEventKind::NoteOn { channel, .. }
        | RawMidiEventKind::NoteOff { channel, .. }
        | RawMidiEventKind::SustainPedal { channel, .. }
        | RawMidiEventKind::PitchBend { channel, .. }
        | RawMidiEventKind::ModWheel { channel, .. }
        | RawMidiEventKind::Aftertouch { channel, .. } => channel,
    }
}

fn imported_tempo_changes(changes: &[RawMidiTempoChange]) -> Vec<PatternTempoChange> {
    let mut result = vec![PatternTempoChange {
        tick: 0,
        bpm: DEFAULT_TEMPO_BPM,
    }];
    for change in changes {
        let bpm = 60_000_000.0 / f64::from(change.microseconds_per_beat);
        if let Some(previous) = result
            .last_mut()
            .filter(|previous| previous.tick == change.tick)
        {
            previous.bpm = bpm;
        } else {
            result.push(PatternTempoChange {
                tick: change.tick,
                bpm,
            });
        }
    }
    result.sort_by_key(|change| change.tick);
    result
}

pub(crate) fn export_pattern(
    path: &Path,
    pattern: &PatternDefinition,
    channel: u8,
) -> Result<(), Vec<Diagnostic>> {
    let pattern_events = midi_events(pattern)?;
    let mut events = Vec::with_capacity(
        pattern_events
            .len()
            .saturating_add(pattern.tempo_changes.len())
            .saturating_add(pattern.time_signature_changes.len()),
    );
    for (index, change) in pattern.tempo_changes.iter().enumerate() {
        let Some(microseconds_per_beat) = tempo_to_microseconds_per_beat(change.bpm) else {
            return Err(vec![
                Diagnostic::error(
                    DiagnosticCode::MidiError,
                    "tempo cannot be represented in Standard MIDI",
                )
                .with_path(format!("tempo_changes[{index}].bpm")),
            ]);
        };
        events.push(ExportEvent {
            tick: change.tick,
            priority: 0,
            source_index: index,
            kind: ExportEventKind::Tempo(microseconds_per_beat),
        });
    }
    for (index, change) in pattern.time_signature_changes.iter().enumerate() {
        events.push(ExportEvent {
            tick: change.tick,
            priority: 0,
            source_index: index,
            kind: ExportEventKind::TimeSignature {
                numerator: change.numerator,
                denominator_power: time_signature_denominator_power(change.denominator),
            },
        });
    }
    for event in pattern_events {
        events.push(ExportEvent {
            tick: event.tick,
            priority: event.kind.priority().saturating_add(1),
            source_index: event.source_index,
            kind: ExportEventKind::Midi(event.kind),
        });
    }
    events.sort_by_key(|event| (event.tick, event.priority, event.source_index));

    let mut track = Vec::with_capacity(events.len().saturating_add(1));
    let mut previous_tick = 0_u64;
    for event in events {
        let delta = event.tick.checked_sub(previous_tick).ok_or_else(|| {
            vec![Diagnostic::error(
                DiagnosticCode::MidiError,
                "MIDI events are not in ascending tick order",
            )]
        })?;
        track.push(TrackEvent {
            delta: u28_value(delta)?,
            kind: export_event_kind(event.kind, channel),
        });
        previous_tick = event.tick;
    }
    let end_delta = pattern
        .length_ticks
        .checked_sub(previous_tick)
        .ok_or_else(|| {
            vec![Diagnostic::error(
                DiagnosticCode::MidiError,
                "MIDI event lies beyond pattern length",
            )]
        })?;
    track.push(TrackEvent {
        delta: u28_value(end_delta)?,
        kind: TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
    });
    let mut smf = Smf::new(Header::new(
        Format::SingleTrack,
        Timing::Metrical(u15::new(pattern.ticks_per_beat)),
    ));
    smf.tracks.push(track);
    smf.save(path).map_err(|error| {
        vec![
            Diagnostic::error(DiagnosticCode::MidiError, "could not write MIDI output")
                .with_path(path.to_string_lossy())
                .with_detail(error.to_string()),
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::default_pattern;

    #[test]
    fn same_tick_midi_events_use_core_priority() {
        let pattern = PatternDefinition {
            length_ticks: 480,
            events: vec![
                PatternEvent::Note {
                    tick: 0,
                    duration_ticks: 240,
                    note: 60,
                    velocity: 100,
                },
                PatternEvent::SustainPedal {
                    tick: 0,
                    down: true,
                },
            ],
            ..default_pattern()
        };

        let events = midi_events(&pattern).expect("valid MIDI events");
        assert!(matches!(
            events[0].kind,
            PatternMidiEventKind::SustainPedal { down: true }
        ));
        assert!(matches!(
            events[1].kind,
            PatternMidiEventKind::NoteOn { .. }
        ));
    }

    #[test]
    fn midi_events_reject_overlapping_notes_with_the_same_pitch() {
        let pattern = PatternDefinition {
            length_ticks: 960,
            events: vec![
                PatternEvent::Note {
                    tick: 0,
                    duration_ticks: 720,
                    note: 60,
                    velocity: 100,
                },
                PatternEvent::Note {
                    tick: 480,
                    duration_ticks: 240,
                    note: 60,
                    velocity: 80,
                },
            ],
            ..default_pattern()
        };

        let diagnostics = midi_events(&pattern).expect_err("overlapping notes are not MIDI-safe");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].message,
            "notes with the same pitch cannot overlap in Standard MIDI"
        );
        assert_eq!(diagnostics[0].path.as_deref(), Some("events[1]"));
    }
}

#[derive(Debug, Clone, Copy)]
struct ExportEvent {
    tick: u64,
    priority: u8,
    source_index: usize,
    kind: ExportEventKind,
}

#[derive(Debug, Clone, Copy)]
enum ExportEventKind {
    Tempo(u32),
    TimeSignature {
        numerator: u8,
        denominator_power: u8,
    },
    Midi(PatternMidiEventKind),
}

fn u28_value(value: u64) -> Result<u28, Vec<Diagnostic>> {
    if value > 0x0fff_ffff {
        return Err(vec![Diagnostic::error(
            DiagnosticCode::MidiError,
            "MIDI delta tick exceeds the Standard MIDI variable-length range",
        )]);
    }
    #[allow(clippy::cast_possible_truncation)]
    Ok(u28::new(value as u32))
}

fn export_event_kind(kind: ExportEventKind, channel: u8) -> TrackEventKind<'static> {
    let channel = u4::new(channel);
    match kind {
        ExportEventKind::Tempo(microseconds_per_beat) => {
            TrackEventKind::Meta(midly::MetaMessage::Tempo(u24::new(microseconds_per_beat)))
        }
        ExportEventKind::TimeSignature {
            numerator,
            denominator_power,
        } => TrackEventKind::Meta(midly::MetaMessage::TimeSignature(
            numerator,
            denominator_power,
            24,
            8,
        )),
        ExportEventKind::Midi(event) => TrackEventKind::Midi {
            channel,
            message: match event {
                PatternMidiEventKind::NoteOn { note, velocity } => MidiMessage::NoteOn {
                    key: u7::new(note),
                    vel: u7::new(velocity),
                },
                PatternMidiEventKind::NoteOff { note } => MidiMessage::NoteOff {
                    key: u7::new(note),
                    vel: u7::new(0),
                },
                PatternMidiEventKind::SustainPedal { down } => MidiMessage::Controller {
                    controller: u7::new(SUSTAIN_PEDAL_CONTROLLER),
                    value: u7::new(if down { 127 } else { 0 }),
                },
                PatternMidiEventKind::PitchBend { value } => MidiMessage::PitchBend {
                    bend: midly::PitchBend::from_int(denormalize_pitch_bend(value)),
                },
                PatternMidiEventKind::ModWheel { value } => MidiMessage::Controller {
                    controller: u7::new(MOD_WHEEL_CONTROLLER),
                    value: u7::new(denormalize_control(value)),
                },
                PatternMidiEventKind::Aftertouch { value } => MidiMessage::ChannelAftertouch {
                    vel: u7::new(denormalize_control(value)),
                },
            },
        },
    }
}
