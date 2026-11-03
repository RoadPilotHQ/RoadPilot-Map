//! Tests for the lane model on hand-built roads.

use roadpilot_map_model::{
    Lane, LaneConfig, LaneConnection, LaneDirection, LaneError, LaneId, LaneIssue, LaneNetwork,
    LaneTransition, LaneType, Node, NodeId, NodeLink, OffsetsState, Position, Road, RoadId,
    RoadLanes, RoadNetwork, RoadSide, Rotation, StoredLaneOffset, TrafficSide, TrafficSideIssue,
    TrafficSideState, TransitionIssue,
};

const IDENTITY: Rotation = Rotation {
    w: 1.0,
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// Roads 10 (nodes 1 to 2) and 11 (nodes 2 to 3), joined at node 2; road 12
/// (nodes 4 to 5) stands alone.
fn network() -> RoadNetwork {
    let node = |id, z: i32, backward, forward| Node {
        id: NodeId(id),
        position: Position {
            fixed: [0, 0, z * 256],
        },
        rotation: IDENTITY,
        backward,
        forward,
    };
    let road = |id, start, end| Road {
        id: RoadId(id),
        start_node: NodeId(start),
        end_node: NodeId(end),
        look_token: 0,
        stored_length: 10.0,
    };
    let r = |id| NodeLink::Road(RoadId(id));
    RoadNetwork::new(
        [
            node(1, 0, NodeLink::None, r(10)),
            node(2, -10, r(10), r(11)),
            node(3, -20, r(11), NodeLink::None),
            node(4, 50, NodeLink::None, r(12)),
            node(5, 40, r(12), NodeLink::None),
        ],
        [road(10, 1, 2), road(11, 2, 3), road(12, 4, 5)],
    )
    .expect("valid road network")
}

fn lane(road: u64, side: RoadSide, index: u16, kind: &str) -> Lane {
    Lane {
        id: LaneId {
            road: RoadId(road),
            side,
            index,
        },
        lane_type: LaneType::Known(kind.to_owned()),
        offset: None,
        width: None,
    }
}

/// `left` lanes on the left side and `right` lanes on the right side.
fn road_lanes(road: u64, left: u16, right: u16, side: TrafficSideState) -> RoadLanes {
    let mut lanes: Vec<Lane> = (0..left)
        .map(|i| lane(road, RoadSide::Left, i, "lane.a"))
        .collect();
    lanes.extend((0..right).map(|i| lane(road, RoadSide::Right, i, "lane.a")));
    RoadLanes {
        road: RoadId(road),
        look: "road.test".to_owned(),
        traffic_side: side,
        config: LaneConfig::Resolved(lanes),
        offsets_left: OffsetsState::NotStored,
        offsets_right: OffsetsState::NotStored,
        stored_size_left: None,
        stored_size_right: None,
    }
}

const RIGHT: TrafficSideState = TrafficSideState::Known(TrafficSide::Right);
const LEFT: TrafficSideState = TrafficSideState::Known(TrafficSide::Left);

fn lane_network(entries: Vec<RoadLanes>) -> Result<LaneNetwork, LaneError> {
    LaneNetwork::new(&network(), entries)
}

fn connection(net: &RoadNetwork, from: u64) -> roadpilot_map_model::RoadConnection {
    net.successor(RoadId(from)).expect("roads are connected")
}

fn id(road: u64, side: RoadSide, index: u16) -> LaneId {
    LaneId {
        road: RoadId(road),
        side,
        index,
    }
}

// ------------------------------------------------------------- model

#[test]
fn a_road_with_one_lane() {
    let net = lane_network(vec![
        road_lanes(10, 0, 1, RIGHT),
        road_lanes(11, 0, 1, RIGHT),
        road_lanes(12, 0, 1, RIGHT),
    ])
    .expect("valid");
    let road = net.road(RoadId(10)).expect("entry");
    assert_eq!(road.lanes().len(), 1);
    assert_eq!(road.lanes()[0].id, id(10, RoadSide::Right, 0));
}

#[test]
fn a_road_with_several_lanes_keeps_their_order() {
    let net = lane_network(vec![
        road_lanes(10, 2, 3, RIGHT),
        road_lanes(11, 0, 0, RIGHT),
        road_lanes(12, 0, 0, RIGHT),
    ])
    .expect("valid");
    let ids: Vec<_> = net
        .road(RoadId(10))
        .expect("entry")
        .lanes()
        .iter()
        .map(|l| (l.id.side, l.id.index))
        .collect();
    assert_eq!(
        ids,
        vec![
            (RoadSide::Left, 0),
            (RoadSide::Left, 1),
            (RoadSide::Right, 0),
            (RoadSide::Right, 1),
            (RoadSide::Right, 2)
        ]
    );
}

#[test]
fn widths_are_unknown_and_offsets_are_kept_as_stored() {
    let mut entry = road_lanes(10, 0, 2, RIGHT);
    if let LaneConfig::Resolved(lanes) = &mut entry.config {
        lanes[0].offset = Some(StoredLaneOffset {
            first: -1.5,
            second: 2.0,
        });
        lanes[1].lane_type = LaneType::Uncertain("lane.b".to_owned());
    }
    entry.offsets_right = OffsetsState::Assigned;
    let net = lane_network(vec![
        entry,
        road_lanes(11, 0, 0, RIGHT),
        road_lanes(12, 0, 0, RIGHT),
    ])
    .expect("valid");
    let lanes = net.road(RoadId(10)).expect("entry").lanes();
    assert_eq!(lanes[0].width, None);
    assert_eq!(
        lanes[0].offset,
        Some(StoredLaneOffset {
            first: -1.5,
            second: 2.0
        })
    );
    assert_eq!(lanes[1].offset, None);
    assert_eq!(lanes[0].lane_type, LaneType::Known("lane.a".to_owned()));
    assert_eq!(lanes[1].lane_type, LaneType::Uncertain("lane.b".to_owned()));
}

#[test]
fn a_missing_road_look_leaves_the_lanes_unresolved() {
    let mut entry = road_lanes(10, 0, 0, RIGHT);
    entry.config = LaneConfig::Unresolved(LaneIssue::MissingRoadLook("road.gone".to_owned()));
    let net = lane_network(vec![
        entry,
        road_lanes(11, 0, 1, RIGHT),
        road_lanes(12, 0, 1, RIGHT),
    ])
    .expect("an unresolved road is valid");
    let road = net.road(RoadId(10)).expect("entry");
    assert!(road.lanes().is_empty());
    assert!(road.lanes_in(LaneDirection::Forward).is_empty());
}

#[test]
fn rejects_duplicate_lane_ids() {
    let mut entry = road_lanes(10, 0, 1, RIGHT);
    if let LaneConfig::Resolved(lanes) = &mut entry.config {
        lanes.push(lane(10, RoadSide::Right, 0, "lane.b"));
    }
    assert_eq!(
        lane_network(vec![
            entry,
            road_lanes(11, 0, 0, RIGHT),
            road_lanes(12, 0, 0, RIGHT)
        ]),
        Err(LaneError::DuplicateLane(id(10, RoadSide::Right, 0)))
    );
}

#[test]
fn rejects_invalid_road_references() {
    // Unknown road.
    assert_eq!(
        lane_network(vec![
            road_lanes(10, 0, 1, RIGHT),
            road_lanes(11, 0, 1, RIGHT),
            road_lanes(12, 0, 1, RIGHT),
            road_lanes(99, 0, 1, RIGHT),
        ]),
        Err(LaneError::UnknownRoad(RoadId(99)))
    );
    // A road listed twice.
    assert_eq!(
        lane_network(vec![
            road_lanes(10, 0, 1, RIGHT),
            road_lanes(10, 0, 1, RIGHT),
        ]),
        Err(LaneError::DuplicateRoad(RoadId(10)))
    );
    // A road of the network without an entry.
    assert_eq!(
        lane_network(vec![
            road_lanes(10, 0, 1, RIGHT),
            road_lanes(11, 0, 1, RIGHT)
        ]),
        Err(LaneError::MissingRoad(RoadId(12)))
    );
    // A lane of another road.
    let mut entry = road_lanes(10, 0, 0, RIGHT);
    entry.config = LaneConfig::Resolved(vec![lane(11, RoadSide::Right, 0, "lane.a")]);
    assert!(matches!(
        lane_network(vec![
            entry,
            road_lanes(11, 0, 0, RIGHT),
            road_lanes(12, 0, 0, RIGHT)
        ]),
        Err(LaneError::LaneOfOtherRoad { .. })
    ));
}

// ------------------------------------------------------------- direction

#[test]
fn right_hand_traffic_runs_right_lanes_forward_and_left_lanes_backward() {
    let entry = road_lanes(10, 1, 2, RIGHT);
    assert_eq!(
        entry.direction(RoadSide::Right),
        Some(LaneDirection::Forward)
    );
    assert_eq!(
        entry.direction(RoadSide::Left),
        Some(LaneDirection::Backward)
    );
    assert_eq!(entry.lanes_in(LaneDirection::Forward).len(), 2);
    assert_eq!(entry.lanes_in(LaneDirection::Backward).len(), 1);
}

#[test]
fn left_hand_traffic_mirrors_the_sides() {
    let entry = road_lanes(10, 3, 1, LEFT);
    assert_eq!(
        entry.direction(RoadSide::Left),
        Some(LaneDirection::Forward)
    );
    assert_eq!(
        entry.direction(RoadSide::Right),
        Some(LaneDirection::Backward)
    );
    assert_eq!(entry.lanes_in(LaneDirection::Forward).len(), 3);
}

#[test]
fn an_unknown_traffic_side_gives_no_direction() {
    let entry = road_lanes(
        10,
        1,
        1,
        TrafficSideState::Unknown(TrafficSideIssue::NoCountry),
    );
    assert_eq!(entry.direction(RoadSide::Right), None);
    assert!(entry.lanes_in(LaneDirection::Forward).is_empty());
    assert!(entry.lanes_in(LaneDirection::Backward).is_empty());
}

// ------------------------------------------------------------- transitions

#[test]
fn matching_roads_get_index_candidates_in_both_directions() {
    let roads = network();
    let net = LaneNetwork::new(
        &roads,
        vec![
            road_lanes(10, 1, 2, RIGHT),
            road_lanes(11, 1, 2, RIGHT),
            road_lanes(12, 0, 0, RIGHT),
        ],
    )
    .expect("valid");
    let LaneTransition::IndexCandidates(pairs) = net.transition(&connection(&roads, 10)) else {
        panic!("candidates expected");
    };
    assert_eq!(
        pairs,
        vec![
            // Forward: road 10 into road 11.
            LaneConnection {
                from: id(10, RoadSide::Right, 0),
                to: id(11, RoadSide::Right, 0)
            },
            LaneConnection {
                from: id(10, RoadSide::Right, 1),
                to: id(11, RoadSide::Right, 1)
            },
            // Backward (oncoming): road 11 into road 10.
            LaneConnection {
                from: id(11, RoadSide::Left, 0),
                to: id(10, RoadSide::Left, 0)
            },
        ]
    );
}

#[test]
fn a_different_lane_count_gives_no_pairing() {
    // Two lanes forward become three: several pairings are possible.
    let roads = network();
    let net = LaneNetwork::new(
        &roads,
        vec![
            road_lanes(10, 1, 2, RIGHT),
            road_lanes(11, 1, 3, RIGHT),
            road_lanes(12, 0, 0, RIGHT),
        ],
    )
    .expect("valid");
    assert_eq!(
        net.transition(&connection(&roads, 10)),
        LaneTransition::Unknown(TransitionIssue::LaneCountDiffers(LaneDirection::Forward))
    );
}

#[test]
fn a_road_connection_without_a_known_traffic_side_has_no_lane_pairing() {
    let roads = network();
    let unknown = TrafficSideState::Unknown(TrafficSideIssue::CountryMismatch);
    let net = LaneNetwork::new(
        &roads,
        vec![
            road_lanes(10, 1, 1, RIGHT),
            road_lanes(11, 1, 1, unknown),
            road_lanes(12, 0, 0, RIGHT),
        ],
    )
    .expect("valid");
    assert_eq!(
        net.transition(&connection(&roads, 10)),
        LaneTransition::Unknown(TransitionIssue::TrafficSideUnknown)
    );
}

#[test]
fn different_traffic_sides_give_no_pairing() {
    let roads = network();
    let net = LaneNetwork::new(
        &roads,
        vec![
            road_lanes(10, 1, 1, RIGHT),
            road_lanes(11, 1, 1, LEFT),
            road_lanes(12, 0, 0, RIGHT),
        ],
    )
    .expect("valid");
    assert_eq!(
        net.transition(&connection(&roads, 10)),
        LaneTransition::Unknown(TransitionIssue::TrafficSideDiffers)
    );
}

#[test]
fn unresolved_lanes_give_no_pairing() {
    let roads = network();
    let mut unresolved = road_lanes(11, 0, 0, RIGHT);
    unresolved.config = LaneConfig::Unresolved(LaneIssue::VariantChangesLaneCount);
    let net = LaneNetwork::new(
        &roads,
        vec![
            road_lanes(10, 1, 1, RIGHT),
            unresolved,
            road_lanes(12, 0, 0, RIGHT),
        ],
    )
    .expect("valid");
    assert_eq!(
        net.transition(&connection(&roads, 10)),
        LaneTransition::Unknown(TransitionIssue::LanesUnresolved)
    );
}

#[test]
fn left_hand_one_way_roads_pair_their_left_lanes() {
    let roads = network();
    let net = LaneNetwork::new(
        &roads,
        vec![
            road_lanes(10, 2, 0, LEFT),
            road_lanes(11, 2, 0, LEFT),
            road_lanes(12, 0, 0, LEFT),
        ],
    )
    .expect("valid");
    let LaneTransition::IndexCandidates(pairs) = net.transition(&connection(&roads, 10)) else {
        panic!("candidates expected");
    };
    assert_eq!(pairs.len(), 2);
    assert!(
        pairs
            .iter()
            .all(|p| p.from.road == RoadId(10) && p.to.road == RoadId(11))
    );
    assert!(pairs.iter().all(|p| p.from.side == RoadSide::Left));
}

#[test]
fn roads_on_different_nodes_have_no_connection_to_pair() {
    // Road 12 is not connected to anything, so there is nothing to pair.
    let roads = network();
    assert!(roads.successor(RoadId(12)).is_none());
    assert!(roads.predecessor(RoadId(12)).is_none());
}
