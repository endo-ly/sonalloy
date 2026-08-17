use crate::compiler::CompiledChorusProcessor;
use crate::process::{ProcessError, ProcessorFailureKind};

use super::super::fractional_delay::FractionalDelayLine;
use super::ValueSpan;

pub(crate) struct ChorusRuntime {
    sample_rate: f32,
    delay_frames: f32,
    left: FractionalDelayLine,
    right: FractionalDelayLine,
    phase: f32,
}

impl ChorusRuntime {
    pub(crate) fn new(compiled: &CompiledChorusProcessor) -> Self {
        Self {
            sample_rate: compiled.sample_rate,
            delay_frames: compiled.delay_frames,
            left: FractionalDelayLine::new(compiled.max_delay_frames),
            right: FractionalDelayLine::new(compiled.max_delay_frames),
            phase: 0.0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn process(
        &mut self,
        rate_hz: ValueSpan,
        depth: ValueSpan,
        feedback: ValueSpan,
        width: ValueSpan,
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
            let rate = rate_hz.value_at(index, left.len());
            let depth = depth.value_at(index, left.len());
            let feedback = feedback.value_at(index, left.len());
            let width = width.value_at(index, left.len());
            let mix = mix.value_at(index, left.len());
            let phase = self.phase;
            let left_lfo = (std::f32::consts::TAU * phase).sin();
            let right_lfo = (std::f32::consts::TAU * (phase + 0.5 * width).rem_euclid(1.0)).sin();
            let left_delay = self.delay_frames * (1.0 + 0.9 * depth * left_lfo);
            let right_delay = self.delay_frames * (1.0 + 0.9 * depth * right_lfo);
            let input_left = left[index];
            let input_right = right[index];
            let delayed_left = self.left.read(left_delay)?;
            let delayed_right = self.right.read(right_delay)?;
            self.left.write(input_left + delayed_left * feedback)?;
            self.right.write(input_right + delayed_right * feedback)?;
            left[index] = input_left * (1.0 - mix) + delayed_left * mix;
            right[index] = input_right * (1.0 - mix) + delayed_right * mix;
            if !left[index].is_finite() || !right[index].is_finite() {
                return Err(ProcessError::ProcessorFailure {
                    kind: ProcessorFailureKind::NonFinite,
                });
            }
            self.phase = (self.phase + rate / self.sample_rate).rem_euclid(1.0);
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
        self.phase = 0.0;
    }
}
