use std::collections::{HashMap, VecDeque};
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
    kind: ProcessEventKind,
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
                        MidiMessage::Controller { controller, .. } if controller.as_int() == 64 => {
                            diagnostics.push(
                                Diagnostic::warning(
                                    DiagnosticCode::MidiError,
                                    "sustain pedal event ignored by the MVP",
                                )
                                .with_path(format!("track[{track_index}].event[{event_index}]")),
                            );
                        }
                        MidiMessage::Aftertouch { .. }
                        | MidiMessage::Controller { .. }
                        | MidiMessage::ProgramChange { .. }
                        | MidiMessage::ChannelAftertouch { .. }
                        | MidiMessage::PitchBend { .. } => {
                            diagnostics.push(
                                Diagnostic::warning(
                                    DiagnosticCode::MidiError,
                                    "unsupported MIDI event ignored by the MVP",
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
    let mut active_notes: HashMap<(u8, u8), VecDeque<u64>> = HashMap::new();
    let mut serials: HashMap<(u8, u8), u32> = HashMap::new();
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
                active_notes.entry(key).or_default().push_back(note_id);
                converted.push(ConvertedEvent {
                    frame,
                    track: raw.track,
                    index: raw.index,
                    kind: ProcessEventKind::NoteOn {
                        note_id,
                        note_number: note,
                        velocity,
                    },
                });
            }
            RawKind::NoteOff { channel, note } => {
                let key = (channel, note);
                let Some(note_id) = active_notes.get_mut(&key).and_then(VecDeque::pop_front) else {
                    diagnostics.push(
                        Diagnostic::warning(
                            DiagnosticCode::MidiError,
                            "Note Off without a matching Note On was ignored",
                        )
                        .with_detail(format!("channel {channel}, note {note}")),
                    );
                    continue;
                };
                converted.push(ConvertedEvent {
                    frame,
                    track: raw.track,
                    index: raw.index,
                    kind: ProcessEventKind::NoteOff { note_id },
                });
            }
        }
    }
    if converted.is_empty() {
        return Err(vec![
            Diagnostic::error(
                DiagnosticCode::MidiError,
                "MIDI file contains no note events",
            )
            .with_path(path.to_string_lossy()),
        ]);
    }
    converted.sort_by_key(|event| {
        let priority = u8::from(!matches!(event.kind, ProcessEventKind::NoteOff { .. }));
        (event.frame, priority, event.track, event.index)
    });
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
