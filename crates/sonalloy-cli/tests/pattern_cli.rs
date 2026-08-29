mod support;

use assert_cmd::Command;
use midly::{Format, Smf, Timing};
use midly::{Header, MidiMessage, TrackEvent, TrackEventKind};
use serde_json::json;
use support::*;
use tempfile::tempdir;

fn init_pattern(path: &std::path::Path) {
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["pattern", "init", path.to_str().expect("utf-8 path")])
        .assert()
        .success();
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

fn write_multi_channel_midi(directory: &std::path::Path) -> std::path::PathBuf {
    let path = directory.join("multi-channel.mid");
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
                    key: 36.into(),
                    vel: 100.into(),
                },
            },
        },
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Midi {
                channel: 1.into(),
                message: MidiMessage::NoteOn {
                    key: 60.into(),
                    vel: 100.into(),
                },
            },
        },
        TrackEvent {
            delta: 480.into(),
            kind: TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::NoteOff {
                    key: 36.into(),
                    vel: 0.into(),
                },
            },
        },
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Midi {
                channel: 1.into(),
                message: MidiMessage::NoteOff {
                    key: 60.into(),
                    vel: 0.into(),
                },
            },
        },
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
        },
    ]);
    smf.save(&path).expect("multi-channel MIDI fixture");
    path
}

#[test]
fn pattern_commands_initialize_validate_inspect_and_render() {
    let directory = tempdir().expect("temporary directory");
    let pattern = directory.path().join("groove.json");
    let rendered = directory.path().join("groove.wav");
    init_pattern(&pattern);
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "pattern",
            "validate",
            pattern.to_str().expect("pattern path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"status\":\"ok\""));
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "pattern",
            "inspect",
            pattern.to_str().expect("pattern path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"note_count\":1"))
        .stdout(predicates::str::contains(
            "\"musical_duration_seconds\":2.0",
        ));

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "pattern",
            fixture_path("instruments/basic-poly-synth.json")
                .to_str()
                .expect("definition path"),
            pattern.to_str().expect("pattern path"),
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--tail",
            "0",
            "--output",
            rendered.to_str().expect("rendered path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"status\":\"ok\""));
    let mut reader = hound::WavReader::open(&rendered).expect("pattern render output");
    assert_eq!(reader.spec().channels, 2);
    let samples = reader
        .samples::<f32>()
        .map(|sample| sample.expect("valid pattern sample"))
        .collect::<Vec<_>>();
    assert!(samples.iter().all(|sample| sample.is_finite()));
    assert!(samples.iter().any(|sample| sample.abs() > 0.01));
}

#[test]
fn pattern_commands_export_and_import_midi() {
    let directory = tempdir().expect("temporary directory");
    let pattern = directory.path().join("groove.json");
    let midi = directory.path().join("groove.mid");
    let imported = directory.path().join("imported.json");
    let source = json!({
        "schema_version": 1,
        "name": null,
        "ticks_per_beat": 480,
        "length_ticks": 1920,
        "tempo_changes": [{"tick": 0, "bpm": 120.0}],
        "time_signature_changes": [
            {"tick": 0, "numerator": 4, "denominator": 4},
            {"tick": 960, "numerator": 3, "denominator": 4}
        ],
        "events": [
            {"type": "note", "tick": 0, "duration_ticks": 480, "note": 60, "velocity": 100},
            {"type": "note", "tick": 480, "duration_ticks": 240, "note": 64, "velocity": 80},
            {"type": "sustain_pedal", "tick": 720, "down": true},
            {"type": "sustain_pedal", "tick": 960, "down": false}
        ]
    });
    std::fs::write(
        &pattern,
        serde_json::to_vec_pretty(&source).expect("round-trip pattern serializes"),
    )
    .expect("round-trip pattern writes");

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "pattern",
            "export-midi",
            pattern.to_str().expect("pattern path"),
            "--output",
            midi.to_str().expect("MIDI path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "\"command\":\"pattern export-midi\"",
        ));
    let midi_bytes = std::fs::read(&midi).expect("exported MIDI");
    let midi_file = Smf::parse(&midi_bytes).expect("exported MIDI parses");
    assert_eq!(midi_file.header.format, Format::SingleTrack);
    assert!(matches!(
        midi_file.header.timing,
        Timing::Metrical(ticks) if u16::from(ticks) == 480
    ));
    assert_eq!(midi_file.tracks.len(), 1);

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "pattern",
            "import-midi",
            midi.to_str().expect("MIDI path"),
            "--output",
            imported.to_str().expect("imported pattern path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "\"command\":\"pattern import-midi\"",
        ));
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "pattern",
            "validate",
            imported.to_str().expect("imported pattern path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"status\":\"ok\""));

    let imported_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&imported).expect("imported pattern JSON"))
            .expect("imported pattern parses");
    assert_eq!(imported_json["ticks_per_beat"], source["ticks_per_beat"]);
    assert_eq!(imported_json["length_ticks"], source["length_ticks"]);
    assert_eq!(imported_json["tempo_changes"], source["tempo_changes"]);
    assert_eq!(
        imported_json["time_signature_changes"],
        source["time_signature_changes"]
    );
    assert_eq!(imported_json["events"], source["events"]);
}

#[test]
fn pattern_export_rejects_overlapping_same_pitch_notes() {
    let directory = tempdir().expect("temporary directory");
    let pattern = directory.path().join("overlap.json");
    let output = directory.path().join("overlap.mid");
    let pattern_json = json!({
        "schema_version": 1,
        "name": null,
        "ticks_per_beat": 480,
        "length_ticks": 960,
        "tempo_changes": [{"tick": 0, "bpm": 120.0}],
        "time_signature_changes": [{"tick": 0, "numerator": 4, "denominator": 4}],
        "events": [
            {"type": "note", "tick": 0, "duration_ticks": 720, "note": 60, "velocity": 100},
            {"type": "note", "tick": 480, "duration_ticks": 240, "note": 60, "velocity": 80}
        ]
    });
    std::fs::write(
        &pattern,
        serde_json::to_vec_pretty(&pattern_json).expect("overlap pattern serializes"),
    )
    .expect("overlap pattern writes");

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "pattern",
            "export-midi",
            pattern.to_str().expect("pattern path"),
            "--output",
            output.to_str().expect("MIDI path"),
            "--json",
        ])
        .assert()
        .code(2)
        .stdout(predicates::str::contains(
            "notes with the same pitch cannot overlap in Standard MIDI",
        ));
    assert!(!output.exists());
}

#[test]
fn pattern_import_preserves_supported_controls_and_export_rejects_parameters() {
    let directory = tempdir().expect("temporary directory");
    let midi = write_sustain_midi(directory.path());
    let imported = directory.path().join("sustain.json");

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "pattern",
            "import-midi",
            midi.to_str().expect("MIDI path"),
            "--output",
            imported.to_str().expect("pattern path"),
        ])
        .assert()
        .success();
    let imported_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&imported).expect("imported pattern JSON"))
            .expect("imported pattern parses");
    let events = imported_json["events"].as_array().expect("event array");
    assert!(events.iter().any(|event| event["type"] == "sustain_pedal"));
    assert!(events.iter().any(|event| event["type"] == "note"));

    let parameter_pattern = directory.path().join("parameter.json");
    std::fs::write(
        &parameter_pattern,
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "name": "parameter",
            "ticks_per_beat": 480,
            "length_ticks": 480,
            "tempo_changes": [{"tick": 0, "bpm": 120.0}],
            "time_signature_changes": [{"tick": 0, "numerator": 4, "denominator": 4}],
            "events": [
                {"type": "note", "tick": 0, "duration_ticks": 240, "note": 60, "velocity": 100},
                {"type": "parameter_change", "tick": 0, "parameter": "layer.body.gain", "native_value": 0.0}
            ]
        }))
        .expect("parameter pattern serializes"),
    )
    .expect("parameter pattern writes");
    let output = directory.path().join("parameter.mid");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "pattern",
            "export-midi",
            parameter_pattern.to_str().expect("pattern path"),
            "--output",
            output.to_str().expect("MIDI path"),
            "--json",
        ])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("\"MIDI_ERROR\""));
    assert!(!output.exists());
}

#[test]
fn pattern_import_requires_a_channel_for_multi_channel_midi() {
    let directory = tempdir().expect("temporary directory");
    let midi = write_multi_channel_midi(directory.path());
    let automatic = directory.path().join("automatic.json");
    let selected = directory.path().join("selected.json");

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "pattern",
            "import-midi",
            midi.to_str().expect("MIDI path"),
            "--output",
            automatic.to_str().expect("pattern path"),
        ])
        .assert()
        .code(2)
        .stderr(predicates::str::contains("specify --channel"));
    assert!(!automatic.exists());

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "pattern",
            "import-midi",
            midi.to_str().expect("MIDI path"),
            "--channel",
            "2",
            "--output",
            selected.to_str().expect("pattern path"),
        ])
        .assert()
        .success();
    let selected_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&selected).expect("selected pattern JSON"))
            .expect("selected pattern parses");
    assert_eq!(selected_json["events"][0]["note"], 60);
}
