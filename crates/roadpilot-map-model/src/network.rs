//! Roads, nodes and the connections that follow from their references.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt;

use crate::geometry::{Position, RoadGeometry, Rotation};

/// UID of a node in the game data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u64);

/// UID of a road item in the game data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RoadId(pub u64);

/// What one of a node's two item slots refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeLink {
    /// The slot is empty (UID 0).
    None,
    /// A road of this network.
    Road(RoadId),
    /// An item that exists but is not a road (for example a prefab).
    /// Such items are not part of the road model yet.
    Item(u64),
    /// A UID that matches no item of the game data that was loaded.
    Unresolved(u64),
}

/// A node: a point where items start or end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub position: Position,
    pub rotation: Rotation,
    /// The item that ends at this node.
    pub backward: NodeLink,
    /// The item that starts at this node.
    pub forward: NodeLink,
}

/// A road segment between two nodes.
///
/// The direction from `start_node` to `end_node` is the direction in which
/// the record is stored. It says nothing about which way the road can be
/// driven; that depends on its lanes (not modelled yet).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Road {
    pub id: RoadId,
    pub start_node: NodeId,
    pub end_node: NodeId,
    /// Token of the road's `road_look` definition. Kept as a reference
    /// only; road looks are not interpreted yet.
    pub look_token: u64,
    /// Length stored in the road record, world units.
    pub stored_length: f32,
}

/// Two roads joined at a node: the end node of `from` is the start node of
/// `to`. This follows the stored direction of both records and does not
/// imply that traffic can pass from one to the other in either direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RoadConnection {
    pub from: RoadId,
    pub via: NodeId,
    pub to: RoadId,
}

/// The roads that end and start at one node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NodeRoads {
    /// The road whose end node this is (incoming in stored direction).
    pub ending: Option<RoadId>,
    /// The road whose start node this is (outgoing in stored direction).
    pub starting: Option<RoadId>,
}

/// Why a [`RoadNetwork`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModelError {
    /// A node or road has the reserved UID 0.
    ZeroId { what: &'static str },
    /// Two nodes share a UID but differ. Identical copies (nodes stored in
    /// two neighbouring sectors) are merged.
    ConflictingNode { node: NodeId },
    /// Two roads share a UID.
    DuplicateRoad { road: RoadId },
    /// A node rotation is not a finite unit quaternion.
    InvalidRotation { node: NodeId },
    /// A road's stored length is not finite and positive.
    InvalidLength { road: RoadId },
    /// A road starts and ends at the same node.
    RoadLoop { road: RoadId },
    /// A road refers to a node that does not exist.
    MissingNode { road: RoadId, node: NodeId },
    /// A road's start or end node does not refer back to the road
    /// (`forward` for the start node, `backward` for the end node).
    LinkMismatch { road: RoadId, node: NodeId },
    /// A node refers to a road that does not exist, or to a road that does
    /// not start or end at it.
    InvalidNodeLink { node: NodeId, road: RoadId },
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroId { what } => write!(f, "a {what} has the UID 0"),
            Self::ConflictingNode { node } => {
                write!(f, "node {:016x} occurs twice with different data", node.0)
            }
            Self::DuplicateRoad { road } => write!(f, "road {:016x} occurs twice", road.0),
            Self::InvalidRotation { node } => {
                write!(f, "node {:016x} has an invalid rotation", node.0)
            }
            Self::InvalidLength { road } => {
                write!(f, "road {:016x} has an invalid stored length", road.0)
            }
            Self::RoadLoop { road } => {
                write!(f, "road {:016x} starts and ends at the same node", road.0)
            }
            Self::MissingNode { road, node } => write!(
                f,
                "road {:016x} refers to missing node {:016x}",
                road.0, node.0
            ),
            Self::LinkMismatch { road, node } => write!(
                f,
                "node {:016x} does not refer back to road {:016x}",
                node.0, road.0
            ),
            Self::InvalidNodeLink { node, road } => write!(
                f,
                "node {:016x} refers to road {:016x}, which does not start or end there",
                node.0, road.0
            ),
        }
    }
}

impl std::error::Error for ModelError {}

/// Validated roads and nodes with their connections.
///
/// Collections are ordered by UID, so iteration is deterministic.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RoadNetwork {
    nodes: BTreeMap<NodeId, Node>,
    roads: BTreeMap<RoadId, Road>,
}

impl RoadNetwork {
    /// Builds and validates a network.
    ///
    /// Every road must have a start and an end node that refer back to it,
    /// and every node slot that names a road must name one that starts or
    /// ends there. Nothing invalid is dropped silently.
    ///
    /// # Errors
    ///
    /// The first [`ModelError`] found.
    pub fn new(
        nodes: impl IntoIterator<Item = Node>,
        roads: impl IntoIterator<Item = Road>,
    ) -> Result<Self, ModelError> {
        let mut node_map = BTreeMap::new();
        for node in nodes {
            if node.id.0 == 0 {
                return Err(ModelError::ZeroId { what: "node" });
            }
            if !node.rotation.is_valid() {
                return Err(ModelError::InvalidRotation { node: node.id });
            }
            match node_map.entry(node.id) {
                Entry::Vacant(e) => {
                    e.insert(node);
                }
                Entry::Occupied(e) if *e.get() == node => {}
                Entry::Occupied(_) => return Err(ModelError::ConflictingNode { node: node.id }),
            }
        }
        let mut road_map = BTreeMap::new();
        for road in roads {
            if road.id.0 == 0 {
                return Err(ModelError::ZeroId { what: "road" });
            }
            if !(road.stored_length.is_finite() && road.stored_length > 0.0) {
                return Err(ModelError::InvalidLength { road: road.id });
            }
            if road.start_node == road.end_node {
                return Err(ModelError::RoadLoop { road: road.id });
            }
            if road_map.insert(road.id, road).is_some() {
                return Err(ModelError::DuplicateRoad { road: road.id });
            }
        }
        let network = Self {
            nodes: node_map,
            roads: road_map,
        };
        network.validate()?;
        Ok(network)
    }

    fn validate(&self) -> Result<(), ModelError> {
        for road in self.roads.values() {
            for (node_id, is_start) in [(road.start_node, true), (road.end_node, false)] {
                let node = self.nodes.get(&node_id).ok_or(ModelError::MissingNode {
                    road: road.id,
                    node: node_id,
                })?;
                let slot = if is_start {
                    node.forward
                } else {
                    node.backward
                };
                if slot != NodeLink::Road(road.id) {
                    return Err(ModelError::LinkMismatch {
                        road: road.id,
                        node: node_id,
                    });
                }
            }
        }
        for node in self.nodes.values() {
            for (slot, is_forward) in [(node.forward, true), (node.backward, false)] {
                let NodeLink::Road(road_id) = slot else {
                    continue;
                };
                let ok = self.roads.get(&road_id).is_some_and(|r| {
                    if is_forward {
                        r.start_node == node.id
                    } else {
                        r.end_node == node.id
                    }
                });
                if !ok {
                    return Err(ModelError::InvalidNodeLink {
                        node: node.id,
                        road: road_id,
                    });
                }
            }
        }
        Ok(())
    }

    /// All nodes, ordered by UID.
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    /// All roads, ordered by UID.
    pub fn roads(&self) -> impl Iterator<Item = &Road> {
        self.roads.values()
    }

    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    #[must_use]
    pub fn road(&self, id: RoadId) -> Option<&Road> {
        self.roads.get(&id)
    }

    /// The roads that end and start at `node`, or `None` if the node does
    /// not exist. A node has one slot per direction, so there is at most one
    /// of each.
    #[must_use]
    pub fn roads_at(&self, node: NodeId) -> Option<NodeRoads> {
        let n = self.nodes.get(&node)?;
        let road = |link| match link {
            NodeLink::Road(r) => Some(r),
            _ => None,
        };
        Some(NodeRoads {
            ending: road(n.backward),
            starting: road(n.forward),
        })
    }

    /// The road that continues `road` at its end node, if that is a road.
    #[must_use]
    pub fn successor(&self, road: RoadId) -> Option<RoadConnection> {
        let r = self.roads.get(&road)?;
        let to = self.roads_at(r.end_node)?.starting?;
        Some(RoadConnection {
            from: road,
            via: r.end_node,
            to,
        })
    }

    /// The road that leads into `road` at its start node, if that is a road.
    #[must_use]
    pub fn predecessor(&self, road: RoadId) -> Option<RoadConnection> {
        let r = self.roads.get(&road)?;
        let from = self.roads_at(r.start_node)?.ending?;
        Some(RoadConnection {
            from,
            via: r.start_node,
            to: road,
        })
    }

    /// Every road-to-road connection, ordered by the `from` road.
    pub fn connections(&self) -> impl Iterator<Item = RoadConnection> + '_ {
        self.roads.keys().filter_map(|&id| self.successor(id))
    }

    /// The confirmed geometry of `road`.
    #[must_use]
    pub fn geometry(&self, road: RoadId) -> Option<RoadGeometry> {
        let r = self.roads.get(&road)?;
        let a = self.nodes.get(&r.start_node)?;
        let b = self.nodes.get(&r.end_node)?;
        Some(RoadGeometry::new(
            (a.position, a.rotation),
            (b.position, b.rotation),
            r.stored_length,
        ))
    }
}
