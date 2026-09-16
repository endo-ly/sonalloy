use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use serde::Serialize;
use sonalloy_core::{Diagnostic, DiagnosticCode};

use crate::demo::{self, DemoInspection};
use crate::midi::export_demo;
use crate::output::{CliFailure, StatusReport, finish_failure, print_warnings};

#[derive(Debug, Subcommand)]
pub(super) enum DemoCommand {
    /// Validate a multi-instrument Demo.
    Validate(DemoPathArgs),
    /// Display Demo timing, parts, and mix settings.
    Inspect(DemoPathArgs),
    /// Convert all Demo parts into a Standard MIDI File Type 1.
    ExportMidi(DemoExportMidiArgs),
}

#[derive(Debug, Args)]
pub(super) struct DemoPathArgs {
    /// Demo JSON path.
    demo: PathBuf,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub(super) struct DemoExportMidiArgs {
    /// Demo JSON path.
    demo: PathBuf,
    /// Destination Standard MIDI File path.
    #[arg(long)]
    output: PathBuf,
    /// Emit machine-readable JSON.
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
