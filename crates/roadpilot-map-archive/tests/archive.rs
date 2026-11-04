//! Integration tests for the HashFS v2 reader, using synthetic archives only.

mod common;

use std::fs;

use common::{FLAG_DIRECTORY, META_PLAIN, Method, TempArchive, TestArchive, pattern, sample};
use roadpilot_map_archive::{Archive, DirEntry, EntryKind, Error, Limits, PathCheck, path_hash};

fn open(name: &str, bytes: &[u8]) -> (TempArchive, Result<Archive, Error>) {
    let temp = TempArchive::new(name, bytes);
    let archive = Archive::open(&temp.path);
    (temp, archive)
}

fn open_ok(name: &str, bytes: &[u8]) -> (TempArchive, Archive) {
    let (temp, archive) = open(name, bytes);
    (temp, archive.expect("synthetic archive opens"))
}

fn patch(mut bytes: Vec<u8>, at: usize, value: &[u8]) -> Vec<u8> {
    bytes[at..at + value.len()].copy_from_slice(value);
    bytes
}

// ------------------------------------------------------------- opening

#[test]
fn opens_a_valid_archive() {
    let (_t, archive) = open_ok("opens_valid", &sample().build());
    // 4 files + 3 directories (root, def, def/world).
    assert_eq!(archive.entries().len(), 7);
}

#[test]
fn rejects_files_that_are_not_hashfs() {
    for (i, bytes) in [
        b"".to_vec(),
        b"PK\x03\x04 a zip file".to_vec(),
        pattern(200),
        b"SCX#".repeat(20),
    ]
    .iter()
    .enumerate()
    {
        let (_t, result) = open(&format!("not_hashfs_{i}"), bytes);
        assert!(
            matches!(result, Err(Error::NotHashFs)),
            "case {i}: {result:?}"
        );
    }
}

#[test]
fn rejects_unsupported_versions() {
    for version in [0u16, 1, 3, 0xFFFF] {
        let bytes = patch(sample().build(), 4, &version.to_le_bytes());
        let (_t, result) = open(&format!("version_{version}"), &bytes);
        assert!(
            matches!(result, Err(Error::UnsupportedVersion(v)) if v == version),
            "{result:?}"
        );
    }
}

#[test]
fn rejects_unsupported_header_fields() {
    let (_t, result) = open("hash_method", &patch(sample().build(), 8, b"MD5\0"));
    assert!(matches!(result, Err(Error::UnsupportedHashMethod(m)) if &m == b"MD5\0"));

    let (_t, result) = open("salt", &patch(sample().build(), 6, &7u16.to_le_bytes()));
    assert!(matches!(result, Err(Error::UnsupportedSalt(7))));

    let (_t, result) = open("platform", &patch(sample().build(), 52, &[1]));
    assert!(matches!(result, Err(Error::UnsupportedPlatform(1))));
}

#[test]
fn rejects_a_truncated_header() {
    let bytes = sample().build();
    let (_t, result) = open("truncated_header", &bytes[..20]);
    assert!(
        matches!(result, Err(Error::TruncatedHeader { len: 20 })),
        "{result:?}"
    );
}

#[test]
fn rejects_every_truncation_of_the_archive() {
    // The index tables are at the end, so every shorter file must fail to
    // open, with an error and without a panic.
    let bytes = sample().build();
    for len in 0..bytes.len() {
        let (_t, result) = open("truncated_every", &bytes[..len]);
        assert!(result.is_err(), "a {len}-byte prefix must not open");
    }
}

// ------------------------------------------------------------- index

#[test]
fn reads_the_index() {
    let (_t, archive) = open_ok("reads_index", &sample().build());
    let kind_of = |path: &str| {
        let hash = path_hash(path).expect("valid path");
        archive
            .entries()
            .iter()
            .find(|e| e.hash() == hash)
            .map(|e| (e.kind(), e.size()))
    };
    assert_eq!(kind_of("readme.txt"), Some((EntryKind::File, Some(23))));
    assert_eq!(kind_of("def/b.bin"), Some((EntryKind::File, Some(4096))));
    assert!(matches!(kind_of(""), Some((EntryKind::Directory, Some(_)))));
    assert!(matches!(
        kind_of("def/world"),
        Some((EntryKind::Directory, Some(_)))
    ));
}

#[test]
fn rejects_more_entries_than_the_limit() {
    let temp = TempArchive::new("entry_limit", &sample().build());
    let limits = Limits {
        max_entries: 6,
        ..Limits::default()
    };
    let result = Archive::open_with_limits(&temp.path, limits);
    assert!(
        matches!(
            result,
            Err(Error::LimitExceeded {
                what: "entry count",
                value: 7,
                limit: 6
            })
        ),
        "{result:?}"
    );
}

// ------------------------------------------------------------- lookup

#[test]
fn finds_existing_entries_with_confirmed_paths() {
    let (_t, archive) = open_ok("finds_existing", &sample().build());
    for (path, kind) in [
        ("readme.txt", EntryKind::File),
        ("def/a.txt", EntryKind::File),
        ("def/world/road.sii", EntryKind::File),
        ("def", EntryKind::Directory),
        ("def/world", EntryKind::Directory),
    ] {
        let found = archive
            .find(path)
            .expect("lookup works")
            .expect("entry exists");
        assert_eq!(found.entry.kind(), kind, "{path}");
        assert_eq!(found.check, PathCheck::Listed, "{path}");
        assert!(found.is_verified());
        assert_eq!(found.entry.hash(), path_hash(path).expect("valid path"));
    }
    let root = archive
        .find("")
        .expect("lookup works")
        .expect("root exists");
    assert_eq!(root.check, PathCheck::Root);
}

#[test]
fn missing_entries_give_a_clear_result() {
    let (_t, archive) = open_ok("missing", &sample().build());
    assert!(
        archive
            .find("def/missing.txt")
            .expect("lookup works")
            .is_none()
    );
    assert!(matches!(
        archive.read_file("/def/missing.txt"),
        Err(Error::NotFound { path }) if path == "def/missing.txt"
    ));
    assert!(matches!(
        archive.list_dir("nowhere"),
        Err(Error::NotFound { .. })
    ));
}

#[test]
fn normalizes_paths() {
    let (_t, archive) = open_ok("normalizes", &sample().build());
    assert_eq!(
        archive.read_file("/def/a.txt").expect("leading slash"),
        b"alpha"
    );
    assert!(archive.list_dir("def/").is_ok(), "trailing slash");
    assert!(archive.list_dir("/").is_ok(), "root as '/'");
    // Paths are case-sensitive bytes.
    assert!(archive.find("DEF/a.txt").expect("lookup works").is_none());
    for bad in [
        "def//a.txt",
        "def/../readme.txt",
        "./readme.txt",
        "def\\a.txt",
        "def/a\0",
    ] {
        assert!(
            matches!(archive.find(bad), Err(Error::InvalidPath { .. })),
            "{bad:?}"
        );
        assert!(matches!(
            archive.read_file(bad),
            Err(Error::InvalidPath { .. })
        ));
    }
}

#[test]
fn path_hash_is_cityhash64_v1_0_3() {
    assert_eq!(path_hash("").expect("root"), 0x9ae1_6a3b_2f90_404f);
    assert_eq!(
        path_hash("dir/file.txt").expect("valid"),
        0xe601_9656_ad38_e72a
    );
    assert_eq!(
        path_hash("/dir/file.txt").expect("valid"),
        0xe601_9656_ad38_e72a
    );
}

#[test]
fn duplicate_hashes_are_ambiguous() {
    let hash = path_hash("def/a.txt").expect("valid path");
    let bytes = sample().unlisted_file(hash, b"other").build();
    let (_t, archive) = open_ok("ambiguous", &bytes);
    assert!(matches!(
        archive.find("def/a.txt"),
        Err(Error::AmbiguousPath { path }) if path == "def/a.txt"
    ));
    assert!(matches!(
        archive.read_file("def/a.txt"),
        Err(Error::AmbiguousPath { .. })
    ));
    // Other entries are unaffected.
    assert_eq!(archive.read_file("readme.txt").expect("readable").len(), 23);
}

#[test]
fn a_hash_match_without_listing_is_not_trusted() {
    // An entry has the hash of "def/ghost.txt", but the "def" listing does
    // not name it: a collision or an incomplete listing.
    let hash = path_hash("def/ghost.txt").expect("valid path");
    let bytes = sample().unlisted_file(hash, b"unlisted").build();
    let (_t, archive) = open_ok("not_listed", &bytes);

    let found = archive
        .find("def/ghost.txt")
        .expect("lookup works")
        .expect("hash matches");
    assert_eq!(found.check, PathCheck::NotListed);
    assert!(!found.is_verified());
    assert!(matches!(
        archive.read_file("def/ghost.txt"),
        Err(Error::UnverifiedPath {
            check: PathCheck::NotListed,
            ..
        })
    ));
    // The caller may still decide to read it explicitly.
    assert_eq!(archive.read(&found.entry).expect("readable"), b"unlisted");
}

#[test]
fn entries_without_parent_listing_are_not_verified() {
    // Like some real archives: no root listing, so top-level names cannot
    // be confirmed. Entries below a listed directory still can.
    let bytes = sample().without_listing("").build();
    let (_t, archive) = open_ok("no_root_listing", &bytes);
    let found = archive
        .find("readme.txt")
        .expect("lookup works")
        .expect("hash matches");
    assert_eq!(found.check, PathCheck::NoParentListing);
    assert!(matches!(
        archive.read_file("readme.txt"),
        Err(Error::UnverifiedPath {
            check: PathCheck::NoParentListing,
            ..
        })
    ));
    assert!(matches!(archive.find(""), Ok(None)));
    assert_eq!(
        archive.read_file("def/a.txt").expect("listed in def"),
        b"alpha"
    );
}

#[test]
fn listing_marker_must_match_the_entry_kind() {
    // "def/a.txt" is listed as a file, but its entry is turned into a
    // directory: the listing does not confirm it.
    let bytes = sample().build_with(|t| {
        t.row_mut("def/a.txt").flags |= FLAG_DIRECTORY;
        let w = t.header_word_mut("def/a.txt");
        *w = (*w & 0x00FF_FFFF) | (u32::from(common::META_DIRECTORY) << 24);
    });
    let (_t, archive) = open_ok("marker_mismatch", &bytes);
    let found = archive
        .find("def/a.txt")
        .expect("lookup")
        .expect("hash matches");
    assert_eq!(found.entry.kind(), EntryKind::Directory);
    assert_eq!(found.check, PathCheck::NotListed);
}

// ------------------------------------------------------------- reading

#[test]
fn reads_a_selected_file() {
    let (_t, archive) = open_ok("reads_selected", &sample().build());
    assert_eq!(archive.read_file("def/a.txt").expect("readable"), b"alpha");
    assert_eq!(
        archive.read_file("readme.txt").expect("readable"),
        b"synthetic test archive\n"
    );
}

#[test]
fn reads_multiple_files_independently() {
    let (_t, archive) = open_ok("reads_multiple", &sample().build());
    let expected = [
        ("def/b.bin", pattern(4096)),
        ("def/a.txt", b"alpha".to_vec()),
        (
            "def/world/road.sii",
            b"synthetic text, repeated. synthetic text, repeated.".to_vec(),
        ),
        ("readme.txt", b"synthetic test archive\n".to_vec()),
    ];
    // Any order, repeated reads.
    for (path, data) in expected.iter().chain(expected.iter().rev()) {
        assert_eq!(&archive.read_file(path).expect("readable"), data, "{path}");
    }
    // Concurrent reads from several threads.
    std::thread::scope(|s| {
        for (path, data) in &expected {
            let archive = &archive;
            s.spawn(move || {
                for _ in 0..20 {
                    assert_eq!(&archive.read_file(path).expect("readable"), data);
                }
            });
        }
    });
}

#[test]
fn reads_zlib_compressed_files() {
    let data = pattern(100_000);
    let bytes = TestArchive::new().zlib_file("big.bin", &data).build();
    let (_t, archive) = open_ok("zlib", &bytes);
    assert_eq!(archive.read_file("big.bin").expect("readable"), data);
}

#[test]
fn reads_empty_files() {
    let bytes = TestArchive::new()
        .file("empty_stored", b"")
        .zlib_file("empty_zlib", b"")
        .build();
    let (_t, archive) = open_ok("empty_files", &bytes);
    assert!(
        archive
            .read_file("empty_stored")
            .expect("readable")
            .is_empty()
    );
    assert!(
        archive
            .read_file("empty_zlib")
            .expect("readable")
            .is_empty()
    );
}

#[test]
fn lists_directories() {
    let (_t, archive) = open_ok("lists", &sample().build());
    let entry = |name: &str, is_dir| DirEntry {
        name: name.to_owned(),
        is_dir,
    };
    assert_eq!(
        archive.list_dir("").expect("root listing"),
        vec![entry("def", true), entry("readme.txt", false)]
    );
    assert_eq!(
        archive.list_dir("def").expect("listing"),
        vec![
            entry("world", true),
            entry("a.txt", false),
            entry("b.bin", false)
        ]
    );
}

#[test]
fn files_and_directories_are_not_interchangeable() {
    let (_t, archive) = open_ok("kinds", &sample().build());
    assert!(matches!(
        archive.read_file("def"),
        Err(Error::NotAFile { .. })
    ));
    assert!(matches!(
        archive.list_dir("def/a.txt"),
        Err(Error::NotADirectory { .. })
    ));
}

#[test]
fn rejects_entries_of_another_archive() {
    let (_t1, first) = open_ok("foreign_1", &sample().build());
    let (_t2, second) = open_ok(
        "foreign_2",
        &TestArchive::new().file("other.txt", b"x").build(),
    );
    let entry = first
        .find("def/a.txt")
        .expect("lookup")
        .expect("exists")
        .entry;
    assert!(matches!(
        second.read(&entry),
        Err(Error::ForeignEntry { .. })
    ));
}

// ------------------------------------------------------------- invalid offsets and sizes

#[test]
fn rejects_data_outside_the_archive() {
    let beyond = sample().build_with(|t| t.payload_mut("def/a.txt")[3] = 0x0FFF_FFFF);
    let (_t, result) = open("data_beyond", &beyond);
    assert!(
        matches!(result, Err(Error::InvalidEntry { .. })),
        "{result:?}"
    );

    let in_header = sample().build_with(|t| t.payload_mut("def/a.txt")[3] = 0);
    let (_t, result) = open("data_in_header", &in_header);
    assert!(
        matches!(result, Err(Error::InvalidEntry { .. })),
        "{result:?}"
    );

    let too_long = sample().build_with(|t| {
        let p = t.payload_mut("def/a.txt");
        p[0] = 0x0FFF_FFFF;
        p[1] = 0x0FFF_FFFF;
    });
    let (_t, result) = open("data_too_long", &too_long);
    assert!(
        matches!(result, Err(Error::InvalidEntry { .. })),
        "{result:?}"
    );
}

#[test]
fn rejects_index_tables_outside_the_archive() {
    let bytes = sample().build();
    let len = bytes.len() as u64;
    for (name, at, value) in [
        ("entry_table_beyond", 28, (len + 16).to_le_bytes().to_vec()),
        ("entry_table_in_header", 28, 8u64.to_le_bytes().to_vec()),
        ("metadata_beyond", 36, u64::MAX.to_le_bytes().to_vec()),
        ("entry_table_too_long", 16, u32::MAX.to_le_bytes().to_vec()),
        ("metadata_empty", 24, 0u32.to_le_bytes().to_vec()),
    ] {
        let (_t, result) = open(name, &patch(bytes.clone(), at, &value));
        assert!(
            matches!(
                result,
                Err(Error::InvalidIndex(_) | Error::LimitExceeded { .. })
            ),
            "{name}: {result:?}"
        );
    }
}

#[test]
fn rejects_tables_that_do_not_match_their_declared_size() {
    let bytes = sample().build();
    // One entry more or less than the entry table holds.
    for (name, count) in [("count_more", 8u32), ("count_less", 6u32)] {
        let (_t, result) = open(name, &patch(bytes.clone(), 12, &count.to_le_bytes()));
        assert!(
            matches!(result, Err(Error::CorruptData { hash: None, .. })),
            "{name}: {result:?}"
        );
    }
    let words = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    let (_t, result) = open(
        "words_more",
        &patch(bytes.clone(), 20, &(words + 1).to_le_bytes()),
    );
    assert!(
        matches!(result, Err(Error::CorruptData { hash: None, .. })),
        "{result:?}"
    );
}

#[test]
fn rejects_a_corrupt_compressed_index() {
    let bytes = sample().build();
    let at = u64::from_le_bytes(bytes[28..36].try_into().expect("8 bytes")) as usize;
    let mut corrupt = bytes.clone();
    corrupt[at + 4] ^= 0x55;
    let (_t, result) = open("corrupt_index", &corrupt);
    assert!(result.is_err(), "{result:?}");
}

#[test]
fn rejects_stored_entries_whose_sizes_disagree() {
    let bytes = sample().build_with(|t| t.payload_mut("def/a.txt")[1] = 4);
    let (_t, archive) = open_ok("stored_size_mismatch", &bytes);
    assert!(matches!(
        archive.read_file("def/a.txt"),
        Err(Error::CorruptData { .. })
    ));
}

#[test]
fn rejects_compressed_entries_with_wrong_declared_size() {
    for (name, delta) in [("zlib_bigger", 1i64), ("zlib_smaller", -1)] {
        let bytes = sample().build_with(|t| {
            let p = t.payload_mut("def/b.bin");
            p[1] = (i64::from(p[1]) + delta) as u32;
        });
        let (_t, archive) = open_ok(name, &bytes);
        assert!(
            matches!(
                archive.read_file("def/b.bin"),
                Err(Error::CorruptData { .. })
            ),
            "{name}"
        );
    }
}

#[test]
fn rejects_impossible_compression_ratios_before_allocating() {
    // A few compressed bytes cannot inflate to 200 MB.
    let bytes = TestArchive::new()
        .zlib_file("x", b"tiny")
        .build_with(|t| t.payload_mut("x")[1] = 200_000_000);
    let (_t, archive) = open_ok("ratio", &bytes);
    assert!(matches!(
        archive.read_file("x"),
        Err(Error::CorruptData { .. })
    ));
}

#[test]
fn rejects_files_above_the_size_limit() {
    let temp = TempArchive::new("size_limit", &sample().build());
    let limits = Limits {
        max_file_size: 1000,
        ..Limits::default()
    };
    let archive = Archive::open_with_limits(&temp.path, limits).expect("opens");
    assert!(matches!(
        archive.read_file("def/b.bin"),
        Err(Error::LimitExceeded {
            what: "file size",
            value: 4096,
            limit: 1000
        })
    ));
    assert_eq!(
        archive.read_file("def/a.txt").expect("small file"),
        b"alpha"
    );
}

#[test]
fn rejects_corrupt_compressed_data() {
    // Remember where the compressed data of "def/b.bin" is placed.
    let (mut offset, mut stored) = (0usize, 0usize);
    let bytes = sample().build_with(|t| {
        let p = t.payload_mut("def/b.bin");
        stored = (p[0] & 0x0FFF_FFFF) as usize;
        offset = p[3] as usize * 16;
    });
    for at in [offset + 3, offset + stored / 2, offset + stored - 1] {
        let mut corrupt = bytes.clone();
        corrupt[at] ^= 0xA5;
        let (_t, archive) = open_ok("corrupt_zlib", &corrupt);
        assert!(
            matches!(
                archive.read_file("def/b.bin"),
                Err(Error::CorruptData { .. })
            ),
            "corrupt byte at {at}"
        );
    }
}

// ------------------------------------------------------------- malformed index entries

/// A named change to the uncompressed index tables.
type TableChange = (&'static str, Box<dyn Fn(&mut common::Tables)>);

#[test]
fn rejects_malformed_index_entries() {
    let cases: Vec<TableChange> = vec![
        (
            "no_records",
            Box::new(|t| t.row_mut("def/a.txt").meta_count = 0),
        ),
        (
            "records_outside",
            Box::new(|t| t.row_mut("def/a.txt").meta_index = 1_000_000),
        ),
        (
            "records_overflow",
            Box::new(|t| {
                let r = t.row_mut("def/a.txt");
                r.meta_index = u32::MAX;
                r.meta_count = u16::MAX;
            }),
        ),
        (
            "payload_outside",
            Box::new(|t| {
                let w = t.header_word_mut("def/a.txt");
                *w = (*w & 0xFF00_0000) | 0x00FF_FFFF;
            }),
        ),
        (
            "directory_flag_on_file",
            Box::new(|t| t.row_mut("def/a.txt").flags |= FLAG_DIRECTORY),
        ),
        (
            "directory_flag_missing",
            Box::new(|t| t.row_mut("def").flags &= !FLAG_DIRECTORY),
        ),
    ];
    for (name, change) in cases {
        let bytes = sample().build_with(|t| change(t));
        let (_t, result) = open(name, &bytes);
        assert!(
            matches!(result, Err(Error::InvalidEntry { .. })),
            "{name}: {result:?}"
        );
    }
}

/// Builds an archive whose "def" directory has the given raw listing data.
fn with_def_listing(listing: &[u8]) -> Vec<u8> {
    let hash = path_hash("def").expect("valid path");
    TestArchive::new()
        .file("def/a.txt", b"alpha")
        .without_listing("def")
        .unlisted_file(hash, listing)
        .build_with(|t| {
            t.row_mut("def").flags |= FLAG_DIRECTORY;
            let w = t.header_word_mut("def");
            *w = (*w & 0x00FF_FFFF) | (u32::from(common::META_DIRECTORY) << 24);
        })
}

#[test]
fn rejects_malformed_listings() {
    // A listing that names "..".
    let bytes = with_def_listing(&common::listing(&["..".to_owned()]));
    let (_t, archive) = open_ok("bad_listing_name", &bytes);
    assert!(matches!(
        archive.list_dir("def"),
        Err(Error::InvalidListing { .. })
    ));

    // A truncated listing breaks the lookup of its children too.
    let good = common::listing(&["a.txt".to_owned()]);
    let bytes = with_def_listing(&good[..good.len() - 1]);
    let (_t, archive) = open_ok("truncated_listing", &bytes);
    assert!(matches!(
        archive.find("def/a.txt"),
        Err(Error::InvalidListing { .. })
    ));
    assert!(matches!(
        archive.list_dir("def"),
        Err(Error::InvalidListing { .. })
    ));

    // The same construction with a valid listing works, so the errors above
    // come from the listing contents.
    let (_t, archive) = open_ok("valid_listing", &with_def_listing(&good));
    assert_eq!(archive.read_file("def/a.txt").expect("listed"), b"alpha");
}

// ------------------------------------------------------------- unsupported data

#[test]
fn rejects_unsupported_compression_methods() {
    for method in [2u32, 3, 15] {
        let bytes = sample().build_with(|t| {
            let p = t.payload_mut("def/b.bin");
            p[0] = (p[0] & 0x0FFF_FFFF) | (method << 28);
        });
        let (_t, archive) = open_ok(&format!("method_{method}"), &bytes);
        assert!(
            matches!(
                archive.read_file("def/b.bin"),
                Err(Error::UnsupportedCompression { method: m, .. }) if u32::from(m) == method
            ),
            "method {method}"
        );
    }
}

#[test]
fn unsupported_metadata_is_kept_but_not_read() {
    let texture = path_hash("tex.tobj").expect("valid path");
    let extra = path_hash("model.pmg").expect("valid path");
    let bytes = sample()
        // Texture object: image, sampler and mip records (GDeflate data).
        .records(
            texture,
            0,
            vec![
                (1, vec![0, 0]),
                (2, vec![0]),
                (132, vec![0x3000_0010, 16, 0, 4]),
            ],
        )
        // Plain file with an extra record of unknown meaning.
        .records(extra, 0, vec![(META_PLAIN, vec![5, 5, 0, 4]), (6, vec![0])])
        .build();
    let (_t, archive) = open_ok("unsupported_metadata", &bytes);
    for hash in [texture, extra] {
        let entry = archive
            .entries()
            .iter()
            .find(|e| e.hash() == hash)
            .expect("kept in the index");
        assert_eq!(entry.kind(), EntryKind::Unsupported);
        assert!(matches!(
            archive.read(entry),
            Err(Error::UnsupportedEntry { .. })
        ));
    }
}

#[test]
fn unknown_flags_and_reserved_words_are_not_read() {
    let bytes = sample().build_with(|t| {
        t.row_mut("def/a.txt").flags |= 0x8;
        t.payload_mut("readme.txt")[2] = 1;
        // Bit 2 has no known meaning but is common in real archives.
        t.row_mut("def/b.bin").flags |= 0x4;
    });
    let (_t, archive) = open_ok("flags", &bytes);
    let a = archive.find("def/a.txt").expect("lookup").expect("exists");
    assert_eq!(a.entry.kind(), EntryKind::Unsupported);
    assert!(matches!(
        archive.read_file("def/a.txt"),
        Err(Error::UnsupportedEntry { .. })
    ));
    assert!(matches!(
        archive.read_file("readme.txt"),
        Err(Error::UnsupportedEntry { .. })
    ));
    assert_eq!(
        archive.read_file("def/b.bin").expect("bit 2 is fine"),
        pattern(4096)
    );
}

// ------------------------------------------------------------- read-only

#[test]
fn never_modifies_the_archive() {
    let bytes = sample().zlib_file("extra.bin", &pattern(9000)).build();
    let temp = TempArchive::new("read_only", &bytes);
    let mut permissions = fs::metadata(&temp.path).expect("metadata").permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&temp.path, permissions.clone()).expect("make read-only");
    let modified = fs::metadata(&temp.path).expect("metadata").modified().ok();

    {
        // Opening must work on a read-only file, so no write access is requested.
        let archive = Archive::open(&temp.path).expect("opens read-only file");
        for path in ["readme.txt", "def/a.txt", "def/b.bin", "extra.bin"] {
            archive.read_file(path).expect("readable");
        }
        archive.list_dir("def").expect("listing");
        let _ = archive.read_file("missing");
    }

    assert_eq!(fs::read(&temp.path).expect("read back"), bytes);
    assert_eq!(
        fs::metadata(&temp.path).expect("metadata").modified().ok(),
        modified
    );

    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(&temp.path, permissions).expect("make writable for cleanup");
}

#[test]
fn file_method_helper_matches_stored_and_zlib() {
    // Guards the test builder itself: both methods round-trip.
    let bytes = TestArchive::new()
        .file_with("s", b"same bytes", Method::Stored)
        .file_with("z", b"same bytes", Method::Zlib)
        .build();
    let (_t, archive) = open_ok("builder_methods", &bytes);
    assert_eq!(
        archive.read_file("s").expect("stored"),
        archive.read_file("z").expect("zlib")
    );
}

/// Gives the entry at `path` extra metadata records of the given types, next
/// to its plain data record (as `.pmg` files in real archives have).
fn add_extra_records(t: &mut common::Tables, path: &str, types: &[u8]) {
    let row = *t.row_mut(path);
    let plain = t.words[row.meta_index as usize];
    let mut headers = vec![plain];
    for &ty in types {
        let payload = t.words.len();
        t.words.extend([0u32; 4]);
        headers.push(u32::from(ty) << 24 | payload as u32);
    }
    let first = t.words.len();
    t.words.extend(headers.iter().copied());
    let row = t.row_mut(path);
    row.meta_index = first as u32;
    row.meta_count = headers.len() as u16;
}

#[test]
fn the_primary_record_of_a_file_with_extra_records_is_readable_on_request() {
    let data = pattern(3000);
    let bytes = sample()
        .zlib_file("model/a.pmg", &data)
        .zlib_file("model/b.pmg", &data)
        .zlib_file("model/c.pmg", &data)
        .build_with(|t| {
            add_extra_records(t, "model/a.pmg", &[6]);
            add_extra_records(t, "model/b.pmg", &[5, 6, 7]);
            // A record type that is not known to belong here.
            add_extra_records(t, "model/c.pmg", &[9]);
        });
    let (_t, archive) = open_ok("primary_record", &bytes);
    for path in ["model/a.pmg", "model/b.pmg"] {
        // The default read still refuses these entries.
        assert!(matches!(
            archive.read_file(path),
            Err(Error::UnsupportedEntry { .. })
        ));
        assert_eq!(
            archive.read_primary_record(path).expect("primary"),
            data,
            "{path}"
        );
    }
    assert!(matches!(
        archive.read_primary_record("model/c.pmg"),
        Err(Error::UnsupportedEntry { .. })
    ));
    // Plain files and missing paths behave as with `read_file`.
    assert_eq!(
        archive.read_primary_record("readme.txt").expect("plain"),
        archive.read_file("readme.txt").expect("plain")
    );
    assert!(matches!(
        archive.read_primary_record("model/none.pmg"),
        Err(Error::NotFound { .. })
    ));
    assert!(matches!(
        archive.read_primary_record("def"),
        Err(Error::NotAFile { .. })
    ));
}

#[test]
fn texture_objects_have_no_primary_record() {
    let texture = path_hash("tex.tobj").expect("valid path");
    let bytes = sample()
        .records(
            texture,
            0,
            vec![
                (1, vec![0, 0]),
                (2, vec![0]),
                (132, vec![0x3000_0010, 16, 0, 4]),
            ],
        )
        .build();
    let (_t, archive) = open_ok("texture_primary", &bytes);
    let entry = archive
        .entries()
        .iter()
        .find(|e| e.hash() == texture)
        .expect("kept in the index");
    assert_eq!(entry.size(), None);
}
