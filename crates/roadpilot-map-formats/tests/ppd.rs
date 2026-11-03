//! Prefab descriptor (`.ppd`) parser on synthetic files (no game data).

use roadpilot_map_formats::ParseError;
use roadpilot_map_formats::Token;
use roadpilot_map_formats::ppd::{SUPPORTED_PPD_VERSION, parse_ppd};

/// Entry sizes per block, in offset order (see `docs/formats/scs-prefab-junctions.md`).
const SIZES: [usize; 12] = [104, 132, 52, 84, 36, 12, 12, 8, 48, 48, 16, 188];
/// The count each block uses.
const COUNT_OF: [usize; 12] = [0, 1, 2, 3, 4, 5, 5, 6, 7, 8, 9, 10];

struct Spec {
    /// Node position, input slots, output slots (-1 = empty).
    nodes: Vec<([f32; 3], [i32; 8], [i32; 8])>,
    /// Curve next links, prev links and traffic rule token.
    curves: Vec<(Vec<i32>, Vec<i32>, u64)>,
    /// Semaphore profile tokens.
    semaphores: Vec<u64>,
    /// Count of the last block (not decoded).
    last_block: u32,
}

fn put<const N: usize>(buf: &mut [u8], at: usize, bytes: [u8; N]) {
    buf[at..at + N].copy_from_slice(&bytes);
}

fn build(spec: &Spec) -> Vec<u8> {
    let mut counts = [0u32; 11];
    counts[0] = spec.nodes.len() as u32;
    counts[1] = spec.curves.len() as u32;
    counts[3] = spec.semaphores.len() as u32;
    counts[10] = spec.last_block;
    let mut out = Vec::new();
    out.extend(SUPPORTED_PPD_VERSION.to_le_bytes());
    for c in counts {
        out.extend(c.to_le_bytes());
    }
    let mut offset = 0x60usize;
    for k in 0..12 {
        out.extend((offset as u32).to_le_bytes());
        offset += counts[COUNT_OF[k]] as usize * SIZES[k];
    }
    for (pos, inputs, outputs) in &spec.nodes {
        let mut r = vec![0u8; 104];
        for (i, v) in pos.iter().chain(&[0.0, 0.0, -1.0]).enumerate() {
            put(&mut r, 0x10 + 4 * i, v.to_le_bytes());
        }
        for (i, v) in inputs.iter().chain(outputs).enumerate() {
            put(&mut r, 0x28 + 4 * i, v.to_le_bytes());
        }
        out.extend(r);
    }
    for (i, (next, prev, rule)) in spec.curves.iter().enumerate() {
        let mut r = vec![0u8; 132];
        put(&mut r, 0, (i as u64 + 1).to_le_bytes());
        put(&mut r, 40, 1f32.to_le_bytes());
        put(&mut r, 56, 1f32.to_le_bytes());
        put(&mut r, 72, 5f32.to_le_bytes());
        for k in 0..4 {
            put(
                &mut r,
                76 + 4 * k,
                next.get(k).copied().unwrap_or(-1).to_le_bytes(),
            );
            put(
                &mut r,
                92 + 4 * k,
                prev.get(k).copied().unwrap_or(-1).to_le_bytes(),
            );
        }
        put(&mut r, 108, (next.len() as u32).to_le_bytes());
        put(&mut r, 112, (prev.len() as u32).to_le_bytes());
        put(&mut r, 116, (-1i32).to_le_bytes());
        put(&mut r, 120, rule.to_le_bytes());
        out.extend(r);
    }
    for profile in &spec.semaphores {
        let mut r = vec![0u8; 84];
        put(&mut r, 0, 3f32.to_le_bytes());
        put(&mut r, 12, 1f32.to_le_bytes());
        put(&mut r, 56, profile.to_le_bytes());
        out.extend(r);
    }
    out.resize(out.len() + spec.last_block as usize * 188, 0);
    out
}

const NONE: [i32; 8] = [-1; 8];

fn slots(first: &[i32]) -> [i32; 8] {
    let mut s = NONE;
    s[..first.len()].copy_from_slice(first);
    s
}

/// Two nodes; curve 0 enters at node 0 and continues into curves 1 and 2,
/// which leave at node 1 (slots 0 and 1).
fn valid() -> Spec {
    Spec {
        nodes: vec![
            ([0.0, 0.0, 0.0], slots(&[0]), NONE),
            ([0.0, 0.0, -20.0], NONE, slots(&[1, 2])),
        ],
        curves: vec![
            (vec![1, 2], vec![], 0),
            (vec![], vec![0], 42),
            (vec![], vec![0], 0),
        ],
        semaphores: vec![7],
        last_block: 1,
    }
}

#[test]
fn parses_nodes_curves_and_semaphores() {
    let d = parse_ppd(&build(&valid())).expect("parses");
    assert_eq!(d.nodes.len(), 2);
    assert_eq!(d.nodes[0].input_slots[0], Some(0));
    assert_eq!(d.nodes[0].input_slots[1], None);
    assert_eq!(d.nodes[1].output_slots[..2], [Some(1), Some(2)]);
    assert_eq!(d.nodes[1].position, [0.0, 0.0, -20.0]);
    assert_eq!(d.nodes[1].direction, [0.0, 0.0, -1.0]);
    assert_eq!(d.curves[0].next, vec![1, 2]);
    assert_eq!(d.curves[2].prev, vec![0]);
    assert_eq!(d.curves[0].traffic_rule, None);
    assert_eq!(d.curves[1].traffic_rule, Some(Token(42)));
    assert_eq!(d.curves[1].length, 5.0);
    assert_eq!(d.curves[1].unknown_116, -1);
    assert_eq!(d.semaphores.len(), 1);
    assert_eq!(d.semaphores[0].position, [3.0, 0.0, 0.0]);
    assert_eq!(d.semaphores[0].profile, Token(7));
    assert_eq!(d.other.last_block, 1);
}

#[test]
fn rejects_other_versions_and_empty_files() {
    let mut bytes = build(&valid());
    bytes[0] = 24;
    assert!(matches!(
        parse_ppd(&bytes),
        Err(ParseError::UnsupportedVersion { found: 24 })
    ));
    assert!(matches!(parse_ppd(&[]), Err(ParseError::Empty)));
}

#[test]
fn rejects_truncated_and_trailing_data_without_panicking() {
    let bytes = build(&valid());
    for len in 1..bytes.len() {
        assert!(parse_ppd(&bytes[..len]).is_err(), "prefix of {len} bytes");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(matches!(
        parse_ppd(&longer),
        Err(ParseError::TrailingBytes { .. })
    ));
}

#[test]
fn rejects_non_contiguous_block_offsets() {
    let mut bytes = build(&valid());
    // Offset of the curve block.
    let at = 0x34;
    let v = u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes")) + 4;
    bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
    assert!(matches!(
        parse_ppd(&bytes),
        Err(ParseError::InvalidRecord { .. })
    ));
}

#[test]
fn rejects_huge_counts_without_allocating() {
    let mut bytes = build(&valid());
    bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(parse_ppd(&bytes).is_err());
}

#[test]
fn rejects_out_of_range_curve_indices() {
    let mut spec = valid();
    spec.nodes[0].1[0] = 3;
    assert!(matches!(
        parse_ppd(&build(&spec)),
        Err(ParseError::InvalidRecord { reason, .. }) if reason.contains("out of range")
    ));
    let mut spec = valid();
    spec.curves[0].0 = vec![1, -2];
    assert!(parse_ppd(&build(&spec)).is_err());
}

#[test]
fn rejects_link_counts_that_differ_from_links() {
    let mut bytes = build(&valid());
    // Next count of curve 0.
    let at = 0x60 + 2 * 104 + 108;
    bytes[at..at + 4].copy_from_slice(&1u32.to_le_bytes());
    assert!(matches!(
        parse_ppd(&bytes),
        Err(ParseError::InvalidRecord { reason, .. }) if reason.contains("counts")
    ));
}

#[test]
fn rejects_links_that_are_not_mirrored() {
    let mut spec = valid();
    spec.curves[2].1 = vec![];
    assert!(matches!(
        parse_ppd(&build(&spec)),
        Err(ParseError::InvalidRecord { reason, .. }) if reason.contains("mirrored")
    ));
}
