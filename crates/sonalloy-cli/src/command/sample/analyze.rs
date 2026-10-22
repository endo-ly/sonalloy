use std::collections::BTreeSet;
use std::path::Path;
use std::process::ExitCode;

use serde::Serialize;
use sonalloy_core::{AudioAnalysis, Diagnostic};

use super::{AnalyzeArgs, analyze_file, diagnostic, emit, normalized, walk};

#[derive(Serialize)]
struct FileReport {
    path: String,
    status: &'static str,
    analysis: Option<AudioAnalysis>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Serialize)]
struct Report {
    status: &'static str,
    total: usize,
    succeeded: usize,
    failed: usize,
    files: Vec<FileReport>,
}

pub(super) fn run(args: &AnalyzeArgs) -> ExitCode {
    let mut paths = BTreeSet::new();
    let mut files = Vec::new();
    for input in &args.inputs {
        let mut discovered = BTreeSet::new();
        let result = collect(input, &mut discovered).and_then(|()| {
            if discovered.is_empty() {
                Err(super::failure(
                    Path::new(input),
                    "input contains no WAV files",
                ))
            } else {
                Ok(())
            }
        });
        paths.extend(discovered);
        if let Err(error) = result {
            files.push(FileReport {
                path: input.clone(),
                status: "error",
                analysis: None,
                diagnostics: error.diagnostics,
            });
        }
    }
    for path in paths {
        let (analysis, diagnostics) = match analyze_file(&path, args.reference_frequency) {
            Ok(analysis) => (Some(analysis), Vec::new()),
            Err(error) => (None, vec![error]),
        };
        files.push(FileReport {
            path: path.to_str().expect("normalized UTF-8 path").to_owned(),
            status: if analysis.is_some() { "ok" } else { "error" },
            analysis,
            diagnostics,
        });
    }
    let failed = files.iter().filter(|file| file.analysis.is_none()).count();
    let succeeded = files.len() - failed;
    let report = Report {
        status: if failed == 0 {
            "ok"
        } else if succeeded == 0 {
            "error"
        } else {
            "partial"
        },
        total: files.len(),
        succeeded,
        failed,
        files,
    };
    emit(&report, args.json, failed)
}

fn collect(
    input: &str,
    paths: &mut BTreeSet<std::path::PathBuf>,
) -> Result<(), crate::output::CliFailure> {
    let path = Path::new(input);
    // Existing literal paths may themselves contain glob metacharacters.
    if path.exists() {
        if path.is_dir() {
            walk(path, paths)?;
        } else {
            paths.insert(normalized(path)?);
        }
        return Ok(());
    }
    if input.contains(['*', '?', '[']) {
        let entries = glob::glob(input).map_err(|error| super::failure(path, error.to_string()))?;
        let mut matched = false;
        let mut diagnostics = Vec::new();
        for entry in entries {
            match entry {
                Ok(candidate) => {
                    matched = true;
                    let result = if candidate.is_dir() {
                        walk(&candidate, paths)
                    } else {
                        normalized(&candidate).map(|path| {
                            paths.insert(path);
                        })
                    };
                    if let Err(error) = result {
                        diagnostics.extend(error.diagnostics);
                    }
                }
                Err(error) => diagnostics.push(diagnostic(path, error.to_string())),
            }
        }
        if !matched && diagnostics.is_empty() {
            diagnostics.push(diagnostic(path, "glob matched no files"));
        }
        if diagnostics.is_empty() {
            Ok(())
        } else {
            Err(crate::output::CliFailure {
                code: 2,
                diagnostics,
            })
        }
    } else {
        normalized(path).map(|path| {
            paths.insert(path);
        })
    }
}
