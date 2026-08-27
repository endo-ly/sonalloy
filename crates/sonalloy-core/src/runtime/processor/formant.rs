use crate::compiler::CompiledFormantProfile;
use crate::formant::{geometric_lerp, profile_pair};
use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;
use super::biquad::{BiquadCoefficients, BiquadState};

const FORMANT_BANDS: usize = 5;
const CONTROL_QUANTUM: usize = 32;

pub(crate) struct FormantProcessorRuntime {
    sample_rate: f32,
    profiles: Box<[CompiledFormantProfile]>,
    coefficients: [BiquadCoefficients; FORMANT_BANDS],
    gains: [f32; FORMANT_BANDS],
    left: [BiquadState; FORMANT_BANDS],
    right: [BiquadState; FORMANT_BANDS],
    frame_counter: usize,
}

impl FormantProcessorRuntime {
    pub(crate) fn new(
        profiles: &[CompiledFormantProfile],
        sample_rate: f32,
    ) -> Result<Self, ProcessError> {
        if profiles.is_empty() || profiles.len() > 8 || !sample_rate.is_finite() {
            return Err(invalid_state());
        }
        let coefficients = BiquadCoefficients::band_pass(sample_rate, 100.0, 100.0)?;
        Ok(Self {
            sample_rate,
            profiles: profiles.to_vec().into_boxed_slice(),
            coefficients: [coefficients; FORMANT_BANDS],
            gains: [0.0; FORMANT_BANDS],
            left: [BiquadState::default(); FORMANT_BANDS],
            right: [BiquadState::default(); FORMANT_BANDS],
            frame_counter: 0,
        })
    }

    pub(crate) fn process_mono(
        &mut self,
        vowel_position: ValueSpan,
        formant_shift: ValueSpan,
        throat: ValueSpan,
        mix: ValueSpan,
        buffer: &mut [f32],
    ) -> Result<(), ProcessError> {
        for index in 0..buffer.len() {
            let input = buffer[index];
            let values = (
                vowel_position.value_at(index, buffer.len()),
                formant_shift.value_at(index, buffer.len()),
                throat.value_at(index, buffer.len()),
                mix.value_at(index, buffer.len()),
            );
            self.update_if_due(values.0, values.1, values.2)?;
            let wet =
                Self::process_channel(&self.coefficients, &self.gains, input, &mut self.left)?;
            buffer[index] = mix_sample(input, wet, values.3)?;
            self.advance_frame();
        }
        Ok(())
    }

    pub(crate) fn process_stereo(
        &mut self,
        vowel_position: ValueSpan,
        formant_shift: ValueSpan,
        throat: ValueSpan,
        mix: ValueSpan,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(invalid_state());
        }
        for index in 0..left.len() {
            let input_left = left[index];
            let input_right = right[index];
            let values = (
                vowel_position.value_at(index, left.len()),
                formant_shift.value_at(index, left.len()),
                throat.value_at(index, left.len()),
                mix.value_at(index, left.len()),
            );
            self.update_if_due(values.0, values.1, values.2)?;
            let wet_left =
                Self::process_channel(&self.coefficients, &self.gains, input_left, &mut self.left)?;
            let wet_right = Self::process_channel(
                &self.coefficients,
                &self.gains,
                input_right,
                &mut self.right,
            )?;
            left[index] = mix_sample(input_left, wet_left, values.3)?;
            right[index] = mix_sample(input_right, wet_right, values.3)?;
            self.advance_frame();
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        for state in &mut self.left {
            state.reset();
        }
        for state in &mut self.right {
            state.reset();
        }
        self.frame_counter = 0;
    }

    fn update_if_due(
        &mut self,
        position: f32,
        shift_cents: f32,
        throat: f32,
    ) -> Result<(), ProcessError> {
        if self.frame_counter % CONTROL_QUANTUM != 0 {
            return Ok(());
        }
        if !position.is_finite() || !shift_cents.is_finite() || !throat.is_finite() {
            return Err(non_finite());
        }
        let (first, second, mix) = profile_pair(&self.profiles, position)?;
        let shift = 2.0_f32.powf(shift_cents / 1200.0);
        let bandwidth_scale = 2.0_f32.powf(2.0 * (throat - 0.5));
        let mut energy = 0.0;
        for index in 0..FORMANT_BANDS {
            let first_band = first.formants[index];
            let second_band = second.formants[index];
            let frequency =
                geometric_lerp(first_band.frequency_hz, second_band.frequency_hz, mix) * shift;
            let bandwidth = geometric_lerp(first_band.bandwidth_hz, second_band.bandwidth_hz, mix)
                * shift
                * bandwidth_scale;
            let frequency = frequency.clamp(20.0, self.sample_rate * 0.45);
            self.coefficients[index] =
                BiquadCoefficients::band_pass(self.sample_rate, frequency, bandwidth.max(1.0))?;
            let gain = 10.0_f32.powf(
                (first_band.gain_db + (second_band.gain_db - first_band.gain_db) * mix) / 20.0,
            );
            if !gain.is_finite() {
                return Err(non_finite());
            }
            self.gains[index] = gain;
            energy += gain * gain;
        }
        let normalization = 1.0 / energy.sqrt().max(1.0);
        for gain in &mut self.gains {
            *gain *= normalization;
        }
        Ok(())
    }

    fn process_channel(
        coefficients: &[BiquadCoefficients; FORMANT_BANDS],
        gains: &[f32; FORMANT_BANDS],
        input: f32,
        states: &mut [BiquadState; FORMANT_BANDS],
    ) -> Result<f32, ProcessError> {
        if !input.is_finite() {
            return Err(non_finite());
        }
        let mut output = 0.0_f32;
        for index in 0..FORMANT_BANDS {
            output += states[index].process(coefficients[index], input)? * gains[index];
        }
        if output.is_finite() {
            Ok(output)
        } else {
            Err(non_finite())
        }
    }

    fn advance_frame(&mut self) {
        self.frame_counter = self.frame_counter.wrapping_add(1);
    }
}

fn mix_sample(dry: f32, wet: f32, mix: f32) -> Result<f32, ProcessError> {
    let output = dry * (1.0 - mix) + wet * mix;
    if output.is_finite() {
        Ok(output)
    } else {
        Err(non_finite())
    }
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

#[cfg(test)]
mod tests {
    use crate::compiler::CompiledFormantBand;

    use super::*;

    fn span(value: f32) -> ValueSpan {
        ValueSpan {
            start: value,
            end: value,
        }
    }

    fn profile() -> CompiledFormantProfile {
        CompiledFormantProfile {
            id: "test".to_owned(),
            formants: [
                (400.0, 60.0, 0.0),
                (1_000.0, 80.0, -3.0),
                (2_000.0, 100.0, -6.0),
                (3_000.0, 120.0, -9.0),
                (4_000.0, 140.0, -12.0),
            ]
            .map(
                |(frequency_hz, bandwidth_hz, gain_db)| CompiledFormantBand {
                    frequency_hz,
                    bandwidth_hz,
                    gain_db,
                },
            ),
        }
    }

    #[test]
    fn profile_filter_ramps_and_reset_remain_finite() {
        let profiles = [profile()];
        let mut runtime =
            FormantProcessorRuntime::new(&profiles, 48_000.0).expect("formant processor prepares");
        let mut first = [0.0; 128];
        first[0] = 1.0;
        runtime
            .process_mono(
                ValueSpan {
                    start: 0.0,
                    end: 1.0,
                },
                span(2400.0),
                span(1.0),
                span(1.0),
                &mut first,
            )
            .expect("formant processor processes");
        assert!(first.iter().all(|sample| sample.is_finite()));

        runtime.reset();
        let mut second = [0.0; 128];
        second[0] = 1.0;
        runtime
            .process_mono(span(0.0), span(2400.0), span(1.0), span(1.0), &mut second)
            .expect("reset formant processor processes");
        assert!(second.iter().all(|sample| sample.is_finite()));
        assert!(first.iter().any(|sample| sample.abs() > 1.0e-6));
    }
}
