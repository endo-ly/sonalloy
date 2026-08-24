use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Deserializer, Serialize};

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::generator_parameters::{
    ADDITIVE_INHARMONICITY, ADDITIVE_MORPH, ADDITIVE_SPECTRUM_TILT, FORMANT_SHIFT,
    FORMANT_SPECTRAL_TILT, FORMANT_THROAT, FORMANT_VOWEL_POSITION, GRAIN_DENSITY, GRAIN_PAN_SPREAD,
    GRAIN_PITCH, GRAIN_RANDOMNESS, GRAIN_SIZE, GRANULAR_POSITION, MAX_PARTIALS, MODAL_BRIGHTNESS,
    MODAL_DECAY, MODAL_STRUCTURE, NOISE_CORRELATION, OPERATOR_AM_RING_AMOUNT_MAX,
    OPERATOR_AM_RING_AMOUNT_MIN, OPERATOR_DETUNE_MAX, OPERATOR_DETUNE_MIN, OPERATOR_FEEDBACK_MAX,
    OPERATOR_FEEDBACK_MIN, OPERATOR_LEVEL_MAX, OPERATOR_LEVEL_MIN,
    OPERATOR_PHASE_FREQUENCY_AMOUNT_MAX, OPERATOR_PHASE_FREQUENCY_AMOUNT_MIN, OPERATOR_PHASE_MAX,
    OPERATOR_PHASE_MIN, OPERATOR_RATIO_MAX, OPERATOR_RATIO_MIN, OSCILLATOR_FEEDBACK,
    PHASE_DISTORTION, PHYSICAL_EXCITER_DURATION_SECONDS_MAX, PHYSICAL_EXCITER_DURATION_SECONDS_MIN,
    PHYSICAL_STRING_BRIGHTNESS, PHYSICAL_STRING_DECAY_SECONDS, PHYSICAL_STRING_STIFFNESS,
    PULSE_WIDTH, SPECTRAL_BLUR, SPECTRAL_FREEZE, SPECTRAL_MORPH, SPECTRAL_POSITION, SPECTRAL_SHIFT,
    SYNC_RATIO, UNISON_DETUNE, UNISON_SPREAD, WAVEFOLD, WAVESHAPE, WAVETABLE_POSITION,
};
use crate::parameter::{BUILTIN_SOURCE_IDS, ModulationUnit, is_component_id, is_parameter_id};

/// The Definition schema accepted by the compiler.
pub const CURRENT_SCHEMA_VERSION: u32 = 3;

const MAX_VOICE_MODULATION_SOURCES: usize = 64;
const MAX_MSEG_SOURCES: usize = 16;
const MAX_STEP_SOURCES: usize = 16;
const MAX_SAMPLE_HOLD_SOURCES: usize = 16;
const MAX_SMOOTH_RANDOM_SOURCES: usize = 16;

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
    /// Stable instrument-level macro controls.
    #[serde(default)]
    pub macros: Vec<MacroDefinition>,
    /// Constant-power layer vector controls.
    #[serde(default)]
    pub vectors: Vec<VectorDefinition>,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum PerformanceDefinition {
    /// Polyphonic voice allocation and stealing policy.
    Polyphonic {
        /// Maximum number of simultaneous voices.
        polyphony: u16,
        /// Voice stealing policy.
        voice_stealing: VoiceStealingDefinition,
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

/// Generator variants in the Definition model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratorDefinition {
    /// A DaisySP-backed oscillator.
    Oscillator(OscillatorDefinition),
    /// A deterministic stereo noise generator.
    Noise(NoiseDefinition),
    /// A deterministic fractional-delay feedback string model.
    PhysicalString(PhysicalStringDefinition),
    /// A deterministic `DaisySP` modal resonator.
    Modal(ModalDefinition),
    /// A directly specified bank of sine partials.
    Additive(AdditiveDefinition),
    /// A harmonic bank shaped by interpolated formant profiles.
    Formant(FormantDefinition),
    /// A mapped sample instrument loaded during compilation.
    Sample(SampleDefinition),
    /// A deterministic grain-based reconstruction of a prepared audio asset.
    Granular(GranularDefinition),
    /// A time-ordered sequence of prepared audio assets.
    WaveSequence(WaveSequenceDefinition),
    /// A band-limited wavetable prepared from a mono or stereo asset.
    Wavetable(WavetableDefinition),
    /// A spectral asset reconstructed through fixed STFT frames.
    Spectral(SpectralDefinition),
    /// A fixed-topology four-operator modulation generator.
    OperatorModulation(OperatorModulationDefinition),
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
    /// Optional sine phase-distortion configuration.
    #[serde(default)]
    pub phase_distortion: Option<PhaseDistortionDefinition>,
    /// Optional post-waveshaping Wavefolder configuration.
    #[serde(default)]
    pub wavefold: Option<WavefoldDefinition>,
    /// Optional one-sample phase feedback configuration.
    #[serde(default)]
    pub feedback: Option<OscillatorFeedbackDefinition>,
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

/// Sine phase-distortion settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseDistortionDefinition {
    /// Normalized phase breakpoint displacement.
    pub amount: f32,
}

/// `DaisySP` Wavefolder settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WavefoldDefinition {
    /// Normalized Wavefolder amount.
    pub amount: f32,
}

/// One-sample oscillator feedback settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OscillatorFeedbackDefinition {
    /// Normalized previous-output feedback amount.
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

/// Deterministic excitation used by the physical and modal generators.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PhysicalExciterDefinition {
    /// A single-sample impulse.
    Impulse,
    /// A short deterministic noise burst with a one-pole brightness filter.
    NoiseBurst {
        /// Burst duration in seconds.
        duration_seconds: f32,
        /// Low-pass brightness in the inclusive zero-to-one range.
        brightness: f32,
        /// Deterministic excitation seed.
        seed: u64,
    },
}

/// Fractional-delay feedback string generator settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalStringDefinition {
    /// Deterministic note-start excitation.
    pub exciter: PhysicalExciterDefinition,
    /// Nominal decay time in seconds.
    pub decay_seconds: f32,
    /// Low-pass loop brightness.
    pub brightness: f32,
    /// First-order all-pass dispersion amount.
    pub stiffness: f32,
}

/// `DaisySP` modal resonator generator settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModalDefinition {
    /// Deterministic note-start excitation.
    pub exciter: PhysicalExciterDefinition,
    /// Fixed number of resonant modes.
    pub mode_count: u8,
    /// Resonator structure control.
    pub structure: f32,
    /// Resonator brightness control.
    pub brightness: f32,
    /// Resonator damping control.
    pub decay: f32,
}

/// Additive generator settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdditiveDefinition {
    /// Whether every Note On starts each partial at its initial phase.
    pub phase_reset: bool,
    /// Amplitude interpolation between Spectrum A and Spectrum B.
    pub morph: f32,
    /// Spectral amplitude slope in decibels per octave.
    pub spectrum_tilt_db_per_octave: f32,
    /// Progressive ratio stretch applied to higher partials.
    pub inharmonicity: f32,
    /// Ordered partial definitions.
    pub partials: Vec<AdditivePartialDefinition>,
}

/// One additive sine partial.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdditivePartialDefinition {
    /// Stable identifier used by inspection and diagnostics.
    pub id: String,
    /// Frequency ratio relative to the played note.
    pub ratio: f32,
    /// Spectrum A amplitude.
    pub amplitude_a: f32,
    /// Spectrum B amplitude.
    pub amplitude_b: f32,
    /// Initial phase in cycles.
    pub phase: f32,
    /// Optional partial-local amplitude envelope.
    #[serde(default)]
    pub envelope: Option<AdsrDefinition>,
}

/// Formant generator settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormantDefinition {
    /// Whether every Note On starts the harmonic bank at its initial phase.
    pub phase_reset: bool,
    /// Number of harmonic partials generated by the formant bank.
    pub partial_count: u8,
    /// Position between the first and last formant profiles.
    pub vowel_position: f32,
    /// Frequency shift applied to all formant centers and bandwidths, in cents.
    pub formant_shift_cents: f32,
    /// Global bandwidth control for the formant bands.
    pub throat: f32,
    /// Spectral amplitude slope in decibels per octave.
    pub spectral_tilt_db_per_octave: f32,
    /// Ordered formant profiles used for static vowels and morphing.
    pub profiles: Vec<FormantProfileDefinition>,
}

/// One named point in a formant morph path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormantProfileDefinition {
    /// Stable identifier used by inspection and diagnostics.
    pub id: String,
    /// Five ascending formant bands.
    pub formants: Vec<FormantBandDefinition>,
}

/// One Gaussian-like formant band.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormantBandDefinition {
    /// Center frequency in hertz.
    pub frequency_hz: f32,
    /// Full width at half maximum in hertz.
    pub bandwidth_hz: f32,
    /// Relative band gain in decibels.
    pub gain_db: f32,
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

/// Granular generator settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GranularDefinition {
    /// Referenced mono or stereo audio asset.
    pub asset: AssetReference,
    /// MIDI note represented by the source recording.
    pub root_note: u8,
    /// Region from which grains are selected, expressed in source seconds.
    pub region: SampleRegionDefinition,
    /// Initial normalized position inside the region.
    pub position: f32,
    /// Grain duration in seconds.
    pub grain_size: f32,
    /// Grain density in grains per second.
    pub density: f32,
    /// Grain pitch offset in cents.
    pub pitch: f32,
    /// Normalized source-position randomization amount.
    pub randomness: f32,
    /// Normalized per-grain stereo spread.
    pub pan_spread: f32,
    /// Explicit deterministic grain seed.
    pub seed: u64,
}

/// A sequence of audio steps played by one Generator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaveSequenceDefinition {
    /// MIDI note represented by the sequence assets.
    pub root_note: u8,
    /// Order in which steps are selected.
    pub direction: WaveSequenceDirection,
    /// Whether the sequence returns to its first step after reaching an end.
    #[serde(rename = "loop")]
    pub loop_sequence: bool,
    /// Constant-power overlap ratio between adjacent steps.
    pub crossfade: f32,
    /// Ordered sequence steps.
    pub steps: Vec<WaveSequenceStepDefinition>,
}

/// Sequence order used to select steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveSequenceDirection {
    /// Select steps from the first to the last.
    Forward,
    /// Select steps from the last to the first.
    Reverse,
    /// Traverse from the first to the last and back without repeating endpoints.
    PingPong,
}

/// One time-bounded Wave Sequence step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaveSequenceStepDefinition {
    /// Stable identifier within the sequence.
    pub id: String,
    /// Referenced mono or stereo audio asset.
    pub asset: AssetReference,
    /// Region selected from the prepared asset.
    pub region: SampleRegionDefinition,
    /// Duration of the step in seconds or beats.
    pub duration: WaveSequenceDurationDefinition,
    /// Playback mode for the asset inside the step.
    pub playback: WaveSequenceStepPlayback,
    /// Cursor direction through the step region.
    pub playback_direction: SamplePlaybackDirection,
    /// Step gain in decibels.
    pub gain_db: f32,
    /// Step pitch offset in cents.
    pub pitch_cents: f32,
}

/// Duration unit used by a Wave Sequence step.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum WaveSequenceDurationDefinition {
    /// A tempo-independent duration in seconds.
    Seconds {
        /// Duration in seconds.
        value: f32,
    },
    /// A duration that follows the current process tempo.
    Beats {
        /// Duration in quarter-note beats.
        value: f32,
    },
}

/// Asset playback mode inside one sequence step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveSequenceStepPlayback {
    /// Read the region once and output silence after it ends.
    OneShot,
    /// Repeat the region until the step duration ends.
    Loop,
}

/// Wavetable generator settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WavetableDefinition {
    /// Referenced WAV containing one or more consecutive frames.
    pub asset: AssetReference,
    /// Number of samples in one periodic frame.
    pub frame_length: u16,
    /// Initial position between the first and last frame.
    pub position: f32,
    /// Whether Note On restores the initial phase.
    pub phase_reset: bool,
    /// Initial phase in the inclusive zero-to-one range.
    pub phase: f32,
    /// Optional wavetable unison configuration.
    #[serde(default)]
    pub unison: Option<UnisonDefinition>,
}

/// Spectral analysis and resynthesis settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpectralDefinition {
    /// Primary audio asset and timing source for the generator.
    pub asset_a: AssetReference,
    /// Optional second asset reserved for spectral morphing.
    #[serde(default)]
    pub asset_b: Option<AssetReference>,
    /// MIDI note represented by the primary source asset.
    pub root_note: u8,
    /// Allowed FFT size used for STFT analysis and resynthesis.
    pub fft_size: u16,
    /// Initial normalized source position.
    pub position: f32,
    /// Initial source scan freeze amount.
    pub freeze: f32,
    /// Initial temporal magnitude blur in seconds.
    pub blur_seconds: f32,
    /// Initial frequency translation in hertz.
    pub shift_hz: f32,
    /// Initial morph amount between the two source assets.
    pub morph: f32,
    /// Whether Note On restores the prepared source phase.
    pub phase_reset: bool,
}

/// Four-operator audio-rate modulation settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorModulationDefinition {
    /// Audio-rate interaction mode applied to every operator connection.
    pub mode: OperatorModulationMode,
    /// Fixed algorithm describing the operator topology.
    pub algorithm: OperatorAlgorithm,
    /// Exactly four operators in user-facing order from one to four.
    pub operators: Vec<OperatorDefinition>,
    /// Whether Note On restores the operator phases.
    pub phase_reset: bool,
    /// Optional operator-engine unison configuration.
    #[serde(default)]
    pub unison: Option<UnisonDefinition>,
}

/// Audio-rate interaction mode used by an operator modulation generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorModulationMode {
    /// Add a modulator signal to the carrier read phase.
    Phase,
    /// Add a modulator signal to the carrier instantaneous frequency ratio.
    Frequency,
    /// Apply a unipolar modulator multiplier to the carrier amplitude.
    Amplitude,
    /// Crossfade the carrier with its bipolar modulator product.
    Ring,
}

/// Fixed operator connection algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorAlgorithm {
    /// Operator 4 modulates 3, then 2, then carrier 1.
    #[serde(rename = "stack_4")]
    Stack4,
    /// Stack 4-3-2 with carriers 1 and 2.
    #[serde(rename = "stack_3_plus_carrier")]
    Stack3PlusCarrier,
    /// Independent stacks 2-1 and 4-3.
    TwoStacks,
    /// Operator 4 branches to 2 and 3, which both modulate carrier 1.
    ForkToCarrier,
    /// Operators 3 and 4 modulate carrier 1 while carrier 2 is parallel.
    TwoModulatorsPlusCarrier,
    /// Operators 2, 3, and 4 all modulate carrier 1.
    ThreeModulators,
    /// Operator 4 independently modulates carriers 1, 2, and 3.
    SharedModulator,
    /// Four independent carriers are summed.
    Parallel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperatorTopology {
    pub(crate) evaluation_order: [u8; 4],
    pub(crate) incoming_masks: [u8; 4],
    pub(crate) carrier_mask: u8,
}

impl OperatorAlgorithm {
    pub(crate) const fn topology(self) -> OperatorTopology {
        match self {
            Self::Stack4 => OperatorTopology {
                evaluation_order: [3, 2, 1, 0],
                incoming_masks: [0b0010, 0b0100, 0b1000, 0],
                carrier_mask: 0b0001,
            },
            Self::Stack3PlusCarrier => OperatorTopology {
                evaluation_order: [3, 2, 1, 0],
                incoming_masks: [0, 0b0100, 0b1000, 0],
                carrier_mask: 0b0011,
            },
            Self::TwoStacks => OperatorTopology {
                evaluation_order: [1, 3, 0, 2],
                incoming_masks: [0b0010, 0, 0b1000, 0],
                carrier_mask: 0b0101,
            },
            Self::ForkToCarrier => OperatorTopology {
                evaluation_order: [3, 1, 2, 0],
                incoming_masks: [0b0110, 0b1000, 0b1000, 0],
                carrier_mask: 0b0001,
            },
            Self::TwoModulatorsPlusCarrier => OperatorTopology {
                evaluation_order: [2, 3, 0, 1],
                incoming_masks: [0b1100, 0, 0, 0],
                carrier_mask: 0b0011,
            },
            Self::ThreeModulators => OperatorTopology {
                evaluation_order: [1, 2, 3, 0],
                incoming_masks: [0b1110, 0, 0, 0],
                carrier_mask: 0b0001,
            },
            Self::SharedModulator => OperatorTopology {
                evaluation_order: [3, 0, 1, 2],
                incoming_masks: [0b1000, 0b1000, 0b1000, 0],
                carrier_mask: 0b0111,
            },
            Self::Parallel => OperatorTopology {
                evaluation_order: [0, 1, 2, 3],
                incoming_masks: [0, 0, 0, 0],
                carrier_mask: 0b1111,
            },
        }
    }
}

/// One sine operator in a modulation generator.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorDefinition {
    /// Frequency multiplier relative to the played note.
    pub ratio: f32,
    /// Frequency offset after the ratio, in cents.
    pub detune_cents: f32,
    /// Carrier output level.
    pub level: f32,
    /// Mode-dependent modulation depth.
    pub modulation_amount: f32,
    /// One-sample self-feedback amount.
    pub feedback: f32,
    /// Initial operator phase in the inclusive zero-to-one range.
    pub phase: f32,
    /// Operator-local amplitude envelope.
    pub envelope: AdsrDefinition,
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
#[serde(deny_unknown_fields)]
pub struct SampleZonePlaybackDefinition {
    /// Region selected from the prepared asset.
    pub region: SampleRegionDefinition,
    /// Cursor direction through the region.
    pub direction: SamplePlaybackDirection,
    /// Optional loop inside the region.
    #[serde(rename = "loop")]
    pub r#loop: Option<SampleLoopDefinition>,
    /// Time behavior applied after the region and direction are resolved.
    pub time: SampleTimeDefinition,
}

/// Region boundaries expressed in source seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleRegionDefinition {
    /// Inclusive region start in source seconds.
    pub start_seconds: f32,
    /// Optional exclusive region end in source seconds.
    pub end_seconds: Option<f32>,
}

/// Sample cursor direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SamplePlaybackDirection {
    /// Read from region start toward region end.
    Forward,
    /// Read from region end toward region start.
    Reverse,
}

/// Loop boundaries and crossfade expressed in source seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleLoopDefinition {
    /// Inclusive loop start in source seconds.
    pub start_seconds: f32,
    /// Exclusive loop end in source seconds.
    pub end_seconds: f32,
    /// Constant-power crossfade duration in seconds.
    pub crossfade_seconds: f32,
}

/// Time behavior for a Sample Zone.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum SampleTimeDefinition {
    /// Couple pitch and duration through ordinary resampling.
    Resample,
    /// Keep pitch independent from duration using a fixed duration ratio.
    FixedStretch {
        /// Output duration divided by source duration, in the inclusive 0.5..=2.0 range.
        ratio: f32,
    },
    /// Derive the duration ratio from the source and process tempos.
    TempoSync {
        /// Tempo embedded in the source asset, in beats per minute.
        source_bpm: f32,
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
    /// State-variable filter.
    Filter(FilterProcessorDefinition),
    /// Soft-clipping drive.
    Drive(DriveProcessorDefinition),
    /// Fixed three-band equalizer.
    Eq(EqProcessorDefinition),
    /// Tuned feedback resonator.
    Resonator(ResonatorProcessorDefinition),
    /// Sample-rate reducer and quantizer.
    Bitcrusher(BitcrusherProcessorDefinition),
    /// Stereo chorus.
    Chorus(ChorusProcessorDefinition),
    /// Stereo flanger.
    Flanger(FlangerProcessorDefinition),
    /// Stereo phaser.
    Phaser(PhaserProcessorDefinition),
    /// Stereo feedback delay.
    Delay(DelayProcessorDefinition),
    /// Stereo plate reverb.
    Reverb(ReverbProcessorDefinition),
    /// Stereo-linked compressor.
    Compressor(CompressorProcessorDefinition),
    /// Zero-latency stereo-linked limiter.
    Limiter(LimiterProcessorDefinition),
}

impl ProcessorDefinition {
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Filter(value) => &value.id,
            Self::Drive(value) => &value.id,
            Self::Eq(value) => &value.id,
            Self::Resonator(value) => &value.id,
            Self::Bitcrusher(value) => &value.id,
            Self::Chorus(value) => &value.id,
            Self::Flanger(value) => &value.id,
            Self::Phaser(value) => &value.id,
            Self::Delay(value) => &value.id,
            Self::Reverb(value) => &value.id,
            Self::Compressor(value) => &value.id,
            Self::Limiter(value) => &value.id,
        }
    }
}

/// Output mode selected from the state-variable filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FilterModeDefinition {
    /// Low-pass output.
    #[default]
    LowPass,
    /// High-pass output.
    HighPass,
    /// Band-pass output.
    BandPass,
    /// Notch output.
    Notch,
}

/// State-variable filter processor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Filter output mode.
    #[serde(default)]
    pub mode: FilterModeDefinition,
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

/// Fixed three-band equalizer processor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EqProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Low-shelf midpoint in Hz.
    pub low_frequency_hz: f32,
    /// Low-shelf gain in dB.
    pub low_gain_db: f32,
    /// Mid peaking center frequency in Hz.
    pub mid_frequency_hz: f32,
    /// Mid peaking gain in dB.
    pub mid_gain_db: f32,
    /// Mid peaking Q factor.
    pub mid_q: f32,
    /// High-shelf midpoint in Hz.
    pub high_frequency_hz: f32,
    /// High-shelf gain in dB.
    pub high_gain_db: f32,
}

/// Tuned feedback resonator processor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResonatorProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Resonance frequency in Hz.
    pub frequency_hz: f32,
    /// Approximate T60 decay in seconds.
    pub decay_seconds: f32,
    /// High-frequency damping amount.
    pub damping: f32,
    /// Dry/wet mix.
    pub mix: f32,
}

/// Bitcrusher processor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BitcrusherProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Fractional quantizer bit depth.
    pub bit_depth: f32,
    /// Fraction of the input sample rate retained by sample-and-hold.
    pub sample_rate_ratio: f32,
    /// Dry/wet mix.
    pub mix: f32,
}

/// Chorus processor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChorusProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Center delay in milliseconds.
    pub delay_ms: f32,
    /// LFO rate in Hz.
    pub rate_hz: f32,
    /// Delay modulation depth.
    pub depth: f32,
    /// Positive feedback amount.
    pub feedback: f32,
    /// Stereo LFO phase width.
    pub width: f32,
    /// Dry/wet mix.
    pub mix: f32,
}

/// Flanger processor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlangerProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Center delay in milliseconds.
    pub delay_ms: f32,
    /// LFO rate in Hz.
    pub rate_hz: f32,
    /// Delay modulation depth.
    pub depth: f32,
    /// Positive or negative feedback amount.
    pub feedback: f32,
    /// Stereo LFO phase width.
    pub width: f32,
    /// Dry/wet mix.
    pub mix: f32,
}

/// Phaser processor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaserProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Number of first-order all-pass stages.
    pub stages: u8,
    /// Sweep center frequency in Hz.
    pub center_hz: f32,
    /// Sweep width in octaves.
    pub sweep_octaves: f32,
    /// LFO rate in Hz.
    pub rate_hz: f32,
    /// Sweep depth.
    pub depth: f32,
    /// Positive or negative feedback amount.
    pub feedback: f32,
    /// Stereo LFO phase width.
    pub width: f32,
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

/// Stereo-linked compressor processor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompressorProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Compression threshold in dB.
    pub threshold_db: f32,
    /// Compression ratio.
    pub ratio: f32,
    /// Attack time in milliseconds.
    pub attack_ms: f32,
    /// Release time in milliseconds.
    pub release_ms: f32,
    /// Soft-knee width in dB.
    pub knee_db: f32,
    /// Makeup gain in dB.
    pub makeup_gain_db: f32,
    /// Dry/wet mix.
    pub mix: f32,
}

/// Stereo-linked limiter processor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimiterProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Output ceiling in dBFS.
    pub ceiling_db: f32,
    /// Release time in milliseconds.
    pub release_ms: f32,
    /// Input gain in dB.
    pub input_gain_db: f32,
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
    /// Multi-segment envelope source.
    Mseg(MsegDefinition),
    /// Held step sequence source.
    Step(StepModulatorDefinition),
    /// Deterministic stepped random source.
    SampleHold(SampleHoldDefinition),
    /// Deterministic interpolated random source.
    SmoothRandom(SmoothRandomDefinition),
}

/// LFO source settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LfoDefinition {
    /// Stable source identifier.
    pub id: String,
    /// LFO waveform.
    pub waveform: LfoWaveform,
    /// Oscillation rate.
    pub rate: ModulationRateDefinition,
    /// Initial phase in the half-open zero-to-one range.
    pub phase: f32,
}

/// Rate unit used by periodic and stepped modulation sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulationRateUnit {
    /// Cycles, steps, or transitions per second.
    PerSecond,
    /// Cycles, steps, or transitions per quarter-note beat.
    PerBeat,
}

/// Rate used by periodic and stepped modulation sources.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulationRateDefinition {
    /// Rate value.
    pub value: f32,
    /// Rate unit.
    pub unit: ModulationRateUnit,
}

/// Duration unit used by MSEG segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulationDurationUnit {
    /// Duration in seconds.
    Seconds,
    /// Duration in quarter-note beats.
    Beats,
}

/// Duration used by an MSEG segment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulationDurationDefinition {
    /// Duration value.
    pub value: f32,
    /// Duration unit.
    pub unit: ModulationDurationUnit,
}

/// MSEG segment curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulationSegmentCurve {
    /// Linear interpolation.
    Linear,
    /// Smooth-step interpolation.
    SmoothStep,
}

/// One MSEG segment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MsegSegmentDefinition {
    /// Segment duration.
    pub duration: ModulationDurationDefinition,
    /// Segment target in the bipolar source range.
    pub target: f32,
    /// Segment interpolation curve.
    pub curve: ModulationSegmentCurve,
}

/// Optional MSEG loop range. The end index is exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MsegLoopDefinition {
    /// First segment in the loop.
    pub start_segment: usize,
    /// Exclusive end segment in the loop.
    pub end_segment: usize,
}

/// Multi-segment voice modulation source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MsegDefinition {
    /// Stable source identifier.
    pub id: String,
    /// Initial source value.
    pub initial_value: f32,
    /// Ordered segments.
    pub segments: Vec<MsegSegmentDefinition>,
    /// Optional loop range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_range: Option<MsegLoopDefinition>,
}

/// Held step sequence modulation source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepModulatorDefinition {
    /// Stable source identifier.
    pub id: String,
    /// Bipolar values held for each step.
    pub values: Vec<f32>,
    /// Step rate.
    pub rate: ModulationRateDefinition,
}

/// Deterministic sample-and-hold modulation source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleHoldDefinition {
    /// Stable source identifier.
    pub id: String,
    /// Explicit deterministic seed.
    pub seed: u64,
    /// Update rate.
    pub rate: ModulationRateDefinition,
}

/// Deterministic smooth-random modulation source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SmoothRandomDefinition {
    /// Stable source identifier.
    pub id: String,
    /// Explicit deterministic seed.
    pub seed: u64,
    /// Transition rate.
    pub rate: ModulationRateDefinition,
}

/// A named normalized instrument control.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacroDefinition {
    /// Stable macro identifier.
    pub id: String,
    /// Human-readable macro name.
    pub name: String,
    /// Initial normalized value.
    pub default: f32,
}

/// A constant-power layer vector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum VectorDefinition {
    /// Two-layer constant-power crossfade.
    TwoWay {
        /// Stable vector identifier.
        id: String,
        /// Human-readable vector name.
        name: String,
        /// Layer at position zero.
        layer_a: LayerId,
        /// Layer at position one.
        layer_b: LayerId,
        /// Initial position.
        position: f32,
    },
    /// Four-layer constant-power XY mixer.
    FourWay {
        /// Stable vector identifier.
        id: String,
        /// Human-readable vector name.
        name: String,
        /// Top-left layer.
        top_left: LayerId,
        /// Top-right layer.
        top_right: LayerId,
        /// Bottom-left layer.
        bottom_left: LayerId,
        /// Bottom-right layer.
        bottom_right: LayerId,
        /// Initial horizontal position.
        x: f32,
        /// Initial vertical position.
        y: f32,
    },
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
    /// Signed modulation depth in the target's declared modulation unit.
    pub depth: ModulationDepthDefinition,
    /// Source shaping curve.
    pub curve: ModulationCurve,
}

/// Signed modulation depth written by an instrument author.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulationDepthDefinition {
    /// Signed depth applied when the shaped source reaches one.
    pub value: f32,
    /// Unit required by the target parameter.
    pub unit: ModulationUnit,
}

/// Curve applied to a source before its route depth.
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
        match &self.performance {
            PerformanceDefinition::Polyphonic { polyphony, .. } => {
                if !(1..=64).contains(polyphony) {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "polyphony must be between 1 and 64",
                        )
                        .with_path("performance.polyphony"),
                    );
                }
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
        if let Some(modulation) = &self.modulation {
            validate_modulation(&mut diagnostics, modulation);
        }
        validate_macros(&mut diagnostics, &self.macros);
        validate_vectors(&mut diagnostics, &self.vectors, &self.layers);
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
        if !processor_allowed_at(processor, placement) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ProcessorPlacementInvalid,
                    format!(
                        "{} processor is not allowed in {} processors",
                        processor_type_name(processor),
                        placement_name(placement)
                    ),
                )
                .with_path(&path),
            );
        }
        validate_processor_values(diagnostics, &path, processor);
    }
}

fn processor_allowed_at(processor: &ProcessorDefinition, placement: ProcessorPlacement) -> bool {
    match placement {
        ProcessorPlacement::Layer => matches!(
            processor,
            ProcessorDefinition::Filter(_)
                | ProcessorDefinition::Drive(_)
                | ProcessorDefinition::Eq(_)
                | ProcessorDefinition::Resonator(_)
                | ProcessorDefinition::Bitcrusher(_)
        ),
        ProcessorPlacement::Voice => matches!(
            processor,
            ProcessorDefinition::Filter(_)
                | ProcessorDefinition::Drive(_)
                | ProcessorDefinition::Eq(_)
                | ProcessorDefinition::Resonator(_)
                | ProcessorDefinition::Compressor(_)
                | ProcessorDefinition::Limiter(_)
        ),
        ProcessorPlacement::Global => matches!(
            processor,
            ProcessorDefinition::Filter(_)
                | ProcessorDefinition::Drive(_)
                | ProcessorDefinition::Eq(_)
                | ProcessorDefinition::Chorus(_)
                | ProcessorDefinition::Flanger(_)
                | ProcessorDefinition::Phaser(_)
                | ProcessorDefinition::Delay(_)
                | ProcessorDefinition::Reverb(_)
                | ProcessorDefinition::Compressor(_)
                | ProcessorDefinition::Limiter(_)
        ),
    }
}

fn placement_name(placement: ProcessorPlacement) -> &'static str {
    match placement {
        ProcessorPlacement::Layer => "layer",
        ProcessorPlacement::Voice => "voice",
        ProcessorPlacement::Global => "global",
    }
}

fn processor_type_name(processor: &ProcessorDefinition) -> &'static str {
    match processor {
        ProcessorDefinition::Filter(_) => "filter",
        ProcessorDefinition::Drive(_) => "drive",
        ProcessorDefinition::Eq(_) => "eq",
        ProcessorDefinition::Resonator(_) => "resonator",
        ProcessorDefinition::Bitcrusher(_) => "bitcrusher",
        ProcessorDefinition::Chorus(_) => "chorus",
        ProcessorDefinition::Flanger(_) => "flanger",
        ProcessorDefinition::Phaser(_) => "phaser",
        ProcessorDefinition::Delay(_) => "delay",
        ProcessorDefinition::Reverb(_) => "reverb",
        ProcessorDefinition::Compressor(_) => "compressor",
        ProcessorDefinition::Limiter(_) => "limiter",
    }
}

#[allow(clippy::too_many_lines)]
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
        ProcessorDefinition::Eq(value) => {
            validate_range(
                diagnostics,
                format!("{path}.low_frequency_hz"),
                value.low_frequency_hz,
                20.0..=500.0,
                "low_frequency_hz must be finite and between 20 and 500 Hz",
            );
            validate_range(
                diagnostics,
                format!("{path}.low_gain_db"),
                value.low_gain_db,
                -24.0..=24.0,
                "low_gain_db must be finite and between -24 and 24 dB",
            );
            validate_range(
                diagnostics,
                format!("{path}.mid_frequency_hz"),
                value.mid_frequency_hz,
                100.0..=12_000.0,
                "mid_frequency_hz must be finite and between 100 and 12000 Hz",
            );
            validate_range(
                diagnostics,
                format!("{path}.mid_gain_db"),
                value.mid_gain_db,
                -24.0..=24.0,
                "mid_gain_db must be finite and between -24 and 24 dB",
            );
            validate_range(
                diagnostics,
                format!("{path}.mid_q"),
                value.mid_q,
                0.25..=8.0,
                "mid_q must be finite and between 0.25 and 8",
            );
            validate_range(
                diagnostics,
                format!("{path}.high_frequency_hz"),
                value.high_frequency_hz,
                2_000.0..=20_000.0,
                "high_frequency_hz must be finite and between 2000 and 20000 Hz",
            );
            validate_range(
                diagnostics,
                format!("{path}.high_gain_db"),
                value.high_gain_db,
                -24.0..=24.0,
                "high_gain_db must be finite and between -24 and 24 dB",
            );
            if value.low_frequency_hz >= value.mid_frequency_hz
                || value.mid_frequency_hz >= value.high_frequency_hz
            {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "EQ frequencies must be strictly ordered from low to high",
                    )
                    .with_path(path),
                );
            }
        }
        ProcessorDefinition::Resonator(value) => {
            validate_range(
                diagnostics,
                format!("{path}.frequency_hz"),
                value.frequency_hz,
                40.0..=12_000.0,
                "frequency_hz must be finite and between 40 and 12000 Hz",
            );
            validate_range(
                diagnostics,
                format!("{path}.decay_seconds"),
                value.decay_seconds,
                0.02..=10.0,
                "decay_seconds must be finite and between 0.02 and 10 seconds",
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
                format!("{path}.mix"),
                value.mix,
                0.0..=1.0,
                "mix must be finite and between 0 and 1",
            );
        }
        ProcessorDefinition::Bitcrusher(value) => {
            validate_range(
                diagnostics,
                format!("{path}.bit_depth"),
                value.bit_depth,
                2.0..=16.0,
                "bit_depth must be finite and between 2 and 16",
            );
            validate_range(
                diagnostics,
                format!("{path}.sample_rate_ratio"),
                value.sample_rate_ratio,
                0.01..=1.0,
                "sample_rate_ratio must be finite and between 0.01 and 1",
            );
            validate_range(
                diagnostics,
                format!("{path}.mix"),
                value.mix,
                0.0..=1.0,
                "mix must be finite and between 0 and 1",
            );
        }
        ProcessorDefinition::Chorus(value) => {
            validate_chorus_values(
                diagnostics,
                path,
                value.delay_ms,
                value.rate_hz,
                value.depth,
                value.feedback,
                value.width,
                value.mix,
                5.0..=30.0,
                0.01..=8.0,
                0.0..=0.85,
            );
        }
        ProcessorDefinition::Flanger(value) => {
            validate_chorus_values(
                diagnostics,
                path,
                value.delay_ms,
                value.rate_hz,
                value.depth,
                value.feedback,
                value.width,
                value.mix,
                0.5..=10.0,
                0.01..=10.0,
                -0.95..=0.95,
            );
        }
        ProcessorDefinition::Phaser(value) => {
            if !matches!(value.stages, 2 | 4 | 6 | 8) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "stages must be one of 2, 4, 6, or 8",
                    )
                    .with_path(format!("{path}.stages")),
                );
            }
            validate_range(
                diagnostics,
                format!("{path}.center_hz"),
                value.center_hz,
                100.0..=5_000.0,
                "center_hz must be finite and between 100 and 5000 Hz",
            );
            validate_range(
                diagnostics,
                format!("{path}.sweep_octaves"),
                value.sweep_octaves,
                0.25..=6.0,
                "sweep_octaves must be finite and between 0.25 and 6 octaves",
            );
            validate_range(
                diagnostics,
                format!("{path}.rate_hz"),
                value.rate_hz,
                0.01..=8.0,
                "rate_hz must be finite and between 0.01 and 8 Hz",
            );
            validate_range(
                diagnostics,
                format!("{path}.depth"),
                value.depth,
                0.0..=1.0,
                "depth must be finite and between 0 and 1",
            );
            validate_range(
                diagnostics,
                format!("{path}.feedback"),
                value.feedback,
                -0.9..=0.9,
                "feedback must be finite and between -0.9 and 0.9",
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
        ProcessorDefinition::Compressor(value) => {
            validate_range(
                diagnostics,
                format!("{path}.threshold_db"),
                value.threshold_db,
                -60.0..=0.0,
                "threshold_db must be finite and between -60 and 0 dB",
            );
            validate_range(
                diagnostics,
                format!("{path}.ratio"),
                value.ratio,
                1.0..=20.0,
                "ratio must be finite and between 1 and 20",
            );
            validate_range(
                diagnostics,
                format!("{path}.attack_ms"),
                value.attack_ms,
                0.1..=200.0,
                "attack_ms must be finite and between 0.1 and 200 ms",
            );
            validate_range(
                diagnostics,
                format!("{path}.release_ms"),
                value.release_ms,
                5.0..=2_000.0,
                "release_ms must be finite and between 5 and 2000 ms",
            );
            validate_range(
                diagnostics,
                format!("{path}.knee_db"),
                value.knee_db,
                0.0..=24.0,
                "knee_db must be finite and between 0 and 24 dB",
            );
            validate_range(
                diagnostics,
                format!("{path}.makeup_gain_db"),
                value.makeup_gain_db,
                -12.0..=24.0,
                "makeup_gain_db must be finite and between -12 and 24 dB",
            );
            validate_range(
                diagnostics,
                format!("{path}.mix"),
                value.mix,
                0.0..=1.0,
                "mix must be finite and between 0 and 1",
            );
        }
        ProcessorDefinition::Limiter(value) => {
            validate_range(
                diagnostics,
                format!("{path}.ceiling_db"),
                value.ceiling_db,
                -12.0..=0.0,
                "ceiling_db must be finite and between -12 and 0 dBFS",
            );
            validate_range(
                diagnostics,
                format!("{path}.release_ms"),
                value.release_ms,
                5.0..=1_000.0,
                "release_ms must be finite and between 5 and 1000 ms",
            );
            validate_range(
                diagnostics,
                format!("{path}.input_gain_db"),
                value.input_gain_db,
                -24.0..=24.0,
                "input_gain_db must be finite and between -24 and 24 dB",
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_chorus_values(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    delay_ms: f32,
    rate_hz: f32,
    depth: f32,
    feedback: f32,
    width: f32,
    mix: f32,
    delay_range: std::ops::RangeInclusive<f32>,
    rate_range: std::ops::RangeInclusive<f32>,
    feedback_range: std::ops::RangeInclusive<f32>,
) {
    validate_range(
        diagnostics,
        format!("{path}.delay_ms"),
        delay_ms,
        delay_range,
        "delay_ms is outside its supported range",
    );
    validate_range(
        diagnostics,
        format!("{path}.rate_hz"),
        rate_hz,
        rate_range,
        "rate_hz is outside its supported range",
    );
    validate_range(
        diagnostics,
        format!("{path}.depth"),
        depth,
        0.0..=1.0,
        "depth must be finite and between 0 and 1",
    );
    validate_range(
        diagnostics,
        format!("{path}.feedback"),
        feedback,
        feedback_range,
        "feedback is outside its supported range",
    );
    validate_range(
        diagnostics,
        format!("{path}.width"),
        width,
        0.0..=1.0,
        "width must be finite and between 0 and 1",
    );
    validate_range(
        diagnostics,
        format!("{path}.mix"),
        mix,
        0.0..=1.0,
        "mix must be finite and between 0 and 1",
    );
}

fn validate_modulation(diagnostics: &mut Vec<Diagnostic>, modulation: &ModulationDefinition) {
    validate_modulation_sources(diagnostics, modulation);
    validate_modulation_routes(diagnostics, modulation);
}

fn validate_modulation_sources(
    diagnostics: &mut Vec<Diagnostic>,
    modulation: &ModulationDefinition,
) {
    let mut mseg_count = 0;
    let mut step_count = 0;
    let mut sample_hold_count = 0;
    let mut smooth_random_count = 0;
    if modulation.sources.len() > MAX_VOICE_MODULATION_SOURCES {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                format!(
                    "modulation sources must contain at most {MAX_VOICE_MODULATION_SOURCES} entries"
                ),
            )
            .with_path("modulation.sources"),
        );
    }
    let mut source_ids = HashSet::new();
    for (index, source) in modulation.sources.iter().enumerate() {
        validate_modulation_source_id(diagnostics, &mut source_ids, index, source_id(source));
        match source {
            ModulationSourceDefinition::Lfo(value) => {
                validate_rate(
                    diagnostics,
                    &format!("modulation.sources[{index}].rate"),
                    value.rate,
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
            ModulationSourceDefinition::Mseg(value) => {
                mseg_count += 1;
                validate_mseg(diagnostics, index, value);
            }
            ModulationSourceDefinition::Step(value) => {
                step_count += 1;
                if value.values.is_empty() || value.values.len() > 64 {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "step values must contain between 1 and 64 entries",
                        )
                        .with_path(format!("modulation.sources[{index}].values")),
                    );
                }
                for (value_index, value) in value.values.iter().enumerate() {
                    validate_range(
                        diagnostics,
                        format!("modulation.sources[{index}].values[{value_index}]"),
                        *value,
                        -1.0..=1.0,
                        "step value must be finite and between -1 and 1",
                    );
                }
                validate_rate(
                    diagnostics,
                    &format!("modulation.sources[{index}].rate"),
                    value.rate,
                );
            }
            ModulationSourceDefinition::SampleHold(value) => {
                sample_hold_count += 1;
                validate_rate(
                    diagnostics,
                    &format!("modulation.sources[{index}].rate"),
                    value.rate,
                );
            }
            ModulationSourceDefinition::SmoothRandom(value) => {
                smooth_random_count += 1;
                validate_rate(
                    diagnostics,
                    &format!("modulation.sources[{index}].rate"),
                    value.rate,
                );
            }
        }
    }
    validate_modulation_source_limits(
        diagnostics,
        mseg_count,
        step_count,
        sample_hold_count,
        smooth_random_count,
    );
}

fn validate_modulation_source_limits(
    diagnostics: &mut Vec<Diagnostic>,
    mseg_count: usize,
    step_count: usize,
    sample_hold_count: usize,
    smooth_random_count: usize,
) {
    for (count, limit, name) in [
        (mseg_count, MAX_MSEG_SOURCES, "mseg"),
        (step_count, MAX_STEP_SOURCES, "step"),
        (sample_hold_count, MAX_SAMPLE_HOLD_SOURCES, "sample_hold"),
        (
            smooth_random_count,
            MAX_SMOOTH_RANDOM_SOURCES,
            "smooth_random",
        ),
    ] {
        if count > limit {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    format!("{name} sources must contain at most {limit} entries"),
                )
                .with_path("modulation.sources"),
            );
        }
    }
}

fn validate_modulation_source_id<'a>(
    diagnostics: &mut Vec<Diagnostic>,
    source_ids: &mut HashSet<&'a str>,
    index: usize,
    id: &'a str,
) {
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
    if BUILTIN_SOURCE_IDS.contains(&id) {
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
}

fn source_id(source: &ModulationSourceDefinition) -> &str {
    match source {
        ModulationSourceDefinition::Lfo(value) => &value.id,
        ModulationSourceDefinition::Envelope(value) => &value.id,
        ModulationSourceDefinition::Random(value) => &value.id,
        ModulationSourceDefinition::Mseg(value) => &value.id,
        ModulationSourceDefinition::Step(value) => &value.id,
        ModulationSourceDefinition::SampleHold(value) => &value.id,
        ModulationSourceDefinition::SmoothRandom(value) => &value.id,
    }
}

fn validate_modulation_routes(
    diagnostics: &mut Vec<Diagnostic>,
    modulation: &ModulationDefinition,
) {
    for (index, route) in modulation.routes.iter().enumerate() {
        if !is_source_id(&route.source) {
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
        validate_finite(
            diagnostics,
            format!("modulation.routes[{index}].depth.value"),
            route.depth.value,
            "route depth value must be finite",
        );
    }
}

fn is_source_id(value: &str) -> bool {
    if is_component_id(value) {
        return true;
    }
    let parts: Vec<_> = value.split('.').collect();
    matches!(parts.as_slice(), ["macro", macro_id] if is_component_id(macro_id))
}

fn validate_rate(diagnostics: &mut Vec<Diagnostic>, path: &str, rate: ModulationRateDefinition) {
    let range = match rate.unit {
        ModulationRateUnit::PerSecond => 0.01..=40.0,
        ModulationRateUnit::PerBeat => (1.0 / 64.0)..=16.0,
    };
    validate_range(
        diagnostics,
        format!("{path}.value"),
        rate.value,
        range,
        "modulation rate must be finite and within its unit range",
    );
}

fn validate_mseg(diagnostics: &mut Vec<Diagnostic>, index: usize, mseg: &MsegDefinition) {
    let path = format!("modulation.sources[{index}]");
    validate_range(
        diagnostics,
        format!("{path}.initial_value"),
        mseg.initial_value,
        -1.0..=1.0,
        "mseg initial_value must be finite and between -1 and 1",
    );
    if mseg.segments.is_empty() || mseg.segments.len() > 64 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "mseg segments must contain between 1 and 64 entries",
            )
            .with_path(format!("{path}.segments")),
        );
    }
    for (segment_index, segment) in mseg.segments.iter().enumerate() {
        validate_range(
            diagnostics,
            format!("{path}.segments[{segment_index}].target"),
            segment.target,
            -1.0..=1.0,
            "mseg target must be finite and between -1 and 1",
        );
        let duration_range = match segment.duration.unit {
            ModulationDurationUnit::Seconds => f32::EPSILON..=100.0,
            ModulationDurationUnit::Beats => (1.0 / 128.0)..=64.0,
        };
        validate_range(
            diagnostics,
            format!("{path}.segments[{segment_index}].duration.value"),
            segment.duration.value,
            duration_range,
            "mseg duration must be finite and within its unit range",
        );
    }
    if let Some(loop_range) = mseg.loop_range
        && (loop_range.start_segment >= loop_range.end_segment
            || loop_range.end_segment > mseg.segments.len())
    {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "mseg loop range must be within the segment list and non-empty",
            )
            .with_path(format!("{path}.loop_range")),
        );
    }
}

fn validate_macros(diagnostics: &mut Vec<Diagnostic>, macros: &[MacroDefinition]) {
    if macros.len() > 16 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "macros must contain at most 16 entries",
            )
            .with_path("macros"),
        );
    }
    let mut ids = HashSet::new();
    for (index, value) in macros.iter().enumerate() {
        let path = format!("macros[{index}]");
        if !is_component_id(&value.id) {
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::ParameterIdInvalid, "macro id is invalid")
                    .with_path(format!("{path}.id")),
            );
        }
        if !ids.insert(&value.id) {
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::IdDuplicated, "macro id must be unique")
                    .with_path(format!("{path}.id")),
            );
        }
        validate_range(
            diagnostics,
            format!("{path}.default"),
            value.default,
            0.0..=1.0,
            "macro default must be finite and between 0 and 1",
        );
    }
}

fn validate_vectors(
    diagnostics: &mut Vec<Diagnostic>,
    vectors: &[VectorDefinition],
    layers: &[LayerDefinition],
) {
    if vectors.len() > 8 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "vectors must contain at most 8 entries",
            )
            .with_path("vectors"),
        );
    }
    let mut vector_ids = HashSet::new();
    let mut assigned_layers = HashSet::new();
    for (index, vector) in vectors.iter().enumerate() {
        let path = format!("vectors[{index}]");
        let (id, layer_ids_for_vector, axes) = match vector {
            VectorDefinition::TwoWay {
                id,
                layer_a,
                layer_b,
                position,
                ..
            } => (id, vec![layer_a, layer_b], vec![("position", *position)]),
            VectorDefinition::FourWay {
                id,
                top_left,
                top_right,
                bottom_left,
                bottom_right,
                x,
                y,
                ..
            } => (
                id,
                vec![top_left, top_right, bottom_left, bottom_right],
                vec![("x", *x), ("y", *y)],
            ),
        };
        if !is_component_id(id) {
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::ParameterIdInvalid, "vector id is invalid")
                    .with_path(format!("{path}.id")),
            );
        }
        if !vector_ids.insert(id) {
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::IdDuplicated, "vector id must be unique")
                    .with_path(format!("{path}.id")),
            );
        }
        let mut local_layers = HashSet::new();
        for (layer_index, layer_id) in layer_ids_for_vector.iter().enumerate() {
            let Some(layer) = layers.iter().find(|layer| layer.id == **layer_id) else {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ParameterNotFound,
                        "vector layer does not exist",
                    )
                    .with_path(format!("{path}.layer_{layer_index}")),
                );
                continue;
            };
            if !layer.enabled {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "vector layer must be enabled",
                    )
                    .with_path(format!("{path}.layer_{layer_index}")),
                );
            }
            if !local_layers.insert(*layer_id) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticCode::IdDuplicated, "vector layers must be unique")
                        .with_path(format!("{path}.layer_{layer_index}")),
                );
            }
            if !assigned_layers.insert(*layer_id) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::IdDuplicated,
                        "a layer may belong to only one vector",
                    )
                    .with_path(format!("{path}.layer_{layer_index}")),
                );
            }
        }
        for (axis, value) in axes {
            validate_range(
                diagnostics,
                format!("{path}.{axis}"),
                value,
                0.0..=1.0,
                "vector axis must be finite and between 0 and 1",
            );
        }
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
    validate_asset_reference_at(diagnostics, &format!("{path}.asset"), asset);
}

fn validate_asset_reference_at(
    diagnostics: &mut Vec<Diagnostic>,
    asset_path: &str,
    asset: &AssetReference,
) {
    if asset.path.trim().is_empty() {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::RequiredFieldMissing,
                "asset path must not be empty",
            )
            .with_path(format!("{asset_path}.path")),
        );
    }
    if let Some(hash) = &asset.sha256 {
        let valid = hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !valid {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "asset sha256 must be 64 hexadecimal characters",
                )
                .with_path(format!("{asset_path}.sha256")),
            );
        }
    }
}

fn validate_granular(diagnostics: &mut Vec<Diagnostic>, path: &str, granular: &GranularDefinition) {
    let granular_path = format!("{path}.generator.granular");
    validate_asset_reference(diagnostics, &granular_path, &granular.asset);
    validate_range(
        diagnostics,
        format!("{granular_path}.root_note"),
        f32::from(granular.root_note),
        0.0..=127.0,
        "granular root_note must be between 0 and 127",
    );
    validate_granular_seconds(
        diagnostics,
        &granular_path,
        "region.start_seconds",
        granular.region.start_seconds,
    );
    if let Some(end_seconds) = granular.region.end_seconds {
        validate_granular_seconds(
            diagnostics,
            &granular_path,
            "region.end_seconds",
            end_seconds,
        );
        if granular.region.start_seconds.is_finite()
            && end_seconds.is_finite()
            && end_seconds <= granular.region.start_seconds
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::InvalidGrainRegion,
                    "granular region end must be greater than start",
                )
                .with_path(format!("{granular_path}.region.end_seconds")),
            );
        }
    }
    validate_granular_range(
        diagnostics,
        format!("{granular_path}.position"),
        granular.position,
        GRANULAR_POSITION.min..=GRANULAR_POSITION.max,
        "granular position must be finite and between 0 and 1",
    );
    validate_granular_range(
        diagnostics,
        format!("{granular_path}.grain_size"),
        granular.grain_size,
        GRAIN_SIZE.min..=GRAIN_SIZE.max,
        "granular grain_size must be finite and between 0.005 and 0.5 seconds",
    );
    validate_granular_range(
        diagnostics,
        format!("{granular_path}.density"),
        granular.density,
        GRAIN_DENSITY.min..=GRAIN_DENSITY.max,
        "granular density must be finite and between 1 and 100 grains per second",
    );
    validate_granular_range(
        diagnostics,
        format!("{granular_path}.pitch"),
        granular.pitch,
        GRAIN_PITCH.min..=GRAIN_PITCH.max,
        "granular pitch must be finite and between -2400 and 2400 cents",
    );
    validate_granular_range(
        diagnostics,
        format!("{granular_path}.randomness"),
        granular.randomness,
        GRAIN_RANDOMNESS.min..=GRAIN_RANDOMNESS.max,
        "granular randomness must be finite and between 0 and 1",
    );
    validate_granular_range(
        diagnostics,
        format!("{granular_path}.pan_spread"),
        granular.pan_spread,
        GRAIN_PAN_SPREAD.min..=GRAIN_PAN_SPREAD.max,
        "granular pan_spread must be finite and between 0 and 1",
    );
}

fn validate_granular_seconds(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    field: &str,
    value: f32,
) {
    if !value.is_finite() || value < 0.0 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::InvalidGrainRegion,
                "granular region time must be finite and non-negative",
            )
            .with_path(format!("{path}.{field}")),
        );
    }
}

fn validate_granular_range(
    diagnostics: &mut Vec<Diagnostic>,
    path: String,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    message: &str,
) {
    if !value.is_finite() || !range.contains(&value) {
        diagnostics.push(
            Diagnostic::error(DiagnosticCode::InvalidGrainParameter, message).with_path(path),
        );
    }
}

#[allow(clippy::too_many_lines)]
fn validate_wave_sequence(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    sequence: &WaveSequenceDefinition,
) {
    let sequence_path = format!("{path}.generator.wave_sequence");
    if !(1..=128).contains(&sequence.steps.len()) {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::InvalidSequence,
                "wave sequence steps must contain between 1 and 128 steps",
            )
            .with_path(format!("{sequence_path}.steps")),
        );
    }
    validate_range(
        diagnostics,
        format!("{sequence_path}.root_note"),
        f32::from(sequence.root_note),
        0.0..=127.0,
        "wave sequence root_note must be between 0 and 127",
    );
    validate_range(
        diagnostics,
        format!("{sequence_path}.crossfade"),
        sequence.crossfade,
        0.0..=0.5,
        "wave sequence crossfade must be finite and between 0 and 0.5",
    );
    let mut ids = HashSet::new();
    for (index, step) in sequence.steps.iter().enumerate() {
        let step_path = format!("{sequence_path}.steps[{index}]");
        if !is_component_id(&step.id) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ParameterIdInvalid,
                    "wave sequence step id must use component id syntax",
                )
                .with_path(format!("{step_path}.id")),
            );
        }
        if !ids.insert(&step.id) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::IdDuplicated,
                    "wave sequence step id must be unique",
                )
                .with_path(format!("{step_path}.id")),
            );
        }
        validate_asset_reference(diagnostics, &step_path, &step.asset);
        validate_sequence_region(diagnostics, &step_path, step.region);
        validate_sequence_duration(diagnostics, &step_path, step.duration);
        validate_range(
            diagnostics,
            format!("{step_path}.gain_db"),
            step.gain_db,
            -60.0..=12.0,
            "wave sequence step gain_db must be finite and between -60 and 12 dB",
        );
        validate_range(
            diagnostics,
            format!("{step_path}.pitch_cents"),
            step.pitch_cents,
            -2400.0..=2400.0,
            "wave sequence step pitch_cents must be finite and between -2400 and 2400",
        );
    }
}

fn validate_sequence_region(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    region: SampleRegionDefinition,
) {
    if !region.start_seconds.is_finite() || region.start_seconds < 0.0 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::InvalidSequence,
                "wave sequence region start must be finite and non-negative",
            )
            .with_path(format!("{path}.region.start_seconds")),
        );
    }
    if let Some(end_seconds) = region.end_seconds {
        if !end_seconds.is_finite() || end_seconds < 0.0 {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::InvalidSequence,
                    "wave sequence region end must be finite and non-negative",
                )
                .with_path(format!("{path}.region.end_seconds")),
            );
        }
        if region.start_seconds.is_finite()
            && end_seconds.is_finite()
            && end_seconds <= region.start_seconds
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::InvalidSequence,
                    "wave sequence region end must be greater than start",
                )
                .with_path(format!("{path}.region.end_seconds")),
            );
        }
    }
}

fn validate_sequence_duration(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    duration: WaveSequenceDurationDefinition,
) {
    let (value, unit) = match duration {
        WaveSequenceDurationDefinition::Seconds { value } => (value, "seconds"),
        WaveSequenceDurationDefinition::Beats { value } => (value, "beats"),
    };
    if !value.is_finite() || value <= 0.0 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::InvalidStepDuration,
                format!("wave sequence {unit} duration must be finite and greater than zero"),
            )
            .with_path(format!("{path}.duration.value")),
        );
    }
}

fn validate_sample_playback_definition(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    playback: SampleZonePlaybackDefinition,
) {
    validate_sample_time_definition(diagnostics, path, playback);
    validate_sample_seconds(
        diagnostics,
        path,
        "region.start_seconds",
        playback.region.start_seconds,
    );
    if let Some(end_seconds) = playback.region.end_seconds {
        validate_sample_seconds(diagnostics, path, "region.end_seconds", end_seconds);
        if playback.region.start_seconds.is_finite()
            && end_seconds.is_finite()
            && end_seconds <= playback.region.start_seconds
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "sample region end must be greater than start",
                )
                .with_path(format!("{path}.playback.region.end_seconds")),
            );
        }
    }
    if let Some(loop_definition) = playback.r#loop {
        validate_sample_seconds(
            diagnostics,
            path,
            "loop.start_seconds",
            loop_definition.start_seconds,
        );
        validate_sample_seconds(
            diagnostics,
            path,
            "loop.end_seconds",
            loop_definition.end_seconds,
        );
        validate_sample_seconds(
            diagnostics,
            path,
            "loop.crossfade_seconds",
            loop_definition.crossfade_seconds,
        );
        if loop_definition.start_seconds.is_finite()
            && playback.region.start_seconds.is_finite()
            && loop_definition.start_seconds < playback.region.start_seconds
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "sample loop start must be inside the playback region",
                )
                .with_path(format!("{path}.playback.loop.start_seconds")),
            );
        }
        if loop_definition.end_seconds.is_finite()
            && loop_definition.start_seconds.is_finite()
            && loop_definition.end_seconds <= loop_definition.start_seconds
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "sample loop end must be greater than loop start",
                )
                .with_path(format!("{path}.playback.loop.end_seconds")),
            );
        }
        if let Some(end_seconds) = playback.region.end_seconds
            && loop_definition.end_seconds.is_finite()
            && end_seconds.is_finite()
            && loop_definition.end_seconds > end_seconds
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "sample loop end must be inside the playback region",
                )
                .with_path(format!("{path}.playback.loop.end_seconds")),
            );
        }
        if loop_definition.crossfade_seconds.is_finite()
            && loop_definition.start_seconds.is_finite()
            && loop_definition.end_seconds.is_finite()
            && loop_definition.crossfade_seconds
                > (loop_definition.end_seconds - loop_definition.start_seconds) * 0.5
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "sample loop crossfade must not exceed half the loop length",
                )
                .with_path(format!("{path}.playback.loop.crossfade_seconds")),
            );
        }
    }
}

fn validate_sample_time_definition(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    playback: SampleZonePlaybackDefinition,
) {
    match playback.time {
        SampleTimeDefinition::Resample => {}
        SampleTimeDefinition::FixedStretch { ratio } => {
            if !ratio.is_finite() || !(0.5..=2.0).contains(&ratio) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::InvalidStretchRatio,
                        "sample fixed stretch ratio must be finite and between 0.5 and 2.0",
                    )
                    .with_path(format!("{path}.playback.time.ratio")),
                );
            }
            reject_reverse_stretch(diagnostics, path, playback.direction);
        }
        SampleTimeDefinition::TempoSync { source_bpm } => {
            if !source_bpm.is_finite() || source_bpm <= 0.0 {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::InvalidSourceTempo,
                        "sample source_bpm must be finite and greater than zero",
                    )
                    .with_path(format!("{path}.playback.time.source_bpm")),
                );
            }
            reject_reverse_stretch(diagnostics, path, playback.direction);
        }
    }
}

fn reject_reverse_stretch(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    direction: SamplePlaybackDirection,
) {
    if direction == SamplePlaybackDirection::Reverse {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::UnsupportedPlaybackCombination,
                "reverse sample playback cannot use time stretch",
            )
            .with_path(format!("{path}.playback")),
        );
    }
}

fn validate_sample_seconds(diagnostics: &mut Vec<Diagnostic>, path: &str, field: &str, value: f32) {
    if !value.is_finite() || value < 0.0 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "sample playback time must be finite and non-negative",
            )
            .with_path(format!("{path}.playback.{field}")),
        );
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
            PULSE_WIDTH.min..=PULSE_WIDTH.max,
            "pulse_width must be finite and between 0.05 and 0.95",
        );
    }
    if let Some(hard_sync) = oscillator.hard_sync {
        validate_range(
            diagnostics,
            format!("{path}.generator.oscillator.hard_sync.ratio"),
            hard_sync.ratio,
            SYNC_RATIO.min..=SYNC_RATIO.max,
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
            WAVESHAPE.min..=WAVESHAPE.max,
            "waveshaping amount must be finite and between 0 and 1",
        );
    }
    validate_oscillator_extensions(diagnostics, path, oscillator);
    if let Some(unison) = oscillator.unison {
        validate_unison(
            diagnostics,
            &format!("{path}.generator.oscillator.unison"),
            unison,
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

fn validate_oscillator_extensions(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    oscillator: &OscillatorDefinition,
) {
    if let Some(phase_distortion) = oscillator.phase_distortion {
        validate_range(
            diagnostics,
            format!("{path}.generator.oscillator.phase_distortion.amount"),
            phase_distortion.amount,
            PHASE_DISTORTION.min..=PHASE_DISTORTION.max,
            "phase distortion amount must be finite and between 0 and 1",
        );
        if oscillator.waveform != OscillatorWaveform::Sine {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "phase distortion requires a sine waveform",
                )
                .with_path(format!("{path}.generator.oscillator.phase_distortion")),
            );
        }
        if oscillator.hard_sync.is_some() {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "phase distortion cannot be combined with hard sync",
                )
                .with_path(format!("{path}.generator.oscillator.phase_distortion")),
            );
        }
    }
    if let Some(wavefold) = oscillator.wavefold {
        validate_range(
            diagnostics,
            format!("{path}.generator.oscillator.wavefold.amount"),
            wavefold.amount,
            WAVEFOLD.min..=WAVEFOLD.max,
            "wavefold amount must be finite and between 0 and 1",
        );
    }
    if let Some(feedback) = oscillator.feedback {
        validate_range(
            diagnostics,
            format!("{path}.generator.oscillator.feedback.amount"),
            feedback.amount,
            OSCILLATOR_FEEDBACK.min..=OSCILLATOR_FEEDBACK.max,
            "oscillator feedback amount must be finite and between 0 and 1",
        );
        if oscillator.waveform != OscillatorWaveform::Sine {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "oscillator feedback requires a sine waveform",
                )
                .with_path(format!("{path}.generator.oscillator.feedback")),
            );
        }
        if oscillator.hard_sync.is_some() {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "oscillator feedback cannot be combined with hard sync",
                )
                .with_path(format!("{path}.generator.oscillator.feedback")),
            );
        }
    }
}

fn validate_wavetable(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    wavetable: &WavetableDefinition,
) {
    let wavetable_path = format!("{path}.generator.wavetable");
    let frame_length = usize::from(wavetable.frame_length);
    if !(64..=4096).contains(&frame_length) || !frame_length.is_power_of_two() {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "wavetable frame_length must be a power of two between 64 and 4096",
            )
            .with_path(format!("{wavetable_path}.frame_length")),
        );
    }
    validate_range(
        diagnostics,
        format!("{wavetable_path}.position"),
        wavetable.position,
        WAVETABLE_POSITION.min..=WAVETABLE_POSITION.max,
        "wavetable position must be finite and between 0 and 1",
    );
    validate_range(
        diagnostics,
        format!("{wavetable_path}.phase"),
        wavetable.phase,
        0.0..=1.0,
        "wavetable phase must be finite and between 0 and 1",
    );
    validate_asset_reference(diagnostics, &wavetable_path, &wavetable.asset);
    if let Some(unison) = wavetable.unison {
        validate_unison(diagnostics, &format!("{wavetable_path}.unison"), unison);
    }
}

fn validate_spectral(diagnostics: &mut Vec<Diagnostic>, path: &str, spectral: &SpectralDefinition) {
    let spectral_path = format!("{path}.generator.spectral");
    validate_asset_reference_at(
        diagnostics,
        &format!("{spectral_path}.asset_a"),
        &spectral.asset_a,
    );
    if let Some(asset_b) = &spectral.asset_b {
        validate_asset_reference_at(diagnostics, &format!("{spectral_path}.asset_b"), asset_b);
    }
    validate_range(
        diagnostics,
        format!("{spectral_path}.root_note"),
        f32::from(spectral.root_note),
        0.0..=127.0,
        "spectral root_note must be between 0 and 127",
    );
    validate_range(
        diagnostics,
        format!("{spectral_path}.position"),
        spectral.position,
        SPECTRAL_POSITION.min..=SPECTRAL_POSITION.max,
        "spectral position must be finite and between 0 and 1",
    );
    validate_range(
        diagnostics,
        format!("{spectral_path}.freeze"),
        spectral.freeze,
        SPECTRAL_FREEZE.min..=SPECTRAL_FREEZE.max,
        "spectral freeze must be finite and between 0 and 1",
    );
    validate_range(
        diagnostics,
        format!("{spectral_path}.blur_seconds"),
        spectral.blur_seconds,
        SPECTRAL_BLUR.min..=SPECTRAL_BLUR.max,
        "spectral blur_seconds must be finite and between 0 and 1 second",
    );
    validate_range(
        diagnostics,
        format!("{spectral_path}.shift_hz"),
        spectral.shift_hz,
        SPECTRAL_SHIFT.min..=SPECTRAL_SHIFT.max,
        "spectral shift_hz must be finite and between -12000 and 12000 Hz",
    );
    validate_range(
        diagnostics,
        format!("{spectral_path}.morph"),
        spectral.morph,
        SPECTRAL_MORPH.min..=SPECTRAL_MORPH.max,
        "spectral morph must be finite and between 0 and 1",
    );
    if spectral.asset_b.is_none() && spectral.morph != 0.0 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::DefinitionError,
                "spectral morph requires asset_b",
            )
            .with_path(format!("{spectral_path}.morph")),
        );
    }
    if !matches!(spectral.fft_size, 1024 | 2048 | 4096) {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "spectral fft_size must be 1024, 2048, or 4096",
            )
            .with_path(format!("{spectral_path}.fft_size")),
        );
    }
}

#[allow(clippy::too_many_lines)]
fn validate_operator_modulation(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    operator_modulation: &OperatorModulationDefinition,
) {
    let operator_path = format!("{path}.generator.operator_modulation");
    if operator_modulation.operators.len() != 4 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "operator_modulation operators must contain exactly four operators",
            )
            .with_path(format!("{operator_path}.operators")),
        );
    }

    let topology = operator_modulation.algorithm.topology();
    let (amount_range, amount_message) = match operator_modulation.mode {
        OperatorModulationMode::Phase | OperatorModulationMode::Frequency => (
            OPERATOR_PHASE_FREQUENCY_AMOUNT_MIN..=OPERATOR_PHASE_FREQUENCY_AMOUNT_MAX,
            range_message(
                "modulation_amount",
                OPERATOR_PHASE_FREQUENCY_AMOUNT_MIN,
                OPERATOR_PHASE_FREQUENCY_AMOUNT_MAX,
            ),
        ),
        OperatorModulationMode::Amplitude | OperatorModulationMode::Ring => (
            OPERATOR_AM_RING_AMOUNT_MIN..=OPERATOR_AM_RING_AMOUNT_MAX,
            range_message(
                "modulation_amount",
                OPERATOR_AM_RING_AMOUNT_MIN,
                OPERATOR_AM_RING_AMOUNT_MAX,
            ),
        ),
    };
    let ratio_message = range_message("operator ratio", OPERATOR_RATIO_MIN, OPERATOR_RATIO_MAX);
    let detune_message = range_message(
        "operator detune_cents",
        OPERATOR_DETUNE_MIN,
        OPERATOR_DETUNE_MAX,
    );
    let level_message = range_message("operator level", OPERATOR_LEVEL_MIN, OPERATOR_LEVEL_MAX);
    let feedback_message = range_message(
        "operator feedback",
        OPERATOR_FEEDBACK_MIN,
        OPERATOR_FEEDBACK_MAX,
    );
    let phase_message = range_message("operator phase", OPERATOR_PHASE_MIN, OPERATOR_PHASE_MAX);

    for (index, operator) in operator_modulation.operators.iter().enumerate() {
        let current_path = format!("{operator_path}.operators[{index}]");
        validate_range(
            diagnostics,
            format!("{current_path}.ratio"),
            operator.ratio,
            OPERATOR_RATIO_MIN..=OPERATOR_RATIO_MAX,
            &ratio_message,
        );
        validate_range(
            diagnostics,
            format!("{current_path}.detune_cents"),
            operator.detune_cents,
            OPERATOR_DETUNE_MIN..=OPERATOR_DETUNE_MAX,
            &detune_message,
        );
        validate_range(
            diagnostics,
            format!("{current_path}.level"),
            operator.level,
            OPERATOR_LEVEL_MIN..=OPERATOR_LEVEL_MAX,
            &level_message,
        );
        validate_range(
            diagnostics,
            format!("{current_path}.modulation_amount"),
            operator.modulation_amount,
            amount_range.clone(),
            &amount_message,
        );
        validate_range(
            diagnostics,
            format!("{current_path}.feedback"),
            operator.feedback,
            OPERATOR_FEEDBACK_MIN..=OPERATOR_FEEDBACK_MAX,
            &feedback_message,
        );
        validate_range(
            diagnostics,
            format!("{current_path}.phase"),
            operator.phase,
            OPERATOR_PHASE_MIN..=OPERATOR_PHASE_MAX,
            &phase_message,
        );
        validate_adsr(diagnostics, &current_path, operator.envelope);

        if matches!(
            operator_modulation.mode,
            OperatorModulationMode::Amplitude | OperatorModulationMode::Ring
        ) && operator.feedback != 0.0
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "amplitude and ring modulation do not support operator feedback",
                )
                .with_path(format!("{current_path}.feedback")),
            );
        }
    }

    if operator_modulation.operators.len() == 4 {
        let carrier_indices = (0..4).filter(|index| topology.carrier_mask & (1_u8 << index) != 0);
        if carrier_indices
            .clone()
            .all(|index| operator_modulation.operators[index].level == 0.0)
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "at least one carrier operator must have a non-zero level",
                )
                .with_path(format!("{operator_path}.operators")),
            );
        }
        for (index, operator) in operator_modulation.operators.iter().enumerate() {
            let is_carrier = topology.carrier_mask & (1_u8 << index) != 0;
            if !is_carrier && operator.level != 0.0 {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::DefinitionError,
                        "non-carrier operator level must be zero",
                    )
                    .with_path(format!("{operator_path}.operators[{index}].level")),
                );
            }
            let has_output = topology
                .incoming_masks
                .iter()
                .any(|mask| mask & (1_u8 << index) != 0);
            if !has_output && operator.modulation_amount != 0.0 {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::DefinitionError,
                        "operator without an output connection must have zero modulation_amount",
                    )
                    .with_path(format!(
                        "{operator_path}.operators[{index}].modulation_amount"
                    )),
                );
            }
        }
    }

    if let Some(unison) = operator_modulation.unison {
        validate_unison(diagnostics, &format!("{operator_path}.unison"), unison);
        if unison.voices > 4 {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::GeneratorResourceLimitExceeded,
                    "operator modulation unison voices must not exceed 4",
                )
                .with_path(format!("{operator_path}.unison.voices")),
            );
        }
    }
}

fn validate_unison(diagnostics: &mut Vec<Diagnostic>, path: &str, unison: UnisonDefinition) {
    if !(2..=8).contains(&unison.voices) {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "unison voices must be between 2 and 8",
            )
            .with_path(format!("{path}.voices")),
        );
    }
    validate_range(
        diagnostics,
        format!("{path}.detune_cents"),
        unison.detune_cents,
        UNISON_DETUNE.min..=UNISON_DETUNE.max,
        "unison detune_cents must be finite and between 0 and 100",
    );
    validate_range(
        diagnostics,
        format!("{path}.stereo_spread"),
        unison.stereo_spread,
        UNISON_SPREAD.min..=UNISON_SPREAD.max,
        "unison stereo_spread must be finite and between 0 and 1",
    );
    validate_range(
        diagnostics,
        format!("{path}.phase_spread"),
        unison.phase_spread,
        0.0..=1.0,
        "unison phase_spread must be finite and between 0 and 1",
    );
}

fn validate_noise(diagnostics: &mut Vec<Diagnostic>, path: &str, noise: &NoiseDefinition) {
    validate_range(
        diagnostics,
        format!("{path}.generator.noise.stereo_correlation"),
        noise.stereo_correlation,
        NOISE_CORRELATION.min..=NOISE_CORRELATION.max,
        "stereo_correlation must be finite and between 0 and 1",
    );
}

fn validate_physical_exciter(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    exciter: PhysicalExciterDefinition,
) {
    if let PhysicalExciterDefinition::NoiseBurst {
        duration_seconds,
        brightness,
        ..
    } = exciter
    {
        validate_range(
            diagnostics,
            format!("{path}.duration_seconds"),
            duration_seconds,
            PHYSICAL_EXCITER_DURATION_SECONDS_MIN..=PHYSICAL_EXCITER_DURATION_SECONDS_MAX,
            "noise burst duration_seconds must be finite and between 0.0005 and 0.1 seconds",
        );
        validate_range(
            diagnostics,
            format!("{path}.brightness"),
            brightness,
            0.0..=1.0,
            "noise burst brightness must be finite and between 0 and 1",
        );
    }
}

fn validate_physical_string(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    physical_string: &PhysicalStringDefinition,
) {
    let string_path = format!("{path}.generator.physical_string");
    validate_physical_exciter(
        diagnostics,
        &format!("{string_path}.exciter"),
        physical_string.exciter,
    );
    validate_range(
        diagnostics,
        format!("{string_path}.decay_seconds"),
        physical_string.decay_seconds,
        PHYSICAL_STRING_DECAY_SECONDS.min..=PHYSICAL_STRING_DECAY_SECONDS.max,
        "physical string decay_seconds must be finite and between 0.05 and 20 seconds",
    );
    validate_range(
        diagnostics,
        format!("{string_path}.brightness"),
        physical_string.brightness,
        PHYSICAL_STRING_BRIGHTNESS.min..=PHYSICAL_STRING_BRIGHTNESS.max,
        "physical string brightness must be finite and between 0 and 1",
    );
    validate_range(
        diagnostics,
        format!("{string_path}.stiffness"),
        physical_string.stiffness,
        PHYSICAL_STRING_STIFFNESS.min..=PHYSICAL_STRING_STIFFNESS.max,
        "physical string stiffness must be finite and between 0 and 1",
    );
}

fn validate_modal(diagnostics: &mut Vec<Diagnostic>, path: &str, modal: &ModalDefinition) {
    let modal_path = format!("{path}.generator.modal");
    validate_physical_exciter(diagnostics, &format!("{modal_path}.exciter"), modal.exciter);
    if !matches!(modal.mode_count, 4 | 8 | 12 | 16 | 20 | 24) {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "modal mode_count must be one of 4, 8, 12, 16, 20, or 24",
            )
            .with_path(format!("{modal_path}.mode_count")),
        );
    }
    validate_range(
        diagnostics,
        format!("{modal_path}.structure"),
        modal.structure,
        MODAL_STRUCTURE.min..=MODAL_STRUCTURE.max,
        "modal structure must be finite and between 0 and 1",
    );
    validate_range(
        diagnostics,
        format!("{modal_path}.brightness"),
        modal.brightness,
        MODAL_BRIGHTNESS.min..=MODAL_BRIGHTNESS.max,
        "modal brightness must be finite and between 0 and 1",
    );
    validate_range(
        diagnostics,
        format!("{modal_path}.decay"),
        modal.decay,
        MODAL_DECAY.min..=MODAL_DECAY.max,
        "modal decay must be finite and between 0 and 1",
    );
}

fn validate_additive(diagnostics: &mut Vec<Diagnostic>, path: &str, additive: &AdditiveDefinition) {
    let additive_path = format!("{path}.generator.additive");
    if !(1..=MAX_PARTIALS).contains(&additive.partials.len()) {
        diagnostics.push(
            Diagnostic::error(
                if additive.partials.is_empty() {
                    DiagnosticCode::RequiredFieldMissing
                } else {
                    DiagnosticCode::GeneratorResourceLimitExceeded
                },
                format!("additive partials must contain between 1 and {MAX_PARTIALS} entries"),
            )
            .with_path(format!("{additive_path}.partials")),
        );
    }
    validate_range(
        diagnostics,
        format!("{additive_path}.morph"),
        additive.morph,
        ADDITIVE_MORPH.min..=ADDITIVE_MORPH.max,
        "additive morph must be finite and between 0 and 1",
    );
    validate_range(
        diagnostics,
        format!("{additive_path}.spectrum_tilt_db_per_octave"),
        additive.spectrum_tilt_db_per_octave,
        ADDITIVE_SPECTRUM_TILT.min..=ADDITIVE_SPECTRUM_TILT.max,
        "additive spectrum tilt must be finite and between -24 and 12 dB per octave",
    );
    validate_range(
        diagnostics,
        format!("{additive_path}.inharmonicity"),
        additive.inharmonicity,
        ADDITIVE_INHARMONICITY.min..=ADDITIVE_INHARMONICITY.max,
        "additive inharmonicity must be finite and between 0 and 1",
    );

    let mut ids = HashSet::new();
    let mut has_signal = false;
    for (index, partial) in additive.partials.iter().enumerate() {
        let partial_path = format!("{additive_path}.partials[{index}]");
        if partial.id.trim().is_empty() {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::RequiredFieldMissing,
                    "additive partial id must not be empty",
                )
                .with_path(format!("{partial_path}.id")),
            );
        }
        if !ids.insert(partial.id.clone()) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::IdDuplicated,
                    "additive partial id must be unique",
                )
                .with_path(format!("{partial_path}.id")),
            );
        }
        validate_range(
            diagnostics,
            format!("{partial_path}.ratio"),
            partial.ratio,
            0.125..=64.0,
            "additive partial ratio must be finite and between 0.125 and 64",
        );
        validate_range(
            diagnostics,
            format!("{partial_path}.amplitude_a"),
            partial.amplitude_a,
            0.0..=1.0,
            "additive partial amplitude_a must be finite and between 0 and 1",
        );
        validate_range(
            diagnostics,
            format!("{partial_path}.amplitude_b"),
            partial.amplitude_b,
            0.0..=1.0,
            "additive partial amplitude_b must be finite and between 0 and 1",
        );
        validate_range(
            diagnostics,
            format!("{partial_path}.phase"),
            partial.phase,
            0.0..=1.0,
            "additive partial phase must be finite and between 0 and 1",
        );
        has_signal |= partial.amplitude_a > 0.0 || partial.amplitude_b > 0.0;
        if let Some(envelope) = partial.envelope {
            validate_adsr(diagnostics, &partial_path, envelope);
        }
    }
    if !additive.partials.is_empty() && !has_signal {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::DefinitionError,
                "additive spectra must contain at least one non-zero amplitude",
            )
            .with_path(format!("{additive_path}.partials")),
        );
    }
}

fn validate_formant(diagnostics: &mut Vec<Diagnostic>, path: &str, formant: &FormantDefinition) {
    let formant_path = format!("{path}.generator.formant");
    if !(1..=8).contains(&formant.profiles.len()) {
        diagnostics.push(
            Diagnostic::error(
                if formant.profiles.is_empty() {
                    DiagnosticCode::RequiredFieldMissing
                } else {
                    DiagnosticCode::GeneratorResourceLimitExceeded
                },
                "formant profiles must contain between 1 and 8 entries",
            )
            .with_path(format!("{formant_path}.profiles")),
        );
    }
    if !(1..=MAX_PARTIALS).contains(&usize::from(formant.partial_count)) {
        diagnostics.push(
            Diagnostic::error(
                if formant.partial_count == 0 {
                    DiagnosticCode::RequiredFieldMissing
                } else {
                    DiagnosticCode::GeneratorResourceLimitExceeded
                },
                format!("formant partial_count must be between 1 and {MAX_PARTIALS}"),
            )
            .with_path(format!("{formant_path}.partial_count")),
        );
    }
    validate_range(
        diagnostics,
        format!("{formant_path}.vowel_position"),
        formant.vowel_position,
        FORMANT_VOWEL_POSITION.min..=FORMANT_VOWEL_POSITION.max,
        "formant vowel_position must be finite and between 0 and 1",
    );
    validate_range(
        diagnostics,
        format!("{formant_path}.formant_shift_cents"),
        formant.formant_shift_cents,
        FORMANT_SHIFT.min..=FORMANT_SHIFT.max,
        "formant_shift_cents must be finite and between -2400 and 2400",
    );
    validate_range(
        diagnostics,
        format!("{formant_path}.throat"),
        formant.throat,
        FORMANT_THROAT.min..=FORMANT_THROAT.max,
        "formant throat must be finite and between 0 and 1",
    );
    validate_range(
        diagnostics,
        format!("{formant_path}.spectral_tilt_db_per_octave"),
        formant.spectral_tilt_db_per_octave,
        FORMANT_SPECTRAL_TILT.min..=FORMANT_SPECTRAL_TILT.max,
        "formant spectral tilt must be finite and between -24 and 12 dB per octave",
    );

    let mut ids = HashSet::new();
    for (profile_index, profile) in formant.profiles.iter().enumerate() {
        let profile_path = format!("{formant_path}.profiles[{profile_index}]");
        if profile.id.trim().is_empty() {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::RequiredFieldMissing,
                    "formant profile id must not be empty",
                )
                .with_path(format!("{profile_path}.id")),
            );
        }
        if !ids.insert(profile.id.clone()) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::IdDuplicated,
                    "formant profile id must be unique",
                )
                .with_path(format!("{profile_path}.id")),
            );
        }
        validate_formant_bands(diagnostics, &profile_path, &profile.formants);
    }
}

fn validate_formant_bands(
    diagnostics: &mut Vec<Diagnostic>,
    profile_path: &str,
    bands: &[FormantBandDefinition],
) {
    if bands.len() != 5 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::DefinitionError,
                "formant profile must contain exactly 5 bands",
            )
            .with_path(format!("{profile_path}.formants")),
        );
    }
    let mut previous_frequency: Option<f32> = None;
    for (band_index, band) in bands.iter().enumerate() {
        let band_path = format!("{profile_path}.formants[{band_index}]");
        validate_range(
            diagnostics,
            format!("{band_path}.frequency_hz"),
            band.frequency_hz,
            100.0..=12_000.0,
            "formant frequency must be finite and between 100 and 12000 Hz",
        );
        validate_range(
            diagnostics,
            format!("{band_path}.bandwidth_hz"),
            band.bandwidth_hz,
            20.0..=5_000.0,
            "formant bandwidth must be finite and between 20 and 5000 Hz",
        );
        validate_range(
            diagnostics,
            format!("{band_path}.gain_db"),
            band.gain_db,
            -60.0..=12.0,
            "formant gain must be finite and between -60 and 12 dB",
        );
        if let Some(previous) = previous_frequency {
            if previous.is_finite()
                && band.frequency_hz.is_finite()
                && band.frequency_hz <= previous
            {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::DefinitionError,
                        "formant frequencies must be strictly ascending",
                    )
                    .with_path(format!("{band_path}.frequency_hz")),
                );
            }
        }
        previous_frequency = Some(band.frequency_hz);
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

fn validate_finite(diagnostics: &mut Vec<Diagnostic>, path: String, value: f32, message: &str) {
    if !value.is_finite() {
        diagnostics
            .push(Diagnostic::error(DiagnosticCode::RouteDepthInvalid, message).with_path(path));
    }
}

fn range_message(field: &str, min: f32, max: f32) -> String {
    format!("{field} must be finite and between {min} and {max}")
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
            performance: PerformanceDefinition::Polyphonic {
                polyphony: 4,
                voice_stealing: VoiceStealingDefinition::QuietestReleasingThenOldest,
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
                mode: FilterModeDefinition::LowPass,
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
    fn performance_modulation_and_vector_schema_round_trip() {
        let mut source = definition();
        let mut bright = source.layers[0].clone();
        bright.id = "bright".to_owned();
        source.layers.push(bright);
        source.performance = PerformanceDefinition::Monophonic {
            legato: true,
            portamento: Some(PortamentoDefinition { time_seconds: 0.1 }),
        };
        source.macros.push(MacroDefinition {
            id: "motion".to_owned(),
            name: "Motion".to_owned(),
            default: 0.25,
        });
        source.vectors.push(VectorDefinition::TwoWay {
            id: "tone".to_owned(),
            name: "Tone".to_owned(),
            layer_a: "body".to_owned(),
            layer_b: "bright".to_owned(),
            position: 0.5,
        });
        source.modulation = Some(ModulationDefinition {
            sources: vec![
                ModulationSourceDefinition::Lfo(LfoDefinition {
                    id: "tempo_lfo".to_owned(),
                    waveform: LfoWaveform::Triangle,
                    rate: ModulationRateDefinition {
                        value: 1.0,
                        unit: ModulationRateUnit::PerBeat,
                    },
                    phase: 0.25,
                }),
                ModulationSourceDefinition::Mseg(MsegDefinition {
                    id: "motion_env".to_owned(),
                    initial_value: -1.0,
                    segments: vec![
                        MsegSegmentDefinition {
                            duration: ModulationDurationDefinition {
                                value: 0.25,
                                unit: ModulationDurationUnit::Beats,
                            },
                            target: 1.0,
                            curve: ModulationSegmentCurve::SmoothStep,
                        },
                        MsegSegmentDefinition {
                            duration: ModulationDurationDefinition {
                                value: 0.1,
                                unit: ModulationDurationUnit::Seconds,
                            },
                            target: 0.0,
                            curve: ModulationSegmentCurve::Linear,
                        },
                    ],
                    loop_range: Some(MsegLoopDefinition {
                        start_segment: 0,
                        end_segment: 2,
                    }),
                }),
                ModulationSourceDefinition::Step(StepModulatorDefinition {
                    id: "steps".to_owned(),
                    values: vec![-1.0, 0.0, 1.0],
                    rate: ModulationRateDefinition {
                        value: 2.0,
                        unit: ModulationRateUnit::PerSecond,
                    },
                }),
                ModulationSourceDefinition::SampleHold(SampleHoldDefinition {
                    id: "sample_hold".to_owned(),
                    seed: 7,
                    rate: ModulationRateDefinition {
                        value: 2.0,
                        unit: ModulationRateUnit::PerSecond,
                    },
                }),
                ModulationSourceDefinition::SmoothRandom(SmoothRandomDefinition {
                    id: "smooth_random".to_owned(),
                    seed: 11,
                    rate: ModulationRateDefinition {
                        value: 1.0,
                        unit: ModulationRateUnit::PerBeat,
                    },
                }),
            ],
            routes: vec![ModulationRouteDefinition {
                source: "macro.motion".to_owned(),
                target: "vector.tone.position".to_owned(),
                depth: ModulationDepthDefinition {
                    value: 1.0,
                    unit: ModulationUnit::Normalized,
                },
                curve: ModulationCurve::Linear,
            }],
        });

        assert!(source.validate().is_empty());
        let json = serde_json::to_string(&source).expect("definition serializes");
        let restored: InstrumentDefinition =
            serde_json::from_str(&json).expect("definition parses");
        assert_eq!(source, restored);
    }

    #[test]
    fn performance_modulation_and_vector_ranges_report_field_paths() {
        let mut source = definition();
        source.performance = PerformanceDefinition::Monophonic {
            legato: false,
            portamento: Some(PortamentoDefinition { time_seconds: 0.0 }),
        };
        source.macros = vec![
            MacroDefinition {
                id: "motion".to_owned(),
                name: "Motion".to_owned(),
                default: 1.5,
            },
            MacroDefinition {
                id: "motion".to_owned(),
                name: "Duplicate".to_owned(),
                default: 0.0,
            },
        ];
        source.vectors.push(VectorDefinition::TwoWay {
            id: "tone".to_owned(),
            name: "Tone".to_owned(),
            layer_a: "missing".to_owned(),
            layer_b: "body".to_owned(),
            position: 0.5,
        });
        source.modulation = Some(ModulationDefinition {
            sources: vec![ModulationSourceDefinition::Step(StepModulatorDefinition {
                id: "steps".to_owned(),
                values: Vec::new(),
                rate: ModulationRateDefinition {
                    value: 0.0,
                    unit: ModulationRateUnit::PerSecond,
                },
            })],
            routes: Vec::new(),
        });

        let diagnostics = source.validate();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("performance.portamento.time_seconds")
        }));
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.path.as_deref() == Some("macros[0].default"))
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("macros[1].id")
                && diagnostic.code == DiagnosticCode::IdDuplicated
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("vectors[0].layer_0")
                && diagnostic.code == DiagnosticCode::ParameterNotFound
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("modulation.sources[0].values")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("modulation.sources[0].rate.value")
        }));
    }

    #[test]
    fn modulation_source_resource_limits_are_validated() {
        let mut source = definition();
        source.modulation = Some(ModulationDefinition {
            sources: (0..65)
                .map(|index| {
                    ModulationSourceDefinition::Lfo(LfoDefinition {
                        id: format!("lfo_{index}"),
                        waveform: LfoWaveform::Sine,
                        rate: ModulationRateDefinition {
                            value: 1.0,
                            unit: ModulationRateUnit::PerSecond,
                        },
                        phase: 0.0,
                    })
                })
                .collect(),
            routes: Vec::new(),
        });

        let diagnostics = source.validate();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("modulation.sources")
                && diagnostic.code == DiagnosticCode::ValueOutOfRange
        }));
    }

    #[test]
    fn routes_require_depth_and_reject_legacy_amount_fields() {
        let mut legacy = serde_json::to_value(definition()).expect("definition serializes");
        legacy["modulation"] = serde_json::json!({
            "sources": [],
            "routes": [{
                "source": "velocity",
                "target": "layer.body.gain",
                "amount": 0.5,
                "curve": "linear"
            }]
        });
        assert!(serde_json::from_value::<InstrumentDefinition>(legacy).is_err());

        let mut unknown_depth = serde_json::to_value(definition()).expect("definition serializes");
        unknown_depth["modulation"] = serde_json::json!({
            "sources": [],
            "routes": [{
                "source": "velocity",
                "target": "layer.body.gain",
                "depth": {"value": 1.0, "unit": "decibels", "extra": 1},
                "curve": "linear"
            }]
        });
        assert!(serde_json::from_value::<InstrumentDefinition>(unknown_depth).is_err());
    }

    #[test]
    fn sample_playback_and_trigger_event_serde_preserve_the_new_shape() {
        let playback: SampleZonePlaybackDefinition = serde_json::from_value(serde_json::json!({
            "region": {"start_seconds": 0.25, "end_seconds": null},
            "direction": "reverse",
            "loop": {
                "start_seconds": 0.5,
                "end_seconds": 1.5,
                "crossfade_seconds": 0.1
            },
            "time": {"mode": "resample"}
        }))
        .expect("sample playback parses");
        assert!((playback.region.start_seconds - 0.25).abs() < f32::EPSILON);
        assert_eq!(playback.region.end_seconds, None);
        assert_eq!(playback.direction, SamplePlaybackDirection::Reverse);
        assert_eq!(
            playback.r#loop,
            Some(SampleLoopDefinition {
                start_seconds: 0.5,
                end_seconds: 1.5,
                crossfade_seconds: 0.1,
            })
        );
        assert_eq!(playback.time, SampleTimeDefinition::Resample);

        let trigger: LayerTriggerDefinition = serde_json::from_value(serde_json::json!({
            "event": "note_off",
            "key_min": 0,
            "key_max": 127,
            "velocity_min": 1,
            "velocity_max": 127
        }))
        .expect("trigger event parses");
        assert_eq!(trigger.event, LayerTriggerEvent::NoteOff);
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
            phase_distortion: None,
            wavefold: None,
            feedback: None,
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
    fn wavetable_definition_ranges_are_validated() {
        let mut value = definition();
        value.layers[0].generator = GeneratorDefinition::Wavetable(WavetableDefinition {
            asset: AssetReference {
                path: "table.wav".to_owned(),
                sha256: None,
            },
            frame_length: 64,
            position: 0.0,
            phase_reset: true,
            phase: 0.0,
            unison: None,
        });
        for frame_length in [64, 2_048, 4_096] {
            if let GeneratorDefinition::Wavetable(wavetable) = &mut value.layers[0].generator {
                wavetable.frame_length = frame_length;
            }
            assert!(
                value.validate().is_empty(),
                "frame_length {frame_length} is valid"
            );
        }

        if let GeneratorDefinition::Wavetable(wavetable) = &mut value.layers[0].generator {
            wavetable.frame_length = 65;
            wavetable.position = -f32::EPSILON;
            wavetable.phase = 1.0 + f32::EPSILON;
            wavetable.unison = Some(UnisonDefinition {
                voices: 9,
                detune_cents: 0.0,
                stereo_spread: 0.0,
                phase_spread: 0.0,
            });
        }
        let diagnostics = value.validate();
        for path in [
            "layers[0].generator.wavetable.frame_length",
            "layers[0].generator.wavetable.position",
            "layers[0].generator.wavetable.phase",
            "layers[0].generator.wavetable.unison.voices",
        ] {
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.path.as_deref() == Some(path)),
                "missing diagnostic for {path}: {diagnostics:?}"
            );
        }
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
        let one_shot = SampleZonePlaybackDefinition {
            region: SampleRegionDefinition {
                start_seconds: 0.0,
                end_seconds: None,
            },
            direction: SamplePlaybackDirection::Forward,
            r#loop: None,
            time: SampleTimeDefinition::Resample,
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
    fn sample_time_modes_validate_ratio_tempo_and_direction_constraints() {
        let fixed_playback = SampleZonePlaybackDefinition {
            region: SampleRegionDefinition {
                start_seconds: 0.0,
                end_seconds: None,
            },
            direction: SamplePlaybackDirection::Forward,
            r#loop: None,
            time: SampleTimeDefinition::FixedStretch { ratio: 2.5 },
        };
        let diagnostics = sample_definition(vec![sample_zone(
            "fixed",
            0,
            127,
            1,
            127,
            None,
            fixed_playback,
        )])
        .validate();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::InvalidStretchRatio
                && diagnostic.path.as_deref()
                    == Some("layers[0].generator.sample.zones[0].playback.time.ratio")
        }));

        let tempo_playback = SampleZonePlaybackDefinition {
            time: SampleTimeDefinition::TempoSync { source_bpm: 0.0 },
            ..fixed_playback
        };
        let diagnostics = sample_definition(vec![sample_zone(
            "tempo",
            0,
            127,
            1,
            127,
            None,
            tempo_playback,
        )])
        .validate();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::InvalidSourceTempo
                && diagnostic.path.as_deref()
                    == Some("layers[0].generator.sample.zones[0].playback.time.source_bpm")
        }));

        let reverse_playback = SampleZonePlaybackDefinition {
            direction: SamplePlaybackDirection::Reverse,
            time: SampleTimeDefinition::FixedStretch { ratio: 1.0 },
            ..fixed_playback
        };
        let diagnostics = sample_definition(vec![sample_zone(
            "reverse",
            0,
            127,
            1,
            127,
            None,
            reverse_playback,
        )])
        .validate();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::UnsupportedPlaybackCombination
                && diagnostic.path.as_deref()
                    == Some("layers[0].generator.sample.zones[0].playback")
        }));
    }

    #[test]
    fn sample_zone_midi_fields_have_explicit_bounds() {
        let one_shot = SampleZonePlaybackDefinition {
            region: SampleRegionDefinition {
                start_seconds: 0.0,
                end_seconds: None,
            },
            direction: SamplePlaybackDirection::Forward,
            r#loop: None,
            time: SampleTimeDefinition::Resample,
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
            SampleZonePlaybackDefinition {
                region: SampleRegionDefinition {
                    start_seconds: 1.0,
                    end_seconds: Some(2.0),
                },
                direction: SamplePlaybackDirection::Forward,
                r#loop: Some(SampleLoopDefinition {
                    start_seconds: 0.5,
                    end_seconds: 2.5,
                    crossfade_seconds: 0.0,
                }),
                time: SampleTimeDefinition::Resample,
            },
        )]);
        let diagnostics = value.validate();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::DefinitionError
                && diagnostic.path.as_deref()
                    == Some("layers[0].generator.sample.zones[0].playback.loop.start_seconds")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::DefinitionError
                && diagnostic.path.as_deref()
                    == Some("layers[0].generator.sample.zones[0].playback.loop.end_seconds")
        }));
    }

    #[test]
    fn sample_loop_rejects_a_crossfade_longer_than_half_the_loop() {
        let value = sample_definition(vec![sample_zone(
            "crossfade",
            0,
            127,
            1,
            127,
            None,
            SampleZonePlaybackDefinition {
                region: SampleRegionDefinition {
                    start_seconds: 0.0,
                    end_seconds: Some(2.0),
                },
                direction: SamplePlaybackDirection::Forward,
                r#loop: Some(SampleLoopDefinition {
                    start_seconds: 0.5,
                    end_seconds: 1.5,
                    crossfade_seconds: 0.51,
                }),
                time: SampleTimeDefinition::Resample,
            },
        )]);
        let diagnostics = value.validate();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::DefinitionError
                && diagnostic.path.as_deref()
                    == Some("layers[0].generator.sample.zones[0].playback.loop.crossfade_seconds")
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
                    depth: ModulationDepthDefinition {
                        value: 0.0,
                        unit: crate::parameter::ModulationUnit::Decibels,
                    },
                    curve: ModulationCurve::Linear,
                },
                ModulationRouteDefinition {
                    source: "velocity".to_owned(),
                    target: " layer.body.gain".to_owned(),
                    depth: ModulationDepthDefinition {
                        value: 0.0,
                        unit: crate::parameter::ModulationUnit::Decibels,
                    },
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

    fn operator_definition(
        mode: OperatorModulationMode,
        algorithm: OperatorAlgorithm,
    ) -> OperatorModulationDefinition {
        let envelope = AdsrDefinition {
            attack_seconds: 0.001,
            decay_seconds: 0.2,
            sustain_level: 0.4,
            release_seconds: 0.1,
        };
        OperatorModulationDefinition {
            mode,
            algorithm,
            operators: vec![
                OperatorDefinition {
                    ratio: 1.0,
                    detune_cents: 0.0,
                    level: 0.9,
                    modulation_amount: 0.0,
                    feedback: 0.0,
                    phase: 0.0,
                    envelope,
                },
                OperatorDefinition {
                    ratio: 2.0,
                    detune_cents: 0.0,
                    level: 0.0,
                    modulation_amount: 2.0,
                    feedback: 0.0,
                    phase: 0.0,
                    envelope,
                },
                OperatorDefinition {
                    ratio: 3.0,
                    detune_cents: 0.0,
                    level: 0.0,
                    modulation_amount: 1.0,
                    feedback: 0.0,
                    phase: 0.0,
                    envelope,
                },
                OperatorDefinition {
                    ratio: 5.0,
                    detune_cents: 0.0,
                    level: 0.0,
                    modulation_amount: 0.5,
                    feedback: 0.2,
                    phase: 0.0,
                    envelope,
                },
            ],
            phase_reset: true,
            unison: None,
        }
    }

    #[test]
    fn operator_algorithms_compile_to_expected_topologies() {
        let expected = [
            (
                OperatorAlgorithm::Stack4,
                [3, 2, 1, 0],
                [0b0010, 0b0100, 0b1000, 0],
                0b0001,
            ),
            (
                OperatorAlgorithm::Stack3PlusCarrier,
                [3, 2, 1, 0],
                [0, 0b0100, 0b1000, 0],
                0b0011,
            ),
            (
                OperatorAlgorithm::TwoStacks,
                [1, 3, 0, 2],
                [0b0010, 0, 0b1000, 0],
                0b0101,
            ),
            (
                OperatorAlgorithm::ForkToCarrier,
                [3, 1, 2, 0],
                [0b0110, 0b1000, 0b1000, 0],
                0b0001,
            ),
            (
                OperatorAlgorithm::TwoModulatorsPlusCarrier,
                [2, 3, 0, 1],
                [0b1100, 0, 0, 0],
                0b0011,
            ),
            (
                OperatorAlgorithm::ThreeModulators,
                [1, 2, 3, 0],
                [0b1110, 0, 0, 0],
                0b0001,
            ),
            (
                OperatorAlgorithm::SharedModulator,
                [3, 0, 1, 2],
                [0b1000, 0b1000, 0b1000, 0],
                0b0111,
            ),
            (
                OperatorAlgorithm::Parallel,
                [0, 1, 2, 3],
                [0, 0, 0, 0],
                0b1111,
            ),
        ];
        for (algorithm, evaluation_order, incoming_masks, carrier_mask) in expected {
            let topology = algorithm.topology();
            assert_eq!(topology.evaluation_order, evaluation_order);
            assert_eq!(topology.incoming_masks, incoming_masks);
            assert_eq!(topology.carrier_mask, carrier_mask);
        }
    }

    #[test]
    fn operator_algorithms_use_stable_definition_names() {
        let names = [
            (OperatorAlgorithm::Stack4, "\"stack_4\""),
            (
                OperatorAlgorithm::Stack3PlusCarrier,
                "\"stack_3_plus_carrier\"",
            ),
            (OperatorAlgorithm::TwoStacks, "\"two_stacks\""),
            (OperatorAlgorithm::ForkToCarrier, "\"fork_to_carrier\""),
            (
                OperatorAlgorithm::TwoModulatorsPlusCarrier,
                "\"two_modulators_plus_carrier\"",
            ),
            (OperatorAlgorithm::ThreeModulators, "\"three_modulators\""),
            (OperatorAlgorithm::SharedModulator, "\"shared_modulator\""),
            (OperatorAlgorithm::Parallel, "\"parallel\""),
        ];
        for (algorithm, expected) in names {
            assert_eq!(
                serde_json::to_string(&algorithm).expect("algorithm serializes"),
                expected
            );
        }
    }

    #[test]
    fn operator_definition_validates_four_operator_contract() {
        let mut value = definition();
        value.layers[0].generator = GeneratorDefinition::OperatorModulation(operator_definition(
            OperatorModulationMode::Phase,
            OperatorAlgorithm::Stack4,
        ));
        assert!(value.validate().is_empty());

        {
            let GeneratorDefinition::OperatorModulation(operator_modulation) =
                &mut value.layers[0].generator
            else {
                panic!("operator fixture must use the operator generator");
            };
            operator_modulation.operators[0].ratio = 0.1;
            operator_modulation.operators[1].feedback = 0.1;
            operator_modulation.operators[1].level = 0.1;
            operator_modulation.operators[0].modulation_amount = 0.6;
        }
        let diagnostics = value.validate();
        for path in [
            "layers[0].generator.operator_modulation.operators[0].ratio",
            "layers[0].generator.operator_modulation.operators[1].level",
            "layers[0].generator.operator_modulation.operators[0].modulation_amount",
        ] {
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.path.as_deref() == Some(path)),
                "missing diagnostic for {path}: {diagnostics:?}"
            );
        }

        if let GeneratorDefinition::OperatorModulation(operator_modulation) =
            &mut value.layers[0].generator
        {
            operator_modulation.operators.truncate(3);
        }
        let diagnostics = value.validate();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("layers[0].generator.operator_modulation.operators")
        }));
    }

    #[test]
    fn amplitude_and_ring_reject_feedback() {
        for mode in [
            OperatorModulationMode::Amplitude,
            OperatorModulationMode::Ring,
        ] {
            let mut value = definition();
            let mut operator = operator_definition(mode, OperatorAlgorithm::Stack4);
            operator.operators[3].feedback = 0.1;
            value.layers[0].generator = GeneratorDefinition::OperatorModulation(operator);
            let diagnostics = value.validate();
            assert!(diagnostics.iter().any(|diagnostic| {
                diagnostic.path.as_deref()
                    == Some("layers[0].generator.operator_modulation.operators[3].feedback")
            }));
        }
    }
}
