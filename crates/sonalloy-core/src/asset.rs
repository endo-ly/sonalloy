use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction, audioadapter_buffers::direct::SequentialSliceOfVecs,
};
use sha2::{Digest, Sha256};
use symphonia::core::audio::sample::SampleFormat;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType, probe::Hint};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use thiserror::Error;

use crate::definition::{AssetReference, SampleNormalizationMeasure};

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

/// Failure while decoding or preparing an external audio file.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AudioFileError {
    /// The file could not be decoded or resampled.
    #[error("audio file preparation failed: {0}")]
    Preparation(String),
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

pub(crate) fn measure_audio_region(
    audio: &PreparedAudio,
    start_frame: usize,
    end_frame: usize,
    measure: SampleNormalizationMeasure,
) -> Option<f64> {
    if start_frame >= end_frame || end_frame > audio.frames {
        return None;
    }
    let (first, second) = match &audio.channels {
        PreparedAudioChannels::Mono { samples } => (samples.as_ref(), None),
        PreparedAudioChannels::Stereo { left, right } => (left.as_ref(), Some(right.as_ref())),
    };
    if first.len() < end_frame || second.is_some_and(|channel| channel.len() < end_frame) {
        return None;
    }
    let first = &first[start_frame..end_frame];
    let second = second.map(|channel| &channel[start_frame..end_frame]);
    let peak = first
        .iter()
        .chain(second.into_iter().flatten())
        .map(|sample| f64::from(sample.abs()))
        .fold(0.0_f64, f64::max);
    if !peak.is_finite() || peak == 0.0 {
        return None;
    }
    if measure == SampleNormalizationMeasure::Peak {
        return Some(peak);
    }

    let active_threshold = peak * 0.01;
    let mut square_sum = 0.0;
    let mut sample_count = 0_usize;
    for frame in 0..first.len() {
        let first_value = f64::from(first[frame]);
        let second_value = second.map_or(0.0, |channel| f64::from(channel[frame]));
        if first_value.abs().max(second_value.abs()) >= active_threshold {
            square_sum += first_value * first_value + second_value * second_value;
            sample_count += if second.is_some() { 2 } else { 1 };
        }
    }
    if sample_count == 0 || !square_sum.is_finite() {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let rms = (square_sum / sample_count as f64).sqrt();
    (rms.is_finite() && rms > 0.0).then_some(rms)
}

/// Decode and prepare an external WAV file for one engine sample rate.
///
/// External render inputs do not use Definition asset hash semantics.
///
/// # Errors
///
/// Returns an error when the file cannot be decoded, has unsupported channels, or cannot be
/// resampled to the requested sample rate.
pub fn prepare_audio_file(
    path: &Path,
    target_sample_rate: f64,
) -> Result<PreparedAudio, AudioFileError> {
    let reference = AssetReference {
        path: path.to_string_lossy().into_owned(),
        sha256: None,
    };
    let prepared = prepare_asset(&reference, Path::new(""), target_sample_rate)
        .map_err(|error| AudioFileError::Preparation(error.to_string()))?;
    Ok((*prepared.audio).clone())
}

/// Source WAV properties obtained without decoding audio packets.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AudioFileMetadata {
    /// Original sample rate.
    pub sample_rate: u32,
    /// Original channel count.
    pub channels: usize,
    /// Bit depth reported by the container.
    pub bits_per_sample: Option<u32>,
    /// Exact frame count when supplied by the container.
    pub frames: Option<u64>,
    /// Duration derived from the exact frame count, when available.
    pub duration_seconds: Option<f64>,
}

/// Read WAV metadata without decoding samples or computing a file hash.
///
/// # Errors
/// Returns an error for unreadable files or unsupported WAV encodings.
pub fn read_audio_file_metadata(path: &Path) -> Result<AudioFileMetadata, AudioFileError> {
    open_wav(path)
        .map(|source| source.metadata)
        .map_err(|error| AudioFileError::Preparation(error.to_string()))
}

/// Decode a WAV at its original sample rate and channel layout.
///
/// # Errors
/// Returns an error for unreadable, unsupported, empty, or non-finite WAV data.
pub fn read_audio_file(path: &Path) -> Result<crate::RenderedAudio, AudioFileError> {
    let decoded =
        decode_wav(path).map_err(|error| AudioFileError::Preparation(error.to_string()))?;
    Ok(crate::RenderedAudio {
        sample_rate: decoded.sample_rate,
        channels: (0..decoded.channels)
            .map(|channel| {
                decoded
                    .samples
                    .iter()
                    .skip(channel)
                    .step_by(decoded.channels)
                    .copied()
                    .collect()
            })
            .collect(),
    })
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
    let source_frames = decoded.samples.len() / decoded.channels;

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
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    };
    #[cfg(windows)]
    {
        std::fs::canonicalize(&resolved).unwrap_or(resolved)
    }
    #[cfg(not(windows))]
    {
        resolved
    }
}

struct DecodedAudio {
    sample_rate: u32,
    channels: usize,
    bits_per_sample: Option<u32>,
    samples: Vec<f32>,
}

struct WavSource {
    format: Box<dyn FormatReader>,
    track_id: u32,
    parameters: AudioCodecParameters,
    metadata: AudioFileMetadata,
}

#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
fn open_wav(path: &Path) -> Result<WavSource, AssetError> {
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
    let format = probe;
    let (track_id, codec_params, frames) = {
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| AssetError::Decode("WAV has no audio track".to_owned()))?;
        (
            track.id,
            track
                .codec_params
                .clone()
                .ok_or_else(|| AssetError::Decode("WAV codec parameters are missing".to_owned()))?,
            track.num_frames,
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

    Ok(WavSource {
        format,
        track_id,
        parameters: audio_params.clone(),
        metadata: AudioFileMetadata {
            sample_rate,
            channels,
            bits_per_sample: audio_params.bits_per_sample,
            frames,
            duration_seconds: frames.map(|frames| frames as f64 / f64::from(sample_rate)),
        },
    })
}

fn decode_wav(path: &Path) -> Result<DecodedAudio, AssetError> {
    let WavSource {
        mut format,
        track_id,
        parameters,
        metadata,
    } = open_wav(path)?;
    let audio_params = &parameters;
    let sample_rate = metadata.sample_rate;
    let channels = metadata.channels;
    let mut codec_decoder = symphonia::default::get_codecs()
        .make_audio_decoder(audio_params, &AudioDecoderOptions::default())
        .map_err(|error| AssetError::Decode(error.to_string()))?;
    let mut samples = Vec::<f32>::new();
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
    let source_frames = samples
        .len()
        .checked_div(channels)
        .filter(|frames| *frames > 0)
        .ok_or_else(|| AssetError::Decode("decoded WAV contains no frames".to_owned()))?;
    if samples.len() != source_frames * channels {
        return Err(AssetError::Decode(
            "decoded WAV has an incomplete final frame".to_owned(),
        ));
    }
    if !samples.iter().all(|sample| sample.is_finite()) {
        return Err(AssetError::Decode(
            "decoded WAV contains a non-finite sample".to_owned(),
        ));
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
                .map(|frame| f32::midpoint(frame[0], frame[1]))
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
    use std::path::Path;

    use super::{
        AssetError, PreparedAudio, PreparedAudioChannels, SampleMetadata, downmix, linear_resample,
        measure_audio_region, prepare_asset, resample,
    };
    #[cfg(windows)]
    use super::{load_audio, resolve_path};
    use crate::definition::{AssetReference, SampleNormalizationMeasure};

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

    fn fixture_directory() -> tempfile::TempDir {
        tempfile::tempdir().expect("fixture directory creates")
    }

    #[test]
    fn stereo_downmix_averages_each_frame() {
        assert_eq!(downmix(&[1.0, -1.0, 0.5, 0.25], 2), Some(vec![0.0, 0.375]));
    }

    #[test]
    fn prepared_audio_measures_region_peak_and_active_stereo_rms() {
        let audio = PreparedAudio {
            sample_rate: 48_000.0,
            frames: 4,
            source_metadata: SampleMetadata {
                source_sample_rate: 48_000,
                source_channels: 2,
                bits_per_sample: Some(16),
                source_frames: 4,
            },
            channels: PreparedAudioChannels::Stereo {
                left: std::sync::Arc::from([0.5, 0.001, 0.25, 0.0]),
                right: std::sync::Arc::from([-0.5, 0.0, 0.0, 0.0]),
            },
        };

        assert_eq!(
            measure_audio_region(&audio, 1, 3, SampleNormalizationMeasure::Peak),
            Some(0.25)
        );
        assert_eq!(
            measure_audio_region(&audio, 0, 4, SampleNormalizationMeasure::RmsActive),
            Some(0.375)
        );
        assert_eq!(
            measure_audio_region(&audio, 0, 4, SampleNormalizationMeasure::Peak),
            Some(0.5)
        );
        let silence = PreparedAudio {
            channels: PreparedAudioChannels::Stereo {
                left: std::sync::Arc::from([0.0; 4]),
                right: std::sync::Arc::from([0.0; 4]),
            },
            ..audio
        };
        assert_eq!(
            measure_audio_region(&silence, 0, 4, SampleNormalizationMeasure::Peak),
            None
        );
        assert_eq!(
            measure_audio_region(&silence, 0, 4, SampleNormalizationMeasure::RmsActive),
            None
        );
    }

    #[test]
    fn linear_resample_keeps_constant_signal_constant() {
        let output = linear_resample(&[0.25; 4], 44_100.0, 48_000.0);
        assert_ne!(output, [] as [f32; 0]);
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
        let directory = fixture_directory();
        let path = directory.path().join("stereo.wav");
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
    }

    #[test]
    fn prepare_asset_rejects_more_than_two_channels() {
        let directory = fixture_directory();
        let path = directory.path().join("three-channel.wav");
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
    }

    #[test]
    fn prepare_asset_decodes_pcm24_and_float32() {
        let directory = fixture_directory();
        let pcm24_path = directory.path().join("pcm24.wav");
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

        let float32_path = directory.path().join("float32.wav");
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
    }

    #[test]
    fn prepare_asset_resamples_96khz_to_the_engine_rate() {
        let directory = fixture_directory();
        let path = directory.path().join("96khz.wav");
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
    }

    #[test]
    fn prepare_asset_rejects_non_finite_float_samples() {
        let directory = fixture_directory();
        let path = directory.path().join("nan.wav");
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
    }

    #[test]
    fn prepare_asset_rejects_a_hash_mismatch() {
        let directory = fixture_directory();
        let path = directory.path().join("hash.wav");
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
    }

    #[cfg(windows)]
    #[test]
    fn long_windows_sample_paths_load_from_relative_and_extended_forms() {
        let directory = fixture_directory();
        let canonical_root = std::fs::canonicalize(directory.path()).expect("temp path resolves");
        let root_text = canonical_root.to_string_lossy();
        let root_without_prefix = Path::new(
            root_text
                .strip_prefix(r"\\?\")
                .expect("canonical Windows path has the extended prefix"),
        )
        .to_path_buf();
        let mut relative = std::path::PathBuf::new();
        for index in 0..7 {
            relative.push(format!("segment-{index}-{}", "x".repeat(32)));
        }
        let filename = relative.join("long-sample.wav");
        let extended_path = canonical_root.join(&filename);
        std::fs::create_dir_all(extended_path.parent().expect("file parent exists"))
            .expect("long path directories create");
        write_pcm16_wav(&extended_path, 1, 48_000, &[16_384, -16_384, 8_192, -8_192]);
        let unprefixed_path = root_without_prefix.join(&filename);
        assert!(unprefixed_path.to_string_lossy().len() > 260);

        let relative_audio = load_audio(
            &AssetReference {
                path: filename.to_string_lossy().into_owned(),
                sha256: None,
            },
            &root_without_prefix,
        )
        .expect("long relative path loads");
        let extended_audio = load_audio(
            &AssetReference {
                path: extended_path.to_string_lossy().into_owned(),
                sha256: None,
            },
            Path::new("."),
        )
        .expect("extended path loads");
        assert_eq!(relative_audio.samples, extended_audio.samples);
        assert!(
            resolve_path(Path::new("."), &extended_path.to_string_lossy())
                .to_string_lossy()
                .starts_with(r"\\?\")
        );
    }
}
