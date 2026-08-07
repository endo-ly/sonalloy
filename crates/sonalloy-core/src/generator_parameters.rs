use crate::parameter::{ParameterScale, ParameterUnit};

/// Static contract for a generator parameter exposed through the modulation catalog.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GeneratorParameterSpec {
    pub(crate) suffix: &'static str,
    pub(crate) unit: ParameterUnit,
    pub(crate) scale: ParameterScale,
    pub(crate) min: f32,
    pub(crate) max: f32,
    pub(crate) smoothing_seconds: f32,
}

pub(crate) const PULSE_WIDTH: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "pulse_width",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.05,
    max: 0.95,
    smoothing_seconds: 0.005,
};

pub(crate) const SYNC_RATIO: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "sync_ratio",
    unit: ParameterUnit::Ratio,
    scale: ParameterScale::Log2,
    min: 1.0,
    max: 16.0,
    smoothing_seconds: 0.005,
};

pub(crate) const WAVESHAPE: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "waveshape",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.005,
};

pub(crate) const PHASE_DISTORTION: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "phase_distortion",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.005,
};

pub(crate) const WAVEFOLD: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "wavefold",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.005,
};

pub(crate) const OSCILLATOR_FEEDBACK: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "oscillator_feedback",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.005,
};

pub(crate) const WAVETABLE_POSITION: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "wavetable_position",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const UNISON_DETUNE: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "unison_detune",
    unit: ParameterUnit::Cents,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 100.0,
    smoothing_seconds: 0.010,
};

pub(crate) const UNISON_SPREAD: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "unison_spread",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const NOISE_CORRELATION: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "noise_correlation",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const OPERATOR_RATIO_MIN: f32 = 0.25;
pub(crate) const OPERATOR_RATIO_MAX: f32 = 32.0;
pub(crate) const OPERATOR_DETUNE_MIN: f32 = -100.0;
pub(crate) const OPERATOR_DETUNE_MAX: f32 = 100.0;
pub(crate) const OPERATOR_LEVEL_MIN: f32 = 0.0;
pub(crate) const OPERATOR_LEVEL_MAX: f32 = 1.0;
pub(crate) const OPERATOR_PHASE_MIN: f32 = 0.0;
pub(crate) const OPERATOR_PHASE_MAX: f32 = 1.0;
pub(crate) const OPERATOR_PHASE_FREQUENCY_AMOUNT_MIN: f32 = 0.0;
pub(crate) const OPERATOR_PHASE_FREQUENCY_AMOUNT_MAX: f32 = 8.0;
pub(crate) const OPERATOR_AM_RING_AMOUNT_MIN: f32 = 0.0;
pub(crate) const OPERATOR_AM_RING_AMOUNT_MAX: f32 = 1.0;
pub(crate) const OPERATOR_FEEDBACK_MIN: f32 = 0.0;
pub(crate) const OPERATOR_FEEDBACK_MAX: f32 = 1.0;
pub(crate) const OPERATOR_PARAMETER_SMOOTHING_SECONDS: f32 = 0.005;
pub(crate) const OPERATOR_PARAMETER_SUFFIXES: [&str; 5] =
    ["ratio", "detune", "level", "modulation_amount", "feedback"];

pub(crate) const BASIC_FREQUENCY_LIMIT_RATIO: f64 = 0.45;
pub(crate) const PHASE_DOMAIN_FREQUENCY_LIMIT_RATIO: f64 = 0.24;

pub(crate) fn effective_max_frequency(sample_rate: f64, ratio: f64) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    {
        (sample_rate * ratio) as f32
    }
}

pub(crate) fn is_suffix(value: &str) -> bool {
    [
        PULSE_WIDTH,
        SYNC_RATIO,
        WAVESHAPE,
        PHASE_DISTORTION,
        WAVEFOLD,
        OSCILLATOR_FEEDBACK,
        WAVETABLE_POSITION,
        UNISON_DETUNE,
        UNISON_SPREAD,
        NOISE_CORRELATION,
    ]
    .into_iter()
    .any(|spec| spec.suffix == value)
}
