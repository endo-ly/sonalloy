use crate::process::{ProcessError, ProcessorFailureKind};

#[derive(Clone, Copy)]
pub(crate) struct BiquadCoefficients {
    pub(crate) b0: f32,
    pub(crate) b1: f32,
    pub(crate) b2: f32,
    pub(crate) a1: f32,
    pub(crate) a2: f32,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct BiquadState {
    z1: f32,
    z2: f32,
}

impl BiquadCoefficients {
    pub(crate) fn band_pass(
        sample_rate: f32,
        frequency: f32,
        bandwidth: f32,
    ) -> Result<Self, ProcessError> {
        let omega = std::f32::consts::TAU * frequency / sample_rate;
        let q = (frequency / bandwidth).clamp(0.1, 50.0);
        let alpha = omega.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        let coefficients = Self {
            b0: alpha / a0,
            b1: 0.0,
            b2: -alpha / a0,
            a1: -2.0 * omega.cos() / a0,
            a2: (1.0 - alpha) / a0,
        };
        if coefficients.is_finite() {
            Ok(coefficients)
        } else {
            Err(invalid_state())
        }
    }

    pub(crate) fn is_finite(self) -> bool {
        [self.b0, self.b1, self.b2, self.a1, self.a2]
            .into_iter()
            .all(f32::is_finite)
    }
}

impl BiquadState {
    pub(crate) fn process(
        &mut self,
        coefficients: BiquadCoefficients,
        input: f32,
    ) -> Result<f32, ProcessError> {
        if !input.is_finite() || !self.z1.is_finite() || !self.z2.is_finite() {
            return Err(non_finite());
        }
        let output = coefficients.b0 * input + self.z1;
        self.z1 = coefficients.b1 * input - coefficients.a1 * output + self.z2;
        self.z2 = coefficients.b2 * input - coefficients.a2 * output;
        if output.is_finite() && self.z1.is_finite() && self.z2.is_finite() {
            Ok(output)
        } else {
            Err(non_finite())
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::default();
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
