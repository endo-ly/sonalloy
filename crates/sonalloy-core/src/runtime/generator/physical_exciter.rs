use crate::compiler::{CompiledPhysicalExciter, PHYSICAL_FREQUENCY_LIMIT_RATIO};
use crate::process::{ProcessError, ProcessorFailureKind};

use super::super::random::{bipolar_f32, splitmix64_finalizer};

const STREAM_PHYSICAL_EXCITER: u64 = 0x7068_7973_6963_0001;
pub(super) const PHYSICAL_EXCITER_GAIN: f32 = 0.25;
pub(super) const PHYSICAL_MIN_CUTOFF_HZ: f32 = 200.0;
pub(super) const PHYSICAL_MAX_CUTOFF_HZ: f32 = 18_000.0;
pub(super) const MIN_PHYSICAL_FREQUENCY_HZ: f32 = 4.0;

#[allow(clippy::cast_possible_truncation)]
pub(super) fn physical_max_cutoff(sample_rate: f32) -> f32 {
    PHYSICAL_MAX_CUTOFF_HZ.min(sample_rate * PHYSICAL_FREQUENCY_LIMIT_RATIO as f32)
}

pub(super) fn valid_physical_frequency(frequency: f32, maximum: f32) -> bool {
    frequency.is_finite() && (MIN_PHYSICAL_FREQUENCY_HZ..=maximum).contains(&frequency)
}

pub(crate) struct PhysicalExciterRuntime {
    definition: CompiledPhysicalExciter,
    layer_hash: u64,
    sample_rate: f32,
    duration_frames: usize,
    envelope_coefficient: f32,
    lowpass_coefficient: f32,
    frame: usize,
    envelope: f32,
    random_state: u64,
    lowpass_state: f32,
}

impl PhysicalExciterRuntime {
    pub(super) fn new(
        definition: CompiledPhysicalExciter,
        layer_hash: u64,
        sample_rate: f64,
    ) -> Result<Self, ProcessError> {
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return Err(ProcessError::InvalidSampleRate);
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let sample_rate_f32 = sample_rate as f32;
        let (duration_frames, envelope_coefficient, lowpass_coefficient) = match definition {
            CompiledPhysicalExciter::Impulse => (1, 1.0, 0.0),
            CompiledPhysicalExciter::NoiseBurst {
                duration_seconds,
                brightness,
                ..
            } => {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let duration_frames =
                    (f64::from(duration_seconds) * sample_rate).round().max(1.0) as usize;
                #[allow(clippy::cast_precision_loss)]
                let envelope_coefficient = 10.0_f32.powf(-3.0 / duration_frames as f32);
                let max_cutoff = physical_max_cutoff(sample_rate_f32);
                let cutoff = PHYSICAL_MIN_CUTOFF_HZ
                    * (max_cutoff / PHYSICAL_MIN_CUTOFF_HZ)
                        .max(1.0)
                        .powf(brightness);
                let lowpass_coefficient = (-std::f32::consts::TAU * cutoff / sample_rate_f32).exp();
                (duration_frames, envelope_coefficient, lowpass_coefficient)
            }
        };
        if !sample_rate_f32.is_finite()
            || !envelope_coefficient.is_finite()
            || !lowpass_coefficient.is_finite()
        {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            });
        }
        Ok(Self {
            definition,
            layer_hash,
            sample_rate: sample_rate_f32,
            duration_frames,
            envelope_coefficient,
            lowpass_coefficient,
            frame: 0,
            envelope: 1.0,
            random_state: 0,
            lowpass_state: 0.0,
        })
    }

    pub(super) fn start(&mut self, note_id: u64) {
        self.frame = 0;
        self.envelope = 1.0;
        self.lowpass_state = 0.0;
        self.random_state = match self.definition {
            CompiledPhysicalExciter::Impulse => 0,
            CompiledPhysicalExciter::NoiseBurst { seed, .. } => {
                splitmix64_finalizer(seed ^ self.layer_hash ^ note_id ^ STREAM_PHYSICAL_EXCITER)
            }
        };
    }

    pub(super) fn render(&mut self, frames: usize, output: &mut [f32]) -> Result<(), ProcessError> {
        if output.len() < frames || !self.sample_rate.is_finite() || self.sample_rate <= 0.0 {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            });
        }
        for sample in &mut output[..frames] {
            *sample = match self.definition {
                CompiledPhysicalExciter::Impulse => {
                    if self.frame == 0 {
                        PHYSICAL_EXCITER_GAIN
                    } else {
                        0.0
                    }
                }
                CompiledPhysicalExciter::NoiseBurst { .. } => {
                    if self.frame >= self.duration_frames {
                        0.0
                    } else {
                        self.random_state = self.random_state.wrapping_add(0x9e37_79b9_7f4a_7c15);
                        let white = bipolar_f32(splitmix64_finalizer(self.random_state));
                        self.lowpass_state = self
                            .lowpass_coefficient
                            .mul_add(self.lowpass_state, (1.0 - self.lowpass_coefficient) * white);
                        self.lowpass_state * self.envelope * PHYSICAL_EXCITER_GAIN
                    }
                }
            };
            if !sample.is_finite() {
                return Err(ProcessError::ProcessorFailure {
                    kind: ProcessorFailureKind::NonFinite,
                });
            }
            self.frame = self.frame.saturating_add(1);
            if matches!(self.definition, CompiledPhysicalExciter::NoiseBurst { .. }) {
                self.envelope *= self.envelope_coefficient;
            }
        }
        Ok(())
    }

    pub(super) fn reset(&mut self) {
        self.frame = 0;
        self.envelope = 1.0;
        self.random_state = 0;
        self.lowpass_state = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::PHYSICAL_EXCITER_GAIN;
    use super::PhysicalExciterRuntime;
    use crate::compiler::CompiledPhysicalExciter;

    #[test]
    fn impulse_is_note_deterministic_and_one_sample_long() {
        let mut exciter =
            PhysicalExciterRuntime::new(CompiledPhysicalExciter::Impulse, 7, 48_000.0)
                .expect("exciter");
        exciter.start(3);
        let mut output = [0.0; 4];
        exciter.render(4, &mut output).expect("render");
        assert_eq!(output[0].to_bits(), PHYSICAL_EXCITER_GAIN.to_bits());
        assert!(
            output[1..]
                .iter()
                .all(|sample| sample.to_bits() == 0.0_f32.to_bits())
        );
    }

    #[test]
    fn noise_burst_is_seeded_by_note_and_finite() {
        let definition = CompiledPhysicalExciter::NoiseBurst {
            duration_seconds: 0.01,
            brightness: 0.5,
            seed: 11,
        };
        let mut first = PhysicalExciterRuntime::new(definition, 7, 48_000.0).expect("exciter");
        first.start(3);
        let mut first_output = [0.0; 512];
        first.render(512, &mut first_output).expect("render");
        let mut second = PhysicalExciterRuntime::new(definition, 7, 48_000.0).expect("exciter");
        second.start(3);
        let mut second_output = [0.0; 512];
        second.render(512, &mut second_output).expect("render");
        assert_eq!(
            first_output
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>(),
            second_output
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>(),
        );
        assert!(first_output.iter().all(|sample| sample.is_finite()));
        assert!(
            first_output[480..]
                .iter()
                .all(|sample| sample.abs() < 1.0e-6)
        );
    }
}
