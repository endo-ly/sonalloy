use crate::compiler::CompiledLimiterProcessor;
use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;

pub(crate) struct LimiterRuntime {
    release_coeff: f32,
    current_gain: f32,
}

impl LimiterRuntime {
    pub(crate) fn new(compiled: &CompiledLimiterProcessor) -> Self {
        Self {
            release_coeff: compiled.release_coeff,
            current_gain: 1.0,
        }
    }

    pub(crate) fn process(
        &mut self,
        ceiling_db: ValueSpan,
        input_gain_db: ValueSpan,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            });
        }
        for index in 0..left.len() {
            let ceiling = ceiling_db.value_at(index, left.len());
            let input_gain = input_gain_db.value_at(index, left.len());
            let input_gain_linear = 10.0_f32.powf(input_gain / 20.0);
            let pre_left = left[index] * input_gain_linear;
            let pre_right = right[index] * input_gain_linear;
            if !ceiling.is_finite()
                || !input_gain.is_finite()
                || !pre_left.is_finite()
                || !pre_right.is_finite()
            {
                return Err(ProcessError::ProcessorFailure {
                    kind: ProcessorFailureKind::NonFinite,
                });
            }
            let ceiling_linear = 10.0_f32.powf(ceiling / 20.0);
            let peak = pre_left.abs().max(pre_right.abs());
            let target_gain = if peak <= ceiling_linear || peak <= f32::EPSILON {
                1.0
            } else {
                ceiling_linear / peak
            };
            if target_gain < self.current_gain {
                self.current_gain = target_gain;
            } else {
                self.current_gain =
                    self.release_coeff * self.current_gain + (1.0 - self.release_coeff);
            }
            left[index] = (pre_left * self.current_gain).clamp(-ceiling_linear, ceiling_linear);
            right[index] = (pre_right * self.current_gain).clamp(-ceiling_linear, ceiling_linear);
            if !left[index].is_finite() || !right[index].is_finite() {
                return Err(ProcessError::ProcessorFailure {
                    kind: ProcessorFailureKind::NonFinite,
                });
            }
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.current_gain = 1.0;
    }
}
