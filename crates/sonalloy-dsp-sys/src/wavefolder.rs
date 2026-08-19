use std::ptr::NonNull;

use thiserror::Error;

use crate::ffi;

/// Errors returned by the native Wavefolder boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DspWavefolderError {
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
    #[error("wavefolder is not prepared")]
    NotPrepared,
    /// The native implementation raised an exception.
    #[error("native exception")]
    NativeException,
    /// The native implementation produced a non-finite sample.
    #[error("non-finite output")]
    NonFinite,
    /// The native result code was not recognized.
    #[error("unknown native error code {0}")]
    Unknown(i32),
}

fn result_from_code(code: i32) -> Result<(), DspWavefolderError> {
    match code {
        ffi::OK => Ok(()),
        ffi::INVALID_ARGUMENT => Err(DspWavefolderError::InvalidArgument),
        ffi::NULL_HANDLE => Err(DspWavefolderError::NullHandle),
        ffi::NOT_PREPARED => Err(DspWavefolderError::NotPrepared),
        ffi::NATIVE_EXCEPTION => Err(DspWavefolderError::NativeException),
        ffi::NON_FINITE => Err(DspWavefolderError::NonFinite),
        other => Err(DspWavefolderError::Unknown(other)),
    }
}

/// Safe owner of one opaque `DaisySP` Wavefolder.
pub struct DspWavefolder {
    handle: NonNull<ffi::DspWavefolder>,
    prepared: bool,
}

// SAFETY: The wrapper uniquely owns the native Wavefolder, the native object has no thread
// affinity, and all stateful operations require exclusive `&mut self` access.
unsafe impl Send for DspWavefolder {}

impl DspWavefolder {
    /// Create an unprepared Wavefolder.
    ///
    /// # Errors
    ///
    /// Returns [`DspWavefolderError::AllocationFailed`] when the native object cannot be
    /// allocated.
    pub fn new() -> Result<Self, DspWavefolderError> {
        let handle = unsafe { ffi::sonalloy_dsp_wavefolder_create() };
        let handle = NonNull::new(handle).ok_or(DspWavefolderError::AllocationFailed)?;
        Ok(Self {
            handle,
            prepared: false,
        })
    }

    /// Initialize the Wavefolder for the process sample rate.
    ///
    /// # Errors
    ///
    /// Returns an error when the sample rate is invalid or native preparation fails.
    pub fn prepare(&mut self, sample_rate: f64) -> Result<(), DspWavefolderError> {
        self.prepared = false;
        let code =
            unsafe { ffi::sonalloy_dsp_wavefolder_prepare(self.handle.as_ptr(), sample_rate) };
        result_from_code(code)?;
        self.prepared = true;
        Ok(())
    }

    /// Reset the Wavefolder to its prepared state.
    ///
    /// # Errors
    ///
    /// Returns [`DspWavefolderError::NotPrepared`] before preparation or a native error.
    pub fn reset(&mut self) -> Result<(), DspWavefolderError> {
        let code = unsafe { ffi::sonalloy_dsp_wavefolder_reset(self.handle.as_ptr()) };
        result_from_code(code)
    }

    /// Process a caller-owned buffer with fixed drive and dry/wet mix values.
    ///
    /// # Errors
    ///
    /// Returns an error when the Wavefolder is unprepared, a value is outside the native
    /// contract, or the native implementation fails. The buffer is cleared on failure.
    pub fn process(
        &mut self,
        drive: f32,
        mix: f32,
        buffer: &mut [f32],
    ) -> Result<(), DspWavefolderError> {
        if !self.prepared {
            buffer.fill(0.0);
            return Err(DspWavefolderError::NotPrepared);
        }
        let frames = u32::try_from(buffer.len()).map_err(|_| {
            buffer.fill(0.0);
            DspWavefolderError::InvalidArgument
        })?;
        if buffer.is_empty() {
            return Ok(());
        }
        let code = unsafe {
            ffi::sonalloy_dsp_wavefolder_process(
                self.handle.as_ptr(),
                drive,
                mix,
                buffer.as_mut_ptr(),
                frames,
            )
        };
        let result = result_from_code(code);
        if result.is_err() {
            buffer.fill(0.0);
        }
        result
    }

    /// Process a caller-owned buffer while linearly ramping drive and dry/wet mix.
    ///
    /// # Errors
    ///
    /// Returns an error when an endpoint is outside the native contract, the Wavefolder is
    /// unprepared, or native processing fails. The buffer is cleared on failure.
    pub fn process_ramp(
        &mut self,
        start_drive: f32,
        end_drive: f32,
        start_mix: f32,
        end_mix: f32,
        buffer: &mut [f32],
    ) -> Result<(), DspWavefolderError> {
        if !self.prepared {
            buffer.fill(0.0);
            return Err(DspWavefolderError::NotPrepared);
        }
        let frames = u32::try_from(buffer.len()).map_err(|_| {
            buffer.fill(0.0);
            DspWavefolderError::InvalidArgument
        })?;
        if buffer.is_empty() {
            return Ok(());
        }
        let code = unsafe {
            ffi::sonalloy_dsp_wavefolder_process_ramp(
                self.handle.as_ptr(),
                start_drive,
                end_drive,
                start_mix,
                end_mix,
                buffer.as_mut_ptr(),
                frames,
            )
        };
        let result = result_from_code(code);
        if result.is_err() {
            buffer.fill(0.0);
        }
        result
    }
}

impl Drop for DspWavefolder {
    fn drop(&mut self) {
        unsafe { ffi::sonalloy_dsp_wavefolder_destroy(self.handle.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(sonalloy_test_hooks)]
    unsafe extern "C" {
        fn sonalloy_dsp_test_arm_wavefolder_process_exception(handle: *mut ffi::DspWavefolder);
    }

    #[test]
    fn lifecycle_and_null_handle_errors_are_reported() {
        let code = unsafe { ffi::sonalloy_dsp_wavefolder_prepare(std::ptr::null_mut(), 48_000.0) };
        assert_eq!(code, ffi::NULL_HANDLE);

        let mut output = [1.0_f32; 2];
        let code = unsafe {
            ffi::sonalloy_dsp_wavefolder_process(
                std::ptr::null_mut(),
                2.0,
                1.0,
                output.as_mut_ptr(),
                u32::try_from(output.len()).expect("test buffer length fits in u32"),
            )
        };
        assert_eq!(code, ffi::NULL_HANDLE);
        unsafe { ffi::sonalloy_dsp_wavefolder_destroy(std::ptr::null_mut()) };

        let mut wavefolder = DspWavefolder::new().expect("wavefolder allocation");
        assert_eq!(
            wavefolder.process(2.0, 1.0, &mut output),
            Err(DspWavefolderError::NotPrepared)
        );
        assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));
        assert_eq!(
            wavefolder.prepare(0.0),
            Err(DspWavefolderError::InvalidArgument)
        );
        wavefolder
            .prepare(48_000.0)
            .expect("wavefolder preparation");
        wavefolder.reset().expect("wavefolder reset");
    }

    #[test]
    fn amount_zero_is_an_exact_identity() {
        let mut wavefolder = DspWavefolder::new().expect("wavefolder allocation");
        wavefolder
            .prepare(48_000.0)
            .expect("wavefolder preparation");
        let input = [0.0_f32, 0.25, -0.75, 1.5, -2.0];
        let mut output = input;
        wavefolder
            .process(1.0, 0.0, &mut output)
            .expect("identity processing");
        assert!(
            output
                .iter()
                .zip(input)
                .all(|(actual, expected)| actual.to_bits() == expected.to_bits())
        );
    }

    #[test]
    fn process_matches_wavefolder_reference_and_preserves_guards() {
        let mut wavefolder = DspWavefolder::new().expect("wavefolder allocation");
        wavefolder
            .prepare(48_000.0)
            .expect("wavefolder preparation");
        let mut guarded = [9.0_f32, 0.25, 0.75, 1.25, -0.5, -9.0];
        wavefolder
            .process(2.0, 1.0, &mut guarded[1..5])
            .expect("wavefolder processing");
        assert_eq!(guarded[0].to_bits(), 9.0_f32.to_bits());
        assert_eq!(guarded[5].to_bits(), (-9.0_f32).to_bits());
        assert_eq!(guarded[1..5], [0.5, 0.5, -0.5, -1.0]);
        assert!(guarded[1..5].iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn invalid_values_clear_output() {
        let mut wavefolder = DspWavefolder::new().expect("wavefolder allocation");
        wavefolder
            .prepare(48_000.0)
            .expect("wavefolder preparation");
        let mut output = [1.0_f32; 4];
        assert_eq!(
            wavefolder.process(0.5, 0.0, &mut output),
            Err(DspWavefolderError::InvalidArgument)
        );
        assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));
        output.fill(1.0);
        assert_eq!(
            wavefolder.process_ramp(1.0, 8.0, 0.0, 1.1, &mut output),
            Err(DspWavefolderError::InvalidArgument)
        );
        assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));
    }

    #[test]
    fn non_finite_native_output_is_reported_and_cleared() {
        let mut wavefolder = DspWavefolder::new().expect("wavefolder allocation");
        wavefolder
            .prepare(48_000.0)
            .expect("wavefolder preparation");
        let mut output = [f32::MAX, -f32::MAX];
        assert_eq!(
            wavefolder.process(8.0, 1.0, &mut output),
            Err(DspWavefolderError::NonFinite)
        );
        assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));

        output.fill(f32::MAX);
        assert_eq!(
            wavefolder.process_ramp(8.0, 8.0, 1.0, 1.0, &mut output),
            Err(DspWavefolderError::NonFinite)
        );
        assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));
    }

    #[cfg(sonalloy_test_hooks)]
    #[test]
    fn native_exception_is_caught_and_output_is_cleared() {
        let mut wavefolder = DspWavefolder::new().expect("wavefolder allocation");
        wavefolder
            .prepare(48_000.0)
            .expect("wavefolder preparation");
        unsafe { sonalloy_dsp_test_arm_wavefolder_process_exception(wavefolder.handle.as_ptr()) };
        let mut output = [1.0_f32; 4];
        assert_eq!(
            wavefolder.process(2.0, 0.5, &mut output),
            Err(DspWavefolderError::NativeException)
        );
        assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));
    }
}
