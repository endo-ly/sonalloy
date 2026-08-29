mod support;

use assert_cmd::Command;
use midly::{Format, Header, MidiMessage, PitchBend, Smf, Timing, TrackEvent, TrackEventKind};
use support::fixture_path;
use tempfile::tempdir;

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

fn write_sustain_midi(directory: &std::path::Path) -> std::path::PathBuf {
    let path = directory.join("sustain.mid");
    let mut smf = Smf::new(Header::new(
        Format::SingleTrack,
        Timing::Metrical(480.into()),
    ));
    smf.tracks.push(vec![
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::NoteOn {
                    key: 60.into(),
                    vel: 100.into(),
                },
            },
        },
        TrackEvent {
            delta: 240.into(),
            kind: TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::Controller {
                    controller: 64.into(),
                    value: 127.into(),
                },
            },
        },
        TrackEvent {
            delta: 240.into(),
            kind: TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::NoteOff {
                    key: 60.into(),
                    vel: 0.into(),
                },
            },
        },
        TrackEvent {
            delta: 240.into(),
            kind: TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::Controller {
                    controller: 64.into(),
                    value: 0.into(),
                },
            },
        },
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
        },
    ]);
    smf.save(&path).expect("sustain MIDI fixture");
    path
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
            fixture_path("instruments/basic-poly-synth.json")
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
fn render_note_reports_analysis_and_trace_without_changing_audio() {
    let directory = tempdir().expect("temporary directory");
    let plain_output = directory.path().join("plain.wav");
    let traced_output = directory.path().join("traced.wav");
    let definition = fixture_path("instruments/basic-poly-synth.json");
    let common_args = [
        "render",
        "note",
        definition.to_str().expect("definition path"),
        "--gate",
        "0.02",
        "--tail",
        "0.02",
        "--sample-rate",
        "48000",
        "--block-size",
        "257",
    ];

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(common_args)
        .args([
            "--output",
            plain_output.to_str().expect("plain output path"),
        ])
        .assert()
        .success();

    let traced = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(common_args)
        .args([
            "--analyze",
            "--trace",
            "layer.body.gain",
            "--trace",
            "voice.processor.tone.cutoff",
            "--trace-every-frames",
            "480",
            "--output",
            traced_output.to_str().expect("traced output path"),
            "--json",
        ])
        .output()
        .expect("traced render starts");
    assert!(traced.status.success());

    let report: serde_json::Value = serde_json::from_slice(&traced.stdout).expect("JSON report");
    assert_eq!(report["status"], "ok");
    assert_eq!(report["analysis"]["finite"], true);
    assert!(report["analysis"]["level"]["rms"].is_number());
    assert_eq!(
        report["trace"]["parameters"].as_array().map(Vec::len),
        Some(2)
    );
    for parameter in report["trace"]["parameters"]
        .as_array()
        .expect("trace parameters")
    {
        assert!(
            !parameter["observations"]
                .as_array()
                .expect("trace observations")
                .is_empty()
        );
    }

    let plain_samples = hound::WavReader::open(plain_output)
        .expect("plain WAV")
        .into_samples::<f32>()
        .map(|sample| sample.expect("plain sample"))
        .collect::<Vec<_>>();
    let traced_samples = hound::WavReader::open(traced_output)
        .expect("traced WAV")
        .into_samples::<f32>()
        .map(|sample| sample.expect("traced sample"))
        .collect::<Vec<_>>();
    assert_eq!(plain_samples.len(), traced_samples.len());
    assert!(
        plain_samples
            .iter()
            .zip(traced_samples)
            .all(|(plain, traced)| (plain - traced).abs() <= 1.0e-5)
    );
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
            {"absolute_frame": 128, "type": "parameter_change", "parameter": "layer.body.gain", "native_value": 6.8},
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
            fixture_path("instruments/basic-poly-synth.json")
                .to_str()
                .expect("definition path"),
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
fn render_events_accepts_sustain_pedal_events() {
    let directory = tempdir().expect("temporary directory");
    let events = directory.path().join("sustain-events.json");
    std::fs::write(
        &events,
        r#"{
          "events": [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 100},
            {"absolute_frame": 128, "type": "sustain_pedal", "down": true},
            {"absolute_frame": 256, "type": "note_off", "note_id": 1},
            {"absolute_frame": 384, "type": "sustain_pedal", "down": false}
          ]
        }"#,
    )
    .expect("sustain event sequence fixture");
    let output = directory.path().join("sustain-events.wav");

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "events",
            fixture_path("instruments/basic-poly-synth.json")
                .to_str()
                .expect("definition path"),
            events.to_str().expect("events path"),
            "--duration-frames",
            "512",
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
}

#[test]
fn render_events_reset_check_reports_a_bit_exact_comparison() {
    let directory = tempdir().expect("temporary directory");
    let events = directory.path().join("events.json");
    std::fs::write(
        &events,
        r#"{
          "events": [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 100},
            {"absolute_frame": 512, "type": "note_off", "note_id": 1}
          ]
        }"#,
    )
    .expect("event sequence fixture");
    let output = directory.path().join("reset-check.wav");
    let report = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "events",
            fixture_path("instruments/basic-poly-synth.json")
                .to_str()
                .expect("definition path"),
            events.to_str().expect("events path"),
            "--duration-frames",
            "1024",
            "--tail",
            "0",
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--reset-check",
            "--output",
            output.to_str().expect("output path"),
            "--json",
        ])
        .output()
        .expect("reset-check render starts");
    assert!(report.status.success());
    let report: serde_json::Value = serde_json::from_slice(&report.stdout).expect("JSON report");
    assert_eq!(report["status"], "ok");
    assert_eq!(report["reset_comparison"]["compatible"], true);
    assert_eq!(report["reset_comparison"]["max_abs_difference"], 0.0);
    assert_eq!(report["reset_comparison"]["rms_difference"], 0.0);
    assert_eq!(report["reset_comparison"]["different_sample_count"], 0);
    assert!(output.exists());
}

#[test]
fn render_events_rejects_an_unknown_parameter_before_rendering() {
    let directory = tempdir().expect("temporary directory");
    let events = directory.path().join("events.json");
    std::fs::write(
        &events,
        r#"{"events":[{"absolute_frame":0,"type":"parameter_change","parameter":"layer.missing.gain","native_value":-24.0}]}"#,
    )
    .expect("event sequence fixture");
    let output = directory.path().join("events.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "events",
            fixture_path("instruments/basic-poly-synth.json")
                .to_str()
                .expect("definition path"),
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
            fixture_path("instruments/basic-poly-synth.json")
                .to_str()
                .expect("definition path"),
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
            fixture_path("instruments/basic-poly-synth.json")
                .to_str()
                .expect("utf-8 definition path"),
            fixture_path("midi/basic-poly-synth-phrase.mid")
                .to_str()
                .expect("utf-8 MIDI path"),
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
            fixture_path("instruments/expressive-hybrid-lead.json")
                .to_str()
                .expect("definition path"),
            fixture_path("midi/expressive-hybrid-controls.mid")
                .to_str()
                .expect("MIDI path"),
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
fn render_midi_converts_sustain_cc64() {
    let directory = tempdir().expect("temporary directory");
    let midi = write_sustain_midi(directory.path());
    let output = directory.path().join("sustain.wav");

    let report = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "midi",
            fixture_path("instruments/basic-poly-synth.json")
                .to_str()
                .expect("definition path"),
            midi.to_str().expect("MIDI path"),
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
        .output()
        .expect("sustain MIDI render starts");
    assert!(report.status.success());
    let report: serde_json::Value = serde_json::from_slice(&report.stdout).expect("JSON report");
    assert_eq!(report["status"], "ok");
    assert!(report.get("diagnostics").is_none());
    assert!(output.exists());
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
            fixture_path("instruments/basic-poly-synth.json")
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
fn render_note_rejects_midi_values_above_127() {
    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("note.wav");
    for argument in [["--note", "128"], ["--velocity", "128"]] {
        Command::cargo_bin("sonalloy")
            .expect("binary")
            .args([
                "render",
                "note",
                fixture_path("instruments/basic-poly-synth.json")
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
