use std::sync::Arc;

use crate::compiler::{CompiledOperatorModulation, CompiledOperatorTopology, CompiledUnison};
use crate::definition::{
    OPERATOR_AM_RING_AMOUNT_MAX, OPERATOR_AM_RING_AMOUNT_MIN, OPERATOR_DETUNE_MAX,
    OPERATOR_DETUNE_MIN, OPERATOR_FEEDBACK_MAX, OPERATOR_FEEDBACK_MIN, OPERATOR_LEVEL_MAX,
    OPERATOR_LEVEL_MIN, OPERATOR_PHASE_FREQUENCY_AMOUNT_MAX, OPERATOR_PHASE_FREQUENCY_AMOUNT_MIN,
    OPERATOR_RATIO_MAX, OPERATOR_RATIO_MIN, OperatorModulationMode,
};
use crate::parameter::generator::{UNISON_DETUNE, UNISON_SPREAD};
use crate::process::{ProcessError, ProcessSpec, ProcessorFailureKind};

use super::super::adsr::AdsrRuntime;
use super::super::mix::{constant_power_pan, mix_component_sample};
use super::super::modulation::{LayerGeneratorTargetSpan, OperatorTargetSpan, ValueSpan};
use super::{
    base_frequencies, ensure_finite, initial_phase, invalid_state, non_finite,
    validate_generator_span,
};

struct OperatorComponentRuntime {
    phases: [f32; 4],
    previous_outputs: [f32; 4],
}

pub(crate) struct OperatorModulationRuntime {
    components: Vec<OperatorComponentRuntime>,
    envelopes: [AdsrRuntime; 4],
    initial_phases: [f32; 4],
    mode: OperatorModulationMode,
    topology: CompiledOperatorTopology,
    phase_reset: bool,
    unison: Arc<CompiledUnison>,
    effective_max_frequency: f32,
}

impl OperatorModulationRuntime {
    pub(super) fn new(
        compiled: &CompiledOperatorModulation,
        _spec: ProcessSpec,
    ) -> Result<Self, ProcessError> {
        let voices = compiled.unison.position_distribution.len();
        if !(1..=4).contains(&voices)
            || compiled.unison.phase_distribution.len() != voices
            || !compiled.effective_max_frequency.is_finite()
            || compiled.effective_max_frequency <= 0.0
        {
            return Err(invalid_state());
        }
        let initial_phases = std::array::from_fn(|index| compiled.operators[index].phase);
        let mut components = Vec::with_capacity(voices);
        for offset in compiled.unison.phase_distribution.iter().copied() {
            components.push(OperatorComponentRuntime {
                phases: std::array::from_fn(|index| initial_phase(initial_phases[index], offset)),
                previous_outputs: [0.0; 4],
            });
        }
        let envelopes =
            std::array::from_fn(|index| AdsrRuntime::new(compiled.operators[index].envelope));
        Ok(Self {
            components,
            envelopes,
            initial_phases,
            mode: compiled.mode,
            topology: compiled.topology,
            phase_reset: compiled.phase_reset,
            unison: Arc::clone(&compiled.unison),
            effective_max_frequency: compiled.effective_max_frequency,
        })
    }

    pub(super) fn start(&mut self) {
        if self.phase_reset {
            self.reset_phases();
        }
        for component in &mut self.components {
            component.previous_outputs = [0.0; 4];
        }
        for envelope in &mut self.envelopes {
            envelope.note_on();
        }
    }

    pub(super) fn note_off(&mut self) {
        for envelope in &mut self.envelopes {
            envelope.note_off();
        }
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub(super) fn render(
        &mut self,
        frames: usize,
        note_number: u8,
        tuning_start: f32,
        tuning_end: f32,
        sample_rate: f64,
        targets: LayerGeneratorTargetSpan,
        mono: &mut [f32],
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if frames == 0 {
            return Ok(());
        }
        let LayerGeneratorTargetSpan::OperatorModulation {
            operators,
            unison_detune,
            unison_spread,
        } = targets
        else {
            return Err(invalid_state());
        };
        if mono.len() < frames
            || left.len() < frames
            || right.len() < frames
            || !sample_rate.is_finite()
            || sample_rate <= 0.0
        {
            return Err(invalid_state());
        }
        validate_targets(&operators, self.mode)?;
        let detune = unison_detune.unwrap_or(ValueSpan {
            start: 0.0,
            end: 0.0,
        });
        let spread = unison_spread.unwrap_or(ValueSpan {
            start: 0.0,
            end: 0.0,
        });
        validate_generator_span(detune, UNISON_DETUNE)?;
        validate_generator_span(spread, UNISON_SPREAD)?;
        let (base_start, base_end) = base_frequencies(note_number, tuning_start, tuning_end)?;
        let topology = self.topology;
        let mode = self.mode;
        let normalization = self.unison.normalization;
        let distribution = self.unison.position_distribution.as_ref();

        if self.components.len() == 1 {
            for (frame, output) in mono.iter_mut().take(frames).enumerate() {
                let envelopes = std::array::from_fn(|index| self.envelopes[index].next_sample());
                let component = self.components.first_mut().ok_or_else(invalid_state)?;
                *output = render_sample(
                    frame,
                    frames,
                    base_start,
                    base_end,
                    distribution[0],
                    detune,
                    sample_rate,
                    self.effective_max_frequency,
                    mode,
                    topology,
                    &operators,
                    envelopes,
                    component,
                )?;
            }
            return ensure_finite(&mono[..frames]);
        }

        left[..frames].fill(0.0);
        right[..frames].fill(0.0);
        for frame in 0..frames {
            let envelopes = std::array::from_fn(|index| self.envelopes[index].next_sample());
            for (component_index, component) in self.components.iter_mut().enumerate() {
                let component_distribution = *distribution
                    .get(component_index)
                    .ok_or_else(invalid_state)?;
                let sample = render_sample(
                    frame,
                    frames,
                    base_start,
                    base_end,
                    component_distribution,
                    detune,
                    sample_rate,
                    self.effective_max_frequency,
                    mode,
                    topology,
                    &operators,
                    envelopes,
                    component,
                )?;
                let current_spread = spread.value_at(frame, frames);
                let (left_gain, right_gain) =
                    constant_power_pan(component_distribution * current_spread);
                if !mix_component_sample(
                    frame,
                    frames,
                    sample,
                    &mut left[..frames],
                    &mut right[..frames],
                    ValueSpan {
                        start: left_gain,
                        end: left_gain,
                    },
                    ValueSpan {
                        start: right_gain,
                        end: right_gain,
                    },
                    normalization,
                ) {
                    return Err(invalid_state());
                }
            }
        }
        ensure_finite(&left[..frames])?;
        ensure_finite(&right[..frames])
    }

    pub(super) fn reset(&mut self) {
        self.reset_phases();
        for component in &mut self.components {
            component.previous_outputs = [0.0; 4];
        }
        for envelope in &mut self.envelopes {
            envelope.reset();
        }
    }

    fn reset_phases(&mut self) {
        for (component, offset) in self
            .components
            .iter_mut()
            .zip(self.unison.phase_distribution.iter().copied())
        {
            component.phases =
                std::array::from_fn(|index| initial_phase(self.initial_phases[index], offset));
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn render_sample(
    frame: usize,
    frames: usize,
    base_start: f32,
    base_end: f32,
    distribution: f32,
    unison_detune: ValueSpan,
    sample_rate: f64,
    max_frequency: f32,
    mode: OperatorModulationMode,
    topology: CompiledOperatorTopology,
    targets: &[OperatorTargetSpan; 4],
    envelopes: [f32; 4],
    component: &mut OperatorComponentRuntime,
) -> Result<f32, ProcessError> {
    let base_frequency = ValueSpan {
        start: base_start,
        end: base_end,
    }
    .value_at(frame, frames);
    let current_detune = unison_detune.value_at(frame, frames);
    let mut current_outputs = [0.0_f32; 4];
    for operator_index in topology.evaluation_order {
        let index = usize::from(operator_index);
        let target = targets.get(index).ok_or_else(invalid_state)?;
        let ratio = target.ratio.value_at(frame, frames);
        let detune = target.detune.value_at(frame, frames);
        let frequency = operator_frequency(
            base_frequency,
            ratio,
            detune,
            distribution * current_detune,
            max_frequency,
        )?;
        let incoming = topology.incoming_masks[index];
        let feedback = target
            .feedback
            .map_or(0.0, |value| value.value_at(frame, frames));
        let signal = match mode {
            OperatorModulationMode::Phase => {
                let modulation = incoming_modulation(
                    frame,
                    frames,
                    topology.evaluation_order,
                    incoming,
                    targets,
                    &current_outputs,
                )?;
                let phase_offset = modulation * 0.5;
                let feedback_offset = feedback_offset(component.previous_outputs[index], feedback)?;
                let read_phase = component.phases[index] + phase_offset + feedback_offset;
                let signal = (std::f32::consts::TAU * read_phase).sin() * envelopes[index];
                advance_phase(&mut component.phases[index], frequency, sample_rate)?;
                signal
            }
            OperatorModulationMode::Frequency => {
                let modulation = incoming_modulation(
                    frame,
                    frames,
                    topology.evaluation_order,
                    incoming,
                    targets,
                    &current_outputs,
                )?;
                let feedback_offset = feedback_offset(component.previous_outputs[index], feedback)?;
                let instantaneous = frequency * (1.0 + modulation + feedback_offset);
                let instantaneous = clamp_frequency(instantaneous, max_frequency)?;
                let signal =
                    (std::f32::consts::TAU * component.phases[index]).sin() * envelopes[index];
                advance_phase(&mut component.phases[index], instantaneous, sample_rate)?;
                signal
            }
            OperatorModulationMode::Amplitude => {
                let mut multiplier = 1.0;
                for operator_index in topology.evaluation_order {
                    let modulator = usize::from(operator_index);
                    if incoming & (1_u8 << modulator) != 0 {
                        let amount = targets[modulator]
                            .modulation_amount
                            .map_or(0.0, |value| value.value_at(frame, frames));
                        multiplier *= 1.0 + current_outputs[modulator] * amount;
                    }
                }
                if !multiplier.is_finite() {
                    return Err(non_finite());
                }
                let multiplier = multiplier.clamp(0.0, 4.0);
                let signal = (std::f32::consts::TAU * component.phases[index]).sin()
                    * envelopes[index]
                    * multiplier;
                advance_phase(&mut component.phases[index], frequency, sample_rate)?;
                signal
            }
            OperatorModulationMode::Ring => {
                let mut signal =
                    (std::f32::consts::TAU * component.phases[index]).sin() * envelopes[index];
                for operator_index in topology.evaluation_order {
                    let modulator = usize::from(operator_index);
                    if incoming & (1_u8 << modulator) != 0 {
                        let amount = targets[modulator]
                            .modulation_amount
                            .map_or(0.0, |value| value.value_at(frame, frames));
                        let product = signal * current_outputs[modulator];
                        signal += (product - signal) * amount;
                    }
                }
                advance_phase(&mut component.phases[index], frequency, sample_rate)?;
                signal
            }
        };
        if !signal.is_finite() {
            return Err(non_finite());
        }
        current_outputs[index] = signal;
    }

    let mut carrier_sum = 0.0;
    for index in 0..4 {
        if topology.carrier_mask & (1_u8 << index) == 0 {
            continue;
        }
        let level = targets[index]
            .level
            .ok_or_else(invalid_state)?
            .value_at(frame, frames);
        carrier_sum += current_outputs[index] * level;
    }
    component.previous_outputs = current_outputs;
    let output = carrier_sum * topology.carrier_normalization;
    if output.is_finite() {
        Ok(output)
    } else {
        Err(non_finite())
    }
}

#[inline]
fn incoming_modulation(
    frame: usize,
    frames: usize,
    evaluation_order: [u8; 4],
    incoming: u8,
    targets: &[OperatorTargetSpan; 4],
    current_outputs: &[f32; 4],
) -> Result<f32, ProcessError> {
    let mut modulation = 0.0_f32;
    for operator_index in evaluation_order {
        let modulator = usize::from(operator_index);
        if incoming & (1_u8 << modulator) != 0 {
            let amount = targets[modulator]
                .modulation_amount
                .map_or(0.0, |value| value.value_at(frame, frames));
            modulation += current_outputs[modulator] * amount;
        }
    }
    if modulation.is_finite() {
        Ok(modulation)
    } else {
        Err(non_finite())
    }
}

fn validate_targets(
    targets: &[OperatorTargetSpan; 4],
    mode: OperatorModulationMode,
) -> Result<(), ProcessError> {
    for target in targets {
        validate_span(target.ratio, OPERATOR_RATIO_MIN, OPERATOR_RATIO_MAX)?;
        validate_span(target.detune, OPERATOR_DETUNE_MIN, OPERATOR_DETUNE_MAX)?;
        if let Some(level) = target.level {
            validate_span(level, OPERATOR_LEVEL_MIN, OPERATOR_LEVEL_MAX)?;
        }
        if let Some(amount) = target.modulation_amount {
            let (min, max) = match mode {
                OperatorModulationMode::Phase | OperatorModulationMode::Frequency => (
                    OPERATOR_PHASE_FREQUENCY_AMOUNT_MIN,
                    OPERATOR_PHASE_FREQUENCY_AMOUNT_MAX,
                ),
                OperatorModulationMode::Amplitude | OperatorModulationMode::Ring => {
                    (OPERATOR_AM_RING_AMOUNT_MIN, OPERATOR_AM_RING_AMOUNT_MAX)
                }
            };
            validate_span(amount, min, max)?;
        }
        if let Some(feedback) = target.feedback {
            validate_span(feedback, OPERATOR_FEEDBACK_MIN, OPERATOR_FEEDBACK_MAX)?;
        }
    }
    Ok(())
}

fn validate_span(span: ValueSpan, min: f32, max: f32) -> Result<(), ProcessError> {
    if span.start.is_finite()
        && span.end.is_finite()
        && (min..=max).contains(&span.start)
        && (min..=max).contains(&span.end)
    {
        Ok(())
    } else {
        Err(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::InvalidInput,
        })
    }
}

fn operator_frequency(
    base_frequency: f32,
    ratio: f32,
    detune_cents: f32,
    unison_detune_cents: f32,
    max_frequency: f32,
) -> Result<f32, ProcessError> {
    if !base_frequency.is_finite()
        || !ratio.is_finite()
        || ratio <= 0.0
        || !detune_cents.is_finite()
        || !unison_detune_cents.is_finite()
        || !max_frequency.is_finite()
        || max_frequency <= 0.0
    {
        return Err(ProcessError::InvalidFrequency);
    }
    let detune_ratio = crate::compiler::cents_to_ratio(detune_cents + unison_detune_cents);
    let frequency = base_frequency * ratio * detune_ratio;
    if !frequency.is_finite() || frequency <= 0.0 {
        return Err(ProcessError::InvalidFrequency);
    }
    Ok(frequency.min(max_frequency))
}

fn clamp_frequency(frequency: f32, max_frequency: f32) -> Result<f32, ProcessError> {
    if !frequency.is_finite() || !max_frequency.is_finite() || max_frequency <= 0.0 {
        return Err(ProcessError::InvalidFrequency);
    }
    Ok(frequency.clamp(-max_frequency, max_frequency))
}

fn advance_phase(phase: &mut f32, frequency: f32, sample_rate: f64) -> Result<(), ProcessError> {
    if !frequency.is_finite() || !sample_rate.is_finite() || sample_rate <= 0.0 {
        return Err(ProcessError::InvalidFrequency);
    }
    #[allow(clippy::cast_possible_truncation)]
    let increment = (f64::from(frequency) / sample_rate) as f32;
    *phase = (*phase + increment).rem_euclid(1.0);
    if phase.is_finite() {
        Ok(())
    } else {
        Err(non_finite())
    }
}

fn feedback_offset(previous_output: f32, amount: f32) -> Result<f32, ProcessError> {
    if !previous_output.is_finite() || !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
        return Err(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::InvalidInput,
        });
    }
    let offset = (previous_output * amount * 2.5).tanh() * 0.25;
    if offset.is_finite() {
        Ok(offset)
    } else {
        Err(non_finite())
    }
}

#[cfg(test)]
mod tests {
    use super::operator_frequency;

    #[test]
    fn operator_frequency_applies_ratio_and_detune_before_clamping() {
        let frequency = operator_frequency(440.0, 2.0, 1_200.0, 0.0, 10_000.0)
            .expect("valid operator frequency");
        assert!((frequency - 1_760.0).abs() < 1.0e-4);

        let clamped = operator_frequency(440.0, 32.0, 0.0, 0.0, 5_000.0)
            .expect("valid clamped operator frequency");
        assert!((clamped - 5_000.0).abs() < 1.0e-4);
    }
}
