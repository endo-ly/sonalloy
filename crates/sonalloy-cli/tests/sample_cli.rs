use std::path::Path;

use assert_cmd::Command;
use serde_json::{Value, json};
use tempfile::tempdir;

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn cli(args: &[&str], code: i32) -> Value {
    let output = Command::cargo_bin("sonalloy")
        .unwrap()
        .args(args)
        .assert()
        .code(code)
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output)))
}

fn wav(path: &Path, channels: u16, frames: u32, rate: u32, silence: bool) {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for frame in 0..frames {
        #[allow(clippy::cast_precision_loss)]
        let sample = if silence {
            0.0
        } else {
            0.5 * (std::f32::consts::TAU * 440.0 * frame as f32 / rate as f32).sin()
        };
        for channel in 0..channels {
            writer
                .write_sample(if channel % 2 == 0 { sample } else { -sample })
                .unwrap();
        }
    }
    writer.finalize().unwrap();
}

fn index(root: &Path, output: &Path, analyze: bool) -> Value {
    let mut args = vec![
        "sample",
        "index",
        text(root),
        "--output",
        text(output),
        "--json",
    ];
    if analyze {
        args.push("--analyze");
    }
    cli(&args, 0)
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
fn write(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

#[test]
fn analyze_keeps_source_layout_and_collects_failures_without_losing_valid_inputs() {
    let dir = tempdir().unwrap();
    let mono = dir.path().join("mono.wav");
    let stereo = dir.path().join("stereo.WAV");
    let silence = dir.path().join("silence.wav");
    let short = dir.path().join("short.wav");
    let multi = dir.path().join("multi.wav");
    let broken = dir.path().join("broken.wav");
    wav(&mono, 1, 8192, 44100, false);
    wav(&stereo, 2, 8192, 44100, false);
    wav(&silence, 1, 64, 22050, true);
    wav(&short, 1, 1, 48000, false);
    wav(&multi, 4, 64, 96000, false);
    std::fs::write(&broken, b"invalid WAV").unwrap();

    let result = cli(
        &[
            "sample",
            "analyze",
            text(dir.path()),
            text(&mono),
            "--reference-frequency",
            "440",
            "--json",
        ],
        1,
    );

    assert_eq!(result["status"], "partial");
    assert_eq!(result["total"], 6);
    assert_eq!(result["succeeded"], 5);
    let files = result["files"].as_array().unwrap();
    let find = |name: &str| {
        files
            .iter()
            .find(|v| v["path"].as_str().unwrap().ends_with(name))
            .unwrap()
    };
    let mono = &find("mono.wav")["analysis"];
    let stereo = &find("stereo.WAV")["analysis"];
    assert_eq!(mono["sample_rate"], 44100);
    assert_eq!(mono["channels"], 1);
    assert_eq!(mono["frames"], 8192);
    assert!((stereo["spectrum"]["spectral_centroid_hz"].as_f64().unwrap() - 440.0).abs() < 1.0);
    for (mono, stereo) in mono["spectrum"]["bands"]
        .as_array()
        .unwrap()
        .iter()
        .zip(stereo["spectrum"]["bands"].as_array().unwrap())
    {
        assert!(
            (mono["mean_square"].as_f64().unwrap() - stereo["mean_square"].as_f64().unwrap()).abs()
                < 1e-7
        );
    }
    assert_eq!(find("multi.wav")["analysis"]["channels"], 4);
    assert!(find("silence.wav")["analysis"]["level"]["peak_dbfs"].is_null());
    assert!(find("silence.wav")["analysis"]["spectrum"]["spectral_centroid_hz"].is_null());
    assert_eq!(find("short.wav")["analysis"]["frames"], 1);
    assert_eq!(
        find("broken.wav")["diagnostics"][0]["code"],
        "ASSET_DECODE_FAILED"
    );
    let glob = dir.path().join("*ereo.*");
    assert_eq!(
        cli(&["sample", "analyze", text(&glob), "--json"], 0)["succeeded"],
        1
    );
}

#[test]
fn nonfinite_audio_is_diagnosed_and_index_keeps_its_header_metadata() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("nan.wav");
    wav(&source, 1, 16, 48000, true);
    let mut bytes = std::fs::read(&source).unwrap();
    let end = bytes.len();
    bytes[end - 4..].copy_from_slice(&f32::NAN.to_le_bytes());
    std::fs::write(&source, bytes).unwrap();
    let output = dir.path().join("index.json");

    assert_eq!(
        cli(&["sample", "analyze", text(&source), "--json"], 1)["failed"],
        1
    );
    let report = cli(
        &[
            "sample",
            "index",
            text(dir.path()),
            "--output",
            text(&output),
            "--analyze",
            "--json",
        ],
        1,
    );

    assert_eq!(report["failed"], 1);
    let stored = read(&output);
    assert_eq!(stored["files"][0]["audio"]["sample_rate"], 48000);
    assert!(stored["files"][0]["analysis"].is_null());
    let search = cli(&["sample", "search", text(&output), "--json"], 0);
    assert_eq!(search["results"][0]["analysis_status"], "failed");
    assert_eq!(
        cli(
            &[
                "sample",
                "search",
                text(&output),
                "--min-centroid-hz",
                "1",
                "--json"
            ],
            0
        )["unevaluated"],
        1
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn index_updates_metadata_and_analysis_and_protects_existing_data() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("日本語 samples");
    std::fs::create_dir(&root).unwrap();
    let nested = root.join("nested");
    std::fs::create_dir(&nested).unwrap();
    let kick = nested.join("Kick 音.WAV");
    let snare = root.join("snare.wav");
    wav(&kick, 1, 256, 48000, false);
    wav(&snare, 2, 128, 44100, false);
    let output = dir.path().join("index.json");

    let first = index(&root, &output, false);
    assert_eq!(first["added"], 2);
    assert_eq!(read(&output)["files"][0]["audio"]["frames"], 256);
    let names = cli(
        &[
            "sample",
            "search",
            text(&output),
            "--query",
            "KICK",
            "--json",
        ],
        0,
    );
    assert_eq!(names["total_matches"], 1);
    assert_eq!(names["results"][0]["analysis_status"], "unanalyzed");
    let incomplete = cli(
        &[
            "sample",
            "search",
            text(&output),
            "--min-peak-dbfs",
            "-30",
            "--json",
        ],
        0,
    );
    assert_eq!(incomplete["unevaluated"], 2);
    assert_eq!(incomplete["complete"], false);
    assert_eq!(index(&root, &output, true)["analyzed"], 2);
    let cached = read(&output);
    let again = index(&root, &output, true);
    assert_eq!(again["unchanged"], 2);
    assert_eq!(again["analyzed"], 0);
    assert_eq!(read(&output), cached);
    let filtered = cli(
        &[
            "sample",
            "search",
            text(&output),
            "--query",
            "kick",
            "--channels",
            "1",
            "--sample-rate",
            "48000",
            "--max-duration",
            "1",
            "--min-rms-dbfs",
            "-30",
            "--min-centroid-hz",
            "100",
            "--json",
        ],
        0,
    );
    assert_eq!(filtered["total_matches"], 1);
    assert_eq!(
        Path::new(filtered["results"][0]["path"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        kick.canonicalize().unwrap()
    );

    wav(&kick, 1, 512, 48000, false);
    std::fs::remove_file(&snare).unwrap();
    wav(&root.join("hat.wav"), 1, 128, 48000, false);
    let changed = index(&root, &output, false);
    assert_eq!(changed["updated"], 1);
    assert_eq!(changed["added"], 1);
    assert_eq!(changed["removed"], 1);
    assert!(
        read(&output)["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["analysis"].is_null())
    );
    assert_eq!(index(&root, &output, true)["analyzed"], 2);
    let forced = cli(
        &[
            "sample",
            "index",
            text(&root),
            "--output",
            text(&output),
            "--force",
            "--analyze",
            "--json",
        ],
        0,
    );
    assert_eq!(forced["analyzed"], 2);
    let saved = std::fs::read(&output).unwrap();
    cli(
        &[
            "sample",
            "index",
            text(dir.path()),
            "--output",
            text(&output),
            "--json",
        ],
        2,
    );
    assert_eq!(std::fs::read(&output).unwrap(), saved);
    std::fs::write(&output, b"{invalid").unwrap();
    cli(
        &[
            "sample",
            "index",
            text(&root),
            "--output",
            text(&output),
            "--json",
        ],
        2,
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"{invalid");
}

#[test]
fn search_merges_indexes_before_paging_and_rejects_ambiguous_analysis() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("samples");
    std::fs::create_dir(&root).unwrap();
    for name in ["a.wav", "b.wav", "c.wav"] {
        wav(&root.join(name), 1, 128, 48000, false);
    }
    let old = dir.path().join("old.json");
    let new = dir.path().join("new.json");
    index(&root, &old, false);
    index(&root, &new, true);

    let result = cli(
        &[
            "sample",
            "search",
            text(&old),
            text(&new),
            "--offset",
            "1",
            "--limit",
            "1",
            "--json",
        ],
        0,
    );

    assert_eq!(result["total_files"], 3);
    assert_eq!(result["total_matches"], 3);
    assert_eq!(result["returned"], 1);
    assert_eq!(result["results"][0]["relative_path"], "b.wav");
    assert_eq!(result["results"][0]["analysis_status"], "analyzed");
    let reversed = cli(
        &[
            "sample",
            "search",
            text(&new),
            text(&old),
            "--offset",
            "1",
            "--limit",
            "1",
            "--json",
        ],
        0,
    );
    assert_eq!(result, reversed);
    let mut conflicting = read(&new);
    conflicting["files"][0]["analysis"]["level"]["peak"] = json!(0.25);
    write(&old, &conflicting);
    let error = cli(&["sample", "search", text(&old), text(&new), "--json"], 2);
    assert!(
        error["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("conflicting")
    );
    let unrelated = cli(
        &[
            "sample",
            "search",
            text(&old),
            text(&new),
            "--query",
            "b.wav",
            "--json",
        ],
        0,
    );
    assert_eq!(unrelated["total_matches"], 1);
    let excluded = cli(
        &[
            "sample",
            "search",
            text(&old),
            text(&new),
            "--min-peak-dbfs",
            "200",
            "--json",
        ],
        0,
    );
    assert_eq!(excluded["total_matches"], 0);
    assert_eq!(excluded["unevaluated"], 0);
    let mut stale = read(&new);
    stale["files"][0]["analysis_revision"] = json!(0);
    write(&new, &stale);
    let unevaluated = cli(
        &[
            "sample",
            "search",
            text(&new),
            "--min-peak-dbfs",
            "-30",
            "--json",
        ],
        0,
    );
    assert_eq!(unevaluated["unevaluated"], 1);
    assert_eq!(index(&root, &new, true)["analyzed"], 1);
}

#[test]
fn thousand_file_index_is_stable_and_does_not_require_analysis() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("samples");
    std::fs::create_dir(&root).unwrap();
    let first = root.join("0000.wav");
    wav(&first, 1, 16, 48000, true);
    for n in 1..1000 {
        std::fs::copy(&first, root.join(format!("{n:04}.wav"))).unwrap();
    }
    let output = dir.path().join("index.json");

    assert_eq!(index(&root, &output, false)["added"], 1000);
    let report = index(&root, &output, false);

    assert_eq!(report["unchanged"], 1000);
    assert_eq!(report["analyzed"], 0);
    let search = cli(
        &[
            "sample",
            "search",
            text(&output),
            "--offset",
            "999",
            "--json",
        ],
        0,
    );
    assert_eq!(search["total_matches"], 1000);
    assert_eq!(search["returned"], 1);
    assert_eq!(search["results"][0]["relative_path"], "0999.wav");
}

#[test]
#[allow(clippy::too_many_lines)]
fn sample_instrument_init_validates_compiles_renders_and_protects_destination() {
    let dir = tempdir().unwrap();
    let sources = dir.path().join("素材");
    let definitions = dir.path().join("definitions");
    std::fs::create_dir(&sources).unwrap();
    std::fs::create_dir(&definitions).unwrap();
    let source = sources.join("sample 音.wav");
    wav(&source, 1, 48000, 48000, false);
    let definition = definitions.join("instrument.json");

    cli(
        &[
            "instrument",
            "init",
            text(&definition),
            "--from-sample",
            text(&source),
            "--root-note",
            "64",
            "--key-min",
            "48",
            "--key-max",
            "72",
            "--no-pitch-tracking",
            "--note-off-behavior",
            "ignore",
            "--name",
            "My Sample",
            "--json",
        ],
        0,
    );

    let stored = read(&definition);
    let sample = &stored["layers"][0]["generator"]["sample"];
    assert_eq!(stored["metadata"]["name"], "My Sample");
    assert_eq!(sample["zones"][0]["root_note"], 64);
    assert_eq!(sample["zones"][0]["key_min"], 48);
    assert_eq!(sample["zones"][0]["key_max"], 72);
    assert_eq!(sample["zones"][0]["pitch_tracking"], false);
    assert_eq!(sample["note_off_behavior"], "ignore");
    let reference = Path::new(sample["zones"][0]["asset"]["path"].as_str().unwrap());
    assert!(reference.is_relative());
    assert_eq!(
        definitions.join(reference).canonicalize().unwrap(),
        source.canonicalize().unwrap()
    );
    cli(&["instrument", "validate", text(&definition), "--json"], 0);
    cli(&["instrument", "inspect", text(&definition), "--json"], 0);
    let render = dir.path().join("preview.wav");
    cli(
        &[
            "render",
            "note",
            text(&definition),
            "--output",
            text(&render),
            "--gate",
            "0.01",
            "--tail",
            "0.2",
            "--analyze",
            "--json",
        ],
        0,
    );
    let rendered: Vec<_> = hound::WavReader::open(&render)
        .unwrap()
        .samples::<f32>()
        .map(Result::unwrap)
        .collect();
    assert!(rendered.iter().all(|v| v.is_finite()));
    assert!(rendered[4800..].iter().any(|v| v.abs() > 0.1));
    let saved = std::fs::read(&definition).unwrap();
    cli(
        &[
            "instrument",
            "init",
            text(&definition),
            "--from-sample",
            text(&source),
            "--json",
        ],
        2,
    );
    assert_eq!(std::fs::read(&definition).unwrap(), saved);
    let invalid = definitions.join("invalid.json");
    cli(
        &[
            "instrument",
            "init",
            text(&invalid),
            "--from-sample",
            text(&source),
            "--key-min",
            "80",
            "--key-max",
            "40",
            "--json",
        ],
        2,
    );
    assert!(!invalid.exists());
    let release = definitions.join("release.json");
    cli(
        &[
            "instrument",
            "init",
            text(&release),
            "--from-sample",
            text(&source),
            "--json",
        ],
        0,
    );
    cli(
        &[
            "render",
            "note",
            text(&release),
            "--output",
            text(&render),
            "--gate",
            "0.01",
            "--tail",
            "0.2",
            "--json",
        ],
        0,
    );
    assert!(
        hound::WavReader::open(&render)
            .unwrap()
            .samples::<f32>()
            .skip(10000)
            .all(|v| v.unwrap().abs() < 1e-6)
    );
}
#[cfg(unix)]
#[test]
fn traversal_skips_directory_links() {
    use std::os::unix::fs::symlink;
    let dir = tempdir().unwrap();
    let root = dir.path().join("samples");
    std::fs::create_dir(&root).unwrap();
    wav(&root.join("sample.wav"), 1, 16, 48000, false);
    symlink(&root, root.join("loop")).unwrap();
    let output = dir.path().join("index.json");

    let report = index(&root, &output, false);

    assert_eq!(report["total_files"], 1);
}

#[cfg(target_os = "linux")]
#[test]
fn traversal_preserves_index_on_unrepresentable_paths() {
    use std::os::unix::ffi::OsStringExt;
    let dir = tempdir().unwrap();
    let root = dir.path().join("samples");
    std::fs::create_dir(&root).unwrap();
    wav(&root.join("sample.wav"), 1, 16, 48000, false);
    let output = dir.path().join("index.json");
    index(&root, &output, false);
    let saved = std::fs::read(&output).unwrap();
    let invalid_name = std::ffi::OsString::from_vec(b"invalid-\xff.wav".to_vec());
    wav(&root.join(invalid_name), 1, 16, 48000, false);

    let result = cli(
        &[
            "sample",
            "index",
            text(&root),
            "--output",
            text(&output),
            "--json",
        ],
        2,
    );

    assert!(
        result["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("UTF-8")
    );
    assert_eq!(std::fs::read(&output).unwrap(), saved);
}

#[test]
fn failed_analysis_retries_even_when_file_fingerprint_is_unchanged() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("sample.wav");
    wav(&source, 1, 16, 48000, false);
    let original = std::fs::read(&source).unwrap();
    let mut invalid = original.clone();
    let end = invalid.len();
    invalid[end - 4..].copy_from_slice(&f32::INFINITY.to_le_bytes());
    std::fs::write(&source, invalid).unwrap();
    let modified = std::fs::metadata(&source).unwrap().modified().unwrap();
    let output = dir.path().join("index.json");
    cli(
        &[
            "sample",
            "index",
            text(dir.path()),
            "--output",
            text(&output),
            "--analyze",
            "--json",
        ],
        1,
    );
    std::fs::write(&source, original).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&source)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();

    let report = index(dir.path(), &output, true);

    assert_eq!(report["unchanged"], 1);
    assert_eq!(report["analyzed"], 1);
    assert_eq!(report["failed"], 0);
    assert_eq!(read(&output)["files"][0]["diagnostics"], json!([]));
}
#[cfg(unix)]
#[test]
fn unreadable_subdirectory_never_removes_unconfirmed_index_records() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let root = dir.path().join("samples");
    let nested = root.join("private");
    std::fs::create_dir_all(&nested).unwrap();
    wav(&nested.join("sample.wav"), 1, 16, 48000, false);
    let output = dir.path().join("index.json");
    index(&root, &output, true);
    let saved = std::fs::read(&output).unwrap();
    std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o000)).unwrap();
    // A privileged process can traverse mode-000 directories.
    if std::fs::read_dir(&nested).is_ok() {
        std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }

    let result = cli(
        &[
            "sample",
            "index",
            text(&root),
            "--output",
            text(&output),
            "--json",
        ],
        2,
    );
    std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o700)).unwrap();

    assert_eq!(result["status"], "error");
    assert_eq!(std::fs::read(&output).unwrap(), saved);
}

#[test]
fn positive_reference_frequencies_remain_bounded_and_invalid_ranges_are_diagnosed() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("sample.wav");
    wav(&source, 1, 128, 48000, false);
    let output = dir.path().join("index.json");
    index(dir.path(), &output, false);

    let analysis = cli(
        &[
            "sample",
            "analyze",
            text(&source),
            "--reference-frequency",
            "1e-30",
            "--json",
        ],
        0,
    );
    let invalid_range = cli(
        &[
            "sample",
            "search",
            text(&output),
            "--min-duration",
            "2",
            "--max-duration",
            "1",
            "--json",
        ],
        2,
    );

    assert!(
        analysis["files"][0]["analysis"]["spectrum"]["harmonic_energy_ratio"]
            .as_f64()
            .unwrap()
            .is_finite()
    );
    assert!(
        invalid_range["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("--min-duration")
    );
    Command::cargo_bin("sonalloy")
        .unwrap()
        .args([
            "instrument",
            "init",
            text(&dir.path().join("invalid.json")),
            "--from-sample",
            text(&source),
            "--root-note",
            "128",
        ])
        .assert()
        .code(2)
        .stderr(predicates::str::contains("--root-note"));
}
#[test]
fn valid_analysis_supersedes_conflicts_between_obsolete_caches() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("sample.wav");
    wav(&source, 1, 128, 48000, false);
    let current = dir.path().join("c-current.json");
    index(dir.path(), &current, true);
    let first = dir.path().join("a-stale.json");
    let second = dir.path().join("b-stale.json");
    let mut stale = read(&current);
    stale["files"][0]["analysis_revision"] = json!(0);
    write(&first, &stale);
    stale["files"][0]["analysis"]["level"]["peak"] = json!(0.25);
    write(&second, &stale);

    let report = cli(
        &[
            "sample",
            "search",
            text(&first),
            text(&second),
            text(&current),
            "--json",
        ],
        0,
    );

    assert_eq!(report["total_matches"], 1);
    assert_eq!(report["results"][0]["analysis_status"], "analyzed");
    assert_eq!(
        Path::new(report["results"][0]["source_index"].as_str().unwrap()),
        current.canonicalize().unwrap()
    );
}
#[test]
#[allow(clippy::too_many_lines)]
fn search_distinguishes_unavailable_analysis_from_measured_null_and_combines_conditions() {
    let dir = tempdir().unwrap();
    for name in ["a-unanalyzed.wav", "e-stale.wav", "f-tone.wav"] {
        wav(&dir.path().join(name), 1, 256, 48000, false);
    }
    wav(&dir.path().join("b-silence.wav"), 1, 256, 48000, true);
    let short = dir.path().join("c-short.wav");
    wav(&short, 1, 1, 48000, true);
    let mut bytes = std::fs::read(&short).unwrap();
    let end = bytes.len();
    bytes[end - 4..].copy_from_slice(&0.25_f32.to_le_bytes());
    std::fs::write(&short, bytes).unwrap();
    std::fs::write(dir.path().join("d-failed.wav"), b"invalid WAV").unwrap();
    let output = dir.path().join("index.json");
    cli(
        &[
            "sample",
            "index",
            text(dir.path()),
            "--output",
            text(&output),
            "--analyze",
            "--json",
        ],
        1,
    );
    let mut stored = read(&output);
    let records = stored["files"].as_array_mut().unwrap();
    records[0]["analysis"] = Value::Null;
    records[0]["analysis_revision"] = Value::Null;
    // Missing file metadata can coexist with a measured, undefined acoustic value.
    records[1]["audio"]["duration_seconds"] = Value::Null;
    records[4]["analysis_revision"] = json!(0);
    let boundary = |value: &Value| {
        f64::from(serde_json::from_value::<f32>(value.clone()).unwrap()).to_string()
    };
    let short_peak = boundary(&records[2]["analysis"]["level"]["peak_dbfs"]);
    let short_rms = boundary(&records[2]["analysis"]["level"]["rms_dbfs"]);
    let tone_centroid = boundary(&records[5]["analysis"]["spectrum"]["spectral_centroid_hz"]);
    write(&output, &stored);
    let cases: &[(&[&str], u64, u64)] = &[
        (&[], 6, 0),
        (&["--min-peak-dbfs", "-30"], 2, 3),
        (&["--min-rms-dbfs", "-30"], 2, 3),
        (&["--min-centroid-hz", "0"], 1, 3),
        (&["--query", "silence", "--min-peak-dbfs", "-30"], 0, 0),
        (&["--query", "silence", "--max-peak-dbfs", "0"], 0, 0),
        (&["--query", "short", "--max-centroid-hz", "1000"], 0, 0),
        (
            &[
                "--query",
                "silence",
                "--min-duration",
                "1",
                "--min-peak-dbfs",
                "-30",
            ],
            0,
            0,
        ),
        (
            &[
                "--query",
                "unanalyzed",
                "--max-duration",
                "0",
                "--min-peak-dbfs",
                "-30",
            ],
            0,
            0,
        ),
        (&["--query", "failed", "--min-peak-dbfs", "-30"], 0, 1),
        (&["--query", "stale", "--min-rms-dbfs", "-30"], 0, 1),
        (
            &[
                "--query",
                "short",
                "--min-peak-dbfs",
                &short_peak,
                "--max-peak-dbfs",
                &short_peak,
                "--min-rms-dbfs",
                &short_rms,
                "--max-rms-dbfs",
                &short_rms,
            ],
            1,
            0,
        ),
        (
            &[
                "--query",
                "tone",
                "--min-centroid-hz",
                &tone_centroid,
                "--max-centroid-hz",
                &tone_centroid,
            ],
            1,
            0,
        ),
    ];

    for (filters, matches, unevaluated) in cases {
        let mut args = vec!["sample", "search", text(&output), "--json"];
        args.extend_from_slice(filters);
        let report = cli(&args, 0);

        assert_eq!(report["total_matches"], *matches, "{filters:?}");
        assert_eq!(report["unevaluated"], *unevaluated, "{filters:?}");
        assert_eq!(report["evaluated"], 6 - unevaluated, "{filters:?}");
        assert_eq!(report["complete"], *unevaluated == 0, "{filters:?}");
    }
    let page = cli(
        &[
            "sample",
            "search",
            text(&output),
            "--min-peak-dbfs",
            "-30",
            "--offset",
            "1",
            "--limit",
            "1",
            "--json",
        ],
        0,
    );
    assert_eq!(page["total_matches"], 2);
    assert_eq!(page["returned"], 1);
    assert_eq!(page["results"][0]["relative_path"], "f-tone.wav");
    assert_eq!(page["unevaluated"], 3);
}
#[test]
fn overlapping_roots_keep_all_searchable_paths_and_return_each_wav_once() {
    let dir = tempdir().unwrap();
    let parent = dir.path().join("samples");
    let drums = parent.join("drums");
    std::fs::create_dir_all(&drums).unwrap();
    let kick = drums.join("kick.wav");
    wav(&kick, 1, 128, 48000, false);
    let child_index = dir.path().join("a-child.json");
    let parent_index = dir.path().join("z-parent.json");
    index(&drums, &child_index, true);
    index(&parent, &parent_index, false);

    for sources in [[&child_index, &parent_index], [&parent_index, &child_index]] {
        for query in ["drums", "kick", "drums/kick"] {
            let query = query.replace('/', std::path::MAIN_SEPARATOR_STR);
            let report = cli(
                &[
                    "sample",
                    "search",
                    text(sources[0]),
                    text(sources[1]),
                    "--query",
                    &query,
                    "--min-peak-dbfs",
                    "-30",
                    "--json",
                ],
                0,
            );

            assert_eq!(report["total_files"], 1);
            assert_eq!(report["total_matches"], 1, "{query}");
            assert_eq!(report["returned"], 1);
            assert_eq!(report["results"][0]["analysis_status"], "analyzed");
            assert_eq!(
                Path::new(report["results"][0]["path"].as_str().unwrap()),
                kick.canonicalize().unwrap()
            );
        }
    }
}

#[test]
fn index_reports_an_absolute_output_path_for_relative_destinations() {
    let dir = tempdir().unwrap();
    let samples = dir.path().join("samples");
    std::fs::create_dir(&samples).unwrap();
    wav(&samples.join("sample.wav"), 1, 16, 48000, true);

    let output = Command::cargo_bin("sonalloy")
        .unwrap()
        .current_dir(dir.path())
        .args([
            "sample",
            "index",
            "samples",
            "--output",
            "samples.index.json",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();

    let index = Path::new(report["index"].as_str().unwrap());
    assert!(index.is_absolute());
    assert_eq!(
        index.canonicalize().unwrap(),
        dir.path()
            .join("samples.index.json")
            .canonicalize()
            .unwrap()
    );
}
