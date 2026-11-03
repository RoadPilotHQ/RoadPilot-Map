//! Positions, rotations and the confirmed geometry of a road.
//!
//! All values stay in the SCS world coordinate system (see
//! `docs/formats/scs-road-geometry.md`): +X east, +Y up, +Z south, one
//! position unit = 1/256 of a world unit. Nothing is transformed.

/// A position as stored in the game data: X, Y, Z in fixed point with
/// 8 fractional bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Position {
    pub fixed: [i32; 3],
}

impl Position {
    /// Number of fixed-point steps per world unit.
    pub const SCALE: f64 = 256.0;

    /// The position in world units. Exact: every `i32 / 256` fits in an `f64`.
    #[must_use]
    pub fn world(&self) -> [f64; 3] {
        self.fixed.map(|v| f64::from(v) / Self::SCALE)
    }
}

/// A node rotation: a unit quaternion `(w, x, y, z)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rotation {
    pub w: f32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Rotation {
    /// Largest accepted deviation of the quaternion norm from 1. The game
    /// data never exceeds it.
    pub const NORM_TOLERANCE: f32 = 1e-3;

    /// `true` if all components are finite and the norm is 1 within
    /// [`Rotation::NORM_TOLERANCE`].
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let n = (self.w * self.w + self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        n.is_finite() && (n - 1.0).abs() <= Self::NORM_TOLERANCE
    }

    /// The forward direction: the local `-Z` axis rotated by the quaternion,
    /// as a unit vector in world coordinates.
    ///
    /// This convention was confirmed on the game data: for 99.4 % (ETS2) and
    /// 99.6 % (ATS) of all roads, the start node's forward direction is within
    /// 60° of the direction to the end node.
    #[must_use]
    pub fn forward(&self) -> [f64; 3] {
        let (w, x, y, z) = (
            f64::from(self.w),
            f64::from(self.x),
            f64::from(self.y),
            f64::from(self.z),
        );
        // Third column of the rotation matrix of the normalized quaternion,
        // negated.
        let n2 = w * w + x * x + y * y + z * z;
        let s = if n2 > 0.0 { 2.0 / n2 } else { 0.0 };
        [
            -(s * (x * z + w * y)),
            -(s * (y * z - w * x)),
            -(1.0 - s * (x * x + y * y)),
        ]
    }
}

/// What is known about the shape of a road from its nodes and record.
///
/// The exact curve between the two nodes is **not** part of the model yet:
/// the interpolation the game uses is not confirmed (open question G1 in
/// `docs/formats/scs-road-geometry.md`). Only these values are confirmed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadGeometry {
    /// Start node position, world units.
    pub start: [f64; 3],
    /// End node position, world units.
    pub end: [f64; 3],
    /// Unit forward direction at the start node.
    pub start_direction: [f64; 3],
    /// Unit forward direction at the end node.
    pub end_direction: [f64; 3],
    /// Straight-line distance between the nodes, world units.
    pub chord_length: f64,
    /// The length stored in the road record. For straight roads it equals
    /// the chord length; for curved roads it is the curve length.
    pub stored_length: f64,
}

impl RoadGeometry {
    pub(crate) fn new(start: (Position, Rotation), end: (Position, Rotation), length: f32) -> Self {
        let (a, b) = (start.0.world(), end.0.world());
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        Self {
            start: a,
            end: b,
            start_direction: start.1.forward(),
            end_direction: end.1.forward(),
            chord_length: (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt(),
            stored_length: f64::from(length),
        }
    }

    /// Height change from start to end (`end.y - start.y`), world units.
    #[must_use]
    pub fn height_change(&self) -> f64 {
        self.end[1] - self.start[1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: Rotation = Rotation {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn identity_faces_negative_z() {
        assert!(close(IDENTITY.forward(), [0.0, 0.0, -1.0]));
    }

    #[test]
    fn quarter_turn_about_y_faces_negative_x() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let r = Rotation {
            w: h,
            x: 0.0,
            y: h,
            z: 0.0,
        };
        assert!(close(r.forward(), [-1.0, 0.0, 0.0]));
        // A half turn faces +Z.
        let r = Rotation {
            w: 0.0,
            x: 0.0,
            y: 1.0,
            z: 0.0,
        };
        assert!(close(r.forward(), [0.0, 0.0, 1.0]));
    }

    #[test]
    fn pitch_gives_a_vertical_component() {
        // Rotation of +30° about X tilts -Z upwards (towards +Y).
        let half = 15f32.to_radians();
        let r = Rotation {
            w: half.cos(),
            x: half.sin(),
            y: 0.0,
            z: 0.0,
        };
        let f = r.forward();
        assert!((f[1] - 0.5).abs() < 1e-6, "{f:?}");
        assert!((f[0] * f[0] + f[1] * f[1] + f[2] * f[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn forward_of_a_slightly_denormalized_quaternion_is_unit() {
        let r = Rotation {
            w: 1.0005,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(r.is_valid());
        assert!(close(r.forward(), [0.0, 0.0, -1.0]));
    }

    #[test]
    fn rejects_invalid_rotations() {
        for r in [
            Rotation { w: 0.5, ..IDENTITY },
            Rotation {
                w: f32::NAN,
                ..IDENTITY
            },
            Rotation {
                w: f32::INFINITY,
                ..IDENTITY
            },
            Rotation {
                w: 0.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        ] {
            assert!(!r.is_valid(), "{r:?}");
        }
        // Even an invalid rotation does not panic or produce NaN from zero.
        let zero = Rotation {
            w: 0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(zero.forward().iter().all(|v| v.is_finite()));
    }

    #[test]
    fn positions_convert_exactly_including_extremes() {
        let p = Position {
            fixed: [256, -128, 1],
        };
        assert_eq!(p.world(), [1.0, -0.5, 1.0 / 256.0]);
        let p = Position {
            fixed: [i32::MAX, i32::MIN, 0],
        };
        assert_eq!(
            p.world(),
            [
                f64::from(i32::MAX) / 256.0,
                f64::from(i32::MIN) / 256.0,
                0.0
            ]
        );
    }

    #[test]
    fn geometry_of_a_straight_road_with_height_change() {
        let start = Position { fixed: [0, 0, 0] };
        let end = Position {
            fixed: [0, 3 * 256, -4 * 256],
        };
        let g = RoadGeometry::new((start, IDENTITY), (end, IDENTITY), 5.0);
        assert_eq!(g.chord_length, 5.0);
        assert_eq!(g.stored_length, 5.0);
        assert_eq!(g.height_change(), 3.0);
        assert!(close(g.start_direction, [0.0, 0.0, -1.0]));
    }

    #[test]
    fn geometry_at_coordinate_extremes_does_not_overflow() {
        let a = Position {
            fixed: [i32::MIN, i32::MIN, i32::MIN],
        };
        let b = Position {
            fixed: [i32::MAX, i32::MAX, i32::MAX],
        };
        let g = RoadGeometry::new((a, IDENTITY), (b, IDENTITY), 1.0);
        assert!(g.chord_length.is_finite() && g.chord_length > 0.0);
    }
}
