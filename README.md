# RoadPilot-Map

RoadPilot-Map is the map processing and geographic data infrastructure of the RoadPilot ecosystem. It reads the map data of Euro Truck Simulator 2 (ETS2) and American Truck Simulator (ATS) from the user's own local installation and turns it into structured road data for RoadPilot.

> **Status: early development (Phase 1).** The archive reader, the sector file parser and a road and node model are implemented and tested. Lane geometry, junctions, curvature and map package generation are **not implemented yet**. There are no releases.

Architecture documentation, format specifications, decision records and open questions are in [`docs/`](docs/README.md). An overview for new readers is in the [project wiki](https://github.com/RoadPilotHQ/RoadPilot-Map/wiki).

## Purpose

RoadPilot-Map turns ETS2 and ATS map data into structured, versioned road information (geometry, lanes, curvature and road networks) for other RoadPilot components. It is a separate repository, so that map processing can evolve and be released independently of the desktop application. Processing runs offline on the user's machine, and game data is never distributed ([ADR 0001](docs/adr/0001-rust-library-and-cli-with-local-offline-generation.md)).

## Implemented

| Component | Crate | Scope |
|---|---|---|
| HashFS v2 archive reader | `roadpilot-map-archive` | Opens official `.scs` archives read-only, validates the index, finds entries by path (CityHash64 v1.0.3, confirmed through directory listings) and reads single stored or zlib-compressed entries into memory. See [`docs/formats/hashfs-v2.md`](docs/formats/hashfs-v2.md). |
| SCS sector parser | `roadpilot-map-formats` | Parses map sector `.base` and `.data` files of format version 907: all items with their common header and payload, all nodes, and decoded fields of roads and prefabs. See [`docs/formats/scs-sector-format.md`](docs/formats/scs-sector-format.md). |
| Road and node model | `roadpilot-map-model`, `roadpilot-map` | Validated roads, nodes and road-to-road connections, the confirmed coordinate system and node directions, built from parsed sectors by `build_road_network`. See [`docs/formats/scs-road-geometry.md`](docs/formats/scs-road-geometry.md). |
| Development diagnostics | `roadpilot-map-cli` | `roadpilot-map doctor` checks a checkout: toolchain version, workspace, configuration files, cargo-deny and documentation. `roadpilot-map --version` prints the version. |

The implementation is verified with tests on synthetic data built in code, plus opt-in tests that read a local game installation without modifying it.

## Planned

- Lane model: lane count, widths, offsets and directions from the road look definitions
- Parser for text SII definition files and for prefab descriptors (`.ppd`)
- Junctions and intersections, lane-level connectivity
- The curve between road nodes, curvature and curve radius (the curve formula is under investigation)
- Speed limits, traffic rules, traffic lights and signs
- Installation discovery and a CLI check of a game installation
- The versioned map package, and `build`, `inspect` and `export-debug` commands

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
| `roadpilot-map-formats` | Parse game files | Partially implemented (sector files) |
| `roadpilot-map-model` | Internal map model | Partially implemented (roads and nodes) |
| `roadpilot-map` | Facade library | Partially implemented |
| `roadpilot-map-cli` | Command-line interface (`roadpilot-map`) | `--version` and `doctor` |
| `roadpilot-map-geometry`, `roadpilot-map-network`, `roadpilot-map-package` | Geometry, network, map package | Planned |

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

RoadPilot-Map will provide map data to the RoadPilot application through a versioned map package, generated on the user's machine. The package format has not been designed yet.

## Development status

| Area | Status |
|---|---|
| Source code | In development: archive reader, sector parser, road and node model, diagnostics |
| Lane geometry, junctions, map package | Not started |
| Architecture | Documented in [`docs/`](docs/README.md), ADR 0001 accepted |
| Releases | None |

## Contributing

RoadPilot is at an early stage and the contribution process is still being defined. Shared guidelines are planned in [RoadPilot-Docs](https://github.com/RoadPilotHQ/RoadPilot-Docs).

Until then, please open an issue or start a discussion before starting any work, so it can be coordinated. All contributions, issues and documentation must be written in English. Game files or data extracted from them must never be added to the repository, issues or pull requests.

## License

Licensing is currently undecided. No license has been selected for this repository yet. Until a license is published here, no permission is granted to use, copy, modify or distribute its contents beyond what the GitHub Terms of Service allow.

---

Euro Truck Simulator 2 and American Truck Simulator are trademarks of SCS Software. RoadPilot is not an official SCS Software product.
