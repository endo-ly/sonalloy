use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

const CATEGORIES: [(&str, &str); 9] = [
    ("BASS", "Bass"),
    ("DRUM", "Drum"),
    ("FX", "FX"),
    ("KEYS", "Keys"),
    ("LEAD", "Lead"),
    ("MALLET", "Mallet"),
    ("PAD", "Pad"),
    ("PLUCK", "Pluck"),
    ("SEQ", "Sequence"),
];

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let presets_root = manifest_dir.join("../../presets");
    println!("cargo:rerun-if-changed={}", presets_root.display());

    let catalog = collect_catalog(&presets_root).unwrap_or_else(|error| panic!("{error}"));
    let output_path = PathBuf::from(env::var_os("OUT_DIR").expect("output directory"))
        .join("preset-catalog.json");
    fs::write(
        &output_path,
        serde_json::to_vec(&catalog).expect("preset catalog serializes"),
    )
    .unwrap_or_else(|error| panic!("could not write {}: {error}", output_path.display()));
}

fn collect_catalog(presets_root: &Path) -> Result<Vec<Value>, String> {
    if !presets_root.is_dir() {
        return Err(format!(
            "preset directory is missing: {}",
            presets_root.display()
        ));
    }

    let mut category_directories = fs::read_dir(presets_root)
        .map_err(|error| format!("could not read {}: {error}", presets_root.display()))?
        .map(|entry| {
            entry.map_err(|error| {
                format!(
                    "could not read an entry in {}: {error}",
                    presets_root.display()
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    category_directories.sort_by_key(fs::DirEntry::file_name);

    let mut catalog = Vec::new();
    let mut ids = HashSet::new();
    for entry in category_directories {
        if !entry
            .file_type()
            .map_err(|error| format!("could not inspect {}: {error}", entry.path().display()))?
            .is_dir()
        {
            continue;
        }

        let category_code = entry.file_name().to_string_lossy().into_owned();
        let Some((_, category_label)) = CATEGORIES.iter().find(|(code, _)| *code == category_code)
        else {
            if matches!(category_code.as_str(), "assets" | "common-scores") {
                continue;
            }
            return Err(format!(
                "unexpected preset category directory {} (expected one of: {})",
                entry.path().display(),
                CATEGORIES
                    .iter()
                    .map(|(code, _)| *code)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        };
        catalog.extend(collect_category(
            &entry.path(),
            &category_code,
            category_label,
            &mut ids,
        )?);
    }

    catalog.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    Ok(catalog)
}

fn collect_category(
    category_path: &Path,
    category_code: &str,
    category_label: &str,
    ids: &mut HashSet<String>,
) -> Result<Vec<Value>, String> {
    println!("cargo:rerun-if-changed={}", category_path.display());
    let mut directories = fs::read_dir(category_path)
        .map_err(|error| format!("could not read {}: {error}", category_path.display()))?
        .map(|entry| {
            entry.map_err(|error| {
                format!(
                    "could not read an entry in {}: {error}",
                    category_path.display()
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    directories.sort_by_key(fs::DirEntry::file_name);

    let mut presets = Vec::new();
    for entry in directories {
        if !entry
            .file_type()
            .map_err(|error| format!("could not inspect {}: {error}", entry.path().display()))?
            .is_dir()
        {
            continue;
        }
        let directory_name = entry.file_name().to_string_lossy().into_owned();
        if !is_preset_directory(&directory_name) {
            return Err(format!(
                "preset directory {} must match NNN-kebab-case-name",
                entry.path().display()
            ));
        }

        let id = format!("{category_code}-{}", &directory_name[..3]);
        if !ids.insert(id.clone()) {
            return Err(format!("duplicate built-in preset ID '{id}'"));
        }
        presets.push(read_preset(
            &entry.path().join("definition.json"),
            id,
            format!("{category_code}/{directory_name}"),
            category_label,
        )?);
    }
    Ok(presets)
}

fn read_preset(
    definition_path: &Path,
    id: String,
    preset_path: String,
    category_label: &str,
) -> Result<Value, String> {
    println!("cargo:rerun-if-changed={}", definition_path.display());
    let source = fs::read_to_string(definition_path)
        .map_err(|error| format!("could not read {}: {error}", definition_path.display()))?;
    let definition: Value = serde_json::from_str(&source)
        .map_err(|error| format!("could not parse {}: {error}", definition_path.display()))?;
    let metadata = definition
        .get("metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            format!(
                "{} must contain a metadata object",
                definition_path.display()
            )
        })?;

    required_text(metadata, "name", definition_path)?;
    let category = required_text(metadata, "category", definition_path)?;
    if category != category_label {
        return Err(format!(
            "{} metadata.category must be '{category_label}'",
            definition_path.display()
        ));
    }
    let tags = metadata
        .get("tags")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            format!(
                "{} metadata.tags must be an array",
                definition_path.display()
            )
        })?;
    if tags.iter().any(|tag| !tag.is_string()) {
        return Err(format!(
            "{} metadata.tags entries must be strings",
            definition_path.display()
        ));
    }
    let recommended_range = metadata
        .get("recommended_range")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            format!(
                "{} metadata.recommended_range must be an object",
                definition_path.display()
            )
        })?;
    let min_midi = midi_note(recommended_range, "min_midi", definition_path)?;
    let max_midi = midi_note(recommended_range, "max_midi", definition_path)?;
    if min_midi > max_midi {
        return Err(format!(
            "{} metadata.recommended_range.min_midi must not exceed max_midi",
            definition_path.display()
        ));
    }
    validate_optional_text(metadata, "author", definition_path)?;
    validate_optional_text(metadata, "description", definition_path)?;

    let mut item = Map::new();
    item.insert("id".to_owned(), Value::String(id));
    item.insert("preset".to_owned(), Value::String(preset_path));
    for field in [
        "name",
        "description",
        "author",
        "category",
        "tags",
        "recommended_range",
    ] {
        if let Some(value) = metadata.get(field) {
            item.insert(field.to_owned(), value.clone());
        }
    }
    Ok(Value::Object(item))
}

fn is_preset_directory(name: &str) -> bool {
    let Some((number, slug)) = name.split_once('-') else {
        return false;
    };
    number.len() == 3
        && number.bytes().all(|byte| byte.is_ascii_digit())
        && !slug.is_empty()
        && slug.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

fn required_text<'a>(
    metadata: &'a Map<String, Value>,
    field: &str,
    definition_path: &Path,
) -> Result<&'a str, String> {
    let value = metadata.get(field).and_then(Value::as_str).ok_or_else(|| {
        format!(
            "{} metadata.{field} must be a string",
            definition_path.display()
        )
    })?;
    if value.trim().is_empty() {
        return Err(format!(
            "{} metadata.{field} must not be empty",
            definition_path.display()
        ));
    }
    Ok(value)
}

fn validate_optional_text(
    metadata: &Map<String, Value>,
    field: &str,
    definition_path: &Path,
) -> Result<(), String> {
    if let Some(value) = metadata.get(field)
        && !value.is_null()
        && !value.is_string()
    {
        return Err(format!(
            "{} metadata.{field} must be a string or null",
            definition_path.display()
        ));
    }
    Ok(())
}

fn midi_note(
    range: &Map<String, Value>,
    field: &str,
    definition_path: &Path,
) -> Result<u64, String> {
    range
        .get(field)
        .and_then(Value::as_u64)
        .filter(|note| *note <= 127)
        .ok_or_else(|| {
            format!(
                "{} metadata.recommended_range.{field} must be an integer from 0 through 127",
                definition_path.display()
            )
        })
}
