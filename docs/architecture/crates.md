# Crate structure

RoadPilot-Map is a single Cargo workspace with several small crates. This document describes the planned layout and the dependency rules.

> **Partly implemented.** `roadpilot-map` and `roadpilot-map-cli` exist since Phase 0, `roadpilot-map-archive`, `roadpilot-map-formats` and `roadpilot-map-model` since Phase 1, `roadpilot-map-geometry` and `roadpilot-map-package` since Phase 1, Step 8. `roadpilot-map-network` is added when its phase starts.

## Workspace layout

```
RoadPilot-Map/
├── Cargo.toml                    Workspace manifest
├── rust-toolchain.toml           Pinned toolchain
├── crates/
│   ├── roadpilot-map-archive/    Read SCS archives
│   ├── roadpilot-map-formats/    Parse definition files and map sectors
│   ├── roadpilot-map-model/      Internal map model types
│   ├── roadpilot-map-geometry/   Road and lane geometry
│   ├── roadpilot-map-network/    Road network and intersections
│   ├── roadpilot-map-package/    Map package format, writer and reader
│   ├── roadpilot-map/            Facade library and pipeline orchestration
│   └── roadpilot-map-cli/        Command-line interface (binary: roadpilot-map)
├── tests/fixtures/               Shared synthetic fixtures only (see docs/testing.md)
└── docs/
```

## Crates

| Crate | Kind | Responsibility |
|---|---|---|
| `roadpilot-map-archive` | lib | Open HashFS v2 archives (`.scs`) read-only, find entries by path, list directories and read single entries (stored or zlib). No knowledge of map semantics. See [HashFS v2 archive reader](../formats/hashfs-v2.md). |
| `roadpilot-map-formats` | lib | Parse game definition files and map sector files into raw, format-close records. No geometry computation. Today: sector `.base` and `.data` files, see [SCS sector format](../formats/scs-sector-format.md), a minimal text SII reader for the road look, lane type and country definitions, see [SCS lane model](../formats/scs-lane-model.md), and prefab descriptors (`.ppd`) with the prefab model and traffic rule definitions, see [SCS prefabs and junctions](../formats/scs-prefab-junctions.md). |
| `roadpilot-map-model` | lib | Internal map model: nodes, road segments, prefabs, lanes, sectors, identifiers, and the result types of the geometry and network stages. Plain data types, no I/O. Today: roads, nodes and their connections, see [SCS road geometry](../formats/scs-road-geometry.md), lanes with candidate lane transitions, see [SCS lane model](../formats/scs-lane-model.md), and prefab definitions and instances with their stored internal movements and candidate lane movements, see [SCS prefabs and junctions](../formats/scs-prefab-junctions.md), and the navigation graph with route search, see [Navigation graph](../navigation/navigation-graph.md). |
| `roadpilot-map-geometry` | lib | Road and lane geometry with explicit evidence levels (`Quality`, defined in the model). Today: `RoadCurve`, the estimated Hermite curve of a road with sampling, length check, bounds and projection, and `lane_centerline`, which works only from caller-supplied assumptions because no lane width is stored. Curvature and curve radius are not computed. Depends only on `roadpilot-map-model`. See [SCS road geometry](../formats/scs-road-geometry.md) and [Map package](../formats/map-package.md). |
| `roadpilot-map-network` | lib | Road graph, intersections, lane-level connectivity, and later a spatial index for map matching. Not created yet: the first navigation graph and route search (Step 6) live in `roadpilot-map-model` because they are plain data without dependencies. |
| `roadpilot-map-package` | lib | Map package: manifest, geometry file records, writer, validating reader, FNV-1a checksums and the grid index for spatial queries (`MapPackage`). Also holds the small JSON reader and writer helpers that the navigation export shares. Depends only on `roadpilot-map-model`; no external dependency. Specification: [Map package](../formats/map-package.md), decision: [ADR 0002](../adr/0002-map-package-format-and-versioning.md). |
| `roadpilot-map` | lib | Public facade. Discovers the game installation, builds the map model from parsed records, runs the pipeline stages in order and re-exports the stable public API. Also holds the development diagnostics (`dev` module) used by `roadpilot-map doctor`. Today it loads the navigation model of an installation (`load_installation`, reading the archives read-only), writes the versioned navigation graph export (`export`), see [Navigation graph export](../navigation/navigation-export.md), and writes and checks map packages (`write_package`, `check_package`), see [Map package](../formats/map-package.md). |
| `roadpilot-map-cli` | bin | User-facing commands. Depends only on the facade. |

## Dependency rules

```
                  roadpilot-map-cli
                          │
                          ▼
                    roadpilot-map  (facade)
     ┌──────────┬─────────┼──────────┬──────────┐
     ▼          ▼         ▼          ▼          ▼
  archive    formats   geometry   network    package
                │         │          │          │
                └─────────┴────┬─────┴──────────┘
                               ▼
                             model

  (archive has no workspace dependencies)
```

1. Dependencies only point downward. Cycles are not allowed.
2. `roadpilot-map-model` depends on no other workspace crate.
3. `roadpilot-map-archive` depends on no other workspace crate. It does not need the model.
4. `roadpilot-map-formats` does not depend on `archive`. It takes plain bytes or readers as input, and the facade connects the two. The only exception is a dev-dependency for the opt-in test against a local installation, which reads sector files from the real archives.
5. `roadpilot-map-cli` depends only on `roadpilot-map`.
6. `roadpilot-map-package` must be usable on its own, so a consumer can read packages without pulling in extraction code. It depends only on `model`.
7. `roadpilot-map-geometry`, `roadpilot-map-network` and `roadpilot-map-package` do not depend on each other. They exchange data only through `model` types, and the facade passes results from one stage to the next.

The split into these crates is a starting point. If a boundary turns out to add no value during implementation, crates may be merged. A change like this is recorded in an ADR.

## Conventions

These conventions are in effect since Phase 0 (steps 3 and 4). The error-handling crates are added when the first fallible API needs them.

- **Naming:** crate names use the `roadpilot-map-` prefix in kebab-case. The Rust names are `roadpilot_map_*`.
- **Edition:** Rust 2024.
- **Toolchain:** development uses the stable toolchain pinned in `rust-toolchain.toml`. The minimum supported Rust version is set by `rust-version` in the workspace `Cargo.toml`. Clippy checks code against that minimum.
- **Formatting:** `rustfmt` with the stable options in `rustfmt.toml`. `cargo fmt --all --check` must pass.
- **Linting:** Clippy's default lints plus the workspace lints in `Cargo.toml`. `cargo clippy --workspace --all-targets -- -D warnings` must pass.
- **Errors:** library crates define typed errors. `thiserror` is the default way to write them; only the CLI uses `anyhow`. `roadpilot-map-archive` implements `Display` and `std::error::Error` by hand instead, because `thiserror` adds a proc-macro dependency tree whose `unicode-ident` crate requires the Unicode-3.0 license, which is not on the allow list.
- **Panics:** library code must not panic on malformed input. `unwrap` is rejected by Clippy. `expect` is allowed only where an invariant is proven, and the reason is stated in the message.
- **Unsafe code:** forbidden for all crates by the workspace lint `unsafe_code = "forbid"`. Any exception needs an ADR.
- **Dependencies:** kept minimal. `cargo deny check` must pass. It is configured in `deny.toml` and checks:
  - **advisories:** known vulnerabilities, unmaintained and unsound crates, and yanked versions are errors. An exception needs the advisory ID and a written reason.
  - **licenses:** only common permissive licenses (Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, MIT) are allowed for dependencies. This is not the project's own license, which is still undecided. Review the list before adding the first dependency and again once the project license is chosen.
  - **bans:** wildcard version requirements are errors; duplicate crate versions are warnings. No crates are banned yet, and every future ban needs a documented reason.
  - **sources:** only crates.io. Other registries and git dependencies are rejected until explicitly approved.
- **Current external dependencies:** `miniz_oxide` (zlib decoding in `roadpilot-map-archive`, license MIT OR Zlib OR Apache-2.0) and its dependency `adler2` (0BSD OR MIT OR Apache-2.0).
- **Tool version:** cargo-deny is pinned to 0.20.2 (see the header of `deny.toml`). Install it with `cargo install cargo-deny --version 0.20.2 --locked`.
- **Publishing:** crates are not published to crates.io until the license is decided. Unpublished workspace crates are skipped by the license check; a published crate needs a license field.
