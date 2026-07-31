use std::sync::Arc;

use thiserror::Error;

use crate::compiler::CompiledInstrument;
use crate::process::{
    InstrumentProcessor, ProcessBlock, ProcessContext, ProcessError, ProcessEvent, ProcessSpec,
    ScheduledEvent,
};
use crate::runtime::{InstrumentRuntime, SineRuntime};

const U64_LIMIT_AS_F64: f64 = 18_446_744_073_709_551_616.0;

/// An offline render request expressed in exact frame counts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderRequest {
    /// Sample rate in Hz.
    pub sample_rate: f64,
    /// Maximum process block size.
    pub block_size: usize,
    /// Main render duration in frames.
    pub duration_frames: u64,
    /// Additional frames rendered after the main duration.
    pub tail_frames: u64,
}

impl RenderRequest {
    /// Validate the request and return its total output length.
    ///
    /// # Errors
    ///
    /// Returns an error when the sample rate, block size, or total frame count is invalid.
    pub fn total_frames(self) -> Result<u64, RenderError> {
        if !self.sample_rate.is_finite() || self.sample_rate <= 0.0 {
            return Err(RenderError::InvalidSampleRate);
        }
        if self.sample_rate.fract() != 0.0 || self.sample_rate > f64::from(u32::MAX) {
            return Err(RenderError::InvalidSampleRate);
        }
        if self.block_size == 0 {
            return Err(RenderError::InvalidBlockSize);
        }
        self.duration_frames
            .checked_add(self.tail_frames)
            .ok_or(RenderError::FrameCountOverflow)
    }

    fn process_spec(self) -> Result<ProcessSpec, RenderError> {
        let _ = self.total_frames()?;
        Ok(ProcessSpec::new(self.sample_rate, self.block_size, 2)?)
    }
}

/// Audio produced by the Core renderer, without any file-format dependency.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedAudio {
    /// Integer sample rate suitable for a WAV header.
    pub sample_rate: u32,
    /// Channel-separated samples.
    pub channels: Vec<Vec<f32>>,
}

impl RenderedAudio {
    /// Number of frames in every channel.
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }
}

/// Errors raised by the offline renderer.
#[derive(Debug, Error, PartialEq)]
pub enum RenderError {
    /// The sample rate is unsupported by the output contract.
    #[error("sample rate must be a positive integer representable by wav")]
    InvalidSampleRate,
    /// The process block size must be positive.
    #[error("block size must be greater than zero")]
    InvalidBlockSize,
    /// The output frame count cannot fit in memory or a process counter.
    #[error("render frame count overflow")]
    FrameCountOverflow,
    /// The oscillator frequency is invalid.
    #[error("frequency must be finite and non-negative")]
    InvalidFrequency,
    /// The duration is invalid.
    #[error("duration must be finite and non-negative")]
    InvalidDuration,
    /// The runtime rejected a process block.
    #[error("process failed: {0}")]
    Process(#[from] ProcessError),
    /// Scheduled events are not ordered by absolute frame.
    #[error("scheduled events must be ordered by absolute frame")]
    ScheduledEventsNotSorted,
    /// A scheduled event lies outside the requested render.
    #[error("scheduled event at frame {frame} is outside the render")]
    ScheduledEventOutOfRange {
        /// Invalid absolute frame.
        frame: u64,
    },
}

/// Convert seconds to an exact frame count using round-to-nearest.
///
/// # Errors
///
/// Returns an error when the duration or sample rate is invalid, or when the result cannot fit
/// in a frame counter.
pub fn seconds_to_frames(seconds: f64, sample_rate: f64) -> Result<u64, RenderError> {
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(RenderError::InvalidDuration);
    }
    if !sample_rate.is_finite() || sample_rate <= 0.0 {
        return Err(RenderError::InvalidSampleRate);
    }
    let frames = (seconds * sample_rate).round();
    if !frames.is_finite() || !(0.0..U64_LIMIT_AS_F64).contains(&frames) {
        return Err(RenderError::FrameCountOverflow);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let frame_count = frames as u64;
    Ok(frame_count)
}

/// Render a sine oscillator through the Core Process Contract.
///
/// # Errors
///
/// Returns an error when the request is invalid or the runtime rejects a process block.
pub fn render_sine(
    frequency_hz: f32,
    request: RenderRequest,
) -> Result<RenderedAudio, RenderError> {
    if !frequency_hz.is_finite() || frequency_hz < 0.0 {
        return Err(RenderError::InvalidFrequency);
    }
    let mut runtime = SineRuntime::new(frequency_hz)?;
    render_processor(&mut runtime, request)
}

/// Render a compiled instrument from absolute-frame normalized events.
///
/// # Errors
///
/// Returns an error when the request, event timeline, or instrument runtime is invalid.
pub fn render_instrument(
    compiled: Arc<CompiledInstrument>,
    request: RenderRequest,
    events: &[ScheduledEvent],
) -> Result<RenderedAudio, RenderError> {
    let mut runtime = InstrumentRuntime::new(compiled);
    render_processor_with_events(&mut runtime, request, events)
}

fn render_processor<P: InstrumentProcessor>(
    processor: &mut P,
    request: RenderRequest,
) -> Result<RenderedAudio, RenderError> {
    render_processor_with_events(processor, request, &[])
}

fn render_processor_with_events<P: InstrumentProcessor>(
    processor: &mut P,
    request: RenderRequest,
    events: &[ScheduledEvent],
) -> Result<RenderedAudio, RenderError> {
    let total_frames = request.total_frames()?;
    let spec = request.process_spec()?;
    let total_frames_usize =
        usize::try_from(total_frames).map_err(|_| RenderError::FrameCountOverflow)?;
    for window in events.windows(2) {
        if window[0].absolute_frame > window[1].absolute_frame {
            return Err(RenderError::ScheduledEventsNotSorted);
        }
    }
    if let Some(event) = events
        .iter()
        .find(|event| event.absolute_frame >= total_frames)
    {
        return Err(RenderError::ScheduledEventOutOfRange {
            frame: event.absolute_frame,
        });
    }
    processor.prepare(spec)?;

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let sample_rate = request.sample_rate as u32;
    let mut audio = RenderedAudio {
        sample_rate,
        channels: vec![vec![0.0; total_frames_usize], vec![0.0; total_frames_usize]],
    };
    let mut offset = 0_usize;
    let mut event_index = 0_usize;
    let mut block_events = Vec::with_capacity(events.len());
    while offset < total_frames_usize {
        let frames = (total_frames_usize - offset).min(request.block_size);
        let end = offset + frames;
        block_events.clear();
        while event_index < events.len() && events[event_index].absolute_frame < end as u64 {
            let scheduled = events[event_index];
            if scheduled.absolute_frame < offset as u64 {
                return Err(RenderError::ScheduledEventsNotSorted);
            }
            block_events.push(ProcessEvent {
                sample_offset: usize::try_from(scheduled.absolute_frame - offset as u64)
                    .map_err(|_| RenderError::FrameCountOverflow)?,
                kind: scheduled.kind,
            });
            event_index += 1;
        }
        {
            let (left_channel, right_channel) = audio.channels.split_at_mut(1);
            let mut output: [&mut [f32]; 2] = [
                &mut left_channel[0][offset..end],
                &mut right_channel[0][offset..end],
            ];
            let block = ProcessBlock {
                frames,
                context: ProcessContext {
                    absolute_frame: offset as u64,
                    tempo_bpm: 120.0,
                },
                events: &block_events,
                output: &mut output,
            };
            processor.process(block)?;
        }
        offset = end;
    }
    Ok(audio)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{DiagnosticCode, DiagnosticSeverity, from_render_error};

    struct FailingProcessor {
        calls: usize,
    }

    impl InstrumentProcessor for FailingProcessor {
        fn prepare(&mut self, _spec: ProcessSpec) -> Result<(), ProcessError> {
            Ok(())
        }

        fn process(&mut self, _block: ProcessBlock<'_>) -> Result<(), ProcessError> {
            self.calls += 1;
            if self.calls == 2 {
                Err(ProcessError::EventsUnsupported)
            } else {
                Ok(())
            }
        }

        fn reset(&mut self) -> Result<(), ProcessError> {
            Ok(())
        }
    }

    fn request(duration_frames: u64, tail_frames: u64, block_size: usize) -> RenderRequest {
        RenderRequest {
            sample_rate: 48_000.0,
            block_size,
            duration_frames,
            tail_frames,
        }
    }

    #[test]
    fn duration_zero_produces_empty_stereo() {
        let audio = render_sine(440.0, request(0, 0, 257)).expect("zero-length render");
        assert_eq!(audio.sample_rate, 48_000);
        assert_eq!(audio.channels.len(), 2);
        assert_eq!(audio.frames(), 0);
    }

    #[test]
    fn one_frame_and_tail_are_rendered_exactly() {
        let audio = render_sine(440.0, request(1, 3, 2)).expect("short render");
        assert_eq!(audio.frames(), 4);
        assert!(
            audio
                .channels
                .iter()
                .flatten()
                .all(|sample| sample.is_finite())
        );
    }

    #[test]
    fn final_non_aligned_block_does_not_add_frames() {
        let audio = render_sine(440.0, request(17, 0, 5)).expect("non-aligned render");
        assert_eq!(audio.frames(), 17);
    }

    #[test]
    fn process_error_during_render_is_returned() {
        let mut processor = FailingProcessor { calls: 0 };
        let error = render_processor(&mut processor, request(17, 0, 5))
            .expect_err("second process call should fail");
        assert_eq!(error, RenderError::Process(ProcessError::EventsUnsupported));
        assert_eq!(processor.calls, 2);
    }

    #[test]
    fn duration_conversion_uses_round_to_nearest() {
        assert_eq!(seconds_to_frames(1.0, 48_000.0), Ok(48_000));
        assert_eq!(seconds_to_frames(0.5 / 48_000.0, 48_000.0), Ok(1));
        assert_eq!(
            seconds_to_frames(-1.0, 48_000.0),
            Err(RenderError::InvalidDuration)
        );
    }

    #[test]
    fn invalid_request_becomes_an_error_diagnostic() {
        let error = render_sine(440.0, request(1, 0, 0)).expect_err("invalid block size");
        let diagnostic = from_render_error(&error);
        assert_eq!(diagnostic.code, DiagnosticCode::RenderError);
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
    }
}
