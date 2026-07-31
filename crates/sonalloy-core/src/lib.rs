pub mod diagnostics;
pub mod process;
pub mod render;
pub mod runtime;

pub use diagnostics::{Diagnostic, DiagnosticCode, DiagnosticSeverity, from_render_error};
pub use process::{
    InstrumentProcessor, NoteId, ProcessBlock, ProcessContext, ProcessError, ProcessEvent,
    ProcessEventKind, ProcessSpec,
};
pub use render::{RenderError, RenderRequest, RenderedAudio, render_sine, seconds_to_frames};
pub use runtime::SineRuntime;
