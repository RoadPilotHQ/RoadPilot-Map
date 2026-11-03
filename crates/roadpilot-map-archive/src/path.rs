//! Archive path normalization.
//!
//! Rules, as established for HashFS archives:
//!
//! - Paths are relative to the archive root and use `/` as separator.
//! - They are hashed **without** a leading slash. The root is the empty path.
//! - They are hashed as their exact UTF-8 bytes: no case folding, no Unicode
//!   normalization.
//!
//! For convenience one leading `/` (as in SII references such as
//! `/def/world/road.sii`) and one trailing `/` are accepted and removed.
//! Anything that could make a path ambiguous is rejected instead of being
//! repaired: empty components (`a//b`), `.` and `..`, backslashes and NUL.

use crate::error::{Error, Result};
use crate::hash::city_hash64;

/// Longest accepted path in bytes. Real archive paths are below 130 bytes.
pub(crate) const MAX_PATH_BYTES: usize = 1024;

/// Returns the normalized form of `input`, borrowed from it.
pub(crate) fn normalize(input: &str) -> Result<&str> {
    let invalid = |reason| Error::InvalidPath {
        path: input.to_owned(),
        reason,
    };
    if input.len() > MAX_PATH_BYTES {
        return Err(invalid("the path is longer than 1024 bytes"));
    }
    let path = input.strip_prefix('/').unwrap_or(input);
    let path = path.strip_suffix('/').unwrap_or(path);
    if path.is_empty() {
        return Ok(path);
    }
    for component in path.split('/') {
        check_name(component).map_err(invalid)?;
    }
    Ok(path)
}

/// Checks a single path component, such as a name from a directory listing.
pub(crate) fn check_name(name: &str) -> std::result::Result<(), &'static str> {
    match name {
        "" => Err("the path has an empty component"),
        "." | ".." => Err("the path has a '.' or '..' component"),
        _ if name.contains('/') => Err("the name contains '/'"),
        _ if name.contains('\\') => Err("the path contains a backslash"),
        _ if name.contains('\0') => Err("the path contains a NUL character"),
        _ => Ok(()),
    }
}

/// Splits a normalized, non-root path into parent and last component.
pub(crate) fn split_parent(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

/// HashFS hash of a normalized path.
pub(crate) fn hash_normalized(path: &str) -> u64 {
    city_hash64(path.as_bytes())
}

/// Normalizes `path` and returns its HashFS path hash (CityHash64 v1.0.3).
///
/// # Errors
///
/// [`Error::InvalidPath`] if the path breaks the normalization rules.
///
/// # Examples
///
/// ```
/// use roadpilot_map_archive::path_hash;
///
/// // The root directory is the empty path.
/// assert_eq!(path_hash("").unwrap(), 0x9ae1_6a3b_2f90_404f);
/// // A leading slash is ignored.
/// assert_eq!(path_hash("/dir/file.txt").unwrap(), path_hash("dir/file.txt").unwrap());
/// ```
pub fn path_hash(path: &str) -> Result<u64> {
    normalize(path).map(hash_normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_one_leading_and_one_trailing_slash() {
        assert_eq!(normalize("/def/file.sii").ok(), Some("def/file.sii"));
        assert_eq!(normalize("def/file.sii").ok(), Some("def/file.sii"));
        assert_eq!(normalize("def/").ok(), Some("def"));
        assert_eq!(normalize("/def/").ok(), Some("def"));
    }

    #[test]
    fn root_is_the_empty_path() {
        assert_eq!(normalize("").ok(), Some(""));
        assert_eq!(normalize("/").ok(), Some(""));
    }

    #[test]
    fn keeps_case_and_unicode_unchanged() {
        assert_eq!(normalize("Def/File.SII").ok(), Some("Def/File.SII"));
        assert_eq!(
            normalize("dir/caf\u{e9}.txt").ok(),
            Some("dir/caf\u{e9}.txt")
        );
    }

    #[test]
    fn rejects_ambiguous_paths() {
        for bad in [
            "//def",
            "def//file",
            "def/file//",
            "./def",
            "def/../file",
            "..",
            "def\\file",
            "def/fi\0le",
        ] {
            assert!(
                matches!(normalize(bad), Err(Error::InvalidPath { .. })),
                "{bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn rejects_overlong_paths() {
        let long = "a/".repeat(600);
        assert!(matches!(normalize(&long), Err(Error::InvalidPath { .. })));
    }

    #[test]
    fn splits_parent() {
        assert_eq!(split_parent("a/b/c"), ("a/b", "c"));
        assert_eq!(split_parent("top"), ("", "top"));
    }

    #[test]
    fn path_hash_ignores_leading_slash_but_not_case() {
        assert_eq!(path_hash("/x/y").ok(), path_hash("x/y").ok());
        assert_ne!(path_hash("x/y").ok(), path_hash("X/y").ok());
    }
}
