# Navigation graph export (schema 1)

Status: **implemented** (Phase 1, Step 7) in `crates/roadpilot-map` (`export`, `installation`) and the CLI commands `export-navigation` and `check-navigation-export`.

This document is the specification of the file format. A program in any language can read it without using any RoadPilot-Map code. The format is the data boundary between RoadPilot-Map and other RoadPilot components such as RoadPilot-Visualization.

It exports the [navigation graph](navigation-graph.md) of Step 6 as it is: the same nodes, edges, statuses and origins. It does not interpret, repair or extend the graph. Read the [connectivity audit](connectivity-audit.md) and [Limitations](#limitations) before using the data.

## Overview

| | |
|---|---|
| Format name | `roadpilot-map-navigation-graph` |
| Schema version | `1.0` (`major` 1, `minor` 0) |
| Encoding | UTF-8 text, one JSON object per line (JSON Lines), `\n` line ends |
| File extension | `.jsonl` (recommended) |
| One file per | game (`ets2` or `ats`) |
| Order | header, nodes, edges, issues, footer |
| Determinism | The same models give the same bytes. The file has no time stamp. |

### Why this format

The alternatives were weighed for this step:

| Option | For | Against |
|---|---|---|
| **JSON Lines, one file** (chosen) | Streamed on write and read (memory is independent of the file size); trivial in Rust and TypeScript with the standard library; readable with any text tool; new fields and record types can be added without breaking readers; no dependency | About 10 times larger than a binary format; numbers are parsed from text |
| One JSON document | Simplest to read for small data | Needs the whole document in memory (several hundred MiB) and cannot be streamed |
| Compact binary | Smallest and fastest | Needs a layout specification and a reader per language; not inspectable; schema evolution needs more machinery; no existing binary format convention in the project |
| Several files (metadata + data) | Parallel reading | A set of files can get out of step; more rules for a first version |

The size is acceptable because the text compresses very well: gzip makes the ETS2 export 39.7 MiB from 401.7 MiB and the ATS export 41.3 MiB from 438.3 MiB. A consumer can store or transfer the file compressed. Compression is not part of the format.

No decision of this step depends on a particular consumer. If a binary form is needed later, it can be added as a second schema file beside this one; it should be justified by a measured need.

## Records

Every line is a JSON object with a field `t` that names its type. A reader must ignore fields it does not know, and (for the same major version) records with an unknown `t`.

### Header (`t = "header"`), line 1

| Field | Type | Required | Meaning |
|---|---|---|---|
| `format` | string | yes | Always `roadpilot-map-navigation-graph` |
| `schema_version` | object | yes | `major` and `minor`, both integers |
| `generator` | object | yes | `name` (`roadpilot-map`) and `version` of the program that wrote the file |
| `generated_at` | null | yes | Always `null`: the file contains no time stamp, so that identical input gives identical bytes |
| `game` | string | yes | `ets2` or `ats` |
| `source` | object | yes | See below |
| `coordinate_system` | object | yes | See below |
| `counts` | object | yes | `nodes`, `edges`, `issues`: the number of records of each type |
| `limitations` | array of strings | yes | Codes of the known limitations of this data, see [Limitations](#limitations) |

`source` fields (all required, each may be `null`):

| Field | Meaning |
|---|---|
| `map` | The map folder the sectors came from: `europe` (ETS2) or `usa` (ATS) |
| `game_version` | The game version. **`null`**: RoadPilot-Map does not read it |
| `archives` | File names of the archives that were read, in the order they were read. They show which DLC archives were present. `null` if not given |

Nothing is invented: game version, DLC list and time stamp are `null` when they are not known. A consumer must not assume that `null` means a default.

`coordinate_system`:

| Field | Value | Meaning |
|---|---|---|
| `x`, `y`, `z` | `east`, `up`, `south` | The axes of the game's world (confirmed) |
| `position_scale` | `256` | A position is an integer; divide by this to get the world unit |
| `world_unit` | `unconfirmed` | The physical unit of a world unit is not confirmed (open question G2) |

### Node (`t = "node"`)

Nodes come directly after the header. Node ids are the integers `0, 1, 2, …` in line order.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | integer | yes | The exported node id; equals the node's position in the node list |
| `kind` | string | yes | `lane_entry`, `lane_exit`, `prefab_input`, `prefab_output` |
| `road` | hex string | lane kinds | UID of the road |
| `side` | string | lane kinds | `left` or `right`, relative to the road's stored direction (start to end node) |
| `index` | integer | lane kinds | Index of the lane on that side, in the definition order of the road look. Whether 0 is the inner or outer lane is unknown (LM2) |
| `prefab` | hex string | prefab kinds | UID of the prefab instance |
| `arm` | integer | prefab kinds | Index of the node in the prefab instance |
| `slot` | integer | prefab kinds | Lane slot at that node |
| `map_node` | hex string | yes | UID of the map node where the lane starts (`lane_entry`) or ends (`lane_exit`), or of the prefab node of the slot |
| `position` | `[x, y, z]` or null | yes | The position of `map_node`, as integers (see `position_scale`); `null` if not available |

Meaning of the kinds:

| Kind | Meaning |
|---|---|
| `lane_entry` | Where traffic enters a road lane, in the lane's driving direction |
| `lane_exit` | Where traffic leaves a road lane |
| `prefab_input` | A lane enters a prefab (junction piece) at this slot |
| `prefab_output` | A lane leaves a prefab at this slot |

**Positions are not lane positions.** `position` is the position of the *map node* that the lane or slot is attached to, the same for all lanes and slots at it. Lane widths, lane offsets and curves are not known with sufficient confidence and are not exported. Do not draw a lane at this position.

### Edge (`t = "edge"`)

Edges come after the nodes. Edge ids are `0, 1, 2, …` in line order. The order is by start node.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | integer | yes | The exported edge id |
| `from` | integer | yes | Node id of the start; must exist |
| `to` | integer | yes | Node id of the end; must exist |
| `status` | string | yes | `confirmed`, `candidate`, `unknown`, `forbidden` |
| `category` | string | yes | What kind of connection it is, see below |
| `source` | hex string | yes | UID of the record the edge comes from (the meaning depends on the category) |
| `rules` | array | no | Traffic rule references stored with the edge. Each is `{"name": string, "resolved": boolean}`. Present only if there are any. They are references only; their meaning is not interpreted |

Categories, their node kinds and the meaning of `source`:

| Category | From → to | `source` | Status in this version |
|---|---|---|---|
| `road_lane` | `lane_entry` → `lane_exit` of the same lane | Road UID | `candidate` |
| `road_connection` | `lane_exit` → `lane_entry` of the next road | Shared map node UID | `candidate` |
| `road_to_prefab_slot` | `lane_exit` → `prefab_input` | Prefab node UID | `candidate` |
| `prefab_internal` | `prefab_input` → `prefab_output` of one prefab | Prefab UID | `confirmed` |
| `prefab_slot_to_road` | `prefab_output` → `lane_entry` | Prefab node UID | `candidate` |
| `prefab_to_prefab` | `prefab_output` → `prefab_input` of the other prefab | Shared map node UID | `unknown` |
| `other` | Any | Defined by the writer | any |

The column "Status in this version" describes what the current RoadPilot-Map writes. The format itself allows any status for any category, and a reader must not rely on the column.

### Issue (`t = "issue"`)

Issues come after the edges. They list what is **missing** from the graph and why, so that gaps are not mistaken for non-existence.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `kind` | string | yes | See below |
| `source` | hex string | yes | UID the issue concerns (road, prefab or map node) |
| `reason` | string | no | A more specific cause |
| `arm`, `direction`, `lanes`, `slots` | | no | Details for some kinds |

| Kind | Meaning | `source` |
|---|---|---|
| `road_lanes_not_navigable` | The road's lanes are unresolved or its traffic side is unknown: **the road is not in the graph** | Road |
| `road_transition_unpaired` | Two roads meet but their lanes cannot be paired (`reason`: `lanes_unresolved`, `traffic_side_unknown`, `traffic_side_differs`, `lane_count_differs`) | Map node |
| `prefab_unresolved` | The prefab could not be tied to its definition: **it is not in the graph** | Prefab |
| `prefab_arm_unpaired` | A road at a prefab node cannot be paired with the slots (`reason`: `road_lanes_unknown`, `lane_count_mismatch` with `direction`, `lanes`, `slots`, …) | Prefab |
| `unresolved_node_link` | A node refers to a UID that matches no loaded item | Map node |
| `node_links_to_other_item` | A node refers to an item that is neither a road nor a prefab. These do not affect connectivity | Map node |

### Footer (`t = "footer"`), last line

`{"t":"footer","complete":true,"nodes":N,"edges":M,"issues":K}`

The footer proves that the file was written completely. **A file without a footer is incomplete and must be rejected**, even if every line is valid. The counts must equal the counts of the header and the number of records read.

## Identifiers

| Identifier | Type in the file | Scope |
|---|---|---|
| Node `id`, edge `id` | integer | Only inside one export file. Dense and consecutive. Not stable between exports of different data |
| Road, prefab and map node UIDs (`road`, `prefab`, `map_node`, `source`) | **lower-case hexadecimal string**, without prefix | The UID of the game's map item or node |

UIDs are 64-bit values. They are written as strings, because numbers above 2^53 lose precision in JavaScript and JSON readers that use double precision numbers. A consumer must treat them as opaque strings, or parse them with a 64-bit type (`BigInt` in JavaScript, `u64` in Rust).

Stability scope: the exported node and edge ids depend on the order of the graph and are only valid in the same file. UIDs come from the game's data. They are unique within one game's data in the inspected versions, but **it is not guaranteed that they stay the same between game versions or map revisions**. A consumer must not store them as permanent identifiers across game updates.

## Meaning of the information

### Topology, confidence, routing, routes

These four things are different. The file contains only the first two:

| Level | Question | In the file | Who decides |
|---|---|---|---|
| **1. Topology** | Does this edge exist in the exported graph? | `node` and `edge` records | The graph builder |
| **2. Connection confidence** | How well does the game data support this edge? | `status` | The graph builder, from the evidence in the data |
| **3. Routing eligibility** | May a route search use this edge? | **Not in the file** | The program that searches routes. RoadPilot-Map's current policy: `ConfirmedOnly` uses `confirmed` edges; `AllowCandidates` uses `confirmed` and `candidate` edges; `unknown` and `forbidden` edges are never used. Another consumer, or a later version, may decide differently (for example once traffic rules are interpreted) |
| **4. Route validity** | Can the whole route be driven in the game? | **Not in the file** | Nobody has established this. No status, and no combination of statuses, proves it. No route, cost or ETA is exported |

An edge in the file is not necessarily routable, and a routable edge is not necessarily drivable. A consumer must not treat all edges as drivable, and must not treat a path of exported edges as a validated route.

### Status

| Status | Meaning |
|---|---|
| `confirmed` | The connection itself is stored explicitly in the game data. In this version this is only a movement inside one prefab (`prefab_internal`): the prefab descriptor stores a chain of curves from an input slot to an output slot. It is a statement about the evidence for this one edge. See [What `confirmed` means](#what-confirmed-means) |
| `candidate` | Plausible, but a part is not confirmed: lane directions come from a rule that is measured, not confirmed; lanes are paired by index and the lane order is unknown |
| `unknown` | The data does not say whether the connection exists. The edges join **all** slot combinations of the two prefabs and are not a statement that each pair is connected |
| `forbidden` | Excluded by explicit data. The current version never writes it, because no such data is known. The schema can represent it |

A reader must preserve all four values and must not map `candidate` or `unknown` to `confirmed`.

### What `confirmed` means

`confirmed` answers one question only: *Is this single connection stored in the game data?* In this version it is written for `prefab_internal` edges and for no other category. The meaning of the other statuses is the same kind of statement about the evidence: `candidate` says the connection is plausible but part of it is an assumption, `unknown` says the data does not say whether it exists, `forbidden` says the data excludes it.

**What `confirmed` guarantees**

- The two slots belong to the same prefab, and the prefab descriptor stores a chain of curves that leads from the input slot to the output slot.
- The direction of the edge (from the input slot to the output slot) is the stored direction.

**What `confirmed` does not guarantee**

| Misreading | Why it is wrong |
|---|---|
| "A route made of confirmed edges is connected." | A confirmed edge always joins an input slot to an output slot of the same prefab, and no confirmed edge starts at an output slot, so a path of confirmed edges has at most one edge. No confirmed edge touches a road lane, and none joins two prefabs. Every step onto or off a road is a `candidate` edge |
| "A route made of confirmed edges can be driven." | Traffic rules are stored as references only, signals and their phases are not exported, and vehicle and legal restrictions are not evaluated. Nothing in the data establishes that a route is drivable |
| "The lane direction is verified." | Only the direction inside the prefab is stored. The driving direction of the road lanes before and after it comes from a rule that is measured, not confirmed (`lane_traversal_direction_is_candidate`) |
| "Both ends are attached to the right map nodes." | The attachment of the prefab slots to map nodes uses the node mapping of the prefab instance. It is checked against the node distances; unexplained exceptions remain (open questions PJ2 and NV8) |
| "The edge is usable in every routing mode." | Eligibility is not in the file. RoadPilot-Map's current policy uses `confirmed` edges in both modes, but that is a decision of the route search, not a property of the edge |
| "The graph is connected." | The network consists of many separate pieces (`graph_is_not_one_connected_network`) |

**Why `candidate` and `unknown` stay distinct.** They are different answers. A `candidate` edge is plausible, and the open part is which lanes it joins (the lane order is unknown, LM2). An `unknown` edge says that the data does not say whether the connection exists at all; the prefab-to-prefab edges join *all* slot combinations of the two prefabs, and which of these pairs exist is not known. A consumer that merges them, or treats either as `confirmed`, makes a claim that the data does not support. A consumer may still decide to use `candidate` edges (RoadPilot-Map does so in `AllowCandidates` mode, and marks the result as provisional), but that is an explicit decision at level 3, not a change of the status.

### Disconnected components

The export keeps the graph as it is: it does not merge regions, add edges, or remove isolated parts. The network is **not one connected piece** (the limitation code `graph_is_not_one_connected_network`). In the current data, weak components over all edges number 11,674 (ETS2) and 14,547 (ATS); using only confirmed and candidate edges they number 28,530 and 34,979.

The file contains no component records. A consumer that needs components computes them from the edges and must say which statuses it used. Components were not included in schema 1.0 because the result depends on that choice. A later minor version may add an optional, clearly described record.

### Missing and unknown information

- A field that is not available is `null`, or absent if the field is optional. A default is never written.
- Things that are not in the graph at all (roads without a usable traffic side, unresolved prefabs) are listed as issues, not hidden.

## Example

A small invented example (not game data). Two roads, `a` and `14`, meet a prefab `100`; line breaks inside a line are added for reading only.

```json
{"t":"header","format":"roadpilot-map-navigation-graph","schema_version":{"major":1,"minor":0},"generator":{"name":"roadpilot-map","version":"0.1.0"},"generated_at":null,"game":"ets2","source":{"map":"europe","game_version":null,"archives":["base_map.scs"]},"coordinate_system":{"x":"east","y":"up","z":"south","position_scale":256,"world_unit":"unconfirmed"},"counts":{"nodes":4,"edges":3,"issues":1},"limitations":["lane_order_is_unknown"]}
{"t":"node","id":0,"kind":"lane_entry","road":"a","side":"right","index":0,"map_node":"b","position":[2560,512,0]}
{"t":"node","id":1,"kind":"lane_exit","road":"a","side":"right","index":0,"map_node":"c","position":[2560,512,-5120]}
{"t":"node","id":2,"kind":"prefab_input","prefab":"100","arm":0,"slot":0,"map_node":"c","position":[2560,512,-5120]}
{"t":"node","id":3,"kind":"prefab_output","prefab":"100","arm":1,"slot":0,"map_node":"d","position":[2560,512,-10240]}
{"t":"edge","id":0,"from":0,"to":1,"status":"candidate","category":"road_lane","source":"a"}
{"t":"edge","id":1,"from":1,"to":2,"status":"candidate","category":"road_to_prefab_slot","source":"c"}
{"t":"edge","id":2,"from":2,"to":3,"status":"confirmed","category":"prefab_internal","source":"100","rules":[{"name":"traffic_rule.example","resolved":true}]}
{"t":"issue","kind":"road_lanes_not_navigable","source":"14"}
{"t":"footer","complete":true,"nodes":4,"edges":3,"issues":1}
```

(The real header lists all limitation codes; the example shortens it.)

## Reading the format

A consumer in any language:

1. Read the first line. Check `format`. Read `schema_version.major`; **if it is not a major version you support, stop** and report an unsupported version. Accept any `minor`.
2. Read `game`, `source` and `coordinate_system`. Treat `null` as unknown.
3. Read the node lines. Check `id` equals the line number among nodes. Store them in an array.
4. Read the edge lines. Look up `from` and `to` in the array; a missing node is an error.
5. Keep the status of every edge. Do not merge, filter or upgrade statuses unless your own program decides to, and then label the result.
6. Read the issue lines if you need to know what is missing.
7. Read the footer. If it is missing, or `complete` is not `true`, or its counts differ from the header or from what you read, reject the file.

Sketch in TypeScript (line by line; use a streaming line reader for the large files; UIDs stay strings):

```ts
const header = JSON.parse(await nextLine());
if (header.format !== "roadpilot-map-navigation-graph") throw new Error("not an export");
if (header.schema_version.major !== 1) throw new Error("unsupported schema version");
const nodes: Node[] = [];
const edges: Edge[] = [];
let footer = false;
for await (const line of lines) {
  const r = JSON.parse(line);
  if (r.t === "node") nodes.push(r);
  else if (r.t === "edge") {
    if (!nodes[r.from] || !nodes[r.to]) throw new Error("dangling edge");
    edges.push(r);
  } else if (r.t === "footer") footer = r.complete === true;
}
if (!footer || nodes.length !== header.counts.nodes) throw new Error("incomplete export");
// Drawing: position[i] / header.coordinate_system.position_scale gives world units.

// `status` is the confidence in an edge. It is NOT routing eligibility and NOT proof that
// a path can be driven. Eligibility is your program's decision. This is RoadPilot-Map's
// policy as an example; keep the result labelled with the policy that produced it.
type Mode = "confirmed_only" | "allow_candidates";
function eligible(edge: Edge, mode: Mode): boolean {
  if (edge.status === "confirmed") return true;
  if (edge.status === "candidate") return mode === "allow_candidates";
  return false; // "unknown" and "forbidden" are never used; never map them to "confirmed"
}
// A path of eligible edges is a topological path, not a validated route. If it contains
// "candidate" edges it is provisional; show that to the user.
```

In Rust, `roadpilot_map::export::read_navigation_export` and `check_navigation_export` are a reference reader that applies all these checks; they use no other crate and can serve as an example. `roadpilot-map check-navigation-export <file>` runs them.

## Compatibility rules

| Change | Allowed in | A reader of the same major version |
|---|---|---|
| New optional field in an existing record | minor | ignores it |
| New record type (new `t`) | minor | ignores the line |
| New header field | minor | ignores it |
| New value of `kind`, `category` or `status`; a different meaning of a field; removing or renaming a field; changing the order of the sections; a new required field | **major** | rejects the file (unsupported version) |
| New `limitations` codes | minor | may show them |

Writers increase `minor` for a compatible change and `major` for an incompatible one. A reader must not guess at a major version it does not know.

## Writing the file

```text
roadpilot-map export-navigation --game <ets2|ats> --game-dir <path> --out <file> [--force]
roadpilot-map check-navigation-export <file>
```

| Rule | Behaviour |
|---|---|
| Game | Explicit with `--game`. The installation folder is explicit with `--game-dir` |
| Output | Explicit with `--out`. No default location |
| Existing output | Not replaced. `--force` replaces it. This is checked before the slow loading starts |
| Output inside the game folder | Refused |
| Output folder | Must exist |
| Complete files only | The export is written to `<file>.partial` and renamed when complete. After an error no `<file>` exists that was not there before, and the partial file is removed. A `.partial` file left over from an earlier run is not overwritten |
| Game files | Only read. Archives that are not in a supported format are skipped and counted |
| Exit codes | 0 success; 1 output problem or write error; 2 usage; 3 folder is not an installation of the game; 4 game data or export invalid |

## Size and performance

Measured on one machine with the release build, on current local installations. The whole run includes loading the game data (about 4 seconds); writing the file takes about one second.

| | ETS2 | ATS |
|---|---|---|
| Nodes | 1,275,416 | 1,331,699 |
| Edges (confirmed / candidate / unknown) | 1,374,901 (319,384 / 1,008,339 / 47,178) | 1,456,680 (375,973 / 1,014,758 / 65,949) |
| Issues | 509,972 | 607,155 |
| File size | 421,239,333 bytes (401.7 MiB) | 459,546,387 bytes (438.3 MiB) |
| gzip size | 39.7 MiB | 41.3 MiB |
| Loading the models | 3.9 s | 3.9 s |
| Writing the export | 0.8 s | 0.9 s |
| Checking the file with the reference reader | 4.8 s | 5.2 s |
| Peak memory of the whole `export-navigation` run | about 886 MiB | about 881 MiB |

Peak memory is the operating system's peak working set of the process, sampled every 100 ms while it ran, so it may be slightly low. It includes loading the game data. The export itself adds one 1 MiB write buffer and one line buffer; it does not copy the graph. A reader that streams the file needs memory only for the nodes it keeps.

## Limitations

These limitations are in the data, not in the file format. The codes in the header's `limitations` array point to them. They come from the [connectivity audit](connectivity-audit.md).

| Code | Meaning |
|---|---|
| `roads_without_known_traffic_side_are_not_exported` | Roads whose traffic side is unknown for any reason are not in the graph: 36,418 ETS2 roads (14.4 %) and 25,853 ATS roads (11.5 %). Of these, 35,948 and 25,280 roads have nodes that name no country; the others have nodes that name different countries, a look that contradicts the country, or an undefined country. Each is listed as a `road_lanes_not_navigable` issue, so the issue count is the authoritative number. Roads with zero lanes (10,093 ETS2, 8 ATS) have no lanes to export and are not listed as issues. The roads cut the network into pieces. How the game assigns their traffic side is not understood (NV7). Definitions: [road counts](connectivity-audit.md#road-counts-and-their-definitions) |
| `lane_traversal_direction_is_candidate` | The driving direction of a lane comes from the traffic side of the road. The rule agrees with prefab data for 99.3–99.5 % of the arms where it can be tested, but some contradictions are unexplained, so `road_lane` edges are `candidate` (NV1) |
| `lane_order_is_unknown` | The lane index order across a road is not confirmed (LM2). All pairings of road lanes with lanes or prefab slots are by index and therefore `candidate` |
| `prefab_to_prefab_lane_pairing_is_unknown` | Where two prefabs are joined directly, the lane pairing is unknown (PJ4). `prefab_to_prefab` edges join all slot combinations and are `unknown` |
| `positions_are_map_node_positions_not_lane_positions` | `position` is the position of a map node; lane positions are not exported |
| `curve_geometry_is_not_exported` | The curve between two nodes is not confirmed (G1) and no road lengths are exported; the world unit is unconfirmed (G2) |
| `graph_is_not_one_connected_network` | The network consists of many separate pieces; see the audit |

Further points:

- A `confirmed` edge is a stored connection inside one prefab. It is not a connected route, not a permission to drive and not a verified direction; see [What `confirmed` means](#what-confirmed-means).
- Traffic rules are references only; signals and their phases are not exported (PJ1, PJ3, PJ5).
- Unresolved prefab instances (860 in ETS2, 1,030 in ATS) are not in the graph (PJ2).
- Identifiers are not stable across game versions.
- The file is large. Compress it for storage and transfer.
