use std::path::PathBuf;
use std::sync::Arc;

use approx::assert_relative_eq;
use sonalloy_core::{
    CompileContext, DiagnosticCode, FormantBandDefinition, FormantDefinition,
    FormantProfileDefinition, GeneratorDefinition, InstrumentDefinition, InstrumentProcessor,
    ParameterOwner, ParameterScale, ParameterUnit, ProcessBlock, ProcessContext, ProcessEvent,
    ProcessEventKind, ProcessSpec, RenderRequest, RenderedAudio, ScheduledEvent,
    compile_instrument, render_instrument,
};

fn base_definition() -> InstrumentDefinition {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/basic-poly-synth.json");
    let mut definition: InstrumentDefinition =
        serde_json::from_str(&std::fs::read_to_string(path).expect("reference Definition exists"))
            .expect("reference Definition parses");
    definition.layers[0].gain_db = 0.0;
    definition.layers[0].pan = 0.0;
    definition.layers[0].envelope.attack_seconds = 0.0;
    definition.layers[0].envelope.decay_seconds = 0.0;
    definition.layers[0].envelope.sustain_level = 1.0;
    definition.layers[0].envelope.release_seconds = 0.02;
    definition.layers[0].processors.clear();
    definition.voice_processors.clear();
    definition.global_processors.clear();
    definition.modulation = None;
    definition
}

fn band(frequency_hz: f32, bandwidth_hz: f32, gain_db: f32) -> FormantBandDefinition {
    FormantBandDefinition {
        frequency_hz,
        bandwidth_hz,
        gain_db,
    }
}

fn profile(id: &str, first_frequency: f32) -> FormantProfileDefinition {
    FormantProfileDefinition {
        id: id.to_owned(),
        formants: vec![
            band(first_frequency, 100.0, 0.0),
            band(first_frequency + 300.0, 120.0, -5.0),
            band(first_frequency + 700.0, 140.0, -10.0),
            band(first_frequency + 1_100.0, 160.0, -15.0),
            band(first_frequency + 1_500.0, 180.0, -20.0),
        ],
    }
}

fn formant_definition(profiles: Vec<FormantProfileDefinition>) -> InstrumentDefinition {
    let mut definition = base_definition();
    definition.layers[0].generator = GeneratorDefinition::Formant(FormantDefinition {
        phase_reset: true,
        partial_count: 8,
        vowel_position: 0.0,
        formant_shift_cents: 0.0,
        throat: 0.5,
        spectral_tilt_db_per_octave: 0.0,
        profiles,
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
            .all(|diagnostic| diagnostic.severity != sonalloy_core::DiagnosticSeverity::Error),
        "formant definition must compile: {:?}",
        result.diagnostics
    );
    result.instrument.expect("formant definition compiles")
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
    .expect("formant render succeeds")
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
            parameter: compiled.parameter_handle(id).expect("parameter handle"),
            normalized,
        },
    }
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
        "rendered formant output is silent"
    );
}

fn maximum_difference(left: &[f32], right: &[f32]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(left, right)| (left - right).abs())
        .fold(0.0_f32, f32::max)
}

fn frequency_energy(samples: &[f32], sample_rate: f32, frequency: f32) -> f32 {
    let (real, imaginary) = samples.iter().enumerate().fold(
        (0.0_f32, 0.0_f32),
        |(real, imaginary), (index, sample)| {
            #[allow(clippy::cast_precision_loss)]
            let phase = std::f32::consts::TAU * frequency * index as f32 / sample_rate;
            (
                real + *sample * phase.cos(),
                imaginary - *sample * phase.sin(),
            )
        },
    );
    real * real + imaginary * imaginary
}

fn positive_zero_crossings(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|window| window[0] <= 0.0 && window[1] > 0.0)
        .count()
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
            },
            events,
            input: &[],
            output: &mut output,
        })
        .expect("formant process succeeds");
    assert_eq!(
        left, right,
        "mono formant output must be mirrored to stereo"
    );
    left
}

#[test]
fn formant_compiles_fixed_profiles_and_parameter_contract() {
    let definition = formant_definition(vec![profile("a", 800.0)]);
    let compiled = compile(&definition, 48_000.0, 257);
    assert_eq!(
        compiled.layers[0].generator.output_mode(),
        sonalloy_core::GeneratorOutputMode::Mono
    );
    let sonalloy_core::compiler::CompiledGenerator::Formant(formant) =
        &compiled.layers[0].generator
    else {
        panic!("definition must compile to Formant");
    };
    assert_eq!(formant.partial_count, 8);
    assert_eq!(formant.profiles.len(), 1);
    assert_relative_eq!(formant.profiles[0].formants[0].frequency_hz, 800.0);
    assert_eq!(formant.sine_table.len(), 4_097);
    assert!(formant.sine_table.iter().all(|sample| sample.is_finite()));

    let expected = [
        (
            "layer.body.generator.formant_vowel_position",
            ParameterUnit::Normalized,
            0.0,
            1.0,
            0.0,
        ),
        (
            "layer.body.generator.formant_shift",
            ParameterUnit::Cents,
            -2400.0,
            2400.0,
            0.0,
        ),
        (
            "layer.body.generator.formant_throat",
            ParameterUnit::Normalized,
            0.0,
            1.0,
            0.5,
        ),
        (
            "layer.body.generator.formant_spectral_tilt",
            ParameterUnit::DecibelsPerOctave,
            -24.0,
            12.0,
            0.0,
        ),
    ];
    for (id, unit, min, max, default) in expected {
        let handle = compiled
            .parameter_handle(id)
            .expect("formant parameter handle");
        let descriptor = compiled
            .parameter_descriptor(handle)
            .expect("formant descriptor");
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

    let mut sixty_four = formant_definition(vec![profile("a", 800.0)]);
    let GeneratorDefinition::Formant(formant) = &mut sixty_four.layers[0].generator else {
        panic!("fixture must be formant");
    };
    formant.partial_count = 64;
    assert_eq!(
        match &compile(&sixty_four, 48_000.0, 257).layers[0].generator {
            sonalloy_core::compiler::CompiledGenerator::Formant(value) => value.partial_count,
            _ => 0,
        },
        64
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn formant_validation_rejects_profile_band_and_parameter_contract_errors() {
    let mut empty = formant_definition(Vec::new());
    let GeneratorDefinition::Formant(formant) = &mut empty.layers[0].generator else {
        panic!("fixture must be formant");
    };
    formant.partial_count = 0;
    assert!(diagnostic_contains(
        &empty,
        DiagnosticCode::RequiredFieldMissing,
        "layers[0].generator.formant.profiles"
    ));
    assert!(diagnostic_contains(
        &empty,
        DiagnosticCode::RequiredFieldMissing,
        "layers[0].generator.formant.partial_count"
    ));

    let nine = formant_definition(
        (0..9)
            .map(|index| profile(&format!("p{index}"), 500.0))
            .collect(),
    );
    assert!(diagnostic_contains(
        &nine,
        DiagnosticCode::GeneratorResourceLimitExceeded,
        "layers[0].generator.formant.profiles"
    ));

    let mut invalid = formant_definition(vec![profile("same", 500.0), profile("same", 600.0)]);
    let GeneratorDefinition::Formant(formant) = &mut invalid.layers[0].generator else {
        panic!("fixture must be formant");
    };
    formant.vowel_position = f32::NAN;
    formant.formant_shift_cents = 2401.0;
    formant.throat = -0.01;
    formant.spectral_tilt_db_per_octave = f32::INFINITY;
    formant.profiles[0].id.clear();
    formant.profiles[1].id.clear();
    formant.profiles[0].formants[0].frequency_hz = 99.0;
    formant.profiles[0].formants[1].frequency_hz = 95.0;
    formant.profiles[0].formants[2].bandwidth_hz = 5_001.0;
    formant.profiles[0].formants[3].gain_db = 13.0;
    assert!(diagnostic_contains(
        &invalid,
        DiagnosticCode::IdDuplicated,
        "layers[0].generator.formant.profiles[1].id"
    ));
    assert!(diagnostic_contains(
        &invalid,
        DiagnosticCode::RequiredFieldMissing,
        "layers[0].generator.formant.profiles[0].id"
    ));
    for path in [
        "layers[0].generator.formant.vowel_position",
        "layers[0].generator.formant.formant_shift_cents",
        "layers[0].generator.formant.throat",
        "layers[0].generator.formant.spectral_tilt_db_per_octave",
        "layers[0].generator.formant.profiles[0].formants[0].frequency_hz",
        "layers[0].generator.formant.profiles[0].formants[2].bandwidth_hz",
        "layers[0].generator.formant.profiles[0].formants[3].gain_db",
    ] {
        assert!(
            diagnostic_contains(&invalid, DiagnosticCode::ValueOutOfRange, path),
            "missing diagnostic for {path}"
        );
    }
    assert!(diagnostic_contains(
        &invalid,
        DiagnosticCode::DefinitionError,
        "layers[0].generator.formant.profiles[0].formants[1].frequency_hz"
    ));

    let mut wrong_count = formant_definition(vec![profile("a", 500.0)]);
    {
        let GeneratorDefinition::Formant(formant) = &mut wrong_count.layers[0].generator else {
            panic!("fixture must be formant");
        };
        formant.profiles[0].formants.pop();
    }
    assert!(diagnostic_contains(
        &wrong_count,
        DiagnosticCode::DefinitionError,
        "layers[0].generator.formant.profiles[0].formants"
    ));
    {
        let GeneratorDefinition::Formant(formant) = &mut wrong_count.layers[0].generator else {
            panic!("fixture must be formant");
        };
        formant.profiles[0].formants.push(band(2_000.0, 100.0, 0.0));
        formant.profiles[0].formants.push(band(2_100.0, 100.0, 0.0));
    }
    assert!(diagnostic_contains(
        &wrong_count,
        DiagnosticCode::DefinitionError,
        "layers[0].generator.formant.profiles[0].formants"
    ));

    let mut serialized = serde_json::to_value(formant_definition(vec![profile("a", 500.0)]))
        .expect("definition serializes");
    serialized["layers"][0]["generator"]["formant"]["unknown"] = serde_json::Value::Bool(true);
    let parsed: Result<InstrumentDefinition, _> = serde_json::from_value(serialized);
    assert!(parsed.is_err(), "unknown formant fields must be rejected");
}

#[test]
fn formant_renders_harmonic_pitch_and_profile_morph() {
    let definition = formant_definition(vec![profile("a", 250.0), profile("i", 400.0)]);
    let compiled = compile(&definition, 48_000.0, 257);
    let static_audio = render(
        Arc::clone(&compiled),
        48_000.0,
        257,
        16_384,
        &[note_on(1, 60)],
    );
    assert_finite_and_audible(&static_audio);
    let analysis = &static_audio.channels[0][4_096..12_288];
    #[allow(clippy::cast_precision_loss)]
    let frequency = positive_zero_crossings(analysis) as f32 * 48_000.0 / analysis.len() as f32;
    assert!(
        (frequency - 261.625_58).abs() < 5.0,
        "estimated frequency: {frequency}"
    );

    let dynamic = render(
        Arc::clone(&compiled),
        48_000.0,
        257,
        16_384,
        &[
            note_on(1, 60),
            parameter_event(
                &compiled,
                "layer.body.generator.formant_vowel_position",
                2_048,
                1.0,
            ),
        ],
    );
    assert_finite_and_audible(&dynamic);
    assert!(
        maximum_difference(
            &dynamic.channels[0][..2_048],
            &static_audio.channels[0][..2_048]
        ) < 1.0e-6
    );
    assert!(
        maximum_difference(
            &dynamic.channels[0][8_192..],
            &static_audio.channels[0][8_192..]
        ) > 0.001
    );
}

#[test]
fn formant_shift_throat_and_tilt_change_the_spectrum_without_changing_note_pitch() {
    let definition = formant_definition(vec![profile("a", 250.0), profile("i", 400.0)]);
    let base = compile(&definition, 48_000.0, 257);
    let baseline = render(Arc::clone(&base), 48_000.0, 257, 16_384, &[note_on(1, 60)]);
    let shifted = render(
        Arc::clone(&base),
        48_000.0,
        257,
        16_384,
        &[
            note_on(1, 60),
            parameter_event(&base, "layer.body.generator.formant_shift", 2_048, 0.5625),
        ],
    );
    assert_finite_and_audible(&shifted);
    assert!(
        maximum_difference(
            &baseline.channels[0][8_192..],
            &shifted.channels[0][8_192..]
        ) > 0.001
    );
    let analysis = &shifted.channels[0][4_096..12_288];
    let fundamental_energy = frequency_energy(analysis, 48_000.0, 261.625_58);
    let second_harmonic_energy = frequency_energy(analysis, 48_000.0, 523.251_16);
    assert!(
        fundamental_energy > second_harmonic_energy * 10.0,
        "shift must retain the fundamental frequency"
    );

    let events = [
        note_on(1, 60),
        parameter_event(&base, "layer.body.generator.formant_throat", 2_048, 1.0),
        parameter_event(
            &base,
            "layer.body.generator.formant_spectral_tilt",
            2_048,
            1.0,
        ),
    ];
    let changed = render(Arc::clone(&base), 48_000.0, 257, 16_384, &events);
    assert_finite_and_audible(&changed);
    assert!(
        maximum_difference(
            &baseline.channels[0][8_192..],
            &changed.channels[0][8_192..]
        ) > 0.001
    );
}

#[test]
fn formant_reset_voice_stealing_and_block_sizes_are_deterministic() {
    let mut definition = formant_definition(vec![profile("a", 250.0), profile("i", 400.0)]);
    definition.performance = sonalloy_core::PerformanceDefinition::Polyphonic {
        polyphony: 1,
        voice_stealing: sonalloy_core::VoiceStealingDefinition::QuietestReleasingThenOldest,
    };
    let compiled = compile(&definition, 48_000.0, 1_024);
    let spec = ProcessSpec::new(48_000.0, 1_024, 0, 2).expect("valid spec");
    let event = [ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 110,
        },
    }];
    let mut runtime = compiled.instantiate();
    runtime.prepare(spec).expect("runtime preparation");
    let first = process_runtime(&mut runtime, 512, 0, &event);
    runtime.reset().expect("runtime reset");
    let reset = process_runtime(&mut runtime, 512, 0, &event);
    assert_eq!(
        first, reset,
        "reset must restore formant phase and controls"
    );

    let mut fresh = compiled.instantiate();
    fresh.prepare(spec).expect("fresh runtime preparation");
    assert_eq!(reset, process_runtime(&mut fresh, 512, 0, &event));
    let second_note = [ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 2,
            note_number: 67,
            velocity: 110,
        },
    }];
    let output = process_runtime(&mut runtime, 512, 512, &second_note);
    assert!(output.iter().any(|sample| sample.abs() > 0.01));
    assert_eq!(
        runtime.voice_state(0),
        Some(sonalloy_core::VoiceState::Active)
    );

    let mut renders = Vec::new();
    for block_size in [32, 64, 257, 1_024] {
        let compiled = compile(&definition, 48_000.0, block_size);
        renders.push(render(
            compiled,
            48_000.0,
            block_size,
            8_192,
            &[note_on(1, 60), note_off(4_096, 1)],
        ));
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
}

#[test]
fn formant_remains_finite_at_supported_sample_rates_and_high_notes() {
    let definition = formant_definition(vec![profile("a", 800.0), profile("i", 270.0)]);
    for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
        let audio = render(
            compile(&definition, sample_rate, 257),
            sample_rate,
            257,
            8_192,
            &[note_on(1, 84)],
        );
        assert_finite_and_audible(&audio);
        assert_eq!(audio.channels[0], audio.channels[1]);
    }
}
