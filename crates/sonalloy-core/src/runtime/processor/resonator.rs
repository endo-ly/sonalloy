use crate::compiler::{CompiledResonatorProcessor, GeneratorOutputMode};
use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;
use super::fractional_delay::FractionalDelayLine;

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
