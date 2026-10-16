mod support;

use assert_cmd::Command;
use midly::{Format, Smf, Timing};
use midly::{Header, MidiMessage, TrackEvent, TrackEventKind};
use serde_json::json;
use support::fixture_path;
use tempfile::tempdir;

fn write_sustain_midi(directory: &std::path::Path) -> std::path::PathBuf {
    let path = directory.join("sustain.mid");
    let mut smf = Smf::new(Header::new(
        Format::SingleTrack,
        Timing::Metrical(480.into()),
    ));
    smf.tracks.push(vec![
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(midly::MetaMessage::Tempo(500_000.into())),
        },
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(midly::MetaMessage::TimeSignature(4, 2, 24, 8)),
        },
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
            delta: 137.into(),
            kind: TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::Controller {
                    controller: 1.into(),
                    value: 64.into(),
                },
            },
        },
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::PitchBend {
                    bend: midly::PitchBend::from_int(4096),
                },
            },
        },
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::ChannelAftertouch { vel: 80.into() },
            },
        },
        TrackEvent {
            delta: 103.into(),
            kind: TrackEventKind::Meta(midly::MetaMessage::Tempo(666_667.into())),
        },
        TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::Controller {
                    controller: 64.into(),
                    value: 127.into(),
                },
            },
        },
        TrackEvent {
            delta: 120.into(),
            kind: TrackEventKind::Meta(midly::MetaMessage::TimeSignature(6, 3, 24, 8)),
        },
        TrackEvent {
            delta: 120.into(),
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

fn inspect(path: &std::path::Path) -> serde_json::Value {
    let output = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["score", "inspect"])
        .arg(path)
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    serde_json::from_slice(&output.stdout).expect("inspection")
}

fn inspect_with_preset_root(
    path: &std::path::Path,
    preset_root: &std::path::Path,
) -> serde_json::Value {
    let output = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["score", "inspect"])
        .arg(path)
        .arg("--preset-root")
        .arg(preset_root)
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    serde_json::from_slice(&output.stdout).expect("inspection")
}

#[test]
fn score_commands_share_the_timeline_and_preserve_midi_controls() {
    let directory = tempdir().expect("directory");
    let score = directory.path().join("score.yaml");
    let preset_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../presets")
        .canonicalize()
        .expect("preset root");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["score", "init"])
        .arg(&score)
        .assert()
        .success();
    let generated = std::fs::read_to_string(&score).expect("generated Score");
    assert!(generated.contains("LEAD/001-analog-saw-lead"));
    assert!(!generated.contains("definition:"));
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["score", "validate"])
        .arg(&score)
        .arg("--preset-root")
        .arg(&preset_root)
        .arg("--json")
        .assert()
        .success();
    let original = inspect_with_preset_root(&score, &preset_root);
    assert_eq!(
        original["parts"][0]["events"]
            .as_array()
            .expect("events")
            .len(),
        4
    );
    let midi = directory.path().join("score.mid");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["score", "export-midi"])
        .arg(&score)
        .arg("--output")
        .arg(&midi)
        .arg("--preset-root")
        .arg(&preset_root)
        .assert()
        .success();
    let bytes = std::fs::read(&midi).expect("MIDI");
    let smf = Smf::parse(&bytes).expect("SMF");
    assert_eq!(smf.header.format, Format::Parallel);
    assert_eq!(smf.tracks.len(), 2);
    let imported = directory.path().join("imported.score.yaml");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["score", "import-midi"])
        .arg(&midi)
        .arg("--output")
        .arg(&imported)
        .arg("--instrument")
        .arg(fixture_path("instruments/basic-poly-synth.json"))
        .args(["--tonic", "A"])
        .assert()
        .success();
    assert_eq!(
        inspect(&imported)["parts"][0]["events"],
        original["parts"][0]["events"]
    );
    let output = directory.path().join("score.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["render", "score"])
        .arg(&score)
        .arg("--output")
        .arg(&output)
        .arg("--preset-root")
        .arg(&preset_root)
        .args(["--tail", "0", "--sample-rate", "24000"])
        .assert()
        .success();
    let wav = hound::WavReader::open(&output).expect("WAV");
    assert_eq!(wav.duration(), 48001);
    assert!(
        wav.into_samples::<f32>()
            .map(Result::unwrap)
            .any(|s| s.abs() > 0.01)
    );

    assert_control_and_channel_import(directory.path());
}

#[test]
fn routed_lanes_apply_gain_before_stems_and_reject_invalid_graphs() {
    let directory = tempdir().expect("directory");
    let score = directory.path().join("routed.score.yaml");
    let output = directory.path().join("mix.wav");
    let stems = directory.path().join("stems");
    let mut doc = json!({"meta":{"title":"Routed score","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},"form":[{"section":"phrase","bars":1,"tonic":"C","scale":"ionian"}],"sections":{"phrase":{"placements":[{"instrument":"source","degree":"1","at":{"bar":1,"step":1},"duration_quarters":1},{"instrument":"source","degree":"3","gain_db":-6,"at":{"bar":1,"step":1},"duration_quarters":1},{"instrument":"receiver","degree":"1","at":{"bar":1,"step":1},"duration_quarters":1}]}},"instruments":{"receiver":{"definition":fixture_path("instruments/sidechain-ducking.json"),"audio_input":"source"},"source":{"definition":fixture_path("instruments/basic-poly-synth.json"),"gain_db":-6}}});
    std::fs::write(&score, serde_json::to_string_pretty(&doc).expect("YAML")).expect("score");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["render", "score"])
        .arg(&score)
        .arg("--output")
        .arg(&output)
        .arg("--stems-dir")
        .arg(&stems)
        .args(["--tail", "0", "--json"])
        .assert()
        .success();
    let read = |p: &std::path::Path| {
        hound::WavReader::open(p)
            .expect("WAV")
            .into_samples::<f32>()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
    };
    let source = read(&stems.join("source.wav"));
    let receiver = read(&stems.join("receiver.wav"));
    let mix = read(&output);
    assert!(source.iter().any(|s| s.abs() > 0.01));
    assert_eq!(mix.len(), source.len());
    assert!(
        mix.iter()
            .zip(source.iter().zip(&receiver))
            .all(|(m, (a, b))| (m - a - b).abs() < 1e-6)
    );
    for invalid in ["missing", "receiver"] {
        doc["instruments"]["receiver"]["audio_input"] = json!(invalid);
        std::fs::write(&score, serde_json::to_string_pretty(&doc).expect("YAML")).expect("score");
        let output = Command::cargo_bin("sonalloy")
            .expect("binary")
            .args(["score", "validate"])
            .arg(&score)
            .arg("--json")
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let report: serde_json::Value = serde_json::from_slice(&output).expect("diagnostics");
        let diagnostic = report["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .find(|diagnostic| diagnostic["path"] == "instruments.receiver.audio_input")
            .expect("routing field");
        assert_eq!(diagnostic["actual"], invalid);
        assert!(diagnostic["line"].as_u64().expect("source line") > 0);
    }
}

fn assert_control_and_channel_import(directory: &std::path::Path) {
    let sustain = write_sustain_midi(directory);
    let controlled = directory.join("controlled.score.yaml");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["score", "import-midi"])
        .arg(sustain)
        .arg("--output")
        .arg(&controlled)
        .arg("--instrument")
        .arg(fixture_path("instruments/basic-poly-synth.json"))
        .assert()
        .success();
    let inspection = inspect(&controlled);
    assert_eq!(inspection["parts"][0]["tempo_changes"][1]["tick"], 240);
    assert_eq!(
        inspection["parts"][0]["time_signature_changes"][1]["tick"],
        360
    );
    assert_eq!(inspection["sections"][0]["end_tick"], 360);
    assert_eq!(
        inspection["parts"][0]["tempo_changes"]
            .as_array()
            .expect("tempo map")
            .len(),
        2
    );
    let events = inspection["parts"][0]["events"]
        .as_array()
        .expect("events")
        .clone();
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "sustain_pedal" && e["down"] == true)
    );
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "sustain_pedal" && e["down"] == false)
    );
    assert!(
        events
            .iter()
            .any(|e| e["type"] == "mod_wheel" && e["tick"] == 137)
    );
    for kind in ["pitch_bend", "aftertouch"] {
        assert!(
            events
                .iter()
                .any(|event| event["type"] == kind && event["tick"] == 137)
        );
    }
    assert_controlled_midi_round_trip(directory, &controlled, &inspection);
    let multichannel = write_multi_channel_midi(directory);
    let selected = directory.join("selected.score.yaml");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["score", "import-midi"])
        .arg(&multichannel)
        .arg("--output")
        .arg(&selected)
        .arg("--instrument")
        .arg(fixture_path("instruments/basic-poly-synth.json"))
        .assert()
        .failure();
    assert!(!selected.exists());
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["score", "import-midi"])
        .arg(multichannel)
        .arg("--output")
        .arg(&selected)
        .arg("--instrument")
        .arg(fixture_path("instruments/basic-poly-synth.json"))
        .args(["--channel", "2"])
        .assert()
        .success();
    assert_eq!(inspect(&selected)["parts"][0]["events"][0]["note"], 60);
}

fn assert_controlled_midi_round_trip(
    directory: &std::path::Path,
    score: &std::path::Path,
    original: &serde_json::Value,
) {
    let midi = directory.join("controlled-export.mid");
    let imported = directory.join("controlled-round-trip.score.yaml");
    Command::cargo_bin("sonalloy")
        .unwrap()
        .args(["score", "export-midi"])
        .arg(score)
        .arg("--output")
        .arg(&midi)
        .assert()
        .success();
    let bytes = std::fs::read(&midi).unwrap();
    let smf = Smf::parse(&bytes).unwrap();
    let mut tick = 0;
    let mut changes = Vec::new();
    for event in &smf.tracks[0] {
        tick += event.delta.as_int();
        if matches!(
            event.kind,
            TrackEventKind::Meta(
                midly::MetaMessage::Tempo(_) | midly::MetaMessage::TimeSignature(..)
            )
        ) {
            changes.push(tick);
        }
    }
    assert_eq!(changes, [0, 0, 240, 360]);
    Command::cargo_bin("sonalloy")
        .unwrap()
        .args(["score", "import-midi"])
        .arg(midi)
        .arg("--output")
        .arg(&imported)
        .arg("--instrument")
        .arg(fixture_path("instruments/basic-poly-synth.json"))
        .assert()
        .success();
    let round_trip = inspect(&imported);
    let ordered_events = |report: &serde_json::Value| {
        let mut events = report["parts"][0]["events"].as_array().unwrap().clone();
        events.sort_by_key(|event| {
            (
                event["tick"].as_u64().unwrap(),
                event["type"].as_str().unwrap().to_owned(),
            )
        });
        events
    };
    assert_eq!(ordered_events(&round_trip), ordered_events(original));
    for field in ["tempo_changes", "time_signature_changes"] {
        assert_eq!(
            round_trip["parts"][0][field], original["parts"][0][field],
            "{field}"
        );
    }
}
#[test]
fn midi_export_preserves_notes_across_ports_and_reports_omitted_controls() {
    for count in [16, 17, 32, 41] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("score.yaml");
        let output = directory.path().join("score.mid");
        let mut instruments = serde_json::Map::new();
        let mut placements = Vec::new();
        for i in 0..count {
            let id = format!("part{i:02}");
            instruments.insert(
                id.clone(),
                json!({"definition":fixture_path("instruments/basic-poly-synth.json")}),
            );
            placements.push(json!({"instrument":id,"degree":"1","at":{"bar":1,"step":1},"duration_quarters":1,"velocity":100}));
        }
        placements.push(json!({"instrument":"part00","degree":"1","at":{"bar":1,"step":3},"duration_quarters":1,"gain_db":-6,"velocity":80}));
        let at = json!({"bar":1,"step":1});
        let doc = json!({"meta":{"title":"Ports","bpm":120,"meter":"4/4","tuning":"12edo"},
        "form":[{"section":"a","bars":1,"tonic":"C"}],"instruments":instruments,
        "sections":{"a":{"placements":placements,"events":[
            {"kind":"sustain_pedal","instrument":"part00","at":at,"down":true},
            {"kind":"pitch_bend","instrument":"part00","at":at,"value":0.5},
            {"kind":"mod_wheel","instrument":"part00","at":at,"value":0.5},
            {"kind":"aftertouch","instrument":"part00","at":at,"value":0.5},
            {"kind":"parameter_change","instrument":"part00","at":at,"parameter":"layer.body.gain","native_value":-12},
            {"kind":"automation","instrument":"part00","at":{"bar":1,"step":5},"parameter":"layer.body.gain","from_value":-12,"to_value":-14,"duration_quarters":1}
        ]}}});
        std::fs::write(&path, doc.to_string()).unwrap();
        let result = Command::cargo_bin("sonalloy")
            .unwrap()
            .args(["score", "export-midi"])
            .arg(&path)
            .arg("--output")
            .arg(&output)
            .arg("--json")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let report: serde_json::Value = serde_json::from_slice(&result).unwrap();
        let warnings = report["diagnostics"].as_array().unwrap();
        for expected in [
            "sustain pedal omitted",
            "pitch bend omitted",
            "mod wheel omitted",
            "aftertouch omitted",
            "1 ParameterChange, 1 ParameterRamp",
            "1 overlapping same-pitch",
        ] {
            assert!(
                warnings.iter().any(|d| d["path"] == "instruments.part00"
                    && d["severity"] == "warning"
                    && d["message"].as_str().unwrap().contains(expected)),
                "{expected}: {report}"
            );
        }
        let bytes = std::fs::read(&output).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        assert_eq!(smf.tracks.len(), count + 1);
        assert_eq!(smf.header.format, Format::Parallel);
        assert_exported_tracks(&smf);
        Command::cargo_bin("sonalloy")
            .unwrap()
            .args(["score", "export-midi"])
            .arg(&path)
            .arg("--output")
            .arg(&output)
            .assert()
            .failure();
        assert_eq!(std::fs::read(output).unwrap(), bytes);
    }
}

fn assert_exported_tracks(smf: &Smf<'_>) {
    for (i, track) in smf.tracks[1..].iter().enumerate() {
        let expected_name = format!("part{i:02}");
        assert!(track.iter().any(|e| matches!(e.kind, TrackEventKind::Meta(midly::MetaMessage::TrackName(name)) if name == expected_name.as_bytes())));
        let port = track.iter().find_map(|e| {
            if let TrackEventKind::Meta(midly::MetaMessage::MidiPort(p)) = e.kind {
                Some(p.as_int())
            } else {
                None
            }
        });
        assert_eq!(
            port,
            if i < 16 {
                None
            } else {
                Some(u8::try_from(i / 16).unwrap())
            }
        );
        let mut tick = 0_u64;
        let mut notes = Vec::new();
        for event in track {
            tick += u64::from(event.delta.as_int());
            if let TrackEventKind::Midi { channel, message } = event.kind {
                assert_eq!(usize::from(channel.as_int()), i % 16);
                match message {
                    MidiMessage::NoteOn { key, vel } => {
                        notes.push((tick, true, key.as_int(), vel.as_int()));
                    }
                    MidiMessage::NoteOff { key, .. } => {
                        notes.push((tick, false, key.as_int(), 0));
                    }
                    _ => panic!("conflicting controls must be omitted"),
                }
            }
        }
        assert_eq!(
            notes,
            if i == 0 {
                vec![
                    (0, true, 60, 100),
                    (240, true, 60, 80),
                    (480, false, 60, 0),
                    (720, false, 60, 0),
                ]
            } else {
                vec![(0, true, 60, 100), (480, false, 60, 0)]
            }
        );
    }
}
