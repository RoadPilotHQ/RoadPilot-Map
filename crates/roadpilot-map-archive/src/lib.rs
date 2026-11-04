//! Minimal, read-only reader for SCS **HashFS v2** archives (`.scs`).
//!
//! RoadPilot-Map needs only a few selected files from the game archives (see
//! `docs/research/scs-map-data-inventory.md`). This crate opens an archive,
//! validates its index, finds entries by path and reads single entries into
//! memory. It does not extract archives, write files or know anything about
//! map data.
//!
//! ```no_run
//! use roadpilot_map_archive::Archive;
//!
//! # fn main() -> Result<(), roadpilot_map_archive::Error> {
//! let archive = Archive::open("def.scs")?;
//! let bytes = archive.read_file("def/world/road_look.sii")?;
//! for child in archive.list_dir("def/world")? {
//!     println!("{} (directory: {})", child.name, child.is_dir);
//! }
//! # let _ = bytes;
//! # Ok(())
//! # }
//! ```
//!
//! # Scope
//!
//! - HashFS v2 with hash method `CITY` (CityHash64 v1.0.3), salt 0 and
//!   platform 0, as used by all official ETS2 and ATS archives.
//! - Plain files and directory listings, stored (method 0) or zlib-compressed
//!   (method 1).
//! - Not supported: HashFS v1, ZIP, salted archives, texture objects and
//!   GDeflate (method 3), entries with extra metadata records, unknown entry
//!   flags. Such archives or entries are reported with a typed [`Error`].
//!
//! # Path lookup
//!
//! HashFS stores only path hashes. A hash match alone does not prove that an
//! entry has the requested path, so [`Archive::find`] also checks that the
//! parent directory's listing names the entry ([`PathCheck`]).
//! [`Archive::read_file`] and [`Archive::list_dir`] only accept confirmed
//! paths.
//!
//! # Robustness
//!
//! Archive contents are untrusted. All offsets and sizes are checked against
//! the file and the [`Limits`] before memory is allocated, arithmetic is
//! checked, and malformed input gives an error instead of a panic. The
//! archive file is opened read-only and only the index and requested entries
//! are read.

mod error;
mod hash;
mod index;
mod inflate;
mod listing;
mod path;

use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

pub use error::{Error, Result};
pub use path::path_hash;

use index::{DataBlock, ReadAt};
use inflate::MAX_RATIO;

/// Upper bounds applied while reading an archive.
///
/// The defaults are far above anything in the official game archives (the
/// largest has about 182,000 entries) but stop runaway allocations caused by
/// malformed values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest accepted number of index entries.
    pub max_entries: u32,
    /// Largest accepted size of one index table, compressed or uncompressed.
    pub max_index_bytes: u64,
    /// Largest accepted size of one entry, compressed or uncompressed.
    pub max_file_size: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entries: 4 * 1024 * 1024,
            max_index_bytes: 256 * 1024 * 1024,
            max_file_size: 256 * 1024 * 1024,
        }
    }
}

/// What an index entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A plain file that this reader can read.
    File,
    /// A directory; its data is a listing of child names.
    Directory,
    /// An entry whose metadata this reader does not read, such as a texture
    /// object, a file with extra metadata records, or unknown flags.
    Unsupported,
}

/// One entry of an archive's index.
///
/// Entries only carry what the index stores: a path hash, not a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    index: u32,
    hash: u64,
    kind: EntryKind,
    flags: u16,
    data: Option<DataBlock>,
    /// A plain data record accompanied by records of types 5, 6 or 7.
    extra_records: bool,
}

impl Entry {
    /// The CityHash64 path hash stored in the index.
    #[must_use]
    pub fn hash(&self) -> u64 {
        self.hash
    }

    /// What the entry is.
    #[must_use]
    pub fn kind(&self) -> EntryKind {
        self.kind
    }

    /// Uncompressed size in bytes, as declared by the metadata. `None` for
    /// [`EntryKind::Unsupported`] entries without a known data record.
    #[must_use]
    pub fn size(&self) -> Option<u32> {
        self.data.map(|d| d.size)
    }
}

/// How well a found entry's path is confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathCheck {
    /// The root directory (the empty path). It has no parent to check.
    Root,
    /// The parent directory listing names the entry, with the matching
    /// file or directory marker.
    Listed,
    /// The parent directory listing exists but does not name the entry. The
    /// hash match is a collision or the listing is incomplete.
    NotListed,
    /// No readable parent directory exists, so the name cannot be checked.
    /// Real archives occasionally contain such unlisted entries.
    NoParentListing,
}

/// Result of [`Archive::find`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoundEntry {
    /// The entry with the requested path hash.
    pub entry: Entry,
    /// Whether the path is confirmed by its parent directory listing.
    pub check: PathCheck,
}

impl FoundEntry {
    /// `true` if the path is confirmed ([`PathCheck::Root`] or [`PathCheck::Listed`]).
    #[must_use]
    pub fn is_verified(&self) -> bool {
        matches!(self.check, PathCheck::Root | PathCheck::Listed)
    }
}

/// One child of a directory, as returned by [`Archive::list_dir`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Name of the child, without any directory marker.
    pub name: String,
    /// The listing marks the child as a subdirectory.
    pub is_dir: bool,
}

/// An open HashFS v2 archive.
///
/// Opening reads and validates the header and the complete index. Entry data
/// is read only on request. The archive can be shared between threads.
#[derive(Debug)]
pub struct Archive {
    file: Mutex<File>,
    file_len: u64,
    limits: Limits,
    entries: Vec<Entry>,
    /// `(hash, index)` pairs sorted by hash, for lookups.
    by_hash: Vec<(u64, u32)>,
    /// Names of the most recently checked directory listing, so that many
    /// lookups in one directory do not decode its listing every time.
    listing_cache: Mutex<Option<(u32, Arc<HashSet<String>>)>>,
}

impl ReadAt for Mutex<File> {
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        // A poisoned lock only means another thread panicked while reading;
        // every read seeks first, so the file handle is still usable.
        let mut file = self.lock().unwrap_or_else(PoisonError::into_inner);
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buf)?;
        Ok(())
    }
}

/// Outcome of looking up a hash in the index.
enum Lookup<'a> {
    Missing,
    One(&'a Entry),
    Several,
}

impl Archive {
    /// Opens an archive read-only with the default [`Limits`].
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the file cannot be read, an "unsupported" variant
    /// such as [`Error::NotHashFs`] or [`Error::UnsupportedVersion`] if it is
    /// not a supported HashFS v2 archive, and a "malformed" variant such as
    /// [`Error::InvalidIndex`] or [`Error::InvalidEntry`] if its header or
    /// index is inconsistent.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_limits(path, Limits::default())
    }

    /// Opens an archive read-only with custom [`Limits`].
    ///
    /// # Errors
    ///
    /// As [`Archive::open`], plus [`Error::LimitExceeded`] when the index is
    /// larger than `limits` allow.
    pub fn open_with_limits(path: impl AsRef<Path>, limits: Limits) -> Result<Self> {
        let file = File::open(path)?;
        let file_len = file.metadata()?.len();
        let file = Mutex::new(file);
        let entries = index::read_index(&file, file_len, &limits)?;
        let mut by_hash: Vec<(u64, u32)> = entries.iter().map(|e| (e.hash, e.index)).collect();
        by_hash.sort_unstable();
        Ok(Self {
            file,
            file_len,
            limits,
            entries,
            by_hash,
            listing_cache: Mutex::new(None),
        })
    }

    /// All index entries, in the order of the archive's entry table.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Finds the entry with the path `path`.
    ///
    /// The path is normalized first (see [`path_hash`]). Returns `Ok(None)` if
    /// no entry has its hash. A found entry comes with a [`PathCheck`] that
    /// says whether the parent directory listing confirms the name.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidPath`] for paths that break the normalization rules,
    /// [`Error::AmbiguousPath`] if several entries share the hash, and the
    /// read errors of [`Archive::read`] if the parent listing is unreadable.
    pub fn find(&self, path: &str) -> Result<Option<FoundEntry>> {
        let path = path::normalize(path)?;
        let entry = match self.lookup(path::hash_normalized(path)) {
            Lookup::Missing => return Ok(None),
            Lookup::Several => {
                return Err(Error::AmbiguousPath {
                    path: path.to_owned(),
                });
            }
            Lookup::One(entry) => *entry,
        };
        let check = if path.is_empty() {
            if entry.kind == EntryKind::Directory {
                PathCheck::Root
            } else {
                PathCheck::NotListed
            }
        } else {
            self.check_listed(path, &entry)?
        };
        Ok(Some(FoundEntry { entry, check }))
    }

    /// Reads the complete data of a file entry into memory.
    ///
    /// The entry need not have a confirmed path; use this after
    /// [`Archive::find`] to read an entry whose [`PathCheck`] you accept.
    ///
    /// # Errors
    ///
    /// [`Error::ForeignEntry`] for entries of another archive,
    /// [`Error::NotAFile`] for directories, [`Error::UnsupportedEntry`] and
    /// [`Error::UnsupportedCompression`] for data this reader cannot decode,
    /// [`Error::LimitExceeded`] for entries above the size limit,
    /// [`Error::CorruptData`] if the data does not match its metadata, and
    /// [`Error::Io`] if reading fails.
    pub fn read(&self, entry: &Entry) -> Result<Vec<u8>> {
        self.check_own(entry)?;
        match entry.kind {
            EntryKind::File => self.read_data(entry),
            EntryKind::Directory => Err(Error::NotAFile { hash: entry.hash }),
            EntryKind::Unsupported => Err(Error::UnsupportedEntry {
                hash: entry.hash,
                reason: index::unsupported_reason(entry),
            }),
        }
    }

    /// Finds and reads the file at `path`.
    ///
    /// Only paths confirmed by their parent directory listing are read.
    ///
    /// # Errors
    ///
    /// The errors of [`Archive::find`] and [`Archive::read`], plus
    /// [`Error::NotFound`] if no entry has the path and
    /// [`Error::UnverifiedPath`] if the path is not confirmed.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let found = self.find_verified(path)?;
        self.read(&found.entry)
    }

    /// Finds and reads the data record of a file whose metadata has extra
    /// records (types 5, 6 or 7, seen on `.pmg` model files).
    ///
    /// The extra records are **not** read and their meaning is unknown. The
    /// data record is checked like any other (size, Adler-32, limits), so
    /// the result is exactly the declared bytes of that record. Plain files
    /// are read as by [`Archive::read_file`].
    ///
    /// # Errors
    ///
    /// As [`Archive::read_file`]; [`Error::UnsupportedEntry`] for entries
    /// that are neither plain files nor plain files with extra records (for
    /// example texture objects).
    pub fn read_primary_record(&self, path: &str) -> Result<Vec<u8>> {
        let found = self.find_verified(path)?;
        let entry = found.entry;
        self.check_own(&entry)?;
        match entry.kind {
            EntryKind::File => self.read_data(&entry),
            EntryKind::Unsupported if entry.extra_records => self.read_data_checked(&entry),
            EntryKind::Directory => Err(Error::NotAFile { hash: entry.hash }),
            EntryKind::Unsupported => Err(Error::UnsupportedEntry {
                hash: entry.hash,
                reason: index::unsupported_reason(&entry),
            }),
        }
    }

    /// Lists the children of the directory at `path` (`""` for the root), in
    /// the order of the listing.
    ///
    /// # Errors
    ///
    /// As [`Archive::read_file`], plus [`Error::NotADirectory`] if the path is
    /// not a directory and [`Error::InvalidListing`] if the listing is
    /// malformed or contains an invalid name.
    pub fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>> {
        let found = self.find_verified(path)?;
        let entry = found.entry;
        if entry.kind != EntryKind::Directory {
            return Err(Error::NotADirectory { hash: entry.hash });
        }
        let data = self.read_data(&entry)?;
        let invalid = |reason| Error::InvalidListing {
            hash: entry.hash,
            reason,
        };
        listing::parse(&data)
            .map_err(invalid)?
            .into_iter()
            .map(|raw| {
                let (name, is_dir) = match raw.strip_prefix('/') {
                    Some(name) => (name, true),
                    None => (raw, false),
                };
                path::check_name(name).map_err(invalid)?;
                Ok(DirEntry {
                    name: name.to_owned(),
                    is_dir,
                })
            })
            .collect()
    }

    fn find_verified(&self, path: &str) -> Result<FoundEntry> {
        let normalized = path::normalize(path)?;
        let found = self.find(normalized)?.ok_or_else(|| Error::NotFound {
            path: normalized.to_owned(),
        })?;
        if !found.is_verified() {
            return Err(Error::UnverifiedPath {
                path: normalized.to_owned(),
                check: found.check,
            });
        }
        Ok(found)
    }

    fn lookup(&self, hash: u64) -> Lookup<'_> {
        let start = self.by_hash.partition_point(|&(h, _)| h < hash);
        let mut matches = self.by_hash[start..]
            .iter()
            .take_while(|&&(h, _)| h == hash)
            .filter_map(|&(_, i)| self.entries.get(i as usize));
        match (matches.next(), matches.next()) {
            (None, _) => Lookup::Missing,
            (Some(entry), None) => Lookup::One(entry),
            (Some(_), Some(_)) => Lookup::Several,
        }
    }

    /// Checks that the parent listing of the normalized, non-root `path`
    /// names `entry` with the right kind marker.
    fn check_listed(&self, path: &str, entry: &Entry) -> Result<PathCheck> {
        let (parent, name) = path::split_parent(path);
        let parent = match self.lookup(path::hash_normalized(parent)) {
            Lookup::One(parent) if parent.kind == EntryKind::Directory => *parent,
            _ => return Ok(PathCheck::NoParentListing),
        };
        let names = self.listing_names(&parent)?;
        let listed = if entry.kind == EntryKind::Directory {
            names.contains(&format!("/{name}"))
        } else {
            names.contains(name)
        };
        Ok(if listed {
            PathCheck::Listed
        } else {
            PathCheck::NotListed
        })
    }

    /// Raw names (with `/` markers) of a directory listing, cached for the
    /// most recent directory.
    fn listing_names(&self, dir: &Entry) -> Result<Arc<HashSet<String>>> {
        let mut cache = self
            .listing_cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some((index, names)) = cache.as_ref() {
            if *index == dir.index {
                return Ok(Arc::clone(names));
            }
        }
        let data = self.read_data(dir)?;
        let names: HashSet<String> = listing::parse(&data)
            .map_err(|reason| Error::InvalidListing {
                hash: dir.hash,
                reason,
            })?
            .into_iter()
            .map(str::to_owned)
            .collect();
        let names = Arc::new(names);
        *cache = Some((dir.index, Arc::clone(&names)));
        Ok(names)
    }

    fn check_own(&self, entry: &Entry) -> Result<()> {
        match self.entries.get(entry.index as usize) {
            Some(own) if own == entry => Ok(()),
            _ => Err(Error::ForeignEntry { hash: entry.hash }),
        }
    }

    /// Reads and decodes the data of a file or directory entry.
    fn read_data(&self, entry: &Entry) -> Result<Vec<u8>> {
        if entry.kind == EntryKind::Unsupported {
            return Err(Error::UnsupportedEntry {
                hash: entry.hash,
                reason: index::unsupported_reason(entry),
            });
        }
        self.read_data_checked(entry)
    }

    /// Reads the data record of an entry; the caller has decided that the
    /// entry kind allows it.
    fn read_data_checked(&self, entry: &Entry) -> Result<Vec<u8>> {
        let hash = entry.hash;
        let data = entry.data.ok_or(Error::UnsupportedEntry {
            hash,
            reason: index::unsupported_reason(entry),
        })?;
        if data.reserved != 0 {
            return Err(Error::UnsupportedEntry {
                hash,
                reason: "its reserved metadata word is not zero",
            });
        }
        let size = u64::from(data.size);
        let stored = u64::from(data.stored_size);
        limit("file size", size, self.limits.max_file_size)?;
        limit("stored file size", stored, self.limits.max_file_size)?;
        // Checked when the index was read; repeated because it guards the read.
        match data.offset.checked_add(stored) {
            Some(end) if end <= self.file_len => {}
            _ => {
                return Err(Error::InvalidEntry {
                    hash,
                    reason: "the data lies outside the archive",
                });
            }
        }
        let corrupt = |reason| Error::CorruptData {
            hash: Some(hash),
            reason,
        };
        // Both sizes are at most `max_file_size`, which fits in memory by
        // the caller's choice; convert without truncation.
        let to_usize = |v: u64| usize::try_from(v).map_err(|_| corrupt("the entry is too large"));
        match data.method {
            0 => {
                if stored != size {
                    return Err(corrupt(
                        "the stored size differs from the size, but the data is not compressed",
                    ));
                }
                let mut out = vec![0u8; to_usize(size)?];
                self.file.read_exact_at(data.offset, &mut out)?;
                Ok(out)
            }
            1 => {
                if size > stored.saturating_mul(MAX_RATIO) {
                    return Err(corrupt(
                        "the declared size is too large for the compressed data",
                    ));
                }
                let mut compressed = vec![0u8; to_usize(stored)?];
                self.file.read_exact_at(data.offset, &mut compressed)?;
                inflate::inflate_exact(&compressed, to_usize(size)?).map_err(corrupt)
            }
            method => Err(Error::UnsupportedCompression { hash, method }),
        }
    }
}

fn limit(what: &'static str, value: u64, limit: u64) -> Result<()> {
    if value > limit {
        Err(Error::LimitExceeded { what, value, limit })
    } else {
        Ok(())
    }
}
