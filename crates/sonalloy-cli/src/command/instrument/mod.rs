use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use sonalloy_core::{
    AdsrDefinition, Diagnostic, DiagnosticCode, InstrumentDefinition, InstrumentMetadata,
    LayerDefinition, LayerTriggerDefinition, OscillatorDefinition, OscillatorWaveform,
    PerformanceDefinition, ProcessorDefinition, VoiceStealingDefinition,
};

use crate::output::{CliFailure, finish_failure, print_diagnostics, print_failure, print_warnings};

use super::{DEFAULT_BLOCK_SIZE, DEFAULT_SAMPLE_RATE, load_and_compile};

mod inspect;

#[derive(Debug, Subcommand)]
pub(super) enum InstrumentCommand {
    /// Create a starter Instrument Definition JSON file.
    #[command(
        long_about = "Create a new Instrument Definition JSON file without replacing an existing destination. The starter Definition is a 16-voice polyphonic Saw oscillator with one low-pass filter and no external audio input."
    )]
    Init(InitArgs),
    /// Check one or more Definition JSON files and collect all diagnostics.
    #[command(
        long_about = "Parse and compile every supplied Definition JSON file. Files, directories, and CLI glob patterns are accepted; a failure in one file does not stop the remaining files. With `--json`, each file has its own status and diagnostics."
    )]
    Validate(BatchDefinitionArgs),
    /// Render and compare a single note from one or more Definitions.
    Report(ReportArgs),
    /// Inspect the instrument configuration produced from a Definition.
    #[command(
        long_about = "Compile the Definition and display the instrument configuration that will be used, including Parameters, Generators, Processors, Modulation, Macros, Vectors, and reported latency. Use `--json` for a machine-readable result. Compilation also checks referenced assets."
    )]
    Inspect(DefinitionArgs),
}

#[derive(Debug, Args)]
pub(super) struct InitArgs {
    /// Destination Definition path.
    #[arg(value_name = "PATH")]
    path: PathBuf,
}

#[derive(Debug, Args)]
pub(super) struct DefinitionArgs {
    /// Definition JSON path.
    #[arg(value_name = "DEFINITION")]
    definition: PathBuf,
    /// Emit machine-readable JSON results; execution failures include structured diagnostics.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub(super) struct BatchDefinitionArgs {
    /// Definition JSON path, directory, or glob pattern. Repeat for multiple inputs.
    #[arg(value_name = "DEFINITION", num_args = 1..)]
    definitions: Vec<String>,
    /// Emit machine-readable JSON results; execution failures include structured diagnostics.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub(super) struct ReportArgs {
    /// Definition JSON path, directory, or glob pattern. Repeat for multiple inputs.
    #[arg(value_name = "DEFINITION", num_args = 1..)]
    definitions: Vec<String>,
    /// MIDI note number (0..=127).
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u8).range(0..=127))]
    note: u8,
    /// MIDI velocity (1..=127).
    #[arg(long, default_value_t = 127, value_parser = clap::value_parser!(u8).range(1..=127))]
    velocity: u8,
    /// Note On duration in seconds.
    #[arg(long, default_value_t = 0.5, value_parser = crate::command::parse_positive_f64)]
    gate: f64,
    /// Additional tail in seconds.
    #[arg(long, default_value_t = 0.5, value_parser = crate::command::parse_nonnegative_f64)]
    tail: f64,
    /// External Audio WAV, or `silence`.
    #[arg(long, value_name = "WAV|silence")]
    audio_input: Option<String>,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
}

pub(super) fn run(command: InstrumentCommand) -> ExitCode {
    match command {
        InstrumentCommand::Init(args) => run_init(&args),
        InstrumentCommand::Validate(args) => run_validate(&args),
        InstrumentCommand::Report(args) => run_report(&args),
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

#[derive(Debug, Serialize)]
struct BatchValidationFile {
    path: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize)]
struct BatchValidationReport {
    status: &'static str,
    command: &'static str,
    files: Vec<BatchValidationFile>,
    valid: usize,
    invalid: usize,
}

fn run_validate(args: &BatchDefinitionArgs) -> ExitCode {
    let paths = match collect_definition_paths(&args.definitions) {
        Ok(paths) => paths,
        Err(failure) => return finish_failure(args.json, failure),
    };
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        match load_and_compile(&path, DEFAULT_SAMPLE_RATE, DEFAULT_BLOCK_SIZE) {
            Ok((_, diagnostics)) => files.push(BatchValidationFile {
                path: path.to_string_lossy().into_owned(),
                status: "ok",
                diagnostics,
            }),
            Err(failure) => files.push(BatchValidationFile {
                path: path.to_string_lossy().into_owned(),
                status: "error",
                diagnostics: failure.diagnostics,
            }),
        }
    }
    let valid = files.iter().filter(|file| file.status == "ok").count();
    let invalid = files.len() - valid;
    let report = BatchValidationReport {
        status: if invalid == 0 { "ok" } else { "error" },
        command: "instrument validate",
        files,
        valid,
        invalid,
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&report).expect("batch validation report is serializable")
        );
    } else {
        for file in &report.files {
            println!("{} {}", file.status, file.path);
            print_warnings(&file.diagnostics);
            for diagnostic in &file.diagnostics {
                if diagnostic.severity == sonalloy_core::DiagnosticSeverity::Error {
                    eprintln!("  error[{:?}]: {}", diagnostic.code, diagnostic.message);
                }
            }
        }
        println!(
            "validated {} files: {} valid, {} invalid",
            report.files.len(),
            valid,
            invalid
        );
    }
    if invalid == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn run_inspect(args: &DefinitionArgs) -> ExitCode {
    let result = load_and_compile(&args.definition, DEFAULT_SAMPLE_RATE, DEFAULT_BLOCK_SIZE);
    match result {
        Ok((compiled, diagnostics)) => {
            inspect::print(&compiled, diagnostics, args.json);
            ExitCode::SUCCESS
        }
        Err(failure) => {
            print_failure(args.json, &failure);
            ExitCode::from(failure.code)
        }
    }
}

fn definition_path_failure(input: &str, message: &str, detail: Option<String>) -> CliFailure {
    let mut diagnostic =
        Diagnostic::error(DiagnosticCode::DefinitionError, message).with_path(input);
    diagnostic.detail = detail;
    CliFailure {
        code: 2,
        diagnostics: vec![diagnostic],
    }
}

fn normalize_definition_path(path: &Path, input: &str) -> Result<PathBuf, CliFailure> {
    std::fs::canonicalize(path).map_err(|error| {
        definition_path_failure(
            input,
            "could not normalize instrument path",
            Some(error.to_string()),
        )
    })
}

fn collect_definition_paths(inputs: &[String]) -> Result<Vec<PathBuf>, CliFailure> {
    fn walk_definitions(directory: &Path, result: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                walk_definitions(&path, result)?;
            } else if path
                .file_name()
                .is_some_and(|name| name == "definition.json")
            {
                result.push(path);
            }
        }
        Ok(())
    }

    let mut collected = BTreeSet::new();
    for input in inputs {
        let path = Path::new(input);
        if path.is_dir() {
            let mut paths = Vec::new();
            walk_definitions(path, &mut paths).map_err(|error| {
                definition_path_failure(
                    input,
                    "could not scan instrument directory",
                    Some(error.to_string()),
                )
            })?;
            for path in paths {
                collected.insert(normalize_definition_path(&path, input)?);
            }
        } else if input.contains(['*', '?', '[']) {
            let entries = glob::glob(input).map_err(|error| {
                definition_path_failure(
                    input,
                    "instrument glob pattern is invalid",
                    Some(error.to_string()),
                )
            })?;
            let mut matched = false;
            for entry in entries {
                let candidate = entry.map_err(|error| {
                    definition_path_failure(
                        input,
                        "could not read instrument glob result",
                        Some(error.to_string()),
                    )
                })?;
                if candidate.is_file() {
                    collected.insert(normalize_definition_path(&candidate, input)?);
                    matched = true;
                }
            }
            if !matched {
                return Err(definition_path_failure(
                    input,
                    "instrument glob matched no files",
                    None,
                ));
            }
        } else if path.is_file() {
            collected.insert(normalize_definition_path(path, input)?);
        } else {
            return Err(definition_path_failure(
                input,
                "instrument input does not exist",
                None,
            ));
        }
    }
    if collected.is_empty() {
        return Err(CliFailure {
            code: 2,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::DefinitionError,
                "instrument input matched no Definition files",
            )],
        });
    }
    Ok(collected.into_iter().collect())
}

#[derive(Debug, Serialize)]
struct InstrumentReportFile {
    path: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    analysis: Option<sonalloy_core::AudioAnalysis>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize)]
struct InstrumentReport {
    status: &'static str,
    note: u8,
    velocity: u8,
    files: Vec<InstrumentReportFile>,
}

fn run_report(args: &ReportArgs) -> ExitCode {
    let paths = match collect_definition_paths(&args.definitions) {
        Ok(paths) => paths,
        Err(failure) => return finish_failure(args.json, failure),
    };
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let result = crate::command::render::render_note_report(
            &path,
            args.note,
            args.velocity,
            args.gate,
            args.tail,
            args.audio_input.as_deref(),
        );
        match result {
            Ok(analysis) => files.push(InstrumentReportFile {
                path: path.to_string_lossy().into_owned(),
                status: "ok",
                analysis: Some(analysis),
                diagnostics: Vec::new(),
            }),
            Err(failure) => files.push(InstrumentReportFile {
                path: path.to_string_lossy().into_owned(),
                status: "error",
                analysis: None,
                diagnostics: failure.diagnostics,
            }),
        }
    }
    let failed = files.iter().any(|file| file.status == "error");
    let report = InstrumentReport {
        status: if failed { "error" } else { "ok" },
        note: args.note,
        velocity: args.velocity,
        files,
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&report).expect("instrument report is serializable")
        );
    } else {
        for file in &report.files {
            if let Some(analysis) = &file.analysis {
                println!(
                    "ok {}: peak {:?} dBFS, rms {:?} dBFS",
                    file.path, analysis.level.peak_dbfs, analysis.level.rms_dbfs
                );
            } else {
                println!("error {}", file.path);
                print_diagnostics(&file.diagnostics);
            }
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn default_definition() -> InstrumentDefinition {
    InstrumentDefinition {
        schema_version: sonalloy_core::CURRENT_SCHEMA_VERSION,
        external_audio: None,
        metadata: InstrumentMetadata {
            name: "Basic Poly Synth".to_owned(),
            author: None,
            description: Some("A headless oscillator instrument".to_owned()),
            category: None,
            tags: Vec::new(),
            recommended_range: None,
        },
        performance: PerformanceDefinition::Polyphonic {
            polyphony: 16,
            voice_stealing: VoiceStealingDefinition::QuietestReleasingThenOldest,
            choke_groups: Vec::new(),
        },
        layers: vec![LayerDefinition {
            id: "body".to_owned(),
            enabled: true,
            choke_group: None,
            trigger: LayerTriggerDefinition {
                event: sonalloy_core::LayerTriggerEvent::NoteOn,
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
                mode: sonalloy_core::FilterModeDefinition::LowPass,
                cutoff_hz: 12_000.0,
                resonance: 0.12,
            },
        )],
        global_processors: Vec::new(),
        modulation: None,
        macros: Vec::new(),
        vectors: Vec::new(),
    }
}
