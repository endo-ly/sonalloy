use std::path::Path;
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
    pub(crate) analysis: Option<AudioAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) trace: Option<RenderTraceReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reset_comparison: Option<ResetComparison>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ResetComparison {
    pub(crate) compatible: bool,
    pub(crate) max_abs_difference: f64,
    pub(crate) rms_difference: f64,
    pub(crate) different_sample_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct StatusReport {
    pub(crate) status: &'static str,
    pub(crate) command: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
pub(crate) struct CliFailure {
    pub(crate) code: u8,
    pub(crate) diagnostics: Vec<Diagnostic>,
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
    let code = if matches!(error, RenderError::Process(_)) {
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
        struct ErrorReport<'a> {
            status: &'static str,
            exit_code: u8,
            diagnostics: &'a [Diagnostic],
        }
        let report = ErrorReport {
            status: "error",
            exit_code: failure.code,
            diagnostics: &failure.diagnostics,
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
            if let Some(detail) = &diagnostic.detail {
                eprintln!("  detail: {detail}");
            }
        }
    }
}

pub(crate) fn print_warnings(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        if diagnostic.severity == sonalloy_core::DiagnosticSeverity::Warning {
            eprintln!("warning[{:?}]: {}", diagnostic.code, diagnostic.message);
            if let Some(path) = &diagnostic.path {
                eprintln!("  path: {path}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::render_failure;
    use sonalloy_core::{DiagnosticCode, DspFailureKind, ProcessError, RenderError};

    #[test]
    fn native_error_maps_to_process_exit_and_diagnostic() {
        let error = RenderError::Process(ProcessError::DspFailure {
            kind: DspFailureKind::InvalidInput,
        });
        let failure = render_failure(&error);
        assert_eq!(failure.code, 3);
        assert_eq!(failure.diagnostics[0].code, DiagnosticCode::DspError);
        assert_eq!(
            failure.diagnostics[0].severity,
            sonalloy_core::DiagnosticSeverity::Error
        );
    }
}
