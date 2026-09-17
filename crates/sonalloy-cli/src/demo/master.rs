//! FFmpeg-backed Demo mastering helpers.

use std::fmt::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::DemoMaster;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MasterReport {
    pub(crate) input_i: Option<f64>,
    pub(crate) input_tp: Option<f64>,
    pub(crate) input_lra: Option<f64>,
    pub(crate) output_i: Option<f64>,
    pub(crate) output_tp: Option<f64>,
    pub(crate) output_lra: Option<f64>,
    pub(crate) normalization_type: String,
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
    let second_pass = run_loudnorm(
        input,
        &loudnorm_filter(settings, Some(measurements)),
        Some(output),
        sample_rate,
    )?;
    let normalization_type = second_pass.normalization_type.ok_or_else(|| FfmpegError {
        not_found: false,
        detail: "FFmpeg loudnorm report did not contain normalization_type".to_owned(),
    })?;
    Ok(MasterReport {
        input_i: first_pass.input_i,
        input_tp: first_pass.input_tp,
        input_lra: first_pass.input_lra,
        output_i: second_pass.output_i,
        output_tp: second_pass.output_tp,
        output_lra: second_pass.output_lra,
        normalization_type,
    })
}

pub(crate) fn encode_mp3(input: &Path, output: &Path) -> Result<(), FfmpegError> {
    let mut command = build_mp3_command(input, output);
    let output = run_command(&mut command)?;
    ensure_success(output).map(|_| ())
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
    output_i: Option<f64>,
    output_tp: Option<f64>,
    output_lra: Option<f64>,
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
        output_i: field("output_i"),
        output_tp: field("output_tp"),
        output_lra: field("output_lra"),
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

    use super::{
        build_loudnorm_command, build_mp3_command, find_json_object, loudnorm_filter,
        parse_loudnorm_report,
    };
    use crate::demo::DemoMaster;

    fn command_arguments(command: &std::process::Command) -> Vec<String> {
        command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn ffmpeg_output_containers_are_explicit() {
        let wav_arguments = command_arguments(&build_loudnorm_command(
            Path::new("mix.wav"),
            "loudnorm=I=-16",
            Some(Path::new("master")),
            48_000,
        ));
        assert!(
            wav_arguments
                .windows(2)
                .any(|window| window == ["-f", "wav"])
        );

        let mp3_arguments = command_arguments(&build_mp3_command(
            Path::new("mix.wav"),
            Path::new("preview"),
        ));
        assert!(
            mp3_arguments
                .windows(2)
                .any(|window| window == ["-f", "mp3"])
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
    fn loudnorm_report_parses_ffmpeg_log_prefix_and_string_numbers() {
        let report = parse_loudnorm_report(
            "[Parsed_loudnorm_0 @ 0x0]\n{\"input_i\":\"-18.70\",\"input_tp\":\"-2.4\",\"input_lra\":\"5.1\",\"input_thresh\":\"-28.0\",\"target_offset\":\"2.7\",\"output_i\":\"-16.0\",\"output_tp\":\"-1.0\",\"output_lra\":\"5.1\",\"normalization_type\":\"linear\"}\n",
        )
        .expect("loudnorm report parses");

        assert_eq!(report.input_i, Some(-18.7));
        assert_eq!(report.output_lra, Some(5.1));
        assert_eq!(report.normalization_type.as_deref(), Some("linear"));
        assert!(find_json_object("no report").is_none());
    }

    #[test]
    fn loudnorm_report_omits_non_finite_measurements() {
        let report = parse_loudnorm_report(
            r#"{"input_i":"-inf","input_tp":"NaN","input_lra":"0.0","normalization_type":"dynamic"}"#,
        )
        .expect("loudnorm report parses");

        assert_eq!(report.input_i, None);
        assert_eq!(report.input_tp, None);
        assert_eq!(report.input_lra, Some(0.0));
    }
}
