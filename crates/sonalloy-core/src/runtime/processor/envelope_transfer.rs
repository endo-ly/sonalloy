use crate::compiler::CompiledEnvelopeTransferProcessor;
use crate::process::{ProcessError, ProcessorFailureKind};
use crate::runtime::external_audio::{ExternalAudioBlock, ExternalInputDelay};

use super::ValueSpan;

pub(crate) struct EnvelopeTransferRuntime {
    attack_coeff: f32,
    release_coeff: f32,
    envelope: f32,
    external_input: ExternalInputDelay,
}

impl EnvelopeTransferRuntime {
    pub(crate) fn new(compiled: &CompiledEnvelopeTransferProcessor) -> Self {
        Self {
            attack_coeff: compiled.attack_coeff,
            release_coeff: compiled.release_coeff,
            envelope: 0.0,
            external_input: ExternalInputDelay::new(compiled.external_input_alignment_frames),
        }
    }

    pub(crate) fn process(
        &mut self,
        input_gain_db: ValueSpan,
        floor_db: ValueSpan,
        mix: ValueSpan,
        external: ExternalAudioBlock<'_>,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(invalid_state());
        }
        for index in 0..left.len() {
            let input_gain = db_to_linear(input_gain_db.value_at(index, left.len()));
            let floor = db_to_linear(floor_db.value_at(index, left.len()));
            let mix_value = mix.value_at(index, left.len());
            let (external_left, external_right) = self.external_input.next(external, index);
            let target = (external_left.abs().max(external_right.abs()) * input_gain).min(1.0);
            let coefficient = if target > self.envelope {
                self.attack_coeff
            } else {
                self.release_coeff
            };
            self.envelope = target + coefficient * (self.envelope - target);
            let gain = floor + self.envelope * (1.0 - floor);
            let wet_left = left[index] * gain;
            let wet_right = right[index] * gain;
            left[index] = left[index] * (1.0 - mix_value) + wet_left * mix_value;
            right[index] = right[index] * (1.0 - mix_value) + wet_right * mix_value;
            if !input_gain.is_finite()
                || !floor.is_finite()
                || !mix_value.is_finite()
                || !self.envelope.is_finite()
                || !left[index].is_finite()
                || !right[index].is_finite()
            {
                return Err(non_finite());
            }
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.envelope = 0.0;
        self.external_input.reset();
    }
}

fn db_to_linear(value: f32) -> f32 {
    10.0_f32.powf(value / 20.0)
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
