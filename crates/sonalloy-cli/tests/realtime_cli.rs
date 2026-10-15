use assert_cmd::Command;
use tempfile::tempdir;

#[test]
fn audition_score_validates_before_accessing_audio_devices() {
    let directory = tempdir().expect("temporary directory");
    let pattern = directory.path().join("audition.score.yaml");
    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args(["score", "init", pattern.to_str().expect("pattern path")])
        .assert()
        .success();

    Command::cargo_bin("sonalloy")
        .expect("binary")
        .args([
            "audition",
            "score",
            pattern.to_str().expect("pattern path"),
            "--buffer-size",
            "0",
        ])
        .assert()
        .code(2)
        .stderr(predicates::str::contains("expected a positive integer"));
}
