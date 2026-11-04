//! Lane centre lines from explicit assumptions.
//!
//! The game data stores **no lane width**, the order of the lane indices is
//! unconfirmed (LM2), the meaning of the stored lane offsets is unknown (LM3)
//! and the position of the road axis relative to the node curve is unknown.
//! A lane centre line therefore cannot be derived from the data. This module
//! offers one only for callers who state all of these assumptions
//! explicitly; there are no defaults. The result is always
//! [`Quality::Estimated`], never [`Quality::Confirmed`], and must not be
//! used for active steering.

use roadpilot_map_model::{Quality, RoadSide};

use crate::curve::RoadCurve;

/// Which end of a road side lane index 0 is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneOrder {
    /// Index 0 is the lane next to the road axis.
    InnerFirst,
    /// Index 0 is the lane at the road edge.
    OuterFirst,
}

/// All assumptions a lane centre line depends on. None of them is known from
/// the game data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneAssumptions {
    /// Width of every lane, world units. Must be finite and positive.
    pub lane_width: f64,
    /// Lane index order (LM2).
    pub order: LaneOrder,
    /// Distance between the node curve and the inner edge of the first lane
    /// of each side, world units. `0.0` assumes the node curve is the road
    /// axis. Must be finite and not negative.
    pub axis_offset: f64,
}

/// Why no lane centre line was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LaneGeometryIssue {
    /// The lane width is not finite and positive.
    InvalidWidth,
    /// The axis offset is not finite or is negative.
    InvalidAxisOffset,
    /// The lane index is not below the lane count of the side.
    IndexOutOfRange,
    /// The curve is vertical at a sample, so there is no horizontal side
    /// direction.
    VerticalTangent,
}

/// An estimated lane centre line.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneCenterline {
    /// Always [`Quality::Estimated`].
    pub quality: Quality,
    /// Signed lateral distance from the node curve: positive to the left of
    /// the stored road direction, negative to the right.
    pub lateral_offset: f64,
    /// Points from the start to the end of the road, world units.
    pub points: Vec<[f64; 3]>,
}

/// The signed lateral offset of a lane centre from the node curve under the
/// given assumptions (positive left of the stored direction).
///
/// # Errors
///
/// [`LaneGeometryIssue`] if an assumption is invalid or `index >= count`.
pub fn lane_lateral_offset(
    side: RoadSide,
    index: usize,
    count: usize,
    assumptions: &LaneAssumptions,
) -> Result<f64, LaneGeometryIssue> {
    if !(assumptions.lane_width.is_finite() && assumptions.lane_width > 0.0) {
        return Err(LaneGeometryIssue::InvalidWidth);
    }
    if !(assumptions.axis_offset.is_finite() && assumptions.axis_offset >= 0.0) {
        return Err(LaneGeometryIssue::InvalidAxisOffset);
    }
    if index >= count {
        return Err(LaneGeometryIssue::IndexOutOfRange);
    }
    let from_axis = match assumptions.order {
        LaneOrder::InnerFirst => index,
        LaneOrder::OuterFirst => count - 1 - index,
    };
    let magnitude = assumptions.axis_offset + (from_axis as f64 + 0.5) * assumptions.lane_width;
    Ok(match side {
        RoadSide::Left => magnitude,
        RoadSide::Right => -magnitude,
    })
}

/// The estimated centre line of one lane, sampled at `segments + 1` equal
/// parameter steps of the road curve and shifted horizontally by the lane's
/// lateral offset.
///
/// The side direction in the SCS world (+X east, +Y up, +Z south) for a
/// horizontal travel direction `(dx, dz)` is `left = (dz, -dx)`.
///
/// # Errors
///
/// [`LaneGeometryIssue`] as for [`lane_lateral_offset`], or
/// [`LaneGeometryIssue::VerticalTangent`].
pub fn lane_centerline(
    curve: &RoadCurve,
    side: RoadSide,
    index: usize,
    count: usize,
    assumptions: &LaneAssumptions,
    segments: usize,
) -> Result<LaneCenterline, LaneGeometryIssue> {
    let offset = lane_lateral_offset(side, index, count, assumptions)?;
    let n = segments.max(1);
    let mut points = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let t = i as f64 / n as f64;
        let p = curve.point(t);
        let d = curve.derivative(t);
        let h = (d[0] * d[0] + d[2] * d[2]).sqrt();
        if h.is_nan() || h <= 1e-9 {
            return Err(LaneGeometryIssue::VerticalTangent);
        }
        let left = [d[2] / h, 0.0, -d[0] / h];
        points.push([p[0] + left[0] * offset, p[1], p[2] + left[2] * offset]);
    }
    Ok(LaneCenterline {
        quality: Quality::Estimated,
        lateral_offset: offset,
        points,
    })
}

#[cfg(test)]
mod tests {
    use roadpilot_map_model::RoadGeometry;

    use super::*;

    const A: LaneAssumptions = LaneAssumptions {
        lane_width: 4.0,
        order: LaneOrder::InnerFirst,
        axis_offset: 0.0,
    };

    fn straight_north() -> RoadCurve {
        let g = RoadGeometry {
            start: [0.0; 3],
            end: [0.0, 0.0, -100.0],
            start_direction: [0.0, 0.0, -1.0],
            end_direction: [0.0, 0.0, -1.0],
            chord_length: 100.0,
            stored_length: 100.0,
        };
        RoadCurve::estimate(&g).expect("valid")
    }

    #[test]
    fn offsets_follow_side_and_order() {
        let off = |side, i, order| {
            lane_lateral_offset(side, i, 3, &LaneAssumptions { order, ..A }).expect("valid")
        };
        assert_eq!(off(RoadSide::Right, 0, LaneOrder::InnerFirst), -2.0);
        assert_eq!(off(RoadSide::Right, 2, LaneOrder::InnerFirst), -10.0);
        assert_eq!(off(RoadSide::Left, 0, LaneOrder::InnerFirst), 2.0);
        assert_eq!(off(RoadSide::Right, 0, LaneOrder::OuterFirst), -10.0);
        let with_axis = LaneAssumptions {
            axis_offset: 1.5,
            ..A
        };
        assert_eq!(
            lane_lateral_offset(RoadSide::Left, 1, 2, &with_axis),
            Ok(1.5 + 6.0)
        );
    }

    #[test]
    fn rejects_missing_or_invalid_assumptions() {
        for w in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let a = LaneAssumptions { lane_width: w, ..A };
            assert_eq!(
                lane_lateral_offset(RoadSide::Left, 0, 1, &a),
                Err(LaneGeometryIssue::InvalidWidth)
            );
        }
        for o in [-0.1, f64::NAN] {
            let a = LaneAssumptions {
                axis_offset: o,
                ..A
            };
            assert_eq!(
                lane_lateral_offset(RoadSide::Left, 0, 1, &a),
                Err(LaneGeometryIssue::InvalidAxisOffset)
            );
        }
        assert_eq!(
            lane_lateral_offset(RoadSide::Left, 2, 2, &A),
            Err(LaneGeometryIssue::IndexOutOfRange)
        );
    }

    #[test]
    fn right_lane_of_a_road_heading_north_lies_east() {
        // Heading north (-Z), the right-hand side is east (+X).
        let c = lane_centerline(&straight_north(), RoadSide::Right, 0, 1, &A, 4).expect("valid");
        assert_eq!(c.quality, Quality::Estimated);
        assert!(!c.quality.is_suitable_for_active_steering());
        for p in &c.points {
            assert!((p[0] - 2.0).abs() < 1e-9, "{p:?}");
        }
        assert_eq!(c.points.len(), 5);
        let left = lane_centerline(&straight_north(), RoadSide::Left, 0, 1, &A, 1).expect("valid");
        assert!((left.points[0][0] + 2.0).abs() < 1e-9);
    }

    #[test]
    fn vertical_curves_have_no_side_direction() {
        let g = RoadGeometry {
            start: [0.0; 3],
            end: [0.0, 10.0, 0.0],
            start_direction: [0.0, 1.0, 0.0],
            end_direction: [0.0, 1.0, 0.0],
            chord_length: 10.0,
            stored_length: 10.0,
        };
        let c = RoadCurve::estimate(&g).expect("valid");
        assert_eq!(
            lane_centerline(&c, RoadSide::Left, 0, 1, &A, 2),
            Err(LaneGeometryIssue::VerticalTangent)
        );
    }
}
