use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand};
use serde::Serialize;
use sonalloy_core::{Diagnostic, DiagnosticCode};

use crate::midi::{export_pattern, import_pattern, parse_midi};
use crate::pattern::{
    PatternDefinition, PatternInspection, default_pattern, inspect as inspect_pattern,
    validate as validate_pattern,
};

use crate::output::{CliFailure, StatusReport, finish_failure, print_warnings};

#[derive(Debug, Subcommand)]
pub(super) enum PatternCommand {
    /// Create a valid one-bar pattern.
    Init(PatternInitArgs),
    /// Validate a pattern without an Instrument.
    Validate(PatternPathArgs),
    /// Display pattern contents and musical duration.
    Inspect(PatternPathArgs),
    /// Convert one MIDI channel into a pattern.
    ImportMidi(PatternImportMidiArgs),
    /// Convert a pattern into a Standard MIDI File.
    ExportMidi(PatternExportMidiArgs),
}
#[derive(Debug, Args)]
pub(super) struct PatternInitArgs {
    /// Destination pattern path.
    path: PathBuf,
}

#[derive(Debug, Args)]
pub(super) struct PatternPathArgs {
    /// Pattern JSON path.
    pattern: PathBuf,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub(super) struct PatternImportMidiArgs {
    /// Standard MIDI File path.
    midi: PathBuf,
    /// Destination pattern JSON path.
    #[arg(long)]
    output: PathBuf,
    /// MIDI channel number from 1 to 16.
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=16))]
    channel: Option<u8>,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub(super) struct PatternExportMidiArgs {
    /// Pattern JSON path.
    pattern: PathBuf,
    /// Destination Standard MIDI File path.
    #[arg(long)]
    output: PathBuf,
    /// MIDI channel number from 1 to 16.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=16))]
    channel: u8,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
}

pub(super) fn run(command: PatternCommand) -> ExitCode {
    match command {
        PatternCommand::Init(args) => run_pattern_init(&args),
        PatternCommand::Validate(args) => run_pattern_validate(&args),
        PatternCommand::Inspect(args) => run_pattern_inspect(&args),
        PatternCommand::ImportMidi(args) => run_pattern_import_midi(&args),
        PatternCommand::ExportMidi(args) => run_pattern_export_midi(&args),
    }
}

fn run_pattern_init(args: &PatternInitArgs) -> ExitCode {
    if args.path.exists() {
        return finish_failure(
            false,
            CliFailure {
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
    }
    let json = match serde_json::to_string_pretty(&default_pattern()) {
        Ok(json) => json,
        Err(error) => {
            return finish_failure(
                false,
                CliFailure {
                    code: 4,
                    diagnostics: vec![
                        Diagnostic::error(
                            DiagnosticCode::DefinitionError,
                            "could not serialize default pattern",
                        )
                        .with_detail(error.to_string()),
                    ],
                },
            );
        }
    };
    if let Err(error) = std::fs::write(&args.path, format!("{json}\n")) {
        return finish_failure(
            false,
            CliFailure {
                code: 4,
                diagnostics: vec![
                    Diagnostic::error(DiagnosticCode::WavOutputError, "could not write pattern")
                        .with_path(args.path.to_string_lossy())
                        .with_detail(error.to_string()),
                ],
            },
        );
    }
    println!("created {}", args.path.display());
    ExitCode::SUCCESS
}

fn run_pattern_validate(args: &PatternPathArgs) -> ExitCode {
    let pattern = match load_pattern(&args.pattern) {
        Ok(pattern) => pattern,
        Err(failure) => return finish_failure(args.json, failure),
    };
    let diagnostics = validate_pattern(&pattern);
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == sonalloy_core::DiagnosticSeverity::Error)
    {
        return finish_failure(
            args.json,
            CliFailure {
                code: 2,
                diagnostics,
            },
        );
    }
    let report = StatusReport {
        status: "ok",
        command: "pattern validate",
        diagnostics,
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&report).expect("status report is serializable")
        );
    } else {
        println!("valid {}", args.pattern.display());
        print_warnings(&report.diagnostics);
    }
    ExitCode::SUCCESS
}

fn run_pattern_inspect(args: &PatternPathArgs) -> ExitCode {
    let pattern = match load_pattern(&args.pattern) {
        Ok(pattern) => pattern,
        Err(failure) => return finish_failure(args.json, failure),
    };
    let inspection = match inspect_pattern(&pattern) {
        Ok(inspection) => inspection,
        Err(diagnostics) => {
            return finish_failure(
                args.json,
                CliFailure {
                    code: 2,
                    diagnostics,
                },
            );
        }
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&inspection).expect("pattern inspection is serializable")
        );
    } else {
        print_pattern_inspection(&inspection);
    }
    ExitCode::SUCCESS
}

fn run_pattern_import_midi(args: &PatternImportMidiArgs) -> ExitCode {
    if let Some(failure) = destination_exists_failure(&args.output) {
        return finish_failure(args.json, failure);
    }
    let parsed = match parse_midi(&args.midi) {
        Ok(parsed) => parsed,
        Err(diagnostics) => {
            return finish_failure(
                args.json,
                CliFailure {
                    code: 2,
                    diagnostics,
                },
            );
        }
    };
    let (pattern, diagnostics) =
        match import_pattern(parsed, args.channel.map(|channel| channel - 1)) {
            Ok(result) => result,
            Err(diagnostics) => {
                return finish_failure(
                    args.json,
                    CliFailure {
                        code: 2,
                        diagnostics,
                    },
                );
            }
        };
    let json = match serde_json::to_string_pretty(&pattern) {
        Ok(json) => json,
        Err(error) => {
            return finish_failure(
                args.json,
                CliFailure {
                    code: 4,
                    diagnostics: vec![
                        Diagnostic::error(
                            DiagnosticCode::DefinitionError,
                            "could not serialize imported pattern",
                        )
                        .with_detail(error.to_string()),
                    ],
                },
            );
        }
    };
    if let Err(error) = std::fs::write(&args.output, format!("{json}\n")) {
        return finish_failure(
            args.json,
            CliFailure {
                code: 4,
                diagnostics: vec![
                    Diagnostic::error(DiagnosticCode::WavOutputError, "could not write pattern")
                        .with_path(args.output.to_string_lossy())
                        .with_detail(error.to_string()),
                ],
            },
        );
    }
    let report = PatternSuccessReport {
        status: "ok",
        command: "pattern import-midi",
        output: args.output.to_string_lossy().into_owned(),
        diagnostics,
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&report).expect("pattern report is serializable")
        );
    } else {
        println!("created {}", args.output.display());
        print_warnings(&report.diagnostics);
    }
    ExitCode::SUCCESS
}

fn run_pattern_export_midi(args: &PatternExportMidiArgs) -> ExitCode {
    if let Some(failure) = destination_exists_failure(&args.output) {
        return finish_failure(args.json, failure);
    }
    let pattern = match load_pattern(&args.pattern) {
        Ok(pattern) => pattern,
        Err(failure) => return finish_failure(args.json, failure),
    };
    if let Err(diagnostics) = export_pattern(&args.output, &pattern, args.channel - 1) {
        return finish_failure(
            args.json,
            CliFailure {
                code: 2,
                diagnostics,
            },
        );
    }
    let report = PatternSuccessReport {
        status: "ok",
        command: "pattern export-midi",
        output: args.output.to_string_lossy().into_owned(),
        diagnostics: Vec::new(),
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&report).expect("pattern report is serializable")
        );
    } else {
        println!("created {}", args.output.display());
    }
    ExitCode::SUCCESS
}

#[derive(Debug, Serialize)]
struct PatternSuccessReport {
    status: &'static str,
    command: &'static str,
    output: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<Diagnostic>,
}

pub(super) fn load_pattern(path: &Path) -> Result<PatternDefinition, CliFailure> {
    let text = std::fs::read_to_string(path).map_err(|error| CliFailure {
        code: 2,
        diagnostics: vec![
            Diagnostic::error(
                DiagnosticCode::DefinitionError,
                "could not read pattern input",
            )
            .with_path(path.to_string_lossy())
            .with_detail(error.to_string()),
        ],
    })?;
    serde_json::from_str(&text).map_err(|error| CliFailure {
        code: 1,
        diagnostics: vec![
            Diagnostic::error(DiagnosticCode::JsonInvalid, "could not parse pattern JSON")
                .with_path(path.to_string_lossy())
                .with_detail(format!(
                    "line {}, column {}: {error}",
                    error.line(),
                    error.column()
                )),
        ],
    })
}

fn destination_exists_failure(path: &Path) -> Option<CliFailure> {
    path.exists().then(|| CliFailure {
        code: 2,
        diagnostics: vec![
            Diagnostic::error(
                DiagnosticCode::DefinitionError,
                "destination already exists",
            )
            .with_path(path.to_string_lossy()),
        ],
    })
}

fn print_pattern_inspection(inspection: &PatternInspection) {
    println!("Name: {}", inspection.name.as_deref().unwrap_or("none"));
    println!("Schema Version: {}", inspection.schema_version);
    println!("Ticks Per Beat: {}", inspection.ticks_per_beat);
    println!("Length Ticks: {}", inspection.length_ticks);
    println!("Tempo Changes: {}", inspection.tempo_change_count);
    println!(
        "Time Signature Changes: {}",
        inspection.time_signature_change_count
    );
    println!("Notes: {}", inspection.note_count);
    println!(
        "Note Range: {}",
        format_range(inspection.note_min, inspection.note_max)
    );
    println!(
        "Velocity Range: {}",
        format_range(inspection.velocity_min, inspection.velocity_max)
    );
    println!("Sustain Events: {}", inspection.sustain_event_count);
    println!("Pitch Bend Events: {}", inspection.pitch_bend_event_count);
    println!("Mod Wheel Events: {}", inspection.mod_wheel_event_count);
    println!("Aftertouch Events: {}", inspection.aftertouch_event_count);
    println!("Parameter Changes: {}", inspection.parameter_change_count);
    println!(
        "Distinct Parameter IDs: {}",
        inspection.distinct_parameter_ids.len()
    );
    println!(
        "Musical Duration: {:.6} seconds",
        inspection.musical_duration_seconds
    );
}

fn format_range<T: std::fmt::Display>(min: Option<T>, max: Option<T>) -> String {
    match (min, max) {
        (Some(min), Some(max)) => format!("{min}..={max}"),
        _ => "none".to_owned(),
    }
}
