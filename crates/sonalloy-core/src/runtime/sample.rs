use std::sync::Arc;

use crate::compiler::{CompiledSamplePlayback, CompiledSampleZone};

use super::smoothing::rounded_frame_count;

const END_FADE_SECONDS: f64 = 0.005;

/// Sample playback state owned by a voice layer.
pub(crate) struct SampleRuntime {
    source: Option<Arc<[f32]>>,
    selected_zone_index: Option<usize>,
    root_note: u8,
    position: f64,
    start_frame: usize,
    end_frame: usize,
    loop_frames: Option<(usize, usize)>,
    end_fade_frames: usize,
    finished: bool,
}

impl SampleRuntime {
    pub(crate) fn new() -> Self {
        Self {
            source: None,
            selected_zone_index: None,
            root_note: 60,
            position: 0.0,
            start_frame: 0,
            end_frame: 0,
            loop_frames: None,
            end_fade_frames: 1,
            finished: false,
        }
    }

    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn start(&mut self, zone_index: Option<usize>, zone: Option<&CompiledSampleZone>) {
        let Some(zone) = zone else {
            self.reset();
            self.finished = true;
            return;
        };
        let Some(source) = zone.source.as_ref() else {
            self.reset();
            self.finished = true;
            return;
        };
        self.selected_zone_index = zone_index;
        self.source = Some(Arc::clone(&source.samples));
        self.root_note = zone.root_note;
        self.end_fade_frames = rounded_frame_count(source.sample_rate * END_FADE_SECONDS).max(1);
        match zone.playback {
            CompiledSamplePlayback::OneShot {
                start_frame,
                end_frame,
            } => {
                self.start_frame = start_frame;
                self.end_frame = end_frame;
                self.loop_frames = None;
            }
            CompiledSamplePlayback::ForwardLoop {
                start_frame,
                end_frame,
                loop_start_frame,
                loop_end_frame,
            } => {
                self.start_frame = start_frame;
                self.end_frame = end_frame;
                self.loop_frames = Some((loop_start_frame, loop_end_frame));
            }
        }
        self.position = self.start_frame as f64;
        self.finished = self.start_frame >= self.end_frame || source.samples.is_empty();
    }

    pub(crate) fn reset(&mut self) {
        self.source = None;
        self.selected_zone_index = None;
        self.root_note = 60;
        self.position = 0.0;
        self.start_frame = 0;
        self.end_frame = 0;
        self.loop_frames = None;
        self.end_fade_frames = 1;
        self.finished = false;
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    pub(crate) fn next_sample_with_ratio(&mut self, playback_ratio: f64) -> f32 {
        debug_assert_eq!(self.selected_zone_index.is_some(), self.source.is_some());
        let Some(source) = self.source.as_deref() else {
            self.finished = true;
            return 0.0;
        };
        if self.finished || self.position >= self.end_frame as f64 {
            self.finished = true;
            return 0.0;
        }
        if !playback_ratio.is_finite() || playback_ratio <= 0.0 {
            self.finished = true;
            return 0.0;
        }
        let active_loop = self
            .loop_frames
            .filter(|(loop_start, _)| self.position >= *loop_start as f64);
        let sample = cubic_sample(
            source,
            self.position,
            self.start_frame,
            self.end_frame,
            active_loop,
        );
        let next_position = self.position + playback_ratio;
        let gain = if self.loop_frames.is_some() {
            1.0
        } else {
            let region_length = self.end_frame.saturating_sub(self.start_frame) as f64;
            let fade_length = (self.end_fade_frames as f64).min(region_length);
            let fade_start = self.end_frame as f64 - fade_length;
            if fade_length == 0.0 || next_position <= fade_start {
                1.0
            } else {
                ((self.end_frame as f64 - next_position) / fade_length).clamp(0.0, 1.0) as f32
            }
        };
        if let Some((loop_start, loop_end)) = self.loop_frames {
            let loop_length = (loop_end - loop_start) as f64;
            self.position = if next_position >= loop_end as f64 {
                loop_start as f64 + (next_position - loop_end as f64).rem_euclid(loop_length)
            } else {
                next_position
            };
        } else {
            self.position = next_position;
            if self.position >= self.end_frame as f64 {
                self.finished = true;
            }
        }
        sample * gain
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

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn cubic_sample(
    source: &[f32],
    position: f64,
    start_frame: usize,
    end_frame: usize,
    loop_frames: Option<(usize, usize)>,
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
    let p0 = sample_at(source, base - 1, start_frame, end_frame, loop_frames);
    let p1 = sample_at(source, base, start_frame, end_frame, loop_frames);
    let p2 = sample_at(source, base + 1, start_frame, end_frame, loop_frames);
    let p3 = sample_at(source, base + 2, start_frame, end_frame, loop_frames);
    let a = 0.5 * (p2 - p0);
    let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
    let c = 0.5 * (p3 - p0) + 1.5 * (p1 - p2);
    ((c.mul_add(fraction, b)).mul_add(fraction, a)).mul_add(fraction, p1)
}

fn sample_at(
    source: &[f32],
    index: isize,
    start_frame: usize,
    end_frame: usize,
    loop_frames: Option<(usize, usize)>,
) -> f32 {
    if let Some((loop_start, loop_end)) = loop_frames {
        let length = isize::try_from(loop_end - loop_start).unwrap_or(isize::MAX);
        let relative =
            (index - isize::try_from(loop_start).unwrap_or(isize::MAX)).rem_euclid(length);
        return source[loop_start + usize::try_from(relative).unwrap_or(0)];
    }
    let start = isize::try_from(start_frame).unwrap_or(isize::MAX);
    let end = isize::try_from(end_frame.saturating_sub(1)).unwrap_or(isize::MAX);
    let index = index.clamp(start, end);
    source[usize::try_from(index).unwrap_or(start_frame)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{PreparedSample, SampleMetadata};

    fn next_sample(runtime: &mut SampleRuntime, playback_ratio: f64) -> f32 {
        runtime.next_sample_with_ratio(playback_ratio)
    }

    fn sample(values: &[f32]) -> PreparedSample {
        PreparedSample {
            sample_rate: 48_000.0,
            samples: Arc::from(values.to_vec()),
            source_metadata: SampleMetadata {
                source_sample_rate: 48_000,
                source_channels: 1,
                bits_per_sample: Some(16),
                source_frames: values.len(),
            },
        }
    }

    fn zone(values: &[f32], playback: CompiledSamplePlayback) -> CompiledSampleZone {
        let source = Arc::new(sample(values));
        CompiledSampleZone {
            id: "test".to_owned(),
            source: Some(source),
            root_note: 60,
            key_min: 0,
            key_max: 127,
            velocity_min: 1,
            velocity_max: 127,
            group: None,
            playback,
            asset_path: "test.wav".to_owned(),
            asset_sha256: None,
            enabled: true,
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
        let zone = zone(
            &[0.1, 0.2, 0.3],
            CompiledSamplePlayback::OneShot {
                start_frame: 0,
                end_frame: 3,
            },
        );
        let mut runtime = SampleRuntime::new();
        runtime.start(Some(0), Some(&zone));
        let values: Vec<f32> = (0..5).map(|_| next_sample(&mut runtime, 1.0)).collect();
        assert!(values[..3].iter().all(|value| value.is_finite()));
        assert!(values[3..].iter().all(|value| value.abs() < 1.0e-6));
        assert!(runtime.is_finished());
        assert!((runtime.position - 3.0).abs() < 1.0e-6);
    }

    #[test]
    fn sample_runtime_plays_only_the_compiled_region() {
        let zone = zone(
            &[100.0, 101.0, 1.0, 2.0, 3.0, 4.0, 100.0, 101.0],
            CompiledSamplePlayback::OneShot {
                start_frame: 2,
                end_frame: 6,
            },
        );
        let mut runtime = SampleRuntime::new();
        runtime.start(Some(0), Some(&zone));

        let values: Vec<f32> = (0..6).map(|_| next_sample(&mut runtime, 1.0)).collect();

        assert!(values[..4].iter().all(|value| value.is_finite()));
        assert!(values[4..].iter().all(|value| value.abs() < 1.0e-6));
        assert!((runtime.position - 6.0).abs() < 1.0e-6);
        assert!(runtime.is_finished());
        assert!(values[0] < 2.0);
        assert!(values[0] > 0.0);
    }

    #[test]
    fn forward_loop_wraps_fractional_and_large_overshoot() {
        let zone = zone(
            &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            CompiledSamplePlayback::ForwardLoop {
                start_frame: 0,
                end_frame: 7,
                loop_start_frame: 2,
                loop_end_frame: 5,
            },
        );
        let mut runtime = SampleRuntime::new();
        runtime.start(Some(0), Some(&zone));

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

        let looped = cubic_sample(&source, 4.5, 1, 5, Some((2, 5)));
        let bounded = cubic_sample(&source, 4.5, 1, 5, None);

        assert!(looped.is_finite());
        assert!(bounded.is_finite());
        assert!((looped - bounded).abs() > 0.01);
    }

    #[test]
    fn sample_runtime_fades_nonzero_ends_at_multiple_playback_ratios() {
        for (last_value, playback_ratio) in [(0.3, 1.0), (-0.8, 0.5), (0.8, 2.0)] {
            let mut values = vec![0.25; 2_048];
            *values.last_mut().expect("fixture has samples") = last_value;
            let zone = zone(
                &values,
                CompiledSamplePlayback::OneShot {
                    start_frame: 0,
                    end_frame: values.len(),
                },
            );
            let mut runtime = SampleRuntime::new();
            runtime.start(Some(0), Some(&zone));
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
            let end_frame = values.len();
            let zone = zone(
                values,
                CompiledSamplePlayback::OneShot {
                    start_frame: 0,
                    end_frame,
                },
            );
            let mut runtime = SampleRuntime::new();
            runtime.start(Some(0), Some(&zone));
            for _ in 0..8 {
                assert!(next_sample(&mut runtime, 0.5).is_finite());
            }
        }
    }
}
