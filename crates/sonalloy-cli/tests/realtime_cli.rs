mod support;

use assert_cmd::Command;
use support::fixture_path;
use tempfile::tempdir;

#[test]
fn play_rejects_zero_buffer_before_accessing_devices() {
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "play",
            fixture_path("instruments/basic-poly-synth.json")
                .to_str()
                .expect("definition path"),
            "--buffer-size",
            "0",
        ])
        .assert()
        .code(2)
        .stderr(predicates::str::contains(
            "buffer size must be greater than zero",
        ));
}

#[test]
fn audition_pattern_validates_before_accessing_audio_devices() {
    let directory = tempdir().expect("temporary directory");
    let pattern = directory.path().join("audition.json");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["pattern", "init", pattern.to_str().expect("pattern path")])
        .assert()
        .success();

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "audition",
            "pattern",
            fixture_path("instruments/basic-poly-synth.json")
                .to_str()
                .expect("definition path"),
            pattern.to_str().expect("pattern path"),
            "--buffer-size",
            "0",
        ])
        .assert()
        .code(2)
        .stderr(predicates::str::contains(
            "buffer size must be greater than zero",
        ));
}
