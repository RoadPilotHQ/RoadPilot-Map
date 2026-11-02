# Library and CLI

RoadPilot-Map consists of a reusable library and a standalone CLI ([ADR 0001](../adr/0001-rust-library-and-cli-with-local-offline-generation.md)). This document defines what each is responsible for.

> **Planned.** The commands and API shapes below are proposals and may change during implementation.

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

## CLI

The CLI (`roadpilot-map-cli`, binary name `roadpilot-map`) is a **thin user-facing layer** over the facade library.

### Responsibilities

- Parse command-line arguments.
- Locate the game installation, or accept it via `--game-dir`.
- Show progress and readable errors.
- Return meaningful exit codes.
- Contain no map processing logic of its own.

### Planned commands

| Command | Purpose | Phase |
|---|---|---|
| `roadpilot-map --version` | Print the tool version. | 0 |
| `roadpilot-map doctor --game-dir <path>` | Check that the folder looks like an ATS or ETS2 install and list its archives. No parsing. | 0 |
| `roadpilot-map build --game-dir <path> --out <path>` | Generate a map package. | Later |
| `roadpilot-map inspect <package>` | Show a package's manifest and summary statistics. | Later |
| `roadpilot-map export-debug <package> --out <path>` | Write a readable debug export for development and visualization. | Later |

### Exit codes (proposal)

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | General error |
| 2 | Invalid command-line usage |
| 3 | Game installation not found or not recognized |
| 4 | Processing failed (unsupported or malformed game data) |

## Output location

Generated packages and debug exports are written to a path chosen by the user, outside the repository by default. They are personal, locally generated data and must not be shared or committed.
