use std::sync::Arc;

use crate::compiler::{
    CompiledGenerator, CompiledInstrument, CompiledInstrumentSourceKind, CompiledProcessor,
    CompiledProcessorKind,
};
use crate::parameter::{ParameterHandle, ParameterOwner};
use crate::process::{
    InstrumentProcessor, ProcessBlock, ProcessContext, ProcessError, ProcessSpec, clear_output,
};
use crate::trace::{TraceObservation, TraceVoice, TraceVoiceState};

use super::external_audio::EnvelopeFollowerRuntime;
use super::external_audio::ExternalAudioBlock;
use super::modulation::{
    ParameterSpanValue, SharedParameterSpan, ValueSpan, apply_domain_sum_with_maximum,
    route_domain_delta,
};
use super::processor::{ProcessorTargetSpan, StereoProcessorChain};
use super::smoothing::{Smoother, rounded_frame_count};
use super::source::ceil_boundary_frames;
use super::voice::{PreparedLayerSelection, VoiceRuntime, VoiceState};

mod event;
mod trace;

const CONTROL_SMOOTHING_SECONDS: f64 = 0.005;
const STEAL_FADE_SECONDS: f64 = 0.005;
const QUANTUM_FRAMES: usize = 32;
const MAX_MONOPHONIC_HELD_NOTES: usize = 128;

#[allow(clippy::cast_possible_truncation)]
fn phase_fraction(position: f64) -> f32 {
    position.fract() as f32
}

#[allow(clippy::cast_possible_truncation)]
fn phase_endpoint_fraction(position: f64) -> f32 {
    let nearest_integer = position.round();
    if position > 0.0 && (position - nearest_integer).abs() <= 1.0e-9 {
        1.0
    } else {
        position.fract() as f32
    }
}

fn phase_span(start: f64, end: f64) -> ValueSpan {
    ValueSpan {
        start: phase_fraction(start),
        end: phase_endpoint_fraction(end),
    }
}

#[derive(Debug, Clone, Copy)]
struct HeldNote {
    note_id: crate::process::NoteId,
    note_number: u8,
    velocity: u8,
}

struct RuntimeScratch {
    layer_mono: Vec<f32>,
    layer_left: Vec<f32>,
    layer_right: Vec<f32>,
    voice_left: Vec<f32>,
    voice_right: Vec<f32>,
    parameter_spans: Vec<ParameterSpanValue>,
    instrument_source_spans: Vec<ParameterSpanValue>,
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
    sustain_down: bool,
    held_notes: Vec<HeldNote>,
    global_processors: Option<StereoProcessorChain>,
    global_targets: Vec<ProcessorTargetSpan>,
    round_robin_counters: Vec<Vec<u64>>,
    spec: Option<ProcessSpec>,
    absolute_frame: u64,
    instrument_source_states: Vec<Option<EnvelopeFollowerRuntime>>,
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
                instrument_source_spans: Vec::new(),
            },
            note_layer_selection: Vec::new(),
            parameter_states: Vec::new(),
            pitch_bend: Smoother::new(0.0),
            mod_wheel: Smoother::new(0.0),
            aftertouch: Smoother::new(0.0),
            sustain_down: false,
            held_notes: Vec::new(),
            global_processors: None,
            global_targets: Vec::new(),
            round_robin_counters: Vec::new(),
            spec: None,
            absolute_frame: 0,
            instrument_source_states: Vec::new(),
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

    pub(crate) fn trace_snapshots(
        &self,
        handles: &[ParameterHandle],
        frame: u64,
        sample_rate: f64,
        context: ProcessContext,
    ) -> Result<Vec<(ParameterHandle, TraceObservation)>, ProcessError> {
        let mut observations = Vec::new();
        for &handle in handles {
            let descriptor = self.compiled.parameter_descriptor(handle).ok_or(
                ProcessError::ParameterHandleOutOfRange {
                    handle: handle.index(),
                },
            )?;
            let layer_owned = matches!(
                descriptor.owner,
                ParameterOwner::Layer { .. }
                    | ParameterOwner::LayerGenerator { .. }
                    | ParameterOwner::LayerProcessor { .. }
            );
            let layer_index = match descriptor.owner {
                ParameterOwner::Layer { definition_index }
                | ParameterOwner::LayerGenerator { definition_index }
                | ParameterOwner::LayerProcessor {
                    definition_index, ..
                } => self
                    .compiled
                    .layers
                    .iter()
                    .position(|layer| layer.definition_index == definition_index),
                ParameterOwner::VoiceProcessor { .. }
                | ParameterOwner::GlobalProcessor { .. }
                | ParameterOwner::Macro { .. }
                | ParameterOwner::VectorAxis { .. } => None,
            };
            if layer_owned && layer_index.is_none() {
                continue;
            }
            if matches!(
                descriptor.owner,
                ParameterOwner::GlobalProcessor { .. } | ParameterOwner::Macro { .. }
            ) {
                observations.push((
                    handle,
                    self.trace_observation(handle, frame, sample_rate, context, None, None)?,
                ));
                continue;
            }
            for (voice_index, voice) in self.voices.iter().enumerate() {
                let Some((note_id, note_number, velocity, state)) = voice.trace_identity() else {
                    continue;
                };
                if let Some(layer_index) = layer_index
                    && !voice.trace_layer_active(layer_index)
                {
                    continue;
                }
                let trace_state = match state {
                    VoiceState::Active => TraceVoiceState::Active,
                    VoiceState::Releasing => TraceVoiceState::Releasing,
                    VoiceState::StealFading => TraceVoiceState::StealFading,
                    VoiceState::Idle => continue,
                };
                let voice_info = TraceVoice {
                    index: voice_index,
                    note_id,
                    note_number,
                    velocity,
                    state: trace_state,
                };
                observations.push((
                    handle,
                    self.trace_observation(
                        handle,
                        frame,
                        sample_rate,
                        context,
                        Some(voice_info),
                        Some(voice),
                    )?,
                ));
            }
        }
        Ok(observations)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_range(
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

    pub(crate) fn shared_target_remaining(&self) -> Option<usize> {
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

    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    pub(crate) fn transport_boundary_remaining(
        &self,
        context: ProcessContext,
        offset: usize,
        sample_rate: f64,
        remaining: usize,
    ) -> Option<usize> {
        if !self.compiled.instrument_sources.iter().any(|source| {
            matches!(
                &source.source,
                CompiledInstrumentSourceKind::BeatPhase | CompiledInstrumentSourceKind::BarPhase
            )
        }) {
            return None;
        }
        let beats_per_second = context.tempo_bpm / (60.0 * sample_rate);
        if !beats_per_second.is_finite() || beats_per_second <= 0.0 {
            return None;
        }
        let offset = offset as f64;
        let beat_position = context.beat_position + offset * beats_per_second;
        let bar_position = context.bar_position
            + offset * beats_per_second / context.time_signature.beats_per_bar();
        let beats_per_bar = context.time_signature.beats_per_bar();
        let mut boundary = remaining;
        for source in &*self.compiled.instrument_sources {
            let (position, units_per_frame) = match &source.source {
                CompiledInstrumentSourceKind::BeatPhase => (beat_position, beats_per_second),
                CompiledInstrumentSourceKind::BarPhase => {
                    (bar_position, beats_per_second / beats_per_bar)
                }
                CompiledInstrumentSourceKind::PitchBend
                | CompiledInstrumentSourceKind::ModWheel
                | CompiledInstrumentSourceKind::Aftertouch
                | CompiledInstrumentSourceKind::Macro { .. }
                | CompiledInstrumentSourceKind::EnvelopeFollower(_) => continue,
            };
            let next = position.floor() + 1.0;
            let frames = (next - position) / units_per_frame;
            if frames.is_finite() && frames >= 1.0 {
                boundary = boundary.min(ceil_boundary_frames(frames));
            }
        }
        Some(boundary.max(1).min(remaining))
    }

    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    pub(crate) fn advance_shared(
        &mut self,
        frames: usize,
        context: crate::process::ProcessContext,
        offset: usize,
        sample_rate: f64,
        external: ExternalAudioBlock<'_>,
    ) -> Result<(), ProcessError> {
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
        let pitch = ValueSpan {
            start: pitch_start,
            end: pitch_end,
        };
        let wheel = ValueSpan {
            start: wheel_start,
            end: wheel_end,
        };
        let touch = ValueSpan {
            start: touch_start,
            end: touch_end,
        };
        let start_frame = context.absolute_frame.saturating_add(offset as u64);
        let start_beats = context.beat_position
            + (start_frame.saturating_sub(context.absolute_frame) as f64) * context.tempo_bpm
                / (60.0 * sample_rate);
        let end_beats = start_beats + frames as f64 * context.tempo_bpm / (60.0 * sample_rate);
        let beats_per_bar = context.time_signature.beats_per_bar();
        let start_bar = context.bar_position
            + (start_frame.saturating_sub(context.absolute_frame) as f64) * context.tempo_bpm
                / (60.0 * sample_rate * beats_per_bar);
        let end_bar =
            start_bar + frames as f64 * context.tempo_bpm / (60.0 * sample_rate * beats_per_bar);
        for (state, span) in self
            .instrument_source_states
            .iter_mut()
            .zip(&mut self.scratch.instrument_source_spans)
        {
            if let Some(state) = state {
                let start = state.value();
                for index in 0..frames {
                    state.next(external, index);
                }
                let end = state.value();
                *span = ParameterSpanValue { start, end };
            }
        }
        for (source, span) in self
            .compiled
            .instrument_sources
            .iter()
            .zip(&mut self.scratch.instrument_source_spans)
        {
            let value = match &source.source {
                CompiledInstrumentSourceKind::PitchBend => pitch,
                CompiledInstrumentSourceKind::ModWheel => wheel,
                CompiledInstrumentSourceKind::Aftertouch => touch,
                CompiledInstrumentSourceKind::Macro { parameter } => {
                    let value = self
                        .scratch
                        .parameter_spans
                        .get(parameter.index())
                        .copied()
                        .ok_or_else(invalid_state)?;
                    ValueSpan {
                        start: value.start,
                        end: value.end,
                    }
                }
                CompiledInstrumentSourceKind::BeatPhase => phase_span(start_beats, end_beats),
                CompiledInstrumentSourceKind::BarPhase => phase_span(start_bar, end_bar),
                CompiledInstrumentSourceKind::EnvelopeFollower(_) => ValueSpan {
                    start: span.start,
                    end: span.end,
                },
            };
            *span = ParameterSpanValue {
                start: value.start,
                end: value.end,
            };
        }
        Ok(())
    }

    pub(crate) fn evaluate_global_targets(
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

    #[allow(clippy::too_many_lines)]
    pub(crate) fn evaluate_global_processor_target(
        compiled: &CompiledInstrument,
        processor: &CompiledProcessor,
        shared: SharedParameterSpan<'_>,
    ) -> Result<ProcessorTargetSpan, ProcessError> {
        match &processor.processor {
            CompiledProcessorKind::Filter(value) => Ok(ProcessorTargetSpan::Filter {
                cutoff: Self::evaluate_global_target(compiled, value.parameters.cutoff, shared)?,
                resonance: Self::evaluate_global_target(
                    compiled,
                    value.parameters.resonance,
                    shared,
                )?,
            }),
            CompiledProcessorKind::LadderFilter(value) => Ok(ProcessorTargetSpan::LadderFilter {
                cutoff: Self::evaluate_global_target(compiled, value.parameters.cutoff, shared)?,
                resonance: Self::evaluate_global_target(
                    compiled,
                    value.parameters.resonance,
                    shared,
                )?,
                drive: Self::evaluate_global_target(compiled, value.parameters.drive, shared)?,
            }),
            CompiledProcessorKind::Drive(value) => Ok(ProcessorTargetSpan::Drive {
                amount: Self::evaluate_global_target(compiled, value.amount, shared)?,
                mix: Self::evaluate_global_target(compiled, value.mix, shared)?,
            }),
            CompiledProcessorKind::Eq(value) => Ok(ProcessorTargetSpan::Eq {
                low_gain_db: Self::evaluate_global_target(
                    compiled,
                    value.parameters.low_gain_db,
                    shared,
                )?,
                mid_gain_db: Self::evaluate_global_target(
                    compiled,
                    value.parameters.mid_gain_db,
                    shared,
                )?,
                high_gain_db: Self::evaluate_global_target(
                    compiled,
                    value.parameters.high_gain_db,
                    shared,
                )?,
            }),
            CompiledProcessorKind::Formant(value) => Ok(ProcessorTargetSpan::Formant {
                vowel_position: Self::evaluate_global_target(
                    compiled,
                    value.parameters.vowel_position,
                    shared,
                )?,
                formant_shift: Self::evaluate_global_target(
                    compiled,
                    value.parameters.formant_shift,
                    shared,
                )?,
                throat: Self::evaluate_global_target(compiled, value.parameters.throat, shared)?,
                mix: Self::evaluate_global_target(compiled, value.parameters.mix, shared)?,
            }),
            CompiledProcessorKind::Resonator(value) => Ok(ProcessorTargetSpan::Resonator {
                frequency_hz: Self::evaluate_global_target(
                    compiled,
                    value.parameters.frequency_hz,
                    shared,
                )?,
                decay_seconds: Self::evaluate_global_target(
                    compiled,
                    value.parameters.decay_seconds,
                    shared,
                )?,
                damping: Self::evaluate_global_target(compiled, value.parameters.damping, shared)?,
                mix: Self::evaluate_global_target(compiled, value.parameters.mix, shared)?,
            }),
            CompiledProcessorKind::Chorus(value) => Ok(ProcessorTargetSpan::Chorus {
                rate_hz: Self::evaluate_global_target(compiled, value.parameters.rate_hz, shared)?,
                depth: Self::evaluate_global_target(compiled, value.parameters.depth, shared)?,
                feedback: Self::evaluate_global_target(
                    compiled,
                    value.parameters.feedback,
                    shared,
                )?,
                width: Self::evaluate_global_target(compiled, value.parameters.width, shared)?,
                mix: Self::evaluate_global_target(compiled, value.parameters.mix, shared)?,
            }),
            CompiledProcessorKind::Flanger(value) => Ok(ProcessorTargetSpan::Flanger {
                rate_hz: Self::evaluate_global_target(compiled, value.parameters.rate_hz, shared)?,
                depth: Self::evaluate_global_target(compiled, value.parameters.depth, shared)?,
                feedback: Self::evaluate_global_target(
                    compiled,
                    value.parameters.feedback,
                    shared,
                )?,
                width: Self::evaluate_global_target(compiled, value.parameters.width, shared)?,
                mix: Self::evaluate_global_target(compiled, value.parameters.mix, shared)?,
            }),
            CompiledProcessorKind::Phaser(value) => Ok(ProcessorTargetSpan::Phaser {
                rate_hz: Self::evaluate_global_target(compiled, value.parameters.rate_hz, shared)?,
                depth: Self::evaluate_global_target(compiled, value.parameters.depth, shared)?,
                feedback: Self::evaluate_global_target(
                    compiled,
                    value.parameters.feedback,
                    shared,
                )?,
                width: Self::evaluate_global_target(compiled, value.parameters.width, shared)?,
                mix: Self::evaluate_global_target(compiled, value.parameters.mix, shared)?,
            }),
            CompiledProcessorKind::Delay(value) => Ok(ProcessorTargetSpan::Delay {
                feedback: Self::evaluate_global_target(compiled, value.feedback, shared)?,
                mix: Self::evaluate_global_target(compiled, value.mix, shared)?,
            }),
            CompiledProcessorKind::FrequencyShifter(value) => {
                Ok(ProcessorTargetSpan::FrequencyShifter {
                    shift_hz: Self::evaluate_global_target(
                        compiled,
                        value.parameters.shift_hz,
                        shared,
                    )?,
                    mix: Self::evaluate_global_target(compiled, value.parameters.mix, shared)?,
                })
            }
            CompiledProcessorKind::Reverb(value) => Ok(ProcessorTargetSpan::Reverb {
                decay: Self::evaluate_global_target(compiled, value.decay, shared)?,
                damping: Self::evaluate_global_target(compiled, value.damping, shared)?,
                width: Self::evaluate_global_target(compiled, value.width, shared)?,
                mix: Self::evaluate_global_target(compiled, value.mix, shared)?,
            }),
            CompiledProcessorKind::Convolution(value) => Ok(ProcessorTargetSpan::Convolution {
                gain_db: Self::evaluate_global_target(compiled, value.parameters.gain_db, shared)?,
                mix: Self::evaluate_global_target(compiled, value.parameters.mix, shared)?,
            }),
            CompiledProcessorKind::Gate(value) => Ok(ProcessorTargetSpan::Gate {
                threshold_db: Self::evaluate_global_target(
                    compiled,
                    value.parameters.threshold_db,
                    shared,
                )?,
                range_db: Self::evaluate_global_target(
                    compiled,
                    value.parameters.range_db,
                    shared,
                )?,
            }),
            CompiledProcessorKind::TransientShaper(value) => {
                Ok(ProcessorTargetSpan::TransientShaper {
                    attack: Self::evaluate_global_target(
                        compiled,
                        value.parameters.attack,
                        shared,
                    )?,
                    sustain: Self::evaluate_global_target(
                        compiled,
                        value.parameters.sustain,
                        shared,
                    )?,
                    mix: Self::evaluate_global_target(compiled, value.parameters.mix, shared)?,
                })
            }
            CompiledProcessorKind::Compressor(value) => Ok(ProcessorTargetSpan::Compressor {
                threshold_db: Self::evaluate_global_target(
                    compiled,
                    value.parameters.threshold_db,
                    shared,
                )?,
                ratio: Self::evaluate_global_target(compiled, value.parameters.ratio, shared)?,
                makeup_gain_db: Self::evaluate_global_target(
                    compiled,
                    value.parameters.makeup_gain_db,
                    shared,
                )?,
                mix: Self::evaluate_global_target(compiled, value.parameters.mix, shared)?,
            }),
            CompiledProcessorKind::Vocoder(value) => Ok(ProcessorTargetSpan::Vocoder {
                modulator_gain_db: Self::evaluate_global_target(
                    compiled,
                    value.parameters.modulator_gain_db,
                    shared,
                )?,
                output_gain_db: Self::evaluate_global_target(
                    compiled,
                    value.parameters.output_gain_db,
                    shared,
                )?,
                mix: Self::evaluate_global_target(compiled, value.parameters.mix, shared)?,
            }),
            CompiledProcessorKind::EnvelopeTransfer(value) => {
                Ok(ProcessorTargetSpan::EnvelopeTransfer {
                    input_gain_db: Self::evaluate_global_target(
                        compiled,
                        value.parameters.input_gain_db,
                        shared,
                    )?,
                    floor_db: Self::evaluate_global_target(
                        compiled,
                        value.parameters.floor_db,
                        shared,
                    )?,
                    mix: Self::evaluate_global_target(compiled, value.parameters.mix, shared)?,
                })
            }
            CompiledProcessorKind::SpectralMorph(value) => Ok(ProcessorTargetSpan::SpectralMorph {
                morph: Self::evaluate_global_target(compiled, value.parameters.morph, shared)?,
                output_gain_db: Self::evaluate_global_target(
                    compiled,
                    value.parameters.output_gain_db,
                    shared,
                )?,
            }),
            CompiledProcessorKind::Limiter(value) => Ok(ProcessorTargetSpan::Limiter {
                ceiling_db: Self::evaluate_global_target(
                    compiled,
                    value.parameters.ceiling_db,
                    shared,
                )?,
                input_gain_db: Self::evaluate_global_target(
                    compiled,
                    value.parameters.input_gain_db,
                    shared,
                )?,
            }),
            CompiledProcessorKind::Bitcrusher(_) => Err(invalid_state()),
        }
    }

    pub(crate) fn evaluate_global_target(
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
                crate::compiler::CompiledSourceRef::Instrument(handle) => {
                    shared.instrument_source(handle).ok_or_else(invalid_state)?
                }
                crate::compiler::CompiledSourceRef::Voice(_) => return Err(invalid_state()),
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
}

impl InstrumentRuntime {
    pub(crate) fn reset_for_prepare(&mut self) {
        self.spec = None;
        self.voices.clear();
        self.scratch.layer_mono.clear();
        self.scratch.layer_left.clear();
        self.scratch.layer_right.clear();
        self.scratch.voice_left.clear();
        self.scratch.voice_right.clear();
        self.scratch.parameter_spans.clear();
        self.scratch.instrument_source_spans.clear();
        self.note_layer_selection.clear();
        self.parameter_states.clear();
        self.pitch_bend.reset(0.0);
        self.mod_wheel.reset(0.0);
        self.aftertouch.reset(0.0);
        self.sustain_down = false;
        self.held_notes.clear();
        if self.held_notes.capacity() < MAX_MONOPHONIC_HELD_NOTES {
            self.held_notes
                .reserve(MAX_MONOPHONIC_HELD_NOTES - self.held_notes.capacity());
        }
        self.global_processors = None;
        self.global_targets.clear();
        self.round_robin_counters.clear();
        self.absolute_frame = 0;
        self.instrument_source_states.clear();
    }

    pub(crate) fn prepare_inner(&mut self, spec: ProcessSpec) -> Result<(), ProcessError> {
        self.reset_for_prepare();

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
        let required_input_channels = self.compiled.required_input_channels();
        if spec.input_channels != required_input_channels {
            return Err(ProcessError::InputChannelRequirementMismatch {
                compiled: required_input_channels,
                requested: spec.input_channels,
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
        self.scratch.instrument_source_spans.resize(
            self.compiled.instrument_sources.len(),
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
        self.instrument_source_states = self
            .compiled
            .instrument_sources
            .iter()
            .map(|source| match &source.source {
                CompiledInstrumentSourceKind::EnvelopeFollower(compiled) => {
                    Some(EnvelopeFollowerRuntime::new(*compiled))
                }
                _ => None,
            })
            .collect();
        self.round_robin_counters = self
            .compiled
            .layers
            .iter()
            .map(|layer| match &layer.generator {
                CompiledGenerator::Sample(sample) => vec![0; sample.groups.len()],
                CompiledGenerator::Oscillator(_)
                | CompiledGenerator::Noise(_)
                | CompiledGenerator::PhysicalString(_)
                | CompiledGenerator::Modal(_)
                | CompiledGenerator::Additive(_)
                | CompiledGenerator::Formant(_)
                | CompiledGenerator::Granular(_)
                | CompiledGenerator::WaveSequence(_)
                | CompiledGenerator::Wavetable(_)
                | CompiledGenerator::Spectral(_)
                | CompiledGenerator::OperatorModulation(_) => Vec::new(),
            })
            .collect();
        let mut voices = Vec::with_capacity(self.compiled.performance.voice_count);
        for _ in 0..self.compiled.performance.voice_count {
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
}

impl InstrumentRuntime {
    #[allow(clippy::too_many_lines)]
    pub(crate) fn process_inner(
        &mut self,
        block: &mut ProcessBlock<'_>,
    ) -> Result<(), ProcessError> {
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
            if let Some(remaining) = self.transport_boundary_remaining(
                block.context,
                cursor,
                spec.sample_rate,
                block.frames - cursor,
            ) {
                end = end.min(cursor + remaining);
            }
            if end <= cursor {
                end = cursor + 1;
            }

            let frames = end - cursor;
            let external_channels = [
                block
                    .input
                    .first()
                    .and_then(|channel| channel.get(cursor..end))
                    .unwrap_or(&[]),
                block
                    .input
                    .get(1)
                    .and_then(|channel| channel.get(cursor..end))
                    .unwrap_or(&[]),
            ];
            let external = ExternalAudioBlock::new(
                &external_channels[..block.input.len().min(external_channels.len())],
            );
            if let Err(error) =
                self.advance_shared(frames, block.context, cursor, spec.sample_rate, external)
            {
                clear_output(&mut *block.output, block.frames);
                self.spec = None;
                return Err(error);
            }
            let RuntimeScratch {
                layer_mono,
                layer_left,
                layer_right,
                voice_left,
                voice_right,
                parameter_spans,
                instrument_source_spans,
            } = &mut self.scratch;
            let shared =
                SharedParameterSpan::new(&*parameter_spans, &*instrument_source_spans, frames);
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
                                processors.process(
                                    &self.global_targets,
                                    block.context.tempo_bpm,
                                    external,
                                    left,
                                    right,
                                )
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
}

impl InstrumentProcessor for InstrumentRuntime {
    fn prepare(&mut self, spec: ProcessSpec) -> Result<(), ProcessError> {
        self.prepare_inner(spec)
    }

    fn process(&mut self, block: ProcessBlock<'_>) -> Result<(), ProcessError> {
        let mut block = block;
        self.process_inner(&mut block)
    }

    fn reset(&mut self) -> Result<(), ProcessError> {
        if self.spec.is_none() {
            return Err(ProcessError::NotPrepared);
        }
        for voice in &mut self.voices {
            if let Err(error) = voice.reset(&self.compiled) {
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
        for state in self.instrument_source_states.iter_mut().flatten() {
            state.reset();
        }
        self.pitch_bend.reset(0.0);
        self.mod_wheel.reset(0.0);
        self.aftertouch.reset(0.0);
        self.sustain_down = false;
        self.held_notes.clear();
        self.scratch.layer_mono.fill(0.0);
        self.scratch.layer_left.fill(0.0);
        self.scratch.layer_right.fill(0.0);
        self.scratch.voice_left.fill(0.0);
        self.scratch.voice_right.fill(0.0);
        self.scratch.parameter_spans.fill(ParameterSpanValue {
            start: 0.0,
            end: 0.0,
        });
        self.scratch
            .instrument_source_spans
            .fill(ParameterSpanValue {
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

fn control_smoothing_frames(sample_rate: f64) -> usize {
    rounded_frame_count(sample_rate * CONTROL_SMOOTHING_SECONDS).max(1)
}

fn invalid_state() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: crate::process::ProcessorFailureKind::InvalidState,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use approx::assert_relative_eq;

    use std::sync::Arc;

    use super::InstrumentRuntime;
    use crate::compiler::{CompileContext, CompiledInstrument, compile_instrument};
    use crate::definition::tests::definition;
    use crate::process::{
        InstrumentProcessor, ProcessBlock, ProcessContext, ProcessError, ProcessEvent,
        ProcessEventKind, ProcessSpec,
    };
    use crate::runtime::external_audio::ExternalAudioBlock;
    use crate::runtime::processor::ProcessorTargetSpan;

    pub(crate) fn compiled(polyphony: u16) -> Arc<CompiledInstrument> {
        let mut definition = definition();
        definition.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
        compile_instrument(
            &definition,
            &CompileContext {
                definition_base_dir: ".".into(),
                process_spec: ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid spec"),
            },
        )
        .instrument
        .expect("definition compiles")
    }

    pub(crate) fn runtime_with(
        definition: &crate::definition::InstrumentDefinition,
    ) -> InstrumentRuntime {
        let result = compile_instrument(
            definition,
            &CompileContext {
                definition_base_dir: ".".into(),
                process_spec: ProcessSpec::new(48_000.0, 257, 0, 2).expect("valid spec"),
            },
        );
        result.instrument.expect("compiled").instantiate()
    }

    pub(crate) fn external_runtime_with(
        definition: &crate::definition::InstrumentDefinition,
    ) -> InstrumentRuntime {
        let result = compile_instrument(
            definition,
            &CompileContext {
                definition_base_dir: ".".into(),
                process_spec: ProcessSpec::new(48_000.0, 64, 2, 2).expect("valid spec"),
            },
        );
        result.instrument.expect("compiled").instantiate()
    }

    pub(crate) fn runtime() -> InstrumentRuntime {
        runtime_with(&definition())
    }

    pub(crate) fn monophonic_definition(
        legato: bool,
        portamento_seconds: Option<f32>,
    ) -> crate::definition::InstrumentDefinition {
        let mut value = definition();
        value.performance = crate::definition::PerformanceDefinition::Monophonic {
            legato,
            portamento: portamento_seconds
                .map(|time_seconds| crate::definition::PortamentoDefinition { time_seconds }),
        };
        value
    }

    pub(crate) fn prepare(runtime: &mut InstrumentRuntime) {
        runtime
            .prepare(ProcessSpec::new(48_000.0, 257, 0, 2).expect("valid spec"))
            .expect("prepare");
    }

    pub(crate) fn process(
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
                    beat_position: 0.0,
                    bar_position: 0.0,
                    time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                },
                events,
                input: &[],
                output: &mut output,
            })
            .expect("process succeeds");
        vec![left, right]
    }

    pub(crate) fn traced_source_value(runtime: &InstrumentRuntime, frame: u64) -> f32 {
        let handle = runtime
            .compiled
            .parameter_handle("layer.body.tuning")
            .expect("tuning parameter");
        let context = ProcessContext {
            absolute_frame: frame,
            tempo_bpm: 120.0,
            beat_position: 0.0,
            bar_position: 0.0,
            time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
        };
        let (_, observation) = runtime
            .trace_snapshots(&[handle], frame, 48_000.0, context)
            .expect("trace source")
            .into_iter()
            .next()
            .expect("voice observation");
        observation.routes.first().expect("source route").raw
    }

    #[test]
    pub(crate) fn transport_phase_wrap_and_bar_boundaries_use_their_own_units() {
        let definition_for = |source: &str| {
            let mut definition = definition();
            definition.modulation = Some(crate::definition::ModulationDefinition {
                sources: Vec::new(),
                routes: vec![crate::definition::ModulationRouteDefinition {
                    source: source.to_owned(),
                    target: "layer.body.tuning".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 120.0,
                        unit: crate::parameter::ModulationUnit::Cents,
                    },
                    curve: crate::definition::ModulationCurve::Linear,
                }],
            });
            definition
        };
        let beat_context = ProcessContext {
            absolute_frame: 0,
            tempo_bpm: 120.0,
            beat_position: 0.99,
            bar_position: 0.2475,
            time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
        };
        let mut beat_runtime = runtime_with(&definition_for("transport_beat_phase"));
        prepare(&mut beat_runtime);
        assert_eq!(
            beat_runtime.transport_boundary_remaining(beat_context, 0, 48_000.0, 256),
            Some(240)
        );
        beat_runtime
            .advance_shared(240, beat_context, 0, 48_000.0, ExternalAudioBlock::new(&[]))
            .expect("beat phase span");
        let beat_span = beat_runtime.scratch.instrument_source_spans[0];
        assert!((beat_span.start - 0.99).abs() < 1.0e-6);
        assert!((beat_span.end - 1.0).abs() < 1.0e-6);

        let mut bar_runtime = runtime_with(&definition_for("transport_bar_phase"));
        prepare(&mut bar_runtime);
        let bar_context = ProcessContext {
            beat_position: 0.0,
            bar_position: 0.999,
            ..beat_context
        };
        for (time_signature, expected_frames) in [
            (crate::process::DEFAULT_TIME_SIGNATURE, 96),
            (
                crate::process::TimeSignature {
                    numerator: 7,
                    denominator: 8,
                },
                84,
            ),
            (
                crate::process::TimeSignature {
                    numerator: 1,
                    denominator: 8,
                },
                12,
            ),
        ] {
            let context = ProcessContext {
                time_signature,
                ..bar_context
            };
            assert_eq!(
                bar_runtime.transport_boundary_remaining(context, 0, 48_000.0, 256),
                Some(expected_frames)
            );
        }
        bar_runtime
            .advance_shared(96, bar_context, 0, 48_000.0, ExternalAudioBlock::new(&[]))
            .expect("bar phase span");
        let bar_span = bar_runtime.scratch.instrument_source_spans[0];
        assert!((bar_span.start - 0.999).abs() < 1.0e-6);
        assert!((bar_span.end - 1.0).abs() < 1.0e-6);
    }

    pub(crate) fn process_with_stack_output(
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
                    beat_position: 0.0,
                    bar_position: 0.0,
                    time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                },
                events,
                input: &[],
                output: &mut output,
            })
            .expect("process succeeds");
    }

    pub(crate) fn process_with_external_stack_output(
        runtime: &mut InstrumentRuntime,
        absolute_frame: u64,
        events: &[ProcessEvent],
    ) {
        const FRAMES: usize = 64;
        let mut left = [0.0_f32; FRAMES];
        let mut right = [0.0_f32; FRAMES];
        let external_left = [0.5_f32; FRAMES];
        let external_right = [-0.25_f32; FRAMES];
        let input: [&[f32]; 2] = [&external_left, &external_right];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        runtime
            .process(ProcessBlock {
                frames: FRAMES,
                context: crate::process::ProcessContext {
                    absolute_frame,
                    tempo_bpm: 120.0,
                    beat_position: 0.0,
                    bar_position: 0.0,
                    time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                },
                events,
                input: &input,
                output: &mut output,
            })
            .expect("process succeeds");
    }

    pub(crate) fn write_pcm_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
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

    pub(crate) fn sample_stretch_definition(
        path: &std::path::Path,
    ) -> crate::definition::InstrumentDefinition {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
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
    pub(crate) fn idle_note_on_does_not_allocate_after_prepare() {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let event = [
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 100,
                },
            },
            ProcessEvent {
                sample_offset: 1,
                kind: ProcessEventKind::SustainPedal { down: true },
            },
            ProcessEvent {
                sample_offset: 2,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            },
            ProcessEvent {
                sample_offset: 3,
                kind: ProcessEventKind::SustainPedal { down: false },
            },
        ];

        let _ = process(&mut runtime, 64, 0, &event);
        runtime.reset().expect("reset");

        let allocations = crate::test_allocator::count_allocations(|| {
            process_with_stack_output(&mut runtime, 0, &event);
        });

        assert_eq!(allocations, 0);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    pub(crate) fn processor_expansion_render_does_not_allocate_after_prepare() {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
        source.layers[0].processors = vec![
            crate::definition::ProcessorDefinition::Eq(crate::definition::EqProcessorDefinition {
                id: "layer_eq".to_owned(),
                low_frequency_hz: 120.0,
                low_gain_db: 2.0,
                mid_frequency_hz: 1_000.0,
                mid_gain_db: -2.0,
                mid_q: 1.0,
                high_frequency_hz: 8_000.0,
                high_gain_db: 1.0,
            }),
            crate::definition::ProcessorDefinition::Resonator(
                crate::definition::ResonatorProcessorDefinition {
                    id: "layer_ring".to_owned(),
                    frequency_hz: 220.0,
                    decay_seconds: 0.4,
                    damping: 0.3,
                    mix: 0.2,
                },
            ),
            crate::definition::ProcessorDefinition::Bitcrusher(
                crate::definition::BitcrusherProcessorDefinition {
                    id: "layer_crush".to_owned(),
                    bit_depth: 8.0,
                    sample_rate_ratio: 0.5,
                    mix: 0.2,
                },
            ),
        ];
        source.voice_processors = vec![
            crate::definition::ProcessorDefinition::Eq(crate::definition::EqProcessorDefinition {
                id: "voice_eq".to_owned(),
                low_frequency_hz: 120.0,
                low_gain_db: 0.0,
                mid_frequency_hz: 1_000.0,
                mid_gain_db: 0.0,
                mid_q: 1.0,
                high_frequency_hz: 8_000.0,
                high_gain_db: 0.0,
            }),
            crate::definition::ProcessorDefinition::Resonator(
                crate::definition::ResonatorProcessorDefinition {
                    id: "voice_ring".to_owned(),
                    frequency_hz: 440.0,
                    decay_seconds: 0.2,
                    damping: 0.5,
                    mix: 0.1,
                },
            ),
            crate::definition::ProcessorDefinition::Compressor(
                crate::definition::CompressorProcessorDefinition {
                    id: "voice_compressor".to_owned(),
                    threshold_db: -18.0,
                    ratio: 4.0,
                    attack_ms: 15.0,
                    release_ms: 180.0,
                    knee_db: 6.0,
                    makeup_gain_db: 2.0,
                    mix: 1.0,
                    detector: crate::definition::DynamicsDetectorDefinition::SelfSignal,
                },
            ),
            crate::definition::ProcessorDefinition::Limiter(
                crate::definition::LimiterProcessorDefinition {
                    id: "voice_limiter".to_owned(),
                    ceiling_db: -1.0,
                    release_ms: 80.0,
                    input_gain_db: 0.0,
                },
            ),
        ];
        source.global_processors = vec![
            crate::definition::ProcessorDefinition::Eq(crate::definition::EqProcessorDefinition {
                id: "global_eq".to_owned(),
                low_frequency_hz: 120.0,
                low_gain_db: 0.0,
                mid_frequency_hz: 1_000.0,
                mid_gain_db: 0.0,
                mid_q: 1.0,
                high_frequency_hz: 8_000.0,
                high_gain_db: 0.0,
            }),
            crate::definition::ProcessorDefinition::Chorus(
                crate::definition::ChorusProcessorDefinition {
                    id: "chorus".to_owned(),
                    delay_ms: 15.0,
                    rate_hz: 0.35,
                    depth: 0.65,
                    feedback: 0.1,
                    width: 0.8,
                    mix: 0.3,
                },
            ),
            crate::definition::ProcessorDefinition::Flanger(
                crate::definition::FlangerProcessorDefinition {
                    id: "flanger".to_owned(),
                    delay_ms: 2.0,
                    rate_hz: 0.25,
                    depth: 0.8,
                    feedback: 0.55,
                    width: 0.5,
                    mix: 0.2,
                },
            ),
            crate::definition::ProcessorDefinition::Phaser(
                crate::definition::PhaserProcessorDefinition {
                    id: "phaser".to_owned(),
                    stages: 6,
                    center_hz: 900.0,
                    sweep_octaves: 3.0,
                    rate_hz: 0.3,
                    depth: 0.8,
                    feedback: 0.4,
                    width: 0.7,
                    mix: 0.2,
                },
            ),
        ];
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
        process_with_stack_output(&mut runtime, 0, &event);
        runtime.reset().expect("reset");

        let allocations = crate::test_allocator::count_allocations(|| {
            process_with_stack_output(&mut runtime, 0, &event);
        });

        assert_eq!(allocations, 0);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    pub(crate) fn external_cross_synthesis_render_does_not_allocate_after_prepare() {
        let mut source = definition();
        source.external_audio = Some(crate::definition::ExternalAudioInputDefinition {
            channels: crate::definition::ExternalAudioChannels::Stereo,
        });
        source.voice_processors = vec![crate::definition::ProcessorDefinition::Filter(
            crate::definition::FilterProcessorDefinition {
                id: "tone".to_owned(),
                mode: crate::definition::FilterModeDefinition::LowPass,
                cutoff_hz: 12_000.0,
                resonance: 0.12,
            },
        )];
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![
                crate::definition::ModulationSourceDefinition::EnvelopeFollower(
                    crate::definition::EnvelopeFollowerDefinition {
                        id: "input_env".to_owned(),
                        attack_ms: 2.0,
                        release_ms: 120.0,
                        input_gain_db: 0.0,
                    },
                ),
            ],
            routes: vec![crate::definition::ModulationRouteDefinition {
                source: "input_env".to_owned(),
                target: "voice.processor.tone.cutoff".to_owned(),
                depth: crate::definition::ModulationDepthDefinition {
                    value: 2.2,
                    unit: crate::parameter::ModulationUnit::Octaves,
                },
                curve: crate::definition::ModulationCurve::SmoothStep,
            }],
        });
        source.global_processors = vec![
            crate::definition::ProcessorDefinition::Vocoder(
                crate::definition::VocoderProcessorDefinition {
                    id: "vocoder".to_owned(),
                    attack_ms: 8.0,
                    release_ms: 80.0,
                    modulator_gain_db: 0.0,
                    output_gain_db: -3.0,
                    mix: 1.0,
                },
            ),
            crate::definition::ProcessorDefinition::SpectralMorph(
                crate::definition::SpectralMorphProcessorDefinition {
                    id: "morph".to_owned(),
                    morph: 0.5,
                    output_gain_db: -3.0,
                },
            ),
            crate::definition::ProcessorDefinition::Compressor(
                crate::definition::CompressorProcessorDefinition {
                    id: "post_morph_duck".to_owned(),
                    threshold_db: -24.0,
                    ratio: 6.0,
                    attack_ms: 8.0,
                    release_ms: 180.0,
                    knee_db: 6.0,
                    makeup_gain_db: 0.0,
                    mix: 1.0,
                    detector: crate::definition::DynamicsDetectorDefinition::ExternalAudio,
                },
            ),
            crate::definition::ProcessorDefinition::Delay(
                crate::definition::DelayProcessorDefinition {
                    id: "space".to_owned(),
                    time: crate::definition::DelayTimeDefinition {
                        value: 0.18,
                        unit: crate::definition::DelayTimeUnit::Seconds,
                    },
                    feedback_mode: crate::definition::DelayFeedbackMode::Stereo,
                    feedback: 0.18,
                    taps: vec![],
                    mix: 0.12,
                },
            ),
            crate::definition::ProcessorDefinition::Limiter(
                crate::definition::LimiterProcessorDefinition {
                    id: "ceiling".to_owned(),
                    ceiling_db: -1.0,
                    release_ms: 80.0,
                    input_gain_db: -3.0,
                },
            ),
        ];
        let mut runtime = external_runtime_with(&source);
        runtime
            .prepare(ProcessSpec::new(48_000.0, 64, 2, 2).expect("valid spec"))
            .expect("prepare");
        let event = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];
        process_with_external_stack_output(&mut runtime, 0, &event);
        runtime.reset().expect("reset");
        let no_events: [ProcessEvent; 0] = [];

        let allocations = crate::test_allocator::count_allocations(|| {
            for block in 0..20 {
                process_with_external_stack_output(
                    &mut runtime,
                    u64::try_from(block).expect("block index fits") * 64,
                    if block == 0 { &event } else { &no_events },
                );
            }
        });

        assert_eq!(allocations, 0);
    }

    #[test]
    pub(crate) fn wavetable_render_does_not_allocate_after_prepare() {
        let (_directory, path) = write_pcm_fixture();
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
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
    pub(crate) fn stretch_render_does_not_allocate_in_rust_after_prepare() {
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
    pub(crate) fn operator_render_does_not_allocate_after_prepare() {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
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
    pub(crate) fn complex_oscillator_render_does_not_allocate_after_prepare() {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
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
    pub(crate) fn additive_sixteen_voice_render_does_not_allocate_after_prepare() {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 16,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
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
    pub(crate) fn formant_sixteen_voice_render_does_not_allocate_after_prepare() {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 16,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
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
    pub(crate) fn spectral_sixteen_voice_stereo_morph_render_does_not_allocate_after_prepare() {
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
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 16,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
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
    pub(crate) fn voice_stealing_note_on_does_not_allocate_after_prepare() {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
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
    pub(crate) fn prepare_requires_the_compiled_sample_rate_but_allows_block_size_changes() {
        let mut runtime = runtime();
        runtime
            .prepare(ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec"))
            .expect("matching sample rate and changed block size are valid");
    }

    #[test]
    pub(crate) fn prepare_rejects_a_sample_rate_different_from_compile_time() {
        let mut runtime = runtime();
        let error = runtime
            .prepare(ProcessSpec::new(44_100.0, 257, 0, 2).expect("valid process spec"))
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
    pub(crate) fn failed_sample_rate_prepare_invalidates_previous_runtime_and_allows_reprepare() {
        let mut runtime = runtime();
        let valid_spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
        runtime
            .prepare(valid_spec)
            .expect("initial runtime preparation");
        assert!(runtime.voice_count() > 0);

        assert_eq!(
            runtime.prepare(ProcessSpec::new(44_100.0, 64, 0, 2).expect("valid process spec")),
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
                    beat_position: 0.0,
                    bar_position: 0.0,
                    time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                },
                events: &[],
                input: &[],
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
    pub(crate) fn invalid_prepare_invalidates_previous_runtime_and_allows_reprepare() {
        let mut runtime = runtime();
        let valid_spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid process spec");
        runtime
            .prepare(valid_spec)
            .expect("initial runtime preparation");

        let invalid_spec = ProcessSpec {
            sample_rate: 48_000.0,
            max_block_size: 0,
            input_channels: 0,
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
                    beat_position: 0.0,
                    bar_position: 0.0,
                    time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                },
                events: &[],
                input: &[],
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
    pub(crate) fn prepare_accepts_an_instrument_compiled_at_44_1_khz() {
        let result = compile_instrument(
            &definition(),
            &CompileContext {
                definition_base_dir: ".".into(),
                process_spec: ProcessSpec::new(44_100.0, 257, 0, 2).expect("valid spec"),
            },
        );
        let mut runtime = result
            .instrument
            .expect("compiled instrument")
            .instantiate();
        runtime
            .prepare(ProcessSpec::new(44_100.0, 257, 0, 2).expect("valid spec"))
            .expect("matching 44.1 kHz sample rate is valid");
    }

    #[test]
    pub(crate) fn note_timing_is_independent_of_block_size() {
        let mut first = runtime();
        let mut second = runtime();
        let spec = ProcessSpec::new(48_000.0, 257, 0, 2).expect("valid spec");
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
    pub(crate) fn reset_restarts_the_same_render() {
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
    pub(crate) fn reset_restarts_round_robin_selection_from_the_first_zone() {
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

    pub(crate) fn phase_runtime_with_waveform(
        phase_reset: bool,
        waveform: crate::definition::OscillatorWaveform,
    ) -> InstrumentRuntime {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
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
            | crate::definition::GeneratorDefinition::PhysicalString(_)
            | crate::definition::GeneratorDefinition::Modal(_)
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

    pub(crate) fn phase_runtime(phase_reset: bool) -> InstrumentRuntime {
        phase_runtime_with_waveform(phase_reset, crate::definition::OscillatorWaveform::Sine)
    }

    pub(crate) fn modulated_steal_definition() -> crate::definition::InstrumentDefinition {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
        source.layers[0].envelope.attack_seconds = 0.0;
        source.layers[0].envelope.decay_seconds = 0.0;
        source.layers[0].envelope.sustain_level = 1.0;
        source.layers[0].envelope.release_seconds = 0.25;
        source
            .voice_processors
            .push(crate::definition::ProcessorDefinition::Filter(
                crate::definition::FilterProcessorDefinition {
                    id: "tone".to_owned(),
                    mode: crate::definition::FilterModeDefinition::LowPass,
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
                        rate: crate::definition::ModulationRateDefinition {
                            value: 37.0,
                            unit: crate::definition::ModulationRateUnit::PerSecond,
                        },
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
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 36.0,
                        unit: crate::parameter::ModulationUnit::Decibels,
                    },
                    curve: crate::definition::ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "steal_lfo".to_owned(),
                    target: "voice.processor.tone.cutoff".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 2.491_446_5,
                        unit: crate::parameter::ModulationUnit::Octaves,
                    },
                    curve: crate::definition::ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "steal_envelope".to_owned(),
                    target: "layer.body.tuning".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 240.0,
                        unit: crate::parameter::ModulationUnit::Cents,
                    },
                    curve: crate::definition::ModulationCurve::Linear,
                },
            ],
        });
        source
    }

    #[test]
    pub(crate) fn static_and_dynamic_targets_render_finite_audio() {
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

    pub(crate) fn process_parameter_event(
        runtime: &mut InstrumentRuntime,
        event: ProcessEventKind,
    ) {
        let mut left = [1.0_f32; 32];
        let mut right = [1.0_f32; 32];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        runtime
            .process(ProcessBlock {
                frames: 32,
                context: crate::process::ProcessContext {
                    absolute_frame: 0,
                    tempo_bpm: 120.0,
                    beat_position: 0.0,
                    bar_position: 0.0,
                    time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                },
                events: &[ProcessEvent {
                    sample_offset: 0,
                    kind: event,
                }],
                input: &[],
                output: &mut output,
            })
            .expect("parameter event process");
    }

    #[test]
    pub(crate) fn global_feedback_targets_remain_within_descriptor_limits() {
        let mut source = definition();
        source.global_processors = vec![
            crate::definition::ProcessorDefinition::Delay(
                crate::definition::DelayProcessorDefinition {
                    id: "echo".to_owned(),
                    time: crate::definition::DelayTimeDefinition {
                        value: 0.25,
                        unit: crate::definition::DelayTimeUnit::Seconds,
                    },
                    feedback_mode: crate::definition::DelayFeedbackMode::Stereo,
                    feedback: 0.5,
                    taps: vec![],
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
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 0.94,
                        unit: crate::parameter::ModulationUnit::Normalized,
                    },
                    curve: crate::definition::ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "mod_wheel".to_owned(),
                    target: "global.processor.space.decay".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 0.96,
                        unit: crate::parameter::ModulationUnit::Normalized,
                    },
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
