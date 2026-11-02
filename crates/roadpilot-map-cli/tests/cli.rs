//! End-to-end tests for the `roadpilot-map` binary.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_roadpilot-map"))
        .args(args)
        .output()
        .expect("failed to start the roadpilot-map binary")
}

fn expected_version_line() -> String {
    format!("roadpilot-map {}\n", env!("CARGO_PKG_VERSION"))
}

#[test]
fn long_version_flag_prints_version_and_succeeds() {
    let output = run(&["--version"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected_version_line());
    assert!(output.stderr.is_empty());
}

#[test]
fn short_version_flag_prints_version_and_succeeds() {
    let output = run(&["-V"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected_version_line());
    assert!(output.stderr.is_empty());
}

#[test]
fn no_arguments_is_a_usage_error() {
    let output = run(&[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
}

#[test]
fn unknown_argument_is_a_usage_error() {
    let output = run(&["--unknown"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--unknown"));
}

#[test]
fn version_flag_with_extra_argument_is_a_usage_error() {
    let output = run(&["--version", "extra"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}
