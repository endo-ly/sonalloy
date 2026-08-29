use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;

pub(crate) struct TransientShaperRuntime {
    fast: f32,
    slow: f32,
    fast_attack_coeff: f32,
    fast_release_coeff: f32,
    slow_attack_coeff: f32,
    slow_release_coeff: f32,
}

impl TransientShaperRuntime {
    pub(crate) fn new(
        fast_attack_coeff: f32,
        fast_release_coeff: f32,
        slow_attack_coeff: f32,
        slow_release_coeff: f32,
    ) -> Self {
        Self {
            fast: 0.0,
            slow: 0.0,
            fast_attack_coeff,
            fast_release_coeff,
            slow_attack_coeff,
            slow_release_coeff,
        }
    }

    pub(crate) fn process(
        &mut self,
        attack: ValueSpan,
        sustain: ValueSpan,
        mix: ValueSpan,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(invalid_state());
        }
        for index in 0..left.len() {
            let input_level = left[index].abs().max(right[index].abs());
            let attack_value = attack.value_at(index, left.len());
            let sustain_value = sustain.value_at(index, left.len());
            let mix_value = mix.value_at(index, left.len());
            if !input_level.is_finite()
                || !attack_value.is_finite()
                || !sustain_value.is_finite()
                || !mix_value.is_finite()
            {
                return Err(non_finite());
            }
            self.fast = envelope(
                self.fast,
                input_level,
                self.fast_attack_coeff,
                self.fast_release_coeff,
            );
            self.slow = envelope(
                self.slow,
                input_level,
                self.slow_attack_coeff,
                self.slow_release_coeff,
            );
            let denominator = self.slow.max(1.0e-6);
            let transient = ((self.fast - self.slow) / denominator).clamp(0.0, 1.0);
            let sustain_component = ((self.slow - self.fast) / denominator).clamp(0.0, 1.0);
            let gain_db = (attack_value * transient * 12.0
                + sustain_value * sustain_component * 12.0)
                .clamp(-24.0, 24.0);
            let gain = 10.0_f32.powf(gain_db / 20.0);
            let wet_left = left[index] * gain;
            let wet_right = right[index] * gain;
            left[index] = left[index] * (1.0 - mix_value) + wet_left * mix_value;
            right[index] = right[index] * (1.0 - mix_value) + wet_right * mix_value;
            if !self.fast.is_finite()
                || !self.slow.is_finite()
                || !left[index].is_finite()
                || !right[index].is_finite()
            {
                return Err(non_finite());
            }
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.fast = 0.0;
        self.slow = 0.0;
    }
}

fn envelope(current: f32, input: f32, attack_coeff: f32, release_coeff: f32) -> f32 {
    let coefficient = if input > current {
        attack_coeff
    } else {
        release_coeff
    };
    input + coefficient * (current - input)
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
    use super::TransientShaperRuntime;
    use crate::runtime::modulation::ValueSpan;

    fn span(value: f32) -> ValueSpan {
        ValueSpan {
            start: value,
            end: value,
        }
    }

    #[test]
    fn attack_and_sustain_controls_change_a_linked_signal() {
        let mut runtime = TransientShaperRuntime::new(0.0, 0.0, 0.9, 0.9);
        let mut left = [1.0, 0.5, 0.5, 0.5];
        let mut right = left;
        runtime
            .process(span(1.0), span(-1.0), span(1.0), &mut left, &mut right)
            .expect("transient shaper processes");

        assert!(left.iter().all(|sample| sample.is_finite()));
        assert!(left[0] > 1.0);
        assert!(
            left.iter()
                .zip(right)
                .all(|(left, right)| left.to_bits() == right.to_bits())
        );
    }
}
