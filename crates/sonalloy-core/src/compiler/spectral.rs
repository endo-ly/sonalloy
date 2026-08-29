use std::f32::consts::TAU;
use std::fmt;
use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner};
use thiserror::Error;

use crate::asset::{PreparedAudio, PreparedAudioChannels, SampleMetadata};

/// FFT sizes supported by the spectral generator.
pub(crate) const SPECTRAL_FFT_SIZES: [usize; 3] = [1024, 2048, 4096];
/// Maximum prepared spectral storage for one source asset.
pub(crate) const MAX_PREPARED_SPECTRAL_BYTES_PER_ASSET: usize = 64 * 1024 * 1024;

/// Prepared STFT data shared by every voice using one spectral source.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedSpectralAsset {
    /// Sample rate of the prepared source.
    pub sample_rate: f64,
    /// Number of source channels.
    pub channels: usize,
    /// Number of frames in the prepared source before spectral padding.
    pub source_frames: usize,
    /// Source metadata retained for inspection.
    pub source_metadata: SampleMetadata,
    /// FFT size used for analysis.
    pub fft_size: usize,
    /// Fixed synthesis hop size.
    pub hop_size: usize,
    /// Number of non-negative-frequency bins.
    pub bin_count: usize,
    /// Number of padded STFT frames.
    pub spectral_frame_count: usize,
    /// Algorithmic latency introduced by the leading padding.
    pub latency_frames: usize,
    /// Contiguous channel-major magnitude storage.
    pub magnitudes: Arc<[f32]>,
    /// Contiguous channel-major absolute phase storage.
    pub phases: Arc<[f32]>,
    /// Contiguous channel-major instantaneous-frequency storage.
    pub instantaneous_frequencies_hz: Arc<[f32]>,
    /// Bytes occupied by the three spectral arrays.
    pub prepared_bytes: usize,
}

impl PreparedSpectralAsset {
    pub(crate) fn index(&self, channel: usize, frame: usize, bin: usize) -> Option<usize> {
        channel
            .checked_mul(self.spectral_frame_count)?
            .checked_add(frame)?
            .checked_mul(self.bin_count)?
            .checked_add(bin)
    }

    pub(crate) fn check_layout(&self) -> bool {
        let Some(cell_count) = self
            .channels
            .checked_mul(self.spectral_frame_count)
            .and_then(|value| value.checked_mul(self.bin_count))
        else {
            return false;
        };
        self.channels > 0
            && self.channels <= 2
            && self.source_frames > 0
            && self.fft_size > 0
            && self.hop_size > 0
            && self.bin_count == self.fft_size / 2 + 1
            && self.spectral_frame_count > 0
            && self.magnitudes.len() == cell_count
            && self.phases.len() == cell_count
            && self.instantaneous_frequencies_hz.len() == cell_count
            && self.magnitudes.iter().all(|value| value.is_finite())
            && self.phases.iter().all(|value| value.is_finite())
            && self
                .instantaneous_frequencies_hz
                .iter()
                .all(|value| value.is_finite())
    }
}

/// Shared inverse transform plan and normalized synthesis window.
#[derive(Clone)]
pub(crate) struct SpectralSynthesisPlan {
    fft_size: usize,
    synthesis_window: Arc<[f32]>,
    inverse: Arc<dyn ComplexToReal<f32>>,
}

impl fmt::Debug for SpectralSynthesisPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpectralSynthesisPlan")
            .field("fft_size", &self.fft_size)
            .field("synthesis_window", &self.synthesis_window)
            .finish_non_exhaustive()
    }
}

impl PartialEq for SpectralSynthesisPlan {
    fn eq(&self, other: &Self) -> bool {
        self.fft_size == other.fft_size && self.synthesis_window == other.synthesis_window
    }
}

impl Eq for SpectralSynthesisPlan {}

impl SpectralSynthesisPlan {
    pub(crate) fn new(fft_size: usize) -> Result<Self, SpectralPreparationError> {
        let hop_size = spectral_hop_size(fft_size)?;
        let mut planner = RealFftPlanner::<f32>::new();
        let inverse = planner.plan_fft_inverse(fft_size);
        let synthesis_window =
            Arc::from(build_synthesis_window(fft_size, hop_size).into_boxed_slice());
        Ok(Self {
            fft_size,
            synthesis_window,
            inverse,
        })
    }

    pub(crate) fn fft_size(&self) -> usize {
        self.fft_size
    }

    pub(crate) fn hop_size(&self) -> usize {
        self.fft_size / 4
    }

    pub(crate) fn bin_count(&self) -> usize {
        self.fft_size / 2 + 1
    }

    pub(crate) fn inverse_scratch_len(&self) -> usize {
        self.inverse.get_scratch_len()
    }

    pub(crate) fn synthesis_window(&self) -> &[f32] {
        &self.synthesis_window
    }

    pub(crate) fn inverse(
        &self,
        input: &mut [Complex<f32>],
        output: &mut [f32],
        scratch: &mut [Complex<f32>],
    ) -> Result<(), realfft::FftError> {
        self.inverse.process_with_scratch(input, output, scratch)
    }
}

/// Failure while preparing a spectral asset.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum SpectralPreparationError {
    /// The decoded source asset could not be prepared.
    #[error("spectral asset preparation failed: {0}")]
    Asset(#[source] crate::asset::AssetError),
    /// The requested FFT size is not supported.
    #[error("spectral fft size is invalid: {0}")]
    InvalidFftSize(usize),
    /// The source layout cannot be represented by the prepared arrays.
    #[error("spectral source layout is invalid: {0}")]
    Layout(String),
    /// The prepared arrays exceed the fixed memory budget.
    #[error("prepared spectral asset requires {0} bytes")]
    ResourceLimit(usize),
    /// The STFT calculation produced invalid data.
    #[error("spectral preparation failed: {0}")]
    Preparation(String),
}

/// Prepare one decoded and sample-rate-converted audio asset for spectral processing.
#[allow(clippy::too_many_lines)]
pub(crate) fn prepare_spectral_asset(
    audio: &PreparedAudio,
    fft_size: usize,
) -> Result<PreparedSpectralAsset, SpectralPreparationError> {
    let hop_size = spectral_hop_size(fft_size)?;
    if audio.frames == 0 || audio.sample_rate <= 0.0 || !audio.sample_rate.is_finite() {
        return Err(SpectralPreparationError::Layout(
            "prepared audio must have a finite sample rate and at least one frame".to_owned(),
        ));
    }
    let channels = audio.channel_count();
    if !(1..=2).contains(&channels) {
        return Err(SpectralPreparationError::Layout(
            "only mono and stereo sources are supported".to_owned(),
        ));
    }
    let latency_frames = fft_size - hop_size;
    let padding = latency_frames
        .checked_mul(2)
        .ok_or_else(|| SpectralPreparationError::Layout("spectral padding overflows".to_owned()))?;
    let padded_source_frames = audio.frames.checked_add(padding).ok_or_else(|| {
        SpectralPreparationError::Layout("spectral source length overflows".to_owned())
    })?;
    let spectral_frame_count = if padded_source_frames <= fft_size {
        1
    } else {
        padded_source_frames
            .checked_sub(fft_size)
            .and_then(|value| value.checked_add(hop_size - 1))
            .and_then(|value| value.checked_div(hop_size))
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| {
                SpectralPreparationError::Layout("spectral frame count overflows".to_owned())
            })?
    };
    let bin_count = fft_size / 2 + 1;
    let cell_count = channels
        .checked_mul(spectral_frame_count)
        .and_then(|value| value.checked_mul(bin_count))
        .ok_or(SpectralPreparationError::ResourceLimit(usize::MAX))?;
    let prepared_bytes = cell_count
        .checked_mul(3)
        .and_then(|value| value.checked_mul(std::mem::size_of::<f32>()))
        .ok_or(SpectralPreparationError::ResourceLimit(usize::MAX))?;
    if prepared_bytes > MAX_PREPARED_SPECTRAL_BYTES_PER_ASSET {
        return Err(SpectralPreparationError::ResourceLimit(prepared_bytes));
    }

    let analysis_window = build_analysis_window(fft_size);
    let mut magnitudes = Vec::with_capacity(cell_count);
    let mut phases = Vec::with_capacity(cell_count);
    let mut instantaneous_frequencies_hz = Vec::with_capacity(cell_count);
    match &audio.channels {
        PreparedAudioChannels::Mono { samples } => append_channel_spectrum(
            samples,
            audio.sample_rate,
            fft_size,
            hop_size,
            latency_frames,
            spectral_frame_count,
            &analysis_window,
            &mut magnitudes,
            &mut phases,
            &mut instantaneous_frequencies_hz,
        )?,
        PreparedAudioChannels::Stereo { left, right } => {
            append_channel_spectrum(
                left,
                audio.sample_rate,
                fft_size,
                hop_size,
                latency_frames,
                spectral_frame_count,
                &analysis_window,
                &mut magnitudes,
                &mut phases,
                &mut instantaneous_frequencies_hz,
            )?;
            append_channel_spectrum(
                right,
                audio.sample_rate,
                fft_size,
                hop_size,
                latency_frames,
                spectral_frame_count,
                &analysis_window,
                &mut magnitudes,
                &mut phases,
                &mut instantaneous_frequencies_hz,
            )?;
        }
    }
    let prepared = PreparedSpectralAsset {
        sample_rate: audio.sample_rate,
        channels,
        source_frames: audio.frames,
        source_metadata: audio.source_metadata.clone(),
        fft_size,
        hop_size,
        bin_count,
        spectral_frame_count,
        latency_frames,
        magnitudes: Arc::from(magnitudes.into_boxed_slice()),
        phases: Arc::from(phases.into_boxed_slice()),
        instantaneous_frequencies_hz: Arc::from(instantaneous_frequencies_hz.into_boxed_slice()),
        prepared_bytes,
    };
    if prepared.check_layout() {
        Ok(prepared)
    } else {
        Err(SpectralPreparationError::Preparation(
            "prepared spectral arrays have an invalid layout".to_owned(),
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn append_channel_spectrum(
    samples: &[f32],
    sample_rate: f64,
    fft_size: usize,
    hop_size: usize,
    latency_frames: usize,
    spectral_frame_count: usize,
    analysis_window: &[f32],
    magnitudes: &mut Vec<f32>,
    phases: &mut Vec<f32>,
    instantaneous_frequencies_hz: &mut Vec<f32>,
) -> Result<(), SpectralPreparationError> {
    if samples.is_empty() || !samples.iter().all(|sample| sample.is_finite()) {
        return Err(SpectralPreparationError::Layout(
            "source channel must contain finite samples".to_owned(),
        ));
    }
    let padded_length = spectral_frame_count
        .checked_sub(1)
        .and_then(|value| value.checked_mul(hop_size))
        .and_then(|value| value.checked_add(fft_size))
        .ok_or_else(|| {
            SpectralPreparationError::Layout("padded source length overflows".to_owned())
        })?;
    let mut padded = vec![0.0_f32; padded_length];
    let source_end = latency_frames
        .checked_add(samples.len())
        .ok_or_else(|| SpectralPreparationError::Layout("source padding overflows".to_owned()))?;
    if source_end > padded.len() {
        return Err(SpectralPreparationError::Layout(
            "padded source does not contain the complete source".to_owned(),
        ));
    }
    padded[latency_frames..source_end].copy_from_slice(samples);

    let mut planner = RealFftPlanner::<f32>::new();
    let forward = planner.plan_fft_forward(fft_size);
    let mut input = vec![0.0_f32; fft_size];
    let mut spectrum = forward.make_output_vec();
    let mut scratch = forward.make_scratch_vec();
    let mut previous_phases = vec![0.0_f32; fft_size / 2 + 1];
    for frame in 0..spectral_frame_count {
        let start = frame
            .checked_mul(hop_size)
            .ok_or_else(|| SpectralPreparationError::Layout("frame offset overflows".to_owned()))?;
        input.copy_from_slice(&padded[start..start + fft_size]);
        for (sample, window) in input.iter_mut().zip(analysis_window) {
            *sample *= *window;
        }
        forward
            .process_with_scratch(&mut input, &mut spectrum, &mut scratch)
            .map_err(|error| SpectralPreparationError::Preparation(error.to_string()))?;
        for (bin, value) in spectrum.iter().enumerate() {
            let magnitude = value.norm();
            let phase = value.im.atan2(value.re);
            #[allow(clippy::cast_precision_loss)]
            let nominal_frequency = bin as f64 * sample_rate / fft_size as f64;
            let frequency = if frame == 0 {
                #[allow(clippy::cast_possible_truncation)]
                {
                    nominal_frequency as f32
                }
            } else {
                #[allow(clippy::cast_precision_loss)]
                let expected_phase_advance = TAU * bin as f32 * hop_size as f32 / fft_size as f32;
                let phase_delta = wrap_phase(phase - previous_phases[bin] - expected_phase_advance);
                let true_phase_advance = expected_phase_advance + phase_delta;
                #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                {
                    (f64::from(true_phase_advance) * sample_rate
                        / (f64::from(TAU) * hop_size as f64)) as f32
                }
            };
            if !magnitude.is_finite() || !phase.is_finite() || !frequency.is_finite() {
                return Err(SpectralPreparationError::Preparation(
                    "forward FFT produced a non-finite spectral value".to_owned(),
                ));
            }
            magnitudes.push(magnitude);
            phases.push(phase);
            instantaneous_frequencies_hz.push(frequency);
            previous_phases[bin] = phase;
        }
    }
    Ok(())
}

pub(crate) fn spectral_hop_size(fft_size: usize) -> Result<usize, SpectralPreparationError> {
    if !SPECTRAL_FFT_SIZES.contains(&fft_size) {
        return Err(SpectralPreparationError::InvalidFftSize(fft_size));
    }
    Ok(fft_size / 4)
}

pub(crate) fn build_analysis_window(fft_size: usize) -> Vec<f32> {
    #[allow(clippy::cast_precision_loss)]
    (0..fft_size)
        .map(|index| 0.5 - 0.5 * (TAU * index as f32 / fft_size as f32).cos())
        .collect()
}

pub(crate) fn build_synthesis_window(fft_size: usize, hop_size: usize) -> Vec<f32> {
    let analysis = build_analysis_window(fft_size);
    let mut overlap_power = vec![0.0_f32; fft_size];
    for shift in (0..fft_size).step_by(hop_size) {
        for (index, value) in analysis.iter().enumerate() {
            overlap_power[(shift + index) % fft_size] += value * value;
        }
    }
    analysis
        .iter()
        .zip(overlap_power)
        .map(
            |(value, power)| {
                if power > 0.0 { value / power } else { 0.0 }
            },
        )
        .collect()
}

fn wrap_phase(value: f32) -> f32 {
    (value + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;
    use std::sync::Arc;

    use approx::assert_relative_eq;
    use realfft::RealFftPlanner;

    use super::{
        SpectralPreparationError, build_analysis_window, build_synthesis_window,
        prepare_spectral_asset, spectral_hop_size,
    };
    use crate::asset::{PreparedAudio, PreparedAudioChannels, SampleMetadata};

    #[test]
    fn supported_fft_sizes_use_quarter_hops() {
        assert_eq!(spectral_hop_size(1024), Ok(256));
        assert_eq!(spectral_hop_size(2048), Ok(512));
        assert_eq!(spectral_hop_size(4096), Ok(1024));
        assert_eq!(
            spectral_hop_size(512),
            Err(SpectralPreparationError::InvalidFftSize(512))
        );
    }

    #[test]
    fn periodic_hann_has_normalized_overlap_add_window() {
        let fft_size = 1024;
        let hop_size = 256;
        let analysis = build_analysis_window(fft_size);
        let synthesis = build_synthesis_window(fft_size, hop_size);
        for position in 0..fft_size {
            let sum = (0..fft_size)
                .step_by(hop_size)
                .map(|shift| {
                    let index = (position + shift) % fft_size;
                    analysis[index] * synthesis[index]
                })
                .sum::<f32>();
            assert!(
                (sum - 1.0).abs() <= 1.0e-5,
                "position={position}, sum={sum}"
            );
        }
    }

    #[test]
    fn instantaneous_frequency_tracks_a_non_bin_center_tone() {
        let sample_rate = 48_000.0;
        let fft_size = 2048;
        let hop_size = fft_size / 4;
        let samples = (0..(fft_size * 4))
            .map(|index| {
                #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                let time = index as f32 / sample_rate as f32;
                (TAU * 440.0 * time).sin()
            })
            .collect::<Vec<_>>();
        let audio = PreparedAudio {
            sample_rate,
            frames: samples.len(),
            source_metadata: SampleMetadata {
                source_sample_rate: 48_000,
                source_channels: 1,
                bits_per_sample: Some(32),
                source_frames: samples.len(),
            },
            channels: PreparedAudioChannels::Mono {
                samples: Arc::from(samples.into_boxed_slice()),
            },
        };
        let prepared = prepare_spectral_asset(&audio, fft_size).expect("spectral preparation");
        let frame = prepared.spectral_frame_count / 2;
        let index = prepared.index(0, frame, 19).expect("bin index");
        let frequency = prepared.instantaneous_frequencies_hz[index];
        assert!((frequency - 440.0).abs() < 5.0, "frequency={frequency}");
        assert_eq!(prepared.hop_size, hop_size);
    }

    #[test]
    fn real_fft_roundtrip_preserves_an_impulse_index() {
        let fft_size = 1024;
        let impulse_index = 123;
        let mut planner = RealFftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(fft_size);
        let inverse = planner.plan_fft_inverse(fft_size);
        let mut input = vec![0.0_f32; fft_size];
        input[impulse_index] = 1.0;
        let original = input.clone();
        let mut spectrum = forward.make_output_vec();
        let mut forward_scratch = forward.make_scratch_vec();
        forward
            .process_with_scratch(&mut input, &mut spectrum, &mut forward_scratch)
            .expect("forward transform");
        let mut output = inverse.make_output_vec();
        let mut inverse_scratch = inverse.make_scratch_vec();
        inverse
            .process_with_scratch(&mut spectrum, &mut output, &mut inverse_scratch)
            .expect("inverse transform");
        for sample in &mut output {
            #[allow(clippy::cast_precision_loss)]
            {
                *sample /= fft_size as f32;
            }
        }
        assert_relative_eq!(output.as_slice(), original.as_slice(), epsilon = 1.0e-6);
    }
}
