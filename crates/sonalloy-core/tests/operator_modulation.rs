use std::path::PathBuf;
use std::sync::Arc;

use approx::assert_relative_eq;
use sonalloy_core::{
    AdsrDefinition, CompileContext, GeneratorDefinition, InstrumentDefinition, InstrumentProcessor,
    LfoDefinition, LfoWaveform, ModulationCurve, ModulationDefinition, ModulationRouteDefinition,
    ModulationSourceDefinition, OperatorAlgorithm, OperatorDefinition,
    OperatorModulationDefinition, OperatorModulationMode, ProcessEvent, ProcessEventKind,
    ProcessSpec, RenderRequest, ScheduledEvent, UnisonDefinition, compile_instrument,
    render_instrument,
};

fn base_definition() -> InstrumentDefinition {
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
        release_seconds: 0.05,
    };
    definition.modulation = None;
    definition
}

fn operator_definition(
    mode: OperatorModulationMode,
    algorithm: OperatorAlgorithm,
    unison: Option<UnisonDefinition>,
) -> InstrumentDefinition {
    let mut definition = base_definition();
    let (levels, amounts) = match algorithm {
        OperatorAlgorithm::Stack4 | OperatorAlgorithm::ThreeModulators => {
            ([0.9, 0.0, 0.0, 0.0], [0.0, 2.0, 2.0, 2.0])
        }
        OperatorAlgorithm::Stack3PlusCarrier | OperatorAlgorithm::TwoModulatorsPlusCarrier => {
            ([0.6, 0.6, 0.0, 0.0], [0.0, 0.0, 2.0, 2.0])
        }
        OperatorAlgorithm::TwoStacks => ([0.6, 0.0, 0.6, 0.0], [0.0, 2.0, 0.0, 2.0]),
        OperatorAlgorithm::ForkToCarrier => ([0.9, 0.0, 0.0, 0.0], [0.0, 2.0, 2.0, 0.0]),
        OperatorAlgorithm::SharedModulator => ([0.5, 0.5, 0.5, 0.0], [0.0, 0.0, 0.0, 2.0]),
        OperatorAlgorithm::Parallel => ([0.5, 0.5, 0.5, 0.5], [0.0; 4]),
    };
    let amount_scale = match mode {
        OperatorModulationMode::Phase | OperatorModulationMode::Frequency => 1.0,
        OperatorModulationMode::Amplitude | OperatorModulationMode::Ring => 0.35,
    };
    let envelope = AdsrDefinition {
        attack_seconds: 0.0,
        decay_seconds: 0.15,
        sustain_level: 1.0,
        release_seconds: 0.02,
    };
    let operators = levels
        .into_iter()
        .zip(amounts)
        .enumerate()
        .map(|(index, (level, amount))| OperatorDefinition {
            ratio: [1.0, 2.0, 3.0, 5.0][index],
            detune_cents: 0.0,
            level,
            modulation_amount: amount * amount_scale,
            feedback: if matches!(
                mode,
                OperatorModulationMode::Phase | OperatorModulationMode::Frequency
            ) && index == 3
            {
                0.35
            } else {
                0.0
            },
            phase: 0.0,
            envelope,
        })
        .collect();
    definition.layers[0].generator =
        GeneratorDefinition::OperatorModulation(OperatorModulationDefinition {
            mode,
            algorithm,
            operators,
            phase_reset: true,
            unison,
        });
    definition
}

fn compile(
    definition: &InstrumentDefinition,
    block_size: usize,
) -> Arc<sonalloy_core::CompiledInstrument> {
    let result = compile_instrument(
        definition,
        &CompileContext {
            definition_base_dir: PathBuf::from("."),
            process_spec: ProcessSpec::new(48_000.0, block_size, 2).expect("valid process spec"),
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.severity != sonalloy_core::DiagnosticSeverity::Error }),
        "operator definition must compile: {:?}",
        result.diagnostics
    );
    result.instrument.expect("operator definition compiles")
}

fn render(
    definition: &InstrumentDefinition,
    block_size: usize,
    duration_frames: u64,
    events: &[ScheduledEvent],
) -> sonalloy_core::RenderedAudio {
    render_instrument(
        compile(definition, block_size),
        RenderRequest {
            sample_rate: 48_000.0,
            block_size,
            duration_frames,
            tail_frames: 0,
        },
        events,
    )
    .expect("operator definition renders")
}

fn note_on() -> ScheduledEvent {
    ScheduledEvent {
        absolute_frame: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 110,
        },
    }
}

fn note_off(frame: u64) -> ScheduledEvent {
    ScheduledEvent {
        absolute_frame: frame,
        kind: ProcessEventKind::NoteOff { note_id: 1 },
    }
}

fn assert_finite_and_audible(audio: &sonalloy_core::RenderedAudio) {
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

fn rms(samples: &[f32]) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let count = samples.len() as f32;
    (samples.iter().map(|sample| sample * sample).sum::<f32>() / count).sqrt()
}

#[test]
fn all_algorithms_compile_with_fixed_topology_and_render() {
    let algorithms = [
        OperatorAlgorithm::Stack4,
        OperatorAlgorithm::Stack3PlusCarrier,
        OperatorAlgorithm::TwoStacks,
        OperatorAlgorithm::ForkToCarrier,
        OperatorAlgorithm::TwoModulatorsPlusCarrier,
        OperatorAlgorithm::ThreeModulators,
        OperatorAlgorithm::SharedModulator,
        OperatorAlgorithm::Parallel,
    ];
    for algorithm in algorithms {
        let definition = operator_definition(OperatorModulationMode::Phase, algorithm, None);
        let compiled = compile(&definition, 257);
        let sonalloy_core::compiler::CompiledGenerator::OperatorModulation(operator) =
            &compiled.layers[0].generator
        else {
            panic!("definition must compile to Operator Modulation");
        };
        assert_eq!(operator.topology.evaluation_order.len(), 4);
        assert_eq!(
            operator
                .topology
                .evaluation_order
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            4
        );
        assert_ne!(operator.topology.carrier_mask, 0);
        #[allow(clippy::cast_precision_loss)]
        let carrier_count = operator.topology.carrier_mask.count_ones() as f32;
        assert_relative_eq!(
            operator.topology.carrier_normalization,
            1.0 / carrier_count.sqrt(),
            epsilon = 1.0e-6
        );
        let audio = render(&definition, 257, 1_024, &[note_on()]);
        assert_finite_and_audible(&audio);
    }
}

#[test]
fn modes_have_distinct_finite_audio_and_mode_specific_limits() {
    let modes = [
        OperatorModulationMode::Phase,
        OperatorModulationMode::Frequency,
        OperatorModulationMode::Amplitude,
        OperatorModulationMode::Ring,
    ];
    let mut renders = Vec::new();
    for mode in modes {
        let definition = operator_definition(mode, OperatorAlgorithm::Stack4, None);
        let compiled = compile(&definition, 257);
        let sonalloy_core::compiler::CompiledGenerator::OperatorModulation(operator) =
            &compiled.layers[0].generator
        else {
            panic!("definition must compile to Operator Modulation");
        };
        let expected_limit = if matches!(
            mode,
            OperatorModulationMode::Phase | OperatorModulationMode::Frequency
        ) {
            48_000.0 * 0.24
        } else {
            48_000.0 * 0.45
        };
        assert_relative_eq!(
            operator.effective_max_frequency,
            expected_limit,
            epsilon = 1.0e-3
        );
        let audio = render(&definition, 257, 1_024, &[note_on()]);
        assert_finite_and_audible(&audio);
        renders.push(audio.channels[0].clone());
    }
    assert!(rms(&renders[0]) > 0.0);
    let phase_frequency_difference = renders[0]
        .iter()
        .zip(&renders[1])
        .map(|(left, right)| (left - right).abs())
        .fold(0.0, f32::max);
    assert!(phase_frequency_difference > 1.0e-4);
    assert!(
        renders[0]
            .iter()
            .zip(&renders[2])
            .any(|(left, right)| (left - right).abs() > 1.0e-4)
    );
    assert!(
        renders[2]
            .iter()
            .zip(&renders[3])
            .any(|(left, right)| (left - right).abs() > 1.0e-4)
    );
}

#[test]
fn zero_modulation_amount_is_an_identity_for_all_modes() {
    let modes = [
        OperatorModulationMode::Phase,
        OperatorModulationMode::Frequency,
        OperatorModulationMode::Amplitude,
        OperatorModulationMode::Ring,
    ];
    let renders = modes.map(|mode| {
        let mut definition = operator_definition(mode, OperatorAlgorithm::Stack4, None);
        if let GeneratorDefinition::OperatorModulation(operator) =
            &mut definition.layers[0].generator
        {
            for value in &mut operator.operators {
                value.modulation_amount = 0.0;
                value.feedback = 0.0;
            }
        }
        render(&definition, 257, 1_024, &[note_on()])
    });

    for audio in &renders {
        assert_finite_and_audible(audio);
    }
    for audio in &renders[1..] {
        for (actual, expected) in audio.channels[0].iter().zip(&renders[0].channels[0]) {
            assert_relative_eq!(*actual, *expected, epsilon = 1.0e-6);
        }
    }
}

#[test]
fn parameter_change_ratio_and_index_are_continuous_across_block_sizes() {
    let definition = operator_definition(
        OperatorModulationMode::Phase,
        OperatorAlgorithm::Stack4,
        None,
    );
    let compiled = compile(&definition, 257);
    let ratio = compiled
        .parameter_handle("layer.body.generator.operator.2.ratio")
        .expect("ratio parameter");
    let amount = compiled
        .parameter_handle("layer.body.generator.operator.2.modulation_amount")
        .expect("modulation amount parameter");
    let events = [
        note_on(),
        ScheduledEvent {
            absolute_frame: 512,
            kind: ProcessEventKind::ParameterChange {
                parameter: ratio,
                normalized: 0.4,
            },
        },
        ScheduledEvent {
            absolute_frame: 768,
            kind: ProcessEventKind::ParameterChange {
                parameter: amount,
                normalized: 0.6,
            },
        },
        note_off(1_536),
    ];
    let renders =
        [32, 64, 257, 1_024].map(|block_size| render(&definition, block_size, 2_048, &events));
    let reference = &renders[0];
    for audio in &renders {
        assert_finite_and_audible(audio);
    }
    for (block_size, audio) in [32, 64, 257, 1_024].into_iter().zip(&renders) {
        let (max_error, squared_error) = reference.channels[0].iter().zip(&audio.channels[0]).fold(
            (0.0_f32, 0.0_f32),
            |(max_error, squared_error), (left, right)| {
                let error = (left - right).abs();
                (max_error.max(error), squared_error + error * error)
            },
        );
        #[allow(clippy::cast_precision_loss)]
        let error_rms = (squared_error / reference.channels[0].len() as f32).sqrt();
        assert!(
            max_error <= 1.0e-2 && error_rms <= 2.0e-3,
            "block size {block_size} differs by max {max_error} and RMS {error_rms}"
        );
    }
}

#[test]
fn modulation_route_reaches_operator_index_parameter() {
    let baseline = operator_definition(
        OperatorModulationMode::Phase,
        OperatorAlgorithm::Stack4,
        None,
    );
    let mut modulated = baseline.clone();
    modulated.modulation = Some(ModulationDefinition {
        sources: vec![ModulationSourceDefinition::Lfo(LfoDefinition {
            id: "index_lfo".to_owned(),
            waveform: LfoWaveform::Sine,
            rate_hz: 4.0,
            phase: 0.0,
        })],
        routes: vec![ModulationRouteDefinition {
            source: "index_lfo".to_owned(),
            target: "layer.body.generator.operator.2.modulation_amount".to_owned(),
            amount: 0.25,
            curve: ModulationCurve::Linear,
        }],
    });
    let baseline_audio = render(&baseline, 257, 2_048, &[note_on()]);
    let modulated_audio = render(&modulated, 257, 2_048, &[note_on()]);
    assert_finite_and_audible(&baseline_audio);
    assert_finite_and_audible(&modulated_audio);
    assert!(
        baseline_audio.channels[0]
            .iter()
            .zip(&modulated_audio.channels[0])
            .any(|(baseline, modulated)| (baseline - modulated).abs() > 1.0e-4)
    );
}

#[test]
fn feedback_uses_the_previous_operator_output() {
    let mut without_feedback = operator_definition(
        OperatorModulationMode::Phase,
        OperatorAlgorithm::Stack4,
        None,
    );
    let mut with_feedback = without_feedback.clone();
    if let GeneratorDefinition::OperatorModulation(operator) =
        &mut without_feedback.layers[0].generator
    {
        operator.operators[3].feedback = 0.0;
    }
    if let GeneratorDefinition::OperatorModulation(operator) =
        &mut with_feedback.layers[0].generator
    {
        operator.operators[3].feedback = 0.8;
    }
    let without_audio = render(&without_feedback, 257, 2_048, &[note_on()]);
    let with_audio = render(&with_feedback, 257, 2_048, &[note_on()]);
    assert_finite_and_audible(&without_audio);
    assert_finite_and_audible(&with_audio);
    assert!(
        without_audio.channels[0]
            .iter()
            .zip(&with_audio.channels[0])
            .any(|(without, with)| (without - with).abs() > 1.0e-4)
    );
}

#[test]
fn operator_envelope_note_off_and_voice_stealing_reset_state() {
    let mut definition = operator_definition(
        OperatorModulationMode::Frequency,
        OperatorAlgorithm::Stack4,
        None,
    );
    if let GeneratorDefinition::OperatorModulation(operator) = &mut definition.layers[0].generator {
        operator.operators[3].envelope = AdsrDefinition {
            attack_seconds: 0.0,
            decay_seconds: 0.01,
            sustain_level: 0.0,
            release_seconds: 0.01,
        };
    }
    definition.performance.polyphony = 1;
    let events = [
        note_on(),
        note_off(512),
        ScheduledEvent {
            absolute_frame: 1_024,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 67,
                velocity: 110,
            },
        },
    ];
    let audio = render(&definition, 64, 2_048, &events);
    assert_finite_and_audible(&audio);
    let before_release = rms(&audio.channels[0][128..512]);
    let after_release = rms(&audio.channels[0][640..1_024]);
    assert!(after_release < before_release);
    assert!(rms(&audio.channels[0][1_200..1_800]) > 1.0e-5);
}

#[test]
fn unison_produces_stereo_output_and_feedback_stays_finite() {
    let definition = operator_definition(
        OperatorModulationMode::Frequency,
        OperatorAlgorithm::Stack4,
        Some(UnisonDefinition {
            voices: 4,
            detune_cents: 18.0,
            stereo_spread: 0.9,
            phase_spread: 0.5,
        }),
    );
    let compiled = compile(&definition, 257);
    assert_eq!(
        compiled.layers[0].generator.output_mode(),
        sonalloy_core::compiler::GeneratorOutputMode::Stereo
    );
    let audio = render(&definition, 257, 2_048, &[note_on()]);
    assert_finite_and_audible(&audio);
    assert!(
        audio.channels[0]
            .iter()
            .zip(&audio.channels[1])
            .any(|(left, right)| (left - right).abs() > 1.0e-3)
    );
}

#[test]
fn negative_instantaneous_frequency_is_clamped_without_failure() {
    let mut definition = operator_definition(
        OperatorModulationMode::Frequency,
        OperatorAlgorithm::Stack4,
        None,
    );
    if let GeneratorDefinition::OperatorModulation(operator) = &mut definition.layers[0].generator {
        operator.operators[1].modulation_amount = 8.0;
        operator.operators[2].modulation_amount = 8.0;
        operator.operators[3].modulation_amount = 8.0;
    }
    let audio = render(&definition, 257, 2_048, &[note_on()]);
    assert_finite_and_audible(&audio);
}

#[test]
fn operator_runtime_reset_restarts_state() {
    let definition = operator_definition(
        OperatorModulationMode::Phase,
        OperatorAlgorithm::Stack4,
        None,
    );
    let compiled = compile(&definition, 257);
    let mut runtime = compiled.instantiate();
    runtime
        .prepare(ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"))
        .expect("runtime prepares");
    let events = [ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 110,
        },
    }];
    let process_once = |runtime: &mut sonalloy_core::runtime::InstrumentRuntime| {
        let mut left = [0.0_f32; 64];
        let mut right = [0.0_f32; 64];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        runtime
            .process(sonalloy_core::ProcessBlock {
                frames: 64,
                context: sonalloy_core::ProcessContext {
                    absolute_frame: 0,
                    tempo_bpm: 120.0,
                },
                events: &events,
                output: &mut output,
            })
            .expect("process succeeds");
        (left, right)
    };
    let first = process_once(&mut runtime);
    runtime.reset().expect("runtime resets");
    let reset = process_once(&mut runtime);

    let mut fresh_runtime = compiled.instantiate();
    fresh_runtime
        .prepare(ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"))
        .expect("fresh runtime prepares");
    let fresh = process_once(&mut fresh_runtime);

    for (actual, expected) in reset
        .0
        .iter()
        .zip(first.0)
        .chain(reset.1.iter().zip(first.1))
    {
        assert_relative_eq!(*actual, expected, epsilon = 1.0e-6);
    }
    for (actual, expected) in reset
        .0
        .iter()
        .zip(fresh.0)
        .chain(reset.1.iter().zip(fresh.1))
    {
        assert_relative_eq!(*actual, expected, epsilon = 1.0e-6);
    }
}
