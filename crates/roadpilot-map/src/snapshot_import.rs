//! Import of one RoadPilot-Telemetry frame snapshot into a measurement sheet.
//!
//! The RoadPilot-Telemetry console reader prints the newest frame as one JSON
//! line when Enter is pressed (`roadpilot.telemetry.snapshot`, version 1,
//! documented in RoadPilot-Telemetry `docs/FRAME-SNAPSHOT.md`). This module
//! checks such a line strictly and copies its frame reference, position,
//! orientation and speed into **one planned observation row** of a
//! measurement sheet ([`crate::measurement_sheet`]).
//!
//! The import never sets `criterion_met`, `uncertainty_m`, the template, the
//! road, the look, `template_x` or the game patch: those are human
//! observations or checked metadata. It never invents a value: a missing or
//! invalid field is an error. It does not convert coordinates.
//!
//! Numbers keep their value: each is parsed from the snapshot text into an
//! `f64` and written back as the shortest text of that same value. The text
//! can differ in notation only (for example `1e-5` becomes `0.00001`).

use std::fmt;

use roadpilot_map_package::json::{self, Json};

use crate::Game;
use crate::measurement_sheet::SHEET_COLUMNS;

/// The `kind` of a snapshot line.
pub const SNAPSHOT_KIND: &str = "roadpilot.telemetry.snapshot";
/// The snapshot layout version this module accepts.
pub const SNAPSHOT_VERSION: i64 = 1;
/// Longest snapshot line accepted, bytes.
pub const MAX_SNAPSHOT_LEN: usize = 64 << 10;
/// Largest integer the telemetry protocol uses (2^53 − 1).
const MAX_SAFE_INTEGER: i64 = (1 << 53) - 1;

/// A checked snapshot: only the values a measurement takes over.
#[derive(Debug, Clone, PartialEq)]
pub struct TelemetrySnapshot {
    pub game: Game,
    /// The SDK telemetry game version (`major.minor`), **not** the game
    /// patch. It goes into `telemetry_game_version`.
    pub sdk_game_version: String,
    pub session: u64,
    pub sequence: u64,
    pub local_scale: Option<f64>,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub heading: f64,
    pub pitch: f64,
    pub roll: f64,
    pub speed_mps: f64,
    /// Fields the reader's decoder removed as invalid, for information.
    pub dropped_fields: Vec<String>,
}

/// Why a snapshot line was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotError(pub String);

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "snapshot refused: {}", self.0)
    }
}

impl std::error::Error for SnapshotError {}

fn err<T>(m: impl Into<String>) -> Result<T, SnapshotError> {
    Err(SnapshotError(m.into()))
}

fn field<'a>(v: &'a Json, path: &str) -> Result<&'a Json, SnapshotError> {
    let mut cur = v;
    for key in path.split('.') {
        cur = match cur.get(key) {
            Some(Json::Null) | None => return err(format!("\"{path}\" is missing")),
            Some(x) => x,
        };
    }
    Ok(cur)
}

fn number(v: &Json, path: &str) -> Result<f64, SnapshotError> {
    match field(v, path)? {
        Json::Int(i) => Ok(*i as f64),
        Json::Float(f) if f.is_finite() => Ok(*f),
        _ => err(format!("\"{path}\" must be a finite number")),
    }
}

fn integer(v: &Json, path: &str) -> Result<u64, SnapshotError> {
    match field(v, path)? {
        Json::Int(i) if (0..=MAX_SAFE_INTEGER).contains(i) => Ok(*i as u64),
        _ => err(format!("\"{path}\" must be an integer from 0 to 2^53 - 1")),
    }
}

fn text<'a>(v: &'a Json, path: &str) -> Result<&'a str, SnapshotError> {
    field(v, path)?
        .as_str()
        .ok_or_else(|| SnapshotError(format!("\"{path}\" must be a string")))
}

/// Checks one snapshot line.
///
/// Accepted only if: it is valid JSON of kind `roadpilot.telemetry.snapshot`
/// version 1; `status` is `fresh`; `missing` is empty; the frame is protocol
/// `roadpilot.telemetry` schema 2 with `pluginState` `running`; and session,
/// sequence, game, `gameVersion`, the six placement values and `speedMps` are
/// present and valid (heading in [0, 1), pitch in [−0.25, 0.25], roll in
/// [−0.5, 0.5] turns, as the protocol defines).
///
/// # Errors
///
/// [`SnapshotError`] naming the first problem.
pub fn parse_snapshot(line: &str) -> Result<TelemetrySnapshot, SnapshotError> {
    let line = line.trim();
    if line.is_empty() {
        return err("the line is empty");
    }
    if line.len() > MAX_SNAPSHOT_LEN {
        return err("the line is too long");
    }
    let v = json::parse(line).map_err(|e| SnapshotError(format!("not valid JSON ({e})")))?;
    if v.get("kind").and_then(Json::as_str) != Some(SNAPSHOT_KIND) {
        return err(format!("\"kind\" must be \"{SNAPSHOT_KIND}\""));
    }
    if v.get("snapshotVersion").and_then(Json::as_i64) != Some(SNAPSHOT_VERSION) {
        return err("unsupported snapshotVersion (supported: 1)");
    }
    let status = text(&v, "status")?;
    if status != "fresh" {
        return err(format!(
            "status is \"{status}\"; only a fresh frame can be imported (press Enter again while the game runs)"
        ));
    }
    match field(&v, "missing")? {
        Json::Arr(a) if a.is_empty() => {}
        Json::Arr(a) => {
            let names: Vec<&str> = a.iter().filter_map(Json::as_str).collect();
            return err(format!("the frame lacks {}", names.join(", ")));
        }
        _ => return err("\"missing\" must be an array"),
    }
    let dropped_fields = match v.get("droppedFields") {
        None | Some(Json::Null) => Vec::new(),
        Some(Json::Arr(a)) => a
            .iter()
            .map(|d| {
                d.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| SnapshotError("\"droppedFields\" must hold strings".to_owned()))
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return err("\"droppedFields\" must be an array"),
    };
    let f = field(&v, "frame")?;
    if text(f, "protocol")? != "roadpilot.telemetry" {
        return err("frame.protocol must be \"roadpilot.telemetry\"");
    }
    if f.get("schemaVersion").and_then(Json::as_i64) != Some(2) {
        return err("frame.schemaVersion must be 2");
    }
    if text(f, "pluginState")? != "running" {
        return err("the game was not running (pluginState is not \"running\")");
    }
    let game = Game::parse(text(f, "game")?)
        .ok_or_else(|| SnapshotError("frame.game must be ets2 or ats".to_owned()))?;
    let sdk_game_version = text(f, "gameVersion")?;
    if sdk_game_version.is_empty()
        || sdk_game_version.len() > 32
        || !sdk_game_version
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.')
    {
        return err("frame.gameVersion must look like major.minor");
    }
    let local_scale = match f.get("localScale") {
        None | Some(Json::Null) => None,
        Some(_) => Some(number(f, "localScale")?),
    };
    let heading = number(f, "placement.heading")?;
    let pitch = number(f, "placement.pitch")?;
    let roll = number(f, "placement.roll")?;
    if !(0.0..1.0).contains(&heading) {
        return err("placement.heading must be in [0, 1) turns");
    }
    if !(-0.25..=0.25).contains(&pitch) || !(-0.5..=0.5).contains(&roll) {
        return err("placement.pitch or placement.roll is out of range");
    }
    Ok(TelemetrySnapshot {
        game,
        sdk_game_version: sdk_game_version.to_owned(),
        session: integer(f, "session")?,
        sequence: integer(f, "sequence")?,
        local_scale,
        x: number(f, "placement.x")?,
        y: number(f, "placement.y")?,
        z: number(f, "placement.z")?,
        heading,
        pitch,
        roll,
        speed_mps: number(f, "speedMps")?,
        dropped_fields,
    })
}

/// Why a snapshot could not be put into the sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportError(pub String);

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "import refused: {}", self.0)
    }
}

impl std::error::Error for ImportError {}

fn col(name: &str) -> usize {
    SHEET_COLUMNS
        .iter()
        .position(|c| *c == name)
        .unwrap_or(usize::MAX)
}

/// Columns the import writes into the observation row.
const OBSERVATION_COLUMNS: [&str; 9] = [
    "frame_session",
    "frame_sequence",
    "x",
    "y",
    "z",
    "heading",
    "pitch",
    "roll",
    "speed_mps",
];

/// The result of [`import_snapshot`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    /// The whole sheet with the row filled in.
    pub sheet: String,
    /// The line number (from 1) of the filled row.
    pub line: usize,
    /// Session columns the import filled because they were empty.
    pub filled_session_columns: Vec<&'static str>,
}

fn same_number(cell: &str, value: f64) -> bool {
    cell.parse::<f64>().is_ok_and(|c| c == value)
}

/// Puts a checked snapshot into the observation row with pass number `pass`.
///
/// Requirements: the sheet has its column header and a session row; the
/// session's `game` equals the snapshot's game; the session's `synthetic` cell
/// is filled in (`yes` or `no`) and the import does not change it; exactly
/// one observation row has this pass; its frame and placement cells are empty
/// (unless `replace`); and no other row uses the same frame.
///
/// Writes `frame_session`, `frame_sequence`, `x`, `y`, `z`, `heading`,
/// `pitch`, `roll`, `speed_mps`. In the session row it fills
/// `telemetry_game_version` (SDK version) and `local_scale` if they are empty,
/// and refuses if they hold other values. Nothing else is changed: not
/// `criterion_met`, `uncertainty_m`, `notes`, the template, road, look,
/// candidates or the game patch. All other lines stay byte for byte.
///
/// # Errors
///
/// [`ImportError`] naming the first problem; the sheet is then unchanged.
pub fn import_snapshot(
    sheet: &str,
    pass: u32,
    snapshot: &TelemetrySnapshot,
    replace: bool,
) -> Result<Imported, ImportError> {
    let fail = |m: String| Err(ImportError(m));
    let newline = if sheet.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = sheet
        .lines()
        .map(|l| l.trim_end_matches('\r').to_owned())
        .collect();
    let content = |l: &str| !l.trim().is_empty() && !l.trim_start().starts_with('#');

    let Some(header) = lines.iter().position(|l| content(l)) else {
        return fail("the sheet has no column header".to_owned());
    };
    let cells_of = |l: &str| -> Vec<String> { l.split(',').map(|c| c.trim().to_owned()).collect() };
    if cells_of(&lines[header]) != SHEET_COLUMNS {
        return fail("the first line is not the column header of the sheet template".to_owned());
    }
    let mut session_line = None;
    let mut target = Vec::new();
    for (i, l) in lines.iter().enumerate().skip(header + 1) {
        if !content(l) {
            continue;
        }
        if l.contains('"') {
            return fail(format!("line {}: quotes are not allowed in a sheet", i + 1));
        }
        let c = cells_of(l);
        if c.len() != SHEET_COLUMNS.len() {
            return fail(format!(
                "line {}: a row needs {} cells",
                i + 1,
                SHEET_COLUMNS.len()
            ));
        }
        match c[col("record")].as_str() {
            "session" => session_line = Some(i),
            "observation" => {
                if c[col("pass")] == pass.to_string() {
                    target.push(i);
                }
                if c[col("frame_session")] == snapshot.session.to_string()
                    && c[col("frame_sequence")] == snapshot.sequence.to_string()
                    && c[col("pass")] != pass.to_string()
                {
                    return fail(format!(
                        "line {}: frame {}/{} is already used by pass {}; take a new snapshot",
                        i + 1,
                        snapshot.session,
                        snapshot.sequence,
                        c[col("pass")]
                    ));
                }
            }
            _ => {}
        }
    }
    let Some(s) = session_line else {
        return fail("the sheet has no session row".to_owned());
    };
    let mut session = cells_of(&lines[s]);
    if session[col("game")] != snapshot.game.id() {
        return fail(format!(
            "the snapshot is from {}, the sheet is for {:?}",
            snapshot.game.id(),
            session[col("game")]
        ));
    }
    if !matches!(session[col("synthetic")].as_str(), "yes" | "no") {
        return fail(
            "fill in the session's synthetic cell (yes or no) before importing".to_owned(),
        );
    }
    let mut filled = Vec::new();
    let tv = col("telemetry_game_version");
    if session[tv].is_empty() {
        session[tv] = snapshot.sdk_game_version.clone();
        filled.push("telemetry_game_version");
    } else if session[tv] != snapshot.sdk_game_version {
        return fail(format!(
            "the sheet has telemetry_game_version {}, the snapshot {}; a session must not mix game versions",
            session[tv], snapshot.sdk_game_version
        ));
    }
    let ls = col("local_scale");
    if let Some(scale) = snapshot.local_scale {
        if session[ls].is_empty() {
            session[ls] = format!("{scale}");
            filled.push("local_scale");
        } else if !same_number(&session[ls], scale) {
            return fail(format!(
                "the sheet has local_scale {}, the snapshot {scale}",
                session[ls]
            ));
        }
    }

    let row_line = match target.as_slice() {
        [one] => *one,
        [] => return fail(format!("no observation row has pass {pass}")),
        _ => return fail(format!("several observation rows have pass {pass}")),
    };
    let mut row = cells_of(&lines[row_line]);
    if !replace && OBSERVATION_COLUMNS.iter().any(|c| !row[col(c)].is_empty()) {
        return fail(format!(
            "pass {pass} already has frame or placement values; use --replace to overwrite them"
        ));
    }
    let values = [
        snapshot.session.to_string(),
        snapshot.sequence.to_string(),
        format!("{}", snapshot.x),
        format!("{}", snapshot.y),
        format!("{}", snapshot.z),
        format!("{}", snapshot.heading),
        format!("{}", snapshot.pitch),
        format!("{}", snapshot.roll),
        format!("{}", snapshot.speed_mps),
    ];
    for (c, v) in OBSERVATION_COLUMNS.iter().zip(values) {
        row[col(c)] = v;
    }
    if !filled.is_empty() {
        lines[s] = session.join(",");
    }
    lines[row_line] = row.join(",");
    let mut out = lines.join(newline);
    if sheet.ends_with('\n') {
        out.push_str(newline);
    }
    Ok(Imported {
        sheet: out,
        line: row_line + 1,
        filled_session_columns: filled,
    })
}
