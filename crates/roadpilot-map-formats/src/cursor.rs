//! Bounds-checked little-endian reader over a byte slice.
//!
//! Every read checks the remaining length first, and every size computation
//! uses checked arithmetic, so malformed input produces a [`ParseError`]
//! instead of a panic or an out-of-range access.

use crate::error::{ParseError, Result};

pub(crate) struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }

    /// The whole input the cursor reads from.
    pub(crate) fn source(&self) -> &'a [u8] {
        self.data
    }

    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub(crate) fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8]> {
        let truncated = ParseError::Truncated {
            offset: self.pos,
            needed: n,
            what,
        };
        let end = self.pos.checked_add(n).ok_or(truncated.clone())?;
        let bytes = self.data.get(self.pos..end).ok_or(truncated)?;
        self.pos = end;
        Ok(bytes)
    }

    pub(crate) fn skip(&mut self, n: usize, what: &'static str) -> Result<()> {
        self.take(n, what).map(|_| ())
    }

    fn array<const N: usize>(&mut self, what: &'static str) -> Result<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N, what)?);
        Ok(out)
    }

    pub(crate) fn u8(&mut self, what: &'static str) -> Result<u8> {
        Ok(self.array::<1>(what)?[0])
    }

    pub(crate) fn u16(&mut self, what: &'static str) -> Result<u16> {
        self.array(what).map(u16::from_le_bytes)
    }

    pub(crate) fn i16(&mut self, what: &'static str) -> Result<i16> {
        self.array(what).map(i16::from_le_bytes)
    }

    pub(crate) fn u32(&mut self, what: &'static str) -> Result<u32> {
        self.array(what).map(u32::from_le_bytes)
    }

    pub(crate) fn i32(&mut self, what: &'static str) -> Result<i32> {
        self.array(what).map(i32::from_le_bytes)
    }

    pub(crate) fn u64(&mut self, what: &'static str) -> Result<u64> {
        self.array(what).map(u64::from_le_bytes)
    }

    pub(crate) fn f32(&mut self, what: &'static str) -> Result<f32> {
        self.array(what).map(f32::from_le_bytes)
    }

    /// A UID that must not be zero.
    pub(crate) fn uid(&mut self, what: &'static str) -> Result<u64> {
        let offset = self.pos;
        match self.u64(what)? {
            0 => Err(ParseError::ZeroUid { offset, what }),
            uid => Ok(uid),
        }
    }

    /// Checks that `count` elements of `elem` bytes fit in the rest of the
    /// input and returns their total size. `offset` is where the count was
    /// read, for the error.
    pub(crate) fn fits(
        &self,
        offset: usize,
        count: u64,
        elem: usize,
        what: &'static str,
    ) -> Result<usize> {
        let invalid = ParseError::InvalidCount {
            offset,
            count,
            what,
        };
        let count = usize::try_from(count).map_err(|_| invalid.clone())?;
        let bytes = count.checked_mul(elem).ok_or(invalid.clone())?;
        if bytes > self.remaining() {
            return Err(invalid);
        }
        Ok(bytes)
    }

    /// Reads a `u32` count and skips `count * elem` bytes. Returns the count.
    pub(crate) fn skip_array(&mut self, elem: usize, what: &'static str) -> Result<u32> {
        let offset = self.pos;
        let count = self.u32(what)?;
        let bytes = self.fits(offset, u64::from(count), elem, what)?;
        self.skip(bytes, what)?;
        Ok(count)
    }

    /// Reads a `u32` count followed by that many UIDs.
    pub(crate) fn uid_list(&mut self, what: &'static str) -> Result<Vec<u64>> {
        let offset = self.pos;
        let count = self.u32(what)?;
        // Bounded by the input: the list must fit before anything is reserved.
        let bytes = self.fits(offset, u64::from(count), 8, what)?;
        let mut out = Vec::with_capacity(bytes / 8);
        for _ in 0..count {
            out.push(self.uid(what)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_little_endian_values() {
        let bytes = [1, 0, 2, 0, 0, 0, 0xFF, 0xFF];
        let mut c = Cursor::new(&bytes, 0);
        assert_eq!(c.u16("a"), Ok(1));
        assert_eq!(c.u32("b"), Ok(2));
        assert_eq!(c.i16("c"), Ok(-1));
        assert_eq!(c.remaining(), 0);
    }

    #[test]
    fn reports_truncation_with_offset() {
        let mut c = Cursor::new(&[0, 0, 0], 1);
        assert_eq!(
            c.u32("field"),
            Err(ParseError::Truncated {
                offset: 1,
                needed: 4,
                what: "field"
            })
        );
    }

    #[test]
    fn rejects_counts_that_do_not_fit() {
        // Count 3 of 8-byte elements, but only 8 bytes follow.
        let mut bytes = 3u32.to_le_bytes().to_vec();
        bytes.extend([0u8; 8]);
        let mut c = Cursor::new(&bytes, 0);
        assert!(matches!(
            c.skip_array(8, "list"),
            Err(ParseError::InvalidCount { count: 3, .. })
        ));
    }

    #[test]
    fn size_computations_do_not_overflow() {
        let bytes = u32::MAX.to_le_bytes();
        let mut c = Cursor::new(&bytes, 0);
        assert!(matches!(
            c.skip_array(usize::MAX, "list"),
            Err(ParseError::InvalidCount { .. })
        ));
        let c = Cursor::new(&[], 0);
        assert!(c.fits(0, u64::MAX, 2, "x").is_err());
    }

    #[test]
    fn take_at_the_end_of_input_cannot_overflow() {
        let mut c = Cursor::new(&[1, 2], 2);
        assert!(c.take(usize::MAX, "x").is_err());
    }

    #[test]
    fn zero_uids_are_rejected() {
        let mut c = Cursor::new(&[0u8; 8], 0);
        assert_eq!(
            c.uid("node uid"),
            Err(ParseError::ZeroUid {
                offset: 0,
                what: "node uid"
            })
        );
    }
}
