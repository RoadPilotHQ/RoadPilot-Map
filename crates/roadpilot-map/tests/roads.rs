//! Building the road model from synthetic sector files (no game data).

use roadpilot_map::build_road_network;
use roadpilot_map::formats::parse_base;
use roadpilot_map::model::{ModelError, NodeId, NodeLink, RoadId};

/// One item of a synthetic sector: `(type, uid, payload)`.
type ItemSpec = (u32, u64, Vec<u8>);
/// One node: `(uid, [x, y, z] in world units, backward item, forward item)`.
type NodeSpec = (u64, [i32; 3], u64, u64);

/// Builds a `.base` file following `docs/formats/scs-sector-format.md`.
fn sector(items: &[ItemSpec], nodes: &[NodeSpec]) -> Vec<u8> {
    let mut out = Vec::new();
    for w in [907u32, 7_668_833, 0, 3, items.len() as u32] {
        out.extend(w.to_le_bytes());
    }
    for (t, uid, payload) in items {
        out.extend(t.to_le_bytes());
        out.extend(uid.to_le_bytes());
        for v in [0f32, -1.0, 0.0, 0.0, 0.0, 10.0, 1.0, 10.0, 10.0, 10.0] {
            out.extend(v.to_le_bytes());
        }
        out.extend([0u8; 5]);
        out.extend(payload);
    }
    out.extend((nodes.len() as u32).to_le_bytes());
    for (uid, xyz, back, fwd) in nodes {
        out.extend(uid.to_le_bytes());
        for v in xyz {
            out.extend((v * 256).to_le_bytes());
        }
        for q in [1f32, 0.0, 0.0, 0.0] {
            out.extend(q.to_le_bytes());
        }
        out.extend(back.to_le_bytes());
        out.extend(fwd.to_le_bytes());
        out.extend(0u32.to_le_bytes());
    }
    out.extend(0u32.to_le_bytes());
    out
}

fn road(uid: u64, start: u64, end: u64, length: f32) -> ItemSpec {
    let mut p = vec![0u8; 212];
    p[4..12].copy_from_slice(&7u64.to_le_bytes());
    p[192..200].copy_from_slice(&start.to_le_bytes());
    p[200..208].copy_from_slice(&end.to_le_bytes());
    p[208..212].copy_from_slice(&length.to_le_bytes());
    (3, uid, p)
}

/// A cut plane item (type 8) on the given nodes: any non-road item.
fn other_item(uid: u64, nodes: &[u64]) -> ItemSpec {
    let mut p = (nodes.len() as u32).to_le_bytes().to_vec();
    for n in nodes {
        p.extend(n.to_le_bytes());
    }
    (8, uid, p)
}

#[test]
fn builds_roads_across_two_sectors() {
    // Road 10 lies in sector A, its end node 2 is stored in sector B, where
    // road 11 continues. Node 2 is stored in both sectors, identically.
    let a = sector(
        &[road(10, 1, 2, 10.0)],
        &[(1, [0, 0, 0], 0, 10), (2, [0, 0, -10], 10, 11)],
    );
    let b = sector(
        &[road(11, 2, 3, 10.0), other_item(500, &[3])],
        &[(2, [0, 0, -10], 10, 11), (3, [0, 0, -20], 11, 500)],
    );
    let sectors = [
        parse_base(&a).expect("valid sector"),
        parse_base(&b).expect("valid sector"),
    ];
    let net = build_road_network(&sectors).expect("valid network");
    assert_eq!(net.roads().count(), 2);
    assert_eq!(net.nodes().count(), 3);
    assert_eq!(net.successor(RoadId(10)).map(|c| c.to), Some(RoadId(11)));
    // A non-road item is kept as an item link, not mistaken for a road.
    assert_eq!(
        net.node(NodeId(3)).map(|n| n.forward),
        Some(NodeLink::Item(500))
    );
    assert_eq!(net.road(RoadId(10)).map(|r| r.look_token), Some(7));
}

#[test]
fn unresolved_item_references_are_reported_as_such() {
    let s = sector(
        &[road(10, 1, 2, 10.0)],
        &[(1, [0, 0, 0], 999, 10), (2, [0, 0, -10], 10, 0)],
    );
    let sectors = [parse_base(&s).expect("valid sector")];
    let net = build_road_network(&sectors).expect("valid network");
    assert_eq!(
        net.node(NodeId(1)).map(|n| n.backward),
        Some(NodeLink::Unresolved(999))
    );
    assert_eq!(net.node(NodeId(2)).map(|n| n.forward), Some(NodeLink::None));
}

#[test]
fn a_node_in_a_sector_that_was_not_loaded_is_an_error() {
    let a = sector(&[road(10, 1, 2, 10.0)], &[(1, [0, 0, 0], 0, 10)]);
    let sectors = [parse_base(&a).expect("valid sector")];
    assert_eq!(
        build_road_network(&sectors),
        Err(ModelError::MissingNode {
            road: RoadId(10),
            node: NodeId(2)
        })
    );
}

#[test]
fn conflicting_copies_of_a_node_are_an_error() {
    let a = sector(
        &[road(10, 1, 2, 10.0)],
        &[(1, [0, 0, 0], 0, 10), (2, [0, 0, -10], 10, 0)],
    );
    let b = sector(&[], &[(2, [0, 5, -10], 10, 0)]);
    let sectors = [
        parse_base(&a).expect("valid"),
        parse_base(&b).expect("valid"),
    ];
    assert_eq!(
        build_road_network(&sectors),
        Err(ModelError::ConflictingNode { node: NodeId(2) })
    );
}

#[test]
fn stored_length_is_taken_from_the_record() {
    let s = sector(
        &[road(10, 1, 2, 12.75)],
        &[(1, [0, 0, 0], 0, 10), (2, [0, 0, -10], 10, 0)],
    );
    let sectors = [parse_base(&s).expect("valid")];
    let net = build_road_network(&sectors).expect("valid");
    let g = net.geometry(RoadId(10)).expect("road");
    assert_eq!(g.stored_length, 12.75);
    assert_eq!(g.chord_length, 10.0);
}
