//! Tests for the prefab model on a hand-built prefab (no game data).
//!
//! Prefab 100 has three nodes. Road 10 ends at its node 1, road 20 starts at
//! its node 2, and node 3 is open. The definition has two internal paths:
//! curves 0 -> 1 from definition node 0 to node 1 (with a traffic rule), and
//! curves 2 -> 3 back. With origin 2, instance node `i` is definition node
//! `(i + 2) % 3`: instance nodes (3, 1, 2) are definition nodes (2, 0, 1).

use roadpilot_map_model::{
    ArmIssue, ArmLink, Lane, LaneConfig, LaneId, LaneNetwork, LaneType, MovementStatus, Node,
    NodeId, NodeLink, OffsetsState, PREFAB_LANE_SLOTS, Position, PrefabCurve, PrefabDefinition,
    PrefabDefinitionNode, PrefabError, PrefabId, PrefabInstance, PrefabNetwork, PrefabResolution,
    Road, RoadId, RoadLanes, RoadNetwork, RoadSide, Rotation, RuleRef, SlotDirection, TrafficSide,
    TrafficSideIssue, TrafficSideState,
};

const P: PrefabId = PrefabId(100);

fn roads() -> RoadNetwork {
    let node = |id, z: i32, backward, forward| Node {
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
    };
    let road = |id, start, end| Road {
        id: RoadId(id),
        start_node: NodeId(start),
        end_node: NodeId(end),
        look_token: 0,
        stored_length: 10.0,
    };
    let r = |id| NodeLink::Road(RoadId(id));
    let p = NodeLink::Item(P.0);
    RoadNetwork::new(
        [
            node(11, 10, NodeLink::None, r(10)),
            node(1, 0, r(10), p),
            node(2, -20, p, r(20)),
            node(21, -30, r(20), NodeLink::None),
            node(3, -10, p, NodeLink::None),
        ],
        [road(10, 11, 1), road(20, 2, 21)],
    )
    .expect("valid road network")
}

fn slots(curve: Option<u16>) -> [Option<u16>; PREFAB_LANE_SLOTS] {
    let mut s = [None; PREFAB_LANE_SLOTS];
    s[0] = curve;
    s
}

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
            PrefabDefinitionNode {
                input_slots: slots(None),
                output_slots: slots(None),
            },
        ],
        curves: vec![
            curve(
                vec![1],
                vec![],
                Some(RuleRef::Resolved("traffic_rule.r".to_owned())),
            ),
            curve(vec![], vec![0], None),
            curve(vec![3], vec![], None),
            curve(vec![], vec![2], None),
        ],
        semaphores: Vec::new(),
    }
}

fn instance(resolution: PrefabResolution) -> PrefabInstance {
    PrefabInstance {
        id: P,
        model: "prefab.x".to_owned(),
        nodes: vec![NodeId(3), NodeId(1), NodeId(2)],
        origin: 2,
        resolution,
    }
}

fn prefabs() -> PrefabNetwork {
    PrefabNetwork::new(
        &roads(),
        [definition()],
        [instance(PrefabResolution::Resolved)],
    )
    .expect("valid prefab network")
}

fn lane_id(road: u64, side: RoadSide) -> LaneId {
    LaneId {
        road: RoadId(road),
        side,
        index: 0,
    }
}

/// One lane per side, with the given traffic side.
fn road_lanes(road: u64, traffic_side: TrafficSideState) -> RoadLanes {
    let lane = |side| Lane {
        id: lane_id(road, side),
        lane_type: LaneType::Known("traffic_lane.a".to_owned()),
        offset: None,
        width: None,
    };
    RoadLanes {
        road: RoadId(road),
        look: "road.test".to_owned(),
        traffic_side,
        config: LaneConfig::Resolved(vec![lane(RoadSide::Left), lane(RoadSide::Right)]),
        offsets_left: OffsetsState::NotStored,
        offsets_right: OffsetsState::NotStored,
        stored_size_left: None,
        stored_size_right: None,
    }
}

const RIGHT: TrafficSideState = TrafficSideState::Known(TrafficSide::Right);

#[test]
fn maps_instance_nodes_to_definition_nodes_by_origin() {
    let net = prefabs();
    let inst = net.instance(P).expect("instance");
    assert_eq!(
        (0..3).map(|i| inst.definition_node(i)).collect::<Vec<_>>(),
        [Some(2), Some(0), Some(1)]
    );
    for d in 0..3 {
        assert_eq!(
            inst.definition_node(inst.instance_node(d).expect("mapped")),
            Some(d)
        );
    }
    assert_eq!(inst.definition_node(3), None);
    let unresolved = instance(PrefabResolution::MissingModel);
    assert_eq!(unresolved.definition_node(0), None);
}

#[test]
fn classifies_what_is_attached_to_each_prefab_node() {
    let arms = prefabs().arms(&roads(), P);
    let links: Vec<ArmLink> = arms.iter().map(|a| a.link).collect();
    assert_eq!(
        links,
        [
            ArmLink::None,
            ArmLink::Road {
                road: RoadId(10),
                road_ends_here: true
            },
            ArmLink::Road {
                road: RoadId(20),
                road_ends_here: false
            },
        ]
    );
    assert_eq!(arms[1].node, NodeId(1));
    assert_eq!(arms[1].definition_node, Some(0));
}

#[test]
fn internal_movements_follow_the_stored_curve_links() {
    let movements = prefabs().movements(P);
    assert_eq!(movements.len(), 2);
    // Road 10's node (instance 1) to road 20's node (instance 2), with the rule.
    assert_eq!(
        (
            movements[0].from_arm,
            movements[0].from_slot,
            movements[0].to_arm,
            movements[0].to_slot
        ),
        (1, 0, 2, 0)
    );
    assert_eq!(
        movements[0].traffic_rules,
        [RuleRef::Resolved("traffic_rule.r".to_owned())]
    );
    assert_eq!(
        (movements[1].from_arm, movements[1].to_arm),
        (2, 1),
        "the way back"
    );
    assert!(movements[1].traffic_rules.is_empty());
}

#[test]
fn road_lanes_through_the_prefab_are_candidates() {
    let roads = roads();
    let lanes =
        LaneNetwork::new(&roads, [road_lanes(10, RIGHT), road_lanes(20, RIGHT)]).expect("lanes");
    let result = prefabs().lane_movements(&roads, &lanes, P);
    let pairs: Vec<(LaneId, LaneId, MovementStatus)> = result
        .movements
        .iter()
        .map(|m| (m.from, m.to, m.status))
        .collect();
    // Right-hand traffic: road 10 enters on its right lane (it ends at the
    // prefab), road 20 leaves on its right lane (it starts there).
    assert_eq!(
        pairs,
        [
            (
                lane_id(10, RoadSide::Right),
                lane_id(20, RoadSide::Right),
                MovementStatus::Candidate
            ),
            (
                lane_id(20, RoadSide::Left),
                lane_id(10, RoadSide::Left),
                MovementStatus::Candidate
            ),
        ]
    );
    // The open node 3 is reported, not paired.
    assert!(result.unpaired_arms.contains(&(0, ArmIssue::NoRoad)));
}

#[test]
fn unknown_traffic_side_or_lane_count_leaves_arms_unpaired() {
    let roads = roads();
    let unknown = TrafficSideState::Unknown(TrafficSideIssue::NoCountry);
    let lanes =
        LaneNetwork::new(&roads, [road_lanes(10, unknown), road_lanes(20, RIGHT)]).expect("lanes");
    let result = prefabs().lane_movements(&roads, &lanes, P);
    assert!(result.movements.is_empty());
    assert!(
        result
            .unpaired_arms
            .contains(&(1, ArmIssue::RoadLanesUnknown))
    );

    // Road 20 with two lanes on the right meets one output slot.
    let mut wide = road_lanes(20, RIGHT);
    if let LaneConfig::Resolved(list) = &mut wide.config {
        list.push(Lane {
            id: LaneId {
                road: RoadId(20),
                side: RoadSide::Right,
                index: 1,
            },
            lane_type: LaneType::Known("traffic_lane.a".to_owned()),
            offset: None,
            width: None,
        });
    }
    let lanes = LaneNetwork::new(&roads, [road_lanes(10, RIGHT), wide]).expect("lanes");
    let result = prefabs().lane_movements(&roads, &lanes, P);
    assert!(result.unpaired_arms.contains(&(
        2,
        ArmIssue::LaneCountMismatch {
            direction: SlotDirection::Output,
            lanes: 2,
            slots: 1
        }
    )));
    assert!(
        result.movements.iter().all(|m| m.to.road != RoadId(20)),
        "no movement into the mismatched road"
    );
}

#[test]
fn unresolved_prefabs_have_no_movements() {
    let roads = roads();
    let net = PrefabNetwork::new(
        &roads,
        [definition()],
        [instance(PrefabResolution::NodeGeometryMismatch)],
    )
    .expect("valid");
    assert!(net.movements(P).is_empty());
    let lanes =
        LaneNetwork::new(&roads, [road_lanes(10, RIGHT), road_lanes(20, RIGHT)]).expect("lanes");
    let result = net.lane_movements(&roads, &lanes, P);
    assert!(result.movements.is_empty());
    assert_eq!(result.unpaired_arms.len(), 3);
    assert!(
        net.arms(&roads, P)
            .iter()
            .all(|a| a.definition_node.is_none())
    );
}

#[test]
fn rejects_inconsistent_prefab_data() {
    let roads = roads();
    let mut missing = instance(PrefabResolution::Resolved);
    missing.nodes[0] = NodeId(99);
    assert_eq!(
        PrefabNetwork::new(&roads, [definition()], [missing]),
        Err(PrefabError::MissingNode {
            prefab: P,
            node: NodeId(99)
        })
    );

    let mut foreign = instance(PrefabResolution::Resolved);
    foreign.nodes[0] = NodeId(11);
    assert!(matches!(
        PrefabNetwork::new(&roads, [definition()], [foreign]),
        Err(PrefabError::NodeNotLinked { .. })
    ));

    assert_eq!(
        PrefabNetwork::new(
            &roads,
            [definition()],
            [
                instance(PrefabResolution::Resolved),
                instance(PrefabResolution::Resolved)
            ]
        ),
        Err(PrefabError::DuplicateInstance(P))
    );

    let mut origin = instance(PrefabResolution::Resolved);
    origin.origin = 3;
    assert_eq!(
        PrefabNetwork::new(&roads, [definition()], [origin]),
        Err(PrefabError::InconsistentResolution(P))
    );
    assert_eq!(
        PrefabNetwork::new(&roads, [], [instance(PrefabResolution::Resolved)]),
        Err(PrefabError::InconsistentResolution(P))
    );

    let mut bad = definition();
    bad.curves[1].prev.clear();
    assert!(matches!(
        PrefabNetwork::new(&roads, [bad], []),
        Err(PrefabError::InvalidDefinition { reason, .. }) if reason.contains("mirrored")
    ));
    let mut bad = definition();
    bad.nodes[0].input_slots[1] = Some(9);
    assert!(matches!(
        PrefabNetwork::new(&roads, [bad], []),
        Err(PrefabError::InvalidDefinition { .. })
    ));
    assert_eq!(
        PrefabNetwork::new(&roads, [definition(), definition()], []),
        Err(PrefabError::DuplicateDefinition("prefab.x".to_owned()))
    );
}

#[test]
fn cyclic_curve_links_terminate() {
    let mut def = definition();
    // 0 -> 1 -> 0 loop in addition to the exit.
    def.curves[1].next = vec![0];
    def.curves[0].prev = vec![1];
    let roads = roads();
    let net =
        PrefabNetwork::new(&roads, [def], [instance(PrefabResolution::Resolved)]).expect("valid");
    assert_eq!(net.movements(P).len(), 2);
}
