//! FFmpeg-backed Demo mastering and loudness measurement helpers.

use std::fmt::Write;
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::DemoMaster;

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct LoudnessTarget {
    pub(crate) integrated_lufs: f64,
    pub(crate) true_peak_db: f64,
    pub(crate) loudness_range_lu: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct LoudnessMeasurement {
    pub(crate) integrated_lufs: f64,
    pub(crate) true_peak_db: f64,
    pub(crate) loudness_range_lu: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct LoudnessDeviation {
    pub(crate) integrated_lufs: f64,
    pub(crate) true_peak_db: f64,
    pub(crate) loudness_range_lu: f64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MasterReport {
    pub(crate) target: LoudnessTarget,
    pub(crate) input: LoudnessMeasurement,
    pub(crate) output: LoudnessMeasurement,
    pub(crate) deviation: LoudnessDeviation,
    pub(crate) normalization_type: String,
    pub(crate) true_peak_correction_db: f64,
}

#[derive(Debug, Clone, Copy)]
struct LoudnormMeasurements {
    input_i: f64,
    input_tp: f64,
    input_lra: f64,
    input_thresh: f64,
    target_offset: f64,
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
    let first_pass = run_loudnorm(input, &loudnorm_filter(settings, None), None, sample_rate)?;
    let measurements = LoudnormMeasurements {
        input_i: required_measurement(first_pass.input_i, "input_i")?,
        input_tp: required_measurement(first_pass.input_tp, "input_tp")?,
        input_lra: required_measurement(first_pass.input_lra, "input_lra")?,
        input_thresh: required_measurement(first_pass.input_thresh, "input_thresh")?,
        target_offset: required_measurement(first_pass.target_offset, "target_offset")?,
    };
    let input_measurement = measurement_from_report(&first_pass)?;

    let temporary_directory = tempfile::Builder::new()
        .prefix("sonalloy-demo-master-")
        .tempdir()
        .map_err(|error| ffmpeg_error(error.to_string()))?;
    let normalized_path = temporary_directory.path().join("normalized.wav");
    let corrected_path = temporary_directory.path().join("corrected.wav");
    let second_pass = run_loudnorm(
        input,
        &loudnorm_filter(settings, Some(measurements)),
        Some(&normalized_path),
        sample_rate,
    )?;
    let normalization_type = second_pass.normalization_type.ok_or_else(|| FfmpegError {
        not_found: false,
        detail: "FFmpeg loudnorm report did not contain normalization_type".to_owned(),
    })?;
    let mut output_measurement = measure_loudness(&normalized_path, Some(settings), sample_rate)?;
    let mut final_path = normalized_path.as_path();
    let mut correction_db = 0.0;

    if output_measurement.true_peak_db > settings.true_peak_db {
        correction_db = settings.true_peak_db - output_measurement.true_peak_db;
        let mut command =
            build_gain_correction_command(&normalized_path, &corrected_path, correction_db);
        ensure_success(run_command(&mut command)?)?;
        output_measurement = measure_loudness(&corrected_path, Some(settings), sample_rate)?;
        final_path = corrected_path.as_path();
    }

    if output_measurement.true_peak_db > settings.true_peak_db {
        return Err(ffmpeg_error(format!(
            "completed WAV true peak {} dBTP exceeds target {} dBTP after gain correction",
            output_measurement.true_peak_db, settings.true_peak_db
        )));
    }
    fs::copy(final_path, output)
        .map_err(|error| ffmpeg_error(format!("could not write completed WAV: {error}")))?;

    Ok(master_report(
        settings,
        input_measurement,
        output_measurement,
        normalization_type,
        correction_db,
    ))
}

pub(crate) fn measure_loudness(
    input: &Path,
    target: Option<&DemoMaster>,
    sample_rate: u32,
) -> Result<LoudnessMeasurement, FfmpegError> {
    let filter = target.map_or_else(
        || "loudnorm=I=-24:TP=-2:LRA=7:print_format=json".to_owned(),
        |target| loudnorm_filter(target, None),
    );
    let report = run_loudnorm(input, &filter, None, sample_rate)?;
    measurement_from_report(&report)
}

pub(crate) fn encode_mp3(input: &Path, output: &Path) -> Result<(), FfmpegError> {
    let mut command = build_mp3_command(input, output);
    let output = run_command(&mut command)?;
    ensure_success(output).map(|_| ())
}

fn measurement_from_report(report: &LoudnormReport) -> Result<LoudnessMeasurement, FfmpegError> {
    Ok(LoudnessMeasurement {
        integrated_lufs: required_measurement(report.input_i, "input_i")?,
        true_peak_db: required_measurement(report.input_tp, "input_tp")?,
        loudness_range_lu: required_measurement(report.input_lra, "input_lra")?,
    })
}

fn master_report(
    settings: &DemoMaster,
    input: LoudnessMeasurement,
    output: LoudnessMeasurement,
    normalization_type: String,
    correction_db: f64,
) -> MasterReport {
    let target = LoudnessTarget {
        integrated_lufs: settings.integrated_lufs,
        true_peak_db: settings.true_peak_db,
        loudness_range_lu: settings.loudness_range_lu,
    };
    MasterReport {
        target,
        input,
        output,
        deviation: LoudnessDeviation {
            integrated_lufs: output.integrated_lufs - target.integrated_lufs,
            true_peak_db: output.true_peak_db - target.true_peak_db,
            loudness_range_lu: output.loudness_range_lu - target.loudness_range_lu,
        },
        normalization_type,
        true_peak_correction_db: correction_db,
    }
}

fn run_loudnorm(
    input: &Path,
    filter: &str,
    output: Option<&Path>,
    sample_rate: u32,
) -> Result<LoudnormReport, FfmpegError> {
    let mut command = build_loudnorm_command(input, filter, output, sample_rate);
    let output = run_command(&mut command)?;
    ensure_success(output).and_then(|output| {
        parse_loudnorm_report(&String::from_utf8_lossy(&output.stderr)).map_err(|detail| {
            FfmpegError {
                not_found: false,
                detail,
            }
        })
    })
}

fn build_loudnorm_command(
    input: &Path,
    filter: &str,
    output: Option<&Path>,
    sample_rate: u32,
) -> Command {
    let mut command = Command::new("ffmpeg");
    command
        .args(["-hide_banner", "-nostdin", "-y", "-i"])
        .arg(input)
        .args(["-af", filter]);
    if let Some(output) = output {
        command
            .arg("-ar")
            .arg(sample_rate.to_string())
            .args(["-ac", "2", "-c:a", "pcm_f32le", "-f", "wav"])
            .arg(output);
    } else {
        command.args(["-f", "null", "-"]);
    }
    command.stdin(Stdio::null());
    command
}

fn build_gain_correction_command(input: &Path, output: &Path, gain_db: f64) -> Command {
    let mut command = Command::new("ffmpeg");
    command
        .args(["-hide_banner", "-nostdin", "-y", "-i"])
        .arg(input)
        .arg("-af")
        .arg(format!("volume={gain_db}dB"))
        .args(["-ac", "2", "-c:a", "pcm_f32le", "-f", "wav"])
        .arg(output)
        .stdin(Stdio::null());
    command
}

fn build_mp3_command(input: &Path, output: &Path) -> Command {
    let mut command = Command::new("ffmpeg");
    command
        .args(["-hide_banner", "-nostdin", "-y", "-i"])
        .arg(input)
        .args(["-vn", "-codec:a", "libmp3lame", "-b:a", "256k", "-f", "mp3"])
        .arg(output)
        .stdin(Stdio::null());
    command
}

fn run_command(command: &mut Command) -> Result<Output, FfmpegError> {
    command.output().map_err(|error| FfmpegError {
        not_found: error.kind() == std::io::ErrorKind::NotFound,
        detail: error.to_string(),
    })
}

fn ensure_success(output: Output) -> Result<Output, FfmpegError> {
    if !output.status.success() {
        return Err(FfmpegError {
            not_found: false,
            detail: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(output)
}

fn ffmpeg_error(detail: String) -> FfmpegError {
    FfmpegError {
        not_found: false,
        detail,
    }
}

fn loudnorm_filter(settings: &DemoMaster, measurements: Option<LoudnormMeasurements>) -> String {
    let mut filter = format!(
        "loudnorm=I={}:TP={}:LRA={}",
        settings.integrated_lufs, settings.true_peak_db, settings.loudness_range_lu
    );
    if let Some(measurements) = measurements {
        let _ = write!(
            filter,
            ":measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true",
            measurements.input_i,
            measurements.input_tp,
            measurements.input_lra,
            measurements.input_thresh,
            measurements.target_offset
        );
    }
    filter.push_str(":print_format=json");
    filter
}

fn required_measurement(value: Option<f64>, name: &str) -> Result<f64, FfmpegError> {
    value
        .filter(|value| value.is_finite())
        .ok_or_else(|| FfmpegError {
            not_found: false,
            detail: format!("FFmpeg loudnorm report did not contain finite {name}"),
        })
}

#[derive(Debug)]
struct LoudnormReport {
    input_i: Option<f64>,
    input_tp: Option<f64>,
    input_lra: Option<f64>,
    input_thresh: Option<f64>,
    target_offset: Option<f64>,
    normalization_type: Option<String>,
}

fn parse_loudnorm_report(text: &str) -> Result<LoudnormReport, String> {
    let value = find_json_object(text)
        .ok_or_else(|| "FFmpeg loudnorm did not emit a JSON report".to_owned())?;
    let object = value
        .as_object()
        .ok_or_else(|| "FFmpeg loudnorm report is not an object".to_owned())?;
    let field = |name: &str| object.get(name).and_then(json_number);
    Ok(LoudnormReport {
        input_i: field("input_i"),
        input_tp: field("input_tp"),
        input_lra: field("input_lra"),
        input_thresh: field("input_thresh"),
        target_offset: field("target_offset"),
        normalization_type: object
            .get("normalization_type")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn find_json_object(text: &str) -> Option<Value> {
    for (index, _) in text
        .char_indices()
        .filter(|(_, character)| *character == '{')
    {
        let mut deserializer = serde_json::Deserializer::from_str(&text[index..]);
        if let Ok(value) = Value::deserialize(&mut deserializer)
            && value.get("input_i").is_some()
        {
            return Some(value);
        }
    }
    None
}

fn json_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        .filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::{
        LoudnessMeasurement, build_gain_correction_command, build_loudnorm_command,
        build_mp3_command, find_json_object, loudnorm_filter, master_report, parse_loudnorm_report,
    };
    use crate::demo::DemoMaster;

    #[test]
    fn master_corrects_true_peak_overshoot_after_sample_rate_conversion() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let directory = tempfile::tempdir().expect("temporary directory");
        let input = directory.path().join("overshoot-source.wav");
        let output = directory.path().join("mastered.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(&input, spec).expect("input WAV");
        // Sparse high-frequency bursts expose inter-sample peaks during rate conversion.
        for frame in 0..48_000 * 3 {
            let pulse = frame % 4_800 < 48;
            let time = f64::from(frame) / 48_000.0;
            let sample = if pulse {
                (std::f64::consts::TAU * 18_000.0 * time).sin() * 0.9
            } else {
                (std::f64::consts::TAU * 440.0 * time).sin() * 0.02
            };
            #[allow(clippy::cast_possible_truncation)]
            let sample = sample as f32;
            writer.write_sample(sample).expect("left sample");
            writer.write_sample(sample).expect("right sample");
        }
        writer.finalize().expect("input WAV finalized");

        let settings = DemoMaster {
            integrated_lufs: -10.0,
            true_peak_db: -1.0,
            loudness_range_lu: 7.0,
        };
        let report = super::master(&input, &output, &settings, 44_100).expect("master succeeds");
        let independently_measured_tp = ffmpeg_true_peak(&output);

        assert!(report.true_peak_correction_db < 0.0);
        assert_eq!(
            hound::WavReader::open(&output)
                .expect("mastered WAV opens")
                .spec()
                .sample_rate,
            44_100
        );
        assert!((report.output.true_peak_db - independently_measured_tp).abs() < 0.05);
        assert!(independently_measured_tp <= settings.true_peak_db);
    }

    fn ffmpeg_true_peak(path: &Path) -> f64 {
        let output = Command::new("ffmpeg")
            .args(["-hide_banner", "-nostdin", "-i"])
            .arg(path)
            .args([
                "-af",
                "loudnorm=I=-10:TP=-1:LRA=7:print_format=json",
                "-f",
                "null",
                "-",
            ])
            .output()
            .expect("FFmpeg measurement starts");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        super::parse_loudnorm_report(&String::from_utf8_lossy(&output.stderr))
            .expect("measurement report parses")
            .input_tp
            .expect("measurement includes input true peak")
    }

    fn command_arguments(command: &std::process::Command) -> Vec<String> {
        command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn ffmpeg_commands_keep_sample_rate_and_codec_contracts() {
        let wav_arguments = command_arguments(&build_loudnorm_command(
            Path::new("mix.wav"),
            "loudnorm=I=-16",
            Some(Path::new("master.wav")),
            48_000,
        ));
        assert!(
            wav_arguments
                .windows(2)
                .any(|window| window == ["-ar", "48000"])
        );
        assert!(
            wav_arguments
                .windows(2)
                .any(|window| window == ["-f", "wav"])
        );

        let correction_arguments = command_arguments(&build_gain_correction_command(
            Path::new("candidate.wav"),
            Path::new("corrected.wav"),
            -1.2,
        ));
        assert!(
            correction_arguments
                .windows(2)
                .any(|window| { window == ["-af", "volume=-1.2dB"] })
        );
        assert!(
            !correction_arguments
                .iter()
                .any(|argument| argument == "-ar")
        );

        let mp3_arguments = command_arguments(&build_mp3_command(
            Path::new("master.wav"),
            Path::new("preview.mp3"),
        ));
        assert!(
            mp3_arguments
                .windows(2)
                .any(|window| window == ["-f", "mp3"])
        );
        assert!(
            mp3_arguments
                .windows(2)
                .any(|window| window == ["-b:a", "256k"])
        );
        assert!(
            mp3_arguments
                .windows(2)
                .any(|window| window == ["-codec:a", "libmp3lame"])
        );
    }

    #[test]
    fn loudnorm_filter_contains_first_and_second_pass_arguments() {
        let settings = DemoMaster {
            integrated_lufs: -16.0,
            true_peak_db: -1.0,
            loudness_range_lu: 11.0,
        };
        let first = loudnorm_filter(&settings, None);
        let second = loudnorm_filter(
            &settings,
            Some(super::LoudnormMeasurements {
                input_i: -18.0,
                input_tp: -2.0,
                input_lra: 5.0,
                input_thresh: -28.0,
                target_offset: 2.0,
            }),
        );

        assert_eq!(first, "loudnorm=I=-16:TP=-1:LRA=11:print_format=json");
        assert_eq!(
            second,
            "loudnorm=I=-16:TP=-1:LRA=11:measured_I=-18:measured_TP=-2:measured_LRA=5:measured_thresh=-28:offset=2:linear=true:print_format=json"
        );
    }

    #[test]
    fn loudnorm_report_parses_measurements_and_normalization_type() {
        let report = parse_loudnorm_report(
            "[Parsed_loudnorm_0 @ 0x0]\n{\"input_i\":\"-18.70\",\"input_tp\":\"-2.4\",\"input_lra\":\"5.1\",\"input_thresh\":\"-28.0\",\"target_offset\":\"2.7\",\"normalization_type\":\"dynamic\"}\n",
        )
        .expect("loudnorm report parses");

        assert_eq!(report.input_i, Some(-18.7));
        assert_eq!(report.input_tp, Some(-2.4));
        assert_eq!(report.input_lra, Some(5.1));
        assert_eq!(report.normalization_type.as_deref(), Some("dynamic"));
        assert!(find_json_object("no report").is_none());
    }

    #[test]
    fn master_report_calculates_deviation_from_the_target() {
        let settings = DemoMaster {
            integrated_lufs: -10.0,
            true_peak_db: -1.0,
            loudness_range_lu: 7.0,
        };
        let report = master_report(
            &settings,
            LoudnessMeasurement {
                integrated_lufs: -17.5,
                true_peak_db: -3.2,
                loudness_range_lu: 6.4,
            },
            LoudnessMeasurement {
                integrated_lufs: -11.8,
                true_peak_db: -1.0,
                loudness_range_lu: 6.1,
            },
            "dynamic".to_owned(),
            -1.2,
        );

        assert!((report.target.integrated_lufs - -10.0).abs() < f64::EPSILON);
        assert!((report.input.true_peak_db - -3.2).abs() < 1.0e-12);
        assert!((report.deviation.integrated_lufs - -1.8).abs() < 1.0e-12);
        assert!(report.deviation.true_peak_db.abs() < f64::EPSILON);
        assert!((report.deviation.loudness_range_lu - -0.9).abs() < 1.0e-12);
        assert_eq!(report.normalization_type, "dynamic");
        assert!((report.true_peak_correction_db - -1.2).abs() < 1.0e-12);
    }
}
