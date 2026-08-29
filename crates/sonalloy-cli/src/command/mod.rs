use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::Args;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use sonalloy_core::{
    AdsrDefinition, AudioAnalysis, AudioAnalysisOptions, CompileContext, CompiledInstrument,
    DEFAULT_TEMPO_BPM, Diagnostic, DiagnosticCode, InstrumentDefinition, InstrumentMetadata,
    LayerDefinition, LayerTriggerDefinition, ModulationCurve, ModulationUnit, MusicalTimeMap,
    OscillatorDefinition, OscillatorWaveform, ParameterHandle, ParameterOwner, ParameterScale,
    ParameterUnit, PerformanceDefinition, ProcessEventKind, ProcessSpec, ProcessorDefinition,
    RenderRequest, RenderTraceReport, ScheduledEvent, TraceRequest, VoiceStealingDefinition,
    analyze_rendered_audio, backend_info, compile_instrument, prepare_audio_file,
    render_instrument_with_input, render_instrument_with_input_and_reset,
    render_instrument_with_input_and_trace, render_sine, seconds_to_frames,
};

use crate::midi::{export_pattern, import_pattern, parse_midi, read_midi};
use crate::pattern::{
    PatternDefinition, PatternInspection, compile as compile_pattern, default_pattern,
    inspect as inspect_pattern, validate as validate_pattern,
};

use crate::output::CliFailure;

mod dev;
pub(crate) mod instrument;
pub(crate) mod pattern;
pub(crate) mod realtime;
mod render;

const DEFAULT_SAMPLE_RATE: u32 = 48_000;
const DEFAULT_BLOCK_SIZE: usize = 257;

#[derive(Debug, Parser)]
#[command(
    name = "sonalloy",
    version,
    about = "Sonalloy realtime and offline instrument engine"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Work with JSON Instrument Definitions.
    Instrument {
        #[command(subcommand)]
        command: instrument::InstrumentCommand,
    },
    /// Work with one-instrument audition patterns.
    Pattern {
        #[command(subcommand)]
        command: pattern::PatternCommand,
    },
    /// Render an instrument offline.
    Render {
        #[command(subcommand)]
        command: render::RenderCommand,
    },
    /// Audition a pattern or MIDI file through an audio output.
    Audition {
        #[command(subcommand)]
        command: realtime::AuditionCommand,
    },
    /// Inspect realtime audio and MIDI devices.
    Device {
        #[command(subcommand)]
        command: realtime::DeviceCommand,
    },
    /// Play an instrument from a live MIDI input through an audio output.
    Play(realtime::PlayArgs),
    /// Development-only commands used to verify the audio path.
    Dev {
        #[command(subcommand)]
        command: dev::DevCommand,
    },
}

pub(crate) fn run(cli: Cli) -> ExitCode {
    match cli.command {
        Command::Instrument { command } => instrument::run(command),
        Command::Pattern { command } => pattern::run(command),
        Command::Render { command } => render::run(command),
        Command::Audition { command } => realtime::run_audition(command),
        Command::Device { command } => realtime::run_device(command),
        Command::Play(args) => realtime::run_play(&args),
        Command::Dev { command } => dev::run(command),
    }
}

pub(crate) fn load_and_compile(
    path: &Path,
    sample_rate: u32,
    block_size: usize,
) -> Result<(Arc<CompiledInstrument>, Vec<Diagnostic>), CliFailure> {
    let text = std::fs::read_to_string(path).map_err(|error| CliFailure {
        code: 2,
        diagnostics: vec![
            Diagnostic::error(
                DiagnosticCode::DefinitionError,
                "could not read Definition input",
            )
            .with_path(path.to_string_lossy())
            .with_detail(error.to_string()),
        ],
    })?;
    let definition: InstrumentDefinition =
        serde_json::from_str(&text).map_err(|error| CliFailure {
            code: 1,
            diagnostics: vec![
                Diagnostic::error(
                    if error.to_string().starts_with("missing field") {
                        DiagnosticCode::RequiredFieldMissing
                    } else {
                        DiagnosticCode::JsonInvalid
                    },
                    "could not parse Definition JSON",
                )
                .with_path(path.to_string_lossy())
                .with_detail(format!(
                    "line {}, column {}: {error}",
                    error.line(),
                    error.column()
                )),
            ],
        })?;
    let input_channels = definition
        .external_audio
        .map_or(0, |external_audio| external_audio.channels.channel_count());
    let process_spec = ProcessSpec::new(f64::from(sample_rate), block_size, input_channels, 2)
        .map_err(|error| CliFailure {
            code: 2,
            diagnostics: vec![
                Diagnostic::error(DiagnosticCode::ValueOutOfRange, error.to_string())
                    .with_path("process_spec"),
            ],
        })?;
    let base_dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir.to_path_buf(),
            process_spec,
        },
    );
    let Some(instrument) = result.instrument else {
        return Err(CliFailure {
            code: 1,
            diagnostics: result.diagnostics,
        });
    };
    Ok((instrument, result.diagnostics))
}
