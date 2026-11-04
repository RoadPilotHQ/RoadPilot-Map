//! Reader for road template descriptors (`.pmd`, "Prism model descriptor"),
//! version 4.
//!
//! A descriptor names the **looks** and **variants** of a model, says which
//! **parts** of the paired `.pmg` geometry are visible in each variant, and
//! gives the **material file** of every material slot for every look. It
//! holds no geometry.
//!
//! The layout was established by measurement on all road templates
//! (`road_template/**/*.pmd`) of local ETS2 (454 files) and ATS (222 files)
//! installations; every file has version 4 and every check below holds for
//! all of them. SCS publishes no byte-level specification (the SCS modding
//! wiki only names the file type). No third-party code or documentation was
//! used. See `docs/formats/scs-road-templates.md`.
//!
//! # Layout (little-endian `u32` words unless stated)
//!
//! Header, 16 words:
//!
//! | Word | Meaning |
//! |---|---|
//! | 0 | Version, 4 |
//! | 1 | Material slot count |
//! | 2 | Look count |
//! | 3 | Unknown; 0 in every inspected file (kept as [`Pmd::unknown_word_3`]) |
//! | 4 | Variant count |
//! | 5 | Part count (equals the part count of the `.pmg`) |
//! | 6 | Attribute count |
//! | 7 | Bytes of attribute values per variant |
//! | 8 | Bytes of the material path block |
//! | 9–15 | Offsets of: look names, variant names, part attribute ranges, variant values, attribute definitions, material table, material path block |
//!
//! Tables, in file order: look names (`u64` tokens), variant names (`u64`
//! tokens), part attribute ranges (start and end attribute index per part),
//! attribute definitions (16 bytes: `u64` token name, `u32` type, `u32` byte
//! offset in a variant's values), variant values (one block per variant),
//! material table (look × material slot, `u32` absolute offset of a path),
//! material paths (NUL-terminated). The block ends at the end of the file.

use std::fmt;
use std::ops::Range;

/// The only supported version.
pub const PMD_VERSION: u32 = 4;

/// Name token of the only attribute seen in road templates.
const VISIBLE: &str = "visible";

/// Limits that keep a hostile file from causing large allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PmdLimits {
    /// Largest file accepted, bytes.
    pub max_file_len: usize,
    /// Largest count of any table.
    pub max_count: u32,
}

impl Default for PmdLimits {
    fn default() -> Self {
        Self {
            max_file_len: 16 << 20,
            max_count: 1 << 16,
        }
    }
}

/// Why a `.pmd` file was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PmdError {
    /// The input is shorter than the header.
    Truncated,
    /// The version is not [`PMD_VERSION`].
    UnsupportedVersion { found: u32 },
    /// A count or the file length exceeds the limits.
    LimitExceeded { what: &'static str, value: u64 },
    /// A table is not where the measured layout puts it, or lies outside the
    /// file.
    Layout { what: &'static str },
    /// A value is invalid (for example a material path that is not
    /// terminated or not UTF-8, or a part range that is not ascending).
    Invalid { what: &'static str },
}

impl fmt::Display for PmdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("the file is shorter than the PMD header"),
            Self::UnsupportedVersion { found } => {
                write!(
                    f,
                    "unsupported PMD version {found} (supported: {PMD_VERSION})"
                )
            }
            Self::LimitExceeded { what, value } => write!(f, "{what} ({value}) exceeds the limit"),
            Self::Layout { what } => write!(f, "unexpected PMD layout: {what}"),
            Self::Invalid { what } => write!(f, "invalid PMD data: {what}"),
        }
    }
}

impl std::error::Error for PmdError {}

/// An attribute definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PmdAttribute {
    /// Name token (decode with [`crate::Token`]); `visible` in road templates.
    pub name: u64,
    /// Type number; 0 for every attribute seen.
    pub kind: u32,
    /// Byte offset of the value in a variant's value block.
    pub offset: u32,
}

/// A parsed descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pmd {
    /// Header word 3, meaning unknown (0 in every inspected file).
    pub unknown_word_3: u32,
    /// Look name tokens.
    pub looks: Vec<u64>,
    /// Variant name tokens.
    pub variants: Vec<u64>,
    /// Per part, the range of its attributes in [`Pmd::attributes`].
    pub part_attributes: Vec<Range<u32>>,
    pub attributes: Vec<PmdAttribute>,
    /// Per variant, the raw value words of all attributes.
    pub variant_values: Vec<Vec<u32>>,
    /// `materials[look][slot]` is the path of a material file.
    pub materials: Vec<Vec<String>>,
}

impl Pmd {
    /// Number of material slots.
    #[must_use]
    pub fn material_slots(&self) -> usize {
        self.materials.first().map_or(0, Vec::len)
    }

    /// Whether `part` is visible in `variant`, from its `visible` attribute.
    /// `None` if the part has no such attribute, the indices are out of
    /// range, or the value is neither 0 nor 1.
    #[must_use]
    pub fn part_visible(&self, variant: usize, part: usize) -> Option<bool> {
        let range = self.part_attributes.get(part)?;
        let values = self.variant_values.get(variant)?;
        for a in range.start..range.end {
            let attr = self.attributes.get(a as usize)?;
            if crate::Token(attr.name).name().as_deref() == Some(VISIBLE) {
                let word = values.get(attr.offset as usize / 4)?;
                return match word {
                    0 => Some(false),
                    1 => Some(true),
                    _ => None,
                };
            }
        }
        None
    }
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn dword(bytes: &[u8], at: usize) -> Option<u64> {
    let b = bytes.get(at..at.checked_add(8)?)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(b);
    Some(u64::from_le_bytes(a))
}

/// Parses a `.pmd` file with the default [`PmdLimits`].
///
/// # Errors
///
/// [`PmdError`] for a wrong version, a layout that differs from the measured
/// one, values outside the file, counts beyond the limits, or invalid
/// material paths.
pub fn parse_pmd(bytes: &[u8]) -> Result<Pmd, PmdError> {
    parse_pmd_with_limits(bytes, &PmdLimits::default())
}

/// Parses a `.pmd` file with explicit limits.
///
/// # Errors
///
/// See [`parse_pmd`].
#[allow(clippy::too_many_lines)]
pub fn parse_pmd_with_limits(bytes: &[u8], limits: &PmdLimits) -> Result<Pmd, PmdError> {
    if bytes.len() > limits.max_file_len {
        return Err(PmdError::LimitExceeded {
            what: "file length",
            value: bytes.len() as u64,
        });
    }
    let mut h = [0u32; 16];
    for (i, slot) in h.iter_mut().enumerate() {
        *slot = word(bytes, i * 4).ok_or(PmdError::Truncated)?;
    }
    if h[0] != PMD_VERSION {
        return Err(PmdError::UnsupportedVersion { found: h[0] });
    }
    for (what, v) in [
        ("material slots", h[1]),
        ("looks", h[2]),
        ("variants", h[4]),
        ("parts", h[5]),
        ("attributes", h[6]),
    ] {
        if v > limits.max_count {
            return Err(PmdError::LimitExceeded {
                what,
                value: u64::from(v),
            });
        }
    }
    let (slots, looks, variants, parts, attrs) = (
        h[1] as u64,
        h[2] as u64,
        h[4] as u64,
        h[5] as u64,
        h[6] as u64,
    );
    let (value_bytes, path_bytes) = (u64::from(h[7]), u64::from(h[8]));
    if value_bytes % 4 != 0 {
        return Err(PmdError::Invalid {
            what: "the attribute value block is not a whole number of words",
        });
    }
    // The measured order of the tables; each must start where the previous
    // one ends. All products fit in u64 because the counts are limited.
    let o: Vec<u64> = h[9..16].iter().map(|&v| u64::from(v)).collect();
    let checks = [
        (o[0] == 64, "look names do not follow the header"),
        (
            o[1] == o[0] + looks * 8,
            "variant names do not follow the look names",
        ),
        (
            o[2] == o[1] + variants * 8,
            "part ranges do not follow the variant names",
        ),
        (
            o[4] == o[2] + parts * 8,
            "attribute definitions do not follow the part ranges",
        ),
        (
            o[3] == o[4] + attrs * 16,
            "variant values do not follow the attribute definitions",
        ),
        (
            o[5] == o[3] + variants * value_bytes,
            "the material table does not follow the variant values",
        ),
        (
            o[6] == o[5] + looks * slots * 4,
            "the material paths do not follow the material table",
        ),
        (
            o[6] + path_bytes == bytes.len() as u64,
            "the material paths do not end at the end of the file",
        ),
    ];
    for (ok, what) in checks {
        if !ok {
            return Err(PmdError::Layout { what });
        }
    }
    let at = |v: u64| v as usize; // all offsets are now known to be inside the file

    let tokens = |start: u64, n: u64| -> Result<Vec<u64>, PmdError> {
        (0..n)
            .map(|i| dword(bytes, at(start + i * 8)).ok_or(PmdError::Truncated))
            .collect()
    };
    let looks_v = tokens(o[0], looks)?;
    let variants_v = tokens(o[1], variants)?;

    let mut part_attributes = Vec::with_capacity(parts as usize);
    let mut next = 0u32;
    for i in 0..parts {
        let s = word(bytes, at(o[2] + i * 8)).ok_or(PmdError::Truncated)?;
        let e = word(bytes, at(o[2] + i * 8 + 4)).ok_or(PmdError::Truncated)?;
        if s != next || e < s || u64::from(e) > attrs {
            return Err(PmdError::Invalid {
                what: "part attribute ranges are not contiguous and ascending",
            });
        }
        next = e;
        part_attributes.push(s..e);
    }
    if u64::from(next) != attrs {
        return Err(PmdError::Invalid {
            what: "part attribute ranges do not cover the attributes",
        });
    }

    let mut attributes = Vec::with_capacity(attrs as usize);
    for i in 0..attrs {
        let base = at(o[4] + i * 16);
        let a = PmdAttribute {
            name: dword(bytes, base).ok_or(PmdError::Truncated)?,
            kind: word(bytes, base + 8).ok_or(PmdError::Truncated)?,
            offset: word(bytes, base + 12).ok_or(PmdError::Truncated)?,
        };
        if u64::from(a.offset) + 4 > value_bytes || a.offset % 4 != 0 {
            return Err(PmdError::Invalid {
                what: "an attribute value lies outside the value block",
            });
        }
        attributes.push(a);
    }

    let words_per_variant = value_bytes / 4;
    let mut variant_values = Vec::with_capacity(variants as usize);
    for v in 0..variants {
        let base = o[3] + v * value_bytes;
        variant_values.push(
            (0..words_per_variant)
                .map(|k| word(bytes, at(base + k * 4)).ok_or(PmdError::Truncated))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }

    let mut materials = Vec::with_capacity(looks as usize);
    for l in 0..looks {
        let mut row = Vec::with_capacity(slots as usize);
        for s in 0..slots {
            let off =
                u64::from(word(bytes, at(o[5] + (l * slots + s) * 4)).ok_or(PmdError::Truncated)?);
            if off < o[6] || off >= bytes.len() as u64 {
                return Err(PmdError::Invalid {
                    what: "a material path offset lies outside the path block",
                });
            }
            let rest = &bytes[at(off)..];
            let end = rest.iter().position(|&b| b == 0).ok_or(PmdError::Invalid {
                what: "a material path is not terminated",
            })?;
            let path = std::str::from_utf8(&rest[..end]).map_err(|_| PmdError::Invalid {
                what: "a material path is not UTF-8",
            })?;
            row.push(path.to_owned());
        }
        materials.push(row);
    }

    Ok(Pmd {
        unknown_word_3: h[3],
        looks: looks_v,
        variants: variants_v,
        part_attributes,
        attributes,
        variant_values,
        materials,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(name: &str) -> u64 {
        const A: &[u8] = b"\x000123456789abcdefghijklmnopqrstuvwxyz_";
        name.bytes().rev().fold(0u64, |acc, c| {
            acc * 38 + A.iter().position(|&a| a == c).expect("token character") as u64
        })
    }

    /// Builds a synthetic descriptor (invented names and paths, not game
    /// data): `looks` × `paths.len()` materials, one `visible` attribute per
    /// part, and the visibility matrix `visible[variant][part]`.
    fn build(looks: &[&str], variants: &[&str], visible: &[Vec<u32>], paths: &[&str]) -> Vec<u8> {
        let parts = visible.first().map_or(0, Vec::len);
        let (l, v, s) = (looks.len(), variants.len(), paths.len());
        let o_looks = 64;
        let o_vars = o_looks + l * 8;
        let o_ranges = o_vars + v * 8;
        let o_attrs = o_ranges + parts * 8;
        let o_values = o_attrs + parts * 16;
        let o_table = o_values + v * parts * 4;
        let o_paths = o_table + l * s * 4;
        let mut path_block = Vec::new();
        let mut offsets = Vec::new();
        for _ in 0..l {
            for p in paths {
                offsets.push((o_paths + path_block.len()) as u32);
                path_block.extend(p.as_bytes());
                path_block.push(0);
            }
        }
        let mut out = Vec::new();
        let words = [
            PMD_VERSION,
            s as u32,
            l as u32,
            0,
            v as u32,
            parts as u32,
            parts as u32,
            (parts * 4) as u32,
            path_block.len() as u32,
            o_looks as u32,
            o_vars as u32,
            o_ranges as u32,
            o_values as u32,
            o_attrs as u32,
            o_table as u32,
            o_paths as u32,
        ];
        for w in words {
            out.extend(w.to_le_bytes());
        }
        for n in looks.iter().chain(variants) {
            out.extend(token(n).to_le_bytes());
        }
        for p in 0..parts as u32 {
            out.extend(p.to_le_bytes());
            out.extend((p + 1).to_le_bytes());
        }
        for p in 0..parts as u32 {
            out.extend(token(VISIBLE).to_le_bytes());
            out.extend(0u32.to_le_bytes());
            out.extend((p * 4).to_le_bytes());
        }
        for row in visible {
            for w in row {
                out.extend(w.to_le_bytes());
            }
        }
        for off in offsets {
            out.extend(off.to_le_bytes());
        }
        out.extend(path_block);
        out
    }

    fn sample() -> Vec<u8> {
        build(
            &["grey", "dark"],
            &["plain", "lines"],
            &[vec![1, 0, 1], vec![1, 1, 0]],
            &["/automat/aa/one.mat", "/automat/bb/two.mat"],
        )
    }

    #[test]
    fn reads_looks_variants_visibility_and_materials() {
        let pmd = parse_pmd(&sample()).expect("valid");
        assert_eq!(pmd.looks, vec![token("grey"), token("dark")]);
        assert_eq!(
            crate::Token(pmd.variants[1]).name().as_deref(),
            Some("lines")
        );
        assert_eq!(pmd.part_attributes, vec![0..1, 1..2, 2..3]);
        assert_eq!(pmd.part_visible(0, 1), Some(false));
        assert_eq!(pmd.part_visible(1, 1), Some(true));
        assert_eq!(pmd.part_visible(1, 2), Some(false));
        assert_eq!(pmd.part_visible(2, 0), None);
        assert_eq!(pmd.part_visible(0, 3), None);
        assert_eq!(pmd.material_slots(), 2);
        assert_eq!(pmd.materials[1][1], "/automat/bb/two.mat");
        assert_eq!(pmd.unknown_word_3, 0);
    }

    #[test]
    fn visibility_values_other_than_zero_and_one_are_not_guessed() {
        let pmd = parse_pmd(&build(&["a"], &["b"], &[vec![2]], &["/x.mat"])).expect("valid");
        assert_eq!(pmd.part_visible(0, 0), None);
    }

    #[test]
    fn rejects_other_versions_short_input_and_limits() {
        let mut b = sample();
        assert_eq!(parse_pmd(&b[..40]), Err(PmdError::Truncated));
        b[0] = 5;
        assert_eq!(
            parse_pmd(&b),
            Err(PmdError::UnsupportedVersion { found: 5 })
        );
        let mut b = sample();
        b[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(parse_pmd(&b), Err(PmdError::LimitExceeded { .. })));
        let tight = PmdLimits {
            max_file_len: 10,
            max_count: 10,
        };
        assert!(matches!(
            parse_pmd_with_limits(&sample(), &tight),
            Err(PmdError::LimitExceeded { .. })
        ));
    }

    #[test]
    fn rejects_layouts_that_differ_from_the_measured_one() {
        // A table offset moved by one word.
        let mut b = sample();
        let v = u32::from_le_bytes([b[40], b[41], b[42], b[43]]) + 4;
        b[40..44].copy_from_slice(&v.to_le_bytes());
        assert!(matches!(parse_pmd(&b), Err(PmdError::Layout { .. })));
        // Trailing data after the path block.
        let mut b = sample();
        b.push(0);
        assert!(matches!(parse_pmd(&b), Err(PmdError::Layout { .. })));
        // A cut-off file.
        let b = sample();
        assert!(parse_pmd(&b[..b.len() - 3]).is_err());
    }

    #[test]
    fn rejects_invalid_values() {
        let good = sample();
        let pmd = parse_pmd(&good).expect("valid");
        let table = 64 + 2 * 8 + 2 * 8 + 3 * 8 + 3 * 16 + 2 * 12;
        // A path offset pointing into the header.
        let mut b = good.clone();
        b[table..table + 4].copy_from_slice(&8u32.to_le_bytes());
        assert!(matches!(parse_pmd(&b), Err(PmdError::Invalid { .. })));
        // An unterminated last path.
        let mut b = good.clone();
        let n = b.len();
        b[n - 1] = b'x';
        assert!(matches!(parse_pmd(&b), Err(PmdError::Invalid { .. })));
        // A non-UTF-8 path.
        let mut b = good.clone();
        let n = b.len();
        b[n - 3] = 0xff;
        assert!(matches!(parse_pmd(&b), Err(PmdError::Invalid { .. })));
        // Part ranges that are not contiguous.
        let mut b = good.clone();
        let ranges = 64 + 2 * 8 + 2 * 8;
        b[ranges + 8..ranges + 12].copy_from_slice(&2u32.to_le_bytes());
        assert!(matches!(parse_pmd(&b), Err(PmdError::Invalid { .. })));
        // An attribute offset beyond the value block.
        let mut b = good;
        let attrs = ranges + 3 * 8;
        b[attrs + 12..attrs + 16].copy_from_slice(&64u32.to_le_bytes());
        assert!(matches!(parse_pmd(&b), Err(PmdError::Invalid { .. })));
        assert_eq!(pmd.attributes.len(), 3);
    }

    #[test]
    fn byte_flips_never_panic() {
        let good = sample();
        for n in 0..good.len() {
            let _ = parse_pmd(&good[..n]);
        }
        let mut seed = 0x9e37_79b9u32;
        for _ in 0..3000 {
            let mut b = good.clone();
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let i = (seed >> 8) as usize % b.len();
            b[i] ^= (seed & 0xff) as u8 | 1;
            let _ = parse_pmd(&b);
        }
    }
}
