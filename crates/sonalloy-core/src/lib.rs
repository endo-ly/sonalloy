pub mod asset;
pub mod compiler;
pub mod definition;
pub mod diagnostics;
mod generator_parameters;
pub mod parameter;
pub mod process;
pub mod render;
pub mod runtime;
mod spectral;
mod wavetable;

pub use asset::{PreparedAudio, PreparedAudioChannels, SampleMetadata};
pub use compiler::{
    CompileContext, CompileResult, CompiledAdditive, CompiledAdditiveParameters,
    CompiledAdditivePartial, CompiledBitcrusherParameters, CompiledBitcrusherProcessor,
    CompiledChorusProcessor, CompiledCompressorParameters, CompiledCompressorProcessor,
    CompiledEqParameters, CompiledEqProcessor, CompiledFlangerProcessor, CompiledFormant,
    CompiledFormantBand, CompiledFormantParameters, CompiledFormantProfile, CompiledGranular,
    CompiledGranularParameters, CompiledInstrument, CompiledLimiterParameters,
    CompiledLimiterProcessor, CompiledModulationDelayParameters, CompiledOperator,
    CompiledOperatorModulation, CompiledOperatorParameters, CompiledOperatorTopology,
    CompiledPhaserParameters, CompiledPhaserProcessor, CompiledResonatorParameters,
    CompiledResonatorProcessor, CompiledSampleDirection, CompiledSampleLoop,
    CompiledSamplePlayback, CompiledSampleTime, CompiledSpectral, CompiledSpectralParameters,
    CompiledStretchLatency, CompiledWaveSequence, CompiledWaveSequenceDuration,
    CompiledWaveSequenceStep, CompiledWaveSequenceStepPlayback, CompiledWavetable,
    CompiledWavetableParameters, GeneratorOutputMode, PreparedWavetable, PreparedWavetableBand,
    PreparedWavetableFrame, WavetableSourceMetadata, compile_instrument,
};
pub use definition::{
    AdditiveDefinition, AdditivePartialDefinition, AdsrDefinition, AssetReference,
    BitcrusherProcessorDefinition, CURRENT_SCHEMA_VERSION, ChorusProcessorDefinition,
    CompressorProcessorDefinition, DelayProcessorDefinition, DriveProcessorDefinition,
    EqProcessorDefinition, FilterModeDefinition, FilterProcessorDefinition,
    FlangerProcessorDefinition, FormantBandDefinition, FormantDefinition, FormantProfileDefinition,
    GeneratorDefinition, GranularDefinition, HardSyncDefinition, InstrumentDefinition,
    InstrumentMetadata, LayerDefinition, LayerTriggerDefinition, LayerTriggerEvent,
    LimiterProcessorDefinition, NoiseColor, NoiseDefinition, OperatorAlgorithm, OperatorDefinition,
    OperatorModulationDefinition, OperatorModulationMode, OscillatorDefinition,
    OscillatorFeedbackDefinition, OscillatorWaveform, PerformanceDefinition,
    PhaseDistortionDefinition, PhaserProcessorDefinition, ProcessorDefinition,
    ResonatorProcessorDefinition, ReverbProcessorDefinition, SampleDefinition, SampleInterpolation,
    SampleLoopDefinition, SamplePlaybackDirection, SampleRegionDefinition, SampleTimeDefinition,
    SampleZoneDefinition, SampleZonePlaybackDefinition, SpectralDefinition, UnisonDefinition,
    VoiceStealingDefinition, WaveSequenceDefinition, WaveSequenceDirection,
    WaveSequenceDurationDefinition, WaveSequenceStepDefinition, WaveSequenceStepPlayback,
    WavefoldDefinition, WaveshapingDefinition, WavetableDefinition,
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
    DEFAULT_TEMPO_BPM, RenderError, RenderRequest, RenderedAudio, TempoChange, TempoMap,
    render_instrument, render_instrument_with_tempo, render_instrument_with_tempo_map, render_sine,
    seconds_to_frames,
};
pub use runtime::{InstrumentRuntime, SineRuntime, VoiceState};
pub use spectral::PreparedSpectralAsset;

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

#[cfg(test)]
mod test_allocator {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static ALLOCATION_COUNT: Cell<Option<usize>> = const { Cell::new(None) };
    }

    struct CountingAllocator;

    #[global_allocator]
    static ALLOCATOR: CountingAllocator = CountingAllocator;

    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc(layout) };
            if !pointer.is_null() {
                record_allocation();
            }
            pointer
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc_zeroed(layout) };
            if !pointer.is_null() {
                record_allocation();
            }
            pointer
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            unsafe { System.dealloc(pointer, layout) };
        }

        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            let pointer = unsafe { System.realloc(pointer, layout, new_size) };
            if !pointer.is_null() {
                record_allocation();
            }
            pointer
        }
    }

    fn record_allocation() {
        ALLOCATION_COUNT.with(|count| {
            if let Some(value) = count.get() {
                count.set(Some(value.saturating_add(1)));
            }
        });
    }

    pub(crate) fn count_allocations(function: impl FnOnce()) -> usize {
        let previous = ALLOCATION_COUNT.with(|count| count.replace(Some(0)));
        assert!(
            previous.is_none(),
            "allocation measurement cannot be nested"
        );
        function();
        ALLOCATION_COUNT.with(|count| {
            count
                .replace(None)
                .expect("allocation measurement was unexpectedly disabled")
        })
    }
}
