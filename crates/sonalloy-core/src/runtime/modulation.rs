use crate::compiler::{CompiledLayer, CompiledProcessor, CompiledProcessorKind};
use crate::parameter::ParameterHandle;

use super::processor::ProcessorTargetSpan;

/// A value that changes linearly over one render span.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ValueSpan {
    pub(crate) start: f32,
    pub(crate) end: f32,
}

/// One base parameter value after smoothing for a render span.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ParameterSpanValue {
    pub(crate) start: f32,
    pub(crate) end: f32,
}

/// Shared instrument controls visible to every voice during one render span.
#[derive(Clone, Copy)]
pub(crate) struct SharedParameterSpan<'a> {
    values: &'a [ParameterSpanValue],
    pitch_bend: ValueSpan,
    mod_wheel: ValueSpan,
    aftertouch: ValueSpan,
    offset: usize,
    length: usize,
    total_length: usize,
}

impl<'a> SharedParameterSpan<'a> {
    pub(crate) fn new(
        values: &'a [ParameterSpanValue],
        pitch_bend: ValueSpan,
        mod_wheel: ValueSpan,
        aftertouch: ValueSpan,
        length: usize,
    ) -> Self {
        Self {
            values,
            pitch_bend,
            mod_wheel,
            aftertouch,
            offset: 0,
            length,
            total_length: length,
        }
    }

    pub(crate) fn subspan(self, offset: usize, length: usize) -> Self {
        Self {
            offset: self.offset + offset,
            length,
            ..self
        }
    }

    pub(crate) fn parameter(self, handle: ParameterHandle) -> ValueSpan {
        let value = self.values[handle.index()];
        interpolate(
            value.start,
            value.end,
            self.offset,
            self.length,
            self.total_length,
        )
    }

    pub(crate) fn pitch_bend(self) -> ValueSpan {
        interpolate(
            self.pitch_bend.start,
            self.pitch_bend.end,
            self.offset,
            self.length,
            self.total_length,
        )
    }

    pub(crate) fn mod_wheel(self) -> ValueSpan {
        interpolate(
            self.mod_wheel.start,
            self.mod_wheel.end,
            self.offset,
            self.length,
            self.total_length,
        )
    }

    pub(crate) fn aftertouch(self) -> ValueSpan {
        interpolate(
            self.aftertouch.start,
            self.aftertouch.end,
            self.offset,
            self.length,
            self.total_length,
        )
    }
}

fn interpolate(start: f32, end: f32, offset: usize, length: usize, total: usize) -> ValueSpan {
    let total = total.max(1);
    #[allow(clippy::cast_precision_loss)]
    let start_position = offset.min(total) as f32 / total as f32;
    #[allow(clippy::cast_precision_loss)]
    let end_position = (offset + length).min(total) as f32 / total as f32;
    ValueSpan {
        start: start + (end - start) * start_position,
        end: start + (end - start) * end_position,
    }
}

/// Per-layer target values after base values and routes have been evaluated.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LayerTargetSpan {
    pub(crate) gain: ValueSpan,
    pub(crate) pan_left: ValueSpan,
    pub(crate) pan_right: ValueSpan,
    pub(crate) tuning: ValueSpan,
}

/// Reusable target scratch owned by one voice.
pub(crate) struct VoiceTargetScratch {
    pub(crate) layers: Vec<LayerTargetSpan>,
    pub(crate) layer_processors: Vec<Vec<ProcessorTargetSpan>>,
    pub(crate) voice_processors: Vec<ProcessorTargetSpan>,
}

impl VoiceTargetScratch {
    pub(crate) fn new(layers: &[CompiledLayer], voice_processors: &[CompiledProcessor]) -> Self {
        let zero = ValueSpan {
            start: 0.0,
            end: 0.0,
        };
        Self {
            layers: vec![
                LayerTargetSpan {
                    gain: zero,
                    pan_left: zero,
                    pan_right: zero,
                    tuning: zero,
                };
                layers.len()
            ],
            layer_processors: layers
                .iter()
                .map(|layer| layer.processors.iter().map(zero_processor_target).collect())
                .collect(),
            voice_processors: voice_processors.iter().map(zero_processor_target).collect(),
        }
    }
}

fn zero_processor_target(processor: &CompiledProcessor) -> ProcessorTargetSpan {
    let zero = ValueSpan {
        start: 0.0,
        end: 0.0,
    };
    match &processor.processor {
        CompiledProcessorKind::Filter(_) => ProcessorTargetSpan::Filter {
            cutoff: zero,
            resonance: zero,
        },
        CompiledProcessorKind::Drive(_) => ProcessorTargetSpan::Drive {
            amount: zero,
            mix: zero,
        },
        CompiledProcessorKind::Delay(_) => ProcessorTargetSpan::Delay {
            feedback: zero,
            mix: zero,
        },
        CompiledProcessorKind::Reverb(_) => ProcessorTargetSpan::Reverb {
            decay: zero,
            damping: zero,
            width: zero,
            mix: zero,
        },
    }
}
