mod midi;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use sonalloy_core::{
    AdsrDefinition, CompileContext, CompiledInstrument, Diagnostic, DiagnosticCode,
    InstrumentDefinition, InstrumentMetadata, LayerDefinition, LayerTriggerDefinition,
    ModulationCurve, OscillatorDefinition, OscillatorWaveform, ParameterHandle, ParameterOwner,
    ParameterScale, ParameterUnit, PerformanceDefinition, ProcessEventKind, ProcessSpec,
    ProcessorDefinition, RenderError, RenderRequest, ScheduledEvent, VoiceStealingDefinition,
    backend_info, compile_instrument, from_render_error, render_instrument, render_sine,
    seconds_to_frames,
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
    output: String,
    backend: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<Diagnostic>,
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
    Sample {
        output_mode: &'static str,
        interpolation: &'static str,
        sample_zone_count: usize,
        sample_enabled_zone_count: usize,
        sample_disabled_zone_count: usize,
        sample_asset_count: usize,
        sample_zones: Vec<InspectSampleZone>,
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
    start_frame: usize,
    end_frame: usize,
    loop_start_frame: Option<usize>,
    loop_end_frame: Option<usize>,
    source_sample_rate: Option<u32>,
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
}

#[derive(Debug, Serialize)]
struct InspectSource {
    id: String,
    scope: &'static str,
    kind: &'static str,
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
    amount: f32,
    curve: ModulationCurve,
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
        normalized: f32,
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
    let audio = match render_instrument(Arc::clone(&compiled), request, &events) {
        Ok(audio) => audio,
        Err(error) => return finish_failure(args.json, render_failure(&error)),
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
            output: args.output.to_string_lossy().into_owned(),
            backend: backend_info().version,
            diagnostics: std::mem::take(&mut diagnostics),
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
    let audio = match render_instrument(Arc::clone(&compiled), request, &events) {
        Ok(audio) => audio,
        Err(error) => return finish_failure(args.json, render_failure(&error)),
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
            output: args.output.to_string_lossy().into_owned(),
            backend: backend_info().version,
            diagnostics,
        },
    )
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
                normalized,
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
                if !normalized.is_finite() || !(0.0..=1.0).contains(normalized) {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "normalized must be finite and between 0 and 1",
                        )
                        .with_path(format!("{event_path}.normalized")),
                    );
                }
                ProcessEventKind::ParameterChange {
                    parameter: handle,
                    normalized: *normalized,
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
    let audio = match render_instrument(Arc::clone(&compiled), request, &midi.events) {
        Ok(audio) => audio,
        Err(error) => return finish_failure(args.json, render_failure(&error)),
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
            output: args.output.to_string_lossy().into_owned(),
            backend: backend_info().version,
            diagnostics,
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
        output: args.output.to_string_lossy().into_owned(),
        backend: backend_info().version,
        diagnostics: Vec::new(),
    })
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
    };
    InspectProcessor {
        placement,
        chain_index,
        id: processor.id.clone(),
        kind,
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

fn inspect_sample_zones(
    sample: &sonalloy_core::compiler::CompiledSample,
) -> (Vec<InspectSampleZone>, usize) {
    let mut unique_sources: Vec<Arc<[f32]>> = Vec::new();
    let zones = sample
        .zones
        .iter()
        .map(|zone| {
            let metadata = zone.source.as_ref().map(|source| {
                if !unique_sources
                    .iter()
                    .any(|candidate| Arc::ptr_eq(candidate, &source.samples))
                {
                    unique_sources.push(Arc::clone(&source.samples));
                }
                &source.source_metadata
            });
            let (playback_type, start_frame, end_frame, loop_start_frame, loop_end_frame) =
                match zone.playback {
                    sonalloy_core::compiler::CompiledSamplePlayback::OneShot {
                        start_frame,
                        end_frame,
                    } => ("one_shot", start_frame, end_frame, None, None),
                    sonalloy_core::compiler::CompiledSamplePlayback::ForwardLoop {
                        start_frame,
                        end_frame,
                        loop_start_frame,
                        loop_end_frame,
                    } => (
                        "forward_loop",
                        start_frame,
                        end_frame,
                        Some(loop_start_frame),
                        Some(loop_end_frame),
                    ),
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
                start_frame,
                end_frame,
                loop_start_frame,
                loop_end_frame,
                source_sample_rate: metadata.map(|value| value.source_sample_rate),
                source_channels: metadata.map(|value| value.source_channels),
                prepared_frames: zone.source.as_ref().map(|source| source.samples.len()),
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
        sonalloy_core::compiler::CompiledGenerator::Wavetable(wavetable) => {
            inspect_wavetable_generator(compiled, generator, wavetable)
        }
        sonalloy_core::compiler::CompiledGenerator::OperatorModulation(operator) => {
            inspect_operator_generator(compiled, generator, operator)
        }
    }
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
            effective_max_frequency_hz: sonalloy_core::compiler::wavetable_effective_max_frequency(
                compiled.process_sample_rate,
            ),
        },
        if prepared.is_some() {
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
        .map(|route| InspectRoute {
            source: source_id(compiled, route.source),
            target: compiled
                .parameter_descriptor(route.target)
                .expect("compiled route target handle must be valid")
                .id
                .clone(),
            amount: route.amount,
            curve: route.curve,
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
            .map(|parameter| InspectParameter {
                id: parameter.id.clone(),
                owner: parameter.owner,
                unit: parameter.unit,
                min: parameter.min,
                max: parameter.max,
                default: parameter.default,
                scale: parameter.scale,
                smoothing_seconds: parameter.smoothing_seconds,
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
    for layer in &report.layers {
        print_generator(&layer.id, &layer.generator);
        println!(
            "  trigger: key {}..{} velocity {}..{}",
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
        println!("  smoothing: {:.3} s", parameter.smoothing_seconds);
    }
    for source in &report.sources {
        println!("source {}: {} ({})", source.id, source.kind, source.scope);
    }
    for route in &report.routes {
        println!(
            "route {} -> {} amount {:.3} curve {:?}",
            route.source, route.target, route.amount, route.curve
        );
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
                    "  zone {}: enabled {} key {}..{} velocity {}..{} root_note {} playback {} frames {}..{}",
                    zone.id,
                    zone.enabled,
                    zone.key_min,
                    zone.key_max,
                    zone.velocity_min,
                    zone.velocity_max,
                    zone.root_note,
                    zone.playback_type,
                    zone.start_frame,
                    zone.end_frame,
                );
                if let (Some(loop_start), Some(loop_end)) =
                    (zone.loop_start_frame, zone.loop_end_frame)
                {
                    println!("    loop: {loop_start}..{loop_end}");
                }
                if let Some(group) = &zone.round_robin_group {
                    println!("    round_robin_group: {group}");
                }
            }
        }
        InspectGenerator::Wavetable { .. } => print_wavetable_generator(layer_id, generator),
        InspectGenerator::OperatorModulation { .. } => {
            print_operator_generator(layer_id, generator);
        }
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
        for field in &report.static_fields {
            println!("    {}: {:.3}", field.id, field.value);
        }
        for parameter in &report.parameters {
            println!("    parameter {}", parameter.id);
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
    let diagnostic = if code == 2 {
        Diagnostic::error(DiagnosticCode::ValueOutOfRange, error.to_string())
    } else {
        from_render_error(error)
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
