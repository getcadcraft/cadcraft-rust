# CadCraft in Rust: one page

Stage: M5 done (speed debt, the m3d 3D kernel); the M7/M8 engines done ahead of plan (cc-ifc, cc-manifold booleans,
cc-skp, all at parity); M6 ModelCraft in progress (3D viewport live at https://rust.getcadcraft.com/?demo3d, unlisted).
Plan: [docs/roadmap.md](docs/roadmap.md).
Session log: [docs/progress.md](docs/progress.md).

## Headline numbers

**DWG reading (cc-dwg, `crates/io-dwg`) vs the CadCraft 0.5 JavaScript `readDwg`** — 2026-10-16, `tools/fork-parity`
`bench.mjs` (warm, best of 5, engine call on in-memory bytes, same machine; shared 8-core VM under load ~4.5, so
medians are noisy). Parity = whole-model JSON against the JS reference (kit tolerance abs 1e-9 / rel 1e-12).

| File | Version | Size (MB) | 0.5 JS (ms) | Rust (ms) | Speed-up | Parity |
|---|---|---:|---:|---:|---:|---|
| example-2007.dwg | AC1015 (R2000) | 0.64 | 18.7 | 7.21 | 2.60× | same |
| blocks-and-tables-imperial.dwg | AC1015 (R2000) | 0.63 | 68.2 | 16.3 | 4.18× | same |
| example-2010.dwg | AC1015 (R2000) | 0.63 | 32.0 | 7.69 | 4.16× | same |
| example-2000.dwg | AC1015 (R2000) | 0.55 | 41.9 | 12.4 | 3.39× | same |
| kitchen-equipment-r2000.dwg | AC1015 (R2000) | 0.55 | 52.4 | 17.6 | 2.97× | same |
| kitchen-equipment.dwg | AC1015 (R2000) | 0.55 | 62.2 | 24.0 | 2.59× | same |
| example-2010.dwg | AC1021 (R2007) | 0.43 | 29.3 | 8.45 | 3.46× | same |
| example-2007.dwg | AC1021 (R2007) | 0.43 | 27.1 | 9.19 | 2.95× | same |

Larger client drawings (outside the kit's corpus, not in any repo; `cadcraft-cli bench` vs `js-cli.mjs bench`, best of 5):
a 3.39 MB R2013 drawing (58,989 objects) 446 ms JS → 148 ms Rust (3.0×); a 1.66 MB floor plan 180 → 52.6 ms (3.4×).
Single-threaded Rust (`RAYON_NUM_THREADS=1`, what a web build without wasm threads gets): 249 ms and 30.6 ms (1.8–2×).

Parity: **78 / 78** corpus files match (all six versions AC1015–AC1032 + `_fixtures/objects`), plus every other DWG in
`testfiles` (13 `orig/`, 18 others incl. the client sets): 0 differing paths.

**2D viewer (M3)** — 2026-10-16. Compile step (BimCraft's `dwg-compile.js`) and DXF reader (`dxf-model.js`) at parity:
0 differences on 78 corpus DWGs, 19 client DWGs and 106 DXFs (compile output and DXF model). Compile vs JS: 3.4 MB
drawing 168 → 84 ms, 1.7 MB 32 → 22 ms.

| Measurement | Value |
|---|---|
| Web (Chromium, wasm, single thread, fat LTO + SIMD): read the sample (174 KB R2018), 6 runs | Rust 38–53 ms warm (first 119–254) vs 0.5 JS 46–90 ms warm (first 117–130) in the same browser |
| CPU side of a frame (egui + view + text layout), 3.4 MB drawing, 1600×1000 | 0.72 ms best, 0.95 ms median |
| Whole frame incl. software GPU (lavapipe) + readback, 3.4 MB drawing | 132 ms best (no hardware GPU on this server) |
| Kitchen sample, whole frame on lavapipe | 42 ms best |

**Drafting (M4)** — 2026-10-16. Parity against the frozen 0.5 JavaScript, no tolerance unless noted:

| Part | Harness (outside the repo) | Result |
|---|---|---|
| Doc + geometry (doc.js, geom.js, dims, walk, linetypes, hatch …) | `gdiff.mjs --exact` | 0 differences, ~18,000 cases per seed, seeds 1–10; doc ops 121/121 steps per seed |
| Command line + commands (LINE … TRIM, UNDO, ZOOM) | `kdiff.mjs` | 0 differences, 26 seeds, ~70,000 records |
| DWG writer (writeDwg) | `wdiff.mjs` | byte-identical 468/468 (78 files × 6 versions), 72/72 synthetic models |
| DWG import, DXF R12 read/write | `idiff.mjs` | 0 differences: 78 kit DWGs, 32 other DWGs, 110 DXFs |
| DWG save (saveDwg) 2000–2018, DXF 2000 | `idiff.mjs --save` | byte-identical, reports equal: 78/78 per version (+ other DWGs, DXFs, fixture, 15 dynamic-block variants) |

Timings (best of 5, single-threaded, same machine; JS = the 0.5 engine call in Node 24):

| File | Step | 0.5 JS (ms) | Rust (ms) | Speed-up |
|---|---|---:|---:|---:|
| kitchen-equipment.dwg (0.55 MB) | write DWG (AC1015) | 151 | 76 | 2.0× |
| 3.4 MB R2013 drawing | import (importDwg, after the read) | 350 | 179 | 2.0× |
| 3.4 MB R2013 drawing | save DWG 2018 | 856 | 592 | 1.45× (M5.1; was 1.4×) |
| 3.4 MB R2013 drawing | save DWG 2000 | 752 | 501 | 1.5× (M5.1; was 1.1×) |
| 3.4 MB R2013 drawing | save DXF 2000 | 453 | 357 | 1.26× (M5.1; was 0.7×) |

(M5.1 rows measured the same minute for both sides on a loaded machine; saves stay byte-identical on every file.)

Editing a big drawing (Rust only; the JS app has no comparable headless path): Edit on the 3.4 MB drawing = read +
import 406 ms, Doc load 325 ms; since M5.1 an edit walks only the objects it changed (display rebuild 127 → ~38 ms);
one LINE until the redrawn frame ~270 ms, most of it this server's software rasteriser.

**The 3D model kernel (M5, cc-m3d = ModelCraft's model.js)** — `mdiff.mjs --exact` (frozen JS `Model` vs `cadcraft-cli
m3d`, zero tolerance): 640/640 op records + 1,247/1,247 function cases per seed, seeds 1–10. Speed (best of 5):

| Case | 0.5 JS (ms) | Rust (ms) | Speed-up |
|---|---:|---:|---:|
| 2,000 push/pulls (100 boxes) | 2919 | 619 | 4.7× |
| 200×200 grid of lines | 8522 | 1020 | 8.4× |
| load of a 49,928-face model | 579 | 121 | 4.8× |
| 150 rectangles, 150 undos, 150 redos | 629 | 147 | 4.3× |

**Booleans (cc-manifold = Manifold 3.5.4 ported to Rust)** — identical output to the manifold-3d 3.5.4 wasm the JS uses
on 11,093 cases (9,990 random, 1,102 recorded from the IFC corpus, a 20k-triangle wall with 200 openings: same counts,
order, run ids, bit-equal floats); 1.80× the wasm natively in total, 1.59× compiled to wasm32.

**IFC (cc-ifc, `crates/io-ifc`)** — kit parity: ifc-step **118/118** (exact), ifc-geom **115/115** (booleans included).

| ifc-step | Size (MB) | 0.5 JS (ms) | Rust (ms) | Speed-up | Parity |
|---|---:|---:|---:|---:|---|
| Molio_with_URIs.ifc | 70.52 | 1638 | 217 | 7.54× | same |
| IFC_Schependomlaan_geo.ifc | 47.00 | 1120 | 143 | 7.85× | same |
| Clinic_Structural.ifc | 18.18 | 419 | 67.1 | 6.24× | same |

| ifc-geom (with booleans on both sides) | Size (MB) | 0.5 JS (ms) | Rust (ms) | Speed-up | Parity |
|---|---:|---:|---:|---:|---|
| Clinic_Structural.ifc | 18.18 | 1130 | 248 | 4.55× | same |
| Duplex_MEP_20110907.ifc | 17.04 | 800 | 101 | 7.93× | same |
| Clinic_Architectural.ifc | 12.40 | 615 | 95.1 | 6.47× | same |
| 1807_EP_AR_v18.ifc | 11.55 | 825 | 143 | 5.78× | same |
| AC20-Institute-Var-2.ifc | 10.43 | 284 | 80.1 | 3.54× | same |
| AC-20-Smiley-West-10-Bldg.ifc | 5.83 | 368 | 61.3 | 6.00× | same |

**SketchUp (cc-skp, `crates/io-skp`)** — kit parity **27/27** (canonical JSON byte-identical); 1,296 damaged copies give
the JS's output or error. Beats both cc-skp JS and OpenSKP (OpenSKP numbers from the 2026-10-15 measurement in
`docs/forks/README.md`; no build on this machine):

| File | Size (MB) | 0.5 JS (ms) | Rust (ms) | Speed-up | OpenSKP C++ (ms) |
|---|---:|---:|---:|---:|---:|
| coedge_orientation_regression | 5.25 | 284 | 37.4 | 7.6× | 630 |
| pimeta-v2-pcb (v6) | 2.06 | 52.7 | 8.05 | 6.6× | 47 |
| slot_boundary | 0.78 | 42.7 | 4.20 | 10.2× | 34 |
| gondola_v20 | 0.74 | 46.9 | 3.31 | 14.2× | 20 |
| CAD_furniture_set_large (31 MB, outside the corpus) | 31.2 | 942–1149 | 155–200 | ≈6× | — |

**ModelCraft interaction and formats (M6)** — `tdiff.mjs --exact`: 19 sections 1,281/1,281 records, seeds 1–10
4,708/4,708; `iodiff.mjs`: 801/801 byte-identical. Infer at 1,000 cursor positions on a 50k-face model 163 → 56 ms
(2.9×), pick 36 → 19 ms; export GLB 403 → 37 ms (10.9×), STL 506 → 33 ms (15×), import SKP 5.2 MB 2,508 → 428 ms
(5.9×), import OBJ 3.6 MB 1,194 → 220 ms.

**ModelCraft panels (M6.17)** — `mcuidiff.mjs` (10 inputs × 5 seeds × 80 panel actions): 4,000 / 4,000 status messages
and 4,076 / 4,076 saved files byte-identical with 0.5. `mcuidiff.mjs --bench` (best of 3, JS / Rust ms; this server
under load, so ±30 %): m3d-50k-faces open 1,603 / 1,299, panel edits 1,893 / 206 (9×), save 287 / 18 (16×), 3D-print
check 1,894 / 523 (3.6×); coedge_orientation_regression.skp (5.5 MB) open 13,217 / 6,091 (2.2×), edits 7,777 / 494
(16×), save 1,077 / 190 (5.7×), print check 40,000 / 9,589 (4.2×); pimeta.modelcraft open 97 / 322 (0.3×: the app also
builds the view's meshes and Tools, the JS timing is parse + load only), save 7.7 / 4.4, print check 7,091 / 1,497.

**3D viewport (M6 groundwork)** — `cadcraft-shot demo3d`: 1,002,264 triangles / 83,522 instances at x = 500 000,
y = 4 000 000 draw in 284–308 ms per orbit frame on this server's software rasteriser (no GPU here); the CPU side of an
orbit frame is one uniform write (instances uploaded once per scene).

## Progress log

- 2026-10-21 M4.21: PUBLISH (several sheets in one PDF) and pictures in plotted PDFs (pictures.js pdfImage, the Plot dialog's dpi cap) at kdiff parity (80/80); plot number text, zlib-rs and parallel sheets: PUBLISH 3.0–5.7× the JS (was 0.6–0.8×); plotdiff 1128/1128.
- 2026-10-21 M6.26: ModelCraft scenes' Play (every 3.2 s, Stop / Esc); mcuidiff --edit --lights --play 7,040/7,040 status lines, 6,691/6,691 files identical to 0.5.
- 2026-10-21 M6.25: ModelCraft Render a picture… — Fast (GPU accumulation: jittered camera, sun disc and sky light shadow-mapped, lamps, ACES) and High quality (compute path tracer over a box tree); cleanSettings 3,000/3,000 as 0.5.
- 2026-10-21 M6.24: ModelCraft Lights panel and lamp editor (the Light tool, lamp marks picked in the view); mcuidiff --edit --lights 7,040/7,040 status lines, 6,704/6,704 files identical to 0.5; an undo dropping the open group no longer selects it.
- 2026-10-21 M6.23: ModelCraft material textures drawn by code (textures.js ported, texdiff 180/180 byte-identical, 1.1× the JS) in the 3D view and the Materials swatches.
- 2026-10-21 S.4–S.8: block lists shared copy-on-write, undo / redo that move objects, clipboards without copies: Carey XATTACH 0.6× → 1.8× the JS, SELECT ALL + MOVE 1.1× → 1.6×, EXPLODE ALL 0.9× → 1.5×, ERASE ALL + UNDO 0.8× → 1.7×; ModelCraft undo 0.3–0.5× → 3.5–4.8×; TakeoffCraft copy + paste in place 0.4× → 0.7×; DXF 2000 save 1.0× (unchanged); kdiff / idiff / tkopsdiff / mdiff / tdiff / mcuidiff as before.
- 2026-10-21 M6.22: ModelCraft sun shadows in the 3D view (shadow map + shadow-catching ground, as view3d.js); +6 ms a frame on the 50k-face model.
- 2026-10-21 M6.21: ModelCraft components that stretch (Stretch this copy… dialog, part rules per axis with a repeated part's spacing); mcuidiff --edit full sweep 7,040/7,040 status lines and 6,651/6,651 saved files identical to 0.5; stretching 6–8× faster than 0.5.
- 2026-10-21 M16.8: dates 0.5 writes in local time are local in the port too (zip dates, KISS, LandXML, MarkupCraft lists) through cc-clock (jiff); mkuidiff / mkdiff / todiff / cvuidiff / cvdiff / dcuidiff / tkopsdiff 0 differences under UTC, Denver, Kolkata, Auckland and at DST changes.
- 2026-10-21 M11.16: GeoTIFF reads capped at 1 GB of cells (was 3.2 GB): civil_raster fuzz OOM fixed, regression test + seed.
- 2026-10-21 S.1–S.3: number text without the 1100-digit expansion (cc-takeoff num_str, cc-cad to_fixed) and a typed GLB writer: TakeoffCraft marked PDF 0.3× → 4.5× the JS, CivilCraft section sheets PDF 0.4× → 1.4×, TakeoffCraft 3D + GLB 0.9× → 2.0×; tkopsdiff 61/61, 262/262; cvtooldiff 76/76 projects, 2084/2084 status lines, 20/20 files (section sheets PDF included).
- 2026-10-20 M6.20: ModelCraft solids (union, subtract by two clicks, intersect), intersect faces, IFC storeys dialog, drawing axes (place, align to face, reset); saved files identical to 0.5 (mcuidiff --edit).
- 2026-10-20 M6.19: ModelCraft copy / cut / paste (the copy follows the pointer) / paste in place, duplicate and move, flip and flipped copy, 3D text (text3d.js on a ported Clipper2 polytree: CrossSection Simplify / Decompose, csdiff 2,000/2,000 bit-identical); saved files identical to 0.5 (mcuidiff --edit).
- 2026-10-20 M6.18: the m3d_tools fuzz timeout in add_edge fixed: past 1,024 edges with a NaN end (left by splitting a zero-length edge, doubled by every edge drawn, as in the frozen JS) addEdge stops splitting them; below the cap the results stay the JS's (mdiff seeds 1–10, tdiff, mfdiff identical).
- 2026-10-20 M9.19: MarkupCraft BCF export / import, compare picture overlay, nudge / align line-up and whole-set compare; mkuidiff full sweep 17/17 scenarios and 194/194 status lines identical to 0.5.
- 2026-10-20 M9.18: MarkupCraft cut-outs, spaces, legends, sheet links (PDF links on save) and the punch list; title blocks read at open as 0.5 does; mkuidiff 15/15 scenarios identical; editing actions 1.2–17× faster than 0.5.
- 2026-10-20 M9.17: MarkupCraft picture and signature stamps, the signature pad, reply photos and the tool chest (presets, import / export); pictures read under `image::Limits` (fuzzed); mkuidiff chest + pictures identical to 0.5, 28/28 status lines.
- 2026-10-20 M9.16: MarkupCraft copy / paste, the Markups list (filters, order, jump, CSV / XLSX / PDF summary), replies, sheet rename and the Pages menu; mkuidiff 10/10 scenarios with 104/104 status lines identical to 0.5.
- 2026-10-20 M4.20: BVSTATE BVHIDE BVSHOW (visibility states of a dynamic block in BEDIT; references follow) at kdiff parity (78/78); final sweep kdiff 5073/5073, idiff --xrefs 624/624 byte-identical.
- 2026-10-20 M4.19: IMAGEATTACH IMAGE IMAGECLIP at kdiff parity (58/58); pictures (PNG / JPEG, image crate with limits, fuzz target cad_raster) drawn in the window and the viewer; placed pictures save byte-identically.
- 2026-10-20 M4.18: XATTACH XREF (? Attach Reload Bind Detach, Bind clipped part) XCLIP at kdiff parity (135/135 scenario records); the linked and clipped drawings save byte-identically (idiff --xrefs); walk clips XCLIP'd references (screen, plots, extents); files come through Host::pick_drawing / the window's picker.
- 2026-10-20 M4.17: UCS PLAN TURNLEFT TURNRIGHT TURNWORLD, LWDISPLAY POLARANG, COUNT at kdiff parity (2002/2002 scenario records; random seeds 3/4/7 2800/2800 each); the window turns its view to the UCS and has polar tracking (F10); every new command faster than 0.5.
- 2026-10-19 M4.16: LAYOUT MVIEW VPORTS MSPACE PSPACE VPLAYER VIEWLABEL NAMEDVIEW at kdiff parity; sheets made by them save byte-identically in DWG 2000–2018, DXF 2000 and DXF R12 (idiff --sheets 624/624); LAYOUT 2.1–3.6× faster than 0.5.
- 2026-10-19 M4.15: clipboard (COPYCLIP CUTCLIP COPYBASE PASTECLIP PASTEORIG), QSELECT SELECTSIMILAR FIND with the Quick select and Find dialogs, UNITS; kdiff scenarios 1928/1928; every new command 1.6–9.7× faster than 0.5 on Carey and the floor plan.
- 2026-10-19 M4.14: objects' extra data (xdata, unknown codes) shared copy-on-write in cc-cad; SELECT ALL + MOVE on the 9k floor plan 219 → 15 ms (JS 42), ARRAY 211 → 26, MATCHPROP 110 → 22, ERASE + UNDO 29 → 8; every drafttime row now faster than 0.5; kdiff / propsdiff / plotdiff / mcpdiff / idiff --save unchanged.
- 2026-10-18 M4.12: Plot to PDF (plot.js) in the window, the CLI and MCP `drawing_plot`; 1128/1128 kit plots + 32/32 byte-identical to 0.5 (plain streams).
- 2026-10-18 M4.11: Properties panel (propdefs.js / propexpr.js), each edit one undo step; 720/720 records identical to 0.5.
- 2026-10-18 M4.10: HATCH (pick inside), DIM family, FILLET / CHAMFER, STRETCH, ARRAY, BLOCK / INSERT / BEDIT, MATCHPROP and 40 more commands; 8,600 + 1,386 command-line records identical to 0.5.
- 2026-10-18 M13.4: PartCraft rebuilds over 60 ms on a thread (newest part wins); configurations follow parameter edits, Configurations panel; 257 steps / 27 files byte-identical
- 2026-10-18 M13.3: PartCraft sketcher (every 0.5 sketch tool, relations, dimensions, live drag) and feature tools (extrude … shell, edge / face picking, edit, delete, move, rename); pcuidiff scenarios: 239 steps and 22 files byte-identical to 0.5
- 2026-10-18 M14.3: DetailCraft modelling tools in the app (placing with snapping, connections, clipboard, transforms, windows / lasso, grids, levels, phases, holes, copes, Settings, plan / elevation); 558/558 status lines and files identical to the frozen 0.5 doing the same; 1.5–2.9× the JS on a 2820-member model.
- 2026-10-18 M6.17: ModelCraft's panels (Entity info, Outliner, Components, Colors, Materials, Tags, Style + sun, Scenes, 3D-print check) and inference keys in the app; 4,076 / 4,076 saved files and 4,000 / 4,000 status lines byte-identical to 0.5 after the same actions.

- 2026-10-20 M7.15: BimCraft library items placed from the plan (ifc-place.js in cc-bim; turn, wall snap, typed X, Y, the item-number dialog, one undo step each); bmlibdiff 188/188 engine steps and 16/16 IFC on IFC2X3 / IFC4 / IFC4X3, app 73/73 status lines and 8/8 IFC + plan PDFs identical to 0.5; letting go 3.1× the JS.
- 2026-10-20 M7.14: BimCraft plan grips (wall ends and middles with the wall tool's snapping, doors and windows slid along or onto walls, one undo step per drag); bmgripdiff 248/248 status lines, 28/28 saved IFC and plan PDFs identical to 0.5; drag preview 2.9–3.7×, letting go 2.2–3.3× the JS.
- 2026-10-19 M11.14: CivilCraft lasso (Shift+drag that bends; window / crossing / fence, Space cycles) and the point settings (row order, renumbering, start); cvtooldiff 76/76 projects, 2084/2084 status lines, 20/20 files identical to 0.5; lasso 5.2× the JS on a 500k survey.
- 2026-10-19 M11.13: CivilCraft Properties edits: names, corridor and superelevation, pad elevation and slopes, structures, pipes and inverts, feature-line elevations / grades / raise / offset, parcel courses, points, project and network rules (civil.prop); identical to 0.5 in cvtooldiff.
- 2026-10-19 M11.12: CivilCraft feature lines in the profile (WL.20) and the curve / vertical-curve "why" fixes with the design minimum radius (WL.23); identical to 0.5 in cvtooldiff.
- 2026-10-19 M11.11: CivilCraft cross-section sheets (PDF) and mass haul from the alignment's Sample lines (dialogs, diagram, CSV); cvtooldiff 76/76 projects, 2084/2084 status lines, 20/20 files (PDFs inflated) identical to 0.5.
- 2026-10-18 M11.10: CivilCraft survey points: Shift+click / box / Ctrl+A selection, copy, cut, paste, duplicate, Delete; 52/52 projects byte-identical to 0.5.
- 2026-10-18 M11.9: CivilCraft section panel and sample lines; 48/48 projects and 4/4 section CSVs byte-identical to 0.5.
- 2026-10-18 M11.8: CivilCraft profile panel (ground, design, curves, PVI drag / add / delete, PI and PVI tables typed); 44/44 projects byte-identical to 0.5.
- 2026-10-18 M11.7: CivilCraft drawing tools (alignment, pad, pipes, breakline, boundary, feature line, parcel), traverse and deed-calls dialogs, grips and Delete in the app; 40/40 projects byte-identical to 0.5 after the same clicks; 1.8–2.8× the JS on a 500k-point survey.
- 2026-10-18 M9.15 Several PDFs per plan set (MarkupCraft + TakeoffCraft) and Compare sheets by changed lines with Cloud the changes; saved files identical to the 0.5 JS.
- 2026-10-18 M9.14 MarkupCraft: every 0.5 drawing tool, select / move / grips, text editor and Properties panel in the app; saved PDFs identical to the 0.5 JS (16/16); open 40-270×, save 10-24× the JS.
- 2026-10-18 M9.13 TakeoffCraft panels in the app (item settings, assemblies, bid areas + alternates, deducts, slopes, cut-outs): 12/12 exports identical to the 0.5 JS after 166 edits.
- 2026-10-18 M13/M14/M15: PartCraft view (rebuild kernel, history, parameters, exports at byte parity), DetailCraft view (properties, reports, clash, every export at parity), PartCraft's mesh kernel on cc-manifold (M12) at exact parity; wasm threads built and measured slower (allocator lock), not shipped.
- 2026-10-17 M8–M11: TakeoffCraft, MarkupCraft, BimCraft, CivilCraft, DetailCraft and PartCraft engines at exact parity; PDF sheet view with markup and takeoff tools, exports and Auto count; CivilCraft view and exports at parity.
- 2026-10-16 M6/M7 apps started: ModelCraft's tools (exact parity, 6,000 records) and formats (801/801) in Rust and in the app (edit, open SKP/OBJ/GLB/DAE, export 8 formats); BimCraft's IFC 3D view with picking, properties, storeys, section cut.
- 2026-10-16 M5 + engines: DXF 2000 save 1.26× JS, per-object edit display cache, golden references in CI, nightly fuzz OOM fixed; cc-m3d (ModelCraft's kernel) at exact parity; cc-skp 27/27, cc-ifc 118/118 + 115/115, cc-manifold identical to the wasm on 11,093 cases; 3D viewport (cc-render mesh3d, ui view3d) on desktop and web.

- 2026-10-16 M4: drafting (Doc, geometry, command line, commands, undo), DWG writer, CadCraft's DWG/DXF import and DWG/DXF save at byte parity; Edit + Save as in the app; deployed.
- 2026-10-16 M3: viewer (model + sheets with viewports, pan/zoom, layers, texts, xrefs) on desktop and web (WebGPU only), deployed.
- 2026-10-16 M2: cc-dwg reader in Rust at full-model parity (78/78), 2.6–4.2× faster than 0.5 JS; fuzz target; CLI `info`, `convert --json`, `parity`, `bench`.
- 2026-10-15 M1: workspace, lints, xtask gate.
