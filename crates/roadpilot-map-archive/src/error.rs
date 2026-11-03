//! Error type of the archive reader.

use std::fmt;
use std::io;

use crate::PathCheck;

/// Result type of this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong while opening an archive or reading from it.
///
/// Variants fall into four groups:
///
/// - **Not supported:** the file is not a HashFS v2 archive this reader
///   handles ([`NotHashFs`](Error::NotHashFs),
///   [`UnsupportedVersion`](Error::UnsupportedVersion), ...).
/// - **Malformed:** the file claims to be HashFS v2 but its structure is
///   broken ([`TruncatedHeader`](Error::TruncatedHeader),
///   [`InvalidIndex`](Error::InvalidIndex), [`InvalidEntry`](Error::InvalidEntry),
///   [`CorruptData`](Error::CorruptData), [`InvalidListing`](Error::InvalidListing)).
/// - **Lookup:** the request cannot be answered as asked
///   ([`InvalidPath`](Error::InvalidPath), [`NotFound`](Error::NotFound),
///   [`AmbiguousPath`](Error::AmbiguousPath), [`UnverifiedPath`](Error::UnverifiedPath), ...).
/// - **Environment:** [`Io`](Error::Io) and [`LimitExceeded`](Error::LimitExceeded).
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Reading the archive file failed.
    Io(io::Error),
    /// The file does not start with the `SCS#` signature.
    NotHashFs,
    /// The file is HashFS, but not version 2.
    UnsupportedVersion(u16),
    /// The path hash method is not `CITY` (CityHash64).
    UnsupportedHashMethod([u8; 4]),
    /// The header has a non-zero salt. How a salt changes path hashes is not
    /// known, so paths could not be looked up reliably.
    UnsupportedSalt(u16),
    /// The header names a platform other than `0` (PC).
    UnsupportedPlatform(u8),
    /// The file ends inside the 53-byte header.
    TruncatedHeader {
        /// Length of the file in bytes.
        len: u64,
    },
    /// The header or the index tables are inconsistent.
    InvalidIndex(&'static str),
    /// An entry of the index has inconsistent metadata.
    InvalidEntry {
        /// Path hash of the entry.
        hash: u64,
        /// What is wrong.
        reason: &'static str,
    },
    /// A value from the archive is larger than the configured [`Limits`](crate::Limits).
    LimitExceeded {
        /// The value that was checked.
        what: &'static str,
        /// The value found in the archive.
        value: u64,
        /// The configured maximum.
        limit: u64,
    },
    /// The requested path is not a valid archive path.
    InvalidPath {
        /// The path as given by the caller.
        path: String,
        /// What is wrong.
        reason: &'static str,
    },
    /// No entry has the hash of the requested path.
    NotFound {
        /// The normalized path.
        path: String,
    },
    /// Several entries share the hash of the requested path.
    AmbiguousPath {
        /// The normalized path.
        path: String,
    },
    /// An entry has the hash of the requested path, but its parent directory
    /// listing does not confirm the name. Use [`Archive::find`](crate::Archive::find)
    /// and [`Archive::read`](crate::Archive::read) to read it anyway.
    UnverifiedPath {
        /// The normalized path.
        path: String,
        /// Result of the listing check.
        check: PathCheck,
    },
    /// The entry is a directory, but a file was requested.
    NotAFile {
        /// Path hash of the entry.
        hash: u64,
    },
    /// The entry is not a directory, but a directory was requested.
    NotADirectory {
        /// Path hash of the entry.
        hash: u64,
    },
    /// The entry does not belong to this archive.
    ForeignEntry {
        /// Path hash of the entry.
        hash: u64,
    },
    /// The entry exists, but its metadata is of a kind this reader does not
    /// read (for example a texture object or unknown flags).
    UnsupportedEntry {
        /// Path hash of the entry.
        hash: u64,
        /// Why it cannot be read.
        reason: &'static str,
    },
    /// The entry uses a compression method other than stored (0) or zlib (1).
    UnsupportedCompression {
        /// Path hash of the entry.
        hash: u64,
        /// The 4-bit method value from the metadata.
        method: u8,
    },
    /// The data of an entry does not match its metadata.
    CorruptData {
        /// Path hash of the entry, or `None` for an index table.
        hash: Option<u64>,
        /// What is wrong.
        reason: &'static str,
    },
    /// A directory listing is malformed.
    InvalidListing {
        /// Path hash of the directory.
        hash: u64,
        /// What is wrong.
        reason: &'static str,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "reading the archive failed: {e}"),
            Self::NotHashFs => f.write_str("not a HashFS archive (no SCS# signature)"),
            Self::UnsupportedVersion(v) => {
                write!(f, "HashFS version {v} is not supported (only version 2)")
            }
            Self::UnsupportedHashMethod(m) => write!(
                f,
                "hash method {:?} is not supported (only CITY)",
                String::from_utf8_lossy(m)
            ),
            Self::UnsupportedSalt(s) => write!(f, "archives with salt {s} are not supported"),
            Self::UnsupportedPlatform(p) => write!(f, "archive platform {p} is not supported"),
            Self::TruncatedHeader { len } => {
                write!(
                    f,
                    "the HashFS v2 header is truncated (file has {len} bytes)"
                )
            }
            Self::InvalidIndex(reason) => write!(f, "invalid archive index: {reason}"),
            Self::InvalidEntry { hash, reason } => {
                write!(f, "invalid index entry {hash:016x}: {reason}")
            }
            Self::LimitExceeded { what, value, limit } => {
                write!(f, "{what} is {value}, the limit is {limit}")
            }
            Self::InvalidPath { path, reason } => write!(f, "invalid path {path:?}: {reason}"),
            Self::NotFound { path } => write!(f, "{path:?} is not in the archive"),
            Self::AmbiguousPath { path } => {
                write!(f, "several archive entries have the hash of {path:?}")
            }
            Self::UnverifiedPath { path, check } => write!(
                f,
                "an entry has the hash of {path:?}, but its directory listing does not confirm the name ({check:?})"
            ),
            Self::NotAFile { hash } => write!(f, "entry {hash:016x} is a directory, not a file"),
            Self::NotADirectory { hash } => write!(f, "entry {hash:016x} is not a directory"),
            Self::ForeignEntry { hash } => {
                write!(f, "entry {hash:016x} does not belong to this archive")
            }
            Self::UnsupportedEntry { hash, reason } => {
                write!(f, "entry {hash:016x} cannot be read: {reason}")
            }
            Self::UnsupportedCompression { hash, method } => write!(
                f,
                "entry {hash:016x} uses unsupported compression method {method}"
            ),
            Self::CorruptData {
                hash: Some(hash),
                reason,
            } => write!(f, "entry {hash:016x} is corrupt: {reason}"),
            Self::CorruptData { hash: None, reason } => {
                write!(f, "an index table is corrupt: {reason}")
            }
            Self::InvalidListing { hash, reason } => {
                write!(f, "directory listing {hash:016x} is invalid: {reason}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
