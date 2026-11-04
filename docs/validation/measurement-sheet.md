# Measurement sheet for in-game template sessions

Status: **ready for use, no real measurement recorded** (Phase 1, Steps 12 and 13). This document belongs to the [road template validation](road-template-validation.md). It explains how to plan, record and convert one in-game session. Nothing here confirms a lane width, an axis or the direction of template x. The best automatic result remains `CriteriaMetPendingReview`.

## Files

| File | What it is |
|---|---|
| [`templates/measurement-sheet-ets2.csv`](templates/measurement-sheet-ets2.csv), [`templates/measurement-sheet-ats.csv`](templates/measurement-sheet-ats.csv) | Blank sheets: markings `centre` and `edge_right`, 3 measuring points, 1 pass per alignment (24 planned observations). **No values.** Copy one per session, outside the repository |
| [`examples/synthetic-measurement-sheet.csv`](examples/synthetic-measurement-sheet.csv) | A filled sheet with **invented** values, marked `synthetic: yes`. For learning the layout and for the tests; never a measurement |
| `roadpilot-map measurement-sheet` | Writes a blank sheet with other markings, points or passes |
| `roadpilot-map convert-measurement-sheet` | Converts a filled sheet into the measurement format (schema 1.1) |
| `roadpilot-map check-measurements` | Checks measurement files; with `--package` it evaluates them and applies the criteria C0–C7 |
| `roadpilot-map measurement-candidates` | Lists straight roads per template from a map package as **unverified** candidates |
| `roadpilot-map import-snapshot` | Puts one RoadPilot-Telemetry snapshot line into one planned row of a sheet (see [Taking a telemetry frame](#taking-a-telemetry-frame-snapshot-import)) |

Filled sheets and converted files of real sessions stay **outside the repository** (see [Privacy](#privacy)).

## Taking a telemetry frame: snapshot import

The evaluation needs, for every observation, the values of one telemetry frame: `placement.x`, `.y`, `.z`, `heading`, `pitch`, `roll`, `truck.speedMps`, and the frame reference `session` and `sequence`.

The RoadPilot-Telemetry console reader prints the newest frame as **one JSON line** when you press **Enter** (`roadpilot.telemetry.snapshot`, version 1; see `docs/FRAME-SNAPSHOT.md` in RoadPilot-Telemetry). `roadpilot-map import-snapshot` puts such a line into one planned row of your sheet, so you never copy numbers by hand:

```text
roadpilot-map import-snapshot <sheet.csv> --pass <N>                  # paste the line, then Enter
roadpilot-map import-snapshot <sheet.csv> --pass <N> --from <file>    # or read the first line of a file
```

What the import does:

| Rule | Behaviour |
|---|---|
| Accepted lines | Only valid JSON of kind `roadpilot.telemetry.snapshot`, version 1, with `status: "fresh"`, an empty `missing` list, protocol `roadpilot.telemetry` schema 2 and `pluginState: "running"`. `repeated`, `stale` and `no_frame` are refused: press Enter in the reader again |
| Required values | `session`, `sequence`, `game`, `gameVersion`, `placement.x`, `.y`, `.z`, `.heading`, `.pitch`, `.roll`, `speedMps`. A missing value or a value of the wrong type or out of the protocol's range is an error; nothing is ever set to 0 |
| Copied into the row of pass N | `frame_session`, `frame_sequence`, `x`, `y`, `z`, `heading`, `pitch`, `roll`, `speed_mps`. Each number keeps its value exactly (written as the shortest text of the same `f64`; only the notation can change, for example `1e-5` → `0.00001`). No rounding, no coordinate conversion |
| Session row | `telemetry_game_version` is set from `gameVersion` and `local_scale` from `localScale` if they are empty; if they hold other values, the import refuses (one session, one SDK version). `gameVersion` is the **SDK telemetry version**, not the game patch: `game_patch` stays a manual entry |
| Checks | The snapshot's game must equal the sheet's game. The session's `synthetic` cell must already say `yes` or `no`; the import never changes it, so a frame never makes a synthetic session real or the other way round. Exactly one observation row must have pass N; its frame and placement cells must be empty (`--replace` overwrites them). The same frame may fill only one row |
| Never set | `criterion_met`, `uncertainty_m`, `notes`, `template`, `road`, `look`, `variant`, candidate `template_x`, `game_patch`. These are your observations and checked metadata |
| Writing | The whole sheet is written to `<sheet>.partial` and moved into place. If the import refuses, the sheet is unchanged. All other lines stay byte for byte |

A warning is printed if `speedMps` is above 0.05 m/s (the observation would not be used) or if the reader dropped invalid fields.

What the import does **not** prove: a snapshot gives the position of an undocumented reference point of the truck in the SDK's world space. It does not establish which point that is, that SDK and map coordinates share one basis, or where a lane boundary lies. Those questions are what the four-way design and the acceptance criteria are for.

## Choosing roads and templates

A template is **not** called suitable before it was checked in the game. The selection only narrows the search:

1. Build a package: `roadpilot-map build-package --game <ets2|ats> --game-dir <path> --out <folder> --without-navigation`. Note its `content_id`.
2. List candidates: `roadpilot-map measurement-candidates --package <folder> --game-dir <path> --per-template 3`. A road is listed if its look has a template, its estimated curve turns by at most 1°, its length agrees with the stored length, and it is at least 60 units long. The output gives the template, the number of matching roads, the road UID, look, length, the midpoint (x, z) and the heading of the stored road direction in telemetry turns. On the current local installations, 173 ETS2 templates and 106 ATS templates have at least one such road (measured with the packages of 2026-11-04; counts change with game updates).
3. Prefer templates with many matching roads (common roads), and different templates per game (C2 needs three in each game).
4. In the game, check each candidate before measuring and reject it if any of these holds:
   - the markings are not clearly visible, worn, or interrupted at the measuring points;
   - the road slopes or banks noticeably (`pitch` or `roll` clearly not 0);
   - a junction, merge, bridge joint or other road piece lies within 10 units of a measuring point;
   - traffic makes stopping on both sides of a line unsafe.
5. Write down only checked roads in the session row. Rejected roads are not recorded.

Finding the road in the game: drive towards the listed midpoint using the console reader's rounded `x`/`z` (enough to within a few metres). The road is the one whose direction matches the listed heading (0 = north, 0.25 = west, 0.5 = south, 0.75 = east, counterclockwise). The **forward** direction of the sheet is that heading; **backward** is the opposite.

## Step by step in ETS2 and ATS

The procedure is the same in both games. Use a truck **without a trailer**.

1. **Start the session.**
   - Copy the blank sheet for the game, outside the repository.
   - Fill in the session row: `game`, `game_patch` (as shown in the game), `telemetry_game_version` (`gameVersion` of the telemetry), `package_content_id`, `template`, `road`, `look`, `section` (your id, for example `s1`), `trailer` = `no`, `local_scale` (from the telemetry), `synthetic` = `no`.
   - Fill in the candidate rows: the template x of each marking from the template study (`template_x`). These are **derived** values.
2. **Mark three measuring points** along the road, at least 15 units apart and at least 10 units from either end. Use fixed landmarks (a sign, a post, a crack in the asphalt) so that you find them again; write only short ids (`p1`, `p2`, `p3`) in the sheet.
3. **For each planned row** (marking, point, alignment):
   - Approach from at least 30 units before the point, in the row's `direction`, straight and parallel to the road. One approach is one `pass`. Do not correct sideways while standing; start a new pass instead.
   - Stop so that the outer edge of the row's `tyre` (left or right in the travel direction) is on the row's `marking_part`.
   - **Stop safely:** in ETS2 and ATS, use a road with little traffic, hazard lights on, parking brake engaged, standing still. On edge lines you stand on the shoulder; do not measure where the game's traffic could hit the truck.
   - Look along the tyre with the exterior camera and judge the alignment. Enter `criterion_met`: `yes` if the tyre edge is on the marking part, `no` otherwise (keep the row).
   - Wait until the speed shows 0. Press **Enter** in the telemetry reader, copy the printed line, and run `roadpilot-map import-snapshot <sheet.csv> --pass <N>` with the row's pass number; paste the line and press Enter. Check the printed frame reference. (Copying the nine values by hand is still possible: `frame_session`, `frame_sequence`, `x`, `y`, `z`, `heading`, `pitch`, `roll`, `speed_mps`, exactly as given, dot as decimal separator.)
   - Enter `uncertainty_m`, your standard uncertainty of the alignment (typically 0.05 to 0.10).
   - Optional `notes`: a short remark on a deviation, for example `tyre slightly over line`. No commas, quotes, paths, links or `@`.
4. **Leave unmeasured rows empty.** They are skipped on conversion.
5. **Convert and check:**

   ```text
   roadpilot-map convert-measurement-sheet <sheet.csv> --out <session.jsonl>
   roadpilot-map check-measurements --package <folder> <session.jsonl> [<more sessions>...]
   ```

   The check lists per marking the line position, its uncertainty and the residuals for both directions of template x, then the criteria C0–C7 and the verdict.
6. **Repeat** for at least three templates per game. After a game update, sessions of the older patch cannot be combined with a new package (`content_id` differs).

## Sheet layout

Plain CSV, UTF-8. Lines starting with `#` are comments. The first other line is the column header; cells are separated by commas, quotes are not allowed. Empty cells mean "not given".

| Row `record` | Columns used | Required |
|---|---|---|
| `session` (exactly one, first) | `game`, `game_patch`, `telemetry_game_version`, `package_content_id`, `template`, `road`, `look`, `variant`, `section`, `trailer`, `local_scale`, `synthetic`, `notes` | `game`, `template`, `road`, `look`, `synthetic` |
| `candidate` | `marking`, `marking_part`, `template_x` | `marking`, `marking_part`; rows without `template_x` are skipped |
| `observation` | `marking`, `marking_part`, `point`, `pass`, `direction`, `tyre`, `criterion_met`, `frame_session`, `frame_sequence`, `x`, `y`, `z`, `heading`, `pitch`, `roll`, `speed_mps`, `uncertainty_m`, `notes` | If any of `x`, `y`, `z`, `heading` is filled: all four, plus `marking`, `marking_part`, `pass`, `direction`, `tyre`, `criterion_met`, `uncertainty_m`; `frame_session` and `frame_sequence` together |

Values: `game` `ets2`/`ats`; `synthetic`, `trailer`, `criterion_met` `yes`/`no`; `direction` `forward`/`backward`; `tyre` `left`/`right`; `marking_part` `centre`/`inner_edge`/`outer_edge`; ids (`marking`, `point`, `section`) lower-case letters, digits and `_`; `road` and `package_content_id` lower-case hexadecimal; `heading` in [0, 1) turns; `uncertainty_m` in (0, 1].

### Direct mapping to the measurement format

Every sheet field has a place in the measurement format; nothing is computed during the conversion.

| Sheet | Measurement format (schema 1.1) | New in 1.1 |
|---|---|---|
| session row | `header` (`method` is always `tyre_edge_alignment`) | `section`, `trailer`, `local_scale`, `notes` |
| candidate row | `candidate` record with `quality: "derived"` | the record type |
| observation row | `observation` record; `x`…`roll` in `placement`; `frame_session`/`frame_sequence` in `frame` | `point`, `frame`, `placement.pitch`, `placement.roll`, `speed_mps`, `criterion_met`, `notes` |

All schema 1.1 additions are optional; schema 1.0 files read unchanged (`criterion_met` then counts as `yes`). The reader of 1.0 ignores the new fields and the `candidate` records, as its compatibility rules require. The evaluation leaves out observations with `criterion_met: no` and those with `speed_mps` above 0.05 m/s, and reports how many it left out. Two observations with the same frame reference are rejected.

## Privacy

The sheet and the measurement format are designed to hold no personal or machine data:

- No fields for names, accounts, profiles, save games, screenshots, file paths, routes, times of day or dates.
- `notes` are short (200 characters), single-line and may not contain `/`, `\`, `@`, quotes, commas or links. They are for deviations and review remarks only.
- The telemetry values copied are the truck's position and frame numbers of one moment, nothing else. The import takes only these nine values and the SDK version and scale from a snapshot line; it ignores everything else in it.

Still, a real session file comes from one person's game. Keep filled sheets, converted files and saved snapshot lines (for `--from`) out of the repository; only blank templates and the synthetic example are checked in. Never commit game files, extracted assets or telemetry recordings.

## What you still fill in during real sessions

Per session (ETS2 and ATS each, at least three templates per game):

- the game patch (by hand; it is not in the telemetry), the package `content_id`, and `synthetic` = `no`; `telemetry_game_version` and `local_scale` come from the first imported snapshot;
- the template, road UID and look of a road you **checked** in the game;
- the template x of each marking (from the template study; derived);
- for every measured row: one fresh snapshot imported with `import-snapshot` (frame reference, position, orientation, speed), then by hand `criterion_met`, your `uncertainty_m`, and a note if something deviated.
