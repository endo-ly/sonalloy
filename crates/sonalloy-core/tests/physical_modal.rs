use std::path::PathBuf;
use std::sync::Arc;

use sonalloy_core::compiler::CompiledGenerator;
use sonalloy_core::{
    AdsrDefinition, ChorusProcessorDefinition, CompileContext, CompressorProcessorDefinition,
    DriveProcessorDefinition, FilterModeDefinition, FilterProcessorDefinition, GeneratorDefinition,
    InstrumentDefinition, InstrumentProcessor, LayerTriggerEvent, LfoDefinition, LfoWaveform,
    LimiterProcessorDefinition, ModEnvelopeDefinition, ModalDefinition, ModulationCurve,
    ModulationDefinition, ModulationDepthDefinition, ModulationRouteDefinition,
    ModulationSourceDefinition, MusicalTimeMap, ParameterUnit, PhysicalExciterDefinition,
    PhysicalStringDefinition, ProcessBlock, ProcessContext, ProcessEvent, ProcessEventKind,
    ProcessSpec, ProcessorDefinition, RenderRequest, ResonatorProcessorDefinition,
    ReverbProcessorDefinition, ScheduledEvent, TraceRequest, compile_instrument, render_instrument,
    render_instrument_with_reset, render_instrument_with_trace,
};

fn reference_definition() -> InstrumentDefinition {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/basic-poly-synth.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("reference Definition exists"))
        .expect("reference Definition parses")
}

fn physical_modal_definition() -> InstrumentDefinition {
    let mut definition = reference_definition();
    definition.performance = sonalloy_core::PerformanceDefinition::Polyphonic {
        polyphony: 4,
        voice_stealing: sonalloy_core::VoiceStealingDefinition::QuietestReleasingThenOldest,
    };
    definition.voice_processors.clear();
    definition.modulation = None;
    "string".clone_into(&mut definition.layers[0].id);
    definition.layers[0].gain_db = 0.0;
    definition.layers[0].envelope = AdsrDefinition {
        attack_seconds: 0.0,
        decay_seconds: 0.0,
        sustain_level: 1.0,
        release_seconds: 0.05,
    };
    definition.layers[0].generator =
        GeneratorDefinition::PhysicalString(PhysicalStringDefinition {
            exciter: PhysicalExciterDefinition::NoiseBurst {
                duration_seconds: 0.01,
                brightness: 0.5,
                seed: 17,
            },
            decay_seconds: 1.5,
            brightness: 0.6,
            stiffness: 0.35,
        });
    let mut modal = definition.layers[0].clone();
    "modal".clone_into(&mut modal.id);
    modal.generator = GeneratorDefinition::Modal(ModalDefinition {
        exciter: PhysicalExciterDefinition::Impulse,
        mode_count: 8,
        structure: 0.5,
        brightness: 0.65,
        decay: 0.7,
    });
    definition.layers.push(modal);
    definition
}

fn compile(
    definition: &InstrumentDefinition,
    sample_rate: f64,
    block_size: usize,
) -> Arc<sonalloy_core::CompiledInstrument> {
    compile_instrument(
        definition,
        &CompileContext {
            definition_base_dir: PathBuf::from("."),
            process_spec: ProcessSpec::new(sample_rate, block_size, 2).expect("valid process spec"),
        },
    )
    .instrument
    .expect("physical/modal definition compiles")
}

fn note_on() -> ScheduledEvent {
    ScheduledEvent {
        absolute_frame: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 7,
            note_number: 60,
            velocity: 100,
        },
    }
}

fn render(
    definition: &InstrumentDefinition,
    sample_rate: f64,
    block_size: usize,
    duration_frames: usize,
) -> sonalloy_core::RenderedAudio {
    let instrument = compile(definition, sample_rate, block_size);
    render_instrument(
        instrument,
        RenderRequest {
            sample_rate,
            block_size,
            duration_frames: u64::try_from(duration_frames).expect("test duration fits in u64"),
            tail_frames: 0,
        },
        &[note_on()],
    )
    .expect("physical/modal render succeeds")
}

fn process_runtime(
    runtime: &mut sonalloy_core::InstrumentRuntime,
    frames: usize,
    absolute_frame: u64,
    events: &[ProcessEvent],
) -> [Vec<f32>; 2] {
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    let mut output: [&mut [f32]; 2] = [left.as_mut_slice(), right.as_mut_slice()];
    runtime
        .process(ProcessBlock {
            frames,
            context: ProcessContext {
                absolute_frame,
                tempo_bpm: 120.0,
                beat_position: 0.0,
                bar_position: 0.0,
                time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
            },
            events,
            output: &mut output,
        })
        .expect("runtime process succeeds");
    [left, right]
}

fn max_abs_and_rms_difference(left: &[f32], right: &[f32]) -> (f32, f32) {
    assert_eq!(left.len(), right.len());
    let mut max_abs = 0.0_f32;
    let mut squared_sum = 0.0_f64;
    for (left, right) in left.iter().zip(right) {
        let difference = left - right;
        max_abs = max_abs.max(difference.abs());
        squared_sum += f64::from(difference) * f64::from(difference);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let rms = (squared_sum / left.len() as f64).sqrt() as f32;
    (max_abs, rms)
}

#[test]
fn definition_json_round_trip_and_validation_cover_physical_contracts() {
    let definition = physical_modal_definition();
    let json = serde_json::to_string(&definition).expect("definition serializes");
    let restored: InstrumentDefinition =
        serde_json::from_str(&json).expect("definition round-trips");
    assert_eq!(definition, restored);

    let mut unknown = serde_json::to_value(&definition).expect("definition serializes");
    unknown["layers"][0]["generator"]["physical_string"]["exciter"]["unexpected"] = true.into();
    assert!(serde_json::from_value::<InstrumentDefinition>(unknown).is_err());

    let mut invalid = definition;
    let GeneratorDefinition::Modal(modal) = &mut invalid.layers[1].generator else {
        panic!("modal fixture must use the modal generator");
    };
    modal.mode_count = 6;
    modal.decay = f32::NAN;
    let diagnostics = invalid.validate();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path.as_deref() == Some("layers[1].generator.modal.mode_count")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path.as_deref() == Some("layers[1].generator.modal.decay")
    }));
}

#[test]
fn every_supported_modal_mode_count_compiles_and_renders() {
    for mode_count in [4, 8, 12, 16, 20, 24] {
        let mut definition = physical_modal_definition();
        let GeneratorDefinition::Modal(modal) = &mut definition.layers[1].generator else {
            panic!("modal fixture must use the modal generator");
        };
        modal.mode_count = mode_count;
        assert!(definition.validate().is_empty(), "mode count {mode_count}");
        let audio = render(&definition, 48_000.0, 257, 512);
        assert!(
            audio
                .channels
                .iter()
                .flatten()
                .all(|sample| sample.is_finite()),
            "mode count {mode_count}"
        );
    }
}

#[test]
fn modal_mode_count_changes_resonance_density() {
    let mut definition = physical_modal_definition();
    definition.layers[0].enabled = false;
    {
        let GeneratorDefinition::Modal(modal) = &mut definition.layers[1].generator else {
            panic!("modal fixture must use the modal generator");
        };
        modal.brightness = 0.92;
        modal.mode_count = 12;
    }
    let twelve_modes = render(&definition, 48_000.0, 257, 2_048);
    {
        let GeneratorDefinition::Modal(modal) = &mut definition.layers[1].generator else {
            panic!("modal fixture must use the modal generator");
        };
        modal.mode_count = 24;
    }
    let twenty_four_modes = render(&definition, 48_000.0, 257, 2_048);

    let (max_abs, rms) =
        max_abs_and_rms_difference(&twelve_modes.channels[0], &twenty_four_modes.channels[0]);
    assert!(
        max_abs > 1.0e-3 && rms > 1.0e-4,
        "mode count difference is too small: max {max_abs}, rms {rms}"
    );
}

#[test]
fn compile_exposes_the_declared_parameter_contract() {
    let instrument = compile(&physical_modal_definition(), 48_000.0, 257);
    assert!(matches!(
        instrument.layers[0].generator,
        CompiledGenerator::PhysicalString(_)
    ));
    assert!(matches!(
        instrument.layers[1].generator,
        CompiledGenerator::Modal(_)
    ));
    let expected: [(&str, ParameterUnit, f32, f32); 6] = [
        (
            "layer.string.generator.physical_string_decay_seconds",
            ParameterUnit::Seconds,
            0.05,
            20.0,
        ),
        (
            "layer.string.generator.physical_string_brightness",
            ParameterUnit::Normalized,
            0.0,
            1.0,
        ),
        (
            "layer.string.generator.physical_string_stiffness",
            ParameterUnit::Normalized,
            0.0,
            1.0,
        ),
        (
            "layer.modal.generator.modal_structure",
            ParameterUnit::Normalized,
            0.0,
            1.0,
        ),
        (
            "layer.modal.generator.modal_brightness",
            ParameterUnit::Normalized,
            0.0,
            1.0,
        ),
        (
            "layer.modal.generator.modal_decay",
            ParameterUnit::Normalized,
            0.0,
            1.0,
        ),
    ];
    for (id, unit, min, max) in expected {
        let descriptor = instrument
            .parameters()
            .iter()
            .find(|descriptor| descriptor.id == id)
            .unwrap_or_else(|| panic!("missing parameter {id}"));
        assert_eq!(descriptor.unit, unit);
        assert_eq!(descriptor.min.to_bits(), min.to_bits());
        assert_eq!(descriptor.max.to_bits(), max.to_bits());
        assert_eq!(descriptor.smoothing_seconds.to_bits(), 0.010_f32.to_bits());
    }
}

#[test]
fn two_layer_render_is_finite_at_supported_sample_rates() {
    let definition = physical_modal_definition();
    for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
        let audio = render(&definition, sample_rate, 257, 2_048);
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
                .any(|sample| sample.abs() > 1.0e-6)
        );
    }
}

#[test]
fn constant_render_is_bit_exact_across_block_sizes_and_reset_repeats() {
    let definition = physical_modal_definition();
    let reference = render(&definition, 48_000.0, 32, 2_048);
    for block_size in [64, 257, 1_024] {
        let candidate = render(&definition, 48_000.0, block_size, 2_048);
        assert_eq!(
            reference.channels[0]
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>(),
            candidate.channels[0]
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            reference.channels[1]
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>(),
            candidate.channels[1]
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn parameter_changes_are_stable_across_required_block_sizes() {
    let definition = physical_modal_definition();
    let render_dynamic = |block_size: usize| {
        let instrument = compile(&definition, 48_000.0, block_size);
        let brightness = instrument
            .parameter_handle("layer.string.generator.physical_string_brightness")
            .expect("brightness handle");
        let stiffness = instrument
            .parameter_handle("layer.string.generator.physical_string_stiffness")
            .expect("stiffness handle");
        let structure = instrument
            .parameter_handle("layer.modal.generator.modal_structure")
            .expect("structure handle");
        let events = [
            note_on(),
            ScheduledEvent {
                absolute_frame: 64,
                kind: ProcessEventKind::ParameterChange {
                    parameter: brightness,
                    normalized: 0.15,
                },
            },
            ScheduledEvent {
                absolute_frame: 128,
                kind: ProcessEventKind::ParameterChange {
                    parameter: stiffness,
                    normalized: 0.9,
                },
            },
            ScheduledEvent {
                absolute_frame: 192,
                kind: ProcessEventKind::ParameterChange {
                    parameter: structure,
                    normalized: 0.2,
                },
            },
        ];
        render_instrument(
            instrument,
            RenderRequest {
                sample_rate: 48_000.0,
                block_size,
                duration_frames: 2_048,
                tail_frames: 0,
            },
            &events,
        )
        .expect("dynamic render succeeds")
    };

    let reference = render_dynamic(257);
    for block_size in [32, 64, 1_024] {
        let candidate = render_dynamic(block_size);
        for (reference_channel, candidate_channel) in
            reference.channels.iter().zip(&candidate.channels)
        {
            let (max_abs, rms) = max_abs_and_rms_difference(reference_channel, candidate_channel);
            assert!(
                max_abs <= 1.0e-4 && rms <= 1.0e-5,
                "block size {block_size} differs by max {max_abs} and rms {rms}"
            );
        }
    }
}

#[test]
fn reset_then_same_note_matches_fresh_runtime() {
    let definition = physical_modal_definition();
    let instrument = compile(&definition, 48_000.0, 257);
    let mut runtime = instrument.instantiate();
    runtime
        .prepare(ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"))
        .expect("runtime prepares");
    let events = [ProcessEvent {
        sample_offset: 0,
        kind: note_on().kind,
    }];

    let first = process_runtime(&mut runtime, 257, 0, &events);
    runtime.reset().expect("runtime resets");
    let after_reset = process_runtime(&mut runtime, 257, 0, &events);
    let mut fresh_runtime = instrument.instantiate();
    fresh_runtime
        .prepare(ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"))
        .expect("fresh runtime prepares");
    let fresh = process_runtime(&mut fresh_runtime, 257, 0, &events);

    assert_eq!(first, after_reset);
    assert_eq!(first, fresh);

    let (rendered_first, rendered_after_reset) = render_instrument_with_reset(
        Arc::clone(&instrument),
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 257,
            tail_frames: 0,
        },
        &[note_on()],
        &MusicalTimeMap::constant(120.0).expect("constant tempo"),
    )
    .expect("prepared runtime reset render succeeds");
    assert_eq!(rendered_first.channels, rendered_after_reset.channels);
    assert_eq!(rendered_first.channels[0], first[0]);
    assert_eq!(rendered_first.channels[1], first[1]);
}

#[test]
#[allow(clippy::too_many_lines)]
fn parameter_change_and_note_off_keep_both_layers_active() {
    let definition = physical_modal_definition();
    let baseline = render(&definition, 48_000.0, 257, 257);
    let instrument = compile(&definition, 48_000.0, 257);
    let brightness = instrument
        .parameter_handle("layer.string.generator.physical_string_brightness")
        .expect("physical string brightness handle");
    let stiffness = instrument
        .parameter_handle("layer.string.generator.physical_string_stiffness")
        .expect("physical string stiffness handle");
    let structure = instrument
        .parameter_handle("layer.modal.generator.modal_structure")
        .expect("modal structure handle");
    let decay = instrument
        .parameter_handle("layer.modal.generator.modal_decay")
        .expect("modal decay handle");
    let mut runtime = instrument.instantiate();
    runtime
        .prepare(ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"))
        .expect("runtime prepares");
    let mut left = vec![0.0; 257];
    let mut right = vec![0.0; 257];
    let parameter_events = [
        ProcessEvent {
            sample_offset: 0,
            kind: note_on().kind,
        },
        ProcessEvent {
            sample_offset: 64,
            kind: ProcessEventKind::ParameterChange {
                parameter: brightness,
                normalized: 0.9,
            },
        },
        ProcessEvent {
            sample_offset: 96,
            kind: ProcessEventKind::ParameterChange {
                parameter: stiffness,
                normalized: 0.85,
            },
        },
        ProcessEvent {
            sample_offset: 128,
            kind: ProcessEventKind::ParameterChange {
                parameter: structure,
                normalized: 0.2,
            },
        },
        ProcessEvent {
            sample_offset: 160,
            kind: ProcessEventKind::ParameterChange {
                parameter: decay,
                normalized: 0.9,
            },
        },
    ];
    {
        let mut output: [&mut [f32]; 2] = [left.as_mut_slice(), right.as_mut_slice()];
        runtime
            .process(ProcessBlock {
                frames: 257,
                context: ProcessContext {
                    absolute_frame: 0,
                    tempo_bpm: 120.0,
                    beat_position: 0.0,
                    bar_position: 0.0,
                    time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
                },
                events: &parameter_events,
                output: &mut output,
            })
            .expect("parameter event renders");
    }
    assert!(
        left.iter()
            .chain(&right)
            .any(|sample| sample.abs() > 1.0e-6)
    );
    assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
    assert!(
        baseline.channels[0]
            .iter()
            .zip(&left)
            .any(|(before, after)| (before - after).abs() > 1.0e-6)
    );
    let note_off_events = [ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOff { note_id: 7 },
    }];
    {
        let mut output: [&mut [f32]; 2] = [left.as_mut_slice(), right.as_mut_slice()];
        runtime
            .process(ProcessBlock {
                frames: 257,
                context: ProcessContext {
                    absolute_frame: 257,
                    tempo_bpm: 120.0,
                    beat_position: 0.0,
                    bar_position: 0.0,
                    time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
                },
                events: &note_off_events,
                output: &mut output,
            })
            .expect("note off renders release");
    }
    assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
}

#[test]
fn physical_modal_generators_survive_the_existing_processor_chain() {
    let mut definition = physical_modal_definition();
    definition.layers[0].processors = vec![
        ProcessorDefinition::Filter(FilterProcessorDefinition {
            id: "string_tone".to_owned(),
            mode: FilterModeDefinition::LowPass,
            cutoff_hz: 8_000.0,
            resonance: 0.15,
        }),
        ProcessorDefinition::Resonator(ResonatorProcessorDefinition {
            id: "string_body".to_owned(),
            frequency_hz: 330.0,
            decay_seconds: 0.45,
            damping: 0.4,
            mix: 0.25,
        }),
    ];
    definition.voice_processors = vec![
        ProcessorDefinition::Drive(DriveProcessorDefinition {
            id: "voice_edge".to_owned(),
            amount: 0.15,
            mix: 0.2,
        }),
        ProcessorDefinition::Compressor(CompressorProcessorDefinition {
            id: "voice_glue".to_owned(),
            threshold_db: -20.0,
            ratio: 2.5,
            attack_ms: 8.0,
            release_ms: 120.0,
            knee_db: 6.0,
            makeup_gain_db: 1.0,
            mix: 0.7,
        }),
    ];
    definition.global_processors = vec![
        ProcessorDefinition::Chorus(ChorusProcessorDefinition {
            id: "global_width".to_owned(),
            delay_ms: 14.0,
            rate_hz: 0.22,
            depth: 0.35,
            feedback: 0.08,
            width: 0.75,
            mix: 0.16,
        }),
        ProcessorDefinition::Reverb(ReverbProcessorDefinition {
            id: "global_space".to_owned(),
            pre_delay_seconds: 0.015,
            decay: 0.42,
            damping: 0.32,
            width: 0.95,
            mix: 0.2,
        }),
        ProcessorDefinition::Limiter(LimiterProcessorDefinition {
            id: "global_ceiling".to_owned(),
            ceiling_db: -1.0,
            release_ms: 80.0,
            input_gain_db: 0.0,
        }),
    ];
    let diagnostics = definition.validate();
    assert!(
        diagnostics.is_empty(),
        "processor diagnostics: {diagnostics:?}"
    );
    let audio = render(&definition, 48_000.0, 257, 4_096);
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
            .any(|sample| sample.abs() > 1.0e-6)
    );
}

#[test]
fn modulation_routes_reach_physical_modal_parameters() {
    let mut definition = physical_modal_definition();
    definition.modulation = Some(ModulationDefinition {
        sources: vec![
            ModulationSourceDefinition::Lfo(LfoDefinition {
                id: "physical_lfo".to_owned(),
                waveform: LfoWaveform::Sine,
                rate: sonalloy_core::ModulationRateDefinition {
                    value: 2.0,
                    unit: sonalloy_core::ModulationRateUnit::PerSecond,
                },
                phase: 0.0,
            }),
            ModulationSourceDefinition::Envelope(ModEnvelopeDefinition {
                id: "physical_envelope".to_owned(),
                attack_seconds: 0.01,
                decay_seconds: 0.2,
                sustain_level: 0.5,
                release_seconds: 0.2,
            }),
        ],
        routes: vec![
            ModulationRouteDefinition {
                source: "physical_lfo".to_owned(),
                target: "layer.string.generator.physical_string_stiffness".to_owned(),
                depth: ModulationDepthDefinition {
                    value: 0.5,
                    unit: sonalloy_core::ModulationUnit::Normalized,
                },
                curve: ModulationCurve::Linear,
            },
            ModulationRouteDefinition {
                source: "physical_envelope".to_owned(),
                target: "layer.string.generator.physical_string_brightness".to_owned(),
                depth: ModulationDepthDefinition {
                    value: 0.4,
                    unit: sonalloy_core::ModulationUnit::Normalized,
                },
                curve: ModulationCurve::Linear,
            },
            ModulationRouteDefinition {
                source: "velocity".to_owned(),
                target: "layer.modal.generator.modal_structure".to_owned(),
                depth: ModulationDepthDefinition {
                    value: 0.35,
                    unit: sonalloy_core::ModulationUnit::Normalized,
                },
                curve: ModulationCurve::Linear,
            },
            ModulationRouteDefinition {
                source: "mod_wheel".to_owned(),
                target: "layer.modal.generator.modal_decay".to_owned(),
                depth: ModulationDepthDefinition {
                    value: 0.5,
                    unit: sonalloy_core::ModulationUnit::Normalized,
                },
                curve: ModulationCurve::Linear,
            },
        ],
    });
    let diagnostics = definition.validate();
    assert!(
        diagnostics.is_empty(),
        "modulation diagnostics: {diagnostics:?}"
    );
    let routed_instrument = compile(&definition, 48_000.0, 257);
    let baseline = render(&physical_modal_definition(), 48_000.0, 257, 4_096);
    let routed = render_instrument(
        routed_instrument,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 4_096,
            tail_frames: 0,
        },
        &[
            note_on(),
            ScheduledEvent {
                absolute_frame: 1_024,
                kind: ProcessEventKind::ModWheel { value: 1.0 },
            },
        ],
    )
    .expect("modulated physical/modal render succeeds");
    assert!(
        routed
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert!(
        baseline.channels[0]
            .iter()
            .zip(&routed.channels[0])
            .skip(1_024)
            .any(|(before, after)| (before - after).abs() > 1.0e-6)
    );
}

#[test]
fn modulation_trace_reports_final_values_for_physical_modal_targets() {
    let mut definition = physical_modal_definition();
    definition.modulation = Some(ModulationDefinition {
        sources: vec![ModulationSourceDefinition::Lfo(LfoDefinition {
            id: "trace_lfo".to_owned(),
            waveform: LfoWaveform::Sine,
            rate: sonalloy_core::ModulationRateDefinition {
                value: 2.0,
                unit: sonalloy_core::ModulationRateUnit::PerSecond,
            },
            phase: 0.0,
        })],
        routes: vec![
            ModulationRouteDefinition {
                source: "trace_lfo".to_owned(),
                target: "layer.string.generator.physical_string_stiffness".to_owned(),
                depth: ModulationDepthDefinition {
                    value: 0.4,
                    unit: sonalloy_core::ModulationUnit::Normalized,
                },
                curve: ModulationCurve::Linear,
            },
            ModulationRouteDefinition {
                source: "mod_wheel".to_owned(),
                target: "layer.modal.generator.modal_decay".to_owned(),
                depth: ModulationDepthDefinition {
                    value: 0.5,
                    unit: sonalloy_core::ModulationUnit::Normalized,
                },
                curve: ModulationCurve::Linear,
            },
        ],
    });
    let instrument = compile(&definition, 48_000.0, 257);
    let stiffness = instrument
        .parameter_handle("layer.string.generator.physical_string_stiffness")
        .expect("stiffness handle");
    let decay = instrument
        .parameter_handle("layer.modal.generator.modal_decay")
        .expect("decay handle");
    let (_, report) = render_instrument_with_trace(
        instrument,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 1_024,
            tail_frames: 0,
        },
        &[
            note_on(),
            ScheduledEvent {
                absolute_frame: 256,
                kind: ProcessEventKind::ModWheel { value: 1.0 },
            },
        ],
        &MusicalTimeMap::constant(120.0).expect("constant tempo"),
        &TraceRequest {
            parameters: vec![stiffness, decay],
            every_frames: 128,
        },
    )
    .expect("trace render succeeds");

    assert_eq!(report.parameters.len(), 2);
    for parameter in &report.parameters {
        assert!(
            !parameter.observations.is_empty(),
            "{}",
            parameter.parameter
        );
        assert!(parameter.observations.iter().all(|observation| {
            observation.final_value.is_finite()
                && observation.before_clamp.is_finite()
                && observation
                    .routes
                    .iter()
                    .all(|route| route.contribution.value.is_finite())
        }));
        assert!(
            parameter
                .observations
                .iter()
                .any(|observation| !observation.routes.is_empty())
        );
    }
    let decay_observations = &report.parameters[1].observations;
    assert!(decay_observations.iter().any(|observation| {
        observation
            .routes
            .iter()
            .any(|route| route.source == "mod_wheel" && route.raw > 0.0)
    }));
}

#[test]
fn polyphony_stealing_and_note_off_trigger_keep_physical_layers_finite() {
    let mut definition = physical_modal_definition();
    definition.performance = sonalloy_core::PerformanceDefinition::Polyphonic {
        polyphony: 2,
        voice_stealing: sonalloy_core::VoiceStealingDefinition::QuietestReleasingThenOldest,
    };
    definition.layers[1].trigger.event = sonalloy_core::LayerTriggerEvent::NoteOff;
    let instrument = compile(&definition, 48_000.0, 257);
    let audio = render_instrument(
        instrument,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 2_048,
            tail_frames: 0,
        },
        &[
            ScheduledEvent {
                absolute_frame: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 48,
                    velocity: 112,
                },
            },
            ScheduledEvent {
                absolute_frame: 128,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 60,
                    velocity: 96,
                },
            },
            ScheduledEvent {
                absolute_frame: 256,
                kind: ProcessEventKind::NoteOn {
                    note_id: 3,
                    note_number: 67,
                    velocity: 120,
                },
            },
            ScheduledEvent {
                absolute_frame: 768,
                kind: ProcessEventKind::NoteOff { note_id: 2 },
            },
            ScheduledEvent {
                absolute_frame: 1_024,
                kind: ProcessEventKind::NoteOff { note_id: 3 },
            },
        ],
    )
    .expect("physical/modal lifecycle render succeeds");
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
            .any(|sample| sample.abs() > 1.0e-6)
    );
}

#[test]
fn note_off_trigger_starts_a_modal_exciter() {
    let mut definition = physical_modal_definition();
    definition.layers.truncate(1);
    definition.layers[0].trigger.event = LayerTriggerEvent::NoteOff;
    definition.layers[0].generator = GeneratorDefinition::Modal(ModalDefinition {
        exciter: PhysicalExciterDefinition::Impulse,
        mode_count: 12,
        structure: 0.5,
        brightness: 0.65,
        decay: 0.7,
    });
    let instrument = compile(&definition, 48_000.0, 257);
    let audio = render_instrument(
        instrument,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
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
                absolute_frame: 256,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            },
        ],
    )
    .expect("note-off modal render succeeds");

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
            .take(256)
            .all(|sample| { sample.abs() <= 1.0e-7 })
    );
    assert!(
        audio
            .channels
            .iter()
            .flatten()
            .skip(256)
            .any(|sample| { sample.abs() > 1.0e-6 })
    );
}
