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
    /// A processor identifier has invalid syntax.
    #[serde(rename = "PROCESSOR_ID_INVALID")]
    ProcessorIdInvalid,
    /// A processor identifier is duplicated within one chain.
    #[serde(rename = "PROCESSOR_ID_DUPLICATED")]
    ProcessorIdDuplicated,
    /// A processor is placed outside its supported pipeline position.
    #[serde(rename = "PROCESSOR_PLACEMENT_INVALID")]
    ProcessorPlacementInvalid,
    /// Invalid process or render request.
    #[serde(rename = "VALUE_OUT_OF_RANGE")]
    ValueOutOfRange,
    /// A layer key or velocity range is invalid.
    #[serde(rename = "LAYER_RANGE_INVALID")]
    LayerRangeInvalid,
    /// A component identifier cannot form a canonical parameter identifier.
    #[serde(rename = "PARAMETER_ID_INVALID")]
    ParameterIdInvalid,
    /// A canonical parameter identifier cannot be resolved.
    #[serde(rename = "PARAMETER_NOT_FOUND")]
    ParameterNotFound,
    /// A user source identifier has invalid syntax.
    #[serde(rename = "SOURCE_ID_INVALID")]
    SourceIdInvalid,
    /// A user source identifier is duplicated or reserved.
    #[serde(rename = "SOURCE_ID_DUPLICATED")]
    SourceIdDuplicated,
    /// A route refers to a source that is not defined.
    #[serde(rename = "SOURCE_NOT_FOUND")]
    SourceNotFound,
    /// A source setting is outside its supported range.
    #[serde(rename = "SOURCE_VALUE_INVALID")]
    SourceValueInvalid,
    /// A route amount is outside its supported range.
    #[serde(rename = "ROUTE_AMOUNT_INVALID")]
    RouteAmountInvalid,
    /// A route target is invalid or unsupported.
    #[serde(rename = "ROUTE_TARGET_INVALID")]
    RouteTargetInvalid,
    /// A route uses a source scope that is not valid for its processor target.
    #[serde(rename = "GLOBAL_ROUTE_SCOPE_INVALID")]
    GlobalRouteScopeInvalid,
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
    /// A Wavetable asset cannot be split into valid frames.
    #[serde(rename = "WAVETABLE_LAYOUT_INVALID")]
    WavetableLayoutInvalid,
    /// A Wavetable could not be prepared as finite band tables.
    #[serde(rename = "WAVETABLE_PREPARATION_FAILED")]
    WavetablePreparationFailed,
    /// A Wavetable frame contains no meaningful signal.
    #[serde(rename = "WAVETABLE_SILENT_FRAME")]
    WavetableSilentFrame,
    /// A Wavetable frame has a significant DC offset.
    #[serde(rename = "WAVETABLE_DC_OFFSET")]
    WavetableDcOffset,
    /// A Wavetable exceeds the compiled resource limit.
    #[serde(rename = "GENERATOR_RESOURCE_LIMIT_EXCEEDED")]
    GeneratorResourceLimitExceeded,
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
    /// A Sample playback combination is not supported.
    #[serde(rename = "UNSUPPORTED_PLAYBACK_COMBINATION")]
    UnsupportedPlaybackCombination,
    /// A fixed Sample stretch ratio is outside the supported range.
    #[serde(rename = "INVALID_STRETCH_RATIO")]
    InvalidStretchRatio,
    /// A Sample source tempo is invalid.
    #[serde(rename = "INVALID_SOURCE_TEMPO")]
    InvalidSourceTempo,
    /// A time-stretch backend could not be prepared or processed.
    #[serde(rename = "STRETCH_BACKEND_FAILURE")]
    StretchBackendFailure,
    /// A granular region cannot be represented inside the prepared asset.
    #[serde(rename = "INVALID_GRAIN_REGION")]
    InvalidGrainRegion,
    /// A granular parameter is outside its supported range.
    #[serde(rename = "INVALID_GRAIN_PARAMETER")]
    InvalidGrainParameter,
    /// A Wave Sequence structure or step is invalid.
    #[serde(rename = "INVALID_SEQUENCE")]
    InvalidSequence,
    /// A Wave Sequence step duration is invalid.
    #[serde(rename = "INVALID_STEP_DURATION")]
    InvalidStepDuration,
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
        ProcessError::StretchRatioOutOfRange { .. } => DiagnosticCode::InvalidStretchRatio,
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
        RenderError::Process(ProcessError::StretchRatioOutOfRange { .. }) => {
            DiagnosticCode::InvalidStretchRatio
        }
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

    #[test]
    fn runtime_stretch_range_errors_use_the_stretch_code() {
        let process = from_process_error(&ProcessError::StretchRatioOutOfRange { ratio: 2.5 });
        assert_eq!(process.code, DiagnosticCode::InvalidStretchRatio);

        let render = from_render_error(&RenderError::Process(
            ProcessError::StretchRatioOutOfRange { ratio: 2.5 },
        ));
        assert_eq!(render.code, DiagnosticCode::InvalidStretchRatio);
    }
}
