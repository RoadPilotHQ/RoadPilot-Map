//! Development diagnostics for working on RoadPilot-Map itself.
//!
//! This module is not part of map processing. It checks that a RoadPilot-Map
//! project folder has the expected workspace, configuration and documentation,
//! and that the development tools are available. It is used by the
//! `roadpilot-map doctor` command.
//!
//! The work is split in two so that it can be tested without the real
//! developer environment:
//!
//! - [`detect_tools`] runs `rustc` and `cargo deny` to find their versions.
//! - [`diagnose`] evaluates a project folder against given tool versions. It
//!   only reads files and never modifies anything.
//!
//! ```no_run
//! use std::path::Path;
//! use roadpilot_map::dev;
//!
//! let project = Path::new("path/to/RoadPilot-Map");
//! let report = dev::diagnose(project, &dev::detect_tools(project));
//! assert!(report.is_success());
//! ```

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Workspace members that must exist. Planned crates that do not exist yet are
/// not listed here.
pub const WORKSPACE_MEMBERS: [&str; 2] = ["crates/roadpilot-map", "crates/roadpilot-map-cli"];

/// Configuration files that must exist in the project root.
pub const CONFIG_FILES: [&str; 5] = [
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "rustfmt.toml",
    "deny.toml",
];

/// Documentation files and folders that must exist, relative to the project root.
pub const DOCUMENTATION: [&str; 5] = [
    "README.md",
    "docs/README.md",
    "docs/architecture",
    "docs/roadmap/phase-0.md",
    "docs/testing.md",
];

/// The outcome of one diagnostic check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The check passed.
    Pass,
    /// Something optional is missing. This does not make the report fail.
    Warn,
    /// A required check failed.
    Fail,
}

/// One diagnostic check and its result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Short name of what was checked, for example `"Rust toolchain"`.
    pub name: String,
    /// The outcome.
    pub status: Status,
    /// Human-readable details, for example a version or a list of missing files.
    pub detail: String,
}

/// The results of all diagnostic checks, in a fixed order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// All checks that were run.
    pub checks: Vec<Check>,
}

impl Report {
    /// Returns how many checks have the given status.
    #[must_use]
    pub fn count(&self, status: Status) -> usize {
        self.checks.iter().filter(|c| c.status == status).count()
    }

    /// Returns `true` if no required check failed. Warnings do not count as failures.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.count(Status::Fail) == 0
    }
}

/// Versions reported by the development tools, or `None` if a tool is not available.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolVersions {
    /// Output of `rustc --version`, for example `"rustc 1.97.1 (8bab26f4f 2026-07-14)"`.
    pub rustc: Option<String>,
    /// Output of `cargo deny --version`, for example `"cargo-deny 0.20.2"`.
    pub cargo_deny: Option<String>,
}

/// Runs `rustc --version` and `cargo deny --version` in `project_dir`.
///
/// Running them in the project folder lets rustup apply the project's
/// `rust-toolchain.toml`. A tool that cannot be started, or exits with an
/// error, is reported as `None`. Nothing is installed or modified.
#[must_use]
pub fn detect_tools(project_dir: &Path) -> ToolVersions {
    ToolVersions {
        rustc: command_output(project_dir, "rustc", &["--version"]),
        cargo_deny: command_output(project_dir, "cargo", &["deny", "--version"]),
    }
}

/// Returns the closest folder, starting at `start` and going up, whose
/// `Cargo.toml` defines a Cargo workspace.
#[must_use]
pub fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| {
            fs::read_to_string(dir.join("Cargo.toml"))
                .is_ok_and(|manifest| has_workspace_table(&manifest))
        })
        .map(Path::to_path_buf)
}

/// Checks the project folder `project_dir` using the given tool versions.
///
/// Only reads files. Checks are returned in this order: Rust toolchain,
/// workspace manifest, each workspace member, project configuration,
/// cargo-deny, documentation. A missing cargo-deny is a warning; everything
/// else is required.
#[must_use]
pub fn diagnose(project_dir: &Path, tools: &ToolVersions) -> Report {
    let manifest = fs::read_to_string(project_dir.join("Cargo.toml"));
    let manifest_text = manifest.as_deref().ok();

    let mut checks = vec![
        check_toolchain(manifest_text, tools.rustc.as_deref()),
        check_manifest(&manifest),
    ];
    for member in WORKSPACE_MEMBERS {
        checks.push(check_member(project_dir, manifest_text, member));
    }
    checks.push(check_paths(
        "Project configuration",
        project_dir,
        &CONFIG_FILES,
    ));
    checks.push(check_cargo_deny(tools.cargo_deny.as_deref()));
    checks.push(check_paths("Documentation", project_dir, &DOCUMENTATION));
    Report { checks }
}

fn check(name: impl Into<String>, status: Status, detail: impl Into<String>) -> Check {
    Check {
        name: name.into(),
        status,
        detail: detail.into(),
    }
}

fn check_toolchain(manifest: Option<&str>, rustc: Option<&str>) -> Check {
    const NAME: &str = "Rust toolchain";
    let Some(rustc) = rustc else {
        return check(NAME, Status::Fail, "rustc not found or not runnable");
    };
    let Some(active) = parse_rustc_version(rustc) else {
        return check(
            NAME,
            Status::Fail,
            format!("unrecognized rustc version output: {rustc}"),
        );
    };
    match manifest.and_then(declared_rust_version) {
        None => check(
            NAME,
            Status::Fail,
            format!("rustc {active}; no rust-version declared in Cargo.toml"),
        ),
        Some(minimum) if active >= minimum => check(
            NAME,
            Status::Pass,
            format!("rustc {active} (minimum {minimum})"),
        ),
        Some(minimum) => check(
            NAME,
            Status::Fail,
            format!("rustc {active} is older than the minimum {minimum}"),
        ),
    }
}

fn check_manifest(manifest: &io::Result<String>) -> Check {
    const NAME: &str = "Workspace manifest";
    match manifest {
        Ok(text) if has_workspace_table(text) => check(NAME, Status::Pass, "Cargo.toml"),
        Ok(_) => check(NAME, Status::Fail, "Cargo.toml has no [workspace] table"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            check(NAME, Status::Fail, "Cargo.toml not found")
        }
        Err(e) => check(
            NAME,
            Status::Fail,
            format!("Cargo.toml cannot be read ({:?})", e.kind()),
        ),
    }
}

fn check_member(project_dir: &Path, manifest: Option<&str>, member: &str) -> Check {
    let name = format!("Workspace member: {member}");
    if !project_dir.join(member).join("Cargo.toml").is_file() {
        return check(name, Status::Fail, format!("{member}/Cargo.toml not found"));
    }
    if let Some(manifest) = manifest {
        if !manifest.contains(&format!("\"{member}\"")) {
            return check(name, Status::Fail, "not listed in the workspace members");
        }
    }
    check(name, Status::Pass, "present")
}

fn check_paths(name: &str, project_dir: &Path, paths: &[&str]) -> Check {
    let missing: Vec<&str> = paths
        .iter()
        .copied()
        .filter(|p| !project_dir.join(p).exists())
        .collect();
    if missing.is_empty() {
        check(name, Status::Pass, format!("all {} present", paths.len()))
    } else {
        check(
            name,
            Status::Fail,
            format!("missing: {}", missing.join(", ")),
        )
    }
}

fn check_cargo_deny(version: Option<&str>) -> Check {
    match version {
        Some(v) => check("cargo-deny", Status::Pass, v),
        None => check(
            "cargo-deny",
            Status::Warn,
            "not installed (see the dependency conventions in docs/architecture/crates.md)",
        ),
    }
}

fn command_output(dir: &Path, program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

fn has_workspace_table(manifest: &str) -> bool {
    manifest.lines().any(|line| line.trim() == "[workspace]")
}

/// A Rust version such as `1.85` or `1.97.1`. A missing patch number is 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Parses `1.85`, `1.97.1` or `1.98.0-nightly` (the pre-release part is ignored).
fn parse_version(text: &str) -> Option<Version> {
    let core = text.split('-').next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = match parts.next() {
        Some(p) => p.parse().ok()?,
        None => 0,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(Version {
        major,
        minor,
        patch,
    })
}

/// Parses the output of `rustc --version`, for example `rustc 1.97.1 (8bab26f4f 2026-07-14)`.
fn parse_rustc_version(output: &str) -> Option<Version> {
    let mut words = output.split_whitespace();
    if words.next()? != "rustc" {
        return None;
    }
    parse_version(words.next()?)
}

/// Reads the first `rust-version = "..."` entry from a Cargo manifest.
fn declared_rust_version(manifest: &str) -> Option<Version> {
    manifest.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if key.trim() != "rust-version" {
            return None;
        }
        parse_version(value.trim().trim_matches('"'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    #[test]
    fn parses_versions() {
        assert_eq!(parse_version("1.85"), Some(v(1, 85, 0)));
        assert_eq!(parse_version("1.97.1"), Some(v(1, 97, 1)));
        assert_eq!(parse_version("1.98.0-nightly"), Some(v(1, 98, 0)));
        assert_eq!(parse_version("1"), None);
        assert_eq!(parse_version("1.x"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    #[test]
    fn parses_rustc_output() {
        assert_eq!(
            parse_rustc_version("rustc 1.97.1 (8bab26f4f 2026-07-14)"),
            Some(v(1, 97, 1))
        );
        assert_eq!(parse_rustc_version("cargo 1.97.1"), None);
        assert_eq!(parse_rustc_version(""), None);
    }

    #[test]
    fn reads_declared_rust_version() {
        let manifest = "[workspace.package]\nedition = \"2024\"\nrust-version = \"1.85\"\n";
        assert_eq!(declared_rust_version(manifest), Some(v(1, 85, 0)));
        assert_eq!(
            declared_rust_version("rust-version.workspace = true\n"),
            None
        );
        assert_eq!(declared_rust_version("[workspace]\n"), None);
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(v(1, 97, 1) > v(1, 85, 0));
        assert!(v(1, 100, 0) > v(1, 99, 9));
        assert!(v(1, 85, 0) >= v(1, 85, 0));
    }

    #[test]
    fn toolchain_check_compares_against_the_minimum() {
        let manifest = Some("rust-version = \"1.85\"\n");
        let ok = check_toolchain(manifest, Some("rustc 1.97.1 (x 2026-07-14)"));
        assert_eq!(ok.status, Status::Pass);
        assert_eq!(ok.detail, "rustc 1.97.1 (minimum 1.85.0)");

        let old = check_toolchain(manifest, Some("rustc 1.84.0 (x 2025-01-09)"));
        assert_eq!(old.status, Status::Fail);

        assert_eq!(check_toolchain(manifest, None).status, Status::Fail);
        assert_eq!(
            check_toolchain(None, Some("rustc 1.97.1")).status,
            Status::Fail
        );
        assert_eq!(
            check_toolchain(manifest, Some("not rustc")).status,
            Status::Fail
        );
    }

    #[test]
    fn report_counts_and_success() {
        let report = Report {
            checks: vec![check("a", Status::Pass, ""), check("b", Status::Warn, "")],
        };
        assert_eq!(report.count(Status::Pass), 1);
        assert_eq!(report.count(Status::Warn), 1);
        assert!(report.is_success());

        let failed = Report {
            checks: vec![check("c", Status::Fail, "")],
        };
        assert!(!failed.is_success());
    }
}
