mod support;

use assert_cmd::Command;
use serde_json::json;
use support::fixture_path;
use tempfile::tempdir;

fn write_spectral_definition(directory: &std::path::Path) -> std::path::PathBuf {
    let definition = directory.join("spectral.json");
    let asset = fixture_path("assets/metal-hit.wav");
    let mut value: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixture_path("instruments/basic-poly-synth.json"))
            .expect("reference definition reads"),
    )
    .expect("reference definition parses");
    value["metadata"]["name"] = json!("Spectral Inspect");
    value["metadata"]["description"] = json!("Spectral inspection fixture");
    value["performance"]["polyphony"] = json!(1);
    value["layers"][0]["gain_db"] = json!(0.0);
    value["layers"][0]["envelope"] = json!({
        "attack_seconds": 0.0,
        "decay_seconds": 0.0,
        "sustain_level": 1.0,
        "release_seconds": 0.01
    });
    value["layers"][0]["generator"] = json!({
        "spectral": {
            "asset_a": {
                "path": asset.to_str().expect("asset path is utf-8"),
                "sha256": null
            },
            "asset_b": null,
            "root_note": 60,
            "fft_size": 1024,
            "position": 0.0,
            "freeze": 0.0,
            "blur_seconds": 0.0,
            "shift_hz": 0.0,
            "morph": 0.0,
            "phase_reset": true
        }
    });
    value["layers"][0]["processors"] = json!([]);
    value["voice_processors"] = json!([]);
    value["global_processors"] = json!([]);
    value["modulation"] = serde_json::Value::Null;
    std::fs::write(
        &definition,
        serde_json::to_vec_pretty(&value).expect("spectral definition serializes"),
    )
    .expect("spectral definition writes");
    definition
}

fn assert_measured_loudness(loudness: &serde_json::Value) {
    assert_eq!(loudness["status"], "measured");
    for field in ["integrated_lufs", "true_peak_db"] {
        assert!(loudness[field].as_f64().is_some_and(f64::is_finite));
    }
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
        .stdout(predicates::str::contains("\"asset_status\""))
        .stdout(predicates::str::contains("\"mode\":\"low_pass\""))
        .stdout(predicates::str::contains("\"voice.processor.tone.cutoff\""))
        .stdout(predicates::str::contains("\"effective_max_cutoff_hz\""))
        .stdout(predicates::str::contains(
            "\"voice.processor.tone.resonance\"",
        ));
}

#[test]
fn spectral_definitions_validate_and_inspect_all_asset_configurations() {
    let directory = tempdir().expect("temporary directory");
    let definitions = [
        write_spectral_definition(directory.path()),
        fixture_path("instruments/spectral-generator-reference.json"),
        fixture_path("instruments/spectral-hybrid-reference.json"),
    ];

    for definition in &definitions {
        Command::cargo_bin("sonalloy")
            .expect("binary")
            .args([
                "instrument",
                "validate",
                definition.to_str().expect("utf-8 definition path"),
                "--json",
            ])
            .assert()
            .success()
            .stdout(predicates::str::contains("\"status\":\"ok\""));
    }

    let inspections: [(&std::path::Path, &[&str]); 3] = [
        (
            &definitions[0],
            &[
                "\"kind\":\"spectral\"",
                "\"asset_a_prepared\":true",
                "\"asset_b_prepared\":false",
                "\"prepared_sample_rate\":48000",
                "\"fft_size\":1024",
                "\"hop_size\":256",
                "\"latency_frames\":768",
                "spectral_position",
                "spectral_frame_count",
            ],
        ),
        (
            &definitions[1],
            &[
                "\"kind\":\"spectral\"",
                "\"output_mode\":\"stereo\"",
                "\"asset_b_prepared\":true",
                "\"fft_size\":2048",
                "\"hop_size\":512",
                "spectral_morph",
            ],
        ),
        (
            &definitions[2],
            &[
                "\"layer_count\":4",
                "\"kind\":\"additive\"",
                "\"kind\":\"sample\"",
                "\"kind\":\"noise\"",
                "layer.spectral.generator.spectral_position",
                "global.processor.space.mix",
            ],
        ),
    ];
    for (definition, expected_fields) in inspections {
        let mut inspection = Command::cargo_bin("sonalloy")
            .expect("binary")
            .args([
                "instrument",
                "inspect",
                definition.to_str().expect("utf-8 definition path"),
                "--json",
            ])
            .assert()
            .success();
        for field in expected_fields {
            inspection = inspection.stdout(predicates::str::contains(*field));
        }
    }
}

#[test]
fn spectral_definition_renders_audible_audio() {
    let directory = tempdir().expect("temporary directory");
    let definition = write_spectral_definition(directory.path());
    let output = directory.path().join("spectral.wav");

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "note",
            definition.to_str().expect("utf-8 definition path"),
            "--note",
            "60",
            "--gate",
            "0.1",
            "--tail",
            "0.1",
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--output",
            output.to_str().expect("utf-8 output path"),
        ])
        .assert()
        .success();
    let samples = hound::WavReader::open(output)
        .expect("spectral WAV")
        .into_samples::<f32>()
        .map(|sample| sample.expect("finite spectral sample"))
        .collect::<Vec<_>>();
    assert!(samples.iter().all(|sample| sample.is_finite()));
    assert!(samples.iter().any(|sample| sample.abs() > 0.01));
}

#[test]
fn generator_definitions_validate_and_inspect() {
    let cases: &[(&str, &[&str])] = &[
        (
            "instruments/basic-generators-reference.json",
            &[
                "\"waveform\":\"square\"",
                "\"waveform\":\"triangle\"",
                "\"waveform\":\"pulse\"",
                "\"kind\":\"noise\"",
                "\"output_mode\":\"stereo\"",
                "layer.pulse.generator.pulse_width",
                "layer.pink.generator.noise_correlation",
            ],
        ),
        (
            "instruments/operator-modulation-reference.json",
            &[
                "\"kind\":\"operator_modulation\"",
                "\"mode\":\"phase\"",
                "\"algorithm\":\"stack_4\"",
                "\"evaluation_order\":[4,3,2,1]",
                "layer.body.generator.operator.2.modulation_amount",
                "\"unison_voices\":4",
            ],
        ),
        (
            "instruments/additive-generator-reference.json",
            &[
                "\"kind\":\"additive\"",
                "\"partial_count\":8",
                "\"max_partial_count\":64",
                "\"id\":\"fundamental\"",
                "\"has_envelope\":true",
                "layer.body.generator.additive_spectrum_tilt",
            ],
        ),
        (
            "instruments/formant-generator-reference.json",
            &[
                "\"kind\":\"formant\"",
                "\"partial_count\":48",
                "\"profile_count\":5",
                "\"id\":\"a\"",
                "\"frequency_hz\":800.0",
                "layer.voice.generator.formant_vowel_position",
            ],
        ),
        (
            "instruments/complex-oscillator-reference.json",
            &[
                "\"backend\":\"variable_shape_sync\"",
                "\"sync_ratio_parameter\"",
                "\"unison_voices\":5",
                "\"phase_spread\":0.0",
                "\"unit\":\"ratio\"",
            ],
        ),
        (
            "instruments/complex-oscillator-phase-reference.json",
            &[
                "\"backend\":\"phase_domain\"",
                "\"phase_distortion_parameter\"",
                "\"wavefold_parameter\"",
                "\"oscillator_feedback_parameter\"",
                "\"dc_blocker\":true",
                "\"signal_order\"",
            ],
        ),
    ];

    for &(fixture, expected_fields) in cases {
        let definition = fixture_path(fixture);
        let definition_path = definition.to_str().expect("utf-8 definition path");
        Command::cargo_bin("sonalloy")
            .expect("binary")
            .args(["instrument", "validate", definition_path, "--json"])
            .assert()
            .success()
            .stdout(predicates::str::contains("\"status\":\"ok\""));

        let mut inspection = Command::cargo_bin("sonalloy")
            .expect("binary")
            .args(["instrument", "inspect", definition_path, "--json"])
            .assert()
            .success();
        for expected_field in expected_fields {
            inspection = inspection.stdout(predicates::str::contains(*expected_field));
        }
        inspection.stderr(predicates::str::is_empty());
    }
}

#[test]
fn inspect_lists_external_modulation_sources() {
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "inspect",
            fixture_path("instruments/expressive-hybrid-lead.json")
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
        .stdout(predicates::str::contains("\"kind\":\"external_control\""))
        .stdout(predicates::str::contains("\"max_abs_depth\""))
        .stdout(predicates::str::contains("\"polarity\":\"bipolar\""))
        .stdout(predicates::str::contains("\"effect\""));
}

#[test]
fn instrument_validate_collects_directories_globs_and_all_file_results() {
    let directory = tempdir().expect("temporary directory");
    let root = directory.path().join("instruments");
    let first = root.join("first/definition.json");
    let invalid = root.join("invalid/definition.json");
    let nested = root.join("nested/third/definition.json");
    for path in [&first, &nested] {
        std::fs::create_dir_all(path.parent().expect("definition directory"))
            .expect("directory created");
        std::fs::copy(fixture_path("instruments/basic-poly-synth.json"), path)
            .expect("valid Definition copied");
    }
    std::fs::create_dir_all(invalid.parent().expect("invalid directory"))
        .expect("directory created");
    std::fs::write(&invalid, "{invalid json").expect("invalid Definition written");

    let missing = directory.path().join("missing.json");
    let unmatched_glob = format!("{}/unmatched-*/definition.json", root.display());
    let output = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["instrument", "validate"])
        .arg(&first)
        .arg(&root)
        .arg(&missing)
        .arg(&unmatched_glob)
        .arg("--json")
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).expect("batch report");
    assert_eq!(report["status"], "error");
    let files = report["files"].as_array().expect("per-file results");
    assert_eq!(
        files.len(),
        5,
        "the direct file is de-duplicated and input errors are collected"
    );
    assert_eq!(
        files.iter().filter(|file| file["status"] == "ok").count(),
        2
    );
    assert_eq!(
        files
            .iter()
            .filter(|file| file["status"] == "error")
            .count(),
        3
    );
    assert!(files.iter().any(|file| {
        file["path"] == missing.to_string_lossy().as_ref() && file["status"] == "error"
    }));
    assert!(
        files
            .iter()
            .any(|file| { file["path"] == unmatched_glob && file["status"] == "error" })
    );

    let glob = format!("{}/*/definition.json", root.display());
    let output = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["instrument", "validate", &glob, "--json"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).expect("glob report");
    assert_eq!(report["files"].as_array().expect("glob files").len(), 2);
}

#[test]
fn instrument_report_matches_render_note_analysis_and_keeps_per_file_failures() {
    let directory = tempdir().expect("temporary directory");
    let root = directory.path().join("instruments");
    let valid = root.join("valid/definition.json");
    let invalid = root.join("invalid/definition.json");
    std::fs::create_dir_all(valid.parent().expect("valid directory")).expect("directory created");
    std::fs::create_dir_all(invalid.parent().expect("invalid directory"))
        .expect("directory created");
    std::fs::copy(fixture_path("instruments/basic-poly-synth.json"), &valid)
        .expect("valid Definition copied");
    std::fs::write(&invalid, "{invalid json").expect("invalid Definition written");

    let missing = directory.path().join("missing.json");
    let unmatched_glob = format!("{}/unmatched-*/definition.json", root.display());
    let report_output = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["instrument", "report"])
        .arg(&root)
        .arg(&missing)
        .arg(&unmatched_glob)
        .args([
            "--note",
            "60",
            "--velocity",
            "127",
            "--gate",
            "0.5",
            "--tail",
            "0.5",
            "--loudness",
            "--json",
        ])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value =
        serde_json::from_slice(&report_output).expect("instrument report");
    assert_eq!(report["status"], "error");
    assert_eq!(report["note"], 60);
    assert_eq!(report["velocity"], 127);
    assert_eq!(report["gate_seconds"], 0.5);
    assert_eq!(report["tail_seconds"], 0.5);
    assert_eq!(report["sample_rate"], 48_000);
    let files = report["files"].as_array().expect("per-file results");
    assert_eq!(files.len(), 4);
    assert!(files.iter().any(|file| {
        file["path"] == missing.to_string_lossy().as_ref() && file["status"] == "error"
    }));
    assert!(
        files
            .iter()
            .any(|file| { file["path"] == unmatched_glob && file["status"] == "error" })
    );
    let file = files
        .iter()
        .find(|file| file["status"] == "ok")
        .expect("valid Definition report");
    assert_eq!(file["analysis"]["sample_rate"], 48_000);
    assert_measured_loudness(&file["loudness"]);
    assert_eq!(file["analysis"]["channels"], 2);
    assert_eq!(
        file["analysis"]["spectrum"]["bands"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    let note_output = directory.path().join("note.wav");
    let rendered = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["render", "note"])
        .arg(&valid)
        .args([
            "--note",
            "60",
            "--velocity",
            "127",
            "--gate",
            "0.5",
            "--tail",
            "0.5",
            "--output",
        ])
        .arg(&note_output)
        .args(["--analyze", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rendered: serde_json::Value = serde_json::from_slice(&rendered).expect("render report");
    let mut expected = rendered["analysis"].clone();
    expected
        .as_object_mut()
        .expect("analysis object")
        .remove("loudness");
    assert_eq!(file["analysis"], expected);
}

#[test]
fn instrument_report_silence_input_only_reaches_audio_consumers() {
    let output = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["instrument", "report"])
        .arg(fixture_path("instruments/basic-poly-synth.json"))
        .arg(fixture_path("instruments/sidechain-ducking.json"))
        .args([
            "--audio-input",
            "silence",
            "--gate",
            "0.1",
            "--tail",
            "0.1",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).expect("report");

    assert_eq!(report["status"], "ok");
    let files = report["files"].as_array().expect("files");
    assert_eq!(files.len(), 2);
    assert!(files.iter().all(|file| file["status"] == "ok"));
    assert!(files.iter().all(|file| file.get("loudness").is_none()));
}

#[test]
fn instrument_report_text_includes_all_band_energy_ratios() {
    let output = Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["instrument", "report"])
        .arg(fixture_path("instruments/basic-poly-synth.json"))
        .args([
            "--note",
            "60",
            "--velocity",
            "127",
            "--gate",
            "0.1",
            "--tail",
            "0.1",
            "--loudness",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8(output).expect("report output");
    assert!(output.contains("sample rate 48000 Hz"));
    assert!(output.contains("loudness:"));
    for band in [
        "20–60 Hz",
        "60–120 Hz",
        "120–250 Hz",
        "250–500 Hz",
        "500–2000 Hz",
        "2000–6000 Hz",
        "6000–20000 Hz",
    ] {
        assert!(output.contains(band), "missing band {band} in {output}");
    }
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
fn hybrid_validate_and_inspect_report_sample_layers() {
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "validate",
            fixture_path("instruments/metallic-hybrid.json")
                .to_str()
                .expect("utf-8 definition path"),
            "--json",
        ])
        .assert()
        .success();

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "inspect",
            fixture_path("instruments/metallic-hybrid.json")
                .to_str()
                .expect("utf-8 definition path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"layer_count\":2"))
        .stdout(predicates::str::contains("\"kind\":\"sample\""))
        .stdout(predicates::str::contains("\"asset_status\":\"enabled\""))
        .stdout(predicates::str::contains("\"sample_zone_count\":1"))
        .stdout(predicates::str::contains("\"sample_enabled_zone_count\":1"))
        .stdout(predicates::str::contains("\"sample_asset_count\":1"))
        .stdout(predicates::str::contains("\"sample_zones\""))
        .stdout(predicates::str::contains("\"playback_type\":\"one_shot\""));

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "inspect",
            fixture_path("instruments/metallic-hybrid.json")
                .to_str()
                .expect("utf-8 definition path"),
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("note_off_behavior: release"))
        .stdout(predicates::str::contains("normalization: disabled"))
        .stdout(predicates::str::contains(
            "pitch_tracking true gain_db 0.0 normalize_gain_db 0.0 effective_gain_db 0.0",
        ));
}

#[test]
fn harmonic_formant_hybrid_inspects_all_layers() {
    let definition = fixture_path("instruments/harmonic-formant-hybrid-reference.json");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "validate",
            definition.to_str().expect("utf-8 definition path"),
            "--json",
        ])
        .assert()
        .success();

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
        .stdout(predicates::str::contains("\"layer_count\":4"))
        .stdout(predicates::str::contains("\"kind\":\"formant\""))
        .stdout(predicates::str::contains("\"kind\":\"additive\""))
        .stdout(predicates::str::contains("\"kind\":\"sample\""))
        .stdout(predicates::str::contains("\"kind\":\"noise\""))
        .stdout(predicates::str::contains("formant_vowel_position"))
        .stdout(predicates::str::contains("voice_tone"))
        .stdout(predicates::str::contains("voice_glue"))
        .stdout(predicates::str::contains("echo"))
        .stdout(predicates::str::contains("space"));
}

#[test]
fn processed_hybrid_inspects_processor_chains() {
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "validate",
            fixture_path("instruments/processed-hybrid.json")
                .to_str()
                .expect("utf-8 definition path"),
            "--json",
        ])
        .assert()
        .success();

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "inspect",
            fixture_path("instruments/processed-hybrid.json")
                .to_str()
                .expect("utf-8 definition path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("attack_drive"))
        .stdout(predicates::str::contains("body_tone"))
        .stdout(predicates::str::contains("voice.processor.tone.cutoff"))
        .stdout(predicates::str::contains("global.processor.space.mix"))
        .stdout(predicates::str::contains("\"id\":\"time\""))
        .stdout(predicates::str::contains("pre_delay_frames"));
}

#[test]
fn missing_required_asset_fails_rendering() {
    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("fallback.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "midi",
            fixture_path("instruments/metallic-hybrid-missing-asset.json")
                .to_str()
                .expect("utf-8 definition path"),
            fixture_path("midi/metallic-hybrid-phrase.mid")
                .to_str()
                .expect("utf-8 MIDI path"),
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
        .failure()
        .stdout(predicates::str::contains("\"ASSET_NOT_FOUND\""));
    assert!(!output.exists());
}
