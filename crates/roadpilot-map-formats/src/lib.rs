//! Parsers for SCS map files of Euro Truck Simulator 2 and American Truck
//! Simulator, producing format-close records.
//!
//! This crate reads the binary **sector** files of a map
//! (`map/<map>/sec<x><z>.base` and `.data`), format version 907, and the few
//! **text SII definitions** the lane model needs ([`sii`], [`definitions`]).
//! It works on byte slices and does no I/O; reading the files from the game
//! archives is the job of `roadpilot-map-archive`.
//!
//! ```no_run
//! # fn bytes() -> (Vec<u8>, Vec<u8>) { (Vec::new(), Vec::new()) }
//! use roadpilot_map_formats::{parse_base, parse_data, ItemData};
//!
//! # fn main() -> Result<(), roadpilot_map_formats::ParseError> {
//! let (base_bytes, data_bytes) = bytes();
//! let base = parse_base(&base_bytes)?;
//! for item in &base.items {
//!     if let ItemData::Road(road) = &item.data {
//!         println!("road {:016x} from node {:016x} to {:016x}", item.uid, road.start_node, road.end_node);
//!     }
//! }
//! let data = parse_data(&data_bytes, &base)?;
//! # let _ = data;
//! # Ok(())
//! # }
//! ```
//!
//! # What is decoded
//!
//! - The header, every item's common header (type, UID, bounds, flag bytes)
//!   and its payload bytes, all nodes, and the trailing UID list.
//! - Typed fields only where their meaning is confirmed against the game
//!   data: road look and start/end nodes of roads, model, nodes and attached
//!   items of prefabs.
//! - `.data` files are split into one entry per road and prefab; the entry
//!   contents are not decoded.
//!
//! Item records have no length field, so an item of an unknown type stops
//! parsing with [`ParseError::UnknownItemType`].
//!
//! # Robustness
//!
//! Input is untrusted. All reads are bounds-checked, size computations use
//! checked arithmetic, counts are checked against the remaining input before
//! memory is reserved, there is no recursion, and every loop consumes input,
//! so malformed data returns an error instead of panicking or hanging.

mod cursor;
pub mod definitions;
mod error;
mod items;
pub mod ppd;
mod sector;
pub mod sii;
mod token;

pub use error::{ParseError, Result};
pub use items::{ItemData, ItemType, Prefab, Road};
pub use sector::{
    BaseSector, Bounds, DataEntry, DataSector, Item, Limits, Node, SUPPORTED_VERSION, SectorHeader,
    parse_base, parse_base_with_limits, parse_data, parse_data_with_limits,
};
pub use token::Token;
