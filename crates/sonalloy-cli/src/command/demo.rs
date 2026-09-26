use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use serde::Serialize;
use sonalloy_core::{Diagnostic, DiagnosticCode};

use crate::demo::{self, DemoInspection};
use crate::midi::export_demo;
use crate::output::{CliFailure, StatusReport, finish_failure, print_warnings};

pub(super) const DEMO_JSON_HELP: &str = r"A Demo JSON object requires `schema_version` (currently `1`) and a `parts` array; `name` is optional and `mix` may be omitted. Unknown fields are rejected. `parts` must contain at least one Part. Each Part requires `id`, `instrument`, and `pattern`; `gain_db` defaults to 0.0 and `midi_channel` is optional. `mix.fade_out_seconds` defaults to 0.0 and `mix.master` is optional. A master object requires `integrated_lufs` (-70..=-5 LUFS), `true_peak_db` (-9..=0 dB), and `loudness_range_lu` (1..=50 LU).

Part IDs are 1..=64 ASCII letters, digits, `.`, `_`, or `-`, must start with a letter or digit, and cannot be Windows reserved device names. IDs must be unique ignoring ASCII case. `gain_db` must be finite and yield a finite linear gain. `midi_channel`, when specified, is a 1-based channel from 1 through 16; explicit channels must be unique. Omitted channels are assigned the lowest unused channel numbers in Part order, after reserving explicit channels. Parts without an available channel can still be rendered as audio, but MIDI export fails.

Instrument and Pattern paths are resolved relative to the Demo JSON file; absolute paths are also accepted. All Part Patterns must use the same `ticks_per_beat`. Each shorter Pattern must match the longest Pattern's tempo and time-signature changes up to its own `length_ticks`. The Demo timeline begins at tick 0 and ends at the greatest Part Pattern length. `fade_out_seconds` must be finite and non-negative. Instruments that require external audio cannot be used in an offline Demo.";

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
        long_about = "Report the Demo's schema version, shared tick resolution, timeline length and musical duration, tempo and time-signature changes, Part references and resolved MIDI channels, fade-out duration, and whether mastering requires `FFmpeg`. Human-readable output shows these summaries; `--json` also returns the complete `mix` object, including its mastering targets.",
        after_long_help = DEMO_JSON_HELP
    )]
    Inspect(DemoPathArgs),
    /// Export all Demo parts to a Standard MIDI File Type 1.
    #[command(
        long_about = "Write a Type 1 Standard MIDI File with a Conductor Track for the Demo name, tempo, and time signature, plus one Track per Part for its ID, channel, notes, sustain, pitch bend, mod wheel, and aftertouch. Parameter Change events, overlapping notes with the same pitch, or Parts without an available MIDI channel cause export to fail. The destination must not already exist."
    )]
    ExportMidi(DemoExportMidiArgs),
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
        println!(
            "{}",
            serde_json::to_string(&inspection).expect("Demo inspection is serializable")
        );
    } else {
        print_inspection(&inspection);
    }
    ExitCode::SUCCESS
}

fn run_export_midi(args: &DemoExportMidiArgs) -> ExitCode {
    if args.output.exists() {
        return finish_failure(
            args.json,
            CliFailure {
                code: 2,
                diagnostics: vec![
                    Diagnostic::error(
                        DiagnosticCode::DefinitionError,
                        "destination already exists",
                    )
                    .with_path(args.output.to_string_lossy()),
                ],
            },
        );
    }
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
    }
    println!("Fade Out: {:.6} seconds", inspection.mix.fade_out_seconds);
    println!("FFmpeg Required: {}", inspection.ffmpeg_required);
    print_warnings(&inspection.diagnostics);
}

#[derive(Debug, Serialize)]
struct DemoExportReport {
    status: &'static str,
    command: &'static str,
    output: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<Diagnostic>,
}
