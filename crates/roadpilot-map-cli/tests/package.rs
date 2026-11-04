//! End-to-end tests of the map package commands (no game data).

use std::path::PathBuf;
use std::process::{Command, Output};

use roadpilot_map::export::ExportMetadata;
use roadpilot_map::model::{LaneNetwork, NavigationGraphBuilder, PrefabNetwork, RoadNetwork};
use roadpilot_map::{Game, InstallationModel, PackageOptions, write_package};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_roadpilot-map"))
        .args(args)
        .output()
        .expect("failed to start the roadpilot-map binary")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "roadpilot-map-cli-package-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn empty_package(dir: &std::path::Path) {
    let network = RoadNetwork::new([], []).expect("roads");
    let lanes = LaneNetwork::new(&network, []).expect("lanes");
    let prefabs = PrefabNetwork::new(&network, [], []).expect("prefabs");
    let graph = NavigationGraphBuilder::new().finish().expect("graph");
    let model = InstallationModel {
        game: Game::Ets2,
        network,
        lanes,
        prefabs,
        graph,
        archives: Vec::new(),
        skipped_archives: 0,
    };
    let meta = ExportMetadata {
        game: Game::Ets2,
        map: None,
        game_version: None,
        archives: None,
    };
    write_package(dir, None, &model, &meta, PackageOptions::default()).expect("package");
}

#[test]
fn package_usage_errors_exit_with_two() {
    let dir = temp_dir("usage");
    let d = dir.to_string_lossy().into_owned();
    let out = dir.join("pkg").to_string_lossy().into_owned();
    for args in [
        vec!["build-package"],
        vec!["build-package", "--game", "ets2", "--game-dir", &d],
        vec![
            "build-package",
            "--game",
            "ets3",
            "--game-dir",
            &d,
            "--out",
            &out,
        ],
        vec![
            "build-package",
            "--game",
            "ets2",
            "--game-dir",
            &d,
            "--out",
            &out,
            "--bogus",
        ],
        // The flag belongs to build-package only.
        vec![
            "export-navigation",
            "--game",
            "ets2",
            "--game-dir",
            &d,
            "--out",
            &out,
            "--without-navigation",
        ],
        vec!["check-package"],
        vec!["check-package", "a", "b"],
    ] {
        let o = run(&args);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", stderr(&o));
        assert!(o.stdout.is_empty());
        assert!(stderr(&o).contains("usage:"));
        assert!(stderr(&o).contains("build-package"));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_that_is_no_installation_exits_with_three() {
    let dir = temp_dir("noinstall");
    let elsewhere = temp_dir("noinstall-out");
    let out = elsewhere.join("pkg");
    let o = run(&[
        "build-package",
        "--game",
        "ets2",
        "--game-dir",
        &dir.to_string_lossy(),
        "--out",
        &out.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(3), "{}", stderr(&o));
    assert!(!out.exists() && !elsewhere.join("pkg.partial").exists());

    // An output inside the game folder is refused before anything is read.
    let inside = dir.join("pkg");
    let o = run(&[
        "build-package",
        "--game",
        "ets2",
        "--game-dir",
        &dir.to_string_lossy(),
        "--out",
        &inside.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(stderr(&o).contains("inside the game folder"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&elsewhere);
}

#[test]
fn an_existing_output_is_refused_before_loading() {
    let dir = temp_dir("exists");
    let game = temp_dir("exists-game");
    let out = dir.join("pkg");
    std::fs::create_dir(&out).expect("dir");
    let o = run(&[
        "build-package",
        "--game",
        "ets2",
        "--game-dir",
        &game.to_string_lossy(),
        "--out",
        &out.to_string_lossy(),
    ]);
    // Exit 1 (output problem), not 3: the output is checked first.
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(stderr(&o).contains("already exists"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&game);
}

#[test]
fn check_package_accepts_a_valid_package_and_rejects_damage() {
    let dir = temp_dir("check");
    let pkg = dir.join("pkg");
    empty_package(&pkg);

    let o = run(&["check-package", &pkg.to_string_lossy()]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let text = stdout(&o);
    assert!(text.contains("Valid map package: game ets2"));
    assert!(text.contains("Nodes: 0, roads: 0"));
    assert!(text.contains("Limitations:"));
    assert!(text.contains("lane_positions_are_unavailable"));

    // A damaged geometry file: the checksum no longer matches (exit 4).
    let geometry = pkg.join("geometry.jsonl");
    let mut bytes = std::fs::read(&geometry).expect("geometry");
    bytes[10] ^= 1;
    std::fs::write(&geometry, bytes).expect("write");
    let o = run(&["check-package", &pkg.to_string_lossy()]);
    assert_eq!(o.status.code(), Some(4), "{}", stderr(&o));
    assert!(stderr(&o).contains("checksum"));

    // Not a package folder at all (exit 1).
    let o = run(&["check-package", &dir.join("missing").to_string_lossy()]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    let _ = std::fs::remove_dir_all(&dir);
}
