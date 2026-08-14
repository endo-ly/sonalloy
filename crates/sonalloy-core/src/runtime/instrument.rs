use std::sync::Arc;

use crate::compiler::{
    CompiledGenerator, CompiledInstrument, CompiledProcessor, CompiledProcessorKind,
    CompiledSampleZone,
};
use crate::definition::LayerTriggerEvent;
use crate::parameter::ParameterScale;
use crate::process::{
    InstrumentProcessor, ProcessBlock, ProcessError, ProcessEventKind, ProcessSpec, clear_output,
};

use super::modulation::{ParameterSpanValue, SharedParameterSpan, ValueSpan};
use super::processor::{ProcessorTargetSpan, StereoProcessorChain};
use super::smoothing::{Smoother, rounded_frame_count};
use super::voice::{NoteRequest, PreparedLayerSelection, VoiceRuntime, VoiceState};

const CONTROL_SMOOTHING_SECONDS: f64 = 0.005;
const STEAL_FADE_SECONDS: f64 = 0.005;
const QUANTUM_FRAMES: usize = 32;

struct RuntimeScratch {
    layer_mono: Vec<f32>,
    layer_left: Vec<f32>,
    layer_right: Vec<f32>,
    voice_left: Vec<f32>,
    voice_right: Vec<f32>,
    parameter_spans: Vec<ParameterSpanValue>,
}

/// Prepared polyphonic runtime for one immutable compiled instrument.
pub struct InstrumentRuntime {
    compiled: Arc<CompiledInstrument>,
    voices: Vec<VoiceRuntime>,
    scratch: RuntimeScratch,
    note_layer_selection: Vec<PreparedLayerSelection>,
    parameter_states: Vec<Smoother>,
    pitch_bend: Smoother,
    mod_wheel: Smoother,
    aftertouch: Smoother,
    global_processors: Option<StereoProcessorChain>,
    global_targets: Vec<ProcessorTargetSpan>,
    round_robin_counters: Vec<Vec<u64>>,
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
                layer_left: Vec::new(),
                layer_right: Vec::new(),
                voice_left: Vec::new(),
                voice_right: Vec::new(),
                parameter_spans: Vec::new(),
            },
            note_layer_selection: Vec::new(),
            parameter_states: Vec::new(),
            pitch_bend: Smoother::new(0.0),
            mod_wheel: Smoother::new(0.0),
            aftertouch: Smoother::new(0.0),
            global_processors: None,
            global_targets: Vec::new(),
            round_robin_counters: Vec::new(),
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
        layer_left: &mut [f32],
        layer_right: &mut [f32],
        voice_left: &mut [f32],
        voice_right: &mut [f32],
        output: &mut [&mut [f32]],
        start: usize,
        end: usize,
        sample_rate: f64,
        tempo_bpm: f64,
        shared: SharedParameterSpan<'_>,
    ) -> Result<(), ProcessError> {
        if start >= end {
            return Ok(());
        }
        let frames = end - start;
        if output.len() < 2 {
            return Err(invalid_state());
        }
        let (left_channels, right_channels) = output.split_at_mut(1);
        let left = left_channels
            .first_mut()
            .and_then(|channel| channel.get_mut(start..end))
            .ok_or_else(invalid_state)?;
        let right = right_channels
            .first_mut()
            .and_then(|channel| channel.get_mut(start..end))
            .ok_or_else(invalid_state)?;
        for voice in voices {
            if voice.state() == VoiceState::Idle {
                continue;
            }
            voice.render_span(
                frames,
                sample_rate,
                tempo_bpm,
                compiled,
                shared,
                &mut layer_mono[..frames],
                &mut layer_left[..frames],
                &mut layer_right[..frames],
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
                let request = NoteRequest::new(note_id, note_number, velocity, absolute_frame);
                if !self.prepare_note_request(request)? {
                    return Ok(());
                }
                let voice_index = self.select_voice();
                let fade_frames = rounded_frame_count(spec.sample_rate * STEAL_FADE_SECONDS);
                self.voices
                    .get_mut(voice_index)
                    .ok_or_else(invalid_state)?
                    .request_note(
                        &self.compiled,
                        request,
                        &self.note_layer_selection,
                        fade_frames,
                    )?;
            }
            ProcessEventKind::NoteOff { note_id } => {
                for voice in &mut self.voices {
                    voice.release_note(&self.compiled, note_id)?;
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
                self.parameter_states
                    .get_mut(parameter.index())
                    .ok_or_else(invalid_state)?
                    .set_target(normalized, frames);
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

    fn prepare_note_request(&mut self, note: NoteRequest) -> Result<bool, ProcessError> {
        if self.note_layer_selection.len() != self.compiled.layers.len() {
            return Err(invalid_state());
        }
        self.note_layer_selection
            .fill(PreparedLayerSelection::Inactive);
        let mut can_trigger = false;
        let compiled = Arc::clone(&self.compiled);
        for (layer_index, layer) in compiled.layers.iter().enumerate() {
            if !layer.trigger.matches(note.note_number, note.velocity) {
                continue;
            }
            if !layer.generator.is_available() {
                continue;
            }
            match &layer.generator {
                CompiledGenerator::Oscillator(_)
                | CompiledGenerator::Noise(_)
                | CompiledGenerator::Additive(_)
                | CompiledGenerator::Formant(_)
                | CompiledGenerator::Granular(_)
                | CompiledGenerator::WaveSequence(_)
                | CompiledGenerator::Wavetable(_)
                | CompiledGenerator::Spectral(_)
                | CompiledGenerator::OperatorModulation(_) => {
                    *self
                        .note_layer_selection
                        .get_mut(layer_index)
                        .ok_or_else(invalid_state)? = match layer.trigger.event {
                        LayerTriggerEvent::NoteOn => {
                            PreparedLayerSelection::Active { sample_zone: None }
                        }
                        LayerTriggerEvent::NoteOff => {
                            PreparedLayerSelection::Armed { sample_zone: None }
                        }
                    };
                    can_trigger = true;
                }
                CompiledGenerator::Sample(sample) => {
                    if let Some(zone_index) = self.select_sample_zone(
                        layer_index,
                        sample,
                        note.note_number,
                        note.velocity,
                    )? {
                        *self
                            .note_layer_selection
                            .get_mut(layer_index)
                            .ok_or_else(invalid_state)? = match layer.trigger.event {
                            LayerTriggerEvent::NoteOn => PreparedLayerSelection::Active {
                                sample_zone: Some(zone_index),
                            },
                            LayerTriggerEvent::NoteOff => PreparedLayerSelection::Armed {
                                sample_zone: Some(zone_index),
                            },
                        };
                        can_trigger = true;
                    }
                }
            }
        }
        Ok(can_trigger)
    }

    fn select_sample_zone(
        &mut self,
        layer_index: usize,
        sample: &crate::compiler::CompiledSample,
        note_number: u8,
        velocity: u8,
    ) -> Result<Option<usize>, ProcessError> {
        let Some((first_index, first_zone)) = sample
            .zones
            .iter()
            .enumerate()
            .find(|(_, zone)| zone_matches(zone, note_number, velocity))
        else {
            return Ok(None);
        };
        let Some(group_index) = first_zone.group else {
            return Ok(Some(first_index));
        };
        let group = sample.groups.get(group_index).ok_or_else(invalid_state)?;
        let counter = self
            .round_robin_counters
            .get(layer_index)
            .and_then(|counters| counters.get(group_index))
            .copied()
            .ok_or_else(invalid_state)?;
        let matching_count = group
            .enabled_member_zone_indices
            .iter()
            .filter(|index| {
                sample
                    .zones
                    .get(**index)
                    .is_some_and(|zone| zone_matches(zone, note_number, velocity))
            })
            .count();
        if matching_count == 0 {
            return Ok(None);
        }
        let divisor = u64::try_from(matching_count).map_err(|_| invalid_state())?;
        let selected_offset = usize::try_from(counter % divisor).map_err(|_| invalid_state())?;
        let selected = group
            .enabled_member_zone_indices
            .iter()
            .copied()
            .filter(|index| {
                sample
                    .zones
                    .get(*index)
                    .is_some_and(|zone| zone_matches(zone, note_number, velocity))
            })
            .nth(selected_offset)
            .ok_or_else(invalid_state)?;
        let next_counter = counter.wrapping_add(1);
        *self
            .round_robin_counters
            .get_mut(layer_index)
            .and_then(|counters| counters.get_mut(group_index))
            .ok_or_else(invalid_state)? = next_counter;
        Ok(Some(selected))
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

    fn evaluate_global_targets(
        compiled: &CompiledInstrument,
        targets: &mut [ProcessorTargetSpan],
        shared: SharedParameterSpan<'_>,
    ) -> Result<(), ProcessError> {
        if targets.len() != compiled.global_processors.len() {
            return Err(invalid_state());
        }
        for (target, processor) in targets.iter_mut().zip(&compiled.global_processors) {
            *target = Self::evaluate_global_processor_target(compiled, processor, shared)?;
        }
        Ok(())
    }

    fn evaluate_global_processor_target(
        compiled: &CompiledInstrument,
        processor: &CompiledProcessor,
        shared: SharedParameterSpan<'_>,
    ) -> Result<ProcessorTargetSpan, ProcessError> {
        match &processor.processor {
            CompiledProcessorKind::Filter(value) => Ok(ProcessorTargetSpan::Filter {
                cutoff: clamp_filter_span(
                    Self::evaluate_global_target(compiled, value.parameters.cutoff, shared)?,
                    value.effective_max_cutoff_hz,
                ),
                resonance: Self::evaluate_global_target(
                    compiled,
                    value.parameters.resonance,
                    shared,
                )?,
            }),
            CompiledProcessorKind::Drive(value) => Ok(ProcessorTargetSpan::Drive {
                amount: Self::evaluate_global_target(compiled, value.amount, shared)?,
                mix: Self::evaluate_global_target(compiled, value.mix, shared)?,
            }),
            CompiledProcessorKind::Delay(value) => Ok(ProcessorTargetSpan::Delay {
                feedback: Self::evaluate_global_target(compiled, value.feedback, shared)?,
                mix: Self::evaluate_global_target(compiled, value.mix, shared)?,
            }),
            CompiledProcessorKind::Reverb(value) => Ok(ProcessorTargetSpan::Reverb {
                decay: Self::evaluate_global_target(compiled, value.decay, shared)?,
                damping: Self::evaluate_global_target(compiled, value.damping, shared)?,
                width: Self::evaluate_global_target(compiled, value.width, shared)?,
                mix: Self::evaluate_global_target(compiled, value.mix, shared)?,
            }),
        }
    }

    fn evaluate_global_target(
        compiled: &CompiledInstrument,
        handle: crate::parameter::ParameterHandle,
        shared: SharedParameterSpan<'_>,
    ) -> Result<ValueSpan, ProcessError> {
        let descriptor = compiled.parameter_descriptor(handle).ok_or(
            ProcessError::ParameterHandleOutOfRange {
                handle: handle.index(),
            },
        )?;
        let base = shared.parameter(handle).ok_or_else(invalid_state)?;
        let base_start = descriptor
            .denormalize(base.start)
            .map_err(|_| ProcessError::InvalidEventValue)?;
        let base_end = descriptor
            .denormalize(base.end)
            .map_err(|_| ProcessError::InvalidEventValue)?;
        let range = descriptor.max - descriptor.min;
        let log_range = if descriptor.scale == ParameterScale::Log2 {
            (descriptor.max / descriptor.min).log2()
        } else {
            0.0
        };
        let mut linear_start = 0.0;
        let mut linear_end = 0.0;
        let mut logarithmic_start = 0.0;
        let mut logarithmic_end = 0.0;
        let routes = compiled
            .routes_for_checked(handle)
            .ok_or_else(invalid_state)?;
        for route in routes {
            let source = match route.source {
                crate::compiler::CompiledSourceRef::PitchBend => shared.pitch_bend(),
                crate::compiler::CompiledSourceRef::ModWheel => shared.mod_wheel(),
                crate::compiler::CompiledSourceRef::Aftertouch => shared.aftertouch(),
                crate::compiler::CompiledSourceRef::Voice(_) => {
                    return Err(ProcessError::ProcessorFailure {
                        kind: crate::process::ProcessorFailureKind::InvalidState,
                    });
                }
            };
            let start = curve_value(source.start, route.curve);
            let end = curve_value(source.end, route.curve);
            match descriptor.scale {
                ParameterScale::Linear => {
                    linear_start += start * route.amount * range;
                    linear_end += end * route.amount * range;
                }
                ParameterScale::Log2 => {
                    logarithmic_start += start * route.amount * log_range;
                    logarithmic_end += end * route.amount * log_range;
                }
            }
        }
        let (start, end) = match descriptor.scale {
            ParameterScale::Linear => (
                (base_start + linear_start).clamp(descriptor.min, descriptor.max),
                (base_end + linear_end).clamp(descriptor.min, descriptor.max),
            ),
            ParameterScale::Log2 => (
                (base_start * 2.0_f32.powf(logarithmic_start))
                    .clamp(descriptor.min, descriptor.max),
                (base_end * 2.0_f32.powf(logarithmic_end)).clamp(descriptor.min, descriptor.max),
            ),
        };
        if start.is_finite() && end.is_finite() {
            Ok(ValueSpan { start, end })
        } else {
            Err(ProcessError::InvalidEventValue)
        }
    }
}

impl InstrumentProcessor for InstrumentRuntime {
    fn prepare(&mut self, spec: ProcessSpec) -> Result<(), ProcessError> {
        self.spec = None;
        self.voices.clear();
        self.scratch.layer_mono.clear();
        self.scratch.layer_left.clear();
        self.scratch.layer_right.clear();
        self.scratch.voice_left.clear();
        self.scratch.voice_right.clear();
        self.scratch.parameter_spans.clear();
        self.note_layer_selection.clear();
        self.parameter_states.clear();
        self.pitch_bend.reset(0.0);
        self.mod_wheel.reset(0.0);
        self.aftertouch.reset(0.0);
        self.global_processors = None;
        self.global_targets.clear();
        self.round_robin_counters.clear();
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
        self.scratch.layer_left.resize(spec.max_block_size, 0.0);
        self.scratch.layer_right.resize(spec.max_block_size, 0.0);
        self.scratch.voice_left.resize(spec.max_block_size, 0.0);
        self.scratch.voice_right.resize(spec.max_block_size, 0.0);
        self.scratch.parameter_spans.resize(
            self.compiled.parameters().len(),
            ParameterSpanValue {
                start: 0.0,
                end: 0.0,
            },
        );
        self.note_layer_selection
            .resize(self.compiled.layers.len(), PreparedLayerSelection::Inactive);
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
        self.round_robin_counters = self
            .compiled
            .layers
            .iter()
            .map(|layer| match &layer.generator {
                CompiledGenerator::Sample(sample) => vec![0; sample.groups.len()],
                CompiledGenerator::Oscillator(_)
                | CompiledGenerator::Noise(_)
                | CompiledGenerator::Additive(_)
                | CompiledGenerator::Formant(_)
                | CompiledGenerator::Granular(_)
                | CompiledGenerator::WaveSequence(_)
                | CompiledGenerator::Wavetable(_)
                | CompiledGenerator::Spectral(_)
                | CompiledGenerator::OperatorModulation(_) => Vec::new(),
            })
            .collect();
        let mut voices = Vec::with_capacity(self.compiled.performance.polyphony);
        for _ in 0..self.compiled.performance.polyphony {
            voices.push(VoiceRuntime::new(&self.compiled, spec)?);
        }
        let global_processors = StereoProcessorChain::new(&self.compiled.global_processors, spec)?;
        self.voices = voices;
        self.global_targets = self
            .compiled
            .global_processors
            .iter()
            .map(|processor| ProcessorTargetSpan::zero_for(&processor.processor))
            .collect();
        self.global_processors = Some(global_processors);
        self.spec = Some(spec);
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
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
                    self.spec = None;
                    return Err(error);
                }
                event_index += 1;
            }

            let mut end = block.frames;
            if let Some(next_event) = block.events.get(event_index) {
                end = end.min(next_event.sample_offset);
            }
            let absolute = self.absolute_frame + cursor as u64;
            let Ok(absolute_frame) = usize::try_from(absolute) else {
                self.spec = None;
                return Err(ProcessError::FrameOverflow);
            };
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
                layer_left,
                layer_right,
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
                layer_left,
                layer_right,
                voice_left,
                voice_right,
                block.output,
                cursor,
                end,
                spec.sample_rate,
                block.context.tempo_bpm,
                shared,
            ) {
                clear_output(&mut *block.output, block.frames);
                self.spec = None;
                return Err(error);
            }
            if let Err(error) =
                Self::evaluate_global_targets(&self.compiled, &mut self.global_targets, shared)
            {
                clear_output(&mut *block.output, block.frames);
                self.spec = None;
                return Err(error);
            }
            let global_result = {
                if block.output.len() < 2 {
                    Err(invalid_state())
                } else {
                    let (left_channels, right_channels) = block.output.split_at_mut(1);
                    let left = left_channels
                        .first_mut()
                        .and_then(|channel| channel.get_mut(cursor..end));
                    let right = right_channels
                        .first_mut()
                        .and_then(|channel| channel.get_mut(cursor..end));
                    match (left, right) {
                        (Some(left), Some(right)) => self
                            .global_processors
                            .as_mut()
                            .ok_or(ProcessError::NotPrepared)
                            .and_then(|processors| {
                                processors.process(&self.global_targets, left, right)
                            }),
                        _ => Err(invalid_state()),
                    }
                }
            };
            if let Err(error) = global_result {
                clear_output(&mut *block.output, block.frames);
                self.spec = None;
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
            if let Err(error) = voice.reset() {
                self.spec = None;
                return Err(error);
            }
        }
        if let Some(processors) = self.global_processors.as_mut() {
            if let Err(error) = processors.reset() {
                self.spec = None;
                return Err(error);
            }
        }
        for (state, descriptor) in self
            .parameter_states
            .iter_mut()
            .zip(self.compiled.parameters())
        {
            let normalized = descriptor
                .normalize(descriptor.default)
                .map_err(|_| ProcessError::InvalidCompiledParameterDefault)
                .inspect_err(|_| {
                    self.spec = None;
                })?;
            state.reset(normalized);
        }
        self.pitch_bend.reset(0.0);
        self.mod_wheel.reset(0.0);
        self.aftertouch.reset(0.0);
        self.scratch.layer_mono.fill(0.0);
        self.scratch.layer_left.fill(0.0);
        self.scratch.layer_right.fill(0.0);
        self.scratch.voice_left.fill(0.0);
        self.scratch.voice_right.fill(0.0);
        self.scratch.parameter_spans.fill(ParameterSpanValue {
            start: 0.0,
            end: 0.0,
        });
        for target in &mut self.global_targets {
            target.clear();
        }
        for counters in &mut self.round_robin_counters {
            counters.fill(0);
        }
        self.absolute_frame = 0;
        Ok(())
    }
}

fn clamp_filter_span(span: ValueSpan, maximum: f32) -> ValueSpan {
    ValueSpan {
        start: span.start.min(maximum),
        end: span.end.min(maximum),
    }
}

fn curve_value(value: f32, curve: crate::definition::ModulationCurve) -> f32 {
    match curve {
        crate::definition::ModulationCurve::Linear => value,
        crate::definition::ModulationCurve::SmoothStep => {
            let magnitude = value.abs();
            let shaped = magnitude * magnitude * (3.0 - 2.0 * magnitude);
            value.signum() * shaped
        }
    }
}

fn control_smoothing_frames(sample_rate: f64) -> usize {
    rounded_frame_count(sample_rate * CONTROL_SMOOTHING_SECONDS).max(1)
}

fn zone_matches(zone: &CompiledSampleZone, note_number: u8, velocity: u8) -> bool {
    zone.is_enabled()
        && (zone.key_min..=zone.key_max).contains(&note_number)
        && (zone.velocity_min..=zone.velocity_max).contains(&velocity)
}

fn invalid_state() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: crate::process::ProcessorFailureKind::InvalidState,
    }
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

    fn process_with_stack_output(
        runtime: &mut InstrumentRuntime,
        absolute_frame: u64,
        events: &[ProcessEvent],
    ) {
        const FRAMES: usize = 64;
        let mut left = [0.0_f32; FRAMES];
        let mut right = [0.0_f32; FRAMES];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        runtime
            .process(ProcessBlock {
                frames: FRAMES,
                context: crate::process::ProcessContext {
                    absolute_frame,
                    tempo_bpm: 120.0,
                },
                events,
                output: &mut output,
            })
            .expect("process succeeds");
    }

    fn write_pcm_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().expect("fixture directory creates");
        let path = directory.path().join("fixture.wav");
        let samples = (0..128)
            .map(|index| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
                {
                    (30_000.0 * (std::f32::consts::TAU * index as f32 / 64.0).sin()) as i16
                }
            })
            .collect::<Vec<_>>();
        let payload_length = u32::try_from(samples.len() * 2).expect("fixture fits RIFF");
        let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + payload_length).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&48_000_u32.to_le_bytes());
        bytes.extend_from_slice(&96_000_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&payload_length.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(&path, bytes).expect("fixture WAV writes");
        (directory, path)
    }

    fn sample_stretch_definition(
        path: &std::path::Path,
    ) -> crate::definition::InstrumentDefinition {
        let mut source = definition();
        source.performance.polyphony = 1;
        source.layers[0].generator =
            crate::definition::GeneratorDefinition::Sample(crate::definition::SampleDefinition {
                interpolation: crate::definition::SampleInterpolation::Cubic,
                zones: vec![crate::definition::SampleZoneDefinition {
                    id: "stretch".to_owned(),
                    asset: crate::definition::AssetReference {
                        path: path.to_string_lossy().into_owned(),
                        sha256: None,
                    },
                    root_note: 60,
                    key_min: 0,
                    key_max: 127,
                    velocity_min: 1,
                    velocity_max: 127,
                    round_robin_group: None,
                    playback: crate::definition::SampleZonePlaybackDefinition {
                        region: crate::definition::SampleRegionDefinition {
                            start_seconds: 0.0,
                            end_seconds: None,
                        },
                        direction: crate::definition::SamplePlaybackDirection::Forward,
                        r#loop: None,
                        time: crate::definition::SampleTimeDefinition::FixedStretch { ratio: 1.0 },
                    },
                }],
            });
        source
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
    fn note_on_reuses_prepared_layer_selection_storage() {
        let mut source = definition();
        source.performance.polyphony = 1;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let note_layer_capacity = runtime.note_layer_selection.capacity();
        let pending_layer_capacity = runtime.voices[0].pending_layer_selection_capacity();

        let first_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];
        let _ = process(&mut runtime, 64, 0, &first_note);

        let second_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 64,
                velocity: 100,
            },
        }];
        let _ = process(&mut runtime, 64, 64, &second_note);

        assert_eq!(runtime.note_layer_selection.capacity(), note_layer_capacity);
        assert_eq!(
            runtime.voices[0].pending_layer_selection_capacity(),
            pending_layer_capacity
        );
    }

    #[test]
    fn idle_note_on_does_not_allocate_after_prepare() {
        let mut source = definition();
        source.performance.polyphony = 1;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let event = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];

        let _ = process(&mut runtime, 64, 0, &event);
        runtime.reset().expect("reset");

        let allocations = crate::test_allocator::count_allocations(|| {
            process_with_stack_output(&mut runtime, 0, &event);
        });

        assert_eq!(allocations, 0);
    }

    #[test]
    fn wavetable_render_does_not_allocate_after_prepare() {
        let (_directory, path) = write_pcm_fixture();
        let mut source = definition();
        source.performance.polyphony = 1;
        source.layers[0].generator = crate::definition::GeneratorDefinition::Wavetable(
            crate::definition::WavetableDefinition {
                asset: crate::definition::AssetReference {
                    path: path.to_string_lossy().into_owned(),
                    sha256: None,
                },
                frame_length: 64,
                position: 0.0,
                phase_reset: true,
                phase: 0.0,
                unison: None,
            },
        );
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let event = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];
        let _ = process(&mut runtime, 64, 0, &event);
        runtime.reset().expect("reset");

        let allocations = crate::test_allocator::count_allocations(|| {
            process_with_stack_output(&mut runtime, 0, &event);
        });

        assert_eq!(allocations, 0);
    }

    #[test]
    fn stretch_render_does_not_allocate_in_rust_after_prepare() {
        let (_directory, path) = write_pcm_fixture();
        let source = sample_stretch_definition(&path);
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let event = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];
        let _ = process(&mut runtime, 64, 0, &event);
        runtime.reset().expect("reset");

        let allocations = crate::test_allocator::count_allocations(|| {
            process_with_stack_output(&mut runtime, 0, &event);
        });

        assert_eq!(allocations, 0);
    }

    #[test]
    fn operator_render_does_not_allocate_after_prepare() {
        let mut source = definition();
        source.performance.polyphony = 1;
        let envelope = crate::definition::AdsrDefinition {
            attack_seconds: 0.0,
            decay_seconds: 0.1,
            sustain_level: 1.0,
            release_seconds: 0.01,
        };
        source.layers[0].generator = crate::definition::GeneratorDefinition::OperatorModulation(
            crate::definition::OperatorModulationDefinition {
                mode: crate::definition::OperatorModulationMode::Phase,
                algorithm: crate::definition::OperatorAlgorithm::Stack4,
                operators: vec![
                    crate::definition::OperatorDefinition {
                        ratio: 1.0,
                        detune_cents: 0.0,
                        level: 0.9,
                        modulation_amount: 0.0,
                        feedback: 0.0,
                        phase: 0.0,
                        envelope,
                    },
                    crate::definition::OperatorDefinition {
                        ratio: 2.0,
                        detune_cents: 0.0,
                        level: 0.0,
                        modulation_amount: 2.0,
                        feedback: 0.0,
                        phase: 0.0,
                        envelope,
                    },
                    crate::definition::OperatorDefinition {
                        ratio: 3.0,
                        detune_cents: 0.0,
                        level: 0.0,
                        modulation_amount: 2.0,
                        feedback: 0.0,
                        phase: 0.0,
                        envelope,
                    },
                    crate::definition::OperatorDefinition {
                        ratio: 5.0,
                        detune_cents: 0.0,
                        level: 0.0,
                        modulation_amount: 2.0,
                        feedback: 0.2,
                        phase: 0.0,
                        envelope,
                    },
                ],
                phase_reset: true,
                unison: None,
            },
        );
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let event = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];
        let _ = process(&mut runtime, 64, 0, &event);
        runtime.reset().expect("reset");

        let allocations = crate::test_allocator::count_allocations(|| {
            process_with_stack_output(&mut runtime, 0, &event);
        });

        assert_eq!(allocations, 0);
    }

    #[test]
    fn complex_oscillator_render_does_not_allocate_after_prepare() {
        let mut source = definition();
        source.performance.polyphony = 1;
        source.layers[0].generator = crate::definition::GeneratorDefinition::Oscillator(
            crate::definition::OscillatorDefinition {
                waveform: crate::definition::OscillatorWaveform::Sine,
                phase_reset: true,
                phase: 0.0,
                hard_sync: None,
                waveshaping: None,
                phase_distortion: Some(crate::definition::PhaseDistortionDefinition {
                    amount: 0.5,
                }),
                wavefold: Some(crate::definition::WavefoldDefinition { amount: 0.35 }),
                feedback: Some(crate::definition::OscillatorFeedbackDefinition { amount: 0.2 }),
                unison: None,
            },
        );
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let event = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];
        let _ = process(&mut runtime, 64, 0, &event);
        runtime.reset().expect("reset");

        let allocations = crate::test_allocator::count_allocations(|| {
            process_with_stack_output(&mut runtime, 0, &event);
        });

        assert_eq!(allocations, 0);
    }

    #[test]
    fn additive_sixteen_voice_render_does_not_allocate_after_prepare() {
        let mut source = definition();
        source.performance.polyphony = 16;
        source.layers[0].generator = crate::definition::GeneratorDefinition::Additive(
            crate::definition::AdditiveDefinition {
                phase_reset: true,
                morph: 0.0,
                spectrum_tilt_db_per_octave: -3.0,
                inharmonicity: 0.25,
                partials: (0usize..64)
                    .map(|index| crate::definition::AdditivePartialDefinition {
                        id: format!("partial_{index}"),
                        ratio: f32::from(u16::try_from(index).expect("partial index fits")) + 1.0,
                        amplitude_a: 1.0
                            / (f32::from(u16::try_from(index).expect("partial index fits")) + 1.0),
                        amplitude_b: 0.75
                            / (f32::from(u16::try_from(index).expect("partial index fits")) + 1.0),
                        phase: 0.0,
                        envelope: None,
                    })
                    .collect(),
            },
        );
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let first_events: Vec<ProcessEvent> = (0usize..16)
            .map(|index| ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: u64::try_from(index).expect("voice index fits in note id") + 1,
                    note_number: 48
                        + u8::try_from(index).expect("voice index fits in MIDI note number"),
                    velocity: 100,
                },
            })
            .collect();
        let steal_event = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 17,
                note_number: 72,
                velocity: 100,
            },
        }];
        process_with_stack_output(&mut runtime, 0, &first_events);
        runtime.reset().expect("reset");
        process_with_stack_output(&mut runtime, 0, &first_events);

        let allocations = crate::test_allocator::count_allocations(|| {
            process_with_stack_output(&mut runtime, 64, &steal_event);
        });

        assert_eq!(allocations, 0);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn formant_sixteen_voice_render_does_not_allocate_after_prepare() {
        let mut source = definition();
        source.performance.polyphony = 16;
        source.layers[0].generator =
            crate::definition::GeneratorDefinition::Formant(crate::definition::FormantDefinition {
                phase_reset: true,
                partial_count: 64,
                vowel_position: 0.0,
                formant_shift_cents: 0.0,
                throat: 0.5,
                spectral_tilt_db_per_octave: -6.0,
                profiles: vec![
                    crate::definition::FormantProfileDefinition {
                        id: "a".to_owned(),
                        formants: vec![
                            crate::definition::FormantBandDefinition {
                                frequency_hz: 800.0,
                                bandwidth_hz: 80.0,
                                gain_db: 0.0,
                            },
                            crate::definition::FormantBandDefinition {
                                frequency_hz: 1_200.0,
                                bandwidth_hz: 100.0,
                                gain_db: -5.0,
                            },
                            crate::definition::FormantBandDefinition {
                                frequency_hz: 2_500.0,
                                bandwidth_hz: 120.0,
                                gain_db: -10.0,
                            },
                            crate::definition::FormantBandDefinition {
                                frequency_hz: 3_500.0,
                                bandwidth_hz: 140.0,
                                gain_db: -15.0,
                            },
                            crate::definition::FormantBandDefinition {
                                frequency_hz: 5_000.0,
                                bandwidth_hz: 160.0,
                                gain_db: -20.0,
                            },
                        ],
                    },
                    crate::definition::FormantProfileDefinition {
                        id: "i".to_owned(),
                        formants: vec![
                            crate::definition::FormantBandDefinition {
                                frequency_hz: 300.0,
                                bandwidth_hz: 60.0,
                                gain_db: 0.0,
                            },
                            crate::definition::FormantBandDefinition {
                                frequency_hz: 2_200.0,
                                bandwidth_hz: 100.0,
                                gain_db: -5.0,
                            },
                            crate::definition::FormantBandDefinition {
                                frequency_hz: 3_000.0,
                                bandwidth_hz: 120.0,
                                gain_db: -10.0,
                            },
                            crate::definition::FormantBandDefinition {
                                frequency_hz: 4_000.0,
                                bandwidth_hz: 140.0,
                                gain_db: -15.0,
                            },
                            crate::definition::FormantBandDefinition {
                                frequency_hz: 5_000.0,
                                bandwidth_hz: 160.0,
                                gain_db: -20.0,
                            },
                        ],
                    },
                ],
            });
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let first_events: Vec<ProcessEvent> = (0usize..16)
            .map(|index| ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: u64::try_from(index).expect("voice index fits in note id") + 1,
                    note_number: 48
                        + u8::try_from(index).expect("voice index fits in MIDI note number"),
                    velocity: 100,
                },
            })
            .collect();
        let steal_event = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 17,
                note_number: 72,
                velocity: 100,
            },
        }];
        process_with_stack_output(&mut runtime, 0, &first_events);
        runtime.reset().expect("reset");
        process_with_stack_output(&mut runtime, 0, &first_events);

        let allocations = crate::test_allocator::count_allocations(|| {
            process_with_stack_output(&mut runtime, 64, &steal_event);
        });

        assert_eq!(allocations, 0);
    }

    #[test]
    fn spectral_sixteen_voice_stereo_morph_render_does_not_allocate_after_prepare() {
        let definition_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/instruments/spectral-generator-reference.json");
        let mut source: crate::definition::InstrumentDefinition = serde_json::from_str(
            &std::fs::read_to_string(definition_path).expect("spectral reference reads"),
        )
        .expect("spectral reference parses");
        let asset_directory =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/assets");
        let crate::definition::GeneratorDefinition::Spectral(spectral) =
            &mut source.layers[0].generator
        else {
            panic!("spectral reference uses a Spectral generator");
        };
        spectral.asset_a.path = asset_directory
            .join("spectral-reference-a.wav")
            .to_string_lossy()
            .into_owned();
        spectral.asset_b = Some(crate::definition::AssetReference {
            path: asset_directory
                .join("spectral-reference-b.wav")
                .to_string_lossy()
                .into_owned(),
            sha256: None,
        });
        source.performance.polyphony = 16;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let first_events: Vec<ProcessEvent> = (0usize..16)
            .map(|index| ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: u64::try_from(index).expect("voice index fits in note id") + 1,
                    note_number: 48
                        + u8::try_from(index).expect("voice index fits in MIDI note number"),
                    velocity: 100,
                },
            })
            .collect();
        let steal_event = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 17,
                note_number: 72,
                velocity: 100,
            },
        }];
        process_with_stack_output(&mut runtime, 0, &first_events);
        runtime.reset().expect("reset");
        process_with_stack_output(&mut runtime, 0, &first_events);

        let allocations = crate::test_allocator::count_allocations(|| {
            process_with_stack_output(&mut runtime, 64, &steal_event);
        });

        assert_eq!(allocations, 0);
    }

    #[test]
    fn voice_stealing_note_on_does_not_allocate_after_prepare() {
        let mut source = definition();
        source.performance.polyphony = 1;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let first_event = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];
        let second_event = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 64,
                velocity: 100,
            },
        }];

        let _ = process(&mut runtime, 64, 0, &first_event);
        let _ = process(&mut runtime, 64, 64, &second_event);
        runtime.reset().expect("reset");
        let _ = process(&mut runtime, 64, 0, &first_event);

        let allocations = crate::test_allocator::count_allocations(|| {
            process_with_stack_output(&mut runtime, 64, &second_event);
        });

        assert_eq!(allocations, 0);
    }

    #[test]
    fn note_on_activates_only_layers_matching_the_note() {
        let single_layer = definition();
        let mut layered = single_layer.clone();
        let mut non_matching = layered.layers[0].clone();
        non_matching.id = "non_matching".to_owned();
        non_matching.trigger.key_min = 72;
        non_matching.trigger.key_max = 72;
        non_matching.gain_db = 0.0;
        non_matching.envelope.attack_seconds = 0.0;
        non_matching.envelope.decay_seconds = 0.0;
        non_matching.envelope.sustain_level = 1.0;
        layered.layers.push(non_matching);

        let mut single_runtime = runtime_with(&single_layer);
        let mut layered_runtime = runtime_with(&layered);
        prepare(&mut single_runtime);
        prepare(&mut layered_runtime);
        let event = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];

        let single_audio = process(&mut single_runtime, 128, 0, &event);
        let layered_audio = process(&mut layered_runtime, 128, 0, &event);

        assert_eq!(single_audio[0], layered_audio[0]);
        assert_eq!(single_audio[1], layered_audio[1]);
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
    fn reset_restarts_round_robin_selection_from_the_first_zone() {
        let asset_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/assets/metal-hit.wav")
            .to_string_lossy()
            .into_owned();
        let asset = |start_seconds, end_seconds| crate::definition::SampleZoneDefinition {
            id: if start_seconds == 0.0 {
                "hit_a".to_owned()
            } else {
                "hit_b".to_owned()
            },
            asset: crate::definition::AssetReference {
                path: asset_path.clone(),
                sha256: None,
            },
            root_note: 60,
            key_min: 0,
            key_max: 127,
            velocity_min: 1,
            velocity_max: 127,
            round_robin_group: Some("hits".to_owned()),
            playback: crate::definition::SampleZonePlaybackDefinition {
                region: crate::definition::SampleRegionDefinition {
                    start_seconds,
                    end_seconds: Some(end_seconds),
                },
                direction: crate::definition::SamplePlaybackDirection::Forward,
                r#loop: None,
                time: crate::definition::SampleTimeDefinition::Resample,
            },
        };
        let mut source = definition();
        source.layers[0].generator =
            crate::definition::GeneratorDefinition::Sample(crate::definition::SampleDefinition {
                interpolation: crate::definition::SampleInterpolation::Cubic,
                zones: vec![asset(0.0, 0.08), asset(0.08, 0.16)],
            });
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let event = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 3,
                note_number: 60,
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

    fn phase_runtime_with_waveform(
        phase_reset: bool,
        waveform: crate::definition::OscillatorWaveform,
    ) -> InstrumentRuntime {
        let mut source = definition();
        source.performance.polyphony = 1;
        source.layers[0].envelope.attack_seconds = 0.0;
        source.layers[0].envelope.decay_seconds = 0.0;
        source.layers[0].envelope.sustain_level = 1.0;
        source.layers[0].envelope.release_seconds = 0.001;
        match &mut source.layers[0].generator {
            crate::definition::GeneratorDefinition::Oscillator(oscillator) => {
                oscillator.phase_reset = phase_reset;
                oscillator.waveform = waveform;
            }
            crate::definition::GeneratorDefinition::Sample(_)
            | crate::definition::GeneratorDefinition::Granular(_)
            | crate::definition::GeneratorDefinition::WaveSequence(_)
            | crate::definition::GeneratorDefinition::Noise(_)
            | crate::definition::GeneratorDefinition::Additive(_)
            | crate::definition::GeneratorDefinition::Formant(_)
            | crate::definition::GeneratorDefinition::Wavetable(_)
            | crate::definition::GeneratorDefinition::Spectral(_)
            | crate::definition::GeneratorDefinition::OperatorModulation(_) => {
                panic!("test fixture must use an oscillator");
            }
        }
        runtime_with(&source)
    }

    fn phase_runtime(phase_reset: bool) -> InstrumentRuntime {
        phase_runtime_with_waveform(phase_reset, crate::definition::OscillatorWaveform::Sine)
    }

    fn modulated_steal_definition() -> crate::definition::InstrumentDefinition {
        let mut source = definition();
        source.performance.polyphony = 1;
        source.layers[0].envelope.attack_seconds = 0.0;
        source.layers[0].envelope.decay_seconds = 0.0;
        source.layers[0].envelope.sustain_level = 1.0;
        source.layers[0].envelope.release_seconds = 0.25;
        source
            .voice_processors
            .push(crate::definition::ProcessorDefinition::Filter(
                crate::definition::FilterProcessorDefinition {
                    id: "tone".to_owned(),
                    cutoff_hz: 2_000.0,
                    resonance: 0.15,
                },
            ));
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
                    target: "voice.processor.tone.cutoff".to_owned(),
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
    fn triangle_retrigger_after_release_matches_first_render() {
        let mut runtime =
            phase_runtime_with_waveform(true, crate::definition::OscillatorWaveform::Triangle);
        prepare(&mut runtime);
        let first_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 127,
            },
        }];
        let first = process(&mut runtime, 64, 0, &first_note);
        let note_off = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        }];
        let _ = process(&mut runtime, 64, 64, &note_off);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Idle));

        let second_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 60,
                velocity: 127,
            },
        }];
        let second = process(&mut runtime, 64, 128, &second_note);
        for (first, second) in first[0].iter().zip(&second[0]) {
            assert_relative_eq!(*first, *second, epsilon = 1.0e-6);
        }
    }

    #[test]
    fn triangle_instrument_reset_matches_a_fresh_runtime() {
        let mut runtime =
            phase_runtime_with_waveform(false, crate::definition::OscillatorWaveform::Triangle);
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
        for (first, second) in first[0].iter().zip(&second[0]) {
            assert_relative_eq!(*first, *second, epsilon = 1.0e-6);
        }
    }

    #[test]
    fn triangle_voice_stealing_starts_from_the_compiled_phase() {
        let mut stolen =
            phase_runtime_with_waveform(true, crate::definition::OscillatorWaveform::Triangle);
        let mut direct =
            phase_runtime_with_waveform(true, crate::definition::OscillatorWaveform::Triangle);
        prepare(&mut stolen);
        prepare(&mut direct);
        let first_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 127,
            },
        }];
        let _ = process(&mut stolen, 64, 0, &first_note);

        let pending_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 60,
                velocity: 127,
            },
        }];
        let stolen_audio = process(&mut stolen, 256, 64, &pending_note);
        let direct_audio = process(
            &mut direct,
            16,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 3,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        assert_eq!(stolen.voice_state(0), Some(VoiceState::Active));
        for (stolen_sample, direct_sample) in stolen_audio[0][240..].iter().zip(&direct_audio[0]) {
            assert_relative_eq!(*stolen_sample, *direct_sample, epsilon = 1.0e-6);
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

    #[test]
    fn global_feedback_targets_remain_within_descriptor_limits() {
        let mut source = definition();
        source.global_processors = vec![
            crate::definition::ProcessorDefinition::Delay(
                crate::definition::DelayProcessorDefinition {
                    id: "echo".to_owned(),
                    time_seconds: 0.25,
                    feedback: 0.5,
                    mix: 0.5,
                },
            ),
            crate::definition::ProcessorDefinition::Reverb(
                crate::definition::ReverbProcessorDefinition {
                    id: "space".to_owned(),
                    pre_delay_seconds: 0.02,
                    decay: 0.5,
                    damping: 0.2,
                    width: 1.0,
                    mix: 0.3,
                },
            ),
        ];
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![],
            routes: vec![
                crate::definition::ModulationRouteDefinition {
                    source: "mod_wheel".to_owned(),
                    target: "global.processor.echo.feedback".to_owned(),
                    amount: 1.0,
                    curve: crate::definition::ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "mod_wheel".to_owned(),
                    target: "global.processor.space.decay".to_owned(),
                    amount: 1.0,
                    curve: crate::definition::ModulationCurve::Linear,
                },
            ],
        });
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let delay_feedback = runtime
            .compiled()
            .parameter_handle("global.processor.echo.feedback")
            .expect("delay feedback handle");
        let reverb_decay = runtime
            .compiled()
            .parameter_handle("global.processor.space.decay")
            .expect("reverb decay handle");

        let events = [
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::ParameterChange {
                    parameter: delay_feedback,
                    normalized: 1.0,
                },
            },
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::ParameterChange {
                    parameter: reverb_decay,
                    normalized: 1.0,
                },
            },
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::ModWheel { value: 1.0 },
            },
        ];
        let _ = process(&mut runtime, 257, 0, &events);
        let empty: [ProcessEvent; 0] = [];
        let _ = process(&mut runtime, 257, 257, &empty);
        let _ = process(&mut runtime, 257, 514, &empty);
        let _ = process(&mut runtime, 257, 771, &empty);

        match runtime.global_targets[0] {
            ProcessorTargetSpan::Delay { feedback, .. } => {
                assert!(feedback.start <= 0.95);
                assert!(feedback.end <= 0.95);
                assert!(feedback.end > 0.949);
            }
            _ => panic!("first global processor must be delay"),
        }
        match runtime.global_targets[1] {
            ProcessorTargetSpan::Reverb { decay, .. } => {
                assert!(decay.start <= 0.98);
                assert!(decay.end <= 0.98);
                assert!(decay.end > 0.979);
            }
            _ => panic!("second global processor must be reverb"),
        }
    }
}
