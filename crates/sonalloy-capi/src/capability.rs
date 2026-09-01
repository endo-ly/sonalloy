#![allow(clippy::not_unsafe_ptr_arg_deref)]

use crate::guard;
use crate::types::SonalloyResult;

/// Public C ABI version.
pub const SONALLOY_C_API_VERSION: u32 = 1;

/// Capability identifiers used by `sonalloy_has_capability`.
pub mod capability_id {
    /// Prepared runtime updates are supported.
    pub const REALTIME_RUNTIME_UPDATE: u32 = 1;
    /// External audio input is supported.
    pub const EXTERNAL_AUDIO_INPUT: u32 = 2;
    /// Transport context is supported.
    pub const TRANSPORT_CONTEXT: u32 = 3;
    /// Parameter catalog revisions are supported.
    pub const PARAMETER_CATALOG_REVISION: u32 = 4;
    /// Per-note expression is not supported by this contract.
    pub const NOTE_EXPRESSION: u32 = 5;
    /// Runtime state serialization is not supported by this contract.
    pub const STATE_SERIALIZATION: u32 = 6;
    /// Neural backends are not supported by this contract.
    pub const NEURAL_BACKEND: u32 = 7;
}

/// Return the public C ABI version.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_c_api_version() -> u32 {
    SONALLOY_C_API_VERSION
}

/// Query whether one public runtime capability is supported.
#[unsafe(no_mangle)]
pub extern "C" fn sonalloy_has_capability(
    capability: u32,
    out_supported: *mut u8,
) -> SonalloyResult {
    guard(|| {
        if out_supported.is_null() {
            return SonalloyResult::InvalidArgument;
        }
        let supported = match capability {
            capability_id::REALTIME_RUNTIME_UPDATE
            | capability_id::EXTERNAL_AUDIO_INPUT
            | capability_id::TRANSPORT_CONTEXT
            | capability_id::PARAMETER_CATALOG_REVISION => 1,
            capability_id::NOTE_EXPRESSION
            | capability_id::STATE_SERIALIZATION
            | capability_id::NEURAL_BACKEND => 0,
            _ => return SonalloyResult::InvalidArgument,
        };
        unsafe {
            *out_supported = supported;
        }
        SonalloyResult::Ok
    })
}
