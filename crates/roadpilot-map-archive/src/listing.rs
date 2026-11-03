//! HashFS v2 directory listings.
//!
//! The data of a directory entry is: a `u32` child count, one `u8` name
//! length per child, then the names back to back (UTF-8). Subdirectory names
//! carry a leading `/`. Verified on 74,093 real listings by the SCS-Extractor
//! research.

/// Splits a listing into its raw names (subdirectories keep their `/`).
///
/// Never allocates more than the listing size: the declared count must fit
/// in the data before anything is reserved.
pub(crate) fn parse(data: &[u8]) -> Result<Vec<&str>, &'static str> {
    let count_bytes = data
        .get(..4)
        .ok_or("the listing is shorter than its 4-byte count")?;
    let count = u32::from_le_bytes([
        count_bytes[0],
        count_bytes[1],
        count_bytes[2],
        count_bytes[3],
    ]);
    let count = usize::try_from(count).map_err(|_| "the listing has too many names")?;
    let lengths = 4usize
        .checked_add(count)
        .and_then(|end| data.get(4..end))
        .ok_or("the listing is truncated")?;
    let mut pos = 4 + count;
    let mut names = Vec::with_capacity(count);
    for &len in lengths {
        let end = pos + usize::from(len);
        let raw = data.get(pos..end).ok_or("the listing is truncated")?;
        names.push(std::str::from_utf8(raw).map_err(|_| "a listed name is not valid UTF-8")?);
        pos = end;
    }
    if pos != data.len() {
        return Err("the listing has trailing bytes");
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn listing(names: &[&[u8]]) -> Vec<u8> {
        let mut v = (names.len() as u32).to_le_bytes().to_vec();
        v.extend(names.iter().map(|n| n.len() as u8));
        for n in names {
            v.extend_from_slice(n);
        }
        v
    }

    #[test]
    fn parses_names_and_keeps_directory_markers() {
        let data = listing(&[b"/sub", b"file.txt"]);
        assert_eq!(parse(&data), Ok(vec!["/sub", "file.txt"]));
    }

    #[test]
    fn empty_listing_is_four_zero_bytes() {
        assert_eq!(parse(&[0, 0, 0, 0]), Ok(Vec::new()));
    }

    #[test]
    fn rejects_malformed_listings() {
        let good = listing(&[b"a", b"bc"]);
        assert!(parse(&good[..3]).is_err(), "short count");
        assert!(parse(&good[..good.len() - 1]).is_err(), "truncated name");
        let mut trailing = good.clone();
        trailing.push(b'x');
        assert!(parse(&trailing).is_err(), "trailing bytes");
        assert!(parse(&listing(&[&[0xFF, 0xFE]])).is_err(), "invalid UTF-8");
    }

    #[test]
    fn huge_declared_count_does_not_allocate() {
        // Claims 4 billion names in 8 bytes: rejected before reserving memory.
        assert!(parse(&[0xFF, 0xFF, 0xFF, 0xFF, 1, 2, 3, 4]).is_err());
    }
}
