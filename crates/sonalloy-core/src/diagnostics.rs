use serde::Serialize;

use crate::process::ProcessError;
use crate::render::RenderError;

/// Severity of a diagnostic returned to a frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DiagnosticSeverity {
    /// The requested operation cannot succeed.
    #[serde(rename = "error")]
    Error,
    /// The operation succeeded with a recoverable issue.
    #[serde(rename = "warning")]
    Warning,
    /// Informational status.
    #[serde(rename = "info")]
    Info,
}

/// Stable diagnostic categories used by CLI and adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DiagnosticCode {
    /// Invalid process or render request.
    #[serde(rename = "VALUE_OUT_OF_RANGE")]
    ValueOutOfRange,
    /// Process contract violation.
    #[serde(rename = "PROCESS_ERROR")]
    ProcessError,
    /// Native DSP boundary failure.
    #[serde(rename = "DSP_ERROR")]
    DspError,
    /// Offline rendering failure.
    #[serde(rename = "RENDER_ERROR")]
    RenderError,
    /// WAV output failure.
    #[serde(rename = "WAV_OUTPUT_ERROR")]
    WavOutputError,
}

/// A structured, frontend-neutral diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    /// Stable machine-readable category.
    pub code: DiagnosticCode,
    /// Diagnostic severity.
    pub severity: DiagnosticSeverity,
    /// Optional path associated with the issue.
    pub path: Option<String>,
    /// Human-readable summary.
    pub message: String,
    /// Optional implementation detail.
    pub detail: Option<String>,
}

impl Diagnostic {
    /// Construct an error without a path.
    pub fn error(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: DiagnosticSeverity::Error,
            path: None,
            message: message.into(),
            detail: None,
        }
    }

    /// Attach a path to a diagnostic.
    #[must_use]
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Attach a detailed cause.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// Convert a process failure to a frontend diagnostic.
#[must_use]
pub fn from_process_error(error: &ProcessError) -> Diagnostic {
    let code = if matches!(error, ProcessError::DspFailure { .. }) {
        DiagnosticCode::DspError
    } else {
        DiagnosticCode::ProcessError
    };
    Diagnostic::error(code, error.to_string())
}

/// Convert a render failure to a frontend diagnostic.
#[must_use]
pub fn from_render_error(error: &RenderError) -> Diagnostic {
    let code = if matches!(error, RenderError::Process(ProcessError::DspFailure { .. })) {
        DiagnosticCode::DspError
    } else if matches!(error, RenderError::Process(_)) {
        DiagnosticCode::ProcessError
    } else {
        DiagnosticCode::RenderError
    };
    Diagnostic::error(code, error.to_string())
}
