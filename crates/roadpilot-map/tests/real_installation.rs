//! Opt-in test: build the road model of a whole local ETS2 or ATS map.
//!
//! Ignored by default and never run in CI. Reads the archives read-only,
//! keeps everything in memory, checks properties only and prints counts
//! only.
//!
//! ```powershell
//! $env:ROADPILOT_MAP_GAME_DIR = "C:\Program Files (x86)\Steam\steamapps\common\American Truck Simulator"
//! cargo test --release -p roadpilot-map --test real_installation -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use roadpilot_map::build_road_network;
use roadpilot_map::formats::{ItemData, parse_base};
use roadpilot_map::model::NodeLink;
use roadpilot_map_archive::Archive;

fn game_dir() -> PathBuf {
    let dir = std::env::var_os("ROADPILOT_MAP_GAME_DIR")
        .map(PathBuf::from)
        .expect(
            "set ROADPILOT_MAP_GAME_DIR to an ETS2 or ATS installation folder to run this test",
        );
    assert!(
        dir.join("base_map.scs").is_file(),
        "ROADPILOT_MAP_GAME_DIR ({}) does not contain base_map.scs",
        dir.display()
    );
    dir
}

/// The `.base` files of every map sector, read into memory.
fn sector_files(dir: &PathBuf) -> Vec<Vec<u8>> {
    let mut archives: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("installation folder is readable")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("scs")))
        .collect();
    archives.sort();
    let mut files = Vec::new();
    for path in &archives {
        let Ok(archive) = Archive::open(path) else {
            continue;
        };
        for map in ["map/europe", "map/usa"] {
            let Ok(children) = archive.list_dir(map) else {
                continue;
            };
            for child in children.iter().filter(|c| c.name.ends_with(".base")) {
                files.push(
                    archive
                        .read_file(&format!("{map}/{}", child.name))
                        .expect("sector file is readable"),
                );
            }
        }
    }
    files
}

#[test]
#[ignore = "reads a local game installation; set ROADPILOT_MAP_GAME_DIR and run with --ignored"]
fn builds_the_road_network_of_the_installation() {
    let files = sector_files(&game_dir());
    let sectors: Vec<_> = files
        .iter()
        .map(|f| parse_base(f).expect("sector parses"))
        .collect();
    let road_records: Vec<_> = sectors
        .iter()
        .flat_map(|s| &s.items)
        .filter_map(|i| match &i.data {
            ItemData::Road(r) => Some((i.uid, *r)),
            _ => None,
        })
        .collect();

    let net = build_road_network(&sectors).expect("the whole map builds and validates");

    // Every road record is in the model, with its references unchanged.
    assert!(!road_records.is_empty());
    assert_eq!(net.roads().count(), road_records.len());
    for (uid, record) in &road_records {
        let road = net
            .road(roadpilot_map::model::RoadId(*uid))
            .expect("road is in the model");
        assert_eq!(road.start_node.0, record.start_node);
        assert_eq!(road.end_node.0, record.end_node);
        assert_eq!(road.stored_length.to_bits(), record.length.to_bits());
    }

    // Connections are consistent in both directions.
    let mut connections = 0usize;
    for c in net.connections() {
        assert_eq!(net.predecessor(c.to), Some(c));
        let roads = net.roads_at(c.via).expect("node exists");
        assert_eq!((roads.ending, roads.starting), (Some(c.from), Some(c.to)));
        connections += 1;
    }
    let shared = net
        .nodes()
        .filter(|n| {
            matches!(
                (n.backward, n.forward),
                (NodeLink::Road(_), NodeLink::Road(_))
            )
        })
        .count();
    assert_eq!(connections, shared);

    // Geometry values are finite; the stored length is positive.
    for road in net.roads() {
        let g = net.geometry(road.id).expect("geometry");
        assert!(g.chord_length.is_finite() && g.stored_length > 0.0);
        let d = g.start_direction;
        assert!(((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) - 1.0).abs() < 1e-6);
    }

    let unresolved = net
        .nodes()
        .flat_map(|n| [n.backward, n.forward])
        .filter(|l| matches!(l, NodeLink::Unresolved(_)))
        .count();
    eprintln!(
        "{} sectors, {} roads, {} nodes, {connections} road-to-road connections, {unresolved} unresolved node links",
        sectors.len(),
        road_records.len(),
        net.nodes().count()
    );
}
