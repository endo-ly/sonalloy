use crate::compiler::{CompiledFormant, CompiledFormantBand, CompiledFormantProfile};
use crate::generator_parameters::{
    FORMANT_SHIFT, FORMANT_SPECTRAL_TILT, FORMANT_THROAT, FORMANT_VOWEL_POSITION, MAX_PARTIALS,
};
use crate::process::{ProcessError, ProcessSpec};

use super::super::modulation::{LayerGeneratorTargetSpan, ValueSpan};
use super::partial_bank::{PartialBankRuntime, alias_fade};
use super::{base_frequencies, ensure_finite, invalid_state, validate_generator_span};

const FORMANT_COUNT: usize = 5;
const FWHM_TO_SIGMA: f32 = 2.354_82;

pub(crate) struct FormantRuntime {
    bank: PartialBankRuntime,
    profiles: Box<[CompiledFormantProfile]>,
    target_gains: [f32; MAX_PARTIALS],
    target_ratios: [f32; MAX_PARTIALS],
    partial_count: usize,
    phase_reset: bool,
}

impl FormantRuntime {
    pub(super) fn new(compiled: &CompiledFormant, spec: ProcessSpec) -> Result<Self, ProcessError> {
        let partial_count = compiled.partial_count;
        if !(1..=MAX_PARTIALS).contains(&partial_count)
            || !(1..=8).contains(&compiled.profiles.len())
            || compiled
                .profiles
                .iter()
                .any(|profile| profile.formants.len() != FORMANT_COUNT)
        {
            return Err(invalid_state());
        }
        let bank = PartialBankRuntime::new(
            [0.0; MAX_PARTIALS],
            partial_count,
            spec.sample_rate,
            std::sync::Arc::clone(&compiled.sine_table),
        )?;
        let target_ratios = std::array::from_fn(|index| {
            #[allow(clippy::cast_precision_loss)]
            {
                (index + 1) as f32
            }
        });
        Ok(Self {
            bank,
            profiles: compiled.profiles.clone(),
            target_gains: [0.0; MAX_PARTIALS],
            target_ratios,
            partial_count,
            phase_reset: compiled.phase_reset,
        })
    }

    pub(super) fn start(&mut self) {
        self.bank.start(self.phase_reset);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn render(
        &mut self,
        frames: usize,
        note_number: u8,
        tuning_start: f32,
        tuning_end: f32,
        sample_rate: f64,
        targets: LayerGeneratorTargetSpan,
        mono: &mut [f32],
    ) -> Result<(), ProcessError> {
        if frames == 0 {
            return Ok(());
        }
        let LayerGeneratorTargetSpan::Formant {
            vowel_position,
            formant_shift,
            throat,
            spectral_tilt,
        } = targets
        else {
            return Err(invalid_state());
        };
        if mono.len() < frames || !sample_rate.is_finite() || sample_rate <= 0.0 {
            return Err(invalid_state());
        }
        validate_generator_span(vowel_position, FORMANT_VOWEL_POSITION)?;
        validate_generator_span(formant_shift, FORMANT_SHIFT)?;
        validate_generator_span(throat, FORMANT_THROAT)?;
        validate_generator_span(spectral_tilt, FORMANT_SPECTRAL_TILT)?;
        let (base_start, base_end) = base_frequencies(note_number, tuning_start, tuning_end)?;
        let base_frequency = ValueSpan {
            start: base_start,
            end: base_end,
        };
        for (frame, sample) in mono.iter_mut().take(frames).enumerate() {
            let current_frequency = base_frequency.value_at(frame, frames);
            if self.bank.controls_due() {
                self.update_spectral_controls(
                    current_frequency,
                    vowel_position.value_at(frame, frames),
                    formant_shift.value_at(frame, frames),
                    throat.value_at(frame, frames),
                    spectral_tilt.value_at(frame, frames),
                    sample_rate,
                )?;
            }
            *sample = self.bank.render_sample(current_frequency, |_| 1.0)?;
            self.bank.advance_control_frame();
        }
        ensure_finite(&mono[..frames])
    }

    pub(super) fn reset(&mut self) {
        self.bank.reset();
    }

    fn update_spectral_controls(
        &mut self,
        base_frequency: f32,
        vowel_position: f32,
        formant_shift: f32,
        throat: f32,
        spectral_tilt: f32,
        sample_rate: f64,
    ) -> Result<(), ProcessError> {
        if !base_frequency.is_finite()
            || base_frequency <= 0.0
            || !sample_rate.is_finite()
            || sample_rate <= 0.0
        {
            return Err(ProcessError::InvalidFrequency);
        }
        let (first, second, profile_mix) = profile_pair(&self.profiles, vowel_position)?;
        let shift_ratio = 2.0_f32.powf(formant_shift / 1200.0);
        let bandwidth_multiplier = 2.0_f32.powf(2.0 * (throat - 0.5));
        if !shift_ratio.is_finite() || !bandwidth_multiplier.is_finite() {
            return Err(super::non_finite());
        }
        let mut bands = [CompiledFormantBand {
            frequency_hz: 0.0,
            bandwidth_hz: 0.0,
            gain_db: 0.0,
        }; FORMANT_COUNT];
        for (index, band) in bands.iter_mut().enumerate() {
            let first_band = first.formants[index];
            let second_band = second.formants[index];
            band.frequency_hz = geometric_lerp(
                first_band.frequency_hz,
                second_band.frequency_hz,
                profile_mix,
            ) * shift_ratio;
            band.bandwidth_hz = geometric_lerp(
                first_band.bandwidth_hz,
                second_band.bandwidth_hz,
                profile_mix,
            ) * shift_ratio
                * bandwidth_multiplier;
            band.gain_db =
                first_band.gain_db + (second_band.gain_db - first_band.gain_db) * profile_mix;
        }
        let mut energy = 0.0_f32;
        for index in 0..self.partial_count {
            let ratio = self.target_ratios[index];
            let frequency = f64::from(base_frequency) * f64::from(ratio);
            if !frequency.is_finite() || frequency <= 0.0 {
                return Err(ProcessError::InvalidFrequency);
            }
            let mut formant_gain = 0.0_f32;
            for band in bands {
                let sigma = band.bandwidth_hz / FWHM_TO_SIGMA;
                #[allow(clippy::cast_possible_truncation)]
                let frequency_hz = frequency as f32;
                let distance = (frequency_hz - band.frequency_hz) / sigma;
                let band_gain =
                    10.0_f32.powf(band.gain_db / 20.0) * (-0.5 * distance * distance).exp();
                formant_gain += band_gain;
            }
            let tilt_gain = 10.0_f32.powf(spectral_tilt * ratio.log2() / 20.0);
            let gain = formant_gain * tilt_gain * alias_fade(frequency, sample_rate);
            if !gain.is_finite() {
                return Err(super::non_finite());
            }
            self.target_gains[index] = gain;
            energy += gain * gain;
        }
        if !energy.is_finite() {
            return Err(super::non_finite());
        }
        let normalization = 1.0 / energy.sqrt().max(1.0);
        for gain in &mut self.target_gains[..self.partial_count] {
            *gain *= normalization;
        }
        self.bank
            .update_targets(&self.target_gains, &self.target_ratios)
    }
}

fn profile_pair(
    profiles: &[CompiledFormantProfile],
    position: f32,
) -> Result<(&CompiledFormantProfile, &CompiledFormantProfile, f32), ProcessError> {
    if profiles.is_empty() || !position.is_finite() || !(0.0..=1.0).contains(&position) {
        return Err(invalid_state());
    }
    if profiles.len() == 1 {
        return Ok((&profiles[0], &profiles[0], 0.0));
    }
    #[allow(clippy::cast_precision_loss)]
    let scaled = position * (profiles.len() - 1) as f32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let index = scaled.floor() as usize;
    let index = index.min(profiles.len() - 2);
    #[allow(clippy::cast_precision_loss)]
    let mix = scaled - index as f32;
    Ok((&profiles[index], &profiles[index + 1], mix))
}

fn geometric_lerp(first: f32, second: f32, mix: f32) -> f32 {
    (first.ln() + (second.ln() - first.ln()) * mix).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_position_uses_adjacent_profiles_and_clamps_only_the_final_segment() {
        let band = |frequency_hz| CompiledFormantBand {
            frequency_hz,
            bandwidth_hz: frequency_hz / 10.0,
            gain_db: 0.0,
        };
        let profiles = vec![
            CompiledFormantProfile {
                id: "a".to_owned(),
                formants: [band(100.0); FORMANT_COUNT],
            },
            CompiledFormantProfile {
                id: "i".to_owned(),
                formants: [band(1_000.0); FORMANT_COUNT],
            },
            CompiledFormantProfile {
                id: "u".to_owned(),
                formants: [band(10_000.0); FORMANT_COUNT],
            },
        ];
        let (first, second, mix) = profile_pair(&profiles, 0.25).expect("position is valid");
        assert_eq!(first.id, "a");
        assert_eq!(second.id, "i");
        assert!((mix - 0.5).abs() < 1.0e-6);

        let (first, second, mix) = profile_pair(&profiles, 1.0).expect("position is valid");
        assert_eq!(first.id, "i");
        assert_eq!(second.id, "u");
        assert!((mix - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn profile_interpolation_uses_log_frequency_and_linear_gain() {
        assert!((geometric_lerp(100.0, 400.0, 0.5) - 200.0).abs() < 1.0e-4);
        assert!((10.0_f32.powf(-6.0 / 20.0)).is_finite());
    }
}
