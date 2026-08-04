use std::collections::HashMap;

use serde::Serialize;
use thiserror::Error;

use crate::definition::{InstrumentDefinition, ProcessorDefinition};

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
