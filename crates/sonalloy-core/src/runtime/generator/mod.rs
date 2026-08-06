mod noise;
mod oscillator;

use crate::compiler::CompiledGenerator;
use crate::generator_parameters::GeneratorParameterSpec;
use crate::process::{NoteId, ProcessError, ProcessSpec, ProcessorFailureKind};

use super::modulation::{LayerGeneratorTargetSpan, ValueSpan};
use super::sample::{SampleRuntime, playback_ratio};

use noise::NoiseRuntime;
use oscillator::OscillatorRuntime;

fn validate_generator_span(
    span: ValueSpan,
    spec: GeneratorParameterSpec,
) -> Result<(), ProcessError> {
    if !span.start.is_finite() || !span.end.is_finite() {
        return Err(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::NonFinite,
        });
    }
    if !(spec.min..=spec.max).contains(&span.start) || !(spec.min..=spec.max).contains(&span.end) {
        return Err(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::InvalidInput,
        });
    }
    Ok(())
}

pub(super) enum GeneratorRuntime {
    Oscillator(OscillatorRuntime),
    Noise(Box<NoiseRuntime>),
    Sample { sample: SampleRuntime },
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
            CompiledGenerator::Sample(_) => Ok(Self::Sample {
                sample: SampleRuntime::new(),
            }),
        }
    }

    pub(crate) fn start(
        &mut self,
        note_id: NoteId,
        sample_zone: Option<usize>,
        compiled: &CompiledGenerator,
    ) -> Result<(), ProcessError> {
        match self {
            Self::Oscillator(oscillator) => oscillator.start(),
            Self::Noise(noise) => {
                noise.start(note_id);
                Ok(())
            }
            Self::Sample { sample } => {
                let CompiledGenerator::Sample(compiled) = compiled else {
                    return Err(ProcessError::ProcessorFailure {
                        kind: crate::process::ProcessorFailureKind::InvalidState,
                    });
                };
                let selected_index = sample_zone.and_then(|index| {
                    compiled
                        .zones
                        .get(index)
                        .filter(|zone| zone.is_enabled())
                        .map(|_| index)
                });
                let zone = selected_index.and_then(|index| compiled.zones.get(index));
                sample.start(zone);
                Ok(())
            }
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
        if mono.len() < frames || left.len() < frames || right.len() < frames {
            return Err(ProcessError::ProcessorFailure {
                kind: crate::process::ProcessorFailureKind::InvalidState,
            });
        }
        match self {
            Self::Oscillator(oscillator) => {
                oscillator.render(
                    frames,
                    note_number,
                    tuning_start,
                    tuning_end,
                    sample_rate,
                    targets,
                    mono,
                    left,
                    right,
                )?;
                Ok(false)
            }
            Self::Noise(noise) => {
                let LayerGeneratorTargetSpan::Noise { correlation } = targets else {
                    return Err(ProcessError::ProcessorFailure {
                        kind: crate::process::ProcessorFailureKind::InvalidState,
                    });
                };
                noise.render(frames, correlation, left, right)?;
                Ok(false)
            }
            Self::Sample { sample } => {
                if !matches!(targets, LayerGeneratorTargetSpan::Sample) {
                    return Err(ProcessError::ProcessorFailure {
                        kind: crate::process::ProcessorFailureKind::InvalidState,
                    });
                }
                let start_ratio = playback_ratio(
                    note_number,
                    sample.root_note(),
                    crate::compiler::cents_to_ratio(tuning_start),
                );
                let end_ratio = playback_ratio(
                    note_number,
                    sample.root_note(),
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
        }
    }
}
