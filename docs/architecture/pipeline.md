# Map data pipeline

This document describes the planned stages that turn a local ATS or ETS2 installation into a map package.

> **Planned.** Details of the SCS file formats are not yet verified and are tracked in [Open questions](../open-questions.md).

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

### 2. Archive read

- **Input:** archive paths from `Installation`.
- **Output:** a virtual file system view: list of entries and access to their decompressed bytes.
- **Crate:** `roadpilot-map-archive`.
- **Notes:** must handle the archive format variants used by supported game versions. Later archives may override earlier ones (see DLC handling below).

### 3. Parse

- **Input:** bytes of relevant files (definition files, map sector files).
- **Output:** raw, format-close records such as road look definitions, prefab descriptions, nodes and map items.
- **Crate:** `roadpilot-map-formats`.
- **Notes:** game data is treated as untrusted input. Unknown or unsupported records produce warnings, not crashes.

### 4. Model

- **Input:** raw records.
- **Output:** the internal map model - nodes, road segments, prefabs, lanes and sectors with stable identifiers.
- **Crates:** `roadpilot-map-model` (types), `roadpilot-map` (building).
- **Notes:** resolves references between records, for example road segment to road look.

### 5. Geometry

- **Input:** map model.
- **Output:** road centerlines and lane geometry, with widths, offsets, curvature and curve radius.
- **Crate:** `roadpilot-map-geometry`.
- **Notes:** the proposal is to store lanes in two forms:
  - **analytic curves**, kept as close as possible to the game's own spline definitions, for precision;
  - **sampled polylines** at a fixed spacing, with curvature and radius per sample, for simple consumption.

  The exact curve representation will be decided in its own ADR.

### 6. Network

- **Input:** model, including the geometry results.
- **Output:** a road graph with intersections and lane-level connections, including connections through prefabs (junctions).
- **Crate:** `roadpilot-map-network`.
- **Notes:** designed so a spatial index for future map matching can be added without breaking the package format.

### 7. Package

- **Input:** model, including the geometry and network results.
- **Output:** a map package on disk.
- **Crate:** `roadpilot-map-package`.
- **Notes:** the package format and encoding will be decided in their own ADR.

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

## DLC and override handling

Map DLCs add or replace map content. The order in which archives are applied, and how overrides are resolved, must be understood before the model stage is complete. This is an open question and will get its own ADR.

## Determinism

- The same inputs and tool version give byte-identical data sections. Only `generated_at` in the manifest may differ.
- Stable ordering of all collections in the output.
- No timestamps, random values or machine-specific paths in data sections.
- A reproducibility test builds the same input twice and compares the data sections.

## Testing strategy

| Level | Data | Runs in CI |
|---|---|---|
| Unit tests per stage | Small synthetic fixtures written by hand in `tests/fixtures/` | Yes |
| Pipeline tests | Synthetic installation built from fixtures | Yes |
| Real installation tests | User's local game install, via `ROADPILOT_MAP_GAME_DIR` | No (opt-in, ignored by default) |

No real game files or generated packages are ever committed or used in CI.
