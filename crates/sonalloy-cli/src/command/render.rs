use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use sonalloy_core::{
    AudioAnalysis, AudioAnalysisOptions, CompiledInstrument, DEFAULT_TEMPO_BPM, Diagnostic,
    DiagnosticCode, MusicalTimeMap, ProcessEventKind, RenderRequest, RenderTraceReport,
    ScheduledEvent, TraceRequest, analyze_rendered_audio, backend_info, prepare_audio_file,
    render_instrument_with_input, render_instrument_with_input_and_reset,
    render_instrument_with_input_and_trace, seconds_to_frames,
};

use super::demo::DEMO_JSON_HELP;
use super::{DEFAULT_BLOCK_SIZE, DEFAULT_SAMPLE_RATE, load_and_compile};
use crate::command::pattern::load_pattern;
use crate::demo::{self, FfmpegError, MasterReport, StereoMix, encode_mp3, master as master_demo};
use crate::midi::read_midi;
use crate::output::{
    CliFailure, ResetComparison, SuccessReport, finish_failure, input_failure, print_success,
    print_warnings, render_failure, write_wav,
};
use crate::pattern::{CompiledPattern, compile as compile_pattern};

const RENDER_EVENTS_HELP: &str = r"Render a JSON Event Sequence at exact absolute frame positions. The top-level object has an `events` array. Each entry requires `absolute_frame` (frames from render start) and `type`. Frames must be in ascending order and each must be less than `--duration-frames`; entries at the same frame are allowed.

Supported events and fields:
- `note_on`: `note_id` (a non-negative integer identifying the note), `note` (0..=127), and `velocity` (1..=127).
- `note_off`: `note_id` identifying the note to release.
- `sustain_pedal`: boolean `down`.
- `parameter_change`: `parameter` (an ID in the selected Instrument's Parameter catalog) and `native_value` (a finite value in that Parameter's native unit and range).
- `pitch_bend`: finite `value` in -1..=1.
- `mod_wheel` and `aftertouch`: finite `value` in 0..=1.

`--duration-frames` is the main render duration. `--tail` adds audio after that duration; events cannot be placed in the tail. `--tempo` supplies one constant BPM for tempo-synced parameters. `--reset-check` renders the sequence again after resetting the instrument and cannot be combined with `--trace`. `--trace-every-frames` requires at least one `--trace` parameter.";

const RENDER_DEMO_HELP: &str = r"Render every Part over the Demo timeline, apply Part gain and the configured global fade, and write the final Stereo WAV. The fade must not exceed the duration of the rendered mix, including any `--tail`. `--sample-rate` and `--block-size` are shared by all Parts; both values must be positive. `--tail` adds time after each Pattern. `--stems-dir` writes each Part before gain, global fade, and mastering. `--analyze` reports the fade-applied mix before mastering. The Demo `mix.master` setting applies to the final WAV. `--mp3-output` requires `FFmpeg`; with mastering configured, it encodes the mastered audio, otherwise it encodes the mix. Use `--json` for machine-readable success and structured diagnostics for execution failures.";

#[derive(Debug, Subcommand)]
pub(super) enum RenderCommand {
    /// Render one Note On / Note Off pair.
    Note(RenderNoteArgs),
    /// Render a JSON event sequence at absolute frame positions.
    #[command(long_about = RENDER_EVENTS_HELP)]
    Events(RenderEventsArgs),
    /// Render the events in a Standard MIDI File.
    #[command(
        long_about = "Read a Standard MIDI File and render its events. MIDI tempo changes and time-signature changes determine the playback timeline; absent tempo or meter metadata uses 120 BPM or 4/4. `--tail` adds seconds after the MIDI timeline. External audio, analysis, tracing, WAV output, and JSON reporting follow the same rules as the other `render` commands."
    )]
    Midi(RenderMidiArgs),
    /// Render a tick-based Pattern using its tempo and time-signature changes.
    #[command(
        long_about = "Render a Pattern's tick-based timeline using its `tempo_changes` and `time_signature_changes`. `parameter_change` events are resolved against the selected Instrument's Parameter catalog and must use a known Parameter ID and an allowed native value. `--tail` adds seconds after the Pattern duration. External audio, analysis, tracing, WAV output, and JSON reporting follow the same rules as the other `render` commands."
    )]
    Pattern(RenderPatternArgs),
    /// Render and mix every Part in a Demo.
    #[command(
        long_about = RENDER_DEMO_HELP,
        after_long_help = DEMO_JSON_HELP
    )]
    Demo(RenderDemoArgs),
}
#[derive(Debug, Args)]
struct OfflineRenderCommonArgs {
    /// Definition JSON path.
    #[arg(value_name = "DEFINITION")]
    definition: PathBuf,
    /// Mono/stereo external Audio WAV. Required by Definitions that use it; rejected otherwise.
    /// Resampled to `--sample-rate`; silence is used after the input ends.
    #[arg(long, value_name = "WAV")]
    audio_input: Option<PathBuf>,
    /// Output sample rate in Hz; must be greater than zero.
    #[arg(long, value_name = "HZ", default_value_t = DEFAULT_SAMPLE_RATE, value_parser = clap::value_parser!(u32).range(1..))]
    sample_rate: u32,
    /// Maximum process block size in frames; must be greater than zero.
    #[arg(long, value_name = "FRAMES", default_value_t = DEFAULT_BLOCK_SIZE, value_parser = super::parse_positive_usize)]
    block_size: usize,
    /// Destination WAV path.
    #[arg(long, value_name = "PATH")]
    output: PathBuf,
    /// Emit machine-readable JSON results; execution failures include structured diagnostics.
    #[arg(long)]
    json: bool,
    /// Analyze the latency-corrected WAV for levels, DC, activity, continuity, stereo, and spectrum.
    #[arg(long)]
    analyze: bool,
    /// Trace an existing Dynamic Parameter ID; may be repeated. An unknown ID fails.
    #[arg(long = "trace", value_name = "PARAMETER_ID")]
    trace: Vec<String>,
    /// Trace interval in frames (positive; default: 480 when tracing).
    #[arg(long = "trace-every-frames", value_name = "FRAMES", requires = "trace", value_parser = super::parse_positive_usize)]
    trace_every_frames: Option<usize>,
}

#[derive(Debug, Args)]
pub(super) struct RenderNoteArgs {
    #[command(flatten)]
    common: OfflineRenderCommonArgs,
    /// MIDI note number (0..=127).
    #[arg(long, value_name = "MIDI_NOTE", default_value_t = 60, value_parser = clap::value_parser!(u8).range(0..=127))]
    note: u8,
    /// MIDI velocity (1..=127).
    #[arg(long, value_name = "VELOCITY", default_value_t = 100, value_parser = clap::value_parser!(u8).range(1..=127))]
    velocity: u8,
    /// Note On duration in seconds; finite and greater than zero.
    #[arg(long, value_name = "SECONDS", default_value_t = 0.5, value_parser = super::parse_positive_f64)]
    gate: f64,
    /// Additional tail in seconds; finite and non-negative.
    #[arg(long, value_name = "SECONDS", default_value_t = 0.5, value_parser = super::parse_nonnegative_f64)]
    tail: f64,
    /// Constant tempo for tempo-synced parameters, in BPM (finite and greater than zero).
    #[arg(long, value_name = "BPM", default_value_t = DEFAULT_TEMPO_BPM, value_parser = super::parse_positive_f64)]
    tempo: f64,
}

#[derive(Debug, Args)]
pub(super) struct RenderMidiArgs {
    #[command(flatten)]
    common: OfflineRenderCommonArgs,
    /// Standard MIDI File path.
    #[arg(value_name = "MIDI_FILE")]
    midi: PathBuf,
    /// Additional tail after the MIDI timeline, in seconds (finite and non-negative).
    #[arg(long, value_name = "SECONDS", default_value_t = 1.0, value_parser = super::parse_nonnegative_f64)]
    tail: f64,
}

#[derive(Debug, Args)]
pub(super) struct RenderPatternArgs {
    #[command(flatten)]
    common: OfflineRenderCommonArgs,
    /// Musical-time pattern JSON path.
    #[arg(value_name = "PATTERN")]
    pattern: PathBuf,
    /// Additional tail after the Pattern timeline, in seconds (finite and non-negative).
    #[arg(long, value_name = "SECONDS", default_value_t = 1.0, value_parser = super::parse_nonnegative_f64)]
    tail: f64,
}

#[derive(Debug, Args)]
pub(super) struct RenderDemoArgs {
    /// Demo JSON path.
    #[arg(value_name = "DEMO")]
    demo: PathBuf,
    /// Sample rate in Hz shared by all parts; must be greater than zero.
    #[arg(long, value_name = "HZ", default_value_t = DEFAULT_SAMPLE_RATE, value_parser = clap::value_parser!(u32).range(1..))]
    sample_rate: u32,
    /// Maximum process block size in frames shared by all parts; must be greater than zero.
    #[arg(long, value_name = "FRAMES", default_value_t = DEFAULT_BLOCK_SIZE, value_parser = super::parse_positive_usize)]
    block_size: usize,
    /// Additional tail after each Part Pattern, in seconds (finite and non-negative).
    #[arg(long, value_name = "SECONDS", default_value_t = 1.0, value_parser = super::parse_nonnegative_f64)]
    tail: f64,
    /// Write each Part's WAV before Part gain, global fade, and mastering are applied.
    #[arg(long, value_name = "DIRECTORY")]
    stems_dir: Option<PathBuf>,
    /// Optional MP3 output; requires `FFmpeg` and uses Demo mastering when configured.
    #[arg(long, value_name = "PATH")]
    mp3_output: Option<PathBuf>,
    /// Analyze the fade-applied mix before mastering.
    #[arg(long)]
    analyze: bool,
    /// Destination final Stereo WAV path.
    #[arg(long, value_name = "PATH")]
    output: PathBuf,
    /// Emit machine-readable JSON results; execution failures include structured diagnostics.
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
pub(super) struct RenderEventsArgs {
    #[command(flatten)]
    common: OfflineRenderCommonArgs,
    /// Event Sequence JSON path; see command help for its structure and event fields.
    #[arg(value_name = "EVENTS_JSON")]
    events: PathBuf,
    /// Main render duration in frames.
    #[arg(long, value_name = "FRAMES")]
    duration_frames: u64,
    /// Additional tail after the main duration, in seconds (finite and non-negative).
    #[arg(long, value_name = "SECONDS", default_value_t = 1.0, value_parser = super::parse_nonnegative_f64)]
    tail: f64,
    /// Constant tempo for tempo-synced parameters, in BPM (finite and greater than zero).
    #[arg(long, value_name = "BPM", default_value_t = DEFAULT_TEMPO_BPM, value_parser = super::parse_positive_f64)]
    tempo: f64,
    /// Render the sequence again after resetting the instrument; cannot be combined with --trace.
    #[arg(long, conflicts_with = "trace")]
    reset_check: bool,
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
    SustainPedal {
        down: bool,
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

pub(super) fn run(command: RenderCommand) -> ExitCode {
    match command {
        RenderCommand::Note(args) => run_render_note(&args),
        RenderCommand::Events(args) => run_render_events(&args),
        RenderCommand::Midi(args) => run_render_midi(&args),
        RenderCommand::Pattern(args) => run_render_pattern(&args),
        RenderCommand::Demo(args) => run_render_demo(&args),
    }
}

fn run_render_note(args: &RenderNoteArgs) -> ExitCode {
    let common = &args.common;
    let sample_rate = f64::from(common.sample_rate);
    let (gate_frames, tail_frames, duration_frames) = match note_render_timing(args, sample_rate) {
        Ok(timing) => timing,
        Err(failure) => return finish_failure(common.json, failure),
    };
    let (compiled, diagnostics) =
        match load_and_compile(&common.definition, common.sample_rate, common.block_size) {
            Ok(result) => result,
            Err(failure) => return finish_failure(common.json, failure),
        };
    let trace_request =
        match resolve_trace_request(&compiled, &common.trace, common.trace_every_frames) {
            Ok(request) => request,
            Err(failure) => return finish_failure(common.json, failure),
        };
    let external_audio =
        match load_external_audio(common.audio_input.as_deref(), common.sample_rate) {
            Ok(audio) => audio,
            Err(failure) => return finish_failure(common.json, failure),
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
        block_size: common.block_size,
        duration_frames,
        tail_frames,
    };
    let request = match extend_request_for_latency(request, compiled.reported_latency_frames) {
        Ok(request) => request,
        Err(failure) => return finish_failure(common.json, failure),
    };
    let musical_time_map = match MusicalTimeMap::constant(args.tempo) {
        Ok(musical_time_map) => musical_time_map,
        Err(error) => return finish_failure(common.json, render_failure(&error)),
    };
    let rendered = match render_offline_audio(
        &compiled,
        request,
        &events,
        &musical_time_map,
        trace_request.as_ref(),
        false,
        external_audio.as_ref(),
    ) {
        Ok(rendered) => rendered,
        Err(failure) => return finish_failure(common.json, failure),
    };
    write_offline_render(
        common,
        &compiled,
        diagnostics,
        rendered,
        Some(note_frequency_hz(args.note)),
    )
}

fn note_render_timing(
    args: &RenderNoteArgs,
    sample_rate: f64,
) -> Result<(u64, u64, u64), CliFailure> {
    let gate_frames =
        seconds_to_frames(args.gate, sample_rate).map_err(|error| input_failure(&error))?;
    let tail_frames =
        seconds_to_frames(args.tail, sample_rate).map_err(|error| input_failure(&error))?;
    let duration_frames = gate_frames.checked_add(1).ok_or_else(|| CliFailure {
        code: 2,
        diagnostics: vec![Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            "render duration overflows the frame counter",
        )],
    })?;
    Ok((gate_frames, tail_frames, duration_frames))
}

fn run_render_events(args: &RenderEventsArgs) -> ExitCode {
    let common = &args.common;
    let sample_rate = f64::from(common.sample_rate);
    let tail_frames = match seconds_to_frames(args.tail, sample_rate) {
        Ok(frames) => frames,
        Err(error) => return finish_failure(common.json, input_failure(&error)),
    };
    let (compiled, diagnostics) =
        match load_and_compile(&common.definition, common.sample_rate, common.block_size) {
            Ok(result) => result,
            Err(failure) => return finish_failure(common.json, failure),
        };
    let external_audio =
        match load_external_audio(common.audio_input.as_deref(), common.sample_rate) {
            Ok(audio) => audio,
            Err(failure) => return finish_failure(common.json, failure),
        };
    let trace_request =
        match resolve_trace_request(&compiled, &common.trace, common.trace_every_frames) {
            Ok(request) => request,
            Err(failure) => return finish_failure(common.json, failure),
        };
    let sequence = match load_event_sequence(&args.events) {
        Ok(sequence) => sequence,
        Err(failure) => return finish_failure(common.json, failure),
    };
    let events = match compile_event_sequence(&sequence, &compiled, args.duration_frames) {
        Ok(events) => events,
        Err(failure) => return finish_failure(common.json, failure),
    };
    let request = RenderRequest {
        sample_rate,
        block_size: common.block_size,
        duration_frames: args.duration_frames,
        tail_frames,
    };
    let request = match extend_request_for_latency(request, compiled.reported_latency_frames) {
        Ok(request) => request,
        Err(failure) => return finish_failure(common.json, failure),
    };
    let musical_time_map = match MusicalTimeMap::constant(args.tempo) {
        Ok(musical_time_map) => musical_time_map,
        Err(error) => return finish_failure(common.json, render_failure(&error)),
    };
    let rendered = match render_offline_audio(
        &compiled,
        request,
        &events,
        &musical_time_map,
        trace_request.as_ref(),
        args.reset_check,
        external_audio.as_ref(),
    ) {
        Ok(rendered) => rendered,
        Err(failure) => return finish_failure(common.json, failure),
    };
    write_offline_render(common, &compiled, diagnostics, rendered, None)
}

fn write_offline_render(
    common: &OfflineRenderCommonArgs,
    compiled: &CompiledInstrument,
    diagnostics: Vec<Diagnostic>,
    rendered: (
        sonalloy_core::RenderedAudio,
        Option<RenderTraceReport>,
        Option<ResetComparison>,
    ),
    reference_frequency_hz: Option<f32>,
) -> ExitCode {
    let (mut audio, trace, reset_comparison) = rendered;
    correct_rendered_audio(&mut audio, compiled.reported_latency_frames);
    write_offline_render_result(
        common,
        compiled,
        diagnostics,
        &audio,
        trace,
        reset_comparison,
        reference_frequency_hz,
    )
}

fn write_offline_render_result(
    common: &OfflineRenderCommonArgs,
    compiled: &CompiledInstrument,
    diagnostics: Vec<Diagnostic>,
    audio: &sonalloy_core::RenderedAudio,
    trace: Option<RenderTraceReport>,
    reset_comparison: Option<ResetComparison>,
    reference_frequency_hz: Option<f32>,
) -> ExitCode {
    let analysis = if common.analyze {
        match analyze_audio(audio, reference_frequency_hz) {
            Ok(analysis) => Some(analysis),
            Err(failure) => return finish_failure(common.json, failure),
        }
    } else {
        None
    };
    if let Err(error) = write_wav(&common.output, audio) {
        return finish_failure(
            common.json,
            CliFailure {
                code: 4,
                diagnostics: vec![error],
            },
        );
    }
    print_success(
        common.json,
        SuccessReport {
            status: "ok",
            sample_rate: audio.sample_rate,
            channels: audio.channels.len(),
            frames: audio.frames(),
            reported_latency_frames: compiled.reported_latency_frames,
            output: common.output.to_string_lossy().into_owned(),
            backend: backend_info().version,
            diagnostics,
            analysis,
            trace,
            reset_comparison,
        },
    )
}

pub(crate) fn render_compiled_pattern(
    compiled: &Arc<CompiledInstrument>,
    pattern: &CompiledPattern,
    sample_rate: f64,
    block_size: usize,
    tail_frames: u64,
    trace_request: Option<&TraceRequest>,
    external_audio: Option<&sonalloy_core::PreparedAudio>,
) -> Result<(sonalloy_core::RenderedAudio, Option<RenderTraceReport>), CliFailure> {
    let request = RenderRequest {
        sample_rate,
        block_size,
        duration_frames: pattern.one_shot_duration_frames,
        tail_frames,
    };
    let request = extend_request_for_latency(request, compiled.reported_latency_frames)?;
    let (mut audio, trace, _) = render_offline_audio(
        compiled,
        request,
        &pattern.events,
        &pattern.musical_time_map,
        trace_request,
        false,
        external_audio,
    )?;
    correct_rendered_audio(&mut audio, compiled.reported_latency_frames);
    Ok((audio, trace))
}

fn render_offline_audio(
    compiled: &Arc<CompiledInstrument>,
    request: RenderRequest,
    events: &[ScheduledEvent],
    musical_time_map: &MusicalTimeMap,
    trace_request: Option<&TraceRequest>,
    reset_check: bool,
    external_audio: Option<&sonalloy_core::PreparedAudio>,
) -> Result<
    (
        sonalloy_core::RenderedAudio,
        Option<RenderTraceReport>,
        Option<ResetComparison>,
    ),
    CliFailure,
> {
    if reset_check {
        let (first, second) = render_instrument_with_input_and_reset(
            Arc::clone(compiled),
            request,
            events,
            musical_time_map,
            external_audio,
        )
        .map_err(|error| render_failure(&error))?;
        let comparison = compare_rendered_audio(&first, &second);
        return Ok((second, None, Some(comparison)));
    }
    if let Some(trace_request) = trace_request {
        let (audio, trace) = render_instrument_with_input_and_trace(
            Arc::clone(compiled),
            request,
            events,
            musical_time_map,
            trace_request,
            external_audio,
        )
        .map_err(|error| render_failure(&error))?;
        return Ok((audio, Some(trace), None));
    }
    let audio = render_instrument_with_input(
        Arc::clone(compiled),
        request,
        events,
        musical_time_map,
        external_audio,
    )
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
            EventSequenceKind::SustainPedal { down } => {
                ProcessEventKind::SustainPedal { down: *down }
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
                    catalog_revision: compiled.parameter_catalog_revision(),
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
    let common = &args.common;
    let sample_rate = f64::from(common.sample_rate);
    let tail_frames = match seconds_to_frames(args.tail, sample_rate) {
        Ok(frames) => frames,
        Err(error) => return finish_failure(common.json, input_failure(&error)),
    };
    let (compiled, mut diagnostics) =
        match load_and_compile(&common.definition, common.sample_rate, common.block_size) {
            Ok(result) => result,
            Err(failure) => return finish_failure(common.json, failure),
        };
    let trace_request =
        match resolve_trace_request(&compiled, &common.trace, common.trace_every_frames) {
            Ok(request) => request,
            Err(failure) => return finish_failure(common.json, failure),
        };
    let external_audio =
        match load_external_audio(common.audio_input.as_deref(), common.sample_rate) {
            Ok(audio) => audio,
            Err(failure) => return finish_failure(common.json, failure),
        };
    let midi = match read_midi(&args.midi, sample_rate) {
        Ok(midi) => midi,
        Err(midi_diagnostics) => {
            return finish_failure(
                common.json,
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
        block_size: common.block_size,
        duration_frames: midi.duration_frames,
        tail_frames,
    };
    let request = match extend_request_for_latency(request, compiled.reported_latency_frames) {
        Ok(request) => request,
        Err(failure) => return finish_failure(common.json, failure),
    };
    let rendered = match render_offline_audio(
        &compiled,
        request,
        &midi.events,
        &midi.musical_time_map,
        trace_request.as_ref(),
        false,
        external_audio.as_ref(),
    ) {
        Ok(rendered) => rendered,
        Err(failure) => return finish_failure(common.json, failure),
    };
    write_offline_render(common, &compiled, diagnostics, rendered, None)
}

fn run_render_pattern(args: &RenderPatternArgs) -> ExitCode {
    let common = &args.common;
    let sample_rate = f64::from(common.sample_rate);
    let tail_frames = match seconds_to_frames(args.tail, sample_rate) {
        Ok(frames) => frames,
        Err(error) => return finish_failure(common.json, input_failure(&error)),
    };
    let (compiled, diagnostics) =
        match load_and_compile(&common.definition, common.sample_rate, common.block_size) {
            Ok(result) => result,
            Err(failure) => return finish_failure(common.json, failure),
        };
    let pattern = match load_pattern(&args.pattern) {
        Ok(pattern) => pattern,
        Err(failure) => return finish_failure(common.json, failure),
    };
    let compiled_pattern = match compile_pattern(&pattern, &compiled, sample_rate) {
        Ok(compiled_pattern) => compiled_pattern,
        Err(diagnostics) => {
            return finish_failure(
                common.json,
                CliFailure {
                    code: 2,
                    diagnostics,
                },
            );
        }
    };
    let trace_request =
        match resolve_trace_request(&compiled, &common.trace, common.trace_every_frames) {
            Ok(request) => request,
            Err(failure) => return finish_failure(common.json, failure),
        };
    let external_audio =
        match load_external_audio(common.audio_input.as_deref(), common.sample_rate) {
            Ok(audio) => audio,
            Err(failure) => return finish_failure(common.json, failure),
        };
    let (audio, trace) = match render_compiled_pattern(
        &compiled,
        &compiled_pattern,
        sample_rate,
        common.block_size,
        tail_frames,
        trace_request.as_ref(),
        external_audio.as_ref(),
    ) {
        Ok(rendered) => rendered,
        Err(failure) => return finish_failure(common.json, failure),
    };
    write_offline_render_result(common, &compiled, diagnostics, &audio, trace, None, None)
}

#[allow(clippy::too_many_lines)]
fn run_render_demo(args: &RenderDemoArgs) -> ExitCode {
    let sample_rate = f64::from(args.sample_rate);
    let tail_frames = match seconds_to_frames(args.tail, sample_rate) {
        Ok(frames) => frames,
        Err(error) => return finish_failure(args.json, input_failure(&error)),
    };
    let demo = match demo::load(&args.demo, args.sample_rate, args.block_size) {
        Ok(demo) => demo,
        Err(failure) => return finish_failure(args.json, failure),
    };
    if let Some(stems_dir) = &args.stems_dir
        && let Err(error) = std::fs::create_dir_all(stems_dir)
    {
        return finish_failure(
            args.json,
            CliFailure {
                code: 4,
                diagnostics: vec![
                    Diagnostic::error(
                        DiagnosticCode::WavOutputError,
                        "could not create stems directory",
                    )
                    .with_path(stems_dir.to_string_lossy())
                    .with_detail(error.to_string()),
                ],
            },
        );
    }

    let mut mix = StereoMix::new(args.sample_rate);
    let mut part_reports = Vec::with_capacity(demo.parts.len());
    for (index, part) in demo.parts.iter().enumerate() {
        let (audio, _) = match render_compiled_pattern(
            &part.instrument,
            &part.compiled_pattern,
            sample_rate,
            args.block_size,
            tail_frames,
            None,
            None,
        ) {
            Ok(audio) => audio,
            Err(failure) => return finish_failure(args.json, prefix_part_failure(failure, index)),
        };
        let stem = if let Some(stems_dir) = &args.stems_dir {
            let stem_path = stems_dir.join(format!("{}.wav", part.definition.id));
            if let Err(error) = write_wav(&stem_path, &audio) {
                return finish_failure(
                    args.json,
                    CliFailure {
                        code: 4,
                        diagnostics: vec![error],
                    },
                );
            }
            Some(stem_path.to_string_lossy().into_owned())
        } else {
            None
        };
        let frames = audio.frames();
        if let Err(error) = mix.add_part(&audio, part.definition.gain_db) {
            return finish_failure(
                args.json,
                CliFailure {
                    code: 3,
                    diagnostics: vec![
                        Diagnostic::error(DiagnosticCode::RenderError, error)
                            .with_path(format!("parts[{index}]")),
                    ],
                },
            );
        }
        part_reports.push(DemoPartRenderReport {
            id: part.definition.id.clone(),
            gain_db: part.definition.gain_db,
            frames,
            stem,
        });
    }
    if let Err(error) = mix.apply_fade(demo.definition.mix.fade_out_seconds) {
        return finish_failure(
            args.json,
            CliFailure {
                code: 3,
                diagnostics: vec![
                    Diagnostic::error(DiagnosticCode::RenderError, error)
                        .with_path("mix.fade_out_seconds"),
                ],
            },
        );
    }
    let mix_audio = mix.into_audio();
    let mix_analysis = if args.analyze {
        match analyze_audio(&mix_audio, None) {
            Ok(analysis) => Some(analysis),
            Err(failure) => return finish_failure(args.json, failure),
        }
    } else {
        None
    };

    let master_report = if let Some(settings) = &demo.definition.mix.master {
        let temporary_mix = match tempfile::NamedTempFile::new() {
            Ok(file) => file,
            Err(error) => {
                return finish_failure(
                    args.json,
                    CliFailure {
                        code: 4,
                        diagnostics: vec![
                            Diagnostic::error(
                                DiagnosticCode::WavOutputError,
                                "could not create temporary Demo mix",
                            )
                            .with_detail(error.to_string()),
                        ],
                    },
                );
            }
        };
        if let Err(error) = write_wav(temporary_mix.path(), &mix_audio) {
            return finish_failure(
                args.json,
                CliFailure {
                    code: 4,
                    diagnostics: vec![error],
                },
            );
        }
        match master_demo(
            temporary_mix.path(),
            &args.output,
            settings,
            args.sample_rate,
        ) {
            Ok(report) => Some(report),
            Err(error) => return finish_failure(args.json, ffmpeg_failure(error)),
        }
    } else {
        if let Err(error) = write_wav(&args.output, &mix_audio) {
            return finish_failure(
                args.json,
                CliFailure {
                    code: 4,
                    diagnostics: vec![error],
                },
            );
        }
        None
    };

    if let Some(mp3_output) = &args.mp3_output
        && let Err(error) = encode_mp3(&args.output, mp3_output)
    {
        return finish_failure(args.json, ffmpeg_failure(error));
    }

    let report = DemoRenderReport {
        status: "ok",
        sample_rate: mix_audio.sample_rate,
        channels: mix_audio.channels.len(),
        frames: mix_audio.frames(),
        output: args.output.to_string_lossy().into_owned(),
        mp3_output: args
            .mp3_output
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        stems_dir: args
            .stems_dir
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        parts: part_reports,
        mix_analysis,
        master: master_report,
        diagnostics: demo.diagnostics.clone(),
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&report).expect("Demo render report is serializable")
        );
    } else {
        println!(
            "rendered {} frames at {} Hz to {}",
            report.frames, report.sample_rate, report.output
        );
        if let Some(mp3_output) = &report.mp3_output {
            println!("created {mp3_output}");
        }
        print_warnings(&report.diagnostics);
    }
    ExitCode::SUCCESS
}

fn prefix_part_failure(mut failure: CliFailure, part_index: usize) -> CliFailure {
    let prefix = format!("parts[{part_index}]");
    for diagnostic in &mut failure.diagnostics {
        diagnostic.path = Some(match diagnostic.path.take() {
            Some(path) => format!("{prefix}.{path}"),
            None => prefix.clone(),
        });
    }
    failure
}

fn ffmpeg_failure(error: FfmpegError) -> CliFailure {
    if error.not_found {
        CliFailure {
            code: 4,
            diagnostics: vec![
                Diagnostic::error(
                    DiagnosticCode::RenderError,
                    "FFmpeg is required for Demo mastering or MP3 output",
                )
                .with_detail("install ffmpeg and make it available on PATH"),
            ],
        }
    } else {
        CliFailure {
            code: 4,
            diagnostics: vec![
                Diagnostic::error(DiagnosticCode::RenderError, "FFmpeg Demo processing failed")
                    .with_detail(error.detail),
            ],
        }
    }
}

#[derive(Debug, Serialize)]
struct DemoRenderReport {
    status: &'static str,
    sample_rate: u32,
    channels: usize,
    frames: usize,
    output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mp3_output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stems_dir: Option<String>,
    parts: Vec<DemoPartRenderReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mix_analysis: Option<AudioAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    master: Option<MasterReport>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize)]
struct DemoPartRenderReport {
    id: String,
    gain_db: f64,
    frames: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    stem: Option<String>,
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
        return Ok(None);
    }
    let every_frames = every_frames.unwrap_or(480);
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

fn load_external_audio(
    path: Option<&Path>,
    sample_rate: u32,
) -> Result<Option<sonalloy_core::PreparedAudio>, CliFailure> {
    let Some(path) = path else {
        return Ok(None);
    };
    prepare_audio_file(path, f64::from(sample_rate))
        .map(Some)
        .map_err(|error| CliFailure {
            code: 2,
            diagnostics: vec![
                Diagnostic::error(
                    DiagnosticCode::AssetDecodeFailed,
                    "could not prepare external audio input",
                )
                .with_path(path.to_string_lossy())
                .with_detail(error.to_string()),
            ],
        })
}
