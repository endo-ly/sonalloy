use crate::compiler::{CompiledVocoderProcessor, VOCODER_BANDS};
use crate::process::{ProcessError, ProcessSpec, ProcessorFailureKind};
use crate::runtime::external_audio::{ExternalAudioBlock, ExternalInputDelay};

use super::ValueSpan;
use super::biquad::{BiquadCoefficients, BiquadState};

pub(crate) struct VocoderRuntime {
    coefficients: [BiquadCoefficients; VOCODER_BANDS],
    carrier_left: [BiquadState; VOCODER_BANDS],
    carrier_right: [BiquadState; VOCODER_BANDS],
    modulator_left: [BiquadState; VOCODER_BANDS],
    modulator_right: [BiquadState; VOCODER_BANDS],
    envelopes_left: [f32; VOCODER_BANDS],
    envelopes_right: [f32; VOCODER_BANDS],
    attack_coeff: f32,
    release_coeff: f32,
    external_input: ExternalInputDelay,
}

impl VocoderRuntime {
    pub(crate) fn new(
        compiled: &CompiledVocoderProcessor,
        spec: ProcessSpec,
    ) -> Result<Self, ProcessError> {
        #[allow(clippy::cast_possible_truncation)]
        let sample_rate = spec.sample_rate as f32;
        let maximum = 12_000.0_f32.min(sample_rate * 0.45).max(80.0);
        let minimum = 80.0_f32.min(maximum / 2.0_f32.powf(23.0 / 6.0));
        let ratio = (maximum / minimum).powf(1.0 / 23.0);
        let mut coefficients = [BiquadCoefficients {
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }; VOCODER_BANDS];
        for (index, coefficient) in coefficients.iter_mut().enumerate() {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let frequency = minimum * ratio.powi(index as i32);
            let edge_ratio = ratio.sqrt();
            let bandwidth = (frequency * edge_ratio - frequency / edge_ratio).max(10.0);
            *coefficient = BiquadCoefficients::band_pass(sample_rate, frequency, bandwidth)?;
        }
        Ok(Self {
            coefficients,
            carrier_left: [BiquadState::default(); VOCODER_BANDS],
            carrier_right: [BiquadState::default(); VOCODER_BANDS],
            modulator_left: [BiquadState::default(); VOCODER_BANDS],
            modulator_right: [BiquadState::default(); VOCODER_BANDS],
            envelopes_left: [0.0; VOCODER_BANDS],
            envelopes_right: [0.0; VOCODER_BANDS],
            attack_coeff: compiled.attack_coeff,
            release_coeff: compiled.release_coeff,
            external_input: ExternalInputDelay::new(compiled.external_input_alignment_frames),
        })
    }

    pub(crate) fn process(
        &mut self,
        modulator_gain_db: ValueSpan,
        output_gain_db: ValueSpan,
        mix: ValueSpan,
        external: ExternalAudioBlock<'_>,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(invalid_state());
        }
        for index in 0..left.len() {
            let modulator_gain = db_to_linear(modulator_gain_db.value_at(index, left.len()));
            let output_gain = db_to_linear(output_gain_db.value_at(index, left.len()));
            let mix_value = mix.value_at(index, left.len()).clamp(0.0, 1.0);
            let (external_left, external_right) = self.external_input.next(external, index);
            let modulator_left = external_left * modulator_gain;
            let modulator_right = external_right * modulator_gain;
            let mut wet_left = 0.0;
            let mut wet_right = 0.0;
            for band in 0..VOCODER_BANDS {
                let modulator_left_band =
                    self.modulator_left[band].process(self.coefficients[band], modulator_left)?;
                let modulator_right_band =
                    self.modulator_right[band].process(self.coefficients[band], modulator_right)?;
                let target_left = modulator_left_band.abs();
                let target_right = modulator_right_band.abs();
                let coefficient_left = if target_left > self.envelopes_left[band] {
                    self.attack_coeff
                } else {
                    self.release_coeff
                };
                let coefficient_right = if target_right > self.envelopes_right[band] {
                    self.attack_coeff
                } else {
                    self.release_coeff
                };
                self.envelopes_left[band] =
                    target_left + coefficient_left * (self.envelopes_left[band] - target_left);
                self.envelopes_right[band] =
                    target_right + coefficient_right * (self.envelopes_right[band] - target_right);
                wet_left += self.carrier_left[band]
                    .process(self.coefficients[band], left[index])?
                    * self.envelopes_left[band];
                wet_right += self.carrier_right[band]
                    .process(self.coefficients[band], right[index])?
                    * self.envelopes_right[band];
            }
            wet_left *= output_gain;
            wet_right *= output_gain;
            left[index] = left[index] * (1.0 - mix_value) + wet_left * mix_value;
            right[index] = right[index] * (1.0 - mix_value) + wet_right * mix_value;
            if !modulator_gain.is_finite()
                || !output_gain.is_finite()
                || !mix_value.is_finite()
                || !left[index].is_finite()
                || !right[index].is_finite()
            {
                return Err(non_finite());
            }
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        for state in &mut self.carrier_left {
            state.reset();
        }
        for state in &mut self.carrier_right {
            state.reset();
        }
        for state in &mut self.modulator_left {
            state.reset();
        }
        for state in &mut self.modulator_right {
            state.reset();
        }
        self.envelopes_left.fill(0.0);
        self.envelopes_right.fill(0.0);
        self.external_input.reset();
    }
}

fn db_to_linear(value: f32) -> f32 {
    10.0_f32.powf(value / 20.0)
}

fn invalid_state() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::InvalidState,
    }
}

fn non_finite() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::NonFinite,
    }
}
