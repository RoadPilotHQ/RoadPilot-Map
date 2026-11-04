//! The estimated curve of a road between its two nodes.
//!
//! The game data confirms the end positions and the forward directions at
//! both nodes, and the stored length. The curve between them is open
//! question G1 (`docs/formats/scs-road-geometry.md`). This module models it
//! as a cubic Hermite curve whose tangent vectors at both ends have the
//! **stored road length** as magnitude. That choice reproduces the stored
//! length within 0.1 % for 99.5 % of the roads; it fails for turns above
//! about 120°. The curve is therefore always [`Quality::Estimated`], and each
//! curve reports how well it agrees with the stored length
//! ([`RoadCurve::length_agreement`]).

use roadpilot_map_model::{Quality, RoadGeometry};

/// Name of the tangent model, written to the map package so that consumers
/// know how to reconstruct the curve.
pub const TANGENT_MODEL: &str = "hermite_stored_length";

/// Relative difference between the estimated curve length and the stored road
/// length up to which the two count as consistent (0.1 %).
pub const LENGTH_TOLERANCE: f64 = 1e-3;

/// Number of segments of the polyline used to measure the curve length.
const LENGTH_SEGMENTS: usize = 256;

/// Number of samples of the coarse search in [`RoadCurve::project`].
const PROJECT_SAMPLES: usize = 64;

/// Why no curve could be estimated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CurveIssue {
    /// A position or direction is not finite.
    NonFiniteInput,
    /// The stored length is not finite and positive, so the tangent length
    /// is unknown.
    InvalidStoredLength,
    /// A forward direction is not a unit vector (within 1e-3).
    InvalidDirection,
}

/// How the estimated curve length compares with the stored road length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LengthAgreement {
    /// Within [`LENGTH_TOLERANCE`]. The model reproduces the stored length;
    /// this supports, but does not prove, that the curve matches the game's.
    Consistent { relative_deviation: f64 },
    /// Beyond the tolerance. The curve shape is likely wrong for this road
    /// (the model is known to fail for very sharp turns).
    Deviates { relative_deviation: f64 },
}

impl LengthAgreement {
    /// The relative deviation `|estimated - stored| / stored`.
    #[must_use]
    pub fn relative_deviation(self) -> f64 {
        match self {
            Self::Consistent { relative_deviation } | Self::Deviates { relative_deviation } => {
                relative_deviation
            }
        }
    }

    /// `true` for [`LengthAgreement::Consistent`].
    #[must_use]
    pub fn is_consistent(self) -> bool {
        matches!(self, Self::Consistent { .. })
    }
}

/// An axis-aligned box in world coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// The closest point of a curve to a position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveProjection {
    /// Curve parameter in `[0, 1]`.
    pub t: f64,
    /// The point on the curve.
    pub point: [f64; 3],
    /// Horizontal (X/Z) distance from the position to `point`.
    pub distance: f64,
}

/// The estimated centre curve of one road.
///
/// End positions and end directions are confirmed; everything between is an
/// estimate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadCurve {
    start: [f64; 3],
    end: [f64; 3],
    start_tangent: [f64; 3],
    end_tangent: [f64; 3],
    stored_length: f64,
    chord_length: f64,
    estimated_length: f64,
}

fn finite(v: [f64; 3]) -> bool {
    v.iter().all(|c| c.is_finite())
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    dot(
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]],
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]],
    )
    .sqrt()
}

fn is_unit(v: [f64; 3]) -> bool {
    (dot(v, v).sqrt() - 1.0).abs() <= 1e-3
}

impl RoadCurve {
    /// Estimates the curve of a road from its confirmed geometry.
    ///
    /// # Errors
    ///
    /// [`CurveIssue`] if an input is not finite, a direction is not a unit
    /// vector, or the stored length is not positive. No default is
    /// substituted for a missing tangent length.
    pub fn estimate(geometry: &RoadGeometry) -> Result<Self, CurveIssue> {
        if ![
            geometry.start,
            geometry.end,
            geometry.start_direction,
            geometry.end_direction,
        ]
        .into_iter()
        .all(finite)
        {
            return Err(CurveIssue::NonFiniteInput);
        }
        if !(geometry.stored_length.is_finite() && geometry.stored_length > 0.0) {
            return Err(CurveIssue::InvalidStoredLength);
        }
        if !(is_unit(geometry.start_direction) && is_unit(geometry.end_direction)) {
            return Err(CurveIssue::InvalidDirection);
        }
        let l = geometry.stored_length;
        let mut curve = Self {
            start: geometry.start,
            end: geometry.end,
            start_tangent: geometry.start_direction.map(|c| c * l),
            end_tangent: geometry.end_direction.map(|c| c * l),
            stored_length: l,
            chord_length: dist(geometry.start, geometry.end),
            estimated_length: 0.0,
        };
        curve.estimated_length = curve.measure_length();
        Ok(curve)
    }

    fn measure_length(&self) -> f64 {
        let mut prev = self.start;
        let mut sum = 0.0;
        for i in 1..=LENGTH_SEGMENTS {
            let p = self.point(i as f64 / LENGTH_SEGMENTS as f64);
            sum += dist(prev, p);
            prev = p;
        }
        sum
    }

    /// The quality of the curve between the nodes: always
    /// [`Quality::Estimated`]. The end points are [`Quality::Confirmed`].
    #[must_use]
    pub fn quality(&self) -> Quality {
        Quality::Estimated
    }

    /// Confirmed start position, world units.
    #[must_use]
    pub fn start(&self) -> [f64; 3] {
        self.start
    }

    /// Confirmed end position, world units.
    #[must_use]
    pub fn end(&self) -> [f64; 3] {
        self.end
    }

    /// The length stored in the road record, which is also the magnitude of
    /// the Hermite tangents.
    #[must_use]
    pub fn stored_length(&self) -> f64 {
        self.stored_length
    }

    /// Straight-line distance between the nodes.
    #[must_use]
    pub fn chord_length(&self) -> f64 {
        self.chord_length
    }

    /// The length of the estimated curve, measured on a polyline of 256
    /// segments (an estimate, not the game's length).
    #[must_use]
    pub fn estimated_length(&self) -> f64 {
        self.estimated_length
    }

    /// Compares the estimated curve length with the stored length.
    #[must_use]
    pub fn length_agreement(&self) -> LengthAgreement {
        let relative_deviation =
            (self.estimated_length - self.stored_length).abs() / self.stored_length;
        if relative_deviation <= LENGTH_TOLERANCE {
            LengthAgreement::Consistent { relative_deviation }
        } else {
            LengthAgreement::Deviates { relative_deviation }
        }
    }

    /// The angle between the forward directions at both ends, in radians
    /// (`0` for a road that keeps its direction).
    #[must_use]
    pub fn turn_angle(&self) -> f64 {
        let l2 = self.stored_length * self.stored_length;
        (dot(self.start_tangent, self.end_tangent) / l2)
            .clamp(-1.0, 1.0)
            .acos()
    }

    /// The point at parameter `t` (clamped to `[0, 1]`).
    #[must_use]
    pub fn point(&self, t: f64) -> [f64; 3] {
        let t = t.clamp(0.0, 1.0);
        let (t2, t3) = (t * t, t * t * t);
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        std::array::from_fn(|i| {
            h00 * self.start[i]
                + h10 * self.start_tangent[i]
                + h01 * self.end[i]
                + h11 * self.end_tangent[i]
        })
    }

    /// The derivative `dP/dt` at parameter `t` (clamped to `[0, 1]`). It has
    /// no unit length.
    #[must_use]
    pub fn derivative(&self, t: f64) -> [f64; 3] {
        let t = t.clamp(0.0, 1.0);
        let t2 = t * t;
        let d00 = 6.0 * t2 - 6.0 * t;
        let d10 = 3.0 * t2 - 4.0 * t + 1.0;
        let d01 = -6.0 * t2 + 6.0 * t;
        let d11 = 3.0 * t2 - 2.0 * t;
        std::array::from_fn(|i| {
            d00 * self.start[i]
                + d10 * self.start_tangent[i]
                + d01 * self.end[i]
                + d11 * self.end_tangent[i]
        })
    }

    /// `segments + 1` points at equal parameter steps, from the start to the
    /// end. `segments` is at least 1.
    #[must_use]
    pub fn sample(&self, segments: usize) -> Vec<[f64; 3]> {
        let n = segments.max(1);
        (0..=n).map(|i| self.point(i as f64 / n as f64)).collect()
    }

    /// A box that is guaranteed to contain the whole estimated curve. It is
    /// the box of the Bézier control points of the Hermite curve (convex
    /// hull property). It does **not** bound the game's real curve; a
    /// consumer that needs a safety margin must add one.
    #[must_use]
    pub fn bounds(&self) -> Bounds {
        let third = |a: [f64; 3], b: [f64; 3], sign: f64| -> [f64; 3] {
            std::array::from_fn(|i| a[i] + sign * b[i] / 3.0)
        };
        let points = [
            self.start,
            third(self.start, self.start_tangent, 1.0),
            third(self.end, self.end_tangent, -1.0),
            self.end,
        ];
        let mut min = points[0];
        let mut max = points[0];
        for p in &points[1..] {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        Bounds { min, max }
    }

    /// The point of the estimated curve closest to `position`, measured
    /// horizontally (X/Z). A coarse search over 64 samples is refined by
    /// interval halving; for curves with several local minima the result is
    /// the best of the coarse samples' neighbourhoods.
    #[must_use]
    pub fn project(&self, position: [f64; 3]) -> CurveProjection {
        let horizontal = |t: f64| {
            let p = self.point(t);
            let (dx, dz) = (p[0] - position[0], p[2] - position[2]);
            dx * dx + dz * dz
        };
        let mut best = 0usize;
        let mut best_d = f64::INFINITY;
        for i in 0..=PROJECT_SAMPLES {
            let d = horizontal(i as f64 / PROJECT_SAMPLES as f64);
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        let step = 1.0 / PROJECT_SAMPLES as f64;
        let mut lo = (best as f64 * step - step).max(0.0);
        let mut hi = (best as f64 * step + step).min(1.0);
        for _ in 0..40 {
            let third = (hi - lo) / 3.0;
            let (a, b) = (lo + third, hi - third);
            if horizontal(a) < horizontal(b) {
                hi = b;
            } else {
                lo = a;
            }
        }
        let t = 0.5 * (lo + hi);
        let point = self.point(t);
        // Never return something worse than the coarse sample itself.
        let (t, point) = if horizontal(t) <= best_d {
            (t, point)
        } else {
            let tb = best as f64 * step;
            (tb, self.point(tb))
        };
        CurveProjection {
            t,
            point,
            distance: ((point[0] - position[0]).powi(2) + (point[2] - position[2]).powi(2)).sqrt(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry(
        start: [f64; 3],
        end: [f64; 3],
        sd: [f64; 3],
        ed: [f64; 3],
        len: f64,
    ) -> RoadGeometry {
        // RoadGeometry has public fields, so tests can build it directly.
        RoadGeometry {
            start,
            end,
            start_direction: sd,
            end_direction: ed,
            chord_length: dist(start, end),
            stored_length: len,
        }
    }

    const NORTH: [f64; 3] = [0.0, 0.0, -1.0];

    #[test]
    fn straight_road_is_a_straight_consistent_line() {
        let g = geometry([0.0; 3], [0.0, 0.0, -10.0], NORTH, NORTH, 10.0);
        let c = RoadCurve::estimate(&g).expect("valid");
        assert!((c.estimated_length() - 10.0).abs() < 1e-9);
        assert!(c.length_agreement().is_consistent());
        let mid = c.point(0.5);
        assert!(mid[0].abs() < 1e-12 && (mid[2] + 5.0).abs() < 1e-9);
        assert!(c.turn_angle().abs() < 1e-9);
        assert_eq!(c.quality(), Quality::Estimated);
    }

    #[test]
    fn endpoints_are_exact() {
        let g = geometry(
            [1.0, 2.0, 3.0],
            [5.0, 2.5, -4.0],
            NORTH,
            [1.0, 0.0, 0.0],
            9.0,
        );
        let c = RoadCurve::estimate(&g).expect("valid");
        assert_eq!(c.point(0.0), [1.0, 2.0, 3.0]);
        assert_eq!(c.point(1.0), [5.0, 2.5, -4.0]);
        // Parameters outside [0, 1] are clamped.
        assert_eq!(c.point(-3.0), c.point(0.0));
        assert_eq!(c.point(7.0), c.point(1.0));
    }

    #[test]
    fn quarter_circle_is_close_to_the_stored_arc_length() {
        // A quarter circle of radius 100 from (0,0,0) heading north to
        // (-100, 0, -100) heading west. Arc length = 50 * pi.
        let arc = 50.0 * std::f64::consts::PI;
        let g = geometry(
            [0.0; 3],
            [-100.0, 0.0, -100.0],
            NORTH,
            [-1.0, 0.0, 0.0],
            arc,
        );
        let c = RoadCurve::estimate(&g).expect("valid");
        assert!((c.turn_angle() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        // The model is an estimate: the deviation is reported, not hidden.
        let dev = c.length_agreement().relative_deviation();
        assert!(dev < 0.05, "{dev}");
    }

    #[test]
    fn deviation_flags_inconsistent_lengths() {
        // A stored length far longer than the chord of a straight pair of
        // directions cannot be reproduced.
        let g = geometry([0.0; 3], [0.0, 0.0, -10.0], NORTH, NORTH, 30.0);
        let c = RoadCurve::estimate(&g).expect("valid");
        assert!(!c.length_agreement().is_consistent());
    }

    #[test]
    fn bounds_contain_every_sample() {
        let g = geometry(
            [0.0; 3],
            [-60.0, 4.0, -40.0],
            NORTH,
            [-1.0, 0.0, 0.0],
            110.0,
        );
        let c = RoadCurve::estimate(&g).expect("valid");
        let b = c.bounds();
        for p in c.sample(200) {
            for i in 0..3 {
                assert!(
                    p[i] >= b.min[i] - 1e-9 && p[i] <= b.max[i] + 1e-9,
                    "{p:?} {b:?}"
                );
            }
        }
    }

    #[test]
    fn rejects_invalid_input_without_defaults() {
        let ok = geometry([0.0; 3], [0.0, 0.0, -10.0], NORTH, NORTH, 10.0);
        for len in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let g = RoadGeometry {
                stored_length: len,
                ..ok
            };
            assert_eq!(
                RoadCurve::estimate(&g),
                Err(CurveIssue::InvalidStoredLength),
                "{len}"
            );
        }
        let g = RoadGeometry {
            end: [f64::NAN, 0.0, 0.0],
            ..ok
        };
        assert_eq!(RoadCurve::estimate(&g), Err(CurveIssue::NonFiniteInput));
        let g = RoadGeometry {
            start_direction: [0.0; 3],
            ..ok
        };
        assert_eq!(RoadCurve::estimate(&g), Err(CurveIssue::InvalidDirection));
        let g = RoadGeometry {
            end_direction: [2.0, 0.0, 0.0],
            ..ok
        };
        assert_eq!(RoadCurve::estimate(&g), Err(CurveIssue::InvalidDirection));
    }

    #[test]
    fn projection_finds_the_closest_horizontal_point() {
        let g = geometry([0.0; 3], [0.0, 0.0, -100.0], NORTH, NORTH, 100.0);
        let c = RoadCurve::estimate(&g).expect("valid");
        let p = c.project([3.0, 50.0, -40.0]);
        assert!((p.t - 0.4).abs() < 1e-6, "{p:?}");
        assert!((p.distance - 3.0).abs() < 1e-6);
        // Before the start the projection is the start point.
        let p = c.project([0.0, 0.0, 20.0]);
        assert_eq!(p.t, 0.0);
        assert!((p.distance - 20.0).abs() < 1e-9);
    }

    #[test]
    fn derivative_matches_the_end_tangents() {
        let g = geometry([0.0; 3], [-20.0, 0.0, -20.0], NORTH, [-1.0, 0.0, 0.0], 35.0);
        let c = RoadCurve::estimate(&g).expect("valid");
        let d0 = c.derivative(0.0);
        let d1 = c.derivative(1.0);
        assert!((d0[2] + 35.0).abs() < 1e-9 && d0[0].abs() < 1e-9);
        assert!((d1[0] + 35.0).abs() < 1e-9 && d1[2].abs() < 1e-9);
    }

    #[test]
    fn sampling_is_deterministic_and_has_the_requested_size() {
        let g = geometry([0.0; 3], [0.0, 0.0, -10.0], NORTH, NORTH, 10.0);
        let c = RoadCurve::estimate(&g).expect("valid");
        assert_eq!(c.sample(0).len(), 2);
        assert_eq!(c.sample(8).len(), 9);
        assert_eq!(c.sample(8), c.sample(8));
    }
}
