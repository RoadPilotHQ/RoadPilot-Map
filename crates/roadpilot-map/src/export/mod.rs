//! Export of the navigation graph in the `roadpilot-map-navigation-graph`
//! format, schema version 1.
//!
//! The export is a JSON Lines text file that independent programs can read
//! without any RoadPilot-Map crate. It is written as a stream; the graph is
//! never copied. The format is specified in
//! `docs/navigation/navigation-export.md`. This module also contains a
//! reference reader ([`read_navigation_export`]) that checks a file against
//! the specification.

mod json;
mod reader;

use std::fmt::{self, Write as _};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use roadpilot_map_model::{
    ArmIssue, ConnectionStatus, EdgeId, EdgeOriginKind, GraphIssueKind, LaneDirection, LaneId,
    LaneNetwork, NavNode, NavigationGraph, NodeId, PrefabNetwork, RoadNetwork, RoadSide,
    SlotDirection, TransitionIssue,
};

use crate::installation::{Game, InstallationModel};

pub use reader::{
    CheckError, ExportEdge, ExportHeader, ExportIssue, ExportNode, ExportRecord, ExportSummary,
    check_navigation_export, read_navigation_export,
};

/// The value of the `format` field.
pub const FORMAT_NAME: &str = "roadpilot-map-navigation-graph";

/// The schema version this version of the library writes and reads.
pub const SCHEMA_MAJOR: u32 = 1;
/// Minor version of the schema. Readers of the same major version accept any
/// minor version.
pub const SCHEMA_MINOR: u32 = 0;

/// Position scale: positions are integers in 1/256 world units.
pub const POSITION_SCALE: u32 = 256;

/// Static limitations of the data, written to every export header so that
/// consumers cannot miss them. See the specification for their meaning.
pub const KNOWN_LIMITATIONS: [&str; 7] = [
    "roads_without_known_traffic_side_are_not_exported",
    "lane_traversal_direction_is_candidate",
    "lane_order_is_unknown",
    "prefab_to_prefab_lane_pairing_is_unknown",
    "positions_are_map_node_positions_not_lane_positions",
    "curve_geometry_is_not_exported",
    "graph_is_not_one_connected_network",
];

/// What is known about the source of the data. Unknown values are `None` and
/// are written as `null`; nothing is invented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportMetadata {
    pub game: Game,
    /// The map folder the sectors came from (`europe` or `usa`).
    pub map: Option<String>,
    /// The game version. The library does not read it, so it is normally
    /// `None`.
    pub game_version: Option<String>,
    /// File names of the archives that were read.
    pub archives: Option<Vec<String>>,
}

impl ExportMetadata {
    /// Metadata for an export of a loaded installation.
    #[must_use]
    pub fn for_installation(model: &InstallationModel) -> Self {
        Self {
            game: model.game,
            map: Some(model.game.map_name().to_owned()),
            game_version: None,
            archives: Some(model.archives.clone()),
        }
    }
}

/// The models an export is written from.
#[derive(Debug, Clone, Copy)]
pub struct ExportInput<'a> {
    pub graph: &'a NavigationGraph,
    pub network: &'a RoadNetwork,
    pub lanes: &'a LaneNetwork,
    pub prefabs: &'a PrefabNetwork,
}

impl<'a> ExportInput<'a> {
    /// The models of a loaded installation.
    #[must_use]
    pub fn from_installation(model: &'a InstallationModel) -> Self {
        Self {
            graph: &model.graph,
            network: &model.network,
            lanes: &model.lanes,
            prefabs: &model.prefabs,
        }
    }
}

/// Counts of a written export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportStats {
    pub nodes: usize,
    pub edges: usize,
    pub issues: usize,
    /// Number of bytes written.
    pub bytes: u64,
}

/// Why an export could not be written.
#[derive(Debug)]
#[non_exhaustive]
pub enum ExportError {
    /// Writing failed.
    Io(io::Error),
    /// The models do not agree with each other (for example a node of the
    /// graph that the road network does not know).
    Inconsistent(String),
    /// The output file already exists and overwriting was not requested.
    OutputExists(PathBuf),
    /// The output location is not allowed or cannot be used.
    InvalidOutput(String),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot write the export: {e}"),
            Self::Inconsistent(m) => write!(f, "inconsistent models: {m}"),
            Self::OutputExists(p) => write!(
                f,
                "{} already exists; use --force to replace it",
                p.display()
            ),
            Self::InvalidOutput(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<io::Error> for ExportError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Counts bytes while writing.
struct Counting<W> {
    inner: W,
    bytes: u64,
}

impl<W: Write> Write for Counting<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.bytes = self.bytes.saturating_add(n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn status_name(s: ConnectionStatus) -> &'static str {
    match s {
        ConnectionStatus::Confirmed => "confirmed",
        ConnectionStatus::Candidate => "candidate",
        ConnectionStatus::Unknown => "unknown",
        ConnectionStatus::Forbidden => "forbidden",
    }
}

fn category_name(k: EdgeOriginKind) -> &'static str {
    match k {
        EdgeOriginKind::RoadLane => "road_lane",
        EdgeOriginKind::RoadConnection => "road_connection",
        EdgeOriginKind::RoadToPrefabSlot => "road_to_prefab_slot",
        EdgeOriginKind::PrefabInternalCurve => "prefab_internal",
        EdgeOriginKind::PrefabSlotToRoad => "prefab_slot_to_road",
        EdgeOriginKind::PrefabToPrefab => "prefab_to_prefab",
        EdgeOriginKind::Other => "other",
    }
}

fn side_name(s: RoadSide) -> &'static str {
    match s {
        RoadSide::Left => "left",
        RoadSide::Right => "right",
    }
}

fn lane_direction_name(d: LaneDirection) -> &'static str {
    match d {
        LaneDirection::Forward => "forward",
        LaneDirection::Backward => "backward",
    }
}

fn slot_direction_name(d: SlotDirection) -> &'static str {
    match d {
        SlotDirection::Input => "input",
        SlotDirection::Output => "output",
    }
}

/// The map node a lane starts (entry) or ends (exit) at, from the road
/// record and the lane direction.
fn lane_map_node(input: &ExportInput<'_>, lane: LaneId, exit: bool) -> Option<NodeId> {
    let dir = input.lanes.road(lane.road)?.direction(lane.side)?;
    let road = input.network.road(lane.road)?;
    Some(match (exit, dir) {
        (true, LaneDirection::Forward) | (false, LaneDirection::Backward) => road.end_node,
        _ => road.start_node,
    })
}

fn push_node_line(
    out: &mut String,
    id: usize,
    node: NavNode,
    input: &ExportInput<'_>,
) -> Result<(), ExportError> {
    let (kind, map_node) = match node {
        NavNode::LaneEntry(l) => ("lane_entry", lane_map_node(input, l, false)),
        NavNode::LaneExit(l) => ("lane_exit", lane_map_node(input, l, true)),
        NavNode::PrefabInput(s) => (
            "prefab_input",
            input
                .prefabs
                .instance(s.prefab)
                .and_then(|i| i.nodes.get(usize::from(s.arm)).copied()),
        ),
        NavNode::PrefabOutput(s) => (
            "prefab_output",
            input
                .prefabs
                .instance(s.prefab)
                .and_then(|i| i.nodes.get(usize::from(s.arm)).copied()),
        ),
    };
    let map_node = map_node.ok_or_else(|| {
        ExportError::Inconsistent(format!("node {id} ({kind}) has no map node in the models"))
    })?;
    let position = input
        .network
        .node(map_node)
        .ok_or_else(|| {
            ExportError::Inconsistent(format!("map node {:x} of node {id} is unknown", map_node.0))
        })?
        .position
        .fixed;
    let _ = write!(out, "{{\"t\":\"node\",\"id\":{id},\"kind\":\"{kind}\",");
    match node {
        NavNode::LaneEntry(l) | NavNode::LaneExit(l) => {
            out.push_str("\"road\":");
            json::push_hex(out, l.road.0);
            let _ = write!(
                out,
                ",\"side\":\"{}\",\"index\":{}",
                side_name(l.side),
                l.index
            );
        }
        NavNode::PrefabInput(s) | NavNode::PrefabOutput(s) => {
            out.push_str("\"prefab\":");
            json::push_hex(out, s.prefab.0);
            let _ = write!(out, ",\"arm\":{},\"slot\":{}", s.arm, s.slot);
        }
    }
    out.push_str(",\"map_node\":");
    json::push_hex(out, map_node.0);
    let _ = writeln!(
        out,
        ",\"position\":[{},{},{}]}}",
        position[0], position[1], position[2]
    );
    Ok(())
}

fn transition_reason(out: &mut String, issue: TransitionIssue) {
    match issue {
        TransitionIssue::LanesUnresolved => out.push_str(",\"reason\":\"lanes_unresolved\""),
        TransitionIssue::TrafficSideUnknown => {
            out.push_str(",\"reason\":\"traffic_side_unknown\"");
        }
        TransitionIssue::TrafficSideDiffers => {
            out.push_str(",\"reason\":\"traffic_side_differs\"");
        }
        TransitionIssue::LaneCountDiffers(d) => {
            let _ = write!(
                out,
                ",\"reason\":\"lane_count_differs\",\"direction\":\"{}\"",
                lane_direction_name(d)
            );
        }
    }
}

fn push_issue_line(out: &mut String, kind: GraphIssueKind, source: u64) {
    out.push_str("{\"t\":\"issue\",\"kind\":");
    let name = match kind {
        GraphIssueKind::RoadLanesNotNavigable => "road_lanes_not_navigable",
        GraphIssueKind::RoadTransitionUnpaired(_) => "road_transition_unpaired",
        GraphIssueKind::PrefabUnresolved => "prefab_unresolved",
        GraphIssueKind::PrefabArmUnpaired { .. } => "prefab_arm_unpaired",
        GraphIssueKind::UnresolvedNodeLink => "unresolved_node_link",
        GraphIssueKind::NodeLinksToOtherItem => "node_links_to_other_item",
    };
    json::push_str(out, name);
    out.push_str(",\"source\":");
    json::push_hex(out, source);
    match kind {
        GraphIssueKind::RoadTransitionUnpaired(i) => transition_reason(out, i),
        GraphIssueKind::PrefabArmUnpaired { arm, issue } => {
            let _ = write!(out, ",\"arm\":{arm}");
            match issue {
                ArmIssue::NoRoad => out.push_str(",\"reason\":\"no_road\""),
                ArmIssue::PrefabUnresolved => out.push_str(",\"reason\":\"prefab_unresolved\""),
                ArmIssue::RoadLanesUnknown => out.push_str(",\"reason\":\"road_lanes_unknown\""),
                ArmIssue::LaneCountMismatch {
                    direction,
                    lanes,
                    slots,
                } => {
                    let _ = write!(
                        out,
                        ",\"reason\":\"lane_count_mismatch\",\"direction\":\"{}\",\"lanes\":{lanes},\"slots\":{slots}",
                        slot_direction_name(direction)
                    );
                }
            }
        }
        _ => {}
    }
    out.push_str("}\n");
}

fn push_header(out: &mut String, input: &ExportInput<'_>, meta: &ExportMetadata, issues: usize) {
    let _ = write!(
        out,
        "{{\"t\":\"header\",\"format\":\"{FORMAT_NAME}\",\"schema_version\":{{\"major\":{SCHEMA_MAJOR},\"minor\":{SCHEMA_MINOR}}},"
    );
    let _ = write!(
        out,
        "\"generator\":{{\"name\":\"roadpilot-map\",\"version\":\"{}\"}},\"generated_at\":null,",
        crate::version()
    );
    let _ = write!(out, "\"game\":\"{}\",\"source\":{{\"map\":", meta.game.id());
    match &meta.map {
        Some(m) => json::push_str(out, m),
        None => out.push_str("null"),
    }
    out.push_str(",\"game_version\":");
    match &meta.game_version {
        Some(v) => json::push_str(out, v),
        None => out.push_str("null"),
    }
    out.push_str(",\"archives\":");
    match &meta.archives {
        Some(list) => {
            out.push('[');
            for (i, a) in list.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                json::push_str(out, a);
            }
            out.push(']');
        }
        None => out.push_str("null"),
    }
    let _ = write!(
        out,
        "}},\"coordinate_system\":{{\"x\":\"east\",\"y\":\"up\",\"z\":\"south\",\"position_scale\":{POSITION_SCALE},\"world_unit\":\"unconfirmed\"}},"
    );
    let _ = write!(
        out,
        "\"counts\":{{\"nodes\":{},\"edges\":{},\"issues\":{issues}}},\"limitations\":[",
        input.graph.node_count(),
        input.graph.edge_count()
    );
    for (i, l) in KNOWN_LIMITATIONS.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json::push_str(out, l);
    }
    out.push_str("]}\n");
}

/// Writes the navigation graph as a stream of JSON lines.
///
/// The output is deterministic: it depends only on the models and the
/// metadata, and contains no time stamp. The writer is flushed before the
/// function returns. A file that ends without the footer line is incomplete.
///
/// # Errors
///
/// [`ExportError::Io`] if writing fails and [`ExportError::Inconsistent`] if
/// the models disagree. In both cases the output is incomplete.
pub fn write_navigation_export<W: Write>(
    writer: W,
    input: &ExportInput<'_>,
    meta: &ExportMetadata,
) -> Result<ExportStats, ExportError> {
    let mut w = Counting {
        inner: BufWriter::with_capacity(1 << 20, writer),
        bytes: 0,
    };
    let graph = input.graph;
    let issues = graph.issues();
    let mut line = String::with_capacity(512);

    push_header(&mut line, input, meta, issues.len());
    w.write_all(line.as_bytes())?;

    for (id, node) in graph.nodes().enumerate() {
        line.clear();
        push_node_line(&mut line, id, node, input)?;
        w.write_all(line.as_bytes())?;
    }

    for (id, edge) in graph.edges().enumerate() {
        line.clear();
        let o = edge.origin();
        let _ = write!(
            line,
            "{{\"t\":\"edge\",\"id\":{id},\"from\":{},\"to\":{},\"status\":\"{}\",\"category\":\"{}\",\"source\":",
            edge.from().index(),
            edge.to().index(),
            status_name(edge.status()),
            category_name(o.kind),
        );
        json::push_hex(&mut line, o.source);
        let id32 = u32::try_from(id)
            .map_err(|_| ExportError::Inconsistent("more than u32::MAX edges".to_owned()))?;
        let rules = graph.rules(EdgeId(id32));
        if !rules.is_empty() {
            line.push_str(",\"rules\":[");
            for (i, r) in rules.iter().enumerate() {
                if i > 0 {
                    line.push(',');
                }
                let (name, resolved) = match r {
                    roadpilot_map_model::RuleRef::Resolved(n) => (n, true),
                    roadpilot_map_model::RuleRef::Unresolved(n) => (n, false),
                };
                line.push_str("{\"name\":");
                json::push_str(&mut line, name);
                let _ = write!(line, ",\"resolved\":{resolved}}}");
            }
            line.push(']');
        }
        line.push_str("}\n");
        w.write_all(line.as_bytes())?;
    }

    for issue in issues {
        line.clear();
        push_issue_line(&mut line, issue.kind, issue.source);
        w.write_all(line.as_bytes())?;
    }

    line.clear();
    let _ = writeln!(
        line,
        "{{\"t\":\"footer\",\"complete\":true,\"nodes\":{},\"edges\":{},\"issues\":{}}}",
        graph.node_count(),
        graph.edge_count(),
        issues.len()
    );
    w.write_all(line.as_bytes())?;
    w.flush()?;
    Ok(ExportStats {
        nodes: graph.node_count(),
        edges: graph.edge_count(),
        issues: issues.len(),
        bytes: w.bytes,
    })
}

/// The temporary name an export is written to before it is moved into place.
fn partial_path(out: &Path) -> PathBuf {
    let mut name = out
        .file_name()
        .map(std::ffi::OsString::from)
        .unwrap_or_default();
    name.push(".partial");
    out.with_file_name(name)
}

/// Checks that an export can be written to `out`, without writing anything.
///
/// The output folder must exist, `out` must not be a folder, and it may not
/// be inside `forbidden` (pass the game folder to keep the installation
/// untouched). An existing `out` is an error unless `overwrite` is set.
///
/// # Errors
///
/// [`ExportError::InvalidOutput`] or [`ExportError::OutputExists`].
pub fn check_output_path(
    out: &Path,
    overwrite: bool,
    forbidden: Option<&Path>,
) -> Result<(), ExportError> {
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_real = parent.canonicalize().map_err(|e| {
        ExportError::InvalidOutput(format!(
            "the output folder {} cannot be used ({:?})",
            parent.display(),
            e.kind()
        ))
    })?;
    if out.file_name().is_none() {
        return Err(ExportError::InvalidOutput(format!(
            "{} is not a file path",
            out.display()
        )));
    }
    if let Some(game_dir) = forbidden {
        if let Ok(real) = game_dir.canonicalize() {
            if parent_real.starts_with(&real) {
                return Err(ExportError::InvalidOutput(format!(
                    "the output {} is inside the game folder; choose another location",
                    out.display()
                )));
            }
        }
    }
    if out.is_dir() {
        return Err(ExportError::InvalidOutput(format!(
            "{} is a folder",
            out.display()
        )));
    }
    if out.exists() && !overwrite {
        return Err(ExportError::OutputExists(out.to_path_buf()));
    }
    Ok(())
}

/// Writes an export to a file.
///
/// The output is checked with [`check_output_path`]. The export is written
/// to `<out>.partial` and moved to `out` only after it was written
/// completely, so a file at `out` is always a complete export.
///
/// # Errors
///
/// [`ExportError`]. After an error, no file exists at `out` that was not
/// there before, and the partial file is removed.
pub fn write_navigation_export_file(
    out: &Path,
    overwrite: bool,
    forbidden: Option<&Path>,
    input: &ExportInput<'_>,
    meta: &ExportMetadata,
) -> Result<ExportStats, ExportError> {
    check_output_path(out, overwrite, forbidden)?;
    let partial = partial_path(out);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                ExportError::InvalidOutput(format!(
                    "{} exists from an earlier run; remove it first",
                    partial.display()
                ))
            } else {
                ExportError::Io(e)
            }
        })?;
    let result = write_navigation_export(file, input, meta).and_then(|stats| {
        std::fs::rename(&partial, out)?;
        Ok(stats)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}
