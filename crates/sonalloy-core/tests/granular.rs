use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use approx::assert_relative_eq;
use sonalloy_core::{
    AdsrDefinition, AssetReference, CompileContext, DiagnosticCode, GeneratorDefinition,
    GranularDefinition, InstrumentDefinition, InstrumentProcessor, InstrumentRuntime,
    ParameterUnit, ProcessBlock, ProcessContext, ProcessEvent, ProcessEventKind, ProcessSpec,
    RenderRequest, SampleRegionDefinition, ScheduledEvent, compile_instrument, render_instrument,
};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

fn fixture_path() -> PathBuf {
    let index = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "sonalloy-granular-{}-{index}.wav",
        std::process::id()
    ))
}

fn write_stereo_fixture(path: &Path) {
    let frames = 48_000_usize;
    let payload_len = u32::try_from(frames * 2 * 2).expect("fixture payload fits RIFF");
    let mut bytes = Vec::with_capacity(44 + payload_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + payload_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&(48_000_u32 * 2 * 2).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&payload_len.to_le_bytes());
    for frame in 0..frames {
        #[allow(clippy::cast_precision_loss)]
        let time = frame as f32 / 48_000.0;
        let left = (std::f32::consts::TAU * 220.0 * time).sin() * 0.6;
        let right = (std::f32::consts::TAU * 330.0 * time).sin() * 0.4;
        #[allow(clippy::cast_possible_truncation)]
        {
            bytes.extend_from_slice(&((left * 30_000.0) as i16).to_le_bytes());
            bytes.extend_from_slice(&((right * 30_000.0) as i16).to_le_bytes());
        }
    }
    std::fs::write(path, bytes).expect("fixture WAV writes");
}

fn definition(asset_path: String) -> InstrumentDefinition {
    let reference = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/basic-poly-synth.json");
    let mut definition: InstrumentDefinition = serde_json::from_str(
        &std::fs::read_to_string(reference).expect("reference Definition exists"),
    )
    .expect("reference Definition parses");
    definition.layers[0].gain_db = 0.0;
    definition.layers[0].envelope = AdsrDefinition {
        attack_seconds: 0.0,
        decay_seconds: 0.0,
        sustain_level: 1.0,
        release_seconds: 0.01,
    };
    definition.layers[0].generator = GeneratorDefinition::Granular(GranularDefinition {
        asset: AssetReference {
            path: asset_path,
            sha256: None,
        },
        root_note: 60,
        region: SampleRegionDefinition {
            start_seconds: 0.05,
            end_seconds: Some(0.9),
        },
        position: 0.5,
        grain_size: 0.05,
        density: 30.0,
        pitch: 0.0,
        randomness: 0.0,
        pan_spread: 1.0,
        seed: 8128,
    });
    definition.modulation = None;
    definition
}

fn compile(
    definition: &InstrumentDefinition,
    base_dir: &Path,
    block_size: usize,
) -> Arc<sonalloy_core::CompiledInstrument> {
    let result = compile_instrument(
        definition,
        &CompileContext {
            definition_base_dir: base_dir.to_path_buf(),
            process_spec: ProcessSpec::new(48_000.0, block_size, 2).expect("valid process spec"),
        },
    );
    result.instrument.expect("Granular Definition compiles")
}

fn render(
    definition: &InstrumentDefinition,
    base_dir: &Path,
    block_size: usize,
) -> sonalloy_core::RenderedAudio {
    let instrument = compile(definition, base_dir, block_size);
    render_instrument(
        instrument,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size,
            duration_frames: 8_192,
            tail_frames: 0,
        },
        &[ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }],
    )
    .expect("Granular render succeeds")
}

fn process_runtime(
    runtime: &mut InstrumentRuntime,
    frames: usize,
    absolute_frame: u64,
    events: &[ProcessEvent],
) -> Vec<Vec<f32>> {
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
    runtime
        .process(ProcessBlock {
            frames,
            context: ProcessContext {
                absolute_frame,
                tempo_bpm: 120.0,
            },
            events,
            output: &mut output,
        })
        .expect("Granular process succeeds");
    vec![left, right]
}

#[test]
fn granular_definition_exposes_all_dynamic_parameters_with_native_units() {
    let path = fixture_path();
    write_stereo_fixture(&path);
    let base_dir = path.parent().expect("fixture has a parent");
    let definition = definition(path.file_name().unwrap().to_string_lossy().into_owned());
    let compiled = compile(&definition, base_dir, 257);

    let grain_size = compiled
        .parameter_descriptor(
            compiled
                .parameter_handle("layer.body.generator.grain_size")
                .expect("grain size handle"),
        )
        .expect("grain size descriptor");
    let density = compiled
        .parameter_descriptor(
            compiled
                .parameter_handle("layer.body.generator.grain_density")
                .expect("density handle"),
        )
        .expect("density descriptor");
    assert_eq!(grain_size.unit, ParameterUnit::Seconds);
    assert_relative_eq!(grain_size.min, 0.005);
    assert_relative_eq!(grain_size.max, 0.5);
    assert_eq!(density.unit, ParameterUnit::PerSecond);
    assert_relative_eq!(density.min, 1.0);
    assert_relative_eq!(density.max, 100.0);
    assert_eq!(
        compiled.layers[0].generator.output_mode(),
        sonalloy_core::GeneratorOutputMode::Stereo
    );
    let sonalloy_core::compiler::CompiledGenerator::Granular(granular) =
        &compiled.layers[0].generator
    else {
        panic!("definition must compile to Granular");
    };
    assert_eq!(granular.start_frame, 2_400);
    assert_eq!(granular.end_frame, 43_200);
    assert_eq!(granular.grain_pool_limit, 64);
}

#[test]
fn granular_render_is_stereo_finite_and_block_size_independent() {
    let path = fixture_path();
    write_stereo_fixture(&path);
    let base_dir = path.parent().expect("fixture has a parent");
    let definition = definition(path.file_name().unwrap().to_string_lossy().into_owned());
    let reference = render(&definition, base_dir, 257);
    let candidate = render(&definition, base_dir, 64);

    assert!(
        reference
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert!(
        reference.channels[0]
            .iter()
            .any(|sample| sample.abs() > 1.0e-4)
    );
    assert!(
        reference.channels[0]
            .iter()
            .zip(&reference.channels[1])
            .any(|(left, right)| (left - right).abs() > 1.0e-4)
    );
    for (left, candidate_left) in reference.channels[0].iter().zip(&candidate.channels[0]) {
        assert_relative_eq!(*left, *candidate_left, epsilon = 1.0e-6);
    }
    for (right, candidate_right) in reference.channels[1].iter().zip(&candidate.channels[1]) {
        assert_relative_eq!(*right, *candidate_right, epsilon = 1.0e-6);
    }
}

#[test]
fn granular_invalid_parameter_is_rejected_before_asset_loading() {
    let definition = definition("missing.wav".to_owned());
    let mut invalid = definition.clone();
    let GeneratorDefinition::Granular(granular) = &mut invalid.layers[0].generator else {
        panic!("fixture must be granular");
    };
    granular.grain_size = 0.001;
    let diagnostics = invalid.validate();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidGrainParameter
            && diagnostic.path.as_deref() == Some("layers[0].generator.granular.grain_size")
    }));
}

#[test]
fn granular_region_outside_prepared_asset_is_rejected_at_compile() {
    let path = fixture_path();
    write_stereo_fixture(&path);
    let base_dir = path.parent().expect("fixture has a parent");
    let mut definition = definition(path.file_name().unwrap().to_string_lossy().into_owned());
    let GeneratorDefinition::Granular(granular) = &mut definition.layers[0].generator else {
        panic!("fixture must be granular");
    };
    granular.region.end_seconds = Some(2.0);
    let result = compile_instrument(
        &definition,
        &sonalloy_core::CompileContext {
            definition_base_dir: base_dir.to_path_buf(),
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"),
        },
    );
    assert!(result.instrument.is_none());
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidGrainRegion
            && diagnostic.path.as_deref() == Some("layers[0].generator.granular.region")
    }));
}

#[test]
fn granular_missing_asset_does_not_disable_other_layers() {
    let definition_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/basic-poly-synth.json");
    let reference: InstrumentDefinition = serde_json::from_str(
        &std::fs::read_to_string(definition_path).expect("reference Definition exists"),
    )
    .expect("reference Definition parses");
    let path = fixture_path();
    let base_dir = path.parent().expect("fixture has a parent");
    let mut definition = definition("missing.wav".to_owned());
    let mut oscillator = reference.layers[0].clone();
    oscillator.id = "body_oscillator".to_owned();
    definition.layers.push(oscillator);
    let compiled = compile(&definition, base_dir, 257);
    let mut runtime = compiled.instantiate();
    runtime
        .prepare(ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"))
        .expect("unavailable Granular layer does not prevent prepare");
    let audio = process_runtime(
        &mut runtime,
        257,
        0,
        &[ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }],
    );
    assert!(audio[0].iter().any(|sample| sample.abs() > 1.0e-4));
}

#[test]
fn granular_reset_restarts_the_same_render() {
    let path = fixture_path();
    write_stereo_fixture(&path);
    let base_dir = path.parent().expect("fixture has a parent");
    let definition = definition(path.file_name().unwrap().to_string_lossy().into_owned());
    let compiled = compile(&definition, base_dir, 257);
    let mut runtime = compiled.instantiate();
    runtime
        .prepare(ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"))
        .expect("Granular runtime prepares");
    let event = [ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 100,
        },
    }];
    let first = process_runtime(&mut runtime, 257, 0, &event);
    runtime.reset().expect("Granular runtime resets");
    let second = process_runtime(&mut runtime, 257, 0, &event);
    for (first, second) in first[0].iter().zip(&second[0]) {
        assert_relative_eq!(*first, *second, epsilon = 1.0e-6);
    }
    for (first, second) in first[1].iter().zip(&second[1]) {
        assert_relative_eq!(*first, *second, epsilon = 1.0e-6);
    }
}

#[test]
fn granular_voice_stealing_restarts_grain_state() {
    let path = fixture_path();
    write_stereo_fixture(&path);
    let base_dir = path.parent().expect("fixture has a parent");
    let mut definition = definition(path.file_name().unwrap().to_string_lossy().into_owned());
    definition.performance.polyphony = 1;
    let compiled = compile(&definition, base_dir, 257);
    let mut stolen = compiled.instantiate();
    let mut direct = compiled.instantiate();
    let spec = ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec");
    stolen.prepare(spec).expect("stolen runtime prepares");
    direct.prepare(spec).expect("direct runtime prepares");
    let first_note = [ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 100,
        },
    }];
    let _ = process_runtime(&mut stolen, 64, 0, &first_note);
    let second_note = [ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 2,
            note_number: 60,
            velocity: 100,
        },
    }];
    let stolen_audio = process_runtime(&mut stolen, 256, 64, &second_note);
    let direct_audio = process_runtime(&mut direct, 16, 0, &second_note);
    assert!(stolen_audio[0].iter().all(|sample| sample.is_finite()));
    assert!(stolen_audio[0].iter().any(|sample| sample.abs() > 1.0e-4));
    for (stolen_sample, direct_sample) in stolen_audio[0][240..].iter().zip(&direct_audio[0]) {
        assert_relative_eq!(*stolen_sample, *direct_sample, epsilon = 1.0e-5);
    }
}
