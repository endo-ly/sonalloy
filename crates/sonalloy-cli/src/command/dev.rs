use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use sonalloy_core::{RenderRequest, backend_info, render_sine, seconds_to_frames};

use super::{DEFAULT_BLOCK_SIZE, DEFAULT_SAMPLE_RATE};
use crate::output::{
    CliFailure, SuccessReport, finish_failure, input_failure, print_success, render_failure,
    write_wav,
};

#[derive(Debug, Subcommand)]
pub(super) enum DevCommand {
    /// Render a sine wave through the complete audio path.
    #[command(
        long_about = "Generate a sine wave and write it as a WAV file. Frequency in Hz must be finite, non-negative, and no greater than half the selected sample rate (the Nyquist frequency). Duration and tail are seconds; duration may be zero, and tail must be finite and non-negative. The sample rate and maximum process block size must be greater than zero. With `--json`, success is reported as machine-readable JSON and execution failures include structured diagnostics."
    )]
    RenderSine(RenderSineArgs),
}
#[derive(Debug, Args)]
pub(super) struct RenderSineArgs {
    /// Oscillator frequency in Hz; finite, non-negative, and no greater than half `--sample-rate`.
    #[arg(long, value_name = "HZ", default_value_t = 440.0, value_parser = super::parse_nonnegative_f32)]
    frequency: f32,
    /// Main render duration in seconds (finite and non-negative).
    #[arg(long, value_name = "SECONDS", value_parser = super::parse_nonnegative_f64)]
    duration: f64,
    /// Sample rate in Hz.
    #[arg(long, value_name = "HZ", default_value_t = DEFAULT_SAMPLE_RATE, value_parser = clap::value_parser!(u32).range(1..))]
    sample_rate: u32,
    /// Maximum process block size in frames; must be greater than zero.
    #[arg(long, value_name = "FRAMES", default_value_t = DEFAULT_BLOCK_SIZE, value_parser = super::parse_positive_usize)]
    block_size: usize,
    /// Additional render tail in seconds (finite and non-negative).
    #[arg(long, value_name = "SECONDS", default_value_t = 0.0, value_parser = super::parse_nonnegative_f64)]
    tail: f64,
    /// Destination WAV path.
    #[arg(long, value_name = "PATH")]
    output: PathBuf,
    /// Emit machine-readable JSON results; execution failures include structured diagnostics.
    #[arg(long)]
    json: bool,
}

pub(super) fn run(command: DevCommand) -> ExitCode {
    match command {
        DevCommand::RenderSine(args) => run_render_sine(&args),
    }
}

fn run_render_sine(args: &RenderSineArgs) -> ExitCode {
    match render_sine_command(args) {
        Ok(report) => print_success(args.json, report),
        Err(failure) => finish_failure(args.json, failure),
    }
}

fn render_sine_command(args: &RenderSineArgs) -> Result<SuccessReport, CliFailure> {
    let sample_rate = f64::from(args.sample_rate);
    let duration_frames =
        seconds_to_frames(args.duration, sample_rate).map_err(|error| input_failure(&error))?;
    let tail_frames =
        seconds_to_frames(args.tail, sample_rate).map_err(|error| input_failure(&error))?;
    let request = RenderRequest {
        sample_rate,
        block_size: args.block_size,
        duration_frames,
        tail_frames,
    };
    let audio = render_sine(args.frequency, request).map_err(|error| render_failure(&error))?;
    write_wav(&args.output, &audio).map_err(|error| CliFailure {
        code: 4,
        diagnostics: vec![error],
    })?;
    Ok(SuccessReport {
        status: "ok",
        sample_rate: audio.sample_rate,
        channels: audio.channels.len(),
        frames: audio.frames(),
        reported_latency_frames: 0,
        output: args.output.to_string_lossy().into_owned(),
        backend: backend_info().version,
        diagnostics: Vec::new(),
        analysis: None,
        trace: None,
        reset_comparison: None,
    })
}
