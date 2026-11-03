# SCS prefabs, junctions and traffic attributes

Status: **implemented** (Phase 1, Step 5):

- `crates/roadpilot-map-formats`: prefab descriptor parser (`ppd`), `PrefabModelDef` and `TrafficRuleDef` (`definitions`), and the prefab origin byte (`Prefab::origin`);
- `crates/roadpilot-map-model`: prefab types (`prefabs`);
- `crates/roadpilot-map`: `PrefabDefinitions` and `build_prefab_network`.

It builds on [SCS sector format](scs-sector-format.md), [SCS road geometry](scs-road-geometry.md) and [SCS lane model](scs-lane-model.md). Not part of this step: the curve between nodes and lane geometry (G1), routing and navigation, the meaning of traffic rules, signal states and timing, and the other descriptor blocks.

Evidence levels: **Confirmed** holds for all inspected data. **Measured** gives a stated share. **Inferred** follows from confirmed facts but was not tested on its own. **Unknown** is not established.

Data basis: every prefab item of a local ETS2 (73,211) and ATS installation (84,874), every `prefab_model` and `traffic_rule_data` definition of the base game and the installed DLCs, and every `.ppd` descriptor those models reference (4,238 and 1,947 files). Everything was read read-only. The analysis programs are throwaway tools outside the repository. The opt-in test `builds_the_prefab_network_of_the_installation` in `crates/roadpilot-map/tests/real_installation.rs` reproduces the model statistics below.

ATS was updated by the game launcher during this step. The ATS figures here are from the updated installation. Figures in older documents come from the previous version and differ slightly (for example 224,868 instead of 224,870 roads).

Sources: ts-map (dariowouters/ts-map, MIT) was used as a reference for the descriptor header and the node record; no code was copied. The curve, semaphore and block layout below was established from the data. TruckLib (GPL-2.0) was not consulted.

## Definition and instance

| | Where | Content |
|---|---|---|
| **Definition** | `prefab_model` unit in `def/world/prefab*.sii`; its `prefab_desc` names a `.ppd` file | Nodes, internal lane curves, traffic lights and more, in local coordinates |
| **Instance** | Prefab item (type 4) in a sector `.base` file | Model token, its own node UIDs, an origin byte, attached items |

- Confirmed: every prefab item's model token names a `prefab_model` unit (`prefab.<token>`), and every used model's descriptor exists and parses (both games).
- Several models can share one descriptor (ETS2 4,246 models, 4,238 distinct descriptors). The model keeps one definition per model name.
- A descriptor is read once per path, however many instances use it.

## Descriptor format (`.ppd`, version 25)

Established on all 4,238 (ETS2) and 1,947 (ATS) descriptors that the prefab models reference. All parse with the layout below; the parser rejects anything else.

```text
0x00  u32       version, 25 in every file
0x04  u32[11]   counts: nodes, curves, signs, semaphores, spawn points,
                terrain points, terrain variants, map points, trigger points,
                intersections, last block
0x30  u32[12]   block offsets in that order; terrain points have two blocks
                (positions and normals) with the same count
0x60  blocks    contiguous, each directly after the previous one; the last
                block ends exactly at the end of the file
```

| Block | Entry size | Decoded |
|---|---|---|
| Nodes | 104 | Yes |
| Navigation curves | 132 | Yes |
| Signs | 52 | No (count only) |
| Semaphores | 84 | Position, rotation, profile token |
| Spawn points | 36 | No |
| Terrain point positions, normals | 12 each | No |
| Terrain variants | 8 | No |
| Map points | 48 | No |
| Trigger points | 48 | No |
| Intersections | 16 | No |
| Last block | 188 | No; meaning unknown (PJ6) |

Confirmed: the offsets follow from the counts and entry sizes in every file, and every file ends exactly where the last block ends.

### Node (104 bytes)

| Offset | Type | Field | Evidence |
|---|---|---|---|
| 0x10 | `f32[3]` | Position, local | Confirmed by the instance geometry (below) |
| 0x1C | `f32[3]` | Direction | Confirmed unit length |
| 0x28 | `i32[8]` | Input lane slots: curves that **start** at this node (lanes entering the prefab), -1 = empty | Measured (below) |
| 0x48 | `i32[8]` | Output lane slots: curves that **end** at this node (lanes leaving the prefab) | Measured (below) |

Bytes 0x00–0x0F are not interpreted.

Slot curves start or end beside the node, not on it, as lanes lie beside a road's centre line. **Measured:** the start of an input curve, or the end of an output curve, lies on the line through the node across its direction (less than 0.05 units along the direction):

| Slot curves on the node's cross line | ETS2 | ATS |
|---|---|---|
| Input | 14,051 of 14,138 (99.4 %) | 7,141 of 7,204 (99.1 %) |
| Output | 13,915 of 14,053 (99.0 %) | 6,676 of 6,750 (98.9 %) |

The slot index order across the road (LM2) is not established by this.

### Navigation curve (132 bytes)

A curve is one segment of an internal lane.

| Offset | Type | Field | Evidence |
|---|---|---|---|
| 0 | token | Curve name | Not interpreted |
| 16, 28 | `f32[3]` | Start and end position | Confirmed: the end of a curve equals the start of each next curve (100 %) |
| 40, 56 | `f32[4]` | Start and end rotation | Unit quaternions |
| 72 | `f32` | Length | Not checked against a curve formula |
| 76 | `i32[4]` | Next curves, -1 = none | Confirmed |
| 92 | `i32[4]` | Previous curves | Confirmed: every next link is mirrored by a previous link (100 %) |
| 108, 112 | `u32` | Number of next and previous links | Confirmed: equal to the non-empty entries (100 %) |
| 116 | `i32` | Unknown. Not a semaphore index and not a semaphore field. | Kept raw (`unknown_116`, PJ1) |
| 120 | token | Traffic rule; 0 = none | Confirmed: every non-zero value names a `traffic_rule_data` unit (`traffic_rule.<token>`) |

ETS2 has 75,029 curves (1,486 with a rule) and ATS 32,185 (5,630 with a rule).

The parser rejects out-of-range curve indices, link counts that do not match, and next links without a matching previous link.

### Semaphore (84 bytes)

| Offset | Type | Field | Evidence |
|---|---|---|---|
| 0 | `f32[3]` | Position, local | |
| 12 | `f32[4]` | Rotation | Confirmed unit quaternion |
| 56 | token | Profile | Matches the middle part of a `tr_semaphore_profile` unit name. How the full profile is chosen is unknown (PJ3). |

ETS2 descriptors hold 6,465 semaphores and ATS descriptors 5,429.

## From instance to definition

### Node mapping

- Confirmed: an instance has exactly as many nodes as its definition.
- **The origin byte.** It is stored 8 bytes after the connected item list (see [SCS sector format](scs-sector-format.md#decoded-item-fields)). Instance node `i` is definition node `(i + origin) % n`.
- **Measured:** this mapping keeps all horizontal distances between the nodes, within 0.05 units, for 72,351 of 73,211 (ETS2) and 83,844 of 84,874 (ATS) instances (98.8 % each). Heights are not compared.
- The identity mapping fits 46,736 (ETS2) and 62,867 (ATS) instances, and `(i − origin) % n` fits 48,100 and 65,959. Every instance that fits one of them also fits the origin mapping.
- The remaining 860 and 1,030 instances fit none of the three. Why is unknown (PJ2).

The builder applies this check. An instance that fails it gets `NodeGeometryMismatch`. Its nodes are not mapped and it has no movements; no other mapping is guessed.

| Instance resolution | ETS2 | ATS |
|---|---|---|
| `Resolved` | 72,351 | 83,844 |
| `NodeGeometryMismatch` | 860 | 1,030 |
| `MissingModel`, `MissingDescriptor`, `InvalidDescriptor`, `NodeCountMismatch`, `OriginOutOfRange` | 0 | 0 |

### Prefab nodes and road connections

A prefab node is an ordinary map node.

- Confirmed: every prefab node refers back to its prefab in one of its two item slots. The model rejects instances where this does not hold.
- The other slot says what is attached to that node.

| Prefab node attached to | ETS2 | ATS | Model |
|---|---|---|---|
| A road | 167,221 | 165,008 | `ArmLink::Road { road, road_ends_here }` |
| Another prefab | 26,914 | 32,324 | `ArmLink::Prefab` (each joint counted from both prefabs) |
| Nothing | 13,511 | 33,572 | `ArmLink::None` |
| A UID that matches no item | 12 | 1 | `ArmLink::Unresolved` |

A prefab node with a prefab on the other side is where two prefabs are joined directly, without a road between them.

#### Unresolved node links

Unresolved node links from Step 4A keep their meaning (`NodeLink::Unresolved`, a UID without a loaded item):

- 1,316 of 1,355 such UIDs (ETS2) and 2,558 of 2,566 (ATS) occur somewhere in the sector `.aux` files.
- `.aux` files are not parsed, so what these items are is unknown (PJ7).

## Internal lanes and movements

An **internal movement** (`PrefabMovement`) is a path through the curve graph from an input slot of one node to an output slot of a node. It can pass through several curves.

- The path search follows the stored next links.
- It is iterative and bounded by the number of curves, so loops terminate.
- The movement lists the traffic rules found on the curves between the two slots.

These movements are **Confirmed**: the curve links and slot assignments are stored explicitly. Two things are not modelled:

- whether a vehicle may use a movement under a traffic rule;
- whether a traffic light governs it.

| Internal movements | ETS2 | ATS |
|---|---|---|
| Total, resolved instances | 319,384 | 375,973 |
| With at least one traffic rule | 17,800 | 173,846 |

## Road lanes at a prefab

A road's lanes meet a prefab node's slots:

- Lanes driving **into** the prefab use its input slots. They are the forward lanes if the road ends at the node, and the backward lanes if it starts there.
- Lanes driving **out of** the prefab use its output slots.
- Lane directions come from the lane model (traffic side).

**Measured:** where the road's lanes and traffic side are known, the number of road lanes in each direction equals the number of used slots for:

- ETS2: 322,055 of 326,491 arm sides (98.6 %);
- ATS: 317,125 of 321,469 arm sides (98.6 %).

This also supports the lane direction rule of the lane model for two-way roads (previously only inferred).

The pairing itself is by index: the n-th used slot gets the n-th lane. The lane order is not confirmed (LM2), so a **road lane to slot pairing is a candidate**.

A `LaneMovement` connects a road lane into the prefab, an internal movement, and a road lane out of it. Its status is always `MovementStatus::Candidate`.

| Arm sides (input or output side of a road-attached node) | ETS2 | ATS |
|---|---|---|
| Paired | 322,055 | 317,125 |
| `LaneCountMismatch` | 4,436 | 4,344 |
| `RoadLanesUnknown` (lanes unresolved or traffic side unknown) | 3,593 | 3,599 |
| Road-to-road lane movements (candidates) | 219,243 | 241,860 |

Arms without a road are reported as `NoRoad` (40,229 and 65,711 arm sides).

## Junctions

There is no separate junction record. A **junction is a prefab instance**: its arms are the nodes, and its movements are the stored internal paths.

Larger junctions are made of several prefabs joined node to node. Lane transitions across such a joint are **Unknown**: both sides have slots, but no rule that pairs them is confirmed (PJ4). The model therefore builds no junction structure of its own. It does not merge prefabs and does not derive turn types or junction shapes.

## Connection status

| Status | Used for |
|---|---|
| **Confirmed** | Road–prefab attachment at a node (stored node links); internal movements (stored curve links); instance-to-definition node mapping where the geometry check passes |
| **Candidate** | Road lane to prefab slot, and therefore road lane to road lane through a prefab |
| **Unknown** | Prefab-to-prefab lane transitions; arms with a lane count mismatch or unknown lanes; instances that fail the geometry check; the semaphore that governs a movement |
| **Forbidden** | **Not produced.** No field was found that marks a movement as forbidden. Curves without a link are simply not connected, and traffic rules are referenced but not interpreted. |

The road-to-road lane transitions of Step 4B (`LaneNetwork::transition`) are unchanged: index candidates at shared road nodes.

## Traffic attributes

| Attribute | Source | Model |
|---|---|---|
| Traffic rule on a curve | Curve record offset 120 | `RuleRef::Resolved(name)`, or `Unresolved(name)` if no such `traffic_rule_data` exists (does not occur in the game data) |
| Traffic rules of a lane type | `traffic_lane_data.traffic_rules[]` | `TrafficLaneDef::traffic_rules`, read but not yet attached to lanes |
| Rule content | `traffic_rule_data.rule` | `TrafficRuleDef::rule`, kept as stored; not interpreted (PJ5) |
| Prefab-wide input and output rules | `prefab_model` attributes | Not read; how they combine with curve rules is unknown (PJ5) |
| Traffic lights | `.ppd` semaphores | `PrefabSemaphore`: local position, rotation, profile token; static only |

There are 196 (ETS2) and 179 (ATS) `traffic_rule_data` units. Resolved instances with traffic lights: 17,811 (ETS2) and 21,022 (ATS).

Signal phases, timing and the current state are runtime information. They are not part of the map data and are not modelled.

## Model

| Type | Purpose |
|---|---|
| `PrefabId` | Prefab item UID |
| `PrefabDefinition`, `PrefabDefinitionNode`, `PrefabCurve`, `PrefabSemaphore`, `RuleRef` | A prefab definition, per model name |
| `PrefabInstance`, `PrefabResolution` | A placed prefab, its origin and node mapping (`definition_node`, `instance_node`), or why it is not resolved |
| `PrefabArm`, `ArmLink` | One prefab node and what is attached to it |
| `PrefabMovement` | A confirmed internal path from an input slot to an output slot, with its traffic rules |
| `LaneMovement`, `MovementStatus`, `PrefabLaneMovements`, `ArmIssue`, `SlotDirection` | Candidate road-lane movements through a prefab, and the arms that cannot be paired |
| `PrefabNetwork` | Validated definitions and instances; `arms`, `movements`, `lane_movements` |
| `PrefabError` | Duplicate instance or definition, missing node, node not linked to its prefab, inconsistent resolution, invalid definition |

`roadpilot_map::PrefabDefinitions::from_units` collects `prefab_model` and `traffic_rule_data` units and rejects duplicates. `roadpilot_map::build_prefab_network` builds the network from the road network, the parsed sectors, the definitions and a function that loads a file by archive path.

## ETS2 and ATS

The format and all rules are identical. Differences are in the data only:

- ATS has more open prefab nodes and more prefab-to-prefab joints.
- ATS has many more curves with traffic rules (5,630 of 32,185, against 1,486 of 75,029 in ETS2).

## Open questions

| ID | Question |
|---|---|
| PJ1 | Meaning of the curve field at offset 116, and which semaphore (if any) governs a curve. |
| PJ2 | Why 1.2 % of instances do not fit the origin mapping (mirrored, scaled or differently rotated prefabs?). |
| PJ3 | How the full `tr_semaphore_profile` is chosen from the semaphore token and the `prefab_model`. |
| PJ4 | How lanes continue across two directly joined prefabs (slot pairing at a shared node). |
| PJ5 | Meaning of `traffic_rule_data` rules, and how prefab-wide input and output rules relate to curve rules. |
| PJ6 | Content of the undecoded descriptor blocks (signs, spawn, terrain, map points, triggers, intersections, last block), in particular whether map points or intersections are needed. |
| PJ7 | What the items behind unresolved node links are (they occur in `.aux` files). |
| PJ8 | Lane drops and merges at a prefab arm with a lane count mismatch (1.4 % of arm sides). |

Open questions G1 (curve formula) and LM1–LM6 are unchanged; see [SCS road geometry](scs-road-geometry.md) and [SCS lane model](scs-lane-model.md).
