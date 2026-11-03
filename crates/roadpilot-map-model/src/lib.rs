//! Internal map model of RoadPilot-Map.
//!
//! Plain data types without I/O and without dependencies. The model is
//! built from parsed game records by the `roadpilot-map` facade.
//!
//! So far it holds **roads and nodes** (Phase 1, Step 4A): road segments,
//! their start and end nodes, node positions and rotations, the
//! connections that follow from the node references, and the confirmed
//! geometry of each road. Coordinates stay in the SCS world system.
//!
//! It also holds the **lanes** of each road (Phase 1, Step 4B): lane sides,
//! types, driving directions where the traffic side is known, stored offsets,
//! and candidate lane transitions between connected roads. Unconfirmed
//! values are explicit states, never defaults.
//!
//! Not modelled yet: junctions (prefabs), curves between nodes, lane
//! geometry, speed limits. See `docs/formats/scs-road-geometry.md` and
//! `docs/formats/scs-lane-model.md`.
//!
//! ```
//! use roadpilot_map_model::*;
//!
//! let rotation = Rotation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
//! let node = |id, z: i32, backward, forward| Node {
//!     id: NodeId(id),
//!     position: Position { fixed: [0, 0, z * 256] },
//!     rotation,
//!     backward,
//!     forward,
//! };
//! let road = |id, start, end| Road {
//!     id: RoadId(id),
//!     start_node: NodeId(start),
//!     end_node: NodeId(end),
//!     look_token: 0,
//!     stored_length: 10.0,
//! };
//! let network = RoadNetwork::new(
//!     [
//!         node(1, 0, NodeLink::None, NodeLink::Road(RoadId(10))),
//!         node(2, -10, NodeLink::Road(RoadId(10)), NodeLink::Road(RoadId(11))),
//!         node(3, -20, NodeLink::Road(RoadId(11)), NodeLink::None),
//!     ],
//!     [road(10, 1, 2), road(11, 2, 3)],
//! )?;
//! assert_eq!(network.successor(RoadId(10)).map(|c| c.to), Some(RoadId(11)));
//! # Ok::<(), ModelError>(())
//! ```

mod geometry;
mod lanes;
mod network;

pub use geometry::{Position, RoadGeometry, Rotation};
pub use lanes::{
    Lane, LaneConfig, LaneConnection, LaneDirection, LaneError, LaneId, LaneIssue, LaneNetwork,
    LaneTransition, LaneType, OffsetsState, RoadLanes, RoadSide, StoredLaneOffset, TrafficSide,
    TrafficSideIssue, TrafficSideState, TransitionIssue,
};
pub use network::{
    ModelError, Node, NodeId, NodeLink, NodeRoads, Road, RoadConnection, RoadId, RoadNetwork,
};
