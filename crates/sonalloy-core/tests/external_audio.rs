use std::sync::Arc;

use sonalloy_core::{
    CompileContext, CompiledInstrument, EnvelopeTransferProcessorDefinition, ExternalAudioChannels,
    ExternalAudioInputDefinition, InstrumentDefinition, InstrumentProcessor, PreparedAudio,
    PreparedAudioChannels, ProcessEventKind, ProcessSpec, ProcessorDefinition, RenderError,
    RenderRequest, SampleMetadata, ScheduledEvent, compile_instrument,
    render_instrument_with_input, render_instrument_with_input_and_reset,
};

fn definition() -> InstrumentDefinition {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/basic-poly-synth.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("reference Definition exists"))
        .expect("reference Definition parses")
}

fn external_definition(processor: ProcessorDefinition) -> InstrumentDefinition {
    let mut definition = definition();
    definition.external_audio = Some(ExternalAudioInputDefinition {
        channels: ExternalAudioChannels::Stereo,
    });
    definition.global_processors = vec![processor];
    definition
}

fn compiled(definition: &InstrumentDefinition) -> Arc<CompiledInstrument> {
    compiled_at(definition, 48_000.0, 257)
}

fn compiled_at(
    definition: &InstrumentDefinition,
    sample_rate: f64,
    block_size: usize,
) -> Arc<CompiledInstrument> {
    let result = compile_instrument(
        definition,
        &CompileContext {
            definition_base_dir: ".".into(),
            process_spec: ProcessSpec::new(sample_rate, block_size, 2, 2)
                .expect("valid process spec"),
        },
    );
    result.instrument.expect("external definition compiles")
}

fn audio(frames: usize, left: f32, right: f32) -> PreparedAudio {
    audio_at(48_000.0, frames, left, right)
}

fn audio_at(sample_rate: f64, frames: usize, left: f32, right: f32) -> PreparedAudio {
    PreparedAudio {
        sample_rate,
        frames,
        source_metadata: SampleMetadata {
            source_sample_rate: match sample_rate {
                44_100.0 => 44_100,
                48_000.0 => 48_000,
                96_000.0 => 96_000,
                _ => panic!("test sample rate has no integer metadata mapping"),
            },
            source_channels: 2,
            bits_per_sample: Some(32),
            source_frames: frames,
        },
        channels: PreparedAudioChannels::Stereo {
            left: vec![left; frames].into(),
            right: vec![right; frames].into(),
        },
    }
}

fn request(frames: u64) -> RenderRequest {
    request_at(48_000.0, 257, frames)
}

fn request_at(sample_rate: f64, block_size: usize, frames: u64) -> RenderRequest {
    RenderRequest {
        sample_rate,
        block_size,
        duration_frames: frames,
        tail_frames: 0,
    }
}

fn events() -> [ScheduledEvent; 2] {
    [
        ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        },
        ScheduledEvent {
            absolute_frame: 128,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        },
    ]
}

#[test]
fn external_input_contract_is_strict_at_compile_and_render_boundaries() {
    let processor = ProcessorDefinition::EnvelopeTransfer(EnvelopeTransferProcessorDefinition {
        id: "transfer".to_owned(),
        attack_ms: 2.0,
        release_ms: 120.0,
        input_gain_db: 0.0,
        floor_db: -72.0,
        mix: 1.0,
    });
    let definition = external_definition(processor);
    let mismatch = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: ".".into(),
            process_spec: ProcessSpec::new(48_000.0, 257, 0, 2).expect("valid process spec"),
        },
    );
    assert!(mismatch.instrument.is_none());

    let instrument = compiled(&definition);
    assert_eq!(instrument.required_input_channels(), 2);
    let mut runtime = instrument.instantiate();
    assert_eq!(
        runtime.prepare(ProcessSpec::new(48_000.0, 257, 0, 2).expect("valid process spec")),
        Err(
            sonalloy_core::ProcessError::InputChannelRequirementMismatch {
                compiled: 2,
                requested: 0,
            }
        )
    );
    let time_map = sonalloy_core::MusicalTimeMap::constant(120.0).expect("tempo map");
    assert_eq!(
        render_instrument_with_input(
            Arc::clone(&instrument),
            request(256),
            &events(),
            &time_map,
            None,
        ),
        Err(RenderError::ExternalInputMissing)
    );
    let mut wrong_rate = audio(256, 1.0, 1.0);
    wrong_rate.sample_rate = 44_100.0;
    assert_eq!(
        render_instrument_with_input(
            Arc::clone(&instrument),
            request(256),
            &events(),
            &time_map,
            Some(&wrong_rate),
        ),
        Err(RenderError::ExternalInputSampleRateMismatch)
    );
    let rendered = render_instrument_with_input(
        instrument,
        request(256),
        &events(),
        &time_map,
        Some(&audio(1, 1.0, -1.0)),
    )
    .expect("short external input is padded by the offline adapter");
    assert_eq!(rendered.frames(), 256);
    assert!(
        rendered
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
}

#[test]
fn external_cross_synthesis_processors_are_finite_and_resettable() {
    let processors = [
        ProcessorDefinition::Vocoder(sonalloy_core::VocoderProcessorDefinition {
            id: "vocoder".to_owned(),
            attack_ms: 8.0,
            release_ms: 80.0,
            modulator_gain_db: 0.0,
            output_gain_db: 0.0,
            mix: 1.0,
        }),
        ProcessorDefinition::SpectralMorph(sonalloy_core::SpectralMorphProcessorDefinition {
            id: "morph".to_owned(),
            morph: 0.75,
            output_gain_db: 0.0,
        }),
    ];
    let time_map = sonalloy_core::MusicalTimeMap::constant(120.0).expect("tempo map");
    for processor in processors {
        let instrument = compiled(&external_definition(processor));
        let external = audio(1_500, 0.5, -0.25);
        let (first, second) = render_instrument_with_input_and_reset(
            instrument,
            request(1_500),
            &events(),
            &time_map,
            Some(&external),
        )
        .expect("cross synthesis render succeeds");
        assert!(
            first
                .channels
                .iter()
                .flatten()
                .all(|sample| sample.is_finite())
        );
        assert_eq!(first, second);
    }
}

#[test]
fn supplied_external_audio_is_rejected_when_the_definition_does_not_use_it() {
    let definition = definition();
    let instrument = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: ".".into(),
            process_spec: ProcessSpec::new(48_000.0, 257, 0, 2).expect("valid process spec"),
        },
    )
    .instrument
    .expect("reference definition compiles");
    let time_map = sonalloy_core::MusicalTimeMap::constant(120.0).expect("tempo map");
    assert_eq!(
        render_instrument_with_input(
            instrument,
            request(1),
            &[],
            &time_map,
            Some(&audio(1, 0.0, 0.0)),
        ),
        Err(RenderError::ExternalInputUnused)
    );
}

#[test]
fn external_render_is_finite_across_sample_rates_and_block_sizes() {
    let definition = external_definition(ProcessorDefinition::EnvelopeTransfer(
        EnvelopeTransferProcessorDefinition {
            id: "transfer".to_owned(),
            attack_ms: 2.0,
            release_ms: 120.0,
            input_gain_db: 0.0,
            floor_db: -72.0,
            mix: 1.0,
        },
    ));
    let events = events();
    let time_map = sonalloy_core::MusicalTimeMap::constant(120.0).expect("tempo map");
    for (sample_rate, frames) in [(44_100.0, 4_410), (48_000.0, 4_800), (96_000.0, 9_600)] {
        for block_size in [32, 64, 257, 1_024] {
            let instrument = compiled_at(&definition, sample_rate, block_size);
            let rendered = render_instrument_with_input(
                instrument,
                request_at(sample_rate, block_size, frames as u64),
                &events,
                &time_map,
                Some(&audio_at(sample_rate, frames, 0.5, -0.25)),
            )
            .expect("external render succeeds across the matrix");
            assert!(
                rendered
                    .channels
                    .iter()
                    .flatten()
                    .all(|sample| sample.is_finite()),
                "non-finite output at {sample_rate} Hz and block size {block_size}"
            );
        }
    }
}
