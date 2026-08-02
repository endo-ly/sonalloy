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
    RenderError, RenderRequest, ScheduledEvent, VoiceStealingDefinition, backend_info,
    compile_instrument, from_render_error, render_instrument, render_sine, seconds_to_frames,
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
    voice_filter: Option<InspectFilter>,
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
}

#[derive(Debug, Serialize)]
struct InspectTrigger {
    key_min: u8,
    key_max: u8,
    velocity_min: u8,
    velocity_max: u8,
}

#[derive(Debug, Serialize)]
struct InspectGenerator {
    kind: &'static str,
    waveform: &'static str,
    phase_reset: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    asset_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    root_note: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    playback_mode: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    interpolation: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_sample_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_channels: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
struct InspectFilter {
    cutoff_hz: f32,
    resonance: f32,
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
    let (compiled, mut diagnostics) =
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
    diagnostics.shrink_to_fit();
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

fn compile_event_sequence(
    sequence: &EventSequence,
    compiled: &CompiledInstrument,
    duration_frames: u64,
) -> Result<Vec<ScheduledEvent>, CliFailure> {
    let mut diagnostics = Vec::new();
    let mut events = Vec::with_capacity(sequence.events.len());
    for (index, entry) in sequence.events.iter().enumerate() {
        let event_path = format!("events[{index}]");
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
            }),
        }],
        voice_filter: Some(sonalloy_core::FilterDefinition {
            cutoff_hz: 12_000.0,
            resonance: 0.12,
        }),
        modulation: None,
    }
}

fn parameter_default(compiled: &CompiledInstrument, handle: ParameterHandle) -> f32 {
    compiled
        .parameter_descriptor(handle)
        .map_or(0.0, |descriptor| descriptor.default)
}

fn inspect_source(source: &sonalloy_core::compiler::CompiledSource) -> InspectSource {
    let mut result = InspectSource {
        id: source.id.clone(),
        scope: match source.scope {
            sonalloy_core::compiler::SourceScope::Voice => "voice",
            sonalloy_core::compiler::SourceScope::Instrument => "instrument",
        },
        kind: "unknown",
        waveform: None,
        rate_hz: None,
        phase: None,
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
        sonalloy_core::compiler::CompiledVoiceSource::Envelope(_) => {
            result.kind = "envelope";
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
        sonalloy_core::compiler::CompiledSourceRef::Voice(handle) => {
            compiled.sources.get(handle.index()).map_or_else(
                || format!("source.{}", handle.index()),
                |value| value.id.clone(),
            )
        }
        sonalloy_core::compiler::CompiledSourceRef::PitchBend => "pitch_bend".to_owned(),
        sonalloy_core::compiler::CompiledSourceRef::ModWheel => "mod_wheel".to_owned(),
        sonalloy_core::compiler::CompiledSourceRef::Aftertouch => "aftertouch".to_owned(),
    }
}

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
            let (generator, asset_status) = match &layer.generator {
                sonalloy_core::compiler::CompiledGenerator::Oscillator(oscillator) => (
                    InspectGenerator {
                        kind: "oscillator",
                        waveform: match oscillator.waveform {
                            OscillatorWaveform::Sine => "sine",
                            OscillatorWaveform::Saw => "saw",
                        },
                        phase_reset: oscillator.phase_reset,
                        asset_path: None,
                        root_note: None,
                        playback_mode: None,
                        interpolation: None,
                        source_sample_rate: None,
                        source_channels: None,
                        prepared_frames: None,
                    },
                    "not_applicable (oscillator-only instrument)",
                ),
                sonalloy_core::compiler::CompiledGenerator::Sample(sample) => {
                    let metadata = sample.source.as_ref().map(|source| &source.source_metadata);
                    (
                        InspectGenerator {
                            kind: "sample",
                            waveform: "none",
                            phase_reset: false,
                            asset_path: Some(sample.asset_path.clone()),
                            root_note: Some(sample.root_note),
                            playback_mode: Some("one_shot"),
                            interpolation: Some("cubic"),
                            source_sample_rate: metadata.map(|value| value.source_sample_rate),
                            source_channels: metadata.map(|value| value.source_channels),
                            prepared_frames: sample
                                .source
                                .as_ref()
                                .map(|source| source.samples.len()),
                        },
                        if sample.enabled {
                            "enabled"
                        } else {
                            "disabled"
                        },
                    )
                }
            };
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
            }
        })
        .collect::<Vec<_>>();
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
        voice_filter: compiled.voice_filter.map(|filter| InspectFilter {
            cutoff_hz: filter.cutoff_hz,
            resonance: filter.resonance,
        }),
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
        sources: compiled.sources.iter().map(inspect_source).collect(),
        routes: compiled
            .routes
            .iter()
            .map(|route| InspectRoute {
                source: source_id(compiled, route.source),
                target: compiled.parameter_descriptor(route.target).map_or_else(
                    || format!("handle.{}", route.target.index()),
                    |value| value.id.clone(),
                ),
                amount: route.amount,
                curve: route.curve,
            })
            .collect(),
        diagnostics,
    }
}

fn print_inspect(compiled: &CompiledInstrument, diagnostics: &[Diagnostic]) {
    println!("metadata.name: {}", compiled.metadata.name);
    println!(
        "metadata.author: {}",
        compiled.metadata.author.as_deref().unwrap_or("none")
    );
    println!(
        "metadata.description: {}",
        compiled.metadata.description.as_deref().unwrap_or("none")
    );
    println!("polyphony: {}", compiled.performance.polyphony);
    println!("voice stealing: quietest_releasing_then_oldest");
    for layer in &compiled.layers {
        let asset_status = match &layer.generator {
            sonalloy_core::compiler::CompiledGenerator::Oscillator(oscillator) => {
                println!(
                    "layer {}: enabled true generator oscillator/{} phase_reset {}",
                    layer.id,
                    match oscillator.waveform {
                        OscillatorWaveform::Sine => "sine",
                        OscillatorWaveform::Saw => "saw",
                    },
                    oscillator.phase_reset
                );
                "not_applicable (oscillator-only instrument)"
            }
            sonalloy_core::compiler::CompiledGenerator::Sample(sample) => {
                println!(
                    "layer {}: enabled true generator sample/one_shot interpolation cubic",
                    layer.id
                );
                println!("  sample asset: {}", sample.asset_path);
                println!("  sample root_note: {}", sample.root_note);
                if let Some(source) = &sample.source {
                    println!(
                        "  sample prepared: {} frames at {} Hz from {} channels",
                        source.samples.len(),
                        source.sample_rate,
                        source.source_metadata.source_channels
                    );
                }
                if sample.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            }
        };
        println!(
            "  trigger: key {}..{} velocity {}..{}",
            layer.trigger.key_min,
            layer.trigger.key_max,
            layer.trigger.velocity_min,
            layer.trigger.velocity_max,
        );
        println!("  asset: {asset_status}");
        println!(
            "  gain: {:.3} dB ({:.6} linear) pan: {:.3}",
            parameter_default(compiled, layer.parameters.gain),
            10.0_f32.powf(parameter_default(compiled, layer.parameters.gain) / 20.0),
            parameter_default(compiled, layer.parameters.pan)
        );
        println!(
            "  tuning: {:.3} cents ({:.6} ratio)",
            parameter_default(compiled, layer.parameters.tuning),
            2.0_f32.powf(parameter_default(compiled, layer.parameters.tuning) / 1200.0)
        );
        println!(
            "  envelope: attack {} samples decay {} samples sustain {:.3} release {} samples",
            layer.envelope.attack_samples,
            layer.envelope.decay_samples,
            layer.envelope.sustain_level,
            layer.envelope.release_samples
        );
    }
    if let Some(filter) = compiled.voice_filter {
        println!(
            "voice filter: cutoff {:.2} Hz resonance {:.3}",
            filter.cutoff_hz, filter.resonance
        );
    } else {
        println!("voice filter: none");
    }
    for parameter in compiled.parameters() {
        println!("parameter {}:", parameter.id);
        println!("  owner: {:?}", parameter.owner);
        println!("  unit: {:?}", parameter.unit);
        println!("  range: {:.3} .. {:.3}", parameter.min, parameter.max);
        println!("  default: {:.3}", parameter.default);
        println!("  scale: {:?}", parameter.scale);
        println!("  smoothing: {:.3} s", parameter.smoothing_seconds);
    }
    for source in &compiled.sources {
        println!(
            "source {}: {:?} ({:?})",
            source.id, source.source, source.scope
        );
    }
    for route in &compiled.routes {
        println!(
            "route {} -> {} amount {:.3} curve {:?}",
            source_id(compiled, route.source),
            compiled.parameter_descriptor(route.target).map_or_else(
                || format!("handle.{}", route.target.index()),
                |value| value.id.clone()
            ),
            route.amount,
            route.curve
        );
    }
    print_warnings(diagnostics);
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
