mod bitcrusher;
mod chorus;
mod compressor;
mod delay;
mod drive;
mod eq;
mod flanger;
mod fractional_delay;
mod limiter;
mod phaser;
mod resonator;
mod reverb;

use sonalloy_dsp_sys::{DspFilter, DspFilterMode};

use crate::compiler::{CompiledProcessor, CompiledProcessorKind, GeneratorOutputMode};
use crate::definition::FilterModeDefinition;
use crate::process::{ProcessError, ProcessSpec, ProcessorFailureKind};

use super::modulation::ValueSpan;

pub(crate) use bitcrusher::BitcrusherRuntime;
pub(crate) use chorus::ChorusRuntime;
pub(crate) use compressor::CompressorRuntime;
pub(crate) use delay::StereoDelayRuntime;
pub(crate) use drive::DriveRuntime;
pub(crate) use eq::EqRuntime;
pub(crate) use flanger::FlangerRuntime;
pub(crate) use limiter::LimiterRuntime;
pub(crate) use phaser::PhaserRuntime;
pub(crate) use resonator::ResonatorRuntime;
pub(crate) use reverb::PlateReverbRuntime;

/// Runtime values corresponding to one compiled processor in a chain.
#[derive(Clone, Copy)]
pub(crate) enum ProcessorTargetSpan {
    Filter {
        cutoff: ValueSpan,
        resonance: ValueSpan,
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
    Drive,
    Eq(EqRuntime),
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
                | CompiledProcessorKind::Limiter(_) => {
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
                LayerProcessorRuntime::Drive => {}
                LayerProcessorRuntime::Eq(runtime) => runtime.reset(),
                LayerProcessorRuntime::Resonator(runtime) => runtime.reset(),
                LayerProcessorRuntime::Bitcrusher(runtime) => runtime.reset(),
            }
        }
        Ok(())
    }
}

impl ProcessorTargetSpan {
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
            CompiledProcessorKind::Drive(_) => Self::Drive {
                amount: zero,
                mix: zero,
            },
            CompiledProcessorKind::Eq(_) => Self::Eq {
                low_gain_db: zero,
                mid_gain_db: zero,
                high_gain_db: zero,
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
    Drive,
    Eq(EqRuntime),
    Resonator(ResonatorRuntime),
    Chorus(ChorusRuntime),
    Flanger(FlangerRuntime),
    Phaser(PhaserRuntime),
    Delay(StereoDelayRuntime),
    Reverb(Box<PlateReverbRuntime>),
    Compressor(CompressorRuntime),
    Limiter(LimiterRuntime),
}

impl StereoProcessorChain {
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
                CompiledProcessorKind::Chorus(value) => {
                    runtime.push(StereoProcessorRuntime::Chorus(ChorusRuntime::new(value)));
                }
                CompiledProcessorKind::Flanger(value) => {
                    runtime.push(StereoProcessorRuntime::Flanger(FlangerRuntime::new(value)));
                }
                CompiledProcessorKind::Phaser(value) => {
                    runtime.push(StereoProcessorRuntime::Phaser(PhaserRuntime::new(value)));
                }
                CompiledProcessorKind::Delay(value) => runtime.push(StereoProcessorRuntime::Delay(
                    StereoDelayRuntime::new(value.delay_frames),
                )),
                CompiledProcessorKind::Reverb(value) => runtime.push(
                    StereoProcessorRuntime::Reverb(Box::new(PlateReverbRuntime::new(value))),
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
                    StereoProcessorRuntime::Delay(delay),
                    ProcessorTargetSpan::Delay { feedback, mix },
                ) => delay.process(feedback, mix, left, right)?,
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
                    StereoProcessorRuntime::Compressor(runtime),
                    ProcessorTargetSpan::Compressor {
                        threshold_db,
                        ratio,
                        makeup_gain_db,
                        mix,
                    },
                ) => runtime.process(threshold_db, ratio, makeup_gain_db, mix, left, right)?,
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
                StereoProcessorRuntime::Drive => {}
                StereoProcessorRuntime::Eq(runtime) => runtime.reset(),
                StereoProcessorRuntime::Resonator(runtime) => runtime.reset(),
                StereoProcessorRuntime::Chorus(runtime) => runtime.reset(),
                StereoProcessorRuntime::Flanger(runtime) => runtime.reset(),
                StereoProcessorRuntime::Phaser(runtime) => runtime.reset(),
                StereoProcessorRuntime::Delay(delay) => delay.reset(),
                StereoProcessorRuntime::Reverb(reverb) => reverb.reset(),
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
