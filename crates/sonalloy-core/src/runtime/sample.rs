use std::sync::Arc;

use crate::asset::{PreparedAudio, PreparedAudioChannels};
use crate::compiler::{
    CompiledSample, CompiledSampleDirection, CompiledSampleLoop, CompiledSampleTime,
    CompiledSampleZone,
};
use crate::process::ProcessError;
use sonalloy_dsp_sys::DspStretch;

#[cfg(test)]
use crate::compiler::{CompiledSamplePlayback, CompiledStretchLatency};

use super::smoothing::rounded_frame_count;

const END_FADE_SECONDS: f64 = 0.005;

/// Sample playback state owned by a voice layer.
pub(crate) struct SampleRuntime {
    source: Option<Arc<PreparedAudio>>,
    root_note: u8,
    position: f64,
    direction: CompiledSampleDirection,
    start_frame: usize,
    end_frame: usize,
    loop_region: Option<CompiledSampleLoop>,
    time: CompiledSampleTime,
    stretcher: Option<DspStretch>,
    stretch_input_left: Vec<f32>,
    stretch_input_right: Vec<f32>,
    stretch_input_fraction: f64,
    stretch_output_frames: u64,
    stretch_latency_frames: usize,
    stretch_source_exhausted: bool,
    end_fade_frames: usize,
    finished: bool,
}

impl SampleRuntime {
    pub(crate) fn new() -> Self {
        Self {
            source: None,
            root_note: 60,
            position: 0.0,
            direction: CompiledSampleDirection::Forward,
            start_frame: 0,
            end_frame: 0,
            loop_region: None,
            time: CompiledSampleTime::Resample,
            stretcher: None,
            stretch_input_left: Vec::new(),
            stretch_input_right: Vec::new(),
            stretch_input_fraction: 0.0,
            stretch_output_frames: 0,
            stretch_latency_frames: 0,
            stretch_source_exhausted: false,
            end_fade_frames: 1,
            finished: false,
        }
    }

    pub(crate) fn prepared(
        compiled: &CompiledSample,
        spec: crate::process::ProcessSpec,
    ) -> Result<Self, ProcessError> {
        let stretch_latency_frames = compiled
            .stretch_latency
            .map_or(0, |latency| latency.output_frames);
        let max_input_frames = if compiled.stretch_latency.is_some() {
            spec.max_block_size
                .checked_mul(2)
                .ok_or(ProcessError::InvalidMaxBlockSize)?
        } else {
            0
        };
        let stretcher = if max_input_frames > 0 {
            let mut backend = DspStretch::new().map_err(ProcessError::from_stretch_error)?;
            backend
                .prepare(2, spec.sample_rate, max_input_frames, spec.max_block_size)
                .map_err(ProcessError::from_stretch_error)?;
            Some(backend)
        } else {
            None
        };
        let mut runtime = Self::new();
        runtime.stretcher = stretcher;
        runtime.stretch_input_left = vec![0.0; max_input_frames];
        runtime.stretch_input_right = vec![0.0; max_input_frames];
        runtime.stretch_latency_frames = stretch_latency_frames;
        Ok(runtime)
    }

    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn start(&mut self, zone: Option<&CompiledSampleZone>) -> Result<(), ProcessError> {
        let Some(zone) = zone else {
            self.reset()?;
            self.finished = true;
            return Ok(());
        };
        let Some(source) = zone.source.as_ref() else {
            self.reset()?;
            self.finished = true;
            return Ok(());
        };
        self.source = Some(Arc::clone(source));
        self.root_note = zone.root_note;
        self.direction = zone.playback.direction;
        self.start_frame = zone.playback.start_frame;
        self.end_frame = zone.playback.end_frame;
        self.loop_region = zone.playback.loop_region;
        self.time = zone.playback.time;
        self.stretch_input_fraction = 0.0;
        self.stretch_output_frames = 0;
        self.stretch_source_exhausted = false;
        if let Some(stretcher) = self.stretcher.as_mut() {
            stretcher
                .reset()
                .map_err(ProcessError::from_stretch_error)?;
        }
        self.end_fade_frames = rounded_frame_count(source.sample_rate * END_FADE_SECONDS).max(1);
        self.position = match self.direction {
            CompiledSampleDirection::Forward => self.start_frame as f64,
            CompiledSampleDirection::Reverse => self.end_frame.saturating_sub(1) as f64,
        };
        self.finished = self.start_frame >= self.end_frame || source.frames == 0;
        Ok(())
    }

    pub(crate) fn reset(&mut self) -> Result<(), ProcessError> {
        if let Some(stretcher) = self.stretcher.as_mut() {
            stretcher
                .reset()
                .map_err(ProcessError::from_stretch_error)?;
        }
        self.source = None;
        self.root_note = 60;
        self.position = 0.0;
        self.direction = CompiledSampleDirection::Forward;
        self.start_frame = 0;
        self.end_frame = 0;
        self.loop_region = None;
        self.time = CompiledSampleTime::Resample;
        self.stretch_input_fraction = 0.0;
        self.stretch_output_frames = 0;
        self.stretch_source_exhausted = false;
        self.end_fade_frames = 1;
        self.finished = false;
        Ok(())
    }

    pub(crate) fn uses_stretch(&self) -> bool {
        self.time.uses_stretch()
    }

    pub(crate) fn intrinsic_latency_frames(&self) -> usize {
        if self.uses_stretch() {
            self.stretch_latency_frames
        } else {
            0
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    pub(crate) fn render_stretched(
        &mut self,
        frames: usize,
        note_number: u8,
        tuning_start: f32,
        tempo_bpm: f64,
        mono: &mut [f32],
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<bool, ProcessError> {
        let Some(source) = self.source.as_ref().map(Arc::clone) else {
            self.finished = true;
            return Ok(true);
        };
        if self.stretcher.is_none() {
            return Err(ProcessError::ProcessorFailure {
                kind: crate::process::ProcessorFailureKind::InvalidState,
            });
        }
        let duration_ratio = match self.time {
            CompiledSampleTime::FixedStretch { duration_ratio } => duration_ratio,
            CompiledSampleTime::TempoSync { source_bpm } => source_bpm / tempo_bpm,
            CompiledSampleTime::Resample => {
                return Err(ProcessError::ProcessorFailure {
                    kind: crate::process::ProcessorFailureKind::InvalidState,
                });
            }
        };
        if !duration_ratio.is_finite() || !(0.5..=2.0).contains(&duration_ratio) {
            return Err(ProcessError::StretchRatioOutOfRange {
                ratio: duration_ratio,
            });
        }
        let semitones =
            f64::from(note_number) - f64::from(self.root_note) + f64::from(tuning_start) / 100.0;
        self.stretcher
            .as_mut()
            .ok_or(ProcessError::ProcessorFailure {
                kind: crate::process::ProcessorFailureKind::InvalidState,
            })?
            .set_pitch_semitones(semitones)
            .map_err(ProcessError::from_stretch_error)?;
        let desired_input = frames as f64 / duration_ratio + self.stretch_input_fraction;
        let input_frames = desired_input.floor() as usize;
        self.stretch_input_fraction = desired_input - input_frames as f64;
        if input_frames > self.stretch_input_left.len() {
            return Err(ProcessError::ProcessorFailure {
                kind: crate::process::ProcessorFailureKind::InvalidState,
            });
        }
        for frame in 0..input_frames {
            if self.stretch_source_exhausted {
                self.stretch_input_left[frame] = 0.0;
                self.stretch_input_right[frame] = 0.0;
                continue;
            }
            let current = read_frame(
                source.as_ref(),
                self.position,
                self.start_frame,
                self.end_frame,
                self.loop_region,
            );
            let current = self.crossfaded_frame(source.as_ref(), current);
            self.stretch_input_left[frame] = current.0;
            self.stretch_input_right[frame] = current.1;
            self.advance_stretch_input();
        }
        let input_left = &self.stretch_input_left[..input_frames];
        let input_right = &self.stretch_input_right[..input_frames];
        let input: [&[f32]; 2] = [input_left, input_right];
        let mut output: [&mut [f32]; 2] = [&mut left[..frames], &mut right[..frames]];
        self.stretcher
            .as_mut()
            .ok_or(ProcessError::ProcessorFailure {
                kind: crate::process::ProcessorFailureKind::InvalidState,
            })?
            .process(&input, &mut output)
            .map_err(ProcessError::from_stretch_error)?;
        for ((mono, left), right) in mono[..frames]
            .iter_mut()
            .zip(&left[..frames])
            .zip(&right[..frames])
        {
            *mono = (*left + *right) * 0.5;
        }
        self.stretch_output_frames = self.stretch_output_frames.saturating_add(frames as u64);
        let source_frames = self.end_frame.saturating_sub(self.start_frame) as f64;
        let target_frames =
            (source_frames * duration_ratio).ceil() as u64 + self.stretch_latency_frames as u64;
        if self.loop_region.is_none()
            && self.stretch_source_exhausted
            && self.stretch_output_frames >= target_frames
        {
            self.finished = true;
        }
        Ok(self.finished)
    }

    #[allow(clippy::cast_precision_loss)]
    fn advance_stretch_input(&mut self) {
        let next_position = self.position + 1.0;
        if let Some(loop_region) = self.loop_region {
            let loop_length = (loop_region.end_frame - loop_region.start_frame) as f64;
            self.position = if next_position >= loop_region.end_frame as f64 {
                loop_region.start_frame as f64
                    + (next_position - loop_region.end_frame as f64).rem_euclid(loop_length)
            } else {
                next_position
            };
            return;
        }
        self.position = next_position;
        if self.position >= self.end_frame as f64 {
            self.stretch_source_exhausted = true;
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    pub(crate) fn next_frame_with_ratio(&mut self, playback_ratio: f64) -> (f32, f32) {
        let Some(source) = self.source.as_deref() else {
            self.finished = true;
            return (0.0, 0.0);
        };
        if self.finished || self.start_frame >= self.end_frame {
            self.finished = true;
            return (0.0, 0.0);
        }
        if !playback_ratio.is_finite() || playback_ratio <= 0.0 {
            self.finished = true;
            return (0.0, 0.0);
        }

        let current = read_frame(
            source,
            self.position,
            self.start_frame,
            self.end_frame,
            self.loop_region,
        );
        let current = self.crossfaded_frame(source, current);
        let next_position = match self.direction {
            CompiledSampleDirection::Forward => self.position + playback_ratio,
            CompiledSampleDirection::Reverse => self.position - playback_ratio,
        };
        let gain = self.end_fade_gain(next_position);
        self.advance_position(next_position);
        (current.0 * gain, current.1 * gain)
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn crossfaded_frame(&self, source: &PreparedAudio, current: (f32, f32)) -> (f32, f32) {
        let Some(loop_region) = self.loop_region else {
            return current;
        };
        if loop_region.crossfade_frames == 0 {
            return current;
        }
        let crossfade_frames = loop_region.crossfade_frames as f64;
        let (other_position, current_gain, other_gain) = match self.direction {
            CompiledSampleDirection::Forward => {
                let crossfade_start = loop_region.end_frame as f64 - crossfade_frames;
                if self.position < crossfade_start || self.position >= loop_region.end_frame as f64
                {
                    return current;
                }
                let progress =
                    ((self.position - crossfade_start) / crossfade_frames).clamp(0.0, 1.0);
                let other_position =
                    loop_region.start_frame as f64 + (self.position - crossfade_start);
                (
                    other_position,
                    (progress * std::f64::consts::FRAC_PI_2).cos() as f32,
                    (progress * std::f64::consts::FRAC_PI_2).sin() as f32,
                )
            }
            CompiledSampleDirection::Reverse => {
                let crossfade_start = loop_region.start_frame as f64 + crossfade_frames;
                if self.position < loop_region.start_frame as f64
                    || self.position >= crossfade_start
                {
                    return current;
                }
                let progress =
                    ((crossfade_start - self.position) / crossfade_frames).clamp(0.0, 1.0);
                let other_position = loop_region.end_frame as f64
                    - 1.0
                    - (self.position - loop_region.start_frame as f64);
                (
                    other_position,
                    (progress * std::f64::consts::FRAC_PI_2).cos() as f32,
                    (progress * std::f64::consts::FRAC_PI_2).sin() as f32,
                )
            }
        };
        let other = read_frame(
            source,
            other_position,
            self.start_frame,
            self.end_frame,
            Some(loop_region),
        );
        (
            current.0 * current_gain + other.0 * other_gain,
            current.1 * current_gain + other.1 * other_gain,
        )
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn end_fade_gain(&self, next_position: f64) -> f32 {
        if self.loop_region.is_some() {
            return 1.0;
        }
        let fade_length = (self.end_fade_frames as f64)
            .min(self.end_frame.saturating_sub(self.start_frame) as f64);
        if fade_length == 0.0 {
            return 1.0;
        }
        match self.direction {
            CompiledSampleDirection::Forward => {
                let fade_start = self.end_frame as f64 - fade_length;
                if next_position <= fade_start {
                    1.0
                } else {
                    ((self.end_frame as f64 - next_position) / fade_length).clamp(0.0, 1.0) as f32
                }
            }
            CompiledSampleDirection::Reverse => {
                let fade_start = self.start_frame as f64 + fade_length;
                if next_position >= fade_start {
                    1.0
                } else {
                    ((next_position - self.start_frame as f64) / fade_length).clamp(0.0, 1.0) as f32
                }
            }
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn advance_position(&mut self, next_position: f64) {
        if let Some(loop_region) = self.loop_region {
            let loop_length = (loop_region.end_frame - loop_region.start_frame) as f64;
            self.position = match self.direction {
                CompiledSampleDirection::Forward
                    if next_position >= loop_region.end_frame as f64 =>
                {
                    loop_region.start_frame as f64
                        + (next_position - loop_region.end_frame as f64).rem_euclid(loop_length)
                }
                CompiledSampleDirection::Reverse
                    if next_position < loop_region.start_frame as f64 =>
                {
                    loop_region.start_frame as f64
                        + (next_position - loop_region.start_frame as f64).rem_euclid(loop_length)
                }
                _ => next_position,
            };
            return;
        }
        self.position = next_position;
        self.finished = match self.direction {
            CompiledSampleDirection::Forward => self.position >= self.end_frame as f64,
            CompiledSampleDirection::Reverse => self.position < self.start_frame as f64,
        };
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.finished
    }

    pub(crate) fn root_note(&self) -> u8 {
        self.root_note
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
pub(crate) fn playback_ratio(note_number: u8, root_note: u8, tuning_ratio: f32) -> f64 {
    let semitones = f64::from(note_number) - f64::from(root_note);
    2.0_f64.powf(semitones / 12.0) * f64::from(tuning_ratio)
}

fn read_frame(
    source: &PreparedAudio,
    position: f64,
    start_frame: usize,
    end_frame: usize,
    loop_region: Option<CompiledSampleLoop>,
) -> (f32, f32) {
    #[allow(clippy::cast_precision_loss)]
    let active_loop = loop_region.filter(|loop_region| {
        position >= loop_region.start_frame as f64 && position < loop_region.end_frame as f64
    });
    match &source.channels {
        PreparedAudioChannels::Mono { samples } => {
            let value = cubic_sample(samples, position, start_frame, end_frame, active_loop);
            (value, value)
        }
        PreparedAudioChannels::Stereo { left, right } => (
            cubic_sample(left, position, start_frame, end_frame, active_loop),
            cubic_sample(right, position, start_frame, end_frame, active_loop),
        ),
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn cubic_sample(
    source: &[f32],
    position: f64,
    start_frame: usize,
    end_frame: usize,
    loop_region: Option<CompiledSampleLoop>,
) -> f32 {
    if source.is_empty()
        || !position.is_finite()
        || position < start_frame as f64
        || start_frame >= end_frame
    {
        return 0.0;
    }
    let base = position.floor() as isize;
    let fraction = position.fract() as f32;
    let p0 = sample_at(source, base - 1, start_frame, end_frame, loop_region);
    let p1 = sample_at(source, base, start_frame, end_frame, loop_region);
    let p2 = sample_at(source, base + 1, start_frame, end_frame, loop_region);
    let p3 = sample_at(source, base + 2, start_frame, end_frame, loop_region);
    super::interpolation::cubic_interpolate(p0, p1, p2, p3, fraction)
}

fn sample_at(
    source: &[f32],
    index: isize,
    start_frame: usize,
    end_frame: usize,
    loop_region: Option<CompiledSampleLoop>,
) -> f32 {
    if let Some(loop_region) = loop_region {
        let Some(loop_length) = loop_region.end_frame.checked_sub(loop_region.start_frame) else {
            return 0.0;
        };
        let Ok(length) = isize::try_from(loop_length) else {
            return 0.0;
        };
        if length == 0 {
            return 0.0;
        }
        let Ok(loop_start) = isize::try_from(loop_region.start_frame) else {
            return 0.0;
        };
        let relative = (index - loop_start).rem_euclid(length);
        let Ok(relative) = usize::try_from(relative) else {
            return 0.0;
        };
        return loop_region
            .start_frame
            .checked_add(relative)
            .and_then(|source_index| source.get(source_index))
            .copied()
            .unwrap_or(0.0);
    }
    let start = isize::try_from(start_frame).unwrap_or(isize::MAX);
    let end = isize::try_from(end_frame.saturating_sub(1)).unwrap_or(isize::MAX);
    let index = index.clamp(start, end);
    usize::try_from(index)
        .ok()
        .and_then(|source_index| source.get(source_index))
        .copied()
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{PreparedAudioChannels, SampleMetadata};

    fn next_sample(runtime: &mut SampleRuntime, playback_ratio: f64) -> f32 {
        let (left, right) = runtime.next_frame_with_ratio(playback_ratio);
        (left + right) * 0.5
    }

    fn sample(values: &[f32]) -> PreparedAudio {
        PreparedAudio {
            sample_rate: 48_000.0,
            frames: values.len(),
            channels: PreparedAudioChannels::Mono {
                samples: Arc::from(values.to_vec()),
            },
            source_metadata: SampleMetadata {
                source_sample_rate: 48_000,
                source_channels: 1,
                bits_per_sample: Some(16),
                source_frames: values.len(),
            },
        }
    }

    fn stereo_sample(left: &[f32], right: &[f32]) -> PreparedAudio {
        assert_eq!(left.len(), right.len());
        PreparedAudio {
            sample_rate: 48_000.0,
            frames: left.len(),
            channels: PreparedAudioChannels::Stereo {
                left: Arc::from(left.to_vec()),
                right: Arc::from(right.to_vec()),
            },
            source_metadata: SampleMetadata {
                source_sample_rate: 48_000,
                source_channels: 2,
                bits_per_sample: Some(16),
                source_frames: left.len(),
            },
        }
    }

    fn stretched_sample(time: CompiledSampleTime) -> CompiledSample {
        let source = sample(&vec![1.0; 1_024]);
        let mut backend = DspStretch::new().expect("stretch allocation");
        backend
            .prepare(2, 48_000.0, 128, 64)
            .expect("stretch preparation");
        let input_latency = backend.input_latency().expect("input latency");
        let output_latency = backend.output_latency().expect("output latency");
        let playback = CompiledSamplePlayback {
            direction: CompiledSampleDirection::Forward,
            start_frame: 0,
            end_frame: source.frames,
            loop_region: None,
            time,
        };
        let zone = zone(source, playback);
        CompiledSample {
            zones: vec![zone].into_boxed_slice(),
            groups: Vec::new().into_boxed_slice(),
            stretch_latency: Some(CompiledStretchLatency {
                input_frames: input_latency,
                output_frames: output_latency,
            }),
        }
    }

    fn zone(source: PreparedAudio, playback: CompiledSamplePlayback) -> CompiledSampleZone {
        CompiledSampleZone {
            id: "test".to_owned(),
            source: Some(Arc::new(source)),
            root_note: 60,
            key_min: 0,
            key_max: 127,
            velocity_min: 1,
            velocity_max: 127,
            group: None,
            playback,
            asset_path: "test.wav".to_owned(),
        }
    }

    fn one_shot(start_frame: usize, end_frame: usize) -> CompiledSamplePlayback {
        CompiledSamplePlayback {
            direction: CompiledSampleDirection::Forward,
            start_frame,
            end_frame,
            loop_region: None,
            time: CompiledSampleTime::Resample,
        }
    }

    #[test]
    fn root_note_and_octave_ratios_are_exact() {
        assert!((playback_ratio(60, 60, 1.0) - 1.0).abs() < 1.0e-12);
        assert!((playback_ratio(72, 60, 1.0) - 2.0).abs() < 1.0e-12);
        assert!((playback_ratio(48, 60, 1.0) - 0.5).abs() < 1.0e-12);
    }

    #[test]
    fn cubic_interpolation_clamps_endpoints() {
        let source = [0.0, 1.0, 0.0, -1.0];
        assert!((cubic_sample(&source, 1.0, 0, 4, None) - 1.0).abs() < 1.0e-6);
        assert!(cubic_sample(&source, 3.0, 0, 4, None).is_finite());
        assert!((cubic_sample(&source, 10.0, 0, 4, None) + 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn sample_runtime_finishes_without_out_of_bounds_reads() {
        let zone = zone(sample(&[0.1, 0.2, 0.3]), one_shot(0, 3));
        let mut runtime = SampleRuntime::new();
        runtime.start(Some(&zone)).expect("sample start");
        let values: Vec<f32> = (0..5).map(|_| next_sample(&mut runtime, 1.0)).collect();
        assert!(values[..3].iter().all(|value| value.is_finite()));
        assert!(values[3..].iter().all(|value| value.abs() < 1.0e-6));
        assert!(runtime.is_finished());
    }

    #[test]
    fn sample_runtime_plays_only_the_compiled_region() {
        let zone = zone(
            sample(&[100.0, 101.0, 1.0, 2.0, 3.0, 4.0, 100.0, 101.0]),
            one_shot(2, 6),
        );
        let mut runtime = SampleRuntime::new();
        runtime.start(Some(&zone)).expect("sample start");
        let values: Vec<f32> = (0..6).map(|_| next_sample(&mut runtime, 1.0)).collect();
        assert!(values[..4].iter().all(|value| value.is_finite()));
        assert!(values[4..].iter().all(|value| value.abs() < 1.0e-6));
        assert!(runtime.is_finished());
        assert!(values[0] < 2.0);
        assert!(values[0] > 0.0);
    }

    #[test]
    fn forward_loop_wraps_fractional_and_large_overshoot() {
        let zone = zone(
            sample(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
            CompiledSamplePlayback {
                direction: CompiledSampleDirection::Forward,
                start_frame: 0,
                end_frame: 7,
                loop_region: Some(CompiledSampleLoop {
                    start_frame: 2,
                    end_frame: 5,
                    crossfade_frames: 0,
                }),
                time: CompiledSampleTime::Resample,
            },
        );
        let mut runtime = SampleRuntime::new();
        runtime.start(Some(&zone)).expect("sample start");
        let first = next_sample(&mut runtime, 2.5);
        let second = next_sample(&mut runtime, 2.5);
        let third = next_sample(&mut runtime, 20.0);
        assert!(first.is_finite());
        assert!(second.is_finite());
        assert!(third.is_finite());
        assert!((runtime.position - 4.0).abs() < 1.0e-6);
        assert!(second > 2.0);
        assert!(second < 3.0);
        assert!((third - 2.0).abs() < 1.0e-6);
        assert!(!runtime.is_finished());
    }

    #[test]
    fn cubic_interpolation_wraps_neighbors_inside_forward_loop() {
        let source = [0.0, 1.0, 2.0, 3.0, 4.0, 100.0];
        let loop_region = CompiledSampleLoop {
            start_frame: 2,
            end_frame: 5,
            crossfade_frames: 0,
        };

        let looped = cubic_sample(&source, 4.5, 1, 5, Some(loop_region));
        let bounded = cubic_sample(&source, 4.5, 1, 5, None);

        assert!(looped.is_finite());
        assert!(bounded.is_finite());
        assert!((looped - bounded).abs() > 0.01);
    }

    #[test]
    fn reverse_playback_starts_at_the_region_end_and_finishes_at_the_start() {
        let mut playback = one_shot(1, 9);
        playback.direction = CompiledSampleDirection::Reverse;
        let zone = zone(
            sample(&[100.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 100.0]),
            playback,
        );
        let mut runtime = SampleRuntime::new();
        runtime.start(Some(&zone)).expect("sample start");
        let values: Vec<f32> = (0..10).map(|_| next_sample(&mut runtime, 1.0)).collect();
        assert!(values[0] > values[1]);
        assert!(values[1] > values[2]);
        assert!(values[2] > values[3]);
        assert!(values[5] > 0.0);
        assert!(values[8..].iter().all(|value| value.abs() < 1.0e-6));
        assert!(runtime.is_finished());
    }

    #[test]
    fn reverse_loop_wraps_large_overshoot() {
        let zone = zone(
            sample(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
            CompiledSamplePlayback {
                direction: CompiledSampleDirection::Reverse,
                start_frame: 0,
                end_frame: 7,
                loop_region: Some(CompiledSampleLoop {
                    start_frame: 2,
                    end_frame: 5,
                    crossfade_frames: 0,
                }),
                time: CompiledSampleTime::Resample,
            },
        );
        let mut runtime = SampleRuntime::new();
        runtime.start(Some(&zone)).expect("sample start");
        let _ = next_sample(&mut runtime, 4.0);
        let value = next_sample(&mut runtime, 20.0);
        assert!(value.is_finite());
        assert!((runtime.position - 3.0).abs() < 1.0e-6);
        assert!(!runtime.is_finished());
    }

    #[test]
    fn reverse_loop_wraps_fractional_positions_at_the_loop_boundary() {
        let zone = zone(
            sample(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
            CompiledSamplePlayback {
                direction: CompiledSampleDirection::Reverse,
                start_frame: 0,
                end_frame: 7,
                loop_region: Some(CompiledSampleLoop {
                    start_frame: 2,
                    end_frame: 5,
                    crossfade_frames: 0,
                }),
                time: CompiledSampleTime::Resample,
            },
        );
        let mut runtime = SampleRuntime::new();
        runtime.start(Some(&zone)).expect("sample start");
        let _ = next_sample(&mut runtime, 4.0);
        let _ = next_sample(&mut runtime, 0.5);
        assert!((runtime.position - 4.5).abs() < 1.0e-6);
        assert!(!runtime.is_finished());
    }

    #[test]
    fn stereo_cubic_interpolation_uses_one_cursor_for_both_channels() {
        let left: Vec<f32> = (0..512)
            .map(|index| match index {
                1 => 1.0,
                3 => -1.0,
                _ => 0.0,
            })
            .collect();
        let right: Vec<f32> = left.iter().map(|value| value + 10.0).collect();
        let zone = zone(stereo_sample(&left, &right), one_shot(0, 512));
        let mut runtime = SampleRuntime::new();
        runtime.start(Some(&zone)).expect("sample start");
        let (left, right) = runtime.next_frame_with_ratio(1.5);
        assert!(left.is_finite() && right.is_finite());
        assert!((right - left - 10.0).abs() < 1.0e-5);
    }

    #[test]
    fn crossfade_loop_is_finite_and_stays_within_constant_power_bound() {
        let zone = zone(
            sample(&[0.0, 0.0, 1.0, 1.0, 0.0, 0.0]),
            CompiledSamplePlayback {
                direction: CompiledSampleDirection::Forward,
                start_frame: 0,
                end_frame: 6,
                loop_region: Some(CompiledSampleLoop {
                    start_frame: 2,
                    end_frame: 6,
                    crossfade_frames: 2,
                }),
                time: CompiledSampleTime::Resample,
            },
        );
        let mut runtime = SampleRuntime::new();
        runtime.start(Some(&zone)).expect("sample start");
        let values: Vec<f32> = (0..32).map(|_| next_sample(&mut runtime, 1.0)).collect();
        assert!(values.iter().all(|value| value.is_finite()));
        assert!(values.iter().all(|value| value.abs() <= 1.5));
    }

    #[test]
    fn sample_runtime_fades_nonzero_ends_at_multiple_playback_ratios() {
        for (last_value, playback_ratio) in [(0.3, 1.0), (-0.8, 0.5), (0.8, 2.0)] {
            let mut values = vec![0.25; 2_048];
            *values.last_mut().expect("fixture has samples") = last_value;
            let zone = zone(sample(&values), one_shot(0, values.len()));
            let mut runtime = SampleRuntime::new();
            runtime.start(Some(&zone)).expect("sample start");
            let mut rendered = Vec::new();
            while !runtime.is_finished() {
                rendered.push(next_sample(&mut runtime, playback_ratio));
            }
            assert!(rendered.iter().all(|value| value.is_finite()));
            assert!(rendered.last().is_some_and(|value| value.abs() < 1.0e-6));
            assert!(
                rendered[rendered.len().saturating_sub(240)..]
                    .windows(2)
                    .all(|window| (window[1] - window[0]).abs() < 0.01)
            );
        }
    }

    #[test]
    fn sample_runtime_fade_is_bounds_safe_for_short_sources() {
        for values in [&[][..], &[1.0][..], &[1.0, 2.0][..], &[1.0, 2.0, 3.0][..]] {
            let zone = zone(sample(values), one_shot(0, values.len()));
            let mut runtime = SampleRuntime::new();
            runtime.start(Some(&zone)).expect("sample start");
            for _ in 0..8 {
                assert!(next_sample(&mut runtime, 0.5).is_finite());
            }
        }
    }

    #[test]
    fn fixed_stretch_keeps_a_constant_signal_finite_and_separates_duration() {
        let compiled = stretched_sample(CompiledSampleTime::FixedStretch {
            duration_ratio: 2.0,
        });
        let mut runtime = SampleRuntime::prepared(
            &compiled,
            crate::process::ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec"),
        )
        .expect("stretch runtime prepares");
        runtime
            .start(compiled.zones.first())
            .expect("sample starts");
        let mut rendered = Vec::new();
        for _ in 0..200 {
            let mut mono = [0.0; 64];
            let mut left = [0.0; 64];
            let mut right = [0.0; 64];
            let finished = runtime
                .render_stretched(64, 60, 0.0, 120.0, &mut mono, &mut left, &mut right)
                .expect("stretch renders");
            rendered.extend(left);
            if finished {
                break;
            }
        }
        assert!(rendered.iter().all(|sample| sample.is_finite()));
        assert!(
            rendered.iter().any(|sample| sample.abs() > 0.01),
            "max sample: {}",
            rendered
                .iter()
                .map(|sample| sample.abs())
                .fold(0.0, f32::max)
        );
        assert!(rendered.len() >= 2_048);
        assert!(runtime.is_finished());
    }

    #[test]
    fn tempo_sync_uses_process_tempo_for_duration() {
        let compiled = stretched_sample(CompiledSampleTime::TempoSync { source_bpm: 120.0 });
        let render_length = |tempo_bpm| {
            let mut runtime = SampleRuntime::prepared(
                &compiled,
                crate::process::ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec"),
            )
            .expect("stretch runtime prepares");
            runtime
                .start(compiled.zones.first())
                .expect("sample starts");
            let mut rendered = Vec::new();
            for _ in 0..200 {
                let mut mono = [0.0; 64];
                let mut left = [0.0; 64];
                let mut right = [0.0; 64];
                let finished = runtime
                    .render_stretched(64, 60, 0.0, tempo_bpm, &mut mono, &mut left, &mut right)
                    .expect("stretch renders");
                rendered.extend(left);
                if finished {
                    break;
                }
            }
            assert!(rendered.iter().all(|sample| sample.is_finite()));
            assert!(runtime.is_finished());
            rendered.len()
        };

        let at_120 = render_length(120.0);
        let at_60 = render_length(60.0);
        assert!(at_60 > at_120);
    }
}
