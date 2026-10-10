use crate::compiler::{CompiledGenerator, CompiledOscillatorBackend};
use crate::compiler::{CompiledLayer, CompiledProcessor};
use crate::definition::ModulationCurve;
use crate::parameter::ParameterHandle;
use crate::parameter::{ParameterDescriptor, ParameterScale};
use crate::process::ProcessError;

use super::processor::ProcessorTargetSpan;

/// How a value moves between the endpoints of a render span.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ValueInterpolation {
    /// Linear in the span's own units.
    Linear,
    /// Geometric in the span's own units, matching log-scaled parameters.
    Geometric,
}

/// A value that changes over one render span.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ValueSpan {
    pub(crate) start: f32,
    pub(crate) end: f32,
    interpolation: ValueInterpolation,
}

/// Result of evaluating one parameter in its native value domain.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EvaluatedParameterValue {
    pub(crate) base: f32,
    pub(crate) unclamped: f32,
    pub(crate) final_value: f32,
    pub(crate) clamped: bool,
}

#[cfg(test)]
mod contract_tests {
    use super::apply_domain_sum_with_maximum;
    use crate::parameter::{ParameterDescriptor, ParameterOwner, ParameterScale, ParameterUnit};

    fn descriptor(
        unit: ParameterUnit,
        scale: ParameterScale,
        min: f32,
        max: f32,
    ) -> ParameterDescriptor {
        ParameterDescriptor {
            id: "test".to_owned(),
            owner: ParameterOwner::Layer {
                definition_index: 0,
            },
            unit,
            scale,
            min,
            max,
            default: 0.0,
            smoothing_seconds: 0.0,
        }
    }

    #[test]
    fn linear_depth_is_applied_in_native_units() {
        let descriptor = descriptor(
            ParameterUnit::Cents,
            ParameterScale::Linear,
            -1_200.0,
            1_200.0,
        );
        let evaluated = apply_domain_sum_with_maximum(&descriptor, 0.5, 20.0, descriptor.max)
            .expect("linear evaluation");

        assert!((evaluated.base - 0.0).abs() < 1.0e-5);
        assert!((evaluated.unclamped - 20.0).abs() < 1.0e-5);
        assert!((evaluated.final_value - 20.0).abs() < 1.0e-5);
        assert!(!evaluated.clamped);
    }

    #[test]
    fn logarithmic_depth_is_applied_as_octaves() {
        let descriptor = descriptor(ParameterUnit::Hertz, ParameterScale::Log2, 20.0, 20_000.0);
        let base = descriptor.normalize(1_000.0).expect("base normalizes");
        let evaluated = apply_domain_sum_with_maximum(&descriptor, base, 2.0, descriptor.max)
            .expect("logarithmic evaluation");

        assert!((evaluated.base - 1_000.0).abs() < 1.0e-3);
        assert!((evaluated.unclamped - 4_000.0).abs() < 1.0e-3);
        assert!((evaluated.final_value - 4_000.0).abs() < 1.0e-3);
        assert!(!evaluated.clamped);
    }

    #[test]
    fn final_clamp_is_reported_after_route_sum() {
        let descriptor = descriptor(ParameterUnit::Pan, ParameterScale::Linear, -1.0, 1.0);
        let evaluated = apply_domain_sum_with_maximum(&descriptor, 0.5, 2.0, descriptor.max)
            .expect("clamped evaluation");

        assert!((evaluated.unclamped - 2.0).abs() < 1.0e-5);
        assert!((evaluated.final_value - 1.0).abs() < 1.0e-5);
        assert!(evaluated.clamped);
    }
}

/// Shape one normalized source value before applying a route depth.
#[must_use]
pub fn curve_value(value: f32, curve: ModulationCurve) -> f32 {
    match curve {
        ModulationCurve::Linear => value,
        ModulationCurve::SmoothStep => {
            let magnitude = value.abs();
            let shaped = magnitude * magnitude * (3.0 - 2.0 * magnitude);
            value.signum() * shaped
        }
    }
}

/// Convert a shaped source value into the target's modulation domain.
#[must_use]
pub fn route_domain_delta(source: f32, depth: f32, curve: ModulationCurve) -> f32 {
    curve_value(source, curve) * depth
}

/// Apply a summed direct-depth modulation value to a native parameter value.
pub(crate) fn apply_domain_sum_with_maximum(
    descriptor: &ParameterDescriptor,
    base_normalized: f32,
    domain_sum: f32,
    effective_maximum: f32,
) -> Result<EvaluatedParameterValue, ProcessError> {
    let base = descriptor
        .denormalize(base_normalized)
        .map_err(|_| ProcessError::InvalidEventValue)?;
    let unclamped = match descriptor.scale {
        ParameterScale::Linear | ParameterScale::LinearUnbounded => base + domain_sum,
        ParameterScale::Log2 => base * 2.0_f32.powf(domain_sum),
    };
    if !unclamped.is_finite() {
        return Err(ProcessError::InvalidEventValue);
    }
    let final_value = if descriptor.scale == ParameterScale::LinearUnbounded {
        unclamped
    } else {
        unclamped
            .clamp(descriptor.min, descriptor.max)
            .min(effective_maximum)
    };
    if !final_value.is_finite() {
        return Err(ProcessError::InvalidEventValue);
    }
    Ok(EvaluatedParameterValue {
        base,
        unclamped,
        final_value,
        clamped: final_value.total_cmp(&unclamped).is_ne(),
    })
}

/// Evaluate one parameter over a span and keep the span's interpolation
/// consistent with the parameter's scale.
///
/// A log-scaled parameter moves linearly in normalized space, which is
/// geometric in its native units, so its span interpolates geometrically.
pub(crate) fn evaluate_parameter_span(
    descriptor: &ParameterDescriptor,
    base: ValueSpan,
    start_domain_sum: f32,
    end_domain_sum: f32,
    effective_maximum: f32,
) -> Result<ValueSpan, ProcessError> {
    let start =
        apply_domain_sum_with_maximum(descriptor, base.start, start_domain_sum, effective_maximum)?
            .final_value;
    let end =
        apply_domain_sum_with_maximum(descriptor, base.end, end_domain_sum, effective_maximum)?
            .final_value;
    Ok(match descriptor.scale {
        ParameterScale::Linear | ParameterScale::LinearUnbounded => ValueSpan::linear(start, end),
        ParameterScale::Log2 => ValueSpan::geometric(start, end),
    })
}

impl ValueSpan {
    /// A span whose value moves linearly between its endpoints.
    pub(crate) const fn linear(start: f32, end: f32) -> Self {
        Self {
            start,
            end,
            interpolation: ValueInterpolation::Linear,
        }
    }

    /// A span whose value moves geometrically between positive endpoints, as
    /// log-scaled parameters do when they are interpolated in normalized space.
    pub(crate) const fn geometric(start: f32, end: f32) -> Self {
        Self {
            start,
            end,
            interpolation: ValueInterpolation::Geometric,
        }
    }

    pub(crate) fn is_constant(self) -> bool {
        self.start.total_cmp(&self.end).is_eq()
    }

    pub(crate) fn value_at(self, index: usize, frames: usize) -> f32 {
        let position = if frames == 0 {
            0.0
        } else {
            #[allow(clippy::cast_precision_loss)]
            {
                index as f32 / frames as f32
            }
        };
        match self.interpolation {
            ValueInterpolation::Linear => self.start + (self.end - self.start) * position,
            ValueInterpolation::Geometric => self.geometric_at(position),
        }
    }

    /// Endpoints stay exact so a span always reports the values it was built
    /// with, independently of how the span is partitioned. The ratio is
    /// evaluated in double precision so a sample stays as close to the
    /// per-sample curve as single-precision endpoints allow.
    fn geometric_at(self, position: f32) -> f32 {
        if position <= 0.0 {
            return self.start;
        }
        if position >= 1.0 {
            return self.end;
        }
        #[allow(clippy::cast_possible_truncation)]
        {
            (f64::from(self.start)
                * (f64::from(self.end) / f64::from(self.start)).powf(f64::from(position)))
                as f32
        }
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
    instrument_sources: &'a [ParameterSpanValue],
    offset: usize,
    length: usize,
    total_length: usize,
}

impl<'a> SharedParameterSpan<'a> {
    pub(crate) fn new(
        values: &'a [ParameterSpanValue],
        instrument_sources: &'a [ParameterSpanValue],
        length: usize,
    ) -> Self {
        Self {
            values,
            instrument_sources,
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

    pub(crate) fn instrument_source(
        self,
        handle: crate::compiler::InstrumentSourceHandle,
    ) -> Option<ValueSpan> {
        let value = *self.instrument_sources.get(handle.index())?;
        Some(interpolate(
            value.start,
            value.end,
            self.offset,
            self.length,
            self.total_length,
        ))
    }
}

fn interpolate(start: f32, end: f32, offset: usize, length: usize, total: usize) -> ValueSpan {
    let total = total.max(1);
    #[allow(clippy::cast_precision_loss)]
    let start_position = offset.min(total) as f32 / total as f32;
    #[allow(clippy::cast_precision_loss)]
    let end_position = (offset + length).min(total) as f32 / total as f32;
    ValueSpan::linear(
        start + (end - start) * start_position,
        start + (end - start) * end_position,
    )
}

/// Per-layer target values after base values and routes have been evaluated.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LayerTargetSpan {
    pub(crate) gain: ValueSpan,
    pub(crate) gain_weight: ValueSpan,
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
    PhysicalString {
        decay_seconds: ValueSpan,
        brightness: ValueSpan,
        stiffness: ValueSpan,
    },
    Modal {
        structure: ValueSpan,
        brightness: ValueSpan,
        decay: ValueSpan,
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
        let zero = ValueSpan::linear(0.0, 0.0);
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
            Self::PhysicalString(_) => LayerGeneratorTargetSpan::PhysicalString {
                decay_seconds: zero,
                brightness: zero,
                stiffness: zero,
            },
            Self::Modal(_) => LayerGeneratorTargetSpan::Modal {
                structure: zero,
                brightness: zero,
                decay: zero,
            },
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
        let zero = ValueSpan::linear(0.0, 0.0);
        Self {
            layers: layers
                .iter()
                .map(|layer| LayerTargetSpan {
                    gain: zero,
                    gain_weight: ValueSpan::linear(1.0, 1.0),
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
    use super::ValueSpan;

    #[test]
    fn value_span_uses_one_shared_ramp_formula() {
        let span = ValueSpan::linear(2.0, 6.0);

        assert!((span.value_at(0, 4) - 2.0).abs() < f32::EPSILON);
        assert!((span.value_at(2, 4) - 4.0).abs() < f32::EPSILON);
        assert!((span.value_at(4, 4) - 6.0).abs() < f32::EPSILON);
    }

    #[test]
    fn geometric_span_follows_the_parameter_scale() {
        let span = ValueSpan::geometric(20.0, 20_000.0);

        assert!((span.value_at(0, 32) - 20.0).abs() < f32::EPSILON);
        assert!((span.value_at(16, 32) - 632.455_5).abs() < 1.0e-3);
        assert!((span.value_at(32, 32) - 20_000.0).abs() < f32::EPSILON);
    }
}
