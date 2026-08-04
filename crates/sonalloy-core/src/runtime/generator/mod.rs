mod noise;
mod oscillator;

use crate::compiler::{CompiledGenerator, GeneratorOutputMode};
use crate::process::{NoteId, ProcessError, ProcessSpec};

use super::modulation::LayerGeneratorTargetSpan;
use super::sample::{SampleRuntime, playback_ratio};

use noise::NoiseRuntime;
use oscillator::OscillatorRuntime;

pub(super) enum GeneratorRuntime {
    Oscillator(OscillatorRuntime),
    Noise(Box<NoiseRuntime>),
    Sample {
        sample: SampleRuntime,
        root_note: u8,
    },
    Disabled,
}

impl GeneratorRuntime {
    pub(crate) fn new(
        compiled: &CompiledGenerator,
        spec: ProcessSpec,
    ) -> Result<Self, ProcessError> {
        match compiled {
            CompiledGenerator::Oscillator(value) => {
                Ok(Self::Oscillator(OscillatorRuntime::new(value, spec)?))
            }
            CompiledGenerator::Noise(value) => Ok(Self::Noise(Box::new(NoiseRuntime::new(value)))),
            CompiledGenerator::Sample(value) => value
                .source
                .as_ref()
                .filter(|_| value.enabled)
                .map_or(Ok(Self::Disabled), |source| {
                    Ok(Self::Sample {
                        sample: SampleRuntime::new(source),
                        root_note: value.root_note,
                    })
                }),
        }
    }

    pub(crate) fn output_mode(&self) -> GeneratorOutputMode {
        match self {
            Self::Oscillator(_) | Self::Sample { .. } | Self::Disabled => GeneratorOutputMode::Mono,
            Self::Noise(_) => GeneratorOutputMode::Stereo,
        }
    }

    pub(crate) fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }

    pub(crate) fn start(&mut self, note_id: NoteId) -> Result<(), ProcessError> {
        match self {
            Self::Oscillator(oscillator) => oscillator.start(),
            Self::Noise(noise) => {
                noise.start(note_id);
                Ok(())
            }
            Self::Sample { sample, .. } => {
                sample.start();
                Ok(())
            }
            Self::Disabled => Ok(()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render(
        &mut self,
        frames: usize,
        note_number: u8,
        tuning_start: f32,
        tuning_end: f32,
        sample_rate: f64,
        targets: LayerGeneratorTargetSpan,
        mono: &mut [f32],
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<bool, ProcessError> {
        match self {
            Self::Oscillator(oscillator) => {
                oscillator.render(
                    frames,
                    note_number,
                    tuning_start,
                    tuning_end,
                    sample_rate,
                    targets.pulse_width,
                    mono,
                )?;
                Ok(false)
            }
            Self::Noise(noise) => {
                noise.render(frames, targets.noise_correlation, left, right)?;
                Ok(false)
            }
            Self::Sample { sample, root_note } => {
                let start_ratio = playback_ratio(
                    note_number,
                    *root_note,
                    crate::compiler::cents_to_ratio(tuning_start),
                );
                let end_ratio = playback_ratio(
                    note_number,
                    *root_note,
                    crate::compiler::cents_to_ratio(tuning_end),
                );
                if !start_ratio.is_finite()
                    || !end_ratio.is_finite()
                    || start_ratio <= 0.0
                    || end_ratio <= 0.0
                {
                    return Err(ProcessError::InvalidFrequency);
                }
                if frames == 0 {
                    return Ok(sample.is_finished());
                }
                if start_ratio.total_cmp(&end_ratio).is_eq() {
                    for value in &mut mono[..frames] {
                        *value = sample.next_sample_with_ratio(start_ratio);
                    }
                } else {
                    #[allow(clippy::cast_precision_loss)]
                    let ratio_step = (end_ratio / start_ratio).powf(1.0 / frames as f64);
                    let mut ratio = start_ratio;
                    for value in &mut mono[..frames] {
                        *value = sample.next_sample_with_ratio(ratio);
                        ratio *= ratio_step;
                    }
                }
                Ok(sample.is_finished())
            }
            Self::Disabled => {
                mono[..frames].fill(0.0);
                left[..frames].fill(0.0);
                right[..frames].fill(0.0);
                Ok(true)
            }
        }
    }

    pub(crate) fn reset(&mut self) -> Result<(), ProcessError> {
        match self {
            Self::Oscillator(oscillator) => oscillator.reset(),
            Self::Noise(noise) => {
                noise.reset();
                Ok(())
            }
            Self::Sample { sample, .. } => {
                sample.reset();
                Ok(())
            }
            Self::Disabled => Ok(()),
        }
    }
}
