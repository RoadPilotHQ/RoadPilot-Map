//! Builder for small synthetic HashFS v2 archives.
//!
//! Archives are generated in code from the documented layout (see
//! `docs/formats/hashfs-v2.md`); no real game data is used. The layout
//! follows real archives: header, data blocks aligned to 16 bytes, then the
//! zlib-compressed entry table and metadata table. Directory listings are
//! generated from the file paths.
//!
//! Tests that need broken archives change the uncompressed tables through
//! [`TestArchive::build_with`] or patch the finished bytes.

#![allow(dead_code)] // Each test file uses a different part of the builder.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use miniz_oxide::deflate::compress_to_vec_zlib;
use roadpilot_map_archive::path_hash;

pub const HEADER_LEN: usize = 53;
pub const META_PLAIN: u8 = 128;
pub const META_DIRECTORY: u8 = 129;
pub const FLAG_DIRECTORY: u16 = 0x1;

/// How a file's data is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Stored,
    Zlib,
}

/// One entry table row, uncompressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub hash: u64,
    pub meta_index: u32,
    pub meta_count: u16,
    pub flags: u16,
}

/// The uncompressed index tables, open for changes before they are written.
#[derive(Debug, Clone)]
pub struct Tables {
    pub rows: Vec<Row>,
    pub words: Vec<u32>,
}

impl Tables {
    /// The row of the entry with path `path`.
    pub fn row_mut(&mut self, path: &str) -> &mut Row {
        let hash = path_hash(path).expect("valid test path");
        self.rows
            .iter_mut()
            .find(|r| r.hash == hash)
            .expect("the test archive has this path")
    }

    /// The four payload words of a plain file or directory entry
    /// (`stored | method << 28`, size, reserved, offset / 16).
    pub fn payload_mut(&mut self, path: &str) -> &mut [u32] {
        let row = *self.row_mut(path);
        let header = self.words[row.meta_index as usize];
        let start = (header & 0x00FF_FFFF) as usize;
        &mut self.words[start..start + 4]
    }

    /// The first metadata header word of an entry.
    pub fn header_word_mut(&mut self, path: &str) -> &mut u32 {
        let row = *self.row_mut(path);
        &mut self.words[row.meta_index as usize]
    }
}

enum Content {
    /// Plain file or listing data, stored as `method`.
    Data {
        meta_type: u8,
        bytes: Vec<u8>,
        method: Method,
    },
    /// Arbitrary metadata records `(type, payload words)` without data.
    Records(Vec<(u8, Vec<u32>)>),
}

struct Item {
    hash: u64,
    flags: u16,
    content: Content,
}

/// A synthetic archive under construction.
#[derive(Default)]
pub struct TestArchive {
    files: BTreeMap<String, (Vec<u8>, Method)>,
    unlisted_dirs: Vec<String>,
    /// Raw names removed from / added to generated listings: `(dir, name)`.
    hidden_names: Vec<(String, String)>,
    added_names: Vec<(String, String)>,
    extra: Vec<Item>,
}

impl TestArchive {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a stored file. Parent directories and listings are generated.
    pub fn file(self, path: &str, data: &[u8]) -> Self {
        self.file_with(path, data, Method::Stored)
    }

    /// Adds a zlib-compressed file.
    pub fn zlib_file(self, path: &str, data: &[u8]) -> Self {
        self.file_with(path, data, Method::Zlib)
    }

    pub fn file_with(mut self, path: &str, data: &[u8], method: Method) -> Self {
        self.files.insert(path.to_owned(), (data.to_vec(), method));
        self
    }

    /// Leaves out the listing entry of directory `dir` (`""` = root), as in
    /// real archives without a root listing.
    pub fn without_listing(mut self, dir: &str) -> Self {
        self.unlisted_dirs.push(dir.to_owned());
        self
    }

    /// Removes `path` from its parent's listing. The entry itself stays, so
    /// it is only reachable through its hash, like an orphaned subtree.
    pub fn hide_in_listing(mut self, path: &str) -> Self {
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
        let is_dir = self
            .files
            .keys()
            .any(|f| f.starts_with(&format!("{path}/")));
        let raw = if is_dir {
            format!("/{name}")
        } else {
            name.to_owned()
        };
        self.hidden_names.push((dir.to_owned(), raw));
        self
    }

    /// Adds a raw name (`/` prefix for a subdirectory) to the generated
    /// listing of `dir`, whether or not an entry with that path exists.
    pub fn add_listing_name(mut self, dir: &str, raw_name: &str) -> Self {
        self.added_names.push((dir.to_owned(), raw_name.to_owned()));
        self
    }

    /// Adds a stored file entry that no listing names.
    pub fn unlisted_file(mut self, hash: u64, data: &[u8]) -> Self {
        self.extra.push(Item {
            hash,
            flags: 0,
            content: Content::Data {
                meta_type: META_PLAIN,
                bytes: data.to_vec(),
                method: Method::Stored,
            },
        });
        self
    }

    /// Adds an entry with arbitrary metadata records and no data, named in
    /// no listing.
    pub fn records(mut self, hash: u64, flags: u16, records: Vec<(u8, Vec<u32>)>) -> Self {
        self.extra.push(Item {
            hash,
            flags,
            content: Content::Records(records),
        });
        self
    }

    /// Builds the archive bytes.
    pub fn build(self) -> Vec<u8> {
        self.build_with(|_| {})
    }

    /// Builds the archive, letting `change` edit the uncompressed tables
    /// before they are compressed and written.
    pub fn build_with(self, change: impl FnOnce(&mut Tables)) -> Vec<u8> {
        let mut items = self.directory_items();
        for (path, (bytes, method)) in &self.files {
            items.push(Item {
                hash: path_hash(path).expect("valid test path"),
                flags: 0,
                content: Content::Data {
                    meta_type: META_PLAIN,
                    bytes: bytes.clone(),
                    method: *method,
                },
            });
        }
        items.extend(self.extra);
        items.sort_by_key(|i| i.hash);

        let mut out = vec![0u8; HEADER_LEN];
        let mut tables = Tables {
            rows: Vec::new(),
            words: Vec::new(),
        };
        for item in &items {
            let meta_index = tables.words.len() as u32;
            match &item.content {
                Content::Data {
                    meta_type,
                    bytes,
                    method,
                } => {
                    align16(&mut out);
                    let offset = out.len();
                    let stored = match method {
                        Method::Stored => bytes.clone(),
                        Method::Zlib => compress_to_vec_zlib(bytes, 6),
                    };
                    out.extend_from_slice(&stored);
                    let method_bits = match method {
                        Method::Stored => 0u32,
                        Method::Zlib => 1u32,
                    };
                    tables
                        .words
                        .push((u32::from(*meta_type) << 24) | (meta_index + 1));
                    tables.words.extend([
                        stored.len() as u32 | method_bits << 28,
                        bytes.len() as u32,
                        0,
                        (offset / 16) as u32,
                    ]);
                    tables.rows.push(Row {
                        hash: item.hash,
                        meta_index,
                        meta_count: 1,
                        flags: item.flags,
                    });
                }
                Content::Records(records) => {
                    let mut payload_at = meta_index as usize + records.len();
                    for (meta_type, payload) in records {
                        tables
                            .words
                            .push((u32::from(*meta_type) << 24) | payload_at as u32);
                        payload_at += payload.len();
                    }
                    for (_, payload) in records {
                        tables.words.extend(payload);
                    }
                    tables.rows.push(Row {
                        hash: item.hash,
                        meta_index,
                        meta_count: records.len() as u16,
                        flags: item.flags,
                    });
                }
            }
        }

        change(&mut tables);

        let mut rows = Vec::new();
        for r in &tables.rows {
            rows.extend_from_slice(&r.hash.to_le_bytes());
            rows.extend_from_slice(&r.meta_index.to_le_bytes());
            rows.extend_from_slice(&r.meta_count.to_le_bytes());
            rows.extend_from_slice(&r.flags.to_le_bytes());
        }
        let words: Vec<u8> = tables.words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let rows_z = compress_to_vec_zlib(&rows, 6);
        let words_z = compress_to_vec_zlib(&words, 6);
        align16(&mut out);
        let rows_at = out.len();
        out.extend_from_slice(&rows_z);
        align16(&mut out);
        let words_at = out.len();
        out.extend_from_slice(&words_z);

        out[..4].copy_from_slice(b"SCS#");
        out[4..6].copy_from_slice(&2u16.to_le_bytes());
        out[6..8].copy_from_slice(&0u16.to_le_bytes());
        out[8..12].copy_from_slice(b"CITY");
        out[12..16].copy_from_slice(&(tables.rows.len() as u32).to_le_bytes());
        out[16..20].copy_from_slice(&(rows_z.len() as u32).to_le_bytes());
        out[20..24].copy_from_slice(&(tables.words.len() as u32).to_le_bytes());
        out[24..28].copy_from_slice(&(words_z.len() as u32).to_le_bytes());
        out[28..36].copy_from_slice(&(rows_at as u64).to_le_bytes());
        out[36..44].copy_from_slice(&(words_at as u64).to_le_bytes());
        out[44..52].copy_from_slice(&0x80u64.to_le_bytes());
        out[52] = 0;
        out
    }

    /// One listing entry per directory, generated from the file paths.
    fn directory_items(&self) -> Vec<Item> {
        let mut dirs: BTreeMap<String, Vec<String>> = BTreeMap::new();
        dirs.entry(String::new()).or_default();
        for path in self.files.keys() {
            let mut parent = String::new();
            let parts: Vec<&str> = path.split('/').collect();
            for (i, part) in parts.iter().enumerate() {
                let is_last = i + 1 == parts.len();
                let child = if is_last {
                    (*part).to_owned()
                } else {
                    format!("/{part}")
                };
                let names = dirs.entry(parent.clone()).or_default();
                if !names.contains(&child) {
                    names.push(child);
                }
                if !is_last {
                    parent = if parent.is_empty() {
                        (*part).to_owned()
                    } else {
                        format!("{parent}/{part}")
                    };
                    dirs.entry(parent.clone()).or_default();
                }
            }
        }
        dirs.into_iter()
            .filter(|(dir, _)| !self.unlisted_dirs.contains(dir))
            .map(|(dir, mut names)| {
                names.retain(|n| !self.hidden_names.contains(&(dir.clone(), n.clone())));
                for (d, n) in &self.added_names {
                    if *d == dir && !names.contains(n) {
                        names.push(n.clone());
                    }
                }
                names.sort();
                Item {
                    hash: path_hash(&dir).expect("valid test path"),
                    flags: FLAG_DIRECTORY,
                    content: Content::Data {
                        meta_type: META_DIRECTORY,
                        bytes: listing(&names),
                        method: Method::Zlib,
                    },
                }
            })
            .collect()
    }
}

/// Encodes a HashFS v2 directory listing.
pub fn listing(names: &[String]) -> Vec<u8> {
    let mut v = (names.len() as u32).to_le_bytes().to_vec();
    v.extend(names.iter().map(|n| n.len() as u8));
    for n in names {
        v.extend_from_slice(n.as_bytes());
    }
    v
}

fn align16(out: &mut Vec<u8>) {
    while out.len() % 16 != 0 {
        out.push(0);
    }
}

/// An archive file in a fresh temporary folder, deleted when dropped.
pub struct TempArchive {
    dir: PathBuf,
    pub path: PathBuf,
}

impl TempArchive {
    /// Writes `bytes` to `<temp>/roadpilot-map-archive-<pid>-<name>/test.scs`.
    pub fn new(name: &str, bytes: &[u8]) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "roadpilot-map-archive-{}-{name}",
            std::process::id()
        ));
        if dir.exists() {
            fs::remove_dir_all(&dir).expect("remove leftover test folder");
        }
        fs::create_dir_all(&dir).expect("create test folder");
        let path = dir.join("test.scs");
        fs::write(&path, bytes).expect("write test archive");
        Self { dir, path }
    }
}

impl Drop for TempArchive {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// The small archive most tests start from.
///
/// ```text
/// /                      (root listing)
/// ├── readme.txt         stored
/// └── def/
///     ├── a.txt          stored
///     ├── b.bin          zlib
///     └── world/
///         └── road.sii   zlib
/// ```
pub fn sample() -> TestArchive {
    TestArchive::new()
        .file("readme.txt", b"synthetic test archive\n")
        .file("def/a.txt", b"alpha")
        .zlib_file("def/b.bin", &pattern(4096))
        .zlib_file(
            "def/world/road.sii",
            b"synthetic text, repeated. synthetic text, repeated.",
        )
}

/// Deterministic, compressible synthetic bytes.
pub fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 61) as u8).collect()
}
