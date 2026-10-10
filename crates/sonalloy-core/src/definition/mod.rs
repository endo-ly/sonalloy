use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::parameter::is_component_id;

mod assets;
mod generator;
mod metadata;
mod modulation;
mod processor;

pub(crate) use generator::MAX_PARTIALS;
pub use generator::*;
use generator::{
    validate_additive, validate_adsr, validate_formant, validate_granular, validate_modal,
    validate_noise, validate_operator_modulation, validate_oscillator, validate_physical_string,
    validate_sample, validate_spectral, validate_trigger, validate_wave_sequence,
    validate_wavetable,
};
use metadata::validate_metadata;
pub use metadata::*;
pub use modulation::*;
use modulation::{validate_macros, validate_modulation, validate_vectors};
pub(crate) use processor::MAX_DELAY_TAPS;
pub use processor::*;
use processor::{ProcessorPlacement, validate_processor_chain, validate_processor_resource_limits};

/// The Definition schema accepted by the compiler.
pub const CURRENT_SCHEMA_VERSION: u32 = 6;

/// Stable identifier assigned to a layer.
pub type LayerId = String;

/// JSON source model for an instrument.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentDefinition {
    /// Definition schema version.
    pub schema_version: u32,
    /// External audio bus requirement, when a consumer is configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_audio: Option<ExternalAudioInputDefinition>,
    /// Human-readable instrument information.
    pub metadata: InstrumentMetadata,
    /// Polyphony and allocation policy.
    pub performance: PerformanceDefinition,
    /// Ordered layer definitions.
    pub layers: Vec<LayerDefinition>,
    /// Ordered processors applied after each layer generator.
    #[serde(default)]
    pub voice_processors: Vec<ProcessorDefinition>,
    /// Ordered processors applied after the voice layer mix.
    #[serde(default)]
    pub global_processors: Vec<ProcessorDefinition>,
    /// Optional modulation sources and routes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modulation: Option<ModulationDefinition>,
    /// Stable instrument-level macro controls.
    #[serde(default)]
    pub macros: Vec<MacroDefinition>,
    /// Constant-power layer vector controls.
    #[serde(default)]
    pub vectors: Vec<VectorDefinition>,
}

/// External audio channel layout required by an instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalAudioInputDefinition {
    /// Required external input channel layout.
    pub channels: ExternalAudioChannels,
}

/// Supported external audio channel layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalAudioChannels {
    /// One external input channel.
    Mono,
    /// Two independent external input channels.
    Stereo,
}

impl ExternalAudioChannels {
    /// Return the number of planar samples required by this layout.
    #[must_use]
    pub const fn channel_count(self) -> usize {
        match self {
            Self::Mono => 1,
            Self::Stereo => 2,
        }
    }
}

/// Performance settings owned by the runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum PerformanceDefinition {
    /// Polyphonic voice allocation and stealing policy.
    Polyphonic {
        /// Maximum number of simultaneous voices.
        polyphony: u16,
        /// Voice stealing policy.
        voice_stealing: VoiceStealingDefinition,
        /// Key groups whose notes cut off one another.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        choke_groups: Vec<ChokeGroupDefinition>,
    },
    /// A single last-note-priority voice.
    Monophonic {
        /// Whether connected notes reuse the current voice state.
        legato: bool,
        /// Optional connected-note pitch glide.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        portamento: Option<PortamentoDefinition>,
    },
}

/// Monophonic portamento settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortamentoDefinition {
    /// Glide duration in seconds.
    pub time_seconds: f32,
}

/// MIDI keys whose notes stop every sounding note of the same group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChokeGroupDefinition {
    /// MIDI note numbers that belong to the group.
    pub keys: Vec<u8>,
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
    /// Optional nonempty group whose older sounding layers fade on Note On.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choke_group: Option<String>,
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
    /// Ordered processors applied to the layer generator.
    #[serde(default)]
    pub processors: Vec<ProcessorDefinition>,
}

/// Conditions evaluated once when the layer's trigger event is received.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerTriggerDefinition {
    /// Event at which this layer starts its generator.
    pub event: LayerTriggerEvent,
    /// Lowest MIDI note accepted by the layer.
    pub key_min: u8,
    /// Highest MIDI note accepted by the layer.
    pub key_max: u8,
    /// Lowest MIDI velocity accepted by the layer.
    pub velocity_min: u8,
    /// Highest MIDI velocity accepted by the layer.
    pub velocity_max: u8,
}

/// Layer trigger event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerTriggerEvent {
    /// Start the layer when the note begins.
    NoteOn,
    /// Arm the layer on Note On and start it when the note ends.
    NoteOff,
}

impl InstrumentDefinition {
    /// Return the number of external input channels required by the Definition.
    #[must_use]
    pub fn required_input_channels(&self) -> usize {
        match self.external_audio {
            Some(external_audio) => external_audio.channels.channel_count(),
            None => 0,
        }
    }

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
        validate_metadata(&mut diagnostics, &self.metadata);
        match &self.performance {
            PerformanceDefinition::Polyphonic {
                polyphony,
                choke_groups,
                ..
            } => {
                if !(1..=64).contains(polyphony) {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "polyphony must be between 1 and 64",
                        )
                        .with_path("performance.polyphony"),
                    );
                }
                validate_choke_groups(&mut diagnostics, choke_groups);
            }
            PerformanceDefinition::Monophonic { portamento, .. } => {
                if let Some(portamento) = portamento {
                    validate_range(
                        &mut diagnostics,
                        "performance.portamento.time_seconds".to_owned(),
                        portamento.time_seconds,
                        f32::EPSILON..=10.0,
                        "portamento time_seconds must be finite and greater than zero and at most 10 seconds",
                    );
                }
            }
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
            if let Some(group) = &layer.choke_group {
                if group.trim().is_empty() || layer.trigger.event != LayerTriggerEvent::NoteOn {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "choke_group must be nonempty and requires a note_on trigger",
                        )
                        .with_path(format!("{path}.choke_group")),
                    );
                }
            }
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
            validate_pitch_ratio(
                &mut diagnostics,
                format!("{path}.tuning_cents"),
                layer.tuning_cents,
            );
            validate_adsr(&mut diagnostics, &path, layer.envelope);
            validate_processor_chain(
                &mut diagnostics,
                &format!("{path}.processors"),
                &layer.processors,
                ProcessorPlacement::Layer,
            );
            match &layer.generator {
                GeneratorDefinition::Oscillator(oscillator) => {
                    validate_oscillator(&mut diagnostics, &path, oscillator);
                }
                GeneratorDefinition::Noise(noise) => {
                    validate_noise(&mut diagnostics, &path, noise);
                }
                GeneratorDefinition::PhysicalString(physical_string) => {
                    validate_physical_string(&mut diagnostics, &path, physical_string);
                }
                GeneratorDefinition::Modal(modal) => {
                    validate_modal(&mut diagnostics, &path, modal);
                }
                GeneratorDefinition::Additive(additive) => {
                    validate_additive(&mut diagnostics, &path, additive);
                }
                GeneratorDefinition::Formant(formant) => {
                    validate_formant(&mut diagnostics, &path, formant);
                }
                GeneratorDefinition::Sample(sample) => {
                    validate_sample(&mut diagnostics, &path, sample);
                }
                GeneratorDefinition::Granular(granular) => {
                    validate_granular(&mut diagnostics, &path, granular);
                }
                GeneratorDefinition::WaveSequence(sequence) => {
                    validate_wave_sequence(&mut diagnostics, &path, sequence);
                }
                GeneratorDefinition::Wavetable(wavetable) => {
                    validate_wavetable(&mut diagnostics, &path, wavetable);
                }
                GeneratorDefinition::Spectral(spectral) => {
                    validate_spectral(&mut diagnostics, &path, spectral);
                }
                GeneratorDefinition::OperatorModulation(operator_modulation) => {
                    validate_operator_modulation(&mut diagnostics, &path, operator_modulation);
                }
            }
        }

        validate_processor_chain(
            &mut diagnostics,
            "voice_processors",
            &self.voice_processors,
            ProcessorPlacement::Voice,
        );
        validate_processor_chain(
            &mut diagnostics,
            "global_processors",
            &self.global_processors,
            ProcessorPlacement::Global,
        );
        validate_processor_resource_limits(&mut diagnostics, &self.global_processors);
        if let Some(modulation) = &self.modulation {
            validate_modulation(&mut diagnostics, modulation);
        }
        validate_external_audio_usage(&mut diagnostics, self);
        validate_macros(&mut diagnostics, &self.macros);
        validate_vectors(&mut diagnostics, &self.vectors, &self.layers);
        diagnostics
    }
}

#[derive(Default)]
struct ExternalAudioUsage {
    followers: usize,
    vocoders: usize,
    envelope_transfers: usize,
    spectral_morphs: usize,
    external_detectors: usize,
}

impl ExternalAudioUsage {
    fn consumer_count(&self) -> usize {
        self.followers
            + self.vocoders
            + self.envelope_transfers
            + self.spectral_morphs
            + self.external_detectors
    }
}

fn validate_external_audio_usage(
    diagnostics: &mut Vec<Diagnostic>,
    definition: &InstrumentDefinition,
) {
    let mut usage = ExternalAudioUsage::default();
    if let Some(modulation) = &definition.modulation {
        usage.followers = modulation
            .sources
            .iter()
            .filter(|source| matches!(source, ModulationSourceDefinition::EnvelopeFollower(_)))
            .count();
    }
    for processor in definition
        .voice_processors
        .iter()
        .chain(definition.global_processors.iter())
    {
        match processor {
            ProcessorDefinition::Vocoder(_) => usage.vocoders += 1,
            ProcessorDefinition::EnvelopeTransfer(_) => usage.envelope_transfers += 1,
            ProcessorDefinition::SpectralMorph(_) => usage.spectral_morphs += 1,
            ProcessorDefinition::Gate(value)
                if matches!(value.detector, DynamicsDetectorDefinition::ExternalAudio) =>
            {
                usage.external_detectors += 1;
            }
            ProcessorDefinition::Compressor(value)
                if matches!(value.detector, DynamicsDetectorDefinition::ExternalAudio) =>
            {
                usage.external_detectors += 1;
            }
            _ => {}
        }
    }
    if usage.vocoders > 1 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::GeneratorResourceLimitExceeded,
                "global processors may contain at most one vocoder processor",
            )
            .with_path("global_processors"),
        );
    }
    if usage.spectral_morphs > 1 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::GeneratorResourceLimitExceeded,
                "global processors may contain at most one spectral_morph processor",
            )
            .with_path("global_processors"),
        );
    }
    match (definition.external_audio.is_some(), usage.consumer_count()) {
        (false, count) if count > 0 => diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::RequiredFieldMissing,
                "external_audio is required by an external audio consumer",
            )
            .with_path("external_audio"),
        ),
        (true, 0) => diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::DefinitionError,
                "external_audio must be used by an external audio consumer",
            )
            .with_path("external_audio"),
        ),
        _ => {}
    }
}

fn validate_pitch_ratio(diagnostics: &mut Vec<Diagnostic>, path: String, cents: f32) {
    let ratio = 2.0_f32.powf(cents / 1200.0);
    if !cents.is_finite() || !ratio.is_finite() || ratio <= 0.0 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "pitch offset must produce a finite positive ratio",
            )
            .with_path(path),
        );
    }
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

fn validate_finite(diagnostics: &mut Vec<Diagnostic>, path: String, value: f32, message: &str) {
    if !value.is_finite() {
        diagnostics
            .push(Diagnostic::error(DiagnosticCode::RouteDepthInvalid, message).with_path(path));
    }
}

fn validate_choke_groups(diagnostics: &mut Vec<Diagnostic>, groups: &[ChokeGroupDefinition]) {
    let mut assigned = HashSet::new();
    for (group_index, group) in groups.iter().enumerate() {
        let path = format!("performance.choke_groups[{group_index}].keys");
        if group.keys.is_empty() {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::RequiredFieldMissing,
                    "choke group must contain at least one key",
                )
                .with_path(path.clone()),
            );
        }
        for (key_index, key) in group.keys.iter().enumerate() {
            let key_path = format!("{path}[{key_index}]");
            if *key > 127 {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "choke group key must be between 0 and 127",
                    )
                    .with_path(key_path),
                );
            } else if !assigned.insert(*key) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::IdDuplicated,
                        format!("choke group key {key} is already assigned"),
                    )
                    .with_path(key_path),
                );
            }
        }
    }
}

fn range_message(field: &str, min: f32, max: f32) -> String {
    format!("{field} must be finite and between {min} and {max}")
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::definition::{
        AdsrDefinition, CURRENT_SCHEMA_VERSION, ChokeGroupDefinition,
        EnvelopeTransferProcessorDefinition, ExternalAudioChannels, ExternalAudioInputDefinition,
        GeneratorDefinition, InstrumentDefinition, InstrumentMetadata, LayerDefinition,
        LayerTriggerDefinition, LayerTriggerEvent, OscillatorDefinition, OscillatorWaveform,
        PerformanceDefinition, ProcessorDefinition, VoiceStealingDefinition,
    };
    use crate::diagnostics::DiagnosticCode;

    pub(crate) fn definition() -> InstrumentDefinition {
        InstrumentDefinition {
            schema_version: CURRENT_SCHEMA_VERSION,
            external_audio: None,
            metadata: InstrumentMetadata {
                name: "Test".to_owned(),
                author: None,
                description: None,
                category: None,
                tags: Vec::new(),
                recommended_range: None,
                preview: None,
            },
            performance: PerformanceDefinition::Polyphonic {
                polyphony: 4,
                voice_stealing: VoiceStealingDefinition::QuietestReleasingThenOldest,
                choke_groups: Vec::new(),
            },
            layers: vec![LayerDefinition {
                id: "body".to_owned(),
                enabled: true,
                trigger: LayerTriggerDefinition {
                    event: LayerTriggerEvent::NoteOn,
                    key_min: 0,
                    key_max: 127,
                    velocity_min: 1,
                    velocity_max: 127,
                },
                gain_db: -12.0,
                choke_group: None,
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
                    phase: 0.0,
                    hard_sync: None,
                    waveshaping: None,
                    phase_distortion: None,
                    wavefold: None,
                    feedback: None,
                    unison: None,
                }),
                processors: Vec::new(),
            }],
            voice_processors: Vec::new(),
            global_processors: Vec::new(),
            modulation: None,
            macros: Vec::new(),
            vectors: Vec::new(),
        }
    }

    #[test]
    fn valid_definition_has_no_diagnostics() {
        assert_eq!(definition().validate(), Vec::new());
    }

    #[test]
    fn choke_group_requires_a_name_and_note_on_trigger() {
        let mut source = definition();
        for (group, event) in [
            ("", LayerTriggerEvent::NoteOn),
            ("hat", LayerTriggerEvent::NoteOff),
        ] {
            source.layers[0].choke_group = Some(group.to_owned());
            source.layers[0].trigger.event = event;
            assert!(
                source
                    .validate()
                    .iter()
                    .any(|diagnostic| diagnostic.path.as_deref() == Some("layers[0].choke_group"))
            );
        }
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

        let mut old = definition();
        old.schema_version = CURRENT_SCHEMA_VERSION - 1;
        assert!(
            old.validate()
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::SchemaUnsupported)
        );
    }

    #[test]
    fn invalid_values_have_field_paths() {
        let mut value = definition();
        value.layers[0].pan = f32::NAN;
        value.layers[0].tuning_cents = f32::MAX;
        value.layers[0].trigger.velocity_min = 0;
        let diagnostics = value.validate();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.path.as_deref() == Some("layers[0].tuning_cents"))
        );
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
    fn choke_groups_reject_empty_groups_and_shared_keys() {
        let mut value = definition();
        value.performance = PerformanceDefinition::Polyphonic {
            polyphony: 4,
            voice_stealing: VoiceStealingDefinition::QuietestReleasingThenOldest,
            choke_groups: vec![
                ChokeGroupDefinition { keys: vec![42, 46] },
                ChokeGroupDefinition { keys: Vec::new() },
                ChokeGroupDefinition {
                    keys: vec![46, 128],
                },
            ],
        };

        let diagnostics = value.validate();

        let located = |code, path: &str| {
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.path.as_deref() == Some(path)
            })
        };
        assert!(located(
            DiagnosticCode::RequiredFieldMissing,
            "performance.choke_groups[1].keys"
        ));
        assert!(located(
            DiagnosticCode::IdDuplicated,
            "performance.choke_groups[2].keys[0]"
        ));
        assert!(located(
            DiagnosticCode::ValueOutOfRange,
            "performance.choke_groups[2].keys[1]"
        ));
        assert_eq!(diagnostics.len(), 3);
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
        assert_eq!(value.validate(), Vec::new());
    }

    #[test]
    fn external_audio_requires_a_declared_and_used_bus() {
        let transfer = ProcessorDefinition::EnvelopeTransfer(EnvelopeTransferProcessorDefinition {
            id: "transfer".to_owned(),
            attack_ms: 2.0,
            release_ms: 120.0,
            input_gain_db: 0.0,
            floor_db: -72.0,
            mix: 1.0,
        });

        let mut missing_bus = definition();
        missing_bus.global_processors = vec![transfer.clone()];
        assert!(missing_bus.validate().iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::RequiredFieldMissing
                && diagnostic.path.as_deref() == Some("external_audio")
        }));

        let mut unused_bus = definition();
        unused_bus.external_audio = Some(ExternalAudioInputDefinition {
            channels: ExternalAudioChannels::Mono,
        });
        assert!(unused_bus.validate().iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::DefinitionError
                && diagnostic.path.as_deref() == Some("external_audio")
        }));

        let mut used_bus = unused_bus;
        used_bus.global_processors = vec![transfer];
        assert_eq!(used_bus.validate(), Vec::new());
    }
}
