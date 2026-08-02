use assert_cmd::Command;
use midly::{Format, Header, MidiMessage, PitchBend, Smf, Timing, TrackEvent, TrackEventKind};
use tempfile::tempdir;

fn reference_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/basic-poly-synth.json")
}

fn reference_midi() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/midi/basic-poly-synth-phrase.mid")
}

fn hybrid_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/metallic-hybrid.json")
}

fn hybrid_midi() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/midi/metallic-hybrid-phrase.mid")
}

fn expressive_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/expressive-hybrid-lead.json")
}

fn expressive_midi() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/midi/expressive-hybrid-controls.mid")
}

fn missing_asset_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/metallic-hybrid-missing-asset.json")
}

fn positive_zero_crossings(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|window| window[0] <= 0.0 && window[1] > 0.0)
        .count()
}

fn write_control_only_midi(directory: &std::path::Path) -> std::path::PathBuf {
    let path = directory.join("control-only.mid");
    let mut smf = Smf::new(Header::new(
        Format::SingleTrack,
        Timing::Metrical(480.into()),
    ));
    smf.tracks.push(vec![
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::PitchBend {
                    bend: PitchBend::from_int(0),
                },
            },
        },
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
        },
    ]);
    smf.save(&path).expect("control-only MIDI fixture");
    path
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

#[test]
fn instrument_init_validate_and_inspect_are_available() {
    let directory = tempdir().expect("temporary directory");
    let definition = directory.path().join("init.json");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "init",
            definition.to_str().expect("utf-8 path"),
        ])
        .assert()
        .success();
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "validate",
            definition.to_str().expect("utf-8 path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"status\":\"ok\""));
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "inspect",
            definition.to_str().expect("utf-8 path"),
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("polyphony: 16"))
        .stdout(predicates::str::contains("layer body"))
        .stdout(predicates::str::contains("envelope:"))
        .stdout(predicates::str::contains("parameter layer.body.gain:"))
        .stdout(predicates::str::contains("asset: not_applicable"));

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "inspect",
            definition.to_str().expect("utf-8 definition path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"metadata\""))
        .stdout(predicates::str::contains("\"envelope\""))
        .stdout(predicates::str::contains("\"parameters\""))
        .stdout(predicates::str::contains("layer.body.gain"))
        .stdout(predicates::str::contains("\"phase_reset\":true"))
        .stdout(predicates::str::contains("\"asset_status\""));
}

#[test]
fn inspect_lists_external_modulation_sources() {
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "inspect",
            expressive_definition()
                .to_str()
                .expect("utf-8 definition path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"id\":\"pitch_bend\""))
        .stdout(predicates::str::contains("\"id\":\"mod_wheel\""))
        .stdout(predicates::str::contains("\"id\":\"aftertouch\""))
        .stdout(predicates::str::contains("\"scope\":\"instrument\""))
        .stdout(predicates::str::contains("\"kind\":\"external_control\""));
}

#[test]
fn render_note_uses_the_compiled_instrument() {
    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("note.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "note",
            reference_definition()
                .to_str()
                .expect("utf-8 definition path"),
            "--gate",
            "0.01",
            "--tail",
            "0.01",
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--output",
            output.to_str().expect("utf-8 output path"),
        ])
        .assert()
        .success();
    let reader = hound::WavReader::open(output).expect("rendered WAV");
    assert_eq!(reader.spec().channels, 2);
}

#[test]
fn render_events_supports_parameter_and_external_control_events() {
    let directory = tempdir().expect("temporary directory");
    let events = directory.path().join("events.json");
    std::fs::write(
        &events,
        r#"{
          "events": [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 100},
            {"absolute_frame": 128, "type": "parameter_change", "parameter": "layer.body.gain", "normalized": 0.9},
            {"absolute_frame": 256, "type": "pitch_bend", "value": 0.5},
            {"absolute_frame": 384, "type": "mod_wheel", "value": 1.0},
            {"absolute_frame": 512, "type": "aftertouch", "value": 0.75},
            {"absolute_frame": 768, "type": "note_off", "note_id": 1}
          ]
        }"#,
    )
    .expect("event sequence fixture");
    let output = directory.path().join("events.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "events",
            reference_definition().to_str().expect("definition path"),
            events.to_str().expect("events path"),
            "--duration-frames",
            "1024",
            "--tail",
            "0",
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--output",
            output.to_str().expect("output path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"status\":\"ok\""));
    let mut reader = hound::WavReader::open(output).expect("event render output");
    assert_eq!(reader.duration(), 1024);
    assert!(
        reader
            .samples::<f32>()
            .map(|sample| sample.expect("valid sample"))
            .all(f32::is_finite)
    );
}

#[test]
fn render_events_rejects_an_unknown_parameter_before_rendering() {
    let directory = tempdir().expect("temporary directory");
    let events = directory.path().join("events.json");
    std::fs::write(
        &events,
        r#"{"events":[{"absolute_frame":0,"type":"parameter_change","parameter":"layer.missing.gain","normalized":0.5}]}"#,
    )
    .expect("event sequence fixture");
    let output = directory.path().join("events.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "events",
            reference_definition().to_str().expect("definition path"),
            events.to_str().expect("events path"),
            "--duration-frames",
            "128",
            "--tail",
            "0",
            "--output",
            output.to_str().expect("output path"),
            "--json",
        ])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("\"PARAMETER_NOT_FOUND\""));
    assert!(!output.exists());
}

#[test]
fn render_events_rejects_descending_absolute_frames_before_rendering() {
    let directory = tempdir().expect("temporary directory");
    let events = directory.path().join("events.json");
    std::fs::write(
        &events,
        r#"{
          "events": [
            {"absolute_frame": 128, "type": "note_on", "note_id": 1, "note": 60, "velocity": 100},
            {"absolute_frame": 64, "type": "note_off", "note_id": 1}
          ]
        }"#,
    )
    .expect("event sequence fixture");
    let output = directory.path().join("events.wav");

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "events",
            reference_definition().to_str().expect("definition path"),
            events.to_str().expect("events path"),
            "--duration-frames",
            "256",
            "--tail",
            "0",
            "--output",
            output.to_str().expect("output path"),
            "--json",
        ])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("\"EVENT_ORDER_INVALID\""));
    assert!(!output.exists());
}

#[test]
fn render_midi_converts_tempo_and_note_events() {
    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("midi.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "midi",
            reference_definition()
                .to_str()
                .expect("utf-8 definition path"),
            reference_midi().to_str().expect("utf-8 MIDI path"),
            "--tail",
            "0.01",
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--output",
            output.to_str().expect("utf-8 output path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"status\":\"ok\""));
    let reader = hound::WavReader::open(output).expect("rendered MIDI WAV");
    assert!(reader.duration() > 0);
}

#[test]
fn render_midi_converts_external_controls() {
    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("external-controls.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "midi",
            expressive_definition().to_str().expect("definition path"),
            expressive_midi().to_str().expect("MIDI path"),
            "--tail",
            "0.5",
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--output",
            output.to_str().expect("output path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"status\":\"ok\""));
    let mut reader = hound::WavReader::open(output).expect("external control WAV");
    let samples: Vec<f32> = reader
        .samples()
        .map(|sample| sample.expect("finite sample"))
        .collect();
    assert!(samples.iter().all(|sample| sample.is_finite()));
    assert!(samples.iter().any(|sample| sample.abs() > 0.01));
}

#[test]
fn render_midi_rejects_control_only_input() {
    let directory = tempdir().expect("temporary directory");
    let midi = write_control_only_midi(directory.path());
    let output = directory.path().join("control-only.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "midi",
            reference_definition()
                .to_str()
                .expect("utf-8 definition path"),
            midi.to_str().expect("utf-8 MIDI path"),
            "--sample-rate",
            "48000",
            "--block-size",
            "64",
            "--output",
            output.to_str().expect("utf-8 output path"),
            "--json",
        ])
        .assert()
        .code(2)
        .stdout(predicates::str::contains(
            "MIDI file contains no note events",
        ));
    assert!(!output.exists());
}

#[test]
fn invalid_definition_returns_exit_code_one() {
    let directory = tempdir().expect("temporary directory");
    let definition = directory.path().join("invalid.json");
    std::fs::write(&definition, "{\"schema_version\": 1}").expect("write invalid JSON");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "validate",
            definition.to_str().expect("utf-8 path"),
            "--json",
        ])
        .assert()
        .code(1)
        .stdout(predicates::str::contains("\"status\":\"error\""));
}

#[test]
fn invalid_definition_json_has_a_specific_diagnostic_code() {
    let directory = tempdir().expect("temporary directory");
    let definition = directory.path().join("invalid.json");
    std::fs::write(&definition, "not json").expect("write invalid JSON");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "validate",
            definition.to_str().expect("utf-8 definition path"),
            "--json",
        ])
        .assert()
        .code(1)
        .stdout(predicates::str::contains("\"JSON_INVALID\""));
}

#[test]
fn missing_definition_field_has_a_specific_diagnostic_code() {
    let directory = tempdir().expect("temporary directory");
    let definition = directory.path().join("missing.json");
    std::fs::write(&definition, "{\"schema_version\":1}").expect("write incomplete JSON");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "validate",
            definition.to_str().expect("utf-8 definition path"),
            "--json",
        ])
        .assert()
        .code(1)
        .stdout(predicates::str::contains("\"REQUIRED_FIELD_MISSING\""));
}

#[test]
fn render_note_rejects_midi_values_above_127() {
    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("note.wav");
    for argument in [["--note", "128"], ["--velocity", "128"]] {
        Command::cargo_bin("sonalloy")
            .expect("binary")
            .args([
                "render",
                "note",
                reference_definition()
                    .to_str()
                    .expect("utf-8 definition path"),
                argument[0],
                argument[1],
                "--gate",
                "0.01",
                "--output",
                output.to_str().expect("utf-8 output path"),
                "--json",
            ])
            .assert()
            .code(2)
            .stdout(predicates::str::contains("\"VALUE_OUT_OF_RANGE\""));
    }
}

#[test]
fn hybrid_validate_and_inspect_report_sample_layers() {
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "validate",
            hybrid_definition().to_str().expect("utf-8 definition path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"ASSET_RESAMPLED\""));

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "inspect",
            hybrid_definition().to_str().expect("utf-8 definition path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"layer_count\":2"))
        .stdout(predicates::str::contains("\"kind\":\"sample\""))
        .stdout(predicates::str::contains("\"asset_status\":\"enabled\""));
}

#[test]
fn hybrid_midi_render_writes_stereo_audio() {
    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("hybrid.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "midi",
            hybrid_definition().to_str().expect("utf-8 definition path"),
            hybrid_midi().to_str().expect("utf-8 MIDI path"),
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--tail",
            "0.5",
            "--output",
            output.to_str().expect("utf-8 output path"),
        ])
        .assert()
        .success();
    let mut reader = hound::WavReader::open(output).expect("hybrid WAV");
    assert_eq!(reader.spec().channels, 2);
    let samples: Vec<f32> = reader
        .samples()
        .map(|sample| sample.expect("finite sample"))
        .collect();
    assert!(samples.iter().all(|sample| sample.is_finite()));
    assert!(samples.iter().any(|sample| sample.abs() > 0.01));
}

#[test]
fn missing_asset_is_a_warning_and_body_still_renders() {
    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("fallback.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "midi",
            missing_asset_definition()
                .to_str()
                .expect("utf-8 definition path"),
            hybrid_midi().to_str().expect("utf-8 MIDI path"),
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--tail",
            "0.5",
            "--output",
            output.to_str().expect("utf-8 output path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"ASSET_NOT_FOUND\""));
    assert!(hound::WavReader::open(output).is_ok());
}
