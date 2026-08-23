//! Runtime state and advancement for voice-scoped modulation sources.

use crate::compiler::{
    CompiledMseg, CompiledSampleHold, CompiledSmoothRandom, CompiledStep, CompiledVoiceSource,
};
use crate::definition::{ModulationDurationUnit, ModulationRateUnit, ModulationSegmentCurve};
use crate::process::{NoteId, ProcessError};

use super::adsr::AdsrRuntime;
use super::modulation::ValueSpan;
use super::random::{bipolar_f32, splitmix64_finalizer};

/// Mutable state for one compiled voice source.
pub(crate) enum VoiceSourceRuntime {
    /// Note velocity.
    Velocity(f32),
    /// MIDI key tracking.
    KeyTracking(f32),
    /// Periodic oscillator phase.
    Lfo { phase: f32 },
    /// Note lifecycle envelope.
    Envelope(AdsrRuntime),
    /// Note-scoped fixed value.
    Random(f32),
    /// Multi-segment envelope state.
    Mseg(MsegRuntime),
    /// Held step sequence state.
    Step(StepRuntime),
    /// Sample-and-hold state.
    SampleHold(RandomStepRuntime),
    /// Smooth-random state.
    SmoothRandom(SmoothRandomRuntime),
}

/// State of an MSEG source.
pub(crate) struct MsegRuntime {
    segment: usize,
    position: f64,
    start_value: f32,
    value: f32,
    released: bool,
}

/// State of a held step source.
pub(crate) struct StepRuntime {
    index: usize,
    position: f64,
}

/// State shared by deterministic stepped random sources.
pub(crate) struct RandomStepRuntime {
    index: u64,
    position: f64,
    value: f32,
}

/// State of an interpolated random source.
pub(crate) struct SmoothRandomRuntime {
    index: u64,
    position: f64,
    current: f32,
    next: f32,
}

impl VoiceSourceRuntime {
    /// Build source state from an immutable compiled source.
    pub(crate) fn new(source: &CompiledVoiceSource) -> Self {
        match source {
            CompiledVoiceSource::Velocity => Self::Velocity(0.0),
            CompiledVoiceSource::KeyTracking => Self::KeyTracking(-1.0),
            CompiledVoiceSource::Lfo(value) => Self::Lfo { phase: value.phase },
            CompiledVoiceSource::Envelope(value) => {
                Self::Envelope(AdsrRuntime::new(value.envelope))
            }
            CompiledVoiceSource::Random(_) => Self::Random(0.0),
            CompiledVoiceSource::Mseg(value) => Self::Mseg(MsegRuntime {
                segment: 0,
                position: 0.0,
                start_value: value.initial_value,
                value: value.initial_value,
                released: false,
            }),
            CompiledVoiceSource::Step(_) => Self::Step(StepRuntime {
                index: 0,
                position: 0.0,
            }),
            CompiledVoiceSource::SampleHold(_) => Self::SampleHold(RandomStepRuntime {
                index: 0,
                position: 0.0,
                value: 0.0,
            }),
            CompiledVoiceSource::SmoothRandom(_) => Self::SmoothRandom(SmoothRandomRuntime {
                index: 0,
                position: 0.0,
                current: 0.0,
                next: 0.0,
            }),
        }
    }

    /// Reset state before a new voice assignment.
    pub(crate) fn reset(&mut self, source: &CompiledVoiceSource) {
        *self = Self::new(source);
    }

    /// Apply a fresh note transition.
    pub(crate) fn note_on(
        &mut self,
        source: &CompiledVoiceSource,
        note: NoteId,
        note_number: u8,
        velocity: u8,
    ) {
        match (source, self) {
            (CompiledVoiceSource::Velocity, Self::Velocity(value)) => {
                *value = f32::from(velocity) / 127.0;
            }
            (CompiledVoiceSource::KeyTracking, Self::KeyTracking(value)) => {
                *value = f32::from(note_number) / 127.0 * 2.0 - 1.0;
            }
            (CompiledVoiceSource::Lfo(value), Self::Lfo { phase }) => *phase = value.phase,
            (CompiledVoiceSource::Envelope(_), Self::Envelope(envelope)) => envelope.note_on(),
            (CompiledVoiceSource::Random(value), Self::Random(random)) => {
                *random = deterministic_random(value.seed, note, value.source_hash, 0);
            }
            (CompiledVoiceSource::Mseg(value), Self::Mseg(state)) => {
                state.segment = 0;
                state.position = 0.0;
                state.start_value = value.initial_value;
                state.value = value.initial_value;
                state.released = false;
            }
            (CompiledVoiceSource::Step(_), Self::Step(state)) => {
                state.index = 0;
                state.position = 0.0;
            }
            (CompiledVoiceSource::SampleHold(value), Self::SampleHold(state)) => {
                state.index = 0;
                state.position = 0.0;
                state.value = deterministic_random(value.seed, note, value.source_hash, 0);
            }
            (CompiledVoiceSource::SmoothRandom(value), Self::SmoothRandom(state)) => {
                state.index = 0;
                state.position = 0.0;
                state.current = deterministic_random(value.seed, note, value.source_hash, 0);
                state.next = deterministic_random(value.seed, note, value.source_hash, 1);
            }
            _ => {}
        }
    }

    /// Update note-dependent controls without restarting time state.
    pub(crate) fn transition_note(
        &mut self,
        source: &CompiledVoiceSource,
        note: NoteId,
        note_number: u8,
        velocity: u8,
    ) {
        match (source, self) {
            (CompiledVoiceSource::Velocity, Self::Velocity(value)) => {
                *value = f32::from(velocity) / 127.0;
            }
            (CompiledVoiceSource::KeyTracking, Self::KeyTracking(value)) => {
                *value = f32::from(note_number) / 127.0 * 2.0 - 1.0;
            }
            (CompiledVoiceSource::Random(value), Self::Random(random)) => {
                *random = deterministic_random(value.seed, note, value.source_hash, 0);
            }
            _ => {}
        }
    }

    /// Update note-off lifecycle state.
    pub(crate) fn note_off(&mut self) {
        match self {
            Self::Envelope(envelope) => envelope.note_off(),
            Self::Mseg(state) => state.released = true,
            _ => {}
        }
    }

    /// Return the current source value.
    #[must_use]
    pub(crate) fn current_value(&self, source: &CompiledVoiceSource) -> Option<f32> {
        match (source, self) {
            (CompiledVoiceSource::Velocity, Self::Velocity(value))
            | (CompiledVoiceSource::KeyTracking, Self::KeyTracking(value))
            | (CompiledVoiceSource::Random(_), Self::Random(value)) => Some(*value),
            (CompiledVoiceSource::Lfo(value), Self::Lfo { phase }) => {
                Some(lfo_value(value.waveform, *phase))
            }
            (CompiledVoiceSource::Envelope(_), Self::Envelope(envelope)) => {
                Some(envelope.current_value())
            }
            (CompiledVoiceSource::Mseg(_), Self::Mseg(state)) => Some(state.value),
            (CompiledVoiceSource::Step(value), Self::Step(state)) => {
                value.values.get(state.index).copied()
            }
            (CompiledVoiceSource::SampleHold(_), Self::SampleHold(state)) => Some(state.value),
            (CompiledVoiceSource::SmoothRandom(_), Self::SmoothRandom(state)) => {
                Some(smooth_value(state.current, state.next, state.position))
            }
            _ => None,
        }
    }

    /// Return the next discontinuity in the source, in frames.
    #[must_use]
    pub(crate) fn frames_until_boundary(
        &self,
        source: &CompiledVoiceSource,
        sample_rate: f64,
        tempo_bpm: f64,
        remaining: usize,
    ) -> Option<usize> {
        let frames = match (source, self) {
            (CompiledVoiceSource::Lfo(value), Self::Lfo { phase })
                if value.waveform == crate::definition::LfoWaveform::Triangle =>
            {
                triangle_boundary(
                    *phase,
                    value.rate,
                    value.rate_unit,
                    sample_rate,
                    tempo_bpm,
                    remaining,
                )
            }
            (CompiledVoiceSource::Mseg(value), Self::Mseg(state)) => {
                state.segment_duration(value).map(|(duration, unit)| {
                    let seconds_per_unit = match unit {
                        ModulationDurationUnit::Seconds => 1.0,
                        ModulationDurationUnit::Beats => 60.0 / tempo_bpm,
                    };
                    ceil_positive_frames(
                        (duration - state.position).max(0.0) * seconds_per_unit * sample_rate,
                    )
                })
            }
            (CompiledVoiceSource::Step(value), Self::Step(state)) => {
                let seconds_per_step =
                    1.0 / rate_per_second(value.rate, value.rate_unit, tempo_bpm);
                Some(ceil_boundary_frames(
                    (1.0 - state.position) * seconds_per_step * sample_rate,
                ))
            }
            (CompiledVoiceSource::SampleHold(value), Self::SampleHold(state)) => {
                let seconds_per_step =
                    1.0 / rate_per_second(value.rate, value.rate_unit, tempo_bpm);
                Some(ceil_boundary_frames(
                    (1.0 - state.position) * seconds_per_step * sample_rate,
                ))
            }
            (CompiledVoiceSource::SmoothRandom(value), Self::SmoothRandom(state)) => {
                let seconds_per_step =
                    1.0 / rate_per_second(value.rate, value.rate_unit, tempo_bpm);
                Some(ceil_boundary_frames(
                    (1.0 - state.position) * seconds_per_step * sample_rate,
                ))
            }
            (CompiledVoiceSource::Envelope(_), Self::Envelope(envelope)) => {
                envelope.frames_until_segment_end()
            }
            _ => None,
        };
        frames.map(|value| value.max(1).min(remaining))
    }

    /// Advance the source and return its endpoint span.
    pub(crate) fn advance(
        &mut self,
        source: &CompiledVoiceSource,
        frames: usize,
        sample_rate: f64,
        tempo_bpm: f64,
        note: NoteId,
    ) -> Result<ValueSpan, ProcessError> {
        if frames == 0 {
            return Ok(ValueSpan {
                start: self.current_value(source).unwrap_or(0.0),
                end: self.current_value(source).unwrap_or(0.0),
            });
        }
        let start = self.current_value(source).unwrap_or(0.0);
        let end = match (source, self) {
            (CompiledVoiceSource::Velocity, Self::Velocity(value))
            | (CompiledVoiceSource::KeyTracking, Self::KeyTracking(value))
            | (CompiledVoiceSource::Random(_), Self::Random(value)) => *value,
            (CompiledVoiceSource::Lfo(value), Self::Lfo { phase }) => {
                let increment = rate_per_second(value.rate, value.rate_unit, tempo_bpm)
                    * frames_as_f64(frames)
                    / sample_rate;
                if !increment.is_finite() {
                    return Err(ProcessError::InvalidMusicalTime);
                }
                #[allow(clippy::cast_possible_truncation)]
                let next_phase = (f64::from(*phase) + increment).fract() as f32;
                *phase = next_phase;
                lfo_value(value.waveform, next_phase)
            }
            (CompiledVoiceSource::Envelope(_), Self::Envelope(envelope)) => envelope.span(frames).1,
            (CompiledVoiceSource::Mseg(value), Self::Mseg(state)) => {
                state.advance(value, frames, sample_rate, tempo_bpm)
            }
            (CompiledVoiceSource::Step(value), Self::Step(state)) => {
                let changed_at_endpoint = state.advance(value, frames, sample_rate, tempo_bpm);
                let end = value.values.get(state.index).copied().unwrap_or(0.0);
                if changed_at_endpoint { start } else { end }
            }
            (CompiledVoiceSource::SampleHold(value), Self::SampleHold(state)) => {
                let changed_at_endpoint =
                    state.advance(value, frames, sample_rate, tempo_bpm, note);
                if changed_at_endpoint {
                    start
                } else {
                    state.value
                }
            }
            (CompiledVoiceSource::SmoothRandom(value), Self::SmoothRandom(state)) => {
                state.advance(value, frames, sample_rate, tempo_bpm, note);
                smooth_value(state.current, state.next, state.position)
            }
            _ => start,
        };
        if !end.is_finite() {
            return Err(ProcessError::InvalidMusicalTime);
        }
        Ok(ValueSpan { start, end })
    }
}

impl MsegRuntime {
    fn segment_duration(&self, source: &CompiledMseg) -> Option<(f64, ModulationDurationUnit)> {
        source
            .segments
            .get(self.segment)
            .map(|segment| (f64::from(segment.duration), segment.duration_unit))
    }

    fn advance(
        &mut self,
        source: &CompiledMseg,
        frames: usize,
        sample_rate: f64,
        tempo_bpm: f64,
    ) -> f32 {
        let mut remaining_seconds = frames_as_f64(frames) / sample_rate;
        while remaining_seconds > 0.0 {
            let Some(segment) = source.segments.get(self.segment) else {
                self.value = source
                    .segments
                    .last()
                    .map_or(self.value, |item| item.target);
                return self.value;
            };
            let unit_seconds = match segment.duration_unit {
                ModulationDurationUnit::Seconds => 1.0,
                ModulationDurationUnit::Beats => 60.0 / tempo_bpm,
            };
            let duration_seconds = f64::from(segment.duration) * unit_seconds;
            let remaining_segment = (duration_seconds - self.position * unit_seconds).max(0.0);
            if remaining_segment <= f64::EPSILON {
                self.finish_segment(source);
                continue;
            }
            let consumed = remaining_seconds.min(remaining_segment);
            self.position += consumed / unit_seconds;
            let progress = (self.position / f64::from(segment.duration)).clamp(0.0, 1.0);
            self.value =
                interpolate_segment(self.start_value, segment.target, progress, segment.curve);
            remaining_seconds -= consumed;
            if remaining_segment - consumed <= f64::EPSILON {
                self.value = segment.target;
                self.finish_segment(source);
            }
        }
        self.value
    }

    fn finish_segment(&mut self, source: &CompiledMseg) {
        self.start_value = self.value;
        self.position = 0.0;
        let next = self.segment.saturating_add(1);
        if !self.released && source.loop_range.is_some_and(|(_, end)| next >= end) {
            self.segment = source.loop_range.map_or(next, |(start, _)| start);
        } else {
            self.segment = next;
        }
    }
}

impl StepRuntime {
    fn advance(
        &mut self,
        source: &CompiledStep,
        frames: usize,
        sample_rate: f64,
        tempo_bpm: f64,
    ) -> bool {
        let rate = rate_per_second(source.rate, source.rate_unit, tempo_bpm);
        let total = self.position + frames_as_f64(frames) * rate / sample_rate;
        let steps = whole_steps_as_usize(total + 1.0e-12);
        self.position = normalized_step_position(total, steps_as_f64(steps));
        if !source.values.is_empty() {
            self.index = (self.index + steps) % source.values.len();
        }
        steps > 0 && self.position <= 1.0e-9
    }
}

impl RandomStepRuntime {
    fn advance(
        &mut self,
        source: &CompiledSampleHold,
        frames: usize,
        sample_rate: f64,
        tempo_bpm: f64,
        note: NoteId,
    ) -> bool {
        let rate = rate_per_second(source.rate, source.rate_unit, tempo_bpm);
        let total = self.position + frames_as_f64(frames) * rate / sample_rate;
        let steps = whole_steps_as_u64(total + 1.0e-12);
        self.position = normalized_step_position(total, steps_as_f64_u64(steps));
        self.index = self.index.saturating_add(steps);
        if steps > 0 {
            self.value = deterministic_random(source.seed, note, source.source_hash, self.index);
        }
        steps > 0 && self.position <= 1.0e-9
    }
}

impl SmoothRandomRuntime {
    fn advance(
        &mut self,
        source: &CompiledSmoothRandom,
        frames: usize,
        sample_rate: f64,
        tempo_bpm: f64,
        note: NoteId,
    ) {
        let rate = rate_per_second(source.rate, source.rate_unit, tempo_bpm);
        let total = self.position + frames_as_f64(frames) * rate / sample_rate;
        let steps = whole_steps_as_u64(total + 1.0e-12);
        self.position = normalized_step_position(total, steps_as_f64_u64(steps));
        for _ in 0..steps {
            self.index = self.index.saturating_add(1);
            self.current = self.next;
            self.next = deterministic_random(source.seed, note, source.source_hash, self.index + 1);
        }
    }
}

fn rate_per_second(rate: f32, unit: ModulationRateUnit, tempo_bpm: f64) -> f64 {
    match unit {
        ModulationRateUnit::PerSecond => f64::from(rate),
        ModulationRateUnit::PerBeat => f64::from(rate) * tempo_bpm / 60.0,
    }
}

pub(super) fn triangle_boundary(
    phase: f32,
    rate: f32,
    unit: ModulationRateUnit,
    sample_rate: f64,
    tempo_bpm: f64,
    remaining: usize,
) -> Option<usize> {
    let increment = rate_per_second(rate, unit, tempo_bpm) / sample_rate;
    if increment <= 0.0 || !increment.is_finite() {
        return None;
    }
    let phase = f64::from(phase.fract());
    let next = if phase < 0.5 { 0.5 } else { 1.0 };
    Some(
        ceil_positive_frames((next - phase) / increment)
            .max(1)
            .min(remaining),
    )
}

#[allow(clippy::cast_precision_loss)]
fn frames_as_f64(frames: usize) -> f64 {
    frames as f64
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn ceil_positive_frames(value: f64) -> usize {
    if !value.is_finite() {
        return usize::MAX;
    }
    value.max(0.0).ceil() as usize
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn ceil_boundary_frames(value: f64) -> usize {
    if !value.is_finite() {
        return usize::MAX;
    }
    let nearest_integer = value.round();
    let value = if (value - nearest_integer).abs() <= 1.0e-9 {
        nearest_integer
    } else {
        value
    };
    value.max(0.0).ceil() as usize
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn whole_steps_as_usize(value: f64) -> usize {
    value.max(0.0).floor() as usize
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn whole_steps_as_u64(value: f64) -> u64 {
    value.max(0.0).floor() as u64
}

#[allow(clippy::cast_precision_loss)]
fn steps_as_f64(steps: usize) -> f64 {
    steps as f64
}

#[allow(clippy::cast_precision_loss)]
fn steps_as_f64_u64(steps: u64) -> f64 {
    steps as f64
}

fn normalized_step_position(total: f64, steps: f64) -> f64 {
    (total - steps).max(0.0).clamp(0.0, 1.0 - f64::EPSILON)
}

pub(super) fn lfo_value(waveform: crate::definition::LfoWaveform, phase: f32) -> f32 {
    match waveform {
        crate::definition::LfoWaveform::Sine => (std::f32::consts::TAU * phase).sin(),
        crate::definition::LfoWaveform::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
    }
}

fn interpolate_segment(
    start: f32,
    target: f32,
    progress: f64,
    curve: ModulationSegmentCurve,
) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let progress = progress as f32;
    let progress = match curve {
        ModulationSegmentCurve::Linear => progress,
        ModulationSegmentCurve::SmoothStep => progress * progress * (3.0 - 2.0 * progress),
    };
    start + (target - start) * progress
}

fn smooth_value(current: f32, next: f32, position: f64) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let position = position as f32;
    let position = position * position * (3.0 - 2.0 * position);
    current + (next - current) * position
}

pub(super) fn deterministic_random(seed: u64, note_id: NoteId, source_hash: u64, step: u64) -> f32 {
    bipolar_f32(splitmix64_finalizer(
        seed ^ note_id ^ source_hash ^ step.wrapping_mul(0x9E37_79B9_7F4A_7C15),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{
        CompiledLfo, CompiledMsegSegment, CompiledSampleHold, CompiledSmoothRandom, CompiledStep,
    };
    use crate::definition::{LfoWaveform, ModulationRateUnit};

    #[test]
    fn step_source_holds_values_until_its_boundary() {
        let source = CompiledVoiceSource::Step(CompiledStep {
            values: vec![-1.0, 1.0].into_boxed_slice(),
            rate: 1.0,
            rate_unit: ModulationRateUnit::PerSecond,
        });
        let mut runtime = VoiceSourceRuntime::new(&source);
        runtime.note_on(&source, 1, 60, 100);

        assert_eq!(runtime.current_value(&source), Some(-1.0));
        assert_eq!(
            runtime.frames_until_boundary(&source, 10.0, 120.0, 10),
            Some(10)
        );
        let first_span = runtime
            .advance(&source, 9, 10.0, 120.0, 1)
            .expect("first step span");
        assert!((first_span.start + 1.0).abs() < f32::EPSILON);
        assert!((first_span.end + 1.0).abs() < f32::EPSILON);
        let second_span = runtime
            .advance(&source, 1, 10.0, 120.0, 1)
            .expect("second step span");
        assert!((second_span.start + 1.0).abs() < f32::EPSILON);
        assert!((second_span.end + 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn stepped_boundary_rounding_does_not_accumulate_across_chunks() {
        let source = CompiledVoiceSource::Step(CompiledStep {
            values: vec![-1.0, 1.0].into_boxed_slice(),
            rate: 8.0,
            rate_unit: ModulationRateUnit::PerSecond,
        });
        let mut runtime = VoiceSourceRuntime::new(&source);
        runtime.note_on(&source, 1, 60, 100);

        for _ in 0..562 {
            runtime
                .advance(&source, 32, 48_000.0, 120.0, 1)
                .expect("step source advance");
        }

        assert_eq!(
            runtime.frames_until_boundary(&source, 48_000.0, 120.0, 32),
            Some(16)
        );
    }

    #[test]
    fn per_beat_lfo_uses_the_current_tempo() {
        let source = CompiledVoiceSource::Lfo(CompiledLfo {
            waveform: LfoWaveform::Sine,
            rate: 1.0,
            rate_unit: ModulationRateUnit::PerBeat,
            phase: 0.0,
        });
        let mut runtime = VoiceSourceRuntime::new(&source);
        runtime.note_on(&source, 1, 60, 100);

        runtime
            .advance(&source, 12_000, 48_000.0, 120.0, 1)
            .expect("first beat half");
        let half_beat = runtime.current_value(&source).expect("LFO value");
        runtime
            .advance(&source, 12_000, 48_000.0, 120.0, 1)
            .expect("first beat");
        let full_beat = runtime.current_value(&source).expect("LFO value");

        assert!(half_beat.abs() < 1e-6);
        assert!(full_beat.abs() < 1e-6);
    }

    #[test]
    fn mseg_loops_until_note_off_then_finishes() {
        let source = CompiledVoiceSource::Mseg(CompiledMseg {
            initial_value: 0.0,
            segments: vec![
                CompiledMsegSegment {
                    duration: 1.0,
                    duration_unit: ModulationDurationUnit::Seconds,
                    target: 1.0,
                    curve: ModulationSegmentCurve::Linear,
                },
                CompiledMsegSegment {
                    duration: 1.0,
                    duration_unit: ModulationDurationUnit::Seconds,
                    target: -1.0,
                    curve: ModulationSegmentCurve::Linear,
                },
            ]
            .into_boxed_slice(),
            loop_range: Some((0, 2)),
        });
        let mut runtime = VoiceSourceRuntime::new(&source);
        runtime.note_on(&source, 1, 60, 100);

        runtime
            .advance(&source, 10, 10.0, 120.0, 1)
            .expect("first segment");
        assert_eq!(runtime.current_value(&source), Some(1.0));
        runtime
            .advance(&source, 10, 10.0, 120.0, 1)
            .expect("second segment");
        assert_eq!(runtime.current_value(&source), Some(-1.0));

        runtime.note_off();
        runtime
            .advance(&source, 20, 10.0, 120.0, 1)
            .expect("release path");
        assert_eq!(runtime.current_value(&source), Some(-1.0));
    }

    #[test]
    fn sample_hold_changes_at_the_boundary_without_a_ramp() {
        let source = CompiledVoiceSource::SampleHold(CompiledSampleHold {
            seed: 7,
            source_hash: 11,
            rate: 2.0,
            rate_unit: ModulationRateUnit::PerSecond,
        });
        let mut runtime = VoiceSourceRuntime::new(&source);
        runtime.note_on(&source, 42, 60, 100);
        let initial = runtime.current_value(&source).expect("initial value");

        let span = runtime
            .advance(&source, 500, 1_000.0, 120.0, 42)
            .expect("sample-and-hold advance");
        let next = deterministic_random(7, 42, 11, 1);

        assert!((span.start - initial).abs() <= f32::EPSILON);
        assert!((span.end - initial).abs() <= f32::EPSILON);
        assert_eq!(runtime.current_value(&source), Some(next));
    }

    #[test]
    fn random_sources_are_deterministic_per_note() {
        let sample_hold = CompiledVoiceSource::SampleHold(CompiledSampleHold {
            seed: 7,
            source_hash: 11,
            rate: 2.0,
            rate_unit: ModulationRateUnit::PerSecond,
        });
        let smooth_random = CompiledVoiceSource::SmoothRandom(CompiledSmoothRandom {
            seed: 13,
            source_hash: 17,
            rate: 2.0,
            rate_unit: ModulationRateUnit::PerSecond,
        });
        let mut sample_a = VoiceSourceRuntime::new(&sample_hold);
        let mut sample_b = VoiceSourceRuntime::new(&sample_hold);
        let mut smooth_a = VoiceSourceRuntime::new(&smooth_random);
        let mut smooth_b = VoiceSourceRuntime::new(&smooth_random);
        sample_a.note_on(&sample_hold, 42, 60, 100);
        sample_b.note_on(&sample_hold, 42, 60, 100);
        smooth_a.note_on(&smooth_random, 42, 60, 100);
        smooth_b.note_on(&smooth_random, 42, 60, 100);

        sample_a
            .advance(&sample_hold, 1_000, 1_000.0, 120.0, 42)
            .expect("sample-and-hold advance");
        sample_b
            .advance(&sample_hold, 1_000, 1_000.0, 120.0, 42)
            .expect("sample-and-hold advance");
        smooth_a
            .advance(&smooth_random, 1_000, 1_000.0, 120.0, 42)
            .expect("smooth-random advance");
        smooth_b
            .advance(&smooth_random, 1_000, 1_000.0, 120.0, 42)
            .expect("smooth-random advance");

        assert_eq!(
            sample_a.current_value(&sample_hold),
            sample_b.current_value(&sample_hold)
        );
        assert_eq!(
            smooth_a.current_value(&smooth_random),
            smooth_b.current_value(&smooth_random)
        );
        assert!(
            sample_a
                .current_value(&sample_hold)
                .expect("sample value")
                .is_finite()
        );
        assert!(
            smooth_a
                .current_value(&smooth_random)
                .expect("smooth value")
                .is_finite()
        );
    }
}
