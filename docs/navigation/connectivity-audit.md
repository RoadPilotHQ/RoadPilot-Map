# Navigation graph connectivity audit (Step 6.1)

This is an audit of the Step 6 [navigation graph](navigation-graph.md). It explains why the graph reaches so little from a typical start (4 to about 23,000 of 1.3 million nodes), and whether the `Candidate` status of driving along a lane (NV1) is justified. **No code was changed.** No connection was added, upgraded or relaxed.

Data basis: the current local ETS2 and ATS installations, read read-only. The analysis programs are throwaway tools outside the repository. They use the repository crates and print aggregate counts only; no game data is stored in the repository. The figures below are not reproduced by an automated test.

Every finding has one of these classifications:

- **Bug**: a confirmed implementation bug.
- **Source data**: the data needed is missing or unresolved.
- **Interpretation**: an interpretation of the SCS format is unconfirmed.
- **Expected**: an expected disconnection of the map.
- **Not understood**: the cause is not known.

## Summary

| Finding | Classification |
|---|---|
| The graph is built as designed. Counts, edge endpoints and the route search were verified independently. | No bug found |
| Roads with an unknown traffic side (36,418 ETS2 roads = 14.4 %, 25,853 ATS roads = 11.5 %; of these 35,948 and 25,280 name no country) are not in the graph. They cut the road network into pieces. See [the road counts](#road-counts-and-their-definitions). | Source data (country IDs absent); what the game does there is not understood |
| Directly joined prefabs have only `Unknown` edges. About 13,500 (ETS2) and 16,200 (ATS) such joints split the network further. | Interpretation (PJ4) |
| Roads that end at nothing and prefab nodes with nothing attached end routes. | Expected |
| Lane count mismatches at prefab arms, unresolved prefab instances, lane count differences between roads | Interpretation / not understood, small |
| The traffic-side rule that gives lane directions holds for 99.3–99.5 % of the arms where it can be tested. The exceptions are not explained. | `Candidate` stays justified (NV1) |

## 1. Repository state

Starting commit `41b1542` (Step 6) on `main`, in sync with `origin/main`, working tree clean.

## 2. Verification of the Step 6 figures

The node and edge counts were recomputed independently from the road, lane and prefab models. They match exactly.

| | ETS2 | ATS |
|---|---|---|
| Nodes: 2 per lane with a known direction + used prefab slots | 869,644 + 405,772 = 1,275,416 | 853,490 + 478,209 = 1,331,699 |
| Edges: confirmed (= prefab movements) / candidate / unknown | 319,384 / 1,008,339 / 47,178 | 375,973 / 1,014,758 / 65,949 |
| Parallel edges with equal endpoints and status | 0 | 0 |

Edge endpoints were checked for every edge: the endpoints belong to the node or record named in the edge's origin (a road connection joins the exit of one lane and the entry of another at the same map node, a road-to-prefab edge at the prefab's node, an internal edge stays inside one prefab, a joint edge joins two different prefabs at their shared node). There were **no violations** (all 1.37 million ETS2 edges, all 1.46 million ATS edges). Every lane entry has its traversal edge to the exit of the same lane.

## 3. Route search

The search was checked against independent implementations that use only the public edge list:

| Check | ETS2 | ATS |
|---|---|---|
| Reachable sets (breadth-first search) in both modes, for 126 searches | 0 mismatches | 0 mismatches |
| Route costs (candidate edges, then edges) against an independent Dijkstra | 342 routes, 0 mismatches | 299 routes, 0 mismatches |

Directed edges, candidate edges, lexicographic costs and start/goal validation are also covered by the unit tests of Step 6. The real maps contain no parallel edges with equal endpoints and status, so that case is covered by the unit tests only. Cycles occur in the real graph (see strong components below).

## 4. Components

### Lane graph

| | ETS2 | ATS |
|---|---|---|
| **Weak components, all edges including unknown** | 11,674; largest 133,852, 110,097, 102,338 | 14,547; largest 194,287, 111,884, 66,092 |
| **Weak, confirmed and candidate edges** | 28,530; largest 26,758, 21,805, 19,544 | 34,979; largest 19,810, 16,033, 13,517 |
| Nodes in components of at most 100 / 1,000 nodes | 378,112 / 821,181 (64 %) | 338,851 / 826,810 (62 %) |
| **Weak, confirmed only** | 1,001,698; largest 20 | 1,001,730; largest 28 |
| Isolated nodes (confirmed only) | 869,644 (all lane nodes) | 853,490 (all lane nodes) |
| **Strong, confirmed and candidate edges** | 1,010,542; largest 20,642; 1,008,738 single nodes | 1,073,937; largest 4,668; 1,072,119 single nodes |
| **Strong, confirmed only** | all nodes alone (no cycles) | all nodes alone |

Findings:

- Without unknown edges the network falls into 28,530 (ETS2) and 34,979 (ATS) components. Including the unknown joint edges, it falls into 11,674 and 14,547. The unknown edges therefore bridge about 16,900 (ETS2) and 20,400 (ATS) gaps. This is an upper bound of what their pairing could contribute, because the unknown edges join all slot combinations.
- The largest reachable set of the sampled starts (23,364 in ETS2) is close to the largest weak component (26,758). **Reach is limited by the components, not by the direction of the edges.**
- With candidate or unknown edges, no node is isolated: every lane node has its traversal edge.
- With confirmed edges only, no component is larger than 28 nodes: confirmed edges exist only inside prefabs.
- The strong components are mostly single nodes (79 % in ETS2, 80.5 % in ATS). Inside the large weak components they are large: the largest strong component in ETS2 covers 77 % of the largest weak component. In ATS it covers 24 %. The single nodes belong mostly to the small components. The reason in ATS was not analysed further.

### Meaning of the large components

The 5 largest components of the graph with candidates have 11,700 to 26,800 nodes (ETS2) and cover 2,000 to 3,500 roads and 550 to 930 prefabs each, within an extent of 12,000 to 50,000 units on a side. They are connected pieces of the road network, not construction artefacts: they contain roads and junctions together.

### Topology without lanes

To separate expected disconnection from graph construction effects, the same analysis was done on the plain topology (roads and prefab instances as vertices, node references as links), removing one cause at a time:

| Stage | ETS2: components, largest | ATS: components, largest |
|---|---|---|
| S0: all roads and prefabs | 11,203; 254,505 of 325,338 (78 %) | 16,471; 267,376 of 309,742 (86 %) |
| S1a: without roads that have an unknown traffic side or unresolved lanes | 6,245; 30,339 | 13,190; 42,433 |
| S1b: without roads that have zero lanes | 10,650; 246,836 | 16,315; 267,371 |
| S1: without both | 6,827; 30,306 | 13,190; 42,433 |
| S2: S1 and only resolved prefabs | 7,312; 29,347 | 13,571; 40,818 |
| S3: S2 and no prefab-to-prefab links | 15,098; 4,417 (83 % of vertices in components of at most 1,000) | 23,115; 6,064 (71 %) |

- **S0.** Almost the whole map (78 % / 86 % of the vertices) is one connected topology. The rest are islands, an expected disconnection.
- **S1a.** Removing the roads without a traffic side breaks that giant component from 254,505 to 30,339 vertices (ETS2) and from 267,376 to 42,433 (ATS). These roads are structural bridges.
- **S1b.** Zero-lane roads do not matter for connectivity.
- **S2.** Unresolved prefabs have a minor effect (860 and 1,030 instances).
- **S3.** Removing the joints splits the network further: 7,312 → 15,098 components (ETS2) and 13,571 → 23,115 (ATS), with the largest component shrinking from about 29,000 to 4,400 (ETS2).

## 5. Reachability from representative starts

125 starts per game, spread over a 3 × 3 grid of the map (5 per cell), plus 40 prefab inputs and 40 prefab outputs, taken evenly from graph order. "Candidates" means confirmed and candidate edges.

| Start | Starts | Candidates: median / max nodes | Reach more than 1 road / more than 1 prefab (of starts) | Confirmed only: median / max nodes |
|---|---|---|---|---|
| ETS2 `LaneEntry`, 1 + 1 lanes | 24 | 839 / 23,349 | 22 / 18 | 1 / 1 |
| ETS2 `LaneEntry`, lanes on one side only | 7 | 12 / 212 | 6 / 6 | 1 / 1 |
| ETS2 `LaneEntry`, multi-lane | 14 | 2,404 / 23,364 | 13 / 11 | 1 / 1 |
| ETS2 `PrefabInput` | 40 | 38 / 13,074 | 33 / 33 | 2 / 5 |
| ETS2 `PrefabOutput` | 40 | 33 / 13,074 | 31 / 30 | 1 / 1 |
| ATS `LaneEntry`, 1 + 1 lanes | 20 | 263 / 6,995 | 17 / 16 | 1 / 1 |
| ATS `LaneEntry`, lanes on one side only | 8 | 74 / 768 | 7 / 6 | 1 / 1 |
| ATS `LaneEntry`, multi-lane | 17 | 65 / 5,681 | 13 / 10 | 1 / 1 |
| ATS `PrefabInput` | 40 | 64 / 5,031 | 29 / 29 | 2 / 4 |
| ATS `PrefabOutput` | 40 | 20 / 5,049 | 25 / 26 | 1 / 1 |

- In the mode that allows candidates, most starts reach several roads and junctions (up to 3,187 roads and 665 prefabs from one start in ETS2). Routes are **not** confined to one prefab.
- In the confirmed-only mode, a lane start reaches only itself (1 node, 0 edges). A prefab input reaches at most 5 nodes and 4 edges, and **never more than one prefab**. No confirmed edge starts at a lane node.

## 6. Edge categories

Edges of every category, by status, usable edges in each mode, edges whose target has no usable outgoing edge, and edges inside components of at most 100 nodes (candidate mode).

| Category | Status | ETS2 edges | Dead-end target | In components of at most 100 | ATS edges | Dead-end target | In components of at most 100 |
|---|---|---|---|---|---|---|---|
| `RoadLane` | Candidate | 434,822 | 24,244 | 121,933 | 426,745 | 19,340 | 95,896 |
| `RoadConnection` | Candidate | 247,582 | 0 | 71,313 | 226,353 | 0 | 52,930 |
| `RoadToPrefabSlot` | Candidate | 162,996 | 0 | 40,831 | 181,052 | 0 | 35,560 |
| `PrefabInternalCurve` | Confirmed | 319,384 | 58,885 | 85,250 | 375,973 | 82,844 | 96,113 |
| `PrefabSlotToRoad` | Candidate | 162,939 | 0 | 40,794 | 180,608 | 0 | 35,290 |
| `PrefabToPrefab` | Unknown | 47,178 | – | – | 65,949 | – | – |

Usable in each mode: the confirmed edges (319,384 and 375,973) in both modes; the candidate edges in the mode that allows candidates only; the unknown edges in neither mode.

Missing or asymmetric connections:

- **Road connections.** ETS2: of 151,258 connections, 83,884 pair lanes in both directions, 32,606 in the forward direction only, 22 backward only, and 6,434 have no lanes to pair. ATS: 130,602 connections; 67,981 both, 42,859 forward only, 0 backward only, 4 without lanes. One-directional connections are the one-way roads. No asymmetry points to a construction fault.
- **Unpaired connections.** ETS2: 28,279 because a traffic side is unknown, 33 because the lane count differs. ATS: 19,708 and 50.
- **Prefab arms.** Road arms where both the input and output sides pair: 87,488 and 33,373 + 33,339 with one empty side (ETS2). Arms with a lane count mismatch: about 4,400 arm sides in each game. Arms with unknown road lanes: 3,593 (ETS2) and 3,599 (ATS).
- Every used input slot has a stored movement (202,917 input slots in ETS2, none without a movement).

## 7. Why routes end

Nodes without a usable outgoing edge ("terminal") and without a usable incoming edge ("source"), by cause. The cause is derived independently from the road and node records. "UNEXPLAINED" would have meant a missing edge although a valid pairing exists. **It occurs in no case.**

Terminal nodes (candidate mode):

| Cause | Class | ETS2 | ATS |
|---|---|---|---|
| Output of a prefab joined to another prefab (only unknown edges) | Interpretation (PJ4) | 27,565 | 37,871 |
| Road ends at nothing | Expected | 18,788 | 13,722 |
| Prefab node with nothing attached | Expected | 8,374 | 16,945 |
| Next road has an unknown traffic side (lane exits) | Source data | 1,413 | 1,222 |
| Road arm at a prefab: lane count mismatch | Interpretation (LM2, PJ8) | 2,687 | 2,399 |
| Road arm at a prefab: road lanes unknown | Source data | 3,326 | 3,241 |
| Prefab unresolved | Not understood (PJ2) | 1,908 | 2,132 |
| Transition: lane count differs | Interpretation (LM6) | 49 | 98 |
| Unresolved UID | Source data (PJ7) | 45 | 11 |
| Next road has no lanes | Expected | 5 | 0 |
| **Total** | | **64,160** | **77,641** |

The source nodes (no incoming edge) show the same causes with nearly the same counts, so the graph is symmetric in this respect.

Inside the union of the reach sets of the sampled starts (104,247 nodes in ETS2), 1,390 nodes are terminal. The causes are mostly the same: joints with unknown edges (825), lane exits of roads that end at nothing (1,509) and open prefab nodes (480).

The item type behind `NodeLinksToOtherItem`: **no road end and no prefab node is linked to such an item.** The 431,341 (ETS2) and 546,284 (ATS) links belong to nodes that are used by neither roads nor prefabs, so they have no effect on connectivity (NV5, answered).

## 8. Roads that are not in the graph

| Roads | ETS2 | ATS |
|---|---|---|
| Navigable with lanes | 205,616 | 199,007 |
| Navigable but zero lanes | 10,093 | 8 |
| Traffic side unknown: nodes name no country | 35,948 | 25,280 |
| Traffic side unknown: different countries | 470 | 546 |
| Traffic side unknown: other (look conflict, country not defined) | 0 | 27 |

### Road counts and their definitions

Several documents and reports give a number of "roads without a traffic side". The numbers differ because they count different things. The three definitions below are exact, and the figures are reproduced for the current installations (road records of the sector files, the lane model of Step 4B; lanes are resolved for every road in both games).

| ID | Definition | ETS2 | ATS |
|---|---|---|---|
| R0 | All roads | 252,127 | 224,868 |
| R1 | **Nodes name no country**: the traffic side is unknown because neither node of the road has a country ID (`NoCountry`) | 35,948 (14.3 %) | 25,280 (11.2 %) |
| R2 | **Unknown traffic side for any reason**: R1, plus nodes that name different countries (`CountryMismatch`), plus a road look that contradicts the country (`LookConflict`), plus a country that is not defined (`UnknownCountry`) | 36,418 (14.4 %) = 35,948 + 470 | 25,853 (11.5 %) = 25,280 + 546 + 14 + 13 |
| R3 | **No lane node in the graph**: R2, plus roads with resolved lanes but zero lanes | 46,511 (18.4 %) = 36,418 + 10,093 | 25,861 (11.5 %) = 25,853 + 8 |

- **R2 is the number of `road_lanes_not_navigable` issues** in the graph and in the export (36,418 and 25,853). It is the authoritative count of "roads that are not in the graph because their traffic side is unknown". The export limitation `roads_without_known_traffic_side_are_not_exported` refers to R2.
- **R1 is the number of the Step 6.1 report** (35,948 and 25,280) and of open question NV7 ("name no country"). It is a part of R2, not a different measurement.
- **R3** is the number of roads that have no node in the graph. The zero-lane roads (10,093 and 8) are not listed as issues, because they have no lanes to export.
- **Why the reports differ.** The Step 6.1 report counted R1. The Step 7 report counted R2 (it took the number from the export, which counts every road the graph could not use). The difference is exactly the other reasons: 470 in ETS2, and 546 + 14 + 13 = 573 in ATS. Both were measured on the same installations; no input data, filter or code differed. The Step 6.1 audit tool was run again for this review and reproduces R0, R1 and the breakdown of R2 above for both games. The R2 counts are the `road_lanes_not_navigable` counts that the Step 7 export wrote and checked for both games. The first version of the summary table of this audit said "14.3 % and 11.3 %" for "roads without a usable traffic side". Those percentages are R1, not R2, and the ATS value came from the previous ATS version (25,307 roads in the Step 4B figures, before the ATS update; the current R1 is 11.2 %). It is corrected above.
- The Step 4B lane model document lists R1 for ATS as 25,307. That is the previous ATS version, as stated there; the current value is 25,280.

- Roads that name no country (R1) are **spread over the whole map**. In every populated area of a 6 × 6 grid, between roughly 2 % and 30 % of the roads have no country. They are not one region. Their country bytes are simply zero. What the game does for them is not known (open question in the [lane model](../formats/scs-lane-model.md#open-questions)).
- Zero-lane roads in ETS2 are dominated by a few road looks (17 looks; one look has 8,960 roads). ATS has 8. They have no effect on connectivity (stage S1b above).
- Prefab slots give independent hints for these roads (section 9): where they can be tested, the road lanes follow the pattern "right side forward" in all 218 ETS2 arms and in 650 of 654 ATS arms. This is an observation, not a rule, and the graph does not use it.

## 9. NV1: along-lane traversal

### Method

A road's lanes sit on its left or right side. The model derives the driving direction of a side from the road's traffic side (known from the country of its nodes). Prefab descriptors give an independent test: the used **input slots** of a prefab node are the lanes that enter the prefab, and the **output slots** those that leave it. For a road attached to a resolved prefab, the numbers of entering and leaving lanes show which lanes of the road run towards the prefab. If the numbers of lanes on the left and right side differ, the test tells which side runs in the stored direction.

The test does not depend on the country bytes or the traffic-side rule. It depends on the prefab node mapping (origin, 98.8 % geometric fit, PJ2) and on the slot counts.

### Results

| Arms where the test discriminates (left and right lane counts differ) | ETS2 | ATS |
|---|---|---|
| Traffic side known | 67,999 | 66,840 |
| Slot counts **agree** with the rule | 67,636 (99.47 %) | 66,396 (99.34 %) |
| Slot counts **contradict** the rule | 13 (0.019 %) | 29 (0.043 %) |
| Neither orientation fits exactly (lane counts differ) | 350 (0.51 %) | 415 (0.62 %) |

- **Left-hand traffic.** The only left-hand country is in ETS2. There, 109 arms discriminate and all agree. The sample is small: 1,716 further left-hand arms (symmetric roads) cannot discriminate.
- **Count mismatches.** In 350 of 350 (ETS2) and 414 of 415 (ATS) of the known-side cases the rule's orientation fits at least as well as the opposite one. These look like lane count changes at the prefab, not contradictions of the rule (PJ8).
- **Roads seen at both ends** of a road with a discriminating orientation: 20,948 (ETS2) and 19,422 (ATS). The two ends disagree on 3 and 4 roads.
- **The contradictions** are in 8 prefab instances of 8 different prefab models (ETS2) and in 25 instances (ATS), where a few prefab models occur repeatedly. Often every road arm of the prefab is affected. For each, the alternatives of the node mapping were tested (all rotations that also fit the node distances). In 2 of 8 (ETS2) and 3 of 25 (ATS) prefabs another mapping removes the contradiction. In the others no alternative does. **Their cause is not understood.**
- **Consistency of the code.** Direction is derived in one place (`RoadLanes::direction`, from the lane side and the traffic side). It is applied identically to both games. No second code path exists.

### Decision

The evidence is strong but not conclusive:

1. It covers only roads attached to prefabs, and only roads whose sides differ in lane count. Symmetric roads cannot discriminate, and roads that touch no prefab are not tested at all.
2. The exceptions (about 0.02–0.04 %) are not explained, and some of them contradict the rule on every arm of a prefab.
3. 14.4 % (ETS2) and 11.5 % (ATS) of the roads have no known traffic side (R2 below), so the rule does not apply to them (LM4, LM5).
4. The rule is a rule of the model, not a stored field.

**`Candidate` remains justified.** What is missing for `Confirmed`: an explanation of the contradicting prefabs, a source that states the direction (or a confirmed meaning of the country bytes, LM4), and a decision about roads without country.

Upgrading this status alone would **not** make confirmed-only routes useful. Road-look lane order is documented, but real lane continuity between road links (LM6) and road-lane-to-prefab-slot correspondence (LM2) remain unconfirmed.

## 10. Root causes

### Confirmed implementation bugs

None. Evidence: node and edge counts agree with the models; all edge endpoints are consistent; every lane has its traversal edge; independent search implementations agree in all cases; no missing edge for a valid pairing was found among the terminal and source nodes.

### Source-data limitations

- **Roads with an unknown traffic side** (R2: 36,418 ETS2, 25,853 ATS; of these 35,948 and 25,280 name no country, R1). They are not in the graph. This is the largest single reason for the small components (section 4, stage S1a).
- Unresolved UIDs at node links: 29 road ends and 12 prefab nodes in ETS2, 8 and 1 in ATS.

### Unconfirmed interpretation of the format

- **Prefab-to-prefab joints** (PJ4). 13,457 joints in ETS2 and 16,162 in ATS. Their slot counts are consistent in both directions for 11,684 (87 %) and 14,185 (88 %) of them. The counts alone do not give a pairing.
- **Lane continuity** (LM6) and road-lane-to-prefab-slot correspondence (LM2, PJ8): candidates at roads and prefab arms; mismatches at 4,400 arm sides.
- The along-lane direction (NV1).

### Expected disconnections

- 20,249 (ETS2) and 13,630 (ATS) road ends and 14,239 and 9,886 road starts lead to nothing.
- 8,374 (ETS2) and 16,945 (ATS) prefab outputs have no neighbour.
- Islands of the topology (22 % of the vertices in ETS2, 14 % in ATS are outside the giant component).

### Not yet understood

- Why 860 (ETS2) and 1,030 (ATS) prefab instances do not fit the origin mapping (PJ2).
- The contradicting prefabs in section 9.
- Why the strong components of ATS are much smaller than its weak ones.

## 11. Recommendations

**Before Step 7:**

1. Nothing in the graph needs fixing. No code change is proposed.
2. Decide how Step 7 deals with the small reach: it is a property of the current data. A route planner built on these components will find routes inside pieces of 4 to 23,000 lane nodes in candidate mode, and none across roads in confirmed-only mode. Step 7 should not assume more.

**Research that would have the most effect, in this order:**

1. **Country-less roads** (LM4): find out how the game decides the traffic side of roads whose nodes name no country. Largest effect on the number of connected pieces.
2. **Prefab joints** (PJ4): find out whether the slots of two joined prefabs can be paired from the data (for example by the position of the slot curves at the shared node, compared in the local frame of the node). This would be a research test, not snapping in the graph, and it needs an approval before any edge is changed. 87 % of the joints have consistent slot counts.
3. **Road lane to prefab slot mapping** (LM2) and actual lane continuity (LM6): needed for confirmed connections through the network.
4. **Contradicting prefabs** (NV1).

**Can safely remain open:** dead ends and open prefab nodes, zero-lane roads, unresolved UIDs (a few dozen), the lane count mismatches at prefab arms (about 3 % of arm sides), the 860 and 1,030 unresolved prefab instances.
