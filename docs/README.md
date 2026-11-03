# RoadPilot-Map documentation

This folder contains the architecture documentation, decision records and plans for RoadPilot-Map.

> **Status: Phase 1.** The workspace, the development diagnostics and the HashFS v2 archive reader are implemented. Everything else described here is the planned design unless a document says otherwise.

## Contents

### Architecture

- [Overview](architecture/overview.md) - goals, non-goals, constraints and system context.
- [Crate structure](architecture/crates.md) - Cargo workspace layout and dependency rules.
- [Library and CLI](architecture/library-and-cli.md) - responsibilities and public surface of each.
- [Map data pipeline](architecture/pipeline.md) - stages from game files to a map package.

### Testing

- [Test and fixture policy](testing.md) - test categories, synthetic fixtures and game data restrictions.

### Formats

- [HashFS v2 archive reader](formats/hashfs-v2.md) - supported archive format, API, validation and limitations of `roadpilot-map-archive`.

### Research

- [SCS map data inventory](research/scs-map-data-inventory.md) - the minimal ATS/ETS2 map data RoadPilot needs, with sources, priorities and open questions.

### Decisions

- [Architecture decision records](adr/README.md) - how decisions are recorded, and the index of all ADRs.

### Planning

- [Open questions](open-questions.md) - unresolved technical and legal questions.
- [Phase 0 plan](roadmap/phase-0.md) - small, testable steps to set up the project.

## Conventions

- All documentation, identifiers and code are written in English.
- Significant architectural decisions are recorded as ADRs before or together with the change that implements them.
- Documents describe planned behavior until the corresponding code exists. When code and documentation disagree, the documentation must be updated in the same change.
