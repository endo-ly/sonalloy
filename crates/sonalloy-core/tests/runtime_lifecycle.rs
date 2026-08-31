use std::path::PathBuf;
use std::sync::Arc;

use sonalloy_core::{
    CompileContext, InstrumentProcessor, InstrumentRuntime, ProcessBlock, ProcessContext,
    ProcessEvent, ProcessEventKind, ProcessSpec, PublishError, RuntimeState, TransportState,
    compile_instrument,
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
    compile_instrument(
        definition,
        &CompileContext {
            definition_base_dir: PathBuf::from("testdata/instruments"),
            process_spec: ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec"),
        },
    )
    .instrument
    .expect("definition compiles")
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
