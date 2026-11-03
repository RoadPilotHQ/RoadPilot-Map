//! End-to-end tests of the navigation export commands (no game data).

use std::path::PathBuf;
use std::process::{Command, Output};

use roadpilot_map::Game;
use roadpilot_map::export::{ExportInput, ExportMetadata, write_navigation_export};
use roadpilot_map::model::{LaneNetwork, NavigationGraphBuilder, PrefabNetwork, RoadNetwork};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_roadpilot-map"))
        .args(args)
        .output()
        .expect("failed to start the roadpilot-map binary")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("roadpilot-map-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn export_usage_errors_exit_with_two() {
    let dir = temp_dir("usage");
    let d = dir.to_string_lossy().into_owned();
    let out = dir.join("g.jsonl").to_string_lossy().into_owned();
    for args in [
        vec!["export-navigation"],
        vec!["export-navigation", "--game", "ets2", "--game-dir", &d],
        vec!["export-navigation", "--game-dir", &d, "--out", &out],
        vec![
            "export-navigation",
            "--game",
            "ets3",
            "--game-dir",
            &d,
            "--out",
            &out,
        ],
        vec!["export-navigation", "--game", "ets2", "--game-dir"],
        vec![
            "export-navigation",
            "--game",
            "ets2",
            "--game-dir",
            &d,
            "--out",
            &out,
            "--bogus",
        ],
        vec!["check-navigation-export"],
        vec!["check-navigation-export", "a", "b"],
    ] {
        let o = run(&args);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", stderr(&o));
        assert!(o.stdout.is_empty());
        assert!(stderr(&o).contains("usage:"));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_that_is_no_installation_exits_with_three() {
    let dir = temp_dir("noinstall");
    let out = dir.join("g.jsonl");
    let o = run(&[
        "export-navigation",
        "--game",
        "ats",
        "--game-dir",
        &dir.to_string_lossy(),
        "--out",
        &out.to_string_lossy(),
    ]);
    // The output is inside the game folder here, which is refused first.
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(stderr(&o).contains("inside the game folder"));
    assert!(!out.exists());

    let elsewhere = temp_dir("noinstall-out");
    let out = elsewhere.join("g.jsonl");
    let o = run(&[
        "export-navigation",
        "--game",
        "ats",
        "--game-dir",
        &dir.to_string_lossy(),
        "--out",
        &out.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(3), "{}", stderr(&o));
    assert!(stderr(&o).contains("base_map.scs"));
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&elsewhere);
}

#[test]
fn an_existing_output_is_not_overwritten_without_force() {
    let dir = temp_dir("exists");
    let out = dir.join("g.jsonl");
    std::fs::write(&out, b"keep me").expect("write");
    let game = temp_dir("exists-game");
    let o = run(&[
        "export-navigation",
        "--game",
        "ets2",
        "--game-dir",
        &game.to_string_lossy(),
        "--out",
        &out.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(stderr(&o).contains("--force"));
    assert_eq!(std::fs::read(&out).expect("read"), b"keep me");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&game);
}

#[test]
fn check_accepts_a_valid_export_and_rejects_broken_files() {
    let dir = temp_dir("check");
    let network = RoadNetwork::new([], []).expect("roads");
    let lanes = LaneNetwork::new(&network, []).expect("lanes");
    let prefabs = PrefabNetwork::new(&network, [], []).expect("prefabs");
    let graph = NavigationGraphBuilder::new().finish().expect("graph");
    let input = ExportInput {
        graph: &graph,
        network: &network,
        lanes: &lanes,
        prefabs: &prefabs,
    };
    let meta = ExportMetadata {
        game: Game::Ats,
        map: None,
        game_version: None,
        archives: None,
    };
    let mut bytes = Vec::new();
    write_navigation_export(&mut bytes, &input, &meta).expect("export");

    let good = dir.join("good.jsonl");
    std::fs::write(&good, &bytes).expect("write");
    let o = run(&["check-navigation-export", &good.to_string_lossy()]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(out.contains("Valid navigation graph export: game ats, schema 1.0"));
    assert!(out.contains("Nodes: 0, edges: 0, issues: 0"));

    // Cut off: no footer.
    let cut = dir.join("cut.jsonl");
    std::fs::write(&cut, &bytes[..bytes.len() - 20]).expect("write");
    let o = run(&["check-navigation-export", &cut.to_string_lossy()]);
    assert_eq!(o.status.code(), Some(4));
    assert!(stderr(&o).contains("error:"));

    // Unsupported version.
    let text = String::from_utf8(bytes)
        .expect("UTF-8")
        .replacen("\"major\":1", "\"major\":9", 1);
    let v9 = dir.join("v9.jsonl");
    std::fs::write(&v9, text).expect("write");
    let o = run(&["check-navigation-export", &v9.to_string_lossy()]);
    assert_eq!(o.status.code(), Some(4));
    assert!(stderr(&o).contains("unsupported schema version 9"));

    // Missing file.
    let o = run(&[
        "check-navigation-export",
        &dir.join("missing.jsonl").to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}
