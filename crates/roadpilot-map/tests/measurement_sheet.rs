//! Tests of the capture sheet: blank sheets, conversion into the
//! measurement format, and the privacy rules. Synthetic data only.

use roadpilot_map::Game;
use roadpilot_map::geometry::RoadCurve;
use roadpilot_map::measurement_sheet::{SHEET_COLUMNS, blank_sheet, convert_sheet};
use roadpilot_map::model::RoadGeometry;
use roadpilot_map::validation::{
    Acceptance, Verdict, assess, evaluate, read_measurements, write_measurements,
};

const SHEET: &str =
    include_str!("../../../docs/validation/examples/synthetic-measurement-sheet.csv");
const TEMPLATE: &str =
    include_str!("../../../docs/validation/templates/measurement-sheet-ets2.csv");
const JSONL: &str =
    include_str!("../../../docs/validation/examples/synthetic-template-measurement.jsonl");

fn straight() -> RoadCurve {
    RoadCurve::estimate(&RoadGeometry {
        start: [0.0; 3],
        end: [0.0, 0.0, -100.0],
        start_direction: [0.0, 0.0, -1.0],
        end_direction: [0.0, 0.0, -1.0],
        chord_length: 100.0,
        stored_length: 100.0,
    })
    .expect("valid")
}

#[test]
fn the_synthetic_sheet_converts_and_matches_the_jsonl_example() {
    let c = convert_sheet(SHEET).expect("valid sheet");
    assert!(c.set.header.synthetic);
    assert_eq!(c.set.observations.len(), 25);
    assert_eq!(c.planned_observations, 1);
    assert_eq!(c.set.candidates.len(), 2);
    let excluded = c
        .set
        .observations
        .iter()
        .filter(|o| !o.criterion_met)
        .count();
    assert_eq!(excluded, 1);

    // The sheet carries its own candidates; the result equals the JSONL
    // example evaluated with the same candidates.
    let from_sheet = evaluate(&c.set, &straight(), &c.set.candidates);
    assert_eq!(from_sheet.excluded_observations, 1);
    let example = read_measurements(JSONL).expect("example");
    let from_jsonl = evaluate(&example, &straight(), &c.set.candidates);
    for (a, b) in from_sheet.results.iter().zip(&from_jsonl.results) {
        assert!((a.line_m.expect("a") - b.line_m.expect("b")).abs() < 1e-9);
    }
    // Synthetic: never counts.
    assert_eq!(
        assess(&[from_sheet], &Acceptance::default()).verdict,
        Verdict::InsufficientData
    );
}

#[test]
fn conversion_output_round_trips_through_the_jsonl_format() {
    let c = convert_sheet(SHEET).expect("valid sheet");
    let text = write_measurements(&c.set).expect("writable");
    assert!(text.contains("\"minor\":1"));
    let back = read_measurements(&text).expect("readable");
    assert_eq!(back, c.set);
    assert_eq!(
        write_measurements(&back).expect("again"),
        text,
        "deterministic"
    );
}

#[test]
fn the_template_sheet_is_blank() {
    // The checked-in template has the plan only: no measured value.
    let c = convert_sheet(&TEMPLATE.replace(
        "session,ets2,,,,,,,,,,,,",
        "session,ets2,,,,road_template/x.pmd,a,road.x,,,,,no,",
    ))
    .expect("blank template converts once the session is named");
    assert!(c.set.observations.is_empty());
    assert!(c.set.candidates.is_empty());
    assert!(c.planned_observations >= 3 * 4 * 2);
    // As checked in, it refuses to convert until the session row is filled.
    assert!(convert_sheet(TEMPLATE).is_err());
}

#[test]
fn blank_sheets_have_the_plan_and_no_values() {
    let sheet = blank_sheet(
        Game::Ats,
        &["centre".to_owned(), "edge_right".to_owned()],
        3,
        1,
    );
    let rows: Vec<&str> = sheet.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!(rows[0], SHEET_COLUMNS.join(","));
    // Session, two candidates, 2 markings × 3 points × 4 alignments.
    assert_eq!(rows.len(), 1 + 1 + 2 + 24);
    for r in &rows[1..] {
        let cells: Vec<&str> = r.split(',').collect();
        assert_eq!(cells.len(), SHEET_COLUMNS.len());
        for c in [
            "x",
            "y",
            "z",
            "heading",
            "template_x",
            "uncertainty_m",
            "frame_sequence",
        ] {
            let i = SHEET_COLUMNS.iter().position(|k| *k == c).expect("column");
            assert!(cells[i].is_empty(), "{c} must be blank in {r}");
        }
    }
    // Invalid marking ids are skipped instead of written.
    let skipped = blank_sheet(Game::Ets2, &["Bad Id".to_owned()], 1, 1);
    assert!(!skipped.contains("Bad Id"));
}

#[test]
fn a_filled_blank_sheet_converts() {
    let mut sheet = blank_sheet(Game::Ets2, &["centre".to_owned()], 1, 1);
    sheet = sheet.replace(
        "session,ets2,,,,,,,,,,,,",
        "session,ets2,1.58,1.21,ab,road_template/x.pmd,a,road.x,,s1,no,,no,",
    );
    sheet = sheet.replacen(
        "p1,1,forward,left,,,,,,,,,,,,",
        "p1,1,forward,left,yes,7,1001,1.25,0.5,-20.125,0.0001,0,0,0,0.08,",
        1,
    );
    let c = convert_sheet(&sheet).expect("valid");
    assert_eq!(c.set.observations.len(), 1);
    assert_eq!(c.planned_observations, 3);
    assert_eq!(c.planned_candidates, 1);
    let o = &c.set.observations[0];
    assert_eq!(o.placement.z, -20.125);
    assert_eq!(o.frame.map(|f| f.sequence), Some(1001));
    assert!(!c.set.header.synthetic);
    assert_eq!(c.set.header.package_content_id, Some(0xab));
}

#[test]
fn rejects_sheets_that_break_the_rules() {
    let good = SHEET;
    let cases: Vec<(String, &str)> = vec![
        (
            good.replacen("record,game", "record,gamex", 1),
            "wrong header",
        ),
        (
            good.replace(",yes,1,101,", ",yes,1,102,"),
            "duplicate frame",
        ),
        (
            good.replacen(",1.37,0,-20.0,0,", ",1.37,,-20.0,0,", 1),
            "partial placement",
        ),
        (
            good.replacen(",1.37,0,-20.0,0,", ",1.37,0,-20.0,1.5,", 1),
            "heading out of range",
        ),
        (
            good.replacen(",1.37,0,-20.0,0,", ",1;37,0,-20.0,0,", 1),
            "decimal comma",
        ),
        (
            good.replacen("forward,left,yes", "forward,left,", 1),
            "criterion missing",
        ),
        (
            good.replacen("synthetic_1-1.pmd", "synthetic_1-1.pmg", 1),
            "template",
        ),
        (good.replacen("1.21,", "\"1.21\",", 1), "quotes"),
        (
            good.replacen("Synthetic example", "C:/Users/someone", 1),
            "path in notes",
        ),
        (
            good.replacen("Synthetic example", "mail me at x@y", 1),
            "address in notes",
        ),
        (
            good.replacen("Synthetic example", "see https link", 1),
            "link in notes",
        ),
        (good.replacen("session,ets2", "session,ets3", 1), "game"),
        (
            format!(
                "{good}session,ets2,,,,road_template/a.pmd,a,road.a,,,,,no,,,,,,,,,,,,,,,,,,,\n"
            ),
            "two sessions",
        ),
        (
            good.replacen(",yes,1,101,1.37", ",yes,1,,1.37", 1),
            "half frame reference",
        ),
        (
            good.replacen(
                "candidate,,,,,,,,,,,,,edge_right",
                "candidate,,,,,,,,,,,,,centre",
                1,
            ),
            "candidate twice",
        ),
    ];
    for (bad, what) in cases {
        assert!(convert_sheet(&bad).is_err(), "{what}");
    }
    // Rows before the session row.
    let mut lines: Vec<&str> = good.lines().collect();
    lines.swap(2, 3);
    assert!(
        convert_sheet(&lines.join("\n")).is_err(),
        "candidate before session"
    );
    assert!(convert_sheet("").is_err());
}
