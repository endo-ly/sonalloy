use std::process::ExitCode;

fn main() -> ExitCode {
    sonalloy_cli_core::run(include_str!(concat!(
        env!("OUT_DIR"),
        "/preset-catalog.json"
    )))
}
