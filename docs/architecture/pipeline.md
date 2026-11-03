# Map data pipeline

This document describes the stages that turn a local ATS or ETS2 installation into a map package.

> **Status: partially implemented.** Stages 2 to 4 exist in part; stages 1, 5, 6 and 7 are planned. There is no end-to-end pipeline yet: the facade does not discover installations or read archives, and no map package is written. Each stage below has a status line. The confirmed formats are documented in [`docs/formats/`](../formats/); open points are tracked in [Open questions](../open-questions.md).

| Stage | Status |
|---|---|
| 1. Discover | Planned |
| 2. Archive read | Implemented for HashFS v2 |
| 3. Parse | Partially implemented: sector `.base` and `.data` files, prefab descriptors, road look, lane type, country, prefab model and traffic rule definitions |
| 4. Model | Partially implemented: roads, nodes, their connections, lanes and prefabs |
| 5. Geometry | Planned; the curve between road nodes is under investigation |
| 6. Network | Partially implemented: road-to-road connections, candidate lane transitions, movements through junctions, a navigation graph with route search |
| 7. Package | Planned |

## Overview

```
Game install ─► Discover ─► Archive read ─► Parse ─► Model ─► Geometry ─► Network ─► Package
 (ATS/ETS2)     version,     .scs entries    defs,     nodes,    lanes,      graph,      manifest
                archives,                    sectors   roads,    widths,     junctions,  + data
                DLCs                                   prefabs   curvature   lane links
```

Each stage has a typed input and a typed output. Stages do not share hidden state, so each one can be tested alone with synthetic data.

## Stages

### 1. Discover

- **Input:** path to a game installation.
- **Output:** `Installation` - game (ATS or ETS2), game version, ordered list of archives, installed map DLCs.
- **Crate:** `roadpilot-map` (facade).
- **Notes:** read-only. Fails early with a clear error if the folder is not a recognized installation.
- **Status:** planned. Callers currently pass archive paths themselves.

### 2. Archive read

- **Input:** archive paths from `Installation`.
- **Output:** a virtual file system view: list of entries and access to their decompressed bytes.
- **Crate:** `roadpilot-map-archive`.
- **Notes:** must handle the archive format variants used by supported game versions. Later archives may override earlier ones (see DLC handling below).
- **Status:** implemented for HashFS v2, the format of all official archives: `Archive::open`, `find`, `list_dir`, `read_file` ([HashFS v2 archive reader](../formats/hashfs-v2.md)). Entries are read into memory; nothing is extracted to disk. HashFS v1 and ZIP (used by mods) are not supported. Override handling between archives is not implemented.

### 3. Parse

- **Input:** bytes of relevant files (definition files, map sector files).
- **Output:** raw, format-close records such as road look definitions, prefab descriptions, nodes and map items.
- **Crate:** `roadpilot-map-formats`.
- **Notes:** game data is treated as untrusted input. Unknown or unsupported records produce warnings, not crashes.
- **Status:** partially implemented for sector `.base` and `.data` files, format version 907 (`parse_base`, `parse_data`; [SCS sector format](../formats/scs-sector-format.md)). A minimal text SII reader reads the road look, road template variant, lane type and country definitions ([SCS lane model](../formats/scs-lane-model.md)), and the prefab model and traffic rule definitions. Prefab descriptors (`.ppd`, version 25) are parsed for their nodes, navigation curves and semaphores ([SCS prefabs and junctions](../formats/scs-prefab-junctions.md)). Other definition files are not parsed yet. Malformed input never crashes the parser; it returns a typed `ParseError`. An unknown sector item type is also an error rather than a warning, because sector items have no length field and the rest of the file cannot be located.

### 4. Model

- **Input:** raw records.
- **Output:** the internal map model - nodes, road segments, prefabs, lanes and sectors with stable identifiers.
- **Crates:** `roadpilot-map-model` (types), `roadpilot-map` (building).
- **Notes:** resolves references between records, for example road segment to road look.
- **Status:** partially implemented. `roadpilot-map-model` holds validated roads and nodes (`RoadNetwork`), and `roadpilot_map::build_road_network` builds it from parsed sectors ([SCS road geometry](../formats/scs-road-geometry.md)). The lanes of each road (`LaneNetwork`) are built by `roadpilot_map::build_lane_network` from the road looks and the country of the road's nodes ([SCS lane model](../formats/scs-lane-model.md)); unconfirmed lane properties are explicit unknown states. Prefab definitions and instances (`PrefabNetwork`) are built by `roadpilot_map::build_prefab_network` ([SCS prefabs and junctions](../formats/scs-prefab-junctions.md)). Sectors are not part of the model.

### 5. Geometry

- **Input:** map model.
- **Output:** road centerlines and lane geometry, with widths, offsets, curvature and curve radius.
- **Crate:** `roadpilot-map-geometry`.
- **Notes:** the proposal is to store lanes in two forms:
  - **analytic curves**, kept as close as possible to the game's own spline definitions, for precision;
  - **sampled polylines** at a fixed spacing, with curvature and radius per sample, for simple consumption.

  The exact curve representation will be decided in its own ADR.
- **Status:** planned; the crate does not exist. The model exposes only confirmed geometry: node positions, node directions, the straight-line distance and the stored road length. The curve between two nodes is not confirmed (open question G1 in [SCS road geometry](../formats/scs-road-geometry.md)), so no interpolation, curvature or curve radius is computed.

### 6. Network

- **Input:** model, including the geometry results.
- **Output:** a road graph with intersections and lane-level connections, including connections through prefabs (junctions).
- **Crate:** `roadpilot-map-network`.
- **Notes:** designed so a spatial index for future map matching can be added without breaking the package format.
- **Status:** partially implemented, not yet in its own crate. `RoadNetwork` resolves road-to-road connections through shared nodes in stored direction (`successor`, `predecessor`, `connections`). `LaneNetwork::transition` pairs lanes across such connections as candidates by index where both roads match. `PrefabNetwork` gives the arms of each junction (prefab instance), its stored internal movements (confirmed) and road-lane movements through it (candidates). The navigation graph (`NavigationGraph`) joins these into one directed graph with a status and origin per edge, and finds routes in two modes, `ConfirmedOnly` and `AllowCandidates` ([Navigation graph](../navigation/navigation-graph.md)). Lane transitions between directly joined prefabs and confirmed road-lane pairing are open.

### 7. Package

- **Input:** model, including the geometry and network results.
- **Output:** a map package on disk.
- **Crate:** `roadpilot-map-package`.
- **Notes:** the package format and encoding will be decided in their own ADR.
- **Status:** planned; the crate does not exist and no package is written.

## Map package (proposal)

A map package has a **manifest** and **data sections**.

The manifest identifies the package and its source, but contains no game content:

| Field | Purpose |
|---|---|
| `schema_version` | Version of the package format. |
| `tool_version` | RoadPilot-Map version that generated it. |
| `game` | `ats` or `ets2`. |
| `game_version` | Game version detected during discovery. |
| `sources` | List of archives and DLCs used, with content fingerprints (hashes). |
| `generated_at` | Build time. Kept out of the data sections so data stays deterministic. |

Data sections hold the road, lane and network data. A readable debug export is planned in addition to the main format.

Not implemented; this section remains a proposal.

## DLC and override handling

Map DLCs add or replace map content. The order in which archives are applied, and how overrides are resolved, must be understood before the model stage is complete. This is an open question and will get its own ADR.

## Determinism

- The same inputs and tool version give byte-identical data sections. Only `generated_at` in the manifest may differ.
- Stable ordering of all collections in the output.
- No timestamps, random values or machine-specific paths in data sections.
- A reproducibility test builds the same input twice and compares the data sections.

## Testing strategy

| Level | Data | Runs automatically | Status |
|---|---|---|---|
| Unit and integration tests per stage | Synthetic inputs built in the test code | Yes, in `cargo test` | In place for the archive reader, the sector parser and the road model |
| Pipeline tests | Synthetic installation built from fixtures in a temporary directory | Yes, in `cargo test` | Planned together with installation discovery |
| Real installation tests | User's local game install, via `ROADPILOT_MAP_GAME_DIR` | No (opt-in, ignored by default) | In place for the sector parser and the road network |

There is no CI workflow yet; the checks run locally before every commit.

No real game files or generated packages are ever committed or used in CI. The full rules, including fixture layout and determinism, are in the [Test and fixture policy](../testing.md).
