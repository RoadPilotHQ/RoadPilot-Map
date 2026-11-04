//! Validation of road template geometry against measurements in the game.
//!
//! This module reads a small measurement file (format
//! `roadpilot-map-template-measurement`, specified in
//! `docs/validation/road-template-validation.md`), relates each observation
//! to a road of a map package, and checks the result against fixed
//! acceptance criteria.
//!
//! It **never** confirms a lane width or an axis position by itself and it
//! does not change any package output. The best verdict is
//! [`Verdict::CriteriaMetPendingReview`]: the numbers agree well enough that
//! a human review and an ADR may decide to model the measured templates.
//! Lane positions stay `unavailable` in the package until then.
//!
//! What is measured, transformed and assumed:
//!
//! - **Measured:** the raw telemetry placement (`x`, `y`, `z`, `heading`) at
//!   the moment an observer saw a tyre edge on a marking, and the observer's
//!   stated uncertainty.
//! - **Transformed:** the lateral offset of the placement from the road's
//!   estimated node curve (positive to the left of the stored road
//!   direction), and the travel direction from the heading (0 = north,
//!   counterclockwise, in turns, as documented by RoadPilot-Telemetry).
//! - **Assumed:** that telemetry world space and map world space are the same
//!   space and unit (checked only for consistency, criterion C4), and that the
//!   placement point is a fixed point of the truck. Its position relative to
//!   the tyres is **not** assumed: the four-way design (both tyres, both
//!   directions) cancels it.

use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::fmt;

use roadpilot_map_geometry::RoadCurve;
use roadpilot_map_model::{Position, RoadGeometry, Rotation};
use roadpilot_map_package::json::{self, Json};
use roadpilot_map_package::{MapPackage, NodeRecord};

use crate::Game;

/// The value of the `format` field.
pub const FORMAT_NAME: &str = "roadpilot-map-template-measurement";
/// Schema major version this module reads.
pub const SCHEMA_MAJOR: u32 = 1;

/// Longest measurement file accepted, bytes.
pub const MAX_FILE_LEN: usize = 4 << 20;
/// Most observations per file.
pub const MAX_OBSERVATIONS: usize = 10_000;

/// Travel direction relative to the stored road direction (start to end node).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Direction {
    Forward,
    Backward,
}

/// Which outer tyre edge was on the marking, seen in the travel direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tyre {
    Left,
    Right,
}

/// Which part of a marking the tyre edge was aligned with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MarkingPart {
    Centre,
    InnerEdge,
    OuterEdge,
}

impl MarkingPart {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "centre" => Some(Self::Centre),
            "inner_edge" => Some(Self::InnerEdge),
            "outer_edge" => Some(Self::OuterEdge),
            _ => None,
        }
    }
}

/// Raw telemetry placement, unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// Turns, 0 = north, counterclockwise seen from above.
    pub heading: f64,
}

/// One observation.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// Pass number (a new approach to the measuring point).
    pub pass: u32,
    pub direction: Direction,
    pub tyre: Tyre,
    /// Marking id chosen by the protocol, for example `centre` or `edge_right`.
    pub marking: String,
    pub part: MarkingPart,
    pub placement: Placement,
    /// Observer's standard uncertainty of the alignment, metres.
    pub uncertainty_m: f64,
    /// Measuring point id along the road (schema 1.1), for example `p1`.
    pub point: Option<String>,
    /// The telemetry frame the placement was copied from (schema 1.1).
    pub frame: Option<FrameRef>,
    /// Raw telemetry pitch and roll, turns (schema 1.1).
    pub pitch: Option<f64>,
    pub roll: Option<f64>,
    /// Truck speed of the frame, m/s (schema 1.1). Above
    /// [`MAX_STANDING_SPEED_MPS`] the observation is not used.
    pub speed_mps: Option<f64>,
    /// Whether the observer saw the tyre edge on the marking part (schema
    /// 1.1; `true` when absent). `false` keeps the attempt on record, but it
    /// is not used.
    pub criterion_met: bool,
    /// Short deviation or review note. Never personal data.
    pub notes: Option<String>,
}

/// Reference to one telemetry frame: the plugin session and the frame
/// sequence number, both from RoadPilot-Telemetry schema 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrameRef {
    pub session: u64,
    pub sequence: u64,
}

/// Largest speed at which the truck counts as standing still, m/s.
pub const MAX_STANDING_SPEED_MPS: f64 = 0.05;

/// Schema minor version this module writes.
pub const SCHEMA_MINOR: u32 = 1;

/// Provenance and setup of one measurement session on one road.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasurementHeader {
    pub game: Game,
    /// `gameVersion` from the telemetry (SDK version, not the patch).
    pub telemetry_game_version: Option<String>,
    /// The game patch as shown by the game, entered by the observer.
    pub game_patch: Option<String>,
    /// `content_id` of the map package used to identify the road.
    pub package_content_id: Option<u64>,
    /// Template path, for example `road_template/sc/road_1_scan.pmd`.
    pub template: String,
    /// Road UID.
    pub road: u64,
    /// Road look name.
    pub look: String,
    /// Template variant if the observer identified it; otherwise unknown.
    pub variant: Option<String>,
    /// Observation method; only `tyre_edge_alignment` is defined.
    pub method: String,
    /// `true` for invented example data. Synthetic sets never count for the
    /// acceptance criteria.
    pub synthetic: bool,
    /// Road section id chosen by the observer (schema 1.1), for example `s1`.
    pub section: Option<String>,
    /// Whether a trailer was attached (schema 1.1). The protocol asks for no
    /// trailer.
    pub trailer: Option<bool>,
    /// `localScale` of the telemetry (schema 1.1).
    pub local_scale: Option<f64>,
    /// Short session or review note. Never personal data.
    pub notes: Option<String>,
}

/// A parsed measurement file.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasurementSet {
    pub header: MeasurementHeader,
    pub observations: Vec<Observation>,
    /// Template marking positions to compare with (schema 1.1). Always
    /// *derived* from the template study, never measured.
    pub candidates: Vec<TemplateCandidate>,
}

/// Why a measurement file was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MeasurementError {
    TooLarge,
    /// A line is invalid; `line` counts from 1.
    Line {
        line: usize,
        message: String,
    },
    UnsupportedVersion {
        major: i64,
    },
    /// The file ends without the footer, or the counts disagree.
    Incomplete(String),
}

impl fmt::Display for MeasurementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => f.write_str("the measurement file is too large"),
            Self::Line { line, message } => write!(f, "line {line}: {message}"),
            Self::UnsupportedVersion { major } => write!(
                f,
                "unsupported measurement schema {major} (supported: {SCHEMA_MAJOR})"
            ),
            Self::Incomplete(m) => write!(f, "incomplete measurement file: {m}"),
        }
    }
}

impl std::error::Error for MeasurementError {}

type Fail = String;

fn req<'a>(v: &'a Json, key: &str) -> Result<&'a Json, Fail> {
    v.get(key).ok_or_else(|| format!("missing field \"{key}\""))
}

fn string(v: &Json, key: &str) -> Result<String, Fail> {
    req(v, key)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("\"{key}\" must be a string"))
}

fn opt_string(v: &Json, key: &str) -> Result<Option<String>, Fail> {
    match req(v, key)? {
        Json::Null => Ok(None),
        Json::Str(s) => Ok(Some(s.clone())),
        _ => Err(format!("\"{key}\" must be a string or null")),
    }
}

fn number(v: &Json, key: &str) -> Result<f64, Fail> {
    req(v, key)?
        .as_f64()
        .filter(|f| f.is_finite())
        .ok_or_else(|| format!("\"{key}\" must be a finite number"))
}

fn hex(s: &str) -> Result<u64, Fail> {
    if s.is_empty()
        || s.len() > 16
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(format!("{s:?} is not a lower-case hexadecimal id"));
    }
    u64::from_str_radix(s, 16).map_err(|e| e.to_string())
}

/// An optional field: absent and `null` are both `None`.
fn optional<'a>(v: &'a Json, key: &str) -> Option<&'a Json> {
    v.get(key).filter(|j| !matches!(j, Json::Null))
}

fn opt_number(v: &Json, key: &str) -> Result<Option<f64>, Fail> {
    optional(v, key)
        .map(|j| {
            j.as_f64()
                .filter(|f| f.is_finite())
                .ok_or_else(|| format!("\"{key}\" must be a finite number"))
        })
        .transpose()
}

fn opt_bool(v: &Json, key: &str) -> Result<Option<bool>, Fail> {
    optional(v, key)
        .map(|j| {
            j.as_bool()
                .ok_or_else(|| format!("\"{key}\" must be a boolean"))
        })
        .transpose()
}

/// A short lower-case id (`a-z`, `0-9`, `_`, at most 64 bytes).
pub fn is_valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn opt_id(v: &Json, key: &str) -> Result<Option<String>, Fail> {
    match optional(v, key) {
        None => Ok(None),
        Some(Json::Str(s)) if is_valid_id(s) => Ok(Some(s.clone())),
        Some(_) => Err(format!("\"{key}\" must be a short lower-case id")),
    }
}

/// Longest note accepted, characters.
pub const MAX_NOTE_CHARS: usize = 200;

/// Checks a free-text note. Notes may describe a deviation ("left tyre
/// slightly over the line") or a review remark. To keep personal and
/// machine data out, they are short, single-line, and may not contain path
/// separators, `@`, URLs or control characters.
pub fn check_note(s: &str) -> Result<(), &'static str> {
    if s.chars().count() > MAX_NOTE_CHARS {
        return Err("a note is longer than 200 characters");
    }
    if s.chars()
        .any(|c| c.is_control() || matches!(c, '/' | '\\' | '@' | '"' | ',' | '|'))
    {
        return Err("a note may not contain control characters, / \\ @ \" , or |");
    }
    let lower = s.to_lowercase();
    if lower.contains("http") || lower.contains("www.") {
        return Err("a note may not contain a link");
    }
    Ok(())
}

fn opt_note(v: &Json, key: &str) -> Result<Option<String>, Fail> {
    match optional(v, key) {
        None => Ok(None),
        Some(Json::Str(s)) => {
            check_note(s).map_err(str::to_owned)?;
            Ok(Some(s.clone()))
        }
        Some(_) => Err(format!("\"{key}\" must be a string")),
    }
}

fn header_record(v: &Json) -> Result<MeasurementHeader, Fail> {
    let game = Game::parse(&string(v, "game")?).ok_or("\"game\" must be ets2 or ats")?;
    let method = string(v, "method")?;
    if method != "tyre_edge_alignment" {
        return Err(format!("unknown method {method:?}"));
    }
    let template = string(v, "template")?;
    if template.is_empty() || template.len() > 256 || !template.ends_with(".pmd") {
        return Err("\"template\" must be the path of a .pmd file".to_owned());
    }
    Ok(MeasurementHeader {
        game,
        telemetry_game_version: opt_string(v, "telemetry_game_version")?,
        game_patch: opt_string(v, "game_patch")?,
        package_content_id: opt_string(v, "package_content_id")?
            .map(|s| hex(&s))
            .transpose()?,
        template,
        road: hex(&string(v, "road")?)?,
        look: string(v, "look")?,
        variant: opt_string(v, "variant")?,
        method,
        synthetic: req(v, "synthetic")?
            .as_bool()
            .ok_or("\"synthetic\" must be a boolean")?,
        section: opt_id(v, "section")?,
        trailer: opt_bool(v, "trailer")?,
        local_scale: opt_number(v, "local_scale")?,
        notes: opt_note(v, "notes")?,
    })
}

fn candidate_record(v: &Json) -> Result<TemplateCandidate, Fail> {
    if string(v, "quality")? != "derived" {
        return Err("a template candidate must have quality \"derived\"".to_owned());
    }
    let marking = string(v, "marking")?;
    if !is_valid_id(&marking) {
        return Err("\"marking\" must be a short lower-case id".to_owned());
    }
    Ok(TemplateCandidate {
        marking,
        part: MarkingPart::parse(&string(v, "marking_part")?)
            .ok_or("\"marking_part\" must be centre, inner_edge or outer_edge")?,
        x: number(v, "template_x")?,
    })
}

fn observation_record(v: &Json) -> Result<Observation, Fail> {
    let p = req(v, "placement")?;
    let placement = Placement {
        x: number(p, "x")?,
        y: number(p, "y")?,
        z: number(p, "z")?,
        heading: number(p, "heading")?,
    };
    if !(0.0..1.0).contains(&placement.heading) {
        return Err("heading must be in [0, 1) turns".to_owned());
    }
    let uncertainty_m = number(v, "uncertainty_m")?;
    if !(uncertainty_m > 0.0 && uncertainty_m <= 1.0) {
        return Err("uncertainty_m must be in (0, 1] metres".to_owned());
    }
    let marking = string(v, "marking")?;
    if marking.is_empty()
        || marking.len() > 64
        || !marking
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err("\"marking\" must be a short lower-case id".to_owned());
    }
    Ok(Observation {
        pass: u32::try_from(
            req(v, "pass")?
                .as_i64()
                .ok_or("\"pass\" must be an integer")?,
        )
        .map_err(|_| "\"pass\" must be a non-negative integer".to_owned())?,
        direction: match string(v, "direction")?.as_str() {
            "forward" => Direction::Forward,
            "backward" => Direction::Backward,
            _ => return Err("\"direction\" must be forward or backward".to_owned()),
        },
        tyre: match string(v, "tyre")?.as_str() {
            "left" => Tyre::Left,
            "right" => Tyre::Right,
            _ => return Err("\"tyre\" must be left or right".to_owned()),
        },
        marking,
        part: MarkingPart::parse(&string(v, "marking_part")?)
            .ok_or("\"marking_part\" must be centre, inner_edge or outer_edge")?,
        placement,
        uncertainty_m,
        point: opt_id(v, "point")?,
        frame: match optional(v, "frame") {
            None => None,
            Some(f) => {
                let int = |k: &str| {
                    f.get(k)
                        .and_then(Json::as_i64)
                        .and_then(|i| u64::try_from(i).ok())
                        .ok_or_else(|| format!("frame.{k} must be a non-negative integer"))
                };
                Some(FrameRef {
                    session: int("session")?,
                    sequence: int("sequence")?,
                })
            }
        },
        pitch: opt_number(p, "pitch")?,
        roll: opt_number(p, "roll")?,
        speed_mps: opt_number(v, "speed_mps")?,
        criterion_met: opt_bool(v, "criterion_met")?.unwrap_or(true),
        notes: opt_note(v, "notes")?,
    })
}

/// Reads and checks a measurement file.
///
/// # Errors
///
/// [`MeasurementError`] for a file that is too large, malformed, of another
/// major version, or without a matching footer.
pub fn read_measurements(text: &str) -> Result<MeasurementSet, MeasurementError> {
    if text.len() > MAX_FILE_LEN {
        return Err(MeasurementError::TooLarge);
    }
    let mut header = None;
    let mut observations: Vec<Observation> = Vec::new();
    let mut candidates: Vec<TemplateCandidate> = Vec::new();
    let mut done = false;
    for (i, line) in text.lines().enumerate() {
        let at = |message: String| MeasurementError::Line {
            line: i + 1,
            message,
        };
        if line.trim().is_empty() {
            continue;
        }
        if done {
            return Err(at("data after the footer".to_owned()));
        }
        let v = json::parse(line).map_err(at)?;
        match v.get("t").and_then(Json::as_str) {
            Some("header") if header.is_none() && i == 0 => {
                if v.get("format").and_then(Json::as_str) != Some(FORMAT_NAME) {
                    return Err(at(format!("the format must be {FORMAT_NAME}")));
                }
                let major = v
                    .get("schema_version")
                    .and_then(|s| s.get("major"))
                    .and_then(Json::as_i64)
                    .unwrap_or(-1);
                if major != i64::from(SCHEMA_MAJOR) {
                    return Err(MeasurementError::UnsupportedVersion { major });
                }
                header = Some(header_record(&v).map_err(at)?);
            }
            Some("observation") if header.is_some() => {
                if observations.len() >= MAX_OBSERVATIONS {
                    return Err(at("too many observations".to_owned()));
                }
                let o = observation_record(&v).map_err(at)?;
                if let Some(f) = o.frame {
                    if observations.iter().any(|p| p.frame == Some(f)) {
                        return Err(at(format!(
                            "frame {}/{} is used by two observations",
                            f.session, f.sequence
                        )));
                    }
                }
                observations.push(o);
            }
            Some("candidate") if header.is_some() => {
                let c = candidate_record(&v).map_err(at)?;
                if candidates
                    .iter()
                    .any(|o: &TemplateCandidate| o.marking == c.marking && o.part == c.part)
                {
                    return Err(at(
                        "a candidate for this marking part is listed twice".to_owned()
                    ));
                }
                candidates.push(c);
            }
            Some("footer") if header.is_some() => {
                let n = v.get("observations").and_then(Json::as_i64);
                let nc = v.get("candidates").and_then(Json::as_i64);
                if v.get("complete").and_then(Json::as_bool) != Some(true)
                    || n != Some(observations.len() as i64)
                    || nc.is_some_and(|c| c != candidates.len() as i64)
                {
                    return Err(MeasurementError::Incomplete(
                        "the footer does not match the observations".to_owned(),
                    ));
                }
                done = true;
            }
            Some(_) | None if header.is_none() => {
                return Err(at("the first line must be the header".to_owned()));
            }
            // Unknown record types are ignored (minor versions).
            Some(_) => {}
            None => return Err(at("missing record type \"t\"".to_owned())),
        }
    }
    let header = header.ok_or_else(|| MeasurementError::Incomplete("no header".to_owned()))?;
    if !done {
        return Err(MeasurementError::Incomplete(
            "the file ends before the footer".to_owned(),
        ));
    }
    Ok(MeasurementSet {
        header,
        observations,
        candidates,
    })
}

fn push_opt_str(out: &mut String, v: Option<&str>) {
    match v {
        Some(s) => json::push_str(out, s),
        None => out.push_str("null"),
    }
}

fn push_num(out: &mut String, v: f64) -> Result<(), String> {
    if json::push_f64(out, v) {
        Ok(())
    } else {
        Err("a number is not finite".to_owned())
    }
}

fn part_name(p: MarkingPart) -> &'static str {
    match p {
        MarkingPart::Centre => "centre",
        MarkingPart::InnerEdge => "inner_edge",
        MarkingPart::OuterEdge => "outer_edge",
    }
}

/// Writes a measurement set as schema 1.1 JSON Lines. The output is
/// deterministic and reads back unchanged with [`read_measurements`].
///
/// # Errors
///
/// A message if a value cannot be written (a number that is not finite, or
/// an id or note that the reader would reject).
pub fn write_measurements(set: &MeasurementSet) -> Result<String, String> {
    use std::fmt::Write as _;
    let h = &set.header;
    let mut o = String::new();
    let _ = write!(
        o,
        "{{\"t\":\"header\",\"format\":\"{FORMAT_NAME}\",\"schema_version\":{{\"major\":{SCHEMA_MAJOR},\"minor\":{SCHEMA_MINOR}}},\"game\":\"{}\",\"telemetry_game_version\":",
        h.game.id()
    );
    push_opt_str(&mut o, h.telemetry_game_version.as_deref());
    o.push_str(",\"game_patch\":");
    push_opt_str(&mut o, h.game_patch.as_deref());
    o.push_str(",\"package_content_id\":");
    match h.package_content_id {
        Some(c) => json::push_hex(&mut o, c),
        None => o.push_str("null"),
    }
    o.push_str(",\"template\":");
    json::push_str(&mut o, &h.template);
    o.push_str(",\"road\":");
    json::push_hex(&mut o, h.road);
    o.push_str(",\"look\":");
    json::push_str(&mut o, &h.look);
    o.push_str(",\"variant\":");
    push_opt_str(&mut o, h.variant.as_deref());
    o.push_str(",\"method\":");
    json::push_str(&mut o, &h.method);
    let _ = write!(o, ",\"synthetic\":{},\"section\":", h.synthetic);
    push_opt_str(&mut o, h.section.as_deref());
    o.push_str(",\"trailer\":");
    match h.trailer {
        Some(b) => {
            let _ = write!(o, "{b}");
        }
        None => o.push_str("null"),
    }
    o.push_str(",\"local_scale\":");
    match h.local_scale {
        Some(v) => push_num(&mut o, v)?,
        None => o.push_str("null"),
    }
    o.push_str(",\"notes\":");
    push_opt_str(&mut o, h.notes.as_deref());
    o.push_str("}\n");

    for c in &set.candidates {
        o.push_str("{\"t\":\"candidate\",\"marking\":");
        json::push_str(&mut o, &c.marking);
        let _ = write!(
            o,
            ",\"marking_part\":\"{}\",\"template_x\":",
            part_name(c.part)
        );
        push_num(&mut o, c.x)?;
        o.push_str(",\"quality\":\"derived\"}\n");
    }
    for ob in &set.observations {
        let _ = write!(
            o,
            "{{\"t\":\"observation\",\"pass\":{},\"direction\":\"{}\",\"tyre\":\"{}\",\"marking\":",
            ob.pass,
            match ob.direction {
                Direction::Forward => "forward",
                Direction::Backward => "backward",
            },
            match ob.tyre {
                Tyre::Left => "left",
                Tyre::Right => "right",
            }
        );
        json::push_str(&mut o, &ob.marking);
        let _ = write!(o, ",\"marking_part\":\"{}\",\"point\":", part_name(ob.part));
        push_opt_str(&mut o, ob.point.as_deref());
        o.push_str(",\"frame\":");
        match ob.frame {
            Some(f) => {
                let _ = write!(
                    o,
                    "{{\"session\":{},\"sequence\":{}}}",
                    f.session, f.sequence
                );
            }
            None => o.push_str("null"),
        }
        let p = ob.placement;
        o.push_str(",\"placement\":{\"x\":");
        push_num(&mut o, p.x)?;
        o.push_str(",\"y\":");
        push_num(&mut o, p.y)?;
        o.push_str(",\"z\":");
        push_num(&mut o, p.z)?;
        o.push_str(",\"heading\":");
        push_num(&mut o, p.heading)?;
        for (k, v) in [("pitch", ob.pitch), ("roll", ob.roll)] {
            if let Some(v) = v {
                let _ = write!(o, ",\"{k}\":");
                push_num(&mut o, v)?;
            }
        }
        o.push_str("},\"speed_mps\":");
        match ob.speed_mps {
            Some(v) => push_num(&mut o, v)?,
            None => o.push_str("null"),
        }
        let _ = write!(
            o,
            ",\"criterion_met\":{},\"uncertainty_m\":",
            ob.criterion_met
        );
        push_num(&mut o, ob.uncertainty_m)?;
        o.push_str(",\"notes\":");
        push_opt_str(&mut o, ob.notes.as_deref());
        o.push_str("}\n");
    }
    let _ = writeln!(
        o,
        "{{\"t\":\"footer\",\"complete\":true,\"observations\":{},\"candidates\":{}}}",
        set.observations.len(),
        set.candidates.len()
    );
    // Never write what the reader would reject.
    read_measurements(&o).map_err(|e| e.to_string())?;
    Ok(o)
}

// --- Geometry ----------------------------------------------------------------

fn node_geometry(n: &NodeRecord) -> (Position, Rotation) {
    (
        Position { fixed: n.position },
        Rotation {
            w: n.rotation[0],
            x: n.rotation[1],
            y: n.rotation[2],
            z: n.rotation[3],
        },
    )
}

/// The estimated curve of a road of a package, or `None` if the road or its
/// nodes are missing or no curve can be estimated.
#[must_use]
pub fn road_curve(package: &MapPackage, road: u64) -> Option<RoadCurve> {
    let r = package.road(road)?;
    let (sp, sr) = node_geometry(package.node(r.start)?);
    let (ep, er) = node_geometry(package.node(r.end)?);
    let (a, b) = (sp.world(), ep.world());
    let chord = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt();
    let geometry = RoadGeometry {
        start: a,
        end: b,
        start_direction: sr.forward(),
        end_direction: er.forward(),
        chord_length: chord,
        stored_length: f64::from(r.stored_length),
    };
    RoadCurve::estimate(&geometry).ok()
}

/// The horizontal travel direction `(x, z)` for a telemetry heading.
/// Transformation from the documented convention: 0 = north (−Z), 0.25 =
/// west (−X), counterclockwise.
#[must_use]
pub fn heading_direction(heading: f64) -> [f64; 2] {
    let a = heading * TAU;
    [-a.sin(), -a.cos()]
}

/// A placement related to a road: lateral offset (positive left of the
/// stored road direction) and the angle between travel and road direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projected {
    pub lateral_m: f64,
    pub heading_error_deg: f64,
    pub t: f64,
}

/// Projects a placement onto the road curve.
#[must_use]
pub fn project(curve: &RoadCurve, p: &Placement, direction: Direction) -> Projected {
    let hit = curve.project([p.x, p.y, p.z]);
    let d = curve.derivative(hit.t);
    let h = (d[0] * d[0] + d[2] * d[2]).sqrt().max(1e-12);
    let (tx, tz) = (d[0] / h, d[2] / h);
    // Left of the direction (tx, tz) in X east, Z south is (tz, -tx).
    let left = [tz, -tx];
    let lateral_m = (p.x - hit.point[0]) * left[0] + (p.z - hit.point[2]) * left[1];
    let (ex, ez) = match direction {
        Direction::Forward => (tx, tz),
        Direction::Backward => (-tx, -tz),
    };
    let f = heading_direction(p.heading);
    let cos = (f[0] * ex + f[1] * ez).clamp(-1.0, 1.0);
    Projected {
        lateral_m,
        heading_error_deg: cos.acos().to_degrees(),
        t: hit.t,
    }
}

// --- Evaluation --------------------------------------------------------------

/// A marking position read from a template, to be compared. Its status is
/// *derived* (from the template study), never confirmed.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateCandidate {
    pub marking: String,
    pub part: MarkingPart,
    /// Template x of the marking part.
    pub x: f64,
}

/// Statistics of one of the four alignment kinds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KindStats {
    pub direction: Direction,
    pub tyre: Tyre,
    pub n: usize,
    /// Mean lateral offset of the placement, metres.
    pub mean_m: f64,
    /// Sample standard deviation, metres (0 for n = 1).
    pub std_m: f64,
    /// Mean of the observer uncertainties, metres.
    pub observer_m: f64,
}

/// Result for one marking part of one road.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkingResult {
    pub marking: String,
    pub part: MarkingPart,
    /// The four kinds in the order forward-left, forward-right,
    /// backward-left, backward-right; missing kinds are absent.
    pub kinds: Vec<KindStats>,
    /// Lateral position of the marking relative to the node curve, if all
    /// four kinds are present: the mean of the four kind means. Independent
    /// of where the placement point sits on the truck.
    pub line_m: Option<f64>,
    /// Combined standard uncertainty of `line_m`.
    pub line_uncertainty_m: Option<f64>,
    /// Outer tyre width estimated from the forward and from the backward
    /// passes; they must agree.
    pub width_estimates_m: Option<(f64, f64)>,
    /// Lateral offset of the placement point from the tyre centre, estimated
    /// from the forward and from the backward passes; they must agree.
    pub reference_offset_estimates_m: Option<(f64, f64)>,
    pub candidate_x: Option<f64>,
    /// Residual `line − (−x)`: template x grows to the right of the stored
    /// road direction.
    pub residual_x_right_m: Option<f64>,
    /// Residual `line − x`: template x grows to the left.
    pub residual_x_left_m: Option<f64>,
}

/// The evaluation of one measurement set.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadReport {
    pub header: MeasurementHeader,
    pub results: Vec<MarkingResult>,
    pub max_heading_error_deg: f64,
    pub max_turn_deg: f64,
    pub length_consistent: bool,
    /// Observations not used: criterion not met, or the truck was moving.
    pub excluded_observations: usize,
}

fn stats(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let var = if values.len() > 1 {
        values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0)
    } else {
        0.0
    };
    (mean, var.sqrt())
}

/// Lateral offsets and observer uncertainties per alignment kind.
type KindSamples = BTreeMap<(Direction, Tyre), Vec<(f64, f64)>>;

/// Evaluates one measurement set against a road curve and template
/// candidates.
#[must_use]
pub fn evaluate(
    set: &MeasurementSet,
    curve: &RoadCurve,
    candidates: &[TemplateCandidate],
) -> RoadReport {
    let mut groups: BTreeMap<(String, MarkingPart), KindSamples> = BTreeMap::new();
    let mut max_heading: f64 = 0.0;
    for o in set.observations.iter().filter(|o| is_usable(o)) {
        let p = project(curve, &o.placement, o.direction);
        max_heading = max_heading.max(p.heading_error_deg);
        groups
            .entry((o.marking.clone(), o.part))
            .or_default()
            .entry((o.direction, o.tyre))
            .or_default()
            .push((p.lateral_m, o.uncertainty_m));
    }
    let order = [
        (Direction::Forward, Tyre::Left),
        (Direction::Forward, Tyre::Right),
        (Direction::Backward, Tyre::Left),
        (Direction::Backward, Tyre::Right),
    ];
    let mut results = Vec::new();
    for ((marking, part), kinds) in groups {
        let stats_of: Vec<KindStats> = order
            .iter()
            .filter_map(|k| {
                let v = kinds.get(k)?;
                let lat: Vec<f64> = v.iter().map(|x| x.0).collect();
                let (mean_m, std_m) = stats(&lat);
                Some(KindStats {
                    direction: k.0,
                    tyre: k.1,
                    n: v.len(),
                    mean_m,
                    std_m,
                    observer_m: v.iter().map(|x| x.1).sum::<f64>() / v.len() as f64,
                })
            })
            .collect();
        let complete = stats_of.len() == 4;
        let (line_m, line_uncertainty_m, width, offset) = if complete {
            let m: Vec<f64> = stats_of.iter().map(|k| k.mean_m).collect();
            let line = m.iter().sum::<f64>() / 4.0;
            let u = (stats_of
                .iter()
                .map(|k| k.std_m.powi(2) / k.n as f64 + k.observer_m.powi(2))
                .sum::<f64>())
            .sqrt()
                / 4.0;
            // Forward: pFL = line − eL, pFR = line − eR → width = pFR − pFL.
            // Backward: pBL = line + eL, pBR = line + eR → width = pBL − pBR.
            let width = (m[1] - m[0], m[2] - m[3]);
            let offset = (line - (m[0] + m[1]) / 2.0, (m[2] + m[3]) / 2.0 - line);
            (Some(line), Some(u), Some(width), Some(offset))
        } else {
            (None, None, None, None)
        };
        let candidate_x = candidates
            .iter()
            .find(|c| c.marking == marking && c.part == part)
            .map(|c| c.x);
        let residual = |sign: f64| line_m.zip(candidate_x).map(|(l, x)| l - sign * x);
        results.push(MarkingResult {
            marking,
            part,
            kinds: stats_of,
            line_m,
            line_uncertainty_m,
            width_estimates_m: width,
            reference_offset_estimates_m: offset,
            candidate_x,
            residual_x_right_m: residual(-1.0),
            residual_x_left_m: residual(1.0),
        });
    }
    RoadReport {
        header: set.header.clone(),
        results,
        max_heading_error_deg: max_heading,
        max_turn_deg: curve.turn_angle().to_degrees(),
        length_consistent: curve.length_agreement().is_consistent(),
        excluded_observations: set.observations.len()
            - set.observations.iter().filter(|o| is_usable(o)).count(),
    }
}

// --- Acceptance --------------------------------------------------------------

/// Limits of the acceptance criteria. The defaults are the values of the
/// specification; changing them needs a documented reason.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Acceptance {
    pub min_templates_per_game: usize,
    pub min_passes_per_kind: usize,
    pub max_kind_std_m: f64,
    pub max_line_uncertainty_m: f64,
    pub max_residual_m: f64,
    pub max_heading_error_deg: f64,
    pub max_turn_deg: f64,
    pub max_vehicle_disagreement_m: f64,
}

impl Default for Acceptance {
    fn default() -> Self {
        Self {
            min_templates_per_game: 3,
            min_passes_per_kind: 3,
            max_kind_std_m: 0.10,
            max_line_uncertainty_m: 0.05,
            max_residual_m: 0.15,
            max_heading_error_deg: 2.0,
            max_turn_deg: 1.0,
            max_vehicle_disagreement_m: 0.10,
        }
    }
}

/// Outcome of the acceptance check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Not enough independent, complete, real data.
    InsufficientData,
    /// Enough data, but it does not agree with the template or with itself.
    Inconsistent,
    /// Every criterion holds. A human review and an ADR must still decide;
    /// nothing is confirmed by this verdict alone.
    CriteriaMetPendingReview,
}

/// Which way template x grows, if one hypothesis fits every measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XDirection {
    Right,
    Left,
}

/// One checked criterion.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub id: &'static str,
    pub passed: bool,
    pub detail: String,
}

/// The result of [`assess`].
#[derive(Debug, Clone, PartialEq)]
pub struct Assessment {
    pub verdict: Verdict,
    pub checks: Vec<Check>,
    pub x_direction: Option<XDirection>,
}

/// Checks reports against the acceptance criteria C0–C7 of the
/// specification.
#[must_use]
pub fn assess(reports: &[RoadReport], a: &Acceptance) -> Assessment {
    let real: Vec<&RoadReport> = reports.iter().filter(|r| !r.header.synthetic).collect();
    let mut checks = Vec::new();
    let mut check = |id, passed, detail: String| checks.push(Check { id, passed, detail });

    let synthetic = reports.len() - real.len();
    check(
        "C0",
        !real.is_empty(),
        format!(
            "{} real sets, {synthetic} synthetic sets ignored",
            real.len()
        ),
    );

    let provenance = real.iter().all(|r| {
        r.header.telemetry_game_version.is_some()
            && r.header.game_patch.is_some()
            && r.header.package_content_id.is_some()
    });
    check(
        "C1",
        provenance,
        "every set records telemetry game version, game patch and package content id".to_owned(),
    );

    let complete = |r: &&RoadReport| {
        !r.results.is_empty()
            && r.results.iter().all(|m| {
                m.candidate_x.is_some()
                    && m.kinds.len() == 4
                    && m.kinds.iter().all(|k| k.n >= a.min_passes_per_kind)
            })
    };
    let mut per_game: BTreeMap<&str, std::collections::BTreeSet<&str>> = BTreeMap::new();
    for r in real.iter().filter(|r| complete(r)) {
        per_game
            .entry(r.header.game.id())
            .or_default()
            .insert(&r.header.template);
    }
    let coverage = ["ets2", "ats"]
        .iter()
        .all(|g| per_game.get(g).map_or(0, |s| s.len()) >= a.min_templates_per_game);
    check(
        "C2",
        coverage,
        format!(
            "complete sets per game (templates): ets2 {}, ats {}; needed {} each, every marking with all four kinds and at least {} passes",
            per_game.get("ets2").map_or(0, |s| s.len()),
            per_game.get("ats").map_or(0, |s| s.len()),
            a.min_templates_per_game,
            a.min_passes_per_kind
        ),
    );

    let all_results = || real.iter().flat_map(|r| r.results.iter());
    let repeat = all_results().all(|m| m.kinds.iter().all(|k| k.std_m <= a.max_kind_std_m))
        && all_results().all(|m| {
            m.line_uncertainty_m
                .is_some_and(|u| u <= a.max_line_uncertainty_m)
        });
    check(
        "C3",
        repeat,
        format!(
            "spread per kind ≤ {} m and combined uncertainty of every line ≤ {} m",
            a.max_kind_std_m, a.max_line_uncertainty_m
        ),
    );

    let frame = real.iter().all(|r| {
        r.max_heading_error_deg <= a.max_heading_error_deg
            && r.max_turn_deg <= a.max_turn_deg
            && r.length_consistent
    });
    check(
        "C4",
        frame,
        format!(
            "straight roads (turn ≤ {}°, consistent length) and heading within {}° of the road",
            a.max_turn_deg, a.max_heading_error_deg
        ),
    );

    let vehicle = all_results().all(|m| {
        m.width_estimates_m
            .zip(m.reference_offset_estimates_m)
            .is_some_and(|((w1, w2), (o1, o2))| {
                (w1 - w2).abs() <= a.max_vehicle_disagreement_m
                    && (o1 - o2).abs() <= a.max_vehicle_disagreement_m
            })
    });
    check(
        "C5",
        vehicle,
        "tyre width and placement offset from forward and backward passes agree".to_owned(),
    );

    let fits = |pick: fn(&MarkingResult) -> Option<f64>| {
        let mut any = false;
        let ok = all_results().all(|m| {
            any = true;
            pick(m).is_some_and(|r| r.abs() <= a.max_residual_m)
        });
        any && ok
    };
    let right = fits(|m| m.residual_x_right_m);
    let left = fits(|m| m.residual_x_left_m);
    let x_direction = match (right, left) {
        (true, false) => Some(XDirection::Right),
        (false, true) => Some(XDirection::Left),
        _ => None,
    };
    check(
        "C6",
        x_direction.is_some(),
        format!(
            "one x direction fits every marking of both games within {} m (right: {right}, left: {left})",
            a.max_residual_m
        ),
    );

    check(
        "C7",
        false,
        "independent human review and an ADR are required; never met automatically".to_owned(),
    );

    let get = |id: &str| checks.iter().any(|c| c.id == id && c.passed);
    let verdict = if !(get("C0") && get("C1") && get("C2")) {
        Verdict::InsufficientData
    } else if !(get("C3") && get("C4") && get("C5") && get("C6")) {
        Verdict::Inconsistent
    } else {
        Verdict::CriteriaMetPendingReview
    };
    Assessment {
        verdict,
        checks,
        x_direction,
    }
}

/// Whether an observation is used: the observer saw the criterion and the
/// truck stood still (if the speed was recorded).
#[must_use]
pub fn is_usable(o: &Observation) -> bool {
    o.criterion_met
        && o.speed_mps
            .is_none_or(|v| v.abs() <= MAX_STANDING_SPEED_MPS)
}
