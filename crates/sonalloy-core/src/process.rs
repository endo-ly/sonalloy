use thiserror::Error;

/// Audio preparation settings shared by every runtime processor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessSpec {
    /// Engine sample rate in Hz.
    pub sample_rate: f64,
    /// Maximum number of frames accepted by one process call.
    pub max_block_size: usize,
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
        output_channels: usize,
    ) -> Result<Self, ProcessError> {
        let spec = Self {
            sample_rate,
            max_block_size,
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
    /// Transport tempo. The runtime carries the value but does not use it.
    pub tempo_bpm: f64,
}

/// Stable identity used by normalized note events.
pub type NoteId = u64;

/// A normalized event positioned on the absolute engine timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledEvent {
    /// Absolute frame at which the event is applied.
    pub absolute_frame: u64,
    /// Event payload.
    pub kind: ProcessEventKind,
}

/// Normalized event payload reserved for the later voice runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

/// An event positioned within a process block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Planar output buffers and the context for one process call.
pub struct ProcessBlock<'a> {
    /// Number of frames to process.
    pub frames: usize,
    /// Absolute timing and transport context.
    pub context: ProcessContext,
    /// Normalized events in this block.
    pub events: &'a [ProcessEvent],
    /// Channel-separated output buffers.
    pub output: &'a mut [&'a mut [f32]],
}

impl ProcessBlock<'_> {
    /// Validate the buffer shape against a prepared process specification.
    ///
    /// # Errors
    ///
    /// Returns an error when the frame count, channel count, output lengths, or event offsets
    /// violate the process contract.
    pub fn validate_for(&self, spec: ProcessSpec) -> Result<(), ProcessError> {
        if self.frames > spec.max_block_size {
            return Err(ProcessError::FrameCountExceedsMaximum {
                frames: self.frames,
                max_block_size: spec.max_block_size,
            });
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
        for event in self.events {
            if event.sample_offset >= self.frames {
                return Err(ProcessError::EventOffsetOutOfRange {
                    offset: event.sample_offset,
                    frames: self.frames,
                });
            }
        }
        for window in self.events.windows(2) {
            if window[0].sample_offset > window[1].sample_offset {
                return Err(ProcessError::EventsNotSorted {
                    previous_offset: window[0].sample_offset,
                    current_offset: window[1].sample_offset,
                });
            }
        }
        let mut group_start = 0;
        while group_start < self.events.len() {
            let offset = self.events[group_start].sample_offset;
            let mut group_end = group_start + 1;
            while group_end < self.events.len() && self.events[group_end].sample_offset == offset {
                group_end += 1;
            }
            for on_index in group_start..group_end {
                if !is_note_on(self.events[on_index].kind) {
                    continue;
                }
                for off_index in (on_index + 1)..group_end {
                    if is_same_note_id(self.events[on_index].kind, self.events[off_index].kind)
                        && is_note_off(self.events[off_index].kind)
                    {
                        return Err(ProcessError::EventOrderInvalid);
                    }
                }
            }
            group_start = group_end;
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
    /// The maximum block size must be positive.
    #[error("maximum block size must be greater than zero")]
    InvalidMaxBlockSize,
    /// Exactly two output channels are required.
    #[error("stereo output requires exactly two output channels, got {actual}")]
    InvalidOutputChannels {
        /// Received channel count.
        actual: usize,
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
    /// A Note On precedes the matching Note Off at one sample offset.
    #[error("same-note Note Off must precede Note On at the same sample offset")]
    EventOrderInvalid,
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
}

fn is_note_on(event: ProcessEventKind) -> bool {
    matches!(event, ProcessEventKind::NoteOn { .. })
}

fn is_note_off(event: ProcessEventKind) -> bool {
    matches!(event, ProcessEventKind::NoteOff { .. })
}

fn is_same_note_id(left: ProcessEventKind, right: ProcessEventKind) -> bool {
    match (left, right) {
        (
            ProcessEventKind::NoteOn { note_id: left, .. },
            ProcessEventKind::NoteOff { note_id: right },
        ) => left == right,
        _ => false,
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
        }
    }

    #[test]
    fn process_spec_rejects_invalid_settings() {
        assert_eq!(
            ProcessSpec::new(0.0, 1024, 2),
            Err(ProcessError::InvalidSampleRate)
        );
        assert_eq!(
            ProcessSpec::new(f64::NAN, 1024, 2),
            Err(ProcessError::InvalidSampleRate)
        );
        assert_eq!(
            ProcessSpec::new(f64::INFINITY, 1024, 2),
            Err(ProcessError::InvalidSampleRate)
        );
        assert_eq!(
            ProcessSpec::new(48_000.0, 0, 2),
            Err(ProcessError::InvalidMaxBlockSize)
        );
        assert_eq!(
            ProcessSpec::new(48_000.0, 1024, 1),
            Err(ProcessError::InvalidOutputChannels { actual: 1 })
        );
        assert_eq!(
            ProcessSpec::new(48_000.0, 1024, 3),
            Err(ProcessError::InvalidOutputChannels { actual: 3 })
        );
    }

    #[test]
    fn process_block_accepts_zero_and_maximum_frames() {
        let spec = ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec");

        let mut zero_left = [0.0_f32; 4];
        let mut zero_right = [0.0_f32; 4];
        let mut zero_output: [&mut [f32]; 2] = [&mut zero_left, &mut zero_right];
        let zero_block = ProcessBlock {
            frames: 0,
            context: context(),
            events: &[],
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
            output: &mut max_output,
        };
        assert!(max_block.validate_for(spec).is_ok());
    }

    #[test]
    fn process_block_rejects_invalid_shapes_and_offsets() {
        let spec = ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec");

        let mut large_left = [0.0_f32; 65];
        let mut large_right = [0.0_f32; 65];
        let mut large_output: [&mut [f32]; 2] = [&mut large_left, &mut large_right];
        let large_block = ProcessBlock {
            frames: 65,
            context: context(),
            events: &[],
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
            Err(ProcessError::EventOffsetOutOfRange { .. })
        ));
    }

    #[test]
    fn same_offset_note_off_precedes_matching_note_on() {
        let spec = ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec");
        let invalid_events = [
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
                kind: ProcessEventKind::NoteOff { note_id: 7 },
            },
        ];
        let mut left = [0.0_f32; 64];
        let mut right = [0.0_f32; 64];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        let block = ProcessBlock {
            frames: 64,
            context: context(),
            events: &invalid_events,
            output: &mut output,
        };
        assert_eq!(
            block.validate_for(spec),
            Err(ProcessError::EventOrderInvalid)
        );

        let valid_events = [
            ProcessEvent {
                sample_offset: 4,
                kind: ProcessEventKind::NoteOff { note_id: 7 },
            },
            ProcessEvent {
                sample_offset: 4,
                kind: ProcessEventKind::NoteOn {
                    note_id: 7,
                    note_number: 60,
                    velocity: 100,
                },
            },
        ];
        let mut left = [0.0_f32; 64];
        let mut right = [0.0_f32; 64];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        let block = ProcessBlock {
            frames: 64,
            context: context(),
            events: &valid_events,
            output: &mut output,
        };
        assert!(block.validate_for(spec).is_ok());
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
