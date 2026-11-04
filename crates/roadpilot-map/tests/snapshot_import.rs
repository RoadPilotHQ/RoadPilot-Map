//! Tests of the telemetry snapshot import. Every snapshot here is synthetic
//! (invented values in the documented layout); no recorded telemetry.

use roadpilot_map::Game;
use roadpilot_map::measurement_sheet::{SHEET_COLUMNS, blank_sheet, convert_sheet};
use roadpilot_map::snapshot_import::{import_snapshot, parse_snapshot};

/// A synthetic snapshot line with the given status, `missing` list and frame
/// body.
fn line(status: &str, missing: &str, frame: &str) -> String {
    format!(
        "{{\"kind\":\"roadpilot.telemetry.snapshot\",\"snapshotVersion\":1,\"status\":\"{status}\",\"ageMs\":12,\"missing\":[{missing}],\"droppedFields\":[],\"frame\":{{{frame}}}}}"
    )
}

const FRAME: &str = concat!(
    "\"protocol\":\"roadpilot.telemetry\",\"schemaVersion\":2,\"session\":42,\"sequence\":7,",
    "\"game\":\"ats\",\"gameVersion\":\"1.5\",\"pluginState\":\"running\",\"simulationTimeUs\":1000,",
    "\"localScale\":19.5,\"placement\":{\"x\":-12345.678901234567,\"y\":12.25,\"z\":98765.4321,",
    "\"heading\":0.1,\"pitch\":-0.0021,\"roll\":0.00037},\"speedMps\":0.0,",
    "\"localVelocityMps\":{\"x\":0.0,\"y\":0.0,\"z\":0.0}"
);

fn fresh() -> String {
    line("fresh", "", FRAME)
}

fn frame_with(from: &str, to: &str) -> String {
    line("fresh", "", &FRAME.replacen(from, to, 1))
}

fn sheet(synthetic: &str) -> String {
    blank_sheet(Game::Ats, &["centre".to_owned()], 1, 1).replace(
        "session,ats,,,,,,,,,,,,",
        &format!("session,ats,1.58,,ab,road_template/x.pmd,a,road.x,,s1,no,,{synthetic},"),
    )
}

fn cell(sheet: &str, pass: u32, column: &str) -> String {
    let i = SHEET_COLUMNS
        .iter()
        .position(|c| *c == column)
        .expect("column");
    let p = SHEET_COLUMNS
        .iter()
        .position(|c| *c == "pass")
        .expect("pass");
    sheet
        .lines()
        .filter(|l| l.starts_with("observation,"))
        .map(|l| l.split(',').map(str::to_owned).collect::<Vec<_>>())
        .find(|c| c[p] == pass.to_string())
        .map(|c| c[i].clone())
        .expect("row")
}

fn set_cell(sheet: &str, pass: u32, column: &str, value: &str) -> String {
    let i = SHEET_COLUMNS
        .iter()
        .position(|c| *c == column)
        .expect("column");
    let p = SHEET_COLUMNS
        .iter()
        .position(|c| *c == "pass")
        .expect("pass");
    sheet
        .lines()
        .map(|l| {
            let mut c: Vec<String> = l.split(',').map(str::to_owned).collect();
            if l.starts_with("observation,") && c[p] == pass.to_string() {
                c[i] = value.to_owned();
            }
            c.join(",")
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
fn a_fresh_snapshot_is_taken_over_unchanged() {
    let s = parse_snapshot(&fresh()).expect("valid");
    assert_eq!(s.game, Game::Ats);
    assert_eq!(s.sdk_game_version, "1.5");
    assert_eq!((s.session, s.sequence), (42, 7));
    assert_eq!(s.x, -12345.678901234567);
    assert_eq!(s.local_scale, Some(19.5));

    let out = import_snapshot(&sheet("yes"), 1, &s, false).expect("imported");
    let t = &out.sheet;
    // Same text as in the snapshot: no rounding, no conversion.
    assert_eq!(cell(t, 1, "x"), "-12345.678901234567");
    assert_eq!(cell(t, 1, "y"), "12.25");
    assert_eq!(cell(t, 1, "z"), "98765.4321");
    assert_eq!(cell(t, 1, "heading"), "0.1");
    assert_eq!(cell(t, 1, "pitch"), "-0.0021");
    assert_eq!(cell(t, 1, "roll"), "0.00037");
    assert_eq!(cell(t, 1, "speed_mps"), "0");
    assert_eq!(cell(t, 1, "frame_session"), "42");
    assert_eq!(cell(t, 1, "frame_sequence"), "7");
    // Human observations stay empty.
    for c in ["criterion_met", "uncertainty_m", "notes"] {
        assert_eq!(cell(t, 1, c), "", "{c}");
    }
    // The SDK version and the scale go into the session row; the game patch
    // (1.58, entered by hand) and the template stay as they were.
    assert!(t.contains("session,ats,1.58,1.5,ab,road_template/x.pmd,a,road.x,,s1,no,19.5,yes,"));
    assert_eq!(
        out.filled_session_columns,
        vec!["telemetry_game_version", "local_scale"]
    );
    // Only the session row and the target row change.
    let before = sheet("yes");
    let changed = before
        .lines()
        .zip(t.lines())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(changed, 2);

    // Not convertible until a person fills in criterion_met and uncertainty.
    assert!(convert_sheet(t).is_err());
    let done = set_cell(
        &set_cell(t, 1, "criterion_met", "yes"),
        1,
        "uncertainty_m",
        "0.05",
    );
    let c = convert_sheet(&done).expect("converts after the manual entries");
    assert_eq!(c.set.observations.len(), 1);
    assert_eq!(c.set.observations[0].placement.x, -12345.678901234567);
    assert!(
        c.set.header.synthetic,
        "the import does not change the synthetic flag"
    );
}

#[test]
fn only_fresh_snapshots_are_accepted() {
    for status in ["repeated", "stale"] {
        let e = parse_snapshot(&line(status, "", FRAME)).expect_err(status);
        assert!(e.to_string().contains(status), "{e}");
    }
    let no_frame = "{\"kind\":\"roadpilot.telemetry.snapshot\",\"snapshotVersion\":1,\"status\":\"no_frame\",\"missing\":[],\"droppedFields\":[]}";
    assert!(parse_snapshot(no_frame).is_err());
}

#[test]
fn missing_fields_are_errors_never_zero() {
    let e = parse_snapshot(&line("fresh", "\"placement\"", FRAME)).expect_err("missing");
    assert!(e.to_string().contains("placement"));
    // Fields absent although `missing` is empty.
    for from in [
        "\"speedMps\":0.0,",
        "\"session\":42,",
        "\"gameVersion\":\"1.5\",",
        ",\"roll\":0.00037",
        "\"x\":-12345.678901234567,",
    ] {
        assert!(parse_snapshot(&frame_with(from, "")).is_err(), "{from}");
    }
}

#[test]
fn invalid_values_are_errors() {
    for (from, to) in [
        ("\"heading\":0.1", "\"heading\":1.0"),
        ("\"pitch\":-0.0021", "\"pitch\":-0.3"),
        ("\"x\":-12345.678901234567", "\"x\":\"-12345.67\""),
        ("\"x\":-12345.678901234567", "\"x\":null"),
        ("\"speedMps\":0.0", "\"speedMps\":\"0,0\""),
        ("\"session\":42", "\"session\":-1"),
        ("\"session\":42", "\"session\":42.5"),
        ("\"sequence\":7", "\"sequence\":9007199254740992"),
        ("\"schemaVersion\":2", "\"schemaVersion\":1"),
        ("\"pluginState\":\"running\"", "\"pluginState\":\"paused\""),
        (
            "\"protocol\":\"roadpilot.telemetry\"",
            "\"protocol\":\"other\"",
        ),
        ("\"gameVersion\":\"1.5\"", "\"gameVersion\":\"1,5\""),
    ] {
        assert!(parse_snapshot(&frame_with(from, to)).is_err(), "{to}");
    }
    for bad in [
        String::new(),
        "not json".to_owned(),
        "{}".to_owned(),
        fresh().replace("roadpilot.telemetry.snapshot", "x"),
        fresh().replace("\"snapshotVersion\":1", "\"snapshotVersion\":2"),
        "x".repeat(70_000),
    ] {
        assert!(
            parse_snapshot(&bad).is_err(),
            "{}",
            &bad[..bad.len().min(40)]
        );
    }
}

#[test]
fn unknown_game_and_mismatching_sheets_are_refused() {
    assert!(parse_snapshot(&frame_with("\"game\":\"ats\"", "\"game\":\"ets3\"")).is_err());
    // An ETS2 frame does not go into an ATS sheet.
    let ets2 = parse_snapshot(&frame_with("\"game\":\"ats\"", "\"game\":\"ets2\"")).expect("valid");
    assert!(import_snapshot(&sheet("no"), 1, &ets2, false).is_err());
    let s = parse_snapshot(&fresh()).expect("valid");
    // The synthetic cell must be decided first.
    assert!(import_snapshot(&sheet(""), 1, &s, false).is_err());
    // Another SDK game version than the session's.
    let other = sheet("no").replacen("session,ats,1.58,,", "session,ats,1.58,1.4,", 1);
    assert!(import_snapshot(&other, 1, &s, false).is_err());
    // Another local scale.
    let scale = sheet("no").replacen(",s1,no,,no,", ",s1,no,20,no,", 1);
    assert!(import_snapshot(&scale, 1, &s, false).is_err());
    // Not a sheet.
    assert!(import_snapshot("a,b\n", 1, &s, false).is_err());
}

#[test]
fn frame_references_and_rows_are_checked() {
    let s = parse_snapshot(&fresh()).expect("valid");
    assert!(
        import_snapshot(&sheet("no"), 99, &s, false).is_err(),
        "unknown pass"
    );
    let once = import_snapshot(&sheet("no"), 1, &s, false)
        .expect("first")
        .sheet;
    // The same frame cannot fill a second row.
    let e = import_snapshot(&once, 2, &s, false).expect_err("same frame twice");
    assert!(e.to_string().contains("42/7"));
    // A filled row is not overwritten without replace.
    let newer = parse_snapshot(&frame_with("\"sequence\":7", "\"sequence\":8")).expect("valid");
    assert!(import_snapshot(&once, 1, &newer, false).is_err());
    let replaced = import_snapshot(&once, 1, &newer, true)
        .expect("replace")
        .sheet;
    assert_eq!(cell(&replaced, 1, "frame_sequence"), "8");
    // A new frame goes into another pass.
    let two = import_snapshot(&once, 2, &newer, false)
        .expect("second")
        .sheet;
    assert_eq!(cell(&two, 2, "frame_sequence"), "8");
    assert_eq!(cell(&two, 1, "frame_sequence"), "7");
}

#[test]
fn numbers_keep_their_value_in_other_notations() {
    let s = parse_snapshot(&frame_with("\"roll\":0.00037", "\"roll\":1e-5")).expect("valid");
    let t = import_snapshot(&sheet("no"), 1, &s, false)
        .expect("imported")
        .sheet;
    assert_eq!(cell(&t, 1, "roll").parse::<f64>().expect("number"), 1e-5);
}
