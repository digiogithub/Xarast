//! Entry point for the headless Xarast tools. See [`xarast_cli::USAGE`].

use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(xarast_cli::run(&argv).code())
}
