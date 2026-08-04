use sonalloy_dsp_sys::{DspOscillator, DspOscillatorWaveform};

use crate::compiler::CompiledOscillator;
use crate::definition::OscillatorWaveform;
use crate::process::{ProcessError, ProcessSpec, ProcessorFailureKind};

use super::super::modulation::ValueSpan;

pub(crate) struct OscillatorRuntime {
    oscillator: DspOscillator,
    waveform: OscillatorWaveform,
    phase_reset: bool,
    phase: f32,
}

impl OscillatorRuntime {
    pub(super) fn new(
        compiled: &CompiledOscillator,
        spec: ProcessSpec,
    ) -> Result<Self, ProcessError> {
        let mut oscillator = DspOscillator::new().map_err(ProcessError::from_dsp_error)?;
        oscillator
            .prepare(spec.sample_rate, native_waveform(compiled.waveform))
            .map_err(ProcessError::from_dsp_error)?;
        oscillator
            .reset_phase(compiled.phase)
            .map_err(ProcessError::from_dsp_error)?;
        Ok(Self {
            oscillator,
            waveform: compiled.waveform,
            phase_reset: compiled.phase_reset,
            phase: compiled.phase,
        })
    }

    pub(super) fn start(&mut self) -> Result<(), ProcessError> {
        if self.phase_reset {
            self.reset()?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn render(
        &mut self,
        frames: usize,
        note_number: u8,
        tuning_start: f32,
        tuning_end: f32,
        sample_rate: f64,
        pulse_width: Option<ValueSpan>,
        output: &mut [f32],
    ) -> Result<(), ProcessError> {
        let mut start_frequency = crate::compiler::midi_note_frequency(
            note_number,
            crate::compiler::cents_to_ratio(tuning_start),
        );
        let mut end_frequency = crate::compiler::midi_note_frequency(
            note_number,
            crate::compiler::cents_to_ratio(tuning_end),
        );
        #[allow(clippy::cast_possible_truncation)]
        let max_frequency = (sample_rate * 0.45) as f32;
        if !start_frequency.is_finite()
            || !end_frequency.is_finite()
            || start_frequency <= 0.0
            || end_frequency <= 0.0
        {
            return Err(ProcessError::InvalidFrequency);
        }
        start_frequency = start_frequency.min(max_frequency);
        end_frequency = end_frequency.min(max_frequency);

        let result = if let OscillatorWaveform::Pulse { .. } = self.waveform {
            let pulse_width = pulse_width.ok_or(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            })?;
            let start_width = effective_pulse_width(pulse_width.start)?;
            let end_width = effective_pulse_width(pulse_width.end)?;
            if same_value(start_frequency, end_frequency) && same_value(start_width, end_width) {
                self.oscillator.process_with_pulse_width(
                    start_frequency,
                    start_width,
                    &mut output[..frames],
                )
            } else {
                self.oscillator.process_ramp_with_pulse_width(
                    start_frequency,
                    end_frequency,
                    start_width,
                    end_width,
                    &mut output[..frames],
                )
            }
        } else if same_value(start_frequency, end_frequency) {
            self.oscillator
                .process(start_frequency, &mut output[..frames])
        } else {
            self.oscillator
                .process_ramp(start_frequency, end_frequency, &mut output[..frames])
        };
        result.map_err(ProcessError::from_dsp_error)?;
        if output[..frames].iter().all(|sample| sample.is_finite()) {
            Ok(())
        } else {
            Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            })
        }
    }

    pub(super) fn reset(&mut self) -> Result<(), ProcessError> {
        self.oscillator
            .reset_phase(self.phase)
            .map_err(ProcessError::from_dsp_error)
    }
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

fn effective_pulse_width(value: f32) -> Result<f32, ProcessError> {
    if !value.is_finite() {
        return Err(ProcessError::ProcessorFailure {
            kind: ProcessorFailureKind::NonFinite,
        });
    }
    Ok(value.clamp(0.05, 0.95))
}

fn same_value(left: f32, right: f32) -> bool {
    left.total_cmp(&right).is_eq()
}
