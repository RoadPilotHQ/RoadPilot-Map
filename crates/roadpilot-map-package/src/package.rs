//! Opening and querying a map package folder.

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::checksum::checksum_reader;
use crate::manifest::{MAX_MANIFEST_BYTES, Manifest, ManifestError};
use crate::read::{GeometryData, ReadError, read_geometry};
use crate::records::{CurveRecord, LanesRecord, NodeRecord, RoadRecord};
use crate::spatial::{SpatialError, SpatialIndex};
use crate::{GEOMETRY_FORMAT_NAME, MANIFEST_FILE};

/// Why a package could not be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackageError {
    /// A file could not be read.
    Io(String),
    /// The manifest was rejected.
    Manifest(ManifestError),
    /// The geometry file was rejected.
    Geometry(ReadError),
    /// A file differs from its manifest entry (size, checksum, format or
    /// version), or is missing.
    FileMismatch { name: String, reason: String },
    /// The data disagrees with the manifest or with itself.
    Inconsistent(String),
    /// The spatial index could not be built.
    Spatial(SpatialError),
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(m) => write!(f, "cannot read the package: {m}"),
            Self::Manifest(e) => e.fmt(f),
            Self::Geometry(e) => e.fmt(f),
            Self::FileMismatch { name, reason } => write!(f, "{name}: {reason}"),
            Self::Inconsistent(m) => write!(f, "inconsistent package: {m}"),
            Self::Spatial(e) => write!(f, "spatial index: {e}"),
        }
    }
}

impl std::error::Error for PackageError {}

/// Reads and checks `manifest.json` of a package folder.
///
/// # Errors
///
/// [`PackageError::Io`] or [`PackageError::Manifest`].
pub fn read_manifest(dir: &Path) -> Result<Manifest, PackageError> {
    let path = dir.join(MANIFEST_FILE);
    let meta = std::fs::metadata(&path)
        .map_err(|e| PackageError::Io(format!("{} ({:?})", path.display(), e.kind())))?;
    if meta.len() > MAX_MANIFEST_BYTES as u64 {
        return Err(PackageError::Manifest(ManifestError::Invalid(
            "the manifest is too large".to_owned(),
        )));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| PackageError::Io(format!("{} ({:?})", path.display(), e.kind())))?;
    Manifest::parse(&text).map_err(PackageError::Manifest)
}

/// Checks that every file of the manifest exists and has the recorded size
/// and checksum. Reads all files completely.
///
/// # Errors
///
/// [`PackageError::FileMismatch`] or [`PackageError::Io`].
pub fn verify_files(dir: &Path, manifest: &Manifest) -> Result<(), PackageError> {
    for entry in &manifest.files {
        let mismatch = |reason: String| PackageError::FileMismatch {
            name: entry.name.clone(),
            reason,
        };
        let path = dir.join(&entry.name);
        let file = File::open(&path)
            .map_err(|e| mismatch(format!("cannot be opened ({:?})", e.kind())))?;
        let (sum, bytes) = checksum_reader(BufReader::new(file))
            .map_err(|e| PackageError::Io(format!("{} ({:?})", path.display(), e.kind())))?;
        if bytes != entry.bytes {
            return Err(mismatch(format!(
                "has {bytes} bytes, the manifest says {}",
                entry.bytes
            )));
        }
        if sum != entry.checksum {
            return Err(mismatch(
                "the checksum differs from the manifest".to_owned(),
            ));
        }
    }
    Ok(())
}

/// A road found by a spatial query.
#[derive(Debug, Clone, Copy)]
pub struct RoadHit<'a> {
    pub road: &'a RoadRecord,
    /// Horizontal distance from the query point to the road's box. A lower
    /// bound of the distance to the estimated curve; the real road may be
    /// elsewhere (the curve is an estimate).
    pub distance: f64,
}

/// An opened and validated map package.
#[derive(Debug)]
pub struct MapPackage {
    manifest: Manifest,
    data: GeometryData,
    index: SpatialIndex,
    nodes_by_id: HashMap<u64, usize>,
    roads_by_id: HashMap<u64, usize>,
}

impl MapPackage {
    /// Opens a package folder: reads the manifest, verifies all files
    /// against it, reads and validates the geometry file, compares the
    /// counts with the manifest and builds the spatial index.
    ///
    /// # Errors
    ///
    /// A [`PackageError`] for the first problem found.
    pub fn open(dir: &Path) -> Result<Self, PackageError> {
        let manifest = read_manifest(dir)?;
        verify_files(dir, &manifest)?;
        let entry = manifest.file("geometry").ok_or_else(|| {
            PackageError::Inconsistent("the manifest lists no geometry file".to_owned())
        })?;
        if entry.format != GEOMETRY_FORMAT_NAME {
            return Err(PackageError::FileMismatch {
                name: entry.name.clone(),
                reason: format!("has format {:?}", entry.format),
            });
        }
        let path = dir.join(&entry.name);
        let file = File::open(&path)
            .map_err(|e| PackageError::Io(format!("{} ({:?})", path.display(), e.kind())))?;
        let data = read_geometry(BufReader::new(file), Some(manifest.length_tolerance))
            .map_err(PackageError::Geometry)?;
        Self::from_parts(manifest, data)
    }

    /// Builds a package from a manifest and geometry data already in memory.
    /// Checks the counts and builds the spatial index.
    ///
    /// # Errors
    ///
    /// [`PackageError::Inconsistent`] or [`PackageError::Spatial`].
    pub fn from_parts(manifest: Manifest, data: GeometryData) -> Result<Self, PackageError> {
        let lanes: u64 = data
            .roads
            .iter()
            .map(|r| match &r.lanes {
                LanesRecord::Resolved(l) => l.len() as u64,
                LanesRecord::Unresolved(_) => 0,
            })
            .sum();
        let counted = [
            ("nodes", manifest.counts.nodes, data.nodes.len() as u64),
            ("roads", manifest.counts.roads, data.roads.len() as u64),
            ("lanes", manifest.counts.lanes, lanes),
            (
                "junctions",
                manifest.counts.junctions,
                data.junctions.len() as u64,
            ),
            (
                "connections",
                manifest.counts.connections,
                data.connections.len() as u64,
            ),
        ];
        for (name, declared, found) in counted {
            if declared != found {
                return Err(PackageError::Inconsistent(format!(
                    "{name}: the manifest says {declared}, the geometry file has {found}"
                )));
            }
        }
        let nodes_by_id: HashMap<u64, usize> = data
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
        let roads_by_id: HashMap<u64, usize> = data
            .roads
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id, i))
            .collect();
        let boxes = data
            .roads
            .iter()
            .map(|r| road_box(r, &data.nodes, &nodes_by_id))
            .collect::<Result<Vec<_>, _>>()?;
        let index = SpatialIndex::build(boxes, manifest.spatial_cell_size)
            .map_err(PackageError::Spatial)?;
        Ok(Self {
            manifest,
            data,
            index,
            nodes_by_id,
            roads_by_id,
        })
    }

    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    #[must_use]
    pub fn data(&self) -> &GeometryData {
        &self.data
    }

    /// The road with this UID.
    #[must_use]
    pub fn road(&self, id: u64) -> Option<&RoadRecord> {
        self.roads_by_id.get(&id).map(|&i| &self.data.roads[i])
    }

    /// The node with this UID.
    #[must_use]
    pub fn node(&self, id: u64) -> Option<&NodeRecord> {
        self.nodes_by_id.get(&id).map(|&i| &self.data.nodes[i])
    }

    /// The roads whose box lies within `radius` world units of the
    /// horizontal position `(x, z)`, nearest first. The boxes are those of
    /// the estimated curves (or of the end nodes where no curve exists), so
    /// the result is a candidate list for map matching.
    #[must_use]
    pub fn roads_near(&self, x: f64, z: f64, radius: f64) -> Vec<RoadHit<'_>> {
        self.index
            .query_near(x, z, radius)
            .into_iter()
            .map(|c| RoadHit {
                road: &self.data.roads[c.index],
                distance: c.distance,
            })
            .collect()
    }

    /// The roads whose box intersects the horizontal area, in id order.
    #[must_use]
    pub fn roads_in_bounds(&self, min: [f64; 2], max: [f64; 2]) -> Vec<&RoadRecord> {
        self.index
            .query_bounds(min, max)
            .into_iter()
            .map(|i| &self.data.roads[i])
            .collect()
    }
}

/// The horizontal box of a road: the bounds of its estimated curve, or the
/// box of its two nodes if it has no curve.
fn road_box(
    road: &RoadRecord,
    nodes: &[NodeRecord],
    by_id: &HashMap<u64, usize>,
) -> Result<[f64; 4], PackageError> {
    if let CurveRecord::Estimated { bounds, .. } = road.curve {
        return Ok([bounds[0], bounds[2], bounds[3], bounds[5]]);
    }
    let world = |id: u64| -> Result<[f64; 2], PackageError> {
        let n = by_id
            .get(&id)
            .and_then(|&i| nodes.get(i))
            .ok_or_else(|| PackageError::Inconsistent(format!("node {id:x} is missing")))?;
        Ok([
            f64::from(n.position[0]) / 256.0,
            f64::from(n.position[2]) / 256.0,
        ])
    };
    let (a, b) = (world(road.start)?, world(road.end)?);
    Ok([
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[0].max(b[0]),
        a[1].max(b[1]),
    ])
}
