use std::sync::Arc;

use crate::parameter::generator::MAX_PARTIALS;
use crate::process::{ProcessError, ProcessorFailureKind};

pub(crate) const SINE_TABLE_LENGTH: usize = 4096;
const ALIAS_FADE_START_RATIO: f64 = 0.40;
const ALIAS_FADE_END_RATIO: f64 = 0.45;

pub(crate) fn build_sine_table() -> Arc<[f32]> {
    let mut table = Vec::with_capacity(SINE_TABLE_LENGTH + 1);
    #[allow(clippy::cast_precision_loss)]
    for index in 0..=SINE_TABLE_LENGTH {
        let phase = index as f32 / SINE_TABLE_LENGTH as f32;
        table.push((std::f32::consts::TAU * phase).sin());
    }
    Arc::from(table.into_boxed_slice())
}

pub(super) fn alias_fade(frequency: f64, sample_rate: f64) -> f32 {
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

pub(super) struct PartialBankRuntime {
    phases: [f32; MAX_PARTIALS],
    initial_phases: [f32; MAX_PARTIALS],
    gains: [f32; MAX_PARTIALS],
    gain_steps: [f32; MAX_PARTIALS],
    ratio_factors: [f32; MAX_PARTIALS],
    ratio_steps: [f32; MAX_PARTIALS],
    active_count: usize,
    control_interval: usize,
    control_frames_remaining: usize,
    initialized: bool,
    sample_rate: f64,
    sine_table: Arc<[f32]>,
}

impl PartialBankRuntime {
    pub(super) fn new(
        initial_phases: [f32; MAX_PARTIALS],
        active_count: usize,
        sample_rate: f64,
        sine_table: Arc<[f32]>,
    ) -> Result<Self, ProcessError> {
        if !(1..=MAX_PARTIALS).contains(&active_count)
            || !sample_rate.is_finite()
            || sample_rate <= 0.0
            || sine_table.len() != SINE_TABLE_LENGTH + 1
            || initial_phases[..active_count]
                .iter()
                .any(|phase| !phase.is_finite() || !(0.0..=1.0).contains(phase))
        {
            return Err(invalid_state());
        }
        Ok(Self {
            phases: initial_phases,
            initial_phases,
            gains: [0.0; MAX_PARTIALS],
            gain_steps: [0.0; MAX_PARTIALS],
            ratio_factors: [1.0; MAX_PARTIALS],
            ratio_steps: [0.0; MAX_PARTIALS],
            active_count,
            control_interval: spectral_control_interval(sample_rate),
            control_frames_remaining: 0,
            initialized: false,
            sample_rate,
            sine_table,
        })
    }

    pub(super) fn start(&mut self, phase_reset: bool) {
        if phase_reset {
            self.reset_phases();
        }
        self.reset_controls();
    }

    pub(super) fn reset(&mut self) {
        self.reset_phases();
        self.reset_controls();
    }

    pub(super) fn controls_due(&self) -> bool {
        self.control_frames_remaining == 0
    }

    pub(super) fn update_targets(
        &mut self,
        target_gains: &[f32; MAX_PARTIALS],
        target_ratios: &[f32; MAX_PARTIALS],
    ) -> Result<(), ProcessError> {
        for index in 0..self.active_count {
            let gain = target_gains[index];
            let ratio = target_ratios[index];
            if !gain.is_finite() || !ratio.is_finite() || ratio <= 0.0 {
                return Err(invalid_input());
            }
            if self.initialized {
                #[allow(clippy::cast_precision_loss)]
                let interval = self.control_interval as f32;
                self.gain_steps[index] = (gain - self.gains[index]) / interval;
                self.ratio_steps[index] = (ratio - self.ratio_factors[index]) / interval;
            } else {
                self.gains[index] = gain;
                self.ratio_factors[index] = ratio;
                self.gain_steps[index] = 0.0;
                self.ratio_steps[index] = 0.0;
            }
        }
        self.initialized = true;
        self.control_frames_remaining = self.control_interval;
        Ok(())
    }

    pub(super) fn render_sample<F>(
        &mut self,
        base_frequency: f32,
        mut envelope_gain: F,
    ) -> Result<f32, ProcessError>
    where
        F: FnMut(usize) -> f32,
    {
        if !base_frequency.is_finite() || base_frequency <= 0.0 {
            return Err(ProcessError::InvalidFrequency);
        }
        let mut output = 0.0_f32;
        for index in 0..self.active_count {
            let envelope = envelope_gain(index);
            if !envelope.is_finite() {
                return Err(non_finite());
            }
            let value =
                lookup_sine(&self.sine_table, self.phases[index]) * self.gains[index] * envelope;
            if !value.is_finite() {
                return Err(non_finite());
            }
            output += value;
            let frequency = f64::from(base_frequency) * f64::from(self.ratio_factors[index]);
            if !frequency.is_finite() || frequency <= 0.0 {
                return Err(ProcessError::InvalidFrequency);
            }
            let increment = frequency / self.sample_rate;
            if !increment.is_finite() {
                return Err(ProcessError::InvalidFrequency);
            }
            #[allow(clippy::cast_possible_truncation)]
            {
                self.phases[index] =
                    (f64::from(self.phases[index]) + increment).rem_euclid(1.0) as f32;
            }
        }
        if !output.is_finite() {
            return Err(non_finite());
        }
        Ok(output)
    }

    pub(super) fn advance_control_frame(&mut self) {
        for index in 0..self.active_count {
            self.gains[index] += self.gain_steps[index];
            self.ratio_factors[index] += self.ratio_steps[index];
        }
        self.control_frames_remaining = self.control_frames_remaining.saturating_sub(1);
    }

    fn reset_phases(&mut self) {
        self.phases = self.initial_phases;
    }

    fn reset_controls(&mut self) {
        self.gains = [0.0; MAX_PARTIALS];
        self.gain_steps = [0.0; MAX_PARTIALS];
        self.ratio_factors = [1.0; MAX_PARTIALS];
        self.ratio_steps = [0.0; MAX_PARTIALS];
        self.control_frames_remaining = 0;
        self.initialized = false;
    }
}

fn lookup_sine(table: &[f32], phase: f32) -> f32 {
    let phase = phase.rem_euclid(1.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    let position = phase * SINE_TABLE_LENGTH as f32;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    let index = position.floor() as usize;
    #[allow(clippy::cast_precision_loss)]
    let fraction = position - index as f32;
    table[index] + (table[index + 1] - table[index]) * fraction
}

fn spectral_control_interval(sample_rate: f64) -> usize {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let frames = (sample_rate * 0.001).round() as usize;
    frames.max(1)
}

fn invalid_state() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::InvalidState,
    }
}

fn invalid_input() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::InvalidInput,
    }
}

fn non_finite() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::NonFinite,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_table_stays_within_the_requested_error() {
        let table = build_sine_table();
        let mut maximum_error = 0.0_f32;
        for index in 0..=(SINE_TABLE_LENGTH * 16) {
            #[allow(clippy::cast_precision_loss)]
            let phase = index as f32 / (SINE_TABLE_LENGTH * 16) as f32;
            maximum_error = maximum_error
                .max((lookup_sine(&table, phase) - (std::f32::consts::TAU * phase).sin()).abs());
        }
        assert!(maximum_error <= 1.0e-5, "maximum error was {maximum_error}");
    }

    #[test]
    fn phase_lookup_wraps_at_the_table_end() {
        let table = build_sine_table();
        assert!((lookup_sine(&table, 1.0) - lookup_sine(&table, 0.0)).abs() < 1.0e-7);
        assert!((lookup_sine(&table, -0.25) + 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn control_interval_uses_sample_rate_not_host_block_size() {
        assert_eq!(spectral_control_interval(44_100.0), 44);
        assert_eq!(spectral_control_interval(48_000.0), 48);
        assert_eq!(spectral_control_interval(96_000.0), 96);
    }

    #[test]
    fn phase_reset_and_control_ramps_use_fixed_storage() {
        let mut initial_phases = [0.0_f32; MAX_PARTIALS];
        initial_phases[0] = 0.25;
        let mut bank = PartialBankRuntime::new(initial_phases, 1, 48_000.0, build_sine_table())
            .expect("one partial bank is valid");
        let mut gains = [0.0_f32; MAX_PARTIALS];
        let ratios = [1.0_f32; MAX_PARTIALS];
        gains[0] = 1.0;
        bank.update_targets(&gains, &ratios)
            .expect("initial targets are valid");
        let initial_sample = bank
            .render_sample(440.0, |_| 1.0)
            .expect("initial sample is finite");
        let continued_phase = bank.phases[0];
        assert!(initial_sample.abs() > 0.9);
        assert!(continued_phase > initial_phases[0]);

        bank.start(false);
        assert!((bank.phases[0] - continued_phase).abs() < f32::EPSILON);
        bank.start(true);
        assert!((bank.phases[0] - initial_phases[0]).abs() < f32::EPSILON);

        gains[0] = 1.0;
        bank.update_targets(&gains, &ratios)
            .expect("ramp source targets are valid");
        gains[0] = 0.0;
        bank.update_targets(&gains, &ratios)
            .expect("updated targets are valid");
        assert!(bank.gain_steps[0] < 0.0);
        bank.advance_control_frame();
        assert!(bank.gains[0] < 1.0);
    }

    #[test]
    fn partial_bank_accepts_the_capacity_limit_and_rejects_invalid_counts() {
        let phases = [0.0_f32; MAX_PARTIALS];
        let table = build_sine_table();
        assert!(
            PartialBankRuntime::new(phases, MAX_PARTIALS, 48_000.0, Arc::clone(&table)).is_ok()
        );
        assert!(PartialBankRuntime::new(phases, 0, 48_000.0, Arc::clone(&table)).is_err());
        assert!(PartialBankRuntime::new(phases, MAX_PARTIALS + 1, 48_000.0, table).is_err());
    }
}
