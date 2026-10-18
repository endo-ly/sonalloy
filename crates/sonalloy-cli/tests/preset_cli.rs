use std::fs;
use std::path::Path;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::tempdir;

fn preset_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../presets")
        .canonicalize()
        .expect("preset directory")
}

fn json_output(working_directory: &Path, arguments: &[&str]) -> Value {
    let output = Command::cargo_bin("sonalloy")
        .expect("CLI binary")
        .args(arguments)
        .current_dir(working_directory)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).expect("JSON output")
}

fn matching_ids(report: &Value) -> Vec<&str> {
    report["presets"]
        .as_array()
        .expect("presets array")
        .iter()
        .map(|preset| preset["id"].as_str().expect("preset ID"))
        .collect()
}

fn source_preset_count(root: &Path) -> usize {
    fs::read_dir(root)
        .expect("preset categories")
        .filter_map(Result::ok)
        .filter(|category| category.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|category| {
            fs::read_dir(category.path())
                .expect("category presets")
                .filter_map(Result::ok)
                .filter(|preset| {
                    preset.file_type().is_ok_and(|kind| kind.is_dir())
                        && preset.path().join("definition.json").is_file()
                })
                .count()
        })
        .sum()
}

fn assert_catalog_matches_sources(presets: &[Value], source_root: &Path) {
    for preset in presets {
        let score_reference = preset["preset"].as_str().expect("Score reference");
        let definition_path = source_root.join(score_reference).join("definition.json");
        let definition: Value =
            serde_json::from_slice(&fs::read(&definition_path).expect("source Definition"))
                .expect("source Definition JSON");
        let metadata = &definition["metadata"];
        let (category, directory) = score_reference
            .split_once('/')
            .expect("category and preset directory");

        assert_eq!(
            preset["id"],
            format!("{category}-{}", &directory[..3]),
            "{score_reference} has an ID derived from its directory"
        );
        for field in [
            "name",
            "description",
            "author",
            "category",
            "tags",
            "recommended_range",
        ] {
            assert_eq!(
                preset.get(field),
                metadata.get(field),
                "{score_reference} metadata field {field} is preserved"
            );
        }
    }
}

fn assert_search_contract(working_directory: &Path) {
    for (query, expected_id) in [
        ("BASS-001", "BASS-001"),
        ("BASS/001-clean-sub-bass", "BASS-001"),
        ("Clean Sub", "BASS-001"),
        ("深く滑らかな最低域", "BASS-001"),
        ("Bass", "BASS-001"),
        ("Sub", "BASS-001"),
    ] {
        let report = json_output(
            working_directory,
            &["preset", "list", "--query", query, "--json"],
        );
        assert!(
            matching_ids(&report).contains(&expected_id),
            "query {query:?} finds {expected_id}"
        );
        assert_eq!(
            report["count"].as_u64(),
            Some(report["presets"].as_array().expect("presets").len() as u64)
        );
    }

    assert_eq!(
        json_output(
            working_directory,
            &["preset", "list", "--query", "CLEAN", "--json"]
        ),
        json_output(
            working_directory,
            &["preset", "list", "--query", "clean", "--json"]
        )
    );
    let empty = json_output(
        working_directory,
        &["preset", "list", "--query", "no-such-preset", "--json"],
    );
    assert_eq!(empty["status"], "ok");
    assert_eq!(empty["count"], 0);
    assert_eq!(empty["presets"], serde_json::json!([]));
}

fn assert_table_output(working_directory: &Path) {
    let output = Command::cargo_bin("sonalloy")
        .expect("CLI binary")
        .args(["preset", "list"])
        .current_dir(working_directory)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let table = String::from_utf8(output).expect("table is UTF-8");
    assert!(table.contains("ID | SCORE PRESET | NAME | CATEGORY | TAGS | RANGE"));
    assert!(table.contains("BASS-001 | BASS/001-clean-sub-bass | Clean Sub Bass"));
}

#[test]
fn preset_catalog_is_embedded_exact_and_searchable_from_any_directory() {
    let outside_repository = tempdir().expect("temporary working directory");
    let all = json_output(outside_repository.path(), &["preset", "list", "--json"]);
    let presets = all["presets"].as_array().expect("preset array");
    let source_root = preset_root();
    let expected_count = source_preset_count(&source_root);

    assert_eq!(all["status"], "ok");
    assert_eq!(all["count"].as_u64(), Some(expected_count as u64));
    assert_eq!(presets.len(), expected_count);
    assert!(
        presets
            .windows(2)
            .all(|pair| pair[0]["id"].as_str() <= pair[1]["id"].as_str())
    );
    assert_catalog_matches_sources(presets, &source_root);
    assert_search_contract(outside_repository.path());
    assert_table_output(outside_repository.path());
}
