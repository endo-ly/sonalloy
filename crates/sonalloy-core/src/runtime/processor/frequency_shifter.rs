use std::sync::Arc;

use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;

pub(crate) struct FrequencyShifterRuntime {
    coefficients: Arc<[f32]>,
    sample_rate: f32,
    effective_abs_shift_hz: f32,
    left: FrequencyShifterChannel,
    right: FrequencyShifterChannel,
    phase: f32,
}

struct FrequencyShifterChannel {
    input: Vec<f32>,
    input_position: usize,
    dry: Vec<f32>,
    dry_position: usize,
}

impl FrequencyShifterRuntime {
    pub(crate) fn new(
        coefficients: Arc<[f32]>,
        latency_frames: usize,
        sample_rate: f32,
        effective_abs_shift_hz: f32,
    ) -> Result<Self, ProcessError> {
        if coefficients.is_empty()
            || coefficients
                .iter()
                .any(|coefficient| !coefficient.is_finite())
            || !sample_rate.is_finite()
            || sample_rate <= 0.0
            || !effective_abs_shift_hz.is_finite()
            || effective_abs_shift_hz <= 0.0
        {
            return Err(invalid_state());
        }
        let tap_count = coefficients.len();
        Ok(Self {
            coefficients,
            sample_rate,
            effective_abs_shift_hz,
            left: FrequencyShifterChannel::new(tap_count, latency_frames),
            right: FrequencyShifterChannel::new(tap_count, latency_frames),
            phase: 0.0,
        })
    }

    pub(crate) fn process(
        &mut self,
        shift_hz: ValueSpan,
        mix: ValueSpan,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(invalid_state());
        }
        for index in 0..left.len() {
            let shift = shift_hz
                .value_at(index, left.len())
                .clamp(-self.effective_abs_shift_hz, self.effective_abs_shift_hz);
            let mix = mix.value_at(index, left.len());
            if !shift.is_finite() || !mix.is_finite() {
                return Err(non_finite());
            }
            let input_left = left[index];
            let input_right = right[index];
            let delayed_left = self.left.delayed(input_left)?;
            let delayed_right = self.right.delayed(input_right)?;
            let quadrature_left = self.left.hilbert(&self.coefficients);
            let quadrature_right = self.right.hilbert(&self.coefficients);
            let (sine, cosine) = (std::f32::consts::TAU * self.phase).sin_cos();
            let shifted_left = delayed_left * cosine - quadrature_left * sine;
            let shifted_right = delayed_right * cosine - quadrature_right * sine;
            left[index] = delayed_left * (1.0 - mix) + shifted_left * mix;
            right[index] = delayed_right * (1.0 - mix) + shifted_right * mix;
            if !left[index].is_finite() || !right[index].is_finite() {
                return Err(non_finite());
            }
            self.phase = (self.phase + shift / self.sample_rate).rem_euclid(1.0);
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
        self.phase = 0.0;
    }
}

impl FrequencyShifterChannel {
    fn new(tap_count: usize, latency_frames: usize) -> Self {
        Self {
            input: vec![0.0; tap_count],
            input_position: 0,
            dry: vec![0.0; latency_frames.max(1)],
            dry_position: 0,
        }
    }

    fn delayed(&mut self, input: f32) -> Result<f32, ProcessError> {
        if !input.is_finite() {
            return Err(non_finite());
        }
        let delayed = self.dry[self.dry_position];
        self.dry[self.dry_position] = input;
        self.dry_position = (self.dry_position + 1) % self.dry.len();
        self.input[self.input_position] = input;
        if delayed.is_finite() {
            Ok(delayed)
        } else {
            Err(non_finite())
        }
    }

    fn hilbert(&mut self, coefficients: &[f32]) -> f32 {
        let tap_count = coefficients.len();
        let mut output = 0.0;
        for (index, coefficient) in coefficients.iter().enumerate() {
            let position = (self.input_position + tap_count - index) % tap_count;
            output += *coefficient * self.input[position];
        }
        self.input_position = (self.input_position + 1) % tap_count;
        output
    }

    fn reset(&mut self) {
        self.input.fill(0.0);
        self.dry.fill(0.0);
        self.input_position = 0;
        self.dry_position = 0;
    }
}

fn invalid_state() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::InvalidState,
    }
}

fn non_finite() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::NonFinite,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::FrequencyShifterRuntime;
    use crate::runtime::modulation::ValueSpan;

    const TEST_HILBERT_TAPS: usize = 255;
    const TEST_HILBERT_GROUP_DELAY: usize = (TEST_HILBERT_TAPS - 1) / 2;

    #[allow(clippy::cast_possible_wrap, clippy::cast_precision_loss)]
    fn coefficients() -> Vec<f32> {
        let center = (TEST_HILBERT_TAPS - 1) / 2;
        (0..TEST_HILBERT_TAPS)
            .map(|index| {
                let offset = index as isize - center as isize;
                let ideal = if offset != 0 && offset % 2 != 0 {
                    2.0 / (std::f32::consts::PI * offset as f32)
                } else {
                    0.0
                };
                let phase = std::f32::consts::TAU * index as f32 / (TEST_HILBERT_TAPS - 1) as f32;
                ideal * (0.42 - 0.5 * phase.cos() + 0.08 * (phase * 2.0).cos())
            })
            .collect()
    }

    fn span(value: f32) -> ValueSpan {
        ValueSpan {
            start: value,
            end: value,
        }
    }

    #[test]
    fn zero_shift_keeps_dry_and_wet_time_aligned() {
        let coefficients = Arc::from(coefficients());
        let mut runtime =
            FrequencyShifterRuntime::new(coefficients, TEST_HILBERT_GROUP_DELAY, 48_000.0, 5_000.0)
                .expect("frequency shifter prepares");
        let mut left = [0.0; 256];
        let mut right = [0.0; 256];
        left[0] = 1.0;
        right[0] = -1.0;
        runtime
            .process(span(0.0), span(1.0), &mut left, &mut right)
            .expect("frequency shifter processes");

        assert!(
            left[..TEST_HILBERT_GROUP_DELAY]
                .iter()
                .all(|sample| sample.abs() < 1.0e-6)
        );
        assert!((left[TEST_HILBERT_GROUP_DELAY] - 1.0).abs() < 1.0e-6);
        assert!((right[TEST_HILBERT_GROUP_DELAY] + 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn signed_shift_selects_the_expected_sideband() {
        let positive = render_sine(400.0);
        let negative = render_sine(-400.0);
        let positive_desired = energy_at_frequency(&positive, 1_400.0);
        let positive_image = energy_at_frequency(&positive, 600.0);
        let negative_desired = energy_at_frequency(&negative, 600.0);
        let negative_image = energy_at_frequency(&negative, 1_400.0);

        assert!(
            positive_desired > positive_image * 31.6,
            "+400 Hz desired={positive_desired}, image={positive_image}"
        );
        assert!(
            negative_desired > negative_image * 31.6,
            "-400 Hz desired={negative_desired}, image={negative_image}"
        );
    }

    #[test]
    fn dynamic_shift_remains_finite_without_large_steps() {
        let coefficients = Arc::from(coefficients());
        let mut runtime =
            FrequencyShifterRuntime::new(coefficients, TEST_HILBERT_GROUP_DELAY, 48_000.0, 5_000.0)
                .expect("frequency shifter prepares");
        let mut left = vec![0.0; 16_384];
        let mut right = vec![0.0; 16_384];
        for (index, sample) in left.iter_mut().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let phase = std::f32::consts::TAU * 1_000.0 * index as f32 / 48_000.0;
            *sample = phase.sin();
        }
        right.copy_from_slice(&left);

        runtime
            .process(
                ValueSpan {
                    start: -500.0,
                    end: 500.0,
                },
                span(1.0),
                &mut left,
                &mut right,
            )
            .expect("dynamic frequency shift processes");

        let (maximum_index, maximum_step) = left
            .get(TEST_HILBERT_GROUP_DELAY + 2_048..)
            .expect("dynamic shift has a steady-state region")
            .windows(2)
            .enumerate()
            .map(|(index, window)| (index, (window[1] - window[0]).abs()))
            .max_by(|(_, left), (_, right)| left.total_cmp(right))
            .expect("dynamic shift has adjacent samples");
        assert!(left.iter().all(|sample| sample.is_finite()));
        assert!(
            maximum_step < 0.5,
            "maximum sample step={maximum_step} at index {}",
            maximum_index + TEST_HILBERT_GROUP_DELAY + 2_048
        );
    }

    fn render_sine(shift_hz: f32) -> Vec<f32> {
        let coefficients = Arc::from(coefficients());
        let mut runtime =
            FrequencyShifterRuntime::new(coefficients, TEST_HILBERT_GROUP_DELAY, 48_000.0, 5_000.0)
                .expect("frequency shifter prepares");
        let mut left = vec![0.0; 32_768];
        let mut right = vec![0.0; 32_768];
        for (index, sample) in left.iter_mut().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let phase = std::f32::consts::TAU * 1_000.0 * index as f32 / 48_000.0;
            *sample = phase.sin();
        }
        right.copy_from_slice(&left);
        runtime
            .process(span(shift_hz), span(1.0), &mut left, &mut right)
            .expect("sine frequency shift processes");
        left
    }

    fn energy_at_frequency(samples: &[f32], frequency_hz: f32) -> f32 {
        let start = 8_192;
        let slice = &samples[start..];
        let omega = std::f32::consts::TAU * frequency_hz / 48_000.0;
        let (sin_omega, cos_omega) = omega.sin_cos();
        let mut sine = 0.0;
        let mut cosine = 1.0;
        let mut real = 0.0;
        let mut imaginary = 0.0;
        for sample in slice {
            real += *sample * cosine;
            imaginary -= *sample * sine;
            let next_cosine = cosine * cos_omega - sine * sin_omega;
            sine = sine * cos_omega + cosine * sin_omega;
            cosine = next_cosine;
        }
        #[allow(clippy::cast_precision_loss)]
        let length = slice.len() as f32;
        (real * real + imaginary * imaginary).sqrt() / length
    }
}
