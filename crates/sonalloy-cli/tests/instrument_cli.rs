mod support;

use assert_cmd::Command;
use serde_json::json;
use support::*;
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
fn spectral_reference_and_hybrid_support_inspect_and_midi_render() {
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

    let directory = tempdir().expect("temporary output directory");
    let output = directory.path().join("spectral-hybrid-midi.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "midi",
            fixture_path("instruments/spectral-hybrid-reference.json")
                .to_str()
                .expect("utf-8 definition path"),
            fixture_path("midi/basic-poly-synth-phrase.mid")
                .to_str()
                .expect("utf-8 MIDI path"),
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--tail",
            "0.1",
            "--output",
            output.to_str().expect("utf-8 output path"),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"status\":\"ok\""))
        .stdout(predicates::str::contains(
            "\"reported_latency_frames\":1536",
        ));
    let mut reader = hound::WavReader::open(output).expect("spectral hybrid MIDI WAV");
    assert_eq!(reader.spec().channels, 2);
    let samples = reader
        .samples::<f32>()
        .map(|sample| sample.expect("finite spectral hybrid sample"))
        .collect::<Vec<_>>();
    assert!(samples.iter().all(|sample| sample.is_finite()));
    assert!(samples.iter().any(|sample| sample.abs() > 0.01));
}

#[test]
fn operator_modulation_validate_inspect_and_render() {
    let definition = fixture_path("instruments/operator-modulation-reference.json");
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
        .stdout(predicates::str::contains(
            "\"kind\":\"operator_modulation\"",
        ))
        .stdout(predicates::str::contains("\"mode\":\"phase\""))
        .stdout(predicates::str::contains("\"algorithm\":\"stack_4\""))
        .stdout(predicates::str::contains("\"evaluation_order\":[4,3,2,1]"))
        .stdout(predicates::str::contains(
            "layer.body.generator.operator.2.modulation_amount",
        ))
        .stdout(predicates::str::contains("\"unison_voices\":4"));

    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("operator.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "note",
            definition.to_str().expect("utf-8 definition path"),
            "--note",
            "60",
            "--gate",
            "0.05",
            "--tail",
            "0",
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
    let reader = hound::WavReader::open(output).expect("operator render output");
    assert_eq!(reader.spec().channels, 2);
    assert!(
        reader
            .into_samples::<f32>()
            .map(|sample| sample.expect("valid sample"))
            .all(f32::is_finite)
    );
}

#[test]
fn additive_generator_validate_inspect_and_render() {
    let definition = fixture_path("instruments/additive-generator-reference.json");
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
        .stdout(predicates::str::contains("\"kind\":\"additive\""))
        .stdout(predicates::str::contains("\"partial_count\":8"))
        .stdout(predicates::str::contains("\"max_partial_count\":64"))
        .stdout(predicates::str::contains("\"id\":\"fundamental\""))
        .stdout(predicates::str::contains("\"has_envelope\":true"))
        .stdout(predicates::str::contains(
            "layer.body.generator.additive_spectrum_tilt",
        ));

    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("additive.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "note",
            definition.to_str().expect("utf-8 definition path"),
            "--note",
            "60",
            "--gate",
            "0.05",
            "--tail",
            "0",
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
    let reader = hound::WavReader::open(output).expect("additive render output");
    assert_eq!(reader.spec().channels, 2);
    assert!(
        reader
            .into_samples::<f32>()
            .map(|sample| sample.expect("valid sample"))
            .all(f32::is_finite)
    );
}

#[test]
fn formant_generator_validate_inspect_and_render() {
    let definition = fixture_path("instruments/formant-generator-reference.json");
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
        .stdout(predicates::str::contains("\"kind\":\"formant\""))
        .stdout(predicates::str::contains("\"partial_count\":48"))
        .stdout(predicates::str::contains("\"profile_count\":5"))
        .stdout(predicates::str::contains("\"id\":\"a\""))
        .stdout(predicates::str::contains("\"frequency_hz\":800.0"))
        .stdout(predicates::str::contains(
            "layer.voice.generator.formant_vowel_position",
        ));

    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("formant.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "note",
            definition.to_str().expect("utf-8 definition path"),
            "--note",
            "60",
            "--gate",
            "0.05",
            "--tail",
            "0",
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
    let reader = hound::WavReader::open(output).expect("formant render output");
    assert_eq!(reader.spec().channels, 2);
    assert!(
        reader
            .into_samples::<f32>()
            .map(|sample| sample.expect("valid sample"))
            .all(f32::is_finite)
    );
}

#[test]
fn complex_oscillator_validate_inspect_and_render() {
    let definition = fixture_path("instruments/complex-oscillator-reference.json");
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
        .stdout(predicates::str::contains(
            "\"backend\":\"variable_shape_sync\"",
        ))
        .stdout(predicates::str::contains("\"sync_ratio_parameter\""))
        .stdout(predicates::str::contains("\"unison_voices\":5"))
        .stdout(predicates::str::contains("\"phase_spread\":0.0"))
        .stdout(predicates::str::contains("\"unit\":\"ratio\""));

    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("complex.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "note",
            definition.to_str().expect("utf-8 definition path"),
            "--note",
            "60",
            "--gate",
            "0.05",
            "--tail",
            "0",
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
    let mut reader = hound::WavReader::open(output).expect("complex render output");
    assert_eq!(reader.spec().channels, 2);
    assert!(
        reader
            .samples::<f32>()
            .map(|sample| sample.expect("valid sample"))
            .all(f32::is_finite)
    );
}

#[test]
fn phase_domain_oscillator_validate_inspect_and_render() {
    let definition = fixture_path("instruments/complex-oscillator-phase-reference.json");
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
        .stdout(predicates::str::contains("\"backend\":\"phase_domain\""))
        .stdout(predicates::str::contains("\"phase_distortion_parameter\""))
        .stdout(predicates::str::contains("\"wavefold_parameter\""))
        .stdout(predicates::str::contains(
            "\"oscillator_feedback_parameter\"",
        ))
        .stdout(predicates::str::contains("\"dc_blocker\":true"))
        .stdout(predicates::str::contains("\"signal_order\""));

    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("phase-domain.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "note",
            definition.to_str().expect("utf-8 definition path"),
            "--note",
            "72",
            "--gate",
            "0.05",
            "--tail",
            "0",
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
    let reader = hound::WavReader::open(output).expect("phase-domain render output");
    assert_eq!(reader.spec().channels, 2);
    assert!(
        reader
            .into_samples::<f32>()
            .map(|sample| sample.expect("valid sample"))
            .all(f32::is_finite)
    );
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
fn hybrid_midi_render_writes_stereo_audio() {
    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("hybrid.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "midi",
            fixture_path("instruments/metallic-hybrid.json")
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
fn harmonic_formant_hybrid_inspects_all_layers_and_renders_midi() {
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

    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("harmonic-formant-hybrid.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "midi",
            definition.to_str().expect("utf-8 definition path"),
            fixture_path("midi/basic-poly-synth-phrase.mid")
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
fn processed_hybrid_inspects_and_renders_processor_chains() {
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

    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("processed-hybrid.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "events",
            fixture_path("instruments/processed-hybrid.json")
                .to_str()
                .expect("utf-8 definition path"),
            fixture_path("events/processed-hybrid.json")
                .to_str()
                .expect("utf-8 event path"),
            "--duration-frames",
            "120000",
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
    let mut reader = hound::WavReader::open(output).expect("processed hybrid WAV");
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
