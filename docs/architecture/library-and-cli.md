# Library and CLI

RoadPilot-Map consists of a reusable library and a standalone CLI ([ADR 0001](../adr/0001-rust-library-and-cli-with-local-offline-generation.md)). This document defines what each is responsible for.

> **Status: partially implemented.** The library contains the archive reader, the sector and prefab descriptor parsers, and the road, lane and prefab model, but not yet the end-to-end API sketched below. The CLI implements `--version` and `doctor`; all other commands are planned. Proposals in this document may change during implementation.

## Library

The library (the `roadpilot-map` facade and the crates below it) contains **all map processing logic**.

### Responsibilities

- Discover a game installation: detect the game (ATS or ETS2), its version and its archives.
- Read SCS archives and parse the relevant game files.
- Build the internal map model.
- Compute road geometry, lane geometry, lane widths and offsets, curvature and curve radius.
- Build the road network, intersections and lane connectivity.
- Write and read versioned map packages.
- Report progress and warnings through callbacks or return values, not through printing.

| Responsibility | Status |
|---|---|
| Discover a game installation | Planned (an explicit folder is accepted: `load_installation`) |
| Read SCS archives | Implemented for HashFS v2 (`roadpilot-map-archive`) |
| Parse the relevant game files | Partially implemented: sector `.base` and `.data` files, prefab descriptors (`.ppd`), and the road look, lane type, country, prefab model and traffic rule definitions (`roadpilot-map-formats`) |
| Build the internal map model | Partially implemented: roads, nodes, lanes and prefabs (`roadpilot-map-model`, `roadpilot_map::build_road_network`, `roadpilot_map::build_lane_network`, `roadpilot_map::build_prefab_network`) |
| Road and lane geometry, widths, offsets, curvature and curve radius | Planned (the curve between road nodes is under investigation) |
| Road network, intersections, lane connectivity | Partially implemented: road-to-road connections, candidate lane transitions, junctions as prefab instances with confirmed internal movements and candidate lane movements, a navigation graph and a first route search (`ConfirmedOnly`, `AllowCandidates`); no time, distance or live routing |
| Write and read map packages | Planned |
| Progress reporting | Planned |

In addition, the facade has a separate `dev` module with **development diagnostics** for working on RoadPilot-Map itself (used by `roadpilot-map doctor`). It is not part of map processing and has no effect on map packages.

### Rules

- **No terminal assumptions.** The library never prints, reads from stdin, or exits the process.
- **Typed errors.** Every fallible public function returns a `Result` with a typed error that names the stage and the input that failed.
- **Explicit inputs.** Paths, options and the game type are passed in. No hidden global state, no environment variables read inside the library.
- **Read-only game access.** The library never writes to the game installation.
- **Deterministic output.** Given the same inputs and options, the package data is identical. Only the build time in the manifest may differ (see [Map data pipeline](pipeline.md#determinism)).

### Planned public surface (sketch)

```text
discover(game_dir)        -> Installation      game, version, archive list
build(installation, opts) -> MapPackage        runs the full pipeline
write(package, out_path)  -> ()                writes a package to disk
read(path)                -> MapPackage        reads and validates a package
```

Individual stages are also public so tools and tests can run them one at a time.

None of the four functions above exists yet. What exists today is the stage API:

| Crate | Public API today |
|---|---|
| `roadpilot-map-archive` | `Archive::open`, `Archive::open_with_limits`, `Archive::entries`, `Archive::find`, `Archive::read`, `Archive::read_file`, `Archive::list_dir`, `path_hash` |
| `roadpilot-map-formats` | `parse_base`, `parse_data` (and `*_with_limits` variants) with the record types `BaseSector`, `Item`, `ItemData`, `Road`, `Prefab`, `Node`, `DataSector`; `sii::parse_sii` and the `definitions` records; `ppd::parse_ppd` with `PrefabDescriptor` |
| `roadpilot-map-model` | `RoadNetwork` with `roads_at`, `successor`, `predecessor`, `connections`, `geometry`; `Road`, `Node`, `NodeLink`, `RoadGeometry`; `LaneNetwork` with `road`, `transition`; `RoadLanes`, `Lane`; `PrefabNetwork` with `instance`, `definition`, `arms`, `movements`, `lane_movements`; `PrefabInstance`, `PrefabDefinition`, `PrefabMovement`, `LaneMovement`; `NavigationGraph` with `build`, `route`, `reachable_from`; `NavigationGraphBuilder`, `Route`, `RoutingMode`, `ConnectionStatus` |
| `roadpilot-map` (facade) | `build_road_network`, `LaneDefinitions`, `build_lane_network`, `PrefabDefinitions`, `build_prefab_network`, `load_installation`, `Game`, `export` (`write_navigation_export`, `write_navigation_export_file`, `check_navigation_export`, `read_navigation_export`), `write_package`, `check_package`, `check_package_output`, `PackageOptions`, `version`, the `dev` diagnostics module, and re-exports of the parser (`formats`), the model (`model`), the geometry (`geometry`) and the package format (`map_package`) |

The facade depends on the archive reader since Step 7 and reads the archives of an installation in `load_installation`. The single-step functions (`build_prefab_network` takes a function that loads a file by archive path) remain available for tests and tools.

## CLI

The CLI (`roadpilot-map-cli`, binary name `roadpilot-map`) is a **thin user-facing layer** over the facade library.

### Responsibilities

- Parse command-line arguments.
- Locate the game installation, or accept it via `--game-dir` (`export-navigation` takes it explicitly; there is no discovery yet).
- Show progress and readable errors.
- Return meaningful exit codes.
- Contain no map processing logic of its own.

### Planned commands

| Command | Purpose | Phase |
|---|---|---|
| `roadpilot-map --version` (or `-V`) | Print the tool version. | 0 (done) |
| `roadpilot-map doctor [--project-dir <path>]` | Development diagnostics for a RoadPilot-Map checkout (see [doctor](#doctor)). | 0 (done) |
| `roadpilot-map export-navigation --game <ets2\|ats> --game-dir <path> --out <file> [--force]` | Load the navigation model of an installation and write the navigation graph export ([format](../navigation/navigation-export.md)). | 1 (done) |
| `roadpilot-map check-navigation-export <file>` | Check an export file against the specification and print a summary. | 1 (done) |
| `roadpilot-map build-package --game <ets2\|ats> --game-dir <path> --out <folder> [--force] [--without-navigation]` | Load the models of an installation and write a map package folder ([format](../formats/map-package.md)). | 1 (done) |
| `roadpilot-map check-package <folder>` | Validate a package: manifest, checksums, geometry file, navigation file and their agreement. | 1 (done) |
| Game installation check (`--game-dir <path>`, command name to be decided) | Check that a folder looks like an ATS or ETS2 install and list its archives. No parsing. Originally planned as Phase 0, Step 8. | 1 (not implemented yet) |
| `roadpilot-map inspect <package>` | Show a package's manifest and summary statistics (`check-package` prints a summary today). | Later |
| `roadpilot-map export-debug <package> --out <path>` | Write a readable debug export for development and visualization. | Later |

Any other arguments, including `--help` (not implemented), print the usage text and exit with code 2.

### Exit codes (proposal)

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | General error |
| 2 | Invalid command-line usage |
| 3 | Game installation not found or not recognized |
| 4 | Processing failed (unsupported or malformed game data) |

Implemented today: 0, 1, 2, 3 and 4. `doctor` uses code 1 when a required check fails (see below). `export-navigation` uses 1 for problems with the output (file exists, folder missing, inside the game folder, write error), 3 if the folder is not an installation of the game, and 4 if game data or the models are invalid. `check-navigation-export` uses 4 for an invalid or unsupported file and 1 if the file cannot be read. `build-package` follows `export-navigation` (the output is a folder; `--force` replaces only a folder that holds package files), and `check-package` uses 4 for an invalid package (manifest, checksum, geometry or navigation file, or a disagreement between them) and 1 if the folder cannot be read.

### doctor

`roadpilot-map doctor` checks a RoadPilot-Map checkout for development. It is meant for people working on RoadPilot-Map, not for end users. It only reads files, never installs anything, and does not use the network.

```text
roadpilot-map doctor                        Find the workspace in the current folder or a parent folder
roadpilot-map doctor --project-dir <path>   Check the given project folder
```

| Check | What it verifies | If it fails |
|---|---|---|
| Rust toolchain | `rustc --version`, run in the project folder (so `rust-toolchain.toml` applies), is at least the `rust-version` declared in `Cargo.toml` | Failure |
| Workspace manifest | `Cargo.toml` exists and has a `[workspace]` table | Failure |
| Workspace member (one per member) | `crates/roadpilot-map` and `crates/roadpilot-map-cli` each have a `Cargo.toml` and are listed in the workspace members. Planned crates are not required. | Failure |
| Project configuration | `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `rustfmt.toml` and `deny.toml` exist | Failure, listing the missing files |
| cargo-deny | `cargo deny --version` works, and its version is reported | **Warning** only |
| Documentation | `README.md`, `docs/README.md`, `docs/architecture/`, `docs/roadmap/phase-0.md` and `docs/testing.md` exist | Failure, listing the missing paths |

Each check prints one line: `[PASS]`, `[WARN]` or `[FAIL]`, followed by the check name and details. A summary line ends the output, for example `Result: 7 passed, 0 failed, 0 warnings`.

| Exit code | Meaning |
|---|---|
| 0 | All required checks passed. Warnings (such as a missing cargo-deny) are allowed. |
| 1 | At least one required check failed, or no workspace was found. |
| 2 | Invalid command-line usage. |

## Output location

Generated packages and debug exports are written to a path chosen by the user, outside the repository by default. They are personal, locally generated data and must not be shared or committed.
