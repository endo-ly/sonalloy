use std::sync::Arc;

use crate::asset::PreparedSample;

use super::smoothing::rounded_frame_count;

const END_FADE_SECONDS: f64 = 0.005;

/// One-shot sample playback state owned by a voice layer.
pub(crate) struct SampleRuntime {
    source: Arc<[f32]>,
    position: f64,
    playback_ratio: f64,
    end_fade_frames: usize,
    finished: bool,
}

impl SampleRuntime {
    pub(crate) fn new(source: &PreparedSample) -> Self {
        Self {
            source: Arc::clone(&source.samples),
            position: 0.0,
            playback_ratio: 1.0,
            end_fade_frames: rounded_frame_count(source.sample_rate * END_FADE_SECONDS).max(1),
            finished: false,
        }
    }

    pub(crate) fn start(&mut self, playback_ratio: f64) {
        self.position = 0.0;
        self.playback_ratio = playback_ratio;
        self.finished = self.source.is_empty();
    }

    pub(crate) fn reset(&mut self) {
        self.position = 0.0;
        self.playback_ratio = 1.0;
        self.finished = false;
    }

    #[cfg(test)]
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn next_sample(&mut self) -> f32 {
        let playback_ratio = self.playback_ratio;
        self.next_sample_with_ratio(playback_ratio)
    }

    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn next_sample_with_ratio(&mut self, playback_ratio: f64) -> f32 {
        if self.finished {
            return 0.0;
        }
        if !playback_ratio.is_finite() || playback_ratio <= 0.0 {
            self.finished = true;
            return 0.0;
        }
        let sample = cubic_sample(&self.source, self.position);
        let next_position = self.position + playback_ratio;
        self.position = next_position;
        if self.position >= self.source.len() as f64 {
            self.finished = true;
        }
        let fade_end = self.source.len().saturating_sub(1) as f64;
        let fade_length = self.end_fade_frames.min(fade_end as usize) as f64;
        let gain = if fade_length == 0.0 {
            0.0
        } else if next_position < fade_end - fade_length {
            1.0
        } else {
            ((fade_end - next_position) / fade_length).clamp(0.0, 1.0) as f32
        };
        sample * gain
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.finished
    }

    #[cfg(test)]
    pub(crate) fn position(&self) -> f64 {
        self.position
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
fn cubic_sample(source: &[f32], position: f64) -> f32 {
    if source.is_empty() || !position.is_finite() || position < 0.0 {
        return 0.0;
    }
    let base = position.floor() as isize;
    let fraction = position.fract() as f32;
    let p0 = sample_at(source, base - 1);
    let p1 = sample_at(source, base);
    let p2 = sample_at(source, base + 1);
    let p3 = sample_at(source, base + 2);
    let a = 0.5 * (p2 - p0);
    let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
    let c = 0.5 * (p3 - p0) + 1.5 * (p1 - p2);
    ((c.mul_add(fraction, b)).mul_add(fraction, a)).mul_add(fraction, p1)
}

fn sample_at(source: &[f32], index: isize) -> f32 {
    if index <= 0 {
        return source[0];
    }
    let index = usize::try_from(index)
        .unwrap_or(usize::MAX)
        .min(source.len() - 1);
    source[index]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{PreparedSample, SampleMetadata};

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

    #[test]
    fn root_note_and_octave_ratios_are_exact() {
        assert!((playback_ratio(60, 60, 1.0) - 1.0).abs() < 1.0e-12);
        assert!((playback_ratio(72, 60, 1.0) - 2.0).abs() < 1.0e-12);
        assert!((playback_ratio(48, 60, 1.0) - 0.5).abs() < 1.0e-12);
    }

    #[test]
    fn cubic_interpolation_clamps_endpoints() {
        let source = [0.0, 1.0, 0.0, -1.0];
        assert!((cubic_sample(&source, 1.0) - 1.0).abs() < 1.0e-6);
        assert!(cubic_sample(&source, 3.0).is_finite());
        assert!((cubic_sample(&source, 10.0) + 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn sample_runtime_finishes_without_out_of_bounds_reads() {
        let source = sample(&[0.1, 0.2, 0.3]);
        let mut runtime = SampleRuntime::new(&source);
        runtime.start(1.0);
        let values: Vec<f32> = (0..5).map(|_| runtime.next_sample()).collect();
        assert!(values[..3].iter().all(|value| value.is_finite()));
        assert!(values[3..].iter().all(|value| value.abs() < 1.0e-6));
        assert!(runtime.is_finished());
        assert!((runtime.position() - 3.0).abs() < 1.0e-6);
    }

    #[test]
    fn sample_runtime_fades_nonzero_ends_at_multiple_playback_ratios() {
        for (last_value, playback_ratio) in [(0.3, 1.0), (-0.8, 0.5), (0.8, 2.0)] {
            let mut values = vec![0.25; 2_048];
            *values.last_mut().expect("fixture has samples") = last_value;
            let source = sample(&values);
            let mut runtime = SampleRuntime::new(&source);
            runtime.start(playback_ratio);
            let mut rendered = Vec::new();
            while !runtime.is_finished() {
                rendered.push(runtime.next_sample());
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
            let source = sample(values);
            let mut runtime = SampleRuntime::new(&source);
            runtime.start(0.5);
            for _ in 0..8 {
                assert!(runtime.next_sample().is_finite());
            }
        }
    }

    #[test]
    fn fractional_positions_are_finite_for_short_buffers() {
        for values in [&[][..], &[1.0][..], &[1.0, 2.0][..], &[1.0, 2.0, 3.0][..]] {
            let source = sample(values);
            let mut runtime = SampleRuntime::new(&source);
            runtime.start(0.5);
            for _ in 0..8 {
                assert!(runtime.next_sample().is_finite());
            }
        }
    }
}
