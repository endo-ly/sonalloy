use std::ffi::CStr;
use std::ptr::NonNull;

use thiserror::Error;

use crate::ffi;

/// The waveform implementations exposed by the DSP boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DspOscillatorWaveform {
    /// A sine wave.
    Sine,
    /// A band-limited saw wave provided by `DaisySP`.
    Saw,
    /// A band-limited triangle wave provided by `DaisySP`.
    Triangle,
    /// A band-limited square wave provided by `DaisySP`.
    Square,
    /// A band-limited square wave with a dynamic pulse width.
    Pulse,
}

impl DspOscillatorWaveform {
    pub(crate) fn as_raw(self) -> i32 {
        match self {
            Self::Sine => ffi::WAVEFORM_SINE,
            Self::Saw => ffi::WAVEFORM_SAW,
            Self::Triangle => ffi::WAVEFORM_TRIANGLE,
            Self::Square => ffi::WAVEFORM_SQUARE,
            Self::Pulse => ffi::WAVEFORM_PULSE,
        }
    }
}

/// Errors returned by the native DSP boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DspError {
    /// The native object could not be allocated.
    #[error("native object allocation failed")]
    AllocationFailed,
    /// The caller supplied an invalid value.
    #[error("invalid argument")]
    InvalidArgument,
    /// A native handle was null.
    #[error("null native handle")]
    NullHandle,
    /// Processing was requested before preparation.
    #[error("oscillator is not prepared")]
    NotPrepared,
    /// The selected waveform is unavailable.
    #[error("unsupported waveform")]
    UnsupportedWaveform,
    /// The native implementation raised an exception.
    #[error("native exception")]
    NativeException,
    /// The native result code was not recognized.
    #[error("unknown native error code {0}")]
    Unknown(i32),
}

pub(crate) fn result_from_code(code: i32) -> Result<(), DspError> {
    match code {
        ffi::OK => Ok(()),
        ffi::INVALID_ARGUMENT => Err(DspError::InvalidArgument),
        ffi::NULL_HANDLE => Err(DspError::NullHandle),
        ffi::NOT_PREPARED => Err(DspError::NotPrepared),
        ffi::UNSUPPORTED_WAVEFORM => Err(DspError::UnsupportedWaveform),
        ffi::NATIVE_EXCEPTION => Err(DspError::NativeException),
        other => Err(DspError::Unknown(other)),
    }
}

/// Capabilities reported by the fixed native backend.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DspCapabilities {
    /// Whether sine generation is available.
    pub sine: bool,
    /// Whether saw generation is available.
    pub saw: bool,
    /// Whether triangle generation is available.
    pub triangle: bool,
    /// Whether square generation is available.
    pub square: bool,
    /// Whether pulse generation is available.
    pub pulse: bool,
}

/// Return the native backend version string.
#[must_use]
pub fn backend_version() -> String {
    // The wrapper returns a pointer to immutable static storage.
    unsafe {
        CStr::from_ptr(ffi::sonalloy_dsp_backend_version())
            .to_string_lossy()
            .into_owned()
    }
}

/// Return the native backend capabilities.
#[must_use]
pub fn capabilities() -> DspCapabilities {
    let bits = unsafe { ffi::sonalloy_dsp_capabilities() };
    DspCapabilities {
        sine: bits & ffi::CAPABILITY_SINE != 0,
        saw: bits & ffi::CAPABILITY_SAW != 0,
        triangle: bits & ffi::CAPABILITY_TRIANGLE != 0,
        square: bits & ffi::CAPABILITY_SQUARE != 0,
        pulse: bits & ffi::CAPABILITY_PULSE != 0,
    }
}

/// Safe owner of one opaque `DaisySP` oscillator.
pub struct DspOscillator {
    handle: NonNull<ffi::DspOscillator>,
    prepared: bool,
}

impl DspOscillator {
    /// Create an unprepared oscillator.
    ///
    /// # Errors
    ///
    /// Returns [`DspError::AllocationFailed`] when the native object cannot be allocated.
    pub fn new() -> Result<Self, DspError> {
        let handle = unsafe { ffi::sonalloy_dsp_oscillator_create() };
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
            ffi::sonalloy_dsp_oscillator_prepare(
                self.handle.as_ptr(),
                sample_rate,
                waveform.as_raw(),
            )
        };
        result_from_code(code)?;
        self.prepared = true;
        Ok(())
    }

    /// Reset the oscillator phase and waveform state to their prepared initial states.
    ///
    /// # Errors
    ///
    /// Returns [`DspError::NotPrepared`] before successful preparation, or a native error.
    pub fn reset(&mut self) -> Result<(), DspError> {
        let code = unsafe { ffi::sonalloy_dsp_oscillator_reset(self.handle.as_ptr()) };
        result_from_code(code)
    }

    /// Reset the oscillator phase and waveform state to an arbitrary normalized phase.
    ///
    /// # Errors
    ///
    /// Returns an error when the phase is outside the inclusive zero-to-one range, before
    /// preparation, or when the native implementation reports an exception.
    pub fn reset_phase(&mut self, phase: f32) -> Result<(), DspError> {
        let code = unsafe { ffi::sonalloy_dsp_oscillator_reset_phase(self.handle.as_ptr(), phase) };
        result_from_code(code)
    }

    /// Render one block into caller-owned storage.
    ///
    /// # Errors
    ///
    /// Returns [`DspError::NotPrepared`] before preparation, or a native error for an invalid
    /// frequency or buffer length.
    pub fn process(&mut self, frequency_hz: f32, output: &mut [f32]) -> Result<(), DspError> {
        if !self.prepared {
            output.fill(0.0);
            return Err(DspError::NotPrepared);
        }
        if output.is_empty() {
            return Ok(());
        }
        let Ok(frames) = u32::try_from(output.len()) else {
            output.fill(0.0);
            return Err(DspError::InvalidArgument);
        };
        let code = unsafe {
            ffi::sonalloy_dsp_oscillator_process(
                self.handle.as_ptr(),
                frequency_hz,
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

    /// Render a block with a fixed pulse width.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid frequency or pulse width, an unprepared oscillator, or
    /// a native processing failure. The output is cleared on failure.
    pub fn process_with_pulse_width(
        &mut self,
        frequency_hz: f32,
        pulse_width: f32,
        output: &mut [f32],
    ) -> Result<(), DspError> {
        if !self.prepared {
            output.fill(0.0);
            return Err(DspError::NotPrepared);
        }
        let frames = u32::try_from(output.len()).map_err(|_| {
            output.fill(0.0);
            DspError::InvalidArgument
        })?;
        if output.is_empty() {
            return Ok(());
        }
        let code = unsafe {
            ffi::sonalloy_dsp_oscillator_process_with_pulse_width(
                self.handle.as_ptr(),
                frequency_hz,
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

    /// Render a block while ramping frequency in the native oscillator.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid frequencies, an unprepared oscillator, or a native
    /// processing failure. The output is cleared on failure.
    pub fn process_ramp(
        &mut self,
        start_frequency_hz: f32,
        end_frequency_hz: f32,
        output: &mut [f32],
    ) -> Result<(), DspError> {
        if !self.prepared {
            output.fill(0.0);
            return Err(DspError::NotPrepared);
        }
        let frames = u32::try_from(output.len()).map_err(|_| {
            output.fill(0.0);
            DspError::InvalidArgument
        })?;
        if output.is_empty() {
            return Ok(());
        }
        let code = unsafe {
            ffi::sonalloy_dsp_oscillator_process_ramp(
                self.handle.as_ptr(),
                start_frequency_hz,
                end_frequency_hz,
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

    /// Render a block while ramping frequency and pulse width.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid frequencies or pulse widths, an unprepared oscillator, or a
    /// native processing failure. The output is cleared on failure.
    pub fn process_ramp_with_pulse_width(
        &mut self,
        start_frequency_hz: f32,
        end_frequency_hz: f32,
        start_pulse_width: f32,
        end_pulse_width: f32,
        output: &mut [f32],
    ) -> Result<(), DspError> {
        if !self.prepared {
            output.fill(0.0);
            return Err(DspError::NotPrepared);
        }
        let frames = u32::try_from(output.len()).map_err(|_| {
            output.fill(0.0);
            DspError::InvalidArgument
        })?;
        if output.is_empty() {
            return Ok(());
        }
        let code = unsafe {
            ffi::sonalloy_dsp_oscillator_process_ramp_with_pulse_width(
                self.handle.as_ptr(),
                start_frequency_hz,
                end_frequency_hz,
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

impl Drop for DspOscillator {
    fn drop(&mut self) {
        unsafe { ffi::sonalloy_dsp_oscillator_destroy(self.handle.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(sonalloy_test_hooks)]
    unsafe extern "C" {
        fn sonalloy_dsp_test_arm_process_exception(handle: *mut ffi::DspOscillator);
    }

    #[test]
    fn null_handle_is_reported_by_the_ffi() {
        let code = unsafe {
            ffi::sonalloy_dsp_oscillator_prepare(std::ptr::null_mut(), 48_000.0, ffi::WAVEFORM_SINE)
        };
        assert_eq!(code, ffi::NULL_HANDLE);

        let mut output = [0.0_f32; 1];
        let code = unsafe {
            ffi::sonalloy_dsp_oscillator_process(
                std::ptr::null_mut(),
                440.0,
                output.as_mut_ptr(),
                u32::try_from(output.len()).expect("test buffer length fits in u32"),
            )
        };
        assert_eq!(code, ffi::NULL_HANDLE);

        let code = unsafe { ffi::sonalloy_dsp_oscillator_reset(std::ptr::null_mut()) };
        assert_eq!(code, ffi::NULL_HANDLE);

        unsafe { ffi::sonalloy_dsp_oscillator_destroy(std::ptr::null_mut()) };
    }

    #[test]
    fn native_argument_errors_do_not_leave_stale_output() {
        let mut oscillator = DspOscillator::new().expect("oscillator allocation");
        let code = unsafe {
            ffi::sonalloy_dsp_oscillator_prepare(oscillator.handle.as_ptr(), 48_000.0, 99)
        };
        assert_eq!(code, ffi::UNSUPPORTED_WAVEFORM);

        oscillator
            .prepare(48_000.0, DspOscillatorWaveform::Sine)
            .expect("oscillator preparation");
        let code = unsafe {
            ffi::sonalloy_dsp_oscillator_process(
                oscillator.handle.as_ptr(),
                440.0,
                std::ptr::null_mut(),
                1,
            )
        };
        assert_eq!(code, ffi::INVALID_ARGUMENT);

        let mut output = [1.0_f32; 2];
        let code = unsafe {
            ffi::sonalloy_dsp_oscillator_process(
                oscillator.handle.as_ptr(),
                f32::NAN,
                output.as_mut_ptr(),
                u32::try_from(output.len()).expect("test buffer length fits in u32"),
            )
        };
        assert_eq!(code, ffi::INVALID_ARGUMENT);
        assert!(output.iter().all(|sample| (*sample).abs() < f32::EPSILON));
    }

    #[test]
    fn failed_prepare_invalidates_previous_preparation() {
        let mut oscillator = DspOscillator::new().expect("oscillator allocation");
        oscillator
            .prepare(48_000.0, DspOscillatorWaveform::Sine)
            .expect("oscillator preparation");

        let mut output = [1.0_f32; 2];
        assert!(oscillator.process(440.0, &mut output).is_ok());

        assert_eq!(
            oscillator.prepare(0.0, DspOscillatorWaveform::Sine),
            Err(DspError::InvalidArgument)
        );
        output.fill(1.0);
        assert_eq!(
            oscillator.process(440.0, &mut output),
            Err(DspError::NotPrepared)
        );
        assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));

        oscillator
            .prepare(48_000.0, DspOscillatorWaveform::Sine)
            .expect("oscillator re-preparation");
        assert!(oscillator.process(440.0, &mut output).is_ok());
    }

    #[test]
    fn native_result_codes_are_mapped() {
        assert_eq!(
            result_from_code(ffi::NATIVE_EXCEPTION),
            Err(DspError::NativeException)
        );
        assert_eq!(
            result_from_code(ffi::NOT_PREPARED),
            Err(DspError::NotPrepared)
        );
        assert_eq!(result_from_code(ffi::OK), Ok(()));
    }

    #[cfg(sonalloy_test_hooks)]
    #[test]
    fn native_exception_is_caught_and_output_is_cleared() {
        let mut oscillator = DspOscillator::new().expect("oscillator allocation");
        oscillator
            .prepare(48_000.0, DspOscillatorWaveform::Sine)
            .expect("oscillator preparation");
        unsafe { sonalloy_dsp_test_arm_process_exception(oscillator.handle.as_ptr()) };
        let mut output = [1.0_f32; 2];
        assert_eq!(
            oscillator.process(440.0, &mut output),
            Err(DspError::NativeException)
        );
        assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));
    }

    #[cfg(sonalloy_test_hooks)]
    #[test]
    fn native_ramp_exception_is_caught_and_consumes_the_hook() {
        let mut oscillator = DspOscillator::new().expect("oscillator allocation");
        oscillator
            .prepare(48_000.0, DspOscillatorWaveform::Sine)
            .expect("oscillator preparation");
        unsafe { sonalloy_dsp_test_arm_process_exception(oscillator.handle.as_ptr()) };
        let mut output = [1.0_f32; 2];
        assert_eq!(
            oscillator.process_ramp(440.0, 880.0, &mut output),
            Err(DspError::NativeException)
        );
        assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));

        oscillator
            .process(440.0, &mut output)
            .expect("exception hook is consumed after one process");
    }
}
