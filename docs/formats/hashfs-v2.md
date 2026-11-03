# HashFS v2 archive reader

Status: **implemented** in `crates/roadpilot-map-archive` (Phase 1, Step 2).

All official ETS2 and ATS archives are HashFS v2 (see [SCS map data inventory](../research/scs-map-data-inventory.md)). This crate reads selected entries from them. It is not an extractor: it never writes files and has no "extract all" function.

## Public API

| Item | Purpose |
|---|---|
| `Archive::open(path)` | Open an archive read-only and validate its header and complete index. |
| `Archive::open_with_limits(path, Limits)` | The same, with custom size limits. |
| `Archive::entries()` | All index entries: path hash, kind (`File`, `Directory`, `Unsupported`) and declared size. |
| `Archive::find(path)` | Look up a path. Returns the entry and a `PathCheck`, or `None` if no entry has the path hash. |
| `Archive::read_file(path)` | Read one file into memory. Only paths confirmed by their parent listing. |
| `Archive::read(&Entry)` | Read one file entry, for callers that accept an unconfirmed path. |
| `Archive::list_dir(path)` | Names of a directory's children, in listing order. Needed to enumerate map sectors. |
| `path_hash(path)` | Normalize a path and return its CityHash64 v1.0.3 hash. |
| `Error`, `Limits`, `Entry`, `EntryKind`, `FoundEntry`, `PathCheck`, `DirEntry` | Supporting types. |

The crate is not yet used by the `roadpilot-map` facade. It will be connected when installation discovery needs it.

## Supported format

All values are little-endian. The layout was established by the SCS-Extractor research (191 real archives) and is not repeated beyond what the reader uses.

| Structure | Supported |
|---|---|
| Header (53 bytes) | Magic `SCS#`, version 2, hash method `CITY`, salt 0, platform 0. Entry count, metadata word count, and the offset and stored size of both index tables. The field at offset 44 is ignored. |
| Entry table | zlib stream of 16-byte rows: path hash, first metadata word, metadata word count, flags. Flag bit 0 = directory. Bit 2 is accepted and ignored (common in real archives, no effect on data). |
| Metadata table | zlib stream of `u32` words. Header word `type << 24 \| payload_index`. Only a **single** record of type 128 (file) or 129 (directory) is read. Its payload: `stored_size \| method << 28`, size, reserved word (must be 0), `offset / 16`. |
| Directory listing | `u32` count, one `u8` length per name, then the names. Subdirectories start with `/`. |
| Compression | Method 0 (stored) and method 1 (zlib, RFC 1950). |

## Path lookup

- Paths are normalized: one leading and one trailing `/` are removed, and the root is the empty path. Paths are case-sensitive. Empty components, `.`, `..`, backslashes, NUL and paths over 1024 bytes are rejected (`Error::InvalidPath`).
- The hash is CityHash64 **v1.0.3** of the exact UTF-8 bytes. Version 1.1, which most libraries implement, gives different results for paths of 1 to 64 bytes.
- HashFS stores only hashes, so a hash match alone is not proof of the path. `find` also reads the parent directory listing and reports a `PathCheck`:
  - `Listed`: the listing names the entry, with the right file or directory marker.
  - `NotListed`: the listing exists but does not name it (collision or incomplete listing).
  - `NoParentListing`: there is no parent listing (some real archives have no root listing).
  - `Root`: the root directory itself.
- `read_file` and `list_dir` accept only `Listed` and `Root`. Use `find` and `read` to read an unconfirmed entry on purpose.
- If several index entries share the requested hash, the result is `Error::AmbiguousPath`.
- The most recently used directory listing is cached, so many lookups in one directory decode its listing once.

### Why only the parent listing is checked

HashFS v2 has no parent pointers. Every entry is keyed by the CityHash64 of its **full** path from the root, and the game resolves files by that hash. Directory listings only name children; they are not needed to reach an entry. This was reviewed for the question whether the whole directory chain up to the root should be confirmed:

- The parent is found by the hash of its full path, so it is already tied to the root in the same way as the entry itself. A wrong confirmation would need two independent 64-bit hash collisions: one for the entry and one for a directory whose listing happens to contain the same name.
- Reachability from the root is not a format rule. Official archives without a root listing (`locale.scs`, `base_cfg.scs`) or with an empty one (`core.scs`) are read by the game. A read-only check of a local ETS2 and ATS installation found 1,038 files that their parent lists but that no listing chain connects to the root, all in `locale.scs` and `core.scs`. Requiring the full chain would reject them. All map-relevant files are connected to the root.
- Listings cannot redirect a lookup: a name only extends the requested path, and the extended path has its own hash. A name listed under one directory never confirms an entry of another, and self-referencing listings cannot form cycles. `find` reads exactly one listing and walks nothing.
- Ambiguity is never accepted: several entries with the requested hash give `Error::AmbiguousPath`. A parent hash that is shared by several entries or belongs to a file gives `PathCheck::NoParentListing`.

An orphaned directory (one its own parent does not list) is therefore `NotListed` and cannot be listed with `list_dir`, while files that its listing names are `Listed`. The tests in `crates/roadpilot-map-archive/tests/directory_ancestry.rs` pin this behavior.

## Validation

Archive contents are treated as untrusted input:

- Every offset and size is checked with overflow-checked arithmetic against the file length before it is read. Data and tables must not overlap the header. The two tables must not overlap each other.
- Allocations are bounded before they happen: by `Limits` (default 4 Mi entries, 256 MiB per index table, 256 MiB per entry), and by the maximum DEFLATE ratio (1032:1) for every compressed size. A declared listing count must fit in the listing data.
- Opening fails if any entry has metadata outside the metadata table, has no records, has data outside the file, or has a directory flag that contradicts its metadata.
- zlib data must have a valid header and Adler-32 checksum, and must produce exactly the declared size from exactly the stored bytes. Corrupt, truncated, too long or too short data is an error (`Error::CorruptData`), never a partial result.
- Stored data must have stored size = size.
- The archive file is opened read-only. Only the header, the two index tables, the requested entries and the listings needed for path checks are read.
- No `unsafe` code, no `unwrap`, no panics on malformed input.

## Not supported

| Feature | Behavior |
|---|---|
| HashFS v1, ZIP-based `.scs` | `Error::NotHashFs` or `Error::UnsupportedVersion`. Only used by mods. |
| Non-zero salt, non-zero platform, other hash methods | `Error::UnsupportedSalt`, `UnsupportedPlatform`, `UnsupportedHashMethod`. |
| Texture objects (metadata types 1, 2, 132; GDeflate, method 3) | Kept as `EntryKind::Unsupported`. Reading gives `Error::UnsupportedEntry`. Not needed by RoadPilot. |
| Files with extra metadata records (types 5, 6, 7 on `.pma`, `.pmg`, `.rfx`) | `EntryKind::Unsupported`. Model data, not needed. |
| Unknown entry flags, non-zero reserved word | Entry cannot be read (`Error::UnsupportedEntry`). |
| Compression methods other than 0 and 1 | `Error::UnsupportedCompression`. |
| Streaming large entries, extraction to disk | Not provided. Entries are read completely into memory. |

## Reuse from SCS-Extractor

SCS-Extractor (`C:\Projekte\SCS-Extractor`, read-only reference) provided the verified format knowledge: header and table layout, metadata records, listing format, flag meanings, and the CityHash64 revision. The code in this crate was written new for this smaller scope:

- **CityHash64 v1.0.3:** ported from the published Google CityHash algorithm (MIT), following the same revision. It is tested against vectors produced by SCS-Extractor's validated implementation on synthetic inputs, covering every length branch.
- **zlib:** `miniz_oxide` (MIT OR Zlib OR Apache-2.0) instead of `flate2`. The core decoder writes into a buffer of exactly the declared size.
- **Errors:** a stricter policy than SCS-Extractor. Structural index problems fail `open`, and paths are confirmed through listings.

## Verification

- 77 tests (27 unit, 50 integration) and 2 documentation tests on synthetic archives built in code (`crates/roadpilot-map-archive/tests/common`). No fixture files and no game data.
- A one-off, read-only check against a local ETS2 and ATS installation (not committed, not part of CI) opened all 191 official archives (1,111,851 entries). It read all 180,856 map-relevant files (everything in `def.scs`, `base_map.scs` and the map DLCs except models, textures and sounds, plus all `.ppd` files) without an error. The 379,771 unsupported entries are exactly the texture objects and model files with extra records.

## Known limitations

- Path confirmation checks the parent listing only, by design (see [Why only the parent listing is checked](#why-only-the-parent-listing-is-checked)). It is a defense against hash collisions and incomplete listings, not against deliberately crafted archives, whose author controls every byte anyway.
- Entries named in no listing can be found, but `read_file` refuses them. Real official archives contain about 20 such entries, none of them map-related.
- Each entry is read completely into memory, up to `Limits::max_file_size`.
- Override order between archives (DLCs) is not handled here. It belongs to installation discovery (open question T4).
