mod ffi;
mod filter;
mod oscillator;
mod variable_oscillator;

pub use filter::{DspFilter, DspFilterError};
pub use oscillator::{
    DspCapabilities, DspError, DspOscillator, DspOscillatorWaveform, backend_version, capabilities,
};
pub use variable_oscillator::DspVariableOscillator;
