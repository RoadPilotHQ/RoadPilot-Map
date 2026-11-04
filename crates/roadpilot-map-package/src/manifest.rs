//! The manifest of a map package (`manifest.json`).

use std::fmt::Write as _;

use crate::checksum::Fnv64;
use crate::json::{self, Json};
use crate::{FORMAT_NAME, POSITION_SCALE, SCHEMA_MAJOR};

/// Longest manifest the reader accepts.
pub const MAX_MANIFEST_BYTES: usize = 1 << 20;

/// What a file of the package is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// File name inside the package folder (no path separators).
    pub name: String,
    /// `geometry` or `navigation`.
    pub role: String,
    /// The format name inside the file.
    pub format: String,
    pub schema_major: u32,
    pub schema_minor: u32,
    pub bytes: u64,
    /// FNV-1a 64 checksum of the file, see [`crate::checksum`].
    pub checksum: u64,
}

/// Record counts of the package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub nodes: u64,
    pub roads: u64,
    pub lanes: u64,
    pub junctions: u64,
    pub connections: u64,
}

/// The manifest: everything a consumer needs to know before reading the
/// data files.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub schema_major: u32,
    pub schema_minor: u32,
    /// Revision of the generation logic. Increased when the same game data
    /// would give different content (for example a new curve model), so that
    /// consumers can discard caches.
    pub content_revision: u32,
    pub generator_name: String,
    pub generator_version: String,
    /// `ets2` or `ats`.
    pub game: String,
    /// The map folder (`europe` or `usa`); `None` if unknown.
    pub map: Option<String>,
    /// The game version; `None` if unknown (RoadPilot-Map does not read it).
    pub game_version: Option<String>,
    /// The archives that were read, in reading order; `None` if unknown.
    pub archives: Option<Vec<String>>,
    pub counts: Counts,
    /// Name of the curve model, for example `hermite_stored_length`.
    pub curve_model: String,
    /// Relative tolerance of `length_consistent` in road curves.
    pub length_tolerance: f64,
    /// Cell size, in world units, the spatial index of readers uses.
    pub spatial_cell_size: f64,
    /// Codes of known limitations of the data.
    pub limitations: Vec<String>,
    /// The data files; the geometry file is required.
    pub files: Vec<FileEntry>,
}

/// Why a manifest was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ManifestError {
    /// Not valid JSON, or a field is missing or has the wrong type.
    Invalid(String),
    /// The manifest is not a map package manifest.
    NotAPackage,
    /// The schema major version is not supported.
    UnsupportedVersion { major: i64 },
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(m) => write!(f, "invalid manifest: {m}"),
            Self::NotAPackage => f.write_str("not a RoadPilot-Map package manifest"),
            Self::UnsupportedVersion { major } => write!(
                f,
                "unsupported package schema version {major} (this reader supports {SCHEMA_MAJOR})"
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

fn invalid(m: impl Into<String>) -> ManifestError {
    ManifestError::Invalid(m.into())
}

/// Whether `name` is a plain file name: letters, digits, `.`, `_`, `-`, not
/// starting with a dot. This keeps a manifest from pointing outside the
/// package folder.
#[must_use]
pub fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

impl Manifest {
    /// The content id: a hash over the names, sizes and checksums of the
    /// files. Equal inputs give an equal id; any change of a file changes it.
    #[must_use]
    pub fn content_id(&self) -> u64 {
        let mut h = Fnv64::new();
        for f in &self.files {
            h.update(format!("{}:{}:{:x};", f.name, f.bytes, f.checksum).as_bytes());
        }
        h.finish()
    }

    /// The file entry with the given role.
    #[must_use]
    pub fn file(&self, role: &str) -> Option<&FileEntry> {
        self.files.iter().find(|f| f.role == role)
    }

    /// Serializes the manifest as one line of JSON ending in a newline. The
    /// output is deterministic.
    ///
    /// # Errors
    ///
    /// A message if a number is not finite or a file name is unsafe.
    pub fn to_json(&self) -> Result<String, String> {
        let mut o = String::with_capacity(1024);
        let _ = write!(
            o,
            "{{\"format\":\"{FORMAT_NAME}\",\"schema_version\":{{\"major\":{},\"minor\":{}}},\"content_revision\":{},",
            self.schema_major, self.schema_minor, self.content_revision
        );
        o.push_str("\"content_id\":");
        json::push_hex(&mut o, self.content_id());
        o.push_str(",\"generator\":{\"name\":");
        json::push_str(&mut o, &self.generator_name);
        o.push_str(",\"version\":");
        json::push_str(&mut o, &self.generator_version);
        o.push_str("},\"generated_at\":null,\"game\":");
        json::push_str(&mut o, &self.game);
        o.push_str(",\"source\":{\"map\":");
        push_opt_str(&mut o, self.map.as_deref());
        o.push_str(",\"game_version\":");
        push_opt_str(&mut o, self.game_version.as_deref());
        o.push_str(",\"archives\":");
        match &self.archives {
            Some(list) => {
                o.push('[');
                for (i, a) in list.iter().enumerate() {
                    if i > 0 {
                        o.push(',');
                    }
                    json::push_str(&mut o, a);
                }
                o.push(']');
            }
            None => o.push_str("null"),
        }
        let _ = write!(
            o,
            "}},\"coordinate_system\":{{\"x\":\"east\",\"y\":\"up\",\"z\":\"south\",\"position_scale\":{POSITION_SCALE},\"world_unit\":\"unconfirmed\"}},"
        );
        let _ = write!(
            o,
            "\"counts\":{{\"nodes\":{},\"roads\":{},\"lanes\":{},\"junctions\":{},\"connections\":{}}},",
            self.counts.nodes,
            self.counts.roads,
            self.counts.lanes,
            self.counts.junctions,
            self.counts.connections
        );
        o.push_str("\"geometry_models\":{\"node_position\":\"confirmed\",\"curve\":{\"model\":");
        json::push_str(&mut o, &self.curve_model);
        o.push_str(",\"quality\":\"estimated\",\"length_tolerance\":");
        if !json::push_f64(&mut o, self.length_tolerance) {
            return Err("length tolerance is not finite".to_owned());
        }
        o.push_str("},\"lane_position\":\"unavailable\"},\"spatial_index\":{\"kind\":\"uniform_grid\",\"cell_size\":");
        if !json::push_f64(&mut o, self.spatial_cell_size) {
            return Err("cell size is not finite".to_owned());
        }
        o.push_str("},\"distribution\":\"local_use_only\",\"limitations\":[");
        for (i, l) in self.limitations.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            json::push_str(&mut o, l);
        }
        o.push_str("],\"files\":[");
        for (i, f) in self.files.iter().enumerate() {
            if !is_safe_file_name(&f.name) {
                return Err(format!("unsafe file name {:?}", f.name));
            }
            if i > 0 {
                o.push(',');
            }
            o.push_str("{\"name\":");
            json::push_str(&mut o, &f.name);
            o.push_str(",\"role\":");
            json::push_str(&mut o, &f.role);
            o.push_str(",\"format\":");
            json::push_str(&mut o, &f.format);
            let _ = write!(
                o,
                ",\"schema_version\":{{\"major\":{},\"minor\":{}}},\"bytes\":{},\"checksum\":",
                f.schema_major, f.schema_minor, f.bytes
            );
            json::push_hex(&mut o, f.checksum);
            o.push('}');
        }
        o.push_str("]}\n");
        Ok(o)
    }

    /// Parses and checks a manifest.
    ///
    /// Checks the format name, the supported major version, field types, safe
    /// file names, the geometry file entry, and that `content_id` equals the
    /// id computed from the file entries. Unknown fields are ignored.
    ///
    /// # Errors
    ///
    /// [`ManifestError`].
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        if text.len() > MAX_MANIFEST_BYTES {
            return Err(invalid("the manifest is too large"));
        }
        let v = json::parse(text).map_err(invalid)?;
        if v.get("format").and_then(Json::as_str) != Some(FORMAT_NAME) {
            return Err(ManifestError::NotAPackage);
        }
        let sv = req(&v, "schema_version")?;
        let major = req(sv, "major")?
            .as_i64()
            .ok_or_else(|| invalid("schema_version.major must be an integer"))?;
        if major != i64::from(SCHEMA_MAJOR) {
            return Err(ManifestError::UnsupportedVersion { major });
        }
        let minor = uint32(sv, "minor")?;
        let generator = req(&v, "generator")?;
        let source = req(&v, "source")?;
        let counts = req(&v, "counts")?;
        let models = req(&v, "geometry_models")?;
        let curve = req(models, "curve")?;
        let spatial = req(&v, "spatial_index")?;
        let coordinate = req(&v, "coordinate_system")?;
        if coordinate.get("position_scale").and_then(Json::as_i64)
            != Some(i64::from(POSITION_SCALE))
        {
            return Err(invalid("position_scale is not the supported value"));
        }

        let mut files = Vec::new();
        for f in req(&v, "files")?
            .as_array()
            .ok_or_else(|| invalid("files must be an array"))?
        {
            let name = str_field(f, "name")?.to_owned();
            if !is_safe_file_name(&name) {
                return Err(invalid(format!("unsafe file name {name:?}")));
            }
            let fsv = req(f, "schema_version")?;
            files.push(FileEntry {
                name,
                role: str_field(f, "role")?.to_owned(),
                format: str_field(f, "format")?.to_owned(),
                schema_major: uint32(fsv, "major")?,
                schema_minor: uint32(fsv, "minor")?,
                bytes: uint64(f, "bytes")?,
                checksum: hex(f, "checksum")?,
            });
        }
        let manifest = Self {
            schema_major: SCHEMA_MAJOR,
            schema_minor: minor,
            content_revision: uint32(&v, "content_revision")?,
            generator_name: str_field(generator, "name")?.to_owned(),
            generator_version: str_field(generator, "version")?.to_owned(),
            game: str_field(&v, "game")?.to_owned(),
            map: opt_str(source, "map")?,
            game_version: opt_str(source, "game_version")?,
            archives: match req(source, "archives")? {
                Json::Null => None,
                Json::Arr(list) => Some(
                    list.iter()
                        .map(|a| {
                            a.as_str()
                                .map(str::to_owned)
                                .ok_or_else(|| invalid("archives must be strings"))
                        })
                        .collect::<Result<_, _>>()?,
                ),
                _ => return Err(invalid("archives must be an array or null")),
            },
            counts: Counts {
                nodes: uint64(counts, "nodes")?,
                roads: uint64(counts, "roads")?,
                lanes: uint64(counts, "lanes")?,
                junctions: uint64(counts, "junctions")?,
                connections: uint64(counts, "connections")?,
            },
            curve_model: str_field(curve, "model")?.to_owned(),
            length_tolerance: num(curve, "length_tolerance")?,
            spatial_cell_size: num(spatial, "cell_size")?,
            limitations: req(&v, "limitations")?
                .as_array()
                .ok_or_else(|| invalid("limitations must be an array"))?
                .iter()
                .map(|l| {
                    l.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| invalid("limitations must be strings"))
                })
                .collect::<Result<_, _>>()?,
            files,
        };
        if manifest.game != "ets2" && manifest.game != "ats" {
            return Err(invalid(format!("unknown game {:?}", manifest.game)));
        }
        if !(manifest.length_tolerance.is_finite() && manifest.length_tolerance >= 0.0) {
            return Err(invalid("length_tolerance must be finite and not negative"));
        }
        if !(manifest.spatial_cell_size.is_finite() && manifest.spatial_cell_size > 0.0) {
            return Err(invalid("cell_size must be finite and positive"));
        }
        if manifest.file("geometry").is_none() {
            return Err(invalid("the manifest lists no geometry file"));
        }
        let mut names: Vec<&str> = manifest.files.iter().map(|f| f.name.as_str()).collect();
        names.sort_unstable();
        if names.windows(2).any(|w| w[0] == w[1]) {
            return Err(invalid("a file is listed twice"));
        }
        if hex(&v, "content_id")? != manifest.content_id() {
            return Err(invalid("content_id does not match the file entries"));
        }
        Ok(manifest)
    }
}

fn push_opt_str(out: &mut String, v: Option<&str>) {
    match v {
        Some(s) => json::push_str(out, s),
        None => out.push_str("null"),
    }
}

fn req<'a>(obj: &'a Json, key: &str) -> Result<&'a Json, ManifestError> {
    obj.get(key)
        .ok_or_else(|| invalid(format!("missing field \"{key}\"")))
}

fn str_field<'a>(obj: &'a Json, key: &str) -> Result<&'a str, ManifestError> {
    req(obj, key)?
        .as_str()
        .ok_or_else(|| invalid(format!("\"{key}\" must be a string")))
}

fn opt_str(obj: &Json, key: &str) -> Result<Option<String>, ManifestError> {
    match req(obj, key)? {
        Json::Null => Ok(None),
        Json::Str(s) => Ok(Some(s.clone())),
        _ => Err(invalid(format!("\"{key}\" must be a string or null"))),
    }
}

fn uint64(obj: &Json, key: &str) -> Result<u64, ManifestError> {
    req(obj, key)?
        .as_i64()
        .and_then(|v| u64::try_from(v).ok())
        .ok_or_else(|| invalid(format!("\"{key}\" must be a non-negative integer")))
}

fn uint32(obj: &Json, key: &str) -> Result<u32, ManifestError> {
    u32::try_from(uint64(obj, key)?).map_err(|_| invalid(format!("\"{key}\" is too large")))
}

fn num(obj: &Json, key: &str) -> Result<f64, ManifestError> {
    req(obj, key)?
        .as_f64()
        .ok_or_else(|| invalid(format!("\"{key}\" must be a number")))
}

fn hex(obj: &Json, key: &str) -> Result<u64, ManifestError> {
    let s = str_field(obj, key)?;
    if s.is_empty()
        || s.len() > 16
        || !s
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(invalid(format!(
            "\"{key}\" must be lower-case hexadecimal of at most 16 digits"
        )));
    }
    u64::from_str_radix(s, 16).map_err(|_| invalid(format!("\"{key}\" is not hexadecimal")))
}

/// Cell size, in world units, of the spatial index written by this version.
pub const DEFAULT_SPATIAL_CELL_SIZE: f64 = 256.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SCHEMA_MINOR;

    pub(crate) fn sample() -> Manifest {
        Manifest {
            schema_major: SCHEMA_MAJOR,
            schema_minor: SCHEMA_MINOR,
            content_revision: 1,
            generator_name: "roadpilot-map".to_owned(),
            generator_version: "0.1.0".to_owned(),
            game: "ets2".to_owned(),
            map: Some("europe".to_owned()),
            game_version: None,
            archives: Some(vec!["base_map.scs".to_owned()]),
            counts: Counts {
                nodes: 3,
                roads: 2,
                lanes: 4,
                junctions: 0,
                connections: 1,
            },
            curve_model: "hermite_stored_length".to_owned(),
            length_tolerance: 0.001,
            spatial_cell_size: DEFAULT_SPATIAL_CELL_SIZE,
            limitations: vec!["lane_positions_are_unavailable".to_owned()],
            files: vec![FileEntry {
                name: "geometry.jsonl".to_owned(),
                role: "geometry".to_owned(),
                format: "roadpilot-map-geometry".to_owned(),
                schema_major: 1,
                schema_minor: 0,
                bytes: 123,
                checksum: 0xdead_beef,
            }],
        }
    }

    #[test]
    fn round_trips() {
        let m = sample();
        let text = m.to_json().expect("valid");
        assert!(text.ends_with('\n'));
        assert_eq!(Manifest::parse(&text), Ok(m.clone()));
        // Deterministic.
        assert_eq!(text, m.to_json().expect("valid"));
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let text = sample().to_json().expect("valid");
        let extended = text.replacen("\"files\":", "\"future\":{\"x\":1},\"files\":", 1);
        assert!(Manifest::parse(&extended).is_ok());
    }

    #[test]
    fn rejects_other_formats_and_versions() {
        let text = sample().to_json().expect("valid");
        assert_eq!(
            Manifest::parse(&text.replace(FORMAT_NAME, "something-else")),
            Err(ManifestError::NotAPackage)
        );
        assert_eq!(
            Manifest::parse(&text.replacen("\"major\":1", "\"major\":2", 1)),
            Err(ManifestError::UnsupportedVersion { major: 2 })
        );
        assert!(matches!(
            Manifest::parse("not json"),
            Err(ManifestError::Invalid(_))
        ));
    }

    #[test]
    fn rejects_tampered_content_ids_and_unsafe_names() {
        let text = sample().to_json().expect("valid");
        // Change the recorded size: the content id no longer matches.
        let tampered = text.replace("\"bytes\":123", "\"bytes\":124");
        assert!(matches!(
            Manifest::parse(&tampered),
            Err(ManifestError::Invalid(m)) if m.contains("content_id")
        ));
        for bad in ["../x.jsonl", "a/b", ".hidden", "", "c:evil", "a b"] {
            assert!(!is_safe_file_name(bad), "{bad:?}");
            let mut m = sample();
            m.files[0].name = bad.to_owned();
            assert!(m.to_json().is_err(), "{bad:?}");
        }
        assert!(is_safe_file_name("geometry.jsonl"));
    }

    #[test]
    fn requires_a_geometry_file_and_a_known_game() {
        let mut m = sample();
        m.files[0].role = "other".to_owned();
        let text = m.to_json().expect("valid");
        assert!(Manifest::parse(&text).is_err());
        let mut m = sample();
        m.game = "ets3".to_owned();
        assert!(Manifest::parse(&m.to_json().expect("valid")).is_err());
    }
}
