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

pub(crate) fn is_suffix(value: &str) -> bool {
    [
        PULSE_WIDTH,
        SYNC_RATIO,
        WAVESHAPE,
        WAVETABLE_POSITION,
        UNISON_DETUNE,
        UNISON_SPREAD,
        NOISE_CORRELATION,
    ]
    .into_iter()
    .any(|spec| spec.suffix == value)
}
