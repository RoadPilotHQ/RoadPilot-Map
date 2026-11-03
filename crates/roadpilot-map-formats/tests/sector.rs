//! Tests for the sector parsers on synthetic files built in code.

mod common;

use common::{BaseSpec, ItemSpec, NodeSpec, Rng, data_file, payload_for, token};
use roadpilot_map_formats::{
    ItemData, ItemType, Limits, ParseError, SUPPORTED_VERSION, Token, parse_base,
    parse_base_with_limits, parse_data,
};

/// A road from node 10 to node 11, a prefab on nodes 11 and 12 with one
/// attached sign, and the sign. Nodes link back to the road.
fn sample() -> BaseSpec {
    BaseSpec {
        items: vec![
            ItemSpec::new(3, 100, common::road(token("road_a"), 10, 11)),
            ItemSpec::new(
                4,
                200,
                common::prefab(token("junction"), 2, &[11, 12], &[300]),
            ),
            ItemSpec::new(36, 300, common::sign(2, Some("sign_tmpl"))),
        ],
        nodes: vec![
            NodeSpec::new(10, 0, 100),
            NodeSpec::new(11, 100, 200),
            NodeSpec::new(12, 200, 0),
        ],
        trailing: vec![300],
    }
}

fn set_u32(bytes: &mut [u8], at: usize, v: u32) {
    bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn item_offset(spec: &BaseSpec, index: usize) -> usize {
    20 + spec.items[..index]
        .iter()
        .map(|i| 57 + i.payload.len())
        .sum::<usize>()
}

// ------------------------------------------------------------- header

#[test]
fn parses_a_valid_header() {
    let bytes = BaseSpec::default().build();
    let base = parse_base(&bytes).expect("valid empty sector");
    assert_eq!(base.header.version, SUPPORTED_VERSION);
    assert_eq!(base.header.unknown, [7_668_833, 0, 3]);
    assert!(base.items.is_empty() && base.nodes.is_empty() && base.trailing_item_uids.is_empty());
}

#[test]
fn accepts_only_the_supported_version() {
    assert_eq!(SUPPORTED_VERSION, 907);
    for version in [0u32, 906, 908, u32::MAX] {
        let mut bytes = sample().build();
        set_u32(&mut bytes, 0, version);
        assert_eq!(
            parse_base(&bytes),
            Err(ParseError::UnsupportedVersion { found: version })
        );
    }
}

#[test]
fn rejects_an_empty_file() {
    assert_eq!(parse_base(&[]), Err(ParseError::Empty));
    let base_bytes = BaseSpec::default().build();
    let base = parse_base(&base_bytes).expect("valid");
    assert_eq!(parse_data(&[], &base), Err(ParseError::Empty));
}

#[test]
fn rejects_a_truncated_header() {
    let bytes = BaseSpec::default().build();
    for len in 1..20 {
        assert!(
            matches!(parse_base(&bytes[..len]), Err(ParseError::Truncated { .. })),
            "{len}-byte prefix"
        );
    }
}

// ------------------------------------------------------------- records

#[test]
fn parses_multiple_records_with_confirmed_fields() {
    let bytes = sample().build();
    let base = parse_base(&bytes).expect("valid sample");
    assert_eq!(base.items.len(), 3);
    assert_eq!(base.nodes.len(), 3);
    assert_eq!(base.trailing_item_uids, vec![300]);

    let road = &base.items[0];
    assert_eq!(road.item_type, ItemType::ROAD);
    assert_eq!(road.uid, 100);
    assert_eq!(road.offset, 20);
    assert_eq!(road.payload.len(), 212);
    let ItemData::Road(r) = &road.data else {
        panic!("road data expected");
    };
    assert_eq!(r.look.name().as_deref(), Some("road_a"));
    assert_eq!((r.start_node, r.end_node), (10, 11));

    let ItemData::Prefab(p) = &base.items[1].data else {
        panic!("prefab data expected");
    };
    assert_eq!(p.model, Token(token("junction")));
    assert_eq!(p.nodes, vec![11, 12]);
    assert_eq!(p.connected_items, vec![300]);

    assert_eq!(base.items[2].item_type, ItemType::SIGN);
    assert_eq!(base.items[2].data, ItemData::Other);
    assert_eq!(base.items[2].bounds.max[0], 10.0);

    let n = base.nodes[1];
    assert_eq!((n.uid, n.backward_item, n.forward_item), (11, 100, 200));
    assert_eq!(n.position_units(), [1.0, 0.0, 2.0]);
}

#[test]
fn parses_every_supported_item_type() {
    let items: Vec<ItemSpec> = ItemType::SUPPORTED
        .iter()
        .enumerate()
        .map(|(i, t)| ItemSpec::new(t.0, 1000 + i as u64, payload_for(t.0)))
        .collect();
    let spec = BaseSpec {
        items,
        nodes: vec![NodeSpec::new(1, 0, 0)],
        trailing: vec![],
    };
    let bytes = spec.build();
    let base = parse_base(&bytes).expect("one item of every type");
    assert_eq!(base.items.len(), ItemType::SUPPORTED.len());
    for (item, spec) in base.items.iter().zip(&spec.items) {
        assert_eq!(item.payload, &spec.payload[..], "{:?}", item.item_type);
    }
    assert_eq!(base.nodes.len(), 1);
}

#[test]
fn a_wrong_payload_length_breaks_the_walk() {
    // Items have no length field, so one extra byte in any payload shifts
    // everything after it. The parser must notice and not return a result.
    for t in ItemType::SUPPORTED {
        let mut payload = payload_for(t.0);
        payload.push(0);
        let spec = BaseSpec {
            items: vec![ItemSpec::new(t.0, 1, payload)],
            nodes: vec![NodeSpec::new(1, 0, 1)],
            trailing: vec![],
        };
        assert!(parse_base(&spec.build()).is_err(), "{t:?}");
    }
}

#[test]
fn variable_parts_of_complex_types_are_measured() {
    let cases: Vec<(u32, Vec<u8>)> = vec![
        (36, common::sign(0, None)),
        (36, common::sign(3, None)),
        (36, common::sign(0, Some("a_longer_template_name"))),
        (34, common::trigger(&[1, 2])),
        (34, common::trigger(&[1])),
        (44, common::curve(0, 0)),
        (44, common::curve(1, 0)),
        (44, common::curve(32, 1)),
    ];
    for (t, payload) in cases {
        let len = payload.len();
        let spec = BaseSpec {
            items: vec![ItemSpec::new(t, 1, payload)],
            ..BaseSpec::default()
        };
        let bytes = spec.build();
        let base = parse_base(&bytes).unwrap_or_else(|e| panic!("type {t}: {e}"));
        assert_eq!(base.items[0].payload.len(), len, "type {t}");
    }
}

#[test]
fn rejects_an_unknown_item_type() {
    let mut spec = sample();
    spec.items.insert(1, ItemSpec::new(13, 150, vec![0; 40]));
    let bytes = spec.build();
    assert_eq!(
        parse_base(&bytes),
        Err(ParseError::UnknownItemType {
            index: 1,
            offset: item_offset(&spec, 1),
            item_type: 13
        })
    );
}

#[test]
fn rejects_every_truncation_without_panicking() {
    let bytes = sample().build();
    for len in 1..bytes.len() {
        assert!(parse_base(&bytes[..len]).is_err(), "{len}-byte prefix");
    }
}

#[test]
fn rejects_a_truncated_record() {
    let spec = sample();
    let bytes = spec.build();
    // Cut in the middle of the prefab payload.
    let cut = item_offset(&spec, 1) + 57 + 10;
    assert!(matches!(
        parse_base(&bytes[..cut]),
        Err(ParseError::Truncated { .. })
    ));
}

#[test]
fn rejects_counts_larger_than_the_file() {
    // Item count.
    let mut bytes = sample().build();
    set_u32(&mut bytes, 16, 500);
    assert!(matches!(
        parse_base(&bytes),
        Err(ParseError::InvalidCount { what: "item", .. })
    ));
    // Prefab node count (payload offset 16 + 4 + 8 * parts).
    let spec = sample();
    let mut bytes = spec.build();
    let at = item_offset(&spec, 1) + 57 + 20 + 8 * 2;
    set_u32(&mut bytes, at, 1_000_000);
    assert!(matches!(
        parse_base(&bytes),
        Err(ParseError::InvalidCount {
            count: 1_000_000,
            ..
        })
    ));
    // Node count.
    let spec = sample();
    let mut bytes = spec.build();
    let at = item_offset(&spec, 3);
    set_u32(&mut bytes, at, 4_000);
    assert!(matches!(
        parse_base(&bytes),
        Err(ParseError::InvalidCount { what: "node", .. })
    ));
}

#[test]
fn huge_counts_do_not_overflow_or_allocate() {
    // u32::MAX in every count field of every type that has one: each
    // gives an error, never a panic or a huge allocation.
    for t in ItemType::SUPPORTED {
        let payload = payload_for(t.0);
        for at in 0..payload.len().saturating_sub(3) {
            let mut p = payload.clone();
            p[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            let spec = BaseSpec {
                items: vec![ItemSpec::new(t.0, 1, p)],
                ..BaseSpec::default()
            };
            let _ = parse_base(&spec.build());
        }
    }
}

#[test]
fn rejects_manipulated_item_counts() {
    for count in [u32::MAX, 2_000_000] {
        let mut bytes = sample().build();
        set_u32(&mut bytes, 16, count);
        assert!(matches!(
            parse_base(&bytes),
            Err(ParseError::LimitExceeded {
                what: "item count",
                ..
            })
        ));
    }
    let limits = Limits {
        max_items: 2,
        ..Limits::default()
    };
    assert!(matches!(
        parse_base_with_limits(&sample().build(), &limits),
        Err(ParseError::LimitExceeded {
            value: 3,
            limit: 2,
            ..
        })
    ));
    let limits = Limits {
        max_nodes: 2,
        ..Limits::default()
    };
    assert!(matches!(
        parse_base_with_limits(&sample().build(), &limits),
        Err(ParseError::LimitExceeded {
            what: "node count",
            ..
        })
    ));
    let limits = Limits {
        max_file_size: 100,
        ..Limits::default()
    };
    assert!(matches!(
        parse_base_with_limits(&sample().build(), &limits),
        Err(ParseError::LimitExceeded {
            what: "file size",
            ..
        })
    ));
}

#[test]
fn rejects_zero_uids_where_a_reference_is_required() {
    let cases: Vec<(&str, BaseSpec)> = vec![
        ("item uid", {
            let mut s = sample();
            s.items[2].uid = 0;
            s
        }),
        ("road start node", {
            let mut s = sample();
            s.items[0].payload = common::road(token("a"), 0, 11);
            s
        }),
        ("prefab node", {
            let mut s = sample();
            s.items[1].payload = common::prefab(token("p"), 0, &[11, 0], &[]);
            s
        }),
        ("node uid", {
            let mut s = sample();
            s.nodes[0].uid = 0;
            s
        }),
        ("trailing item uid", {
            let mut s = sample();
            s.trailing = vec![0];
            s
        }),
    ];
    for (what, spec) in cases {
        assert!(
            matches!(parse_base(&spec.build()), Err(ParseError::ZeroUid { what: w, .. }) if w == what),
            "{what}"
        );
    }
}

#[test]
fn rejects_invalid_bounds_and_rotations() {
    let mut s = sample();
    s.items[0].min[2] = 20.0; // min > max
    assert!(matches!(
        parse_base(&s.build()),
        Err(ParseError::InvalidRecord { offset: 20, .. })
    ));
    let mut s = sample();
    s.items[1].max[0] = f32::NAN;
    assert!(matches!(
        parse_base(&s.build()),
        Err(ParseError::InvalidRecord { .. })
    ));
    let mut s = sample();
    s.nodes[2].rotation = [0.5, 0.0, 0.0, 0.0];
    assert!(matches!(
        parse_base(&s.build()),
        Err(ParseError::InvalidRecord { .. })
    ));
}

#[test]
fn rejects_trailing_bytes() {
    let mut bytes = sample().build();
    bytes.push(0);
    assert!(matches!(
        parse_base(&bytes),
        Err(ParseError::TrailingBytes { .. })
    ));
}

#[test]
fn random_mutations_never_panic() {
    let bytes = sample().build();
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for _ in 0..5000 {
        let mut m = bytes.clone();
        for _ in 0..1 + rng.next() % 4 {
            let at = (rng.next() % m.len() as u64) as usize;
            m[at] = rng.next() as u8;
        }
        let _ = parse_base(&m);
    }
}

// ------------------------------------------------------------- .data

#[test]
fn splits_a_data_file_into_entries() {
    let base_bytes = sample().build();
    let base = parse_base(&base_bytes).expect("valid");
    let bytes = data_file(&[(100, vec![1, 2, 3]), (200, vec![4; 40])]);
    let data = parse_data(&bytes, &base).expect("valid data");
    assert_eq!(data.header.version, SUPPORTED_VERSION);
    assert_eq!(data.entries.len(), 2);
    assert_eq!(data.entries[0].item_uid, 100);
    assert_eq!(data.entries[0].item_type, ItemType::ROAD);
    assert_eq!(data.entries[0].offset, 16);
    assert_eq!(data.entries[0].payload, &[1, 2, 3]);
    assert_eq!(data.entries[1].item_type, ItemType::PREFAB);
    assert_eq!(data.entries[1].payload, &[4; 40][..]);
}

#[test]
fn a_sector_without_roads_or_prefabs_has_an_empty_data_file() {
    let base_bytes = BaseSpec::default().build();
    let base = parse_base(&base_bytes).expect("valid");
    let bytes = data_file(&[]);
    assert_eq!(bytes.len(), 24);
    assert!(parse_data(&bytes, &base).expect("valid").entries.is_empty());
}

#[test]
fn rejects_inconsistent_data_files() {
    let base_bytes = sample().build();
    let base = parse_base(&base_bytes).expect("valid");

    // Wrong order, missing entry, unknown first UID.
    for entries in [
        vec![(200, vec![0]), (100, vec![0])],
        vec![(100, vec![0])],
        vec![(999, vec![0]), (200, vec![0])],
    ] {
        assert!(
            matches!(
                parse_data(&data_file(&entries), &base),
                Err(ParseError::DataEntryMissing { .. })
            ),
            "{entries:?}"
        );
    }
    // Missing end marker.
    let mut bytes = data_file(&[(100, vec![0]), (200, vec![0])]);
    let len = bytes.len();
    bytes[len - 1] = 0;
    assert!(matches!(
        parse_data(&bytes, &base),
        Err(ParseError::InvalidRecord { .. })
    ));
    // Unsupported version.
    let mut bytes = data_file(&[(100, vec![0]), (200, vec![0])]);
    set_u32(&mut bytes, 0, 906);
    assert_eq!(
        parse_data(&bytes, &base),
        Err(ParseError::UnsupportedVersion { found: 906 })
    );
    // Too short for header and end marker.
    assert!(parse_data(&bytes[..20], &base).is_err());
    // Entries for a sector without roads or prefabs.
    let empty_bytes = BaseSpec::default().build();
    let empty = parse_base(&empty_bytes).expect("valid");
    assert!(matches!(
        parse_data(&data_file(&[(100, vec![0])]), &empty),
        Err(ParseError::TrailingBytes { offset: 16 })
    ));
}

#[test]
fn every_data_truncation_is_rejected() {
    let base_bytes = sample().build();
    let base = parse_base(&base_bytes).expect("valid");
    let bytes = data_file(&[(100, vec![7; 30]), (200, vec![8; 30])]);
    for len in 1..bytes.len() {
        assert!(
            parse_data(&bytes[..len], &base).is_err(),
            "{len}-byte prefix"
        );
    }
}
