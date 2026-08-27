use crate::compiler::{CompiledEqProcessor, GeneratorOutputMode};
use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;
use super::biquad::{BiquadCoefficients, BiquadState};

#[derive(Clone, Copy)]
struct FrequencyTerms {
    cos_w0: f32,
    alpha: f32,
}

pub(crate) struct EqRuntime {
    low: FrequencyTerms,
    mid: FrequencyTerms,
    high: FrequencyTerms,
    left: [BiquadState; 3],
    right: Option<[BiquadState; 3]>,
}

impl EqRuntime {
    pub(crate) fn new(
        compiled: &CompiledEqProcessor,
        sample_rate: f32,
        output_mode: GeneratorOutputMode,
    ) -> Result<Self, ProcessError> {
        let low = shelf_frequency_terms(compiled.low_frequency_hz, sample_rate)?;
        let mid = peaking_frequency_terms(compiled.mid_frequency_hz, compiled.mid_q, sample_rate)?;
        let high = shelf_frequency_terms(compiled.high_frequency_hz, sample_rate)?;
        Ok(Self {
            low,
            mid,
            high,
            left: [BiquadState::default(); 3],
            right: match output_mode {
                GeneratorOutputMode::Mono => None,
                GeneratorOutputMode::Stereo => Some([BiquadState::default(); 3]),
            },
        })
    }

    pub(crate) fn process_mono(
        &mut self,
        low_gain_db: ValueSpan,
        mid_gain_db: ValueSpan,
        high_gain_db: ValueSpan,
        buffer: &mut [f32],
    ) -> Result<(), ProcessError> {
        process_channel(
            &mut self.left,
            self.low,
            self.mid,
            self.high,
            low_gain_db,
            mid_gain_db,
            high_gain_db,
            buffer,
        )
    }

    pub(crate) fn process_stereo(
        &mut self,
        low_gain_db: ValueSpan,
        mid_gain_db: ValueSpan,
        high_gain_db: ValueSpan,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            });
        }
        process_channel(
            &mut self.left,
            self.low,
            self.mid,
            self.high,
            low_gain_db,
            mid_gain_db,
            high_gain_db,
            left,
        )?;
        let right_state = self.right.as_mut().ok_or(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::InvalidState,
        })?;
        process_channel(
            right_state,
            self.low,
            self.mid,
            self.high,
            low_gain_db,
            mid_gain_db,
            high_gain_db,
            right,
        )
    }

    pub(crate) fn reset(&mut self) {
        self.left = [BiquadState::default(); 3];
        if let Some(right) = &mut self.right {
            *right = [BiquadState::default(); 3];
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn process_channel(
    states: &mut [BiquadState; 3],
    low_terms: FrequencyTerms,
    mid_terms: FrequencyTerms,
    high_terms: FrequencyTerms,
    low_gain_db: ValueSpan,
    mid_gain_db: ValueSpan,
    high_gain_db: ValueSpan,
    buffer: &mut [f32],
) -> Result<(), ProcessError> {
    if buffer.is_empty() {
        return Ok(());
    }
    let low_static = if low_gain_db.is_constant() {
        Some(low_shelf(low_terms, low_gain_db.start)?)
    } else {
        None
    };
    let mid_static = if mid_gain_db.is_constant() {
        Some(peaking(mid_terms, mid_gain_db.start)?)
    } else {
        None
    };
    let high_static = if high_gain_db.is_constant() {
        Some(high_shelf(high_terms, high_gain_db.start)?)
    } else {
        None
    };

    for index in 0..buffer.len() {
        let mut value = buffer[index];
        let low = match low_static {
            Some(coefficients) => coefficients,
            None => low_shelf(low_terms, low_gain_db.value_at(index, buffer.len()))?,
        };
        value = states[0].process(low, value)?;

        let mid = match mid_static {
            Some(coefficients) => coefficients,
            None => peaking(mid_terms, mid_gain_db.value_at(index, buffer.len()))?,
        };
        value = states[1].process(mid, value)?;

        let high = match high_static {
            Some(coefficients) => coefficients,
            None => high_shelf(high_terms, high_gain_db.value_at(index, buffer.len()))?,
        };
        value = states[2].process(high, value)?;

        if !value.is_finite() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            });
        }
        buffer[index] = value;
    }
    Ok(())
}

fn low_shelf(terms: FrequencyTerms, gain_db: f32) -> Result<BiquadCoefficients, ProcessError> {
    let amplitude = gain_db_to_amplitude(gain_db)?;
    let sqrt_amplitude = amplitude.sqrt();
    normalize(
        amplitude
            * ((amplitude + 1.0) - (amplitude - 1.0) * terms.cos_w0
                + 2.0 * sqrt_amplitude * terms.alpha),
        2.0 * amplitude * ((amplitude - 1.0) - (amplitude + 1.0) * terms.cos_w0),
        amplitude
            * ((amplitude + 1.0)
                - (amplitude - 1.0) * terms.cos_w0
                - 2.0 * sqrt_amplitude * terms.alpha),
        (amplitude + 1.0) + (amplitude - 1.0) * terms.cos_w0 + 2.0 * sqrt_amplitude * terms.alpha,
        -2.0 * ((amplitude - 1.0) + (amplitude + 1.0) * terms.cos_w0),
        (amplitude + 1.0) + (amplitude - 1.0) * terms.cos_w0 - 2.0 * sqrt_amplitude * terms.alpha,
    )
}

fn peaking(terms: FrequencyTerms, gain_db: f32) -> Result<BiquadCoefficients, ProcessError> {
    let amplitude = gain_db_to_amplitude(gain_db)?;
    normalize(
        1.0 + terms.alpha * amplitude,
        -2.0 * terms.cos_w0,
        1.0 - terms.alpha * amplitude,
        1.0 + terms.alpha / amplitude,
        -2.0 * terms.cos_w0,
        1.0 - terms.alpha / amplitude,
    )
}

fn high_shelf(terms: FrequencyTerms, gain_db: f32) -> Result<BiquadCoefficients, ProcessError> {
    let amplitude = gain_db_to_amplitude(gain_db)?;
    let sqrt_amplitude = amplitude.sqrt();
    normalize(
        amplitude
            * ((amplitude + 1.0)
                + (amplitude - 1.0) * terms.cos_w0
                + 2.0 * sqrt_amplitude * terms.alpha),
        -2.0 * amplitude * ((amplitude - 1.0) + (amplitude + 1.0) * terms.cos_w0),
        amplitude
            * ((amplitude + 1.0) + (amplitude - 1.0) * terms.cos_w0
                - 2.0 * sqrt_amplitude * terms.alpha),
        (amplitude + 1.0) - (amplitude - 1.0) * terms.cos_w0 + 2.0 * sqrt_amplitude * terms.alpha,
        2.0 * ((amplitude - 1.0) - (amplitude + 1.0) * terms.cos_w0),
        (amplitude + 1.0) - (amplitude - 1.0) * terms.cos_w0 - 2.0 * sqrt_amplitude * terms.alpha,
    )
}

fn shelf_frequency_terms(
    frequency_hz: f32,
    sample_rate: f32,
) -> Result<FrequencyTerms, ProcessError> {
    let w = angular_frequency(frequency_hz, sample_rate)?;
    let (sin_w0, cos_w0) = w.sin_cos();
    Ok(FrequencyTerms {
        cos_w0,
        // RBJ Audio EQ Cookbook shelf slope S=1.
        alpha: sin_w0 * std::f32::consts::FRAC_1_SQRT_2,
    })
}

fn peaking_frequency_terms(
    frequency_hz: f32,
    q: f32,
    sample_rate: f32,
) -> Result<FrequencyTerms, ProcessError> {
    let w = angular_frequency(frequency_hz, sample_rate)?;
    if !q.is_finite() || q <= 0.0 {
        return Err(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::InvalidState,
        });
    }
    let (sin_w0, cos_w0) = w.sin_cos();
    Ok(FrequencyTerms {
        cos_w0,
        alpha: sin_w0 / (2.0 * q),
    })
}

fn angular_frequency(frequency_hz: f32, sample_rate: f32) -> Result<f32, ProcessError> {
    let value = 2.0 * std::f32::consts::PI * frequency_hz / sample_rate;
    if value.is_finite() && value > 0.0 && value < std::f32::consts::PI {
        Ok(value)
    } else {
        Err(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::InvalidState,
        })
    }
}

fn gain_db_to_amplitude(gain_db: f32) -> Result<f32, ProcessError> {
    let amplitude = 10.0_f32.powf(gain_db / 40.0);
    if amplitude.is_finite() && amplitude > 0.0 {
        Ok(amplitude)
    } else {
        Err(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::NonFinite,
        })
    }
}

fn normalize(
    b0: f32,
    b1: f32,
    b2: f32,
    a0: f32,
    a1: f32,
    a2: f32,
) -> Result<BiquadCoefficients, ProcessError> {
    let coefficients = BiquadCoefficients {
        b0: b0 / a0,
        b1: b1 / a0,
        b2: b2 / a0,
        a1: a1 / a0,
        a2: a2 / a0,
    };
    if [
        coefficients.b0,
        coefficients.b1,
        coefficients.b2,
        coefficients.a1,
        coefficients.a2,
    ]
    .into_iter()
    .all(f32::is_finite)
    {
        Ok(coefficients)
    } else {
        Err(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::NonFinite,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::CompiledEqParameters;
    use crate::parameter::ParameterHandle;

    fn span(value: f32) -> ValueSpan {
        ValueSpan {
            start: value,
            end: value,
        }
    }

    #[test]
    fn shelf_terms_use_rbj_slope_and_expected_end_point_gain() {
        let low_terms = shelf_frequency_terms(100.0, 48_000.0).expect("valid low shelf");
        let high_terms = shelf_frequency_terms(8_000.0, 48_000.0).expect("valid high shelf");
        let low_omega = 2.0 * std::f32::consts::PI * 100.0 / 48_000.0;
        assert!(
            (low_terms.alpha - low_omega.sin() * std::f32::consts::FRAC_1_SQRT_2).abs() < 1.0e-7
        );
        let low = low_shelf(low_terms, 6.0).expect("low shelf coefficients");
        let high = high_shelf(high_terms, 6.0).expect("high shelf coefficients");
        let expected = 10.0_f32.powf(6.0 / 20.0);
        assert!((dc_gain(low) - expected).abs() < 5.0e-4);
        assert!((nyquist_gain(high) - expected).abs() < 5.0e-4);
    }

    #[test]
    fn peaking_terms_reach_the_requested_center_gain() {
        let frequency_hz = 1_000.0;
        let sample_rate = 48_000.0;
        let terms =
            peaking_frequency_terms(frequency_hz, 1.0, sample_rate).expect("valid peaking band");
        let coefficients = peaking(terms, 6.0).expect("peaking coefficients");
        let omega = 2.0 * std::f32::consts::PI * frequency_hz / sample_rate;
        let expected = 10.0_f32.powf(6.0 / 20.0);
        assert!((frequency_response(coefficients, omega) - expected).abs() < 1.0e-4);
    }

    #[test]
    fn zero_gain_is_an_identity() {
        let compiled = CompiledEqProcessor {
            low_frequency_hz: 100.0,
            mid_frequency_hz: 1_000.0,
            mid_q: 1.0,
            high_frequency_hz: 8_000.0,
            parameters: CompiledEqParameters {
                low_gain_db: ParameterHandle::new(0),
                mid_gain_db: ParameterHandle::new(1),
                high_gain_db: ParameterHandle::new(2),
            },
        };
        let mut runtime =
            EqRuntime::new(&compiled, 48_000.0, GeneratorOutputMode::Mono).expect("EQ prepares");
        let original = [0.1, -0.8, 0.4, 0.0, 0.7, -0.2];
        let mut buffer = original;
        runtime
            .process_mono(span(0.0), span(0.0), span(0.0), &mut buffer)
            .expect("EQ processes");
        assert!(
            original
                .into_iter()
                .zip(buffer)
                .all(|(expected, actual)| (expected - actual).abs() < 1.0e-6)
        );
    }

    #[test]
    fn gain_ramp_updates_coefficients_without_non_finite_output() {
        let compiled = CompiledEqProcessor {
            low_frequency_hz: 100.0,
            mid_frequency_hz: 1_000.0,
            mid_q: 1.0,
            high_frequency_hz: 8_000.0,
            parameters: CompiledEqParameters {
                low_gain_db: ParameterHandle::new(0),
                mid_gain_db: ParameterHandle::new(1),
                high_gain_db: ParameterHandle::new(2),
            },
        };
        let mut runtime =
            EqRuntime::new(&compiled, 48_000.0, GeneratorOutputMode::Mono).expect("EQ prepares");
        let mut buffer = [0.0; 32];
        buffer[0] = 1.0;
        runtime
            .process_mono(
                ValueSpan {
                    start: -12.0,
                    end: 12.0,
                },
                span(0.0),
                span(0.0),
                &mut buffer,
            )
            .expect("EQ gain ramp processes");
        assert!(buffer.iter().all(|sample| sample.is_finite()));
        assert!(buffer.iter().skip(1).any(|sample| sample.abs() > 1.0e-5));
    }

    fn dc_gain(coefficients: BiquadCoefficients) -> f32 {
        (coefficients.b0 + coefficients.b1 + coefficients.b2)
            / (1.0 + coefficients.a1 + coefficients.a2)
    }

    fn nyquist_gain(coefficients: BiquadCoefficients) -> f32 {
        (coefficients.b0 - coefficients.b1 + coefficients.b2)
            / (1.0 - coefficients.a1 + coefficients.a2)
    }

    fn frequency_response(coefficients: BiquadCoefficients, omega: f32) -> f32 {
        let (sin_omega, cos_omega) = omega.sin_cos();
        let (sin_double_omega, cos_double_omega) = (2.0 * omega).sin_cos();
        let numerator = (
            coefficients.b0 + coefficients.b1 * cos_omega + coefficients.b2 * cos_double_omega,
            -coefficients.b1 * sin_omega - coefficients.b2 * sin_double_omega,
        );
        let denominator = (
            1.0 + coefficients.a1 * cos_omega + coefficients.a2 * cos_double_omega,
            -coefficients.a1 * sin_omega - coefficients.a2 * sin_double_omega,
        );
        numerator.0.hypot(numerator.1) / denominator.0.hypot(denominator.1)
    }
}
