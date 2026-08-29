use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use sonalloy_core::{
    AdsrDefinition, Diagnostic, DiagnosticCode, InstrumentDefinition, InstrumentMetadata,
    LayerDefinition, LayerTriggerDefinition, OscillatorDefinition, OscillatorWaveform,
    PerformanceDefinition, ProcessorDefinition, VoiceStealingDefinition,
};

use crate::output::{CliFailure, StatusReport, print_failure, print_warnings};

use super::{DEFAULT_BLOCK_SIZE, DEFAULT_SAMPLE_RATE, load_and_compile};

mod inspect;

#[derive(Debug, Subcommand)]
pub(super) enum InstrumentCommand {
    /// Create a minimal oscillator Definition.
    Init(InitArgs),
    /// Parse, validate, and compile a Definition.
    Validate(DefinitionArgs),
    /// Display the compiled Definition in a human-readable form.
    Inspect(DefinitionArgs),
}

#[derive(Debug, Args)]
pub(super) struct InitArgs {
    /// Destination Definition path.
    path: PathBuf,
}

#[derive(Debug, Args)]
pub(super) struct DefinitionArgs {
    /// Definition JSON path.
    definition: PathBuf,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
}

pub(super) fn run(command: InstrumentCommand) -> ExitCode {
    match command {
        InstrumentCommand::Init(args) => run_init(&args),
        InstrumentCommand::Validate(args) => run_validate(&args),
        InstrumentCommand::Inspect(args) => run_inspect(&args),
    }
}
fn run_init(args: &InitArgs) -> ExitCode {
    if args.path.exists() {
        print_failure(
            false,
            &CliFailure {
                code: 2,
                diagnostics: vec![
                    Diagnostic::error(
                        DiagnosticCode::DefinitionError,
                        "destination already exists",
                    )
                    .with_path(args.path.to_string_lossy()),
                ],
            },
        );
        return ExitCode::from(2);
    }
    let json = match serde_json::to_string_pretty(&default_definition()) {
        Ok(json) => json,
        Err(error) => {
            print_failure(
                false,
                &CliFailure {
                    code: 4,
                    diagnostics: vec![
                        Diagnostic::error(
                            DiagnosticCode::DefinitionError,
                            "could not serialize default Definition",
                        )
                        .with_detail(error.to_string()),
                    ],
                },
            );
            return ExitCode::from(4);
        }
    };
    if let Err(error) = std::fs::write(&args.path, format!("{json}\n")) {
        print_failure(
            false,
            &CliFailure {
                code: 4,
                diagnostics: vec![
                    Diagnostic::error(DiagnosticCode::WavOutputError, "could not write Definition")
                        .with_path(args.path.to_string_lossy())
                        .with_detail(error.to_string()),
                ],
            },
        );
        return ExitCode::from(4);
    }
    println!("created {}", args.path.display());
    ExitCode::SUCCESS
}

fn run_validate(args: &DefinitionArgs) -> ExitCode {
    let result = load_and_compile(&args.definition, DEFAULT_SAMPLE_RATE, DEFAULT_BLOCK_SIZE);
    match result {
        Ok((_, diagnostics)) => {
            let report = StatusReport {
                status: "ok",
                command: "instrument validate",
                diagnostics,
            };
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string(&report).expect("status report is serializable")
                );
            } else {
                println!("valid {}", args.definition.display());
                print_warnings(&report.diagnostics);
            }
            ExitCode::SUCCESS
        }
        Err(failure) => {
            print_failure(args.json, &failure);
            ExitCode::from(failure.code)
        }
    }
}

fn run_inspect(args: &DefinitionArgs) -> ExitCode {
    let result = load_and_compile(&args.definition, DEFAULT_SAMPLE_RATE, DEFAULT_BLOCK_SIZE);
    match result {
        Ok((compiled, diagnostics)) => {
            inspect::print(&compiled, diagnostics, args.json);
            ExitCode::SUCCESS
        }
        Err(failure) => {
            print_failure(args.json, &failure);
            ExitCode::from(failure.code)
        }
    }
}

fn default_definition() -> InstrumentDefinition {
    InstrumentDefinition {
        schema_version: sonalloy_core::CURRENT_SCHEMA_VERSION,
        external_audio: None,
        metadata: InstrumentMetadata {
            name: "Basic Poly Synth".to_owned(),
            author: None,
            description: Some("A headless oscillator instrument".to_owned()),
        },
        performance: PerformanceDefinition::Polyphonic {
            polyphony: 16,
            voice_stealing: VoiceStealingDefinition::QuietestReleasingThenOldest,
        },
        layers: vec![LayerDefinition {
            id: "body".to_owned(),
            enabled: true,
            trigger: LayerTriggerDefinition {
                event: sonalloy_core::LayerTriggerEvent::NoteOn,
                key_min: 0,
                key_max: 127,
                velocity_min: 1,
                velocity_max: 127,
            },
            gain_db: -14.0,
            pan: 0.0,
            tuning_cents: 0.0,
            envelope: AdsrDefinition {
                attack_seconds: 0.005,
                decay_seconds: 0.18,
                sustain_level: 0.65,
                release_seconds: 0.3,
            },
            generator: sonalloy_core::GeneratorDefinition::Oscillator(OscillatorDefinition {
                waveform: OscillatorWaveform::Saw,
                phase_reset: true,
                phase: 0.0,
                hard_sync: None,
                waveshaping: None,
                phase_distortion: None,
                wavefold: None,
                feedback: None,
                unison: None,
            }),
            processors: Vec::new(),
        }],
        voice_processors: vec![ProcessorDefinition::Filter(
            sonalloy_core::FilterProcessorDefinition {
                id: "tone".to_owned(),
                mode: sonalloy_core::FilterModeDefinition::LowPass,
                cutoff_hz: 12_000.0,
                resonance: 0.12,
            },
        )],
        global_processors: Vec::new(),
        modulation: None,
        macros: Vec::new(),
        vectors: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::default_definition;

    #[test]
    fn default_definition_is_valid() {
        assert!(default_definition().validate().is_empty());
    }
}
