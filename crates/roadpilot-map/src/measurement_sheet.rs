//! The capture sheet for in-game template measurements.
//!
//! A sheet is a plain CSV file that an observer fills in during a session
//! (in a spreadsheet or a text editor) and that converts directly into the
//! `roadpilot-map-template-measurement` JSON Lines format
//! ([`crate::validation`]). The layout is specified in
//! `docs/validation/measurement-sheet.md`.
//!
//! One sheet is one session: one game, one road, one template. Rows have a
//! `record` type: one `session` row, `candidate` rows (template marking
//! positions, *derived*), and `observation` rows. Observation rows whose
//! placement is still empty are **planned** and are skipped on conversion,
//! so a half-filled sheet converts.
//!
//! The sheet never contains measured values that it did not receive: a blank
//! sheet from [`blank_sheet`] has only the plan.

use std::fmt;

use crate::Game;
use crate::validation::{
    Direction, FrameRef, MarkingPart, MeasurementHeader, MeasurementSet, Observation, Placement,
    TemplateCandidate, Tyre, check_note, is_valid_id,
};

/// The columns of a sheet, in this order.
pub const SHEET_COLUMNS: [&str; 32] = [
    "record",
    "game",
    "game_patch",
    "telemetry_game_version",
    "package_content_id",
    "template",
    "road",
    "look",
    "variant",
    "section",
    "trailer",
    "local_scale",
    "synthetic",
    "marking",
    "marking_part",
    "template_x",
    "point",
    "pass",
    "direction",
    "tyre",
    "criterion_met",
    "frame_session",
    "frame_sequence",
    "x",
    "y",
    "z",
    "heading",
    "pitch",
    "roll",
    "speed_mps",
    "uncertainty_m",
    "notes",
];

/// Longest sheet accepted, bytes.
pub const MAX_SHEET_LEN: usize = 4 << 20;

/// Why a sheet could not be converted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetError {
    /// Line in the sheet, counting from 1 (0 for the whole sheet).
    pub line: usize,
    pub message: String,
}

impl fmt::Display for SheetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "sheet: {}", self.message)
        } else {
            write!(f, "sheet line {}: {}", self.line, self.message)
        }
    }
}

impl std::error::Error for SheetError {}

/// The result of a conversion.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetConversion {
    pub set: MeasurementSet,
    /// Observation rows without a placement (planned, not measured).
    pub planned_observations: usize,
    /// Candidate rows without a template x (not filled in yet).
    pub planned_candidates: usize,
}

struct Row<'a> {
    cells: Vec<&'a str>,
}

impl Row<'_> {
    fn get(&self, column: &str) -> Option<&str> {
        let i = SHEET_COLUMNS.iter().position(|c| *c == column)?;
        self.cells.get(i).copied().filter(|s| !s.is_empty())
    }
}

fn need<'a>(row: &'a Row<'_>, column: &str) -> Result<&'a str, String> {
    row.get(column)
        .ok_or_else(|| format!("\"{column}\" must be filled in"))
}

fn num(s: &str, column: &str) -> Result<f64, String> {
    s.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("\"{column}\" must be a number (use a dot as decimal separator)"))
}

fn opt_num(row: &Row<'_>, column: &str) -> Result<Option<f64>, String> {
    row.get(column).map(|s| num(s, column)).transpose()
}

fn yes_no(s: &str, column: &str) -> Result<bool, String> {
    match s {
        "yes" | "true" => Ok(true),
        "no" | "false" => Ok(false),
        _ => Err(format!("\"{column}\" must be yes or no")),
    }
}

fn int(s: &str, column: &str) -> Result<u64, String> {
    s.parse::<u64>()
        .map_err(|_| format!("\"{column}\" must be a non-negative integer"))
}

fn id(row: &Row<'_>, column: &str) -> Result<Option<String>, String> {
    match row.get(column) {
        None => Ok(None),
        Some(s) if is_valid_id(s) => Ok(Some(s.to_owned())),
        Some(_) => Err(format!("\"{column}\" must be a short lower-case id")),
    }
}

fn note(row: &Row<'_>, column: &str) -> Result<Option<String>, String> {
    match row.get(column) {
        None => Ok(None),
        Some(s) => {
            check_note(s).map_err(str::to_owned)?;
            Ok(Some(s.to_owned()))
        }
    }
}

fn hex(s: &str, column: &str) -> Result<u64, String> {
    if s.is_empty()
        || s.len() > 16
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(format!("\"{column}\" must be a lower-case hexadecimal id"));
    }
    u64::from_str_radix(s, 16).map_err(|e| e.to_string())
}

fn part(s: &str) -> Result<MarkingPart, String> {
    match s {
        "centre" => Ok(MarkingPart::Centre),
        "inner_edge" => Ok(MarkingPart::InnerEdge),
        "outer_edge" => Ok(MarkingPart::OuterEdge),
        _ => Err("\"marking_part\" must be centre, inner_edge or outer_edge".to_owned()),
    }
}

fn session_row(row: &Row<'_>) -> Result<MeasurementHeader, String> {
    let template = need(row, "template")?.to_owned();
    if template.len() > 256 || !template.ends_with(".pmd") || template.contains('\\') {
        return Err("\"template\" must be the path of a .pmd file inside the archives".to_owned());
    }
    Ok(MeasurementHeader {
        game: Game::parse(need(row, "game")?).ok_or("\"game\" must be ets2 or ats")?,
        telemetry_game_version: row.get("telemetry_game_version").map(str::to_owned),
        game_patch: row.get("game_patch").map(str::to_owned),
        package_content_id: row
            .get("package_content_id")
            .map(|s| hex(s, "package_content_id"))
            .transpose()?,
        template,
        road: hex(need(row, "road")?, "road")?,
        look: need(row, "look")?.to_owned(),
        variant: row.get("variant").map(str::to_owned),
        method: "tyre_edge_alignment".to_owned(),
        synthetic: yes_no(need(row, "synthetic")?, "synthetic")?,
        section: id(row, "section")?,
        trailer: row
            .get("trailer")
            .map(|s| yes_no(s, "trailer"))
            .transpose()?,
        local_scale: opt_num(row, "local_scale")?,
        notes: note(row, "notes")?,
    })
}

/// `Ok(None)` for a planned row (placement still empty).
fn observation_row(row: &Row<'_>) -> Result<Option<Observation>, String> {
    let placement_cells = ["x", "y", "z", "heading"].map(|c| row.get(c));
    if placement_cells.iter().all(Option::is_none) {
        return Ok(None);
    }
    if placement_cells.iter().any(Option::is_none) {
        return Err("x, y, z and heading must all be filled in, or all left empty".to_owned());
    }
    let marking = need(row, "marking")?;
    if !is_valid_id(marking) {
        return Err("\"marking\" must be a short lower-case id".to_owned());
    }
    let frame = match (row.get("frame_session"), row.get("frame_sequence")) {
        (None, None) => None,
        (Some(s), Some(q)) => Some(FrameRef {
            session: int(s, "frame_session")?,
            sequence: int(q, "frame_sequence")?,
        }),
        _ => return Err("frame_session and frame_sequence go together".to_owned()),
    };
    let heading = num(need(row, "heading")?, "heading")?;
    if !(0.0..1.0).contains(&heading) {
        return Err("heading must be in [0, 1) turns, as the telemetry gives it".to_owned());
    }
    let uncertainty_m = num(need(row, "uncertainty_m")?, "uncertainty_m")?;
    if !(uncertainty_m > 0.0 && uncertainty_m <= 1.0) {
        return Err("uncertainty_m must be in (0, 1] metres".to_owned());
    }
    Ok(Some(Observation {
        pass: u32::try_from(int(need(row, "pass")?, "pass")?)
            .map_err(|_| "\"pass\" is too large".to_owned())?,
        direction: match need(row, "direction")? {
            "forward" => Direction::Forward,
            "backward" => Direction::Backward,
            _ => return Err("\"direction\" must be forward or backward".to_owned()),
        },
        tyre: match need(row, "tyre")? {
            "left" => Tyre::Left,
            "right" => Tyre::Right,
            _ => return Err("\"tyre\" must be left or right".to_owned()),
        },
        marking: marking.to_owned(),
        part: part(need(row, "marking_part")?)?,
        placement: Placement {
            x: num(need(row, "x")?, "x")?,
            y: num(need(row, "y")?, "y")?,
            z: num(need(row, "z")?, "z")?,
            heading,
        },
        uncertainty_m,
        point: id(row, "point")?,
        frame,
        pitch: opt_num(row, "pitch")?,
        roll: opt_num(row, "roll")?,
        speed_mps: opt_num(row, "speed_mps")?,
        // A measured row must say whether the criterion was met.
        criterion_met: yes_no(need(row, "criterion_met")?, "criterion_met")?,
        notes: note(row, "notes")?,
    }))
}

/// Converts a filled-in sheet into a measurement set.
///
/// Lines starting with `#` and empty lines are ignored. The first other line
/// must be the column header ([`SHEET_COLUMNS`]). Cells are separated by
/// commas; quotes are not allowed, so a note cannot contain a comma.
///
/// # Errors
///
/// [`SheetError`] for the first problem found. The same checks as for the
/// JSON Lines format apply, and the result is written and read back once.
pub fn convert_sheet(text: &str) -> Result<SheetConversion, SheetError> {
    let whole = |message: String| SheetError { line: 0, message };
    if text.len() > MAX_SHEET_LEN {
        return Err(whole("the sheet is too large".to_owned()));
    }
    let mut header_seen = false;
    let mut session: Option<MeasurementHeader> = None;
    let mut observations = Vec::new();
    let mut candidates: Vec<TemplateCandidate> = Vec::new();
    let (mut planned_observations, mut planned_candidates) = (0, 0);
    for (i, raw) in text.lines().enumerate() {
        let at = |message: String| SheetError {
            line: i + 1,
            message,
        };
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if line.contains('"') {
            return Err(at("quotes are not allowed in a sheet".to_owned()));
        }
        let cells: Vec<&str> = line.split(',').map(str::trim).collect();
        if !header_seen {
            if cells != SHEET_COLUMNS {
                return Err(at(
                    "the first line must be the column header of the sheet template".to_owned(),
                ));
            }
            header_seen = true;
            continue;
        }
        if cells.len() != SHEET_COLUMNS.len() {
            return Err(at(format!(
                "a row needs {} cells, this one has {}",
                SHEET_COLUMNS.len(),
                cells.len()
            )));
        }
        let row = Row { cells };
        match row.get("record") {
            Some("session") => {
                if session.is_some() {
                    return Err(at("a sheet has exactly one session row".to_owned()));
                }
                session = Some(session_row(&row).map_err(at)?);
            }
            Some("candidate") if session.is_some() => match row.get("template_x") {
                None => planned_candidates += 1,
                Some(x) => {
                    let marking = need(&row, "marking").map_err(at)?;
                    if !is_valid_id(marking) {
                        return Err(at("\"marking\" must be a short lower-case id".to_owned()));
                    }
                    candidates.push(TemplateCandidate {
                        marking: marking.to_owned(),
                        part: part(need(&row, "marking_part").map_err(at)?).map_err(at)?,
                        x: num(x, "template_x").map_err(at)?,
                    });
                }
            },
            Some("observation") if session.is_some() => match observation_row(&row).map_err(at)? {
                None => planned_observations += 1,
                Some(o) => observations.push(o),
            },
            Some("candidate" | "observation") => {
                return Err(at("the session row must come first".to_owned()));
            }
            _ => {
                return Err(at(
                    "\"record\" must be session, candidate or observation".to_owned()
                ));
            }
        }
    }
    let header = session.ok_or_else(|| whole("the sheet has no session row".to_owned()))?;
    let set = MeasurementSet {
        header,
        observations,
        candidates,
    };
    // Same checks as the JSON Lines format (duplicate frames, candidates).
    let text = crate::validation::write_measurements(&set).map_err(whole)?;
    let set = crate::validation::read_measurements(&text).map_err(|e| whole(e.to_string()))?;
    Ok(SheetConversion {
        set,
        planned_observations,
        planned_candidates,
    })
}

/// A blank sheet for one session: the session row with the game filled in,
/// one candidate row per marking, and one planned observation row per
/// marking, measuring point, pass and alignment. **No measured value is
/// filled in.**
///
/// # Panics
///
/// Never; invalid marking ids are skipped.
#[must_use]
pub fn blank_sheet(game: Game, markings: &[String], points: u32, passes: u32) -> String {
    let mut out = String::new();
    for line in [
        "# RoadPilot-Map template measurement sheet (schema 1.1). One sheet = one session on one road.",
        "# Fill in the session row first. Never enter names, profile names, save games or screenshot paths.",
        "# Copy x, y, z, heading, pitch, roll and speed exactly as the telemetry frame gives them (dot as decimal separator).",
        "# criterion_met: yes if the tyre edge was on the marking part when the frame was taken, otherwise no (keep the row).",
        "# Leave a row's x, y, z and heading empty if it was not measured; it is then skipped. No commas or quotes in notes.",
        "# See docs/validation/measurement-sheet.md.",
    ] {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&SHEET_COLUMNS.join(","));
    out.push('\n');
    let row = |values: &[(&str, String)]| {
        SHEET_COLUMNS
            .iter()
            .map(|c| {
                values
                    .iter()
                    .find(|(k, _)| k == c)
                    .map_or(String::new(), |(_, v)| v.clone())
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    out.push_str(&row(&[
        ("record", "session".to_owned()),
        ("game", game.id().to_owned()),
    ]));
    out.push('\n');
    let markings: Vec<&String> = markings.iter().filter(|m| is_valid_id(m)).collect();
    for m in &markings {
        out.push_str(&row(&[
            ("record", "candidate".to_owned()),
            ("marking", (*m).clone()),
            ("marking_part", "centre".to_owned()),
        ]));
        out.push('\n');
    }
    let alignments = [
        ("forward", "left"),
        ("forward", "right"),
        ("backward", "left"),
        ("backward", "right"),
    ];
    let mut pass = 0u32;
    for m in &markings {
        for p in 1..=points.max(1) {
            for _ in 0..passes.max(1) {
                for (direction, tyre) in alignments {
                    pass += 1;
                    out.push_str(&row(&[
                        ("record", "observation".to_owned()),
                        ("marking", (*m).clone()),
                        ("marking_part", "centre".to_owned()),
                        ("point", format!("p{p}")),
                        ("pass", pass.to_string()),
                        ("direction", direction.to_owned()),
                        ("tyre", tyre.to_owned()),
                    ]));
                    out.push('\n');
                }
            }
        }
    }
    out
}
