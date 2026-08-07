use std::sync::Arc;

use sonalloy_dsp_sys::{
    DspOscillator, DspOscillatorWaveform, DspVariableOscillator, DspWavefolder,
};

use crate::compiler::{CompiledOscillator, CompiledOscillatorBackend, CompiledUnison};
use crate::definition::OscillatorWaveform;
use crate::generator_parameters::{
    OSCILLATOR_FEEDBACK, PHASE_DISTORTION, PULSE_WIDTH, SYNC_RATIO, UNISON_DETUNE, UNISON_SPREAD,
    WAVEFOLD, WAVESHAPE,
};
use crate::process::{ProcessError, ProcessSpec, ProcessorFailureKind};

use super::super::mix::mix_component;
use super::super::modulation::LayerGeneratorTargetSpan;
use super::super::modulation::ValueSpan;
use super::validate_generator_span;

enum OscillatorComponentRuntime {
    Basic(DspOscillator),
    HardSync(DspVariableOscillator),
    PhaseDomain(PhaseDomainOscillatorComponent),
}

struct PhaseDomainOscillatorComponent {
    phase: f32,
    previous_output: f32,
}

enum WavefolderRuntime {
    Mono(DspWavefolder),
    Stereo(DspWavefolder, DspWavefolder),
}

struct DcBlocker {
    coefficient: f32,
    previous_input: [f32; 2],
    previous_output: [f32; 2],
}

pub(crate) struct OscillatorRuntime {
    components: Vec<OscillatorComponentRuntime>,
    backend: CompiledOscillatorBackend,
    waveform: OscillatorWaveform,
    phase_reset: bool,
    phase: f32,
    unison: Arc<CompiledUnison>,
    waveshaping: Option<()>,
    phase_distortion: Option<()>,
    wavefold: Option<WavefolderRuntime>,
    oscillator_feedback: Option<()>,
    dc_blocker: Option<DcBlocker>,
}

impl OscillatorRuntime {
    pub(super) fn new(
        compiled: &CompiledOscillator,
        spec: ProcessSpec,
    ) -> Result<Self, ProcessError> {
        let voices = compiled.unison.position_distribution.len();
        if voices == 0 || compiled.unison.phase_distribution.len() != voices {
            return Err(invalid_state());
        }
        let mut components = Vec::with_capacity(voices);
        for index in 0..voices {
            let component = match compiled.backend {
                CompiledOscillatorBackend::Basic => {
                    let mut oscillator =
                        DspOscillator::new().map_err(ProcessError::from_dsp_error)?;
                    oscillator
                        .prepare(spec.sample_rate, native_waveform(compiled.waveform))
                        .map_err(ProcessError::from_dsp_error)?;
                    oscillator
                        .reset_phase(initial_phase(
                            compiled.phase,
                            compiled.unison.phase_distribution[index],
                        ))
                        .map_err(ProcessError::from_dsp_error)?;
                    OscillatorComponentRuntime::Basic(oscillator)
                }
                CompiledOscillatorBackend::VariableShapeSync { .. } => {
                    let mut oscillator =
                        DspVariableOscillator::new().map_err(ProcessError::from_dsp_error)?;
                    oscillator
                        .prepare(spec.sample_rate, native_waveform(compiled.waveform))
                        .map_err(ProcessError::from_dsp_error)?;
                    oscillator.reset().map_err(ProcessError::from_dsp_error)?;
                    OscillatorComponentRuntime::HardSync(oscillator)
                }
                CompiledOscillatorBackend::PhaseDomain => {
                    OscillatorComponentRuntime::PhaseDomain(PhaseDomainOscillatorComponent {
                        phase: initial_phase(
                            compiled.phase,
                            compiled.unison.phase_distribution[index],
                        ),
                        previous_output: 0.0,
                    })
                }
            };
            components.push(component);
        }
        let wavefold = if compiled.parameters.wavefold.is_some() {
            if voices == 1 {
                let mut wavefolder =
                    DspWavefolder::new().map_err(ProcessError::from_wavefolder_error)?;
                wavefolder
                    .prepare(spec.sample_rate)
                    .map_err(ProcessError::from_wavefolder_error)?;
                Some(WavefolderRuntime::Mono(wavefolder))
            } else {
                let mut left = DspWavefolder::new().map_err(ProcessError::from_wavefolder_error)?;
                left.prepare(spec.sample_rate)
                    .map_err(ProcessError::from_wavefolder_error)?;
                let mut right =
                    DspWavefolder::new().map_err(ProcessError::from_wavefolder_error)?;
                right
                    .prepare(spec.sample_rate)
                    .map_err(ProcessError::from_wavefolder_error)?;
                Some(WavefolderRuntime::Stereo(left, right))
            }
        } else {
            None
        };
        let dc_blocker = compiled
            .dc_blocker
            .then(|| DcBlocker::new(spec.sample_rate))
            .transpose()?;
        Ok(Self {
            components,
            backend: compiled.backend,
            waveform: compiled.waveform,
            phase_reset: compiled.phase_reset,
            phase: compiled.phase,
            unison: Arc::clone(&compiled.unison),
            waveshaping: compiled.parameters.waveshape.map(|_| ()),
            phase_distortion: compiled.parameters.phase_distortion.map(|_| ()),
            wavefold,
            oscillator_feedback: compiled.parameters.oscillator_feedback.map(|_| ()),
            dc_blocker,
        })
    }

    pub(super) fn start(&mut self) -> Result<(), ProcessError> {
        if self.phase_reset {
            self.reset()?;
        } else if self.phase_distortion.is_some()
            || self.oscillator_feedback.is_some()
            || self.wavefold.is_some()
        {
            self.reset_stage_state()?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_lines)]
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
        let LayerGeneratorTargetSpan::Oscillator {
            pulse_width,
            sync_ratio,
            waveshape,
            phase_distortion,
            wavefold,
            oscillator_feedback,
            unison_detune,
            unison_spread,
        } = targets
        else {
            return Err(invalid_state());
        };
        let sync_ratio = match self.backend {
            CompiledOscillatorBackend::Basic | CompiledOscillatorBackend::PhaseDomain => None,
            CompiledOscillatorBackend::VariableShapeSync { .. } => {
                Some(sync_ratio.ok_or_else(invalid_state)?)
            }
        };
        let phase_distortion = if self.phase_distortion.is_some() {
            Some(phase_distortion.ok_or_else(invalid_state)?)
        } else {
            None
        };
        let oscillator_feedback = if self.oscillator_feedback.is_some() {
            Some(oscillator_feedback.ok_or_else(invalid_state)?)
        } else {
            None
        };
        let wavefold = if self.wavefold.is_some() {
            Some(wavefold.ok_or_else(invalid_state)?)
        } else {
            None
        };
        if let Some(amount) = phase_distortion {
            validate_generator_span(amount, PHASE_DISTORTION)?;
        }
        if let Some(amount) = oscillator_feedback {
            validate_generator_span(amount, OSCILLATOR_FEEDBACK)?;
        }
        if let Some(amount) = wavefold {
            validate_generator_span(amount, WAVEFOLD)?;
        }
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
        if let Some(ratio) = sync_ratio {
            validate_generator_span(ratio, SYNC_RATIO)?;
        }
        let (base_start, base_end) = base_frequencies(note_number, tuning_start, tuning_end)?;
        let pulse_width = if matches!(self.waveform, OscillatorWaveform::Pulse { .. }) {
            pulse_width.ok_or_else(invalid_state)?
        } else {
            ValueSpan {
                start: 0.5,
                end: 0.5,
            }
        };
        validate_generator_span(pulse_width, PULSE_WIDTH)?;

        if self.unison.position_distribution.len() == 1 {
            let position = self
                .unison
                .position_distribution
                .first()
                .copied()
                .ok_or_else(invalid_state)?;
            let (start_master, end_master) = component_frequency(
                base_start,
                base_end,
                position,
                detune,
                sample_rate,
                self.backend,
            )?;
            let slave = sync_ratio
                .map(|ratio| {
                    Ok((
                        clamp_frequency(start_master * ratio.start, sample_rate, self.backend)?,
                        clamp_frequency(end_master * ratio.end, sample_rate, self.backend)?,
                    ))
                })
                .transpose()?;
            self.render_component_or_phase_domain(
                0,
                frames,
                start_master,
                end_master,
                slave.map(|value| value.0),
                slave.map(|value| value.1),
                pulse_width,
                phase_distortion,
                oscillator_feedback,
                sample_rate,
                mono,
            )?;
            if self.waveshaping.is_some() {
                apply_waveshaping(waveshape.ok_or_else(invalid_state)?, &mut mono[..frames])?;
            }
            if let Some(amount) = wavefold {
                self.apply_wavefold_mono(amount, &mut mono[..frames])?;
            }
            if self.dc_blocker.is_some() {
                self.apply_dc_blocker_mono(&mut mono[..frames])?;
            }
            return ensure_finite(&mono[..frames]);
        }

        left[..frames].fill(0.0);
        right[..frames].fill(0.0);
        for index in 0..self.unison.position_distribution.len() {
            let (start_master, end_master) = component_frequency(
                base_start,
                base_end,
                self.unison.position_distribution[index],
                detune,
                sample_rate,
                self.backend,
            )?;
            let slave = sync_ratio
                .map(|ratio| {
                    Ok((
                        clamp_frequency(start_master * ratio.start, sample_rate, self.backend)?,
                        clamp_frequency(end_master * ratio.end, sample_rate, self.backend)?,
                    ))
                })
                .transpose()?;
            self.render_component_or_phase_domain(
                index,
                frames,
                start_master,
                end_master,
                slave.map(|value| value.0),
                slave.map(|value| value.1),
                pulse_width,
                phase_distortion,
                oscillator_feedback,
                sample_rate,
                mono,
            )?;
            if !mix_component(
                frames,
                mono,
                &mut left[..frames],
                &mut right[..frames],
                self.unison.position_distribution[index],
                spread,
                self.unison.normalization,
            ) {
                return Err(invalid_state());
            }
        }
        if self.waveshaping.is_some() {
            let amount = waveshape.ok_or_else(invalid_state)?;
            apply_waveshaping(amount, &mut left[..frames])?;
            apply_waveshaping(amount, &mut right[..frames])?;
        }
        if let Some(amount) = wavefold {
            self.apply_wavefold_stereo(amount, &mut left[..frames], &mut right[..frames])?;
        }
        if self.dc_blocker.is_some() {
            self.apply_dc_blocker_stereo(&mut left[..frames], &mut right[..frames])?;
        }
        ensure_finite(&left[..frames])?;
        ensure_finite(&right[..frames])
    }

    #[allow(clippy::too_many_arguments)]
    fn render_component_or_phase_domain(
        &mut self,
        index: usize,
        frames: usize,
        start_master: f32,
        end_master: f32,
        start_slave: Option<f32>,
        end_slave: Option<f32>,
        pulse_width: ValueSpan,
        phase_distortion: Option<ValueSpan>,
        oscillator_feedback: Option<ValueSpan>,
        sample_rate: f64,
        output: &mut [f32],
    ) -> Result<(), ProcessError> {
        if matches!(self.backend, CompiledOscillatorBackend::PhaseDomain) {
            self.render_phase_domain_component(
                index,
                frames,
                start_master,
                end_master,
                phase_distortion,
                oscillator_feedback,
                sample_rate,
                output,
            )
        } else {
            self.render_component(
                index,
                frames,
                start_master,
                end_master,
                start_slave,
                end_slave,
                pulse_width,
                output,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_phase_domain_component(
        &mut self,
        index: usize,
        frames: usize,
        start_frequency: f32,
        end_frequency: f32,
        phase_distortion: Option<ValueSpan>,
        oscillator_feedback: Option<ValueSpan>,
        sample_rate: f64,
        output: &mut [f32],
    ) -> Result<(), ProcessError> {
        let component = self.components.get_mut(index).ok_or_else(invalid_state)?;
        let OscillatorComponentRuntime::PhaseDomain(component) = component else {
            return Err(invalid_state());
        };
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return Err(ProcessError::InvalidFrequency);
        }
        #[allow(clippy::cast_possible_truncation)]
        let sample_rate = sample_rate as f32;
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return Err(ProcessError::InvalidFrequency);
        }
        for (index, sample) in output[..frames].iter_mut().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let position = index as f32 / frames as f32;
            let frequency = start_frequency + (end_frequency - start_frequency) * position;
            if !frequency.is_finite() || frequency <= 0.0 {
                return Err(ProcessError::InvalidFrequency);
            }
            if !component.previous_output.is_finite() {
                return Err(non_finite());
            }
            let feedback_phase = oscillator_feedback.map_or(0.0, |amount| {
                (component.previous_output * amount.value_at(index, frames) * 2.5).tanh() * 0.25
            });
            if !feedback_phase.is_finite() {
                return Err(non_finite());
            }
            let read_phase = (component.phase + feedback_phase).rem_euclid(1.0);
            let phase = phase_distortion.map_or(read_phase, |amount| {
                phase_distortion_phase(read_phase, amount.value_at(index, frames))
            });
            let value = (phase * std::f32::consts::TAU).sin();
            if !value.is_finite() {
                return Err(non_finite());
            }
            *sample = value;
            component.previous_output = value;
            component.phase = (component.phase + frequency / sample_rate).rem_euclid(1.0);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn render_component(
        &mut self,
        index: usize,
        frames: usize,
        start_master: f32,
        end_master: f32,
        start_slave: Option<f32>,
        end_slave: Option<f32>,
        pulse_width: ValueSpan,
        output: &mut [f32],
    ) -> Result<(), ProcessError> {
        let component = self.components.get_mut(index).ok_or_else(invalid_state)?;
        let result = match component {
            OscillatorComponentRuntime::Basic(oscillator) => {
                if let OscillatorWaveform::Pulse { .. } = self.waveform {
                    if same_value(start_master, end_master)
                        && same_value(pulse_width.start, pulse_width.end)
                    {
                        oscillator.process_with_pulse_width(
                            start_master,
                            pulse_width.start,
                            &mut output[..frames],
                        )
                    } else {
                        oscillator.process_ramp_with_pulse_width(
                            start_master,
                            end_master,
                            pulse_width.start,
                            pulse_width.end,
                            &mut output[..frames],
                        )
                    }
                } else if same_value(start_master, end_master) {
                    oscillator.process(start_master, &mut output[..frames])
                } else {
                    oscillator.process_ramp(start_master, end_master, &mut output[..frames])
                }
            }
            OscillatorComponentRuntime::HardSync(oscillator) => {
                let (start_slave, end_slave) = (
                    start_slave.ok_or_else(invalid_state)?,
                    end_slave.ok_or_else(invalid_state)?,
                );
                if same_value(start_master, end_master)
                    && same_value(start_slave, end_slave)
                    && same_value(pulse_width.start, pulse_width.end)
                {
                    oscillator.process(
                        start_master,
                        start_slave,
                        pulse_width.start,
                        &mut output[..frames],
                    )
                } else {
                    oscillator.process_ramp(
                        start_master,
                        end_master,
                        start_slave,
                        end_slave,
                        pulse_width.start,
                        pulse_width.end,
                        &mut output[..frames],
                    )
                }
            }
            OscillatorComponentRuntime::PhaseDomain(_) => return Err(invalid_state()),
        };
        result.map_err(ProcessError::from_dsp_error)?;
        ensure_finite(&output[..frames])
    }

    fn apply_wavefold_mono(
        &mut self,
        amount: ValueSpan,
        output: &mut [f32],
    ) -> Result<(), ProcessError> {
        let Some(WavefolderRuntime::Mono(wavefolder)) = self.wavefold.as_mut() else {
            return Err(invalid_state());
        };
        apply_wavefolder_stage(amount, wavefolder, output)
    }

    fn apply_wavefold_stereo(
        &mut self,
        amount: ValueSpan,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        let Some(WavefolderRuntime::Stereo(left_wavefolder, right_wavefolder)) =
            self.wavefold.as_mut()
        else {
            return Err(invalid_state());
        };
        apply_wavefolder_stage(amount, left_wavefolder, left)?;
        apply_wavefolder_stage(amount, right_wavefolder, right)
    }

    fn apply_dc_blocker_mono(&mut self, output: &mut [f32]) -> Result<(), ProcessError> {
        let Some(dc_blocker) = self.dc_blocker.as_mut() else {
            return Err(invalid_state());
        };
        dc_blocker.process_mono(output)
    }

    fn apply_dc_blocker_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        let Some(dc_blocker) = self.dc_blocker.as_mut() else {
            return Err(invalid_state());
        };
        dc_blocker.process_stereo(left, right)
    }

    fn reset_stage_state(&mut self) -> Result<(), ProcessError> {
        for component in &mut self.components {
            if let OscillatorComponentRuntime::PhaseDomain(component) = component {
                component.previous_output = 0.0;
            }
        }
        if let Some(wavefolder) = self.wavefold.as_mut() {
            match wavefolder {
                WavefolderRuntime::Mono(wavefolder) => wavefolder
                    .reset()
                    .map_err(ProcessError::from_wavefolder_error)?,
                WavefolderRuntime::Stereo(left, right) => {
                    left.reset().map_err(ProcessError::from_wavefolder_error)?;
                    right.reset().map_err(ProcessError::from_wavefolder_error)?;
                }
            }
        }
        if let Some(dc_blocker) = self.dc_blocker.as_mut() {
            dc_blocker.reset();
        }
        Ok(())
    }

    pub(super) fn reset(&mut self) -> Result<(), ProcessError> {
        for (index, component) in self.components.iter_mut().enumerate() {
            match component {
                OscillatorComponentRuntime::Basic(oscillator) => oscillator
                    .reset_phase(initial_phase(
                        self.phase,
                        self.unison.phase_distribution[index],
                    ))
                    .map_err(ProcessError::from_dsp_error)?,
                OscillatorComponentRuntime::HardSync(oscillator) => {
                    oscillator.reset().map_err(ProcessError::from_dsp_error)?;
                }
                OscillatorComponentRuntime::PhaseDomain(component) => {
                    component.phase =
                        initial_phase(self.phase, self.unison.phase_distribution[index]);
                    component.previous_output = 0.0;
                }
            }
        }
        self.reset_stage_state()
    }
}

fn base_frequencies(
    note_number: u8,
    tuning_start: f32,
    tuning_end: f32,
) -> Result<(f32, f32), ProcessError> {
    let start = crate::compiler::midi_note_frequency(
        note_number,
        crate::compiler::cents_to_ratio(tuning_start),
    );
    let end = crate::compiler::midi_note_frequency(
        note_number,
        crate::compiler::cents_to_ratio(tuning_end),
    );
    if !start.is_finite() || !end.is_finite() || start <= 0.0 || end <= 0.0 {
        return Err(ProcessError::InvalidFrequency);
    }
    Ok((start, end))
}

fn component_frequency(
    base_start: f32,
    base_end: f32,
    distribution: f32,
    detune: ValueSpan,
    sample_rate: f64,
    backend: CompiledOscillatorBackend,
) -> Result<(f32, f32), ProcessError> {
    let start = base_start * cents_ratio(distribution * detune.start)?;
    let end = base_end * cents_ratio(distribution * detune.end)?;
    if !start.is_finite() || !end.is_finite() || start <= 0.0 || end <= 0.0 {
        return Err(ProcessError::InvalidFrequency);
    }
    let max_frequency = backend.effective_max_frequency(sample_rate);
    Ok((start.min(max_frequency), end.min(max_frequency)))
}

fn cents_ratio(cents: f32) -> Result<f32, ProcessError> {
    if !cents.is_finite() {
        return Err(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::NonFinite,
        });
    }
    let ratio = 2.0_f32.powf(cents / 1200.0);
    if ratio.is_finite() && ratio > 0.0 {
        Ok(ratio)
    } else {
        Err(ProcessError::InvalidFrequency)
    }
}

fn clamp_frequency(
    frequency: f32,
    sample_rate: f64,
    backend: CompiledOscillatorBackend,
) -> Result<f32, ProcessError> {
    if !frequency.is_finite() || frequency <= 0.0 {
        return Err(ProcessError::InvalidFrequency);
    }
    let max_frequency = backend.effective_max_frequency(sample_rate);
    Ok(frequency.clamp(f32::MIN_POSITIVE, max_frequency))
}

fn apply_waveshaping(amount: ValueSpan, output: &mut [f32]) -> Result<(), ProcessError> {
    validate_generator_span(amount, WAVESHAPE)?;
    if same_value(amount.start, 0.0) && same_value(amount.end, 0.0) {
        return Ok(());
    }
    let frames = output.len();
    for (index, sample) in output.iter_mut().enumerate() {
        let current_amount = amount.value_at(index, frames);
        if same_value(current_amount, 0.0) {
            continue;
        }
        if !sample.is_finite() {
            return Err(non_finite());
        }
        let shape = 1.0 + current_amount * 3.0;
        let denominator = shape.tanh();
        let wet = (shape * *sample).tanh() / denominator;
        let shaped = *sample + (wet - *sample) * current_amount;
        if !shaped.is_finite() {
            return Err(non_finite());
        }
        *sample = shaped;
    }
    Ok(())
}

fn phase_distortion_phase(phase: f32, amount: f32) -> f32 {
    if same_value(amount, 0.0) {
        return phase;
    }
    let breakpoint = 0.5 - amount * 0.45;
    if phase < breakpoint {
        0.5 * phase / breakpoint
    } else {
        0.5 + 0.5 * (phase - breakpoint) / (1.0 - breakpoint)
    }
}

fn apply_wavefolder_stage(
    amount: ValueSpan,
    wavefolder: &mut DspWavefolder,
    output: &mut [f32],
) -> Result<(), ProcessError> {
    let start_drive = 1.0 + amount.start * 7.0;
    let end_drive = 1.0 + amount.end * 7.0;
    if same_value(amount.start, 0.0) && same_value(amount.end, 0.0) {
        return Ok(());
    }
    if same_value(start_drive, end_drive) && same_value(amount.start, amount.end) {
        wavefolder
            .process(start_drive, amount.start, output)
            .map_err(ProcessError::from_wavefolder_error)
    } else {
        wavefolder
            .process_ramp(start_drive, end_drive, amount.start, amount.end, output)
            .map_err(ProcessError::from_wavefolder_error)
    }
}

impl DcBlocker {
    fn new(sample_rate: f64) -> Result<Self, ProcessError> {
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return Err(ProcessError::InvalidFrequency);
        }
        #[allow(clippy::cast_possible_truncation)]
        let coefficient = (-std::f64::consts::TAU * 10.0 / sample_rate).exp() as f32;
        if !coefficient.is_finite() || !(0.0..1.0).contains(&coefficient) {
            return Err(non_finite());
        }
        Ok(Self {
            coefficient,
            previous_input: [0.0; 2],
            previous_output: [0.0; 2],
        })
    }

    fn process_mono(&mut self, output: &mut [f32]) -> Result<(), ProcessError> {
        for sample in output {
            let input = *sample;
            if !input.is_finite() {
                return Err(non_finite());
            }
            let value = input - self.previous_input[0] + self.coefficient * self.previous_output[0];
            if !value.is_finite() {
                return Err(non_finite());
            }
            self.previous_input[0] = input;
            self.previous_output[0] = value;
            *sample = value;
        }
        Ok(())
    }

    fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32]) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(invalid_state());
        }
        for (channel, output) in [left, right].into_iter().enumerate() {
            for sample in output {
                let input = *sample;
                if !input.is_finite() {
                    return Err(non_finite());
                }
                let value = input - self.previous_input[channel]
                    + self.coefficient * self.previous_output[channel];
                if !value.is_finite() {
                    return Err(non_finite());
                }
                self.previous_input[channel] = input;
                self.previous_output[channel] = value;
                *sample = value;
            }
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.previous_input = [0.0; 2];
        self.previous_output = [0.0; 2];
    }
}

fn ensure_finite(samples: &[f32]) -> Result<(), ProcessError> {
    if samples.iter().all(|sample| sample.is_finite()) {
        Ok(())
    } else {
        Err(non_finite())
    }
}

fn non_finite() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::NonFinite,
    }
}

fn invalid_state() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::InvalidState,
    }
}

fn initial_phase(base: f32, offset: f32) -> f32 {
    (base + offset).rem_euclid(1.0)
}

fn native_waveform(waveform: OscillatorWaveform) -> DspOscillatorWaveform {
    match waveform {
        OscillatorWaveform::Sine => DspOscillatorWaveform::Sine,
        OscillatorWaveform::Saw => DspOscillatorWaveform::Saw,
        OscillatorWaveform::Square => DspOscillatorWaveform::Square,
        OscillatorWaveform::Triangle => DspOscillatorWaveform::Triangle,
        OscillatorWaveform::Pulse { .. } => DspOscillatorWaveform::Pulse,
    }
}

fn same_value(left: f32, right: f32) -> bool {
    left.total_cmp(&right).is_eq()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_distortion_amount_zero_is_identity() {
        for phase in [0.0, 0.1, 0.49, 0.5, 0.9, 0.999] {
            assert_eq!(
                phase_distortion_phase(phase, 0.0).to_bits(),
                phase.to_bits()
            );
        }
    }

    #[test]
    fn phase_distortion_mapping_is_continuous_at_the_breakpoint() {
        let amount = 0.75;
        let breakpoint = 0.5 - amount * 0.45;
        let left = phase_distortion_phase(breakpoint - 1.0e-6, amount);
        let right = phase_distortion_phase(breakpoint + 1.0e-6, amount);
        assert!((left - right).abs() < 1.0e-5);
        assert!((phase_distortion_phase(0.0, amount) - 0.0).abs() < f32::EPSILON);
        assert!((phase_distortion_phase(0.999_999, amount) - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn dc_blocker_reset_clears_history() {
        let mut blocker = DcBlocker::new(48_000.0).expect("valid blocker");
        let mut first = [1.0_f32, 1.0, 1.0];
        blocker.process_mono(&mut first).expect("first process");
        assert!(first[1] < 1.0);
        blocker.reset();
        let mut after_reset = [1.0_f32];
        blocker
            .process_mono(&mut after_reset)
            .expect("process after reset");
        assert_eq!(after_reset[0].to_bits(), 1.0_f32.to_bits());
    }
}
