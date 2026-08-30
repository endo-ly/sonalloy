mod biquad;
mod bitcrusher;
mod chorus;
mod compressor;
mod convolution;
mod delay;
mod drive;
mod envelope_transfer;
mod eq;
mod flanger;
mod formant;
mod frequency_shifter;
mod gate;
mod ladder;
mod limiter;
mod phaser;
mod resonator;
mod reverb;
mod spectral_morph;
mod transient_shaper;
mod vocoder;

use sonalloy_dsp_sys::{DspFilter, DspFilterMode};

use crate::compiler::{CompiledProcessor, CompiledProcessorKind, GeneratorOutputMode};
use crate::definition::FilterModeDefinition;
use crate::process::{ProcessError, ProcessSpec, ProcessorFailureKind};
use crate::runtime::external_audio::ExternalAudioBlock;

use super::modulation::ValueSpan;

pub(crate) use bitcrusher::BitcrusherRuntime;
pub(crate) use chorus::ChorusRuntime;
pub(crate) use compressor::CompressorRuntime;
pub(crate) use convolution::ConvolutionRuntime;
pub(crate) use delay::StereoDelayRuntime;
pub(crate) use drive::DriveRuntime;
pub(crate) use envelope_transfer::EnvelopeTransferRuntime;
pub(crate) use eq::EqRuntime;
pub(crate) use flanger::FlangerRuntime;
pub(crate) use formant::FormantProcessorRuntime;
pub(crate) use frequency_shifter::FrequencyShifterRuntime;
pub(crate) use gate::GateRuntime;
pub(crate) use ladder::LadderFilterRuntime;
pub(crate) use limiter::LimiterRuntime;
pub(crate) use phaser::PhaserRuntime;
pub(crate) use resonator::ResonatorRuntime;
pub(crate) use reverb::PlateReverbRuntime;
pub(crate) use spectral_morph::SpectralMorphRuntime;
pub(crate) use transient_shaper::TransientShaperRuntime;
pub(crate) use vocoder::VocoderRuntime;

pub(crate) fn spectral_morph_runtime_buffer_bytes(alignment_frames: usize) -> usize {
    spectral_morph::SpectralMorphRuntime::runtime_buffer_bytes_for_alignment(alignment_frames)
}

/// Runtime values corresponding to one compiled processor in a chain.
#[derive(Clone, Copy)]
pub(crate) enum ProcessorTargetSpan {
    Filter {
        cutoff: ValueSpan,
        resonance: ValueSpan,
    },
    LadderFilter {
        cutoff: ValueSpan,
        resonance: ValueSpan,
        drive: ValueSpan,
    },
    Drive {
        amount: ValueSpan,
        mix: ValueSpan,
    },
    Eq {
        low_gain_db: ValueSpan,
        mid_gain_db: ValueSpan,
        high_gain_db: ValueSpan,
    },
    Formant {
        vowel_position: ValueSpan,
        formant_shift: ValueSpan,
        throat: ValueSpan,
        mix: ValueSpan,
    },
    Resonator {
        frequency_hz: ValueSpan,
        decay_seconds: ValueSpan,
        damping: ValueSpan,
        mix: ValueSpan,
    },
    Bitcrusher {
        bit_depth: ValueSpan,
        sample_rate_ratio: ValueSpan,
        mix: ValueSpan,
    },
    Chorus {
        rate_hz: ValueSpan,
        depth: ValueSpan,
        feedback: ValueSpan,
        width: ValueSpan,
        mix: ValueSpan,
    },
    Flanger {
        rate_hz: ValueSpan,
        depth: ValueSpan,
        feedback: ValueSpan,
        width: ValueSpan,
        mix: ValueSpan,
    },
    Phaser {
        rate_hz: ValueSpan,
        depth: ValueSpan,
        feedback: ValueSpan,
        width: ValueSpan,
        mix: ValueSpan,
    },
    FrequencyShifter {
        shift_hz: ValueSpan,
        mix: ValueSpan,
    },
    Delay {
        feedback: ValueSpan,
        mix: ValueSpan,
    },
    Reverb {
        decay: ValueSpan,
        damping: ValueSpan,
        width: ValueSpan,
        mix: ValueSpan,
    },
    Convolution {
        gain_db: ValueSpan,
        mix: ValueSpan,
    },
    Gate {
        threshold_db: ValueSpan,
        range_db: ValueSpan,
    },
    Vocoder {
        modulator_gain_db: ValueSpan,
        output_gain_db: ValueSpan,
        mix: ValueSpan,
    },
    EnvelopeTransfer {
        input_gain_db: ValueSpan,
        floor_db: ValueSpan,
        mix: ValueSpan,
    },
    SpectralMorph {
        morph: ValueSpan,
        output_gain_db: ValueSpan,
    },
    TransientShaper {
        attack: ValueSpan,
        sustain: ValueSpan,
        mix: ValueSpan,
    },
    Compressor {
        threshold_db: ValueSpan,
        ratio: ValueSpan,
        makeup_gain_db: ValueSpan,
        mix: ValueSpan,
    },
    Limiter {
        ceiling_db: ValueSpan,
        input_gain_db: ValueSpan,
    },
}

pub(crate) struct LayerProcessorChain {
    processors: Vec<LayerProcessorRuntime>,
}

enum LayerProcessorRuntime {
    Filter {
        left: DspFilter,
        right: Option<DspFilter>,
        mode: DspFilterMode,
    },
    LadderFilter(LadderFilterRuntime),
    Drive,
    Eq(EqRuntime),
    Formant(FormantProcessorRuntime),
    Resonator(ResonatorRuntime),
    Bitcrusher(BitcrusherRuntime),
}

impl LayerProcessorChain {
    pub(crate) fn new(
        processors: &[CompiledProcessor],
        spec: ProcessSpec,
        output_mode: GeneratorOutputMode,
    ) -> Result<Self, ProcessError> {
        let mut runtime = Vec::with_capacity(processors.len());
        for processor in processors {
            match &processor.processor {
                CompiledProcessorKind::Filter(value) => {
                    runtime.push(LayerProcessorRuntime::Filter {
                        left: prepare_filter(spec)?,
                        right: match output_mode {
                            GeneratorOutputMode::Mono => None,
                            GeneratorOutputMode::Stereo => Some(prepare_filter(spec)?),
                        },
                        mode: filter_mode(value.mode),
                    });
                }
                CompiledProcessorKind::LadderFilter(_) => {
                    #[allow(clippy::cast_possible_truncation)]
                    let sample_rate = spec.sample_rate as f32;
                    runtime.push(LayerProcessorRuntime::LadderFilter(
                        LadderFilterRuntime::new(sample_rate),
                    ));
                }
                CompiledProcessorKind::Drive(_) => {
                    runtime.push(LayerProcessorRuntime::Drive);
                }
                CompiledProcessorKind::Eq(value) => {
                    #[allow(clippy::cast_possible_truncation)]
                    let sample_rate = spec.sample_rate as f32;
                    runtime.push(LayerProcessorRuntime::Eq(EqRuntime::new(
                        value,
                        sample_rate,
                        output_mode,
                    )?));
                }
                CompiledProcessorKind::Resonator(value) => {
                    runtime.push(LayerProcessorRuntime::Resonator(ResonatorRuntime::new(
                        value,
                        output_mode,
                    )));
                }
                CompiledProcessorKind::Formant(value) => {
                    #[allow(clippy::cast_possible_truncation)]
                    let sample_rate = spec.sample_rate as f32;
                    runtime.push(LayerProcessorRuntime::Formant(
                        FormantProcessorRuntime::new(&value.profiles, sample_rate)?,
                    ));
                }
                CompiledProcessorKind::Bitcrusher(_) => {
                    runtime.push(LayerProcessorRuntime::Bitcrusher(BitcrusherRuntime::new(
                        output_mode,
                    )));
                }
                CompiledProcessorKind::Chorus(_)
                | CompiledProcessorKind::Flanger(_)
                | CompiledProcessorKind::Phaser(_)
                | CompiledProcessorKind::Delay(_)
                | CompiledProcessorKind::Reverb(_)
                | CompiledProcessorKind::Compressor(_)
                | CompiledProcessorKind::Limiter(_)
                | CompiledProcessorKind::FrequencyShifter(_)
                | CompiledProcessorKind::Convolution(_)
                | CompiledProcessorKind::Gate(_)
                | CompiledProcessorKind::Vocoder(_)
                | CompiledProcessorKind::EnvelopeTransfer(_)
                | CompiledProcessorKind::SpectralMorph(_)
                | CompiledProcessorKind::TransientShaper(_) => {
                    return Err(ProcessError::ProcessorFailure {
                        kind: ProcessorFailureKind::InvalidState,
                    });
                }
            }
        }
        Ok(Self {
            processors: runtime,
        })
    }

    pub(crate) fn process_mono(
        &mut self,
        targets: &[ProcessorTargetSpan],
        buffer: &mut [f32],
    ) -> Result<(), ProcessError> {
        if self.processors.len() != targets.len() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            });
        }
        for (processor, target) in self.processors.iter_mut().zip(targets) {
            match (processor, *target) {
                (
                    LayerProcessorRuntime::Filter {
                        left: filter, mode, ..
                    },
                    ProcessorTargetSpan::Filter { cutoff, resonance },
                ) => process_filter(filter, *mode, cutoff, resonance, buffer)?,
                (
                    LayerProcessorRuntime::LadderFilter(runtime),
                    ProcessorTargetSpan::LadderFilter {
                        cutoff,
                        resonance,
                        drive,
                    },
                ) => runtime.process_mono(cutoff, resonance, drive, buffer)?,
                (LayerProcessorRuntime::Drive, ProcessorTargetSpan::Drive { amount, mix }) => {
                    DriveRuntime::process_mono(amount, mix, buffer)?;
                }
                (
                    LayerProcessorRuntime::Eq(runtime),
                    ProcessorTargetSpan::Eq {
                        low_gain_db,
                        mid_gain_db,
                        high_gain_db,
                    },
                ) => runtime.process_mono(low_gain_db, mid_gain_db, high_gain_db, buffer)?,
                (
                    LayerProcessorRuntime::Formant(runtime),
                    ProcessorTargetSpan::Formant {
                        vowel_position,
                        formant_shift,
                        throat,
                        mix,
                    },
                ) => runtime.process_mono(vowel_position, formant_shift, throat, mix, buffer)?,
                (
                    LayerProcessorRuntime::Resonator(runtime),
                    ProcessorTargetSpan::Resonator {
                        frequency_hz,
                        decay_seconds,
                        damping,
                        mix,
                    },
                ) => runtime.process_mono(frequency_hz, decay_seconds, damping, mix, buffer)?,
                (
                    LayerProcessorRuntime::Bitcrusher(runtime),
                    ProcessorTargetSpan::Bitcrusher {
                        bit_depth,
                        sample_rate_ratio,
                        mix,
                    },
                ) => runtime.process_mono(bit_depth, sample_rate_ratio, mix, buffer)?,
                _ => {
                    return Err(ProcessError::ProcessorFailure {
                        kind: ProcessorFailureKind::InvalidState,
                    });
                }
            }
        }
        Ok(())
    }

    pub(crate) fn process_stereo(
        &mut self,
        targets: &[ProcessorTargetSpan],
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if self.processors.len() != targets.len() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            });
        }
        for (processor, target) in self.processors.iter_mut().zip(targets) {
            match (processor, *target) {
                (
                    LayerProcessorRuntime::Filter {
                        left: filter_left,
                        right: Some(filter_right),
                        mode,
                    },
                    ProcessorTargetSpan::Filter { cutoff, resonance },
                ) => {
                    process_filter(filter_left, *mode, cutoff, resonance, left)?;
                    process_filter(filter_right, *mode, cutoff, resonance, right)?;
                }
                (
                    LayerProcessorRuntime::LadderFilter(runtime),
                    ProcessorTargetSpan::LadderFilter {
                        cutoff,
                        resonance,
                        drive,
                    },
                ) => runtime.process_stereo(cutoff, resonance, drive, left, right)?,
                (LayerProcessorRuntime::Drive, ProcessorTargetSpan::Drive { amount, mix }) => {
                    DriveRuntime::process_stereo(amount, mix, left, right)?;
                }
                (
                    LayerProcessorRuntime::Eq(runtime),
                    ProcessorTargetSpan::Eq {
                        low_gain_db,
                        mid_gain_db,
                        high_gain_db,
                    },
                ) => runtime.process_stereo(low_gain_db, mid_gain_db, high_gain_db, left, right)?,
                (
                    LayerProcessorRuntime::Formant(runtime),
                    ProcessorTargetSpan::Formant {
                        vowel_position,
                        formant_shift,
                        throat,
                        mix,
                    },
                ) => runtime.process_stereo(
                    vowel_position,
                    formant_shift,
                    throat,
                    mix,
                    left,
                    right,
                )?,
                (
                    LayerProcessorRuntime::Resonator(runtime),
                    ProcessorTargetSpan::Resonator {
                        frequency_hz,
                        decay_seconds,
                        damping,
                        mix,
                    },
                ) => runtime.process_stereo(
                    frequency_hz,
                    decay_seconds,
                    damping,
                    mix,
                    left,
                    right,
                )?,
                (
                    LayerProcessorRuntime::Bitcrusher(runtime),
                    ProcessorTargetSpan::Bitcrusher {
                        bit_depth,
                        sample_rate_ratio,
                        mix,
                    },
                ) => runtime.process_stereo(bit_depth, sample_rate_ratio, mix, left, right)?,
                _ => {
                    return Err(ProcessError::ProcessorFailure {
                        kind: ProcessorFailureKind::InvalidState,
                    });
                }
            }
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) -> Result<(), ProcessError> {
        for processor in &mut self.processors {
            match processor {
                LayerProcessorRuntime::Filter { left, right, .. } => {
                    left.reset().map_err(ProcessError::from_filter_error)?;
                    if let Some(right) = right {
                        right.reset().map_err(ProcessError::from_filter_error)?;
                    }
                }
                LayerProcessorRuntime::LadderFilter(runtime) => runtime.reset(),
                LayerProcessorRuntime::Drive => {}
                LayerProcessorRuntime::Eq(runtime) => runtime.reset(),
                LayerProcessorRuntime::Formant(runtime) => runtime.reset(),
                LayerProcessorRuntime::Resonator(runtime) => runtime.reset(),
                LayerProcessorRuntime::Bitcrusher(runtime) => runtime.reset(),
            }
        }
        Ok(())
    }
}

impl ProcessorTargetSpan {
    #[allow(clippy::too_many_lines)]
    pub(crate) fn zero_for(processor: &CompiledProcessorKind) -> Self {
        let zero = ValueSpan {
            start: 0.0,
            end: 0.0,
        };
        match processor {
            CompiledProcessorKind::Filter(_) => Self::Filter {
                cutoff: zero,
                resonance: zero,
            },
            CompiledProcessorKind::LadderFilter(_) => Self::LadderFilter {
                cutoff: zero,
                resonance: zero,
                drive: zero,
            },
            CompiledProcessorKind::Drive(_) => Self::Drive {
                amount: zero,
                mix: zero,
            },
            CompiledProcessorKind::Eq(_) => Self::Eq {
                low_gain_db: zero,
                mid_gain_db: zero,
                high_gain_db: zero,
            },
            CompiledProcessorKind::Formant(_) => Self::Formant {
                vowel_position: zero,
                formant_shift: zero,
                throat: zero,
                mix: zero,
            },
            CompiledProcessorKind::Resonator(_) => Self::Resonator {
                frequency_hz: zero,
                decay_seconds: zero,
                damping: zero,
                mix: zero,
            },
            CompiledProcessorKind::Bitcrusher(_) => Self::Bitcrusher {
                bit_depth: zero,
                sample_rate_ratio: zero,
                mix: zero,
            },
            CompiledProcessorKind::Chorus(_) => Self::Chorus {
                rate_hz: zero,
                depth: zero,
                feedback: zero,
                width: zero,
                mix: zero,
            },
            CompiledProcessorKind::Flanger(_) => Self::Flanger {
                rate_hz: zero,
                depth: zero,
                feedback: zero,
                width: zero,
                mix: zero,
            },
            CompiledProcessorKind::Phaser(_) => Self::Phaser {
                rate_hz: zero,
                depth: zero,
                feedback: zero,
                width: zero,
                mix: zero,
            },
            CompiledProcessorKind::FrequencyShifter(_) => Self::FrequencyShifter {
                shift_hz: zero,
                mix: zero,
            },
            CompiledProcessorKind::Delay(_) => Self::Delay {
                feedback: zero,
                mix: zero,
            },
            CompiledProcessorKind::Reverb(_) => Self::Reverb {
                decay: zero,
                damping: zero,
                width: zero,
                mix: zero,
            },
            CompiledProcessorKind::Convolution(_) => Self::Convolution {
                gain_db: zero,
                mix: zero,
            },
            CompiledProcessorKind::Gate(_) => Self::Gate {
                threshold_db: zero,
                range_db: zero,
            },
            CompiledProcessorKind::Vocoder(_) => Self::Vocoder {
                modulator_gain_db: zero,
                output_gain_db: zero,
                mix: zero,
            },
            CompiledProcessorKind::EnvelopeTransfer(_) => Self::EnvelopeTransfer {
                input_gain_db: zero,
                floor_db: zero,
                mix: zero,
            },
            CompiledProcessorKind::SpectralMorph(_) => Self::SpectralMorph {
                morph: zero,
                output_gain_db: zero,
            },
            CompiledProcessorKind::TransientShaper(_) => Self::TransientShaper {
                attack: zero,
                sustain: zero,
                mix: zero,
            },
            CompiledProcessorKind::Compressor(_) => Self::Compressor {
                threshold_db: zero,
                ratio: zero,
                makeup_gain_db: zero,
                mix: zero,
            },
            CompiledProcessorKind::Limiter(_) => Self::Limiter {
                ceiling_db: zero,
                input_gain_db: zero,
            },
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn clear(&mut self) {
        let zero = ValueSpan {
            start: 0.0,
            end: 0.0,
        };
        match self {
            Self::Filter { cutoff, resonance } => {
                *cutoff = zero;
                *resonance = zero;
            }
            Self::LadderFilter {
                cutoff,
                resonance,
                drive,
            } => {
                *cutoff = zero;
                *resonance = zero;
                *drive = zero;
            }
            Self::Drive { amount, mix } => {
                *amount = zero;
                *mix = zero;
            }
            Self::Eq {
                low_gain_db,
                mid_gain_db,
                high_gain_db,
            } => {
                *low_gain_db = zero;
                *mid_gain_db = zero;
                *high_gain_db = zero;
            }
            Self::Formant {
                vowel_position,
                formant_shift,
                throat,
                mix,
            } => {
                *vowel_position = zero;
                *formant_shift = zero;
                *throat = zero;
                *mix = zero;
            }
            Self::Resonator {
                frequency_hz,
                decay_seconds,
                damping,
                mix,
            } => {
                *frequency_hz = zero;
                *decay_seconds = zero;
                *damping = zero;
                *mix = zero;
            }
            Self::Bitcrusher {
                bit_depth,
                sample_rate_ratio,
                mix,
            } => {
                *bit_depth = zero;
                *sample_rate_ratio = zero;
                *mix = zero;
            }
            Self::Chorus {
                rate_hz,
                depth,
                feedback,
                width,
                mix,
            }
            | Self::Flanger {
                rate_hz,
                depth,
                feedback,
                width,
                mix,
            }
            | Self::Phaser {
                rate_hz,
                depth,
                feedback,
                width,
                mix,
            } => {
                *rate_hz = zero;
                *depth = zero;
                *feedback = zero;
                *width = zero;
                *mix = zero;
            }
            Self::FrequencyShifter { shift_hz, mix } => {
                *shift_hz = zero;
                *mix = zero;
            }
            Self::Delay { feedback, mix } => {
                *feedback = zero;
                *mix = zero;
            }
            Self::Reverb {
                decay,
                damping,
                width,
                mix,
            } => {
                *decay = zero;
                *damping = zero;
                *width = zero;
                *mix = zero;
            }
            Self::Convolution { gain_db, mix } => {
                *gain_db = zero;
                *mix = zero;
            }
            Self::Gate {
                threshold_db,
                range_db,
            } => {
                *threshold_db = zero;
                *range_db = zero;
            }
            Self::Vocoder {
                modulator_gain_db,
                output_gain_db,
                mix,
            } => {
                *modulator_gain_db = zero;
                *output_gain_db = zero;
                *mix = zero;
            }
            Self::EnvelopeTransfer {
                input_gain_db,
                floor_db,
                mix,
            } => {
                *input_gain_db = zero;
                *floor_db = zero;
                *mix = zero;
            }
            Self::SpectralMorph {
                morph,
                output_gain_db,
            } => {
                *morph = zero;
                *output_gain_db = zero;
            }
            Self::TransientShaper {
                attack,
                sustain,
                mix,
            } => {
                *attack = zero;
                *sustain = zero;
                *mix = zero;
            }
            Self::Compressor {
                threshold_db,
                ratio,
                makeup_gain_db,
                mix,
            } => {
                *threshold_db = zero;
                *ratio = zero;
                *makeup_gain_db = zero;
                *mix = zero;
            }
            Self::Limiter {
                ceiling_db,
                input_gain_db,
            } => {
                *ceiling_db = zero;
                *input_gain_db = zero;
            }
        }
    }
}

pub(crate) struct StereoProcessorChain {
    processors: Vec<StereoProcessorRuntime>,
}

enum StereoProcessorRuntime {
    Filter {
        left: DspFilter,
        right: DspFilter,
        mode: DspFilterMode,
    },
    LadderFilter(LadderFilterRuntime),
    Drive,
    Eq(EqRuntime),
    Formant(FormantProcessorRuntime),
    Resonator(ResonatorRuntime),
    Chorus(ChorusRuntime),
    Flanger(FlangerRuntime),
    Phaser(PhaserRuntime),
    FrequencyShifter(Box<FrequencyShifterRuntime>),
    Delay(StereoDelayRuntime),
    Reverb(Box<PlateReverbRuntime>),
    Convolution(Box<ConvolutionRuntime>),
    Gate(GateRuntime),
    Vocoder(Box<VocoderRuntime>),
    EnvelopeTransfer(EnvelopeTransferRuntime),
    SpectralMorph(Box<SpectralMorphRuntime>),
    TransientShaper(TransientShaperRuntime),
    Compressor(CompressorRuntime),
    Limiter(LimiterRuntime),
}

impl StereoProcessorChain {
    #[allow(clippy::too_many_lines)]
    pub(crate) fn new(
        processors: &[CompiledProcessor],
        spec: ProcessSpec,
    ) -> Result<Self, ProcessError> {
        let mut runtime = Vec::with_capacity(processors.len());
        for processor in processors {
            match &processor.processor {
                CompiledProcessorKind::Filter(value) => {
                    runtime.push(StereoProcessorRuntime::Filter {
                        left: prepare_filter(spec)?,
                        right: prepare_filter(spec)?,
                        mode: filter_mode(value.mode),
                    });
                }
                CompiledProcessorKind::LadderFilter(_) => {
                    #[allow(clippy::cast_possible_truncation)]
                    let sample_rate = spec.sample_rate as f32;
                    runtime.push(StereoProcessorRuntime::LadderFilter(
                        LadderFilterRuntime::new(sample_rate),
                    ));
                }
                CompiledProcessorKind::Drive(_) => {
                    runtime.push(StereoProcessorRuntime::Drive);
                }
                CompiledProcessorKind::Eq(value) => {
                    #[allow(clippy::cast_possible_truncation)]
                    let sample_rate = spec.sample_rate as f32;
                    runtime.push(StereoProcessorRuntime::Eq(EqRuntime::new(
                        value,
                        sample_rate,
                        GeneratorOutputMode::Stereo,
                    )?));
                }
                CompiledProcessorKind::Resonator(value) => {
                    runtime.push(StereoProcessorRuntime::Resonator(ResonatorRuntime::new(
                        value,
                        GeneratorOutputMode::Stereo,
                    )));
                }
                CompiledProcessorKind::Formant(value) => {
                    #[allow(clippy::cast_possible_truncation)]
                    let sample_rate = spec.sample_rate as f32;
                    runtime.push(StereoProcessorRuntime::Formant(
                        FormantProcessorRuntime::new(&value.profiles, sample_rate)?,
                    ));
                }
                CompiledProcessorKind::Chorus(value) => {
                    runtime.push(StereoProcessorRuntime::Chorus(ChorusRuntime::new(value)));
                }
                CompiledProcessorKind::Flanger(value) => {
                    runtime.push(StereoProcessorRuntime::Flanger(FlangerRuntime::new(value)));
                }
                CompiledProcessorKind::Phaser(value) => {
                    runtime.push(StereoProcessorRuntime::Phaser(PhaserRuntime::new(value)));
                }
                CompiledProcessorKind::FrequencyShifter(value) => {
                    #[allow(clippy::cast_possible_truncation)]
                    let sample_rate = spec.sample_rate as f32;
                    runtime.push(StereoProcessorRuntime::FrequencyShifter(Box::new(
                        FrequencyShifterRuntime::new(
                            value.coefficients.clone(),
                            value.latency_frames,
                            sample_rate,
                            value.effective_abs_shift_hz,
                        )?,
                    )));
                }
                CompiledProcessorKind::Delay(value) => {
                    #[allow(clippy::cast_possible_truncation)]
                    let sample_rate = spec.sample_rate as f32;
                    runtime.push(StereoProcessorRuntime::Delay(StereoDelayRuntime::new(
                        value,
                        sample_rate,
                    )));
                }
                CompiledProcessorKind::Reverb(value) => runtime.push(
                    StereoProcessorRuntime::Reverb(Box::new(PlateReverbRuntime::new(value))),
                ),
                CompiledProcessorKind::Convolution(value) => {
                    #[allow(clippy::cast_possible_truncation)]
                    let sample_rate = spec.sample_rate as f32;
                    runtime.push(StereoProcessorRuntime::Convolution(Box::new(
                        ConvolutionRuntime::new(value.prepared_ir.clone(), sample_rate)?,
                    )));
                }
                CompiledProcessorKind::Gate(value) => {
                    runtime.push(StereoProcessorRuntime::Gate(GateRuntime::new(value)));
                }
                CompiledProcessorKind::Vocoder(value) => {
                    runtime.push(StereoProcessorRuntime::Vocoder(Box::new(
                        VocoderRuntime::new(value, spec)?,
                    )));
                }
                CompiledProcessorKind::EnvelopeTransfer(value) => {
                    runtime.push(StereoProcessorRuntime::EnvelopeTransfer(
                        EnvelopeTransferRuntime::new(value),
                    ));
                }
                CompiledProcessorKind::SpectralMorph(value) => {
                    runtime.push(StereoProcessorRuntime::SpectralMorph(Box::new(
                        SpectralMorphRuntime::new(value, spec),
                    )));
                }
                CompiledProcessorKind::TransientShaper(value) => runtime.push(
                    StereoProcessorRuntime::TransientShaper(TransientShaperRuntime::new(
                        value.fast_attack_coeff,
                        value.fast_release_coeff,
                        value.slow_attack_coeff,
                        value.slow_release_coeff,
                    )),
                ),
                CompiledProcessorKind::Compressor(value) => {
                    runtime.push(StereoProcessorRuntime::Compressor(CompressorRuntime::new(
                        value,
                    )));
                }
                CompiledProcessorKind::Limiter(value) => {
                    runtime.push(StereoProcessorRuntime::Limiter(LimiterRuntime::new(value)));
                }
                CompiledProcessorKind::Bitcrusher(_) => {
                    return Err(ProcessError::ProcessorFailure {
                        kind: ProcessorFailureKind::InvalidState,
                    });
                }
            }
        }
        Ok(Self {
            processors: runtime,
        })
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn process(
        &mut self,
        targets: &[ProcessorTargetSpan],
        tempo_bpm: f64,
        external: ExternalAudioBlock<'_>,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if self.processors.len() != targets.len() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            });
        }
        for (processor, target) in self.processors.iter_mut().zip(targets) {
            match (processor, *target) {
                (
                    StereoProcessorRuntime::Filter {
                        left: filter_left,
                        right: filter_right,
                        mode,
                    },
                    ProcessorTargetSpan::Filter { cutoff, resonance },
                ) => {
                    process_filter(filter_left, *mode, cutoff, resonance, left)?;
                    process_filter(filter_right, *mode, cutoff, resonance, right)?;
                }
                (
                    StereoProcessorRuntime::LadderFilter(runtime),
                    ProcessorTargetSpan::LadderFilter {
                        cutoff,
                        resonance,
                        drive,
                    },
                ) => runtime.process_stereo(cutoff, resonance, drive, left, right)?,
                (StereoProcessorRuntime::Drive, ProcessorTargetSpan::Drive { amount, mix }) => {
                    DriveRuntime::process_stereo(amount, mix, left, right)?;
                }
                (
                    StereoProcessorRuntime::Eq(runtime),
                    ProcessorTargetSpan::Eq {
                        low_gain_db,
                        mid_gain_db,
                        high_gain_db,
                    },
                ) => runtime.process_stereo(low_gain_db, mid_gain_db, high_gain_db, left, right)?,
                (
                    StereoProcessorRuntime::Formant(runtime),
                    ProcessorTargetSpan::Formant {
                        vowel_position,
                        formant_shift,
                        throat,
                        mix,
                    },
                ) => runtime.process_stereo(
                    vowel_position,
                    formant_shift,
                    throat,
                    mix,
                    left,
                    right,
                )?,
                (
                    StereoProcessorRuntime::Resonator(runtime),
                    ProcessorTargetSpan::Resonator {
                        frequency_hz,
                        decay_seconds,
                        damping,
                        mix,
                    },
                ) => runtime.process_stereo(
                    frequency_hz,
                    decay_seconds,
                    damping,
                    mix,
                    left,
                    right,
                )?,
                (
                    StereoProcessorRuntime::Chorus(runtime),
                    ProcessorTargetSpan::Chorus {
                        rate_hz,
                        depth,
                        feedback,
                        width,
                        mix,
                    },
                ) => runtime.process(rate_hz, depth, feedback, width, mix, left, right)?,
                (
                    StereoProcessorRuntime::Flanger(runtime),
                    ProcessorTargetSpan::Flanger {
                        rate_hz,
                        depth,
                        feedback,
                        width,
                        mix,
                    },
                ) => runtime.process(rate_hz, depth, feedback, width, mix, left, right)?,
                (
                    StereoProcessorRuntime::Phaser(runtime),
                    ProcessorTargetSpan::Phaser {
                        rate_hz,
                        depth,
                        feedback,
                        width,
                        mix,
                    },
                ) => runtime.process(rate_hz, depth, feedback, width, mix, left, right)?,
                (
                    StereoProcessorRuntime::FrequencyShifter(runtime),
                    ProcessorTargetSpan::FrequencyShifter { shift_hz, mix },
                ) => runtime.process(shift_hz, mix, left, right)?,
                (
                    StereoProcessorRuntime::Delay(delay),
                    ProcessorTargetSpan::Delay { feedback, mix },
                ) => delay.process(feedback, mix, tempo_bpm, left, right)?,
                (
                    StereoProcessorRuntime::Reverb(reverb),
                    ProcessorTargetSpan::Reverb {
                        decay,
                        damping,
                        width,
                        mix,
                    },
                ) => reverb.process(decay, damping, width, mix, left, right)?,
                (
                    StereoProcessorRuntime::Convolution(runtime),
                    ProcessorTargetSpan::Convolution { gain_db, mix },
                ) => runtime.process(gain_db, mix, left, right)?,
                (
                    StereoProcessorRuntime::Gate(runtime),
                    ProcessorTargetSpan::Gate {
                        threshold_db,
                        range_db,
                    },
                ) => runtime.process(threshold_db, range_db, external, left, right)?,
                (
                    StereoProcessorRuntime::Vocoder(runtime),
                    ProcessorTargetSpan::Vocoder {
                        modulator_gain_db,
                        output_gain_db,
                        mix,
                    },
                ) => runtime.process(
                    modulator_gain_db,
                    output_gain_db,
                    mix,
                    external,
                    left,
                    right,
                )?,
                (
                    StereoProcessorRuntime::EnvelopeTransfer(runtime),
                    ProcessorTargetSpan::EnvelopeTransfer {
                        input_gain_db,
                        floor_db,
                        mix,
                    },
                ) => runtime.process(input_gain_db, floor_db, mix, external, left, right)?,
                (
                    StereoProcessorRuntime::SpectralMorph(runtime),
                    ProcessorTargetSpan::SpectralMorph {
                        morph,
                        output_gain_db,
                    },
                ) => runtime.process(morph, output_gain_db, external, left, right)?,
                (
                    StereoProcessorRuntime::TransientShaper(runtime),
                    ProcessorTargetSpan::TransientShaper {
                        attack,
                        sustain,
                        mix,
                    },
                ) => runtime.process(attack, sustain, mix, left, right)?,
                (
                    StereoProcessorRuntime::Compressor(runtime),
                    ProcessorTargetSpan::Compressor {
                        threshold_db,
                        ratio,
                        makeup_gain_db,
                        mix,
                    },
                ) => runtime.process(
                    threshold_db,
                    ratio,
                    makeup_gain_db,
                    mix,
                    external,
                    left,
                    right,
                )?,
                (
                    StereoProcessorRuntime::Limiter(runtime),
                    ProcessorTargetSpan::Limiter {
                        ceiling_db,
                        input_gain_db,
                    },
                ) => runtime.process(ceiling_db, input_gain_db, left, right)?,
                _ => {
                    return Err(ProcessError::ProcessorFailure {
                        kind: ProcessorFailureKind::InvalidState,
                    });
                }
            }
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) -> Result<(), ProcessError> {
        for processor in &mut self.processors {
            match processor {
                StereoProcessorRuntime::Filter { left, right, .. } => {
                    left.reset().map_err(ProcessError::from_filter_error)?;
                    right.reset().map_err(ProcessError::from_filter_error)?;
                }
                StereoProcessorRuntime::LadderFilter(runtime) => runtime.reset(),
                StereoProcessorRuntime::Drive => {}
                StereoProcessorRuntime::Eq(runtime) => runtime.reset(),
                StereoProcessorRuntime::Formant(runtime) => runtime.reset(),
                StereoProcessorRuntime::Resonator(runtime) => runtime.reset(),
                StereoProcessorRuntime::Chorus(runtime) => runtime.reset(),
                StereoProcessorRuntime::Flanger(runtime) => runtime.reset(),
                StereoProcessorRuntime::Phaser(runtime) => runtime.reset(),
                StereoProcessorRuntime::Delay(delay) => delay.reset(),
                StereoProcessorRuntime::Reverb(reverb) => reverb.reset(),
                StereoProcessorRuntime::FrequencyShifter(runtime) => runtime.reset(),
                StereoProcessorRuntime::Convolution(runtime) => runtime.reset(),
                StereoProcessorRuntime::Gate(runtime) => runtime.reset(),
                StereoProcessorRuntime::Vocoder(runtime) => runtime.reset(),
                StereoProcessorRuntime::EnvelopeTransfer(runtime) => runtime.reset(),
                StereoProcessorRuntime::SpectralMorph(runtime) => runtime.reset(),
                StereoProcessorRuntime::TransientShaper(runtime) => runtime.reset(),
                StereoProcessorRuntime::Compressor(runtime) => runtime.reset(),
                StereoProcessorRuntime::Limiter(runtime) => runtime.reset(),
            }
        }
        Ok(())
    }
}

fn prepare_filter(spec: ProcessSpec) -> Result<DspFilter, ProcessError> {
    let mut filter = DspFilter::new().map_err(ProcessError::from_filter_error)?;
    filter
        .prepare(spec.sample_rate)
        .map_err(ProcessError::from_filter_error)?;
    filter.reset().map_err(ProcessError::from_filter_error)?;
    Ok(filter)
}

fn filter_mode(mode: FilterModeDefinition) -> DspFilterMode {
    match mode {
        FilterModeDefinition::LowPass => DspFilterMode::LowPass,
        FilterModeDefinition::HighPass => DspFilterMode::HighPass,
        FilterModeDefinition::BandPass => DspFilterMode::BandPass,
        FilterModeDefinition::Notch => DspFilterMode::Notch,
    }
}

fn process_filter(
    filter: &mut DspFilter,
    mode: DspFilterMode,
    cutoff: ValueSpan,
    resonance: ValueSpan,
    buffer: &mut [f32],
) -> Result<(), ProcessError> {
    let process_mode =
        if same_value(cutoff.start, cutoff.end) && same_value(resonance.start, resonance.end) {
            0
        } else if same_value(resonance.start, resonance.end) {
            1
        } else {
            2
        };
    match process_mode {
        0 => filter
            .process(mode, cutoff.start, resonance.start, buffer)
            .map_err(ProcessError::from_filter_error),
        1 => filter
            .process_ramp(mode, cutoff.start, cutoff.end, resonance.start, buffer)
            .map_err(ProcessError::from_filter_error),
        _ => filter
            .process_ramp_with_resonance(
                mode,
                cutoff.start,
                cutoff.end,
                resonance.start,
                resonance.end,
                buffer,
            )
            .map_err(ProcessError::from_filter_error),
    }
}

fn same_value(left: f32, right: f32) -> bool {
    left.total_cmp(&right).is_eq()
}
