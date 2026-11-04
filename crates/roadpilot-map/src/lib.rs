//! RoadPilot-Map facade library.
//!
//! This crate is the public entry point of RoadPilot-Map. It will turn the map
//! data of a locally installed American Truck Simulator or Euro Truck Simulator 2
//! game into versioned, lane-level map packages.
//!
//! So far it builds the road model from parsed sector files
//! ([`build_road_network`]): road segments, nodes and the connections that
//! follow from their references; and the lanes of each road from the road
//! look, lane type and country definitions ([`build_lane_network`]).
//! Prefabs (junctions and other road pieces) with their stored internal lane
//! movements come from the prefab models and their `.ppd` descriptors
//! ([`build_prefab_network`]). Lane geometry and the map package are added
//! in later steps,
//! as described in `docs/architecture/`.
//!
//! The parser and model crates are re-exported as [`formats`] and [`model`].
//!
//! The library never writes to the terminal. Presentation is the job of the
//! caller, such as the `roadpilot-map` command-line tool.
//!
//! The [`dev`] module contains development diagnostics for working on
//! RoadPilot-Map itself. It is not part of map processing.

pub mod dev;
pub mod export;
mod installation;
mod lanes;
pub mod measurement_sheet;
mod package;
mod prefabs;
mod roads;
pub mod snapshot_import;
pub mod validation;

pub use installation::{
    Game, InstallationError, InstallationModel, load_installation, road_look_templates,
};
pub use lanes::{DefinitionsError, LaneBuildError, LaneDefinitions, build_lane_network};
pub use package::{
    CONTENT_REVISION, PACKAGE_LIMITATIONS, PackageCheckError, PackageOptions, PackageStats,
    PackageSummary, PackageWriteError, check_package, check_package_output, write_package,
};
pub use prefabs::{PrefabDefinitions, build_prefab_network};
pub use roadpilot_map_formats as formats;
pub use roadpilot_map_geometry as geometry;
pub use roadpilot_map_model as model;
pub use roadpilot_map_package as map_package;
pub use roads::build_road_network;

/// Returns the version of this library, for example `"0.1.0"`.
///
/// The value is taken from the package version at compile time.
///
/// # Examples
///
/// ```
/// let version = roadpilot_map::version();
/// assert!(!version.is_empty());
/// ```
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn version_matches_package_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn version_has_three_numeric_parts() {
        let parts: Vec<&str> = version().split('.').collect();
        assert_eq!(parts.len(), 3, "unexpected version format: {}", version());
        for part in parts {
            assert!(
                part.parse::<u64>().is_ok(),
                "non-numeric version part {part:?} in {}",
                version()
            );
        }
    }
}
