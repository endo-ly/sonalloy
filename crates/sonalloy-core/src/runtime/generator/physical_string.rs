use crate::compiler::CompiledPhysicalString;
use crate::generator_parameters::{
    PHYSICAL_STRING_BRIGHTNESS, PHYSICAL_STRING_DECAY_SECONDS, PHYSICAL_STRING_STIFFNESS,
};
use crate::process::{ProcessError, ProcessSpec};

use super::super::fractional_delay::FractionalDelayLine;
use super::super::modulation::{LayerGeneratorTargetSpan, ValueSpan};
use super::physical_exciter::{
    MIN_PHYSICAL_FREQUENCY_HZ, PHYSICAL_EXCITER_GAIN, PHYSICAL_MIN_CUTOFF_HZ,
    PhysicalExciterRuntime, physical_max_cutoff, valid_physical_frequency,
};
use super::{base_frequencies, ensure_finite, invalid_state, non_finite, validate_generator_span};

#[derive(Clone, Copy)]
struct PhysicalStringCoefficients {
    delay_frames: f32,
    feedback: f32,
    lowpass_coefficient: f32,
    allpass_coefficient: f32,
}

#[derive(Clone, Copy)]
struct PhysicalStringParameterSpans {
    base: ValueSpan,
    decay_seconds: ValueSpan,
    brightness: ValueSpan,
    stiffness: ValueSpan,
}

#[derive(Clone, Copy)]
struct PhysicalStringCoefficientCache {
    full: Option<PhysicalStringCoefficients>,
    delay_frames: Option<f32>,
    feedback: Option<f32>,
    lowpass_coefficient: Option<f32>,
    allpass_coefficient: Option<f32>,
}

impl PhysicalStringCoefficientCache {
    fn new(
        sample_rate: f32,
        max_delay_frames: f32,
        spans: PhysicalStringParameterSpans,
    ) -> Result<Self, ProcessError> {
        let full = if spans.base.is_constant()
            && spans.decay_seconds.is_constant()
            && spans.brightness.is_constant()
            && spans.stiffness.is_constant()
        {
            Some(calculate_coefficients(
                sample_rate,
                max_delay_frames,
                spans.base.start,
                spans.decay_seconds.start,
                spans.brightness.start,
                spans.stiffness.start,
            )?)
        } else {
            None
        };
        let feedback =
            if full.is_none() && spans.base.is_constant() && spans.decay_seconds.is_constant() {
                Some(calculate_feedback(
                    spans.base.start,
                    spans.decay_seconds.start,
                )?)
            } else {
                None
            };
        let lowpass_coefficient =
            if full.is_none() && spans.base.is_constant() && spans.brightness.is_constant() {
                Some(calculate_lowpass(
                    sample_rate,
                    spans.base.start,
                    spans.brightness.start,
                ))
            } else {
                None
            };
        let delay_frames =
            if full.is_none() && spans.base.is_constant() && spans.stiffness.is_constant() {
                Some(calculate_delay(
                    sample_rate,
                    max_delay_frames,
                    spans.base.start,
                    spans.stiffness.start,
                )?)
            } else {
                None
            };
        let allpass_coefficient = if spans.stiffness.is_constant() {
            Some(spans.stiffness.start * 0.75)
        } else {
            None
        };
        Ok(Self {
            full,
            delay_frames,
            feedback,
            lowpass_coefficient,
            allpass_coefficient,
        })
    }

    fn value_at(
        self,
        sample_rate: f32,
        max_delay_frames: f32,
        index: usize,
        frames: usize,
        spans: PhysicalStringParameterSpans,
    ) -> Result<PhysicalStringCoefficients, ProcessError> {
        if let Some(full) = self.full {
            return Ok(full);
        }
        let frequency = spans.base.value_at(index, frames);
        let decay = spans.decay_seconds.value_at(index, frames);
        let brightness = spans.brightness.value_at(index, frames);
        let stiffness = spans.stiffness.value_at(index, frames);
        Ok(PhysicalStringCoefficients {
            delay_frames: match self.delay_frames {
                Some(delay_frames) => delay_frames,
                None => calculate_delay(sample_rate, max_delay_frames, frequency, stiffness)?,
            },
            feedback: match self.feedback {
                Some(feedback) => feedback,
                None => calculate_feedback(frequency, decay)?,
            },
            lowpass_coefficient: self
                .lowpass_coefficient
                .unwrap_or_else(|| calculate_lowpass(sample_rate, frequency, brightness)),
            allpass_coefficient: self.allpass_coefficient.unwrap_or(stiffness * 0.75),
        })
    }
}

pub(crate) struct PhysicalStringRuntime {
    sample_rate: f32,
    max_delay_frames: usize,
    effective_max_frequency: f32,
    delay: FractionalDelayLine,
    exciter: PhysicalExciterRuntime,
    exciter_scratch: Vec<f32>,
    filter_state: f32,
    allpass_input: f32,
    allpass_output: f32,
}

impl PhysicalStringRuntime {
    pub(super) fn new(
        compiled: &CompiledPhysicalString,
        spec: ProcessSpec,
    ) -> Result<Self, ProcessError> {
        #[allow(clippy::cast_possible_truncation)]
        let sample_rate = spec.sample_rate as f32;
        if !sample_rate.is_finite()
            || sample_rate <= 0.0
            || !compiled.effective_max_frequency.is_finite()
            || compiled.effective_max_frequency <= MIN_PHYSICAL_FREQUENCY_HZ
        {
            return Err(invalid_state());
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let max_delay_frames = ((spec.sample_rate / f64::from(MIN_PHYSICAL_FREQUENCY_HZ)).ceil()
            as usize)
            .saturating_add(4);
        Ok(Self {
            sample_rate,
            max_delay_frames,
            effective_max_frequency: compiled.effective_max_frequency,
            delay: FractionalDelayLine::new(max_delay_frames),
            exciter: PhysicalExciterRuntime::new(
                compiled.exciter,
                compiled.layer_hash,
                spec.sample_rate,
            )?,
            exciter_scratch: vec![0.0; spec.max_block_size],
            filter_state: 0.0,
            allpass_input: 0.0,
            allpass_output: 0.0,
        })
    }

    pub(super) fn start(&mut self, note_id: u64) {
        self.reset();
        self.exciter.start(note_id);
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
        let LayerGeneratorTargetSpan::PhysicalString {
            decay_seconds,
            brightness,
            stiffness,
        } = targets
        else {
            return Err(invalid_state());
        };
        #[allow(clippy::cast_possible_truncation)]
        let requested_sample_rate = sample_rate as f32;
        #[allow(clippy::cast_precision_loss)]
        let max_delay_frames = self.max_delay_frames as f32;
        if mono.len() < frames
            || self.exciter_scratch.len() < frames
            || !sample_rate.is_finite()
            || sample_rate <= 0.0
            || requested_sample_rate.total_cmp(&self.sample_rate).is_ne()
        {
            return Err(invalid_state());
        }
        validate_generator_span(decay_seconds, PHYSICAL_STRING_DECAY_SECONDS)?;
        validate_generator_span(brightness, PHYSICAL_STRING_BRIGHTNESS)?;
        validate_generator_span(stiffness, PHYSICAL_STRING_STIFFNESS)?;
        let (base_start, base_end) = base_frequencies(note_number, tuning_start, tuning_end)?;
        if !valid_physical_frequency(base_start, self.effective_max_frequency)
            || !valid_physical_frequency(base_end, self.effective_max_frequency)
        {
            return Err(ProcessError::InvalidFrequency);
        }
        self.exciter
            .render(frames, &mut self.exciter_scratch[..frames])?;
        let spans = PhysicalStringParameterSpans {
            base: ValueSpan {
                start: base_start,
                end: base_end,
            },
            decay_seconds,
            brightness,
            stiffness,
        };
        let coefficient_cache =
            PhysicalStringCoefficientCache::new(self.sample_rate, max_delay_frames, spans)?;
        for (index, sample) in mono[..frames].iter_mut().enumerate() {
            let coefficients = coefficient_cache.value_at(
                self.sample_rate,
                max_delay_frames,
                index,
                frames,
                spans,
            )?;
            let delayed = self.delay.read(coefficients.delay_frames)?;
            let filtered = coefficients.lowpass_coefficient.mul_add(
                self.filter_state,
                (1.0 - coefficients.lowpass_coefficient) * delayed,
            );
            self.filter_state = filtered;
            let dispersed = coefficients.allpass_coefficient * filtered + self.allpass_input
                - coefficients.allpass_coefficient * self.allpass_output;
            self.allpass_input = filtered;
            self.allpass_output = dispersed;
            self.delay
                .write(self.exciter_scratch[index] + dispersed * coefficients.feedback)?;
            *sample = delayed + self.exciter_scratch[index] * PHYSICAL_EXCITER_GAIN;
            if !coefficients.feedback.is_finite()
                || !coefficients.lowpass_coefficient.is_finite()
                || !(*sample).is_finite()
            {
                return Err(non_finite());
            }
        }
        ensure_finite(&mono[..frames])
    }

    pub(super) fn reset(&mut self) {
        self.delay.reset();
        self.exciter.reset();
        self.filter_state = 0.0;
        self.allpass_input = 0.0;
        self.allpass_output = 0.0;
    }
}

fn calculate_coefficients(
    sample_rate: f32,
    max_delay_frames: f32,
    frequency: f32,
    decay: f32,
    brightness: f32,
    stiffness: f32,
) -> Result<PhysicalStringCoefficients, ProcessError> {
    Ok(PhysicalStringCoefficients {
        delay_frames: calculate_delay(sample_rate, max_delay_frames, frequency, stiffness)?,
        feedback: calculate_feedback(frequency, decay)?,
        lowpass_coefficient: calculate_lowpass(sample_rate, frequency, brightness),
        allpass_coefficient: stiffness * 0.75,
    })
}

fn calculate_delay(
    sample_rate: f32,
    max_delay_frames: f32,
    frequency: f32,
    stiffness: f32,
) -> Result<f32, ProcessError> {
    let period = sample_rate / frequency;
    let allpass_coefficient = stiffness * 0.75;
    let omega = std::f32::consts::TAU * frequency / sample_rate;
    let group_delay = (1.0 - allpass_coefficient * allpass_coefficient)
        / (1.0
            + allpass_coefficient * allpass_coefficient
            + 2.0 * allpass_coefficient * omega.cos());
    let delay_frames = (period - group_delay).max(0.05);
    if delay_frames.is_finite() && delay_frames + 2.0 < max_delay_frames + 4.0 {
        Ok(delay_frames)
    } else {
        Err(invalid_state())
    }
}

fn calculate_feedback(frequency: f32, decay: f32) -> Result<f32, ProcessError> {
    let feedback = 10.0_f32.powf(-3.0 / (frequency * decay));
    if feedback.is_finite() {
        Ok(feedback)
    } else {
        Err(non_finite())
    }
}

fn calculate_lowpass(sample_rate: f32, frequency: f32, brightness: f32) -> f32 {
    let max_cutoff = physical_max_cutoff(sample_rate);
    let cutoff = (frequency * 2.0_f32.powf(2.0 + brightness * 6.0))
        .clamp(PHYSICAL_MIN_CUTOFF_HZ, max_cutoff);
    (-std::f32::consts::TAU * cutoff / sample_rate).exp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{CompiledPhysicalExciter, CompiledPhysicalStringParameters};
    use crate::parameter::ParameterHandle;
    use crate::runtime::modulation::ValueSpan;

    fn compiled() -> CompiledPhysicalString {
        CompiledPhysicalString {
            exciter: CompiledPhysicalExciter::Impulse,
            parameters: CompiledPhysicalStringParameters {
                decay_seconds: ParameterHandle::new(0),
                brightness: ParameterHandle::new(1),
                stiffness: ParameterHandle::new(2),
            },
            layer_hash: 7,
            effective_max_frequency: 21_600.0,
        }
    }

    #[test]
    fn string_is_finite_and_reset_repeats() {
        let spec = ProcessSpec::new(48_000.0, 257, 2).expect("spec");
        let mut runtime = PhysicalStringRuntime::new(&compiled(), spec).expect("runtime");
        runtime.start(3);
        let targets = LayerGeneratorTargetSpan::PhysicalString {
            decay_seconds: ValueSpan {
                start: 1.0,
                end: 1.0,
            },
            brightness: ValueSpan {
                start: 0.5,
                end: 0.5,
            },
            stiffness: ValueSpan {
                start: 0.5,
                end: 0.5,
            },
        };
        let mut first = vec![0.0; 257];
        runtime
            .render(257, 69, 0.0, 0.0, 48_000.0, targets, &mut first)
            .expect("render");
        assert!(first.iter().all(|sample| sample.is_finite()));
        assert_eq!(first[0].to_bits(), 0.0625_f32.to_bits());
        runtime.start(3);
        let mut second = vec![0.0; 257];
        runtime
            .render(257, 69, 0.0, 0.0, 48_000.0, targets, &mut second)
            .expect("render after reset");
        assert_eq!(first, second);
    }

    #[test]
    fn frequency_limit_is_enforced() {
        let spec = ProcessSpec::new(48_000.0, 64, 2).expect("spec");
        let mut runtime = PhysicalStringRuntime::new(&compiled(), spec).expect("runtime");
        runtime.start(3);
        let targets = LayerGeneratorTargetSpan::PhysicalString {
            decay_seconds: ValueSpan {
                start: 1.0,
                end: 1.0,
            },
            brightness: ValueSpan {
                start: 0.5,
                end: 0.5,
            },
            stiffness: ValueSpan {
                start: 0.5,
                end: 0.5,
            },
        };
        let mut output = [0.0; 64];
        assert_eq!(
            runtime.render(64, 127, 1_200.0, 1_200.0, 48_000.0, targets, &mut output),
            Err(ProcessError::InvalidFrequency)
        );
    }

    #[test]
    fn stiffness_preserves_fundamental_pitch_within_twenty_cents() {
        let spec = ProcessSpec::new(48_000.0, 8_192, 2).expect("spec");
        let expected_frequency = 440.0_f64;
        for stiffness in [0.0, 0.5, 1.0] {
            let mut runtime = PhysicalStringRuntime::new(&compiled(), spec).expect("runtime");
            runtime.start(3);
            let targets = LayerGeneratorTargetSpan::PhysicalString {
                decay_seconds: ValueSpan {
                    start: 4.0,
                    end: 4.0,
                },
                brightness: ValueSpan {
                    start: 0.5,
                    end: 0.5,
                },
                stiffness: ValueSpan {
                    start: stiffness,
                    end: stiffness,
                },
            };
            let mut output = vec![0.0; 8_192];
            runtime
                .render(8_192, 69, 0.0, 0.0, 48_000.0, targets, &mut output)
                .expect("render");

            let window = &output[512..];
            let expected_lag = 48_000.0 / expected_frequency;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let minimum_lag = (expected_lag * 0.8).round() as usize;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let maximum_lag = (expected_lag * 1.2).round() as usize;
            let mut best_lag = 0_usize;
            let mut best_correlation = f64::NEG_INFINITY;
            for lag in minimum_lag..=maximum_lag {
                let first = &window[..window.len() - lag];
                let second = &window[lag..];
                let first_energy: f64 = first
                    .iter()
                    .map(|sample| f64::from(*sample) * f64::from(*sample))
                    .sum();
                let second_energy: f64 = second
                    .iter()
                    .map(|sample| f64::from(*sample) * f64::from(*sample))
                    .sum();
                let normalization = (first_energy * second_energy).sqrt();
                if normalization <= 0.0 {
                    continue;
                }
                let correlation: f64 = first
                    .iter()
                    .zip(second)
                    .map(|(left, right)| f64::from(*left) * f64::from(*right))
                    .sum::<f64>()
                    / normalization;
                if correlation > best_correlation {
                    best_lag = lag;
                    best_correlation = correlation;
                }
            }
            assert!(best_lag > 0 && best_correlation.is_finite());
            #[allow(clippy::cast_precision_loss)]
            let estimated_frequency = 48_000.0 / best_lag as f64;
            let error_cents = 1_200.0 * (estimated_frequency / expected_frequency).log2();
            assert!(
                error_cents.abs() <= 20.0,
                "stiffness {stiffness} pitch error: {error_cents} cents"
            );
        }
    }

    #[test]
    fn long_decay_keeps_more_feedback_tail_energy() {
        let spec = ProcessSpec::new(48_000.0, 8_192, 2).expect("spec");
        let render = |decay_seconds: f32| {
            let mut runtime = PhysicalStringRuntime::new(&compiled(), spec).expect("runtime");
            runtime.start(3);
            let targets = LayerGeneratorTargetSpan::PhysicalString {
                decay_seconds: ValueSpan {
                    start: decay_seconds,
                    end: decay_seconds,
                },
                brightness: ValueSpan {
                    start: 0.5,
                    end: 0.5,
                },
                stiffness: ValueSpan {
                    start: 0.0,
                    end: 0.0,
                },
            };
            let mut output = vec![0.0; 8_192];
            runtime
                .render(8_192, 69, 0.0, 0.0, 48_000.0, targets, &mut output)
                .expect("render");
            output
        };
        let short = render(0.25);
        let long = render(5.0);
        let short_tail_energy: f64 = short[2_048..]
            .iter()
            .map(|sample| f64::from(*sample) * f64::from(*sample))
            .sum();
        let long_tail_energy: f64 = long[2_048..]
            .iter()
            .map(|sample| f64::from(*sample) * f64::from(*sample))
            .sum();
        assert!(long_tail_energy > short_tail_energy * 2.0);
    }

    #[test]
    fn brightness_changes_feedback_tail_energy() {
        let spec = ProcessSpec::new(48_000.0, 4_096, 2).expect("spec");
        let render = |brightness: f32| {
            let mut runtime = PhysicalStringRuntime::new(&compiled(), spec).expect("runtime");
            runtime.start(3);
            let targets = LayerGeneratorTargetSpan::PhysicalString {
                decay_seconds: ValueSpan {
                    start: 4.0,
                    end: 4.0,
                },
                brightness: ValueSpan {
                    start: brightness,
                    end: brightness,
                },
                stiffness: ValueSpan {
                    start: 0.0,
                    end: 0.0,
                },
            };
            let mut output = vec![0.0; 4_096];
            runtime
                .render(4_096, 69, 0.0, 0.0, 48_000.0, targets, &mut output)
                .expect("render");
            output
        };

        let dark = render(0.0);
        let bright = render(1.0);
        let dark_tail_energy: f32 = dark[512..].iter().map(|sample| sample * sample).sum();
        let bright_tail_energy: f32 = bright[512..].iter().map(|sample| sample * sample).sum();
        assert!(bright_tail_energy > dark_tail_energy * 1.5);
    }
}
