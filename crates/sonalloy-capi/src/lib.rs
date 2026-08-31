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

use std::cell::UnsafeCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU8, Ordering};

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

#[cfg(test)]
mod test_hooks {
    use std::sync::atomic::{AtomicBool, Ordering};

    static PANIC_ON_NEXT_EXTERN_CALL: AtomicBool = AtomicBool::new(false);

    pub(crate) fn panic_on_next_extern_call() {
        PANIC_ON_NEXT_EXTERN_CALL.store(true, Ordering::Release);
    }

    pub(crate) fn panic_if_requested() {
        assert!(
            !PANIC_ON_NEXT_EXTERN_CALL.swap(false, Ordering::Acquire),
            "test panic from public extern entry"
        );
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
    pub(crate) reclaimable_slots: [SonalloyReclaimable; MAX_RECLAIMABLE_SLOTS],
    pub(crate) max_block_size: usize,
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
    state: AtomicU8,
    inner: UnsafeCell<Option<sonalloy_core::ReclaimableRuntimeResource>>,
}

const RECLAIMABLE_FREE: u8 = 0;
const RECLAIMABLE_AUDIO_WRITING: u8 = 1;
const RECLAIMABLE_CONTROL_OWNED: u8 = 2;
const RECLAIMABLE_CONTROL_DROPPING: u8 = 3;
const MAX_RECLAIMABLE_SLOTS: usize = 16;

impl SonalloyReclaimable {
    pub(crate) const fn new() -> Self {
        Self {
            state: AtomicU8::new(RECLAIMABLE_FREE),
            inner: UnsafeCell::new(None),
        }
    }

    pub(crate) fn try_claim_for_audio(&self) -> bool {
        self.state
            .compare_exchange(
                RECLAIMABLE_FREE,
                RECLAIMABLE_AUDIO_WRITING,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    pub(crate) fn publish_from_audio(&self, resource: sonalloy_core::ReclaimableRuntimeResource) {
        unsafe {
            *self.inner.get() = Some(resource);
        }
        self.state
            .store(RECLAIMABLE_CONTROL_OWNED, Ordering::Release);
    }

    pub(crate) fn release_audio_claim(&self) {
        debug_assert_eq!(
            self.state.load(Ordering::Relaxed),
            RECLAIMABLE_AUDIO_WRITING
        );
        self.state.store(RECLAIMABLE_FREE, Ordering::Release);
    }

    pub(crate) fn destroy_from_control(&self) {
        if self
            .state
            .compare_exchange(
                RECLAIMABLE_CONTROL_OWNED,
                RECLAIMABLE_CONTROL_DROPPING,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_err()
        {
            return;
        }
        unsafe {
            let _ = (*self.inner.get()).take();
        }
        self.state.store(RECLAIMABLE_FREE, Ordering::Release);
    }
}

// SAFETY: The AtomicU8 state gives exclusive access to `inner`: audio writes only while holding
// AUDIO_WRITING, and control drops only after acquiring CONTROL_OWNED.
unsafe impl Send for SonalloyReclaimable {}
// SAFETY: Shared references only access `inner` after the same atomic ownership handoff.
unsafe impl Sync for SonalloyReclaimable {}

#[cfg(test)]
mod tests {
    use std::panic::catch_unwind;
    use std::ptr;

    use super::runtime::{sonalloy_runtime_activate, sonalloy_runtime_prepare};
    use super::{SonalloyProcessSpec, SonalloyReclaimable, SonalloyResult, test_hooks};

    #[test]
    fn panic_is_contained_by_public_guard_extern_entry() {
        test_hooks::panic_on_next_extern_call();

        let result = catch_unwind(|| sonalloy_runtime_activate(ptr::null_mut()));

        assert!(matches!(result, Ok(SonalloyResult::InternalPanic)));
    }

    #[test]
    fn panic_is_contained_by_public_guard_result_extern_entry() {
        test_hooks::panic_on_next_extern_call();

        let result = catch_unwind(|| {
            sonalloy_runtime_prepare(
                ptr::null_mut(),
                SonalloyProcessSpec {
                    sample_rate: 48_000.0,
                    max_block_size: 64,
                    input_channels: 0,
                    output_channels: 2,
                },
            )
        });

        assert!(matches!(result, Ok(SonalloyResult::InternalPanic)));
    }

    #[test]
    fn reclaimable_slot_claim_is_exclusive() {
        use std::sync::Arc;
        use std::thread;

        let slot = Arc::new(SonalloyReclaimable::new());
        thread::scope(|scope| {
            let threads: Vec<_> = (0..8)
                .map(|_| {
                    let slot = Arc::clone(&slot);
                    scope.spawn(move || slot.try_claim_for_audio())
                })
                .collect();
            let claims = threads
                .into_iter()
                .map(|thread| thread.join().expect("claim thread completed"))
                .filter(|claimed| *claimed)
                .count();
            assert_eq!(claims, 1);
        });
        slot.release_audio_claim();
        assert!(slot.try_claim_for_audio());
        slot.release_audio_claim();
    }

    #[test]
    fn reclaimable_destroy_cannot_take_an_audio_owned_slot() {
        let slot = SonalloyReclaimable::new();
        assert!(slot.try_claim_for_audio());
        slot.destroy_from_control();
        assert!(!slot.try_claim_for_audio());
        slot.release_audio_claim();
        assert!(slot.try_claim_for_audio());
        slot.release_audio_claim();
    }
}
