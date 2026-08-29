pub(crate) mod command;
mod midi;
mod musical_time;
mod output;
mod pattern;
mod realtime;

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    command::run(command::Cli::parse())
}
