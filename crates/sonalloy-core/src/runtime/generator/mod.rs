mod additive;
mod formant;
mod granular;
mod modal;
mod noise;
mod operator;
mod oscillator;
pub(crate) mod partial_bank;
mod physical_exciter;
mod physical_string;
mod spectral;
mod wave_sequence;
mod wavetable;

use crate::compiler::CompiledGenerator;
use crate::generator_parameters::GeneratorParameterSpec;
use crate::process::{NoteId, ProcessError, ProcessSpec, ProcessorFailureKind};

use super::modulation::{LayerGeneratorTargetSpan, ValueSpan};
use super::sample::{SampleRuntime, playback_ratio};

use additive::AdditiveRuntime;
use formant::FormantRuntime;
use granular::GranularRuntime;
use modal::ModalRuntime;
use noise::NoiseRuntime;
use operator::OperatorModulationRuntime;
use oscillator::OscillatorRuntime;
use physical_string::PhysicalStringRuntime;
use spectral::SpectralRuntime;
use wave_sequence::WaveSequenceRuntime;
use wavetable::WavetableRuntime;

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

pub(super) fn base_frequencies(
    note_number: u8,
    tuning_start: f32,
    tuning_end: f32,
) -> Result<(f32, f32), ProcessError> {
    let start = crate::compiler::midi_note_frequency(
        note_number,
        crate::compiler::cents_to_ratio(tuning_start),
    );
    let end = crate::compiler::midi_note_frequency(
        note_number,
        crate::compiler::cents_to_ratio(tuning_end),
    );
    if start.is_finite() && end.is_finite() && start > 0.0 && end > 0.0 {
        Ok((start, end))
    } else {
        Err(ProcessError::InvalidFrequency)
    }
}

pub(super) fn ensure_finite(samples: &[f32]) -> Result<(), ProcessError> {
    if samples.iter().all(|sample| sample.is_finite()) {
        Ok(())
    } else {
        Err(non_finite())
    }
}

pub(super) fn initial_phase(base: f32, offset: f32) -> f32 {
    (base + offset).rem_euclid(1.0)
}

pub(super) fn invalid_state() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::InvalidState,
    }
}

pub(super) fn non_finite() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::NonFinite,
    }
}

pub(super) enum GeneratorRuntime {
    Oscillator(OscillatorRuntime),
    Noise(Box<NoiseRuntime>),
    PhysicalString(Box<PhysicalStringRuntime>),
    Modal(Box<ModalRuntime>),
    Additive(Box<AdditiveRuntime>),
    Formant(Box<FormantRuntime>),
    Sample { sample: SampleRuntime },
    Granular(Box<GranularRuntime>),
    WaveSequence(WaveSequenceRuntime),
    Wavetable(WavetableRuntime),
    Spectral(Box<SpectralRuntime>),
    OperatorModulation(Box<OperatorModulationRuntime>),
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
            CompiledGenerator::PhysicalString(value) => Ok(Self::PhysicalString(Box::new(
                PhysicalStringRuntime::new(value, spec)?,
            ))),
            CompiledGenerator::Modal(value) => {
                Ok(Self::Modal(Box::new(ModalRuntime::new(value, spec)?)))
            }
            CompiledGenerator::Additive(value) => {
                Ok(Self::Additive(Box::new(AdditiveRuntime::new(value, spec)?)))
            }
            CompiledGenerator::Formant(value) => {
                Ok(Self::Formant(Box::new(FormantRuntime::new(value, spec)?)))
            }
            CompiledGenerator::Sample(compiled) => Ok(Self::Sample {
                sample: SampleRuntime::prepared(compiled, spec)?,
            }),
            CompiledGenerator::Granular(compiled) => {
                Ok(Self::Granular(Box::new(GranularRuntime::new(compiled)?)))
            }
            CompiledGenerator::WaveSequence(compiled) => {
                Ok(Self::WaveSequence(WaveSequenceRuntime::new(compiled)?))
            }
            CompiledGenerator::Wavetable(value) => {
                Ok(Self::Wavetable(WavetableRuntime::new(value, spec)?))
            }
            CompiledGenerator::Spectral(value) => {
                Ok(Self::Spectral(Box::new(SpectralRuntime::new(value, spec)?)))
            }
            CompiledGenerator::OperatorModulation(value) => Ok(Self::OperatorModulation(Box::new(
                OperatorModulationRuntime::new(value, spec)?,
            ))),
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
            Self::PhysicalString(string) => {
                string.start(note_id);
                Ok(())
            }
            Self::Modal(modal) => modal.start(note_id),
            Self::Additive(additive) => {
                additive.start();
                Ok(())
            }
            Self::Formant(formant) => {
                formant.start();
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
                sample.start(zone)
            }
            Self::Granular(granular) => granular.start(note_id),
            Self::WaveSequence(sequence) => sequence.start(note_id),
            Self::Wavetable(wavetable) => wavetable.start(),
            Self::Spectral(spectral) => spectral.start(),
            Self::OperatorModulation(operator) => {
                operator.start();
                Ok(())
            }
        }
    }

    pub(crate) fn note_off(&mut self) {
        match self {
            Self::Additive(additive) => additive.note_off(),
            Self::OperatorModulation(operator) => operator.note_off(),
            _ => {}
        }
    }

    pub(crate) fn intrinsic_latency_frames(&self) -> usize {
        match self {
            Self::Sample { sample } => sample.intrinsic_latency_frames(),
            Self::Spectral(spectral) => spectral.intrinsic_latency_frames(),
            Self::Oscillator(_)
            | Self::Noise(_)
            | Self::PhysicalString(_)
            | Self::Modal(_)
            | Self::Additive(_)
            | Self::Formant(_)
            | Self::Granular(_)
            | Self::WaveSequence(_)
            | Self::Wavetable(_)
            | Self::OperatorModulation(_) => 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_lines)]
    pub(crate) fn render(
        &mut self,
        frames: usize,
        note_number: u8,
        tuning_start: f32,
        tuning_end: f32,
        sample_rate: f64,
        tempo_bpm: f64,
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
            Self::PhysicalString(string) => {
                string.render(
                    frames,
                    note_number,
                    tuning_start,
                    tuning_end,
                    sample_rate,
                    targets,
                    mono,
                )?;
                Ok(false)
            }
            Self::Modal(modal) => {
                modal.render(
                    frames,
                    note_number,
                    tuning_start,
                    tuning_end,
                    sample_rate,
                    targets,
                    mono,
                )?;
                Ok(false)
            }
            Self::Additive(additive) => {
                additive.render(
                    frames,
                    note_number,
                    tuning_start,
                    tuning_end,
                    sample_rate,
                    targets,
                    mono,
                )?;
                Ok(false)
            }
            Self::Formant(formant) => {
                formant.render(
                    frames,
                    note_number,
                    tuning_start,
                    tuning_end,
                    sample_rate,
                    targets,
                    mono,
                )?;
                Ok(false)
            }
            Self::Sample { sample } => Self::render_sample(
                sample,
                frames,
                note_number,
                tuning_start,
                tuning_end,
                targets,
                tempo_bpm,
                mono,
                left,
                right,
            ),
            Self::Granular(granular) => {
                let LayerGeneratorTargetSpan::Granular { .. } = targets else {
                    return Err(ProcessError::ProcessorFailure {
                        kind: crate::process::ProcessorFailureKind::InvalidState,
                    });
                };
                granular.render(
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
            Self::WaveSequence(sequence) => {
                let LayerGeneratorTargetSpan::WaveSequence = targets else {
                    return Err(ProcessError::ProcessorFailure {
                        kind: crate::process::ProcessorFailureKind::InvalidState,
                    });
                };
                sequence.render(
                    frames,
                    note_number,
                    tuning_start,
                    tuning_end,
                    sample_rate,
                    tempo_bpm,
                    mono,
                    left,
                    right,
                )
            }
            Self::Wavetable(wavetable) => {
                wavetable.render(
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
            Self::Spectral(spectral) => spectral.render(
                frames,
                note_number,
                tuning_start,
                tuning_end,
                sample_rate,
                targets,
                mono,
                left,
                right,
            ),
            Self::OperatorModulation(operator) => {
                operator.render(
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
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_sample(
        sample: &mut SampleRuntime,
        frames: usize,
        note_number: u8,
        tuning_start: f32,
        tuning_end: f32,
        targets: LayerGeneratorTargetSpan,
        tempo_bpm: f64,
        mono: &mut [f32],
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<bool, ProcessError> {
        if !matches!(targets, LayerGeneratorTargetSpan::Sample) {
            return Err(ProcessError::ProcessorFailure {
                kind: crate::process::ProcessorFailureKind::InvalidState,
            });
        }
        if sample.uses_stretch() {
            return sample.render_stretched(
                frames,
                note_number,
                tuning_start,
                tuning_end,
                tempo_bpm,
                mono,
                left,
                right,
            );
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
            for ((mono, left), right) in mono[..frames]
                .iter_mut()
                .zip(&mut left[..frames])
                .zip(&mut right[..frames])
            {
                let (sample_left, sample_right) = sample.next_frame_with_ratio(start_ratio);
                *mono = f32::midpoint(sample_left, sample_right);
                *left = sample_left;
                *right = sample_right;
            }
        } else {
            #[allow(clippy::cast_precision_loss)]
            let ratio_step = (end_ratio / start_ratio).powf(1.0 / frames as f64);
            let mut ratio = start_ratio;
            for ((mono, left), right) in mono[..frames]
                .iter_mut()
                .zip(&mut left[..frames])
                .zip(&mut right[..frames])
            {
                let (sample_left, sample_right) = sample.next_frame_with_ratio(ratio);
                *mono = f32::midpoint(sample_left, sample_right);
                *left = sample_left;
                *right = sample_right;
                ratio *= ratio_step;
            }
        }
        Ok(sample.is_finished())
    }

    pub(crate) fn reset(&mut self) -> Result<(), ProcessError> {
        match self {
            Self::Oscillator(oscillator) => oscillator.reset(),
            Self::Noise(noise) => {
                noise.reset();
                Ok(())
            }
            Self::PhysicalString(string) => {
                string.reset();
                Ok(())
            }
            Self::Modal(modal) => modal.reset(),
            Self::Additive(additive) => {
                additive.reset();
                Ok(())
            }
            Self::Formant(formant) => {
                formant.reset();
                Ok(())
            }
            Self::Sample { sample, .. } => sample.reset(),
            Self::Granular(granular) => {
                granular.reset();
                Ok(())
            }
            Self::WaveSequence(sequence) => {
                sequence.reset();
                Ok(())
            }
            Self::Wavetable(wavetable) => {
                wavetable.reset();
                Ok(())
            }
            Self::Spectral(spectral) => {
                spectral.reset();
                Ok(())
            }
            Self::OperatorModulation(operator) => {
                operator.reset();
                Ok(())
            }
        }
    }
}
