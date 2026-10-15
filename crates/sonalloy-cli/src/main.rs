mod command;
mod midi;
mod musical_time;
mod output;
mod performance;
mod realtime;
mod score;

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    command::run(command::Cli::parse())
}
