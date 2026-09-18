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
    let mut paths = fs::read_dir(root)
        .expect("presets directory")
        .map(|entry| entry.expect("preset directory entry").path())
        .filter(|path| path.is_dir() && path.file_name().is_some_and(|name| name != "assets"))
        .filter(|path| path.join("definition.json").is_file())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    paths
}

const BUILTIN_TAG_VOCABULARY: &[&str] = &[
    "Warm",
    "Bright",
    "Dark",
    "Clean",
    "Noisy",
    "Metallic",
    "Glassy",
    "Soft",
    "Aggressive",
    "Punchy",
    "Wide",
    "Deep",
    "Analog",
    "Digital",
    "FM",
    "Wavetable",
    "Wavefold",
    "Additive",
    "Formant",
    "Granular",
    "Physical",
    "Spectral",
    "Noise",
    "Mono",
    "Polyphonic",
    "Motion",
    "Rhythmic",
    "Sustained",
    "Plucky",
    "Percussive",
    "Evolving",
    "Gated",
    "Random",
    "Sub",
    "Acid",
    "Reese",
    "Supersaw",
    "Drone",
    "Chord",
    "Bell",
    "Kick",
    "Snare",
    "Clap",
    "Hihat",
    "Crash",
    "Tom",
    "Rim",
    "Shaker",
    "Riser",
    "Impact",
    "Sequence",
    "Texture",
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

#[test]
fn every_builtin_preset_metadata_validates_and_renders() {
    let paths = preset_paths();
    assert_eq!(paths.len(), 60, "all 60 built-in presets must be present");

    for path in paths {
        let preset_id = path
            .file_name()
            .expect("preset directory name")
            .to_str()
            .expect("preset directory name is UTF-8");
        let definition_path = path.join("definition.json");
        let definition: InstrumentDefinition = serde_json::from_str(
            &fs::read_to_string(&definition_path).expect("definition JSON is readable"),
        )
        .unwrap_or_else(|error| panic!("{preset_id} must parse: {error}"));

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
