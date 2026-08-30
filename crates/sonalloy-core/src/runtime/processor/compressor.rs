use crate::compiler::{CompiledCompressorProcessor, CompiledDynamicsDetector};
use crate::process::{ProcessError, ProcessorFailureKind};
use crate::runtime::external_audio::{ExternalAudioBlock, ExternalInputDelay};

use super::ValueSpan;

pub(crate) struct CompressorRuntime {
    attack_coeff: f32,
    release_coeff: f32,
    knee_db: f32,
    reduction_db: f32,
    external_input: Option<ExternalInputDelay>,
}

impl CompressorRuntime {
    pub(crate) fn new(compiled: &CompiledCompressorProcessor) -> Self {
        Self {
            attack_coeff: compiled.attack_coeff,
            release_coeff: compiled.release_coeff,
            knee_db: compiled.knee_db,
            reduction_db: 0.0,
            external_input: (compiled.detector == CompiledDynamicsDetector::ExternalAudio)
                .then(|| ExternalInputDelay::new(compiled.external_input_alignment_frames)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn process(
        &mut self,
        threshold_db: ValueSpan,
        ratio: ValueSpan,
        makeup_gain_db: ValueSpan,
        mix: ValueSpan,
        external: ExternalAudioBlock<'_>,
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
            let (detector_left, detector_right) =
                if let Some(external_input) = self.external_input.as_mut() {
                    external_input.next(external, index)
                } else {
                    (input_left, input_right)
                };
            let level = detector_left
                .abs()
                .max(detector_right.abs())
                .max(f32::EPSILON);
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
        if let Some(external_input) = self.external_input.as_mut() {
            external_input.reset();
        }
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

#[cfg(test)]
mod tests {
    use super::CompressorRuntime;
    use crate::compiler::{
        CompiledCompressorParameters, CompiledCompressorProcessor, CompiledDynamicsDetector,
    };
    use crate::parameter::ParameterHandle;
    use crate::runtime::external_audio::ExternalAudioBlock;
    use crate::runtime::modulation::ValueSpan;

    fn span(value: f32) -> ValueSpan {
        ValueSpan {
            start: value,
            end: value,
        }
    }

    fn runtime(attack_coeff: f32, release_coeff: f32) -> CompressorRuntime {
        CompressorRuntime::new(&CompiledCompressorProcessor {
            attack_coeff,
            release_coeff,
            knee_db: 0.0,
            detector: CompiledDynamicsDetector::SelfSignal,
            external_input_alignment_frames: 0,
            parameters: CompiledCompressorParameters {
                threshold_db: ParameterHandle::new(0),
                ratio: ParameterHandle::new(1),
                makeup_gain_db: ParameterHandle::new(2),
                mix: ParameterHandle::new(3),
            },
        })
    }

    #[test]
    fn ratio_one_without_makeup_is_an_identity() {
        let mut runtime = runtime(0.0, 0.0);
        let original_left = [0.8, -0.4, 0.2, -0.1];
        let original_right = [-0.3, 0.6, -0.2, 0.05];
        let mut left = original_left;
        let mut right = original_right;
        runtime
            .process(
                span(-18.0),
                span(1.0),
                span(0.0),
                span(1.0),
                ExternalAudioBlock::new(&[]),
                &mut left,
                &mut right,
            )
            .expect("compressor processes");
        assert!(
            left.into_iter()
                .zip(original_left)
                .all(|(actual, expected)| (actual - expected).abs() < f32::EPSILON)
        );
        assert!(
            right
                .into_iter()
                .zip(original_right)
                .all(|(actual, expected)| (actual - expected).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn stereo_link_uses_one_gain_reduction_for_both_channels() {
        let mut runtime = runtime(0.0, 0.0);
        let mut left = [0.8; 4];
        let mut right = [0.2; 4];
        runtime
            .process(
                span(-12.0),
                span(4.0),
                span(0.0),
                span(1.0),
                ExternalAudioBlock::new(&[]),
                &mut left,
                &mut right,
            )
            .expect("compressor processes");
        let left_gain = left[0] / 0.8;
        let right_gain = right[0] / 0.2;
        assert!(left_gain < 1.0);
        assert!((left_gain - right_gain).abs() < 1.0e-6);
    }

    #[test]
    fn attack_reacts_to_loud_input_and_release_recovers() {
        let mut runtime = runtime(0.0, 0.5);
        let mut left = vec![0.8; 4];
        left.extend([0.2; 8]);
        let mut right = left.clone();
        runtime
            .process(
                span(-12.0),
                span(4.0),
                span(0.0),
                span(1.0),
                ExternalAudioBlock::new(&[]),
                &mut left,
                &mut right,
            )
            .expect("compressor processes");
        assert!(left[0] < 0.8);
        assert!(left[11] > left[4]);
    }
}
