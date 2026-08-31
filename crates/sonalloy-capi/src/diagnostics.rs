#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::ptr;

use sonalloy_core::{Diagnostic, DiagnosticCode, DiagnosticSeverity};

use crate::types::{SonalloyDiagnosticView, SonalloyResult, SonalloyStringView};
use crate::{SonalloyDiagnostics, guard};

pub(crate) fn boxed(entries: Vec<Diagnostic>) -> *mut SonalloyDiagnostics {
    Box::into_raw(Box::new(SonalloyDiagnostics { entries }))
}

fn code_id(code: DiagnosticCode) -> u32 {
    match code {
        DiagnosticCode::SchemaUnsupported => 1,
        DiagnosticCode::JsonInvalid => 2,
        DiagnosticCode::RequiredFieldMissing => 3,
        DiagnosticCode::IdDuplicated => 4,
        DiagnosticCode::ProcessorIdInvalid => 5,
        DiagnosticCode::ProcessorIdDuplicated => 6,
        DiagnosticCode::ProcessorPlacementInvalid => 7,
        DiagnosticCode::ValueOutOfRange => 8,
        DiagnosticCode::LayerRangeInvalid => 9,
        DiagnosticCode::ParameterIdInvalid => 10,
        DiagnosticCode::ParameterNotFound => 11,
        DiagnosticCode::SourceIdInvalid => 12,
        DiagnosticCode::SourceIdDuplicated => 13,
        DiagnosticCode::SourceNotFound => 14,
        DiagnosticCode::SourceValueInvalid => 15,
        DiagnosticCode::RouteDepthInvalid => 16,
        DiagnosticCode::RouteDepthUnitInvalid => 17,
        DiagnosticCode::RouteTargetInvalid => 18,
        DiagnosticCode::GlobalRouteScopeInvalid => 19,
        DiagnosticCode::GeneratorUnsupported => 20,
        DiagnosticCode::AssetNotFound => 21,
        DiagnosticCode::AssetHashMismatch => 22,
        DiagnosticCode::AssetDecodeFailed => 23,
        DiagnosticCode::AssetDownmixed => 24,
        DiagnosticCode::AssetAbsolutePath => 25,
        DiagnosticCode::AssetHashMissing => 26,
        DiagnosticCode::AssetResampled => 27,
        DiagnosticCode::WavetableLayoutInvalid => 28,
        DiagnosticCode::WavetablePreparationFailed => 29,
        DiagnosticCode::WavetableSilentFrame => 30,
        DiagnosticCode::WavetableDcOffset => 31,
        DiagnosticCode::SpectralPreparationFailed => 32,
        DiagnosticCode::GeneratorResourceLimitExceeded => 33,
        DiagnosticCode::ProcessError => 34,
        DiagnosticCode::DspError => 35,
        DiagnosticCode::RenderError => 36,
        DiagnosticCode::WavOutputError => 37,
        DiagnosticCode::DefinitionError => 38,
        DiagnosticCode::CompileError => 39,
        DiagnosticCode::CompileWarning => 40,
        DiagnosticCode::FilterCutoffClamped => 41,
        DiagnosticCode::EventOrderInvalid => 42,
        DiagnosticCode::MidiError => 43,
        DiagnosticCode::AudioDeviceError => 44,
        DiagnosticCode::UnsupportedPlaybackCombination => 45,
        DiagnosticCode::InvalidStretchRatio => 46,
        DiagnosticCode::InvalidSourceTempo => 47,
        DiagnosticCode::StretchBackendFailure => 48,
        DiagnosticCode::InvalidGrainRegion => 49,
        DiagnosticCode::InvalidGrainParameter => 50,
        DiagnosticCode::InvalidSequence => 51,
        DiagnosticCode::InvalidStepDuration => 52,
        DiagnosticCode::TraceLimitExceeded => 53,
    }
}

fn severity_id(severity: DiagnosticSeverity) -> u32 {
    match severity {
        DiagnosticSeverity::Error => 0,
        DiagnosticSeverity::Warning => 1,
        DiagnosticSeverity::Info => 2,
    }
}

fn view(value: Option<&str>) -> SonalloyStringView {
    let Some(value) = value else {
        return SonalloyStringView {
            data: ptr::null(),
            length: 0,
        };
    };
    SonalloyStringView {
        data: value.as_ptr().cast(),
        length: value.len(),
    }
}

/// Return the number of diagnostics in a handle.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_diagnostics_count(diagnostics: *const SonalloyDiagnostics) -> u32 {
    let Some(diagnostics) = (!diagnostics.is_null()).then(|| unsafe { &*diagnostics }) else {
        return 0;
    };
    u32::try_from(diagnostics.entries.len()).unwrap_or(u32::MAX)
}

/// Copy one diagnostic view from a diagnostics handle.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_diagnostics_get(
    diagnostics: *const SonalloyDiagnostics,
    index: u32,
    out_diagnostic: *mut SonalloyDiagnosticView,
) -> SonalloyResult {
    guard(|| {
        if diagnostics.is_null() || out_diagnostic.is_null() {
            return SonalloyResult::InvalidArgument;
        }
        let diagnostics = unsafe { &*diagnostics };
        let Some(diagnostic) = diagnostics.entries.get(index as usize) else {
            return SonalloyResult::InvalidArgument;
        };
        unsafe {
            *out_diagnostic = SonalloyDiagnosticView {
                code: code_id(diagnostic.code),
                severity: severity_id(diagnostic.severity),
                path: view(diagnostic.path.as_deref()),
                message: view(Some(&diagnostic.message)),
                detail: view(diagnostic.detail.as_deref()),
            };
        }
        SonalloyResult::Ok
    })
}

/// Destroy a diagnostics handle. Null is a no-op.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_diagnostics_destroy(diagnostics: *mut SonalloyDiagnostics) {
    let _ = guard(|| {
        if !diagnostics.is_null() {
            drop(unsafe { Box::from_raw(diagnostics) });
        }
        SonalloyResult::Ok
    });
}
