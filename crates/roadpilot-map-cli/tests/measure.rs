//! End-to-end tests of the measurement commands (synthetic data only).

use std::path::PathBuf;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_roadpilot-map"))
        .args(args)
        .output()
        .expect("failed to start the roadpilot-map binary")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "roadpilot-map-cli-measure-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn repo_file(rel: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
        .to_string_lossy()
        .into_owned()
}

fn text(o: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn blank_sheet_convert_and_check() {
    let dir = temp_dir("flow");
    let sheet = dir.join("s.csv");
    let s = sheet.to_string_lossy().into_owned();
    let o = run(&[
        "measurement-sheet",
        "--game",
        "ats",
        "--markings",
        "centre",
        "--points",
        "2",
        "--out",
        &s,
    ]);
    assert_eq!(o.status.code(), Some(0), "{:?}", text(&o));
    assert!(text(&o).0.contains("8 planned observations"));
    // Not replaced without --force.
    let o = run(&[
        "measurement-sheet",
        "--game",
        "ats",
        "--markings",
        "centre",
        "--out",
        &s,
    ]);
    assert_eq!(o.status.code(), Some(1));

    // The blank sheet has no session details yet: conversion fails (exit 4).
    let out = dir.join("m.jsonl").to_string_lossy().into_owned();
    let o = run(&["convert-measurement-sheet", &s, "--out", &out]);
    assert_eq!(o.status.code(), Some(4), "{:?}", text(&o));

    // The synthetic example converts and checks.
    let example = repo_file("docs/validation/examples/synthetic-measurement-sheet.csv");
    let o = run(&["convert-measurement-sheet", &example, "--out", &out]);
    assert_eq!(o.status.code(), Some(0), "{:?}", text(&o));
    assert!(text(&o).0.contains("synthetic"));
    let o = run(&["check-measurements", &out]);
    assert_eq!(o.status.code(), Some(0), "{:?}", text(&o));
    assert!(text(&o).0.contains("(synthetic)"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn usage_errors_exit_with_two() {
    for args in [
        vec!["measurement-sheet"],
        vec![
            "measurement-sheet",
            "--game",
            "ets2",
            "--markings",
            "Bad Id",
            "--out",
            "x.csv",
        ],
        vec![
            "measurement-sheet",
            "--game",
            "ets2",
            "--markings",
            "a",
            "--points",
            "0",
            "--out",
            "x.csv",
        ],
        vec!["convert-measurement-sheet", "a.csv"],
        vec!["check-measurements"],
        vec!["measurement-candidates", "--package", "p"],
    ] {
        let o = run(&args);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {:?}", text(&o));
        assert!(text(&o).1.contains("usage:"));
    }
}

#[test]
fn an_invalid_measurement_file_exits_with_four() {
    let dir = temp_dir("invalid");
    let f = dir.join("bad.jsonl");
    std::fs::write(&f, "{\"t\":\"header\"}\n").expect("write");
    let o = run(&["check-measurements", &f.to_string_lossy()]);
    assert_eq!(o.status.code(), Some(4), "{:?}", text(&o));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A synthetic snapshot line (invented values).
const SNAPSHOT: &str = r#"{"kind":"roadpilot.telemetry.snapshot","snapshotVersion":1,"status":"fresh","ageMs":5,"missing":[],"droppedFields":[],"frame":{"protocol":"roadpilot.telemetry","schemaVersion":2,"session":5,"sequence":11,"game":"ets2","gameVersion":"1.21","pluginState":"running","localScale":19.0,"placement":{"x":100.125,"y":2.5,"z":-300.75,"heading":0.25,"pitch":0.0,"roll":0.0},"speedMps":0.0}}"#;

fn import_sheet(dir: &std::path::Path) -> String {
    let sheet = dir.join("s.csv");
    let s = sheet.to_string_lossy().into_owned();
    let o = run(&[
        "measurement-sheet",
        "--game",
        "ets2",
        "--markings",
        "centre",
        "--points",
        "1",
        "--out",
        &s,
    ]);
    assert_eq!(o.status.code(), Some(0), "{:?}", text(&o));
    let t = std::fs::read_to_string(&sheet).expect("sheet");
    std::fs::write(
        &sheet,
        t.replace(
            "session,ets2,,,,,,,,,,,,",
            "session,ets2,1.58,,,road_template/x.pmd,a,road.x,,s1,no,,yes,",
        ),
    )
    .expect("write");
    s
}

#[test]
fn imports_a_snapshot_from_a_file_and_from_stdin() {
    let dir = temp_dir("import");
    let s = import_sheet(&dir);
    let snap = dir.join("snap.txt");
    std::fs::write(&snap, format!("\n{SNAPSHOT}\n")).expect("write");
    let o = run(&[
        "import-snapshot",
        &s,
        "--pass",
        "1",
        "--from",
        &snap.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(0), "{:?}", text(&o));
    assert!(text(&o).0.contains("criterion_met and uncertainty_m"));
    let t = std::fs::read_to_string(&s).expect("sheet");
    assert!(t.contains(",5,11,100.125,2.5,-300.75,0.25,0,0,0,,"), "{t}");
    assert!(!dir.join("s.csv.partial").exists());

    // From stdin, into pass 2, with another frame.
    let mut child = Command::new(env!("CARGO_BIN_EXE_roadpilot-map"))
        .args(["import-snapshot", &s, "--pass", "2"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("start");
    {
        use std::io::Write as _;
        let mut stdin = child.stdin.take().expect("stdin");
        writeln!(
            stdin,
            "{}",
            SNAPSHOT.replace("\"sequence\":11", "\"sequence\":12")
        )
        .expect("write");
    }
    let o = child.wait_with_output().expect("run");
    assert_eq!(o.status.code(), Some(0), "{:?}", text(&o));
    assert!(
        std::fs::read_to_string(&s)
            .expect("sheet")
            .contains(",5,12,100.125,")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refused_snapshot_leaves_the_sheet_unchanged() {
    let dir = temp_dir("refuse");
    let s = import_sheet(&dir);
    let before = std::fs::read(&s).expect("sheet");
    let snap = dir.join("snap.txt");
    for bad in [
        SNAPSHOT.replace("\"fresh\"", "\"stale\""),
        SNAPSHOT.replace("\"missing\":[]", "\"missing\":[\"placement\"]"),
        SNAPSHOT.replace("\"game\":\"ets2\"", "\"game\":\"ats\""),
        "{not json".to_owned(),
    ] {
        std::fs::write(&snap, &bad).expect("write");
        let o = run(&[
            "import-snapshot",
            &s,
            "--pass",
            "1",
            "--from",
            &snap.to_string_lossy(),
        ]);
        assert_eq!(o.status.code(), Some(4), "{bad}: {:?}", text(&o));
        assert_eq!(std::fs::read(&s).expect("sheet"), before, "{bad}");
    }
    let o = run(&["import-snapshot", &s]);
    assert_eq!(o.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}
