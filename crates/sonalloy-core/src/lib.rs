pub mod compiler;
pub mod definition;
pub mod diagnostics;
pub mod process;
pub mod render;
pub mod runtime;

pub use compiler::{CompileContext, CompileResult, CompiledInstrument, compile_instrument};
pub use definition::{
    AdsrDefinition, CURRENT_SCHEMA_VERSION, FilterDefinition, GeneratorDefinition,
    InstrumentDefinition, InstrumentMetadata, LayerDefinition, LayerTriggerDefinition,
    OscillatorDefinition, OscillatorWaveform, PerformanceDefinition, VelocityResponseDefinition,
    VoiceStealingDefinition,
};
pub use diagnostics::{Diagnostic, DiagnosticCode, DiagnosticSeverity, from_render_error};
pub use process::{
    DspFailureKind, InstrumentProcessor, NoteId, ProcessBlock, ProcessContext, ProcessError,
    ProcessEvent, ProcessEventKind, ProcessSpec, ScheduledEvent,
};
pub use render::{
    RenderError, RenderRequest, RenderedAudio, render_instrument, render_sine, seconds_to_frames,
};
pub use runtime::{InstrumentRuntime, SineRuntime, VoiceState};

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
