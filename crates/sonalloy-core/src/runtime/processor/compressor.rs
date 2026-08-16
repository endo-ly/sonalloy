use crate::compiler::CompiledCompressorProcessor;
use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;

pub(crate) struct CompressorRuntime {
    attack_coeff: f32,
    release_coeff: f32,
    knee_db: f32,
    reduction_db: f32,
}

impl CompressorRuntime {
    pub(crate) fn new(compiled: &CompiledCompressorProcessor) -> Self {
        Self {
            attack_coeff: compiled.attack_coeff,
            release_coeff: compiled.release_coeff,
            knee_db: compiled.knee_db,
            reduction_db: 0.0,
        }
    }

    pub(crate) fn process(
        &mut self,
        threshold_db: ValueSpan,
        ratio: ValueSpan,
        makeup_gain_db: ValueSpan,
        mix: ValueSpan,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            });
        }
        for index in 0..left.len() {
            let threshold = threshold_db.value_at(index, left.len());
            let ratio = ratio.value_at(index, left.len());
            let makeup = makeup_gain_db.value_at(index, left.len());
            let mix = mix.value_at(index, left.len());
            let input_left = left[index];
            let input_right = right[index];
            if !input_left.is_finite()
                || !input_right.is_finite()
                || !threshold.is_finite()
                || !ratio.is_finite()
                || !makeup.is_finite()
                || !mix.is_finite()
            {
                return Err(ProcessError::ProcessorFailure {
                    kind: ProcessorFailureKind::NonFinite,
                });
            }
            let level = input_left.abs().max(input_right.abs()).max(f32::EPSILON);
            let level_db = 20.0 * level.log10();
            let target = compression_reduction(level_db, threshold, ratio, self.knee_db);
            let coefficient = if target < self.reduction_db {
                self.attack_coeff
            } else {
                self.release_coeff
            };
            self.reduction_db = coefficient * self.reduction_db + (1.0 - coefficient) * target;
            let wet_gain = 10.0_f32.powf((self.reduction_db + makeup) / 20.0);
            let output_left = input_left * (1.0 - mix) + input_left * wet_gain * mix;
            let output_right = input_right * (1.0 - mix) + input_right * wet_gain * mix;
            if !output_left.is_finite() || !output_right.is_finite() {
                return Err(ProcessError::ProcessorFailure {
                    kind: ProcessorFailureKind::NonFinite,
                });
            }
            left[index] = output_left;
            right[index] = output_right;
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.reduction_db = 0.0;
    }
}

fn compression_reduction(level_db: f32, threshold_db: f32, ratio: f32, knee_db: f32) -> f32 {
    let slope = 1.0 - 1.0 / ratio.max(1.0);
    let over = level_db - threshold_db;
    if knee_db <= 0.0 || over > knee_db * 0.5 {
        -slope * over.max(0.0)
    } else if over < -knee_db * 0.5 {
        0.0
    } else {
        -slope * (over + knee_db * 0.5).powi(2) / (2.0 * knee_db)
    }
}
