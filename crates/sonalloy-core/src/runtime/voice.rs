use sonalloy_dsp_sys::{DspFilter, DspOscillator, DspOscillatorWaveform};

use crate::compiler::{CompiledFilter, CompiledGenerator, CompiledInstrument, midi_note_frequency};
use crate::process::{DspFailureKind, NoteId, ProcessError, ProcessSpec};

use super::adsr::AdsrRuntime;
use super::mix::{constant_power_pan, velocity_cutoff, velocity_gain};
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
    /// Note Off has started the release envelope.
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

struct LayerRuntime {
    trigger: crate::compiler::CompiledLayerTrigger,
    envelope: AdsrRuntime,
    oscillator: DspOscillator,
    gain_linear: f32,
    pan_left: f32,
    pan_right: f32,
    tuning_ratio: f32,
    frequency_hz: f32,
    active: bool,
    gain_smoother: Smoother,
}

/// One prepared voice and its owned DSP state.
pub(crate) struct VoiceRuntime {
    state: VoiceState,
    note_id: Option<NoteId>,
    note_number: u8,
    velocity: u8,
    started_at_frame: u64,
    estimated_level: f32,
    layer: LayerRuntime,
    filter_left: DspFilter,
    filter_right: DspFilter,
    filter: Option<CompiledFilter>,
    filter_cutoff: Smoother,
    default_filter_cutoff: f32,
    pending: Option<NoteRequest>,
    steal_fade_total: usize,
    steal_fade_remaining: usize,
    gain_smoothing_frames: usize,
    filter_smoothing_frames: usize,
}

impl VoiceRuntime {
    pub(crate) fn new(
        compiled: &CompiledInstrument,
        spec: ProcessSpec,
    ) -> Result<Self, ProcessError> {
        let compiled_layer = compiled.layers.first().ok_or(ProcessError::DspFailure {
            kind: DspFailureKind::InvalidInput,
        })?;
        let CompiledGenerator::Oscillator(oscillator_definition) = compiled_layer.generator;
        let mut oscillator = DspOscillator::new().map_err(ProcessError::from_dsp_error)?;
        let waveform = match oscillator_definition.waveform {
            crate::definition::OscillatorWaveform::Sine => DspOscillatorWaveform::Sine,
            crate::definition::OscillatorWaveform::Saw => DspOscillatorWaveform::Saw,
        };
        oscillator
            .prepare(spec.sample_rate, waveform)
            .map_err(ProcessError::from_dsp_error)?;
        oscillator.reset().map_err(ProcessError::from_dsp_error)?;

        let mut filter_left = DspFilter::new().map_err(ProcessError::from_filter_error)?;
        let mut filter_right = DspFilter::new().map_err(ProcessError::from_filter_error)?;
        filter_left
            .prepare(spec.sample_rate)
            .map_err(ProcessError::from_filter_error)?;
        filter_right
            .prepare(spec.sample_rate)
            .map_err(ProcessError::from_filter_error)?;
        let default_cutoff = compiled.voice_filter.map_or(1.0, |filter| filter.cutoff_hz);
        let (pan_left, pan_right) = constant_power_pan(compiled_layer.pan);
        let gain_smoothing_frames =
            rounded_frame_count(spec.sample_rate * GAIN_SMOOTHING_SECONDS).max(1);
        let filter_smoothing_frames =
            rounded_frame_count(spec.sample_rate * FILTER_SMOOTHING_SECONDS).max(1);
        Ok(Self {
            state: VoiceState::Idle,
            note_id: None,
            note_number: 0,
            velocity: 0,
            started_at_frame: 0,
            estimated_level: 0.0,
            layer: LayerRuntime {
                trigger: compiled_layer.trigger,
                envelope: AdsrRuntime::new(compiled_layer.envelope),
                oscillator,
                gain_linear: compiled_layer.gain_linear,
                pan_left,
                pan_right,
                tuning_ratio: compiled_layer.tuning_ratio,
                frequency_hz: 0.0,
                active: false,
                gain_smoother: Smoother::new(0.0),
            },
            filter_left,
            filter_right,
            filter: compiled.voice_filter,
            filter_cutoff: Smoother::new(default_cutoff),
            default_filter_cutoff: default_cutoff,
            pending: None,
            steal_fade_total: 0,
            steal_fade_remaining: 0,
            gain_smoothing_frames,
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
        if self.note_id != Some(note_id) {
            return;
        }
        if self.state == VoiceState::Active {
            self.layer.envelope.note_off();
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
        if self.state == VoiceState::Idle || !self.layer.active {
            return Ok(());
        }
        self.layer
            .oscillator
            .process(self.layer.frequency_hz, &mut layer_mono[..frames])
            .map_err(ProcessError::from_dsp_error)?;
        for index in 0..frames {
            let envelope = self.layer.envelope.next_sample();
            let gain = self.layer.gain_smoother.next();
            let mono = layer_mono[index] * envelope * gain;
            voice_left[index] = mono * self.layer.pan_left;
            voice_right[index] = mono * self.layer.pan_right;
            if self.layer.envelope.is_idle() {
                self.layer.active = false;
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

        let mut peak = 0.0_f32;
        if self.state == VoiceState::StealFading {
            #[allow(clippy::cast_precision_loss)]
            let total = self.steal_fade_total.max(1) as f32;
            for index in 0..frames {
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
            for index in 0..frames {
                peak = peak
                    .max(voice_left[index].abs())
                    .max(voice_right[index].abs());
            }
        }
        self.estimated_level = self.estimated_level.mul_add(0.95, peak * 0.05);
        if self.state != VoiceState::StealFading && !self.layer.active {
            self.reset_to_idle()?;
        } else if self.state == VoiceState::StealFading && self.steal_fade_remaining == 0 {
            self.complete_steal(sample_rate, velocity_response)?;
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) -> Result<(), ProcessError> {
        self.reset_to_idle()
    }

    fn start_note(
        &mut self,
        note: NoteRequest,
        sample_rate: f64,
        velocity_response: crate::compiler::CompiledVelocityResponse,
    ) -> Result<(), ProcessError> {
        self.reset_dsp_state()?;
        self.note_id = Some(note.note_id);
        self.note_number = note.note_number;
        self.velocity = note.velocity;
        self.started_at_frame = note.started_at_frame;
        if note.note_number < self.layer.trigger.key_min
            || note.note_number > self.layer.trigger.key_max
            || note.velocity < self.layer.trigger.velocity_min
            || note.velocity > self.layer.trigger.velocity_max
        {
            self.reset_to_idle()?;
            return Ok(());
        }
        self.layer.frequency_hz = midi_note_frequency(note.note_number, self.layer.tuning_ratio);
        if !self.layer.frequency_hz.is_finite()
            || self.layer.frequency_hz < 0.0
            || f64::from(self.layer.frequency_hz) > sample_rate * 0.5
        {
            self.reset_to_idle()?;
            return Err(ProcessError::InvalidFrequency);
        }
        self.layer.envelope.note_on();
        self.layer.active = true;
        let target_gain = self.layer.gain_linear
            * velocity_gain(note.velocity, velocity_response.layer_gain_amount);
        self.layer.gain_smoother.reset(0.0);
        self.layer
            .gain_smoother
            .set_target(target_gain, self.gain_smoothing_frames);
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
        self.reset_dsp_state()?;
        self.note_id = None;
        self.state = VoiceState::Idle;
        self.steal_fade_total = 0;
        self.steal_fade_remaining = 0;
        if let Some(note) = pending {
            self.start_note(note, sample_rate, velocity_response)?;
        }
        Ok(())
    }

    fn reset_to_idle(&mut self) -> Result<(), ProcessError> {
        self.reset_dsp_state()?;
        self.state = VoiceState::Idle;
        self.note_id = None;
        self.velocity = 0;
        self.estimated_level = 0.0;
        self.pending = None;
        self.steal_fade_total = 0;
        self.steal_fade_remaining = 0;
        self.layer.active = false;
        self.layer.envelope.reset();
        self.layer.gain_smoother.reset(0.0);
        self.filter_cutoff.reset(self.default_filter_cutoff);
        Ok(())
    }

    fn reset_dsp_state(&mut self) -> Result<(), ProcessError> {
        self.layer
            .oscillator
            .reset()
            .map_err(ProcessError::from_dsp_error)?;
        self.filter_left
            .reset()
            .map_err(ProcessError::from_filter_error)?;
        self.filter_right
            .reset()
            .map_err(ProcessError::from_filter_error)?;
        self.layer.envelope.reset();
        self.layer.active = false;
        Ok(())
    }
}
