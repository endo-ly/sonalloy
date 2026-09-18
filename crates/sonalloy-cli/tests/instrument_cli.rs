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
fn basic_generators_validate_and_inspect_all_generator_modes() {
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "validate",
            fixture_path("instruments/basic-generators-reference.json")
                .to_str()
                .expect("utf-8 definition path"),
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
            fixture_path("instruments/basic-generators-reference.json")
                .to_str()
                .expect("utf-8 definition path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"waveform\":\"square\""))
        .stdout(predicates::str::contains("\"waveform\":\"triangle\""))
        .stdout(predicates::str::contains("\"waveform\":\"pulse\""))
        .stdout(predicates::str::contains("\"kind\":\"noise\""))
        .stdout(predicates::str::contains("\"output_mode\":\"stereo\""))
        .stdout(predicates::str::contains(
            "layer.pulse.generator.pulse_width",
        ))
        .stdout(predicates::str::contains(
            "layer.pink.generator.noise_correlation",
        ));
}

#[test]
fn spectral_validate_and_inspect_reports_prepared_asset() {
    let directory = tempdir().expect("temporary directory");
    let definition = write_spectral_definition(directory.path());

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
        .stdout(predicates::str::contains("\"kind\":\"spectral\""))
        .stdout(predicates::str::contains("\"asset_a_prepared\":true"))
        .stdout(predicates::str::contains("\"asset_b_prepared\":false"))
        .stdout(predicates::str::contains("\"prepared_sample_rate\":48000"))
        .stdout(predicates::str::contains("\"fft_size\":1024"))
        .stdout(predicates::str::contains("\"hop_size\":256"))
        .stdout(predicates::str::contains("\"latency_frames\":768"))
        .stdout(predicates::str::contains("spectral_position"))
        .stdout(predicates::str::contains("spectral_frame_count"));

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
    let mut reader = hound::WavReader::open(output).expect("spectral WAV");
    let samples = reader
        .samples::<f32>()
        .map(|sample| sample.expect("finite spectral sample"))
        .collect::<Vec<_>>();
    assert!(samples.iter().all(|sample| sample.is_finite()));
    assert!(samples.iter().any(|sample| sample.abs() > 0.01));
}

#[test]
fn spectral_reference_and_hybrid_support_inspect() {
    for definition in [
        fixture_path("instruments/spectral-generator-reference.json"),
        fixture_path("instruments/spectral-hybrid-reference.json"),
    ] {
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

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "inspect",
            fixture_path("instruments/spectral-generator-reference.json")
                .to_str()
                .expect("utf-8 definition path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"kind\":\"spectral\""))
        .stdout(predicates::str::contains("\"output_mode\":\"stereo\""))
        .stdout(predicates::str::contains("\"asset_a_prepared\":true"))
        .stdout(predicates::str::contains("\"asset_b_prepared\":true"))
        .stdout(predicates::str::contains("\"fft_size\":2048"))
        .stdout(predicates::str::contains("\"hop_size\":512"))
        .stdout(predicates::str::contains("spectral_morph"));

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "instrument",
            "inspect",
            fixture_path("instruments/spectral-hybrid-reference.json")
                .to_str()
                .expect("utf-8 definition path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"layer_count\":4"))
        .stdout(predicates::str::contains("\"kind\":\"additive\""))
        .stdout(predicates::str::contains("\"kind\":\"sample\""))
        .stdout(predicates::str::contains("\"kind\":\"noise\""))
        .stdout(predicates::str::contains(
            "layer.spectral.generator.spectral_position",
        ))
        .stdout(predicates::str::contains("global.processor.space.mix"));
}

#[test]
fn generator_definitions_validate_and_inspect() {
    let cases: &[(&str, &[&str])] = &[
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
        .success()
        .stdout(predicates::str::contains("\"ASSET_RESAMPLED\""));

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
fn missing_asset_is_a_warning_and_body_still_renders() {
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
        .success()
        .stdout(predicates::str::contains("\"ASSET_NOT_FOUND\""));
    assert!(hound::WavReader::open(output).is_ok());
}
