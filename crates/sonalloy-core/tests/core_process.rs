use std::path::PathBuf;
use std::sync::Arc;

use approx::assert_relative_eq;
use sonalloy_core::{
    CompileContext, DiagnosticCode, InstrumentDefinition, InstrumentProcessor, ModulationCurve,
    ModulationDefinition, ModulationRouteDefinition, ModulationSourceDefinition, ProcessBlock,
    ProcessContext, ProcessEventKind, ProcessSpec, RandomDefinition, RenderRequest, ScheduledEvent,
    SineRuntime, compile_instrument, render_instrument,
};

fn render_sine_blocks(block_size: usize) -> Vec<Vec<f32>> {
    let spec = ProcessSpec::new(48_000.0, block_size, 2).expect("valid process spec");
    let mut runtime = SineRuntime::new(440.0).expect("valid sine runtime");
    runtime.prepare(spec).expect("runtime preparation");

    let mut channels = vec![vec![0.0_f32; 48_000], vec![0.0_f32; 48_000]];
    let mut offset = 0_usize;
    while offset < channels[0].len() {
        let frames = (channels[0].len() - offset).min(block_size);
        let end = offset + frames;
        let (left, right) = channels.split_at_mut(1);
        let mut output: [&mut [f32]; 2] = [&mut left[0][offset..end], &mut right[0][offset..end]];
        runtime
            .process(ProcessBlock {
                frames,
                context: ProcessContext {
                    absolute_frame: offset as u64,
                    tempo_bpm: 120.0,
                },
                events: &[],
                output: &mut output,
            })
            .expect("runtime process");
        offset = end;
    }
    channels
}

#[test]
fn sine_runtime_is_stable_across_block_sizes() {
    let reference = render_sine_blocks(64);
    for block_size in [257, 1024] {
        let candidate = render_sine_blocks(block_size);
        assert_eq!(candidate[0].len(), 48_000);
        assert_eq!(candidate[1].len(), 48_000);
        assert!(candidate.iter().flatten().all(|sample| sample.is_finite()));
        for (left, right) in reference[0].iter().zip(candidate[0].iter()) {
            assert_relative_eq!(*left, *right, epsilon = 1.0e-6);
        }
        for (left, right) in candidate[0].iter().zip(candidate[1].iter()) {
            assert_relative_eq!(*left, *right, epsilon = 1.0e-7);
        }
    }
}

#[test]
fn sine_runtime_reset_restarts_signal() {
    let spec = ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec");
    let mut runtime = SineRuntime::new(440.0).expect("valid sine runtime");
    runtime.prepare(spec).expect("runtime preparation");
    let mut first_left = [0.0_f32; 128];
    let mut first_right = [0.0_f32; 128];
    let mut second_left = [0.0_f32; 128];
    let mut second_right = [0.0_f32; 128];
    let mut output: [&mut [f32]; 2] = [&mut first_left, &mut first_right];
    runtime
        .process(ProcessBlock {
            frames: 128,
            context: ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
            },
            events: &[],
            output: &mut output,
        })
        .expect("first process");
    runtime.reset().expect("runtime reset");
    let mut reset_output: [&mut [f32]; 2] = [&mut second_left, &mut second_right];
    runtime
        .process(ProcessBlock {
            frames: 128,
            context: ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
            },
            events: &[],
            output: &mut reset_output,
        })
        .expect("second process");
    for (first, second) in first_left.iter().zip(second_left.iter()) {
        assert_relative_eq!(*first, *second, epsilon = 1.0e-7);
    }
    for (first, second) in first_right.iter().zip(second_right.iter()) {
        assert_relative_eq!(*first, *second, epsilon = 1.0e-7);
    }
}

fn definition() -> InstrumentDefinition {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/basic-poly-synth.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("reference Definition exists"))
        .expect("reference Definition parses")
}

fn render_instrument_blocks(block_size: usize) -> sonalloy_core::RenderedAudio {
    let definition = definition();
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: ".".into(),
            process_spec: ProcessSpec::new(48_000.0, block_size, 2).expect("valid spec"),
        },
    );
    let instrument = result.instrument.expect("reference Definition compiles");
    let events = [
        ScheduledEvent {
            absolute_frame: 100,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        },
        ScheduledEvent {
            absolute_frame: 1_100,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        },
    ];
    render_instrument(
        Arc::clone(&instrument),
        RenderRequest {
            sample_rate: 48_000.0,
            block_size,
            duration_frames: 1_200,
            tail_frames: 0,
        },
        &events,
    )
    .expect("instrument render succeeds")
}

#[test]
fn reference_definition_compiles_and_renders_stereo() {
    let audio = render_instrument_blocks(257);
    assert_eq!(audio.sample_rate, 48_000);
    assert_eq!(audio.channels.len(), 2);
    assert_eq!(audio.frames(), 1_200);
    assert!(
        audio
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert!(
        audio
            .channels
            .iter()
            .flatten()
            .any(|sample| sample.abs() > 0.01)
    );
}

#[test]
fn moving_hybrid_routes_cover_the_reference_signal_paths() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/moving-hybrid-pad.json");
    let definition: InstrumentDefinition = serde_json::from_str(
        &std::fs::read_to_string(path).expect("moving hybrid Definition exists"),
    )
    .expect("moving hybrid Definition parses");
    let routes = &definition
        .modulation
        .as_ref()
        .expect("moving hybrid modulation")
        .routes;
    for expected in [
        ("velocity", "layer.attack.gain"),
        ("velocity", "layer.body.gain"),
        ("voice_pan", "layer.attack.pan"),
        ("filter_motion", "layer.attack.pan"),
        ("pitch_motion", "layer.body.tuning"),
        ("filter_motion", "voice.filter.cutoff"),
        ("key_tracking", "voice.filter.cutoff"),
        ("mod_wheel", "voice.filter.cutoff"),
    ] {
        assert!(
            routes
                .iter()
                .any(|route| (route.source.as_str(), route.target.as_str()) == expected),
            "missing route {} -> {}",
            expected.0,
            expected.1
        );
    }
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/instruments"),
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"),
        },
    );
    assert!(result.instrument.is_some(), "moving hybrid should compile");
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.severity != sonalloy_core::DiagnosticSeverity::Error })
    );
}

#[test]
fn expressive_reference_renders_at_supported_sample_rates() {
    let definition_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/expressive-hybrid-lead.json");
    let definition: InstrumentDefinition = serde_json::from_str(
        &std::fs::read_to_string(&definition_path).expect("expressive Definition exists"),
    )
    .expect("expressive Definition parses");
    let definition_base_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/instruments");
    for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
        let process_spec = ProcessSpec::new(sample_rate, 257, 2).expect("valid process spec");
        let instrument = compile_instrument(
            &definition,
            &CompileContext {
                definition_base_dir: definition_base_dir.clone(),
                process_spec,
            },
        )
        .instrument
        .expect("expressive Definition compiles");
        let audio = render_instrument(
            instrument,
            RenderRequest {
                sample_rate,
                block_size: 257,
                duration_frames: 4_096,
                tail_frames: 0,
            },
            &[
                ScheduledEvent {
                    absolute_frame: 0,
                    kind: ProcessEventKind::NoteOn {
                        note_id: 1,
                        note_number: 60,
                        velocity: 112,
                    },
                },
                ScheduledEvent {
                    absolute_frame: 2_048,
                    kind: ProcessEventKind::NoteOff { note_id: 1 },
                },
            ],
        )
        .expect("reference render succeeds");
        assert!(
            audio
                .channels
                .iter()
                .flatten()
                .all(|sample| sample.is_finite())
        );
        assert!(
            audio
                .channels
                .iter()
                .flatten()
                .any(|sample| sample.abs() > 0.01)
        );
    }
}

#[test]
fn absolute_event_timing_is_stable_across_block_sizes() {
    let reference = render_instrument_blocks(64);
    for candidate in [
        render_instrument_blocks(257),
        render_instrument_blocks(1024),
    ] {
        for (left, right) in reference.channels[0].iter().zip(&candidate.channels[0]) {
            assert_relative_eq!(*left, *right, epsilon = 1.0e-5);
        }
        for (left, right) in reference.channels[1].iter().zip(&candidate.channels[1]) {
            assert_relative_eq!(*left, *right, epsilon = 1.0e-5);
        }
    }
}

fn render_expressive_blocks(block_size: usize) -> sonalloy_core::RenderedAudio {
    let definition_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/expressive-hybrid-lead.json");
    let definition: InstrumentDefinition = serde_json::from_str(
        &std::fs::read_to_string(&definition_path).expect("expressive Definition exists"),
    )
    .expect("expressive Definition parses");
    let definition_base_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/instruments");
    let instrument = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir,
            process_spec: ProcessSpec::new(48_000.0, block_size, 2).expect("valid spec"),
        },
    )
    .instrument
    .expect("expressive Definition compiles");
    let events = [
        ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 112,
            },
        },
        ScheduledEvent {
            absolute_frame: 12_000,
            kind: ProcessEventKind::ParameterChange {
                parameter: instrument
                    .parameter_handle("voice.filter.cutoff")
                    .expect("filter cutoff parameter exists"),
                normalized: 0.62,
            },
        },
        ScheduledEvent {
            absolute_frame: 24_000,
            kind: ProcessEventKind::ModWheel { value: 0.8 },
        },
        ScheduledEvent {
            absolute_frame: 36_000,
            kind: ProcessEventKind::PitchBend { value: 0.5 },
        },
        ScheduledEvent {
            absolute_frame: 48_000,
            kind: ProcessEventKind::Aftertouch { value: 0.75 },
        },
        ScheduledEvent {
            absolute_frame: 72_000,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        },
    ];
    render_instrument(
        instrument,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size,
            duration_frames: 80_000,
            tail_frames: 0,
        },
        &events,
    )
    .expect("expressive render succeeds")
}

#[test]
fn dynamic_events_are_stable_across_block_sizes() {
    let reference = render_expressive_blocks(64);
    for candidate in [
        render_expressive_blocks(257),
        render_expressive_blocks(1024),
    ] {
        assert!(
            candidate
                .channels
                .iter()
                .flatten()
                .all(|sample| sample.is_finite())
        );
        for (left, right) in reference.channels[0].iter().zip(&candidate.channels[0]) {
            assert_relative_eq!(*left, *right, epsilon = 1.0e-3);
        }
        for (left, right) in reference.channels[1].iter().zip(&candidate.channels[1]) {
            assert_relative_eq!(*left, *right, epsilon = 1.0e-3);
        }
    }
}

fn absolute_energy(samples: &[f32]) -> f64 {
    samples
        .iter()
        .map(|sample| f64::from(sample.abs()))
        .sum::<f64>()
        / f64::from(u32::try_from(samples.len()).expect("sample count fits in u32"))
}

#[test]
fn parameter_change_updates_the_compiled_target_after_smoothing() {
    let instrument = compile_instrument(
        &definition(),
        &CompileContext {
            definition_base_dir: ".".into(),
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid spec"),
        },
    )
    .instrument
    .expect("reference Definition compiles");
    let gain = instrument
        .parameter_handle("layer.body.gain")
        .expect("gain parameter exists");
    let events = [
        ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 11,
                note_number: 60,
                velocity: 100,
            },
        },
        ScheduledEvent {
            absolute_frame: 128,
            kind: ProcessEventKind::ParameterChange {
                parameter: gain,
                normalized: 1.0,
            },
        },
    ];
    let audio = render_instrument(
        instrument,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 768,
            tail_frames: 0,
        },
        &events,
    )
    .expect("dynamic render succeeds");
    assert!(
        audio
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    let before = absolute_energy(&audio.channels[0][64..128]);
    let after = absolute_energy(&audio.channels[0][512..576]);
    assert!(
        after > before * 3.0,
        "parameter change did not reach the target"
    );
}

#[test]
fn deterministic_random_route_repeats_across_runtime_instances() {
    let mut source = definition();
    source.modulation = Some(ModulationDefinition {
        sources: vec![ModulationSourceDefinition::Random(RandomDefinition {
            id: "random_pan".to_owned(),
            seed: 42,
        })],
        routes: vec![ModulationRouteDefinition {
            source: "random_pan".to_owned(),
            target: "layer.body.pan".to_owned(),
            amount: 1.0,
            curve: ModulationCurve::Linear,
        }],
    });
    let context = CompileContext {
        definition_base_dir: ".".into(),
        process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid spec"),
    };
    let compiled = compile_instrument(&source, &context)
        .instrument
        .expect("random route compiles");
    let events = [ScheduledEvent {
        absolute_frame: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 17,
            note_number: 60,
            velocity: 100,
        },
    }];
    let request = RenderRequest {
        sample_rate: 48_000.0,
        block_size: 257,
        duration_frames: 256,
        tail_frames: 0,
    };
    let first = render_instrument(Arc::clone(&compiled), request, &events).expect("first render");
    let second = render_instrument(compiled, request, &events).expect("second render");
    assert_eq!(first, second);
}

fn hybrid_definition() -> InstrumentDefinition {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/metallic-hybrid.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("hybrid Definition exists"))
        .expect("hybrid Definition parses")
}

#[test]
fn hybrid_compiles_two_layers_and_prepares_the_sample() {
    let definition = hybrid_definition();
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/instruments");
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir,
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"),
        },
    );
    let instrument = result.instrument.expect("hybrid compiles");
    assert_eq!(instrument.layers.len(), 2);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == DiagnosticCode::AssetResampled })
    );
    match &instrument.layers[0].generator {
        sonalloy_core::compiler::CompiledGenerator::Sample(sample) => {
            assert!(sample.enabled);
            assert!(sample.source.is_some());
        }
        sonalloy_core::compiler::CompiledGenerator::Oscillator(_) => {
            panic!("attack layer must be a sample")
        }
    }
}

#[test]
fn hybrid_layers_share_one_voice_and_render_finite_audio() {
    let definition = hybrid_definition();
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/instruments");
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir,
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"),
        },
    );
    let instrument = result.instrument.expect("hybrid compiles");
    let events = [
        ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 7,
                note_number: 60,
                velocity: 110,
            },
        },
        ScheduledEvent {
            absolute_frame: 4_800,
            kind: ProcessEventKind::NoteOff { note_id: 7 },
        },
    ];
    let audio = render_instrument(
        std::sync::Arc::clone(&instrument),
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 24_000,
            tail_frames: 24_000,
        },
        &events,
    )
    .expect("hybrid render succeeds");
    assert_eq!(audio.channels.len(), 2);
    assert!(
        audio
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert!(
        audio
            .channels
            .iter()
            .flatten()
            .any(|sample| sample.abs() > 0.01)
    );
}

#[test]
fn missing_sample_keeps_the_oscillator_available() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/metallic-hybrid-missing-asset.json");
    let definition: InstrumentDefinition =
        serde_json::from_str(&std::fs::read_to_string(path).expect("missing fixture exists"))
            .expect("missing fixture parses");
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/instruments");
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir,
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"),
        },
    );
    let instrument = result.instrument.expect("missing asset is recoverable");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == DiagnosticCode::AssetNotFound })
    );
    let audio = render_instrument(
        std::sync::Arc::clone(&instrument),
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 1_024,
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
    .expect("oscillator fallback renders");
    assert!(
        audio
            .channels
            .iter()
            .flatten()
            .any(|sample| sample.abs() > 0.01)
    );
}

#[test]
fn sample_without_hash_is_enabled_with_a_warning() {
    let mut definition = hybrid_definition();
    match &mut definition.layers[0].generator {
        sonalloy_core::GeneratorDefinition::Sample(sample) => sample.asset.sha256 = None,
        sonalloy_core::GeneratorDefinition::Oscillator(_) => {
            panic!("attack layer must be a sample")
        }
    }
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/instruments");
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir,
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"),
        },
    );
    let instrument = result.instrument.expect("sample without hash compiles");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::AssetHashMissing)
    );
    match &instrument.layers[0].generator {
        sonalloy_core::compiler::CompiledGenerator::Sample(sample) => {
            assert!(sample.enabled);
            assert!(sample.source.is_some());
        }
        sonalloy_core::compiler::CompiledGenerator::Oscillator(_) => {
            panic!("attack layer must be a sample")
        }
    }
}

#[test]
fn absolute_sample_path_is_enabled_with_a_warning() {
    let mut definition = hybrid_definition();
    let asset_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/assets/metal-hit.wav")
        .canonicalize()
        .expect("reference asset exists");
    match &mut definition.layers[0].generator {
        sonalloy_core::GeneratorDefinition::Sample(sample) => {
            sample.asset.path = asset_path.to_string_lossy().into_owned();
        }
        sonalloy_core::GeneratorDefinition::Oscillator(_) => {
            panic!("attack layer must be a sample")
        }
    }
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/instruments");
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir,
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid spec"),
        },
    );
    assert!(result.instrument.is_some());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::AssetAbsolutePath)
    );
}

#[test]
fn mismatched_sample_hash_disables_only_the_sample_layer() {
    let mut definition = hybrid_definition();
    match &mut definition.layers[0].generator {
        sonalloy_core::GeneratorDefinition::Sample(sample) => {
            sample.asset.sha256 = Some("00".repeat(32));
        }
        sonalloy_core::GeneratorDefinition::Oscillator(_) => {
            panic!("attack layer must be a sample")
        }
    }
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/instruments");
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir,
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"),
        },
    );
    let instrument = result
        .instrument
        .expect("mismatched hash partially compiles");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::AssetHashMismatch)
    );
    match &instrument.layers[0].generator {
        sonalloy_core::compiler::CompiledGenerator::Sample(sample) => {
            assert!(!sample.enabled);
            assert!(sample.source.is_none());
        }
        sonalloy_core::compiler::CompiledGenerator::Oscillator(_) => {
            panic!("attack layer must be a sample")
        }
    }
    let audio = render_instrument(
        std::sync::Arc::clone(&instrument),
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 1_024,
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
    .expect("oscillator remains renderable");
    assert!(
        audio
            .channels
            .iter()
            .flatten()
            .any(|sample| sample.abs() > 0.01)
    );
}
