//! Tests for the navigation graph and route search (no game data).

use roadpilot_map_model::{
    ConnectionStatus, EdgeId, EdgeOrigin, EdgeOriginKind, GraphIssueKind, Lane, LaneConfig, LaneId,
    LaneNetwork, LaneType, NavNode, NavigationError, NavigationGraph, NavigationGraphBuilder, Node,
    NodeId, NodeLink, OffsetsState, PREFAB_LANE_SLOTS, Position, PrefabCurve, PrefabDefinition,
    PrefabDefinitionNode, PrefabId, PrefabInstance, PrefabNetwork, PrefabResolution, PrefabSlotRef,
    Road, RoadId, RoadLanes, RoadNetwork, RoadSide, Rotation, RouteConfirmation, RouteError,
    RoutingMode, RuleRef, TrafficSide, TrafficSideState,
};

use ConnectionStatus::{Candidate, Confirmed, Forbidden, Unknown};

fn lane(road: u64, index: u16) -> LaneId {
    LaneId {
        road: RoadId(road),
        side: RoadSide::Right,
        index,
    }
}

fn entry(i: u64) -> NavNode {
    NavNode::LaneEntry(lane(i, 0))
}

fn exit(i: u64) -> NavNode {
    NavNode::LaneExit(lane(i, 0))
}

fn origin() -> EdgeOrigin {
    EdgeOrigin {
        kind: EdgeOriginKind::Other,
        source: 0,
    }
}

/// A graph over lane entries `1..=n` and the given directed edges.
fn graph(n: u64, edges: &[(u64, u64, ConnectionStatus)]) -> NavigationGraph {
    let mut b = NavigationGraphBuilder::new();
    for i in 1..=n {
        b.add_node(entry(i)).expect("node");
    }
    for &(from, to, status) in edges {
        b.add_edge(entry(from), entry(to), status, origin(), &[])
            .expect("edge");
    }
    b.finish().expect("graph")
}

fn path(route: &roadpilot_map_model::Route) -> Vec<NavNode> {
    let mut nodes = vec![route.start];
    nodes.extend(route.steps.iter().map(|s| s.to));
    nodes
}

// ---- Graph construction ----

#[test]
fn builds_single_and_connected_edges_with_direction() {
    let g = graph(3, &[(1, 2, Confirmed), (2, 3, Confirmed)]);
    assert_eq!((g.node_count(), g.edge_count()), (3, 2));
    let id = |i| g.node_id(entry(i)).expect("node");
    assert_eq!(g.out_edge_ids(id(1)).count(), 1);
    assert_eq!(g.out_edge_ids(id(3)).count(), 0, "edges are directed");
    assert_eq!(g.status_counts(), [2, 0, 0, 0]);
}

#[test]
fn separate_components_are_not_reachable() {
    let g = graph(4, &[(1, 2, Confirmed), (3, 4, Confirmed)]);
    assert_eq!(
        g.route(entry(1), entry(4), RoutingMode::AllowCandidates),
        Err(RouteError::NoRoute)
    );
    assert_eq!(
        g.route(entry(1), entry(4), RoutingMode::ConfirmedOnly),
        Err(RouteError::NoConfirmedRoute)
    );
}

#[test]
fn opposite_directions_are_separate_edges() {
    let g = graph(2, &[(1, 2, Confirmed), (2, 1, Candidate)]);
    assert_eq!(g.edge_count(), 2);
    assert!(
        g.route(entry(1), entry(2), RoutingMode::ConfirmedOnly)
            .is_ok()
    );
    assert_eq!(
        g.route(entry(2), entry(1), RoutingMode::ConfirmedOnly),
        Err(RouteError::NoConfirmedRoute)
    );
    assert!(
        g.route(entry(2), entry(1), RoutingMode::AllowCandidates)
            .is_ok()
    );
}

#[test]
fn identical_edges_are_merged_and_counted() {
    let g = graph(
        2,
        &[(1, 2, Confirmed), (1, 2, Confirmed), (1, 2, Candidate)],
    );
    assert_eq!(g.merged_duplicates(), 1);
    // Different status: both are kept as parallel edges.
    assert_eq!(g.edge_count(), 2);
}

#[test]
fn rejects_duplicate_nodes_and_missing_references() {
    let mut b = NavigationGraphBuilder::new();
    b.add_node(entry(1)).expect("node");
    assert_eq!(
        b.add_node(entry(1)),
        Err(NavigationError::DuplicateNode(entry(1)))
    );
    assert_eq!(
        b.add_edge(entry(1), entry(9), Confirmed, origin(), &[]),
        Err(NavigationError::MissingNode(entry(9)))
    );
    assert_eq!(
        b.add_edge(entry(8), entry(1), Confirmed, origin(), &[]),
        Err(NavigationError::MissingNode(entry(8)))
    );
}

#[test]
fn a_forbidden_edge_may_not_run_parallel_to_another() {
    let mut b = NavigationGraphBuilder::new();
    b.add_node(entry(1)).expect("node");
    b.add_node(entry(2)).expect("node");
    b.add_edge(entry(1), entry(2), Forbidden, origin(), &[])
        .expect("edge");
    b.add_edge(entry(1), entry(2), Confirmed, origin(), &[])
        .expect("edge");
    assert!(matches!(
        b.finish(),
        Err(NavigationError::ConflictingForbidden { .. })
    ));
}

#[test]
fn the_empty_graph_is_valid_and_has_no_routes() {
    let g = NavigationGraphBuilder::new().finish().expect("empty graph");
    assert_eq!((g.node_count(), g.edge_count()), (0, 0));
    assert_eq!(
        g.route(entry(1), entry(2), RoutingMode::AllowCandidates),
        Err(RouteError::UnknownStart(entry(1)))
    );
}

#[test]
fn rules_are_kept_as_references_and_shared() {
    let rule = RuleRef::Resolved("traffic_rule.r".to_owned());
    let mut b = NavigationGraphBuilder::new();
    for i in 1..=3 {
        b.add_node(entry(i)).expect("node");
    }
    b.add_edge(
        entry(1),
        entry(2),
        Confirmed,
        origin(),
        std::slice::from_ref(&rule),
    )
    .expect("edge");
    b.add_edge(
        entry(2),
        entry(3),
        Confirmed,
        origin(),
        std::slice::from_ref(&rule),
    )
    .expect("edge");
    let g = b.finish().expect("graph");
    let ids: Vec<EdgeId> = (0..2).map(EdgeId).collect();
    assert!(ids.iter().all(|&e| g.rules(e) == [rule.clone()]));
    // Rules never change what is routable.
    assert!(
        g.route(entry(1), entry(3), RoutingMode::ConfirmedOnly)
            .is_ok()
    );
}

// ---- Routing: ConfirmedOnly ----

#[test]
fn confirmed_only_finds_simple_and_multi_step_routes() {
    let g = graph(
        4,
        &[(1, 2, Confirmed), (2, 3, Confirmed), (3, 4, Confirmed)],
    );
    let route = g
        .route(entry(1), entry(4), RoutingMode::ConfirmedOnly)
        .expect("route");
    assert_eq!(path(&route), [entry(1), entry(2), entry(3), entry(4)]);
    assert_eq!(route.cost.edges, 3);
    assert_eq!(
        route.confirmation(),
        RouteConfirmation::AllConnectionsConfirmed
    );
    assert!(route.steps.iter().all(|s| s.status == Confirmed));
}

#[test]
fn confirmed_only_never_uses_candidate_unknown_or_forbidden_edges() {
    for status in [Candidate, Unknown, Forbidden] {
        let g = graph(3, &[(1, 2, Confirmed), (2, 3, status)]);
        assert_eq!(
            g.route(entry(1), entry(3), RoutingMode::ConfirmedOnly),
            Err(RouteError::NoConfirmedRoute),
            "{status:?}"
        );
    }
}

#[test]
fn the_start_equals_the_goal_gives_an_empty_route() {
    let g = graph(1, &[]);
    let route = g
        .route(entry(1), entry(1), RoutingMode::ConfirmedOnly)
        .expect("route");
    assert!(route.steps.is_empty());
    assert_eq!(route.cost.edges, 0);
}

// ---- Routing: AllowCandidates ----

#[test]
fn allow_candidates_marks_candidate_edges_and_positions() {
    let g = graph(
        5,
        &[
            (1, 2, Confirmed),
            (2, 3, Candidate),
            (3, 4, Confirmed),
            (4, 5, Candidate),
        ],
    );
    let route = g
        .route(entry(1), entry(5), RoutingMode::AllowCandidates)
        .expect("route");
    assert_eq!(route.candidate_positions(), [1, 3]);
    assert_eq!(
        route.confirmation(),
        RouteConfirmation::Provisional { candidate_edges: 2 }
    );
    assert_eq!(route.cost.candidate_edges, 2);
}

#[test]
fn allow_candidates_with_only_confirmed_edges_is_fully_confirmed() {
    let g = graph(3, &[(1, 2, Confirmed), (2, 3, Confirmed)]);
    let route = g
        .route(entry(1), entry(3), RoutingMode::AllowCandidates)
        .expect("route");
    assert_eq!(
        route.confirmation(),
        RouteConfirmation::AllConnectionsConfirmed
    );
    assert!(route.candidate_positions().is_empty());
}

#[test]
fn allow_candidates_still_excludes_unknown_and_forbidden() {
    for status in [Unknown, Forbidden] {
        let g = graph(3, &[(1, 2, Candidate), (2, 3, status)]);
        assert_eq!(
            g.route(entry(1), entry(3), RoutingMode::AllowCandidates),
            Err(RouteError::NoRoute),
            "{status:?}"
        );
    }
}

#[test]
fn a_confirmed_route_is_preferred_over_an_equally_long_candidate_route() {
    // 1 -> 2 -> 4 uses a candidate; 1 -> 3 -> 4 is confirmed. Same length.
    let g = graph(
        4,
        &[
            (1, 2, Candidate),
            (2, 4, Confirmed),
            (1, 3, Confirmed),
            (3, 4, Confirmed),
        ],
    );
    let route = g
        .route(entry(1), entry(4), RoutingMode::AllowCandidates)
        .expect("route");
    assert_eq!(path(&route), [entry(1), entry(3), entry(4)]);
    assert_eq!(
        route.confirmation(),
        RouteConfirmation::AllConnectionsConfirmed
    );
}

#[test]
fn fewer_candidates_win_over_a_shorter_route() {
    // Direct route with a candidate (1 edge) against a confirmed detour (3 edges).
    let g = graph(
        4,
        &[
            (1, 4, Candidate),
            (1, 2, Confirmed),
            (2, 3, Confirmed),
            (3, 4, Confirmed),
        ],
    );
    let route = g
        .route(entry(1), entry(4), RoutingMode::AllowCandidates)
        .expect("route");
    assert_eq!(route.cost.candidate_edges, 0);
    assert_eq!(route.cost.edges, 3);
}

// ---- Cycles and large graphs ----

#[test]
fn cycles_are_supported() {
    let g = graph(
        4,
        &[
            (1, 2, Confirmed),
            (2, 3, Confirmed),
            (3, 1, Confirmed),
            (3, 4, Confirmed),
            (2, 2, Confirmed),
        ],
    );
    let route = g
        .route(entry(1), entry(4), RoutingMode::ConfirmedOnly)
        .expect("route");
    assert_eq!(path(&route), [entry(1), entry(2), entry(3), entry(4)]);
    let reachable = g
        .reachable_from(entry(3), RoutingMode::ConfirmedOnly)
        .expect("reachable");
    assert_eq!(reachable.len(), 4);
    assert_eq!(reachable[0], entry(3));
}

#[test]
fn a_long_chain_is_searched_without_recursion() {
    let n = 200_000u64;
    let edges: Vec<_> = (1..n).map(|i| (i, i + 1, Confirmed)).collect();
    let g = graph(n, &edges);
    let route = g
        .route(entry(1), entry(n), RoutingMode::ConfirmedOnly)
        .expect("route");
    assert_eq!(route.steps.len() as u64, n - 1);
}

#[test]
fn routes_are_reproducible() {
    let edges = [
        (1, 2, Candidate),
        (1, 3, Candidate),
        (2, 4, Confirmed),
        (3, 4, Confirmed),
    ];
    let a = graph(4, &edges);
    let b = graph(4, &edges);
    assert_eq!(a, b);
    assert_eq!(
        a.route(entry(1), entry(4), RoutingMode::AllowCandidates),
        b.route(entry(1), entry(4), RoutingMode::AllowCandidates)
    );
}

#[test]
fn invalid_start_and_goal_are_structured_errors() {
    let g = graph(2, &[(1, 2, Confirmed)]);
    assert_eq!(
        g.route(entry(7), entry(2), RoutingMode::ConfirmedOnly),
        Err(RouteError::UnknownStart(entry(7)))
    );
    assert_eq!(
        g.route(entry(1), exit(7), RoutingMode::ConfirmedOnly),
        Err(RouteError::UnknownGoal(exit(7)))
    );
    assert!(
        g.reachable_from(entry(7), RoutingMode::ConfirmedOnly)
            .is_err()
    );
}

// ---- Building from the road, lane and prefab models ----

const P: PrefabId = PrefabId(100);
const Q: PrefabId = PrefabId(101);

fn node(id: u64, z: i32, backward: NodeLink, forward: NodeLink) -> Node {
    Node {
        id: NodeId(id),
        position: Position {
            fixed: [0, 0, z * 256],
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

/// Two nodes; curves 0 -> 1 lead from node 0 to node 1 (with a rule), curves
/// 2 -> 3 from node 1 to node 0.
fn through_definition() -> PrefabDefinition {
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
                Some(RuleRef::Unresolved("traffic_rule.r".to_owned())),
            ),
            curve(vec![], vec![0], None),
            curve(vec![3], vec![], None),
            curve(vec![], vec![2], None),
        ],
        semaphores: Vec::new(),
    }
}

fn instance(id: PrefabId, nodes: [u64; 2]) -> PrefabInstance {
    PrefabInstance {
        id,
        model: "prefab.x".to_owned(),
        nodes: nodes.map(NodeId).to_vec(),
        origin: 0,
        resolution: PrefabResolution::Resolved,
    }
}

fn road_lanes(road: u64, side: TrafficSideState) -> RoadLanes {
    let mk = |s| Lane {
        id: LaneId {
            road: RoadId(road),
            side: s,
            index: 0,
        },
        lane_type: LaneType::Known("traffic_lane.a".to_owned()),
        offset: None,
        width: None,
    };
    RoadLanes {
        road: RoadId(road),
        look: "road.test".to_owned(),
        traffic_side: side,
        config: LaneConfig::Resolved(vec![mk(RoadSide::Left), mk(RoadSide::Right)]),
        offsets_left: OffsetsState::NotStored,
        offsets_right: OffsetsState::NotStored,
        stored_size_left: None,
        stored_size_right: None,
    }
}

const RIGHT: TrafficSideState = TrafficSideState::Known(TrafficSide::Right);

/// Road 10 (nodes 11 to 1) ends at prefab 100's node 1; road 20 (nodes 2 to
/// 21) starts at its node 2.
fn through_graph() -> NavigationGraph {
    let r = |id| NodeLink::Road(RoadId(id));
    let p = NodeLink::Item(P.0);
    let roads = RoadNetwork::new(
        [
            node(11, 10, NodeLink::None, r(10)),
            node(1, 0, r(10), p),
            node(2, -20, p, r(20)),
            node(21, -30, r(20), NodeLink::None),
        ],
        [road(10, 11, 1), road(20, 2, 21)],
    )
    .expect("roads");
    let lanes =
        LaneNetwork::new(&roads, [road_lanes(10, RIGHT), road_lanes(20, RIGHT)]).expect("lanes");
    let prefabs =
        PrefabNetwork::new(&roads, [through_definition()], [instance(P, [1, 2])]).expect("prefabs");
    NavigationGraph::build(&roads, &lanes, &prefabs).expect("graph")
}

fn slot(prefab: PrefabId, arm: u16) -> PrefabSlotRef {
    PrefabSlotRef {
        prefab,
        arm,
        slot: 0,
    }
}

#[test]
fn internal_prefab_movements_are_confirmed_and_carry_their_rules() {
    let g = through_graph();
    let from = NavNode::PrefabInput(slot(P, 0));
    let to = NavNode::PrefabOutput(slot(P, 1));
    let route = g
        .route(from, to, RoutingMode::ConfirmedOnly)
        .expect("confirmed route");
    assert_eq!(route.steps.len(), 1);
    assert_eq!(
        route.steps[0].origin.kind,
        EdgeOriginKind::PrefabInternalCurve
    );
    assert_eq!(route.steps[0].origin.source, P.0);
    assert_eq!(
        g.rules(route.steps[0].edge),
        [RuleRef::Unresolved("traffic_rule.r".to_owned())]
    );
    assert_eq!(
        route.confirmation(),
        RouteConfirmation::AllConnectionsConfirmed
    );
}

#[test]
fn road_to_prefab_connections_are_candidates_and_never_confirmed() {
    let g = through_graph();
    let start = NavNode::LaneEntry(lane(10, 0));
    let goal = NavNode::LaneExit(lane(20, 0));
    assert_eq!(
        g.route(start, goal, RoutingMode::ConfirmedOnly),
        Err(RouteError::NoConfirmedRoute)
    );
    let route = g
        .route(start, goal, RoutingMode::AllowCandidates)
        .expect("provisional route");
    let kinds: Vec<_> = route
        .steps
        .iter()
        .map(|s| (s.origin.kind, s.status))
        .collect();
    assert_eq!(
        kinds,
        [
            (EdgeOriginKind::RoadLane, Candidate),
            (EdgeOriginKind::RoadToPrefabSlot, Candidate),
            (EdgeOriginKind::PrefabInternalCurve, Confirmed),
            (EdgeOriginKind::PrefabSlotToRoad, Candidate),
            (EdgeOriginKind::RoadLane, Candidate),
        ]
    );
    // A confirmed part does not make the whole route confirmed.
    assert_eq!(
        route.confirmation(),
        RouteConfirmation::Provisional { candidate_edges: 4 }
    );
    assert_eq!(route.candidate_positions(), [0, 1, 3, 4]);
}

#[test]
fn directions_are_respected_through_the_prefab() {
    let g = through_graph();
    // Road 10's right lane runs towards the prefab; its left lane away from it.
    let left_entry = NavNode::LaneEntry(LaneId {
        road: RoadId(10),
        side: RoadSide::Left,
        index: 0,
    });
    assert!(
        g.route(
            left_entry,
            NavNode::LaneExit(lane(20, 0)),
            RoutingMode::AllowCandidates
        )
        .is_err(),
        "the left lane of road 10 leads away from the prefab"
    );
    // The way back along the opposite lanes exists.
    let back_start = NavNode::LaneEntry(LaneId {
        road: RoadId(20),
        side: RoadSide::Left,
        index: 0,
    });
    let back_goal = NavNode::LaneExit(LaneId {
        road: RoadId(10),
        side: RoadSide::Left,
        index: 0,
    });
    assert!(
        g.route(back_start, back_goal, RoutingMode::AllowCandidates)
            .is_ok()
    );
    assert!(g.issues().is_empty(), "{:?}", g.issues());
}

#[test]
fn joined_prefabs_get_unknown_edges_that_no_mode_uses() {
    let p = NodeLink::Item(P.0);
    let q = NodeLink::Item(Q.0);
    let roads = RoadNetwork::new(
        [
            node(1, 0, NodeLink::None, p),
            node(2, -20, p, q),
            node(3, -40, q, NodeLink::None),
        ],
        [],
    )
    .expect("roads");
    let lanes = LaneNetwork::new(&roads, []).expect("lanes");
    let prefabs = PrefabNetwork::new(
        &roads,
        [through_definition()],
        [instance(P, [1, 2]), instance(Q, [2, 3])],
    )
    .expect("prefabs");
    let g = NavigationGraph::build(&roads, &lanes, &prefabs).expect("graph");
    // Prefab P's node 1 (arm 1) meets Q's node 0 (arm 0), in both directions.
    let unknown: Vec<_> = g
        .edges()
        .filter(|e| e.status() == Unknown)
        .map(|e| (g.node(e.from()), g.node(e.to()), e.origin().kind))
        .collect();
    assert_eq!(
        unknown,
        [
            (
                Some(NavNode::PrefabOutput(slot(P, 1))),
                Some(NavNode::PrefabInput(slot(Q, 0))),
                EdgeOriginKind::PrefabToPrefab
            ),
            (
                Some(NavNode::PrefabOutput(slot(Q, 0))),
                Some(NavNode::PrefabInput(slot(P, 1))),
                EdgeOriginKind::PrefabToPrefab
            ),
        ]
    );
    // Entering P at arm 0 reaches P's arm 1 output, but not Q.
    let start = NavNode::PrefabInput(slot(P, 0));
    let goal = NavNode::PrefabOutput(slot(Q, 1));
    for mode in [RoutingMode::ConfirmedOnly, RoutingMode::AllowCandidates] {
        assert!(g.route(start, goal, mode).is_err());
    }
}

#[test]
fn unnavigable_roads_and_unresolved_prefabs_are_listed_as_issues() {
    let r = |id| NodeLink::Road(RoadId(id));
    let p = NodeLink::Item(P.0);
    let roads = RoadNetwork::new(
        [
            node(11, 10, NodeLink::None, r(10)),
            node(1, 0, r(10), p),
            node(2, -20, p, NodeLink::Unresolved(77)),
        ],
        [road(10, 11, 1)],
    )
    .expect("roads");
    let unknown_side = TrafficSideState::Unknown(roadpilot_map_model::TrafficSideIssue::NoCountry);
    let lanes = LaneNetwork::new(&roads, [road_lanes(10, unknown_side)]).expect("lanes");
    let mut inst = instance(P, [1, 2]);
    inst.resolution = PrefabResolution::NodeGeometryMismatch;
    let prefabs = PrefabNetwork::new(&roads, [], [inst]).expect("prefabs");
    let g = NavigationGraph::build(&roads, &lanes, &prefabs).expect("graph");
    assert_eq!((g.node_count(), g.edge_count()), (0, 0));
    let kinds: Vec<_> = g.issues().iter().map(|i| i.kind).collect();
    assert!(kinds.contains(&GraphIssueKind::RoadLanesNotNavigable));
    assert!(kinds.contains(&GraphIssueKind::PrefabUnresolved));
    assert!(kinds.contains(&GraphIssueKind::UnresolvedNodeLink));
}

#[test]
fn lane_count_mismatches_at_a_prefab_are_listed_and_not_connected() {
    let r = |id| NodeLink::Road(RoadId(id));
    let p = NodeLink::Item(P.0);
    let roads = RoadNetwork::new(
        [
            node(11, 10, NodeLink::None, r(10)),
            node(1, 0, r(10), p),
            node(2, -20, p, NodeLink::None),
        ],
        [road(10, 11, 1)],
    )
    .expect("roads");
    // Two lanes on the right side meet one input slot.
    let mut wide = road_lanes(10, RIGHT);
    if let LaneConfig::Resolved(list) = &mut wide.config {
        list.push(Lane {
            id: LaneId {
                road: RoadId(10),
                side: RoadSide::Right,
                index: 1,
            },
            lane_type: LaneType::Known("traffic_lane.a".to_owned()),
            offset: None,
            width: None,
        });
    }
    let lanes = LaneNetwork::new(&roads, [wide]).expect("lanes");
    let prefabs =
        PrefabNetwork::new(&roads, [through_definition()], [instance(P, [1, 2])]).expect("prefabs");
    let g = NavigationGraph::build(&roads, &lanes, &prefabs).expect("graph");
    assert!(
        g.issues()
            .iter()
            .any(|i| matches!(i.kind, GraphIssueKind::PrefabArmUnpaired { arm: 0, .. }))
    );
    assert!(
        g.edges()
            .all(|e| e.origin().kind != EdgeOriginKind::RoadToPrefabSlot),
        "no pairing is invented"
    );
}

#[test]
fn building_twice_gives_the_same_graph() {
    assert_eq!(through_graph(), through_graph());
}
