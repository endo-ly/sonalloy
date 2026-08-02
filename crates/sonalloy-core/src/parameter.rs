use std::collections::HashMap;

use serde::Serialize;
use thiserror::Error;

use crate::definition::InstrumentDefinition;

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
    /// A value belonging to the per-voice filter.
    VoiceFilter,
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
        let mut descriptors = Vec::with_capacity(definition.layers.len() * 3 + 2);
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
        }
        if let Some(filter) = definition.voice_filter {
            descriptors.push(ParameterDescriptor {
                id: "voice.filter.cutoff".to_owned(),
                owner: ParameterOwner::VoiceFilter,
                unit: ParameterUnit::Hertz,
                scale: ParameterScale::Log2,
                min: 20.0,
                max: 20_000.0,
                default: filter.cutoff_hz,
                smoothing_seconds: 0.010,
            });
            descriptors.push(ParameterDescriptor {
                id: "voice.filter.resonance".to_owned(),
                owner: ParameterOwner::VoiceFilter,
                unit: ParameterUnit::Normalized,
                scale: ParameterScale::Linear,
                min: 0.0,
                max: 1.0,
                default: filter.resonance,
                smoothing_seconds: 0.010,
            });
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

/// Build a canonical layer parameter identifier.
#[must_use]
pub fn layer_parameter_id(layer_id: &str, parameter: &str) -> String {
    format!("layer.{layer_id}.{parameter}")
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
    fn catalog_order_is_definition_order_then_filter() {
        let mut source = definition();
        source.voice_filter = Some(crate::definition::FilterDefinition {
            cutoff_hz: 1_000.0,
            resonance: 0.2,
        });
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
                "voice.filter.cutoff",
                "voice.filter.resonance"
            ]
        );
    }

    #[test]
    fn linear_and_logarithmic_mappings_round_trip() {
        let linear = ParameterDescriptor {
            id: "pan".to_owned(),
            owner: ParameterOwner::VoiceFilter,
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
            owner: ParameterOwner::VoiceFilter,
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
    fn component_ids_follow_the_stable_grammar() {
        assert!(is_component_id("body_2"));
        assert!(!is_component_id("Body"));
        assert!(!is_component_id("body.part"));
        assert!(!is_component_id(""));
    }
}
