use std::sync::Arc;

use crate::compiler::{CompiledGenerator, CompiledInstrument};
use crate::process::{
    InstrumentProcessor, ProcessBlock, ProcessError, ProcessEventKind, ProcessSpec, clear_output,
};

use super::modulation::{ParameterSpanValue, SharedParameterSpan, ValueSpan};
use super::smoothing::{Smoother, rounded_frame_count};
use super::voice::{NoteRequest, VoiceRuntime, VoiceState};

const CONTROL_SMOOTHING_SECONDS: f64 = 0.005;
const STEAL_FADE_SECONDS: f64 = 0.005;
const QUANTUM_FRAMES: usize = 32;

struct RuntimeScratch {
    layer_mono: Vec<f32>,
    voice_left: Vec<f32>,
    voice_right: Vec<f32>,
    parameter_spans: Vec<ParameterSpanValue>,
}

/// Prepared polyphonic runtime for one immutable compiled instrument.
pub struct InstrumentRuntime {
    compiled: Arc<CompiledInstrument>,
    voices: Vec<VoiceRuntime>,
    scratch: RuntimeScratch,
    parameter_states: Vec<Smoother>,
    pitch_bend: Smoother,
    mod_wheel: Smoother,
    aftertouch: Smoother,
    spec: Option<ProcessSpec>,
    absolute_frame: u64,
}

impl InstrumentRuntime {
    /// Create an unprepared runtime from a compiled instrument.
    #[must_use]
    pub fn new(compiled: Arc<CompiledInstrument>) -> Self {
        Self {
            compiled,
            voices: Vec::new(),
            scratch: RuntimeScratch {
                layer_mono: Vec::new(),
                voice_left: Vec::new(),
                voice_right: Vec::new(),
                parameter_spans: Vec::new(),
            },
            parameter_states: Vec::new(),
            pitch_bend: Smoother::new(0.0),
            mod_wheel: Smoother::new(0.0),
            aftertouch: Smoother::new(0.0),
            spec: None,
            absolute_frame: 0,
        }
    }

    /// Return the immutable compiled configuration used by this runtime.
    #[must_use]
    pub fn compiled(&self) -> &Arc<CompiledInstrument> {
        &self.compiled
    }

    /// Return the number of prepared voices.
    #[must_use]
    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }

    /// Return the runtime's absolute frame position.
    #[must_use]
    pub fn absolute_frame(&self) -> u64 {
        self.absolute_frame
    }

    /// Return one voice state for diagnostics and tests.
    #[must_use]
    pub fn voice_state(&self, index: usize) -> Option<VoiceState> {
        self.voices.get(index).map(VoiceRuntime::state)
    }

    fn render_range(
        voices: &mut [VoiceRuntime],
        compiled: &CompiledInstrument,
        layer_mono: &mut [f32],
        voice_left: &mut [f32],
        voice_right: &mut [f32],
        output: &mut [&mut [f32]],
        start: usize,
        end: usize,
        sample_rate: f64,
        shared: SharedParameterSpan<'_>,
    ) -> Result<(), ProcessError> {
        if start >= end {
            return Ok(());
        }
        let frames = end - start;
        let (left_channels, right_channels) = output.split_at_mut(1);
        let left = &mut left_channels[0][start..end];
        let right = &mut right_channels[0][start..end];
        for voice in voices {
            voice.render_span(
                frames,
                sample_rate,
                compiled,
                shared,
                &mut layer_mono[..frames],
                &mut voice_left[..frames],
                &mut voice_right[..frames],
            )?;
            for index in 0..frames {
                left[index] += voice_left[index];
                right[index] += voice_right[index];
            }
        }
        Ok(())
    }

    fn apply_event(
        &mut self,
        event: ProcessEventKind,
        absolute_frame: u64,
    ) -> Result<(), ProcessError> {
        let spec = self.spec.ok_or(ProcessError::NotPrepared)?;
        match event {
            ProcessEventKind::NoteOn {
                note_id,
                note_number,
                velocity,
            } => {
                let can_trigger = self.compiled.layers.iter().any(|layer| {
                    if !layer.trigger.matches(note_number, velocity) {
                        return false;
                    }
                    match &layer.generator {
                        CompiledGenerator::Oscillator(_) => true,
                        CompiledGenerator::Sample(sample) => {
                            sample.enabled && sample.source.is_some()
                        }
                    }
                });
                if !can_trigger {
                    return Ok(());
                }
                let voice_index = self.select_voice();
                let fade_frames = rounded_frame_count(spec.sample_rate * STEAL_FADE_SECONDS);
                self.voices[voice_index].request_note(
                    NoteRequest::new(note_id, note_number, velocity, absolute_frame),
                    fade_frames,
                    &self.compiled,
                )?;
            }
            ProcessEventKind::NoteOff { note_id } => {
                for voice in &mut self.voices {
                    voice.release_note(note_id);
                }
            }
            ProcessEventKind::ParameterChange {
                parameter,
                normalized,
            } => {
                let descriptor = self.compiled.parameter_descriptor(parameter).ok_or(
                    ProcessError::ParameterHandleOutOfRange {
                        handle: parameter.index(),
                    },
                )?;
                let frames =
                    rounded_frame_count(f64::from(descriptor.smoothing_seconds) * spec.sample_rate)
                        .max(1);
                self.parameter_states[parameter.index()].set_target(normalized, frames);
            }
            ProcessEventKind::PitchBend { value } => {
                self.pitch_bend
                    .set_target(value, control_smoothing_frames(spec.sample_rate));
            }
            ProcessEventKind::ModWheel { value } => {
                self.mod_wheel
                    .set_target(value, control_smoothing_frames(spec.sample_rate));
            }
            ProcessEventKind::Aftertouch { value } => {
                self.aftertouch
                    .set_target(value, control_smoothing_frames(spec.sample_rate));
            }
        }
        Ok(())
    }

    fn select_voice(&self) -> usize {
        if let Some((index, _)) = self
            .voices
            .iter()
            .enumerate()
            .find(|(_, voice)| voice.state() == VoiceState::Idle)
        {
            return index;
        }
        if let Some((index, _)) = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, voice)| voice.state() == VoiceState::Releasing)
            .min_by(|(_, left), (_, right)| {
                left.estimated_level().total_cmp(&right.estimated_level())
            })
        {
            return index;
        }
        if let Some((index, _)) = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, voice)| voice.state() == VoiceState::Active)
            .min_by_key(|(_, voice)| voice.started_at_frame())
        {
            return index;
        }
        self.voices
            .iter()
            .enumerate()
            .min_by_key(|(_, voice)| voice.started_at_frame())
            .map_or(0, |(index, _)| index)
    }

    fn validate_parameter_events(
        &self,
        events: &[crate::process::ProcessEvent],
    ) -> Result<(), ProcessError> {
        for event in events {
            if let ProcessEventKind::ParameterChange { parameter, .. } = event.kind
                && self.compiled.parameter_descriptor(parameter).is_none()
            {
                return Err(ProcessError::ParameterHandleOutOfRange {
                    handle: parameter.index(),
                });
            }
        }
        Ok(())
    }

    fn shared_target_remaining(&self) -> Option<usize> {
        let mut remaining = self
            .parameter_states
            .iter()
            .filter_map(Smoother::frames_until_target)
            .min();
        for control in [
            self.pitch_bend.frames_until_target(),
            self.mod_wheel.frames_until_target(),
            self.aftertouch.frames_until_target(),
        ]
        .into_iter()
        .flatten()
        {
            remaining = Some(remaining.map_or(control, |value| value.min(control)));
        }
        remaining
    }

    fn advance_shared(&mut self, frames: usize) -> (ValueSpan, ValueSpan, ValueSpan) {
        for (state, span) in self
            .parameter_states
            .iter_mut()
            .zip(&mut self.scratch.parameter_spans)
        {
            let (start, end) = state.span(frames);
            *span = ParameterSpanValue { start, end };
        }
        let (pitch_start, pitch_end) = self.pitch_bend.span(frames);
        let (wheel_start, wheel_end) = self.mod_wheel.span(frames);
        let (touch_start, touch_end) = self.aftertouch.span(frames);
        (
            ValueSpan {
                start: pitch_start,
                end: pitch_end,
            },
            ValueSpan {
                start: wheel_start,
                end: wheel_end,
            },
            ValueSpan {
                start: touch_start,
                end: touch_end,
            },
        )
    }
}

impl InstrumentProcessor for InstrumentRuntime {
    fn prepare(&mut self, spec: ProcessSpec) -> Result<(), ProcessError> {
        self.spec = None;
        self.voices.clear();
        self.scratch.layer_mono.clear();
        self.scratch.voice_left.clear();
        self.scratch.voice_right.clear();
        self.scratch.parameter_spans.clear();
        self.parameter_states.clear();
        self.pitch_bend.reset(0.0);
        self.mod_wheel.reset(0.0);
        self.aftertouch.reset(0.0);
        self.absolute_frame = 0;

        spec.validate()?;
        if spec
            .sample_rate
            .total_cmp(&self.compiled.process_sample_rate)
            != std::cmp::Ordering::Equal
        {
            return Err(ProcessError::SampleRateMismatch {
                compiled: self.compiled.process_sample_rate,
                requested: spec.sample_rate,
            });
        }
        self.scratch.layer_mono.resize(spec.max_block_size, 0.0);
        self.scratch.voice_left.resize(spec.max_block_size, 0.0);
        self.scratch.voice_right.resize(spec.max_block_size, 0.0);
        self.scratch.parameter_spans.resize(
            self.compiled.parameters().len(),
            ParameterSpanValue {
                start: 0.0,
                end: 0.0,
            },
        );
        self.parameter_states = self
            .compiled
            .parameters()
            .iter()
            .map(|descriptor| {
                let normalized = descriptor.normalize(descriptor.default).unwrap_or(0.0);
                Smoother::new(normalized)
            })
            .collect();
        let mut voices = Vec::with_capacity(self.compiled.performance.polyphony);
        for _ in 0..self.compiled.performance.polyphony {
            voices.push(VoiceRuntime::new(&self.compiled, spec)?);
        }
        self.voices = voices;
        self.spec = Some(spec);
        Ok(())
    }

    fn process(&mut self, block: ProcessBlock<'_>) -> Result<(), ProcessError> {
        clear_output(&mut *block.output, block.frames);
        let spec = self.spec.ok_or(ProcessError::NotPrepared)?;
        block.validate_for(spec)?;
        self.validate_parameter_events(block.events)?;
        if block.context.absolute_frame != self.absolute_frame {
            return Err(ProcessError::ContextDiscontinuity {
                received: block.context.absolute_frame,
                expected: self.absolute_frame,
            });
        }
        let next_frame = self
            .absolute_frame
            .checked_add(block.frames as u64)
            .ok_or(ProcessError::FrameOverflow)?;
        if block.frames == 0 {
            self.absolute_frame = next_frame;
            return Ok(());
        }

        let mut cursor = 0;
        let mut event_index = 0;
        while cursor < block.frames {
            while event_index < block.events.len()
                && block.events[event_index].sample_offset == cursor
            {
                let event = block.events[event_index].kind;
                self.apply_event(event, self.absolute_frame + cursor as u64)?;
                event_index += 1;
            }

            let mut end = block.frames;
            if let Some(next_event) = block.events.get(event_index) {
                end = end.min(next_event.sample_offset);
            }
            let absolute = self.absolute_frame + cursor as u64;
            let quantum = QUANTUM_FRAMES - (absolute as usize % QUANTUM_FRAMES);
            end = end.min(cursor + quantum);
            if let Some(remaining) = self.shared_target_remaining() {
                end = end.min(cursor + remaining);
            }
            if end <= cursor {
                end = cursor + 1;
            }

            let frames = end - cursor;
            let (pitch_bend, mod_wheel, aftertouch) = self.advance_shared(frames);
            let RuntimeScratch {
                layer_mono,
                voice_left,
                voice_right,
                parameter_spans,
            } = &mut self.scratch;
            let shared = SharedParameterSpan::new(
                &*parameter_spans,
                pitch_bend,
                mod_wheel,
                aftertouch,
                frames,
            );
            Self::render_range(
                &mut self.voices,
                &self.compiled,
                layer_mono,
                voice_left,
                voice_right,
                block.output,
                cursor,
                end,
                spec.sample_rate,
                shared,
            )?;
            cursor = end;
        }
        self.absolute_frame = next_frame;
        Ok(())
    }

    fn reset(&mut self) -> Result<(), ProcessError> {
        if self.spec.is_none() {
            return Err(ProcessError::NotPrepared);
        }
        for voice in &mut self.voices {
            voice.reset()?;
        }
        for (state, descriptor) in self
            .parameter_states
            .iter_mut()
            .zip(self.compiled.parameters())
        {
            let normalized = descriptor.normalize(descriptor.default).unwrap_or(0.0);
            state.reset(normalized);
        }
        self.pitch_bend.reset(0.0);
        self.mod_wheel.reset(0.0);
        self.aftertouch.reset(0.0);
        self.scratch.layer_mono.fill(0.0);
        self.scratch.voice_left.fill(0.0);
        self.scratch.voice_right.fill(0.0);
        self.scratch.parameter_spans.fill(ParameterSpanValue {
            start: 0.0,
            end: 0.0,
        });
        self.absolute_frame = 0;
        Ok(())
    }
}

fn control_smoothing_frames(sample_rate: f64) -> usize {
    rounded_frame_count(sample_rate * CONTROL_SMOOTHING_SECONDS).max(1)
}
