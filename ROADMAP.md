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

**3D viewport (M6 groundwork)** — `cadcraft-shot demo3d`: 1,002,264 triangles / 83,522 instances at x = 500 000,
y = 4 000 000 draw in 284–308 ms per orbit frame on this server's software rasteriser (no GPU here); the CPU side of an
orbit frame is one uniform write (instances uploaded once per scene).

## Progress log

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
