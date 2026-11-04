//! Tests of map package creation and checking on hand-built models (no game
//! data).

use std::fs;
use std::path::{Path, PathBuf};

use roadpilot_map::export::{self, ExportInput, ExportMetadata};
use roadpilot_map::geometry::RoadCurve;
use roadpilot_map::map_package::{
    self as pkg, ConnectionStatus, CurveRecord, LanesRecord, MapPackage, Quality, TrafficSideRecord,
};
use roadpilot_map::model::{
    Lane, LaneConfig, LaneId, LaneNetwork, LaneType, NavigationGraph, Node, NodeId, NodeLink,
    OffsetsState, PREFAB_LANE_SLOTS, Position, PrefabCurve, PrefabDefinition, PrefabDefinitionNode,
    PrefabId, PrefabInstance, PrefabNetwork, PrefabResolution, Road, RoadId, RoadLanes,
    RoadNetwork, RoadSide, Rotation, TrafficSide, TrafficSideIssue, TrafficSideState,
};
use roadpilot_map::{
    Game, InstallationModel, PackageCheckError, PackageOptions, PackageWriteError, check_package,
    write_package,
};

// ---- Fixtures (invented, not game data) ----

fn node(id: u64, z: i32, backward: NodeLink, forward: NodeLink) -> Node {
    Node {
        id: NodeId(id),
        position: Position {
            fixed: [id as i32 * 256, 512, z * 256],
        },
        rotation: Rotation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        backward,
        forward,
    }
}

fn road(id: u64, start: u64, end: u64) -> Road {
    Road {
        id: RoadId(id),
        start_node: NodeId(start),
        end_node: NodeId(end),
        look_token: 0,
        stored_length: 10.0,
    }
}

fn slots(curve: Option<u16>) -> [Option<u16>; PREFAB_LANE_SLOTS] {
    let mut s = [None; PREFAB_LANE_SLOTS];
    s[0] = curve;
    s
}

fn definition() -> PrefabDefinition {
    let curve = |next: Vec<u16>, prev: Vec<u16>| PrefabCurve {
        next,
        prev,
        traffic_rule: None,
    };
    PrefabDefinition {
        model: "prefab.x".to_owned(),
        nodes: vec![
            PrefabDefinitionNode {
                input_slots: slots(Some(0)),
                output_slots: slots(Some(3)),
            },
            PrefabDefinitionNode {
                input_slots: slots(Some(2)),
                output_slots: slots(Some(1)),
            },
        ],
        curves: vec![
            curve(vec![1], vec![]),
            curve(vec![], vec![0]),
            curve(vec![3], vec![]),
            curve(vec![], vec![2]),
        ],
        semaphores: Vec::new(),
    }
}

fn road_lanes(road: u64) -> RoadLanes {
    let lane = |side, offset| Lane {
        id: LaneId {
            road: RoadId(road),
            side,
            index: 0,
        },
        lane_type: LaneType::Known("traffic_lane.a".to_owned()),
        offset,
        width: None,
    };
    RoadLanes {
        road: RoadId(road),
        look: "road.test".to_owned(),
        traffic_side: TrafficSideState::Known(TrafficSide::Right),
        config: LaneConfig::Resolved(vec![
            lane(RoadSide::Left, None),
            lane(
                RoadSide::Right,
                Some(roadpilot_map::model::StoredLaneOffset {
                    first: 0.5,
                    second: 1.5,
                }),
            ),
        ]),
        offsets_left: OffsetsState::NotStored,
        offsets_right: OffsetsState::Assigned,
        stored_size_left: None,
        stored_size_right: None,
    }
}

/// Road 10 -> prefab 100 -> road 20 -> road 22; road 30 alone and with an
/// unknown traffic side.
fn model() -> InstallationModel {
    let r = |id| NodeLink::Road(RoadId(id));
    let p = NodeLink::Item(100);
    let network = RoadNetwork::new(
        [
            node(11, 10, NodeLink::None, r(10)),
            node(1, 0, r(10), p),
            node(2, -20, p, r(20)),
            node(21, -30, r(20), r(22)),
            node(23, -40, r(22), NodeLink::None),
            node(41, 100, NodeLink::None, r(30)),
            node(42, 90, r(30), NodeLink::None),
        ],
        [
            road(10, 11, 1),
            road(20, 2, 21),
            road(22, 21, 23),
            road(30, 41, 42),
        ],
    )
    .expect("roads");
    let unknown_side = RoadLanes {
        traffic_side: TrafficSideState::Unknown(TrafficSideIssue::NoCountry),
        ..road_lanes(30)
    };
    let lanes = LaneNetwork::new(
        &network,
        [road_lanes(10), road_lanes(20), road_lanes(22), unknown_side],
    )
    .expect("lanes");
    let prefabs = PrefabNetwork::new(
        &network,
        [definition()],
        [PrefabInstance {
            id: PrefabId(100),
            model: "prefab.x".to_owned(),
            nodes: vec![NodeId(1), NodeId(2)],
            origin: 0,
            resolution: PrefabResolution::Resolved,
        }],
    )
    .expect("prefabs");
    let graph = NavigationGraph::build(&network, &lanes, &prefabs).expect("graph");
    InstallationModel {
        game: Game::Ets2,
        network,
        lanes,
        prefabs,
        graph,
        archives: vec!["base_map.scs".to_owned()],
        skipped_archives: 0,
    }
}

fn meta() -> ExportMetadata {
    ExportMetadata {
        game: Game::Ets2,
        map: Some("europe".to_owned()),
        game_version: None,
        archives: Some(vec!["base_map.scs".to_owned()]),
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "roadpilot-map-package-facade-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn build(dir: &Path, options: PackageOptions) -> roadpilot_map::PackageStats {
    write_package(dir, None, &model(), &meta(), options).expect("package")
}

// ---- Tests ----

#[test]
fn a_written_package_opens_and_checks() {
    let root = temp_dir("roundtrip");
    let out = root.join("pkg");
    let stats = build(&out, PackageOptions::default());
    assert_eq!(stats.counts.nodes, 7);
    assert_eq!(stats.counts.roads, 4);
    assert_eq!(stats.counts.lanes, 8);
    assert_eq!(stats.counts.junctions, 1);
    assert!(stats.navigation_bytes.is_some());

    let summary = check_package(&out).expect("valid");
    assert_eq!(summary.manifest.counts, stats.counts);
    assert_eq!(summary.manifest.content_id(), stats.content_id);
    let nav = summary.navigation.expect("navigation checked");
    assert_eq!(nav.header.nodes, model().graph.node_count());
    assert!(!out.with_extension("partial").exists());
    assert!(!root.join("pkg.partial").exists());
    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn the_manifest_states_what_is_known_and_what_is_not() {
    let root = temp_dir("manifest");
    let out = root.join("pkg");
    build(&out, PackageOptions::default());
    let text = fs::read_to_string(out.join("manifest.json")).expect("manifest");
    for expected in [
        "\"format\":\"roadpilot-map-package\"",
        "\"generated_at\":null",
        "\"game_version\":null",
        "\"world_unit\":\"unconfirmed\"",
        "\"node_position\":\"confirmed\"",
        "\"quality\":\"estimated\"",
        "\"lane_position\":\"unavailable\"",
        "\"distribution\":\"local_use_only\"",
        "lane_positions_are_unavailable",
        "road_curve_is_estimated",
    ] {
        assert!(text.contains(expected), "missing {expected}");
    }
    // The navigation file still has no curves; its limitation is not copied
    // into the package's own list as if it were fixed.
    assert!(!text.contains("curve_geometry_is_not_exported"));
    // Every limitation code appears once, and the navigation file's codes are
    // present when it is included.
    let manifest = pkg::read_manifest(&out).expect("manifest");
    let mut codes = manifest.limitations.clone();
    codes.sort();
    codes.dedup();
    assert_eq!(codes.len(), manifest.limitations.len());
    assert!(
        manifest
            .limitations
            .iter()
            .any(|l| l == "graph_is_not_one_connected_network")
    );
    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn records_follow_the_models_and_keep_unknowns_unknown() {
    let root = temp_dir("records");
    let out = root.join("pkg");
    build(&out, PackageOptions::default());
    let package = MapPackage::open(&out).expect("opens");

    // Lanes: no lane has a position, whatever the lane data says.
    for road in &package.data().roads {
        if let LanesRecord::Resolved(lanes) = &road.lanes {
            assert!(lanes.iter().all(|l| l.geometry == Quality::Unavailable));
        }
    }
    // The stored offset of road 10 is kept as stored.
    let r10 = package.road(10).expect("road 10");
    let LanesRecord::Resolved(lanes) = &r10.lanes else {
        panic!("lanes must be resolved");
    };
    assert_eq!(lanes[1].offset, Some([0.5, 1.5]));
    assert_eq!(lanes[0].offset, None);
    assert_eq!(r10.traffic_side, TrafficSideRecord::Right);

    // Road 30 has an unknown traffic side: no lane direction is invented.
    let r30 = package.road(30).expect("road 30");
    assert_eq!(
        r30.traffic_side,
        TrafficSideRecord::Unknown("no_country".to_owned())
    );
    let LanesRecord::Resolved(lanes) = &r30.lanes else {
        panic!("lanes must be resolved");
    };
    assert!(lanes.iter().all(|l| l.direction.is_none()));

    // Connections: 20 -> 22 is a candidate with lane pairs, never confirmed.
    let c = package
        .data()
        .connections
        .iter()
        .find(|c| c.from == 20)
        .expect("connection");
    assert_eq!(c.status, ConnectionStatus::Candidate);
    assert!(!c.pairs.is_empty());
    assert!(
        package
            .data()
            .connections
            .iter()
            .all(|c| c.status != ConnectionStatus::Confirmed)
    );

    // The junction keeps its confirmed internal movements and its arms.
    let j = &package.data().junctions[0];
    assert_eq!(j.id, 100);
    assert!(j.unresolved.is_none());
    assert_eq!(j.nodes, vec![1, 2]);
    assert!(!j.movements.is_empty());
    assert!(
        j.movements
            .iter()
            .all(|m| m.status == ConnectionStatus::Confirmed)
    );
    assert_eq!(j.arms.len(), 2);
    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn road_curves_are_the_estimate_of_the_geometry_crate() {
    let root = temp_dir("curves");
    let out = root.join("pkg");
    let stats = build(&out, PackageOptions::default());
    let m = model();
    let package = MapPackage::open(&out).expect("opens");
    assert_eq!(stats.roads_with_curve, 4);
    for road in m.network.roads() {
        let curve =
            RoadCurve::estimate(&m.network.geometry(road.id).expect("geometry")).expect("estimate");
        let record = package.road(road.id.0).expect("record");
        let CurveRecord::Estimated {
            estimated_length,
            length_consistent,
            bounds,
            tangent_scale,
            ..
        } = record.curve
        else {
            panic!("curve must be estimated");
        };
        assert_eq!(estimated_length, curve.estimated_length());
        assert_eq!(length_consistent, curve.length_agreement().is_consistent());
        assert_eq!(tangent_scale, f64::from(road.stored_length));
        // The box contains the end points, which are confirmed values.
        let (s, e) = (curve.start(), curve.end());
        for p in [s, e] {
            assert!(p[0] >= bounds[0] && p[0] <= bounds[3]);
            assert!(p[2] >= bounds[2] && p[2] <= bounds[5]);
        }
    }
    // The invented roads are about 2 % longer than their stored length, so
    // the model must flag them instead of calling them consistent.
    assert!(stats.roads_length_consistent < stats.roads_with_curve);
    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn spatial_queries_find_roads_near_a_position() {
    let root = temp_dir("spatial");
    let out = root.join("pkg");
    build(&out, PackageOptions::default());
    let package = MapPackage::open(&out).expect("opens");
    // Road 30 lies around x = 41..42, z = 90..100.
    let hits = package.roads_near(41.5, 95.0, 2.0);
    assert_eq!(hits.iter().map(|h| h.road.id).collect::<Vec<_>>(), vec![30]);
    assert!(package.roads_near(5000.0, 5000.0, 50.0).is_empty());
    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn output_is_deterministic() {
    let root = temp_dir("determinism");
    let (a, b) = (root.join("a"), root.join("b"));
    let sa = build(&a, PackageOptions::default());
    let sb = build(&b, PackageOptions::default());
    assert_eq!(sa, sb);
    for f in ["manifest.json", "geometry.jsonl", "navigation.jsonl"] {
        assert_eq!(
            fs::read(a.join(f)).expect("a"),
            fs::read(b.join(f)).expect("b"),
            "{f}"
        );
    }
    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn the_navigation_file_is_optional() {
    let root = temp_dir("nonav");
    let out = root.join("pkg");
    let stats = build(
        &out,
        PackageOptions {
            include_navigation: false,
            overwrite: false,
        },
    );
    assert!(stats.navigation_bytes.is_none());
    assert!(!out.join("navigation.jsonl").exists());
    let summary = check_package(&out).expect("valid");
    assert!(summary.navigation.is_none());
    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn existing_output_is_protected() {
    let root = temp_dir("policy");
    let out = root.join("pkg");
    build(&out, PackageOptions::default());
    let before = fs::read(out.join("manifest.json")).expect("manifest");

    let err = write_package(&out, None, &model(), &meta(), PackageOptions::default())
        .expect_err("exists");
    assert!(matches!(err, PackageWriteError::OutputExists(_)));
    assert_eq!(
        fs::read(out.join("manifest.json")).expect("manifest"),
        before
    );

    // Overwriting replaces a package.
    let overwrite = PackageOptions {
        include_navigation: false,
        overwrite: true,
    };
    write_package(&out, None, &model(), &meta(), overwrite).expect("replaced");
    assert!(!out.join("navigation.jsonl").exists());
    assert!(check_package(&out).is_ok());

    // A folder with other files is never replaced.
    let foreign = root.join("mine");
    fs::create_dir(&foreign).expect("dir");
    fs::write(foreign.join("notes.txt"), "keep me").expect("file");
    let err = write_package(&foreign, None, &model(), &meta(), overwrite).expect_err("foreign");
    assert!(matches!(err, PackageWriteError::InvalidOutput(_)));
    assert_eq!(
        fs::read_to_string(foreign.join("notes.txt")).expect("kept"),
        "keep me"
    );

    // A plain file is not a package.
    let file = root.join("file");
    fs::write(&file, "x").expect("file");
    assert!(matches!(
        write_package(&file, None, &model(), &meta(), overwrite),
        Err(PackageWriteError::InvalidOutput(_))
    ));

    // A leftover partial folder is never overwritten.
    fs::create_dir(root.join("again.partial")).expect("dir");
    assert!(matches!(
        write_package(
            &root.join("again"),
            None,
            &model(),
            &meta(),
            PackageOptions::default()
        ),
        Err(PackageWriteError::InvalidOutput(_))
    ));

    // A missing parent folder and the game folder are refused.
    assert!(matches!(
        write_package(
            &root.join("no").join("pkg"),
            None,
            &model(),
            &meta(),
            PackageOptions::default()
        ),
        Err(PackageWriteError::InvalidOutput(_))
    ));
    let game = root.join("game");
    fs::create_dir(&game).expect("dir");
    assert!(matches!(
        write_package(
            &game.join("pkg"),
            Some(&game),
            &model(),
            &meta(),
            PackageOptions::default()
        ),
        Err(PackageWriteError::InvalidOutput(_))
    ));
    assert!(!game.join("pkg").exists() && !game.join("pkg.partial").exists());
    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn damaged_navigation_files_are_detected() {
    let root = temp_dir("damaged");
    let out = root.join("pkg");
    build(&out, PackageOptions::default());
    let nav = out.join("navigation.jsonl");
    let mut bytes = fs::read(&nav).expect("nav");
    let last = bytes.len() - 2;
    bytes[last] ^= 1;
    fs::write(&nav, bytes).expect("write");
    assert!(matches!(
        check_package(&out),
        Err(PackageCheckError::Package(
            pkg::PackageError::FileMismatch { .. }
        ))
    ));
    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn a_navigation_file_of_other_data_is_a_mismatch() {
    // Build a package without navigation, then add the navigation file of a
    // different map (it names a map node the geometry file does not have)
    // with a correct manifest entry. Only the cross-check can catch this.
    let root = temp_dir("mismatch");
    let out = root.join("pkg");
    build(
        &out,
        PackageOptions {
            include_navigation: false,
            overwrite: false,
        },
    );
    let other = {
        let mut m = model();
        let p = NodeLink::Item(100);
        let network = RoadNetwork::new(
            [
                node(1, 0, NodeLink::None, p),
                node(2, -20, p, NodeLink::Item(101)),
                node(999, -40, NodeLink::Item(101), NodeLink::None),
            ],
            [],
        )
        .expect("roads");
        let lanes = LaneNetwork::new(&network, []).expect("lanes");
        let instance = |id, nodes: [u64; 2]| PrefabInstance {
            id: PrefabId(id),
            model: "prefab.x".to_owned(),
            nodes: nodes.map(NodeId).to_vec(),
            origin: 0,
            resolution: PrefabResolution::Resolved,
        };
        let prefabs = PrefabNetwork::new(
            &network,
            [definition()],
            [instance(100, [1, 2]), instance(101, [2, 999])],
        )
        .expect("prefabs");
        m.graph = NavigationGraph::build(&network, &lanes, &prefabs).expect("graph");
        m.network = network;
        m.lanes = lanes;
        m.prefabs = prefabs;
        m
    };
    let mut bytes = Vec::new();
    export::write_navigation_export(&mut bytes, &ExportInput::from_installation(&other), &meta())
        .expect("nav");
    fs::write(out.join("navigation.jsonl"), &bytes).expect("nav file");

    let mut manifest = pkg::read_manifest(&out).expect("manifest");
    manifest.files.push(pkg::FileEntry {
        name: "navigation.jsonl".to_owned(),
        role: "navigation".to_owned(),
        format: export::FORMAT_NAME.to_owned(),
        schema_major: export::SCHEMA_MAJOR,
        schema_minor: export::SCHEMA_MINOR,
        bytes: bytes.len() as u64,
        checksum: pkg::checksum(&bytes),
    });
    fs::write(out.join("manifest.json"), manifest.to_json().expect("json")).expect("manifest");

    let err = check_package(&out).expect_err("mismatch");
    assert!(
        matches!(&err, PackageCheckError::Mismatch(m) if m.contains("map node")),
        "{err}"
    );
    fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn an_empty_installation_gives_a_valid_empty_package() {
    let network = RoadNetwork::new([], []).expect("roads");
    let lanes = LaneNetwork::new(&network, []).expect("lanes");
    let prefabs = PrefabNetwork::new(&network, [], []).expect("prefabs");
    let graph = roadpilot_map::model::NavigationGraphBuilder::new()
        .finish()
        .expect("graph");
    let empty = InstallationModel {
        game: Game::Ats,
        network,
        lanes,
        prefabs,
        graph,
        archives: Vec::new(),
        skipped_archives: 0,
    };
    let root = temp_dir("empty");
    let out = root.join("pkg");
    let m = ExportMetadata {
        game: Game::Ats,
        map: None,
        game_version: None,
        archives: None,
    };
    let stats = write_package(&out, None, &empty, &m, PackageOptions::default()).expect("package");
    assert_eq!(stats.counts, pkg::Counts::default());
    let summary = check_package(&out).expect("valid");
    assert_eq!(summary.manifest.map, None);
    assert_eq!(summary.manifest.archives, None);
    fs::remove_dir_all(&root).expect("cleanup");
}
