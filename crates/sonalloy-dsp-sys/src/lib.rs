mod ffi;
mod filter;
mod oscillator;
mod variable_oscillator;
mod wavefolder;

pub use filter::{DspFilter, DspFilterError};
pub use oscillator::{
    DspCapabilities, DspError, DspOscillator, DspOscillatorWaveform, backend_version, capabilities,
};
pub use variable_oscillator::DspVariableOscillator;
pub use wavefolder::{DspWavefolder, DspWavefolderError};
