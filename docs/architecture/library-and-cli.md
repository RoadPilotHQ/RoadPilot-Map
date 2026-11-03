# Library and CLI

RoadPilot-Map consists of a reusable library and a standalone CLI ([ADR 0001](../adr/0001-rust-library-and-cli-with-local-offline-generation.md)). This document defines what each is responsible for.

> **Status: partially implemented.** The library contains the archive reader, the sector parser and the road and node model, but not yet the end-to-end API sketched below. The CLI implements `--version` and `doctor`; all other commands are planned. Proposals in this document may change during implementation.

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
| Discover a game installation | Planned |
| Read SCS archives | Implemented for HashFS v2 (`roadpilot-map-archive`) |
| Parse the relevant game files | Partially implemented: sector `.base` and `.data` files (`roadpilot-map-formats`); definition files are not parsed |
| Build the internal map model | Partially implemented: roads and nodes (`roadpilot-map-model`, `roadpilot_map::build_road_network`) |
| Road and lane geometry, widths, offsets, curvature and curve radius | Planned (the curve between road nodes is under investigation) |
| Road network, intersections, lane connectivity | Partially implemented: road-to-road connections only |
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
| `roadpilot-map-formats` | `parse_base`, `parse_data` (and `*_with_limits` variants) with the record types `BaseSector`, `Item`, `ItemData`, `Road`, `Prefab`, `Node`, `DataSector` |
| `roadpilot-map-model` | `RoadNetwork` with `roads_at`, `successor`, `predecessor`, `connections`, `geometry`; `Road`, `Node`, `NodeLink`, `RoadGeometry` |
| `roadpilot-map` (facade) | `build_road_network`, `version`, the `dev` diagnostics module, and re-exports of the parser (`formats`) and the model (`model`) |

The facade does not depend on the archive reader yet; callers combine it with the facade themselves.

## CLI

The CLI (`roadpilot-map-cli`, binary name `roadpilot-map`) is a **thin user-facing layer** over the facade library.

### Responsibilities

- Parse command-line arguments.
- Locate the game installation, or accept it via `--game-dir` (planned; no command reads a game installation yet).
- Show progress and readable errors.
- Return meaningful exit codes.
- Contain no map processing logic of its own.

### Planned commands

| Command | Purpose | Phase |
|---|---|---|
| `roadpilot-map --version` (or `-V`) | Print the tool version. | 0 (done) |
| `roadpilot-map doctor [--project-dir <path>]` | Development diagnostics for a RoadPilot-Map checkout (see [doctor](#doctor)). | 0 (done) |
| Game installation check (`--game-dir <path>`, command name to be decided) | Check that a folder looks like an ATS or ETS2 install and list its archives. No parsing. Originally planned as Phase 0, Step 8. | 1 (not implemented yet) |
| `roadpilot-map build --game-dir <path> --out <path>` | Generate a map package. | Later |
| `roadpilot-map inspect <package>` | Show a package's manifest and summary statistics. | Later |
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

Implemented today: 0, 1 (`doctor` only) and 2. Codes 3 and 4 belong to the planned commands. `doctor` uses code 1 when a required check fails (see below).

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
