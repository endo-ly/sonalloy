use std::process::ExitCode;
use std::sync::LazyLock;

use clap::{Args, Subcommand};
use serde_json::{Value, json};

static PRESETS: LazyLock<Vec<Value>> = LazyLock::new(|| {
    serde_json::from_str(include_str!(concat!(
        env!("OUT_DIR"),
        "/preset-catalog.json"
    )))
    .expect("build-time preset catalog is valid JSON")
});

#[derive(Debug, Subcommand)]
pub(super) enum PresetCommand {
    /// List built-in presets and search their metadata.
    #[command(
        long_about = "Results use stable category and ID order. Query values match case-insensitively as partial text, and no matches produce an empty successful result. Table output omits full descriptions, while machine-readable output includes complete metadata. The Score reference uses `CATEGORY/NNN-name` and the management ID uses `CATEGORY-NNN`."
    )]
    List(ListArgs),
}

#[derive(Debug, Args)]
pub(super) struct ListArgs {
    /// Filter preset ID, Score reference, name, description, category, or tags.
    #[arg(long, value_name = "TEXT")]
    query: Option<String>,
    /// Emit machine-readable JSON, including full descriptions.
    #[arg(long)]
    json: bool,
}

pub(super) fn run(command: PresetCommand) -> ExitCode {
    match command {
        PresetCommand::List(args) => list(&args),
    }
}

fn list(args: &ListArgs) -> ExitCode {
    let presets = PRESETS
        .iter()
        .filter(|preset| {
            args.query
                .as_deref()
                .is_none_or(|query| matches_query(preset, query))
        })
        .collect::<Vec<_>>();

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": "ok",
                "count": presets.len(),
                "presets": presets,
            }))
            .expect("preset report serializes")
        );
    } else {
        println!("ID | SCORE PRESET | NAME | CATEGORY | TAGS | RANGE");
        for preset in presets {
            let range = &preset["recommended_range"];
            let tags = preset["tags"]
                .as_array()
                .expect("build script validates preset tags")
                .iter()
                .map(|tag| tag.as_str().expect("build script validates tag strings"))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "{} | {} | {} | {} | {} | {}–{}",
                preset["id"].as_str().expect("preset ID"),
                preset["preset"].as_str().expect("Score preset reference"),
                preset["name"].as_str().expect("preset name"),
                preset["category"].as_str().expect("preset category"),
                tags,
                range["min_midi"].as_u64().expect("minimum MIDI note"),
                range["max_midi"].as_u64().expect("maximum MIDI note"),
            );
        }
    }

    ExitCode::SUCCESS
}

fn matches_query(preset: &Value, query: &str) -> bool {
    let query = query.to_lowercase();
    ["id", "preset", "name", "description", "category"]
        .into_iter()
        .filter_map(|field| preset.get(field).and_then(Value::as_str))
        .any(|value| value.to_lowercase().contains(&query))
        || preset["tags"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|tag| tag.to_lowercase().contains(&query))
}
