# Validating road template geometry against in-game measurements

Status: **procedure and tooling defined, no real measurement made** (Phase 1, Steps 11 to 13; the capture sheet, its commands and the import of RoadPilot-Telemetry frame snapshots are in [measurement sheet](measurement-sheet.md)). The format reader, the evaluation and the acceptance check are in `crates/roadpilot-map/src/validation.rs` (`roadpilot_map::validation`). The only data in the repository is a synthetic example. **No lane width or axis position is confirmed**, and the map package keeps lane positions `unavailable` and widths `null`.

The question this procedure answers: do the marking positions read from a road template (`.pmd`/`.pmg`, see [SCS road templates](../formats/scs-road-templates.md)) match where the markings are in the running game, relative to the road's node curve? That would settle RT3 (is template x = 0 the node curve, and which way is right) for the measured templates, and give evidence for RT4 to RT6.

## What the telemetry can and cannot give

Source: RoadPilot-Telemetry, `docs/PROTOCOL-V2.md` (schema 2), read locally. No other interface is used.

| Field | What it is | Use here |
|---|---|---|
| `placement.x`, `.y`, `.z` | World position of the truck, metres, double precision, from the SDK channel `truck.world.placement` | **Measured value.** The position of a reference point of the truck |
| `placement.heading` | 0…1 turn, 0 = north, counterclockwise | **Measured value**, transformed into a travel direction |
| `placement.pitch`, `.roll` | Turns | Recorded only to reject frames on slopes or bumps |
| `game`, `gameVersion` | `ets2`/`ats`, the SDK telemetry version (not the game patch) | Provenance |
| `localScale` | Map scale applied to time and distance | Recorded; not used in the computation |
| `motion.localVelocityMps` | Vehicle-space velocity | Used by the observer to make sure the truck stands still |

What the telemetry does **not** give (from the same document):

- No road, lane or marking information ("Not available from the SDK").
- No statement **which point of the truck** `placement` is. Schema 2 does not forward the SDK's truck configuration with wheel, cabin or hook positions, and the SDK headers are not in this workspace. So the position of the tyres relative to `placement` is **unknown**.
- No statement that SDK world space equals map world space. Both are documented as X east, Y up, Z south ([SCS road geometry](../formats/scs-road-geometry.md#coordinate-system), protocol "Coordinate conventions"), and the SDK calls its unit metres; that map world units are metres is open question T3.

**Conclusion: telemetry alone is not enough.** It gives where a point of the truck is, not where that point is relative to a marking. The relation between the truck and a marking must come from an **independent observation in the game**.

## The independent reference

The observation is a **tyre edge on a marking**: the observer places the truck so that the outer edge of one tyre is exactly on a chosen part (centre, inner or outer edge) of a marking, the truck stands still, and the observer records the telemetry placement. This uses only what the game shows on screen. It is not taken from the template files, so it is independent of what it tests.

The unknown position of the placement point on the truck is removed by design, not assumed:

| Alignment | Truck travels | Tyre on the line | Placement offset from the line (positive = left of the stored road direction) |
|---|---|---|---|
| FL | forward (start → end node) | left | `−e_L` |
| FR | forward | right | `−e_R` |
| BL | backward | left | `+e_L` |
| BR | backward | right | `+e_R` |

`e_L` and `e_R` are the lateral positions of the left and right outer tyre edges relative to the placement point (unknown). The mean of the four kind means is the line position **without** `e_L` or `e_R`. The same data gives two independent estimates of the tyre width (`FR − FL` and `BL − BR`) and of the placement point's offset from the tyre centre. If they disagree, the data is wrong; this is criterion C5.

Driving on both sides of a line and in both directions is needed. For a centre line that is normal driving; for an edge line it means stopping on the shoulder or against traffic, which is acceptable in a controlled session but must be safe in the game (for example a quiet road). Partial sets are kept but never count (criterion C2).

### Alternative references

| Reference | Use | Status |
|---|---|---|
| Tyre edge alignment (above) | Default | Defined |
| SDK truck configuration (wheel positions) | Would give `e_L`, `e_R` directly. Not forwarded by schema 2; adding it is a change to RoadPilot-Telemetry, not done here | Not available |
| Known vehicle dimensions from a manufacturer | Only a cross-check of the tyre width estimate; the game model may differ | Optional |
| Developer camera or editor tools | Would need changes to the user's game configuration; not part of this procedure | Not used |

## Measurement protocol (draft)

Each session measures **one road of one template** in one game. Repeat for at least three templates per game (C2).

1. **Prepare.**
   - Build a map package of the installation (`roadpilot-map build-package`) and note its `content_id`.
   - Choose a road whose look has a `template_right`, whose estimated curve is straight (turn angle ≤ 1°, `length_consistent`), at least 60 units long, on level ground, with clearly visible markings. Note the road UID, the look and the template path from the package and the definitions.
   - Note the game patch shown in the game. Note `gameVersion` from the telemetry.
2. **Choose the markings** to measure, for example `centre` and `edge_right`, and the part of each (`centre`, `inner_edge`, `outer_edge`). Use the same ids as in the template candidates.
3. **Choose three measuring points** along the road, at least 15 units apart and at least 10 units from either end (away from junction transitions).
4. **For each measuring point, each marking and each of the four alignments (FL, FR, BL, BR):**
   - Approach from at least 30 units before the point, in the required direction, aligned with the road. One approach is one **pass**; never correct a position by moving sideways while standing.
   - Stop with the tyre edge on the marking part. Use the exterior camera looking along the tyre; record the uncertainty you see (typically 0.05–0.10 m).
   - Wait until `localVelocityMps` is about zero, then record one telemetry frame: `placement` and `heading`.
5. **Record** the session in the measurement format below. Do not store names, profile names, save games, screenshots, routes or other personal data.
6. **Repeat the session** after a game update: results are valid only for the recorded patch and package `content_id`.

## Measurement format

`roadpilot-map-template-measurement`, schema 1.1 (1.0 files read unchanged). UTF-8 JSON Lines: header, optional `candidate` records, observations, footer. Sessions are recorded on the [measurement sheet](measurement-sheet.md), which converts directly into this format; the optional 1.1 fields (`section`, `trailer`, `local_scale`, `notes` in the header; `point`, `frame`, `placement.pitch`/`roll`, `speed_mps`, `criterion_met`, `notes` per observation; `candidate` records with `quality: "derived"`) are listed there. Unknown record types and fields are ignored by the reader; a field that holds personal data is never part of the format, and must not be added.

Header:

| Field | Type | Meaning |
|---|---|---|
| `format`, `schema_version` | | `roadpilot-map-template-measurement`, `{major, minor}` |
| `game` | `ets2` \| `ats` | |
| `telemetry_game_version` | string or null | `gameVersion` of the telemetry |
| `game_patch` | string or null | The game patch shown in the game |
| `package_content_id` | hex string or null | `content_id` of the package used |
| `template` | string | `.pmd` path of the template |
| `road` | hex string | Road UID |
| `look` | string | Road look name |
| `variant` | string or null | Template variant, if identified (LM1); usually null |
| `method` | string | `tyre_edge_alignment` |
| `synthetic` | bool | `true` for invented data; such sets never count |

Observation:

| Field | Type | Meaning |
|---|---|---|
| `pass` | integer | Approach number |
| `direction` | `forward` \| `backward` | Relative to the stored road direction |
| `tyre` | `left` \| `right` | Outer tyre edge on the marking, seen in the travel direction |
| `marking` | lower-case id | For example `centre`, `edge_right` |
| `marking_part` | `centre` \| `inner_edge` \| `outer_edge` | |
| `placement` | `{x, y, z, heading}` | Raw telemetry values, unchanged |
| `uncertainty_m` | number in (0, 1] | Observer's standard uncertainty of the alignment |

Footer: `{"t":"footer","complete":true,"observations":N}`; schema 1.1 writers add `"candidates":M`.

Example: [`examples/synthetic-template-measurement.jsonl`](examples/synthetic-template-measurement.jsonl) (invented values, `synthetic: true`). The test `crates/roadpilot-map/tests/validation.rs` reads and evaluates it.

## Evaluation

For every observation (`roadpilot_map::validation::evaluate`):

| Step | Kind |
|---|---|
| `placement`, `heading` | Measured |
| Estimated road curve from the package (`validation::road_curve`) | Estimate (G1); used only on straight roads, where it equals the chord |
| Closest point on the curve, lateral offset positive to the left of the stored direction | Transformation |
| Travel direction from `heading` (0 = north = −Z, 0.25 = west) and its angle to the road | Transformation of a documented convention |
| Same space and unit for telemetry and map | **Assumption**, only checked for consistency (C4) |
| Placement point on the truck | **Not assumed**; cancelled by the four alignments |

Per marking part: mean and spread of each alignment kind, the line position (mean of the four means), its combined uncertainty `√Σ(sᵢ²/nᵢ + uᵢ²) / 4`, the two tyre width and two offset estimates, and the residuals against the template candidate under both hypotheses for the direction of template x (right or left of the stored direction).

The template candidate is the x of the marking read from the template. It is **derived**: which piece is which marking is read from effect and texture names, and the variant is usually unknown.

## Acceptance criteria

`roadpilot_map::validation::assess` applies these with the defaults below. Changing a limit needs a written reason in this document.

| ID | Criterion | Default |
|---|---|---|
| C0 | At least one set of real data; synthetic sets are ignored | — |
| C1 | Every set records the telemetry game version, the game patch and the package `content_id` | — |
| C2 | Complete sets for at least 3 different templates **in each game**; every marking with all four alignments and at least 3 passes each | 3 templates, 3 passes |
| C3 | Spread of each alignment kind ≤ 0.10 m, and combined uncertainty of each line ≤ 0.05 m | 0.10 m, 0.05 m |
| C4 | Straight road (turn ≤ 1°, length consistent) and every heading within 2° of the road | 1°, 2° |
| C5 | The two tyre width estimates and the two placement offset estimates agree within 0.10 m | 0.10 m |
| C6 | **One** direction of template x (right or left) fits every marking of both games within 0.15 m | 0.15 m |
| C7 | Independent human review and an ADR | Never met automatically |

Verdict: `InsufficientData` if C0, C1 or C2 fails; `Inconsistent` if C3, C4, C5 or C6 fails; otherwise `CriteriaMetPendingReview`. There is no "confirmed" verdict in the code.

Only after `CriteriaMetPendingReview` **and** C7 may an ADR decide to model the marking positions of the **measured templates** as confirmed. Other templates stay `unavailable`. The package's `content_revision` must then increase. A lane width is still a separate statement: it needs both boundary lines of a lane measured, and which line bounds which lane index (RT5, LM2) must be shown, not assumed.

Even then, nothing here supports active steering or a safety-critical lane departure warning. That needs requirements on accuracy (T11), which are not agreed.

## Limits of the method

- The observer's alignment is a visual judgement. 0.05–0.10 m is realistic; this limits what can be confirmed.
- The four-way design assumes the placement point is fixed on the truck and the truck is straight; articulation (trailer) does not matter if the truck stands still, but driving with a trailer makes alignment harder. Measure without a trailer.
- A measurement relates a marking to the **estimated** node curve. On straight roads the estimate is the chord between two confirmed node positions; that is why curves are excluded (C4).
- If telemetry and map positions do not share space and unit, C4 and C6 fail; the procedure cannot tell which of the two is wrong.
- Results are per game patch and package `content_id`.

## Open points

| ID | Status after this step |
|---|---|
| RT3 | Procedure defined (C6 decides the direction of x, the line position tests x = 0); no measurement yet |
| RT4 | Measuring a half template's markings with the same procedure would show how `road_offset` places it; not done |
| RT5 | Measuring both boundary lines of a lane gives its width between lines; which lane index it is remains open (LM2) |
| RT6 | Agreement under C6 would support unscaled placement in metres for the measured templates (T3); not done |
| — | Telemetry: the placement point on the truck is undocumented in schema 2; forwarding the SDK wheel positions would allow a direct check |
