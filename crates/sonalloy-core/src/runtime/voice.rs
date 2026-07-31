use sonalloy_dsp_sys::{DspFilter, DspOscillator, DspOscillatorWaveform};

use crate::compiler::{
    CompiledFilter, CompiledGenerator, CompiledInstrument, CompiledLayer, midi_note_frequency,
};
use crate::process::{NoteId, ProcessError, ProcessSpec};

use super::adsr::AdsrRuntime;
use super::mix::{constant_power_pan, velocity_cutoff, velocity_gain};
use super::sample::{SampleRuntime, playback_ratio};
use super::smoothing::{Smoother, rounded_frame_count};

const GAIN_SMOOTHING_SECONDS: f64 = 0.005;
const FILTER_SMOOTHING_SECONDS: f64 = 0.010;

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
        frequency_hz: f32,
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
    gain_linear: f32,
    pan_left: f32,
    pan_right: f32,
    tuning_ratio: f32,
    active: bool,
    gain_smoother: Smoother,
    gain_smoothing_frames: usize,
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
                    frequency_hz: 0.0,
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
        let (pan_left, pan_right) = constant_power_pan(compiled.pan);
        let gain_smoothing_frames =
            rounded_frame_count(spec.sample_rate * GAIN_SMOOTHING_SECONDS).max(1);
        Ok(Self {
            trigger: compiled.trigger,
            envelope: AdsrRuntime::new(compiled.envelope),
            generator,
            gain_linear: compiled.gain_linear,
            pan_left,
            pan_right,
            tuning_ratio: compiled.tuning_ratio,
            active: false,
            gain_smoother: Smoother::new(0.0),
            gain_smoothing_frames,
            sample_root_note,
        })
    }

    fn can_trigger(&self, note_number: u8, velocity: u8) -> bool {
        !matches!(self.generator, GeneratorRuntime::Disabled)
            && self.trigger.matches(note_number, velocity)
    }

    fn start(
        &mut self,
        note: NoteRequest,
        velocity_response: crate::compiler::CompiledVelocityResponse,
        sample_rate: f64,
    ) -> Result<bool, ProcessError> {
        if !self.can_trigger(note.note_number, note.velocity) {
            return Ok(false);
        }
        match &mut self.generator {
            GeneratorRuntime::Oscillator { frequency_hz, .. } => {
                *frequency_hz = midi_note_frequency(note.note_number, self.tuning_ratio);
                if !frequency_hz.is_finite()
                    || *frequency_hz < 0.0
                    || f64::from(*frequency_hz) > sample_rate * 0.5
                {
                    return Err(ProcessError::InvalidFrequency);
                }
            }
            GeneratorRuntime::Sample { sample } => {
                sample.start(playback_ratio(
                    note.note_number,
                    self.sample_root_note,
                    self.tuning_ratio,
                ));
            }
            GeneratorRuntime::Disabled => return Ok(false),
        }
        self.envelope.note_on();
        self.active = true;
        let target_gain =
            self.gain_linear * velocity_gain(note.velocity, velocity_response.layer_gain_amount);
        self.gain_smoother.reset(0.0);
        self.gain_smoother
            .set_target(target_gain, self.gain_smoothing_frames);
        Ok(true)
    }

    fn render_source(&mut self, frames: usize, output: &mut [f32]) -> Result<bool, ProcessError> {
        match &mut self.generator {
            GeneratorRuntime::Oscillator {
                oscillator,
                frequency_hz,
                ..
            } => {
                oscillator
                    .process(*frequency_hz, &mut output[..frames])
                    .map_err(ProcessError::from_dsp_error)?;
                Ok(false)
            }
            GeneratorRuntime::Sample { sample } => {
                for value in &mut output[..frames] {
                    *value = sample.next_sample();
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
        self.gain_smoother.reset(0.0);
        self.active = false;
        Ok(())
    }

    fn reset_to_idle(&mut self) {
        self.envelope.reset();
        self.gain_smoother.reset(0.0);
        self.active = false;
    }

    fn reset_for_note(&mut self) -> Result<(), ProcessError> {
        match &mut self.generator {
            GeneratorRuntime::Oscillator {
                oscillator,
                phase_reset,
                ..
            } => {
                if *phase_reset {
                    oscillator.reset().map_err(ProcessError::from_dsp_error)?;
                }
            }
            GeneratorRuntime::Sample { sample } => sample.reset(),
            GeneratorRuntime::Disabled => {}
        }
        self.envelope.reset();
        self.gain_smoother.reset(0.0);
        self.active = false;
        Ok(())
    }
}

/// One prepared voice and its owned DSP state.
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
    filter: Option<CompiledFilter>,
    filter_cutoff: Smoother,
    default_filter_cutoff: f32,
    pending: Option<NoteRequest>,
    steal_fade_total: usize,
    steal_fade_remaining: usize,
    filter_smoothing_frames: usize,
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
        let default_cutoff = compiled.voice_filter.map_or(1.0, |filter| filter.cutoff_hz);
        let filter_smoothing_frames =
            rounded_frame_count(spec.sample_rate * FILTER_SMOOTHING_SECONDS).max(1);
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
            filter_cutoff: Smoother::new(default_cutoff),
            default_filter_cutoff: default_cutoff,
            pending: None,
            steal_fade_total: 0,
            steal_fade_remaining: 0,
            filter_smoothing_frames,
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

    pub(crate) fn is_stealing(&self) -> bool {
        self.state == VoiceState::StealFading
    }

    pub(crate) fn steal_frames_remaining(&self) -> usize {
        self.steal_fade_remaining
    }

    pub(crate) fn request_note(
        &mut self,
        note: NoteRequest,
        sample_rate: f64,
        fade_frames: usize,
        velocity_response: crate::compiler::CompiledVelocityResponse,
    ) -> Result<(), ProcessError> {
        if !self.can_trigger(note.note_number, note.velocity) {
            return Ok(());
        }
        if self.state == VoiceState::Idle {
            self.start_note(note, sample_rate, velocity_response)?;
            return Ok(());
        }
        self.pending = Some(note);
        self.state = VoiceState::StealFading;
        self.steal_fade_total = fade_frames;
        self.steal_fade_remaining = fade_frames;
        if fade_frames == 0 {
            self.complete_steal(sample_rate, velocity_response)?;
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
        if self.state == VoiceState::Active {
            for layer in &mut self.layers {
                if layer.active {
                    layer.envelope.note_off();
                }
            }
            self.state = VoiceState::Releasing;
        }
    }

    pub(crate) fn render(
        &mut self,
        frames: usize,
        sample_rate: f64,
        velocity_response: crate::compiler::CompiledVelocityResponse,
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
            if self.state == VoiceState::StealFading {
                if !self.has_active_layer() {
                    self.complete_steal(sample_rate, velocity_response)?;
                    continue;
                }
                let envelope_remaining = self
                    .layers
                    .iter()
                    .filter(|layer| layer.active)
                    .map(|layer| layer.envelope.frames_until_idle().unwrap_or(usize::MAX))
                    .min()
                    .unwrap_or(usize::MAX);
                let chunk = self
                    .steal_fade_remaining
                    .min(frames - offset)
                    .min(envelope_remaining);
                if chunk == 0 {
                    self.complete_steal(sample_rate, velocity_response)?;
                    continue;
                }
                self.render_active_segment(
                    chunk,
                    &mut layer_mono[offset..offset + chunk],
                    &mut voice_left[offset..offset + chunk],
                    &mut voice_right[offset..offset + chunk],
                )?;
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
                offset += chunk;
                if !self.has_active_layer() || self.steal_fade_remaining == 0 {
                    self.complete_steal(sample_rate, velocity_response)?;
                }
            } else {
                if !self.has_active_layer() {
                    self.reset_to_idle()?;
                    break;
                }
                let chunk = frames - offset;
                self.render_active_segment(
                    chunk,
                    &mut layer_mono[offset..offset + chunk],
                    &mut voice_left[offset..offset + chunk],
                    &mut voice_right[offset..offset + chunk],
                )?;
                for index in offset..offset + chunk {
                    peak = peak
                        .max(voice_left[index].abs())
                        .max(voice_right[index].abs());
                }
                offset += chunk;
                if !self.has_active_layer() {
                    self.reset_to_idle()?;
                }
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

    fn start_note(
        &mut self,
        note: NoteRequest,
        sample_rate: f64,
        velocity_response: crate::compiler::CompiledVelocityResponse,
    ) -> Result<(), ProcessError> {
        self.reset_note_state()?;
        self.note_id = Some(note.note_id);
        self.note_number = note.note_number;
        self.velocity = note.velocity;
        self.started_at_frame = note.started_at_frame;
        let mut triggered = false;
        for layer in &mut self.layers {
            triggered |= layer.start(note, velocity_response, sample_rate)?;
        }
        if !triggered {
            self.reset_to_idle()?;
            return Ok(());
        }
        if let Some(filter) = self.filter {
            let cutoff = velocity_cutoff(
                filter.cutoff_hz,
                note.velocity,
                velocity_response.filter_cutoff_octaves,
            )
            .max(1.0);
            self.filter_cutoff
                .set_target(cutoff, self.filter_smoothing_frames);
        }
        self.state = VoiceState::Active;
        self.estimated_level = 0.0;
        Ok(())
    }

    fn complete_steal(
        &mut self,
        sample_rate: f64,
        velocity_response: crate::compiler::CompiledVelocityResponse,
    ) -> Result<(), ProcessError> {
        let pending = self.pending.take();
        self.note_id = None;
        self.state = VoiceState::Idle;
        self.steal_fade_total = 0;
        self.steal_fade_remaining = 0;
        if let Some(note) = pending {
            self.start_note(note, sample_rate, velocity_response)?;
        } else {
            self.reset_to_idle()?;
        }
        Ok(())
    }

    fn render_active_segment(
        &mut self,
        frames: usize,
        layer_mono: &mut [f32],
        voice_left: &mut [f32],
        voice_right: &mut [f32],
    ) -> Result<(), ProcessError> {
        voice_left[..frames].fill(0.0);
        voice_right[..frames].fill(0.0);
        for layer in &mut self.layers {
            if !layer.active {
                continue;
            }
            layer_mono[..frames].fill(0.0);
            let generator_finished = layer.render_source(frames, layer_mono)?;
            for index in 0..frames {
                let envelope = layer.envelope.next_sample();
                let gain = layer.gain_smoother.next();
                let mono = layer_mono[index] * envelope * gain;
                voice_left[index] += mono * layer.pan_left;
                voice_right[index] += mono * layer.pan_right;
                if layer.envelope.is_idle() {
                    layer.active = false;
                }
            }
            if generator_finished {
                layer.active = false;
            }
        }

        if let Some(filter) = self.filter {
            if self.filter_cutoff.is_smoothing() {
                for index in 0..frames {
                    let cutoff = self.filter_cutoff.next().max(1.0);
                    self.filter_left
                        .process(cutoff, filter.resonance, &mut voice_left[index..=index])
                        .map_err(ProcessError::from_filter_error)?;
                    self.filter_right
                        .process(cutoff, filter.resonance, &mut voice_right[index..=index])
                        .map_err(ProcessError::from_filter_error)?;
                }
            } else {
                let cutoff = self.filter_cutoff.value().max(1.0);
                self.filter_left
                    .process(cutoff, filter.resonance, &mut voice_left[..frames])
                    .map_err(ProcessError::from_filter_error)?;
                self.filter_right
                    .process(cutoff, filter.resonance, &mut voice_right[..frames])
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
        self.filter_cutoff.reset(self.default_filter_cutoff);
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
        self.filter_cutoff.reset(self.default_filter_cutoff);
        Ok(())
    }
}
