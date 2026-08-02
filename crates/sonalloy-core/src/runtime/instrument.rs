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

    #[allow(clippy::too_many_arguments)]
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
            if voice.state() == VoiceState::Idle {
                continue;
            }
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
                descriptor
                    .normalize(descriptor.default)
                    .map(Smoother::new)
                    .map_err(|_| ProcessError::InvalidCompiledParameterDefault)
            })
            .collect::<Result<Vec<_>, _>>()?;
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
                if let Err(error) = self.apply_event(event, self.absolute_frame + cursor as u64) {
                    clear_output(&mut *block.output, block.frames);
                    return Err(error);
                }
                event_index += 1;
            }

            let mut end = block.frames;
            if let Some(next_event) = block.events.get(event_index) {
                end = end.min(next_event.sample_offset);
            }
            let absolute = self.absolute_frame + cursor as u64;
            let absolute_frame =
                usize::try_from(absolute).map_err(|_| ProcessError::FrameOverflow)?;
            let quantum = QUANTUM_FRAMES - (absolute_frame % QUANTUM_FRAMES);
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
            if let Err(error) = Self::render_range(
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
            ) {
                clear_output(&mut *block.output, block.frames);
                return Err(error);
            }
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
            let normalized = descriptor
                .normalize(descriptor.default)
                .map_err(|_| ProcessError::InvalidCompiledParameterDefault)?;
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

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use super::*;
    use crate::compiler::{CompileContext, compile_instrument};
    use crate::definition::tests::definition;
    use crate::parameter::ParameterHandle;
    use crate::process::ProcessEvent;

    fn compiled(polyphony: u16) -> Arc<CompiledInstrument> {
        let mut definition = definition();
        definition.performance.polyphony = polyphony;
        compile_instrument(
            &definition,
            &CompileContext {
                definition_base_dir: ".".into(),
                process_spec: ProcessSpec::new(48_000.0, 64, 2).expect("valid spec"),
            },
        )
        .instrument
        .expect("definition compiles")
    }

    fn runtime_with(definition: &crate::definition::InstrumentDefinition) -> InstrumentRuntime {
        let result = compile_instrument(
            definition,
            &CompileContext {
                definition_base_dir: ".".into(),
                process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid spec"),
            },
        );
        result.instrument.expect("compiled").instantiate()
    }

    fn runtime() -> InstrumentRuntime {
        runtime_with(&definition())
    }

    fn prepare(runtime: &mut InstrumentRuntime) {
        runtime
            .prepare(ProcessSpec::new(48_000.0, 257, 2).expect("valid spec"))
            .expect("prepare");
    }

    fn process(
        runtime: &mut InstrumentRuntime,
        frames: usize,
        absolute_frame: u64,
        events: &[ProcessEvent],
    ) -> Vec<Vec<f32>> {
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        runtime
            .process(ProcessBlock {
                frames,
                context: crate::process::ProcessContext {
                    absolute_frame,
                    tempo_bpm: 120.0,
                },
                events,
                output: &mut output,
            })
            .expect("process succeeds");
        vec![left, right]
    }

    #[test]
    fn note_lifecycle_produces_stereo_audio_and_release() {
        let mut runtime = runtime();
        prepare(&mut runtime);
        let on = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 69,
                velocity: 100,
            },
        }];
        let audio = process(&mut runtime, 128, 0, &on);
        assert!(audio.iter().flatten().any(|sample| sample.abs() > 0.01));
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));
        let off = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        }];
        let _ = process(&mut runtime, 256, 128, &off);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Releasing));
    }

    #[test]
    fn prepare_requires_the_compiled_sample_rate_but_allows_block_size_changes() {
        let mut runtime = runtime();
        runtime
            .prepare(ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec"))
            .expect("matching sample rate and changed block size are valid");
    }

    #[test]
    fn prepare_rejects_a_sample_rate_different_from_compile_time() {
        let mut runtime = runtime();
        let error = runtime
            .prepare(ProcessSpec::new(44_100.0, 257, 2).expect("valid process spec"))
            .expect_err("a compiled instrument cannot be prepared at another sample rate");
        assert_eq!(
            error,
            ProcessError::SampleRateMismatch {
                compiled: 48_000.0,
                requested: 44_100.0,
            }
        );
    }

    #[test]
    fn failed_sample_rate_prepare_invalidates_previous_runtime_and_allows_reprepare() {
        let mut runtime = runtime();
        let valid_spec = ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec");
        runtime
            .prepare(valid_spec)
            .expect("initial runtime preparation");
        assert!(runtime.voice_count() > 0);

        assert_eq!(
            runtime.prepare(ProcessSpec::new(44_100.0, 64, 2).expect("valid process spec")),
            Err(ProcessError::SampleRateMismatch {
                compiled: 48_000.0,
                requested: 44_100.0,
            })
        );
        assert_eq!(runtime.voice_count(), 0);
        assert_eq!(runtime.absolute_frame(), 0);

        let mut left = [1.0_f32; 8];
        let mut right = [1.0_f32; 8];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        assert_eq!(
            runtime.process(ProcessBlock {
                frames: 8,
                context: crate::process::ProcessContext {
                    absolute_frame: 0,
                    tempo_bpm: 120.0,
                },
                events: &[],
                output: &mut output,
            }),
            Err(ProcessError::NotPrepared)
        );
        assert!(left.iter().all(|sample| sample.abs() < f32::EPSILON));
        assert!(right.iter().all(|sample| sample.abs() < f32::EPSILON));

        runtime.prepare(valid_spec).expect("runtime re-preparation");
        let _ = process(&mut runtime, 8, 0, &[]);
    }

    #[test]
    fn invalid_prepare_invalidates_previous_runtime_and_allows_reprepare() {
        let mut runtime = runtime();
        let valid_spec = ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec");
        runtime
            .prepare(valid_spec)
            .expect("initial runtime preparation");

        let invalid_spec = ProcessSpec {
            sample_rate: 48_000.0,
            max_block_size: 0,
            output_channels: 2,
        };
        assert_eq!(
            runtime.prepare(invalid_spec),
            Err(ProcessError::InvalidMaxBlockSize)
        );
        assert_eq!(runtime.voice_count(), 0);
        assert_eq!(runtime.absolute_frame(), 0);

        let mut left = [1.0_f32; 8];
        let mut right = [1.0_f32; 8];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        assert_eq!(
            runtime.process(ProcessBlock {
                frames: 8,
                context: crate::process::ProcessContext {
                    absolute_frame: 0,
                    tempo_bpm: 120.0,
                },
                events: &[],
                output: &mut output,
            }),
            Err(ProcessError::NotPrepared)
        );
        assert!(left.iter().all(|sample| sample.abs() < f32::EPSILON));
        assert!(right.iter().all(|sample| sample.abs() < f32::EPSILON));

        runtime.prepare(valid_spec).expect("runtime re-preparation");
        let _ = process(&mut runtime, 8, 0, &[]);
    }

    #[test]
    fn prepare_accepts_an_instrument_compiled_at_44_1_khz() {
        let result = compile_instrument(
            &definition(),
            &CompileContext {
                definition_base_dir: ".".into(),
                process_spec: ProcessSpec::new(44_100.0, 257, 2).expect("valid spec"),
            },
        );
        let mut runtime = result
            .instrument
            .expect("compiled instrument")
            .instantiate();
        runtime
            .prepare(ProcessSpec::new(44_100.0, 257, 2).expect("valid spec"))
            .expect("matching 44.1 kHz sample rate is valid");
    }

    #[test]
    fn note_timing_is_independent_of_block_size() {
        let mut first = runtime();
        let mut second = runtime();
        let spec = ProcessSpec::new(48_000.0, 257, 2).expect("valid spec");
        first.prepare(spec).expect("prepare");
        second.prepare(spec).expect("prepare");
        let on = [ProcessEvent {
            sample_offset: 37,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 127,
            },
        }];
        let first_audio = process(&mut first, 128, 0, &on);
        let empty: [ProcessEvent; 0] = [];
        let _ = process(&mut second, 37, 0, &empty);
        let second_audio = process(
            &mut second,
            91,
            37,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        assert!(
            first_audio[0][..37]
                .iter()
                .all(|sample| sample.abs() < 1.0e-6)
        );
        assert!(second_audio[0].iter().any(|sample| sample.abs() > 0.01));
        assert_relative_eq!(first_audio[0][37], second_audio[0][0], epsilon = 1.0e-5);
    }

    #[test]
    fn reset_restarts_the_same_render() {
        let mut runtime = runtime();
        prepare(&mut runtime);
        let event = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 3,
                note_number: 64,
                velocity: 100,
            },
        }];
        let first = process(&mut runtime, 128, 0, &event);
        runtime.reset().expect("reset");
        let second = process(&mut runtime, 128, 0, &event);
        for (left, right) in first[0].iter().zip(&second[0]) {
            assert_relative_eq!(*left, *right, epsilon = 1.0e-6);
        }
    }

    #[test]
    fn releasing_voice_is_selected_before_active_voice() {
        let mut runtime = runtime();
        prepare(&mut runtime);
        let events = [
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            },
            ProcessEvent {
                sample_offset: 1,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 64,
                    velocity: 127,
                },
            },
            ProcessEvent {
                sample_offset: 2,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            },
        ];
        let _ = process(&mut runtime, 16, 0, &events);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Releasing));
        assert_eq!(runtime.voice_state(1), Some(VoiceState::Active));
    }

    #[test]
    fn steal_starts_pending_note_when_release_finishes_before_fade() {
        let mut source = definition();
        source.performance.polyphony = 1;
        source.layers[0].envelope = crate::definition::AdsrDefinition {
            attack_seconds: 0.0,
            decay_seconds: 0.0,
            sustain_level: 1.0,
            release_seconds: 0.001,
        };
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            64,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        let audio = process(
            &mut runtime,
            64,
            64,
            &[
                ProcessEvent {
                    sample_offset: 0,
                    kind: ProcessEventKind::NoteOff { note_id: 1 },
                },
                ProcessEvent {
                    sample_offset: 47,
                    kind: ProcessEventKind::NoteOn {
                        note_id: 2,
                        note_number: 64,
                        velocity: 127,
                    },
                },
            ],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));
        assert!(audio[0][48..].iter().any(|sample| sample.abs() > 1.0e-6));
    }

    #[test]
    fn steal_fade_completes_across_multiple_blocks() {
        let mut source = definition();
        source.performance.polyphony = 1;
        source.layers[0].envelope.attack_seconds = 0.0;
        source.layers[0].envelope.decay_seconds = 0.0;
        source.layers[0].envelope.sustain_level = 1.0;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            64,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        let _ = process(
            &mut runtime,
            64,
            64,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 64,
                    velocity: 127,
                },
            }],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::StealFading));
        let empty: [ProcessEvent; 0] = [];
        let _ = process(&mut runtime, 64, 128, &empty);
        let _ = process(&mut runtime, 64, 192, &empty);
        let _ = process(&mut runtime, 64, 256, &empty);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));
    }

    #[test]
    fn pending_note_off_cancels_note_before_steal_completion() {
        let mut source = definition();
        source.performance.polyphony = 1;
        source.layers[0].envelope.attack_seconds = 0.0;
        source.layers[0].envelope.decay_seconds = 0.0;
        source.layers[0].envelope.sustain_level = 1.0;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            64,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        let _ = process(
            &mut runtime,
            64,
            64,
            &[
                ProcessEvent {
                    sample_offset: 0,
                    kind: ProcessEventKind::NoteOn {
                        note_id: 2,
                        note_number: 64,
                        velocity: 127,
                    },
                },
                ProcessEvent {
                    sample_offset: 1,
                    kind: ProcessEventKind::NoteOff { note_id: 2 },
                },
            ],
        );
        let empty: [ProcessEvent; 0] = [];
        let _ = process(&mut runtime, 64, 128, &empty);
        let _ = process(&mut runtime, 64, 192, &empty);
        let _ = process(&mut runtime, 64, 256, &empty);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Idle));
    }

    #[test]
    fn reset_discards_pending_note_during_steal() {
        let mut source = definition();
        source.performance.polyphony = 1;
        source.layers[0].envelope.attack_seconds = 0.0;
        source.layers[0].envelope.decay_seconds = 0.0;
        source.layers[0].envelope.sustain_level = 1.0;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            64,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        let _ = process(
            &mut runtime,
            64,
            64,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 64,
                    velocity: 127,
                },
            }],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::StealFading));
        runtime.reset().expect("reset");
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Idle));
    }

    #[test]
    fn out_of_range_key_does_not_steal_a_full_voice() {
        let mut source = definition();
        source.performance.polyphony = 1;
        source.layers[0].trigger.key_min = 60;
        source.layers[0].trigger.key_max = 72;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            64,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        let _ = process(
            &mut runtime,
            64,
            64,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 59,
                    velocity: 127,
                },
            }],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));
    }

    #[test]
    fn out_of_range_velocity_does_not_steal_a_full_voice() {
        let mut source = definition();
        source.performance.polyphony = 1;
        source.layers[0].trigger.velocity_min = 64;
        source.layers[0].trigger.velocity_max = 127;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            64,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 64,
                },
            }],
        );
        let _ = process(
            &mut runtime,
            64,
            64,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 60,
                    velocity: 63,
                },
            }],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));
    }

    #[test]
    fn out_of_range_note_does_not_start_an_idle_voice_and_boundaries_trigger() {
        let mut source = definition();
        source.layers[0].trigger.key_min = 60;
        source.layers[0].trigger.key_max = 60;
        source.layers[0].trigger.velocity_min = 64;
        source.layers[0].trigger.velocity_max = 64;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            1,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 59,
                    velocity: 64,
                },
            }],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Idle));
        let _ = process(
            &mut runtime,
            1,
            1,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 60,
                    velocity: 64,
                },
            }],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));
    }

    fn phase_runtime(phase_reset: bool) -> InstrumentRuntime {
        let mut source = definition();
        source.performance.polyphony = 1;
        source.layers[0].envelope.attack_seconds = 0.0;
        source.layers[0].envelope.decay_seconds = 0.0;
        source.layers[0].envelope.sustain_level = 1.0;
        source.layers[0].envelope.release_seconds = 0.001;
        match &mut source.layers[0].generator {
            crate::definition::GeneratorDefinition::Oscillator(oscillator) => {
                oscillator.phase_reset = phase_reset;
            }
            crate::definition::GeneratorDefinition::Sample(_) => {
                panic!("test fixture must use an oscillator");
            }
        }
        runtime_with(&source)
    }

    fn modulated_steal_definition() -> crate::definition::InstrumentDefinition {
        let mut source = definition();
        source.performance.polyphony = 1;
        source.layers[0].envelope.attack_seconds = 0.0;
        source.layers[0].envelope.decay_seconds = 0.0;
        source.layers[0].envelope.sustain_level = 1.0;
        source.layers[0].envelope.release_seconds = 0.25;
        source.voice_filter = Some(crate::definition::FilterDefinition {
            cutoff_hz: 2_000.0,
            resonance: 0.15,
        });
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![
                crate::definition::ModulationSourceDefinition::Lfo(
                    crate::definition::LfoDefinition {
                        id: "steal_lfo".to_owned(),
                        waveform: crate::definition::LfoWaveform::Triangle,
                        rate_hz: 37.0,
                        phase: 0.0,
                    },
                ),
                crate::definition::ModulationSourceDefinition::Envelope(
                    crate::definition::ModEnvelopeDefinition {
                        id: "steal_envelope".to_owned(),
                        attack_seconds: 0.01,
                        decay_seconds: 0.03,
                        sustain_level: 0.4,
                        release_seconds: 0.1,
                    },
                ),
            ],
            routes: vec![
                crate::definition::ModulationRouteDefinition {
                    source: "steal_lfo".to_owned(),
                    target: "layer.body.gain".to_owned(),
                    amount: 0.5,
                    curve: crate::definition::ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "steal_lfo".to_owned(),
                    target: "voice.filter.cutoff".to_owned(),
                    amount: 0.25,
                    curve: crate::definition::ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "steal_envelope".to_owned(),
                    target: "layer.body.tuning".to_owned(),
                    amount: 0.1,
                    curve: crate::definition::ModulationCurve::Linear,
                },
            ],
        });
        source
    }

    #[test]
    fn steal_fade_continues_parameter_lfo_and_envelope_processing() {
        let dynamic_definition = modulated_steal_definition();
        let mut static_definition = dynamic_definition.clone();
        static_definition.modulation = None;
        let mut dynamic = runtime_with(&dynamic_definition);
        let mut static_runtime = runtime_with(&static_definition);
        prepare(&mut dynamic);
        prepare(&mut static_runtime);
        let first_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 127,
            },
        }];
        let _ = process(&mut dynamic, 256, 0, &first_note);
        let _ = process(&mut static_runtime, 256, 0, &first_note);
        let dynamic_gain = dynamic
            .compiled()
            .parameter_handle("layer.body.gain")
            .expect("body gain parameter");
        let static_gain = static_runtime
            .compiled()
            .parameter_handle("layer.body.gain")
            .expect("body gain parameter");
        let steal_events = [
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 67,
                    velocity: 110,
                },
            },
            ProcessEvent {
                sample_offset: 32,
                kind: ProcessEventKind::ParameterChange {
                    parameter: dynamic_gain,
                    normalized: 1.0,
                },
            },
        ];
        let static_events = [
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 67,
                    velocity: 110,
                },
            },
            ProcessEvent {
                sample_offset: 32,
                kind: ProcessEventKind::ParameterChange {
                    parameter: static_gain,
                    normalized: 1.0,
                },
            },
        ];
        let dynamic_audio = process(&mut dynamic, 256, 256, &steal_events);
        let static_audio = process(&mut static_runtime, 256, 256, &static_events);
        assert_eq!(dynamic.voice_state(0), Some(VoiceState::Active));
        assert_eq!(static_runtime.voice_state(0), Some(VoiceState::Active));
        assert!(
            dynamic_audio
                .iter()
                .flatten()
                .all(|sample| sample.is_finite())
        );
        assert!(
            dynamic_audio[0][..240]
                .iter()
                .zip(&static_audio[0][..240])
                .any(|(dynamic, static_sample)| (dynamic - static_sample).abs() > 1.0e-5)
        );
    }

    #[test]
    fn steal_completion_inside_a_control_span_starts_the_pending_note() {
        let mut runtime = runtime_with(&modulated_steal_definition());
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            256,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        let audio = process(
            &mut runtime,
            256,
            256,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 67,
                    velocity: 110,
                },
            }],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));
        assert!(audio.iter().flatten().all(|sample| sample.is_finite()));
        assert!(audio[0][240..].iter().any(|sample| sample.abs() > 1.0e-6));
        assert!((audio[0][240] - audio[0][239]).abs() < 0.5);
    }

    #[test]
    fn static_and_dynamic_targets_render_finite_audio() {
        let mut static_runtime = runtime();
        prepare(&mut static_runtime);
        let static_audio = process(
            &mut static_runtime,
            128,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 100,
                },
            }],
        );
        assert!(
            static_audio
                .iter()
                .flatten()
                .all(|sample| sample.is_finite())
        );
        assert!(
            static_audio
                .iter()
                .flatten()
                .any(|sample| sample.abs() > 1.0e-6)
        );

        let mut static_filter_definition = modulated_steal_definition();
        static_filter_definition.modulation = None;
        let mut static_filter_runtime = runtime_with(&static_filter_definition);
        prepare(&mut static_filter_runtime);
        let static_filter_audio = process(
            &mut static_filter_runtime,
            128,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 100,
                },
            }],
        );
        assert!(
            static_filter_audio
                .iter()
                .flatten()
                .all(|sample| sample.is_finite())
        );
        assert!(
            static_filter_audio
                .iter()
                .flatten()
                .any(|sample| sample.abs() > 1.0e-6)
        );

        let mut dynamic_runtime = runtime_with(&modulated_steal_definition());
        prepare(&mut dynamic_runtime);
        let dynamic_audio = process(
            &mut dynamic_runtime,
            256,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 100,
                },
            }],
        );
        assert!(
            dynamic_audio
                .iter()
                .flatten()
                .all(|sample| sample.is_finite())
        );
        assert!(
            dynamic_audio
                .iter()
                .flatten()
                .any(|sample| sample.abs() > 1.0e-6)
        );
    }

    #[test]
    fn phase_reset_changes_retriggered_note_phase() {
        let mut reset_runtime = phase_runtime(true);
        let mut continue_runtime = phase_runtime(false);
        prepare(&mut reset_runtime);
        prepare(&mut continue_runtime);
        let first_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 127,
            },
        }];
        let _ = process(&mut reset_runtime, 64, 0, &first_note);
        let _ = process(&mut continue_runtime, 64, 0, &first_note);
        let retrigger = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 60,
                velocity: 127,
            },
        }];
        let reset_audio = process(&mut reset_runtime, 257, 64, &retrigger);
        let continue_audio = process(&mut continue_runtime, 257, 64, &retrigger);
        assert!(
            reset_audio[0][240..]
                .iter()
                .zip(&continue_audio[0][240..])
                .any(|(reset, continued)| (reset - continued).abs() > 1.0e-4)
        );
    }

    #[test]
    fn full_reset_restarts_phase_even_when_note_phase_reset_is_disabled() {
        let mut runtime = phase_runtime(false);
        prepare(&mut runtime);
        let note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 127,
            },
        }];
        let first = process(&mut runtime, 64, 0, &note);
        runtime.reset().expect("reset");
        let second = process(&mut runtime, 64, 0, &note);
        for (left, right) in first[0].iter().zip(&second[0]) {
            assert_relative_eq!(*left, *right, epsilon = 1.0e-6);
        }
    }

    #[test]
    fn phase_reset_disabled_preserves_phase_after_release() {
        let mut runtime = phase_runtime(false);
        prepare(&mut runtime);
        let note_on = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 127,
            },
        }];
        let first = process(&mut runtime, 64, 0, &note_on);
        let note_off = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        }];
        let _ = process(&mut runtime, 64, 64, &note_off);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Idle));

        let continued = process(
            &mut runtime,
            64,
            128,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        assert!(
            continued[0]
                .iter()
                .zip(&first[0])
                .any(|(continued, first)| (continued - first).abs() > 1.0e-4)
        );
    }

    fn process_parameter_event(runtime: &mut InstrumentRuntime, event: ProcessEventKind) {
        let mut left = [1.0_f32; 32];
        let mut right = [1.0_f32; 32];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        runtime
            .process(ProcessBlock {
                frames: 32,
                context: crate::process::ProcessContext {
                    absolute_frame: 0,
                    tempo_bpm: 120.0,
                },
                events: &[ProcessEvent {
                    sample_offset: 0,
                    kind: event,
                }],
                output: &mut output,
            })
            .expect("parameter event process");
    }

    #[test]
    fn invalid_parameter_event_leaves_runtime_state_unchanged() {
        let instrument = compiled(2);
        let mut runtime = instrument.instantiate();
        runtime
            .prepare(ProcessSpec::new(48_000.0, 64, 2).expect("valid spec"))
            .expect("runtime preparation");
        let invalid = ProcessEvent {
            sample_offset: 8,
            kind: ProcessEventKind::ParameterChange {
                parameter: ParameterHandle::new(instrument.parameters().len()),
                normalized: 0.5,
            },
        };
        let note_on = ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        };
        let mut left = [1.0_f32; 32];
        let mut right = [1.0_f32; 32];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        let error = runtime.process(ProcessBlock {
            frames: 32,
            context: crate::process::ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
            },
            events: &[note_on, invalid],
            output: &mut output,
        });
        assert_eq!(
            error,
            Err(ProcessError::ParameterHandleOutOfRange {
                handle: instrument.parameters().len(),
            })
        );
        assert_eq!(runtime.absolute_frame(), 0);
        assert!(
            runtime
                .voices
                .iter()
                .all(|voice| voice.state() == VoiceState::Idle)
        );
        assert!(left.iter().all(|sample| sample.abs() < f32::EPSILON));
        assert!(right.iter().all(|sample| sample.abs() < f32::EPSILON));
    }

    #[test]
    fn shared_parameter_state_advances_once_for_any_voice_count() {
        let mut one_voice = compiled(1).instantiate();
        let mut eight_voices = compiled(8).instantiate();
        let spec = ProcessSpec::new(48_000.0, 64, 2).expect("valid spec");
        one_voice.prepare(spec).expect("one voice preparation");
        eight_voices.prepare(spec).expect("eight voice preparation");
        let parameter = one_voice.compiled().parameters()[0].id.clone();
        let one_handle = one_voice
            .compiled()
            .parameter_handle(&parameter)
            .expect("parameter handle");
        let eight_handle = eight_voices
            .compiled()
            .parameter_handle(&parameter)
            .expect("parameter handle");
        process_parameter_event(
            &mut one_voice,
            ProcessEventKind::ParameterChange {
                parameter: one_handle,
                normalized: 1.0,
            },
        );
        process_parameter_event(
            &mut eight_voices,
            ProcessEventKind::ParameterChange {
                parameter: eight_handle,
                normalized: 1.0,
            },
        );
        let one_current = one_voice.parameter_states[one_handle.index()].span(0).0;
        let eight_current = eight_voices.parameter_states[eight_handle.index()]
            .span(0)
            .0;
        assert!((one_current - eight_current).abs() < f32::EPSILON);
    }
}
