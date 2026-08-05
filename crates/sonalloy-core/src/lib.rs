pub mod asset;
pub mod compiler;
pub mod definition;
pub mod diagnostics;
pub mod parameter;
pub mod process;
pub mod render;
pub mod runtime;

pub use compiler::{CompileContext, CompileResult, CompiledInstrument, compile_instrument};
pub use definition::{
    AdsrDefinition, AssetReference, CURRENT_SCHEMA_VERSION, DelayProcessorDefinition,
    DriveProcessorDefinition, FilterProcessorDefinition, GeneratorDefinition, HardSyncDefinition,
    InstrumentDefinition, InstrumentMetadata, LayerDefinition, LayerTriggerDefinition, NoiseColor,
    NoiseDefinition, OscillatorDefinition, OscillatorWaveform, PerformanceDefinition,
    ProcessorDefinition, ReverbProcessorDefinition, SampleDefinition, SampleInterpolation,
    SampleZoneDefinition, SampleZonePlaybackDefinition, UnisonDefinition, VoiceStealingDefinition,
    WaveshapingDefinition,
};
pub use definition::{
    LfoDefinition, LfoWaveform, ModEnvelopeDefinition, ModulationCurve, ModulationDefinition,
    ModulationRouteDefinition, ModulationSourceDefinition, RandomDefinition,
};
pub use diagnostics::{Diagnostic, DiagnosticCode, DiagnosticSeverity, from_render_error};
pub use parameter::{
    BUILTIN_SOURCE_IDS, ParameterCatalog, ParameterDescriptor, ParameterHandle, ParameterOwner,
    ParameterScale, ParameterUnit, ParameterValueError, global_processor_parameter_id,
    is_component_id, is_parameter_id, layer_generator_parameter_id, layer_parameter_id,
    layer_processor_parameter_id, voice_processor_parameter_id,
};
pub use process::{
    DspFailureKind, InstrumentProcessor, NoteId, ProcessBlock, ProcessContext, ProcessError,
    ProcessEvent, ProcessEventKind, ProcessSpec, ProcessorFailureKind, ScheduledEvent,
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
