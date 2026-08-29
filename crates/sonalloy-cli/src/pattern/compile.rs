#[allow(clippy::wildcard_imports)]
use super::*;
use crate::musical_time::{
    MusicalTimeError, TempoPoint, TimeSignaturePoint, build_musical_time_map, tick_to_frame,
};
use sonalloy_core::{
    CompiledInstrument, Diagnostic, MusicalTimeMap, ProcessEventKind, ScheduledEvent,
};

#[derive(Debug, Clone)]
pub(crate) struct CompiledPattern {
    pub(crate) events: Vec<ScheduledEvent>,
    pub(crate) musical_time_map: MusicalTimeMap,
    pub(crate) length_frames: u64,
    pub(crate) one_shot_duration_frames: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PendingPatternEvent {
    pub(crate) absolute_frame: u64,
    pub(crate) original_tick: u64,
    pub(crate) source_index: usize,
    pub(crate) kind: ProcessEventKind,
}

struct PatternTimeContext<'a> {
    pattern: &'a PatternDefinition,
    tempo_changes: &'a [TempoPoint],
    sample_rate: f64,
}

#[allow(clippy::too_many_lines)]
pub(crate) fn compile(
    pattern: &PatternDefinition,
    instrument: &CompiledInstrument,
    sample_rate: f64,
) -> Result<CompiledPattern, Vec<Diagnostic>> {
    let diagnostics = validate(pattern);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let tempo_changes = tempo_points(pattern);
    let mut diagnostics = Vec::new();
    let length_frames = match tick_to_frame(
        pattern.length_ticks,
        pattern.ticks_per_beat,
        &tempo_changes,
        sample_rate,
    ) {
        Ok(frame) if frame > 0 => frame,
        Ok(_) => {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "pattern length must occupy at least one frame",
                )
                .with_path("length_ticks"),
            );
            0
        }
        Err(error) => {
            diagnostics.push(time_error(error, "length_ticks"));
            0
        }
    };
    let time_signature_changes = time_signature_points(pattern);
    let musical_time_map = match build_musical_time_map(
        pattern.ticks_per_beat,
        &tempo_changes,
        &time_signature_changes,
        sample_rate,
    ) {
        Ok(map) => Some(map),
        Err(error) => {
            diagnostics.push(time_error(error, "tempo_changes"));
            None
        }
    };
    let time = PatternTimeContext {
        pattern,
        tempo_changes: &tempo_changes,
        sample_rate,
    };

    let mut pending = Vec::with_capacity(pattern.events.len().saturating_mul(2));
    let mut note_serial = 0_u32;
    for (source_index, event) in pattern.events.iter().enumerate() {
        match event {
            PatternEvent::Note {
                tick,
                duration_ticks,
                note,
                velocity,
            } => {
                let Some(note_serial_next) = note_serial.checked_add(1) else {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "pattern contains more notes than the note identity capacity",
                        )
                        .with_path(format!("events[{source_index}]")),
                    );
                    continue;
                };
                let note_id = loop_note_id(0, note_serial);
                note_serial = note_serial_next;
                let Some(end_tick) = tick.checked_add(*duration_ticks) else {
                    continue;
                };
                let start_frame =
                    match tick_to_frame(*tick, pattern.ticks_per_beat, &tempo_changes, sample_rate)
                    {
                        Ok(frame) => frame,
                        Err(error) => {
                            diagnostics
                                .push(time_error(error, format!("events[{source_index}].tick")));
                            continue;
                        }
                    };
                let end_frame = match tick_to_frame(
                    end_tick,
                    pattern.ticks_per_beat,
                    &tempo_changes,
                    sample_rate,
                ) {
                    Ok(frame) => frame,
                    Err(error) => {
                        diagnostics.push(time_error(
                            error,
                            format!("events[{source_index}].duration_ticks"),
                        ));
                        continue;
                    }
                };
                if start_frame == end_frame {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "note start and end must map to different frames",
                        )
                        .with_path(format!("events[{source_index}].duration_ticks")),
                    );
                    continue;
                }
                pending.push(PendingPatternEvent {
                    absolute_frame: start_frame,
                    original_tick: *tick,
                    source_index,
                    kind: ProcessEventKind::NoteOn {
                        note_id,
                        note_number: *note,
                        velocity: *velocity,
                    },
                });
                pending.push(PendingPatternEvent {
                    absolute_frame: end_frame,
                    original_tick: end_tick,
                    source_index,
                    kind: ProcessEventKind::NoteOff { note_id },
                });
            }
            PatternEvent::SustainPedal { tick, down } => {
                push_control_event(
                    &mut pending,
                    *tick,
                    source_index,
                    &time,
                    ProcessEventKind::SustainPedal { down: *down },
                    &mut diagnostics,
                );
            }
            PatternEvent::PitchBend { tick, value } => {
                push_control_event(
                    &mut pending,
                    *tick,
                    source_index,
                    &time,
                    ProcessEventKind::PitchBend { value: *value },
                    &mut diagnostics,
                );
            }
            PatternEvent::ModWheel { tick, value } => {
                push_control_event(
                    &mut pending,
                    *tick,
                    source_index,
                    &time,
                    ProcessEventKind::ModWheel { value: *value },
                    &mut diagnostics,
                );
            }
            PatternEvent::Aftertouch { tick, value } => {
                push_control_event(
                    &mut pending,
                    *tick,
                    source_index,
                    &time,
                    ProcessEventKind::Aftertouch { value: *value },
                    &mut diagnostics,
                );
            }
            PatternEvent::ParameterChange {
                tick,
                parameter,
                native_value,
            } => {
                let Some(handle) = instrument.parameter_handle(parameter) else {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ParameterNotFound,
                            "parameter id is not present in the compiled catalog",
                        )
                        .with_path(format!("events[{source_index}].parameter")),
                    );
                    continue;
                };
                let Some(descriptor) = instrument.parameter_descriptor(handle) else {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ParameterNotFound,
                            "parameter descriptor is not present in the compiled catalog",
                        )
                        .with_path(format!("events[{source_index}].parameter")),
                    );
                    continue;
                };
                let normalized = match descriptor.normalize(*native_value) {
                    Ok(normalized) => normalized,
                    Err(error) => {
                        diagnostics.push(
                            Diagnostic::error(DiagnosticCode::ValueOutOfRange, error.to_string())
                                .with_path(format!("events[{source_index}].native_value")),
                        );
                        continue;
                    }
                };
                push_control_event(
                    &mut pending,
                    *tick,
                    source_index,
                    &time,
                    ProcessEventKind::ParameterChange {
                        parameter: handle,
                        normalized,
                    },
                    &mut diagnostics,
                );
            }
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    pending.sort_by_key(|event| {
        (
            event.absolute_frame,
            event.original_tick,
            event.kind.priority(),
            event.source_index,
        )
    });
    let last_event_frame = pending
        .iter()
        .map(|event| event.absolute_frame)
        .max()
        .unwrap_or(0);
    let Some(one_shot_duration_frames) = last_event_frame
        .checked_add(1)
        .map(|frame| frame.max(length_frames))
    else {
        return Err(vec![Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            "pattern duration overflows the frame counter",
        )]);
    };
    let Some(musical_time_map) = musical_time_map else {
        return Err(vec![Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            "pattern tempo map is invalid",
        )]);
    };
    Ok(CompiledPattern {
        events: pending
            .into_iter()
            .map(|event| ScheduledEvent {
                absolute_frame: event.absolute_frame,
                kind: event.kind,
            })
            .collect(),
        musical_time_map,
        length_frames,
        one_shot_duration_frames,
    })
}

fn push_control_event(
    pending: &mut Vec<PendingPatternEvent>,
    tick: u64,
    source_index: usize,
    time: &PatternTimeContext<'_>,
    kind: ProcessEventKind,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let frame = match tick_to_frame(
        tick,
        time.pattern.ticks_per_beat,
        time.tempo_changes,
        time.sample_rate,
    ) {
        Ok(frame) => frame,
        Err(error) => {
            diagnostics.push(time_error(error, format!("events[{source_index}].tick")));
            return;
        }
    };
    pending.push(PendingPatternEvent {
        absolute_frame: frame,
        original_tick: tick,
        source_index,
        kind,
    });
}

pub(crate) fn tempo_points(pattern: &PatternDefinition) -> Vec<TempoPoint> {
    pattern
        .tempo_changes
        .iter()
        .map(|change| TempoPoint {
            tick: change.tick,
            bpm: change.bpm,
        })
        .collect()
}

pub(crate) fn time_signature_points(pattern: &PatternDefinition) -> Vec<TimeSignaturePoint> {
    pattern
        .time_signature_changes
        .iter()
        .map(|change| TimeSignaturePoint {
            tick: change.tick,
            numerator: u16::from(change.numerator),
            denominator: u16::from(change.denominator),
        })
        .collect()
}

pub(crate) fn loop_note_id(iteration: u32, note_serial: u32) -> u64 {
    (u64::from(iteration) << 32) | u64::from(note_serial)
}

pub(super) fn time_error(error: MusicalTimeError, path: impl Into<String>) -> Diagnostic {
    Diagnostic::error(DiagnosticCode::ValueOutOfRange, error.to_string()).with_path(path)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sonalloy_core::{CompileContext, ProcessSpec, compile_instrument};

    use super::*;

    #[test]
    fn pattern_compile_preserves_musical_positions_across_sample_rates() {
        let definition = crate::command::instrument::default_definition();
        let pattern = default_pattern();
        for (sample_rate_hz, sample_rate) in [
            (44_100_u32, 44_100.0),
            (48_000, 48_000.0),
            (96_000, 96_000.0),
        ] {
            let process_spec = ProcessSpec::new(sample_rate, 257, 0, 2).expect("process spec");
            let compiled_instrument = compile_instrument(
                &definition,
                &CompileContext {
                    definition_base_dir: PathBuf::from("."),
                    process_spec,
                },
            )
            .instrument
            .expect("instrument compiles");

            let compiled =
                compile(&pattern, &compiled_instrument, sample_rate).expect("pattern compiles");

            assert_eq!(compiled.length_frames, u64::from(sample_rate_hz) * 2);
            assert!(matches!(
                compiled.events.as_slice(),
                [
                    ScheduledEvent {
                        absolute_frame: 0,
                        kind: ProcessEventKind::NoteOn { .. }
                    },
                    ScheduledEvent {
                        absolute_frame,
                        kind: ProcessEventKind::NoteOff { .. }
                    }
                ] if *absolute_frame == u64::from(sample_rate_hz) / 2
            ));
        }
    }
}
