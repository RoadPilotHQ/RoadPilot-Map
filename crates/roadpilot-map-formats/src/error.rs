//! Error type of the parsers.

use std::fmt;

/// Result type of this crate.
pub type Result<T> = std::result::Result<T, ParseError>;

/// Why a sector file could not be parsed.
///
/// Offsets are byte positions in the parsed file. Parsing stops at the first
/// error; no partial result is returned.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseError {
    /// The input is empty.
    Empty,
    /// The input ends before a field could be read.
    Truncated {
        /// Where the field starts.
        offset: usize,
        /// Bytes the field needs.
        needed: usize,
        /// What was being read.
        what: &'static str,
    },
    /// The header names a format version other than [`SUPPORTED_VERSION`](crate::SUPPORTED_VERSION).
    UnsupportedVersion {
        /// The version in the file.
        found: u32,
    },
    /// A count field promises more elements than the remaining bytes can
    /// hold, or its byte size overflows.
    InvalidCount {
        /// Where the count field starts.
        offset: usize,
        /// The count read from the file.
        count: u64,
        /// What the count is for.
        what: &'static str,
    },
    /// An item has a type whose layout is not known. Item records have no
    /// length field, so parsing cannot continue past it.
    UnknownItemType {
        /// Position of the item in the item list.
        index: usize,
        /// Where the item starts.
        offset: usize,
        /// The type value.
        item_type: u32,
    },
    /// A UID that must refer to something is zero.
    ZeroUid {
        /// Where the UID field starts.
        offset: usize,
        /// Which UID.
        what: &'static str,
    },
    /// A record has values that contradict the format.
    InvalidRecord {
        /// Where the record starts.
        offset: usize,
        /// What is wrong.
        reason: &'static str,
    },
    /// Bytes remain after the last structure of the file.
    TrailingBytes {
        /// Where the unexpected bytes start.
        offset: usize,
    },
    /// A value is larger than the configured [`Limits`](crate::Limits).
    LimitExceeded {
        /// What was checked.
        what: &'static str,
        /// The value found.
        value: u64,
        /// The configured maximum.
        limit: u64,
    },
    /// A `.data` entry for an item of the `.base` file was not found where
    /// expected.
    DataEntryMissing {
        /// Position of the entry in the expected entry list.
        index: usize,
        /// UID of the item the entry belongs to.
        item_uid: u64,
        /// Where the entry was expected.
        offset: usize,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the sector file is empty"),
            Self::Truncated {
                offset,
                needed,
                what,
            } => write!(
                f,
                "the file ends inside {what} at offset {offset} ({needed} bytes needed)"
            ),
            Self::UnsupportedVersion { found } => write!(
                f,
                "sector format version {found} is not supported (only {})",
                crate::SUPPORTED_VERSION
            ),
            Self::InvalidCount {
                offset,
                count,
                what,
            } => write!(
                f,
                "{what} count {count} at offset {offset} does not fit in the file"
            ),
            Self::UnknownItemType {
                index,
                offset,
                item_type,
            } => write!(
                f,
                "item {index} at offset {offset} has unknown type {item_type}"
            ),
            Self::ZeroUid { offset, what } => write!(f, "{what} at offset {offset} is zero"),
            Self::InvalidRecord { offset, reason } => {
                write!(f, "invalid record at offset {offset}: {reason}")
            }
            Self::TrailingBytes { offset } => {
                write!(
                    f,
                    "unexpected bytes after the end of the data at offset {offset}"
                )
            }
            Self::LimitExceeded { what, value, limit } => {
                write!(f, "{what} is {value}, the limit is {limit}")
            }
            Self::DataEntryMissing {
                index,
                item_uid,
                offset,
            } => write!(
                f,
                "data entry {index} for item {item_uid:016x} was not found at offset {offset}"
            ),
        }
    }
}

impl std::error::Error for ParseError {}
