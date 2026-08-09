use std::path::PathBuf;
use std::sync::Arc;

use approx::assert_relative_eq;
use sonalloy_core::{
    AdsrDefinition, AssetReference, CompileContext, DiagnosticCode, DriveProcessorDefinition,
    GeneratorDefinition, HardSyncDefinition, InstrumentDefinition, InstrumentProcessor,
    InstrumentRuntime, LfoDefinition, LfoWaveform, ModulationCurve, ModulationDefinition,
    ModulationRouteDefinition, ModulationSourceDefinition, NoiseColor, NoiseDefinition,
    OscillatorDefinition, OscillatorWaveform, ProcessBlock, ProcessContext, ProcessEvent,
    ProcessEventKind, ProcessSpec, ProcessorDefinition, RandomDefinition, RenderRequest,
    SampleZoneDefinition, SampleZonePlaybackDefinition, ScheduledEvent, SineRuntime,
    UnisonDefinition, WaveshapingDefinition, compile_instrument, render_instrument,
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

fn basic_generator_definition() -> InstrumentDefinition {
    let mut value = definition();
    value.layers[0].gain_db = 0.0;
    value.layers[0].envelope = AdsrDefinition {
        attack_seconds: 0.0,
        decay_seconds: 0.0,
        sustain_level: 1.0,
        release_seconds: 0.01,
    };
    value
}

fn noise_definition(color: NoiseColor, correlation: f32, pan: f32) -> InstrumentDefinition {
    let mut value = basic_generator_definition();
    value.layers[0].pan = pan;
    value.layers[0].generator = GeneratorDefinition::Noise(NoiseDefinition {
        color,
        seed: 7,
        stereo_correlation: correlation,
    });
    value
}

fn pulse_definition(with_modulation: bool) -> InstrumentDefinition {
    let mut value = basic_generator_definition();
    value.layers[0].generator = GeneratorDefinition::Oscillator(OscillatorDefinition {
        waveform: OscillatorWaveform::Pulse { pulse_width: 0.25 },
        phase_reset: true,
        phase: 0.0,
        hard_sync: None,
        waveshaping: None,
        phase_distortion: None,
        wavefold: None,
        feedback: None,
        unison: None,
    });
    if with_modulation {
        value.modulation = Some(ModulationDefinition {
            sources: vec![ModulationSourceDefinition::Lfo(LfoDefinition {
                id: "pwm_lfo".to_owned(),
                waveform: LfoWaveform::Sine,
                rate_hz: 2.0,
                phase: 0.0,
            })],
            routes: vec![ModulationRouteDefinition {
                source: "pwm_lfo".to_owned(),
                target: "layer.body.generator.pulse_width".to_owned(),
                amount: 0.5,
                curve: ModulationCurve::Linear,
            }],
        });
    }
    value
}

fn render_basic_generator(
    definition: &InstrumentDefinition,
    block_size: usize,
    duration_frames: usize,
) -> sonalloy_core::RenderedAudio {
    render_basic_generator_at_note(definition, block_size, duration_frames, 60)
}

fn render_basic_generator_at_note(
    definition: &InstrumentDefinition,
    block_size: usize,
    duration_frames: usize,
    note_number: u8,
) -> sonalloy_core::RenderedAudio {
    let instrument = compile_instrument(
        definition,
        &CompileContext {
            definition_base_dir: ".".into(),
            process_spec: ProcessSpec::new(48_000.0, block_size, 2).expect("valid process spec"),
        },
    )
    .instrument
    .expect("basic generator compiles");
    render_instrument(
        instrument,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size,
            duration_frames: u64::try_from(duration_frames).expect("duration fits in u64"),
            tail_frames: 0,
        },
        &[
            ScheduledEvent {
                absolute_frame: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 11,
                    note_number,
                    velocity: 100,
                },
            },
            ScheduledEvent {
                absolute_frame: u64::try_from(duration_frames / 2)
                    .expect("event frame fits in u64"),
                kind: ProcessEventKind::NoteOff { note_id: 11 },
            },
        ],
    )
    .expect("basic generator render succeeds")
}

fn complex_oscillator_definition(
    hard_sync: bool,
    waveshaping: bool,
    unison_voices: Option<u8>,
) -> InstrumentDefinition {
    let mut value = basic_generator_definition();
    value.layers[0].generator = GeneratorDefinition::Oscillator(OscillatorDefinition {
        waveform: OscillatorWaveform::Saw,
        phase_reset: true,
        phase: 0.0,
        hard_sync: hard_sync.then_some(HardSyncDefinition { ratio: 3.0 }),
        waveshaping: waveshaping.then_some(WaveshapingDefinition { amount: 0.45 }),
        phase_distortion: None,
        wavefold: None,
        feedback: None,
        unison: unison_voices.map(|voices| UnisonDefinition {
            voices,
            detune_cents: 18.0,
            stereo_spread: 0.8,
            phase_spread: if hard_sync { 0.0 } else { 0.2 },
        }),
    });
    value
}

#[test]
fn complex_oscillator_compiles_backend_parameters_and_distributions() {
    let definition = complex_oscillator_definition(true, true, Some(5));
    assert!(definition.validate().is_empty());
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: ".".into(),
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid spec"),
        },
    );
    let instrument = result.instrument.expect("complex oscillator compiles");
    let sonalloy_core::compiler::CompiledGenerator::Oscillator(oscillator) =
        &instrument.layers[0].generator
    else {
        panic!("complex definition must compile to an oscillator");
    };
    assert_eq!(
        oscillator.backend,
        sonalloy_core::compiler::CompiledOscillatorBackend::VariableShapeSync {
            sync_ratio: instrument
                .parameter_handle("layer.body.generator.sync_ratio")
                .expect("sync ratio handle is present")
        }
    );
    assert_eq!(
        instrument.layers[0].generator.output_mode(),
        sonalloy_core::compiler::GeneratorOutputMode::Stereo
    );
    assert_eq!(oscillator.unison.position_distribution.len(), 5);
    assert_eq!(oscillator.unison.phase_distribution.len(), 5);
    for (actual, expected) in oscillator
        .unison
        .position_distribution
        .iter()
        .zip([-1.0, -0.5, 0.0, 0.5, 1.0])
    {
        assert_relative_eq!(*actual, expected, epsilon = 1.0e-6);
    }
    for (actual, expected) in oscillator
        .unison
        .phase_distribution
        .iter()
        .zip([0.0, 0.0, 0.0, 0.0, 0.0])
    {
        assert_relative_eq!(*actual, expected, epsilon = 1.0e-6);
    }
    assert_relative_eq!(oscillator.unison.normalization, 1.0 / 5.0_f32.sqrt());
    assert!(matches!(
        oscillator.backend,
        sonalloy_core::compiler::CompiledOscillatorBackend::VariableShapeSync { .. }
    ));
    assert!(oscillator.parameters.waveshape.is_some());
    assert!(oscillator.parameters.unison_detune.is_some());
    assert!(oscillator.parameters.unison_spread.is_some());
    assert!(
        instrument
            .parameter_handle("layer.body.generator.sync_ratio")
            .is_some()
    );
    assert!(
        instrument
            .parameter_handle("layer.body.generator.waveshape")
            .is_some()
    );
    assert!(
        instrument
            .parameter_handle("layer.body.generator.unison_detune")
            .is_some()
    );
    assert!(
        instrument
            .parameter_handle("layer.body.generator.unison_spread")
            .is_some()
    );
}

#[test]
fn basic_unison_compiles_phase_distribution() {
    let definition = complex_oscillator_definition(false, false, Some(5));
    assert!(definition.validate().is_empty());
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: ".".into(),
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid spec"),
        },
    );
    let instrument = result.instrument.expect("basic unison compiles");
    let sonalloy_core::compiler::CompiledGenerator::Oscillator(oscillator) =
        &instrument.layers[0].generator
    else {
        panic!("basic unison definition must compile to an oscillator");
    };
    for (actual, expected) in oscillator
        .unison
        .phase_distribution
        .iter()
        .zip([0.0, 0.04, 0.08, 0.12, 0.16])
    {
        assert_relative_eq!(*actual, expected, epsilon = 1.0e-6);
    }
}

#[test]
fn oscillator_definition_rejects_invalid_complex_combinations() {
    let mut sine_sync = complex_oscillator_definition(true, false, None);
    if let GeneratorDefinition::Oscillator(oscillator) = &mut sine_sync.layers[0].generator {
        oscillator.waveform = OscillatorWaveform::Sine;
    }
    assert!(sine_sync.validate().iter().any(|diagnostic| {
        diagnostic.path.as_deref() == Some("layers[0].generator.oscillator.hard_sync")
    }));

    let mut phase_spread = complex_oscillator_definition(true, false, Some(3));
    if let GeneratorDefinition::Oscillator(oscillator) = &mut phase_spread.layers[0].generator {
        if let Some(unison) = &mut oscillator.unison {
            unison.phase_spread = 0.2;
        }
    }
    assert!(phase_spread.validate().iter().any(|diagnostic| {
        diagnostic.path.as_deref() == Some("layers[0].generator.oscillator.unison.phase_spread")
    }));

    let mut phase = complex_oscillator_definition(true, false, None);
    if let GeneratorDefinition::Oscillator(oscillator) = &mut phase.layers[0].generator {
        oscillator.phase = 0.5;
    }
    assert!(phase.validate().iter().any(|diagnostic| {
        diagnostic.path.as_deref() == Some("layers[0].generator.oscillator.phase")
    }));

    let mut invalid_voices = complex_oscillator_definition(false, false, Some(9));
    assert!(invalid_voices.validate().iter().any(|diagnostic| {
        diagnostic.path.as_deref() == Some("layers[0].generator.oscillator.unison.voices")
    }));
    if let GeneratorDefinition::Oscillator(oscillator) = &mut invalid_voices.layers[0].generator {
        oscillator.waveshaping = Some(WaveshapingDefinition { amount: 1.1 });
    }
    assert!(invalid_voices.validate().iter().any(|diagnostic| {
        diagnostic.path.as_deref() == Some("layers[0].generator.oscillator.waveshaping.amount")
    }));
}

#[test]
fn waveshape_zero_is_an_exact_identity() {
    let baseline = complex_oscillator_definition(false, false, None);
    let mut identity = baseline.clone();
    if let GeneratorDefinition::Oscillator(oscillator) = &mut identity.layers[0].generator {
        oscillator.waveshaping = Some(WaveshapingDefinition { amount: 0.0 });
    }
    let baseline_audio = render_basic_generator(&baseline, 257, 2_048);
    let identity_audio = render_basic_generator(&identity, 257, 2_048);
    assert_eq!(
        baseline_audio.channels[0]
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>(),
        identity_audio.channels[0]
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        baseline_audio.channels[1]
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>(),
        identity_audio.channels[1]
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );
}

#[test]
fn hard_sync_and_unison_render_finite_stereo_audio_across_blocks() {
    let hard_sync = complex_oscillator_definition(true, false, Some(3));
    let unison = complex_oscillator_definition(false, true, Some(8));
    let hard_sync_audio = render_basic_generator(&hard_sync, 32, 4_096);
    let unison_audio = render_basic_generator(&unison, 257, 4_096);
    for audio in [hard_sync_audio, unison_audio] {
        assert!(
            audio
                .channels
                .iter()
                .flatten()
                .all(|sample| sample.is_finite())
        );
        assert!(
            audio.channels[0]
                .iter()
                .zip(&audio.channels[1])
                .any(|(left, right)| (left - right).abs() > 1.0e-4)
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
fn hard_sync_high_register_is_clamped_to_finite_audio() {
    let mut definition = complex_oscillator_definition(true, true, Some(8));
    if let GeneratorDefinition::Oscillator(oscillator) = &mut definition.layers[0].generator {
        oscillator.hard_sync = Some(HardSyncDefinition { ratio: 16.0 });
    }
    let audio = render_basic_generator_at_note(&definition, 257, 4_096, 127);
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
fn complex_oscillator_parameter_changes_are_block_size_independent() {
    let mut definition = complex_oscillator_definition(true, true, Some(5));
    definition.modulation = None;
    let instrument = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: ".".into(),
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid spec"),
        },
    )
    .instrument
    .expect("complex instrument compiles");
    let ratio = instrument
        .parameter_handle("layer.body.generator.sync_ratio")
        .expect("sync ratio parameter");
    let events = [
        ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        },
        ScheduledEvent {
            absolute_frame: 512,
            kind: ProcessEventKind::ParameterChange {
                parameter: ratio,
                normalized: 1.0,
            },
        },
        ScheduledEvent {
            absolute_frame: 1_536,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        },
    ];
    let render = |block_size| {
        render_instrument(
            Arc::clone(&instrument),
            RenderRequest {
                sample_rate: 48_000.0,
                block_size,
                duration_frames: 2_048,
                tail_frames: 0,
            },
            &events,
        )
        .expect("complex parameter render")
    };
    let reference = render(32);
    let candidate = render(257);
    assert_eq!(reference.frames(), candidate.frames());
    for (left, right) in reference.channels[0].iter().zip(&candidate.channels[0]) {
        assert_relative_eq!(*left, *right, epsilon = 1.0e-5);
    }
    for (left, right) in reference.channels[1].iter().zip(&candidate.channels[1]) {
        assert_relative_eq!(*left, *right, epsilon = 1.0e-5);
    }
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
fn noise_is_stereo_deterministic_and_block_size_independent() {
    let correlated_definition = noise_definition(NoiseColor::White, 1.0, 0.0);
    let correlated = render_basic_generator(&correlated_definition, 257, 2_048);
    assert!(
        correlated
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert!(
        correlated.channels[0]
            .iter()
            .any(|sample| sample.abs() > 0.01)
    );
    assert_eq!(correlated.channels[0], correlated.channels[1]);

    let independent_definition = noise_definition(NoiseColor::White, 0.0, 0.0);
    let independent = render_basic_generator(&independent_definition, 257, 2_048);
    assert!(
        independent.channels[0]
            .iter()
            .zip(&independent.channels[1])
            .any(|(left, right)| left.to_bits() != right.to_bits())
    );

    let reference =
        render_basic_generator(&noise_definition(NoiseColor::Pink, 0.4, 0.0), 32, 2_048);
    for block_size in [64, 257, 1_024] {
        let candidate = render_basic_generator(
            &noise_definition(NoiseColor::Pink, 0.4, 0.0),
            block_size,
            2_048,
        );
        for (expected, actual) in reference.channels[0].iter().zip(&candidate.channels[0]) {
            assert_relative_eq!(*expected, *actual, epsilon = 1.0e-6);
        }
        for (expected, actual) in reference.channels[1].iter().zip(&candidate.channels[1]) {
            assert_relative_eq!(*expected, *actual, epsilon = 1.0e-6);
        }
    }
    let repeated = render_basic_generator(&correlated_definition, 257, 2_048);
    assert_eq!(correlated, repeated);

    for color in [NoiseColor::White, NoiseColor::Pink, NoiseColor::Brown] {
        let audio = render_basic_generator(&noise_definition(color, 0.5, 0.0), 257, 2_048);
        assert!(
            audio
                .channels
                .iter()
                .flatten()
                .all(|sample| sample.is_finite())
        );
        assert!(audio.channels[0].iter().any(|sample| sample.abs() > 0.001));
    }
}

#[test]
fn stereo_layer_processors_and_balance_preserve_the_generator_contract() {
    let mut definition = noise_definition(NoiseColor::White, 1.0, 0.0);
    definition.layers[0].processors = vec![
        ProcessorDefinition::Filter(sonalloy_core::FilterProcessorDefinition {
            id: "tone".to_owned(),
            cutoff_hz: 8_000.0,
            resonance: 0.1,
        }),
        ProcessorDefinition::Drive(DriveProcessorDefinition {
            id: "drive".to_owned(),
            amount: 0.2,
            mix: 0.4,
        }),
    ];
    let centered = render_basic_generator(&definition, 257, 2_048);
    assert_eq!(centered.channels[0], centered.channels[1]);

    definition.layers[0].pan = -1.0;
    let left = render_basic_generator(&definition, 257, 2_048);
    assert!(left.channels[0].iter().any(|sample| sample.abs() > 0.01));
    assert!(left.channels[1].iter().all(|sample| sample.abs() < 1.0e-6));

    definition.layers[0].pan = 1.0;
    let right = render_basic_generator(&definition, 257, 2_048);
    assert!(right.channels[0].iter().all(|sample| sample.abs() < 1.0e-6));
    assert!(right.channels[1].iter().any(|sample| sample.abs() > 0.01));
}

#[test]
fn existing_lfo_modulation_controls_pulse_width() {
    let static_audio = render_basic_generator(&pulse_definition(false), 257, 4_096);
    let pwm_audio = render_basic_generator(&pulse_definition(true), 257, 4_096);
    assert!(
        pwm_audio
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    let difference = static_audio.channels[0]
        .iter()
        .zip(&pwm_audio.channels[0])
        .map(|(static_sample, pwm_sample)| f64::from((*static_sample - pwm_sample).abs()))
        .sum::<f64>();
    assert!(difference > 1.0);
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
        ("filter_motion", "voice.processor.tone.cutoff"),
        ("key_tracking", "voice.processor.tone.cutoff"),
        ("mod_wheel", "voice.processor.tone.cutoff"),
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

fn harmonic_formant_hybrid_reference() -> (InstrumentDefinition, PathBuf) {
    let definition_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/harmonic-formant-hybrid-reference.json");
    let definition: InstrumentDefinition = serde_json::from_str(
        &std::fs::read_to_string(&definition_path).expect("harmonic formant hybrid exists"),
    )
    .expect("harmonic formant hybrid parses");
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/instruments");
    (definition, base_dir)
}

fn assert_harmonic_formant_hybrid_structure(definition: &InstrumentDefinition) {
    assert_eq!(definition.layers.len(), 4);
    assert!(matches!(
        &definition.layers[0].generator,
        sonalloy_core::GeneratorDefinition::Formant(_)
    ));
    assert!(matches!(
        &definition.layers[1].generator,
        sonalloy_core::GeneratorDefinition::Additive(_)
    ));
    assert!(matches!(
        &definition.layers[2].generator,
        sonalloy_core::GeneratorDefinition::Sample(_)
    ));
    assert!(matches!(
        &definition.layers[3].generator,
        sonalloy_core::GeneratorDefinition::Noise(_)
    ));
    assert_eq!(definition.voice_processors.len(), 2);
    assert_eq!(definition.global_processors.len(), 2);
    let routes = &definition
        .modulation
        .as_ref()
        .expect("hybrid modulation")
        .routes;
    for expected in [
        (
            "vowel_motion",
            "layer.voice.generator.formant_vowel_position",
        ),
        (
            "brightness_motion",
            "layer.voice.generator.formant_spectral_tilt",
        ),
        ("formant_breath", "layer.voice.generator.formant_throat"),
        ("mod_wheel", "layer.voice.generator.formant_shift"),
        ("vowel_motion", "voice.processor.voice_tone.cutoff"),
        ("mod_wheel", "global.processor.space.mix"),
        ("aftertouch", "global.processor.echo.mix"),
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
}

fn render_harmonic_formant_hybrid(
    definition: &InstrumentDefinition,
    base_dir: &std::path::Path,
    block_size: usize,
) -> sonalloy_core::RenderedAudio {
    let result = compile_instrument(
        definition,
        &CompileContext {
            definition_base_dir: base_dir.to_path_buf(),
            process_spec: ProcessSpec::new(48_000.0, block_size, 2).expect("valid spec"),
        },
    );
    let instrument = result.instrument.expect("hybrid compiles");
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != sonalloy_core::DiagnosticSeverity::Error)
    );
    assert_eq!(instrument.layers.len(), 4);
    assert_eq!(instrument.voice_processors.len(), 2);
    assert_eq!(instrument.global_processors.len(), 2);
    for parameter in [
        "layer.voice.generator.formant_vowel_position",
        "layer.voice.generator.formant_shift",
        "layer.voice.generator.formant_throat",
        "layer.voice.generator.formant_spectral_tilt",
        "voice.processor.voice_tone.cutoff",
        "global.processor.space.mix",
    ] {
        assert!(
            instrument.parameter_handle(parameter).is_some(),
            "missing compiled parameter {parameter}"
        );
    }
    let formant_shift = instrument
        .parameter_handle("layer.voice.generator.formant_shift")
        .expect("formant shift handle");
    render_instrument(
        instrument,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size,
            duration_frames: 10_240,
            tail_frames: 2_048,
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
                kind: ProcessEventKind::ParameterChange {
                    parameter: formant_shift,
                    normalized: 0.75,
                },
            },
            ScheduledEvent {
                absolute_frame: 4_096,
                kind: ProcessEventKind::ModWheel { value: 0.8 },
            },
            ScheduledEvent {
                absolute_frame: 6_144,
                kind: ProcessEventKind::Aftertouch { value: 0.7 },
            },
            ScheduledEvent {
                absolute_frame: 8_192,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            },
        ],
    )
    .expect("hybrid render succeeds")
}

fn process_harmonic_formant_hybrid(
    runtime: &mut InstrumentRuntime,
    events: &[ProcessEvent],
) -> Vec<Vec<f32>> {
    let mut left = vec![0.0_f32; 256];
    let mut right = vec![0.0_f32; 256];
    let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
    runtime
        .process(ProcessBlock {
            frames: 256,
            context: ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
            },
            events,
            output: &mut output,
        })
        .expect("hybrid process succeeds");
    vec![left, right]
}

#[test]
fn harmonic_formant_hybrid_integrates_generators_processors_and_modulation() {
    let (definition, base_dir) = harmonic_formant_hybrid_reference();
    assert_harmonic_formant_hybrid_structure(&definition);

    let reference = render_harmonic_formant_hybrid(&definition, &base_dir, 32);
    assert!(
        reference
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert!(
        reference
            .channels
            .iter()
            .flatten()
            .any(|sample| sample.abs() > 0.01)
    );
    for block_size in [64, 257, 1_024] {
        let candidate = render_harmonic_formant_hybrid(&definition, &base_dir, block_size);
        for (expected, actual) in reference.channels[0].iter().zip(&candidate.channels[0]) {
            assert_relative_eq!(*expected, *actual, epsilon = 1.0e-5);
        }
        for (expected, actual) in reference.channels[1].iter().zip(&candidate.channels[1]) {
            assert_relative_eq!(*expected, *actual, epsilon = 1.0e-5);
        }
    }
    let fresh = render_harmonic_formant_hybrid(&definition, &base_dir, 257);
    assert_eq!(
        fresh.channels,
        render_harmonic_formant_hybrid(&definition, &base_dir, 257).channels
    );

    let compiled = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir,
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid spec"),
        },
    )
    .instrument
    .expect("hybrid compiles for reset");
    let spec = ProcessSpec::new(48_000.0, 257, 2).expect("valid spec");
    let event = [ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 3,
            note_number: 60,
            velocity: 112,
        },
    }];
    let mut runtime = compiled.instantiate();
    runtime.prepare(spec).expect("hybrid runtime prepares");
    let first = process_harmonic_formant_hybrid(&mut runtime, &event);
    runtime.reset().expect("hybrid runtime resets");
    let reset = process_harmonic_formant_hybrid(&mut runtime, &event);
    assert_eq!(first, reset);
    let mut fresh_runtime = compiled.instantiate();
    fresh_runtime
        .prepare(spec)
        .expect("fresh hybrid runtime prepares");
    assert_eq!(
        reset,
        process_harmonic_formant_hybrid(&mut fresh_runtime, &event)
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
                    .parameter_handle("voice.processor.tone.cutoff")
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

const METAL_HIT_HASH: &str = "ecebbaa000ad97f19d659b4c7b42313ae47889b54191b85e6da0e8471979635c";

#[allow(clippy::too_many_arguments)]
fn sample_zone(
    id: &str,
    asset_path: &str,
    key_min: u8,
    key_max: u8,
    velocity_min: u8,
    velocity_max: u8,
    round_robin_group: Option<&str>,
    start_seconds: f32,
    end_seconds: f32,
) -> SampleZoneDefinition {
    SampleZoneDefinition {
        id: id.to_owned(),
        asset: AssetReference {
            path: asset_path.to_owned(),
            sha256: Some(METAL_HIT_HASH.to_owned()),
        },
        root_note: 60,
        key_min,
        key_max,
        velocity_min,
        velocity_max,
        round_robin_group: round_robin_group.map(str::to_owned),
        playback: SampleZonePlaybackDefinition {
            region: sonalloy_core::SampleRegionDefinition {
                start_seconds,
                end_seconds: Some(end_seconds),
            },
            direction: sonalloy_core::SamplePlaybackDirection::Forward,
            r#loop: None,
            time: sonalloy_core::SampleTimeDefinition::Resample,
        },
    }
}

fn sample_only_definition(zones: Vec<SampleZoneDefinition>) -> InstrumentDefinition {
    let mut value = hybrid_definition();
    value.layers.truncate(1);
    value.layers[0].gain_db = 0.0;
    value.layers[0].envelope = AdsrDefinition {
        attack_seconds: 0.0,
        decay_seconds: 0.0,
        sustain_level: 1.0,
        release_seconds: 0.01,
    };
    if let Some(modulation) = &mut value.modulation {
        modulation.routes.clear();
    }
    if let GeneratorDefinition::Sample(sample) = &mut value.layers[0].generator {
        sample.zones = zones;
    } else {
        panic!("hybrid attack layer must be a sample");
    }
    value
}

fn processed_hybrid_definition() -> InstrumentDefinition {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/processed-hybrid.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("processed hybrid exists"))
        .expect("processed hybrid parses")
}

#[test]
fn processed_hybrid_compiles_all_processor_scopes_and_keeps_a_global_tail() {
    let definition = processed_hybrid_definition();
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/instruments");
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir,
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"),
        },
    );
    let instrument = result.instrument.expect("processed hybrid compiles");

    assert_eq!(instrument.layers[0].processors.len(), 2);
    assert_eq!(instrument.layers[1].processors.len(), 2);
    assert_eq!(instrument.voice_processors.len(), 2);
    assert_eq!(instrument.global_processors.len(), 2);
    assert!(
        instrument
            .parameter_handle("layer.body.processor.body_drive.amount")
            .is_some()
    );
    assert!(
        instrument
            .parameter_handle("global.processor.space.mix")
            .is_some()
    );

    let audio = render_instrument(
        instrument,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 96_000,
            tail_frames: 48_000,
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
                absolute_frame: 48_000,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            },
        ],
    )
    .expect("processed hybrid render succeeds");

    assert!(
        audio
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert!(audio.channels[0].iter().any(|sample| sample.abs() > 0.01));
    assert!(
        audio.channels[0][96_000..]
            .iter()
            .any(|sample| sample.abs() > 1.0e-6)
    );
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
            assert_eq!(sample.zones.len(), 1);
            assert!(sample.zones[0].is_enabled());
            assert!(sample.zones[0].source.is_some());
        }
        sonalloy_core::compiler::CompiledGenerator::Oscillator(_)
        | sonalloy_core::compiler::CompiledGenerator::Noise(_)
        | sonalloy_core::compiler::CompiledGenerator::Additive(_)
        | sonalloy_core::compiler::CompiledGenerator::Formant(_)
        | sonalloy_core::compiler::CompiledGenerator::Granular(_)
        | sonalloy_core::compiler::CompiledGenerator::WaveSequence(_)
        | sonalloy_core::compiler::CompiledGenerator::Wavetable(_)
        | sonalloy_core::compiler::CompiledGenerator::OperatorModulation(_) => {
            panic!("attack layer must be a sample")
        }
    }
}

#[test]
fn release_sample_layer_remains_armed_until_note_off() {
    let mut definition = sample_only_definition(vec![sample_zone(
        "release",
        "../../testdata/assets/metal-hit.wav",
        0,
        127,
        1,
        127,
        None,
        0.0,
        0.08,
    )]);
    definition.layers[0].trigger.event = sonalloy_core::LayerTriggerEvent::NoteOff;
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/instruments");
    let instrument = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir,
            process_spec: ProcessSpec::new(48_000.0, 512, 2).expect("valid spec"),
        },
    )
    .instrument
    .expect("release sample compiles");
    let mut runtime = instrument.instantiate();
    runtime
        .prepare(ProcessSpec::new(48_000.0, 512, 2).expect("valid spec"))
        .expect("release sample prepares");

    let note_on = [ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 9,
            note_number: 60,
            velocity: 100,
        },
    }];
    let mut armed_left = vec![0.0; 128];
    let mut armed_right = vec![0.0; 128];
    let mut armed_output: [&mut [f32]; 2] = [&mut armed_left, &mut armed_right];
    runtime
        .process(ProcessBlock {
            frames: 128,
            context: ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
            },
            events: &note_on,
            output: &mut armed_output,
        })
        .expect("note on arms the release layer");
    assert_eq!(
        runtime.voice_state(0),
        Some(sonalloy_core::VoiceState::Active)
    );
    assert!(armed_left.iter().all(|sample| sample.abs() < 1.0e-12));
    assert!(armed_right.iter().all(|sample| sample.abs() < 1.0e-12));

    let note_off = [ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOff { note_id: 9 },
    }];
    let mut release_left = vec![0.0; 512];
    let mut release_right = vec![0.0; 512];
    let mut release_output: [&mut [f32]; 2] = [&mut release_left, &mut release_right];
    runtime
        .process(ProcessBlock {
            frames: 512,
            context: ProcessContext {
                absolute_frame: 128,
                tempo_bpm: 120.0,
            },
            events: &note_off,
            output: &mut release_output,
        })
        .expect("note off starts the release layer");
    assert_eq!(
        runtime.voice_state(0),
        Some(sonalloy_core::VoiceState::Releasing)
    );
    assert!(release_left.iter().all(|sample| sample.is_finite()));
    assert!(release_right.iter().all(|sample| sample.is_finite()));
    assert!(release_left.iter().any(|sample| sample.abs() > 1.0e-6));
}

#[test]
fn sample_zone_mapping_and_asset_cache_select_by_key_and_share_preparation() {
    let definition = sample_only_definition(vec![
        sample_zone(
            "low",
            "../../testdata/assets/metal-hit.wav",
            0,
            60,
            1,
            127,
            None,
            0.0,
            0.08,
        ),
        sample_zone(
            "high",
            "../../testdata/assets/metal-hit.wav",
            61,
            127,
            1,
            127,
            None,
            0.08,
            0.16,
        ),
    ]);
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/instruments");
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir,
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"),
        },
    );
    let instrument = result.instrument.expect("mapped sample compiles");
    let sonalloy_core::compiler::CompiledGenerator::Sample(sample) =
        &instrument.layers[0].generator
    else {
        panic!("sample layer compiles as a sample generator");
    };
    assert_eq!(sample.zones.len(), 2);
    assert!(
        sample
            .zones
            .iter()
            .all(sonalloy_core::compiler::CompiledSampleZone::is_enabled)
    );
    assert!(Arc::ptr_eq(
        sample.zones[0].source.as_ref().expect("low source"),
        sample.zones[1].source.as_ref().expect("high source")
    ));

    let low = render_instrument(
        Arc::clone(&instrument),
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 512,
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
    .expect("low zone renders");
    let high = render_instrument(
        instrument,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 512,
            tail_frames: 0,
        },
        &[ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 61,
                velocity: 100,
            },
        }],
    )
    .expect("high zone renders");
    let difference = low.channels[0]
        .iter()
        .zip(&high.channels[0])
        .map(|(left, right)| f64::from((*left - *right).abs()))
        .sum::<f64>();
    assert!(difference > 0.1, "key mapping did not change the region");
}

#[test]
#[allow(clippy::too_many_lines)]
fn round_robin_selection_is_definition_ordered_and_block_independent() {
    let definition = sample_only_definition(vec![
        sample_zone(
            "hit_a",
            "../../testdata/assets/metal-hit.wav",
            60,
            60,
            1,
            127,
            Some("hits"),
            0.0,
            0.08,
        ),
        sample_zone(
            "hit_b",
            "../../testdata/assets/metal-hit.wav",
            60,
            60,
            1,
            127,
            Some("hits"),
            0.08,
            0.16,
        ),
    ]);
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/instruments");
    let compiled_result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir,
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"),
        },
    );
    let compiled = compiled_result.instrument.unwrap_or_else(|| {
        panic!(
            "round robin sample diagnostics: {:?}",
            compiled_result.diagnostics
        )
    });
    let sonalloy_core::compiler::CompiledGenerator::Sample(sample) = &compiled.layers[0].generator
    else {
        panic!("sample layer compiles as a sample generator");
    };
    assert_eq!(sample.groups.len(), 1);
    assert_eq!(
        sample.groups[0].enabled_member_zone_indices.as_ref(),
        &[0, 1]
    );

    let events = [
        ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        },
        ScheduledEvent {
            absolute_frame: 2_000,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        },
        ScheduledEvent {
            absolute_frame: 4_000,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 60,
                velocity: 100,
            },
        },
        ScheduledEvent {
            absolute_frame: 6_000,
            kind: ProcessEventKind::NoteOff { note_id: 2 },
        },
        ScheduledEvent {
            absolute_frame: 8_000,
            kind: ProcessEventKind::NoteOn {
                note_id: 3,
                note_number: 60,
                velocity: 100,
            },
        },
    ];
    let render = |block_size| {
        render_instrument(
            Arc::clone(&compiled),
            RenderRequest {
                sample_rate: 48_000.0,
                block_size,
                duration_frames: 8_512,
                tail_frames: 0,
            },
            &events,
        )
        .expect("round robin render succeeds")
    };
    let reference = render(32);
    let candidate = render(257);
    for (left, right) in reference.channels[0].iter().zip(&candidate.channels[0]) {
        assert_relative_eq!(*left, *right, epsilon = 1.0e-6);
    }
    let first = &reference.channels[0][0..128];
    let second = &reference.channels[0][4_000..4_128];
    let third = &reference.channels[0][8_000..8_128];
    assert!(
        first
            .iter()
            .zip(second)
            .map(|(left, right)| f64::from((*left - *right).abs()))
            .sum::<f64>()
            > 0.1
    );
    assert_relative_eq!(
        first.iter().map(|value| f64::from(*value)).sum::<f64>(),
        third.iter().map(|value| f64::from(*value)).sum::<f64>(),
        epsilon = 0.1
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn pending_round_robin_selection_is_captured_before_voice_stealing() {
    let mut definition = sample_only_definition(vec![
        sample_zone(
            "hit_a",
            "../../testdata/assets/metal-hit.wav",
            60,
            60,
            1,
            127,
            Some("hits"),
            0.0,
            0.08,
        ),
        sample_zone(
            "hit_b",
            "../../testdata/assets/metal-hit.wav",
            60,
            60,
            1,
            127,
            Some("hits"),
            0.08,
            0.16,
        ),
    ]);
    definition.performance.polyphony = 1;
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/instruments");
    let compiled = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir.clone(),
            process_spec: ProcessSpec::new(48_000.0, 32, 2).expect("valid process spec"),
        },
    )
    .instrument
    .expect("pending selection fixture compiles");
    let stolen = render_instrument(
        compiled,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 32,
            duration_frames: 1_024,
            tail_frames: 0,
        },
        &[
            ScheduledEvent {
                absolute_frame: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 100,
                },
            },
            ScheduledEvent {
                absolute_frame: 96,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 60,
                    velocity: 100,
                },
            },
            ScheduledEvent {
                absolute_frame: 800,
                kind: ProcessEventKind::NoteOff { note_id: 2 },
            },
        ],
    )
    .expect("voice stealing render succeeds");

    let direct = sample_only_definition(vec![sample_zone(
        "hit_b",
        "../../testdata/assets/metal-hit.wav",
        60,
        60,
        1,
        127,
        Some("hits"),
        0.08,
        0.16,
    )]);
    let direct = compile_instrument(
        &direct,
        &CompileContext {
            definition_base_dir: base_dir,
            process_spec: ProcessSpec::new(48_000.0, 32, 2).expect("valid process spec"),
        },
    )
    .instrument
    .expect("direct pending zone fixture compiles");
    let direct = render_instrument(
        direct,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 32,
            duration_frames: 128,
            tail_frames: 0,
        },
        &[ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 3,
                note_number: 60,
                velocity: 100,
            },
        }],
    )
    .expect("direct zone render succeeds");

    let pending_start = 96 + 240;
    for (stolen_sample, direct_sample) in stolen.channels[0][pending_start..pending_start + 128]
        .iter()
        .zip(&direct.channels[0])
    {
        assert_relative_eq!(*stolen_sample, *direct_sample, epsilon = 1.0e-6);
    }
}

#[test]
fn missing_round_robin_member_is_skipped_without_disabling_valid_zone() {
    let definition = sample_only_definition(vec![
        sample_zone(
            "missing",
            "../../testdata/assets/not-present.wav",
            60,
            60,
            1,
            127,
            Some("hits"),
            0.0,
            0.08,
        ),
        sample_zone(
            "valid",
            "../../testdata/assets/metal-hit.wav",
            60,
            60,
            1,
            127,
            Some("hits"),
            0.08,
            0.16,
        ),
    ]);
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/instruments");
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir,
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"),
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::AssetNotFound)
    );
    let instrument = result.instrument.expect("partial sample compile succeeds");
    let sonalloy_core::compiler::CompiledGenerator::Sample(sample) =
        &instrument.layers[0].generator
    else {
        panic!("sample layer compiles as a sample generator");
    };
    assert_eq!(sample.groups[0].enabled_member_zone_indices.as_ref(), &[1]);
    let audio = render_instrument(
        instrument,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 512,
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
    .expect("valid round robin member renders");
    assert!(audio.channels[0].iter().any(|sample| sample.abs() > 0.01));
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
        sonalloy_core::GeneratorDefinition::Sample(sample) => {
            sample.zones[0].asset.sha256 = None;
        }
        sonalloy_core::GeneratorDefinition::Oscillator(_)
        | sonalloy_core::GeneratorDefinition::Noise(_)
        | sonalloy_core::GeneratorDefinition::Additive(_)
        | sonalloy_core::GeneratorDefinition::Formant(_)
        | sonalloy_core::GeneratorDefinition::Granular(_)
        | sonalloy_core::GeneratorDefinition::WaveSequence(_)
        | sonalloy_core::GeneratorDefinition::Wavetable(_)
        | sonalloy_core::GeneratorDefinition::OperatorModulation(_) => {
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
            assert!(sample.zones[0].is_enabled());
            assert!(sample.zones[0].source.is_some());
        }
        sonalloy_core::compiler::CompiledGenerator::Oscillator(_)
        | sonalloy_core::compiler::CompiledGenerator::Noise(_)
        | sonalloy_core::compiler::CompiledGenerator::Additive(_)
        | sonalloy_core::compiler::CompiledGenerator::Formant(_)
        | sonalloy_core::compiler::CompiledGenerator::Granular(_)
        | sonalloy_core::compiler::CompiledGenerator::WaveSequence(_)
        | sonalloy_core::compiler::CompiledGenerator::Wavetable(_)
        | sonalloy_core::compiler::CompiledGenerator::OperatorModulation(_) => {
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
            sample.zones[0].asset.path = asset_path.to_string_lossy().into_owned();
        }
        sonalloy_core::GeneratorDefinition::Oscillator(_)
        | sonalloy_core::GeneratorDefinition::Noise(_)
        | sonalloy_core::GeneratorDefinition::Additive(_)
        | sonalloy_core::GeneratorDefinition::Formant(_)
        | sonalloy_core::GeneratorDefinition::Granular(_)
        | sonalloy_core::GeneratorDefinition::WaveSequence(_)
        | sonalloy_core::GeneratorDefinition::Wavetable(_)
        | sonalloy_core::GeneratorDefinition::OperatorModulation(_) => {
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
            sample.zones[0].asset.sha256 = Some("00".repeat(32));
        }
        sonalloy_core::GeneratorDefinition::Oscillator(_)
        | sonalloy_core::GeneratorDefinition::Noise(_)
        | sonalloy_core::GeneratorDefinition::Additive(_)
        | sonalloy_core::GeneratorDefinition::Formant(_)
        | sonalloy_core::GeneratorDefinition::Granular(_)
        | sonalloy_core::GeneratorDefinition::WaveSequence(_)
        | sonalloy_core::GeneratorDefinition::Wavetable(_)
        | sonalloy_core::GeneratorDefinition::OperatorModulation(_) => {
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
            assert!(!sample.zones[0].is_enabled());
            assert!(sample.zones[0].source.is_none());
        }
        sonalloy_core::compiler::CompiledGenerator::Oscillator(_)
        | sonalloy_core::compiler::CompiledGenerator::Noise(_)
        | sonalloy_core::compiler::CompiledGenerator::Additive(_)
        | sonalloy_core::compiler::CompiledGenerator::Formant(_)
        | sonalloy_core::compiler::CompiledGenerator::Granular(_)
        | sonalloy_core::compiler::CompiledGenerator::WaveSequence(_)
        | sonalloy_core::compiler::CompiledGenerator::Wavetable(_)
        | sonalloy_core::compiler::CompiledGenerator::OperatorModulation(_) => {
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
