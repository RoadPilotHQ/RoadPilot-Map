//! Opt-in test against a local ETS2 or ATS installation.
//!
//! Ignored by default and never run in CI. It reads the game archives
//! read-only, parses every sector `.base` and `.data` file in memory, and
//! checks properties only. Nothing is written and no game data is printed.
//!
//! ```powershell
//! $env:ROADPILOT_MAP_GAME_DIR = "C:\Program Files (x86)\Steam\steamapps\common\Euro Truck Simulator 2"
//! cargo test -p roadpilot-map-formats --test real_installation -- --ignored --nocapture
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

use roadpilot_map_archive::Archive;
use roadpilot_map_formats::{ItemData, ItemType, parse_base, parse_data};

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

#[test]
#[ignore = "reads a local game installation; set ROADPILOT_MAP_GAME_DIR and run with --ignored"]
fn parses_every_sector_of_the_installation() {
    let dir = game_dir();
    let mut archives: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("installation folder is readable")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("scs")))
        .collect();
    archives.sort();

    let mut sectors = 0usize;
    let mut items = 0usize;
    let mut nodes: HashMap<u64, (u64, u64)> = HashMap::new();
    let mut roads = Vec::new();
    let mut data_entries = 0usize;
    let mut road_and_prefab_items = 0usize;

    for path in &archives {
        let Ok(archive) = Archive::open(path) else {
            continue;
        };
        for map in ["map/europe", "map/usa"] {
            let Ok(children) = archive.list_dir(map) else {
                continue;
            };
            for child in children.iter().filter(|c| c.name.ends_with(".base")) {
                let stem = child.name.trim_end_matches(".base");
                let base_bytes = archive
                    .read_file(&format!("{map}/{stem}.base"))
                    .expect("base file is readable");
                let data_bytes = archive
                    .read_file(&format!("{map}/{stem}.data"))
                    .expect("data file is readable");
                let base = parse_base(&base_bytes)
                    .unwrap_or_else(|e| panic!("{} {map}/{stem}.base: {e}", path.display()));
                let data = parse_data(&data_bytes, &base)
                    .unwrap_or_else(|e| panic!("{} {map}/{stem}.data: {e}", path.display()));
                sectors += 1;
                items += base.items.len();
                road_and_prefab_items += base
                    .items
                    .iter()
                    .filter(|i| matches!(i.item_type, ItemType::ROAD | ItemType::PREFAB))
                    .count();
                data_entries += data.entries.len();
                for n in &base.nodes {
                    nodes.insert(n.uid, (n.backward_item, n.forward_item));
                }
                for item in &base.items {
                    if let ItemData::Road(road) = &item.data {
                        roads.push((item.uid, road.start_node, road.end_node));
                        assert!(road.look.name().is_some(), "road look is a valid token");
                    }
                    if let ItemData::Prefab(prefab) = &item.data {
                        assert!(
                            prefab.model.name().is_some(),
                            "prefab model is a valid token"
                        );
                        assert!(!prefab.nodes.is_empty(), "a prefab has nodes");
                    }
                }
            }
        }
    }

    assert!(sectors > 0, "no sector files found");
    assert!(items > 0 && !roads.is_empty());
    assert_eq!(data_entries, road_and_prefab_items);
    // The road/node links in both directions (nodes may be in another sector).
    for (road, start, end) in &roads {
        assert_eq!(
            nodes.get(start).map(|n| n.1),
            Some(*road),
            "start node links forward"
        );
        assert_eq!(
            nodes.get(end).map(|n| n.0),
            Some(*road),
            "end node links backward"
        );
    }
    eprintln!(
        "parsed {sectors} sectors, {items} items, {} roads, {} unique nodes, {data_entries} data entries",
        roads.len(),
        nodes.len()
    );
}
