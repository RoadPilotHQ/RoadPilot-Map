//! Item types and the length rules of their payloads.
//!
//! Item records have no length field. Each type has its own layout, and the
//! parser must know it to find the next item. The rules below were confirmed
//! by walking every `.base` file of a local ETS2 and ATS installation
//! (2,026 sectors, 1,113,691 items) exactly to the end of the file. See
//! `docs/formats/scs-sector-format.md`.
//!
//! Offsets in comments are relative to the start of the payload, which
//! follows the 57-byte common item header.

use crate::cursor::Cursor;
use crate::error::Result;
use crate::token::Token;

/// The type number of a map item.
///
/// Names come from the community tool ts-map (MIT license) and are labels
/// only. Only [`ItemType::ROAD`] and [`ItemType::PREFAB`] were confirmed
/// against the game definitions so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemType(pub u32);

impl ItemType {
    pub const TERRAIN: Self = Self(1);
    pub const BUILDING: Self = Self(2);
    pub const ROAD: Self = Self(3);
    pub const PREFAB: Self = Self(4);
    pub const MODEL: Self = Self(5);
    pub const COMPANY: Self = Self(6);
    pub const SERVICE: Self = Self(7);
    pub const CUT_PLANE: Self = Self(8);
    pub const CITY: Self = Self(12);
    pub const MAP_OVERLAY: Self = Self(18);
    pub const FERRY: Self = Self(19);
    pub const GARAGE: Self = Self(22);
    pub const TRIGGER: Self = Self(34);
    pub const FUEL_PUMP: Self = Self(35);
    pub const SIGN: Self = Self(36);
    pub const BUS_STOP: Self = Self(37);
    pub const TRAFFIC_RULE: Self = Self(38);
    pub const BEZIER_PATCH: Self = Self(39);
    pub const TRAJECTORY: Self = Self(41);
    pub const MAP_AREA: Self = Self(42);
    pub const CURVE: Self = Self(44);
    pub const CUTSCENE: Self = Self(46);
    pub const VISIBILITY_AREA: Self = Self(48);

    /// All types whose layout the parser knows.
    pub const SUPPORTED: [Self; 23] = [
        Self::TERRAIN,
        Self::BUILDING,
        Self::ROAD,
        Self::PREFAB,
        Self::MODEL,
        Self::COMPANY,
        Self::SERVICE,
        Self::CUT_PLANE,
        Self::CITY,
        Self::MAP_OVERLAY,
        Self::FERRY,
        Self::GARAGE,
        Self::TRIGGER,
        Self::FUEL_PUMP,
        Self::SIGN,
        Self::BUS_STOP,
        Self::TRAFFIC_RULE,
        Self::BEZIER_PATCH,
        Self::TRAJECTORY,
        Self::MAP_AREA,
        Self::CURVE,
        Self::CUTSCENE,
        Self::VISIBILITY_AREA,
    ];

    /// Label of a supported type, `None` for any other value.
    #[must_use]
    pub fn name(self) -> Option<&'static str> {
        Some(match self.0 {
            1 => "terrain",
            2 => "building",
            3 => "road",
            4 => "prefab",
            5 => "model",
            6 => "company",
            7 => "service",
            8 => "cut_plane",
            12 => "city",
            18 => "map_overlay",
            19 => "ferry",
            22 => "garage",
            34 => "trigger",
            35 => "fuel_pump",
            36 => "sign",
            37 => "bus_stop",
            38 => "traffic_rule",
            39 => "bezier_patch",
            41 => "trajectory",
            42 => "map_area",
            44 => "curve",
            46 => "cutscene",
            48 => "visibility_area",
            _ => return None,
        })
    }
}

/// Fields of a road item whose meaning is confirmed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Road {
    /// Road look, the name of a `road_look` unit (`road.<name>`).
    pub look: Token,
    /// Start node. Its `forward_item` is this road.
    pub start_node: u64,
    /// End node. Its `backward_item` is this road.
    pub end_node: u64,
    /// Stored length of the road, at payload offset 208. It equals the
    /// distance between the nodes for straight roads and the curve length
    /// for curved ones (within 0.1 % for 99.5 % of the inspected roads).
    /// How the game computes it is not confirmed.
    pub length: f32,
}

/// Fields of a prefab item whose meaning is confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefab {
    /// Prefab model, the name of a `prefab_model` unit (`prefab.<name>`).
    pub model: Token,
    /// Node UIDs of the prefab, in stored order.
    pub nodes: Vec<u64>,
    /// UIDs of items attached to the prefab (signs, models, companies,
    /// services and similar).
    pub connected_items: Vec<u64>,
}

/// Type-specific content that the parser exposes.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemData {
    Road(Road),
    Prefab(Prefab),
    /// Known layout, but no field is interpreted yet. The bytes are in
    /// [`Item::payload`](crate::Item::payload).
    Other,
}

/// Size of a road payload. Fixed.
const ROAD_PAYLOAD: usize = 212;

/// Decodes a road payload (already checked to be [`ROAD_PAYLOAD`] bytes).
fn road(payload: &[u8]) -> Result<Road> {
    let mut c = Cursor::new(payload, 4);
    let look = Token(c.u64("road look")?);
    c.skip(192 - 12, "road payload")?;
    let start_node = c.uid("road start node")?;
    let end_node = c.uid("road end node")?;
    let length = c.f32("road length")?;
    Ok(Road {
        look,
        start_node,
        end_node,
        length,
    })
}

/// Decodes a prefab payload that [`skip_payload`] already measured.
fn prefab(payload: &[u8]) -> Result<Prefab> {
    let mut c = Cursor::new(payload, 0);
    let model = Token(c.u64("prefab model")?);
    c.skip(8, "prefab payload")?;
    c.skip_array(8, "prefab additional parts")?;
    let nodes = c.uid_list("prefab node")?;
    let connected_items = c.uid_list("prefab connected item")?;
    Ok(Prefab {
        model,
        nodes,
        connected_items,
    })
}

/// Skips the payload of one item. Returns `None` for an unknown type.
///
/// Every loop below consumes at least one byte per iteration, so the work
/// is bounded by the input length whatever the counts say.
pub(crate) fn skip_payload(item_type: ItemType, c: &mut Cursor<'_>) -> Option<Result<()>> {
    let rule: fn(&mut Cursor<'_>) -> Result<()> = match item_type.0 {
        1 => terrain,
        2 => |c: &mut Cursor<'_>| {
            c.skip(44, "building")?;
            c.skip_array(4, "building list").map(drop)
        },
        3 => |c: &mut Cursor<'_>| c.skip(ROAD_PAYLOAD, "road"),
        4 => prefab_len,
        5 => |c: &mut Cursor<'_>| {
            c.skip(24, "model")?;
            c.skip_array(8, "model list")?;
            c.skip(36, "model")
        },
        6 => |c: &mut Cursor<'_>| {
            c.skip(32, "company")?;
            c.skip_array(12, "company list").map(drop)
        },
        7 => |c: &mut Cursor<'_>| {
            c.skip(16, "service")?;
            c.skip_array(8, "service list").map(drop)
        },
        8 => |c: &mut Cursor<'_>| c.skip_array(8, "cut plane node").map(drop),
        12 => |c: &mut Cursor<'_>| c.skip(24, "city"),
        18 => |c: &mut Cursor<'_>| c.skip(16, "map overlay"),
        19 => |c: &mut Cursor<'_>| c.skip(36, "ferry"),
        22 => |c: &mut Cursor<'_>| {
            c.skip(28, "garage")?;
            c.skip_array(8, "garage list").map(drop)
        },
        34 => trigger,
        35 => |c: &mut Cursor<'_>| c.skip(20, "fuel pump"),
        36 => sign,
        37 => |c: &mut Cursor<'_>| c.skip(24, "bus stop"),
        38 => |c: &mut Cursor<'_>| {
            c.skip_array(8, "traffic rule list")?;
            c.skip_array(8, "traffic rule list")?;
            c.skip(12, "traffic rule")
        },
        39 => |c: &mut Cursor<'_>| {
            c.skip(241, "bezier patch")?;
            c.skip_array(20, "bezier patch vegetation sphere")?;
            quad_info(c)
        },
        41 => |c: &mut Cursor<'_>| {
            c.skip_array(8, "trajectory node")?;
            c.skip(8, "trajectory")?;
            c.skip_array(28, "trajectory route rule")?;
            c.skip_array(16, "trajectory checkpoint")?;
            c.skip_array(8, "trajectory tag").map(drop)
        },
        42 => |c: &mut Cursor<'_>| {
            c.skip_array(8, "map area node")?;
            c.skip(4, "map area")
        },
        44 => curve,
        46 => cutscene,
        48 => |c: &mut Cursor<'_>| {
            c.skip(16, "visibility area")?;
            c.skip_array(8, "visibility area list").map(drop)
        },
        _ => return None,
    };
    Some(rule(c))
}

/// Decodes the confirmed fields of a payload that [`skip_payload`] measured.
pub(crate) fn decode(item_type: ItemType, payload: &[u8]) -> Result<ItemData> {
    Ok(match item_type {
        ItemType::ROAD => ItemData::Road(road(payload)?),
        ItemType::PREFAB => ItemData::Prefab(prefab(payload)?),
        _ => ItemData::Other,
    })
}

fn terrain(c: &mut Cursor<'_>) -> Result<()> {
    c.skip(238, "terrain")?;
    c.skip_array(20, "terrain vegetation sphere")?;
    quad_info(c)?;
    quad_info(c)?;
    c.skip(32, "terrain")
}

fn prefab_len(c: &mut Cursor<'_>) -> Result<()> {
    c.skip(16, "prefab")?;
    c.skip_array(8, "prefab additional parts")?;
    let nodes = c.skip_array(8, "prefab node")?;
    c.skip_array(8, "prefab connected item")?;
    // 8 + 1 + 1 bytes, 12 bytes per node, 8 bytes.
    let offset = c.pos();
    let per_node = c.fits(offset, u64::from(nodes), 12, "prefab node data")?;
    c.skip(10, "prefab")?;
    c.skip(per_node, "prefab node data")?;
    c.skip(8, "prefab")
}

fn trigger(c: &mut Cursor<'_>) -> Result<()> {
    c.skip_array(8, "trigger tag")?;
    let nodes = c.skip_array(8, "trigger node")?;
    let actions = c.u32("trigger action count")?;
    for _ in 0..actions {
        c.skip(8, "trigger action")?;
        let overrides = c.i32("trigger action override count")?;
        // A negative count means the action has nothing else (10,625 such
        // actions in the inspected game data).
        let Ok(overrides) = u64::try_from(overrides) else {
            continue;
        };
        let offset = c.pos();
        let bytes = c.fits(offset, overrides, 4, "trigger action override")?;
        c.skip(bytes, "trigger action override")?;
        let params = c.u32("trigger action parameter count")?;
        for _ in 0..params {
            string(c, "trigger action parameter")?;
        }
        c.skip_array(8, "trigger action target tag")?;
        c.skip(8, "trigger action")?;
    }
    if nodes == 1 {
        c.skip(4, "trigger")?;
    }
    Ok(())
}

fn sign(c: &mut Cursor<'_>) -> Result<()> {
    c.skip(32, "sign")?;
    let boards = c.u8("sign board count")?;
    c.skip(usize::from(boards) * 24, "sign board")?;
    let template = c.u32("sign template length")?;
    c.skip(4, "sign template length")?;
    if template == 0 {
        return Ok(());
    }
    let offset = c.pos();
    let len = c.fits(offset, u64::from(template), 1, "sign template")?;
    c.skip(len, "sign template")?;
    let board_overrides = c.u32("sign board override count")?;
    for _ in 0..board_overrides {
        c.skip(8, "sign board override")?;
        let flags = c.u8("sign board override flags")?;
        if flags & 0x01 != 0 {
            c.skip(2, "sign board override")?;
        }
        if flags & 0x02 != 0 {
            c.skip(8, "sign board override")?;
        }
    }
    let sign_overrides = c.u32("sign override count")?;
    for _ in 0..sign_overrides {
        c.skip(12, "sign override")?;
        let attributes = c.u32("sign override attribute count")?;
        for _ in 0..attributes {
            let kind = c.i16("sign override attribute type")?;
            c.skip(4, "sign override attribute")?;
            match kind {
                5 => string(c, "sign override attribute text")?,
                6 => c.skip(8, "sign override attribute value")?,
                1 => c.skip(1, "sign override attribute value")?,
                _ => c.skip(4, "sign override attribute value")?,
            }
        }
    }
    Ok(())
}

fn curve(c: &mut Cursor<'_>) -> Result<()> {
    c.skip(36, "curve")?;
    let mask = c.u32("curve sub-curve mask")?;
    for _ in 0..mask.count_ones() {
        c.skip(0x4C, "curve sub-curve")?;
        c.skip_array(4, "curve height offset")?;
        c.skip(0x14, "curve sub-curve")?;
    }
    Ok(())
}

fn cutscene(c: &mut Cursor<'_>) -> Result<()> {
    c.skip_array(8, "cutscene tag")?;
    c.skip(8, "cutscene node")?;
    let actions = c.u32("cutscene action count")?;
    for _ in 0..actions {
        c.skip_array(4, "cutscene action number")?;
        let strings = c.u32("cutscene action string count")?;
        for _ in 0..strings {
            string(c, "cutscene action string")?;
        }
        c.skip_array(8, "cutscene action target tag")?;
        c.skip(8, "cutscene action")?;
    }
    Ok(())
}

/// Surface data of terrain and bezier patch items.
fn quad_info(c: &mut Cursor<'_>) -> Result<()> {
    let offset = c.pos();
    let materials = c.u16("quad material count")?;
    let bytes = c.fits(offset, u64::from(materials), 10, "quad material")?;
    c.skip(bytes, "quad material")?;
    let offset = c.pos();
    let colors = c.u16("quad color count")?;
    let bytes = c.fits(offset, u64::from(colors), 4, "quad color")?;
    c.skip(bytes, "quad color")?;
    c.skip(4, "quad size")?;
    c.skip_array(4, "quad storage")?;
    c.skip_array(16, "quad offset")?;
    c.skip_array(16, "quad normal")?;
    Ok(())
}

/// A string with a `u64` length (stored as `u32` length and 4 zero bytes).
fn string(c: &mut Cursor<'_>, what: &'static str) -> Result<()> {
    let offset = c.pos();
    let len = c.u64(what)?;
    let bytes = c.fits(offset, len, 1, what)?;
    c.skip(bytes, what)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_type_has_a_name_and_a_rule() {
        for t in ItemType::SUPPORTED {
            assert!(t.name().is_some(), "{t:?}");
            // An empty payload is always too short, but the type is known.
            let mut c = Cursor::new(&[], 0);
            assert!(matches!(skip_payload(t, &mut c), Some(Err(_))), "{t:?}");
        }
    }

    #[test]
    fn unknown_types_have_no_rule() {
        for t in [0, 9, 13, 14, 40, 49, 50, u32::MAX] {
            let mut c = Cursor::new(&[0; 64], 0);
            assert!(skip_payload(ItemType(t), &mut c).is_none(), "{t}");
            assert_eq!(ItemType(t).name(), None);
        }
    }
}
