use realfft::RealFftPlanner;
use serde::Serialize;
use thiserror::Error;

use crate::render::RenderedAudio;

const ACTIVITY_THRESHOLD_DBFS: f32 = -80.0;
const LARGE_DELTA_THRESHOLD: f32 = 0.25;
const MAX_REPORTED_PEAKS: usize = 8;

/// Options controlling deterministic render analysis.
#[derive(Debug, Clone, Copy, Default)]
pub struct AudioAnalysisOptions {
    /// Optional known frequency used for a harmonic-energy measurement.
    pub reference_frequency_hz: Option<f32>,
}

/// Error returned when rendered audio cannot be analyzed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AudioAnalysisError {
    /// The rendered sample rate is zero.
    #[error("audio sample rate must be greater than zero")]
    InvalidSampleRate,
    /// The rendered audio has no channel data.
    #[error("audio must contain at least one channel")]
    NoChannels,
    /// Channels do not contain the same number of frames.
    #[error("audio channels must contain the same number of frames")]
    UnequalChannelLengths,
    /// The harmonic reference is not a positive finite frequency.
    #[error("reference frequency must be finite and greater than zero")]
    InvalidReferenceFrequency,
}

/// Deterministic facts measured from one rendered audio buffer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AudioAnalysis {
    /// Output sample rate.
    pub sample_rate: u32,
    /// Number of output channels.
    pub channels: usize,
    /// Number of rendered frames.
    pub frames: usize,
    /// Duration in seconds.
    pub duration_seconds: f64,
    /// Whether every input sample was finite.
    pub finite: bool,
    /// Aggregate level metrics.
    pub level: LevelAnalysis,
    /// Arithmetic mean for each channel.
    pub dc: Vec<f32>,
    /// Threshold-based activity locations.
    pub activity: ActivityAnalysis,
    /// Adjacent-frame discontinuity metrics.
    pub continuity: ContinuityAnalysis,
    /// Zero-mean stereo correlation when defined.
    pub stereo: StereoAnalysis,
    /// Deterministic Hann-windowed spectrum summary.
    pub spectrum: SpectrumAnalysis,
}

/// Aggregate level facts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LevelAnalysis {
    /// Maximum absolute sample over all channels.
    pub peak: f32,
    /// Peak in dBFS, or null for digital silence.
    pub peak_dbfs: Option<f32>,
    /// Root mean square over all samples.
    pub rms: f32,
    /// RMS in dBFS, or null for digital silence.
    pub rms_dbfs: Option<f32>,
    /// Peak-to-RMS ratio in dB, or null when either value is zero.
    pub crest_factor_db: Option<f32>,
    /// Whether the floating-point signal exceeds full scale.
    pub over_full_scale: bool,
}

/// Threshold-based signal activity facts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActivityAnalysis {
    /// Fixed activity threshold in dBFS.
    pub threshold_dbfs: f32,
    /// First active frame, or null for silence.
    pub first_frame: Option<usize>,
    /// Overall peak frame, or null for silence.
    pub peak_frame: Option<usize>,
    /// Last active frame, or null for silence.
    pub last_frame: Option<usize>,
}

/// Adjacent-frame continuity facts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ContinuityAnalysis {
    /// Fixed large-delta threshold.
    pub large_delta_threshold: f32,
    /// Greatest adjacent-frame delta across channels.
    pub max_adjacent_frame_delta: f32,
    /// Number of frame boundaries above the threshold.
    pub large_delta_count: usize,
    /// First up to sixteen candidate frame indices.
    pub first_large_delta_frames: Vec<usize>,
}

/// Stereo relationship facts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StereoAnalysis {
    /// Pearson-style zero-mean correlation, or null when undefined.
    pub correlation: Option<f32>,
}

/// Spectrum and known-reference facts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpectrumAnalysis {
    /// FFT size used for the summary, or zero when no FFT can be formed.
    pub fft_size: usize,
    /// Hop size used between windows, or zero when no FFT can be formed.
    pub hop_size: usize,
    /// Power-weighted spectral centroid.
    pub spectral_centroid_hz: Option<f32>,
    /// Strongest local spectral peaks.
    pub peaks: Vec<SpectralPeak>,
    /// Known frequency supplied by the render command.
    pub reference_frequency_hz: Option<f32>,
    /// Energy near integer multiples of the known frequency.
    pub harmonic_energy_ratio: Option<f32>,
}

/// One local maximum in the averaged spectrum.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpectralPeak {
    /// Center frequency of the FFT bin.
    pub frequency_hz: f32,
    /// Power relative to the strongest reported peak.
    pub relative_power: f32,
}

/// Analyze a rendered buffer without reading or writing a file.
///
/// # Errors
///
/// Returns an error when the buffer shape or requested harmonic reference is invalid.
#[allow(clippy::too_many_lines)]
pub fn analyze_rendered_audio(
    audio: &RenderedAudio,
    options: AudioAnalysisOptions,
) -> Result<AudioAnalysis, AudioAnalysisError> {
    if audio.sample_rate == 0 {
        return Err(AudioAnalysisError::InvalidSampleRate);
    }
    if audio.channels.is_empty() {
        return Err(AudioAnalysisError::NoChannels);
    }
    let frames = audio.channels[0].len();
    if audio.channels.iter().any(|channel| channel.len() != frames) {
        return Err(AudioAnalysisError::UnequalChannelLengths);
    }
    if let Some(reference) = options.reference_frequency_hz
        && (!reference.is_finite() || reference <= 0.0)
    {
        return Err(AudioAnalysisError::InvalidReferenceFrequency);
    }

    let mut finite = true;
    let mut peak = 0.0_f32;
    let mut sum_squares = 0.0_f64;
    let mut sample_count = 0_usize;
    let mut peak_frame = None;
    let mut peak_frame_value = 0.0_f32;
    for frame in 0..frames {
        let mut frame_peak = 0.0_f32;
        for channel in &audio.channels {
            let sample = channel[frame];
            if !sample.is_finite() {
                finite = false;
                continue;
            }
            let magnitude = sample.abs();
            frame_peak = frame_peak.max(magnitude);
            peak = peak.max(magnitude);
            sum_squares += f64::from(sample) * f64::from(sample);
            sample_count += 1;
        }
        if frame_peak > peak_frame_value {
            peak_frame_value = frame_peak;
            peak_frame = Some(frame);
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let rms = if sample_count == 0 {
        0.0
    } else {
        (sum_squares / sample_count as f64).sqrt() as f32
    };
    let peak_dbfs = dbfs(peak);
    let rms_dbfs = dbfs(rms);
    let crest_factor_db = match (peak_dbfs, rms_dbfs) {
        (Some(peak), Some(rms)) => Some(peak - rms),
        _ => None,
    };
    let activity_threshold = 10.0_f32.powf(ACTIVITY_THRESHOLD_DBFS / 20.0);
    let mut first_activity = None;
    let mut last_activity = None;
    for frame in 0..frames {
        let active = audio
            .channels
            .iter()
            .filter_map(|channel| channel.get(frame).copied())
            .filter(|sample| sample.is_finite())
            .map(f32::abs)
            .fold(0.0, f32::max)
            >= activity_threshold;
        if active {
            first_activity.get_or_insert(frame);
            last_activity = Some(frame);
        }
    }

    let dc = audio
        .channels
        .iter()
        .map(|channel| {
            if channel.is_empty() {
                0.0
            } else {
                let (sum, count) =
                    channel
                        .iter()
                        .fold((0.0_f64, 0_usize), |(sum, count), sample| {
                            if sample.is_finite() {
                                (sum + f64::from(*sample), count + 1)
                            } else {
                                (sum, count)
                            }
                        });
                if count == 0 {
                    0.0
                } else {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
                    {
                        (sum / count as f64) as f32
                    }
                }
            }
        })
        .collect();
    let continuity = continuity(audio);
    let stereo = stereo_correlation(audio);
    let spectrum = spectrum(audio, options.reference_frequency_hz);

    Ok(AudioAnalysis {
        sample_rate: audio.sample_rate,
        channels: audio.channels.len(),
        frames,
        #[allow(clippy::cast_precision_loss)]
        duration_seconds: frames as f64 / f64::from(audio.sample_rate),
        finite,
        level: LevelAnalysis {
            peak,
            peak_dbfs,
            rms,
            rms_dbfs,
            crest_factor_db,
            over_full_scale: peak > 1.0,
        },
        dc,
        activity: ActivityAnalysis {
            threshold_dbfs: ACTIVITY_THRESHOLD_DBFS,
            first_frame: first_activity,
            peak_frame,
            last_frame: last_activity,
        },
        continuity,
        stereo,
        spectrum,
    })
}

fn dbfs(value: f32) -> Option<f32> {
    (value > 0.0 && value.is_finite()).then(|| 20.0 * value.log10())
}

fn continuity(audio: &RenderedAudio) -> ContinuityAnalysis {
    let mut max_delta = 0.0_f32;
    let mut large_delta_count = 0;
    let mut first_large_delta_frames = Vec::new();
    for frame in 1..audio.frames() {
        let delta = audio
            .channels
            .iter()
            .map(|channel| (channel[frame] - channel[frame - 1]).abs())
            .filter(|delta| delta.is_finite())
            .fold(0.0, f32::max);
        max_delta = max_delta.max(delta);
        if delta > LARGE_DELTA_THRESHOLD {
            large_delta_count += 1;
            if first_large_delta_frames.len() < 16 {
                first_large_delta_frames.push(frame);
            }
        }
    }
    ContinuityAnalysis {
        large_delta_threshold: LARGE_DELTA_THRESHOLD,
        max_adjacent_frame_delta: max_delta,
        large_delta_count,
        first_large_delta_frames,
    }
}

fn stereo_correlation(audio: &RenderedAudio) -> StereoAnalysis {
    let Some((left, right)) = audio.channels.first().zip(audio.channels.get(1)) else {
        return StereoAnalysis { correlation: None };
    };
    if left.is_empty() {
        return StereoAnalysis { correlation: None };
    }
    let (left_mean, right_mean) = {
        #[allow(clippy::cast_precision_loss)]
        (
            left.iter().copied().sum::<f32>() / left.len() as f32,
            right.iter().copied().sum::<f32>() / right.len() as f32,
        )
    };
    let (mut numerator, mut left_energy, mut right_energy) = (0.0_f64, 0.0_f64, 0.0_f64);
    for (&left_sample, &right_sample) in left.iter().zip(right) {
        if !left_sample.is_finite() || !right_sample.is_finite() {
            continue;
        }
        let left_delta = f64::from(left_sample - left_mean);
        let right_delta = f64::from(right_sample - right_mean);
        numerator += left_delta * right_delta;
        left_energy += left_delta * left_delta;
        right_energy += right_delta * right_delta;
    }
    let denominator = (left_energy * right_energy).sqrt();
    #[allow(clippy::cast_possible_truncation)]
    let correlation = (denominator > 0.0).then(|| (numerator / denominator) as f32);
    StereoAnalysis { correlation }
}

#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
fn spectrum(audio: &RenderedAudio, reference_frequency_hz: Option<f32>) -> SpectrumAnalysis {
    let frames = audio.frames();
    let Some(fft_size) = largest_power_of_two(frames).filter(|size| *size >= 2) else {
        return SpectrumAnalysis {
            fft_size: 0,
            hop_size: 0,
            spectral_centroid_hz: None,
            peaks: Vec::new(),
            reference_frequency_hz,
            harmonic_energy_ratio: None,
        };
    };
    let fft_size = fft_size.min(4096);
    let hop_size = fft_size / 2;
    let mono = (0..frames)
        .map(|frame| {
            let (sum, count) =
                audio
                    .channels
                    .iter()
                    .fold((0.0_f32, 0_u32), |(sum, count), channel| {
                        let sample = channel[frame];
                        if sample.is_finite() {
                            (sum + sample, count + 1)
                        } else {
                            (sum, count)
                        }
                    });
            if count == 0 { 0.0 } else { sum / count as f32 }
        })
        .collect::<Vec<_>>();
    let mut planner = RealFftPlanner::<f32>::new();
    let plan = planner.plan_fft_forward(fft_size);
    let mut input = plan.make_input_vec();
    let mut output = plan.make_output_vec();
    let mut power = vec![0.0_f32; output.len()];
    let mut windows = 0_usize;
    let mut start = 0_usize;
    while start + fft_size <= frames {
        for (index, sample) in input.iter_mut().enumerate() {
            let angle = std::f32::consts::TAU * index as f32 / fft_size as f32;
            let window = 0.5 - 0.5 * angle.cos();
            *sample = mono[start + index] * window;
        }
        if plan.process(&mut input, &mut output).is_err() {
            return SpectrumAnalysis {
                fft_size,
                hop_size,
                spectral_centroid_hz: None,
                peaks: Vec::new(),
                reference_frequency_hz,
                harmonic_energy_ratio: None,
            };
        }
        for (bin, value) in output.iter().enumerate() {
            power[bin] += value.norm_sqr();
        }
        windows += 1;
        start += hop_size;
    }
    if windows == 0 {
        return SpectrumAnalysis {
            fft_size,
            hop_size,
            spectral_centroid_hz: None,
            peaks: Vec::new(),
            reference_frequency_hz,
            harmonic_energy_ratio: None,
        };
    }
    let sample_rate = audio.sample_rate as f32;
    let bin_hz = sample_rate / fft_size as f32;
    let positive_bins = 1..output.len();
    let total_power: f32 = positive_bins.clone().map(|bin| power[bin]).sum();
    let centroid = (total_power > 0.0).then(|| {
        positive_bins
            .clone()
            .map(|bin| power[bin] * bin as f32 * bin_hz)
            .sum::<f32>()
            / total_power
    });
    let mut candidates = (1..output.len().saturating_sub(1))
        .filter(|&bin| power[bin] >= power[bin - 1] && power[bin] >= power[bin + 1])
        .collect::<Vec<_>>();
    if candidates.is_empty() && total_power > 0.0 {
        if let Some((bin, _)) = power
            .iter()
            .enumerate()
            .skip(1)
            .max_by(|left, right| left.1.total_cmp(right.1))
        {
            candidates.push(bin);
        }
    }
    candidates.sort_by(|left, right| power[*right].total_cmp(&power[*left]));
    candidates.truncate(MAX_REPORTED_PEAKS);
    let strongest = candidates
        .first()
        .map(|bin| power[*bin])
        .filter(|power| *power > 0.0);
    let peaks = candidates
        .iter()
        .filter_map(|bin| {
            strongest.map(|strongest| SpectralPeak {
                frequency_hz: *bin as f32 * bin_hz,
                relative_power: power[*bin] / strongest,
            })
        })
        .collect();
    let harmonic_energy_ratio = reference_frequency_hz.and_then(|reference| {
        if total_power <= 0.0 {
            return None;
        }
        let tolerance_hz = bin_hz.max(reference * 0.01);
        let mut harmonic_power = 0.0_f32;
        let mut harmonic = 1_u32;
        while {
            #[allow(clippy::cast_precision_loss)]
            let harmonic_value = harmonic as f32;
            harmonic_value * reference < sample_rate * 0.5
        } {
            #[allow(clippy::cast_precision_loss)]
            let center = harmonic as f32 * reference;
            for (bin, value) in power.iter().enumerate().skip(1) {
                let frequency = bin as f32 * bin_hz;
                if (frequency - center).abs() <= tolerance_hz {
                    harmonic_power += *value;
                }
            }
            harmonic = harmonic.saturating_add(1);
        }
        Some((harmonic_power / total_power).min(1.0))
    });
    SpectrumAnalysis {
        fft_size,
        hop_size,
        spectral_centroid_hz: centroid,
        peaks,
        reference_frequency_hz,
        harmonic_energy_ratio,
    }
}

fn largest_power_of_two(frames: usize) -> Option<usize> {
    (frames > 0)
        .then(|| usize::BITS as usize - frames.leading_zeros() as usize - 1)
        .map(|power| 1_usize << power)
}

#[cfg(test)]
mod tests {
    use super::{AudioAnalysisOptions, RenderedAudio, analyze_rendered_audio};

    fn audio(left: Vec<f32>, right: Vec<f32>) -> RenderedAudio {
        RenderedAudio {
            sample_rate: 48_000,
            channels: vec![left, right],
        }
    }

    #[test]
    fn silence_reports_zero_levels_and_no_activity() {
        let report = analyze_rendered_audio(
            &audio(vec![0.0; 32], vec![0.0; 32]),
            AudioAnalysisOptions::default(),
        )
        .expect("silence analysis");

        assert!(report.finite);
        assert!(report.level.peak.abs() < f32::EPSILON);
        assert!(report.level.rms.abs() < f32::EPSILON);
        assert_eq!(report.level.peak_dbfs, None);
        assert_eq!(report.level.rms_dbfs, None);
        assert_eq!(report.activity.first_frame, None);
        assert_eq!(report.activity.peak_frame, None);
        assert_eq!(report.stereo.correlation, None);
        assert_eq!(report.spectrum.fft_size, 32);
        assert_eq!(report.spectrum.hop_size, 16);
    }

    #[test]
    fn dc_and_continuity_metrics_use_channel_native_values() {
        let mut left = vec![0.25; 16];
        let right = left.clone();
        left[8] = 0.75;
        let report = analyze_rendered_audio(&audio(left, right), AudioAnalysisOptions::default())
            .expect("dc analysis");

        assert!((report.dc[0] - 0.281_25).abs() < 1.0e-6);
        assert!((report.dc[1] - 0.25).abs() < 1.0e-6);
        assert_eq!(report.continuity.large_delta_count, 2);
        assert_eq!(report.continuity.first_large_delta_frames, vec![8, 9]);
        assert_eq!(report.stereo.correlation, None);
    }

    #[test]
    fn sine_analysis_reports_level_and_reference_spectrum() {
        let frames = 8_192;
        let frequency = 440.0_f32;
        let left = (0..frames)
            .map(|frame| {
                #[allow(clippy::cast_precision_loss)]
                let phase = std::f32::consts::TAU * frequency * frame as f32 / 48_000.0;
                0.5 * phase.sin()
            })
            .collect::<Vec<_>>();
        let report = analyze_rendered_audio(
            &audio(left.clone(), left),
            AudioAnalysisOptions {
                reference_frequency_hz: Some(frequency),
            },
        )
        .expect("sine analysis");

        assert!(report.finite);
        assert!((report.level.peak - 0.5).abs() < 0.001);
        assert!((report.level.rms - 0.5 / 2.0_f32.sqrt()).abs() < 0.002);
        assert!((report.dc[0]).abs() < 0.002);
        assert!((report.stereo.correlation.expect("identical sine") - 1.0).abs() < 1.0e-6);
        assert_eq!(report.spectrum.reference_frequency_hz, Some(frequency));
        assert!(report.spectrum.spectral_centroid_hz.expect("centroid") > 420.0);
        assert!(
            report
                .spectrum
                .peaks
                .iter()
                .any(|peak| (peak.frequency_hz - frequency).abs() < 20.0)
        );
        assert!(
            report
                .spectrum
                .harmonic_energy_ratio
                .expect("harmonic ratio")
                > 0.9
        );
    }
}
