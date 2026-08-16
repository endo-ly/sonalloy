use std::path::PathBuf;
use std::sync::Arc;

use sonalloy_core::{
    AdsrDefinition, BitcrusherProcessorDefinition, ChorusProcessorDefinition, CompileContext,
    CompressorProcessorDefinition, EqProcessorDefinition, FilterModeDefinition,
    FlangerProcessorDefinition, InstrumentDefinition, InstrumentProcessor,
    LimiterProcessorDefinition, PhaserProcessorDefinition, ProcessBlock, ProcessContext,
    ProcessEventKind, ProcessSpec, ProcessorDefinition, RenderRequest,
    ResonatorProcessorDefinition, ScheduledEvent, compile_instrument, render_instrument,
};

fn base_definition() -> InstrumentDefinition {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/basic-poly-synth.json");
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
    definition.layers[0].processors.clear();
    definition.voice_processors.clear();
    definition.global_processors.clear();
    definition.modulation = None;
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
            process_spec: ProcessSpec::new(sample_rate, block_size, 2).expect("valid spec"),
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != sonalloy_core::DiagnosticSeverity::Error),
        "processor definition must compile: {:?}",
        result.diagnostics
    );
    result.instrument.expect("processor definition compiles")
}

fn render(
    definition: &InstrumentDefinition,
    sample_rate: f64,
    block_size: usize,
    frames: u64,
) -> sonalloy_core::RenderedAudio {
    render_instrument(
        compile(definition, sample_rate, block_size),
        RenderRequest {
            sample_rate,
            block_size,
            duration_frames: frames,
            tail_frames: 0,
        },
        &[ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 110,
            },
        }],
    )
    .expect("processor render succeeds")
}

#[test]
fn filter_mode_defaults_to_low_pass_when_omitted() {
    let processor: ProcessorDefinition = serde_json::from_value(serde_json::json!({
        "type": "filter",
        "id": "tone",
        "cutoff_hz": 1_000.0,
        "resonance": 0.2,
    }))
    .expect("filter without mode parses");
    assert!(matches!(
        processor,
        ProcessorDefinition::Filter(value) if value.mode == FilterModeDefinition::LowPass
    ));
}

#[test]
fn processor_definitions_round_trip_and_reject_unknown_fields() {
    let values = [
        serde_json::json!({
            "type": "filter", "id": "filter", "mode": "notch",
            "cutoff_hz": 1_000.0, "resonance": 0.2
        }),
        serde_json::json!({
            "type": "eq", "id": "eq", "low_frequency_hz": 120.0,
            "low_gain_db": 2.0, "mid_frequency_hz": 1_000.0, "mid_gain_db": -2.0,
            "mid_q": 1.0, "high_frequency_hz": 8_000.0, "high_gain_db": 1.0
        }),
        serde_json::json!({
            "type": "resonator", "id": "resonator", "frequency_hz": 440.0,
            "decay_seconds": 0.4, "damping": 0.3, "mix": 0.2
        }),
        serde_json::json!({
            "type": "bitcrusher", "id": "bitcrusher", "bit_depth": 8.0,
            "sample_rate_ratio": 0.5, "mix": 0.2
        }),
        serde_json::json!({
            "type": "chorus", "id": "chorus", "delay_ms": 15.0, "rate_hz": 0.35,
            "depth": 0.65, "feedback": 0.1, "width": 0.8, "mix": 0.3
        }),
        serde_json::json!({
            "type": "flanger", "id": "flanger", "delay_ms": 2.0, "rate_hz": 0.25,
            "depth": 0.8, "feedback": -0.55, "width": 0.5, "mix": 0.3
        }),
        serde_json::json!({
            "type": "phaser", "id": "phaser", "stages": 6, "center_hz": 900.0,
            "sweep_octaves": 3.0, "rate_hz": 0.3, "depth": 0.8, "feedback": 0.4,
            "width": 0.7, "mix": 0.5
        }),
        serde_json::json!({
            "type": "compressor", "id": "compressor", "threshold_db": -18.0,
            "ratio": 4.0, "attack_ms": 15.0, "release_ms": 180.0, "knee_db": 6.0,
            "makeup_gain_db": 2.0, "mix": 1.0
        }),
        serde_json::json!({
            "type": "limiter", "id": "limiter", "ceiling_db": -1.0,
            "release_ms": 80.0, "input_gain_db": 0.0
        }),
    ];
    for value in values {
        let processor: ProcessorDefinition =
            serde_json::from_value(value.clone()).expect("processor definition parses");
        let serialized = serde_json::to_value(&processor).expect("processor serializes");
        let decoded: ProcessorDefinition =
            serde_json::from_value(serialized.clone()).expect("serialized processor parses");
        assert_eq!(decoded, processor);
        let mut unknown = serialized
            .as_object()
            .expect("processor serializes as an object")
            .clone();
        unknown.insert("unexpected".to_owned(), serde_json::json!(true));
        assert!(serde_json::from_value::<ProcessorDefinition>(unknown.into()).is_err());
    }
}

#[test]
fn filter_modes_are_serialized_and_produce_distinct_outputs() {
    let mut definition = base_definition();
    let mut outputs = Vec::new();
    for mode in [
        FilterModeDefinition::LowPass,
        FilterModeDefinition::HighPass,
        FilterModeDefinition::BandPass,
        FilterModeDefinition::Notch,
    ] {
        definition.voice_processors = vec![ProcessorDefinition::Filter(
            sonalloy_core::FilterProcessorDefinition {
                id: "tone".to_owned(),
                mode,
                cutoff_hz: 1_000.0,
                resonance: 0.4,
            },
        )];
        let json = serde_json::to_value(&definition).expect("definition serializes");
        assert_eq!(
            json["voice_processors"][0]["mode"],
            serde_json::to_value(mode).unwrap()
        );
        outputs.push(render(&definition, 48_000.0, 64, 2_048).channels[0].clone());
    }
    assert!(outputs.iter().flatten().all(|sample| sample.is_finite()));
    assert!(
        outputs[0]
            .iter()
            .zip(&outputs[1])
            .any(|(left, right)| (left - right).abs() > 1.0e-4)
    );
    assert!(
        outputs[2]
            .iter()
            .zip(&outputs[3])
            .any(|(left, right)| (left - right).abs() > 1.0e-4)
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn all_processor_scopes_compile_with_stable_parameter_ids() {
    let mut definition = base_definition();
    definition.layers[0].processors = vec![
        ProcessorDefinition::Eq(EqProcessorDefinition {
            id: "layer_eq".to_owned(),
            low_frequency_hz: 120.0,
            low_gain_db: 2.0,
            mid_frequency_hz: 1_000.0,
            mid_gain_db: -2.0,
            mid_q: 1.0,
            high_frequency_hz: 8_000.0,
            high_gain_db: 1.0,
        }),
        ProcessorDefinition::Resonator(ResonatorProcessorDefinition {
            id: "body".to_owned(),
            frequency_hz: 220.0,
            decay_seconds: 0.4,
            damping: 0.3,
            mix: 0.2,
        }),
        ProcessorDefinition::Bitcrusher(BitcrusherProcessorDefinition {
            id: "crush".to_owned(),
            bit_depth: 8.0,
            sample_rate_ratio: 0.5,
            mix: 0.2,
        }),
    ];
    definition.voice_processors = vec![
        ProcessorDefinition::Eq(EqProcessorDefinition {
            id: "voice_eq".to_owned(),
            low_frequency_hz: 120.0,
            low_gain_db: 0.0,
            mid_frequency_hz: 1_000.0,
            mid_gain_db: 0.0,
            mid_q: 1.0,
            high_frequency_hz: 8_000.0,
            high_gain_db: 0.0,
        }),
        ProcessorDefinition::Resonator(ResonatorProcessorDefinition {
            id: "voice_body".to_owned(),
            frequency_hz: 440.0,
            decay_seconds: 0.2,
            damping: 0.5,
            mix: 0.1,
        }),
        ProcessorDefinition::Compressor(CompressorProcessorDefinition {
            id: "glue".to_owned(),
            threshold_db: -18.0,
            ratio: 4.0,
            attack_ms: 15.0,
            release_ms: 180.0,
            knee_db: 6.0,
            makeup_gain_db: 2.0,
            mix: 1.0,
        }),
        ProcessorDefinition::Limiter(LimiterProcessorDefinition {
            id: "voice_ceiling".to_owned(),
            ceiling_db: -1.0,
            release_ms: 80.0,
            input_gain_db: 0.0,
        }),
    ];
    definition.global_processors = vec![
        ProcessorDefinition::Eq(EqProcessorDefinition {
            id: "global_eq".to_owned(),
            low_frequency_hz: 120.0,
            low_gain_db: 0.0,
            mid_frequency_hz: 1_000.0,
            mid_gain_db: 0.0,
            mid_q: 1.0,
            high_frequency_hz: 8_000.0,
            high_gain_db: 0.0,
        }),
        ProcessorDefinition::Chorus(ChorusProcessorDefinition {
            id: "chorus".to_owned(),
            delay_ms: 15.0,
            rate_hz: 0.35,
            depth: 0.65,
            feedback: 0.1,
            width: 0.8,
            mix: 0.3,
        }),
        ProcessorDefinition::Flanger(FlangerProcessorDefinition {
            id: "flanger".to_owned(),
            delay_ms: 2.0,
            rate_hz: 0.25,
            depth: 0.8,
            feedback: 0.55,
            width: 0.5,
            mix: 0.2,
        }),
        ProcessorDefinition::Phaser(PhaserProcessorDefinition {
            id: "phaser".to_owned(),
            stages: 6,
            center_hz: 900.0,
            sweep_octaves: 3.0,
            rate_hz: 0.3,
            depth: 0.8,
            feedback: 0.4,
            width: 0.7,
            mix: 0.2,
        }),
        ProcessorDefinition::Compressor(CompressorProcessorDefinition {
            id: "master_glue".to_owned(),
            threshold_db: -18.0,
            ratio: 4.0,
            attack_ms: 15.0,
            release_ms: 180.0,
            knee_db: 6.0,
            makeup_gain_db: 2.0,
            mix: 1.0,
        }),
        ProcessorDefinition::Limiter(LimiterProcessorDefinition {
            id: "ceiling".to_owned(),
            ceiling_db: -1.0,
            release_ms: 80.0,
            input_gain_db: 0.0,
        }),
    ];
    let compiled = compile(&definition, 48_000.0, 257);
    let ids: Vec<_> = compiled
        .parameters()
        .iter()
        .map(|parameter| parameter.id.as_str())
        .collect();
    for id in [
        "layer.body.processor.layer_eq.low_gain_db",
        "layer.body.processor.body.frequency_hz",
        "layer.body.processor.crush.bit_depth",
        "voice.processor.voice_eq.mid_gain_db",
        "voice.processor.voice_body.decay_seconds",
        "voice.processor.glue.threshold_db",
        "voice.processor.voice_ceiling.ceiling_db",
        "global.processor.global_eq.high_gain_db",
        "global.processor.chorus.rate_hz",
        "global.processor.flanger.feedback",
        "global.processor.phaser.mix",
        "global.processor.master_glue.makeup_gain_db",
        "global.processor.ceiling.input_gain_db",
    ] {
        assert!(ids.contains(&id), "missing parameter {id}");
    }
    for static_field in [
        "low_frequency_hz",
        "mid_frequency_hz",
        "mid_q",
        "high_frequency_hz",
        "delay_ms",
        "stages",
        "center_hz",
        "sweep_octaves",
        "attack_ms",
        "release_ms",
        "knee_db",
    ] {
        assert!(
            !ids.iter().any(|id| id.ends_with(static_field)),
            "static field registered as a parameter: {static_field}"
        );
    }
    assert_eq!(
        compiled.layers[0]
            .processors
            .iter()
            .map(|processor| processor.id.as_str())
            .collect::<Vec<_>>(),
        ["layer_eq", "body", "crush"]
    );
    assert_eq!(
        compiled
            .voice_processors
            .iter()
            .map(|processor| processor.id.as_str())
            .collect::<Vec<_>>(),
        ["voice_eq", "voice_body", "glue", "voice_ceiling"]
    );
    assert_eq!(
        compiled
            .global_processors
            .iter()
            .map(|processor| processor.id.as_str())
            .collect::<Vec<_>>(),
        [
            "global_eq",
            "chorus",
            "flanger",
            "phaser",
            "master_glue",
            "ceiling"
        ]
    );
    let audio = render(&definition, 48_000.0, 257, 2_048);
    assert!(
        audio
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
}

#[test]
fn processor_placement_matrix_rejects_unsupported_scopes() {
    let mut definition = base_definition();
    definition.layers[0].processors =
        vec![ProcessorDefinition::Chorus(ChorusProcessorDefinition {
            id: "layer_chorus".to_owned(),
            delay_ms: 15.0,
            rate_hz: 0.35,
            depth: 0.65,
            feedback: 0.1,
            width: 0.8,
            mix: 0.3,
        })];
    definition.voice_processors = vec![ProcessorDefinition::Bitcrusher(
        BitcrusherProcessorDefinition {
            id: "voice_crusher".to_owned(),
            bit_depth: 8.0,
            sample_rate_ratio: 0.5,
            mix: 0.2,
        },
    )];
    definition.global_processors = vec![ProcessorDefinition::Resonator(
        ResonatorProcessorDefinition {
            id: "global_resonator".to_owned(),
            frequency_hz: 440.0,
            decay_seconds: 0.4,
            damping: 0.3,
            mix: 0.2,
        },
    )];

    let diagnostics = definition.validate();
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.path.as_deref() == Some("layers[0].processors[0]") })
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.path.as_deref() == Some("voice_processors[0]") })
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.path.as_deref() == Some("global_processors[0]") })
    );
}

#[test]
fn modulation_effect_state_is_independent_of_block_size() {
    let mut definition = base_definition();
    definition.global_processors = vec![ProcessorDefinition::Chorus(ChorusProcessorDefinition {
        id: "wide".to_owned(),
        delay_ms: 15.0,
        rate_hz: 0.35,
        depth: 0.65,
        feedback: 0.1,
        width: 1.0,
        mix: 0.5,
    })];
    let whole = render(&definition, 48_000.0, 64, 4_096);
    let split = render(&definition, 48_000.0, 257, 4_096);
    assert!(
        whole
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert!(
        split
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert!(
        whole.channels[0]
            .iter()
            .zip(&split.channels[0])
            .all(|(left, right)| (left - right).abs() < 1.0e-5)
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn processor_expansion_reset_matches_a_fresh_runtime() {
    let mut definition = base_definition();
    definition.layers[0].processors = vec![
        ProcessorDefinition::Resonator(ResonatorProcessorDefinition {
            id: "ring".to_owned(),
            frequency_hz: 330.0,
            decay_seconds: 0.4,
            damping: 0.35,
            mix: 0.35,
        }),
        ProcessorDefinition::Bitcrusher(BitcrusherProcessorDefinition {
            id: "crush".to_owned(),
            bit_depth: 8.0,
            sample_rate_ratio: 0.5,
            mix: 0.2,
        }),
    ];
    definition.voice_processors = vec![ProcessorDefinition::Eq(EqProcessorDefinition {
        id: "voice_eq".to_owned(),
        low_frequency_hz: 120.0,
        low_gain_db: 1.0,
        mid_frequency_hz: 1_000.0,
        mid_gain_db: -2.0,
        mid_q: 1.0,
        high_frequency_hz: 8_000.0,
        high_gain_db: 2.0,
    })];
    definition.global_processors = vec![
        ProcessorDefinition::Chorus(ChorusProcessorDefinition {
            id: "chorus".to_owned(),
            delay_ms: 15.0,
            rate_hz: 0.35,
            depth: 0.65,
            feedback: 0.1,
            width: 0.8,
            mix: 0.3,
        }),
        ProcessorDefinition::Flanger(FlangerProcessorDefinition {
            id: "flanger".to_owned(),
            delay_ms: 2.0,
            rate_hz: 0.25,
            depth: 0.8,
            feedback: 0.55,
            width: 0.5,
            mix: 0.2,
        }),
        ProcessorDefinition::Phaser(PhaserProcessorDefinition {
            id: "phaser".to_owned(),
            stages: 6,
            center_hz: 900.0,
            sweep_octaves: 3.0,
            rate_hz: 0.3,
            depth: 0.8,
            feedback: 0.4,
            width: 0.7,
            mix: 0.2,
        }),
        ProcessorDefinition::Compressor(CompressorProcessorDefinition {
            id: "compressor".to_owned(),
            threshold_db: -18.0,
            ratio: 4.0,
            attack_ms: 15.0,
            release_ms: 180.0,
            knee_db: 6.0,
            makeup_gain_db: 2.0,
            mix: 1.0,
        }),
        ProcessorDefinition::Limiter(LimiterProcessorDefinition {
            id: "limiter".to_owned(),
            ceiling_db: -1.0,
            release_ms: 80.0,
            input_gain_db: 0.0,
        }),
    ];
    let compiled = compile(&definition, 48_000.0, 257);
    let spec = ProcessSpec::new(48_000.0, 257, 2).expect("valid spec");
    let event = [sonalloy_core::ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 110,
        },
    }];
    let mut runtime = compiled.instantiate();
    runtime.prepare(spec).expect("runtime prepares");
    let mut first_left = vec![0.0; 257];
    let mut first_right = vec![0.0; 257];
    let mut first_output: [&mut [f32]; 2] = [&mut first_left, &mut first_right];
    runtime
        .process(ProcessBlock {
            frames: 257,
            context: ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
            },
            events: &event,
            output: &mut first_output,
        })
        .expect("first process");
    runtime.reset().expect("runtime resets");
    let mut reset_left = vec![0.0; 257];
    let mut reset_right = vec![0.0; 257];
    let mut reset_output: [&mut [f32]; 2] = [&mut reset_left, &mut reset_right];
    runtime
        .process(ProcessBlock {
            frames: 257,
            context: ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
            },
            events: &event,
            output: &mut reset_output,
        })
        .expect("reset process");
    assert_eq!(first_left, reset_left);
    assert_eq!(first_right, reset_right);
}

#[test]
fn limiter_never_exceeds_its_ceiling() {
    let mut definition = base_definition();
    definition.global_processors = vec![ProcessorDefinition::Limiter(LimiterProcessorDefinition {
        id: "ceiling".to_owned(),
        ceiling_db: -6.0,
        release_ms: 80.0,
        input_gain_db: 24.0,
    })];
    let audio = render(&definition, 48_000.0, 32, 4_096);
    let peak = audio
        .channels
        .iter()
        .flatten()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max);
    assert!(peak <= 10.0_f32.powf(-6.0 / 20.0) + 1.0e-5, "peak={peak}");
}
