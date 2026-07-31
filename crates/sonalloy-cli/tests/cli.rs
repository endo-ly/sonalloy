use assert_cmd::Command;
use tempfile::tempdir;

fn positive_zero_crossings(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|window| window[0] <= 0.0 && window[1] > 0.0)
        .count()
}

#[test]
fn render_sine_writes_stereo_wav() {
    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("sine.wav");
    let mut command = Command::cargo_bin("sonalloy").expect("binary");
    command
        .args([
            "dev",
            "render-sine",
            "--frequency",
            "440",
            "--duration",
            "1.0",
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--output",
            output.to_str().expect("utf-8 path"),
        ])
        .assert()
        .success();

    let mut reader = hound::WavReader::open(&output).expect("wav output");
    let spec = reader.spec();
    assert_eq!(spec.channels, 2);
    assert_eq!(spec.sample_rate, 48_000);
    let samples: Vec<f32> = reader
        .samples::<f32>()
        .map(|sample| sample.expect("valid sample"))
        .collect();
    assert_eq!(samples.len(), 96_000);
    assert!(samples.iter().all(|sample| sample.is_finite()));
    assert!(samples.iter().any(|sample| sample.abs() > 0.5));
    assert!(samples.iter().all(|sample| sample.abs() <= 1.1));
    let peak = samples
        .iter()
        .map(|sample| f64::from(sample.abs()))
        .fold(0.0_f64, f64::max);
    let rms = (samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / f64::from(u32::try_from(samples.len()).expect("sample count fits in u32")))
    .sqrt();
    let dc = samples.iter().map(|sample| f64::from(*sample)).sum::<f64>()
        / f64::from(u32::try_from(samples.len()).expect("sample count fits in u32"));
    let left: Vec<f32> = samples.iter().step_by(2).copied().collect();
    let crossing_count =
        u32::try_from(positive_zero_crossings(&left)).expect("crossing count fits in u32");
    let frame_count = u32::try_from(left.len()).expect("frame count fits in u32");
    let estimated_frequency = f64::from(crossing_count) * 48_000.0 / f64::from(frame_count);
    assert!((estimated_frequency - 440.0).abs() < 1.0);

    let expected_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/expected/sine_metrics.json");
    let expected: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(expected_path).expect("expected metrics fixture"),
    )
    .expect("valid expected metrics fixture");
    assert_eq!(
        expected["sample_rate"].as_u64(),
        Some(u64::from(spec.sample_rate))
    );
    assert_eq!(
        expected["channels"].as_u64(),
        Some(u64::from(spec.channels))
    );
    assert_eq!(
        expected["frames"].as_u64(),
        Some(u64::from(
            u32::try_from(left.len()).expect("frame count fits in u32")
        ))
    );
    assert_eq!(expected["finite"].as_bool(), Some(true));
    assert!((peak - expected["peak"].as_f64().expect("peak metric")).abs() < 1.0e-6);
    assert!((rms - expected["rms"].as_f64().expect("rms metric")).abs() < 1.0e-6);
    assert!((dc - expected["dc"].as_f64().expect("dc metric")).abs() < 1.0e-6);
    assert!(
        (estimated_frequency
            - expected["estimated_frequency_hz"]
                .as_f64()
                .expect("frequency metric"))
        .abs()
            < 1.0
    );
}

#[test]
fn invalid_request_returns_machine_readable_error() {
    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("sine.wav");
    let mut command = Command::cargo_bin("sonalloy").expect("binary");
    command
        .args([
            "dev",
            "render-sine",
            "--duration",
            "1.0",
            "--sample-rate",
            "48000",
            "--block-size",
            "0",
            "--output",
            output.to_str().expect("utf-8 path"),
            "--json",
        ])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("\"status\":\"error\""))
        .stdout(predicates::str::contains("\"VALUE_OUT_OF_RANGE\""));
    assert!(!output.exists());
}

#[test]
fn missing_output_directory_returns_wav_error() {
    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("missing").join("sine.wav");
    let mut command = Command::cargo_bin("sonalloy").expect("binary");
    command
        .args([
            "dev",
            "render-sine",
            "--duration",
            "0.01",
            "--sample-rate",
            "48000",
            "--block-size",
            "64",
            "--output",
            output.to_str().expect("utf-8 path"),
            "--json",
        ])
        .assert()
        .code(4)
        .stdout(predicates::str::contains("\"WAV_OUTPUT_ERROR\""));
    assert!(!output.exists());
}

#[test]
fn output_directory_used_as_file_returns_wav_error() {
    let directory = tempdir().expect("temporary directory");
    let output = directory.path();
    let mut command = Command::cargo_bin("sonalloy").expect("binary");
    command
        .args([
            "dev",
            "render-sine",
            "--duration",
            "0.01",
            "--sample-rate",
            "48000",
            "--block-size",
            "64",
            "--output",
            output.to_str().expect("utf-8 path"),
            "--json",
        ])
        .assert()
        .code(4)
        .stdout(predicates::str::contains("\"WAV_OUTPUT_ERROR\""));
}

#[test]
fn invalid_sample_rate_returns_error() {
    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("sine.wav");
    let mut command = Command::cargo_bin("sonalloy").expect("binary");
    command
        .args([
            "dev",
            "render-sine",
            "--duration",
            "0.01",
            "--sample-rate",
            "0",
            "--output",
            output.to_str().expect("utf-8 path"),
            "--json",
        ])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("\"VALUE_OUT_OF_RANGE\""));
    assert!(!output.exists());
}

#[test]
fn invalid_frequency_is_not_reported_as_success() {
    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("sine.wav");
    let mut command = Command::cargo_bin("sonalloy").expect("binary");
    command
        .args([
            "dev",
            "render-sine",
            "--duration",
            "0.01",
            "--frequency",
            "30000",
            "--sample-rate",
            "48000",
            "--output",
            output.to_str().expect("utf-8 path"),
            "--json",
        ])
        .assert()
        .code(3)
        .stdout(predicates::str::contains("\"status\":\"error\""));
    assert!(!output.exists());
}
