//! Tests of the package format on synthetic data.
//!
//! The fixture is invented: a few nodes, two joined roads and a junction.
//! No game data is used.

use std::io::Cursor;
use std::path::PathBuf;

use roadpilot_map_package::*;

const ROT: [f32; 4] = [1.0, 0.0, 0.0, 0.0];

fn node(id: u64, z_world: i32, backward: LinkRecord, forward: LinkRecord) -> NodeRecord {
    NodeRecord {
        id,
        position: [0, 0, z_world * 256],
        rotation: ROT,
        backward,
        forward,
    }
}

fn lane(side: Side, index: u16, direction: Option<LaneDirectionRecord>) -> LaneRecord {
    LaneRecord {
        side,
        index,
        lane_type: "traffic_lane.road".to_owned(),
        type_certain: index == 0,
        direction,
        offset: (index == 0).then_some([0.5, 1.5]),
        geometry: Quality::Unavailable,
    }
}

fn curve(min_z: f64, max_z: f64) -> CurveRecord {
    CurveRecord::Estimated {
        tangent_scale: 10.0,
        estimated_length: 10.0,
        length_deviation: 0.0,
        length_consistent: true,
        bounds: [0.0, 0.0, min_z, 0.0, 0.0, max_z],
    }
}

fn road(id: u64, start: u64, end: u64, curve: CurveRecord) -> RoadRecord {
    RoadRecord {
        id,
        start,
        end,
        look: "road.test".to_owned(),
        stored_length: 10.0,
        traffic_side: TrafficSideRecord::Right,
        lanes: LanesRecord::Resolved(vec![
            lane(Side::Left, 0, Some(LaneDirectionRecord::Backward)),
            lane(Side::Right, 0, Some(LaneDirectionRecord::Forward)),
            lane(Side::Right, 1, Some(LaneDirectionRecord::Forward)),
        ]),
        curve,
    }
}

struct Fixture {
    nodes: Vec<NodeRecord>,
    roads: Vec<RoadRecord>,
    junctions: Vec<JunctionRecord>,
    connections: Vec<ConnectionRecord>,
}

fn fixture() -> Fixture {
    Fixture {
        nodes: vec![
            node(0x1, 0, LinkRecord::None, LinkRecord::Road(0xa)),
            node(0x2, -10, LinkRecord::Road(0xa), LinkRecord::Road(0xb)),
            node(0x3, -20, LinkRecord::Road(0xb), LinkRecord::Item(0x100)),
        ],
        roads: vec![
            road(0xa, 0x1, 0x2, curve(-10.0, 0.0)),
            RoadRecord {
                traffic_side: TrafficSideRecord::Unknown("no_country".to_owned()),
                lanes: LanesRecord::Unresolved("missing_road_look".to_owned()),
                curve: CurveRecord::Unavailable("invalid_stored_length".to_owned()),
                ..road(0xb, 0x2, 0x3, curve(0.0, 0.0))
            },
        ],
        junctions: vec![JunctionRecord {
            id: 0x100,
            model: "prefab.test".to_owned(),
            unresolved: None,
            nodes: vec![0x3],
            arms: vec![ArmRecord {
                index: 0,
                node: 0x3,
                link: ArmLinkRecord::Road {
                    road: 0xb,
                    road_ends_here: true,
                },
            }],
            movements: vec![MovementRecord {
                from_arm: 0,
                from_slot: 0,
                to_arm: 0,
                to_slot: 1,
                status: ConnectionStatus::Confirmed,
            }],
        }],
        connections: vec![ConnectionRecord {
            from: 0xa,
            via: 0x2,
            to: 0xb,
            status: ConnectionStatus::Unknown,
            reason: Some("lanes_unresolved".to_owned()),
            pairs: vec![],
        }],
    }
}

fn write(f: &Fixture) -> (Vec<u8>, WrittenFile) {
    let mut out = Vec::new();
    let counts = GeometryCounts {
        nodes: f.nodes.len() as u64,
        roads: f.roads.len() as u64,
        junctions: f.junctions.len() as u64,
        connections: f.connections.len() as u64,
    };
    let info = write_geometry(
        &mut out,
        counts,
        f.nodes.clone(),
        f.roads.clone(),
        f.junctions.clone(),
        f.connections.clone(),
    )
    .expect("writes");
    (out, info)
}

fn text(f: &Fixture) -> String {
    String::from_utf8(write(f).0).expect("utf-8")
}

fn read(text: &str) -> Result<GeometryData, ReadError> {
    read_geometry(Cursor::new(text.as_bytes()), Some(0.001))
}

fn error_message(text: &str) -> String {
    read(text).expect_err("must be rejected").to_string()
}

#[test]
fn round_trips_every_record() {
    let f = fixture();
    let data = read(&text(&f)).expect("valid");
    assert_eq!(data.nodes, f.nodes);
    assert_eq!(data.roads, f.roads);
    assert_eq!(data.junctions, f.junctions);
    assert_eq!(data.connections, f.connections);
    assert_eq!(data.schema_minor, SCHEMA_MINOR);
}

#[test]
fn candidate_connections_round_trip_with_lane_pairs() {
    let mut f = fixture();
    f.roads[1] = road(0xb, 0x2, 0x3, curve(-20.0, -10.0));
    f.connections[0] = ConnectionRecord {
        from: 0xa,
        via: 0x2,
        to: 0xb,
        status: ConnectionStatus::Candidate,
        reason: None,
        pairs: vec![
            ((Side::Right, 0), (Side::Right, 0)),
            ((Side::Left, 0), (Side::Left, 0)),
        ],
    };
    let data = read(&text(&f)).expect("valid");
    assert_eq!(data.connections, f.connections);
}

#[test]
fn output_is_deterministic_and_has_no_time_stamp() {
    let f = fixture();
    let (a, info_a) = write(&f);
    let (b, info_b) = write(&f);
    assert_eq!(a, b);
    assert_eq!(info_a, info_b);
    assert_eq!(info_a.bytes, a.len() as u64);
    assert_eq!(info_a.checksum, checksum(&a));
    assert_eq!(info_a.lanes, 3);
    assert!(a.ends_with(b"}\n"));
}

#[test]
fn file_starts_with_header_and_ends_with_footer() {
    let t = text(&fixture());
    let lines: Vec<&str> = t.lines().collect();
    assert!(lines[0].starts_with("{\"t\":\"header\",\"format\":\"roadpilot-map-geometry\""));
    assert!(
        lines
            .last()
            .expect("lines")
            .starts_with("{\"t\":\"footer\",\"complete\":true")
    );
    // 1 header + 3 nodes + 2 roads + 1 junction + 1 connection + footer
    assert_eq!(lines.len(), 9);
}

#[test]
fn unknown_quality_is_never_written_as_a_value() {
    let t = text(&fixture());
    assert!(t.contains("\"width\":null"));
    assert!(t.contains("\"geometry\":\"unavailable\""));
    assert!(t.contains("\"quality\":\"unavailable\",\"reason\":\"invalid_stored_length\""));
    assert!(t.contains("\"traffic_side\":\"unknown\",\"traffic_side_reason\":\"no_country\""));
}

#[test]
fn rejects_a_missing_footer_and_empty_input() {
    let t = text(&fixture());
    let cut: String = t
        .lines()
        .take(t.lines().count() - 1)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(matches!(read(&cut), Err(ReadError::Incomplete(_))));
    assert!(matches!(read(""), Err(ReadError::Incomplete(_))));
}

#[test]
fn rejects_unsupported_major_versions_and_accepts_newer_minors() {
    let t = text(&fixture());
    let v2 = t.replacen("\"major\":1", "\"major\":2", 1);
    assert_eq!(read(&v2), Err(ReadError::UnsupportedVersion { major: 2 }));
    let minor = t.replacen("\"minor\":0", "\"minor\":7", 1);
    assert_eq!(read(&minor).expect("valid").schema_minor, 7);
}

#[test]
fn ignores_unknown_record_types_and_fields() {
    let t = text(&fixture());
    let with_record = t.replacen(
        "{\"t\":\"road\"",
        "{\"t\":\"future_record\",\"x\":1}\n{\"t\":\"road\"",
        1,
    );
    // The unknown record sits between sections without breaking the order.
    let data = read(&with_record).expect("valid");
    assert_eq!(data.ignored_records, 1);
    let with_field = t.replace("\"look\":", "\"future\":[1,2],\"look\":");
    assert!(read(&with_field).is_ok());
}

#[test]
fn rejects_wrong_formats_and_malformed_lines() {
    let t = text(&fixture());
    assert!(error_message(&t.replacen("roadpilot-map-geometry", "other", 1)).contains("line 1"));
    assert!(error_message("not json\n").contains("line 1"));
    assert!(error_message(&format!("{t}\n{{\"t\":\"node\"}}")).contains("data after the footer"));
    let broken = t.replacen("\"position\":[0,0,0]", "\"position\":[0,0]", 1);
    assert!(error_message(&broken).contains("position"));
    let long = format!(
        "{}{}",
        &t[..t.find('\n').expect("newline") + 1],
        "x".repeat(5 << 20)
    );
    assert!(error_message(&long).contains("too long"));
}

#[test]
fn rejects_count_mismatches() {
    let t = text(&fixture());
    let wrong_header = t.replacen("\"nodes\":3", "\"nodes\":4", 1);
    assert!(error_message(&wrong_header).contains("nodes"));
    let wrong_footer = t.replace(
        "\"roads\":2,\"junctions\":1,\"connections\":1}\n",
        "\"roads\":9,\"junctions\":1,\"connections\":1}\n",
    );
    assert!(read(&wrong_footer).is_err());
}

#[test]
fn rejects_dangling_references_and_unordered_ids() {
    let mut f = fixture();
    f.roads[0].end = 0x99;
    assert!(error_message(&text(&f)).contains("end node 99 does not exist"));

    let mut f = fixture();
    f.nodes.swap(0, 1);
    assert!(error_message(&text(&f)).contains("strictly ascending"));

    let mut f = fixture();
    f.roads[1].id = 0xa;
    assert!(error_message(&text(&f)).contains("strictly ascending"));

    let mut f = fixture();
    f.nodes[0].forward = LinkRecord::Road(0xb);
    assert!(
        read(&text(&f)).is_err(),
        "a node must name a road that starts there"
    );

    let mut f = fixture();
    f.roads[0].start = f.roads[0].end;
    assert!(error_message(&text(&f)).contains("same node"));
}

#[test]
fn rejects_invalid_lane_lists() {
    let mut f = fixture();
    if let LanesRecord::Resolved(l) = &mut f.roads[0].lanes {
        l[2].index = 5;
    }
    assert!(error_message(&text(&f)).contains("consecutive"));
    let mut f = fixture();
    if let LanesRecord::Resolved(l) = &mut f.roads[0].lanes {
        l.swap(0, 1);
    }
    assert!(error_message(&text(&f)).contains("left lanes must come before"));
}

#[test]
fn rejects_inconsistent_curves() {
    let mut f = fixture();
    f.roads[0].curve = CurveRecord::Estimated {
        tangent_scale: 10.0,
        estimated_length: 10.5,
        length_deviation: 0.05,
        length_consistent: true,
        bounds: [0.0; 6],
    };
    assert!(error_message(&text(&f)).contains("contradicts"));

    let mut f = fixture();
    f.roads[0].curve = CurveRecord::Estimated {
        tangent_scale: 10.0,
        estimated_length: 10.0,
        length_deviation: 0.0,
        length_consistent: true,
        bounds: [0.0, 0.0, 0.0, 99_999.0, 1.0, 1.0],
    };
    assert!(error_message(&text(&f)).contains("implausibly large"));

    let mut f = fixture();
    f.roads[0].curve = CurveRecord::Estimated {
        tangent_scale: 10.0,
        estimated_length: 10.0,
        length_deviation: 0.0,
        length_consistent: true,
        bounds: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    };
    assert!(error_message(&text(&f)).contains("minimum is above maximum"));
}

#[test]
fn curves_cannot_claim_to_be_confirmed() {
    let t = text(&fixture()).replacen("\"quality\":\"estimated\"", "\"quality\":\"confirmed\"", 1);
    assert!(error_message(&t).contains("never confirmed"));
}

#[test]
fn rejects_invalid_connections() {
    let mut f = fixture();
    f.connections[0].via = 0x3;
    assert!(error_message(&text(&f)).contains("connection's node"));

    let mut f = fixture();
    f.connections[0].pairs = vec![((Side::Right, 0), (Side::Right, 0))];
    assert!(error_message(&text(&f)).contains("no lane pairs"));

    let mut f = fixture();
    f.connections[0].status = ConnectionStatus::Candidate;
    f.connections[0].reason = None;
    assert!(error_message(&text(&f)).contains("needs lane pairs"));

    let mut f = fixture();
    f.connections[0].reason = None;
    assert!(error_message(&text(&f)).contains("needs a reason"));
}

#[test]
fn rejects_invalid_junctions() {
    let mut f = fixture();
    f.junctions[0].arms[0].link = ArmLinkRecord::Road {
        road: 0xa,
        road_ends_here: true,
    };
    assert!(error_message(&text(&f)).contains("does not end at the arm's node"));

    let mut f = fixture();
    f.junctions[0].movements[0].to_arm = 4;
    assert!(error_message(&text(&f)).contains("arm that does not exist"));

    let mut f = fixture();
    f.junctions[0].unresolved = Some("missing_model".to_owned());
    assert!(error_message(&text(&f)).contains("unresolved junction cannot have movements"));

    let mut f = fixture();
    f.junctions[0].nodes = vec![0x77];
    assert!(read(&text(&f)).is_err());
}

#[test]
fn writer_refuses_non_finite_values_and_wrong_counts() {
    let mut f = fixture();
    f.nodes[0].rotation[1] = f32::NAN;
    let mut out = Vec::new();
    let err = write_geometry(
        &mut out,
        GeometryCounts {
            nodes: 3,
            roads: 2,
            junctions: 1,
            connections: 1,
        },
        f.nodes.clone(),
        f.roads.clone(),
        f.junctions.clone(),
        f.connections.clone(),
    )
    .expect_err("NaN");
    assert!(matches!(err, WriteError::Invalid(_)));
    // The incomplete output has no footer, so no reader accepts it.
    assert!(read(&String::from_utf8_lossy(&out)).is_err());

    let f = fixture();
    let mut out = Vec::new();
    let err = write_geometry(
        &mut out,
        GeometryCounts {
            nodes: 99,
            roads: 2,
            junctions: 1,
            connections: 1,
        },
        f.nodes,
        f.roads,
        f.junctions,
        f.connections,
    )
    .expect_err("count");
    assert!(err.to_string().contains("nodes"));
}

#[test]
fn lane_ids_are_stable_text() {
    assert_eq!(
        lane_id(0x2524_9798_d200_0001, Side::Right, 0),
        "25249798d2000001:r0"
    );
    assert_eq!(lane_id(0xa, Side::Left, 12), "a:l12");
}

// --- Package folder -------------------------------------------------------

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "roadpilot-map-package-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn manifest_for(bytes: &[u8], f: &Fixture, info: &WrittenFile) -> Manifest {
    Manifest {
        schema_major: SCHEMA_MAJOR,
        schema_minor: SCHEMA_MINOR,
        content_revision: 1,
        generator_name: "test".to_owned(),
        generator_version: "0".to_owned(),
        game: "ets2".to_owned(),
        map: None,
        game_version: None,
        archives: None,
        counts: Counts {
            nodes: f.nodes.len() as u64,
            roads: f.roads.len() as u64,
            lanes: info.lanes,
            junctions: f.junctions.len() as u64,
            connections: f.connections.len() as u64,
        },
        curve_model: "hermite_stored_length".to_owned(),
        length_tolerance: 0.001,
        spatial_cell_size: DEFAULT_SPATIAL_CELL_SIZE,
        limitations: vec!["lane_positions_are_unavailable".to_owned()],
        files: vec![FileEntry {
            name: GEOMETRY_FILE.to_owned(),
            role: "geometry".to_owned(),
            format: GEOMETRY_FORMAT_NAME.to_owned(),
            schema_major: SCHEMA_MAJOR,
            schema_minor: SCHEMA_MINOR,
            bytes: bytes.len() as u64,
            checksum: checksum(bytes),
        }],
    }
}

fn write_package(dir: &std::path::Path, f: &Fixture) {
    let (bytes, info) = write(f);
    std::fs::write(dir.join(GEOMETRY_FILE), &bytes).expect("geometry");
    let manifest = manifest_for(&bytes, f, &info);
    std::fs::write(
        dir.join(MANIFEST_FILE),
        manifest.to_json().expect("manifest"),
    )
    .expect("manifest");
}

#[test]
fn opens_a_package_and_answers_spatial_queries() {
    let dir = temp_dir("open");
    write_package(&dir, &fixture());
    let package = MapPackage::open(&dir).expect("opens");
    assert_eq!(package.data().roads.len(), 2);
    assert_eq!(package.road(0xa).map(|r| r.start), Some(0x1));
    assert!(package.road(0x999).is_none());
    assert_eq!(package.node(0x2).map(|n| n.position), Some([0, 0, -2560]));

    // Road a lies at z in [-10, 0]; road b has no curve and uses its nodes,
    // z in [-20, -10].
    let near = package.roads_near(0.0, -5.0, 1.0);
    assert_eq!(near.len(), 1);
    assert_eq!(near[0].road.id, 0xa);
    assert_eq!(near[0].distance, 0.0);
    let both = package.roads_near(0.0, -10.0, 1.0);
    assert_eq!(
        both.iter().map(|h| h.road.id).collect::<Vec<_>>(),
        vec![0xa, 0xb]
    );
    let area = package.roads_in_bounds([-5.0, -30.0], [5.0, -15.0]);
    assert_eq!(area.iter().map(|r| r.id).collect::<Vec<_>>(), vec![0xb]);
    assert!(package.roads_near(1000.0, 1000.0, 10.0).is_empty());
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn detects_changed_truncated_and_missing_files() {
    let f = fixture();

    let dir = temp_dir("changed");
    write_package(&dir, &f);
    let path = dir.join(GEOMETRY_FILE);
    let mut bytes = std::fs::read(&path).expect("read");
    let i = bytes.iter().position(|&b| b == b'5').expect("a digit");
    bytes[i] = b'6';
    std::fs::write(&path, &bytes).expect("write");
    assert!(matches!(
        MapPackage::open(&dir),
        Err(PackageError::FileMismatch { ref reason, .. }) if reason.contains("checksum")
    ));

    std::fs::write(&path, &bytes[..bytes.len() - 10]).expect("write");
    assert!(matches!(
        MapPackage::open(&dir),
        Err(PackageError::FileMismatch { ref reason, .. }) if reason.contains("bytes")
    ));

    std::fs::remove_file(&path).expect("remove");
    assert!(matches!(
        MapPackage::open(&dir),
        Err(PackageError::FileMismatch { .. })
    ));
    std::fs::remove_dir_all(&dir).expect("cleanup");

    let empty = temp_dir("nomanifest");
    assert!(matches!(MapPackage::open(&empty), Err(PackageError::Io(_))));
    std::fs::write(empty.join(MANIFEST_FILE), "{").expect("write");
    assert!(matches!(
        MapPackage::open(&empty),
        Err(PackageError::Manifest(_))
    ));
    std::fs::remove_dir_all(&empty).expect("cleanup");
}

#[test]
fn rejects_manifest_counts_that_disagree_with_the_data() {
    let f = fixture();
    let dir = temp_dir("counts");
    let (bytes, info) = write(&f);
    std::fs::write(dir.join(GEOMETRY_FILE), &bytes).expect("geometry");
    let mut manifest = manifest_for(&bytes, &f, &info);
    manifest.counts.lanes += 1;
    std::fs::write(
        dir.join(MANIFEST_FILE),
        manifest.to_json().expect("manifest"),
    )
    .expect("manifest");
    assert!(matches!(
        MapPackage::open(&dir),
        Err(PackageError::Inconsistent(ref m)) if m.contains("lanes")
    ));
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn manifest_tolerance_is_checked_against_the_roads() {
    // A road claims consistency with a deviation the manifest tolerance
    // does not allow.
    let mut f = fixture();
    f.roads[0].curve = CurveRecord::Estimated {
        tangent_scale: 10.0,
        estimated_length: 10.05,
        length_deviation: 0.005,
        length_consistent: true,
        bounds: [0.0, 0.0, -10.0, 0.0, 0.0, 0.0],
    };
    let dir = temp_dir("tolerance");
    write_package(&dir, &f);
    assert!(matches!(
        MapPackage::open(&dir),
        Err(PackageError::Geometry(_))
    ));
    std::fs::remove_dir_all(&dir).expect("cleanup");
}
