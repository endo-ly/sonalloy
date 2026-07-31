pub mod diagnostics;
pub mod process;
pub mod render;
pub mod runtime;

pub use diagnostics::{Diagnostic, DiagnosticCode, DiagnosticSeverity, from_render_error};
pub use process::{
    DspFailureKind, InstrumentProcessor, NoteId, ProcessBlock, ProcessContext, ProcessError,
    ProcessEvent, ProcessEventKind, ProcessSpec,
};
pub use render::{RenderError, RenderRequest, RenderedAudio, render_sine, seconds_to_frames};
pub use runtime::SineRuntime;

/// Backend metadata exposed without backend-specific types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendInfo {
    /// Human-readable backend version.
    pub version: String,
}

/// Return backend metadata for diagnostics and render reports.
#[must_use]
pub fn backend_info() -> BackendInfo {
    BackendInfo {
        version: sonalloy_dsp_sys::backend_version(),
    }
}
