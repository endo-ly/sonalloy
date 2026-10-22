use std::io::Write;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use sonalloy_core::{
    AdsrDefinition, AssetReference, CompileContext, Diagnostic, DiagnosticCode,
    GeneratorDefinition, InstrumentDefinition, InstrumentMetadata, LayerDefinition,
    LayerTriggerDefinition, LayerTriggerEvent, PerformanceDefinition, ProcessSpec,
    SampleDefinition, SampleInterpolation, SampleNoteOffBehavior, SamplePlaybackDirection,
    SampleRegionDefinition, SampleTimeDefinition, SampleZoneDefinition,
    SampleZonePlaybackDefinition, VoiceStealingDefinition, compile_instrument, read_audio_file,
};

use super::InitArgs;
use crate::output::CliFailure;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(in crate::command) enum NoteOff {
    Release,
    Ignore,
}

pub(super) fn initialize(args: &InitArgs) -> Result<(), CliFailure> {
    if args.path.to_str().is_none() {
        return Err(failure(&args.path, "destination path is not valid UTF-8"));
    }
    if args.path.symlink_metadata().is_ok() {
        return Err(failure(&args.path, "destination already exists"));
    }
    let parent = args
        .path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let base = std::fs::canonicalize(parent).map_err(|error| output_failure(&args.path, &error))?;
    let definition = if let Some(source) = &args.from_sample {
        let source =
            std::fs::canonicalize(source).map_err(|error| failure(source, error.to_string()))?;
        read_audio_file(&source).map_err(|error| failure(&source, error.to_string()))?;
        if args.key_min > args.key_max {
            return Err(failure(
                &args.path,
                "--key-min must be less than or equal to --key-max",
            ));
        }
        let reference = relative_path(&base, &source).unwrap_or_else(|| source.clone());
        let path = reference
            .to_str()
            .ok_or_else(|| failure(&source, "sample path is not valid UTF-8"))?
            .to_owned();
        let resolved = std::fs::canonicalize(base.join(&reference))
            .map_err(|error| failure(&source, error.to_string()))?;
        if resolved != source {
            return Err(failure(
                &source,
                "asset reference does not resolve to the input WAV",
            ));
        }
        sample_definition(args, &source, path)?
    } else {
        super::default_definition()
    };
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base.clone(),
            process_spec: ProcessSpec::new(
                f64::from(super::DEFAULT_SAMPLE_RATE),
                super::DEFAULT_BLOCK_SIZE,
                0,
                2,
            )
            .map_err(|error| failure(&args.path, error.to_string()))?,
        },
    );
    if result.instrument.is_none() {
        return Err(CliFailure {
            code: 1,
            diagnostics: result.diagnostics,
        });
    }
    let mut pending = tempfile::NamedTempFile::new_in(&base)
        .map_err(|error| output_failure(&args.path, &error))?;
    serde_json::to_writer_pretty(&mut pending, &definition)
        .map_err(|error| output_failure(&args.path, &error))?;
    pending
        .write_all(b"\n")
        .and_then(|()| pending.as_file().sync_all())
        .map_err(|error| output_failure(&args.path, &error))?;
    pending.persist_noclobber(&args.path).map_err(|error| {
        if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            failure(&args.path, "destination already exists")
        } else {
            output_failure(&args.path, &error.error)
        }
    })?;
    Ok(())
}

fn failure(path: &Path, message: impl Into<String>) -> CliFailure {
    CliFailure {
        code: 2,
        diagnostics: vec![
            Diagnostic::error(DiagnosticCode::DefinitionError, message)
                .with_path(path.to_string_lossy()),
        ],
    }
}

fn output_failure(path: &Path, error: &dyn std::fmt::Display) -> CliFailure {
    CliFailure {
        code: 4,
        diagnostics: vec![
            Diagnostic::error(DiagnosticCode::WavOutputError, "could not write Definition")
                .with_path(path.to_string_lossy())
                .with_detail(error.to_string()),
        ],
    }
}

fn relative_path(base: &Path, source: &Path) -> Option<PathBuf> {
    let base: Vec<_> = base.components().collect();
    let source: Vec<_> = source.components().collect();
    let common = base.iter().zip(&source).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    let mut path = PathBuf::new();
    for _ in common..base.len() {
        path.push("..");
    }
    for component in &source[common..] {
        path.push(component.as_os_str());
    }
    Some(path)
}

fn sample_definition(
    args: &InitArgs,
    source: &Path,
    path: String,
) -> Result<InstrumentDefinition, CliFailure> {
    let name = args
        .name
        .clone()
        .or_else(|| {
            source
                .file_stem()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        })
        .ok_or_else(|| failure(source, "sample filename is not valid UTF-8"))?;
    Ok(InstrumentDefinition {
        schema_version: sonalloy_core::CURRENT_SCHEMA_VERSION,
        external_audio: None,
        metadata: InstrumentMetadata {
            name,
            author: None,
            description: None,
            category: None,
            tags: Vec::new(),
            recommended_range: None,
        },
        performance: PerformanceDefinition::Polyphonic {
            polyphony: 16,
            voice_stealing: VoiceStealingDefinition::QuietestReleasingThenOldest,
            choke_groups: Vec::new(),
        },
        layers: vec![LayerDefinition {
            id: "sample".to_owned(),
            enabled: true,
            choke_group: None,
            trigger: LayerTriggerDefinition {
                event: LayerTriggerEvent::NoteOn,
                key_min: args.key_min,
                key_max: args.key_max,
                velocity_min: 1,
                velocity_max: 127,
            },
            gain_db: 0.0,
            pan: 0.0,
            tuning_cents: 0.0,
            envelope: AdsrDefinition {
                attack_seconds: 0.0,
                decay_seconds: 0.0,
                sustain_level: 1.0,
                release_seconds: 0.05,
            },
            generator: GeneratorDefinition::Sample(SampleDefinition {
                interpolation: SampleInterpolation::Cubic,
                note_off_behavior: match args.note_off_behavior {
                    NoteOff::Release => SampleNoteOffBehavior::Release,
                    NoteOff::Ignore => SampleNoteOffBehavior::Ignore,
                },
                normalize: None,
                zones: vec![SampleZoneDefinition {
                    id: "sample".to_owned(),
                    asset: AssetReference { path, sha256: None },
                    root_note: args.root_note,
                    pitch_tracking: !args.no_pitch_tracking,
                    gain_db: 0.0,
                    key_min: args.key_min,
                    key_max: args.key_max,
                    velocity_min: 1,
                    velocity_max: 127,
                    round_robin_group: None,
                    playback: SampleZonePlaybackDefinition {
                        region: SampleRegionDefinition {
                            start_seconds: 0.0,
                            end_seconds: None,
                        },
                        direction: SamplePlaybackDirection::Forward,
                        r#loop: None,
                        time: SampleTimeDefinition::Resample,
                    },
                }],
            }),
            processors: Vec::new(),
        }],
        voice_processors: Vec::new(),
        global_processors: Vec::new(),
        modulation: None,
        macros: Vec::new(),
        vectors: Vec::new(),
    })
}
