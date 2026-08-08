use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction, audioadapter_buffers::direct::SequentialSliceOfVecs,
};
use sha2::{Digest, Sha256};
use symphonia::core::audio::sample::SampleFormat;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, TrackType, probe::Hint};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use thiserror::Error;

use crate::definition::AssetReference;

/// Audio metadata retained with a prepared sample.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleMetadata {
    /// Source sample rate before resampling.
    pub source_sample_rate: u32,
    /// Number of source channels before preparation.
    pub source_channels: usize,
    /// Source bit depth when supplied by the decoder.
    pub bits_per_sample: Option<u32>,
    /// Number of source frames before resampling.
    pub source_frames: usize,
}

/// Planar channels prepared for one engine sample rate.
#[derive(Debug, Clone, PartialEq)]
pub enum PreparedAudioChannels {
    /// One channel of prepared audio.
    Mono {
        /// Prepared mono samples shared by all voices.
        samples: Arc<[f32]>,
    },
    /// Two independent channels of prepared audio.
    Stereo {
        /// Prepared left samples shared by all voices.
        left: Arc<[f32]>,
        /// Prepared right samples shared by all voices.
        right: Arc<[f32]>,
    },
}

/// Immutable audio prepared for one engine sample rate.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedAudio {
    /// Sample rate of the prepared channels.
    pub sample_rate: f64,
    /// Number of prepared frames in every channel.
    pub frames: usize,
    /// Metadata from the source asset.
    pub source_metadata: SampleMetadata,
    /// Mono or stereo planar samples shared by all voices.
    pub channels: PreparedAudioChannels,
}

impl PreparedAudio {
    /// Return the number of prepared channels.
    #[must_use]
    pub fn channel_count(&self) -> usize {
        match &self.channels {
            PreparedAudioChannels::Mono { .. } => 1,
            PreparedAudioChannels::Stereo { .. } => 2,
        }
    }
}

/// Result of resolving and preparing one asset.
#[derive(Debug, Clone)]
pub(crate) struct PreparedAsset {
    /// Prepared audio data.
    pub audio: Arc<PreparedAudio>,
}

/// Asset preparation failure classified for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum AssetError {
    /// The file could not be found or opened.
    #[error("asset not found: {0}")]
    NotFound(String),
    /// The file digest differs from the Definition.
    #[error("asset hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    /// The file format or decoded signal is unsupported.
    #[error("asset decode failed: {0}")]
    Decode(String),
    /// Sample-rate conversion failed.
    #[error("asset resample failed: {0}")]
    Resample(String),
}

/// Immutable mono audio decoded without changing its source sample rate.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MonoAudioAsset {
    pub(crate) sample_rate: u32,
    pub(crate) channels: usize,
    pub(crate) bits_per_sample: Option<u32>,
    pub(crate) source_frames: usize,
    pub(crate) samples: Vec<f32>,
}

pub(crate) fn prepare_asset(
    reference: &AssetReference,
    definition_base_dir: &Path,
    target_sample_rate: f64,
) -> Result<PreparedAsset, AssetError> {
    let decoded = load_audio(reference, definition_base_dir)?;
    let source_metadata = SampleMetadata {
        source_sample_rate: decoded.sample_rate,
        source_channels: decoded.channels,
        bits_per_sample: decoded.bits_per_sample,
        source_frames: decoded.source_frames,
    };
    let channels = prepare_channels(
        &decoded.samples,
        decoded.channels,
        f64::from(decoded.sample_rate),
        target_sample_rate,
    )?;
    let frames = match &channels {
        PreparedAudioChannels::Mono { samples } => samples.len(),
        PreparedAudioChannels::Stereo { left, right } => {
            if left.len() != right.len() {
                return Err(AssetError::Resample(
                    "stereo channels have different prepared frame counts".to_owned(),
                ));
            }
            left.len()
        }
    };
    if frames == 0 {
        return Err(AssetError::Resample(
            "prepared audio contains no frames".to_owned(),
        ));
    }

    Ok(PreparedAsset {
        audio: Arc::new(PreparedAudio {
            sample_rate: target_sample_rate,
            frames,
            source_metadata,
            channels,
        }),
    })
}

pub(crate) fn load_audio(
    reference: &AssetReference,
    definition_base_dir: &Path,
) -> Result<DecodedAudioAsset, AssetError> {
    let resolved_path = resolve_path(definition_base_dir, &reference.path);
    let bytes =
        std::fs::read(&resolved_path).map_err(|error| AssetError::NotFound(error.to_string()))?;
    let digest = Sha256::digest(&bytes);
    let mut actual_hash = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(actual_hash, "{byte:02x}");
    }
    if let Some(expected) = &reference.sha256 {
        if !expected.eq_ignore_ascii_case(&actual_hash) {
            return Err(AssetError::HashMismatch {
                expected: expected.clone(),
                actual: actual_hash,
            });
        }
    }

    let decoded = decode_wav(&resolved_path)?;
    if decoded.channels > 2 {
        return Err(AssetError::Decode(format!(
            "only mono and stereo WAV assets are supported, got {} channels",
            decoded.channels
        )));
    }
    let source_frames = decoded
        .samples
        .len()
        .checked_div(decoded.channels)
        .filter(|frames| *frames > 0)
        .ok_or_else(|| AssetError::Decode("decoded WAV contains no frames".to_owned()))?;
    if decoded.samples.len() != source_frames * decoded.channels {
        return Err(AssetError::Decode(
            "decoded WAV has an incomplete final frame".to_owned(),
        ));
    }
    if !decoded.samples.iter().all(|sample| sample.is_finite()) {
        return Err(AssetError::Decode(
            "decoded WAV contains a non-finite sample".to_owned(),
        ));
    }

    Ok(DecodedAudioAsset {
        sample_rate: decoded.sample_rate,
        channels: decoded.channels,
        bits_per_sample: decoded.bits_per_sample,
        source_frames,
        samples: decoded.samples,
    })
}

pub(crate) fn load_mono_audio(
    reference: &AssetReference,
    definition_base_dir: &Path,
) -> Result<MonoAudioAsset, AssetError> {
    let decoded = load_audio(reference, definition_base_dir)?;
    let samples = downmix(&decoded.samples, decoded.channels)
        .ok_or_else(|| AssetError::Decode("decoded WAV contains no frames".to_owned()))?;

    Ok(MonoAudioAsset {
        sample_rate: decoded.sample_rate,
        channels: decoded.channels,
        bits_per_sample: decoded.bits_per_sample,
        source_frames: decoded.source_frames,
        samples,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DecodedAudioAsset {
    pub(crate) sample_rate: u32,
    pub(crate) channels: usize,
    pub(crate) bits_per_sample: Option<u32>,
    pub(crate) source_frames: usize,
    pub(crate) samples: Vec<f32>,
}

fn prepare_channels(
    interleaved: &[f32],
    channels: usize,
    source_sample_rate: f64,
    target_sample_rate: f64,
) -> Result<PreparedAudioChannels, AssetError> {
    let mut planar = (0..channels)
        .map(|channel| {
            interleaved
                .iter()
                .skip(channel)
                .step_by(channels)
                .copied()
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for channel in &mut planar {
        if (source_sample_rate - target_sample_rate).abs() >= f64::EPSILON {
            *channel = resample(channel, source_sample_rate, target_sample_rate)?;
        }
        if channel.is_empty() || !channel.iter().all(|sample| sample.is_finite()) {
            return Err(AssetError::Resample(
                "prepared audio contains a non-finite or empty channel".to_owned(),
            ));
        }
    }
    match planar.as_mut_slice() {
        [mono] => Ok(PreparedAudioChannels::Mono {
            samples: Arc::from(std::mem::take(mono)),
        }),
        [left, right] => {
            if left.len() != right.len() {
                return Err(AssetError::Resample(
                    "stereo channels have different prepared frame counts".to_owned(),
                ));
            }
            Ok(PreparedAudioChannels::Stereo {
                left: Arc::from(std::mem::take(left)),
                right: Arc::from(std::mem::take(right)),
            })
        }
        _ => Err(AssetError::Decode(
            "only mono and stereo WAV assets are supported".to_owned(),
        )),
    }
}

pub(crate) fn resolved_asset_path(base_dir: &Path, reference: &str) -> PathBuf {
    resolve_path(base_dir, reference)
}

fn resolve_path(base_dir: &Path, reference: &str) -> PathBuf {
    let path = Path::new(reference);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

struct DecodedAudio {
    sample_rate: u32,
    channels: usize,
    bits_per_sample: Option<u32>,
    samples: Vec<f32>,
}

#[allow(clippy::too_many_lines)]
fn decode_wav(path: &Path) -> Result<DecodedAudio, AssetError> {
    let source =
        std::fs::File::open(path).map_err(|error| AssetError::NotFound(error.to_string()))?;
    let stream = MediaSourceStream::new(
        Box::new(source),
        symphonia::core::io::MediaSourceStreamOptions::default(),
    );
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    let probe = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| AssetError::Decode(error.to_string()))?;
    let mut format = probe;
    let (track_id, codec_params) = {
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| AssetError::Decode("WAV has no audio track".to_owned()))?;
        (
            track.id,
            track
                .codec_params
                .clone()
                .ok_or_else(|| AssetError::Decode("WAV codec parameters are missing".to_owned()))?,
        )
    };
    let audio_params = codec_params
        .audio()
        .ok_or_else(|| AssetError::Decode("WAV codec parameters are not audio".to_owned()))?;
    let sample_rate = audio_params
        .sample_rate
        .ok_or_else(|| AssetError::Decode("WAV sample rate is missing".to_owned()))?;
    if sample_rate == 0 {
        return Err(AssetError::Decode(
            "WAV sample rate must be positive".to_owned(),
        ));
    }
    let channels = audio_params
        .channels
        .as_ref()
        .map(symphonia::core::audio::Channels::count)
        .ok_or_else(|| AssetError::Decode("WAV channel layout is missing".to_owned()))?;
    if channels == 0 {
        return Err(AssetError::Decode(
            "WAV must contain at least one channel".to_owned(),
        ));
    }
    if let Some(format) = audio_params.sample_format {
        let supported = matches!(
            format,
            SampleFormat::S16 | SampleFormat::S24 | SampleFormat::F32
        );
        if !supported {
            return Err(AssetError::Decode(format!(
                "unsupported WAV sample format {format:?}"
            )));
        }
    }
    if let Some(bits) = audio_params.bits_per_sample {
        if !matches!(bits, 16 | 24 | 32) {
            return Err(AssetError::Decode(format!(
                "unsupported WAV bit depth {bits}"
            )));
        }
    }

    let mut codec_decoder = symphonia::default::get_codecs()
        .make_audio_decoder(audio_params, &AudioDecoderOptions::default())
        .map_err(|error| AssetError::Decode(error.to_string()))?;
    let mut samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::ResetRequired) => {
                return Err(AssetError::Decode(
                    "WAV decoder requested a reset".to_owned(),
                ));
            }
            Err(error) => return Err(AssetError::Decode(error.to_string())),
        };
        if packet.track_id != track_id {
            continue;
        }
        let packet_samples = codec_decoder
            .decode(&packet)
            .map_err(|error| AssetError::Decode(error.to_string()))?;
        let mut packet_samples_f32 = Vec::new();
        packet_samples.copy_to_vec_interleaved(&mut packet_samples_f32);
        samples.extend_from_slice(&packet_samples_f32);
    }
    Ok(DecodedAudio {
        sample_rate,
        channels,
        bits_per_sample: audio_params.bits_per_sample,
        samples,
    })
}

fn downmix(samples: &[f32], channels: usize) -> Option<Vec<f32>> {
    if channels == 0 || samples.is_empty() || samples.len() % channels != 0 {
        return None;
    }
    if channels == 1 {
        return Some(samples.to_vec());
    }
    if channels == 2 {
        return Some(
            samples
                .chunks_exact(2)
                .map(|frame| (frame[0] + frame[1]) * 0.5)
                .collect(),
        );
    }
    None
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn resample(
    samples: &[f32],
    source_sample_rate: f64,
    target_sample_rate: f64,
) -> Result<Vec<f32>, AssetError> {
    if samples.len() < 4 {
        return Ok(linear_resample(
            samples,
            source_sample_rate,
            target_sample_rate,
        ));
    }
    let ratio = target_sample_rate / source_sample_rate;
    let parameters = SincInterpolationParameters::new(128, WindowFunction::Blackman2)
        .oversampling_factor(128)
        .interpolation(SincInterpolationType::Cubic);
    let expected_len = ((samples.len() as f64 * ratio).round() as usize).max(1);
    let sinc_len = parameters.sinc_len;
    let padding = sinc_len * 2;
    let mut padded = Vec::with_capacity(samples.len() + padding);
    padded.extend_from_slice(samples);
    padded.resize(samples.len() + padding, 0.0);
    let mut resampler =
        Async::<f32>::new_sinc(ratio, 1.0, &parameters, padded.len(), 1, FixedAsync::Input)
            .map_err(|error| AssetError::Resample(error.to_string()))?;
    let input_data = vec![padded];
    let input = SequentialSliceOfVecs::new(&input_data, 1, input_data[0].len())
        .map_err(|error| AssetError::Resample(error.to_string()))?;
    let mut output = resampler
        .process_all(&input, input_data[0].len(), None)
        .map_err(|error| AssetError::Resample(error.to_string()))?
        .take_data();
    output.truncate(expected_len);
    output.resize(expected_len, 0.0);
    Ok(output)
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn linear_resample(samples: &[f32], source_sample_rate: f64, target_sample_rate: f64) -> Vec<f32> {
    let output_len =
        ((samples.len() as f64 * target_sample_rate / source_sample_rate).round() as usize).max(1);
    (0..output_len)
        .map(|index| {
            let position = index as f64 * source_sample_rate / target_sample_rate;
            let left = position.floor() as usize;
            let left = left.min(samples.len() - 1);
            let right = (left + 1).min(samples.len() - 1);
            let fraction = position.fract() as f32;
            samples[left] + fraction * (samples[right] - samples[left])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_pcm16_wav(path: &Path, channels: u16, sample_rate: u32, samples: &[i16]) {
        let payload_len = u32::try_from(samples.len() * 2).expect("test WAV fits RIFF");
        let block_align = channels * 2;
        let byte_rate = sample_rate * u32::from(block_align);
        let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + payload_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&byte_rate.to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&payload_len.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(path, bytes).expect("test WAV writes");
    }

    fn write_pcm24_wav(path: &Path, channels: u16, sample_rate: u32, samples: &[i32]) {
        let payload_len = u32::try_from(samples.len() * 3).expect("test WAV fits RIFF");
        let block_align = channels * 3;
        let byte_rate = sample_rate * u32::from(block_align);
        let mut bytes = Vec::with_capacity(44 + samples.len() * 3);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + payload_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&byte_rate.to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&24_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&payload_len.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes()[..3]);
        }
        std::fs::write(path, bytes).expect("test WAV writes");
    }

    fn write_float32_wav(path: &Path, channels: u16, sample_rate: u32, samples: &[f32]) {
        let payload_len = u32::try_from(samples.len() * 4).expect("test WAV fits RIFF");
        let block_align = channels * 4;
        let byte_rate = sample_rate * u32::from(block_align);
        let mut bytes = Vec::with_capacity(44 + samples.len() * 4);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + payload_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&3_u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&byte_rate.to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&32_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&payload_len.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(path, bytes).expect("test WAV writes");
    }

    fn temporary_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sonalloy-asset-{name}-{}.wav", std::process::id()))
    }

    #[test]
    fn stereo_downmix_averages_each_frame() {
        assert_eq!(downmix(&[1.0, -1.0, 0.5, 0.25], 2), Some(vec![0.0, 0.375]));
    }

    #[test]
    fn linear_resample_keeps_constant_signal_constant() {
        let output = linear_resample(&[0.25; 4], 44_100.0, 48_000.0);
        assert!(!output.is_empty());
        assert!(output.iter().all(|sample| (*sample - 0.25).abs() < 1.0e-6));
    }

    #[test]
    fn sinc_resample_preserves_the_expected_frame_count() {
        let input = vec![0.25; 4_410];
        let output = resample(&input, 44_100.0, 48_000.0).expect("sinc resample succeeds");
        assert_eq!(output.len(), 4_800);
        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn prepare_asset_decodes_pcm_and_preserves_stereo() {
        let path = temporary_path("stereo");
        write_pcm16_wav(&path, 2, 44_100, &[32_767, -32_767, 16_384, 8_192]);
        let result = prepare_asset(
            &AssetReference {
                path: path.to_string_lossy().into_owned(),
                sha256: None,
            },
            Path::new("."),
            48_000.0,
        )
        .expect("stereo PCM asset prepares");
        assert_eq!(result.audio.source_metadata.source_channels, 2);
        assert_eq!(result.audio.source_metadata.bits_per_sample, Some(16));
        assert_eq!(result.audio.frames, 2);
        assert!(matches!(
            &result.audio.channels,
            PreparedAudioChannels::Stereo { .. }
        ));
        if let PreparedAudioChannels::Stereo { left, right } = &result.audio.channels {
            assert!(
                left.iter()
                    .chain(right.iter())
                    .all(|sample| sample.is_finite())
            );
            assert!((left[0] - right[0]).abs() > 0.5);
        }
        std::fs::remove_file(path).expect("test WAV removes");
    }

    #[test]
    fn prepare_asset_rejects_more_than_two_channels() {
        let path = temporary_path("three-channel");
        write_pcm16_wav(&path, 3, 48_000, &[0, 1, 2, 3, 4, 5]);
        let error = prepare_asset(
            &AssetReference {
                path: path.to_string_lossy().into_owned(),
                sha256: None,
            },
            Path::new("."),
            48_000.0,
        )
        .expect_err("three-channel asset must fail");
        assert!(matches!(error, AssetError::Decode(_)));
        std::fs::remove_file(path).expect("test WAV removes");
    }

    #[test]
    fn prepare_asset_decodes_pcm24_and_float32() {
        let pcm24_path = temporary_path("pcm24");
        write_pcm24_wav(&pcm24_path, 1, 48_000, &[0x0012_3456, -0x0012_3456, 0]);
        let pcm24 = prepare_asset(
            &AssetReference {
                path: pcm24_path.to_string_lossy().into_owned(),
                sha256: None,
            },
            Path::new("."),
            48_000.0,
        )
        .expect("24-bit PCM asset prepares");
        assert_eq!(pcm24.audio.source_metadata.bits_per_sample, Some(24));
        assert_eq!(pcm24.audio.source_metadata.source_frames, 3);
        assert!(matches!(
            &pcm24.audio.channels,
            PreparedAudioChannels::Mono { .. }
        ));
        if let PreparedAudioChannels::Mono { samples } = &pcm24.audio.channels {
            assert!(samples.iter().all(|sample| sample.is_finite()));
        }
        std::fs::remove_file(pcm24_path).expect("test WAV removes");

        let float32_path = temporary_path("float32");
        write_float32_wav(&float32_path, 1, 48_000, &[0.25, -0.5, 0.75]);
        let float32 = prepare_asset(
            &AssetReference {
                path: float32_path.to_string_lossy().into_owned(),
                sha256: None,
            },
            Path::new("."),
            48_000.0,
        )
        .expect("32-bit float asset prepares");
        assert_eq!(float32.audio.source_metadata.source_frames, 3);
        if let PreparedAudioChannels::Mono { samples } = &float32.audio.channels {
            assert!(samples.iter().all(|sample| sample.is_finite()));
        }
        std::fs::remove_file(float32_path).expect("test WAV removes");
    }

    #[test]
    fn prepare_asset_resamples_96khz_to_the_engine_rate() {
        let path = temporary_path("96khz");
        let samples = (0..960)
            .map(|index| if index % 2 == 0 { 16_384 } else { -16_384 })
            .collect::<Vec<_>>();
        write_pcm16_wav(&path, 1, 96_000, &samples);
        let result = prepare_asset(
            &AssetReference {
                path: path.to_string_lossy().into_owned(),
                sha256: None,
            },
            Path::new("."),
            48_000.0,
        )
        .expect("96 kHz asset prepares");
        assert_eq!(result.audio.source_metadata.source_sample_rate, 96_000);
        assert_eq!(result.audio.frames, 480);
        if let PreparedAudioChannels::Mono { samples } = &result.audio.channels {
            assert!(samples.iter().all(|sample| sample.is_finite()));
        }
        std::fs::remove_file(path).expect("test WAV removes");
    }

    #[test]
    fn prepare_asset_rejects_non_finite_float_samples() {
        let path = temporary_path("nan");
        write_float32_wav(&path, 1, 48_000, &[0.0, f32::NAN, 0.0]);
        let error = prepare_asset(
            &AssetReference {
                path: path.to_string_lossy().into_owned(),
                sha256: None,
            },
            Path::new("."),
            48_000.0,
        )
        .expect_err("non-finite sample must fail");
        assert!(matches!(error, AssetError::Decode(_)));
        std::fs::remove_file(path).expect("test WAV removes");
    }

    #[test]
    fn prepare_asset_rejects_a_hash_mismatch() {
        let path = temporary_path("hash");
        write_pcm16_wav(&path, 1, 48_000, &[0, 1, -1, 0]);
        let error = prepare_asset(
            &AssetReference {
                path: path.to_string_lossy().into_owned(),
                sha256: Some("00".repeat(32)),
            },
            Path::new("."),
            48_000.0,
        )
        .expect_err("wrong hash must fail");
        assert!(matches!(error, AssetError::HashMismatch { .. }));
        std::fs::remove_file(path).expect("test WAV removes");
    }
}
