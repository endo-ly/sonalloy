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

pub(crate) const ADDITIVE_MORPH: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "additive_morph",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const ADDITIVE_SPECTRUM_TILT: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "additive_spectrum_tilt",
    unit: ParameterUnit::DecibelsPerOctave,
    scale: ParameterScale::Linear,
    min: -24.0,
    max: 12.0,
    smoothing_seconds: 0.010,
};

pub(crate) const ADDITIVE_INHARMONICITY: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "additive_inharmonicity",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const FORMANT_VOWEL_POSITION: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "formant_vowel_position",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const FORMANT_SHIFT: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "formant_shift",
    unit: ParameterUnit::Cents,
    scale: ParameterScale::Linear,
    min: -2400.0,
    max: 2400.0,
    smoothing_seconds: 0.010,
};

pub(crate) const FORMANT_THROAT: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "formant_throat",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const FORMANT_SPECTRAL_TILT: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "formant_spectral_tilt",
    unit: ParameterUnit::DecibelsPerOctave,
    scale: ParameterScale::Linear,
    min: -24.0,
    max: 12.0,
    smoothing_seconds: 0.010,
};

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

pub(crate) const SPECTRAL_POSITION: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "spectral_position",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const SPECTRAL_FREEZE: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "spectral_freeze",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const SPECTRAL_BLUR: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "spectral_blur",
    unit: ParameterUnit::Seconds,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.020,
};

pub(crate) const SPECTRAL_SHIFT: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "spectral_shift",
    unit: ParameterUnit::Hertz,
    scale: ParameterScale::Linear,
    min: -12_000.0,
    max: 12_000.0,
    smoothing_seconds: 0.010,
};

pub(crate) const SPECTRAL_MORPH: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "spectral_morph",
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

pub(crate) const PHYSICAL_STRING_DECAY_SECONDS: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "physical_string_decay_seconds",
    unit: ParameterUnit::Seconds,
    scale: ParameterScale::Log2,
    min: 0.05,
    max: 20.0,
    smoothing_seconds: 0.010,
};

pub(crate) const PHYSICAL_STRING_BRIGHTNESS: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "physical_string_brightness",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const PHYSICAL_STRING_STIFFNESS: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "physical_string_stiffness",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const MODAL_STRUCTURE: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "modal_structure",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const MODAL_BRIGHTNESS: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "modal_brightness",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const MODAL_DECAY: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "modal_decay",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const GRANULAR_POSITION: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "granular_position",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.005,
};

pub(crate) const GRAIN_SIZE: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "grain_size",
    unit: ParameterUnit::Seconds,
    scale: ParameterScale::Log2,
    min: 0.005,
    max: 0.5,
    smoothing_seconds: 0.010,
};

pub(crate) const GRAIN_DENSITY: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "grain_density",
    unit: ParameterUnit::PerSecond,
    scale: ParameterScale::Log2,
    min: 1.0,
    max: 100.0,
    smoothing_seconds: 0.010,
};

pub(crate) const GRAIN_PITCH: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "grain_pitch",
    unit: ParameterUnit::Cents,
    scale: ParameterScale::Linear,
    min: -2400.0,
    max: 2400.0,
    smoothing_seconds: 0.005,
};

pub(crate) const GRAIN_RANDOMNESS: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "grain_randomness",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const GRAIN_PAN_SPREAD: GeneratorParameterSpec = GeneratorParameterSpec {
    suffix: "grain_pan_spread",
    unit: ParameterUnit::Normalized,
    scale: ParameterScale::Linear,
    min: 0.0,
    max: 1.0,
    smoothing_seconds: 0.010,
};

pub(crate) const OPERATOR_PARAMETER_SMOOTHING_SECONDS: f32 = 0.005;
pub(crate) const OPERATOR_PARAMETER_SUFFIXES: [&str; 5] =
    ["ratio", "detune", "level", "modulation_amount", "feedback"];

pub(crate) fn is_suffix(value: &str) -> bool {
    [
        PULSE_WIDTH,
        SYNC_RATIO,
        WAVESHAPE,
        PHASE_DISTORTION,
        WAVEFOLD,
        OSCILLATOR_FEEDBACK,
        GRANULAR_POSITION,
        GRAIN_SIZE,
        GRAIN_DENSITY,
        GRAIN_PITCH,
        GRAIN_RANDOMNESS,
        GRAIN_PAN_SPREAD,
        WAVETABLE_POSITION,
        UNISON_DETUNE,
        UNISON_SPREAD,
        NOISE_CORRELATION,
        PHYSICAL_STRING_DECAY_SECONDS,
        PHYSICAL_STRING_BRIGHTNESS,
        PHYSICAL_STRING_STIFFNESS,
        MODAL_STRUCTURE,
        MODAL_BRIGHTNESS,
        MODAL_DECAY,
        ADDITIVE_MORPH,
        ADDITIVE_SPECTRUM_TILT,
        ADDITIVE_INHARMONICITY,
        FORMANT_VOWEL_POSITION,
        FORMANT_SHIFT,
        FORMANT_THROAT,
        FORMANT_SPECTRAL_TILT,
        SPECTRAL_POSITION,
        SPECTRAL_FREEZE,
        SPECTRAL_BLUR,
        SPECTRAL_SHIFT,
        SPECTRAL_MORPH,
    ]
    .into_iter()
    .any(|spec| spec.suffix == value)
}
