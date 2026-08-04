use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;

pub(crate) struct StereoDelayRuntime {
    left: DelayLine,
    right: DelayLine,
}

impl StereoDelayRuntime {
    pub(crate) fn new(delay_frames: usize) -> Self {
        Self {
            left: DelayLine::new(delay_frames),
            right: DelayLine::new(delay_frames),
        }
    }

    pub(crate) fn process(
        &mut self,
        feedback: ValueSpan,
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
            let position = if left.len() <= 1 {
                0.0
            } else {
                #[allow(clippy::cast_precision_loss)]
                {
                    index as f32 / (left.len() - 1) as f32
                }
            };
            let feedback = feedback.start + (feedback.end - feedback.start) * position;
            let mix = mix.start + (mix.end - mix.start) * position;
            let left_input = left[index];
            let right_input = right[index];
            let left_delayed = self.left.read();
            let right_delayed = self.right.read();
            if !left_input.is_finite()
                || !right_input.is_finite()
                || !left_delayed.is_finite()
                || !right_delayed.is_finite()
                || !feedback.is_finite()
                || !mix.is_finite()
            {
                return Err(ProcessError::ProcessorFailure {
                    kind: ProcessorFailureKind::NonFinite,
                });
            }
            self.left.write(left_input + left_delayed * feedback)?;
            self.right.write(right_input + right_delayed * feedback)?;
            left[index] = left_input * (1.0 - mix) + left_delayed * mix;
            right[index] = right_input * (1.0 - mix) + right_delayed * mix;
            if !left[index].is_finite() || !right[index].is_finite() {
                return Err(ProcessError::ProcessorFailure {
                    kind: ProcessorFailureKind::NonFinite,
                });
            }
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
    }
}

struct DelayLine {
    buffer: Vec<f32>,
    position: usize,
}

impl DelayLine {
    fn new(delay_frames: usize) -> Self {
        Self {
            buffer: vec![0.0; delay_frames.max(1)],
            position: 0,
        }
    }

    fn read(&self) -> f32 {
        self.buffer[self.position]
    }

    fn write(&mut self, value: f32) -> Result<(), ProcessError> {
        if !value.is_finite() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            });
        }
        self.buffer[self.position] = value;
        self.position += 1;
        if self.position == self.buffer.len() {
            self.position = 0;
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.position = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_frame_delay_places_the_first_echo_at_the_next_frame() {
        let mut runtime = StereoDelayRuntime::new(1);
        let mut left = [1.0, 0.0, 0.0];
        let mut right = [0.0, 0.0, 0.0];
        let feedback = ValueSpan {
            start: 0.0,
            end: 0.0,
        };
        let mix = ValueSpan {
            start: 1.0,
            end: 1.0,
        };
        runtime
            .process(feedback, mix, &mut left, &mut right)
            .expect("delay process");
        assert!(
            left.iter()
                .zip([0.0, 1.0, 0.0])
                .all(|(actual, expected)| (actual - expected).abs() < 1.0e-6)
        );
        assert!(
            right
                .iter()
                .zip([0.0, 0.0, 0.0])
                .all(|(actual, expected)| (actual - expected).abs() < 1.0e-6)
        );
    }

    #[test]
    fn reset_clears_the_tail() {
        let mut runtime = StereoDelayRuntime::new(2);
        let mut left = [1.0, 0.0, 0.0];
        let mut right = [0.0, 0.0, 0.0];
        let feedback = ValueSpan {
            start: 0.8,
            end: 0.8,
        };
        let mix = ValueSpan {
            start: 1.0,
            end: 1.0,
        };
        runtime
            .process(feedback, mix, &mut left, &mut right)
            .expect("delay process");
        runtime.reset();
        let mut reset_left = [0.0, 0.0];
        let mut reset_right = [0.0, 0.0];
        runtime
            .process(feedback, mix, &mut reset_left, &mut reset_right)
            .expect("reset delay process");
        assert!(reset_left.iter().all(|sample| sample.abs() < 1.0e-6));
    }
}
