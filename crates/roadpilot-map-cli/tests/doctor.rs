//! End-to-end tests for `roadpilot-map doctor`.
//!
//! These tests run the real binary against synthetic project folders in the
//! system temporary directory. The doctor runs `rustc` and `cargo deny`, so
//! the Rust toolchain that runs these tests must be available. One test hides
//! cargo-deny by giving the binary a reduced `PATH` and an empty `CARGO_HOME`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The repository's toolchain file. Synthetic projects use the same content,
/// so rustup never needs to install another toolchain for them.
const RUST_TOOLCHAIN: &str = include_str!("../../../rust-toolchain.toml");

/// A synthetic project folder that is deleted when dropped.
struct TempProject {
    root: PathBuf,
}

impl TempProject {
    fn complete(test_name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "roadpilot-map-cli-doctor-{}-{test_name}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).expect("remove leftover test folder");
        }
        let project = TempProject { root };
        project.write(
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = [\n    \"crates/roadpilot-map\",\n    \"crates/roadpilot-map-cli\",\n]\n\n[workspace.package]\nrust-version = \"1.85\"\n",
        );
        project.write("rust-toolchain.toml", RUST_TOOLCHAIN);
        for file in ["Cargo.lock", "rustfmt.toml", "deny.toml"] {
            project.write(file, "# synthetic test file\n");
        }
        for member in ["crates/roadpilot-map", "crates/roadpilot-map-cli"] {
            project.write(&format!("{member}/Cargo.toml"), "[package]\n");
        }
        for doc in [
            "README.md",
            "docs/README.md",
            "docs/architecture/overview.md",
            "docs/roadmap/phase-0.md",
            "docs/testing.md",
        ] {
            project.write(doc, "# Synthetic\n");
        }
        project
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.root.join(relative);
        let parent = path.parent().expect("test paths have a parent folder");
        fs::create_dir_all(parent).expect("create test folder");
        fs::write(path, content).expect("write test file");
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn doctor() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_roadpilot-map"));
    command.arg("doctor");
    command
}

fn run(command: &mut Command) -> (Option<i32>, String, String) {
    let Output {
        status,
        stdout,
        stderr,
    } = command
        .output()
        .expect("failed to start the roadpilot-map binary");
    (
        status.code(),
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    )
}

/// The folder that contains the active toolchain's own `rustc` and `cargo`.
fn toolchain_bin_dir(project: &Path) -> PathBuf {
    let output = Command::new("rustc")
        .args(["--print", "sysroot"])
        .current_dir(project)
        .output()
        .expect("run rustc --print sysroot");
    assert!(output.status.success(), "rustc --print sysroot failed");
    PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()).join("bin")
}

#[test]
fn complete_project_exits_with_zero() {
    let project = TempProject::complete("complete");
    let (code, stdout, stderr) = run(doctor().arg("--project-dir").arg(project.path()));
    assert_eq!(code, Some(0), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.starts_with("RoadPilot-Map Doctor\n"), "{stdout}");
    assert!(stdout.contains("[PASS] Rust toolchain: rustc "), "{stdout}");
    assert!(stdout.contains("[PASS] Documentation"), "{stdout}");
    assert!(stdout.contains("Result: "), "{stdout}");
    assert!(!stdout.contains("[FAIL]"), "{stdout}");
}

#[test]
fn failed_required_check_exits_with_one() {
    let project = TempProject::complete("missing-config");
    fs::remove_file(project.path().join("rustfmt.toml")).expect("remove test file");
    let (code, stdout, _) = run(doctor().arg("--project-dir").arg(project.path()));
    assert_eq!(code, Some(1), "{stdout}");
    assert!(
        stdout.contains("[FAIL] Project configuration: missing: rustfmt.toml"),
        "{stdout}"
    );
    assert!(stdout.contains(", 1 failed, "), "{stdout}");
}

#[test]
fn missing_cargo_deny_warns_and_exits_with_zero() {
    let project = TempProject::complete("no-cargo-deny");
    let empty_cargo_home = project.path().join("empty cargo home");
    fs::create_dir_all(&empty_cargo_home).expect("create empty CARGO_HOME");
    let bin = toolchain_bin_dir(project.path());

    let (code, stdout, stderr) = run(doctor()
        .arg("--project-dir")
        .arg(project.path())
        .env("PATH", &bin)
        .env("CARGO_HOME", &empty_cargo_home));
    assert_eq!(code, Some(0), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        stdout.contains("[WARN] cargo-deny: not installed"),
        "{stdout}"
    );
    assert!(stdout.contains("[PASS] Rust toolchain"), "{stdout}");
    assert!(stdout.contains(", 0 failed, 1 warning"), "{stdout}");
}

#[test]
fn project_path_with_spaces_works() {
    let project = TempProject::complete("with spaces in the name");
    let (code, stdout, _) = run(doctor().arg("--project-dir").arg(project.path()));
    assert_eq!(code, Some(0), "{stdout}");
    assert!(stdout.contains("with spaces in the name"), "{stdout}");
}

#[test]
fn project_is_found_from_a_subfolder() {
    let project = TempProject::complete("discover");
    let (code, stdout, _) = run(doctor().current_dir(project.path().join("docs")));
    assert_eq!(code, Some(0), "{stdout}");
}

#[test]
fn missing_project_dir_value_is_a_usage_error() {
    let (code, stdout, stderr) = run(doctor().arg("--project-dir"));
    assert_eq!(code, Some(2));
    assert!(stdout.is_empty());
    assert!(stderr.contains("usage:"), "{stderr}");
}

#[test]
fn unknown_doctor_argument_is_a_usage_error() {
    let (code, stdout, stderr) = run(doctor().arg("--bogus"));
    assert_eq!(code, Some(2));
    assert!(stdout.is_empty());
    assert!(stderr.contains("--bogus"), "{stderr}");
}

#[test]
fn this_repository_passes() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (code, stdout, stderr) = run(doctor().arg("--project-dir").arg(&repo_root));
    assert_eq!(code, Some(0), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(!stdout.contains("[FAIL]"), "{stdout}");
}
