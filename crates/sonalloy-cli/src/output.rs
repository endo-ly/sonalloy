use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Serialize;
use sonalloy_core::{
    AudioAnalysis, Diagnostic, DiagnosticCode, RenderError, RenderTraceReport, from_render_error,
};

#[derive(Debug, Serialize)]
pub(crate) struct SuccessReport {
    pub(crate) status: &'static str,
    pub(crate) sample_rate: u32,
    pub(crate) channels: usize,
    pub(crate) frames: usize,
    pub(crate) reported_latency_frames: usize,
    pub(crate) output: String,
    pub(crate) backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) analysis: Option<OfflineAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) trace: Option<RenderTraceReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reset_comparison: Option<ResetComparison>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize)]
pub(crate) struct OfflineAnalysis {
    #[serde(flatten)]
    pub(crate) audio: AudioAnalysis,
    pub(crate) loudness: crate::performance::LoudnessAnalysis,
}

#[derive(Debug, Serialize)]
pub(crate) struct ResetComparison {
    pub(crate) compatible: bool,
    pub(crate) max_abs_difference: f64,
    pub(crate) rms_difference: f64,
    pub(crate) different_sample_count: usize,
}

#[derive(Debug)]
pub(crate) struct CliFailure {
    pub(crate) code: u8,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

pub(crate) fn pending_wav(path: &Path) -> Result<tempfile::TempPath, CliFailure> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    tempfile::NamedTempFile::new_in(parent)
        .map(tempfile::NamedTempFile::into_temp_path)
        .map_err(|error| wav_commit_failure(path, &error))
}

pub(crate) fn output_identity(path: &Path) -> Result<std::path::PathBuf, CliFailure> {
    if path.file_name().is_none() {
        return Err(wav_commit_failure(
            path,
            &std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "output path must name a file",
            ),
        ));
    }
    if path.exists() {
        return std::fs::canonicalize(path).map_err(|error| wav_commit_failure(path, &error));
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| wav_commit_failure(path, &error))?
            .join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                resolved.push(component.as_os_str());
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            std::path::Component::Normal(name) => {
                let candidate = resolved.join(name);
                if candidate.exists() {
                    resolved = std::fs::canonicalize(candidate)
                        .map_err(|error| wav_commit_failure(path, &error))?;
                } else {
                    #[cfg(windows)]
                    let name = name.to_ascii_lowercase();
                    resolved.push(name);
                }
            }
        }
    }
    if resolved.file_name().is_none() {
        return Err(wav_commit_failure(
            path,
            &std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "output path must name a file",
            ),
        ));
    }
    Ok(resolved)
}

pub(crate) fn commit_wav(pending: tempfile::TempPath, output: &Path) -> Result<(), CliFailure> {
    pending
        .persist(output)
        .map_err(|error| wav_commit_failure(output, &error.error))
}

fn wav_commit_failure(path: &Path, error: &std::io::Error) -> CliFailure {
    CliFailure {
        code: 4,
        diagnostics: vec![
            Diagnostic::error(
                DiagnosticCode::WavOutputError,
                "could not commit wav output",
            )
            .with_path(path.to_string_lossy())
            .with_detail(error.to_string()),
        ],
    }
}

pub(crate) fn write_wav(
    path: &Path,
    audio: &sonalloy_core::RenderedAudio,
) -> Result<(), Diagnostic> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: audio.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|error| {
        Diagnostic::error(
            DiagnosticCode::WavOutputError,
            "could not create wav output",
        )
        .with_path(path.to_string_lossy())
        .with_detail(error.to_string())
    })?;
    for frame in 0..audio.frames() {
        writer
            .write_sample(audio.channels[0][frame])
            .map_err(|error| {
                Diagnostic::error(DiagnosticCode::WavOutputError, "could not write wav output")
                    .with_path(path.to_string_lossy())
                    .with_detail(error.to_string())
            })?;
        writer
            .write_sample(audio.channels[1][frame])
            .map_err(|error| {
                Diagnostic::error(DiagnosticCode::WavOutputError, "could not write wav output")
                    .with_path(path.to_string_lossy())
                    .with_detail(error.to_string())
            })?;
    }
    writer.finalize().map_err(|error| {
        Diagnostic::error(
            DiagnosticCode::WavOutputError,
            "could not finalize wav output",
        )
        .with_path(path.to_string_lossy())
        .with_detail(error.to_string())
    })?;
    Ok(())
}

pub(crate) fn input_failure(error: &RenderError) -> CliFailure {
    CliFailure {
        code: 2,
        diagnostics: vec![Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            error.to_string(),
        )],
    }
}

pub(crate) fn render_failure(error: &RenderError) -> CliFailure {
    let code = if matches!(
        error,
        RenderError::Process(_) | RenderError::ProcessAt { .. }
    ) {
        3
    } else {
        2
    };
    let diagnostic = match error {
        RenderError::TraceLimitExceeded { .. } => from_render_error(error),
        _ if code == 2 => Diagnostic::error(DiagnosticCode::ValueOutOfRange, error.to_string()),
        _ => from_render_error(error),
    };
    CliFailure {
        code,
        diagnostics: vec![diagnostic],
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn print_success(json: bool, report: SuccessReport) -> ExitCode {
    if json {
        println!(
            "{}",
            serde_json::to_string(&report).expect("success report is serializable")
        );
    } else {
        println!(
            "rendered {} frames at {} Hz to {} using {}",
            report.frames, report.sample_rate, report.output, report.backend
        );
        if let Some(analysis) = &report.analysis {
            let analysis = &analysis.audio;
            println!("analysis");
            match analysis.level.peak_dbfs {
                Some(peak) => println!("  peak: {peak:.2} dBFS"),
                None => println!("  peak: -inf dBFS"),
            }
            match analysis.level.rms_dbfs {
                Some(rms) => println!("  rms: {rms:.2} dBFS"),
                None => println!("  rms: -inf dBFS"),
            }
            if let Some(centroid) = analysis.spectrum.spectral_centroid_hz {
                println!("  centroid: {centroid:.1} Hz");
            } else {
                println!("  centroid: none");
            }
            match analysis.stereo.correlation {
                Some(correlation) => println!("  stereo correlation: {correlation:.3}"),
                None => println!("  stereo correlation: none"),
            }
            match (analysis.activity.first_frame, analysis.activity.last_frame) {
                (Some(first), Some(last)) => println!(
                    "  activity: frames {first}..{last} at {:.0} dBFS threshold",
                    analysis.activity.threshold_dbfs
                ),
                _ => println!(
                    "  activity: none at {:.0} dBFS threshold",
                    analysis.activity.threshold_dbfs
                ),
            }
            println!(
                "  large discontinuities: {}",
                analysis.continuity.large_delta_count
            );
        }
        if let Some(trace) = &report.trace {
            for parameter in &trace.parameters {
                let last_frame = parameter.observations.last().map_or_else(
                    || "none".to_owned(),
                    |observation| observation.frame.to_string(),
                );
                println!(
                    "trace {}: {} observations, last frame {}",
                    parameter.parameter,
                    parameter.observations.len(),
                    last_frame
                );
            }
        }
        if let Some(analysis) = &report.analysis {
            println!(
                "loudness: {}",
                serde_json::to_string(&analysis.loudness).expect("loudness serializes")
            );
        }
        print_warnings(&report.diagnostics);
    }
    ExitCode::SUCCESS
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn finish_failure(json: bool, failure: CliFailure) -> ExitCode {
    print_failure(json, &failure);
    ExitCode::from(failure.code)
}

pub(crate) fn print_failure(json: bool, failure: &CliFailure) {
    if json {
        #[derive(Serialize)]
        struct ErrorReport {
            status: &'static str,
            exit_code: u8,
            diagnostics: Vec<serde_json::Value>,
        }
        let report = ErrorReport {
            status: "error",
            exit_code: failure.code,
            diagnostics: failure.diagnostics.iter().map(diagnostic_value).collect(),
        };
        println!(
            "{}",
            serde_json::to_string(&report).expect("error report is serializable")
        );
    } else {
        for diagnostic in &failure.diagnostics {
            eprintln!("error[{:?}]: {}", diagnostic.code, diagnostic.message);
            if let Some(path) = &diagnostic.path {
                eprintln!("  path: {path}");
            }
            if let Some(context) = diagnostic.context.as_deref() {
                if let Some(part) = &context.part {
                    eprintln!("  part: {part}");
                }
                if let Some(section) = &context.section {
                    eprintln!("  section: {section}");
                }
                if let Some(occurrence) = context.occurrence {
                    eprintln!("  occurrence: {occurrence}");
                }
                if let Some(time_seconds) = context.time_seconds {
                    eprintln!("  time_seconds: {time_seconds:.6}");
                }
                if let Some(block_start_frame) = context.block_start_frame {
                    eprintln!("  block_start_frame: {block_start_frame}");
                }
                if context.tail == Some(true) {
                    eprintln!("  position: tail");
                }
            }
            if let Some(detail) = &diagnostic.detail {
                match serde_json::from_str::<serde_json::Value>(detail) {
                    Ok(fields) if fields["source_type"] == "score" => {
                        for key in [
                            "line",
                            "column",
                            "source_bar",
                            "section",
                            "occurrence",
                            "bar",
                            "step",
                            "actual",
                        ] {
                            if let Some(value) = fields.get(key).filter(|value| !value.is_null()) {
                                eprintln!("  {key}: {value}");
                            }
                        }
                    }
                    _ => eprintln!("  detail: {detail}"),
                }
            }
        }
    }
}

pub(crate) fn print_warnings(diagnostics: &[Diagnostic]) {
    let mut score_range_warnings = Vec::new();
    for diagnostic in diagnostics {
        if diagnostic.severity != sonalloy_core::DiagnosticSeverity::Warning {
            continue;
        }
        let detail = diagnostic
            .detail
            .as_deref()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
            .filter(|detail| detail["source_type"] == "score" && detail["midi_note"].is_number());
        if let Some(detail) = detail {
            score_range_warnings.push((diagnostic, detail));
        } else {
            print_warning(diagnostic);
        }
    }
    if !score_range_warnings.is_empty() {
        eprintln!(
            "warning: {} Score notes are outside their recommended ranges; showing at most 3",
            score_range_warnings.len()
        );
        for (diagnostic, detail) in score_range_warnings.iter().take(3) {
            print_warning(diagnostic);
            for key in [
                "line",
                "column",
                "source_path",
                "source_bar",
                "part",
                "section",
                "occurrence",
                "bar",
                "step",
                "midi_note",
                "recommended_range",
            ] {
                if let Some(value) = detail.get(key).filter(|value| !value.is_null()) {
                    eprintln!("  {key}: {value}");
                }
            }
        }
    }
}

fn print_warning(diagnostic: &Diagnostic) {
    eprintln!("warning[{:?}]: {}", diagnostic.code, diagnostic.message);
    if let Some(path) = &diagnostic.path {
        eprintln!("  path: {path}");
    }
}

pub(crate) fn print_diagnostics(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        eprintln!("error[{:?}]: {}", diagnostic.code, diagnostic.message);
        if let Some(path) = &diagnostic.path {
            eprintln!("  path: {path}");
        }
        if let Some(detail) = &diagnostic.detail {
            eprintln!("  detail: {detail}");
        }
    }
}

pub(crate) fn diagnostic_values(diagnostics: &[Diagnostic]) -> Vec<serde_json::Value> {
    diagnostics.iter().map(diagnostic_value).collect()
}

pub(crate) fn diagnostic_value(diagnostic: &Diagnostic) -> serde_json::Value {
    let mut value = serde_json::to_value(diagnostic).expect("diagnostic serializes");
    if let Some(detail) = diagnostic
        .detail
        .as_deref()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .filter(|v| v["source_type"] == "score")
    {
        let map = value.as_object_mut().expect("diagnostic object");
        map.remove("detail");
        for key in [
            "line",
            "column",
            "source_path",
            "source_bar",
            "section",
            "occurrence",
            "bar",
            "step",
            "actual",
            "expected",
            "part",
            "midi_note",
            "recommended_range",
        ] {
            if let Some(field) = detail.get(key) {
                map.insert(key.into(), field.clone());
            }
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::render_failure;
    use sonalloy_core::{DiagnosticCode, DspFailureKind, ProcessError, RenderError};

    #[test]
    fn native_error_maps_to_process_exit_and_diagnostic() {
        let error = RenderError::ProcessAt {
            error: ProcessError::DspFailure {
                kind: DspFailureKind::InvalidInput,
            },
            block_start_frame: 8,
        };
        let failure = render_failure(&error);
        assert_eq!(failure.code, 3);
        assert_eq!(failure.diagnostics[0].code, DiagnosticCode::DspError);
        assert_eq!(
            failure.diagnostics[0].severity,
            sonalloy_core::DiagnosticSeverity::Error
        );
    }
}
