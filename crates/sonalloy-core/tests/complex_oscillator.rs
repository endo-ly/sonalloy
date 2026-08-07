use std::path::PathBuf;
use std::sync::Arc;

use approx::assert_relative_eq;
use sonalloy_core::{
    AdsrDefinition, CompileContext, GeneratorDefinition, InstrumentDefinition, InstrumentProcessor,
    OscillatorDefinition, OscillatorFeedbackDefinition, OscillatorWaveform,
    PhaseDistortionDefinition, ProcessSpec, RenderRequest, ScheduledEvent, UnisonDefinition,
    WavefoldDefinition, compile_instrument, render_instrument,
};

fn definition() -> InstrumentDefinition {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/basic-poly-synth.json");
    let mut definition: InstrumentDefinition =
        serde_json::from_str(&std::fs::read_to_string(path).expect("reference Definition exists"))
            .expect("reference Definition parses");
    definition.layers[0].gain_db = 0.0;
    definition.layers[0].envelope = AdsrDefinition {
        attack_seconds: 0.0,
        decay_seconds: 0.0,
        sustain_level: 1.0,
        release_seconds: 0.01,
    };
    definition.layers[0].processors.clear();
    definition.voice_processors.clear();
    definition.global_processors.clear();
    definition.modulation = None;
    definition
}

fn oscillator(
    waveform: OscillatorWaveform,
    phase_distortion: Option<f32>,
    wavefold: Option<f32>,
    feedback: Option<f32>,
    unison: Option<UnisonDefinition>,
) -> OscillatorDefinition {
    OscillatorDefinition {
        waveform,
        phase_reset: true,
        phase: 0.0,
        hard_sync: None,
        waveshaping: None,
        phase_distortion: phase_distortion.map(|amount| PhaseDistortionDefinition { amount }),
        wavefold: wavefold.map(|amount| WavefoldDefinition { amount }),
        feedback: feedback.map(|amount| OscillatorFeedbackDefinition { amount }),
        unison,
    }
}

fn render(
    definition: &InstrumentDefinition,
    block_size: usize,
    duration_frames: usize,
    events: &[ScheduledEvent],
) -> sonalloy_core::RenderedAudio {
    let result = compile_instrument(
        definition,
        &CompileContext {
            definition_base_dir: PathBuf::from("."),
            process_spec: ProcessSpec::new(48_000.0, block_size, 2).expect("valid process spec"),
        },
    );
    let instrument = result.instrument.expect("complex oscillator compiles");
    render_instrument(
        instrument,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size,
            duration_frames: u64::try_from(duration_frames).expect("duration fits in u64"),
            tail_frames: 0,
        },
        events,
    )
    .expect("complex oscillator render succeeds")
}

fn note_on() -> ScheduledEvent {
    ScheduledEvent {
        absolute_frame: 0,
        kind: sonalloy_core::ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 100,
        },
    }
}

#[test]
fn definition_validation_rejects_unsupported_phase_domain_combinations() {
    let mut value = definition();
    value.layers[0].generator = GeneratorDefinition::Oscillator(OscillatorDefinition {
        waveform: OscillatorWaveform::Saw,
        phase_reset: true,
        phase: 0.0,
        hard_sync: Some(sonalloy_core::HardSyncDefinition { ratio: 2.0 }),
        waveshaping: None,
        phase_distortion: Some(PhaseDistortionDefinition { amount: 0.5 }),
        wavefold: Some(WavefoldDefinition { amount: 0.5 }),
        feedback: Some(OscillatorFeedbackDefinition { amount: 0.5 }),
        unison: None,
    });

    let diagnostics = value.validate();

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path.as_deref() == Some("layers[0].generator.oscillator.phase_distortion")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path.as_deref() == Some("layers[0].generator.oscillator.feedback")
    }));
}

#[test]
fn wavefold_keeps_existing_backends_and_amounts_use_the_normalized_range() {
    let mut value = definition();
    value.layers[0].generator = GeneratorDefinition::Oscillator(oscillator(
        OscillatorWaveform::Saw,
        None,
        Some(0.5),
        None,
        None,
    ));
    let basic = compile_instrument(
        &value,
        &CompileContext {
            definition_base_dir: PathBuf::from("."),
            process_spec: ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec"),
        },
    )
    .instrument
    .expect("wavefold compiles");
    let GeneratorDefinition::Oscillator(basic_oscillator) = &value.layers[0].generator else {
        panic!("definition must contain an oscillator");
    };
    let sonalloy_core::compiler::CompiledGenerator::Oscillator(compiled_basic) =
        &basic.layers[0].generator
    else {
        panic!("compiled definition must contain an oscillator");
    };
    assert_eq!(
        compiled_basic.backend,
        sonalloy_core::compiler::CompiledOscillatorBackend::Basic
    );
    assert!(compiled_basic.dc_blocker);
    assert_eq!(
        basic_oscillator
            .wavefold
            .expect("wavefold definition")
            .amount
            .to_bits(),
        0.5_f32.to_bits()
    );

    let mut hard_sync = value.clone();
    if let GeneratorDefinition::Oscillator(oscillator) = &mut hard_sync.layers[0].generator {
        oscillator.hard_sync = Some(sonalloy_core::HardSyncDefinition { ratio: 2.0 });
    }
    let compiled_hard_sync = compile_instrument(
        &hard_sync,
        &CompileContext {
            definition_base_dir: PathBuf::from("."),
            process_spec: ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec"),
        },
    )
    .instrument
    .expect("hard-sync wavefold compiles");
    let sonalloy_core::compiler::CompiledGenerator::Oscillator(compiled_hard_sync) =
        &compiled_hard_sync.layers[0].generator
    else {
        panic!("compiled definition must contain an oscillator");
    };
    assert!(matches!(
        compiled_hard_sync.backend,
        sonalloy_core::compiler::CompiledOscillatorBackend::VariableShapeSync { .. }
    ));

    for (field, path) in [
        (
            "phase_distortion",
            "layers[0].generator.oscillator.phase_distortion.amount",
        ),
        ("wavefold", "layers[0].generator.oscillator.wavefold.amount"),
        ("feedback", "layers[0].generator.oscillator.feedback.amount"),
    ] {
        let mut invalid = value.clone();
        let GeneratorDefinition::Oscillator(oscillator) = &mut invalid.layers[0].generator else {
            panic!("definition must contain an oscillator");
        };
        match field {
            "phase_distortion" => {
                oscillator.waveform = OscillatorWaveform::Sine;
                oscillator.phase_distortion = Some(PhaseDistortionDefinition { amount: 1.1 });
            }
            "wavefold" => {
                oscillator.wavefold = Some(WavefoldDefinition { amount: 1.1 });
            }
            "feedback" => {
                oscillator.waveform = OscillatorWaveform::Sine;
                oscillator.feedback = Some(OscillatorFeedbackDefinition { amount: 1.1 });
            }
            _ => unreachable!("test field is fixed"),
        }
        assert!(
            invalid
                .validate()
                .iter()
                .any(|diagnostic| { diagnostic.path.as_deref() == Some(path) })
        );
    }
}

#[test]
fn compile_binds_complex_parameters_and_phase_domain_limits() {
    let mut value = definition();
    value.layers[0].generator = GeneratorDefinition::Oscillator(oscillator(
        OscillatorWaveform::Sine,
        Some(0.65),
        Some(0.4),
        Some(0.3),
        Some(UnisonDefinition {
            voices: 3,
            detune_cents: 9.0,
            stereo_spread: 0.6,
            phase_spread: 0.25,
        }),
    ));

    let result = compile_instrument(
        &value,
        &CompileContext {
            definition_base_dir: PathBuf::from("."),
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"),
        },
    );
    let instrument = result.instrument.expect("complex oscillator compiles");
    let sonalloy_core::compiler::CompiledGenerator::Oscillator(compiled) =
        &instrument.layers[0].generator
    else {
        panic!("definition must compile to an oscillator");
    };

    assert_eq!(
        compiled.backend,
        sonalloy_core::compiler::CompiledOscillatorBackend::PhaseDomain
    );
    assert!(compiled.dc_blocker);
    assert_eq!(
        compiled.backend.effective_max_frequency(48_000.0).to_bits(),
        11_520.0_f32.to_bits()
    );
    for id in [
        "layer.body.generator.phase_distortion",
        "layer.body.generator.wavefold",
        "layer.body.generator.oscillator_feedback",
    ] {
        let descriptor = instrument
            .parameters()
            .iter()
            .find(|parameter| parameter.id == id)
            .expect("complex parameter descriptor");
        assert_eq!(descriptor.min.to_bits(), 0.0_f32.to_bits());
        assert_eq!(descriptor.max.to_bits(), 1.0_f32.to_bits());
        assert_eq!(descriptor.smoothing_seconds.to_bits(), 0.005_f32.to_bits());
    }
    assert_eq!(compiled.unison.position_distribution.len(), 3);
}

#[test]
fn complex_runtime_is_finite_stereo_and_parameter_sweeps_are_continuous() {
    let mut value = definition();
    value.layers[0].generator = GeneratorDefinition::Oscillator(oscillator(
        OscillatorWaveform::Sine,
        Some(0.25),
        Some(0.25),
        Some(0.3),
        Some(UnisonDefinition {
            voices: 3,
            detune_cents: 9.0,
            stereo_spread: 0.6,
            phase_spread: 0.25,
        }),
    ));
    let compiled = compile_instrument(
        &value,
        &CompileContext {
            definition_base_dir: PathBuf::from("."),
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"),
        },
    )
    .instrument
    .expect("complex oscillator compiles");
    let wavefold = compiled
        .parameter_handle("layer.body.generator.wavefold")
        .expect("wavefold handle");
    let events = [
        note_on(),
        ScheduledEvent {
            absolute_frame: 512,
            kind: sonalloy_core::ProcessEventKind::ParameterChange {
                parameter: wavefold,
                normalized: 0.0,
            },
        },
        ScheduledEvent {
            absolute_frame: 1_024,
            kind: sonalloy_core::ProcessEventKind::ParameterChange {
                parameter: wavefold,
                normalized: 1.0,
            },
        },
    ];
    let audio = render(&value, 257, 2_048, &events);

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
            .any(|sample| sample.abs() > 1.0e-4)
    );
    assert!(
        audio.channels[0]
            .iter()
            .zip(&audio.channels[1])
            .any(|(left, right)| (left - right).abs() > 1.0e-5)
    );
    assert!(
        audio.channels[0]
            .windows(2)
            .all(|pair| (pair[1] - pair[0]).abs() < 2.0)
    );
}

#[test]
fn phase_distortion_feedback_and_wavefold_change_the_rendered_signal() {
    let mut phase_distortion_zero = definition();
    phase_distortion_zero.layers[0].generator = GeneratorDefinition::Oscillator(oscillator(
        OscillatorWaveform::Sine,
        Some(0.0),
        None,
        None,
        None,
    ));
    let mut phase_distortion_high = phase_distortion_zero.clone();
    if let GeneratorDefinition::Oscillator(value) = &mut phase_distortion_high.layers[0].generator {
        value.phase_distortion = Some(PhaseDistortionDefinition { amount: 0.75 });
    }
    let mut feedback = phase_distortion_zero.clone();
    if let GeneratorDefinition::Oscillator(value) = &mut feedback.layers[0].generator {
        value.phase_distortion = None;
        value.feedback = Some(OscillatorFeedbackDefinition { amount: 0.8 });
    }
    let mut feedback_zero = phase_distortion_zero.clone();
    if let GeneratorDefinition::Oscillator(value) = &mut feedback_zero.layers[0].generator {
        value.phase_distortion = None;
        value.feedback = Some(OscillatorFeedbackDefinition { amount: 0.0 });
    }
    let mut wavefold_zero = definition();
    wavefold_zero.layers[0].generator = GeneratorDefinition::Oscillator(oscillator(
        OscillatorWaveform::Saw,
        None,
        Some(0.0),
        None,
        None,
    ));
    let mut wavefold = wavefold_zero.clone();
    if let GeneratorDefinition::Oscillator(value) = &mut wavefold.layers[0].generator {
        value.wavefold = Some(WavefoldDefinition { amount: 0.75 });
    }

    let zero = render(&phase_distortion_zero, 64, 1_024, &[note_on()]);
    let distorted = render(&phase_distortion_high, 64, 1_024, &[note_on()]);
    let feedback_zero_audio = render(&feedback_zero, 64, 1_024, &[note_on()]);
    let feedback_audio = render(&feedback, 64, 1_024, &[note_on()]);
    let wavefold_zero_audio = render(&wavefold_zero, 64, 1_024, &[note_on()]);
    let folded = render(&wavefold, 64, 1_024, &[note_on()]);

    assert!(
        zero.channels[0]
            .iter()
            .zip(&distorted.channels[0])
            .any(|(left, right)| (left - right).abs() > 1.0e-4)
    );
    assert!(
        zero.channels[0]
            .iter()
            .zip(&feedback_audio.channels[0])
            .any(|(left, right)| (left - right).abs() > 1.0e-4)
    );
    assert!(
        zero.channels[0]
            .iter()
            .zip(&feedback_zero_audio.channels[0])
            .all(|(left, right)| (left - right).abs() < 1.0e-6)
    );
    assert!(
        wavefold_zero_audio.channels[0]
            .iter()
            .zip(&folded.channels[0])
            .any(|(left, right)| (left - right).abs() > 1.0e-4)
    );
    assert_relative_eq!(zero.channels[0][0], 0.0, epsilon = 1.0e-6);
}

#[test]
fn reset_matches_a_fresh_complex_runtime() {
    let mut value = definition();
    value.layers[0].generator = GeneratorDefinition::Oscillator(oscillator(
        OscillatorWaveform::Sine,
        Some(0.65),
        Some(0.5),
        Some(0.7),
        None,
    ));
    let result = compile_instrument(
        &value,
        &CompileContext {
            definition_base_dir: PathBuf::from("."),
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"),
        },
    );
    let instrument = result.instrument.expect("complex oscillator compiles");
    let spec = ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec");
    let mut runtime = Arc::clone(&instrument).instantiate();
    runtime.prepare(spec).expect("runtime preparation");
    let mut first_left = [0.0_f32; 257];
    let mut first_right = [0.0_f32; 257];
    let mut output: [&mut [f32]; 2] = [&mut first_left, &mut first_right];
    runtime
        .process(sonalloy_core::ProcessBlock {
            frames: 257,
            context: sonalloy_core::ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
            },
            events: &[sonalloy_core::ProcessEvent {
                sample_offset: 0,
                kind: note_on().kind,
            }],
            output: &mut output,
        })
        .expect("first process");
    runtime.reset().expect("runtime reset");
    let mut reset_left = [0.0_f32; 257];
    let mut reset_right = [0.0_f32; 257];
    let mut reset_output: [&mut [f32]; 2] = [&mut reset_left, &mut reset_right];
    runtime
        .process(sonalloy_core::ProcessBlock {
            frames: 257,
            context: sonalloy_core::ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
            },
            events: &[sonalloy_core::ProcessEvent {
                sample_offset: 0,
                kind: note_on().kind,
            }],
            output: &mut reset_output,
        })
        .expect("reset process");

    for (first, reset) in first_left.iter().zip(reset_left) {
        assert_relative_eq!(*first, reset, epsilon = 1.0e-6);
    }
    for (first, reset) in first_right.iter().zip(reset_right) {
        assert_relative_eq!(*first, reset, epsilon = 1.0e-6);
    }
}
