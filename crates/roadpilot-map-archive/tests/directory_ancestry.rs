//! Regression tests for path confirmation and directory ancestry.
//!
//! HashFS v2 has no parent pointers. Every entry is keyed by the CityHash64
//! of its *full* path from the root, and the game resolves files by that
//! hash. Directory listings only name children. The reader therefore checks
//! one thing: that the parent directory (found by the hash of the full
//! parent path) lists the requested name. These tests pin down what that
//! check accepts and rejects. See `docs/formats/hashfs-v2.md`.

mod common;

use common::{FLAG_DIRECTORY, TempArchive, TestArchive};
use roadpilot_map_archive::{Archive, DirEntry, EntryKind, Error, PathCheck, path_hash};

fn open(name: &str, bytes: &[u8]) -> (TempArchive, Archive) {
    let temp = TempArchive::new(name, bytes);
    let archive = Archive::open(&temp.path).expect("synthetic archive opens");
    (temp, archive)
}

fn check(archive: &Archive, path: &str) -> Option<PathCheck> {
    archive
        .find(path)
        .expect("lookup works")
        .map(|found| found.check)
}

#[test]
fn nested_directories_with_valid_listings_are_confirmed_at_every_level() {
    let bytes = TestArchive::new()
        .file("a/b/c/d/file.txt", b"deep")
        .file("a/b/side.txt", b"side")
        .build();
    let (_t, archive) = open("ancestry_nested", &bytes);

    assert_eq!(check(&archive, ""), Some(PathCheck::Root));
    for dir in ["a", "a/b", "a/b/c", "a/b/c/d"] {
        assert_eq!(check(&archive, dir), Some(PathCheck::Listed), "{dir}");
        assert!(archive.list_dir(dir).is_ok(), "{dir}");
    }
    assert_eq!(check(&archive, "a/b/c/d/file.txt"), Some(PathCheck::Listed));
    assert_eq!(
        archive.read_file("a/b/c/d/file.txt").expect("readable"),
        b"deep"
    );
    assert_eq!(
        archive.read_file("/a/b/side.txt").expect("readable"),
        b"side"
    );
    assert_eq!(
        archive.list_dir("a/b").expect("listing"),
        vec![
            DirEntry {
                name: "c".to_owned(),
                is_dir: true
            },
            DirEntry {
                name: "side.txt".to_owned(),
                is_dir: false
            },
        ]
    );
}

#[test]
fn an_entry_missing_from_its_parent_listing_is_not_confirmed() {
    let bytes = common::sample().hide_in_listing("def/a.txt").build();
    let (_t, archive) = open("ancestry_missing_name", &bytes);

    assert_eq!(check(&archive, "def/a.txt"), Some(PathCheck::NotListed));
    assert!(matches!(
        archive.read_file("def/a.txt"),
        Err(Error::UnverifiedPath {
            check: PathCheck::NotListed,
            ..
        })
    ));
    // Its siblings in the same listing are unaffected.
    assert_eq!(archive.read_file("def/b.bin").expect("listed").len(), 4096);
}

#[test]
fn an_orphaned_directory_is_not_confirmed_but_its_listed_children_are() {
    // "def" does not list "world" (an orphaned subtree, as found in real
    // archives). The entry "def/world/road.sii" is still keyed by the hash
    // of its full path, and the game reads it by that hash; its own parent
    // listing names it. So the file is confirmed, the orphaned directory
    // itself is not, and it cannot be listed.
    let bytes = common::sample().hide_in_listing("def/world").build();
    let (_t, archive) = open("ancestry_orphan", &bytes);

    assert_eq!(check(&archive, "def/world"), Some(PathCheck::NotListed));
    assert!(matches!(
        archive.list_dir("def/world"),
        Err(Error::UnverifiedPath {
            check: PathCheck::NotListed,
            ..
        })
    ));
    assert_eq!(
        check(&archive, "def/world/road.sii"),
        Some(PathCheck::Listed)
    );
    assert!(archive.read_file("def/world/road.sii").is_ok());
    // The orphan is not reported as a child of "def".
    assert!(
        archive
            .list_dir("def")
            .expect("listing")
            .iter()
            .all(|c| c.name != "world")
    );
}

#[test]
fn an_archive_without_root_listing_only_confirms_below_listed_directories() {
    // Like the official locale.scs and base_cfg.scs: no root listing.
    let bytes = common::sample().without_listing("").build();
    let (_t, archive) = open("ancestry_no_root", &bytes);

    assert_eq!(check(&archive, ""), None);
    assert_eq!(check(&archive, "def"), Some(PathCheck::NoParentListing));
    assert_eq!(
        check(&archive, "readme.txt"),
        Some(PathCheck::NoParentListing)
    );
    assert!(matches!(
        archive.list_dir("def"),
        Err(Error::UnverifiedPath { .. })
    ));
    assert_eq!(check(&archive, "def/a.txt"), Some(PathCheck::Listed));
    assert_eq!(
        archive.read_file("def/a.txt").expect("listed in def"),
        b"alpha"
    );
}

#[test]
fn a_listed_name_without_an_entry_is_not_found() {
    // The listing names "ghost.txt", but no entry has the hash of "def/ghost.txt".
    let bytes = common::sample()
        .add_listing_name("def", "ghost.txt")
        .build();
    let (_t, archive) = open("ancestry_listed_ghost", &bytes);

    assert_eq!(check(&archive, "def/ghost.txt"), None);
    assert!(matches!(
        archive.read_file("def/ghost.txt"),
        Err(Error::NotFound { .. })
    ));
}

#[test]
fn a_name_listed_under_another_parent_does_not_confirm_an_entry() {
    // "def" lists "moved.txt", but the only entry with that file name is
    // keyed by the hash of "other/moved.txt", which "other" does not list.
    // A listing never maps a name to an entry of another directory.
    let bytes = common::sample()
        .file("other/moved.txt", b"moved")
        .file("other/keep.txt", b"keep")
        .hide_in_listing("other/moved.txt")
        .add_listing_name("def", "moved.txt")
        .build();
    let (_t, archive) = open("ancestry_wrong_parent", &bytes);

    assert_eq!(check(&archive, "def/moved.txt"), None);
    assert_eq!(
        check(&archive, "other/moved.txt"),
        Some(PathCheck::NotListed)
    );
    assert!(matches!(
        archive.read_file("other/moved.txt"),
        Err(Error::UnverifiedPath { .. })
    ));
}

#[test]
fn the_listing_marker_must_match_the_entry_kind() {
    // "def" marks "a.txt" as a subdirectory, but the entry is a file.
    let bytes = common::sample()
        .hide_in_listing("def/a.txt")
        .add_listing_name("def", "/a.txt")
        .build();
    let (_t, archive) = open("ancestry_marker", &bytes);

    let found = archive
        .find("def/a.txt")
        .expect("lookup")
        .expect("hash matches");
    assert_eq!(found.entry.kind(), EntryKind::File);
    assert_eq!(found.check, PathCheck::NotListed);
}

#[test]
fn self_referencing_listings_cannot_create_cycles() {
    // "loop" lists itself as a subdirectory. Names only extend the full path,
    // and every path has its own hash, so "loop/loop" is a different key.
    // Lookups do not walk listings, so nothing can loop.
    let bytes = TestArchive::new()
        .file("loop/x.txt", b"x")
        .add_listing_name("loop", "/loop")
        .build();
    let (_t, archive) = open("ancestry_cycle", &bytes);

    assert_eq!(check(&archive, "loop/loop"), None);
    let deep = "loop/".repeat(100);
    assert_eq!(check(&archive, deep.trim_end_matches('/')), None);
    assert!(
        archive
            .list_dir("loop")
            .expect("listing")
            .contains(&DirEntry {
                name: "loop".to_owned(),
                is_dir: true
            })
    );
    assert!(matches!(
        archive.list_dir("loop/loop"),
        Err(Error::NotFound { .. })
    ));
}

#[test]
fn an_ambiguous_parent_hash_does_not_confirm_children() {
    // A second entry shares the hash of the directory "def".
    let hash = path_hash("def").expect("valid path");
    let bytes = common::sample().unlisted_file(hash, b"collision").build();
    let (_t, archive) = open("ancestry_ambiguous_parent", &bytes);

    assert!(matches!(
        archive.find("def"),
        Err(Error::AmbiguousPath { .. })
    ));
    assert_eq!(
        check(&archive, "def/a.txt"),
        Some(PathCheck::NoParentListing)
    );
    assert!(matches!(
        archive.read_file("def/a.txt"),
        Err(Error::UnverifiedPath {
            check: PathCheck::NoParentListing,
            ..
        })
    ));
    // Deeper levels keep their own, unambiguous parent.
    assert_eq!(
        check(&archive, "def/world/road.sii"),
        Some(PathCheck::Listed)
    );
}

#[test]
fn a_parent_hash_that_belongs_to_a_file_does_not_confirm_children() {
    // The entry with the hash of "def" is a plain file, not a listing, even
    // though its bytes would parse as a listing that names "a.txt".
    let hash = path_hash("def").expect("valid path");
    let fake_listing = common::listing(&["a.txt".to_owned()]);
    let bytes = common::sample()
        .without_listing("def")
        .unlisted_file(hash, &fake_listing)
        .build();
    let (_t, archive) = open("ancestry_parent_is_file", &bytes);

    assert_eq!(
        check(&archive, "def/a.txt"),
        Some(PathCheck::NoParentListing)
    );
    assert!(matches!(
        archive.read_file("def/a.txt"),
        Err(Error::UnverifiedPath { .. })
    ));
}

#[test]
fn an_ambiguous_requested_hash_is_rejected_even_when_listed() {
    // "def/a.txt" is listed, but two entries carry its hash.
    let hash = path_hash("def/a.txt").expect("valid path");
    let bytes = common::sample()
        .records(
            hash,
            FLAG_DIRECTORY,
            vec![(common::META_DIRECTORY, vec![0, 0, 0, 4])],
        )
        .build();
    let (_t, archive) = open("ancestry_ambiguous_entry", &bytes);

    assert!(matches!(
        archive.find("def/a.txt"),
        Err(Error::AmbiguousPath { .. })
    ));
    assert!(matches!(
        archive.read_file("def/a.txt"),
        Err(Error::AmbiguousPath { .. })
    ));
}
