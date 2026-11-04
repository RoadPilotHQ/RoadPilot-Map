//! A uniform grid over horizontal boxes, for finding map-matching
//! candidates.
//!
//! The index stores the position of each box in the input order. A query
//! returns candidates from the boxes only; the boxes of roads are the bounds
//! of the **estimated** curves, so a hit is a candidate to be checked with
//! the geometry, not proof that the road is near.

use std::collections::HashMap;
use std::fmt;

/// Most grid cells one box may cover. A larger box means corrupt data.
pub const MAX_CELLS_PER_BOX: u64 = 4096;

/// Why an index could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SpatialError {
    /// The cell size is not finite and positive.
    InvalidCellSize,
    /// A box has a value that is not finite or a minimum above its maximum.
    InvalidBox(usize),
    /// A box covers more than [`MAX_CELLS_PER_BOX`] cells.
    BoxTooLarge(usize),
}

impl fmt::Display for SpatialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCellSize => f.write_str("the cell size must be finite and positive"),
            Self::InvalidBox(i) => write!(f, "box {i} is not a valid box"),
            Self::BoxTooLarge(i) => write!(f, "box {i} covers too many grid cells"),
        }
    }
}

impl std::error::Error for SpatialError {}

/// One hit of a distance query.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    /// Position of the box in the input order.
    pub index: usize,
    /// Horizontal distance from the query point to the box (0 inside). This
    /// is a lower bound of the distance to the content of the box.
    pub distance: f64,
}

/// A grid index. Results are deterministic: they depend only on the boxes,
/// never on hash order.
#[derive(Debug, Clone)]
pub struct SpatialIndex {
    cell: f64,
    cells: HashMap<(i64, i64), Vec<u32>>,
    /// `[min_x, min_z, max_x, max_z]` per box.
    boxes: Vec<[f64; 4]>,
}

impl SpatialIndex {
    /// Builds an index over `[min_x, min_z, max_x, max_z]` boxes.
    ///
    /// # Errors
    ///
    /// [`SpatialError`] for an invalid cell size or box, or more than
    /// `u32::MAX` boxes.
    pub fn build(
        boxes: impl IntoIterator<Item = [f64; 4]>,
        cell_size: f64,
    ) -> Result<Self, SpatialError> {
        if !(cell_size.is_finite() && cell_size > 0.0) {
            return Err(SpatialError::InvalidCellSize);
        }
        let mut index = Self {
            cell: cell_size,
            cells: HashMap::new(),
            boxes: Vec::new(),
        };
        for (i, b) in boxes.into_iter().enumerate() {
            if !b.iter().all(|v| v.is_finite()) || b[0] > b[2] || b[1] > b[3] {
                return Err(SpatialError::InvalidBox(i));
            }
            let id = u32::try_from(i).map_err(|_| SpatialError::InvalidBox(i))?;
            let (x0, z0) = index.cell_of(b[0], b[1]);
            let (x1, z1) = index.cell_of(b[2], b[3]);
            let count = u64::try_from(x1 - x0 + 1)
                .unwrap_or(u64::MAX)
                .saturating_mul(u64::try_from(z1 - z0 + 1).unwrap_or(u64::MAX));
            if count > MAX_CELLS_PER_BOX {
                return Err(SpatialError::BoxTooLarge(i));
            }
            for cx in x0..=x1 {
                for cz in z0..=z1 {
                    index.cells.entry((cx, cz)).or_default().push(id);
                }
            }
            index.boxes.push(b);
        }
        Ok(index)
    }

    fn cell_of(&self, x: f64, z: f64) -> (i64, i64) {
        // `as` saturates for huge values; the cell-count check then rejects
        // such boxes.
        (
            (x / self.cell).floor() as i64,
            (z / self.cell).floor() as i64,
        )
    }

    /// Number of boxes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    /// `true` if there are no boxes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    /// The indices of all boxes that intersect the query box, ascending.
    /// Non-finite or inverted query boxes give no result.
    #[must_use]
    pub fn query_bounds(&self, min: [f64; 2], max: [f64; 2]) -> Vec<usize> {
        if !(min.iter().chain(&max).all(|v| v.is_finite()) && min[0] <= max[0] && min[1] <= max[1])
        {
            return Vec::new();
        }
        let (x0, z0) = self.cell_of(min[0], min[1]);
        let (x1, z1) = self.cell_of(max[0], max[1]);
        // A query that would visit more cells than there are boxes is
        // answered by scanning the boxes.
        let cells = u64::try_from(x1 - x0 + 1)
            .unwrap_or(u64::MAX)
            .saturating_mul(u64::try_from(z1 - z0 + 1).unwrap_or(u64::MAX));
        let mut hits: Vec<usize> = if cells > self.boxes.len() as u64 {
            (0..self.boxes.len()).collect()
        } else {
            let mut v = Vec::new();
            for cx in x0..=x1 {
                for cz in z0..=z1 {
                    if let Some(ids) = self.cells.get(&(cx, cz)) {
                        v.extend(ids.iter().map(|&i| i as usize));
                    }
                }
            }
            v
        };
        hits.sort_unstable();
        hits.dedup();
        hits.retain(|&i| {
            let b = &self.boxes[i];
            b[0] <= max[0] && b[2] >= min[0] && b[1] <= max[1] && b[3] >= min[1]
        });
        hits
    }

    /// The boxes within `radius` of the horizontal position `(x, z)`, nearest
    /// first (ties by index). Returns nothing for non-finite or negative
    /// input.
    #[must_use]
    pub fn query_near(&self, x: f64, z: f64, radius: f64) -> Vec<Candidate> {
        if !(x.is_finite() && z.is_finite() && radius.is_finite() && radius >= 0.0) {
            return Vec::new();
        }
        let mut out: Vec<Candidate> = self
            .query_bounds([x - radius, z - radius], [x + radius, z + radius])
            .into_iter()
            .filter_map(|index| {
                let b = &self.boxes[index];
                let dx = (b[0] - x).max(0.0).max(x - b[2]);
                let dz = (b[1] - z).max(0.0).max(z - b[3]);
                let distance = (dx * dx + dz * dz).sqrt();
                (distance <= radius).then_some(Candidate { index, distance })
            })
            .collect();
        out.sort_by(|a, b| {
            a.distance
                .total_cmp(&b.distance)
                .then(a.index.cmp(&b.index))
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> SpatialIndex {
        SpatialIndex::build(
            [
                [0.0, 0.0, 10.0, 10.0],
                [500.0, 500.0, 600.0, 520.0],
                [-300.0, -300.0, -290.0, 400.0],
                [5.0, 5.0, 6.0, 6.0],
            ],
            128.0,
        )
        .expect("valid")
    }

    #[test]
    fn finds_boxes_by_area_in_index_order() {
        let i = index();
        assert_eq!(i.len(), 4);
        assert_eq!(i.query_bounds([0.0, 0.0], [20.0, 20.0]), vec![0, 3]);
        assert_eq!(i.query_bounds([550.0, 510.0], [551.0, 511.0]), vec![1]);
        assert_eq!(i.query_bounds([-295.0, 399.0], [-295.0, 399.0]), vec![2]);
        assert!(
            i.query_bounds([1000.0, 1000.0], [1100.0, 1100.0])
                .is_empty()
        );
        // Touching counts as intersecting.
        assert_eq!(i.query_bounds([10.0, 10.0], [11.0, 11.0]), vec![0]);
    }

    #[test]
    fn nearest_candidates_are_sorted_with_box_distance() {
        let i = index();
        let hits = i.query_near(8.0, 8.0, 50.0);
        assert_eq!(hits.iter().map(|h| h.index).collect::<Vec<_>>(), vec![0, 3]);
        assert_eq!(hits[0].distance, 0.0);
        assert_eq!(hits[1].distance, 2.0_f64.sqrt() * 2.0);
        let far = i.query_near(0.0, 0.0, 100.0);
        assert!(far.iter().all(|h| h.distance <= 100.0));
    }

    #[test]
    fn huge_queries_fall_back_to_a_scan() {
        let i = index();
        let all = i.query_bounds([-1e12, -1e12], [1e12, 1e12]);
        assert_eq!(all, vec![0, 1, 2, 3]);
    }

    #[test]
    fn rejects_invalid_input() {
        assert_eq!(
            SpatialIndex::build([[0.0; 4]], 0.0).expect_err("must be rejected"),
            SpatialError::InvalidCellSize
        );
        assert_eq!(
            SpatialIndex::build([[1.0, 0.0, 0.0, 0.0]], 1.0).expect_err("must be rejected"),
            SpatialError::InvalidBox(0)
        );
        assert_eq!(
            SpatialIndex::build([[0.0, 0.0, f64::NAN, 0.0]], 1.0).expect_err("must be rejected"),
            SpatialError::InvalidBox(0)
        );
        assert_eq!(
            SpatialIndex::build([[0.0, 0.0, 1e9, 1e9]], 1.0).expect_err("must be rejected"),
            SpatialError::BoxTooLarge(0)
        );
        let i = index();
        assert!(i.query_bounds([f64::NAN, 0.0], [1.0, 1.0]).is_empty());
        assert!(i.query_near(0.0, 0.0, -1.0).is_empty());
        assert!(SpatialIndex::build([], 1.0).expect("empty").is_empty());
    }

    #[test]
    fn building_twice_gives_the_same_answers() {
        let (a, b) = (index(), index());
        for (x, z) in [(0.0, 0.0), (550.0, 510.0), (-295.0, 100.0)] {
            assert_eq!(a.query_near(x, z, 200.0), b.query_near(x, z, 200.0));
        }
    }
}
