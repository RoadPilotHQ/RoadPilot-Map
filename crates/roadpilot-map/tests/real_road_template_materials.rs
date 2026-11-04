//! Opt-in test: decode the road template descriptors (`.pmd`), join them with
//! the geometry (`.pmg`) and the material files (`.mat`) of a local ETS2 or
//! ATS installation, and measure what the materials can tell apart.
//!
//! Ignored by default and never run in CI. Reads the archives read-only,
//! copies nothing into the repository, and prints counts only. See
//! `docs/formats/scs-road-templates.md`.
//!
//! ```powershell
//! $env:ROADPILOT_MAP_GAME_DIR = "C:\Program Files (x86)\Steam\steamapps\common\Euro Truck Simulator 2"
//! cargo test --release -p roadpilot-map --test real_road_template_materials -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use roadpilot_map::formats::Token;
use roadpilot_map::formats::mat::{Material, parse_mat};
use roadpilot_map::formats::pmd::parse_pmd;
use roadpilot_map::formats::pmg::parse_pmg;
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

/// The class a material falls in by its own text. This is a *reading* of
/// effect and texture names, not a confirmed meaning.
fn class(m: &Material) -> &'static str {
    let decal = m.effect.contains("decal");
    let lines = m
        .textures
        .iter()
        .filter_map(|t| t.source.as_deref())
        .any(|s| s.contains("line"));
    match (decal, lines) {
        (true, true) => "decal with a line texture",
        (true, false) => "other decal",
        (false, _) if m.substance.is_some() => "surface with a substance",
        _ => "surface without a substance",
    }
}

#[test]
#[ignore = "reads a local game installation; set ROADPILOT_MAP_GAME_DIR and run with --ignored"]
fn joins_descriptors_geometry_and_materials_of_the_installation() {
    let dir = game_dir();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("readable")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("scs")))
        .collect();
    paths.sort();
    let archives: Vec<Archive> = paths.iter().filter_map(|p| Archive::open(p).ok()).collect();

    // Later archives override earlier ones.
    let mut pmd = BTreeMap::new();
    let mut pmg = BTreeMap::new();
    for archive in &archives {
        let mut files = Vec::new();
        walk(archive, "road_template", &mut files);
        for f in files {
            if f.ends_with(".pmd") {
                pmd.insert(f.clone(), archive.read_file(&f).expect("pmd readable"));
            } else if f.ends_with(".pmg") {
                pmg.insert(
                    f.clone(),
                    archive.read_primary_record(&f).expect("pmg readable"),
                );
            }
        }
    }
    let read_mat = |path: &str| {
        archives
            .iter()
            .rev()
            .find_map(|a| a.read_file(path.trim_start_matches('/')).ok())
    };

    let mut materials: BTreeMap<String, Material> = BTreeMap::new();
    let (mut parts, mut pieces, mut slots, mut looks, mut variants) = (0, 0, 0, 0, 0);
    let mut missing_mat = BTreeSet::new();
    let mut bad_mat = BTreeMap::new();
    let mut pieces_by_class: BTreeMap<&str, usize> = BTreeMap::new();
    let mut substances: BTreeMap<String, usize> = BTreeMap::new();
    let mut effects: BTreeMap<String, usize> = BTreeMap::new();
    // Templates with line decals whose edge-line surface material also lies
    // outside the outermost line (shoulder and lane share one material).
    let (mut with_lines, mut shared_across_edge) = (0usize, 0usize);
    let mut visibility_switches = 0usize;

    for (path, bytes) in &pmd {
        let d = parse_pmd(bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
        let g_path = path.replace(".pmd", ".pmg");
        let g = parse_pmg(pmg.get(&g_path).expect("paired pmg"))
            .unwrap_or_else(|e| panic!("{g_path}: {e}"));
        assert_eq!(d.unknown_word_3, 0, "{path}");
        assert_eq!(
            d.part_attributes.len(),
            g.parts.len(),
            "{path}: part counts"
        );
        // Parts cover the pieces in order, without gaps.
        let mut next = 0u32;
        for p in &g.parts {
            assert_eq!(p.first_piece, next, "{g_path}: parts are not contiguous");
            next += p.piece_count;
        }
        assert_eq!(
            next as usize,
            g.pieces.len(),
            "{g_path}: parts do not cover the pieces"
        );
        for (part, _) in g.parts.iter().enumerate() {
            for v in 0..d.variants.len() {
                assert!(
                    d.part_visible(v, part).is_some(),
                    "{path}: part {part} has no visibility"
                );
            }
            let states: BTreeSet<_> = (0..d.variants.len())
                .filter_map(|v| d.part_visible(v, part))
                .collect();
            if states.len() > 1 {
                visibility_switches += 1;
            }
        }
        for piece in &g.pieces {
            assert!(
                (piece.material as usize) < d.material_slots(),
                "{g_path}: material slot"
            );
        }
        parts += g.parts.len();
        pieces += g.pieces.len();
        slots += d.material_slots();
        looks += d.looks.len();
        variants += d.variants.len();
        assert!(
            d.looks.iter().all(|t| Token(*t).name().is_some()),
            "{path}: look tokens"
        );
        assert!(
            d.variants.iter().all(|t| Token(*t).name().is_some()),
            "{path}: variant tokens"
        );

        for row in &d.materials {
            for m in row {
                if materials.contains_key(m) || missing_mat.contains(m) || bad_mat.contains_key(m) {
                    continue;
                }
                match read_mat(m) {
                    None => {
                        missing_mat.insert(m.clone());
                    }
                    Some(b) => match parse_mat(&b) {
                        Ok(mat) => {
                            materials.insert(m.clone(), mat);
                        }
                        Err(e) => {
                            bad_mat.insert(m.clone(), e.to_string());
                        }
                    },
                }
            }
        }

        // Classes of pieces under the first look, and the edge question.
        let Some(look0) = d.materials.first() else {
            continue;
        };
        let mat_of = |slot: u32| look0.get(slot as usize).and_then(|p| materials.get(p));
        let mut line_x: f32 = 0.0;
        for piece in &g.pieces {
            let Some(m) = mat_of(piece.material) else {
                continue;
            };
            *pieces_by_class.entry(class(m)).or_default() += 1;
            if class(m) == "decal with a line texture" {
                for p in &piece.positions {
                    line_x = line_x.max(p[0].abs());
                }
            }
        }
        if line_x > 0.5 {
            with_lines += 1;
            let shared = g.pieces.iter().any(|piece| {
                mat_of(piece.material).is_some_and(|m| {
                    m.substance.as_deref() == Some("road") && !m.effect.contains("decal")
                }) && piece.positions.iter().any(|p| p[0].abs() < line_x - 0.5)
                    && piece.positions.iter().any(|p| p[0].abs() > line_x + 0.2)
            });
            if shared {
                shared_across_edge += 1;
            }
        }
    }
    for m in materials.values() {
        *substances
            .entry(m.substance.clone().unwrap_or_else(|| "(none)".to_owned()))
            .or_default() += 1;
        let family = m.effect.split('.').take(2).collect::<Vec<_>>().join(".");
        *effects.entry(family).or_default() += 1;
    }

    println!(
        "{} pmd: {looks} looks, {variants} variants, {slots} material slots, {parts} parts, {pieces} pieces; {visibility_switches} parts change visibility between variants",
        pmd.len()
    );
    println!(
        "{} distinct material files read and parsed, {} missing, {} unreadable",
        materials.len(),
        missing_mat.len(),
        bad_mat.len()
    );
    for (m, e) in bad_mat.iter().take(5) {
        println!("  unreadable {m}: {e}");
    }
    println!("substances: {substances:?}");
    println!("effect families: {effects:?}");
    println!("pieces by material class (first look): {pieces_by_class:?}");
    println!(
        "templates with line decals beyond 0.5: {with_lines}; of these, a non-decal `road` material reaches both inside and beyond the outermost line in {shared_across_edge}"
    );
    assert!(missing_mat.is_empty(), "every referenced material exists");
    assert!(bad_mat.is_empty(), "every referenced material parses");
}
