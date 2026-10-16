use std::collections::{HashMap, VecDeque};

use midly::{
    MidiMessage, TrackEvent, TrackEventKind,
    num::{u4, u7, u24, u28},
};
use sonalloy_core::{DEFAULT_TEMPO_BPM, Diagnostic, DiagnosticCode, ProcessEventKind};

use crate::midi::parse::{ParsedMidi, RawMidiEventKind, RawMidiTempoChange};
use crate::midi::render::imported_time_signature_changes;
use crate::midi::{
    MOD_WHEEL_CONTROLLER, SUSTAIN_PEDAL_CONTROLLER, denormalize_control, denormalize_pitch_bend,
    normalize_control, normalize_pitch_bend, tempo_to_microseconds_per_beat,
};
use crate::performance::events::{MusicalEvent, PerformanceEvents, TempoChange, validate};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PerformanceMidiEventKind {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
    SustainPedal { down: bool },
    PitchBend { value: f32 },
    ModWheel { value: f32 },
    Aftertouch { value: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PerformanceMidiEvent {
    pub(crate) tick: u64,
    pub(crate) source_index: usize,
    pub(crate) kind: PerformanceMidiEventKind,
}

impl PerformanceMidiEventKind {
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
struct ImportedMusicalEvent {
    tick: u64,
    track: usize,
    index: usize,
    event: MusicalEvent,
}

pub(crate) fn midi_events(
    pattern: &PerformanceEvents,
) -> Result<Vec<PerformanceMidiEvent>, Vec<Diagnostic>> {
    let diagnostics = validate(pattern);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let mut events = Vec::with_capacity(pattern.events.len().saturating_mul(2));
    let mut errors = Vec::new();
    let mut parameter_event_count = 0;
    for (source_index, event) in pattern.events.iter().enumerate() {
        match event {
            MusicalEvent::Note {
                tick,
                duration_ticks,
                note,
                velocity,
            } => {
                let end_tick = tick
                    .checked_add(*duration_ticks)
                    .expect("validated note duration fits the tick counter");
                events.push(PerformanceMidiEvent {
                    tick: *tick,
                    source_index,
                    kind: PerformanceMidiEventKind::NoteOn {
                        note: *note,
                        velocity: *velocity,
                    },
                });
                events.push(PerformanceMidiEvent {
                    tick: end_tick,
                    source_index,
                    kind: PerformanceMidiEventKind::NoteOff { note: *note },
                });
            }
            MusicalEvent::SustainPedal { tick, down } => events.push(PerformanceMidiEvent {
                tick: *tick,
                source_index,
                kind: PerformanceMidiEventKind::SustainPedal { down: *down },
            }),
            MusicalEvent::PitchBend { tick, value } => events.push(PerformanceMidiEvent {
                tick: *tick,
                source_index,
                kind: PerformanceMidiEventKind::PitchBend { value: *value },
            }),
            MusicalEvent::ModWheel { tick, value } => events.push(PerformanceMidiEvent {
                tick: *tick,
                source_index,
                kind: PerformanceMidiEventKind::ModWheel { value: *value },
            }),
            MusicalEvent::Aftertouch { tick, value } => events.push(PerformanceMidiEvent {
                tick: *tick,
                source_index,
                kind: PerformanceMidiEventKind::Aftertouch { value: *value },
            }),
            MusicalEvent::ParameterChange { .. } | MusicalEvent::ParameterRamp { .. } => {
                parameter_event_count += 1;
            }
        }
    }
    if parameter_event_count > 0 {
        errors.push(
            Diagnostic::error(
                DiagnosticCode::MidiError,
                format!("{parameter_event_count} Sonalloy parameter events cannot be represented in Standard MIDI"),
            )
            .with_path("events"),
        );
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    events.sort_by_key(|event| (event.tick, event.kind.priority(), event.source_index));
    Ok(events)
}

pub(crate) fn time_signature_denominator_power(denominator: u8) -> u8 {
    u8::try_from(denominator.trailing_zeros()).expect("u8 denominator has a small exponent")
}

#[allow(clippy::too_many_lines)]
pub(crate) fn import_performance(
    parsed: ParsedMidi,
    channel: Option<u8>,
) -> Result<(PerformanceEvents, Vec<Diagnostic>), Vec<Diagnostic>> {
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
                imported.push(ImportedMusicalEvent {
                    tick: pending.tick,
                    track: pending.track,
                    index: pending.index,
                    event: MusicalEvent::Note {
                        tick: pending.tick,
                        duration_ticks,
                        note: pending.note,
                        velocity: pending.velocity,
                    },
                });
            }
            RawMidiEventKind::SustainPedal { down, .. } => imported.push(ImportedMusicalEvent {
                tick: raw.tick,
                track: raw.track,
                index: raw.index,
                event: MusicalEvent::SustainPedal {
                    tick: raw.tick,
                    down,
                },
            }),
            RawMidiEventKind::PitchBend { value, .. } => imported.push(ImportedMusicalEvent {
                tick: raw.tick,
                track: raw.track,
                index: raw.index,
                event: MusicalEvent::PitchBend {
                    tick: raw.tick,
                    value: normalize_pitch_bend(value),
                },
            }),
            RawMidiEventKind::ModWheel { value, .. } => imported.push(ImportedMusicalEvent {
                tick: raw.tick,
                track: raw.track,
                index: raw.index,
                event: MusicalEvent::ModWheel {
                    tick: raw.tick,
                    value: normalize_control(value),
                },
            }),
            RawMidiEventKind::Aftertouch { value, .. } => imported.push(ImportedMusicalEvent {
                tick: raw.tick,
                track: raw.track,
                index: raw.index,
                event: MusicalEvent::Aftertouch {
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
    let pattern = PerformanceEvents {
        name: None,
        ticks_per_beat: parsed.ticks_per_beat,
        length_ticks,
        tempo_changes,
        time_signature_changes,
        events: imported.into_iter().map(|event| event.event).collect(),
    };
    let pattern_diagnostics = crate::performance::events::validate(&pattern);
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

fn imported_tempo_changes(changes: &[RawMidiTempoChange]) -> Vec<TempoChange> {
    let mut result = vec![TempoChange {
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
            result.push(TempoChange {
                tick: change.tick,
                bpm,
            });
        }
    }
    result.sort_by_key(|change| change.tick);
    result
}

pub(crate) fn conductor_events(
    pattern: &PerformanceEvents,
) -> Result<Vec<ExportEvent>, Vec<Diagnostic>> {
    let mut events = Vec::with_capacity(
        pattern
            .tempo_changes
            .len()
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
    Ok(events)
}

pub(crate) fn midi_export_events(pattern_events: &[PerformanceMidiEvent]) -> Vec<ExportEvent> {
    pattern_events
        .iter()
        .map(|event| ExportEvent {
            tick: event.tick,
            priority: event.kind.priority().saturating_add(1),
            source_index: event.source_index,
            kind: ExportEventKind::Midi(event.kind),
        })
        .collect()
}

pub(crate) fn build_track(
    mut events: Vec<ExportEvent>,
    channel: u8,
    end_tick: u64,
    track_name: Option<&[u8]>,
) -> Result<Vec<TrackEvent<'_>>, Vec<Diagnostic>> {
    events.sort_by_key(|event| (event.tick, event.priority, event.source_index));
    let mut track = Vec::with_capacity(events.len().saturating_add(2));
    if let Some(track_name) = track_name {
        track.push(TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(midly::MetaMessage::TrackName(track_name)),
        });
    }
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
    let end_delta = end_tick.checked_sub(previous_tick).ok_or_else(|| {
        vec![Diagnostic::error(
            DiagnosticCode::MidiError,
            "MIDI event lies beyond the requested track length",
        )]
    })?;
    track.push(TrackEvent {
        delta: u28_value(end_delta)?,
        kind: TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
    });
    Ok(track)
}

#[cfg(test)]
mod tests {
    use super::{MusicalEvent, PerformanceEvents, PerformanceMidiEventKind, midi_events};
    use crate::performance::events::default_pattern;

    #[test]
    fn same_tick_midi_events_use_core_priority() {
        let pattern = PerformanceEvents {
            length_ticks: 480,
            events: vec![
                MusicalEvent::Note {
                    tick: 0,
                    duration_ticks: 240,
                    note: 60,
                    velocity: 100,
                },
                MusicalEvent::SustainPedal {
                    tick: 0,
                    down: true,
                },
            ],
            ..default_pattern()
        };

        let events = midi_events(&pattern).expect("valid MIDI events");
        assert!(matches!(
            events[0].kind,
            PerformanceMidiEventKind::SustainPedal { down: true }
        ));
        assert!(matches!(
            events[1].kind,
            PerformanceMidiEventKind::NoteOn { .. }
        ));
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ExportEvent {
    pub(crate) tick: u64,
    pub(crate) priority: u8,
    pub(crate) source_index: usize,
    pub(crate) kind: ExportEventKind,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ExportEventKind {
    Tempo(u32),
    TimeSignature {
        numerator: u8,
        denominator_power: u8,
    },
    Midi(PerformanceMidiEventKind),
}

pub(crate) fn u28_value(value: u64) -> Result<u28, Vec<Diagnostic>> {
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
                PerformanceMidiEventKind::NoteOn { note, velocity } => MidiMessage::NoteOn {
                    key: u7::new(note),
                    vel: u7::new(velocity),
                },
                PerformanceMidiEventKind::NoteOff { note } => MidiMessage::NoteOff {
                    key: u7::new(note),
                    vel: u7::new(0),
                },
                PerformanceMidiEventKind::SustainPedal { down } => MidiMessage::Controller {
                    controller: u7::new(SUSTAIN_PEDAL_CONTROLLER),
                    value: u7::new(if down { 127 } else { 0 }),
                },
                PerformanceMidiEventKind::PitchBend { value } => MidiMessage::PitchBend {
                    bend: midly::PitchBend::from_int(denormalize_pitch_bend(value)),
                },
                PerformanceMidiEventKind::ModWheel { value } => MidiMessage::Controller {
                    controller: u7::new(MOD_WHEEL_CONTROLLER),
                    value: u7::new(denormalize_control(value)),
                },
                PerformanceMidiEventKind::Aftertouch { value } => MidiMessage::ChannelAftertouch {
                    vel: u7::new(denormalize_control(value)),
                },
            },
        },
    }
}
