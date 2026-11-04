//! Tests of the template measurement format and its evaluation, on
//! synthetic data only. No game data, no telemetry recording.

use roadpilot_map::Game;
use roadpilot_map::geometry::RoadCurve;
use roadpilot_map::model::RoadGeometry;
use roadpilot_map::validation::*;

const EXAMPLE: &str =
    include_str!("../../../docs/validation/examples/synthetic-template-measurement.jsonl");

/// A straight road heading north (−Z), 100 units long.
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

fn candidates(edge_x: f64) -> Vec<TemplateCandidate> {
    vec![
        TemplateCandidate {
            marking: "centre".to_owned(),
            part: MarkingPart::Centre,
            x: 0.0,
        },
        TemplateCandidate {
            marking: "edge_right".to_owned(),
            part: MarkingPart::Centre,
            x: edge_x,
        },
    ]
}

/// A generated set (in memory only) for the logic tests. Tyre edges at
/// `+e_l` and `e_r` from the placement point; the markings at template
/// x = 0 and x = 4.5, placed to the right of the stored direction.
fn generated(game: Game, template: &str, synthetic: bool, edge_true_x: f64) -> MeasurementSet {
    let (e_l, e_r) = (1.35, -1.15);
    let mut observations = Vec::new();
    for (marking, x) in [("centre", 0.0), ("edge_right", edge_true_x)] {
        // Lateral position (positive left) of a line at template x, x to the right.
        let line = -x;
        for pass in 1..=3u32 {
            let jitter = [0.0, 0.02, -0.02][pass as usize % 3];
            let z = -20.0 * f64::from(pass);
            for (direction, tyre, lateral, heading) in [
                (Direction::Forward, Tyre::Left, line - e_l, 0.0),
                (Direction::Forward, Tyre::Right, line - e_r, 0.0),
                (Direction::Backward, Tyre::Left, line + e_l, 0.5),
                (Direction::Backward, Tyre::Right, line + e_r, 0.5),
            ] {
                // On a road heading north, lateral (left) = −x.
                observations.push(Observation {
                    pass,
                    direction,
                    tyre,
                    marking: marking.to_owned(),
                    part: MarkingPart::Centre,
                    placement: Placement {
                        x: -lateral + jitter,
                        y: 0.0,
                        z,
                        heading,
                    },
                    uncertainty_m: 0.05,
                    point: Some(format!("p{pass}")),
                    frame: None,
                    pitch: None,
                    roll: None,
                    speed_mps: Some(0.0),
                    criterion_met: true,
                    notes: None,
                });
            }
        }
    }
    MeasurementSet {
        header: MeasurementHeader {
            game,
            telemetry_game_version: Some("1.0".to_owned()),
            game_patch: Some("generated".to_owned()),
            package_content_id: Some(1),
            template: template.to_owned(),
            road: 1,
            look: "road.generated".to_owned(),
            variant: None,
            method: "tyre_edge_alignment".to_owned(),
            synthetic,
            section: None,
            trailer: Some(false),
            local_scale: None,
            notes: None,
        },
        observations,
        candidates: Vec::new(),
    }
}

#[test]
fn the_example_file_reads_and_evaluates_but_counts_for_nothing() {
    let set = read_measurements(EXAMPLE).expect("valid example");
    assert!(set.header.synthetic);
    assert_eq!(set.observations.len(), 24);
    let report = evaluate(&set, &straight(), &candidates(4.5));
    assert_eq!(report.results.len(), 2);
    for m in &report.results {
        let line = m.line_m.expect("all four kinds");
        let x = m.candidate_x.expect("candidate");
        // Template x grows to the right in the generated example.
        assert!((line + x).abs() < 0.02, "{m:?}");
        let (w1, w2) = m.width_estimates_m.expect("width");
        assert!(
            (w1 - 2.5).abs() < 0.03 && (w2 - 2.5).abs() < 0.03,
            "{w1} {w2}"
        );
        let (o1, o2) = m.reference_offset_estimates_m.expect("offset");
        assert!(
            (o1 - 0.1).abs() < 0.03 && (o2 - 0.1).abs() < 0.03,
            "{o1} {o2}"
        );
    }
    assert!(report.max_heading_error_deg < 1e-6);
    let a = assess(&[report], &Acceptance::default());
    assert_eq!(a.verdict, Verdict::InsufficientData);
    assert!(!a.checks.iter().find(|c| c.id == "C0").expect("C0").passed);
}

#[test]
fn heading_follows_the_telemetry_convention() {
    let close =
        |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-12 && (a[1] - b[1]).abs() < 1e-12;
    assert!(close(heading_direction(0.0), [0.0, -1.0])); // north = −Z
    assert!(close(heading_direction(0.25), [-1.0, 0.0])); // west = −X
    assert!(close(heading_direction(0.5), [0.0, 1.0])); // south
    assert!(close(heading_direction(0.75), [1.0, 0.0])); // east
}

#[test]
fn the_placement_offset_on_the_truck_cancels_out() {
    // The same line measured with two different (unknown) placement points
    // gives the same line position.
    let mut a = generated(Game::Ets2, "t.pmd", false, 4.5);
    let report_a = evaluate(&a, &straight(), &candidates(4.5));
    // Shift the placement point 0.4 m to the truck's left: forward passes
    // move left (−x), backward passes move right (+x).
    for o in &mut a.observations {
        o.placement.x += match o.direction {
            Direction::Forward => -0.4,
            Direction::Backward => 0.4,
        };
    }
    let report_b = evaluate(&a, &straight(), &candidates(4.5));
    for (ra, rb) in report_a.results.iter().zip(&report_b.results) {
        assert!((ra.line_m.expect("a") - rb.line_m.expect("b")).abs() < 1e-9);
    }
}

#[test]
fn complete_real_data_from_both_games_meets_the_criteria_but_is_not_confirmed() {
    let mut reports = Vec::new();
    for game in [Game::Ets2, Game::Ats] {
        for t in ["a.pmd", "b.pmd", "c.pmd"] {
            reports.push(evaluate(
                &generated(game, t, false, 4.5),
                &straight(),
                &candidates(4.5),
            ));
        }
    }
    let a = assess(&reports, &Acceptance::default());
    assert_eq!(
        a.verdict,
        Verdict::CriteriaMetPendingReview,
        "{:#?}",
        a.checks
    );
    assert_eq!(a.x_direction, Some(XDirection::Right));
    // The review criterion is never met automatically.
    assert!(!a.checks.iter().find(|c| c.id == "C7").expect("C7").passed);
}

#[test]
fn one_game_or_too_few_templates_is_insufficient() {
    let ets2_only: Vec<_> = ["a.pmd", "b.pmd", "c.pmd"]
        .iter()
        .map(|t| {
            evaluate(
                &generated(Game::Ets2, t, false, 4.5),
                &straight(),
                &candidates(4.5),
            )
        })
        .collect();
    assert_eq!(
        assess(&ets2_only, &Acceptance::default()).verdict,
        Verdict::InsufficientData
    );
    // Missing provenance.
    let mut set = generated(Game::Ats, "a.pmd", false, 4.5);
    set.header.game_patch = None;
    let r = evaluate(&set, &straight(), &candidates(4.5));
    assert_eq!(
        assess(&[r], &Acceptance::default()).verdict,
        Verdict::InsufficientData
    );
}

#[test]
fn a_template_that_disagrees_with_the_measurement_is_inconsistent() {
    // The road's edge line really lies at 4.9, the template says 4.5.
    let mut reports = Vec::new();
    for game in [Game::Ets2, Game::Ats] {
        for t in ["a.pmd", "b.pmd", "c.pmd"] {
            reports.push(evaluate(
                &generated(game, t, false, 4.9),
                &straight(),
                &candidates(4.5),
            ));
        }
    }
    let a = assess(&reports, &Acceptance::default());
    assert_eq!(a.verdict, Verdict::Inconsistent);
    assert_eq!(a.x_direction, None);
}

#[test]
fn a_curved_road_or_a_wrong_heading_is_inconsistent() {
    let mut reports = Vec::new();
    for game in [Game::Ets2, Game::Ats] {
        for t in ["a.pmd", "b.pmd", "c.pmd"] {
            let mut set = generated(game, t, false, 4.5);
            // Heading 5° off the road.
            for o in &mut set.observations {
                o.placement.heading = (o.placement.heading + 5.0 / 360.0) % 1.0;
            }
            reports.push(evaluate(&set, &straight(), &candidates(4.5)));
        }
    }
    assert_eq!(
        assess(&reports, &Acceptance::default()).verdict,
        Verdict::Inconsistent
    );
}

#[test]
fn rejects_malformed_measurement_files() {
    let first = EXAMPLE.lines().next().expect("header");
    for (bad, what) in [
        ("", "empty"),
        (first, "no footer"),
        (
            &EXAMPLE.replace("\"major\":1", "\"major\":2")[..],
            "version",
        ),
        (
            &EXAMPLE.replace("roadpilot-map-template-measurement", "other")[..],
            "format",
        ),
        (
            &EXAMPLE.replace("\"observations\":24", "\"observations\":23")[..],
            "count",
        ),
        (
            &EXAMPLE.replacen("\"heading\":0.5", "\"heading\":1.5", 1)[..],
            "heading",
        ),
        (
            &EXAMPLE.replacen("\"uncertainty_m\":0.05", "\"uncertainty_m\":0", 1)[..],
            "uncertainty",
        ),
        (
            &EXAMPLE.replacen("\"tyre\":\"left\"", "\"tyre\":\"middle\"", 1)[..],
            "tyre",
        ),
        (
            &EXAMPLE.replace("tyre_edge_alignment", "guess")[..],
            "method",
        ),
        (
            &EXAMPLE.replace("synthetic_1-1.pmd", "synthetic_1-1.pmg")[..],
            "template",
        ),
        (
            &EXAMPLE.replace("\"synthetic\":true", "\"synthetic\":\"yes\"")[..],
            "synthetic flag",
        ),
        (
            &EXAMPLE.replacen("\"marking\":\"centre\"", "\"marking\":\"Centre Line\"", 1)[..],
            "marking id",
        ),
    ] {
        assert!(read_measurements(bad).is_err(), "{what}");
    }
    let trailing = format!("{EXAMPLE}{first}\n");
    assert!(
        read_measurements(&trailing).is_err(),
        "data after the footer"
    );
    assert_eq!(
        read_measurements(&" ".repeat(MAX_FILE_LEN + 1)),
        Err(MeasurementError::TooLarge)
    );
}
