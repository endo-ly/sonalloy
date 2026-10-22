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

use crate::output::{print_diagnostics, print_failure, print_warnings};

use super::{DEFAULT_BLOCK_SIZE, DEFAULT_SAMPLE_RATE, load_and_compile};

mod inspect;
mod sample;

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
    #[command(
        long_about = "Render one note from each supplied Definition, directory, or glob match and compare their audio analysis under the same Note, Velocity, Gate, Tail, and Sample Rate conditions. Repeat Definition inputs to compare selected files. `--loudness` adds Integrated LUFS and True Peak measurements and requires FFmpeg; `--audio-input silence` supplies silence to Definitions that require external audio. A failure in one input does not stop the remaining comparisons. Use `--json` for machine-readable results."
    )]
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
    /// Create a one-zone Sample instrument from this WAV.
    #[arg(long, value_name = "WAV")]
    from_sample: Option<PathBuf>,
    /// Instrument name (defaults to the WAV filename).
    #[arg(long, requires = "from_sample")]
    name: Option<String>,
    #[arg(long, default_value_t = 60, requires = "from_sample", value_parser = clap::value_parser!(u8).range(0..=127))]
    root_note: u8,
    #[arg(long, default_value_t = 0, requires = "from_sample", value_parser = clap::value_parser!(u8).range(0..=127))]
    key_min: u8,
    #[arg(long, default_value_t = 127, requires = "from_sample", value_parser = clap::value_parser!(u8).range(0..=127))]
    key_max: u8,
    #[arg(long, requires = "from_sample")]
    no_pitch_tracking: bool,
    #[arg(long, default_value = "release", requires = "from_sample")]
    note_off_behavior: sample::NoteOff,
    #[arg(long)]
    json: bool,
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
    /// Measure Integrated LUFS and True Peak with the rendered audio.
    #[arg(long)]
    loudness: bool,
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
    match sample::initialize(args) {
        Ok(()) => {
            if args.json {
                println!(
                    "{}",
                    serde_json::json!({"status": "ok", "output": args.path})
                );
            } else {
                println!("created {}", args.path.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => crate::output::finish_failure(args.json, error),
    }
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
    let collection = collect_definition_paths(&args.definitions);
    let mut files = collection
        .failures
        .into_iter()
        .map(|failure| BatchValidationFile {
            path: failure.path,
            status: "error",
            diagnostics: failure.diagnostics,
        })
        .collect::<Vec<_>>();
    for path in collection.paths {
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
    files.sort_by(|left, right| left.path.cmp(&right.path));
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

fn definition_path_diagnostic(input: &str, message: &str, detail: Option<String>) -> Diagnostic {
    let mut diagnostic =
        Diagnostic::error(DiagnosticCode::DefinitionError, message).with_path(input);
    diagnostic.detail = detail;
    diagnostic
}

#[derive(Debug)]
struct DefinitionInputFailure {
    path: String,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
struct DefinitionPathCollection {
    paths: Vec<PathBuf>,
    failures: Vec<DefinitionInputFailure>,
}

fn normalize_definition_path(
    path: &Path,
    input: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(path) => Some(path),
        Err(error) => {
            diagnostics.push(definition_path_diagnostic(
                input,
                "could not normalize instrument path",
                Some(format!("{}: {error}", path.display())),
            ));
            None
        }
    }
}

fn walk_definition_directory(
    directory: &Path,
    input: &str,
    paths: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            diagnostics.push(definition_path_diagnostic(
                input,
                "could not scan instrument directory",
                Some(format!("{}: {error}", directory.display())),
            ));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                diagnostics.push(definition_path_diagnostic(
                    input,
                    "could not read instrument directory entry",
                    Some(format!("{}: {error}", directory.display())),
                ));
                continue;
            }
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                diagnostics.push(definition_path_diagnostic(
                    input,
                    "could not inspect instrument directory entry",
                    Some(format!("{}: {error}", path.display())),
                ));
                continue;
            }
        };
        if file_type.is_dir() {
            walk_definition_directory(&path, input, paths, diagnostics);
        } else if path
            .file_name()
            .is_some_and(|name| name == "definition.json")
        {
            if let Some(path) = normalize_definition_path(&path, input, diagnostics) {
                paths.insert(path);
            }
        }
    }
}

fn collect_glob_definition_paths(
    input: &str,
    paths: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let entries = match glob::glob(input) {
        Ok(entries) => entries,
        Err(error) => {
            diagnostics.push(definition_path_diagnostic(
                input,
                "instrument glob pattern is invalid",
                Some(error.to_string()),
            ));
            return;
        }
    };
    let mut matched_file = false;
    for entry in entries {
        let candidate = match entry {
            Ok(candidate) => candidate,
            Err(error) => {
                diagnostics.push(definition_path_diagnostic(
                    input,
                    "could not read instrument glob result",
                    Some(error.to_string()),
                ));
                continue;
            }
        };
        match std::fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_file() => {
                matched_file = true;
                if let Some(path) = normalize_definition_path(&candidate, input, diagnostics) {
                    paths.insert(path);
                }
            }
            Ok(_) => {}
            Err(error) => diagnostics.push(definition_path_diagnostic(
                input,
                "could not inspect instrument glob result",
                Some(format!("{}: {error}", candidate.display())),
            )),
        }
    }
    if !matched_file && diagnostics.is_empty() {
        diagnostics.push(definition_path_diagnostic(
            input,
            "instrument glob matched no files",
            None,
        ));
    }
}

fn collect_definition_input(
    input: &str,
    paths: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if input.contains(['*', '?', '[']) {
        collect_glob_definition_paths(input, paths, diagnostics);
        return;
    }
    match std::fs::metadata(input) {
        Ok(metadata) if metadata.is_file() => {
            if let Some(path) = normalize_definition_path(Path::new(input), input, diagnostics) {
                paths.insert(path);
            }
        }
        Ok(metadata) if metadata.is_dir() => {
            walk_definition_directory(Path::new(input), input, paths, diagnostics);
            if paths.is_empty() && diagnostics.is_empty() {
                diagnostics.push(definition_path_diagnostic(
                    input,
                    "instrument directory contains no definition.json files",
                    None,
                ));
            }
        }
        Ok(_) => diagnostics.push(definition_path_diagnostic(
            input,
            "instrument input is not a file or directory",
            None,
        )),
        Err(error) => {
            let message = if error.kind() == std::io::ErrorKind::NotFound {
                "instrument input does not exist"
            } else {
                "could not inspect instrument input"
            };
            diagnostics.push(definition_path_diagnostic(
                input,
                message,
                Some(error.to_string()),
            ));
        }
    }
}

fn collect_definition_paths(inputs: &[String]) -> DefinitionPathCollection {
    let mut collected = BTreeSet::new();
    let mut failures = Vec::new();
    for input in inputs {
        let mut input_paths = BTreeSet::new();
        let mut diagnostics = Vec::new();
        collect_definition_input(input, &mut input_paths, &mut diagnostics);
        collected.extend(input_paths);
        if !diagnostics.is_empty() {
            diagnostics.sort_by(|left, right| {
                left.detail
                    .cmp(&right.detail)
                    .then_with(|| left.message.cmp(&right.message))
            });
            failures.push(DefinitionInputFailure {
                path: input.clone(),
                diagnostics,
            });
        }
    }
    failures.sort_by(|left, right| left.path.cmp(&right.path));
    DefinitionPathCollection {
        paths: collected.into_iter().collect(),
        failures,
    }
}

#[derive(Debug, Serialize)]
struct InstrumentReportFile {
    path: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    analysis: Option<sonalloy_core::AudioAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    loudness: Option<crate::performance::AudioLoudnessReport>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize)]
struct InstrumentReport {
    status: &'static str,
    note: u8,
    velocity: u8,
    gate_seconds: f64,
    tail_seconds: f64,
    sample_rate: u32,
    files: Vec<InstrumentReportFile>,
}

fn run_report(args: &ReportArgs) -> ExitCode {
    let collection = collect_definition_paths(&args.definitions);
    let mut files = collection
        .failures
        .into_iter()
        .map(|failure| InstrumentReportFile {
            path: failure.path,
            status: "error",
            analysis: None,
            loudness: None,
            diagnostics: failure.diagnostics,
        })
        .collect::<Vec<_>>();
    for path in collection.paths {
        let result = crate::command::render::render_note_report(
            &path,
            args.note,
            args.velocity,
            args.gate,
            args.tail,
            args.audio_input.as_deref(),
            args.loudness,
        );
        match result {
            Ok(note_report) => files.push(InstrumentReportFile {
                path: path.to_string_lossy().into_owned(),
                status: "ok",
                analysis: Some(note_report.analysis),
                loudness: note_report.loudness,
                diagnostics: Vec::new(),
            }),
            Err(failure) => files.push(InstrumentReportFile {
                path: path.to_string_lossy().into_owned(),
                status: "error",
                analysis: None,
                loudness: None,
                diagnostics: failure.diagnostics,
            }),
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let failed = files.iter().any(|file| file.status == "error");
    let report = InstrumentReport {
        status: if failed { "error" } else { "ok" },
        note: args.note,
        velocity: args.velocity,
        gate_seconds: args.gate,
        tail_seconds: args.tail,
        sample_rate: DEFAULT_SAMPLE_RATE,
        files,
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&report).expect("instrument report is serializable")
        );
    } else {
        print_report_text(&report);
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn print_report_text(report: &InstrumentReport) {
    println!(
        "note {}, velocity {}, gate {:.3}s, tail {:.3}s, sample rate {} Hz",
        report.note, report.velocity, report.gate_seconds, report.tail_seconds, report.sample_rate
    );
    for file in &report.files {
        if let Some(analysis) = &file.analysis {
            println!(
                "ok {}: peak {:?} dBFS, rms {:?} dBFS",
                file.path, analysis.level.peak_dbfs, analysis.level.rms_dbfs
            );
            if let Some(loudness) = file.loudness {
                let status = match loudness.status {
                    crate::performance::LoudnessStatus::Measured => "measured",
                    crate::performance::LoudnessStatus::Silent => "silent",
                    crate::performance::LoudnessStatus::Unmeasurable => "unmeasurable",
                };
                println!(
                    "  loudness: {status}, {:?} LUFS, {:?} dBTP",
                    loudness.integrated_lufs, loudness.true_peak_db
                );
            }
            let bands = analysis
                .spectrum
                .bands
                .iter()
                .map(|band| {
                    let ratio = band.energy_ratio.map_or_else(
                        || "n/a".to_owned(),
                        |ratio| format!("{:.1}%", ratio * 100.0),
                    );
                    format!("{:.0}–{:.0} Hz {ratio}", band.lower_hz, band.upper_hz)
                })
                .collect::<Vec<_>>()
                .join(", ");
            println!("  band energy ratio: {bands}");
        } else {
            println!("error {}", file.path);
            print_diagnostics(&file.diagnostics);
        }
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
