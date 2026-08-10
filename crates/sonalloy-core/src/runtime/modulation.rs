use crate::compiler::{CompiledGenerator, CompiledOscillatorBackend};
use crate::compiler::{CompiledLayer, CompiledProcessor};
use crate::parameter::ParameterHandle;

use super::processor::ProcessorTargetSpan;

/// A value that changes linearly over one render span.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ValueSpan {
    pub(crate) start: f32,
    pub(crate) end: f32,
}

impl ValueSpan {
    pub(crate) fn value_at(self, index: usize, frames: usize) -> f32 {
        let position = if frames == 0 {
            0.0
        } else {
            #[allow(clippy::cast_precision_loss)]
            {
                index as f32 / frames as f32
            }
        };
        self.start + (self.end - self.start) * position
    }
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

    pub(crate) fn parameter(self, handle: ParameterHandle) -> Option<ValueSpan> {
        let value = *self.values.get(handle.index())?;
        Some(interpolate(
            value.start,
            value.end,
            self.offset,
            self.length,
            self.total_length,
        ))
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
    pub(crate) pan: ValueSpan,
    pub(crate) tuning: ValueSpan,
    pub(crate) generator: LayerGeneratorTargetSpan,
}

/// Dynamic values consumed by a layer generator during one render span.
#[derive(Debug, Clone, Copy)]
pub(crate) enum LayerGeneratorTargetSpan {
    Oscillator {
        pulse_width: Option<ValueSpan>,
        sync_ratio: Option<ValueSpan>,
        waveshape: Option<ValueSpan>,
        phase_distortion: Option<ValueSpan>,
        wavefold: Option<ValueSpan>,
        oscillator_feedback: Option<ValueSpan>,
        unison_detune: Option<ValueSpan>,
        unison_spread: Option<ValueSpan>,
    },
    Noise {
        correlation: ValueSpan,
    },
    Additive {
        morph: ValueSpan,
        spectrum_tilt: ValueSpan,
        inharmonicity: ValueSpan,
    },
    Formant {
        vowel_position: ValueSpan,
        formant_shift: ValueSpan,
        throat: ValueSpan,
        spectral_tilt: ValueSpan,
    },
    Sample,
    Granular {
        position: ValueSpan,
        grain_size: ValueSpan,
        density: ValueSpan,
        pitch: ValueSpan,
        randomness: ValueSpan,
        pan_spread: ValueSpan,
    },
    WaveSequence,
    Wavetable {
        position: ValueSpan,
        unison_detune: Option<ValueSpan>,
        unison_spread: Option<ValueSpan>,
    },
    Spectral {
        position: ValueSpan,
        freeze: ValueSpan,
        blur: ValueSpan,
        shift: ValueSpan,
        morph: Option<ValueSpan>,
    },
    OperatorModulation {
        operators: [OperatorTargetSpan; 4],
        unison_detune: Option<ValueSpan>,
        unison_spread: Option<ValueSpan>,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OperatorTargetSpan {
    pub(crate) ratio: ValueSpan,
    pub(crate) detune: ValueSpan,
    pub(crate) level: Option<ValueSpan>,
    pub(crate) modulation_amount: Option<ValueSpan>,
    pub(crate) feedback: Option<ValueSpan>,
}

impl CompiledGenerator {
    pub(crate) fn zero_target_span(&self) -> LayerGeneratorTargetSpan {
        let zero = ValueSpan {
            start: 0.0,
            end: 0.0,
        };
        match self {
            Self::Oscillator(value) => LayerGeneratorTargetSpan::Oscillator {
                pulse_width: value.parameters.pulse_width.map(|_| zero),
                sync_ratio: match value.backend {
                    CompiledOscillatorBackend::Basic | CompiledOscillatorBackend::PhaseDomain => {
                        None
                    }
                    CompiledOscillatorBackend::VariableShapeSync { .. } => Some(zero),
                },
                waveshape: value.parameters.waveshape.map(|_| zero),
                phase_distortion: value.parameters.phase_distortion.map(|_| zero),
                wavefold: value.parameters.wavefold.map(|_| zero),
                oscillator_feedback: value.parameters.oscillator_feedback.map(|_| zero),
                unison_detune: value.parameters.unison_detune.map(|_| zero),
                unison_spread: value.parameters.unison_spread.map(|_| zero),
            },
            Self::Noise(_) => LayerGeneratorTargetSpan::Noise { correlation: zero },
            Self::Additive(_) => LayerGeneratorTargetSpan::Additive {
                morph: zero,
                spectrum_tilt: zero,
                inharmonicity: zero,
            },
            Self::Formant(_) => LayerGeneratorTargetSpan::Formant {
                vowel_position: zero,
                formant_shift: zero,
                throat: zero,
                spectral_tilt: zero,
            },
            Self::Sample(_) => LayerGeneratorTargetSpan::Sample,
            Self::Granular(_) => LayerGeneratorTargetSpan::Granular {
                position: zero,
                grain_size: zero,
                density: zero,
                pitch: zero,
                randomness: zero,
                pan_spread: zero,
            },
            Self::WaveSequence(_) => LayerGeneratorTargetSpan::WaveSequence,
            Self::Wavetable(value) => LayerGeneratorTargetSpan::Wavetable {
                position: zero,
                unison_detune: value.parameters.unison_detune.map(|_| zero),
                unison_spread: value.parameters.unison_spread.map(|_| zero),
            },
            Self::Spectral(value) => LayerGeneratorTargetSpan::Spectral {
                position: zero,
                freeze: zero,
                blur: zero,
                shift: zero,
                morph: value.parameters.morph.map(|_| zero),
            },
            Self::OperatorModulation(value) => LayerGeneratorTargetSpan::OperatorModulation {
                operators: std::array::from_fn(|index| {
                    let parameters = value.parameters[index];
                    OperatorTargetSpan {
                        ratio: zero,
                        detune: zero,
                        level: parameters.level.map(|_| zero),
                        modulation_amount: parameters.modulation_amount.map(|_| zero),
                        feedback: parameters.feedback.map(|_| zero),
                    }
                }),
                unison_detune: value.unison_detune.map(|_| zero),
                unison_spread: value.unison_spread.map(|_| zero),
            },
        }
    }
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
            layers: layers
                .iter()
                .map(|layer| LayerTargetSpan {
                    gain: zero,
                    pan: zero,
                    tuning: zero,
                    generator: layer.generator.zero_target_span(),
                })
                .collect(),
            layer_processors: layers
                .iter()
                .map(|layer| {
                    layer
                        .processors
                        .iter()
                        .map(|processor| ProcessorTargetSpan::zero_for(&processor.processor))
                        .collect()
                })
                .collect(),
            voice_processors: voice_processors
                .iter()
                .map(|processor| ProcessorTargetSpan::zero_for(&processor.processor))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_span_uses_one_shared_ramp_formula() {
        let span = ValueSpan {
            start: 2.0,
            end: 6.0,
        };

        assert!((span.value_at(0, 4) - 2.0).abs() < f32::EPSILON);
        assert!((span.value_at(2, 4) - 4.0).abs() < f32::EPSILON);
        assert!((span.value_at(4, 4) - 6.0).abs() < f32::EPSILON);
    }
}
