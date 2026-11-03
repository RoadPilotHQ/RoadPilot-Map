//! Tests for the development diagnostics in `roadpilot_map::dev`.
//!
//! Each test builds a small synthetic project folder in the system temporary
//! directory and removes it afterwards. Tool versions are passed in, so no
//! real tools are run.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use roadpilot_map::dev::{self, Status, ToolVersions};

/// A synthetic project folder that is deleted when dropped.
struct TempProject {
    root: PathBuf,
}

impl TempProject {
    /// Creates a complete synthetic project in a fresh folder named after the test.
    fn complete(test_name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "roadpilot-map-doctor-{}-{test_name}",
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
        for file in [
            "Cargo.lock",
            "rust-toolchain.toml",
            "rustfmt.toml",
            "deny.toml",
        ] {
            project.write(file, "# synthetic test file\n");
        }
        for member in dev::WORKSPACE_MEMBERS {
            project.write(&format!("{member}/Cargo.toml"), "[package]\n");
        }
        for doc in [
            "README.md",
            "docs/README.md",
            "docs/roadmap/phase-0.md",
            "docs/testing.md",
        ] {
            project.write(doc, "# Synthetic\n");
        }
        project.write("docs/architecture/overview.md", "# Synthetic\n");
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

    fn remove(&self, relative: &str) {
        let path = self.root.join(relative);
        if path.is_dir() {
            fs::remove_dir_all(path).expect("remove test folder");
        } else {
            fs::remove_file(path).expect("remove test file");
        }
    }

    /// Every file below the root with its content, sorted by path.
    fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(dir).expect("read test folder") {
                let path = entry.expect("read test folder entry").path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    let content = fs::read(&path).expect("read test file");
                    out.insert(path, content);
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.root, &mut out);
        out
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn all_tools() -> ToolVersions {
    ToolVersions {
        rustc: Some("rustc 1.97.1 (8bab26f4f 2026-07-14)".to_owned()),
        cargo_deny: Some("cargo-deny 0.20.2".to_owned()),
    }
}

fn status_of(report: &dev::Report, name: &str) -> Status {
    report
        .checks
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("check {name:?} is missing from the report"))
        .status
}

#[test]
fn complete_project_passes_every_check() {
    let project = TempProject::complete("complete");
    let report = dev::diagnose(project.path(), &all_tools());

    let names: Vec<&str> = report.checks.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Rust toolchain",
            "Workspace manifest",
            "Workspace member: crates/roadpilot-map",
            "Workspace member: crates/roadpilot-map-cli",
            "Project configuration",
            "cargo-deny",
            "Documentation",
        ]
    );
    assert!(
        report.checks.iter().all(|c| c.status == Status::Pass),
        "{report:#?}"
    );
    assert!(report.is_success());
}

#[test]
fn missing_configuration_file_fails() {
    let project = TempProject::complete("missing-config");
    project.remove("deny.toml");
    let report = dev::diagnose(project.path(), &all_tools());

    assert_eq!(status_of(&report, "Project configuration"), Status::Fail);
    let detail = &report.checks[4].detail;
    assert_eq!(detail, "missing: deny.toml");
    assert!(!report.is_success());
}

#[test]
fn missing_workspace_member_fails() {
    let project = TempProject::complete("missing-member");
    project.remove("crates/roadpilot-map-cli");
    let report = dev::diagnose(project.path(), &all_tools());

    assert_eq!(
        status_of(&report, "Workspace member: crates/roadpilot-map-cli"),
        Status::Fail
    );
    assert_eq!(
        status_of(&report, "Workspace member: crates/roadpilot-map"),
        Status::Pass
    );
    assert!(!report.is_success());
}

#[test]
fn member_not_listed_in_the_manifest_fails() {
    let project = TempProject::complete("unlisted-member");
    project.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/roadpilot-map\"]\n\n[workspace.package]\nrust-version = \"1.85\"\n",
    );
    let report = dev::diagnose(project.path(), &all_tools());

    assert_eq!(
        status_of(&report, "Workspace member: crates/roadpilot-map-cli"),
        Status::Fail
    );
    assert!(!report.is_success());
}

#[test]
fn missing_documentation_fails() {
    let project = TempProject::complete("missing-docs");
    project.remove("docs/testing.md");
    project.remove("docs/architecture");
    let report = dev::diagnose(project.path(), &all_tools());

    assert_eq!(status_of(&report, "Documentation"), Status::Fail);
    let detail = &report.checks[6].detail;
    assert_eq!(detail, "missing: docs/architecture, docs/testing.md");
    assert!(!report.is_success());
}

#[test]
fn missing_manifest_fails_without_panicking() {
    let project = TempProject::complete("missing-manifest");
    project.remove("Cargo.toml");
    let report = dev::diagnose(project.path(), &all_tools());

    assert_eq!(status_of(&report, "Workspace manifest"), Status::Fail);
    assert_eq!(status_of(&report, "Rust toolchain"), Status::Fail);
    assert_eq!(status_of(&report, "Project configuration"), Status::Fail);
    assert!(!report.is_success());
}

#[test]
fn missing_cargo_deny_is_a_warning_not_a_failure() {
    let project = TempProject::complete("no-cargo-deny");
    let tools = ToolVersions {
        cargo_deny: None,
        ..all_tools()
    };
    let report = dev::diagnose(project.path(), &tools);

    assert_eq!(status_of(&report, "cargo-deny"), Status::Warn);
    assert_eq!(report.count(Status::Warn), 1);
    assert_eq!(report.count(Status::Fail), 0);
    assert!(report.is_success());
}

#[test]
fn missing_or_old_rustc_fails() {
    let project = TempProject::complete("rustc");
    let missing = ToolVersions {
        rustc: None,
        ..all_tools()
    };
    assert_eq!(
        status_of(&dev::diagnose(project.path(), &missing), "Rust toolchain"),
        Status::Fail
    );

    let old = ToolVersions {
        rustc: Some("rustc 1.84.1 (e71f9a9a9 2025-01-27)".to_owned()),
        ..all_tools()
    };
    assert_eq!(
        status_of(&dev::diagnose(project.path(), &old), "Rust toolchain"),
        Status::Fail
    );
}

#[test]
fn paths_with_spaces_work() {
    let project = TempProject::complete("with spaces in the name");
    assert!(project.path().to_string_lossy().contains(' '));
    let report = dev::diagnose(project.path(), &all_tools());
    assert!(report.is_success(), "{report:#?}");
}

#[test]
fn diagnose_does_not_modify_the_project() {
    let project = TempProject::complete("read-only");
    let before = project.snapshot();
    let _ = dev::diagnose(project.path(), &all_tools());
    assert_eq!(before, project.snapshot());
}

#[test]
fn workspace_root_is_found_from_a_subfolder() {
    let project = TempProject::complete("find-root");
    let start = project.path().join("docs").join("roadmap");
    assert_eq!(
        dev::find_workspace_root(&start).as_deref(),
        Some(project.path())
    );
}

#[test]
fn member_manifest_is_not_mistaken_for_the_workspace_root() {
    let project = TempProject::complete("member-root");
    let start = project.path().join("crates").join("roadpilot-map");
    assert_eq!(
        dev::find_workspace_root(&start).as_deref(),
        Some(project.path())
    );
}
