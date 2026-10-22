use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand};
use serde::Serialize;
use sonalloy_core::{
    AudioAnalysis, AudioAnalysisOptions, Diagnostic, DiagnosticCode, analyze_rendered_audio,
    read_audio_file,
};

use crate::output::{CliFailure, finish_failure};

mod analyze;
mod index;
mod search;

#[derive(Debug, Subcommand)]
pub(super) enum SampleCommand {
    /// Analyze WAV files, recursive directories, or glob patterns at their source sample rate.
    #[command(
        long_about = "Analyze WAV files, recursive directories, or glob patterns at their source sample rate. Duplicate paths are analyzed once. A failed input does not stop the remaining files; any failure gives exit code 1."
    )]
    Analyze(AnalyzeArgs),
    /// Create or incrementally update a recursive WAV index.
    #[command(
        long_about = "Index WAV files recursively without following directory symlinks. Re-running updates added, changed, and removed files using size and modification time. An existing index must belong to the same directory. Individual WAV failures are recorded; traversal or index errors preserve the existing index."
    )]
    Index(IndexArgs),
    /// Search saved indexes without decoding audio.
    #[command(
        long_about = "Search saved WAV indexes using AND conditions with inclusive bounds. Results reflect the last index update; re-run `sample index` after changing files.

Each WAV is returned once, and name matching uses every indexed relative path. Unavailable analysis leaves acoustic conditions unevaluated; an analyzed null value is a non-match. Any non-match excludes the WAV. Conflicting records that could affect the result produce diagnostics. Results are ordered by index and relative path before pagination."
    )]
    Search(search::SearchArgs),
}

#[derive(Debug, Args)]
pub(super) struct AnalyzeArgs {
    /// WAV file, directory, or glob pattern. Repeat for multiple inputs.
    #[arg(value_name = "INPUT", required = true, num_args = 1..)]
    inputs: Vec<String>,
    /// Known reference frequency for harmonic-energy analysis.
    #[arg(long, value_parser = reference_frequency)]
    reference_frequency: Option<f32>,
    /// Emit machine-readable JSON with counts and diagnostics.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub(super) struct IndexArgs {
    /// Directory containing the WAV files.
    directory: PathBuf,
    /// Index JSON to create or update.
    #[arg(long)]
    output: PathBuf,
    /// Decode audio and cache acoustic analysis as well as metadata.
    #[arg(long)]
    analyze: bool,
    /// Refresh metadata and discard cached analysis; combine with --analyze to recompute it.
    #[arg(long)]
    force: bool,
    /// Emit machine-readable JSON with counts and diagnostics.
    #[arg(long)]
    json: bool,
}

fn reference_frequency(value: &str) -> Result<f32, String> {
    let value = value
        .parse::<f32>()
        .map_err(|_| "expected a finite positive frequency")?;
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err("expected a finite positive frequency".to_owned())
    }
}

pub(super) fn run(command: SampleCommand) -> ExitCode {
    match command {
        SampleCommand::Analyze(args) => analyze::run(&args),
        SampleCommand::Index(args) => match index::run(&args) {
            Ok(report) => emit(&report, args.json, report.failed),
            Err(error) => finish_failure(args.json, error),
        },
        SampleCommand::Search(args) => match search::run(&args) {
            Ok(report) => emit(&report, args.json, 0),
            Err(error) => finish_failure(args.json, error),
        },
    }
}

fn diagnostic(path: &Path, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(DiagnosticCode::AssetDecodeFailed, message).with_path(path.to_string_lossy())
}

fn failure(path: &Path, message: impl Into<String>) -> CliFailure {
    CliFailure {
        code: 2,
        diagnostics: vec![
            Diagnostic::error(DiagnosticCode::DefinitionError, message)
                .with_path(path.to_string_lossy()),
        ],
    }
}

fn emit(report: &impl Serialize, json: bool, failed: usize) -> ExitCode {
    let result = if json {
        serde_json::to_string(report)
    } else {
        serde_json::to_string_pretty(report)
    };
    match result {
        Ok(text) => println!("{text}"),
        Err(error) => return finish_failure(json, failure(Path::new(""), error.to_string())),
    }
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn analyze_file(
    path: &Path,
    reference_frequency_hz: Option<f32>,
) -> Result<AudioAnalysis, Diagnostic> {
    let audio = read_audio_file(path).map_err(|error| diagnostic(path, error.to_string()))?;
    analyze_rendered_audio(
        &audio,
        AudioAnalysisOptions {
            reference_frequency_hz,
        },
    )
    .map_err(|error| diagnostic(path, error.to_string()))
}

fn is_wav(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
}

fn path_text(path: &Path) -> Result<String, CliFailure> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| failure(path, "path is not valid UTF-8"))
}

fn normalized(path: &Path) -> Result<PathBuf, CliFailure> {
    let path = std::fs::canonicalize(path).map_err(|error| failure(path, error.to_string()))?;
    path_text(&path)?;
    Ok(path)
}

/// A failed traversal is never treated as a complete snapshot for index deletion.
fn walk(directory: &Path, files: &mut BTreeSet<PathBuf>) -> Result<(), CliFailure> {
    let entries =
        std::fs::read_dir(directory).map_err(|error| failure(directory, error.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|error| failure(directory, error.to_string()))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|error| failure(&path, error.to_string()))?;
        if kind.is_dir() {
            walk(&path, files)?;
        } else if kind.is_file() && is_wav(&path) {
            files.insert(normalized(&path)?);
        }
    }
    Ok(())
}
