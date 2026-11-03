//! Sector `.base` and `.data` files.

use crate::cursor::Cursor;
use crate::error::{ParseError, Result};
use crate::items::{self, ItemData, ItemType};

/// The only sector format version this parser reads. All `.base` and
/// `.data` files of the inspected ETS2 and ATS installations have it.
pub const SUPPORTED_VERSION: u32 = 907;

/// Size of the header shared by `.base` and `.data` files.
const HEADER_LEN: usize = 16;
/// Common item header: type, UID, bounds and 5 flag bytes.
const ITEM_HEADER_LEN: usize = 57;
const NODE_LEN: usize = 56;

/// Upper bounds applied while parsing.
///
/// The defaults are far above the inspected game data (largest `.base` file
/// about 3 MB with 3,417 items) and only stop runaway input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest accepted input in bytes.
    pub max_file_size: usize,
    /// Largest accepted item count of a `.base` file.
    pub max_items: u32,
    /// Largest accepted node count of a `.base` file.
    pub max_nodes: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_file_size: 256 * 1024 * 1024,
            max_items: 1_000_000,
            max_nodes: 4_000_000,
        }
    }
}

/// The 16-byte header of `.base` and `.data` files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectorHeader {
    /// Format version, [`SUPPORTED_VERSION`].
    pub version: u32,
    /// The three words after the version. They are identical in every
    /// inspected file (`7668833`, `0`, `3`); their meaning is unknown.
    pub unknown: [u32; 3],
}

/// The bounding volume of an item: minimum and maximum along five axes.
///
/// `min[i] <= max[i]` holds for every inspected item. The first three axes
/// match world X, Y and Z (node positions lie inside them); the other two are
/// presumably diagonal axes (a k-DOP), which is not confirmed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: [f32; 5],
    pub max: [f32; 5],
}

/// One map item of a `.base` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Item<'a> {
    pub item_type: ItemType,
    /// Unique ID of the item. Unique within each game in the inspected data.
    pub uid: u64,
    pub bounds: Bounds,
    /// The five bytes after the bounds. Their meaning is not confirmed.
    pub flags: [u8; 5],
    /// Byte offset of the item in the file.
    pub offset: usize,
    /// Type-specific bytes after the common header.
    pub payload: &'a [u8],
    /// Decoded fields whose meaning is confirmed.
    pub data: ItemData,
}

/// A node: a point that items start or end at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Node {
    pub uid: u64,
    /// Position as stored: fixed point with 8 fractional bits (X, Y, Z).
    pub position: [i32; 3],
    /// Four floats that always form a unit quaternion. The component order is
    /// not confirmed.
    pub rotation: [f32; 4],
    /// The item that ends at this node, or 0.
    pub backward_item: u64,
    /// The item that starts at this node, or 0.
    pub forward_item: u64,
    /// Meaning not confirmed.
    pub flags: u32,
}

impl Node {
    /// Position with the fixed point resolved (`position / 256`). The
    /// values are in the same units as item [`Bounds`]; whether these are
    /// meters is tracked as open question T3.
    #[must_use]
    pub fn position_units(&self) -> [f64; 3] {
        self.position.map(|v| f64::from(v) / 256.0)
    }
}

/// A parsed `.base` sector file.
#[derive(Debug, Clone, PartialEq)]
pub struct BaseSector<'a> {
    pub header: SectorHeader,
    pub items: Vec<Item<'a>>,
    pub nodes: Vec<Node>,
    /// The UID list at the end of the file. All entries are items of the
    /// game; ts-map describes them as children of visibility areas (not
    /// confirmed).
    pub trailing_item_uids: Vec<u64>,
}

/// One entry of a `.data` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataEntry<'a> {
    /// The road or prefab item the entry belongs to.
    pub item_uid: u64,
    pub item_type: ItemType,
    /// Byte offset of the entry (its UID) in the file.
    pub offset: usize,
    /// The bytes after the UID. Their layout is not decoded yet.
    pub payload: &'a [u8],
}

/// A parsed `.data` sector file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataSector<'a> {
    pub header: SectorHeader,
    pub entries: Vec<DataEntry<'a>>,
}

fn check_size(bytes: &[u8], limits: &Limits) -> Result<()> {
    if bytes.is_empty() {
        return Err(ParseError::Empty);
    }
    if bytes.len() > limits.max_file_size {
        return Err(ParseError::LimitExceeded {
            what: "file size",
            value: bytes.len() as u64,
            limit: limits.max_file_size as u64,
        });
    }
    Ok(())
}

fn header(c: &mut Cursor<'_>) -> Result<SectorHeader> {
    let version = c.u32("sector header")?;
    if version != SUPPORTED_VERSION {
        return Err(ParseError::UnsupportedVersion { found: version });
    }
    let unknown = [
        c.u32("sector header")?,
        c.u32("sector header")?,
        c.u32("sector header")?,
    ];
    Ok(SectorHeader { version, unknown })
}

fn limit(what: &'static str, value: u32, limit: u32) -> Result<()> {
    if value > limit {
        return Err(ParseError::LimitExceeded {
            what,
            value: u64::from(value),
            limit: u64::from(limit),
        });
    }
    Ok(())
}

/// Parses a `.base` sector file with the default [`Limits`].
///
/// # Errors
///
/// A [`ParseError`] for unsupported versions, unknown item types and any
/// truncated, inconsistent or trailing data.
pub fn parse_base(bytes: &[u8]) -> Result<BaseSector<'_>> {
    parse_base_with_limits(bytes, &Limits::default())
}

/// Parses a `.base` sector file.
///
/// # Errors
///
/// As [`parse_base`], plus [`ParseError::LimitExceeded`].
pub fn parse_base_with_limits<'a>(bytes: &'a [u8], limits: &Limits) -> Result<BaseSector<'a>> {
    check_size(bytes, limits)?;
    let mut c = Cursor::new(bytes, 0);
    let header = header(&mut c)?;

    let count_at = c.pos();
    let count = c.u32("item count")?;
    limit("item count", count, limits.max_items)?;
    // Every item needs at least its header; reserve no more than that allows.
    c.fits(count_at, u64::from(count), ITEM_HEADER_LEN, "item")?;
    let mut items = Vec::with_capacity(count as usize);
    for index in 0..count as usize {
        items.push(item(&mut c, index)?);
    }

    let count_at = c.pos();
    let count = c.u32("node count")?;
    limit("node count", count, limits.max_nodes)?;
    c.fits(count_at, u64::from(count), NODE_LEN, "node")?;
    let mut nodes = Vec::with_capacity(count as usize);
    for _ in 0..count {
        nodes.push(node(&mut c)?);
    }

    let trailing_item_uids = c.uid_list("trailing item uid")?;
    if c.remaining() != 0 {
        return Err(ParseError::TrailingBytes { offset: c.pos() });
    }
    Ok(BaseSector {
        header,
        items,
        nodes,
        trailing_item_uids,
    })
}

fn item<'a>(c: &mut Cursor<'a>, index: usize) -> Result<Item<'a>> {
    let offset = c.pos();
    let item_type = ItemType(c.u32("item type")?);
    let uid = c.uid("item uid")?;
    let mut min = [0f32; 5];
    let mut max = [0f32; 5];
    for v in &mut min {
        *v = c.f32("item bounds")?;
    }
    for v in &mut max {
        *v = c.f32("item bounds")?;
    }
    if !min.iter().chain(&max).all(|v| v.is_finite()) || min.iter().zip(&max).any(|(a, b)| a > b) {
        return Err(ParseError::InvalidRecord {
            offset,
            reason: "item bounds are not finite or have min > max",
        });
    }
    let mut flags = [0u8; 5];
    flags.copy_from_slice(c.take(5, "item flags")?);

    let payload_start = c.pos();
    let Some(result) = items::skip_payload(item_type, c) else {
        return Err(ParseError::UnknownItemType {
            index,
            offset,
            item_type: item_type.0,
        });
    };
    result?;
    // The cursor only moved forward over checked ranges.
    let mut whole = Cursor::new(c.source(), payload_start);
    let payload = whole.take(c.pos() - payload_start, "item payload")?;
    let data = items::decode(item_type, payload)?;
    Ok(Item {
        item_type,
        uid,
        bounds: Bounds { min, max },
        flags,
        offset,
        payload,
        data,
    })
}

fn node(c: &mut Cursor<'_>) -> Result<Node> {
    let offset = c.pos();
    let uid = c.uid("node uid")?;
    let position = [
        c.i32("node position")?,
        c.i32("node position")?,
        c.i32("node position")?,
    ];
    let rotation = [
        c.f32("node rotation")?,
        c.f32("node rotation")?,
        c.f32("node rotation")?,
        c.f32("node rotation")?,
    ];
    let norm: f32 = rotation.iter().map(|v| v * v).sum::<f32>().sqrt();
    if !norm.is_finite() || (norm - 1.0).abs() > 1e-3 {
        return Err(ParseError::InvalidRecord {
            offset,
            reason: "node rotation is not a unit quaternion",
        });
    }
    Ok(Node {
        uid,
        position,
        rotation,
        backward_item: c.u64("node backward item")?,
        forward_item: c.u64("node forward item")?,
        flags: c.u32("node flags")?,
    })
}

/// Parses the `.data` file that belongs to `base` (same sector).
///
/// The file holds one entry per road and prefab item of `base`, in the same
/// order, then the end marker `u64::MAX`. Entries have no length field:
/// each entry ends where the UID of the next expected item appears. In the
/// inspected game data no item UID occurs inside another entry, but the
/// format does not rule it out (see `docs/formats/scs-sector-format.md`).
///
/// # Errors
///
/// [`ParseError::DataEntryMissing`] if an entry is not where it is
/// expected, plus the header and size errors of [`parse_base`].
pub fn parse_data<'a>(bytes: &'a [u8], base: &BaseSector<'_>) -> Result<DataSector<'a>> {
    parse_data_with_limits(bytes, base, &Limits::default())
}

/// Parses a `.data` file with custom [`Limits`].
///
/// # Errors
///
/// As [`parse_data`].
pub fn parse_data_with_limits<'a>(
    bytes: &'a [u8],
    base: &BaseSector<'_>,
    limits: &Limits,
) -> Result<DataSector<'a>> {
    check_size(bytes, limits)?;
    let mut c = Cursor::new(bytes, 0);
    let header = header(&mut c)?;
    let end_marker_at = bytes
        .len()
        .checked_sub(8)
        .filter(|&at| at >= HEADER_LEN)
        .ok_or(ParseError::Truncated {
            offset: HEADER_LEN,
            needed: 8,
            what: "data end marker",
        })?;
    let mut marker = Cursor::new(bytes, end_marker_at);
    if marker.u64("data end marker")? != u64::MAX {
        return Err(ParseError::InvalidRecord {
            offset: end_marker_at,
            reason: "the data file does not end with the end marker",
        });
    }

    let expected: Vec<&Item<'_>> = base
        .items
        .iter()
        .filter(|i| matches!(i.item_type, ItemType::ROAD | ItemType::PREFAB))
        .collect();
    let mut entries = Vec::with_capacity(expected.len());
    let mut pos = HEADER_LEN;
    for (index, item) in expected.iter().enumerate() {
        let missing = ParseError::DataEntryMissing {
            index,
            item_uid: item.uid,
            offset: pos,
        };
        let uid_end = pos.checked_add(8).filter(|&e| e <= end_marker_at);
        let Some(uid_end) = uid_end else {
            return Err(missing);
        };
        if bytes.get(pos..uid_end) != Some(&item.uid.to_le_bytes()[..]) {
            return Err(missing);
        }
        let end = match expected.get(index + 1) {
            None => end_marker_at,
            Some(next) => find(bytes, uid_end, end_marker_at, next.uid).ok_or(
                ParseError::DataEntryMissing {
                    index: index + 1,
                    item_uid: next.uid,
                    offset: uid_end,
                },
            )?,
        };
        let payload = bytes.get(uid_end..end).ok_or(missing)?;
        entries.push(DataEntry {
            item_uid: item.uid,
            item_type: item.item_type,
            offset: pos,
            payload,
        });
        pos = end;
    }
    if pos != end_marker_at {
        return Err(ParseError::TrailingBytes { offset: pos });
    }
    Ok(DataSector { header, entries })
}

/// First position in `bytes[from..to]` where `uid` is stored.
fn find(bytes: &[u8], from: usize, to: usize, uid: u64) -> Option<usize> {
    let needle = uid.to_le_bytes();
    bytes
        .get(from..to)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| from + i)
}

#[cfg(test)]
mod tests {
    use super::find;

    #[test]
    fn find_searches_only_the_given_range() {
        let mut bytes = vec![0u8; 4];
        bytes.extend(7u64.to_le_bytes());
        assert_eq!(find(&bytes, 0, bytes.len(), 7), Some(4));
        assert_eq!(find(&bytes, 0, 11, 7), None);
        assert_eq!(find(&bytes, 5, bytes.len(), 7), None);
        assert_eq!(find(&bytes, 20, 30, 7), None);
    }
}
