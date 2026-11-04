# RoadPilot-Map

RoadPilot-Map is the map processing and geographic data infrastructure of the RoadPilot ecosystem. It reads the map data of Euro Truck Simulator 2 (ETS2) and American Truck Simulator (ATS) from the user's own local installation and turns it into structured road data for RoadPilot.

> **Status: early development (Phase 1).** The archive reader, the sector file parser, a road and node model, a lane model (lanes per road, traffic side, driving directions) a prefab and junction model (prefab descriptors, road-to-prefab connections, lane movements through junctions) a first navigation graph with route search and a versioned export of the navigation graph are implemented and tested. An estimated road curve with explicit evidence levels (confirmed, estimated, unavailable) and a versioned map package (schema 1) with a spatial index are implemented too. Lane positions (no lane width is stored), a confirmed curve, travel time and distance, live routing and any use as a basis for active steering are **not available**. There are no releases.

Architecture documentation, format specifications, decision records and open questions are in [`docs/`](docs/README.md). An overview for new readers is in the [project wiki](https://github.com/RoadPilotHQ/RoadPilot-Map/wiki).

## Purpose

RoadPilot-Map turns ETS2 and ATS map data into structured, versioned road information (geometry, lanes, curvature and road networks) for other RoadPilot components. It is a separate repository, so that map processing can evolve and be released independently of the desktop application. Processing runs offline on the user's machine, and game data is never distributed ([ADR 0001](docs/adr/0001-rust-library-and-cli-with-local-offline-generation.md)).

## Implemented

| Component | Crate | Scope |
|---|---|---|
| HashFS v2 archive reader | `roadpilot-map-archive` | Opens official `.scs` archives read-only, validates the index, finds entries by path (CityHash64 v1.0.3, confirmed through directory listings) and reads single stored or zlib-compressed entries into memory. See [`docs/formats/hashfs-v2.md`](docs/formats/hashfs-v2.md). |
| SCS sector parser | `roadpilot-map-formats` | Parses map sector `.base` and `.data` files of format version 907: all items with their common header and payload, all nodes, and decoded fields of roads and prefabs. See [`docs/formats/scs-sector-format.md`](docs/formats/scs-sector-format.md). |
| Road and node model | `roadpilot-map-model`, `roadpilot-map` | Validated roads, nodes and road-to-road connections, the confirmed coordinate system and node directions, built from parsed sectors by `build_road_network`. See [`docs/formats/scs-road-geometry.md`](docs/formats/scs-road-geometry.md). |
| Lane model | `roadpilot-map-formats`, `roadpilot-map-model`, `roadpilot-map` | Lanes per road from the road look definitions (read with a minimal text SII reader), the traffic side per road from the country of its nodes, driving directions, stored lane offsets and candidate lane transitions between connected roads. Lane widths and the exact lane order are not stored in the data and stay unknown. See [`docs/formats/scs-lane-model.md`](docs/formats/scs-lane-model.md). |
| Prefab and junction model | `roadpilot-map-formats`, `roadpilot-map-model`, `roadpilot-map` | Prefab descriptors (`.ppd`), prefab definitions and instances, the road or prefab attached to each prefab node, the stored internal lane movements with their traffic rule references, static traffic light positions, and candidate road-lane movements through a junction. Traffic rule semantics and signal states are not modelled. See [`docs/formats/scs-prefab-junctions.md`](docs/formats/scs-prefab-junctions.md). |
| Navigation graph | `roadpilot-map-model` | A directed graph of lanes and prefab slots. Every connection has a status (confirmed, candidate, unknown, forbidden) and an origin. Route search in two modes: `ConfirmedOnly` and `AllowCandidates`, with an algorithmic cost (not a distance). See [`docs/navigation/navigation-graph.md`](docs/navigation/navigation-graph.md). |
| Navigation graph export | `roadpilot-map`, `roadpilot-map-cli` | Loads the navigation model from a local installation (read-only) and writes it as a versioned JSON Lines file for external consumers, with node and edge statuses, origins and known limitations. `roadpilot-map export-navigation` writes it, `roadpilot-map check-navigation-export` validates it. See [`docs/navigation/navigation-export.md`](docs/navigation/navigation-export.md). |
| Road curve and evidence levels | `roadpilot-map-geometry`, `roadpilot-map-model` | `Quality` (confirmed, estimated, unavailable) and `RoadCurve`: the cubic Hermite curve through the node positions with the stored road length as tangent length. It is an **estimate**: it reproduces the stored length within 0.1 % for 99.5 % of the roads and fails above about 120° of turn; its position accuracy cannot be measured. Lane centre lines exist only from explicit caller assumptions because no lane width is stored. See [`docs/formats/scs-road-geometry.md`](docs/formats/scs-road-geometry.md). |
| Map package | `roadpilot-map-package`, `roadpilot-map`, `roadpilot-map-cli` | A versioned package folder: `manifest.json`, `geometry.jsonl` (nodes, roads with lanes and curve, junctions, road connections with status) and the unchanged navigation graph export. Deterministic output, checksums, validation on reading, and a grid index for roads near a position. `roadpilot-map build-package` writes it from the local installation, `check-package` validates it. Marked local use only. See [`docs/formats/map-package.md`](docs/formats/map-package.md). |
| Development diagnostics | `roadpilot-map-cli` | `roadpilot-map doctor` checks a checkout: toolchain version, workspace, configuration files, cargo-deny and documentation. `roadpilot-map --version` prints the version. |

The implementation is verified with tests on synthetic data built in code, plus opt-in tests that read a local game installation without modifying it.

## Planned

- Lane geometry (lane positions along the road); needs lane widths, the lane order and a confirmed curve (LM2, LM3, G1). Nothing in the package supports active steering until then
- Measure the accuracy of the road curve against the game (a reference for the curve position is needed), and the curve formula itself (G1)
- Reader memory and load time targets, and whether the application needs a binary encoding of the package (T5, T7)
- Lane continuity between directly joined prefabs, and the meaning of traffic rules
- Routes from positions, physical route cost and travel time (need a confirmed curve and the unit)
- Curvature and curve radius (they depend on a confirmed curve)
- Speed limits, signs, and the association of traffic lights with lanes
- Installation discovery and a CLI check of a game installation
- `inspect` and `export-debug` commands for packages

## Getting started

Requires [rustup](https://rustup.rs/); the toolchain (Rust 1.97.1) is pinned in `rust-toolchain.toml`.

```sh
cargo build --workspace
cargo test --workspace
cargo run -p roadpilot-map-cli -- --version
cargo run -p roadpilot-map-cli -- doctor
```

Tests need no game installation. The test policy, including the opt-in tests against a local installation, is in [`docs/testing.md`](docs/testing.md).

## Architecture overview

| Crate | Role | Status |
|---|---|---|
| `roadpilot-map-archive` | Read SCS archives | Implemented (HashFS v2) |
| `roadpilot-map-formats` | Parse game files | Partially implemented (sector files, lane and prefab definitions, prefab descriptors) |
| `roadpilot-map-model` | Internal map model | Partially implemented (roads, nodes, lanes and prefabs) |
| `roadpilot-map` | Facade library | Partially implemented |
| `roadpilot-map-cli` | Command-line interface (`roadpilot-map`) | `--version`, `doctor`, `export-navigation`, `check-navigation-export`, `build-package`, `check-package` |
| `roadpilot-map-geometry` | Road curve estimate, evidence levels | Partially implemented (estimated curve; lane positions only from explicit assumptions) |
| `roadpilot-map-package` | Map package format, writer, reader, spatial index | Implemented (schema 1) |
| `roadpilot-map-network` | Road network | Planned (the navigation graph lives in `roadpilot-map-model`) |

Details are in [`docs/architecture/`](docs/architecture/overview.md).

## Integration with the RoadPilot ecosystem

RoadPilot is split into independent repositories. Each one is developed, versioned and released on its own; there is no monorepo and no shared source code.

| Repository | Role |
|---|---|
| [RoadPilot](https://github.com/RoadPilotHQ/RoadPilot) | Main desktop application |
| **RoadPilot-Map** (this repository) | Map processing and geographic data |
| [RoadPilot-Language](https://github.com/RoadPilotHQ/RoadPilot-Language) | Translations and localization |
| [RoadPilot-Visualization](https://github.com/RoadPilotHQ/RoadPilot-Visualization) | Visualization components and development tools |
| [RoadPilot-Plugin](https://github.com/RoadPilotHQ/RoadPilot-Plugin) | Plugin architecture and SDK |
| [RoadPilot-Website](https://github.com/RoadPilotHQ/RoadPilot-Website) | Official website |
| [RoadPilot-Discord](https://github.com/RoadPilotHQ/RoadPilot-Discord) | Discord community infrastructure |
| [RoadPilot-Docs](https://github.com/RoadPilotHQ/RoadPilot-Docs) | Central documentation |

RoadPilot-Map provides map data to the RoadPilot application through a versioned map package, generated on the user's machine ([format](docs/formats/map-package.md), [ADR 0002](docs/adr/0002-map-package-format-and-versioning.md)). The package is derived from game data and is for local use only. The RoadPilot application does not use it yet, and the encoding (JSON) has not been agreed with the consumer.

## Development status

| Area | Status |
|---|---|
| Source code | In development: archive reader, sector parser, road and node model, lane model, prefab and junction model, navigation graph, road curve estimate, map package, diagnostics |
| Lane model | Implemented: lanes, traffic side, directions, candidate transitions |
| Junctions | Implemented: prefabs, road-to-prefab connections, internal movements, candidate lane movements |
| Navigation | Implemented: graph and first route search; confirmed-only routes are limited to prefab interiors by the data |
| Navigation export | Implemented: versioned JSON Lines format, writer, reference reader, CLI |
| Road curve | Implemented as a labelled estimate; the game's curve is not confirmed (G1) |
| Lane geometry | Not available: no lane width is stored (LM3), the lane order is unknown (LM2) |
| Map package | Implemented: schema 1, writer, validating reader, grid index, CLI. Reader memory and load time are not measured |
| Architecture | Documented in [`docs/`](docs/README.md), ADR 0001 and ADR 0002 accepted |
| Releases | None |

## Contributing

RoadPilot is at an early stage and the contribution process is still being defined. Shared guidelines are planned in [RoadPilot-Docs](https://github.com/RoadPilotHQ/RoadPilot-Docs).

Until then, please open an issue or start a discussion before starting any work, so it can be coordinated. All contributions, issues and documentation must be written in English. Game files or data extracted from them must never be added to the repository, issues or pull requests.

## License

Licensing is currently undecided. No license has been selected for this repository yet. Until a license is published here, no permission is granted to use, copy, modify or distribute its contents beyond what the GitHub Terms of Service allow.

---

Euro Truck Simulator 2 and American Truck Simulator are trademarks of SCS Software. RoadPilot is not an official SCS Software product.
