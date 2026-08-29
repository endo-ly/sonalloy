use rustfft::{FftPlanner, num_complex::Complex};

use crate::asset::{PreparedAudio, PreparedAudioChannels};

pub(crate) const CONVOLUTION_PARTITION_SIZE: usize = 256;
pub(crate) const CONVOLUTION_FFT_SIZE: usize = CONVOLUTION_PARTITION_SIZE * 2;
pub(crate) const CONVOLUTION_LATENCY_FRAMES: usize = CONVOLUTION_PARTITION_SIZE;
pub(crate) const MAX_IR_SECONDS: f64 = 10.0;

/// Immutable, FFT-partitioned impulse response shared by convolution runtimes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedConvolutionIr {
    /// Process sample rate.
    pub(crate) sample_rate: f64,
    /// Number of source channels.
    pub(crate) source_channels: usize,
    /// Source frame count before partition padding.
    pub(crate) source_frames: usize,
    /// Number of frames in the prepared response.
    pub(crate) prepared_frames: usize,
    /// Uniform partition length.
    pub(crate) partition_size: usize,
    /// FFT length.
    pub(crate) fft_size: usize,
    /// Mono or stereo partition spectra.
    pub(crate) spectra: PreparedConvolutionSpectra,
}

impl PreparedConvolutionIr {
    pub(crate) fn empty(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            source_channels: 1,
            source_frames: 0,
            prepared_frames: 0,
            partition_size: CONVOLUTION_PARTITION_SIZE,
            fft_size: CONVOLUTION_FFT_SIZE,
            spectra: PreparedConvolutionSpectra::Mono(Box::new([])),
        }
    }

    pub(crate) fn partition_count(&self) -> usize {
        match &self.spectra {
            PreparedConvolutionSpectra::Mono(partitions) => partitions.len(),
            PreparedConvolutionSpectra::Stereo { left, .. } => left.len(),
        }
    }
}

/// Channel layout of an impulse response's frequency-domain partitions.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreparedConvolutionSpectra {
    /// One response shared by both output channels.
    Mono(Box<[Box<[Complex<f32>]>]>),
    /// Independent left and right responses.
    Stereo {
        /// Left response partitions.
        left: Box<[Box<[Complex<f32>]>]>,
        /// Right response partitions.
        right: Box<[Box<[Complex<f32>]>]>,
    },
}

/// Prepare one decoded and resampled impulse response for partitioned convolution.
pub(crate) fn prepare_convolution_ir(
    audio: &PreparedAudio,
) -> Result<PreparedConvolutionIr, String> {
    if !audio.sample_rate.is_finite() || audio.sample_rate <= 0.0 {
        return Err("convolution sample rate must be finite and positive".to_owned());
    }
    if audio.frames == 0 {
        return Err("convolution impulse response must not be empty".to_owned());
    }
    let maximum_frames = (audio.sample_rate * MAX_IR_SECONDS).ceil();
    #[allow(clippy::cast_precision_loss)]
    if (audio.frames as f64) > maximum_frames {
        return Err(format!(
            "convolution impulse response must be at most {MAX_IR_SECONDS} seconds"
        ));
    }
    let peak = match &audio.channels {
        PreparedAudioChannels::Mono { samples } => {
            if samples.iter().any(|sample| !sample.is_finite()) {
                return Err("convolution impulse response contains a non-finite sample".to_owned());
            }
            samples
                .iter()
                .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
        }
        PreparedAudioChannels::Stereo { left, right } => {
            if left
                .iter()
                .chain(right.iter())
                .any(|sample| !sample.is_finite())
            {
                return Err("convolution impulse response contains a non-finite sample".to_owned());
            }
            left.iter()
                .chain(right.iter())
                .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
        }
    };
    if peak <= f32::EPSILON {
        return Err("convolution impulse response must contain a signal".to_owned());
    }

    let spectra = match &audio.channels {
        PreparedAudioChannels::Mono { samples } => {
            PreparedConvolutionSpectra::Mono(partition_spectra(samples))
        }
        PreparedAudioChannels::Stereo { left, right } => PreparedConvolutionSpectra::Stereo {
            left: partition_spectra(left),
            right: partition_spectra(right),
        },
    };
    Ok(PreparedConvolutionIr {
        sample_rate: audio.sample_rate,
        source_channels: audio.channel_count(),
        source_frames: audio.source_metadata.source_frames,
        prepared_frames: audio.frames,
        partition_size: CONVOLUTION_PARTITION_SIZE,
        fft_size: CONVOLUTION_FFT_SIZE,
        spectra,
    })
}

pub(crate) fn partition_spectra(samples: &[f32]) -> Box<[Box<[Complex<f32>]>]> {
    let partition_count =
        samples.len().saturating_add(CONVOLUTION_PARTITION_SIZE - 1) / CONVOLUTION_PARTITION_SIZE;
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(CONVOLUTION_FFT_SIZE);
    let mut partitions = Vec::with_capacity(partition_count);
    for partition_index in 0..partition_count {
        let start = partition_index * CONVOLUTION_PARTITION_SIZE;
        let end = (start + CONVOLUTION_PARTITION_SIZE).min(samples.len());
        let mut buffer = vec![Complex::new(0.0, 0.0); CONVOLUTION_FFT_SIZE];
        for (target, source) in buffer[..end - start].iter_mut().zip(&samples[start..end]) {
            target.re = *source;
        }
        fft.process(&mut buffer);
        partitions.push(buffer.into_boxed_slice());
    }
    partitions.into_boxed_slice()
}
