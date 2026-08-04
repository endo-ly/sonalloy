use crate::compiler::{CompiledReverbProcessor, ReverbOutputTap, ReverbTapSource};
use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;

const INPUT_BANDWIDTH: f32 = 0.9995;
const INPUT_DIFFUSION_COEFFICIENTS: [f32; 4] = [0.75, 0.75, 0.625, 0.625];
const TANK_FIRST_DIFFUSION_COEFFICIENT: f32 = 0.7;
const TANK_SECOND_DIFFUSION_COEFFICIENT: f32 = 0.5;
const OUTPUT_GAIN: f32 = 0.6;

pub(crate) struct PlateReverbRuntime {
    pre_delay: DelayLine,
    pre_delay_frames: usize,
    input_bandwidth: OnePole,
    input_diffusion: [Allpass; 4],
    left_tank: Tank,
    right_tank: Tank,
    left_output_taps: [ReverbOutputTap; 7],
    right_output_taps: [ReverbOutputTap; 7],
    phase: f32,
    modulation_increment: f32,
}

impl PlateReverbRuntime {
    pub(crate) fn new(compiled: &CompiledReverbProcessor) -> Self {
        Self {
            pre_delay: DelayLine::new(compiled.pre_delay_frames),
            pre_delay_frames: compiled.pre_delay_frames,
            input_bandwidth: OnePole::new(),
            input_diffusion: std::array::from_fn(|index| {
                Allpass::new(
                    compiled.input_diffusion_lengths[index],
                    INPUT_DIFFUSION_COEFFICIENTS[index],
                    0.0,
                )
            }),
            left_tank: Tank::new(compiled.tank_left_lengths, compiled.modulation_excursion),
            right_tank: Tank::new(compiled.tank_right_lengths, compiled.modulation_excursion),
            left_output_taps: compiled.left_output_taps,
            right_output_taps: compiled.right_output_taps,
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
            let position = span_position(index, left.len());
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

            let mono = 0.5 * (dry_left + dry_right);
            let pre_delayed = if self.pre_delay_frames == 0 {
                mono
            } else {
                self.pre_delay.process(mono)?
            };
            let mut diffused = self.input_bandwidth.process(pre_delayed, INPUT_BANDWIDTH)?;
            for allpass in &mut self.input_diffusion {
                diffused = allpass.process(diffused, 0.0)?;
            }

            let feedback = reverb_feedback(decay);
            let previous_left = self.left_tank.feedback();
            let previous_right = self.right_tank.feedback();
            let phase = self.phase * std::f32::consts::TAU;
            let left_modulation = modulation_offset(phase, self.left_tank.modulation_excursion());
            let right_modulation = modulation_offset(
                phase + std::f32::consts::FRAC_PI_2,
                self.right_tank.modulation_excursion(),
            );

            self.left_tank.process(
                diffused + previous_right * feedback,
                damping,
                feedback,
                left_modulation,
            )?;
            self.right_tank.process(
                diffused + previous_left * feedback,
                damping,
                feedback,
                right_modulation,
            )?;

            let wet_left =
                sum_output_taps(&self.left_output_taps, &self.left_tank, &self.right_tank)?;
            let wet_right =
                sum_output_taps(&self.right_output_taps, &self.left_tank, &self.right_tank)?;
            let wet_mid = 0.5 * (wet_left + wet_right);
            let wet_left = wet_mid * (1.0 - width) + wet_left * width;
            let wet_right = wet_mid * (1.0 - width) + wet_right * width;
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
        self.input_bandwidth.reset();
        for allpass in &mut self.input_diffusion {
            allpass.reset();
        }
        self.left_tank.reset();
        self.right_tank.reset();
        self.phase = 0.0;
    }
}

fn sum_output_taps(
    taps: &[ReverbOutputTap; 7],
    left: &Tank,
    right: &Tank,
) -> Result<f32, ProcessError> {
    let mut sum = 0.0;
    for tap in taps {
        let value = match tap.source {
            ReverbTapSource::LeftLongDelay => left.long_delay_tap(tap.delay_frames),
            ReverbTapSource::LeftTankAllpass => left.second_allpass_tap(tap.delay_frames),
            ReverbTapSource::LeftOutputDelay => left.output_delay_tap(tap.delay_frames),
            ReverbTapSource::RightLongDelay => right.long_delay_tap(tap.delay_frames),
            ReverbTapSource::RightTankAllpass => right.second_allpass_tap(tap.delay_frames),
            ReverbTapSource::RightOutputDelay => right.output_delay_tap(tap.delay_frames),
        }?;
        sum += f32::from(tap.sign) * value;
    }
    let output = OUTPUT_GAIN * sum;
    if output.is_finite() {
        Ok(output)
    } else {
        Err(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::NonFinite,
        })
    }
}

fn reverb_feedback(decay: f32) -> f32 {
    (decay * 0.2).clamp(0.0, 0.19)
}

fn modulation_offset(phase: f32, excursion: f32) -> f32 {
    (1.0 + phase.sin()) * 0.5 * excursion
}

struct Tank {
    modulated_allpass: Allpass,
    long_delay: DelayLine,
    damping: OnePole,
    second_allpass: Allpass,
    output_delay: DelayLine,
    modulation_excursion: f32,
    last_output: f32,
}

impl Tank {
    fn new(lengths: [usize; 4], modulation_excursion: f32) -> Self {
        let [modulated_allpass, long_delay, second_allpass, output_delay] = lengths;
        Self {
            modulated_allpass: Allpass::new(
                modulated_allpass,
                TANK_FIRST_DIFFUSION_COEFFICIENT,
                modulation_excursion,
            ),
            long_delay: DelayLine::new(long_delay),
            damping: OnePole::new(),
            second_allpass: Allpass::new(second_allpass, TANK_SECOND_DIFFUSION_COEFFICIENT, 0.0),
            output_delay: DelayLine::new(output_delay),
            modulation_excursion,
            last_output: 0.0,
        }
    }

    fn process(
        &mut self,
        input: f32,
        damping: f32,
        decay: f32,
        modulation: f32,
    ) -> Result<(), ProcessError> {
        let first = self.modulated_allpass.process(input, modulation)?;
        let long = self.long_delay.process(first)?;
        let damped = self.damping.process(long, 1.0 - damping)?;
        let second = self.second_allpass.process(damped * decay, 0.0)?;
        self.last_output = self.output_delay.read_current()?;
        self.output_delay.write(second)?;
        Ok(())
    }

    fn feedback(&self) -> f32 {
        self.last_output
    }

    fn modulation_excursion(&self) -> f32 {
        self.modulation_excursion
    }

    fn long_delay_tap(&self, delay: usize) -> Result<f32, ProcessError> {
        self.long_delay.tap(delay)
    }

    fn second_allpass_tap(&self, delay: usize) -> Result<f32, ProcessError> {
        self.second_allpass.tap(delay)
    }

    fn output_delay_tap(&self, delay: usize) -> Result<f32, ProcessError> {
        self.output_delay.tap(delay)
    }

    fn reset(&mut self) {
        self.modulated_allpass.reset();
        self.long_delay.reset();
        self.damping.reset();
        self.second_allpass.reset();
        self.output_delay.reset();
        self.last_output = 0.0;
    }
}

struct Allpass {
    delay: DelayLine,
    coefficient: f32,
}

impl Allpass {
    fn new(delay_frames: usize, coefficient: f32, modulation_excursion: f32) -> Self {
        let extra_capacity = if modulation_excursion > 0.0 {
            modulation_excursion + 1.0
        } else {
            0.0
        };
        Self {
            delay: DelayLine::with_extra_capacity(delay_frames, extra_capacity),
            coefficient,
        }
    }

    fn process(&mut self, input: f32, modulation: f32) -> Result<f32, ProcessError> {
        let delayed = if modulation == 0.0 {
            self.delay.read_current()?
        } else {
            self.delay.read_modulated(modulation)?
        };
        let output = -self.coefficient * input + delayed;
        self.delay.write(input + self.coefficient * delayed)?;
        if output.is_finite() {
            Ok(output)
        } else {
            Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            })
        }
    }

    fn tap(&self, delay: usize) -> Result<f32, ProcessError> {
        self.delay.tap(delay)
    }

    fn reset(&mut self) {
        self.delay.reset();
    }
}

struct OnePole {
    state: f32,
}

impl OnePole {
    fn new() -> Self {
        Self { state: 0.0 }
    }

    fn process(&mut self, input: f32, coefficient: f32) -> Result<f32, ProcessError> {
        if !input.is_finite() || !coefficient.is_finite() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            });
        }
        let output = coefficient * input + (1.0 - coefficient) * self.state;
        self.state = if output.abs() < f32::MIN_POSITIVE {
            0.0
        } else {
            output
        };
        if self.state.is_finite() {
            Ok(self.state)
        } else {
            Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            })
        }
    }

    fn reset(&mut self) {
        self.state = 0.0;
    }
}

struct DelayLine {
    buffer: Vec<f32>,
    read_position: usize,
    write_position: usize,
    write_offset: usize,
}

impl DelayLine {
    fn new(delay_frames: usize) -> Self {
        Self::with_extra_capacity(delay_frames, 0.0)
    }

    fn with_extra_capacity(delay_frames: usize, extra_frames: f32) -> Self {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let extra_frames = extra_frames.max(0.0).ceil() as usize;
        let capacity = delay_frames.max(1).saturating_add(extra_frames).max(1);
        Self {
            buffer: vec![0.0; capacity],
            read_position: 0,
            write_position: delay_frames % capacity,
            write_offset: delay_frames % capacity,
        }
    }

    fn read_current(&mut self) -> Result<f32, ProcessError> {
        let value = self.buffer[self.read_position];
        self.read_position = (self.read_position + 1) % self.buffer.len();
        if value.is_finite() {
            Ok(value)
        } else {
            Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            })
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    fn read_modulated(&mut self, offset: f32) -> Result<f32, ProcessError> {
        if !offset.is_finite() || offset.abs() >= self.buffer.len() as f32 {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            });
        }
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let position = self.read_position as f32 - offset;
        let length = self.buffer.len() as f32;
        let wrapped = position.rem_euclid(length);
        let index = wrapped.floor() as usize;
        let next = (index + 1) % self.buffer.len();
        let fraction = wrapped - index as f32;
        let value = self.buffer[index] + (self.buffer[next] - self.buffer[index]) * fraction;
        self.read_position = (self.read_position + 1) % self.buffer.len();
        if value.is_finite() {
            Ok(value)
        } else {
            Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            })
        }
    }

    fn write(&mut self, value: f32) -> Result<(), ProcessError> {
        if !value.is_finite() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            });
        }
        self.buffer[self.write_position] = value;
        self.write_position = (self.write_position + 1) % self.buffer.len();
        Ok(())
    }

    fn process(&mut self, value: f32) -> Result<f32, ProcessError> {
        let delayed = self.read_current()?;
        self.write(value)?;
        Ok(delayed)
    }

    fn tap(&self, delay: usize) -> Result<f32, ProcessError> {
        if delay >= self.buffer.len() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            });
        }
        let position = (self.read_position + self.buffer.len() - delay) % self.buffer.len();
        let value = self.buffer[position];
        if value.is_finite() {
            Ok(value)
        } else {
            Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            })
        }
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.read_position = 0;
        self.write_position = self.write_offset;
    }
}

fn span_position(index: usize, length: usize) -> f32 {
    if length == 0 {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        {
            index as f32 / length as f32
        }
    }
}

fn span_value(span: ValueSpan, position: f32) -> f32 {
    span.start + (span.end - span.start) * position
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{CompiledReverbProcessor, ReverbOutputTap, ReverbTapSource};
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
            left_output_taps: [ReverbOutputTap {
                source: ReverbTapSource::LeftLongDelay,
                delay_frames: 1,
                sign: 1,
            }; 7],
            right_output_taps: [ReverbOutputTap {
                source: ReverbTapSource::RightLongDelay,
                delay_frames: 1,
                sign: 1,
            }; 7],
            modulation_increment: 0.001,
            modulation_excursion: 1.0,
        }
    }

    fn constant(value: f32) -> ValueSpan {
        ValueSpan {
            start: value,
            end: value,
        }
    }

    #[test]
    fn impulse_produces_a_finite_tail() {
        let mut runtime = PlateReverbRuntime::new(&compiled());
        let mut left = vec![0.0; 256];
        let mut right = vec![0.0; 256];
        left[0] = 1.0;

        runtime
            .process(
                constant(0.7),
                constant(0.2),
                constant(1.0),
                constant(1.0),
                &mut left,
                &mut right,
            )
            .expect("reverb process");

        assert!(left.iter().chain(&right).all(|value| value.is_finite()));
        assert!(left.iter().skip(1).any(|value| value.abs() > 1.0e-5));
    }

    #[test]
    fn reset_removes_tail() {
        let mut runtime = PlateReverbRuntime::new(&compiled());
        let mut left = vec![1.0; 32];
        let mut right = vec![0.0; 32];
        runtime
            .process(
                constant(0.8),
                constant(0.2),
                constant(1.0),
                constant(1.0),
                &mut left,
                &mut right,
            )
            .expect("reverb process");
        runtime.reset();

        let mut reset_left = vec![0.0; 32];
        let mut reset_right = vec![0.0; 32];
        runtime
            .process(
                constant(0.8),
                constant(0.2),
                constant(1.0),
                constant(1.0),
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

    #[test]
    #[allow(clippy::too_many_lines)]
    fn ramp_is_independent_of_span_partitioning() {
        let mut whole_runtime = PlateReverbRuntime::new(&compiled());
        let mut whole_left = vec![0.0; 32];
        let mut whole_right = vec![0.0; 32];
        whole_left[0] = 1.0;
        whole_runtime
            .process(
                ValueSpan {
                    start: 0.2,
                    end: 0.9,
                },
                ValueSpan {
                    start: 0.1,
                    end: 0.8,
                },
                ValueSpan {
                    start: 0.3,
                    end: 1.0,
                },
                ValueSpan {
                    start: 0.2,
                    end: 0.7,
                },
                &mut whole_left,
                &mut whole_right,
            )
            .expect("whole reverb process");

        let mut split_runtime = PlateReverbRuntime::new(&compiled());
        let mut split_left = vec![0.0; 32];
        let mut split_right = vec![0.0; 32];
        split_left[0] = 1.0;
        split_runtime
            .process(
                ValueSpan {
                    start: 0.2,
                    end: 0.2 + 0.7 / 32.0,
                },
                ValueSpan {
                    start: 0.1,
                    end: 0.1 + 0.7 / 32.0,
                },
                ValueSpan {
                    start: 0.3,
                    end: 0.3 + 0.7 / 32.0,
                },
                ValueSpan {
                    start: 0.2,
                    end: 0.2 + 0.5 / 32.0,
                },
                &mut split_left[..1],
                &mut split_right[..1],
            )
            .expect("first split reverb process");
        split_runtime
            .process(
                ValueSpan {
                    start: 0.2 + 0.7 / 32.0,
                    end: 0.9,
                },
                ValueSpan {
                    start: 0.1 + 0.7 / 32.0,
                    end: 0.8,
                },
                ValueSpan {
                    start: 0.3 + 0.7 / 32.0,
                    end: 1.0,
                },
                ValueSpan {
                    start: 0.2 + 0.5 / 32.0,
                    end: 0.7,
                },
                &mut split_left[1..],
                &mut split_right[1..],
            )
            .expect("second split reverb process");

        assert!(
            whole_left
                .iter()
                .zip(&split_left)
                .chain(whole_right.iter().zip(&split_right))
                .all(|(left, right)| (left - right).abs() < 1.0e-6)
        );

        let mut whole_tail_left = vec![0.0; 64];
        let mut whole_tail_right = vec![0.0; 64];
        whole_runtime
            .process(
                constant(0.9),
                constant(0.8),
                constant(1.0),
                constant(0.7),
                &mut whole_tail_left,
                &mut whole_tail_right,
            )
            .expect("whole reverb tail process");
        let mut split_tail_left = vec![0.0; 64];
        let mut split_tail_right = vec![0.0; 64];
        split_runtime
            .process(
                constant(0.9),
                constant(0.8),
                constant(1.0),
                constant(0.7),
                &mut split_tail_left,
                &mut split_tail_right,
            )
            .expect("split reverb tail process");

        assert!(
            whole_tail_left
                .iter()
                .zip(&split_tail_left)
                .chain(whole_tail_right.iter().zip(&split_tail_right))
                .all(|(left, right)| (left - right).abs() < 1.0e-6)
        );
    }

    #[test]
    fn output_taps_read_their_declared_delay_lines() {
        let compiled = compiled();
        let mut left_tank = Tank::new(compiled.tank_left_lengths, 1.0);
        let mut right_tank = Tank::new(compiled.tank_right_lengths, 1.0);
        left_tank.long_delay.buffer[8] = 2.0;
        right_tank.long_delay.buffer[12] = 3.0;

        let left = sum_output_taps(&compiled.left_output_taps, &left_tank, &right_tank)
            .expect("left output taps");
        let right = sum_output_taps(&compiled.right_output_taps, &left_tank, &right_tank)
            .expect("right output taps");
        assert!((left - 8.4).abs() < 1.0e-6);
        assert!((right - 12.6).abs() < 1.0e-6);
    }

    #[test]
    fn modulation_offset_matches_reference_excursion() {
        assert!((modulation_offset(0.0, 16.0) - 8.0).abs() < 1.0e-6);
        assert!((modulation_offset(std::f32::consts::FRAC_PI_2, 16.0) - 16.0).abs() < 1.0e-6);
        assert!(modulation_offset(-std::f32::consts::FRAC_PI_2, 16.0).abs() < 1.0e-6);
    }
}
