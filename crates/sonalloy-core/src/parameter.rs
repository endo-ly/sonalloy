use std::collections::HashMap;

use serde::Serialize;
use thiserror::Error;

use crate::definition::{
    GeneratorDefinition, InstrumentDefinition, OperatorModulationMode, OscillatorWaveform,
    ProcessorDefinition,
};
use crate::generator_parameters::{
    ADDITIVE_INHARMONICITY, ADDITIVE_MORPH, ADDITIVE_SPECTRUM_TILT, GRAIN_DENSITY,
    GRAIN_PAN_SPREAD, GRAIN_PITCH, GRAIN_RANDOMNESS, GRAIN_SIZE, GRANULAR_POSITION,
    GeneratorParameterSpec, NOISE_CORRELATION, OPERATOR_AM_RING_AMOUNT_MAX,
    OPERATOR_AM_RING_AMOUNT_MIN, OPERATOR_DETUNE_MAX, OPERATOR_DETUNE_MIN, OPERATOR_FEEDBACK_MAX,
    OPERATOR_FEEDBACK_MIN, OPERATOR_LEVEL_MAX, OPERATOR_LEVEL_MIN,
    OPERATOR_PARAMETER_SMOOTHING_SECONDS, OPERATOR_PARAMETER_SUFFIXES,
    OPERATOR_PHASE_FREQUENCY_AMOUNT_MAX, OPERATOR_PHASE_FREQUENCY_AMOUNT_MIN, OPERATOR_RATIO_MAX,
    OPERATOR_RATIO_MIN, OSCILLATOR_FEEDBACK, PHASE_DISTORTION, PULSE_WIDTH, SYNC_RATIO,
    UNISON_DETUNE, UNISON_SPREAD, WAVEFOLD, WAVESHAPE, WAVETABLE_POSITION,
};

/// Dense reference to a parameter in one compiled instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct ParameterHandle(usize);

impl ParameterHandle {
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Return the dense catalog index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Owner of a parameter in the Definition structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterOwner {
    /// A shared value belonging to a Definition layer.
    Layer { definition_index: usize },
    /// A continuous value belonging to a layer generator.
    LayerGenerator { definition_index: usize },
    /// A value belonging to a layer processor.
    LayerProcessor {
        /// Original Definition layer index.
        definition_index: usize,
        /// Processor index within the layer chain.
        processor_index: usize,
    },
    /// A value belonging to a voice processor.
    VoiceProcessor {
        /// Processor index within the voice chain.
        processor_index: usize,
    },
    /// A value belonging to a global processor.
    GlobalProcessor {
        /// Processor index within the global chain.
        processor_index: usize,
    },
}

/// Native unit exposed by the parameter contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterUnit {
    /// Gain in decibels.
    Decibels,
    /// Constant-power pan position.
    Pan,
    /// Tuning offset in cents.
    Cents,
    /// Frequency in hertz.
    Hertz,
    /// A frequency ratio.
    Ratio,
    /// A duration in seconds.
    Seconds,
    /// A rate expressed per second.
    PerSecond,
    /// A unitless synthesis index.
    Index,
    /// Spectral tilt in decibels per octave.
    DecibelsPerOctave,
    /// A unitless value in the inclusive zero-to-one range.
    Normalized,
}

/// Mapping used by normalized control values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterScale {
    /// Uniform spacing in native units.
    Linear,
    /// Uniform spacing in base-two logarithmic units.
    Log2,
}

/// Error returned when a parameter value cannot be represented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ParameterValueError {
    /// A native or normalized value was not finite.
    #[error("parameter value must be finite")]
    NonFinite,
    /// A value was outside the descriptor range.
    #[error("parameter value is outside its range")]
    OutOfRange,
}

/// Immutable metadata and range contract for one continuous parameter.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParameterDescriptor {
    /// Canonical stable identifier.
    pub id: String,
    /// Definition owner.
    pub owner: ParameterOwner,
    /// Native unit.
    pub unit: ParameterUnit,
    /// Normalized mapping.
    pub scale: ParameterScale,
    /// Inclusive native minimum.
    pub min: f32,
    /// Inclusive native maximum.
    pub max: f32,
    /// Native default copied from the Definition.
    pub default: f32,
    /// Base-value smoothing duration in seconds.
    pub smoothing_seconds: f32,
}

impl ParameterDescriptor {
    /// Convert a native value to normalized form.
    ///
    /// # Errors
    ///
    /// Returns an error for non-finite or out-of-range input.
    pub fn normalize(&self, native: f32) -> Result<f32, ParameterValueError> {
        if !native.is_finite() {
            return Err(ParameterValueError::NonFinite);
        }
        if !(self.min..=self.max).contains(&native) {
            return Err(ParameterValueError::OutOfRange);
        }
        let normalized = match self.scale {
            ParameterScale::Linear => (native - self.min) / (self.max - self.min),
            ParameterScale::Log2 => (native / self.min).log2() / (self.max / self.min).log2(),
        };
        if normalized.is_finite() {
            Ok(normalized.clamp(0.0, 1.0))
        } else {
            Err(ParameterValueError::NonFinite)
        }
    }

    /// Convert a normalized value to native form.
    ///
    /// # Errors
    ///
    /// Returns an error for non-finite or out-of-range input.
    pub fn denormalize(&self, normalized: f32) -> Result<f32, ParameterValueError> {
        if !normalized.is_finite() {
            return Err(ParameterValueError::NonFinite);
        }
        if !(0.0..=1.0).contains(&normalized) {
            return Err(ParameterValueError::OutOfRange);
        }
        let native = match self.scale {
            ParameterScale::Linear => self.min + normalized * (self.max - self.min),
            ParameterScale::Log2 => {
                self.min * 2.0_f32.powf(normalized * (self.max / self.min).log2())
            }
        };
        if native.is_finite() {
            Ok(native.clamp(self.min, self.max))
        } else {
            Err(ParameterValueError::NonFinite)
        }
    }
}

/// Compiled catalog used by control code and runtime bindings.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterCatalog {
    descriptors: Box<[ParameterDescriptor]>,
    lookup: HashMap<String, ParameterHandle>,
}

impl ParameterCatalog {
    pub(crate) fn from_definition(definition: &InstrumentDefinition) -> Self {
        let mut descriptors = Vec::with_capacity(definition.layers.len() * 3);
        for (definition_index, layer) in definition.layers.iter().enumerate() {
            descriptors.push(ParameterDescriptor {
                id: layer_parameter_id(&layer.id, "gain"),
                owner: ParameterOwner::Layer { definition_index },
                unit: ParameterUnit::Decibels,
                scale: ParameterScale::Linear,
                min: -60.0,
                max: 12.0,
                default: layer.gain_db,
                smoothing_seconds: 0.005,
            });
            descriptors.push(ParameterDescriptor {
                id: layer_parameter_id(&layer.id, "pan"),
                owner: ParameterOwner::Layer { definition_index },
                unit: ParameterUnit::Pan,
                scale: ParameterScale::Linear,
                min: -1.0,
                max: 1.0,
                default: layer.pan,
                smoothing_seconds: 0.005,
            });
            descriptors.push(ParameterDescriptor {
                id: layer_parameter_id(&layer.id, "tuning"),
                owner: ParameterOwner::Layer { definition_index },
                unit: ParameterUnit::Cents,
                scale: ParameterScale::Linear,
                min: -1200.0,
                max: 1200.0,
                default: layer.tuning_cents,
                smoothing_seconds: 0.005,
            });
            push_generator_descriptors(
                &mut descriptors,
                &layer.generator,
                ParameterOwner::LayerGenerator { definition_index },
                &format!("layer.{}.generator", layer.id),
            );
            for (processor_index, processor) in layer.processors.iter().enumerate() {
                push_processor_descriptors(
                    &mut descriptors,
                    processor,
                    ParameterOwner::LayerProcessor {
                        definition_index,
                        processor_index,
                    },
                    &format!("layer.{}.processor", layer.id),
                );
            }
        }
        for (processor_index, processor) in definition.voice_processors.iter().enumerate() {
            push_processor_descriptors(
                &mut descriptors,
                processor,
                ParameterOwner::VoiceProcessor { processor_index },
                "voice.processor",
            );
        }
        for (processor_index, processor) in definition.global_processors.iter().enumerate() {
            push_processor_descriptors(
                &mut descriptors,
                processor,
                ParameterOwner::GlobalProcessor { processor_index },
                "global.processor",
            );
        }
        let descriptors = descriptors.into_boxed_slice();
        let lookup = descriptors
            .iter()
            .enumerate()
            .map(|(index, descriptor)| (descriptor.id.clone(), ParameterHandle::new(index)))
            .collect();
        Self {
            descriptors,
            lookup,
        }
    }

    /// Return descriptors in their stable catalog order.
    #[must_use]
    pub fn parameters(&self) -> &[ParameterDescriptor] {
        &self.descriptors
    }

    /// Resolve a canonical identifier before entering the audio path.
    #[must_use]
    pub fn parameter_handle(&self, id: &str) -> Option<ParameterHandle> {
        self.lookup.get(id).copied()
    }

    /// Return a descriptor by compiled handle.
    #[must_use]
    pub fn descriptor(&self, handle: ParameterHandle) -> Option<&ParameterDescriptor> {
        self.descriptors.get(handle.index())
    }

    pub(crate) fn len(&self) -> usize {
        self.descriptors.len()
    }
}

#[allow(clippy::too_many_lines)]
fn push_generator_descriptors(
    descriptors: &mut Vec<ParameterDescriptor>,
    generator: &GeneratorDefinition,
    owner: ParameterOwner,
    prefix: &str,
) {
    match generator {
        GeneratorDefinition::Oscillator(oscillator) => {
            if let OscillatorWaveform::Pulse { pulse_width } = oscillator.waveform {
                push_generator_descriptor(descriptors, prefix, owner, PULSE_WIDTH, pulse_width);
            }
            if let Some(hard_sync) = oscillator.hard_sync {
                push_generator_descriptor(descriptors, prefix, owner, SYNC_RATIO, hard_sync.ratio);
            }
            if let Some(waveshaping) = oscillator.waveshaping {
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    WAVESHAPE,
                    waveshaping.amount,
                );
            }
            if let Some(phase_distortion) = oscillator.phase_distortion {
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    PHASE_DISTORTION,
                    phase_distortion.amount,
                );
            }
            if let Some(wavefold) = oscillator.wavefold {
                push_generator_descriptor(descriptors, prefix, owner, WAVEFOLD, wavefold.amount);
            }
            if let Some(feedback) = oscillator.feedback {
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    OSCILLATOR_FEEDBACK,
                    feedback.amount,
                );
            }
            if let Some(unison) = oscillator.unison {
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    UNISON_DETUNE,
                    unison.detune_cents,
                );
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    UNISON_SPREAD,
                    unison.stereo_spread,
                );
            }
        }
        GeneratorDefinition::Noise(noise) => {
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                NOISE_CORRELATION,
                noise.stereo_correlation,
            );
        }
        GeneratorDefinition::Additive(additive) => {
            push_generator_descriptor(descriptors, prefix, owner, ADDITIVE_MORPH, additive.morph);
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                ADDITIVE_SPECTRUM_TILT,
                additive.spectrum_tilt_db_per_octave,
            );
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                ADDITIVE_INHARMONICITY,
                additive.inharmonicity,
            );
        }
        GeneratorDefinition::Sample(_) | GeneratorDefinition::WaveSequence(_) => {}
        GeneratorDefinition::Granular(granular) => {
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                GRANULAR_POSITION,
                granular.position,
            );
            push_generator_descriptor(descriptors, prefix, owner, GRAIN_SIZE, granular.grain_size);
            push_generator_descriptor(descriptors, prefix, owner, GRAIN_DENSITY, granular.density);
            push_generator_descriptor(descriptors, prefix, owner, GRAIN_PITCH, granular.pitch);
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                GRAIN_RANDOMNESS,
                granular.randomness,
            );
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                GRAIN_PAN_SPREAD,
                granular.pan_spread,
            );
        }
        GeneratorDefinition::Wavetable(wavetable) => {
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                WAVETABLE_POSITION,
                wavetable.position,
            );
            if let Some(unison) = wavetable.unison {
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    UNISON_DETUNE,
                    unison.detune_cents,
                );
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    UNISON_SPREAD,
                    unison.stereo_spread,
                );
            }
        }
        GeneratorDefinition::OperatorModulation(operator_modulation) => {
            let topology = operator_modulation.algorithm.topology();
            for (index, operator) in operator_modulation.operators.iter().take(4).enumerate() {
                push_operator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    index,
                    "ratio",
                    ParameterUnit::Ratio,
                    ParameterScale::Log2,
                    OPERATOR_RATIO_MIN,
                    OPERATOR_RATIO_MAX,
                    operator.ratio,
                );
                push_operator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    index,
                    "detune",
                    ParameterUnit::Cents,
                    ParameterScale::Linear,
                    OPERATOR_DETUNE_MIN,
                    OPERATOR_DETUNE_MAX,
                    operator.detune_cents,
                );
                if topology.carrier_mask & (1_u8 << index) != 0 {
                    push_operator_descriptor(
                        descriptors,
                        prefix,
                        owner,
                        index,
                        "level",
                        ParameterUnit::Normalized,
                        ParameterScale::Linear,
                        OPERATOR_LEVEL_MIN,
                        OPERATOR_LEVEL_MAX,
                        operator.level,
                    );
                }
                let has_output = topology
                    .incoming_masks
                    .iter()
                    .any(|mask| mask & (1_u8 << index) != 0);
                if has_output {
                    let (unit, min, max) = match operator_modulation.mode {
                        OperatorModulationMode::Phase | OperatorModulationMode::Frequency => (
                            ParameterUnit::Index,
                            OPERATOR_PHASE_FREQUENCY_AMOUNT_MIN,
                            OPERATOR_PHASE_FREQUENCY_AMOUNT_MAX,
                        ),
                        OperatorModulationMode::Amplitude | OperatorModulationMode::Ring => (
                            ParameterUnit::Normalized,
                            OPERATOR_AM_RING_AMOUNT_MIN,
                            OPERATOR_AM_RING_AMOUNT_MAX,
                        ),
                    };
                    push_operator_descriptor(
                        descriptors,
                        prefix,
                        owner,
                        index,
                        "modulation_amount",
                        unit,
                        ParameterScale::Linear,
                        min,
                        max,
                        operator.modulation_amount,
                    );
                }
                if matches!(
                    operator_modulation.mode,
                    OperatorModulationMode::Phase | OperatorModulationMode::Frequency
                ) {
                    push_operator_descriptor(
                        descriptors,
                        prefix,
                        owner,
                        index,
                        "feedback",
                        ParameterUnit::Normalized,
                        ParameterScale::Linear,
                        OPERATOR_FEEDBACK_MIN,
                        OPERATOR_FEEDBACK_MAX,
                        operator.feedback,
                    );
                }
            }
            if let Some(unison) = operator_modulation.unison {
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    UNISON_DETUNE,
                    unison.detune_cents,
                );
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    UNISON_SPREAD,
                    unison.stereo_spread,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_operator_descriptor(
    descriptors: &mut Vec<ParameterDescriptor>,
    prefix: &str,
    owner: ParameterOwner,
    index: usize,
    suffix: &str,
    unit: ParameterUnit,
    scale: ParameterScale,
    min: f32,
    max: f32,
    default: f32,
) {
    descriptors.push(ParameterDescriptor {
        id: format!("{prefix}.operator.{}.{}", index + 1, suffix),
        owner,
        unit,
        scale,
        min,
        max,
        default,
        smoothing_seconds: OPERATOR_PARAMETER_SMOOTHING_SECONDS,
    });
}

fn push_generator_descriptor(
    descriptors: &mut Vec<ParameterDescriptor>,
    prefix: &str,
    owner: ParameterOwner,
    spec: GeneratorParameterSpec,
    default: f32,
) {
    descriptors.push(ParameterDescriptor {
        id: format!("{prefix}.{}", spec.suffix),
        owner,
        unit: spec.unit,
        scale: spec.scale,
        min: spec.min,
        max: spec.max,
        default,
        smoothing_seconds: spec.smoothing_seconds,
    });
}

fn push_processor_descriptors(
    descriptors: &mut Vec<ParameterDescriptor>,
    processor: &ProcessorDefinition,
    owner: ParameterOwner,
    prefix: &str,
) {
    let processor_id = processor.id();
    let base = format!("{prefix}.{processor_id}");
    match processor {
        ProcessorDefinition::Filter(value) => {
            descriptors.push(ParameterDescriptor {
                id: format!("{base}.cutoff"),
                owner,
                unit: ParameterUnit::Hertz,
                scale: ParameterScale::Log2,
                min: 20.0,
                max: 20_000.0,
                default: value.cutoff_hz,
                smoothing_seconds: 0.010,
            });
            descriptors.push(ParameterDescriptor {
                id: format!("{base}.resonance"),
                owner,
                unit: ParameterUnit::Normalized,
                scale: ParameterScale::Linear,
                min: 0.0,
                max: 1.0,
                default: value.resonance,
                smoothing_seconds: 0.010,
            });
        }
        ProcessorDefinition::Drive(value) => {
            push_normalized_descriptor(
                descriptors,
                format!("{base}.amount"),
                owner,
                value.amount,
                0.005,
                1.0,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.mix"),
                owner,
                value.mix,
                0.005,
                1.0,
            );
        }
        ProcessorDefinition::Delay(value) => {
            push_normalized_descriptor(
                descriptors,
                format!("{base}.feedback"),
                owner,
                value.feedback,
                0.010,
                0.95,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.mix"),
                owner,
                value.mix,
                0.010,
                1.0,
            );
        }
        ProcessorDefinition::Reverb(value) => {
            push_normalized_descriptor(
                descriptors,
                format!("{base}.decay"),
                owner,
                value.decay,
                0.020,
                0.98,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.damping"),
                owner,
                value.damping,
                0.020,
                1.0,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.width"),
                owner,
                value.width,
                0.020,
                1.0,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.mix"),
                owner,
                value.mix,
                0.020,
                1.0,
            );
        }
    }
}

fn push_normalized_descriptor(
    descriptors: &mut Vec<ParameterDescriptor>,
    id: String,
    owner: ParameterOwner,
    default: f32,
    smoothing_seconds: f32,
    max: f32,
) {
    descriptors.push(ParameterDescriptor {
        id,
        owner,
        unit: ParameterUnit::Normalized,
        scale: ParameterScale::Linear,
        min: 0.0,
        max,
        default,
        smoothing_seconds,
    });
}

/// Build a canonical layer parameter identifier.
#[must_use]
pub fn layer_parameter_id(layer_id: &str, parameter: &str) -> String {
    format!("layer.{layer_id}.{parameter}")
}

/// Build a canonical layer generator parameter identifier.
#[must_use]
pub fn layer_generator_parameter_id(layer_id: &str, parameter: &str) -> String {
    format!("layer.{layer_id}.generator.{parameter}")
}

/// Build a canonical layer processor parameter identifier.
#[must_use]
pub fn layer_processor_parameter_id(layer_id: &str, processor_id: &str, parameter: &str) -> String {
    format!("layer.{layer_id}.processor.{processor_id}.{parameter}")
}

/// Build a canonical voice processor parameter identifier.
#[must_use]
pub fn voice_processor_parameter_id(processor_id: &str, parameter: &str) -> String {
    format!("voice.processor.{processor_id}.{parameter}")
}

/// Build a canonical global processor parameter identifier.
#[must_use]
pub fn global_processor_parameter_id(processor_id: &str, parameter: &str) -> String {
    format!("global.processor.{processor_id}.{parameter}")
}

/// Check the identifier grammar used by Definition components.
#[must_use]
pub fn is_component_id(value: &str) -> bool {
    if !(1..=64).contains(&value.len()) {
        return false;
    }
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

/// Check the canonical parameter identifier grammar used by modulation routes.
#[must_use]
pub fn is_parameter_id(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    match parts.as_slice() {
        ["layer", layer_id, parameter] => {
            is_component_id(layer_id) && matches!(*parameter, "gain" | "pan" | "tuning")
        }
        ["layer", layer_id, "processor", processor_id, parameter] => {
            is_component_id(layer_id)
                && is_component_id(processor_id)
                && is_processor_parameter(parameter)
        }
        ["layer", layer_id, "generator", parameter] => {
            is_component_id(layer_id) && crate::generator_parameters::is_suffix(parameter)
        }
        [
            "layer",
            layer_id,
            "generator",
            "operator",
            operator,
            parameter,
        ] => {
            is_component_id(layer_id)
                && matches!(*operator, "1" | "2" | "3" | "4")
                && is_operator_parameter(parameter)
        }
        ["voice" | "global", "processor", processor_id, parameter] => {
            is_component_id(processor_id) && is_processor_parameter(parameter)
        }
        _ => false,
    }
}

fn is_processor_parameter(value: &str) -> bool {
    matches!(
        value,
        "cutoff" | "resonance" | "amount" | "mix" | "feedback" | "decay" | "damping" | "width"
    )
}

fn is_operator_parameter(value: &str) -> bool {
    OPERATOR_PARAMETER_SUFFIXES.contains(&value)
}

/// Built-in source identifiers accepted by routes.
pub const BUILTIN_SOURCE_IDS: &[&str] = &[
    "velocity",
    "key_tracking",
    "pitch_bend",
    "mod_wheel",
    "aftertouch",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::tests::definition;

    #[test]
    fn catalog_order_is_definition_order_then_processor_scope() {
        let mut source = definition();
        source.layers[0].processors.push(ProcessorDefinition::Drive(
            crate::definition::DriveProcessorDefinition {
                id: "drive".to_owned(),
                amount: 0.5,
                mix: 0.5,
            },
        ));
        source.voice_processors.push(ProcessorDefinition::Filter(
            crate::definition::FilterProcessorDefinition {
                id: "tone".to_owned(),
                cutoff_hz: 1_000.0,
                resonance: 0.2,
            },
        ));
        let catalog = ParameterCatalog::from_definition(&source);
        let ids: Vec<_> = catalog
            .parameters()
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "layer.body.gain",
                "layer.body.pan",
                "layer.body.tuning",
                "layer.body.processor.drive.amount",
                "layer.body.processor.drive.mix",
                "voice.processor.tone.cutoff",
                "voice.processor.tone.resonance"
            ]
        );
    }

    #[test]
    fn generator_parameters_follow_layer_parameters_and_preserve_metadata() {
        let mut source = definition();
        source.layers[0].generator =
            GeneratorDefinition::Oscillator(crate::definition::OscillatorDefinition {
                waveform: OscillatorWaveform::Pulse { pulse_width: 0.25 },
                phase_reset: true,
                phase: 0.0,
                hard_sync: None,
                waveshaping: None,
                phase_distortion: None,
                wavefold: None,
                feedback: None,
                unison: None,
            });
        let mut noise_layer = source.layers[0].clone();
        noise_layer.id = "texture".to_owned();
        noise_layer.generator = GeneratorDefinition::Noise(crate::definition::NoiseDefinition {
            color: crate::definition::NoiseColor::White,
            seed: 7,
            stereo_correlation: 0.6,
        });
        source.layers.push(noise_layer);

        let catalog = ParameterCatalog::from_definition(&source);
        let ids: Vec<_> = catalog
            .parameters()
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "layer.body.gain",
                "layer.body.pan",
                "layer.body.tuning",
                "layer.body.generator.pulse_width",
                "layer.texture.gain",
                "layer.texture.pan",
                "layer.texture.tuning",
                "layer.texture.generator.noise_correlation",
            ]
        );

        let pulse = catalog
            .parameters()
            .iter()
            .find(|parameter| parameter.id == "layer.body.generator.pulse_width")
            .expect("pulse width descriptor");
        assert_eq!(
            pulse.owner,
            ParameterOwner::LayerGenerator {
                definition_index: 0
            }
        );
        assert_eq!(pulse.unit, ParameterUnit::Normalized);
        assert_eq!(pulse.scale, ParameterScale::Linear);
        assert!((pulse.min - 0.05).abs() < f32::EPSILON);
        assert!((pulse.max - 0.95).abs() < f32::EPSILON);
        assert!((pulse.default - 0.25).abs() < f32::EPSILON);
        assert!((pulse.smoothing_seconds - 0.005).abs() < f32::EPSILON);

        let correlation = catalog
            .parameters()
            .iter()
            .find(|parameter| parameter.id == "layer.texture.generator.noise_correlation")
            .expect("noise correlation descriptor");
        assert_eq!(
            correlation.owner,
            ParameterOwner::LayerGenerator {
                definition_index: 1
            }
        );
        assert!((correlation.min - 0.0).abs() < f32::EPSILON);
        assert!((correlation.max - 1.0).abs() < f32::EPSILON);
        assert!((correlation.default - 0.6).abs() < f32::EPSILON);
        assert!((correlation.smoothing_seconds - 0.010).abs() < f32::EPSILON);
    }

    #[test]
    fn generator_parameter_ids_follow_the_canonical_grammar() {
        assert_eq!(
            layer_generator_parameter_id("body", "pulse_width"),
            "layer.body.generator.pulse_width"
        );
        assert!(is_parameter_id("layer.body.generator.pulse_width"));
        assert!(is_parameter_id("layer.body.generator.sync_ratio"));
        assert!(is_parameter_id("layer.body.generator.waveshape"));
        assert!(is_parameter_id("layer.body.generator.unison_detune"));
        assert!(is_parameter_id("layer.body.generator.unison_spread"));
        assert!(is_parameter_id("layer.body.generator.noise_correlation"));
        assert!(is_parameter_id("layer.body.generator.operator.1.ratio"));
        assert!(is_parameter_id(
            "layer.body.generator.operator.4.modulation_amount"
        ));
        assert!(!is_parameter_id("layer.Body.generator.pulse_width"));
        assert!(!is_parameter_id("layer.body.generator.operator.5.ratio"));
    }

    #[test]
    fn complex_generator_parameters_preserve_catalog_metadata() {
        let mut source = definition();
        source.layers[0].generator =
            GeneratorDefinition::Oscillator(crate::definition::OscillatorDefinition {
                waveform: OscillatorWaveform::Saw,
                phase_reset: true,
                phase: 0.0,
                hard_sync: Some(crate::definition::HardSyncDefinition { ratio: 3.0 }),
                waveshaping: Some(crate::definition::WaveshapingDefinition { amount: 0.25 }),
                phase_distortion: None,
                wavefold: None,
                feedback: None,
                unison: Some(crate::definition::UnisonDefinition {
                    voices: 5,
                    detune_cents: 18.0,
                    stereo_spread: 0.8,
                    phase_spread: 0.2,
                }),
            });
        let catalog = ParameterCatalog::from_definition(&source);
        let parameters = catalog.parameters();
        let ids: Vec<_> = parameters
            .iter()
            .map(|parameter| parameter.id.as_str())
            .collect();
        assert_eq!(
            &ids[3..],
            [
                "layer.body.generator.sync_ratio",
                "layer.body.generator.waveshape",
                "layer.body.generator.unison_detune",
                "layer.body.generator.unison_spread",
            ]
        );

        let sync = &parameters[3];
        assert_eq!(sync.unit, ParameterUnit::Ratio);
        assert_eq!(sync.scale, ParameterScale::Log2);
        assert!((sync.min - 1.0).abs() < f32::EPSILON);
        assert!((sync.max - 16.0).abs() < f32::EPSILON);
        assert!((sync.default - 3.0).abs() < f32::EPSILON);
        assert!((sync.smoothing_seconds - 0.005).abs() < f32::EPSILON);

        let detune = &parameters[5];
        assert_eq!(detune.unit, ParameterUnit::Cents);
        assert!(detune.min.abs() < f32::EPSILON);
        assert!((detune.max - 100.0).abs() < f32::EPSILON);
        assert!((detune.default - 18.0).abs() < f32::EPSILON);
        assert!((detune.smoothing_seconds - 0.010).abs() < f32::EPSILON);
    }

    #[test]
    fn operator_parameters_follow_topology_and_mode_contract() {
        let mut source = definition();
        let operator = crate::definition::OperatorDefinition {
            ratio: 1.0,
            detune_cents: 0.0,
            level: 0.0,
            modulation_amount: 0.0,
            feedback: 0.0,
            phase: 0.0,
            envelope: crate::definition::AdsrDefinition {
                attack_seconds: 0.0,
                decay_seconds: 0.1,
                sustain_level: 0.5,
                release_seconds: 0.1,
            },
        };
        let mut operators = vec![operator; 4];
        operators[0].level = 0.8;
        operators[1].modulation_amount = 2.0;
        operators[2].modulation_amount = 1.0;
        operators[3].modulation_amount = 0.5;
        source.layers[0].generator = GeneratorDefinition::OperatorModulation(
            crate::definition::OperatorModulationDefinition {
                mode: crate::definition::OperatorModulationMode::Phase,
                algorithm: crate::definition::OperatorAlgorithm::Stack4,
                operators,
                phase_reset: true,
                unison: Some(crate::definition::UnisonDefinition {
                    voices: 4,
                    detune_cents: 12.0,
                    stereo_spread: 0.7,
                    phase_spread: 0.4,
                }),
            },
        );

        let catalog = ParameterCatalog::from_definition(&source);
        let operator_one_level = catalog
            .parameters()
            .iter()
            .find(|parameter| parameter.id == "layer.body.generator.operator.1.level")
            .expect("carrier level descriptor");
        assert_eq!(operator_one_level.unit, ParameterUnit::Normalized);
        assert!((operator_one_level.default - 0.8).abs() < f32::EPSILON);
        assert!(
            catalog
                .parameters()
                .iter()
                .all(|parameter| parameter.id != "layer.body.generator.operator.4.level")
        );

        let operator_two_amount = catalog
            .parameters()
            .iter()
            .find(|parameter| parameter.id == "layer.body.generator.operator.2.modulation_amount")
            .expect("modulation amount descriptor");
        assert_eq!(operator_two_amount.unit, ParameterUnit::Index);
        assert!((operator_two_amount.max - 8.0).abs() < f32::EPSILON);
        assert!((operator_two_amount.smoothing_seconds - 0.005).abs() < f32::EPSILON);
        assert!(
            catalog
                .parameters()
                .iter()
                .any(|parameter| parameter.id == "layer.body.generator.operator.4.feedback")
        );
        assert!(
            catalog
                .parameters()
                .iter()
                .any(|parameter| parameter.id == "layer.body.generator.unison_spread")
        );
    }

    #[test]
    fn linear_and_logarithmic_mappings_round_trip() {
        let linear = ParameterDescriptor {
            id: "pan".to_owned(),
            owner: ParameterOwner::Layer {
                definition_index: 0,
            },
            unit: ParameterUnit::Pan,
            scale: ParameterScale::Linear,
            min: -1.0,
            max: 1.0,
            default: 0.0,
            smoothing_seconds: 0.0,
        };
        assert!((linear.normalize(0.0).expect("normalizes") - 0.5).abs() < 1.0e-6);
        assert!((linear.denormalize(0.5).expect("denormalizes")).abs() < 1.0e-6);

        let log = ParameterDescriptor {
            id: "cutoff".to_owned(),
            owner: ParameterOwner::Layer {
                definition_index: 0,
            },
            unit: ParameterUnit::Hertz,
            scale: ParameterScale::Log2,
            min: 20.0,
            max: 20_000.0,
            default: 1_000.0,
            smoothing_seconds: 0.0,
        };
        let normalized = log.normalize(1_000.0).expect("normalizes");
        assert!((log.denormalize(normalized).expect("denormalizes") - 1_000.0).abs() < 0.01);
    }

    #[test]
    fn feedback_and_decay_descriptors_bound_dynamic_values() {
        let mut source = definition();
        source.global_processors = vec![
            ProcessorDefinition::Delay(crate::definition::DelayProcessorDefinition {
                id: "echo".to_owned(),
                time_seconds: 0.25,
                feedback: 0.5,
                mix: 0.5,
            }),
            ProcessorDefinition::Reverb(crate::definition::ReverbProcessorDefinition {
                id: "space".to_owned(),
                pre_delay_seconds: 0.02,
                decay: 0.5,
                damping: 0.2,
                width: 1.0,
                mix: 0.3,
            }),
        ];
        let catalog = ParameterCatalog::from_definition(&source);

        let delay_feedback = catalog
            .parameters()
            .iter()
            .find(|parameter| parameter.id == "global.processor.echo.feedback")
            .expect("delay feedback descriptor");
        let reverb_decay = catalog
            .parameters()
            .iter()
            .find(|parameter| parameter.id == "global.processor.space.decay")
            .expect("reverb decay descriptor");

        assert!((delay_feedback.max - 0.95).abs() < f32::EPSILON);
        assert!((reverb_decay.max - 0.98).abs() < f32::EPSILON);
        assert!((delay_feedback.denormalize(1.0).expect("delay max") - 0.95).abs() < f32::EPSILON);
        assert!((reverb_decay.denormalize(1.0).expect("reverb max") - 0.98).abs() < f32::EPSILON);
    }

    #[test]
    fn component_ids_follow_the_stable_grammar() {
        assert!(is_component_id("body_2"));
        assert!(!is_component_id("Body"));
        assert!(!is_component_id("body.part"));
        assert!(!is_component_id(""));
    }

    #[test]
    fn parameter_ids_follow_the_canonical_target_grammar() {
        for value in [
            "layer.body.gain",
            "layer.attack_2.pan",
            "layer.body.tuning",
            "layer.body.processor.tone.cutoff",
            "layer.body.processor.tone.resonance",
            "voice.processor.tone.cutoff",
            "global.processor.space.mix",
        ] {
            assert!(is_parameter_id(value), "{value} should be valid");
        }
        for value in [
            "",
            "layer.body",
            "layer.body.gain.extra",
            "layer.Body.gain",
            "layer.body.unknown",
            "layer..gain",
            "voice.processor",
            "voice.processor.tone.cutoff.extra",
            "voice.Filter.cutoff",
        ] {
            assert!(!is_parameter_id(value), "{value} should be invalid");
        }
    }
}
