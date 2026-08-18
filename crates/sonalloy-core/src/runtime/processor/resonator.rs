use crate::compiler::{CompiledResonatorProcessor, GeneratorOutputMode};
use crate::process::{ProcessError, ProcessorFailureKind};

use super::super::fractional_delay::FractionalDelayLine;
use super::ValueSpan;

pub(crate) struct ResonatorRuntime {
    sample_rate: f32,
    max_delay_frames: usize,
    left: ResonatorChannel,
    right: Option<ResonatorChannel>,
}

struct ResonatorChannel {
    delay: FractionalDelayLine,
    damping_state: f32,
}

impl ResonatorRuntime {
    pub(crate) fn new(
        compiled: &CompiledResonatorProcessor,
        output_mode: GeneratorOutputMode,
    ) -> Self {
        Self {
            sample_rate: compiled.sample_rate,
            max_delay_frames: compiled.max_delay_frames,
            left: ResonatorChannel::new(compiled.max_delay_frames),
            right: match output_mode {
                GeneratorOutputMode::Mono => None,
                GeneratorOutputMode::Stereo => {
                    Some(ResonatorChannel::new(compiled.max_delay_frames))
                }
            },
        }
    }

    pub(crate) fn process_mono(
        &mut self,
        frequency_hz: ValueSpan,
        decay_seconds: ValueSpan,
        damping: ValueSpan,
        mix: ValueSpan,
        buffer: &mut [f32],
    ) -> Result<(), ProcessError> {
        process_channel(
            &mut self.left,
            self.sample_rate,
            self.max_delay_frames,
            frequency_hz,
            decay_seconds,
            damping,
            mix,
            buffer,
        )
    }

    pub(crate) fn process_stereo(
        &mut self,
        frequency_hz: ValueSpan,
        decay_seconds: ValueSpan,
        damping: ValueSpan,
        mix: ValueSpan,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            });
        }
        process_channel(
            &mut self.left,
            self.sample_rate,
            self.max_delay_frames,
            frequency_hz,
            decay_seconds,
            damping,
            mix,
            left,
        )?;
        let right_channel = self.right.as_mut().ok_or(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::InvalidState,
        })?;
        process_channel(
            right_channel,
            self.sample_rate,
            self.max_delay_frames,
            frequency_hz,
            decay_seconds,
            damping,
            mix,
            right,
        )
    }

    pub(crate) fn reset(&mut self) {
        self.left.reset();
        if let Some(right) = &mut self.right {
            right.reset();
        }
    }
}

impl ResonatorChannel {
    fn new(max_delay_frames: usize) -> Self {
        Self {
            delay: FractionalDelayLine::new(max_delay_frames),
            damping_state: 0.0,
        }
    }

    fn reset(&mut self) {
        self.delay.reset();
        self.damping_state = 0.0;
    }
}

#[allow(clippy::too_many_arguments)]
fn process_channel(
    channel: &mut ResonatorChannel,
    sample_rate: f32,
    max_delay_frames: usize,
    frequency_hz: ValueSpan,
    decay_seconds: ValueSpan,
    damping: ValueSpan,
    mix: ValueSpan,
    buffer: &mut [f32],
) -> Result<(), ProcessError> {
    for index in 0..buffer.len() {
        let input = buffer[index];
        let frequency = frequency_hz.value_at(index, buffer.len());
        let decay = decay_seconds.value_at(index, buffer.len());
        let damping = damping.value_at(index, buffer.len());
        let mix = mix.value_at(index, buffer.len());
        if !input.is_finite()
            || !frequency.is_finite()
            || !decay.is_finite()
            || !damping.is_finite()
            || !mix.is_finite()
        {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            });
        }
        #[allow(clippy::cast_precision_loss)]
        let max_delay = max_delay_frames as f32 - 1.0;
        let delay_frames = (sample_rate / frequency).clamp(1.0, max_delay);
        let delayed = channel.delay.read(delay_frames)?;
        let max_cutoff = 18_000.0_f32.min(sample_rate * 0.45);
        let cutoff = 200.0 + (1.0 - damping).powi(2) * (max_cutoff - 200.0);
        let damping_coefficient = (-2.0 * std::f32::consts::PI * cutoff / sample_rate).exp();
        channel.damping_state =
            (1.0 - damping_coefficient) * delayed + damping_coefficient * channel.damping_state;
        let loop_period = 1.0 / frequency;
        let feedback = 10.0_f32.powf(-3.0 * loop_period / decay);
        channel
            .delay
            .write(input + channel.damping_state * feedback)?;
        let output = input * (1.0 - mix) + channel.damping_state * mix;
        if !output.is_finite() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            });
        }
        buffer[index] = output;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{CompiledResonatorParameters, CompiledResonatorProcessor};
    use crate::parameter::ParameterHandle;

    fn span(value: f32) -> ValueSpan {
        ValueSpan {
            start: value,
            end: value,
        }
    }

    fn runtime() -> ResonatorRuntime {
        ResonatorRuntime::new(
            &CompiledResonatorProcessor {
                parameters: CompiledResonatorParameters {
                    frequency_hz: ParameterHandle::new(0),
                    decay_seconds: ParameterHandle::new(1),
                    damping: ParameterHandle::new(2),
                    mix: ParameterHandle::new(3),
                },
                max_delay_frames: 48_000,
                sample_rate: 48_000.0,
            },
            GeneratorOutputMode::Mono,
        )
    }

    fn impulse_response(frequency_hz: f32, decay_seconds: f32) -> Vec<f32> {
        let mut runtime = runtime();
        let mut buffer = vec![0.0; 4_096];
        buffer[0] = 1.0;
        runtime
            .process_mono(
                span(frequency_hz),
                span(decay_seconds),
                span(0.35),
                span(1.0),
                &mut buffer,
            )
            .expect("resonator processes");
        buffer
    }

    #[test]
    fn mix_zero_is_an_identity() {
        let mut runtime = runtime();
        let original = [0.1, -0.8, 0.4, 0.0, 0.7, -0.2];
        let mut buffer = original;
        runtime
            .process_mono(span(440.0), span(0.5), span(0.35), span(0.0), &mut buffer)
            .expect("resonator processes");
        assert!(
            buffer
                .into_iter()
                .zip(original)
                .all(|(actual, expected)| (actual - expected).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn frequency_and_decay_change_the_impulse_response() {
        let low = impulse_response(220.0, 0.55);
        let high = impulse_response(440.0, 0.55);
        let short = impulse_response(330.0, 0.05);
        let long = impulse_response(330.0, 1.0);
        assert!(
            low.iter()
                .zip(&high)
                .any(|(left, right)| (left - right).abs() > 1.0e-4)
        );
        let short_tail = short[1_024..]
            .iter()
            .map(|sample| sample * sample)
            .sum::<f32>();
        let long_tail = long[1_024..]
            .iter()
            .map(|sample| sample * sample)
            .sum::<f32>();
        assert!(long_tail > short_tail * 2.0);
    }
}
