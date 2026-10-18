use serde::Serialize;
use sonalloy_core::RenderedAudio;
use thiserror::Error;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LoudnessStatus {
    Measured,
    Silent,
    Unmeasurable,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct AudioLoudnessReport {
    pub(crate) status: LoudnessStatus,
    pub(crate) integrated_lufs: Option<f64>,
    pub(crate) true_peak_db: Option<f64>,
}

#[derive(Debug, Error)]
pub(crate) enum LoudnessError {
    #[error("sample rate must be greater than zero")]
    InvalidSampleRate,
    #[error("audio must contain one or two channels")]
    InvalidChannelCount,
    #[error("audio channels have different lengths")]
    UnequalChannelLengths,
    #[error("frame range is outside the audio or empty")]
    InvalidFrameRange,
    #[error("audio contains a non-finite sample")]
    NonFiniteSample,
    #[error("could not initialize the loudness meter")]
    MeterInitialization,
    #[error("could not process audio in the loudness meter")]
    MeterProcessing,
    #[error("could not read loudness meter results")]
    MeterResult,
}

pub(crate) fn analyze_audio_loudness(
    audio: &RenderedAudio,
    start_frame: usize,
    end_frame: usize,
) -> Result<AudioLoudnessReport, LoudnessError> {
    if audio.sample_rate == 0 {
        return Err(LoudnessError::InvalidSampleRate);
    }
    if !(1..=2).contains(&audio.channels.len()) {
        return Err(LoudnessError::InvalidChannelCount);
    }
    let frames = audio.channels[0].len();
    if audio.channels.iter().any(|channel| channel.len() != frames) {
        return Err(LoudnessError::UnequalChannelLengths);
    }
    if start_frame >= end_frame || end_frame > frames {
        return Err(LoudnessError::InvalidFrameRange);
    }

    let channels = audio
        .channels
        .iter()
        .map(|channel| &channel[start_frame..end_frame])
        .collect::<Vec<_>>();
    let mut is_silent = true;
    for sample in channels.iter().flat_map(|channel| channel.iter()) {
        if !sample.is_finite() {
            return Err(LoudnessError::NonFiniteSample);
        }
        is_silent &= *sample == 0.0;
    }
    if is_silent {
        return Ok(AudioLoudnessReport {
            status: LoudnessStatus::Silent,
            integrated_lufs: None,
            true_peak_db: None,
        });
    }

    let channel_count = u32::try_from(channels.len()).expect("one or two channels fit in u32");
    let mut meter = ebur128::EbuR128::new(
        channel_count,
        audio.sample_rate,
        ebur128::Mode::I | ebur128::Mode::TRUE_PEAK,
    )
    .map_err(|_| LoudnessError::MeterInitialization)?;
    meter
        .add_frames_planar_f32(&channels)
        .map_err(|_| LoudnessError::MeterProcessing)?;

    let integrated_lufs = meter
        .loudness_global()
        .map_err(|_| LoudnessError::MeterResult)?;
    let mut true_peak = 0.0_f64;
    for channel in 0..channel_count {
        true_peak = true_peak.max(
            meter
                .true_peak(channel)
                .map_err(|_| LoudnessError::MeterResult)?,
        );
    }
    let true_peak_db = (true_peak > 0.0 && true_peak.is_finite())
        .then(|| 20.0 * true_peak.log10())
        .filter(|value| value.is_finite());
    let integrated_lufs = integrated_lufs.is_finite().then_some(integrated_lufs);

    Ok(AudioLoudnessReport {
        status: if integrated_lufs.is_some() {
            LoudnessStatus::Measured
        } else {
            LoudnessStatus::Unmeasurable
        },
        integrated_lufs,
        true_peak_db,
    })
}

#[cfg(test)]
mod tests {
    use super::{AudioLoudnessReport, LoudnessError, LoudnessStatus, analyze_audio_loudness};
    use sonalloy_core::RenderedAudio;

    fn stereo_audio(left: Vec<f32>, right: Vec<f32>) -> RenderedAudio {
        RenderedAudio {
            sample_rate: 48_000,
            channels: vec![left, right],
        }
    }

    fn sine(amplitude: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|frame| {
                let phase_index = u8::try_from(frame % 48).expect("sine phase fits in u8");
                let phase = std::f32::consts::TAU * f32::from(phase_index) / 48.0;
                amplitude * phase.sin()
            })
            .collect()
    }

    #[test]
    fn measured_audio_reports_loudness_and_true_peak_for_the_selected_range() {
        let mut left = sine(0.9, 4_800);
        left.extend(sine(0.25, 48_000));
        let mut right = sine(0.9, 4_800);
        right.extend(sine(0.25, 48_000));
        let audio = stereo_audio(left, right);

        let report = analyze_audio_loudness(&audio, 4_800, 52_800).expect("loudness measured");
        assert!(matches!(report.status, LoudnessStatus::Measured));
        assert!(report.integrated_lufs.is_some_and(f64::is_finite));
        assert!(report.true_peak_db.is_some_and(f64::is_finite));
        assert!(
            (report.true_peak_db.expect("non-zero audio") - 20.0 * 0.25_f64.log10()).abs() < 0.1
        );

        let quiet_audio = stereo_audio(sine(0.125, 48_000), sine(0.125, 48_000));
        let quiet_report = analyze_audio_loudness(&quiet_audio, 0, 48_000).expect("quiet measured");
        assert!(
            (quiet_report.integrated_lufs.expect("quiet LUFS")
                - report.integrated_lufs.expect("measured LUFS")
                - 20.0 * 0.5_f64.log10())
            .abs()
                < 0.1
        );

        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_ok()
        {
            let directory = tempfile::tempdir().expect("temporary directory");
            let selected_audio = stereo_audio(
                audio.channels[0][4_800..52_800].to_vec(),
                audio.channels[1][4_800..52_800].to_vec(),
            );
            let path = directory.path().join("selected.wav");
            crate::output::write_wav(&path, &selected_audio).expect("selected WAV written");
            let ffmpeg = super::super::measure_loudness(&path).expect("FFmpeg loudness measured");
            assert!(
                (ffmpeg.integrated_lufs.expect("FFmpeg LUFS")
                    - report.integrated_lufs.expect("measured LUFS"))
                .abs()
                    <= 0.5
            );
        }
    }

    #[test]
    fn silent_audio_has_no_loudness_or_true_peak() {
        let audio = stereo_audio(vec![0.0; 48_000], vec![0.0; 48_000]);

        let report = analyze_audio_loudness(&audio, 0, 48_000).expect("silence analyzed");
        assert!(matches!(report.status, LoudnessStatus::Silent));
        assert_eq!(report.integrated_lufs, None);
        assert_eq!(report.true_peak_db, None);
    }

    #[test]
    fn audio_without_a_complete_loudness_window_retains_true_peak() {
        let audio = stereo_audio(vec![0.5; 1_000], vec![0.5; 1_000]);

        let report = analyze_audio_loudness(&audio, 0, 1_000).expect("short audio analyzed");
        assert!(matches!(report.status, LoudnessStatus::Unmeasurable));
        assert_eq!(report.integrated_lufs, None);
        assert!(report.true_peak_db.is_some_and(f64::is_finite));
    }

    #[test]
    fn invalid_audio_input_is_rejected() {
        let audio = stereo_audio(vec![0.25; 4_800], vec![0.25; 4_800]);
        assert!(matches!(
            analyze_audio_loudness(&audio, 4_800, 4_800),
            Err(LoudnessError::InvalidFrameRange)
        ));
        assert!(matches!(
            analyze_audio_loudness(&audio, 0, 4_801),
            Err(LoudnessError::InvalidFrameRange)
        ));

        let invalid = stereo_audio(vec![f32::NAN; 4_800], vec![0.25; 4_800]);
        assert!(matches!(
            analyze_audio_loudness(&invalid, 0, 4_800),
            Err(LoudnessError::NonFiniteSample)
        ));

        let invalid_sample_rate = RenderedAudio {
            sample_rate: 0,
            channels: vec![vec![0.25; 4_800], vec![0.25; 4_800]],
        };
        assert!(matches!(
            analyze_audio_loudness(&invalid_sample_rate, 0, 4_800),
            Err(LoudnessError::InvalidSampleRate)
        ));

        let unequal_channels = stereo_audio(vec![0.25; 4_800], vec![0.25; 4_799]);
        assert!(matches!(
            analyze_audio_loudness(&unequal_channels, 0, 4_799),
            Err(LoudnessError::UnequalChannelLengths)
        ));
    }

    #[test]
    fn loudness_report_serializes_measurement_state_and_null_values() {
        let report = AudioLoudnessReport {
            status: LoudnessStatus::Unmeasurable,
            integrated_lufs: None,
            true_peak_db: Some(-6.0),
        };

        let json = serde_json::to_value(report).expect("report serializes");
        assert_eq!(json["status"], "unmeasurable");
        assert!(json["integrated_lufs"].is_null());
        assert_eq!(json["true_peak_db"], -6.0);
    }
}
