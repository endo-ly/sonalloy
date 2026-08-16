use crate::compiler::GeneratorOutputMode;
use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;

pub(crate) struct BitcrusherRuntime {
    phase: f32,
    held_left: f32,
    held_right: Option<f32>,
}

impl BitcrusherRuntime {
    pub(crate) fn new(output_mode: GeneratorOutputMode) -> Self {
        Self {
            phase: 0.0,
            held_left: 0.0,
            held_right: match output_mode {
                GeneratorOutputMode::Mono => None,
                GeneratorOutputMode::Stereo => Some(0.0),
            },
        }
    }

    pub(crate) fn process_mono(
        &mut self,
        bit_depth: ValueSpan,
        sample_rate_ratio: ValueSpan,
        mix: ValueSpan,
        buffer: &mut [f32],
    ) -> Result<(), ProcessError> {
        for index in 0..buffer.len() {
            let bit_depth = bit_depth.value_at(index, buffer.len());
            let ratio = sample_rate_ratio.value_at(index, buffer.len());
            let mix = mix.value_at(index, buffer.len());
            let input = buffer[index];
            self.advance_phase(ratio, input)?;
            let wet = self.quantize_and_mix(input, bit_depth, mix, false)?;
            buffer[index] = wet;
        }
        Ok(())
    }

    pub(crate) fn process_stereo(
        &mut self,
        bit_depth: ValueSpan,
        sample_rate_ratio: ValueSpan,
        mix: ValueSpan,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            });
        }
        if self.held_right.is_none() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            });
        }
        for index in 0..left.len() {
            let bit_depth = bit_depth.value_at(index, left.len());
            let ratio = sample_rate_ratio.value_at(index, left.len());
            let mix = mix.value_at(index, left.len());
            let left_input = left[index];
            let right_input = right[index];
            let updated = self.advance_phase(ratio, left_input)?;
            if updated {
                if let Some(held_right) = &mut self.held_right {
                    *held_right = right_input;
                }
            }
            let left_wet = self.quantize_and_mix(left_input, bit_depth, mix, false)?;
            let right_wet = self.quantize_and_mix(right_input, bit_depth, mix, true)?;
            left[index] = left_wet;
            right[index] = right_wet;
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.phase = 0.0;
        self.held_left = 0.0;
        if let Some(held_right) = &mut self.held_right {
            *held_right = 0.0;
        }
    }

    fn advance_phase(&mut self, ratio: f32, input: f32) -> Result<bool, ProcessError> {
        if !ratio.is_finite() || !input.is_finite() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            });
        }
        self.phase += ratio;
        let mut updated = false;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
            updated = true;
            self.held_left = input;
        }
        Ok(updated)
    }

    fn quantize_and_mix(
        &mut self,
        input: f32,
        bit_depth: f32,
        mix: f32,
        right_channel: bool,
    ) -> Result<f32, ProcessError> {
        if !input.is_finite() || !bit_depth.is_finite() || !mix.is_finite() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            });
        }
        let held = if right_channel {
            self.held_right.ok_or(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            })?
        } else {
            self.held_left
        };
        let levels = 2.0_f32.powf(bit_depth);
        let denominator = levels * 0.5 - 1.0;
        let quantized = (held.clamp(-1.0, 1.0) * denominator).round() / denominator;
        let output = input * (1.0 - mix) + quantized * mix;
        if output.is_finite() {
            Ok(output)
        } else {
            Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            })
        }
    }
}
