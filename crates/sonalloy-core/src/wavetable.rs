use std::path::Path;
use std::sync::Arc;

use rustfft::{FftPlanner, num_complex::Complex};
use thiserror::Error;

use crate::asset::{AssetError, load_mono_audio};
use crate::compiler::{
    PreparedWavetable, PreparedWavetableBand, PreparedWavetableFrame, WavetableSourceMetadata,
};
use crate::definition::AssetReference;

const MIN_FRAME_LENGTH: usize = 64;
const MAX_FRAME_LENGTH: usize = 4096;
const MAX_FRAME_COUNT: usize = 256;
const MAX_PREPARED_BYTES: usize = 256 * 1024 * 1024;

/// Compile-time warnings found while preparing Wavetable frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum WavetableWarning {
    /// A frame has an RMS value below the useful-signal threshold.
    SilentFrame { index: usize, rms: f32 },
    /// A frame has a source DC offset above the review threshold.
    DcOffset { index: usize, mean: f32 },
}

/// Prepared Wavetable and its frame-level review information.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WavetablePreparation {
    /// Immutable band tables shared by all voices.
    pub(crate) prepared: Arc<PreparedWavetable>,
    /// Warnings found before spectral preparation.
    pub(crate) warnings: Box<[WavetableWarning]>,
}

/// Failure while preparing a Wavetable asset.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum WavetablePreparationError {
    /// Asset resolution, verification, or decoding failed.
    #[error(transparent)]
    Asset(#[from] AssetError),
    /// The decoded sample layout cannot represent the requested table.
    #[error("wavetable layout is invalid: {0}")]
    Layout(String),
    /// The prepared table exceeds the fixed memory limit.
    #[error("prepared wavetable requires {0} bytes")]
    ResourceLimit(usize),
    /// FFT or table generation produced invalid data.
    #[error("wavetable preparation failed: {0}")]
    Preparation(String),
    /// Every source frame is silent.
    #[error("wavetable asset contains no audible frame")]
    Silent,
}

/// Decode and prepare one Wavetable asset without sample-rate conversion.
pub(crate) fn prepare_wavetable_asset(
    reference: &AssetReference,
    definition_base_dir: &Path,
    frame_length: usize,
) -> Result<WavetablePreparation, WavetablePreparationError> {
    if !(MIN_FRAME_LENGTH..=MAX_FRAME_LENGTH).contains(&frame_length)
        || !frame_length.is_power_of_two()
    {
        return Err(WavetablePreparationError::Layout(format!(
            "frame length must be a power of two between {MIN_FRAME_LENGTH} and {MAX_FRAME_LENGTH}, got {frame_length}"
        )));
    }

    let source = load_mono_audio(reference, definition_base_dir)?;
    if source.samples.is_empty() || source.samples.len() % frame_length != 0 {
        return Err(WavetablePreparationError::Layout(format!(
            "decoded sample count {} is not divisible by frame length {frame_length}",
            source.samples.len()
        )));
    }
    let frame_count = source.samples.len() / frame_length;
    if !(1..=MAX_FRAME_COUNT).contains(&frame_count) {
        return Err(WavetablePreparationError::Layout(format!(
            "frame count must be between 1 and {MAX_FRAME_COUNT}, got {frame_count}"
        )));
    }

    let band_limits = band_limits(frame_length);
    let prepared_bytes = band_limits
        .len()
        .checked_mul(frame_count)
        .and_then(|value| value.checked_mul(frame_length + 3))
        .and_then(|value| value.checked_mul(std::mem::size_of::<f32>()))
        .ok_or(WavetablePreparationError::ResourceLimit(usize::MAX))?;
    if prepared_bytes >= MAX_PREPARED_BYTES {
        return Err(WavetablePreparationError::ResourceLimit(prepared_bytes));
    }

    let warnings = frame_warnings(&source.samples, frame_length);
    if warnings
        .iter()
        .filter(|warning| matches!(warning, WavetableWarning::SilentFrame { .. }))
        .count()
        == frame_count
    {
        return Err(WavetablePreparationError::Silent);
    }

    let mut planner = FftPlanner::<f32>::new();
    let forward = planner.plan_fft_forward(frame_length);
    let inverse = planner.plan_fft_inverse(frame_length);
    #[allow(clippy::cast_precision_loss)]
    let scale = 1.0 / frame_length as f32;
    let mut bands = Vec::with_capacity(band_limits.len());
    for max_harmonic in band_limits {
        let mut frames = Vec::with_capacity(frame_count);
        for source_frame in source.samples.chunks_exact(frame_length) {
            let mut spectrum = source_frame
                .iter()
                .copied()
                .map(|sample| Complex::new(sample, 0.0))
                .collect::<Vec<_>>();
            forward.process(&mut spectrum);
            for (bin, value) in spectrum.iter_mut().enumerate() {
                if bin != 0 && harmonic_for_bin(bin, frame_length) > max_harmonic {
                    *value = Complex::new(0.0, 0.0);
                }
            }
            inverse.process(&mut spectrum);
            let mut samples = Vec::with_capacity(frame_length);
            for value in spectrum {
                let sample = value.re * scale;
                if !sample.is_finite() {
                    return Err(WavetablePreparationError::Preparation(
                        "inverse FFT produced a non-finite sample".to_owned(),
                    ));
                }
                samples.push(sample);
            }
            let mut guarded = Vec::with_capacity(frame_length + 3);
            guarded.push(*samples.last().ok_or_else(|| {
                WavetablePreparationError::Preparation("prepared frame is empty".to_owned())
            })?);
            guarded.extend_from_slice(&samples);
            guarded.push(samples[0]);
            guarded.push(samples[1]);
            frames.push(PreparedWavetableFrame {
                guarded_samples: guarded.into_boxed_slice(),
            });
        }
        bands.push(PreparedWavetableBand {
            max_harmonic,
            frames: frames.into_boxed_slice(),
        });
    }

    Ok(WavetablePreparation {
        prepared: Arc::new(PreparedWavetable {
            frame_length,
            frame_count,
            bands: bands.into_boxed_slice(),
            source_metadata: WavetableSourceMetadata {
                source_sample_rate: source.sample_rate,
                source_channels: source.channels,
                bits_per_sample: source.bits_per_sample,
                source_frames: source.source_frames,
            },
        }),
        warnings: warnings.into_boxed_slice(),
    })
}

fn band_limits(frame_length: usize) -> Vec<usize> {
    let mut limits = Vec::new();
    let mut limit = frame_length / 2;
    while limit >= 1 {
        limits.push(limit);
        limit /= 2;
    }
    limits
}

fn harmonic_for_bin(bin: usize, frame_length: usize) -> usize {
    if bin <= frame_length / 2 {
        bin
    } else {
        frame_length - bin
    }
}

fn frame_warnings(samples: &[f32], frame_length: usize) -> Vec<WavetableWarning> {
    samples
        .chunks_exact(frame_length)
        .enumerate()
        .flat_map(|(index, frame)| {
            let sum = frame.iter().copied().sum::<f32>();
            #[allow(clippy::cast_precision_loss)]
            let frame_length_f32 = frame_length as f32;
            let mean = sum / frame_length_f32;
            #[allow(clippy::cast_precision_loss)]
            let rms = (frame
                .iter()
                .copied()
                .map(|sample| sample * sample)
                .sum::<f32>()
                / frame_length_f32)
                .sqrt();
            let mut warnings = Vec::with_capacity(2);
            if rms < 1.0e-6 {
                warnings.push(WavetableWarning::SilentFrame { index, rms });
            }
            if mean.abs() > 0.01 {
                warnings.push(WavetableWarning::DcOffset { index, mean });
            }
            warnings
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_limits_are_descending_powers_of_two() {
        assert_eq!(band_limits(64), [32, 16, 8, 4, 2, 1]);
        assert_eq!(band_limits(2048).len(), 11);
    }

    #[test]
    fn harmonic_lookup_preserves_negative_frequency_bins() {
        assert_eq!(harmonic_for_bin(0, 64), 0);
        assert_eq!(harmonic_for_bin(1, 64), 1);
        assert_eq!(harmonic_for_bin(32, 64), 32);
        assert_eq!(harmonic_for_bin(63, 64), 1);
    }

    #[test]
    fn frame_warnings_distinguish_silent_and_dc_frames() {
        let samples = [0.0_f32; 64]
            .into_iter()
            .chain(std::iter::repeat_n(0.02_f32, 64))
            .collect::<Vec<_>>();
        let warnings = frame_warnings(&samples, 64);
        assert!(
            warnings.iter().any(|warning| {
                matches!(warning, WavetableWarning::SilentFrame { index: 0, .. })
            })
        );
        assert!(
            warnings
                .iter()
                .any(|warning| { matches!(warning, WavetableWarning::DcOffset { index: 1, .. }) })
        );
    }
}
