mod command;
mod midi;
mod musical_time;
mod output;
mod performance;
mod realtime;
mod score;

use std::process::ExitCode;

use clap::Parser;

/// Runs the CLI with its build-time preset catalog.
///
/// # Panics
///
/// Panics if the supplied catalog is not valid JSON when the preset list command runs.
#[must_use]
pub fn run(preset_catalog_json: &str) -> ExitCode {
    command::run(command::Cli::parse(), preset_catalog_json)
}
