use super::generator::validate_formant_profiles;
#[allow(clippy::wildcard_imports)]
use super::*;

/// Processor definitions supported by the fixed signal pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProcessorDefinition {
    /// State-variable filter.
    Filter(FilterProcessorDefinition),
    /// Four-pole nonlinear low-pass ladder filter.
    LadderFilter(LadderFilterProcessorDefinition),
    /// Soft-clipping drive.
    Drive(DriveProcessorDefinition),
    /// Fixed three-band equalizer.
    Eq(EqProcessorDefinition),
    /// Five-band formant filter bank.
    Formant(FormantProcessorDefinition),
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
    /// Global frequency translation effect.
    FrequencyShifter(FrequencyShifterProcessorDefinition),
    /// Stereo feedback delay.
    Delay(DelayProcessorDefinition),
    /// Stereo plate reverb.
    Reverb(ReverbProcessorDefinition),
    /// Global impulse-response convolution effect.
    Convolution(ConvolutionProcessorDefinition),
    /// Stereo-linked self-keyed gate.
    Gate(GateProcessorDefinition),
    /// Global filter-bank vocoder driven by external audio.
    Vocoder(VocoderProcessorDefinition),
    /// Global amplitude-envelope transfer driven by external audio.
    EnvelopeTransfer(EnvelopeTransferProcessorDefinition),
    /// Global streaming spectral magnitude morph driven by external audio.
    SpectralMorph(SpectralMorphProcessorDefinition),
    /// Stereo-linked transient and sustain shaper.
    TransientShaper(TransientShaperProcessorDefinition),
    /// Stereo-linked compressor.
    Compressor(CompressorProcessorDefinition),
    /// Zero-latency stereo-linked limiter.
    Limiter(LimiterProcessorDefinition),
}

impl ProcessorDefinition {
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Filter(value) => &value.id,
            Self::LadderFilter(value) => &value.id,
            Self::Drive(value) => &value.id,
            Self::Eq(value) => &value.id,
            Self::Formant(value) => &value.id,
            Self::Resonator(value) => &value.id,
            Self::Bitcrusher(value) => &value.id,
            Self::Chorus(value) => &value.id,
            Self::Flanger(value) => &value.id,
            Self::Phaser(value) => &value.id,
            Self::FrequencyShifter(value) => &value.id,
            Self::Delay(value) => &value.id,
            Self::Reverb(value) => &value.id,
            Self::Convolution(value) => &value.id,
            Self::Gate(value) => &value.id,
            Self::Vocoder(value) => &value.id,
            Self::EnvelopeTransfer(value) => &value.id,
            Self::SpectralMorph(value) => &value.id,
            Self::TransientShaper(value) => &value.id,
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

/// Four-pole nonlinear low-pass ladder filter settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LadderFilterProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Cutoff frequency in Hz.
    pub cutoff_hz: f32,
    /// Normalized resonance.
    pub resonance: f32,
    /// Normalized input drive.
    pub drive: f32,
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

/// Five-band formant filter bank settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormantProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Position between the first and last formant profiles.
    pub vowel_position: f32,
    /// Frequency shift applied to formant centers and bandwidths in cents.
    pub formant_shift_cents: f32,
    /// Formant bandwidth control.
    pub throat: f32,
    /// Ordered formant profiles.
    pub profiles: Vec<FormantProfileDefinition>,
    /// Dry/wet mix.
    pub mix: f32,
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

/// Frequency shifter settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrequencyShifterProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Constant frequency offset in Hz.
    pub shift_hz: f32,
    /// Dry/wet mix.
    pub mix: f32,
}

/// Convolution processor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvolutionProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Impulse-response asset.
    pub ir: AssetReference,
    /// Wet gain in decibels.
    pub gain_db: f32,
    /// Dry/wet mix.
    pub mix: f32,
}

/// Stereo delay processor settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelayProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Primary delay time.
    pub time: DelayTimeDefinition,
    /// Feedback routing mode.
    pub feedback_mode: DelayFeedbackMode,
    /// Feedback amount.
    pub feedback: f32,
    /// Additional feed-forward taps.
    pub taps: Vec<DelayTapDefinition>,
    /// Dry/wet mix.
    pub mix: f32,
}

/// Unit used by a delay time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelayTimeUnit {
    /// Wall-clock seconds.
    Seconds,
    /// Quarter-note beats.
    Beats,
}

/// Delay time written in seconds or quarter-note beats.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelayTimeDefinition {
    /// Time value in the selected unit.
    pub value: f32,
    /// Time unit.
    pub unit: DelayTimeUnit,
}

/// Feedback routing mode for a stereo delay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelayFeedbackMode {
    /// Feed each channel back into itself.
    Stereo,
    /// Cross-feed the delayed channels.
    PingPong,
}

/// One feed-forward delay tap.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelayTapDefinition {
    /// Tap time.
    pub time: DelayTimeDefinition,
    /// Tap gain in decibels.
    pub gain_db: f32,
}

/// Self-keyed stereo gate settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Opening threshold in decibels.
    pub threshold_db: f32,
    /// Opening/closing hysteresis in decibels.
    pub hysteresis_db: f32,
    /// Opening attack in milliseconds.
    pub attack_ms: f32,
    /// Hold time in milliseconds.
    pub hold_ms: f32,
    /// Closing release in milliseconds.
    pub release_ms: f32,
    /// Closed-state attenuation in decibels.
    pub range_db: f32,
    /// Signal used by the detector.
    pub detector: DynamicsDetectorDefinition,
}

/// Signal source used by a dynamics detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicsDetectorDefinition {
    /// Detect the signal currently passing through the processor.
    SelfSignal,
    /// Detect the aligned external audio bus.
    ExternalAudio,
}

/// Transient and sustain shaping settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransientShaperProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Attack emphasis or reduction.
    pub attack: f32,
    /// Sustain emphasis or reduction.
    pub sustain: f32,
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
    /// Signal used by the detector.
    pub detector: DynamicsDetectorDefinition,
}

/// Global fixed-band vocoder settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VocoderProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Analyzer attack time in milliseconds.
    pub attack_ms: f32,
    /// Analyzer release time in milliseconds.
    pub release_ms: f32,
    /// External modulator gain in decibels.
    pub modulator_gain_db: f32,
    /// Wet output gain in decibels.
    pub output_gain_db: f32,
    /// Dry/wet mix.
    pub mix: f32,
}

/// Global amplitude-envelope transfer settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvelopeTransferProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Envelope attack time in milliseconds.
    pub attack_ms: f32,
    /// Envelope release time in milliseconds.
    pub release_ms: f32,
    /// External input gain in decibels.
    pub input_gain_db: f32,
    /// Minimum wet gain in decibels.
    pub floor_db: f32,
    /// Dry/wet mix.
    pub mix: f32,
}

/// Global fixed-size streaming spectral magnitude morph settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpectralMorphProcessorDefinition {
    /// Stable processor identifier.
    pub id: String,
    /// Magnitude interpolation from carrier to external audio.
    pub morph: f32,
    /// Output gain in decibels.
    pub output_gain_db: f32,
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

#[derive(Clone, Copy)]
pub(super) enum ProcessorPlacement {
    Layer,
    Voice,
    Global,
}

pub(super) fn validate_processor_chain(
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
                | ProcessorDefinition::LadderFilter(_)
                | ProcessorDefinition::Drive(_)
                | ProcessorDefinition::Eq(_)
                | ProcessorDefinition::Formant(_)
                | ProcessorDefinition::Resonator(_)
                | ProcessorDefinition::Bitcrusher(_)
        ),
        ProcessorPlacement::Voice => matches!(
            processor,
            ProcessorDefinition::Filter(_)
                | ProcessorDefinition::LadderFilter(_)
                | ProcessorDefinition::Drive(_)
                | ProcessorDefinition::Eq(_)
                | ProcessorDefinition::Formant(_)
                | ProcessorDefinition::Resonator(_)
                | ProcessorDefinition::Gate(GateProcessorDefinition {
                    detector: DynamicsDetectorDefinition::SelfSignal,
                    ..
                })
                | ProcessorDefinition::TransientShaper(_)
                | ProcessorDefinition::Compressor(CompressorProcessorDefinition {
                    detector: DynamicsDetectorDefinition::SelfSignal,
                    ..
                })
                | ProcessorDefinition::Limiter(_)
        ),
        ProcessorPlacement::Global => matches!(
            processor,
            ProcessorDefinition::Filter(_)
                | ProcessorDefinition::LadderFilter(_)
                | ProcessorDefinition::Drive(_)
                | ProcessorDefinition::Eq(_)
                | ProcessorDefinition::Formant(_)
                | ProcessorDefinition::Chorus(_)
                | ProcessorDefinition::Flanger(_)
                | ProcessorDefinition::Phaser(_)
                | ProcessorDefinition::FrequencyShifter(_)
                | ProcessorDefinition::Delay(_)
                | ProcessorDefinition::Reverb(_)
                | ProcessorDefinition::Convolution(_)
                | ProcessorDefinition::Gate(_)
                | ProcessorDefinition::TransientShaper(_)
                | ProcessorDefinition::Compressor(_)
                | ProcessorDefinition::Limiter(_)
                | ProcessorDefinition::Vocoder(_)
                | ProcessorDefinition::EnvelopeTransfer(_)
                | ProcessorDefinition::SpectralMorph(_)
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
        ProcessorDefinition::LadderFilter(_) => "ladder_filter",
        ProcessorDefinition::Drive(_) => "drive",
        ProcessorDefinition::Eq(_) => "eq",
        ProcessorDefinition::Formant(_) => "formant",
        ProcessorDefinition::Resonator(_) => "resonator",
        ProcessorDefinition::Bitcrusher(_) => "bitcrusher",
        ProcessorDefinition::Chorus(_) => "chorus",
        ProcessorDefinition::Flanger(_) => "flanger",
        ProcessorDefinition::Phaser(_) => "phaser",
        ProcessorDefinition::FrequencyShifter(_) => "frequency_shifter",
        ProcessorDefinition::Delay(_) => "delay",
        ProcessorDefinition::Reverb(_) => "reverb",
        ProcessorDefinition::Convolution(_) => "convolution",
        ProcessorDefinition::Gate(_) => "gate",
        ProcessorDefinition::Vocoder(_) => "vocoder",
        ProcessorDefinition::EnvelopeTransfer(_) => "envelope_transfer",
        ProcessorDefinition::SpectralMorph(_) => "spectral_morph",
        ProcessorDefinition::TransientShaper(_) => "transient_shaper",
        ProcessorDefinition::Compressor(_) => "compressor",
        ProcessorDefinition::Limiter(_) => "limiter",
    }
}

pub(super) fn validate_processor_resource_limits(
    diagnostics: &mut Vec<Diagnostic>,
    processors: &[ProcessorDefinition],
) {
    let convolution_count = processors
        .iter()
        .filter(|processor| matches!(processor, ProcessorDefinition::Convolution(_)))
        .count();
    if convolution_count > MAX_CONVOLUTION_PROCESSORS {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::GeneratorResourceLimitExceeded,
                format!("global processors may contain at most {MAX_CONVOLUTION_PROCESSORS} convolution processors"),
            )
            .with_path("global_processors"),
        );
    }
    let delay_count = processors
        .iter()
        .filter(|processor| matches!(processor, ProcessorDefinition::Delay(_)))
        .count();
    if delay_count > MAX_DELAY_PROCESSORS {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::GeneratorResourceLimitExceeded,
                format!(
                    "global processors may contain at most {MAX_DELAY_PROCESSORS} delay processors"
                ),
            )
            .with_path("global_processors"),
        );
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
        ProcessorDefinition::LadderFilter(value) => {
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
            validate_range(
                diagnostics,
                format!("{path}.drive"),
                value.drive,
                0.0..=1.0,
                "drive must be finite and between 0 and 1",
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
        ProcessorDefinition::Formant(value) => {
            validate_range(
                diagnostics,
                format!("{path}.vowel_position"),
                value.vowel_position,
                FORMANT_VOWEL_POSITION.min..=FORMANT_VOWEL_POSITION.max,
                "vowel_position must be finite and between 0 and 1",
            );
            validate_range(
                diagnostics,
                format!("{path}.formant_shift_cents"),
                value.formant_shift_cents,
                FORMANT_SHIFT.min..=FORMANT_SHIFT.max,
                "formant_shift_cents must be finite and between -2400 and 2400",
            );
            validate_range(
                diagnostics,
                format!("{path}.throat"),
                value.throat,
                FORMANT_THROAT.min..=FORMANT_THROAT.max,
                "throat must be finite and between 0 and 1",
            );
            validate_range(
                diagnostics,
                format!("{path}.mix"),
                value.mix,
                0.0..=1.0,
                "mix must be finite and between 0 and 1",
            );
            validate_formant_profiles(diagnostics, &format!("{path}.profiles"), &value.profiles);
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
        ProcessorDefinition::FrequencyShifter(value) => {
            validate_range(
                diagnostics,
                format!("{path}.shift_hz"),
                value.shift_hz,
                -5_000.0..=5_000.0,
                "shift_hz must be finite and between -5000 and 5000 Hz",
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
                format!("{path}.time.value"),
                value.time.value,
                match value.time.unit {
                    DelayTimeUnit::Seconds => 0.001..=8.0,
                    DelayTimeUnit::Beats => 0.015_625..=2.0,
                },
                "delay time must be finite and within the supported range",
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
            if value.taps.len() > MAX_DELAY_TAPS {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::GeneratorResourceLimitExceeded,
                        format!("delay taps must contain at most {MAX_DELAY_TAPS} entries"),
                    )
                    .with_path(format!("{path}.taps")),
                );
            }
            for (index, tap) in value.taps.iter().enumerate() {
                validate_range(
                    diagnostics,
                    format!("{path}.taps[{index}].time.value"),
                    tap.time.value,
                    match tap.time.unit {
                        DelayTimeUnit::Seconds => 0.001..=8.0,
                        DelayTimeUnit::Beats => 0.015_625..=2.0,
                    },
                    "delay tap time must be finite and within the supported range",
                );
                validate_range(
                    diagnostics,
                    format!("{path}.taps[{index}].gain_db"),
                    tap.gain_db,
                    -24.0..=0.0,
                    "delay tap gain must be finite and between -24 and 0 dB",
                );
            }
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
        ProcessorDefinition::Convolution(value) => {
            validate_range(
                diagnostics,
                format!("{path}.gain_db"),
                value.gain_db,
                -24.0..=24.0,
                "gain_db must be finite and between -24 and 24 dB",
            );
            validate_range(
                diagnostics,
                format!("{path}.mix"),
                value.mix,
                0.0..=1.0,
                "mix must be finite and between 0 and 1",
            );
            if value.ir.path.trim().is_empty() {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::RequiredFieldMissing,
                        "convolution IR path must not be empty",
                    )
                    .with_path(format!("{path}.ir.path")),
                );
            }
        }
        ProcessorDefinition::Gate(value) => {
            validate_range(
                diagnostics,
                format!("{path}.threshold_db"),
                value.threshold_db,
                -80.0..=0.0,
                "threshold_db must be finite and between -80 and 0 dB",
            );
            validate_range(
                diagnostics,
                format!("{path}.hysteresis_db"),
                value.hysteresis_db,
                0.0..=12.0,
                "hysteresis_db must be finite and between 0 and 12 dB",
            );
            validate_range(
                diagnostics,
                format!("{path}.attack_ms"),
                value.attack_ms,
                0.1..=100.0,
                "attack_ms must be finite and between 0.1 and 100 ms",
            );
            validate_range(
                diagnostics,
                format!("{path}.hold_ms"),
                value.hold_ms,
                0.0..=500.0,
                "hold_ms must be finite and between 0 and 500 ms",
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
                format!("{path}.range_db"),
                value.range_db,
                -96.0..=0.0,
                "range_db must be finite and between -96 and 0 dB",
            );
        }
        ProcessorDefinition::Vocoder(value) => {
            validate_range(
                diagnostics,
                format!("{path}.attack_ms"),
                value.attack_ms,
                0.1..=100.0,
                "attack_ms must be finite and between 0.1 and 100 ms",
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
                format!("{path}.modulator_gain_db"),
                value.modulator_gain_db,
                -24.0..=24.0,
                "modulator_gain_db must be finite and between -24 and 24 dB",
            );
            validate_range(
                diagnostics,
                format!("{path}.output_gain_db"),
                value.output_gain_db,
                -24.0..=24.0,
                "output_gain_db must be finite and between -24 and 24 dB",
            );
            validate_range(
                diagnostics,
                format!("{path}.mix"),
                value.mix,
                0.0..=1.0,
                "mix must be finite and between 0 and 1",
            );
        }
        ProcessorDefinition::EnvelopeTransfer(value) => {
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
                1.0..=2_000.0,
                "release_ms must be finite and between 1 and 2000 ms",
            );
            validate_range(
                diagnostics,
                format!("{path}.input_gain_db"),
                value.input_gain_db,
                -24.0..=24.0,
                "input_gain_db must be finite and between -24 and 24 dB",
            );
            validate_range(
                diagnostics,
                format!("{path}.floor_db"),
                value.floor_db,
                -96.0..=0.0,
                "floor_db must be finite and between -96 and 0 dB",
            );
            validate_range(
                diagnostics,
                format!("{path}.mix"),
                value.mix,
                0.0..=1.0,
                "mix must be finite and between 0 and 1",
            );
        }
        ProcessorDefinition::SpectralMorph(value) => {
            validate_range(
                diagnostics,
                format!("{path}.morph"),
                value.morph,
                0.0..=1.0,
                "morph must be finite and between 0 and 1",
            );
            validate_range(
                diagnostics,
                format!("{path}.output_gain_db"),
                value.output_gain_db,
                -24.0..=24.0,
                "output_gain_db must be finite and between -24 and 24 dB",
            );
        }
        ProcessorDefinition::TransientShaper(value) => {
            validate_range(
                diagnostics,
                format!("{path}.attack"),
                value.attack,
                -1.0..=1.0,
                "attack must be finite and between -1 and 1",
            );
            validate_range(
                diagnostics,
                format!("{path}.sustain"),
                value.sustain,
                -1.0..=1.0,
                "sustain must be finite and between -1 and 1",
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

#[cfg(test)]
mod tests {
    use crate::definition::tests::definition;

    use super::super::tests::{convolution_processor, extended_definition, seconds_delay};
    use super::*;

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
                time: DelayTimeDefinition {
                    value: 0.2,
                    unit: DelayTimeUnit::Seconds,
                },
                feedback_mode: DelayFeedbackMode::Stereo,
                feedback: 0.3,
                taps: vec![],
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
    fn extended_processors_use_the_declared_placement_matrix() {
        let value = extended_definition();
        assert!(value.validate().is_empty());

        let mut invalid_layer = value.clone();
        invalid_layer.layers[0]
            .processors
            .push(ProcessorDefinition::FrequencyShifter(
                FrequencyShifterProcessorDefinition {
                    id: "invalid_shift".to_owned(),
                    shift_hz: 100.0,
                    mix: 0.5,
                },
            ));
        let diagnostics = invalid_layer.validate();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::ProcessorPlacementInvalid
                && diagnostic.path.as_deref() == Some("layers[0].processors[2]")
        }));

        let mut invalid_voice = value;
        invalid_voice
            .voice_processors
            .push(ProcessorDefinition::Convolution(
                ConvolutionProcessorDefinition {
                    id: "invalid_body".to_owned(),
                    ir: AssetReference {
                        path: "assets/body.wav".to_owned(),
                        sha256: None,
                    },
                    gain_db: 0.0,
                    mix: 0.5,
                },
            ));
        let diagnostics = invalid_voice.validate();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::ProcessorPlacementInvalid
                && diagnostic.path.as_deref() == Some("voice_processors[2]")
        }));
    }

    #[test]
    fn processor_resource_limits_are_validation_errors() {
        let mut delays = definition();
        delays.global_processors = (0..5)
            .map(|index| seconds_delay(&format!("delay_{index}")))
            .collect();
        assert!(delays.validate().iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::GeneratorResourceLimitExceeded
                && diagnostic.path.as_deref() == Some("global_processors")
        }));

        let mut taps = definition();
        let ProcessorDefinition::Delay(delay) = seconds_delay("taps") else {
            unreachable!("test helper returns a delay");
        };
        taps.global_processors = vec![ProcessorDefinition::Delay(DelayProcessorDefinition {
            taps: [
                (0.1, 0.0),
                (0.11, -1.0),
                (0.12, -2.0),
                (0.13, -3.0),
                (0.14, -4.0),
                (0.15, -5.0),
                (0.16, -6.0),
                (0.17, -7.0),
                (0.18, -8.0),
            ]
            .into_iter()
            .map(|(value, gain_db)| DelayTapDefinition {
                time: DelayTimeDefinition {
                    value,
                    unit: DelayTimeUnit::Seconds,
                },
                gain_db,
            })
            .collect(),
            ..delay
        })];
        assert!(taps.validate().iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::GeneratorResourceLimitExceeded
                && diagnostic.path.as_deref() == Some("global_processors[0].taps")
        }));

        let mut convolutions = definition();
        convolutions.global_processors = (0..3)
            .map(|index| convolution_processor(&format!("body_{index}")))
            .collect();
        assert!(convolutions.validate().iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::GeneratorResourceLimitExceeded
                && diagnostic.path.as_deref() == Some("global_processors")
        }));
    }

    #[test]
    fn schema_rejects_the_old_delay_shape() {
        let mut value = serde_json::to_value(definition()).expect("definition serializes");
        value["global_processors"] = serde_json::json!([{
            "type": "delay",
            "id": "echo",
            "time_seconds": 0.3,
            "feedback": 0.4,
            "mix": 0.3,
        }]);

        assert!(serde_json::from_value::<InstrumentDefinition>(value).is_err());
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
}
