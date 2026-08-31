#![doc = "Public C ABI for the Sonalloy runtime."]

mod capability;
mod compile;
mod diagnostics;
mod parameter;
mod runtime;
mod types;

pub use capability::{sonalloy_c_api_version, sonalloy_has_capability};
pub use compile::{
    sonalloy_compile_json, sonalloy_compiled_destroy, sonalloy_compiled_parameter_catalog_revision,
    sonalloy_compiled_parameter_count, sonalloy_compiled_reported_latency_frames,
    sonalloy_compiled_required_input_channels,
};
pub use diagnostics::{
    sonalloy_diagnostics_count, sonalloy_diagnostics_destroy, sonalloy_diagnostics_get,
};
pub use parameter::{
    sonalloy_compiled_parameter_denormalize, sonalloy_compiled_parameter_descriptor,
    sonalloy_compiled_parameter_handle, sonalloy_compiled_parameter_normalize,
};
pub use runtime::{
    sonalloy_reclaimable_destroy, sonalloy_runtime_activate, sonalloy_runtime_create,
    sonalloy_runtime_deactivate, sonalloy_runtime_destroy, sonalloy_runtime_generation_id,
    sonalloy_runtime_last_error, sonalloy_runtime_prepare, sonalloy_runtime_process,
    sonalloy_runtime_publish, sonalloy_runtime_reset, sonalloy_runtime_stale_parameter_event_count,
    sonalloy_runtime_state, sonalloy_runtime_take_reclaimable, sonalloy_update_destroy,
    sonalloy_update_prepare,
};

pub use types::{
    SonalloyDiagnosticView, SonalloyEvent, SonalloyEventType, SonalloyParameterDescriptor,
    SonalloyProcessContext, SonalloyProcessSpec, SonalloyPublishOutcome, SonalloyResult,
    SonalloyRuntimeErrorInfo, SonalloyStringView, SonalloyTransportState,
};

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::types::SONALLOY_INTERNAL_PANIC;

pub(crate) fn guard(function: impl FnOnce() -> SonalloyResult) -> SonalloyResult {
    catch_unwind(AssertUnwindSafe(function)).unwrap_or(SONALLOY_INTERNAL_PANIC)
}

pub(crate) fn guard_result(
    function: impl FnOnce() -> Result<(), SonalloyResult>,
) -> SonalloyResult {
    match catch_unwind(AssertUnwindSafe(function)) {
        Ok(Ok(())) => SonalloyResult::Ok,
        Ok(Err(error)) => error,
        Err(_) => SONALLOY_INTERNAL_PANIC,
    }
}

/// Opaque compiled instrument handle.
#[repr(C)]
pub struct SonalloyCompiledInstrument {
    pub(crate) inner: std::sync::Arc<sonalloy_core::CompiledInstrument>,
}

/// Opaque runtime handle.
#[repr(C)]
pub struct SonalloyRuntime {
    pub(crate) inner: sonalloy_core::InstrumentRuntime,
    pub(crate) event_scratch: Vec<sonalloy_core::ProcessEvent>,
    pub(crate) last_error: SonalloyRuntimeErrorInfo,
    pub(crate) reclaimable_slots: Vec<SonalloyReclaimable>,
}

/// Opaque prepared update handle.
#[repr(C)]
pub struct SonalloyPreparedUpdate {
    pub(crate) inner: sonalloy_core::PreparedInstrumentUpdate,
}

/// Opaque diagnostics handle.
#[repr(C)]
pub struct SonalloyDiagnostics {
    pub(crate) entries: Vec<sonalloy_core::Diagnostic>,
}

/// Opaque deferred-resource handle.
#[repr(C)]
pub struct SonalloyReclaimable {
    pub(crate) inner: Option<sonalloy_core::ReclaimableRuntimeResource>,
}

#[cfg(test)]
mod tests {
    use super::{SonalloyResult, guard, guard_result};

    #[test]
    fn panic_is_contained_at_both_guard_entry_points() {
        assert_eq!(
            guard(|| panic!("test panic")),
            SonalloyResult::InternalPanic
        );
        assert_eq!(
            guard_result(|| panic!("test panic")),
            SonalloyResult::InternalPanic
        );
    }
}
