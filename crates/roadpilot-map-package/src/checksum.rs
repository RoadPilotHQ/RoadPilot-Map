//! FNV-1a 64-bit checksum of file contents.
//!
//! This detects accidental corruption and incomplete copies. It is **not** a
//! cryptographic hash and gives no protection against deliberate changes.

use std::io::{self, Read, Write};

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// Incremental FNV-1a (64-bit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fnv64(u64);

impl Default for Fnv64 {
    fn default() -> Self {
        Self::new()
    }
}

impl Fnv64 {
    #[must_use]
    pub const fn new() -> Self {
        Self(OFFSET)
    }

    pub fn update(&mut self, bytes: &[u8]) {
        let mut h = self.0;
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(PRIME);
        }
        self.0 = h;
    }

    #[must_use]
    pub const fn finish(&self) -> u64 {
        self.0
    }
}

/// The checksum of `bytes`.
#[must_use]
pub fn checksum(bytes: &[u8]) -> u64 {
    let mut h = Fnv64::new();
    h.update(bytes);
    h.finish()
}

/// Reads `reader` to its end and returns `(checksum, byte count)`.
///
/// # Errors
///
/// Any read error.
pub fn checksum_reader<R: Read>(mut reader: R) -> io::Result<(u64, u64)> {
    let mut h = Fnv64::new();
    let mut total = 0u64;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            return Ok((h.finish(), total));
        }
        h.update(&buf[..n]);
        total += n as u64;
    }
}

/// A writer that counts bytes and computes their checksum on the way.
#[derive(Debug)]
pub struct HashingWriter<W> {
    inner: W,
    hash: Fnv64,
    bytes: u64,
}

impl<W: Write> HashingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            hash: Fnv64::new(),
            bytes: 0,
        }
    }

    /// Bytes written so far.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Checksum of the bytes written so far.
    pub fn checksum(&self) -> u64 {
        self.hash.finish()
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hash.update(&buf[..n]);
        self.bytes += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_published_test_vectors() {
        assert_eq!(checksum(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(checksum(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(checksum(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn writer_and_reader_agree() {
        let data = b"line one\nline two\n";
        let mut w = HashingWriter::new(Vec::new());
        w.write_all(data).expect("memory write");
        assert_eq!(w.bytes(), data.len() as u64);
        assert_eq!(w.checksum(), checksum(data));
        assert_eq!(
            checksum_reader(&data[..]).expect("memory read"),
            (checksum(data), data.len() as u64)
        );
    }
}
