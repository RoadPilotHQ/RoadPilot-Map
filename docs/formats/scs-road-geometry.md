# SCS road geometry and node connections

Status: **implemented** in `crates/roadpilot-map-model` (types) and `crates/roadpilot-map` (`build_road_network`), Phase 1, Step 4A. Lanes are not part of this step.

Builds on the [SCS sector format](scs-sector-format.md). Evidence levels are the same as there:

- **Confirmed:** holds for all inspected game data.
- **Cross-checked:** confirmed through a reference to other data.
- **Near:** holds for almost all data within a stated tolerance, but not exactly.
- **Unknown:** not established.

Data basis: every sector of a local ETS2 (1,101 sectors, 252,127 roads, 804,304 unique nodes) and ATS installation (925 sectors, 224,870 roads, 902,347 unique nodes), read read-only. The analysis programs are throwaway tools outside the repository. The opt-in tests `crates/roadpilot-map-formats/tests/real_installation.rs` and `crates/roadpilot-map/tests/real_installation.rs` repeat the structural checks.

## Road record

A road is a type 3 item. Its 212-byte payload holds:

| Payload offset | Type | Field | Evidence |
|---|---|---|---|
| 4 | `u64` token | Road look (`road.<name>` in `def/world/road_look*.sii`) | Cross-checked (100 %) |
| 192 | `u64` | Start node UID | Cross-checked: the node's `forward_item` is the road (100 %) |
| 200 | `u64` | End node UID | Cross-checked: the node's `backward_item` is the road (100 %) |
| 208 | `f32` | Length | Cross-checked, see [Length](#length) |
| other | | Not decoded | Unknown |

The road UID is the item UID of the common item header. The road record holds no coordinates; all positions come from its two nodes. Road looks are kept as an uninterpreted reference (they describe lanes, which are Step 4B).

## Node record and references

| Field | Evidence |
|---|---|
| UID | Confirmed unique: identical copies of a node occur in neighbouring sectors (7,315 in ETS2, 8,086 in ATS), always byte-identical. The model merges them and rejects copies that differ. |
| Position: 3 × `i32`, fixed point ÷ 256 | Confirmed (see coordinate system). |
| Rotation: 4 × `f32` | Confirmed unit quaternion, order **(w, x, y, z)** (see directions). |
| Backward item, forward item | Each slot holds at most one item UID or 0. |

Topology rules, all confirmed on both games:

- A node has one backward and one forward slot. No node is the start of more than one road or the end of more than one road.
- If a node's forward item is a road, that road starts at the node; if its backward item is a road, that road ends there (no exception).
- No road starts and ends at the same node, and no two roads share both nodes.
- Road-to-road joins are symmetric: the end node of road A has road B as its forward item exactly when the start node of B has A as its backward item. There are 151,258 such joins in ETS2 and 130,609 in ATS.
- Branches and merges never happen between roads directly; a road end leads to a prefab (junction) instead. Road ends lead to a prefab 80,607 times (ETS2) and 80,623 times (ATS), or to nothing (20,262 / 13,638).
- Unresolved references exist. 29 (ETS2) and 8 (ATS) node slots at road ends name item UIDs that occur in no `.base` file. Over all nodes there are 1,419 and 2,688 such slots, many of them probably items stored in `.aux` files, which are not loaded yet. The model keeps them as `NodeLink::Unresolved` and does not treat them as roads.

The direction from start node to end node is the direction in which the record is stored. It does not say which way traffic may use the road; that comes from the lanes (Step 4B).

## Coordinate system

| Property | Finding | Evidence |
|---|---|---|
| Axes | X and Z horizontal, **Y up** | Confirmed: Y spans about −280 to +670, X and Z span about 160,000–210,000 units |
| Orientation | **+X east, +Z south** (right-handed with Y up) | Cross-checked: country positions in `def/country.sii` lie further east for eastern countries and further north (smaller Z) for northern ones, in both games |
| Scale | Stored integers ÷ 256 = world units | Confirmed: the result lies inside the items' bounds (99.9 % within 1 unit) |
| Sector grid | Sector `sec<X><Z>` covers `[X·4000, (X+1)·4000) × [Z·4000, (Z+1)·4000)` | Confirmed for 98 % of nodes; the rest belong to items that cross the sector border |
| Origin | World origin; sector `+0000+0000` starts at (0, 0) | Follows from the sector grid |
| Unit | Same unit as the road lengths and the distances in the game definitions; presumably meters | **Unknown** (open question T3) |

The model keeps all positions in this system and in the stored fixed-point form. No transformation is applied.

## Directions

The forward direction of a node is its local **−Z** axis rotated by the quaternion **(w, x, y, z)**. Near: for 99.44 % (ETS2) and 99.62 % (ATS) of all roads, the start node's forward direction is within 60° of the straight line to the end node. Every other component order and axis choice scores at most 39 %.

Roads with height changes (up to 90 units along one road) tilt these directions vertically. The model uses the full 3D direction.

## Length

The `f32` at road payload offset 208 is the road's length:

- For straight roads (both node directions along the chord), it equals the node distance within 0.01 % in 99.85 % of cases (sample of every 11th road: ETS2 3,350 of 3,355, ATS 6,150 of 6,158).
- For curved roads, it is longer than the node distance and matches the curve length (see below) within 0.1 % for 99.5 % of all roads.
- It is never 0, negative or non-finite (range about 1.3 to 998 units). In 152 roads it is slightly shorter than the node distance, so the model does not require `length ≥ distance`.

## Curve between the nodes (not confirmed)

Roads are not straight segments in general. About 23 % of the ETS2 roads turn by more than 30° between their ends. The curve is presumably a cubic Hermite spline: it starts at the start node, ends at the end node, and is tangent to both forward directions. What is not confirmed is the **length of the tangents**:

| Tangent length | Arc length matches the stored length within 0.1 % | Within 0.01 % |
|---|---|---|
| Node distance | 85 % (ETS2), 88 % (ATS) of sampled roads; fails above about 40° of turn | — |
| Circular-arc estimate `distance · (θ/2) / sin(θ/2)` | 95.5 % (both games) | 80–87 % for turns below 30°, 6–8 % for 60–90° |
| The stored length itself | 99.46 % (ETS2), 99.60 % (ATS) | 81 % (ETS2), 86 % (ATS) overall; 0.1 % for 90–120°; fails above 120° |

Two further tests also failed: no fixed polyline resolution reproduces the stored length exactly, and a fixed point (tangent length = arc length) matches only 57–64 % of the roads that turn by more than 45°. The curve formula is therefore **open question G1**, and the model provides **no curve interpolation**. It exposes only what is confirmed, as `RoadGeometry`: the end positions, the forward directions at both ends, the straight-line distance and the stored length.

## Model

| Type | Purpose |
|---|---|
| `NodeId`, `RoadId` | UIDs from the game data |
| `Position`, `Rotation` | Stored position (fixed point) and quaternion, with `world()` and `forward()` |
| `Node` | UID, position, rotation, `backward` and `forward` as `NodeLink` |
| `NodeLink` | `None`, `Road(RoadId)`, `Item(uid)` (exists, not a road, for example a prefab), `Unresolved(uid)` |
| `Road` | UID, start and end node, road look token, stored length |
| `RoadNetwork` | Validated roads and nodes: `roads_at(node)`, `successor(road)`, `predecessor(road)`, `connections()`, `geometry(road)` |
| `RoadConnection` | `from` road, `via` node, `to` road, in stored direction |
| `RoadGeometry` | Confirmed geometry, see above |
| `ModelError` | Zero UID, conflicting node copies, duplicate road, invalid rotation or length, road loop, missing node, link mismatch, node link to a road that does not start or end there |

`RoadNetwork::new` rejects all of the following:

- a road whose nodes are missing or do not refer back to it,
- a node that names a road which does not start or end at it,
- any of the invalid values listed above.

Nothing is dropped silently. The facade function `roadpilot_map::build_road_network(&[BaseSector])` converts parsed sectors: it classifies each node slot against all items of the given sectors, so the sectors that hold a road's nodes must be included.

Collections are ordered by UID, so results are deterministic. The model has no dependencies, uses no `unsafe` code, does no I/O and has no recursion.

## ETS2 and ATS

No structural difference was found. All rules above hold for both games; only the counts differ.

## Open questions

| ID | Question |
|---|---|
| G1 | The exact curve between two nodes: the tangent length (and whether it is a Hermite spline at all for sharp turns above 120°). Needed for lane geometry and curvature. |
| G2 | The world unit (T3). The scale ÷ 256 and the axes are confirmed, the unit is not. |
| G3 | What the remaining 200 bytes of the road payload contain. Some may affect geometry, for example offsets or a road-specific curve setting. |
| G4 | Node flags (except the country IDs in bits 8–23, see [SCS lane model](scs-lane-model.md)) and the 0.4–0.6 % of roads whose start direction points away from the end node by more than 60°. These are presumably very sharp curves, but this is not checked. |
| G5 | Which items the unresolved node links refer to, presumably `.aux` items. |
