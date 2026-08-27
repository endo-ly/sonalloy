use std::sync::Arc;

use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;

pub(crate) const HILBERT_TAPS: usize = 255;
pub(crate) const HILBERT_GROUP_DELAY: usize = (HILBERT_TAPS - 1) / 2;

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
        sample_rate: f32,
        effective_abs_shift_hz: f32,
    ) -> Result<Self, ProcessError> {
        if coefficients.len() != HILBERT_TAPS
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
        Ok(Self {
            coefficients,
            sample_rate,
            effective_abs_shift_hz,
            left: FrequencyShifterChannel::new(),
            right: FrequencyShifterChannel::new(),
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
            let shifted_left = if shift.abs() < f32::EPSILON {
                delayed_left
            } else {
                delayed_left * cosine - quadrature_left * sine
            };
            let shifted_right = if shift.abs() < f32::EPSILON {
                delayed_right
            } else {
                delayed_right * cosine - quadrature_right * sine
            };
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
    fn new() -> Self {
        Self {
            input: vec![0.0; HILBERT_TAPS],
            input_position: 0,
            dry: vec![0.0; HILBERT_GROUP_DELAY.max(1)],
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
        let mut output = 0.0;
        for (index, coefficient) in coefficients.iter().enumerate() {
            let position = (self.input_position + HILBERT_TAPS - index) % HILBERT_TAPS;
            output += *coefficient * self.input[position];
        }
        self.input_position = (self.input_position + 1) % HILBERT_TAPS;
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

    use super::*;

    fn span(value: f32) -> ValueSpan {
        ValueSpan {
            start: value,
            end: value,
        }
    }

    #[test]
    fn zero_shift_keeps_dry_and_wet_time_aligned() {
        let coefficients = Arc::from(vec![0.0; HILBERT_TAPS]);
        let mut runtime = FrequencyShifterRuntime::new(coefficients, 48_000.0, 5_000.0)
            .expect("frequency shifter prepares");
        let mut left = [0.0; 256];
        let mut right = [0.0; 256];
        left[0] = 1.0;
        right[0] = -1.0;
        runtime
            .process(span(0.0), span(1.0), &mut left, &mut right)
            .expect("frequency shifter processes");

        assert!(
            left[..HILBERT_GROUP_DELAY]
                .iter()
                .all(|sample| sample.abs() < 1.0e-6)
        );
        assert!((left[HILBERT_GROUP_DELAY] - 1.0).abs() < 1.0e-6);
        assert!((right[HILBERT_GROUP_DELAY] + 1.0).abs() < 1.0e-6);
    }
}
