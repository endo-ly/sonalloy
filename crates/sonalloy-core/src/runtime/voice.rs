use crate::compiler::{
    CompiledGenerator, CompiledInstrument, CompiledLayer, CompiledProcessorKind, CompiledSourceRef,
    CompiledVoiceSource, GeneratorOutputMode, db_to_linear,
};
use crate::definition::LfoWaveform;
use crate::parameter::ParameterScale;
use crate::process::{NoteId, ProcessError, ProcessSpec};

use super::adsr::AdsrRuntime;
use super::generator::GeneratorRuntime;
use super::mix::constant_power_pan;
use super::modulation::{
    LayerGeneratorTargetSpan, LayerTargetSpan, SharedParameterSpan, ValueSpan, VoiceTargetScratch,
};
use super::processor::{LayerProcessorChain, ProcessorTargetSpan, StereoProcessorChain};
use super::random::{bipolar_f32, splitmix64_finalizer};
use super::smoothing::{Smoother, rounded_frame_count};

const GAIN_SMOOTHING_SECONDS: f64 = 0.005;

/// Runtime state of one polyphonic voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    /// No note is assigned.
    Idle,
    /// A note is held or its envelope is sustaining.
    Active,
    /// Note Off has started the release envelopes.
    Releasing,
    /// The old note is fading before a pending note starts.
    StealFading,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct NoteRequest {
    pub(crate) note_id: NoteId,
    pub(crate) note_number: u8,
    pub(crate) velocity: u8,
    pub(crate) started_at_frame: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreparedLayerSelection {
    Inactive,
    Armed { sample_zone: Option<usize> },
    Active { sample_zone: Option<usize> },
}

impl NoteRequest {
    pub(crate) fn new(
        note_id: NoteId,
        note_number: u8,
        velocity: u8,
        started_at_frame: u64,
    ) -> Self {
        Self {
            note_id,
            note_number,
            velocity,
            started_at_frame,
        }
    }
}

struct LayerRuntime {
    envelope: AdsrRuntime,
    generator: GeneratorRuntime,
    output_mode: GeneratorOutputMode,
    processors: LayerProcessorChain,
    active: bool,
    armed: bool,
    armed_sample_zone: Option<usize>,
    note_start_fade: Smoother,
    note_start_fade_frames: usize,
    instrument_latency_frames: usize,
    delay: LayerDelayCompensation,
}

struct LayerDelayCompensation {
    delay_frames: usize,
    left: Vec<f32>,
    right: Vec<f32>,
    position: usize,
    pending_frames: usize,
}

impl LayerDelayCompensation {
    fn new(delay_frames: usize) -> Self {
        Self {
            delay_frames,
            left: vec![0.0; delay_frames],
            right: vec![0.0; delay_frames],
            position: 0,
            pending_frames: 0,
        }
    }

    fn reset(&mut self) {
        self.left.fill(0.0);
        self.right.fill(0.0);
        self.position = 0;
        self.pending_frames = 0;
    }

    fn has_pending(&self) -> bool {
        self.pending_frames > 0
    }

    fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        if self.delay_frames == 0 {
            return (left, right);
        }
        let output = (self.left[self.position], self.right[self.position]);
        self.left[self.position] = left;
        self.right[self.position] = right;
        self.position = (self.position + 1) % self.delay_frames;
        self.pending_frames = self.pending_frames.saturating_add(1).min(self.delay_frames);
        output
    }

    fn process_silence(&mut self) -> (f32, f32) {
        if self.delay_frames == 0 || self.pending_frames == 0 {
            return (0.0, 0.0);
        }
        let output = (self.left[self.position], self.right[self.position]);
        self.left[self.position] = 0.0;
        self.right[self.position] = 0.0;
        self.position = (self.position + 1) % self.delay_frames;
        self.pending_frames -= 1;
        output
    }

    fn set_delay_frames(&mut self, delay_frames: usize) {
        debug_assert!(delay_frames <= self.left.len());
        self.delay_frames = delay_frames;
        self.position = 0;
        self.pending_frames = 0;
    }
}

impl LayerRuntime {
    fn new(
        compiled: &CompiledLayer,
        spec: ProcessSpec,
        instrument_latency_frames: usize,
    ) -> Result<Self, ProcessError> {
        let generator = GeneratorRuntime::new(&compiled.generator, spec)?;
        let note_start_fade_frames =
            rounded_frame_count(spec.sample_rate * GAIN_SMOOTHING_SECONDS).max(1);
        let processors = LayerProcessorChain::new(&compiled.processors, spec)?;
        Ok(Self {
            envelope: AdsrRuntime::new(compiled.envelope),
            output_mode: compiled.generator.output_mode(),
            generator,
            processors,
            active: false,
            armed: false,
            armed_sample_zone: None,
            note_start_fade: Smoother::new(0.0),
            note_start_fade_frames,
            instrument_latency_frames,
            delay: LayerDelayCompensation::new(instrument_latency_frames),
        })
    }

    fn start(
        &mut self,
        note: NoteRequest,
        sample_zone: Option<usize>,
        compiled: &CompiledLayer,
    ) -> Result<(), ProcessError> {
        self.generator
            .start(note.note_id, sample_zone, &compiled.generator)?;
        self.delay.reset();
        self.delay.set_delay_frames(
            self.instrument_latency_frames
                .saturating_sub(self.generator.intrinsic_latency_frames()),
        );
        self.armed = false;
        self.armed_sample_zone = None;
        self.envelope.note_on();
        self.note_start_fade.reset(0.0);
        self.note_start_fade
            .set_target(1.0, self.note_start_fade_frames);
        self.active = true;
        Ok(())
    }

    fn arm(&mut self, sample_zone: Option<usize>) {
        self.active = false;
        self.armed = true;
        self.armed_sample_zone = sample_zone;
    }

    fn start_armed(
        &mut self,
        note: NoteRequest,
        compiled: &CompiledLayer,
    ) -> Result<(), ProcessError> {
        let sample_zone = self.armed_sample_zone.take();
        self.start(note, sample_zone, compiled)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_source(
        &mut self,
        frames: usize,
        note_number: u8,
        tuning_start: f32,
        tuning_end: f32,
        sample_rate: f64,
        tempo_bpm: f64,
        targets: LayerGeneratorTargetSpan,
        mono: &mut [f32],
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<bool, ProcessError> {
        self.generator.render(
            frames,
            note_number,
            tuning_start,
            tuning_end,
            sample_rate,
            tempo_bpm,
            targets,
            mono,
            left,
            right,
        )
    }

    fn note_off(&mut self) {
        self.generator.note_off();
        self.envelope.note_off();
    }

    fn reset(&mut self) -> Result<(), ProcessError> {
        self.generator.reset()?;
        self.processors.reset()?;
        self.delay.reset();
        self.envelope.reset();
        self.note_start_fade.reset(0.0);
        self.active = false;
        self.armed = false;
        self.armed_sample_zone = None;
        Ok(())
    }

    fn reset_state(&mut self) -> Result<(), ProcessError> {
        self.processors.reset()?;
        self.delay.reset();
        self.envelope.reset();
        self.note_start_fade.reset(0.0);
        self.active = false;
        self.armed = false;
        self.armed_sample_zone = None;
        Ok(())
    }
}

enum VoiceSourceRuntime {
    Velocity(f32),
    KeyTracking(f32),
    Lfo { phase: f32 },
    Envelope(AdsrRuntime),
    Random(f32),
}

/// One prepared voice and its owned DSP and source state.
pub(crate) struct VoiceRuntime {
    state: VoiceState,
    note_id: Option<NoteId>,
    note_number: u8,
    velocity: u8,
    started_at_frame: u64,
    estimated_level: f32,
    layers: Vec<LayerRuntime>,
    processors: StereoProcessorChain,
    source_states: Vec<VoiceSourceRuntime>,
    source_spans: Vec<ValueSpan>,
    source_definitions: Vec<CompiledVoiceSource>,
    source_used: Vec<bool>,
    targets: VoiceTargetScratch,
    pending: Option<NoteRequest>,
    pending_layer_selection: Vec<PreparedLayerSelection>,
    steal_fade_total: usize,
    steal_fade_remaining: usize,
}

impl VoiceRuntime {
    pub(crate) fn new(
        compiled: &CompiledInstrument,
        spec: ProcessSpec,
    ) -> Result<Self, ProcessError> {
        let layers = compiled
            .layers
            .iter()
            .map(|layer| LayerRuntime::new(layer, spec, compiled.reported_latency_frames))
            .collect::<Result<Vec<_>, _>>()?;
        let processors = StereoProcessorChain::new(&compiled.voice_processors, spec)?;
        let source_definitions = compiled
            .sources
            .iter()
            .map(|source| source.source.clone())
            .collect::<Vec<_>>();
        let source_states = source_definitions
            .iter()
            .map(|source| match source {
                CompiledVoiceSource::Velocity => VoiceSourceRuntime::Velocity(0.0),
                CompiledVoiceSource::KeyTracking => VoiceSourceRuntime::KeyTracking(-1.0),
                CompiledVoiceSource::Lfo(value) => VoiceSourceRuntime::Lfo { phase: value.phase },
                CompiledVoiceSource::Envelope(value) => {
                    VoiceSourceRuntime::Envelope(AdsrRuntime::new(value.envelope))
                }
                CompiledVoiceSource::Random(_) => VoiceSourceRuntime::Random(0.0),
            })
            .collect::<Vec<_>>();
        let source_spans = vec![
            ValueSpan {
                start: 0.0,
                end: 0.0
            };
            source_definitions.len()
        ];
        let mut source_used = vec![false; source_definitions.len()];
        for route in &compiled.routes {
            if let CompiledSourceRef::Voice(handle) = route.source {
                if let Some(used) = source_used.get_mut(handle.index()) {
                    *used = true;
                }
            }
        }
        Ok(Self {
            state: VoiceState::Idle,
            note_id: None,
            note_number: 0,
            velocity: 0,
            started_at_frame: 0,
            estimated_level: 0.0,
            layers,
            processors,
            source_states,
            source_spans,
            source_definitions,
            source_used,
            targets: VoiceTargetScratch::new(&compiled.layers, &compiled.voice_processors),
            pending: None,
            pending_layer_selection: vec![PreparedLayerSelection::Inactive; compiled.layers.len()],
            steal_fade_total: 0,
            steal_fade_remaining: 0,
        })
    }

    pub(crate) fn state(&self) -> VoiceState {
        self.state
    }

    pub(crate) fn started_at_frame(&self) -> u64 {
        self.started_at_frame
    }

    pub(crate) fn estimated_level(&self) -> f32 {
        self.estimated_level
    }

    #[cfg(test)]
    pub(super) fn pending_layer_selection_capacity(&self) -> usize {
        self.pending_layer_selection.capacity()
    }

    pub(crate) fn request_note(
        &mut self,
        compiled: &CompiledInstrument,
        request: NoteRequest,
        layer_selection: &[PreparedLayerSelection],
        fade_frames: usize,
    ) -> Result<(), ProcessError> {
        if layer_selection.len() != self.layers.len()
            || self.pending_layer_selection.len() != self.layers.len()
        {
            return Err(invalid_state());
        }
        if self.state == VoiceState::Idle {
            self.start_note(compiled, request, layer_selection)?;
            return Ok(());
        }
        self.pending_layer_selection
            .copy_from_slice(layer_selection);
        self.pending = Some(request);
        self.state = VoiceState::StealFading;
        self.steal_fade_total = fade_frames;
        self.steal_fade_remaining = fade_frames;
        if fade_frames == 0 {
            self.complete_steal(compiled)?;
        }
        Ok(())
    }

    pub(crate) fn release_note(
        &mut self,
        compiled: &CompiledInstrument,
        note_id: NoteId,
    ) -> Result<(), ProcessError> {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.note_id == note_id)
        {
            self.pending = None;
        }
        if self.note_id != Some(note_id) {
            return Ok(());
        }
        if matches!(self.state, VoiceState::Active) {
            let note = NoteRequest::new(
                note_id,
                self.note_number,
                self.velocity,
                self.started_at_frame,
            );
            for (index, layer) in self.layers.iter_mut().enumerate() {
                if layer.active {
                    layer.note_off();
                } else if layer.armed {
                    let compiled_layer = compiled.layers.get(index).ok_or_else(invalid_state)?;
                    layer.start_armed(note, compiled_layer)?;
                }
            }
            for state in &mut self.source_states {
                if let VoiceSourceRuntime::Envelope(envelope) = state {
                    envelope.note_off();
                }
            }
            self.state = VoiceState::Releasing;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_span(
        &mut self,
        frames: usize,
        sample_rate: f64,
        tempo_bpm: f64,
        compiled: &CompiledInstrument,
        shared: SharedParameterSpan<'_>,
        layer_mono: &mut [f32],
        layer_left: &mut [f32],
        layer_right: &mut [f32],
        voice_left: &mut [f32],
        voice_right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if frames == 0 {
            return Ok(());
        }
        if self.layers.len() != compiled.layers.len()
            || self.targets.layers.len() != compiled.layers.len()
            || self.targets.layer_processors.len() != compiled.layers.len()
            || self.targets.voice_processors.len() != compiled.voice_processors.len()
            || self.source_states.len() != self.source_definitions.len()
            || self.source_spans.len() != self.source_definitions.len()
            || self.source_used.len() != self.source_definitions.len()
            || layer_mono.len() < frames
            || layer_left.len() < frames
            || layer_right.len() < frames
            || voice_left.len() < frames
            || voice_right.len() < frames
            || self
                .targets
                .layer_processors
                .iter()
                .zip(&compiled.layers)
                .any(|(targets, layer)| targets.len() != layer.processors.len())
        {
            return Err(invalid_state());
        }
        layer_mono[..frames].fill(0.0);
        layer_left[..frames].fill(0.0);
        layer_right[..frames].fill(0.0);
        voice_left[..frames].fill(0.0);
        voice_right[..frames].fill(0.0);
        if self.state == VoiceState::Idle {
            return Ok(());
        }
        let mut peak = 0.0_f32;
        let mut offset = 0;
        while offset < frames {
            if self.state == VoiceState::Idle {
                break;
            }
            let mut chunk = self.next_voice_boundary(frames - offset, sample_rate);
            if self.state == VoiceState::StealFading {
                chunk = chunk.min(self.steal_fade_remaining);
            }
            if chunk == 0 {
                if self.state == VoiceState::StealFading {
                    self.complete_steal(compiled)?;
                    continue;
                }
                chunk = 1.min(frames - offset);
            }
            let subspan = shared.subspan(offset, chunk);
            self.advance_source_spans(chunk, sample_rate);
            self.evaluate_targets(compiled, subspan)?;
            self.render_active_segment(
                chunk,
                sample_rate,
                tempo_bpm,
                &mut layer_mono[offset..offset + chunk],
                &mut layer_left[offset..offset + chunk],
                &mut layer_right[offset..offset + chunk],
                &mut voice_left[offset..offset + chunk],
                &mut voice_right[offset..offset + chunk],
            )?;
            if self.state == VoiceState::StealFading {
                #[allow(clippy::cast_precision_loss)]
                let total = self.steal_fade_total.max(1) as f32;
                for index in offset..offset + chunk {
                    #[allow(clippy::cast_precision_loss)]
                    let gain = self.steal_fade_remaining as f32 / total;
                    voice_left[index] *= gain;
                    voice_right[index] *= gain;
                    self.steal_fade_remaining = self.steal_fade_remaining.saturating_sub(1);
                    peak = peak
                        .max(voice_left[index].abs())
                        .max(voice_right[index].abs());
                }
            } else {
                for index in offset..offset + chunk {
                    peak = peak
                        .max(voice_left[index].abs())
                        .max(voice_right[index].abs());
                }
            }
            offset += chunk;
            if !self.has_active_layer() {
                if self.state == VoiceState::StealFading {
                    self.complete_steal(compiled)?;
                } else {
                    self.reset_to_idle()?;
                }
            } else if self.state == VoiceState::StealFading && self.steal_fade_remaining == 0 {
                self.complete_steal(compiled)?;
            }
        }
        self.estimated_level = self.estimated_level.mul_add(0.95, peak * 0.05);
        Ok(())
    }

    pub(crate) fn reset(&mut self) -> Result<(), ProcessError> {
        for layer in &mut self.layers {
            layer.reset()?;
        }
        self.processors.reset()?;
        self.clear_assignment_state();
        Ok(())
    }

    fn has_active_layer(&self) -> bool {
        self.layers
            .iter()
            .any(|layer| layer.active || layer.armed || layer.delay.has_pending())
    }

    fn start_note(
        &mut self,
        compiled: &CompiledInstrument,
        request: NoteRequest,
        layer_selection: &[PreparedLayerSelection],
    ) -> Result<(), ProcessError> {
        self.reset_note_state()?;
        self.activate_note(compiled, request, layer_selection)
    }

    fn activate_note(
        &mut self,
        compiled: &CompiledInstrument,
        note: NoteRequest,
        layer_selection: &[PreparedLayerSelection],
    ) -> Result<(), ProcessError> {
        if layer_selection.len() != self.layers.len() {
            return Err(invalid_state());
        }
        self.note_id = Some(note.note_id);
        self.note_number = note.note_number;
        self.velocity = note.velocity;
        self.started_at_frame = note.started_at_frame;
        for (index, (layer, selection)) in self.layers.iter_mut().zip(layer_selection).enumerate() {
            let compiled_layer = compiled.layers.get(index).ok_or_else(invalid_state)?;
            match selection {
                PreparedLayerSelection::Armed { sample_zone } => layer.arm(*sample_zone),
                PreparedLayerSelection::Active { sample_zone } => {
                    layer.start(note, *sample_zone, compiled_layer)?;
                }
                PreparedLayerSelection::Inactive => {}
            }
        }
        self.initialize_source_state(note);
        if !self.has_active_layer() {
            self.reset_to_idle()?;
            return Ok(());
        }
        self.state = VoiceState::Active;
        self.estimated_level = 0.0;
        Ok(())
    }

    fn complete_steal(&mut self, compiled: &CompiledInstrument) -> Result<(), ProcessError> {
        let pending = self.pending.take();
        self.note_id = None;
        self.state = VoiceState::Idle;
        self.steal_fade_total = 0;
        self.steal_fade_remaining = 0;
        if let Some(request) = pending {
            self.activate_pending_note(compiled, request)?;
        } else {
            self.reset_to_idle()?;
        }
        Ok(())
    }

    fn activate_pending_note(
        &mut self,
        compiled: &CompiledInstrument,
        request: NoteRequest,
    ) -> Result<(), ProcessError> {
        let pending_layer_selection = std::mem::take(&mut self.pending_layer_selection);
        let result = (|| {
            self.reset_note_state()?;
            self.activate_note(compiled, request, &pending_layer_selection)
        })();
        self.pending_layer_selection = pending_layer_selection;
        result
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_lines)]
    fn render_active_segment(
        &mut self,
        frames: usize,
        sample_rate: f64,
        tempo_bpm: f64,
        layer_mono: &mut [f32],
        layer_left: &mut [f32],
        layer_right: &mut [f32],
        voice_left: &mut [f32],
        voice_right: &mut [f32],
    ) -> Result<(), ProcessError> {
        voice_left[..frames].fill(0.0);
        voice_right[..frames].fill(0.0);
        let targets = &self.targets;
        for (index, layer) in self.layers.iter_mut().enumerate() {
            if !layer.active && !layer.delay.has_pending() {
                continue;
            }
            let was_active = layer.active;
            let target = targets.layers[index];
            layer_mono[..frames].fill(0.0);
            layer_left[..frames].fill(0.0);
            layer_right[..frames].fill(0.0);
            let generator_finished = if was_active {
                let finished = layer.render_source(
                    frames,
                    self.note_number,
                    target.tuning.start,
                    target.tuning.end,
                    sample_rate,
                    tempo_bpm,
                    target.generator,
                    layer_mono,
                    layer_left,
                    layer_right,
                )?;
                match layer.output_mode {
                    GeneratorOutputMode::Mono => layer.processors.process_mono(
                        &targets.layer_processors[index],
                        &mut layer_mono[..frames],
                    )?,
                    GeneratorOutputMode::Stereo => layer.processors.process_stereo(
                        &targets.layer_processors[index],
                        &mut layer_left[..frames],
                        &mut layer_right[..frames],
                    )?,
                }
                finished
            } else {
                true
            };
            let gain = ValueSpan {
                start: db_to_linear(target.gain.start),
                end: db_to_linear(target.gain.end),
            };
            let (mono_left_start, mono_right_start) = constant_power_pan(target.pan.start);
            let (mono_left_end, mono_right_end) = constant_power_pan(target.pan.end);
            let mono_left = ValueSpan {
                start: mono_left_start,
                end: mono_left_end,
            };
            let mono_right = ValueSpan {
                start: mono_right_start,
                end: mono_right_end,
            };
            let (stereo_left_start, stereo_right_start) =
                super::mix::stereo_balance(target.pan.start);
            let (stereo_left_end, stereo_right_end) = super::mix::stereo_balance(target.pan.end);
            let stereo_left = ValueSpan {
                start: stereo_left_start,
                end: stereo_left_end,
            };
            let stereo_right = ValueSpan {
                start: stereo_right_start,
                end: stereo_right_end,
            };
            for frame in 0..frames {
                let (input_left, input_right) = if was_active {
                    let envelope = layer.envelope.next_sample();
                    let fade = layer.note_start_fade.next();
                    let amplitude = envelope * fade * gain.value_at(frame, frames);
                    match layer.output_mode {
                        GeneratorOutputMode::Mono => {
                            let mono = layer_mono[frame] * amplitude;
                            (
                                mono * mono_left.value_at(frame, frames),
                                mono * mono_right.value_at(frame, frames),
                            )
                        }
                        GeneratorOutputMode::Stereo => (
                            layer_left[frame] * amplitude * stereo_left.value_at(frame, frames),
                            layer_right[frame] * amplitude * stereo_right.value_at(frame, frames),
                        ),
                    }
                } else {
                    (0.0, 0.0)
                };
                let (output_left, output_right) = if was_active {
                    layer.delay.process(input_left, input_right)
                } else {
                    layer.delay.process_silence()
                };
                voice_left[frame] += output_left;
                voice_right[frame] += output_right;
            }
            if was_active && (layer.envelope.is_idle() || generator_finished) {
                layer.active = false;
            }
        }
        self.processors.process(
            &targets.voice_processors,
            &mut voice_left[..frames],
            &mut voice_right[..frames],
        )?;
        Ok(())
    }

    fn reset_to_idle(&mut self) -> Result<(), ProcessError> {
        for layer in &mut self.layers {
            layer.reset_state()?;
        }
        self.processors.reset()?;
        self.clear_assignment_state();
        Ok(())
    }

    fn clear_assignment_state(&mut self) {
        self.state = VoiceState::Idle;
        self.note_id = None;
        self.note_number = 0;
        self.velocity = 0;
        self.estimated_level = 0.0;
        self.pending = None;
        self.steal_fade_total = 0;
        self.steal_fade_remaining = 0;
        self.reset_source_state();
    }

    fn reset_note_state(&mut self) -> Result<(), ProcessError> {
        for layer in &mut self.layers {
            layer.reset_state()?;
        }
        self.processors.reset()?;
        self.reset_source_state();
        Ok(())
    }

    fn initialize_source_state(&mut self, note: NoteRequest) {
        for (index, (definition, state)) in self
            .source_definitions
            .iter()
            .zip(&mut self.source_states)
            .enumerate()
        {
            if !self.source_used[index] {
                continue;
            }
            match (definition, state) {
                (CompiledVoiceSource::Velocity, VoiceSourceRuntime::Velocity(value)) => {
                    *value = f32::from(note.velocity) / 127.0;
                }
                (CompiledVoiceSource::KeyTracking, VoiceSourceRuntime::KeyTracking(value)) => {
                    *value = f32::from(note.note_number) / 127.0 * 2.0 - 1.0;
                }
                (CompiledVoiceSource::Lfo(value), VoiceSourceRuntime::Lfo { phase }) => {
                    *phase = value.phase;
                }
                (CompiledVoiceSource::Envelope(_), VoiceSourceRuntime::Envelope(envelope)) => {
                    envelope.note_on();
                }
                (CompiledVoiceSource::Random(value), VoiceSourceRuntime::Random(random)) => {
                    *random = deterministic_random(value.seed, note.note_id, value.source_hash);
                }
                _ => {}
            }
        }
    }

    fn reset_source_state(&mut self) {
        for (definition, state) in self.source_definitions.iter().zip(&mut self.source_states) {
            match (definition, state) {
                (CompiledVoiceSource::Velocity, VoiceSourceRuntime::Velocity(value)) => {
                    *value = 0.0;
                }
                (CompiledVoiceSource::KeyTracking, VoiceSourceRuntime::KeyTracking(value)) => {
                    *value = -1.0;
                }
                (CompiledVoiceSource::Lfo(value), VoiceSourceRuntime::Lfo { phase }) => {
                    *phase = value.phase;
                }
                (CompiledVoiceSource::Envelope(_), VoiceSourceRuntime::Envelope(envelope)) => {
                    envelope.reset();
                }
                (CompiledVoiceSource::Random(_), VoiceSourceRuntime::Random(random)) => {
                    *random = 0.0;
                }
                _ => {}
            }
        }
    }

    fn advance_source_spans(&mut self, frames: usize, sample_rate: f64) {
        for (((definition, state), span), used) in self
            .source_definitions
            .iter()
            .zip(&mut self.source_states)
            .zip(&mut self.source_spans)
            .zip(&self.source_used)
        {
            if !*used {
                *span = ValueSpan {
                    start: 0.0,
                    end: 0.0,
                };
                continue;
            }
            match (definition, state) {
                (CompiledVoiceSource::Velocity, VoiceSourceRuntime::Velocity(value))
                | (CompiledVoiceSource::KeyTracking, VoiceSourceRuntime::KeyTracking(value))
                | (CompiledVoiceSource::Random(_), VoiceSourceRuntime::Random(value)) => {
                    *span = ValueSpan {
                        start: *value,
                        end: *value,
                    };
                }
                (CompiledVoiceSource::Lfo(value), VoiceSourceRuntime::Lfo { phase }) => {
                    let start_phase = *phase;
                    #[allow(clippy::cast_precision_loss)]
                    let increment = f64::from(value.rate_hz) / sample_rate * frames as f64;
                    #[allow(clippy::cast_possible_truncation)]
                    let end_phase = (f64::from(start_phase) + increment).fract() as f32;
                    *phase = end_phase;
                    *span = ValueSpan {
                        start: lfo_value(value.waveform, start_phase),
                        end: lfo_value(value.waveform, end_phase),
                    };
                }
                (CompiledVoiceSource::Envelope(_), VoiceSourceRuntime::Envelope(envelope)) => {
                    let (start, end) = envelope.span(frames);
                    *span = ValueSpan { start, end };
                }
                _ => {
                    *span = ValueSpan {
                        start: 0.0,
                        end: 0.0,
                    }
                }
            }
        }
    }

    fn evaluate_targets(
        &mut self,
        compiled: &CompiledInstrument,
        shared: SharedParameterSpan<'_>,
    ) -> Result<(), ProcessError> {
        for (index, layer) in compiled.layers.iter().enumerate() {
            if !self.layers[index].active {
                continue;
            }
            let pan = self.evaluate_target(compiled, layer.parameters.pan, shared)?;
            let generator = match &layer.generator {
                CompiledGenerator::Oscillator(value) => {
                    self.evaluate_oscillator_targets(compiled, value, shared)?
                }
                CompiledGenerator::Noise(value) => LayerGeneratorTargetSpan::Noise {
                    correlation: self.evaluate_target(compiled, value.correlation, shared)?,
                },
                CompiledGenerator::Sample(_) => LayerGeneratorTargetSpan::Sample,
                CompiledGenerator::Wavetable(value) => LayerGeneratorTargetSpan::Wavetable {
                    position: self.evaluate_target(compiled, value.parameters.position, shared)?,
                    unison_detune: value
                        .parameters
                        .unison_detune
                        .map(|handle| self.evaluate_target(compiled, handle, shared))
                        .transpose()?,
                    unison_spread: value
                        .parameters
                        .unison_spread
                        .map(|handle| self.evaluate_target(compiled, handle, shared))
                        .transpose()?,
                },
                CompiledGenerator::OperatorModulation(value) => {
                    LayerGeneratorTargetSpan::OperatorModulation {
                        operators: [
                            self.evaluate_operator_target(compiled, value, 0, shared)?,
                            self.evaluate_operator_target(compiled, value, 1, shared)?,
                            self.evaluate_operator_target(compiled, value, 2, shared)?,
                            self.evaluate_operator_target(compiled, value, 3, shared)?,
                        ],
                        unison_detune: value
                            .unison_detune
                            .map(|handle| self.evaluate_target(compiled, handle, shared))
                            .transpose()?,
                        unison_spread: value
                            .unison_spread
                            .map(|handle| self.evaluate_target(compiled, handle, shared))
                            .transpose()?,
                    }
                }
            };
            self.targets.layers[index] = LayerTargetSpan {
                gain: self.evaluate_target(compiled, layer.parameters.gain, shared)?,
                pan,
                tuning: self.evaluate_target(compiled, layer.parameters.tuning, shared)?,
                generator,
            };
            for (processor_index, processor) in layer.processors.iter().enumerate() {
                self.targets.layer_processors[index][processor_index] =
                    self.evaluate_processor_target(compiled, processor, shared)?;
            }
        }
        for (processor_index, processor) in compiled.voice_processors.iter().enumerate() {
            self.targets.voice_processors[processor_index] =
                self.evaluate_processor_target(compiled, processor, shared)?;
        }
        Ok(())
    }

    fn evaluate_oscillator_targets(
        &mut self,
        compiled: &CompiledInstrument,
        oscillator: &crate::compiler::CompiledOscillator,
        shared: SharedParameterSpan<'_>,
    ) -> Result<LayerGeneratorTargetSpan, ProcessError> {
        let sync_ratio = match oscillator.backend {
            crate::compiler::CompiledOscillatorBackend::Basic
            | crate::compiler::CompiledOscillatorBackend::PhaseDomain => None,
            crate::compiler::CompiledOscillatorBackend::VariableShapeSync { sync_ratio } => {
                Some(self.evaluate_target(compiled, sync_ratio, shared)?)
            }
        };
        Ok(LayerGeneratorTargetSpan::Oscillator {
            pulse_width: oscillator
                .parameters
                .pulse_width
                .map(|handle| self.evaluate_target(compiled, handle, shared))
                .transpose()?,
            sync_ratio,
            waveshape: oscillator
                .parameters
                .waveshape
                .map(|handle| self.evaluate_target(compiled, handle, shared))
                .transpose()?,
            phase_distortion: oscillator
                .parameters
                .phase_distortion
                .map(|handle| self.evaluate_target(compiled, handle, shared))
                .transpose()?,
            wavefold: oscillator
                .parameters
                .wavefold
                .map(|handle| self.evaluate_target(compiled, handle, shared))
                .transpose()?,
            oscillator_feedback: oscillator
                .parameters
                .oscillator_feedback
                .map(|handle| self.evaluate_target(compiled, handle, shared))
                .transpose()?,
            unison_detune: oscillator
                .parameters
                .unison_detune
                .map(|handle| self.evaluate_target(compiled, handle, shared))
                .transpose()?,
            unison_spread: oscillator
                .parameters
                .unison_spread
                .map(|handle| self.evaluate_target(compiled, handle, shared))
                .transpose()?,
        })
    }

    fn evaluate_operator_target(
        &self,
        compiled: &CompiledInstrument,
        operator_modulation: &crate::compiler::CompiledOperatorModulation,
        index: usize,
        shared: SharedParameterSpan<'_>,
    ) -> Result<super::modulation::OperatorTargetSpan, ProcessError> {
        let parameters = operator_modulation
            .parameters
            .get(index)
            .copied()
            .ok_or_else(invalid_state)?;
        Ok(super::modulation::OperatorTargetSpan {
            ratio: self.evaluate_target(compiled, parameters.ratio, shared)?,
            detune: self.evaluate_target(compiled, parameters.detune, shared)?,
            level: parameters
                .level
                .map(|handle| self.evaluate_target(compiled, handle, shared))
                .transpose()?,
            modulation_amount: parameters
                .modulation_amount
                .map(|handle| self.evaluate_target(compiled, handle, shared))
                .transpose()?,
            feedback: parameters
                .feedback
                .map(|handle| self.evaluate_target(compiled, handle, shared))
                .transpose()?,
        })
    }

    fn evaluate_processor_target(
        &self,
        compiled: &CompiledInstrument,
        processor: &crate::compiler::CompiledProcessor,
        shared: SharedParameterSpan<'_>,
    ) -> Result<ProcessorTargetSpan, ProcessError> {
        match &processor.processor {
            CompiledProcessorKind::Filter(value) => {
                let cutoff = self.evaluate_target(compiled, value.parameters.cutoff, shared)?;
                Ok(ProcessorTargetSpan::Filter {
                    cutoff: ValueSpan {
                        start: cutoff.start.min(value.effective_max_cutoff_hz),
                        end: cutoff.end.min(value.effective_max_cutoff_hz),
                    },
                    resonance: self.evaluate_target(
                        compiled,
                        value.parameters.resonance,
                        shared,
                    )?,
                })
            }
            CompiledProcessorKind::Drive(value) => Ok(ProcessorTargetSpan::Drive {
                amount: self.evaluate_target(compiled, value.amount, shared)?,
                mix: self.evaluate_target(compiled, value.mix, shared)?,
            }),
            CompiledProcessorKind::Delay(value) => Ok(ProcessorTargetSpan::Delay {
                feedback: self.evaluate_target(compiled, value.feedback, shared)?,
                mix: self.evaluate_target(compiled, value.mix, shared)?,
            }),
            CompiledProcessorKind::Reverb(value) => Ok(ProcessorTargetSpan::Reverb {
                decay: self.evaluate_target(compiled, value.decay, shared)?,
                damping: self.evaluate_target(compiled, value.damping, shared)?,
                width: self.evaluate_target(compiled, value.width, shared)?,
                mix: self.evaluate_target(compiled, value.mix, shared)?,
            }),
        }
    }

    fn evaluate_target(
        &self,
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
        let mut linear_start = 0.0;
        let mut linear_end = 0.0;
        let mut logarithmic_start = 0.0;
        let mut logarithmic_end = 0.0;
        let range = descriptor.max - descriptor.min;
        let log_range = if descriptor.scale == ParameterScale::Log2 {
            (descriptor.max / descriptor.min).log2()
        } else {
            0.0
        };
        let routes = compiled
            .routes_for_checked(handle)
            .ok_or_else(invalid_state)?;
        for route in routes {
            let source = match route.source {
                CompiledSourceRef::Voice(handle) => self
                    .source_spans
                    .get(handle.index())
                    .copied()
                    .ok_or_else(invalid_state)?,
                CompiledSourceRef::PitchBend => shared.pitch_bend(),
                CompiledSourceRef::ModWheel => shared.mod_wheel(),
                CompiledSourceRef::Aftertouch => shared.aftertouch(),
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

    fn next_voice_boundary(&self, remaining: usize, sample_rate: f64) -> usize {
        let mut boundary = remaining;
        for layer in &self.layers {
            if !layer.active {
                continue;
            }
            if let Some(frames) = layer.envelope.frames_until_segment_end() {
                if frames > 0 {
                    boundary = boundary.min(frames);
                }
            }
        }
        for ((definition, state), used) in self
            .source_definitions
            .iter()
            .zip(&self.source_states)
            .zip(&self.source_used)
        {
            if !*used {
                continue;
            }
            if let (CompiledVoiceSource::Lfo(value), VoiceSourceRuntime::Lfo { phase }) =
                (definition, state)
            {
                if value.waveform == LfoWaveform::Triangle {
                    boundary =
                        boundary.min(lfo_boundary(*phase, value.rate_hz, sample_rate, remaining));
                }
            }
            if let VoiceSourceRuntime::Envelope(envelope) = state {
                if let Some(frames) = envelope.frames_until_segment_end() {
                    if frames > 0 {
                        boundary = boundary.min(frames);
                    }
                }
            }
        }
        boundary.max(1).min(remaining)
    }
}

fn lfo_value(waveform: LfoWaveform, phase: f32) -> f32 {
    match waveform {
        LfoWaveform::Sine => (std::f32::consts::TAU * phase).sin(),
        LfoWaveform::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
    }
}

fn invalid_state() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: crate::process::ProcessorFailureKind::InvalidState,
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

fn lfo_boundary(phase: f32, rate_hz: f32, sample_rate: f64, remaining: usize) -> usize {
    #[allow(clippy::cast_precision_loss)]
    let increment = f64::from(rate_hz) / sample_rate;
    if increment <= 0.0 || !increment.is_finite() {
        return remaining;
    }
    let phase = f64::from(phase.fract());
    let next = if phase < 0.5 { 0.5 } else { 1.0 };
    let distance = (next - phase) / increment;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let frames = distance.ceil() as usize;
    frames.max(1).min(remaining)
}

fn deterministic_random(seed: u64, note_id: NoteId, source_hash: u64) -> f32 {
    bipolar_f32(splitmix64_finalizer(seed ^ note_id ^ source_hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfo_waveforms_match_the_definition() {
        assert!((lfo_value(LfoWaveform::Sine, 0.0)).abs() < 1.0e-6);
        assert!((lfo_value(LfoWaveform::Sine, 0.25) - 1.0).abs() < 1.0e-6);
        assert!((lfo_value(LfoWaveform::Sine, 0.5)).abs() < 1.0e-6);
        assert!((lfo_value(LfoWaveform::Triangle, 0.0) + 1.0).abs() < 1.0e-6);
        assert!(lfo_value(LfoWaveform::Triangle, 0.5) - 1.0 < 1.0e-6);
        assert!((lfo_value(LfoWaveform::Triangle, 1.0) + 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn triangle_boundary_is_sample_accurate() {
        assert_eq!(lfo_boundary(0.0, 1.0, 48_000.0, 30_000), 24_000);
        assert_eq!(lfo_boundary(0.5, 1.0, 48_000.0, 128), 128);
        assert_eq!(lfo_boundary(0.9, 1.0, 48_000.0, 128), 128);
    }

    #[test]
    fn smooth_step_preserves_bipolar_sign() {
        assert!(
            (curve_value(0.5, crate::definition::ModulationCurve::SmoothStep) - 0.5).abs() < 1.0e-6
        );
        assert!(
            (curve_value(-0.5, crate::definition::ModulationCurve::SmoothStep) + 0.5).abs()
                < 1.0e-6
        );
        assert!(
            (curve_value(-1.0, crate::definition::ModulationCurve::SmoothStep) + 1.0).abs()
                < 1.0e-6
        );
    }

    #[test]
    fn random_mix_is_stable_and_bipolar() {
        let source_hash = crate::compiler::source_id_hash("voice_pan");
        let value = deterministic_random(8128, 60, source_hash);
        assert!((value - 0.094_552_636).abs() < 1.0e-6);
        assert!((-1.0..=1.0).contains(&value));
        assert!((value - deterministic_random(8128, 60, source_hash)).abs() < f32::EPSILON);
        assert!((value - deterministic_random(8129, 60, source_hash)).abs() > f32::EPSILON);
        assert!((value - deterministic_random(8128, 61, source_hash)).abs() > f32::EPSILON);
        assert!(
            (value - deterministic_random(8128, 60, crate::compiler::source_id_hash("other_pan"),))
                .abs()
                > f32::EPSILON
        );
    }

    #[test]
    fn layer_delay_compensation_preserves_the_declared_frame_delay() {
        let mut delay = LayerDelayCompensation::new(2);
        assert_eq!(delay.process(1.0, -1.0), (0.0, 0.0));
        assert_eq!(delay.process(2.0, -2.0), (0.0, 0.0));
        assert_eq!(delay.process(3.0, -3.0), (1.0, -1.0));
        assert_eq!(delay.process_silence(), (2.0, -2.0));
        assert_eq!(delay.process_silence(), (3.0, -3.0));
        assert!(!delay.has_pending());
    }

    #[test]
    fn layer_delay_compensation_can_change_for_the_selected_generator() {
        let mut delay = LayerDelayCompensation::new(2);
        delay.set_delay_frames(1);
        assert_eq!(delay.process(1.0, -1.0), (0.0, 0.0));
        assert_eq!(delay.process_silence(), (1.0, -1.0));

        delay.set_delay_frames(0);
        assert_eq!(delay.process(2.0, -2.0), (2.0, -2.0));
        assert!(!delay.has_pending());
    }
}
