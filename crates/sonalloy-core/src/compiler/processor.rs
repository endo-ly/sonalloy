use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use super::convolution::{PreparedConvolutionIr, prepare_convolution_ir};
use super::generator::{CompiledFormantBand, CompiledFormantProfile};
use super::{
    AssetCacheKey, BASIC_FREQUENCY_LIMIT_RATIO, SPECTRAL_MORPH_LATENCY_FRAMES, asset_diagnostic,
    db_to_linear, effective_max_cutoff, effective_max_frequency, prepare_cached_asset,
};
use crate::asset::{AssetError, PreparedAsset};
use crate::definition::{
    BitcrusherProcessorDefinition, ChorusProcessorDefinition, CompressorProcessorDefinition,
    ConvolutionProcessorDefinition, DelayFeedbackMode, DelayProcessorDefinition,
    DelayTimeDefinition, DelayTimeUnit, DriveProcessorDefinition, DynamicsDetectorDefinition,
    EnvelopeTransferProcessorDefinition, EqProcessorDefinition, FilterModeDefinition,
    FilterProcessorDefinition, FlangerProcessorDefinition, FormantProcessorDefinition,
    FrequencyShifterProcessorDefinition, GateProcessorDefinition, LadderFilterProcessorDefinition,
    LimiterProcessorDefinition, PhaserProcessorDefinition, ProcessorDefinition,
    ResonatorProcessorDefinition, ReverbProcessorDefinition, SpectralMorphProcessorDefinition,
    TransientShaperProcessorDefinition, VocoderProcessorDefinition,
};
use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::parameter::{
    ParameterCatalog, ParameterHandle, global_processor_parameter_id, layer_processor_parameter_id,
    voice_processor_parameter_id,
};

const MAX_DELAY_RUNTIME_SECONDS: f32 = 16.0;
const HILBERT_TAPS: usize = 255;
const FREQUENCY_SHIFTER_LATENCY_FRAMES: usize = (HILBERT_TAPS - 1) / 2;

#[allow(clippy::cast_possible_wrap, clippy::cast_precision_loss)]
fn build_hilbert_coefficients() -> Vec<f32> {
    let center = (HILBERT_TAPS - 1) / 2;
    (0..HILBERT_TAPS)
        .map(|index| {
            let offset = index as isize - center as isize;
            let ideal = if offset != 0 && offset % 2 != 0 {
                2.0 / (std::f32::consts::PI * offset as f32)
            } else {
                0.0
            };
            let phase = std::f32::consts::TAU * index as f32 / (HILBERT_TAPS - 1) as f32;
            ideal * (0.42 - 0.5 * phase.cos() + 0.08 * (phase * 2.0).cos())
        })
        .collect()
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
    /// Selected filter output mode.
    pub mode: FilterModeDefinition,
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

/// Dynamic parameter handles used by a three-band equalizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledEqParameters {
    /// Low-shelf gain handle.
    pub low_gain_db: ParameterHandle,
    /// Mid peaking gain handle.
    pub mid_gain_db: ParameterHandle,
    /// High-shelf gain handle.
    pub high_gain_db: ParameterHandle,
}

/// Compiled three-band equalizer processor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledEqProcessor {
    /// Low-shelf midpoint.
    pub low_frequency_hz: f32,
    /// Mid peaking center frequency.
    pub mid_frequency_hz: f32,
    /// Mid peaking Q factor.
    pub mid_q: f32,
    /// High-shelf midpoint.
    pub high_frequency_hz: f32,
    /// Dynamic gain bindings.
    pub parameters: CompiledEqParameters,
}

/// Dynamic parameter handles used by a tuned resonator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledResonatorParameters {
    /// Resonance frequency handle.
    pub frequency_hz: ParameterHandle,
    /// Decay time handle.
    pub decay_seconds: ParameterHandle,
    /// Damping handle.
    pub damping: ParameterHandle,
    /// Dry/wet mix handle.
    pub mix: ParameterHandle,
}

/// Compiled tuned resonator processor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledResonatorProcessor {
    /// Dynamic parameter bindings.
    pub parameters: CompiledResonatorParameters,
    /// Maximum delay-line length in frames.
    pub max_delay_frames: usize,
    /// Process sample rate.
    pub sample_rate: f32,
}

/// Dynamic parameter handles used by a bitcrusher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledBitcrusherParameters {
    /// Quantizer bit-depth handle.
    pub bit_depth: ParameterHandle,
    /// Sample-rate ratio handle.
    pub sample_rate_ratio: ParameterHandle,
    /// Dry/wet mix handle.
    pub mix: ParameterHandle,
}

/// Compiled bitcrusher processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledBitcrusherProcessor {
    /// Dynamic parameter bindings.
    pub parameters: CompiledBitcrusherParameters,
}

/// Dynamic parameter handles used by a ladder filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledLadderFilterParameters {
    /// Cutoff handle.
    pub cutoff: ParameterHandle,
    /// Resonance handle.
    pub resonance: ParameterHandle,
    /// Drive handle.
    pub drive: ParameterHandle,
}

/// Compiled ladder filter processor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledLadderFilterProcessor {
    /// Dynamic parameter bindings.
    pub parameters: CompiledLadderFilterParameters,
    /// Process-rate-specific safe cutoff maximum.
    pub effective_max_cutoff_hz: f32,
}

/// Dynamic parameter handles used by a formant processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledFormantProcessorParameters {
    /// Vowel position handle.
    pub vowel_position: ParameterHandle,
    /// Formant shift handle.
    pub formant_shift: ParameterHandle,
    /// Throat handle.
    pub throat: ParameterHandle,
    /// Dry/wet mix handle.
    pub mix: ParameterHandle,
}

/// Compiled formant processor.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledFormantProcessor {
    /// Interpolated profile source data.
    pub profiles: Box<[CompiledFormantProfile]>,
    /// Dynamic parameter bindings.
    pub parameters: CompiledFormantProcessorParameters,
}

/// Dynamic parameter handles used by a frequency shifter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledFrequencyShifterParameters {
    /// Frequency shift handle.
    pub shift_hz: ParameterHandle,
    /// Dry/wet mix handle.
    pub mix: ParameterHandle,
}

/// Compiled frequency shifter processor.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledFrequencyShifterProcessor {
    /// Dynamic parameter bindings.
    pub parameters: CompiledFrequencyShifterParameters,
    /// Prepared Hilbert transformer coefficients.
    pub coefficients: Arc<[f32]>,
    /// Fixed group delay.
    pub latency_frames: usize,
    /// Effective symmetric shift limit.
    pub effective_abs_shift_hz: f32,
}

/// Dynamic parameter handles used by a convolution processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledConvolutionParameters {
    /// Wet gain handle.
    pub gain_db: ParameterHandle,
    /// Dry/wet mix handle.
    pub mix: ParameterHandle,
}

/// Compiled convolution processor.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledConvolutionProcessor {
    /// Prepared immutable impulse response.
    pub(crate) prepared_ir: Arc<PreparedConvolutionIr>,
    /// Definition-relative or absolute asset path.
    pub asset_path: String,
    /// Whether the Definition supplied a SHA-256 constraint.
    pub asset_sha256_specified: bool,
    /// Dynamic parameter bindings.
    pub parameters: CompiledConvolutionParameters,
    /// Fixed partition latency.
    pub latency_frames: usize,
}

impl CompiledConvolutionProcessor {
    /// Return the number of channels in the prepared impulse response.
    #[must_use]
    pub fn source_channels(&self) -> usize {
        self.prepared_ir.source_channels
    }

    /// Return the source frame count before partition padding.
    #[must_use]
    pub fn source_frames(&self) -> usize {
        self.prepared_ir.source_frames
    }

    /// Return the number of prepared response frames.
    #[must_use]
    pub fn prepared_frames(&self) -> usize {
        self.prepared_ir.prepared_frames
    }

    /// Return the number of prepared FFT partitions.
    #[must_use]
    pub fn partition_count(&self) -> usize {
        self.prepared_ir.partition_count()
    }
}
/// Dynamic parameter handles used by a gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledGateParameters {
    /// Threshold handle.
    pub threshold_db: ParameterHandle,
    /// Range handle.
    pub range_db: ParameterHandle,
}

/// Compiled gate processor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledGateProcessor {
    /// Hysteresis in decibels.
    pub hysteresis_db: f32,
    /// Fixed detector attack coefficient.
    pub detector_attack_coeff: f32,
    /// Fixed detector release coefficient.
    pub detector_release_coeff: f32,
    /// Attack coefficient.
    pub attack_coeff: f32,
    /// Hold duration in frames.
    pub hold_frames: usize,
    /// Release coefficient.
    pub release_coeff: f32,
    /// Detector source.
    pub detector: CompiledDynamicsDetector,
    /// External input delay required to match the carrier timeline.
    pub external_input_alignment_frames: usize,
    /// Dynamic parameter bindings.
    pub parameters: CompiledGateParameters,
}

/// Compiled signal source used by a dynamics detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledDynamicsDetector {
    /// Detect the processor carrier.
    SelfSignal,
    /// Detect aligned external audio.
    ExternalAudio,
}

/// Dynamic parameters used by a vocoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledVocoderParameters {
    /// Modulator gain handle.
    pub modulator_gain_db: ParameterHandle,
    /// Wet output gain handle.
    pub output_gain_db: ParameterHandle,
    /// Dry/wet mix handle.
    pub mix: ParameterHandle,
}

/// Compiled fixed-band vocoder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledVocoderProcessor {
    /// Analyzer attack coefficient.
    pub attack_coeff: f32,
    /// Analyzer release coefficient.
    pub release_coeff: f32,
    /// Dynamic parameter bindings.
    pub parameters: CompiledVocoderParameters,
    /// External input delay required to match the carrier timeline.
    pub external_input_alignment_frames: usize,
}

/// Dynamic parameters used by envelope transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledEnvelopeTransferParameters {
    /// External input gain handle.
    pub input_gain_db: ParameterHandle,
    /// Minimum gain handle.
    pub floor_db: ParameterHandle,
    /// Dry/wet mix handle.
    pub mix: ParameterHandle,
}

/// Compiled amplitude-envelope transfer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledEnvelopeTransferProcessor {
    /// Detector attack coefficient.
    pub attack_coeff: f32,
    /// Detector release coefficient.
    pub release_coeff: f32,
    /// Dynamic parameter bindings.
    pub parameters: CompiledEnvelopeTransferParameters,
    /// External input delay required to match the carrier timeline.
    pub external_input_alignment_frames: usize,
}

/// Dynamic parameters used by spectral morph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledSpectralMorphParameters {
    /// Magnitude morph handle.
    pub morph: ParameterHandle,
    /// Output gain handle.
    pub output_gain_db: ParameterHandle,
}

/// Compiled streaming spectral magnitude morph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledSpectralMorphProcessor {
    /// Dynamic parameter bindings.
    pub parameters: CompiledSpectralMorphParameters,
    /// External input delay required to match the carrier timeline.
    pub external_input_alignment_frames: usize,
}

/// Dynamic parameter handles used by a transient shaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledTransientShaperParameters {
    /// Attack handle.
    pub attack: ParameterHandle,
    /// Sustain handle.
    pub sustain: ParameterHandle,
    /// Dry/wet mix handle.
    pub mix: ParameterHandle,
}

/// Compiled transient shaper processor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledTransientShaperProcessor {
    /// Fast detector attack coefficient.
    pub fast_attack_coeff: f32,
    /// Fast detector release coefficient.
    pub fast_release_coeff: f32,
    /// Slow detector attack coefficient.
    pub slow_attack_coeff: f32,
    /// Slow detector release coefficient.
    pub slow_release_coeff: f32,
    /// Dynamic parameter bindings.
    pub parameters: CompiledTransientShaperParameters,
}

/// Dynamic parameter handles used by a modulated delay effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledModulationDelayParameters {
    /// LFO rate handle.
    pub rate_hz: ParameterHandle,
    /// Delay depth handle.
    pub depth: ParameterHandle,
    /// Feedback handle.
    pub feedback: ParameterHandle,
    /// Stereo width handle.
    pub width: ParameterHandle,
    /// Dry/wet mix handle.
    pub mix: ParameterHandle,
}

/// Compiled chorus processor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledChorusProcessor {
    /// Center delay in frames.
    pub delay_frames: f32,
    /// Allocated maximum delay length in frames.
    pub max_delay_frames: usize,
    /// Process sample rate.
    pub sample_rate: f32,
    /// Dynamic parameter bindings.
    pub parameters: CompiledModulationDelayParameters,
}

/// Compiled flanger processor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledFlangerProcessor {
    /// Center delay in frames.
    pub delay_frames: f32,
    /// Allocated maximum delay length in frames.
    pub max_delay_frames: usize,
    /// Process sample rate.
    pub sample_rate: f32,
    /// Dynamic parameter bindings.
    pub parameters: CompiledModulationDelayParameters,
}

/// Dynamic parameter handles used by a phaser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledPhaserParameters {
    /// LFO rate handle.
    pub rate_hz: ParameterHandle,
    /// Sweep depth handle.
    pub depth: ParameterHandle,
    /// Feedback handle.
    pub feedback: ParameterHandle,
    /// Stereo width handle.
    pub width: ParameterHandle,
    /// Dry/wet mix handle.
    pub mix: ParameterHandle,
}

/// Compiled phaser processor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledPhaserProcessor {
    /// Number of all-pass stages.
    pub stages: u8,
    /// Sweep center frequency.
    pub center_hz: f32,
    /// Sweep range in octaves.
    pub sweep_octaves: f32,
    /// Process sample rate.
    pub sample_rate: f32,
    /// Dynamic parameter bindings.
    pub parameters: CompiledPhaserParameters,
}

/// Dynamic parameter handles used by a compressor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledCompressorParameters {
    /// Threshold handle.
    pub threshold_db: ParameterHandle,
    /// Ratio handle.
    pub ratio: ParameterHandle,
    /// Makeup gain handle.
    pub makeup_gain_db: ParameterHandle,
    /// Dry/wet mix handle.
    pub mix: ParameterHandle,
}

/// Compiled compressor processor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledCompressorProcessor {
    /// Attack coefficient.
    pub attack_coeff: f32,
    /// Release coefficient.
    pub release_coeff: f32,
    /// Soft-knee width.
    pub knee_db: f32,
    /// Detector source.
    pub detector: CompiledDynamicsDetector,
    /// External input delay required to match the carrier timeline.
    pub external_input_alignment_frames: usize,
    /// Dynamic parameter bindings.
    pub parameters: CompiledCompressorParameters,
}

/// Dynamic parameter handles used by a limiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledLimiterParameters {
    /// Ceiling handle.
    pub ceiling_db: ParameterHandle,
    /// Input gain handle.
    pub input_gain_db: ParameterHandle,
}

/// Compiled limiter processor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledLimiterProcessor {
    /// Release coefficient.
    pub release_coeff: f32,
    /// Dynamic parameter bindings.
    pub parameters: CompiledLimiterParameters,
}

/// A compiled delay time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompiledDelayTime {
    /// Time in seconds.
    Seconds(f64),
    /// Time in quarter-note beats.
    Beats(f64),
}

/// A compiled feed-forward delay tap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledDelayTap {
    /// Tap time.
    pub time: CompiledDelayTime,
    /// Linear tap gain.
    pub gain_linear: f32,
}

/// Compiled delay processor.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledDelayProcessor {
    /// Primary delay time.
    pub time: CompiledDelayTime,
    /// Feedback routing mode.
    pub feedback_mode: DelayFeedbackMode,
    /// Feed-forward taps.
    pub taps: Box<[CompiledDelayTap]>,
    /// Maximum allocated delay length in frames.
    pub max_delay_frames: usize,
    /// Wet tap normalization.
    pub wet_normalization: f32,
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
    /// State-variable filter processor.
    Filter(CompiledFilterProcessor),
    /// Four-pole ladder filter processor.
    LadderFilter(CompiledLadderFilterProcessor),
    /// Soft-clipping drive processor.
    Drive(CompiledDriveProcessor),
    /// Three-band equalizer processor.
    Eq(CompiledEqProcessor),
    /// Five-band formant processor.
    Formant(CompiledFormantProcessor),
    /// Tuned resonator processor.
    Resonator(CompiledResonatorProcessor),
    /// Bitcrusher processor.
    Bitcrusher(CompiledBitcrusherProcessor),
    /// Chorus processor.
    Chorus(CompiledChorusProcessor),
    /// Flanger processor.
    Flanger(CompiledFlangerProcessor),
    /// Phaser processor.
    Phaser(CompiledPhaserProcessor),
    /// Frequency shifter processor.
    FrequencyShifter(CompiledFrequencyShifterProcessor),
    /// Stereo delay processor.
    Delay(CompiledDelayProcessor),
    /// Stereo plate reverb processor.
    Reverb(CompiledReverbProcessor),
    /// Impulse-response convolution processor.
    Convolution(CompiledConvolutionProcessor),
    /// Stereo-linked gate processor.
    Gate(CompiledGateProcessor),
    /// Fixed-band external-audio vocoder.
    Vocoder(CompiledVocoderProcessor),
    /// External amplitude-envelope transfer.
    EnvelopeTransfer(CompiledEnvelopeTransferProcessor),
    /// Streaming external spectral morph.
    SpectralMorph(CompiledSpectralMorphProcessor),
    /// Transient shaper processor.
    TransientShaper(CompiledTransientShaperProcessor),
    /// Stereo-linked compressor processor.
    Compressor(CompiledCompressorProcessor),
    /// Zero-latency limiter processor.
    Limiter(CompiledLimiterProcessor),
}

impl CompiledProcessorKind {
    /// Return fixed algorithmic latency introduced by this processor.
    #[must_use]
    pub fn intrinsic_latency_frames(&self) -> usize {
        match self {
            Self::FrequencyShifter(value) => value.latency_frames,
            Self::Convolution(value) => value.latency_frames,
            Self::SpectralMorph(_) => SPECTRAL_MORPH_LATENCY_FRAMES,
            Self::Filter(_)
            | Self::LadderFilter(_)
            | Self::Drive(_)
            | Self::Eq(_)
            | Self::Formant(_)
            | Self::Resonator(_)
            | Self::Bitcrusher(_)
            | Self::Chorus(_)
            | Self::Flanger(_)
            | Self::Phaser(_)
            | Self::Delay(_)
            | Self::Reverb(_)
            | Self::Gate(_)
            | Self::Vocoder(_)
            | Self::EnvelopeTransfer(_)
            | Self::TransientShaper(_)
            | Self::Compressor(_)
            | Self::Limiter(_) => 0,
        }
    }
}

/// One processor in a Definition-ordered chain.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledProcessor {
    /// Stable processor identifier.
    pub id: String,
    /// Compiled processor kind.
    pub processor: CompiledProcessorKind,
}

#[derive(Clone, Copy)]
pub(super) enum ProcessorPlacement {
    Layer,
    Voice,
    Global,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_processor_chain(
    processors: &[ProcessorDefinition],
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    base_path: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    definition_base_dir: &Path,
    asset_cache: &mut HashMap<AssetCacheKey, Result<PreparedAsset, AssetError>>,
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
                definition_base_dir,
                asset_cache,
                diagnostics,
            )
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn compile_processor(
    processor: &ProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    path: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    definition_base_dir: &Path,
    asset_cache: &mut HashMap<AssetCacheKey, Result<PreparedAsset, AssetError>>,
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
        ProcessorDefinition::LadderFilter(value) => {
            compile_ladder_filter_processor(value, placement, layer_id, catalog, sample_rate)
        }
        ProcessorDefinition::Drive(value) => {
            compile_drive_processor(value, placement, layer_id, catalog)
        }
        ProcessorDefinition::Eq(value) => compile_eq_processor(
            value,
            placement,
            layer_id,
            path,
            catalog,
            sample_rate,
            diagnostics,
        ),
        ProcessorDefinition::Formant(value) => {
            compile_formant_processor(value, placement, layer_id, catalog)
        }
        ProcessorDefinition::Resonator(value) => compile_resonator_processor(
            value,
            placement,
            layer_id,
            path,
            catalog,
            sample_rate,
            diagnostics,
        ),
        ProcessorDefinition::Bitcrusher(value) => {
            compile_bitcrusher_processor(value, placement, layer_id, catalog)
        }
        ProcessorDefinition::Chorus(value) => compile_chorus_processor(
            value,
            placement,
            layer_id,
            path,
            catalog,
            sample_rate,
            diagnostics,
        ),
        ProcessorDefinition::Flanger(value) => compile_flanger_processor(
            value,
            placement,
            layer_id,
            path,
            catalog,
            sample_rate,
            diagnostics,
        ),
        ProcessorDefinition::Phaser(value) => compile_phaser_processor(
            value,
            placement,
            layer_id,
            path,
            catalog,
            sample_rate,
            diagnostics,
        ),
        ProcessorDefinition::FrequencyShifter(value) => compile_frequency_shifter_processor(
            value,
            placement,
            layer_id,
            path,
            catalog,
            sample_rate,
            diagnostics,
        ),
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
        ProcessorDefinition::Convolution(value) => compile_convolution_processor(
            value,
            placement,
            layer_id,
            path,
            catalog,
            definition_base_dir,
            asset_cache,
            sample_rate,
            diagnostics,
        ),
        ProcessorDefinition::Gate(value) => {
            compile_gate_processor(value, placement, layer_id, catalog, sample_rate)
        }
        ProcessorDefinition::Vocoder(value) => {
            compile_vocoder_processor(value, placement, layer_id, catalog, sample_rate)
        }
        ProcessorDefinition::EnvelopeTransfer(value) => {
            compile_envelope_transfer_processor(value, placement, layer_id, catalog, sample_rate)
        }
        ProcessorDefinition::SpectralMorph(value) => {
            compile_spectral_morph_processor(value, placement, layer_id, catalog)
        }
        ProcessorDefinition::TransientShaper(value) => {
            compile_transient_shaper_processor(value, placement, layer_id, catalog, sample_rate)
        }
        ProcessorDefinition::Compressor(value) => {
            compile_compressor_processor(value, placement, layer_id, catalog, sample_rate)
        }
        ProcessorDefinition::Limiter(value) => {
            compile_limiter_processor(value, placement, layer_id, catalog, sample_rate)
        }
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
    let id = match placement {
        ProcessorPlacement::Layer => layer_processor_parameter_id(
            layer_id.expect("layer processor has a layer id"),
            processor_id,
            parameter,
        ),
        ProcessorPlacement::Voice => voice_processor_parameter_id(processor_id, parameter),
        ProcessorPlacement::Global => global_processor_parameter_id(processor_id, parameter),
    };
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
        mode: value.mode,
        parameters: CompiledFilterParameters { cutoff, resonance },
        effective_max_cutoff_hz,
    })
}

fn compile_ladder_filter_processor(
    value: &LadderFilterProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    catalog: &ParameterCatalog,
    sample_rate: f64,
) -> CompiledProcessorKind {
    CompiledProcessorKind::LadderFilter(CompiledLadderFilterProcessor {
        parameters: CompiledLadderFilterParameters {
            cutoff: processor_parameter_handle(catalog, placement, layer_id, &value.id, "cutoff"),
            resonance: processor_parameter_handle(
                catalog,
                placement,
                layer_id,
                &value.id,
                "resonance",
            ),
            drive: processor_parameter_handle(catalog, placement, layer_id, &value.id, "drive"),
        },
        effective_max_cutoff_hz: effective_max_cutoff(sample_rate),
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

fn compile_eq_processor(
    value: &EqProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    path: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledProcessorKind {
    validate_processor_frequency(
        value.high_frequency_hz,
        sample_rate,
        &format!("{path}.high_frequency_hz"),
        diagnostics,
    );
    let parameters = CompiledEqParameters {
        low_gain_db: processor_parameter_handle(
            catalog,
            placement,
            layer_id,
            &value.id,
            "low_gain_db",
        ),
        mid_gain_db: processor_parameter_handle(
            catalog,
            placement,
            layer_id,
            &value.id,
            "mid_gain_db",
        ),
        high_gain_db: processor_parameter_handle(
            catalog,
            placement,
            layer_id,
            &value.id,
            "high_gain_db",
        ),
    };
    CompiledProcessorKind::Eq(CompiledEqProcessor {
        low_frequency_hz: value.low_frequency_hz,
        mid_frequency_hz: value.mid_frequency_hz,
        mid_q: value.mid_q,
        high_frequency_hz: value.high_frequency_hz,
        parameters,
    })
}

fn compile_formant_processor(
    value: &FormantProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    catalog: &ParameterCatalog,
) -> CompiledProcessorKind {
    let profiles = value
        .profiles
        .iter()
        .map(|profile| CompiledFormantProfile {
            id: profile.id.clone(),
            formants: std::array::from_fn(|index| {
                let band = profile
                    .formants
                    .get(index)
                    .expect("validated formant processor profile contains five bands");
                CompiledFormantBand {
                    frequency_hz: band.frequency_hz,
                    bandwidth_hz: band.bandwidth_hz,
                    gain_db: band.gain_db,
                }
            }),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    CompiledProcessorKind::Formant(CompiledFormantProcessor {
        profiles,
        parameters: CompiledFormantProcessorParameters {
            vowel_position: processor_parameter_handle(
                catalog,
                placement,
                layer_id,
                &value.id,
                "vowel_position",
            ),
            formant_shift: processor_parameter_handle(
                catalog,
                placement,
                layer_id,
                &value.id,
                "formant_shift",
            ),
            throat: processor_parameter_handle(catalog, placement, layer_id, &value.id, "throat"),
            mix: processor_parameter_handle(catalog, placement, layer_id, &value.id, "mix"),
        },
    })
}

fn compile_resonator_processor(
    value: &ResonatorProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    path: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledProcessorKind {
    validate_processor_frequency(
        value.frequency_hz,
        sample_rate,
        &format!("{path}.frequency_hz"),
        diagnostics,
    );
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let sample_rate = sample_rate as f32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let max_delay_frames = (sample_rate / 40.0).ceil() as usize + 4;
    let parameters = CompiledResonatorParameters {
        frequency_hz: processor_parameter_handle(
            catalog,
            placement,
            layer_id,
            &value.id,
            "frequency_hz",
        ),
        decay_seconds: processor_parameter_handle(
            catalog,
            placement,
            layer_id,
            &value.id,
            "decay_seconds",
        ),
        damping: processor_parameter_handle(catalog, placement, layer_id, &value.id, "damping"),
        mix: processor_parameter_handle(catalog, placement, layer_id, &value.id, "mix"),
    };
    CompiledProcessorKind::Resonator(CompiledResonatorProcessor {
        parameters,
        max_delay_frames,
        sample_rate,
    })
}

fn compile_bitcrusher_processor(
    value: &BitcrusherProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    catalog: &ParameterCatalog,
) -> CompiledProcessorKind {
    CompiledProcessorKind::Bitcrusher(CompiledBitcrusherProcessor {
        parameters: CompiledBitcrusherParameters {
            bit_depth: processor_parameter_handle(
                catalog,
                placement,
                layer_id,
                &value.id,
                "bit_depth",
            ),
            sample_rate_ratio: processor_parameter_handle(
                catalog,
                placement,
                layer_id,
                &value.id,
                "sample_rate_ratio",
            ),
            mix: processor_parameter_handle(catalog, placement, layer_id, &value.id, "mix"),
        },
    })
}

fn compile_chorus_processor(
    value: &ChorusProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    path: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledProcessorKind {
    compile_modulation_delay_processor(
        value.delay_ms,
        placement,
        layer_id,
        &value.id,
        path,
        catalog,
        sample_rate,
        diagnostics,
        true,
    )
}

fn compile_flanger_processor(
    value: &FlangerProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    path: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledProcessorKind {
    compile_modulation_delay_processor(
        value.delay_ms,
        placement,
        layer_id,
        &value.id,
        path,
        catalog,
        sample_rate,
        diagnostics,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn compile_modulation_delay_processor(
    delay_ms: f32,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    processor_id: &str,
    path: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
    chorus: bool,
) -> CompiledProcessorKind {
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let sample_rate_f32 = sample_rate as f32;
    let center_frames = delay_ms * sample_rate_f32 / 1_000.0;
    let modulation_factor = if chorus { 0.9 } else { 0.95 };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let max_delay_frames = (center_frames * (1.0 + modulation_factor) + 4.0).ceil() as usize;
    let parameters = CompiledModulationDelayParameters {
        rate_hz: processor_parameter_handle(catalog, placement, layer_id, processor_id, "rate_hz"),
        depth: processor_parameter_handle(catalog, placement, layer_id, processor_id, "depth"),
        feedback: processor_parameter_handle(
            catalog,
            placement,
            layer_id,
            processor_id,
            "feedback",
        ),
        width: processor_parameter_handle(catalog, placement, layer_id, processor_id, "width"),
        mix: processor_parameter_handle(catalog, placement, layer_id, processor_id, "mix"),
    };
    if max_delay_frames == 0 || !center_frames.is_finite() {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::CompileError,
                "modulation delay length cannot be represented",
            )
            .with_path(format!("{path}.delay_ms")),
        );
    }
    if chorus {
        CompiledProcessorKind::Chorus(CompiledChorusProcessor {
            delay_frames: center_frames,
            max_delay_frames: max_delay_frames.max(1),
            sample_rate: sample_rate_f32,
            parameters,
        })
    } else {
        CompiledProcessorKind::Flanger(CompiledFlangerProcessor {
            delay_frames: center_frames,
            max_delay_frames: max_delay_frames.max(1),
            sample_rate: sample_rate_f32,
            parameters,
        })
    }
}

fn compile_phaser_processor(
    value: &PhaserProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    path: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledProcessorKind {
    let upper_frequency = value.center_hz * 2.0_f32.powf(value.sweep_octaves / 2.0);
    validate_processor_frequency(
        upper_frequency,
        sample_rate,
        &format!("{path}.sweep_octaves"),
        diagnostics,
    );
    CompiledProcessorKind::Phaser(CompiledPhaserProcessor {
        stages: value.stages,
        center_hz: value.center_hz,
        sweep_octaves: value.sweep_octaves,
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        sample_rate: sample_rate as f32,
        parameters: CompiledPhaserParameters {
            rate_hz: processor_parameter_handle(catalog, placement, layer_id, &value.id, "rate_hz"),
            depth: processor_parameter_handle(catalog, placement, layer_id, &value.id, "depth"),
            feedback: processor_parameter_handle(
                catalog, placement, layer_id, &value.id, "feedback",
            ),
            width: processor_parameter_handle(catalog, placement, layer_id, &value.id, "width"),
            mix: processor_parameter_handle(catalog, placement, layer_id, &value.id, "mix"),
        },
    })
}

fn compile_frequency_shifter_processor(
    value: &FrequencyShifterProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    _path: &str,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    _diagnostics: &mut Vec<Diagnostic>,
) -> CompiledProcessorKind {
    CompiledProcessorKind::FrequencyShifter(CompiledFrequencyShifterProcessor {
        parameters: CompiledFrequencyShifterParameters {
            shift_hz: processor_parameter_handle(
                catalog, placement, layer_id, &value.id, "shift_hz",
            ),
            mix: processor_parameter_handle(catalog, placement, layer_id, &value.id, "mix"),
        },
        coefficients: Arc::from(build_hilbert_coefficients()),
        latency_frames: FREQUENCY_SHIFTER_LATENCY_FRAMES,
        effective_abs_shift_hz: effective_max_frequency(sample_rate, BASIC_FREQUENCY_LIMIT_RATIO),
    })
}

fn compile_compressor_processor(
    value: &CompressorProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    catalog: &ParameterCatalog,
    sample_rate: f64,
) -> CompiledProcessorKind {
    let attack_seconds = value.attack_ms / 1_000.0;
    let release_seconds = value.release_ms / 1_000.0;
    CompiledProcessorKind::Compressor(CompiledCompressorProcessor {
        attack_coeff: time_constant_coefficient(attack_seconds, sample_rate),
        release_coeff: time_constant_coefficient(release_seconds, sample_rate),
        knee_db: value.knee_db,
        detector: compile_detector(value.detector),
        external_input_alignment_frames: 0,
        parameters: CompiledCompressorParameters {
            threshold_db: processor_parameter_handle(
                catalog,
                placement,
                layer_id,
                &value.id,
                "threshold_db",
            ),
            ratio: processor_parameter_handle(catalog, placement, layer_id, &value.id, "ratio"),
            makeup_gain_db: processor_parameter_handle(
                catalog,
                placement,
                layer_id,
                &value.id,
                "makeup_gain_db",
            ),
            mix: processor_parameter_handle(catalog, placement, layer_id, &value.id, "mix"),
        },
    })
}

fn compile_limiter_processor(
    value: &LimiterProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    catalog: &ParameterCatalog,
    sample_rate: f64,
) -> CompiledProcessorKind {
    CompiledProcessorKind::Limiter(CompiledLimiterProcessor {
        release_coeff: time_constant_coefficient(value.release_ms / 1_000.0, sample_rate),
        parameters: CompiledLimiterParameters {
            ceiling_db: processor_parameter_handle(
                catalog,
                placement,
                layer_id,
                &value.id,
                "ceiling_db",
            ),
            input_gain_db: processor_parameter_handle(
                catalog,
                placement,
                layer_id,
                &value.id,
                "input_gain_db",
            ),
        },
    })
}

pub(crate) fn time_constant_coefficient(seconds: f32, sample_rate: f64) -> f32 {
    let denominator = f64::from(seconds.max(f32::MIN_POSITIVE)) * sample_rate;
    #[allow(clippy::cast_possible_truncation)]
    {
        (-1.0 / denominator).exp() as f32
    }
}

fn validate_processor_frequency(
    frequency_hz: f32,
    sample_rate: f64,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let limit = (sample_rate * 0.45) as f32;
    if !frequency_hz.is_finite() || frequency_hz >= limit {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::CompileError,
                format!("processor frequency must be below {limit:.3} Hz at this sample rate"),
            )
            .with_path(path),
        );
    }
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
    let time = compile_delay_time(value.time, path, diagnostics);
    let taps = value
        .taps
        .iter()
        .enumerate()
        .map(|(index, tap)| CompiledDelayTap {
            time: compile_delay_time(tap.time, &format!("{path}.taps[{index}]"), diagnostics),
            gain_linear: db_to_linear(tap.gain_db),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let energy = 1.0
        + taps
            .iter()
            .map(|tap| tap.gain_linear * tap.gain_linear)
            .sum::<f32>();
    let wet_normalization = if energy.is_finite() && energy > 0.0 {
        1.0 / energy.sqrt()
    } else {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::CompileError,
                "delay tap normalization is not finite",
            )
            .with_path(format!("{path}.taps")),
        );
        1.0
    };
    let max_delay_frames = processor_seconds_to_frames(
        MAX_DELAY_RUNTIME_SECONDS,
        sample_rate,
        &format!("{path}.time.value"),
        diagnostics,
        1,
    );
    CompiledProcessorKind::Delay(CompiledDelayProcessor {
        time,
        feedback_mode: value.feedback_mode,
        taps,
        max_delay_frames,
        wet_normalization,
        feedback,
        mix,
    })
}

fn compile_delay_time(
    value: DelayTimeDefinition,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledDelayTime {
    if !value.value.is_finite() || value.value <= 0.0 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::CompileError,
                "delay time must be finite and positive",
            )
            .with_path(format!("{path}.time.value")),
        );
    }
    match value.unit {
        DelayTimeUnit::Seconds => CompiledDelayTime::Seconds(f64::from(value.value)),
        DelayTimeUnit::Beats => CompiledDelayTime::Beats(f64::from(value.value)),
    }
}

#[allow(clippy::too_many_arguments)]
fn compile_convolution_processor(
    value: &ConvolutionProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    path: &str,
    catalog: &ParameterCatalog,
    definition_base_dir: &Path,
    asset_cache: &mut HashMap<AssetCacheKey, Result<PreparedAsset, AssetError>>,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledProcessorKind {
    let prepared_ir =
        match prepare_cached_asset(&value.ir, definition_base_dir, sample_rate, asset_cache) {
            Ok(asset) => match prepare_convolution_ir(&asset.audio) {
                Ok(ir) => Arc::new(ir),
                Err(error) => {
                    diagnostics.push(
                        Diagnostic::error(DiagnosticCode::CompileError, error)
                            .with_path(format!("{path}.ir")),
                    );
                    Arc::new(PreparedConvolutionIr::empty(sample_rate))
                }
            },
            Err(error) => {
                let (code, message) = asset_diagnostic(&error);
                diagnostics
                    .push(Diagnostic::error(code, message).with_path(format!("{path}.ir.path")));
                Arc::new(PreparedConvolutionIr::empty(sample_rate))
            }
        };
    CompiledProcessorKind::Convolution(CompiledConvolutionProcessor {
        prepared_ir,
        asset_path: value.ir.path.clone(),
        asset_sha256_specified: value.ir.sha256.is_some(),
        parameters: CompiledConvolutionParameters {
            gain_db: processor_parameter_handle(catalog, placement, layer_id, &value.id, "gain_db"),
            mix: processor_parameter_handle(catalog, placement, layer_id, &value.id, "mix"),
        },
        latency_frames: crate::compiler::convolution::CONVOLUTION_LATENCY_FRAMES,
    })
}

fn compile_gate_processor(
    value: &GateProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    catalog: &ParameterCatalog,
    sample_rate: f64,
) -> CompiledProcessorKind {
    CompiledProcessorKind::Gate(CompiledGateProcessor {
        hysteresis_db: value.hysteresis_db,
        detector_attack_coeff: time_constant_coefficient(0.001, sample_rate),
        detector_release_coeff: time_constant_coefficient(0.020, sample_rate),
        attack_coeff: time_constant_coefficient(value.attack_ms / 1_000.0, sample_rate),
        hold_frames: duration_to_frames(value.hold_ms / 1_000.0, sample_rate),
        release_coeff: time_constant_coefficient(value.release_ms / 1_000.0, sample_rate),
        detector: compile_detector(value.detector),
        external_input_alignment_frames: 0,
        parameters: CompiledGateParameters {
            threshold_db: processor_parameter_handle(
                catalog,
                placement,
                layer_id,
                &value.id,
                "threshold_db",
            ),
            range_db: processor_parameter_handle(
                catalog, placement, layer_id, &value.id, "range_db",
            ),
        },
    })
}

fn compile_detector(detector: DynamicsDetectorDefinition) -> CompiledDynamicsDetector {
    match detector {
        DynamicsDetectorDefinition::SelfSignal => CompiledDynamicsDetector::SelfSignal,
        DynamicsDetectorDefinition::ExternalAudio => CompiledDynamicsDetector::ExternalAudio,
    }
}

fn compile_vocoder_processor(
    value: &VocoderProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    catalog: &ParameterCatalog,
    sample_rate: f64,
) -> CompiledProcessorKind {
    CompiledProcessorKind::Vocoder(CompiledVocoderProcessor {
        attack_coeff: time_constant_coefficient(value.attack_ms / 1_000.0, sample_rate),
        release_coeff: time_constant_coefficient(value.release_ms / 1_000.0, sample_rate),
        parameters: CompiledVocoderParameters {
            modulator_gain_db: processor_parameter_handle(
                catalog,
                placement,
                layer_id,
                &value.id,
                "modulator_gain_db",
            ),
            output_gain_db: processor_parameter_handle(
                catalog,
                placement,
                layer_id,
                &value.id,
                "output_gain_db",
            ),
            mix: processor_parameter_handle(catalog, placement, layer_id, &value.id, "mix"),
        },
        external_input_alignment_frames: 0,
    })
}

fn compile_envelope_transfer_processor(
    value: &EnvelopeTransferProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    catalog: &ParameterCatalog,
    sample_rate: f64,
) -> CompiledProcessorKind {
    CompiledProcessorKind::EnvelopeTransfer(CompiledEnvelopeTransferProcessor {
        attack_coeff: time_constant_coefficient(value.attack_ms / 1_000.0, sample_rate),
        release_coeff: time_constant_coefficient(value.release_ms / 1_000.0, sample_rate),
        parameters: CompiledEnvelopeTransferParameters {
            input_gain_db: processor_parameter_handle(
                catalog,
                placement,
                layer_id,
                &value.id,
                "input_gain_db",
            ),
            floor_db: processor_parameter_handle(
                catalog, placement, layer_id, &value.id, "floor_db",
            ),
            mix: processor_parameter_handle(catalog, placement, layer_id, &value.id, "mix"),
        },
        external_input_alignment_frames: 0,
    })
}

fn compile_spectral_morph_processor(
    value: &SpectralMorphProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    catalog: &ParameterCatalog,
) -> CompiledProcessorKind {
    CompiledProcessorKind::SpectralMorph(CompiledSpectralMorphProcessor {
        parameters: CompiledSpectralMorphParameters {
            morph: processor_parameter_handle(catalog, placement, layer_id, &value.id, "morph"),
            output_gain_db: processor_parameter_handle(
                catalog,
                placement,
                layer_id,
                &value.id,
                "output_gain_db",
            ),
        },
        external_input_alignment_frames: 0,
    })
}

fn compile_transient_shaper_processor(
    value: &TransientShaperProcessorDefinition,
    placement: ProcessorPlacement,
    layer_id: Option<&str>,
    catalog: &ParameterCatalog,
    sample_rate: f64,
) -> CompiledProcessorKind {
    CompiledProcessorKind::TransientShaper(CompiledTransientShaperProcessor {
        fast_attack_coeff: time_constant_coefficient(0.001, sample_rate),
        fast_release_coeff: time_constant_coefficient(0.020, sample_rate),
        slow_attack_coeff: time_constant_coefficient(0.020, sample_rate),
        slow_release_coeff: time_constant_coefficient(0.200, sample_rate),
        parameters: CompiledTransientShaperParameters {
            attack: processor_parameter_handle(catalog, placement, layer_id, &value.id, "attack"),
            sustain: processor_parameter_handle(catalog, placement, layer_id, &value.id, "sustain"),
            mix: processor_parameter_handle(catalog, placement, layer_id, &value.id, "mix"),
        },
    })
}

fn duration_to_frames(seconds: f32, sample_rate: f64) -> usize {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        (f64::from(seconds) * sample_rate).round().max(0.0) as usize
    }
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

#[cfg(test)]
mod tests {
    use super::super::tests::{context, definition};
    use super::{
        CompiledProcessorKind, FREQUENCY_SHIFTER_LATENCY_FRAMES, HILBERT_TAPS, ProcessorDefinition,
        build_hilbert_coefficients,
    };
    use super::{ReverbOutputTap, ReverbTapSource};
    use crate::definition::{
        AssetReference, ConvolutionProcessorDefinition, DelayFeedbackMode,
        DelayProcessorDefinition, DelayTimeDefinition, DelayTimeUnit,
        FrequencyShifterProcessorDefinition, GateProcessorDefinition,
        LadderFilterProcessorDefinition, ModulationCurve, ReverbProcessorDefinition,
        TransientShaperProcessorDefinition,
    };
    use crate::diagnostics::DiagnosticCode;
    use crate::{CompileContext, DiagnosticSeverity, ProcessSpec, compile_instrument};

    #[test]
    fn hilbert_coefficients_are_finite_and_anti_symmetric() {
        let coefficients = build_hilbert_coefficients();

        assert_eq!(coefficients.len(), HILBERT_TAPS);
        assert!(
            coefficients
                .iter()
                .all(|coefficient| coefficient.is_finite())
        );
        assert!(coefficients[HILBERT_TAPS / 2].abs() < 1.0e-7);
        for index in 0..HILBERT_TAPS / 2 {
            assert!((coefficients[index] + coefficients[HILBERT_TAPS - 1 - index]).abs() < 1.0e-7);
        }
    }

    #[test]
    fn processor_chains_compile_in_definition_order() {
        let mut source = definition();
        source.layers[0].processors = vec![
            ProcessorDefinition::Filter(crate::definition::FilterProcessorDefinition {
                id: "layer_tone".to_owned(),
                mode: crate::definition::FilterModeDefinition::LowPass,
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
                time: crate::definition::DelayTimeDefinition {
                    value: 0.2,
                    unit: crate::definition::DelayTimeUnit::Seconds,
                },
                feedback_mode: crate::definition::DelayFeedbackMode::Stereo,
                feedback: 0.3,
                taps: vec![],
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
    fn extended_processors_compile_with_parameter_handles_and_fixed_latency() {
        let mut source = definition();
        source.layers[0]
            .processors
            .push(ProcessorDefinition::LadderFilter(
                LadderFilterProcessorDefinition {
                    id: "ladder".to_owned(),
                    cutoff_hz: 850.0,
                    resonance: 0.7,
                    drive: 0.3,
                },
            ));
        source.voice_processors = vec![
            ProcessorDefinition::Gate(GateProcessorDefinition {
                id: "gate".to_owned(),
                threshold_db: -35.0,
                hysteresis_db: 4.0,
                attack_ms: 2.0,
                hold_ms: 35.0,
                release_ms: 90.0,
                range_db: -72.0,
                detector: crate::definition::DynamicsDetectorDefinition::SelfSignal,
            }),
            ProcessorDefinition::TransientShaper(TransientShaperProcessorDefinition {
                id: "shape".to_owned(),
                attack: 0.5,
                sustain: -0.3,
                mix: 1.0,
            }),
        ];
        source.global_processors = vec![
            ProcessorDefinition::FrequencyShifter(FrequencyShifterProcessorDefinition {
                id: "shift".to_owned(),
                shift_hz: 420.0,
                mix: 0.7,
            }),
            ProcessorDefinition::Delay(DelayProcessorDefinition {
                id: "echo".to_owned(),
                time: DelayTimeDefinition {
                    value: 0.75,
                    unit: DelayTimeUnit::Beats,
                },
                feedback_mode: DelayFeedbackMode::PingPong,
                feedback: 0.45,
                taps: vec![],
                mix: 0.35,
            }),
        ];

        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("extended processors compile");

        assert!(result.diagnostics.is_empty());
        assert_eq!(compiled.layer_alignment_latency_frames, 0);
        assert_eq!(
            compiled.reported_latency_frames,
            FREQUENCY_SHIFTER_LATENCY_FRAMES
        );
        assert!(matches!(
            compiled.layers[0].processors[0].processor,
            CompiledProcessorKind::LadderFilter(_)
        ));
        assert!(matches!(
            compiled.voice_processors[0].processor,
            CompiledProcessorKind::Gate(_)
        ));
        assert!(matches!(
            compiled.voice_processors[1].processor,
            CompiledProcessorKind::TransientShaper(_)
        ));
        let CompiledProcessorKind::FrequencyShifter(shifter) =
            &compiled.global_processors[0].processor
        else {
            panic!("global processor must be a frequency shifter");
        };
        assert_eq!(
            compiled
                .parameter_handle("global.processor.shift.shift_hz")
                .expect("frequency shift handle")
                .index(),
            shifter.parameters.shift_hz.index()
        );
    }

    #[test]
    fn missing_convolution_ir_is_a_compile_error() {
        let mut source = definition();
        source
            .global_processors
            .push(ProcessorDefinition::Convolution(
                ConvolutionProcessorDefinition {
                    id: "body".to_owned(),
                    ir: AssetReference {
                        path: "missing-body.wav".to_owned(),
                        sha256: None,
                    },
                    gain_db: 0.0,
                    mix: 1.0,
                },
            ));

        let result = compile_instrument(&source, &context());

        assert!(result.instrument.is_none());
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Error
                && diagnostic.path.as_deref() == Some("global_processors[0].ir.path")
        }));
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
            process_spec: ProcessSpec::new(29_761.0, 257, 0, 2).expect("valid spec"),
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
                depth: crate::definition::ModulationDepthDefinition {
                    value: 0.2,
                    unit: crate::parameter::ModulationUnit::Normalized,
                },
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
}
