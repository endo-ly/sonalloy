mod midi;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use sonalloy_core::{
    AdsrDefinition, AudioAnalysis, AudioAnalysisOptions, CompileContext, CompiledInstrument,
    DEFAULT_TEMPO_BPM, Diagnostic, DiagnosticCode, InstrumentDefinition, InstrumentMetadata,
    LayerDefinition, LayerTriggerDefinition, ModulationCurve, ModulationUnit, OscillatorDefinition,
    OscillatorWaveform, ParameterHandle, ParameterOwner, ParameterScale, ParameterUnit,
    PerformanceDefinition, ProcessEventKind, ProcessSpec, ProcessorDefinition, RenderError,
    RenderRequest, RenderTraceReport, ScheduledEvent, TempoMap, TraceRequest,
    VoiceStealingDefinition, analyze_rendered_audio, backend_info, compile_instrument,
    from_render_error, render_instrument_with_reset, render_instrument_with_tempo,
    render_instrument_with_tempo_map, render_instrument_with_trace, render_sine, seconds_to_frames,
};

use crate::midi::read_midi;

const DEFAULT_SAMPLE_RATE: u32 = 48_000;
const DEFAULT_BLOCK_SIZE: usize = 257;

#[derive(Debug, Parser)]
#[command(
    name = "sonalloy",
    version,
    about = "Sonalloy offline instrument engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Work with JSON Instrument Definitions.
    Instrument {
        #[command(subcommand)]
        command: InstrumentCommand,
    },
    /// Render an instrument offline.
    Render {
        #[command(subcommand)]
        command: RenderCommand,
    },
    /// Development-only commands used to verify the audio path.
    Dev {
        #[command(subcommand)]
        command: DevCommand,
    },
}

#[derive(Debug, Subcommand)]
enum InstrumentCommand {
    /// Create a minimal oscillator Definition.
    Init(InitArgs),
    /// Parse, validate, and compile a Definition.
    Validate(DefinitionArgs),
    /// Display the compiled Definition in a human-readable form.
    Inspect(DefinitionArgs),
}

#[derive(Debug, Subcommand)]
enum RenderCommand {
    /// Render one Note On / Note Off pair.
    Note(RenderNoteArgs),
    /// Render an absolute-frame event sequence.
    Events(RenderEventsArgs),
    /// Render events from a Standard MIDI File.
    Midi(RenderMidiArgs),
}

#[derive(Debug, Subcommand)]
enum DevCommand {
    /// Render a sine wave through the complete audio path.
    RenderSine(RenderSineArgs),
}

#[derive(Debug, Args)]
struct InitArgs {
    /// Destination Definition path.
    path: PathBuf,
}

#[derive(Debug, Args)]
struct DefinitionArgs {
    /// Definition JSON path.
    definition: PathBuf,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RenderNoteArgs {
    /// Definition JSON path.
    definition: PathBuf,
    /// MIDI note number.
    #[arg(long, default_value_t = 60)]
    note: u8,
    /// MIDI velocity.
    #[arg(long, default_value_t = 100)]
    velocity: u8,
    /// Gate duration in seconds.
    #[arg(long, default_value_t = 0.5)]
    gate: f64,
    /// Additional render tail in seconds.
    #[arg(long, default_value_t = 0.5)]
    tail: f64,
    /// Processing tempo in beats per minute.
    #[arg(long, default_value_t = DEFAULT_TEMPO_BPM)]
    tempo: f64,
    /// Sample rate in Hz.
    #[arg(long, default_value_t = DEFAULT_SAMPLE_RATE)]
    sample_rate: u32,
    /// Maximum process block size.
    #[arg(long, default_value_t = DEFAULT_BLOCK_SIZE)]
    block_size: usize,
    /// Destination WAV path.
    #[arg(long)]
    output: PathBuf,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
    /// Analyze the corrected output audio.
    #[arg(long)]
    analyze: bool,
    /// Trace a compiled Dynamic Parameter; may be repeated.
    #[arg(long = "trace")]
    trace: Vec<String>,
    /// Interval between trace observations in frames.
    #[arg(long = "trace-every-frames")]
    trace_every_frames: Option<usize>,
}

#[derive(Debug, Args)]
struct RenderMidiArgs {
    /// Definition JSON path.
    definition: PathBuf,
    /// Standard MIDI File path.
    midi: PathBuf,
    /// Additional render tail in seconds.
    #[arg(long, default_value_t = 1.0)]
    tail: f64,
    /// Sample rate in Hz.
    #[arg(long, default_value_t = DEFAULT_SAMPLE_RATE)]
    sample_rate: u32,
    /// Maximum process block size.
    #[arg(long, default_value_t = DEFAULT_BLOCK_SIZE)]
    block_size: usize,
    /// Destination WAV path.
    #[arg(long)]
    output: PathBuf,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
    /// Analyze the corrected output audio.
    #[arg(long)]
    analyze: bool,
    /// Trace a compiled Dynamic Parameter; may be repeated.
    #[arg(long = "trace")]
    trace: Vec<String>,
    /// Interval between trace observations in frames.
    #[arg(long = "trace-every-frames")]
    trace_every_frames: Option<usize>,
}

#[derive(Debug, Args)]
struct RenderEventsArgs {
    /// Definition JSON path.
    definition: PathBuf,
    /// Absolute-frame event sequence JSON path.
    events: PathBuf,
    /// Main render duration in frames.
    #[arg(long)]
    duration_frames: u64,
    /// Additional render tail in seconds.
    #[arg(long, default_value_t = 1.0)]
    tail: f64,
    /// Processing tempo in beats per minute.
    #[arg(long, default_value_t = DEFAULT_TEMPO_BPM)]
    tempo: f64,
    /// Sample rate in Hz.
    #[arg(long, default_value_t = DEFAULT_SAMPLE_RATE)]
    sample_rate: u32,
    /// Maximum process block size.
    #[arg(long, default_value_t = DEFAULT_BLOCK_SIZE)]
    block_size: usize,
    /// Destination WAV path.
    #[arg(long)]
    output: PathBuf,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
    /// Analyze the corrected output audio.
    #[arg(long)]
    analyze: bool,
    /// Trace a compiled Dynamic Parameter; may be repeated.
    #[arg(long = "trace")]
    trace: Vec<String>,
    /// Interval between trace observations in frames.
    #[arg(long = "trace-every-frames")]
    trace_every_frames: Option<usize>,
    /// Render the same event sequence again after resetting the prepared runtime.
    #[arg(long)]
    reset_check: bool,
}

#[derive(Debug, Args)]
struct RenderSineArgs {
    /// Oscillator frequency in Hz.
    #[arg(long, default_value_t = 440.0)]
    frequency: f32,
    /// Main render duration in seconds.
    #[arg(long)]
    duration: f64,
    /// Sample rate in Hz.
    #[arg(long, default_value_t = DEFAULT_SAMPLE_RATE)]
    sample_rate: u32,
    /// Maximum process block size.
    #[arg(long, default_value_t = DEFAULT_BLOCK_SIZE)]
    block_size: usize,
    /// Additional render tail in seconds.
    #[arg(long, default_value_t = 0.0)]
    tail: f64,
    /// Destination WAV path.
    #[arg(long)]
    output: PathBuf,
    /// Emit machine-readable JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Serialize)]
struct SuccessReport {
    status: &'static str,
    sample_rate: u32,
    channels: usize,
    frames: usize,
    reported_latency_frames: usize,
    output: String,
    backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    analysis: Option<AudioAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace: Option<RenderTraceReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reset_comparison: Option<ResetComparison>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize)]
struct ResetComparison {
    compatible: bool,
    max_abs_difference: f64,
    rms_difference: f64,
    different_sample_count: usize,
}

#[derive(Debug, Serialize)]
struct StatusReport {
    status: &'static str,
    command: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
struct CliFailure {
    code: u8,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize)]
struct InspectReport {
    status: &'static str,
    name: String,
    metadata: InspectMetadata,
    polyphony: usize,
    voice_stealing: &'static str,
    reported_latency_frames: usize,
    layer_count: usize,
    layers: Vec<InspectLayer>,
    voice_processors: Vec<InspectProcessor>,
    global_processors: Vec<InspectProcessor>,
    parameters: Vec<InspectParameter>,
    sources: Vec<InspectSource>,
    routes: Vec<InspectRoute>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize)]
struct InspectMetadata {
    name: String,
    author: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Serialize)]
struct InspectLayer {
    id: String,
    enabled: bool,
    trigger: InspectTrigger,
    generator: InspectGenerator,
    asset_status: &'static str,
    gain_db: f32,
    gain_linear: f32,
    pan: f32,
    tuning_cents: f32,
    tuning_ratio: f32,
    envelope: InspectEnvelope,
    processors: Vec<InspectProcessor>,
}

#[derive(Debug, Serialize)]
struct InspectTrigger {
    event: &'static str,
    key_min: u8,
    key_max: u8,
    velocity_min: u8,
    velocity_max: u8,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum InspectGenerator {
    Oscillator {
        waveform: &'static str,
        phase_reset: bool,
        phase: f32,
        output_mode: &'static str,
        backend: &'static str,
        hard_sync: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        sync_ratio_parameter: Option<String>,
        waveshaping: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        waveshape_parameter: Option<String>,
        phase_distortion: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        phase_distortion_parameter: Option<String>,
        wavefold: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        wavefold_parameter: Option<String>,
        oscillator_feedback: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        oscillator_feedback_parameter: Option<String>,
        dc_blocker: bool,
        signal_order: &'static str,
        combination_constraints: &'static str,
        unison_voices: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        unison_detune_parameter: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        unison_spread_parameter: Option<String>,
        phase_spread: f32,
        effective_max_frequency_hz: f32,
        #[serde(skip_serializing_if = "Option::is_none")]
        pulse_width: Option<f32>,
    },
    Noise {
        output_mode: &'static str,
        noise_color: &'static str,
        noise_seed: u64,
        noise_correlation_parameter: String,
    },
    PhysicalString {
        output_mode: &'static str,
        exciter: InspectPhysicalExciter,
        decay_seconds: f32,
        decay_parameter: String,
        brightness: f32,
        brightness_parameter: String,
        stiffness: f32,
        stiffness_parameter: String,
        effective_max_frequency_hz: f32,
    },
    Modal {
        output_mode: &'static str,
        exciter: InspectPhysicalExciter,
        mode_count: u8,
        structure: f32,
        structure_parameter: String,
        brightness: f32,
        brightness_parameter: String,
        decay: f32,
        decay_parameter: String,
        effective_max_frequency_hz: f32,
    },
    Additive {
        output_mode: &'static str,
        partial_count: usize,
        max_partial_count: usize,
        phase_reset: bool,
        morph: f32,
        spectrum_tilt_db_per_octave: f32,
        inharmonicity: f32,
        partials: Vec<InspectAdditivePartial>,
    },
    Formant {
        output_mode: &'static str,
        partial_count: usize,
        max_partial_count: usize,
        phase_reset: bool,
        profile_count: usize,
        vowel_position: f32,
        formant_shift_cents: f32,
        throat: f32,
        spectral_tilt_db_per_octave: f32,
        profiles: Vec<InspectFormantProfile>,
    },
    Sample {
        output_mode: &'static str,
        interpolation: &'static str,
        sample_zone_count: usize,
        sample_enabled_zone_count: usize,
        sample_disabled_zone_count: usize,
        sample_asset_count: usize,
        sample_zones: Vec<InspectSampleZone>,
    },
    Granular {
        output_mode: &'static str,
        asset_path: String,
        asset_sha256_specified: bool,
        prepared: bool,
        source_channels: Option<usize>,
        prepared_frames: Option<usize>,
        region_start_frame: usize,
        region_end_frame: usize,
        root_note: u8,
        position: f32,
        position_parameter: String,
        grain_size: f32,
        grain_size_parameter: String,
        density: f32,
        density_parameter: String,
        pitch: f32,
        pitch_parameter: String,
        randomness: f32,
        randomness_parameter: String,
        pan_spread: f32,
        pan_spread_parameter: String,
        seed: u64,
        grain_pool_limit: usize,
    },
    WaveSequence {
        output_mode: &'static str,
        step_count: usize,
        enabled_step_count: usize,
        direction: &'static str,
        loop_sequence: bool,
        crossfade: f32,
        steps: Vec<InspectWaveSequenceStep>,
    },
    Wavetable {
        output_mode: &'static str,
        asset_path: String,
        asset_sha256_specified: bool,
        prepared: bool,
        source_channels: Option<usize>,
        source_frame_count: Option<usize>,
        frame_length: usize,
        frame_count: Option<usize>,
        band_count: Option<usize>,
        band_max_harmonics: Vec<usize>,
        position: f32,
        position_parameter: String,
        phase_reset: bool,
        phase: f32,
        unison_voices: usize,
        unison_detune_parameter: Option<String>,
        unison_spread_parameter: Option<String>,
        effective_max_frequency_hz: f32,
    },
    Spectral {
        output_mode: &'static str,
        asset_a_path: String,
        asset_a_sha256_specified: bool,
        asset_a_prepared: bool,
        asset_b_path: Option<String>,
        asset_b_sha256_specified: bool,
        asset_b_prepared: bool,
        asset_b_source_sample_rate: Option<u32>,
        asset_b_prepared_sample_rate: Option<f64>,
        asset_b_source_channels: Option<usize>,
        asset_b_source_frame_count: Option<usize>,
        asset_b_spectral_frame_count: Option<usize>,
        asset_b_prepared_bytes: Option<usize>,
        source_sample_rate: Option<u32>,
        prepared_sample_rate: Option<f64>,
        source_channels: Option<usize>,
        source_frame_count: Option<usize>,
        spectral_frame_count: Option<usize>,
        prepared_bytes: Option<usize>,
        fft_size: usize,
        hop_size: usize,
        bin_count: usize,
        latency_frames: usize,
        root_note: u8,
        position: f32,
        position_parameter: String,
        freeze: f32,
        freeze_parameter: String,
        blur_seconds: f32,
        blur_parameter: String,
        shift_hz: f32,
        shift_parameter: String,
        morph: Option<f32>,
        morph_parameter: Option<String>,
        phase_reset: bool,
    },
    OperatorModulation {
        output_mode: &'static str,
        mode: &'static str,
        algorithm: &'static str,
        evaluation_order: Vec<usize>,
        incoming_masks: Vec<u8>,
        carrier_operators: Vec<usize>,
        operators: Vec<InspectOperator>,
        phase_reset: bool,
        unison_voices: usize,
        unison_detune_parameter: Option<String>,
        unison_spread_parameter: Option<String>,
        effective_max_frequency_hz: f32,
    },
}

#[derive(Debug, Serialize)]
struct InspectPhysicalExciter {
    kind: &'static str,
    duration_seconds: Option<f32>,
    brightness: Option<f32>,
    seed: Option<u64>,
}

#[derive(Debug, Serialize)]
struct InspectAdditivePartial {
    id: String,
    ratio: f32,
    amplitude_a: f32,
    amplitude_b: f32,
    phase: f32,
    has_envelope: bool,
}

#[derive(Debug, Serialize)]
struct InspectFormantProfile {
    id: String,
    formants: Vec<InspectFormantBand>,
}

#[derive(Debug, Serialize)]
struct InspectFormantBand {
    frequency_hz: f32,
    bandwidth_hz: f32,
    gain_db: f32,
}

#[derive(Debug, Serialize)]
struct InspectOperator {
    index: usize,
    ratio: f32,
    detune_cents: f32,
    level: Option<f32>,
    modulation_amount: Option<f32>,
    feedback: Option<f32>,
    phase: f32,
    envelope: InspectEnvelope,
    ratio_parameter: String,
    detune_parameter: String,
    level_parameter: Option<String>,
    modulation_amount_parameter: Option<String>,
    feedback_parameter: Option<String>,
}

#[derive(Debug, Serialize)]
struct InspectSampleZone {
    id: String,
    enabled: bool,
    asset_path: String,
    root_note: u8,
    key_min: u8,
    key_max: u8,
    velocity_min: u8,
    velocity_max: u8,
    round_robin_group: Option<String>,
    playback_type: &'static str,
    direction: &'static str,
    start_frame: usize,
    end_frame: usize,
    loop_start_frame: Option<usize>,
    loop_end_frame: Option<usize>,
    crossfade_frames: Option<usize>,
    time_mode: &'static str,
    duration_ratio: Option<f64>,
    source_bpm: Option<f64>,
    source_sample_rate: Option<u32>,
    source_channels: Option<usize>,
    prepared_frames: Option<usize>,
}

#[derive(Debug, Serialize)]
struct InspectWaveSequenceStep {
    id: String,
    enabled: bool,
    asset_path: String,
    start_frame: usize,
    end_frame: usize,
    duration_type: &'static str,
    duration: f64,
    playback: &'static str,
    playback_direction: &'static str,
    gain_db: f32,
    gain_linear: f32,
    pitch_cents: f32,
    source_channels: Option<usize>,
    prepared_frames: Option<usize>,
}

#[derive(Debug, Serialize)]
struct InspectEnvelope {
    attack_samples: usize,
    decay_samples: usize,
    sustain_level: f32,
    release_samples: usize,
}

#[derive(Debug, Serialize)]
struct InspectProcessor {
    placement: &'static str,
    chain_index: usize,
    id: String,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<&'static str>,
    static_fields: Vec<InspectStaticField>,
    parameters: Vec<InspectProcessorParameter>,
}

#[derive(Debug, Serialize)]
struct InspectStaticField {
    id: &'static str,
    value: f32,
}

#[derive(Debug, Serialize)]
struct InspectProcessorParameter {
    id: String,
    unit: ParameterUnit,
    min: f32,
    max: f32,
    default: f32,
    scale: ParameterScale,
    smoothing_seconds: f32,
    modulation: InspectModulation,
}

#[derive(Debug, Serialize)]
struct InspectParameter {
    id: String,
    owner: ParameterOwner,
    unit: ParameterUnit,
    min: f32,
    max: f32,
    default: f32,
    scale: ParameterScale,
    smoothing_seconds: f32,
    modulation: InspectModulation,
    #[serde(skip_serializing_if = "Option::is_none")]
    modulated_range_from_default: Option<InspectModulatedRange>,
}

#[derive(Debug, Serialize)]
struct InspectModulation {
    unit: ModulationUnit,
    max_abs_depth: f32,
}

#[derive(Debug, Serialize)]
struct InspectModulatedRange {
    unclamped_min: f32,
    unclamped_max: f32,
    effective_min: f32,
    effective_max: f32,
    may_clamp: bool,
}

#[derive(Debug, Serialize)]
struct InspectSourceRange {
    min: f32,
    max: f32,
    polarity: InspectPolarity,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum InspectPolarity {
    Unipolar,
    Bipolar,
}

#[derive(Debug, Serialize)]
struct InspectRouteEffect {
    kind: &'static str,
    unit: ModulationUnit,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_delta: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_delta: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_octaves: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_octaves: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_factor: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_factor: Option<f32>,
}

#[derive(Debug, Serialize)]
struct InspectSource {
    id: String,
    scope: &'static str,
    kind: &'static str,
    value_range: InspectSourceRange,
    #[serde(skip_serializing_if = "Option::is_none")]
    waveform: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rate_hz: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    phase: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attack_samples: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    decay_samples: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sustain_level: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_samples: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<u64>,
}

#[derive(Debug, Serialize)]
struct InspectRoute {
    source: String,
    target: String,
    depth: InspectDepth,
    curve: ModulationCurve,
    source_range: InspectSourceBounds,
    effect: InspectRouteEffect,
}

#[derive(Debug, Serialize)]
struct InspectDepth {
    value: f32,
    unit: ModulationUnit,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct InspectSourceBounds {
    min: f32,
    max: f32,
}

#[derive(Debug, Deserialize)]
struct EventSequence {
    events: Vec<EventSequenceEntry>,
}

#[derive(Debug, Deserialize)]
struct EventSequenceEntry {
    absolute_frame: u64,
    #[serde(flatten)]
    event: EventSequenceKind,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum EventSequenceKind {
    NoteOn {
        note: u8,
        velocity: u8,
        note_id: u64,
    },
    NoteOff {
        note_id: u64,
    },
    ParameterChange {
        parameter: String,
        native_value: f32,
    },
    PitchBend {
        value: f32,
    },
    ModWheel {
        value: f32,
    },
    Aftertouch {
        value: f32,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Instrument { command } => run_instrument(command),
        Command::Render { command } => match command {
            RenderCommand::Note(args) => run_render_note(&args),
            RenderCommand::Events(args) => run_render_events(&args),
            RenderCommand::Midi(args) => run_render_midi(&args),
        },
        Command::Dev { command } => match command {
            DevCommand::RenderSine(args) => run_render_sine(&args),
        },
    }
}

fn run_instrument(command: InstrumentCommand) -> ExitCode {
    match command {
        InstrumentCommand::Init(args) => run_init(&args),
        InstrumentCommand::Validate(args) => run_validate(&args),
        InstrumentCommand::Inspect(args) => run_inspect(&args),
    }
}

fn run_init(args: &InitArgs) -> ExitCode {
    if args.path.exists() {
        print_failure(
            false,
            &CliFailure {
                code: 2,
                diagnostics: vec![
                    Diagnostic::error(
                        DiagnosticCode::DefinitionError,
                        "destination already exists",
                    )
                    .with_path(args.path.to_string_lossy()),
                ],
            },
        );
        return ExitCode::from(2);
    }
    let json = match serde_json::to_string_pretty(&default_definition()) {
        Ok(json) => json,
        Err(error) => {
            print_failure(
                false,
                &CliFailure {
                    code: 4,
                    diagnostics: vec![
                        Diagnostic::error(
                            DiagnosticCode::DefinitionError,
                            "could not serialize default Definition",
                        )
                        .with_detail(error.to_string()),
                    ],
                },
            );
            return ExitCode::from(4);
        }
    };
    if let Err(error) = std::fs::write(&args.path, format!("{json}\n")) {
        print_failure(
            false,
            &CliFailure {
                code: 4,
                diagnostics: vec![
                    Diagnostic::error(DiagnosticCode::WavOutputError, "could not write Definition")
                        .with_path(args.path.to_string_lossy())
                        .with_detail(error.to_string()),
                ],
            },
        );
        return ExitCode::from(4);
    }
    println!("created {}", args.path.display());
    ExitCode::SUCCESS
}

fn run_validate(args: &DefinitionArgs) -> ExitCode {
    let result = load_and_compile(&args.definition, DEFAULT_SAMPLE_RATE, DEFAULT_BLOCK_SIZE);
    match result {
        Ok((_, diagnostics)) => {
            let report = StatusReport {
                status: "ok",
                command: "instrument validate",
                diagnostics,
            };
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string(&report).expect("status report is serializable")
                );
            } else {
                println!("valid {}", args.definition.display());
                print_warnings(&report.diagnostics);
            }
            ExitCode::SUCCESS
        }
        Err(failure) => {
            print_failure(args.json, &failure);
            ExitCode::from(failure.code)
        }
    }
}

fn run_inspect(args: &DefinitionArgs) -> ExitCode {
    let result = load_and_compile(&args.definition, DEFAULT_SAMPLE_RATE, DEFAULT_BLOCK_SIZE);
    match result {
        Ok((compiled, diagnostics)) => {
            if args.json {
                let report = make_inspect_report(&compiled, diagnostics);
                println!(
                    "{}",
                    serde_json::to_string(&report).expect("inspect report is serializable")
                );
            } else {
                print_inspect(&compiled, &diagnostics);
            }
            ExitCode::SUCCESS
        }
        Err(failure) => {
            print_failure(args.json, &failure);
            ExitCode::from(failure.code)
        }
    }
}

#[allow(clippy::too_many_lines)]
fn run_render_note(args: &RenderNoteArgs) -> ExitCode {
    if args.note > 127 {
        return finish_failure(
            args.json,
            CliFailure {
                code: 2,
                diagnostics: vec![Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "note must be between 0 and 127",
                )],
            },
        );
    }
    if args.velocity == 0 || args.velocity > 127 {
        return finish_failure(
            args.json,
            CliFailure {
                code: 2,
                diagnostics: vec![Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "velocity must be between 1 and 127",
                )],
            },
        );
    }
    if args.gate <= 0.0 {
        return finish_failure(
            args.json,
            CliFailure {
                code: 2,
                diagnostics: vec![Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "gate must be greater than zero",
                )],
            },
        );
    }
    let sample_rate = f64::from(args.sample_rate);
    let gate_frames = match seconds_to_frames(args.gate, sample_rate) {
        Ok(frames) => frames,
        Err(error) => {
            return finish_failure(args.json, input_failure(&error));
        }
    };
    let tail_frames = match seconds_to_frames(args.tail, sample_rate) {
        Ok(frames) => frames,
        Err(error) => {
            return finish_failure(args.json, input_failure(&error));
        }
    };
    let Some(duration_frames) = gate_frames.checked_add(1) else {
        return finish_failure(
            args.json,
            CliFailure {
                code: 2,
                diagnostics: vec![Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "render duration overflows the frame counter",
                )],
            },
        );
    };
    let (compiled, mut diagnostics) =
        match load_and_compile(&args.definition, args.sample_rate, args.block_size) {
            Ok(result) => result,
            Err(failure) => return finish_failure(args.json, failure),
        };
    let trace_request = match resolve_trace_request(&compiled, &args.trace, args.trace_every_frames)
    {
        Ok(request) => request,
        Err(failure) => return finish_failure(args.json, failure),
    };
    let events = [
        ScheduledEvent {
            absolute_frame: 0,
            kind: sonalloy_core::ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: args.note,
                velocity: args.velocity,
            },
        },
        ScheduledEvent {
            absolute_frame: gate_frames,
            kind: sonalloy_core::ProcessEventKind::NoteOff { note_id: 1 },
        },
    ];
    let request = RenderRequest {
        sample_rate,
        block_size: args.block_size,
        duration_frames,
        tail_frames,
    };
    let request = match extend_request_for_latency(request, compiled.reported_latency_frames) {
        Ok(request) => request,
        Err(failure) => return finish_failure(args.json, failure),
    };
    let (mut audio, trace) = if let Some(trace_request) = trace_request.as_ref() {
        let tempo_map = match TempoMap::constant(args.tempo) {
            Ok(tempo_map) => tempo_map,
            Err(error) => return finish_failure(args.json, render_failure(&error)),
        };
        match render_instrument_with_trace(
            Arc::clone(&compiled),
            request,
            &events,
            &tempo_map,
            trace_request,
        ) {
            Ok((audio, trace)) => (audio, Some(trace)),
            Err(error) => return finish_failure(args.json, render_failure(&error)),
        }
    } else {
        match render_instrument_with_tempo(Arc::clone(&compiled), request, &events, args.tempo) {
            Ok(audio) => (audio, None),
            Err(error) => return finish_failure(args.json, render_failure(&error)),
        }
    };
    correct_rendered_audio(&mut audio, compiled.reported_latency_frames);
    let analysis = if args.analyze {
        match analyze_audio(&audio, Some(note_frequency_hz(args.note))) {
            Ok(analysis) => Some(analysis),
            Err(failure) => return finish_failure(args.json, failure),
        }
    } else {
        None
    };
    if let Err(error) = write_wav(&args.output, &audio) {
        return finish_failure(
            args.json,
            CliFailure {
                code: 4,
                diagnostics: vec![error],
            },
        );
    }
    print_success(
        args.json,
        SuccessReport {
            status: "ok",
            sample_rate: audio.sample_rate,
            channels: audio.channels.len(),
            frames: audio.frames(),
            reported_latency_frames: compiled.reported_latency_frames,
            output: args.output.to_string_lossy().into_owned(),
            backend: backend_info().version,
            diagnostics: std::mem::take(&mut diagnostics),
            analysis,
            trace,
            reset_comparison: None,
        },
    )
}

fn run_render_events(args: &RenderEventsArgs) -> ExitCode {
    let sample_rate = f64::from(args.sample_rate);
    let tail_frames = match seconds_to_frames(args.tail, sample_rate) {
        Ok(frames) => frames,
        Err(error) => return finish_failure(args.json, input_failure(&error)),
    };
    let (compiled, diagnostics) =
        match load_and_compile(&args.definition, args.sample_rate, args.block_size) {
            Ok(result) => result,
            Err(failure) => return finish_failure(args.json, failure),
        };
    let trace_request = match resolve_trace_request(&compiled, &args.trace, args.trace_every_frames)
    {
        Ok(request) => request,
        Err(failure) => return finish_failure(args.json, failure),
    };
    let sequence = match load_event_sequence(&args.events) {
        Ok(sequence) => sequence,
        Err(failure) => return finish_failure(args.json, failure),
    };
    let events = match compile_event_sequence(&sequence, &compiled, args.duration_frames) {
        Ok(events) => events,
        Err(failure) => return finish_failure(args.json, failure),
    };
    let request = RenderRequest {
        sample_rate,
        block_size: args.block_size,
        duration_frames: args.duration_frames,
        tail_frames,
    };
    let request = match extend_request_for_latency(request, compiled.reported_latency_frames) {
        Ok(request) => request,
        Err(failure) => return finish_failure(args.json, failure),
    };
    let tempo_map = match TempoMap::constant(args.tempo) {
        Ok(tempo_map) => tempo_map,
        Err(error) => return finish_failure(args.json, render_failure(&error)),
    };
    let (mut audio, trace, reset_comparison) = match render_event_audio(
        &compiled,
        request,
        &events,
        &tempo_map,
        trace_request.as_ref(),
        args.reset_check,
    ) {
        Ok(rendered) => rendered,
        Err(failure) => return finish_failure(args.json, failure),
    };
    correct_rendered_audio(&mut audio, compiled.reported_latency_frames);
    let analysis = if args.analyze {
        match analyze_audio(&audio, None) {
            Ok(analysis) => Some(analysis),
            Err(failure) => return finish_failure(args.json, failure),
        }
    } else {
        None
    };
    if let Err(error) = write_wav(&args.output, &audio) {
        return finish_failure(
            args.json,
            CliFailure {
                code: 4,
                diagnostics: vec![error],
            },
        );
    }
    print_success(
        args.json,
        SuccessReport {
            status: "ok",
            sample_rate: audio.sample_rate,
            channels: audio.channels.len(),
            frames: audio.frames(),
            reported_latency_frames: compiled.reported_latency_frames,
            output: args.output.to_string_lossy().into_owned(),
            backend: backend_info().version,
            diagnostics,
            analysis,
            trace,
            reset_comparison,
        },
    )
}

fn render_event_audio(
    compiled: &Arc<CompiledInstrument>,
    request: RenderRequest,
    events: &[ScheduledEvent],
    tempo_map: &TempoMap,
    trace_request: Option<&TraceRequest>,
    reset_check: bool,
) -> Result<
    (
        sonalloy_core::RenderedAudio,
        Option<RenderTraceReport>,
        Option<ResetComparison>,
    ),
    CliFailure,
> {
    if reset_check {
        if trace_request.is_some() {
            return Err(CliFailure {
                code: 2,
                diagnostics: vec![Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "--reset-check cannot be combined with --trace",
                )],
            });
        }
        let (first, second) =
            render_instrument_with_reset(Arc::clone(compiled), request, events, tempo_map)
                .map_err(|error| render_failure(&error))?;
        let comparison = compare_rendered_audio(&first, &second);
        return Ok((second, None, Some(comparison)));
    }
    if let Some(trace_request) = trace_request {
        let (audio, trace) = render_instrument_with_trace(
            Arc::clone(compiled),
            request,
            events,
            tempo_map,
            trace_request,
        )
        .map_err(|error| render_failure(&error))?;
        return Ok((audio, Some(trace), None));
    }
    let audio = render_instrument_with_tempo_map(Arc::clone(compiled), request, events, tempo_map)
        .map_err(|error| render_failure(&error))?;
    Ok((audio, None, None))
}

fn load_event_sequence(path: &Path) -> Result<EventSequence, CliFailure> {
    let text = std::fs::read_to_string(path).map_err(|error| CliFailure {
        code: 2,
        diagnostics: vec![
            Diagnostic::error(
                DiagnosticCode::DefinitionError,
                "could not read event input",
            )
            .with_path(path.to_string_lossy())
            .with_detail(error.to_string()),
        ],
    })?;
    serde_json::from_str(&text).map_err(|error| CliFailure {
        code: 1,
        diagnostics: vec![
            Diagnostic::error(DiagnosticCode::JsonInvalid, "could not parse event JSON")
                .with_path(path.to_string_lossy())
                .with_detail(format!(
                    "line {}, column {}: {error}",
                    error.line(),
                    error.column()
                )),
        ],
    })
}

#[allow(clippy::too_many_lines)]
fn compile_event_sequence(
    sequence: &EventSequence,
    compiled: &CompiledInstrument,
    duration_frames: u64,
) -> Result<Vec<ScheduledEvent>, CliFailure> {
    let mut diagnostics = Vec::new();
    let mut events = Vec::with_capacity(sequence.events.len());
    let mut previous_absolute_frame = None;
    for (index, entry) in sequence.events.iter().enumerate() {
        let event_path = format!("events[{index}]");
        if previous_absolute_frame.is_some_and(|previous| entry.absolute_frame < previous) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::EventOrderInvalid,
                    "event absolute_frame values must be in ascending order",
                )
                .with_path(format!("{event_path}.absolute_frame")),
            );
        }
        previous_absolute_frame = Some(entry.absolute_frame);
        if entry.absolute_frame >= duration_frames {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "event frame must be less than duration_frames",
                )
                .with_path(format!("{event_path}.absolute_frame")),
            );
            continue;
        }
        let kind = match &entry.event {
            EventSequenceKind::NoteOn {
                note,
                velocity,
                note_id,
            } => {
                if *note > 127 {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "note must be between 0 and 127",
                        )
                        .with_path(format!("{event_path}.note")),
                    );
                }
                if *velocity == 0 || *velocity > 127 {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "velocity must be between 1 and 127",
                        )
                        .with_path(format!("{event_path}.velocity")),
                    );
                }
                ProcessEventKind::NoteOn {
                    note_id: *note_id,
                    note_number: *note,
                    velocity: *velocity,
                }
            }
            EventSequenceKind::NoteOff { note_id } => {
                ProcessEventKind::NoteOff { note_id: *note_id }
            }
            EventSequenceKind::ParameterChange {
                parameter,
                native_value,
            } => {
                let Some(handle) = compiled.parameter_handle(parameter) else {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ParameterNotFound,
                            "parameter id is not present in the compiled catalog",
                        )
                        .with_path(format!("{event_path}.parameter")),
                    );
                    continue;
                };
                let descriptor = compiled
                    .parameter_descriptor(handle)
                    .expect("parameter handle was resolved from the catalog");
                let normalized = match descriptor.normalize(*native_value) {
                    Ok(normalized) => normalized,
                    Err(error) => {
                        diagnostics.push(
                            Diagnostic::error(DiagnosticCode::ValueOutOfRange, error.to_string())
                                .with_path(format!("{event_path}.native_value")),
                        );
                        0.0
                    }
                };
                ProcessEventKind::ParameterChange {
                    parameter: handle,
                    normalized,
                }
            }
            EventSequenceKind::PitchBend { value } => {
                if !value.is_finite() || !(-1.0..=1.0).contains(value) {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "pitch bend value must be finite and between -1 and 1",
                        )
                        .with_path(format!("{event_path}.value")),
                    );
                }
                ProcessEventKind::PitchBend { value: *value }
            }
            EventSequenceKind::ModWheel { value } => {
                if !value.is_finite() || !(0.0..=1.0).contains(value) {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "mod wheel value must be finite and between 0 and 1",
                        )
                        .with_path(format!("{event_path}.value")),
                    );
                }
                ProcessEventKind::ModWheel { value: *value }
            }
            EventSequenceKind::Aftertouch { value } => {
                if !value.is_finite() || !(0.0..=1.0).contains(value) {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "aftertouch value must be finite and between 0 and 1",
                        )
                        .with_path(format!("{event_path}.value")),
                    );
                }
                ProcessEventKind::Aftertouch { value: *value }
            }
        };
        events.push((
            index,
            ScheduledEvent {
                absolute_frame: entry.absolute_frame,
                kind,
            },
        ));
    }
    if !diagnostics.is_empty() {
        return Err(CliFailure {
            code: 2,
            diagnostics,
        });
    }
    events.sort_by_key(|(index, event)| (event.absolute_frame, event.kind.priority(), *index));
    Ok(events.into_iter().map(|(_, event)| event).collect())
}

fn run_render_midi(args: &RenderMidiArgs) -> ExitCode {
    let sample_rate = f64::from(args.sample_rate);
    let tail_frames = match seconds_to_frames(args.tail, sample_rate) {
        Ok(frames) => frames,
        Err(error) => return finish_failure(args.json, input_failure(&error)),
    };
    let (compiled, mut diagnostics) =
        match load_and_compile(&args.definition, args.sample_rate, args.block_size) {
            Ok(result) => result,
            Err(failure) => return finish_failure(args.json, failure),
        };
    let trace_request = match resolve_trace_request(&compiled, &args.trace, args.trace_every_frames)
    {
        Ok(request) => request,
        Err(failure) => return finish_failure(args.json, failure),
    };
    let midi = match read_midi(&args.midi, sample_rate) {
        Ok(midi) => midi,
        Err(midi_diagnostics) => {
            return finish_failure(
                args.json,
                CliFailure {
                    code: 2,
                    diagnostics: midi_diagnostics,
                },
            );
        }
    };
    diagnostics.extend(midi.diagnostics);
    let request = RenderRequest {
        sample_rate,
        block_size: args.block_size,
        duration_frames: midi.duration_frames,
        tail_frames,
    };
    let request = match extend_request_for_latency(request, compiled.reported_latency_frames) {
        Ok(request) => request,
        Err(failure) => return finish_failure(args.json, failure),
    };
    let (mut audio, trace) = if let Some(trace_request) = trace_request.as_ref() {
        match render_instrument_with_trace(
            Arc::clone(&compiled),
            request,
            &midi.events,
            &midi.tempo_map,
            trace_request,
        ) {
            Ok((audio, trace)) => (audio, Some(trace)),
            Err(error) => return finish_failure(args.json, render_failure(&error)),
        }
    } else {
        match render_instrument_with_tempo_map(
            Arc::clone(&compiled),
            request,
            &midi.events,
            &midi.tempo_map,
        ) {
            Ok(audio) => (audio, None),
            Err(error) => return finish_failure(args.json, render_failure(&error)),
        }
    };
    correct_rendered_audio(&mut audio, compiled.reported_latency_frames);
    let analysis = if args.analyze {
        match analyze_audio(&audio, None) {
            Ok(analysis) => Some(analysis),
            Err(failure) => return finish_failure(args.json, failure),
        }
    } else {
        None
    };
    if let Err(error) = write_wav(&args.output, &audio) {
        return finish_failure(
            args.json,
            CliFailure {
                code: 4,
                diagnostics: vec![error],
            },
        );
    }
    print_success(
        args.json,
        SuccessReport {
            status: "ok",
            sample_rate: audio.sample_rate,
            channels: audio.channels.len(),
            frames: audio.frames(),
            reported_latency_frames: compiled.reported_latency_frames,
            output: args.output.to_string_lossy().into_owned(),
            backend: backend_info().version,
            diagnostics,
            analysis,
            trace,
            reset_comparison: None,
        },
    )
}

fn run_render_sine(args: &RenderSineArgs) -> ExitCode {
    match render_sine_command(args) {
        Ok(report) => print_success(args.json, report),
        Err(failure) => finish_failure(args.json, failure),
    }
}

fn render_sine_command(args: &RenderSineArgs) -> Result<SuccessReport, CliFailure> {
    if !args.frequency.is_finite() || args.frequency < 0.0 {
        return Err(CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "frequency must be finite and non-negative",
            )],
        });
    }
    let sample_rate = f64::from(args.sample_rate);
    let duration_frames =
        seconds_to_frames(args.duration, sample_rate).map_err(|error| input_failure(&error))?;
    let tail_frames =
        seconds_to_frames(args.tail, sample_rate).map_err(|error| input_failure(&error))?;
    let request = RenderRequest {
        sample_rate,
        block_size: args.block_size,
        duration_frames,
        tail_frames,
    };
    let audio = render_sine(args.frequency, request).map_err(|error| render_failure(&error))?;
    write_wav(&args.output, &audio).map_err(|error| CliFailure {
        code: 4,
        diagnostics: vec![error],
    })?;
    Ok(SuccessReport {
        status: "ok",
        sample_rate: audio.sample_rate,
        channels: audio.channels.len(),
        frames: audio.frames(),
        reported_latency_frames: 0,
        output: args.output.to_string_lossy().into_owned(),
        backend: backend_info().version,
        diagnostics: Vec::new(),
        analysis: None,
        trace: None,
        reset_comparison: None,
    })
}

fn compare_rendered_audio(
    first: &sonalloy_core::RenderedAudio,
    second: &sonalloy_core::RenderedAudio,
) -> ResetComparison {
    let compatible = first.sample_rate == second.sample_rate
        && first.channels.len() == second.channels.len()
        && first
            .channels
            .iter()
            .zip(&second.channels)
            .all(|(left, right)| left.len() == right.len());
    if !compatible {
        return ResetComparison {
            compatible: false,
            max_abs_difference: 0.0,
            rms_difference: 0.0,
            different_sample_count: 0,
        };
    }
    let mut max_abs_difference = 0.0_f64;
    let mut squared_sum = 0.0_f64;
    let mut sample_count = 0_usize;
    let mut different_sample_count = 0_usize;
    for (first_channel, second_channel) in first.channels.iter().zip(&second.channels) {
        for (first, second) in first_channel.iter().zip(second_channel) {
            let difference = f64::from(*first) - f64::from(*second);
            max_abs_difference = max_abs_difference.max(difference.abs());
            squared_sum += difference * difference;
            sample_count += 1;
            if difference != 0.0 {
                different_sample_count += 1;
            }
        }
    }
    ResetComparison {
        compatible: true,
        max_abs_difference,
        rms_difference: if sample_count == 0 {
            0.0
        } else {
            #[allow(clippy::cast_precision_loss)]
            let sample_count = sample_count as f64;
            (squared_sum / sample_count).sqrt()
        },
        different_sample_count,
    }
}

fn resolve_trace_request(
    compiled: &CompiledInstrument,
    ids: &[String],
    every_frames: Option<usize>,
) -> Result<Option<TraceRequest>, CliFailure> {
    if ids.is_empty() {
        if every_frames.is_some() {
            return Err(CliFailure {
                code: 2,
                diagnostics: vec![Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "--trace-every-frames requires at least one --trace parameter",
                )],
            });
        }
        return Ok(None);
    }
    let every_frames = every_frames.unwrap_or(480);
    if every_frames == 0 {
        return Err(CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "--trace-every-frames must be greater than zero",
            )],
        });
    }
    let mut parameters = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(handle) = compiled.parameter_handle(id) else {
            return Err(CliFailure {
                code: 2,
                diagnostics: vec![
                    Diagnostic::error(
                        DiagnosticCode::ParameterNotFound,
                        "trace parameter id is not present in the compiled catalog",
                    )
                    .with_path("--trace")
                    .with_detail(id),
                ],
            });
        };
        if !parameters.contains(&handle) {
            parameters.push(handle);
        }
    }
    Ok(Some(TraceRequest {
        parameters,
        every_frames,
    }))
}

fn analyze_audio(
    audio: &sonalloy_core::RenderedAudio,
    reference_frequency_hz: Option<f32>,
) -> Result<AudioAnalysis, CliFailure> {
    analyze_rendered_audio(
        audio,
        AudioAnalysisOptions {
            reference_frequency_hz,
        },
    )
    .map_err(|error| CliFailure {
        code: 3,
        diagnostics: vec![
            Diagnostic::error(DiagnosticCode::RenderError, "audio analysis failed")
                .with_detail(error.to_string()),
        ],
    })
}

fn note_frequency_hz(note: u8) -> f32 {
    440.0_f32 * 2.0_f32.powf((f32::from(note) - 69.0) / 12.0)
}

fn extend_request_for_latency(
    request: RenderRequest,
    latency_frames: usize,
) -> Result<RenderRequest, CliFailure> {
    let latency_frames = u64::try_from(latency_frames).map_err(|_| CliFailure {
        code: 2,
        diagnostics: vec![Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            "reported latency does not fit the render frame counter",
        )],
    })?;
    let duration_frames = request
        .duration_frames
        .checked_add(latency_frames)
        .ok_or_else(|| CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "render duration including reported latency overflows the frame counter",
            )],
        })?;
    Ok(RenderRequest {
        duration_frames,
        ..request
    })
}

fn correct_rendered_audio(audio: &mut sonalloy_core::RenderedAudio, latency_frames: usize) {
    for channel in &mut audio.channels {
        if latency_frames >= channel.len() {
            channel.clear();
        } else {
            channel.drain(..latency_frames);
        }
    }
}

fn load_and_compile(
    path: &Path,
    sample_rate: u32,
    block_size: usize,
) -> Result<(Arc<CompiledInstrument>, Vec<Diagnostic>), CliFailure> {
    let text = std::fs::read_to_string(path).map_err(|error| CliFailure {
        code: 2,
        diagnostics: vec![
            Diagnostic::error(
                DiagnosticCode::DefinitionError,
                "could not read Definition input",
            )
            .with_path(path.to_string_lossy())
            .with_detail(error.to_string()),
        ],
    })?;
    let definition: InstrumentDefinition =
        serde_json::from_str(&text).map_err(|error| CliFailure {
            code: 1,
            diagnostics: vec![
                Diagnostic::error(
                    if error.to_string().starts_with("missing field") {
                        DiagnosticCode::RequiredFieldMissing
                    } else {
                        DiagnosticCode::JsonInvalid
                    },
                    "could not parse Definition JSON",
                )
                .with_path(path.to_string_lossy())
                .with_detail(format!(
                    "line {}, column {}: {error}",
                    error.line(),
                    error.column()
                )),
            ],
        })?;
    let process_spec =
        ProcessSpec::new(f64::from(sample_rate), block_size, 2).map_err(|error| CliFailure {
            code: 2,
            diagnostics: vec![
                Diagnostic::error(DiagnosticCode::ValueOutOfRange, error.to_string())
                    .with_path("process_spec"),
            ],
        })?;
    let base_dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir.to_path_buf(),
            process_spec,
        },
    );
    let Some(instrument) = result.instrument else {
        return Err(CliFailure {
            code: 1,
            diagnostics: result.diagnostics,
        });
    };
    Ok((instrument, result.diagnostics))
}

fn default_definition() -> InstrumentDefinition {
    InstrumentDefinition {
        schema_version: sonalloy_core::CURRENT_SCHEMA_VERSION,
        metadata: InstrumentMetadata {
            name: "Basic Poly Synth".to_owned(),
            author: None,
            description: Some("A headless oscillator instrument".to_owned()),
        },
        performance: PerformanceDefinition {
            polyphony: 16,
            voice_stealing: VoiceStealingDefinition::QuietestReleasingThenOldest,
        },
        layers: vec![LayerDefinition {
            id: "body".to_owned(),
            enabled: true,
            trigger: LayerTriggerDefinition {
                event: sonalloy_core::LayerTriggerEvent::NoteOn,
                key_min: 0,
                key_max: 127,
                velocity_min: 1,
                velocity_max: 127,
            },
            gain_db: -14.0,
            pan: 0.0,
            tuning_cents: 0.0,
            envelope: AdsrDefinition {
                attack_seconds: 0.005,
                decay_seconds: 0.18,
                sustain_level: 0.65,
                release_seconds: 0.3,
            },
            generator: sonalloy_core::GeneratorDefinition::Oscillator(OscillatorDefinition {
                waveform: OscillatorWaveform::Saw,
                phase_reset: true,
                phase: 0.0,
                hard_sync: None,
                waveshaping: None,
                phase_distortion: None,
                wavefold: None,
                feedback: None,
                unison: None,
            }),
            processors: Vec::new(),
        }],
        voice_processors: vec![ProcessorDefinition::Filter(
            sonalloy_core::FilterProcessorDefinition {
                id: "tone".to_owned(),
                mode: sonalloy_core::FilterModeDefinition::LowPass,
                cutoff_hz: 12_000.0,
                resonance: 0.12,
            },
        )],
        global_processors: Vec::new(),
        modulation: None,
    }
}

fn parameter_default(compiled: &CompiledInstrument, handle: ParameterHandle) -> f32 {
    compiled
        .parameter_descriptor(handle)
        .expect("compiled parameter handle must be valid")
        .default
}

fn parameter_descriptor_id(compiled: &CompiledInstrument, handle: ParameterHandle) -> String {
    compiled
        .parameter_descriptor(handle)
        .expect("compiled parameter handle must be valid")
        .id
        .clone()
}

#[allow(clippy::too_many_lines)]
fn inspect_processor(
    compiled: &CompiledInstrument,
    processor: &sonalloy_core::compiler::CompiledProcessor,
    placement: &'static str,
    chain_index: usize,
) -> InspectProcessor {
    let (kind, static_fields, handles) = match &processor.processor {
        sonalloy_core::compiler::CompiledProcessorKind::Filter(value) => (
            "filter",
            vec![InspectStaticField {
                id: "effective_max_cutoff_hz",
                value: value.effective_max_cutoff_hz,
            }],
            vec![value.parameters.cutoff, value.parameters.resonance],
        ),
        sonalloy_core::compiler::CompiledProcessorKind::Drive(value) => {
            ("drive", Vec::new(), vec![value.amount, value.mix])
        }
        sonalloy_core::compiler::CompiledProcessorKind::Eq(value) => (
            "eq",
            vec![
                InspectStaticField {
                    id: "low_frequency_hz",
                    value: value.low_frequency_hz,
                },
                InspectStaticField {
                    id: "mid_frequency_hz",
                    value: value.mid_frequency_hz,
                },
                InspectStaticField {
                    id: "mid_q",
                    value: value.mid_q,
                },
                InspectStaticField {
                    id: "high_frequency_hz",
                    value: value.high_frequency_hz,
                },
            ],
            vec![
                value.parameters.low_gain_db,
                value.parameters.mid_gain_db,
                value.parameters.high_gain_db,
            ],
        ),
        sonalloy_core::compiler::CompiledProcessorKind::Resonator(value) => (
            "resonator",
            vec![InspectStaticField {
                id: "max_delay_frames",
                #[allow(clippy::cast_precision_loss)]
                value: value.max_delay_frames as f32,
            }],
            vec![
                value.parameters.frequency_hz,
                value.parameters.decay_seconds,
                value.parameters.damping,
                value.parameters.mix,
            ],
        ),
        sonalloy_core::compiler::CompiledProcessorKind::Bitcrusher(value) => (
            "bitcrusher",
            Vec::new(),
            vec![
                value.parameters.bit_depth,
                value.parameters.sample_rate_ratio,
                value.parameters.mix,
            ],
        ),
        sonalloy_core::compiler::CompiledProcessorKind::Chorus(value) => (
            "chorus",
            vec![InspectStaticField {
                id: "delay_frames",
                value: value.delay_frames,
            }],
            vec![
                value.parameters.rate_hz,
                value.parameters.depth,
                value.parameters.feedback,
                value.parameters.width,
                value.parameters.mix,
            ],
        ),
        sonalloy_core::compiler::CompiledProcessorKind::Flanger(value) => (
            "flanger",
            vec![InspectStaticField {
                id: "delay_frames",
                value: value.delay_frames,
            }],
            vec![
                value.parameters.rate_hz,
                value.parameters.depth,
                value.parameters.feedback,
                value.parameters.width,
                value.parameters.mix,
            ],
        ),
        sonalloy_core::compiler::CompiledProcessorKind::Phaser(value) => (
            "phaser",
            vec![
                InspectStaticField {
                    id: "stages",
                    value: f32::from(value.stages),
                },
                InspectStaticField {
                    id: "center_hz",
                    value: value.center_hz,
                },
                InspectStaticField {
                    id: "sweep_octaves",
                    value: value.sweep_octaves,
                },
            ],
            vec![
                value.parameters.rate_hz,
                value.parameters.depth,
                value.parameters.feedback,
                value.parameters.width,
                value.parameters.mix,
            ],
        ),
        sonalloy_core::compiler::CompiledProcessorKind::Delay(value) => (
            "delay",
            vec![InspectStaticField {
                id: "time_frames",
                #[allow(clippy::cast_precision_loss)]
                value: value.delay_frames as f32,
            }],
            vec![value.feedback, value.mix],
        ),
        sonalloy_core::compiler::CompiledProcessorKind::Reverb(value) => (
            "reverb",
            vec![InspectStaticField {
                id: "pre_delay_frames",
                #[allow(clippy::cast_precision_loss)]
                value: value.pre_delay_frames as f32,
            }],
            vec![value.decay, value.damping, value.width, value.mix],
        ),
        sonalloy_core::compiler::CompiledProcessorKind::Compressor(value) => (
            "compressor",
            vec![
                InspectStaticField {
                    id: "attack_coeff",
                    value: value.attack_coeff,
                },
                InspectStaticField {
                    id: "release_coeff",
                    value: value.release_coeff,
                },
                InspectStaticField {
                    id: "knee_db",
                    value: value.knee_db,
                },
            ],
            vec![
                value.parameters.threshold_db,
                value.parameters.ratio,
                value.parameters.makeup_gain_db,
                value.parameters.mix,
            ],
        ),
        sonalloy_core::compiler::CompiledProcessorKind::Limiter(value) => (
            "limiter",
            vec![InspectStaticField {
                id: "release_coeff",
                value: value.release_coeff,
            }],
            vec![value.parameters.ceiling_db, value.parameters.input_gain_db],
        ),
    };
    let mode = match &processor.processor {
        sonalloy_core::compiler::CompiledProcessorKind::Filter(value) => Some(match value.mode {
            sonalloy_core::FilterModeDefinition::LowPass => "low_pass",
            sonalloy_core::FilterModeDefinition::HighPass => "high_pass",
            sonalloy_core::FilterModeDefinition::BandPass => "band_pass",
            sonalloy_core::FilterModeDefinition::Notch => "notch",
        }),
        _ => None,
    };
    InspectProcessor {
        placement,
        chain_index,
        id: processor.id.clone(),
        kind,
        mode,
        static_fields,
        parameters: handles
            .into_iter()
            .map(|handle| {
                let descriptor = compiled
                    .parameter_descriptor(handle)
                    .expect("processor parameter handle must be valid");
                InspectProcessorParameter {
                    id: descriptor.id.clone(),
                    unit: descriptor.unit,
                    min: descriptor.min,
                    max: descriptor.max,
                    default: descriptor.default,
                    scale: descriptor.scale,
                    smoothing_seconds: descriptor.smoothing_seconds,
                    modulation: inspect_modulation(descriptor),
                }
            })
            .collect(),
    }
}

fn inspect_source(source: &sonalloy_core::compiler::CompiledSource) -> InspectSource {
    let mut result = InspectSource {
        id: source.id.clone(),
        scope: "voice",
        kind: "unknown",
        value_range: inspect_source_range("unknown"),
        waveform: None,
        rate_hz: None,
        phase: None,
        attack_samples: None,
        decay_samples: None,
        sustain_level: None,
        release_samples: None,
        seed: None,
    };
    match &source.source {
        sonalloy_core::compiler::CompiledVoiceSource::Velocity => result.kind = "velocity",
        sonalloy_core::compiler::CompiledVoiceSource::KeyTracking => {
            result.kind = "key_tracking";
        }
        sonalloy_core::compiler::CompiledVoiceSource::Lfo(value) => {
            result.kind = "lfo";
            result.waveform = Some(match value.waveform {
                sonalloy_core::LfoWaveform::Sine => "sine",
                sonalloy_core::LfoWaveform::Triangle => "triangle",
            });
            result.rate_hz = Some(value.rate_hz);
            result.phase = Some(value.phase);
        }
        sonalloy_core::compiler::CompiledVoiceSource::Envelope(value) => {
            result.kind = "envelope";
            result.attack_samples = Some(value.envelope.attack_samples);
            result.decay_samples = Some(value.envelope.decay_samples);
            result.sustain_level = Some(value.envelope.sustain_level);
            result.release_samples = Some(value.envelope.release_samples);
        }
        sonalloy_core::compiler::CompiledVoiceSource::Random(value) => {
            result.kind = "random";
            result.seed = Some(value.seed);
        }
    }
    result.value_range = inspect_source_range(result.kind);
    result
}

fn source_id(
    compiled: &CompiledInstrument,
    source: sonalloy_core::compiler::CompiledSourceRef,
) -> String {
    match source {
        sonalloy_core::compiler::CompiledSourceRef::Voice(handle) => compiled
            .sources
            .get(handle.index())
            .expect("compiled voice source handle must be valid")
            .id
            .clone(),
        sonalloy_core::compiler::CompiledSourceRef::PitchBend => "pitch_bend".to_owned(),
        sonalloy_core::compiler::CompiledSourceRef::ModWheel => "mod_wheel".to_owned(),
        sonalloy_core::compiler::CompiledSourceRef::Aftertouch => "aftertouch".to_owned(),
    }
}

fn external_source_name(
    source: sonalloy_core::compiler::CompiledSourceRef,
) -> Option<&'static str> {
    match source {
        sonalloy_core::compiler::CompiledSourceRef::PitchBend => Some("pitch_bend"),
        sonalloy_core::compiler::CompiledSourceRef::ModWheel => Some("mod_wheel"),
        sonalloy_core::compiler::CompiledSourceRef::Aftertouch => Some("aftertouch"),
        sonalloy_core::compiler::CompiledSourceRef::Voice(_) => None,
    }
}

fn inspect_external_source(id: &'static str) -> InspectSource {
    InspectSource {
        id: id.to_owned(),
        scope: "instrument",
        kind: "external_control",
        value_range: inspect_source_range(id),
        waveform: None,
        rate_hz: None,
        phase: None,
        attack_samples: None,
        decay_samples: None,
        sustain_level: None,
        release_samples: None,
        seed: None,
    }
}

fn inspect_source_range(kind: &str) -> InspectSourceRange {
    let (min, max, polarity) = match kind {
        "velocity" | "envelope" | "mod_wheel" | "aftertouch" => {
            (0.0, 1.0, InspectPolarity::Unipolar)
        }
        "key_tracking" | "lfo" | "random" | "pitch_bend" => (-1.0, 1.0, InspectPolarity::Bipolar),
        _ => (0.0, 0.0, InspectPolarity::Unipolar),
    };
    InspectSourceRange { min, max, polarity }
}

fn inspect_modulation(descriptor: &sonalloy_core::ParameterDescriptor) -> InspectModulation {
    InspectModulation {
        unit: descriptor.modulation_unit(),
        max_abs_depth: descriptor.max_modulation_depth(),
    }
}

fn inspect_sample_zones(
    sample: &sonalloy_core::compiler::CompiledSample,
) -> (Vec<InspectSampleZone>, usize) {
    let mut unique_sources: Vec<Arc<sonalloy_core::PreparedAudio>> = Vec::new();
    let zones = sample
        .zones
        .iter()
        .map(|zone| {
            let metadata = zone.source.as_ref().map(|source| {
                if !unique_sources
                    .iter()
                    .any(|candidate| Arc::ptr_eq(candidate, source))
                {
                    unique_sources.push(Arc::clone(source));
                }
                &source.source_metadata
            });
            let playback_type = if zone.playback.loop_region.is_some() {
                "loop"
            } else {
                "one_shot"
            };
            let direction = match zone.playback.direction {
                sonalloy_core::compiler::CompiledSampleDirection::Forward => "forward",
                sonalloy_core::compiler::CompiledSampleDirection::Reverse => "reverse",
            };
            let (time_mode, duration_ratio, source_bpm) = match zone.playback.time {
                sonalloy_core::compiler::CompiledSampleTime::Resample => ("resample", None, None),
                sonalloy_core::compiler::CompiledSampleTime::FixedStretch { duration_ratio } => {
                    ("fixed_stretch", Some(duration_ratio), None)
                }
                sonalloy_core::compiler::CompiledSampleTime::TempoSync { source_bpm } => {
                    ("tempo_sync", None, Some(source_bpm))
                }
            };
            InspectSampleZone {
                id: zone.id.clone(),
                enabled: zone.is_enabled(),
                asset_path: zone.asset_path.clone(),
                root_note: zone.root_note,
                key_min: zone.key_min,
                key_max: zone.key_max,
                velocity_min: zone.velocity_min,
                velocity_max: zone.velocity_max,
                round_robin_group: zone
                    .group
                    .and_then(|index| sample.groups.get(index))
                    .map(|group| group.id.clone()),
                playback_type,
                direction,
                start_frame: zone.playback.start_frame,
                end_frame: zone.playback.end_frame,
                loop_start_frame: zone.playback.loop_region.map(|value| value.start_frame),
                loop_end_frame: zone.playback.loop_region.map(|value| value.end_frame),
                crossfade_frames: zone
                    .playback
                    .loop_region
                    .map(|value| value.crossfade_frames),
                time_mode,
                duration_ratio,
                source_bpm,
                source_sample_rate: metadata.map(|value| value.source_sample_rate),
                source_channels: metadata.map(|value| value.source_channels),
                prepared_frames: zone.source.as_ref().map(|source| source.frames),
            }
        })
        .collect();
    (zones, unique_sources.len())
}

fn output_mode_name(mode: sonalloy_core::compiler::GeneratorOutputMode) -> &'static str {
    match mode {
        sonalloy_core::compiler::GeneratorOutputMode::Mono => "mono",
        sonalloy_core::compiler::GeneratorOutputMode::Stereo => "stereo",
    }
}

fn inspect_generator(
    compiled: &CompiledInstrument,
    generator: &sonalloy_core::compiler::CompiledGenerator,
) -> (InspectGenerator, &'static str) {
    match generator {
        sonalloy_core::compiler::CompiledGenerator::Oscillator(oscillator) => {
            inspect_oscillator_generator(compiled, generator, oscillator)
        }
        sonalloy_core::compiler::CompiledGenerator::Noise(noise) => (
            InspectGenerator::Noise {
                output_mode: output_mode_name(generator.output_mode()),
                noise_color: match noise.color {
                    sonalloy_core::NoiseColor::White => "white",
                    sonalloy_core::NoiseColor::Pink => "pink",
                    sonalloy_core::NoiseColor::Brown => "brown",
                },
                noise_seed: noise.seed,
                noise_correlation_parameter: parameter_descriptor_id(compiled, noise.correlation),
            },
            "not_applicable (noise)",
        ),
        sonalloy_core::compiler::CompiledGenerator::PhysicalString(string) => (
            InspectGenerator::PhysicalString {
                output_mode: output_mode_name(generator.output_mode()),
                exciter: inspect_physical_exciter(string.exciter),
                decay_seconds: parameter_default(compiled, string.parameters.decay_seconds),
                decay_parameter: parameter_descriptor_id(compiled, string.parameters.decay_seconds),
                brightness: parameter_default(compiled, string.parameters.brightness),
                brightness_parameter: parameter_descriptor_id(
                    compiled,
                    string.parameters.brightness,
                ),
                stiffness: parameter_default(compiled, string.parameters.stiffness),
                stiffness_parameter: parameter_descriptor_id(compiled, string.parameters.stiffness),
                effective_max_frequency_hz: string.effective_max_frequency,
            },
            "ready",
        ),
        sonalloy_core::compiler::CompiledGenerator::Modal(modal) => (
            InspectGenerator::Modal {
                output_mode: output_mode_name(generator.output_mode()),
                exciter: inspect_physical_exciter(modal.exciter),
                mode_count: modal.mode_count,
                structure: parameter_default(compiled, modal.parameters.structure),
                structure_parameter: parameter_descriptor_id(compiled, modal.parameters.structure),
                brightness: parameter_default(compiled, modal.parameters.brightness),
                brightness_parameter: parameter_descriptor_id(
                    compiled,
                    modal.parameters.brightness,
                ),
                decay: parameter_default(compiled, modal.parameters.decay),
                decay_parameter: parameter_descriptor_id(compiled, modal.parameters.decay),
                effective_max_frequency_hz: modal.effective_max_frequency,
            },
            "ready",
        ),
        sonalloy_core::compiler::CompiledGenerator::Additive(additive) => {
            inspect_additive_generator(compiled, generator, additive)
        }
        sonalloy_core::compiler::CompiledGenerator::Formant(formant) => {
            inspect_formant_generator(compiled, generator, formant)
        }
        sonalloy_core::compiler::CompiledGenerator::Sample(sample) => {
            let (sample_zones, sample_asset_count) = inspect_sample_zones(sample);
            let sample_zone_count = sample.zones.len();
            let sample_enabled_zone_count =
                sample.zones.iter().filter(|zone| zone.is_enabled()).count();
            (
                InspectGenerator::Sample {
                    output_mode: output_mode_name(generator.output_mode()),
                    interpolation: "cubic",
                    sample_zone_count,
                    sample_enabled_zone_count,
                    sample_disabled_zone_count: sample_zone_count - sample_enabled_zone_count,
                    sample_asset_count,
                    sample_zones,
                },
                if sample_enabled_zone_count > 0 {
                    "enabled"
                } else {
                    "disabled"
                },
            )
        }
        sonalloy_core::compiler::CompiledGenerator::Granular(granular) => {
            inspect_granular_generator(compiled, generator, granular)
        }
        sonalloy_core::compiler::CompiledGenerator::WaveSequence(sequence) => {
            inspect_wave_sequence_generator(generator, sequence)
        }
        sonalloy_core::compiler::CompiledGenerator::Wavetable(wavetable) => {
            inspect_wavetable_generator(compiled, generator, wavetable)
        }
        sonalloy_core::compiler::CompiledGenerator::Spectral(spectral) => {
            inspect_spectral_generator(compiled, generator, spectral)
        }
        sonalloy_core::compiler::CompiledGenerator::OperatorModulation(operator) => {
            inspect_operator_generator(compiled, generator, operator)
        }
    }
}

fn inspect_physical_exciter(
    exciter: sonalloy_core::compiler::CompiledPhysicalExciter,
) -> InspectPhysicalExciter {
    match exciter {
        sonalloy_core::compiler::CompiledPhysicalExciter::Impulse => InspectPhysicalExciter {
            kind: "impulse",
            duration_seconds: None,
            brightness: None,
            seed: None,
        },
        sonalloy_core::compiler::CompiledPhysicalExciter::NoiseBurst {
            duration_seconds,
            brightness,
            seed,
        } => InspectPhysicalExciter {
            kind: "noise_burst",
            duration_seconds: Some(duration_seconds),
            brightness: Some(brightness),
            seed: Some(seed),
        },
    }
}

fn inspect_additive_generator(
    compiled: &CompiledInstrument,
    generator: &sonalloy_core::compiler::CompiledGenerator,
    additive: &sonalloy_core::compiler::CompiledAdditive,
) -> (InspectGenerator, &'static str) {
    let partials = additive
        .partials
        .iter()
        .map(|partial| InspectAdditivePartial {
            id: partial.id.clone(),
            ratio: partial.ratio,
            amplitude_a: partial.amplitude_a,
            amplitude_b: partial.amplitude_b,
            phase: partial.phase,
            has_envelope: partial.envelope.is_some(),
        })
        .collect();
    (
        InspectGenerator::Additive {
            output_mode: output_mode_name(generator.output_mode()),
            partial_count: additive.partials.len(),
            max_partial_count: 64,
            phase_reset: additive.phase_reset,
            morph: parameter_default(compiled, additive.parameters.morph),
            spectrum_tilt_db_per_octave: parameter_default(
                compiled,
                additive.parameters.spectrum_tilt,
            ),
            inharmonicity: parameter_default(compiled, additive.parameters.inharmonicity),
            partials,
        },
        "enabled",
    )
}

fn inspect_formant_generator(
    compiled: &CompiledInstrument,
    generator: &sonalloy_core::compiler::CompiledGenerator,
    formant: &sonalloy_core::compiler::CompiledFormant,
) -> (InspectGenerator, &'static str) {
    let profiles = formant
        .profiles
        .iter()
        .map(|profile| InspectFormantProfile {
            id: profile.id.clone(),
            formants: profile
                .formants
                .iter()
                .map(|band| InspectFormantBand {
                    frequency_hz: band.frequency_hz,
                    bandwidth_hz: band.bandwidth_hz,
                    gain_db: band.gain_db,
                })
                .collect(),
        })
        .collect();
    (
        InspectGenerator::Formant {
            output_mode: output_mode_name(generator.output_mode()),
            partial_count: formant.partial_count,
            max_partial_count: 64,
            phase_reset: formant.phase_reset,
            profile_count: formant.profiles.len(),
            vowel_position: parameter_default(compiled, formant.parameters.vowel_position),
            formant_shift_cents: parameter_default(compiled, formant.parameters.formant_shift),
            throat: parameter_default(compiled, formant.parameters.throat),
            spectral_tilt_db_per_octave: parameter_default(
                compiled,
                formant.parameters.spectral_tilt,
            ),
            profiles,
        },
        "enabled",
    )
}

fn inspect_granular_generator(
    compiled: &CompiledInstrument,
    generator: &sonalloy_core::compiler::CompiledGenerator,
    granular: &sonalloy_core::compiler::CompiledGranular,
) -> (InspectGenerator, &'static str) {
    let metadata = granular
        .source
        .as_ref()
        .map(|source| &source.source_metadata);
    (
        InspectGenerator::Granular {
            output_mode: output_mode_name(generator.output_mode()),
            asset_path: granular.asset_path.clone(),
            asset_sha256_specified: granular.asset_sha256_specified,
            prepared: granular.source.is_some(),
            source_channels: metadata.map(|value| value.source_channels),
            prepared_frames: granular.source.as_ref().map(|source| source.frames),
            region_start_frame: granular.start_frame,
            region_end_frame: granular.end_frame,
            root_note: granular.root_note,
            position: parameter_default(compiled, granular.parameters.position),
            position_parameter: parameter_descriptor_id(compiled, granular.parameters.position),
            grain_size: parameter_default(compiled, granular.parameters.grain_size),
            grain_size_parameter: parameter_descriptor_id(compiled, granular.parameters.grain_size),
            density: parameter_default(compiled, granular.parameters.density),
            density_parameter: parameter_descriptor_id(compiled, granular.parameters.density),
            pitch: parameter_default(compiled, granular.parameters.pitch),
            pitch_parameter: parameter_descriptor_id(compiled, granular.parameters.pitch),
            randomness: parameter_default(compiled, granular.parameters.randomness),
            randomness_parameter: parameter_descriptor_id(compiled, granular.parameters.randomness),
            pan_spread: parameter_default(compiled, granular.parameters.pan_spread),
            pan_spread_parameter: parameter_descriptor_id(compiled, granular.parameters.pan_spread),
            seed: granular.seed,
            grain_pool_limit: granular.grain_pool_limit,
        },
        if granular.source.is_some() {
            "enabled"
        } else {
            "disabled"
        },
    )
}

fn inspect_wave_sequence_generator(
    generator: &sonalloy_core::compiler::CompiledGenerator,
    sequence: &sonalloy_core::compiler::CompiledWaveSequence,
) -> (InspectGenerator, &'static str) {
    let steps = sequence
        .steps
        .iter()
        .map(|step| {
            let metadata = step.source.as_ref().map(|source| &source.source_metadata);
            let (duration_type, duration) = match step.duration {
                sonalloy_core::compiler::CompiledWaveSequenceDuration::Seconds(value) => {
                    ("seconds", value)
                }
                sonalloy_core::compiler::CompiledWaveSequenceDuration::Beats(value) => {
                    ("beats", value)
                }
            };
            InspectWaveSequenceStep {
                id: step.id.clone(),
                enabled: step.is_enabled(),
                asset_path: step.asset_path.clone(),
                start_frame: step.start_frame,
                end_frame: step.end_frame,
                duration_type,
                duration,
                playback: match step.playback {
                    sonalloy_core::compiler::CompiledWaveSequenceStepPlayback::OneShot => {
                        "one_shot"
                    }
                    sonalloy_core::compiler::CompiledWaveSequenceStepPlayback::Loop => "loop",
                },
                playback_direction: match step.playback_direction {
                    sonalloy_core::compiler::CompiledSampleDirection::Forward => "forward",
                    sonalloy_core::compiler::CompiledSampleDirection::Reverse => "reverse",
                },
                gain_db: 20.0 * step.gain.log10(),
                gain_linear: step.gain,
                pitch_cents: step.pitch_cents,
                source_channels: metadata.map(|value| value.source_channels),
                prepared_frames: step.source.as_ref().map(|source| source.frames),
            }
        })
        .collect::<Vec<_>>();
    let enabled_step_count = steps.iter().filter(|step| step.enabled).count();
    (
        InspectGenerator::WaveSequence {
            output_mode: output_mode_name(generator.output_mode()),
            step_count: steps.len(),
            enabled_step_count,
            direction: match sequence.direction {
                sonalloy_core::WaveSequenceDirection::Forward => "forward",
                sonalloy_core::WaveSequenceDirection::Reverse => "reverse",
                sonalloy_core::WaveSequenceDirection::PingPong => "ping_pong",
            },
            loop_sequence: sequence.loop_sequence,
            crossfade: sequence.crossfade,
            steps,
        },
        if enabled_step_count > 0 {
            "enabled"
        } else {
            "disabled"
        },
    )
}

fn inspect_oscillator_generator(
    compiled: &CompiledInstrument,
    generator: &sonalloy_core::compiler::CompiledGenerator,
    oscillator: &sonalloy_core::compiler::CompiledOscillator,
) -> (InspectGenerator, &'static str) {
    let (backend, sync_ratio) = match oscillator.backend {
        sonalloy_core::compiler::CompiledOscillatorBackend::Basic => ("basic", None),
        sonalloy_core::compiler::CompiledOscillatorBackend::VariableShapeSync { sync_ratio } => {
            ("variable_shape_sync", Some(sync_ratio))
        }
        sonalloy_core::compiler::CompiledOscillatorBackend::PhaseDomain => ("phase_domain", None),
    };
    (
        InspectGenerator::Oscillator {
            waveform: match oscillator.waveform {
                OscillatorWaveform::Sine => "sine",
                OscillatorWaveform::Saw => "saw",
                OscillatorWaveform::Square => "square",
                OscillatorWaveform::Triangle => "triangle",
                OscillatorWaveform::Pulse { .. } => "pulse",
            },
            phase_reset: oscillator.phase_reset,
            phase: oscillator.phase,
            output_mode: output_mode_name(generator.output_mode()),
            backend,
            hard_sync: sync_ratio.is_some(),
            sync_ratio_parameter: sync_ratio
                .map(|handle| parameter_descriptor_id(compiled, handle)),
            waveshaping: oscillator.parameters.waveshape.is_some(),
            waveshape_parameter: oscillator
                .parameters
                .waveshape
                .map(|handle| parameter_descriptor_id(compiled, handle)),
            phase_distortion: oscillator.parameters.phase_distortion.is_some(),
            phase_distortion_parameter: oscillator
                .parameters
                .phase_distortion
                .map(|handle| parameter_descriptor_id(compiled, handle)),
            wavefold: oscillator.parameters.wavefold.is_some(),
            wavefold_parameter: oscillator
                .parameters
                .wavefold
                .map(|handle| parameter_descriptor_id(compiled, handle)),
            oscillator_feedback: oscillator.parameters.oscillator_feedback.is_some(),
            oscillator_feedback_parameter: oscillator
                .parameters
                .oscillator_feedback
                .map(|handle| parameter_descriptor_id(compiled, handle)),
            dc_blocker: oscillator.dc_blocker,
            signal_order: if oscillator.backend
                == sonalloy_core::compiler::CompiledOscillatorBackend::PhaseDomain
            {
                "phase_domain -> unison_mix -> waveshaping -> wavefolder -> dc_blocker"
            } else if oscillator.parameters.wavefold.is_some() {
                "oscillator -> unison_mix -> waveshaping -> wavefolder -> dc_blocker"
            } else {
                "oscillator -> unison_mix -> waveshaping"
            },
            combination_constraints: "phase_distortion and oscillator_feedback require sine; neither combines with hard_sync",
            unison_voices: oscillator.unison.position_distribution.len(),
            unison_detune_parameter: oscillator
                .parameters
                .unison_detune
                .map(|handle| parameter_descriptor_id(compiled, handle)),
            unison_spread_parameter: oscillator
                .parameters
                .unison_spread
                .map(|handle| parameter_descriptor_id(compiled, handle)),
            phase_spread: oscillator.unison.phase_spread,
            effective_max_frequency_hz: oscillator
                .backend
                .effective_max_frequency(compiled.process_sample_rate),
            pulse_width: oscillator
                .parameters
                .pulse_width
                .map(|handle| parameter_default(compiled, handle)),
        },
        "not_applicable (oscillator-only instrument)",
    )
}

fn inspect_wavetable_generator(
    compiled: &CompiledInstrument,
    generator: &sonalloy_core::compiler::CompiledGenerator,
    wavetable: &sonalloy_core::compiler::CompiledWavetable,
) -> (InspectGenerator, &'static str) {
    let prepared = wavetable.prepared.as_ref();
    let metadata = prepared.map(|value| &value.source_metadata);
    (
        InspectGenerator::Wavetable {
            output_mode: output_mode_name(generator.output_mode()),
            asset_path: wavetable.asset_path.clone(),
            asset_sha256_specified: wavetable.asset_sha256_specified,
            prepared: prepared.is_some(),
            source_channels: metadata.map(|value| value.source_channels),
            source_frame_count: metadata.map(|value| value.source_frames),
            frame_length: wavetable.frame_length,
            frame_count: prepared.map(|value| value.frame_count),
            band_count: prepared.map(|value| value.bands.len()),
            band_max_harmonics: prepared.map_or_else(Vec::new, |value| {
                value.bands.iter().map(|band| band.max_harmonic).collect()
            }),
            position: parameter_default(compiled, wavetable.parameters.position),
            position_parameter: parameter_descriptor_id(compiled, wavetable.parameters.position),
            phase_reset: wavetable.phase_reset,
            phase: wavetable.phase,
            unison_voices: wavetable.unison.position_distribution.len(),
            unison_detune_parameter: wavetable
                .parameters
                .unison_detune
                .map(|handle| parameter_descriptor_id(compiled, handle)),
            unison_spread_parameter: wavetable
                .parameters
                .unison_spread
                .map(|handle| parameter_descriptor_id(compiled, handle)),
            effective_max_frequency_hz: wavetable.effective_max_frequency,
        },
        if prepared.is_some() {
            "enabled"
        } else {
            "disabled"
        },
    )
}

fn inspect_spectral_generator(
    compiled: &CompiledInstrument,
    generator: &sonalloy_core::compiler::CompiledGenerator,
    spectral: &sonalloy_core::compiler::CompiledSpectral,
) -> (InspectGenerator, &'static str) {
    let source = spectral.source.as_ref();
    let source_b = spectral.source_b.as_ref();
    let metadata = source.map(|value| &value.source_metadata);
    let metadata_b = source_b.map(|value| &value.source_metadata);
    (
        InspectGenerator::Spectral {
            output_mode: output_mode_name(generator.output_mode()),
            asset_a_path: spectral.asset_a_path.clone(),
            asset_a_sha256_specified: spectral.asset_a_sha256_specified,
            asset_a_prepared: source.is_some(),
            asset_b_path: spectral.asset_b_path.clone(),
            asset_b_sha256_specified: spectral.asset_b_sha256_specified,
            asset_b_prepared: source_b.is_some(),
            asset_b_source_sample_rate: metadata_b.map(|value| value.source_sample_rate),
            asset_b_prepared_sample_rate: source_b.map(|value| value.sample_rate),
            asset_b_source_channels: metadata_b.map(|value| value.source_channels),
            asset_b_source_frame_count: metadata_b.map(|value| value.source_frames),
            asset_b_spectral_frame_count: source_b.map(|value| value.spectral_frame_count),
            asset_b_prepared_bytes: source_b.map(|value| value.prepared_bytes),
            source_sample_rate: metadata.map(|value| value.source_sample_rate),
            prepared_sample_rate: source.map(|value| value.sample_rate),
            source_channels: metadata.map(|value| value.source_channels),
            source_frame_count: metadata.map(|value| value.source_frames),
            spectral_frame_count: source.map(|value| value.spectral_frame_count),
            prepared_bytes: source.map(|value| value.prepared_bytes),
            fft_size: spectral.fft_size,
            hop_size: spectral.hop_size,
            bin_count: source.map_or(spectral.fft_size / 2 + 1, |value| value.bin_count),
            latency_frames: spectral.latency_frames,
            root_note: spectral.root_note,
            position: parameter_default(compiled, spectral.parameters.position),
            position_parameter: parameter_descriptor_id(compiled, spectral.parameters.position),
            freeze: parameter_default(compiled, spectral.parameters.freeze),
            freeze_parameter: parameter_descriptor_id(compiled, spectral.parameters.freeze),
            blur_seconds: parameter_default(compiled, spectral.parameters.blur),
            blur_parameter: parameter_descriptor_id(compiled, spectral.parameters.blur),
            shift_hz: parameter_default(compiled, spectral.parameters.shift),
            shift_parameter: parameter_descriptor_id(compiled, spectral.parameters.shift),
            morph: spectral
                .parameters
                .morph
                .map(|handle| parameter_default(compiled, handle)),
            morph_parameter: spectral
                .parameters
                .morph
                .map(|handle| parameter_descriptor_id(compiled, handle)),
            phase_reset: spectral.phase_reset,
        },
        if source.is_some() && (spectral.asset_b_path.is_none() || spectral.source_b.is_some()) {
            "enabled"
        } else {
            "disabled"
        },
    )
}

fn inspect_operator_generator(
    compiled: &CompiledInstrument,
    generator: &sonalloy_core::compiler::CompiledGenerator,
    operator: &sonalloy_core::compiler::CompiledOperatorModulation,
) -> (InspectGenerator, &'static str) {
    let operators = operator
        .operators
        .iter()
        .zip(operator.parameters)
        .enumerate()
        .map(|(index, (compiled_operator, parameters))| InspectOperator {
            index: index + 1,
            ratio: parameter_default(compiled, parameters.ratio),
            detune_cents: parameter_default(compiled, parameters.detune),
            level: parameters
                .level
                .map(|handle| parameter_default(compiled, handle)),
            modulation_amount: parameters
                .modulation_amount
                .map(|handle| parameter_default(compiled, handle)),
            feedback: parameters
                .feedback
                .map(|handle| parameter_default(compiled, handle)),
            phase: compiled_operator.phase,
            envelope: InspectEnvelope {
                attack_samples: compiled_operator.envelope.attack_samples,
                decay_samples: compiled_operator.envelope.decay_samples,
                sustain_level: compiled_operator.envelope.sustain_level,
                release_samples: compiled_operator.envelope.release_samples,
            },
            ratio_parameter: parameter_descriptor_id(compiled, parameters.ratio),
            detune_parameter: parameter_descriptor_id(compiled, parameters.detune),
            level_parameter: parameters
                .level
                .map(|handle| parameter_descriptor_id(compiled, handle)),
            modulation_amount_parameter: parameters
                .modulation_amount
                .map(|handle| parameter_descriptor_id(compiled, handle)),
            feedback_parameter: parameters
                .feedback
                .map(|handle| parameter_descriptor_id(compiled, handle)),
        })
        .collect();
    (
        InspectGenerator::OperatorModulation {
            output_mode: output_mode_name(generator.output_mode()),
            mode: operator_mode_name(operator.mode),
            algorithm: operator_algorithm_name(operator.algorithm),
            evaluation_order: operator
                .topology
                .evaluation_order
                .iter()
                .map(|index| usize::from(*index) + 1)
                .collect(),
            incoming_masks: operator.topology.incoming_masks.to_vec(),
            carrier_operators: (0..4)
                .filter(|index| operator.topology.carrier_mask & (1_u8 << index) != 0)
                .map(|index| index + 1)
                .collect(),
            operators,
            phase_reset: operator.phase_reset,
            unison_voices: operator.unison.position_distribution.len(),
            unison_detune_parameter: operator
                .unison_detune
                .map(|handle| parameter_descriptor_id(compiled, handle)),
            unison_spread_parameter: operator
                .unison_spread
                .map(|handle| parameter_descriptor_id(compiled, handle)),
            effective_max_frequency_hz: operator.effective_max_frequency,
        },
        "enabled",
    )
}

fn operator_mode_name(mode: sonalloy_core::OperatorModulationMode) -> &'static str {
    match mode {
        sonalloy_core::OperatorModulationMode::Phase => "phase",
        sonalloy_core::OperatorModulationMode::Frequency => "frequency",
        sonalloy_core::OperatorModulationMode::Amplitude => "amplitude",
        sonalloy_core::OperatorModulationMode::Ring => "ring",
    }
}

fn operator_algorithm_name(algorithm: sonalloy_core::OperatorAlgorithm) -> &'static str {
    match algorithm {
        sonalloy_core::OperatorAlgorithm::Stack4 => "stack_4",
        sonalloy_core::OperatorAlgorithm::Stack3PlusCarrier => "stack_3_plus_carrier",
        sonalloy_core::OperatorAlgorithm::TwoStacks => "two_stacks",
        sonalloy_core::OperatorAlgorithm::ForkToCarrier => "fork_to_carrier",
        sonalloy_core::OperatorAlgorithm::TwoModulatorsPlusCarrier => "two_modulators_plus_carrier",
        sonalloy_core::OperatorAlgorithm::ThreeModulators => "three_modulators",
        sonalloy_core::OperatorAlgorithm::SharedModulator => "shared_modulator",
        sonalloy_core::OperatorAlgorithm::Parallel => "parallel",
    }
}

fn inspect_source_bounds(
    compiled: &CompiledInstrument,
    source: sonalloy_core::compiler::CompiledSourceRef,
) -> InspectSourceBounds {
    let range = match source {
        sonalloy_core::compiler::CompiledSourceRef::Voice(handle) => {
            compiled.sources.get(handle.index()).map_or_else(
                || inspect_source_range("unknown"),
                |source| inspect_source(source).value_range,
            )
        }
        sonalloy_core::compiler::CompiledSourceRef::PitchBend => inspect_source_range("pitch_bend"),
        sonalloy_core::compiler::CompiledSourceRef::ModWheel => inspect_source_range("mod_wheel"),
        sonalloy_core::compiler::CompiledSourceRef::Aftertouch => {
            inspect_source_range("aftertouch")
        }
    };
    InspectSourceBounds {
        min: range.min,
        max: range.max,
    }
}

fn inspect_route_effect(
    descriptor: &sonalloy_core::ParameterDescriptor,
    source_range: InspectSourceBounds,
    depth: f32,
    curve: ModulationCurve,
) -> InspectRouteEffect {
    let first =
        sonalloy_core::runtime::modulation::route_domain_delta(source_range.min, depth, curve);
    let second =
        sonalloy_core::runtime::modulation::route_domain_delta(source_range.max, depth, curve);
    let (min, max) = if first <= second {
        (first, second)
    } else {
        (second, first)
    };
    match descriptor.scale {
        ParameterScale::Linear => InspectRouteEffect {
            kind: "additive",
            unit: descriptor.modulation_unit(),
            min_delta: Some(min),
            max_delta: Some(max),
            min_octaves: None,
            max_octaves: None,
            min_factor: None,
            max_factor: None,
        },
        ParameterScale::Log2 => InspectRouteEffect {
            kind: "multiplicative",
            unit: descriptor.modulation_unit(),
            min_delta: None,
            max_delta: None,
            min_octaves: Some(min),
            max_octaves: Some(max),
            min_factor: Some(2.0_f32.powf(min)),
            max_factor: Some(2.0_f32.powf(max)),
        },
    }
}

fn inspect_modulated_range(
    compiled: &CompiledInstrument,
    handle: ParameterHandle,
) -> Option<InspectModulatedRange> {
    let descriptor = compiled.parameter_descriptor(handle)?;
    let routes = compiled.routes_for(handle);
    if routes.is_empty() {
        return None;
    }
    let mut minimum = 0.0_f32;
    let mut maximum = 0.0_f32;
    for route in routes {
        let source_range = inspect_source_bounds(compiled, route.source);
        let first = sonalloy_core::runtime::modulation::route_domain_delta(
            source_range.min,
            route.depth,
            route.curve,
        );
        let second = sonalloy_core::runtime::modulation::route_domain_delta(
            source_range.max,
            route.depth,
            route.curve,
        );
        minimum += first.min(second);
        maximum += first.max(second);
    }
    let (unclamped_min, unclamped_max) = match descriptor.scale {
        ParameterScale::Linear => (descriptor.default + minimum, descriptor.default + maximum),
        ParameterScale::Log2 => (
            descriptor.default * 2.0_f32.powf(minimum),
            descriptor.default * 2.0_f32.powf(maximum),
        ),
    };
    let effective_min = unclamped_min.clamp(descriptor.min, descriptor.max);
    let effective_max = unclamped_max.clamp(descriptor.min, descriptor.max);
    Some(InspectModulatedRange {
        unclamped_min,
        unclamped_max,
        effective_min,
        effective_max,
        may_clamp: unclamped_min < descriptor.min || unclamped_max > descriptor.max,
    })
}

#[allow(clippy::too_many_lines)]
fn make_inspect_report(
    compiled: &CompiledInstrument,
    diagnostics: Vec<Diagnostic>,
) -> InspectReport {
    let layers = compiled
        .layers
        .iter()
        .map(|layer| {
            let gain_db = parameter_default(compiled, layer.parameters.gain);
            let pan = parameter_default(compiled, layer.parameters.pan);
            let tuning_cents = parameter_default(compiled, layer.parameters.tuning);
            let (generator, asset_status) = inspect_generator(compiled, &layer.generator);
            InspectLayer {
                id: layer.id.clone(),
                enabled: true,
                trigger: InspectTrigger {
                    event: match layer.trigger.event {
                        sonalloy_core::LayerTriggerEvent::NoteOn => "note_on",
                        sonalloy_core::LayerTriggerEvent::NoteOff => "note_off",
                    },
                    key_min: layer.trigger.key_min,
                    key_max: layer.trigger.key_max,
                    velocity_min: layer.trigger.velocity_min,
                    velocity_max: layer.trigger.velocity_max,
                },
                generator,
                asset_status,
                gain_db,
                gain_linear: 10.0_f32.powf(gain_db / 20.0),
                pan,
                tuning_cents,
                tuning_ratio: 2.0_f32.powf(tuning_cents / 1200.0),
                envelope: InspectEnvelope {
                    attack_samples: layer.envelope.attack_samples,
                    decay_samples: layer.envelope.decay_samples,
                    sustain_level: layer.envelope.sustain_level,
                    release_samples: layer.envelope.release_samples,
                },
                processors: layer
                    .processors
                    .iter()
                    .enumerate()
                    .map(|(index, processor)| {
                        inspect_processor(compiled, processor, "layer", index)
                    })
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let routes = compiled
        .routes
        .iter()
        .map(|route| {
            let descriptor = compiled
                .parameter_descriptor(route.target)
                .expect("compiled route target handle must be valid");
            let source_range = inspect_source_bounds(compiled, route.source);
            let effect = inspect_route_effect(descriptor, source_range, route.depth, route.curve);
            InspectRoute {
                source: source_id(compiled, route.source),
                target: descriptor.id.clone(),
                depth: InspectDepth {
                    value: route.depth,
                    unit: descriptor.modulation_unit(),
                },
                curve: route.curve,
                source_range,
                effect,
            }
        })
        .collect::<Vec<_>>();
    let mut sources = compiled
        .sources
        .iter()
        .map(inspect_source)
        .collect::<Vec<_>>();
    for route in &compiled.routes {
        let Some(id) = external_source_name(route.source) else {
            continue;
        };
        if !sources.iter().any(|source| source.id == id) {
            sources.push(inspect_external_source(id));
        }
    }
    InspectReport {
        status: "ok",
        name: compiled.metadata.name.clone(),
        metadata: InspectMetadata {
            name: compiled.metadata.name.clone(),
            author: compiled.metadata.author.clone(),
            description: compiled.metadata.description.clone(),
        },
        polyphony: compiled.performance.polyphony,
        voice_stealing: match compiled.performance.voice_stealing {
            sonalloy_core::compiler::CompiledVoiceStealing::QuietestReleasingThenOldest => {
                "quietest_releasing_then_oldest"
            }
        },
        reported_latency_frames: compiled.reported_latency_frames,
        layer_count: layers.len(),
        layers,
        voice_processors: compiled
            .voice_processors
            .iter()
            .enumerate()
            .map(|(index, processor)| inspect_processor(compiled, processor, "voice", index))
            .collect(),
        global_processors: compiled
            .global_processors
            .iter()
            .enumerate()
            .map(|(index, processor)| inspect_processor(compiled, processor, "global", index))
            .collect(),
        parameters: compiled
            .parameters()
            .iter()
            .map(|parameter| {
                let handle = compiled
                    .parameter_handle(&parameter.id)
                    .expect("parameter descriptor must resolve to its catalog handle");
                InspectParameter {
                    id: parameter.id.clone(),
                    owner: parameter.owner,
                    unit: parameter.unit,
                    min: parameter.min,
                    max: parameter.max,
                    default: parameter.default,
                    scale: parameter.scale,
                    smoothing_seconds: parameter.smoothing_seconds,
                    modulation: inspect_modulation(parameter),
                    modulated_range_from_default: inspect_modulated_range(compiled, handle),
                }
            })
            .collect(),
        sources,
        routes,
        diagnostics,
    }
}

#[allow(clippy::too_many_lines)]
fn print_inspect(compiled: &CompiledInstrument, diagnostics: &[Diagnostic]) {
    let report = make_inspect_report(compiled, diagnostics.to_vec());
    println!("metadata.name: {}", report.metadata.name);
    println!(
        "metadata.author: {}",
        report.metadata.author.as_deref().unwrap_or("none")
    );
    println!(
        "metadata.description: {}",
        report.metadata.description.as_deref().unwrap_or("none")
    );
    println!("polyphony: {}", report.polyphony);
    println!("voice stealing: quietest_releasing_then_oldest");
    println!(
        "reported latency: {} frames",
        report.reported_latency_frames
    );
    for layer in &report.layers {
        print_generator(&layer.id, &layer.generator);
        println!(
            "  trigger: {} key {}..{} velocity {}..{}",
            layer.trigger.event,
            layer.trigger.key_min,
            layer.trigger.key_max,
            layer.trigger.velocity_min,
            layer.trigger.velocity_max,
        );
        println!("  asset: {}", layer.asset_status);
        println!(
            "  gain: {:.3} dB ({:.6} linear) pan: {:.3}",
            layer.gain_db, layer.gain_linear, layer.pan
        );
        println!(
            "  tuning: {:.3} cents ({:.6} ratio)",
            layer.tuning_cents, layer.tuning_ratio
        );
        println!(
            "  envelope: attack {} samples decay {} samples sustain {:.3} release {} samples",
            layer.envelope.attack_samples,
            layer.envelope.decay_samples,
            layer.envelope.sustain_level,
            layer.envelope.release_samples
        );
        print_processor_reports(&layer.processors, "layer", &layer.id);
    }
    print_processor_reports(&report.voice_processors, "voice", "voice");
    print_processor_reports(&report.global_processors, "global", "global");
    for parameter in &report.parameters {
        println!("parameter {}:", parameter.id);
        println!("  owner: {:?}", parameter.owner);
        println!("  unit: {:?}", parameter.unit);
        println!("  range: {:.3} .. {:.3}", parameter.min, parameter.max);
        println!("  default: {:.3}", parameter.default);
        println!("  scale: {:?}", parameter.scale);
        println!(
            "  modulation: {:?}, max absolute depth {:.3}",
            parameter.modulation.unit, parameter.modulation.max_abs_depth
        );
        if let Some(range) = &parameter.modulated_range_from_default {
            println!(
                "  reachable: {:.3} .. {:.3} (effective {:.3} .. {:.3}, may_clamp {})",
                range.unclamped_min,
                range.unclamped_max,
                range.effective_min,
                range.effective_max,
                range.may_clamp
            );
        }
        println!("  smoothing: {:.3} s", parameter.smoothing_seconds);
    }
    for source in &report.sources {
        println!(
            "source {}: {} ({}) range {:.3} .. {:.3} {:?}",
            source.id,
            source.kind,
            source.scope,
            source.value_range.min,
            source.value_range.max,
            source.value_range.polarity
        );
    }
    for route in &report.routes {
        print!(
            "route {} -> {} depth {:.3} {:?} curve {:?} source_range {:.3} .. {:.3} effect {} {:?}",
            route.source,
            route.target,
            route.depth.value,
            route.depth.unit,
            route.curve,
            route.source_range.min,
            route.source_range.max,
            route.effect.kind,
            route.effect.unit
        );
        match route.effect.kind {
            "additive" => println!(
                " delta {:.3} .. {:.3}",
                route.effect.min_delta.unwrap_or(0.0),
                route.effect.max_delta.unwrap_or(0.0)
            ),
            "multiplicative" => println!(
                " octaves {:.3} .. {:.3} factor {:.3} .. {:.3}",
                route.effect.min_octaves.unwrap_or(0.0),
                route.effect.max_octaves.unwrap_or(0.0),
                route.effect.min_factor.unwrap_or(1.0),
                route.effect.max_factor.unwrap_or(1.0)
            ),
            _ => println!(),
        }
    }
    print_warnings(&report.diagnostics);
}

fn print_oscillator_generator(layer_id: &str, generator: &InspectGenerator) {
    let InspectGenerator::Oscillator {
        waveform,
        phase_reset,
        phase,
        output_mode,
        backend,
        hard_sync,
        sync_ratio_parameter,
        waveshaping,
        waveshape_parameter,
        phase_distortion,
        phase_distortion_parameter,
        wavefold,
        wavefold_parameter,
        oscillator_feedback,
        oscillator_feedback_parameter,
        dc_blocker,
        signal_order,
        combination_constraints,
        unison_voices,
        unison_detune_parameter,
        unison_spread_parameter,
        phase_spread,
        effective_max_frequency_hz,
        pulse_width,
    } = generator
    else {
        return;
    };
    println!(
        "layer {layer_id}: enabled true generator oscillator/{waveform} phase_reset {phase_reset} phase {phase} output_mode {output_mode}"
    );
    println!("  backend: {backend} effective_max_frequency_hz: {effective_max_frequency_hz:.3}");
    println!(
        "  hard_sync: {hard_sync} waveshaping: {waveshaping} phase_distortion: {phase_distortion} wavefold: {wavefold} oscillator_feedback: {oscillator_feedback}"
    );
    println!("  dc_blocker: {dc_blocker} signal_order: {signal_order}");
    println!("  combination_constraints: {combination_constraints}");
    if let Some(value) = pulse_width {
        println!("  pulse_width: {value}");
    }
    print_parameter_reference("sync_ratio", sync_ratio_parameter.as_ref());
    print_parameter_reference("waveshape", waveshape_parameter.as_ref());
    print_parameter_reference("phase_distortion", phase_distortion_parameter.as_ref());
    print_parameter_reference("wavefold", wavefold_parameter.as_ref());
    print_parameter_reference(
        "oscillator_feedback",
        oscillator_feedback_parameter.as_ref(),
    );
    print_parameter_reference("unison_detune", unison_detune_parameter.as_ref());
    print_parameter_reference("unison_spread", unison_spread_parameter.as_ref());
    println!("  unison_voices: {unison_voices} phase_spread: {phase_spread:.3}");
}

#[allow(clippy::too_many_lines)]
fn print_generator(layer_id: &str, generator: &InspectGenerator) {
    match generator {
        InspectGenerator::Oscillator { .. } => print_oscillator_generator(layer_id, generator),
        InspectGenerator::Noise {
            output_mode,
            noise_color,
            noise_seed,
            noise_correlation_parameter,
        } => {
            println!(
                "layer {layer_id}: enabled true generator noise/{noise_color} output_mode {output_mode}"
            );
            println!("  noise seed: {noise_seed}");
            println!("  noise correlation parameter: {noise_correlation_parameter}");
        }
        InspectGenerator::PhysicalString {
            output_mode,
            exciter,
            decay_seconds,
            decay_parameter,
            brightness,
            brightness_parameter,
            stiffness,
            stiffness_parameter,
            effective_max_frequency_hz,
        } => {
            println!(
                "layer {layer_id}: enabled true generator physical_string output_mode {output_mode}"
            );
            print_physical_exciter(exciter);
            println!(
                "  decay_seconds: {decay_seconds:.6} ({decay_parameter}) brightness: {brightness:.6} ({brightness_parameter}) stiffness: {stiffness:.6} ({stiffness_parameter})"
            );
            println!("  effective_max_frequency_hz: {effective_max_frequency_hz:.3}");
        }
        InspectGenerator::Modal {
            output_mode,
            exciter,
            mode_count,
            structure,
            structure_parameter,
            brightness,
            brightness_parameter,
            decay,
            decay_parameter,
            effective_max_frequency_hz,
        } => {
            println!("layer {layer_id}: enabled true generator modal output_mode {output_mode}");
            print_physical_exciter(exciter);
            println!(
                "  mode_count: {mode_count} structure: {structure:.6} ({structure_parameter}) brightness: {brightness:.6} ({brightness_parameter}) decay: {decay:.6} ({decay_parameter})"
            );
            println!("  effective_max_frequency_hz: {effective_max_frequency_hz:.3}");
        }
        InspectGenerator::Additive { .. } => print_additive_generator(layer_id, generator),
        InspectGenerator::Formant { .. } => print_formant_generator(layer_id, generator),
        InspectGenerator::Sample {
            output_mode,
            interpolation,
            sample_zone_count,
            sample_enabled_zone_count,
            sample_disabled_zone_count: _,
            sample_asset_count,
            sample_zones,
        } => {
            println!(
                "layer {layer_id}: enabled {} generator sample interpolation {interpolation} output_mode {output_mode}",
                *sample_enabled_zone_count > 0,
            );
            println!("  sample zones: {sample_enabled_zone_count}/{sample_zone_count} enabled");
            println!("  sample prepared assets: {sample_asset_count}");
            for zone in sample_zones {
                println!(
                    "  zone {}: enabled {} key {}..{} velocity {}..{} root_note {} playback {} direction {} time {} frames {}..{}",
                    zone.id,
                    zone.enabled,
                    zone.key_min,
                    zone.key_max,
                    zone.velocity_min,
                    zone.velocity_max,
                    zone.root_note,
                    zone.playback_type,
                    zone.direction,
                    zone.time_mode,
                    zone.start_frame,
                    zone.end_frame,
                );
                if let Some(ratio) = zone.duration_ratio {
                    println!("    duration ratio: {ratio:.6}");
                }
                if let Some(source_bpm) = zone.source_bpm {
                    println!("    source tempo: {source_bpm:.3} bpm");
                }
                if let (Some(loop_start), Some(loop_end)) =
                    (zone.loop_start_frame, zone.loop_end_frame)
                {
                    println!("    loop: {loop_start}..{loop_end}");
                    if let Some(crossfade) = zone.crossfade_frames {
                        println!("    loop crossfade: {crossfade} frames");
                    }
                }
                if let Some(group) = &zone.round_robin_group {
                    println!("    round_robin_group: {group}");
                }
                println!(
                    "    source: {} channels, {} prepared frames",
                    zone.source_channels
                        .map_or_else(|| "none".to_owned(), |value| value.to_string()),
                    zone.prepared_frames
                        .map_or_else(|| "none".to_owned(), |value| value.to_string()),
                );
            }
        }
        InspectGenerator::Granular { .. } => print_granular_generator(layer_id, generator),
        InspectGenerator::WaveSequence { .. } => print_wave_sequence_generator(layer_id, generator),
        InspectGenerator::Wavetable { .. } => print_wavetable_generator(layer_id, generator),
        InspectGenerator::Spectral { .. } => print_spectral_generator(layer_id, generator),
        InspectGenerator::OperatorModulation { .. } => {
            print_operator_generator(layer_id, generator);
        }
    }
}

fn print_physical_exciter(exciter: &InspectPhysicalExciter) {
    println!(
        "  exciter: {} duration_seconds: {:?} brightness: {:?} seed: {:?}",
        exciter.kind, exciter.duration_seconds, exciter.brightness, exciter.seed
    );
}

fn print_additive_generator(layer_id: &str, generator: &InspectGenerator) {
    let InspectGenerator::Additive {
        output_mode,
        partial_count,
        max_partial_count,
        phase_reset,
        morph,
        spectrum_tilt_db_per_octave,
        inharmonicity,
        partials,
    } = generator
    else {
        return;
    };
    println!("layer {layer_id}: enabled true generator additive output_mode {output_mode}");
    println!(
        "  partials: {partial_count}/{max_partial_count} phase_reset: {phase_reset} morph: {morph:.6}"
    );
    println!(
        "  spectrum_tilt_db_per_octave: {spectrum_tilt_db_per_octave:.6} inharmonicity: {inharmonicity:.6}"
    );
    for partial in partials {
        println!(
            "  partial {}: ratio {:.6} amplitude_a {:.6} amplitude_b {:.6} phase {:.6} envelope {}",
            partial.id,
            partial.ratio,
            partial.amplitude_a,
            partial.amplitude_b,
            partial.phase,
            partial.has_envelope,
        );
    }
}

fn print_formant_generator(layer_id: &str, generator: &InspectGenerator) {
    let InspectGenerator::Formant {
        output_mode,
        partial_count,
        max_partial_count,
        phase_reset,
        profile_count,
        vowel_position,
        formant_shift_cents,
        throat,
        spectral_tilt_db_per_octave,
        profiles,
    } = generator
    else {
        return;
    };
    println!("layer {layer_id}: enabled true generator formant output_mode {output_mode}");
    println!(
        "  partials: {partial_count}/{max_partial_count} phase_reset: {phase_reset} profiles: {profile_count}"
    );
    println!(
        "  vowel_position: {vowel_position:.6} formant_shift_cents: {formant_shift_cents:.6} throat: {throat:.6}"
    );
    println!("  spectral_tilt_db_per_octave: {spectral_tilt_db_per_octave:.6}");
    for profile in profiles {
        println!("  profile {}:", profile.id);
        for (index, band) in profile.formants.iter().enumerate() {
            println!(
                "    formant {}: frequency_hz {:.3} bandwidth_hz {:.3} gain_db {:.3}",
                index + 1,
                band.frequency_hz,
                band.bandwidth_hz,
                band.gain_db,
            );
        }
    }
}

fn print_granular_generator(layer_id: &str, generator: &InspectGenerator) {
    let InspectGenerator::Granular {
        output_mode,
        asset_path,
        asset_sha256_specified,
        prepared,
        source_channels,
        prepared_frames,
        region_start_frame,
        region_end_frame,
        root_note,
        position,
        position_parameter,
        grain_size,
        grain_size_parameter,
        density,
        density_parameter,
        pitch,
        pitch_parameter,
        randomness,
        randomness_parameter,
        pan_spread,
        pan_spread_parameter,
        seed,
        grain_pool_limit,
    } = generator
    else {
        return;
    };
    println!("layer {layer_id}: enabled {prepared} generator granular output_mode {output_mode}");
    println!(
        "  asset: {asset_path} sha256_specified: {asset_sha256_specified} source_channels: {} prepared_frames: {}",
        source_channels.map_or_else(|| "none".to_owned(), |value| value.to_string()),
        prepared_frames.map_or_else(|| "none".to_owned(), |value| value.to_string()),
    );
    println!(
        "  region: {region_start_frame}..{region_end_frame} root_note: {root_note} seed: {seed} grain_pool_limit: {grain_pool_limit}"
    );
    println!(
        "  position: {position:.3} ({position_parameter}) grain_size: {grain_size:.6} s ({grain_size_parameter}) density: {density:.3} /s ({density_parameter})"
    );
    println!(
        "  pitch: {pitch:.3} cents ({pitch_parameter}) randomness: {randomness:.3} ({randomness_parameter}) pan_spread: {pan_spread:.3} ({pan_spread_parameter})"
    );
}

fn print_wave_sequence_generator(layer_id: &str, generator: &InspectGenerator) {
    let InspectGenerator::WaveSequence {
        output_mode,
        step_count,
        enabled_step_count,
        direction,
        loop_sequence,
        crossfade,
        steps,
    } = generator
    else {
        return;
    };
    println!(
        "layer {layer_id}: enabled {} generator wave_sequence output_mode {output_mode}",
        *enabled_step_count > 0
    );
    println!(
        "  steps: {enabled_step_count}/{step_count} enabled direction: {direction} loop: {loop_sequence} crossfade: {crossfade:.3}"
    );
    for step in steps {
        println!(
            "  step {}: enabled {} asset {} frames {}..{} duration {} {:.6} playback {} direction {} gain_db {:.3} pitch_cents {:.3}",
            step.id,
            step.enabled,
            step.asset_path,
            step.start_frame,
            step.end_frame,
            step.duration_type,
            step.duration,
            step.playback,
            step.playback_direction,
            step.gain_db,
            step.pitch_cents,
        );
        println!(
            "    source: {} channels, {} prepared frames",
            step.source_channels
                .map_or_else(|| "none".to_owned(), |value| value.to_string()),
            step.prepared_frames
                .map_or_else(|| "none".to_owned(), |value| value.to_string()),
        );
    }
}

fn print_wavetable_generator(layer_id: &str, generator: &InspectGenerator) {
    let InspectGenerator::Wavetable {
        output_mode,
        asset_path,
        asset_sha256_specified,
        prepared,
        source_channels,
        source_frame_count,
        frame_length,
        frame_count,
        band_count,
        band_max_harmonics,
        position,
        position_parameter,
        phase_reset,
        phase,
        unison_voices,
        unison_detune_parameter,
        unison_spread_parameter,
        effective_max_frequency_hz,
    } = generator
    else {
        return;
    };
    println!("layer {layer_id}: enabled {prepared} generator wavetable output_mode {output_mode}");
    println!("  asset: {asset_path} sha256_specified: {asset_sha256_specified}");
    println!(
        "  prepared: {prepared} source_channels: {} source_frame_count: {}",
        source_channels.map_or_else(|| "none".to_owned(), |value| value.to_string()),
        source_frame_count.map_or_else(|| "none".to_owned(), |value| value.to_string()),
    );
    println!(
        "  frame_length: {frame_length} frame_count: {} band_count: {}",
        frame_count.map_or_else(|| "none".to_owned(), |value| value.to_string()),
        band_count.map_or_else(|| "none".to_owned(), |value| value.to_string()),
    );
    println!("  band_max_harmonics: {band_max_harmonics:?}");
    println!(
        "  position: {position:.3} parameter: {position_parameter} phase_reset: {phase_reset} phase: {phase:.3}"
    );
    println!("  unison_voices: {unison_voices}");
    print_parameter_reference("unison_detune", unison_detune_parameter.as_ref());
    print_parameter_reference("unison_spread", unison_spread_parameter.as_ref());
    println!("  effective_max_frequency_hz: {effective_max_frequency_hz:.3}");
}

fn print_spectral_generator(layer_id: &str, generator: &InspectGenerator) {
    let InspectGenerator::Spectral {
        output_mode,
        asset_a_path,
        asset_a_sha256_specified,
        asset_a_prepared,
        asset_b_path,
        asset_b_sha256_specified,
        asset_b_prepared,
        asset_b_source_sample_rate,
        asset_b_prepared_sample_rate,
        asset_b_source_channels,
        asset_b_source_frame_count,
        asset_b_spectral_frame_count,
        asset_b_prepared_bytes,
        source_sample_rate,
        prepared_sample_rate,
        source_channels,
        source_frame_count,
        spectral_frame_count,
        prepared_bytes,
        fft_size,
        hop_size,
        bin_count,
        latency_frames,
        root_note,
        position,
        position_parameter,
        freeze,
        freeze_parameter,
        blur_seconds,
        blur_parameter,
        shift_hz,
        shift_parameter,
        morph,
        morph_parameter,
        phase_reset,
    } = generator
    else {
        return;
    };
    let enabled = *asset_a_prepared && (asset_b_path.is_none() || *asset_b_prepared);
    println!("layer {layer_id}: enabled {enabled} generator spectral output_mode {output_mode}");
    println!(
        "  asset_a: {asset_a_path} sha256_specified: {asset_a_sha256_specified} prepared: {asset_a_prepared}"
    );
    println!(
        "  asset_b: {} sha256_specified: {asset_b_sha256_specified} prepared: {asset_b_prepared}",
        asset_b_path.as_deref().unwrap_or("none")
    );
    println!(
        "  asset_a_source: sample_rate {} prepared_sample_rate {} channels {} frames {}",
        source_sample_rate.map_or_else(|| "none".to_owned(), |value| value.to_string()),
        prepared_sample_rate.map_or_else(|| "none".to_owned(), |value| value.to_string()),
        source_channels.map_or_else(|| "none".to_owned(), |value| value.to_string()),
        source_frame_count.map_or_else(|| "none".to_owned(), |value| value.to_string()),
    );
    println!(
        "  asset_a_spectral: frames {} prepared_bytes {}",
        spectral_frame_count.map_or_else(|| "none".to_owned(), |value| value.to_string()),
        prepared_bytes.map_or_else(|| "none".to_owned(), |value| value.to_string()),
    );
    println!(
        "  asset_b_source: sample_rate {} prepared_sample_rate {} channels {} frames {}",
        asset_b_source_sample_rate.map_or_else(|| "none".to_owned(), |value| value.to_string()),
        asset_b_prepared_sample_rate.map_or_else(|| "none".to_owned(), |value| value.to_string()),
        asset_b_source_channels.map_or_else(|| "none".to_owned(), |value| value.to_string()),
        asset_b_source_frame_count.map_or_else(|| "none".to_owned(), |value| value.to_string()),
    );
    println!(
        "  asset_b_spectral: frames {} prepared_bytes {}",
        asset_b_spectral_frame_count.map_or_else(|| "none".to_owned(), |value| value.to_string()),
        asset_b_prepared_bytes.map_or_else(|| "none".to_owned(), |value| value.to_string()),
    );
    println!(
        "  fft_size: {fft_size} hop_size: {hop_size} bin_count: {bin_count} latency_frames: {latency_frames}"
    );
    println!("  root_note: {root_note} phase_reset: {phase_reset}");
    println!("  position: {position:.3} parameter: {position_parameter}");
    println!("  freeze: {freeze:.3} parameter: {freeze_parameter}");
    println!("  blur_seconds: {blur_seconds:.3} parameter: {blur_parameter}");
    println!("  shift_hz: {shift_hz:.3} parameter: {shift_parameter}");
    if let Some(morph) = morph {
        println!(
            "  morph: {morph:.3} parameter: {}",
            morph_parameter.as_deref().unwrap_or("none")
        );
    }
}

fn print_operator_generator(layer_id: &str, generator: &InspectGenerator) {
    let InspectGenerator::OperatorModulation {
        output_mode,
        mode,
        algorithm,
        evaluation_order,
        incoming_masks,
        carrier_operators,
        operators,
        phase_reset,
        unison_voices,
        unison_detune_parameter,
        unison_spread_parameter,
        effective_max_frequency_hz,
    } = generator
    else {
        return;
    };
    println!(
        "layer {layer_id}: enabled true generator operator_modulation/{mode} output_mode {output_mode}"
    );
    println!(
        "  algorithm: {algorithm} evaluation_order: {evaluation_order:?} carrier_operators: {carrier_operators:?}"
    );
    println!(
        "  incoming_masks: {incoming_masks:?} phase_reset: {phase_reset} unison_voices: {unison_voices}"
    );
    for operator in operators {
        println!(
            "  operator {}: ratio {:.3} detune_cents {:.3} level {} modulation_amount {} feedback {} phase {:.3}",
            operator.index,
            operator.ratio,
            operator.detune_cents,
            optional_value(operator.level),
            optional_value(operator.modulation_amount),
            optional_value(operator.feedback),
            operator.phase,
        );
        println!(
            "    envelope: attack {} decay {} sustain {:.3} release {}",
            operator.envelope.attack_samples,
            operator.envelope.decay_samples,
            operator.envelope.sustain_level,
            operator.envelope.release_samples,
        );
        println!("    parameter ratio: {}", operator.ratio_parameter);
        println!("    parameter detune: {}", operator.detune_parameter);
        print_parameter_reference("level", operator.level_parameter.as_ref());
        print_parameter_reference(
            "modulation_amount",
            operator.modulation_amount_parameter.as_ref(),
        );
        print_parameter_reference("feedback", operator.feedback_parameter.as_ref());
    }
    print_parameter_reference("unison_detune", unison_detune_parameter.as_ref());
    print_parameter_reference("unison_spread", unison_spread_parameter.as_ref());
    println!("  effective_max_frequency_hz: {effective_max_frequency_hz:.3}");
}

fn optional_value(value: Option<f32>) -> String {
    value.map_or_else(|| "unused".to_owned(), |value| format!("{value:.3}"))
}

fn print_parameter_reference(name: &str, parameter: Option<&String>) {
    if let Some(id) = parameter {
        println!("  {name}: ({id})");
    }
}

fn print_processor_reports(processors: &[InspectProcessor], placement: &'static str, owner: &str) {
    if processors.is_empty() {
        println!("{placement} processors ({owner}): none");
        return;
    }
    println!("{placement} processors ({owner}):");
    for report in processors {
        println!(
            "  processor[{}] {} ({})",
            report.chain_index, report.id, report.kind
        );
        if let Some(mode) = report.mode {
            println!("    mode: {mode}");
        }
        for field in &report.static_fields {
            println!("    {}: {:.3}", field.id, field.value);
        }
        for parameter in &report.parameters {
            println!(
                "    parameter {}: {:?} {:.3}..{:.3}, default {:.3}, scale {:?}, modulation {:?} max {:.3}",
                parameter.id,
                parameter.unit,
                parameter.min,
                parameter.max,
                parameter.default,
                parameter.scale,
                parameter.modulation.unit,
                parameter.modulation.max_abs_depth
            );
        }
    }
}

fn write_wav(path: &Path, audio: &sonalloy_core::RenderedAudio) -> Result<(), Diagnostic> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: audio.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|error| {
        Diagnostic::error(
            DiagnosticCode::WavOutputError,
            "could not create wav output",
        )
        .with_path(path.to_string_lossy())
        .with_detail(error.to_string())
    })?;
    for frame in 0..audio.frames() {
        writer
            .write_sample(audio.channels[0][frame])
            .map_err(|error| {
                Diagnostic::error(DiagnosticCode::WavOutputError, "could not write wav output")
                    .with_path(path.to_string_lossy())
                    .with_detail(error.to_string())
            })?;
        writer
            .write_sample(audio.channels[1][frame])
            .map_err(|error| {
                Diagnostic::error(DiagnosticCode::WavOutputError, "could not write wav output")
                    .with_path(path.to_string_lossy())
                    .with_detail(error.to_string())
            })?;
    }
    writer.finalize().map_err(|error| {
        Diagnostic::error(
            DiagnosticCode::WavOutputError,
            "could not finalize wav output",
        )
        .with_path(path.to_string_lossy())
        .with_detail(error.to_string())
    })?;
    Ok(())
}

fn input_failure(error: &RenderError) -> CliFailure {
    CliFailure {
        code: 2,
        diagnostics: vec![Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            error.to_string(),
        )],
    }
}

fn render_failure(error: &RenderError) -> CliFailure {
    let code = if matches!(error, RenderError::Process(_)) {
        3
    } else {
        2
    };
    let diagnostic = match error {
        RenderError::TraceLimitExceeded { .. } => from_render_error(error),
        _ if code == 2 => Diagnostic::error(DiagnosticCode::ValueOutOfRange, error.to_string()),
        _ => from_render_error(error),
    };
    CliFailure {
        code,
        diagnostics: vec![diagnostic],
    }
}

#[allow(clippy::needless_pass_by_value)]
fn print_success(json: bool, report: SuccessReport) -> ExitCode {
    if json {
        println!(
            "{}",
            serde_json::to_string(&report).expect("success report is serializable")
        );
    } else {
        println!(
            "rendered {} frames at {} Hz to {} using {}",
            report.frames, report.sample_rate, report.output, report.backend
        );
        if let Some(analysis) = &report.analysis {
            println!("analysis");
            match analysis.level.peak_dbfs {
                Some(peak) => println!("  peak: {peak:.2} dBFS"),
                None => println!("  peak: -inf dBFS"),
            }
            match analysis.level.rms_dbfs {
                Some(rms) => println!("  rms: {rms:.2} dBFS"),
                None => println!("  rms: -inf dBFS"),
            }
            if let Some(centroid) = analysis.spectrum.spectral_centroid_hz {
                println!("  centroid: {centroid:.1} Hz");
            } else {
                println!("  centroid: none");
            }
            match analysis.stereo.correlation {
                Some(correlation) => println!("  stereo correlation: {correlation:.3}"),
                None => println!("  stereo correlation: none"),
            }
            match (analysis.activity.first_frame, analysis.activity.last_frame) {
                (Some(first), Some(last)) => println!(
                    "  activity: frames {first}..{last} at {:.0} dBFS threshold",
                    analysis.activity.threshold_dbfs
                ),
                _ => println!(
                    "  activity: none at {:.0} dBFS threshold",
                    analysis.activity.threshold_dbfs
                ),
            }
            println!(
                "  large discontinuities: {}",
                analysis.continuity.large_delta_count
            );
        }
        if let Some(trace) = &report.trace {
            for parameter in &trace.parameters {
                let last_frame = parameter.observations.last().map_or_else(
                    || "none".to_owned(),
                    |observation| observation.frame.to_string(),
                );
                println!(
                    "trace {}: {} observations, last frame {}",
                    parameter.parameter,
                    parameter.observations.len(),
                    last_frame
                );
            }
        }
        print_warnings(&report.diagnostics);
    }
    ExitCode::SUCCESS
}

#[allow(clippy::needless_pass_by_value)]
fn finish_failure(json: bool, failure: CliFailure) -> ExitCode {
    print_failure(json, &failure);
    ExitCode::from(failure.code)
}

fn print_failure(json: bool, failure: &CliFailure) {
    if json {
        #[derive(Serialize)]
        struct ErrorReport<'a> {
            status: &'static str,
            exit_code: u8,
            diagnostics: &'a [Diagnostic],
        }
        let report = ErrorReport {
            status: "error",
            exit_code: failure.code,
            diagnostics: &failure.diagnostics,
        };
        println!(
            "{}",
            serde_json::to_string(&report).expect("error report is serializable")
        );
    } else {
        for diagnostic in &failure.diagnostics {
            eprintln!("error[{:?}]: {}", diagnostic.code, diagnostic.message);
            if let Some(path) = &diagnostic.path {
                eprintln!("  path: {path}");
            }
            if let Some(detail) = &diagnostic.detail {
                eprintln!("  detail: {detail}");
            }
        }
    }
}

fn print_warnings(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        if diagnostic.severity == sonalloy_core::DiagnosticSeverity::Warning {
            eprintln!("warning[{:?}]: {}", diagnostic.code, diagnostic.message);
            if let Some(path) = &diagnostic.path {
                eprintln!("  path: {path}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_definition_is_valid() {
        assert!(default_definition().validate().is_empty());
    }

    #[test]
    fn inspect_reports_sample_playback_and_trigger_event() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/instruments/mapped-sample-instrument.json");
        let mut definition: InstrumentDefinition =
            serde_json::from_str(&std::fs::read_to_string(path).expect("sample Definition exists"))
                .expect("sample Definition parses");
        definition.layers[0].trigger.event = sonalloy_core::LayerTriggerEvent::NoteOff;
        let sonalloy_core::GeneratorDefinition::Sample(sample) =
            &mut definition.layers[0].generator
        else {
            panic!("reference Definition must contain a sample generator");
        };
        sample.zones[0].playback.direction = sonalloy_core::SamplePlaybackDirection::Reverse;
        sample.zones[0].playback.r#loop = Some(sonalloy_core::SampleLoopDefinition {
            start_seconds: 0.02,
            end_seconds: 0.06,
            crossfade_seconds: 0.01,
        });

        let result = compile_instrument(
            &definition,
            &CompileContext {
                definition_base_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../testdata/instruments"),
                process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid spec"),
            },
        );
        let sonalloy_core::CompileResult {
            instrument,
            diagnostics,
        } = result;
        let compiled = instrument.expect("sample Definition compiles");
        let report = make_inspect_report(&compiled, diagnostics);
        assert_eq!(report.layers[0].trigger.event, "note_off");
        let InspectGenerator::Sample { sample_zones, .. } = &report.layers[0].generator else {
            panic!("reference Definition must inspect as a sample generator");
        };
        assert_eq!(sample_zones[0].direction, "reverse");
        assert_eq!(sample_zones[0].crossfade_frames, Some(480));
        assert!(sample_zones[0].source_channels.is_some());
        assert!(sample_zones[0].prepared_frames.is_some());
    }

    #[test]
    fn native_error_maps_to_process_exit_and_diagnostic() {
        let error = RenderError::Process(sonalloy_core::ProcessError::DspFailure {
            kind: sonalloy_core::DspFailureKind::InvalidInput,
        });
        let failure = render_failure(&error);
        assert_eq!(failure.code, 3);
        assert_eq!(failure.diagnostics[0].code, DiagnosticCode::DspError);
        assert_eq!(
            failure.diagnostics[0].severity,
            sonalloy_core::DiagnosticSeverity::Error
        );
    }
}
