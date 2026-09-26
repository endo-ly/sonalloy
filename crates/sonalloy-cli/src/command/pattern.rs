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
    /// Create a valid one-bar Pattern JSON file.
    #[command(
        long_about = "Create a new valid Pattern JSON file without replacing an existing destination. The starter Pattern is one 4/4 bar at 120 BPM, with 480 ticks per beat and 1920 ticks total, plus one note at tick 0 (MIDI note 60, velocity 100, duration 480 ticks)."
    )]
    Init(PatternInitArgs),
    /// Validate a tick-based Pattern without an Instrument.
    #[command(
        long_about = r#"Validate a Pattern JSON object independently of any Instrument. Required top-level fields are `schema_version` (currently `1`), `ticks_per_beat`, `length_ticks`, `tempo_changes`, `time_signature_changes`, and `events`; `name` is optional. Unknown fields are rejected.

`ticks_per_beat` must be 1..=32767. `length_ticks` must be greater than zero. Each `tempo_changes` entry has `tick` and `bpm`; the first tick is 0, ticks are strictly ascending and less than `length_ticks`, and BPM must be finite and greater than zero. Each `time_signature_changes` entry has `tick`, `numerator`, and `denominator`; the first tick is 0, ticks are strictly ascending and less than `length_ticks`, the numerator is positive, and the denominator is a power of two from 1 through 128. Both change arrays must contain at least one entry.

`events` must contain at least one `note`. All event `tick` values are measured from the Pattern start. A note uses `tick`, `duration_ticks`, `note` (0..=127), and `velocity` (1..=127); its tick is less than `length_ticks`, its duration is positive, and its end tick does not exceed `length_ticks`. `sustain_pedal` uses `tick` and boolean `down`. `pitch_bend` uses `tick` and finite `value` in -1..=1. `mod_wheel` and `aftertouch` use `tick` and finite `value` in 0..=1. These control events, including `parameter_change`, may use ticks from 0 through `length_ticks`, including the endpoint.

`parameter_change` uses `tick`, `parameter` (a Parameter ID), and finite `native_value`. Its Parameter ID and value range are checked against the selected Instrument when rendered or auditioned. Event objects reject unknown fields."#
    )]
    Validate(PatternPathArgs),
    /// Inspect Pattern timing, notes, controls, and musical duration.
    #[command(
        long_about = "Report the tick resolution and length, musical duration, tempo and time signature changes, note count and note and velocity ranges, and counts of sustain, pitch bend, modulation wheel, aftertouch, and Parameter Change events. Use `--json` for a machine-readable result."
    )]
    Inspect(PatternPathArgs),
    /// Import one MIDI Note Channel into a Pattern JSON file.
    #[command(
        long_about = "Convert one Note Channel from a Standard MIDI File into a tick-based Pattern. `--channel` selects a 1-based MIDI channel from 1 through 16. If omitted, the only Note Channel is selected automatically; a file with no Note Channel fails, and a file with multiple Note Channels requires `--channel`. Notes from multiple tracks on the selected channel are merged. An unmatched Note Off is ignored with a warning; a Note On without a matching Note Off fails. The destination must not already exist. Tempo and time signature changes are imported; missing MIDI metadata uses 120 BPM and 4/4. With `--json`, success is reported as JSON and execution failures include structured diagnostics."
    )]
    ImportMidi(PatternImportMidiArgs),
    /// Export a Pattern as a Standard MIDI File.
    #[command(
        long_about = "Write a single-track Standard MIDI File (Type 0) using the Pattern tick resolution and selected 1-based channel. `--channel` accepts 1..=16. Notes, tempo and time signature changes, sustain pedal, pitch bend, mod wheel (CC1), and channel aftertouch are represented. Parameter Change events cannot be represented and cause the export to fail. Overlapping notes with the same pitch also fail because MIDI Note Off events do not identify a note instance. The destination must not already exist. With `--json`, success is reported as JSON and execution failures include structured diagnostics."
    )]
    ExportMidi(PatternExportMidiArgs),
}
#[derive(Debug, Args)]
pub(super) struct PatternInitArgs {
    /// Destination pattern path.
    #[arg(value_name = "PATH")]
    path: PathBuf,
}

#[derive(Debug, Args)]
pub(super) struct PatternPathArgs {
    /// Pattern JSON path.
    #[arg(value_name = "PATTERN")]
    pattern: PathBuf,
    /// Emit machine-readable JSON results; execution failures include structured diagnostics.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub(super) struct PatternImportMidiArgs {
    /// Standard MIDI File path.
    #[arg(value_name = "MIDI_FILE")]
    midi: PathBuf,
    /// Destination pattern JSON path.
    #[arg(long, value_name = "PATH")]
    output: PathBuf,
    /// 1-based MIDI Note Channel to import; required when the file has several.
    #[arg(long, value_name = "CHANNEL", value_parser = clap::value_parser!(u8).range(1..=16))]
    channel: Option<u8>,
    /// Emit machine-readable JSON results; execution failures include structured diagnostics.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub(super) struct PatternExportMidiArgs {
    /// Pattern JSON path.
    #[arg(value_name = "PATTERN")]
    pattern: PathBuf,
    /// Destination Standard MIDI File path.
    #[arg(long, value_name = "PATH")]
    output: PathBuf,
    /// 1-based MIDI channel for exported events (1..=16).
    #[arg(long, value_name = "CHANNEL", default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=16))]
    channel: u8,
    /// Emit machine-readable JSON results; execution failures include structured diagnostics.
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

pub(crate) fn load_pattern(path: &Path) -> Result<PatternDefinition, CliFailure> {
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
