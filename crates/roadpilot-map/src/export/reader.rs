//! Reference reader and checker of the navigation graph export.
//!
//! It follows `docs/navigation/navigation-export.md` and doubles as an
//! example of an independent consumer: it uses only the text of the file.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{BufRead, Read};

use super::{FORMAT_NAME, SCHEMA_MAJOR};
use roadpilot_map_package::json::{self, Json};

/// Longest line the reader accepts.
const MAX_LINE: usize = 1 << 20;

const NODE_KINDS: [&str; 4] = ["lane_entry", "lane_exit", "prefab_input", "prefab_output"];
const STATUSES: [&str; 4] = ["confirmed", "candidate", "unknown", "forbidden"];
const CATEGORIES: [&str; 7] = [
    "road_lane",
    "road_connection",
    "road_to_prefab_slot",
    "prefab_internal",
    "prefab_slot_to_road",
    "prefab_to_prefab",
    "other",
];

/// Why an export file was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CheckError {
    /// The file could not be read.
    Io(String),
    /// A line is not valid; `line` counts from 1.
    Line { line: usize, message: String },
    /// The file is written in a schema version this reader does not support.
    UnsupportedVersion { major: i64 },
    /// The file ends without the footer, so it may be cut off.
    Incomplete(String),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(m) => write!(f, "cannot read the export: {m}"),
            Self::Line { line, message } => write!(f, "line {line}: {message}"),
            Self::UnsupportedVersion { major } => write!(
                f,
                "unsupported schema version {major} (this reader supports {SCHEMA_MAJOR})"
            ),
            Self::Incomplete(m) => write!(f, "incomplete export: {m}"),
        }
    }
}

impl std::error::Error for CheckError {}

/// The header of an export.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExportHeader {
    pub schema_major: u32,
    pub schema_minor: u32,
    pub generator_version: Option<String>,
    pub game: String,
    pub nodes: usize,
    pub edges: usize,
    pub issues: usize,
    pub position_scale: i64,
    pub limitations: Vec<String>,
}

/// A navigation node. Fields that do not apply to the kind are `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportNode {
    pub id: u32,
    pub kind: String,
    pub road: Option<u64>,
    pub side: Option<String>,
    pub index: Option<u64>,
    pub prefab: Option<u64>,
    pub arm: Option<u64>,
    pub slot: Option<u64>,
    pub map_node: u64,
    pub position: Option<[i64; 3]>,
}

/// A navigation edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportEdge {
    pub id: u32,
    pub from: u32,
    pub to: u32,
    pub status: String,
    pub category: String,
    pub source: u64,
    /// Traffic rule references as `(name, resolved)`.
    pub rules: Vec<(String, bool)>,
}

/// Something that is missing from the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportIssue {
    pub kind: String,
    pub source: u64,
    pub reason: Option<String>,
}

/// One record, handed to the visitor of [`read_navigation_export`].
#[derive(Debug, Clone, Copy)]
pub enum ExportRecord<'a> {
    Header(&'a ExportHeader),
    Node(&'a ExportNode),
    Edge(&'a ExportEdge),
    Issue(&'a ExportIssue),
    Footer,
}

/// What a checked export contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSummary {
    pub header: ExportHeader,
    pub nodes_by_kind: BTreeMap<String, usize>,
    pub edges_by_status: BTreeMap<String, usize>,
    pub edges_by_category: BTreeMap<String, usize>,
    pub issues_by_kind: BTreeMap<String, usize>,
    /// Records of an unknown type that were ignored (allowed for minor
    /// versions).
    pub ignored_records: usize,
}

type Fail = String;

fn req<'a>(obj: &'a Json, key: &str) -> Result<&'a Json, Fail> {
    obj.get(key)
        .ok_or_else(|| format!("missing field \"{key}\""))
}

fn req_str<'a>(obj: &'a Json, key: &str) -> Result<&'a str, Fail> {
    req(obj, key)?
        .as_str()
        .ok_or_else(|| format!("field \"{key}\" must be a string"))
}

fn req_u(obj: &Json, key: &str) -> Result<u64, Fail> {
    req(obj, key)?
        .as_i64()
        .and_then(|v| u64::try_from(v).ok())
        .ok_or_else(|| format!("field \"{key}\" must be a non-negative integer"))
}

fn opt_u(obj: &Json, key: &str) -> Result<Option<u64>, Fail> {
    match obj.get(key) {
        None => Ok(None),
        Some(_) => req_u(obj, key).map(Some),
    }
}

fn opt_str(obj: &Json, key: &str) -> Result<Option<String>, Fail> {
    match obj.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(v) => v
            .as_str()
            .map(|s| Some(s.to_owned()))
            .ok_or_else(|| format!("field \"{key}\" must be a string")),
    }
}

fn hex(obj: &Json, key: &str) -> Result<u64, Fail> {
    let s = req_str(obj, key)?;
    if s.is_empty()
        || s.len() > 16
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(format!(
            "field \"{key}\" must be a lower-case hexadecimal string"
        ));
    }
    u64::from_str_radix(s, 16).map_err(|_| format!("field \"{key}\" is not hexadecimal"))
}

fn one_of(value: &str, allowed: &[&str], what: &str) -> Result<(), Fail> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!("unknown {what} \"{value}\""))
    }
}

fn header_record(v: &Json) -> Result<ExportHeader, CheckError> {
    let line_err = |message: String| CheckError::Line { line: 1, message };
    let format = req_str(v, "format").map_err(line_err)?;
    if format != FORMAT_NAME {
        return Err(line_err(format!("unknown format \"{format}\"")));
    }
    let version = req(v, "schema_version").map_err(line_err)?;
    let major = version
        .get("major")
        .and_then(Json::as_i64)
        .ok_or_else(|| line_err("missing schema_version.major".to_owned()))?;
    if major != i64::from(SCHEMA_MAJOR) {
        return Err(CheckError::UnsupportedVersion { major });
    }
    let minor = version
        .get("minor")
        .and_then(Json::as_i64)
        .and_then(|m| u32::try_from(m).ok())
        .ok_or_else(|| line_err("missing schema_version.minor".to_owned()))?;
    let counts = req(v, "counts").map_err(line_err)?;
    let count = |k: &str| {
        req_u(counts, k)
            .and_then(|c| usize::try_from(c).map_err(|_| format!("counts.{k} is too large")))
            .map_err(line_err)
    };
    let nodes = count("nodes")?;
    if u32::try_from(nodes).is_err() {
        return Err(line_err("more than u32::MAX nodes".to_owned()));
    }
    let coords = req(v, "coordinate_system").map_err(line_err)?;
    let scale = req_u(coords, "position_scale").map_err(line_err)?;
    let limitations = match req(v, "limitations").map_err(line_err)?.as_array() {
        Some(list) => list
            .iter()
            .map(|l| l.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| line_err("limitations must be strings".to_owned()))?,
        None => return Err(line_err("limitations must be an array".to_owned())),
    };
    let generator_version = v
        .get("generator")
        .and_then(|g| g.get("version"))
        .and_then(Json::as_str)
        .map(str::to_owned);
    Ok(ExportHeader {
        schema_major: SCHEMA_MAJOR,
        schema_minor: minor,
        generator_version,
        game: req_str(v, "game").map_err(line_err)?.to_owned(),
        nodes,
        edges: count("edges")?,
        issues: count("issues")?,
        position_scale: i64::try_from(scale).unwrap_or(i64::MAX),
        limitations,
    })
}

fn node_record(v: &Json, expected_id: usize) -> Result<ExportNode, Fail> {
    let id = req_u(v, "id")?;
    if id != expected_id as u64 {
        return Err(format!(
            "node id {id} is out of order (expected {expected_id})"
        ));
    }
    let kind = req_str(v, "kind")?.to_owned();
    one_of(&kind, &NODE_KINDS, "node kind")?;
    let lane = kind.starts_with("lane");
    let (mut road, mut side, mut index, mut prefab, mut arm, mut slot) =
        (None, None, None, None, None, None);
    if lane {
        road = Some(hex(v, "road")?);
        let s = req_str(v, "side")?;
        one_of(s, &["left", "right"], "side")?;
        side = Some(s.to_owned());
        index = Some(req_u(v, "index")?);
    } else {
        prefab = Some(hex(v, "prefab")?);
        arm = Some(req_u(v, "arm")?);
        slot = Some(req_u(v, "slot")?);
    }
    let position = match req(v, "position")? {
        Json::Null => None,
        Json::Arr(a) if a.len() == 3 => {
            let mut p = [0i64; 3];
            for (slot, value) in p.iter_mut().zip(a) {
                *slot = value
                    .as_i64()
                    .ok_or_else(|| "position must hold integers".to_owned())?;
            }
            Some(p)
        }
        _ => return Err("position must be null or three integers".to_owned()),
    };
    Ok(ExportNode {
        id: u32::try_from(id).map_err(|_| "node id is too large".to_owned())?,
        kind,
        road,
        side,
        index,
        prefab,
        arm,
        slot,
        map_node: hex(v, "map_node")?,
        position,
    })
}

fn edge_record(v: &Json, expected_id: usize, nodes: &[u8]) -> Result<ExportEdge, Fail> {
    let id = req_u(v, "id")?;
    if id != expected_id as u64 {
        return Err(format!(
            "edge id {id} is out of order (expected {expected_id})"
        ));
    }
    let endpoint = |key: &str| -> Result<(u32, u8), Fail> {
        let n = req_u(v, key)?;
        let kind = usize::try_from(n)
            .ok()
            .and_then(|i| nodes.get(i))
            .ok_or_else(|| format!("\"{key}\" refers to missing node {n}"))?;
        Ok((
            u32::try_from(n).map_err(|_| "node id too large".to_owned())?,
            *kind,
        ))
    };
    let (from, from_kind) = endpoint("from")?;
    let (to, to_kind) = endpoint("to")?;
    let status = req_str(v, "status")?.to_owned();
    one_of(&status, &STATUSES, "status")?;
    let category = req_str(v, "category")?.to_owned();
    one_of(&category, &CATEGORIES, "category")?;
    // Node kind numbers: 0 lane_entry, 1 lane_exit, 2 prefab_input, 3 prefab_output.
    let expected = match category.as_str() {
        "road_lane" => Some((0, 1)),
        "road_connection" => Some((1, 0)),
        "road_to_prefab_slot" => Some((1, 2)),
        "prefab_internal" => Some((2, 3)),
        "prefab_slot_to_road" => Some((3, 0)),
        "prefab_to_prefab" => Some((3, 2)),
        _ => None,
    };
    if expected.is_some_and(|e| e != (from_kind, to_kind)) {
        return Err(format!(
            "edge category \"{category}\" does not fit its node kinds"
        ));
    }
    let mut rules = Vec::new();
    if let Some(list) = v.get("rules") {
        for r in list
            .as_array()
            .ok_or_else(|| "rules must be an array".to_owned())?
        {
            let name = req_str(r, "name")?.to_owned();
            let resolved = req(r, "resolved")?
                .as_bool()
                .ok_or_else(|| "resolved must be a boolean".to_owned())?;
            rules.push((name, resolved));
        }
    }
    Ok(ExportEdge {
        id: u32::try_from(id).map_err(|_| "edge id is too large".to_owned())?,
        from,
        to,
        status,
        category,
        source: hex(v, "source")?,
        rules,
    })
}

fn issue_record(v: &Json) -> Result<ExportIssue, Fail> {
    Ok(ExportIssue {
        kind: req_str(v, "kind")?.to_owned(),
        source: hex(v, "source")?,
        reason: opt_str(v, "reason")?,
    })
}

#[derive(PartialEq, PartialOrd, Clone, Copy)]
enum Stage {
    Nodes,
    Edges,
    Issues,
    Done,
}

/// Reads and checks an export, handing every record to `visit`.
///
/// Checks that the header names the format and a supported major version,
/// that node and edge ids are consecutive, that every edge refers to
/// existing nodes whose kinds fit the edge category, that statuses, kinds
/// and categories are known, that the sections come in order, and that the
/// footer is present and its counts equal the header counts and the number of
/// records. Records of an unknown type are ignored; unknown fields are
/// ignored.
///
/// # Errors
///
/// A [`CheckError`] for the first problem found.
pub fn read_navigation_export<R: BufRead>(
    mut reader: R,
    visit: &mut dyn FnMut(ExportRecord<'_>),
) -> Result<ExportSummary, CheckError> {
    let mut buf = String::new();
    let mut line_no = 0usize;
    let mut next_line = |buf: &mut String, line_no: &mut usize| -> Result<bool, CheckError> {
        buf.clear();
        let mut limited = (&mut reader).take(MAX_LINE as u64 + 1);
        let n = limited
            .read_line(buf)
            .map_err(|e| CheckError::Io(e.to_string()))?;
        *line_no += 1;
        if n > MAX_LINE {
            return Err(CheckError::Line {
                line: *line_no,
                message: "line is too long".to_owned(),
            });
        }
        Ok(n > 0)
    };

    if !next_line(&mut buf, &mut line_no)? {
        return Err(CheckError::Incomplete("the file is empty".to_owned()));
    }
    let first = json::parse(&buf).map_err(|message| CheckError::Line { line: 1, message })?;
    if first.get("t").and_then(Json::as_str) != Some("header") {
        return Err(CheckError::Line {
            line: 1,
            message: "the first line must be the header".to_owned(),
        });
    }
    let header = header_record(&first)?;
    visit(ExportRecord::Header(&header));

    let mut summary = ExportSummary {
        header: header.clone(),
        nodes_by_kind: BTreeMap::new(),
        edges_by_status: BTreeMap::new(),
        edges_by_category: BTreeMap::new(),
        issues_by_kind: BTreeMap::new(),
        ignored_records: 0,
    };
    // Kind number per node, for checking edges. Grown record by record, so a
    // wrong header count cannot cause a large allocation.
    let mut node_kinds: Vec<u8> = Vec::new();
    let (mut edges, mut issues) = (0usize, 0usize);
    let mut stage = Stage::Nodes;

    while next_line(&mut buf, &mut line_no)? {
        let at = |message: String| CheckError::Line {
            line: line_no,
            message,
        };
        if buf.trim().is_empty() {
            if stage == Stage::Done {
                continue;
            }
            return Err(at("empty line".to_owned()));
        }
        if stage == Stage::Done {
            return Err(at("data after the footer".to_owned()));
        }
        let v = json::parse(&buf).map_err(at)?;
        let t = v
            .get("t")
            .and_then(Json::as_str)
            .ok_or_else(|| at("missing record type \"t\"".to_owned()))?
            .to_owned();
        match t.as_str() {
            "node" => {
                if stage != Stage::Nodes {
                    return Err(at("node after the nodes section".to_owned()));
                }
                let node = node_record(&v, node_kinds.len()).map_err(at)?;
                let k = NODE_KINDS
                    .iter()
                    .position(|n| *n == node.kind)
                    .unwrap_or(usize::MAX);
                node_kinds.push(u8::try_from(k).unwrap_or(u8::MAX));
                *summary.nodes_by_kind.entry(node.kind.clone()).or_default() += 1;
                visit(ExportRecord::Node(&node));
            }
            "edge" => {
                if stage > Stage::Edges {
                    return Err(at("edge after the edges section".to_owned()));
                }
                stage = Stage::Edges;
                let edge = edge_record(&v, edges, &node_kinds).map_err(at)?;
                edges += 1;
                *summary
                    .edges_by_status
                    .entry(edge.status.clone())
                    .or_default() += 1;
                *summary
                    .edges_by_category
                    .entry(edge.category.clone())
                    .or_default() += 1;
                visit(ExportRecord::Edge(&edge));
            }
            "issue" => {
                if stage > Stage::Issues {
                    return Err(at("issue after the issues section".to_owned()));
                }
                stage = Stage::Issues;
                let issue = issue_record(&v).map_err(at)?;
                issues += 1;
                *summary
                    .issues_by_kind
                    .entry(issue.kind.clone())
                    .or_default() += 1;
                visit(ExportRecord::Issue(&issue));
            }
            "footer" => {
                let complete = v.get("complete").and_then(Json::as_bool);
                if complete != Some(true) {
                    return Err(at("the footer must say complete: true".to_owned()));
                }
                let counts = [
                    ("nodes", node_kinds.len(), header.nodes),
                    ("edges", edges, header.edges),
                    ("issues", issues, header.issues),
                ];
                for (name, found, declared) in counts {
                    let in_footer = opt_u(&v, name).map_err(at)?;
                    if found != declared || in_footer != Some(declared as u64) {
                        return Err(at(format!(
                            "{name}: header says {declared}, footer says {in_footer:?}, file has {found}"
                        )));
                    }
                }
                stage = Stage::Done;
                visit(ExportRecord::Footer);
            }
            _ => summary.ignored_records += 1,
        }
    }
    if stage != Stage::Done {
        return Err(CheckError::Incomplete(
            "the file ends before the footer".to_owned(),
        ));
    }
    Ok(summary)
}

/// Checks an export and returns what it contains.
///
/// # Errors
///
/// A [`CheckError`], see [`read_navigation_export`].
pub fn check_navigation_export<R: BufRead>(reader: R) -> Result<ExportSummary, CheckError> {
    read_navigation_export(reader, &mut |_| {})
}
