//! Command-line interface for RoadPilot-Map.
//!
//! This binary is a thin layer over the `roadpilot_map` library. It parses
//! arguments, prints results and sets the exit code. It contains no map
//! processing logic.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use roadpilot_map::dev::{self, Report, Status};
use roadpilot_map::export::{
    self, CheckError, ExportError, ExportInput, ExportMetadata, ExportSummary,
};
use roadpilot_map::{Game, InstallationError, load_installation};

/// Name of the installed binary, used in output and usage messages.
const BIN_NAME: &str = "roadpilot-map";

/// Exit code when a required doctor check failed (see `docs/architecture/library-and-cli.md`).
const EXIT_FAILURE: u8 = 1;

/// Exit code when the game installation is not found or not recognized.
const EXIT_INSTALLATION: u8 = 3;

/// Exit code when processing failed (unsupported or malformed data).
const EXIT_PROCESSING: u8 = 4;

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
        [command, rest @ ..] if command == "export-navigation" => run_export(rest),
        [command, file] if command == "check-navigation-export" => run_check(Path::new(file)),
        [] => usage_error("no arguments given"),
        _ => usage_error(&format!("unrecognized arguments: {}", args.join(" "))),
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    eprintln!("usage: {BIN_NAME} --version");
    eprintln!("       {BIN_NAME} doctor [--project-dir <path>]");
    eprintln!(
        "       {BIN_NAME} export-navigation --game <ets2|ats> --game-dir <path> --out <file> [--force]"
    );
    eprintln!("       {BIN_NAME} check-navigation-export <file>");
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

/// The options of `export-navigation`.
struct ExportArgs {
    game: Game,
    game_dir: PathBuf,
    out: PathBuf,
    force: bool,
}

fn parse_export_args(args: &[String]) -> Result<ExportArgs, String> {
    let (mut game, mut game_dir, mut out, mut force) = (None, None, None, false);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--game" => {
                let v = value("--game")?;
                game = Some(
                    Game::parse(&v)
                        .ok_or_else(|| format!("unknown game \"{v}\" (use ets2 or ats)"))?,
                );
            }
            "--game-dir" => game_dir = Some(PathBuf::from(value("--game-dir")?)),
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--force" => force = true,
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }
    Ok(ExportArgs {
        game: game.ok_or("--game is required")?,
        game_dir: game_dir.ok_or("--game-dir is required")?,
        out: out.ok_or("--out is required")?,
        force,
    })
}

/// Loads the navigation model of a game installation and writes the export.
fn run_export(args: &[String]) -> ExitCode {
    let args = match parse_export_args(args) {
        Ok(a) => a,
        Err(message) => return usage_error(&message),
    };
    // Fail early, before the slow loading, if the output cannot be used.
    if let Err(e) = export::check_output_path(&args.out, args.force, Some(&args.game_dir)) {
        eprintln!("error: {e}");
        return ExitCode::from(EXIT_FAILURE);
    }
    let started = Instant::now();
    let model = match load_installation(&args.game_dir, args.game) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(match e {
                InstallationError::NotRecognized(_) => EXIT_INSTALLATION,
                _ => EXIT_PROCESSING,
            });
        }
    };
    println!(
        "Loaded {}: {} nodes, {} edges in {:.1} s ({} archives read, {} skipped)",
        args.game.id(),
        model.graph.node_count(),
        model.graph.edge_count(),
        started.elapsed().as_secs_f64(),
        model.archives.len(),
        model.skipped_archives
    );
    let started = Instant::now();
    let meta = ExportMetadata::for_installation(&model);
    match export::write_navigation_export_file(
        &args.out,
        args.force,
        Some(&args.game_dir),
        &ExportInput::from_installation(&model),
        &meta,
    ) {
        Ok(stats) => {
            println!(
                "Wrote {}: {} nodes, {} edges, {} issues, {} bytes in {:.1} s",
                args.out.display(),
                stats.nodes,
                stats.edges,
                stats.issues,
                stats.bytes,
                started.elapsed().as_secs_f64()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(match e {
                ExportError::Inconsistent(_) => EXIT_PROCESSING,
                _ => EXIT_FAILURE,
            })
        }
    }
}

/// Checks an export file against the specification.
fn run_check(path: &Path) -> ExitCode {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: cannot open {} ({:?})", path.display(), e.kind());
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    match export::check_navigation_export(BufReader::new(file)) {
        Ok(summary) => {
            print_summary(&summary);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(match e {
                CheckError::Io(_) => EXIT_FAILURE,
                _ => EXIT_PROCESSING,
            })
        }
    }
}

fn print_summary(summary: &ExportSummary) {
    let h = &summary.header;
    println!(
        "Valid navigation graph export: game {}, schema {}.{}, generator {}",
        h.game,
        h.schema_major,
        h.schema_minor,
        h.generator_version.as_deref().unwrap_or("unknown")
    );
    println!(
        "Nodes: {}, edges: {}, issues: {}",
        h.nodes, h.edges, h.issues
    );
    for (title, map) in [
        ("Nodes", &summary.nodes_by_kind),
        ("Edges by status", &summary.edges_by_status),
        ("Edges by category", &summary.edges_by_category),
        ("Issues", &summary.issues_by_kind),
    ] {
        let parts: Vec<String> = map.iter().map(|(k, v)| format!("{k} {v}")).collect();
        println!("{title}: {}", parts.join(", "));
    }
    println!("Limitations: {}", h.limitations.join(", "));
}
