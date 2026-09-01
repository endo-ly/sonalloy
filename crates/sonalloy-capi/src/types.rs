use std::ffi::c_char;
use std::slice;

use sonalloy_core::{
    ProcessContext, ProcessError, ProcessEvent, ProcessEventKind, ProcessSpec, TimeSignature,
    TransportState,
};

pub(crate) const SONALLOY_INTERNAL_PANIC: SonalloyResult = SonalloyResult::InternalPanic;
pub(crate) const MAX_EVENTS_PER_BLOCK: usize = 1024;

/// Result returned by every fallible C ABI operation.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SonalloyResult {
    /// The operation completed successfully.
    Ok = 0,
    /// A pointer, length, enum, or value was invalid.
    InvalidArgument = 1,
    /// The requested operation is not valid for the current lifecycle state.
    InvalidState = 2,
    /// Definition parsing or compilation failed.
    CompileFailed = 3,
    /// Runtime preparation failed.
    PrepareFailed = 4,
    /// Audio processing failed.
    ProcessFailed = 5,
    /// An update requires a compatible process configuration or reactivation.
    UpdateIncompatible = 6,
    /// The bounded generation or reclaim capacity was exhausted.
    UpdateCapacityExceeded = 7,
    /// A global processor transition is still in progress.
    TransitionBusy = 8,
    /// A Rust panic was contained at the ABI boundary.
    InternalPanic = 255,
}

/// A non-owning UTF-8 string view.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SonalloyStringView {
    /// Pointer to the first byte, or null when `length` is zero.
    pub data: *const c_char,
    /// Number of bytes in the view.
    pub length: usize,
}

impl SonalloyStringView {
    pub(crate) fn to_owned(self) -> Result<String, SonalloyResult> {
        if self.length == 0 {
            return Ok(String::new());
        }
        if self.data.is_null() {
            return Err(SonalloyResult::InvalidArgument);
        }
        let bytes = unsafe { slice::from_raw_parts(self.data.cast::<u8>(), self.length) };
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| SonalloyResult::InvalidArgument)
    }
}

/// Process preparation settings.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SonalloyProcessSpec {
    /// Engine sample rate in Hz.
    pub sample_rate: f64,
    /// Maximum process block size.
    pub max_block_size: u32,
    /// External input channel count.
    pub input_channels: u32,
    /// Output channel count; Sonalloy requires two.
    pub output_channels: u32,
}

impl SonalloyProcessSpec {
    pub(crate) fn to_core(self) -> Result<ProcessSpec, SonalloyResult> {
        ProcessSpec::new(
            self.sample_rate,
            usize::try_from(self.max_block_size).map_err(|_| SonalloyResult::InvalidArgument)?,
            usize::try_from(self.input_channels).map_err(|_| SonalloyResult::InvalidArgument)?,
            usize::try_from(self.output_channels).map_err(|_| SonalloyResult::InvalidArgument)?,
        )
        .map_err(|_| SonalloyResult::InvalidArgument)
    }
}

/// Host transport state.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SonalloyTransportState {
    /// The transport is stopped.
    Stopped = 0,
    /// The transport is playing.
    Playing = 1,
}

/// Process context supplied for one block.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SonalloyProcessContext {
    /// Absolute frame at which the block begins.
    pub absolute_frame: u64,
    /// Tempo in beats per minute.
    pub tempo_bpm: f64,
    /// Beat position at the beginning of the block.
    pub beat_position: f64,
    /// Bar position at the beginning of the block.
    pub bar_position: f64,
    /// Time-signature numerator.
    pub time_signature_numerator: u16,
    /// Time-signature denominator.
    pub time_signature_denominator: u16,
    /// Transport state value from [`SonalloyTransportState`].
    pub transport_state: u32,
}

impl SonalloyProcessContext {
    pub(crate) fn to_core(self) -> Result<ProcessContext, SonalloyResult> {
        let transport_state = match self.transport_state {
            0 => TransportState::Stopped,
            1 => TransportState::Playing,
            _ => return Err(SonalloyResult::InvalidArgument),
        };
        Ok(ProcessContext {
            absolute_frame: self.absolute_frame,
            tempo_bpm: self.tempo_bpm,
            beat_position: self.beat_position,
            bar_position: self.bar_position,
            time_signature: TimeSignature {
                numerator: self.time_signature_numerator,
                denominator: self.time_signature_denominator,
            },
            transport_state,
        })
    }
}

/// Event kinds accepted by the process function.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SonalloyEventType {
    /// Start a note.
    NoteOn = 1,
    /// Release a note.
    NoteOff = 2,
    /// Change sustain pedal state.
    Sustain = 3,
    /// Change a normalized parameter.
    ParameterChange = 4,
    /// Change pitch bend.
    PitchBend = 5,
    /// Change modulation wheel.
    ModWheel = 6,
    /// Change channel aftertouch.
    Aftertouch = 7,
}

/// Flat, fixed-layout process event.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SonalloyEvent {
    /// Offset from the beginning of the process block.
    pub sample_offset: u32,
    /// Event type value from [`SonalloyEventType`].
    pub event_type: u32,
    /// Note identity for note events.
    pub note_id: u64,
    /// Catalog revision used by a parameter event.
    pub parameter_catalog_revision: u64,
    /// Dense parameter handle for a parameter event.
    pub parameter_handle: u32,
    /// MIDI-style note number.
    pub note_number: u8,
    /// MIDI-style velocity.
    pub velocity: u8,
    /// Boolean sustain value, which must be zero or one.
    pub bool_value: u8,
    /// Reserved for ABI alignment and future validation.
    pub reserved: u8,
    /// Normalized or bipolar event value.
    pub value: f32,
}

impl SonalloyEvent {
    pub(crate) fn to_core(self) -> Result<ProcessEvent, SonalloyResult> {
        let kind = match self.event_type {
            1 => ProcessEventKind::NoteOn {
                note_id: self.note_id,
                note_number: self.note_number,
                velocity: self.velocity,
            },
            2 => ProcessEventKind::NoteOff {
                note_id: self.note_id,
            },
            3 => {
                if self.bool_value > 1 {
                    return Err(SonalloyResult::InvalidArgument);
                }
                ProcessEventKind::SustainPedal {
                    down: self.bool_value != 0,
                }
            }
            4 => ProcessEventKind::ParameterChange {
                catalog_revision: self.parameter_catalog_revision,
                parameter: sonalloy_core::ParameterHandle::from_index(
                    usize::try_from(self.parameter_handle)
                        .map_err(|_| SonalloyResult::InvalidArgument)?,
                ),
                normalized: self.value,
            },
            5 => ProcessEventKind::PitchBend { value: self.value },
            6 => ProcessEventKind::ModWheel { value: self.value },
            7 => ProcessEventKind::Aftertouch { value: self.value },
            _ => return Err(SonalloyResult::InvalidArgument),
        };
        Ok(ProcessEvent {
            sample_offset: usize::try_from(self.sample_offset)
                .map_err(|_| SonalloyResult::InvalidArgument)?,
            kind,
        })
    }
}

/// Public parameter metadata returned by catalog queries.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SonalloyParameterDescriptor {
    /// Canonical parameter identifier.
    pub id: SonalloyStringView,
    /// Owner kind identifier.
    pub owner_kind: u32,
    /// Primary owner index.
    pub owner_index: u32,
    /// Secondary owner index, when applicable.
    pub owner_sub_index: u32,
    /// Vector axis identifier, when applicable.
    pub owner_axis: u32,
    /// Native unit identifier.
    pub unit: u32,
    /// Normalization scale identifier.
    pub scale: u32,
    /// Inclusive native minimum.
    pub min: f32,
    /// Inclusive native maximum.
    pub max: f32,
    /// Native default.
    pub default: f32,
    /// Smoothing duration in seconds.
    pub smoothing_seconds: f32,
}

/// Metadata returned after a successful runtime update publication.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SonalloyPublishOutcome {
    /// Active generation identity.
    pub generation_id: u64,
    /// Active parameter catalog revision.
    pub parameter_catalog_revision: u64,
    /// Fixed reported output latency.
    pub reported_latency_frames: u32,
    /// Required external input channel count.
    pub required_input_channels: u32,
}

/// Fixed-size process error information retained by a runtime.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SonalloyRuntimeErrorInfo {
    /// Stable broad error category.
    pub code: u32,
    /// Stable detail category.
    pub detail_kind: u32,
    /// First numeric detail value.
    pub value_a: u64,
    /// Second numeric detail value.
    pub value_b: u64,
}

/// A diagnostic view whose strings borrow from a diagnostics handle.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SonalloyDiagnosticView {
    /// Stable diagnostic code.
    pub code: u32,
    /// Severity (`0` error, `1` warning, `2` info).
    pub severity: u32,
    /// Optional JSON-style field path.
    pub path: SonalloyStringView,
    /// Human-readable summary.
    pub message: SonalloyStringView,
    /// Optional detail string.
    pub detail: SonalloyStringView,
}

pub(crate) fn runtime_error_info(error: &ProcessError) -> SonalloyRuntimeErrorInfo {
    let (detail_kind, value_a, value_b) = match error {
        ProcessError::ContextDiscontinuity { received, expected } => (1, *received, *expected),
        ProcessError::ParameterHandleOutOfRange { handle } => (2, *handle as u64, 0),
        ProcessError::DuplicateNoteId { note_id } => (3, *note_id, 0),
        ProcessError::FrameCountExceedsMaximum {
            frames,
            max_block_size,
        } => (4, *frames as u64, *max_block_size as u64),
        _ => (0, 0, 0),
    };
    SonalloyRuntimeErrorInfo {
        code: 1,
        detail_kind,
        value_a,
        value_b,
    }
}
