use std::marker::PhantomData;
use std::ptr::NonNull;

use thiserror::Error;

use crate::ffi;

/// Errors returned by the native modal resonator boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DspModalResonatorError {
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
    #[error("modal resonator is not prepared")]
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

fn result_from_code(code: i32) -> Result<(), DspModalResonatorError> {
    match code {
        ffi::OK => Ok(()),
        ffi::INVALID_ARGUMENT => Err(DspModalResonatorError::InvalidArgument),
        ffi::NULL_HANDLE => Err(DspModalResonatorError::NullHandle),
        ffi::NOT_PREPARED => Err(DspModalResonatorError::NotPrepared),
        ffi::NATIVE_EXCEPTION => Err(DspModalResonatorError::NativeException),
        ffi::NON_FINITE => Err(DspModalResonatorError::NonFinite),
        other => Err(DspModalResonatorError::Unknown(other)),
    }
}

/// Safe owner of one opaque `DaisySP` modal resonator.
pub struct DspModalResonator {
    handle: NonNull<ffi::DspModalResonator>,
    prepared: bool,
    not_send_or_sync: PhantomData<*mut ()>,
}

impl DspModalResonator {
    /// Create an unprepared modal resonator.
    ///
    /// # Errors
    ///
    /// Returns `DspModalResonatorError::AllocationFailed` when the native object cannot be
    /// allocated.
    pub fn new() -> Result<Self, DspModalResonatorError> {
        let handle = unsafe { ffi::sonalloy_dsp_modal_resonator_create() };
        let handle = NonNull::new(handle).ok_or(DspModalResonatorError::AllocationFailed)?;
        Ok(Self {
            handle,
            prepared: false,
            not_send_or_sync: PhantomData,
        })
    }

    /// Initialize the resonator for a sample rate and fixed mode count.
    ///
    /// # Errors
    ///
    /// Returns an error when the sample rate, mode count, or native preparation is invalid.
    pub fn prepare(
        &mut self,
        sample_rate: f64,
        mode_count: u8,
    ) -> Result<(), DspModalResonatorError> {
        self.prepared = false;
        let code = unsafe {
            ffi::sonalloy_dsp_modal_resonator_prepare(
                self.handle.as_ptr(),
                sample_rate,
                i32::from(mode_count),
            )
        };
        result_from_code(code)?;
        self.prepared = true;
        Ok(())
    }

    /// Reset the resonator state without reallocating its native object.
    ///
    /// # Errors
    ///
    /// Returns an error before preparation or when the native implementation fails.
    pub fn reset(&mut self) -> Result<(), DspModalResonatorError> {
        let code = unsafe { ffi::sonalloy_dsp_modal_resonator_reset(self.handle.as_ptr()) };
        result_from_code(code)
    }

    /// Process an input buffer in place while ramping the modal parameters.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid endpoints, an unprepared resonator, or a native failure.
    /// The buffer is cleared on failure.
    #[allow(clippy::too_many_arguments)]
    pub fn process_ramp(
        &mut self,
        start_frequency_hz: f32,
        end_frequency_hz: f32,
        start_structure: f32,
        end_structure: f32,
        start_brightness: f32,
        end_brightness: f32,
        start_decay: f32,
        end_decay: f32,
        buffer: &mut [f32],
    ) -> Result<(), DspModalResonatorError> {
        if !self.prepared {
            buffer.fill(0.0);
            return Err(DspModalResonatorError::NotPrepared);
        }
        let frames = u32::try_from(buffer.len()).map_err(|_| {
            buffer.fill(0.0);
            DspModalResonatorError::InvalidArgument
        })?;
        if buffer.is_empty() {
            return Ok(());
        }
        let code = unsafe {
            ffi::sonalloy_dsp_modal_resonator_process_ramp(
                self.handle.as_ptr(),
                start_frequency_hz,
                end_frequency_hz,
                start_structure,
                end_structure,
                start_brightness,
                end_brightness,
                start_decay,
                end_decay,
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

impl Drop for DspModalResonator {
    fn drop(&mut self) {
        unsafe { ffi::sonalloy_dsp_modal_resonator_destroy(self.handle.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(sonalloy_test_hooks)]
    unsafe extern "C" {
        fn sonalloy_dsp_test_arm_modal_process_exception(handle: *mut ffi::DspModalResonator);
    }

    #[test]
    fn lifecycle_and_mode_contract_are_enforced() {
        let mut resonator = DspModalResonator::new().expect("modal resonator allocation");
        let mut output = [1.0_f32; 4];
        assert_eq!(
            resonator.process_ramp(440.0, 440.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, &mut output,),
            Err(DspModalResonatorError::NotPrepared)
        );
        assert!(output.iter().all(|sample| *sample == 0.0));
        assert_eq!(resonator.prepare(48_000.0, 4), Ok(()));
        assert_eq!(
            resonator.prepare(48_000.0, 6),
            Err(DspModalResonatorError::InvalidArgument)
        );
    }

    #[test]
    fn processing_is_finite_and_reset_is_bit_exact() {
        let mut resonator = DspModalResonator::new().expect("modal resonator allocation");
        resonator
            .prepare(48_000.0, 8)
            .expect("modal resonator preparation");
        let mut first = [0.0_f32; 64];
        first[0] = 1.0;
        resonator
            .process_ramp(440.0, 440.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, &mut first)
            .expect("modal processing");
        assert!(first.iter().all(|sample| sample.is_finite()));
        resonator.reset().expect("modal reset");
        let mut second = [0.0_f32; 64];
        second[0] = 1.0;
        resonator
            .process_ramp(440.0, 440.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, &mut second)
            .expect("modal processing after reset");
        assert_eq!(
            first
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>(),
            second
                .iter()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn invalid_input_clears_output() {
        let mut resonator = DspModalResonator::new().expect("modal resonator allocation");
        resonator
            .prepare(48_000.0, 4)
            .expect("modal resonator preparation");
        let mut output = [1.0_f32; 4];
        assert_eq!(
            resonator.process_ramp(440.0, 24_000.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, &mut output,),
            Err(DspModalResonatorError::InvalidArgument)
        );
        assert!(output.iter().all(|sample| *sample == 0.0));
    }

    #[cfg(sonalloy_test_hooks)]
    #[test]
    fn native_exception_clears_output() {
        let mut resonator = DspModalResonator::new().expect("modal resonator allocation");
        resonator
            .prepare(48_000.0, 4)
            .expect("modal resonator preparation");
        unsafe { sonalloy_dsp_test_arm_modal_process_exception(resonator.handle.as_ptr()) };
        let mut output = [1.0_f32; 4];
        assert_eq!(
            resonator.process_ramp(440.0, 440.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, &mut output,),
            Err(DspModalResonatorError::NativeException)
        );
        assert!(output.iter().all(|sample| *sample == 0.0));
    }
}
