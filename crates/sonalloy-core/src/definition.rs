use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::parameter::{BUILTIN_SOURCE_IDS, is_component_id};

/// The Definition schema accepted by the compiler.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Stable identifier assigned to a layer.
pub type LayerId = String;

/// JSON source model for an instrument.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentDefinition {
    /// Definition schema version.
    pub schema_version: u32,
    /// Human-readable instrument information.
    pub metadata: InstrumentMetadata,
    /// Polyphony and allocation policy.
    pub performance: PerformanceDefinition,
    /// Ordered layer definitions.
    pub layers: Vec<LayerDefinition>,
    /// Optional filter applied after the voice layer mix.
    pub voice_filter: Option<FilterDefinition>,
    /// Optional modulation sources and routes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modulation: Option<ModulationDefinition>,
}

/// Human-readable instrument information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentMetadata {
    /// Instrument name.
    pub name: String,
    /// Optional author name.
    #[serde(default)]
    pub author: Option<String>,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
}

/// Performance settings owned by the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceDefinition {
    /// Maximum number of simultaneous voices.
    pub polyphony: u16,
    /// Voice stealing policy.
    pub voice_stealing: VoiceStealingDefinition,
}

/// Voice stealing policies supported by the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceStealingDefinition {
    /// Prefer the quietest releasing voice, then the oldest active voice.
    QuietestReleasingThenOldest,
}

/// A source layer and its trigger/mix settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerDefinition {
    /// Stable layer identifier.
    pub id: LayerId,
    /// Whether this layer participates in the compiled instrument.
    pub enabled: bool,
    /// Key and velocity trigger conditions.
    pub trigger: LayerTriggerDefinition,
    /// Layer gain in decibels.
    pub gain_db: f32,
    /// Constant-power pan position, from left (-1) to right (1).
    pub pan: f32,
    /// Layer tuning offset in cents.
    pub tuning_cents: f32,
    /// Layer envelope settings.
    pub envelope: AdsrDefinition,
    /// Layer generator.
    pub generator: GeneratorDefinition,
}

/// Conditions evaluated once when a Note On is received.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerTriggerDefinition {
    /// Lowest MIDI note accepted by the layer.
    pub key_min: u8,
    /// Highest MIDI note accepted by the layer.
    pub key_max: u8,
    /// Lowest MIDI velocity accepted by the layer.
    pub velocity_min: u8,
    /// Highest MIDI velocity accepted by the layer.
    pub velocity_max: u8,
}

/// Generator variants in the Definition model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratorDefinition {
    /// A DaisySP-backed oscillator.
    Oscillator(OscillatorDefinition),
    /// A one-shot sample loaded during compilation.
    Sample(SampleDefinition),
}

/// Oscillator generator settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OscillatorDefinition {
    /// Selected waveform.
    pub waveform: OscillatorWaveform,
    /// Whether every Note On starts at the engine's initial phase.
    pub phase_reset: bool,
}

/// Sample generator settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleDefinition {
    /// Referenced audio asset.
    pub asset: AssetReference,
    /// MIDI note represented by the source recording.
    pub root_note: u8,
    /// Sample playback mode.
    pub playback_mode: SamplePlaybackMode,
    /// Sample interpolation mode.
    pub interpolation: SampleInterpolation,
}

/// A source file referenced by a Definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetReference {
    /// Relative or absolute path to the asset.
    pub path: String,
    /// Optional SHA-256 digest of the file bytes.
    #[serde(default)]
    pub sha256: Option<String>,
}

/// Supported sample playback modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SamplePlaybackMode {
    /// Play the source once from the beginning.
    OneShot,
}

/// Supported sample interpolation modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleInterpolation {
    /// Four-point cubic interpolation.
    Cubic,
}

/// Oscillator waveforms exposed by Sonalloy, independent of `DaisySP` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OscillatorWaveform {
    /// Sinusoidal oscillator.
    Sine,
    /// Band-limited saw oscillator.
    Saw,
}

/// ADSR envelope values in seconds and normalized amplitude.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdsrDefinition {
    /// Attack duration in seconds.
    pub attack_seconds: f32,
    /// Decay duration in seconds.
    pub decay_seconds: f32,
    /// Sustain amplitude.
    pub sustain_level: f32,
    /// Release duration in seconds.
    pub release_seconds: f32,
}

/// Voice low-pass filter settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterDefinition {
    /// Cutoff frequency in Hz.
    pub cutoff_hz: f32,
    /// Normalized resonance.
    pub resonance: f32,
}

/// Modulation sources and routes stored in a Definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulationDefinition {
    /// User-defined source definitions.
    pub sources: Vec<ModulationSourceDefinition>,
    /// Source-to-parameter connections.
    pub routes: Vec<ModulationRouteDefinition>,
}

/// A user-defined voice-scoped modulation source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModulationSourceDefinition {
    /// Periodic bipolar source.
    Lfo(LfoDefinition),
    /// Note lifecycle envelope source.
    Envelope(ModEnvelopeDefinition),
    /// Deterministic note-scoped sample-and-hold source.
    Random(RandomDefinition),
}

/// LFO source settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LfoDefinition {
    /// Stable source identifier.
    pub id: String,
    /// LFO waveform.
    pub waveform: LfoWaveform,
    /// Frequency in hertz.
    pub rate_hz: f32,
    /// Initial phase in the half-open zero-to-one range.
    pub phase: f32,
}

/// LFO waveform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LfoWaveform {
    /// Sine waveform.
    Sine,
    /// Bipolar triangle waveform.
    Triangle,
}

/// Note lifecycle modulation envelope settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModEnvelopeDefinition {
    /// Stable source identifier.
    pub id: String,
    /// Attack duration in seconds.
    pub attack_seconds: f32,
    /// Decay duration in seconds.
    pub decay_seconds: f32,
    /// Sustain level.
    pub sustain_level: f32,
    /// Release duration in seconds.
    pub release_seconds: f32,
}

/// Note-scoped deterministic random source settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RandomDefinition {
    /// Stable source identifier.
    pub id: String,
    /// Explicit deterministic seed.
    pub seed: u64,
}

/// Source-to-target modulation connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulationRouteDefinition {
    /// Source identifier, including built-in source identifiers.
    pub source: String,
    /// Canonical parameter identifier.
    pub target: String,
    /// Signed amount in target-range units.
    pub amount: f32,
    /// Source shaping curve.
    pub curve: ModulationCurve,
}

/// Curve applied to a source before its route amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulationCurve {
    /// No shaping.
    Linear,
    /// Smooth interpolation around zero.
    SmoothStep,
}

impl InstrumentDefinition {
    /// Validate the Definition without resolving files or allocating runtime state.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn validate(&self) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::SchemaUnsupported,
                    format!(
                        "unsupported schema_version {}, expected {}",
                        self.schema_version, CURRENT_SCHEMA_VERSION
                    ),
                )
                .with_path("schema_version"),
            );
        }
        if self.metadata.name.trim().is_empty() {
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::ValueOutOfRange, "name must not be empty")
                    .with_path("metadata.name"),
            );
        }
        if !(1..=64).contains(&self.performance.polyphony) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "polyphony must be between 1 and 64",
                )
                .with_path("performance.polyphony"),
            );
        }
        if self.layers.is_empty() {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::RequiredFieldMissing,
                    "at least one layer is required",
                )
                .with_path("layers"),
            );
        }
        let mut ids = HashSet::new();
        for (index, layer) in self.layers.iter().enumerate() {
            let path = format!("layers[{index}]");
            if layer.id.trim().is_empty() {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::RequiredFieldMissing,
                        "layer id must not be empty",
                    )
                    .with_path(format!("{path}.id")),
                );
            }
            if !is_component_id(&layer.id) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ParameterIdInvalid,
                        "layer id must start with a lowercase letter and contain only lowercase letters, digits, or underscores",
                    )
                    .with_path(format!("{path}.id")),
                );
            }
            if !ids.insert(layer.id.clone()) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticCode::IdDuplicated, "layer id must be unique")
                        .with_path(format!("{path}.id")),
                );
            }
            validate_trigger(&mut diagnostics, &path, layer.trigger);
            validate_range(
                &mut diagnostics,
                format!("{path}.gain_db"),
                layer.gain_db,
                -60.0..=12.0,
                "gain_db must be finite and between -60 and 12 dB",
            );
            validate_range(
                &mut diagnostics,
                format!("{path}.pan"),
                layer.pan,
                -1.0..=1.0,
                "pan must be finite and between -1 and 1",
            );
            validate_range(
                &mut diagnostics,
                format!("{path}.tuning_cents"),
                layer.tuning_cents,
                -1200.0..=1200.0,
                "tuning_cents must be finite and between -1200 and 1200",
            );
            validate_adsr(&mut diagnostics, &path, layer.envelope);
            match &layer.generator {
                GeneratorDefinition::Oscillator(_) => {}
                GeneratorDefinition::Sample(sample) => {
                    validate_sample(&mut diagnostics, &path, sample);
                }
            }
        }

        if let Some(filter) = self.voice_filter {
            validate_range(
                &mut diagnostics,
                "voice_filter.cutoff_hz".to_owned(),
                filter.cutoff_hz,
                20.0..=20_000.0,
                "cutoff_hz must be finite and between 20 and 20000 Hz",
            );
            validate_range(
                &mut diagnostics,
                "voice_filter.resonance".to_owned(),
                filter.resonance,
                0.0..=1.0,
                "resonance must be finite and between 0 and 1",
            );
        }
        if let Some(modulation) = &self.modulation {
            validate_modulation(&mut diagnostics, modulation);
        }
        diagnostics
    }
}

fn validate_modulation(diagnostics: &mut Vec<Diagnostic>, modulation: &ModulationDefinition) {
    let mut source_ids = HashSet::new();
    for (index, source) in modulation.sources.iter().enumerate() {
        let id = match source {
            ModulationSourceDefinition::Lfo(value) => &value.id,
            ModulationSourceDefinition::Envelope(value) => &value.id,
            ModulationSourceDefinition::Random(value) => &value.id,
        };
        let path = format!("modulation.sources[{index}].id");
        if !is_component_id(id) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::SourceIdInvalid,
                    "source id has invalid format",
                )
                .with_path(path.clone()),
            );
        }
        if BUILTIN_SOURCE_IDS.contains(&id.as_str()) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::SourceIdDuplicated,
                    "user source id conflicts with a built-in source",
                )
                .with_path(path.clone()),
            );
        }
        if !source_ids.insert(id) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::SourceIdDuplicated,
                    "source id must be unique",
                )
                .with_path(path),
            );
        }
        match source {
            ModulationSourceDefinition::Lfo(value) => {
                validate_range(
                    diagnostics,
                    format!("modulation.sources[{index}].rate_hz"),
                    value.rate_hz,
                    0.01..=40.0,
                    "lfo rate must be finite and between 0.01 and 40 Hz",
                );
                if !value.phase.is_finite() || !(0.0..1.0).contains(&value.phase) {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::SourceValueInvalid,
                            "lfo phase must be finite and between 0 and 1",
                        )
                        .with_path(format!("modulation.sources[{index}].phase")),
                    );
                }
            }
            ModulationSourceDefinition::Envelope(value) => {
                validate_modulation_envelope(diagnostics, index, value);
            }
            ModulationSourceDefinition::Random(_) => {}
        }
    }
    for (index, route) in modulation.routes.iter().enumerate() {
        if route.source.trim().is_empty() || route.source.contains('.') {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::SourceIdInvalid,
                    "route source id is invalid",
                )
                .with_path(format!("modulation.routes[{index}].source")),
            );
        }
        if route.target.trim().is_empty() {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::RouteTargetInvalid,
                    "route target must not be empty",
                )
                .with_path(format!("modulation.routes[{index}].target")),
            );
        }
        validate_range(
            diagnostics,
            format!("modulation.routes[{index}].amount"),
            route.amount,
            -1.0..=1.0,
            "route amount must be finite and between -1 and 1",
        );
    }
}

fn validate_modulation_envelope(
    diagnostics: &mut Vec<Diagnostic>,
    index: usize,
    envelope: &ModEnvelopeDefinition,
) {
    for (field, value) in [
        ("attack_seconds", envelope.attack_seconds),
        ("decay_seconds", envelope.decay_seconds),
        ("release_seconds", envelope.release_seconds),
    ] {
        validate_range(
            diagnostics,
            format!("modulation.sources[{index}].{field}"),
            value,
            0.0..=30.0,
            "modulation envelope time must be finite and between 0 and 30 seconds",
        );
    }
    validate_range(
        diagnostics,
        format!("modulation.sources[{index}].sustain_level"),
        envelope.sustain_level,
        0.0..=1.0,
        "modulation envelope sustain must be finite and between 0 and 1",
    );
}

fn validate_sample(diagnostics: &mut Vec<Diagnostic>, path: &str, sample: &SampleDefinition) {
    if sample.asset.path.trim().is_empty() {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::RequiredFieldMissing,
                "sample asset path must not be empty",
            )
            .with_path(format!("{path}.generator.sample.asset.path")),
        );
    }
    if let Some(hash) = &sample.asset.sha256 {
        let valid = hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !valid {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "sample asset sha256 must be 64 hexadecimal characters",
                )
                .with_path(format!("{path}.generator.sample.asset.sha256")),
            );
        }
    }
}

fn validate_trigger(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    trigger: LayerTriggerDefinition,
) {
    if trigger.key_min > trigger.key_max || trigger.key_max > 127 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::LayerRangeInvalid,
                "key range must be between 0 and 127 with min <= max",
            )
            .with_path(format!("{path}.trigger")),
        );
    }
    if trigger.velocity_min == 0
        || trigger.velocity_min > trigger.velocity_max
        || trigger.velocity_max > 127
    {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::LayerRangeInvalid,
                "velocity range must be between 1 and 127 with min <= max",
            )
            .with_path(format!("{path}.trigger")),
        );
    }
}

fn validate_adsr(diagnostics: &mut Vec<Diagnostic>, path: &str, envelope: AdsrDefinition) {
    for (field, value) in [
        ("attack_seconds", envelope.attack_seconds),
        ("decay_seconds", envelope.decay_seconds),
        ("release_seconds", envelope.release_seconds),
    ] {
        validate_range(
            diagnostics,
            format!("{path}.envelope.{field}"),
            value,
            0.0..=30.0,
            "envelope time must be finite and between 0 and 30 seconds",
        );
    }
    validate_range(
        diagnostics,
        format!("{path}.envelope.sustain_level"),
        envelope.sustain_level,
        0.0..=1.0,
        "sustain_level must be finite and between 0 and 1",
    );
}

fn validate_range(
    diagnostics: &mut Vec<Diagnostic>,
    path: String,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    message: &str,
) {
    if !value.is_finite() || !range.contains(&value) {
        diagnostics
            .push(Diagnostic::error(DiagnosticCode::ValueOutOfRange, message).with_path(path));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn definition() -> InstrumentDefinition {
        InstrumentDefinition {
            schema_version: CURRENT_SCHEMA_VERSION,
            metadata: InstrumentMetadata {
                name: "Test".to_owned(),
                author: None,
                description: None,
            },
            performance: PerformanceDefinition {
                polyphony: 4,
                voice_stealing: VoiceStealingDefinition::QuietestReleasingThenOldest,
            },
            layers: vec![LayerDefinition {
                id: "body".to_owned(),
                enabled: true,
                trigger: LayerTriggerDefinition {
                    key_min: 0,
                    key_max: 127,
                    velocity_min: 1,
                    velocity_max: 127,
                },
                gain_db: -12.0,
                pan: 0.0,
                tuning_cents: 0.0,
                envelope: AdsrDefinition {
                    attack_seconds: 0.01,
                    decay_seconds: 0.2,
                    sustain_level: 0.7,
                    release_seconds: 0.3,
                },
                generator: GeneratorDefinition::Oscillator(OscillatorDefinition {
                    waveform: OscillatorWaveform::Sine,
                    phase_reset: true,
                }),
            }],
            voice_filter: None,
            modulation: None,
        }
    }

    #[test]
    fn valid_definition_has_no_diagnostics() {
        assert!(definition().validate().is_empty());
    }

    #[test]
    fn schema_and_duplicate_ids_use_specific_diagnostic_codes() {
        let mut value = definition();
        value.schema_version = CURRENT_SCHEMA_VERSION + 1;
        let mut duplicate = value.layers[0].clone();
        duplicate.enabled = false;
        value.layers.push(duplicate);
        let diagnostics = value.validate();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::SchemaUnsupported)
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::IdDuplicated)
        );
    }

    #[test]
    fn invalid_values_have_field_paths() {
        let mut value = definition();
        value.layers[0].pan = f32::NAN;
        value.layers[0].trigger.velocity_min = 0;
        let diagnostics = value.validate();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.path.as_deref() == Some("layers[0].pan") })
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.path.as_deref() == Some("layers[0].trigger") })
        );
    }

    #[test]
    fn trigger_rejects_values_above_midi_range() {
        let mut value = definition();
        value.layers[0].trigger.key_min = 128;
        value.layers[0].trigger.velocity_max = 200;
        let diagnostics = value.validate();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::LayerRangeInvalid
                && diagnostic.path.as_deref() == Some("layers[0].trigger")
        }));
    }

    #[test]
    fn validation_accepts_multiple_enabled_layers() {
        let mut value = definition();
        value.layers.push(value.layers[0].clone());
        value.layers[1].id = "second".to_owned();
        assert!(value.validate().is_empty());
    }

    #[test]
    fn serde_round_trip_preserves_definition() {
        let source = definition();
        let json = serde_json::to_string(&source).expect("definition serializes");
        let restored: InstrumentDefinition =
            serde_json::from_str(&json).expect("definition parses");
        assert_eq!(source, restored);
    }
}
