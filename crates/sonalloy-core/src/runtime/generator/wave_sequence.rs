use std::sync::Arc;

use crate::asset::{PreparedAudio, PreparedAudioChannels};
use crate::compiler::{
    CompiledSampleDirection, CompiledWaveSequence, CompiledWaveSequenceDuration,
    CompiledWaveSequenceStep, CompiledWaveSequenceStepPlayback,
};
use crate::definition::WaveSequenceDirection;
use crate::process::ProcessError;

use super::super::interpolation::cubic_interpolate;
use super::ensure_finite;
use super::playback_ratio;

const INACTIVE_STEP: usize = usize::MAX;

#[derive(Debug, Clone, Copy)]
struct SequenceCursor {
    index: usize,
    ping_pong_forward: bool,
}

#[derive(Debug, Clone, Copy)]
struct PlaybackSlot {
    active: bool,
    cursor: SequenceCursor,
    progress: f64,
    source_position: f64,
}

impl PlaybackSlot {
    const fn inactive() -> Self {
        Self {
            active: false,
            cursor: SequenceCursor {
                index: INACTIVE_STEP,
                ping_pong_forward: true,
            },
            progress: 0.0,
            source_position: 0.0,
        }
    }
}

/// Runtime state for one prepared Wave Sequence Generator.
pub(crate) struct WaveSequenceRuntime {
    steps: Arc<[CompiledWaveSequenceStep]>,
    root_note: u8,
    direction: WaveSequenceDirection,
    loop_sequence: bool,
    crossfade: f32,
    current: PlaybackSlot,
    next: PlaybackSlot,
    next_cursor: Option<SequenceCursor>,
    crossfade_start: f64,
    crossfade_progress: f64,
    started: bool,
    finished: bool,
}

impl WaveSequenceRuntime {
    pub(super) fn new(compiled: &CompiledWaveSequence) -> Result<Self, ProcessError> {
        if !(1..=128).contains(&compiled.steps.len())
            || !compiled.crossfade.is_finite()
            || !(0.0..=0.5).contains(&compiled.crossfade)
        {
            return Err(super::invalid_state());
        }
        Ok(Self {
            steps: Arc::clone(&compiled.steps),
            root_note: compiled.root_note,
            direction: compiled.direction,
            loop_sequence: compiled.loop_sequence,
            crossfade: compiled.crossfade,
            current: PlaybackSlot::inactive(),
            next: PlaybackSlot::inactive(),
            next_cursor: None,
            crossfade_start: 0.0,
            crossfade_progress: 0.0,
            started: false,
            finished: false,
        })
    }

    pub(super) fn start(&mut self, _note_id: u64) -> Result<(), ProcessError> {
        let cursor = self.initial_cursor();
        self.current = self.slot_for(cursor)?;
        self.next = PlaybackSlot::inactive();
        self.next_cursor = None;
        self.crossfade_start = 0.0;
        self.crossfade_progress = 0.0;
        self.started = true;
        self.finished = false;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn render(
        &mut self,
        frames: usize,
        note_number: u8,
        tuning_start: f32,
        tuning_end: f32,
        sample_rate: f64,
        tempo_bpm: f64,
        mono: &mut [f32],
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<bool, ProcessError> {
        if !self.started
            || frames > mono.len()
            || frames > left.len()
            || frames > right.len()
            || !sample_rate.is_finite()
            || sample_rate <= 0.0
            || !tempo_bpm.is_finite()
            || tempo_bpm <= 0.0
            || !tuning_start.is_finite()
            || !tuning_end.is_finite()
        {
            return Err(super::invalid_state());
        }
        left[..frames].fill(0.0);
        right[..frames].fill(0.0);
        mono[..frames].fill(0.0);
        for frame in 0..frames {
            if self.finished {
                continue;
            }
            self.transition_finished_steps(tempo_bpm)?;
            if self.finished {
                continue;
            }
            self.prepare_crossfade(tempo_bpm)?;
            let tuning = interpolate_tuning(tuning_start, tuning_end, frame, frames);
            let (current_left, current_right) = Self::render_slot(
                &self.steps,
                self.root_note,
                &mut self.current,
                note_number,
                tuning,
            )?;
            let (frame_left, frame_right) = if self.next.active {
                let (next_left, next_right) = Self::render_slot(
                    &self.steps,
                    self.root_note,
                    &mut self.next,
                    note_number,
                    tuning,
                )?;
                let fade = if self.crossfade_progress <= 0.0 {
                    0.0
                } else {
                    ((self.current.progress - self.crossfade_start) / self.crossfade_progress)
                        .clamp(0.0, 1.0)
                };
                let angle = fade * std::f64::consts::FRAC_PI_2;
                #[allow(clippy::cast_possible_truncation)]
                let current_gain = angle.cos() as f32;
                #[allow(clippy::cast_possible_truncation)]
                let next_gain = angle.sin() as f32;
                (
                    current_left * current_gain + next_left * next_gain,
                    current_right * current_gain + next_right * next_gain,
                )
            } else {
                (current_left, current_right)
            };
            left[frame] = frame_left;
            right[frame] = frame_right;
            mono[frame] = f32::midpoint(frame_left, frame_right);
            Self::advance_slot(&self.steps, &mut self.current, tempo_bpm, sample_rate);
            if self.next.active {
                Self::advance_slot(&self.steps, &mut self.next, tempo_bpm, sample_rate);
            }
            self.transition_finished_steps(tempo_bpm)?;
        }
        ensure_finite(&left[..frames])?;
        ensure_finite(&right[..frames])?;
        ensure_finite(&mono[..frames])?;
        Ok(self.finished)
    }

    fn initial_cursor(&self) -> SequenceCursor {
        let index = match self.direction {
            WaveSequenceDirection::Forward | WaveSequenceDirection::PingPong => 0,
            WaveSequenceDirection::Reverse => self.steps.len().saturating_sub(1),
        };
        SequenceCursor {
            index,
            ping_pong_forward: true,
        }
    }

    fn next_cursor(&self, cursor: SequenceCursor) -> Option<SequenceCursor> {
        let last = self.steps.len().saturating_sub(1);
        match self.direction {
            WaveSequenceDirection::Forward => {
                if cursor.index < last {
                    Some(SequenceCursor {
                        index: cursor.index + 1,
                        ping_pong_forward: true,
                    })
                } else if self.loop_sequence {
                    Some(SequenceCursor {
                        index: 0,
                        ping_pong_forward: true,
                    })
                } else {
                    None
                }
            }
            WaveSequenceDirection::Reverse => {
                if cursor.index > 0 {
                    Some(SequenceCursor {
                        index: cursor.index - 1,
                        ping_pong_forward: true,
                    })
                } else if self.loop_sequence {
                    Some(SequenceCursor {
                        index: last,
                        ping_pong_forward: true,
                    })
                } else {
                    None
                }
            }
            WaveSequenceDirection::PingPong => {
                if self.steps.len() == 1 {
                    return self.loop_sequence.then_some(cursor);
                }
                if cursor.ping_pong_forward {
                    if cursor.index < last {
                        Some(SequenceCursor {
                            index: cursor.index + 1,
                            ping_pong_forward: true,
                        })
                    } else {
                        Some(SequenceCursor {
                            index: last - 1,
                            ping_pong_forward: false,
                        })
                    }
                } else if cursor.index > 0 {
                    Some(SequenceCursor {
                        index: cursor.index - 1,
                        ping_pong_forward: false,
                    })
                } else if self.loop_sequence {
                    Some(SequenceCursor {
                        index: 1,
                        ping_pong_forward: true,
                    })
                } else {
                    None
                }
            }
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn slot_for(&self, cursor: SequenceCursor) -> Result<PlaybackSlot, ProcessError> {
        let step = self
            .steps
            .get(cursor.index)
            .ok_or_else(super::invalid_state)?;
        let source_position = match step.playback_direction {
            CompiledSampleDirection::Forward => step.start_frame as f64,
            CompiledSampleDirection::Reverse => step.end_frame.saturating_sub(1) as f64,
        };
        Ok(PlaybackSlot {
            active: true,
            cursor,
            progress: 0.0,
            source_position,
        })
    }

    fn prepare_crossfade(&mut self, tempo_bpm: f64) -> Result<(), ProcessError> {
        if self.next.active || self.crossfade <= 0.0 {
            return Ok(());
        }
        let Some(next_cursor) = self.next_cursor(self.current.cursor) else {
            return Ok(());
        };
        let current_step = self
            .steps
            .get(self.current.cursor.index)
            .ok_or_else(super::invalid_state)?;
        let next_step = self
            .steps
            .get(next_cursor.index)
            .ok_or_else(super::invalid_state)?;
        let current_seconds = duration_seconds(current_step.duration, tempo_bpm)?;
        let next_seconds = duration_seconds(next_step.duration, tempo_bpm)?;
        let overlap_seconds = f64::from(self.crossfade) * current_seconds.min(next_seconds);
        let overlap_progress = match current_step.duration {
            CompiledWaveSequenceDuration::Seconds(_) => overlap_seconds,
            CompiledWaveSequenceDuration::Beats(_) => overlap_seconds * tempo_bpm / 60.0,
        };
        if !overlap_progress.is_finite() || overlap_progress <= 0.0 {
            return Err(super::invalid_state());
        }
        let duration = duration_value(current_step.duration);
        if self.current.progress + f64::EPSILON < duration - overlap_progress {
            return Ok(());
        }
        self.next = self.slot_for(next_cursor)?;
        self.next_cursor = Some(next_cursor);
        self.crossfade_progress = overlap_progress;
        self.crossfade_start = (duration - overlap_progress).max(0.0);
        Ok(())
    }

    fn transition_finished_steps(&mut self, tempo_bpm: f64) -> Result<(), ProcessError> {
        loop {
            let current_step = self
                .steps
                .get(self.current.cursor.index)
                .ok_or_else(super::invalid_state)?;
            if self.current.progress + f64::EPSILON < duration_value(current_step.duration) {
                return Ok(());
            }
            if self.next.active {
                self.next_cursor.take().ok_or_else(super::invalid_state)?;
                self.current = self.next;
                self.next = PlaybackSlot::inactive();
                self.crossfade_start = 0.0;
                self.crossfade_progress = 0.0;
                continue;
            }
            let Some(next_cursor) = self.next_cursor(self.current.cursor) else {
                self.current.active = false;
                self.finished = true;
                return Ok(());
            };
            self.current = self.slot_for(next_cursor)?;
            self.next_cursor = None;
            self.crossfade_start = 0.0;
            self.crossfade_progress = 0.0;
            self.prepare_crossfade(tempo_bpm)?;
        }
    }

    fn advance_slot(
        steps: &Arc<[CompiledWaveSequenceStep]>,
        slot: &mut PlaybackSlot,
        tempo_bpm: f64,
        sample_rate: f64,
    ) {
        let Some(step) = steps.get(slot.cursor.index) else {
            return;
        };
        slot.progress += progress_increment(step.duration, tempo_bpm, sample_rate);
    }

    fn render_slot(
        steps: &Arc<[CompiledWaveSequenceStep]>,
        root_note: u8,
        slot: &mut PlaybackSlot,
        note_number: u8,
        tuning_cents: f32,
    ) -> Result<(f32, f32), ProcessError> {
        let step = steps
            .get(slot.cursor.index)
            .ok_or_else(super::invalid_state)?;
        if slot.progress >= duration_value(step.duration) {
            return Ok((0.0, 0.0));
        }
        let Some(source) = step.source.as_ref() else {
            return Ok((0.0, 0.0));
        };
        if step.start_frame >= step.end_frame || step.end_frame > source.frames {
            return Err(super::invalid_state());
        }
        let ratio = playback_ratio(
            note_number,
            root_note,
            crate::compiler::cents_to_ratio(tuning_cents + step.pitch_cents),
        );
        if !ratio.is_finite() || ratio <= 0.0 {
            return Err(ProcessError::InvalidFrequency);
        }
        let length = step.end_frame - step.start_frame;
        let mut position = slot.source_position;
        if step.playback == CompiledWaveSequenceStepPlayback::Loop {
            position = normalize_position(position, step.start_frame, length);
        } else if !is_inside_region(position, step.start_frame, step.end_frame) {
            return Ok((0.0, 0.0));
        }
        let looping = step.playback == CompiledWaveSequenceStepPlayback::Loop;
        let (left, right) = read_frame(source, position, step.start_frame, step.end_frame, looping);
        let direction = match step.playback_direction {
            CompiledSampleDirection::Forward => 1.0,
            CompiledSampleDirection::Reverse => -1.0,
        };
        slot.source_position = position + direction * ratio;
        if step.playback == CompiledWaveSequenceStepPlayback::Loop {
            slot.source_position =
                normalize_position(slot.source_position, step.start_frame, length);
        }
        Ok((left * step.gain, right * step.gain))
    }

    pub(super) fn reset(&mut self) {
        self.current = PlaybackSlot::inactive();
        self.next = PlaybackSlot::inactive();
        self.next_cursor = None;
        self.crossfade_start = 0.0;
        self.crossfade_progress = 0.0;
        self.started = false;
        self.finished = false;
    }
}

fn duration_value(duration: CompiledWaveSequenceDuration) -> f64 {
    match duration {
        CompiledWaveSequenceDuration::Seconds(value)
        | CompiledWaveSequenceDuration::Beats(value) => value,
    }
}

fn duration_seconds(
    duration: CompiledWaveSequenceDuration,
    tempo_bpm: f64,
) -> Result<f64, ProcessError> {
    let seconds = match duration {
        CompiledWaveSequenceDuration::Seconds(value) => value,
        CompiledWaveSequenceDuration::Beats(value) => value * 60.0 / tempo_bpm,
    };
    if seconds.is_finite() && seconds > 0.0 {
        Ok(seconds)
    } else {
        Err(super::invalid_state())
    }
}

fn progress_increment(
    duration: CompiledWaveSequenceDuration,
    tempo_bpm: f64,
    sample_rate: f64,
) -> f64 {
    match duration {
        CompiledWaveSequenceDuration::Seconds(_) => 1.0 / sample_rate,
        CompiledWaveSequenceDuration::Beats(_) => tempo_bpm / 60.0 / sample_rate,
    }
}

fn interpolate_tuning(start: f32, end: f32, frame: usize, frames: usize) -> f32 {
    if frames == 0 {
        return start;
    }
    #[allow(clippy::cast_precision_loss)]
    {
        start + (end - start) * frame as f32 / frames as f32
    }
}

#[allow(clippy::cast_precision_loss)]
fn normalize_position(position: f64, start_frame: usize, length: usize) -> f64 {
    start_frame as f64 + (position - start_frame as f64).rem_euclid(length as f64)
}

#[allow(clippy::cast_precision_loss)]
fn is_inside_region(position: f64, start_frame: usize, end_frame: usize) -> bool {
    position >= start_frame as f64 && position < end_frame as f64
}

fn read_frame(
    source: &PreparedAudio,
    position: f64,
    start_frame: usize,
    end_frame: usize,
    looping: bool,
) -> (f32, f32) {
    match &source.channels {
        PreparedAudioChannels::Mono { samples } => {
            let value = cubic_sample(samples, position, start_frame, end_frame, looping);
            (value, value)
        }
        PreparedAudioChannels::Stereo { left, right } => (
            cubic_sample(left, position, start_frame, end_frame, looping),
            cubic_sample(right, position, start_frame, end_frame, looping),
        ),
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn cubic_sample(
    source: &[f32],
    position: f64,
    start_frame: usize,
    end_frame: usize,
    looping: bool,
) -> f32 {
    if source.is_empty()
        || !position.is_finite()
        || start_frame >= end_frame
        || position < start_frame as f64
    {
        return 0.0;
    }
    let base = position.floor() as isize;
    let fraction = position.fract() as f32;
    let p0 = sample_at(source, base - 1, start_frame, end_frame, looping);
    let p1 = sample_at(source, base, start_frame, end_frame, looping);
    let p2 = sample_at(source, base + 1, start_frame, end_frame, looping);
    let p3 = sample_at(source, base + 2, start_frame, end_frame, looping);
    cubic_interpolate(p0, p1, p2, p3, fraction)
}

fn sample_at(
    source: &[f32],
    index: isize,
    start_frame: usize,
    end_frame: usize,
    looping: bool,
) -> f32 {
    let start = isize::try_from(start_frame).unwrap_or(isize::MAX);
    let end = isize::try_from(end_frame.saturating_sub(1)).unwrap_or(isize::MAX);
    let source_index = if looping {
        let length = end_frame.saturating_sub(start_frame);
        let Ok(length) = isize::try_from(length) else {
            return 0.0;
        };
        let relative = index.saturating_sub(start).rem_euclid(length);
        start.saturating_add(relative)
    } else {
        index.clamp(start, end)
    };
    usize::try_from(source_index)
        .ok()
        .and_then(|source_index| source.get(source_index))
        .copied()
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::SampleMetadata;
    use crate::compiler::{CompiledWaveSequenceDuration, CompiledWaveSequenceStepPlayback};

    fn sequence(direction: WaveSequenceDirection, loop_sequence: bool) -> CompiledWaveSequence {
        let source = Arc::new(PreparedAudio {
            sample_rate: 48_000.0,
            frames: 4,
            source_metadata: SampleMetadata {
                source_sample_rate: 48_000,
                source_channels: 1,
                bits_per_sample: Some(16),
                source_frames: 4,
            },
            channels: PreparedAudioChannels::Mono {
                samples: Arc::from([0.0, 0.25, 0.5, 0.75]),
            },
        });
        let steps = (0..3)
            .map(|index| CompiledWaveSequenceStep {
                id: format!("step_{index}"),
                source: Some(Arc::clone(&source)),
                asset_path: "fixture.wav".to_owned(),
                start_frame: 0,
                end_frame: 4,
                duration: CompiledWaveSequenceDuration::Seconds(0.01),
                playback: CompiledWaveSequenceStepPlayback::OneShot,
                playback_direction: CompiledSampleDirection::Forward,
                gain: 1.0,
                pitch_cents: 0.0,
            })
            .collect::<Vec<_>>();
        CompiledWaveSequence {
            root_note: 60,
            direction,
            loop_sequence,
            crossfade: 0.0,
            steps: Arc::from(steps.into_boxed_slice()),
        }
    }

    #[test]
    fn ping_pong_does_not_repeat_endpoints() {
        let runtime = WaveSequenceRuntime::new(&sequence(WaveSequenceDirection::PingPong, true))
            .expect("sequence runtime");
        let first = runtime.initial_cursor();
        let second = runtime.next_cursor(first).expect("second cursor");
        let third = runtime.next_cursor(second).expect("third cursor");
        let fourth = runtime.next_cursor(third).expect("fourth cursor");
        assert_eq!(
            [first.index, second.index, third.index, fourth.index],
            [0, 1, 2, 1]
        );
    }

    #[test]
    fn sequence_duration_uses_current_tempo_for_beats() {
        let seconds =
            duration_seconds(CompiledWaveSequenceDuration::Beats(2.0), 120.0).expect("duration");
        assert!((seconds - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn wave_sequence_process_path_does_not_allocate() {
        let mut definition = sequence(WaveSequenceDirection::Forward, true);
        definition.crossfade = 0.25;
        let mut runtime = WaveSequenceRuntime::new(&definition).expect("sequence runtime");
        runtime.start(1).expect("sequence starts");
        let mut mono = [0.0; 128];
        let mut left = [0.0; 128];
        let mut right = [0.0; 128];
        let allocations = crate::test_allocator::count_allocations(|| {
            for _ in 0..32 {
                runtime
                    .render(
                        128, 60, 0.0, 0.0, 48_000.0, 120.0, &mut mono, &mut left, &mut right,
                    )
                    .expect("sequence render");
            }
        });
        assert_eq!(allocations, 0);
    }
}
