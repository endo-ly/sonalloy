use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use sonalloy_core::{
    CompileContext, CompiledInstrument, Diagnostic, DiagnosticCode, InstrumentDefinition,
    ProcessSpec, compile_instrument,
};

use crate::output::CliFailure;

mod demo;
mod dev;
mod instrument;
pub(crate) mod pattern;
mod realtime;
mod render;
mod update;

const DEFAULT_SAMPLE_RATE: u32 = 48_000;
const DEFAULT_BLOCK_SIZE: usize = 257;

#[derive(Debug, Parser)]
#[command(
    name = "sonalloy",
    version,
    about = "Validate instruments, render audio, audition patterns, and play from MIDI",
    long_about = "Define instruments in JSON, validate and inspect their compiled configuration, render notes, events, MIDI, patterns, or multi-part demos to audio, audition patterns and MIDI files through an audio output, and play an instrument from a live MIDI input. Use `device list` to find audio and MIDI device IDs."
)]
pub(super) struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create, validate, and inspect JSON Instrument Definitions.
    Instrument {
        #[command(subcommand)]
        command: instrument::InstrumentCommand,
    },
    /// Create, validate, inspect, and convert one-instrument audition patterns.
    Pattern {
        #[command(subcommand)]
        command: pattern::PatternCommand,
    },
    /// Validate, inspect, and export a multi-instrument offline Demo.
    Demo {
        #[command(subcommand)]
        command: demo::DemoCommand,
    },
    /// Render an instrument, pattern, MIDI file, or Demo to audio.
    Render {
        #[command(subcommand)]
        command: render::RenderCommand,
    },
    /// Play a pattern or MIDI file through an audio output without a MIDI input.
    Audition {
        #[command(subcommand)]
        command: realtime::AuditionCommand,
    },
    /// List audio input, audio output, and MIDI input devices.
    Device {
        #[command(subcommand)]
        command: realtime::DeviceCommand,
    },
    /// Play an instrument from a live MIDI input through an audio output.
    #[command(
        long_about = "Play an Instrument Definition from live MIDI input through an audio output; press Enter to stop. `--audio-device` selects an Audio Output ID from `device list`; omission uses the OS default, and an unknown ID fails. `--audio-input-device` selects an Audio Input for Definitions that require external audio; omission uses the OS default input, which must support the Definition's required channel count and the selected sample rate. An unused external input is rejected.

If `--midi-device` is omitted, no available MIDI inputs is an error, one input is selected automatically, and two or more require an explicit ID. Unknown IDs fail. Omit `--sample-rate` to use the output device's default rate; the rate must be positive and supported. `--buffer-size` must be positive and accepted by the device.

`--tempo` is a finite positive playback tempo in BPM. `--time-signature` uses `numerator/denominator`, requires a positive numerator, and allows denominators 1, 2, 4, 8, 16, 32, 64, or 128. Repeat `--macro-cc id=cc` to map Macro IDs to MIDI CC values 0..=127. CC1 and CC64 are reserved; a CC or Macro cannot be mapped more than once, and the Macro ID must exist in the Definition."
    )]
    Play(realtime::PlayArgs),
    /// Update the user-local installation from the latest GitHub release.
    #[command(
        long_about = "Download and install the latest GitHub release for this platform. `update` applies only when the running binary is the regular user-local installation at `~/.local/bin/sonalloy` (or the platform executable equivalent); it fails when run from another location. The downloaded archive is verified against the release's SHA-256 checksum. After a successful replacement, the previous binary remains beside it as `.sonalloy.old` (with the platform executable suffix where applicable)."
    )]
    Update,
    /// Render diagnostic audio for development checks.
    Dev {
        #[command(subcommand)]
        command: dev::DevCommand,
    },
}

pub(super) fn run(cli: Cli) -> ExitCode {
    match cli.command {
        Command::Instrument { command } => instrument::run(command),
        Command::Pattern { command } => pattern::run(command),
        Command::Demo { command } => demo::run(command),
        Command::Render { command } => render::run(command),
        Command::Audition { command } => realtime::run_audition(command),
        Command::Device { command } => realtime::run_device(command),
        Command::Play(args) => realtime::run_play(&args),
        Command::Update => update::run(),
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

pub(super) fn parse_positive_f64(value: &str) -> Result<f64, String> {
    let value = value
        .parse::<f64>()
        .map_err(|_| "expected a finite number greater than zero".to_owned())?;
    (value.is_finite() && value > 0.0)
        .then_some(value)
        .ok_or_else(|| "expected a finite number greater than zero".to_owned())
}

pub(super) fn parse_nonnegative_f64(value: &str) -> Result<f64, String> {
    let value = value
        .parse::<f64>()
        .map_err(|_| "expected a finite non-negative number".to_owned())?;
    (value.is_finite() && value >= 0.0)
        .then_some(value)
        .ok_or_else(|| "expected a finite non-negative number".to_owned())
}

pub(super) fn parse_nonnegative_f32(value: &str) -> Result<f32, String> {
    let value = value
        .parse::<f32>()
        .map_err(|_| "expected a finite non-negative number".to_owned())?;
    (value.is_finite() && value >= 0.0)
        .then_some(value)
        .ok_or_else(|| "expected a finite non-negative number".to_owned())
}

pub(super) fn parse_positive_usize(value: &str) -> Result<usize, String> {
    let value = value
        .parse::<usize>()
        .map_err(|_| "expected a positive integer".to_owned())?;
    (value > 0)
        .then_some(value)
        .ok_or_else(|| "expected a positive integer".to_owned())
}

#[cfg(test)]
mod tests {
    use clap::{Command, CommandFactory, Parser};

    use super::Cli;

    fn assert_help_is_complete(command: &Command) {
        assert!(
            command.get_about().is_some() || command.get_long_about().is_some(),
            "{} has no command description",
            command.get_name()
        );
        for arg in command.get_arguments() {
            if arg.is_hide_set() || matches!(arg.get_id().as_str(), "help" | "version") {
                continue;
            }
            assert!(
                arg.get_help().is_some() || arg.get_long_help().is_some(),
                "{} argument {} has no help",
                command.get_name(),
                arg.get_id()
            );
        }
        for child in command.get_subcommands() {
            assert_help_is_complete(child);
        }
    }

    #[test]
    fn every_command_and_argument_has_help() {
        assert_help_is_complete(&Cli::command());
    }

    #[test]
    fn render_event_option_relationships_are_enforced_by_clap() {
        let common = [
            "sonalloy",
            "render",
            "events",
            "instrument.json",
            "events.json",
            "--duration-frames",
            "32",
            "--output",
            "out.wav",
        ];
        let mut requires_trace = common.to_vec();
        requires_trace.extend(["--trace-every-frames", "64"]);
        assert!(Cli::try_parse_from(requires_trace).is_err());

        let mut zero_trace_interval = common.to_vec();
        zero_trace_interval.extend(["--trace", "voice.tone", "--trace-every-frames", "0"]);
        assert!(Cli::try_parse_from(zero_trace_interval).is_err());

        let mut conflicts_with_trace = common.to_vec();
        conflicts_with_trace.extend(["--trace", "voice.tone", "--reset-check"]);
        assert!(Cli::try_parse_from(conflicts_with_trace).is_err());

        let mut valid_trace = common.to_vec();
        valid_trace.extend(["--trace", "voice.tone", "--trace-every-frames", "64"]);
        assert!(Cli::try_parse_from(valid_trace).is_ok());
    }

    #[test]
    fn numeric_cli_constraints_are_enforced_by_clap() {
        assert!(
            Cli::try_parse_from([
                "sonalloy",
                "render",
                "note",
                "instrument.json",
                "--output",
                "out.wav",
                "--note",
                "128",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "sonalloy",
                "render",
                "note",
                "instrument.json",
                "--output",
                "out.wav",
                "--velocity",
                "0",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "sonalloy",
                "render",
                "note",
                "instrument.json",
                "--output",
                "out.wav",
                "--sample-rate",
                "0",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "sonalloy",
                "render",
                "note",
                "instrument.json",
                "--output",
                "out.wav",
                "--block-size",
                "0",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "sonalloy",
                "render",
                "note",
                "instrument.json",
                "--output",
                "out.wav",
                "--tail",
                "NaN",
            ])
            .is_err()
        );
    }

    #[test]
    fn play_syntax_constraints_are_enforced_by_clap() {
        assert!(
            Cli::try_parse_from([
                "sonalloy",
                "play",
                "instrument.json",
                "--time-signature",
                "4/3",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "sonalloy",
                "play",
                "instrument.json",
                "--macro-cc",
                "motion=64",
            ])
            .is_err()
        );
    }
}
