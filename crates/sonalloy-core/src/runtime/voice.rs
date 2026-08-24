use crate::compiler::{
    CompiledGenerator, CompiledInstrument, CompiledLayer, CompiledProcessorKind, CompiledSourceRef,
    CompiledVoiceSource, GeneratorOutputMode, SourceHandle, db_to_linear,
};
use crate::process::{NoteId, ProcessError, ProcessSpec};

use super::adsr::AdsrRuntime;
use super::generator::GeneratorRuntime;
use super::mix::constant_power_pan;
use super::modulation::{
    LayerGeneratorTargetSpan, LayerTargetSpan, SharedParameterSpan, ValueSpan, VoiceTargetScratch,
    apply_domain_sum_with_maximum, route_domain_delta,
};
use super::processor::{LayerProcessorChain, ProcessorTargetSpan, StereoProcessorChain};
use super::smoothing::{Smoother, rounded_frame_count};
use super::source::VoiceSourceRuntime;

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

#[derive(Debug, Clone, Copy)]
struct PendingNote {
    request: NoteRequest,
    key_down: bool,
    sustain_held: bool,
}

const MONOPHONIC_PORTAMENTO_DEFAULT_FRAMES: usize = 0;

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
        let output_mode = compiled.generator.output_mode();
        let note_start_fade_frames =
            rounded_frame_count(spec.sample_rate * GAIN_SMOOTHING_SECONDS).max(1);
        let processors = LayerProcessorChain::new(&compiled.processors, spec, output_mode)?;
        Ok(Self {
            envelope: AdsrRuntime::new(compiled.envelope),
            output_mode,
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

/// One prepared voice and its owned DSP and source state.
pub(crate) struct VoiceRuntime {
    state: VoiceState,
    note_id: Option<NoteId>,
    note_number: u8,
    velocity: u8,
    started_at_frame: u64,
    key_down: bool,
    sustain_held: bool,
    estimated_level: f32,
    layers: Vec<LayerRuntime>,
    processors: StereoProcessorChain,
    source_states: Vec<VoiceSourceRuntime>,
    source_spans: Vec<ValueSpan>,
    source_definitions: Vec<CompiledVoiceSource>,
    source_used: Vec<bool>,
    targets: VoiceTargetScratch,
    pending: Option<PendingNote>,
    pending_layer_selection: Vec<PreparedLayerSelection>,
    steal_fade_total: usize,
    steal_fade_remaining: usize,
    pitch_glide: Smoother,
    pitch_glide_span: ValueSpan,
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
            .map(VoiceSourceRuntime::new)
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
            key_down: false,
            sustain_held: false,
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
            pitch_glide: Smoother::new(0.0),
            pitch_glide_span: ValueSpan {
                start: 0.0,
                end: 0.0,
            },
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

    pub(crate) fn trace_identity(&self) -> Option<(NoteId, u8, u8, VoiceState)> {
        self.note_id
            .map(|note_id| (note_id, self.note_number, self.velocity, self.state))
    }

    pub(crate) fn portamento_offset_cents(&self) -> f32 {
        self.pitch_glide.current()
    }

    pub(crate) fn trace_layer_active(&self, index: usize) -> bool {
        self.layers.get(index).is_some_and(|layer| layer.active)
    }

    pub(crate) fn trace_source_value(&self, handle: SourceHandle) -> Option<f32> {
        let definition = self.source_definitions.get(handle.index())?;
        let state = self.source_states.get(handle.index())?;
        match (definition, state) {
            (CompiledVoiceSource::Velocity, VoiceSourceRuntime::Velocity(value))
            | (CompiledVoiceSource::KeyTracking, VoiceSourceRuntime::KeyTracking(value))
            | (CompiledVoiceSource::Random(_), VoiceSourceRuntime::Random(value)) => Some(*value),
            _ => state.current_value(definition),
        }
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
        self.pending = Some(PendingNote {
            request,
            key_down: true,
            sustain_held: false,
        });
        self.state = VoiceState::StealFading;
        self.steal_fade_total = fade_frames;
        self.steal_fade_remaining = fade_frames;
        if fade_frames == 0 {
            self.complete_steal(compiled)?;
        }
        Ok(())
    }

    pub(crate) fn transition_legato(
        &mut self,
        request: NoteRequest,
        portamento_frames: Option<usize>,
    ) -> Result<(), ProcessError> {
        if self.state != VoiceState::Active || self.note_id.is_none() {
            return Err(invalid_state());
        }
        let current_pitch = f32::from(self.note_number) * 100.0 + self.pitch_glide.current();
        let target_offset = current_pitch - f32::from(request.note_number) * 100.0;
        self.note_id = Some(request.note_id);
        self.note_number = request.note_number;
        self.velocity = request.velocity;
        self.started_at_frame = request.started_at_frame;
        self.key_down = true;
        self.sustain_held = false;
        self.pitch_glide.reset(target_offset);
        self.pitch_glide.set_target(
            0.0,
            portamento_frames.unwrap_or(MONOPHONIC_PORTAMENTO_DEFAULT_FRAMES),
        );
        for (definition, state) in self.source_definitions.iter().zip(&mut self.source_states) {
            state.transition_note(
                definition,
                request.note_id,
                request.note_number,
                request.velocity,
            );
        }
        Ok(())
    }

    pub(crate) fn retrigger_monophonic(
        &mut self,
        compiled: &CompiledInstrument,
        request: NoteRequest,
        layer_selection: &[PreparedLayerSelection],
        portamento_frames: Option<usize>,
    ) -> Result<(), ProcessError> {
        let current_pitch = if self.note_id.is_some() {
            f32::from(self.note_number) * 100.0 + self.pitch_glide.current()
        } else {
            f32::from(request.note_number) * 100.0
        };
        self.reset_note_state()?;
        self.activate_note(compiled, request, layer_selection, true, false)?;
        let target_offset = current_pitch - f32::from(request.note_number) * 100.0;
        self.pitch_glide.reset(target_offset);
        self.pitch_glide.set_target(
            0.0,
            portamento_frames.unwrap_or(MONOPHONIC_PORTAMENTO_DEFAULT_FRAMES),
        );
        Ok(())
    }

    pub(crate) fn release_note(
        &mut self,
        compiled: &CompiledInstrument,
        note_id: NoteId,
        sustain_down: bool,
    ) -> Result<(), ProcessError> {
        if let Some(pending) = self
            .pending
            .as_mut()
            .filter(|pending| pending.request.note_id == note_id)
        {
            if sustain_down {
                pending.key_down = false;
                pending.sustain_held = true;
            } else {
                self.pending = None;
            }
        }
        if self.note_id != Some(note_id) {
            return Ok(());
        }
        if matches!(self.state, VoiceState::Active) {
            if sustain_down {
                self.key_down = false;
                self.sustain_held = true;
                return Ok(());
            }
            self.begin_release(compiled)?;
        }
        Ok(())
    }

    pub(crate) fn release_sustain(
        &mut self,
        compiled: &CompiledInstrument,
    ) -> Result<(), ProcessError> {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| !pending.key_down && pending.sustain_held)
        {
            self.pending = None;
        }
        if self.state == VoiceState::Active && !self.key_down && self.sustain_held {
            self.begin_release(compiled)?;
        }
        Ok(())
    }

    fn begin_release(&mut self, compiled: &CompiledInstrument) -> Result<(), ProcessError> {
        if !matches!(self.state, VoiceState::Active) {
            return Ok(());
        }
        let note_id = self.note_id.ok_or_else(invalid_state)?;
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
            state.note_off();
        }
        self.key_down = false;
        self.sustain_held = false;
        self.state = VoiceState::Releasing;
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
            let mut chunk = self.next_voice_boundary(frames - offset, sample_rate, tempo_bpm);
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
            self.advance_source_spans(chunk, sample_rate, tempo_bpm)?;
            let (pitch_start, pitch_end) = self.pitch_glide.span(chunk);
            self.pitch_glide_span = ValueSpan {
                start: pitch_start,
                end: pitch_end,
            };
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
        self.activate_note(compiled, request, layer_selection, true, false)
    }

    fn activate_note(
        &mut self,
        compiled: &CompiledInstrument,
        note: NoteRequest,
        layer_selection: &[PreparedLayerSelection],
        key_down: bool,
        sustain_held: bool,
    ) -> Result<(), ProcessError> {
        if layer_selection.len() != self.layers.len() {
            return Err(invalid_state());
        }
        self.note_id = Some(note.note_id);
        self.note_number = note.note_number;
        self.velocity = note.velocity;
        self.started_at_frame = note.started_at_frame;
        self.key_down = key_down;
        self.sustain_held = sustain_held;
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
        if let Some(pending) = pending {
            self.activate_pending_note(compiled, pending)?;
        } else {
            self.reset_to_idle()?;
        }
        Ok(())
    }

    fn activate_pending_note(
        &mut self,
        compiled: &CompiledInstrument,
        pending: PendingNote,
    ) -> Result<(), ProcessError> {
        let pending_layer_selection = std::mem::take(&mut self.pending_layer_selection);
        let result = (|| {
            self.reset_note_state()?;
            self.activate_note(
                compiled,
                pending.request,
                &pending_layer_selection,
                pending.key_down,
                pending.sustain_held,
            )
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
                    target.tuning.start + self.pitch_glide_span.start,
                    target.tuning.end + self.pitch_glide_span.end,
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
            let gain = linear_gain_span(target.gain, target.gain_weight);
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
        self.key_down = false;
        self.sustain_held = false;
        self.estimated_level = 0.0;
        self.pending = None;
        self.steal_fade_total = 0;
        self.steal_fade_remaining = 0;
        self.pitch_glide.reset(0.0);
        self.pitch_glide_span = ValueSpan {
            start: 0.0,
            end: 0.0,
        };
        self.reset_source_state();
    }

    fn reset_note_state(&mut self) -> Result<(), ProcessError> {
        for layer in &mut self.layers {
            layer.reset_state()?;
        }
        self.processors.reset()?;
        self.pending = None;
        self.steal_fade_total = 0;
        self.steal_fade_remaining = 0;
        self.reset_source_state();
        self.pitch_glide.reset(0.0);
        self.pitch_glide_span = ValueSpan {
            start: 0.0,
            end: 0.0,
        };
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
            state.note_on(definition, note.note_id, note.note_number, note.velocity);
        }
    }

    fn reset_source_state(&mut self) {
        for (definition, state) in self.source_definitions.iter().zip(&mut self.source_states) {
            state.reset(definition);
        }
    }

    fn advance_source_spans(
        &mut self,
        frames: usize,
        sample_rate: f64,
        tempo_bpm: f64,
    ) -> Result<(), ProcessError> {
        let note_id = self.note_id.unwrap_or_default();
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
            *span = state.advance(definition, frames, sample_rate, tempo_bpm, note_id)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
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
                CompiledGenerator::PhysicalString(value) => {
                    LayerGeneratorTargetSpan::PhysicalString {
                        decay_seconds: self.evaluate_target(
                            compiled,
                            value.parameters.decay_seconds,
                            shared,
                        )?,
                        brightness: self.evaluate_target(
                            compiled,
                            value.parameters.brightness,
                            shared,
                        )?,
                        stiffness: self.evaluate_target(
                            compiled,
                            value.parameters.stiffness,
                            shared,
                        )?,
                    }
                }
                CompiledGenerator::Modal(value) => LayerGeneratorTargetSpan::Modal {
                    structure: self.evaluate_target(
                        compiled,
                        value.parameters.structure,
                        shared,
                    )?,
                    brightness: self.evaluate_target(
                        compiled,
                        value.parameters.brightness,
                        shared,
                    )?,
                    decay: self.evaluate_target(compiled, value.parameters.decay, shared)?,
                },
                CompiledGenerator::Additive(value) => LayerGeneratorTargetSpan::Additive {
                    morph: self.evaluate_target(compiled, value.parameters.morph, shared)?,
                    spectrum_tilt: self.evaluate_target(
                        compiled,
                        value.parameters.spectrum_tilt,
                        shared,
                    )?,
                    inharmonicity: self.evaluate_target(
                        compiled,
                        value.parameters.inharmonicity,
                        shared,
                    )?,
                },
                CompiledGenerator::Formant(value) => {
                    self.evaluate_formant_targets(compiled, value, shared)?
                }
                CompiledGenerator::Sample(_) => LayerGeneratorTargetSpan::Sample,
                CompiledGenerator::Granular(value) => LayerGeneratorTargetSpan::Granular {
                    position: self.evaluate_target(compiled, value.parameters.position, shared)?,
                    grain_size: self.evaluate_target(
                        compiled,
                        value.parameters.grain_size,
                        shared,
                    )?,
                    density: self.evaluate_target(compiled, value.parameters.density, shared)?,
                    pitch: self.evaluate_target(compiled, value.parameters.pitch, shared)?,
                    randomness: self.evaluate_target(
                        compiled,
                        value.parameters.randomness,
                        shared,
                    )?,
                    pan_spread: self.evaluate_target(
                        compiled,
                        value.parameters.pan_spread,
                        shared,
                    )?,
                },
                CompiledGenerator::WaveSequence(_) => LayerGeneratorTargetSpan::WaveSequence,
                CompiledGenerator::Wavetable(value) => {
                    self.evaluate_wavetable_targets(compiled, value, shared)?
                }
                CompiledGenerator::Spectral(value) => {
                    self.evaluate_spectral_targets(compiled, value, shared)?
                }
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
                gain_weight: ValueSpan {
                    start: 1.0,
                    end: 1.0,
                },
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
        for vector in &compiled.vectors {
            match *vector {
                crate::compiler::CompiledVector::TwoWay { position, layers } => {
                    let position = self.evaluate_target(compiled, position, shared)?;
                    let left = map_span(position, |value| {
                        (value * std::f32::consts::FRAC_PI_2).cos()
                    });
                    let right = map_span(position, |value| {
                        (value * std::f32::consts::FRAC_PI_2).sin()
                    });
                    self.targets.layers[layers[0]].gain_weight =
                        multiply_span(self.targets.layers[layers[0]].gain_weight, left);
                    self.targets.layers[layers[1]].gain_weight =
                        multiply_span(self.targets.layers[layers[1]].gain_weight, right);
                }
                crate::compiler::CompiledVector::FourWay { x, y, layers } => {
                    let x = self.evaluate_target(compiled, x, shared)?;
                    let y = self.evaluate_target(compiled, y, shared)?;
                    let x_cos = map_span(x, |value| (value * std::f32::consts::FRAC_PI_2).cos());
                    let x_sin = map_span(x, |value| (value * std::f32::consts::FRAC_PI_2).sin());
                    let y_cos = map_span(y, |value| (value * std::f32::consts::FRAC_PI_2).cos());
                    let y_sin = map_span(y, |value| (value * std::f32::consts::FRAC_PI_2).sin());
                    let weights = [
                        multiply_span(x_cos, y_cos),
                        multiply_span(x_sin, y_cos),
                        multiply_span(x_cos, y_sin),
                        multiply_span(x_sin, y_sin),
                    ];
                    for (layer, weight) in layers.into_iter().zip(weights) {
                        self.targets.layers[layer].gain_weight =
                            multiply_span(self.targets.layers[layer].gain_weight, weight);
                    }
                }
            }
        }
        Ok(())
    }

    fn evaluate_spectral_targets(
        &self,
        compiled: &CompiledInstrument,
        value: &crate::compiler::CompiledSpectral,
        shared: SharedParameterSpan<'_>,
    ) -> Result<LayerGeneratorTargetSpan, ProcessError> {
        Ok(LayerGeneratorTargetSpan::Spectral {
            position: self.evaluate_target(compiled, value.parameters.position, shared)?,
            freeze: self.evaluate_target(compiled, value.parameters.freeze, shared)?,
            blur: self.evaluate_target(compiled, value.parameters.blur, shared)?,
            shift: self.evaluate_target(compiled, value.parameters.shift, shared)?,
            morph: value
                .parameters
                .morph
                .map(|handle| self.evaluate_target(compiled, handle, shared))
                .transpose()?,
        })
    }

    fn evaluate_wavetable_targets(
        &self,
        compiled: &CompiledInstrument,
        value: &crate::compiler::CompiledWavetable,
        shared: SharedParameterSpan<'_>,
    ) -> Result<LayerGeneratorTargetSpan, ProcessError> {
        Ok(LayerGeneratorTargetSpan::Wavetable {
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
        })
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

    fn evaluate_formant_targets(
        &mut self,
        compiled: &CompiledInstrument,
        formant: &crate::compiler::CompiledFormant,
        shared: SharedParameterSpan<'_>,
    ) -> Result<LayerGeneratorTargetSpan, ProcessError> {
        Ok(LayerGeneratorTargetSpan::Formant {
            vowel_position: self.evaluate_target(
                compiled,
                formant.parameters.vowel_position,
                shared,
            )?,
            formant_shift: self.evaluate_target(
                compiled,
                formant.parameters.formant_shift,
                shared,
            )?,
            throat: self.evaluate_target(compiled, formant.parameters.throat, shared)?,
            spectral_tilt: self.evaluate_target(
                compiled,
                formant.parameters.spectral_tilt,
                shared,
            )?,
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
                    cutoff,
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
            CompiledProcessorKind::Eq(value) => Ok(ProcessorTargetSpan::Eq {
                low_gain_db: self.evaluate_target(
                    compiled,
                    value.parameters.low_gain_db,
                    shared,
                )?,
                mid_gain_db: self.evaluate_target(
                    compiled,
                    value.parameters.mid_gain_db,
                    shared,
                )?,
                high_gain_db: self.evaluate_target(
                    compiled,
                    value.parameters.high_gain_db,
                    shared,
                )?,
            }),
            CompiledProcessorKind::Resonator(value) => Ok(ProcessorTargetSpan::Resonator {
                frequency_hz: self.evaluate_target(
                    compiled,
                    value.parameters.frequency_hz,
                    shared,
                )?,
                decay_seconds: self.evaluate_target(
                    compiled,
                    value.parameters.decay_seconds,
                    shared,
                )?,
                damping: self.evaluate_target(compiled, value.parameters.damping, shared)?,
                mix: self.evaluate_target(compiled, value.parameters.mix, shared)?,
            }),
            CompiledProcessorKind::Compressor(value) => Ok(ProcessorTargetSpan::Compressor {
                threshold_db: self.evaluate_target(
                    compiled,
                    value.parameters.threshold_db,
                    shared,
                )?,
                ratio: self.evaluate_target(compiled, value.parameters.ratio, shared)?,
                makeup_gain_db: self.evaluate_target(
                    compiled,
                    value.parameters.makeup_gain_db,
                    shared,
                )?,
                mix: self.evaluate_target(compiled, value.parameters.mix, shared)?,
            }),
            CompiledProcessorKind::Limiter(value) => Ok(ProcessorTargetSpan::Limiter {
                ceiling_db: self.evaluate_target(compiled, value.parameters.ceiling_db, shared)?,
                input_gain_db: self.evaluate_target(
                    compiled,
                    value.parameters.input_gain_db,
                    shared,
                )?,
            }),
            CompiledProcessorKind::Bitcrusher(value) => Ok(ProcessorTargetSpan::Bitcrusher {
                bit_depth: self.evaluate_target(compiled, value.parameters.bit_depth, shared)?,
                sample_rate_ratio: self.evaluate_target(
                    compiled,
                    value.parameters.sample_rate_ratio,
                    shared,
                )?,
                mix: self.evaluate_target(compiled, value.parameters.mix, shared)?,
            }),
            CompiledProcessorKind::Chorus(_)
            | CompiledProcessorKind::Flanger(_)
            | CompiledProcessorKind::Phaser(_)
            | CompiledProcessorKind::Delay(_)
            | CompiledProcessorKind::Reverb(_) => Err(invalid_state()),
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
        let mut start_domain_sum = 0.0;
        let mut end_domain_sum = 0.0;
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
                CompiledSourceRef::Instrument(handle) => {
                    shared.instrument_source(handle).ok_or_else(invalid_state)?
                }
            };
            start_domain_sum += route_domain_delta(source.start, route.depth, route.curve);
            end_domain_sum += route_domain_delta(source.end, route.depth, route.curve);
        }
        let effective_maximum = compiled
            .effective_parameter_maximum(handle)
            .ok_or_else(invalid_state)?;
        let start = apply_domain_sum_with_maximum(
            descriptor,
            base.start,
            start_domain_sum,
            effective_maximum,
        )?
        .final_value;
        let end =
            apply_domain_sum_with_maximum(descriptor, base.end, end_domain_sum, effective_maximum)?
                .final_value;
        Ok(ValueSpan { start, end })
    }

    fn next_voice_boundary(&self, remaining: usize, sample_rate: f64, tempo_bpm: f64) -> usize {
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
        if let Some(frames) = self.pitch_glide.frames_until_target() {
            boundary = boundary.min(frames);
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
            if let Some(frames) =
                state.frames_until_boundary(definition, sample_rate, tempo_bpm, remaining)
            {
                boundary = boundary.min(frames);
            }
        }
        boundary.max(1).min(remaining)
    }
}

fn invalid_state() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: crate::process::ProcessorFailureKind::InvalidState,
    }
}

fn map_span(span: ValueSpan, map: impl Fn(f32) -> f32) -> ValueSpan {
    ValueSpan {
        start: map(span.start),
        end: map(span.end),
    }
}

fn multiply_span(left: ValueSpan, right: ValueSpan) -> ValueSpan {
    ValueSpan {
        start: left.start * right.start,
        end: left.end * right.end,
    }
}

fn linear_gain_span(gain_db: ValueSpan, weight: ValueSpan) -> ValueSpan {
    multiply_span(
        ValueSpan {
            start: db_to_linear(gain_db.start),
            end: db_to_linear(gain_db.end),
        },
        weight,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::modulation::curve_value;

    #[test]
    fn vector_weight_scales_linear_gain_after_db_conversion() {
        let gain = linear_gain_span(
            ValueSpan {
                start: -20.0,
                end: -20.0,
            },
            ValueSpan {
                start: 0.5,
                end: 0.5,
            },
        );

        assert!((gain.start - 0.05).abs() < 1.0e-6);
        assert!((gain.end - 0.05).abs() < 1.0e-6);
    }

    #[test]
    fn lfo_waveforms_match_the_definition() {
        assert!(
            (crate::runtime::source::lfo_value(crate::definition::LfoWaveform::Sine, 0.0)).abs()
                < 1.0e-6
        );
        assert!(
            (crate::runtime::source::lfo_value(crate::definition::LfoWaveform::Sine, 0.25) - 1.0)
                .abs()
                < 1.0e-6
        );
        assert!(
            (crate::runtime::source::lfo_value(crate::definition::LfoWaveform::Sine, 0.5)).abs()
                < 1.0e-6
        );
        assert!(
            (crate::runtime::source::lfo_value(crate::definition::LfoWaveform::Triangle, 0.0)
                + 1.0)
                .abs()
                < 1.0e-6
        );
        assert!(
            crate::runtime::source::lfo_value(crate::definition::LfoWaveform::Triangle, 0.5) - 1.0
                < 1.0e-6
        );
        assert!(
            (crate::runtime::source::lfo_value(crate::definition::LfoWaveform::Triangle, 1.0)
                + 1.0)
                .abs()
                < 1.0e-6
        );
    }

    #[test]
    fn triangle_boundary_is_sample_accurate() {
        assert_eq!(
            crate::runtime::source::triangle_boundary(
                0.0,
                1.0,
                crate::definition::ModulationRateUnit::PerSecond,
                48_000.0,
                120.0,
                30_000
            ),
            Some(24_000)
        );
        assert_eq!(
            crate::runtime::source::triangle_boundary(
                0.5,
                1.0,
                crate::definition::ModulationRateUnit::PerSecond,
                48_000.0,
                120.0,
                128
            ),
            Some(128)
        );
        assert_eq!(
            crate::runtime::source::triangle_boundary(
                0.9,
                1.0,
                crate::definition::ModulationRateUnit::PerSecond,
                48_000.0,
                120.0,
                128
            ),
            Some(128)
        );
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
        let value = crate::runtime::source::deterministic_random(8128, 60, source_hash, 0);
        assert!((value - 0.094_552_636).abs() < 1.0e-6);
        assert!((-1.0..=1.0).contains(&value));
        assert!(
            (value - crate::runtime::source::deterministic_random(8128, 60, source_hash, 0)).abs()
                < f32::EPSILON
        );
        assert!(
            (value - crate::runtime::source::deterministic_random(8129, 60, source_hash, 0)).abs()
                > f32::EPSILON
        );
        assert!(
            (value - crate::runtime::source::deterministic_random(8128, 61, source_hash, 0)).abs()
                > f32::EPSILON
        );
        assert!(
            (value
                - crate::runtime::source::deterministic_random(
                    8128,
                    60,
                    crate::compiler::source_id_hash("other_pan"),
                    0
                ))
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
