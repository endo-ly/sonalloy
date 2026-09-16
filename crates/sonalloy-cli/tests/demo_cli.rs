mod support;

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use midly::{Format, Smf};
use serde_json::{Value, json};
use support::fixture_path;
use tempfile::TempDir;

struct DemoFixture {
    _directory: TempDir,
    demo: PathBuf,
    second_instrument: PathBuf,
    second_pattern: PathBuf,
}

fn demo_fixture() -> DemoFixture {
    let directory = tempfile::tempdir().expect("temporary directory");
    let song = directory.path().join("song");
    let instruments = song.join("instruments");
    let patterns = song.join("patterns");
    std::fs::create_dir_all(&instruments).expect("instrument directory");
    std::fs::create_dir_all(&patterns).expect("pattern directory");
    let first_instrument = instruments.join("first.json");
    let second_instrument = instruments.join("second.json");
    std::fs::copy(
        fixture_path("instruments/basic-poly-synth.json"),
        &first_instrument,
    )
    .expect("first instrument");
    std::fs::copy(
        fixture_path("instruments/basic-poly-synth.json"),
        &second_instrument,
    )
    .expect("second instrument");
    let first_pattern = patterns.join("first.json");
    let second_pattern = patterns.join("second.json");
    write_pattern(&first_pattern, 960, 480, 60);
    write_pattern(&second_pattern, 480, 240, 67);
    let demo = song.join("demo.json");
    std::fs::write(
        &demo,
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "name": "Test Demo",
            "parts": [
                {
                    "id": "first-part",
                    "instrument": "instruments/first.json",
                    "pattern": "patterns/first.json",
                    "midi_channel": 2
                },
                {
                    "id": "second.part",
                    "instrument": "instruments/second.json",
                    "pattern": "patterns/second.json",
                    "gain_db": -6.0
                }
            ],
            "mix": {"fade_out_seconds": 0.0}
        }))
        .expect("Demo JSON"),
    )
    .expect("Demo file");
    DemoFixture {
        _directory: directory,
        demo,
        second_instrument,
        second_pattern,
    }
}

fn write_pattern(path: &Path, length_ticks: u64, duration_ticks: u64, note: u8) {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "name": null,
            "ticks_per_beat": 480,
            "length_ticks": length_ticks,
            "tempo_changes": [{"tick": 0, "bpm": 120.0}],
            "time_signature_changes": [{"tick": 0, "numerator": 4, "denominator": 4}],
            "events": [{"type": "note", "tick": 0, "duration_ticks": duration_ticks, "note": note, "velocity": 100}]
        }))
        .expect("pattern JSON"),
    )
    .expect("pattern file");
}

fn json_report(output: &std::process::Output) -> Value {
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).expect("JSON report")
}

#[test]
fn demo_validate_and_inspect_use_relative_references_and_longest_pattern() {
    let fixture = demo_fixture();

    let validation = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "demo",
            "validate",
            fixture.demo.to_str().expect("Demo path"),
            "--json",
        ])
        .output()
        .expect("validation starts");
    let validation_report = json_report(&validation);
    assert_eq!(validation_report["status"], "ok");
    assert_eq!(validation_report["command"], "demo validate");
    assert!(validation_report.get("diagnostics").is_none());

    let inspection = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "demo",
            "inspect",
            fixture.demo.to_str().expect("Demo path"),
            "--json",
        ])
        .output()
        .expect("inspection starts");
    let inspection_report = json_report(&inspection);
    assert_eq!(inspection_report["part_count"], 2);
    assert_eq!(inspection_report["ticks_per_beat"], 480);
    assert_eq!(inspection_report["length_ticks"], 960);
    assert_eq!(inspection_report["musical_duration_seconds"], 1.0);
    assert_eq!(inspection_report["parts"][0]["midi_channel"], 2);
    assert_eq!(inspection_report["parts"][1]["midi_channel"], 1);
    assert_eq!(inspection_report["ffmpeg_required"], false);
}

#[test]
fn demo_export_midi_writes_conductor_and_part_tracks() {
    let fixture = demo_fixture();
    let output = fixture.demo.with_file_name("song.mid");
    let report = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "demo",
            "export-midi",
            fixture.demo.to_str().expect("Demo path"),
            "--output",
            output.to_str().expect("MIDI output"),
            "--json",
        ])
        .output()
        .expect("MIDI export starts");
    let report = json_report(&report);
    assert_eq!(report["command"], "demo export-midi");

    let bytes = std::fs::read(output).expect("MIDI output");
    let smf = Smf::parse(&bytes).expect("Type 1 MIDI");
    assert_eq!(smf.header.format, Format::Parallel);
    assert_eq!(smf.tracks.len(), 3);
    assert!(smf.tracks[0].iter().any(|event| {
        matches!(
            event.kind,
            midly::TrackEventKind::Meta(midly::MetaMessage::TrackName(name))
                if name == b"Test Demo"
        )
    }));
    for (track, name) in smf.tracks[1..]
        .iter()
        .zip([&b"first-part"[..], &b"second.part"[..]])
    {
        assert!(track.iter().any(|event| {
            matches!(
                event.kind,
                midly::TrackEventKind::Meta(midly::MetaMessage::TrackName(actual))
                    if actual == name
            )
        }));
    }
    assert!(smf.tracks[1].iter().any(|event| {
        matches!(
            event.kind,
            midly::TrackEventKind::Midi { channel, .. } if channel.as_int() == 1
        )
    }));
    assert!(smf.tracks[2].iter().any(|event| {
        matches!(
            event.kind,
            midly::TrackEventKind::Midi { channel, .. } if channel.as_int() == 0
        )
    }));
}

#[test]
fn render_demo_writes_stems_before_gain_and_reports_mix() {
    let fixture = demo_fixture();
    let output = fixture.demo.with_file_name("song.wav");
    let stems = fixture.demo.with_file_name("stems");
    let report = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "demo",
            fixture.demo.to_str().expect("Demo path"),
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--tail",
            "0",
            "--stems-dir",
            stems.to_str().expect("stems directory"),
            "--analyze",
            "--output",
            output.to_str().expect("WAV output"),
            "--json",
        ])
        .output()
        .expect("Demo render starts");
    let report = json_report(&report);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["sample_rate"], 48000);
    assert_eq!(report["channels"], 2);
    assert_eq!(report["frames"], 48000);
    assert_eq!(report["parts"][0]["frames"], 48000);
    assert_eq!(report["parts"][1]["frames"], 24000);
    assert!(report["mix_analysis"].is_object());

    let reader = hound::WavReader::open(&output).expect("mixed WAV");
    assert_eq!(reader.spec().channels, 2);
    assert_eq!(reader.spec().sample_rate, 48000);
    assert_eq!(reader.duration(), 48000);
    let stem = stems.join("second.part.wav");
    assert!(stem.exists());

    let reference = fixture.demo.with_file_name("second-reference.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "pattern",
            fixture.second_instrument.to_str().expect("instrument path"),
            fixture.second_pattern.to_str().expect("pattern path"),
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--tail",
            "0",
            "--output",
            reference.to_str().expect("reference output"),
        ])
        .assert()
        .success();
    let expected = hound::WavReader::open(reference)
        .expect("reference WAV")
        .into_samples::<f32>()
        .map(|sample| sample.expect("reference sample"))
        .collect::<Vec<_>>();
    let actual = hound::WavReader::open(stem)
        .expect("stem WAV")
        .into_samples::<f32>()
        .map(|sample| sample.expect("stem sample"))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}
