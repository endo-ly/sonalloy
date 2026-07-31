use std::path::PathBuf;
use std::sync::Arc;

use approx::assert_relative_eq;
use sonalloy_core::{
    CompileContext, InstrumentDefinition, InstrumentProcessor, ProcessBlock, ProcessContext,
    ProcessEventKind, ProcessSpec, RenderRequest, ScheduledEvent, SineRuntime, compile_instrument,
    render_instrument,
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
