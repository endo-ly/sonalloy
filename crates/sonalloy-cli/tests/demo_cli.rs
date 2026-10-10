mod support;

use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use assert_cmd::Command;
use midly::{Format, Smf};
use serde_json::{Value, json};
use support::fixture_path;
use tempfile::TempDir;

struct DemoFixture {
    _directory: TempDir,
    demo: PathBuf,
    first_pattern: PathBuf,
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
        first_pattern,
        second_instrument,
        second_pattern,
    }
}

fn write_pattern(path: &Path, length_ticks: u64, duration_ticks: u64, note: u8) {
    write_pattern_with_axis(
        path,
        length_ticks,
        duration_ticks,
        note,
        &json!([{"tick": 0, "bpm": 120.0}]),
        &json!([{"tick": 0, "numerator": 4, "denominator": 4}]),
    );
}

fn write_pattern_with_axis(
    path: &Path,
    length_ticks: u64,
    duration_ticks: u64,
    note: u8,
    tempo_changes: &Value,
    time_signature_changes: &Value,
) {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "name": null,
            "ticks_per_beat": 480,
            "length_ticks": length_ticks,
            "tempo_changes": tempo_changes,
            "time_signature_changes": time_signature_changes,
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

fn track_end_tick(track: &[midly::TrackEvent<'_>]) -> u64 {
    track
        .iter()
        .map(|event| u64::from(event.delta.as_int()))
        .sum()
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

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "demo",
            "validate",
            fixture.demo.to_str().expect("Demo path"),
        ])
        .assert()
        .success();

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
    assert_eq!(inspection_report["status"], "ok");
    assert_eq!(inspection_report["command"], "demo inspect");
    assert_eq!(inspection_report["part_count"], 2);
    assert_eq!(inspection_report["ticks_per_beat"], 480);
    assert_eq!(inspection_report["length_ticks"], 960);
    assert_eq!(inspection_report["musical_duration_seconds"], 1.0);
    assert_eq!(inspection_report["parts"][0]["midi_channel"], 2);
    assert_eq!(inspection_report["parts"][1]["midi_channel"], 1);
    assert_eq!(inspection_report["ffmpeg_required"], false);

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["demo", "inspect", fixture.demo.to_str().expect("Demo path")])
        .assert()
        .success();
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

    let bytes = std::fs::read(&output).expect("MIDI output");
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
    assert!(smf.tracks[0].iter().any(|event| {
        matches!(
            event.kind,
            midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(value))
                if value.as_int() == 500_000
        )
    }));
    assert!(smf.tracks[0].iter().any(|event| {
        matches!(
            event.kind,
            midly::TrackEventKind::Meta(midly::MetaMessage::TimeSignature(4, 2, _, _))
        )
    }));
    assert!(smf.tracks[1..].iter().all(|track| {
        track.iter().all(|event| {
            !matches!(
                event.kind,
                midly::TrackEventKind::Meta(
                    midly::MetaMessage::Tempo(_) | midly::MetaMessage::TimeSignature(_, _, _, _)
                )
            )
        })
    }));
    assert!(smf.tracks.iter().all(|track| track_end_tick(track) == 960));

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "demo",
            "export-midi",
            fixture.demo.to_str().expect("Demo path"),
            "--output",
            output.to_str().expect("MIDI output"),
        ])
        .assert()
        .success();
    let rewritten = std::fs::read(output).expect("overwritten MIDI output");
    assert_eq!(
        Smf::parse(&rewritten)
            .expect("rewritten Type 1 MIDI")
            .header
            .format,
        Format::Parallel
    );
}

#[test]
fn demo_export_midi_prefixes_parameter_change_errors_with_part_path() {
    let fixture = demo_fixture();
    std::fs::write(
        &fixture.second_pattern,
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "ticks_per_beat": 480,
            "length_ticks": 480,
            "tempo_changes": [{"tick": 0, "bpm": 120.0}],
            "time_signature_changes": [{"tick": 0, "numerator": 4, "denominator": 4}],
            "events": [
                {"type": "note", "tick": 0, "duration_ticks": 240, "note": 67, "velocity": 100},
                {"type": "parameter_change", "tick": 0, "parameter": "voice.processor.tone.cutoff", "native_value": 8000.0}
            ]
        }))
        .expect("parameter pattern JSON"),
    )
    .expect("parameter pattern");
    let output = fixture.demo.with_file_name("parameter.mid");
    let result = Command::cargo_bin("sonalloy")
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

    assert_eq!(result.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&result.stdout).expect("error report");
    assert_eq!(report["diagnostics"][0]["code"], "MIDI_ERROR");
    assert_eq!(
        report["diagnostics"][0]["path"],
        "parts[1].pattern.events[1]"
    );
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
    assert!(report["output_analysis"].is_object());

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

#[test]
fn render_demo_reports_measured_master_output_analysis_and_mp3() {
    if ProcessCommand::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        return;
    }
    let fixture = demo_fixture();
    let mut definition: Value =
        serde_json::from_slice(&std::fs::read(&fixture.demo).expect("Demo JSON")).expect("Demo");
    definition["mix"]["master"] = json!({
        "integrated_lufs": -10.0,
        "true_peak_db": -1.0,
        "loudness_range_lu": 7.0
    });
    std::fs::write(
        &fixture.demo,
        serde_json::to_vec_pretty(&definition).expect("Demo JSON"),
    )
    .expect("Demo file");

    let output = fixture.demo.with_file_name("mastered.wav");
    let mp3 = fixture.demo.with_file_name("mastered.mp3");
    let result = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "demo",
            fixture.demo.to_str().expect("Demo path"),
            "--tail",
            "0",
            "--analyze",
            "--output",
            output.to_str().expect("WAV output"),
            "--mp3-output",
            mp3.to_str().expect("MP3 output"),
            "--json",
        ])
        .output()
        .expect("Demo render starts");
    let report = json_report(&result);
    assert!(report["mix_analysis"].is_object());
    assert!(report["output_analysis"].is_object());
    assert_eq!(report["master"]["target"]["true_peak_db"], -1.0);
    assert!(report["master"]["normalization_type"].is_string());
    assert!(report["master"]["output"]["true_peak_db"].as_f64().unwrap() <= -1.0);
    assert_eq!(report["mp3_output"], mp3.to_string_lossy().as_ref());
    for field in ["integrated_lufs", "true_peak_db", "loudness_range_lu"] {
        assert!(
            report["mp3_measurement"][field]
                .as_f64()
                .is_some_and(f64::is_finite)
        );
    }

    let independently_measured_tp = ffmpeg_true_peak(&output);
    let reported_tp = report["master"]["output"]["true_peak_db"]
        .as_f64()
        .expect("reported WAV True Peak");
    assert!((reported_tp - independently_measured_tp).abs() < 0.05);
    assert!(independently_measured_tp <= -1.0);

    let sample_peak = wav_samples(&output)
        .into_iter()
        .map(f32::abs)
        .fold(0.0_f32, f32::max);
    let sample_peak_dbfs = 20.0 * f64::from(sample_peak).log10();
    let analyzed_peak_dbfs = report["output_analysis"]["level"]["peak_dbfs"]
        .as_f64()
        .expect("completed WAV sample peak analysis");
    assert!((sample_peak_dbfs - analyzed_peak_dbfs).abs() < 0.01);
}

#[test]
fn render_demo_measures_mp3_without_mastering() {
    if ProcessCommand::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        return;
    }
    let fixture = demo_fixture();
    let output = fixture.demo.with_file_name("unmastered.wav");
    let mp3 = fixture.demo.with_file_name("unmastered.mp3");
    let result = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "demo",
            fixture.demo.to_str().expect("Demo path"),
            "--tail",
            "0",
            "--output",
            output.to_str().expect("WAV output"),
            "--mp3-output",
            mp3.to_str().expect("MP3 output"),
            "--json",
        ])
        .output()
        .expect("Demo render starts");
    let report = json_report(&result);
    assert!(report.get("master").is_none());
    assert!(mp3.exists());
    assert!(
        report["mp3_measurement"]["integrated_lufs"]
            .as_f64()
            .is_some_and(f64::is_finite)
    );
    assert!(
        report["mp3_measurement"]["true_peak_db"]
            .as_f64()
            .is_some_and(f64::is_finite)
    );
    assert!(
        report["mp3_measurement"]["loudness_range_lu"]
            .as_f64()
            .is_some_and(f64::is_finite)
    );
}

fn ffmpeg_true_peak(path: &Path) -> f64 {
    let output = ProcessCommand::new("ffmpeg")
        .args([
            "-hide_banner",
            "-nostdin",
            "-i",
            path.to_str().expect("WAV path"),
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    let report_start = stderr.find('{').expect("loudnorm report starts");
    let report_end = report_start + stderr[report_start..].find('}').expect("report ends") + 1;
    let report: Value =
        serde_json::from_str(&stderr[report_start..report_end]).expect("measurement JSON");
    report["input_tp"]
        .as_str()
        .and_then(|value| value.parse().ok())
        .expect("measured True Peak")
}

#[test]
fn render_demo_routes_source_audio_with_gain_and_keeps_definition_order() {
    let fixture = demo_fixture();
    configure_routed_demo(&fixture);

    let output = fixture.demo.with_file_name("routed.wav");
    let stems = fixture.demo.with_file_name("routed-stems");
    let report = render_routed_demo(&fixture, &output, &stems);
    assert_eq!(report["parts"][0]["id"], "lead");
    assert_eq!(report["parts"][1]["id"], "bass");
    assert_eq!(report["parts"][2]["id"], "first-part");
    assert!(stems.join("lead.wav").exists());
    assert!(stems.join("bass.wav").exists());

    assert_routed_midi_order(&fixture);
    let reference = render_bass_reference(&fixture);
    assert_wav_matches(&stems.join("bass.wav"), &reference);
}

fn configure_routed_demo(fixture: &DemoFixture) {
    let mut definition: Value =
        serde_json::from_slice(&std::fs::read(&fixture.demo).expect("Demo JSON")).expect("Demo");
    definition["parts"][0]["gain_db"] = json!(-6.0);
    definition["parts"][1]["id"] = json!("bass");
    definition["parts"][1]["instrument"] = json!(
        fixture_path("instruments/sidechain-ducking.json")
            .to_string_lossy()
            .into_owned()
    );
    definition["parts"][1]["audio_input"] = json!({"part": "first-part"});
    let mut lead = definition["parts"][1].clone();
    lead["id"] = json!("lead");
    lead["audio_input"]["part"] = json!("bass");
    let parts = definition["parts"].as_array_mut().expect("Demo parts");
    parts.insert(0, lead);
    parts.swap(1, 2);
    std::fs::write(
        &fixture.demo,
        serde_json::to_vec_pretty(&definition).expect("Demo JSON"),
    )
    .expect("Demo file");
}

fn render_routed_demo(fixture: &DemoFixture, output: &Path, stems: &Path) -> Value {
    let result = Command::cargo_bin("sonalloy")
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
            "--output",
            output.to_str().expect("WAV output"),
            "--json",
        ])
        .output()
        .expect("Demo render starts");
    json_report(&result)
}

fn assert_routed_midi_order(fixture: &DemoFixture) {
    let midi = fixture.demo.with_file_name("routed.mid");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "demo",
            "export-midi",
            fixture.demo.to_str().expect("Demo path"),
            "--output",
            midi.to_str().expect("MIDI output"),
        ])
        .assert()
        .success();
    let midi_bytes = std::fs::read(midi).expect("MIDI output");
    let midi = Smf::parse(&midi_bytes).expect("Type 1 MIDI");
    let track_names = midi.tracks[1..]
        .iter()
        .map(|track| {
            track
                .iter()
                .find_map(|event| match event.kind {
                    midly::TrackEventKind::Meta(midly::MetaMessage::TrackName(name)) => {
                        Some(name.to_vec())
                    }
                    _ => None,
                })
                .expect("Part track name")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        track_names,
        [b"lead".to_vec(), b"bass".to_vec(), b"first-part".to_vec()]
    );
}

fn render_bass_reference(fixture: &DemoFixture) -> PathBuf {
    let source = fixture.demo.with_file_name("kick-source.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "pattern",
            fixture_path("instruments/basic-poly-synth.json")
                .to_str()
                .expect("source instrument"),
            fixture.first_pattern.to_str().expect("source pattern"),
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--tail",
            "0",
            "--output",
            source.to_str().expect("source WAV"),
        ])
        .assert()
        .success();

    let gained_source = fixture.demo.with_file_name("kick-source-gained.wav");
    write_gained_source(&source, &gained_source);
    render_bass_with_audio_input(fixture, &gained_source)
}

fn write_gained_source(source: &Path, destination: &Path) {
    let source_reader = hound::WavReader::open(source).expect("source WAV");
    let spec = source_reader.spec();
    let mut gained_writer = hound::WavWriter::create(destination, spec).expect("gained source");
    #[allow(clippy::cast_possible_truncation)]
    let gain = 10.0_f64.powf(-6.0 / 20.0) as f32;
    for sample in source_reader.into_samples::<f32>() {
        let sample = sample.expect("source sample");
        gained_writer
            .write_sample(sample * gain)
            .expect("gained sample");
    }
    gained_writer.finalize().expect("gained source finalized");
}

fn render_bass_with_audio_input(fixture: &DemoFixture, audio_input: &Path) -> PathBuf {
    let reference = fixture.demo.with_file_name("bass-reference.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "pattern",
            fixture_path("instruments/sidechain-ducking.json")
                .to_str()
                .expect("consumer instrument"),
            fixture.second_pattern.to_str().expect("consumer pattern"),
            "--audio-input",
            audio_input.to_str().expect("gained source WAV"),
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--tail",
            "0",
            "--output",
            reference.to_str().expect("reference WAV"),
        ])
        .assert()
        .success();
    reference
}

fn assert_wav_matches(actual_path: &Path, expected_path: &Path) {
    let actual = wav_samples(actual_path);
    let expected = wav_samples(expected_path);
    assert_eq!(actual.len(), expected.len());
    let max_difference = actual
        .iter()
        .zip(&expected)
        .map(|(actual, expected)| (actual - expected).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        max_difference < 1.0e-6,
        "maximum sample difference: {max_difference}"
    );
}

fn wav_samples(path: &Path) -> Vec<f32> {
    hound::WavReader::open(path)
        .expect("WAV opens")
        .into_samples::<f32>()
        .map(|sample| sample.expect("valid WAV sample"))
        .collect()
}

#[test]
fn demo_uses_longest_pattern_for_timeline_and_conductor() {
    let fixture = demo_fixture();
    write_pattern_with_axis(
        &fixture.first_pattern,
        480,
        240,
        60,
        &json!([{"tick": 0, "bpm": 120.0}]),
        &json!([{"tick": 0, "numerator": 4, "denominator": 4}]),
    );
    write_pattern_with_axis(
        &fixture.second_pattern,
        960,
        240,
        67,
        &json!([
            {"tick": 0, "bpm": 120.0},
            {"tick": 480, "bpm": 60.0}
        ]),
        &json!([
            {"tick": 0, "numerator": 4, "denominator": 4},
            {"tick": 480, "numerator": 3, "denominator": 4}
        ]),
    );

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
    assert_eq!(inspection_report["length_ticks"], 960);
    assert_eq!(inspection_report["musical_duration_seconds"], 1.5);
    assert_eq!(inspection_report["tempo_change_count"], 2);
    assert_eq!(inspection_report["time_signature_change_count"], 2);

    let output = fixture.demo.with_file_name("longest.mid");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "demo",
            "export-midi",
            fixture.demo.to_str().expect("Demo path"),
            "--output",
            output.to_str().expect("MIDI output"),
        ])
        .assert()
        .success();
    let bytes = std::fs::read(output).expect("MIDI output");
    let smf = Smf::parse(&bytes).expect("Type 1 MIDI");
    assert!(smf.tracks[0].iter().any(|event| {
        matches!(
            event.kind,
            midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(value))
                if value.as_int() == 1_000_000
        )
    }));
    assert!(smf.tracks[0].iter().any(|event| {
        matches!(
            event.kind,
            midly::TrackEventKind::Meta(midly::MetaMessage::TimeSignature(3, 2, _, _))
        )
    }));
    assert!(smf.tracks.iter().all(|track| track_end_tick(track) == 960));
}

#[test]
fn demo_audio_input_must_match_the_instrument_contract() {
    let fixture = demo_fixture();
    let mut definition: Value =
        serde_json::from_slice(&std::fs::read(&fixture.demo).expect("Demo JSON")).expect("Demo");
    definition["parts"][0]["instrument"] = json!(
        fixture_path("instruments/sidechain-ducking.json")
            .to_string_lossy()
            .into_owned()
    );
    std::fs::write(
        &fixture.demo,
        serde_json::to_vec_pretty(&definition).expect("Demo JSON"),
    )
    .expect("Demo file");

    let result = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "demo",
            "validate",
            fixture.demo.to_str().expect("Demo path"),
            "--json",
        ])
        .output()
        .expect("validation starts");

    assert_eq!(result.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&result.stdout).expect("error report");
    assert!(report["diagnostics"].as_array().is_some_and(|diagnostics| {
        diagnostics.iter().any(|diagnostic| {
            diagnostic["code"] == "DEFINITION_ERROR"
                && diagnostic["path"] == "parts[0].audio_input"
                && diagnostic["message"] == "audio_input is required by this instrument"
        })
    }));

    definition["parts"][0]["audio_input"] = json!({"part": "second.part"});
    std::fs::write(
        &fixture.demo,
        serde_json::to_vec_pretty(&definition).expect("Demo JSON"),
    )
    .expect("Demo file");
    let connected = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "demo",
            "inspect",
            fixture.demo.to_str().expect("Demo path"),
            "--json",
        ])
        .output()
        .expect("Demo inspection starts");
    let connected_report = json_report(&connected);
    assert_eq!(
        connected_report["parts"][0]["audio_input"]["part"],
        "second.part"
    );

    definition["parts"][0]["instrument"] = json!(
        fixture_path("instruments/basic-poly-synth.json")
            .to_string_lossy()
            .into_owned()
    );
    std::fs::write(
        &fixture.demo,
        serde_json::to_vec_pretty(&definition).expect("Demo JSON"),
    )
    .expect("Demo file");
    let invalid = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "demo",
            "validate",
            fixture.demo.to_str().expect("Demo path"),
            "--json",
        ])
        .output()
        .expect("validation starts");
    assert_eq!(invalid.status.code(), Some(1));
    let invalid_report: Value = serde_json::from_slice(&invalid.stdout).expect("error report");
    assert!(
        invalid_report["diagnostics"]
            .as_array()
            .is_some_and(|diagnostics| {
                diagnostics.iter().any(|diagnostic| {
                    diagnostic["code"] == "DEFINITION_ERROR"
                        && diagnostic["path"] == "parts[0].audio_input"
                        && diagnostic["message"] == "audio_input is not used by this instrument"
                })
            })
    );
}

fn read_json_file(path: &Path) -> Value {
    let context = || format!("{}: JSON file", path.display());
    let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("{}: {error}", context()));
    serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("{}: {error}", context()))
}

fn write_json_file(path: &Path, value: &impl serde::Serialize) {
    std::fs::write(path, serde_json::to_vec_pretty(value).expect("JSON")).expect("write JSON");
}

fn pack_demo(input: &Path, output: &Path, with_render: bool) -> std::process::Output {
    let mut command = Command::cargo_bin("sonalloy").expect("binary");
    command
        .args(["demo", "pack"])
        .arg(input)
        .arg("--output")
        .arg(output)
        .args(["--tail", "0", "--json"]);
    if with_render {
        command.arg("--with-render");
    }
    command.output().expect("pack starts")
}

fn bundle_file_paths(root: &Path, path: &Path, paths: &mut Vec<String>) {
    for entry in std::fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        assert!(!entry.file_type().unwrap().is_symlink());
        if entry.file_type().unwrap().is_dir() {
            bundle_file_paths(root, &entry.path(), paths);
        } else {
            let relative = entry
                .path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if relative != "bundle.json" {
                paths.push(relative);
            }
        }
    }
}

fn assert_bundle_manifest(root: &Path) -> Value {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let manifest = read_json_file(&root.join("bundle.json"));
    assert_eq!(manifest.as_object().unwrap().len(), 5);
    assert_eq!(manifest["format_version"], 1);
    assert_eq!(manifest["demo"], "demo.json");
    assert_eq!(
        manifest["render_settings"],
        json!({"sample_rate": 48000, "block_size": 257, "tail_seconds": 0.0})
    );
    let files = manifest["files"].as_array().unwrap();
    let paths = files
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(paths.windows(2).all(|pair| pair[0] < pair[1]));
    for file in files {
        assert_eq!(file.as_object().unwrap().len(), 2);
        let path = file["path"].as_str().unwrap();
        assert!(!path.contains('\\'));
        assert!(path.split('/').all(|part| !["", ".", ".."].contains(&part)));
        let mut hash = String::new();
        for byte in Sha256::digest(std::fs::read(root.join(path)).unwrap()) {
            write!(hash, "{byte:02x}").unwrap();
        }
        assert_eq!(file["sha256"], hash);
    }
    let mut actual = Vec::new();
    bundle_file_paths(root, root, &mut actual);
    actual.sort();
    assert_eq!(actual, paths);
    manifest
}

#[test]
fn demo_pack_preserves_performance_and_renders_after_source_is_moved() {
    let fixture = demo_fixture();
    let output_directory = tempfile::tempdir().unwrap();
    let bundle = output_directory.path().join("bundle");
    let mut pattern = read_json_file(&fixture.first_pattern);
    pattern["tempo_changes"] = json!([{"tick":0,"bpm":120.0},{"tick":600,"bpm":90.0}]);
    pattern["time_signature_changes"] = json!([{"tick":0,"numerator":4,"denominator":4},{"tick":600,"numerator":3,"denominator":4}]);
    // Deliberately keep events in definition order rather than tick order.
    pattern["events"] = json!([
        {"type":"sustain_pedal","tick":200,"down":true},
        {"type":"note","tick":0,"duration_ticks":480,"note":60,"velocity":100},
        {"type":"pitch_bend","tick":10,"value":0.25},
        {"type":"mod_wheel","tick":20,"value":0.4},
        {"type":"aftertouch","tick":30,"value":0.5},
        {"type":"parameter_change","tick":40,"parameter":"voice.processor.tone.cutoff","native_value":3000.0},
        {"type":"sustain_pedal","tick":500,"down":false}
    ]);
    write_json_file(&fixture.first_pattern, &pattern);
    let mut original = read_json_file(&fixture.demo);
    original["mix"]["fade_out_seconds"] = json!(0.2);
    write_json_file(&fixture.demo, &original);
    let reference = output_directory.path().join("reference.wav");
    let reference_stems = output_directory.path().join("reference-stems");
    render_routed_demo(&fixture, &reference, &reference_stems);

    let result = json_report(&pack_demo(&fixture.demo, &bundle, false));
    let manifest = assert_bundle_manifest(&bundle);
    let mut relocated = read_json_file(&bundle.join("demo.json"));
    for (index, id) in ["first-part", "second.part"].iter().enumerate() {
        assert!(bundle.join(format!("instruments/{id}/assets")).is_dir());
        assert_eq!(
            relocated["parts"][index]["instrument"],
            format!("instruments/{id}/definition.json")
        );
        assert_eq!(
            relocated["parts"][index]["pattern"],
            format!("patterns/{id}.json")
        );
        relocated["parts"][index]["instrument"] = original["parts"][index]["instrument"].clone();
        relocated["parts"][index]["pattern"] = original["parts"][index]["pattern"].clone();
    }
    assert_eq!(relocated, original);
    assert_eq!(
        read_json_file(&bundle.join("patterns/first-part.json")),
        pattern
    );
    assert_eq!(result["command"], "demo pack");
    assert_eq!(result["part_count"], 2);
    assert_eq!(
        result["file_count"],
        manifest["files"].as_array().unwrap().len()
    );
    assert_eq!(result["render_included"], false);
    assert!(result["diagnostics"].is_array());
    assert!(manifest["render"].is_null());
    assert!(!bundle.join("render").exists());
    std::fs::rename(
        fixture.demo.parent().unwrap(),
        fixture.demo.parent().unwrap().with_file_name("moved-song"),
    )
    .unwrap();
    Command::cargo_bin("sonalloy")
        .unwrap()
        .args(["demo", "validate"])
        .arg(bundle.join("demo.json"))
        .assert()
        .success();
    let rerender = output_directory.path().join("rerender.wav");
    Command::cargo_bin("sonalloy")
        .unwrap()
        .args(["render", "demo"])
        .arg(bundle.join("demo.json"))
        .args(["--tail", "0", "--output"])
        .arg(&rerender)
        .assert()
        .success();
    assert_wav_matches(&rerender, &reference);
}

#[test]
fn demo_pack_includes_all_stems_and_preserves_external_audio_and_mastering() {
    for mastered in [false, true] {
        if mastered
            && !ProcessCommand::new("ffmpeg")
                .arg("-version")
                .output()
                .is_ok_and(|output| output.status.success())
        {
            continue;
        }
        let fixture = demo_fixture();
        configure_routed_demo(&fixture);
        let mut definition = read_json_file(&fixture.demo);
        definition["mix"]["fade_out_seconds"] = json!(0.1);
        if mastered {
            definition["mix"]["master"] =
                json!({"integrated_lufs":-16.0,"true_peak_db":-1.0,"loudness_range_lu":11.0});
        }
        write_json_file(&fixture.demo, &definition);
        let directory = tempfile::tempdir().unwrap();
        let reference = directory.path().join("reference.wav");
        let reference_stems = directory.path().join("reference-stems");
        render_routed_demo(&fixture, &reference, &reference_stems);
        let bundle = directory.path().join("bundle");

        let result = json_report(&pack_demo(&fixture.demo, &bundle, true));

        let manifest = assert_bundle_manifest(&bundle);
        assert_eq!(result["render_included"], true);
        assert_eq!(manifest["render"]["mix"], "render/mix.wav");
        assert_eq!(manifest["render"]["stems"].as_object().unwrap().len(), 3);
        assert_wav_matches(&bundle.join("render/mix.wav"), &reference);
        let relocated = read_json_file(&bundle.join("demo.json"));
        for (index, id) in ["lead", "bass", "first-part"].iter().enumerate() {
            assert_eq!(
                relocated["parts"][index]["audio_input"],
                definition["parts"][index]["audio_input"]
            );
            assert_eq!(
                manifest["render"]["stems"][id],
                format!("render/stems/{id}.wav")
            );
            assert_wav_matches(
                &bundle.join(format!("render/stems/{id}.wav")),
                &reference_stems.join(format!("{id}.wav")),
            );
        }
        assert_eq!(relocated["mix"], definition["mix"]);
    }
}

fn bundled_asset_instrument(
    fixture: &DemoFixture,
) -> sonalloy_core::definition::InstrumentDefinition {
    use sonalloy_core::definition::{
        AssetReference, ConvolutionProcessorDefinition, GeneratorDefinition, InstrumentDefinition,
        ProcessorDefinition,
    };
    let mut instrument: InstrumentDefinition = serde_json::from_value(read_json_file(
        &fixture_path("instruments/mapped-sample-instrument.json"),
    ))
    .unwrap();
    let sample = fixture.demo.parent().unwrap().join("sample.wav");
    let source_sample = sample.with_file_name("source-sample.wav");
    std::fs::copy(fixture_path("assets/metal-hit.wav"), &source_sample).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&source_sample, &sample).unwrap();
    #[cfg(windows)]
    std::fs::copy(&source_sample, &sample).unwrap();
    instrument
        .try_for_each_asset_mut(|_, reference| {
            reference.path = sample.to_string_lossy().into_owned();
            Ok::<_, ()>(())
        })
        .unwrap();
    for relative in [
        "presets/PAD/008-granular-texture-pad/definition.json",
        "presets/SEQ/001-rhythmic-wave-sequence/definition.json",
        "presets/PAD/005-wavetable-motion-pad/definition.json",
        "testdata/instruments/spectral-generator-reference.json",
    ] {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative);
        let mut extra: InstrumentDefinition =
            serde_json::from_value(read_json_file(&source)).unwrap();
        extra
            .try_for_each_asset_mut(|_, reference| {
                reference.path = source
                    .parent()
                    .unwrap()
                    .join(&reference.path)
                    .to_string_lossy()
                    .into_owned();
                Ok::<_, ()>(())
            })
            .unwrap();
        let mut layer = extra
            .layers
            .into_iter()
            .find(|layer| {
                matches!(
                    layer.generator,
                    GeneratorDefinition::Granular(_)
                        | GeneratorDefinition::WaveSequence(_)
                        | GeneratorDefinition::Wavetable(_)
                        | GeneratorDefinition::Spectral(_)
                )
            })
            .unwrap();
        layer.enabled = false;
        instrument.layers.push(layer);
    }
    // The convolution IR reaches the very same bytes through a second path, so both
    // references must collapse onto a single stored asset.
    instrument
        .global_processors
        .push(ProcessorDefinition::Convolution(
            ConvolutionProcessorDefinition {
                id: "room".to_owned(),
                ir: AssetReference {
                    path: source_sample.to_string_lossy().into_owned(),
                    sha256: None,
                },
                gain_db: -12.0,
                mix: 0.1,
            },
        ));
    instrument
}

#[test]
fn demo_pack_copies_every_asset_kind_and_deduplicates_content_per_instrument() {
    use sonalloy_core::definition::InstrumentDefinition;
    let fixture = demo_fixture();
    let instrument_path = fixture
        .demo
        .parent()
        .unwrap()
        .join("instruments/first.json");
    let mut instrument = bundled_asset_instrument(&fixture);
    let mut reference_count = 0;
    instrument
        .try_for_each_asset_mut(|_, _| {
            reference_count += 1;
            Ok::<_, ()>(())
        })
        .unwrap();
    assert!(reference_count >= 11);
    write_json_file(&instrument_path, &instrument);
    let directory = tempfile::tempdir().unwrap();
    let bundle = directory.path().join("bundle");

    json_report(&pack_demo(&fixture.demo, &bundle, false));

    let manifest = assert_bundle_manifest(&bundle);
    let packed_path = bundle.join("instruments/first-part/definition.json");
    let mut packed: InstrumentDefinition =
        serde_json::from_value(read_json_file(&packed_path)).unwrap();
    let mut hashes = std::collections::HashSet::new();
    let mut actual_count = 0;
    packed
        .try_for_each_asset_mut(|_, reference| {
            actual_count += 1;
            let hash = reference.sha256.as_ref().unwrap();
            hashes.insert(hash.clone());
            assert_eq!(reference.path, format!("assets/{hash}.wav"));
            let relative = format!("instruments/first-part/{}", reference.path);
            assert!(
                manifest["files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|file| file["path"] == relative && file["sha256"] == *hash)
            );
            Ok::<_, ()>(())
        })
        .unwrap();
    assert_eq!(actual_count, reference_count);
    assert_eq!(
        std::fs::read_dir(packed_path.parent().unwrap().join("assets"))
            .unwrap()
            .count(),
        hashes.len()
    );
    assert!(hashes.len() < reference_count);
    std::fs::remove_dir_all(fixture.demo.parent().unwrap()).unwrap();
    Command::cargo_bin("sonalloy")
        .unwrap()
        .args(["demo", "validate"])
        .arg(bundle.join("demo.json"))
        .assert()
        .success();
    Command::cargo_bin("sonalloy")
        .unwrap()
        .args(["render", "demo"])
        .arg(bundle.join("demo.json"))
        .args(["--tail", "0", "--output"])
        .arg(directory.path().join("mix.wav"))
        .assert()
        .success();
}

#[test]
fn demo_pack_rejects_invalid_inputs_and_existing_outputs_without_residue() {
    for case in [
        "missing-pattern",
        "missing-asset",
        "hash",
        "part-id",
        "existing-file",
        "existing-directory",
        "render",
    ] {
        let fixture = demo_fixture();
        let directory = tempfile::tempdir().unwrap();
        let bundle = directory.path().join("bundle");
        let instrument = fixture
            .demo
            .parent()
            .unwrap()
            .join("instruments/first.json");
        let expected_code = match case {
            "missing-pattern" => {
                std::fs::remove_file(&fixture.first_pattern).unwrap();
                "DEFINITION_ERROR"
            }
            "missing-asset" | "hash" => {
                let mut definition =
                    read_json_file(&fixture_path("instruments/mapped-sample-instrument.json"));
                for zone in definition["layers"][0]["generator"]["sample"]["zones"]
                    .as_array_mut()
                    .unwrap()
                {
                    zone["asset"]["path"] = fixture_path("assets/metal-hit.wav")
                        .to_string_lossy()
                        .into_owned()
                        .into();
                    if case == "missing-asset" {
                        zone["asset"]["path"] = json!("absent.wav");
                    } else {
                        zone["asset"]["sha256"] = json!("0".repeat(64));
                    }
                }
                write_json_file(&instrument, &definition);
                if case == "hash" {
                    "ASSET_HASH_MISMATCH"
                } else {
                    "ASSET_NOT_FOUND"
                }
            }
            "part-id" => {
                let mut demo = read_json_file(&fixture.demo);
                demo["parts"][0]["id"] = json!("first.part.");
                write_json_file(&fixture.demo, &demo);
                "VALUE_OUT_OF_RANGE"
            }
            "existing-file" => {
                std::fs::write(&bundle, b"keep").unwrap();
                "BUNDLE_OUTPUT_EXISTS"
            }
            "existing-directory" => {
                std::fs::create_dir(&bundle).unwrap();
                "BUNDLE_OUTPUT_EXISTS"
            }
            "render" => {
                let mut demo = read_json_file(&fixture.demo);
                demo["mix"]["fade_out_seconds"] = json!(100.0);
                write_json_file(&fixture.demo, &demo);
                "RENDER_ERROR"
            }
            _ => unreachable!(),
        };
        let before = std::fs::read(&fixture.demo).unwrap();

        let output = pack_demo(&fixture.demo, &bundle, case == "render");

        assert!(!output.status.success(), "{case}");
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            report["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["code"] == expected_code),
            "{case}: {report}"
        );
        assert_eq!(std::fs::read(&fixture.demo).unwrap(), before);
        if case.starts_with("existing") {
            if case == "existing-file" {
                assert_eq!(std::fs::read(&bundle).unwrap(), b"keep");
            }
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        } else {
            assert!(!bundle.exists());
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        }
    }
}
