//! Command-line interface for RoadPilot-Map.
//!
//! This binary is a thin layer over the `roadpilot_map` library. It parses
//! arguments, prints results and sets the exit code. It contains no map
//! processing logic.

use std::process::ExitCode;

/// Name of the installed binary, used in output and usage messages.
const BIN_NAME: &str = "roadpilot-map";

/// Exit code for invalid command-line usage (see `docs/architecture/library-and-cli.md`).
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.as_slice() {
        [flag] if flag == "--version" || flag == "-V" => {
            println!("{BIN_NAME} {}", roadpilot_map::version());
            ExitCode::SUCCESS
        }
        [] => {
            eprintln!("error: no arguments given");
            eprintln!("usage: {BIN_NAME} --version");
            ExitCode::from(EXIT_USAGE)
        }
        _ => {
            eprintln!("error: unrecognized arguments: {}", args.join(" "));
            eprintln!("usage: {BIN_NAME} --version");
            ExitCode::from(EXIT_USAGE)
        }
    }
}
