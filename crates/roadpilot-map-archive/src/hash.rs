//! CityHash64 v1.0.3, the path hash of HashFS archives (hash method `CITY`).
//!
//! HashFS uses the **v1.0.3** revision. It differs from the more common v1.1
//! for inputs of 1 to 64 bytes (v1.0.x short-input functions with the extra
//! constant `K3`) and shares the long-input code with v1.1. A v1.1
//! implementation gives wrong hashes for short paths.
//!
//! The algorithm follows the published Google CityHash reference (MIT
//! license). It was ported independently, following the same revision as the
//! SCS-Extractor research project, where it was validated against 477,121
//! real archive paths. The tests below compare it with vectors produced by that
//! validated implementation on synthetic inputs.

const K0: u64 = 0xc3a5_c85c_97cb_3127;
const K1: u64 = 0xb492_b66f_be98_f273;
const K2: u64 = 0x9ae1_6a3b_2f90_404f;
const K3: u64 = 0xc949_d7c7_509e_6557;
const K_MUL: u64 = 0x9ddf_ea08_eb38_2d69;

// Every caller passes `at + N <= s.len()`; the length branches below
// guarantee it, so these slices are always in range.
fn fetch64(s: &[u8], at: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&s[at..at + 8]);
    u64::from_le_bytes(b)
}

fn fetch32(s: &[u8], at: usize) -> u64 {
    let mut b = [0u8; 4];
    b.copy_from_slice(&s[at..at + 4]);
    u64::from(u32::from_le_bytes(b))
}

fn shift_mix(v: u64) -> u64 {
    v ^ (v >> 47)
}

/// `Hash128to64` of the reference implementation.
fn hash_16(u: u64, v: u64) -> u64 {
    let mut a = (u ^ v).wrapping_mul(K_MUL);
    a ^= a >> 47;
    let mut b = (v ^ a).wrapping_mul(K_MUL);
    b ^= b >> 47;
    b.wrapping_mul(K_MUL)
}

fn len_0_to_16(s: &[u8]) -> u64 {
    let len = s.len();
    let len64 = len as u64;
    if len > 8 {
        let a = fetch64(s, 0);
        let b = fetch64(s, len - 8);
        // 9 <= len <= 16, so the rotation amount is in range and never zero.
        return hash_16(a, b.wrapping_add(len64).rotate_right(len as u32)) ^ b;
    }
    if len >= 4 {
        let a = fetch32(s, 0);
        return hash_16(len64.wrapping_add(a << 3), fetch32(s, len - 4));
    }
    if len > 0 {
        let a = u32::from(s[0]);
        let b = u32::from(s[len >> 1]);
        let c = u32::from(s[len - 1]);
        let y = a.wrapping_add(b << 8);
        let z = (len as u32).wrapping_add(c << 2);
        // v1.0.x: K3 here (v1.1 uses K0).
        return shift_mix(u64::from(y).wrapping_mul(K2) ^ u64::from(z).wrapping_mul(K3))
            .wrapping_mul(K2);
    }
    K2
}

fn len_17_to_32(s: &[u8]) -> u64 {
    let len = s.len();
    let a = fetch64(s, 0).wrapping_mul(K1);
    let b = fetch64(s, 8);
    let c = fetch64(s, len - 8).wrapping_mul(K2);
    let d = fetch64(s, len - 16).wrapping_mul(K0);
    hash_16(
        a.wrapping_sub(b)
            .rotate_right(43)
            .wrapping_add(c.rotate_right(30))
            .wrapping_add(d),
        a.wrapping_add((b ^ K3).rotate_right(20))
            .wrapping_sub(c)
            .wrapping_add(len as u64),
    )
}

/// One half of the 33-64 byte mix. Returns `(fast, slow)`.
fn mix_33_to_64(s: &[u8], first: u64, z: u64, at1: usize, at2: usize) -> (u64, u64) {
    let mut a = first;
    let b = a.wrapping_add(z).rotate_right(52);
    let mut c = a.rotate_right(37);
    a = a.wrapping_add(fetch64(s, at1));
    c = c.wrapping_add(a.rotate_right(7));
    a = a.wrapping_add(fetch64(s, at2));
    (
        a.wrapping_add(z),
        b.wrapping_add(a.rotate_right(31)).wrapping_add(c),
    )
}

fn len_33_to_64(s: &[u8]) -> u64 {
    let len = s.len();
    let first = fetch64(s, 0).wrapping_add(
        (len as u64)
            .wrapping_add(fetch64(s, len - 16))
            .wrapping_mul(K0),
    );
    let (vf, vs) = mix_33_to_64(s, first, fetch64(s, 24), 8, 16);
    let second = fetch64(s, 16).wrapping_add(fetch64(s, len - 32));
    let (wf, ws) = mix_33_to_64(s, second, fetch64(s, len - 8), len - 24, len - 16);
    let r = shift_mix(
        vf.wrapping_add(ws)
            .wrapping_mul(K2)
            .wrapping_add(wf.wrapping_add(vs).wrapping_mul(K0)),
    );
    shift_mix(r.wrapping_mul(K0).wrapping_add(vs)).wrapping_mul(K2)
}

/// `WeakHashLen32WithSeeds` over `s[at..at + 32]`.
fn weak_32(s: &[u8], at: usize, a: u64, b: u64) -> (u64, u64) {
    let w = fetch64(s, at);
    let x = fetch64(s, at + 8);
    let y = fetch64(s, at + 16);
    let z = fetch64(s, at + 24);
    let mut a = a.wrapping_add(w);
    let mut b = b.wrapping_add(a).wrapping_add(z).rotate_right(21);
    let c = a;
    a = a.wrapping_add(x).wrapping_add(y);
    b = b.wrapping_add(a.rotate_right(44));
    (a.wrapping_add(z), b.wrapping_add(c))
}

/// CityHash64 v1.0.3 of `s`.
pub(crate) fn city_hash64(s: &[u8]) -> u64 {
    let len = s.len();
    if len <= 16 {
        return len_0_to_16(s);
    }
    if len <= 32 {
        return len_17_to_32(s);
    }
    if len <= 64 {
        return len_33_to_64(s);
    }

    // More than 64 bytes: 56 bytes of state, seeded from the last 64 bytes.
    let len64 = len as u64;
    let mut x = fetch64(s, len - 40);
    let mut y = fetch64(s, len - 16).wrapping_add(fetch64(s, len - 56));
    let mut z = hash_16(
        fetch64(s, len - 48).wrapping_add(len64),
        fetch64(s, len - 24),
    );
    let mut v = weak_32(s, len - 64, len64, z);
    let mut w = weak_32(s, len - 32, y.wrapping_add(K1), x);
    x = x.wrapping_mul(K1).wrapping_add(fetch64(s, 0));

    // Whole 64-byte blocks from the start; the (possibly partial) tail was
    // already folded in above. `(len - 1) & !63` is at least 64 here.
    let blocks_end = (len - 1) & !63;
    let mut pos = 0;
    while pos < blocks_end {
        x = x
            .wrapping_add(y)
            .wrapping_add(v.0)
            .wrapping_add(fetch64(s, pos + 8))
            .rotate_right(37)
            .wrapping_mul(K1);
        y = y
            .wrapping_add(v.1)
            .wrapping_add(fetch64(s, pos + 48))
            .rotate_right(42)
            .wrapping_mul(K1);
        x ^= w.1;
        y = y.wrapping_add(v.0).wrapping_add(fetch64(s, pos + 40));
        z = z.wrapping_add(w.0).rotate_right(33).wrapping_mul(K1);
        v = weak_32(s, pos, v.1.wrapping_mul(K1), x.wrapping_add(w.0));
        w = weak_32(
            s,
            pos + 32,
            z.wrapping_add(w.1),
            y.wrapping_add(fetch64(s, pos + 16)),
        );
        std::mem::swap(&mut z, &mut x);
        pos += 64;
    }
    hash_16(
        hash_16(v.0, w.0)
            .wrapping_add(shift_mix(y).wrapping_mul(K1))
            .wrapping_add(z),
        hash_16(v.1, w.1).wrapping_add(x),
    )
}

#[cfg(test)]
mod tests {
    use super::city_hash64;

    /// Synthetic input of length `n`: byte `i` is `(i * 7 + 3) % 251`.
    fn pattern(n: usize) -> Vec<u8> {
        (0..n).map(|i| ((i * 7 + 3) % 251) as u8).collect()
    }

    /// Expected hashes of `pattern(n)`, produced by the CityHash64 v1.0.3
    /// implementation of SCS-Extractor (validated against real archive
    /// paths). The lengths cover every branch and its boundaries, including
    /// one, two and many 64-byte blocks.
    const PATTERN_VECTORS: &[(usize, u64)] = &[
        (0, 0x9ae16a3b2f90404f),
        (1, 0x5068a5b3d87a0284),
        (2, 0x6ea90655f7a8ea3c),
        (3, 0xf6aa543ca4b8bf14),
        (4, 0x9dd33b80b9fa6393),
        (5, 0x4f7df53bd5a2ced5),
        (7, 0x0e1bc1022a1b992c),
        (8, 0x37fed2dba3a300e3),
        (9, 0x29b1ff616f22d6f6),
        (15, 0xf35205fd9db2c504),
        (16, 0xbdf5bcbc9b4603a4),
        (17, 0xa75fe55f0d775eda),
        (24, 0x7353f72904323f52),
        (31, 0xa74e1d551760088d),
        (32, 0xd7989f47d40c660d),
        (33, 0xef04fa08e8e5b8fb),
        (48, 0x5115cb7392c8ce84),
        (63, 0xceb2a602eadf821d),
        (64, 0x585d4d8b89f08236),
        (65, 0xd4956fceda7039f9),
        (96, 0x511087923681108b),
        (127, 0xeeecbdf4b5fbb98a),
        (128, 0xe57626f7d182892a),
        (129, 0xfa5938ce10890961),
        (200, 0x2a231b9997ae0bf7),
        (256, 0x2009a56b6fc852b4),
        (1000, 0x7c02499cb9a589ed),
    ];

    /// Made-up path-like strings, same source as above.
    const TEXT_VECTORS: &[(&str, u64)] = &[
        ("a", 0x2420662cd003acfa),
        ("abc", 0x3a912f483a4ece31),
        ("dir", 0xe6ba8c0c87a471bb),
        ("x/y", 0xe7e5c2c2b5214e32),
        ("dir/file.txt", 0xe6019656ad38e72a),
        ("alpha/beta/gamma.bin", 0x77ade6c602394f21),
        (
            "folder_one/folder_two/sample_file_name.ext",
            0xa6f29a3c173f1416,
        ),
        (
            "a/very/long/synthetic/path/that/is/longer/than/sixty/four/bytes/for/testing.dat",
            0x2661c2588475b7a4,
        ),
    ];

    #[test]
    fn empty_input_is_k2() {
        // The hash of the root directory (the empty path).
        assert_eq!(city_hash64(b""), 0x9ae1_6a3b_2f90_404f);
    }

    #[test]
    fn matches_v1_0_3_vectors_for_every_length_branch() {
        for &(n, expected) in PATTERN_VECTORS {
            assert_eq!(city_hash64(&pattern(n)), expected, "length {n}");
        }
    }

    #[test]
    fn matches_v1_0_3_vectors_for_path_like_text() {
        for &(text, expected) in TEXT_VECTORS {
            assert_eq!(city_hash64(text.as_bytes()), expected, "{text:?}");
        }
    }

    #[test]
    fn is_case_sensitive_and_byte_exact() {
        assert_ne!(city_hash64(b"dir/file.txt"), city_hash64(b"Dir/file.txt"));
        assert_ne!(city_hash64(b"dir/file.txt"), city_hash64(b"/dir/file.txt"));
    }
}
