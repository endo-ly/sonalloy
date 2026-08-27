use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;

pub(crate) struct LadderFilterRuntime {
    sample_rate: f32,
    left: [f32; 4],
    right: [f32; 4],
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
        for index in 0..buffer.len() {
            buffer[index] = Self::process_sample(
                self.sample_rate,
                buffer[index],
                cutoff.value_at(index, buffer.len()),
                resonance.value_at(index, buffer.len()),
                drive.value_at(index, buffer.len()),
                &mut self.left,
            )?;
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
        for index in 0..left.len() {
            let current_cutoff = cutoff.value_at(index, left.len());
            let current_resonance = resonance.value_at(index, left.len());
            let current_drive = drive.value_at(index, left.len());
            left[index] = Self::process_sample(
                self.sample_rate,
                left[index],
                current_cutoff,
                current_resonance,
                current_drive,
                &mut self.left,
            )?;
            right[index] = Self::process_sample(
                self.sample_rate,
                right[index],
                current_cutoff,
                current_resonance,
                current_drive,
                &mut self.right,
            )?;
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.left = [0.0; 4];
        self.right = [0.0; 4];
    }

    fn process_sample(
        sample_rate: f32,
        input: f32,
        cutoff: f32,
        resonance: f32,
        drive: f32,
        state: &mut [f32; 4],
    ) -> Result<f32, ProcessError> {
        if !input.is_finite()
            || !cutoff.is_finite()
            || !resonance.is_finite()
            || !drive.is_finite()
            || !state.iter().all(|value| value.is_finite())
        {
            return Err(non_finite());
        }
        let cutoff = cutoff.clamp(20.0, sample_rate * 0.45);
        let tangent = (std::f32::consts::PI * cutoff / sample_rate).tan().max(0.0);
        let g = tangent / (1.0 + tangent);
        let input_gain = 10.0_f32.powf(drive.clamp(0.0, 1.0) * 24.0 / 20.0);
        let feedback = resonance.clamp(0.0, 1.0) * 3.7;
        let driven = (input * input_gain - feedback * state[3]).tanh();
        let mut value = driven;
        for stage in state.iter_mut() {
            *stage += g * (value - *stage);
            value = *stage;
        }
        if value.is_finite() && state.iter().all(|sample| sample.is_finite()) {
            Ok(value)
        } else {
            Err(non_finite())
        }
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
    use super::*;

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
}
