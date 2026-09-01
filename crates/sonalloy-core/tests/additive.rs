use std::path::PathBuf;
use std::sync::Arc;

use approx::assert_relative_eq;
use sonalloy_core::{
    AdditiveDefinition, AdditivePartialDefinition, AdsrDefinition, CompileContext, DiagnosticCode,
    GeneratorDefinition, InstrumentDefinition, InstrumentProcessor, ParameterOwner, ParameterScale,
    ParameterUnit, ProcessBlock, ProcessContext, ProcessEvent, ProcessEventKind, ProcessSpec,
    RenderRequest, RenderedAudio, ScheduledEvent, compile_instrument, render_instrument,
};

fn base_definition() -> InstrumentDefinition {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/basic-poly-synth.json");
    let mut definition: InstrumentDefinition =
        serde_json::from_str(&std::fs::read_to_string(path).expect("reference Definition exists"))
            .expect("reference Definition parses");
    definition.layers[0].gain_db = 0.0;
    definition.layers[0].pan = 0.0;
    definition.layers[0].envelope = AdsrDefinition {
        attack_seconds: 0.0,
        decay_seconds: 0.0,
        sustain_level: 1.0,
        release_seconds: 0.02,
    };
    definition.layers[0].processors.clear();
    definition.voice_processors.clear();
    definition.global_processors.clear();
    definition.modulation = None;
    definition
}

fn partial(
    id: &str,
    ratio: f32,
    amplitude_a: f32,
    amplitude_b: f32,
    phase: f32,
    envelope: Option<AdsrDefinition>,
) -> AdditivePartialDefinition {
    AdditivePartialDefinition {
        id: id.to_owned(),
        ratio,
        amplitude_a,
        amplitude_b,
        phase,
        envelope,
    }
}

fn additive_definition(partials: Vec<AdditivePartialDefinition>) -> InstrumentDefinition {
    let mut definition = base_definition();
    definition.layers[0].generator = GeneratorDefinition::Additive(AdditiveDefinition {
        phase_reset: true,
        morph: 0.0,
        spectrum_tilt_db_per_octave: 0.0,
        inharmonicity: 0.0,
        partials,
    });
    definition
}

fn compile(
    definition: &InstrumentDefinition,
    sample_rate: f64,
    block_size: usize,
) -> Arc<sonalloy_core::CompiledInstrument> {
    let result = compile_instrument(
        definition,
        &CompileContext {
            definition_base_dir: PathBuf::from("."),
            process_spec: ProcessSpec::new(sample_rate, block_size, 0, 2).expect("valid spec"),
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.severity != sonalloy_core::DiagnosticSeverity::Error }),
        "additive definition must compile: {:?}",
        result.diagnostics
    );
    result.instrument.expect("additive definition compiles")
}

fn render(
    compiled: Arc<sonalloy_core::CompiledInstrument>,
    sample_rate: f64,
    block_size: usize,
    duration_frames: u64,
    events: &[ScheduledEvent],
) -> RenderedAudio {
    render_instrument(
        compiled,
        RenderRequest {
            sample_rate,
            block_size,
            duration_frames,
            tail_frames: 0,
        },
        events,
    )
    .expect("additive render succeeds")
}

fn note_on(note_id: u64, note_number: u8) -> ScheduledEvent {
    ScheduledEvent {
        absolute_frame: 0,
        kind: ProcessEventKind::NoteOn {
            note_id,
            note_number,
            velocity: 110,
        },
    }
}

fn note_off(frame: u64, note_id: u64) -> ScheduledEvent {
    ScheduledEvent {
        absolute_frame: frame,
        kind: ProcessEventKind::NoteOff { note_id },
    }
}

fn parameter_event(
    compiled: &sonalloy_core::CompiledInstrument,
    id: &str,
    frame: u64,
    normalized: f32,
) -> ScheduledEvent {
    ScheduledEvent {
        absolute_frame: frame,
        kind: ProcessEventKind::ParameterChange {
            catalog_revision: compiled.parameter_catalog_revision(),
            parameter: compiled.parameter_handle(id).expect("parameter handle"),
            normalized,
        },
    }
}

fn assert_finite_and_audible(audio: &RenderedAudio) {
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
            .any(|sample| sample.abs() > 0.01),
        "rendered additive output is silent"
    );
}

fn rms(samples: &[f32]) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let count = samples.len() as f32;
    (samples.iter().map(|sample| sample * sample).sum::<f32>() / count).sqrt()
}

fn maximum_difference(left: &[f32], right: &[f32]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(left, right)| (left - right).abs())
        .fold(0.0_f32, f32::max)
}

fn positive_zero_crossings(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|window| window[0] <= 0.0 && window[1] > 0.0)
        .count()
}

fn diagnostic_contains(
    definition: &InstrumentDefinition,
    code: DiagnosticCode,
    path: &str,
) -> bool {
    definition
        .validate()
        .iter()
        .any(|diagnostic| diagnostic.code == code && diagnostic.path.as_deref() == Some(path))
}

fn process_runtime(
    runtime: &mut sonalloy_core::InstrumentRuntime,
    frames: usize,
    absolute_frame: u64,
    events: &[ProcessEvent],
) -> Vec<f32> {
    let mut left = vec![0.0_f32; frames];
    let mut right = vec![0.0_f32; frames];
    let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
    runtime
        .process(ProcessBlock {
            frames,
            context: ProcessContext {
                absolute_frame,
                tempo_bpm: 120.0,
                beat_position: 0.0,
                bar_position: 0.0,
                time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
                transport_state: sonalloy_core::TransportState::Playing,
            },
            events,
            input: &[],
            output: &mut output,
        })
        .expect("additive process succeeds");
    assert_eq!(
        left, right,
        "mono additive output must be mirrored to stereo"
    );
    left
}

#[test]
fn additive_compiles_partial_bank_and_parameter_contract() {
    let definition = additive_definition(vec![partial("fundamental", 1.0, 0.8, 0.6, 0.25, None)]);
    let compiled = compile(&definition, 48_000.0, 257);
    assert_eq!(
        compiled.layers[0].generator.output_mode(),
        sonalloy_core::GeneratorOutputMode::Mono
    );

    let sonalloy_core::compiler::CompiledGenerator::Additive(additive) =
        &compiled.layers[0].generator
    else {
        panic!("definition must compile to Additive");
    };
    assert_eq!(additive.partials.len(), 1);
    assert_eq!(additive.partials[0].id, "fundamental");
    assert_relative_eq!(additive.partials[0].ratio, 1.0);
    assert_relative_eq!(additive.partials[0].phase, 0.25);
    assert_eq!(additive.sine_table.len(), 4_097);
    assert!(additive.sine_table.iter().all(|sample| sample.is_finite()));

    let expected = [
        (
            "layer.body.generator.additive_morph",
            ParameterUnit::Normalized,
            0.0,
            1.0,
            0.0,
        ),
        (
            "layer.body.generator.additive_spectrum_tilt",
            ParameterUnit::DecibelsPerOctave,
            -24.0,
            12.0,
            0.0,
        ),
        (
            "layer.body.generator.additive_inharmonicity",
            ParameterUnit::Normalized,
            0.0,
            1.0,
            0.0,
        ),
    ];
    for (id, unit, min, max, default) in expected {
        let handle = compiled
            .parameter_handle(id)
            .expect("additive parameter handle");
        let descriptor = compiled
            .parameter_descriptor(handle)
            .expect("additive descriptor");
        assert_eq!(
            descriptor.owner,
            ParameterOwner::LayerGenerator {
                definition_index: 0
            }
        );
        assert_eq!(descriptor.unit, unit);
        assert_eq!(descriptor.scale, ParameterScale::Linear);
        assert_relative_eq!(descriptor.min, min);
        assert_relative_eq!(descriptor.max, max);
        assert_relative_eq!(descriptor.default, default);
        assert_relative_eq!(descriptor.smoothing_seconds, 0.010);
    }

    let sixty_four = additive_definition(
        (0usize..64)
            .map(|index| {
                partial(
                    &format!("partial_{index}"),
                    f32::from(u16::try_from(index).expect("partial index fits")) + 1.0,
                    if index == 0 { 1.0 } else { 0.0 },
                    if index == 0 { 1.0 } else { 0.0 },
                    0.0,
                    None,
                )
            })
            .collect(),
    );
    let compiled = compile(&sixty_four, 48_000.0, 257);
    let sonalloy_core::compiler::CompiledGenerator::Additive(additive) =
        &compiled.layers[0].generator
    else {
        panic!("64-partial definition must compile to Additive");
    };
    assert_eq!(additive.partials.len(), 64);
}

#[test]
#[allow(clippy::too_many_lines)]
fn additive_validation_rejects_invalid_partial_contracts() {
    let mut empty = additive_definition(Vec::new());
    assert!(diagnostic_contains(
        &empty,
        DiagnosticCode::RequiredFieldMissing,
        "layers[0].generator.additive.partials"
    ));

    let sixty_five = additive_definition(
        (0..65)
            .map(|index| partial(&format!("partial_{index}"), 1.0, 0.0, 1.0, 0.0, None))
            .collect(),
    );
    assert!(diagnostic_contains(
        &sixty_five,
        DiagnosticCode::GeneratorResourceLimitExceeded,
        "layers[0].generator.additive.partials"
    ));

    empty.layers[0].generator = GeneratorDefinition::Additive(AdditiveDefinition {
        phase_reset: true,
        morph: 0.0,
        spectrum_tilt_db_per_octave: 0.0,
        inharmonicity: 0.0,
        partials: vec![
            partial("duplicate", 0.125, 0.0, 1.0, 0.0, None),
            partial("duplicate", 64.0, 1.0, 0.0, 1.0, None),
        ],
    });
    assert!(diagnostic_contains(
        &empty,
        DiagnosticCode::IdDuplicated,
        "layers[0].generator.additive.partials[1].id"
    ));

    let mut invalid = additive_definition(vec![partial(
        "valid",
        1.0,
        1.0,
        1.0,
        0.0,
        Some(AdsrDefinition {
            attack_seconds: 31.0,
            decay_seconds: 0.0,
            sustain_level: 1.0,
            release_seconds: 0.0,
        }),
    )]);
    let GeneratorDefinition::Additive(additive) = &mut invalid.layers[0].generator else {
        panic!("fixture must be additive");
    };
    additive.morph = f32::NAN;
    additive.spectrum_tilt_db_per_octave = -25.0;
    additive.inharmonicity = 1.1;
    additive.partials[0].id.clear();
    additive.partials[0].ratio = 0.124;
    additive.partials[0].amplitude_a = -0.01;
    additive.partials[0].amplitude_b = 1.01;
    additive.partials[0].phase = 1.01;
    assert!(diagnostic_contains(
        &invalid,
        DiagnosticCode::ValueOutOfRange,
        "layers[0].generator.additive.morph"
    ));
    assert!(diagnostic_contains(
        &invalid,
        DiagnosticCode::ValueOutOfRange,
        "layers[0].generator.additive.spectrum_tilt_db_per_octave"
    ));
    assert!(diagnostic_contains(
        &invalid,
        DiagnosticCode::ValueOutOfRange,
        "layers[0].generator.additive.inharmonicity"
    ));
    assert!(diagnostic_contains(
        &invalid,
        DiagnosticCode::RequiredFieldMissing,
        "layers[0].generator.additive.partials[0].id"
    ));
    assert!(diagnostic_contains(
        &invalid,
        DiagnosticCode::ValueOutOfRange,
        "layers[0].generator.additive.partials[0].ratio"
    ));
    assert!(diagnostic_contains(
        &invalid,
        DiagnosticCode::ValueOutOfRange,
        "layers[0].generator.additive.partials[0].amplitude_a"
    ));
    assert!(diagnostic_contains(
        &invalid,
        DiagnosticCode::ValueOutOfRange,
        "layers[0].generator.additive.partials[0].amplitude_b"
    ));
    assert!(diagnostic_contains(
        &invalid,
        DiagnosticCode::ValueOutOfRange,
        "layers[0].generator.additive.partials[0].phase"
    ));
    assert!(diagnostic_contains(
        &invalid,
        DiagnosticCode::ValueOutOfRange,
        "layers[0].generator.additive.partials[0].envelope.attack_seconds"
    ));

    let silent = additive_definition(vec![partial("silent", 1.0, 0.0, 0.0, 0.0, None)]);
    assert!(diagnostic_contains(
        &silent,
        DiagnosticCode::DefinitionError,
        "layers[0].generator.additive.partials"
    ));

    let mut serialized = serde_json::to_value(additive_definition(vec![partial(
        "fundamental",
        1.0,
        1.0,
        1.0,
        0.0,
        None,
    )]))
    .expect("definition serializes");
    serialized["layers"][0]["generator"]["additive"]["unknown"] = serde_json::Value::Bool(true);
    let parsed: Result<InstrumentDefinition, _> = serde_json::from_value(serialized);
    assert!(parsed.is_err(), "unknown additive fields must be rejected");
}

#[test]
fn additive_renders_harmonic_fractional_and_inharmonic_spectra() {
    let fundamental_definition =
        additive_definition(vec![partial("fundamental", 1.0, 1.0, 1.0, 0.0, None)]);
    let fundamental = render(
        compile(&fundamental_definition, 48_000.0, 257),
        48_000.0,
        257,
        24_576,
        &[note_on(1, 60)],
    );
    assert_finite_and_audible(&fundamental);
    let analysis = &fundamental.channels[0][4_096..20_480];
    #[allow(clippy::cast_precision_loss)]
    let frequency = positive_zero_crossings(analysis) as f32 * 48_000.0 / analysis.len() as f32;
    assert!(
        (frequency - 261.625_58).abs() < 1.0,
        "estimated frequency: {frequency}"
    );

    let harmonic = additive_definition(vec![
        partial("fundamental", 1.0, 1.0, 1.0, 0.0, None),
        partial("octave", 2.0, 0.5, 0.5, 0.0, None),
        partial("fifth", 3.0, 0.3, 0.3, 0.0, None),
    ]);
    let fractional = additive_definition(vec![
        partial("fundamental", 1.0, 1.0, 1.0, 0.0, None),
        partial("fractional", 2.73, 0.5, 0.5, 0.0, None),
        partial("fifth", 3.0, 0.3, 0.3, 0.0, None),
    ]);
    let harmonic_audio = render(
        compile(&harmonic, 48_000.0, 257),
        48_000.0,
        257,
        8_192,
        &[note_on(1, 60)],
    );
    let fractional_audio = render(
        compile(&fractional, 48_000.0, 257),
        48_000.0,
        257,
        8_192,
        &[note_on(1, 60)],
    );
    assert_finite_and_audible(&harmonic_audio);
    assert_finite_and_audible(&fractional_audio);
    assert!(
        maximum_difference(&harmonic_audio.channels[0], &fractional_audio.channels[0]) > 0.01,
        "fractional ratio must change the rendered spectrum"
    );

    let inharmonic = {
        let mut value = harmonic.clone();
        let GeneratorDefinition::Additive(additive) = &mut value.layers[0].generator else {
            panic!("fixture must be additive");
        };
        additive.inharmonicity = 1.0;
        value
    };
    let inharmonic_audio = render(
        compile(&inharmonic, 48_000.0, 257),
        48_000.0,
        257,
        8_192,
        &[note_on(1, 60)],
    );
    assert_finite_and_audible(&inharmonic_audio);
    assert!(
        maximum_difference(&harmonic_audio.channels[0], &inharmonic_audio.channels[0]) > 0.001,
        "global inharmonicity must change higher partials"
    );

    let fundamental_inharmonic = {
        let mut value = fundamental_definition.clone();
        let GeneratorDefinition::Additive(additive) = &mut value.layers[0].generator else {
            panic!("fixture must be additive");
        };
        additive.inharmonicity = 1.0;
        value
    };
    let fundamental_inharmonic_audio = render(
        compile(&fundamental_inharmonic, 48_000.0, 257),
        48_000.0,
        257,
        8_192,
        &[note_on(1, 60)],
    );
    for (base, stretched) in fundamental.channels[0]
        .iter()
        .zip(&fundamental_inharmonic_audio.channels[0])
        .skip(1_000)
    {
        assert_relative_eq!(*base, *stretched, epsilon = 1.0e-6);
    }
}

#[test]
fn additive_dynamic_spectrum_parameters_ramp_without_block_dependency() {
    let mut definition = additive_definition(vec![
        partial("fundamental", 1.0, 1.0, 0.2, 0.0, None),
        partial("third", 3.0, 0.0, 0.8, 0.0, None),
    ]);
    let GeneratorDefinition::Additive(additive) = &mut definition.layers[0].generator else {
        panic!("fixture must be additive");
    };
    additive.spectrum_tilt_db_per_octave = -24.0;
    let compiled = compile(&definition, 48_000.0, 257);
    let events = [
        note_on(1, 60),
        parameter_event(&compiled, "layer.body.generator.additive_morph", 2_048, 1.0),
        parameter_event(
            &compiled,
            "layer.body.generator.additive_spectrum_tilt",
            2_048,
            1.0,
        ),
        parameter_event(
            &compiled,
            "layer.body.generator.additive_inharmonicity",
            2_048,
            1.0,
        ),
    ];
    let dynamic = render(Arc::clone(&compiled), 48_000.0, 257, 16_384, &events);

    let mut start_definition = definition.clone();
    let GeneratorDefinition::Additive(additive) = &mut start_definition.layers[0].generator else {
        panic!("fixture must be additive");
    };
    additive.morph = 0.0;
    additive.spectrum_tilt_db_per_octave = -24.0;
    additive.inharmonicity = 0.0;
    let start = render(
        compile(&start_definition, 48_000.0, 257),
        48_000.0,
        257,
        16_384,
        &[note_on(1, 60)],
    );
    assert!(
        maximum_difference(&dynamic.channels[0][..2_048], &start.channels[0][..2_048]) < 1.0e-6
    );
    assert!(
        maximum_difference(&dynamic.channels[0][12_000..], &start.channels[0][12_000..]) > 0.001
    );
    assert!(
        maximum_difference(
            &dynamic.channels[0][4_096..8_192],
            &start.channels[0][4_096..8_192]
        ) > 0.001
    );
}

#[test]
fn additive_partial_envelopes_note_off_reset_and_voice_stealing_are_deterministic() {
    let envelope = AdsrDefinition {
        attack_seconds: 0.0,
        decay_seconds: 0.01,
        sustain_level: 0.0,
        release_seconds: 0.01,
    };
    let definition = additive_definition(vec![partial(
        "transient",
        3.0,
        0.8,
        0.8,
        0.0,
        Some(envelope),
    )]);
    let compiled = compile(&definition, 48_000.0, 257);
    let rendered = render(
        Arc::clone(&compiled),
        48_000.0,
        257,
        8_192,
        &[note_on(1, 60), note_off(4_096, 1)],
    );
    assert_finite_and_audible(&rendered);
    assert!(rms(&rendered.channels[0][128..256]) > rms(&rendered.channels[0][2_048..2_560]));
    assert!(rms(&rendered.channels[0][6_000..7_000]) < 0.01);

    let mut runtime = compiled.instantiate();
    let spec = ProcessSpec::new(48_000.0, 1_024, 0, 2).expect("valid spec");
    runtime.prepare(spec).expect("runtime preparation");
    runtime.activate().expect("runtime activation");
    let event = [ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 110,
        },
    }];
    let first = process_runtime(&mut runtime, 512, 0, &event);
    runtime.reset().expect("runtime reset");
    let reset = process_runtime(&mut runtime, 512, 0, &event);
    assert_eq!(
        first, reset,
        "reset must restore additive phase and envelopes"
    );

    let mut fresh = compiled.instantiate();
    fresh.prepare(spec).expect("fresh runtime preparation");
    fresh.activate().expect("fresh runtime activation");
    let fresh_output = process_runtime(&mut fresh, 512, 0, &event);
    assert_eq!(
        reset, fresh_output,
        "reset must match a fresh additive runtime"
    );

    let mut stealing_definition = definition;
    stealing_definition.performance = sonalloy_core::PerformanceDefinition::Polyphonic {
        polyphony: 1,
        voice_stealing: sonalloy_core::VoiceStealingDefinition::QuietestReleasingThenOldest,
    };
    let stealing = compile(&stealing_definition, 48_000.0, 257);
    let mut stealing_runtime = stealing.instantiate();
    stealing_runtime
        .prepare(spec)
        .expect("stealing preparation");
    stealing_runtime.activate().expect("stealing activation");
    let first_note = [ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 110,
        },
    }];
    let second_note = [ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 2,
            note_number: 67,
            velocity: 110,
        },
    }];
    let _ = process_runtime(&mut stealing_runtime, 512, 0, &first_note);
    let output = process_runtime(&mut stealing_runtime, 512, 512, &second_note);
    assert!(output.iter().any(|sample| sample.abs() > 0.01));
    assert_eq!(
        stealing_runtime.voice_state(0),
        Some(sonalloy_core::VoiceState::Active)
    );
}

#[test]
fn additive_render_is_independent_of_block_size_and_sample_rate() {
    let definition = additive_definition(vec![
        partial("fundamental", 1.0, 1.0, 0.8, 0.0, None),
        partial("second", 2.0, 0.5, 0.7, 0.0, None),
        partial("fractional", 2.73, 0.25, 0.5, 0.25, None),
        partial("sixth", 6.0, 0.15, 0.3, 0.0, None),
    ]);
    let mut renders = Vec::new();
    for block_size in [32, 64, 257, 1_024] {
        let compiled = compile(&definition, 48_000.0, block_size);
        let events = [
            note_on(1, 60),
            parameter_event(&compiled, "layer.body.generator.additive_morph", 1_024, 1.0),
            parameter_event(
                &compiled,
                "layer.body.generator.additive_inharmonicity",
                3_072,
                0.75,
            ),
        ];
        renders.push(render(compiled, 48_000.0, block_size, 8_192, &events));
    }
    for candidate in &renders[1..] {
        for (reference, candidate) in renders[0]
            .channels
            .iter()
            .flatten()
            .zip(candidate.channels.iter().flatten())
        {
            assert_relative_eq!(*reference, *candidate, epsilon = 1.0e-5);
        }
    }

    for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
        let compiled = compile(&definition, sample_rate, 257);
        let audio = render(compiled, sample_rate, 257, 4_096, &[note_on(1, 96)]);
        assert_finite_and_audible(&audio);
        assert_eq!(audio.channels[0], audio.channels[1]);
    }
}
