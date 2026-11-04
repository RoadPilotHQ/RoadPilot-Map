//! Writer of the geometry file.
//!
//! The file is written as a stream of JSON lines. The output depends only on
//! the records and contains no time stamp, so equal records give equal bytes.

use std::fmt::{self, Write as _};
use std::io::{self, BufWriter, Write};

use crate::checksum::HashingWriter;
use crate::json;
use crate::records::{
    ArmLinkRecord, ConnectionRecord, ConnectionStatus, CurveRecord, JunctionRecord,
    LaneDirectionRecord, LanesRecord, LinkRecord, MovementRecord, NodeRecord, RoadRecord,
    TrafficSideRecord,
};
use crate::{GEOMETRY_FORMAT_NAME, SCHEMA_MAJOR, SCHEMA_MINOR};

/// The number of records of each type in the geometry file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GeometryCounts {
    pub nodes: u64,
    pub roads: u64,
    pub junctions: u64,
    pub connections: u64,
}

/// What a written geometry file looks like from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WrittenFile {
    pub counts: GeometryCounts,
    /// Number of lanes over all roads.
    pub lanes: u64,
    pub bytes: u64,
    pub checksum: u64,
}

/// Why the geometry file could not be written.
#[derive(Debug)]
#[non_exhaustive]
pub enum WriteError {
    /// Writing failed.
    Io(io::Error),
    /// The records are not valid for the format (for example a value that is
    /// not finite) or do not match the declared counts. The output is
    /// incomplete.
    Invalid(String),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot write the geometry file: {e}"),
            Self::Invalid(m) => write!(f, "invalid geometry records: {m}"),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<io::Error> for WriteError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

fn float(out: &mut String, v: f64, what: &str) -> Result<(), WriteError> {
    if json::push_f64(out, v) {
        Ok(())
    } else {
        Err(WriteError::Invalid(format!("{what} is not finite")))
    }
}

fn link(out: &mut String, l: LinkRecord) {
    match l {
        LinkRecord::None => out.push_str("null"),
        LinkRecord::Road(u) => {
            out.push_str("{\"road\":");
            json::push_hex(out, u);
            out.push('}');
        }
        LinkRecord::Item(u) => {
            out.push_str("{\"item\":");
            json::push_hex(out, u);
            out.push('}');
        }
        LinkRecord::Unresolved(u) => {
            out.push_str("{\"unresolved\":");
            json::push_hex(out, u);
            out.push('}');
        }
    }
}

fn node_line(out: &mut String, n: &NodeRecord) -> Result<(), WriteError> {
    out.push_str("{\"t\":\"node\",\"id\":");
    json::push_hex(out, n.id);
    let _ = write!(
        out,
        ",\"position\":[{},{},{}],\"rotation\":[",
        n.position[0], n.position[1], n.position[2]
    );
    for (i, c) in n.rotation.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        float(out, f64::from(*c), "node rotation")?;
    }
    out.push_str("],\"backward\":");
    link(out, n.backward);
    out.push_str(",\"forward\":");
    link(out, n.forward);
    out.push_str("}\n");
    Ok(())
}

fn lanes_json(out: &mut String, lanes: &LanesRecord) -> Result<u64, WriteError> {
    match lanes {
        LanesRecord::Unresolved(reason) => {
            out.push_str("{\"status\":\"unresolved\",\"reason\":");
            json::push_str(out, reason);
            out.push('}');
            Ok(0)
        }
        LanesRecord::Resolved(list) => {
            out.push_str("{\"status\":\"resolved\",\"items\":[");
            for (i, l) in list.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let _ = write!(
                    out,
                    "{{\"side\":\"{}\",\"index\":{},\"type\":",
                    l.side.name(),
                    l.index
                );
                json::push_str(out, &l.lane_type);
                let _ = write!(out, ",\"type_certain\":{},\"direction\":", l.type_certain);
                match l.direction {
                    Some(LaneDirectionRecord::Forward) => out.push_str("\"forward\""),
                    Some(LaneDirectionRecord::Backward) => out.push_str("\"backward\""),
                    None => out.push_str("null"),
                }
                out.push_str(",\"offset\":");
                match l.offset {
                    Some([a, b]) => {
                        out.push('[');
                        float(out, a, "lane offset")?;
                        out.push(',');
                        float(out, b, "lane offset")?;
                        out.push(']');
                    }
                    None => out.push_str("null"),
                }
                let _ = write!(
                    out,
                    ",\"width\":null,\"geometry\":\"{}\"}}",
                    l.geometry.as_str()
                );
            }
            out.push_str("]}");
            Ok(list.len() as u64)
        }
    }
}

fn curve_json(out: &mut String, c: &CurveRecord) -> Result<(), WriteError> {
    match c {
        CurveRecord::Unavailable(reason) => {
            out.push_str("{\"quality\":\"unavailable\",\"reason\":");
            json::push_str(out, reason);
            out.push('}');
        }
        CurveRecord::Estimated {
            tangent_scale,
            estimated_length,
            length_deviation,
            length_consistent,
            bounds,
        } => {
            out.push_str("{\"quality\":\"estimated\",\"tangent_scale\":");
            float(out, *tangent_scale, "tangent scale")?;
            out.push_str(",\"estimated_length\":");
            float(out, *estimated_length, "estimated length")?;
            out.push_str(",\"length_deviation\":");
            float(out, *length_deviation, "length deviation")?;
            let _ = write!(
                out,
                ",\"length_consistent\":{length_consistent},\"bounds\":["
            );
            for (i, b) in bounds.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                float(out, *b, "curve bounds")?;
            }
            out.push_str("]}");
        }
    }
    Ok(())
}

fn road_line(out: &mut String, r: &RoadRecord) -> Result<u64, WriteError> {
    out.push_str("{\"t\":\"road\",\"id\":");
    json::push_hex(out, r.id);
    out.push_str(",\"start\":");
    json::push_hex(out, r.start);
    out.push_str(",\"end\":");
    json::push_hex(out, r.end);
    out.push_str(",\"look\":");
    json::push_str(out, &r.look);
    out.push_str(",\"stored_length\":");
    float(out, f64::from(r.stored_length), "stored length")?;
    match &r.traffic_side {
        TrafficSideRecord::Right => out.push_str(",\"traffic_side\":\"right\""),
        TrafficSideRecord::Left => out.push_str(",\"traffic_side\":\"left\""),
        TrafficSideRecord::Unknown(reason) => {
            out.push_str(",\"traffic_side\":\"unknown\",\"traffic_side_reason\":");
            json::push_str(out, reason);
        }
    }
    out.push_str(",\"lanes\":");
    let lanes = lanes_json(out, &r.lanes)?;
    out.push_str(",\"curve\":");
    curve_json(out, &r.curve)?;
    out.push_str("}\n");
    Ok(lanes)
}

fn arm_link(out: &mut String, l: ArmLinkRecord) {
    match l {
        ArmLinkRecord::Road {
            road,
            road_ends_here,
        } => {
            out.push_str("{\"kind\":\"road\",\"road\":");
            json::push_hex(out, road);
            let _ = write!(out, ",\"ends_here\":{road_ends_here}}}");
        }
        ArmLinkRecord::Prefab(u) => {
            out.push_str("{\"kind\":\"prefab\",\"prefab\":");
            json::push_hex(out, u);
            out.push('}');
        }
        ArmLinkRecord::Item(u) => {
            out.push_str("{\"kind\":\"item\",\"item\":");
            json::push_hex(out, u);
            out.push('}');
        }
        ArmLinkRecord::Unresolved(u) => {
            out.push_str("{\"kind\":\"unresolved\",\"uid\":");
            json::push_hex(out, u);
            out.push('}');
        }
        ArmLinkRecord::None => out.push_str("{\"kind\":\"none\"}"),
    }
}

fn movement(out: &mut String, m: &MovementRecord) {
    let _ = write!(
        out,
        "{{\"from_arm\":{},\"from_slot\":{},\"to_arm\":{},\"to_slot\":{},\"status\":\"{}\"}}",
        m.from_arm,
        m.from_slot,
        m.to_arm,
        m.to_slot,
        m.status.name()
    );
}

fn junction_line(out: &mut String, j: &JunctionRecord) {
    out.push_str("{\"t\":\"junction\",\"id\":");
    json::push_hex(out, j.id);
    out.push_str(",\"model\":");
    json::push_str(out, &j.model);
    match &j.unresolved {
        None => out.push_str(",\"resolution\":\"resolved\""),
        Some(reason) => {
            out.push_str(",\"resolution\":\"unresolved\",\"resolution_reason\":");
            json::push_str(out, reason);
        }
    }
    out.push_str(",\"nodes\":[");
    for (i, n) in j.nodes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json::push_hex(out, *n);
    }
    out.push_str("],\"arms\":[");
    for (i, a) in j.arms.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "{{\"index\":{},\"node\":", a.index);
        json::push_hex(out, a.node);
        out.push_str(",\"link\":");
        arm_link(out, a.link);
        out.push('}');
    }
    out.push_str("],\"movements\":[");
    for (i, m) in j.movements.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        movement(out, m);
    }
    out.push_str("]}\n");
}

fn connection_line(out: &mut String, c: &ConnectionRecord) {
    out.push_str("{\"t\":\"connection\",\"kind\":\"road_road\",\"from\":");
    json::push_hex(out, c.from);
    out.push_str(",\"via\":");
    json::push_hex(out, c.via);
    out.push_str(",\"to\":");
    json::push_hex(out, c.to);
    let _ = write!(out, ",\"status\":\"{}\"", c.status.name());
    if let Some(reason) = &c.reason {
        out.push_str(",\"reason\":");
        json::push_str(out, reason);
    }
    if c.status == ConnectionStatus::Candidate || !c.pairs.is_empty() {
        out.push_str(",\"pairs\":[");
        for (i, ((fs, fi), (ts, ti))) in c.pairs.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(out, "[\"{}{fi}\",\"{}{ti}\"]", fs.letter(), ts.letter());
        }
        out.push(']');
    }
    out.push_str("}\n");
}

/// Writes the geometry file.
///
/// `counts` are declared in the header and must equal the number of records
/// the iterators yield. Records must come in strictly ascending id order
/// (connections in ascending `(from, to)` order), because the reader checks it.
///
/// # Errors
///
/// [`WriteError::Io`] if writing fails, [`WriteError::Invalid`] if a value is
/// not finite or the counts do not match. In both cases the output is
/// incomplete and has no footer.
pub fn write_geometry<W, N, R, J, C>(
    writer: W,
    counts: GeometryCounts,
    nodes: N,
    roads: R,
    junctions: J,
    connections: C,
) -> Result<WrittenFile, WriteError>
where
    W: Write,
    N: IntoIterator<Item = NodeRecord>,
    R: IntoIterator<Item = RoadRecord>,
    J: IntoIterator<Item = JunctionRecord>,
    C: IntoIterator<Item = ConnectionRecord>,
{
    let mut w = HashingWriter::new(BufWriter::with_capacity(1 << 20, writer));
    let mut line = String::with_capacity(1024);
    let _ = writeln!(
        line,
        "{{\"t\":\"header\",\"format\":\"{GEOMETRY_FORMAT_NAME}\",\"schema_version\":{{\"major\":{SCHEMA_MAJOR},\"minor\":{SCHEMA_MINOR}}},\"counts\":{{\"nodes\":{},\"roads\":{},\"junctions\":{},\"connections\":{}}}}}",
        counts.nodes, counts.roads, counts.junctions, counts.connections
    );
    w.write_all(line.as_bytes())?;

    let mismatch = |what: &str, found: u64, declared: u64| {
        WriteError::Invalid(format!("{what}: {declared} declared, {found} given"))
    };

    let mut n = 0u64;
    for node in nodes {
        line.clear();
        node_line(&mut line, &node)?;
        w.write_all(line.as_bytes())?;
        n += 1;
    }
    if n != counts.nodes {
        return Err(mismatch("nodes", n, counts.nodes));
    }

    let (mut r, mut lanes) = (0u64, 0u64);
    for road in roads {
        line.clear();
        lanes += road_line(&mut line, &road)?;
        w.write_all(line.as_bytes())?;
        r += 1;
    }
    if r != counts.roads {
        return Err(mismatch("roads", r, counts.roads));
    }

    let mut j = 0u64;
    for junction in junctions {
        line.clear();
        junction_line(&mut line, &junction);
        w.write_all(line.as_bytes())?;
        j += 1;
    }
    if j != counts.junctions {
        return Err(mismatch("junctions", j, counts.junctions));
    }

    let mut c = 0u64;
    for connection in connections {
        line.clear();
        connection_line(&mut line, &connection);
        w.write_all(line.as_bytes())?;
        c += 1;
    }
    if c != counts.connections {
        return Err(mismatch("connections", c, counts.connections));
    }

    line.clear();
    let _ = writeln!(
        line,
        "{{\"t\":\"footer\",\"complete\":true,\"nodes\":{n},\"roads\":{r},\"junctions\":{j},\"connections\":{c}}}"
    );
    w.write_all(line.as_bytes())?;
    w.flush()?;
    Ok(WrittenFile {
        counts,
        lanes,
        bytes: w.bytes(),
        checksum: w.checksum(),
    })
}
