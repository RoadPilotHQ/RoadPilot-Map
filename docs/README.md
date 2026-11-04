# RoadPilot-Map documentation

This folder contains the architecture documentation, decision records and plans for RoadPilot-Map.

> **Status: Phase 1.** The workspace, the development diagnostics, the HashFS v2 archive reader, the sector file parser, the road and node model, the lane model, the prefab and junction model, a first navigation graph with route search, an export of the navigation graph, an estimated road curve with explicit evidence levels and a versioned map package (schema 1) are implemented. Everything else described here is the planned design unless a document says otherwise.

## Contents

### Architecture

- [Overview](architecture/overview.md) - goals, non-goals, constraints and system context.
- [Crate structure](architecture/crates.md) - Cargo workspace layout and dependency rules.
- [Library and CLI](architecture/library-and-cli.md) - responsibilities and public surface of each.
- [Map data pipeline](architecture/pipeline.md) - stages from game files to a map package.

### Testing

- [Test and fixture policy](testing.md) - test categories, synthetic fixtures and game data restrictions.
- [Road template validation](validation/road-template-validation.md) - what telemetry can and cannot measure, the in-game measurement protocol, the measurement format, the evaluation and the acceptance criteria C0-C7 before any lane geometry may be modelled as confirmed.
- [Measurement sheet](validation/measurement-sheet.md) - capture sheet for in-game sessions, its conversion into the measurement format, road selection, step-by-step guide for ETS2 and ATS, privacy rules, and what still has to be filled in.

### Formats

- [Map package](formats/map-package.md) - the versioned package (manifest, geometry file, navigation file): specification, evidence levels, identifiers, spatial queries, validation, compatibility rules and limitations.
- [HashFS v2 archive reader](formats/hashfs-v2.md) - supported archive format, API, validation and limitations of `roadpilot-map-archive`.
- [SCS lane model](formats/scs-lane-model.md) - lanes per road from road looks, traffic side per country, driving directions, candidate lane transitions; confirmed and open lane questions.
- [SCS prefabs and junctions](formats/scs-prefab-junctions.md) - prefab descriptors (`.ppd`), prefab definitions and instances, road-to-prefab connections, internal lane movements, traffic rules and static traffic lights; confirmed and open junction questions.
- [SCS road templates](formats/scs-road-templates.md) - the `.pmd`, `.pmg` and `.mat` files of road templates and their readers: layouts, how materials and pieces connect, what the meshes and materials show, why no lane width or axis offset follows, and the open questions RT1-RT7.
- [SCS road geometry](formats/scs-road-geometry.md) - roads, nodes, their connections and the coordinate system; confirmed and open geometry questions.
- [SCS sector format](formats/scs-sector-format.md) - confirmed layout of sector `.base` and `.data` files, the parser in `roadpilot-map-formats`, and open format questions.

### Navigation

- [Navigation graph and route planning](navigation/navigation-graph.md) - the directed graph of lanes and prefab slots, connection status and origin, the two routing modes, the algorithmic cost model and measurements.
- [Navigation graph export](navigation/navigation-export.md) - the versioned file format (JSON Lines, schema 1) for external consumers: specification, identifiers, status semantics, compatibility rules, limitations and measurements.
- [Navigation graph connectivity audit](navigation/connectivity-audit.md) - why the graph has small connected pieces, classification of the causes, and the evidence for the lane direction rule (Step 6.1).

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
