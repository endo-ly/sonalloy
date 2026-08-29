use crate::compiler::{CompiledDynamicsDetector, CompiledGateProcessor};
use crate::process::{ProcessError, ProcessorFailureKind};
use crate::runtime::external_audio::{ExternalAudioBlock, ExternalInputDelay};

use super::ValueSpan;

pub(crate) struct GateRuntime {
    detector: f32,
    gain: f32,
    open: bool,
    hold_remaining: usize,
    detector_attack_coeff: f32,
    detector_release_coeff: f32,
    attack_coeff: f32,
    release_coeff: f32,
    hold_frames: usize,
    hysteresis_db: f32,
    external_input: Option<ExternalInputDelay>,
}

impl GateRuntime {
    pub(crate) fn new(compiled: &CompiledGateProcessor) -> Self {
        Self {
            detector: 0.0,
            gain: 0.0,
            open: false,
            hold_remaining: 0,
            detector_attack_coeff: compiled.detector_attack_coeff,
            detector_release_coeff: compiled.detector_release_coeff,
            attack_coeff: compiled.attack_coeff,
            release_coeff: compiled.release_coeff,
            hold_frames: compiled.hold_frames,
            hysteresis_db: compiled.hysteresis_db,
            external_input: (compiled.detector == CompiledDynamicsDetector::ExternalAudio)
                .then(|| ExternalInputDelay::new(compiled.external_input_alignment_frames)),
        }
    }

    pub(crate) fn process(
        &mut self,
        threshold_db: ValueSpan,
        range_db: ValueSpan,
        external: ExternalAudioBlock<'_>,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(invalid_state());
        }
        for index in 0..left.len() {
            let threshold = threshold_db.value_at(index, left.len());
            let range = range_db.value_at(index, left.len());
            let input_level = if let Some(external_input) = self.external_input.as_mut() {
                let (external_left, external_right) = external_input.next(external, index);
                external_left.abs().max(external_right.abs())
            } else {
                left[index].abs().max(right[index].abs())
            };
            if !threshold.is_finite()
                || !range.is_finite()
                || !input_level.is_finite()
                || !self.hysteresis_db.is_finite()
            {
                return Err(non_finite());
            }
            let detector_coeff = if input_level > self.detector {
                self.detector_attack_coeff
            } else {
                self.detector_release_coeff
            };
            self.detector = input_level + detector_coeff * (self.detector - input_level);
            let detector_db = 20.0 * self.detector.max(1.0e-12).log10();
            if !self.open && detector_db >= threshold {
                self.open = true;
                self.hold_remaining = self.hold_frames;
            } else if self.open && detector_db < threshold - self.hysteresis_db {
                if self.hold_remaining == 0 {
                    self.open = false;
                } else {
                    self.hold_remaining -= 1;
                }
            } else if self.open {
                self.hold_remaining = self.hold_frames;
            }
            if range.abs() <= f32::EPSILON {
                self.gain = 1.0;
            } else {
                let target = if self.open {
                    1.0
                } else {
                    10.0_f32.powf(range.clamp(-96.0, 0.0) / 20.0)
                };
                let coefficient = if target > self.gain {
                    self.attack_coeff
                } else {
                    self.release_coeff
                };
                self.gain = target + coefficient * (self.gain - target);
                left[index] *= self.gain;
                right[index] *= self.gain;
            }
            if !self.detector.is_finite()
                || !self.gain.is_finite()
                || !left[index].is_finite()
                || !right[index].is_finite()
            {
                return Err(non_finite());
            }
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.detector = 0.0;
        self.gain = 0.0;
        self.open = false;
        self.hold_remaining = 0;
        if let Some(external_input) = self.external_input.as_mut() {
            external_input.reset();
        }
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
    use super::GateRuntime;
    use crate::compiler::{CompiledDynamicsDetector, CompiledGateProcessor};
    use crate::runtime::external_audio::ExternalAudioBlock;
    use crate::runtime::modulation::ValueSpan;

    fn compiled(
        detector_attack_coeff: f32,
        detector_release_coeff: f32,
        attack_coeff: f32,
        release_coeff: f32,
        hold_frames: usize,
        hysteresis_db: f32,
    ) -> CompiledGateProcessor {
        CompiledGateProcessor {
            hysteresis_db,
            detector_attack_coeff,
            detector_release_coeff,
            attack_coeff,
            hold_frames,
            release_coeff,
            detector: CompiledDynamicsDetector::SelfSignal,
            external_input_alignment_frames: 0,
            parameters: crate::compiler::CompiledGateParameters {
                threshold_db: crate::parameter::ParameterHandle::new(0),
                range_db: crate::parameter::ParameterHandle::new(1),
            },
        }
    }

    fn span(value: f32) -> ValueSpan {
        ValueSpan {
            start: value,
            end: value,
        }
    }

    #[test]
    fn linked_gate_opens_on_stereo_peak_and_holds() {
        let mut runtime = GateRuntime::new(&compiled(0.0, 0.0, 0.0, 0.0, 2, 3.0));
        let mut left = [0.01, 1.0, 1.0, 0.01];
        let mut right = [0.01, 0.5, 0.5, 0.01];
        runtime
            .process(
                span(-20.0),
                span(-96.0),
                ExternalAudioBlock::new(&[]),
                &mut left,
                &mut right,
            )
            .expect("gate processes");

        assert!(left[0] < 0.001);
        assert!(left[1] > 0.9 && right[1] > 0.4);
        assert!(left[2] > 0.9);
        assert!(left.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn zero_range_keeps_a_closed_gate_at_unity() {
        let mut runtime = GateRuntime::new(&compiled(0.25, 0.5, 0.25, 0.5, 2, 3.0));
        let original_left = [0.001, -0.002, 0.003, -0.004];
        let original_right = [-0.004, 0.003, -0.002, 0.001];
        let mut left = original_left;
        let mut right = original_right;
        runtime
            .process(
                span(-20.0),
                span(0.0),
                ExternalAudioBlock::new(&[]),
                &mut left,
                &mut right,
            )
            .expect("unity-range gate processes");

        assert!(
            left.iter()
                .zip(original_left)
                .all(|(actual, expected)| actual.to_bits() == expected.to_bits())
        );
        assert!(
            right
                .iter()
                .zip(original_right)
                .all(|(actual, expected)| actual.to_bits() == expected.to_bits())
        );

        runtime.reset();
        let mut reset_left = original_left;
        let mut reset_right = original_right;
        runtime
            .process(
                span(-20.0),
                span(0.0),
                ExternalAudioBlock::new(&[]),
                &mut reset_left,
                &mut reset_right,
            )
            .expect("reset unity-range gate processes");
        assert!(
            reset_left
                .iter()
                .zip(original_left)
                .all(|(actual, expected)| actual.to_bits() == expected.to_bits())
        );
        assert!(
            reset_right
                .iter()
                .zip(original_right)
                .all(|(actual, expected)| actual.to_bits() == expected.to_bits())
        );
    }
}
