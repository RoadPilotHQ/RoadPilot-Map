//! Lanes of roads and candidate lane transitions between connected roads.
//!
//! The model stores what the game data confirms and marks everything else
//! as unknown. See `docs/formats/scs-lane-model.md` for the evidence.
//!
//! - Lanes belong to the **left** or **right** side of a road, relative to
//!   the road's stored direction (start node to end node), in definition
//!   order. SCS documents this order as centre-to-border for ETS2; ATS uses
//!   the same definition fields, so that interpretation is inferred there.
//! - The **driving direction** of a lane follows from its side and the
//!   road's traffic side: with right-hand traffic, right-side lanes run in
//!   the stored direction and left-side lanes against it; with left-hand
//!   traffic it is the other way round. Without a known traffic side the
//!   direction is unknown.
//! - **Widths** are not stored per lane in the game data, so every width is
//!   `None`. **Offsets** are kept as the stored value pairs, whose meaning is
//!   not confirmed.
//! - **Lane transitions** between two roads are not stored in road data.
//!   Lanes can only be paired by index as *candidates*, and only when both
//!   roads are fully resolved, share a traffic side and have the same number
//!   of lanes per direction.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt;

use crate::network::{RoadConnection, RoadId, RoadNetwork};

/// Side of a road, relative to its stored direction (start to end node).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RoadSide {
    Left,
    Right,
}

/// Which side of the road traffic drives on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrafficSide {
    Right,
    Left,
}

/// Why a road's traffic side is not known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrafficSideIssue {
    /// The road's nodes name no country.
    NoCountry,
    /// The road's nodes name different countries (for example at a border).
    CountryMismatch,
    /// The nodes name a country that is not defined.
    UnknownCountry(u32),
    /// The road look sets a traffic side that contradicts the country.
    LookConflict,
}

/// A road's traffic side, or why it is unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrafficSideState {
    Known(TrafficSide),
    Unknown(TrafficSideIssue),
}

/// Direction of travel on a lane, relative to the road's stored direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaneDirection {
    /// From the start node to the end node.
    Forward,
    /// From the end node to the start node.
    Backward,
}

/// Identifies a lane: its road, its side and its index on that side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LaneId {
    pub road: RoadId,
    pub side: RoadSide,
    pub index: u16,
}

/// The lane type, the name of a `traffic_lane_data` definition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LaneType {
    /// Confirmed: no variant of the road look changes this side.
    Known(String),
    /// The road look's own type. A variant of the look replaces the lane
    /// types of this side, and which variant the road uses is not known.
    Uncertain(String),
}

/// A lane offset pair as stored in `lane_offsets_<side>`. The meaning of
/// the two numbers is not confirmed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StoredLaneOffset {
    pub first: f32,
    pub second: f32,
}

/// One lane.
#[derive(Debug, Clone, PartialEq)]
pub struct Lane {
    pub id: LaneId,
    pub lane_type: LaneType,
    /// Stored offset pair, if the look stores exactly one per lane.
    pub offset: Option<StoredLaneOffset>,
    /// Lane width. The game data stores no per-lane width, so this is
    /// `None` unless a confirmed source is added.
    pub width: Option<f32>,
}

/// How a side's stored lane offsets relate to its lanes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OffsetsState {
    /// The look stores no offsets for this side.
    NotStored,
    /// One offset per lane; assigned in order.
    Assigned,
    /// The number of offsets differs from the number of lanes, so they are
    /// not assigned.
    CountMismatch { offsets: usize, lanes: usize },
}

/// Why a road's lane configuration cannot be determined.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LaneIssue {
    /// The road look is not defined.
    MissingRoadLook(String),
    /// A variant referenced by the look is not defined.
    MissingVariant(String),
    /// A lane type is not defined.
    MissingLaneType(String),
    /// A variant of the look has a different number of lanes on a side, and
    /// which variant the road uses is not known.
    VariantChangesLaneCount,
}

/// A road's lanes, or why they cannot be determined.
#[derive(Debug, Clone, PartialEq)]
pub enum LaneConfig {
    /// Left-side lanes first, then right-side lanes, each in index order.
    Resolved(Vec<Lane>),
    Unresolved(LaneIssue),
}

/// The lanes of one road.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadLanes {
    pub road: RoadId,
    /// Name of the road look, `road.<token>`.
    pub look: String,
    pub traffic_side: TrafficSideState,
    pub config: LaneConfig,
    pub offsets_left: OffsetsState,
    pub offsets_right: OffsetsState,
    /// `road_size_left` of the look, as stored. Presumably the width of the
    /// left side; not confirmed.
    pub stored_size_left: Option<f32>,
    /// `road_size_right` of the look, as stored. Not confirmed.
    pub stored_size_right: Option<f32>,
}

impl RoadLanes {
    /// The resolved lanes, or an empty slice.
    #[must_use]
    pub fn lanes(&self) -> &[Lane] {
        match &self.config {
            LaneConfig::Resolved(lanes) => lanes,
            LaneConfig::Unresolved(_) => &[],
        }
    }

    /// Driving direction of a lane of this road; `None` if the traffic side
    /// is unknown.
    #[must_use]
    pub fn direction(&self, side: RoadSide) -> Option<LaneDirection> {
        let TrafficSideState::Known(traffic) = self.traffic_side else {
            return None;
        };
        Some(match (traffic, side) {
            (TrafficSide::Right, RoadSide::Right) | (TrafficSide::Left, RoadSide::Left) => {
                LaneDirection::Forward
            }
            _ => LaneDirection::Backward,
        })
    }

    /// The lanes that run in `direction`, in index order. Empty if the
    /// traffic side or the lanes are unknown.
    #[must_use]
    pub fn lanes_in(&self, direction: LaneDirection) -> Vec<&Lane> {
        self.lanes()
            .iter()
            .filter(|l| self.direction(l.id.side) == Some(direction))
            .collect()
    }
}

/// A candidate transition from one lane to another, in driving direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LaneConnection {
    pub from: LaneId,
    pub to: LaneId,
}

/// Why lanes across a road connection cannot be paired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransitionIssue {
    /// One of the roads has unresolved lanes.
    LanesUnresolved,
    /// One of the roads has an unknown traffic side.
    TrafficSideUnknown,
    /// The roads have different traffic sides.
    TrafficSideDiffers,
    /// The number of lanes in this direction differs between the roads.
    LaneCountDiffers(LaneDirection),
}

/// The lane-level view of a road-to-road connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaneTransition {
    /// Lanes paired by index within each direction. These are candidates:
    /// the game data stores no lane transitions between roads.
    IndexCandidates(Vec<LaneConnection>),
    /// Lanes cannot be paired.
    Unknown(TransitionIssue),
}

/// Why a [`LaneNetwork`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LaneError {
    /// Lanes are given for a road that is not in the road network.
    UnknownRoad(RoadId),
    /// Lanes are given twice for a road.
    DuplicateRoad(RoadId),
    /// A road of the road network has no lane entry.
    MissingRoad(RoadId),
    /// A lane belongs to another road than the entry it is listed in.
    LaneOfOtherRoad { road: RoadId, lane: LaneId },
    /// Two lanes have the same ID.
    DuplicateLane(LaneId),
}

impl fmt::Display for LaneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownRoad(r) => write!(f, "lanes given for unknown road {:016x}", r.0),
            Self::DuplicateRoad(r) => write!(f, "lanes given twice for road {:016x}", r.0),
            Self::MissingRoad(r) => write!(f, "road {:016x} has no lane entry", r.0),
            Self::LaneOfOtherRoad { road, lane } => write!(
                f,
                "lane of road {:016x} listed under road {:016x}",
                lane.road.0, road.0
            ),
            Self::DuplicateLane(l) => write!(
                f,
                "lane {:?} {} of road {:016x} occurs twice",
                l.side, l.index, l.road.0
            ),
        }
    }
}

impl std::error::Error for LaneError {}

/// Lanes of all roads of a [`RoadNetwork`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LaneNetwork {
    roads: BTreeMap<RoadId, RoadLanes>,
}

impl LaneNetwork {
    /// Builds and validates the lane network for `network`. Every road of
    /// the network needs exactly one entry, and lane IDs must be unique and
    /// belong to their road.
    ///
    /// # Errors
    ///
    /// The first [`LaneError`] found.
    pub fn new(
        network: &RoadNetwork,
        lanes: impl IntoIterator<Item = RoadLanes>,
    ) -> Result<Self, LaneError> {
        let mut roads = BTreeMap::new();
        for entry in lanes {
            if network.road(entry.road).is_none() {
                return Err(LaneError::UnknownRoad(entry.road));
            }
            let mut seen = std::collections::BTreeSet::new();
            for lane in entry.lanes() {
                if lane.id.road != entry.road {
                    return Err(LaneError::LaneOfOtherRoad {
                        road: entry.road,
                        lane: lane.id,
                    });
                }
                if !seen.insert(lane.id) {
                    return Err(LaneError::DuplicateLane(lane.id));
                }
            }
            match roads.entry(entry.road) {
                Entry::Vacant(e) => {
                    e.insert(entry);
                }
                Entry::Occupied(e) => return Err(LaneError::DuplicateRoad(*e.key())),
            }
        }
        if let Some(missing) = network.roads().find(|r| !roads.contains_key(&r.id)) {
            return Err(LaneError::MissingRoad(missing.id));
        }
        Ok(Self { roads })
    }

    /// Lanes of `road`.
    #[must_use]
    pub fn road(&self, road: RoadId) -> Option<&RoadLanes> {
        self.roads.get(&road)
    }

    /// All entries, ordered by road UID.
    pub fn roads(&self) -> impl Iterator<Item = &RoadLanes> {
        self.roads.values()
    }

    /// The lane-level view of a road-to-road connection (see
    /// [`RoadNetwork::connections`]).
    #[must_use]
    pub fn transition(&self, connection: &RoadConnection) -> LaneTransition {
        let (Some(from), Some(to)) = (
            self.roads.get(&connection.from),
            self.roads.get(&connection.to),
        ) else {
            return LaneTransition::Unknown(TransitionIssue::LanesUnresolved);
        };
        if matches!(from.config, LaneConfig::Unresolved(_))
            || matches!(to.config, LaneConfig::Unresolved(_))
        {
            return LaneTransition::Unknown(TransitionIssue::LanesUnresolved);
        }
        let (TrafficSideState::Known(a), TrafficSideState::Known(b)) =
            (from.traffic_side, to.traffic_side)
        else {
            return LaneTransition::Unknown(TransitionIssue::TrafficSideUnknown);
        };
        if a != b {
            return LaneTransition::Unknown(TransitionIssue::TrafficSideDiffers);
        }
        let mut connections = Vec::new();
        // Forward lanes run from `from` into `to`; backward lanes run from
        // `to` back into `from`.
        for (direction, source, target) in [
            (LaneDirection::Forward, from, to),
            (LaneDirection::Backward, to, from),
        ] {
            let (out, into) = (source.lanes_in(direction), target.lanes_in(direction));
            if out.len() != into.len() {
                return LaneTransition::Unknown(TransitionIssue::LaneCountDiffers(direction));
            }
            connections.extend(out.iter().zip(&into).map(|(o, i)| LaneConnection {
                from: o.id,
                to: i.id,
            }));
        }
        LaneTransition::IndexCandidates(connections)
    }
}
