# SCS lane model

Status: **implemented** (Phase 1, Step 4B):

- `crates/roadpilot-map-formats`: text SII reader and definition records (`sii`, `definitions`),
- `crates/roadpilot-map-model`: lane types (`lanes`),
- `crates/roadpilot-map`: `LaneDefinitions` and `build_lane_network`.

It builds on [SCS road geometry](scs-road-geometry.md) and [SCS sector format](scs-sector-format.md). Lane geometry and traffic rules are not part of this step. The navigation graph that uses the lanes is described in [Navigation graph](../navigation/navigation-graph.md). Junctions (prefabs) and lane movements through them are described in [SCS prefabs and junctions](scs-prefab-junctions.md) (Step 5).

ATS was updated by the game launcher after this step. The ATS figures below come from the previous version; the current version differs slightly (for example 224,868 roads and 212 road looks).

Evidence levels: **Confirmed** holds for all inspected data. **Measured** gives a stated share. **Inferred** follows from confirmed facts but was not tested on its own. **Unknown** is not established.

Data basis: every road of a local ETS2 (252,127 roads) and ATS installation (224,870 roads), read read-only, and every `road_look*`, `traffic_lane*` and `def/country.sii` definition of the base game and the installed DLCs. The analysis programs are throwaway tools outside the repository. The opt-in test `builds_the_lane_network_of_the_installation` in `crates/roadpilot-map/tests/real_installation.rs` reproduces the statistics below.

## Definitions used

All four kinds are text SII files (see the reader below). No other definition is read.

| Unit class | Files | ETS2 / ATS units | Used for |
|---|---|---|---|
| `road_look` | `def/world/road_look*.sii` (base game and DLCs) | 374 / 215 | Lane lists, offsets, side sizes, `lane_direction` |
| `road_template_variant` | `def/world/road_look.template*.sii` | 362 / 172 | Replacement lane lists |
| `traffic_lane_data` | `def/world/traffic_lane.sii` | 41 / 34 | Lane types |
| `country_data` | `def/country.sii` with its `.sui` includes | 35 / 21 | Country ID and `leftside_traffic` |

No unit name occurs twice. The model rejects duplicates instead of choosing one.

## Road look to road

Every road record stores its road look as a token (payload offset 4). It resolves to `road.<token>` for **100 %** of the roads in both games (confirmed).

### Lanes per side

A road look lists lane types per side: `lanes_left[]` and `lanes_right[]`, each entry naming a `traffic_lane_data` unit. Every lane type reference resolves (ETS2 495,080, ATS 470,080). **Left** and **right** refer to the road's stored direction, from start node to end node.

Lane counts (left, right) over all roads:

| Configuration | ETS2 roads | ATS roads |
|---|---|---|
| (1, 1) | 143,209 | 119,258 |
| (0, 1) | 38,027 | 38,876 |
| (0, 2) | 22,927 | 34,214 |
| (2, 2) | 18,672 | 16,206 |
| (0, 0), no lanes | 15,126 | 656 |
| (0, 3) | 9,225 | 6,119 |
| (3, 3) | 2,119 | 608 |
| (3, 2), (2, 3), (2, 1), (1, 2) | 1,035 | 8,211 |
| (0, 4), (0, 5) | 1,470 | 626 |
| Lanes on the left only: (1, 0), (3, 0) | 242 | 4 |
| Others | 75 | 92 |

### Variants

Template looks can reference `road_template_variant` units (`template_variants_left[]`, `template_variants_right[]`, `lane_variants[]`). A variant can replace the lane list of the sides it sets.

- Confirmed: no variant changes the **number** of lanes on a side it sets, in either game. Lane counts and lane sides are therefore known for every road.
- Confirmed: variants do change lane **types**. This affects the looks of 131,821 ETS2 roads (52 %) and 157,697 ATS roads (70 %).
- Unknown: **which variant a road uses.** Road payload offsets 28 and 36 hold tokens that sometimes equal a variant name of the look, but often they do not, even for looks without variants (open question LM1). The model therefore marks the types of such sides as `LaneType::Uncertain`, carrying the look's own type.

### Lane order

Lanes keep the order of the definition list, with index 0 for the first entry. The official [ETS2 1.19 modding guide](https://modding.scssoft.com/wiki/Games/ETS2/Modding_guides/1.19) specifies that `lanes_left` and `lanes_right` entries are ordered from the road centre towards the borders. Thus index 0 is the innermost lane on either side. The ATS guide uses the same fields, but does not state their order explicitly; applying this interpretation to ATS is inferred from the shared definition format. This resolves the road-look lane order, but does not establish lane widths, lane positions, or lane continuity across junctions.

### Widths and offsets

| Value | Finding | Model |
|---|---|---|
| Per-lane width | **Not stored** anywhere in the look or lane type definitions | `Lane::width` is always `None`. No default width is assumed. |
| `road_size_left`, `road_size_right` | Stored only in the 32 (ETS2) and 34 (ATS) non-template looks; presumably the width of each side. Not confirmed. | Kept as `RoadLanes::stored_size_*`, not interpreted |
| `lane_offsets_left[]`, `lane_offsets_right[]` | One pair of numbers per entry, on 270 (ETS2) and 117 (ATS) look sides. The meaning of the two numbers is **unknown** (LM3). | Kept as `StoredLaneOffset { first, second }`, assigned in order only when the count equals the lane count |
| Offset count ≠ lane count | 23 (ETS2) and 1,204 (ATS) road sides | Not assigned (`OffsetsState::CountMismatch`) |
| `road_offset` | Stored in some looks, meaning unknown | Not used |

Template looks take their geometry from a 3D model template (`template_right`, a `.pmd` file with a `.pmg` mesh). Step 9 studied these: [SCS road templates](scs-road-templates.md). The meshes can be read, but they do not give a reliable lane width or axis offset, so both stay unknown.

### Lane geometry (Step 8)

Lane positions are **unavailable**. A lane centre line needs a lane width and the position of the road axis relative to the node curve; neither is known (LM3), and the curve itself is only estimated (G1). Therefore:

- The map package marks every lane `geometry: unavailable` and `width: null` ([Map package](map-package.md)).
- `roadpilot_map_geometry::lane_centerline` exists for callers who supply all assumptions explicitly (`LaneAssumptions`: width, order, axis offset). There are no defaults. Its result is `estimated`, never `confirmed`, and `Quality::is_suitable_for_active_steering` is false for it.
- The stored offsets are kept as they are (`StoredLaneOffset`). They are **not** used to compute positions, because their meaning is unknown.

A lane departure warning still needs reliable width and position evidence. Road-look templates reference `.pmd` descriptors, while the model geometry is stored in paired `.pmg` files. Step 9 reads the `.pmg` vertex positions (not the `.pmd`) and finds a plausible 4.5-unit lane pitch for part of the roads, which is a derived, not a confirmed value ([SCS road templates](scs-road-templates.md)). Widths and the road-axis offset must still be established. Even with those values, the estimated road curve is not yet suitable evidence for active steering.

## Driving direction

### Traffic side per road

Each node stores **country IDs** in its flags (bits 8–15 and 16–23). This was confirmed in this step:

- every non-zero value is a `country_id` from `def/country.sii`, except 48 ATS nodes with an ID that has no definition;
- for each country, the median position of its nodes lies within 15,000 units of the country position in the definitions (34 of 35 ETS2 countries, the remaining one at 17,000; 21 of 21 ATS countries);
- the two bytes differ only at about 300 border roads per game. Which byte belongs to which side of the node is **unknown** (LM4).

The traffic side of a road follows from the country of its two nodes and the country's `leftside_traffic` flag. ETS2 has one left-hand country, ATS none. There is no global assumption per game.

| Road traffic side | ETS2 roads | ATS roads |
|---|---|---|
| Right | 212,338 | 198,990 |
| Left | 3,371 | 0 |
| Unknown: nodes name no country | 35,948 | 25,307 |
| Unknown: nodes name different countries | 470 | 546 |
| Unknown: country not defined | 0 | 13 |
| Unknown: look contradicts the country | 0 | 14 |

`lane_direction` (`right_hand` or `left_hand`) is set in 13 ETS2 and 2 ATS looks. In ETS2 those looks are used only in right-hand countries, so they agree. In ATS, the two `left_hand` looks are used by 14 roads in right-hand countries. What `lane_direction` does there is **unknown** (LM5), so those roads get no traffic side (`LookConflict`) rather than a guess.

### Lane direction

| Rule | Evidence |
|---|---|
| With right-hand traffic, lanes on the **right** side run in the stored direction (start to end node), lanes on the **left** side against it. | Measured on divided carriageways. For one-way roads with lanes only on the right, the nearest oncoming one-way road lies on the **left** of the stored direction in 98.4 % (ETS2, 22,792 pairs) and 97.9 % (ATS, 26,584 pairs) of cases. That is right-hand traffic travelling in the stored direction. |
| With left-hand traffic the sides are mirrored: **left** lanes run in the stored direction. | Measured on a small sample. The left-only roads use the UK one-way highway and exit road looks; all 17 matched pairs have the oncoming road on the **right**. No right-only one-way road lies in the left-hand country. |
| Two-way roads follow the same rule on both sides. | Inferred from the side rule above. Supported in Step 5: at prefab nodes, the road lanes in each direction match the prefab's input and output slots in 98.6 % of cases ([SCS prefabs and junctions](scs-prefab-junctions.md#road-lanes-at-a-prefab)). |
| Without a known traffic side, the direction is unknown. | Model rule |

The stored road direction is not a driving direction. A road with lanes on both sides is driven both ways, and a one-way road is driven in the direction its lanes give.

## Lane transitions between roads

- **Confirmed:** road records store no lane-to-lane connections. Junction lanes are described in prefab descriptors (`.ppd`), which are read since Step 5 ([SCS prefabs and junctions](scs-prefab-junctions.md)).
- **The model** pairs lanes across a road-to-road connection (end node of one road = start node of the next) only as **candidates by index**, separately per direction. This requires that both roads have resolved lanes, the same known traffic side and the same number of lanes in that direction. Otherwise the transition is `Unknown`, with the reason.
- Whether index-paired lanes are really connected in the game is **unknown** (LM6), because the index order itself is not confirmed. Candidates never connect lanes of opposite directions.

| Road-to-road connections | ETS2 | ATS |
|---|---|---|
| Lane candidates | 122,946 (81.3 %) | 110,827 (84.9 %) |
| Unknown: traffic side unknown | 28,279 | 19,732 |
| Unknown: forward lane count differs | 31 | 40 |
| Unknown: backward lane count differs | 2 | 10 |

The distinction the model keeps:

1. A **road connection** is a shared node, in stored direction (`RoadNetwork::connections`).
2. A **lane transition** is a candidate pairing of lanes (`LaneNetwork::transition`).
3. **Whether a vehicle can actually drive** from one lane to the next is not modelled. Movements through junctions are modelled in Step 5 (internal movements confirmed, road lane pairing as candidates); traffic rules are referenced but not interpreted.
4. The **driving direction** of each lane comes from the traffic side (above).

## Model

| Type | Purpose |
|---|---|
| `RoadSide` | Left or right of the stored direction |
| `TrafficSide`, `TrafficSideState`, `TrafficSideIssue` | Traffic side per road, or why it is unknown |
| `LaneDirection` | Forward (start to end) or backward |
| `LaneId` | Road, side and index |
| `Lane` | ID, `LaneType` (`Known` or `Uncertain`), optional `StoredLaneOffset`, `width` (always `None` today) |
| `RoadLanes` | Road, look name, traffic side, `LaneConfig` (`Resolved` or `Unresolved` with `LaneIssue`), offset states, stored side sizes; `direction(side)`, `lanes_in(direction)` |
| `LaneNetwork` | Validated lanes of every road of a `RoadNetwork`; `transition(connection)` |
| `LaneTransition` | `IndexCandidates(Vec<LaneConnection>)` or `Unknown(TransitionIssue)` |
| `LaneError` | Unknown or duplicate road, missing road, lane of another road, duplicate lane ID |

`roadpilot_map::LaneDefinitions::from_units` collects the four unit classes. It rejects duplicates, unreadable values and unknown `lane_direction` values. `roadpilot_map::build_lane_network` then builds the `LaneNetwork`; it needs the parsed sectors for the node country IDs.

## Text SII reader

`roadpilot_map_formats::sii::parse_sii` reads text SII:

- units, `key: value` attributes, `key[]` appends and `key[i]` entries (a preceding `key: N` size declaration is dropped),
- `#`, `//` and `/* */` comments outside strings,
- `@include`, resolved through a caller function: iteratively, at most 8 levels deep, and at most 64 MiB in total.

Binary or encrypted SII variants are rejected; none occur in these files. All 12 (ETS2) and 5 (ATS) definition files parse without errors.

## ETS2 and ATS

The structure is identical. Differences are in the data only: ETS2 has one left-hand country (with 3,371 roads) and many more roads without lanes. ATS uses variants more often and has the two `left_hand` looks.

## Open questions

| ID | Question |
|---|---|
| LM1 | How a road selects a variant of its look (road payload offsets 28 and 36 are candidates). Needed for exact lane types. |
| LM2 | Whether prefab slot indices correspond to road lane indices when a road meets a prefab. Road-look lane list ordering is documented for ETS2; its application to ATS follows the shared format and is inferred. |
| LM3 | Meaning of the `lane_offsets` pairs and of `road_offset`; lane widths of template looks. Step 9: the template meshes (`.pmg`, referenced through `.pmd`) are readable and show break lines at multiples of 4.5 for part of the roads, but no lane semantics. Step 10 decoded the `.pmd` descriptors and `.mat` materials: material names are hashes, line decals can be found by effect and texture name, and lane surface and shoulder often share one material, so no lane width follows either (RT1–RT7 in [SCS road templates](scs-road-templates.md)). |
| LM4 | Which of the two node country bytes belongs to which side of the node. |
| LM5 | The effect of a look's `lane_direction` when it contradicts the country. |
| LM6 | Real lane continuity across road connections (lane drops and merges). Junction lanes are read from `.ppd` since Step 5; lane continuity at prefab arms with a lane count mismatch is PJ8. |
| — | Roads whose nodes name no country (R1: 14.3 % in ETS2; 11.3 % in ATS in the previous version, 11.2 % now): which traffic side applies there. With the roads whose nodes name different countries and a few ATS exceptions, 36,418 ETS2 and 25,853 ATS roads have no known traffic side (R2, see [the road counts](../navigation/connectivity-audit.md#road-counts-and-their-definitions)). They are spread over the whole map and are the largest single reason for the small connected pieces of the navigation graph ([connectivity audit](../navigation/connectivity-audit.md), NV7). |

Geometry questions (G1 and following) are in [SCS road geometry](scs-road-geometry.md).
