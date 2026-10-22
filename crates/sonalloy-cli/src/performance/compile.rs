use crate::musical_time::{
    MusicalTimeError, TempoPoint, TimeSignaturePoint, build_musical_time_map, tick_to_frame,
};
use sonalloy_core::{
    CompiledInstrument, Diagnostic, DiagnosticCode, MusicalTimeMap, ProcessEventKind,
    ScheduledEvent,
};

use super::validate;
use crate::score::events::{MusicalEvent, PerformanceEvents};

#[derive(Debug, Clone)]
pub(crate) struct CompiledEvents {
    pub(crate) events: Vec<ScheduledEvent>,
    pub(crate) musical_time_map: MusicalTimeMap,
    pub(crate) length_frames: u64,
    pub(crate) one_shot_duration_frames: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PendingMusicalEvent {
    pub(crate) absolute_frame: u64,
    pub(crate) original_tick: u64,
    pub(crate) source_index: usize,
    pub(crate) kind: ProcessEventKind,
}

struct EventTimeContext<'a> {
    pattern: &'a PerformanceEvents,
    tempo_changes: &'a [TempoPoint],
    sample_rate: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct Timeline {
    map: MusicalTimeMap,
    length_frames: u64,
    tempo_changes: Vec<TempoPoint>,
    sample_rate: f64,
}
impl Timeline {
    pub(crate) fn new(
        pattern: &PerformanceEvents,
        sample_rate: f64,
    ) -> Result<Self, Vec<Diagnostic>> {
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

        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        Ok(Self {
            map: musical_time_map.expect("validated time map"),
            length_frames,
            tempo_changes,
            sample_rate,
        })
    }
}
pub(crate) fn compile(
    pattern: &PerformanceEvents,
    instrument: &CompiledInstrument,
    sample_rate: f64,
) -> Result<CompiledEvents, Vec<Diagnostic>> {
    compile_on_timeline(pattern, instrument, &Timeline::new(pattern, sample_rate)?)
}
#[allow(clippy::too_many_lines)]
pub(crate) fn compile_on_timeline(
    pattern: &PerformanceEvents,
    instrument: &CompiledInstrument,
    timeline: &Timeline,
) -> Result<CompiledEvents, Vec<Diagnostic>> {
    let mut diagnostics = validate(pattern);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let sample_rate = timeline.sample_rate;
    let tempo_changes = &timeline.tempo_changes;
    let length_frames = timeline.length_frames;
    let musical_time_map = Some(timeline.map.clone());
    let time = EventTimeContext {
        pattern,
        tempo_changes,
        sample_rate,
    };

    let mut pending = Vec::with_capacity(pattern.events.len().saturating_mul(2));
    let mut note_serial = 0_u32;
    for (source_index, event) in pattern.events.iter().enumerate() {
        match event {
            MusicalEvent::Note {
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
                let start_frame = match tick_to_frame(
                    *tick,
                    pattern.ticks_per_beat,
                    tempo_changes,
                    sample_rate,
                ) {
                    Ok(frame) => frame,
                    Err(error) => {
                        diagnostics.push(time_error(error, format!("events[{source_index}].tick")));
                        continue;
                    }
                };
                let end_frame = match tick_to_frame(
                    end_tick,
                    pattern.ticks_per_beat,
                    tempo_changes,
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
                pending.push(PendingMusicalEvent {
                    absolute_frame: start_frame,
                    original_tick: *tick,
                    source_index,
                    kind: ProcessEventKind::NoteOn {
                        note_id,
                        note_number: *note,
                        velocity: *velocity,
                    },
                });
                pending.push(PendingMusicalEvent {
                    absolute_frame: end_frame,
                    original_tick: end_tick,
                    source_index,
                    kind: ProcessEventKind::NoteOff { note_id },
                });
            }
            MusicalEvent::SustainPedal { tick, down } => {
                push_control_event(
                    &mut pending,
                    *tick,
                    source_index,
                    &time,
                    ProcessEventKind::SustainPedal { down: *down },
                    &mut diagnostics,
                );
            }
            MusicalEvent::PitchBend { tick, value } => {
                push_control_event(
                    &mut pending,
                    *tick,
                    source_index,
                    &time,
                    ProcessEventKind::PitchBend { value: *value },
                    &mut diagnostics,
                );
            }
            MusicalEvent::ModWheel { tick, value } => {
                push_control_event(
                    &mut pending,
                    *tick,
                    source_index,
                    &time,
                    ProcessEventKind::ModWheel { value: *value },
                    &mut diagnostics,
                );
            }
            MusicalEvent::Aftertouch { tick, value } => {
                push_control_event(
                    &mut pending,
                    *tick,
                    source_index,
                    &time,
                    ProcessEventKind::Aftertouch { value: *value },
                    &mut diagnostics,
                );
            }
            MusicalEvent::ParameterChange {
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
                        catalog_revision: instrument.parameter_catalog_revision(),
                        parameter: handle,
                        normalized,
                    },
                    &mut diagnostics,
                );
            }
            MusicalEvent::ParameterRamp {
                tick,
                duration_ticks,
                parameter,
                from_value,
                to_value,
            } => {
                let path = format!("events[{source_index}]");
                let result = (|| {
                    let start =
                        tick_to_frame(*tick, pattern.ticks_per_beat, tempo_changes, sample_rate)
                            .map_err(|error| time_error(error, format!("{path}.tick")))?;
                    let end = tick_to_frame(
                        tick.checked_add(*duration_ticks)
                            .expect("validated ramp end tick"),
                        pattern.ticks_per_beat,
                        tempo_changes,
                        sample_rate,
                    )
                    .map_err(|error| time_error(error, format!("{path}.duration_ticks")))?;
                    let duration_frames = end
                        .checked_sub(start)
                        .and_then(|frames| usize::try_from(frames).ok())
                        .filter(|frames| *frames > 0)
                        .ok_or_else(|| {
                            Diagnostic::error(
                                DiagnosticCode::ValueOutOfRange,
                                "ramp duration must occupy at least one frame and fit in the process frame counter",
                            )
                            .with_path(format!("{path}.duration_ticks"))
                        })?;
                    let kind = resolve_parameter_ramp(
                        instrument,
                        parameter,
                        *from_value,
                        *to_value,
                        duration_frames,
                        &path,
                    )?;
                    Ok(PendingMusicalEvent {
                        absolute_frame: start,
                        original_tick: *tick,
                        source_index,
                        kind,
                    })
                })();
                match result {
                    Ok(event) => pending.push(event),
                    Err(error) => diagnostics.push(error),
                }
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
    Ok(CompiledEvents {
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
    pending: &mut Vec<PendingMusicalEvent>,
    tick: u64,
    source_index: usize,
    time: &EventTimeContext<'_>,
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
    pending.push(PendingMusicalEvent {
        absolute_frame: frame,
        original_tick: tick,
        source_index,
        kind,
    });
}

pub(crate) fn resolve_parameter_ramp(
    instrument: &CompiledInstrument,
    parameter: &str,
    from_value: f32,
    to_value: f32,
    duration_frames: usize,
    path: &str,
) -> Result<ProcessEventKind, Diagnostic> {
    let handle = instrument.parameter_handle(parameter).ok_or_else(|| {
        Diagnostic::error(
            DiagnosticCode::ParameterNotFound,
            "parameter id is not present in the compiled catalog",
        )
        .with_path(format!("{path}.parameter"))
    })?;
    let descriptor = instrument
        .parameter_descriptor(handle)
        .expect("resolved catalog handle");
    let normalize = |value, field| {
        descriptor.normalize(value).map_err(|error| {
            Diagnostic::error(DiagnosticCode::ValueOutOfRange, error.to_string())
                .with_path(format!("{path}.{field}"))
        })
    };
    Ok(ProcessEventKind::ParameterRamp {
        catalog_revision: instrument.parameter_catalog_revision(),
        parameter: handle,
        from_normalized: normalize(from_value, "from_value")?,
        to_normalized: normalize(to_value, "to_value")?,
        duration_frames,
    })
}

pub(crate) fn tempo_points(pattern: &PerformanceEvents) -> Vec<TempoPoint> {
    pattern
        .tempo_changes
        .iter()
        .map(|change| TempoPoint {
            tick: change.tick,
            bpm: change.bpm,
        })
        .collect()
}

pub(crate) fn time_signature_points(pattern: &PerformanceEvents) -> Vec<TimeSignaturePoint> {
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

    use sonalloy_core::{
        CompileContext, InstrumentDefinition, ProcessEventKind, ProcessSpec, ScheduledEvent,
        compile_instrument,
    };

    use super::compile;
    use crate::performance::events::default_pattern;

    #[test]
    fn pattern_compile_preserves_musical_positions_across_sample_rates() {
        let definition: InstrumentDefinition = serde_json::from_str(include_str!(
            "../../../../testdata/instruments/basic-poly-synth.json"
        ))
        .expect("reference Definition parses");
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

    #[test]
    fn ramp_duration_uses_both_tick_endpoints_across_tempo_changes() {
        let definition: InstrumentDefinition = serde_json::from_str(include_str!(
            "../../../../testdata/instruments/basic-poly-synth.json"
        ))
        .expect("definition");
        let instrument = compile_instrument(
            &definition,
            &CompileContext {
                definition_base_dir: PathBuf::from("."),
                process_spec: ProcessSpec::new(48_000.0, 257, 0, 2).expect("process spec"),
            },
        )
        .instrument
        .expect("instrument");
        let mut pattern = default_pattern();
        pattern
            .tempo_changes
            .push(crate::score::events::TempoChange {
                tick: 480,
                bpm: 60.0,
            });
        pattern
            .events
            .push(crate::score::events::MusicalEvent::ParameterRamp {
                tick: 240,
                duration_ticks: 480,
                parameter: "layer.body.gain".to_owned(),
                from_value: -24.0,
                to_value: 0.0,
            });

        let compiled = compile(&pattern, &instrument, 48_000.0).expect("ramp compiles");

        let event = compiled
            .events
            .iter()
            .find(|event| matches!(event.kind, ProcessEventKind::ParameterRamp { .. }))
            .expect("ramp");
        assert_eq!(event.absolute_frame, 12_000);
        let ProcessEventKind::ParameterRamp {
            duration_frames,
            from_normalized,
            to_normalized,
            parameter,
            ..
        } = event.kind
        else {
            unreachable!()
        };
        assert_eq!(duration_frames, 36_000);
        let descriptor = instrument
            .parameter_descriptor(parameter)
            .expect("descriptor");
        assert!(
            (from_normalized - descriptor.normalize(-24.0).expect("from")).abs() < f32::EPSILON
        );
        assert!((to_normalized - descriptor.normalize(0.0).expect("to")).abs() < f32::EPSILON);

        pattern.tempo_changes[0].bpm = 1.0e6;
        pattern.events.pop();
        pattern
            .events
            .push(crate::score::events::MusicalEvent::ParameterRamp {
                tick: 0,
                duration_ticks: 1,
                parameter: "layer.body.gain".to_owned(),
                from_value: -24.0,
                to_value: 0.0,
            });
        let errors =
            compile(&pattern, &instrument, 48_000.0).expect_err("sub-frame ramp must fail");
        assert!(
            errors
                .iter()
                .any(|error| error.path.as_deref() == Some("events[1].duration_ticks"))
        );
    }
}
