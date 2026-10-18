use std::path::Path;

use crate::midi::{MOD_WHEEL_CONTROLLER, SUSTAIN_PEDAL_CONTROLLER};
use midly::{MidiMessage, Smf, Timing, TrackEventKind};
use sonalloy_core::{Diagnostic, DiagnosticCode};

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
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use midly::{
        Format, Header, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
        num::{u4, u7, u15, u24},
    };

    use super::parse_midi;

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
}
