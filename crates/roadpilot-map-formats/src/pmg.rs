//! Reader for the geometry of road templates (`.pmg`, "Prism model
//! geometry"), version `0x506d6715`.
//!
//! The reader is deliberately narrow. It reads the **piece table** and the
//! **vertex positions** of each piece, nothing else: no normals, texture
//! coordinates, indices, materials by name or animation data. That is what
//! the study of road cross-sections needs (see
//! `docs/formats/scs-road-templates.md`).
//!
//! The layout was established by measurement on the road templates
//! (`road_template/**/*.pmg`) of local ETS2 and ATS installations: every one of
//! them has this version and the structure below. SCS publishes no byte-level
//! specification of the format (the SCS modding wiki only names the file
//! types). No third-party parser code or documentation was used.
//!
//! # Layout (little-endian, all offsets from the start of the file)
//!
//! | Offset | Type | Meaning | Evidence |
//! |---|---|---|---|
//! | 0 | `u32` | Version `0x506d6715` | Measured |
//! | 4 | `u32` | Number of pieces | Measured |
//! | `0x54` | `u32` | Offset of the piece table (equals the value at `0x50` in ETS2; ATS files have a block of other data before it) | Measured |
//! | `0x58` | `u32` | End of the piece table (offset + count × 100) | Measured |
//!
//! Piece record (100 bytes, words are `u32`): word 0 index count, word 1
//! vertex count, word 4 material number, words 9–11 minimum corner and 12–14
//! maximum corner of the piece bounds (`f32`), word 15 vertex stride in
//! bytes, word 16 offset of the position stream. A vertex position is three
//! `f32` at `position offset + vertex × stride`.
//!
//! All of this is *measured*, not specified: the meaning of the other words
//! is unknown and ignored. The reader rejects anything it cannot place
//! inside the file.

use std::fmt;

/// The only supported version.
pub const PMG_VERSION: u32 = 0x506d_6715;

/// Size of one piece record.
const PIECE_LEN: usize = 100;

/// Size of one part record.
const PART_LEN: usize = 24;

/// End of the fixed header (the last header word read is at `0x58`).
const HEADER_LEN: usize = 0x5c;

/// Limits that keep a hostile file from causing large allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PmgLimits {
    /// Most pieces accepted.
    pub max_pieces: u32,
    /// Most vertices per piece accepted.
    pub max_vertices_per_piece: u32,
    /// Most vertices over all pieces accepted.
    pub max_vertices: u32,
}

impl Default for PmgLimits {
    fn default() -> Self {
        Self {
            max_pieces: 4096,
            max_vertices_per_piece: 1 << 20,
            max_vertices: 1 << 22,
        }
    }
}

/// Why a `.pmg` file was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PmgError {
    /// The input is shorter than the fixed header.
    Truncated,
    /// The first four bytes are not a supported version. `found` is the
    /// value read; a value whose low 24 bits are not `0x6d6715` is not a
    /// PMG file of any known version.
    UnsupportedVersion { found: u32 },
    /// A count or limit is exceeded.
    LimitExceeded { what: &'static str, value: u64 },
    /// A table or stream does not lie inside the file, or its size overflows.
    OutOfBounds { what: &'static str },
    /// The table header is inconsistent (for example the end offset does not
    /// match the piece count).
    Inconsistent { what: &'static str },
    /// A vertex position or bound is not a finite number.
    NotFinite { piece: usize },
}

impl fmt::Display for PmgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("the file is shorter than the PMG header"),
            Self::UnsupportedVersion { found } => write!(
                f,
                "unsupported PMG version {found:#010x} (supported: {PMG_VERSION:#010x})"
            ),
            Self::LimitExceeded { what, value } => {
                write!(f, "{what} ({value}) exceeds the limit")
            }
            Self::OutOfBounds { what } => write!(f, "{what} lies outside the file"),
            Self::Inconsistent { what } => write!(f, "inconsistent PMG header: {what}"),
            Self::NotFinite { piece } => {
                write!(
                    f,
                    "piece {piece} has a position or bound that is not finite"
                )
            }
        }
    }
}

impl std::error::Error for PmgError {}

/// One piece (mesh part) of a model.
#[derive(Debug, Clone, PartialEq)]
pub struct PmgPiece {
    /// Material number (an index, not a name).
    pub material: u32,
    /// Number of indices as stored.
    pub index_count: u32,
    /// Minimum corner of the stored bounds.
    pub bounds_min: [f32; 3],
    /// Maximum corner of the stored bounds.
    pub bounds_max: [f32; 3],
    /// Vertex positions, in model units.
    pub positions: Vec<[f32; 3]>,
}

/// A part: a named group of consecutive pieces. The `.pmd` decides per
/// variant which parts are visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PmgPart {
    /// Name token (decode with [`crate::Token`]).
    pub name: u64,
    /// The pieces of the part, as indices into [`PmgModel::pieces`].
    pub first_piece: u32,
    pub piece_count: u32,
    /// Record words 4 and 5. They are 0 except in 10 ATS templates, which
    /// also have one 44-byte record between the part and the piece table;
    /// presumably locators. Not read further; meaning unknown.
    pub unknown_count: u32,
    pub unknown_first: u32,
}

/// The parts and pieces of a model with their vertex positions.
#[derive(Debug, Clone, PartialEq)]
pub struct PmgModel {
    pub parts: Vec<PmgPart>,
    pub pieces: Vec<PmgPiece>,
}

impl PmgModel {
    /// The index of the part that contains `piece`, if any.
    #[must_use]
    pub fn part_of(&self, piece: usize) -> Option<usize> {
        self.parts.iter().position(|p| {
            let start = p.first_piece as usize;
            piece >= start && piece < start + p.piece_count as usize
        })
    }
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn float(bytes: &[u8], at: usize) -> Option<f32> {
    word(bytes, at).map(f32::from_bits)
}

/// Parses a `.pmg` file with the default [`PmgLimits`].
///
/// # Errors
///
/// [`PmgError`]: wrong version, a table or stream outside the file, a count
/// beyond the limits, an inconsistent header, or non-finite numbers.
pub fn parse_pmg(bytes: &[u8]) -> Result<PmgModel, PmgError> {
    parse_pmg_with_limits(bytes, &PmgLimits::default())
}

/// Parses a `.pmg` file with explicit limits.
///
/// # Errors
///
/// See [`parse_pmg`].
pub fn parse_pmg_with_limits(bytes: &[u8], limits: &PmgLimits) -> Result<PmgModel, PmgError> {
    let (Some(version), Some(count), Some(table), Some(table_end)) = (
        word(bytes, 0),
        word(bytes, 4),
        word(bytes, 0x54),
        word(bytes, 0x58),
    ) else {
        return Err(PmgError::Truncated);
    };
    if version != PMG_VERSION {
        return Err(PmgError::UnsupportedVersion { found: version });
    }
    if count > limits.max_pieces {
        return Err(PmgError::LimitExceeded {
            what: "piece count",
            value: u64::from(count),
        });
    }
    let table = table as usize;
    let table_len = (count as usize)
        .checked_mul(PIECE_LEN)
        .ok_or(PmgError::OutOfBounds {
            what: "piece table",
        })?;
    let end = table.checked_add(table_len).ok_or(PmgError::OutOfBounds {
        what: "piece table",
    })?;
    if end > bytes.len() {
        return Err(PmgError::OutOfBounds {
            what: "piece table",
        });
    }
    if table_end as usize != end {
        return Err(PmgError::Inconsistent {
            what: "the piece table end does not match the piece count",
        });
    }

    // Part table: count at word 8, start at 0x48, 24-byte records, before
    // the piece table.
    let (Some(part_count), Some(part_table)) = (word(bytes, 8), word(bytes, 0x48)) else {
        return Err(PmgError::Truncated);
    };
    if part_count > limits.max_pieces {
        return Err(PmgError::LimitExceeded {
            what: "part count",
            value: u64::from(part_count),
        });
    }
    let part_table = part_table as usize;
    let mut parts = Vec::with_capacity(part_count as usize);
    if part_count > 0 {
        let part_end = part_table
            .checked_add(part_count as usize * PART_LEN)
            .ok_or(PmgError::OutOfBounds { what: "part table" })?;
        if part_table < HEADER_LEN || part_end > table {
            return Err(PmgError::OutOfBounds { what: "part table" });
        }
        for i in 0..part_count as usize {
            let at = part_table + i * PART_LEN;
            let w = |k: usize| word(bytes, at + 4 * k).ok_or(PmgError::Truncated);
            let part = PmgPart {
                name: u64::from(w(0)?) | (u64::from(w(1)?) << 32),
                piece_count: w(2)?,
                first_piece: w(3)?,
                unknown_count: w(4)?,
                unknown_first: w(5)?,
            };
            if u64::from(part.first_piece) + u64::from(part.piece_count) > u64::from(count) {
                return Err(PmgError::Inconsistent {
                    what: "a part names pieces that do not exist",
                });
            }
            parts.push(part);
        }
    }

    let mut pieces = Vec::new();
    let mut total_vertices = 0u64;
    for i in 0..count as usize {
        let at = table + i * PIECE_LEN;
        let w = |k: usize| word(bytes, at + 4 * k).ok_or(PmgError::Truncated);
        let f = |k: usize| float(bytes, at + 4 * k).ok_or(PmgError::Truncated);
        let vertex_count = w(1)?;
        if vertex_count > limits.max_vertices_per_piece {
            return Err(PmgError::LimitExceeded {
                what: "vertices per piece",
                value: u64::from(vertex_count),
            });
        }
        total_vertices += u64::from(vertex_count);
        if total_vertices > u64::from(limits.max_vertices) {
            return Err(PmgError::LimitExceeded {
                what: "vertices",
                value: total_vertices,
            });
        }
        let stride = w(15)? as usize;
        let stream = w(16)? as usize;
        if vertex_count > 0 && stride < 12 {
            return Err(PmgError::Inconsistent {
                what: "the vertex stride is below 12 bytes",
            });
        }
        // Last vertex must lie completely inside the file.
        if vertex_count > 0 {
            let last = (vertex_count as usize - 1)
                .checked_mul(stride)
                .and_then(|o| o.checked_add(stream))
                .and_then(|o| o.checked_add(12))
                .ok_or(PmgError::OutOfBounds {
                    what: "position stream",
                })?;
            if last > bytes.len() {
                return Err(PmgError::OutOfBounds {
                    what: "position stream",
                });
            }
        }
        let mut positions = Vec::with_capacity(vertex_count as usize);
        for v in 0..vertex_count as usize {
            let o = stream + v * stride;
            let p = [
                float(bytes, o).ok_or(PmgError::Truncated)?,
                float(bytes, o + 4).ok_or(PmgError::Truncated)?,
                float(bytes, o + 8).ok_or(PmgError::Truncated)?,
            ];
            if !p.iter().all(|c| c.is_finite()) {
                return Err(PmgError::NotFinite { piece: i });
            }
            positions.push(p);
        }
        let bounds_min = [f(9)?, f(10)?, f(11)?];
        let bounds_max = [f(12)?, f(13)?, f(14)?];
        if !bounds_min.iter().chain(&bounds_max).all(|c| c.is_finite()) {
            return Err(PmgError::NotFinite { piece: i });
        }
        pieces.push(PmgPiece {
            material: w(4)?,
            index_count: w(0)?,
            bounds_min,
            bounds_max,
            positions,
        });
    }
    Ok(PmgModel { parts, pieces })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds a synthetic PMG (invented geometry, not game data).
    pub(crate) fn build(pieces: &[(u32, Vec<[f32; 3]>)], stride: usize) -> Vec<u8> {
        build_with_parts(pieces, &[], stride)
    }

    /// As [`build`], with parts `(name, first piece, piece count)`.
    pub(crate) fn build_with_parts(
        pieces: &[(u32, Vec<[f32; 3]>)],
        parts: &[(u64, u32, u32)],
        stride: usize,
    ) -> Vec<u8> {
        let part_table = 0x60usize;
        let table = part_table + parts.len() * PART_LEN;
        let table_end = table + pieces.len() * PIECE_LEN;
        let mut out = vec![0u8; table_end];
        let put = |out: &mut Vec<u8>, at: usize, v: u32| {
            out[at..at + 4].copy_from_slice(&v.to_le_bytes())
        };
        put(&mut out, 0, PMG_VERSION);
        put(&mut out, 4, pieces.len() as u32);
        put(&mut out, 8, parts.len() as u32);
        put(&mut out, 0x48, part_table as u32);
        for (i, (name, first, n)) in parts.iter().enumerate() {
            let at = part_table + i * PART_LEN;
            put(&mut out, at, *name as u32);
            put(&mut out, at + 4, (*name >> 32) as u32);
            put(&mut out, at + 8, *n);
            put(&mut out, at + 12, *first);
        }
        put(&mut out, 0x54, table as u32);
        put(&mut out, 0x58, table_end as u32);
        for (i, (material, positions)) in pieces.iter().enumerate() {
            let at = table + i * PIECE_LEN;
            let stream = out.len();
            for p in positions {
                let mut v = vec![0u8; stride];
                for (k, c) in p.iter().enumerate() {
                    v[k * 4..k * 4 + 4].copy_from_slice(&c.to_le_bytes());
                }
                out.extend(v);
            }
            put(&mut out, at, positions.len() as u32 * 3);
            put(&mut out, at + 4, positions.len() as u32);
            put(&mut out, at + 16, *material);
            let min = positions.iter().fold([f32::MAX; 3], |m, p| {
                [m[0].min(p[0]), m[1].min(p[1]), m[2].min(p[2])]
            });
            let max = positions.iter().fold([f32::MIN; 3], |m, p| {
                [m[0].max(p[0]), m[1].max(p[1]), m[2].max(p[2])]
            });
            for k in 0..3 {
                put(&mut out, at + 36 + 4 * k, min[k].to_bits());
                put(&mut out, at + 48 + 4 * k, max[k].to_bits());
            }
            put(&mut out, at + 60, stride as u32);
            put(&mut out, at + 64, stream as u32);
        }
        out
    }

    #[test]
    fn reads_pieces_and_positions() {
        let quad = vec![
            [0.0, 0.0, 0.0],
            [4.5, 0.0, 0.0],
            [4.5, 0.0, -5.0],
            [0.0, 0.0, -5.0],
        ];
        let bytes = build(&[(0, quad.clone()), (3, vec![[-1.0, 0.5, 2.0]])], 36);
        let model = parse_pmg(&bytes).expect("valid");
        assert_eq!(model.pieces.len(), 2);
        assert_eq!(model.pieces[0].positions, quad);
        assert_eq!(model.pieces[0].bounds_max, [4.5, 0.0, 0.0]);
        assert_eq!(model.pieces[0].index_count, 12);
        assert_eq!(model.pieces[1].material, 3);
        // Another stride works the same.
        assert_eq!(
            parse_pmg(&build(&[(0, quad.clone())], 44))
                .expect("valid")
                .pieces[0]
                .positions,
            quad
        );
    }

    #[test]
    fn empty_model_is_valid() {
        assert_eq!(parse_pmg(&build(&[], 36)).expect("valid").pieces.len(), 0);
    }

    #[test]
    fn rejects_other_versions_and_short_input() {
        let mut bytes = build(&[(0, vec![[0.0; 3]])], 36);
        assert_eq!(parse_pmg(&bytes[..0x40]), Err(PmgError::Truncated));
        assert_eq!(parse_pmg(&[]), Err(PmgError::Truncated));
        bytes[0..4].copy_from_slice(&0x506d_6714u32.to_le_bytes());
        assert_eq!(
            parse_pmg(&bytes),
            Err(PmgError::UnsupportedVersion { found: 0x506d_6714 })
        );
    }

    #[test]
    fn rejects_tables_and_streams_outside_the_file() {
        let good = build(&[(0, vec![[0.0; 3]; 3])], 36);
        // Piece table offset beyond the file.
        let mut b = good.clone();
        b[0x54..0x58].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(parse_pmg(&b), Err(PmgError::OutOfBounds { .. })));
        // Table end that disagrees with the count.
        let mut b = good.clone();
        b[0x58..0x5c].copy_from_slice(&0x70u32.to_le_bytes());
        assert!(matches!(parse_pmg(&b), Err(PmgError::Inconsistent { .. })));
        // Position stream beyond the file.
        let mut b = good.clone();
        b[0x60 + 64..0x60 + 68].copy_from_slice(&0x00ff_ffffu32.to_le_bytes());
        assert!(matches!(parse_pmg(&b), Err(PmgError::OutOfBounds { .. })));
        // Truncated vertex data.
        assert!(matches!(
            parse_pmg(&good[..good.len() - 30]),
            Err(PmgError::OutOfBounds { .. })
        ));
        // Stride too small.
        let mut b = good;
        b[0x60 + 60..0x60 + 64].copy_from_slice(&4u32.to_le_bytes());
        assert!(matches!(parse_pmg(&b), Err(PmgError::Inconsistent { .. })));
    }

    #[test]
    fn enforces_limits_before_allocating() {
        let mut b = build(&[(0, vec![[0.0; 3]])], 36);
        b[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            parse_pmg(&b),
            Err(PmgError::LimitExceeded {
                what: "piece count",
                ..
            })
        ));
        let mut b = build(&[(0, vec![[0.0; 3]])], 36);
        b[0x60 + 4..0x60 + 8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(parse_pmg(&b), Err(PmgError::LimitExceeded { .. })));
        // Stride and count overflow cannot wrap.
        let mut b = build(&[(0, vec![[0.0; 3]])], 36);
        b[0x60 + 4..0x60 + 8].copy_from_slice(&(1u32 << 20).to_le_bytes());
        b[0x60 + 60..0x60 + 64].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(parse_pmg(&b), Err(PmgError::OutOfBounds { .. })));
        let tight = PmgLimits {
            max_pieces: 1,
            max_vertices_per_piece: 2,
            max_vertices: 2,
        };
        assert!(matches!(
            parse_pmg_with_limits(&build(&[(0, vec![[0.0; 3]; 3])], 36), &tight),
            Err(PmgError::LimitExceeded {
                what: "vertices per piece",
                ..
            })
        ));
    }

    #[test]
    fn rejects_non_finite_numbers() {
        let b = build(&[(0, vec![[f32::NAN, 0.0, 0.0]])], 36);
        assert_eq!(parse_pmg(&b), Err(PmgError::NotFinite { piece: 0 }));
    }

    #[test]
    fn random_bytes_never_panic() {
        // A cheap deterministic fuzz: prefixes and byte flips of a valid file.
        let good = build(&[(0, vec![[0.0, 1.0, 2.0]; 5]), (1, vec![[3.0; 3]; 2])], 36);
        for n in 0..good.len() {
            let _ = parse_pmg(&good[..n]);
        }
        let mut seed = 0x1234_5678u32;
        for _ in 0..2000 {
            let mut b = good.clone();
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let i = (seed >> 8) as usize % b.len();
            b[i] ^= (seed & 0xff) as u8 | 1;
            let _ = parse_pmg(&b);
        }
    }
}

#[cfg(test)]
mod part_tests {
    use super::tests::build_with_parts;
    use super::*;

    fn three_pieces() -> Vec<(u32, Vec<[f32; 3]>)> {
        vec![
            (0, vec![[0.0; 3]]),
            (1, vec![[1.0, 0.0, 0.0]]),
            (1, vec![[2.0, 0.0, 0.0]]),
        ]
    }

    #[test]
    fn reads_parts_and_maps_pieces_to_parts() {
        let b = build_with_parts(&three_pieces(), &[(7, 0, 1), (8, 1, 2)], 36);
        let model = parse_pmg(&b).expect("valid");
        assert_eq!(model.parts.len(), 2);
        assert_eq!(model.parts[1].name, 8);
        assert_eq!(
            (model.parts[1].first_piece, model.parts[1].piece_count),
            (1, 2)
        );
        assert_eq!(model.part_of(0), Some(0));
        assert_eq!(model.part_of(2), Some(1));
        assert_eq!(model.part_of(3), None);
    }

    #[test]
    fn rejects_parts_that_name_missing_pieces_or_overlap_the_piece_table() {
        let b = build_with_parts(&three_pieces(), &[(7, 2, 2)], 36);
        assert!(matches!(parse_pmg(&b), Err(PmgError::Inconsistent { .. })));
        // Part table that runs into the piece table.
        let mut b = build_with_parts(&three_pieces(), &[(7, 0, 3)], 36);
        b[8..12].copy_from_slice(&2u32.to_le_bytes());
        assert!(matches!(parse_pmg(&b), Err(PmgError::OutOfBounds { .. })));
        // Part table inside the header.
        let mut b = build_with_parts(&three_pieces(), &[(7, 0, 3)], 36);
        b[0x48..0x4c].copy_from_slice(&8u32.to_le_bytes());
        assert!(matches!(parse_pmg(&b), Err(PmgError::OutOfBounds { .. })));
        // Part count beyond the limit.
        let mut b = build_with_parts(&three_pieces(), &[(7, 0, 3)], 36);
        b[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(parse_pmg(&b), Err(PmgError::LimitExceeded { .. })));
    }
}
