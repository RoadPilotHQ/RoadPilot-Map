# Architecture overview

This document describes the goals, scope and context of RoadPilot-Map. The foundational decisions are recorded in [ADR 0001](../adr/0001-rust-library-and-cli-with-local-offline-generation.md).

## Goals

- Produce **accurate, lane-level map data** from the user's own ATS or ETS2 installation, for RoadPilot's driving assistance systems.
- Provide road geometry, lane geometry, lane widths and offsets, lane curvature and curve radius, and a connected road network with intersections.
- Package the result in a **versioned, self-describing map package** that the RoadPilot application can consume.
- Make every processing stage **testable in isolation** with synthetic data.
- Produce **deterministic output**: the same inputs and tool version give byte-identical data sections. Only the build time in the manifest may differ.

## Non-goals (for now)

- Map matching. It is a future responsibility, and the design keeps it possible.
- Rendering or visualization. This belongs to RoadPilot-Visualization.
- Routing or navigation logic.
- Mod map support. This may be considered later.
- Running inside the RoadPilot application at drive time.
- Distributing game assets or generated map packages.

## Constraints

- No SCS game assets or extracted map datasets in the repository, CI, test fixtures or releases.
- Users generate map packages locally from their own installation.
- No final software license until legal review is complete.
- No dependency on the RoadPilot desktop application's source code.
- Everything in English.

## System context

```
 User's machine
 ┌───────────────────────────────────────────────────────────────────┐
 │                                                                   │
 │  ATS / ETS2 install          RoadPilot-Map              RoadPilot │
 │  ┌───────────────┐    read   ┌──────────────┐  write   ┌────────┐ │
 │  │ .scs archives │──────────►│ CLI + library│─────────►│  map   │ │
 │  │ (game + DLC)  │           └──────────────┘          │package │ │
 │  └───────────────┘                                     └───┬────┘ │
 │                                                            │ read │
 │                                                            ▼      │
 │                                                ┌───────────────┐  │
 │                                                │ RoadPilot app │  │
 │                                                └───────────────┘  │
 └───────────────────────────────────────────────────────────────────┘
```

- RoadPilot-Map only **reads** the game installation. It never modifies game files.
- The map package is the **only interface** between RoadPilot-Map and the RoadPilot application.
- Nothing leaves the user's machine.

## Quality attributes

| Attribute | Approach |
|---|---|
| Accuracy | Keep lane geometry as close as possible to the game's own definitions. Proposed: store analytic curves alongside sampled data (to be decided in the spline representation ADR). |
| Testability | Typed stage boundaries, synthetic fixtures, opt-in tests against a real install. |
| Determinism | Stable ordering, no timestamps or randomness in data sections, reproducibility tests. |
| Robustness | Game files are treated as untrusted input. Parsers return errors instead of panicking. |
| Evolvability | Versioned package schema. Crates added per phase with one-way dependencies. |
| Legal safety | No game data in the repository. Dependency licenses checked with `cargo-deny`. |

## Related documents

- [Crate structure](crates.md)
- [Library and CLI](library-and-cli.md)
- [Map data pipeline](pipeline.md)
- [Open questions](../open-questions.md)
