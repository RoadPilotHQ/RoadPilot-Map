//! Commands for preparing and checking in-game template measurements.
//!
//! See `docs/validation/measurement-sheet.md`. These commands only read the
//! game installation and the map package; they write only the file given
//! with `--out`.

use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use roadpilot_map::Game;
use roadpilot_map::map_package::{CurveRecord, MapPackage};
use roadpilot_map::measurement_sheet::{MAX_SHEET_LEN, blank_sheet, convert_sheet};
use roadpilot_map::validation::{
    self, Acceptance, MAX_FILE_LEN, Verdict, assess, evaluate, read_measurements,
    write_measurements,
};

use crate::{EXIT_FAILURE, EXIT_INSTALLATION, EXIT_PROCESSING, usage_error};

/// A candidate road: UID, look, length, midpoint (x, z), forward heading.
type Candidate<'a> = (u64, &'a str, f64, [f64; 2], f64);

/// Roads shorter than this are not proposed (world units).
const MIN_LENGTH: f64 = 60.0;
/// Roads that turn more than this are not proposed (degrees).
const MAX_TURN_DEG: f64 = 1.0;

fn fail(code: u8, message: &str) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::from(code)
}

/// Writes `text` to `out`, refusing to replace a file unless `force`.
fn write_out(out: &Path, text: &str, force: bool) -> Result<(), String> {
    if out.is_dir() {
        return Err(format!("{} is a folder", out.display()));
    }
    if out.exists() && !force {
        return Err(format!(
            "{} already exists; use --force to replace it",
            out.display()
        ));
    }
    std::fs::write(out, text)
        .map_err(|e| format!("cannot write {} ({:?})", out.display(), e.kind()))
}

fn read_limited(path: &Path, limit: usize) -> Result<String, String> {
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("cannot open {} ({:?})", path.display(), e.kind()))?;
    if meta.len() > limit as u64 {
        return Err(format!("{} is too large", path.display()));
    }
    std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {} ({:?})", path.display(), e.kind()))
}

pub(crate) fn run_blank_sheet(args: &[String]) -> ExitCode {
    let (mut game, mut markings, mut points, mut passes, mut out, mut force) =
        (None, None, 3u32, 1u32, None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        let r: Result<(), String> = (|| {
            match a.as_str() {
                "--game" => {
                    let v = value("--game")?;
                    game = Some(Game::parse(&v).ok_or_else(|| format!("unknown game \"{v}\""))?);
                }
                "--markings" => {
                    let v = value("--markings")?;
                    let list: Vec<String> = v.split(',').map(str::to_owned).collect();
                    if list.iter().any(|m| !validation::is_valid_id(m)) {
                        return Err(
                            "markings must be short lower-case ids, separated by commas".to_owned()
                        );
                    }
                    markings = Some(list);
                }
                "--points" => {
                    points = value("--points")?
                        .parse()
                        .map_err(|_| "--points needs a number".to_owned())?
                }
                "--passes" => {
                    passes = value("--passes")?
                        .parse()
                        .map_err(|_| "--passes needs a number".to_owned())?
                }
                "--out" => out = Some(PathBuf::from(value("--out")?)),
                "--force" => force = true,
                other => return Err(format!("unrecognized argument: {other}")),
            }
            Ok(())
        })();
        if let Err(m) = r {
            return usage_error(&m);
        }
    }
    let (Some(game), Some(markings), Some(out)) = (game, markings, out) else {
        return usage_error("--game, --markings and --out are required");
    };
    if !(1..=20).contains(&points) || !(1..=10).contains(&passes) {
        return usage_error("--points must be 1 to 20 and --passes 1 to 10");
    }
    let sheet = blank_sheet(game, &markings, points, passes);
    match write_out(&out, &sheet, force) {
        Ok(()) => {
            println!(
                "Wrote a blank {} sheet to {}: {} markings, {points} points, {passes} passes, {} planned observations. No values are filled in.",
                game.id(),
                out.display(),
                markings.len(),
                markings.len() as u32 * points * passes * 4
            );
            ExitCode::SUCCESS
        }
        Err(m) => fail(EXIT_FAILURE, &m),
    }
}

pub(crate) fn run_convert_sheet(args: &[String]) -> ExitCode {
    let (mut sheet, mut out, mut force) = (None, None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => match it.next() {
                Some(v) => out = Some(PathBuf::from(v)),
                None => return usage_error("--out needs a value"),
            },
            "--force" => force = true,
            s if s.starts_with("--") => return usage_error(&format!("unrecognized argument: {s}")),
            s if sheet.is_none() => sheet = Some(PathBuf::from(s)),
            s => return usage_error(&format!("unexpected argument: {s}")),
        }
    }
    let (Some(sheet), Some(out)) = (sheet, out) else {
        return usage_error("a sheet and --out are required");
    };
    let text = match read_limited(&sheet, MAX_SHEET_LEN) {
        Ok(t) => t,
        Err(m) => return fail(EXIT_FAILURE, &m),
    };
    let c = match convert_sheet(&text) {
        Ok(c) => c,
        Err(e) => return fail(EXIT_PROCESSING, &e.to_string()),
    };
    let jsonl = match write_measurements(&c.set) {
        Ok(j) => j,
        Err(m) => return fail(EXIT_PROCESSING, &m),
    };
    if let Err(m) = write_out(&out, &jsonl, force) {
        return fail(EXIT_FAILURE, &m);
    }
    let not_met = c
        .set
        .observations
        .iter()
        .filter(|o| !o.criterion_met)
        .count();
    println!(
        "Converted {}: {} observations ({not_met} with the criterion not met), {} candidates; {} planned observations and {} candidates without values were skipped.",
        sheet.display(),
        c.set.observations.len(),
        c.set.candidates.len(),
        c.planned_observations,
        c.planned_candidates
    );
    if c.set.header.synthetic {
        println!("The session is marked synthetic: it never counts for the acceptance criteria.");
    }
    ExitCode::SUCCESS
}

pub(crate) fn run_check_measurements(args: &[String]) -> ExitCode {
    let (mut package_dir, mut files) = (None, Vec::new());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--package" => match it.next() {
                Some(v) => package_dir = Some(PathBuf::from(v)),
                None => return usage_error("--package needs a value"),
            },
            s if s.starts_with("--") => return usage_error(&format!("unrecognized argument: {s}")),
            s => files.push(PathBuf::from(s)),
        }
    }
    if files.is_empty() {
        return usage_error("at least one measurement file is required");
    }
    let package = match package_dir.as_deref().map(MapPackage::open).transpose() {
        Ok(p) => p,
        Err(e) => return fail(EXIT_PROCESSING, &e.to_string()),
    };
    let mut reports = Vec::new();
    for f in &files {
        let text = match read_limited(f, MAX_FILE_LEN) {
            Ok(t) => t,
            Err(m) => return fail(EXIT_FAILURE, &m),
        };
        let set = match read_measurements(&text) {
            Ok(s) => s,
            Err(e) => return fail(EXIT_PROCESSING, &format!("{}: {e}", f.display())),
        };
        let h = &set.header;
        println!(
            "{}: {} {}, road {:x}, {} observations, {} candidates{}",
            f.display(),
            h.game.id(),
            h.template,
            h.road,
            set.observations.len(),
            set.candidates.len(),
            if h.synthetic { " (synthetic)" } else { "" }
        );
        let Some(package) = &package else {
            continue;
        };
        if package.manifest().game != h.game.id() {
            return fail(
                EXIT_PROCESSING,
                &format!(
                    "{}: the package is for {}",
                    f.display(),
                    package.manifest().game
                ),
            );
        }
        if h.package_content_id
            .is_some_and(|c| c != package.manifest().content_id())
        {
            return fail(
                EXIT_PROCESSING,
                &format!(
                    "{}: the session used another package (content id differs)",
                    f.display()
                ),
            );
        }
        let Some(curve) = validation::road_curve(package, h.road) else {
            return fail(
                EXIT_PROCESSING,
                &format!(
                    "{}: road {:x} has no curve in the package",
                    f.display(),
                    h.road
                ),
            );
        };
        let report = evaluate(&set, &curve, &set.candidates);
        println!(
            "  road turn {:.2}°, length consistent: {}, max heading error {:.2}°, excluded observations {}",
            report.max_turn_deg,
            report.length_consistent,
            report.max_heading_error_deg,
            report.excluded_observations
        );
        for m in &report.results {
            let fmt = |v: Option<f64>| v.map_or("—".to_owned(), |x| format!("{x:+.3}"));
            println!(
                "  {} ({:?}): kinds {}, line {} m ± {}, template x {}, residual x-right {}, x-left {}",
                m.marking,
                m.part,
                m.kinds.len(),
                fmt(m.line_m),
                fmt(m.line_uncertainty_m),
                fmt(m.candidate_x),
                fmt(m.residual_x_right_m),
                fmt(m.residual_x_left_m)
            );
        }
        reports.push(report);
    }
    if package.is_none() {
        println!("All files are valid. Pass --package to evaluate them against a map package.");
        return ExitCode::SUCCESS;
    }
    let a = assess(&reports, &Acceptance::default());
    for c in &a.checks {
        println!(
            "[{}] {}: {}",
            if c.passed { "PASS" } else { "OPEN" },
            c.id,
            c.detail
        );
    }
    println!(
        "Verdict: {}",
        match a.verdict {
            Verdict::InsufficientData => "insufficient data",
            Verdict::Inconsistent => "inconsistent",
            Verdict::CriteriaMetPendingReview =>
                "criteria met, pending review (not confirmed; C7 needs a human review and an ADR)",
        }
    );
    ExitCode::SUCCESS
}

pub(crate) fn run_candidates(args: &[String]) -> ExitCode {
    let (mut package_dir, mut game_dir, mut per_template) = (None, None, 3usize);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let v = it.next();
        match (a.as_str(), v) {
            ("--package", Some(v)) => package_dir = Some(PathBuf::from(v)),
            ("--game-dir", Some(v)) => game_dir = Some(PathBuf::from(v)),
            ("--per-template", Some(v)) => match v.parse() {
                Ok(n) if (1..=50).contains(&n) => per_template = n,
                _ => return usage_error("--per-template must be 1 to 50"),
            },
            (other, _) => {
                return usage_error(&format!("unrecognized or incomplete argument: {other}"));
            }
        }
    }
    let (Some(package_dir), Some(game_dir)) = (package_dir, game_dir) else {
        return usage_error("--package and --game-dir are required");
    };
    let package = match MapPackage::open(&package_dir) {
        Ok(p) => p,
        Err(e) => return fail(EXIT_PROCESSING, &e.to_string()),
    };
    let templates = match roadpilot_map::road_look_templates(&game_dir) {
        Ok(t) => t,
        Err(e) => {
            return fail(
                match e {
                    roadpilot_map::InstallationError::NotRecognized(_) => EXIT_INSTALLATION,
                    _ => EXIT_PROCESSING,
                },
                &e.to_string(),
            );
        }
    };
    // Template → straight, long, length-consistent roads (by UID order).
    let mut found: BTreeMap<&str, Vec<Candidate<'_>>> = BTreeMap::new();
    let mut considered = 0usize;
    for road in &package.data().roads {
        let Some(template) = templates.get(&road.look) else {
            continue;
        };
        considered += 1;
        if !matches!(
            road.curve,
            CurveRecord::Estimated {
                length_consistent: true,
                ..
            }
        ) || f64::from(road.stored_length) < MIN_LENGTH
        {
            continue;
        }
        let Some(curve) = validation::road_curve(&package, road.id) else {
            continue;
        };
        if curve.turn_angle().to_degrees() > MAX_TURN_DEG {
            continue;
        }
        let mid = curve.point(0.5);
        let d = curve.derivative(0.5);
        let heading = ((-d[0]).atan2(-d[2]) / TAU).rem_euclid(1.0);
        found.entry(template.as_str()).or_default().push((
            road.id,
            road.look.as_str(),
            f64::from(road.stored_length),
            [mid[0], mid[2]],
            heading,
        ));
    }
    println!(
        "Package {} ({}), content id {:x}. {considered} roads use a template look; candidates: straight (turn ≤ {MAX_TURN_DEG}°), length-consistent, at least {MIN_LENGTH} units long.",
        package_dir.display(),
        package.manifest().game,
        package.manifest().content_id()
    );
    println!(
        "These are UNVERIFIED candidates: check markings, traffic, slope and surroundings in the game before measuring."
    );
    println!("template,roads_matching,road,look,length,mid_x,mid_z,forward_heading");
    for (template, roads) in &found {
        for (id, look, length, mid, heading) in roads.iter().take(per_template) {
            println!(
                "{template},{},{id:x},{look},{length:.1},{:.1},{:.1},{heading:.4}",
                roads.len(),
                mid[0],
                mid[1]
            );
        }
    }
    println!(
        "{} templates have at least one candidate road.",
        found.len()
    );
    ExitCode::SUCCESS
}

/// Reads the first non-empty line of `reader`, at most `limit` bytes.
fn first_line(reader: impl std::io::BufRead, limit: usize) -> Result<String, String> {
    let mut buf = String::new();
    let mut limited = reader.take(limit as u64 + 1);
    loop {
        buf.clear();
        let n = std::io::BufRead::read_line(&mut limited, &mut buf)
            .map_err(|e| format!("cannot read the snapshot ({:?})", e.kind()))?;
        if n == 0 {
            return Err("no snapshot line was given".to_owned());
        }
        if buf.len() > limit {
            return Err("the snapshot line is too long".to_owned());
        }
        if !buf.trim().is_empty() {
            return Ok(buf.trim().to_owned());
        }
    }
}

pub(crate) fn run_import_snapshot(args: &[String]) -> ExitCode {
    use roadpilot_map::snapshot_import::{MAX_SNAPSHOT_LEN, import_snapshot, parse_snapshot};
    let (mut sheet, mut pass, mut from, mut replace) = (None, None, None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--pass" => match it.next().and_then(|v| v.parse::<u32>().ok()) {
                Some(p) => pass = Some(p),
                None => return usage_error("--pass needs a pass number"),
            },
            "--from" => match it.next() {
                Some(v) => from = Some(PathBuf::from(v)),
                None => return usage_error("--from needs a file"),
            },
            "--replace" => replace = true,
            s if s.starts_with("--") => return usage_error(&format!("unrecognized argument: {s}")),
            s if sheet.is_none() => sheet = Some(PathBuf::from(s)),
            s => return usage_error(&format!("unexpected argument: {s}")),
        }
    }
    let (Some(sheet), Some(pass)) = (sheet, pass) else {
        return usage_error("a sheet and --pass are required");
    };
    let text = match read_limited(&sheet, MAX_SHEET_LEN) {
        Ok(t) => t,
        Err(m) => return fail(EXIT_FAILURE, &m),
    };
    let line = match &from {
        Some(f) => std::fs::File::open(f)
            .map_err(|e| format!("cannot open {} ({:?})", f.display(), e.kind()))
            .and_then(|file| first_line(std::io::BufReader::new(file), MAX_SNAPSHOT_LEN)),
        None => {
            eprintln!("Paste one snapshot line from the telemetry reader and press Enter:");
            first_line(std::io::stdin().lock(), MAX_SNAPSHOT_LEN)
        }
    };
    let line = match line {
        Ok(l) => l,
        Err(m) => return fail(EXIT_FAILURE, &m),
    };
    let snapshot = match parse_snapshot(&line) {
        Ok(s) => s,
        Err(e) => return fail(EXIT_PROCESSING, &e.to_string()),
    };
    let imported = match import_snapshot(&text, pass, &snapshot, replace) {
        Ok(i) => i,
        Err(e) => return fail(EXIT_PROCESSING, &e.to_string()),
    };
    // Write next to the sheet and move into place, so the sheet is never
    // left half-written.
    let mut partial = sheet.clone().into_os_string();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    if partial.exists() {
        return fail(
            EXIT_FAILURE,
            &format!(
                "{} exists from an earlier run; remove it first",
                partial.display()
            ),
        );
    }
    if let Err(e) =
        std::fs::write(&partial, &imported.sheet).and_then(|()| std::fs::rename(&partial, &sheet))
    {
        let _ = std::fs::remove_file(&partial);
        return fail(
            EXIT_FAILURE,
            &format!("cannot write {} ({:?})", sheet.display(), e.kind()),
        );
    }
    println!(
        "Filled pass {pass} (line {}) with frame {}/{} of {} (SDK game version {}).",
        imported.line,
        snapshot.session,
        snapshot.sequence,
        snapshot.game.id(),
        snapshot.sdk_game_version
    );
    if !imported.filled_session_columns.is_empty() {
        println!(
            "Session columns filled: {}.",
            imported.filled_session_columns.join(", ")
        );
    }
    if !snapshot.dropped_fields.is_empty() {
        println!(
            "Note: the reader dropped invalid fields: {}.",
            snapshot.dropped_fields.join(", ")
        );
    }
    if snapshot.speed_mps.abs() > validation::MAX_STANDING_SPEED_MPS {
        println!(
            "Warning: speed {} m/s; the truck was not standing still, so this observation will not be used.",
            snapshot.speed_mps
        );
    }
    println!(
        "Now fill in criterion_met and uncertainty_m for pass {pass} yourself. The game patch stays a manual entry."
    );
    ExitCode::SUCCESS
}
