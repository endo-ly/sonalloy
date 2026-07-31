use std::ptr::NonNull;

use thiserror::Error;

use crate::ffi;

fn result_from_code(code: i32) -> Result<(), DspFilterError> {
    match code {
        ffi::OK => Ok(()),
        ffi::INVALID_ARGUMENT => Err(DspFilterError::InvalidArgument),
        ffi::NULL_HANDLE => Err(DspFilterError::NullHandle),
        ffi::NOT_PREPARED => Err(DspFilterError::NotPrepared),
        ffi::NATIVE_EXCEPTION => Err(DspFilterError::NativeException),
        other => Err(DspFilterError::Unknown(other)),
    }
}

/// Errors returned by the native low-pass filter boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DspFilterError {
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
    #[error("filter is not prepared")]
    NotPrepared,
    /// The native implementation raised an exception.
    #[error("native exception")]
    NativeException,
    /// The native result code was not recognized.
    #[error("unknown native error code {0}")]
    Unknown(i32),
}

/// Safe owner of one opaque `DaisySP` state-variable low-pass filter.
pub struct DspFilter {
    handle: NonNull<ffi::DspFilter>,
    prepared: bool,
}

impl DspFilter {
    /// Create an unprepared filter.
    ///
    /// # Errors
    ///
    /// Returns [`DspFilterError::AllocationFailed`] when the native object cannot be allocated.
    pub fn new() -> Result<Self, DspFilterError> {
        let handle = unsafe { ffi::sonalloy_dsp_filter_create() };
        let handle = NonNull::new(handle).ok_or(DspFilterError::AllocationFailed)?;
        Ok(Self {
            handle,
            prepared: false,
        })
    }

    /// Initialize the filter for a sample rate.
    ///
    /// # Errors
    ///
    /// Returns an error when the sample rate is invalid or the native implementation fails.
    pub fn prepare(&mut self, sample_rate: f64) -> Result<(), DspFilterError> {
        self.prepared = false;
        let code = unsafe { ffi::sonalloy_dsp_filter_prepare(self.handle.as_ptr(), sample_rate) };
        result_from_code(code)?;
        self.prepared = true;
        Ok(())
    }

    /// Reset the filter state to its prepared initial state.
    ///
    /// # Errors
    ///
    /// Returns an error before preparation or when the native implementation fails.
    pub fn reset(&mut self) -> Result<(), DspFilterError> {
        let code = unsafe { ffi::sonalloy_dsp_filter_reset(self.handle.as_ptr()) };
        result_from_code(code)
    }

    /// Process an input buffer in place.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid parameters, an unprepared filter, or a native failure. On
    /// error the supplied buffer is cleared by the native boundary.
    pub fn process(
        &mut self,
        cutoff_hz: f32,
        resonance: f32,
        buffer: &mut [f32],
    ) -> Result<(), DspFilterError> {
        if !self.prepared {
            buffer.fill(0.0);
            return Err(DspFilterError::NotPrepared);
        }
        let frames = u32::try_from(buffer.len()).map_err(|_| {
            buffer.fill(0.0);
            DspFilterError::InvalidArgument
        })?;
        if buffer.is_empty() {
            return Ok(());
        }
        let code = unsafe {
            ffi::sonalloy_dsp_filter_process(
                self.handle.as_ptr(),
                cutoff_hz,
                resonance,
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

impl Drop for DspFilter {
    fn drop(&mut self) {
        unsafe { ffi::sonalloy_dsp_filter_destroy(self.handle.as_ptr()) };
    }
}
