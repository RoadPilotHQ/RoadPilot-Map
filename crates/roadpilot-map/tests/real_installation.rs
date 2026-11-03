//! Opt-in tests: build the road and lane model of a whole local ETS2 or ATS
//! map.
//!
//! Ignored by default and never run in CI. Reads the archives read-only,
//! keeps everything in memory, checks properties only and prints counts
//! only.
//!
//! ```powershell
//! $env:ROADPILOT_MAP_GAME_DIR = "C:\Program Files (x86)\Steam\steamapps\common\American Truck Simulator"
//! cargo test --release -p roadpilot-map --test real_installation -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use roadpilot_map::formats::sii::{SiiUnit, parse_sii};
use roadpilot_map::formats::{ItemData, parse_base};
use roadpilot_map::model::{
    LaneConfig, LaneDirection, LaneTransition, LaneType, NodeLink, OffsetsState, RoadId, RoadSide,
    TrafficSideState,
};
use roadpilot_map::{LaneDefinitions, build_lane_network, build_road_network};
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

/// The definition units the lane model needs: every `road_look*` and
/// `traffic_lane*` file in `def/world` and `def/country.sii` of every
/// archive, with `@include` resolved relative to the including file.
fn lane_definition_units(dir: &PathBuf) -> Vec<SiiUnit> {
    let mut archives: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("installation folder is readable")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("scs")))
        .collect();
    archives.sort();
    let mut units = Vec::new();
    for path in &archives {
        let Ok(archive) = Archive::open(path) else {
            continue;
        };
        let mut files = Vec::new();
        if let Ok(children) = archive.list_dir("def/world") {
            files.extend(
                children
                    .iter()
                    .filter(|c| !c.is_dir && c.name.ends_with(".sii"))
                    .filter(|c| {
                        c.name.starts_with("road_look") || c.name.starts_with("traffic_lane")
                    })
                    .map(|c| format!("def/world/{}", c.name)),
            );
        }
        if archive.find("def/country.sii").ok().flatten().is_some() {
            files.push("def/country.sii".to_owned());
        }
        for file in files {
            let bytes = archive
                .read_file(&file)
                .expect("definition file is readable");
            let base = file.rsplit_once('/').map_or("", |(d, _)| d).to_owned();
            let mut include = |p: &str| {
                let full = p
                    .strip_prefix('/')
                    .map_or_else(|| format!("{base}/{p}"), str::to_owned);
                archive.read_file(&full).ok()
            };
            units.extend(parse_sii(&bytes, &mut include).expect("definition file parses"));
        }
    }
    units
}

#[test]
#[ignore = "reads a local game installation; set ROADPILOT_MAP_GAME_DIR and run with --ignored"]
fn builds_the_lane_network_of_the_installation() {
    let dir = game_dir();
    let files = sector_files(&dir);
    let sectors: Vec<_> = files
        .iter()
        .map(|f| parse_base(f).expect("sector parses"))
        .collect();
    let units = lane_definition_units(&dir);
    let defs = LaneDefinitions::from_units(&units).expect("definitions are consistent");
    let network = build_road_network(&sectors).expect("road network builds");
    let lanes = build_lane_network(&network, &sectors, &defs).expect("lane network builds");

    // Reproducible: a second build gives the same result.
    assert_eq!(
        build_lane_network(&network, &sectors, &defs).expect("lane network builds"),
        lanes
    );

    // The road look of each road, looked up independently of the builder.
    let looks: BTreeMap<&str, &SiiUnit> = units
        .iter()
        .filter(|u| u.class == "road_look")
        .map(|u| (u.name.as_str(), u))
        .collect();

    let mut stats: BTreeMap<String, usize> = BTreeMap::new();
    let mut count = |key: String| *stats.entry(key).or_default() += 1;
    for entry in lanes.roads() {
        let look = looks
            .get(entry.look.as_str())
            .expect("every road look resolves");
        match &entry.config {
            LaneConfig::Unresolved(issue) => count(format!("lanes unresolved: {issue:?}")),
            LaneConfig::Resolved(list) => {
                count("lanes resolved".to_owned());
                // Count and order match the look; indices are 0..n per side.
                for (side, key) in [
                    (RoadSide::Left, "lanes_left"),
                    (RoadSide::Right, "lanes_right"),
                ] {
                    let on_side: Vec<_> = list.iter().filter(|l| l.id.side == side).collect();
                    assert_eq!(on_side.len(), look.values(key).len());
                    for (i, lane) in on_side.iter().enumerate() {
                        assert_eq!(usize::from(lane.id.index), i);
                        let (LaneType::Known(name) | LaneType::Uncertain(name)) = &lane.lane_type;
                        assert_eq!(name, &look.values(key)[i]);
                        assert_eq!(lane.width, None, "no lane width is invented");
                    }
                }
                let left = list.iter().filter(|l| l.id.side == RoadSide::Left).count();
                let right = list.len() - left;
                count(format!("lane counts (left, right) = ({left}, {right})"));
                if list
                    .iter()
                    .any(|l| matches!(l.lane_type, LaneType::Uncertain(_)))
                {
                    count("lane types uncertain (variants)".to_owned());
                }
            }
        }
        match entry.traffic_side {
            TrafficSideState::Known(side) => count(format!("traffic side {side:?}")),
            TrafficSideState::Unknown(issue) => count(format!("traffic side unknown: {issue:?}")),
        }
        for state in [entry.offsets_left, entry.offsets_right] {
            if let OffsetsState::CountMismatch { .. } = state {
                count("offset count differs from lane count".to_owned());
            }
        }
    }

    // Candidates never connect other roads or opposite driving directions.
    let road_of = |id: RoadId| lanes.road(id).expect("road has a lane entry");
    for connection in network.connections() {
        match lanes.transition(&connection) {
            LaneTransition::IndexCandidates(pairs) => {
                count("road connections with lane candidates".to_owned());
                for pair in pairs {
                    let d_from = road_of(pair.from.road)
                        .direction(pair.from.side)
                        .expect("direction is known");
                    let d_to = road_of(pair.to.road)
                        .direction(pair.to.side)
                        .expect("direction is known");
                    assert_eq!(d_from, d_to, "a candidate keeps the driving direction");
                    let expected = if d_from == LaneDirection::Forward {
                        (connection.from, connection.to)
                    } else {
                        (connection.to, connection.from)
                    };
                    assert_eq!((pair.from.road, pair.to.road), expected);
                }
            }
            LaneTransition::Unknown(issue) => {
                count(format!("road connections without lane pairing: {issue:?}"));
            }
        }
    }

    let (looks_n, variants_n, lane_types_n, countries_n) = defs.counts();
    eprintln!(
        "definitions: {looks_n} road looks, {variants_n} variants, {lane_types_n} lane types, {countries_n} countries"
    );
    for (key, value) in &stats {
        eprintln!("{value:>8}  {key}");
    }
    assert!(
        !stats
            .keys()
            .any(|k| k.starts_with("lanes unresolved: Missing")),
        "every look, variant and lane type resolves"
    );
}
