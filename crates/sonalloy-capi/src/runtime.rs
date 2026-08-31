#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::ptr;
use std::slice;
use std::sync::Arc;

use sonalloy_core::{InstrumentProcessor, ProcessBlock, ProcessError, ProcessSpec, RuntimeState};

use crate::types::{
    MAX_EVENTS_PER_BLOCK, SonalloyEvent, SonalloyProcessContext, SonalloyProcessSpec,
    SonalloyPublishOutcome, SonalloyResult, SonalloyRuntimeErrorInfo, runtime_error_info,
};
use crate::{
    SonalloyCompiledInstrument, SonalloyPreparedUpdate, SonalloyReclaimable, SonalloyRuntime,
    guard, guard_result,
};

fn process_error(runtime: &mut SonalloyRuntime, error: &ProcessError) -> SonalloyResult {
    runtime.last_error = runtime_error_info(error);
    match error {
        ProcessError::NotPrepared | ProcessError::NotActive => SonalloyResult::InvalidState,
        _ => SonalloyResult::ProcessFailed,
    }
}

fn prepare_spec(spec: SonalloyProcessSpec) -> Result<ProcessSpec, SonalloyResult> {
    spec.to_core()
}

fn validate_channel_pointer_array<T>(
    channels: *const *const T,
    count: usize,
) -> Result<(), SonalloyResult> {
    if count != 0 && channels.is_null() {
        Err(SonalloyResult::InvalidArgument)
    } else {
        Ok(())
    }
}

fn input_buffers<'a>(
    channels: *const *const f32,
    count: usize,
    frames: usize,
) -> Result<[&'a [f32]; 2], SonalloyResult> {
    if count > 2 {
        return Err(SonalloyResult::InvalidArgument);
    }
    validate_channel_pointer_array(channels, count)?;
    let pointers = if count == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(channels, count) }
    };
    let mut result = [&[][..], &[][..]];
    for (index, pointer) in pointers.iter().copied().enumerate() {
        if frames == 0 && pointer.is_null() {
            continue;
        }
        if pointer.is_null() {
            return Err(SonalloyResult::InvalidArgument);
        }
        result[index] = unsafe { slice::from_raw_parts(pointer, frames) };
    }
    Ok(result)
}

fn output_buffers<'a>(
    channels: *mut *mut f32,
    count: usize,
    frames: usize,
) -> Result<[&'a mut [f32]; 2], SonalloyResult> {
    if count != 2 {
        return Err(SonalloyResult::InvalidArgument);
    }
    if channels.is_null() {
        return Err(SonalloyResult::InvalidArgument);
    }
    let pointers = unsafe { slice::from_raw_parts(channels, count) };
    let mut result: [&mut [f32]; 2] = [&mut [], &mut []];
    for (index, pointer) in pointers.iter().copied().enumerate() {
        if frames == 0 && pointer.is_null() {
            continue;
        }
        if pointer.is_null() {
            return Err(SonalloyResult::InvalidArgument);
        }
        result[index] = unsafe { slice::from_raw_parts_mut(pointer, frames) };
    }
    Ok(result)
}

/// Create a runtime from a compiled instrument.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_runtime_create(
    compiled: *const SonalloyCompiledInstrument,
    out_runtime: *mut *mut SonalloyRuntime,
) -> SonalloyResult {
    guard(|| {
        if compiled.is_null() || out_runtime.is_null() {
            return SonalloyResult::InvalidArgument;
        }
        unsafe {
            *out_runtime = ptr::null_mut();
        }
        let compiled = unsafe { &*compiled };
        let mut runtime = Box::new(SonalloyRuntime {
            inner: sonalloy_core::InstrumentRuntime::new(Arc::clone(&compiled.inner)),
            event_scratch: Vec::with_capacity(MAX_EVENTS_PER_BLOCK),
            last_error: SonalloyRuntimeErrorInfo {
                code: 0,
                detail_kind: 0,
                value_a: 0,
                value_b: 0,
            },
            reclaimable_slots: Vec::with_capacity(16),
        });
        for _ in 0..16 {
            runtime
                .reclaimable_slots
                .push(SonalloyReclaimable { inner: None });
        }
        unsafe {
            *out_runtime = Box::into_raw(runtime);
        }
        SonalloyResult::Ok
    })
}

/// Prepare all runtime resources for a process specification.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_runtime_prepare(
    runtime: *mut SonalloyRuntime,
    spec: SonalloyProcessSpec,
) -> SonalloyResult {
    guard_result(|| {
        if runtime.is_null() {
            return Err(SonalloyResult::InvalidArgument);
        }
        let runtime = unsafe { &mut *runtime };
        let spec = prepare_spec(spec)?;
        runtime.inner.prepare(spec).map_or_else(
            |error| {
                runtime.last_error = runtime_error_info(&error);
                Err(match error {
                    ProcessError::NotActive => SonalloyResult::InvalidState,
                    _ => SonalloyResult::PrepareFailed,
                })
            },
            |()| Ok(()),
        )
    })
}

/// Activate a prepared runtime.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_runtime_activate(runtime: *mut SonalloyRuntime) -> SonalloyResult {
    guard(|| {
        if runtime.is_null() {
            return SonalloyResult::InvalidArgument;
        }
        let runtime = unsafe { &mut *runtime };
        runtime
            .inner
            .activate()
            .map_or(SonalloyResult::InvalidState, |()| SonalloyResult::Ok)
    })
}

/// Reset a prepared or active runtime to its initial state.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_runtime_reset(runtime: *mut SonalloyRuntime) -> SonalloyResult {
    guard(|| {
        if runtime.is_null() {
            return SonalloyResult::InvalidArgument;
        }
        let runtime = unsafe { &mut *runtime };
        runtime
            .inner
            .reset()
            .map_or(SonalloyResult::InvalidState, |()| SonalloyResult::Ok)
    })
}

/// Deactivate a runtime while retaining prepared resources.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_runtime_deactivate(runtime: *mut SonalloyRuntime) -> SonalloyResult {
    guard(|| {
        if runtime.is_null() {
            return SonalloyResult::InvalidArgument;
        }
        let runtime = unsafe { &mut *runtime };
        runtime
            .inner
            .deactivate()
            .map_or(SonalloyResult::InvalidState, |()| SonalloyResult::Ok)
    })
}

/// Process one planar stereo block.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_runtime_process(
    runtime: *mut SonalloyRuntime,
    context: *const SonalloyProcessContext,
    events: *const SonalloyEvent,
    event_count: u32,
    input_channels: *const *const f32,
    input_channel_count: u32,
    output_channels: *mut *mut f32,
    output_channel_count: u32,
    frames: u32,
) -> SonalloyResult {
    guard_result(|| {
        if runtime.is_null() || context.is_null() {
            return Err(SonalloyResult::InvalidArgument);
        }
        let runtime = unsafe { &mut *runtime };
        let event_count =
            usize::try_from(event_count).map_err(|_| SonalloyResult::InvalidArgument)?;
        if event_count > MAX_EVENTS_PER_BLOCK {
            return Err(SonalloyResult::InvalidArgument);
        }
        if event_count != 0 && events.is_null() {
            return Err(SonalloyResult::InvalidArgument);
        }
        let input_channel_count =
            usize::try_from(input_channel_count).map_err(|_| SonalloyResult::InvalidArgument)?;
        let output_channel_count =
            usize::try_from(output_channel_count).map_err(|_| SonalloyResult::InvalidArgument)?;
        let frames = usize::try_from(frames).map_err(|_| SonalloyResult::InvalidArgument)?;
        let core_context = unsafe { *context }.to_core()?;
        let input = input_buffers(input_channels, input_channel_count, frames)?;
        let mut output = output_buffers(output_channels, output_channel_count, frames)?;
        runtime.event_scratch.clear();
        if event_count != 0 {
            let source = unsafe { slice::from_raw_parts(events, event_count) };
            for event in source {
                runtime.event_scratch.push(event.to_core()?);
            }
        }
        let process_result = runtime.inner.process(ProcessBlock {
            frames,
            context: core_context,
            events: &runtime.event_scratch,
            input: &input[..input_channel_count.min(2)],
            output: &mut output[..output_channel_count.min(2)],
        });
        match process_result {
            Ok(()) => Ok(()),
            Err(error) => Err(process_error(runtime, &error)),
        }
    })
}

/// Prepare an update without changing the active runtime.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_update_prepare(
    compiled: *const SonalloyCompiledInstrument,
    spec: SonalloyProcessSpec,
    out_update: *mut *mut SonalloyPreparedUpdate,
) -> SonalloyResult {
    guard_result(|| {
        if compiled.is_null() || out_update.is_null() {
            return Err(SonalloyResult::InvalidArgument);
        }
        unsafe {
            *out_update = ptr::null_mut();
        }
        let spec = prepare_spec(spec)?;
        let compiled = unsafe { &*compiled };
        let update =
            sonalloy_core::InstrumentRuntime::prepare_update(Arc::clone(&compiled.inner), spec)
                .map_err(|_| SonalloyResult::PrepareFailed)?;
        unsafe {
            *out_update = Box::into_raw(Box::new(SonalloyPreparedUpdate { inner: update }));
        }
        Ok(())
    })
}

/// Publish a prepared update at a process-block boundary.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_runtime_publish(
    runtime: *mut SonalloyRuntime,
    update: *mut SonalloyPreparedUpdate,
    out_outcome: *mut SonalloyPublishOutcome,
) -> SonalloyResult {
    guard_result(|| {
        if runtime.is_null() || update.is_null() || out_outcome.is_null() {
            return Err(SonalloyResult::InvalidArgument);
        }
        let runtime = unsafe { &mut *runtime };
        let update = unsafe { &mut *update };
        let outcome =
            runtime
                .inner
                .publish_prepared(&mut update.inner)
                .map_err(|error| match error {
                    sonalloy_core::PublishError::RequiresReactivation { .. }
                    | sonalloy_core::PublishError::ProcessSpecMismatch => {
                        SonalloyResult::UpdateIncompatible
                    }
                    sonalloy_core::PublishError::CapacityExceeded
                    | sonalloy_core::PublishError::ReclaimCapacityExceeded => {
                        SonalloyResult::UpdateCapacityExceeded
                    }
                    sonalloy_core::PublishError::TransitionBusy => SonalloyResult::TransitionBusy,
                    _ => SonalloyResult::InvalidState,
                })?;
        unsafe {
            *out_outcome = SonalloyPublishOutcome {
                generation_id: outcome.generation_id.get(),
                parameter_catalog_revision: outcome.parameter_catalog_revision,
                reported_latency_frames: u32::try_from(outcome.reported_latency_frames)
                    .unwrap_or(u32::MAX),
                required_input_channels: u32::try_from(outcome.required_input_channels)
                    .unwrap_or(u32::MAX),
            };
        }
        Ok(())
    })
}

/// Move one deferred resource into a caller-owned opaque handle.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_runtime_take_reclaimable(
    runtime: *mut SonalloyRuntime,
    out_reclaimable: *mut *mut SonalloyReclaimable,
) -> SonalloyResult {
    guard(|| {
        if runtime.is_null() || out_reclaimable.is_null() {
            return SonalloyResult::InvalidArgument;
        }
        unsafe {
            *out_reclaimable = ptr::null_mut();
        }
        let runtime = unsafe { &mut *runtime };
        let Some(slot) = runtime
            .reclaimable_slots
            .iter_mut()
            .find(|slot| slot.inner.is_none())
        else {
            return SonalloyResult::Ok;
        };
        let Some(resource) = runtime.inner.take_reclaimable() else {
            return SonalloyResult::Ok;
        };
        slot.inner = Some(resource);
        unsafe {
            *out_reclaimable = ptr::from_mut(slot);
        }
        SonalloyResult::Ok
    })
}

/// Destroy a deferred resource on the control thread. Null is a no-op.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_reclaimable_destroy(reclaimable: *mut SonalloyReclaimable) {
    let _ = guard(|| {
        if reclaimable.is_null() {
            return SonalloyResult::Ok;
        }
        let reclaimable = unsafe { &mut *reclaimable };
        let _ = reclaimable.inner.take();
        SonalloyResult::Ok
    });
}

/// Return the current runtime state as a stable numeric value.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_runtime_state(runtime: *const SonalloyRuntime) -> u32 {
    if runtime.is_null() {
        return u32::MAX;
    }
    match unsafe { (*runtime).inner.state() } {
        RuntimeState::Unprepared => 0,
        RuntimeState::Prepared => 1,
        RuntimeState::Active => 2,
        RuntimeState::Faulted => 3,
    }
}

/// Return the active generation identity, or zero while unprepared.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_runtime_generation_id(runtime: *const SonalloyRuntime) -> u64 {
    if runtime.is_null() {
        return 0;
    }
    unsafe { (*runtime).inner.generation_id().get() }
}

/// Return the number of stale parameter events ignored by the runtime.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_runtime_stale_parameter_event_count(
    runtime: *const SonalloyRuntime,
) -> u64 {
    if runtime.is_null() {
        return 0;
    }
    unsafe { (*runtime).inner.stale_parameter_event_count() }
}

/// Copy fixed-size information about the last process failure.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_runtime_last_error(
    runtime: *const SonalloyRuntime,
    out_error: *mut SonalloyRuntimeErrorInfo,
) -> SonalloyResult {
    guard(|| {
        if runtime.is_null() || out_error.is_null() {
            return SonalloyResult::InvalidArgument;
        }
        unsafe {
            *out_error = (*runtime).last_error;
        }
        SonalloyResult::Ok
    })
}

/// Destroy a runtime handle. Null is a no-op.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_runtime_destroy(runtime: *mut SonalloyRuntime) {
    let _ = guard(|| {
        if !runtime.is_null() {
            drop(unsafe { Box::from_raw(runtime) });
        }
        SonalloyResult::Ok
    });
}

/// Destroy a prepared update handle. Null is a no-op.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_update_destroy(update: *mut SonalloyPreparedUpdate) {
    let _ = guard(|| {
        if !update.is_null() {
            drop(unsafe { Box::from_raw(update) });
        }
        SonalloyResult::Ok
    });
}
