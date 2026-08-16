use crate::compiler::CompiledPhaserProcessor;
use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;

#[derive(Clone, Copy, Default)]
struct AllPassState {
    input_history: f32,
    output_history: f32,
}

pub(crate) struct PhaserRuntime {
    sample_rate: f32,
    stages: usize,
    center_hz: f32,
    sweep_octaves: f32,
    left: [AllPassState; 8],
    right: [AllPassState; 8],
    left_feedback: f32,
    right_feedback: f32,
    phase: f32,
}

impl PhaserRuntime {
    pub(crate) fn new(compiled: &CompiledPhaserProcessor) -> Self {
        Self {
            sample_rate: compiled.sample_rate,
            stages: usize::from(compiled.stages),
            center_hz: compiled.center_hz,
            sweep_octaves: compiled.sweep_octaves,
            left: [AllPassState::default(); 8],
            right: [AllPassState::default(); 8],
            left_feedback: 0.0,
            right_feedback: 0.0,
            phase: 0.0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn process(
        &mut self,
        rate_hz: ValueSpan,
        depth: ValueSpan,
        feedback: ValueSpan,
        width: ValueSpan,
        mix: ValueSpan,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            });
        }
        for index in 0..left.len() {
            let rate = rate_hz.value_at(index, left.len());
            let depth = depth.value_at(index, left.len());
            let feedback = feedback.value_at(index, left.len());
            let width = width.value_at(index, left.len());
            let mix = mix.value_at(index, left.len());
            let left_lfo = (std::f32::consts::TAU * self.phase).sin();
            let right_lfo =
                (std::f32::consts::TAU * (self.phase + 0.5 * width).rem_euclid(1.0)).sin();
            let left_frequency = self.frequency(left_lfo, depth)?;
            let right_frequency = self.frequency(right_lfo, depth)?;
            let input_left = left[index];
            let input_right = right[index];
            let wet_left = process_channel(
                &mut self.left,
                self.stages,
                self.sample_rate,
                left_frequency,
                feedback,
                input_left,
                &mut self.left_feedback,
            )?;
            let wet_right = process_channel(
                &mut self.right,
                self.stages,
                self.sample_rate,
                right_frequency,
                feedback,
                input_right,
                &mut self.right_feedback,
            )?;
            left[index] = input_left * (1.0 - mix) + wet_left * mix;
            right[index] = input_right * (1.0 - mix) + wet_right * mix;
            if !left[index].is_finite() || !right[index].is_finite() {
                return Err(ProcessError::ProcessorFailure {
                    kind: ProcessorFailureKind::NonFinite,
                });
            }
            self.phase = (self.phase + rate / self.sample_rate).rem_euclid(1.0);
        }
        Ok(())
    }

    fn frequency(&self, lfo: f32, depth: f32) -> Result<f32, ProcessError> {
        let frequency = self.center_hz * 2.0_f32.powf((self.sweep_octaves * 0.5) * depth * lfo);
        let maximum = self.sample_rate * 0.45;
        if frequency.is_finite() && frequency > 0.0 && frequency < maximum {
            Ok(frequency)
        } else {
            Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            })
        }
    }

    pub(crate) fn reset(&mut self) {
        self.left = [AllPassState::default(); 8];
        self.right = [AllPassState::default(); 8];
        self.left_feedback = 0.0;
        self.right_feedback = 0.0;
        self.phase = 0.0;
    }
}

fn process_channel(
    stages: &mut [AllPassState; 8],
    stage_count: usize,
    sample_rate: f32,
    frequency_hz: f32,
    feedback: f32,
    input: f32,
    feedback_state: &mut f32,
) -> Result<f32, ProcessError> {
    let coefficient_argument = std::f32::consts::PI * frequency_hz / sample_rate;
    let g = coefficient_argument.tan();
    let coefficient = (1.0 - g) / (1.0 + g);
    let mut value = input + *feedback_state * feedback;
    for state in stages.iter_mut().take(stage_count) {
        let output = coefficient * value + state.input_history - coefficient * state.output_history;
        state.input_history = value;
        state.output_history = output;
        value = output;
    }
    *feedback_state = value;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::NonFinite,
        })
    }
}
