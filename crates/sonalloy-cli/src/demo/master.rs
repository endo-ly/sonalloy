//! Offline `FFmpeg` measurements and fixed-gain, oversampled Demo mastering.

use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::DemoMaster;

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct LoudnessTarget {
    pub(crate) integrated_lufs: f64,
    pub(crate) true_peak_db: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct LoudnessMeasurement {
    pub(crate) integrated_lufs: Option<f64>,
    pub(crate) true_peak_db: Option<f64>,
    pub(crate) loudness_range_lu: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ShortTermLoudness {
    /// End of the trailing three-second window, in seconds from render start.
    pub(crate) time_seconds: f64,
    pub(crate) short_term_lufs: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LoudnessAnalysis {
    #[serde(flatten)]
    pub(crate) measurement: LoudnessMeasurement,
    pub(crate) short_term_window_seconds: u32,
    pub(crate) short_term_interval_seconds: u32,
    pub(crate) short_term: Vec<ShortTermLoudness>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct LoudnessDeviation {
    pub(crate) integrated_lufs: f64,
    pub(crate) true_peak_db: f64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MasterReport {
    pub(crate) target: LoudnessTarget,
    pub(crate) input: LoudnessMeasurement,
    pub(crate) output: LoudnessMeasurement,
    pub(crate) deviation: LoudnessDeviation,
    pub(crate) input_gain_db: f64,
    pub(crate) output_gain_db: f64,
    pub(crate) attempts: u32,
}

#[derive(Debug)]
pub(crate) struct FfmpegError {
    pub(crate) not_found: bool,
    pub(crate) detail: String,
}

pub(crate) fn master(
    input: &Path,
    output: &Path,
    settings: &DemoMaster,
    sample_rate: u32,
) -> Result<MasterReport, FfmpegError> {
    let input_measurement = measure_loudness(input)?;
    let input_i = input_measurement.integrated_lufs.ok_or_else(|| ffmpeg_error(format!(
        "master target {} LUFS / {} dBTP cannot be reached: input integrated loudness and required gain are undefined",
        settings.integrated_lufs, settings.true_peak_db
    )))?;
    let oversampled_rate = sample_rate
        .checked_mul(4)
        .ok_or_else(|| ffmpeg_error("oversampled sample rate overflows"))?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let directory = tempfile::Builder::new()
        .prefix("sonalloy-master-")
        .tempdir_in(parent)
        .map_err(|error| ffmpeg_error(error.to_string()))?;
    let candidate = directory.path().join("candidate.wav");
    let corrected = directory.path().join("corrected.wav");
    let mut gain_db = settings.integrated_lufs - input_i;
    let mut last = input_measurement;
    let mut last_output_gain_db = 0.0;
    for attempt in 1..=8 {
        let limit = 10.0_f64.powf(settings.true_peak_db / 20.0);
        let filter = format!(
            "volume={gain_db}dB:precision=double,aresample={oversampled_rate},alimiter=limit={limit}:attack=5:release=50:level=0:latency=1,aresample={sample_rate}"
        );
        process_wav(input, &candidate, &filter, sample_rate)?;
        last = measure_loudness(&candidate)?;
        let mut output_gain_db = 0.0;
        let mut final_path = &candidate;
        if required(last.true_peak_db, "output true peak")? >= settings.true_peak_db {
            // Reserve one measurement quantum (loudnorm reports hundredths of a dB).
            output_gain_db =
                settings.true_peak_db - required(last.true_peak_db, "output true peak")? - 0.01;
            process_wav(
                &candidate,
                &corrected,
                &format!("volume={output_gain_db}dB:precision=double"),
                sample_rate,
            )?;
            last = measure_loudness(&corrected)?;
            final_path = &corrected;
        }
        let deviation = required(last.integrated_lufs, "output integrated loudness")?
            - settings.integrated_lufs;
        let peak_deviation =
            required(last.true_peak_db, "output true peak")? - settings.true_peak_db;
        last_output_gain_db = output_gain_db;
        if deviation.abs() <= 0.5 && peak_deviation <= 0.0 {
            // Candidates live beside the destination: only a verified WAV becomes final.
            std::fs::rename(final_path, output).map_err(|error| ffmpeg_error(error.to_string()))?;
            return Ok(MasterReport {
                target: LoudnessTarget {
                    integrated_lufs: settings.integrated_lufs,
                    true_peak_db: settings.true_peak_db,
                },
                input: input_measurement,
                output: last,
                deviation: LoudnessDeviation {
                    integrated_lufs: deviation,
                    true_peak_db: peak_deviation,
                },
                input_gain_db: gain_db,
                output_gain_db,
                attempts: attempt,
            });
        }
        if peak_deviation > 0.0 {
            break;
        }
        if attempt < 8 {
            gain_db -= deviation;
        }
    }
    let output_i = required(last.integrated_lufs, "output integrated loudness")?;
    let output_tp = required(last.true_peak_db, "output true peak")?;
    Err(ffmpeg_error(format!(
        "master target not reached: target {} LUFS / {} dBTP, measured {output_i} LUFS / {output_tp} dBTP, input gain {gain_db} dB, output gain {last_output_gain_db} dB",
        settings.integrated_lufs, settings.true_peak_db
    )))
}

pub(crate) fn measure_loudness(input: &Path) -> Result<LoudnessMeasurement, FfmpegError> {
    analyze_loudness(input).map(|analysis| analysis.measurement)
}

pub(crate) fn analyze_loudness(input: &Path) -> Result<LoudnessAnalysis, FfmpegError> {
    let output = run_filter(
        input,
        "ebur128=metadata=1,ametadata=mode=print:key=lavfi.r128.S,loudnorm=I=-24:TP=-2:LRA=7:print_format=json",
    )?;
    let text = String::from_utf8_lossy(&output.stderr);
    let mut measurement = parse_measurement(&text)?;
    let short_term = parse_short_term(&text);
    if short_term.is_empty() {
        measurement.loudness_range_lu = None;
    }
    Ok(LoudnessAnalysis {
        measurement,
        short_term_window_seconds: 3,
        short_term_interval_seconds: 3,
        short_term,
    })
}

fn parse_short_term(text: &str) -> Vec<ShortTermLoudness> {
    let mut result = Vec::new();
    let mut timestamp = None;
    let mut next_end = 3.0;
    for line in text.lines() {
        if let Some((_, time)) = line.split_once("pts_time:") {
            timestamp = time
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<f64>().ok());
        }
        if let Some((_, value)) = line.split_once("lavfi.r128.S=")
            && let Some(time) = timestamp
            // Metadata timestamps mark the START of a 100 ms scanner frame.
            && time + 0.100_001 >= next_end
        {
            // The scanner's formatted zero-energy sentinel is not a measured LUFS value.
            let lufs = (value.trim() != "-120.691")
                .then(|| value.trim().parse::<f64>().ok())
                .flatten()
                .filter(|value| value.is_finite());
            result.push(ShortTermLoudness {
                time_seconds: next_end,
                short_term_lufs: lufs,
            });
            next_end += 3.0;
        }
    }
    result
}

fn parse_measurement(text: &str) -> Result<LoudnessMeasurement, FfmpegError> {
    for (index, _) in text
        .char_indices()
        .filter(|(_, character)| *character == '{')
    {
        let mut deserializer = serde_json::Deserializer::from_str(&text[index..]);
        if let Ok(value) = Value::deserialize(&mut deserializer)
            && value.get("input_i").is_some()
        {
            let field = |name| -> Result<Option<f64>, FfmpegError> {
                let number = value
                    .get(name)
                    .and_then(|value| {
                        value
                            .as_f64()
                            .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
                    })
                    .ok_or_else(|| {
                        ffmpeg_error(format!("ffmpeg loudness report contains no numeric {name}"))
                    })?;
                Ok(number.is_finite().then_some(number))
            };
            let integrated_lufs = field("input_i")?;
            let lra = field("input_lra")?;
            return Ok(LoudnessMeasurement {
                integrated_lufs,
                true_peak_db: field("input_tp")?,
                // LRA is not defined when integrated loudness cannot be measured.
                loudness_range_lu: integrated_lufs.and(lra),
            });
        }
    }
    Err(ffmpeg_error("ffmpeg did not emit a loudness JSON report"))
}

fn required(value: Option<f64>, name: &str) -> Result<f64, FfmpegError> {
    value.ok_or_else(|| ffmpeg_error(format!("{name} is undefined")))
}

fn run_filter(input: &Path, filter: &str) -> Result<Output, FfmpegError> {
    run_command(
        Command::new("ffmpeg")
            .args(["-hide_banner", "-nostdin", "-i"])
            .arg(input)
            .args(["-af", filter, "-f", "null", "-"]),
    )
}

fn process_wav(
    input: &Path,
    output: &Path,
    filter: &str,
    sample_rate: u32,
) -> Result<(), FfmpegError> {
    run_command(
        Command::new("ffmpeg")
            .args(["-hide_banner", "-nostdin", "-y", "-i"])
            .arg(input)
            .args(["-af", filter, "-ar"])
            .arg(sample_rate.to_string())
            .args(["-ac", "2", "-c:a", "pcm_f32le", "-f", "wav"])
            .arg(output),
    )?;
    Ok(())
}

pub(crate) fn encode_mp3(input: &Path, output: &Path) -> Result<(), FfmpegError> {
    run_command(
        Command::new("ffmpeg")
            .args(["-hide_banner", "-nostdin", "-y", "-i"])
            .arg(input)
            .args(["-vn", "-codec:a", "libmp3lame", "-b:a", "256k", "-f", "mp3"])
            .arg(output),
    )?;
    Ok(())
}

fn run_command(command: &mut Command) -> Result<Output, FfmpegError> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|error| FfmpegError {
            not_found: error.kind() == std::io::ErrorKind::NotFound,
            detail: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(ffmpeg_error(String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(output)
}

fn ffmpeg_error(detail: impl Into<String>) -> FfmpegError {
    FfmpegError {
        not_found: false,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_verifies_loudness_peak_and_preserves_limiter_tail() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let directory = tempfile::tempdir().expect("directory");
        let input = directory.path().join("input.wav");
        let output = directory.path().join("output.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(&input, spec).expect("writer");
        for frame in 0..48_000 * 6 {
            let time = f64::from(frame) / 48_000.0;
            let envelope = if frame % 24_000 < 2_400 { 0.8 } else { 0.12 };
            #[allow(clippy::cast_possible_truncation)]
            let sample = ((std::f64::consts::TAU * 440.0 * time).sin() * envelope) as f32;
            writer.write_sample(sample).expect("left");
            writer.write_sample(sample).expect("right");
        }
        writer.finalize().expect("finalize");

        let settings = DemoMaster {
            integrated_lufs: -9.0,
            true_peak_db: -1.0,
        };
        for sample_rate in [44_100, 48_000] {
            let report = master(&input, &output, &settings, sample_rate).expect("master");
            let measured = analyze_loudness(&output).expect("independent measurement");
            let mut reader = hound::WavReader::open(&output).expect("output");
            assert_eq!(reader.duration(), sample_rate * 6);
            assert_eq!(
                reader.spec(),
                hound::WavSpec {
                    sample_rate,
                    ..spec
                }
            );
            assert!(
                reader
                    .samples::<f32>()
                    .last()
                    .expect("tail")
                    .expect("sample")
                    .abs()
                    > 0.001
            );
            assert!((measured.measurement.integrated_lufs.expect("LUFS") + 9.0).abs() <= 0.5);
            assert!(measured.measurement.true_peak_db.expect("peak") <= -1.0);
            assert!(report.attempts <= 8);
            assert_eq!(measured.short_term.len(), 2);
            assert!((measured.short_term[0].time_seconds - 3.0).abs() < f64::EPSILON);
            assert!(
                measured
                    .short_term
                    .iter()
                    .all(|window| window.short_term_lufs.is_some())
            );
        }

        let silence = directory.path().join("silence.wav");
        let mut writer = hound::WavWriter::create(&silence, spec).expect("silence");
        for _ in 0..48_000 {
            writer.write_sample(0.0_f32).expect("sample");
        }
        writer.finalize().expect("silence finalized");
        let failed = directory.path().join("failed.wav");
        let impossible = DemoMaster {
            integrated_lufs: -5.0,
            true_peak_db: -9.0,
        };
        let error = master(&input, &failed, &impossible, 48_000).expect_err("unreachable loudness");
        assert!(error.detail.contains("target -5 LUFS / -9 dBTP"));
        assert!(error.detail.contains("input gain"));
        assert!(!failed.exists());
        assert!(master(&silence, &failed, &settings, 48_000).is_err());
        assert!(!failed.exists());
        let measurement = analyze_loudness(&silence).expect("silence analysis");
        assert!(measurement.measurement.integrated_lufs.is_none());
        assert!(measurement.measurement.true_peak_db.is_none());
        assert!(measurement.short_term.is_empty());
    }
}
