use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::spectral::{
    PreparedSpectralAsset, SpectralPreparationError, SpectralSynthesisPlan, prepare_spectral_asset,
    spectral_hop_size,
};
use super::wavetable::{
    WavetablePreparation, WavetablePreparationError, WavetableWarning, prepare_wavetable_asset,
};
use super::{
    AssetCacheKey, BASIC_FREQUENCY_LIMIT_RATIO, PHYSICAL_FREQUENCY_LIMIT_RATIO, asset_diagnostic,
    brown_noise_coefficient, compile_adsr, db_to_linear, effective_max_frequency,
    prepare_cached_asset, source_id_hash,
};
use crate::asset::{
    AssetError, PreparedAsset, PreparedAudio, PreparedAudioChannels, resolved_asset_path,
};
use crate::definition::{
    AdditiveDefinition, AssetReference, FormantDefinition, GeneratorDefinition, GranularDefinition,
    ModalDefinition, NoiseColor, OperatorAlgorithm, OperatorModulationDefinition,
    OperatorModulationMode, OscillatorDefinition, OscillatorWaveform, PhysicalExciterDefinition,
    PhysicalStringDefinition, SamplePlaybackDirection, SampleTimeDefinition, SampleZoneDefinition,
    SpectralDefinition, UnisonDefinition, WaveSequenceDefinition, WaveSequenceDirection,
    WaveSequenceDurationDefinition, WaveSequenceStepPlayback, WavetableDefinition,
};
use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticSeverity};
use crate::parameter::generator::{
    ADDITIVE_INHARMONICITY, ADDITIVE_MORPH, ADDITIVE_SPECTRUM_TILT, FORMANT_SHIFT,
    FORMANT_SPECTRAL_TILT, FORMANT_THROAT, FORMANT_VOWEL_POSITION, GRAIN_DENSITY, GRAIN_PAN_SPREAD,
    GRAIN_PITCH, GRAIN_RANDOMNESS, GRAIN_SIZE, GRANULAR_POSITION, GeneratorParameterSpec,
    MODAL_BRIGHTNESS, MODAL_DECAY, MODAL_STRUCTURE, NOISE_CORRELATION, OSCILLATOR_FEEDBACK,
    PHASE_DISTORTION, PHYSICAL_STRING_BRIGHTNESS, PHYSICAL_STRING_DECAY_SECONDS,
    PHYSICAL_STRING_STIFFNESS, PULSE_WIDTH, SPECTRAL_BLUR, SPECTRAL_FREEZE, SPECTRAL_MORPH,
    SPECTRAL_POSITION, SPECTRAL_SHIFT, SYNC_RATIO, UNISON_DETUNE, UNISON_SPREAD, WAVEFOLD,
    WAVESHAPE, WAVETABLE_POSITION,
};
use crate::parameter::{ParameterCatalog, ParameterHandle, layer_generator_parameter_id};

pub(crate) const GRANULAR_GRAIN_POOL_LIMIT: usize = 64;
pub(crate) const PHASE_DOMAIN_FREQUENCY_LIMIT_RATIO: f64 = 0.24;

fn build_sine_table() -> Arc<[f32]> {
    const SINE_TABLE_LENGTH: usize = 4096;
    let mut table = Vec::with_capacity(SINE_TABLE_LENGTH + 1);
    #[allow(clippy::cast_precision_loss)]
    for index in 0..=SINE_TABLE_LENGTH {
        let phase = index as f32 / SINE_TABLE_LENGTH as f32;
        table.push((std::f32::consts::TAU * phase).sin());
    }
    Arc::from(table.into_boxed_slice())
}

/// Compiled generator variants.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum CompiledGenerator {
    /// Oscillator generator.
    Oscillator(CompiledOscillator),
    /// Noise generator.
    Noise(CompiledNoise),
    /// Fractional-delay feedback string generator.
    PhysicalString(CompiledPhysicalString),
    /// `DaisySP` modal resonator generator.
    Modal(CompiledModal),
    /// Directly specified sine partial generator.
    Additive(CompiledAdditive),
    /// Harmonic generator shaped by interpolated formant profiles.
    Formant(CompiledFormant),
    /// Prepared sample generator.
    Sample(CompiledSample),
    /// Prepared granular generator.
    Granular(CompiledGranular),
    /// Prepared Wave Sequence generator.
    WaveSequence(CompiledWaveSequence),
    /// Prepared band-limited wavetable generator.
    Wavetable(CompiledWavetable),
    /// Prepared spectral resynthesis generator.
    Spectral(CompiledSpectral),
    /// Fixed-topology four-operator modulation generator.
    OperatorModulation(CompiledOperatorModulation),
}

/// Fixed channel layout produced by a compiled generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratorOutputMode {
    /// A single source channel is passed through the mono layer path.
    Mono,
    /// Independent left and right source channels are passed through the stereo layer path.
    Stereo,
}

impl CompiledGenerator {
    /// Return the fixed channel layout for this generator.
    #[must_use]
    pub fn output_mode(&self) -> GeneratorOutputMode {
        match self {
            Self::Oscillator(value) => value.unison.output_mode(),
            Self::Noise(_) | Self::Granular(_) => GeneratorOutputMode::Stereo,
            Self::Additive(_) | Self::Formant(_) | Self::PhysicalString(_) | Self::Modal(_) => {
                GeneratorOutputMode::Mono
            }
            Self::WaveSequence(value) => value.output_mode(),
            Self::Sample(value) => value.output_mode(),
            Self::Wavetable(value) => value.unison.output_mode(),
            Self::Spectral(value) => value.output_mode(),
            Self::OperatorModulation(value) => value.unison.output_mode(),
        }
    }

    /// Return the maximum latency prepared for this generator.
    #[must_use]
    pub fn max_intrinsic_latency_frames(&self) -> usize {
        match self {
            Self::Sample(value) => value
                .stretch_latency
                .map_or(0, |latency| latency.output_frames),
            Self::Spectral(value) => value.latency_frames,
            Self::Oscillator(_)
            | Self::Noise(_)
            | Self::Additive(_)
            | Self::Formant(_)
            | Self::Granular(_)
            | Self::WaveSequence(_)
            | Self::Wavetable(_)
            | Self::PhysicalString(_)
            | Self::Modal(_)
            | Self::OperatorModulation(_) => 0,
        }
    }

    pub(crate) fn is_available(&self) -> bool {
        match self {
            Self::Oscillator(_)
            | Self::Noise(_)
            | Self::Additive(_)
            | Self::Formant(_)
            | Self::PhysicalString(_)
            | Self::Modal(_)
            | Self::OperatorModulation(_) => true,
            Self::Granular(value) => value.source.is_some(),
            Self::WaveSequence(value) => value.steps.iter().any(|step| step.source.is_some()),
            Self::Sample(value) => value.zones.iter().any(CompiledSampleZone::is_enabled),
            Self::Wavetable(value) => value.prepared.is_some(),
            Self::Spectral(value) => {
                value.source.is_some() && (value.asset_b_path.is_none() || value.source_b.is_some())
            }
        }
    }
}

/// Generator parameter handles owned by an oscillator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledOscillatorParameters {
    /// Pulse width handle for a pulse waveform.
    pub pulse_width: Option<ParameterHandle>,
    /// Waveshaping amount handle.
    pub waveshape: Option<ParameterHandle>,
    /// Phase-distortion amount handle.
    pub phase_distortion: Option<ParameterHandle>,
    /// Wavefolder amount handle.
    pub wavefold: Option<ParameterHandle>,
    /// Oscillator feedback amount handle.
    pub oscillator_feedback: Option<ParameterHandle>,
    /// Unison detune handle.
    pub unison_detune: Option<ParameterHandle>,
    /// Unison stereo spread handle.
    pub unison_spread: Option<ParameterHandle>,
}

/// Backend selected for a compiled oscillator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledOscillatorBackend {
    /// `DaisySP` basic oscillator backend.
    Basic,
    /// `DaisySP` variable-shape hard-sync backend.
    VariableShapeSync {
        /// Sync ratio handle for the hard-sync oscillator.
        sync_ratio: ParameterHandle,
    },
    /// Rust phase-domain sine backend for distortion and feedback.
    PhaseDomain,
}

impl CompiledOscillatorBackend {
    /// Return the maximum frequency accepted by the selected oscillator backend.
    #[must_use]
    pub fn effective_max_frequency(self, sample_rate: f64) -> f32 {
        let ratio = match self {
            Self::Basic => BASIC_FREQUENCY_LIMIT_RATIO,
            Self::VariableShapeSync { .. } | Self::PhaseDomain => {
                PHASE_DOMAIN_FREQUENCY_LIMIT_RATIO
            }
        };
        effective_max_frequency(sample_rate, ratio)
    }
}

/// Static unison distribution prepared by the compiler.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledUnison {
    /// Symmetric position coefficients used for detune and pan.
    pub position_distribution: Box<[f32]>,
    /// Static phase offsets.
    pub phase_distribution: Box<[f32]>,
    /// Static phase spread used to build the offsets.
    pub phase_spread: f32,
    /// Normalization applied to the component sum.
    pub normalization: f32,
}

impl CompiledUnison {
    fn output_mode(&self) -> GeneratorOutputMode {
        if self.position_distribution.len() == 1 {
            GeneratorOutputMode::Mono
        } else {
            GeneratorOutputMode::Stereo
        }
    }
}

/// Compiled oscillator settings.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledOscillator {
    /// Waveform selected by the Definition.
    pub waveform: OscillatorWaveform,
    /// Whether Note On resets the phase.
    pub phase_reset: bool,
    /// Initial phase used by instrument and note resets.
    pub phase: f32,
    /// Native backend selected by static Definition fields.
    pub backend: CompiledOscillatorBackend,
    /// Generator parameter bindings.
    pub parameters: CompiledOscillatorParameters,
    /// Whether a DC blocker is required after the nonlinear stages.
    pub dc_blocker: bool,
    /// Static Unison component configuration.
    pub unison: Arc<CompiledUnison>,
}

/// Compiled noise settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledNoise {
    /// Spectral color selected by the Definition.
    pub color: NoiseColor,
    /// Deterministic Definition seed.
    pub seed: u64,
    /// Stereo correlation parameter handle.
    pub correlation: ParameterHandle,
    /// Stable hash of the owning layer identifier.
    pub layer_hash: u64,
    /// Sample-rate-specific Brown noise coefficient.
    pub brown_coefficient: f32,
}

/// Compiled deterministic excitation shared by physical generators.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompiledPhysicalExciter {
    /// A single-sample impulse.
    Impulse,
    /// A filtered deterministic noise burst.
    NoiseBurst {
        /// Burst duration in seconds.
        duration_seconds: f32,
        /// Low-pass brightness.
        brightness: f32,
        /// Explicit deterministic seed.
        seed: u64,
    },
}

/// Dynamic parameters owned by a physical string generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledPhysicalStringParameters {
    /// Nominal decay time handle.
    pub decay_seconds: ParameterHandle,
    /// Loop brightness handle.
    pub brightness: ParameterHandle,
    /// Dispersion stiffness handle.
    pub stiffness: ParameterHandle,
}

/// Compiled fractional-delay feedback string generator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledPhysicalString {
    /// Deterministic note-start excitation.
    pub exciter: CompiledPhysicalExciter,
    /// Dynamic parameter bindings.
    pub parameters: CompiledPhysicalStringParameters,
    /// Stable hash of the owning layer identifier.
    pub layer_hash: u64,
    /// Process-rate-derived safe frequency limit.
    pub effective_max_frequency: f32,
}

/// Dynamic parameters owned by a modal generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledModalParameters {
    /// Structure handle.
    pub structure: ParameterHandle,
    /// Brightness handle.
    pub brightness: ParameterHandle,
    /// Decay handle.
    pub decay: ParameterHandle,
}

/// Compiled `DaisySP` modal resonator generator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledModal {
    /// Deterministic note-start excitation.
    pub exciter: CompiledPhysicalExciter,
    /// Fixed number of modes.
    pub mode_count: u8,
    /// Dynamic parameter bindings.
    pub parameters: CompiledModalParameters,
    /// Stable hash of the owning layer identifier.
    pub layer_hash: u64,
    /// Process-rate-derived safe frequency limit.
    pub effective_max_frequency: f32,
}

/// Parameter handles owned by an Additive Generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledAdditiveParameters {
    /// Spectrum A/B morph handle.
    pub morph: ParameterHandle,
    /// Spectrum tilt handle.
    pub spectrum_tilt: ParameterHandle,
    /// Global inharmonicity handle.
    pub inharmonicity: ParameterHandle,
}

/// Compiled static and envelope settings for one Additive partial.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledAdditivePartial {
    /// Stable Definition identifier.
    pub id: String,
    /// Frequency ratio relative to the played note.
    pub ratio: f32,
    /// Spectrum A amplitude.
    pub amplitude_a: f32,
    /// Spectrum B amplitude.
    pub amplitude_b: f32,
    /// Initial phase in cycles.
    pub phase: f32,
    /// Optional sample-rate-specific partial envelope.
    pub envelope: Option<CompiledAdsr>,
}

/// Compiled Additive Generator.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledAdditive {
    /// Partials in Definition order.
    pub partials: Box<[CompiledAdditivePartial]>,
    /// Whether Note On restores all partial phases.
    pub phase_reset: bool,
    /// Dynamic parameter bindings.
    pub parameters: CompiledAdditiveParameters,
    /// Shared lookup table used by all voices of this generator.
    pub sine_table: Arc<[f32]>,
}

/// Parameter handles owned by a Formant Generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledFormantParameters {
    /// Vowel profile position handle.
    pub vowel_position: ParameterHandle,
    /// Formant center and bandwidth shift handle.
    pub formant_shift: ParameterHandle,
    /// Formant bandwidth multiplier handle.
    pub throat: ParameterHandle,
    /// Spectral amplitude slope handle.
    pub spectral_tilt: ParameterHandle,
}

/// Compiled static settings for one formant band.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledFormantBand {
    /// Center frequency in hertz.
    pub frequency_hz: f32,
    /// Full width at half maximum in hertz.
    pub bandwidth_hz: f32,
    /// Relative gain in decibels.
    pub gain_db: f32,
}

/// Compiled formant profile with five corresponding bands.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledFormantProfile {
    /// Stable Definition identifier.
    pub id: String,
    /// Five bands in ascending frequency order.
    pub formants: [CompiledFormantBand; 5],
}

/// Compiled Formant Generator.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledFormant {
    /// Number of harmonic partials generated by the runtime.
    pub partial_count: usize,
    /// Whether Note On restores all partial phases.
    pub phase_reset: bool,
    /// Profiles in Definition order.
    pub profiles: Box<[CompiledFormantProfile]>,
    /// Dynamic parameter bindings.
    pub parameters: CompiledFormantParameters,
    /// Shared lookup table used by all voices of this generator.
    pub sine_table: Arc<[f32]>,
}

/// Compiled sample configuration and prepared zones.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSample {
    /// Zones in Definition order.
    pub zones: Box<[CompiledSampleZone]>,
    /// Round Robin groups in stable Definition order.
    pub groups: Box<[CompiledRoundRobinGroup]>,
    /// Latency measured from the prepared stretch backend.
    pub stretch_latency: Option<CompiledStretchLatency>,
}

impl CompiledSample {
    /// Return the fixed output layout required by the prepared zones.
    #[must_use]
    pub fn output_mode(&self) -> GeneratorOutputMode {
        if self
            .zones
            .iter()
            .filter_map(|zone| zone.source.as_deref())
            .any(|source| matches!(source.channels, PreparedAudioChannels::Stereo { .. }))
        {
            GeneratorOutputMode::Stereo
        } else {
            GeneratorOutputMode::Mono
        }
    }
}

/// Dynamic parameter handles owned by a compiled Granular Generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledGranularParameters {
    /// Grain source position handle.
    pub position: ParameterHandle,
    /// Grain duration in seconds handle.
    pub grain_size: ParameterHandle,
    /// Grain density per second handle.
    pub density: ParameterHandle,
    /// Grain pitch offset in cents handle.
    pub pitch: ParameterHandle,
    /// Source position randomization handle.
    pub randomness: ParameterHandle,
    /// Per-grain stereo spread handle.
    pub pan_spread: ParameterHandle,
}

/// Compiled granular generator and its shared prepared source.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledGranular {
    /// Prepared source shared by all voices.
    pub source: Option<Arc<PreparedAudio>>,
    /// Asset path as written in the Definition.
    pub asset_path: String,
    /// Whether the Definition supplied an asset hash.
    pub asset_sha256_specified: bool,
    /// MIDI note represented by the source recording.
    pub root_note: u8,
    /// Inclusive region start in prepared frames.
    pub start_frame: usize,
    /// Exclusive region end in prepared frames.
    pub end_frame: usize,
    /// Dynamic parameter bindings.
    pub parameters: CompiledGranularParameters,
    /// Explicit deterministic grain seed.
    pub seed: u64,
    /// Stable hash of the owning layer identifier.
    pub layer_hash: u64,
    /// Maximum active grains per voice.
    pub grain_pool_limit: usize,
}

/// Duration unit resolved for one Wave Sequence step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompiledWaveSequenceDuration {
    /// Tempo-independent duration in seconds.
    Seconds(f64),
    /// Tempo-dependent duration in quarter-note beats.
    Beats(f64),
}

/// Asset playback mode resolved for one Wave Sequence step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledWaveSequenceStepPlayback {
    /// Read the region once and then output silence.
    OneShot,
    /// Repeat the region until the step ends.
    Loop,
}

/// One prepared Wave Sequence step.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledWaveSequenceStep {
    /// Stable Definition identifier.
    pub id: String,
    /// Prepared source, absent when the asset or region is unavailable.
    pub source: Option<Arc<PreparedAudio>>,
    /// Asset path as written in the Definition.
    pub asset_path: String,
    /// Inclusive region start in prepared frames.
    pub start_frame: usize,
    /// Exclusive region end in prepared frames.
    pub end_frame: usize,
    /// Step duration and its unit.
    pub duration: CompiledWaveSequenceDuration,
    /// Playback mode inside the step.
    pub playback: CompiledWaveSequenceStepPlayback,
    /// Playback direction inside the step region.
    pub playback_direction: CompiledSampleDirection,
    /// Linear step gain.
    pub gain: f32,
    /// Step pitch offset in cents.
    pub pitch_cents: f32,
}

impl CompiledWaveSequenceStep {
    /// Return whether this step can read an audio source.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.source.is_some()
    }
}

/// Compiled Wave Sequence configuration and immutable step data.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledWaveSequence {
    /// MIDI note represented by the sequence assets.
    pub root_note: u8,
    /// Order in which steps are selected.
    pub direction: WaveSequenceDirection,
    /// Whether the sequence repeats after reaching an end.
    pub loop_sequence: bool,
    /// Constant-power overlap ratio between adjacent steps.
    pub crossfade: f32,
    /// Steps in Definition order, including unavailable silence steps.
    pub steps: Arc<[CompiledWaveSequenceStep]>,
}

impl CompiledWaveSequence {
    /// Return the fixed output layout required by available steps.
    #[must_use]
    pub fn output_mode(&self) -> GeneratorOutputMode {
        if self.steps.iter().any(|step| {
            step.source.as_deref().is_some_and(|source| {
                matches!(source.channels, PreparedAudioChannels::Stereo { .. })
            })
        }) {
            GeneratorOutputMode::Stereo
        } else {
            GeneratorOutputMode::Mono
        }
    }
}

/// Compiled Sample Zone and its prepared Asset.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSampleZone {
    /// Stable Definition identifier.
    pub id: String,
    /// Prepared mono or stereo source, absent when the asset could not be loaded.
    pub source: Option<Arc<PreparedAudio>>,
    /// MIDI note represented by the source recording.
    pub root_note: u8,
    /// Lowest accepted MIDI note.
    pub key_min: u8,
    /// Highest accepted MIDI note.
    pub key_max: u8,
    /// Lowest accepted MIDI velocity.
    pub velocity_min: u8,
    /// Highest accepted MIDI velocity.
    pub velocity_max: u8,
    /// Compiled Round Robin group handle.
    pub group: Option<usize>,
    /// Compiled playback region.
    pub playback: CompiledSamplePlayback,
    /// Path as written in the Definition.
    pub asset_path: String,
}

/// Source metadata retained by a prepared Wavetable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavetableSourceMetadata {
    /// Source sample rate, retained for inspection but not used as pitch data.
    pub source_sample_rate: u32,
    /// Source channel count before downmixing.
    pub source_channels: usize,
    /// Source bit depth when supplied by the decoder.
    pub bits_per_sample: Option<u32>,
    /// Number of frames in the source asset.
    pub source_frames: usize,
}

/// One band-limited frame with interpolation guard samples.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedWavetableFrame {
    /// Interpolation layout containing the previous and following samples.
    pub guarded_samples: Box<[f32]>,
}

/// One harmonic-limited Wavetable band.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedWavetableBand {
    /// Highest harmonic retained in this band.
    pub max_harmonic: usize,
    /// Frames in Definition asset order.
    pub frames: Box<[PreparedWavetableFrame]>,
}

/// Compile-time Wavetable data shared by all voices.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedWavetable {
    /// Samples in one periodic frame.
    pub frame_length: usize,
    /// Number of frames in the source asset.
    pub frame_count: usize,
    /// Harmonic-limited bands ordered from widest to narrowest.
    pub bands: Box<[PreparedWavetableBand]>,
    /// Source metadata retained for inspection.
    pub source_metadata: WavetableSourceMetadata,
}

/// Parameter handles owned by a Wavetable generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledWavetableParameters {
    /// Frame-position handle.
    pub position: ParameterHandle,
    /// Optional dynamic unison detune handle.
    pub unison_detune: Option<ParameterHandle>,
    /// Optional dynamic stereo spread handle.
    pub unison_spread: Option<ParameterHandle>,
}

/// Compiled Wavetable settings.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledWavetable {
    /// Prepared asset, absent when the asset is unavailable.
    pub prepared: Option<Arc<PreparedWavetable>>,
    /// Samples in one periodic frame as specified by the Definition.
    pub frame_length: usize,
    /// Whether Note On restores the phase.
    pub phase_reset: bool,
    /// Initial phase.
    pub phase: f32,
    /// Process-rate-derived frequency limit used for band selection.
    pub effective_max_frequency: f32,
    /// Generator parameter bindings.
    pub parameters: CompiledWavetableParameters,
    /// Static Unison component configuration.
    pub unison: Arc<CompiledUnison>,
    /// Asset path as written in the Definition.
    pub asset_path: String,
    /// Whether the Definition supplied an asset hash.
    pub asset_sha256_specified: bool,
}

/// Dynamic parameter handles owned by a Spectral Generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledSpectralParameters {
    /// Normalized source position handle.
    pub position: ParameterHandle,
    /// Source scan freeze handle.
    pub freeze: ParameterHandle,
    /// Temporal magnitude blur handle.
    pub blur: ParameterHandle,
    /// Frequency translation handle.
    pub shift: ParameterHandle,
    /// Optional A/B morph handle.
    pub morph: Option<ParameterHandle>,
}

/// Compiled spectral source and fixed resynthesis plan.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSpectral {
    /// Prepared primary source, absent when preparation failed.
    pub source: Option<Arc<PreparedSpectralAsset>>,
    /// Prepared optional morph source, absent when the Definition source is unavailable.
    pub source_b: Option<Arc<PreparedSpectralAsset>>,
    /// Primary asset path as written in the Definition.
    pub asset_a_path: String,
    /// Whether the primary Definition supplied an asset hash.
    pub asset_a_sha256_specified: bool,
    /// Optional second asset path retained for inspection.
    pub asset_b_path: Option<String>,
    /// Whether the second Definition supplied an asset hash.
    pub asset_b_sha256_specified: bool,
    /// MIDI note represented by the primary source.
    pub root_note: u8,
    /// FFT size used for the prepared source.
    pub fft_size: usize,
    /// Fixed hop size used by the prepared source.
    pub hop_size: usize,
    /// Whether Note On restores the prepared phase.
    pub phase_reset: bool,
    /// Dynamic parameter bindings.
    pub parameters: CompiledSpectralParameters,
    /// Shared inverse FFT plan and synthesis window.
    pub(crate) synthesis_plan: Arc<SpectralSynthesisPlan>,
    /// Reported algorithmic latency.
    pub latency_frames: usize,
}

impl CompiledSpectral {
    /// Return the prepared source channel layout, or mono while unavailable.
    #[must_use]
    pub fn output_mode(&self) -> GeneratorOutputMode {
        match self.source.as_ref().map(|source| source.channels) {
            Some(2) => GeneratorOutputMode::Stereo,
            _ => GeneratorOutputMode::Mono,
        }
    }
}

/// Compiled operator connection topology.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledOperatorTopology {
    /// Operator evaluation order from modulator to carrier.
    pub evaluation_order: [u8; 4],
    /// Bit mask of incoming operator connections for each destination.
    pub incoming_masks: [u8; 4],
    /// Bit mask of operators contributing to the final output.
    pub carrier_mask: u8,
    /// Normalization applied to the carrier sum.
    pub carrier_normalization: f32,
}

/// Parameter handles owned by one compiled operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledOperatorParameters {
    /// Frequency ratio handle.
    pub ratio: ParameterHandle,
    /// Detune handle.
    pub detune: ParameterHandle,
    /// Carrier level handle.
    pub level: Option<ParameterHandle>,
    /// Modulation amount handle.
    pub modulation_amount: Option<ParameterHandle>,
    /// Self-feedback handle.
    pub feedback: Option<ParameterHandle>,
}

/// Compiled static and envelope settings for one operator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledOperator {
    /// Sample-rate-specific operator envelope.
    pub envelope: CompiledAdsr,
    /// Initial phase.
    pub phase: f32,
}

/// Compiled four-operator modulation generator.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledOperatorModulation {
    /// Audio-rate interaction mode.
    pub mode: OperatorModulationMode,
    /// Definition algorithm retained for inspection.
    pub algorithm: OperatorAlgorithm,
    /// Resolved fixed topology.
    pub topology: CompiledOperatorTopology,
    /// Four compiled operators in user-facing order.
    pub operators: [CompiledOperator; 4],
    /// Whether Note On restores operator phases.
    pub phase_reset: bool,
    /// Dynamic parameter handles for each operator.
    pub parameters: [CompiledOperatorParameters; 4],
    /// Optional dynamic unison detune handle.
    pub unison_detune: Option<ParameterHandle>,
    /// Optional dynamic stereo spread handle.
    pub unison_spread: Option<ParameterHandle>,
    /// Static Unison component configuration.
    pub unison: Arc<CompiledUnison>,
    /// Safe operator frequency limit for this process sample rate.
    pub effective_max_frequency: f32,
}

impl CompiledSampleZone {
    /// Return whether the zone has a prepared source and can be selected.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.source.is_some()
    }
}

/// Playback direction resolved for the Sample Runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledSampleDirection {
    /// Advance the cursor toward the region end.
    Forward,
    /// Advance the cursor toward the region start.
    Reverse,
}

/// Loop region in prepared frame coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledSampleLoop {
    /// Inclusive loop start frame.
    pub start_frame: usize,
    /// Exclusive loop end frame.
    pub end_frame: usize,
    /// Constant-power crossfade length in frames.
    pub crossfade_frames: usize,
}

/// Sample playback configuration in prepared frame coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledSamplePlayback {
    /// Playback direction.
    pub direction: CompiledSampleDirection,
    /// Inclusive region start frame.
    pub start_frame: usize,
    /// Exclusive region end frame.
    pub end_frame: usize,
    /// Optional loop inside the region.
    pub loop_region: Option<CompiledSampleLoop>,
    /// Time behavior in prepared frame coordinates.
    pub time: CompiledSampleTime,
}

/// Time behavior resolved for Sample Runtime.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompiledSampleTime {
    /// Couple pitch and duration through ordinary resampling.
    Resample,
    /// Keep pitch independent from duration with a fixed output ratio.
    FixedStretch {
        /// Output duration divided by source duration.
        duration_ratio: f64,
    },
    /// Derive the output ratio from the process tempo.
    TempoSync {
        /// Source tempo in beats per minute.
        source_bpm: f64,
    },
}

impl CompiledSampleTime {
    /// Return whether this mode uses the native stretch backend.
    #[must_use]
    pub const fn uses_stretch(self) -> bool {
        !matches!(self, Self::Resample)
    }
}

/// Latency reported by one prepared stretch backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledStretchLatency {
    /// Input-side latency in frames.
    pub input_frames: usize,
    /// Output-side latency in frames.
    pub output_frames: usize,
}

/// Compiled Round Robin group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledRoundRobinGroup {
    /// Stable Definition identifier.
    pub id: String,
    /// Members with successfully prepared assets.
    pub enabled_member_zone_indices: Box<[usize]>,
}

/// Sample-rate-specific ADSR settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledAdsr {
    /// Attack duration in frames.
    pub attack_samples: usize,
    /// Decay duration in frames.
    pub decay_samples: usize,
    /// Sustain amplitude.
    pub sustain_level: f32,
    /// Release duration in frames.
    pub release_samples: usize,
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) fn compile_generator(
    generator: &GeneratorDefinition,
    layer_index: usize,
    layer_id: &str,
    catalog: &ParameterCatalog,
    definition_base_dir: &Path,
    sample_rate: f64,
    max_block_size: usize,
    asset_cache: &mut HashMap<AssetCacheKey, Result<PreparedAsset, AssetError>>,
    wavetable_asset_cache: &mut HashMap<
        WavetableAssetCacheKey,
        Result<WavetablePreparation, WavetablePreparationError>,
    >,
    spectral_asset_cache: &mut HashMap<
        SpectralAssetCacheKey,
        Result<Arc<PreparedSpectralAsset>, SpectralPreparationError>,
    >,
    spectral_plan_cache: &mut HashMap<usize, Arc<SpectralSynthesisPlan>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledGenerator {
    match generator {
        GeneratorDefinition::Oscillator(OscillatorDefinition {
            waveform,
            phase_reset,
            phase,
            hard_sync,
            waveshaping,
            phase_distortion,
            wavefold,
            feedback,
            unison,
        }) => {
            let pulse_width = matches!(waveform, OscillatorWaveform::Pulse { .. })
                .then(|| generator_parameter_handle(catalog, layer_id, PULSE_WIDTH));
            let waveshape = waveshaping
                .as_ref()
                .map(|_| generator_parameter_handle(catalog, layer_id, WAVESHAPE));
            let phase_distortion = phase_distortion
                .as_ref()
                .map(|_| generator_parameter_handle(catalog, layer_id, PHASE_DISTORTION));
            let wavefold = wavefold
                .as_ref()
                .map(|_| generator_parameter_handle(catalog, layer_id, WAVEFOLD));
            let oscillator_feedback = feedback
                .as_ref()
                .map(|_| generator_parameter_handle(catalog, layer_id, OSCILLATOR_FEEDBACK));
            let unison_detune = unison
                .as_ref()
                .map(|_| generator_parameter_handle(catalog, layer_id, UNISON_DETUNE));
            let unison_spread = unison
                .as_ref()
                .map(|_| generator_parameter_handle(catalog, layer_id, UNISON_SPREAD));
            let unison = compile_unison(*unison);
            CompiledGenerator::Oscillator(CompiledOscillator {
                waveform: *waveform,
                phase_reset: *phase_reset,
                phase: *phase,
                backend: if phase_distortion.is_some() || oscillator_feedback.is_some() {
                    CompiledOscillatorBackend::PhaseDomain
                } else if hard_sync.is_some() {
                    CompiledOscillatorBackend::VariableShapeSync {
                        sync_ratio: generator_parameter_handle(catalog, layer_id, SYNC_RATIO),
                    }
                } else {
                    CompiledOscillatorBackend::Basic
                },
                parameters: CompiledOscillatorParameters {
                    pulse_width,
                    waveshape,
                    phase_distortion,
                    wavefold,
                    oscillator_feedback,
                    unison_detune,
                    unison_spread,
                },
                dc_blocker: phase_distortion.is_some()
                    || wavefold.is_some()
                    || oscillator_feedback.is_some(),
                unison: Arc::new(unison),
            })
        }
        GeneratorDefinition::Noise(noise) => CompiledGenerator::Noise(CompiledNoise {
            color: noise.color,
            seed: noise.seed,
            correlation: generator_parameter_handle(catalog, layer_id, NOISE_CORRELATION),
            layer_hash: source_id_hash(layer_id),
            brown_coefficient: brown_noise_coefficient(sample_rate),
        }),
        GeneratorDefinition::PhysicalString(physical_string) => CompiledGenerator::PhysicalString(
            compile_physical_string(physical_string, layer_id, catalog, sample_rate),
        ),
        GeneratorDefinition::Modal(modal) => {
            CompiledGenerator::Modal(compile_modal(modal, layer_id, catalog, sample_rate))
        }
        GeneratorDefinition::Additive(additive) => CompiledGenerator::Additive(compile_additive(
            additive,
            layer_index,
            layer_id,
            catalog,
            sample_rate,
            diagnostics,
        )),
        GeneratorDefinition::Formant(formant) => {
            CompiledGenerator::Formant(compile_formant(formant, layer_id, catalog))
        }
        GeneratorDefinition::Sample(sample) => CompiledGenerator::Sample(compile_sample(
            sample,
            layer_index,
            definition_base_dir,
            sample_rate,
            max_block_size,
            asset_cache,
            diagnostics,
        )),
        GeneratorDefinition::Granular(granular) => CompiledGenerator::Granular(compile_granular(
            granular,
            layer_index,
            layer_id,
            catalog,
            definition_base_dir,
            sample_rate,
            asset_cache,
            diagnostics,
        )),
        GeneratorDefinition::WaveSequence(sequence) => {
            CompiledGenerator::WaveSequence(compile_wave_sequence(
                sequence,
                layer_index,
                definition_base_dir,
                sample_rate,
                asset_cache,
                diagnostics,
            ))
        }
        GeneratorDefinition::Wavetable(wavetable) => {
            CompiledGenerator::Wavetable(compile_wavetable(
                wavetable,
                layer_index,
                layer_id,
                catalog,
                definition_base_dir,
                sample_rate,
                wavetable_asset_cache,
                diagnostics,
            ))
        }
        GeneratorDefinition::Spectral(spectral) => CompiledGenerator::Spectral(compile_spectral(
            spectral,
            layer_index,
            layer_id,
            catalog,
            definition_base_dir,
            sample_rate,
            asset_cache,
            spectral_asset_cache,
            spectral_plan_cache,
            diagnostics,
        )),
        GeneratorDefinition::OperatorModulation(operator_modulation) => {
            CompiledGenerator::OperatorModulation(compile_operator_modulation(
                operator_modulation,
                layer_index,
                layer_id,
                catalog,
                sample_rate,
                diagnostics,
            ))
        }
    }
}

fn compile_physical_exciter(exciter: PhysicalExciterDefinition) -> CompiledPhysicalExciter {
    match exciter {
        PhysicalExciterDefinition::Impulse => CompiledPhysicalExciter::Impulse,
        PhysicalExciterDefinition::NoiseBurst {
            duration_seconds,
            brightness,
            seed,
        } => CompiledPhysicalExciter::NoiseBurst {
            duration_seconds,
            brightness,
            seed,
        },
    }
}

fn compile_physical_string(
    value: &PhysicalStringDefinition,
    layer_id: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
) -> CompiledPhysicalString {
    CompiledPhysicalString {
        exciter: compile_physical_exciter(value.exciter),
        parameters: CompiledPhysicalStringParameters {
            decay_seconds: generator_parameter_handle(
                catalog,
                layer_id,
                PHYSICAL_STRING_DECAY_SECONDS,
            ),
            brightness: generator_parameter_handle(catalog, layer_id, PHYSICAL_STRING_BRIGHTNESS),
            stiffness: generator_parameter_handle(catalog, layer_id, PHYSICAL_STRING_STIFFNESS),
        },
        layer_hash: source_id_hash(layer_id),
        effective_max_frequency: effective_max_frequency(
            sample_rate,
            PHYSICAL_FREQUENCY_LIMIT_RATIO,
        ),
    }
}

fn compile_modal(
    value: &ModalDefinition,
    layer_id: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
) -> CompiledModal {
    CompiledModal {
        exciter: compile_physical_exciter(value.exciter),
        mode_count: value.mode_count,
        parameters: CompiledModalParameters {
            structure: generator_parameter_handle(catalog, layer_id, MODAL_STRUCTURE),
            brightness: generator_parameter_handle(catalog, layer_id, MODAL_BRIGHTNESS),
            decay: generator_parameter_handle(catalog, layer_id, MODAL_DECAY),
        },
        layer_hash: source_id_hash(layer_id),
        effective_max_frequency: effective_max_frequency(
            sample_rate,
            PHYSICAL_FREQUENCY_LIMIT_RATIO,
        ),
    }
}

fn generator_parameter_handle(
    catalog: &ParameterCatalog,
    layer_id: &str,
    spec: GeneratorParameterSpec,
) -> ParameterHandle {
    catalog
        .parameter_handle(&layer_generator_parameter_id(layer_id, spec.suffix))
        .expect("generator parameter catalog entry exists")
}

fn compile_additive(
    additive: &AdditiveDefinition,
    layer_index: usize,
    layer_id: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledAdditive {
    let partials = additive
        .partials
        .iter()
        .enumerate()
        .map(|(index, partial)| CompiledAdditivePartial {
            id: partial.id.clone(),
            ratio: partial.ratio,
            amplitude_a: partial.amplitude_a,
            amplitude_b: partial.amplitude_b,
            phase: partial.phase,
            envelope: partial.envelope.map(|envelope| {
                compile_adsr(
                    envelope,
                    sample_rate,
                    &format!("layers[{layer_index}].generator.additive.partials[{index}]"),
                    diagnostics,
                )
            }),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    CompiledAdditive {
        partials,
        phase_reset: additive.phase_reset,
        parameters: CompiledAdditiveParameters {
            morph: generator_parameter_handle(catalog, layer_id, ADDITIVE_MORPH),
            spectrum_tilt: generator_parameter_handle(catalog, layer_id, ADDITIVE_SPECTRUM_TILT),
            inharmonicity: generator_parameter_handle(catalog, layer_id, ADDITIVE_INHARMONICITY),
        },
        sine_table: build_sine_table(),
    }
}

fn compile_formant(
    formant: &FormantDefinition,
    layer_id: &str,
    catalog: &ParameterCatalog,
) -> CompiledFormant {
    let profiles = formant
        .profiles
        .iter()
        .map(|profile| CompiledFormantProfile {
            id: profile.id.clone(),
            formants: std::array::from_fn(|index| {
                let band = profile
                    .formants
                    .get(index)
                    .expect("validated formant profile contains five bands");
                CompiledFormantBand {
                    frequency_hz: band.frequency_hz,
                    bandwidth_hz: band.bandwidth_hz,
                    gain_db: band.gain_db,
                }
            }),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    CompiledFormant {
        partial_count: usize::from(formant.partial_count),
        phase_reset: formant.phase_reset,
        profiles,
        parameters: CompiledFormantParameters {
            vowel_position: generator_parameter_handle(catalog, layer_id, FORMANT_VOWEL_POSITION),
            formant_shift: generator_parameter_handle(catalog, layer_id, FORMANT_SHIFT),
            throat: generator_parameter_handle(catalog, layer_id, FORMANT_THROAT),
            spectral_tilt: generator_parameter_handle(catalog, layer_id, FORMANT_SPECTRAL_TILT),
        },
        sine_table: build_sine_table(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct WavetableAssetCacheKey {
    path: PathBuf,
    sha256: Option<String>,
    frame_length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SpectralAssetCacheKey {
    path: PathBuf,
    sha256: Option<String>,
    sample_rate_bits: u64,
    fft_size: usize,
}

fn prepare_cached_wavetable(
    reference: &crate::definition::AssetReference,
    definition_base_dir: &Path,
    frame_length: usize,
    asset_cache: &mut HashMap<
        WavetableAssetCacheKey,
        Result<WavetablePreparation, WavetablePreparationError>,
    >,
) -> Result<WavetablePreparation, WavetablePreparationError> {
    let resolved = resolved_asset_path(definition_base_dir, &reference.path);
    let path = std::fs::canonicalize(&resolved).unwrap_or(resolved);
    let key = WavetableAssetCacheKey {
        path,
        sha256: reference
            .sha256
            .as_ref()
            .map(|value| value.to_ascii_lowercase()),
        frame_length,
    };
    if let Some(result) = asset_cache.get(&key) {
        return result.clone();
    }
    let result = prepare_wavetable_asset(reference, definition_base_dir, frame_length);
    asset_cache.insert(key, result.clone());
    result
}

fn prepare_cached_spectral(
    reference: &crate::definition::AssetReference,
    definition_base_dir: &Path,
    sample_rate: f64,
    fft_size: usize,
    asset_cache: &mut HashMap<AssetCacheKey, Result<PreparedAsset, AssetError>>,
    spectral_cache: &mut HashMap<
        SpectralAssetCacheKey,
        Result<Arc<PreparedSpectralAsset>, SpectralPreparationError>,
    >,
) -> Result<Arc<PreparedSpectralAsset>, SpectralPreparationError> {
    let resolved = resolved_asset_path(definition_base_dir, &reference.path);
    let path = std::fs::canonicalize(&resolved).unwrap_or(resolved);
    let key = SpectralAssetCacheKey {
        path,
        sha256: reference
            .sha256
            .as_ref()
            .map(|value| value.to_ascii_lowercase()),
        sample_rate_bits: sample_rate.to_bits(),
        fft_size,
    };
    if let Some(result) = spectral_cache.get(&key) {
        return result.clone();
    }
    let result = prepare_cached_asset(reference, definition_base_dir, sample_rate, asset_cache)
        .map_err(SpectralPreparationError::Asset)
        .and_then(|prepared| prepare_spectral_asset(&prepared.audio, fft_size).map(Arc::new));
    spectral_cache.insert(key, result.clone());
    result
}

#[allow(clippy::too_many_arguments)]
fn compile_wavetable(
    wavetable: &WavetableDefinition,
    layer_index: usize,
    layer_id: &str,
    catalog: &ParameterCatalog,
    definition_base_dir: &Path,
    sample_rate: f64,
    asset_cache: &mut HashMap<
        WavetableAssetCacheKey,
        Result<WavetablePreparation, WavetablePreparationError>,
    >,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledWavetable {
    let asset_path = format!("layers[{layer_index}].generator.wavetable.asset.path");
    if Path::new(&wavetable.asset.path).is_absolute() {
        diagnostics.push(
            Diagnostic::warning(
                DiagnosticCode::AssetAbsolutePath,
                "absolute asset paths reduce Definition portability",
            )
            .with_path(asset_path.clone()),
        );
    }
    let prepared = prepare_compiled_wavetable(
        &wavetable.asset,
        definition_base_dir,
        usize::from(wavetable.frame_length),
        asset_cache,
        &asset_path,
        &format!("layers[{layer_index}].generator.wavetable.asset.sha256"),
        diagnostics,
    )
    .map(|prepared| prepared.prepared);
    let position = generator_parameter_handle(catalog, layer_id, WAVETABLE_POSITION);
    let unison_detune = wavetable
        .unison
        .as_ref()
        .map(|_| generator_parameter_handle(catalog, layer_id, UNISON_DETUNE));
    let unison_spread = wavetable
        .unison
        .as_ref()
        .map(|_| generator_parameter_handle(catalog, layer_id, UNISON_SPREAD));
    CompiledWavetable {
        prepared,
        frame_length: usize::from(wavetable.frame_length),
        phase_reset: wavetable.phase_reset,
        phase: wavetable.phase,
        effective_max_frequency: effective_max_frequency(sample_rate, BASIC_FREQUENCY_LIMIT_RATIO),
        parameters: CompiledWavetableParameters {
            position,
            unison_detune,
            unison_spread,
        },
        unison: Arc::new(compile_unison(wavetable.unison)),
        asset_path: wavetable.asset.path.clone(),
        asset_sha256_specified: wavetable.asset.sha256.is_some(),
    }
}

#[allow(clippy::too_many_arguments)]
fn compile_spectral(
    spectral: &SpectralDefinition,
    layer_index: usize,
    layer_id: &str,
    catalog: &ParameterCatalog,
    definition_base_dir: &Path,
    sample_rate: f64,
    asset_cache: &mut HashMap<AssetCacheKey, Result<PreparedAsset, AssetError>>,
    spectral_asset_cache: &mut HashMap<
        SpectralAssetCacheKey,
        Result<Arc<PreparedSpectralAsset>, SpectralPreparationError>,
    >,
    spectral_plan_cache: &mut HashMap<usize, Arc<SpectralSynthesisPlan>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledSpectral {
    let spectral_path = format!("layers[{layer_index}].generator.spectral");
    let asset_a_path = format!("{spectral_path}.asset_a.path");
    let fft_size = usize::from(spectral.fft_size);
    let hop_size = spectral_hop_size(fft_size).expect("validated spectral fft size");
    let source = compile_spectral_asset(
        &spectral.asset_a,
        &asset_a_path,
        &format!("{spectral_path}.asset_a.sha256"),
        definition_base_dir,
        sample_rate,
        fft_size,
        asset_cache,
        spectral_asset_cache,
        diagnostics,
    );
    let asset_b_path = spectral
        .asset_b
        .as_ref()
        .map(|_| format!("{spectral_path}.asset_b.path"));
    let source_b = spectral.asset_b.as_ref().and_then(|asset_b| {
        let path = asset_b_path.as_deref().expect("asset B path exists");
        let prepared = compile_spectral_asset(
            asset_b,
            path,
            &format!("{spectral_path}.asset_b.sha256"),
            definition_base_dir,
            sample_rate,
            fft_size,
            asset_cache,
            spectral_asset_cache,
            diagnostics,
        );
        if let (Some(source_a), Some(source_b)) = (source.as_ref(), prepared.as_ref())
            && source_a.channels != source_b.channels
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::SpectralPreparationFailed,
                    "spectral asset A and asset B must have the same channel count",
                )
                .with_path(path)
                .with_detail(format!(
                    "asset A has {} channels, asset B has {} channels",
                    source_a.channels, source_b.channels
                )),
            );
            return None;
        }
        prepared
    });
    let synthesis_plan = spectral_plan_cache
        .entry(fft_size)
        .or_insert_with(|| {
            Arc::new(SpectralSynthesisPlan::new(fft_size).expect("validated spectral fft size"))
        })
        .clone();
    let available = source.is_some() && (spectral.asset_b.is_none() || source_b.is_some());
    CompiledSpectral {
        latency_frames: if available {
            source.as_ref().map_or(0, |value| value.latency_frames)
        } else {
            0
        },
        source,
        source_b,
        asset_a_path: spectral.asset_a.path.clone(),
        asset_a_sha256_specified: spectral.asset_a.sha256.is_some(),
        asset_b_path: spectral.asset_b.as_ref().map(|asset| asset.path.clone()),
        asset_b_sha256_specified: spectral
            .asset_b
            .as_ref()
            .and_then(|asset| asset.sha256.as_ref())
            .is_some(),
        root_note: spectral.root_note,
        fft_size,
        hop_size,
        phase_reset: spectral.phase_reset,
        parameters: CompiledSpectralParameters {
            position: generator_parameter_handle(catalog, layer_id, SPECTRAL_POSITION),
            freeze: generator_parameter_handle(catalog, layer_id, SPECTRAL_FREEZE),
            blur: generator_parameter_handle(catalog, layer_id, SPECTRAL_BLUR),
            shift: generator_parameter_handle(catalog, layer_id, SPECTRAL_SHIFT),
            morph: spectral
                .asset_b
                .as_ref()
                .map(|_| generator_parameter_handle(catalog, layer_id, SPECTRAL_MORPH)),
        },
        synthesis_plan,
    }
}

#[allow(clippy::too_many_arguments)]
fn compile_spectral_asset(
    reference: &AssetReference,
    asset_path: &str,
    hash_path: &str,
    definition_base_dir: &Path,
    sample_rate: f64,
    fft_size: usize,
    asset_cache: &mut HashMap<AssetCacheKey, Result<PreparedAsset, AssetError>>,
    spectral_asset_cache: &mut HashMap<
        SpectralAssetCacheKey,
        Result<Arc<PreparedSpectralAsset>, SpectralPreparationError>,
    >,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Arc<PreparedSpectralAsset>> {
    if Path::new(&reference.path).is_absolute() {
        diagnostics.push(
            Diagnostic::warning(
                DiagnosticCode::AssetAbsolutePath,
                "absolute asset paths reduce Definition portability",
            )
            .with_path(asset_path),
        );
    }
    match prepare_cached_spectral(
        reference,
        definition_base_dir,
        sample_rate,
        fft_size,
        asset_cache,
        spectral_asset_cache,
    ) {
        Ok(source) => {
            if reference.sha256.is_none() {
                diagnostics.push(
                    Diagnostic::warning(
                        DiagnosticCode::AssetHashMissing,
                        "asset sha256 is not specified",
                    )
                    .with_path(hash_path),
                );
            }
            if (f64::from(source.source_metadata.source_sample_rate) - sample_rate).abs()
                > f64::EPSILON
            {
                diagnostics.push(
                    Diagnostic::warning(
                        DiagnosticCode::AssetResampled,
                        "asset was resampled to the process sample rate",
                    )
                    .with_path(asset_path),
                );
            }
            Some(source)
        }
        Err(error) => {
            report_spectral_preparation_error(asset_path, error, diagnostics);
            None
        }
    }
}

fn report_spectral_preparation_error(
    asset_path: &str,
    error: SpectralPreparationError,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let (severity, code, message, detail) = match error {
        SpectralPreparationError::Asset(error) => {
            let (code, message) = asset_diagnostic(&error);
            (
                DiagnosticSeverity::Warning,
                code,
                message.to_owned(),
                Some(error.to_string()),
            )
        }
        SpectralPreparationError::InvalidFftSize(value) => (
            DiagnosticSeverity::Error,
            DiagnosticCode::SpectralPreparationFailed,
            "spectral FFT size is invalid".to_owned(),
            Some(format!("FFT size {value} is not supported")),
        ),
        SpectralPreparationError::Layout(detail)
        | SpectralPreparationError::Preparation(detail) => (
            DiagnosticSeverity::Error,
            DiagnosticCode::SpectralPreparationFailed,
            "spectral preparation failed".to_owned(),
            Some(detail),
        ),
        SpectralPreparationError::ResourceLimit(bytes) => (
            DiagnosticSeverity::Error,
            DiagnosticCode::GeneratorResourceLimitExceeded,
            "prepared spectral asset exceeds the resource limit".to_owned(),
            Some(format!("prepared spectral data requires {bytes} bytes")),
        ),
    };
    let diagnostic = if severity == DiagnosticSeverity::Warning {
        Diagnostic::warning(code, message)
    } else {
        Diagnostic::error(code, message)
    }
    .with_path(asset_path);
    diagnostics.push(if let Some(detail) = detail {
        diagnostic.with_detail(detail)
    } else {
        diagnostic
    });
}

fn compile_operator_modulation(
    operator_modulation: &OperatorModulationDefinition,
    layer_index: usize,
    layer_id: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledOperatorModulation {
    let topology = operator_modulation.algorithm.topology();
    let carrier_count = topology.carrier_mask.count_ones().max(1);
    #[allow(clippy::cast_precision_loss)]
    let carrier_count = carrier_count as f32;
    let compiled_topology = CompiledOperatorTopology {
        evaluation_order: topology.evaluation_order,
        incoming_masks: topology.incoming_masks,
        carrier_mask: topology.carrier_mask,
        carrier_normalization: 1.0 / carrier_count.sqrt(),
    };
    let operators = std::array::from_fn(|index| {
        let operator = operator_modulation
            .operators
            .get(index)
            .expect("operator validation guarantees four operators");
        CompiledOperator {
            envelope: compile_adsr(
                operator.envelope,
                sample_rate,
                &format!("layers[{layer_index}].generator.operator_modulation.operators[{index}]"),
                diagnostics,
            ),
            phase: operator.phase,
        }
    });
    let parameters = std::array::from_fn(|index| {
        let is_carrier = topology.carrier_mask & (1_u8 << index) != 0;
        let has_output = topology
            .incoming_masks
            .iter()
            .any(|mask| mask & (1_u8 << index) != 0);
        CompiledOperatorParameters {
            ratio: operator_parameter_handle(catalog, layer_id, index, "ratio"),
            detune: operator_parameter_handle(catalog, layer_id, index, "detune"),
            level: is_carrier.then(|| operator_parameter_handle(catalog, layer_id, index, "level")),
            modulation_amount: has_output
                .then(|| operator_parameter_handle(catalog, layer_id, index, "modulation_amount")),
            feedback: matches!(
                operator_modulation.mode,
                OperatorModulationMode::Phase | OperatorModulationMode::Frequency
            )
            .then(|| operator_parameter_handle(catalog, layer_id, index, "feedback")),
        }
    });
    let frequency_limit_ratio = match operator_modulation.mode {
        OperatorModulationMode::Phase | OperatorModulationMode::Frequency => {
            PHASE_DOMAIN_FREQUENCY_LIMIT_RATIO
        }
        OperatorModulationMode::Amplitude | OperatorModulationMode::Ring => {
            BASIC_FREQUENCY_LIMIT_RATIO
        }
    };
    let effective_max_frequency = effective_max_frequency(sample_rate, frequency_limit_ratio);
    let unison_detune = operator_modulation
        .unison
        .as_ref()
        .map(|_| generator_parameter_handle(catalog, layer_id, UNISON_DETUNE));
    let unison_spread = operator_modulation
        .unison
        .as_ref()
        .map(|_| generator_parameter_handle(catalog, layer_id, UNISON_SPREAD));
    CompiledOperatorModulation {
        mode: operator_modulation.mode,
        algorithm: operator_modulation.algorithm,
        topology: compiled_topology,
        operators,
        phase_reset: operator_modulation.phase_reset,
        parameters,
        unison_detune,
        unison_spread,
        unison: Arc::new(compile_unison(operator_modulation.unison)),
        effective_max_frequency,
    }
}

fn operator_parameter_handle(
    catalog: &ParameterCatalog,
    layer_id: &str,
    index: usize,
    parameter: &str,
) -> ParameterHandle {
    catalog
        .parameter_handle(&format!(
            "layer.{layer_id}.generator.operator.{}.{}",
            index + 1,
            parameter
        ))
        .expect("operator parameter catalog entry exists")
}

fn prepare_compiled_wavetable(
    reference: &crate::definition::AssetReference,
    definition_base_dir: &Path,
    frame_length: usize,
    asset_cache: &mut HashMap<
        WavetableAssetCacheKey,
        Result<WavetablePreparation, WavetablePreparationError>,
    >,
    diagnostic_path: &str,
    hash_path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<WavetablePreparation> {
    let result =
        prepare_cached_wavetable(reference, definition_base_dir, frame_length, asset_cache);
    match result {
        Ok(preparation) => {
            report_wavetable_warnings(
                reference,
                &preparation,
                diagnostic_path,
                hash_path,
                diagnostics,
            );
            Some(preparation)
        }
        Err(error) => {
            report_wavetable_preparation_error(diagnostic_path, error, diagnostics);
            None
        }
    }
}

fn report_wavetable_warnings(
    reference: &crate::definition::AssetReference,
    preparation: &WavetablePreparation,
    diagnostic_path: &str,
    hash_path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for warning in &preparation.warnings {
        match warning {
            WavetableWarning::SilentFrame { index, rms } => diagnostics.push(
                Diagnostic::warning(
                    DiagnosticCode::WavetableSilentFrame,
                    format!("wavetable frame {index} is nearly silent"),
                )
                .with_path(diagnostic_path)
                .with_detail(format!("frame index {index}, rms is {rms:.6e}")),
            ),
            WavetableWarning::DcOffset { index, mean } => diagnostics.push(
                Diagnostic::warning(
                    DiagnosticCode::WavetableDcOffset,
                    format!("wavetable frame {index} has a DC offset"),
                )
                .with_path(diagnostic_path)
                .with_detail(format!("frame index {index}, mean is {mean:.6e}")),
            ),
        }
    }
    if reference.sha256.is_none() {
        diagnostics.push(
            Diagnostic::warning(
                DiagnosticCode::AssetHashMissing,
                "asset sha256 is not specified",
            )
            .with_path(hash_path),
        );
    }
    if preparation.prepared.source_metadata.source_channels > 1 {
        diagnostics.push(
            Diagnostic::warning(
                DiagnosticCode::AssetDownmixed,
                "stereo asset was downmixed to mono",
            )
            .with_path(diagnostic_path),
        );
    }
}

fn report_wavetable_preparation_error(
    asset_path: &str,
    error: WavetablePreparationError,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let (severity, code, message, detail) = match error {
        WavetablePreparationError::Asset(error) => {
            let (code, message) = asset_diagnostic(&error);
            (
                DiagnosticSeverity::Warning,
                code,
                message.to_owned(),
                Some(error.to_string()),
            )
        }
        WavetablePreparationError::Silent => (
            DiagnosticSeverity::Warning,
            DiagnosticCode::WavetablePreparationFailed,
            "wavetable asset contains no audible frame".to_owned(),
            None,
        ),
        WavetablePreparationError::Layout(detail) => (
            DiagnosticSeverity::Error,
            DiagnosticCode::WavetableLayoutInvalid,
            "wavetable asset layout is invalid".to_owned(),
            Some(detail),
        ),
        WavetablePreparationError::ResourceLimit(bytes) => (
            DiagnosticSeverity::Error,
            DiagnosticCode::GeneratorResourceLimitExceeded,
            "prepared wavetable exceeds the resource limit".to_owned(),
            Some(format!("prepared table requires {bytes} bytes")),
        ),
        WavetablePreparationError::Preparation(detail) => (
            DiagnosticSeverity::Error,
            DiagnosticCode::WavetablePreparationFailed,
            "wavetable preparation failed".to_owned(),
            Some(detail),
        ),
    };
    let diagnostic = if severity == DiagnosticSeverity::Warning {
        Diagnostic::warning(code, message)
    } else {
        Diagnostic::error(code, message)
    }
    .with_path(asset_path);
    let diagnostic = if let Some(detail) = detail {
        diagnostic.with_detail(detail)
    } else {
        diagnostic
    };
    diagnostics.push(diagnostic);
}

#[allow(clippy::too_many_lines)]
#[allow(clippy::too_many_arguments)]
fn compile_granular(
    granular: &GranularDefinition,
    layer_index: usize,
    layer_id: &str,
    catalog: &ParameterCatalog,
    definition_base_dir: &Path,
    sample_rate: f64,
    asset_cache: &mut HashMap<AssetCacheKey, Result<PreparedAsset, AssetError>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledGranular {
    let granular_path = format!("layers[{layer_index}].generator.granular");
    let mut source = None;
    let mut start_frame = 0;
    let mut end_frame = 0;
    if Path::new(&granular.asset.path).is_absolute() {
        diagnostics.push(
            Diagnostic::warning(
                DiagnosticCode::AssetAbsolutePath,
                "absolute asset paths reduce Definition portability",
            )
            .with_path(format!("{granular_path}.asset.path")),
        );
    }
    match prepare_cached_asset(
        &granular.asset,
        definition_base_dir,
        sample_rate,
        asset_cache,
    ) {
        Ok(prepared) => {
            if granular.asset.sha256.is_none() {
                diagnostics.push(
                    Diagnostic::warning(
                        DiagnosticCode::AssetHashMissing,
                        "asset sha256 is not specified",
                    )
                    .with_path(format!("{granular_path}.asset.sha256")),
                );
            }
            if (f64::from(prepared.audio.source_metadata.source_sample_rate) - sample_rate).abs()
                > f64::EPSILON
            {
                diagnostics.push(
                    Diagnostic::warning(
                        DiagnosticCode::AssetResampled,
                        "asset was resampled to the process sample rate",
                    )
                    .with_path(format!("{granular_path}.asset.path")),
                );
            }
            let region_start = granular_time_to_frame(
                granular.region.start_seconds,
                sample_rate,
                &granular_path,
                "region.start_seconds",
                diagnostics,
            );
            let region_end =
                granular
                    .region
                    .end_seconds
                    .map_or(Some(prepared.audio.frames), |seconds| {
                        granular_time_to_frame(
                            seconds,
                            sample_rate,
                            &granular_path,
                            "region.end_seconds",
                            diagnostics,
                        )
                    });
            if let (Some(region_start), Some(region_end)) = (region_start, region_end) {
                if region_start < region_end
                    && region_end <= prepared.audio.frames
                    && region_end - region_start >= 2
                {
                    source = Some(Arc::clone(&prepared.audio));
                    start_frame = region_start;
                    end_frame = region_end;
                } else {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::InvalidGrainRegion,
                            "granular region must contain at least two prepared frames",
                        )
                        .with_path(format!("{granular_path}.region")),
                    );
                }
            }
        }
        Err(error) => {
            let (code, message) = asset_diagnostic(&error);
            diagnostics.push(
                Diagnostic::warning(code, message)
                    .with_path(format!("{granular_path}.asset.path"))
                    .with_detail(error.to_string()),
            );
        }
    }
    CompiledGranular {
        source,
        asset_path: granular.asset.path.clone(),
        asset_sha256_specified: granular.asset.sha256.is_some(),
        root_note: granular.root_note,
        start_frame,
        end_frame,
        parameters: CompiledGranularParameters {
            position: generator_parameter_handle(catalog, layer_id, GRANULAR_POSITION),
            grain_size: generator_parameter_handle(catalog, layer_id, GRAIN_SIZE),
            density: generator_parameter_handle(catalog, layer_id, GRAIN_DENSITY),
            pitch: generator_parameter_handle(catalog, layer_id, GRAIN_PITCH),
            randomness: generator_parameter_handle(catalog, layer_id, GRAIN_RANDOMNESS),
            pan_spread: generator_parameter_handle(catalog, layer_id, GRAIN_PAN_SPREAD),
        },
        seed: granular.seed,
        layer_hash: source_id_hash(layer_id),
        grain_pool_limit: GRANULAR_GRAIN_POOL_LIMIT,
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
fn compile_wave_sequence(
    sequence: &WaveSequenceDefinition,
    layer_index: usize,
    definition_base_dir: &Path,
    sample_rate: f64,
    asset_cache: &mut HashMap<AssetCacheKey, Result<PreparedAsset, AssetError>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledWaveSequence {
    let sequence_path = format!("layers[{layer_index}].generator.wave_sequence");
    let steps = sequence
        .steps
        .iter()
        .enumerate()
        .map(|(step_index, step)| {
            let step_path = format!("{sequence_path}.steps[{step_index}]");
            let mut compiled = CompiledWaveSequenceStep {
                id: step.id.clone(),
                source: None,
                asset_path: step.asset.path.clone(),
                start_frame: 0,
                end_frame: 0,
                duration: compile_wave_sequence_duration(step.duration),
                playback: match step.playback {
                    WaveSequenceStepPlayback::OneShot => {
                        CompiledWaveSequenceStepPlayback::OneShot
                    }
                    WaveSequenceStepPlayback::Loop => CompiledWaveSequenceStepPlayback::Loop,
                },
                playback_direction: match step.playback_direction {
                    SamplePlaybackDirection::Forward => CompiledSampleDirection::Forward,
                    SamplePlaybackDirection::Reverse => CompiledSampleDirection::Reverse,
                },
                gain: db_to_linear(step.gain_db),
                pitch_cents: step.pitch_cents,
            };
            if Path::new(&step.asset.path).is_absolute() {
                diagnostics.push(
                    Diagnostic::warning(
                        DiagnosticCode::AssetAbsolutePath,
                        "absolute asset paths reduce Definition portability",
                    )
                    .with_path(format!("{step_path}.asset.path")),
                );
            }
            match prepare_cached_asset(&step.asset, definition_base_dir, sample_rate, asset_cache) {
                Ok(prepared) => {
                    if step.asset.sha256.is_none() {
                        diagnostics.push(
                            Diagnostic::warning(
                                DiagnosticCode::AssetHashMissing,
                                "asset sha256 is not specified",
                            )
                            .with_path(format!("{step_path}.asset.sha256")),
                        );
                    }
                    if (f64::from(prepared.audio.source_metadata.source_sample_rate)
                        - sample_rate)
                        .abs()
                        > f64::EPSILON
                    {
                        diagnostics.push(
                            Diagnostic::warning(
                                DiagnosticCode::AssetResampled,
                                "asset was resampled to the process sample rate",
                            )
                            .with_path(format!("{step_path}.asset.path")),
                        );
                    }
                    let start_frame = sequence_time_to_frame(
                        step.region.start_seconds,
                        sample_rate,
                        &format!("{step_path}.region.start_seconds"),
                        diagnostics,
                    );
                    let end_frame = step.region.end_seconds.map_or(
                        Some(prepared.audio.frames),
                        |seconds| {
                            sequence_time_to_frame(
                                seconds,
                                sample_rate,
                                &format!("{step_path}.region.end_seconds"),
                                diagnostics,
                            )
                        },
                    );
                    if let (Some(start_frame), Some(end_frame)) = (start_frame, end_frame) {
                        if start_frame < end_frame
                            && end_frame <= prepared.audio.frames
                            && end_frame - start_frame >= 2
                        {
                            compiled.start_frame = start_frame;
                            compiled.end_frame = end_frame;
                            compiled.source = Some(Arc::clone(&prepared.audio));
                        } else {
                            diagnostics.push(
                                Diagnostic::error(
                                    DiagnosticCode::InvalidSequence,
                                    "wave sequence region must contain at least two prepared frames inside the asset",
                                )
                                .with_path(format!("{step_path}.region")),
                            );
                        }
                    }
                }
                Err(error) => {
                    let (code, message) = asset_diagnostic(&error);
                    diagnostics.push(
                        Diagnostic::warning(code, message)
                            .with_path(format!("{step_path}.asset.path"))
                            .with_detail(error.to_string()),
                    );
                }
            }
            compiled
        })
        .collect::<Vec<_>>();
    CompiledWaveSequence {
        root_note: sequence.root_note,
        direction: sequence.direction,
        loop_sequence: sequence.loop_sequence,
        crossfade: sequence.crossfade,
        steps: Arc::from(steps.into_boxed_slice()),
    }
}

fn compile_wave_sequence_duration(
    duration: WaveSequenceDurationDefinition,
) -> CompiledWaveSequenceDuration {
    match duration {
        WaveSequenceDurationDefinition::Seconds { value } => {
            CompiledWaveSequenceDuration::Seconds(f64::from(value))
        }
        WaveSequenceDurationDefinition::Beats { value } => {
            CompiledWaveSequenceDuration::Beats(f64::from(value))
        }
    }
}

fn sequence_time_to_frame(
    seconds: f32,
    sample_rate: f64,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<usize> {
    let frames = (f64::from(seconds) * sample_rate).round();
    #[allow(clippy::cast_precision_loss)]
    let max_usize = usize::MAX as f64;
    if !frames.is_finite() || frames < 0.0 || frames > max_usize {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::InvalidSequence,
                "wave sequence region time does not fit in the process frame counter",
            )
            .with_path(path),
        );
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(frames as usize)
}

#[allow(clippy::too_many_lines)]
fn compile_sample(
    sample: &crate::definition::SampleDefinition,
    layer_index: usize,
    definition_base_dir: &Path,
    sample_rate: f64,
    max_block_size: usize,
    asset_cache: &mut HashMap<AssetCacheKey, Result<PreparedAsset, AssetError>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledSample {
    let mut zones = Vec::with_capacity(sample.zones.len());
    for (zone_index, zone) in sample.zones.iter().enumerate() {
        let zone_path = format!("layers[{layer_index}].generator.sample.zones[{zone_index}]");
        let mut compiled = CompiledSampleZone {
            id: zone.id.clone(),
            source: None,
            root_note: zone.root_note,
            key_min: zone.key_min,
            key_max: zone.key_max,
            velocity_min: zone.velocity_min,
            velocity_max: zone.velocity_max,
            group: None,
            playback: CompiledSamplePlayback {
                direction: match zone.playback.direction {
                    SamplePlaybackDirection::Forward => CompiledSampleDirection::Forward,
                    SamplePlaybackDirection::Reverse => CompiledSampleDirection::Reverse,
                },
                start_frame: 0,
                end_frame: 0,
                loop_region: None,
                time: CompiledSampleTime::Resample,
            },
            asset_path: zone.asset.path.clone(),
        };
        if Path::new(&zone.asset.path).is_absolute() {
            diagnostics.push(
                Diagnostic::warning(
                    DiagnosticCode::AssetAbsolutePath,
                    "absolute asset paths reduce Definition portability",
                )
                .with_path(format!("{zone_path}.asset.path")),
            );
        }
        match prepare_cached_asset(&zone.asset, definition_base_dir, sample_rate, asset_cache) {
            Ok(prepared) => {
                if zone.asset.sha256.is_none() {
                    diagnostics.push(
                        Diagnostic::warning(
                            DiagnosticCode::AssetHashMissing,
                            "asset sha256 is not specified",
                        )
                        .with_path(format!("{zone_path}.asset.sha256")),
                    );
                }
                if (f64::from(prepared.audio.source_metadata.source_sample_rate) - sample_rate)
                    .abs()
                    > f64::EPSILON
                {
                    diagnostics.push(
                        Diagnostic::warning(
                            DiagnosticCode::AssetResampled,
                            "asset was resampled to the process sample rate",
                        )
                        .with_path(format!("{zone_path}.asset.path")),
                    );
                }
                if let Some(playback) = compile_sample_playback(
                    zone,
                    prepared.audio.frames,
                    sample_rate,
                    &format!("{zone_path}.playback"),
                    diagnostics,
                ) {
                    compiled.playback = playback;
                    compiled.source = Some(Arc::clone(&prepared.audio));
                }
            }
            Err(error) => {
                let (code, message) = asset_diagnostic(&error);
                diagnostics.push(
                    Diagnostic::warning(code, message)
                        .with_path(format!("{zone_path}.asset.path"))
                        .with_detail(error.to_string()),
                );
            }
        }
        zones.push(compiled);
    }

    let mut group_indices = HashMap::<String, usize>::new();
    let mut group_ids = Vec::new();
    let mut member_indices = Vec::<Vec<usize>>::new();
    let mut zone_groups = vec![None; sample.zones.len()];
    for (zone_index, zone) in sample.zones.iter().enumerate() {
        let Some(group_id) = &zone.round_robin_group else {
            continue;
        };
        let group_index = if let Some(index) = group_indices.get(group_id) {
            *index
        } else {
            let index = group_ids.len();
            group_indices.insert(group_id.clone(), index);
            group_ids.push(group_id.clone());
            member_indices.push(Vec::new());
            index
        };
        member_indices[group_index].push(zone_index);
        zone_groups[zone_index] = Some(group_index);
    }
    for (zone, group) in zones.iter_mut().zip(zone_groups) {
        zone.group = group;
    }
    let groups = group_ids
        .into_iter()
        .zip(member_indices)
        .map(|(id, members)| {
            let enabled = members
                .iter()
                .copied()
                .filter(|index| zones[*index].is_enabled())
                .collect::<Vec<_>>();
            CompiledRoundRobinGroup {
                id,
                enabled_member_zone_indices: enabled.into_boxed_slice(),
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();

    let stretch_latency = if sample
        .zones
        .iter()
        .any(|zone| zone.playback.time != SampleTimeDefinition::Resample)
    {
        compile_stretch_latency(
            sample_rate,
            max_block_size,
            &format!("layers[{layer_index}].generator.sample"),
            diagnostics,
        )
    } else {
        None
    };

    CompiledSample {
        zones: zones.into_boxed_slice(),
        groups,
        stretch_latency,
    }
}

fn compile_stretch_latency(
    sample_rate: f64,
    max_block_size: usize,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CompiledStretchLatency> {
    let Some(max_input_frames) = max_block_size.checked_mul(2) else {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::StretchBackendFailure,
                "stretch input capacity overflows the process frame counter",
            )
            .with_path(path),
        );
        return None;
    };
    let mut backend = match sonalloy_dsp_sys::DspStretch::new() {
        Ok(backend) => backend,
        Err(error) => {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::StretchBackendFailure,
                    "stretch backend allocation failed",
                )
                .with_path(path)
                .with_detail(error.to_string()),
            );
            return None;
        }
    };
    if let Err(error) = backend.prepare(2, sample_rate, max_input_frames, max_block_size) {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::StretchBackendFailure,
                "stretch backend preparation failed",
            )
            .with_path(path)
            .with_detail(error.to_string()),
        );
        return None;
    }
    let input_frames = match backend.input_latency() {
        Ok(value) => value,
        Err(error) => {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::StretchBackendFailure,
                    "stretch input latency is unavailable",
                )
                .with_path(path)
                .with_detail(error.to_string()),
            );
            return None;
        }
    };
    let output_frames = match backend.output_latency() {
        Ok(value) => value,
        Err(error) => {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::StretchBackendFailure,
                    "stretch output latency is unavailable",
                )
                .with_path(path)
                .with_detail(error.to_string()),
            );
            return None;
        }
    };
    Some(CompiledStretchLatency {
        input_frames,
        output_frames,
    })
}

fn compile_sample_playback(
    zone: &SampleZoneDefinition,
    source_frames: usize,
    sample_rate: f64,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CompiledSamplePlayback> {
    let start_seconds = zone.playback.region.start_seconds;
    let end_seconds = zone.playback.region.end_seconds;
    let start_frame = sample_time_to_frame(
        start_seconds,
        sample_rate,
        path,
        "start_seconds",
        diagnostics,
    )?;
    let end_frame = end_seconds.map_or(Some(source_frames), |seconds| {
        sample_time_to_frame(seconds, sample_rate, path, "end_seconds", diagnostics)
    })?;
    if start_frame >= source_frames || end_frame > source_frames || end_frame <= start_frame {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::CompileError,
                "sample region must fit inside the prepared asset",
            )
            .with_path(path),
        );
        return None;
    }
    if end_frame - start_frame < 2 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::CompileError,
                "sample region must contain at least two prepared frames",
            )
            .with_path(path),
        );
        return None;
    }
    let loop_region = if let Some(loop_definition) = zone.playback.r#loop {
        let loop_start_frame = sample_time_to_frame(
            loop_definition.start_seconds,
            sample_rate,
            path,
            "loop.start_seconds",
            diagnostics,
        )?;
        let loop_end_frame = sample_time_to_frame(
            loop_definition.end_seconds,
            sample_rate,
            path,
            "loop.end_seconds",
            diagnostics,
        )?;
        let crossfade_frames = sample_time_to_frame(
            loop_definition.crossfade_seconds,
            sample_rate,
            path,
            "loop.crossfade_seconds",
            diagnostics,
        )?;
        let loop_length = loop_end_frame.saturating_sub(loop_start_frame);
        if loop_start_frame < start_frame
            || loop_end_frame > end_frame
            || loop_end_frame <= loop_start_frame
            || loop_length < 2
            || crossfade_frames > loop_length / 2
        {
            diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::CompileError,
                        "sample loop must contain at least two frames inside the sample region and its crossfade must not exceed half the loop",
                    )
                    .with_path(path),
                );
            return None;
        }
        Some(CompiledSampleLoop {
            start_frame: loop_start_frame,
            end_frame: loop_end_frame,
            crossfade_frames,
        })
    } else {
        None
    };
    Some(CompiledSamplePlayback {
        direction: match zone.playback.direction {
            SamplePlaybackDirection::Forward => CompiledSampleDirection::Forward,
            SamplePlaybackDirection::Reverse => CompiledSampleDirection::Reverse,
        },
        start_frame,
        end_frame,
        loop_region,
        time: match zone.playback.time {
            SampleTimeDefinition::Resample => CompiledSampleTime::Resample,
            SampleTimeDefinition::FixedStretch { ratio } => CompiledSampleTime::FixedStretch {
                duration_ratio: f64::from(ratio),
            },
            SampleTimeDefinition::TempoSync { source_bpm } => CompiledSampleTime::TempoSync {
                source_bpm: f64::from(source_bpm),
            },
        },
    })
}

fn sample_time_to_frame(
    seconds: f32,
    sample_rate: f64,
    path: &str,
    field: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<usize> {
    let frames = (f64::from(seconds) * sample_rate).round();
    #[allow(clippy::cast_precision_loss)]
    let max_usize = usize::MAX as f64;
    if !frames.is_finite() || frames < 0.0 || frames > max_usize {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::CompileError,
                "sample playback time does not fit in the process frame counter",
            )
            .with_path(format!("{path}.{field}")),
        );
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(frames as usize)
}

fn granular_time_to_frame(
    seconds: f32,
    sample_rate: f64,
    path: &str,
    field: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<usize> {
    let frames = (f64::from(seconds) * sample_rate).round();
    #[allow(clippy::cast_precision_loss)]
    let max_usize = usize::MAX as f64;
    if !frames.is_finite() || frames < 0.0 || frames > max_usize {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::InvalidGrainRegion,
                "granular region time does not fit in the process frame counter",
            )
            .with_path(format!("{path}.{field}")),
        );
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(frames as usize)
}

fn compile_unison(unison: Option<UnisonDefinition>) -> CompiledUnison {
    let (position_distribution, phase_spread) = unison.map_or((vec![0.0], 0.0), |value| {
        let voices = usize::from(value.voices);
        let distribution = if voices > 1 {
            #[allow(clippy::cast_precision_loss)]
            let denominator = (voices - 1) as f32;
            (0..voices)
                .map(|index| {
                    #[allow(clippy::cast_precision_loss)]
                    let index = index as f32;
                    -1.0 + 2.0 * index / denominator
                })
                .collect()
        } else {
            vec![0.0]
        };
        (distribution, value.phase_spread)
    });
    let voices = position_distribution.len();
    #[allow(clippy::cast_precision_loss)]
    let phase_distribution = (0..voices)
        .map(|index| index as f32 / voices.max(1) as f32 * phase_spread)
        .collect();
    #[allow(clippy::cast_precision_loss)]
    let normalization = 1.0 / (voices.max(1) as f32).sqrt();
    CompiledUnison {
        position_distribution: position_distribution.into_boxed_slice(),
        phase_distribution,
        phase_spread,
        normalization,
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{context, definition};
    use super::{CompiledGenerator, GeneratorOutputMode, build_sine_table};
    use crate::compile_instrument;
    use crate::definition::{
        GeneratorDefinition, NoiseColor, OscillatorDefinition, OscillatorWaveform,
    };

    #[test]
    fn sine_table_has_the_expected_periodic_samples() {
        let table = build_sine_table();

        assert_eq!(table.len(), 4_097);
        assert_eq!(table[0].to_bits(), 0.0_f32.to_bits());
        assert!((table[1_024] - 1.0).abs() < 1.0e-6);
        assert!(table[2_048].abs() < 1.0e-6);
        assert!((table[3_072] + 1.0).abs() < 1.0e-6);
        assert!(table[4_096].abs() < 1.0e-6);
    }

    #[test]
    fn basic_generators_compile_with_parameter_handles_and_output_modes() {
        let mut pulse_definition = definition();
        pulse_definition.layers[0].generator =
            GeneratorDefinition::Oscillator(OscillatorDefinition {
                waveform: OscillatorWaveform::Pulse { pulse_width: 0.3 },
                phase_reset: true,
                phase: 0.25,
                hard_sync: None,
                waveshaping: None,
                phase_distortion: None,
                wavefold: None,
                feedback: None,
                unison: None,
            });
        let pulse = compile_instrument(&pulse_definition, &context())
            .instrument
            .expect("pulse compiles");
        let CompiledGenerator::Oscillator(pulse_oscillator) = &pulse.layers[0].generator else {
            panic!("pulse definition must compile to an oscillator");
        };
        assert_eq!(
            pulse.layers[0].generator.output_mode(),
            GeneratorOutputMode::Mono
        );
        assert!((pulse_oscillator.phase - 0.25).abs() < f32::EPSILON);
        assert!(pulse_oscillator.parameters.pulse_width.is_some());
        assert!(
            pulse
                .parameter_handle("layer.body.generator.pulse_width")
                .is_some()
        );

        let mut noise_definition = definition();
        noise_definition.layers[0].generator =
            GeneratorDefinition::Noise(crate::definition::NoiseDefinition {
                color: NoiseColor::Brown,
                seed: 42,
                stereo_correlation: 0.5,
            });
        let noise = compile_instrument(&noise_definition, &context())
            .instrument
            .expect("noise compiles");
        let CompiledGenerator::Noise(compiled_noise) = &noise.layers[0].generator else {
            panic!("noise definition must compile to noise");
        };
        assert_eq!(
            noise.layers[0].generator.output_mode(),
            GeneratorOutputMode::Stereo
        );
        assert_eq!(compiled_noise.color, NoiseColor::Brown);
        assert_eq!(compiled_noise.seed, 42);
        assert!(compiled_noise.brown_coefficient > 0.0);
        assert!(
            noise
                .parameter_handle("layer.body.generator.noise_correlation")
                .is_some()
        );
    }

    #[test]
    fn disabled_layers_stay_in_the_catalog_but_not_the_runtime_layers() {
        let mut source = definition();
        let mut disabled = source.layers[0].clone();
        disabled.id = "disabled".to_owned();
        disabled.enabled = false;
        source.layers.insert(0, disabled);
        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("enabled layer compiles");
        assert_eq!(compiled.layers.len(), 1);
        assert_eq!(compiled.parameters()[0].id, "layer.disabled.gain");
        assert_eq!(compiled.layers[0].id, "body");
    }
}
