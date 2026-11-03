//! Building the road model from parsed sector records.

use std::collections::HashSet;

use roadpilot_map_formats::{BaseSector, ItemData};
use roadpilot_map_model::{
    ModelError, Node, NodeId, NodeLink, Position, Road, RoadId, RoadNetwork, Rotation,
};

/// Builds a validated [`RoadNetwork`] from parsed `.base` sectors.
///
/// Pass every sector whose roads should be included, and every sector that
/// holds their nodes: nodes of roads that cross a sector border can be
/// stored in a neighbouring sector. Each node slot is classified as a road,
/// another item found in `sectors`, or an unresolved UID.
///
/// # Errors
///
/// The [`ModelError`] of [`RoadNetwork::new`], for example a road whose
/// node is in none of the given sectors.
pub fn build_road_network(sectors: &[BaseSector<'_>]) -> Result<RoadNetwork, ModelError> {
    let item_count = sectors.iter().map(|s| s.items.len()).sum();
    let mut items = HashSet::with_capacity(item_count);
    let mut road_ids = HashSet::new();
    let mut roads = Vec::new();
    for item in sectors.iter().flat_map(|s| &s.items) {
        items.insert(item.uid);
        if let ItemData::Road(road) = &item.data {
            road_ids.insert(item.uid);
            roads.push(Road {
                id: RoadId(item.uid),
                start_node: NodeId(road.start_node),
                end_node: NodeId(road.end_node),
                look_token: road.look.0,
                stored_length: road.length,
            });
        }
    }
    let link = |uid: u64| match uid {
        0 => NodeLink::None,
        _ if road_ids.contains(&uid) => NodeLink::Road(RoadId(uid)),
        _ if items.contains(&uid) => NodeLink::Item(uid),
        _ => NodeLink::Unresolved(uid),
    };
    let nodes = sectors.iter().flat_map(|s| &s.nodes).map(|n| Node {
        id: NodeId(n.uid),
        position: Position { fixed: n.position },
        rotation: Rotation {
            w: n.rotation[0],
            x: n.rotation[1],
            y: n.rotation[2],
            z: n.rotation[3],
        },
        backward: link(n.backward_item),
        forward: link(n.forward_item),
    });
    RoadNetwork::new(nodes, roads)
}
