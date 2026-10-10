use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use serde::Serialize;
use sonalloy_core::Diagnostic;

use crate::demo::{self, DemoInspection};
use crate::midi::export_demo;
use crate::output::{CliFailure, StatusReport, finish_failure, print_warnings};

pub(super) const DEMO_JSON_HELP: &str = r"A Demo JSON object requires `schema_version` (currently `1`) and a `parts` array; `name` is optional and `mix` may be omitted. Unknown fields are rejected. `parts` must contain at least one Part. Each Part requires `id`, `instrument`, and `pattern`; `gain_db` defaults to 0.0, `midi_channel` and `audio_input` are optional. `mix.fade_out_seconds` defaults to 0.0 and `mix.master` is optional. A master object requires `integrated_lufs` (-70..=-5 LUFS) and `true_peak_db` (-9..=0 dB).

Part IDs are 1..=64 ASCII letters, digits, `.`, `_`, or `-`, must start with a letter or digit, and cannot be Windows reserved device names. IDs must be unique ignoring ASCII case. `gain_db` must be finite and yield a finite linear gain. `midi_channel`, when specified, is a 1-based channel from 1 through 16; explicit channels must be unique. Omitted channels are assigned the lowest unused channel numbers in Part order, after reserving explicit channels. Parts without an available channel can still be rendered as audio, but MIDI export fails.

Instrument and Pattern paths are resolved relative to the Demo JSON file; absolute paths are also accepted. All Part Patterns must use the same `ticks_per_beat`. Each shorter Pattern must match the longest Pattern's tempo and time-signature changes up to its own `length_ticks`. The Demo timeline begins at tick 0 and ends at the greatest Part Pattern length. `fade_out_seconds` must be finite and non-negative.

`audio_input` has one field, `part`, which names another Demo Part ID exactly. Instruments that require external audio must have `audio_input`; instruments that do not use external audio cannot specify it. Self-references, unknown Part IDs, and routing cycles are invalid. Each Source Part is rendered before the Part that consumes its audio. The input signal includes the Source Part's gain.";

#[derive(Debug, Subcommand)]
pub(super) enum DemoCommand {
    /// Validate a Demo and compile each referenced Instrument and Pattern.
    #[command(
        long_about = "Validate the Demo JSON, load and compile each referenced Instrument and Pattern, and check their shared timeline.",
        after_long_help = DEMO_JSON_HELP
    )]
    Validate(DemoPathArgs),
    /// Inspect Demo timing, parts, fade, and mastering requirements.
    #[command(
        long_about = "Report the Demo's schema version, shared tick resolution, timeline length and musical duration, tempo and time-signature changes, Part references, resolved MIDI channels, external audio connections, fade-out duration, and whether mastering requires `FFmpeg`. Human-readable output shows these summaries; `--json` also returns the complete `mix` object, including its mastering targets.",
        after_long_help = DEMO_JSON_HELP
    )]
    Inspect(DemoPathArgs),
    /// Export all Demo parts to a Standard MIDI File Type 1.
    #[command(
        long_about = "Write a Type 1 Standard MIDI File with a Conductor Track for the Demo name, tempo, and time signature, plus one Track per Part for its ID, channel, notes, sustain, pitch bend, mod wheel, and aftertouch. Parameter Change and Ramp events are counted in one MIDI_ERROR per Pattern with the Part index and ID. These events, overlapping notes with the same pitch, or Parts without an available MIDI channel cause export to fail. An existing destination is overwritten."
    )]
    ExportMidi(DemoExportMidiArgs),
    /// Export a self-contained Demo Bundle.
    #[command(
        long_about = "Package a Demo with its Patterns, Instrument Definitions, and assets. Use --with-render to include the final mix and all Part stems."
    )]
    Pack(DemoPackArgs),
}

#[derive(Debug, Args)]
pub(super) struct DemoPathArgs {
    /// Demo JSON path.
    #[arg(value_name = "DEMO")]
    demo: PathBuf,
    /// Emit machine-readable JSON results; execution failures include structured diagnostics.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub(super) struct DemoExportMidiArgs {
    /// Demo JSON path.
    #[arg(value_name = "DEMO")]
    demo: PathBuf,
    /// Destination Standard MIDI File path.
    #[arg(long, value_name = "PATH")]
    output: PathBuf,
    /// Emit machine-readable JSON results; execution failures include structured diagnostics.
    #[arg(long)]
    json: bool,
}

pub(super) fn run(command: DemoCommand) -> ExitCode {
    match command {
        DemoCommand::Validate(args) => run_validate(&args),
        DemoCommand::Inspect(args) => run_inspect(&args),
        DemoCommand::ExportMidi(args) => run_export_midi(&args),
        DemoCommand::Pack(args) => run_pack(&args),
    }
}

fn run_validate(args: &DemoPathArgs) -> ExitCode {
    let demo = match demo::load(
        &args.demo,
        super::DEFAULT_SAMPLE_RATE,
        super::DEFAULT_BLOCK_SIZE,
    ) {
        Ok(demo) => demo,
        Err(failure) => return finish_failure(args.json, failure),
    };
    let report = StatusReport {
        status: "ok",
        command: "demo validate",
        diagnostics: demo_diagnostics(&demo),
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&report).expect("status report is serializable")
        );
    } else {
        println!("valid {}", args.demo.display());
        print_warnings(&report.diagnostics);
    }
    ExitCode::SUCCESS
}

fn run_inspect(args: &DemoPathArgs) -> ExitCode {
    let demo = match demo::load(
        &args.demo,
        super::DEFAULT_SAMPLE_RATE,
        super::DEFAULT_BLOCK_SIZE,
    ) {
        Ok(demo) => demo,
        Err(failure) => return finish_failure(args.json, failure),
    };
    let inspection = demo::inspect(&demo);
    if args.json {
        let report = DemoInspectReport {
            status: "ok",
            command: "demo inspect",
            inspection,
        };
        println!(
            "{}",
            serde_json::to_string(&report).expect("Demo inspection report is serializable")
        );
    } else {
        print_inspection(&inspection);
    }
    ExitCode::SUCCESS
}

fn run_export_midi(args: &DemoExportMidiArgs) -> ExitCode {
    let demo = match demo::load(
        &args.demo,
        super::DEFAULT_SAMPLE_RATE,
        super::DEFAULT_BLOCK_SIZE,
    ) {
        Ok(demo) => demo,
        Err(failure) => return finish_failure(args.json, failure),
    };
    if let Err(diagnostics) = export_demo(&args.output, &demo) {
        return finish_failure(
            args.json,
            CliFailure {
                code: 2,
                diagnostics,
            },
        );
    }
    let report = DemoExportReport {
        status: "ok",
        command: "demo export-midi",
        output: args.output.to_string_lossy().into_owned(),
        diagnostics: demo_diagnostics(&demo),
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&report).expect("Demo export report is serializable")
        );
    } else {
        println!("created {}", args.output.display());
        print_warnings(&report.diagnostics);
    }
    ExitCode::SUCCESS
}

fn demo_diagnostics(demo: &demo::LoadedDemo) -> Vec<Diagnostic> {
    demo.diagnostics.clone()
}

fn print_inspection(inspection: &DemoInspection) {
    println!("Name: {}", inspection.name.as_deref().unwrap_or("none"));
    println!("Schema Version: {}", inspection.schema_version);
    println!("Parts: {}", inspection.part_count);
    println!("Ticks Per Beat: {}", inspection.ticks_per_beat);
    println!("Length Ticks: {}", inspection.length_ticks);
    println!(
        "Musical Duration: {:.6} seconds",
        inspection.musical_duration_seconds
    );
    println!("Tempo Changes: {}", inspection.tempo_change_count);
    println!(
        "Time Signature Changes: {}",
        inspection.time_signature_change_count
    );
    for part in &inspection.parts {
        println!(
            "Part {}: instrument={}, pattern={}, gain_db={:.2}, midi_channel={}",
            part.id,
            part.instrument.display(),
            part.pattern.display(),
            part.gain_db,
            part.midi_channel
                .map_or_else(|| "none".to_owned(), |channel| channel.to_string())
        );
        if let Some(audio_input) = &part.audio_input {
            println!("  External Audio Input: {}", audio_input.part);
        }
    }
    println!("Fade Out: {:.6} seconds", inspection.mix.fade_out_seconds);
    println!("FFmpeg Required: {}", inspection.ffmpeg_required);
    print_warnings(&inspection.diagnostics);
}

#[derive(Debug, Serialize)]
struct DemoInspectReport {
    status: &'static str,
    command: &'static str,
    #[serde(flatten)]
    inspection: DemoInspection,
}

#[derive(Debug, Serialize)]
struct DemoExportReport {
    status: &'static str,
    command: &'static str,
    output: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Args)]
pub(super) struct DemoPackArgs {
    /// Source Demo JSON path.
    #[arg(value_name = "DEMO")]
    demo: PathBuf,
    /// New Bundle directory; existing paths are rejected.
    #[arg(long, value_name = "DIRECTORY")]
    output: PathBuf,
    /// Include the final Mix and every Part Stem.
    #[arg(long)]
    with_render: bool,
    /// Render sample rate in Hz; recorded even without WAVs.
    #[arg(long, value_name = "HZ", default_value_t = super::DEFAULT_SAMPLE_RATE, value_parser = clap::value_parser!(u32).range(1..))]
    sample_rate: u32,
    /// Maximum process block size in frames.
    #[arg(long, value_name = "FRAMES", default_value_t = super::DEFAULT_BLOCK_SIZE, value_parser = super::parse_positive_usize)]
    block_size: usize,
    /// Additional tail in seconds (finite and non-negative).
    #[arg(long, value_name = "SECONDS", default_value_t = 1.0, value_parser = super::parse_nonnegative_f64)]
    tail: f64,
    /// Emit machine-readable results and structured diagnostics.
    #[arg(long)]
    json: bool,
}

fn run_pack(args: &DemoPackArgs) -> ExitCode {
    let settings = demo::RenderSettings {
        sample_rate: args.sample_rate,
        block_size: args.block_size,
        tail_seconds: args.tail,
    };
    let report: demo::PackReport =
        match demo::pack(&args.demo, &args.output, settings, args.with_render) {
            Ok(report) => report,
            Err(failure) => return finish_failure(args.json, failure),
        };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&report).expect("bundle report is serializable")
        );
    } else {
        println!("created {}", args.output.display());
        print_warnings(&report.diagnostics);
    }
    ExitCode::SUCCESS
}
