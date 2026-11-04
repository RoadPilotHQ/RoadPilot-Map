//! Opt-in test: build, check and measure a map package of a whole local ETS2
//! or ATS installation.
//!
//! Ignored by default and never run in CI. Reads the archives read-only,
//! writes only to a temporary folder outside the game folder, checks
//! properties only and prints counts and percentages only.
//!
//! ```powershell
//! $env:ROADPILOT_MAP_GAME_DIR = "C:\Program Files (x86)\Steam\steamapps\common\Euro Truck Simulator 2"
//! $env:ROADPILOT_MAP_GAME = "ets2"   # optional; ats if the folder name contains "American"
//! cargo test --release -p roadpilot-map --test real_package -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use roadpilot_map::export::ExportMetadata;
use roadpilot_map::geometry::RoadCurve;
use roadpilot_map::{Game, PackageOptions, check_package, load_installation, write_package};

fn game_dir() -> PathBuf {
    let dir = std::env::var_os("ROADPILOT_MAP_GAME_DIR")
        .map(PathBuf::from)
        .expect("set ROADPILOT_MAP_GAME_DIR to an ETS2 or ATS installation folder");
    assert!(
        dir.join("base_map.scs").is_file(),
        "ROADPILOT_MAP_GAME_DIR ({}) does not contain base_map.scs",
        dir.display()
    );
    dir
}

fn game(dir: &std::path::Path) -> Game {
    if let Some(g) = std::env::var("ROADPILOT_MAP_GAME")
        .ok()
        .and_then(|v| Game::parse(&v))
    {
        return g;
    }
    if dir.to_string_lossy().contains("American") {
        Game::Ats
    } else {
        Game::Ets2
    }
}

/// Upper bounds, in degrees, of the turn-angle classes.
const CLASSES: [f64; 6] = [10.0, 30.0, 60.0, 90.0, 120.0, 181.0];

#[test]
#[ignore = "reads a local game installation; set ROADPILOT_MAP_GAME_DIR and run with --ignored"]
fn builds_checks_and_measures_a_package_of_the_installation() {
    let dir = game_dir();
    let game = game(&dir);
    let model = load_installation(&dir, game).expect("the installation loads");

    // Accuracy of the estimated curve against the stored road length, by the
    // angle between the node directions.
    let mut total = [0u64; CLASSES.len()];
    let mut consistent = [0u64; CLASSES.len()];
    let mut worst: f64 = 0.0;
    let mut no_curve = 0u64;
    for road in model.network.roads() {
        let Some(curve) = model
            .network
            .geometry(road.id)
            .and_then(|g| RoadCurve::estimate(&g).ok())
        else {
            no_curve += 1;
            continue;
        };
        let angle = curve.turn_angle().to_degrees();
        let class = CLASSES
            .iter()
            .position(|&c| angle < c)
            .unwrap_or(CLASSES.len() - 1);
        total[class] += 1;
        let agreement = curve.length_agreement();
        if agreement.is_consistent() {
            consistent[class] += 1;
        }
        worst = worst.max(agreement.relative_deviation());
    }
    let mut lower = 0.0;
    for (i, upper) in CLASSES.iter().enumerate() {
        let share = if total[i] == 0 {
            f64::NAN
        } else {
            100.0 * consistent[i] as f64 / total[i] as f64
        };
        println!(
            "{}: turn {lower:>3}..{:<3} degrees: {:>7} roads, {share:6.2} % within 0.1 % of the stored length",
            game.id(),
            upper.min(180.0),
            total[i]
        );
        lower = *upper;
    }
    let (all, ok) = (total.iter().sum::<u64>(), consistent.iter().sum::<u64>());
    println!(
        "{}: all roads: {all} with a curve ({no_curve} without), {:.2} % consistent, worst deviation {:.1} %",
        game.id(),
        100.0 * ok as f64 / all as f64,
        100.0 * worst
    );
    assert!(all > 0);
    assert_eq!(
        no_curve, 0,
        "every road of the installation has a curve estimate"
    );
    // The model is an estimate; this only guards against a broken formula.
    assert!(
        ok as f64 / all as f64 > 0.90,
        "the curve model no longer reproduces the stored lengths"
    );

    // Build, check and compare two builds.
    let root =
        std::env::temp_dir().join(format!("roadpilot-map-real-package-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temp folder");
    let meta = ExportMetadata::for_installation(&model);
    let full = root.join("full");
    let stats = write_package(&full, Some(&dir), &model, &meta, PackageOptions::default())
        .expect("package is written");
    println!(
        "{}: package: {} nodes, {} roads, {} lanes, {} junctions, {} connections; geometry {} bytes, navigation {:?} bytes",
        game.id(),
        stats.counts.nodes,
        stats.counts.roads,
        stats.counts.lanes,
        stats.counts.junctions,
        stats.counts.connections,
        stats.geometry_bytes,
        stats.navigation_bytes
    );
    let summary = check_package(&full).expect("the package is valid");
    assert_eq!(summary.manifest.counts, stats.counts);
    assert_eq!(stats.roads_with_curve, all);

    let again = root.join("again");
    let options = PackageOptions {
        include_navigation: false,
        overwrite: false,
    };
    write_package(&again, Some(&dir), &model, &meta, options).expect("second package");
    assert_eq!(
        std::fs::read(full.join("geometry.jsonl")).expect("first geometry"),
        std::fs::read(again.join("geometry.jsonl")).expect("second geometry"),
        "equal input must give equal bytes"
    );
    std::fs::remove_dir_all(&root).expect("cleanup");
}
