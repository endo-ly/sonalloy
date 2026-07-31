use approx::assert_relative_eq;
use sonalloy_core::{InstrumentProcessor, ProcessBlock, ProcessContext, ProcessSpec, SineRuntime};

fn render_blocks(block_size: usize) -> Vec<Vec<f32>> {
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
fn public_process_path_is_stable_across_block_sizes() {
    let reference = render_blocks(64);
    for block_size in [257, 1024] {
        let candidate = render_blocks(block_size);
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
fn public_process_path_restarts_after_reset() {
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
