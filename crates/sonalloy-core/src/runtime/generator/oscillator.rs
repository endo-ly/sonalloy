use sonalloy_dsp_sys::{DspOscillator, DspOscillatorWaveform, DspVariableOscillator};

use crate::compiler::{CompiledOscillator, CompiledOscillatorBackend, CompiledUnison};
use crate::definition::OscillatorWaveform;
use crate::process::{ProcessError, ProcessSpec, ProcessorFailureKind};

use super::super::modulation::LayerGeneratorTargetSpan;
use super::super::modulation::ValueSpan;

enum OscillatorComponentRuntime {
    Basic(DspOscillator),
    HardSync(DspVariableOscillator),
}

pub(crate) struct OscillatorRuntime {
    components: Vec<OscillatorComponentRuntime>,
    backend: CompiledOscillatorBackend,
    waveform: OscillatorWaveform,
    phase_reset: bool,
    phase: f32,
    unison: CompiledUnison,
    waveshaping: bool,
    output_mode: crate::compiler::GeneratorOutputMode,
}

impl OscillatorRuntime {
    pub(super) fn new(
        compiled: &CompiledOscillator,
        spec: ProcessSpec,
    ) -> Result<Self, ProcessError> {
        let voices = compiled.unison.voices;
        if voices == 0
            || compiled.unison.detune_distribution.len() != voices
            || compiled.unison.pan_distribution.len() != voices
            || compiled.unison.phase_distribution.len() != voices
        {
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
                CompiledOscillatorBackend::VariableShapeSync => {
                    let mut oscillator =
                        DspVariableOscillator::new().map_err(ProcessError::from_dsp_error)?;
                    oscillator
                        .prepare(spec.sample_rate, native_waveform(compiled.waveform))
                        .map_err(ProcessError::from_dsp_error)?;
                    oscillator.reset().map_err(ProcessError::from_dsp_error)?;
                    OscillatorComponentRuntime::HardSync(oscillator)
                }
            };
            components.push(component);
        }
        Ok(Self {
            components,
            backend: compiled.backend,
            waveform: compiled.waveform,
            phase_reset: compiled.phase_reset,
            phase: compiled.phase,
            unison: compiled.unison.clone(),
            waveshaping: compiled.waveshaping.is_some(),
            output_mode: compiled.output_mode,
        })
    }

    pub(super) fn start(&mut self) -> Result<(), ProcessError> {
        if self.phase_reset {
            self.reset()?;
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
        let hard_sync = self.backend == CompiledOscillatorBackend::VariableShapeSync;
        let sync_ratio = if hard_sync {
            Some(targets.sync_ratio.ok_or_else(invalid_state)?)
        } else {
            None
        };
        let detune = targets.unison_detune.unwrap_or(ValueSpan {
            start: 0.0,
            end: 0.0,
        });
        let spread = targets.unison_spread.unwrap_or(ValueSpan {
            start: 0.0,
            end: 0.0,
        });
        validate_span(detune, 0.0, 100.0)?;
        validate_span(spread, 0.0, 1.0)?;
        if let Some(ratio) = sync_ratio {
            validate_span(ratio, 1.0, 16.0)?;
        }
        let (base_start, base_end) = base_frequencies(note_number, tuning_start, tuning_end)?;
        let pulse_width = if matches!(self.waveform, OscillatorWaveform::Pulse { .. }) {
            targets.pulse_width.ok_or_else(invalid_state)?
        } else {
            ValueSpan {
                start: 0.5,
                end: 0.5,
            }
        };
        validate_span(pulse_width, 0.05, 0.95)?;

        if self.unison.voices == 1 {
            let (start_master, end_master) = component_frequency(
                base_start,
                base_end,
                self.unison.detune_distribution[0],
                detune,
                sample_rate,
                hard_sync,
            )?;
            let slave = sync_ratio
                .map(|ratio| {
                    Ok((
                        clamp_hard_sync_frequency(start_master * ratio.start, sample_rate)?,
                        clamp_hard_sync_frequency(end_master * ratio.end, sample_rate)?,
                    ))
                })
                .transpose()?;
            self.render_component(
                0,
                frames,
                start_master,
                end_master,
                slave.map(|value| value.0),
                slave.map(|value| value.1),
                pulse_width,
                mono,
            )?;
            if self.waveshaping {
                apply_waveshaping(
                    targets.waveshape.ok_or_else(invalid_state)?,
                    &mut mono[..frames],
                )?;
            }
            return ensure_finite(&mono[..frames]);
        }

        left[..frames].fill(0.0);
        right[..frames].fill(0.0);
        for index in 0..self.unison.voices {
            let (start_master, end_master) = component_frequency(
                base_start,
                base_end,
                self.unison.detune_distribution[index],
                detune,
                sample_rate,
                hard_sync,
            )?;
            let slave = sync_ratio
                .map(|ratio| {
                    Ok((
                        clamp_hard_sync_frequency(start_master * ratio.start, sample_rate)?,
                        clamp_hard_sync_frequency(end_master * ratio.end, sample_rate)?,
                    ))
                })
                .transpose()?;
            self.render_component(
                index,
                frames,
                start_master,
                end_master,
                slave.map(|value| value.0),
                slave.map(|value| value.1),
                pulse_width,
                mono,
            )?;
            mix_component(
                frames,
                mono,
                &mut left[..frames],
                &mut right[..frames],
                self.unison.pan_distribution[index],
                spread,
                self.unison.normalization,
            )?;
        }
        if self.waveshaping {
            let amount = targets.waveshape.ok_or_else(invalid_state)?;
            apply_waveshaping(amount, &mut left[..frames])?;
            apply_waveshaping(amount, &mut right[..frames])?;
        }
        ensure_finite(&left[..frames])?;
        ensure_finite(&right[..frames])
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
        };
        result.map_err(ProcessError::from_dsp_error)?;
        ensure_finite(&output[..frames])
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
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub(super) fn output_mode(&self) -> crate::compiler::GeneratorOutputMode {
        self.output_mode
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
    hard_sync: bool,
) -> Result<(f32, f32), ProcessError> {
    let start = base_start * cents_ratio(distribution * detune.start)?;
    let end = base_end * cents_ratio(distribution * detune.end)?;
    if !start.is_finite() || !end.is_finite() || start <= 0.0 || end <= 0.0 {
        return Err(ProcessError::InvalidFrequency);
    }
    if hard_sync {
        Ok((
            clamp_hard_sync_frequency(start, sample_rate)?,
            clamp_hard_sync_frequency(end, sample_rate)?,
        ))
    } else {
        #[allow(clippy::cast_possible_truncation)]
        let max_frequency = (sample_rate * 0.45) as f32;
        Ok((start.min(max_frequency), end.min(max_frequency)))
    }
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

fn clamp_hard_sync_frequency(frequency: f32, sample_rate: f64) -> Result<f32, ProcessError> {
    if !frequency.is_finite() || frequency <= 0.0 {
        return Err(ProcessError::InvalidFrequency);
    }
    #[allow(clippy::cast_possible_truncation)]
    let max_frequency = (sample_rate * 0.24) as f32;
    Ok(frequency.clamp(f32::MIN_POSITIVE, max_frequency))
}

fn mix_component(
    frames: usize,
    component: &[f32],
    left: &mut [f32],
    right: &mut [f32],
    pan_distribution: f32,
    spread: ValueSpan,
    normalization: f32,
) -> Result<(), ProcessError> {
    if !normalization.is_finite() || normalization <= 0.0 {
        return Err(invalid_state());
    }
    for index in 0..frames {
        #[allow(clippy::cast_precision_loss)]
        let position = index as f32 / frames.max(1) as f32;
        let current_spread = spread.start + (spread.end - spread.start) * position;
        let pan = (pan_distribution * current_spread).clamp(-1.0, 1.0);
        let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
        let sample = component[index];
        left[index] += sample * angle.cos() * normalization;
        right[index] += sample * angle.sin() * normalization;
    }
    Ok(())
}

fn apply_waveshaping(amount: ValueSpan, output: &mut [f32]) -> Result<(), ProcessError> {
    validate_span(amount, 0.0, 1.0)?;
    if same_value(amount.start, 0.0) && same_value(amount.end, 0.0) {
        return Ok(());
    }
    let length = output.len().max(1);
    for (index, sample) in output.iter_mut().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let position = index as f32 / length as f32;
        let current_amount = amount.start + (amount.end - amount.start) * position;
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

fn validate_span(span: ValueSpan, minimum: f32, maximum: f32) -> Result<(), ProcessError> {
    if !span.start.is_finite()
        || !span.end.is_finite()
        || !(minimum..=maximum).contains(&span.start)
        || !(minimum..=maximum).contains(&span.end)
    {
        Err(non_finite())
    } else {
        Ok(())
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
