# Open questions

This document tracks unresolved technical and legal questions. When a question is answered, record the answer here with a link to the ADR or document that settles it, and move it to the "Resolved" section.

## Technical questions

| ID | Question | Why it matters | Resolution path |
|---|---|---|---|
| T1 | Which SCS archive format variants are used by current and supported ATS and ETS2 versions? | Determines what `roadpilot-map-archive` must read. | Research and document in `docs/formats/` before Phase 1 implementation. |
| T2 | Which spline or curve types do roads and prefabs use, and how are lane positions derived from them? | Determines the geometry representation and the achievable lane accuracy. | Research, then a dedicated ADR on the spline representation. Status (Phase 1, Step 4A): roads run between two nodes whose positions and forward directions are confirmed; the curve between them is presumably a cubic Hermite spline, but its tangent length is not confirmed (G1 in [SCS road geometry](formats/scs-road-geometry.md)). |
| T3 | What are the world units and scale of map coordinates? Should the package keep native game coordinates or transform them? | Affects every geometric value in the package and how consumers interpret it. | Dedicated ADR on the coordinate system and units. Status (Phase 1, Step 4A): axes (+X east, +Y up, +Z south), fixed-point scale (÷ 256) and the 4000-unit sector grid are confirmed; the unit itself is not (see [SCS road geometry](formats/scs-road-geometry.md)). |
| T4 | In which order are base game and DLC archives applied, and how are overrides resolved? | A wrong order produces wrong or missing roads. | Research, then a dedicated ADR on DLC handling. |
| T5 | Which data encoding should the package use, and which language does the consuming RoadPilot application use? | The encoding must be easy and efficient to read for the consumer. | Clarify consumer requirements, then a dedicated ADR on the package format. |
| T6 | How are package versions defined, and how do schema changes stay compatible with consumers? | The package is the stable contract with RoadPilot. | Dedicated ADR on the versioning scheme. |
| T7 | What coverage and performance targets apply (full map build time, memory use, package size)? | Guides data structures and optimization effort. | Agree targets with the RoadPilot application's needs. |
| T8 | Which game versions are supported, and how are unsupported versions reported? | Game updates may change file formats. | Define a support policy once Phase 1 is working. |
| T9 | Should mod maps ever be supported? | Mods vary widely and increase complexity. | Out of scope for now; revisit after ETS2 support. |
| T10 | Which operating systems are supported for running the CLI? | Affects CI targets and installation discovery. | Decide in Phase 0 together with CI setup. |
| T11 | Which precision does driving assistance require for lane positions, widths and curvature? | Defines acceptance criteria for the geometry stage. | Agree requirements with the RoadPilot application. |

## Legal questions

| ID | Question | Why it matters | Resolution path |
|---|---|---|---|
| L1 | Do the SCS Software EULA and modding policy allow a third-party tool to read and extract map data from a user's local installation? | The whole processing model depends on this. | Legal review of the EULA and SCS modding policy. |
| L2 | Does derived data (lane geometry, network) count as an "extracted map dataset" under the no-redistribution rule? May users share their generated packages? | Decides whether packages may ever leave the user's machine. | Legal review. Until resolved, packages are treated as non-shareable. |
| L3 | Under which licenses are existing community documentation and tools for SCS formats published, and may they be used as references or dependencies? | Using them could impose license obligations. | Check each source's license before using it, and record the result. Recorded so far: ts-map (dariowouters/ts-map) is MIT and was used as a reference for the sector format (no code copied); TruckLib (sk-zk/TruckLib) is GPL-2.0 and was not consulted. See [SCS sector format](formats/scs-sector-format.md). |
| L4 | Which final software license should RoadPilot-Map use? | Needed before publishing crates or accepting outside contributions. | Decide after L1-L3, then record in a dedicated ADR. |
| L5 | Which dependency licenses are acceptable while the final license is undecided? | Some licenses could restrict the later choice. | Define a `cargo-deny` allow list in Phase 0 and revisit after L4. |
| L6 | How should the SCS trademarks (ATS, ETS2) be referenced in documentation and the CLI? | Avoid implying an official relationship. | Follow SCS guidelines; keep the existing trademark notice. |

## Resolved

None yet.
