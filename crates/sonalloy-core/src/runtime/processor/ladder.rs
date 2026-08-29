use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;

pub(crate) struct LadderFilterRuntime {
    sample_rate: f32,
    left: [f32; 4],
    right: [f32; 4],
}

#[derive(Clone, Copy)]
struct LadderCoefficients {
    integrator_g: f32,
    input_gain: f32,
    feedback: f32,
}

impl LadderFilterRuntime {
    pub(crate) fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            left: [0.0; 4],
            right: [0.0; 4],
        }
    }

    pub(crate) fn process_mono(
        &mut self,
        cutoff: ValueSpan,
        resonance: ValueSpan,
        drive: ValueSpan,
        buffer: &mut [f32],
    ) -> Result<(), ProcessError> {
        let constant_coefficients =
            if cutoff.is_constant() && resonance.is_constant() && drive.is_constant() {
                Some(Self::coefficients(
                    self.sample_rate,
                    cutoff.start,
                    resonance.start,
                    drive.start,
                )?)
            } else {
                None
            };
        for index in 0..buffer.len() {
            let coefficients = match constant_coefficients {
                Some(coefficients) => coefficients,
                None => Self::coefficients(
                    self.sample_rate,
                    cutoff.value_at(index, buffer.len()),
                    resonance.value_at(index, buffer.len()),
                    drive.value_at(index, buffer.len()),
                )?,
            };
            buffer[index] = Self::process_sample(buffer[index], coefficients, &mut self.left)?;
        }
        Ok(())
    }

    pub(crate) fn process_stereo(
        &mut self,
        cutoff: ValueSpan,
        resonance: ValueSpan,
        drive: ValueSpan,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(invalid_state());
        }
        let constant_coefficients =
            if cutoff.is_constant() && resonance.is_constant() && drive.is_constant() {
                Some(Self::coefficients(
                    self.sample_rate,
                    cutoff.start,
                    resonance.start,
                    drive.start,
                )?)
            } else {
                None
            };
        for index in 0..left.len() {
            let coefficients = match constant_coefficients {
                Some(coefficients) => coefficients,
                None => Self::coefficients(
                    self.sample_rate,
                    cutoff.value_at(index, left.len()),
                    resonance.value_at(index, left.len()),
                    drive.value_at(index, left.len()),
                )?,
            };
            left[index] = Self::process_sample(left[index], coefficients, &mut self.left)?;
            right[index] = Self::process_sample(right[index], coefficients, &mut self.right)?;
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.left = [0.0; 4];
        self.right = [0.0; 4];
    }

    fn process_sample(
        input: f32,
        coefficients: LadderCoefficients,
        state: &mut [f32; 4],
    ) -> Result<f32, ProcessError> {
        if !input.is_finite() || !state.iter().all(|value| value.is_finite()) {
            return Err(non_finite());
        }
        let feedback_input = input * coefficients.input_gain - coefficients.feedback * state[3];
        let driven = if coefficients.input_gain.total_cmp(&1.0).is_eq()
            && coefficients.feedback.total_cmp(&0.0).is_eq()
        {
            input
        } else {
            feedback_input.tanh()
        };
        let mut value = driven;
        for stage in state.iter_mut() {
            let v = (value - *stage) * coefficients.integrator_g;
            let output = v + *stage;
            *stage = output + v;
            value = output;
        }
        if value.is_finite() && state.iter().all(|sample| sample.is_finite()) {
            Ok(value)
        } else {
            Err(non_finite())
        }
    }

    fn coefficients(
        sample_rate: f32,
        cutoff: f32,
        resonance: f32,
        drive: f32,
    ) -> Result<LadderCoefficients, ProcessError> {
        if !sample_rate.is_finite()
            || sample_rate <= 0.0
            || !cutoff.is_finite()
            || !resonance.is_finite()
            || !drive.is_finite()
        {
            return Err(non_finite());
        }
        let cutoff = cutoff.clamp(20.0, sample_rate * 0.45);
        let tangent = (std::f32::consts::PI * cutoff / sample_rate).tan();
        let integrator_g = tangent / (1.0 + tangent);
        let input_gain = 10.0_f32.powf(drive.clamp(0.0, 1.0) * 24.0 / 20.0);
        let feedback = resonance.clamp(0.0, 1.0) * 3.7;
        if !integrator_g.is_finite() || !input_gain.is_finite() || !feedback.is_finite() {
            return Err(non_finite());
        }
        Ok(LadderCoefficients {
            integrator_g,
            input_gain,
            feedback,
        })
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
    use super::LadderFilterRuntime;
    use crate::runtime::modulation::ValueSpan;

    fn span(value: f32) -> ValueSpan {
        ValueSpan {
            start: value,
            end: value,
        }
    }

    #[test]
    fn maximum_resonance_and_drive_remain_finite_and_resettable() {
        let mut runtime = LadderFilterRuntime::new(48_000.0);
        let mut first = [0.0; 128];
        first[0] = 1.0;
        runtime
            .process_mono(span(1_000.0), span(1.0), span(1.0), &mut first)
            .expect("ladder filter processes");
        assert!(first.iter().all(|sample| sample.is_finite()));
        assert!(first.iter().any(|sample| sample.abs() > 1.0e-6));

        runtime.reset();
        let mut second = [0.0; 128];
        second[0] = 1.0;
        runtime
            .process_mono(span(1_000.0), span(1.0), span(1.0), &mut second)
            .expect("reset ladder filter processes");
        assert!(
            first
                .iter()
                .zip(second)
                .all(|(first, second)| first.to_bits() == second.to_bits())
        );
    }

    #[test]
    fn cutoff_frequency_response_remains_near_the_requested_corner() {
        for cutoff in [250.0, 1_000.0, 4_000.0] {
            let mut runtime = LadderFilterRuntime::new(48_000.0);
            let mut buffer = vec![0.0; 48_000];
            for (index, sample) in buffer.iter_mut().enumerate() {
                #[allow(clippy::cast_precision_loss)]
                let phase = std::f32::consts::TAU * cutoff * index as f32 / 48_000.0;
                *sample = 0.01 * phase.sin();
            }
            runtime
                .process_mono(span(cutoff), span(0.0), span(0.0), &mut buffer)
                .expect("ladder filter processes a small-signal sine");

            let start = 24_000;
            let input_rms = 0.01 / 2.0_f32.sqrt();
            let output_rms = (buffer[start..]
                .iter()
                .map(|sample| sample * sample)
                .sum::<f32>()
                / {
                    #[allow(clippy::cast_precision_loss)]
                    {
                        (buffer.len() - start) as f32
                    }
                })
            .sqrt();
            let gain = output_rms / input_rms;
            assert!(
                (0.12..=0.40).contains(&gain),
                "cutoff={cutoff} Hz, measured gain={gain}"
            );
        }
    }
}
