# SCS road templates (`.pmd`, `.pmg`, `.mat`) and lane widths

Status: **feasibility study with narrow readers** (Phase 1, Steps 9 and 10). The readers are `roadpilot_map_formats::pmg` (`parse_pmg`: parts, pieces, vertex positions), `roadpilot_map_formats::pmd` (`parse_pmd`: looks, variants, part visibility, material paths) and `roadpilot_map_formats::mat` (`parse_mat`: effect, substance, texture sources). The archive access is `Archive::read_primary_record` in `roadpilot-map-archive`. **No lane width, lane offset or road-axis offset is derived or exported.** The study shows what the templates contain and why that is not yet enough.

Evidence levels used here:

- **Confirmed:** stated by a specification or reliable reference, or follows from the format itself.
- **Measured:** holds for all or a stated share of the inspected files. A fact about the data, not about its meaning.
- **Derived:** a conclusion from measured facts and an assumption that is not confirmed.
- **Unknown:** not established.

## Sources

| Source | What it gives |
|---|---|
| SCS modding wiki, [Documentation/Engine/Formats](https://modding.scssoft.com/wiki/Documentation/Engine/Formats) | `.pmd` is the "Prism model descriptor" and `.pmg` the "Prism model geometry", both binary. **No byte layout** for either. A separate wiki page lists the changes between PMG versions `0x506d6713` and `0x506d6714`; its body was not retrieved. |
| Local ETS2 and ATS installations (read-only) | All findings below. Reproduce them with the opt-in tests `crates/roadpilot-map/tests/real_road_templates.rs` and `crates/roadpilot-map/tests/real_road_template_materials.rs` (see [Reproducing](#reproducing)). |
| Third-party parsers and format notes | **Not used.** Their licences (for example GPL) were not cleared, and the repository's rule is not to take over code or text before that ([L3](../open-questions.md)). The layouts below are derived from the files alone. |

No game file, extract or hex dump is in the repository. The numbers below are counts and percentages.

Data basis (current): 454 ETS2 templates, and 228 ATS templates of the ATS version installed by the game launcher on 2026-11-04 (222 before that update; the earlier `.pmg` figures were measured on those 222 and were the same in structure).

## Where the geometry comes from

A road look that has `template_right` (342 ETS2 and 178 ATS looks; `template_left` on 140 and 52) takes its **visible geometry** from the template. The look units themselves hold no widths for these looks (see [SCS lane model](scs-lane-model.md#widths-and-offsets)).

| Fact | Evidence |
|---|---|
| `template_right` names a `.pmd` (for example `/road_template/sc/road_1_scan.pmd`), found under `road_template/` in the base and DLC archives | Measured |
| Each `.pmd` has a `.pmg` of the same stem in the same folder: 454 of 454 (ETS2), 228 of 228 (ATS) | Measured |
| All `.pmd` files have version 4, all `.pmg` files have version `0x506d6715` | Measured, both games |
| The `.pmd` is the descriptor (looks, variants, part visibility, material files), the `.pmg` the meshes | Confirmed by the wiki (names) and measured (layouts below) |

The chain from a look to a triangle, measured on all templates of both games:

```text
road look (def)  ──template_right──►  .pmd ──same stem──► .pmg
.pmd  look index × material slot  ──►  material file path  ──►  .mat (effect, substance, textures)
.pmd  variant × part              ──►  visible 0/1
.pmg  part  ──first piece, count──►  pieces  ──material slot──►  .pmd material table
.pmg  piece ──►  vertex positions
```

## Reading the archive entries

Every `.pmg` entry has **two** metadata records: the plain data record and one extra record of type 6 (ordinary files, including `.pmd` and `.mat`, have only the plain record). The archive reader refused such entries before Step 9 ([HashFS v2](hashfs-v2.md)).

- The plain data record is stored with zlib. It inflates to exactly its declared size, with a valid Adler-32 checksum, for all files. **Measured.**
- The meaning of the type 6 record is **unknown** and it is not read. `Archive::read_primary_record` returns the data record of an entry that has a plain record plus records of type 5, 6 or 7, and refuses everything else. `read_file` still refuses these entries.
- Whether the extra record changes how the game interprets the geometry (for example a mip or level-of-detail table) is unknown. All findings below are about the plain data record.

## `.pmd` layout (version 4)

All values little-endian `u32` unless stated. Every row holds for all 454 ETS2 and 228 ATS descriptors (**measured**); the reader rejects any file where one does not.

Header, 16 words:

| Word | Meaning | Evidence |
|---|---|---|
| 0 | Version 4 | Measured |
| 1 | Material slot count | Measured: the material table has looks × slots entries; every `.pmg` piece's material number is below it |
| 2 | Look count | Measured |
| 3 | Unknown | Always 0 |
| 4 | Variant count | Measured |
| 5 | Part count | Measured: equals the `.pmg` part count in every pair |
| 6 | Attribute count | Measured |
| 7 | Bytes of attribute values per variant | Measured |
| 8 | Bytes of the material path block | Measured: the block ends exactly at the end of the file |
| 9–15 | Offsets of: look names, variant names, part attribute ranges, variant values, attribute definitions, material table, material path block | Measured: each table starts exactly where the previous one ends, in the file order look names (at 64), variant names, part ranges, attribute definitions, variant values, material table, paths |

Tables:

| Table | Record | Evidence |
|---|---|---|
| Look names | `u64` token, for example `light_gray`, `gray_dk` | Measured: all decode as tokens |
| Variant names | `u64` token, for example `no_lines`, `lane_broken`, `double_nor` | Measured; the same names appear in `road_template_variant` units of the definitions |
| Part attribute ranges | start and end index into the attribute definitions | Measured: contiguous from 0, covering all attributes |
| Attribute definitions | `u64` token name, `u32` type, `u32` byte offset in the variant values | Measured: every attribute is `visible`, type 0, one per part (4,614 ETS2, 6,146 ATS before the update) |
| Variant values | one block per variant | Measured: the `visible` values are only 0 or 1 |
| Material table | per look and slot the absolute file offset of a path | Measured |
| Material paths | NUL-terminated paths of the form `/automat/<xx>/<16 hex digits>.mat` | Measured. The **names are hashes**: they say nothing about what the material is |

So the descriptor answers *which parts are visible in which variant* and *which material file a slot uses under which look*. It holds **no** geometry, no part names (those are in the `.pmg`), no locators and no road-axis information. **Measured.**

## `.pmg` layout (version `0x506d6715`)

| Offset / field | Meaning | Evidence |
|---|---|---|
| 0 | Version `0x506d6715` (the bytes `15 67 6d 50`) | Measured |
| 4 | Number of pieces | Measured |
| 8 | Number of parts | Measured: equals word 5 of the `.pmd` |
| `0x48` | Start of the part table; records of 24 bytes | Measured |
| part record | `u64` token name, piece count, first piece, then two words | Measured: the parts cover all pieces in order without gap in every file. The last two words are 0 except for 10 ATS parts that have a count of 1 |
| `0x50`..`0x54` | A block between the part table and the piece table | Measured: empty in all ETS2 files; 44 bytes in the 10 ATS files with such a part (a token and numbers that look like a position at the origin and a rotation). Meaning **unknown**, presumably a locator; not read |
| `0x54`, `0x58` | Start and end of the piece table; end = start + pieces × 100 | Measured |
| piece record word 0, 1 | Index count, vertex count | Measured |
| piece record word 4 | Material slot of the `.pmd` | Measured: always below the slot count |
| piece record words 9–14 | Bounds minimum and maximum corner (`f32`) | Measured: equal to the min and max of the piece's vertices for every piece within 0.001 |
| piece record word 15, 16 | Vertex stride (36 or 44 bytes seen), offset of the position stream | Measured |
| vertex | Position as three `f32` (x, y, z) at `stream + vertex × stride` | Measured |

Not read: normals, texture coordinates, colours, indices, skinning, the block at `0x50` and every other word. The reader checks the version, the part and piece tables, every stream against the file length, counts against limits, and that numbers are finite.

Totals: ETS2 4,614 parts, 4,886 pieces, 171,566 vertices; ATS 6,173 parts, 7,373 pieces, 598,042 vertices. All parse.

The coordinate system of a template is: the road runs along **−Z** (a segment spans z from −15 to 0 in all shapes seen, 30 or 45 with `template_repeat`), **x** is across the road, **y** up. Whether x grows to the right of the road direction is not checked against the sector coordinate system. **Unknown.**

## `.mat` material files

Every material path in every descriptor exists in the archives (591 distinct files in ETS2, 529 in ATS) and is a short **text** file of the form `effect : "<effect>" { … texture : "<slot>" { source : "<path>" … } }`. All of them parse with `parse_mat`. **Measured.**

| Content | Values seen | Evidence |
|---|---|---|
| Effect (shader) | Families `eut2.dif` (most), `eut2.none`, `eut2.shadowonly`, in ATS also `eut2.retroreflective`; many effect names contain `decal` | Measured |
| `substance` | ETS2: `road` (331 materials), `road_dirt` (116), `dirt` (12), `gravel` (6), `rumble_stripe` (2), `grass` (1), `metal` (1), none (122). ATS: `road` (238), `road_dirt` (45), `dirt` (21), `rumble_stripe` (4), `static` (1), none (220) | Measured. No substance names a lane, a shoulder or a sidewalk |
| Texture sources | Paths such as `/material/road/road_template/sc/asphalt_light_gray.tobj` or `/material/special/lines_eu_01.tobj` | Measured |

What the `substance` value does in the game is not documented here; that it is a surface type for the physics or sound is plausible from its values but **not confirmed**.

## Joining the three files

For each template, a piece's material is found through its material slot in the `.pmd` under a look; whether the piece is drawn depends on the variant through its part's `visible` value. In ETS2 2,166 of 4,614 parts and in ATS 1,334 of 6,173 parts change visibility between variants, so the **variant decides which markings exist**, and the variant a road uses is not known (LM1).

`road_1_scan` under its first look, as an example:

| Pieces | Slot | Material | x positions |
|---|---|---|---|
| 0, 1 | 0 | `substance: road`, asphalt textures | −5.5, −4.5, 0, 4.5, 5.5 |
| 2–13 | 1 | decal effect, texture `lines_eu_01` | strips at ±4.5 and around 0 |
| 14 | 3 | decal effect, `decal_ground_01` | scattered |
| 15 | 2 | `substance: road`, two asphalt texture sets | −5.5…5.5 |
| 16, 17 | 4 | decal effect, `substance: road_dirt` | ±0.095 |

## Do materials tell lane, marking, shoulder and sidewalk apart?

| Question | Answer | Evidence |
|---|---|---|
| Are material **names** meaningful? | **No.** They are hashes (`/automat/…`) | Measured |
| Can line markings be found? | **Derived, not confirmed.** Pieces whose material has a `decal` effect and a texture whose path contains `line`: 1,514 pieces in ETS2, 948 in ATS (first look). This reads texture names; it does not say which line is a lane boundary, an edge line, a stop line or a hatch, and the variant decides which are drawn | Measured counts, derived meaning |
| Are lane surfaces and shoulders different materials? | **No, not in general.** Of the templates with line markings beyond 0.5 units from the axis, a single non-decal `road` material reaches both inside and beyond the outermost line in 73 of 167 (ETS2) and 73 of 122 (ATS) | Measured. Where the materials differ, it is not shown that the difference is lane against shoulder |
| Are sidewalks in the templates? | **Not identifiable.** No substance or effect names a sidewalk; sidewalks are usually separate edge models (`compatible_edges_*`), which are not read | Measured / Unknown |
| Do the files hold an axis or `road_offset`? | **No.** The `.pmd` holds no geometry; `road_offset` exists only in the road look definitions. The only position-like data is the unknown 44-byte block of 10 ATS `.pmg` files, at the origin | Measured |

## What can be said about lane width and axis offset

| Quantity | Status | Why |
|---|---|---|
| Overall width of a template mesh (bounds, surface break lines) | **Measured** | Directly in the file, for all templates |
| Material class of a piece (decal or surface, substance, texture name) | **Measured** | Through the `.pmd` material table and the `.mat` files |
| Which parts a variant shows | **Measured** | `visible` attribute; the variant a road uses is unknown (LM1) |
| Position of the road axis in template coordinates (x = 0 for symmetric templates) | **Derived** | Centre lines and the symmetric break lines sit at 0 in the examples; no source says "x = 0 is the road axis" |
| Lane width **4.5** for ordinary roads | **Derived, not confirmed** | From break lines and from `road_size` of 1-lane looks. The rule holds for about 70 % (ETS2) and 33 % (ATS) of the eligible looks (below). No file says "this is a lane" |
| Width of an individual lane of a multi-lane road | **Unknown** | Line decals exist at multiples of 4.5 in examples, but nothing ties a line to a lane boundary or to `lanes_right[i]` |
| Which pieces are lane surfaces, shoulders or sidewalks | **Unknown** | Materials do not separate them (above) |
| `road_offset` | **Unknown** | Only in the look definition; no reference geometry to tell how it places a half template |
| Unit | **Unknown** | T3; and whether templates are placed unscaled was not verified |

Lane width study (unchanged by the `.pmd` decoding):

| Observation | Evidence |
|---|---|
| The non-template looks with 1 lane per side have `road_size` 4.5 per side; 2 lanes per side have 8.0, 12.5 or 14.5; 3 lanes 17 | Measured. So `road_size` is **not** lanes × 4.5 |
| For 43 of 59 eligible ETS2 looks and 6 of 18 eligible ATS looks, `(outer x − shoulder_space_right) / lanes_right` is 4.50 | Measured. Eligible: a right-side lane count above 0, a known `shoulder_space_right`, a symmetric or right-half template and no `template_repeat`. The rest give other values (6.7–6.9 in ETS2, 5.25–6.75 in ATS) |
| Vertices exist at every multiple of 4.5 up to the lane count for 33 of 59 (ETS2) and 12 of 18 (ATS) eligible looks | Measured |

**Conclusion:** the three files are now decoded far enough to name every piece's material and visibility. They still do not let us derive a **reliable** lane width or axis offset. Material classes are not lane boundaries. The package keeps `lane_position: unavailable`, `width: null`. Do not use 4.5 as a default.

## Next steps that could change the answer

1. Find a reference that fixes x = 0, the right-hand direction, `road_offset` and the lane indices for a few templates: an official statement, or an independent measurement of a road in the game compared with its template. Only that can turn a derived value into a confirmed one.
2. Relate variants to roads (LM1): without it, the markings a road shows are unknown.
3. Read the edge models (`compatible_edges_*`, `add_parts_*`) if sidewalks or shoulders matter.
4. Work out the type 6 record and the 44-byte `.pmg` block.
5. Only then add lane widths to the model with a `confirmed` or `derived` quality per look, and agree the needed accuracy with the application (T11).

## Reproducing

```powershell
$env:ROADPILOT_MAP_GAME_DIR = "C:\Program Files (x86)\Steam\steamapps\common\Euro Truck Simulator 2"
cargo test --release -p roadpilot-map --test real_road_templates -- --ignored --nocapture
cargo test --release -p roadpilot-map --test real_road_template_materials -- --ignored --nocapture
```

The first test checks that every `.pmd` has a `.pmg`, that every `.pmg` parses and its stored bounds equal its vertex bounds, and prints the lane-width study. The second parses every `.pmd`, checks it against its `.pmg` (part counts, contiguous parts, material slots, visibility for every part and variant), reads and parses every referenced `.mat`, and prints the material statistics and the edge test. Both read the archives only and copy nothing. Run them with the ATS folder as well.

## Open questions

| ID | Question | Status (Step 10) |
|---|---|---|
| RT1 | The structure of the `.pmd` (version 4). | **Decoded for road templates:** looks, variants, part visibility, material table and paths. Header word 3 (always 0) is unknown. Other model kinds may use more of the format |
| RT2 | The meaning of the type 6 (and 5, 7) metadata record of `.pmg` entries. | Open |
| RT3 | Whether x = 0 of a template is the road axis, and which direction is right. | Open; the `.pmd` holds no axis data. Step 11 defines an in-game measurement procedure and acceptance criteria ([validation](../validation/road-template-validation.md)); no measurement yet |
| RT4 | The meaning of `road_offset` for template looks, and how a half template is placed. | Open; `road_offset` is not in the template files. Can be measured with the same procedure on a half template; not done |
| RT5 | Which pieces are lane surfaces, markings, shoulders and sidewalks, and how they relate to `lanes_left[]` and `lanes_right[]`. | Partly: line decals can be found by effect and texture name (derived); lane surface and shoulder often share one material; no link to lane indices. Measuring both boundary lines of a lane gives a width between lines, not the lane index |
| RT6 | Whether template geometry is used unscaled in the game (units, T3). | Open; agreement in the validation (criterion C6) would support it for the measured templates |
| RT7 | The 44-byte block between the part and the piece table of 10 ATS `.pmg` files, and the last two words of their part records. | Open |
