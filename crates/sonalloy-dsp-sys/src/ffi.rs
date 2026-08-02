use std::os::raw::{c_char, c_double, c_float, c_int, c_uint};

#[repr(C)]
pub(crate) struct DspOscillator {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct DspFilter {
    _private: [u8; 0],
}

pub(crate) const OK: c_int = 0;
pub(crate) const INVALID_ARGUMENT: c_int = 1;
pub(crate) const NULL_HANDLE: c_int = 2;
pub(crate) const NOT_PREPARED: c_int = 3;
pub(crate) const UNSUPPORTED_WAVEFORM: c_int = 4;
pub(crate) const NATIVE_EXCEPTION: c_int = 5;
pub(crate) const WAVEFORM_SINE: c_int = 0;
pub(crate) const WAVEFORM_SAW: c_int = 1;
pub(crate) const CAPABILITY_SINE: c_uint = 1 << 0;
pub(crate) const CAPABILITY_SAW: c_uint = 1 << 1;

unsafe extern "C" {
    pub(crate) fn sonalloy_dsp_backend_version() -> *const c_char;
    pub(crate) fn sonalloy_dsp_capabilities() -> c_uint;
    pub(crate) fn sonalloy_dsp_oscillator_create() -> *mut DspOscillator;
    pub(crate) fn sonalloy_dsp_oscillator_destroy(handle: *mut DspOscillator);
    pub(crate) fn sonalloy_dsp_oscillator_prepare(
        handle: *mut DspOscillator,
        sample_rate: c_double,
        waveform: c_int,
    ) -> c_int;
    pub(crate) fn sonalloy_dsp_oscillator_reset(handle: *mut DspOscillator) -> c_int;
    pub(crate) fn sonalloy_dsp_oscillator_process(
        handle: *mut DspOscillator,
        frequency_hz: c_float,
        output: *mut c_float,
        frames: c_uint,
    ) -> c_int;
    pub(crate) fn sonalloy_dsp_oscillator_process_ramp(
        handle: *mut DspOscillator,
        start_frequency_hz: c_float,
        end_frequency_hz: c_float,
        output: *mut c_float,
        frames: c_uint,
    ) -> c_int;

    pub(crate) fn sonalloy_dsp_filter_create() -> *mut DspFilter;
    pub(crate) fn sonalloy_dsp_filter_destroy(handle: *mut DspFilter);
    pub(crate) fn sonalloy_dsp_filter_prepare(
        handle: *mut DspFilter,
        sample_rate: c_double,
    ) -> c_int;
    pub(crate) fn sonalloy_dsp_filter_reset(handle: *mut DspFilter) -> c_int;
    pub(crate) fn sonalloy_dsp_filter_process(
        handle: *mut DspFilter,
        cutoff_hz: c_float,
        resonance: c_float,
        buffer: *mut c_float,
        frames: c_uint,
    ) -> c_int;
    pub(crate) fn sonalloy_dsp_filter_process_ramp(
        handle: *mut DspFilter,
        start_cutoff_hz: c_float,
        end_cutoff_hz: c_float,
        resonance: c_float,
        buffer: *mut c_float,
        frames: c_uint,
    ) -> c_int;
    pub(crate) fn sonalloy_dsp_filter_process_ramp_with_resonance(
        handle: *mut DspFilter,
        start_cutoff_hz: c_float,
        end_cutoff_hz: c_float,
        start_resonance: c_float,
        end_resonance: c_float,
        buffer: *mut c_float,
        frames: c_uint,
    ) -> c_int;

    #[cfg(all(sonalloy_test_hooks, test))]
    pub(crate) fn sonalloy_dsp_test_arm_process_exception(handle: *mut DspOscillator);
}
