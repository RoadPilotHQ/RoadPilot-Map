# SCS sector format (`.base`, `.data`)

Status: **implemented** in `crates/roadpilot-map-formats` (Phase 1, Step 3). Format version **907**, the only version in current ETS2 and ATS.

Sector files live in `map/<map>/sec<±XXXX><±YYYY>.<ext>` in `base_map.scs` and the map DLC archives (see [SCS map data inventory](../research/scs-map-data-inventory.md)). This document covers `.base` and `.data`. `.aux`, `.snd` and `.desc` are not parsed.

All values are little-endian. Records are packed; fields are not aligned.

## Evidence

Every statement below is marked with one of:

- **Confirmed:** checked on every sector of a local ETS2 and ATS installation. That is 2,026 sectors (1,101 ETS2, 925 ATS), 1,113,691 items and 1,722,052 nodes, read read-only. Item lengths count as confirmed because a deterministic walk of every `.base` file ends exactly at the end of the file.
- **Cross-checked:** a field's meaning was confirmed by a reference to other game data, such as a node pointing back to the item or a token naming a definition unit.
- **Reference:** taken from the community tool [ts-map](https://github.com/dariowouters/ts-map) (MIT license) and consistent with the data, but the meaning is not checked independently.
- **Unknown:** not established.

ts-map was used only as a source of hypotheses; no code was copied. Its license was checked first (open question L3). TruckLib (GPL-2.0) was **not** consulted.

## Header (`.base` and `.data`)

| Offset | Type | Field | Evidence |
|---|---|---|---|
| 0 | `u32` | Version, 907 | Confirmed (all files of both games) |
| 4 | `u32` | 7668833 in every file | Confirmed value, meaning unknown |
| 8 | `u32` | 0 in every file | Confirmed value, meaning unknown |
| 12 | `u32` | 3 in every file | Confirmed value, meaning unknown |

The parser checks the version and exposes the other three words unchanged.

## `.base` file

```
header (16)
u32 item_count
item[item_count]
u32 node_count
node[node_count]                (56 bytes each)
u32 trailing_count
u64 trailing_item_uid[trailing_count]
<end of file>
```

All of this is confirmed.

### Common item header (57 bytes)

| Offset | Type | Field | Evidence |
|---|---|---|---|
| 0 | `u32` | Item type | Confirmed |
| 4 | `u64` | Item UID | Confirmed. Non-zero and unique within each game. Nodes refer to items by it. |
| 12 | `f32[5]` | Bounds minimum | Confirmed `min ≤ max` on all items. Axes 0–2 contain the item's node positions (X, Y, Z; 99.9 % within 1 unit). Axes 3–4 are unknown (presumably diagonal k-DOP axes). |
| 32 | `f32[5]` | Bounds maximum | As above |
| 52 | `u8[5]` | Flags | Unknown. ts-map reads visibility bits from them; not checked. |
| 57 | … | Type-specific payload | No length field. The length follows from the type (see below). |

Because there is no length field, an unknown item type stops the parse (`ParseError::UnknownItemType`); the rest of the file cannot be located.

### Item types and payload length rules

Names are ts-map labels. Counts are `u32` unless noted. "Fixed" means the same length in every item.

| Type | Name | ETS2 / ATS items | Payload length rule | Rule source |
|---|---|---|---|---|
| 1 | terrain | 7,879 / 13,803 | 238 bytes, count × 20, 2 × quad info, 32 bytes | Reference (ts-map 903+), confirmed |
| 2 | building | 922 / 916 | 44 bytes, count × 4 | Own analysis, confirmed |
| 3 | road | 252,127 / 224,870 | Fixed 212 bytes | Own analysis and ts-map 895+, confirmed |
| 4 | prefab | 73,211 / 84,855 | 16 bytes, parts × 8, nodes × 8, connected × 8, 10 bytes, nodes × 12, 8 bytes | Reference (ts-map 855+), confirmed |
| 5 | model | 2,024 / 7,184 | 24 bytes, count × 8, 36 bytes | Own analysis, confirmed |
| 6 | company | 1,844 / 2,186 | 32 bytes, count × 12 | Own analysis, confirmed |
| 7 | service | 4,479 / 5,592 | 16 bytes, count × 8 | Own analysis, confirmed |
| 8 | cut plane | 16,657 / 12,629 | count × 8 (node UIDs) | Own analysis, confirmed |
| 12 | city | 1,140 / 2,089 | Fixed 24 | Confirmed |
| 18 | map overlay | 3,199 / 2,240 | Fixed 16 | Confirmed |
| 19 | ferry | 87 / 10 | Fixed 36 | Confirmed |
| 22 | garage | 468 / 248 | 28 bytes, count × 8 | Own analysis, confirmed |
| 34 | trigger | 3,486 / 8,171 | tags × 8, nodes × 8, actions (see below), 4 bytes if exactly one node | Reference (ts-map 875+), confirmed |
| 35 | fuel pump | 234 / 124 | Fixed 20 | Confirmed |
| 36 | sign | 144,925 / 91,475 | See below | Reference (ts-map 906+), confirmed |
| 37 | bus stop | 289 / 0 | Fixed 24 | Confirmed |
| 38 | traffic rule | 5,444 / 6,795 | count × 8, count × 8, 12 bytes | Own analysis, confirmed |
| 39 | bezier patch | 123 / 69 | 241 bytes, count × 20, quad info | Reference (ts-map 884+), confirmed |
| 41 | trajectory | 34,691 / 36,632 | nodes × 8, 8 bytes, rules × 28, checkpoints × 16, tags × 8 | Own analysis and ts-map 895+, confirmed |
| 42 | map area | 8,868 / 27,870 | count × 8, 4 bytes | Own analysis, confirmed |
| 44 | curve | 3,138 / 11,078 | 36 bytes, `u32` mask, then per set mask bit: 0x4C bytes, count × 4, 0x14 bytes | Reference (ts-map 907), confirmed |
| 46 | cutscene | 718 / 462 | tags × 8, 8 bytes, actions (see below) | ts-map 884+ with a 907 change, confirmed |
| 48 | visibility area | 5,297 / 3,143 | 16 bytes, count × 8 | Own analysis, confirmed |

The only difference between the games is that ATS has no type 37 items. The layouts are identical.

Nested structures:

- **String:** `u64` length, then the bytes. ts-map reads it as a `u32` length plus 4 bytes; the upper half is 0 in all game data.
- **Quad info:** `u16` count × 10, `u16` count × 4, 4 bytes, `u32` count × 4, `u32` count × 16, `u32` count × 16.
- **Trigger action:** 8 bytes, `i32` override count. If it is negative, the action ends here (10,625 actions in the game data). Otherwise: overrides × 4, `u32` parameter count with that many strings, target tags × 8, 8 bytes.
- **Sign:** 32 bytes, `u8` board count × 24, `u32` template length and 4 bytes. If the length is not 0: the template bytes; `u32` board override count, each with 8 bytes, a `u8` flag byte, +2 bytes if bit 0 is set and +8 if bit 1 is set; `u32` sign override count, each with 12 bytes and a `u32` attribute count. Each attribute has an `i16` kind and 4 bytes, then a string (kind 5), 8 bytes (kind 6), 1 byte (kind 1) or 4 bytes (any other kind).
- **Cutscene action:** count × 4, `u32` string count with that many strings, target tags × 8, 8 bytes. ts-map 884 has 4 more bytes after the string count; version 907 does not.

### Decoded item fields

| Item | Field | Payload offset | Evidence |
|---|---|---|---|
| Road | Road look (token) | 4 | Cross-checked: 100 % name a `road_look` unit (`road.<name>`) in the game definitions |
| Road | Start node UID | 192 | Cross-checked: 100 % of these nodes have the road as `forward_item` |
| Road | End node UID | 200 | Cross-checked: 100 % have the road as `backward_item` |
| Prefab | Model (token) | 0 | Cross-checked: 100 % name a `prefab_model` unit (`prefab.<name>`) |
| Prefab | Node UIDs | after the parts list | Cross-checked: 100 % are nodes |
| Prefab | Connected item UIDs | after the nodes | Cross-checked: all are items (signs, models, companies, services, garages, fuel pumps, bus stops) |

All other payload bytes are exposed as raw `Item::payload`; ts-map gives meanings for some of them that are not checked yet.

A **token** packs a name of up to 12 characters (`0-9`, `a-z`, `_`) into a `u64` as a base-38 number, first character in the lowest digit, with 0 meaning "end". Cross-checked through the road look and prefab model references above.

### Node (56 bytes)

| Offset | Type | Field | Evidence |
|---|---|---|---|
| 0 | `u64` | Node UID | Confirmed, non-zero |
| 8 | `i32[3]` | Position X, Y, Z, fixed point (÷ 256) | Cross-checked against item bounds. Units (meters?) are open question T3. |
| 20 | `f32[4]` | Rotation | Confirmed unit quaternion (norm 1 ± 0.001) on all nodes. Component order unknown. |
| 36 | `u64` | Backward item UID or 0 | Cross-checked (road end nodes) |
| 44 | `u64` | Forward item UID or 0 | Cross-checked (road start nodes) |
| 52 | `u32` | Flags | Unknown |

Nodes of items that cross a sector border can be stored in a neighbouring sector.

### Trailing item UIDs

A `u32` count and that many `u64` UIDs. All 3,884 UIDs in the game data are items (types 1, 2, 3, 4, 5, 36, 39, 44). ts-map describes them as children of visibility areas; that is not checked.

## `.data` file

```
header (16)
entry[...]       one per road and prefab item of the .base file, same order
u64 0xFFFF_FFFF_FFFF_FFFF   end marker
<end of file>
```

- Confirmed: every road (476,997) and prefab (158,066) has exactly one entry. Entries are in `.base` order and start with the item UID. All files end with the end marker. A sector without roads and prefabs has a 24-byte `.data` file.
- Entries have **no length field**, and their contents vary (over 7,500 different lengths for roads, over 4,400 for prefabs). No length field was found in the first 64 bytes of any entry.
- The parser therefore ends each entry where the UID of the next expected item appears. In the game data, no road or prefab UID occurs inside another entry. The format does not rule this out, though, so the boundary rule counts as plausible, not structural.
- The entry contents are **unknown** and are exposed as raw bytes.

## Parser

| API | Purpose |
|---|---|
| `parse_base(&[u8])` / `parse_base_with_limits` | Parse a `.base` file into `BaseSector`: header, `Item`s (type, UID, bounds, flags, payload, decoded `ItemData`), `Node`s, trailing UIDs. |
| `parse_data(&[u8], &BaseSector)` / `parse_data_with_limits` | Split the matching `.data` file into `DataEntry`s. |
| `ItemType`, `ItemData::{Road, Prefab, Other}`, `Token`, `Bounds`, `Node`, `SectorHeader` | Records. Payloads borrow from the input; nothing is copied. |
| `ParseError` | Typed errors with the byte offset. |
| `Limits` | File size (256 MiB), item count (1,000,000), node count (4,000,000). |

Safety measures:

- Every read is bounds-checked, and every size computation uses checked arithmetic.
- Every count is checked against the remaining input before anything is skipped or reserved, so memory stays proportional to the input.
- There is no recursion, and every loop consumes input, so even a count of `u32::MAX` ends quickly with an error.
- The version must be 907.
- Bounds must be finite with min ≤ max, and node rotations must be unit quaternions.
- UIDs that must refer to something must not be 0: item, node, road start/end, prefab nodes and the trailing list.
- After the last structure the file must end; trailing bytes are an error.
- There is no I/O, no `unsafe` code and no external dependencies.

Tests:

- Synthetic files built in code (`crates/roadpilot-map-formats/tests`).
- An opt-in test against a local installation (`tests/real_installation.rs`, `ROADPILOT_MAP_GAME_DIR`), which parses every sector `.base` and `.data` file and checks the road-node links.

## Open questions

| ID | Question |
|---|---|
| S1 | Meaning of header words 4–12, the 5 item flag bytes, node flags and bounds axes 3–4. |
| S2 | Meaning of the undecoded payload fields of all item types (ts-map gives hypotheses for several). In particular, the 212-byte road payload holds more than the look and the nodes. |
| S3 | Layout of `.data` entries. They are needed if lane-relevant road or prefab data is stored there (not known yet). |
| S4 | Component order of the node rotation quaternion and the coordinate units (T3). |
| S5 | Item types that do not occur in `.base` today (for example type 13 or 40) and how `.aux` and `.snd` are structured. |
| S6 | Behavior for other format versions. Only 907 was observed, and any other version is rejected. |
