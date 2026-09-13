#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::path::PathBuf;
use std::ptr;
use std::sync::Arc;

use sonalloy_core::{
    CompileContext, Diagnostic, DiagnosticCode, DiagnosticSeverity, InstrumentDefinition,
    compile_instrument,
};

use crate::diagnostics::boxed;
use crate::types::{
    SonalloyDefinitionInfo, SonalloyProcessSpec, SonalloyResult, SonalloyStringView,
};
use crate::{SonalloyCompiledInstrument, SonalloyDiagnostics, guard};

/// Return the public compile result and diagnostics for a JSON Definition.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_compile_json(
    definition_json: SonalloyStringView,
    definition_base_dir: SonalloyStringView,
    process_spec: SonalloyProcessSpec,
    out_compiled: *mut *mut SonalloyCompiledInstrument,
    out_diagnostics: *mut *mut SonalloyDiagnostics,
) -> SonalloyResult {
    guard(|| {
        if out_compiled.is_null() || out_diagnostics.is_null() {
            return SonalloyResult::InvalidArgument;
        }
        unsafe {
            *out_compiled = ptr::null_mut();
            *out_diagnostics = ptr::null_mut();
        }
        let json = match definition_json.to_owned() {
            Ok(value) => value,
            Err(error) => return error,
        };
        let base_dir = match definition_base_dir.to_owned() {
            Ok(value) => value,
            Err(error) => return error,
        };
        let process_spec = match process_spec.to_core() {
            Ok(value) => value,
            Err(error) => return error,
        };
        let definition = match serde_json::from_str::<InstrumentDefinition>(&json) {
            Ok(definition) => definition,
            Err(error) => {
                unsafe {
                    *out_diagnostics = boxed(vec![
                        Diagnostic::error(
                            DiagnosticCode::JsonInvalid,
                            "definition JSON is invalid",
                        )
                        .with_detail(error.to_string()),
                    ]);
                }
                return SonalloyResult::CompileFailed;
            }
        };
        let result = compile_instrument(
            &definition,
            &CompileContext {
                definition_base_dir: if base_dir.is_empty() {
                    PathBuf::from(".")
                } else {
                    PathBuf::from(base_dir)
                },
                process_spec,
            },
        );
        unsafe {
            *out_diagnostics = boxed(result.diagnostics);
        }
        let Some(instrument) = result.instrument else {
            return SonalloyResult::CompileFailed;
        };
        unsafe {
            *out_compiled = Box::into_raw(Box::new(SonalloyCompiledInstrument {
                inner: Arc::clone(&instrument),
            }));
        }
        SonalloyResult::Ok
    })
}

/// Inspect a Definition and return its execution conditions without resolving assets.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_inspect_json(
    definition_json: SonalloyStringView,
    definition_base_dir: SonalloyStringView,
    out_info: *mut SonalloyDefinitionInfo,
    out_diagnostics: *mut *mut SonalloyDiagnostics,
) -> SonalloyResult {
    guard(|| {
        if out_info.is_null() || out_diagnostics.is_null() {
            return SonalloyResult::InvalidArgument;
        }
        unsafe {
            *out_info = SonalloyDefinitionInfo {
                required_input_channels: 0,
            };
            *out_diagnostics = ptr::null_mut();
        }
        let json = match definition_json.to_owned() {
            Ok(value) => value,
            Err(error) => return error,
        };
        if let Err(error) = definition_base_dir.to_owned() {
            return error;
        }
        let definition = match serde_json::from_str::<InstrumentDefinition>(&json) {
            Ok(definition) => definition,
            Err(error) => {
                unsafe {
                    *out_diagnostics = boxed(vec![
                        Diagnostic::error(
                            DiagnosticCode::JsonInvalid,
                            "definition JSON is invalid",
                        )
                        .with_detail(error.to_string()),
                    ]);
                }
                return SonalloyResult::CompileFailed;
            }
        };
        let diagnostics = definition.validate();
        let has_errors = diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error);
        unsafe {
            *out_info = SonalloyDefinitionInfo {
                required_input_channels: u32::try_from(definition.required_input_channels())
                    .unwrap_or(u32::MAX),
            };
            *out_diagnostics = boxed(diagnostics);
        }
        if has_errors {
            SonalloyResult::CompileFailed
        } else {
            SonalloyResult::Ok
        }
    })
}

/// Return the fixed latency reported by a compiled instrument.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_compiled_reported_latency_frames(
    compiled: *const SonalloyCompiledInstrument,
) -> u32 {
    if compiled.is_null() {
        return 0;
    }
    u32::try_from(unsafe { (*compiled).inner.reported_latency_frames() }).unwrap_or(u32::MAX)
}

/// Return the required external input channel count.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_compiled_required_input_channels(
    compiled: *const SonalloyCompiledInstrument,
) -> u32 {
    if compiled.is_null() {
        return 0;
    }
    u32::try_from(unsafe { (*compiled).inner.required_input_channels() }).unwrap_or(u32::MAX)
}

/// Return the parameter catalog revision.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_compiled_parameter_catalog_revision(
    compiled: *const SonalloyCompiledInstrument,
) -> u64 {
    if compiled.is_null() {
        return 0;
    }
    unsafe { (*compiled).inner.parameter_catalog_revision() }
}

/// Return the number of parameters in the catalog.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_compiled_parameter_count(
    compiled: *const SonalloyCompiledInstrument,
) -> u32 {
    if compiled.is_null() {
        return 0;
    }
    u32::try_from(unsafe { (*compiled).inner.parameters().len() }).unwrap_or(u32::MAX)
}

/// Destroy a compiled instrument handle. Null is a no-op.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_compiled_destroy(compiled: *mut SonalloyCompiledInstrument) {
    let _ = guard(|| {
        if !compiled.is_null() {
            drop(unsafe { Box::from_raw(compiled) });
        }
        SonalloyResult::Ok
    });
}
