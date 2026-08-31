use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Subcommand};
use serde::Deserialize;
use sonalloy_core::{
    AudioAnalysis, AudioAnalysisOptions, CompiledInstrument, DEFAULT_TEMPO_BPM, Diagnostic,
    DiagnosticCode, MusicalTimeMap, ProcessEventKind, RenderRequest, RenderTraceReport,
    ScheduledEvent, TraceRequest, analyze_rendered_audio, backend_info, prepare_audio_file,
    render_instrument_with_input, render_instrument_with_input_and_reset,
    render_instrument_with_input_and_trace, seconds_to_frames,
};

use super::{DEFAULT_BLOCK_SIZE, DEFAULT_SAMPLE_RATE, load_and_compile};
use crate::command::pattern::load_pattern;
use crate::midi::read_midi;
use crate::output::{
    CliFailure, ResetComparison, SuccessReport, finish_failure, input_failure, print_success,
    render_failure, write_wav,
};
use crate::pattern::compile as compile_pattern;

#[derive(Debug, Subcommand)]
pub(super) enum RenderCommand {
    /// Render one Note On / Note Off pair.
    Note(RenderNoteArgs),
    /// Render an absolute-frame event sequence.
    Events(RenderEventsArgs),
    /// Render events from a Standard MIDI File.
    Midi(RenderMidiArgs),
    /// Render a musical-time audition pattern.
    Pattern(RenderPatternArgs),
}
#[derive(Debug, Args)]
struct OfflineRenderCommonArgs {
    /// Definition JSON path.
    definition: PathBuf,
    /// External audio input WAV path.
    #[arg(long)]
    audio_input: Option<PathBuf>,
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
pub(super) struct RenderNoteArgs {
    #[command(flatten)]
    common: OfflineRenderCommonArgs,
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
}

#[derive(Debug, Args)]
pub(super) struct RenderMidiArgs {
    #[command(flatten)]
    common: OfflineRenderCommonArgs,
    /// Standard MIDI File path.
    midi: PathBuf,
    /// Additional render tail in seconds.
    #[arg(long, default_value_t = 1.0)]
    tail: f64,
}

#[derive(Debug, Args)]
pub(super) struct RenderPatternArgs {
    #[command(flatten)]
    common: OfflineRenderCommonArgs,
    /// Musical-time pattern JSON path.
    pattern: PathBuf,
    /// Additional render tail in seconds.
    #[arg(long, default_value_t = 1.0)]
    tail: f64,
}
#[derive(Debug, Args)]
pub(super) struct RenderEventsArgs {
    #[command(flatten)]
    common: OfflineRenderCommonArgs,
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
    /// Render the same event sequence again after resetting the prepared runtime.
    #[arg(long)]
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
    if args.note > 127 {
        return Err(CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "note must be between 0 and 127",
            )],
        });
    }
    if args.velocity == 0 || args.velocity > 127 {
        return Err(CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "velocity must be between 1 and 127",
            )],
        });
    }
    if args.gate <= 0.0 {
        return Err(CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "gate must be greater than zero",
            )],
        });
    }
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
    let analysis = if common.analyze {
        match analyze_audio(&audio, reference_frequency_hz) {
            Ok(analysis) => Some(analysis),
            Err(failure) => return finish_failure(common.json, failure),
        }
    } else {
        None
    };
    if let Err(error) = write_wav(&common.output, &audio) {
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
        if trace_request.is_some() {
            return Err(CliFailure {
                code: 2,
                diagnostics: vec![Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "--reset-check cannot be combined with --trace",
                )],
            });
        }
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
    let request = RenderRequest {
        sample_rate,
        block_size: common.block_size,
        duration_frames: compiled_pattern.one_shot_duration_frames,
        tail_frames,
    };
    let request = match extend_request_for_latency(request, compiled.reported_latency_frames) {
        Ok(request) => request,
        Err(failure) => return finish_failure(common.json, failure),
    };
    let rendered = match render_offline_audio(
        &compiled,
        request,
        &compiled_pattern.events,
        &compiled_pattern.musical_time_map,
        trace_request.as_ref(),
        false,
        external_audio.as_ref(),
    ) {
        Ok(rendered) => rendered,
        Err(failure) => return finish_failure(common.json, failure),
    };
    write_offline_render(common, &compiled, diagnostics, rendered, None)
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
