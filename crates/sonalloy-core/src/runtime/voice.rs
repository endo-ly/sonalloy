use sonalloy_dsp_sys::{DspFilter, DspOscillator, DspOscillatorWaveform};

use crate::compiler::{
    CompiledGenerator, CompiledInstrument, CompiledLayer, CompiledSourceRef, CompiledVoiceSource,
    cents_to_ratio, db_to_linear, midi_note_frequency,
};
use crate::definition::LfoWaveform;
use crate::parameter::ParameterScale;
use crate::process::{NoteId, ProcessError, ProcessSpec};

use super::adsr::AdsrRuntime;
use super::mix::constant_power_pan;
use super::modulation::{
    FilterTargetSpan, LayerTargetSpan, SharedParameterSpan, ValueSpan, VoiceTargetScratch,
};
use super::sample::{SampleRuntime, playback_ratio};
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

enum GeneratorRuntime {
    Oscillator {
        oscillator: DspOscillator,
        phase_reset: bool,
    },
    Sample {
        sample: SampleRuntime,
    },
    Disabled,
}

struct LayerRuntime {
    trigger: crate::compiler::CompiledLayerTrigger,
    envelope: AdsrRuntime,
    generator: GeneratorRuntime,
    active: bool,
    note_start_fade: Smoother,
    note_start_fade_frames: usize,
    sample_root_note: u8,
}

impl LayerRuntime {
    fn new(compiled: &CompiledLayer, spec: ProcessSpec) -> Result<Self, ProcessError> {
        let generator = match &compiled.generator {
            CompiledGenerator::Oscillator(definition) => {
                let mut oscillator = DspOscillator::new().map_err(ProcessError::from_dsp_error)?;
                let waveform = match definition.waveform {
                    crate::definition::OscillatorWaveform::Sine => DspOscillatorWaveform::Sine,
                    crate::definition::OscillatorWaveform::Saw => DspOscillatorWaveform::Saw,
                };
                oscillator
                    .prepare(spec.sample_rate, waveform)
                    .map_err(ProcessError::from_dsp_error)?;
                oscillator.reset().map_err(ProcessError::from_dsp_error)?;
                GeneratorRuntime::Oscillator {
                    oscillator,
                    phase_reset: definition.phase_reset,
                }
            }
            CompiledGenerator::Sample(sample) => {
                sample
                    .source
                    .as_ref()
                    .map_or(GeneratorRuntime::Disabled, |source| {
                        GeneratorRuntime::Sample {
                            sample: SampleRuntime::new(source),
                        }
                    })
            }
        };
        let sample_root_note = match &compiled.generator {
            CompiledGenerator::Sample(sample) => sample.root_note,
            CompiledGenerator::Oscillator(_) => 69,
        };
        let note_start_fade_frames =
            rounded_frame_count(spec.sample_rate * GAIN_SMOOTHING_SECONDS).max(1);
        Ok(Self {
            trigger: compiled.trigger,
            envelope: AdsrRuntime::new(compiled.envelope),
            generator,
            active: false,
            note_start_fade: Smoother::new(0.0),
            note_start_fade_frames,
            sample_root_note,
        })
    }

    fn can_trigger(&self, note_number: u8, velocity: u8) -> bool {
        !matches!(self.generator, GeneratorRuntime::Disabled)
            && self.trigger.matches(note_number, velocity)
    }

    fn start(&mut self, note: NoteRequest) -> Result<bool, ProcessError> {
        if !self.can_trigger(note.note_number, note.velocity) {
            return Ok(false);
        }
        match &mut self.generator {
            GeneratorRuntime::Oscillator {
                oscillator,
                phase_reset,
            } => {
                if *phase_reset {
                    oscillator.reset().map_err(ProcessError::from_dsp_error)?;
                }
            }
            GeneratorRuntime::Sample { sample } => sample.start(1.0),
            GeneratorRuntime::Disabled => return Ok(false),
        }
        self.envelope.note_on();
        self.note_start_fade.reset(0.0);
        self.note_start_fade
            .set_target(1.0, self.note_start_fade_frames);
        self.active = true;
        Ok(true)
    }

    fn render_source(
        &mut self,
        frames: usize,
        note_number: u8,
        tuning_start: f32,
        tuning_end: f32,
        sample_rate: f64,
        output: &mut [f32],
    ) -> Result<bool, ProcessError> {
        match &mut self.generator {
            GeneratorRuntime::Oscillator { oscillator, .. } => {
                let mut start_frequency =
                    midi_note_frequency(note_number, cents_to_ratio(tuning_start));
                let mut end_frequency =
                    midi_note_frequency(note_number, cents_to_ratio(tuning_end));
                #[allow(clippy::cast_possible_truncation)]
                let max_frequency = (sample_rate * 0.45) as f32;
                if !start_frequency.is_finite()
                    || !end_frequency.is_finite()
                    || start_frequency <= 0.0
                    || end_frequency <= 0.0
                {
                    return Err(ProcessError::InvalidFrequency);
                }
                start_frequency = start_frequency.min(max_frequency);
                end_frequency = end_frequency.min(max_frequency);
                match oscillator_processing_mode(start_frequency, end_frequency) {
                    OscillatorProcessingMode::Constant => oscillator
                        .process(start_frequency, &mut output[..frames])
                        .map_err(ProcessError::from_dsp_error)?,
                    OscillatorProcessingMode::Ramp => oscillator
                        .process_ramp(start_frequency, end_frequency, &mut output[..frames])
                        .map_err(ProcessError::from_dsp_error)?,
                }
                Ok(false)
            }
            GeneratorRuntime::Sample { sample } => {
                let start_ratio = playback_ratio(
                    note_number,
                    self.sample_root_note,
                    cents_to_ratio(tuning_start),
                );
                let end_ratio = playback_ratio(
                    note_number,
                    self.sample_root_note,
                    cents_to_ratio(tuning_end),
                );
                if !start_ratio.is_finite()
                    || !end_ratio.is_finite()
                    || start_ratio <= 0.0
                    || end_ratio <= 0.0
                {
                    return Err(ProcessError::InvalidFrequency);
                }
                for (index, value) in output[..frames].iter_mut().enumerate() {
                    #[allow(clippy::cast_precision_loss)]
                    let position = index as f64 / frames.max(1) as f64;
                    let ratio = start_ratio * (end_ratio / start_ratio).powf(position);
                    *value = sample.next_sample_with_ratio(ratio);
                }
                Ok(sample.is_finished())
            }
            GeneratorRuntime::Disabled => {
                output[..frames].fill(0.0);
                Ok(true)
            }
        }
    }

    fn reset(&mut self) -> Result<(), ProcessError> {
        match &mut self.generator {
            GeneratorRuntime::Oscillator { oscillator, .. } => {
                oscillator.reset().map_err(ProcessError::from_dsp_error)?;
            }
            GeneratorRuntime::Sample { sample } => sample.reset(),
            GeneratorRuntime::Disabled => {}
        }
        self.envelope.reset();
        self.note_start_fade.reset(0.0);
        self.active = false;
        Ok(())
    }

    fn reset_to_idle(&mut self) {
        self.envelope.reset();
        self.note_start_fade.reset(0.0);
        self.active = false;
    }

    fn reset_for_note(&mut self) -> Result<(), ProcessError> {
        match &mut self.generator {
            GeneratorRuntime::Oscillator {
                oscillator,
                phase_reset,
            } => {
                if *phase_reset {
                    oscillator.reset().map_err(ProcessError::from_dsp_error)?;
                }
            }
            GeneratorRuntime::Sample { sample } => sample.reset(),
            GeneratorRuntime::Disabled => {}
        }
        self.envelope.reset();
        self.note_start_fade.reset(0.0);
        self.active = false;
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
    filter_left: DspFilter,
    filter_right: DspFilter,
    filter: Option<crate::compiler::CompiledFilter>,
    source_states: Vec<VoiceSourceRuntime>,
    source_spans: Vec<ValueSpan>,
    source_definitions: Vec<CompiledVoiceSource>,
    source_used: Vec<bool>,
    targets: VoiceTargetScratch,
    pending: Option<NoteRequest>,
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
            .map(|layer| LayerRuntime::new(layer, spec))
            .collect::<Result<Vec<_>, _>>()?;
        let mut filter_left = DspFilter::new().map_err(ProcessError::from_filter_error)?;
        let mut filter_right = DspFilter::new().map_err(ProcessError::from_filter_error)?;
        filter_left
            .prepare(spec.sample_rate)
            .map_err(ProcessError::from_filter_error)?;
        filter_right
            .prepare(spec.sample_rate)
            .map_err(ProcessError::from_filter_error)?;
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
            filter_left,
            filter_right,
            filter: compiled.voice_filter,
            source_states,
            source_spans,
            source_definitions,
            source_used,
            targets: VoiceTargetScratch::new(
                compiled.layers.len(),
                compiled.voice_filter.is_some(),
            ),
            pending: None,
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

    pub(crate) fn request_note(
        &mut self,
        note: NoteRequest,
        fade_frames: usize,
    ) -> Result<(), ProcessError> {
        if !self.can_trigger(note.note_number, note.velocity) {
            return Ok(());
        }
        if self.state == VoiceState::Idle {
            self.start_note(note)?;
            return Ok(());
        }
        self.pending = Some(note);
        self.state = VoiceState::StealFading;
        self.steal_fade_total = fade_frames;
        self.steal_fade_remaining = fade_frames;
        if fade_frames == 0 {
            self.complete_steal()?;
        }
        Ok(())
    }

    pub(crate) fn release_note(&mut self, note_id: NoteId) {
        if self
            .pending
            .is_some_and(|pending| pending.note_id == note_id)
        {
            self.pending = None;
        }
        if self.note_id != Some(note_id) {
            return;
        }
        if matches!(self.state, VoiceState::Active) {
            for layer in &mut self.layers {
                if layer.active {
                    layer.envelope.note_off();
                }
            }
            for state in &mut self.source_states {
                if let VoiceSourceRuntime::Envelope(envelope) = state {
                    envelope.note_off();
                }
            }
            self.state = VoiceState::Releasing;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_span(
        &mut self,
        frames: usize,
        sample_rate: f64,
        compiled: &CompiledInstrument,
        shared: SharedParameterSpan<'_>,
        layer_mono: &mut [f32],
        voice_left: &mut [f32],
        voice_right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if frames == 0 {
            return Ok(());
        }
        layer_mono[..frames].fill(0.0);
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
                    self.complete_steal()?;
                    continue;
                }
                chunk = 1.min(frames - offset);
            }
            let subspan = shared.subspan(offset, chunk, frames);
            self.advance_source_spans(chunk, sample_rate);
            self.evaluate_targets(compiled, subspan)?;
            self.render_active_segment(
                chunk,
                sample_rate,
                &mut layer_mono[offset..offset + chunk],
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
                    self.complete_steal()?;
                } else {
                    self.reset_to_idle()?;
                }
            } else if self.state == VoiceState::StealFading && self.steal_fade_remaining == 0 {
                self.complete_steal()?;
            }
        }
        self.estimated_level = self.estimated_level.mul_add(0.95, peak * 0.05);
        Ok(())
    }

    pub(crate) fn reset(&mut self) -> Result<(), ProcessError> {
        self.reset_to_idle()?;
        for layer in &mut self.layers {
            layer.reset()?;
        }
        self.reset_source_state();
        Ok(())
    }

    fn can_trigger(&self, note_number: u8, velocity: u8) -> bool {
        self.layers
            .iter()
            .any(|layer| layer.can_trigger(note_number, velocity))
    }

    fn has_active_layer(&self) -> bool {
        self.layers.iter().any(|layer| layer.active)
    }

    fn start_note(&mut self, note: NoteRequest) -> Result<(), ProcessError> {
        self.reset_note_state()?;
        self.note_id = Some(note.note_id);
        self.note_number = note.note_number;
        self.velocity = note.velocity;
        self.started_at_frame = note.started_at_frame;
        for layer in &mut self.layers {
            let _ = layer.start(note)?;
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

    fn complete_steal(&mut self) -> Result<(), ProcessError> {
        let pending = self.pending.take();
        self.note_id = None;
        self.state = VoiceState::Idle;
        self.steal_fade_total = 0;
        self.steal_fade_remaining = 0;
        if let Some(note) = pending {
            self.start_note(note)?;
        } else {
            self.reset_to_idle()?;
        }
        Ok(())
    }

    fn render_active_segment(
        &mut self,
        frames: usize,
        sample_rate: f64,
        layer_mono: &mut [f32],
        voice_left: &mut [f32],
        voice_right: &mut [f32],
    ) -> Result<(), ProcessError> {
        voice_left[..frames].fill(0.0);
        voice_right[..frames].fill(0.0);
        let targets = &self.targets;
        for (index, layer) in self.layers.iter_mut().enumerate() {
            if !layer.active {
                continue;
            }
            let target = targets.layers[index];
            layer_mono[..frames].fill(0.0);
            let generator_finished = layer.render_source(
                frames,
                self.note_number,
                target.tuning.start,
                target.tuning.end,
                sample_rate,
                layer_mono,
            )?;
            let gain_start = db_to_linear(target.gain.start);
            let gain_end = db_to_linear(target.gain.end);
            for frame in 0..frames {
                let envelope = layer.envelope.next_sample();
                let fade = layer.note_start_fade.next();
                #[allow(clippy::cast_precision_loss)]
                let position = frame as f32 / frames.max(1) as f32;
                let gain = gain_start + (gain_end - gain_start) * position;
                let left = target.pan_left.start
                    + (target.pan_left.end - target.pan_left.start) * position;
                let right = target.pan_right.start
                    + (target.pan_right.end - target.pan_right.start) * position;
                let mono = layer_mono[frame] * envelope * fade * gain;
                voice_left[frame] += mono * left;
                voice_right[frame] += mono * right;
            }
            if layer.envelope.is_idle() || generator_finished {
                layer.active = false;
            }
        }
        if let (Some(filter), Some(target)) = (self.filter, targets.filter) {
            self.render_filter(
                filter,
                target,
                &mut voice_left[..frames],
                &mut voice_right[..frames],
            )?;
        }
        Ok(())
    }

    fn render_filter(
        &mut self,
        filter: crate::compiler::CompiledFilter,
        target: FilterTargetSpan,
        voice_left: &mut [f32],
        voice_right: &mut [f32],
    ) -> Result<(), ProcessError> {
        let cutoff_start = target.cutoff.start.min(filter.effective_max_cutoff_hz);
        let cutoff_end = target.cutoff.end.min(filter.effective_max_cutoff_hz);
        let resonance_start = target.resonance.start;
        let resonance_end = target.resonance.end;
        match filter_processing_mode(cutoff_start, cutoff_end, resonance_start, resonance_end) {
            FilterProcessingMode::Constant => {
                self.filter_left
                    .process(cutoff_start, resonance_start, voice_left)
                    .map_err(ProcessError::from_filter_error)?;
                self.filter_right
                    .process(cutoff_start, resonance_start, voice_right)
                    .map_err(ProcessError::from_filter_error)?;
            }
            FilterProcessingMode::CutoffRamp => {
                self.filter_left
                    .process_ramp(cutoff_start, cutoff_end, resonance_start, voice_left)
                    .map_err(ProcessError::from_filter_error)?;
                self.filter_right
                    .process_ramp(cutoff_start, cutoff_end, resonance_start, voice_right)
                    .map_err(ProcessError::from_filter_error)?;
            }
            FilterProcessingMode::CutoffAndResonanceRamp => {
                self.filter_left
                    .process_ramp_with_resonance(
                        cutoff_start,
                        cutoff_end,
                        resonance_start,
                        resonance_end,
                        voice_left,
                    )
                    .map_err(ProcessError::from_filter_error)?;
                self.filter_right
                    .process_ramp_with_resonance(
                        cutoff_start,
                        cutoff_end,
                        resonance_start,
                        resonance_end,
                        voice_right,
                    )
                    .map_err(ProcessError::from_filter_error)?;
            }
        }
        Ok(())
    }

    fn reset_to_idle(&mut self) -> Result<(), ProcessError> {
        for layer in &mut self.layers {
            layer.reset_to_idle();
        }
        self.filter_left
            .reset()
            .map_err(ProcessError::from_filter_error)?;
        self.filter_right
            .reset()
            .map_err(ProcessError::from_filter_error)?;
        self.state = VoiceState::Idle;
        self.note_id = None;
        self.note_number = 0;
        self.velocity = 0;
        self.estimated_level = 0.0;
        self.pending = None;
        self.steal_fade_total = 0;
        self.steal_fade_remaining = 0;
        self.reset_source_state();
        Ok(())
    }

    fn reset_note_state(&mut self) -> Result<(), ProcessError> {
        for layer in &mut self.layers {
            layer.reset_for_note()?;
        }
        self.filter_left
            .reset()
            .map_err(ProcessError::from_filter_error)?;
        self.filter_right
            .reset()
            .map_err(ProcessError::from_filter_error)?;
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
            self.targets.layers[index] = LayerTargetSpan {
                gain: self.evaluate_target(compiled, layer.parameters.gain, shared)?,
                pan_left: ValueSpan {
                    start: 0.0,
                    end: 0.0,
                },
                pan_right: ValueSpan {
                    start: 0.0,
                    end: 0.0,
                },
                tuning: self.evaluate_target(compiled, layer.parameters.tuning, shared)?,
            };
            let pan = self.evaluate_target(compiled, layer.parameters.pan, shared)?;
            let (left_start, right_start) = constant_power_pan(pan.start);
            let (left_end, right_end) = constant_power_pan(pan.end);
            self.targets.layers[index].pan_left = ValueSpan {
                start: left_start,
                end: left_end,
            };
            self.targets.layers[index].pan_right = ValueSpan {
                start: right_start,
                end: right_end,
            };
        }
        self.targets.filter = if let Some(filter) = self.filter {
            Some(FilterTargetSpan {
                cutoff: self.evaluate_target(compiled, filter.parameters.cutoff, shared)?,
                resonance: self.evaluate_target(compiled, filter.parameters.resonance, shared)?,
            })
        } else {
            None
        };
        Ok(())
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
        let base = shared.parameter(handle);
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
        for route in compiled.routes_for(handle) {
            let source = match route.source {
                CompiledSourceRef::Voice(handle) => self.source_spans[handle.index()],
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
        for (definition, state) in self.source_definitions.iter().zip(&self.source_states) {
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

fn span_is_constant(start: f32, end: f32) -> bool {
    start.total_cmp(&end) == std::cmp::Ordering::Equal
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OscillatorProcessingMode {
    Constant,
    Ramp,
}

fn oscillator_processing_mode(start: f32, end: f32) -> OscillatorProcessingMode {
    if span_is_constant(start, end) {
        OscillatorProcessingMode::Constant
    } else {
        OscillatorProcessingMode::Ramp
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterProcessingMode {
    Constant,
    CutoffRamp,
    CutoffAndResonanceRamp,
}

fn filter_processing_mode(
    cutoff_start: f32,
    cutoff_end: f32,
    resonance_start: f32,
    resonance_end: f32,
) -> FilterProcessingMode {
    if span_is_constant(cutoff_start, cutoff_end)
        && span_is_constant(resonance_start, resonance_end)
    {
        FilterProcessingMode::Constant
    } else if span_is_constant(resonance_start, resonance_end) {
        FilterProcessingMode::CutoffRamp
    } else {
        FilterProcessingMode::CutoffAndResonanceRamp
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
    let mixed = splitmix64_finalizer(seed ^ note_id ^ source_hash);
    #[allow(clippy::cast_precision_loss)]
    let unit = (mixed >> 40) as f32 / (1_u32 << 24) as f32;
    unit * 2.0 - 1.0
}

fn splitmix64_finalizer(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processing_modes_select_constant_and_dynamic_paths() {
        assert_eq!(
            oscillator_processing_mode(440.0, 440.0),
            OscillatorProcessingMode::Constant
        );
        assert_eq!(
            oscillator_processing_mode(440.0, 441.0),
            OscillatorProcessingMode::Ramp
        );
        assert_eq!(
            filter_processing_mode(1_000.0, 1_000.0, 0.1, 0.1),
            FilterProcessingMode::Constant
        );
        assert_eq!(
            filter_processing_mode(1_000.0, 1_100.0, 0.1, 0.1),
            FilterProcessingMode::CutoffRamp
        );
        assert_eq!(
            filter_processing_mode(1_000.0, 1_000.0, 0.1, 0.2),
            FilterProcessingMode::CutoffAndResonanceRamp
        );
    }

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
}
