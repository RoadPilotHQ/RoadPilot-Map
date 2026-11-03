//! Tests of the navigation graph export on hand-built models (no game data).

use std::io::{self, Cursor, Write};
use std::path::PathBuf;

use roadpilot_map::Game;
use roadpilot_map::export::{
    self, CheckError, ExportError, ExportInput, ExportMetadata, ExportRecord, KNOWN_LIMITATIONS,
    check_navigation_export, read_navigation_export, write_navigation_export,
    write_navigation_export_file,
};
use roadpilot_map::model::{
    ConnectionStatus, EdgeOrigin, EdgeOriginKind, Lane, LaneConfig, LaneId, LaneNetwork, LaneType,
    NavNode, NavigationGraph, NavigationGraphBuilder, Node, NodeId, NodeLink, OffsetsState,
    PREFAB_LANE_SLOTS, Position, PrefabCurve, PrefabDefinition, PrefabDefinitionNode, PrefabId,
    PrefabInstance, PrefabNetwork, PrefabResolution, Road, RoadId, RoadLanes, RoadNetwork,
    RoadSide, Rotation, RuleRef, TrafficSide, TrafficSideState,
};

// ---- Fixtures ----

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

/// Curves 0 -> 1 lead from node 0 to node 1 (with a rule), 2 -> 3 back.
fn definition() -> PrefabDefinition {
    let curve = |next: Vec<u16>, prev: Vec<u16>, rule: Option<RuleRef>| PrefabCurve {
        next,
        prev,
        traffic_rule: rule,
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
            curve(
                vec![1],
                vec![],
                Some(RuleRef::Resolved("traffic_rule.\"quoted\"".to_owned())),
            ),
            curve(vec![], vec![0], None),
            curve(vec![3], vec![], None),
            curve(vec![], vec![2], None),
        ],
        semaphores: Vec::new(),
    }
}

fn instance(id: u64, nodes: [u64; 2]) -> PrefabInstance {
    PrefabInstance {
        id: PrefabId(id),
        model: "prefab.x".to_owned(),
        nodes: nodes.map(NodeId).to_vec(),
        origin: 0,
        resolution: PrefabResolution::Resolved,
    }
}

fn road_lanes(road: u64) -> RoadLanes {
    let lane = |side| Lane {
        id: LaneId {
            road: RoadId(road),
            side,
            index: 0,
        },
        lane_type: LaneType::Known("traffic_lane.a".to_owned()),
        offset: None,
        width: None,
    };
    RoadLanes {
        road: RoadId(road),
        look: "road.test".to_owned(),
        traffic_side: TrafficSideState::Known(TrafficSide::Right),
        config: LaneConfig::Resolved(vec![lane(RoadSide::Left), lane(RoadSide::Right)]),
        offsets_left: OffsetsState::NotStored,
        offsets_right: OffsetsState::NotStored,
        stored_size_left: None,
        stored_size_right: None,
    }
}

struct Models {
    network: RoadNetwork,
    lanes: LaneNetwork,
    prefabs: PrefabNetwork,
    graph: NavigationGraph,
}

impl Models {
    fn input(&self) -> ExportInput<'_> {
        ExportInput {
            graph: &self.graph,
            network: &self.network,
            lanes: &self.lanes,
            prefabs: &self.prefabs,
        }
    }
}

/// Road 10 -> prefab 100 -> road 20 -> road 22, and road 30 on its own.
fn connected_models() -> Models {
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
    let lanes = LaneNetwork::new(&network, [10, 20, 22, 30].map(road_lanes)).expect("lanes");
    let prefabs =
        PrefabNetwork::new(&network, [definition()], [instance(100, [1, 2])]).expect("prefabs");
    let graph = NavigationGraph::build(&network, &lanes, &prefabs).expect("graph");
    Models {
        network,
        lanes,
        prefabs,
        graph,
    }
}

/// Two prefabs joined at node 2, no roads.
fn joint_models() -> Models {
    let p = NodeLink::Item(100);
    let q = NodeLink::Item(101);
    let network = RoadNetwork::new(
        [
            node(1, 0, NodeLink::None, p),
            node(2, -20, p, q),
            node(3, -40, q, NodeLink::None),
        ],
        [],
    )
    .expect("roads");
    let lanes = LaneNetwork::new(&network, []).expect("lanes");
    let prefabs = PrefabNetwork::new(
        &network,
        [definition()],
        [instance(100, [1, 2]), instance(101, [2, 3])],
    )
    .expect("prefabs");
    let graph = NavigationGraph::build(&network, &lanes, &prefabs).expect("graph");
    Models {
        network,
        lanes,
        prefabs,
        graph,
    }
}

fn lane(road: u64, side: RoadSide) -> LaneId {
    LaneId {
        road: RoadId(road),
        side,
        index: 0,
    }
}

/// The models of `connected_models` with a graph that has one edge of every
/// status, built by hand.
fn all_status_models() -> Models {
    let m = connected_models();
    let nodes = [
        NavNode::LaneEntry(lane(10, RoadSide::Right)),
        NavNode::LaneExit(lane(10, RoadSide::Right)),
        NavNode::LaneEntry(lane(20, RoadSide::Right)),
    ];
    let mut b = NavigationGraphBuilder::new();
    for n in nodes {
        b.add_node(n).expect("node");
    }
    let origin = |source| EdgeOrigin {
        kind: EdgeOriginKind::Other,
        source,
    };
    let rule = [RuleRef::Unresolved("traffic_rule.missing".to_owned())];
    b.add_edge(
        nodes[0],
        nodes[1],
        ConnectionStatus::Forbidden,
        origin(1),
        &[],
    )
    .expect("edge");
    b.add_edge(
        nodes[1],
        nodes[2],
        ConnectionStatus::Unknown,
        origin(2),
        &rule,
    )
    .expect("edge");
    b.add_edge(
        nodes[0],
        nodes[2],
        ConnectionStatus::Candidate,
        origin(u64::MAX),
        &[],
    )
    .expect("edge");
    b.add_edge(
        nodes[2],
        nodes[0],
        ConnectionStatus::Confirmed,
        origin(0),
        &[],
    )
    .expect("edge");
    Models {
        graph: b.finish().expect("graph"),
        ..m
    }
}

fn empty_models() -> Models {
    let network = RoadNetwork::new([], []).expect("roads");
    let lanes = LaneNetwork::new(&network, []).expect("lanes");
    let prefabs = PrefabNetwork::new(&network, [], []).expect("prefabs");
    let graph = NavigationGraphBuilder::new().finish().expect("graph");
    Models {
        network,
        lanes,
        prefabs,
        graph,
    }
}

fn meta() -> ExportMetadata {
    ExportMetadata {
        game: Game::Ets2,
        map: Some("europe".to_owned()),
        game_version: None,
        archives: Some(vec!["base.scs".to_owned(), "dlc_\"x\".scs".to_owned()]),
    }
}

fn export_bytes(m: &Models) -> Vec<u8> {
    let mut out = Vec::new();
    write_navigation_export(&mut out, &m.input(), &meta()).expect("export");
    out
}

fn export_text(m: &Models) -> String {
    String::from_utf8(export_bytes(m)).expect("UTF-8")
}

fn check(text: &str) -> Result<export::ExportSummary, CheckError> {
    check_navigation_export(Cursor::new(text.as_bytes()))
}

fn kind_name(n: NavNode) -> &'static str {
    match n {
        NavNode::LaneEntry(_) => "lane_entry",
        NavNode::LaneExit(_) => "lane_exit",
        NavNode::PrefabInput(_) => "prefab_input",
        NavNode::PrefabOutput(_) => "prefab_output",
    }
}

/// Replaces the first occurrence of `from` in `text`, which must exist.
fn replace(text: &str, from: &str, to: &str) -> String {
    assert!(text.contains(from), "{from:?} not found");
    text.replacen(from, to, 1)
}

// ---- Schema ----

#[test]
fn writes_the_metadata_header() {
    let text = export_text(&connected_models());
    let first = text.lines().next().expect("header");
    assert!(first.starts_with(
        r#"{"t":"header","format":"roadpilot-map-navigation-graph","schema_version":{"major":1,"minor":0}"#
    ));
    assert!(first.contains(r#""generated_at":null"#));
    assert!(first.contains(r#""game":"ets2""#));
    assert!(first.contains(r#""map":"europe","game_version":null"#));
    assert!(first.contains(r#""archives":["base.scs","dlc_\"x\".scs"]"#));
    assert!(first.contains(r#""position_scale":256"#));
    assert!(first.contains(r#""world_unit":"unconfirmed""#));
    for l in KNOWN_LIMITATIONS {
        assert!(first.contains(l), "{l}");
    }
    let summary = check(&text).expect("valid");
    assert_eq!(summary.header.game, "ets2");
    assert_eq!(summary.header.schema_major, 1);
    assert_eq!(summary.header.limitations.len(), KNOWN_LIMITATIONS.len());
}

#[test]
fn unavailable_metadata_is_written_as_null() {
    let m = connected_models();
    let mut out = Vec::new();
    let meta = ExportMetadata {
        game: Game::Ats,
        map: None,
        game_version: None,
        archives: None,
    };
    write_navigation_export(&mut out, &m.input(), &meta).expect("export");
    let text = String::from_utf8(out).expect("UTF-8");
    let first = text.lines().next().expect("header");
    assert!(
        first.contains(r#""game":"ats","source":{"map":null,"game_version":null,"archives":null}"#)
    );
    assert_eq!(check(&text).expect("valid").header.game, "ats");
}

#[test]
fn every_node_kind_edge_category_and_status_is_written() {
    let mut kinds = std::collections::BTreeSet::new();
    let mut categories = std::collections::BTreeSet::new();
    let mut statuses = std::collections::BTreeSet::new();
    for m in [connected_models(), joint_models(), all_status_models()] {
        let s = check(&export_text(&m)).expect("valid");
        kinds.extend(s.nodes_by_kind.keys().cloned());
        categories.extend(s.edges_by_category.keys().cloned());
        statuses.extend(s.edges_by_status.keys().cloned());
    }
    assert_eq!(
        kinds.into_iter().collect::<Vec<_>>(),
        ["lane_entry", "lane_exit", "prefab_input", "prefab_output"]
    );
    assert_eq!(
        categories.into_iter().collect::<Vec<_>>(),
        [
            "other",
            "prefab_internal",
            "prefab_slot_to_road",
            "prefab_to_prefab",
            "road_connection",
            "road_lane",
            "road_to_prefab_slot"
        ]
    );
    assert_eq!(
        statuses.into_iter().collect::<Vec<_>>(),
        ["candidate", "confirmed", "forbidden", "unknown"]
    );
}

#[test]
fn records_equal_the_graph_one_by_one() {
    for m in [connected_models(), joint_models(), all_status_models()] {
        let bytes = export_bytes(&m);
        let mut nodes = m.graph.nodes();
        let mut edges = m.graph.edges();
        let (mut n, mut e, mut footers) = (0, 0, 0);
        read_navigation_export(Cursor::new(&bytes), &mut |r| match r {
            ExportRecord::Node(x) => {
                let src = nodes.next().expect("one node per record");
                assert_eq!(x.id as usize, n);
                assert_eq!(x.kind, kind_name(src));
                let map_node = m.network.node(NodeId(x.map_node)).expect("map node exists");
                assert_eq!(x.position, Some(map_node.position.fixed.map(i64::from)));
                n += 1;
            }
            ExportRecord::Edge(x) => {
                let src = edges.next().expect("one edge per record");
                assert_eq!(x.id as usize, e);
                assert_eq!(x.from as usize, src.from().index());
                assert_eq!(x.to as usize, src.to().index());
                let status = match src.status() {
                    ConnectionStatus::Confirmed => "confirmed",
                    ConnectionStatus::Candidate => "candidate",
                    ConnectionStatus::Unknown => "unknown",
                    ConnectionStatus::Forbidden => "forbidden",
                };
                assert_eq!(x.status, status);
                assert_eq!(x.source, src.origin().source);
                assert_eq!(
                    x.rules.len(),
                    m.graph.rules(roadpilot_map::model::EdgeId(x.id)).len()
                );
                e += 1;
            }
            ExportRecord::Footer => footers += 1,
            _ => {}
        })
        .expect("valid");
        assert_eq!(
            (n, e, footers),
            (m.graph.node_count(), m.graph.edge_count(), 1)
        );
        assert!(
            nodes.next().is_none() && edges.next().is_none(),
            "nothing is lost"
        );
    }
}

#[test]
fn unknown_candidate_and_forbidden_edges_are_not_dropped_or_changed() {
    let s = check(&export_text(&all_status_models())).expect("valid");
    assert_eq!(s.edges_by_status["confirmed"], 1);
    assert_eq!(s.edges_by_status["candidate"], 1);
    assert_eq!(s.edges_by_status["unknown"], 1);
    assert_eq!(s.edges_by_status["forbidden"], 1);
    let j = check(&export_text(&joint_models())).expect("valid");
    assert!(j.edges_by_status["unknown"] > 0);
    let c = check(&export_text(&connected_models())).expect("valid");
    assert!(c.edges_by_status["candidate"] > 0 && c.edges_by_status["confirmed"] > 0);
    assert!(!c.edges_by_status.contains_key("forbidden"));
}

#[test]
fn rules_and_large_identifiers_survive_the_round_trip() {
    let m = all_status_models();
    let mut rules = Vec::new();
    let mut sources = Vec::new();
    read_navigation_export(Cursor::new(export_bytes(&m)), &mut |r| {
        if let ExportRecord::Edge(e) = r {
            sources.push(e.source);
            rules.extend(e.rules.clone());
        }
    })
    .expect("valid");
    assert_eq!(rules, [("traffic_rule.missing".to_owned(), false)]);
    assert!(sources.contains(&u64::MAX), "64-bit ids are exact");
    let c = connected_models();
    let mut quoted = Vec::new();
    read_navigation_export(Cursor::new(export_bytes(&c)), &mut |r| {
        if let ExportRecord::Edge(e) = r {
            quoted.extend(e.rules.clone());
        }
    })
    .expect("valid");
    assert!(quoted.contains(&("traffic_rule.\"quoted\"".to_owned(), true)));
}

#[test]
fn issues_are_exported_with_their_reasons() {
    let s = check(&export_text(&connected_models())).expect("valid");
    assert_eq!(s.header.issues, s.issues_by_kind.values().sum::<usize>());
    let j = check(&export_text(&joint_models())).expect("valid");
    assert_eq!(j.header.edges, j.edges_by_category.values().sum::<usize>());
}

#[test]
fn the_empty_graph_is_a_valid_export() {
    let text = export_text(&empty_models());
    assert_eq!(text.lines().count(), 2, "header and footer only");
    let s = check(&text).expect("valid");
    assert_eq!((s.header.nodes, s.header.edges, s.header.issues), (0, 0, 0));
}

// ---- Integrity ----

#[test]
fn the_output_is_deterministic() {
    for m in [connected_models(), joint_models(), all_status_models()] {
        assert_eq!(export_bytes(&m), export_bytes(&m));
    }
    // Models built twice give the same bytes.
    assert_eq!(
        export_bytes(&connected_models()),
        export_bytes(&connected_models())
    );
}

/// Weak component sizes of a graph given as edges over `n` nodes.
fn components(n: usize, edges: &[(usize, usize)]) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for &(a, b) in edges {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        parent[ra] = rb;
    }
    let mut size = std::collections::BTreeMap::new();
    for i in 0..n {
        *size.entry(find(&mut parent, i)).or_insert(0usize) += 1;
    }
    let mut sizes: Vec<usize> = size.into_values().collect();
    sizes.sort_unstable();
    sizes
}

#[test]
fn disconnected_regions_stay_separate() {
    let m = connected_models();
    let mut edges = Vec::new();
    let mut n = 0;
    read_navigation_export(Cursor::new(export_bytes(&m)), &mut |r| match r {
        ExportRecord::Node(_) => n += 1,
        ExportRecord::Edge(e) => edges.push((e.from as usize, e.to as usize)),
        _ => {}
    })
    .expect("valid");
    let from_file = components(n, &edges);
    let from_graph = components(
        m.graph.node_count(),
        &m.graph
            .edges()
            .map(|e| (e.from().index(), e.to().index()))
            .collect::<Vec<_>>(),
    );
    assert_eq!(from_file, from_graph);
    // Road 30 (2 lanes, 4 nodes) is its own region of two components (one
    // per direction); the rest of the network is separate from it.
    assert!(from_file.len() >= 3, "{from_file:?}");
    assert!(from_file.contains(&2));
    // The nodes of road 30 are exported.
    let text = export_text(&m);
    assert!(text.contains(&format!("\"road\":\"{:x}\"", 30)));
}

#[test]
fn issues_of_the_models_are_not_hidden() {
    let m = joint_models();
    let s = check(&export_text(&m)).expect("valid");
    assert_eq!(s.header.issues, m.graph.issues().len());
}

// ---- Reader: invalid and unsupported files ----

#[test]
fn rejects_unsupported_schema_versions_safely() {
    let text = export_text(&connected_models());
    let v2 = replace(&text, r#""major":1"#, r#""major":2"#);
    assert_eq!(check(&v2), Err(CheckError::UnsupportedVersion { major: 2 }));
    let v0 = replace(&text, r#""major":1"#, r#""major":0"#);
    assert!(matches!(
        check(&v0),
        Err(CheckError::UnsupportedVersion { major: 0 })
    ));
}

#[test]
fn accepts_newer_minor_versions_with_unknown_fields_and_records() {
    let text = export_text(&connected_models());
    let newer = replace(&text, r#""minor":0"#, r#""minor":7,"future":{"x":[1,2]}"#);
    let newer = replace(
        &newer,
        "{\"t\":\"footer\"",
        "{\"t\":\"component\",\"id\":1}\n{\"t\":\"footer\"",
    );
    let s = check(&newer).expect("valid");
    assert_eq!(s.header.schema_minor, 7);
    assert_eq!(s.ignored_records, 1);
}

#[test]
fn rejects_files_that_are_not_exports() {
    let text = export_text(&connected_models());
    assert!(matches!(check(""), Err(CheckError::Incomplete(_))));
    assert!(check("{}\n").is_err());
    assert!(check("not json\n").is_err());
    let wrong = replace(&text, "roadpilot-map-navigation-graph", "something-else");
    assert!(matches!(
        check(&wrong),
        Err(CheckError::Line { line: 1, .. })
    ));
    // A node before the header.
    let lines: Vec<&str> = text.lines().collect();
    let swapped = format!("{}\n{}\n", lines[1], lines[0]);
    assert!(check(&swapped).is_err());
}

#[test]
fn rejects_incomplete_files() {
    let text = export_text(&connected_models());
    let without_footer: String = text
        .lines()
        .take(text.lines().count() - 1)
        .map(|l| format!("{l}\n"))
        .collect();
    assert!(matches!(
        check(&without_footer),
        Err(CheckError::Incomplete(_))
    ));
    // Cut in the middle of a line.
    let cut = &text[..text.len() / 2];
    assert!(check(cut).is_err());
    // Every prefix of a small export is rejected.
    let small = export_text(&joint_models());
    // (The footer line without its final newline is still complete.)
    for end in 0..small.trim_end().len() {
        if small.is_char_boundary(end) {
            assert!(check(&small[..end]).is_err(), "prefix {end}");
        }
    }
    // Fewer nodes than the header says.
    let lines: Vec<&str> = text.lines().collect();
    let dropped = lines
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 2)
        .map(|(_, l)| format!("{l}\n"))
        .collect::<String>();
    assert!(check(&dropped).is_err());
}

#[test]
fn rejects_invalid_references_and_orders() {
    let text = export_text(&connected_models());
    let count = text
        .lines()
        .filter(|l| l.contains("\"t\":\"node\""))
        .count();
    let bad_to = replace(
        &text,
        r#""id":0,"from":0,"to":1"#,
        &format!(r#""id":0,"from":0,"to":{count}"#),
    );
    let err = check(&bad_to).expect_err("dangling reference");
    assert!(err.to_string().contains("missing node"), "{err}");
    let out_of_order = replace(&text, r#""t":"node","id":1,"#, r#""t":"node","id":5,"#);
    assert!(check(&out_of_order).is_err());
    let edge_id = replace(&text, r#""t":"edge","id":0,"#, r#""t":"edge","id":3,"#);
    assert!(check(&edge_id).is_err());
}

#[test]
fn rejects_invalid_values() {
    let text = export_text(&connected_models());
    assert!(
        check(&replace(
            &text,
            r#""status":"candidate""#,
            r#""status":"maybe""#
        ))
        .is_err()
    );
    assert!(
        check(&replace(
            &text,
            r#""kind":"lane_entry""#,
            r#""kind":"lane_middle""#
        ))
        .is_err()
    );
    assert!(
        check(&replace(
            &text,
            r#""category":"road_lane""#,
            r#""category":"road_curve""#
        ))
        .is_err()
    );
    // A category that does not fit its node kinds.
    assert!(
        check(&replace(
            &text,
            r#""category":"road_lane""#,
            r#""category":"prefab_internal""#
        ))
        .is_err()
    );
    // Identifiers must be lower-case hex strings.
    assert!(check(&replace(&text, r#""road":"a"#, r#""road":"Z"#)).is_err());
    assert!(check(&replace(&text, r#""map_node":""#, r#""map_node":"0x"#)).is_err());
    assert!(check(&replace(&text, r#""side":"left""#, r#""side":"up""#)).is_err());
    assert!(check(&replace(&text, r#""position":["#, r#""position":[1.5,"#)).is_err());
    let missing = replace(&text, r#""map_node":"#, r#""mapnode":"#);
    assert!(check(&missing).is_err());
}

#[test]
fn rejects_count_mismatches_and_trailing_data() {
    let text = export_text(&connected_models());
    let nodes = text
        .lines()
        .filter(|l| l.contains("\"t\":\"node\""))
        .count();
    let bad_header = replace(
        &text,
        &format!("\"nodes\":{nodes},\"edges\""),
        &format!("\"nodes\":{},\"edges\"", nodes + 1),
    );
    assert!(check(&bad_header).is_err());
    let bad_footer = replace(&text, "\"complete\":true", "\"complete\":false");
    assert!(check(&bad_footer).is_err());
    assert!(check(&format!("{text}{{\"t\":\"edge\"}}\n")).is_err());
    // Blank lines at the end are tolerated.
    assert!(check(&format!("{text}\n")).is_ok());
    // Sections out of order.
    let lines: Vec<&str> = text.lines().collect();
    let node_line = lines
        .iter()
        .position(|l| l.contains("\"t\":\"node\""))
        .expect("node");
    let edge_line = lines
        .iter()
        .position(|l| l.contains("\"t\":\"edge\""))
        .expect("edge");
    let mut reordered = lines.clone();
    reordered.swap(node_line, edge_line);
    let joined = reordered
        .iter()
        .map(|l| format!("{l}\n"))
        .collect::<String>();
    assert!(check(&joined).is_err());
}

#[test]
fn rejects_sizes_beyond_the_index_space_and_overlong_lines() {
    let text = export_text(&empty_models());
    let huge = replace(
        &text,
        "\"nodes\":0,\"edges\":0",
        "\"nodes\":4294967296,\"edges\":0",
    );
    assert!(check(&huge).is_err());
    let long = format!(
        "{}{}\n",
        text.lines().next().expect("header").trim_end_matches('}'),
        ",\"pad\":\"".to_owned() + &"x".repeat(1 << 21) + "\"}"
    );
    assert!(matches!(
        check(&long),
        Err(CheckError::Line { line: 1, .. })
    ));
}

// ---- Writing: failures and files ----

/// A writer that fails after `limit` bytes.
struct FailingWriter {
    written: usize,
    limit: usize,
}

impl Write for FailingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.written + buf.len() > self.limit {
            return Err(io::Error::other("disk full"));
        }
        self.written += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.written >= self.limit {
            return Err(io::Error::other("disk full"));
        }
        Ok(())
    }
}

#[test]
fn write_failures_are_reported() {
    let m = connected_models();
    let total = export_bytes(&m).len();
    for limit in [0, 10, total / 2, total - 1] {
        let w = FailingWriter { written: 0, limit };
        let result = write_navigation_export(w, &m.input(), &meta());
        assert!(matches!(result, Err(ExportError::Io(_))), "limit {limit}");
    }
}

#[test]
fn inconsistent_models_are_an_error_not_a_partial_success() {
    let m = connected_models();
    let other = joint_models();
    // The graph's lane nodes do not exist in the other road network.
    let input = ExportInput {
        graph: &m.graph,
        network: &other.network,
        lanes: &other.lanes,
        prefabs: &other.prefabs,
    };
    let mut out = Vec::new();
    let result = write_navigation_export(&mut out, &input, &meta());
    assert!(matches!(result, Err(ExportError::Inconsistent(_))));
    assert!(check(std::str::from_utf8(&out).expect("UTF-8")).is_err());
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "roadpilot-map-export-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

#[test]
fn writes_a_file_atomically_and_applies_the_overwrite_policy() {
    let dir = temp_dir("file");
    let out = dir.join("graph.jsonl");
    let m = connected_models();
    let stats =
        write_navigation_export_file(&out, false, None, &m.input(), &meta()).expect("written");
    assert_eq!(std::fs::metadata(&out).expect("file").len(), stats.bytes);
    assert_eq!(
        (stats.nodes, stats.edges),
        (m.graph.node_count(), m.graph.edge_count())
    );
    assert!(!dir.join("graph.jsonl.partial").exists());
    assert_eq!(std::fs::read(&out).expect("read"), export_bytes(&m));

    // An existing file is kept without --force.
    std::fs::write(&out, b"precious").expect("write");
    let result = write_navigation_export_file(&out, false, None, &m.input(), &meta());
    assert!(matches!(result, Err(ExportError::OutputExists(_))));
    assert_eq!(std::fs::read(&out).expect("read"), b"precious");

    // With the overwrite flag, it is replaced by a complete export.
    write_navigation_export_file(&out, true, None, &m.input(), &meta()).expect("replaced");
    assert!(
        check_navigation_export(io::BufReader::new(std::fs::File::open(&out).expect("open")))
            .is_ok()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn invalid_output_locations_are_rejected_without_writing() {
    let dir = temp_dir("invalid");
    let m = connected_models();
    // The output folder does not exist.
    let missing = dir.join("no-such-folder").join("graph.jsonl");
    assert!(matches!(
        write_navigation_export_file(&missing, false, None, &m.input(), &meta()),
        Err(ExportError::InvalidOutput(_))
    ));
    // The output is a folder.
    assert!(matches!(
        write_navigation_export_file(&dir, true, None, &m.input(), &meta()),
        Err(ExportError::InvalidOutput(_))
    ));
    // The output is inside the forbidden (game) folder.
    let game = dir.join("game");
    std::fs::create_dir_all(&game).expect("dir");
    let inside = game.join("graph.jsonl");
    assert!(matches!(
        write_navigation_export_file(&inside, true, Some(&game), &m.input(), &meta()),
        Err(ExportError::InvalidOutput(_))
    ));
    assert!(!inside.exists());
    // A stale partial file is not touched.
    let out = dir.join("graph.jsonl");
    std::fs::write(dir.join("graph.jsonl.partial"), b"stale").expect("write");
    assert!(matches!(
        write_navigation_export_file(&out, false, None, &m.input(), &meta()),
        Err(ExportError::InvalidOutput(_))
    ));
    assert_eq!(
        std::fs::read(dir.join("graph.jsonl.partial")).expect("read"),
        b"stale"
    );
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_export_leaves_no_file_and_keeps_an_existing_one() {
    let dir = temp_dir("failed");
    let out = dir.join("graph.jsonl");
    let m = connected_models();
    let other = joint_models();
    let input = ExportInput {
        graph: &m.graph,
        network: &other.network,
        lanes: &other.lanes,
        prefabs: &other.prefabs,
    };
    let result = write_navigation_export_file(&out, false, None, &input, &meta());
    assert!(matches!(result, Err(ExportError::Inconsistent(_))));
    assert!(!out.exists());
    assert!(!dir.join("graph.jsonl.partial").exists());

    std::fs::write(&out, b"previous").expect("write");
    let result = write_navigation_export_file(&out, true, None, &input, &meta());
    assert!(result.is_err());
    assert_eq!(std::fs::read(&out).expect("read"), b"previous");
    assert!(!dir.join("graph.jsonl.partial").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
