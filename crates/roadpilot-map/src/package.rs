//! Creating and checking map packages.
//!
//! A package is a folder with `manifest.json`, `geometry.jsonl` and
//! (optionally) `navigation.jsonl`; see `docs/formats/map-package.md`. It is
//! created from the models of a local installation and is never meant to be
//! shared. This module converts the models into package records
//! (using `roadpilot-map-geometry` for the road curves), writes the files in
//! a temporary folder next to the target, and moves the folder into place
//! only when everything was written.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use roadpilot_map_geometry::{LENGTH_TOLERANCE, RoadCurve, TANGENT_MODEL};
use roadpilot_map_model::{
    LaneConfig, LaneDirection, LaneIssue, LaneNetwork, LaneTransition, LaneType, NodeLink,
    PrefabId, PrefabNetwork, PrefabResolution, RoadNetwork, RoadSide, TrafficSide,
    TrafficSideIssue, TrafficSideState, TransitionIssue,
};
use roadpilot_map_package as pkg;
use roadpilot_map_package::{
    ArmLinkRecord, ArmRecord, ConnectionRecord, ConnectionStatus, Counts, CurveRecord, FileEntry,
    GeometryCounts, HashingWriter, JunctionRecord, LaneDirectionRecord, LaneRecord, LanesRecord,
    LinkRecord, Manifest, MapPackage, MovementRecord, NodeRecord, PackageError, RoadRecord, Side,
    TrafficSideRecord,
};

use crate::export::{
    self, CheckError, ExportInput, ExportMetadata, ExportRecord, KNOWN_LIMITATIONS,
};
use crate::installation::InstallationModel;

/// Revision of the generation logic. Increase it when the same game data
/// would give different package content, for example after a change of the
/// curve model, so that consumers can discard cached packages.
pub const CONTENT_REVISION: u32 = 1;

/// Known limitations of the package data. See the specification.
pub const PACKAGE_LIMITATIONS: [&str; 9] = [
    "lane_positions_are_unavailable",
    "lane_widths_are_not_stored",
    "lane_order_is_unknown",
    "road_curve_is_estimated",
    "world_unit_is_unconfirmed",
    "road_connections_are_candidates",
    "prefab_to_prefab_lane_pairing_is_unknown",
    "roads_without_known_traffic_side_have_no_lane_directions",
    "identifiers_are_not_stable_across_game_versions",
];

/// Options for [`write_package`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageOptions {
    /// Include the navigation graph file (large). Default `true`.
    pub include_navigation: bool,
    /// Replace an existing package at the output location.
    pub overwrite: bool,
}

impl Default for PackageOptions {
    fn default() -> Self {
        Self {
            include_navigation: true,
            overwrite: false,
        }
    }
}

/// Counts of a written package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageStats {
    pub counts: Counts,
    /// Roads whose curve could be estimated.
    pub roads_with_curve: u64,
    /// Roads whose estimated curve length agrees with the stored length
    /// within the tolerance.
    pub roads_length_consistent: u64,
    pub geometry_bytes: u64,
    pub navigation_bytes: Option<u64>,
    pub content_id: u64,
}

/// Why a package could not be written.
#[derive(Debug)]
#[non_exhaustive]
pub enum PackageWriteError {
    /// Writing failed.
    Io(io::Error),
    /// The models disagree with each other or a value cannot be written.
    Inconsistent(String),
    /// The output exists and overwriting was not requested.
    OutputExists(PathBuf),
    /// The output location cannot be used.
    InvalidOutput(String),
}

impl fmt::Display for PackageWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot write the package: {e}"),
            Self::Inconsistent(m) => write!(f, "inconsistent models: {m}"),
            Self::OutputExists(p) => {
                write!(
                    f,
                    "{} already exists; use --force to replace it",
                    p.display()
                )
            }
            Self::InvalidOutput(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for PackageWriteError {}

impl From<io::Error> for PackageWriteError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<pkg::WriteError> for PackageWriteError {
    fn from(e: pkg::WriteError) -> Self {
        match e {
            pkg::WriteError::Io(e) => Self::Io(e),
            other => Self::Inconsistent(other.to_string()),
        }
    }
}

impl From<export::ExportError> for PackageWriteError {
    fn from(e: export::ExportError) -> Self {
        match e {
            export::ExportError::Io(e) => Self::Io(e),
            other => Self::Inconsistent(other.to_string()),
        }
    }
}

// --- Model to records -----------------------------------------------------

fn side_record(s: RoadSide) -> Side {
    match s {
        RoadSide::Left => Side::Left,
        RoadSide::Right => Side::Right,
    }
}

fn link_record(l: NodeLink) -> LinkRecord {
    match l {
        NodeLink::None => LinkRecord::None,
        NodeLink::Road(r) => LinkRecord::Road(r.0),
        NodeLink::Item(u) => LinkRecord::Item(u),
        NodeLink::Unresolved(u) => LinkRecord::Unresolved(u),
    }
}

fn traffic_side_record(state: &TrafficSideState) -> TrafficSideRecord {
    match state {
        TrafficSideState::Known(TrafficSide::Right) => TrafficSideRecord::Right,
        TrafficSideState::Known(TrafficSide::Left) => TrafficSideRecord::Left,
        TrafficSideState::Unknown(issue) => TrafficSideRecord::Unknown(
            match issue {
                TrafficSideIssue::NoCountry => "no_country",
                TrafficSideIssue::CountryMismatch => "country_mismatch",
                TrafficSideIssue::UnknownCountry(_) => "unknown_country",
                TrafficSideIssue::LookConflict => "look_conflict",
            }
            .to_owned(),
        ),
    }
}

fn lane_issue_code(issue: &LaneIssue) -> &'static str {
    match issue {
        LaneIssue::MissingRoadLook(_) => "missing_road_look",
        LaneIssue::MissingVariant(_) => "missing_variant",
        LaneIssue::MissingLaneType(_) => "missing_lane_type",
        LaneIssue::VariantChangesLaneCount => "variant_changes_lane_count",
    }
}

fn lanes_record(lanes: Option<&roadpilot_map_model::RoadLanes>) -> LanesRecord {
    let Some(lanes) = lanes else {
        return LanesRecord::Unresolved("no_lane_data".to_owned());
    };
    match &lanes.config {
        LaneConfig::Unresolved(issue) => LanesRecord::Unresolved(lane_issue_code(issue).to_owned()),
        LaneConfig::Resolved(list) => {
            let mut records: Vec<LaneRecord> = list
                .iter()
                .map(|l| {
                    let (lane_type, type_certain) = match &l.lane_type {
                        LaneType::Known(t) => (t.clone(), true),
                        LaneType::Uncertain(t) => (t.clone(), false),
                    };
                    LaneRecord {
                        side: side_record(l.id.side),
                        index: l.id.index,
                        lane_type,
                        type_certain,
                        direction: lanes.direction(l.id.side).map(|d| match d {
                            LaneDirection::Forward => LaneDirectionRecord::Forward,
                            LaneDirection::Backward => LaneDirectionRecord::Backward,
                        }),
                        offset: l.offset.map(|o| [f64::from(o.first), f64::from(o.second)]),
                        // No lane width is stored, so no lane position exists.
                        geometry: roadpilot_map_model::Quality::Unavailable,
                    }
                })
                .collect();
            records.sort_by_key(|l| (l.side, l.index));
            LanesRecord::Resolved(records)
        }
    }
}

/// The curve record of a road and whether the estimate agrees with the
/// stored length.
fn curve_record(network: &RoadNetwork, road: roadpilot_map_model::RoadId) -> (CurveRecord, bool) {
    let Some(geometry) = network.geometry(road) else {
        return (CurveRecord::Unavailable("no_geometry".to_owned()), false);
    };
    match RoadCurve::estimate(&geometry) {
        Ok(curve) => {
            let agreement = curve.length_agreement();
            let b = curve.bounds();
            (
                CurveRecord::Estimated {
                    tangent_scale: curve.stored_length(),
                    estimated_length: curve.estimated_length(),
                    length_deviation: agreement.relative_deviation(),
                    length_consistent: agreement.is_consistent(),
                    bounds: [b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2]],
                },
                agreement.is_consistent(),
            )
        }
        Err(issue) => (
            CurveRecord::Unavailable(
                match issue {
                    roadpilot_map_geometry::CurveIssue::NonFiniteInput => "non_finite_input",
                    roadpilot_map_geometry::CurveIssue::InvalidStoredLength => {
                        "invalid_stored_length"
                    }
                    roadpilot_map_geometry::CurveIssue::InvalidDirection => "invalid_direction",
                    _ => "unknown_issue",
                }
                .to_owned(),
            ),
            false,
        ),
    }
}

fn resolution_code(r: &PrefabResolution) -> Option<&'static str> {
    match r {
        PrefabResolution::Resolved => None,
        PrefabResolution::MissingModel => Some("missing_model"),
        PrefabResolution::MissingDescriptor(_) => Some("missing_descriptor"),
        PrefabResolution::InvalidDescriptor(_) => Some("invalid_descriptor"),
        PrefabResolution::NodeCountMismatch { .. } => Some("node_count_mismatch"),
        PrefabResolution::OriginOutOfRange(_) => Some("origin_out_of_range"),
        PrefabResolution::NodeGeometryMismatch => Some("node_geometry_mismatch"),
    }
}

fn junction_record(
    network: &RoadNetwork,
    prefabs: &PrefabNetwork,
    id: PrefabId,
) -> Option<JunctionRecord> {
    let inst = prefabs.instance(id)?;
    let arms = prefabs
        .arms(network, id)
        .into_iter()
        .map(|a| ArmRecord {
            index: u32::try_from(a.index).unwrap_or(u32::MAX),
            node: a.node.0,
            link: match a.link {
                roadpilot_map_model::ArmLink::Road {
                    road,
                    road_ends_here,
                } => ArmLinkRecord::Road {
                    road: road.0,
                    road_ends_here,
                },
                roadpilot_map_model::ArmLink::Prefab(p) => ArmLinkRecord::Prefab(p.0),
                roadpilot_map_model::ArmLink::Item(u) => ArmLinkRecord::Item(u),
                roadpilot_map_model::ArmLink::Unresolved(u) => ArmLinkRecord::Unresolved(u),
                roadpilot_map_model::ArmLink::None => ArmLinkRecord::None,
            },
        })
        .collect();
    let movements = prefabs
        .movements(id)
        .into_iter()
        .map(|m| MovementRecord {
            from_arm: u32::try_from(m.from_arm).unwrap_or(u32::MAX),
            from_slot: u32::try_from(m.from_slot).unwrap_or(u32::MAX),
            to_arm: u32::try_from(m.to_arm).unwrap_or(u32::MAX),
            to_slot: u32::try_from(m.to_slot).unwrap_or(u32::MAX),
            // The internal movement is stored in the prefab descriptor.
            status: ConnectionStatus::Confirmed,
        })
        .collect();
    Some(JunctionRecord {
        id: id.0,
        model: inst.model.clone(),
        unresolved: resolution_code(&inst.resolution).map(str::to_owned),
        nodes: inst.nodes.iter().map(|n| n.0).collect(),
        arms,
        movements,
    })
}

fn connection_record(
    lanes: &LaneNetwork,
    connection: &roadpilot_map_model::RoadConnection,
) -> ConnectionRecord {
    let (status, reason, pairs) = match lanes.transition(connection) {
        LaneTransition::IndexCandidates(list) if list.is_empty() => (
            ConnectionStatus::Unknown,
            Some("no_lanes".to_owned()),
            Vec::new(),
        ),
        LaneTransition::IndexCandidates(list) => (
            ConnectionStatus::Candidate,
            None,
            list.iter()
                .map(|c| {
                    (
                        (side_record(c.from.side), c.from.index),
                        (side_record(c.to.side), c.to.index),
                    )
                })
                .collect(),
        ),
        LaneTransition::Unknown(issue) => (
            ConnectionStatus::Unknown,
            Some(
                match issue {
                    TransitionIssue::LanesUnresolved => "lanes_unresolved",
                    TransitionIssue::TrafficSideUnknown => "traffic_side_unknown",
                    TransitionIssue::TrafficSideDiffers => "traffic_side_differs",
                    TransitionIssue::LaneCountDiffers(LaneDirection::Forward) => {
                        "lane_count_differs_forward"
                    }
                    TransitionIssue::LaneCountDiffers(LaneDirection::Backward) => {
                        "lane_count_differs_backward"
                    }
                }
                .to_owned(),
            ),
            Vec::new(),
        ),
    };
    ConnectionRecord {
        from: connection.from.0,
        via: connection.via.0,
        to: connection.to.0,
        status,
        reason,
        pairs,
    }
}

// --- Writing ---------------------------------------------------------------

fn partial_path(out: &Path) -> PathBuf {
    let mut name = out
        .file_name()
        .map(std::ffi::OsString::from)
        .unwrap_or_default();
    name.push(".partial");
    out.with_file_name(name)
}

/// File names a package folder may contain. Replacing an existing folder is
/// refused if it holds anything else.
const PACKAGE_FILES: [&str; 3] = [pkg::MANIFEST_FILE, pkg::GEOMETRY_FILE, pkg::NAVIGATION_FILE];

/// Checks that a package can be written to `out`, without writing anything.
///
/// The parent folder must exist, `out` may not be inside `forbidden` (pass the
/// game folder), and an existing `out` is an error unless `overwrite` is set
/// and it is a package folder (it holds only package files).
///
/// # Errors
///
/// [`PackageWriteError::InvalidOutput`] or [`PackageWriteError::OutputExists`].
pub fn check_package_output(
    out: &Path,
    overwrite: bool,
    forbidden: Option<&Path>,
) -> Result<(), PackageWriteError> {
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_real = parent.canonicalize().map_err(|e| {
        PackageWriteError::InvalidOutput(format!(
            "the output folder {} cannot be used ({:?})",
            parent.display(),
            e.kind()
        ))
    })?;
    if out.file_name().is_none() {
        return Err(PackageWriteError::InvalidOutput(format!(
            "{} is not a folder path",
            out.display()
        )));
    }
    if let Some(game_dir) = forbidden {
        if let Ok(real) = game_dir.canonicalize() {
            if parent_real.starts_with(&real) {
                return Err(PackageWriteError::InvalidOutput(format!(
                    "the output {} is inside the game folder; choose another location",
                    out.display()
                )));
            }
        }
    }
    if out.exists() {
        if !overwrite {
            return Err(PackageWriteError::OutputExists(out.to_path_buf()));
        }
        if !out.is_dir() {
            return Err(PackageWriteError::InvalidOutput(format!(
                "{} is not a folder",
                out.display()
            )));
        }
        for entry in fs::read_dir(out)? {
            let name = entry?.file_name();
            if !PACKAGE_FILES.iter().any(|f| name == *f) {
                return Err(PackageWriteError::InvalidOutput(format!(
                    "{} contains {}, which is not a package file; it will not be replaced",
                    out.display(),
                    name.to_string_lossy()
                )));
            }
        }
    }
    let partial = partial_path(out);
    if partial.exists() {
        return Err(PackageWriteError::InvalidOutput(format!(
            "{} exists from an earlier run; remove it first",
            partial.display()
        )));
    }
    Ok(())
}

/// The limitation codes of a package: its own, plus those of the navigation
/// file if it is included (without `curve_geometry_is_not_exported`, which the
/// package's `curve` object replaces). Each code appears once.
fn limitations(include_navigation: bool) -> Vec<String> {
    let mut codes: Vec<String> = PACKAGE_LIMITATIONS
        .iter()
        .map(|l| (*l).to_owned())
        .collect();
    if include_navigation {
        for l in KNOWN_LIMITATIONS {
            if l != "curve_geometry_is_not_exported" && !codes.iter().any(|c| c == l) {
                codes.push(l.to_owned());
            }
        }
    }
    codes
}

fn write_files(
    dir: &Path,
    model: &InstallationModel,
    meta: &ExportMetadata,
    options: PackageOptions,
) -> Result<PackageStats, PackageWriteError> {
    let network = &model.network;
    let lanes = &model.lanes;
    let prefabs = &model.prefabs;

    let nodes = network.nodes().map(|n| NodeRecord {
        id: n.id.0,
        position: n.position.fixed,
        rotation: [n.rotation.w, n.rotation.x, n.rotation.y, n.rotation.z],
        backward: link_record(n.backward),
        forward: link_record(n.forward),
    });

    // Statistics are gathered while the roads stream by.
    let mut with_curve = 0u64;
    let mut consistent = 0u64;
    let roads = network.roads().map(|r| {
        let (curve, ok) = curve_record(network, r.id);
        if matches!(curve, CurveRecord::Estimated { .. }) {
            with_curve += 1;
        }
        if ok {
            consistent += 1;
        }
        let road_lanes = lanes.road(r.id);
        RoadRecord {
            id: r.id.0,
            start: r.start_node.0,
            end: r.end_node.0,
            look: road_lanes.map_or_else(String::new, |l| l.look.clone()),
            stored_length: r.stored_length,
            traffic_side: road_lanes.map_or_else(
                || TrafficSideRecord::Unknown("no_lane_data".to_owned()),
                |l| traffic_side_record(&l.traffic_side),
            ),
            lanes: lanes_record(road_lanes),
            curve,
        }
    });

    let junctions = prefabs
        .instances()
        .filter_map(|i| junction_record(network, prefabs, i.id));
    let connections = network.connections().map(|c| connection_record(lanes, &c));

    let counts = GeometryCounts {
        nodes: network.nodes().count() as u64,
        roads: network.roads().count() as u64,
        junctions: prefabs.instances().count() as u64,
        connections: network.connections().count() as u64,
    };

    let geometry_path = dir.join(pkg::GEOMETRY_FILE);
    let geometry_file = File::create_new(&geometry_path)?;
    let written = pkg::write_geometry(geometry_file, counts, nodes, roads, junctions, connections)?;

    let mut files = vec![FileEntry {
        name: pkg::GEOMETRY_FILE.to_owned(),
        role: "geometry".to_owned(),
        format: pkg::GEOMETRY_FORMAT_NAME.to_owned(),
        schema_major: pkg::SCHEMA_MAJOR,
        schema_minor: pkg::SCHEMA_MINOR,
        bytes: written.bytes,
        checksum: written.checksum,
    }];

    let mut navigation_bytes = None;
    if options.include_navigation {
        let file = File::create_new(dir.join(pkg::NAVIGATION_FILE))?;
        let mut w = HashingWriter::new(file);
        export::write_navigation_export(&mut w, &ExportInput::from_installation(model), meta)?;
        navigation_bytes = Some(w.bytes());
        files.push(FileEntry {
            name: pkg::NAVIGATION_FILE.to_owned(),
            role: "navigation".to_owned(),
            format: export::FORMAT_NAME.to_owned(),
            schema_major: export::SCHEMA_MAJOR,
            schema_minor: export::SCHEMA_MINOR,
            bytes: w.bytes(),
            checksum: w.checksum(),
        });
    }

    let manifest = Manifest {
        schema_major: pkg::SCHEMA_MAJOR,
        schema_minor: pkg::SCHEMA_MINOR,
        content_revision: CONTENT_REVISION,
        generator_name: "roadpilot-map".to_owned(),
        generator_version: crate::version().to_owned(),
        game: meta.game.id().to_owned(),
        map: meta.map.clone(),
        game_version: meta.game_version.clone(),
        archives: meta.archives.clone(),
        counts: Counts {
            nodes: written.counts.nodes,
            roads: written.counts.roads,
            lanes: written.lanes,
            junctions: written.counts.junctions,
            connections: written.counts.connections,
        },
        curve_model: TANGENT_MODEL.to_owned(),
        length_tolerance: LENGTH_TOLERANCE,
        spatial_cell_size: pkg::DEFAULT_SPATIAL_CELL_SIZE,
        limitations: limitations(options.include_navigation),
        files,
    };
    let content_id = manifest.content_id();
    let text = manifest
        .to_json()
        .map_err(PackageWriteError::Inconsistent)?;
    fs::write(dir.join(pkg::MANIFEST_FILE), text)?;

    Ok(PackageStats {
        counts: manifest.counts,
        roads_with_curve: with_curve,
        roads_length_consistent: consistent,
        geometry_bytes: written.bytes,
        navigation_bytes,
        content_id,
    })
}

/// Writes a map package folder for a loaded installation.
///
/// The files are written to `<out>.partial` and the folder is moved to `out`
/// only after everything was written, so a folder at `out` is always a
/// complete package. After an error the partial folder is removed and nothing
/// that was not there before remains. An existing package at `out` is
/// replaced only with `options.overwrite`.
///
/// The output is deterministic: the same models and metadata give the same
/// files.
///
/// # Errors
///
/// [`PackageWriteError`].
pub fn write_package(
    out: &Path,
    forbidden: Option<&Path>,
    model: &InstallationModel,
    meta: &ExportMetadata,
    options: PackageOptions,
) -> Result<PackageStats, PackageWriteError> {
    check_package_output(out, options.overwrite, forbidden)?;
    let partial = partial_path(out);
    fs::create_dir(&partial)?;
    let result = write_files(&partial, model, meta, options).and_then(|stats| {
        if out.exists() {
            // `check_package_output` verified that it holds package files only.
            for f in PACKAGE_FILES {
                let p = out.join(f);
                if p.exists() {
                    fs::remove_file(p)?;
                }
            }
            fs::remove_dir(out)?;
        }
        fs::rename(&partial, out)?;
        Ok(stats)
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&partial);
    }
    result
}

// --- Checking --------------------------------------------------------------

/// What a checked package contains.
#[derive(Debug)]
pub struct PackageSummary {
    pub manifest: Manifest,
    pub roads_with_estimated_curve: u64,
    pub roads_length_consistent: u64,
    pub navigation: Option<export::ExportSummary>,
}

/// Why a package was rejected.
#[derive(Debug)]
#[non_exhaustive]
pub enum PackageCheckError {
    /// The package itself (manifest, files, geometry) is invalid.
    Package(PackageError),
    /// The navigation file is invalid.
    Navigation(CheckError),
    /// The navigation file and the geometry file disagree.
    Mismatch(String),
}

impl fmt::Display for PackageCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package(e) => e.fmt(f),
            Self::Navigation(e) => write!(f, "navigation file: {e}"),
            Self::Mismatch(m) => write!(f, "the files of the package disagree: {m}"),
        }
    }
}

impl std::error::Error for PackageCheckError {}

/// Opens a package, validates it completely and cross-checks the navigation
/// file against the geometry file.
///
/// Verifies the checksums of all files, validates the manifest and the
/// geometry file, runs the reference reader over the navigation file, and
/// checks that every road and map node the navigation file names exists in
/// the geometry file with the same position.
///
/// # Errors
///
/// [`PackageCheckError`].
pub fn check_package(dir: &Path) -> Result<PackageSummary, PackageCheckError> {
    let package = MapPackage::open(dir).map_err(PackageCheckError::Package)?;
    let manifest = package.manifest().clone();
    let (mut with_curve, mut consistent) = (0u64, 0u64);
    for r in &package.data().roads {
        if let CurveRecord::Estimated {
            length_consistent, ..
        } = r.curve
        {
            with_curve += 1;
            consistent += u64::from(length_consistent);
        }
    }

    let mut navigation = None;
    if let Some(entry) = manifest.file("navigation") {
        if entry.format != export::FORMAT_NAME {
            return Err(PackageCheckError::Package(PackageError::FileMismatch {
                name: entry.name.clone(),
                reason: format!("has format {:?}", entry.format),
            }));
        }
        let file = File::open(dir.join(&entry.name))
            .map_err(|e| PackageCheckError::Package(PackageError::Io(e.to_string())))?;
        let mut mismatch: Option<String> = None;
        let summary = export::read_navigation_export(BufReader::new(file), &mut |record| {
            if mismatch.is_some() {
                return;
            }
            if let ExportRecord::Node(n) = record {
                if let Some(road) = n.road {
                    if package.road(road).is_none() {
                        mismatch = Some(format!(
                            "navigation node {} names road {road:x}, which the geometry file does not have",
                            n.id
                        ));
                        return;
                    }
                }
                match (package.node(n.map_node), n.position) {
                    (None, _) => {
                        mismatch = Some(format!(
                            "navigation node {} names map node {:x}, which the geometry file does not have",
                            n.id, n.map_node
                        ));
                    }
                    (Some(node), Some(p)) if node.position.map(i64::from) != p => {
                        mismatch = Some(format!(
                            "navigation node {} and the geometry file give different positions for map node {:x}",
                            n.id, n.map_node
                        ));
                    }
                    _ => {}
                }
            }
        })
        .map_err(PackageCheckError::Navigation)?;
        if let Some(m) = mismatch {
            return Err(PackageCheckError::Mismatch(m));
        }
        navigation = Some(summary);
    }
    Ok(PackageSummary {
        manifest,
        roads_with_estimated_curve: with_curve,
        roads_length_consistent: consistent,
        navigation,
    })
}
