use thiserror::Error;

use crate::parameter::ParameterHandle;

/// Audio preparation settings shared by every runtime processor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessSpec {
    /// Engine sample rate in Hz.
    pub sample_rate: f64,
    /// Maximum number of frames accepted by one process call.
    pub max_block_size: usize,
    /// Number of planar external input channels.
    pub input_channels: usize,
    /// Number of planar output channels. The runtime requires stereo.
    pub output_channels: usize,
}

impl ProcessSpec {
    /// Construct and validate a process specification.
    ///
    /// # Errors
    ///
    /// Returns an error when the sample rate, block size, or channel count violates the
    /// process contract.
    pub fn new(
        sample_rate: f64,
        max_block_size: usize,
        input_channels: usize,
        output_channels: usize,
    ) -> Result<Self, ProcessError> {
        let spec = Self {
            sample_rate,
            max_block_size,
            input_channels,
            output_channels,
        };
        spec.validate()?;
        Ok(spec)
    }

    /// Validate the settings without allocating runtime state.
    ///
    /// # Errors
    ///
    /// Returns an error when the sample rate, block size, or channel count violates the
    /// process contract.
    pub fn validate(&self) -> Result<(), ProcessError> {
        if !self.sample_rate.is_finite() || self.sample_rate <= 0.0 {
            return Err(ProcessError::InvalidSampleRate);
        }
        if self.max_block_size == 0 {
            return Err(ProcessError::InvalidMaxBlockSize);
        }
        if self.input_channels > 2 {
            return Err(ProcessError::InvalidInputChannels {
                actual: self.input_channels,
            });
        }
        if self.output_channels != 2 {
            return Err(ProcessError::InvalidOutputChannels {
                actual: self.output_channels,
            });
        }
        Ok(())
    }
}

/// Context associated with one process block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessContext {
    /// Absolute frame at which the block begins.
    pub absolute_frame: u64,
    /// Transport tempo in beats per minute. It is constant for one process call.
    pub tempo_bpm: f64,
    /// Continuous quarter-note position at the beginning of the block.
    pub beat_position: f64,
    /// Continuous bar position at the beginning of the block.
    pub bar_position: f64,
    /// Meter active for the block.
    pub time_signature: TimeSignature,
}

/// Musical meter used by transport-aware modulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSignature {
    /// Number of denominator units in one bar.
    pub numerator: u16,
    /// Denominator, which must be a power of two from 1 through 128.
    pub denominator: u16,
}

impl TimeSignature {
    /// Return the bar length in quarter-note beats.
    #[must_use]
    pub fn beats_per_bar(self) -> f64 {
        f64::from(self.numerator) * 4.0 / f64::from(self.denominator)
    }

    /// Validate the meter contract.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.numerator > 0
            && self.denominator.is_power_of_two()
            && self.denominator <= 128
            && self.beats_per_bar().is_finite()
            && self.beats_per_bar() > 0.0
    }
}

/// Default meter used by offline and realtime adapters.
pub const DEFAULT_TIME_SIGNATURE: TimeSignature = TimeSignature {
    numerator: 4,
    denominator: 4,
};

/// Stable identity used by normalized note events.
pub type NoteId = u64;

/// A normalized event positioned on the absolute engine timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScheduledEvent {
    /// Absolute frame at which the event is applied.
    pub absolute_frame: u64,
    /// Event payload.
    pub kind: ProcessEventKind,
}

/// Normalized event payload consumed by the voice runtime.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessEventKind {
    /// Start a note.
    NoteOn {
        /// Frontend-assigned note identity.
        note_id: NoteId,
        /// MIDI-style note number.
        note_number: u8,
        /// MIDI-style velocity.
        velocity: u8,
    },
    /// Stop a note.
    NoteOff {
        /// Frontend-assigned note identity.
        note_id: NoteId,
    },
    /// Hold released notes until the sustain pedal is lifted.
    SustainPedal {
        /// Whether the pedal is currently held down.
        down: bool,
    },
    /// Change a continuous parameter's normalized base value.
    ParameterChange {
        /// Compiled parameter handle resolved by control code.
        parameter: ParameterHandle,
        /// Target value in the inclusive zero-to-one range.
        normalized: f32,
    },
    /// Change the shared pitch bend control.
    PitchBend {
        /// Bipolar normalized value.
        value: f32,
    },
    /// Change the shared modulation wheel control.
    ModWheel {
        /// Unipolar normalized value.
        value: f32,
    },
    /// Change the shared channel aftertouch control.
    Aftertouch {
        /// Unipolar normalized value.
        value: f32,
    },
}

impl ProcessEventKind {
    /// Return the canonicalization priority used by frontend adapters for same-offset events.
    #[must_use]
    pub const fn priority(self) -> u8 {
        match self {
            Self::SustainPedal { .. } => 0,
            Self::NoteOff { .. } => 1,
            Self::ParameterChange { .. } => 2,
            Self::PitchBend { .. } => 3,
            Self::ModWheel { .. } => 4,
            Self::Aftertouch { .. } => 5,
            Self::NoteOn { .. } => 6,
        }
    }

    fn validate_value(self) -> Result<(), ProcessError> {
        let (value, min, max) = match self {
            Self::NoteOn {
                note_number,
                velocity,
                ..
            } => {
                if note_number > 127 || !(1..=127).contains(&velocity) {
                    return Err(ProcessError::InvalidEventValue);
                }
                return Ok(());
            }
            Self::NoteOff { .. } | Self::SustainPedal { .. } => return Ok(()),
            Self::ParameterChange { normalized, .. } => (normalized, 0.0, 1.0),
            Self::PitchBend { value } => (value, -1.0, 1.0),
            Self::ModWheel { value } | Self::Aftertouch { value } => (value, 0.0, 1.0),
        };
        if value.is_finite() && (min..=max).contains(&value) {
            Ok(())
        } else {
            Err(ProcessError::InvalidEventValue)
        }
    }
}

/// An event positioned within a process block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessEvent {
    /// Sample offset from the start of the containing block.
    pub sample_offset: usize,
    /// Event payload.
    pub kind: ProcessEventKind,
}

/// Backend-independent categories for DSP failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DspFailureKind {
    /// The processor could not allocate required resources.
    ResourceUnavailable,
    /// The processor received an invalid input.
    InvalidInput,
    /// The processor was in an invalid lifecycle state.
    InvalidState,
    /// The backend failed without a caller-correctable cause.
    BackendFailure,
}

/// Categories of failures produced by an in-process processor implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessorFailureKind {
    /// A compiled processor or runtime state violated its lifecycle contract.
    InvalidState,
    /// A finite input violated a processor's parameter contract.
    InvalidInput,
    /// A processor received or produced a non-finite sample.
    NonFinite,
}

impl std::fmt::Display for ProcessorFailureKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidState => "invalid state",
            Self::InvalidInput => "invalid input",
            Self::NonFinite => "non-finite sample",
        };
        formatter.write_str(message)
    }
}

impl std::fmt::Display for DspFailureKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::ResourceUnavailable => "resource unavailable",
            Self::InvalidInput => "invalid input",
            Self::InvalidState => "invalid state",
            Self::BackendFailure => "backend failure",
        };
        formatter.write_str(message)
    }
}

/// Planar input/output buffers and the context for one process call.
pub struct ProcessBlock<'a> {
    /// Number of frames to process.
    pub frames: usize,
    /// Absolute timing and transport context.
    pub context: ProcessContext,
    /// Normalized events in this block.
    pub events: &'a [ProcessEvent],
    /// Read-only channel-separated external input buffers.
    pub input: &'a [&'a [f32]],
    /// Channel-separated output buffers.
    pub output: &'a mut [&'a mut [f32]],
}

impl ProcessBlock<'_> {
    /// Validate the buffer shape against a prepared process specification.
    ///
    /// # Errors
    ///
    /// Returns an error when the frame count, channel count, output lengths, or event offsets
    /// violate the process contract. Events with the same offset are applied in slice order.
    pub fn validate_for(&self, spec: ProcessSpec) -> Result<(), ProcessError> {
        if !self.context.tempo_bpm.is_finite() || self.context.tempo_bpm <= 0.0 {
            return Err(ProcessError::InvalidTempo);
        }
        if !self.context.beat_position.is_finite()
            || self.context.beat_position < 0.0
            || !self.context.bar_position.is_finite()
            || self.context.bar_position < 0.0
            || !self.context.time_signature.is_valid()
        {
            return Err(ProcessError::InvalidMusicalTime);
        }
        if self.frames > spec.max_block_size {
            return Err(ProcessError::FrameCountExceedsMaximum {
                frames: self.frames,
                max_block_size: spec.max_block_size,
            });
        }
        if self.input.len() != spec.input_channels {
            return Err(ProcessError::InvalidInputChannels {
                actual: self.input.len(),
            });
        }
        for (channel, buffer) in self.input.iter().enumerate() {
            if buffer.len() < self.frames {
                return Err(ProcessError::InputBufferTooShort {
                    channel,
                    available: buffer.len(),
                    required: self.frames,
                });
            }
            for (frame, sample) in buffer[..self.frames].iter().enumerate() {
                if !sample.is_finite() {
                    return Err(ProcessError::InputSampleNonFinite { channel, frame });
                }
            }
        }
        if self.output.len() != spec.output_channels {
            return Err(ProcessError::InvalidOutputChannels {
                actual: self.output.len(),
            });
        }
        for (channel, buffer) in self.output.iter().enumerate() {
            if buffer.len() < self.frames {
                return Err(ProcessError::OutputBufferTooShort {
                    channel,
                    available: buffer.len(),
                    required: self.frames,
                });
            }
        }
        if self.frames == 0 && !self.events.is_empty() {
            return Err(ProcessError::ZeroFrameEvents);
        }
        for event in self.events {
            if event.sample_offset >= self.frames {
                return Err(ProcessError::EventOffsetOutOfRange {
                    offset: event.sample_offset,
                    frames: self.frames,
                });
            }
        }
        for event in self.events {
            event.kind.validate_value()?;
        }
        for window in self.events.windows(2) {
            if window[0].sample_offset > window[1].sample_offset {
                return Err(ProcessError::EventsNotSorted {
                    previous_offset: window[0].sample_offset,
                    current_offset: window[1].sample_offset,
                });
            }
        }
        Ok(())
    }
}

/// Errors at the common process boundary.
#[derive(Debug, Error, PartialEq)]
pub enum ProcessError {
    /// The sample rate is not finite and positive.
    #[error("sample rate must be finite and greater than zero")]
    InvalidSampleRate,
    /// The runtime was prepared at a different sample rate from the compiled instrument.
    #[error("compiled instrument uses sample rate {compiled} Hz, requested {requested} Hz")]
    SampleRateMismatch {
        /// Sample rate captured during compilation.
        compiled: f64,
        /// Sample rate requested by the runtime host.
        requested: f64,
    },
    /// The runtime input channel count does not match the compiled instrument requirement.
    #[error(
        "compiled instrument requires {compiled} external input channels, requested {requested}"
    )]
    InputChannelRequirementMismatch {
        /// Channel count captured during compilation.
        compiled: usize,
        /// Channel count requested by the runtime host.
        requested: usize,
    },
    /// The maximum block size must be positive.
    #[error("maximum block size must be greater than zero")]
    InvalidMaxBlockSize,
    /// Exactly two output channels are required.
    #[error("stereo output requires exactly two output channels, got {actual}")]
    InvalidOutputChannels {
        /// Received channel count.
        actual: usize,
    },
    /// The external input channel count is outside the supported range or does not match the
    /// prepared specification.
    #[error("external input requires a different channel count, got {actual}")]
    InvalidInputChannels {
        /// Received channel count.
        actual: usize,
    },
    /// An input channel does not have enough readable frames.
    #[error("input channel {channel} has {available} frames, {required} required")]
    InputBufferTooShort {
        /// Channel index.
        channel: usize,
        /// Available readable frames.
        available: usize,
        /// Required readable frames.
        required: usize,
    },
    /// An external input sample is not finite.
    #[error("input channel {channel} frame {frame} is non-finite")]
    InputSampleNonFinite {
        /// Channel index.
        channel: usize,
        /// Frame index within the block.
        frame: usize,
    },
    /// The block is larger than the prepared maximum.
    #[error("frame count {frames} exceeds maximum block size {max_block_size}")]
    FrameCountExceedsMaximum {
        /// Requested frame count.
        frames: usize,
        /// Prepared maximum.
        max_block_size: usize,
    },
    /// An output channel does not have enough writable frames.
    #[error("output channel {channel} has {available} frames, {required} required")]
    OutputBufferTooShort {
        /// Channel index.
        channel: usize,
        /// Available writable frames.
        available: usize,
        /// Required writable frames.
        required: usize,
    },
    /// An event lies outside the current block.
    #[error("event offset {offset} is outside block with {frames} frames")]
    EventOffsetOutOfRange {
        /// Invalid sample offset.
        offset: usize,
        /// Block length.
        frames: usize,
    },
    /// Events are not ordered by sample offset.
    #[error("events must be ordered by non-decreasing sample offset")]
    EventsNotSorted {
        /// Previous event offset.
        previous_offset: usize,
        /// Current event offset.
        current_offset: usize,
    },
    /// An event was supplied to a zero-frame block.
    #[error("zero-frame blocks cannot contain events")]
    ZeroFrameEvents,
    /// An event value is non-finite or outside its normalized range.
    #[error("event value is invalid")]
    InvalidEventValue,
    /// A monophonic note identity was already held.
    #[error("note id {note_id} is already held")]
    DuplicateNoteId {
        /// Duplicate identity.
        note_id: NoteId,
    },
    /// The monophonic held-note stack is full.
    #[error("monophonic held note limit {limit} exceeded")]
    MonophonicHeldNoteLimitExceeded {
        /// Maximum number of held notes.
        limit: usize,
    },
    /// The process context tempo is non-finite or not positive.
    #[error("tempo must be finite and greater than zero")]
    InvalidTempo,
    /// Musical positions or the time signature are invalid.
    #[error("musical time context is invalid")]
    InvalidMusicalTime,
    /// A tempo-derived stretch ratio is outside the supported range.
    #[error("stretch ratio {ratio} is outside the supported range 0.5..=2.0")]
    StretchRatioOutOfRange {
        /// Invalid duration ratio.
        ratio: f64,
    },
    /// A parameter handle is not part of the compiled catalog.
    #[error("parameter handle {handle} is outside the compiled catalog")]
    ParameterHandleOutOfRange {
        /// Invalid dense parameter index.
        handle: usize,
    },
    /// A compiled parameter default violates its descriptor range.
    #[error("compiled parameter default is invalid")]
    InvalidCompiledParameterDefault,
    /// A runtime has not been prepared.
    #[error("processor is not prepared")]
    NotPrepared,
    /// The caller's absolute frame does not match runtime state.
    #[error("process context starts at frame {received}, expected {expected}")]
    ContextDiscontinuity {
        /// Context frame supplied by the caller.
        received: u64,
        /// Runtime frame expected by the processor.
        expected: u64,
    },
    /// The sine runtime does not consume events.
    #[error("events are not supported by the sine runtime")]
    EventsUnsupported,
    /// The requested oscillator frequency is invalid.
    #[error("frequency must be finite, non-negative, and below the nyquist frequency")]
    InvalidFrequency,
    /// The absolute frame counter would overflow.
    #[error("absolute frame counter overflow")]
    FrameOverflow,
    /// The DSP backend failed.
    #[error("dsp failure: {kind}")]
    DspFailure {
        /// Backend-independent failure category.
        kind: DspFailureKind,
    },
    /// A Rust processor implementation failed its runtime contract.
    #[error("processor failure: {kind}")]
    ProcessorFailure {
        /// Processor failure category.
        kind: ProcessorFailureKind,
    },
}

impl ProcessError {
    pub(crate) fn from_dsp_error(error: sonalloy_dsp_sys::DspError) -> Self {
        let kind = match error {
            sonalloy_dsp_sys::DspError::AllocationFailed => DspFailureKind::ResourceUnavailable,
            sonalloy_dsp_sys::DspError::InvalidArgument
            | sonalloy_dsp_sys::DspError::UnsupportedWaveform => DspFailureKind::InvalidInput,
            sonalloy_dsp_sys::DspError::NotPrepared => DspFailureKind::InvalidState,
            sonalloy_dsp_sys::DspError::NullHandle
            | sonalloy_dsp_sys::DspError::NativeException
            | sonalloy_dsp_sys::DspError::Unknown(_) => DspFailureKind::BackendFailure,
        };
        Self::DspFailure { kind }
    }

    pub(crate) fn from_filter_error(error: sonalloy_dsp_sys::DspFilterError) -> Self {
        let kind = match error {
            sonalloy_dsp_sys::DspFilterError::AllocationFailed => {
                DspFailureKind::ResourceUnavailable
            }
            sonalloy_dsp_sys::DspFilterError::InvalidArgument => DspFailureKind::InvalidInput,
            sonalloy_dsp_sys::DspFilterError::NotPrepared => DspFailureKind::InvalidState,
            sonalloy_dsp_sys::DspFilterError::NullHandle
            | sonalloy_dsp_sys::DspFilterError::NativeException
            | sonalloy_dsp_sys::DspFilterError::Unknown(_) => DspFailureKind::BackendFailure,
        };
        Self::DspFailure { kind }
    }

    pub(crate) fn from_wavefolder_error(error: sonalloy_dsp_sys::DspWavefolderError) -> Self {
        let kind = match error {
            sonalloy_dsp_sys::DspWavefolderError::AllocationFailed => {
                DspFailureKind::ResourceUnavailable
            }
            sonalloy_dsp_sys::DspWavefolderError::InvalidArgument => DspFailureKind::InvalidInput,
            sonalloy_dsp_sys::DspWavefolderError::NotPrepared => DspFailureKind::InvalidState,
            sonalloy_dsp_sys::DspWavefolderError::NonFinite => {
                return Self::ProcessorFailure {
                    kind: ProcessorFailureKind::NonFinite,
                };
            }
            sonalloy_dsp_sys::DspWavefolderError::NullHandle
            | sonalloy_dsp_sys::DspWavefolderError::NativeException
            | sonalloy_dsp_sys::DspWavefolderError::Unknown(_) => DspFailureKind::BackendFailure,
        };
        Self::DspFailure { kind }
    }

    pub(crate) fn from_modal_resonator_error(
        error: sonalloy_dsp_sys::DspModalResonatorError,
    ) -> Self {
        let kind = match error {
            sonalloy_dsp_sys::DspModalResonatorError::AllocationFailed => {
                DspFailureKind::ResourceUnavailable
            }
            sonalloy_dsp_sys::DspModalResonatorError::InvalidArgument => {
                DspFailureKind::InvalidInput
            }
            sonalloy_dsp_sys::DspModalResonatorError::NotPrepared => DspFailureKind::InvalidState,
            sonalloy_dsp_sys::DspModalResonatorError::NonFinite => {
                return Self::ProcessorFailure {
                    kind: ProcessorFailureKind::NonFinite,
                };
            }
            sonalloy_dsp_sys::DspModalResonatorError::NullHandle
            | sonalloy_dsp_sys::DspModalResonatorError::NativeException
            | sonalloy_dsp_sys::DspModalResonatorError::Unknown(_) => {
                DspFailureKind::BackendFailure
            }
        };
        Self::DspFailure { kind }
    }

    pub(crate) fn from_stretch_error(error: sonalloy_dsp_sys::DspStretchError) -> Self {
        let kind = match error {
            sonalloy_dsp_sys::DspStretchError::AllocationFailed => {
                DspFailureKind::ResourceUnavailable
            }
            sonalloy_dsp_sys::DspStretchError::InvalidArgument => DspFailureKind::InvalidInput,
            sonalloy_dsp_sys::DspStretchError::NotPrepared => DspFailureKind::InvalidState,
            sonalloy_dsp_sys::DspStretchError::NonFinite => {
                return Self::ProcessorFailure {
                    kind: ProcessorFailureKind::NonFinite,
                };
            }
            sonalloy_dsp_sys::DspStretchError::NullHandle
            | sonalloy_dsp_sys::DspStretchError::NativeException
            | sonalloy_dsp_sys::DspStretchError::Unknown(_) => DspFailureKind::BackendFailure,
        };
        Self::DspFailure { kind }
    }
}

/// A processor that follows the prepare/process/reset lifecycle.
pub trait InstrumentProcessor {
    /// Allocate and initialize audio state for a process specification.
    ///
    /// # Errors
    ///
    /// Returns an error when the specification cannot be prepared.
    fn prepare(&mut self, spec: ProcessSpec) -> Result<(), ProcessError>;
    /// Process one variable-sized planar block.
    ///
    /// # Errors
    ///
    /// Returns an error when the block violates the process contract or processing fails.
    fn process(&mut self, block: ProcessBlock<'_>) -> Result<(), ProcessError>;
    /// Return the processor to its prepared initial state.
    ///
    /// # Errors
    ///
    /// Returns an error when the processor has not been prepared or cannot reset its state.
    fn reset(&mut self) -> Result<(), ProcessError>;
}

/// Clear the writable portion of all output channels.
pub fn clear_output(output: &mut [&mut [f32]], frames: usize) {
    for channel in output {
        let writable_frames = frames.min(channel.len());
        channel[..writable_frames].fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> ProcessContext {
        ProcessContext {
            absolute_frame: 0,
            tempo_bpm: 120.0,
            beat_position: 0.0,
            bar_position: 0.0,
            time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
        }
    }

    #[test]
    fn process_spec_rejects_invalid_settings() {
        assert_eq!(
            ProcessSpec::new(0.0, 1024, 0, 2),
            Err(ProcessError::InvalidSampleRate)
        );
        assert_eq!(
            ProcessSpec::new(f64::NAN, 1024, 0, 2),
            Err(ProcessError::InvalidSampleRate)
        );
        assert_eq!(
            ProcessSpec::new(f64::INFINITY, 1024, 0, 2),
            Err(ProcessError::InvalidSampleRate)
        );
        assert_eq!(
            ProcessSpec::new(48_000.0, 0, 0, 2),
            Err(ProcessError::InvalidMaxBlockSize)
        );
        assert_eq!(
            ProcessSpec::new(48_000.0, 1024, 0, 1),
            Err(ProcessError::InvalidOutputChannels { actual: 1 })
        );
        assert_eq!(
            ProcessSpec::new(48_000.0, 1024, 0, 3),
            Err(ProcessError::InvalidOutputChannels { actual: 3 })
        );
        assert_eq!(
            ProcessSpec::new(48_000.0, 1024, 3, 2),
            Err(ProcessError::InvalidInputChannels { actual: 3 })
        );
    }

    #[test]
    fn process_block_accepts_zero_and_maximum_frames() {
        let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");

        let mut zero_left = [0.0_f32; 4];
        let mut zero_right = [0.0_f32; 4];
        let mut zero_output: [&mut [f32]; 2] = [&mut zero_left, &mut zero_right];
        let zero_block = ProcessBlock {
            frames: 0,
            context: context(),
            events: &[],
            input: &[],
            output: &mut zero_output,
        };
        assert!(zero_block.validate_for(spec).is_ok());

        let mut max_left = [0.0_f32; 64];
        let mut max_right = [0.0_f32; 64];
        let mut max_output: [&mut [f32]; 2] = [&mut max_left, &mut max_right];
        let max_block = ProcessBlock {
            frames: 64,
            context: context(),
            events: &[],
            input: &[],
            output: &mut max_output,
        };
        assert!(max_block.validate_for(spec).is_ok());
    }

    #[test]
    fn process_block_rejects_invalid_tempo() {
        let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");

        for tempo_bpm in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut left = [0.0_f32; 1];
            let mut right = [0.0_f32; 1];
            let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
            let block = ProcessBlock {
                frames: 1,
                context: ProcessContext {
                    absolute_frame: 0,
                    tempo_bpm,
                    beat_position: 0.0,
                    bar_position: 0.0,
                    time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                },
                events: &[],
                input: &[],
                output: &mut output,
            };

            assert_eq!(block.validate_for(spec), Err(ProcessError::InvalidTempo));
        }
    }

    #[test]
    fn process_block_rejects_invalid_musical_positions_and_meter() {
        let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
        for (beat_position, bar_position, time_signature) in [
            (f64::NAN, 0.0, DEFAULT_TIME_SIGNATURE),
            (0.0, -1.0, DEFAULT_TIME_SIGNATURE),
            (
                0.0,
                0.0,
                TimeSignature {
                    numerator: 4,
                    denominator: 3,
                },
            ),
        ] {
            let mut left = [0.0_f32; 1];
            let mut right = [0.0_f32; 1];
            let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
            let block = ProcessBlock {
                frames: 1,
                context: ProcessContext {
                    absolute_frame: 0,
                    tempo_bpm: 120.0,
                    beat_position,
                    bar_position,
                    time_signature,
                },
                events: &[],
                input: &[],
                output: &mut output,
            };

            assert_eq!(
                block.validate_for(spec),
                Err(ProcessError::InvalidMusicalTime)
            );
        }
    }

    #[test]
    fn process_block_rejects_invalid_shapes_and_offsets() {
        let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");

        let mut large_left = [0.0_f32; 65];
        let mut large_right = [0.0_f32; 65];
        let mut large_output: [&mut [f32]; 2] = [&mut large_left, &mut large_right];
        let large_block = ProcessBlock {
            frames: 65,
            context: context(),
            events: &[],
            input: &[],
            output: &mut large_output,
        };
        assert!(matches!(
            large_block.validate_for(spec),
            Err(ProcessError::FrameCountExceedsMaximum { .. })
        ));

        let mut short_left = [0.0_f32; 4];
        let mut short_right = [0.0_f32; 4];
        let mut short_output: [&mut [f32]; 2] = [&mut short_left, &mut short_right];
        let short_block = ProcessBlock {
            frames: 8,
            context: context(),
            events: &[],
            input: &[],
            output: &mut short_output,
        };
        assert!(matches!(
            short_block.validate_for(spec),
            Err(ProcessError::OutputBufferTooShort { .. })
        ));

        let mut one_channel = [0.0_f32; 8];
        let mut one_output: [&mut [f32]; 1] = [&mut one_channel];
        let one_channel_block = ProcessBlock {
            frames: 8,
            context: context(),
            events: &[],
            input: &[],
            output: &mut one_output,
        };
        assert_eq!(
            one_channel_block.validate_for(spec),
            Err(ProcessError::InvalidOutputChannels { actual: 1 })
        );

        assert!(validate_event(spec, 64, 0).is_ok());
        assert!(validate_event(spec, 64, 63).is_ok());
        assert!(matches!(
            validate_event(spec, 64, 64),
            Err(ProcessError::EventOffsetOutOfRange { .. })
        ));
        assert!(matches!(
            validate_event(spec, 0, 0),
            Err(ProcessError::ZeroFrameEvents)
        ));
    }

    #[test]
    fn process_block_rejects_short_and_non_finite_external_input() {
        let spec = ProcessSpec::new(48_000.0, 64, 1, 2).expect("valid process spec");
        let short_input = [[0.0_f32; 2]; 1];
        let mut left = [0.0_f32; 4];
        let mut right = [0.0_f32; 4];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        let short_input_channels: [&[f32]; 1] = [&short_input[0]];
        {
            let block = ProcessBlock {
                frames: 4,
                context: context(),
                events: &[],
                input: &short_input_channels,
                output: &mut output,
            };
            assert_eq!(
                block.validate_for(spec),
                Err(ProcessError::InputBufferTooShort {
                    channel: 0,
                    available: 2,
                    required: 4,
                })
            );
        }

        let non_finite_input = [f32::NAN; 4];
        let mut non_finite_left = [0.0_f32; 4];
        let mut non_finite_right = [0.0_f32; 4];
        let mut non_finite_output: [&mut [f32]; 2] = [&mut non_finite_left, &mut non_finite_right];
        let block = ProcessBlock {
            frames: 4,
            context: context(),
            events: &[],
            input: &[&non_finite_input],
            output: &mut non_finite_output,
        };
        assert_eq!(
            block.validate_for(spec),
            Err(ProcessError::InputSampleNonFinite {
                channel: 0,
                frame: 0,
            })
        );
    }

    #[test]
    fn same_offset_events_follow_the_input_order() {
        let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
        let events = [
            ProcessEvent {
                sample_offset: 4,
                kind: ProcessEventKind::NoteOn {
                    note_id: 7,
                    note_number: 60,
                    velocity: 100,
                },
            },
            ProcessEvent {
                sample_offset: 4,
                kind: ProcessEventKind::NoteOn {
                    note_id: 8,
                    note_number: 64,
                    velocity: 100,
                },
            },
            ProcessEvent {
                sample_offset: 4,
                kind: ProcessEventKind::SustainPedal { down: true },
            },
            ProcessEvent {
                sample_offset: 4,
                kind: ProcessEventKind::NoteOff { note_id: 7 },
            },
        ];
        let mut left = [0.0_f32; 64];
        let mut right = [0.0_f32; 64];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        let block = ProcessBlock {
            frames: 64,
            context: context(),
            events: &events,
            input: &[],
            output: &mut output,
        };
        assert_eq!(block.validate_for(spec), Ok(()));
    }

    fn validate_event(
        spec: ProcessSpec,
        frames: usize,
        sample_offset: usize,
    ) -> Result<(), ProcessError> {
        let event = ProcessEvent {
            sample_offset,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        };
        let mut left = [0.0_f32; 64];
        let mut right = [0.0_f32; 64];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        ProcessBlock {
            frames,
            context: context(),
            events: &[event],
            input: &[],
            output: &mut output,
        }
        .validate_for(spec)
    }

    #[test]
    fn clear_output_clears_only_the_target_range() {
        let mut left = [1.0_f32; 6];
        let mut right = [2.0_f32; 6];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        clear_output(&mut output, 4);
        assert!(left[..4].iter().all(|sample| sample.abs() < f32::EPSILON));
        assert!(right[..4].iter().all(|sample| sample.abs() < f32::EPSILON));
        assert!((left[4] - 1.0).abs() < f32::EPSILON);
        assert!((left[5] - 1.0).abs() < f32::EPSILON);
        assert!((right[4] - 2.0).abs() < f32::EPSILON);
        assert!((right[5] - 2.0).abs() < f32::EPSILON);
    }
}
