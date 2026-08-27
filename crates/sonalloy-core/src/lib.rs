pub mod analysis;
pub mod asset;
pub mod compiler;
mod convolution;
pub mod definition;
pub mod diagnostics;
mod formant;
mod generator_parameters;
pub mod parameter;
pub mod process;
pub mod render;
pub mod runtime;
mod spectral;
pub mod trace;
mod wavetable;

pub use analysis::{
    ActivityAnalysis, AudioAnalysis, AudioAnalysisError, AudioAnalysisOptions, ContinuityAnalysis,
    LevelAnalysis, SpectralPeak, SpectrumAnalysis, StereoAnalysis, analyze_rendered_audio,
};
pub use asset::{PreparedAudio, PreparedAudioChannels, SampleMetadata};
pub use compiler::{
    CompileContext, CompileResult, CompiledAdditive, CompiledAdditiveParameters,
    CompiledAdditivePartial, CompiledBitcrusherParameters, CompiledBitcrusherProcessor,
    CompiledChorusProcessor, CompiledCompressorParameters, CompiledCompressorProcessor,
    CompiledConvolutionParameters, CompiledConvolutionProcessor, CompiledDelayTap,
    CompiledDelayTime, CompiledEqParameters, CompiledEqProcessor, CompiledFlangerProcessor,
    CompiledFormant, CompiledFormantBand, CompiledFormantParameters, CompiledFormantProcessor,
    CompiledFormantProcessorParameters, CompiledFormantProfile, CompiledFrequencyShifterParameters,
    CompiledFrequencyShifterProcessor, CompiledGateParameters, CompiledGateProcessor,
    CompiledGranular, CompiledGranularParameters, CompiledInstrument, CompiledInstrumentSource,
    CompiledLfo, CompiledLimiterParameters, CompiledLimiterProcessor, CompiledModal,
    CompiledModalParameters, CompiledModulationDelayParameters, CompiledMseg, CompiledMsegSegment,
    CompiledOperator, CompiledOperatorModulation, CompiledOperatorParameters,
    CompiledOperatorTopology, CompiledPerformance, CompiledPerformanceMode,
    CompiledPhaserParameters, CompiledPhaserProcessor, CompiledPhysicalExciter,
    CompiledPhysicalString, CompiledPhysicalStringParameters, CompiledResonatorParameters,
    CompiledResonatorProcessor, CompiledSampleDirection, CompiledSampleHold, CompiledSampleLoop,
    CompiledSamplePlayback, CompiledSampleTime, CompiledSmoothRandom, CompiledSource,
    CompiledSourceRef, CompiledSpectral, CompiledSpectralParameters, CompiledStep,
    CompiledStretchLatency, CompiledTransientShaperParameters, CompiledTransientShaperProcessor,
    CompiledVector, CompiledVoiceSource, CompiledVoiceStealing, CompiledWaveSequence,
    CompiledWaveSequenceDuration, CompiledWaveSequenceStep, CompiledWaveSequenceStepPlayback,
    CompiledWavetable, CompiledWavetableParameters, GeneratorOutputMode, InstrumentSourceHandle,
    PreparedWavetable, PreparedWavetableBand, PreparedWavetableFrame, SourceHandle,
    WavetableSourceMetadata, compile_instrument,
};
pub use definition::{
    AdditiveDefinition, AdditivePartialDefinition, AdsrDefinition, AssetReference,
    BitcrusherProcessorDefinition, CURRENT_SCHEMA_VERSION, ChorusProcessorDefinition,
    CompressorProcessorDefinition, ConvolutionProcessorDefinition, DelayFeedbackMode,
    DelayProcessorDefinition, DelayTapDefinition, DelayTimeDefinition, DelayTimeUnit,
    DriveProcessorDefinition, EqProcessorDefinition, FilterModeDefinition,
    FilterProcessorDefinition, FlangerProcessorDefinition, FormantBandDefinition,
    FormantDefinition, FormantProcessorDefinition, FormantProfileDefinition,
    FrequencyShifterProcessorDefinition, GateProcessorDefinition, GeneratorDefinition,
    GranularDefinition, HardSyncDefinition, InstrumentDefinition, InstrumentMetadata,
    LayerDefinition, LayerTriggerDefinition, LayerTriggerEvent, LimiterProcessorDefinition,
    ModalDefinition, ModulationDepthDefinition, ModulationDurationDefinition,
    ModulationDurationUnit, ModulationRateDefinition, ModulationRateUnit, ModulationSegmentCurve,
    MsegDefinition, MsegLoopDefinition, MsegSegmentDefinition, NoiseColor, NoiseDefinition,
    OperatorAlgorithm, OperatorDefinition, OperatorModulationDefinition, OperatorModulationMode,
    OscillatorDefinition, OscillatorFeedbackDefinition, OscillatorWaveform, PerformanceDefinition,
    PhaseDistortionDefinition, PhaserProcessorDefinition, PhysicalExciterDefinition,
    PhysicalStringDefinition, PortamentoDefinition, ProcessorDefinition,
    ResonatorProcessorDefinition, ReverbProcessorDefinition, SampleDefinition, SampleInterpolation,
    SampleLoopDefinition, SamplePlaybackDirection, SampleRegionDefinition, SampleTimeDefinition,
    SampleZoneDefinition, SampleZonePlaybackDefinition, SpectralDefinition,
    TransientShaperProcessorDefinition, UnisonDefinition, VoiceStealingDefinition,
    WaveSequenceDefinition, WaveSequenceDirection, WaveSequenceDurationDefinition,
    WaveSequenceStepDefinition, WaveSequenceStepPlayback, WavefoldDefinition,
    WaveshapingDefinition, WavetableDefinition,
};
pub use definition::{
    LfoDefinition, LfoWaveform, MacroDefinition, ModEnvelopeDefinition, ModulationCurve,
    ModulationDefinition, ModulationRouteDefinition, ModulationSourceDefinition, RandomDefinition,
    SampleHoldDefinition, SmoothRandomDefinition, StepModulatorDefinition, VectorDefinition,
};
pub use diagnostics::{Diagnostic, DiagnosticCode, DiagnosticSeverity, from_render_error};
pub use parameter::{
    BUILTIN_SOURCE_IDS, ModulationUnit, ParameterCatalog, ParameterDescriptor, ParameterHandle,
    ParameterOwner, ParameterScale, ParameterUnit, ParameterValueError, VectorAxis,
    global_processor_parameter_id, is_component_id, is_parameter_id, layer_generator_parameter_id,
    layer_parameter_id, layer_processor_parameter_id, voice_processor_parameter_id,
};
pub use process::{
    DEFAULT_TIME_SIGNATURE, DspFailureKind, InstrumentProcessor, NoteId, ProcessBlock,
    ProcessContext, ProcessError, ProcessEvent, ProcessEventKind, ProcessSpec,
    ProcessorFailureKind, ScheduledEvent, TimeSignature,
};
pub use render::{
    DEFAULT_TEMPO_BPM, MusicalTimeChange, MusicalTimeMap, PreparedMusicalTimeMap, RenderError,
    RenderRequest, RenderedAudio, render_instrument, render_instrument_with_musical_time_map,
    render_instrument_with_reset, render_instrument_with_tempo, render_instrument_with_trace,
    render_sine, seconds_to_frames,
};
pub use runtime::{InstrumentRuntime, SineRuntime, VoiceState};
pub use spectral::PreparedSpectralAsset;
pub use trace::{
    MAX_TRACE_OBSERVATIONS, RenderTraceReport, TraceContribution, TraceDepth, TraceObservation,
    TraceParameterReport, TraceRequest, TraceRoute, TraceVoice, TraceVoiceState,
};

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
