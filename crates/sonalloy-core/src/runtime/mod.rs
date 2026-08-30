use std::sync::Arc;

use crate::compiler::CompiledInstrument;

pub(crate) mod adsr;
pub(crate) mod external_audio;
pub(crate) mod formant;
pub(crate) mod fractional_delay;
pub(crate) mod generator;
mod instrument;
pub(crate) mod interpolation;
pub(crate) mod mix;
pub mod modulation;
pub(crate) mod processor;
mod random;
pub(crate) mod sample;
pub(crate) mod sine;
pub(crate) mod smoothing;
pub(crate) mod source;
mod voice;
pub use instrument::InstrumentRuntime;
pub use sine::SineRuntime;
pub use voice::VoiceState;

impl CompiledInstrument {
    /// Create a fresh runtime instance that owns no active audio state yet.
    #[must_use]
    pub fn instantiate(self: &Arc<Self>) -> InstrumentRuntime {
        InstrumentRuntime::new(Arc::clone(self))
    }
}

/// Return the bytes allocated by Spectral Morph's runtime-owned buffers.
///
/// The result includes the fixed FFT, history, overlap-add, window, and scratch buffers, plus the
/// stereo external-input alignment delay requested by `alignment_frames`.
#[must_use]
pub fn spectral_morph_runtime_buffer_bytes(alignment_frames: usize) -> usize {
    processor::spectral_morph_runtime_buffer_bytes(alignment_frames)
}
