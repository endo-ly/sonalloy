mod command;
mod demo;
mod midi;
mod musical_time;
mod output;
mod pattern;
mod realtime;
mod score;

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    command::run(command::Cli::parse())
}
