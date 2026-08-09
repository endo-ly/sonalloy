use std::ffi::CStr;
use std::ptr::NonNull;

use thiserror::Error;

use crate::ffi;

fn result_from_code(code: i32) -> Result<(), DspStretchError> {
    match code {
        ffi::STRETCH_OK => Ok(()),
        ffi::STRETCH_INVALID_ARGUMENT => Err(DspStretchError::InvalidArgument),
        ffi::STRETCH_NULL_HANDLE => Err(DspStretchError::NullHandle),
        ffi::STRETCH_NOT_PREPARED => Err(DspStretchError::NotPrepared),
        ffi::STRETCH_NATIVE_EXCEPTION => Err(DspStretchError::NativeException),
        ffi::STRETCH_NON_FINITE => Err(DspStretchError::NonFinite),
        other => Err(DspStretchError::Unknown(other)),
    }
}

/// Errors returned by the native time-stretch boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DspStretchError {
    /// The native object could not be allocated.
    #[error("native object allocation failed")]
    AllocationFailed,
    /// The caller supplied an invalid value or buffer shape.
    #[error("invalid argument")]
    InvalidArgument,
    /// A native handle was null.
    #[error("null native handle")]
    NullHandle,
    /// Processing was requested before preparation.
    #[error("stretch backend is not prepared")]
    NotPrepared,
    /// The native implementation raised an exception.
    #[error("native exception")]
    NativeException,
    /// An input or output contained a non-finite value.
    #[error("non-finite sample")]
    NonFinite,
    /// The native result code was not recognized.
    #[error("unknown native error code {0}")]
    Unknown(i32),
}

/// Fixed native Signalsmith Stretch state owned by one runtime voice.
pub struct DspStretch {
    handle: NonNull<ffi::DspStretch>,
    channels: usize,
    max_input_frames: usize,
    max_output_frames: usize,
    interval_frames: usize,
    prepared: bool,
}

impl DspStretch {
    /// Create an unprepared stretch backend.
    ///
    /// # Errors
    ///
    /// Returns [`DspStretchError::AllocationFailed`] when the native state cannot be created.
    pub fn new() -> Result<Self, DspStretchError> {
        let handle = unsafe { ffi::sonalloy_stretch_create() };
        let handle = NonNull::new(handle).ok_or(DspStretchError::AllocationFailed)?;
        Ok(Self {
            handle,
            channels: 0,
            max_input_frames: 0,
            max_output_frames: 0,
            interval_frames: 0,
            prepared: false,
        })
    }

    /// Prepare the backend and reserve its process capacity.
    ///
    /// # Errors
    ///
    /// Returns an error when the channel count, sample rate, capacity, or native configuration is
    /// invalid.
    pub fn prepare(
        &mut self,
        channels: usize,
        sample_rate: f64,
        max_input_frames: usize,
        max_output_frames: usize,
    ) -> Result<(), DspStretchError> {
        self.prepared = false;
        let channels = i32::try_from(channels).map_err(|_| DspStretchError::InvalidArgument)?;
        let max_input_frames =
            u32::try_from(max_input_frames).map_err(|_| DspStretchError::InvalidArgument)?;
        let max_output_frames =
            u32::try_from(max_output_frames).map_err(|_| DspStretchError::InvalidArgument)?;
        let code = unsafe {
            ffi::sonalloy_stretch_prepare(
                self.handle.as_ptr(),
                channels,
                sample_rate,
                max_input_frames,
                max_output_frames,
            )
        };
        result_from_code(code)?;
        self.channels = usize::try_from(channels).map_err(|_| DspStretchError::InvalidArgument)?;
        self.max_input_frames =
            usize::try_from(max_input_frames).map_err(|_| DspStretchError::InvalidArgument)?;
        self.max_output_frames =
            usize::try_from(max_output_frames).map_err(|_| DspStretchError::InvalidArgument)?;
        self.interval_frames = latency_from_raw(unsafe {
            ffi::sonalloy_stretch_interval_samples(self.handle.as_ptr())
        })?;
        self.prepared = true;
        Ok(())
    }

    /// Clear the backend's spectral and streaming state.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend has not been prepared or native reset fails.
    pub fn reset(&mut self) -> Result<(), DspStretchError> {
        if !self.prepared {
            return Err(DspStretchError::NotPrepared);
        }
        let code = unsafe { ffi::sonalloy_stretch_reset(self.handle.as_ptr()) };
        result_from_code(code)
    }

    /// Set the pitch shift in semitones without changing the requested duration ratio.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend is not prepared, the value is non-finite, or native
    /// processing fails.
    pub fn set_pitch_semitones(&mut self, semitones: f64) -> Result<(), DspStretchError> {
        if !self.prepared {
            return Err(DspStretchError::NotPrepared);
        }
        let code = unsafe { ffi::sonalloy_stretch_set_pitch(self.handle.as_ptr(), semitones) };
        result_from_code(code)
    }

    /// Move the processing position to the supplied input without producing output.
    ///
    /// The input should contain the backend's input latency worth of samples when starting a
    /// fixed-length sound. The playback rate is input frames per output frame.
    ///
    /// # Errors
    ///
    /// Returns an error when the input shape, playback rate, or native seek operation is invalid.
    pub fn seek(&mut self, input: &[&[f32]], playback_rate: f64) -> Result<(), DspStretchError> {
        if !self.prepared {
            return Err(DspStretchError::NotPrepared);
        }
        if input.len() != self.channels || !playback_rate.is_finite() || playback_rate <= 0.0 {
            return Err(DspStretchError::InvalidArgument);
        }
        let input_frames = input.first().map_or(0, |channel| channel.len());
        if input.iter().any(|channel| channel.len() != input_frames)
            || input_frames > self.max_input_frames
        {
            return Err(DspStretchError::InvalidArgument);
        }
        if input
            .iter()
            .flat_map(|channel| channel.iter())
            .any(|sample| !sample.is_finite())
        {
            return Err(DspStretchError::NonFinite);
        }
        let mut input_pointers = [std::ptr::null(); 2];
        for index in 0..self.channels {
            input_pointers[index] = input[index].as_ptr();
        }
        let input_frames =
            u32::try_from(input_frames).map_err(|_| DspStretchError::InvalidArgument)?;
        let code = unsafe {
            ffi::sonalloy_stretch_seek(
                self.handle.as_ptr(),
                input_pointers.as_ptr(),
                input_frames,
                playback_rate,
            )
        };
        result_from_code(code)
    }

    /// Process planar input and output buffers with independently chosen frame counts.
    ///
    /// # Errors
    ///
    /// Returns an error when the buffers violate the prepared channel or capacity contract, a
    /// sample is non-finite, or the native backend fails. Output buffers are cleared on failure.
    pub fn process(
        &mut self,
        input: &[&[f32]],
        output: &mut [&mut [f32]],
    ) -> Result<(), DspStretchError> {
        if !self.prepared {
            clear_output(output);
            return Err(DspStretchError::NotPrepared);
        }
        if input.len() != self.channels || output.len() != self.channels {
            clear_output(output);
            return Err(DspStretchError::InvalidArgument);
        }
        let input_frames = input.first().map_or(0, |channel| channel.len());
        let output_frames = output.first().map_or(0, |channel| channel.len());
        if input.iter().any(|channel| channel.len() != input_frames)
            || output.iter().any(|channel| channel.len() != output_frames)
            || input_frames > self.max_input_frames
            || output_frames > self.max_output_frames
        {
            clear_output(output);
            return Err(DspStretchError::InvalidArgument);
        }
        if input
            .iter()
            .flat_map(|channel| channel.iter())
            .any(|sample| !sample.is_finite())
        {
            clear_output(output);
            return Err(DspStretchError::NonFinite);
        }
        let mut input_pointers = [std::ptr::null(); 2];
        let mut output_pointers = [std::ptr::null_mut(); 2];
        for index in 0..self.channels {
            input_pointers[index] = input[index].as_ptr();
            output_pointers[index] = output[index].as_mut_ptr();
        }
        let input_frames = u32::try_from(input_frames).map_err(|_| {
            clear_output(output);
            DspStretchError::InvalidArgument
        })?;
        let output_frames = u32::try_from(output_frames).map_err(|_| {
            clear_output(output);
            DspStretchError::InvalidArgument
        })?;
        let code = unsafe {
            ffi::sonalloy_stretch_process(
                self.handle.as_ptr(),
                input_pointers.as_ptr(),
                input_frames,
                output_pointers.as_mut_ptr(),
                output_frames,
            )
        };
        let result = result_from_code(code);
        if result.is_err() {
            clear_output(output);
        }
        result
    }

    /// Flush the remaining output after all input has reached the processing end.
    ///
    /// The native backend is reset by this operation; a subsequent lifecycle should begin with
    /// seek or reset.
    ///
    /// # Errors
    ///
    /// Returns an error when the output shape exceeds prepared capacity or native flushing fails.
    pub fn flush(&mut self, output: &mut [&mut [f32]]) -> Result<(), DspStretchError> {
        if !self.prepared {
            clear_output(output);
            return Err(DspStretchError::NotPrepared);
        }
        if output.len() != self.channels {
            clear_output(output);
            return Err(DspStretchError::InvalidArgument);
        }
        let output_frames = output.first().map_or(0, |channel| channel.len());
        if output.iter().any(|channel| channel.len() != output_frames)
            || output_frames > self.max_output_frames
        {
            clear_output(output);
            return Err(DspStretchError::InvalidArgument);
        }
        let mut output_pointers = [std::ptr::null_mut(); 2];
        for index in 0..self.channels {
            output_pointers[index] = output[index].as_mut_ptr();
        }
        let output_frames = u32::try_from(output_frames).map_err(|_| {
            clear_output(output);
            DspStretchError::InvalidArgument
        })?;
        let code = unsafe {
            ffi::sonalloy_stretch_flush(
                self.handle.as_ptr(),
                output_pointers.as_mut_ptr(),
                output_frames,
            )
        };
        let result = result_from_code(code);
        if result.is_err() {
            clear_output(output);
        }
        result
    }

    /// Return the backend's input-side latency in frames.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend is not prepared or reports an invalid latency.
    pub fn input_latency(&self) -> Result<usize, DspStretchError> {
        if !self.prepared {
            return Err(DspStretchError::NotPrepared);
        }
        latency_from_raw(unsafe { ffi::sonalloy_stretch_input_latency(self.handle.as_ptr()) })
    }

    /// Return the backend's output-side latency in frames.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend is not prepared or reports an invalid latency.
    pub fn output_latency(&self) -> Result<usize, DspStretchError> {
        if !self.prepared {
            return Err(DspStretchError::NotPrepared);
        }
        latency_from_raw(unsafe { ffi::sonalloy_stretch_output_latency(self.handle.as_ptr()) })
    }

    /// Return the backend's spectral analysis interval in frames.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend is not prepared or reports an invalid interval.
    pub fn interval_samples(&self) -> Result<usize, DspStretchError> {
        if !self.prepared {
            return Err(DspStretchError::NotPrepared);
        }
        Ok(self.interval_frames)
    }
}

impl Drop for DspStretch {
    fn drop(&mut self) {
        unsafe { ffi::sonalloy_stretch_destroy(self.handle.as_ptr()) };
    }
}

fn latency_from_raw(value: i32) -> Result<usize, DspStretchError> {
    usize::try_from(value).map_err(|_| DspStretchError::NotPrepared)
}

fn clear_output(output: &mut [&mut [f32]]) {
    for channel in output {
        channel.fill(0.0);
    }
}

/// Return the fixed backend version exposed by the Native boundary.
#[must_use]
pub fn backend_version() -> String {
    unsafe {
        CStr::from_ptr(ffi::sonalloy_stretch_backend_version())
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(sonalloy_test_hooks)]
    unsafe extern "C" {
        fn sonalloy_dsp_test_arm_stretch_process_exception(handle: *mut ffi::DspStretch);
    }

    #[test]
    fn backend_reports_fixed_version_and_latency() {
        let mut stretch = DspStretch::new().expect("stretch allocation");
        assert!(backend_version().contains("signalsmith-stretch-1.3.2"));
        assert_eq!(stretch.reset(), Err(DspStretchError::NotPrepared));
        stretch
            .prepare(2, 48_000.0, 512, 257)
            .expect("stretch preparation");
        assert!(stretch.input_latency().expect("input latency") > 0);
        assert!(stretch.output_latency().expect("output latency") > 0);
        assert!(stretch.interval_samples().expect("interval") > 0);
    }

    #[test]
    fn seek_and_flush_complete_a_prepared_lifecycle() {
        let mut stretch = DspStretch::new().expect("stretch allocation");
        stretch
            .prepare(2, 48_000.0, 8_192, 8_192)
            .expect("stretch preparation");
        let input_latency = stretch.input_latency().expect("input latency");
        let output_latency = stretch.output_latency().expect("output latency");
        let mut seek_left = vec![0.0; input_latency];
        let mut seek_right = vec![0.0; input_latency];
        seek_left[0] = 1.0;
        seek_right[0] = -1.0;
        let input: [&[f32]; 2] = [&seek_left, &seek_right];
        stretch.seek(&input, 1.0).expect("stretch seek");
        stretch
            .set_pitch_semitones(0.0)
            .expect("pitch configuration");

        let empty_left: [f32; 0] = [];
        let empty_right: [f32; 0] = [];
        let input: [&[f32]; 2] = [&empty_left, &empty_right];
        let mut process_left = vec![0.0; 128];
        let mut process_right = vec![0.0; 128];
        let mut process_output: [&mut [f32]; 2] = [&mut process_left, &mut process_right];
        stretch
            .process(&input, &mut process_output)
            .expect("stretch process after seek");
        assert!(process_left.iter().all(|sample| sample.is_finite()));

        let mut flush_left = vec![0.0; output_latency];
        let mut flush_right = vec![0.0; output_latency];
        let mut flush_output: [&mut [f32]; 2] = [&mut flush_left, &mut flush_right];
        stretch.flush(&mut flush_output).expect("stretch flush");
        assert!(flush_left.iter().all(|sample| sample.is_finite()));
        assert!(flush_right.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn preparation_rejects_invalid_channels_rate_and_capacity() {
        let mut stretch = DspStretch::new().expect("stretch allocation");

        assert_eq!(
            stretch.prepare(3, 48_000.0, 512, 257),
            Err(DspStretchError::InvalidArgument)
        );
        assert_eq!(
            stretch.prepare(2, 0.0, 512, 257),
            Err(DspStretchError::InvalidArgument)
        );
        assert_eq!(
            stretch.prepare(2, 48_000.0, 0, 257),
            Err(DspStretchError::InvalidArgument)
        );
    }

    #[test]
    fn failed_preparation_invalidates_the_previous_lifecycle_state() {
        let mut stretch = DspStretch::new().expect("stretch allocation");
        stretch
            .prepare(2, 48_000.0, 512, 257)
            .expect("stretch preparation");
        assert_eq!(
            stretch.prepare(3, 48_000.0, 512, 257),
            Err(DspStretchError::InvalidArgument)
        );
        assert_eq!(stretch.reset(), Err(DspStretchError::NotPrepared));
        assert_eq!(
            stretch.set_pitch_semitones(0.0),
            Err(DspStretchError::NotPrepared)
        );
        assert_eq!(stretch.input_latency(), Err(DspStretchError::NotPrepared));
        assert_eq!(stretch.output_latency(), Err(DspStretchError::NotPrepared));
    }

    #[test]
    fn conversion_failure_invalidates_the_previous_preparation() {
        let mut stretch = DspStretch::new().expect("stretch allocation");
        stretch
            .prepare(2, 48_000.0, 512, 257)
            .expect("stretch preparation");

        let too_many_channels = usize::try_from(i32::MAX)
            .expect("i32 fits in usize")
            .saturating_add(1);
        assert_eq!(
            stretch.prepare(too_many_channels, 48_000.0, 512, 257),
            Err(DspStretchError::InvalidArgument)
        );
        assert_eq!(stretch.reset(), Err(DspStretchError::NotPrepared));
    }

    #[test]
    fn mono_process_preserves_finite_output() {
        let mut stretch = DspStretch::new().expect("stretch allocation");
        stretch
            .prepare(1, 48_000.0, 512, 257)
            .expect("stretch preparation");
        let input = [0.25_f32; 128];
        let input: [&[f32]; 1] = [&input];
        let mut output_values = [0.0_f32; 64];
        let mut output: [&mut [f32]; 1] = [&mut output_values];

        stretch
            .process(&input, &mut output)
            .expect("mono stretch process");
        assert!(output_values.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn stereo_process_preserves_finite_planar_output() {
        let mut stretch = DspStretch::new().expect("stretch allocation");
        stretch
            .prepare(2, 48_000.0, 512, 257)
            .expect("stretch preparation");
        stretch
            .set_pitch_semitones(12.0)
            .expect("pitch configuration");
        let input_left = vec![0.25_f32; 128];
        let input_right = vec![-0.25_f32; 128];
        let input: [&[f32]; 2] = [&input_left, &input_right];
        let mut output_left = vec![0.0_f32; 64];
        let mut output_right = vec![0.0_f32; 64];
        let mut output: [&mut [f32]; 2] = [&mut output_left, &mut output_right];
        stretch
            .process(&input, &mut output)
            .expect("stretch process");
        assert!(output_left.iter().all(|sample| sample.is_finite()));
        assert!(output_right.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn zero_input_frames_produce_a_finite_output() {
        let mut stretch = DspStretch::new().expect("stretch allocation");
        stretch
            .prepare(2, 48_000.0, 512, 257)
            .expect("stretch preparation");
        let input_left: [f32; 0] = [];
        let input_right: [f32; 0] = [];
        let input: [&[f32]; 2] = [&input_left, &input_right];
        let mut output_left = [0.0_f32; 4];
        let mut output_right = [0.0_f32; 4];
        let mut output: [&mut [f32]; 2] = [&mut output_left, &mut output_right];

        stretch
            .process(&input, &mut output)
            .expect("zero-input stretch process");
        assert!(output_left.iter().all(|sample| sample.is_finite()));
        assert!(output_right.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn invalid_process_input_clears_output() {
        let mut stretch = DspStretch::new().expect("stretch allocation");
        stretch
            .prepare(2, 48_000.0, 512, 257)
            .expect("stretch preparation");
        let input = [[f32::NAN]; 2];
        let input: [&[f32]; 2] = [&input[0], &input[1]];
        let mut left = [1.0_f32; 4];
        let mut right = [1.0_f32; 4];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        assert_eq!(
            stretch.process(&input, &mut output),
            Err(DspStretchError::NonFinite)
        );
        assert!(left.iter().all(|sample| *sample == 0.0));
        assert!(right.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn mismatched_process_buffers_are_rejected_and_cleared() {
        let mut stretch = DspStretch::new().expect("stretch allocation");
        stretch
            .prepare(2, 48_000.0, 512, 257)
            .expect("stretch preparation");
        let input_left = [0.25_f32; 8];
        let input_right = [0.25_f32; 7];
        let input: [&[f32]; 2] = [&input_left, &input_right];
        let mut output_left = [1.0_f32; 4];
        let mut output_right = [1.0_f32; 4];
        let mut output: [&mut [f32]; 2] = [&mut output_left, &mut output_right];

        assert_eq!(
            stretch.process(&input, &mut output),
            Err(DspStretchError::InvalidArgument)
        );
        assert!(output_left.iter().all(|sample| *sample == 0.0));
        assert!(output_right.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn native_null_handle_is_rejected() {
        let code =
            unsafe { ffi::sonalloy_stretch_prepare(std::ptr::null_mut(), 2, 48_000.0, 8, 8) };
        assert_eq!(code, ffi::STRETCH_NULL_HANDLE);
        let code = unsafe { ffi::sonalloy_stretch_reset(std::ptr::null_mut()) };
        assert_eq!(code, ffi::STRETCH_NULL_HANDLE);
        unsafe { ffi::sonalloy_stretch_destroy(std::ptr::null_mut()) };
    }

    #[cfg(sonalloy_test_hooks)]
    #[test]
    fn native_exception_is_caught_and_output_is_cleared() {
        let mut stretch = DspStretch::new().expect("stretch allocation");
        stretch
            .prepare(2, 48_000.0, 128, 64)
            .expect("stretch preparation");
        unsafe { sonalloy_dsp_test_arm_stretch_process_exception(stretch.handle.as_ptr()) };
        let input_left = [0.25_f32; 32];
        let input_right = [-0.25_f32; 32];
        let input: [&[f32]; 2] = [&input_left, &input_right];
        let mut output_left = [1.0_f32; 64];
        let mut output_right = [1.0_f32; 64];
        let mut output: [&mut [f32]; 2] = [&mut output_left, &mut output_right];
        assert_eq!(
            stretch.process(&input, &mut output),
            Err(DspStretchError::NativeException)
        );
        assert!(output_left.iter().all(|sample| *sample == 0.0));
        assert!(output_right.iter().all(|sample| *sample == 0.0));
    }
}
