//! Opt-in test: read and measure the road templates (`.pmd`/`.pmg`) of a
//! local ETS2 or ATS installation.
//!
//! Ignored by default and never run in CI. Reads the archives read-only,
//! copies nothing into the repository, checks structural properties and
//! prints counts and percentages only. See
//! `docs/formats/scs-road-templates.md` for what the numbers mean.
//!
//! ```powershell
//! $env:ROADPILOT_MAP_GAME_DIR = "C:\Program Files (x86)\Steam\steamapps\common\Euro Truck Simulator 2"
//! cargo test --release -p roadpilot-map --test real_road_templates -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use roadpilot_map::formats::pmg::{PMG_VERSION, parse_pmg};
use roadpilot_map::formats::sii::{SiiUnit, parse_sii};
use roadpilot_map_archive::Archive;

fn game_dir() -> PathBuf {
    let dir = std::env::var_os("ROADPILOT_MAP_GAME_DIR")
        .map(PathBuf::from)
        .expect("set ROADPILOT_MAP_GAME_DIR to an ETS2 or ATS installation folder");
    assert!(dir.join("base_map.scs").is_file());
    dir
}

fn walk(archive: &Archive, dir: &str, out: &mut Vec<String>) {
    let Ok(children) = archive.list_dir(dir) else {
        return;
    };
    for c in children {
        let path = format!("{dir}/{}", c.name.trim_start_matches('/'));
        if c.is_dir {
            walk(archive, &path, out);
        } else {
            out.push(path);
        }
    }
}

fn float(unit: &SiiUnit, key: &str) -> Option<f32> {
    unit.value(key)?.trim().parse().ok()
}

#[test]
#[ignore = "reads a local game installation; set ROADPILOT_MAP_GAME_DIR and run with --ignored"]
fn reads_and_measures_the_road_templates_of_the_installation() {
    let dir = game_dir();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("readable")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("scs")))
        .collect();
    paths.sort();

    // Later archives override earlier ones, as for the other definitions.
    let mut pmg: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut pmd: BTreeMap<String, usize> = BTreeMap::new();
    let mut units: Vec<SiiUnit> = Vec::new();
    let (mut extra_record_files, mut plain_files) = (0usize, 0usize);
    for path in &paths {
        let Ok(archive) = Archive::open(path) else {
            continue;
        };
        let mut files = Vec::new();
        walk(&archive, "road_template", &mut files);
        for f in files {
            if f.ends_with(".pmg") {
                // Plain read first: it must be refused for files with extra
                // records, which is why `read_primary_record` exists.
                if archive.read_file(&f).is_ok() {
                    plain_files += 1;
                } else {
                    extra_record_files += 1;
                }
                let bytes = archive.read_primary_record(&f).expect("pmg is readable");
                pmg.insert(f, bytes);
            } else if f.ends_with(".pmd") {
                let bytes = archive.read_file(&f).expect("pmd is readable");
                assert!(bytes.len() >= 4);
                pmd.insert(
                    f,
                    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize,
                );
            }
        }
        if let Ok(children) = archive.list_dir("def/world") {
            for c in children
                .iter()
                .filter(|c| c.name.starts_with("road_look") && c.name.ends_with(".sii"))
            {
                let file = format!("def/world/{}", c.name);
                let bytes = archive.read_file(&file).expect("definition is readable");
                let mut include = |p: &str| {
                    let full = p
                        .strip_prefix('/')
                        .map_or_else(|| format!("def/world/{p}"), str::to_owned);
                    archive.read_file(&full).ok()
                };
                units.extend(parse_sii(&bytes, &mut include).expect("definition parses"));
            }
        }
    }
    println!(
        "{} pmg files ({plain_files} plain, {extra_record_files} with extra metadata records), {} pmd files",
        pmg.len(),
        pmd.len()
    );
    assert!(!pmg.is_empty());

    // 1. Every pmd has a pmg of the same stem, and all pmd are version 4.
    let mut versions: BTreeMap<usize, usize> = BTreeMap::new();
    for (path, version) in &pmd {
        *versions.entry(*version).or_default() += 1;
        assert!(
            pmg.contains_key(&path.replace(".pmd", ".pmg")),
            "{path} has no pmg"
        );
    }
    println!("pmd versions: {versions:?}");

    // 2. Every pmg parses; stored piece bounds agree with the vertices.
    let mut models = BTreeMap::new();
    let (mut pieces, mut vertices, mut bound_mismatch) = (0usize, 0usize, 0usize);
    for (path, bytes) in &pmg {
        let model = parse_pmg(bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            PMG_VERSION
        );
        for p in &model.pieces {
            pieces += 1;
            vertices += p.positions.len();
            if p.positions.is_empty() {
                continue;
            }
            for axis in 0..3 {
                let min = p.positions.iter().map(|v| v[axis]).fold(f32::MAX, f32::min);
                let max = p.positions.iter().map(|v| v[axis]).fold(f32::MIN, f32::max);
                if (min - p.bounds_min[axis]).abs() > 1e-3
                    || (max - p.bounds_max[axis]).abs() > 1e-3
                {
                    bound_mismatch += 1;
                    break;
                }
            }
        }
        models.insert(path.clone(), model);
    }
    println!(
        "all {} pmg files parse: {pieces} pieces, {vertices} vertices, {bound_mismatch} pieces whose stored bounds differ from their vertices",
        models.len()
    );

    // 3. Lane width study. Only looks with a template, a known right-side
    //    lane count and shoulder space, a symmetric or right-half template and
    //    no repeat are eligible. See the format document for the reasoning.
    let (mut eligible, mut width_4_5, mut breaks_at_lane_multiples) = (0usize, 0usize, 0usize);
    let mut widths: BTreeMap<i32, usize> = BTreeMap::new();
    for unit in units.iter().filter(|u| u.class == "road_look") {
        let Some(template) = unit.value("template_right") else {
            continue;
        };
        let key = template
            .trim_matches('"')
            .trim_start_matches('/')
            .replace(".pmd", ".pmg");
        let Some(model) = models.get(&key) else {
            continue;
        };
        let (nl, nr) = (
            unit.values("lanes_left").len(),
            unit.values("lanes_right").len(),
        );
        let (Some(shl), Some(shr)) = (
            float(unit, "shoulder_space_left"),
            float(unit, "shoulder_space_right"),
        ) else {
            continue;
        };
        if nr == 0 || shr < 0.0 || unit.value("template_repeat").is_some() {
            continue;
        }
        let xs: Vec<f32> = model
            .pieces
            .iter()
            .flat_map(|p| p.positions.iter().map(|v| v[0]))
            .collect();
        let (xmin, xmax) = (
            xs.iter().copied().fold(f32::MAX, f32::min),
            xs.iter().copied().fold(f32::MIN, f32::max),
        );
        let half = xmin > -0.1;
        let symmetric = (xmin + xmax).abs() < 0.01 && nl == nr && (shl - shr).abs() < 1e-6;
        if !(half || symmetric) {
            continue;
        }
        eligible += 1;
        let w = (xmax - shr) / nr as f32;
        *widths.entry((w * 100.0).round() as i32).or_default() += 1;
        if (w - 4.5).abs() < 0.02 {
            width_4_5 += 1;
        }
        // Do vertices exist at every multiple of 4.5 up to the lane count?
        if (1..=nr).all(|k| xs.iter().any(|x| (x - 4.5 * k as f32).abs() < 0.01)) {
            breaks_at_lane_multiples += 1;
        }
    }
    println!(
        "lane width study: {eligible} eligible looks; (outer extent - shoulder space) / right lanes = 4.50 for {width_4_5}; vertices at every multiple of 4.5 up to the lane count for {breaks_at_lane_multiples}"
    );
    let mut top: Vec<_> = widths.into_iter().collect();
    top.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    println!(
        "most common values (hundredths): {:?}",
        &top[..top.len().min(8)]
    );
    assert!(eligible > 0);
}
