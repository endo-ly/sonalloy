mod catalog;
pub(crate) mod generator;

pub use catalog::ParameterCatalog;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use self::generator::{
    ADDITIVE_INHARMONICITY, ADDITIVE_MORPH, ADDITIVE_SPECTRUM_TILT, FORMANT_SHIFT,
    FORMANT_SPECTRAL_TILT, FORMANT_THROAT, FORMANT_VOWEL_POSITION, GRAIN_DENSITY, GRAIN_PAN_SPREAD,
    GRAIN_PITCH, GRAIN_RANDOMNESS, GRAIN_SIZE, GRANULAR_POSITION, GeneratorParameterSpec,
    MODAL_BRIGHTNESS, MODAL_DECAY, MODAL_STRUCTURE, NOISE_CORRELATION, OPERATOR_AM_RING_AMOUNT_MAX,
    OPERATOR_AM_RING_AMOUNT_MIN, OPERATOR_DETUNE_MAX, OPERATOR_DETUNE_MIN, OPERATOR_FEEDBACK_MAX,
    OPERATOR_FEEDBACK_MIN, OPERATOR_LEVEL_MAX, OPERATOR_LEVEL_MIN,
    OPERATOR_PARAMETER_SMOOTHING_SECONDS, OPERATOR_PARAMETER_SUFFIXES,
    OPERATOR_PHASE_FREQUENCY_AMOUNT_MAX, OPERATOR_PHASE_FREQUENCY_AMOUNT_MIN, OPERATOR_RATIO_MAX,
    OPERATOR_RATIO_MIN, OSCILLATOR_FEEDBACK, PHASE_DISTORTION, PHYSICAL_STRING_BRIGHTNESS,
    PHYSICAL_STRING_DECAY_SECONDS, PHYSICAL_STRING_STIFFNESS, PULSE_WIDTH, SPECTRAL_BLUR,
    SPECTRAL_FREEZE, SPECTRAL_MORPH, SPECTRAL_POSITION, SPECTRAL_SHIFT, SYNC_RATIO, UNISON_DETUNE,
    UNISON_SPREAD, WAVEFOLD, WAVESHAPE, WAVETABLE_POSITION,
};
use crate::definition::{
    GeneratorDefinition, InstrumentDefinition, OperatorModulationMode, OscillatorWaveform,
    ProcessorDefinition,
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
    /// A normalized instrument macro.
    Macro {
        /// Definition macro index.
        macro_index: usize,
    },
    /// A normalized vector axis.
    VectorAxis {
        /// Definition vector index.
        vector_index: usize,
        /// Axis represented by the parameter.
        axis: VectorAxis,
    },
}

/// Axis represented by a vector parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorAxis {
    /// Two-way position axis.
    Position,
    /// Four-way horizontal axis.
    X,
    /// Four-way vertical axis.
    Y,
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

/// Unit used to express a signed modulation depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulationUnit {
    /// Gain change in decibels.
    Decibels,
    /// Constant-power pan change.
    Pan,
    /// Tuning change in cents.
    Cents,
    /// Additive frequency change in hertz.
    Hertz,
    /// Additive duration change in seconds.
    Seconds,
    /// Additive rate change per second.
    PerSecond,
    /// Additive synthesis index change.
    Index,
    /// Spectral tilt change in decibels per octave.
    DecibelsPerOctave,
    /// Additive change in a normalized parameter.
    Normalized,
    /// Base-two logarithmic change.
    Octaves,
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
    /// Return the native unit used by modulation routes targeting this parameter.
    ///
    /// # Panics
    ///
    /// Panics when a descriptor contains a unit and scale combination that is not part of the
    /// parameter contract.
    #[must_use]
    pub fn modulation_unit(&self) -> ModulationUnit {
        match self.unit {
            ParameterUnit::Decibels if self.scale == ParameterScale::Linear => {
                ModulationUnit::Decibels
            }
            ParameterUnit::Pan if self.scale == ParameterScale::Linear => ModulationUnit::Pan,
            ParameterUnit::Cents if self.scale == ParameterScale::Linear => ModulationUnit::Cents,
            ParameterUnit::Hertz => match self.scale {
                ParameterScale::Linear => ModulationUnit::Hertz,
                ParameterScale::Log2 => ModulationUnit::Octaves,
            },
            ParameterUnit::Ratio if self.scale == ParameterScale::Log2 => ModulationUnit::Octaves,
            ParameterUnit::Seconds => match self.scale {
                ParameterScale::Linear => ModulationUnit::Seconds,
                ParameterScale::Log2 => ModulationUnit::Octaves,
            },
            ParameterUnit::PerSecond => match self.scale {
                ParameterScale::Linear => ModulationUnit::PerSecond,
                ParameterScale::Log2 => ModulationUnit::Octaves,
            },
            ParameterUnit::Index if self.scale == ParameterScale::Linear => ModulationUnit::Index,
            ParameterUnit::DecibelsPerOctave if self.scale == ParameterScale::Linear => {
                ModulationUnit::DecibelsPerOctave
            }
            ParameterUnit::Normalized if self.scale == ParameterScale::Linear => {
                ModulationUnit::Normalized
            }
            unit => panic!(
                "unsupported parameter unit/scale combination: {unit:?}/{:?}",
                self.scale
            ),
        }
    }

    /// Return the greatest absolute modulation depth representable by this parameter.
    #[must_use]
    pub fn max_modulation_depth(&self) -> f32 {
        match self.scale {
            ParameterScale::Linear => self.max - self.min,
            ParameterScale::Log2 => (self.max / self.min).log2(),
        }
    }

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
            is_component_id(layer_id) && generator::is_suffix(parameter)
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
        ["macro", macro_id] => is_component_id(macro_id),
        ["vector", vector_id, axis] => {
            is_component_id(vector_id) && matches!(*axis, "position" | "x" | "y")
        }
        _ => false,
    }
}

fn is_processor_parameter(value: &str) -> bool {
    matches!(
        value,
        "cutoff"
            | "resonance"
            | "drive"
            | "amount"
            | "mix"
            | "feedback"
            | "decay"
            | "damping"
            | "width"
            | "low_gain_db"
            | "mid_gain_db"
            | "high_gain_db"
            | "frequency_hz"
            | "shift_hz"
            | "gain_db"
            | "decay_seconds"
            | "bit_depth"
            | "sample_rate_ratio"
            | "rate_hz"
            | "depth"
            | "threshold_db"
            | "range_db"
            | "vowel_position"
            | "formant_shift"
            | "throat"
            | "attack"
            | "sustain"
            | "ratio"
            | "makeup_gain_db"
            | "ceiling_db"
            | "input_gain_db"
            | "modulator_gain_db"
            | "output_gain_db"
            | "floor_db"
            | "morph"
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
    "transport_beat_phase",
    "transport_bar_phase",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::tests::definition;

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
    fn modulation_contract_reports_target_units_and_limits() {
        let mut source = definition();
        source.voice_processors.push(ProcessorDefinition::Filter(
            crate::definition::FilterProcessorDefinition {
                id: "tone".to_owned(),
                mode: crate::definition::FilterModeDefinition::LowPass,
                cutoff_hz: 1_000.0,
                resonance: 0.2,
            },
        ));
        let catalog = ParameterCatalog::from_definition(&source);

        let tuning = catalog
            .parameters()
            .iter()
            .find(|parameter| parameter.id == "layer.body.tuning")
            .expect("layer tuning descriptor");
        assert_eq!(tuning.modulation_unit(), ModulationUnit::Cents);
        assert!((tuning.max_modulation_depth() - 2_400.0).abs() < f32::EPSILON);

        let cutoff = catalog
            .parameters()
            .iter()
            .find(|parameter| parameter.id == "voice.processor.tone.cutoff")
            .expect("filter cutoff descriptor");
        assert_eq!(cutoff.modulation_unit(), ModulationUnit::Octaves);
        assert!((cutoff.max_modulation_depth() - 1_000.0_f32.log2()).abs() < 1.0e-6);

        let resonance = catalog
            .parameters()
            .iter()
            .find(|parameter| parameter.id == "voice.processor.tone.resonance")
            .expect("filter resonance descriptor");
        assert_eq!(resonance.modulation_unit(), ModulationUnit::Normalized);
        assert!((resonance.max_modulation_depth() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn logarithmic_duration_and_rate_use_octave_depth() {
        let descriptor = |unit, min, max| ParameterDescriptor {
            id: "test".to_owned(),
            owner: ParameterOwner::Layer {
                definition_index: 0,
            },
            unit,
            scale: ParameterScale::Log2,
            min,
            max,
            default: min,
            smoothing_seconds: 0.0,
        };

        let seconds = descriptor(ParameterUnit::Seconds, 0.01, 4.0);
        assert_eq!(seconds.modulation_unit(), ModulationUnit::Octaves);
        assert!((seconds.max_modulation_depth() - 400.0_f32.log2()).abs() < 1.0e-6);

        let rate = descriptor(ParameterUnit::PerSecond, 1.0, 256.0);
        assert_eq!(rate.modulation_unit(), ModulationUnit::Octaves);
        assert!((rate.max_modulation_depth() - 8.0).abs() < 1.0e-6);
    }

    #[test]
    fn feedback_and_decay_descriptors_bound_dynamic_values() {
        let mut source = definition();
        source.global_processors = vec![
            ProcessorDefinition::Delay(crate::definition::DelayProcessorDefinition {
                id: "echo".to_owned(),
                time: crate::definition::DelayTimeDefinition {
                    value: 0.25,
                    unit: crate::definition::DelayTimeUnit::Seconds,
                },
                feedback_mode: crate::definition::DelayFeedbackMode::Stereo,
                feedback: 0.5,
                taps: vec![],
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
