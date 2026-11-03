//! Building the prefab model from synthetic sectors, definitions and
//! descriptors (no game data).
//!
//! Prefab 100 (model `prefab.x`) has two nodes: road 10 ends at node 1 and
//! road 20 starts at node 2. Its descriptor has one path from descriptor
//! node 0 to descriptor node 1 (curves 0 -> 1, with traffic rule `r`) and
//! one path back (curve 2).

use std::cell::Cell;

use roadpilot_map::formats::parse_base;
use roadpilot_map::formats::sii::parse_sii;
use roadpilot_map::model::{
    ArmLink, MovementStatus, PrefabId, PrefabNetwork, PrefabResolution, RoadId, RoadNetwork,
    RoadSide, RuleRef,
};
use roadpilot_map::{
    DefinitionsError, LaneDefinitions, PrefabDefinitions, build_lane_network, build_prefab_network,
    build_road_network,
};

const P: PrefabId = PrefabId(100);

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

fn header(out: &mut Vec<u8>, item_type: u32, uid: u64) {
    out.extend(item_type.to_le_bytes());
    out.extend(uid.to_le_bytes());
    for v in [0f32, -1.0, -50.0, 0.0, 0.0, 10.0, 1.0, 10.0, 10.0, 10.0] {
        out.extend(v.to_le_bytes());
    }
    out.extend([0u8; 5]);
}

/// A sector with roads 10 (nodes 11 to 1) and 20 (nodes 2 to 21) and
/// prefab 100 on nodes `prefab_nodes` with `origin`. Node `(uid, z)`.
fn sector(prefab_nodes: &[u64], origin: u8, node_z: &[(u64, i32)]) -> Vec<u8> {
    let mut out = Vec::new();
    for w in [907u32, 7_668_833, 0, 3, 3] {
        out.extend(w.to_le_bytes());
    }
    for (uid, start, end) in [(10u64, 11u64, 1u64), (20, 2, 21)] {
        header(&mut out, 3, uid);
        let mut p = vec![0u8; 212];
        p[4..12].copy_from_slice(&token("look").to_le_bytes());
        p[192..200].copy_from_slice(&start.to_le_bytes());
        p[200..208].copy_from_slice(&end.to_le_bytes());
        p[208..212].copy_from_slice(&10f32.to_le_bytes());
        out.extend(p);
    }
    header(&mut out, 4, P.0);
    out.extend(token("x").to_le_bytes());
    out.extend([0u8; 8]);
    out.extend(0u32.to_le_bytes());
    out.extend((prefab_nodes.len() as u32).to_le_bytes());
    for n in prefab_nodes {
        out.extend(n.to_le_bytes());
    }
    out.extend(0u32.to_le_bytes());
    out.extend([0u8; 8]);
    out.push(origin);
    out.push(0);
    out.resize(out.len() + 12 * prefab_nodes.len() + 8, 0);

    let link = |uid: u64| match uid {
        11 => (0, 10),
        1 => (10, P.0),
        2 => (P.0, 20),
        21 => (20, 0),
        _ => (P.0, 0),
    };
    out.extend((node_z.len() as u32).to_le_bytes());
    for &(uid, z) in node_z {
        out.extend(uid.to_le_bytes());
        for v in [0i32, 0, z * 256] {
            out.extend(v.to_le_bytes());
        }
        for q in [1f32, 0.0, 0.0, 0.0] {
            out.extend(q.to_le_bytes());
        }
        let (back, fwd) = link(uid);
        out.extend(back.to_le_bytes());
        out.extend(fwd.to_le_bytes());
        // Country 1 on both sides of every node.
        out.extend(((1u32 << 8) | (1 << 16)).to_le_bytes());
    }
    out.extend(0u32.to_le_bytes());
    out
}

const NODES: [(u64, i32); 4] = [(11, 10), (1, 0), (2, -20), (21, -30)];

/// A descriptor with nodes at z = 0 and z = -20 (see the module docs).
fn descriptor() -> Vec<u8> {
    let sizes = [104usize, 132, 52, 84, 36, 12, 12, 8, 48, 48, 16, 188];
    let (nodes, curves) = (2u32, 3u32);
    let mut out = Vec::new();
    out.extend(25u32.to_le_bytes());
    let mut counts = [0u32; 11];
    counts[0] = nodes;
    counts[1] = curves;
    for c in counts {
        out.extend(c.to_le_bytes());
    }
    let mut offset = 0x60u32;
    for (k, size) in sizes.iter().enumerate() {
        out.extend(offset.to_le_bytes());
        let count = match k {
            0 => nodes,
            1 => curves,
            _ => 0,
        };
        offset += count * *size as u32;
    }
    let put = |r: &mut Vec<u8>, at: usize, v: [u8; 4]| r[at..at + 4].copy_from_slice(&v);
    // Node 0: input slot 0 = curve 0, output slot 0 = curve 2.
    // Node 1: input slot 0 = curve 2, output slot 0 = curve 1.
    for (z, input, output) in [(0f32, 0i32, 2i32), (-20.0, 2, 1)] {
        let mut r = vec![0u8; 104];
        put(&mut r, 0x18, z.to_le_bytes());
        for i in 0..16 {
            put(&mut r, 0x28 + 4 * i, (-1i32).to_le_bytes());
        }
        put(&mut r, 0x28, input.to_le_bytes());
        put(&mut r, 0x48, output.to_le_bytes());
        out.extend(r);
    }
    // Curve 0 -> 1 carries rule `r`; curve 2 stands alone.
    for (next, prev, rule) in [(1i32, -1i32, token("r")), (-1, 0, 0), (-1, -1, 0)] {
        let mut r = vec![0u8; 132];
        for k in 0..8 {
            put(&mut r, 76 + 4 * k, (-1i32).to_le_bytes());
        }
        put(&mut r, 76, next.to_le_bytes());
        put(&mut r, 92, prev.to_le_bytes());
        put(&mut r, 108, u32::from(next >= 0).to_le_bytes());
        put(&mut r, 112, u32::from(prev >= 0).to_le_bytes());
        r[120..128].copy_from_slice(&rule.to_le_bytes());
        out.extend(r);
    }
    out
}

const DEFINITIONS: &str = r#"SiiNunit
{
prefab_model : prefab.x {
 prefab_desc: "/prefab/x.ppd"
}
traffic_rule_data : traffic_rule.r {
 rule: some_rule
}
}
"#;

fn definitions() -> PrefabDefinitions {
    let units = parse_sii(DEFINITIONS.as_bytes(), &mut |_| None).expect("valid SII");
    PrefabDefinitions::from_units(&units).expect("consistent definitions")
}

fn build(bytes: &[u8], ppd: Option<Vec<u8>>) -> (RoadNetwork, PrefabNetwork) {
    let sectors = [parse_base(bytes).expect("valid sector")];
    let network = build_road_network(&sectors).expect("valid road network");
    let loads = Cell::new(0);
    let prefabs = build_prefab_network(&network, &sectors, &definitions(), |path| {
        assert_eq!(path, "prefab/x.ppd");
        loads.set(loads.get() + 1);
        ppd.clone()
    })
    .expect("valid prefab network");
    assert!(loads.get() <= 1, "each descriptor is loaded once");
    (network, prefabs)
}

#[test]
fn builds_a_resolved_prefab_with_arms_and_movements() {
    let (network, prefabs) = build(&sector(&[1, 2], 0, &NODES), Some(descriptor()));
    let inst = prefabs.instance(P).expect("instance");
    assert_eq!(inst.resolution, PrefabResolution::Resolved);
    assert_eq!(inst.model, "prefab.x");
    let arms = prefabs.arms(&network, P);
    assert_eq!(
        arms.iter().map(|a| a.link).collect::<Vec<_>>(),
        [
            ArmLink::Road {
                road: RoadId(10),
                road_ends_here: true
            },
            ArmLink::Road {
                road: RoadId(20),
                road_ends_here: false
            }
        ]
    );
    let movements = prefabs.movements(P);
    let ends: Vec<_> = movements.iter().map(|m| (m.from_arm, m.to_arm)).collect();
    assert_eq!(ends, [(0, 1), (1, 0)]);
    assert_eq!(
        movements[0].traffic_rules,
        [RuleRef::Resolved("traffic_rule.r".to_owned())]
    );
    assert!(movements[1].traffic_rules.is_empty());
}

#[test]
fn origin_rotates_the_node_mapping() {
    // Stored the other way round: instance node 0 is node 2, origin 1.
    let (_, prefabs) = build(&sector(&[2, 1], 1, &NODES), Some(descriptor()));
    let inst = prefabs.instance(P).expect("instance");
    assert_eq!(inst.resolution, PrefabResolution::Resolved);
    assert_eq!(inst.definition_node(0), Some(1));
    let ends: Vec<_> = prefabs
        .movements(P)
        .iter()
        .map(|m| (m.from_arm, m.to_arm))
        .collect();
    assert_eq!(ends, [(0, 1), (1, 0)]);
}

#[test]
fn unresolvable_prefabs_keep_the_reason() {
    let bytes = sector(&[1, 2], 0, &NODES);
    let (_, prefabs) = build(&bytes, None);
    assert_eq!(
        prefabs.instance(P).map(|i| i.resolution.clone()),
        Some(PrefabResolution::MissingDescriptor(
            "prefab/x.ppd".to_owned()
        ))
    );
    assert!(prefabs.movements(P).is_empty());

    let (_, prefabs) = build(&bytes, Some(vec![1, 2, 3]));
    assert!(matches!(
        prefabs.instance(P).map(|i| &i.resolution),
        Some(PrefabResolution::InvalidDescriptor(_))
    ));

    let (_, prefabs) = build(&sector(&[1, 2], 2, &NODES), Some(descriptor()));
    assert_eq!(
        prefabs.instance(P).map(|i| i.resolution.clone()),
        Some(PrefabResolution::OriginOutOfRange(2))
    );

    let three = [(11, 10), (1, 0), (2, -20), (21, -30), (3, -40)];
    let (_, prefabs) = build(&sector(&[1, 2, 3], 0, &three), Some(descriptor()));
    assert_eq!(
        prefabs.instance(P).map(|i| i.resolution.clone()),
        Some(PrefabResolution::NodeCountMismatch {
            instance: 3,
            definition: 2
        })
    );
}

#[test]
fn a_mapping_that_breaks_node_distances_is_not_trusted() {
    // Node 2 at 30 units from node 1, while the descriptor has 20.
    let moved = [(11, 10), (1, 0), (2, -30), (21, -40)];
    let (_, prefabs) = build(&sector(&[1, 2], 0, &moved), Some(descriptor()));
    assert_eq!(
        prefabs.instance(P).map(|i| i.resolution.clone()),
        Some(PrefabResolution::NodeGeometryMismatch)
    );
    assert!(prefabs.movements(P).is_empty());
}

#[test]
fn a_missing_model_is_kept_as_unresolved() {
    let bytes = sector(&[1, 2], 0, &NODES);
    let sectors = [parse_base(&bytes).expect("valid sector")];
    let network = build_road_network(&sectors).expect("valid road network");
    let prefabs = build_prefab_network(&network, &sectors, &PrefabDefinitions::default(), |_| {
        panic!("no descriptor is loaded without a model")
    })
    .expect("valid");
    assert_eq!(
        prefabs.instance(P).map(|i| i.resolution.clone()),
        Some(PrefabResolution::MissingModel)
    );
}

#[test]
fn an_unknown_rule_is_kept_as_unresolved() {
    let units = parse_sii(
        b"SiiNunit\n{\nprefab_model : prefab.x {\n prefab_desc: \"/prefab/x.ppd\"\n}\n}\n",
        &mut |_| None,
    )
    .expect("valid SII");
    let defs = PrefabDefinitions::from_units(&units).expect("consistent");
    let bytes = sector(&[1, 2], 0, &NODES);
    let sectors = [parse_base(&bytes).expect("valid sector")];
    let network = build_road_network(&sectors).expect("valid road network");
    let prefabs =
        build_prefab_network(&network, &sectors, &defs, |_| Some(descriptor())).expect("valid");
    assert_eq!(
        prefabs.movements(P)[0].traffic_rules,
        [RuleRef::Unresolved("traffic_rule.r".to_owned())]
    );
}

#[test]
fn duplicate_definitions_are_rejected() {
    let text = "SiiNunit\n{\nprefab_model : prefab.x {\n prefab_desc: \"/a.ppd\"\n}\nprefab_model : prefab.x {\n prefab_desc: \"/b.ppd\"\n}\n}\n";
    let units = parse_sii(text.as_bytes(), &mut |_| None).expect("valid SII");
    assert!(matches!(
        PrefabDefinitions::from_units(&units),
        Err(DefinitionsError::Duplicate {
            class: "prefab_model",
            ..
        })
    ));
    let units = parse_sii(
        b"SiiNunit\n{\nprefab_model : prefab.y {\n}\n}\n",
        &mut |_| None,
    )
    .expect("valid SII");
    assert!(matches!(
        PrefabDefinitions::from_units(&units),
        Err(DefinitionsError::Invalid(_))
    ));
}

#[test]
fn road_lanes_through_the_prefab_are_candidates() {
    let lane_units = parse_sii(
        b"SiiNunit
{
traffic_lane_data : lane.a {
}
country_data : country.data.r {
 country_id: 1
}
road_look : road.look {
 lanes_left[]: lane.a
 lanes_right[]: lane.a
}
}
",
        &mut |_| None,
    )
    .expect("valid SII");
    let lane_defs = LaneDefinitions::from_units(&lane_units).expect("consistent");
    let bytes = sector(&[1, 2], 0, &NODES);
    let sectors = [parse_base(&bytes).expect("valid sector")];
    let network = build_road_network(&sectors).expect("valid road network");
    let lanes = build_lane_network(&network, &sectors, &lane_defs).expect("valid lanes");
    let prefabs = build_prefab_network(&network, &sectors, &definitions(), |_| Some(descriptor()))
        .expect("valid");
    let result = prefabs.lane_movements(&network, &lanes, P);
    let pairs: Vec<_> = result
        .movements
        .iter()
        .map(|m| ((m.from.road, m.from.side), (m.to.road, m.to.side), m.status))
        .collect();
    // Right-hand traffic: road 10 ends at the prefab and enters on its right
    // lane; road 20 starts there and leaves on its right lane.
    assert_eq!(
        pairs,
        [
            (
                (RoadId(10), RoadSide::Right),
                (RoadId(20), RoadSide::Right),
                MovementStatus::Candidate
            ),
            (
                (RoadId(20), RoadSide::Left),
                (RoadId(10), RoadSide::Left),
                MovementStatus::Candidate
            ),
        ]
    );
    assert!(result.unpaired_arms.is_empty());
}
