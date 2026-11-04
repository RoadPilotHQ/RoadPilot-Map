# Map package (schema 1)

Status: **implemented** (Phase 1, Step 8) in `crates/roadpilot-map-package` (format, writer, validating reader, spatial index), `crates/roadpilot-map-geometry` (road curve and evidence levels) and `crates/roadpilot-map` (`write_package`, `check_package`). CLI commands: `build-package` and `check-package`. Decision record: [ADR 0002](../adr/0002-map-package-format-and-versioning.md).

This document is the specification. A program in any language can read a package without any RoadPilot-Map code. The package is the data boundary between RoadPilot-Map and RoadPilot (map display, map matching, route planning).

Read [Evidence levels](#evidence-levels) and [Limitations](#limitations) before using any geometric value. **Nothing in schema 1 is suitable as a basis for active steering.**

## Overview

| | |
|---|---|
| Package | A folder with `manifest.json`, `geometry.jsonl` and optionally `navigation.jsonl` |
| Manifest format | `roadpilot-map-package`, one JSON object in one line |
| Geometry format | `roadpilot-map-geometry`, JSON Lines |
| Navigation format | `roadpilot-map-navigation-graph`, JSON Lines, **unchanged**, see [Navigation graph export](../navigation/navigation-export.md) |
| Schema version | `1.0` (`major` 1, `minor` 0) |
| Encoding | UTF-8, `\n` line ends |
| One package per | game (`ets2` or `ats`) |
| Determinism | The same models give the same bytes. No file contains a time stamp. |
| Distribution | **Local use only.** A package is derived from the user's game data and is not meant to be shared (open question L2) |

### One contract, not two

The navigation graph export (schema 1, Step 7) stays the only description of lanes as graph nodes and of connection statuses between lanes and prefab slots. The package does **not** repeat it. It adds what the export lacks, geometry, and keys it by the same UIDs, so the two files refer to each other:

| Data | Where |
|---|---|
| Map nodes (position, rotation, links) | `geometry.jsonl` |
| Roads, their lanes, traffic side, curve | `geometry.jsonl` |
| Junctions (prefab instances), their arms and stored movements | `geometry.jsonl` |
| Road-to-road connections and their lane pairing status | `geometry.jsonl` |
| Lane-level graph, edges with status and origin, traffic rule references, issues | `navigation.jsonl` |

`roadpilot-map check-package` verifies the link: every `road` and `map_node` that the navigation file names must exist in the geometry file, and the positions must be equal. If the navigation format changes in an incompatible way, its major version changes independently; the manifest records the version of each file.

### Folder layout

```text
<package>/
  manifest.json      required
  geometry.jsonl     required
  navigation.jsonl   optional (build-package --without-navigation omits it)
```

A reader must ignore files it does not know. Only the files listed in the manifest are part of the package.

## Manifest (`manifest.json`)

| Field | Type | Meaning |
|---|---|---|
| `format` | string | `roadpilot-map-package` |
| `schema_version` | `{major, minor}` | Version of the format. A reader rejects a `major` it does not support and accepts any `minor` of its major |
| `content_revision` | integer | Revision of the generation logic. Increases when the same game data would give different content (for example a new curve model). Consumers use it to discard cached packages |
| `content_id` | hex string | Hash over the names, sizes and checksums of the files. Equal content gives an equal id; the reader checks it |
| `generator` | `{name, version}` | The program that wrote the package |
| `generated_at` | null | Always `null` (determinism) |
| `game` | string | `ets2` or `ats` |
| `source` | `{map, game_version, archives}` | Each may be `null`. `game_version` is `null` today: RoadPilot-Map does not read it. `archives` lists the archives that were read, in reading order |
| `coordinate_system` | object | `x` `east`, `y` `up`, `z` `south`; `position_scale` 256 (a stored position integer divided by 256 gives world units); `world_unit` `unconfirmed` |
| `counts` | object | `nodes`, `roads`, `lanes`, `junctions`, `connections` |
| `geometry_models` | object | The evidence level of each kind of geometric value, see below |
| `spatial_index` | `{kind, cell_size}` | The grid a reader builds, `uniform_grid`, cell size in world units |
| `distribution` | string | `local_use_only` |
| `limitations` | array of strings | Codes of known limitations, see [Limitations](#limitations) |
| `files` | array | One entry per file: `name`, `role` (`geometry` or `navigation`), `format`, `schema_version`, `bytes`, `checksum` |

`file.checksum` is an FNV-1a 64-bit hash (lower-case hexadecimal). It finds accidental damage and incomplete copies. It is **not** cryptographic and says nothing about who made the package. File names must be plain names (letters, digits, `.`, `_`, `-`, not starting with a dot), so a manifest cannot point outside the package folder.

`geometry_models`:

```json
{"node_position":"confirmed",
 "curve":{"model":"hermite_stored_length","quality":"estimated","length_tolerance":0.001},
 "lane_position":"unavailable"}
```

### Versions

| Version | Meaning | Changes when |
|---|---|---|
| `schema_version.major` | Incompatible change of the format | A field is removed or renamed, a meaning changes, a required field is added, a new value is added to a closed value set, or the order of sections changes. Readers reject other majors |
| `schema_version.minor` | Compatible addition | An optional field, a new record type or a new limitation code is added. Readers ignore what they do not know |
| `content_revision` | Meaning of the generated content | The generation logic changes (curve model, id rules, how a value is derived) |
| `content_id` | Identity of this exact package | Any byte of any file changes |
| `generator.version` | The RoadPilot-Map version | Every release; **not** a compatibility signal |

A consumer decides compatibility with `schema_version.major` alone. `content_revision` and `content_id` are for caches.

## Geometry file (`geometry.jsonl`)

Every line is a JSON object with a field `t` that names its type. Order: header, nodes, roads, junctions, connections, footer. Records of an unknown type and unknown fields are ignored by readers of the same major version. A reader that finds a known record type out of order rejects the file.

All UIDs are lower-case hexadecimal strings without prefix (see [Identifiers](#identifiers)).

### Header

`{"t":"header","format":"roadpilot-map-geometry","schema_version":{"major":1,"minor":0},"counts":{"nodes":N,"roads":N,"junctions":N,"connections":N}}`

### Node (`t = "node"`)

| Field | Type | Meaning |
|---|---|---|
| `id` | hex | Node UID. Strictly ascending over the nodes |
| `position` | `[x, y, z]` | Integers, divide by `position_scale`. **Confirmed** |
| `rotation` | `[w, x, y, z]` | Unit quaternion as stored. The travel direction at the node is the local −Z axis rotated by it (near-confirmed: the direction lies within 60° of the line to the other node for 99.4 % of ETS2 roads, see [SCS road geometry](scs-road-geometry.md#directions)) |
| `backward`, `forward` | link | `null`, `{"road": hex}`, `{"item": hex}` (an existing item that is not a road, for example a prefab) or `{"unresolved": hex}` (a UID that matches no loaded item). `forward: {"road": r}` means road `r` starts here; `backward: {"road": r}` means it ends here. The reader checks this |

### Road (`t = "road"`)

| Field | Type | Meaning |
|---|---|---|
| `id` | hex | Road UID. Strictly ascending |
| `start`, `end` | hex | Node UIDs; must exist and differ |
| `look` | string | Road look name (`road.<token>`) |
| `stored_length` | number | The length stored in the road record, in world units (unit unconfirmed). Positive |
| `traffic_side` | string | `right`, `left` or `unknown`; for `unknown` the field `traffic_side_reason` names the cause (`no_country`, `country_mismatch`, `unknown_country`, `look_conflict`, `no_lane_data`) |
| `lanes` | object | See below |
| `curve` | object | See below |

`lanes` is `{"status":"resolved","items":[…]}` or `{"status":"unresolved","reason":code}` (`missing_road_look`, `missing_variant`, `missing_lane_type`, `variant_changes_lane_count`, `no_lane_data`). In `items`, left lanes come before right lanes and the `index` is consecutive from 0 on each side. A lane item:

| Field | Meaning |
|---|---|
| `side` | `left` or `right` of the stored road direction (start to end node) |
| `index` | Position in the road-look definition order; 0 is the lane nearest the road centre (ETS2 documented; ATS inferred from the shared format) |
| `type` | Lane type name |
| `type_certain` | `false` if the type depends on a road look variant that the data does not let us choose (LM1) |
| `direction` | `forward` (start to end), `backward`, or `null` if the traffic side is unknown. The rule behind it is measured, not confirmed (NV1) |
| `offset` | The stored offset pair `[a, b]` or `null`. Unit and meaning unknown (LM3) |
| `width` | Always `null`: no lane width is stored |
| `geometry` | Evidence level of the lane's position data. `unavailable` in schema 1.0 |

The stable id of a lane is the text `<road uid>:<l|r><index>`, for example `25249798d2000001:r0` (`roadpilot_map_package::lane_id`).

`curve` is one of:

```json
{"quality":"estimated","tangent_scale":L,"estimated_length":E,"length_deviation":D,
 "length_consistent":true,"bounds":[minx,miny,minz,maxx,maxy,maxz]}
{"quality":"unavailable","reason":"invalid_stored_length"}
```

An `estimated` curve is the cubic Hermite curve through the positions of the start and end node, with the node travel directions as tangent directions and `tangent_scale` (the stored road length) as tangent length (model `hermite_stored_length`):

```text
P(t) = h00(t)·P0 + h10(t)·L·d0 + h01(t)·P1 + h11(t)·L·d1,   t in [0, 1]
h00 = 2t³ − 3t² + 1   h10 = t³ − 2t² + t   h01 = −2t³ + 3t²   h11 = t³ − t²
```

`P0`, `P1` are the node positions in world units, `d0`, `d1` the unit travel directions, `L` the stored length. `estimated_length` is the length of that curve (a 256-segment polyline). `length_deviation` is `|estimated_length − stored_length| / stored_length`; `length_consistent` is `length_deviation ≤ length_tolerance` (the reader checks the claim). `bounds` is a box that contains the **estimated** curve (the box of its Bézier control points); it does not bound the game's real curve. `unavailable` has a reason: `non_finite_input`, `invalid_stored_length`, `invalid_direction`, `no_geometry`. A curve is never `confirmed` in schema 1.0 and the reader rejects that.

### Junction (`t = "junction"`)

A junction is a prefab instance on the map. There is no separate junction concept in the game data.

| Field | Meaning |
|---|---|
| `id` | Prefab instance UID. Strictly ascending |
| `model` | Prefab model name |
| `resolution`, `resolution_reason` | `resolved`, or `unresolved` with a code (`missing_model`, `missing_descriptor`, `invalid_descriptor`, `node_count_mismatch`, `origin_out_of_range`, `node_geometry_mismatch`). Unresolved junctions have no movements |
| `nodes` | The instance's node UIDs in stored order |
| `arms` | Per node: `index`, `node` and `link`: `{"kind":"road","road":hex,"ends_here":bool}`, `prefab`, `item`, `unresolved` or `none`. The road named by an arm must end (`ends_here`) or start at the arm's node |
| `movements` | The stored internal lane movements: `from_arm`, `from_slot`, `to_arm`, `to_slot`, `status` (`confirmed`: the prefab descriptor stores the curve chain) |

Which road lane belongs to which prefab slot is **not** here; those pairings are candidate edges of the navigation file.

### Connection (`t = "connection"`)

Joins of two roads at a shared node (`kind` `road_road`), ascending by `(from, to)`.

| Field | Meaning |
|---|---|
| `from`, `via`, `to` | Road, node, road. The `from` road ends at `via` and the `to` road starts there; the reader checks it |
| `status` | `candidate`: lanes can be paired by index, listed in `pairs` (`["r0","r0"]` pairs right lane 0 of `from` with right lane 0 of `to`). `unknown`: they cannot, with a `reason` (`lanes_unresolved`, `traffic_side_unknown`, `traffic_side_differs`, `lane_count_differs_forward`, `lane_count_differs_backward`, `no_lanes`) |

No road-to-road connection is `confirmed` in schema 1.0: the data stores no lane-to-lane connection between roads (LM6). `forbidden` and `confirmed` are valid values of the format.

### Footer

`{"t":"footer","complete":true,"nodes":N,"roads":N,"junctions":N,"connections":N}`. **A file without the footer is incomplete and must be rejected.** The counts must equal the header and the records read.

## Evidence levels

Every geometric value has one of three levels:

| Level | Meaning | Examples (schema 1.0) |
|---|---|---|
| `confirmed` | Established by the game data itself | Node positions; the stored road length; the quaternion as stored |
| `estimated` | Derived with a model that is only empirically supported | The curve between two nodes |
| `unavailable` | Not known; no value is given and none may be assumed | Lane positions and widths |

Rules for consumers:

1. A value that is `estimated` or `unavailable` is not a basis for active steering or any intervention. `roadpilot_map_geometry::Quality::is_suitable_for_active_steering` is true only for `confirmed`. Estimated geometry is for display, for candidate selection in map matching and for warnings that a person can ignore.
2. `unavailable` must never be read as zero, as the node curve or as a default width.
3. A result computed from several values is no better than the weakest input.
4. The `geometry_models` object of the manifest states the level per kind of value; a later minor version may add finer information, and a `curve` object always states its own level.

### Accuracy of the curve estimate

Measured on the local installations (read-only, opt-in test `crates/roadpilot-map/tests/real_package.rs`) by comparing the length of the estimated curve with the stored road length. This tests the length only, not the shape.

| Turn between node directions | ETS2 roads | within 0.1 % | ATS roads | within 0.1 % |
|---|---|---|---|---|
| 0–10° | 132,205 | 99.52 % | 143,867 | 99.67 % |
| 10–30° | 62,880 | 99.95 % | 41,155 | 100.00 % |
| 30–60° | 36,043 | 99.96 % | 23,802 | 99.99 % |
| 60–90° | 15,310 | 99.99 % | 10,972 | 99.98 % |
| 90–120° | 5,189 | 96.70 % | 4,731 | 98.48 % |
| above 120° | 500 | 0.00 % | 341 | 0.29 % |
| All roads | 252,127 | 99.46 % | 224,868 | 99.60 % |

The worst deviation is 15.1 % (ETS2) and 11.1 % (ATS). That the length agrees does not prove that the shape agrees. Roads with `length_consistent: false` (about 0.5 %) are those where the model is known to fail and must be treated with extra care. The accuracy of the **position** of the curve against the game's real road is **not measured**: the data contains no reference curve. Do not read the table as a position accuracy.

## Identifiers

| Identifier | Form | Stable |
|---|---|---|
| Node, road, junction UIDs | lower-case hexadecimal string, at most 16 digits | They come from the game data. Unique within one game's data in the inspected versions. **Not guaranteed to stay the same between game versions or map revisions** (limitation `identifiers_are_not_stable_across_game_versions`) |
| Lane id | `<road uid>:<l|r><index>` | As stable as the road UID and the lane index order of its road look |
| `content_id` | hex | Identifies one exact package |

UIDs are 64-bit values and are written as strings, because numbers above 2^53 lose precision in JavaScript. Treat them as opaque strings, or parse them as `u64`/`BigInt`. A consumer must not store a UID as a permanent identifier across game updates; it should store `content_id` with its own data and rebuild its references when the package changes.

## Spatial queries

The package has no index file. A reader builds a uniform grid over the horizontal (X/Z) boxes of the roads, with the cell size of `spatial_index.cell_size` (256 world units). The box of a road is the `bounds` of its estimated curve, or the box of its two nodes if it has no curve. `roadpilot_map_package::MapPackage` offers:

| Call | Result |
|---|---|
| `roads_near(x, z, radius)` | Roads whose box is within `radius` of the position, nearest first (ties by UID order). The distance is the distance to the box, a lower bound of the distance to the estimated curve |
| `roads_in_bounds(min, max)` | Roads whose box intersects the area, in id order |

Results are deterministic. A hit is a **candidate** for map matching: because the curve is an estimate and the box is the box of the estimate, a consumer must add a margin and must expect that the real road lies elsewhere. `roadpilot_map_geometry::RoadCurve::project` finds the closest point on the estimated curve. Building the index for a whole map is a one-time cost at load (see [Size and performance](#size-and-performance)).

## Validation

`MapPackage::open` and `roadpilot-map check-package` check, and stop at the first problem:

- the manifest: format name, supported major version, field types, known game, safe file names, a geometry file entry, and `content_id` equal to the id of the file entries;
- every listed file: exists, has the recorded size and checksum;
- the geometry file: header, supported major version, line length limit (4 MiB), section order, strictly ascending ids, finite numbers, unit quaternions, existing referenced nodes and roads, node links that agree with the roads, lane lists with consecutive indices, curve claims that agree with the manifest tolerance, curve boxes with minimum below maximum and an extent below 10,000 units, connection statuses that fit their pairs and reasons, junction arms that fit their nodes and roads, counts in header, footer and manifest, and the footer;
- with `check-package`, additionally the navigation file with its reference reader, and that its roads and map nodes exist in the geometry file with equal positions.

A reader grows its memory record by record; a wrong count in a header cannot cause a large allocation.

## Creating a package

```text
roadpilot-map build-package --game <ets2|ats> --game-dir <path> --out <folder> [--force] [--without-navigation]
roadpilot-map check-package <folder>
```

| Rule | Behaviour |
|---|---|
| Source | The user's own installation, read-only. Nothing is downloaded and no game data is shipped |
| Output | An explicit folder. No default location |
| Existing output | Not replaced. `--force` replaces a folder that holds **only** package files; any other folder is refused untouched |
| Output inside the game folder | Refused |
| Complete packages only | Written to `<folder>.partial` and renamed when complete. After an error nothing remains that was not there before. A leftover `.partial` folder is never overwritten |
| Exit codes | 0 success; 1 output problem or write error; 2 usage; 3 not an installation of the game; 4 game data or package invalid |

## Size and performance

Measured on one machine with the release build on the current local installations (`real_package` test: loading the models, curve statistics, writing and checking the full package with navigation file, and a second geometry build).

| | ETS2 | ATS |
|---|---|---|
| Nodes / roads / lanes | 804,304 / 252,127 / 495,080 | 903,077 / 224,868 / 470,073 |
| Junctions / road connections | 73,211 / 151,258 | 84,874 / 130,602 |
| `geometry.jsonl` | 450,588,576 bytes | 462,154,502 bytes |
| `navigation.jsonl` | 421,239,333 bytes | 459,546,387 bytes |
| Whole test run | 23 s | 36 s |

The text compresses well (the navigation export shrinks by a factor of about 10 with gzip). Memory use and load time of a reader were **not measured** and the target figures of open question T7 are not agreed yet. A reader that needs a small footprint can stream the geometry file; `MapPackage` loads it completely.

## Reading a package (any language)

1. Read `manifest.json`. Check `format`; if `schema_version.major` is not one you support, stop. Treat `null` as unknown, never as a default.
2. For each file you use, compare size and checksum with the manifest (FNV-1a 64).
3. Read the geometry file line by line. Check the header, keep the order, keep every quality and status as it is, ignore unknown records and fields.
4. Check the footer. Reject files without it.
5. Do not merge or upgrade levels: `estimated` stays `estimated`, `candidate` stays `candidate`.

`roadpilot_map_package::read_geometry` and `MapPackage::open` are a reference reader in Rust.

## Compatibility rules

| Change | Allowed in | A reader of the same major version |
|---|---|---|
| New optional field in an existing record or the manifest | minor | ignores it |
| New record type (new `t`) | minor | ignores the line |
| New file in the manifest with a new `role` | minor | ignores it |
| New limitation code | minor | may show it |
| New value of `quality`, `status`, `kind`, `traffic_side` or `direction`; removing or renaming a field; a different meaning; a new required field; a different section order; a new schema of an existing file role | **major** | rejects the file |

## Limitations

The codes in the manifest's `limitations` array:

| Code | Meaning |
|---|---|
| `lane_positions_are_unavailable` | No lane has a position. No lane width is stored (LM3) |
| `lane_widths_are_not_stored` | `width` is `null` for every lane |
| `road_curve_is_estimated` | The curve between two nodes is a model (G1); about 0.5 % of the roads do not even reproduce the stored length |
| `world_unit_is_unconfirmed` | The physical unit of a world unit is not confirmed (T3). Lengths and distances are in world units, not meters |
| `road_connections_are_candidates` | No road-to-road lane connection is confirmed (LM6) |
| `prefab_to_prefab_lane_pairing_is_unknown` | Where two prefabs are joined directly, the pairing is unknown (PJ4) |
| `roads_without_known_traffic_side_have_no_lane_directions` | Lane directions are `null` where the traffic side is unknown (R1, R2) |
| `identifiers_are_not_stable_across_game_versions` | UIDs may change with game updates |

With the navigation file, the limitation codes of the [navigation export](../navigation/navigation-export.md#limitations) are added, except `curve_geometry_is_not_exported`, which the package replaces by the `curve` object.

Further points:

- Speed limits, signs, traffic light assignment, signal states and traffic rule meaning are not in the package.
- Roads without a known traffic side are in the geometry file but not in the navigation graph.
- Mod maps, other games and DLC order effects are not handled (T4, T9).
- The package must be regenerated after a game update.
- Whether a package may be shared is a legal question (L2). Until it is answered the package is `local_use_only`.
