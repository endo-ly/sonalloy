mod ffi;
mod filter;
mod oscillator;
mod stretch;
mod variable_oscillator;
mod wavefolder;

pub use filter::{DspFilter, DspFilterError};
pub use oscillator::{
    DspCapabilities, DspError, DspOscillator, DspOscillatorWaveform, backend_version, capabilities,
};
pub use stretch::{DspStretch, DspStretchError, backend_version as stretch_backend_version};
pub use variable_oscillator::DspVariableOscillator;
pub use wavefolder::{DspWavefolder, DspWavefolderError};
