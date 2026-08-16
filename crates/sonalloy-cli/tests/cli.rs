use assert_cmd::Command;
use midly::{Format, Header, MidiMessage, PitchBend, Smf, Timing, TrackEvent, TrackEventKind};
use serde_json::json;
use tempfile::tempdir;

fn reference_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/basic-poly-synth.json")
}

fn basic_generators_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/basic-generators-reference.json")
}

fn write_spectral_definition(directory: &std::path::Path) -> std::path::PathBuf {
    let definition = directory.join("spectral.json");
    let asset = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/assets/metal-hit.wav");
    let mut value: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(reference_definition()).expect("reference definition reads"),
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

fn complex_oscillator_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/complex-oscillator-reference.json")
}

fn complex_oscillator_phase_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/complex-oscillator-phase-reference.json")
}

fn operator_modulation_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/operator-modulation-reference.json")
}

fn additive_generator_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/additive-generator-reference.json")
}

fn formant_generator_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/formant-generator-reference.json")
}

fn harmonic_formant_hybrid_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/harmonic-formant-hybrid-reference.json")
}

fn spectral_reference_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/spectral-generator-reference.json")
}

fn spectral_hybrid_reference_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/spectral-hybrid-reference.json")
}

fn reference_midi() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/midi/basic-poly-synth-phrase.mid")
}

fn hybrid_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/metallic-hybrid.json")
}

fn hybrid_midi() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/midi/metallic-hybrid-phrase.mid")
}

fn expressive_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/expressive-hybrid-lead.json")
}

fn expressive_midi() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/midi/expressive-hybrid-controls.mid")
}

fn missing_asset_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/metallic-hybrid-missing-asset.json")
}

fn processed_definition() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/processed-hybrid.json")
}

fn processed_events() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/events/processed-hybrid.json")
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
            basic_generators_definition()
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
            basic_generators_definition()
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
        spectral_reference_definition(),
        spectral_hybrid_reference_definition(),
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
            spectral_reference_definition()
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
            spectral_hybrid_reference_definition()
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
            spectral_hybrid_reference_definition()
                .to_str()
                .expect("utf-8 definition path"),
            reference_midi().to_str().expect("utf-8 MIDI path"),
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
    let definition = operator_modulation_definition();
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
    let definition = additive_generator_definition();
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
    let definition = formant_generator_definition();
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
    let definition = complex_oscillator_definition();
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
    let definition = complex_oscillator_phase_definition();
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
        .stdout(predicates::str::contains("\"kind\":\"external_control\""))
        .stdout(predicates::str::contains("\"max_abs_depth\""))
        .stdout(predicates::str::contains("\"polarity\":\"bipolar\""))
        .stdout(predicates::str::contains("\"effect\""));
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
fn render_note_reports_analysis_and_trace_without_changing_audio() {
    let directory = tempdir().expect("temporary directory");
    let plain_output = directory.path().join("plain.wav");
    let traced_output = directory.path().join("traced.wav");
    let definition = reference_definition();
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
        r#"{"events":[{"absolute_frame":0,"type":"parameter_change","parameter":"layer.missing.gain","native_value":-24.0}]}"#,
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
fn harmonic_formant_hybrid_inspects_all_layers_and_renders_midi() {
    let definition = harmonic_formant_hybrid_definition();
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
            reference_midi().to_str().expect("utf-8 MIDI path"),
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
            processed_definition()
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
            processed_definition()
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
        .stdout(predicates::str::contains("time_frames"))
        .stdout(predicates::str::contains("pre_delay_frames"));

    let directory = tempdir().expect("temporary directory");
    let output = directory.path().join("processed-hybrid.wav");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "render",
            "events",
            processed_definition()
                .to_str()
                .expect("utf-8 definition path"),
            processed_events().to_str().expect("utf-8 event path"),
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
