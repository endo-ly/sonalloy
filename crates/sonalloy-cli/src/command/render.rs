use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use sonalloy_core::{
    AudioAnalysis, AudioAnalysisOptions, CompiledInstrument, DEFAULT_TEMPO_BPM, Diagnostic,
    DiagnosticCode, DiagnosticContext, MusicalTimeMap, PreparedAudioChannels, ProcessEventKind,
    RenderRequest, RenderTraceReport, ScheduledEvent, TraceRequest, analyze_rendered_audio,
    backend_info, prepare_audio_file, render_instrument_with_input,
    render_instrument_with_input_and_reset, render_instrument_with_input_and_trace,
    seconds_to_frames,
};

use super::{DEFAULT_BLOCK_SIZE, DEFAULT_SAMPLE_RATE, load_and_compile};
use crate::midi::read_midi;
use crate::output::{
    CliFailure, ResetComparison, SuccessReport, finish_failure, input_failure, print_success,
    print_warnings, render_failure, write_wav,
};
use crate::performance::events::CompiledEvents;
use crate::performance::{
    self, FfmpegError, LoudnessAnalysis, LoudnessMeasurement, MasterReport, StereoMix, encode_mp3,
    master as master_score,
};

const RENDER_EVENTS_HELP: &str = r"Render a JSON Event Sequence at exact absolute frame positions. The top-level object has an `events` array. Each entry requires `absolute_frame` (frames from render start) and `type`. Frames must be in ascending order and each must be less than `--duration-frames`; entries at the same frame are allowed.

Supported events and fields:
- `note_on`: `note_id` (a non-negative integer identifying the note), `note` (0..=127), and `velocity` (1..=127).
- `note_off`: `note_id` identifying the note to release.
- `sustain_pedal`: boolean `down`.
- `parameter_change`: `parameter` (an ID in the selected Instrument's Parameter catalog) and `native_value` (a finite value in that Parameter's native unit and range).
- `parameter_ramp`: `parameter`, native-unit `from_value` and `to_value`, and positive `duration_frames`. The ramp end must fit within `--duration-frames`.
- `pitch_bend`: finite `value` in -1..=1.
- `mod_wheel` and `aftertouch`: finite `value` in 0..=1.

`--duration-frames` is the main render duration. `--tail` adds audio after that duration; events cannot be placed in the tail. `--tempo` supplies one constant BPM for tempo-synced parameters. `--reset-check` renders the sequence again after resetting the instrument and cannot be combined with `--trace`. `--trace-every-frames` requires at least one `--trace` parameter.";

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
    /// Render and mix a Score YAML performance.
    #[command(
        long_about = "Render a Score to a final stereo WAV. A full-form render applies `meta.master` when present, and mastering requires FFmpeg even when loudness analysis and MP3 output are not requested. Section, bar, Part, and mute selections are partial renders and omit Score mastering unless `--apply-master` is set. A section occurrence is its one-based position in the entire form; use `score inspect --summary` to find the matching number. Preroll processes earlier bars before a selected range so stateful processors can settle."
    )]
    Score(RenderScoreArgs),
}
#[derive(Debug, Args)]
struct OfflineRenderCommonArgs {
    /// Definition JSON path.
    #[arg(value_name = "DEFINITION")]
    definition: PathBuf,
    /// Mono/stereo external Audio WAV, or `silence`. Required by Definitions that use it;
    /// rejected otherwise. Resampled to `--sample-rate`; silence is used after the input ends.
    #[arg(long, value_name = "WAV|silence")]
    audio_input: Option<String>,
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
    /// Analyze the latency-corrected WAV, including band energy and loudness; requires `FFmpeg`.
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
pub(super) struct RenderScoreArgs {
    /// Score YAML path.
    #[arg(value_name = "SCORE")]
    score: PathBuf,
    #[command(flatten)]
    bindings: crate::performance::InstrumentBindings,
    /// Select one named Section occurrence.
    #[arg(long, conflicts_with = "bars")]
    section: Option<String>,
    /// One-based position of the matching entry in the entire form.
    #[arg(long, requires = "section", value_parser = super::parse_positive_usize)]
    occurrence: Option<usize>,
    /// Inclusive global bar range, for example `49-56`.
    #[arg(long, conflicts_with = "section")]
    bars: Option<String>,
    /// Include only this Part in the mix; repeat for multiple Parts.
    #[arg(long = "part", value_name = "ID")]
    parts: Vec<String>,
    /// Exclude this Part from the mix; repeat for multiple Parts.
    #[arg(long = "mute", value_name = "ID")]
    mutes: Vec<String>,
    /// Begin processing this many bars before a selected range.
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(usize))]
    preroll_bars: usize,
    /// Apply Score mastering to a partial render.
    #[arg(long)]
    apply_master: bool,
    /// Sample rate in Hz shared by all parts; must be greater than zero.
    #[arg(long, value_name = "HZ", default_value_t = DEFAULT_SAMPLE_RATE, value_parser = clap::value_parser!(u32).range(1..))]
    sample_rate: u32,
    /// Maximum process block size in frames shared by all parts; must be greater than zero.
    #[arg(long, value_name = "FRAMES", default_value_t = DEFAULT_BLOCK_SIZE, value_parser = super::parse_positive_usize)]
    block_size: usize,
    /// Additional tail after each Part Pattern, in seconds (finite and non-negative).
    #[arg(long, value_name = "SECONDS", default_value_t = 1.0, value_parser = super::parse_nonnegative_f64)]
    tail: f64,
    /// Write each Part's WAV after Part gain and pan, before global fade and mastering are applied.
    #[arg(long, value_name = "DIRECTORY")]
    stems_dir: Option<PathBuf>,
    /// Write the Stereo mix after Part gain and global fade, before mastering.
    #[arg(long, value_name = "WAV")]
    premaster_output: Option<PathBuf>,
    /// Optional MP3 output; requires `FFmpeg` and uses Score mastering when configured.
    #[arg(long, value_name = "PATH")]
    mp3_output: Option<PathBuf>,
    /// Analyze premaster and final WAV, including loudness; requires `FFmpeg`.
    #[arg(long)]
    analyze: bool,
    /// Observe [`instrument_id`:]`parameter_id`; repeat for multiple parameters.
    #[arg(long = "trace", value_name = "PARAMETER_ID")]
    trace: Vec<String>,
    /// Positive observation interval in frames.
    #[arg(long,requires="trace",value_parser=super::parse_positive_usize)]
    trace_every_frames: Option<usize>,
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
    ParameterRamp {
        duration_frames: u64,
        parameter: String,
        from_value: f32,
        to_value: f32,
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
        RenderCommand::Score(args) => run_render_score(&args),
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
    let external_audio = match load_render_audio_input(common, &compiled) {
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
    let external_audio = match load_render_audio_input(common, &compiled) {
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
    let pending = match crate::output::pending_wav(&common.output) {
        Ok(path) => path,
        Err(failure) => return finish_failure(common.json, failure),
    };
    if let Err(error) = write_wav(&pending, audio) {
        return finish_failure(
            common.json,
            CliFailure {
                code: 4,
                diagnostics: vec![error],
            },
        );
    }
    let loudness = if common.analyze {
        match performance::analyze_loudness(&pending) {
            Ok(loudness) => Some(loudness),
            Err(error) => return finish_failure(common.json, ffmpeg_failure(error)),
        }
    } else {
        None
    };
    if let Err(failure) = crate::output::commit_wav(pending, &common.output) {
        return finish_failure(common.json, failure);
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
            analysis: analysis
                .zip(loudness)
                .map(|(audio, loudness)| crate::output::OfflineAnalysis { audio, loudness }),
            trace,
            reset_comparison,
        },
    )
}

pub(crate) fn render_compiled_pattern(
    compiled: &Arc<CompiledInstrument>,
    pattern: &CompiledEvents,
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

fn render_score_parts(
    demo: &performance::PerformancePlan,
    args: &RenderScoreArgs,
    sample_rate: f64,
    block_size: usize,
    tail_frames: u64,
    trace_requests: &[Option<TraceRequest>],
    mut consume: impl FnMut(
        usize,
        &sonalloy_core::RenderedAudio,
        Vec<RenderTraceReport>,
    ) -> Result<(), CliFailure>,
) -> Result<(), CliFailure> {
    let mut required_parts = demo
        .parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            score_part_is_selected(args, &part.definition.id) || trace_requests[index].is_some()
        })
        .collect::<Vec<_>>();
    for index in 0..required_parts.len() {
        if required_parts[index] {
            let mut dependency = demo.audio_dependencies[index];
            while let Some(source_index) = dependency {
                required_parts[source_index] = true;
                dependency = demo.audio_dependencies[source_index];
            }
        }
    }

    let prerender_order = performance::prerender_order(demo, &required_parts);
    let mut remaining_input_uses = vec![0; demo.parts.len()];
    for (index, source_index) in demo.audio_dependencies.iter().enumerate() {
        if required_parts[index] {
            if let Some(source_index) = source_index {
                remaining_input_uses[*source_index] += 1;
            }
        }
    }
    for index in &prerender_order {
        if let Some(source_index) = demo.audio_dependencies[*index] {
            remaining_input_uses[source_index] += 1;
        }
    }
    let mut prepared_parts = PreparedScoreParts {
        sample_rate,
        block_size,
        tail_frames,
        inputs: (0..demo.parts.len()).map(|_| None).collect(),
        remaining_input_uses,
        rendered_as_dependency: vec![false; demo.parts.len()],
    };

    for index in 0..demo.parts.len() {
        let output_selected = score_part_is_selected(args, &demo.parts[index].definition.id);
        if !required_parts[index]
            || (prepared_parts.rendered_as_dependency[index]
                && !output_selected
                && trace_requests[index].is_none())
        {
            continue;
        }
        if let Some(source_index) = demo.audio_dependencies[index] {
            if prepared_parts.inputs[source_index].is_none() {
                ensure_prepared_part_audio(demo, source_index, &mut prepared_parts)?;
            }
        }
        let (audio, traces) = render_score_part_audio(
            demo,
            index,
            sample_rate,
            block_size,
            tail_frames,
            &prepared_parts.inputs,
            trace_requests[index].as_ref(),
        )?;
        release_part_audio_input(
            index,
            demo,
            &mut prepared_parts.inputs,
            &mut prepared_parts.remaining_input_uses,
        );
        consume(index, &audio, traces)?;
        if prepared_parts.remaining_input_uses[index] > 0 && prepared_parts.inputs[index].is_none()
        {
            prepared_parts.inputs[index] = Some(
                prepare_part_audio_input(audio)
                    .map_err(|detail| score_routing_failure(index, &detail))?,
            );
        }
    }
    Ok(())
}

struct PreparedScoreParts {
    sample_rate: f64,
    block_size: usize,
    tail_frames: u64,
    inputs: Vec<Option<sonalloy_core::PreparedAudio>>,
    remaining_input_uses: Vec<usize>,
    rendered_as_dependency: Vec<bool>,
}

fn ensure_prepared_part_audio(
    demo: &performance::PerformancePlan,
    index: usize,
    prepared: &mut PreparedScoreParts,
) -> Result<(), CliFailure> {
    if prepared.inputs[index].is_some() {
        return Ok(());
    }
    if let Some(source_index) = demo.audio_dependencies[index] {
        ensure_prepared_part_audio(demo, source_index, prepared)?;
    }
    let (audio, _) = render_score_part_audio(
        demo,
        index,
        prepared.sample_rate,
        prepared.block_size,
        prepared.tail_frames,
        &prepared.inputs,
        None,
    )?;
    release_part_audio_input(
        index,
        demo,
        &mut prepared.inputs,
        &mut prepared.remaining_input_uses,
    );
    prepared.inputs[index] = Some(
        prepare_part_audio_input(audio).map_err(|detail| score_routing_failure(index, &detail))?,
    );
    prepared.rendered_as_dependency[index] = true;
    Ok(())
}

fn render_score_part_audio(
    demo: &performance::PerformancePlan,
    index: usize,
    sample_rate: f64,
    block_size: usize,
    tail_frames: u64,
    prepared_inputs: &[Option<sonalloy_core::PreparedAudio>],
    trace_request: Option<&TraceRequest>,
) -> Result<(sonalloy_core::RenderedAudio, Vec<RenderTraceReport>), CliFailure> {
    let part = &demo.parts[index];
    let external_audio = demo.audio_dependencies[index]
        .map(|source_index| {
            prepared_inputs[source_index]
                .as_ref()
                .ok_or_else(|| score_routing_failure(index, "audio source was not prepared"))
        })
        .transpose()?;
    let mut mix = StereoMix::new(demo.sample_rate);
    let mut traces = Vec::new();
    for lane in &part.render_lanes {
        let (audio, trace) = render_compiled_pattern(
            &part.instrument,
            &lane.compiled,
            sample_rate,
            block_size,
            tail_frames,
            trace_request,
            external_audio,
        )
        .map_err(|failure| prefix_part_failure(failure, index, demo, sample_rate))?;
        if let Some(trace) = trace {
            traces.push(trace);
        }
        mix.add_part(&audio, lane.gain_db)
            .map_err(|detail| score_routing_failure(index, &detail))?;
    }
    mix.apply_pan(part.definition.pan);
    let mut audio = mix.into_audio();
    performance::apply_section_part_gain(&mut audio, demo, &part.definition.id)
        .map_err(|detail| score_routing_failure(index, &detail))?;
    Ok((audio, traces))
}

fn release_part_audio_input(
    index: usize,
    demo: &performance::PerformancePlan,
    prepared_inputs: &mut [Option<sonalloy_core::PreparedAudio>],
    remaining_input_uses: &mut [usize],
) {
    let Some(source_index) = demo.audio_dependencies[index] else {
        return;
    };
    remaining_input_uses[source_index] -= 1;
    if remaining_input_uses[source_index] == 0 {
        prepared_inputs[source_index] = None;
    }
}

fn prepare_part_audio_input(
    audio: sonalloy_core::RenderedAudio,
) -> Result<sonalloy_core::PreparedAudio, String> {
    if audio.channels.len() != 2 || audio.channels[0].len() != audio.channels[1].len() {
        return Err("external audio source must be stereo".into());
    }
    let mut channels = audio.channels.into_iter();
    let left = channels
        .next()
        .ok_or("external audio source must be stereo")?;
    let right = channels
        .next()
        .ok_or("external audio source must be stereo")?;
    let frames = left.len();
    Ok(sonalloy_core::PreparedAudio {
        sample_rate: f64::from(audio.sample_rate),
        frames,
        source_metadata: sonalloy_core::SampleMetadata {
            source_sample_rate: audio.sample_rate,
            source_channels: 2,
            bits_per_sample: Some(32),
            source_frames: frames,
        },
        channels: sonalloy_core::PreparedAudioChannels::Stereo {
            left: Arc::from(left),
            right: Arc::from(right),
        },
    })
}

fn score_routing_failure(part_index: usize, detail: &str) -> CliFailure {
    CliFailure {
        code: 3,
        diagnostics: vec![
            Diagnostic::error(
                DiagnosticCode::RenderError,
                "Score external audio routing failed",
            )
            .with_path(format!("parts[{part_index}].audio_input"))
            .with_detail(detail),
        ],
    }
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
            EventSequenceKind::ParameterRamp {
                duration_frames: ramp_frames,
                parameter,
                from_value,
                to_value,
            } => {
                let Some(ramp_frames) = usize::try_from(*ramp_frames)
                    .ok()
                    .filter(|frames| *frames > 0)
                    .filter(|_| {
                        entry
                            .absolute_frame
                            .checked_add(*ramp_frames)
                            .is_some_and(|end| end <= duration_frames)
                    })
                else {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "ramp duration_frames must be positive, fit in the process frame counter, and end within the render duration",
                        )
                        .with_path(format!("{event_path}.duration_frames")),
                    );
                    continue;
                };
                match crate::performance::events::resolve_parameter_ramp(
                    compiled,
                    parameter,
                    *from_value,
                    *to_value,
                    ramp_frames,
                    &event_path,
                ) {
                    Ok(kind) => kind,
                    Err(error) => {
                        diagnostics.push(error);
                        continue;
                    }
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
    let external_audio = match load_render_audio_input(common, &compiled) {
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

fn score_part_is_selected(args: &RenderScoreArgs, id: &str) -> bool {
    (args.parts.is_empty() || args.parts.iter().any(|part| part == id))
        && !args.mutes.iter().any(|part| part == id)
}

fn score_timeline_frame(
    demo: &performance::PerformancePlan,
    tick: u64,
    sample_rate: f64,
) -> Result<u64, CliFailure> {
    crate::musical_time::tick_to_frame(
        tick,
        demo.score.ticks_per_beat,
        &performance::events::tempo_points(&demo.score.timeline),
        sample_rate,
    )
    .map_err(|error| CliFailure {
        code: 2,
        diagnostics: vec![Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            error.to_string(),
        )],
    })
}

fn score_output_offset_frames(
    demo: &performance::PerformancePlan,
    sample_rate: f64,
) -> Result<usize, CliFailure> {
    let render_start = score_timeline_frame(demo, demo.render_start_tick, sample_rate)?;
    let output_start = score_timeline_frame(demo, demo.output_start_tick, sample_rate)?;
    usize::try_from(output_start.saturating_sub(render_start)).map_err(|_| CliFailure {
        code: 2,
        diagnostics: vec![Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            "render output offset does not fit in memory",
        )],
    })
}

fn trim_rendered_audio(
    audio: &sonalloy_core::RenderedAudio,
    offset: usize,
) -> sonalloy_core::RenderedAudio {
    sonalloy_core::RenderedAudio {
        sample_rate: audio.sample_rate,
        channels: audio
            .channels
            .iter()
            .map(|channel| channel.get(offset..).unwrap_or_default().to_vec())
            .collect(),
    }
}

type ScoreMixRender = (
    sonalloy_core::RenderedAudio,
    Vec<PartRenderReport>,
    Vec<Vec<SectionPartLevel>>,
);

fn render_score_mix(
    demo: &performance::PerformancePlan,
    args: &RenderScoreArgs,
    tail_frames: u64,
) -> Result<ScoreMixRender, CliFailure> {
    let sample_rate = f64::from(args.sample_rate);
    let output_offset = score_output_offset_frames(demo, sample_rate)?;
    if let Some(stems_dir) = &args.stems_dir {
        if let Err(error) = std::fs::create_dir_all(stems_dir) {
            return Err(CliFailure {
                code: 4,
                diagnostics: vec![
                    Diagnostic::error(
                        DiagnosticCode::WavOutputError,
                        "could not create stems directory",
                    )
                    .with_path(stems_dir.to_string_lossy())
                    .with_detail(error.to_string()),
                ],
            });
        }
    }

    let trace_requests = score_trace_requests(demo, args)?;
    let mut mix = StereoMix::new(args.sample_rate);
    let mut part_reports = Vec::with_capacity(demo.parts.len());
    let mut part_section_levels = Vec::new();
    let mut section_ranges = None;
    if args.analyze {
        part_section_levels.resize_with(demo.score.sections.len(), Vec::new);
    }
    render_score_parts(
        demo,
        args,
        sample_rate,
        args.block_size,
        tail_frames,
        &trace_requests,
        |index, audio, traces| {
            let part = &demo.parts[index];
            let output_selected = score_part_is_selected(args, &part.definition.id);
            if !output_selected && traces.is_empty() {
                return Ok(());
            }
            let output_audio = trim_rendered_audio(audio, output_offset);
            let stem = if output_selected {
                if let Some(stems_dir) = &args.stems_dir {
                    let stem_path = stems_dir.join(format!("{}.wav", part.definition.id));
                    write_wav(&stem_path, &output_audio).map_err(|error| CliFailure {
                        code: 4,
                        diagnostics: vec![error],
                    })?;
                    Some(stem_path.to_string_lossy().into_owned())
                } else {
                    None
                }
            } else {
                None
            };
            let frames = output_audio.frames();
            let loudness = if output_selected && args.analyze {
                Some(analyze_audio_loudness(&output_audio, 0, frames)?)
            } else {
                None
            };
            if output_selected {
                mix.add_part(&output_audio, 0.0)
                    .map_err(|error| CliFailure {
                        code: 3,
                        diagnostics: vec![
                            Diagnostic::error(DiagnosticCode::RenderError, error)
                                .with_path(format!("parts[{index}]")),
                        ],
                    })?;
                if args.analyze {
                    collect_section_part_levels(
                        demo,
                        &part.definition.id,
                        &output_audio,
                        loudness.expect("part loudness exists when analysis is enabled"),
                        &mut section_ranges,
                        &mut part_section_levels,
                    )?;
                }
            }
            part_reports.push(PartRenderReport {
                id: part.definition.id.clone(),
                lane_gains_db: part.render_lanes.iter().map(|lane| lane.gain_db).collect(),
                frames,
                loudness,
                stem,
                traces,
            });
            Ok(())
        },
    )?;
    apply_score_mix_fade(&mut mix, demo, sample_rate, tail_frames)?;
    let mix_audio = mix.into_audio();
    Ok((mix_audio, part_reports, part_section_levels))
}

fn apply_score_mix_fade(
    mix: &mut StereoMix,
    demo: &performance::PerformancePlan,
    sample_rate: f64,
    tail_frames: u64,
) -> Result<(), CliFailure> {
    let fade_out_seconds = demo.definition.mix.fade_out_seconds;
    if fade_out_seconds == 0.0 {
        return Ok(());
    }
    let timeline_start_frame = score_timeline_frame(demo, demo.output_start_tick, sample_rate)?;
    let score_end_frame = score_timeline_frame(demo, demo.length_ticks, sample_rate)?;
    let timeline_frames = score_end_frame
        .checked_add(tail_frames)
        .ok_or_else(|| CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "Score mix frame range overflows the timeline",
            )],
        })?;
    mix.apply_fade(fade_out_seconds, timeline_start_frame, timeline_frames)
        .map_err(|error| CliFailure {
            code: 3,
            diagnostics: vec![
                Diagnostic::error(DiagnosticCode::RenderError, error)
                    .with_path("mix.fade_out_seconds"),
            ],
        })
}

/// Reject a Score render whose final, premaster and MP3 outputs would share a path.
fn validate_score_output_paths(args: &RenderScoreArgs) -> Result<(), CliFailure> {
    let mut output_paths = Vec::new();
    for path in std::iter::once(&args.output)
        .chain(args.premaster_output.iter())
        .chain(args.mp3_output.iter())
    {
        let identity = crate::output::output_identity(path)?;
        if output_paths.contains(&identity) {
            return Err(CliFailure {
                code: 2,
                diagnostics: vec![Diagnostic::error(
                    DiagnosticCode::WavOutputError,
                    "final, premaster and MP3 output paths must be different",
                )],
            });
        }
        output_paths.push(identity);
    }
    Ok(())
}

fn validate_score_input_paths(
    plan: &performance::PerformancePlan,
    args: &RenderScoreArgs,
) -> Result<(), CliFailure> {
    let mut inputs = vec![crate::output::output_identity(&args.score)?];
    for part in &plan.parts {
        let definition = &part.definition.instrument;
        let identity = crate::output::output_identity(definition)?;
        if !inputs.contains(&identity) {
            inputs.push(identity);
        }
        for asset in score_asset_paths(definition)? {
            let identity = crate::output::output_identity(&asset)?;
            if !inputs.contains(&identity) {
                inputs.push(identity);
            }
        }
    }
    let mut outputs = std::iter::once(&args.output)
        .chain(args.premaster_output.iter())
        .chain(args.mp3_output.iter())
        .map(|p| crate::output::output_identity(p))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(directory) = &args.stems_dir {
        for part in &plan.parts {
            let path = directory.join(format!("{}.wav", part.definition.id));
            let identity = crate::output::output_identity(&path)?;
            if outputs.contains(&identity) {
                return Err(score_output_path_failure(
                    &path,
                    "stem and mix output paths must be distinct",
                ));
            }
            outputs.push(identity);
        }
    }
    if let Some(output) = outputs.iter().find(|output| inputs.contains(output)) {
        return Err(score_output_path_failure(
            output,
            "output must not overwrite a score, instrument definition, or referenced asset",
        ));
    }
    Ok(())
}

fn score_asset_paths(definition_path: &Path) -> Result<Vec<PathBuf>, CliFailure> {
    let text = std::fs::read_to_string(definition_path).map_err(|error| CliFailure {
        code: 1,
        diagnostics: vec![
            Diagnostic::error(
                DiagnosticCode::DefinitionError,
                "could not read instrument definition assets",
            )
            .with_path(definition_path.to_string_lossy())
            .with_detail(error.to_string()),
        ],
    })?;
    let mut definition: sonalloy_core::InstrumentDefinition =
        serde_json::from_str(&text).map_err(|error| CliFailure {
            code: 1,
            diagnostics: vec![
                Diagnostic::error(
                    DiagnosticCode::JsonInvalid,
                    "could not inspect instrument definition assets",
                )
                .with_path(definition_path.to_string_lossy())
                .with_detail(error.to_string()),
            ],
        })?;
    definition.layers.retain(|layer| layer.enabled);
    let base = definition_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut assets = Vec::new();
    definition
        .try_for_each_asset_mut(|_, reference| {
            let path = Path::new(&reference.path);
            assets.push(if path.is_absolute() {
                path.to_path_buf()
            } else {
                base.join(path)
            });
            Ok::<_, std::convert::Infallible>(())
        })
        .expect("asset collection is infallible");
    Ok(assets)
}

fn score_output_path_failure(path: &Path, detail: &str) -> CliFailure {
    CliFailure {
        code: 2,
        diagnostics: vec![
            Diagnostic::error(DiagnosticCode::WavOutputError, detail)
                .with_path(path.to_string_lossy()),
        ],
    }
}

/// Resolve the WAV that mastering and loudness analysis read, filling a temporary
/// mix only when no premaster was requested and a WAV is still needed. The
/// returned directory keeps that temporary mix alive for the caller.
fn score_premaster_source(
    args: &RenderScoreArgs,
    master_configured: bool,
    mix_audio: &sonalloy_core::RenderedAudio,
) -> Result<(Option<tempfile::TempDir>, Option<PathBuf>), CliFailure> {
    if args.premaster_output.is_none() && !(args.analyze || master_configured) {
        return Ok((None, None));
    }
    let Some(path) = &args.premaster_output else {
        let directory = tempfile::tempdir().map_err(|error| CliFailure {
            code: 4,
            diagnostics: vec![
                Diagnostic::error(
                    DiagnosticCode::WavOutputError,
                    "could not create temporary Score mix",
                )
                .with_detail(error.to_string()),
            ],
        })?;
        let path = directory.path().join("mix.wav");
        write_wav(&path, mix_audio).map_err(|error| CliFailure {
            code: 4,
            diagnostics: vec![error],
        })?;
        return Ok((Some(directory), Some(path)));
    };
    Ok((None, Some(path.clone())))
}

/// Master the premaster WAV into the pending output, or write the mix directly
/// when the Score configures no mastering. Both paths fill the pending name, so
/// the caller still publishes the final WAV only after every later step.
fn render_score_master(
    args: &RenderScoreArgs,
    master: Option<&performance::Master>,
    premaster_path: Option<&Path>,
    pending: &Path,
    mix_audio: &sonalloy_core::RenderedAudio,
) -> Result<Option<MasterReport>, CliFailure> {
    if let Some(settings) = master {
        Ok(Some(
            master_score(
                premaster_path.expect("mastering has a premaster WAV"),
                pending,
                settings,
                args.sample_rate,
            )
            .map_err(ffmpeg_failure)?,
        ))
    } else {
        write_wav(pending, mix_audio).map_err(|error| CliFailure {
            code: 4,
            diagnostics: vec![error],
        })?;
        Ok(None)
    }
}

/// Inspect the rendered WAV and encode an optional MP3, so the final output is
/// only published once every measurement has succeeded.
fn measure_score_output(
    args: &RenderScoreArgs,
    output_wav: &Path,
    master_report: Option<&MasterReport>,
    mix_loudness: Option<&LoudnessAnalysis>,
) -> Result<OutputMeasurements, CliFailure> {
    let analysis = if args.analyze {
        Some(analyze_output_wav(output_wav, args.sample_rate)?)
    } else {
        None
    };
    let loudness = if args.analyze {
        if master_report.is_none() {
            mix_loudness.cloned()
        } else {
            Some(performance::analyze_loudness(output_wav).map_err(ffmpeg_failure)?)
        }
    } else {
        None
    };
    let mp3_measurement = if let Some(mp3_output) = &args.mp3_output {
        encode_mp3(output_wav, mp3_output).map_err(ffmpeg_failure)?;
        Some(performance::measure_loudness(mp3_output).map_err(ffmpeg_failure)?)
    } else {
        None
    };
    Ok(OutputMeasurements {
        analysis,
        loudness,
        mp3_measurement,
    })
}

struct OutputMeasurements {
    analysis: Option<AudioAnalysis>,
    loudness: Option<LoudnessAnalysis>,
    mp3_measurement: Option<LoudnessMeasurement>,
}

/// Typical true peak overshoot that MP3 encoding adds to a WAV already
/// limited to the ceiling. Overshoot within this margin is lossy-coding
/// physics, not a mastering mistake.
const MP3_TRUE_PEAK_TOLERANCE_DB: f64 = 1.0;

/// Build the warning for an encoded MP3 whose measured true peak exceeds the
/// `meta.master.max_true_peak_db` ceiling by more than the encoder tolerance.
/// Encoders push inter-sample peaks above the rendered WAV's ceiling, so the
/// comparison uses the value measured after encoding. `None` while the
/// overshoot stays within the tolerance.
fn mp3_true_peak_warning(measured_db: f64, limit_db: f64) -> Option<Diagnostic> {
    if measured_db - limit_db <= MP3_TRUE_PEAK_TOLERANCE_DB {
        return None;
    }
    Some(
        Diagnostic::warning(
            DiagnosticCode::ValueOutOfRange,
            format!(
                "the encoded MP3 measures {measured_db:.2} dBTP, {:.2} dB above the meta.master.max_true_peak_db limit of {limit_db:.2} dBTP; the value is measured after MP3 encoding. lower meta.master.max_true_peak_db to reserve lossy encoding headroom",
                measured_db - limit_db
            ),
        )
        .with_path("meta.master.max_true_peak_db"),
    )
}

#[derive(Debug, Clone, Copy)]
struct ScoreSelection {
    partial: bool,
    render_start_tick: u64,
    output_start_tick: u64,
    end_tick: u64,
}

fn parse_bar_range(value: &str) -> Result<(usize, usize), CliFailure> {
    let (start, end) = value.split_once('-').ok_or_else(|| CliFailure {
        code: 2,
        diagnostics: vec![Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            "--bars must use START-END",
        )],
    })?;
    let start = start.parse::<usize>().ok().filter(|value| *value > 0);
    let end = end.parse::<usize>().ok().filter(|value| *value > 0);
    match (start, end) {
        (Some(start), Some(end)) if start <= end => Ok((start, end)),
        _ => Err(CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "--bars must contain a positive ascending range",
            )],
        }),
    }
}

#[allow(clippy::too_many_lines)]
fn resolve_score_selection(
    demo: &performance::PerformancePlan,
    args: &RenderScoreArgs,
) -> Result<ScoreSelection, CliFailure> {
    let mut selected_start = 0;
    let mut selected_end = demo.length_ticks;
    let range_requested = args.section.is_some() || args.bars.is_some();
    if let Some(name) = &args.section {
        let selected = if let Some(occurrence) = args.occurrence {
            let Some(selected) = demo.score.sections.get(occurrence - 1) else {
                return Err(CliFailure {
                    code: 2,
                    diagnostics: vec![
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "section occurrence is outside the form",
                        )
                        .with_path("--occurrence"),
                    ],
                });
            };
            if &selected.section != name {
                return Err(CliFailure {
                    code: 2,
                    diagnostics: vec![
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "section name does not match the selected form occurrence",
                        )
                        .with_path("--occurrence"),
                    ],
                });
            }
            selected
        } else {
            let matches = demo
                .score
                .sections
                .iter()
                .filter(|section| &section.section == name)
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [selected] => *selected,
                [] => {
                    return Err(CliFailure {
                        code: 2,
                        diagnostics: vec![
                            Diagnostic::error(
                                DiagnosticCode::DefinitionError,
                                "section does not occur in the Score form",
                            )
                            .with_path("--section"),
                        ],
                    });
                }
                _ => {
                    return Err(CliFailure {
                        code: 2,
                        diagnostics: vec![
                            Diagnostic::error(
                                DiagnosticCode::ValueOutOfRange,
                                "section selection matches multiple occurrences; specify --occurrence",
                            )
                            .with_path("--section"),
                        ],
                    });
                }
            }
        };
        selected_start = selected.start_tick;
        selected_end = selected.end_tick;
    } else if let Some(bars) = &args.bars {
        let (first, last) = parse_bar_range(bars)?;
        let all_bars = demo
            .score
            .sections
            .iter()
            .flat_map(|section| {
                section
                    .bar_start_ticks
                    .iter()
                    .copied()
                    .zip(section.bar_end_ticks.iter().copied())
            })
            .collect::<Vec<_>>();
        let Some((start, _)) = all_bars.get(first - 1).copied() else {
            return Err(CliFailure {
                code: 2,
                diagnostics: vec![
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "bar range starts outside the Score",
                    )
                    .with_path("--bars"),
                ],
            });
        };
        let Some((_, end)) = all_bars.get(last - 1).copied() else {
            return Err(CliFailure {
                code: 2,
                diagnostics: vec![
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "bar range ends outside the Score",
                    )
                    .with_path("--bars"),
                ],
            });
        };
        selected_start = start;
        selected_end = end;
    } else if args.preroll_bars > 0 {
        return Err(CliFailure {
            code: 2,
            diagnostics: vec![
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "--preroll-bars requires --section or --bars",
                )
                .with_path("--preroll-bars"),
            ],
        });
    }

    for id in args.parts.iter().chain(&args.mutes) {
        if !demo.parts.iter().any(|part| part.definition.id == *id) {
            return Err(CliFailure {
                code: 2,
                diagnostics: vec![
                    Diagnostic::error(DiagnosticCode::DefinitionError, "unknown Score Part")
                        .with_path(id),
                ],
            });
        }
    }
    let output_parts = demo
        .parts
        .iter()
        .filter(|part| args.parts.is_empty() || args.parts.contains(&part.definition.id))
        .filter(|part| !args.mutes.contains(&part.definition.id))
        .count();
    if output_parts == 0 {
        return Err(CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "Part selection produces an empty output",
            )],
        });
    }

    let mut render_start = selected_start;
    if args.preroll_bars > 0 {
        let all_bars = demo
            .score
            .sections
            .iter()
            .flat_map(|section| {
                section
                    .bar_start_ticks
                    .iter()
                    .copied()
                    .zip(section.bar_end_ticks.iter().copied())
            })
            .collect::<Vec<_>>();
        let first_bar = all_bars
            .iter()
            .position(|(_, end)| *end > selected_start)
            .unwrap_or(0);
        render_start = all_bars
            .get(first_bar.saturating_sub(args.preroll_bars))
            .map_or(0, |(start, _)| *start);
    }
    Ok(ScoreSelection {
        partial: range_requested || !args.parts.is_empty() || !args.mutes.is_empty(),
        render_start_tick: render_start,
        output_start_tick: selected_start,
        end_tick: selected_end,
    })
}

#[allow(clippy::too_many_lines)]
fn execute_render_score(args: &RenderScoreArgs) -> Result<ScoreRenderReport, CliFailure> {
    validate_score_output_paths(args)?;
    let sample_rate = f64::from(args.sample_rate);
    let tail_frames = match seconds_to_frames(args.tail, sample_rate) {
        Ok(frames) => frames,
        Err(error) => return Err(input_failure(&error)),
    };
    let mut demo = performance::load(
        &args.score,
        &args.bindings,
        args.sample_rate,
        args.block_size,
    )?;
    let selection = resolve_score_selection(&demo, args)?;
    if selection.render_start_tick != 0 || selection.end_tick != demo.length_ticks {
        performance::select_range(
            &mut demo,
            selection.render_start_tick,
            selection.end_tick,
            selection.output_start_tick,
        )?;
    }
    validate_score_input_paths(&demo, args)?;
    let (mix_audio, part_reports, part_section_levels) =
        render_score_mix(&demo, args, tail_frames)?;
    // Persist a requested premaster before any mastering or FFmpeg analysis can fail.
    if let Some(path) = &args.premaster_output {
        if let Err(error) = write_wav(path, &mix_audio) {
            return Err(CliFailure {
                code: 4,
                diagnostics: vec![error],
            });
        }
    }
    let mix_analysis = if args.analyze {
        Some(analyze_audio(&mix_audio, None)?)
    } else {
        None
    };
    let section_analysis = if args.analyze {
        Some(analyze_score_sections(
            &demo,
            &mix_audio,
            part_section_levels,
        )?)
    } else {
        None
    };

    let score_master = if selection.partial && !args.apply_master {
        None
    } else {
        demo.definition.mix.master.as_ref()
    };
    let (_temporary_mix_directory, premaster_path) =
        score_premaster_source(args, score_master.is_some(), &mix_audio)?;
    let mix_loudness = if args.analyze {
        Some(
            performance::analyze_loudness(
                premaster_path
                    .as_deref()
                    .expect("analysis has a premaster WAV"),
            )
            .map_err(ffmpeg_failure)?,
        )
    } else {
        None
    };
    // The final WAV stays under a pending name until analysis and MP3 succeed, so a
    // failure never replaces an already published output.
    let pending = crate::output::pending_wav(&args.output)?;
    let master_report = render_score_master(
        args,
        score_master,
        premaster_path.as_deref(),
        &pending,
        &mix_audio,
    )?;

    let measurements = measure_score_output(
        args,
        &pending,
        master_report.as_ref(),
        mix_loudness.as_ref(),
    )?;
    crate::output::commit_wav(pending, &args.output)?;
    let mut diagnostics = demo.diagnostics.clone();
    if let Some(comparison) = master_report
        .as_ref()
        .and_then(|report| report.lra.as_ref())
    {
        if comparison
            .deviation_lu
            .is_none_or(|difference| difference.abs() > 0.5)
        {
            diagnostics.push(
                Diagnostic::warning(
                    DiagnosticCode::ValueOutOfRange,
                    comparison.reason.map_or_else(
                        || {
                            format!(
                                "measured LRA differs from target by {:.2} LU",
                                comparison.deviation_lu.expect("measured deviation")
                            )
                        },
                        str::to_owned,
                    ),
                )
                .with_path("meta.master.target_lra_lu"),
            );
        }
    }
    if let (Some(master), Some(measurement)) = (score_master, measurements.mp3_measurement.as_ref())
    {
        if let Some(mp3_true_peak_db) = measurement.true_peak_db {
            if let Some(warning) = mp3_true_peak_warning(mp3_true_peak_db, master.true_peak_db) {
                diagnostics.push(warning);
            }
        }
    }
    let report = ScoreRenderReport {
        status: "ok",
        sample_rate: mix_audio.sample_rate,
        channels: mix_audio.channels.len(),
        frames: mix_audio.frames(),
        output: args.output.to_string_lossy().into_owned(),
        premaster_output: args
            .premaster_output
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        mp3_output: args
            .mp3_output
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        stems_dir: args
            .stems_dir
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        parts: part_reports,
        sections: section_analysis,
        mix_analysis,
        output_analysis: measurements.analysis,
        mix_loudness,
        output_loudness: measurements.loudness,
        master: master_report,
        mp3_measurement: measurements.mp3_measurement,
        diagnostics,
    };
    Ok(report)
}

fn run_render_score(args: &RenderScoreArgs) -> ExitCode {
    let report = match execute_render_score(args) {
        Ok(report) => report,
        Err(failure) => return finish_failure(args.json, failure),
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&report).expect("Score render report is serializable")
        );
    } else {
        println!(
            "rendered {} frames at {} Hz to {}",
            report.frames, report.sample_rate, report.output
        );
        if let Some(mp3_output) = &report.mp3_output {
            println!("created {mp3_output}");
        }
        if let Some(premaster) = &report.premaster_output {
            println!("created premaster {premaster}");
        }
        for (label, loudness) in [
            ("mix loudness", &report.mix_loudness),
            ("output loudness", &report.output_loudness),
        ] {
            if let Some(loudness) = loudness {
                println!(
                    "{label}: {}",
                    serde_json::to_string(loudness).expect("loudness serializes")
                );
            }
        }
        if args.analyze {
            println!("part loudness (Integrated LUFS / True Peak dBTP):");
            for part in &report.parts {
                if let Some(loudness) = &part.loudness {
                    print_score_loudness_row(
                        &format!("  {}", part.id),
                        loudness.status,
                        loudness.integrated_lufs,
                        loudness.true_peak_db,
                    );
                }
            }
            if let Some(sections) = &report.sections {
                println!("section loudness (Integrated LUFS / True Peak dBTP):");
                for section in sections {
                    print_score_loudness_row(
                        &format!("  {} #{}", section.section, section.occurrence),
                        section.loudness_status,
                        section.integrated_lufs,
                        section.true_peak_db,
                    );
                    for part in &section.parts {
                        print_score_loudness_row(
                            &format!("    {}", part.id),
                            part.loudness.status,
                            part.loudness.integrated_lufs,
                            part.loudness.true_peak_db,
                        );
                    }
                }
            }
        }
        if let Some(master) = &report.master {
            println!(
                "master target: {:.1} LUFS, {:.2} dBTP",
                master.target.integrated_lufs, master.target.true_peak_db
            );
            print_loudness_measurement("master input", &master.input);
            print_loudness_measurement("master output", &master.output);
            println!(
                "master deviation: {:+.1} LUFS, {:+.2} dBTP",
                master.deviation.integrated_lufs, master.deviation.true_peak_db
            );
            println!(
                "master gain: input {:.2} dB, output {:.2} dB ({} attempts)",
                master.input_gain_db, master.output_gain_db, master.attempts
            );
        }
        if let Some(measurement) = &report.mp3_measurement {
            print_loudness_measurement("mp3 measurement", measurement);
        }
        print_warnings(&report.diagnostics);
    }
    ExitCode::SUCCESS
}

fn prefix_part_failure(
    mut failure: CliFailure,
    part_index: usize,
    demo: &performance::PerformancePlan,
    sample_rate: f64,
) -> CliFailure {
    let prefix = format!("parts[{part_index}]");
    let part = &demo.parts[part_index];
    let tempo_changes = performance::events::tempo_points(&demo.score.timeline);
    let render_start_frame = crate::musical_time::tick_to_frame(
        demo.render_start_tick,
        demo.score.ticks_per_beat,
        &tempo_changes,
        sample_rate,
    )
    .ok();
    let score_end_frame = crate::musical_time::tick_to_frame(
        demo.output_end_tick,
        demo.score.ticks_per_beat,
        &tempo_changes,
        sample_rate,
    )
    .ok();
    for diagnostic in &mut failure.diagnostics {
        diagnostic.path = Some(match diagnostic.path.take() {
            Some(path) => format!("{prefix}.{path}"),
            None => prefix.clone(),
        });
        let context = diagnostic
            .context
            .get_or_insert_with(|| Box::new(DiagnosticContext::default()));
        context.part = Some(part.definition.id.clone());
        if let Some(process_frame) = context.block_start_frame {
            let audio_frame = process_frame.saturating_sub(
                u64::try_from(part.instrument.reported_latency_frames).unwrap_or(u64::MAX),
            );
            if let Some(render_start_frame) = render_start_frame {
                if let Some(score_frame) = render_start_frame.checked_add(audio_frame) {
                    context.block_start_frame = Some(score_frame);
                    #[allow(clippy::cast_precision_loss)]
                    {
                        context.time_seconds = Some(score_frame as f64 / sample_rate);
                    }
                    context.tail = Some(score_end_frame.is_some_and(|end| score_frame >= end));
                    if let Some((section_index, section)) =
                        demo.score.sections.iter().enumerate().find(|(_, section)| {
                            let start = crate::musical_time::tick_to_frame(
                                section.start_tick,
                                demo.score.ticks_per_beat,
                                &tempo_changes,
                                sample_rate,
                            )
                            .unwrap_or(u64::MAX);
                            let end = crate::musical_time::tick_to_frame(
                                section.end_tick,
                                demo.score.ticks_per_beat,
                                &tempo_changes,
                                sample_rate,
                            )
                            .unwrap_or(0);
                            score_frame >= start && score_frame < end
                        })
                    {
                        context.section = Some(section.section.clone());
                        context.occurrence = Some(section_index + 1);
                    }
                }
            }
        }
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
                    "FFmpeg is required for loudness analysis, Score mastering or MP3 output",
                )
                .with_detail("install ffmpeg and make it available on PATH"),
            ],
        }
    } else {
        CliFailure {
            code: 4,
            diagnostics: vec![
                Diagnostic::error(DiagnosticCode::RenderError, "FFmpeg processing failed")
                    .with_detail(error.detail),
            ],
        }
    }
}

#[derive(Debug, Serialize)]
struct ScoreRenderReport {
    status: &'static str,
    sample_rate: u32,
    channels: usize,
    frames: usize,
    output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    premaster_output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mix_loudness: Option<LoudnessAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_loudness: Option<LoudnessAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mp3_output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stems_dir: Option<String>,
    parts: Vec<PartRenderReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sections: Option<Vec<SectionAnalysisReport>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mix_analysis: Option<AudioAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_analysis: Option<AudioAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    master: Option<MasterReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mp3_measurement: Option<LoudnessMeasurement>,
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "serialize_score_diagnostics"
    )]
    diagnostics: Vec<Diagnostic>,
}

fn serialize_score_diagnostics<S>(
    diagnostics: &[Diagnostic],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    crate::output::diagnostic_values(diagnostics).serialize(serializer)
}

fn print_loudness_measurement(label: &str, measurement: &LoudnessMeasurement) {
    println!(
        "{label}: {:?} LUFS, {:?} dBTP, {:?} LU LRA",
        measurement.integrated_lufs, measurement.true_peak_db, measurement.loudness_range_lu
    );
}

fn print_score_loudness_row(
    label: &str,
    status: performance::LoudnessStatus,
    integrated_lufs: Option<f64>,
    true_peak_db: Option<f64>,
) {
    let status = match status {
        performance::LoudnessStatus::Measured => "measured",
        performance::LoudnessStatus::Silent => "silent",
        performance::LoudnessStatus::Unmeasurable => "unmeasurable",
    };
    let integrated_lufs =
        integrated_lufs.map_or_else(|| "—".to_owned(), |value| format!("{value:.2}"));
    let true_peak_db = true_peak_db.map_or_else(|| "—".to_owned(), |value| format!("{value:+.2}"));
    println!("{label}: {status} | {integrated_lufs} LUFS | {true_peak_db} dBTP");
}

#[derive(Debug, Serialize)]
struct PartRenderReport {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    traces: Vec<RenderTraceReport>,
    id: String,
    lane_gains_db: Vec<f64>,
    frames: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    loudness: Option<performance::AudioLoudnessReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stem: Option<String>,
}

#[derive(Debug, Serialize)]
struct SectionAnalysisReport {
    section: String,
    occurrence: usize,
    start_frame: u64,
    end_frame: u64,
    start_seconds: f64,
    end_seconds: f64,
    integrated_lufs: Option<f64>,
    true_peak_db: Option<f64>,
    loudness_status: performance::LoudnessStatus,
    rms: f32,
    peak: f32,
    bands: Vec<sonalloy_core::SpectralBand>,
    parts: Vec<SectionPartAnalysisReport>,
}

struct SectionPartLevel {
    id: String,
    rms: f32,
    peak: f32,
    loudness: performance::AudioLoudnessReport,
}

#[derive(Debug, Serialize)]
struct SectionPartAnalysisReport {
    id: String,
    rms: f32,
    peak: f32,
    rms_dbfs: Option<f32>,
    peak_dbfs: Option<f32>,
    loudness: performance::AudioLoudnessReport,
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

fn analyze_audio_loudness(
    audio: &sonalloy_core::RenderedAudio,
    start_frame: usize,
    end_frame: usize,
) -> Result<performance::AudioLoudnessReport, CliFailure> {
    performance::analyze_audio_loudness(audio, start_frame, end_frame).map_err(|error| CliFailure {
        code: 3,
        diagnostics: vec![
            Diagnostic::error(
                DiagnosticCode::RenderError,
                "audio loudness analysis failed",
            )
            .with_detail(error.to_string()),
        ],
    })
}

#[derive(Clone, Copy)]
struct SectionFrameRange {
    start_frame: u64,
    end_frame: u64,
    start_seconds: f64,
    end_seconds: f64,
    relative_start: usize,
    relative_end: usize,
}

fn section_analysis_ranges(
    demo: &performance::PerformancePlan,
    output_frames: usize,
) -> Result<Vec<Option<SectionFrameRange>>, CliFailure> {
    let sample_rate = f64::from(demo.sample_rate);
    let output_start_frame = score_timeline_frame(demo, demo.output_start_tick, sample_rate)?;
    let output_end_frame = output_start_frame
        .checked_add(u64::try_from(output_frames).map_err(|_| CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "section analysis frame count does not fit in the Score timeline",
            )],
        })?)
        .ok_or_else(|| CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "section analysis frame range overflows",
            )],
        })?;
    let tempo_changes = performance::events::tempo_points(&demo.score.timeline);
    let mut ranges = Vec::with_capacity(demo.score.sections.len());
    for section in &demo.score.sections {
        let start_tick = section.start_tick.max(demo.output_start_tick);
        let end_tick = section.end_tick.min(demo.output_end_tick);
        if start_tick >= end_tick {
            ranges.push(None);
            continue;
        }
        let start_frame =
            score_timeline_frame(demo, start_tick, sample_rate)?.max(output_start_frame);
        let end_frame = score_timeline_frame(demo, end_tick, sample_rate)?.min(output_end_frame);
        if start_frame >= end_frame {
            ranges.push(None);
            continue;
        }
        let relative_start =
            usize::try_from(start_frame - output_start_frame).map_err(|_| CliFailure {
                code: 2,
                diagnostics: vec![Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "section analysis start frame does not fit in memory",
                )],
            })?;
        let relative_end =
            usize::try_from(end_frame - output_start_frame).map_err(|_| CliFailure {
                code: 2,
                diagnostics: vec![Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "section analysis end frame does not fit in memory",
                )],
            })?;
        let start_seconds = crate::musical_time::musical_duration_seconds(
            start_tick,
            demo.score.ticks_per_beat,
            &tempo_changes,
        )
        .map_err(|error| CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                error.to_string(),
            )],
        })?;
        let end_seconds = crate::musical_time::musical_duration_seconds(
            end_tick,
            demo.score.ticks_per_beat,
            &tempo_changes,
        )
        .map_err(|error| CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                error.to_string(),
            )],
        })?;
        ranges.push(Some(SectionFrameRange {
            start_frame,
            end_frame,
            start_seconds,
            end_seconds,
            relative_start,
            relative_end,
        }));
    }
    Ok(ranges)
}

fn collect_section_part_levels(
    demo: &performance::PerformancePlan,
    id: &str,
    audio: &sonalloy_core::RenderedAudio,
    full_loudness: performance::AudioLoudnessReport,
    section_ranges: &mut Option<Vec<Option<SectionFrameRange>>>,
    part_section_levels: &mut [Vec<SectionPartLevel>],
) -> Result<(), CliFailure> {
    if section_ranges.is_none() {
        *section_ranges = Some(section_analysis_ranges(demo, audio.frames())?);
    }
    for (index, range) in section_ranges
        .as_ref()
        .expect("section analysis ranges are initialized")
        .iter()
        .copied()
        .enumerate()
    {
        if let Some(range) = range {
            part_section_levels[index].push(analyze_section_part(id, audio, range, full_loudness)?);
        }
    }
    Ok(())
}

fn analyze_section_part(
    id: &str,
    audio: &sonalloy_core::RenderedAudio,
    range: SectionFrameRange,
    full_loudness: performance::AudioLoudnessReport,
) -> Result<SectionPartLevel, CliFailure> {
    let mut peak = 0.0_f32;
    let mut sum_squares = 0.0_f64;
    let mut sample_count = 0_usize;
    for channel in &audio.channels {
        let start = range.relative_start.min(channel.len());
        let end = range.relative_end.min(channel.len()).max(start);
        for sample in &channel[start..end] {
            if sample.is_finite() {
                peak = peak.max(sample.abs());
                sum_squares += f64::from(*sample) * f64::from(*sample);
                sample_count += 1;
            }
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let rms = if sample_count == 0 {
        0.0
    } else {
        (sum_squares / sample_count as f64).sqrt() as f32
    };
    let loudness = if range.relative_start == 0 && range.relative_end == audio.frames() {
        full_loudness
    } else {
        analyze_audio_loudness(audio, range.relative_start, range.relative_end)?
    };
    Ok(SectionPartLevel {
        id: id.to_owned(),
        rms,
        peak,
        loudness,
    })
}

fn amplitude_dbfs(value: f32) -> Option<f32> {
    (value > 0.0 && value.is_finite()).then(|| 20.0 * value.log10())
}

fn analyze_score_sections(
    demo: &performance::PerformancePlan,
    mix_audio: &sonalloy_core::RenderedAudio,
    part_section_levels: Vec<Vec<SectionPartLevel>>,
) -> Result<Vec<SectionAnalysisReport>, CliFailure> {
    let ranges = section_analysis_ranges(demo, mix_audio.frames())?;
    let mut reports = Vec::new();
    for (occurrence, ((section, range), parts)) in demo
        .score
        .sections
        .iter()
        .zip(ranges)
        .zip(part_section_levels)
        .enumerate()
    {
        let Some(range) = range else {
            continue;
        };
        let section_audio =
            slice_rendered_audio(mix_audio, range.relative_start, range.relative_end);
        let analysis = analyze_audio(&section_audio, None)?;
        let loudness = analyze_audio_loudness(mix_audio, range.relative_start, range.relative_end)?;
        let parts = parts
            .into_iter()
            .map(|part| SectionPartAnalysisReport {
                id: part.id,
                rms: part.rms,
                peak: part.peak,
                rms_dbfs: amplitude_dbfs(part.rms),
                peak_dbfs: amplitude_dbfs(part.peak),
                loudness: part.loudness,
            })
            .collect();
        reports.push(SectionAnalysisReport {
            section: section.section.clone(),
            occurrence: occurrence + 1,
            start_frame: range.start_frame,
            end_frame: range.end_frame,
            start_seconds: range.start_seconds,
            end_seconds: range.end_seconds,
            integrated_lufs: loudness.integrated_lufs,
            true_peak_db: loudness.true_peak_db,
            loudness_status: loudness.status,
            rms: analysis.level.rms,
            peak: analysis.level.peak,
            bands: analysis.spectrum.bands,
            parts,
        });
    }
    Ok(reports)
}

fn slice_rendered_audio(
    audio: &sonalloy_core::RenderedAudio,
    start: usize,
    end: usize,
) -> sonalloy_core::RenderedAudio {
    sonalloy_core::RenderedAudio {
        sample_rate: audio.sample_rate,
        channels: audio
            .channels
            .iter()
            .map(|channel| {
                let start = start.min(channel.len());
                let end = end.min(channel.len()).max(start);
                channel[start..end].to_vec()
            })
            .collect(),
    }
}

fn analyze_output_wav(path: &Path, sample_rate: u32) -> Result<AudioAnalysis, CliFailure> {
    let prepared =
        prepare_audio_file(path, f64::from(sample_rate)).map_err(|error| CliFailure {
            code: 3,
            diagnostics: vec![
                Diagnostic::error(
                    DiagnosticCode::AssetDecodeFailed,
                    "could not read output WAV for analysis",
                )
                .with_path(path.to_string_lossy())
                .with_detail(error.to_string()),
            ],
        })?;
    let channels = match prepared.channels {
        PreparedAudioChannels::Stereo { left, right } => vec![left.to_vec(), right.to_vec()],
        PreparedAudioChannels::Mono { .. } => {
            return Err(CliFailure {
                code: 3,
                diagnostics: vec![
                    Diagnostic::error(
                        DiagnosticCode::RenderError,
                        "output WAV for analysis must be stereo",
                    )
                    .with_path(path.to_string_lossy()),
                ],
            });
        }
    };
    analyze_audio(
        &sonalloy_core::RenderedAudio {
            sample_rate,
            channels,
        },
        None,
    )
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

fn prepare_render_audio_input(
    value: Option<&str>,
    required_channels: usize,
    sample_rate: u32,
) -> Result<Option<sonalloy_core::PreparedAudio>, CliFailure> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.eq_ignore_ascii_case("silence") {
        let source_metadata = sonalloy_core::SampleMetadata {
            source_sample_rate: sample_rate,
            source_channels: required_channels,
            bits_per_sample: Some(32),
            source_frames: 1,
        };
        let channels = match required_channels {
            1 => sonalloy_core::PreparedAudioChannels::Mono {
                samples: Arc::from([0.0_f32]),
            },
            2 => sonalloy_core::PreparedAudioChannels::Stereo {
                left: Arc::from([0.0_f32]),
                right: Arc::from([0.0_f32]),
            },
            actual => {
                return Err(CliFailure {
                    code: 2,
                    diagnostics: vec![Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        format!("unsupported external audio channel count {actual}"),
                    )],
                });
            }
        };
        return Ok(Some(sonalloy_core::PreparedAudio {
            sample_rate: f64::from(sample_rate),
            frames: 1,
            source_metadata,
            channels,
        }));
    }
    let path = Path::new(value);
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

fn load_render_audio_input(
    common: &OfflineRenderCommonArgs,
    compiled: &CompiledInstrument,
) -> Result<Option<sonalloy_core::PreparedAudio>, CliFailure> {
    let required = compiled.required_input_channels() > 0;
    if required && common.audio_input.is_none() {
        return Err(CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::AudioInputRequired,
                "external audio input is required; specify --audio-input <WAV> or --audio-input silence",
            )],
        });
    }
    if !required && common.audio_input.is_some() {
        return Err(CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::DefinitionError,
                "external audio input is not used by this instrument",
            )],
        });
    }
    prepare_render_audio_input(
        common.audio_input.as_deref(),
        compiled.required_input_channels(),
        common.sample_rate,
    )
}

pub(crate) struct NoteReport {
    pub(crate) analysis: AudioAnalysis,
    pub(crate) loudness: Option<performance::AudioLoudnessReport>,
}

pub(crate) fn render_note_report(
    definition: &Path,
    note: u8,
    velocity: u8,
    gate: f64,
    tail: f64,
    audio_input: Option<&str>,
    include_loudness: bool,
) -> Result<NoteReport, CliFailure> {
    let (compiled, _) =
        super::load_and_compile(definition, DEFAULT_SAMPLE_RATE, DEFAULT_BLOCK_SIZE)?;
    let required_channels = compiled.required_input_channels();
    let external_audio = match audio_input {
        Some(value) if required_channels == 0 && value.eq_ignore_ascii_case("silence") => None,
        Some(_) if required_channels == 0 => {
            return Err(CliFailure {
                code: 2,
                diagnostics: vec![Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "external audio input is not used by this instrument",
                )],
            });
        }
        None if required_channels > 0 => {
            return Err(CliFailure {
                code: 2,
                diagnostics: vec![Diagnostic::error(
                    DiagnosticCode::AudioInputRequired,
                    "external audio input is required; specify --audio-input <WAV> or --audio-input silence",
                )],
            });
        }
        input => prepare_render_audio_input(input, required_channels, DEFAULT_SAMPLE_RATE)?,
    };
    let gate_frames = seconds_to_frames(gate, f64::from(DEFAULT_SAMPLE_RATE))
        .map_err(|error| input_failure(&error))?;
    let tail_frames = seconds_to_frames(tail, f64::from(DEFAULT_SAMPLE_RATE))
        .map_err(|error| input_failure(&error))?;
    let duration_frames = gate_frames.checked_add(1).ok_or_else(|| CliFailure {
        code: 2,
        diagnostics: vec![Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            "render duration overflows the frame counter",
        )],
    })?;
    let request = extend_request_for_latency(
        RenderRequest {
            sample_rate: f64::from(DEFAULT_SAMPLE_RATE),
            block_size: DEFAULT_BLOCK_SIZE,
            duration_frames,
            tail_frames,
        },
        compiled.reported_latency_frames,
    )?;
    let map =
        MusicalTimeMap::constant(DEFAULT_TEMPO_BPM).map_err(|error| render_failure(&error))?;
    let events = [
        ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: note,
                velocity,
            },
        },
        ScheduledEvent {
            absolute_frame: gate_frames,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        },
    ];
    let (mut audio, _, _) = render_offline_audio(
        &compiled,
        request,
        &events,
        &map,
        None,
        false,
        external_audio.as_ref(),
    )?;
    correct_rendered_audio(&mut audio, compiled.reported_latency_frames);
    let analysis = analyze_audio(&audio, Some(note_frequency_hz(note)))?;
    let loudness = include_loudness
        .then(|| analyze_audio_loudness(&audio, 0, audio.frames()))
        .transpose()?;
    Ok(NoteReport { analysis, loudness })
}

fn score_trace_requests(
    plan: &performance::PerformancePlan,
    args: &RenderScoreArgs,
) -> Result<Vec<Option<TraceRequest>>, CliFailure> {
    let mut ids = vec![Vec::new(); plan.parts.len()];
    for requested in &args.trace {
        let (part_id, parameter) = requested
            .split_once(':')
            .map_or((None, requested.as_str()), |(id, p)| (Some(id), p));
        let mut matched = false;
        for (index, part) in plan.parts.iter().enumerate() {
            if part_id.is_none_or(|id| id == part.definition.id)
                && part.instrument.parameter_handle(parameter).is_some()
            {
                ids[index].push(parameter.to_owned());
                matched = true;
            }
        }
        if !matched {
            return Err(CliFailure {
                code: 2,
                diagnostics: vec![
                    Diagnostic::error(
                        DiagnosticCode::ParameterNotFound,
                        "trace instrument or parameter does not exist",
                    )
                    .with_path("--trace")
                    .with_detail(requested),
                ],
            });
        }
    }
    plan.parts
        .iter()
        .zip(ids)
        .map(|(part, ids)| resolve_trace_request(&part.instrument, &ids, args.trace_every_frames))
        .collect()
}

#[cfg(test)]
mod mastering_tests {
    use super::*;

    #[test]
    fn part_failure_maps_preroll_frames_to_the_global_form_occurrence() {
        let directory = tempfile::tempdir().expect("directory");
        let (mut definition, args) = fixture(directory.path());
        definition["form"] = serde_json::json!([
            {"section":"phrase","bars":1,"tonic":"C","scale":"ionian"},
            {"section":"phrase","bars":1,"tonic":"C","scale":"ionian"},
            {"section":"phrase","bars":1,"tonic":"C","scale":"ionian"}
        ]);
        std::fs::write(
            &args.score,
            serde_json::to_string_pretty(&definition).expect("Score YAML"),
        )
        .expect("score file");
        let mut plan = performance::load(
            &args.score,
            &args.bindings,
            args.sample_rate,
            args.block_size,
        )
        .expect("Score compiles");
        performance::select_range(&mut plan, 1_920, 5_760, 3_840)
            .expect("selected output retains its preroll");

        let block_start_frame = 110_000_u64;
        let mut failure = crate::output::render_failure(&sonalloy_core::RenderError::ProcessAt {
            error: sonalloy_core::ProcessError::DspFailure {
                kind: sonalloy_core::DspFailureKind::InvalidInput,
            },
            block_start_frame,
        });
        failure.diagnostics[0].context = Some(Box::new(DiagnosticContext {
            block_start_frame: Some(block_start_frame),
            ..DiagnosticContext::default()
        }));
        let failure = prefix_part_failure(failure, 0, &plan, 48_000.0);
        let diagnostic = &failure.diagnostics[0];
        let context = diagnostic.context.as_deref().expect("render context");
        assert_eq!(context.part.as_deref(), Some("bass"));
        assert_eq!(context.section.as_deref(), Some("phrase"));
        assert_eq!(context.occurrence, Some(3));
        assert_eq!(context.block_start_frame, Some(206_000));
        assert_eq!(context.tail, Some(false));
        assert!((context.time_seconds.expect("score time") - 206_000.0 / 48_000.0).abs() < 1e-9);
        let value = crate::output::diagnostic_value(diagnostic);
        assert_eq!(value["part"], "bass");
        assert_eq!(value["occurrence"], 3);
        assert_eq!(value["block_start_frame"], 206_000);
    }

    #[test]
    fn silent_audio_input_prepares_the_requested_mono_and_stereo_channels() {
        for channels in [1, 2] {
            let input = prepare_render_audio_input(Some("silence"), channels, 44_100)
                .expect("silence input prepares")
                .expect("external input exists");
            assert_eq!(input.frames, 1);
            assert_eq!(input.sample_rate, 44_100.0);
            match input.channels {
                PreparedAudioChannels::Mono { samples } => {
                    assert_eq!(channels, 1);
                    assert_eq!(samples.as_ref(), [0.0]);
                }
                PreparedAudioChannels::Stereo { left, right } => {
                    assert_eq!(channels, 2);
                    assert_eq!(left.as_ref(), [0.0]);
                    assert_eq!(right.as_ref(), [0.0]);
                }
            }
        }
    }

    #[test]
    fn partial_section_analysis_reports_the_rendered_subrange() {
        let directory = tempfile::tempdir().expect("directory");
        let (mut definition, mut args) = fixture(directory.path());
        definition["form"][0]["bars"] = serde_json::json!(4);
        definition["sections"]["phrase"]["placements"] = serde_json::json!([
            {"instrument":"bass","degree":"1","at":{"bar":2,"step":1},"duration_quarters":1}
        ]);
        args.bars = Some("2-3".to_owned());
        args.analyze = true;
        args.stems_dir = None;
        std::fs::write(
            &args.score,
            serde_json::to_string_pretty(&definition).expect("Score YAML"),
        )
        .expect("score file");
        let mut plan = performance::load(
            &args.score,
            &args.bindings,
            args.sample_rate,
            args.block_size,
        )
        .expect("Score compiles");
        let selection = resolve_score_selection(&plan, &args).expect("bar range resolves");
        performance::select_range(
            &mut plan,
            selection.render_start_tick,
            selection.end_tick,
            selection.output_start_tick,
        )
        .expect("partial range applies");
        let tail_frames =
            seconds_to_frames(args.tail, f64::from(args.sample_rate)).expect("tail converts");
        let (mix, _, parts) = render_score_mix(&plan, &args, tail_frames).expect("mix renders");
        let report = analyze_score_sections(&plan, &mix, parts).expect("sections analyze");

        assert_eq!(report.len(), 1);
        assert_eq!(report[0].section, "phrase");
        assert_eq!(report[0].occurrence, 1);
        assert_eq!(report[0].start_frame, 96_000);
        assert_eq!(report[0].end_frame, 288_000);
        assert!((report[0].start_seconds - 2.0).abs() < 1e-9);
        assert!((report[0].end_seconds - 6.0).abs() < 1e-9);
        assert_eq!(report[0].parts.len(), 1);
        assert!(report[0].parts[0].rms > 0.0);
        assert!(matches!(
            report[0].loudness_status,
            performance::LoudnessStatus::Measured
        ));
        assert!(report[0].integrated_lufs.is_some_and(f64::is_finite));
        assert!(
            report[0].parts[0]
                .loudness
                .integrated_lufs
                .is_some_and(f64::is_finite)
        );
    }

    #[test]
    fn section_selection_uses_global_form_occurrences_and_validates_names() {
        let directory = tempfile::tempdir().expect("directory");
        let (mut definition, mut args) = fixture(directory.path());
        definition["form"] = serde_json::json!([
            {"section":"phrase","bars":1,"tonic":"C","scale":"ionian"},
            {"section":"bridge","bars":1,"tonic":"C","scale":"ionian"},
            {"section":"phrase","bars":1,"tonic":"C","scale":"ionian"}
        ]);
        definition["sections"]["bridge"] = serde_json::json!({});
        std::fs::write(
            &args.score,
            serde_json::to_string_pretty(&definition).expect("Score YAML"),
        )
        .expect("score file");
        let plan = performance::load(
            &args.score,
            &args.bindings,
            args.sample_rate,
            args.block_size,
        )
        .expect("Score compiles");

        args.section = Some("phrase".to_owned());
        args.occurrence = Some(3);
        let selection = resolve_score_selection(&plan, &args).expect("third form entry");
        assert_eq!(
            (selection.output_start_tick, selection.end_tick),
            (3_840, 5_760)
        );

        args.occurrence = Some(2);
        assert!(resolve_score_selection(&plan, &args).is_err());
        args.occurrence = None;
        assert!(resolve_score_selection(&plan, &args).is_err());

        args.section = Some("bridge".to_owned());
        let selection = resolve_score_selection(&plan, &args).expect("unique section");
        assert_eq!(
            (selection.output_start_tick, selection.end_tick),
            (1_920, 3_840)
        );
    }

    #[test]
    fn builtin_preview_scores_render_finite_audible_stereo() {
        let snapshots: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../../testdata/expected/score-auditions.sha256.json"
        ))
        .expect("audition catalog");
        let previews: Vec<_> = snapshots
            .iter()
            .filter_map(|v| v["score"].as_str())
            .filter(|p| p.ends_with("/preview.score.yaml"))
            .collect();
        assert_eq!(previews.len(), 109);
        for path in previews {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(path);
            let plan = performance::load(
                &path,
                &crate::performance::InstrumentBindings::default(),
                48000,
                257,
            )
            .expect("preview loads");
            let part = &plan.parts[0];
            let (audio, _) = render_compiled_pattern(
                &part.instrument,
                &part.render_lanes[0].compiled,
                48000.0,
                257,
                4800,
                None,
                None,
            )
            .expect("preview renders");
            assert_eq!(audio.channels.len(), 2, "{}", path.display());
            assert!(
                audio.channels.iter().flatten().all(|s| s.is_finite()),
                "{}",
                path.display()
            );
            assert!(
                audio.channels.iter().flatten().any(|s| s.abs() > 1e-5),
                "{} is silent",
                path.display()
            );
        }
    }

    #[test]
    fn part_pan_positions_stems_and_premaster_consistently() {
        let directory = tempfile::tempdir().expect("directory");
        let (definition, mut args) = fixture(directory.path());
        let samples = |path: &Path| {
            hound::WavReader::open(path)
                .expect("WAV")
                .into_samples::<f32>()
                .map(|sample| sample.expect("sample"))
                .collect::<Vec<_>>()
        };
        let mut stems: Vec<(Option<f64>, Vec<f32>)> = Vec::new();
        for pan in [None, Some(0.0), Some(-1.0), Some(1.0)] {
            let mut definition = definition.clone();
            definition["meta"]["fade_out_seconds"] = serde_json::json!(0.0);
            if let Some(pan) = pan {
                definition["instruments"]["bass"]["pan"] = serde_json::json!(pan);
            }
            std::fs::write(
                &args.score,
                serde_json::to_string_pretty(&definition).expect("score YAML"),
            )
            .expect("score");
            let label = pan.map_or("absent", |pan| if pan == 0.0 { "center" } else { "side" });
            let stems_dir = directory.path().join(format!("stems-{label}"));
            args.stems_dir = Some(stems_dir.clone());
            args.output = directory.path().join(format!("final-{label}.wav"));
            args.premaster_output = Some(directory.path().join(format!("premaster-{label}.wav")));
            assert_eq!(run_render_score(&args), ExitCode::SUCCESS);
            let stem = samples(&stems_dir.join("bass.wav"));
            let premaster = samples(args.premaster_output.as_deref().expect("premaster path"));
            let final_wav = samples(&args.output);
            assert_eq!(stem, premaster, "stem and premaster share the panned audio");
            assert_eq!(
                stem, final_wav,
                "final mix matches the stem without mastering"
            );
            stems.push((pan, stem));
        }
        let (absent, center, left, right) = (
            stems[0].1.as_slice(),
            stems[1].1.as_slice(),
            stems[2].1.as_slice(),
            stems[3].1.as_slice(),
        );
        assert_eq!(absent, center, "omitted pan equals pan 0");
        assert!(left.chunks(2).all(|frame| frame[1] == 0.0));
        assert!(left.chunks(2).any(|frame| frame[0] != 0.0));
        assert!(right.chunks(2).all(|frame| frame[0] == 0.0));
        assert!(right.chunks(2).any(|frame| frame[1] != 0.0));
    }

    #[test]
    fn mp3_true_peak_warning_triggers_only_above_the_limit() {
        assert!(
            mp3_true_peak_warning(-9.01, -9.0).is_none(),
            "the ceiling itself is within the limit"
        );
        assert!(
            mp3_true_peak_warning(-8.5, -9.0).is_none(),
            "typical encoder overshoot stays within the tolerance"
        );
        let warning = mp3_true_peak_warning(-7.9, -9.0).expect("abnormal overshoot warns");
        assert_eq!(
            warning.path.as_deref(),
            Some("meta.master.max_true_peak_db")
        );
        assert_eq!(warning.severity, sonalloy_core::DiagnosticSeverity::Warning);
        assert!(
            warning.message.contains("-7.90 dBTP") && warning.message.contains("1.10 dB above"),
            "the message reports the measured value and the overshoot: {}",
            warning.message
        );
    }

    #[test]
    fn mp3_render_measures_within_the_configured_limit() {
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            return;
        }
        let directory = tempfile::tempdir().expect("directory");
        let (mut definition, mut args) = fixture(directory.path());
        args.mp3_output = Some(directory.path().join("master.mp3"));
        let measure = |definition: &serde_json::Value, args: &mut RenderScoreArgs| {
            std::fs::write(
                &args.score,
                serde_json::to_string_pretty(definition).expect("score YAML"),
            )
            .expect("score");
            let report = execute_render_score(args).expect("render succeeds");
            let peak = report
                .mp3_measurement
                .as_ref()
                .expect("MP3 is measured")
                .true_peak_db
                .expect("MP3 true peak");
            let warned = report.diagnostics.iter().any(|diagnostic| {
                diagnostic.path.as_deref() == Some("meta.master.max_true_peak_db")
            });
            (peak, warned)
        };

        definition["meta"]["master"] = serde_json::json!({"target_lufs":-11,"max_true_peak_db":-9});
        let (peak, warned) = measure(&definition, &mut args);
        assert!(!warned, "limited output at {peak} dBTP must not warn");
        assert!(
            peak <= -9.0,
            "the limiter pins the output just below the ceiling"
        );

        definition["meta"]["master"] = serde_json::json!({"target_lufs":-30,"max_true_peak_db":-3});
        let (peak, warned) = measure(&definition, &mut args);
        assert!(!warned, "quiet output at {peak} dBTP must not warn");
        assert!(peak < -3.0);
    }

    fn fixture(directory: &Path) -> (serde_json::Value, RenderScoreArgs) {
        let instrument = directory.join("instrument.json");
        std::fs::write(
            &instrument,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../presets/BASS/001-clean-sub-bass/definition.json"
            )),
        )
        .expect("instrument");
        let score_path = directory.join("score.yaml");
        let definition = serde_json::json!({
            "meta":{"title":"Master test","bpm":120,"meter":"4/4","tuning":"12edo","gate":1,"fade_out_seconds":0.2},
            "form":[{"section":"phrase","bars":1,"tonic":"C","scale":"ionian"}],
            "sections":{"phrase":{"placements":[{"instrument":"bass","degree":"1","at":{"bar":1,"step":1},"duration_quarters":1}]}},
            "instruments":{"bass":{"definition":instrument,"gain_db":-6}}
        });
        std::fs::write(
            &score_path,
            serde_json::to_string_pretty(&definition).expect("score YAML"),
        )
        .expect("score");
        let args = RenderScoreArgs {
            score: score_path,
            trace: Vec::new(),
            trace_every_frames: None,
            bindings: crate::performance::InstrumentBindings::default(),
            section: None,
            occurrence: None,
            bars: None,
            parts: Vec::new(),
            mutes: Vec::new(),
            preroll_bars: 0,
            apply_master: false,
            sample_rate: 48000,
            block_size: 257,
            tail: 0.2,
            stems_dir: Some(directory.join("stems")),
            premaster_output: Some(directory.join("premaster.wav")),
            mp3_output: None,
            analyze: false,
            output: directory.join("final.wav"),
            json: true,
        };
        (definition, args)
    }

    #[test]
    fn part_only_full_render_keeps_the_score_fade() {
        let directory = tempfile::tempdir().expect("directory");
        let (_, mut args) = fixture(directory.path());
        args.stems_dir = None;
        let plan = performance::load(
            &args.score,
            &args.bindings,
            args.sample_rate,
            args.block_size,
        )
        .expect("Score loads");
        let tail_frames =
            seconds_to_frames(args.tail, f64::from(args.sample_rate)).expect("tail converts");
        let full = render_score_mix(&plan, &args, tail_frames)
            .expect("full render")
            .0;

        args.parts = vec!["bass".to_owned()];
        let part_only = render_score_mix(&plan, &args, tail_frames)
            .expect("Part-only render")
            .0;

        assert_eq!(part_only.channels, full.channels);
        assert!(plan.definition.mix.fade_out_seconds > 0.0);
    }

    #[test]
    fn partial_render_applies_only_the_overlap_with_the_score_fade() {
        let directory = tempfile::tempdir().expect("directory");
        let (mut definition, mut args) = fixture(directory.path());
        definition["form"][0]["bars"] = serde_json::json!(2);
        definition["meta"]["fade_out_seconds"] = serde_json::json!(1.0);
        std::fs::write(
            &args.score,
            serde_json::to_vec_pretty(&definition).expect("Score YAML"),
        )
        .expect("score file");
        args.stems_dir = None;
        args.parts = vec!["bass".to_owned()];
        let mut plan = performance::load(
            &args.score,
            &args.bindings,
            args.sample_rate,
            args.block_size,
        )
        .expect("Score loads");
        let tail_frames =
            seconds_to_frames(args.tail, f64::from(args.sample_rate)).expect("tail converts");
        let full = render_score_mix(&plan, &args, tail_frames)
            .expect("full render")
            .0;

        args.bars = Some("2-2".to_owned());
        args.preroll_bars = 1;
        let selection = resolve_score_selection(&plan, &args).expect("range resolves");
        performance::select_range(
            &mut plan,
            selection.render_start_tick,
            selection.end_tick,
            selection.output_start_tick,
        )
        .expect("range applies");
        let partial = render_score_mix(&plan, &args, tail_frames)
            .expect("partial render")
            .0;

        let start_frame = 96_000;
        for channel in 0..2 {
            assert_eq!(
                partial.channels[channel],
                full.channels[channel][start_frame..]
            );
        }
    }

    #[test]
    fn premaster_applies_mix_gain_and_fade_and_survives_master_failure() {
        let directory = tempfile::tempdir().expect("directory");
        let (mut definition, mut args) = fixture(directory.path());

        assert_eq!(run_render_score(&args), ExitCode::SUCCESS);
        let samples = |path: &Path| {
            hound::WavReader::open(path)
                .expect("WAV")
                .into_samples::<f32>()
                .map(|sample| sample.expect("sample"))
                .collect::<Vec<_>>()
        };
        let premaster = samples(args.premaster_output.as_deref().expect("premaster path"));
        assert_eq!(premaster, samples(&args.output));
        let stem = samples(&args.stems_dir.as_ref().expect("stems").join("bass.wav"));
        let active = stem
            .iter()
            .position(|sample| sample.abs() > 0.001)
            .expect("audible stem");
        assert!((premaster[active] - stem[active]).abs() < 1.0e-6);
        assert!(premaster.last().expect("fade endpoint").abs() < f32::EPSILON);

        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            return;
        }
        definition["meta"]["master"] =
            serde_json::json!({"target_lufs":-9,"max_true_peak_db":-1,"target_lra_lu":7});
        std::fs::write(
            &args.score,
            serde_json::to_string_pretty(&definition).expect("score YAML"),
        )
        .expect("score");
        args.analyze = true;
        assert_eq!(run_render_score(&args), ExitCode::SUCCESS);
        let measurement =
            performance::measure_loudness(&args.output).expect("master output measurement");
        assert!((measurement.integrated_lufs.expect("LUFS") + 9.0).abs() <= 0.5);
        assert!(measurement.true_peak_db.expect("true peak") <= -1.0);
        assert_eq!(
            premaster,
            samples(args.premaster_output.as_deref().expect("premaster path"))
        );
        definition["instruments"]["bass"]["gain_db"] = (-1000).into();
        std::fs::write(
            &args.score,
            serde_json::to_string_pretty(&definition).expect("silent score"),
        )
        .expect("score");
        std::fs::write(&args.output, b"existing completed output").expect("existing output");

        assert_ne!(run_render_score(&args), ExitCode::SUCCESS);
        assert_eq!(
            std::fs::read(&args.output).expect("existing output retained"),
            b"existing completed output"
        );
        assert!(
            samples(args.premaster_output.as_deref().expect("premaster path"))
                .iter()
                .all(|sample| sample.abs() < f32::EPSILON)
        );
    }
}
