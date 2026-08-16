use crate::compiler::{CompiledEqProcessor, GeneratorOutputMode};
use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;

#[derive(Clone, Copy, Default)]
struct BiquadState {
    z1: f32,
    z2: f32,
}

#[derive(Clone, Copy)]
struct BiquadCoefficients {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

pub(crate) struct EqRuntime {
    sample_rate: f32,
    low_frequency_hz: f32,
    mid_frequency_hz: f32,
    mid_q: f32,
    high_frequency_hz: f32,
    left: [BiquadState; 3],
    right: Option<[BiquadState; 3]>,
}

impl EqRuntime {
    pub(crate) fn new(
        compiled: &CompiledEqProcessor,
        sample_rate: f32,
        output_mode: GeneratorOutputMode,
    ) -> Self {
        Self {
            sample_rate,
            low_frequency_hz: compiled.low_frequency_hz,
            mid_frequency_hz: compiled.mid_frequency_hz,
            mid_q: compiled.mid_q,
            high_frequency_hz: compiled.high_frequency_hz,
            left: [BiquadState::default(); 3],
            right: match output_mode {
                GeneratorOutputMode::Mono => None,
                GeneratorOutputMode::Stereo => Some([BiquadState::default(); 3]),
            },
        }
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
            self.sample_rate,
            self.low_frequency_hz,
            self.mid_frequency_hz,
            self.mid_q,
            self.high_frequency_hz,
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
            self.sample_rate,
            self.low_frequency_hz,
            self.mid_frequency_hz,
            self.mid_q,
            self.high_frequency_hz,
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
            self.sample_rate,
            self.low_frequency_hz,
            self.mid_frequency_hz,
            self.mid_q,
            self.high_frequency_hz,
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
    sample_rate: f32,
    low_frequency_hz: f32,
    mid_frequency_hz: f32,
    mid_q: f32,
    high_frequency_hz: f32,
    low_gain_db: ValueSpan,
    mid_gain_db: ValueSpan,
    high_gain_db: ValueSpan,
    buffer: &mut [f32],
) -> Result<(), ProcessError> {
    for index in 0..buffer.len() {
        let input = buffer[index];
        let low_gain = low_gain_db.value_at(index, buffer.len());
        let mid_gain = mid_gain_db.value_at(index, buffer.len());
        let high_gain = high_gain_db.value_at(index, buffer.len());
        let mut value = input;
        let coefficients = [
            low_shelf(low_frequency_hz, low_gain, sample_rate)?,
            peaking(mid_frequency_hz, mid_q, mid_gain, sample_rate)?,
            high_shelf(high_frequency_hz, high_gain, sample_rate)?,
        ];
        for (state, coefficient) in states.iter_mut().zip(coefficients) {
            value = process_biquad(*state, coefficient, value, state);
        }
        if !value.is_finite() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            });
        }
        buffer[index] = value;
    }
    Ok(())
}

fn process_biquad(
    previous: BiquadState,
    coefficients: BiquadCoefficients,
    input: f32,
    state: &mut BiquadState,
) -> f32 {
    let output = coefficients.b0 * input + previous.z1;
    state.z1 = coefficients.b1 * input - coefficients.a1 * output + previous.z2;
    state.z2 = coefficients.b2 * input - coefficients.a2 * output;
    output
}

fn low_shelf(
    frequency_hz: f32,
    gain_db: f32,
    sample_rate: f32,
) -> Result<BiquadCoefficients, ProcessError> {
    let w = angular_frequency(frequency_hz, sample_rate)?;
    let (sin, cos) = w.sin_cos();
    let amplitude = gain_db_to_amplitude(gain_db)?;
    let alpha = sin / 2.0;
    let sqrt_amplitude = amplitude.sqrt();
    normalize(
        amplitude * ((amplitude + 1.0) - (amplitude - 1.0) * cos + 2.0 * sqrt_amplitude * alpha),
        2.0 * amplitude * ((amplitude - 1.0) - (amplitude + 1.0) * cos),
        amplitude * ((amplitude + 1.0) - (amplitude - 1.0) * cos - 2.0 * sqrt_amplitude * alpha),
        (amplitude + 1.0) + (amplitude - 1.0) * cos + 2.0 * sqrt_amplitude * alpha,
        -2.0 * ((amplitude - 1.0) + (amplitude + 1.0) * cos),
        (amplitude + 1.0) + (amplitude - 1.0) * cos - 2.0 * sqrt_amplitude * alpha,
    )
}

fn peaking(
    frequency_hz: f32,
    q: f32,
    gain_db: f32,
    sample_rate: f32,
) -> Result<BiquadCoefficients, ProcessError> {
    let w = angular_frequency(frequency_hz, sample_rate)?;
    let (sin, cos) = w.sin_cos();
    let amplitude = gain_db_to_amplitude(gain_db)?;
    let alpha = sin / (2.0 * q);
    normalize(
        1.0 + alpha * amplitude,
        -2.0 * cos,
        1.0 - alpha * amplitude,
        1.0 + alpha / amplitude,
        -2.0 * cos,
        1.0 - alpha / amplitude,
    )
}

fn high_shelf(
    frequency_hz: f32,
    gain_db: f32,
    sample_rate: f32,
) -> Result<BiquadCoefficients, ProcessError> {
    let w = angular_frequency(frequency_hz, sample_rate)?;
    let (sin, cos) = w.sin_cos();
    let amplitude = gain_db_to_amplitude(gain_db)?;
    let alpha = sin / 2.0;
    let sqrt_amplitude = amplitude.sqrt();
    normalize(
        amplitude * ((amplitude + 1.0) + (amplitude - 1.0) * cos + 2.0 * sqrt_amplitude * alpha),
        -2.0 * amplitude * ((amplitude - 1.0) + (amplitude + 1.0) * cos),
        amplitude * ((amplitude + 1.0) + (amplitude - 1.0) * cos - 2.0 * sqrt_amplitude * alpha),
        (amplitude + 1.0) - (amplitude - 1.0) * cos + 2.0 * sqrt_amplitude * alpha,
        2.0 * ((amplitude - 1.0) - (amplitude + 1.0) * cos),
        (amplitude + 1.0) - (amplitude - 1.0) * cos - 2.0 * sqrt_amplitude * alpha,
    )
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
