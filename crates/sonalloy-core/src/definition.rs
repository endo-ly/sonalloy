use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Deserializer, Serialize};

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::parameter::{BUILTIN_SOURCE_IDS, is_component_id, is_parameter_id};

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
    /// Ordered processors applied after each layer generator.
    #[serde(default)]
    pub voice_processors: Vec<ProcessorDefinition>,
    /// Ordered processors applied after the voice layer mix.
    #[serde(default)]
    pub global_processors: Vec<ProcessorDefinition>,
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
    /// Ordered processors applied to the layer generator.
    #[serde(default)]
    pub processors: Vec<ProcessorDefinition>,
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
    /// A deterministic stereo noise generator.
    Noise(NoiseDefinition),
    /// A mapped sample instrument loaded during compilation.
    Sample(SampleDefinition),
}

/// Oscillator generator settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OscillatorDefinition {
    /// Selected waveform.
    pub waveform: OscillatorWaveform,
    /// Whether every Note On starts at the engine's initial phase.
    pub phase_reset: bool,
    /// Initial oscillator phase in the inclusive zero-to-one range.
    pub phase: f32,
    /// Optional hard-sync configuration.
    #[serde(default)]
    pub hard_sync: Option<HardSyncDefinition>,
    /// Optional generator waveshaping configuration.
    #[serde(default)]
    pub waveshaping: Option<WaveshapingDefinition>,
    /// Optional unison configuration.
    #[serde(default)]
    pub unison: Option<UnisonDefinition>,
}

/// Hard-sync oscillator settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardSyncDefinition {
    /// Slave-to-master frequency ratio.
    pub ratio: f32,
}

/// Generator waveshaping settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaveshapingDefinition {
    /// Normalized waveshaping amount.
    pub amount: f32,
}

/// Oscillator unison settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnisonDefinition {
    /// Number of oscillator components.
    pub voices: u8,
    /// Maximum symmetric detune in cents.
    pub detune_cents: f32,
    /// Dynamic stereo spread.
    pub stereo_spread: f32,
    /// Static phase spread.
    pub phase_spread: f32,
}

/// Noise generator settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoiseDefinition {
    /// Spectral color of the generated noise.
    pub color: NoiseColor,
    /// Deterministic stream seed.
    pub seed: u64,
    /// Shared-to-independent stereo mix in the inclusive zero-to-one range.
    pub stereo_correlation: f32,
}

/// Sample generator settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleDefinition {
    /// Sample interpolation mode.
    pub interpolation: SampleInterpolation,
    /// Ordered key, velocity, and playback zones.
    pub zones: Vec<SampleZoneDefinition>,
}

/// A single mapped sample region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleZoneDefinition {
    /// Stable identifier within the Sample Generator.
    pub id: String,
    /// Referenced audio asset.
    pub asset: AssetReference,
    /// MIDI note represented by the source recording.
    pub root_note: u8,
    /// Lowest MIDI note accepted by the zone.
    pub key_min: u8,
    /// Highest MIDI note accepted by the zone.
    pub key_max: u8,
    /// Lowest MIDI velocity accepted by the zone.
    pub velocity_min: u8,
    /// Highest MIDI velocity accepted by the zone.
    pub velocity_max: u8,
    /// Optional deterministic Round Robin group.
    pub round_robin_group: Option<String>,
    /// Region and playback behavior.
    pub playback: SampleZonePlaybackDefinition,
}

/// Playback region owned by one Sample Zone.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SampleZonePlaybackDefinition {
    /// Play a finite region once.
    OneShot {
        /// Region start in source seconds.
        start_seconds: f32,
        /// Optional region end in source seconds.
        end_seconds: Option<f32>,
    },
    /// Repeat a region forward while the layer envelope is active.
    ForwardLoop {
        /// Region start in source seconds.
        start_seconds: f32,
        /// Optional region end in source seconds.
        end_seconds: Option<f32>,
        /// Loop start in source seconds.
        loop_start_seconds: f32,
        /// Loop end in source seconds.
        loop_end_seconds: f32,
    },
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

/// Supported sample interpolation modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleInterpolation {
    /// Four-point cubic interpolation.
    Cubic,
}

/// Oscillator waveforms exposed by Sonalloy, independent of `DaisySP` names.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OscillatorWaveform {
    /// Sinusoidal oscillator.
    Sine,
    /// Band-limited saw oscillator.
    Saw,
    /// Band-limited square oscillator with a fixed 50% duty cycle.
    Square,
    /// Band-limited triangle oscillator.
    Triangle,
    /// Band-limited square oscillator with a dynamic duty cycle.
    Pulse {
        /// Initial pulse width in the inclusive 0.05-to-0.95 range.
        pulse_width: f32,
    },
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OscillatorWaveformType {
    Sine,
    Saw,
    Square,
    Triangle,
    Pulse,
}

#[derive(Debug, Clone, Copy, Default)]
enum PulseWidthField {
    #[default]
    Absent,
    Null,
    Value(f32),
}

impl<'de> Deserialize<'de> for PulseWidthField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match Option::<f32>::deserialize(deserializer)? {
            Some(value) => Self::Value(value),
            None => Self::Null,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OscillatorWaveformObject {
    #[serde(rename = "type")]
    kind: OscillatorWaveformType,
    #[serde(default)]
    pulse_width: PulseWidthField,
}

impl<'de> Deserialize<'de> for OscillatorWaveform {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error as _;

        let object = OscillatorWaveformObject::deserialize(deserializer)?;
        match (object.kind, object.pulse_width) {
            (OscillatorWaveformType::Sine, PulseWidthField::Absent) => Ok(Self::Sine),
            (OscillatorWaveformType::Saw, PulseWidthField::Absent) => Ok(Self::Saw),
            (OscillatorWaveformType::Square, PulseWidthField::Absent) => Ok(Self::Square),
            (OscillatorWaveformType::Triangle, PulseWidthField::Absent) => Ok(Self::Triangle),
            (OscillatorWaveformType::Pulse, PulseWidthField::Value(pulse_width)) => {
                Ok(Self::Pulse { pulse_width })
            }
            (OscillatorWaveformType::Pulse, PulseWidthField::Absent | PulseWidthField::Null) => {
                Err(D::Error::missing_field("pulse_width"))
            }
            (_, PulseWidthField::Null | PulseWidthField::Value(_)) => Err(D::Error::custom(
                "pulse_width is only valid for the pulse waveform",
            )),
        }
    }
}

/// Noise colors exposed by the Definition model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoiseColor {
    /// Equal-energy white noise.
    White,
    /// Voss-McCartney pink noise.
    Pink,
    /// Leaky-integrated brown noise.
    Brown,
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

/// Processor definitions supported by the fixed signal pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProcessorDefinition {
    /// Low-pass filter.
    Filter(FilterProcessorDefinition),
    /// Soft-clipping drive.
    Drive(DriveProcessorDefinition),
    /// Stereo feedback delay.
    Delay(DelayProcessorDefinition),
    /// Stereo plate reverb.
    Reverb(ReverbProcessorDefinition),
}

impl ProcessorDefinition {
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Filter(value) => &value.id,
            Self::Drive(value) => &value.id,
            Self::Delay(value) => &value.id,
            Self::Reverb(value) => &value.id,
        }
    }
}

/// Low-pass filter processor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Cutoff frequency in Hz.
    pub cutoff_hz: f32,
    /// Normalized resonance.
    pub resonance: f32,
}

/// Soft-clipping drive processor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Soft-clipping amount.
    pub amount: f32,
    /// Dry/wet mix.
    pub mix: f32,
}

/// Stereo delay processor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelayProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Static delay time in seconds.
    pub time_seconds: f32,
    /// Feedback amount.
    pub feedback: f32,
    /// Dry/wet mix.
    pub mix: f32,
}

/// Stereo plate reverb processor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReverbProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Static pre-delay in seconds.
    pub pre_delay_seconds: f32,
    /// Decay amount.
    pub decay: f32,
    /// Damping amount.
    pub damping: f32,
    /// Wet stereo width.
    pub width: f32,
    /// Dry/wet mix.
    pub mix: f32,
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
                GeneratorDefinition::Sample(sample) => {
                    validate_sample(&mut diagnostics, &path, sample);
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
        if let Some(modulation) = &self.modulation {
            validate_modulation(&mut diagnostics, modulation);
        }
        diagnostics
    }
}

#[derive(Clone, Copy)]
enum ProcessorPlacement {
    Layer,
    Voice,
    Global,
}

fn validate_processor_chain(
    diagnostics: &mut Vec<Diagnostic>,
    base_path: &str,
    processors: &[ProcessorDefinition],
    placement: ProcessorPlacement,
) {
    let mut ids = HashSet::new();
    for (index, processor) in processors.iter().enumerate() {
        let path = format!("{base_path}[{index}]");
        let id_path = format!("{path}.id");
        if !is_component_id(processor.id()) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ProcessorIdInvalid,
                    "processor id must start with a lowercase letter and contain only lowercase letters, digits, or underscores",
                )
                .with_path(id_path.clone()),
            );
        }
        if !ids.insert(processor.id()) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ProcessorIdDuplicated,
                    "processor id must be unique within its chain",
                )
                .with_path(id_path),
            );
        }
        if let (
            ProcessorPlacement::Layer | ProcessorPlacement::Voice,
            ProcessorDefinition::Delay(_) | ProcessorDefinition::Reverb(_),
        ) = (placement, processor)
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ProcessorPlacementInvalid,
                    "delay and reverb processors are allowed only in global_processors",
                )
                .with_path(&path),
            );
        }
        validate_processor_values(diagnostics, &path, processor);
    }
}

fn validate_processor_values(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    processor: &ProcessorDefinition,
) {
    match processor {
        ProcessorDefinition::Filter(value) => {
            validate_range(
                diagnostics,
                format!("{path}.cutoff_hz"),
                value.cutoff_hz,
                20.0..=20_000.0,
                "cutoff_hz must be finite and between 20 and 20000 Hz",
            );
            validate_range(
                diagnostics,
                format!("{path}.resonance"),
                value.resonance,
                0.0..=1.0,
                "resonance must be finite and between 0 and 1",
            );
        }
        ProcessorDefinition::Drive(value) => {
            validate_range(
                diagnostics,
                format!("{path}.amount"),
                value.amount,
                0.0..=1.0,
                "amount must be finite and between 0 and 1",
            );
            validate_range(
                diagnostics,
                format!("{path}.mix"),
                value.mix,
                0.0..=1.0,
                "mix must be finite and between 0 and 1",
            );
        }
        ProcessorDefinition::Delay(value) => {
            validate_range(
                diagnostics,
                format!("{path}.time_seconds"),
                value.time_seconds,
                0.001..=2.0,
                "time_seconds must be finite and between 0.001 and 2 seconds",
            );
            validate_range(
                diagnostics,
                format!("{path}.feedback"),
                value.feedback,
                0.0..=0.95,
                "feedback must be finite and between 0 and 0.95",
            );
            validate_range(
                diagnostics,
                format!("{path}.mix"),
                value.mix,
                0.0..=1.0,
                "mix must be finite and between 0 and 1",
            );
        }
        ProcessorDefinition::Reverb(value) => {
            validate_range(
                diagnostics,
                format!("{path}.pre_delay_seconds"),
                value.pre_delay_seconds,
                0.0..=0.2,
                "pre_delay_seconds must be finite and between 0 and 0.2 seconds",
            );
            validate_range(
                diagnostics,
                format!("{path}.decay"),
                value.decay,
                0.0..=0.98,
                "decay must be finite and between 0 and 0.98",
            );
            validate_range(
                diagnostics,
                format!("{path}.damping"),
                value.damping,
                0.0..=1.0,
                "damping must be finite and between 0 and 1",
            );
            validate_range(
                diagnostics,
                format!("{path}.width"),
                value.width,
                0.0..=1.0,
                "width must be finite and between 0 and 1",
            );
            validate_range(
                diagnostics,
                format!("{path}.mix"),
                value.mix,
                0.0..=1.0,
                "mix must be finite and between 0 and 1",
            );
        }
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
        if !is_component_id(&route.source) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::SourceIdInvalid,
                    "route source id is invalid",
                )
                .with_path(format!("modulation.routes[{index}].source")),
            );
        }
        if !is_parameter_id(&route.target) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::RouteTargetInvalid,
                    "route target has invalid format",
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

#[allow(clippy::too_many_lines)]
fn validate_sample(diagnostics: &mut Vec<Diagnostic>, path: &str, sample: &SampleDefinition) {
    let sample_path = format!("{path}.generator.sample");
    if sample.zones.is_empty() {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::RequiredFieldMissing,
                "sample zones must contain at least one zone",
            )
            .with_path(format!("{sample_path}.zones")),
        );
        return;
    }
    if sample.zones.len() > 256 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "sample zones must contain at most 256 zones",
            )
            .with_path(format!("{sample_path}.zones")),
        );
    }

    let mut ids = HashSet::new();
    let mut groups = HashMap::<String, (u8, u8, u8, u8)>::new();
    for (index, zone) in sample.zones.iter().enumerate() {
        let zone_path = format!("{sample_path}.zones[{index}]");
        if !is_component_id(&zone.id) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ParameterIdInvalid,
                    "sample zone id must use component id syntax",
                )
                .with_path(format!("{zone_path}.id")),
            );
        }
        if !ids.insert(&zone.id) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::IdDuplicated,
                    "sample zone id must be unique within the generator",
                )
                .with_path(format!("{zone_path}.id")),
            );
        }
        validate_asset_reference(diagnostics, &zone_path, &zone.asset);
        if zone.root_note > 127 {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::LayerRangeInvalid,
                    "sample zone root note must be between 0 and 127",
                )
                .with_path(format!("{zone_path}.root_note")),
            );
        }
        if zone.key_min > 127 {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::LayerRangeInvalid,
                    "sample zone key range must be between 0 and 127",
                )
                .with_path(format!("{zone_path}.key_min")),
            );
        }
        if zone.key_max > 127 {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::LayerRangeInvalid,
                    "sample zone key range must be between 0 and 127",
                )
                .with_path(format!("{zone_path}.key_max")),
            );
        }
        if zone.key_min > zone.key_max {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::LayerRangeInvalid,
                    "sample zone key range must be ordered",
                )
                .with_path(format!("{zone_path}.key_min")),
            );
        }
        if zone.velocity_min == 0 || zone.velocity_min > 127 {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::LayerRangeInvalid,
                    "sample zone velocity range must be between 1 and 127",
                )
                .with_path(format!("{zone_path}.velocity_min")),
            );
        }
        if zone.velocity_max > 127 {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::LayerRangeInvalid,
                    "sample zone velocity range must be between 1 and 127",
                )
                .with_path(format!("{zone_path}.velocity_max")),
            );
        }
        if zone.velocity_min > zone.velocity_max {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::LayerRangeInvalid,
                    "sample zone velocity range must be ordered",
                )
                .with_path(format!("{zone_path}.velocity_min")),
            );
        }
        validate_sample_playback_definition(diagnostics, &zone_path, zone.playback);
        if let Some(group) = &zone.round_robin_group {
            if !is_component_id(group) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ParameterIdInvalid,
                        "round robin group must use component id syntax",
                    )
                    .with_path(format!("{zone_path}.round_robin_group")),
                );
            }
            let ranges = (
                zone.key_min,
                zone.key_max,
                zone.velocity_min,
                zone.velocity_max,
            );
            if let Some(previous) = groups.insert(group.clone(), ranges)
                && previous != ranges
            {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::DefinitionError,
                        "round robin group members must share key and velocity ranges",
                    )
                    .with_path(format!("{zone_path}.round_robin_group")),
                );
            }
        }
    }

    for (left_index, left) in sample.zones.iter().enumerate() {
        for (right_index, right) in sample.zones.iter().enumerate().skip(left_index + 1) {
            let key_overlap = left.key_min <= right.key_max && right.key_min <= left.key_max;
            let velocity_overlap =
                left.velocity_min <= right.velocity_max && right.velocity_min <= left.velocity_max;
            if !key_overlap || !velocity_overlap {
                continue;
            }
            let allowed = left.round_robin_group.is_some()
                && left.round_robin_group == right.round_robin_group
                && left.key_min == right.key_min
                && left.key_max == right.key_max
                && left.velocity_min == right.velocity_min
                && left.velocity_max == right.velocity_max;
            if !allowed {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::DefinitionError,
                        "overlapping sample zones require one matching round robin group and identical ranges",
                    )
                    .with_path(format!("{sample_path}.zones[{right_index}].id")),
                );
            }
        }
    }
}

fn validate_asset_reference(diagnostics: &mut Vec<Diagnostic>, path: &str, asset: &AssetReference) {
    if asset.path.trim().is_empty() {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::RequiredFieldMissing,
                "sample asset path must not be empty",
            )
            .with_path(format!("{path}.asset.path")),
        );
    }
    if let Some(hash) = &asset.sha256 {
        let valid = hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !valid {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "sample asset sha256 must be 64 hexadecimal characters",
                )
                .with_path(format!("{path}.asset.sha256")),
            );
        }
    }
}

fn validate_sample_playback_definition(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    playback: SampleZonePlaybackDefinition,
) {
    let mut validate_seconds = |field: &str, value: f32| {
        if !value.is_finite() || value < 0.0 {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "sample playback time must be finite and non-negative",
                )
                .with_path(format!("{path}.playback.{field}")),
            );
        }
    };
    match playback {
        SampleZonePlaybackDefinition::OneShot {
            start_seconds,
            end_seconds,
        } => {
            validate_seconds("start_seconds", start_seconds);
            if let Some(end_seconds) = end_seconds {
                validate_seconds("end_seconds", end_seconds);
                if start_seconds.is_finite()
                    && end_seconds.is_finite()
                    && end_seconds <= start_seconds
                {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::DefinitionError,
                            "one-shot end must be greater than start",
                        )
                        .with_path(format!("{path}.playback.end_seconds")),
                    );
                }
            }
        }
        SampleZonePlaybackDefinition::ForwardLoop {
            start_seconds,
            end_seconds,
            loop_start_seconds,
            loop_end_seconds,
        } => {
            validate_seconds("start_seconds", start_seconds);
            validate_seconds("loop_start_seconds", loop_start_seconds);
            validate_seconds("loop_end_seconds", loop_end_seconds);
            if let Some(end_seconds) = end_seconds {
                validate_seconds("end_seconds", end_seconds);
                if start_seconds.is_finite()
                    && end_seconds.is_finite()
                    && end_seconds <= start_seconds
                {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::DefinitionError,
                            "forward loop end must be greater than start",
                        )
                        .with_path(format!("{path}.playback.end_seconds")),
                    );
                }
            }
            if loop_end_seconds.is_finite()
                && loop_start_seconds.is_finite()
                && loop_end_seconds <= loop_start_seconds
            {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::DefinitionError,
                        "forward loop end must be greater than loop start",
                    )
                    .with_path(format!("{path}.playback.loop_end_seconds")),
                );
            }
            if start_seconds.is_finite()
                && loop_start_seconds.is_finite()
                && loop_start_seconds < start_seconds
            {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::DefinitionError,
                        "forward loop start must be inside the playback region",
                    )
                    .with_path(format!("{path}.playback.loop_start_seconds")),
                );
            }
            if let Some(end_seconds) = end_seconds
                && loop_end_seconds.is_finite()
                && end_seconds.is_finite()
                && loop_end_seconds > end_seconds
            {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::DefinitionError,
                        "forward loop end must be inside the playback region",
                    )
                    .with_path(format!("{path}.playback.loop_end_seconds")),
                );
            }
        }
    }
}

fn validate_oscillator(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    oscillator: &OscillatorDefinition,
) {
    validate_range(
        diagnostics,
        format!("{path}.generator.oscillator.phase"),
        oscillator.phase,
        0.0..=1.0,
        "oscillator phase must be finite and between 0 and 1",
    );
    if let OscillatorWaveform::Pulse { pulse_width } = oscillator.waveform {
        validate_range(
            diagnostics,
            format!("{path}.generator.oscillator.waveform.pulse_width"),
            pulse_width,
            0.05..=0.95,
            "pulse_width must be finite and between 0.05 and 0.95",
        );
    }
    if let Some(hard_sync) = oscillator.hard_sync {
        validate_range(
            diagnostics,
            format!("{path}.generator.oscillator.hard_sync.ratio"),
            hard_sync.ratio,
            1.0..=16.0,
            "hard sync ratio must be finite and between 1 and 16",
        );
        if oscillator.waveform == OscillatorWaveform::Sine {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "sine waveform cannot use hard sync",
                )
                .with_path(format!("{path}.generator.oscillator.hard_sync")),
            );
        }
        if oscillator.phase.is_finite() && oscillator.phase.total_cmp(&0.0).is_ne() {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "hard sync requires zero oscillator phase",
                )
                .with_path(format!("{path}.generator.oscillator.phase")),
            );
        }
    }
    if let Some(waveshaping) = oscillator.waveshaping {
        validate_range(
            diagnostics,
            format!("{path}.generator.oscillator.waveshaping.amount"),
            waveshaping.amount,
            0.0..=1.0,
            "waveshaping amount must be finite and between 0 and 1",
        );
    }
    if let Some(unison) = oscillator.unison {
        if !(2..=8).contains(&unison.voices) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "unison voices must be between 2 and 8",
                )
                .with_path(format!("{path}.generator.oscillator.unison.voices")),
            );
        }
        validate_range(
            diagnostics,
            format!("{path}.generator.oscillator.unison.detune_cents"),
            unison.detune_cents,
            0.0..=100.0,
            "unison detune_cents must be finite and between 0 and 100",
        );
        validate_range(
            diagnostics,
            format!("{path}.generator.oscillator.unison.stereo_spread"),
            unison.stereo_spread,
            0.0..=1.0,
            "unison stereo_spread must be finite and between 0 and 1",
        );
        validate_range(
            diagnostics,
            format!("{path}.generator.oscillator.unison.phase_spread"),
            unison.phase_spread,
            0.0..=1.0,
            "unison phase_spread must be finite and between 0 and 1",
        );
        if oscillator.hard_sync.is_some() && unison.phase_spread.total_cmp(&0.0).is_ne() {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "hard sync does not support non-zero phase spread",
                )
                .with_path(format!("{path}.generator.oscillator.unison.phase_spread")),
            );
        }
    }
}

fn validate_noise(diagnostics: &mut Vec<Diagnostic>, path: &str, noise: &NoiseDefinition) {
    validate_range(
        diagnostics,
        format!("{path}.generator.noise.stereo_correlation"),
        noise.stereo_correlation,
        0.0..=1.0,
        "stereo_correlation must be finite and between 0 and 1",
    );
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
                    phase: 0.0,
                    hard_sync: None,
                    waveshaping: None,
                    unison: None,
                }),
                processors: Vec::new(),
            }],
            voice_processors: Vec::new(),
            global_processors: Vec::new(),
            modulation: None,
        }
    }

    fn sample_zone(
        id: &str,
        key_min: u8,
        key_max: u8,
        velocity_min: u8,
        velocity_max: u8,
        round_robin_group: Option<&str>,
        playback: SampleZonePlaybackDefinition,
    ) -> SampleZoneDefinition {
        SampleZoneDefinition {
            id: id.to_owned(),
            asset: AssetReference {
                path: "test.wav".to_owned(),
                sha256: None,
            },
            root_note: 60,
            key_min,
            key_max,
            velocity_min,
            velocity_max,
            round_robin_group: round_robin_group.map(str::to_owned),
            playback,
        }
    }

    fn set_sample_zone_midi_field(zone: &mut SampleZoneDefinition, field: &str, value: u8) {
        match field {
            "root_note" => zone.root_note = value,
            "key_min" => zone.key_min = value,
            "key_max" => zone.key_max = value,
            "velocity_min" => zone.velocity_min = value,
            "velocity_max" => zone.velocity_max = value,
            _ => panic!("unknown sample zone MIDI field: {field}"),
        }
    }

    fn sample_definition(zones: Vec<SampleZoneDefinition>) -> InstrumentDefinition {
        let mut value = definition();
        value.layers[0].generator = GeneratorDefinition::Sample(SampleDefinition {
            interpolation: SampleInterpolation::Cubic,
            zones,
        });
        value
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
    fn processor_validation_rejects_duplicate_ids_and_invalid_placement() {
        let mut value = definition();
        value.layers[0].processors = vec![
            ProcessorDefinition::Drive(DriveProcessorDefinition {
                id: "drive".to_owned(),
                amount: 0.2,
                mix: 0.4,
            }),
            ProcessorDefinition::Delay(DelayProcessorDefinition {
                id: "echo".to_owned(),
                time_seconds: 0.2,
                feedback: 0.3,
                mix: 0.2,
            }),
        ];
        value.voice_processors = vec![
            ProcessorDefinition::Filter(FilterProcessorDefinition {
                id: "tone".to_owned(),
                cutoff_hz: 1_000.0,
                resonance: 0.1,
            }),
            ProcessorDefinition::Drive(DriveProcessorDefinition {
                id: "tone".to_owned(),
                amount: 0.2,
                mix: 0.4,
            }),
        ];

        let diagnostics = value.validate();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::ProcessorPlacementInvalid
                && diagnostic.path.as_deref() == Some("layers[0].processors[1]")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::ProcessorIdDuplicated
                && diagnostic.path.as_deref() == Some("voice_processors[1].id")
        }));
    }

    #[test]
    fn processor_validation_rejects_invalid_ids_and_values() {
        let mut value = definition();
        value.global_processors = vec![ProcessorDefinition::Reverb(ReverbProcessorDefinition {
            id: "Space".to_owned(),
            pre_delay_seconds: 0.3,
            decay: 1.0,
            damping: 0.5,
            width: 0.5,
            mix: 0.5,
        })];

        let diagnostics = value.validate();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::ProcessorIdInvalid
                && diagnostic.path.as_deref() == Some("global_processors[0].id")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::ValueOutOfRange
                && diagnostic.path.as_deref() == Some("global_processors[0].pre_delay_seconds")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::ValueOutOfRange
                && diagnostic.path.as_deref() == Some("global_processors[0].decay")
        }));
    }

    #[test]
    fn serde_rejects_unknown_processor_fields() {
        let mut value = serde_json::to_value(definition()).expect("definition serializes");
        value["voice_processors"] = serde_json::json!([{
            "type": "filter",
            "id": "tone",
            "cutoff_hz": 1_000.0,
            "resonance": 0.1,
            "unexpected": true,
        }]);

        let parsed = serde_json::from_value::<InstrumentDefinition>(value);

        assert!(parsed.is_err());
    }

    #[test]
    fn serde_round_trip_preserves_definition() {
        let source = definition();
        let json = serde_json::to_string(&source).expect("definition serializes");
        let restored: InstrumentDefinition =
            serde_json::from_str(&json).expect("definition parses");
        assert_eq!(source, restored);
    }

    #[test]
    fn oscillator_waveforms_use_tagged_objects() {
        let mut value = serde_json::to_value(definition()).expect("definition serializes");
        for waveform in ["sine", "saw", "square", "triangle"] {
            value["layers"][0]["generator"]["oscillator"]["waveform"] =
                serde_json::json!({"type": waveform});
            let parsed: InstrumentDefinition =
                serde_json::from_value(value.clone()).expect("basic waveform parses");
            assert!(matches!(
                parsed.layers[0].generator,
                GeneratorDefinition::Oscillator(OscillatorDefinition { .. })
            ));
        }
        value["layers"][0]["generator"]["oscillator"]["waveform"] =
            serde_json::json!({"type": "pulse", "pulse_width": 0.35});
        let parsed: InstrumentDefinition =
            serde_json::from_value(value).expect("pulse waveform parses");
        assert!(matches!(
            parsed.layers[0].generator,
            GeneratorDefinition::Oscillator(OscillatorDefinition {
                waveform: OscillatorWaveform::Pulse { pulse_width },
                ..
            }) if (pulse_width - 0.35).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn legacy_string_waveform_is_rejected() {
        let mut value = serde_json::to_value(definition()).expect("definition serializes");
        value["layers"][0]["generator"]["oscillator"]["waveform"] = serde_json::json!("saw");
        assert!(serde_json::from_value::<InstrumentDefinition>(value).is_err());
    }

    #[test]
    fn oscillator_and_noise_ranges_are_validated() {
        let mut value = definition();
        value.layers[0].generator = GeneratorDefinition::Oscillator(OscillatorDefinition {
            waveform: OscillatorWaveform::Pulse { pulse_width: 0.05 },
            phase_reset: true,
            phase: 1.0,
            hard_sync: None,
            waveshaping: None,
            unison: None,
        });
        assert!(value.validate().is_empty());

        if let GeneratorDefinition::Oscillator(oscillator) = &mut value.layers[0].generator {
            oscillator.phase = -f32::EPSILON;
            if let OscillatorWaveform::Pulse { pulse_width } = &mut oscillator.waveform {
                *pulse_width = 0.95 + f32::EPSILON;
            }
        }
        let diagnostics = value.validate();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("layers[0].generator.oscillator.phase")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref()
                == Some("layers[0].generator.oscillator.waveform.pulse_width")
        }));

        value.layers[0].generator = GeneratorDefinition::Noise(NoiseDefinition {
            color: NoiseColor::Pink,
            seed: 42,
            stereo_correlation: 0.0,
        });
        assert!(value.validate().is_empty());
        if let GeneratorDefinition::Noise(noise) = &mut value.layers[0].generator {
            noise.stereo_correlation = 1.0;
        }
        assert!(value.validate().is_empty());
        if let GeneratorDefinition::Noise(noise) = &mut value.layers[0].generator {
            noise.stereo_correlation = 1.0 + f32::EPSILON;
        }
        assert!(value.validate().iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("layers[0].generator.noise.stereo_correlation")
        }));
    }

    #[test]
    fn generator_unknown_fields_are_rejected() {
        let mut value = serde_json::to_value(definition()).expect("definition serializes");
        value["layers"][0]["generator"]["oscillator"]["waveform"] =
            serde_json::json!({"type": "square", "unexpected": true});
        assert!(serde_json::from_value::<InstrumentDefinition>(value).is_err());

        let mut value = serde_json::to_value(definition()).expect("definition serializes");
        value["layers"][0]["generator"]["oscillator"]["waveform"] =
            serde_json::json!({"type": "square", "pulse_width": null});
        assert!(serde_json::from_value::<InstrumentDefinition>(value).is_err());

        let mut value = serde_json::to_value(definition()).expect("definition serializes");
        value["layers"][0]["generator"] = serde_json::json!({
            "noise": {
                "color": "white",
                "seed": 7,
                "stereo_correlation": 0.5,
                "unexpected": true
            }
        });
        assert!(serde_json::from_value::<InstrumentDefinition>(value).is_err());
    }

    #[test]
    fn sample_schema_rejects_legacy_direct_fields() {
        let mut value = serde_json::to_value(definition()).expect("definition serializes");
        value["layers"][0]["generator"] = serde_json::json!({
            "sample": {
                "asset": {"path": "test.wav"},
                "root_note": 60,
                "playback_mode": "one_shot",
                "interpolation": "cubic"
            }
        });

        assert!(serde_json::from_value::<InstrumentDefinition>(value).is_err());
    }

    #[test]
    fn sample_zone_mapping_and_round_robin_ranges_are_validated() {
        let one_shot = SampleZonePlaybackDefinition::OneShot {
            start_seconds: 0.0,
            end_seconds: None,
        };
        let value = sample_definition(vec![
            sample_zone("soft", 0, 127, 1, 64, None, one_shot),
            sample_zone("hard", 0, 127, 65, 127, None, one_shot),
        ]);
        assert!(value.validate().is_empty());

        let value = sample_definition(vec![
            sample_zone("hit_a", 60, 60, 1, 127, Some("hits"), one_shot),
            sample_zone("hit_b", 60, 60, 1, 127, Some("hits"), one_shot),
        ]);
        assert!(value.validate().is_empty());

        let mut value =
            sample_definition(vec![sample_zone("invalid", 60, 59, 1, 127, None, one_shot)]);
        let diagnostics = value.validate();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::LayerRangeInvalid
                && diagnostic.path.as_deref() == Some("layers[0].generator.sample.zones[0].key_min")
        }));

        value.layers[0].generator = GeneratorDefinition::Sample(SampleDefinition {
            interpolation: SampleInterpolation::Cubic,
            zones: vec![
                sample_zone("a", 60, 60, 1, 127, None, one_shot),
                sample_zone("b", 60, 60, 1, 127, None, one_shot),
            ],
        });
        let diagnostics = value.validate();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::DefinitionError
                && diagnostic.path.as_deref() == Some("layers[0].generator.sample.zones[1].id")
        }));
    }

    #[test]
    fn sample_zone_midi_fields_have_explicit_bounds() {
        let one_shot = SampleZonePlaybackDefinition::OneShot {
            start_seconds: 0.0,
            end_seconds: None,
        };
        let fields = [
            ("root_note", "layers[0].generator.sample.zones[0].root_note"),
            ("key_min", "layers[0].generator.sample.zones[0].key_min"),
            ("key_max", "layers[0].generator.sample.zones[0].key_max"),
            (
                "velocity_min",
                "layers[0].generator.sample.zones[0].velocity_min",
            ),
            (
                "velocity_max",
                "layers[0].generator.sample.zones[0].velocity_max",
            ),
        ];

        for (field, path) in fields {
            let mut value =
                sample_definition(vec![sample_zone("valid", 0, 127, 1, 127, None, one_shot)]);
            if let GeneratorDefinition::Sample(sample) = &mut value.layers[0].generator {
                set_sample_zone_midi_field(&mut sample.zones[0], field, 127);
            }
            assert!(value.validate().is_empty(), "{field}=127 must be valid");

            for invalid in [128, 255] {
                let mut value =
                    sample_definition(vec![sample_zone("invalid", 0, 127, 1, 127, None, one_shot)]);
                if let GeneratorDefinition::Sample(sample) = &mut value.layers[0].generator {
                    let zone = &mut sample.zones[0];
                    match field {
                        "key_min" => zone.key_max = 255,
                        "velocity_min" => zone.velocity_max = 255,
                        _ => {}
                    }
                    set_sample_zone_midi_field(zone, field, invalid);
                }
                let diagnostics = value.validate();
                assert!(
                    diagnostics.iter().any(|diagnostic| {
                        diagnostic.code == DiagnosticCode::LayerRangeInvalid
                            && diagnostic.path.as_deref() == Some(path)
                    }),
                    "{field}={invalid} must report {path}: {diagnostics:?}"
                );
            }
        }
    }

    #[test]
    fn sample_forward_loop_requires_an_ordered_region_inside_the_zone() {
        let value = sample_definition(vec![sample_zone(
            "loop",
            0,
            127,
            1,
            127,
            None,
            SampleZonePlaybackDefinition::ForwardLoop {
                start_seconds: 1.0,
                end_seconds: Some(2.0),
                loop_start_seconds: 0.5,
                loop_end_seconds: 2.5,
            },
        )]);
        let diagnostics = value.validate();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::DefinitionError
                && diagnostic.path.as_deref()
                    == Some("layers[0].generator.sample.zones[0].playback.loop_start_seconds")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::DefinitionError
                && diagnostic.path.as_deref()
                    == Some("layers[0].generator.sample.zones[0].playback.loop_end_seconds")
        }));
    }

    #[test]
    fn modulation_route_identifiers_are_validated_without_trimming() {
        let mut value = definition();
        value.modulation = Some(ModulationDefinition {
            sources: vec![],
            routes: vec![
                ModulationRouteDefinition {
                    source: "bad-id".to_owned(),
                    target: "layer.body.gain".to_owned(),
                    amount: 0.0,
                    curve: ModulationCurve::Linear,
                },
                ModulationRouteDefinition {
                    source: "velocity".to_owned(),
                    target: " layer.body.gain".to_owned(),
                    amount: 0.0,
                    curve: ModulationCurve::Linear,
                },
            ],
        });
        let diagnostics = value.validate();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::SourceIdInvalid
                && diagnostic.path.as_deref() == Some("modulation.routes[0].source")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::RouteTargetInvalid
                && diagnostic.path.as_deref() == Some("modulation.routes[1].target")
        }));
    }
}
