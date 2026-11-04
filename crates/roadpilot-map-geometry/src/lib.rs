//! Road and lane geometry of RoadPilot-Map.
//!
//! Every geometric result carries an explicit [`Quality`]:
//!
//! - [`Quality::Confirmed`]: established by the game data itself,
//! - [`Quality::Estimated`]: derived with a model that is only empirically
//!   supported, or from caller-supplied assumptions,
//! - [`Quality::Unavailable`]: not known; no value is produced.
//!
//! Today the only confirmed values are the node positions and the forward
//! directions at the nodes (see `docs/formats/scs-road-geometry.md`). The curve
//! between two nodes is an [`Estimated`](Quality::Estimated) cubic Hermite
//! curve ([`RoadCurve`]). Lane positions need a lane width, a lane order and
//! a road axis offset, none of which is stored in the game data, so
//! [`lane_centerline`] only works from explicit caller assumptions and its
//! result is never [`Quality::Confirmed`].
//!
//! Nothing here is a basis for active steering. See
//! [`Quality::is_suitable_for_active_steering`].
//!
//! ```
//! use roadpilot_map_geometry::Quality;
//!
//! assert!(!Quality::Estimated.is_suitable_for_active_steering());
//! assert!(Quality::Confirmed.is_suitable_for_active_steering());
//! ```

mod curve;
mod lane;

pub use curve::{
    Bounds, CurveIssue, CurveProjection, LENGTH_TOLERANCE, LengthAgreement, RoadCurve,
    TANGENT_MODEL,
};
pub use lane::{
    LaneAssumptions, LaneCenterline, LaneGeometryIssue, LaneOrder, lane_centerline,
    lane_lateral_offset,
};
/// The evidence level type, defined in the model so that the map package can
/// use it without depending on this crate.
pub use roadpilot_map_model::Quality;
