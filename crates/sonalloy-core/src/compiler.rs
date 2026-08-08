use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::asset::{
    AssetError, PreparedAsset, PreparedAudio, PreparedAudioChannels, prepare_asset,
    resolved_asset_path,
};
use crate::definition::{
    AdsrDefinition, DelayProcessorDefinition, DriveProcessorDefinition, FilterProcessorDefinition,
    GeneratorDefinition, GranularDefinition, InstrumentDefinition, LayerTriggerEvent,
    LfoDefinition, LfoWaveform, ModulationCurve, ModulationSourceDefinition, NoiseColor,
    OperatorAlgorithm, OperatorModulationDefinition, OperatorModulationMode, OscillatorDefinition,
    OscillatorWaveform, ProcessorDefinition, ReverbProcessorDefinition, SamplePlaybackDirection,
    SampleTimeDefinition, SampleZoneDefinition, UnisonDefinition, VoiceStealingDefinition,
    WavetableDefinition,
};
use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticSeverity};
use crate::generator_parameters::{
    BASIC_FREQUENCY_LIMIT_RATIO, GRAIN_DENSITY, GRAIN_PAN_SPREAD, GRAIN_PITCH, GRAIN_RANDOMNESS,
    GRAIN_SIZE, GRANULAR_GRAIN_POOL_LIMIT, GRANULAR_POSITION, GeneratorParameterSpec,
    NOISE_CORRELATION, OSCILLATOR_FEEDBACK, PHASE_DISTORTION, PHASE_DOMAIN_FREQUENCY_LIMIT_RATIO,
    PULSE_WIDTH, SYNC_RATIO, UNISON_DETUNE, UNISON_SPREAD, WAVEFOLD, WAVESHAPE, WAVETABLE_POSITION,
    effective_max_frequency,
};
use crate::parameter::{BUILTIN_SOURCE_IDS, ParameterCatalog, ParameterHandle, ParameterOwner};
use crate::process::ProcessSpec;
use crate::runtime::InstrumentRuntime;
use crate::wavetable::{
    WavetablePreparation, WavetablePreparationError, WavetableWarning, prepare_wavetable_asset,
};

/// Input required to compile a Definition for one engine configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct CompileContext {
    /// Directory used to resolve referenced assets.
    pub definition_base_dir: PathBuf,
    /// Engine sample rate and block configuration.
    pub process_spec: ProcessSpec,
}

/// Result of compiling an instrument.
#[derive(Debug, Clone)]
pub struct CompileResult {
    /// Immutable compiled instrument when no error diagnostics were produced.
    pub instrument: Option<Arc<CompiledInstrument>>,
    /// Errors and warnings collected during compilation.
    pub diagnostics: Vec<Diagnostic>,
}

/// Runtime-independent instrument configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledInstrument {
    /// Sample rate used to compile sample and time-dependent values.
    pub process_sample_rate: f64,
    /// Maximum intrinsic latency reported by the compiled layers.
    pub reported_latency_frames: usize,
    /// Metadata copied from the Definition.
    pub metadata: CompiledMetadata,
    /// Compiled performance settings.
    pub performance: CompiledPerformance,
    /// Enabled layers in Definition order.
    pub layers: Box<[CompiledLayer]>,
    /// Processors applied after the layer mix for each voice.
    pub voice_processors: Box<[CompiledProcessor]>,
    /// Processors applied after the voice sum for the instrument.
    pub global_processors: Box<[CompiledProcessor]>,
    /// Dense continuous parameter catalog.
    pub parameter_catalog: ParameterCatalog,
    /// Voice-scoped source table.
    pub sources: Box<[CompiledSource]>,
    /// Compiled routes grouped by target handle.
    pub routes: Box<[CompiledRoute]>,
    /// Route range for each parameter handle.
    pub route_ranges: Box<[RouteRange]>,
    /// Warnings retained for inspection and review output.
    pub diagnostics: Box<[Diagnostic]>,
}

impl CompiledInstrument {
    /// Create a fresh runtime instance that owns no active audio state yet.
    #[must_use]
    pub fn instantiate(self: &Arc<Self>) -> InstrumentRuntime {
        InstrumentRuntime::new(Arc::clone(self))
    }

    /// Return parameter descriptors in stable Definition order.
    #[must_use]
    pub fn parameters(&self) -> &[crate::parameter::ParameterDescriptor] {
        self.parameter_catalog.parameters()
    }

    /// Resolve a canonical parameter identifier for control code.
    #[must_use]
    pub fn parameter_handle(&self, id: &str) -> Option<ParameterHandle> {
        self.parameter_catalog.parameter_handle(id)
    }

    /// Return one descriptor by handle.
    #[must_use]
    pub fn parameter_descriptor(
        &self,
        handle: ParameterHandle,
    ) -> Option<&crate::parameter::ParameterDescriptor> {
        self.parameter_catalog.descriptor(handle)
    }

    /// Return the route slice for one target handle.
    #[must_use]
    pub fn routes_for(&self, handle: ParameterHandle) -> &[CompiledRoute] {
        let Some(range) = self.route_ranges.get(handle.index()) else {
            return &[];
        };
        let Some(end) = range.start.checked_add(range.len) else {
            return &[];
        };
        self.routes.get(range.start..end).unwrap_or(&[])
    }

    pub(crate) fn routes_for_checked(&self, handle: ParameterHandle) -> Option<&[CompiledRoute]> {
        let range = self.route_ranges.get(handle.index())?;
        let end = range.start.checked_add(range.len)?;
        self.routes.get(range.start..end)
    }
}

/// Compiled metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledMetadata {
    /// Instrument name.
    pub name: String,
    /// Optional author.
    pub author: Option<String>,
    /// Optional description.
    pub description: Option<String>,
}

/// Compiled performance settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledPerformance {
    /// Maximum number of voices.
    pub polyphony: usize,
    /// Voice allocation policy.
    pub voice_stealing: CompiledVoiceStealing,
}

/// Compiled voice allocation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledVoiceStealing {
    /// Prefer quiet releasing voices, then oldest active voices.
    QuietestReleasingThenOldest,
}

/// Parameter handles used by one compiled layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledLayerParameters {
    /// Layer gain handle.
    pub gain: ParameterHandle,
    /// Layer pan handle.
    pub pan: ParameterHandle,
    /// Layer tuning handle.
    pub tuning: ParameterHandle,
}

/// Compiled layer configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledLayer {
    /// Stable layer identifier.
    pub id: String,
    /// Compiled trigger conditions.
    pub trigger: CompiledLayerTrigger,
    /// Runtime parameter bindings.
    pub parameters: CompiledLayerParameters,
    /// Sample-rate-specific envelope.
    pub envelope: CompiledAdsr,
    /// Compiled generator.
    pub generator: CompiledGenerator,
    /// Latency introduced by the layer's generator.
    pub intrinsic_latency_frames: usize,
    /// Processors applied after the generator.
    pub processors: Box<[CompiledProcessor]>,
}

/// Compiled trigger conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledLayerTrigger {
    /// Event at which the layer starts.
    pub event: LayerTriggerEvent,
    /// Lowest accepted MIDI note.
    pub key_min: u8,
    /// Highest accepted MIDI note.
    pub key_max: u8,
    /// Lowest accepted velocity.
    pub velocity_min: u8,
    /// Highest accepted velocity.
    pub velocity_max: u8,
}

impl CompiledLayerTrigger {
    pub(crate) fn matches(self, note_number: u8, velocity: u8) -> bool {
        (self.key_min..=self.key_max).contains(&note_number)
            && (self.velocity_min..=self.velocity_max).contains(&velocity)
    }
}

/// Compiled generator variants.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum CompiledGenerator {
    /// Oscillator generator.
    Oscillator(CompiledOscillator),
    /// Noise generator.
    Noise(CompiledNoise),
    /// Prepared sample generator.
    Sample(CompiledSample),
    /// Prepared granular generator.
    Granular(CompiledGranular),
    /// Prepared band-limited wavetable generator.
    Wavetable(CompiledWavetable),
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
            Self::Oscillator(value) => {
                if value.unison.position_distribution.len() == 1 {
                    GeneratorOutputMode::Mono
                } else {
                    GeneratorOutputMode::Stereo
                }
            }
            Self::Noise(_) | Self::Granular(_) => GeneratorOutputMode::Stereo,
            Self::Sample(value) => value.output_mode(),
            Self::Wavetable(value) => {
                if value.unison.position_distribution.len() == 1 {
                    GeneratorOutputMode::Mono
                } else {
                    GeneratorOutputMode::Stereo
                }
            }
            Self::OperatorModulation(value) => {
                if value.unison.position_distribution.len() == 1 {
                    GeneratorOutputMode::Mono
                } else {
                    GeneratorOutputMode::Stereo
                }
            }
        }
    }

    /// Return the intrinsic latency introduced by this generator.
    #[must_use]
    pub fn intrinsic_latency_frames(&self) -> usize {
        match self {
            Self::Sample(value) => value
                .stretch_latency
                .map_or(0, |latency| latency.output_frames),
            Self::Oscillator(_)
            | Self::Noise(_)
            | Self::Granular(_)
            | Self::Wavetable(_)
            | Self::OperatorModulation(_) => 0,
        }
    }

    pub(crate) fn is_available(&self) -> bool {
        match self {
            Self::Oscillator(_) | Self::Noise(_) | Self::OperatorModulation(_) => true,
            Self::Granular(value) => value.source.is_some(),
            Self::Sample(value) => value.zones.iter().any(CompiledSampleZone::is_enabled),
            Self::Wavetable(value) => value.prepared.is_some(),
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

/// Parameter handles used by a filter processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledFilterParameters {
    /// Cutoff handle.
    pub cutoff: ParameterHandle,
    /// Resonance handle.
    pub resonance: ParameterHandle,
}

/// Compiled filter processor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledFilterProcessor {
    /// Runtime parameter bindings.
    pub parameters: CompiledFilterParameters,
    /// Safe DSP cutoff upper bound for this process sample rate.
    pub effective_max_cutoff_hz: f32,
}

/// Compiled drive processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledDriveProcessor {
    /// Amount handle.
    pub amount: ParameterHandle,
    /// Mix handle.
    pub mix: ParameterHandle,
}

/// Compiled delay processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledDelayProcessor {
    /// Integer delay length in frames.
    pub delay_frames: usize,
    /// Feedback handle.
    pub feedback: ParameterHandle,
    /// Mix handle.
    pub mix: ParameterHandle,
}

/// Delay-line source used by one Dattorro stereo output tap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReverbTapSource {
    /// The first long delay in the left tank.
    LeftLongDelay,
    /// The second all-pass delay in the left tank.
    LeftTankAllpass,
    /// The final output delay in the left tank.
    LeftOutputDelay,
    /// The first long delay in the right tank.
    RightLongDelay,
    /// The second all-pass delay in the right tank.
    RightTankAllpass,
    /// The final output delay in the right tank.
    RightOutputDelay,
}

/// One sample-rate-scaled Dattorro output tap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReverbOutputTap {
    /// Delay-line containing the tap source.
    pub source: ReverbTapSource,
    /// Offset into that delay-line.
    pub delay_frames: usize,
    /// Signed contribution to the output accumulator.
    pub sign: i8,
}

/// Compiled plate reverb processor.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledReverbProcessor {
    /// Static pre-delay in frames.
    pub pre_delay_frames: usize,
    /// Decay handle.
    pub decay: ParameterHandle,
    /// Damping handle.
    pub damping: ParameterHandle,
    /// Width handle.
    pub width: ParameterHandle,
    /// Mix handle.
    pub mix: ParameterHandle,
    /// Input diffusion delay lengths.
    pub input_diffusion_lengths: [usize; 4],
    /// Left tank delay lengths.
    pub tank_left_lengths: [usize; 4],
    /// Right tank delay lengths.
    pub tank_right_lengths: [usize; 4],
    /// Left stereo output taps.
    pub left_output_taps: [ReverbOutputTap; 7],
    /// Right stereo output taps.
    pub right_output_taps: [ReverbOutputTap; 7],
    /// Per-sample internal modulation phase increment.
    pub modulation_increment: f32,
    /// Sample-rate-scaled maximum modulation excursion in frames.
    pub modulation_excursion: f32,
}

/// Processor kind with all control bindings resolved.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum CompiledProcessorKind {
    /// Low-pass filter processor.
    Filter(CompiledFilterProcessor),
    /// Soft-clipping drive processor.
    Drive(CompiledDriveProcessor),
    /// Stereo delay processor.
    Delay(CompiledDelayProcessor),
    /// Stereo plate reverb processor.
    Reverb(CompiledReverbProcessor),
}

/// One processor in a Definition-ordered chain.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledProcessor {
    /// Stable processor identifier.
    pub id: String,
    /// Compiled processor kind.
    pub processor: CompiledProcessorKind,
}

/// Dense source handle for voice-scoped sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceHandle(usize);

impl SourceHandle {
    /// Return the dense source index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Compiled voice source.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSource {
    /// Stable source identifier.
    pub id: String,
    /// Compiled source behavior.
    pub source: CompiledVoiceSource,
}

/// Compiled voice source behavior.
#[derive(Debug, Clone, PartialEq)]
pub enum CompiledVoiceSource {
    /// Built-in note velocity source.
    Velocity,
    /// Built-in key tracking source.
    KeyTracking,
    /// Periodic low-frequency oscillator.
    Lfo(CompiledLfo),
    /// Note lifecycle envelope.
    Envelope(CompiledModEnvelope),
    /// Note-scoped deterministic random value.
    Random(CompiledRandom),
}

/// Compiled LFO settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledLfo {
    /// LFO waveform.
    pub waveform: LfoWaveform,
    /// Rate in hertz.
    pub rate_hz: f32,
    /// Initial phase.
    pub phase: f32,
}

/// Compiled modulation envelope settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledModEnvelope {
    /// Sample-rate-specific envelope.
    pub envelope: CompiledAdsr,
}

/// Compiled deterministic random settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledRandom {
    /// Explicit source seed.
    pub seed: u64,
    /// FNV-1a hash of the stable source identifier, precomputed off the audio path.
    pub source_hash: u64,
}

/// A source reference in a compiled route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledSourceRef {
    /// Voice-scoped source table entry.
    Voice(SourceHandle),
    /// Shared pitch bend control.
    PitchBend,
    /// Shared modulation wheel control.
    ModWheel,
    /// Shared channel aftertouch control.
    Aftertouch,
}

/// Compiled route with a fixed target and source evaluation order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledRoute {
    /// Compiled source reference.
    pub source: CompiledSourceRef,
    /// Target parameter handle.
    pub target: ParameterHandle,
    /// Signed target-range amount.
    pub amount: f32,
    /// Source curve.
    pub curve: ModulationCurve,
}

/// Contiguous route range for one target handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteRange {
    /// First route index.
    pub start: usize,
    /// Number of routes.
    pub len: usize,
}

/// Compile a validated Definition for a process configuration.
///
/// # Panics
///
/// Panics only if the parameter catalog does not contain a parameter generated
/// from the same Definition. Validation and catalog construction keep those
/// entries synchronized.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn compile_instrument(
    definition: &InstrumentDefinition,
    context: &CompileContext,
) -> CompileResult {
    let mut diagnostics = definition.validate();
    if let Err(error) = context.process_spec.validate() {
        diagnostics.push(
            Diagnostic::error(DiagnosticCode::CompileError, error.to_string())
                .with_path("process_spec"),
        );
    }
    if has_errors(&diagnostics) {
        return CompileResult {
            instrument: None,
            diagnostics,
        };
    }

    let parameter_catalog = ParameterCatalog::from_definition(definition);
    let mut asset_cache = HashMap::new();
    let mut wavetable_asset_cache = HashMap::new();

    let performance = CompiledPerformance {
        polyphony: usize::from(definition.performance.polyphony),
        voice_stealing: match definition.performance.voice_stealing {
            VoiceStealingDefinition::QuietestReleasingThenOldest => {
                CompiledVoiceStealing::QuietestReleasingThenOldest
            }
        },
    };
    let layers = definition
        .layers
        .iter()
        .enumerate()
        .filter(|(_, layer)| layer.enabled)
        .map(|(definition_index, layer)| {
            let generator = compile_generator(
                &layer.generator,
                definition_index,
                &layer.id,
                &parameter_catalog,
                &context.definition_base_dir,
                context.process_spec.sample_rate,
                context.process_spec.max_block_size,
                &mut asset_cache,
                &mut wavetable_asset_cache,
                &mut diagnostics,
            );
            let envelope_path = format!("layers[{definition_index}].envelope");
            let envelope = compile_adsr(
                layer.envelope,
                context.process_spec.sample_rate,
                &envelope_path,
                &mut diagnostics,
            );
            let parameters = CompiledLayerParameters {
                gain: parameter_catalog
                    .parameter_handle(&format!("layer.{}.gain", layer.id))
                    .expect("layer gain catalog entry exists"),
                pan: parameter_catalog
                    .parameter_handle(&format!("layer.{}.pan", layer.id))
                    .expect("layer pan catalog entry exists"),
                tuning: parameter_catalog
                    .parameter_handle(&format!("layer.{}.tuning", layer.id))
                    .expect("layer tuning catalog entry exists"),
            };
            let processors = compile_processor_chain(
                &layer.processors,
                ProcessorPlacement::Layer,
                Some(&layer.id),
                &format!("layers[{definition_index}].processors"),
                &parameter_catalog,
                context.process_spec.sample_rate,
                &mut diagnostics,
            );
            let intrinsic_latency_frames = generator.intrinsic_latency_frames();
            CompiledLayer {
                id: layer.id.clone(),
                trigger: compile_trigger(layer.trigger),
                parameters,
                envelope,
                generator,
                intrinsic_latency_frames,
                processors,
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();

    let voice_processors = compile_processor_chain(
        &definition.voice_processors,
        ProcessorPlacement::Voice,
        None,
        "voice_processors",
        &parameter_catalog,
        context.process_spec.sample_rate,
        &mut diagnostics,
    );
    let global_processors = compile_processor_chain(
        &definition.global_processors,
        ProcessorPlacement::Global,
        None,
        "global_processors",
        &parameter_catalog,
        context.process_spec.sample_rate,
        &mut diagnostics,
    );

    let (sources, routes, route_ranges) = compile_modulation(
        definition,
        &parameter_catalog,
        context.process_spec.sample_rate,
        &mut diagnostics,
    );
    if has_errors(&diagnostics) {
        return CompileResult {
            instrument: None,
            diagnostics,
        };
    }

    let compiled = CompiledInstrument {
        process_sample_rate: context.process_spec.sample_rate,
        reported_latency_frames: layers
            .iter()
            .map(|layer| layer.intrinsic_latency_frames)
            .max()
            .unwrap_or(0),
        metadata: CompiledMetadata {
            name: definition.metadata.name.clone(),
            author: definition.metadata.author.clone(),
            description: definition.metadata.description.clone(),
        },
        performance,
        layers,
        voice_processors,
        global_processors,
        parameter_catalog,
        sources,
        routes,
        route_ranges,
        diagnostics: diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
            .cloned()
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    };
    CompileResult {
        instrument: Some(Arc::new(compiled)),
        diagnostics,
    }
}

#[derive(Clone, Copy)]
enum ProcessorPlacement {
    Layer,
    Voice,
    Global,
}

fn compile_processor_chain(
    processors: &[ProcessorDefinition],
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    base_path: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> Box<[CompiledProcessor]> {
    processors
        .iter()
        .enumerate()
        .map(|(index, processor)| {
            compile_processor(
                processor,
                placement,
                layer_id,
                &format!("{base_path}[{index}]"),
                catalog,
                sample_rate,
                diagnostics,
            )
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn compile_processor(
    processor: &ProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    path: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledProcessor {
    let id = processor.id().to_owned();
    let processor = match processor {
        ProcessorDefinition::Filter(value) => compile_filter_processor(
            value,
            placement,
            layer_id,
            path,
            catalog,
            sample_rate,
            diagnostics,
        ),
        ProcessorDefinition::Drive(value) => {
            compile_drive_processor(value, placement, layer_id, catalog)
        }
        ProcessorDefinition::Delay(value) => compile_delay_processor(
            value,
            placement,
            layer_id,
            path,
            catalog,
            sample_rate,
            diagnostics,
        ),
        ProcessorDefinition::Reverb(value) => compile_reverb_processor(
            value,
            placement,
            layer_id,
            path,
            catalog,
            sample_rate,
            diagnostics,
        ),
    };
    CompiledProcessor { id, processor }
}

fn processor_parameter_handle(
    catalog: &ParameterCatalog,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    processor_id: &str,
    parameter: &str,
) -> ParameterHandle {
    let id = processor_parameter_id(placement, layer_id, processor_id, parameter);
    catalog
        .parameter_handle(&id)
        .expect("processor parameter catalog entry exists")
}

fn compile_filter_processor(
    value: &FilterProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    path: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledProcessorKind {
    let cutoff = processor_parameter_handle(catalog, placement, layer_id, &value.id, "cutoff");
    let resonance =
        processor_parameter_handle(catalog, placement, layer_id, &value.id, "resonance");
    let effective_max_cutoff_hz = effective_max_cutoff(sample_rate);
    if value.cutoff_hz > effective_max_cutoff_hz {
        diagnostics.push(
            Diagnostic::warning(
                DiagnosticCode::FilterCutoffClamped,
                format!(
                    "cutoff exceeds the process-safe maximum and will be clamped to {effective_max_cutoff_hz:.3} Hz during DSP processing"
                ),
            )
            .with_path(format!("{path}.cutoff_hz")),
        );
    }
    CompiledProcessorKind::Filter(CompiledFilterProcessor {
        parameters: CompiledFilterParameters { cutoff, resonance },
        effective_max_cutoff_hz,
    })
}

fn compile_drive_processor(
    value: &DriveProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    catalog: &ParameterCatalog,
) -> CompiledProcessorKind {
    let amount = processor_parameter_handle(catalog, placement, layer_id, &value.id, "amount");
    let mix = processor_parameter_handle(catalog, placement, layer_id, &value.id, "mix");
    CompiledProcessorKind::Drive(CompiledDriveProcessor { amount, mix })
}

fn compile_delay_processor(
    value: &DelayProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    path: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledProcessorKind {
    let feedback = processor_parameter_handle(catalog, placement, layer_id, &value.id, "feedback");
    let mix = processor_parameter_handle(catalog, placement, layer_id, &value.id, "mix");
    let delay_frames = processor_seconds_to_frames(
        value.time_seconds,
        sample_rate,
        &format!("{path}.time_seconds"),
        diagnostics,
        1,
    );
    CompiledProcessorKind::Delay(CompiledDelayProcessor {
        delay_frames,
        feedback,
        mix,
    })
}

fn compile_reverb_processor(
    value: &ReverbProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    path: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledProcessorKind {
    let decay = processor_parameter_handle(catalog, placement, layer_id, &value.id, "decay");
    let damping = processor_parameter_handle(catalog, placement, layer_id, &value.id, "damping");
    let width = processor_parameter_handle(catalog, placement, layer_id, &value.id, "width");
    let mix = processor_parameter_handle(catalog, placement, layer_id, &value.id, "mix");
    let pre_delay_frames = processor_seconds_to_frames(
        value.pre_delay_seconds,
        sample_rate,
        &format!("{path}.pre_delay_seconds"),
        diagnostics,
        0,
    );
    let input_diffusion_lengths = scale_reverb_lengths(
        [142.0, 107.0, 379.0, 277.0],
        sample_rate,
        &format!("{path}.input_diffusion"),
        diagnostics,
    );
    let tank_left_lengths = scale_reverb_lengths(
        [672.0, 4_453.0, 1_800.0, 3_720.0],
        sample_rate,
        &format!("{path}.tank_left"),
        diagnostics,
    );
    let tank_right_lengths = scale_reverb_lengths(
        [908.0, 4_217.0, 2_656.0, 3_163.0],
        sample_rate,
        &format!("{path}.tank_right"),
        diagnostics,
    );
    let left_output_taps = scale_reverb_taps(
        [
            (ReverbTapSource::RightLongDelay, 266.0, 1),
            (ReverbTapSource::RightLongDelay, 2_974.0, 1),
            (ReverbTapSource::RightTankAllpass, 1_913.0, -1),
            (ReverbTapSource::RightOutputDelay, 1_996.0, 1),
            (ReverbTapSource::LeftLongDelay, 1_990.0, -1),
            (ReverbTapSource::LeftTankAllpass, 187.0, -1),
            (ReverbTapSource::LeftOutputDelay, 1_066.0, -1),
        ],
        sample_rate,
        &format!("{path}.left_output_taps"),
        diagnostics,
    );
    let right_output_taps = scale_reverb_taps(
        [
            (ReverbTapSource::LeftLongDelay, 353.0, 1),
            (ReverbTapSource::LeftLongDelay, 3_627.0, 1),
            (ReverbTapSource::LeftTankAllpass, 1_228.0, -1),
            (ReverbTapSource::LeftOutputDelay, 2_673.0, 1),
            (ReverbTapSource::RightLongDelay, 2_111.0, -1),
            (ReverbTapSource::RightTankAllpass, 335.0, -1),
            (ReverbTapSource::RightOutputDelay, 121.0, -1),
        ],
        sample_rate,
        &format!("{path}.right_output_taps"),
        diagnostics,
    );
    #[allow(clippy::cast_possible_truncation)]
    let modulation_increment = (1.0 / sample_rate) as f32;
    let modulation_excursion = scale_reverb_excursion(
        16.0,
        sample_rate,
        &format!("{path}.modulation_excursion"),
        diagnostics,
    );
    CompiledProcessorKind::Reverb(CompiledReverbProcessor {
        pre_delay_frames,
        decay,
        damping,
        width,
        mix,
        input_diffusion_lengths,
        tank_left_lengths,
        tank_right_lengths,
        left_output_taps,
        right_output_taps,
        modulation_increment,
        modulation_excursion,
    })
}

fn processor_parameter_id(
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    processor_id: &str,
    parameter: &str,
) -> String {
    match placement {
        ProcessorPlacement::Layer => format!(
            "layer.{}.processor.{processor_id}.{parameter}",
            layer_id.expect("layer processor has a layer id")
        ),
        ProcessorPlacement::Voice => format!("voice.processor.{processor_id}.{parameter}"),
        ProcessorPlacement::Global => format!("global.processor.{processor_id}.{parameter}"),
    }
}

fn processor_seconds_to_frames(
    seconds: f32,
    sample_rate: f64,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
    minimum_frames: usize,
) -> usize {
    let frames = (f64::from(seconds) * sample_rate).round();
    #[allow(clippy::cast_precision_loss)]
    let max_usize = usize::MAX as f64;
    if !frames.is_finite() || frames < 0.0 || frames > max_usize {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::CompileError,
                "processor duration does not fit in the process frame counter",
            )
            .with_path(path),
        );
        return 1;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    {
        frames.max(minimum_frames as f64) as usize
    }
}

fn scale_reverb_lengths<const N: usize>(
    reference_lengths: [f64; N],
    sample_rate: f64,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> [usize; N] {
    let mut result = [1_usize; N];
    for (index, reference) in reference_lengths.into_iter().enumerate() {
        let seconds = reference / 29_761.0;
        let frames = (seconds * sample_rate).round();
        #[allow(clippy::cast_precision_loss)]
        let max_usize = usize::MAX as f64;
        if !frames.is_finite() || frames < 1.0 || frames > max_usize {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::CompileError,
                    "reverb delay length does not fit in the process frame counter",
                )
                .with_path(format!("{path}[{index}]")),
            );
            continue;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            result[index] = frames as usize;
        }
    }
    result
}

fn scale_reverb_taps<const N: usize>(
    reference_taps: [(ReverbTapSource, f64, i8); N],
    sample_rate: f64,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> [ReverbOutputTap; N] {
    let reference_lengths: [f64; N] = std::array::from_fn(|index| reference_taps[index].1);
    let lengths = scale_reverb_lengths(reference_lengths, sample_rate, path, diagnostics);
    std::array::from_fn(|index| {
        let (source, _, sign) = reference_taps[index];
        ReverbOutputTap {
            source,
            delay_frames: lengths[index],
            sign,
        }
    })
}

fn scale_reverb_excursion(
    reference_frames: f64,
    sample_rate: f64,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> f32 {
    let frames = reference_frames / 29_761.0 * sample_rate;
    if !frames.is_finite() || frames <= 0.0 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::CompileError,
                "reverb modulation excursion is not finite",
            )
            .with_path(path),
        );
        return 1.0;
    }
    #[allow(clippy::cast_possible_truncation)]
    {
        frames as f32
    }
}

type CompiledModulation = (
    Box<[CompiledSource]>,
    Box<[CompiledRoute]>,
    Box<[RouteRange]>,
);

#[allow(clippy::too_many_lines)]
fn compile_modulation(
    definition: &InstrumentDefinition,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledModulation {
    let mut sources = Vec::with_capacity(BUILTIN_SOURCE_IDS.len());
    sources.push(CompiledSource {
        id: "velocity".to_owned(),
        source: CompiledVoiceSource::Velocity,
    });
    sources.push(CompiledSource {
        id: "key_tracking".to_owned(),
        source: CompiledVoiceSource::KeyTracking,
    });
    let mut source_lookup = HashMap::new();
    for (index, source) in sources.iter().enumerate() {
        source_lookup.insert(source.id.clone(), SourceHandle(index));
    }
    if let Some(modulation) = &definition.modulation {
        for (source_index, source) in modulation.sources.iter().enumerate() {
            let compiled = match source {
                ModulationSourceDefinition::Lfo(value) => {
                    CompiledVoiceSource::Lfo(compile_lfo(value))
                }
                ModulationSourceDefinition::Envelope(value) => {
                    let envelope_path = format!("modulation.sources[{source_index}]");
                    CompiledVoiceSource::Envelope(CompiledModEnvelope {
                        envelope: compile_adsr(
                            AdsrDefinition {
                                attack_seconds: value.attack_seconds,
                                decay_seconds: value.decay_seconds,
                                sustain_level: value.sustain_level,
                                release_seconds: value.release_seconds,
                            },
                            sample_rate,
                            &envelope_path,
                            diagnostics,
                        ),
                    })
                }
                ModulationSourceDefinition::Random(value) => {
                    CompiledVoiceSource::Random(CompiledRandom {
                        seed: value.seed,
                        source_hash: source_id_hash(&value.id),
                    })
                }
            };
            let handle = SourceHandle(sources.len());
            source_lookup.insert(source_id(source).to_owned(), handle);
            sources.push(CompiledSource {
                id: source_id(source).to_owned(),
                source: compiled,
            });
        }
    }

    let mut unresolved_routes = Vec::new();
    if let Some(modulation) = &definition.modulation {
        for (index, route) in modulation.routes.iter().enumerate() {
            let Some(target) = catalog.parameter_handle(&route.target) else {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ParameterNotFound,
                        "route target does not name a continuous parameter",
                    )
                    .with_path(format!("modulation.routes[{index}].target")),
                );
                continue;
            };
            let source = match route.source.as_str() {
                "pitch_bend" => Some(CompiledSourceRef::PitchBend),
                "mod_wheel" => Some(CompiledSourceRef::ModWheel),
                "aftertouch" => Some(CompiledSourceRef::Aftertouch),
                _ => source_lookup
                    .get(&route.source)
                    .copied()
                    .map(CompiledSourceRef::Voice),
            };
            let Some(source) = source else {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::SourceNotFound,
                        "route source is not defined",
                    )
                    .with_path(format!("modulation.routes[{index}].source")),
                );
                continue;
            };
            let owner = catalog
                .descriptor(target)
                .expect("compiled route target handle must be valid")
                .owner;
            if !route_source_allowed(owner, source) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::GlobalRouteScopeInvalid,
                        "global processor targets accept only shared external controls",
                    )
                    .with_path(format!("modulation.routes[{index}].source")),
                );
                continue;
            }
            unresolved_routes.push((
                target.index(),
                CompiledRoute {
                    source,
                    target,
                    amount: route.amount,
                    curve: route.curve,
                },
            ));
        }
    }
    unresolved_routes.sort_by_key(|(target, _)| *target);
    let mut routes = Vec::with_capacity(unresolved_routes.len());
    let mut route_ranges = vec![RouteRange { start: 0, len: 0 }; catalog.len()];
    let mut index = 0;
    while index < unresolved_routes.len() {
        let target = unresolved_routes[index].0;
        let start = routes.len();
        while index < unresolved_routes.len() && unresolved_routes[index].0 == target {
            routes.push(unresolved_routes[index].1);
            index += 1;
        }
        route_ranges[target] = RouteRange {
            start,
            len: routes.len() - start,
        };
    }
    (
        sources.into_boxed_slice(),
        routes.into_boxed_slice(),
        route_ranges.into_boxed_slice(),
    )
}

fn route_source_allowed(owner: ParameterOwner, source: CompiledSourceRef) -> bool {
    match owner {
        ParameterOwner::GlobalProcessor { .. } => !matches!(source, CompiledSourceRef::Voice(_)),
        ParameterOwner::Layer { .. }
        | ParameterOwner::LayerGenerator { .. }
        | ParameterOwner::LayerProcessor { .. }
        | ParameterOwner::VoiceProcessor { .. } => true,
    }
}

fn source_id(source: &ModulationSourceDefinition) -> &str {
    match source {
        ModulationSourceDefinition::Lfo(value) => &value.id,
        ModulationSourceDefinition::Envelope(value) => &value.id,
        ModulationSourceDefinition::Random(value) => &value.id,
    }
}

pub(crate) fn source_id_hash(source_id: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in source_id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn compile_lfo(value: &LfoDefinition) -> CompiledLfo {
    CompiledLfo {
        waveform: value.waveform,
        rate_hz: value.rate_hz,
        phase: value.phase,
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn compile_generator(
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

fn generator_parameter_handle(
    catalog: &ParameterCatalog,
    layer_id: &str,
    spec: GeneratorParameterSpec,
) -> ParameterHandle {
    catalog
        .parameter_handle(&crate::parameter::layer_generator_parameter_id(
            layer_id,
            spec.suffix,
        ))
        .expect("generator parameter catalog entry exists")
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AssetCacheKey {
    path: PathBuf,
    sha256: Option<String>,
    sample_rate_bits: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WavetableAssetCacheKey {
    path: PathBuf,
    sha256: Option<String>,
    frame_length: usize,
}

fn prepare_cached_asset(
    reference: &crate::definition::AssetReference,
    definition_base_dir: &Path,
    sample_rate: f64,
    asset_cache: &mut HashMap<AssetCacheKey, Result<PreparedAsset, AssetError>>,
) -> Result<PreparedAsset, AssetError> {
    let resolved = resolved_asset_path(definition_base_dir, &reference.path);
    let path = std::fs::canonicalize(&resolved).unwrap_or(resolved);
    let key = AssetCacheKey {
        path,
        sha256: reference
            .sha256
            .as_ref()
            .map(|value| value.to_ascii_lowercase()),
        sample_rate_bits: sample_rate.to_bits(),
    };
    if let Some(result) = asset_cache.get(&key) {
        return result.clone();
    }
    let result = prepare_asset(reference, definition_base_dir, sample_rate);
    asset_cache.insert(key, result.clone());
    result
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

fn brown_noise_coefficient(sample_rate: f64) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let coefficient = (-std::f64::consts::TAU * 20.0 / sample_rate).exp() as f32;
    coefficient.clamp(0.0, 1.0)
}

fn asset_diagnostic(error: &AssetError) -> (DiagnosticCode, &'static str) {
    match error {
        AssetError::NotFound(_) => (DiagnosticCode::AssetNotFound, "asset is unavailable"),
        AssetError::HashMismatch { .. } => (
            DiagnosticCode::AssetHashMismatch,
            "asset sha256 does not match",
        ),
        AssetError::Decode(_) => (
            DiagnosticCode::AssetDecodeFailed,
            "asset could not be decoded",
        ),
        AssetError::Resample(_) => (
            DiagnosticCode::AssetDecodeFailed,
            "asset could not be prepared",
        ),
    }
}

fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
}

fn compile_trigger(trigger: crate::definition::LayerTriggerDefinition) -> CompiledLayerTrigger {
    CompiledLayerTrigger {
        event: trigger.event,
        key_min: trigger.key_min,
        key_max: trigger.key_max,
        velocity_min: trigger.velocity_min,
        velocity_max: trigger.velocity_max,
    }
}

fn compile_adsr(
    definition: AdsrDefinition,
    sample_rate: f64,
    base_path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledAdsr {
    CompiledAdsr {
        attack_samples: seconds_to_samples(
            definition.attack_seconds,
            sample_rate,
            base_path,
            "attack_seconds",
            diagnostics,
        ),
        decay_samples: seconds_to_samples(
            definition.decay_seconds,
            sample_rate,
            base_path,
            "decay_seconds",
            diagnostics,
        ),
        sustain_level: definition.sustain_level,
        release_samples: seconds_to_samples(
            definition.release_seconds,
            sample_rate,
            base_path,
            "release_seconds",
            diagnostics,
        ),
    }
}

fn seconds_to_samples(
    seconds: f32,
    sample_rate: f64,
    base_path: &str,
    field: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> usize {
    let frames = (f64::from(seconds) * sample_rate).round();
    #[allow(clippy::cast_precision_loss)]
    let max_usize = usize::MAX as f64;
    if !frames.is_finite() || frames < 0.0 || frames > max_usize {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::CompileError,
                "envelope duration does not fit in the process frame counter",
            )
            .with_path(format!("{base_path}.{field}")),
        );
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        frames as usize
    }
}

fn effective_max_cutoff(sample_rate: f64) -> f32 {
    #[allow(clippy::manual_clamp)]
    let max = (sample_rate * 0.45).min(20_000.0).max(1.0);
    #[allow(clippy::cast_possible_truncation)]
    {
        max as f32
    }
}

/// Convert decibels to linear amplitude.
#[must_use]
pub fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// Convert cents to a frequency ratio.
#[must_use]
pub fn cents_to_ratio(cents: f32) -> f32 {
    2.0_f32.powf(cents / 1200.0)
}

/// Convert a MIDI note and tuning ratio to frequency in Hz.
#[must_use]
pub fn midi_note_frequency(note_number: u8, tuning_ratio: f32) -> f32 {
    440.0 * 2.0_f32.powf((f32::from(note_number) - 69.0) / 12.0) * tuning_ratio
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::tests::definition;

    fn context() -> CompileContext {
        CompileContext {
            definition_base_dir: PathBuf::from("."),
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid spec"),
        }
    }

    #[test]
    fn conversion_helpers_match_audio_units() {
        assert!((db_to_linear(-6.0206) - 0.5).abs() < 0.001);
        assert!((cents_to_ratio(1200.0) - 2.0).abs() < 1.0e-6);
        assert!((midi_note_frequency(69, 1.0) - 440.0).abs() < 1.0e-6);
    }

    #[test]
    fn oscillator_backend_frequency_limits_are_rate_derived() {
        assert!(
            (CompiledOscillatorBackend::Basic.effective_max_frequency(48_000.0) - 21_600.0).abs()
                < f32::EPSILON
        );
        assert!(
            (CompiledOscillatorBackend::VariableShapeSync {
                sync_ratio: ParameterHandle::new(0),
            }
            .effective_max_frequency(48_000.0)
                - 11_520.0)
                .abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn valid_definition_compiles_to_catalog_bindings() {
        let source = definition();
        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("compiled instrument");
        assert!(result.diagnostics.is_empty());
        assert_eq!(compiled.layers.len(), 1);
        assert_eq!(compiled.parameters()[0].id, "layer.body.gain");
        assert_eq!(
            compiled.parameter_handle("layer.body.gain"),
            Some(compiled.layers[0].parameters.gain)
        );
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

    #[test]
    fn cutoff_is_clamped_with_a_warning_but_catalog_range_is_stable() {
        let mut source = definition();
        source.voice_processors.push(ProcessorDefinition::Filter(
            crate::definition::FilterProcessorDefinition {
                id: "tone".to_owned(),
                cutoff_hz: 20_000.0,
                resonance: 0.1,
            },
        ));
        let low_rate = CompileContext {
            process_spec: ProcessSpec::new(22_050.0, 257, 2).expect("valid spec"),
            ..context()
        };
        let result = compile_instrument(&source, &low_rate);
        let compiled = result.instrument.expect("compiled");
        let warning = result.diagnostics.iter().find(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Warning
                && diagnostic.code == DiagnosticCode::FilterCutoffClamped
        });
        assert_eq!(
            warning.expect("cutoff warning").message,
            "cutoff exceeds the process-safe maximum and will be clamped to 9922.500 Hz during DSP processing"
        );
        assert!(
            (match &compiled.voice_processors[0].processor {
                CompiledProcessorKind::Filter(filter) => filter.effective_max_cutoff_hz,
                _ => panic!("voice processor must be a filter"),
            } - 9_922.5)
                .abs()
                < 0.1
        );
        assert!(
            (compiled
                .parameters()
                .iter()
                .find(|parameter| parameter.id == "voice.processor.tone.cutoff")
                .expect("cutoff parameter")
                .default
                - 20_000.0)
                .abs()
                < 0.1
        );
        assert!((compiled.parameters().last().expect("resonance").max - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn processor_chains_compile_in_definition_order() {
        let mut source = definition();
        source.layers[0].processors = vec![
            ProcessorDefinition::Filter(crate::definition::FilterProcessorDefinition {
                id: "layer_tone".to_owned(),
                cutoff_hz: 8_000.0,
                resonance: 0.1,
            }),
            ProcessorDefinition::Drive(crate::definition::DriveProcessorDefinition {
                id: "layer_drive".to_owned(),
                amount: 0.2,
                mix: 0.4,
            }),
        ];
        source.voice_processors.push(ProcessorDefinition::Drive(
            crate::definition::DriveProcessorDefinition {
                id: "glue".to_owned(),
                amount: 0.1,
                mix: 0.2,
            },
        ));
        source.global_processors.push(ProcessorDefinition::Delay(
            crate::definition::DelayProcessorDefinition {
                id: "echo".to_owned(),
                time_seconds: 0.2,
                feedback: 0.3,
                mix: 0.15,
            },
        ));

        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("processor chains compile");

        assert_eq!(compiled.layers[0].processors.len(), 2);
        assert_eq!(compiled.layers[0].processors[0].id, "layer_tone");
        assert_eq!(compiled.layers[0].processors[1].id, "layer_drive");
        assert_eq!(compiled.voice_processors[0].id, "glue");
        assert_eq!(compiled.global_processors[0].id, "echo");
        assert_eq!(
            compiled.parameters().last().expect("delay mix").id,
            "global.processor.echo.mix"
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn reverb_compiles_reference_delay_lengths_and_output_taps() {
        let mut source = definition();
        source
            .global_processors
            .push(ProcessorDefinition::Reverb(ReverbProcessorDefinition {
                id: "space".to_owned(),
                pre_delay_seconds: 0.0,
                decay: 0.5,
                damping: 0.2,
                width: 1.0,
                mix: 0.3,
            }));
        let context = CompileContext {
            process_spec: ProcessSpec::new(29_761.0, 257, 2).expect("valid spec"),
            ..context()
        };

        let result = compile_instrument(&source, &context);
        let compiled = result.instrument.expect("reverb compiles");
        assert!(result.diagnostics.is_empty());

        let CompiledProcessorKind::Reverb(reverb) = &compiled.global_processors[0].processor else {
            panic!("global processor must be reverb");
        };
        assert_eq!(reverb.pre_delay_frames, 0);
        assert_eq!(reverb.input_diffusion_lengths, [142, 107, 379, 277]);
        assert_eq!(reverb.tank_left_lengths, [672, 4_453, 1_800, 3_720]);
        assert_eq!(reverb.tank_right_lengths, [908, 4_217, 2_656, 3_163]);
        assert_eq!(
            reverb.left_output_taps,
            [
                ReverbOutputTap {
                    source: ReverbTapSource::RightLongDelay,
                    delay_frames: 266,
                    sign: 1,
                },
                ReverbOutputTap {
                    source: ReverbTapSource::RightLongDelay,
                    delay_frames: 2_974,
                    sign: 1,
                },
                ReverbOutputTap {
                    source: ReverbTapSource::RightTankAllpass,
                    delay_frames: 1_913,
                    sign: -1,
                },
                ReverbOutputTap {
                    source: ReverbTapSource::RightOutputDelay,
                    delay_frames: 1_996,
                    sign: 1,
                },
                ReverbOutputTap {
                    source: ReverbTapSource::LeftLongDelay,
                    delay_frames: 1_990,
                    sign: -1,
                },
                ReverbOutputTap {
                    source: ReverbTapSource::LeftTankAllpass,
                    delay_frames: 187,
                    sign: -1,
                },
                ReverbOutputTap {
                    source: ReverbTapSource::LeftOutputDelay,
                    delay_frames: 1_066,
                    sign: -1,
                },
            ]
        );
        assert_eq!(
            reverb.right_output_taps,
            [
                ReverbOutputTap {
                    source: ReverbTapSource::LeftLongDelay,
                    delay_frames: 353,
                    sign: 1,
                },
                ReverbOutputTap {
                    source: ReverbTapSource::LeftLongDelay,
                    delay_frames: 3_627,
                    sign: 1,
                },
                ReverbOutputTap {
                    source: ReverbTapSource::LeftTankAllpass,
                    delay_frames: 1_228,
                    sign: -1,
                },
                ReverbOutputTap {
                    source: ReverbTapSource::LeftOutputDelay,
                    delay_frames: 2_673,
                    sign: 1,
                },
                ReverbOutputTap {
                    source: ReverbTapSource::RightLongDelay,
                    delay_frames: 2_111,
                    sign: -1,
                },
                ReverbOutputTap {
                    source: ReverbTapSource::RightTankAllpass,
                    delay_frames: 335,
                    sign: -1,
                },
                ReverbOutputTap {
                    source: ReverbTapSource::RightOutputDelay,
                    delay_frames: 121,
                    sign: -1,
                },
            ]
        );
        assert!((reverb.modulation_increment - 1.0 / 29_761.0).abs() < 1.0e-10);
        assert!((reverb.modulation_excursion - 16.0).abs() < 1.0e-5);
    }

    #[test]
    fn voice_sources_cannot_target_global_processors() {
        let mut source = definition();
        source.global_processors.push(ProcessorDefinition::Drive(
            crate::definition::DriveProcessorDefinition {
                id: "master_drive".to_owned(),
                amount: 0.1,
                mix: 0.2,
            },
        ));
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![],
            routes: vec![crate::definition::ModulationRouteDefinition {
                source: "velocity".to_owned(),
                target: "global.processor.master_drive.mix".to_owned(),
                amount: 0.2,
                curve: ModulationCurve::Linear,
            }],
        });

        let result = compile_instrument(&source, &context());

        assert!(result.instrument.is_none());
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::GlobalRouteScopeInvalid
                && diagnostic.path.as_deref() == Some("modulation.routes[0].source")
        }));
    }

    #[test]
    fn invalid_definition_does_not_produce_compiled_state() {
        let mut source = definition();
        source.layers[0].gain_db = 99.0;
        let result = compile_instrument(&source, &context());
        assert!(result.instrument.is_none());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        );
    }

    #[test]
    fn routes_resolve_to_dense_targets_and_preserve_same_target_order() {
        let mut source = definition();
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![],
            routes: vec![
                crate::definition::ModulationRouteDefinition {
                    source: "velocity".to_owned(),
                    target: "layer.body.gain".to_owned(),
                    amount: 0.1,
                    curve: ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "key_tracking".to_owned(),
                    target: "layer.body.gain".to_owned(),
                    amount: -0.2,
                    curve: ModulationCurve::SmoothStep,
                },
            ],
        });
        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("routes compile");
        let routes = compiled.routes_for(compiled.layers[0].parameters.gain);
        assert_eq!(routes.len(), 2);
        assert!((routes[0].amount - 0.1).abs() < f32::EPSILON);
        assert!((routes[1].amount + 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn all_voice_source_kinds_compile_with_routes() {
        let mut source = definition();
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![
                crate::definition::ModulationSourceDefinition::Lfo(
                    crate::definition::LfoDefinition {
                        id: "slow_lfo".to_owned(),
                        waveform: crate::definition::LfoWaveform::Sine,
                        rate_hz: 2.0,
                        phase: 0.0,
                    },
                ),
                crate::definition::ModulationSourceDefinition::Envelope(
                    crate::definition::ModEnvelopeDefinition {
                        id: "mod_env".to_owned(),
                        attack_seconds: 0.01,
                        decay_seconds: 0.1,
                        sustain_level: 0.5,
                        release_seconds: 0.1,
                    },
                ),
                crate::definition::ModulationSourceDefinition::Random(
                    crate::definition::RandomDefinition {
                        id: "random".to_owned(),
                        seed: 7,
                    },
                ),
            ],
            routes: vec![
                crate::definition::ModulationRouteDefinition {
                    source: "slow_lfo".to_owned(),
                    target: "layer.body.gain".to_owned(),
                    amount: 0.1,
                    curve: ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "mod_env".to_owned(),
                    target: "layer.body.pan".to_owned(),
                    amount: 0.2,
                    curve: ModulationCurve::SmoothStep,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "random".to_owned(),
                    target: "layer.body.tuning".to_owned(),
                    amount: 0.05,
                    curve: ModulationCurve::Linear,
                },
            ],
        });
        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("all source kinds compile");
        assert_eq!(compiled.sources.len(), 5);
        assert_eq!(compiled.routes.len(), 3);
        assert!(
            compiled
                .routes
                .iter()
                .all(|route| route.target.index() < compiled.parameters().len())
        );
    }

    #[test]
    fn unresolved_routes_are_reported_without_panicking() {
        let mut source = definition();
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![],
            routes: vec![crate::definition::ModulationRouteDefinition {
                source: "missing".to_owned(),
                target: "layer.missing.gain".to_owned(),
                amount: 0.1,
                curve: ModulationCurve::Linear,
            }],
        });
        let result = compile_instrument(&source, &context());
        assert!(result.instrument.is_none());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::ParameterNotFound)
        );
    }
}
