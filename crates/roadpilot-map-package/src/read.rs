//! Validating reader of the geometry file.
//!
//! Reads the whole file into memory and checks it: format and version, the
//! order of sections, ascending unique ids, references between records,
//! finite numbers, consistent lane lists and connection statuses, and the
//! footer. Unknown record types and unknown fields are ignored (allowed for
//! minor versions).

use std::collections::HashMap;
use std::fmt;
use std::io::{BufRead, Read};

use roadpilot_map_model::Quality;

use crate::json::{self, Json};
use crate::records::{
    ArmLinkRecord, ArmRecord, ConnectionRecord, ConnectionStatus, CurveRecord, JunctionRecord,
    LaneDirectionRecord, LaneRecord, LanesRecord, LinkRecord, MovementRecord, NodeRecord,
    RoadRecord, Side, TrafficSideRecord,
};
use crate::{GEOMETRY_FORMAT_NAME, SCHEMA_MAJOR};

/// Longest line the reader accepts.
const MAX_LINE: usize = 4 << 20;

/// Largest extent of a curve box, per axis, world units. Real roads are
/// shorter than 1,000 units; larger values mean corrupt data and would make
/// the spatial index huge.
pub const MAX_CURVE_EXTENT: f64 = 10_000.0;

/// Why a geometry file was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReadError {
    /// The file could not be read.
    Io(String),
    /// A line is not valid; `line` counts from 1.
    Line { line: usize, message: String },
    /// The file uses a schema version this reader does not support.
    UnsupportedVersion { major: i64 },
    /// The file ends without the footer, so it may be cut off.
    Incomplete(String),
    /// Records disagree with each other.
    Inconsistent(String),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(m) => write!(f, "cannot read the geometry file: {m}"),
            Self::Line { line, message } => write!(f, "line {line}: {message}"),
            Self::UnsupportedVersion { major } => write!(
                f,
                "unsupported geometry schema version {major} (this reader supports {SCHEMA_MAJOR})"
            ),
            Self::Incomplete(m) => write!(f, "incomplete geometry file: {m}"),
            Self::Inconsistent(m) => write!(f, "inconsistent geometry file: {m}"),
        }
    }
}

impl std::error::Error for ReadError {}

/// The content of a geometry file.
#[derive(Debug, Clone, PartialEq)]
pub struct GeometryData {
    pub schema_minor: u32,
    pub nodes: Vec<NodeRecord>,
    pub roads: Vec<RoadRecord>,
    pub junctions: Vec<JunctionRecord>,
    pub connections: Vec<ConnectionRecord>,
    /// Records of an unknown type that were ignored.
    pub ignored_records: usize,
}

type Fail = String;

fn req<'a>(obj: &'a Json, key: &str) -> Result<&'a Json, Fail> {
    obj.get(key)
        .ok_or_else(|| format!("missing field \"{key}\""))
}

fn str_of<'a>(obj: &'a Json, key: &str) -> Result<&'a str, Fail> {
    req(obj, key)?
        .as_str()
        .ok_or_else(|| format!("\"{key}\" must be a string"))
}

fn bool_of(obj: &Json, key: &str) -> Result<bool, Fail> {
    req(obj, key)?
        .as_bool()
        .ok_or_else(|| format!("\"{key}\" must be a boolean"))
}

fn u64_of(obj: &Json, key: &str) -> Result<u64, Fail> {
    req(obj, key)?
        .as_i64()
        .and_then(|v| u64::try_from(v).ok())
        .ok_or_else(|| format!("\"{key}\" must be a non-negative integer"))
}

fn u32_of(obj: &Json, key: &str) -> Result<u32, Fail> {
    u32::try_from(u64_of(obj, key)?).map_err(|_| format!("\"{key}\" is too large"))
}

fn f64_of(v: &Json, what: &str) -> Result<f64, Fail> {
    v.as_f64()
        .filter(|f| f.is_finite())
        .ok_or_else(|| format!("{what} must be a finite number"))
}

fn parse_hex(s: &str) -> Result<u64, Fail> {
    if s.is_empty()
        || s.len() > 16
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(format!(
            "{s:?} is not a lower-case hexadecimal id of at most 16 digits"
        ));
    }
    u64::from_str_radix(s, 16).map_err(|_| format!("{s:?} is not hexadecimal"))
}

fn hex_of(obj: &Json, key: &str) -> Result<u64, Fail> {
    parse_hex(str_of(obj, key)?).map_err(|m| format!("\"{key}\": {m}"))
}

fn array_of<'a>(obj: &'a Json, key: &str) -> Result<&'a [Json], Fail> {
    req(obj, key)?
        .as_array()
        .ok_or_else(|| format!("\"{key}\" must be an array"))
}

fn opt_str(obj: &Json, key: &str) -> Result<Option<String>, Fail> {
    match obj.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("\"{key}\" must be a string")),
    }
}

fn link_of(v: &Json) -> Result<LinkRecord, Fail> {
    if matches!(v, Json::Null) {
        return Ok(LinkRecord::None);
    }
    if v.get("road").is_some() {
        return Ok(LinkRecord::Road(hex_of(v, "road")?));
    }
    if v.get("item").is_some() {
        return Ok(LinkRecord::Item(hex_of(v, "item")?));
    }
    if v.get("unresolved").is_some() {
        return Ok(LinkRecord::Unresolved(hex_of(v, "unresolved")?));
    }
    Err("a node link must be null or name road, item or unresolved".to_owned())
}

fn node_record(v: &Json) -> Result<NodeRecord, Fail> {
    let pos = array_of(v, "position")?;
    if pos.len() != 3 {
        return Err("position needs three integers".to_owned());
    }
    let mut position = [0i32; 3];
    for (slot, p) in position.iter_mut().zip(pos) {
        *slot = p
            .as_i64()
            .and_then(|i| i32::try_from(i).ok())
            .ok_or_else(|| "position values must be 32-bit integers".to_owned())?;
    }
    let rot = array_of(v, "rotation")?;
    if rot.len() != 4 {
        return Err("rotation needs four numbers".to_owned());
    }
    let mut rotation = [0f32; 4];
    for (slot, r) in rotation.iter_mut().zip(rot) {
        *slot = f64_of(r, "a rotation value")? as f32;
    }
    let norm = rotation.iter().map(|c| c * c).sum::<f32>().sqrt();
    if norm.is_nan() || (norm - 1.0).abs() > 1e-3 {
        return Err("rotation is not a unit quaternion".to_owned());
    }
    Ok(NodeRecord {
        id: hex_of(v, "id")?,
        position,
        rotation,
        backward: link_of(req(v, "backward")?)?,
        forward: link_of(req(v, "forward")?)?,
    })
}

fn lanes_of(v: &Json) -> Result<LanesRecord, Fail> {
    match str_of(v, "status")? {
        "unresolved" => Ok(LanesRecord::Unresolved(str_of(v, "reason")?.to_owned())),
        "resolved" => {
            let mut lanes = Vec::new();
            for l in array_of(v, "items")? {
                let side = Side::parse(str_of(l, "side")?).ok_or("unknown lane side")?;
                let index = u16::try_from(u64_of(l, "index")?)
                    .map_err(|_| "lane index is too large".to_owned())?;
                let direction = match req(l, "direction")? {
                    Json::Null => None,
                    Json::Str(s) if s == "forward" => Some(LaneDirectionRecord::Forward),
                    Json::Str(s) if s == "backward" => Some(LaneDirectionRecord::Backward),
                    _ => return Err("unknown lane direction".to_owned()),
                };
                let offset = match req(l, "offset")? {
                    Json::Null => None,
                    Json::Arr(a) if a.len() == 2 => Some([
                        f64_of(&a[0], "a lane offset")?,
                        f64_of(&a[1], "a lane offset")?,
                    ]),
                    _ => return Err("offset must be null or two numbers".to_owned()),
                };
                match req(l, "width")? {
                    Json::Null => {}
                    // A width appears only if the data ever stores one; the
                    // field then needs the same quality handling as the other
                    // geometry. Schema 1.0 never writes one.
                    _ => return Err("lane widths are not part of schema 1.0".to_owned()),
                }
                let geometry = Quality::parse(str_of(l, "geometry")?)
                    .ok_or("unknown lane geometry quality")?;
                lanes.push(LaneRecord {
                    side,
                    index,
                    lane_type: str_of(l, "type")?.to_owned(),
                    type_certain: bool_of(l, "type_certain")?,
                    direction,
                    offset,
                    geometry,
                });
            }
            // Per side, indices must be 0, 1, 2, ... and sides must not be
            // interleaved: left lanes first, then right lanes.
            let mut expected = (Side::Left, 0u16);
            for l in &lanes {
                if l.side != expected.0 {
                    if l.side < expected.0 {
                        return Err("left lanes must come before right lanes".to_owned());
                    }
                    expected = (l.side, 0);
                }
                if l.index != expected.1 {
                    return Err("lane indices must be consecutive from 0 per side".to_owned());
                }
                expected.1 += 1;
            }
            Ok(LanesRecord::Resolved(lanes))
        }
        other => Err(format!("unknown lanes status {other:?}")),
    }
}

fn curve_of(v: &Json, tolerance: Option<f64>) -> Result<CurveRecord, Fail> {
    let quality = Quality::parse(str_of(v, "quality")?).ok_or("unknown curve quality")?;
    match quality {
        Quality::Unavailable => Ok(CurveRecord::Unavailable(str_of(v, "reason")?.to_owned())),
        Quality::Confirmed => Err("curves are never confirmed in schema 1.0".to_owned()),
        Quality::Estimated => {
            let num = |key: &str| f64_of(req(v, key)?, key);
            let b = array_of(v, "bounds")?;
            if b.len() != 6 {
                return Err("bounds needs six numbers".to_owned());
            }
            let mut bounds = [0f64; 6];
            for (slot, x) in bounds.iter_mut().zip(b) {
                *slot = f64_of(x, "a bounds value")?;
            }
            for axis in 0..3 {
                if bounds[axis] > bounds[axis + 3] {
                    return Err("bounds minimum is above maximum".to_owned());
                }
                if bounds[axis + 3] - bounds[axis] > MAX_CURVE_EXTENT {
                    return Err("curve bounds are implausibly large".to_owned());
                }
            }
            let tangent_scale = num("tangent_scale")?;
            let estimated_length = num("estimated_length")?;
            let length_deviation = num("length_deviation")?;
            if tangent_scale <= 0.0 || estimated_length < 0.0 || length_deviation < 0.0 {
                return Err("curve lengths must be positive".to_owned());
            }
            let length_consistent = bool_of(v, "length_consistent")?;
            if let Some(t) = tolerance {
                if length_consistent != (length_deviation <= t) {
                    return Err(
                        "length_consistent contradicts length_deviation and the tolerance"
                            .to_owned(),
                    );
                }
            }
            Ok(CurveRecord::Estimated {
                tangent_scale,
                estimated_length,
                length_deviation,
                length_consistent,
                bounds,
            })
        }
    }
}

fn road_record(v: &Json, tolerance: Option<f64>) -> Result<RoadRecord, Fail> {
    let traffic_side = match str_of(v, "traffic_side")? {
        "right" => TrafficSideRecord::Right,
        "left" => TrafficSideRecord::Left,
        "unknown" => TrafficSideRecord::Unknown(str_of(v, "traffic_side_reason")?.to_owned()),
        other => return Err(format!("unknown traffic side {other:?}")),
    };
    let stored_length = f64_of(req(v, "stored_length")?, "stored_length")?;
    if stored_length <= 0.0 {
        return Err("stored_length must be positive".to_owned());
    }
    let record = RoadRecord {
        id: hex_of(v, "id")?,
        start: hex_of(v, "start")?,
        end: hex_of(v, "end")?,
        look: str_of(v, "look")?.to_owned(),
        stored_length: stored_length as f32,
        traffic_side,
        lanes: lanes_of(req(v, "lanes")?)?,
        curve: curve_of(req(v, "curve")?, tolerance)?,
    };
    if record.start == record.end {
        return Err("a road cannot start and end at the same node".to_owned());
    }
    Ok(record)
}

fn arm_link_of(v: &Json) -> Result<ArmLinkRecord, Fail> {
    Ok(match str_of(v, "kind")? {
        "road" => ArmLinkRecord::Road {
            road: hex_of(v, "road")?,
            road_ends_here: bool_of(v, "ends_here")?,
        },
        "prefab" => ArmLinkRecord::Prefab(hex_of(v, "prefab")?),
        "item" => ArmLinkRecord::Item(hex_of(v, "item")?),
        "unresolved" => ArmLinkRecord::Unresolved(hex_of(v, "uid")?),
        "none" => ArmLinkRecord::None,
        other => return Err(format!("unknown arm link kind {other:?}")),
    })
}

fn junction_record(v: &Json) -> Result<JunctionRecord, Fail> {
    let unresolved = match str_of(v, "resolution")? {
        "resolved" => None,
        "unresolved" => Some(str_of(v, "resolution_reason")?.to_owned()),
        other => return Err(format!("unknown resolution {other:?}")),
    };
    let nodes = array_of(v, "nodes")?
        .iter()
        .map(|n| {
            n.as_str()
                .ok_or_else(|| "node ids must be strings".to_owned())
                .and_then(parse_hex)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut arms = Vec::new();
    for a in array_of(v, "arms")? {
        let arm = ArmRecord {
            index: u32_of(a, "index")?,
            node: hex_of(a, "node")?,
            link: arm_link_of(req(a, "link")?)?,
        };
        if nodes.get(arm.index as usize) != Some(&arm.node) {
            return Err("an arm does not name the node at its index".to_owned());
        }
        arms.push(arm);
    }
    let mut movements = Vec::new();
    for m in array_of(v, "movements")? {
        let status = ConnectionStatus::parse(str_of(m, "status")?).ok_or("unknown status")?;
        let movement = MovementRecord {
            from_arm: u32_of(m, "from_arm")?,
            from_slot: u32_of(m, "from_slot")?,
            to_arm: u32_of(m, "to_arm")?,
            to_slot: u32_of(m, "to_slot")?,
            status,
        };
        if movement.from_arm as usize >= nodes.len() || movement.to_arm as usize >= nodes.len() {
            return Err("a movement names an arm that does not exist".to_owned());
        }
        movements.push(movement);
    }
    if unresolved.is_some() && !movements.is_empty() {
        return Err("an unresolved junction cannot have movements".to_owned());
    }
    Ok(JunctionRecord {
        id: hex_of(v, "id")?,
        model: str_of(v, "model")?.to_owned(),
        unresolved,
        nodes,
        arms,
        movements,
    })
}

fn lane_ref(v: &Json) -> Result<(Side, u16), Fail> {
    let s = v.as_str().ok_or("a lane reference must be a string")?;
    let side = match s.as_bytes().first() {
        Some(b'l') => Side::Left,
        Some(b'r') => Side::Right,
        _ => return Err(format!("bad lane reference {s:?}")),
    };
    let index = s[1..]
        .parse::<u16>()
        .map_err(|_| format!("bad lane reference {s:?}"))?;
    Ok((side, index))
}

fn connection_record(v: &Json) -> Result<ConnectionRecord, Fail> {
    if str_of(v, "kind")? != "road_road" {
        return Err("unknown connection kind".to_owned());
    }
    let status = ConnectionStatus::parse(str_of(v, "status")?).ok_or("unknown status")?;
    let reason = opt_str(v, "reason")?;
    let mut pairs = Vec::new();
    if let Some(list) = v.get("pairs") {
        for p in list.as_array().ok_or("pairs must be an array")? {
            let p = p
                .as_array()
                .filter(|p| p.len() == 2)
                .ok_or("a pair needs two lanes")?;
            pairs.push((lane_ref(&p[0])?, lane_ref(&p[1])?));
        }
    }
    match status {
        ConnectionStatus::Candidate | ConnectionStatus::Confirmed => {
            if pairs.is_empty() {
                return Err("a candidate or confirmed connection needs lane pairs".to_owned());
            }
        }
        ConnectionStatus::Unknown | ConnectionStatus::Forbidden => {
            if !pairs.is_empty() {
                return Err("an unknown or forbidden connection has no lane pairs".to_owned());
            }
            if reason.is_none() {
                return Err("an unknown or forbidden connection needs a reason".to_owned());
            }
        }
    }
    Ok(ConnectionRecord {
        from: hex_of(v, "from")?,
        via: hex_of(v, "via")?,
        to: hex_of(v, "to")?,
        status,
        reason,
        pairs,
    })
}

#[derive(PartialEq, PartialOrd, Clone, Copy)]
enum Stage {
    Nodes,
    Roads,
    Junctions,
    Connections,
    Done,
}

/// Counts promised by the header.
struct Declared {
    nodes: u64,
    roads: u64,
    junctions: u64,
    connections: u64,
}

fn ascending(prev: Option<u64>, id: u64, what: &str) -> Result<(), Fail> {
    match prev {
        Some(p) if id <= p => Err(format!("{what} ids must be strictly ascending")),
        _ => Ok(()),
    }
}

/// Reads and validates a geometry file.
///
/// `length_tolerance` is the tolerance of the manifest; if given, the reader
/// checks that every road's `length_consistent` agrees with it.
///
/// # Errors
///
/// A [`ReadError`] for the first problem found.
pub fn read_geometry<R: BufRead>(
    mut reader: R,
    length_tolerance: Option<f64>,
) -> Result<GeometryData, ReadError> {
    let mut buf = String::new();
    let mut line_no = 0usize;
    let mut next_line = |buf: &mut String, line_no: &mut usize| -> Result<bool, ReadError> {
        buf.clear();
        let mut limited = (&mut reader).take(MAX_LINE as u64 + 1);
        let n = limited
            .read_line(buf)
            .map_err(|e| ReadError::Io(e.to_string()))?;
        *line_no += 1;
        if n > MAX_LINE {
            return Err(ReadError::Line {
                line: *line_no,
                message: "line is too long".to_owned(),
            });
        }
        Ok(n > 0)
    };

    if !next_line(&mut buf, &mut line_no)? {
        return Err(ReadError::Incomplete("the file is empty".to_owned()));
    }
    let at1 = |message: String| ReadError::Line { line: 1, message };
    let first = json::parse(&buf).map_err(at1)?;
    if first.get("t").and_then(Json::as_str) != Some("header")
        || first.get("format").and_then(Json::as_str) != Some(GEOMETRY_FORMAT_NAME)
    {
        return Err(ReadError::Line {
            line: 1,
            message: format!("the first line must be a {GEOMETRY_FORMAT_NAME} header"),
        });
    }
    let version = first.get("schema_version").ok_or_else(|| ReadError::Line {
        line: 1,
        message: "missing field \"schema_version\"".to_owned(),
    })?;
    let major = version.get("major").and_then(Json::as_i64).unwrap_or(-1);
    if major != i64::from(SCHEMA_MAJOR) {
        return Err(ReadError::UnsupportedVersion { major });
    }
    let line1 = |m: String| ReadError::Line {
        line: 1,
        message: m,
    };
    let schema_minor = u32_of(version, "minor").map_err(line1)?;
    let counts = first.get("counts").ok_or_else(|| ReadError::Line {
        line: 1,
        message: "missing field \"counts\"".to_owned(),
    })?;
    let declared = (|| -> Result<Declared, Fail> {
        Ok(Declared {
            nodes: u64_of(counts, "nodes")?,
            roads: u64_of(counts, "roads")?,
            junctions: u64_of(counts, "junctions")?,
            connections: u64_of(counts, "connections")?,
        })
    })()
    .map_err(|message| ReadError::Line { line: 1, message })?;

    let mut data = GeometryData {
        schema_minor,
        nodes: Vec::new(),
        roads: Vec::new(),
        junctions: Vec::new(),
        connections: Vec::new(),
        ignored_records: 0,
    };
    let mut stage = Stage::Nodes;
    // Position of every id in the vectors, for the reference checks. Filled
    // record by record, so a wrong header count cannot cause a large
    // allocation.
    let mut node_index: HashMap<u64, usize> = HashMap::new();
    let mut road_index: HashMap<u64, usize> = HashMap::new();

    while next_line(&mut buf, &mut line_no)? {
        let at = |message: String| ReadError::Line {
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
                let n = node_record(&v).map_err(at)?;
                ascending(data.nodes.last().map(|p| p.id), n.id, "node").map_err(at)?;
                node_index.insert(n.id, data.nodes.len());
                data.nodes.push(n);
            }
            "road" => {
                if stage > Stage::Roads {
                    return Err(at("road after the roads section".to_owned()));
                }
                stage = Stage::Roads;
                let r = road_record(&v, length_tolerance).map_err(at)?;
                ascending(data.roads.last().map(|p| p.id), r.id, "road").map_err(at)?;
                for (what, node) in [("start", r.start), ("end", r.end)] {
                    if !node_index.contains_key(&node) {
                        return Err(at(format!("the {what} node {node:x} does not exist")));
                    }
                }
                road_index.insert(r.id, data.roads.len());
                data.roads.push(r);
            }
            "junction" => {
                if stage > Stage::Junctions {
                    return Err(at("junction after the junctions section".to_owned()));
                }
                stage = Stage::Junctions;
                let j = junction_record(&v).map_err(at)?;
                ascending(data.junctions.last().map(|p| p.id), j.id, "junction").map_err(at)?;
                for n in &j.nodes {
                    if !node_index.contains_key(n) {
                        return Err(at(format!("the node {n:x} does not exist")));
                    }
                }
                for a in &j.arms {
                    if let ArmLinkRecord::Road {
                        road,
                        road_ends_here,
                    } = a.link
                    {
                        let Some(&ri) = road_index.get(&road) else {
                            return Err(at(format!("the road {road:x} of an arm does not exist")));
                        };
                        let r = &data.roads[ri];
                        let node = if road_ends_here { r.end } else { r.start };
                        if node != a.node {
                            return Err(at(format!(
                                "the road {road:x} does not {} at the arm's node",
                                if road_ends_here { "end" } else { "start" }
                            )));
                        }
                    }
                }
                data.junctions.push(j);
            }
            "connection" => {
                if stage > Stage::Connections {
                    return Err(at("connection after the connections section".to_owned()));
                }
                stage = Stage::Connections;
                let c = connection_record(&v).map_err(at)?;
                let key = |c: &ConnectionRecord| (c.from, c.to);
                if let Some(p) = data.connections.last() {
                    if key(&c) <= key(p) {
                        return Err(at("connections must be strictly ascending".to_owned()));
                    }
                }
                let (Some(&fi), Some(&ti)) = (road_index.get(&c.from), road_index.get(&c.to))
                else {
                    return Err(at(
                        "a connection names a road that does not exist".to_owned()
                    ));
                };
                if data.roads[fi].end != c.via || data.roads[ti].start != c.via {
                    return Err(at(
                        "the connection's node is not the end of the first road and the start of the second"
                            .to_owned(),
                    ));
                }
                data.connections.push(c);
            }
            "footer" => {
                if v.get("complete").and_then(Json::as_bool) != Some(true) {
                    return Err(at("the footer must say complete: true".to_owned()));
                }
                let found = [
                    ("nodes", data.nodes.len() as u64, declared.nodes),
                    ("roads", data.roads.len() as u64, declared.roads),
                    ("junctions", data.junctions.len() as u64, declared.junctions),
                    (
                        "connections",
                        data.connections.len() as u64,
                        declared.connections,
                    ),
                ];
                for (name, n, d) in found {
                    if n != d || u64_of(&v, name).map_err(at)? != d {
                        return Err(ReadError::Line {
                            line: line_no,
                            message: format!(
                                "{name}: header says {d}, the file has {n}, the footer disagrees"
                            ),
                        });
                    }
                }
                stage = Stage::Done;
            }
            _ => data.ignored_records += 1,
        }
    }
    if stage != Stage::Done {
        return Err(ReadError::Incomplete(
            "the file ends before the footer".to_owned(),
        ));
    }
    // Node links to roads must agree with the roads: a node whose forward
    // slot is a road must be that road's start, and its backward slot its
    // end.
    for n in &data.nodes {
        for (link, ends_here) in [(n.forward, false), (n.backward, true)] {
            if let LinkRecord::Road(r) = link {
                let ok = road_index.get(&r).is_some_and(|&i| {
                    let road = &data.roads[i];
                    (if ends_here { road.end } else { road.start }) == n.id
                });
                if !ok {
                    return Err(ReadError::Inconsistent(format!(
                        "node {:x} names road {r:x}, which does not {} there",
                        n.id,
                        if ends_here { "end" } else { "start" }
                    )));
                }
            }
        }
    }
    Ok(data)
}
