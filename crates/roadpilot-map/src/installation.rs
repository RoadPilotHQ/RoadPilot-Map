//! Loading the navigation model of a local game installation.
//!
//! This reads the official `.scs` archives of an installation **read-only**
//! and keeps everything in memory. It reads exactly what the navigation
//! graph needs: the map sectors of the game, the road look, lane type,
//! country, prefab model and traffic rule definitions, and the prefab
//! descriptors that the used prefab models reference.

use std::fmt;
use std::path::{Path, PathBuf};

use roadpilot_map_archive::Archive;
use roadpilot_map_formats::parse_base;
use roadpilot_map_formats::sii::{SiiUnit, parse_sii};
use roadpilot_map_model::{LaneNetwork, NavigationGraph, PrefabNetwork, RoadNetwork};

use crate::{
    LaneDefinitions, PrefabDefinitions, build_lane_network, build_prefab_network,
    build_road_network,
};

/// The supported games.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Game {
    /// Euro Truck Simulator 2.
    Ets2,
    /// American Truck Simulator.
    Ats,
}

impl Game {
    /// The identifier used in exports: `ets2` or `ats`.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Ets2 => "ets2",
            Self::Ats => "ats",
        }
    }

    /// Parses `ets2` or `ats` (lower case).
    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        match id {
            "ets2" => Some(Self::Ets2),
            "ats" => Some(Self::Ats),
            _ => None,
        }
    }

    /// The name of the game's map folder under `map/` in the archives.
    #[must_use]
    pub fn map_name(self) -> &'static str {
        match self {
            Self::Ets2 => "europe",
            Self::Ats => "usa",
        }
    }
}

/// Why an installation could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InstallationError {
    /// The folder is not an installation of the game (no `base_map.scs`, no
    /// map data, or it cannot be read).
    NotRecognized(String),
    /// Reading or parsing game data failed.
    Processing(String),
}

impl fmt::Display for InstallationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRecognized(m) | Self::Processing(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for InstallationError {}

fn processing(what: &str, e: impl fmt::Display) -> InstallationError {
    InstallationError::Processing(format!("{what}: {e}"))
}

/// The models built from one installation.
#[derive(Debug)]
pub struct InstallationModel {
    pub game: Game,
    pub network: RoadNetwork,
    pub lanes: LaneNetwork,
    pub prefabs: PrefabNetwork,
    pub graph: NavigationGraph,
    /// File names of the archives that were opened, in the order they were
    /// read.
    pub archives: Vec<String>,
    /// Number of `.scs` files that could not be opened as an archive and
    /// were skipped.
    pub skipped_archives: usize,
}

const DEFINITION_PREFIXES: [&str; 4] = ["road_look", "traffic_lane", "prefab", "traffic_rules"];

/// Opens every readable `.scs` archive of the folder, in file name order.
fn open_archives(dir: &Path) -> Result<(Vec<(String, Archive)>, usize), InstallationError> {
    let entries = std::fs::read_dir(dir).map_err(|e| {
        InstallationError::NotRecognized(format!("cannot read {} ({:?})", dir.display(), e.kind()))
    })?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("scs")))
        .collect();
    paths.sort();
    let (mut archives, mut skipped) = (Vec::new(), 0);
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match Archive::open(&path) {
            Ok(a) => archives.push((name, a)),
            Err(_) => skipped += 1,
        }
    }
    Ok((archives, skipped))
}

/// The definition units of every archive: the `.sii` files of `def/world`
/// that start with one of the prefixes, and `def/country.sii`, with
/// `@include` resolved inside the same archive.
fn definition_units(archives: &[(String, Archive)]) -> Result<Vec<SiiUnit>, InstallationError> {
    let mut units = Vec::new();
    for (name, archive) in archives {
        let mut files = Vec::new();
        if let Ok(children) = archive.list_dir("def/world") {
            files.extend(
                children
                    .iter()
                    .filter(|c| !c.is_dir && c.name.ends_with(".sii"))
                    .filter(|c| DEFINITION_PREFIXES.iter().any(|p| c.name.starts_with(p)))
                    .map(|c| format!("def/world/{}", c.name)),
            );
        }
        if archive.find("def/country.sii").ok().flatten().is_some() {
            files.push("def/country.sii".to_owned());
        }
        for file in files {
            let bytes = archive
                .read_file(&file)
                .map_err(|e| processing(&format!("{name}: {file}"), e))?;
            let base = file.rsplit_once('/').map_or("", |(d, _)| d).to_owned();
            let mut include = |p: &str| {
                let full = p
                    .strip_prefix('/')
                    .map_or_else(|| format!("{base}/{p}"), str::to_owned);
                archive.read_file(&full).ok()
            };
            units.extend(
                parse_sii(&bytes, &mut include)
                    .map_err(|e| processing(&format!("{name}: {file}"), e))?,
            );
        }
    }
    Ok(units)
}

/// Loads the road, lane, prefab and navigation model of one game.
///
/// The installation is only read. Archives that are not in a supported
/// format are skipped and counted in
/// [`InstallationModel::skipped_archives`].
///
/// # Errors
///
/// [`InstallationError::NotRecognized`] if the folder has no `base_map.scs`
/// or no map data of the game, and [`InstallationError::Processing`] if game
/// data cannot be read or the models reject it.
pub fn load_installation(dir: &Path, game: Game) -> Result<InstallationModel, InstallationError> {
    if !dir.join("base_map.scs").is_file() {
        return Err(InstallationError::NotRecognized(format!(
            "{} does not contain base_map.scs",
            dir.display()
        )));
    }
    let (archives, skipped_archives) = open_archives(dir)?;
    let map = format!("map/{}", game.map_name());

    let mut files = Vec::new();
    for (name, archive) in &archives {
        let Ok(children) = archive.list_dir(&map) else {
            continue;
        };
        for child in children.iter().filter(|c| c.name.ends_with(".base")) {
            let path = format!("{map}/{}", child.name);
            files.push(
                archive
                    .read_file(&path)
                    .map_err(|e| processing(&format!("{name}: {path}"), e))?,
            );
        }
    }
    if files.is_empty() {
        return Err(InstallationError::NotRecognized(format!(
            "{} has no {map} sector files; is it an installation of {}?",
            dir.display(),
            game.id()
        )));
    }
    let sectors = files
        .iter()
        .map(|f| parse_base(f))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| processing("sector file", e))?;

    let units = definition_units(&archives)?;
    let lane_defs =
        LaneDefinitions::from_units(&units).map_err(|e| processing("lane definitions", e))?;
    let prefab_defs =
        PrefabDefinitions::from_units(&units).map_err(|e| processing("prefab definitions", e))?;

    let network = build_road_network(&sectors).map_err(|e| processing("road network", e))?;
    let lanes = build_lane_network(&network, &sectors, &lane_defs)
        .map_err(|e| processing("lane network", e))?;
    // A descriptor comes from the last archive (in name order) that has it.
    let load = |path: &str| {
        archives
            .iter()
            .rev()
            .find_map(|(_, a)| a.read_file(path).ok())
    };
    let prefabs = build_prefab_network(&network, &sectors, &prefab_defs, load)
        .map_err(|e| processing("prefab network", e))?;
    let graph = NavigationGraph::build(&network, &lanes, &prefabs)
        .map_err(|e| processing("navigation graph", e))?;

    Ok(InstallationModel {
        game,
        network,
        lanes,
        prefabs,
        graph,
        archives: archives.into_iter().map(|(n, _)| n).collect(),
        skipped_archives,
    })
}

/// The template of every road look that has one: look name (`road.<name>`)
/// to the `template_right` path without the leading `/`, read from the road
/// look definitions of an installation. The installation is only read.
///
/// # Errors
///
/// [`InstallationError::NotRecognized`] if the folder has no `base_map.scs`,
/// and [`InstallationError::Processing`] if a definition cannot be read.
pub fn road_look_templates(
    dir: &Path,
) -> Result<std::collections::BTreeMap<String, String>, InstallationError> {
    if !dir.join("base_map.scs").is_file() {
        return Err(InstallationError::NotRecognized(format!(
            "{} does not contain base_map.scs",
            dir.display()
        )));
    }
    let (archives, _) = open_archives(dir)?;
    let units = definition_units(&archives)?;
    Ok(units
        .iter()
        .filter(|u| u.class == "road_look")
        .filter_map(|u| {
            let t = u.value("template_right")?.trim().trim_matches('"');
            Some((u.name.clone(), t.trim_start_matches('/').to_owned()))
        })
        .collect())
}
