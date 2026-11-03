//! Building the lane model from synthetic sectors and definitions (no game data).

use roadpilot_map::formats::parse_base;
use roadpilot_map::formats::sii::parse_sii;
use roadpilot_map::model::{
    LaneConfig, LaneDirection, LaneIssue, LaneNetwork, LaneTransition, LaneType, OffsetsState,
    RoadId, RoadSide, StoredLaneOffset, TrafficSide, TrafficSideIssue, TrafficSideState,
};
use roadpilot_map::{DefinitionsError, LaneDefinitions, build_lane_network, build_road_network};

/// Encodes a token name (`0-9`, `a-z`, `_`).
fn token(name: &str) -> u64 {
    name.bytes().rev().fold(0u64, |acc, c| {
        let digit = match c {
            b'0'..=b'9' => u64::from(c - b'0') + 1,
            b'a'..=b'z' => u64::from(c - b'a') + 11,
            _ => 37,
        };
        acc * 38 + digit
    })
}

/// A road item `(uid, look token name, start node, end node)`.
type RoadSpec = (u64, &'static str, u64, u64);
/// A node `(uid, z in world units, backward item, forward item, country ids)`.
type NodeSpec = (u64, i32, u64, u64, [u8; 2]);

/// Builds a `.base` file following `docs/formats/scs-sector-format.md`.
fn sector(roads: &[RoadSpec], nodes: &[NodeSpec]) -> Vec<u8> {
    let mut out = Vec::new();
    for w in [907u32, 7_668_833, 0, 3, roads.len() as u32] {
        out.extend(w.to_le_bytes());
    }
    for (uid, look, start, end) in roads {
        out.extend(3u32.to_le_bytes());
        out.extend(uid.to_le_bytes());
        for v in [0f32, -1.0, -50.0, 0.0, 0.0, 10.0, 1.0, 10.0, 10.0, 10.0] {
            out.extend(v.to_le_bytes());
        }
        out.extend([0u8; 5]);
        let mut p = vec![0u8; 212];
        p[4..12].copy_from_slice(&token(look).to_le_bytes());
        p[192..200].copy_from_slice(&start.to_le_bytes());
        p[200..208].copy_from_slice(&end.to_le_bytes());
        p[208..212].copy_from_slice(&10f32.to_le_bytes());
        out.extend(p);
    }
    out.extend((nodes.len() as u32).to_le_bytes());
    for (uid, z, back, fwd, countries) in nodes {
        out.extend(uid.to_le_bytes());
        for v in [0i32, 0, z * 256] {
            out.extend(v.to_le_bytes());
        }
        for q in [1f32, 0.0, 0.0, 0.0] {
            out.extend(q.to_le_bytes());
        }
        out.extend(back.to_le_bytes());
        out.extend(fwd.to_le_bytes());
        let flags = (u32::from(countries[0]) << 8) | (u32::from(countries[1]) << 16);
        out.extend(flags.to_le_bytes());
    }
    out.extend(0u32.to_le_bytes());
    out
}

/// Synthetic definitions: lane types, two countries (1 right-hand, 2
/// left-hand) and road looks covering the cases under test.
const DEFINITIONS: &str = r#"SiiNunit
{
traffic_lane_data : lane.a {
 rank: 1
}
traffic_lane_data : lane.b {
 rank: 2
}
country_data : country.data.r {
 country_id: 1
}
country_data : country.data.l {
 country_id: 2
 leftside_traffic: true
}
road_look : road.two {
 road_size_left: 4.5
 road_size_right: 4.5
 lanes_left[]: lane.a
 lanes_right[]: lane.a
 lanes_right[]: lane.b
 lane_offsets_right[]: (1, 2)
 lane_offsets_right[]: (3, 4)
}
road_look : road.lh {
 lane_direction: left_hand
 lanes_right[]: lane.a
}
road_look : road.typed {
 lanes_right[]: lane.a
 template_variants_right[]: .v.types
}
road_look : road.counted {
 lanes_right[]: lane.a
 lane_variants[]: .v.count
}
road_look : road.novar {
 lanes_right[]: lane.a
 lane_variants[]: .v.gone
}
road_look : road.badlane {
 lanes_right[]: lane.gone
}
road_look : road.offs {
 lanes_right[]: lane.a
 lane_offsets_right[]: (1, 2)
 lane_offsets_right[]: (3, 4)
}
road_template_variant : .v.types {
 lanes_right[]: lane.b
}
road_template_variant : .v.count {
 lanes_right[]: lane.a
 lanes_right[]: lane.a
}
}
"#;

fn definitions() -> LaneDefinitions {
    let units = parse_sii(DEFINITIONS.as_bytes(), &mut |_| None).expect("valid SII");
    LaneDefinitions::from_units(&units).expect("consistent definitions")
}

/// Builds a one-road network: road 10 with `look` from node 1 to node 2,
/// whose flags carry `start` and `end` country IDs.
fn one_road(look: &'static str, start: [u8; 2], end: [u8; 2]) -> LaneNetwork {
    let bytes = sector(
        &[(10, look, 1, 2)],
        &[(1, 0, 0, 10, start), (2, -10, 10, 0, end)],
    );
    let sectors = [parse_base(&bytes).expect("valid sector")];
    let network = build_road_network(&sectors).expect("valid road network");
    build_lane_network(&network, &sectors, &definitions()).expect("valid lane network")
}

#[test]
fn resolves_lanes_types_offsets_and_right_hand_traffic() {
    let net = one_road("two", [1, 1], [1, 1]);
    let road = net.road(RoadId(10)).expect("entry");
    assert_eq!(
        road.traffic_side,
        TrafficSideState::Known(TrafficSide::Right)
    );
    assert_eq!(
        (road.stored_size_left, road.stored_size_right),
        (Some(4.5), Some(4.5))
    );
    assert_eq!(road.offsets_left, OffsetsState::NotStored);
    assert_eq!(road.offsets_right, OffsetsState::Assigned);
    let lanes = road.lanes();
    assert_eq!(lanes.len(), 3);
    assert_eq!((lanes[0].id.side, lanes[0].id.index), (RoadSide::Left, 0));
    assert_eq!(lanes[2].lane_type, LaneType::Known("lane.b".to_owned()));
    assert_eq!(
        lanes[2].offset,
        Some(StoredLaneOffset {
            first: 3.0,
            second: 4.0
        })
    );
    assert_eq!(lanes[0].offset, None);
    assert!(lanes.iter().all(|l| l.width.is_none()));
    assert_eq!(road.lanes_in(LaneDirection::Forward).len(), 2);
    assert_eq!(road.lanes_in(LaneDirection::Backward).len(), 1);
}

#[test]
fn left_hand_countries_mirror_the_directions() {
    let net = one_road("two", [2, 2], [2, 2]);
    let road = net.road(RoadId(10)).expect("entry");
    assert_eq!(
        road.traffic_side,
        TrafficSideState::Known(TrafficSide::Left)
    );
    assert_eq!(road.direction(RoadSide::Left), Some(LaneDirection::Forward));
    assert_eq!(road.lanes_in(LaneDirection::Forward).len(), 1);
}

#[test]
fn traffic_side_is_unknown_without_a_clear_country() {
    let side = |start, end| {
        one_road("two", start, end)
            .road(RoadId(10))
            .expect("entry")
            .traffic_side
    };
    assert_eq!(
        side([0, 0], [0, 0]),
        TrafficSideState::Unknown(TrafficSideIssue::NoCountry)
    );
    assert_eq!(
        side([1, 1], [2, 2]),
        TrafficSideState::Unknown(TrafficSideIssue::CountryMismatch)
    );
    assert_eq!(
        side([1, 2], [1, 1]),
        TrafficSideState::Unknown(TrafficSideIssue::CountryMismatch)
    );
    assert_eq!(
        side([9, 9], [9, 9]),
        TrafficSideState::Unknown(TrafficSideIssue::UnknownCountry(9))
    );
    // One node without a country is fine if the other names one.
    assert_eq!(
        side([0, 0], [1, 1]),
        TrafficSideState::Known(TrafficSide::Right)
    );
}

#[test]
fn a_look_traffic_side_that_contradicts_the_country_is_a_conflict() {
    let conflict = one_road("lh", [1, 1], [1, 1]);
    assert_eq!(
        conflict.road(RoadId(10)).expect("entry").traffic_side,
        TrafficSideState::Unknown(TrafficSideIssue::LookConflict)
    );
    let agree = one_road("lh", [2, 2], [2, 2]);
    assert_eq!(
        agree.road(RoadId(10)).expect("entry").traffic_side,
        TrafficSideState::Known(TrafficSide::Left)
    );
}

#[test]
fn variants_make_types_uncertain_or_lanes_unresolved() {
    let typed = one_road("typed", [1, 1], [1, 1]);
    assert_eq!(
        typed.road(RoadId(10)).expect("entry").lanes()[0].lane_type,
        LaneType::Uncertain("lane.a".to_owned())
    );
    let counted = one_road("counted", [1, 1], [1, 1]);
    assert_eq!(
        counted.road(RoadId(10)).expect("entry").config,
        LaneConfig::Unresolved(LaneIssue::VariantChangesLaneCount)
    );
}

#[test]
fn missing_definitions_leave_lanes_unresolved() {
    let config = |look| {
        one_road(look, [1, 1], [1, 1])
            .road(RoadId(10))
            .expect("entry")
            .config
            .clone()
    };
    assert_eq!(
        config("nothere"),
        LaneConfig::Unresolved(LaneIssue::MissingRoadLook("road.nothere".to_owned()))
    );
    assert_eq!(
        config("badlane"),
        LaneConfig::Unresolved(LaneIssue::MissingLaneType("lane.gone".to_owned()))
    );
    assert_eq!(
        config("novar"),
        LaneConfig::Unresolved(LaneIssue::MissingVariant(".v.gone".to_owned()))
    );
}

#[test]
fn offsets_are_not_assigned_when_their_count_differs() {
    let net = one_road("offs", [1, 1], [1, 1]);
    let road = net.road(RoadId(10)).expect("entry");
    assert_eq!(
        road.offsets_right,
        OffsetsState::CountMismatch {
            offsets: 2,
            lanes: 1
        }
    );
    assert_eq!(road.lanes()[0].offset, None);
}

#[test]
fn connected_roads_with_matching_lanes_get_candidates() {
    let bytes = sector(
        &[(10, "two", 1, 2), (11, "two", 2, 3)],
        &[
            (1, 0, 0, 10, [1, 1]),
            (2, -10, 10, 11, [1, 1]),
            (3, -20, 11, 0, [1, 1]),
        ],
    );
    let sectors = [parse_base(&bytes).expect("valid sector")];
    let network = build_road_network(&sectors).expect("valid");
    let lanes = build_lane_network(&network, &sectors, &definitions()).expect("valid");
    let connection = network.successor(RoadId(10)).expect("connected");
    let LaneTransition::IndexCandidates(pairs) = lanes.transition(&connection) else {
        panic!("candidates expected");
    };
    // Two forward lanes into road 11, one backward lane from road 11.
    assert_eq!(pairs.len(), 3);
}

#[test]
fn inconsistent_definitions_are_rejected() {
    let parse = |text: &str| parse_sii(text.as_bytes(), &mut |_| None).expect("valid SII");
    let dup = parse("SiiNunit\n{\nroad_look : road.a {\n}\nroad_look : road.a {\n}\n}\n");
    assert!(matches!(
        LaneDefinitions::from_units(&dup),
        Err(DefinitionsError::Duplicate { .. })
    ));
    let dir = parse("SiiNunit\n{\nroad_look : road.a {\n lane_direction: sideways\n}\n}\n");
    assert!(matches!(
        LaneDefinitions::from_units(&dir),
        Err(DefinitionsError::UnknownLaneDirection { .. })
    ));
    let country = parse("SiiNunit\n{\ncountry_data : c.a {\n leftside_traffic: true\n}\n}\n");
    assert!(matches!(
        LaneDefinitions::from_units(&country),
        Err(DefinitionsError::Invalid(_))
    ));
    let same_id = parse(
        "SiiNunit\n{\ncountry_data : c.a {\n country_id: 1\n}\ncountry_data : c.b {\n country_id: 1\n}\n}\n",
    );
    assert!(matches!(
        LaneDefinitions::from_units(&same_id),
        Err(DefinitionsError::Duplicate { .. })
    ));
}
