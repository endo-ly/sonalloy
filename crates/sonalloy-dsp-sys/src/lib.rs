mod ffi;
mod oscillator;

pub use oscillator::{
    DspCapabilities, DspError, DspOscillator, DspOscillatorWaveform, backend_version, capabilities,
};
