use std::sync::Arc;

use thiserror::Error;

use crate::compiler::CompiledInstrument;
use crate::process::{
    DEFAULT_TIME_SIGNATURE, InstrumentProcessor, ProcessBlock, ProcessContext, ProcessError,
    ProcessEvent, ProcessSpec, ScheduledEvent, TimeSignature,
};
use crate::runtime::{InstrumentRuntime, SineRuntime};
use crate::trace::{
    MAX_TRACE_OBSERVATIONS, RenderTraceReport, TraceCollectError, TraceCollector, TraceRequest,
};

const U64_LIMIT_AS_F64: f64 = 18_446_744_073_709_551_616.0;

/// The default tempo used by renders without an explicit tempo.
pub const DEFAULT_TEMPO_BPM: f64 = 120.0;

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

/// A tempo and meter change at an absolute render frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MusicalTimeChange {
    /// Absolute frame at which this tempo and meter become active.
    pub absolute_frame: u64,
    /// Tempo in beats per minute.
    pub tempo_bpm: f64,
    /// Meter that becomes active at the change.
    pub time_signature: TimeSignature,
}

/// An ordered musical-time map used by the offline renderer.
#[derive(Debug, Clone, PartialEq)]
pub struct MusicalTimeMap {
    changes: Vec<MusicalTimeChange>,
}

impl MusicalTimeMap {
    /// Create a musical-time map whose first change is active at frame zero.
    ///
    /// # Errors
    ///
    /// Returns an error when the map is empty, does not start at frame zero, is not strictly
    /// ordered, or contains a non-positive or non-finite tempo.
    pub fn new(changes: Vec<MusicalTimeChange>) -> Result<Self, RenderError> {
        let Some(first) = changes.first() else {
            return Err(RenderError::MusicalTimeMapEmpty);
        };
        if first.absolute_frame != 0 {
            return Err(RenderError::MusicalTimeMapMustStartAtZero);
        }
        if changes
            .iter()
            .any(|change| !change.tempo_bpm.is_finite() || change.tempo_bpm <= 0.0)
        {
            return Err(RenderError::InvalidTempo);
        }
        if changes
            .iter()
            .any(|change| !change.time_signature.is_valid())
        {
            return Err(RenderError::InvalidTimeSignature);
        }
        if changes
            .windows(2)
            .any(|window| window[0].absolute_frame >= window[1].absolute_frame)
        {
            return Err(RenderError::MusicalTimeMapNotSorted);
        }
        Ok(Self { changes })
    }

    /// Create a constant musical-time map.
    ///
    /// # Errors
    ///
    /// Returns an error when the tempo is non-positive or non-finite.
    pub fn constant(tempo_bpm: f64) -> Result<Self, RenderError> {
        Self::new(vec![MusicalTimeChange {
            absolute_frame: 0,
            tempo_bpm,
            time_signature: DEFAULT_TIME_SIGNATURE,
        }])
    }

    /// Return the musical-time changes in absolute-frame order.
    #[must_use]
    pub fn changes(&self) -> &[MusicalTimeChange] {
        &self.changes
    }

    #[cfg(test)]
    fn tempo_at(&self, absolute_frame: u64) -> f64 {
        let index = self
            .changes
            .partition_point(|change| change.absolute_frame <= absolute_frame)
            .saturating_sub(1);
        self.changes[index].tempo_bpm
    }

    #[cfg(test)]
    fn next_change_after(&self, absolute_frame: u64) -> Option<u64> {
        self.changes
            .iter()
            .find(|change| change.absolute_frame > absolute_frame)
            .map(|change| change.absolute_frame)
    }

    /// Prepare musical positions for one render sample rate.
    ///
    /// # Errors
    ///
    /// Returns an error when the sample rate is not finite and positive.
    pub fn prepare(&self, sample_rate: f64) -> Result<PreparedMusicalTimeMap, RenderError> {
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return Err(RenderError::InvalidSampleRate);
        }
        let mut segments = Vec::with_capacity(self.changes.len());
        let mut beat_position = 0.0;
        let mut bar_position = 0.0;
        for (index, change) in self.changes.iter().enumerate() {
            if let Some(previous) = self.changes.get(index.saturating_sub(1)) {
                let frame_delta = change
                    .absolute_frame
                    .saturating_sub(previous.absolute_frame);
                #[allow(clippy::cast_precision_loss)]
                let frame_delta = frame_delta as f64;
                let beat_delta = frame_delta * previous.tempo_bpm / (60.0 * sample_rate);
                beat_position += beat_delta;
                if previous.time_signature == change.time_signature {
                    bar_position += beat_delta / previous.time_signature.beats_per_bar();
                } else {
                    bar_position = (bar_position
                        + beat_delta / previous.time_signature.beats_per_bar())
                    .ceil();
                }
            }
            segments.push(PreparedMusicalTimeSegment {
                start_frame: change.absolute_frame,
                tempo_bpm: change.tempo_bpm,
                time_signature: change.time_signature,
                beat_position,
                bar_position,
            });
        }
        Ok(PreparedMusicalTimeMap {
            sample_rate,
            segments,
        })
    }
}

/// Prepared musical-time positions for one sample rate.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedMusicalTimeMap {
    sample_rate: f64,
    segments: Vec<PreparedMusicalTimeSegment>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PreparedMusicalTimeSegment {
    start_frame: u64,
    tempo_bpm: f64,
    time_signature: TimeSignature,
    beat_position: f64,
    bar_position: f64,
}

impl PreparedMusicalTimeMap {
    fn segment_at(&self, absolute_frame: u64) -> PreparedMusicalTimeSegment {
        let index = self
            .segments
            .partition_point(|segment| segment.start_frame <= absolute_frame)
            .saturating_sub(1);
        self.segments[index]
    }

    fn next_change_after(&self, absolute_frame: u64) -> Option<u64> {
        self.segments
            .iter()
            .find(|segment| segment.start_frame > absolute_frame)
            .map(|segment| segment.start_frame)
    }

    fn context_at(&self, absolute_frame: u64) -> ProcessContext {
        let segment = self.segment_at(absolute_frame);
        #[allow(clippy::cast_precision_loss)]
        let frame_delta = absolute_frame.saturating_sub(segment.start_frame) as f64;
        let beat_delta = frame_delta * segment.tempo_bpm / (60.0 * self.sample_rate);
        ProcessContext {
            absolute_frame,
            tempo_bpm: segment.tempo_bpm,
            beat_position: segment.beat_position + beat_delta,
            bar_position: segment.bar_position
                + beat_delta / segment.time_signature.beats_per_bar(),
            time_signature: segment.time_signature,
        }
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
    /// The render tempo is unsupported.
    #[error("tempo must be finite and greater than zero")]
    InvalidTempo,
    /// The time signature is unsupported.
    #[error("time signature is invalid")]
    InvalidTimeSignature,
    /// No musical-time change was supplied.
    #[error("musical time map must contain at least one change")]
    MusicalTimeMapEmpty,
    /// The first musical-time change must define the initial tempo and meter.
    #[error("musical time map must start at frame zero")]
    MusicalTimeMapMustStartAtZero,
    /// Musical-time changes must be strictly ordered by absolute frame.
    #[error("musical time map changes must be strictly ordered")]
    MusicalTimeMapNotSorted,
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
    /// A trace interval is zero.
    #[error("trace interval must be greater than zero")]
    TraceIntervalInvalid,
    /// A trace target is not present in the compiled parameter catalog.
    #[error("trace parameter handle {handle} is not present in the compiled catalog")]
    TraceParameterInvalid {
        /// Invalid dense parameter handle.
        handle: usize,
    },
    /// A trace would retain more observations than the diagnostic safety limit.
    #[error("trace would exceed the observation limit of {limit} records")]
    TraceLimitExceeded {
        /// Estimated or actual observation count.
        estimated: usize,
        /// Maximum allowed records.
        limit: usize,
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
    render_instrument_with_tempo(compiled, request, events, DEFAULT_TEMPO_BPM)
}

/// Render a compiled instrument at one constant tempo.
///
/// # Errors
///
/// Returns an error when the tempo, request, event timeline, or instrument runtime is invalid.
pub fn render_instrument_with_tempo(
    compiled: Arc<CompiledInstrument>,
    request: RenderRequest,
    events: &[ScheduledEvent],
    tempo_bpm: f64,
) -> Result<RenderedAudio, RenderError> {
    let mut runtime = InstrumentRuntime::new(compiled);
    let musical_time_map = MusicalTimeMap::constant(tempo_bpm)?;
    render_processor_with_musical_time_map(&mut runtime, request, events, &musical_time_map)
}

/// Render a compiled instrument with an absolute-frame tempo map.
///
/// # Errors
///
/// Returns an error when the request, tempo map, event timeline, or instrument runtime is
/// invalid.
pub fn render_instrument_with_musical_time_map(
    compiled: Arc<CompiledInstrument>,
    request: RenderRequest,
    events: &[ScheduledEvent],
    musical_time_map: &MusicalTimeMap,
) -> Result<RenderedAudio, RenderError> {
    let mut runtime = InstrumentRuntime::new(compiled);
    render_processor_with_musical_time_map(&mut runtime, request, events, musical_time_map)
}

/// Render an instrument, reset the same prepared runtime, and render the same events again.
///
/// This is intended for deterministic runtime diagnostics. Both renders use the same prepared
/// allocation and the second render begins only after [`InstrumentProcessor::reset`] succeeds.
///
/// # Errors
///
/// Returns an error when the request, event timeline, preparation, reset, or either render is
/// invalid.
pub fn render_instrument_with_reset(
    compiled: Arc<CompiledInstrument>,
    request: RenderRequest,
    events: &[ScheduledEvent],
    musical_time_map: &MusicalTimeMap,
) -> Result<(RenderedAudio, RenderedAudio), RenderError> {
    let total_frames_usize = validate_render_inputs(request, events)?;
    let spec = request.process_spec()?;
    let mut runtime = InstrumentRuntime::new(compiled);
    runtime.prepare(spec)?;
    let mut observe =
        |_frame: u64, _runtime: &mut InstrumentRuntime, _context: ProcessContext| Ok(());
    let first = render_prepared_processor_with_musical_time_map_observed(
        &mut runtime,
        request,
        events,
        musical_time_map,
        &[],
        &mut observe,
        total_frames_usize,
    )?;
    runtime.reset()?;
    let second = render_prepared_processor_with_musical_time_map_observed(
        &mut runtime,
        request,
        events,
        musical_time_map,
        &[],
        &mut observe,
        total_frames_usize,
    )?;
    Ok((first, second))
}

/// Render a compiled instrument and collect selected runtime parameter observations.
///
/// The observation boundaries are inserted into the offline process loop. The same runtime and
/// event ordering are used as an ordinary render, so the trace is diagnostic output rather than
/// an alternate audio path.
///
/// # Errors
///
/// Returns an error when the request, trace selection, event timeline, or instrument runtime is
/// invalid.
#[allow(clippy::needless_pass_by_value)]
pub fn render_instrument_with_trace(
    compiled: Arc<CompiledInstrument>,
    request: RenderRequest,
    events: &[ScheduledEvent],
    musical_time_map: &MusicalTimeMap,
    trace_request: &TraceRequest,
) -> Result<(RenderedAudio, RenderTraceReport), RenderError> {
    if trace_request.every_frames == 0 {
        return Err(RenderError::TraceIntervalInvalid);
    }
    for handle in &trace_request.parameters {
        if compiled.parameter_descriptor(*handle).is_none() {
            return Err(RenderError::TraceParameterInvalid {
                handle: handle.index(),
            });
        }
    }
    let total_frames = request.total_frames()?;
    if trace_request.parameters.is_empty() {
        let mut runtime = InstrumentRuntime::new(Arc::clone(&compiled));
        let audio = render_processor_with_musical_time_map(
            &mut runtime,
            request,
            events,
            musical_time_map,
        )?;
        return Ok((
            audio,
            TraceCollector::new(trace_request, &compiled).finish(),
        ));
    }
    let latency_frames = u64::try_from(compiled.reported_latency_frames)
        .map_err(|_| RenderError::FrameCountOverflow)?;
    // Audio is rendered through the latency-extended request, while Trace follows the runtime's
    // performance timeline before the generator output reaches the delayed audio frame.
    let trace_frames = total_frames.saturating_sub(latency_frames);
    let boundary_count = trace_boundary_count(trace_frames, trace_request.every_frames, events);
    let estimate = boundary_count
        .saturating_mul(trace_request.parameters.len())
        .saturating_mul(compiled.performance.voice_count.max(1));
    if estimate > MAX_TRACE_OBSERVATIONS {
        return Err(RenderError::TraceLimitExceeded {
            estimated: estimate,
            limit: MAX_TRACE_OBSERVATIONS,
        });
    }
    let mut boundaries = Vec::with_capacity(boundary_count);
    if trace_frames > 0 {
        boundaries.push(0);
        let every_frames = u64::try_from(trace_request.every_frames)
            .map_err(|_| RenderError::FrameCountOverflow)?;
        let mut periodic = every_frames;
        while periodic < trace_frames {
            boundaries.push(periodic);
            periodic = periodic
                .checked_add(every_frames)
                .ok_or(RenderError::FrameCountOverflow)?;
        }
        for event in events {
            if event.absolute_frame < trace_frames
                && let Some(frame) = event.absolute_frame.checked_add(1)
                && frame <= trace_frames
            {
                boundaries.push(frame);
            }
        }
        boundaries.push(trace_frames);
        boundaries.sort_unstable();
        boundaries.dedup();
    }
    let mut runtime = InstrumentRuntime::new(Arc::clone(&compiled));
    let mut collector = TraceCollector::new(trace_request, &compiled);
    let mut observe = |frame: u64, runtime: &mut InstrumentRuntime, context: ProcessContext| {
        collector
            .observe(runtime, frame, context)
            .map_err(|error| match error {
                TraceCollectError::Process(error) => RenderError::Process(error),
                TraceCollectError::LimitExceeded { observed, limit } => {
                    RenderError::TraceLimitExceeded {
                        estimated: observed,
                        limit,
                    }
                }
            })
    };
    let audio = render_processor_with_musical_time_map_observed(
        &mut runtime,
        request,
        events,
        musical_time_map,
        &boundaries,
        &mut observe,
    )?;
    Ok((audio, collector.finish()))
}

fn trace_boundary_count(
    public_frames: u64,
    every_frames: usize,
    events: &[ScheduledEvent],
) -> usize {
    let periodic_count = if public_frames == 0 {
        0
    } else {
        let every_frames = u64::try_from(every_frames).unwrap_or(u64::MAX);
        (public_frames - 1) / every_frames
    };
    let event_count = events
        .iter()
        .filter(|event| {
            event.absolute_frame < public_frames
                && event
                    .absolute_frame
                    .checked_add(1)
                    .is_some_and(|frame| frame <= public_frames)
        })
        .count();
    let fixed_count = if public_frames > 0 { 2 } else { 0 };
    let count = periodic_count
        .saturating_add(u64::try_from(event_count).unwrap_or(u64::MAX))
        .saturating_add(fixed_count);
    usize::try_from(count).unwrap_or(usize::MAX)
}

fn render_processor<P: InstrumentProcessor>(
    processor: &mut P,
    request: RenderRequest,
) -> Result<RenderedAudio, RenderError> {
    let musical_time_map = MusicalTimeMap::constant(DEFAULT_TEMPO_BPM)?;
    render_processor_with_musical_time_map(processor, request, &[], &musical_time_map)
}

fn render_processor_with_musical_time_map<P: InstrumentProcessor>(
    processor: &mut P,
    request: RenderRequest,
    events: &[ScheduledEvent],
    musical_time_map: &MusicalTimeMap,
) -> Result<RenderedAudio, RenderError> {
    let mut observe = |_frame: u64, _processor: &mut P, _context: ProcessContext| Ok(());
    render_processor_with_musical_time_map_observed(
        processor,
        request,
        events,
        musical_time_map,
        &[],
        &mut observe,
    )
}

fn render_processor_with_musical_time_map_observed<P, F>(
    processor: &mut P,
    request: RenderRequest,
    events: &[ScheduledEvent],
    musical_time_map: &MusicalTimeMap,
    observation_boundaries: &[u64],
    observe: &mut F,
) -> Result<RenderedAudio, RenderError>
where
    P: InstrumentProcessor,
    F: FnMut(u64, &mut P, ProcessContext) -> Result<(), RenderError>,
{
    let total_frames_usize = validate_render_inputs(request, events)?;
    let spec = request.process_spec()?;
    processor.prepare(spec)?;
    render_prepared_processor_with_musical_time_map_observed(
        processor,
        request,
        events,
        musical_time_map,
        observation_boundaries,
        observe,
        total_frames_usize,
    )
}

fn render_prepared_processor_with_musical_time_map_observed<P, F>(
    processor: &mut P,
    request: RenderRequest,
    events: &[ScheduledEvent],
    musical_time_map: &MusicalTimeMap,
    observation_boundaries: &[u64],
    observe: &mut F,
    total_frames_usize: usize,
) -> Result<RenderedAudio, RenderError>
where
    P: InstrumentProcessor,
    F: FnMut(u64, &mut P, ProcessContext) -> Result<(), RenderError>,
{
    let timeline = musical_time_map.prepare(request.sample_rate)?;
    let mut observation_index = 0_usize;
    if observation_boundaries.first() == Some(&0) {
        observe(0, processor, timeline.context_at(0))?;
        observation_index = 1;
    }

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
        let next_observation = observation_boundaries.get(observation_index).copied();
        let next_tempo_frame = timeline
            .next_change_after(u64::try_from(offset).map_err(|_| RenderError::FrameCountOverflow)?)
            .and_then(|frame| usize::try_from(frame).ok())
            .unwrap_or(total_frames_usize);
        let frames = (total_frames_usize - offset)
            .min(request.block_size)
            .min(next_tempo_frame.saturating_sub(offset))
            .min(
                next_observation.map_or(total_frames_usize - offset, |boundary| {
                    usize::try_from(boundary)
                        .ok()
                        .map_or(0, |boundary| boundary.saturating_sub(offset))
                }),
            );
        if frames == 0 {
            return Err(RenderError::FrameCountOverflow);
        }
        let end = offset + frames;
        block_events.clear();
        let end_frame = u64::try_from(end).map_err(|_| RenderError::FrameCountOverflow)?;
        while event_index < events.len() && events[event_index].absolute_frame < end_frame {
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
            let context = timeline
                .context_at(u64::try_from(offset).map_err(|_| RenderError::FrameCountOverflow)?);
            let block = ProcessBlock {
                frames,
                context,
                events: &block_events,
                output: &mut output,
            };
            processor.process(block)?;
        }
        offset = end;
        if next_observation == Some(end as u64) {
            observe(end as u64, processor, timeline.context_at(end as u64))?;
            observation_index += 1;
        }
    }
    Ok(audio)
}

fn validate_render_inputs(
    request: RenderRequest,
    events: &[ScheduledEvent],
) -> Result<usize, RenderError> {
    let total_frames = request.total_frames()?;
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
    Ok(total_frames_usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{DiagnosticCode, DiagnosticSeverity, from_render_error};

    struct FailingProcessor {
        calls: usize,
    }

    struct TempoRecordingProcessor {
        blocks: Vec<(u64, usize, f64)>,
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

    impl InstrumentProcessor for TempoRecordingProcessor {
        fn prepare(&mut self, _spec: ProcessSpec) -> Result<(), ProcessError> {
            Ok(())
        }

        fn process(&mut self, block: ProcessBlock<'_>) -> Result<(), ProcessError> {
            self.blocks.push((
                block.context.absolute_frame,
                block.frames,
                block.context.tempo_bpm,
            ));
            Ok(())
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
    fn tempo_boundaries_split_process_contexts() {
        let musical_time_map = MusicalTimeMap::new(vec![
            MusicalTimeChange {
                absolute_frame: 0,
                tempo_bpm: 120.0,
                time_signature: DEFAULT_TIME_SIGNATURE,
            },
            MusicalTimeChange {
                absolute_frame: 32,
                tempo_bpm: 90.0,
                time_signature: DEFAULT_TIME_SIGNATURE,
            },
            MusicalTimeChange {
                absolute_frame: 80,
                tempo_bpm: 60.0,
                time_signature: DEFAULT_TIME_SIGNATURE,
            },
        ])
        .expect("tempo map");
        let mut processor = TempoRecordingProcessor { blocks: Vec::new() };

        render_processor_with_musical_time_map(
            &mut processor,
            request(96, 0, 64),
            &[],
            &musical_time_map,
        )
        .expect("tempo-aware render");

        assert_eq!(processor.blocks.len(), 3);
        assert_eq!((processor.blocks[0].0, processor.blocks[0].1), (0, 32));
        assert_eq!((processor.blocks[1].0, processor.blocks[1].1), (32, 48));
        assert_eq!((processor.blocks[2].0, processor.blocks[2].1), (80, 16));
        assert!((processor.blocks[0].2 - 120.0).abs() < f64::EPSILON);
        assert!((processor.blocks[1].2 - 90.0).abs() < f64::EPSILON);
        assert!((processor.blocks[2].2 - 60.0).abs() < f64::EPSILON);
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

    #[test]
    fn musical_time_map_requires_valid_ordered_changes() {
        assert_eq!(
            MusicalTimeMap::new(Vec::new()),
            Err(RenderError::MusicalTimeMapEmpty)
        );
        assert_eq!(
            MusicalTimeMap::new(vec![MusicalTimeChange {
                absolute_frame: 1,
                tempo_bpm: 120.0,
                time_signature: DEFAULT_TIME_SIGNATURE,
            }]),
            Err(RenderError::MusicalTimeMapMustStartAtZero)
        );
        assert_eq!(
            MusicalTimeMap::new(vec![
                MusicalTimeChange {
                    absolute_frame: 0,
                    tempo_bpm: 120.0,
                    time_signature: DEFAULT_TIME_SIGNATURE,
                },
                MusicalTimeChange {
                    absolute_frame: 0,
                    tempo_bpm: 90.0,
                    time_signature: DEFAULT_TIME_SIGNATURE,
                },
            ]),
            Err(RenderError::MusicalTimeMapNotSorted)
        );
        assert_eq!(
            MusicalTimeMap::constant(f64::NAN),
            Err(RenderError::InvalidTempo)
        );
    }

    #[test]
    fn musical_time_map_exposes_the_tempo_at_each_boundary() {
        let map = MusicalTimeMap::new(vec![
            MusicalTimeChange {
                absolute_frame: 0,
                tempo_bpm: 120.0,
                time_signature: DEFAULT_TIME_SIGNATURE,
            },
            MusicalTimeChange {
                absolute_frame: 32,
                tempo_bpm: 90.0,
                time_signature: DEFAULT_TIME_SIGNATURE,
            },
        ])
        .expect("tempo map");
        assert_eq!(map.changes().len(), 2);
        assert!((map.tempo_at(0) - 120.0).abs() < f64::EPSILON);
        assert!((map.tempo_at(31) - 120.0).abs() < f64::EPSILON);
        assert!((map.tempo_at(32) - 90.0).abs() < f64::EPSILON);
        assert_eq!(map.next_change_after(0), Some(32));
        assert_eq!(map.next_change_after(32), None);
    }

    #[test]
    fn prepared_musical_time_tracks_beats_and_resets_bars_on_meter_change() {
        let map = MusicalTimeMap::new(vec![
            MusicalTimeChange {
                absolute_frame: 0,
                tempo_bpm: 120.0,
                time_signature: TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
            },
            MusicalTimeChange {
                absolute_frame: 48_000,
                tempo_bpm: 120.0,
                time_signature: TimeSignature {
                    numerator: 3,
                    denominator: 4,
                },
            },
        ])
        .expect("valid musical-time map");
        let prepared = map.prepare(48_000.0).expect("prepared map");

        let before_change = prepared.context_at(47_999);
        let at_change = prepared.context_at(48_000);
        let later = prepared.context_at(72_000);

        assert!((before_change.beat_position - 1.999_958_333_333_333_3).abs() < 1e-12);
        assert!((at_change.beat_position - 2.0).abs() < f64::EPSILON);
        assert!((at_change.bar_position - 1.0).abs() < f64::EPSILON);
        assert_eq!(at_change.time_signature.denominator, 4);
        assert!((later.beat_position - 3.0).abs() < f64::EPSILON);
        assert!((later.bar_position - (4.0 / 3.0)).abs() < 1e-12);
    }

    #[test]
    fn musical_time_map_rejects_invalid_meter() {
        assert_eq!(
            MusicalTimeMap::new(vec![MusicalTimeChange {
                absolute_frame: 0,
                tempo_bpm: 120.0,
                time_signature: TimeSignature {
                    numerator: 4,
                    denominator: 3,
                },
            }]),
            Err(RenderError::InvalidTimeSignature)
        );
    }
}
