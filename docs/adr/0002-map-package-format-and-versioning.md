# ADR 0002: Map package format and versioning

- **Status:** Accepted
- **Date:** 2026-11-04
- **Supersedes:** -
- **Superseded by:** -

## Context

RoadPilot needs a stable, versioned contract for map display, map matching and route planning ([open questions T5, T6](../open-questions.md)). Phase 1 already produced a versioned JSON Lines export of the navigation graph ([Navigation graph export](../navigation/navigation-export.md)). What is missing is geometry: node data, road curves, lanes with their evidence level, junctions and a way to find roads near a position.

Constraints: map data is derived from the user's local installation and is not distributed ([ADR 0001](0001-rust-library-and-cli-with-local-offline-generation.md), L2). Several geometric values are not known (curve G1, unit T3, lane widths and order LM2/LM3), so the format must carry the evidence level of every value instead of a bare number. The consuming application's language and performance needs (T5, T7) are not specified yet.

## Decision

RoadPilot-Map will write a **package folder** with `manifest.json`, `geometry.jsonl` and optionally the existing `navigation.jsonl`.

- **Encoding:** UTF-8 JSON, with JSON Lines for the large files. The manifest is a single JSON object. The format is specified in [Map package](../formats/map-package.md).
- **One contract:** the navigation export keeps its own schema and version and is referenced by the manifest, not copied. The geometry file is keyed by the same game UIDs. The package does not repeat connection edges.
- **Versions:** `schema_version.major.minor` decides compatibility (readers reject another major, accept any minor). `content_revision` signals a change of the generation logic, `content_id` identifies one exact package. The generator version is informational only.
- **Evidence levels:** every geometric value is `confirmed`, `estimated` or `unavailable`. Estimated and unavailable values are never a basis for active steering. Unknown values are `null` or absent, never zero.
- **Determinism:** no time stamps, ordered records, shortest round-trip number text.
- **Integrity:** sizes and FNV-1a 64 checksums in the manifest find accidental damage; they are not authentication.
- **Spatial queries:** no index file. The reader builds a uniform grid from the road boxes. A box is the bounds of the estimated curve and gives candidates only.
- **Crates:** `roadpilot-map-geometry` (curve and lane assumptions, depends on `roadpilot-map-model`) and `roadpilot-map-package` (format, writer, reader, index, depends on `roadpilot-map-model` only). The shared type `Quality` lives in the model. No new external dependency.
- **Local use only:** packages are marked `distribution: local_use_only`.

## Alternatives considered

| Alternative | Reason not chosen |
|---|---|
| Binary format (for example FlatBuffers or a custom layout) | Needs a layout specification and a reader per language, and the consumer's needs are unknown (T5, T7). Can be added as a second encoding of the same records if a measured need appears |
| One JSON document | Cannot be streamed; hundreds of MiB |
| SQLite or another database file | Adds a dependency and a native library; harder to inspect and to keep deterministic |
| Extend the navigation export with geometry | Mixes two contracts with different change rates, and would force a major version of the export that other tools already read |
| Copy the navigation graph into the package | Two descriptions of the same edges could disagree |
| Store curve sample points | Larger, and presents an estimate as data; the model parameters let a consumer sample at any resolution and know what the points are |
| Store a spatial index in the package | Adds a second representation that can disagree with the roads; the grid is cheap to build |
| A default lane width | Invents a value. The format says `unavailable` instead |

## Consequences

### Positive

- Consumers can tell confirmed, estimated and missing values apart, per value.
- The navigation export stays valid and unchanged.
- No external dependency; the reader is small enough to port.
- Packages are reproducible and can be checked for damage and for agreement between files.

### Negative

- Text files are large (about 450 MiB of geometry per game). They compress well but must be stored, or a binary encoding must be added later.
- The reader loads the geometry file completely; memory and load time are not yet measured against targets.
- The checksum is no protection against deliberate changes.
- Spatial results are candidates from an estimated curve, not matches.

### Follow-up work

- Measure reader memory and load time and agree targets (T7).
- Decide with the RoadPilot application whether it reads JSON directly or needs a binary encoding (T5).
- Add confirmed lane geometry once lane widths and the lane order are established (LM2, LM3, T11).
- Replace the curve estimate once the game's curve is understood (G1).
