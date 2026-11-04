//! HashFS v2 header, index tables and entry metadata.
//!
//! Layout (little-endian), as verified by the SCS-Extractor research on 191
//! real archives:
//!
//! | Offset | Type | Field |
//! |---|---|---|
//! | 0 | `[u8; 4]` | Magic `SCS#` |
//! | 4 | `u16` | Version, 2 |
//! | 6 | `u16` | Salt, 0 in every known archive |
//! | 8 | `[u8; 4]` | Hash method `CITY` |
//! | 12 | `u32` | Entry count |
//! | 16 | `u32` | Entry table stored (zlib) size |
//! | 20 | `u32` | Metadata table length in `u32` words |
//! | 24 | `u32` | Metadata table stored (zlib) size |
//! | 28 | `u64` | Entry table offset |
//! | 36 | `u64` | Metadata table offset |
//! | 44 | `u64` | Unknown, `0x80` in every known archive. Not used. |
//! | 52 | `u8` | Platform, 0 (PC) |
//!
//! Entry table row (16 bytes): `u64` path hash, `u32` index of the first
//! metadata header word, `u16` number of header words, `u16` flags (bit 0:
//! directory).
//!
//! A metadata header word is `(type << 24) | payload_index`. This reader
//! understands exactly two record sets: a single type 128 record (plain file)
//! and a single type 129 record (directory listing). Both have a 4-word
//! payload: `stored_size | method << 28`, uncompressed size, a reserved word
//! (0 in every known archive) and `offset / 16`.

use crate::error::{Error, Result};
use crate::inflate::{MAX_RATIO, inflate_exact};
use crate::{Entry, EntryKind, Limits};

pub(crate) const HEADER_LEN: usize = 53;
const MAGIC: &[u8; 4] = b"SCS#";
const VERSION: u16 = 2;
const HASH_METHOD: &[u8; 4] = b"CITY";
const ROW_LEN: usize = 16;

const FLAG_DIRECTORY: u16 = 0x1;
/// Set on many entries; its meaning is unknown, but it was shown to have no
/// effect on how data is stored. Accepted and ignored.
const FLAG_BIT_2: u16 = 0x4;
const KNOWN_FLAGS: u16 = FLAG_DIRECTORY | FLAG_BIT_2;

const META_PLAIN: u32 = 128;
const META_DIRECTORY: u32 = 129;
const META_EXTRA_5: u32 = 5;
const META_EXTRA_6: u32 = 6;
const META_EXTRA_7: u32 = 7;
const PAYLOAD_WORDS: usize = 4;

/// Location and encoding of an entry's data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DataBlock {
    pub(crate) offset: u64,
    pub(crate) stored_size: u32,
    pub(crate) method: u8,
    pub(crate) size: u32,
    pub(crate) reserved: u32,
}

/// The parts of the header the reader needs.
#[derive(Debug, Clone, Copy)]
struct Header {
    entry_count: u32,
    entry_table: (u64, u32),
    metadata_words: u32,
    metadata_table: (u64, u32),
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(a)
}

fn limit(what: &'static str, value: u64, limit: u64) -> Result<()> {
    if value > limit {
        Err(Error::LimitExceeded { what, value, limit })
    } else {
        Ok(())
    }
}

/// Parses the header from the first bytes of the file (`head` holds at most
/// [`HEADER_LEN`] bytes, fewer if the file is shorter).
fn parse_header(head: &[u8], file_len: u64, limits: &Limits) -> Result<Header> {
    let prefix = head.len().min(MAGIC.len());
    if head.is_empty() || head[..prefix] != MAGIC[..prefix] {
        return Err(Error::NotHashFs);
    }
    if head.len() >= 6 && u16_at(head, 4) != VERSION {
        return Err(Error::UnsupportedVersion(u16_at(head, 4)));
    }
    if head.len() >= 12 && &head[8..12] != HASH_METHOD {
        let mut method = [0u8; 4];
        method.copy_from_slice(&head[8..12]);
        return Err(Error::UnsupportedHashMethod(method));
    }
    if head.len() < HEADER_LEN {
        return Err(Error::TruncatedHeader { len: file_len });
    }
    let salt = u16_at(head, 6);
    if salt != 0 {
        return Err(Error::UnsupportedSalt(salt));
    }
    if head[52] != 0 {
        return Err(Error::UnsupportedPlatform(head[52]));
    }

    let header = Header {
        entry_count: u32_at(head, 12),
        entry_table: (u64_at(head, 28), u32_at(head, 16)),
        metadata_words: u32_at(head, 20),
        metadata_table: (u64_at(head, 36), u32_at(head, 24)),
    };
    limit(
        "entry count",
        u64::from(header.entry_count),
        u64::from(limits.max_entries),
    )?;
    let entry_bytes = u64::from(header.entry_count) * ROW_LEN as u64;
    let metadata_bytes = u64::from(header.metadata_words) * 4;
    limit("entry table size", entry_bytes, limits.max_index_bytes)?;
    limit(
        "metadata table size",
        metadata_bytes,
        limits.max_index_bytes,
    )?;
    check_table(header.entry_table, entry_bytes, file_len, limits)?;
    check_table(header.metadata_table, metadata_bytes, file_len, limits)?;

    let (e_off, e_len) = header.entry_table;
    let (m_off, m_len) = header.metadata_table;
    // Both ranges lie inside the file (checked above), so the sums fit.
    if e_off < m_off + u64::from(m_len) && m_off < e_off + u64::from(e_len) {
        return Err(Error::InvalidIndex(
            "the entry table and the metadata table overlap",
        ));
    }
    Ok(header)
}

/// Checks one compressed table: inside the file, after the header, not empty,
/// within limits, and able to inflate to `expected` bytes at all.
fn check_table(
    (offset, stored): (u64, u32),
    expected: u64,
    file_len: u64,
    limits: &Limits,
) -> Result<()> {
    let stored = u64::from(stored);
    // Even an empty table is stored as a (short) zlib stream.
    if stored == 0 {
        return Err(Error::InvalidIndex("an index table has a stored size of 0"));
    }
    limit("stored index table size", stored, limits.max_index_bytes)?;
    if offset < HEADER_LEN as u64 {
        return Err(Error::InvalidIndex("an index table overlaps the header"));
    }
    match offset.checked_add(stored) {
        Some(end) if end <= file_len => {}
        _ => {
            return Err(Error::InvalidIndex(
                "an index table lies outside the archive",
            ));
        }
    }
    if expected > stored.saturating_mul(MAX_RATIO) {
        return Err(Error::InvalidIndex(
            "an index table is too small for its declared size",
        ));
    }
    Ok(())
}

/// Source of archive bytes, implemented by the archive file.
pub(crate) trait ReadAt {
    /// Reads exactly `buf.len()` bytes at `offset`.
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> Result<()>;
}

fn read_table(source: &dyn ReadAt, (offset, stored): (u64, u32), expected: u64) -> Result<Vec<u8>> {
    // `stored` and `expected` were bounded by `check_table` and the limits.
    let stored = usize::try_from(stored)
        .map_err(|_| Error::InvalidIndex("an index table is too large for this platform"))?;
    let expected = usize::try_from(expected)
        .map_err(|_| Error::InvalidIndex("an index table is too large for this platform"))?;
    let mut compressed = vec![0u8; stored];
    source.read_exact_at(offset, &mut compressed)?;
    inflate_exact(&compressed, expected).map_err(|reason| Error::CorruptData { hash: None, reason })
}

/// Reads and validates the header, both index tables and every entry.
pub(crate) fn read_index(
    source: &dyn ReadAt,
    file_len: u64,
    limits: &Limits,
) -> Result<Vec<Entry>> {
    let head_len = usize::try_from(file_len.min(HEADER_LEN as u64)).unwrap_or(HEADER_LEN);
    let mut head = [0u8; HEADER_LEN];
    source.read_exact_at(0, &mut head[..head_len])?;
    let header = parse_header(&head[..head_len], file_len, limits)?;

    let rows = read_table(
        source,
        header.entry_table,
        u64::from(header.entry_count) * ROW_LEN as u64,
    )?;
    let words: Vec<u32> = read_table(
        source,
        header.metadata_table,
        u64::from(header.metadata_words) * 4,
    )?
    .chunks_exact(4)
    .map(|w| u32_at(w, 0))
    .collect();

    rows.chunks_exact(ROW_LEN)
        .enumerate()
        .map(|(i, row)| {
            // `entry_count` is a u32, so every index fits.
            let index = u32::try_from(i)
                .map_err(|_| Error::InvalidIndex("too many entries for this platform"))?;
            parse_entry(row, index, &words, file_len)
        })
        .collect()
}

/// Validates one entry table row and its metadata.
fn parse_entry(row: &[u8], index: u32, words: &[u32], file_len: u64) -> Result<Entry> {
    let hash = u64_at(row, 0);
    let first = u32_at(row, 8) as usize;
    let count = usize::from(u16_at(row, 12));
    let flags = u16_at(row, 14);
    let invalid = |reason| Error::InvalidEntry { hash, reason };

    if count == 0 {
        return Err(invalid("the entry has no metadata records"));
    }
    let headers = first
        .checked_add(count)
        .and_then(|end| words.get(first..end))
        .ok_or_else(|| invalid("the metadata records lie outside the metadata table"))?;

    let data_block = |word: u32| -> Result<DataBlock> {
        let start = (word & 0x00FF_FFFF) as usize;
        let payload = words
            .get(start..start + PAYLOAD_WORDS)
            .ok_or_else(|| invalid("the data record lies outside the metadata table"))?;
        let data = DataBlock {
            stored_size: payload[0] & 0x0FFF_FFFF,
            method: (payload[0] >> 28) as u8,
            size: payload[1],
            reserved: payload[2],
            offset: u64::from(payload[3]) * 16,
        };
        match data.offset.checked_add(u64::from(data.stored_size)) {
            Some(end) if data.offset >= HEADER_LEN as u64 && end <= file_len => Ok(data),
            _ => Err(invalid("the data lies outside the archive")),
        }
    };

    let mut extra_records = false;
    let (kind, data) = match headers {
        [word] if word >> 24 == META_PLAIN || word >> 24 == META_DIRECTORY => {
            let data = data_block(*word)?;
            let is_dir = word >> 24 == META_DIRECTORY;
            if is_dir != (flags & FLAG_DIRECTORY != 0) {
                return Err(invalid("the directory flag does not match the metadata"));
            }
            let kind = if flags & !KNOWN_FLAGS != 0 {
                EntryKind::Unsupported
            } else if is_dir {
                EntryKind::Directory
            } else {
                EntryKind::File
            };
            (kind, Some(data))
        }
        // One plain record plus records of types 5, 6 or 7 (seen on `.pmg`
        // files). The extra records are not read; the plain record is only
        // reachable through `Archive::read_primary_record`.
        records
            if flags & !KNOWN_FLAGS == 0
                && flags & FLAG_DIRECTORY == 0
                && records.iter().filter(|w| *w >> 24 == META_PLAIN).count() == 1
                && records.iter().all(|w| {
                    matches!(
                        *w >> 24,
                        META_PLAIN | META_EXTRA_5 | META_EXTRA_6 | META_EXTRA_7
                    )
                }) =>
        {
            let plain = records
                .iter()
                .find(|w| *w >> 24 == META_PLAIN)
                .ok_or_else(|| invalid("the plain record disappeared"))?;
            extra_records = true;
            (EntryKind::Unsupported, Some(data_block(*plain)?))
        }
        // Texture objects, other files with extra records, unknown record types.
        _ => (EntryKind::Unsupported, None),
    };
    Ok(Entry {
        index,
        hash,
        kind,
        flags,
        data,
        extra_records,
    })
}

/// Why an unsupported entry cannot be read.
pub(crate) fn unsupported_reason(entry: &Entry) -> &'static str {
    if entry.extra_records {
        "it has extra metadata records; use Archive::read_primary_record to read its data record"
    } else if entry.data.is_none() {
        "its metadata records are not a single plain file or directory record \
         (for example a texture object or a file with extra records)"
    } else if entry.flags & !KNOWN_FLAGS != 0 {
        "it has unknown entry flag bits"
    } else {
        "it is not a plain file"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_bytes() -> [u8; HEADER_LEN] {
        let mut h = [0u8; HEADER_LEN];
        h[..4].copy_from_slice(b"SCS#");
        h[4..6].copy_from_slice(&2u16.to_le_bytes());
        h[8..12].copy_from_slice(b"CITY");
        h[16..20].copy_from_slice(&8u32.to_le_bytes());
        h[24..28].copy_from_slice(&8u32.to_le_bytes());
        h[28..36].copy_from_slice(&100u64.to_le_bytes());
        h[36..44].copy_from_slice(&200u64.to_le_bytes());
        h
    }

    #[test]
    fn accepts_a_minimal_header() {
        assert!(parse_header(&header_bytes(), 300, &Limits::default()).is_ok());
    }

    #[test]
    fn short_prefix_of_the_magic_is_a_truncated_header() {
        assert!(matches!(
            parse_header(b"SC", 2, &Limits::default()),
            Err(Error::TruncatedHeader { len: 2 })
        ));
        assert!(matches!(
            parse_header(b"", 0, &Limits::default()),
            Err(Error::NotHashFs)
        ));
        assert!(matches!(
            parse_header(b"XY", 2, &Limits::default()),
            Err(Error::NotHashFs)
        ));
    }

    #[test]
    fn rejects_overlapping_tables() {
        let mut h = header_bytes();
        h[36..44].copy_from_slice(&104u64.to_le_bytes());
        assert!(matches!(
            parse_header(&h, 300, &Limits::default()),
            Err(Error::InvalidIndex(_))
        ));
    }

    #[test]
    fn rejects_tables_that_cannot_hold_their_declared_size() {
        let mut h = header_bytes();
        // 1,000,000 entries cannot come from an 8-byte zlib stream.
        h[12..16].copy_from_slice(&1_000_000u32.to_le_bytes());
        assert!(matches!(
            parse_header(&h, 300, &Limits::default()),
            Err(Error::InvalidIndex(_))
        ));
    }

    #[test]
    fn offset_arithmetic_does_not_overflow() {
        let mut h = header_bytes();
        h[28..36].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(
            parse_header(&h, 300, &Limits::default()),
            Err(Error::InvalidIndex(_))
        ));
    }
}
