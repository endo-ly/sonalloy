use std::sync::Arc;

use crate::compiler::{CompiledGenerator, CompiledInstrument};
use crate::process::{
    InstrumentProcessor, ProcessBlock, ProcessError, ProcessEventKind, ProcessSpec, clear_output,
};

use super::smoothing::rounded_frame_count;
use super::voice::{NoteRequest, VoiceRuntime, VoiceState};

const STEAL_FADE_SECONDS: f64 = 0.005;

struct RuntimeScratch {
    layer_mono: Vec<f32>,
    voice_left: Vec<f32>,
    voice_right: Vec<f32>,
}

/// Prepared polyphonic runtime for one immutable compiled instrument.
pub struct InstrumentRuntime {
    compiled: Arc<CompiledInstrument>,
    voices: Vec<VoiceRuntime>,
    scratch: RuntimeScratch,
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
            },
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
        &mut self,
        output: &mut [&mut [f32]],
        start: usize,
        end: usize,
        sample_rate: f64,
    ) -> Result<(), ProcessError> {
        if start >= end {
            return Ok(());
        }
        let frames = end - start;
        let velocity_response = self.compiled.velocity_response;
        let (left_channels, right_channels) = output.split_at_mut(1);
        let left = &mut left_channels[0][start..end];
        let right = &mut right_channels[0][start..end];
        for voice in &mut self.voices {
            let mut offset = 0;
            while offset < frames {
                let remaining = frames - offset;
                let chunk = if voice.is_stealing() {
                    voice.steal_frames_remaining().min(remaining)
                } else {
                    remaining
                };
                if chunk == 0 {
                    break;
                }
                voice.render(
                    chunk,
                    sample_rate,
                    velocity_response,
                    &mut self.scratch.layer_mono,
                    &mut self.scratch.voice_left,
                    &mut self.scratch.voice_right,
                )?;
                for index in 0..chunk {
                    left[offset + index] += self.scratch.voice_left[index];
                    right[offset + index] += self.scratch.voice_right[index];
                }
                offset += chunk;
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
                    spec.sample_rate,
                    fade_frames,
                    self.compiled.velocity_response,
                )?;
            }
            ProcessEventKind::NoteOff { note_id } => {
                for voice in &mut self.voices {
                    voice.release_note(note_id);
                }
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
}

impl InstrumentProcessor for InstrumentRuntime {
    fn prepare(&mut self, spec: ProcessSpec) -> Result<(), ProcessError> {
        // A failed preparation must not leave an older prepared state usable.
        self.spec = None;
        self.voices.clear();
        self.scratch.layer_mono.clear();
        self.scratch.voice_left.clear();
        self.scratch.voice_right.clear();
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
        let mut cursor = 0;
        let mut event_index = 0;
        while event_index < block.events.len() {
            let offset = block.events[event_index].sample_offset;
            self.render_range(block.output, cursor, offset, spec.sample_rate)?;
            while event_index < block.events.len()
                && block.events[event_index].sample_offset == offset
            {
                let event = block.events[event_index].kind;
                self.apply_event(event, block.context.absolute_frame + offset as u64)?;
                event_index += 1;
            }
            cursor = offset;
        }
        self.render_range(block.output, cursor, block.frames, spec.sample_rate)?;
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
        self.scratch.layer_mono.fill(0.0);
        self.scratch.voice_left.fill(0.0);
        self.scratch.voice_right.fill(0.0);
        self.absolute_frame = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use super::*;
    use crate::compiler::{CompileContext, compile_instrument};
    use crate::definition::tests::definition;
    use crate::process::ProcessEvent;

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
        let definition = definition();
        runtime_with(&definition)
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
        runtime
            .prepare(ProcessSpec::new(48_000.0, 257, 2).expect("valid spec"))
            .expect("prepare");
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
                process_spec: ProcessSpec::new(44_100.0, 257, 2).expect("valid process spec"),
            },
        );
        let mut runtime = result
            .instrument
            .expect("compiled instrument")
            .instantiate();
        runtime
            .prepare(ProcessSpec::new(44_100.0, 257, 2).expect("valid process spec"))
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
        let spec = ProcessSpec::new(48_000.0, 257, 2).expect("valid spec");
        runtime.prepare(spec).expect("prepare");
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
        let spec = ProcessSpec::new(48_000.0, 257, 2).expect("valid spec");
        runtime.prepare(spec).expect("prepare");
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
}
