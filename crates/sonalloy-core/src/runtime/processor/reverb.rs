use crate::compiler::CompiledReverbProcessor;
use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;

const DIFFUSION_COEFFICIENTS: [f32; 4] = [0.7, 0.7, 0.625, 0.625];
const TANK_DIFFUSION_COEFFICIENT: f32 = 0.7;

pub(crate) struct PlateReverbRuntime {
    pre_delay: DelayLine,
    pre_delay_frames: usize,
    input_diffusion: [Allpass; 4],
    left_tank: Tank,
    right_tank: Tank,
    phase: f32,
    modulation_increment: f32,
}

impl PlateReverbRuntime {
    pub(crate) fn new(compiled: &CompiledReverbProcessor) -> Self {
        Self {
            pre_delay: DelayLine::new(compiled.pre_delay_frames),
            pre_delay_frames: compiled.pre_delay_frames,
            input_diffusion: std::array::from_fn(|index| {
                Allpass::new(
                    compiled.input_diffusion_lengths[index],
                    DIFFUSION_COEFFICIENTS[index],
                )
            }),
            left_tank: Tank::new(
                compiled.tank_left_lengths,
                compiled.output_taps[..4]
                    .try_into()
                    .expect("four left taps"),
            ),
            right_tank: Tank::new(
                compiled.tank_right_lengths,
                compiled.output_taps[4..]
                    .try_into()
                    .expect("four right taps"),
            ),
            phase: 0.0,
            modulation_increment: compiled.modulation_increment,
        }
    }

    pub(crate) fn process(
        &mut self,
        decay: ValueSpan,
        damping: ValueSpan,
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
            let position = if left.len() <= 1 {
                0.0
            } else {
                #[allow(clippy::cast_precision_loss)]
                {
                    index as f32 / (left.len() - 1) as f32
                }
            };
            let decay = span_value(decay, position);
            let damping = span_value(damping, position);
            let width = span_value(width, position);
            let mix = span_value(mix, position);
            let dry_left = left[index];
            let dry_right = right[index];
            if !dry_left.is_finite()
                || !dry_right.is_finite()
                || !decay.is_finite()
                || !damping.is_finite()
                || !width.is_finite()
                || !mix.is_finite()
            {
                return Err(ProcessError::ProcessorFailure {
                    kind: ProcessorFailureKind::NonFinite,
                });
            }
            let input = if self.pre_delay_frames == 0 {
                0.5 * (dry_left + dry_right)
            } else {
                self.pre_delay
                    .process(self.pre_delay_frames, 0.5 * (dry_left + dry_right))?
            };
            let mut diffused = input;
            for allpass in &mut self.input_diffusion {
                diffused = allpass.process(diffused, 0.0)?;
            }
            let modulation = (self.phase * std::f32::consts::TAU).sin() * 0.5;
            let feedback = reverb_feedback(decay);
            let (left_signal, left_wet) = self.left_tank.process(
                diffused + self.right_tank.feedback() * feedback,
                damping,
                feedback,
                modulation,
            )?;
            let (right_signal, right_wet) = self.right_tank.process(
                diffused - left_signal * feedback,
                damping,
                feedback,
                -modulation,
            )?;
            let left_wet = 0.5 * (left_wet + left_signal);
            let right_wet = 0.5 * (right_wet + right_signal);
            let wet_mid = 0.5 * (left_wet + right_wet);
            let wet_left = wet_mid * (1.0 - width) + left_wet * width;
            let wet_right = wet_mid * (1.0 - width) + right_wet * width;
            left[index] = dry_left + (wet_left - dry_left) * mix;
            right[index] = dry_right + (wet_right - dry_right) * mix;
            if !left[index].is_finite() || !right[index].is_finite() {
                return Err(ProcessError::ProcessorFailure {
                    kind: ProcessorFailureKind::NonFinite,
                });
            }
            self.phase = (self.phase + self.modulation_increment).fract();
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.pre_delay.reset();
        for allpass in &mut self.input_diffusion {
            allpass.reset();
        }
        self.left_tank.reset();
        self.right_tank.reset();
        self.phase = 0.0;
    }
}

fn reverb_feedback(decay: f32) -> f32 {
    (decay * 0.2).clamp(0.0, 0.19)
}

struct Tank {
    first_allpass: Allpass,
    long_delay: DelayLine,
    long_delay_frames: usize,
    second_allpass: Allpass,
    output_delay: DelayLine,
    output_delay_frames: usize,
    output_taps: [usize; 4],
    damping_state: f32,
    last_output: f32,
}

impl Tank {
    fn new(lengths: [usize; 4], output_taps: [usize; 4]) -> Self {
        let first_delay = lengths[0];
        let long_delay = lengths[1];
        let second_delay = lengths[2];
        let output_delay = lengths[3];
        Self {
            first_allpass: Allpass::new(first_delay, TANK_DIFFUSION_COEFFICIENT),
            long_delay: DelayLine::with_capacity(
                long_delay.max(output_taps[0]).max(output_taps[1]),
            ),
            long_delay_frames: long_delay,
            second_allpass: Allpass::new(second_delay, TANK_DIFFUSION_COEFFICIENT),
            output_delay: DelayLine::with_capacity(
                output_delay.max(output_taps[2]).max(output_taps[3]),
            ),
            output_delay_frames: output_delay,
            output_taps,
            damping_state: 0.0,
            last_output: 0.0,
        }
    }

    fn process(
        &mut self,
        input: f32,
        damping: f32,
        decay: f32,
        modulation: f32,
    ) -> Result<(f32, f32), ProcessError> {
        let first = self
            .first_allpass
            .process(input, modulation.clamp(-0.5, 0.5))?;
        let long = self.long_delay.read(self.long_delay_frames);
        self.long_delay.write(first + long * decay)?;
        self.damping_state = long * (1.0 - damping) + self.damping_state * damping;
        if !self.damping_state.is_finite() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            });
        }
        let second = self
            .second_allpass
            .process(self.damping_state, -modulation.clamp(-0.5, 0.5))?;
        let output = self.output_delay.read(self.output_delay_frames);
        self.output_delay.write(second + output * decay)?;
        self.last_output = output;
        let wet = self.tap(0) - self.tap(1) + self.tap(2) - self.tap(3);
        Ok((output, wet))
    }

    fn tap(&self, index: usize) -> f32 {
        let delay = self.output_taps[index];
        self.output_delay.read(delay)
    }

    fn feedback(&self) -> f32 {
        self.last_output
    }

    fn reset(&mut self) {
        self.first_allpass.reset();
        self.long_delay.reset();
        self.second_allpass.reset();
        self.output_delay.reset();
        self.damping_state = 0.0;
        self.last_output = 0.0;
    }
}

struct Allpass {
    delay: DelayLine,
    delay_frames: usize,
    coefficient: f32,
}

impl Allpass {
    fn new(delay_frames: usize, coefficient: f32) -> Self {
        Self {
            delay: DelayLine::new(delay_frames),
            delay_frames,
            coefficient,
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn process(&mut self, input: f32, delay_offset: f32) -> Result<f32, ProcessError> {
        let delay = self
            .delay
            .read_fractional(self.delay_frames as f32 + delay_offset)?;
        let output = -self.coefficient * input + delay;
        self.delay.write(input + self.coefficient * delay)?;
        if output.is_finite() {
            Ok(output)
        } else {
            Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            })
        }
    }

    fn reset(&mut self) {
        self.delay.reset();
    }
}

struct DelayLine {
    buffer: Vec<f32>,
    position: usize,
}

impl DelayLine {
    fn new(delay_frames: usize) -> Self {
        Self::with_capacity(delay_frames)
    }

    fn with_capacity(delay_frames: usize) -> Self {
        Self {
            buffer: vec![0.0; delay_frames.saturating_add(4).max(4)],
            position: 0,
        }
    }

    fn read(&self, delay: usize) -> f32 {
        let delay = delay.min(self.buffer.len() - 1);
        let position = (self.position + self.buffer.len() - delay) % self.buffer.len();
        self.buffer[position]
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    fn read_fractional(&self, offset: f32) -> Result<f32, ProcessError> {
        if !offset.is_finite() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            });
        }
        let base = self.position as f32 - offset;
        let length = self.buffer.len() as f32;
        let wrapped = base.rem_euclid(length);
        let index = wrapped.floor() as usize;
        let next = (index + 1) % self.buffer.len();
        let fraction = wrapped - index as f32;
        Ok(self.buffer[index] + (self.buffer[next] - self.buffer[index]) * fraction)
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

    fn process(&mut self, delay: usize, value: f32) -> Result<f32, ProcessError> {
        let delayed = self.read(delay);
        self.write(value)?;
        Ok(delayed)
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.position = 0;
    }
}

fn span_value(span: ValueSpan, position: f32) -> f32 {
    span.start + (span.end - span.start) * position
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::CompiledReverbProcessor;
    use crate::parameter::ParameterHandle;

    fn compiled() -> CompiledReverbProcessor {
        CompiledReverbProcessor {
            pre_delay_frames: 0,
            decay: ParameterHandle::new(0),
            damping: ParameterHandle::new(1),
            width: ParameterHandle::new(2),
            mix: ParameterHandle::new(3),
            input_diffusion_lengths: [4, 5, 6, 7],
            tank_left_lengths: [8, 9, 10, 11],
            tank_right_lengths: [12, 13, 14, 15],
            output_taps: [2, 3, 4, 5, 2, 3, 4, 5],
            modulation_increment: 0.001,
        }
    }

    #[test]
    fn impulse_produces_a_finite_tail() {
        let mut runtime = PlateReverbRuntime::new(&compiled());
        let constant = ValueSpan {
            start: 0.7,
            end: 0.7,
        };
        let mut left = vec![0.0; 256];
        let mut right = vec![0.0; 256];
        left[0] = 1.0;
        runtime
            .process(
                constant, constant, constant, constant, &mut left, &mut right,
            )
            .expect("reverb process");
        assert!(left.iter().chain(&right).all(|value| value.is_finite()));
        assert!(left.iter().skip(1).any(|value| value.abs() > 1.0e-5));
    }

    #[test]
    fn reset_removes_tail() {
        let mut runtime = PlateReverbRuntime::new(&compiled());
        let constant = ValueSpan {
            start: 0.8,
            end: 0.8,
        };
        let mut left = vec![1.0; 32];
        let mut right = vec![0.0; 32];
        runtime
            .process(
                constant, constant, constant, constant, &mut left, &mut right,
            )
            .expect("reverb process");
        runtime.reset();
        let mut reset_left = vec![0.0; 32];
        let mut reset_right = vec![0.0; 32];
        runtime
            .process(
                constant,
                constant,
                constant,
                constant,
                &mut reset_left,
                &mut reset_right,
            )
            .expect("reset reverb process");
        assert!(
            reset_left
                .iter()
                .chain(&reset_right)
                .all(|value| value.abs() < 1.0e-6)
        );
    }
}
