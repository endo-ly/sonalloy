use std::ptr::NonNull;

use crate::ffi;

use super::oscillator::{DspError, DspOscillatorWaveform, result_from_code};

/// Safe owner of one `DaisySP` variable-shape hard-sync oscillator.
pub struct DspVariableOscillator {
    handle: NonNull<ffi::DspVariableOscillator>,
    prepared: bool,
}

impl DspVariableOscillator {
    /// Create an unprepared hard-sync oscillator.
    ///
    /// # Errors
    ///
    /// Returns `DspError::AllocationFailed` when the native object cannot be allocated.
    pub fn new() -> Result<Self, DspError> {
        let handle = unsafe { ffi::sonalloy_dsp_variable_oscillator_create() };
        let handle = NonNull::new(handle).ok_or(DspError::AllocationFailed)?;
        Ok(Self {
            handle,
            prepared: false,
        })
    }

    /// Initialize the oscillator for a sample rate and waveform.
    ///
    /// # Errors
    ///
    /// Returns an error when the sample rate or waveform is invalid, or when the native
    /// implementation reports an exception.
    pub fn prepare(
        &mut self,
        sample_rate: f64,
        waveform: DspOscillatorWaveform,
    ) -> Result<(), DspError> {
        self.prepared = false;
        let code = unsafe {
            ffi::sonalloy_dsp_variable_oscillator_prepare(
                self.handle.as_ptr(),
                sample_rate,
                waveform.as_raw(),
            )
        };
        result_from_code(code)?;
        self.prepared = true;
        Ok(())
    }

    /// Reset the hard-sync oscillator to its deterministic initial phase.
    ///
    /// # Errors
    ///
    /// Returns `DspError::NotPrepared` before successful preparation, or a native error.
    pub fn reset(&mut self) -> Result<(), DspError> {
        let code = unsafe { ffi::sonalloy_dsp_variable_oscillator_reset(self.handle.as_ptr()) };
        result_from_code(code)
    }

    /// Render a block with fixed master and slave frequencies.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid frequencies or pulse width, an unprepared oscillator, or a
    /// native processing failure. The output is cleared on failure.
    pub fn process(
        &mut self,
        master_frequency_hz: f32,
        slave_frequency_hz: f32,
        pulse_width: f32,
        output: &mut [f32],
    ) -> Result<(), DspError> {
        if !self.prepared {
            output.fill(0.0);
            return Err(DspError::NotPrepared);
        }
        let frames = frames_for(output)?;
        if frames == 0 {
            return Ok(());
        }
        let code = unsafe {
            ffi::sonalloy_dsp_variable_oscillator_process(
                self.handle.as_ptr(),
                master_frequency_hz,
                slave_frequency_hz,
                pulse_width,
                output.as_mut_ptr(),
                frames,
            )
        };
        let result = result_from_code(code);
        if result.is_err() {
            output.fill(0.0);
        }
        result
    }

    /// Render a block while ramping master frequency, slave frequency, and pulse width.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid frequencies or pulse widths, an unprepared oscillator, or a
    /// native processing failure. The output is cleared on failure.
    #[allow(clippy::too_many_arguments)]
    pub fn process_ramp(
        &mut self,
        start_master_frequency_hz: f32,
        end_master_frequency_hz: f32,
        start_slave_frequency_hz: f32,
        end_slave_frequency_hz: f32,
        start_pulse_width: f32,
        end_pulse_width: f32,
        output: &mut [f32],
    ) -> Result<(), DspError> {
        if !self.prepared {
            output.fill(0.0);
            return Err(DspError::NotPrepared);
        }
        let frames = frames_for(output)?;
        if frames == 0 {
            return Ok(());
        }
        let code = unsafe {
            ffi::sonalloy_dsp_variable_oscillator_process_ramp(
                self.handle.as_ptr(),
                start_master_frequency_hz,
                end_master_frequency_hz,
                start_slave_frequency_hz,
                end_slave_frequency_hz,
                start_pulse_width,
                end_pulse_width,
                output.as_mut_ptr(),
                frames,
            )
        };
        let result = result_from_code(code);
        if result.is_err() {
            output.fill(0.0);
        }
        result
    }
}

fn frames_for(output: &mut [f32]) -> Result<u32, DspError> {
    u32::try_from(output.len()).map_err(|_| {
        output.fill(0.0);
        DspError::InvalidArgument
    })
}

impl Drop for DspVariableOscillator {
    fn drop(&mut self) {
        unsafe { ffi::sonalloy_dsp_variable_oscillator_destroy(self.handle.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(sonalloy_test_hooks)]
    unsafe extern "C" {
        fn sonalloy_dsp_test_arm_variable_process_exception(
            handle: *mut ffi::DspVariableOscillator,
        );
    }

    #[test]
    fn sine_is_not_a_hard_sync_waveform() {
        let mut oscillator = DspVariableOscillator::new().expect("oscillator allocation");
        assert_eq!(
            oscillator.prepare(48_000.0, DspOscillatorWaveform::Sine),
            Err(DspError::UnsupportedWaveform)
        );
    }

    #[test]
    fn hard_sync_waveforms_render_finite_audio() {
        for waveform in [
            DspOscillatorWaveform::Saw,
            DspOscillatorWaveform::Triangle,
            DspOscillatorWaveform::Square,
            DspOscillatorWaveform::Pulse,
        ] {
            let mut oscillator = DspVariableOscillator::new().expect("oscillator allocation");
            oscillator
                .prepare(48_000.0, waveform)
                .expect("hard sync preparation");
            let mut output = [0.0_f32; 4_096];
            oscillator
                .process_ramp(220.0, 880.0, 440.0, 5_280.0, 0.25, 0.75, &mut output)
                .expect("hard sync process");
            assert!(output.iter().all(|sample| sample.is_finite()));
            assert!(output.iter().any(|sample| sample.abs() > 0.1));
        }
    }

    #[test]
    fn hard_sync_accepts_supported_ratios_and_sample_rates() {
        for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
            for ratio in [1.0, 2.0, 8.0, 16.0] {
                let mut oscillator = DspVariableOscillator::new().expect("oscillator allocation");
                oscillator
                    .prepare(sample_rate, DspOscillatorWaveform::Saw)
                    .expect("hard sync preparation");
                let mut output = [0.0_f32; 256];
                oscillator
                    .process(220.0, 220.0 * ratio, 0.5, &mut output)
                    .expect("supported ratio and sample rate");
                assert!(output.iter().all(|sample| sample.is_finite()));
            }
        }
    }

    #[test]
    fn reset_is_deterministic() {
        let mut oscillator = DspVariableOscillator::new().expect("oscillator allocation");
        oscillator
            .prepare(48_000.0, DspOscillatorWaveform::Saw)
            .expect("hard sync preparation");
        let mut first = [0.0_f32; 128];
        oscillator
            .process(220.0, 660.0, 0.5, &mut first)
            .expect("initial process");
        oscillator.reset().expect("hard sync reset");
        let mut second = [0.0_f32; 128];
        oscillator
            .process(220.0, 660.0, 0.5, &mut second)
            .expect("repeated process");
        assert_eq!(
            first
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>(),
            second
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>()
        );
    }

    #[cfg(sonalloy_test_hooks)]
    #[test]
    fn native_exception_clears_output() {
        let mut oscillator = DspVariableOscillator::new().expect("oscillator allocation");
        oscillator
            .prepare(48_000.0, DspOscillatorWaveform::Saw)
            .expect("hard sync preparation");
        unsafe { sonalloy_dsp_test_arm_variable_process_exception(oscillator.handle.as_ptr()) };
        let mut output = [1.0_f32; 2];
        assert_eq!(
            oscillator.process(220.0, 660.0, 0.5, &mut output),
            Err(DspError::NativeException)
        );
        assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));
    }
}
