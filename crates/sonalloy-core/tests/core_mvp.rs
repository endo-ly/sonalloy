use std::path::PathBuf;
use std::sync::Arc;

use approx::assert_relative_eq;
use sonalloy_core::{
    CompileContext, InstrumentDefinition, ProcessEventKind, ProcessSpec, RenderRequest,
    ScheduledEvent, compile_instrument, render_instrument,
};

fn definition() -> InstrumentDefinition {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/basic-poly-synth.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("reference Definition exists"))
        .expect("reference Definition parses")
}

fn render(block_size: usize) -> sonalloy_core::RenderedAudio {
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
    let audio = render(257);
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
    let reference = render(64);
    for candidate in [render(257), render(1024)] {
        for (left, right) in reference.channels[0].iter().zip(&candidate.channels[0]) {
            assert_relative_eq!(*left, *right, epsilon = 1.0e-5);
        }
        for (left, right) in reference.channels[1].iter().zip(&candidate.channels[1]) {
            assert_relative_eq!(*left, *right, epsilon = 1.0e-5);
        }
    }
}
