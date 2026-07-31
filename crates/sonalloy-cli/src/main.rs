use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use sonalloy_core::{
    Diagnostic, DiagnosticCode, RenderError, RenderRequest, backend_info, from_render_error,
    render_sine, seconds_to_frames,
};

#[derive(Debug, Parser)]
#[command(
    name = "sonalloy",
    version,
    about = "Sonalloy offline instrument engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Development-only commands used to verify the audio path.
    Dev {
        #[command(subcommand)]
        command: DevCommand,
    },
}

#[derive(Debug, Subcommand)]
enum DevCommand {
    /// Render a sine wave through the complete audio path.
    RenderSine(RenderSineArgs),
}

#[derive(Debug, Args)]
struct RenderSineArgs {
    /// Oscillator frequency in Hz.
    #[arg(long, default_value_t = 440.0)]
    frequency: f32,
    /// Main render duration in seconds.
    #[arg(long)]
    duration: f64,
    /// Sample rate in Hz.
    #[arg(long, default_value_t = 48_000)]
    sample_rate: u32,
    /// Maximum process block size.
    #[arg(long, default_value_t = 257)]
    block_size: usize,
    /// Additional render tail in seconds.
    #[arg(long, default_value_t = 0.0)]
    tail: f64,
    /// Destination WAV path.
    #[arg(long)]
    output: PathBuf,
    /// Emit machine-readable JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Serialize)]
struct SuccessReport {
    status: &'static str,
    sample_rate: u32,
    channels: usize,
    frames: usize,
    output: String,
    backend: String,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Dev {
            command: DevCommand::RenderSine(args),
        } => run_render_sine(&args),
    }
}

fn run_render_sine(args: &RenderSineArgs) -> ExitCode {
    let result = render_sine_command(args);
    match result {
        Ok(report) => {
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string(&report).expect("success report is serializable")
                );
            } else {
                println!(
                    "rendered {} frames at {} Hz to {} using {}",
                    report.frames, report.sample_rate, report.output, report.backend
                );
            }
            ExitCode::SUCCESS
        }
        Err((code, diagnostic)) => {
            print_diagnostic(args.json, code, &diagnostic);
            ExitCode::from(code)
        }
    }
}

fn render_sine_command(args: &RenderSineArgs) -> Result<SuccessReport, (u8, Diagnostic)> {
    if !args.frequency.is_finite() || args.frequency < 0.0 {
        return Err((
            2,
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "frequency must be finite and non-negative",
            ),
        ));
    }
    let sample_rate = f64::from(args.sample_rate);
    let duration_frames = seconds_to_frames(args.duration, sample_rate)
        .map_err(|error| (2, input_diagnostic(&error)))?;
    let tail_frames =
        seconds_to_frames(args.tail, sample_rate).map_err(|error| (2, input_diagnostic(&error)))?;
    let request = RenderRequest {
        sample_rate,
        block_size: args.block_size,
        duration_frames,
        tail_frames,
    };
    let audio =
        render_sine(args.frequency, request).map_err(|error| render_error_result(&error))?;
    write_wav(&args.output, &audio).map_err(|error| (4, error))?;

    Ok(SuccessReport {
        status: "ok",
        sample_rate: audio.sample_rate,
        channels: audio.channels.len(),
        frames: audio.frames(),
        output: args.output.to_string_lossy().into_owned(),
        backend: backend_info().version,
    })
}

fn input_diagnostic(error: &RenderError) -> Diagnostic {
    Diagnostic::error(DiagnosticCode::ValueOutOfRange, error.to_string())
}

fn render_error_result(error: &RenderError) -> (u8, Diagnostic) {
    let code = if matches!(error, RenderError::Process(_)) {
        3
    } else {
        2
    };
    let diagnostic = if code == 2 {
        input_diagnostic(error)
    } else {
        from_render_error(error)
    };
    (code, diagnostic)
}

fn write_wav(path: &Path, audio: &sonalloy_core::RenderedAudio) -> Result<(), Diagnostic> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: audio.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|error| {
        Diagnostic::error(
            DiagnosticCode::WavOutputError,
            "could not create wav output",
        )
        .with_path(path.to_string_lossy())
        .with_detail(error.to_string())
    })?;
    for frame in 0..audio.frames() {
        writer
            .write_sample(audio.channels[0][frame])
            .map_err(|error| {
                Diagnostic::error(DiagnosticCode::WavOutputError, "could not write wav output")
                    .with_path(path.to_string_lossy())
                    .with_detail(error.to_string())
            })?;
        writer
            .write_sample(audio.channels[1][frame])
            .map_err(|error| {
                Diagnostic::error(DiagnosticCode::WavOutputError, "could not write wav output")
                    .with_path(path.to_string_lossy())
                    .with_detail(error.to_string())
            })?;
    }
    writer.finalize().map_err(|error| {
        Diagnostic::error(
            DiagnosticCode::WavOutputError,
            "could not finalize wav output",
        )
        .with_path(path.to_string_lossy())
        .with_detail(error.to_string())
    })?;
    Ok(())
}

fn print_diagnostic(json: bool, exit_code: u8, diagnostic: &Diagnostic) {
    if json {
        #[derive(Serialize)]
        struct ErrorReport<'a> {
            status: &'static str,
            exit_code: u8,
            diagnostics: [&'a Diagnostic; 1],
        }
        let report = ErrorReport {
            status: "error",
            exit_code,
            diagnostics: [diagnostic],
        };
        println!(
            "{}",
            serde_json::to_string(&report).expect("error report is serializable")
        );
    } else {
        eprintln!("error[{:?}]: {}", diagnostic.code, diagnostic.message);
        if let Some(path) = &diagnostic.path {
            eprintln!("  path: {path}");
        }
        if let Some(detail) = &diagnostic.detail {
            eprintln!("  detail: {detail}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_error_maps_to_process_exit_and_diagnostic() {
        let error = RenderError::Process(sonalloy_core::ProcessError::DspFailure {
            kind: sonalloy_core::DspFailureKind::InvalidInput,
        });
        let (code, diagnostic) = render_error_result(&error);
        assert_eq!(code, 3);
        assert_eq!(diagnostic.code, DiagnosticCode::DspError);
        assert_eq!(
            diagnostic.severity,
            sonalloy_core::DiagnosticSeverity::Error
        );
    }
}
