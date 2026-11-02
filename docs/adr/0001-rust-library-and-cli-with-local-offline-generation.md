# ADR 0001: Rust library and CLI with local offline generation

- **Status:** Accepted
- **Date:** 2026-11-03
- **Supersedes:** -
- **Superseded by:** -

## Context

RoadPilot needs accurate, lane-level map data for its driving assistance systems. The map data of American Truck Simulator (ATS) and Euro Truck Simulator 2 (ETS2) is stored in SCS game files. It must be extracted, parsed and turned into structured road, lane and network data that the RoadPilot application can consume.

RoadPilot is split into independent repositories with no shared source code. RoadPilot-Map must be developed, versioned and released independently of the RoadPilot desktop application.

The original game assets belong to SCS Software. Redistributing them, or datasets extracted from them, is not acceptable for this project.

This ADR records project decisions, not legal conclusions. Whether local extraction is permitted, and how derived data may be used, has not been legally reviewed yet. These points are tracked as open questions L1 and L2 in [Open questions](../open-questions.md).

## Decision

### 1. Language

RoadPilot-Map is written in **Rust**.

### 2. Architecture

RoadPilot-Map consists of a **reusable Rust library** and a **standalone command-line interface (CLI)** built on top of it. All map processing logic lives in the library. The CLI is a thin layer for users and developers. See [Crate structure](../architecture/crates.md) and [Library and CLI](../architecture/library-and-cli.md).

### 3. Processing model

Map data is produced by **offline extraction from the user's locally installed ATS or ETS2 game files**. Processing runs on the user's machine, ahead of time, and produces a map package. Map processing does not run while the RoadPilot application is driving.

### 4. Integration

The main RoadPilot application consumes the **generated, versioned map data** (the map package). It does not depend on RoadPilot-Map's source code, and RoadPilot-Map does not depend on the RoadPilot application's source code. The package format is the contract between them and will be defined in its own ADR.

### 5. Game priority

**ATS is supported first. ETS2 is supported second.** The design must not prevent ETS2 support, but ATS-specific work is prioritized and tested first.

### 6. Primary use case

The primary use case is **accurate lane-level map data for RoadPilot's driving assistance systems**. Where precision and simplicity conflict, precision of lane geometry takes priority.

### Core responsibilities

- Road geometry
- Lane geometry
- Lane widths and offsets
- Lane curvature and curve radius
- Road network and intersections
- Map metadata and versioning
- Future map matching support (not implemented initially, but the design must allow it)

### Constraints

1. **No redistribution of SCS assets or extracted map data.** The repository, its CI, its test fixtures and its releases must never contain original SCS game files or map datasets extracted or derived from them.
2. **Users generate their own map packages** from their own game installations.
3. **No final software license yet.** A license is chosen only after the legal implications have been reviewed. Until then, dependencies must be chosen so that they do not restrict that later choice.
4. **Modularity and independence.** RoadPilot-Map is modular and independent of the RoadPilot desktop application's source code.
5. **English only.** All documentation, identifiers and code are written in English.

## Alternatives considered

| Alternative | Reason not chosen |
|---|---|
| CLI only, no reusable library | Prevents reuse by other tools and makes stage-by-stage testing harder. |
| Library only, no CLI | Users would need a separate tool to generate packages. |
| Runtime map processing inside the RoadPilot application | Couples the two code bases, slows startup and makes map data harder to version and verify. |
| Distributing prebuilt map packages | Conflicts with the no-redistribution constraint. |
| Supporting ATS and ETS2 equally from the start | Splits early effort. ATS first gives a smaller, focused first target. |

## Consequences

### Positive

- Each pipeline stage can be developed and tested in isolation as a library component.
- The RoadPilot application depends only on a documented, versioned package format.
- Keeping all game data on the user's machine avoids redistributing SCS content.
- Rust gives predictable performance and memory safety for parsing untrusted binary game files.

### Negative

- Users must run a generation step before RoadPilot can use map data, and repeat it after game or DLC updates.
- Tests cannot use real game data in the repository or CI. Synthetic fixtures must be written and maintained.
- The package format becomes a stable contract and needs careful versioning.
- The processing model depends on the outcome of the legal review (open questions L1 and L2). A negative outcome would require revisiting this ADR.

### Follow-up work

- ADRs for the package format, versioning scheme, coordinate system, spline representation and DLC handling.
- Legal review, followed by a license ADR.
- The Phase 0 setup described in [Phase 0 plan](../roadmap/phase-0.md).
