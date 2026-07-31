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
    /// The Definition schema version is not supported.
    #[serde(rename = "SCHEMA_UNSUPPORTED")]
    SchemaUnsupported,
    /// The Definition JSON cannot be parsed.
    #[serde(rename = "JSON_INVALID")]
    JsonInvalid,
    /// A required Definition field or collection is missing.
    #[serde(rename = "REQUIRED_FIELD_MISSING")]
    RequiredFieldMissing,
    /// A layer identifier is duplicated.
    #[serde(rename = "ID_DUPLICATED")]
    IdDuplicated,
    /// Invalid process or render request.
    #[serde(rename = "VALUE_OUT_OF_RANGE")]
    ValueOutOfRange,
    /// A layer key or velocity range is invalid.
    #[serde(rename = "LAYER_RANGE_INVALID")]
    LayerRangeInvalid,
    /// A generator is not supported by this build.
    #[serde(rename = "GENERATOR_UNSUPPORTED")]
    GeneratorUnsupported,
    /// A referenced asset is not available.
    #[serde(rename = "ASSET_NOT_FOUND")]
    AssetNotFound,
    /// A referenced asset hash does not match.
    #[serde(rename = "ASSET_HASH_MISMATCH")]
    AssetHashMismatch,
    /// A referenced asset cannot be decoded.
    #[serde(rename = "ASSET_DECODE_FAILED")]
    AssetDecodeFailed,
    /// A source asset was downmixed to the engine's internal format.
    #[serde(rename = "ASSET_DOWNMIXED")]
    AssetDownmixed,
    /// A relative asset path was absolute.
    #[serde(rename = "ASSET_ABSOLUTE_PATH")]
    AssetAbsolutePath,
    /// A referenced asset has no digest.
    #[serde(rename = "ASSET_HASH_MISSING")]
    AssetHashMissing,
    /// A source asset was resampled for the process configuration.
    #[serde(rename = "ASSET_RESAMPLED")]
    AssetResampled,
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
    /// Instrument Definition or schema validation failure.
    #[serde(rename = "DEFINITION_ERROR")]
    DefinitionError,
    /// Instrument compilation failure.
    #[serde(rename = "COMPILE_ERROR")]
    CompileError,
    /// Non-fatal compiler adjustment.
    #[serde(rename = "COMPILE_WARNING")]
    CompileWarning,
    /// A filter cutoff was limited by the process sample rate.
    #[serde(rename = "FILTER_CUTOFF_CLAMPED")]
    FilterCutoffClamped,
    /// Events violate the same-offset ordering contract.
    #[serde(rename = "EVENT_ORDER_INVALID")]
    EventOrderInvalid,
    /// MIDI input or conversion failure.
    #[serde(rename = "MIDI_ERROR")]
    MidiError,
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

    /// Construct a warning without a path.
    pub fn warning(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: DiagnosticSeverity::Warning,
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
    let code = match error {
        ProcessError::DspFailure { .. } => DiagnosticCode::DspError,
        ProcessError::EventOrderInvalid => DiagnosticCode::EventOrderInvalid,
        _ => DiagnosticCode::ProcessError,
    };
    Diagnostic::error(code, error.to_string())
}

/// Convert a render failure to a frontend diagnostic.
#[must_use]
pub fn from_render_error(error: &RenderError) -> Diagnostic {
    let code = match error {
        RenderError::Process(ProcessError::DspFailure { .. }) => DiagnosticCode::DspError,
        RenderError::Process(ProcessError::EventOrderInvalid) => DiagnosticCode::EventOrderInvalid,
        RenderError::Process(_) => DiagnosticCode::ProcessError,
        _ => DiagnosticCode::RenderError,
    };
    Diagnostic::error(code, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_order_errors_use_the_stable_event_code() {
        let process = from_process_error(&ProcessError::EventOrderInvalid);
        assert_eq!(process.code, DiagnosticCode::EventOrderInvalid);

        let render = from_render_error(&RenderError::Process(ProcessError::EventOrderInvalid));
        assert_eq!(render.code, DiagnosticCode::EventOrderInvalid);
    }
}
