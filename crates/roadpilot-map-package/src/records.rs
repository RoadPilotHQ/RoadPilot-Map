//! The records of the geometry file of a map package.
//!
//! Plain data. Ids are the UIDs of the game data (see the format
//! specification for their stability). Nothing here is read from or written
//! to a file; see `write` and `read`.

use roadpilot_map_model::Quality;

/// Which side of the stored road direction (start to end node).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }

    pub(crate) fn letter(self) -> char {
        match self {
            Self::Left => 'l',
            Self::Right => 'r',
        }
    }
}

/// The stable text id of a lane: the road UID in lower-case hexadecimal, a
/// colon, `l` or `r` for the side and the lane index, for example
/// `25249798d2000001:r0`.
#[must_use]
pub fn lane_id(road: u64, side: Side, index: u16) -> String {
    format!("{road:x}:{}{index}", side.letter())
}

/// What a node slot refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkRecord {
    /// Nothing.
    None,
    /// A road of this package.
    Road(u64),
    /// An existing item that is not a road, for example a prefab.
    Item(u64),
    /// A UID that matches no loaded item.
    Unresolved(u64),
}

/// A map node. Position and rotation are the stored values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeRecord {
    pub id: u64,
    /// Fixed point, divide by the position scale of the manifest. Quality:
    /// confirmed.
    pub position: [i32; 3],
    /// Unit quaternion `(w, x, y, z)` as stored. How it gives a travel
    /// direction is documented in the specification (local -Z axis, a
    /// near-confirmed convention).
    pub rotation: [f32; 4],
    pub backward: LinkRecord,
    pub forward: LinkRecord,
}

/// Direction of travel in a lane relative to the stored road direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneDirectionRecord {
    Forward,
    Backward,
}

/// One lane of a road.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneRecord {
    pub side: Side,
    /// Index on its side, in the definition order of the road look (whether
    /// 0 is the inner lane is unknown).
    pub index: u16,
    pub lane_type: String,
    /// `false` if the lane type depends on a road look variant that the
    /// data does not let us choose.
    pub type_certain: bool,
    /// `None` if the traffic side is unknown.
    pub direction: Option<LaneDirectionRecord>,
    /// The stored offset pair, unit and meaning unknown. `None` if not
    /// stored or not assignable.
    pub offset: Option<[f64; 2]>,
    /// Quality of the lane's position data. [`Quality::Unavailable`] in
    /// schema 1.0: no lane width is stored.
    pub geometry: Quality,
}

/// The lanes of a road.
#[derive(Debug, Clone, PartialEq)]
pub enum LanesRecord {
    Resolved(Vec<LaneRecord>),
    /// The lanes could not be determined; the text is a reason code.
    Unresolved(String),
}

/// Traffic side of a road.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrafficSideRecord {
    Right,
    Left,
    /// Unknown, with a reason code.
    Unknown(String),
}

/// The curve of a road between its nodes.
#[derive(Debug, Clone, PartialEq)]
pub enum CurveRecord {
    /// A cubic Hermite curve through the node positions with the node
    /// directions as tangent directions and `tangent_scale` as tangent
    /// length (model `hermite_stored_length`). Quality: estimated.
    Estimated {
        tangent_scale: f64,
        estimated_length: f64,
        /// `|estimated_length - stored_length| / stored_length`.
        length_deviation: f64,
        /// `length_deviation` is within the manifest's tolerance.
        length_consistent: bool,
        /// `[min_x, min_y, min_z, max_x, max_y, max_z]` in world units.
        /// Contains the estimated curve. Not a bound of the game's curve.
        bounds: [f64; 6],
    },
    /// No curve could be estimated; the text is a reason code.
    Unavailable(String),
}

impl CurveRecord {
    /// The quality of the curve.
    #[must_use]
    pub fn quality(&self) -> Quality {
        match self {
            Self::Estimated { .. } => Quality::Estimated,
            Self::Unavailable(_) => Quality::Unavailable,
        }
    }
}

/// A road.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadRecord {
    pub id: u64,
    pub start: u64,
    pub end: u64,
    /// The road look name, for example `road.<token>`.
    pub look: String,
    /// The stored length, world units (unit unconfirmed). Quality:
    /// confirmed as a stored value.
    pub stored_length: f32,
    pub traffic_side: TrafficSideRecord,
    pub lanes: LanesRecord,
    pub curve: CurveRecord,
}

/// What is attached to a junction node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmLinkRecord {
    Road { road: u64, road_ends_here: bool },
    Prefab(u64),
    Item(u64),
    Unresolved(u64),
    None,
}

/// A node of a junction and what is attached to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmRecord {
    /// Index of the node in the junction's node list.
    pub index: u32,
    pub node: u64,
    pub link: ArmLinkRecord,
}

/// How well the data supports a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    /// Stored explicitly in the game data.
    Confirmed,
    /// Plausible, but part of it is an assumption.
    Candidate,
    /// The data does not say whether it exists.
    Unknown,
    /// Excluded by explicit data.
    Forbidden,
}

impl ConnectionStatus {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Candidate => "candidate",
            Self::Unknown => "unknown",
            Self::Forbidden => "forbidden",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "confirmed" => Some(Self::Confirmed),
            "candidate" => Some(Self::Candidate),
            "unknown" => Some(Self::Unknown),
            "forbidden" => Some(Self::Forbidden),
            _ => None,
        }
    }
}

/// A lane movement stored inside a junction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovementRecord {
    pub from_arm: u32,
    pub from_slot: u32,
    pub to_arm: u32,
    pub to_slot: u32,
    pub status: ConnectionStatus,
}

/// A junction: a prefab instance on the map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JunctionRecord {
    pub id: u64,
    /// The prefab model name, `prefab.<token>`.
    pub model: String,
    /// `None` if the junction is tied to its definition; otherwise a reason
    /// code. Unresolved junctions have no movements.
    pub unresolved: Option<String>,
    pub nodes: Vec<u64>,
    pub arms: Vec<ArmRecord>,
    pub movements: Vec<MovementRecord>,
}

/// Which lane of the road a lane connection end belongs to, as side and
/// index.
pub type LaneRef = (Side, u16);

/// A connection between two roads that share a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionRecord {
    pub from: u64,
    pub via: u64,
    pub to: u64,
    /// `Candidate` when lanes can be paired by index, `Unknown` otherwise.
    /// Never `Confirmed` in schema 1.0: no road-to-road lane connection is
    /// stored in the data.
    pub status: ConnectionStatus,
    /// Reason code for `Unknown`.
    pub reason: Option<String>,
    /// Lane pairs (lane of `from`, lane of `to`) for `Candidate`.
    pub pairs: Vec<(LaneRef, LaneRef)>,
}
