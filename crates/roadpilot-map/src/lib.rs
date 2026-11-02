//! RoadPilot-Map facade library.
//!
//! This crate is the public entry point of RoadPilot-Map. It will turn the map
//! data of a locally installed American Truck Simulator or Euro Truck Simulator 2
//! game into versioned, lane-level map packages.
//!
//! At this stage it only exposes version information. Map processing is added
//! in later phases, as described in `docs/architecture/`.
//!
//! The library never writes to the terminal. Presentation is the job of the
//! caller, such as the `roadpilot-map` command-line tool.

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
