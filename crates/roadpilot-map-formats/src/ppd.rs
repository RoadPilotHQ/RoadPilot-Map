//! Prefab descriptors (`.ppd`), format version 25.
//!
//! A descriptor defines a prefab (junction or other road piece) in local
//! coordinates: its nodes, the navigation curves that form its internal
//! lanes, and further blocks. The layout was established on every `.ppd`
//! referenced by the ETS2 and ATS prefab models (4,238 and 1,942 files); see
//! `docs/formats/scs-prefab-junctions.md`.
//!
//! ```text
//! 0x00  u32       version (25)
//! 0x04  u32[11]   counts: nodes, curves, signs, semaphores, spawn points,
//!                 terrain points, terrain variants, map points,
//!                 trigger points, intersections, last block
//! 0x30  u32[12]   block offsets, in that order, with terrain points using
//!                 two blocks (positions, normals); the first offset is 0x60
//! 0x60  blocks    contiguous; the last block runs to the end of the file
//! ```

use crate::cursor::Cursor;
use crate::error::{ParseError, Result};
use crate::token::Token;

/// The only descriptor version this parser reads.
pub const SUPPORTED_PPD_VERSION: u32 = 25;

const HEADER_LEN: usize = 0x60;
/// Number of input and output lane slots per node.
pub const NODE_LANE_SLOTS: usize = 8;
/// Number of next and previous links per curve.
pub const CURVE_LINKS: usize = 4;

/// Size of one entry for each block, in offset order. Terrain points have
/// two blocks (positions and normals) that both use the terrain point count.
const BLOCKS: [(usize, usize); 12] = [
    (0, 104),  // nodes
    (1, 132),  // navigation curves
    (2, 52),   // signs
    (3, 84),   // semaphores
    (4, 36),   // spawn points
    (5, 12),   // terrain point positions
    (5, 12),   // terrain point normals
    (6, 8),    // terrain point variants
    (7, 48),   // map points
    (8, 48),   // trigger points
    (9, 16),   // intersections
    (10, 188), // last block, meaning unknown
];

/// A prefab node in local coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PpdNode {
    pub position: [f32; 3],
    /// Unit direction vector.
    pub direction: [f32; 3],
    /// Curves that start at this node (lanes entering the prefab), by slot.
    pub input_slots: [Option<u16>; NODE_LANE_SLOTS],
    /// Curves that end at this node (lanes leaving the prefab), by slot.
    pub output_slots: [Option<u16>; NODE_LANE_SLOTS],
}

/// A navigation curve: one internal lane segment of the prefab.
#[derive(Debug, Clone, PartialEq)]
pub struct PpdCurve {
    pub name: Token,
    pub start: [f32; 3],
    pub end: [f32; 3],
    pub start_rotation: [f32; 4],
    pub end_rotation: [f32; 4],
    pub length: f32,
    /// Curves that continue this one.
    pub next: Vec<u16>,
    /// Curves that lead into this one.
    pub prev: Vec<u16>,
    /// A `traffic_rule_data` unit (`traffic_rule.<name>`), if set.
    pub traffic_rule: Option<Token>,
    /// The `i32` at record offset 116. Not a semaphore index and not a
    /// semaphore field; meaning unknown.
    pub unknown_116: i32,
}

/// A traffic light (semaphore) of the prefab.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PpdSemaphore {
    /// Local position.
    pub position: [f32; 3],
    /// Unit quaternion as stored.
    pub rotation: [f32; 4],
    /// Token at record offset 56. It matches the middle part of a
    /// `tr_semaphore_profile` unit name; how the full profile is chosen is
    /// not known.
    pub profile: Token,
}

/// Entry counts of the blocks that are not decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PpdOtherCounts {
    pub signs: u32,
    pub spawn_points: u32,
    pub terrain_points: u32,
    pub terrain_variants: u32,
    pub map_points: u32,
    pub trigger_points: u32,
    pub intersections: u32,
    pub last_block: u32,
}

/// A parsed prefab descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct PrefabDescriptor {
    pub nodes: Vec<PpdNode>,
    pub curves: Vec<PpdCurve>,
    pub semaphores: Vec<PpdSemaphore>,
    pub other: PpdOtherCounts,
}

fn vec3(c: &mut Cursor<'_>, what: &'static str) -> Result<[f32; 3]> {
    Ok([c.f32(what)?, c.f32(what)?, c.f32(what)?])
}

fn vec4(c: &mut Cursor<'_>, what: &'static str) -> Result<[f32; 4]> {
    Ok([c.f32(what)?, c.f32(what)?, c.f32(what)?, c.f32(what)?])
}

fn invalid(offset: usize, reason: &'static str) -> ParseError {
    ParseError::InvalidRecord { offset, reason }
}

/// Reads an `i32` curve index: -1 for none, otherwise below `curves`.
fn curve_index(c: &mut Cursor<'_>, curves: usize, what: &'static str) -> Result<Option<u16>> {
    let offset = c.pos();
    match c.i32(what)? {
        -1 => Ok(None),
        v => usize::try_from(v)
            .ok()
            .filter(|&i| i < curves)
            .and_then(|i| u16::try_from(i).ok())
            .map(Some)
            .ok_or(invalid(offset, "curve index out of range")),
    }
}

/// Parses a prefab descriptor.
///
/// # Errors
///
/// A [`ParseError`] for other versions, inconsistent block layout,
/// truncated data, out-of-range curve indices, and curve links whose stored
/// counts do not match.
pub fn parse_ppd(bytes: &[u8]) -> Result<PrefabDescriptor> {
    if bytes.is_empty() {
        return Err(ParseError::Empty);
    }
    let mut c = Cursor::new(bytes, 0);
    let version = c.u32("ppd header")?;
    if version != SUPPORTED_PPD_VERSION {
        return Err(ParseError::UnsupportedVersion { found: version });
    }
    let mut counts = [0usize; 11];
    for count in &mut counts {
        *count = c.u32("ppd header")? as usize;
    }
    let mut offsets = [0usize; 12];
    for offset in &mut offsets {
        *offset = c.u32("ppd header")? as usize;
    }
    // The blocks must be contiguous, in order, and end at the end of file.
    let mut expected = HEADER_LEN;
    for (k, &(count_index, size)) in BLOCKS.iter().enumerate() {
        if offsets[k] != expected {
            return Err(invalid(
                0x30 + 4 * k,
                "ppd block offset does not follow the previous block",
            ));
        }
        let bytes_len = counts[count_index]
            .checked_mul(size)
            .and_then(|b| expected.checked_add(b))
            .ok_or(invalid(0x30 + 4 * k, "ppd block size overflows"))?;
        expected = bytes_len;
    }
    if expected != bytes.len() {
        return Err(if expected > bytes.len() {
            ParseError::Truncated {
                offset: bytes.len(),
                needed: expected - bytes.len(),
                what: "ppd blocks",
            }
        } else {
            ParseError::TrailingBytes { offset: expected }
        });
    }
    let (node_count, curve_count, semaphore_count) = (counts[0], counts[1], counts[3]);

    let mut nodes = Vec::with_capacity(node_count);
    for i in 0..node_count {
        let mut c = Cursor::new(bytes, offsets[0] + 104 * i);
        c.skip(0x10, "ppd node")?;
        let position = vec3(&mut c, "ppd node position")?;
        let direction = vec3(&mut c, "ppd node direction")?;
        let mut input_slots = [None; NODE_LANE_SLOTS];
        let mut output_slots = [None; NODE_LANE_SLOTS];
        for slot in &mut input_slots {
            *slot = curve_index(&mut c, curve_count, "ppd node input lane")?;
        }
        for slot in &mut output_slots {
            *slot = curve_index(&mut c, curve_count, "ppd node output lane")?;
        }
        nodes.push(PpdNode {
            position,
            direction,
            input_slots,
            output_slots,
        });
    }

    let mut curves = Vec::with_capacity(curve_count);
    for i in 0..curve_count {
        let start_offset = offsets[1] + 132 * i;
        let mut c = Cursor::new(bytes, start_offset);
        let name = Token(c.u64("ppd curve name")?);
        c.skip(8, "ppd curve")?;
        let start = vec3(&mut c, "ppd curve start")?;
        let end = vec3(&mut c, "ppd curve end")?;
        let start_rotation = vec4(&mut c, "ppd curve rotation")?;
        let end_rotation = vec4(&mut c, "ppd curve rotation")?;
        let length = c.f32("ppd curve length")?;
        let mut next = Vec::new();
        let mut prev = Vec::new();
        for _ in 0..CURVE_LINKS {
            next.extend(curve_index(&mut c, curve_count, "ppd curve next")?);
        }
        for _ in 0..CURVE_LINKS {
            prev.extend(curve_index(&mut c, curve_count, "ppd curve prev")?);
        }
        let (next_count, prev_count) = (c.u32("ppd curve")?, c.u32("ppd curve")?);
        if next_count as usize != next.len() || prev_count as usize != prev.len() {
            return Err(invalid(
                start_offset,
                "ppd curve link counts do not match its links",
            ));
        }
        let unknown_116 = c.i32("ppd curve")?;
        let rule = c.u64("ppd curve traffic rule")?;
        curves.push(PpdCurve {
            name,
            start,
            end,
            start_rotation,
            end_rotation,
            length,
            next,
            prev,
            traffic_rule: (rule != 0).then_some(Token(rule)),
            unknown_116,
        });
    }

    // Every next link is mirrored by a prev link (holds for all game data).
    for (i, curve) in curves.iter().enumerate() {
        for &n in &curve.next {
            let mirrored = curves
                .get(usize::from(n))
                .is_some_and(|other| other.prev.iter().any(|&p| usize::from(p) == i));
            if !mirrored {
                return Err(invalid(
                    offsets[1] + 132 * i,
                    "ppd curve link is not mirrored",
                ));
            }
        }
    }

    let mut semaphores = Vec::with_capacity(semaphore_count);
    for i in 0..semaphore_count {
        let mut c = Cursor::new(bytes, offsets[3] + 84 * i);
        let position = vec3(&mut c, "ppd semaphore position")?;
        let rotation = vec4(&mut c, "ppd semaphore rotation")?;
        c.skip(56 - 28, "ppd semaphore")?;
        let profile = Token(c.u64("ppd semaphore profile")?);
        semaphores.push(PpdSemaphore {
            position,
            rotation,
            profile,
        });
    }

    let count = |i: usize| u32::try_from(counts[i]).unwrap_or(u32::MAX);
    Ok(PrefabDescriptor {
        nodes,
        curves,
        semaphores,
        other: PpdOtherCounts {
            signs: count(2),
            spawn_points: count(4),
            terrain_points: count(5),
            terrain_variants: count(6),
            map_points: count(7),
            trigger_points: count(8),
            intersections: count(9),
            last_block: count(10),
        },
    })
}
