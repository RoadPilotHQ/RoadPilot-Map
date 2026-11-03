# RoadPilot-Map documentation

This folder contains the architecture documentation, decision records and plans for RoadPilot-Map.

> **Status: early planning.** No source code exists yet. Everything described here is the planned design unless a document says otherwise.

## Contents

### Architecture

- [Overview](architecture/overview.md) - goals, non-goals, constraints and system context.
- [Crate structure](architecture/crates.md) - Cargo workspace layout and dependency rules.
- [Library and CLI](architecture/library-and-cli.md) - responsibilities and public surface of each.
- [Map data pipeline](architecture/pipeline.md) - stages from game files to a map package.

### Testing

- [Test and fixture policy](testing.md) - test categories, synthetic fixtures and game data restrictions.

### Decisions

- [Architecture decision records](adr/README.md) - how decisions are recorded, and the index of all ADRs.

### Planning

- [Open questions](open-questions.md) - unresolved technical and legal questions.
- [Phase 0 plan](roadmap/phase-0.md) - small, testable steps to set up the project.

## Conventions

- All documentation, identifiers and code are written in English.
- Significant architectural decisions are recorded as ADRs before or together with the change that implements them.
- Documents describe planned behavior until the corresponding code exists. When code and documentation disagree, the documentation must be updated in the same change.
