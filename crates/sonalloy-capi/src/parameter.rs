#![allow(clippy::not_unsafe_ptr_arg_deref)]

use sonalloy_core::{ParameterOwner, ParameterScale, ParameterUnit, VectorAxis};

use crate::types::{SonalloyParameterDescriptor, SonalloyResult, SonalloyStringView};
use crate::{SonalloyCompiledInstrument, guard, guard_result};

fn owner_fields(owner: ParameterOwner) -> (u32, u32, u32, u32) {
    match owner {
        ParameterOwner::Layer { definition_index } => (1, index_field(definition_index), 0, 0),
        ParameterOwner::LayerGenerator { definition_index } => {
            (2, index_field(definition_index), 0, 0)
        }
        ParameterOwner::LayerProcessor {
            definition_index,
            processor_index,
        } => (
            3,
            index_field(definition_index),
            index_field(processor_index),
            0,
        ),
        ParameterOwner::VoiceProcessor { processor_index } => {
            (4, index_field(processor_index), 0, 0)
        }
        ParameterOwner::GlobalProcessor { processor_index } => {
            (5, index_field(processor_index), 0, 0)
        }
        ParameterOwner::Macro { macro_index } => (6, index_field(macro_index), 0, 0),
        ParameterOwner::VectorAxis { vector_index, axis } => {
            let axis = match axis {
                VectorAxis::Position => 1,
                VectorAxis::X => 2,
                VectorAxis::Y => 3,
            };
            (7, index_field(vector_index), 0, axis)
        }
    }
}

fn index_field(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn unit_id(unit: ParameterUnit) -> u32 {
    match unit {
        ParameterUnit::Decibels => 1,
        ParameterUnit::Pan => 2,
        ParameterUnit::Cents => 3,
        ParameterUnit::Hertz => 4,
        ParameterUnit::Ratio => 5,
        ParameterUnit::Seconds => 6,
        ParameterUnit::PerSecond => 7,
        ParameterUnit::Index => 8,
        ParameterUnit::DecibelsPerOctave => 9,
        ParameterUnit::Normalized => 10,
    }
}

fn scale_id(scale: ParameterScale) -> u32 {
    match scale {
        ParameterScale::Linear => 0,
        ParameterScale::Log2 => 1,
    }
}

fn parameter_handle(
    compiled: *const SonalloyCompiledInstrument,
    value: u32,
) -> Result<sonalloy_core::ParameterHandle, SonalloyResult> {
    if compiled.is_null() {
        return Err(SonalloyResult::InvalidArgument);
    }
    Ok(sonalloy_core::ParameterHandle::from_index(value as usize))
}

/// Copy one parameter descriptor from a compiled catalog.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_compiled_parameter_descriptor(
    compiled: *const SonalloyCompiledInstrument,
    index: u32,
    out_descriptor: *mut SonalloyParameterDescriptor,
) -> SonalloyResult {
    guard(|| {
        if compiled.is_null() || out_descriptor.is_null() {
            return SonalloyResult::InvalidArgument;
        }
        let compiled = unsafe { &*compiled };
        let Some(parameter) = compiled.inner.parameters().get(index as usize) else {
            return SonalloyResult::InvalidArgument;
        };
        let (owner_kind, owner_index, owner_sub_index, owner_axis) = owner_fields(parameter.owner);
        unsafe {
            *out_descriptor = SonalloyParameterDescriptor {
                id: SonalloyStringView {
                    data: parameter.id.as_ptr().cast(),
                    length: parameter.id.len(),
                },
                owner_kind,
                owner_index,
                owner_sub_index,
                owner_axis,
                unit: unit_id(parameter.unit),
                scale: scale_id(parameter.scale),
                min: parameter.min,
                max: parameter.max,
                default: parameter.default,
                smoothing_seconds: parameter.smoothing_seconds,
            };
        }
        SonalloyResult::Ok
    })
}

/// Resolve a canonical parameter identifier to a dense handle.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_compiled_parameter_handle(
    compiled: *const SonalloyCompiledInstrument,
    parameter_id: SonalloyStringView,
    out_handle: *mut u32,
) -> SonalloyResult {
    guard_result(|| {
        if compiled.is_null() || out_handle.is_null() {
            return Err(SonalloyResult::InvalidArgument);
        }
        let parameter_id = parameter_id.to_owned()?;
        let compiled = unsafe { &*compiled };
        let Some(handle) = compiled.inner.parameter_handle(&parameter_id) else {
            return Err(SonalloyResult::InvalidArgument);
        };
        unsafe {
            *out_handle = u32::try_from(handle.index()).unwrap_or(u32::MAX);
        }
        Ok(())
    })
}

/// Convert a native parameter value to the normalized control range.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_compiled_parameter_normalize(
    compiled: *const SonalloyCompiledInstrument,
    handle: u32,
    native_value: f32,
    out_normalized: *mut f32,
) -> SonalloyResult {
    guard_result(|| {
        if out_normalized.is_null() {
            return Err(SonalloyResult::InvalidArgument);
        }
        let handle = parameter_handle(compiled, handle)?;
        let descriptor = unsafe { &*compiled }
            .inner
            .parameter_descriptor(handle)
            .ok_or(SonalloyResult::InvalidArgument)?;
        let normalized = descriptor
            .normalize(native_value)
            .map_err(|_| SonalloyResult::InvalidArgument)?;
        unsafe {
            *out_normalized = normalized;
        }
        Ok(())
    })
}

/// Convert a normalized parameter value to native units.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_compiled_parameter_denormalize(
    compiled: *const SonalloyCompiledInstrument,
    handle: u32,
    normalized: f32,
    out_native_value: *mut f32,
) -> SonalloyResult {
    guard_result(|| {
        if out_native_value.is_null() {
            return Err(SonalloyResult::InvalidArgument);
        }
        let handle = parameter_handle(compiled, handle)?;
        let descriptor = unsafe { &*compiled }
            .inner
            .parameter_descriptor(handle)
            .ok_or(SonalloyResult::InvalidArgument)?;
        let native = descriptor
            .denormalize(normalized)
            .map_err(|_| SonalloyResult::InvalidArgument)?;
        unsafe {
            *out_native_value = native;
        }
        Ok(())
    })
}
