use std::fs;
use std::path::{Path, PathBuf};

use sonalloy_core::InstrumentDefinition;

fn preset_paths() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../presets");
    let mut paths = Vec::new();
    for category in fs::read_dir(root)
        .expect("presets directory")
        .map(|entry| entry.expect("preset category entry").path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .is_some_and(|name| name != "assets" && name != "common-scores")
        })
    {
        for preset in fs::read_dir(category).expect("preset category directory") {
            let path = preset.expect("preset directory entry").path();
            if path.is_dir() && path.join("definition.json").is_file() {
                paths.push(path);
            }
        }
    }
    paths.sort_unstable();
    paths
}

const BUILTIN_TAG_VOCABULARY: &[&str] = &[
    "Bright",
    "Dark",
    "Warm",
    "Mellow",
    "Clean",
    "Metallic",
    "Glassy",
    "Noisy",
    "Hollow",
    "Airy",
    "Sub",
    "Resonant",
    "Smooth",
    "Rough",
    "Dense",
    "Thin",
    "Punchy",
    "Sharp",
    "Soft",
    "Wide",
    "Narrow",
    "Centered",
    "Detuned",
    "Diffuse",
    "Short",
    "Sustained",
    "Plucky",
    "Percussive",
    "Swelling",
    "Evolving",
    "Rhythmic",
    "Stable",
    "Long-Tail",
    "Analog-Style",
    "FM",
    "PM",
    "AM",
    "Ring-Mod",
    "Wavetable",
    "Additive",
    "Granular",
    "Formant",
    "Wavefold",
    "Hard-Sync",
    "Phase-Distortion",
    "Physical",
    "Modal",
    "Sample-Based",
    "Wave-Sequence",
    "Spectral",
];

fn expected_preset_category(path: &Path, preset_id: &str) -> &'static str {
    let category_code = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .expect("preset category directory name is UTF-8");
    let preset_number = preset_id
        .get(..3)
        .expect("preset directory must start with a three-digit number");
    assert!(
        preset_number.bytes().all(|byte| byte.is_ascii_digit()),
        "{preset_id} directory must start with its three-digit ID"
    );
    match category_code {
        "BASS" => "Bass",
        "LEAD" => "Lead",
        "PAD" => "Pad",
        "KEYS" => "Keys",
        "PLUCK" => "Pluck",
        "MALLET" => "Mallet",
        "DRUM" => "Drum",
        "SEQ" => "Sequence",
        "FX" => "FX",
        other => panic!("unexpected built-in preset category: {other}"),
    }
}

#[test]
fn every_builtin_preset_has_valid_library_metadata() {
    let paths = preset_paths();

    for path in paths {
        let preset_id = path
            .file_name()
            .expect("preset directory name")
            .to_str()
            .expect("preset directory name is UTF-8");
        let expected_category = expected_preset_category(&path, preset_id);
        let definition_path = path.join("definition.json");
        let definition: InstrumentDefinition = serde_json::from_str(
            &fs::read_to_string(&definition_path).expect("definition JSON is readable"),
        )
        .unwrap_or_else(|error| panic!("{preset_id} must parse: {error}"));

        assert_eq!(
            definition.metadata.category.as_deref(),
            Some(expected_category),
            "{preset_id} metadata category must match its directory category"
        );

        let diagnostics = definition.validate();
        assert!(
            diagnostics.is_empty(),
            "{preset_id} has definition diagnostics: {diagnostics:?}"
        );
        assert!(
            definition
                .metadata
                .tags
                .iter()
                .all(|tag| { BUILTIN_TAG_VOCABULARY.contains(&tag.as_str()) })
        );
    }
}
