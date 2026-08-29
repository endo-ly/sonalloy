use crate::compiler::{CompiledDelayProcessor, CompiledDelayTime};
use crate::definition::DelayFeedbackMode;
use crate::process::{ProcessError, ProcessorFailureKind};
use crate::runtime::fractional_delay::FractionalDelayLine;

use super::ValueSpan;

use crate::definition::MAX_DELAY_TAPS;

const DELAY_TIME_SMOOTHING_SECONDS: f32 = 0.020;

pub(crate) struct StereoDelayRuntime {
    left: FractionalDelayLine,
    right: FractionalDelayLine,
    time: CompiledDelayTime,
    feedback_mode: DelayFeedbackMode,
    taps: Box<[CompiledDelayTapRuntime]>,
    wet_normalization: f32,
    sample_rate: f32,
    resolved_delay_frames: f32,
    initialized: bool,
}

struct CompiledDelayTapRuntime {
    time: CompiledDelayTime,
    gain_linear: f32,
}

impl StereoDelayRuntime {
    pub(crate) fn new(compiled: &CompiledDelayProcessor, sample_rate: f32) -> Self {
        Self {
            left: FractionalDelayLine::new(compiled.max_delay_frames),
            right: FractionalDelayLine::new(compiled.max_delay_frames),
            time: compiled.time,
            feedback_mode: compiled.feedback_mode,
            taps: compiled
                .taps
                .iter()
                .map(|tap| CompiledDelayTapRuntime {
                    time: tap.time,
                    gain_linear: tap.gain_linear,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            wet_normalization: compiled.wet_normalization,
            sample_rate,
            resolved_delay_frames: 0.0,
            initialized: false,
        }
    }

    pub(crate) fn process(
        &mut self,
        feedback: ValueSpan,
        mix: ValueSpan,
        tempo_bpm: f64,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() || !tempo_bpm.is_finite() || tempo_bpm <= 0.0 {
            return Err(invalid_state());
        }
        let target_delay = self.resolve_time(self.time, tempo_bpm)?;
        #[allow(clippy::cast_possible_truncation)]
        let smoothing = (-1.0
            / (f64::from(DELAY_TIME_SMOOTHING_SECONDS) * f64::from(self.sample_rate)))
        .exp() as f32;
        if !self.initialized {
            self.resolved_delay_frames = target_delay;
            self.initialized = true;
        }
        let mut tap_frames = [0.0; MAX_DELAY_TAPS];
        for (resolved, tap) in tap_frames.iter_mut().zip(&self.taps) {
            *resolved = self.resolve_time(tap.time, tempo_bpm)?;
        }
        for index in 0..left.len() {
            let current_feedback = feedback.value_at(index, left.len());
            let current_mix = mix.value_at(index, left.len());
            if !current_feedback.is_finite()
                || !current_mix.is_finite()
                || !left[index].is_finite()
                || !right[index].is_finite()
            {
                return Err(non_finite());
            }
            self.resolved_delay_frames =
                target_delay + smoothing * (self.resolved_delay_frames - target_delay);
            let primary_left = self.left.read(self.resolved_delay_frames)?;
            let primary_right = self.right.read(self.resolved_delay_frames)?;
            let mut wet_left = primary_left;
            let mut wet_right = primary_right;
            for (tap, &tap_frames) in self.taps.iter().zip(tap_frames.iter()) {
                wet_left += self.left.read(tap_frames)? * tap.gain_linear;
                wet_right += self.right.read(tap_frames)? * tap.gain_linear;
            }
            let feedback = current_feedback.clamp(0.0, 0.95);
            let (write_left, write_right) = match self.feedback_mode {
                DelayFeedbackMode::Stereo => (
                    left[index] + primary_left * feedback,
                    right[index] + primary_right * feedback,
                ),
                DelayFeedbackMode::PingPong => (
                    left[index] + primary_right * feedback,
                    right[index] + primary_left * feedback,
                ),
            };
            self.left.write(write_left)?;
            self.right.write(write_right)?;
            left[index] =
                left[index] * (1.0 - current_mix) + wet_left * self.wet_normalization * current_mix;
            right[index] = right[index] * (1.0 - current_mix)
                + wet_right * self.wet_normalization * current_mix;
            if !left[index].is_finite() || !right[index].is_finite() {
                return Err(non_finite());
            }
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
        self.resolved_delay_frames = 0.0;
        self.initialized = false;
    }

    fn resolve_time(&self, time: CompiledDelayTime, tempo_bpm: f64) -> Result<f32, ProcessError> {
        let seconds = match time {
            CompiledDelayTime::Seconds(seconds) => seconds,
            CompiledDelayTime::Beats(beats) => beats * 60.0 / tempo_bpm,
        };
        let capacity = self.left.capacity().saturating_sub(3);
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let frames = (seconds * f64::from(self.sample_rate)).clamp(1.0, capacity as f64) as f32;
        if frames.is_finite() {
            Ok(frames)
        } else {
            Err(invalid_state())
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
    use super::StereoDelayRuntime;
    use crate::compiler::{CompiledDelayProcessor, CompiledDelayTime};
    use crate::definition::DelayFeedbackMode;
    use crate::parameter::ParameterHandle;
    use crate::runtime::modulation::ValueSpan;

    fn span(value: f32) -> ValueSpan {
        ValueSpan {
            start: value,
            end: value,
        }
    }

    fn compiled(time: CompiledDelayTime, mode: DelayFeedbackMode) -> CompiledDelayProcessor {
        CompiledDelayProcessor {
            time,
            feedback_mode: mode,
            taps: Box::new([]),
            max_delay_frames: 1_000,
            wet_normalization: 1.0,
            feedback: ParameterHandle::new(0),
            mix: ParameterHandle::new(1),
        }
    }

    #[test]
    fn beats_resolve_from_the_process_tempo() {
        let mut runtime = StereoDelayRuntime::new(
            &compiled(CompiledDelayTime::Beats(1.0), DelayFeedbackMode::Stereo),
            1_000.0,
        );
        let mut left = [0.0; 502];
        let mut right = [0.0; 502];
        left[0] = 1.0;
        runtime
            .process(span(0.0), span(1.0), 120.0, &mut left, &mut right)
            .expect("tempo delay processes");

        assert!(left[..500].iter().all(|sample| sample.abs() < 1.0e-6));
        assert!((left[500] - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn ping_pong_feedback_crosses_channels() {
        let mut runtime = StereoDelayRuntime::new(
            &compiled(
                CompiledDelayTime::Seconds(0.003),
                DelayFeedbackMode::PingPong,
            ),
            1_000.0,
        );
        let mut left = [0.0; 8];
        let mut right = [0.0; 8];
        left[0] = 1.0;
        runtime
            .process(span(0.95), span(1.0), 120.0, &mut left, &mut right)
            .expect("ping-pong delay processes");

        assert!(right[6] > 0.8);
    }
}
