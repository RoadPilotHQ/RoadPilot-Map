//! Builders for small synthetic sector files, following the layout in
//! `docs/formats/scs-sector-format.md`. No game data is used.

#![allow(dead_code)] // Not every test file uses every helper.

pub const VERSION: u32 = 907;

/// One item of a synthetic `.base` file.
#[derive(Clone)]
pub struct ItemSpec {
    pub item_type: u32,
    pub uid: u64,
    pub min: [f32; 5],
    pub max: [f32; 5],
    pub payload: Vec<u8>,
}

impl ItemSpec {
    pub fn new(item_type: u32, uid: u64, payload: Vec<u8>) -> Self {
        Self {
            item_type,
            uid,
            min: [0.0, -1.0, 0.0, 0.0, 0.0],
            max: [10.0, 1.0, 10.0, 10.0, 10.0],
            payload,
        }
    }
}

/// One node of a synthetic `.base` file.
#[derive(Clone, Copy)]
pub struct NodeSpec {
    pub uid: u64,
    pub position: [i32; 3],
    pub rotation: [f32; 4],
    pub backward_item: u64,
    pub forward_item: u64,
    pub flags: u32,
}

impl NodeSpec {
    pub fn new(uid: u64, backward_item: u64, forward_item: u64) -> Self {
        Self {
            uid,
            position: [256, 0, 512],
            rotation: [1.0, 0.0, 0.0, 0.0],
            backward_item,
            forward_item,
            flags: 0,
        }
    }
}

/// A synthetic `.base` file.
#[derive(Clone, Default)]
pub struct BaseSpec {
    pub items: Vec<ItemSpec>,
    pub nodes: Vec<NodeSpec>,
    pub trailing: Vec<u64>,
}

pub fn header(out: &mut Vec<u8>, version: u32) {
    out.extend(version.to_le_bytes());
    out.extend(7_668_833u32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(3u32.to_le_bytes());
}

impl BaseSpec {
    pub fn build(&self) -> Vec<u8> {
        let mut out = Vec::new();
        header(&mut out, VERSION);
        out.extend((self.items.len() as u32).to_le_bytes());
        for item in &self.items {
            out.extend(item.item_type.to_le_bytes());
            out.extend(item.uid.to_le_bytes());
            for v in item.min.iter().chain(&item.max) {
                out.extend(v.to_le_bytes());
            }
            out.extend([0u8; 5]);
            out.extend(&item.payload);
        }
        out.extend((self.nodes.len() as u32).to_le_bytes());
        for n in &self.nodes {
            out.extend(n.uid.to_le_bytes());
            for p in n.position {
                out.extend(p.to_le_bytes());
            }
            for r in n.rotation {
                out.extend(r.to_le_bytes());
            }
            out.extend(n.backward_item.to_le_bytes());
            out.extend(n.forward_item.to_le_bytes());
            out.extend(n.flags.to_le_bytes());
        }
        out.extend((self.trailing.len() as u32).to_le_bytes());
        for u in &self.trailing {
            out.extend(u.to_le_bytes());
        }
        out
    }
}

/// Builds a `.data` file from `(item uid, payload)` entries.
pub fn data_file(entries: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    header(&mut out, VERSION);
    for (uid, payload) in entries {
        out.extend(uid.to_le_bytes());
        out.extend(payload);
    }
    out.extend(u64::MAX.to_le_bytes());
    out
}

// ------------------------------------------------------------ payloads

fn u32s(out: &mut Vec<u8>, v: u32) {
    out.extend(v.to_le_bytes());
}

fn list(out: &mut Vec<u8>, uids: &[u64]) {
    u32s(out, uids.len() as u32);
    for u in uids {
        out.extend(u.to_le_bytes());
    }
}

/// Encodes a token name (characters `0-9`, `a-z`, `_`).
pub fn token(name: &str) -> u64 {
    name.bytes().rev().fold(0u64, |acc, c| {
        let digit = match c {
            b'0'..=b'9' => u64::from(c - b'0') + 1,
            b'a'..=b'z' => u64::from(c - b'a') + 11,
            _ => 37,
        };
        acc * 38 + digit
    })
}

/// Road: 212 bytes; look token at 4, start node at 192, end node at 200,
/// length (`f32`) at 208.
pub fn road(look: u64, start: u64, end: u64) -> Vec<u8> {
    road_with_length(look, start, end, 12.5)
}

pub fn road_with_length(look: u64, start: u64, end: u64, length: f32) -> Vec<u8> {
    let mut p = vec![0u8; 212];
    p[4..12].copy_from_slice(&look.to_le_bytes());
    p[192..200].copy_from_slice(&start.to_le_bytes());
    p[200..208].copy_from_slice(&end.to_le_bytes());
    p[208..212].copy_from_slice(&length.to_le_bytes());
    p
}

/// Prefab: model, 8 bytes, parts, nodes, connected items, 10 bytes,
/// 12 bytes per node, 8 bytes.
pub fn prefab(model: u64, parts: usize, nodes: &[u64], connected: &[u64]) -> Vec<u8> {
    let mut p = model.to_le_bytes().to_vec();
    p.extend([0u8; 8]);
    u32s(&mut p, parts as u32);
    p.extend(vec![0u8; 8 * parts]);
    list(&mut p, nodes);
    list(&mut p, connected);
    p.extend(vec![0u8; 10 + 12 * nodes.len() + 8]);
    p
}

pub fn cut_plane(nodes: &[u64]) -> Vec<u8> {
    let mut p = Vec::new();
    list(&mut p, nodes);
    p
}

/// A `u64`-length string as used by triggers, signs and cutscenes.
fn string(out: &mut Vec<u8>, s: &str) {
    out.extend((s.len() as u64).to_le_bytes());
    out.extend(s.as_bytes());
}

/// Sign with `boards` board templates, an optional template string with one
/// board override (both optional fields) and one sign override holding one
/// attribute of each value kind.
pub fn sign(boards: u8, template: Option<&str>) -> Vec<u8> {
    let mut p = vec![0u8; 32];
    p.push(boards);
    p.extend(vec![0u8; 24 * usize::from(boards)]);
    match template {
        None => p.extend([0u8; 8]),
        Some(t) => {
            string(&mut p, t);
            u32s(&mut p, 1); // board overrides
            p.extend([0u8; 8]);
            p.push(0x03);
            p.extend([0u8; 2 + 8]);
            u32s(&mut p, 1); // sign overrides
            p.extend([0u8; 12]);
            u32s(&mut p, 4); // attributes
            for (kind, value) in [(5i16, None), (6, Some(8)), (1, Some(1)), (2, Some(4))] {
                p.extend(kind.to_le_bytes());
                p.extend([0u8; 4]);
                match value {
                    None => string(&mut p, "text"),
                    Some(n) => p.extend(vec![0u8; n]),
                }
            }
        }
    }
    p
}

/// Trigger with one node (so the 4-byte tail is present), one action with
/// two parameters, and one action with a negative override count.
pub fn trigger(nodes: &[u64]) -> Vec<u8> {
    let mut p = Vec::new();
    list(&mut p, &[]); // tags
    list(&mut p, nodes);
    u32s(&mut p, 2);
    // Action 1: token, 1 override value, 2 parameters, 1 target tag, 8 bytes.
    p.extend(token("act").to_le_bytes());
    p.extend(1i32.to_le_bytes());
    p.extend([0u8; 4]);
    u32s(&mut p, 2);
    string(&mut p, "one");
    string(&mut p, "two");
    list(&mut p, &[5]);
    p.extend([0u8; 8]);
    // Action 2: token and a negative override count, nothing else.
    p.extend(token("other").to_le_bytes());
    p.extend((-1i32).to_le_bytes());
    if nodes.len() == 1 {
        p.extend([0u8; 4]);
    }
    p
}

/// Curve whose sub-curve mask has `sub_curves` bits set, each with
/// `heights` height offsets.
pub fn curve(sub_curves: u32, heights: u32) -> Vec<u8> {
    let mut p = vec![0u8; 36];
    let mask = if sub_curves == 0 {
        0
    } else {
        u32::MAX >> (32 - sub_curves)
    };
    u32s(&mut p, mask);
    for _ in 0..sub_curves {
        p.extend([0u8; 0x4C]);
        u32s(&mut p, heights);
        p.extend(vec![0u8; 4 * heights as usize]);
        p.extend([0u8; 0x14]);
    }
    p
}

pub fn cutscene() -> Vec<u8> {
    let mut p = Vec::new();
    list(&mut p, &[9]); // tags
    p.extend(7u64.to_le_bytes()); // node
    u32s(&mut p, 1); // actions
    u32s(&mut p, 2); // numbers
    p.extend([0u8; 8]);
    u32s(&mut p, 2); // strings
    string(&mut p, "load");
    string(&mut p, "play");
    list(&mut p, &[3]);
    p.extend([0u8; 8]);
    p
}

fn quad(out: &mut Vec<u8>) {
    out.extend(1u16.to_le_bytes());
    out.extend([0u8; 10]);
    out.extend(2u16.to_le_bytes());
    out.extend([0u8; 8]);
    out.extend([0u8; 4]);
    for (count, size) in [(3u32, 4usize), (1, 16), (2, 16)] {
        u32s(out, count);
        out.extend(vec![0u8; count as usize * size]);
    }
}

pub fn terrain() -> Vec<u8> {
    let mut p = vec![0u8; 238];
    u32s(&mut p, 2);
    p.extend([0u8; 40]);
    quad(&mut p);
    quad(&mut p);
    p.extend([0u8; 32]);
    p
}

pub fn bezier_patch() -> Vec<u8> {
    let mut p = vec![0u8; 241];
    u32s(&mut p, 1);
    p.extend([0u8; 20]);
    quad(&mut p);
    p
}

/// A valid minimal payload for every supported type.
pub fn payload_for(item_type: u32) -> Vec<u8> {
    let fixed_then_list = |fixed: usize, count: u32, elem: usize, tail: usize| {
        let mut p = vec![0u8; fixed];
        u32s(&mut p, count);
        p.extend(vec![0u8; count as usize * elem + tail]);
        p
    };
    match item_type {
        1 => terrain(),
        2 => fixed_then_list(44, 2, 4, 0),
        3 => road(token("look"), 1, 2),
        4 => prefab(token("model"), 1, &[1, 2], &[3]),
        5 => fixed_then_list(24, 1, 8, 36),
        6 => fixed_then_list(32, 2, 12, 0),
        7 => fixed_then_list(16, 1, 8, 0),
        8 => cut_plane(&[1, 2]),
        12 => vec![0u8; 24],
        18 => vec![0u8; 16],
        19 => vec![0u8; 36],
        22 => fixed_then_list(28, 1, 8, 0),
        34 => trigger(&[1]),
        35 => vec![0u8; 20],
        36 => sign(1, Some("tmpl")),
        37 => vec![0u8; 24],
        38 => {
            let mut p = Vec::new();
            list(&mut p, &[1]);
            list(&mut p, &[2, 3]);
            p.extend([0u8; 12]);
            p
        }
        39 => bezier_patch(),
        41 => {
            let mut p = Vec::new();
            list(&mut p, &[1, 2]);
            p.extend([0u8; 8]);
            for (count, size) in [(1u32, 28usize), (2, 16), (1, 8)] {
                u32s(&mut p, count);
                p.extend(vec![0u8; count as usize * size]);
            }
            p
        }
        42 => {
            let mut p = Vec::new();
            list(&mut p, &[1, 2, 3]);
            p.extend([0u8; 4]);
            p
        }
        44 => curve(2, 3),
        46 => cutscene(),
        48 => fixed_then_list(16, 2, 8, 0),
        other => panic!("no payload helper for type {other}"),
    }
}

/// Deterministic pseudo-random numbers (xorshift), for mutation tests.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}
