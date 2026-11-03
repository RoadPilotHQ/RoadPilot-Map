# Navigation graph and route planning

Status: **implemented** (Phase 1, Step 6) in `crates/roadpilot-map-model` (`navigation`, `routing`). It uses only the road, lane and prefab models of the earlier steps; it reads no game data itself.

The graph restates what the earlier models know and **never upgrades a connection**. The route search is conservative: it uses unconfirmed connections only when the caller allows it, and then marks them.

The graph is exported for other programs in the format described in [Navigation graph export](navigation-export.md) (Step 7).

Not part of this step: live or dynamic routing, travel time or ETA, physical distances, speed, signal states, lane changes, vehicle control, and a map package export.

Evidence levels follow the other format documents: **Confirmed**, **Measured**, **Inferred**, **Unknown**.

## Where the code lives

The graph and the search are plain data and algorithms without I/O or dependencies, so they live in `roadpilot-map-model`, next to the models they are built from. The crate `roadpilot-map-network` named in [Crate structure](../architecture/crates.md) does not exist yet. The graph can move there later without changing its API. No new dependency was added.

## Nodes

A node is a place where traffic can be. Nodes are typed (`NavNode`) and never positions:

| Node | Meaning |
|---|---|
| `LaneEntry(LaneId)` | Where traffic enters a road lane, in the lane's driving direction |
| `LaneExit(LaneId)` | Where traffic leaves a road lane |
| `PrefabInput(PrefabSlotRef)` | A lane enters a prefab (junction) at this slot |
| `PrefabOutput(PrefabSlotRef)` | A lane leaves a prefab at this slot |

A `PrefabSlotRef` is the prefab UID, the index of the prefab node in the instance and the slot index. Only lanes with a known driving direction (resolved lanes and a known traffic side) are in the graph. Nodes are stored once, with a hash index for lookup (`node_id`) and 32-bit node IDs.

## Edges and their origin

An edge is a directed connection with a **status** and an **origin** (a record kind and the UID of the record it was derived from).

| Origin kind | From → to | Source UID | Status |
|---|---|---|---|
| `RoadLane` | `LaneEntry` → `LaneExit` of one lane | Road | **Candidate** |
| `RoadConnection` | `LaneExit` → `LaneEntry` of the next road's lane | Shared node | **Candidate** |
| `RoadToPrefabSlot` | `LaneExit` → `PrefabInput` | Prefab node | **Candidate** |
| `PrefabInternalCurve` | `PrefabInput` → `PrefabOutput` | Prefab | **Confirmed** |
| `PrefabSlotToRoad` | `PrefabOutput` → `LaneEntry` | Prefab node | **Candidate** |
| `PrefabToPrefab` | `PrefabOutput` of one prefab → `PrefabInput` of the prefab joined at the same node | Shared node | **Unknown** |
| `Other` | Defined by the caller (tests, later sources) | Caller | Caller |

### Why each status

- **Confirmed, internal prefab movements.** The slot assignments and the next/previous curve links are stored explicitly in the prefab descriptor ([SCS prefabs and junctions](../formats/scs-prefab-junctions.md#internal-lanes-and-movements)): a chain of stored curves leads from an input slot to an output slot. Only this stored movement inside the prefab is confirmed. Traffic rules stored with it are kept as references and do not change its status. The attachment of the slots to map nodes uses the node mapping of the prefab instance (origin byte, checked against the node distances; a few exceptions are unexplained, PJ2, NV8). `Confirmed` is a statement about the evidence for the connection. It says nothing about routing eligibility or about the drivability of a route; see [What `confirmed` means](navigation-export.md#what-confirmed-means).
- **Candidate, driving along a lane.** The lanes of a road are known, but the **driving direction** comes from the traffic-side rule, which is measured (about 98 %) and not confirmed ([SCS lane model](../formats/scs-lane-model.md#driving-direction), LM4, LM5). A route must not call a direction confirmed because of a rule that is only measured. This is the one place where the status is a judgement call; it is a single line in `NavigationGraph::build` (NV1).
- **Candidate, road lane to lane at a road connection.** The road connection is confirmed, but the pairing of lanes is by index, and the index order is unknown (LM2, LM6).
- **Candidate, road lane to prefab slot.** The same index pairing (LM2). The count match of lanes and slots (98.6 %) supports it but does not confirm the order.
- **Unknown, prefab to prefab.** Both prefabs have slots at the shared node, but no rule pairing them is confirmed (PJ4). The edges are created for **all** combinations of output and input slots at the joint, so the graph does not pretend to know which pair is meant. They are never used by a route.
- **Forbidden.** No source of explicit exclusions is known in the game data, so **the builder never produces Forbidden edges**. The status exists so that a confirmed source can be added later. A graph builder may add them, and routing already excludes them. A forbidden edge may not run parallel to another edge between the same nodes.

A confirmed part never makes the whole connection confirmed: the connection from a road into a prefab and out to the next road consists of candidate, confirmed and candidate edges, and stays three separate edges with three statuses.

No connection is created from geometric proximity, similar lane indices alone, or an assumed direction.

### Traffic rules

The curve traffic rules of a prefab movement (`traffic_rule_data` references) are stored with the edge (`NavigationGraph::rules`), as `RuleRef::Resolved` or `Unresolved`. They are **not interpreted** and do not influence routing, because their meaning is unknown (PJ5). Lane type rules (`traffic_rules[]`) are read by the lane definitions but are not attached to the graph. No priority rule, turn restriction or entry ban is assumed.

## Issues

What cannot become part of the graph is kept as a list (`NavigationGraph::issues`), each with the UID it concerns:

| Issue | Meaning |
|---|---|
| `RoadLanesNotNavigable` | Lanes unresolved or traffic side unknown: the road's lanes are not in the graph |
| `RoadTransitionUnpaired(reason)` | Two roads meet but their lanes cannot be paired (reason from the lane model) |
| `PrefabUnresolved` | The prefab is not tied to its definition |
| `PrefabArmUnpaired { arm, reason }` | A road at a prefab node cannot be paired with the slots (lane count differs, lanes unknown) |
| `UnresolvedNodeLink` | A node refers to a UID that matches no item (see PJ7) |
| `NodeLinksToOtherItem` | A node refers to an item that is neither a road nor a prefab |

## Building the graph

`NavigationGraph::build(&RoadNetwork, &LaneNetwork, &PrefabNetwork)` creates the graph in four passes:

1. Lane nodes and `RoadLane` edges for every lane with a known direction.
2. `RoadConnection` edges from the lane transitions of every road connection.
3. Slot nodes for every used slot of every resolved prefab.
4. `PrefabInternalCurve` edges from the prefab movements, road-to-slot edges from the lane/slot pairing, and `PrefabToPrefab` edges at joints.

`NavigationGraphBuilder` is public for other sources and for tests. Duplicates are handled in a fixed way:

- Adding a node twice is an error.
- An edge to a node that was not added is an error.
- Edges are sorted by start node and then by all their fields. Identical edges are merged and counted (`merged_duplicates`). Edges that differ in status or origin stay as parallel edges.
- Cycles are allowed. A road network has natural loops, and the search handles them.

The adjacency structure is a sorted edge array with one offset per node (compressed rows), so the out-edges of a node are one slice and no lookup scans the graph. IDs are 32-bit and checked: more than `u32::MAX` nodes or edges is an error. An edge is 24 bytes.

## Routing

### Points

A route runs from one node to another node of the graph. Start and goal are **typed nodes**, for example `LaneEntry(lane_id)` as the start and `LaneExit(lane_id)` as the goal. There is no lookup by coordinates and no nearest-lane matching. A node that is not in the graph gives `RouteError::UnknownStart` or `UnknownGoal`.

### Modes

| Mode | Uses | Result |
|---|---|---|
| `ConfirmedOnly` | Confirmed edges only | A route, or `NoConfirmedRoute` |
| `AllowCandidates` | Confirmed and candidate edges | A route, or `NoRoute` |

Unknown and forbidden edges are excluded in both modes. Nothing is released automatically.

`NoConfirmedRoute` means that no route with confirmed edges only was found. It does not say whether a provisional route exists.

A route (`Route`) lists its steps with the status and origin of each edge, the cost, and:

- `candidate_positions()`: the positions of the candidate steps;
- `confirmation()`: `AllConnectionsConfirmed` if no step is a candidate, otherwise `Provisional { candidate_edges }`.

A provisional route is never called confirmed or drivable. Even `AllConnectionsConfirmed` only says that all of its connections are confirmed; it does not say that the route can be driven, because rules, signals and driving behaviour are not evaluated.

### Algorithm

Dijkstra's algorithm with a binary heap, an explicit loop without recursion and memory proportional to the number of nodes. It stops when the goal is settled. Ties are resolved by node order, so equal inputs give equal routes.

### Cost

The cost is **algorithmic, not physical**, and has two parts compared in this order:

1. the number of candidate edges,
2. the number of edges.

So a route with fewer unconfirmed connections is preferred over a shorter one, and between equally unconfirmed routes the one with fewer edges wins. This is deliberate for a conservative first version.

No road length is used (the unit is unconfirmed, G2, T3), no curve length is estimated (G1), and no time, ETA or speed is computed.

## Measurements

Current local installations, release build, one machine. Build time is for the graph only; the road, lane and prefab models were built before.

| | ETS2 | ATS |
|---|---|---|
| Nodes | 1,275,416 | 1,331,699 |
| Edges | 1,374,901 | 1,456,680 |
| Confirmed | 319,384 | 375,973 |
| Candidate | 1,008,339 | 1,014,758 |
| Unknown | 47,178 | 65,949 |
| Forbidden | 0 | 0 |
| Merged duplicates | 0 | 0 |
| Build time | 1.5 s | 1.7 s |
| Heap, lower bound | 119 MiB | 127 MiB |

Edges by origin:

| Origin | ETS2 | ATS |
|---|---|---|
| `PrefabInternalCurve` (confirmed) | 319,384 | 375,973 |
| `RoadLane` | 434,822 | 426,745 |
| `RoadConnection` | 247,582 | 226,353 |
| `RoadToPrefabSlot` | 162,996 | 181,052 |
| `PrefabSlotToRoad` | 162,939 | 180,608 |
| `PrefabToPrefab` (unknown) | 47,178 | 65,949 |

Issues:

| Issue | ETS2 | ATS |
|---|---|---|
| `RoadLanesNotNavigable` (R2: traffic side unknown for any reason) | 36,418 | 25,853 |
| `RoadTransitionUnpaired` | 28,312 | 19,758 |
| `PrefabArmUnpaired` | 11,622 | 11,542 |
| `PrefabUnresolved` | 860 | 1,030 |
| `UnresolvedNodeLink` | 1,419 | 2,688 |
| `NodeLinksToOtherItem` | 431,341 | 546,284 |

The heap figure is computed from the sizes of the structures; allocator overhead and hash table slack are not included, and the process peak was not measured. The result is reproducible: two builds are equal.

### Routes on the real maps

For each game, the first lane of each cell of a 3 × 3 grid over the map was used as a start (nine regions). For each start, the farthest lane exit reachable with candidates was the goal.

- A route was found in all nine regions in both games. The routes have 1 to 1,239 edges, and all of them use candidate edges (between 1 and 1,136).
- **Confirmed-only reachability is 1 node** from every lane start: nothing beyond the start itself. Confirmed routes exist only inside a prefab, from an input slot to an output slot (checked with a stored prefab movement as a route).
- The reachable set with candidates is small compared with the graph: 4 to about 23,000 nodes of about 1.3 million. The [connectivity audit](connectivity-audit.md) (Step 6.1) explains it: the reach is limited by the size of the connected pieces, which are cut by roads without a known traffic side and by directly joined prefabs (unknown edges). No implementation bug was found.
- Every checked route is connected from start to goal, uses allowed edges only, and is reproducible.
- Nine searches including a full reachability scan each took about 130 ms in total in both games.

These numbers say how much of the map the **current data** supports. They are not a statement about the game's real connectivity.

## Limits of this version

- `ConfirmedOnly` cannot cross a road at all, because lane direction and lane pairing are not confirmed. This follows from the data and the status rules above, not from the algorithm.
- There is no start or goal from a position, an address or a road name.
- No cost reflects length, time, speed, road class or turns.
- Traffic rules and signals are not evaluated.
- Lane changes are not modelled: a lane is a node pair, and no edge joins neighbouring lanes.

## Open questions

The earlier questions G1–G5, LM1–LM6 and PJ1–PJ8 are unchanged; see [SCS road geometry](../formats/scs-road-geometry.md), [SCS lane model](../formats/scs-lane-model.md) and [SCS prefabs and junctions](../formats/scs-prefab-junctions.md). New:

| ID | Question |
|---|---|
| NV1 | Whether the traffic-side rule that gives lane directions can be confirmed (LM4, LM5). Then driving along a lane could become confirmed. Tested against prefab slots in the [connectivity audit](connectivity-audit.md): 99.3–99.5 % agree, a few contradictions are unexplained, so `Candidate` stays. |
| NV2 | A physical route cost needs the curve formula (G1) and the unit (G2/T3). Until then the cost is not a distance. |
| NV3 | Answered in the [connectivity audit](connectivity-audit.md): small components, caused by roads without a traffic side (NV7) and directly joined prefabs (PJ4). |
| NV4 | Where positions come from for start and goal (a spatial index with a defined matching rule). No proximity matching is done today. |
| NV5 | Answered in the [connectivity audit](connectivity-audit.md): no road end and no prefab node links to such an item, so they do not affect connectivity. |
| NV6 | Whether and how traffic rules and signals restrict a movement (PJ1, PJ5). |
| NV7 | How the game assigns a traffic side to the roads whose nodes name no country (R1: 35,948 in ETS2, 25,280 in ATS). Together with the roads whose nodes name different countries and the few other cases, 36,418 and 25,853 roads (R2) have no known traffic side. They are not in the graph and cut the network into pieces. See [the road counts](connectivity-audit.md#road-counts-and-their-definitions). |
| NV8 | Whether the contradicting prefabs of the NV1 test (8 in ETS2, 25 in ATS) show an error in the node mapping or an exception of the direction rule. |
