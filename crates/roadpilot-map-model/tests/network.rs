//! Tests for the road and node model on hand-built networks.

use roadpilot_map_model::{
    ModelError, Node, NodeId, NodeLink, NodeRoads, Position, Road, RoadConnection, RoadId,
    RoadNetwork, Rotation,
};

const IDENTITY: Rotation = Rotation {
    w: 1.0,
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

fn node(id: u64, xyz: [i32; 3], backward: NodeLink, forward: NodeLink) -> Node {
    Node {
        id: NodeId(id),
        position: Position {
            fixed: xyz.map(|v| v * 256),
        },
        rotation: IDENTITY,
        backward,
        forward,
    }
}

fn road(id: u64, start: u64, end: u64, length: f32) -> Road {
    Road {
        id: RoadId(id),
        start_node: NodeId(start),
        end_node: NodeId(end),
        look_token: 42,
        stored_length: length,
    }
}

fn r(id: u64) -> NodeLink {
    NodeLink::Road(RoadId(id))
}

/// Nodes 1 -> 2 -> 3 along -Z, joined by roads 10 (1 to 2) and 11 (2 to 3).
/// Node 3 leads into a prefab (item 900); node 1 has nothing before it.
fn chain() -> (Vec<Node>, Vec<Road>) {
    (
        vec![
            node(1, [0, 0, 0], NodeLink::None, r(10)),
            node(2, [0, 2, -10], r(10), r(11)),
            node(3, [0, 2, -20], r(11), NodeLink::Item(900)),
        ],
        vec![road(10, 1, 2, 10.2), road(11, 2, 3, 10.0)],
    )
}

fn build((nodes, roads): (Vec<Node>, Vec<Road>)) -> Result<RoadNetwork, ModelError> {
    RoadNetwork::new(nodes, roads)
}

// ------------------------------------------------------------- model

#[test]
fn road_with_valid_start_and_end_node() {
    let net = build(chain()).expect("valid chain");
    let road = net.road(RoadId(10)).expect("road exists");
    assert_eq!((road.start_node, road.end_node), (NodeId(1), NodeId(2)));
    assert_eq!(road.look_token, 42);
    assert_eq!(
        net.node(NodeId(2)).map(|n| n.position.world()),
        Some([0.0, 2.0, -10.0])
    );
}

#[test]
fn ids_are_unique_and_iteration_is_ordered() {
    let (mut nodes, mut roads) = chain();
    nodes.reverse();
    roads.reverse();
    let net = build((nodes, roads)).expect("valid");
    let node_ids: Vec<u64> = net.nodes().map(|n| n.id.0).collect();
    let road_ids: Vec<u64> = net.roads().map(|r| r.id.0).collect();
    assert_eq!(node_ids, vec![1, 2, 3]);
    assert_eq!(road_ids, vec![10, 11]);
}

#[test]
fn identical_duplicate_nodes_are_merged() {
    // Nodes on sector borders are stored in both sectors.
    let (mut nodes, roads) = chain();
    nodes.push(nodes[1]);
    let net = build((nodes, roads)).expect("identical copies are fine");
    assert_eq!(net.nodes().count(), 3);
}

#[test]
fn rejects_duplicate_ids() {
    let (mut nodes, roads) = chain();
    let mut other = nodes[1];
    other.position.fixed[0] += 1;
    nodes.push(other);
    assert_eq!(
        build((nodes, roads)),
        Err(ModelError::ConflictingNode { node: NodeId(2) })
    );

    let (nodes, mut roads) = chain();
    roads.push(road(10, 1, 2, 1.0));
    assert_eq!(
        build((nodes, roads)),
        Err(ModelError::DuplicateRoad { road: RoadId(10) })
    );
}

#[test]
fn rejects_a_missing_node_reference() {
    let (mut nodes, roads) = chain();
    nodes.retain(|n| n.id != NodeId(3));
    assert_eq!(
        build((nodes, roads)),
        Err(ModelError::MissingNode {
            road: RoadId(11),
            node: NodeId(3)
        })
    );
}

#[test]
fn rejects_invalid_references() {
    // The start node does not point forward to the road.
    let (mut nodes, roads) = chain();
    nodes[0].forward = NodeLink::None;
    assert_eq!(
        build((nodes, roads)),
        Err(ModelError::LinkMismatch {
            road: RoadId(10),
            node: NodeId(1)
        })
    );
    // The end node points backward to another road.
    let (mut nodes, roads) = chain();
    nodes[2].backward = r(10);
    assert!(matches!(
        build((nodes, roads)),
        Err(ModelError::LinkMismatch {
            road: RoadId(11),
            ..
        })
    ));
    // A node names a road that does not exist.
    let (mut nodes, roads) = chain();
    nodes[0].backward = r(77);
    assert_eq!(
        build((nodes, roads)),
        Err(ModelError::InvalidNodeLink {
            node: NodeId(1),
            road: RoadId(77)
        })
    );
    // A node names a road that neither starts nor ends there.
    let (mut nodes, mut roads) = chain();
    nodes.push(node(4, [5, 0, 0], NodeLink::None, r(11)));
    roads.push(road(12, 4, 1, 1.0));
    assert!(build((nodes, roads)).is_err());
}

#[test]
fn rejects_zero_ids_loops_and_bad_values() {
    let (mut nodes, roads) = chain();
    nodes[0].id = NodeId(0);
    assert_eq!(
        build((nodes, roads)),
        Err(ModelError::ZeroId { what: "node" })
    );

    let (nodes, mut roads) = chain();
    roads[0].id = RoadId(0);
    assert_eq!(
        build((nodes, roads)),
        Err(ModelError::ZeroId { what: "road" })
    );

    let (nodes, mut roads) = chain();
    roads[0].end_node = NodeId(1);
    assert_eq!(
        build((nodes, roads)),
        Err(ModelError::RoadLoop { road: RoadId(10) })
    );

    for length in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let (nodes, mut roads) = chain();
        roads[1].stored_length = length;
        assert_eq!(
            build((nodes, roads)),
            Err(ModelError::InvalidLength { road: RoadId(11) }),
            "{length}"
        );
    }

    let (mut nodes, roads) = chain();
    nodes[1].rotation.w = f32::NAN;
    assert_eq!(
        build((nodes, roads)),
        Err(ModelError::InvalidRotation { node: NodeId(2) })
    );
}

#[test]
fn non_road_and_unresolved_links_are_kept_as_they_are() {
    let (mut nodes, roads) = chain();
    nodes[0].backward = NodeLink::Unresolved(555);
    let net = build((nodes, roads)).expect("unresolved items are not road errors");
    assert_eq!(
        net.node(NodeId(1)).map(|n| n.backward),
        Some(NodeLink::Unresolved(555))
    );
    assert_eq!(
        net.node(NodeId(3)).map(|n| n.forward),
        Some(NodeLink::Item(900))
    );
}

// ------------------------------------------------------------- connections

#[test]
fn two_roads_with_a_common_node_are_connected() {
    let net = build(chain()).expect("valid");
    let c = RoadConnection {
        from: RoadId(10),
        via: NodeId(2),
        to: RoadId(11),
    };
    assert_eq!(net.successor(RoadId(10)), Some(c));
    assert_eq!(net.predecessor(RoadId(11)), Some(c));
    assert_eq!(net.connections().collect::<Vec<_>>(), vec![c]);
}

#[test]
fn incoming_and_outgoing_roads_are_distinguished() {
    let net = build(chain()).expect("valid");
    assert_eq!(
        net.roads_at(NodeId(2)),
        Some(NodeRoads {
            ending: Some(RoadId(10)),
            starting: Some(RoadId(11))
        })
    );
    // Road 10 starts at node 1; nothing ends there.
    assert_eq!(
        net.roads_at(NodeId(1)),
        Some(NodeRoads {
            ending: None,
            starting: Some(RoadId(10))
        })
    );
    // Road 11 ends at node 3; a prefab, not a road, follows.
    assert_eq!(
        net.roads_at(NodeId(3)),
        Some(NodeRoads {
            ending: Some(RoadId(11)),
            starting: None
        })
    );
    assert_eq!(net.successor(RoadId(11)), None);
    assert_eq!(net.predecessor(RoadId(10)), None);
    assert_eq!(net.roads_at(NodeId(99)), None);
}

#[test]
fn several_roads_around_one_node() {
    // Road 20 ends at node 5, road 21 starts there; road 22 is attached to
    // node 6 only. Node 5 connects 20 and 21, node 6 connects 21 and 22.
    let nodes = vec![
        node(4, [0, 0, 0], NodeLink::None, r(20)),
        node(5, [0, 0, -5], r(20), r(21)),
        node(6, [0, 0, -9], r(21), r(22)),
        node(7, [3, 0, -12], r(22), NodeLink::None),
    ];
    let roads = vec![
        road(20, 4, 5, 5.0),
        road(21, 5, 6, 4.0),
        road(22, 6, 7, 4.3),
    ];
    let net = RoadNetwork::new(nodes, roads).expect("valid");
    let to: Vec<(u64, u64)> = net.connections().map(|c| (c.from.0, c.to.0)).collect();
    assert_eq!(to, vec![(20, 21), (21, 22)]);
    assert_eq!(
        net.roads_at(NodeId(5)).and_then(|n| n.ending),
        Some(RoadId(20))
    );
}

#[test]
fn roads_on_different_nodes_are_not_connected() {
    let nodes = vec![
        node(1, [0, 0, 0], NodeLink::None, r(10)),
        node(2, [0, 0, -10], r(10), NodeLink::None),
        node(3, [0, 0, -10], NodeLink::None, r(11)),
        node(4, [0, 0, -20], r(11), NodeLink::None),
    ];
    let roads = vec![road(10, 1, 2, 10.0), road(11, 3, 4, 10.0)];
    let net = RoadNetwork::new(nodes, roads).expect("valid");
    // Nodes 2 and 3 have the same position, but they are different nodes.
    assert_eq!(net.connections().count(), 0);
    assert_eq!(net.successor(RoadId(10)), None);
}

// ------------------------------------------------------------- geometry

#[test]
fn geometry_of_a_straight_road() {
    let net = build(chain()).expect("valid");
    let g = net.geometry(RoadId(11)).expect("road exists");
    assert_eq!(g.start, [0.0, 2.0, -10.0]);
    assert_eq!(g.end, [0.0, 2.0, -20.0]);
    assert_eq!(g.chord_length, 10.0);
    assert_eq!(g.stored_length, 10.0);
    assert_eq!(g.start_direction, [0.0, 0.0, -1.0]);
    assert_eq!(g.height_change(), 0.0);
}

#[test]
fn geometry_with_a_height_change() {
    let net = build(chain()).expect("valid");
    let g = net.geometry(RoadId(10)).expect("road exists");
    assert_eq!(g.height_change(), 2.0);
    let expected = (2.0f64 * 2.0 + 10.0 * 10.0).sqrt();
    assert!((g.chord_length - expected).abs() < 1e-12);
    // The stored length is reported as stored, not recomputed.
    assert_eq!(g.stored_length, f64::from(10.2f32));
}

#[test]
fn geometry_of_unknown_roads_is_none() {
    let net = build(chain()).expect("valid");
    assert_eq!(net.geometry(RoadId(12345)), None);
}

#[test]
fn an_empty_network_is_valid() {
    let net = RoadNetwork::new(Vec::new(), Vec::new()).expect("empty is valid");
    assert_eq!(net.connections().count(), 0);
}
