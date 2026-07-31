use sonalloy_dsp_sys::{
    DspError, DspOscillator, DspOscillatorWaveform, backend_version, capabilities,
};

fn render_blocks(block_size: usize, waveform: DspOscillatorWaveform) -> Vec<f32> {
    let mut oscillator = DspOscillator::new().expect("oscillator allocation");
    oscillator
        .prepare(48_000.0, waveform)
        .expect("oscillator preparation");

    let mut output = Vec::with_capacity(48_000);
    let mut block = vec![0.0_f32; block_size];
    while output.len() < 48_000 {
        let frames = (48_000 - output.len()).min(block_size);
        oscillator
            .process(440.0, &mut block[..frames])
            .expect("oscillator process");
        output.extend_from_slice(&block[..frames]);
    }
    output
}

fn positive_zero_crossings(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|window| window[0] <= 0.0 && window[1] > 0.0)
        .count()
}

#[test]
fn lifecycle_and_reset_are_safe() {
    let mut oscillator = DspOscillator::new().expect("oscillator allocation");
    let mut output = [1.0_f32; 64];

    assert_eq!(
        oscillator.process(440.0, &mut output),
        Err(DspError::NotPrepared)
    );
    assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));
    for sample_rate in [0.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            oscillator.prepare(sample_rate, DspOscillatorWaveform::Sine),
            Err(DspError::InvalidArgument)
        );
    }
    oscillator
        .prepare(48_000.0, DspOscillatorWaveform::Sine)
        .expect("oscillator preparation");
    oscillator
        .process(440.0, &mut output)
        .expect("oscillator process");
    assert!(output.iter().all(|sample| sample.is_finite()));
    assert!(output.iter().any(|sample| sample.abs() > 0.5));
    assert!(output.iter().all(|sample| sample.abs() <= 1.1));

    oscillator.reset().expect("oscillator reset");
    let mut after_reset = [0.0_f32; 64];
    oscillator
        .process(440.0, &mut after_reset)
        .expect("oscillator process after reset");
    for (before, after) in output.iter().zip(after_reset.iter()) {
        assert!((before - after).abs() < 1.0e-7);
    }
}

#[test]
fn block_sizes_produce_the_same_signal() {
    let reference = render_blocks(64, DspOscillatorWaveform::Sine);
    for block_size in [257, 1024] {
        let candidate = render_blocks(block_size, DspOscillatorWaveform::Sine);
        assert_eq!(candidate.len(), reference.len());
        for (left, right) in reference.iter().zip(candidate.iter()) {
            assert!((left - right).abs() < 1.0e-6);
        }
    }
}

#[test]
fn signal_frequency_and_saw_generation_are_verified() {
    let sine = render_blocks(257, DspOscillatorWaveform::Sine);
    let crossings = positive_zero_crossings(&sine);
    let crossing_count = u32::try_from(crossings).expect("crossing count fits in u32");
    let frame_count = u32::try_from(sine.len()).expect("frame count fits in u32");
    let estimated_frequency = f64::from(crossing_count) * 48_000.0 / f64::from(frame_count);
    assert!((estimated_frequency - 440.0).abs() < 1.0);

    let saw = render_blocks(257, DspOscillatorWaveform::Saw);
    assert!(saw.iter().all(|sample| sample.is_finite()));
    assert!(saw.iter().any(|sample| sample.abs() > 0.5));
    assert!(saw.iter().all(|sample| sample.abs() <= 1.1));
}

#[test]
fn empty_buffer_and_native_guard_are_safe() {
    let mut oscillator = DspOscillator::new().expect("oscillator allocation");
    oscillator
        .prepare(48_000.0, DspOscillatorWaveform::Sine)
        .expect("oscillator preparation");
    let mut empty = [];
    oscillator
        .process(440.0, &mut empty)
        .expect("empty buffer process");

    let mut buffer = [0.0_f32; 66];
    buffer[0] = 122.0;
    buffer[65] = 123.0;
    oscillator
        .process(440.0, &mut buffer[1..65])
        .expect("guarded buffer process");
    assert!((buffer[0] - 122.0).abs() < f32::EPSILON);
    assert!((buffer[65] - 123.0).abs() < f32::EPSILON);
}

#[test]
fn backend_reports_capabilities() {
    assert!(backend_version().contains("DaisySP V1.0.0"));
    assert_eq!(
        capabilities(),
        sonalloy_dsp_sys::DspCapabilities {
            sine: true,
            saw: true,
        }
    );
}
