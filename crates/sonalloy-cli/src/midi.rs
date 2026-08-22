use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use midly::{
    Format, Header, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
    num::{u4, u7, u15, u24, u28},
};
use sonalloy_core::{
    DEFAULT_TEMPO_BPM, Diagnostic, DiagnosticCode, ProcessEventKind, ScheduledEvent, TempoMap,
};

use crate::midi_common::{
    MOD_WHEEL_CONTROLLER, SUSTAIN_PEDAL_CONTROLLER, denormalize_control, denormalize_pitch_bend,
    normalize_control, normalize_pitch_bend, note_id, tempo_to_microseconds_per_beat,
};
use crate::musical_time::{TempoPoint, build_tempo_map, tick_to_frame};
use crate::pattern::{
    PATTERN_SCHEMA_VERSION, PatternDefinition, PatternEvent, PatternMidiEventKind,
    PatternTempoChange, PatternTimeSignatureChange, midi_events, time_signature_denominator_power,
};

pub(crate) struct MidiRender {
    pub(crate) events: Vec<ScheduledEvent>,
    pub(crate) duration_frames: u64,
    pub(crate) tempo_map: TempoMap,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum RawMidiEventKind {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8 },
    SustainPedal { channel: u8, down: bool },
    PitchBend { channel: u8, value: i16 },
    ModWheel { channel: u8, value: u8 },
    Aftertouch { channel: u8, value: u8 },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RawMidiEvent {
    pub(crate) tick: u64,
    pub(crate) track: usize,
    pub(crate) index: usize,
    pub(crate) kind: RawMidiEventKind,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RawMidiTempoChange {
    pub(crate) tick: u64,
    pub(crate) microseconds_per_beat: u32,
    pub(crate) track: usize,
    pub(crate) index: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RawMidiTimeSignatureChange {
    pub(crate) tick: u64,
    pub(crate) numerator: u8,
    pub(crate) denominator_power: u8,
    pub(crate) track: usize,
    pub(crate) index: usize,
}

#[derive(Debug)]
pub(crate) struct ParsedMidi {
    pub(crate) ticks_per_beat: u16,
    pub(crate) end_tick: u64,
    pub(crate) events: Vec<RawMidiEvent>,
    pub(crate) tempo_changes: Vec<RawMidiTempoChange>,
    pub(crate) time_signature_changes: Vec<RawMidiTimeSignatureChange>,
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
pub(crate) fn parse_midi(path: &Path) -> Result<ParsedMidi, Vec<Diagnostic>> {
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
        Timing::Metrical(value) => value.as_int(),
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
    let mut events = Vec::new();
    let mut tempo_changes = Vec::new();
    let mut time_signature_changes = Vec::new();
    let mut end_tick = 0_u64;
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
            end_tick = end_tick.max(tick);
            match event.kind {
                TrackEventKind::Midi { channel, message } => {
                    let channel = channel.as_int();
                    match message {
                        MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                            events.push(RawMidiEvent {
                                tick,
                                track: track_index,
                                index: event_index,
                                kind: RawMidiEventKind::NoteOn {
                                    channel,
                                    note: key.as_int(),
                                    velocity: vel.as_int(),
                                },
                            });
                        }
                        MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                            events.push(RawMidiEvent {
                                tick,
                                track: track_index,
                                index: event_index,
                                kind: RawMidiEventKind::NoteOff {
                                    channel,
                                    note: key.as_int(),
                                },
                            });
                        }
                        MidiMessage::PitchBend { bend } => events.push(RawMidiEvent {
                            tick,
                            track: track_index,
                            index: event_index,
                            kind: RawMidiEventKind::PitchBend {
                                channel,
                                value: bend.as_int(),
                            },
                        }),
                        MidiMessage::Controller { controller, value }
                            if controller.as_int() == MOD_WHEEL_CONTROLLER =>
                        {
                            events.push(RawMidiEvent {
                                tick,
                                track: track_index,
                                index: event_index,
                                kind: RawMidiEventKind::ModWheel {
                                    channel,
                                    value: value.as_int(),
                                },
                            });
                        }
                        MidiMessage::Controller { controller, value }
                            if controller.as_int() == SUSTAIN_PEDAL_CONTROLLER =>
                        {
                            events.push(RawMidiEvent {
                                tick,
                                track: track_index,
                                index: event_index,
                                kind: RawMidiEventKind::SustainPedal {
                                    channel,
                                    down: value.as_int() >= 64,
                                },
                            });
                        }
                        MidiMessage::ChannelAftertouch { vel } => {
                            events.push(RawMidiEvent {
                                tick,
                                track: track_index,
                                index: event_index,
                                kind: RawMidiEventKind::Aftertouch {
                                    channel,
                                    value: vel.as_int(),
                                },
                            });
                        }
                        MidiMessage::Aftertouch { .. } => diagnostics.push(
                            Diagnostic::warning(
                                DiagnosticCode::MidiError,
                                "polyphonic aftertouch is not supported and was ignored",
                            )
                            .with_path(format!("track[{track_index}].event[{event_index}]")),
                        ),
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
                    tempo_changes.push(RawMidiTempoChange {
                        tick,
                        microseconds_per_beat,
                        track: track_index,
                        index: event_index,
                    });
                }
                TrackEventKind::Meta(midly::MetaMessage::TimeSignature(
                    numerator,
                    denominator_power,
                    _,
                    _,
                )) => time_signature_changes.push(RawMidiTimeSignatureChange {
                    tick,
                    numerator,
                    denominator_power,
                    track: track_index,
                    index: event_index,
                }),
                TrackEventKind::Meta(_) => {}
                TrackEventKind::SysEx(_) | TrackEventKind::Escape(_) => diagnostics.push(
                    Diagnostic::warning(
                        DiagnosticCode::MidiError,
                        "unsupported MIDI event was ignored",
                    )
                    .with_path(format!("track[{track_index}].event[{event_index}]")),
                ),
            }
        }
    }
    events.sort_by_key(|event| (event.tick, event.track, event.index));
    tempo_changes.sort_by_key(|change| (change.tick, change.track, change.index));
    time_signature_changes.sort_by_key(|change| (change.tick, change.track, change.index));
    Ok(ParsedMidi {
        ticks_per_beat,
        end_tick,
        events,
        tempo_changes,
        time_signature_changes,
        diagnostics,
    })
}

#[allow(clippy::too_many_lines)]
pub(crate) fn read_midi(path: &Path, sample_rate: f64) -> Result<MidiRender, Vec<Diagnostic>> {
    let parsed = parse_midi(path)?;
    let mut diagnostics = parsed.diagnostics;
    let tempo_points = midi_tempo_points(&parsed.tempo_changes);
    let tempo_map =
        build_tempo_map(parsed.ticks_per_beat, &tempo_points, sample_rate).map_err(|error| {
            vec![
                Diagnostic::error(DiagnosticCode::MidiError, "MIDI tempo map is invalid")
                    .with_path(path.to_string_lossy())
                    .with_detail(error.to_string()),
            ]
        })?;

    let mut active_notes: HashMap<(u8, u8), VecDeque<(u64, u64)>> = HashMap::new();
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
                    .push_back((note_id, raw.tick));
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
                let Some((note_id, start_tick)) =
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
                if start_tick == raw.tick {
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
        tempo_map,
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

fn imported_time_signature_changes(
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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
    fn parse_keeps_tempo_and_time_signature_in_tick_domain() {
        let (_directory, path) = midi_file(vec![
            TrackEvent {
                delta: 0.into(),
                kind: TrackEventKind::Meta(midly::MetaMessage::Tempo(u24::new(500_000))),
            },
            TrackEvent {
                delta: 0.into(),
                kind: TrackEventKind::Meta(midly::MetaMessage::TimeSignature(3, 2, 24, 8)),
            },
            note_on_with_delta(0, 0),
            note_off_with_delta(0, 480),
            end_of_track(),
        ]);

        let parsed = parse_midi(&path).expect("valid MIDI");
        assert_eq!(parsed.ticks_per_beat, 480);
        assert_eq!(parsed.tempo_changes[0].tick, 0);
        assert_eq!(parsed.time_signature_changes[0].tick, 0);
        assert_eq!(parsed.end_tick, 480);
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
        assert_eq!(render.tempo_map.changes().len(), 2);
        assert_eq!(render.tempo_map.changes()[1].absolute_frame, 24_000);
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
}
