use std::os::raw::{c_char, c_double, c_float, c_int, c_uint};

#[repr(C)]
pub(crate) struct DspOscillator {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct DspVariableOscillator {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct DspFilter {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct DspWavefolder {
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
pub(crate) const WAVEFORM_TRIANGLE: c_int = 2;
pub(crate) const WAVEFORM_SQUARE: c_int = 3;
pub(crate) const WAVEFORM_PULSE: c_int = 4;
pub(crate) const CAPABILITY_SINE: c_uint = 1 << 0;
pub(crate) const CAPABILITY_SAW: c_uint = 1 << 1;
pub(crate) const CAPABILITY_TRIANGLE: c_uint = 1 << 2;
pub(crate) const CAPABILITY_SQUARE: c_uint = 1 << 3;
pub(crate) const CAPABILITY_PULSE: c_uint = 1 << 4;

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
    pub(crate) fn sonalloy_dsp_oscillator_reset_phase(
        handle: *mut DspOscillator,
        phase: c_float,
    ) -> c_int;
    pub(crate) fn sonalloy_dsp_oscillator_process(
        handle: *mut DspOscillator,
        frequency_hz: c_float,
        output: *mut c_float,
        frames: c_uint,
    ) -> c_int;
    pub(crate) fn sonalloy_dsp_oscillator_process_with_pulse_width(
        handle: *mut DspOscillator,
        frequency_hz: c_float,
        pulse_width: c_float,
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
    pub(crate) fn sonalloy_dsp_oscillator_process_ramp_with_pulse_width(
        handle: *mut DspOscillator,
        start_frequency_hz: c_float,
        end_frequency_hz: c_float,
        start_pulse_width: c_float,
        end_pulse_width: c_float,
        output: *mut c_float,
        frames: c_uint,
    ) -> c_int;

    pub(crate) fn sonalloy_dsp_variable_oscillator_create() -> *mut DspVariableOscillator;
    pub(crate) fn sonalloy_dsp_variable_oscillator_destroy(handle: *mut DspVariableOscillator);
    pub(crate) fn sonalloy_dsp_variable_oscillator_prepare(
        handle: *mut DspVariableOscillator,
        sample_rate: c_double,
        waveform: c_int,
    ) -> c_int;
    pub(crate) fn sonalloy_dsp_variable_oscillator_reset(
        handle: *mut DspVariableOscillator,
    ) -> c_int;
    pub(crate) fn sonalloy_dsp_variable_oscillator_process(
        handle: *mut DspVariableOscillator,
        master_frequency_hz: c_float,
        slave_frequency_hz: c_float,
        pulse_width: c_float,
        output: *mut c_float,
        frames: c_uint,
    ) -> c_int;
    pub(crate) fn sonalloy_dsp_variable_oscillator_process_ramp(
        handle: *mut DspVariableOscillator,
        start_master_frequency_hz: c_float,
        end_master_frequency_hz: c_float,
        start_slave_frequency_hz: c_float,
        end_slave_frequency_hz: c_float,
        start_pulse_width: c_float,
        end_pulse_width: c_float,
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

    pub(crate) fn sonalloy_dsp_wavefolder_create() -> *mut DspWavefolder;
    pub(crate) fn sonalloy_dsp_wavefolder_destroy(handle: *mut DspWavefolder);
    pub(crate) fn sonalloy_dsp_wavefolder_prepare(
        handle: *mut DspWavefolder,
        sample_rate: c_double,
    ) -> c_int;
    pub(crate) fn sonalloy_dsp_wavefolder_reset(handle: *mut DspWavefolder) -> c_int;
    pub(crate) fn sonalloy_dsp_wavefolder_process(
        handle: *mut DspWavefolder,
        drive: c_float,
        mix: c_float,
        buffer: *mut c_float,
        frames: c_uint,
    ) -> c_int;
    pub(crate) fn sonalloy_dsp_wavefolder_process_ramp(
        handle: *mut DspWavefolder,
        start_drive: c_float,
        end_drive: c_float,
        start_mix: c_float,
        end_mix: c_float,
        buffer: *mut c_float,
        frames: c_uint,
    ) -> c_int;

}
