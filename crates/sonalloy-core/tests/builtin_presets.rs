use std::fs;
use std::path::{Path, PathBuf};

use sonalloy_core::{
    CompileContext, InstrumentDefinition, InstrumentPreviewDefinition, ProcessEventKind,
    ProcessSpec, RenderRequest, ScheduledEvent, compile_instrument, render_instrument_with_tempo,
    seconds_to_frames,
};

const SAMPLE_RATE: f64 = 48_000.0;
const BLOCK_SIZE: usize = 257;

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
                    .is_some_and(|name| name != "assets" && name != "common-patterns")
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

#[allow(clippy::cast_precision_loss)]
fn ticks_to_frames(ticks: u64, preview: &InstrumentPreviewDefinition) -> u64 {
    let seconds = ticks as f64 / f64::from(preview.ticks_per_beat) * 60.0 / preview.tempo_bpm;
    seconds_to_frames(seconds, SAMPLE_RATE).expect("preview event frame fits")
}

fn preview_events(preview: &InstrumentPreviewDefinition) -> Vec<ScheduledEvent> {
    let mut events = Vec::with_capacity(preview.notes.len() * 2);
    for (index, note) in preview.notes.iter().enumerate() {
        let note_id = u64::try_from(index + 1).expect("preview note ID fits");
        events.push(ScheduledEvent {
            absolute_frame: ticks_to_frames(note.tick, preview),
            kind: ProcessEventKind::NoteOn {
                note_id,
                note_number: note.note,
                velocity: note.velocity,
            },
        });
        events.push(ScheduledEvent {
            absolute_frame: ticks_to_frames(note.tick + note.duration_ticks, preview),
            kind: ProcessEventKind::NoteOff { note_id },
        });
    }
    events.sort_by_key(|event| (event.absolute_frame, event.kind.priority()));
    events
}

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
fn every_builtin_preset_metadata_validates_and_renders() {
    let paths = preset_paths();
    assert_eq!(paths.len(), 65, "all 65 built-in presets must be present");

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
        let preview = definition
            .metadata
            .preview
            .as_ref()
            .expect("built-in preview");

        let result = compile_instrument(
            &definition,
            &CompileContext {
                definition_base_dir: path.clone(),
                process_spec: ProcessSpec::new(SAMPLE_RATE, BLOCK_SIZE, 0, 2)
                    .expect("valid process spec"),
            },
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity != sonalloy_core::DiagnosticSeverity::Error),
            "{preset_id} has compile diagnostics: {:?}",
            result.diagnostics
        );
        let instrument = result
            .instrument
            .unwrap_or_else(|| panic!("{preset_id} must compile"));
        let duration_frames = ticks_to_frames(preview.length_ticks, preview);
        let tail_frames = seconds_to_frames(0.1, SAMPLE_RATE).expect("preview tail fits");
        let audio = render_instrument_with_tempo(
            instrument,
            RenderRequest {
                sample_rate: SAMPLE_RATE,
                block_size: BLOCK_SIZE,
                duration_frames,
                tail_frames,
            },
            &preview_events(preview),
            preview.tempo_bpm,
        )
        .unwrap_or_else(|error| panic!("{preset_id} preview must render: {error}"));
        assert_eq!(audio.channels.len(), 2, "{preset_id} renders stereo audio");
        assert!(
            audio
                .channels
                .iter()
                .flatten()
                .all(|sample| sample.is_finite()),
            "{preset_id} preview contains non-finite samples"
        );
        let peak = audio
            .channels
            .iter()
            .flatten()
            .map(|sample| sample.abs())
            .fold(0.0_f32, f32::max);
        assert!(peak > 1.0e-5, "{preset_id} preview is silent: peak={peak}");
    }
}
