use std::path::PathBuf;
use std::sync::Arc;

use sonalloy_core::{
    CompileContext, CompressorProcessorDefinition, DiagnosticCode, DriveProcessorDefinition,
    DynamicsDetectorDefinition, ExternalAudioChannels, ExternalAudioInputDefinition,
    FrequencyShifterProcessorDefinition, InstrumentProcessor, InstrumentRuntime, ModulationCurve,
    ModulationDefinition, ModulationDepthDefinition, ModulationRouteDefinition, ModulationUnit,
    PerformanceDefinition, ProcessBlock, ProcessContext, ProcessEvent, ProcessEventKind,
    ProcessSpec, ProcessorDefinition, PublishError, ReclaimableRuntimeResourceKind, RuntimeState,
    TransportState, compile_instrument,
};

fn definition_json(waveform: &str, gain_db: f32) -> sonalloy_core::InstrumentDefinition {
    let mut value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../testdata/instruments/basic-poly-synth.json"
    ))
    .expect("fixture is valid JSON");
    value["layers"][0]["gain_db"] = serde_json::json!(gain_db);
    value["layers"][0]["generator"]["oscillator"]["waveform"]["type"] = serde_json::json!(waveform);
    serde_json::from_value(value).expect("fixture remains a valid definition")
}

fn compile(
    definition: &sonalloy_core::InstrumentDefinition,
) -> Arc<sonalloy_core::CompiledInstrument> {
    compile_with_spec(
        definition,
        ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec"),
    )
}

fn compile_with_spec(
    definition: &sonalloy_core::InstrumentDefinition,
    process_spec: ProcessSpec,
) -> Arc<sonalloy_core::CompiledInstrument> {
    let result = compile_instrument(
        definition,
        &CompileContext {
            definition_base_dir: PathBuf::from("testdata/instruments"),
            process_spec,
        },
    );
    let sonalloy_core::CompileResult {
        instrument,
        diagnostics,
    } = result;
    instrument.unwrap_or_else(|| panic!("definition compiles: {diagnostics:?}"))
}

fn active_runtime(
    compiled: Arc<sonalloy_core::CompiledInstrument>,
    spec: ProcessSpec,
) -> InstrumentRuntime {
    let mut runtime = InstrumentRuntime::new(compiled);
    runtime.prepare(spec).expect("runtime prepares");
    runtime.activate().expect("runtime activates");
    runtime
}

fn global_drive_definition(waveform: &str, gain_db: f32) -> sonalloy_core::InstrumentDefinition {
    let mut definition = definition_json(waveform, gain_db);
    definition.global_processors = vec![ProcessorDefinition::Drive(DriveProcessorDefinition {
        id: "drive".to_owned(),
        amount: 0.0,
        mix: 1.0,
    })];
    definition
}

fn global_filter_definition(waveform: &str, gain_db: f32) -> sonalloy_core::InstrumentDefinition {
    let mut definition = definition_json(waveform, gain_db);
    definition.global_processors = vec![ProcessorDefinition::Filter(
        sonalloy_core::FilterProcessorDefinition {
            id: "filter".to_owned(),
            mode: sonalloy_core::FilterModeDefinition::LowPass,
            cutoff_hz: 1_200.0,
            resonance: 0.0,
        },
    )];
    definition
}

fn performance_control_definition(
    waveform: &str,
    gain_db: f32,
) -> sonalloy_core::InstrumentDefinition {
    let mut definition = definition_json(waveform, gain_db);
    definition.modulation = Some(ModulationDefinition {
        sources: Vec::new(),
        routes: ["pitch_bend", "mod_wheel", "aftertouch"]
            .into_iter()
            .map(|source| ModulationRouteDefinition {
                source: source.to_owned(),
                target: "layer.body.tuning".to_owned(),
                depth: ModulationDepthDefinition {
                    value: 1_200.0,
                    unit: ModulationUnit::Cents,
                },
                curve: ModulationCurve::Linear,
            })
            .collect(),
    });
    definition.layers[0].envelope.release_seconds = 0.0;
    definition
}

fn monophonic_definition(waveform: &str, gain_db: f32) -> sonalloy_core::InstrumentDefinition {
    let mut definition = definition_json(waveform, gain_db);
    definition.performance = PerformanceDefinition::Monophonic {
        legato: true,
        portamento: None,
    };
    definition.layers[0].envelope.release_seconds = 0.0;
    definition
}

fn process(runtime: &mut InstrumentRuntime, frame: u64, events: &[ProcessEvent]) -> [Vec<f32>; 2] {
    let frame_f64 = f64::from(u32::try_from(frame).expect("test frame fits u32"));
    let mut left = vec![0.0; 64];
    let mut right = vec![0.0; 64];
    let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
    runtime
        .process(ProcessBlock {
            frames: 64,
            context: ProcessContext {
                absolute_frame: frame,
                tempo_bpm: 120.0,
                beat_position: frame_f64 / 24_000.0,
                bar_position: frame_f64 / 192_000.0,
                time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
                transport_state: TransportState::Playing,
            },
            events,
            input: &[],
            output: &mut output,
        })
        .expect("runtime process succeeds");
    [left, right]
}

fn runtime_after_update(
    first: &Arc<sonalloy_core::CompiledInstrument>,
    second: &Arc<sonalloy_core::CompiledInstrument>,
) -> InstrumentRuntime {
    let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
    let mut runtime = InstrumentRuntime::new(Arc::clone(first));
    runtime.prepare(spec).expect("runtime prepares");
    runtime.activate().expect("runtime activates");
    let _ = process(
        &mut runtime,
        0,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 110,
            },
        }],
    );
    let mut update =
        InstrumentRuntime::prepare_update(Arc::clone(second), spec).expect("update prepares");
    runtime
        .publish_prepared(&mut update)
        .expect("update publishes");
    runtime
}

fn process_note_switch(runtime: &mut InstrumentRuntime) -> [Vec<f32>; 2] {
    process_note_switch_at(runtime, 64)
}

fn process_note_switch_at(runtime: &mut InstrumentRuntime, frame: u64) -> [Vec<f32>; 2] {
    process(
        runtime,
        frame,
        &[
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            },
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 67,
                    velocity: 110,
                },
            },
        ],
    )
}

#[test]
fn lifecycle_update_routes_new_notes_and_reclaims_old_generation() {
    let first = compile(&definition_json("saw", -14.0));
    let second = compile(&definition_json("sine", -8.0));
    assert_ne!(
        first.parameter_catalog_revision(),
        second.parameter_catalog_revision()
    );

    let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
    let mut runtime = InstrumentRuntime::new(Arc::clone(&first));
    assert_eq!(runtime.state(), RuntimeState::Unprepared);
    assert_eq!(
        runtime.activate(),
        Err(sonalloy_core::ProcessError::NotPrepared)
    );
    runtime.prepare(spec).expect("runtime prepares");
    assert_eq!(runtime.state(), RuntimeState::Prepared);
    runtime.activate().expect("runtime activates");

    let first_audio = process(
        &mut runtime,
        0,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 110,
            },
        }],
    );
    assert!(first_audio[0].iter().any(|sample| sample.abs() > 1.0e-6));

    let mut mismatched = InstrumentRuntime::prepare_update(
        Arc::clone(&second),
        ProcessSpec::new(48_000.0, 128, 0, 2).expect("valid update spec"),
    )
    .expect("mismatched update prepares");
    assert_eq!(
        runtime.publish_prepared(&mut mismatched),
        Err(PublishError::ProcessSpecMismatch)
    );
    assert!(!mismatched.is_consumed());

    let mut update =
        InstrumentRuntime::prepare_update(Arc::clone(&second), spec).expect("update prepares");
    let outcome = runtime
        .publish_prepared(&mut update)
        .expect("update publishes");
    assert_eq!(outcome.generation_id.get(), 2);
    assert_eq!(runtime.generation_id().get(), 2);
    assert!(update.is_consumed());
    assert_eq!(
        runtime.publish_prepared(&mut update),
        Err(PublishError::UpdateConsumed)
    );

    let next_audio = process(
        &mut runtime,
        64,
        &[
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            },
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 67,
                    velocity: 110,
                },
            },
        ],
    );
    assert!(next_audio[0].iter().all(|sample| sample.is_finite()));

    let stale = ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::ParameterChange {
            catalog_revision: first.parameter_catalog_revision(),
            parameter: second
                .parameter_handle("layer.body.gain")
                .expect("gain handle"),
            normalized: 0.2,
        },
    };
    let _ = process(&mut runtime, 128, &[stale]);
    assert_eq!(runtime.stale_parameter_event_count(), 1);

    runtime.reset().expect("reset succeeds");
    assert_eq!(runtime.absolute_frame(), 0);
    assert!(runtime.take_reclaimable().is_some());
    runtime.deactivate().expect("deactivate succeeds");
    assert_eq!(runtime.state(), RuntimeState::Prepared);
}

#[test]
fn multigeneration_global_parameter_changes_start_at_their_event_offset() {
    let first = compile(&global_drive_definition("saw", -14.0));
    let second = compile(&global_drive_definition("saw", -8.0));
    let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");

    let mut with_event = InstrumentRuntime::new(Arc::clone(&first));
    with_event.prepare(spec).expect("runtime prepares");
    with_event.activate().expect("runtime activates");
    let _ = process(
        &mut with_event,
        0,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 110,
            },
        }],
    );
    let mut update =
        InstrumentRuntime::prepare_update(Arc::clone(&second), spec).expect("update prepares");
    with_event
        .publish_prepared(&mut update)
        .expect("update publishes");

    let mut without_event = InstrumentRuntime::new(Arc::clone(&first));
    without_event.prepare(spec).expect("runtime prepares");
    without_event.activate().expect("runtime activates");
    let _ = process(
        &mut without_event,
        0,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 110,
            },
        }],
    );
    let mut update =
        InstrumentRuntime::prepare_update(Arc::clone(&second), spec).expect("update prepares");
    without_event
        .publish_prepared(&mut update)
        .expect("update publishes");

    let amount = second
        .parameter_handle("global.processor.drive.amount")
        .expect("global drive amount handle");
    let with_event_audio = process(
        &mut with_event,
        64,
        &[ProcessEvent {
            sample_offset: 32,
            kind: ProcessEventKind::ParameterChange {
                catalog_revision: second.parameter_catalog_revision(),
                parameter: amount,
                normalized: 1.0,
            },
        }],
    );
    let without_event_audio = process(&mut without_event, 64, &[]);

    for (actual, expected) in with_event_audio[0][..32]
        .iter()
        .zip(&without_event_audio[0][..32])
    {
        assert!((actual - expected).abs() < 1.0e-7);
    }
    assert!(
        with_event_audio[0][32..]
            .iter()
            .zip(&without_event_audio[0][32..])
            .any(|(actual, expected)| (actual - expected).abs() > 1.0e-6)
    );
}

#[test]
fn update_keeps_old_waveform_and_uses_new_waveform_for_new_notes() {
    let first = compile(&definition_json("saw", -14.0));
    let sine = compile(&definition_json("sine", -8.0));
    let saw = compile(&definition_json("saw", -8.0));

    let mut baseline = InstrumentRuntime::new(Arc::clone(&first));
    baseline
        .prepare(ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec"))
        .expect("runtime prepares");
    baseline.activate().expect("runtime activates");
    let _ = process(
        &mut baseline,
        0,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 110,
            },
        }],
    );
    let baseline_audio = process(&mut baseline, 64, &[]);
    let mut updated = runtime_after_update(&first, &sine);
    let updated_audio = process(&mut updated, 64, &[]);
    for (actual, expected) in updated_audio[0].iter().zip(&baseline_audio[0]) {
        assert!((actual - expected).abs() < 1.0e-7);
    }

    let mut saw_update = runtime_after_update(&first, &saw);
    let saw_audio = process_note_switch(&mut saw_update);
    let mut sine_update = runtime_after_update(&first, &sine);
    let sine_audio = process_note_switch(&mut sine_update);
    assert!(
        saw_audio[0]
            .iter()
            .zip(&sine_audio[0])
            .any(|(saw, sine)| (saw - sine).abs() > 1.0e-5)
    );
}

#[test]
fn update_applies_new_layers_and_voice_processors_to_new_notes() {
    let first = compile(&definition_json("saw", -14.0));
    let one_layer = compile(&definition_json("saw", -8.0));

    let mut two_layer_definition = definition_json("saw", -8.0);
    let mut extra_layer = two_layer_definition.layers[0].clone();
    extra_layer.id = "upper".to_owned();
    two_layer_definition.layers.push(extra_layer);
    let two_layers = compile(&two_layer_definition);
    let mut one_layer_runtime = runtime_after_update(&first, &one_layer);
    let one_layer_old_audio = process(&mut one_layer_runtime, 64, &[]);
    let mut two_layer_runtime = runtime_after_update(&first, &two_layers);
    let two_layer_old_audio = process(&mut two_layer_runtime, 64, &[]);
    for (one, two) in one_layer_old_audio[0].iter().zip(&two_layer_old_audio[0]) {
        assert!((one - two).abs() < 1.0e-7);
    }
    let one_layer_audio = process_note_switch_at(&mut one_layer_runtime, 128);
    let two_layer_audio = process_note_switch_at(&mut two_layer_runtime, 128);
    assert!(
        two_layer_audio[0]
            .iter()
            .zip(&one_layer_audio[0])
            .any(|(two, one)| (two - one).abs() > 1.0e-5)
    );

    let mut changed_voice_definition = definition_json("saw", -8.0);
    let ProcessorDefinition::Filter(filter) = &mut changed_voice_definition.voice_processors[0]
    else {
        panic!("basic fixture must use a filter voice processor");
    };
    filter.cutoff_hz = 400.0;
    let changed_voice = compile(&changed_voice_definition);
    let mut default_voice_runtime = runtime_after_update(&first, &one_layer);
    let default_old_audio = process(&mut default_voice_runtime, 64, &[]);
    let mut changed_voice_runtime = runtime_after_update(&first, &changed_voice);
    let changed_old_audio = process(&mut changed_voice_runtime, 64, &[]);
    for (default, changed) in default_old_audio[0].iter().zip(&changed_old_audio[0]) {
        assert!((default - changed).abs() < 1.0e-7);
    }
    let default_voice_audio = process_note_switch_at(&mut default_voice_runtime, 128);
    let changed_voice_audio = process_note_switch_at(&mut changed_voice_runtime, 128);
    assert!(
        changed_voice_audio[0]
            .iter()
            .zip(&default_voice_audio[0])
            .any(|(changed, default)| (changed - default).abs() > 1.0e-5)
    );
}

#[test]
fn global_processor_update_crossfades_and_blocks_overlapping_publish() {
    let first = compile(&global_drive_definition("saw", -14.0));
    let second = compile(&global_filter_definition("saw", -8.0));
    let third = compile(&global_drive_definition("saw", -6.0));
    let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
    let mut runtime = InstrumentRuntime::new(Arc::clone(&first));
    runtime.prepare(spec).expect("runtime prepares");
    runtime.activate().expect("runtime activates");
    let _ = process(
        &mut runtime,
        0,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 110,
            },
        }],
    );
    let mut update = InstrumentRuntime::prepare_update(Arc::clone(&second), spec)
        .expect("global update prepares");
    runtime
        .publish_prepared(&mut update)
        .expect("global update publishes");
    let mut overlapping_update = InstrumentRuntime::prepare_update(Arc::clone(&third), spec)
        .expect("overlapping update prepares");
    assert_eq!(
        runtime.publish_prepared(&mut overlapping_update),
        Err(PublishError::TransitionBusy)
    );
    let mut transition_audio = Vec::with_capacity(256);
    for frame in [64, 128, 192, 256] {
        let audio = process(&mut runtime, frame, &[]);
        assert!(audio.iter().flatten().all(|sample| sample.is_finite()));
        transition_audio.extend_from_slice(&audio[0]);
    }
    assert!(transition_audio.iter().any(|sample| sample.abs() > 1.0e-6));
    assert!(
        transition_audio
            .windows(2)
            .map(|window| (window[1] - window[0]).abs())
            .all(|jump| jump < 0.5),
        "global transition contains a one-sample discontinuity"
    );
    assert_eq!(
        runtime.take_reclaimable().map(|resource| resource.kind()),
        Some(ReclaimableRuntimeResourceKind::GlobalProcessor)
    );
    runtime
        .publish_prepared(&mut overlapping_update)
        .expect("publish succeeds after crossfade");
}

#[test]
fn unchanged_global_processor_keeps_state_across_generation_update() {
    let first = compile(&global_filter_definition("saw", -14.0));
    let second = compile(&global_filter_definition("sine", -14.0));
    assert_eq!(
        first.global_processors, second.global_processors,
        "the update must only change the voice definition"
    );
    let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
    let mut reference = active_runtime(Arc::clone(&first), spec);
    let mut updated = active_runtime(Arc::clone(&first), spec);
    let note_on = [ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 110,
        },
    }];
    let _ = process(&mut reference, 0, &note_on);
    let _ = process(&mut updated, 0, &note_on);
    for frame in [64, 128, 192] {
        let reference_audio = process(&mut reference, frame, &[]);
        let updated_audio = process(&mut updated, frame, &[]);
        for (actual, expected) in updated_audio[0].iter().zip(&reference_audio[0]) {
            assert!((actual - expected).abs() < 1.0e-7);
        }
    }

    let mut update = InstrumentRuntime::prepare_update(Arc::clone(&second), spec)
        .expect("unchanged global update prepares");
    updated
        .publish_prepared(&mut update)
        .expect("unchanged global update publishes");
    let reference_audio = process(&mut reference, 256, &[]);
    let updated_audio = process(&mut updated, 256, &[]);
    assert!(
        reference_audio[0]
            .iter()
            .any(|sample| sample.abs() > 1.0e-6)
    );
    for (actual, expected) in updated_audio[0].iter().zip(&reference_audio[0]) {
        assert!((actual - expected).abs() < 1.0e-7);
    }
}

#[test]
fn sustain_and_note_off_reach_the_retired_generation() {
    let mut first_definition = definition_json("saw", -14.0);
    first_definition.layers[0].envelope.release_seconds = 0.0;
    let mut second_definition = definition_json("sine", -8.0);
    second_definition.layers[0].envelope.release_seconds = 0.0;
    let first = compile(&first_definition);
    let second = compile(&second_definition);
    let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
    let mut runtime = active_runtime(Arc::clone(&first), spec);

    let _ = process(
        &mut runtime,
        0,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 110,
            },
        }],
    );
    let sustained_audio = process(
        &mut runtime,
        64,
        &[
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::SustainPedal { down: true },
            },
            ProcessEvent {
                sample_offset: 1,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            },
        ],
    );
    assert!(
        sustained_audio[0]
            .iter()
            .any(|sample| sample.abs() > 1.0e-6)
    );

    let mut update =
        InstrumentRuntime::prepare_update(Arc::clone(&second), spec).expect("update prepares");
    runtime
        .publish_prepared(&mut update)
        .expect("update publishes while sustain is down");
    let held_audio = process(&mut runtime, 128, &[]);
    assert!(held_audio[0].iter().any(|sample| sample.abs() > 1.0e-6));
    assert!(runtime.take_reclaimable().is_none());

    let released_audio = process(
        &mut runtime,
        192,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::SustainPedal { down: false },
        }],
    );
    assert!(
        released_audio
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert_eq!(
        runtime.take_reclaimable().map(|resource| resource.kind()),
        Some(ReclaimableRuntimeResourceKind::Generation)
    );
}

#[test]
fn performance_controls_reach_the_retired_generation() {
    let first = compile(&performance_control_definition("saw", -14.0));
    let second = compile(&performance_control_definition("saw", -8.0));
    let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
    let controls_before_publish = [
        ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::PitchBend { value: 0.2 },
        },
        ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::ModWheel { value: 0.3 },
        },
        ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::Aftertouch { value: 0.4 },
        },
    ];
    let controls_after_publish = [
        ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::PitchBend { value: -0.7 },
        },
        ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::ModWheel { value: 0.8 },
        },
        ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::Aftertouch { value: 0.9 },
        },
    ];
    let note_on = ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 110,
        },
    };

    let mut actual = active_runtime(Arc::clone(&first), spec);
    let mut old_reference = active_runtime(Arc::clone(&first), spec);
    let _ = process(&mut actual, 0, &[note_on]);
    let _ = process(&mut old_reference, 0, &[note_on]);
    let _ = process(&mut actual, 64, &controls_before_publish);
    let _ = process(&mut old_reference, 64, &controls_before_publish);

    let mut update =
        InstrumentRuntime::prepare_update(Arc::clone(&second), spec).expect("update prepares");
    actual
        .publish_prepared(&mut update)
        .expect("update publishes");
    let actual_old_audio = process(&mut actual, 128, &controls_after_publish);
    let reference_old_audio = process(&mut old_reference, 128, &controls_after_publish);
    for (actual, expected) in actual_old_audio[0].iter().zip(&reference_old_audio[0]) {
        assert!((actual - expected).abs() < 1.0e-7);
    }
}

#[test]
fn performance_controls_are_inherited_by_the_new_generation() {
    let first = compile(&performance_control_definition("saw", -14.0));
    let second = compile(&performance_control_definition("saw", -8.0));
    let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
    let controls = [
        ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::PitchBend { value: 1.0 },
        },
        ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::ModWheel { value: 1.0 },
        },
        ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::Aftertouch { value: 1.0 },
        },
    ];
    let note_one = ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 110,
        },
    };
    let mut actual = active_runtime(Arc::clone(&first), spec);
    let _ = process(&mut actual, 0, &[note_one]);
    let _ = process(&mut actual, 64, &controls);
    let mut update =
        InstrumentRuntime::prepare_update(Arc::clone(&second), spec).expect("update prepares");
    actual
        .publish_prepared(&mut update)
        .expect("update publishes");
    let _ = process(
        &mut actual,
        128,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        }],
    );
    assert!(actual.take_reclaimable().is_some());
    let new_note = ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 2,
            note_number: 67,
            velocity: 110,
        },
    };
    let actual_audio = process(&mut actual, 192, &[new_note]);

    let mut without_inherited_controls = active_runtime(Arc::clone(&second), spec);
    let _ = process(&mut without_inherited_controls, 0, &[]);
    let _ = process(&mut without_inherited_controls, 64, &[]);
    let _ = process(&mut without_inherited_controls, 128, &[]);
    let expected_without_controls = process(&mut without_inherited_controls, 192, &[new_note]);
    let max_difference = actual_audio[0]
        .iter()
        .zip(&expected_without_controls[0])
        .map(|(actual, expected)| (actual - expected).abs())
        .fold(0.0, f32::max);
    assert!(
        max_difference > 1.0e-4,
        "inherited performance controls did not affect the new voice"
    );
    assert!(actual_audio[0].iter().any(|sample| sample.abs() > 1.0e-6));
}

#[test]
fn monophonic_update_keeps_held_notes_in_their_original_generation() {
    let first = compile(&monophonic_definition("saw", -14.0));
    let second = compile(&monophonic_definition("sine", -8.0));
    let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
    let note_one = ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 110,
        },
    };
    let mut actual = active_runtime(Arc::clone(&first), spec);
    let mut old_reference = active_runtime(Arc::clone(&first), spec);
    let _ = process(&mut actual, 0, &[note_one]);
    let _ = process(&mut old_reference, 0, &[note_one]);
    let mut update =
        InstrumentRuntime::prepare_update(Arc::clone(&second), spec).expect("update prepares");
    actual
        .publish_prepared(&mut update)
        .expect("update publishes");

    let actual_old_audio = process(&mut actual, 64, &[]);
    let reference_old_audio = process(&mut old_reference, 64, &[]);
    for (actual, expected) in actual_old_audio[0].iter().zip(&reference_old_audio[0]) {
        assert!((actual - expected).abs() < 1.0e-7);
    }

    let _ = process(
        &mut actual,
        128,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        }],
    );
    assert_eq!(
        actual.take_reclaimable().map(|resource| resource.kind()),
        Some(ReclaimableRuntimeResourceKind::Generation)
    );
    let new_note = ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 2,
            note_number: 67,
            velocity: 110,
        },
    };
    let actual_new_audio = process(&mut actual, 192, &[new_note]);

    let mut fresh = active_runtime(Arc::clone(&second), spec);
    let expected_new_audio = process(&mut fresh, 0, &[new_note]);
    for (actual, expected) in actual_new_audio[0].iter().zip(&expected_new_audio[0]) {
        assert!((actual - expected).abs() < 1.0e-7);
    }
    assert!(
        actual_new_audio[0]
            .iter()
            .any(|sample| sample.abs() > 1.0e-6)
    );
}

#[test]
fn parameter_changes_only_reconfigure_the_active_generation() {
    let mut first_definition = definition_json("saw", -14.0);
    first_definition.layers[0].envelope.release_seconds = 0.0;
    let mut second_definition = definition_json("saw", -14.0);
    second_definition.layers[0].envelope.release_seconds = 0.0;
    let first = compile(&first_definition);
    let second = compile(&second_definition);
    let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
    let note_one = ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 110,
        },
    };
    let mut actual = active_runtime(Arc::clone(&first), spec);
    let mut old_reference = active_runtime(Arc::clone(&first), spec);
    let _ = process(&mut actual, 0, &[note_one]);
    let _ = process(&mut old_reference, 0, &[note_one]);
    let mut update =
        InstrumentRuntime::prepare_update(Arc::clone(&second), spec).expect("update prepares");
    actual
        .publish_prepared(&mut update)
        .expect("update publishes");

    let gain = second
        .parameter_handle("layer.body.gain")
        .expect("gain handle");
    let actual_old_audio = process(
        &mut actual,
        64,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::ParameterChange {
                catalog_revision: second.parameter_catalog_revision(),
                parameter: gain,
                normalized: 1.0,
            },
        }],
    );
    let reference_old_audio = process(&mut old_reference, 64, &[]);
    for (actual, expected) in actual_old_audio[0].iter().zip(&reference_old_audio[0]) {
        assert!((actual - expected).abs() < 1.0e-7);
    }

    let _ = process(
        &mut actual,
        128,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        }],
    );
    assert!(actual.take_reclaimable().is_some());
    let actual_new_audio = process(
        &mut actual,
        192,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 67,
                velocity: 110,
            },
        }],
    );

    let mut fresh = active_runtime(Arc::clone(&second), spec);
    let _ = process(
        &mut fresh,
        0,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::ParameterChange {
                catalog_revision: second.parameter_catalog_revision(),
                parameter: gain,
                normalized: 1.0,
            },
        }],
    );
    let _ = process(&mut fresh, 64, &[]);
    let expected_new_audio = process(
        &mut fresh,
        128,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 67,
                velocity: 110,
            },
        }],
    );
    for (actual, expected) in actual_new_audio[0].iter().zip(&expected_new_audio[0]) {
        assert!((actual - expected).abs() < 1.0e-7);
    }
    assert!(
        actual_new_audio[0]
            .iter()
            .any(|sample| sample.abs() > 1.0e-6)
    );
}

#[test]
fn compile_failure_keeps_the_active_runtime_unchanged() {
    let first = compile(&definition_json("saw", -14.0));
    let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
    let mut actual = active_runtime(Arc::clone(&first), spec);
    let mut expected = active_runtime(Arc::clone(&first), spec);
    let note_on = ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 110,
        },
    };
    let _ = process(&mut actual, 0, &[note_on]);
    let _ = process(&mut expected, 0, &[note_on]);

    let mut invalid = definition_json("sine", -8.0);
    invalid.layers.clear();
    let result = compile_instrument(
        &invalid,
        &CompileContext {
            definition_base_dir: PathBuf::from("testdata/instruments"),
            process_spec: spec,
        },
    );
    assert!(result.instrument.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::RequiredFieldMissing)
    );
    assert_eq!(actual.generation_id().get(), 1);

    let actual_audio = process(&mut actual, 64, &[]);
    let expected_audio = process(&mut expected, 64, &[]);
    for (actual, expected) in actual_audio[0].iter().zip(&expected_audio[0]) {
        assert!((actual - expected).abs() < 1.0e-7);
    }
}

#[test]
fn publish_rejects_latency_and_external_input_changes_for_reactivation() {
    let first = compile(&definition_json("saw", -14.0));
    let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
    let mut runtime = InstrumentRuntime::new(Arc::clone(&first));
    runtime.prepare(spec).expect("runtime prepares");
    runtime.activate().expect("runtime activates");
    let _ = process(
        &mut runtime,
        0,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 110,
            },
        }],
    );

    let mut latency_definition = definition_json("saw", -14.0);
    latency_definition.global_processors = vec![ProcessorDefinition::FrequencyShifter(
        FrequencyShifterProcessorDefinition {
            id: "shift".to_owned(),
            shift_hz: 240.0,
            mix: 1.0,
        },
    )];
    let latency_compiled = compile(&latency_definition);
    assert!(latency_compiled.reported_latency_frames() > first.reported_latency_frames());
    let mut latency_update = InstrumentRuntime::prepare_update(Arc::clone(&latency_compiled), spec)
        .expect("latency update prepares");
    assert!(matches!(
        runtime.publish_prepared(&mut latency_update),
        Err(PublishError::RequiresReactivation {
            reason: sonalloy_core::ReactivationReason::LatencyChanged,
            ..
        })
    ));
    assert!(!latency_update.is_consumed());
    let latency_audio = process(&mut runtime, 64, &[]);
    assert!(
        latency_audio
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert!(latency_audio[0].iter().any(|sample| sample.abs() > 1.0e-6));

    let mut external_definition = definition_json("saw", -14.0);
    external_definition.external_audio = Some(ExternalAudioInputDefinition {
        channels: ExternalAudioChannels::Stereo,
    });
    external_definition.global_processors = vec![ProcessorDefinition::Compressor(
        CompressorProcessorDefinition {
            id: "external".to_owned(),
            threshold_db: -18.0,
            ratio: 4.0,
            attack_ms: 15.0,
            release_ms: 180.0,
            knee_db: 6.0,
            makeup_gain_db: 2.0,
            mix: 1.0,
            detector: DynamicsDetectorDefinition::ExternalAudio,
        },
    )];
    let external_spec = ProcessSpec::new(48_000.0, 64, 2, 2).expect("external process spec");
    let external_compiled = compile_with_spec(&external_definition, external_spec);
    let mut external_update =
        InstrumentRuntime::prepare_update(Arc::clone(&external_compiled), external_spec)
            .expect("external input update prepares");
    assert!(matches!(
        runtime.publish_prepared(&mut external_update),
        Err(PublishError::RequiresReactivation {
            reason: sonalloy_core::ReactivationReason::InputChannelsChanged,
            ..
        })
    ));
    assert!(!external_update.is_consumed());
    let external_audio = process(&mut runtime, 128, &[]);
    assert!(
        external_audio
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert!(external_audio[0].iter().any(|sample| sample.abs() > 1.0e-6));
}

#[test]
fn generation_capacity_can_be_retried_after_reclaim() {
    let first = compile(&definition_json("saw", -14.0));
    let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
    let mut runtime = InstrumentRuntime::new(Arc::clone(&first));
    runtime.prepare(spec).expect("runtime prepares");
    runtime.activate().expect("runtime activates");
    let _ = process(
        &mut runtime,
        0,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 110,
            },
        }],
    );

    let mut last_update = None;
    let gains = [-8.0, -9.0, -10.0, -11.0, -12.0, -13.0, -14.0, -15.0];
    for (index, gain) in gains.into_iter().enumerate() {
        let definition = definition_json("saw", gain);
        let compiled = compile(&definition);
        let mut update = InstrumentRuntime::prepare_update(Arc::clone(&compiled), spec)
            .expect("update prepares");
        let result = runtime.publish_prepared(&mut update);
        if index < 7 {
            result.expect("generation publishes before capacity is exhausted");
        } else {
            assert_eq!(result, Err(PublishError::CapacityExceeded));
            last_update = Some(update);
        }
    }
    let mut last_update = last_update.expect("capacity update is retained");
    runtime.reset().expect("reset reclaims retired generations");
    while runtime.take_reclaimable().is_some() {}
    runtime
        .publish_prepared(&mut last_update)
        .expect("capacity update retries after reclaim");
    assert_eq!(runtime.generation_id().get(), 9);
    let audio = process(
        &mut runtime,
        0,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 67,
                velocity: 110,
            },
        }],
    );
    assert!(audio.iter().flatten().all(|sample| sample.is_finite()));
    assert!(audio[0].iter().any(|sample| sample.abs() > 1.0e-6));
}

#[test]
fn stale_revision_is_ignored_but_current_invalid_handle_is_fatal() {
    let first = compile(&definition_json("saw", -14.0));
    let second = compile(&definition_json("sine", -8.0));
    let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
    let mut runtime = InstrumentRuntime::new(Arc::clone(&first));
    runtime.prepare(spec).expect("runtime prepares");
    runtime.activate().expect("runtime activates");
    let mut update =
        InstrumentRuntime::prepare_update(Arc::clone(&second), spec).expect("update prepares");
    runtime
        .publish_prepared(&mut update)
        .expect("update publishes");

    let mut left = vec![0.0; 64];
    let mut right = vec![0.0; 64];
    let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
    let result = runtime.process(ProcessBlock {
        frames: 64,
        context: ProcessContext {
            absolute_frame: 0,
            tempo_bpm: 120.0,
            beat_position: 0.0,
            bar_position: 0.0,
            time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
            transport_state: TransportState::Playing,
        },
        events: &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::ParameterChange {
                catalog_revision: second.parameter_catalog_revision(),
                parameter: sonalloy_core::ParameterHandle::from_index(usize::MAX),
                normalized: 0.5,
            },
        }],
        input: &[],
        output: &mut output,
    });
    assert!(matches!(
        result,
        Err(sonalloy_core::ProcessError::ParameterHandleOutOfRange { .. })
    ));
    assert_eq!(runtime.state(), RuntimeState::Faulted);
    runtime.prepare(spec).expect("faulted runtime re-prepares");
    assert_eq!(runtime.state(), RuntimeState::Prepared);
}

#[test]
fn lifecycle_requires_activation_and_rejects_processing_after_deactivation() {
    let compiled = compile(&definition_json("saw", -14.0));
    let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
    let mut runtime = InstrumentRuntime::new(compiled);
    runtime.prepare(spec).expect("runtime prepares");

    let mut left = vec![1.0; 64];
    let mut right = vec![1.0; 64];
    let result = {
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        runtime.process(ProcessBlock {
            frames: 64,
            context: ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
                beat_position: 0.0,
                bar_position: 0.0,
                time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
                transport_state: TransportState::Playing,
            },
            events: &[],
            input: &[],
            output: &mut output,
        })
    };
    assert_eq!(result, Err(sonalloy_core::ProcessError::NotActive));
    assert!(left.iter().all(|sample| *sample == 0.0));
    runtime.activate().expect("runtime activates");
    {
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        runtime
            .process(ProcessBlock {
                frames: 64,
                context: ProcessContext {
                    absolute_frame: 0,
                    tempo_bpm: 120.0,
                    beat_position: 0.0,
                    bar_position: 0.0,
                    time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
                    transport_state: TransportState::Playing,
                },
                events: &[],
                input: &[],
                output: &mut output,
            })
            .expect("active runtime processes");
    }
    runtime.deactivate().expect("runtime deactivates");
    let result = {
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        runtime.process(ProcessBlock {
            frames: 64,
            context: ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
                beat_position: 0.0,
                bar_position: 0.0,
                time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
                transport_state: TransportState::Playing,
            },
            events: &[],
            input: &[],
            output: &mut output,
        })
    };
    assert_eq!(result, Err(sonalloy_core::ProcessError::NotActive));
    assert_eq!(runtime.state(), RuntimeState::Prepared);
}
