//! Command-line interface for RoadPilot-Map.
//!
//! This binary is a thin layer over the `roadpilot_map` library. It parses
//! arguments, prints results and sets the exit code. It contains no map
//! processing logic.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use roadpilot_map::dev::{self, Report, Status};

/// Name of the installed binary, used in output and usage messages.
const BIN_NAME: &str = "roadpilot-map";

/// Exit code when a required doctor check failed (see `docs/architecture/library-and-cli.md`).
const EXIT_FAILURE: u8 = 1;

/// Exit code for invalid command-line usage (see `docs/architecture/library-and-cli.md`).
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.as_slice() {
        [flag] if flag == "--version" || flag == "-V" => {
            println!("{BIN_NAME} {}", roadpilot_map::version());
            ExitCode::SUCCESS
        }
        [command] if command == "doctor" => run_doctor(None),
        [command, flag, dir] if command == "doctor" && flag == "--project-dir" => {
            run_doctor(Some(PathBuf::from(dir)))
        }
        [] => usage_error("no arguments given"),
        _ => usage_error(&format!("unrecognized arguments: {}", args.join(" "))),
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    eprintln!("usage: {BIN_NAME} --version");
    eprintln!("       {BIN_NAME} doctor [--project-dir <path>]");
    ExitCode::from(EXIT_USAGE)
}

/// Runs the development diagnostics for the given project folder, or for the
/// workspace found above the current folder.
fn run_doctor(project_dir: Option<PathBuf>) -> ExitCode {
    let project_dir = match project_dir {
        Some(dir) => dir,
        None => match find_project_from_current_dir() {
            Ok(dir) => dir,
            Err(message) => {
                eprintln!("error: {message}");
                eprintln!(
                    "Run the command inside a RoadPilot-Map checkout, or pass --project-dir <path>."
                );
                return ExitCode::from(EXIT_FAILURE);
            }
        },
    };

    let report = dev::diagnose(&project_dir, &dev::detect_tools(&project_dir));
    print_report(&project_dir, &report);
    if report.is_success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_FAILURE)
    }
}

fn find_project_from_current_dir() -> Result<PathBuf, String> {
    let current = std::env::current_dir()
        .map_err(|e| format!("cannot determine the current folder ({:?})", e.kind()))?;
    dev::find_workspace_root(&current).ok_or_else(|| {
        format!(
            "no Cargo workspace found in {} or any parent folder",
            current.display()
        )
    })
}

fn print_report(project_dir: &Path, report: &Report) {
    println!("RoadPilot-Map Doctor");
    println!("Project: {}", project_dir.display());
    println!();
    for check in &report.checks {
        let label = match check.status {
            Status::Pass => "PASS",
            Status::Warn => "WARN",
            Status::Fail => "FAIL",
        };
        println!("[{label}] {}: {}", check.name, check.detail);
    }
    let warnings = report.count(Status::Warn);
    println!();
    println!(
        "Result: {} passed, {} failed, {} {}",
        report.count(Status::Pass),
        report.count(Status::Fail),
        warnings,
        if warnings == 1 { "warning" } else { "warnings" }
    );
}
