# SCS map data inventory

Status: **Phase 1, Step 1 research.** This document lists the smallest set of ATS and ETS2 game data that RoadPilot needs, and how well each part is understood. It describes findings only. Nothing here is implemented yet.

Confidence levels used throughout:

- **Verified:** confirmed by inspecting archive indexes, file encodings or cross-file references on a local installation, or by validated earlier research.
- **Partial:** some evidence exists, but important details are still unknown.
- **Unknown:** not established yet.

## 1. Research scope

RoadPilot-Map is not a general archive extractor. It only needs map data that serves these RoadPilot features:

1. Lane keeping: lane geometry, direction, curvature, connections, width.
2. Road network: segments, junctions, connectivity, positions, data for route calculation.
3. Traffic lights and intersections: static positions and their link to lanes.
4. Speed information: speed limits and the road attributes they depend on.
5. Other driving information: traffic signs, restrictions, turn rules.

Route calculation itself stays in the RoadPilot application. RoadPilot-Map only provides a routable network (see the non-goals in the [architecture overview](../architecture/overview.md)).

### Method

- Read-only inspection of a local ETS2 and ATS installation (Steam, Windows). The installed game versions were not recorded.
- The installation was inspected with a throwaway probe outside both repositories. It used the existing, validated `scs-hashfs` reader of SCS-Extractor as a path dependency, so no archive code was rewritten.
- Steps, in order: (1) list entry names and sizes of all archives, (2) classify the encoding of selected files from their first bytes (text SII, `BSII`, `3nK`, `ScsC`, or the leading 32-bit number for binary files), (3) collect unit class names and attribute key names from selected text definitions, (4) check that references between definition files resolve. Results were recorded as counts only.
- Nothing was extracted to disk, and no game file was modified. No game content, excerpts or byte dumps are in this document. Only paths, extensions, counts and our own descriptions are.
- Information about binary layouts that comes from community tools, not from our own inspection, is marked **Partial** at most. Those sources still need a license review before use (open question L3).

## 2. Reused SCS-Extractor findings

Source: `C:\Projekte\SCS-Extractor` (read-only; commit `52c51b6`), mainly `docs/FORMAT_RESEARCH.md`, `docs/FORMAT_DETECTION.md` and `crates/scs-hashfs`.

| Finding | Status there | Relevance for RoadPilot-Map |
|---|---|---|
| Path hashing is CityHash64 v1.0.3, paths without leading slash, exact UTF-8 bytes | Verified on 477,121 real paths, 0 mismatches | Required to look up entries by path. |
| Format detection by content: HashFS v1, HashFS v2, ZIP (207 local archives: 191 v2, 9 v1, 7 ZIP) | Verified | All official ETS2/ATS archives are HashFS v2. v1 and ZIP were only seen in mods. |
| HashFS v2 layout: zlib entry and metadata tables, plain files stored or zlib-compressed, binary directory listings | Verified on 1,111,851 entries of 191 archives | The only archive format needed for official map data. |
| HashFS v1 reader validated on 185,256 entries (18.32 GB) | Verified | Mods only. Not needed initially. |
| Texture objects (`.tobj`, GDeflate) | Not reconstructed (open item R6) | Not needed. |
| Locked ZIP variants, non-zero salt, v2 field 44, flag bit 2 | Open (R2, R4, R7) | Does not affect plain files in official archives. |
| Text SII starts with `SiiNunit`; `BSII`, `3nK` and `ScsC` variants are known from the community | Partial (R8) | See section 3.4. |

Independent re-check during this step: listing all official archives with `scs-hashfs` gave 1,111,829 named entries. This agrees with SCS-Extractor's figure (1,111,831; the difference comes from different seed names). `base_map.scs`, `def.scs` and every map DLC archive had **zero unresolved entries**. Only `core.scs`, `base_cfg.scs`, `base.scs` (1) and `base_share.scs` (2) contain unlisted entries.

## 3. Relevant map file categories

### 3.1 Archives that contain map data

| Archive | Map-relevant content | Confidence |
|---|---|---|
| `base_map.scs` | Map descriptor (`map/europe.mbd` or `map/usa.mbd`), `map/<map>.external.sii`, sector files of the base map. This archive was not in SCS-Extractor's sample table. | Verified |
| `def.scs` | All base game definitions under `def/` (road looks, lanes, traffic rules, prefab models, semaphores, signs, countries, cities). | Verified |
| `base.scs` | Prefab descriptors `.ppd` of the base game (ETS2 5,563, ATS 4,478), plus models and textures that are not needed. | Verified |
| `base_share.scs` | 4 `.ppd` files per game, plus shared assets. | Verified |
| Map DLC archives (`dlc_<region>.scs`) | Additional sectors in the same `map/<map>/` folder, DLC definitions (`def/world/<type>.<dlc>.sii`, `def/country/...`), DLC prefab descriptors. | Verified |
| ETS2 map DLCs found | `dlc_balkan_e`, `dlc_balkan_w`, `dlc_balt`, `dlc_east`, `dlc_fr`, `dlc_greece`, `dlc_iberia`, `dlc_it`, `dlc_north`, `dlc_polar` (819 sectors) | Verified |
| ATS map DLCs found | 20 state DLCs (`dlc_ar` ... `dlc_wy`, including `dlc_arizona`, `dlc_nevada`) (698 sectors) | Verified |
| `dlc_oversize.scs` | Only `map/scenes*` (special scenes), no regular sectors. | Verified |

All other archives (vehicles, trucks, trailers, cosmetic DLCs, flags, effects, locale, sound) contain no map data that RoadPilot needs.

### 3.2 Map sector files

Path pattern: `map/<map>/sec<±XXXX><±YYYY>.<ext>`, where `<map>` is `europe` (ETS2) or `usa` (ATS). Every sector has five files.

| Extension | Count (ETS2 / ATS) | Total size, uncompressed (ETS2 / ATS) | Leading u32 | Likely content | Class | Confidence |
|---|---|---|---|---|---|---|
| `.base` | 1,101 / 925 | 216 MB / 231 MB | 907 in all files | Core map items (roads, prefabs, nodes and other items). Exact split between files unknown. | **A** | Encoding Verified, content Partial |
| `.data` | 1,101 / 925 | 503 MB / 660 MB | 907 in all files | Item payloads that do not fit the base records (believed to include point and curve data). | **A** (until proven unneeded) | Partial |
| `.aux` | 1,101 / 925 | 1,755 MB / 2,151 MB | 907 in all files | Auxiliary items. May hold signs and other objects relevant for traffic rules. Largest file type. | **B** | Unknown |
| `.desc` | 1,101 / 925 | < 1 MB | 3 | Sector description, very small. | **B** | Unknown |
| `.snd` | 1,101 / 925 | 4 MB / 3 MB | 901, 903 or 907 | Sound items. | **C** | Partial |

Other files under `map/`:

| Path | Purpose | Class | Confidence |
|---|---|---|---|
| `map/<map>.mbd` | Map descriptor (binary, leading u32 907). Only one per game, in `base_map.scs`. Map DLCs add sectors to the same map without their own descriptor. | **A** | Partial |
| `map/<map>.external.sii` | Text SII with one `external_map_data` unit (map root path, UI camera limits). | **C** | Verified |
| `map/academy/*`, `map/halloween_*/*` | Separate small maps (driving academy, event map). | **C** | Verified |
| `map/scenes/*.sbd`, `map/academy/scenes/*.sbd`, `map/scenes_source/*` | Scene files for menus, academy and oversize scenarios. | **C** | Verified |

The same leading value (907) in all `.base`, `.data` and `.aux` files of both games and all DLCs strongly suggests a shared map format version. This is **Partial**: the meaning of the field is not confirmed.

No sector file name appears in more than one archive: base and DLC sectors **add to** each other at file level. Whether DLC items replace base items within sectors is unknown (T4).

### 3.3 Prefab descriptors

| Item | Finding | Class | Confidence |
|---|---|---|---|
| `.ppd` files | ETS2 about 7,800, ATS about 4,600, mostly under `prefab/` and `prefab2/` in `base.scs` and the map DLCs. Total about 34 MB (ETS2) and 15 MB (ATS). | **A** | Verified (location, count) |
| Encoding | Binary. Leading u32 is 25 in every inspected `.ppd` (base, DLC and `road_template/`). | | Verified (value), Partial (meaning: likely version) |
| Content | Believed to hold the prefab's control nodes, internal lane curves (paths for driving through junctions), semaphore positions and map points. Not inspected beyond the leading word. | | Partial (community knowledge) |
| Reference | `prefab_model` units in `def/world/prefab*.sii` reference their `.ppd` path. 4,242 of 4,246 (ETS2) and 1,957 of 1,961 (ATS) references point to an existing entry. | | Verified |
| `.pmd`, `.pmg`, `.pma`, `.pmc`, `.tobj` next to them | 3D model, geometry, animation, collision and texture data. ETS2 `.pmg` alone is about 3.1 GB. | **C** | Verified (by extension and metadata) |

### 3.4 Definition files (`def/`)

All inspected files under `def/world/`, `def/country/`, `def/city/` and `def/*.sii` are **text SII**. Some start with a UTF-8 byte order mark, and `.sui` files are text fragments loaded through `@include`. No `BSII`, `3nK` or `ScsC` encoding was found in these files. **Verified.** (SCS-Extractor found `3nK` only in `base_cfg.scs`, which is not needed.)

DLC and add-on definitions use the pattern `def/world/<type>.<suffix>.sii` (for example `prefab.dlc_iberia.sii`, and in `def.scs` also `.mod_*` suffixes). The base files contain no `@include` for them, so the game most likely loads every file that matches the type name. **Partial** (inferred, not confirmed).

| File(s) | Unit class(es) | Information available (by attribute names) | Needed for | Class | Confidence |
|---|---|---|---|---|---|
| `def/world/road_look*.sii` (incl. `road_look.template*.sii`) | `road_look`, `road_template_variant` | Lane count per side, lane types per side, road offset, shoulder sizes, line offsets and widths, lane offsets and lane direction (template looks), travel-time factor | Lane geometry, lane direction, speed (via lane type), routing cost | **A** | Verified (attributes), Partial (semantics) |
| `def/world/traffic_lane.sii` | `traffic_lane_data` | Lane rank, speed class and traffic rules per lane type | Speed limits, lane restrictions | **A** | Verified |
| `def/country/<country>/speed_limits.sii` | `country_speed_limit` | Limit per lane speed class and vehicle speed class, with urban and maximum limits | Speed limits | **A** | Verified |
| `def/country.sii` (+ `.sui` includes) | `country_data` | Country ID and code, position, left-side traffic flag, imperial units flag | Lane direction (left-hand traffic), speed units, region association | **A** | Verified |
| `def/world/prefab*.sii` | `prefab_model` | `.ppd` reference, category, semaphore profile, traffic rules for input and output, "GPS avoid" and travel-time attributes | Junctions, traffic lights, routing | **A** | Verified |
| `def/world/traffic_rules.sii` | `traffic_rule_data` | Named rules with parameters | Restrictions, speed rules from signs and prefabs | **A** | Verified (structure), Partial (rule semantics) |
| `def/world/semaphore_profile*.sii` | `tr_semaphore_profile` | Profile type, cycle and interval timing, semaphore model | Traffic light association, static timing | **B** | Verified (structure) |
| `def/world/semaphore_model*.sii` | `tr_semaphore_model`, `semaphore_model` | Model and animation references only | — | **C** | Verified |
| `def/world/sign*.sii` | `sign_model` | Sign category and an optional traffic rule (ETS2: 1,243 of 8,963 sign models, ATS: 110 of 3,609) | Speed limits and restrictions from signs | **B** | Verified (structure) |
| `def/world/road.sii` | `road_def` | Road line widths, boundary distance | Possibly lane marking geometry | **B** | Verified (structure) |
| `def/map_data.sii` | `map_data` | Navigation penalties for cities and slow roads, plus UI colors | Optional routing cost tuning | **B** | Verified (structure) |
| `def/world/trajectory_rules.sii` | `trajectory_rule`, `trajectory_rule_data` | Named trajectory rules with parameters | Possibly AI lane behavior | **B** | Unknown (semantics) |
| `def/city*.sii`, `def/city/*.sui` | city units | City definitions | Optional urban context for speed limits | **B** | Partial |
| `def/sign/**` (template, board, atlas, frame) | — | Visual layout of sign boards (about 26,600 files in ETS2) | — | **C** | Verified (by path) |
| Other `def/world/*` (model, curve_model, mover, vegetation, terrain, building, railing, sound, prefab_corner, road_edge, road_material, sidewalk) | — | Scenery and visual data | — | **C** | Verified (by path) |
| `def/vehicle/`, `def/company/`, `def/cargo/`, `def/ferry/`, UI files | — | Economy, vehicles, UI | — | **C** | Verified (by path) |

Note on ferries: `def/ferry/` holds ferry connections. They are only needed if RoadPilot ever routes across ferries (class C for now).

## 4. Minimal data requirement matrix

| RoadPilot requirement | Source file/category | Information available | Priority | Confidence |
|---|---|---|---|---|
| Road geometry (centerline) | Sector `.base` (+ `.data`): road items and their nodes | Node positions and directions, believed to define a curve between two nodes | A | Partial |
| Lane geometry and positions | `road_look` + road items; prefab lane curves in `.ppd` | Lane count, lane types and offsets per side, road offset; lanes through junctions from `.ppd` | A | Partial |
| Lane direction | `road_look` (sides, lane direction), `country_data` (left-side traffic) | Left and right lanes per side; the left-side traffic attribute is set in one ETS2 country definition and in no ATS one | A | Partial |
| Road curvature | Derived from road curves | Not stored; computed from node positions and directions | A | Partial (depends on T2) |
| Lane width | `road_look` sizes and offsets, `road_def` | No explicit lane width attribute found; must be derived from offsets or sizes | A | Unknown |
| Lane connections and transitions | Road nodes (sector items) + `.ppd` lane curves | Connectivity believed to be via shared node IDs; lane-level links inside prefabs | A | Partial |
| Junctions | Prefab items (sectors) + `prefab_model` + `.ppd` | Junction placement in sectors, junction layout in `.ppd` | A | Partial |
| Route network | Road and prefab items, nodes | Graph built from road segments and prefabs connected by nodes | A | Partial |
| Routing cost attributes | `road_look`, `prefab_model`, `map_data` | Travel-time factors, "GPS avoid", navigation penalties | B | Verified (attributes), Partial (semantics) |
| Positions and coordinates | Sector items, `.mbd` | World positions per node; units and scale not yet confirmed | A | Partial (T3) |
| Traffic light positions | `.ppd` semaphore entries; `prefab_model` semaphore profile | Static positions within prefabs; profile per prefab | B | Partial |
| Traffic light to lane association | `.ppd` (semaphore and lane curve links) | Believed to link curves to semaphores; not yet inspected | B | Unknown |
| Traffic light timing (static) | `tr_semaphore_profile` | Type, cycle and interval values | B | Verified (attributes) |
| Speed limits | `road_look` lanes → `traffic_lane_data` speed class → `country_speed_limit` | Per lane type and vehicle class; urban and maximum limits | A | Verified (reference chain), Partial (urban rule, local overrides) |
| Road categories | `traffic_lane_data` (rank, speed class), `prefab_model` category | Lane rank and speed class | A | Verified (attributes) |
| Road signs and restrictions | Sign items (sector file unknown, possibly `.aux`) + `sign_model` traffic rule + `traffic_rule_data` | Signs that carry a rule (e.g. speed or prohibition) | B | Partial (definitions), Unknown (sector records) |
| Turn restrictions | `prefab_model` traffic rules, `.ppd` curve flags | Rule references on prefab inputs and outputs | B | Partial |

Verified reference checks (counts from the local installation):

| Reference | ETS2 | ATS |
|---|---|---|
| `road_look` lanes → `traffic_lane_data` unit | 1,083 / 1,083 | 725 / 725 |
| `traffic_lane_data` traffic rules → `traffic_rule_data` unit | 203 / 203 | 152 / 152 |
| `country_speed_limit` lane speed class → speed class used by a lane type | 405 / 427 | 273 / 273 |
| `prefab_model` → existing `.ppd` entry | 4,242 / 4,246 | 1,957 / 1,961 |
| `sign_model` traffic rule → `traffic_rule_data` unit | 1,243 / 1,243 | 110 / 110 |
| `prefab_model` traffic rules input → second name part of a `traffic_rule_data` unit | 174 / 174 | 111 / 111 |
| `prefab_model` semaphore profile → second name part of a `tr_semaphore_profile` unit | 303 / 446 | 828 / 830 |

The last two references use only one part of a dotted unit name, not the full name. Several profiles share that part (up to 22 variants in ETS2), probably one per region or country. The selection rule is not known yet.

## 5. Dependencies between file types

```
 map/<map>.mbd                       (one per game, base_map.scs)
      │
      ▼
 map/<map>/sec*.base/.data/.aux      (base_map.scs + map DLCs)
      │  road items ──────────────► def/world/road_look*.sii ──► def/world/traffic_lane.sii
      │                                                               │        │
      │                                                               │        └─► def/world/traffic_rules.sii
      │                                                               ▼
      │                                         def/country/<c>/speed_limits.sii (by speed class)
      │
      │  prefab items ────────────► def/world/prefab*.sii ──► prefab*/**/*.ppd  (base.scs + DLCs)
      │                                     │   └─► semaphore_profile*.sii (name part + variant)
      │                                     └─► traffic_rules.sii (name part)
      │
      │  sign items (file unknown) ► def/world/sign*.sii ──► traffic_rules.sii
      │
      └─ position ────────────────► def/country.sii (country: left-side traffic, units)
```

How sector items reference definitions (by name, by hashed token or by index) is **Unknown**. It must be established when the sector format is researched.

## 6. Static versus runtime information

| Information | Static map data | Runtime (game integration) |
|---|---|---|
| Road and lane geometry, connectivity, junction layout | Yes | — |
| Lane direction, left-hand traffic | Yes | — |
| Legal speed limits (per lane type and country) | Yes | — |
| Speed limit currently in force (including signs and local rules) | Partly (signs, rules) | The telemetry SDK reports a speed limit for the truck's current road. It is the better source while driving. |
| Truck position, heading and speed | — | Yes (telemetry) |
| Traffic light positions and static timing profile | Yes (Partial) | — |
| Current traffic light state (red, amber, green) | No | Runtime only. Not known to be in the telemetry SDK; may need visual recognition. Out of scope for RoadPilot-Map. |
| AI traffic, other vehicles, obstacles | No | Runtime only |
| Weather, time of day | No | Runtime only |
| Road works, events, detours | Partly (definitions exist under `def/world/journey_*`) | Activation is runtime |
| Navigation route chosen by the player in the game | No | Runtime only |

## 7. Required, conditional and unnecessary data

**A – Required (initial map engine)**

- Archives: `base_map.scs`, `def.scs`, `base.scs` (only `.ppd`), `base_share.scs` (only `.ppd`), installed map DLC archives (sectors, definitions, `.ppd`).
- `map/<map>.mbd`, sector `.base` and `.data`.
- `.ppd` prefab descriptors that a used `prefab_model` references.
- `def/world/road_look*.sii`, `def/world/traffic_lane.sii`, `def/world/prefab*.sii`, `def/world/traffic_rules.sii`, `def/country.sii` with its `.sui` includes, `def/country/<c>/speed_limits.sii`.

**B – Conditional**

- Sector `.aux` – only if signs or traffic lights that RoadPilot needs are stored there.
- Sector `.desc` – only if it turns out to be needed to place or interpret a sector.
- `def/world/semaphore_profile*.sii` – for the traffic light feature.
- `def/world/sign*.sii` – for sign-based speed limits and restrictions.
- `def/world/road.sii` – if lane marking geometry is needed.
- `def/map_data.sii` – for routing cost tuning.
- `def/world/trajectory_rules.sii`, `def/city*` – once their effect on lanes or speed is confirmed.
- `def/ferry/` – only if routing across ferries is required.

**C – Not required**

- All models, textures and materials (`.pmd`, `.pmg`, `.pma`, `.pmc`, `.tobj`, `.dds`, `.mat`), including those under `prefab*/` and `road_template/`.
- Sector `.snd`, scene files (`.sbd`), `map/academy`, event maps, `map/<map>.external.sii`.
- `def/sign/**` board layouts, semaphore models, scenery definitions, vehicles, companies, cargo, economy, UI, locale, sound, effects.
- Archives without map content: vehicle, truck, trailer, paint, flag and other cosmetic DLCs, `effect.scs`, `locale.scs`, `core.scs`, `base_cfg.scs`, `base_vehicle.scs`, `version.scs` (except, possibly, for reading the game version during discovery).

By size, the required data is a small part of the installation: about 0.7 GB (ETS2) or 0.9 GB (ATS) of uncompressed sector data, under 50 MB of `.ppd` files, and a few MB of text definitions. The `.aux` files (1.8 to 2.2 GB) are excluded unless needed.

## 8. Unresolved technical questions

| ID | Question | Blocks |
|---|---|---|
| M1 | Binary layout of sector files with leading value 907: header, item record types, and which item types are in `.base`, `.data` and `.aux`. | Parsing roads, prefabs, nodes, signs |
| M2 | Binary layout of `.ppd` with leading value 25: nodes, lane curves, semaphores, curve-to-semaphore links. | Junctions, traffic lights |
| M3 | Road curve representation between two nodes (T2), and how lane positions are derived from `road_look` offsets. | Lane geometry, curvature |
| M4 | Source of lane width: fixed value, derived from offsets and road size, or from template variants. | Lane width |
| M5 | How sector items reference definitions (name, hashed token, or index). | Linking map items to definitions |
| M6 | How the semaphore profile variant and the traffic rule are chosen when a prefab uses only part of a unit name. | Traffic lights, turn rules |
| M7 | Loading order of `def/world/<type>.<suffix>.sii` files and unit-level overrides by DLCs (T4). | Correct definitions with DLCs |
| M8 | When the urban speed limit applies (city area, road attribute or sign), and how sign rules override the country limit. | Speed limits |
| M9 | Meaning of the `.mbd` and `.desc` contents, and whether they are needed. | Discovery, sector handling |
| M10 | World units, axis orientation and scale (T3). | All geometry |
| M11 | Whether sign items are needed from `.aux`, given that telemetry already reports a speed limit at runtime. | Scope of `.aux` |
| M12 | 4 `prefab_model` references per game point to no listed `.ppd`. Unused definitions, or entries in another archive? | Completeness check |
| M13 | Community documentation of sector and `.ppd` layouts exists. Its license must be reviewed (L3) before it is used as a reference. | M1, M2 |

The installed game versions were not recorded during this step. Supported versions are tracked as T8.

## 9. Recommended minimal extraction scope

The smallest practical set of capabilities for the next steps:

1. **Installation discovery.** Find the game folder, detect ATS or ETS2, and list the `.scs` archives. Select only `base_map.scs`, `def.scs`, `base.scs`, `base_share.scs` and the installed map DLC archives (those with a `map/` folder).
2. **HashFS v2 index reading.** Read the header, entry table, metadata table and directory listings of the selected archives. HashFS v1 and ZIP are not needed for official data and can wait for mod support (T9).
3. **Direct lookup by path.** Look up the needed files by CityHash64 path hash and by listing `map/<map>/`, `def/world/`, `def/country/` and the referenced `.ppd` paths. No full tree walk of other folders.
4. **In-memory reading of selected entries.** Read stored or zlib-compressed plain files into memory and verify their size. No GDeflate, no textures, no writing to disk.
5. **Text SII reader.** Units, attributes and `@include` of `.sui` fragments. Only the classes listed as A (later B). Unknown classes and attributes are skipped.
6. **Sector parser.** Only the road, prefab and node records from `.base` and `.data`, after M1 is researched. Other item types are skipped by their declared size.
7. **`.ppd` parser.** Only nodes, lane curves and semaphores, after M2 is researched.

Not in scope: an archive browser, extraction to disk, texture or model handling, HashFS v1, ZIP, `BSII`/`3nK`/`ScsC`, sound, UI and economy data, mod maps.

Intermediate data stays minimal: parsed records are kept in memory only until the map model is built. No raw game files or excerpts are written anywhere.

### Suggested next research before implementation

- Document the HashFS v2 subset that RoadPilot-Map needs in `docs/formats/` (resolves T1 for official archives).
- Research M1, M2, M3 and M5 with synthetic fixtures, following the [test and fixture policy](../testing.md).
- Record the reference resolution rules (M6, M7) in an ADR before the model stage.
