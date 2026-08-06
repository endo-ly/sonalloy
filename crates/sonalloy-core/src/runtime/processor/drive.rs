use crate::process::{ProcessError, ProcessorFailureKind};

const DRIVE_SHAPE: f32 = 4.0;

pub(crate) struct DriveRuntime;

impl DriveRuntime {
    pub(crate) fn process_mono(
        amount: super::ValueSpan,
        mix: super::ValueSpan,
        buffer: &mut [f32],
    ) -> Result<(), ProcessError> {
        for index in 0..buffer.len() {
            let amount = amount.value_at(index, buffer.len());
            let mix = mix.value_at(index, buffer.len());
            buffer[index] = process_sample(buffer[index], amount, mix)?;
        }
        Ok(())
    }

    pub(crate) fn process_stereo(
        amount: super::ValueSpan,
        mix: super::ValueSpan,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            });
        }
        for index in 0..left.len() {
            let amount = amount.value_at(index, left.len());
            let mix = mix.value_at(index, left.len());
            left[index] = process_sample(left[index], amount, mix)?;
            right[index] = process_sample(right[index], amount, mix)?;
        }
        Ok(())
    }
}

fn process_sample(input: f32, amount: f32, mix: f32) -> Result<f32, ProcessError> {
    if !input.is_finite() || !amount.is_finite() || !mix.is_finite() {
        return Err(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::NonFinite,
        });
    }
    if amount == 0.0 || mix == 0.0 {
        return Ok(input);
    }
    let shape = amount * DRIVE_SHAPE;
    let normalization = shape.tanh();
    if !normalization.is_finite() || normalization == 0.0 {
        return Err(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::NonFinite,
        });
    }
    let wet = (shape * input).tanh() / normalization;
    let output = input + (wet - input) * mix;
    if output.is_finite() {
        Ok(output)
    } else {
        Err(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::NonFinite,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amount_zero_is_identity() {
        let mut buffer = [-1.0, -0.25, 0.0, 0.5, 1.0];
        DriveRuntime::process_mono(
            super::super::ValueSpan {
                start: 0.0,
                end: 0.0,
            },
            super::super::ValueSpan {
                start: 1.0,
                end: 1.0,
            },
            &mut buffer,
        )
        .expect("drive process");
        assert!(
            buffer
                .iter()
                .zip([-1.0, -0.25, 0.0, 0.5, 1.0])
                .all(|(actual, expected)| (actual - expected).abs() < 1.0e-6)
        );
    }

    #[test]
    fn wet_output_is_odd_and_finite() {
        let mut positive = [0.75];
        let mut negative = [-0.75];
        let span = super::super::ValueSpan {
            start: 0.8,
            end: 0.8,
        };
        let mix = super::super::ValueSpan {
            start: 1.0,
            end: 1.0,
        };
        DriveRuntime::process_mono(span, mix, &mut positive).expect("positive drive");
        DriveRuntime::process_mono(span, mix, &mut negative).expect("negative drive");
        assert!((positive[0] + negative[0]).abs() < 1.0e-6);
        assert!(positive[0].is_finite());
    }

    #[test]
    fn ramp_is_independent_of_span_partitioning() {
        #[allow(clippy::cast_precision_loss)]
        let input: Vec<f32> = (0..32).map(|index| index as f32 / 31.0 - 0.5).collect();
        let mut whole = input.clone();
        DriveRuntime::process_mono(
            super::super::ValueSpan {
                start: 0.0,
                end: 1.0,
            },
            super::super::ValueSpan {
                start: 0.2,
                end: 0.8,
            },
            &mut whole,
        )
        .expect("whole drive process");

        let mut split = input;
        DriveRuntime::process_mono(
            super::super::ValueSpan {
                start: 0.0,
                end: 1.0 / 32.0,
            },
            super::super::ValueSpan {
                start: 0.2,
                end: 0.2 + 0.6 / 32.0,
            },
            &mut split[..1],
        )
        .expect("first split drive process");
        DriveRuntime::process_mono(
            super::super::ValueSpan {
                start: 1.0 / 32.0,
                end: 1.0,
            },
            super::super::ValueSpan {
                start: 0.2 + 0.6 / 32.0,
                end: 0.8,
            },
            &mut split[1..],
        )
        .expect("second split drive process");

        assert!(
            whole
                .iter()
                .zip(split)
                .all(|(left, right)| (left - right).abs() < 1.0e-6)
        );
    }
}
