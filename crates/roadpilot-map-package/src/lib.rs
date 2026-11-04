//! The versioned map package of RoadPilot-Map.
//!
//! A package is a folder with a `manifest.json` and data files. This crate
//! defines the format (the record types and [`Manifest`]), writes the geometry file
//! ([`write_geometry`]) and reads it back with full validation
//! ([`read_geometry`], [`MapPackage::open`]). It also provides a grid index
//! for finding the roads near a position ([`SpatialIndex`],
//! [`MapPackage::roads_near`]).
//!
//! The crate depends only on the model crate and does no game-data
//! extraction: a consumer can read packages without the code that creates
//! them. The format is specified in `docs/formats/map-package.md`.
//!
//! Geometric values carry an explicit [`Quality`]. Node positions are
//! confirmed, road curves are estimated, lane positions are unavailable. A
//! consumer must not use estimated or unavailable values as a basis for
//! active steering.
//!
//! Packages are created from a user's local game installation and are
//! derived from game data. They are not meant to be shared or redistributed.

mod checksum;
pub mod json;
mod manifest;
mod package;
mod read;
mod records;
mod spatial;
mod write;

pub use checksum::{Fnv64, HashingWriter, checksum, checksum_reader};
pub use manifest::{
    Counts, DEFAULT_SPATIAL_CELL_SIZE, FileEntry, MAX_MANIFEST_BYTES, Manifest, ManifestError,
    is_safe_file_name,
};
pub use package::{MapPackage, PackageError, RoadHit, read_manifest, verify_files};
pub use read::{GeometryData, MAX_CURVE_EXTENT, ReadError, read_geometry};
pub use records::{
    ArmLinkRecord, ArmRecord, ConnectionRecord, ConnectionStatus, CurveRecord, JunctionRecord,
    LaneDirectionRecord, LaneRecord, LaneRef, LanesRecord, LinkRecord, MovementRecord, NodeRecord,
    RoadRecord, Side, TrafficSideRecord, lane_id,
};
pub use roadpilot_map_model::Quality;
pub use spatial::{Candidate, MAX_CELLS_PER_BOX, SpatialError, SpatialIndex};
pub use write::{GeometryCounts, WriteError, WrittenFile, write_geometry};

/// The value of the `format` field of `manifest.json`.
pub const FORMAT_NAME: &str = "roadpilot-map-package";

/// The value of the `format` field of the geometry file.
pub const GEOMETRY_FORMAT_NAME: &str = "roadpilot-map-geometry";

/// The schema major version this version writes and reads. A reader rejects
/// other major versions.
pub const SCHEMA_MAJOR: u32 = 1;

/// The schema minor version this version writes. A reader accepts any minor
/// version of its major version.
pub const SCHEMA_MINOR: u32 = 0;

/// Positions are integers in 1/256 world units.
pub const POSITION_SCALE: u32 = 256;

/// Name of the manifest file in a package folder.
pub const MANIFEST_FILE: &str = "manifest.json";

/// Name of the geometry file written by RoadPilot-Map.
pub const GEOMETRY_FILE: &str = "geometry.jsonl";

/// Name of the navigation graph file written by RoadPilot-Map.
pub const NAVIGATION_FILE: &str = "navigation.jsonl";
