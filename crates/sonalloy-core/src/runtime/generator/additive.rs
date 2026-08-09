use crate::compiler::CompiledAdditive;
use crate::generator_parameters::{
    ADDITIVE_INHARMONICITY, ADDITIVE_MORPH, ADDITIVE_SPECTRUM_TILT, MAX_PARTIALS,
};
use crate::process::{ProcessError, ProcessSpec};

use super::super::adsr::AdsrRuntime;
use super::super::modulation::{LayerGeneratorTargetSpan, ValueSpan};
use super::partial_bank::PartialBankRuntime;
use super::{base_frequencies, ensure_finite, invalid_state, validate_generator_span};

const INHARMONICITY_MAX: f32 = 0.0005;
const ALIAS_FADE_START_RATIO: f64 = 0.40;
const ALIAS_FADE_END_RATIO: f64 = 0.45;

pub(crate) struct AdditiveRuntime {
    bank: PartialBankRuntime,
    ratios: [f32; MAX_PARTIALS],
    amplitudes_a: [f32; MAX_PARTIALS],
    amplitudes_b: [f32; MAX_PARTIALS],
    envelopes: [Option<AdsrRuntime>; MAX_PARTIALS],
    target_gains: [f32; MAX_PARTIALS],
    target_ratios: [f32; MAX_PARTIALS],
    partial_count: usize,
    phase_reset: bool,
}

impl AdditiveRuntime {
    pub(super) fn new(
        compiled: &CompiledAdditive,
        spec: ProcessSpec,
    ) -> Result<Self, ProcessError> {
        let partial_count = compiled.partials.len();
        if !(1..=MAX_PARTIALS).contains(&partial_count) {
            return Err(invalid_state());
        }
        let mut initial_phases = [0.0_f32; MAX_PARTIALS];
        let mut ratios = [1.0_f32; MAX_PARTIALS];
        let mut amplitudes_a = [0.0_f32; MAX_PARTIALS];
        let mut amplitudes_b = [0.0_f32; MAX_PARTIALS];
        let envelopes = std::array::from_fn(|index| {
            compiled.partials.get(index).and_then(|partial| {
                initial_phases[index] = partial.phase;
                ratios[index] = partial.ratio;
                amplitudes_a[index] = partial.amplitude_a;
                amplitudes_b[index] = partial.amplitude_b;
                partial.envelope.map(AdsrRuntime::new)
            })
        });
        let bank = PartialBankRuntime::new(
            initial_phases,
            partial_count,
            spec.sample_rate,
            std::sync::Arc::clone(&compiled.sine_table),
        )?;
        Ok(Self {
            bank,
            ratios,
            amplitudes_a,
            amplitudes_b,
            envelopes,
            target_gains: [0.0; MAX_PARTIALS],
            target_ratios: [1.0; MAX_PARTIALS],
            partial_count,
            phase_reset: compiled.phase_reset,
        })
    }

    pub(super) fn start(&mut self) {
        self.bank.start(self.phase_reset);
        for envelope in self.envelopes[..self.partial_count].iter_mut().flatten() {
            envelope.note_on();
        }
    }

    pub(super) fn note_off(&mut self) {
        for envelope in self.envelopes[..self.partial_count].iter_mut().flatten() {
            envelope.note_off();
        }
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
        let LayerGeneratorTargetSpan::Additive {
            morph,
            spectrum_tilt,
            inharmonicity,
        } = targets
        else {
            return Err(invalid_state());
        };
        if mono.len() < frames || !sample_rate.is_finite() || sample_rate <= 0.0 {
            return Err(invalid_state());
        }
        validate_generator_span(morph, ADDITIVE_MORPH)?;
        validate_generator_span(spectrum_tilt, ADDITIVE_SPECTRUM_TILT)?;
        validate_generator_span(inharmonicity, ADDITIVE_INHARMONICITY)?;
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
                    morph.value_at(frame, frames),
                    spectrum_tilt.value_at(frame, frames),
                    inharmonicity.value_at(frame, frames),
                    sample_rate,
                )?;
            }
            let envelopes = &mut self.envelopes;
            *sample = self.bank.render_sample(current_frequency, |index| {
                envelopes[index]
                    .as_mut()
                    .map_or(1.0, AdsrRuntime::next_sample)
            })?;
            self.bank.advance_control_frame();
        }
        ensure_finite(&mono[..frames])
    }

    pub(super) fn reset(&mut self) {
        self.bank.reset();
        for envelope in self.envelopes.iter_mut().flatten() {
            envelope.reset();
        }
    }

    fn update_spectral_controls(
        &mut self,
        base_frequency: f32,
        morph: f32,
        spectrum_tilt: f32,
        inharmonicity: f32,
        sample_rate: f64,
    ) -> Result<(), ProcessError> {
        if !base_frequency.is_finite()
            || base_frequency <= 0.0
            || !sample_rate.is_finite()
            || sample_rate <= 0.0
        {
            return Err(ProcessError::InvalidFrequency);
        }
        let b = inharmonicity * INHARMONICITY_MAX;
        let mut energy = 0.0_f32;
        for index in 0..self.partial_count {
            let ratio = self.ratios[index];
            let amplitude = self.amplitudes_a[index]
                + (self.amplitudes_b[index] - self.amplitudes_a[index]) * morph;
            let tilt_gain = 10.0_f32.powf(spectrum_tilt * ratio.log2() / 20.0);
            let effective_ratio = effective_ratio(ratio, b)?;
            let frequency = f64::from(base_frequency) * f64::from(effective_ratio);
            let alias_gain = alias_fade(frequency, sample_rate);
            let gain = amplitude * tilt_gain * alias_gain;
            if !gain.is_finite() {
                return Err(super::non_finite());
            }
            self.target_gains[index] = gain;
            self.target_ratios[index] = effective_ratio;
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

fn effective_ratio(ratio: f32, b: f32) -> Result<f32, ProcessError> {
    if !ratio.is_finite() || ratio <= 0.0 || !b.is_finite() || b < 0.0 {
        return Err(ProcessError::InvalidFrequency);
    }
    #[allow(clippy::float_cmp)]
    if ratio == 1.0 {
        return Ok(1.0);
    }
    let numerator = 1.0 + b * ratio * ratio;
    let denominator = 1.0 + b;
    let ratio = ratio * (numerator / denominator).sqrt();
    if ratio.is_finite() && ratio > 0.0 {
        Ok(ratio)
    } else {
        Err(ProcessError::InvalidFrequency)
    }
}

fn alias_fade(frequency: f64, sample_rate: f64) -> f32 {
    let normalized = frequency / sample_rate;
    if normalized <= ALIAS_FADE_START_RATIO {
        1.0
    } else if normalized >= ALIAS_FADE_END_RATIO {
        0.0
    } else {
        #[allow(clippy::cast_possible_truncation)]
        {
            ((ALIAS_FADE_END_RATIO - normalized) / (ALIAS_FADE_END_RATIO - ALIAS_FADE_START_RATIO))
                as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inharmonicity_keeps_the_fundamental_at_one() {
        assert_eq!(effective_ratio(1.0, INHARMONICITY_MAX), Ok(1.0));
        assert!(effective_ratio(2.0, INHARMONICITY_MAX).expect("ratio is finite") > 2.0);
    }

    #[test]
    fn alias_fade_has_the_declared_boundaries() {
        assert!((alias_fade(0.39, 1.0) - 1.0).abs() < f32::EPSILON);
        assert!((alias_fade(0.40, 1.0) - 1.0).abs() < f32::EPSILON);
        assert!(alias_fade(0.45, 1.0).abs() < f32::EPSILON);
        assert!(alias_fade(0.46, 1.0).abs() < f32::EPSILON);
        assert!((alias_fade(0.425, 1.0) - 0.5).abs() < 1.0e-6);
    }
}
