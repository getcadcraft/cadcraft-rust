# CadCraft in Rust: one page

Stage: M4 done (drafting: command line, LINE … TRIM, undo, Edit + Save as DWG / DXF, desktop + web, live at
https://rust.getcadcraft.com, unlisted). M5 next (re-planned in [docs/roadmap.md](docs/roadmap.md)).
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
| 3.4 MB R2013 drawing | save DWG 2018 | 942 | 686 | 1.4× |
| 3.4 MB R2013 drawing | save DWG 2000 | 706 | 623 | 1.1× |
| 3.4 MB R2013 drawing | save DXF 2000 | 483 | 702 | **0.7× (slower)** |

Editing a big drawing (Rust only; the JS app has no comparable headless path): Edit on the 3.4 MB drawing = read +
import 406 ms, Doc load 325 ms, display model 127 ms; one LINE until the redrawn frame 360 ms, of which ~200 ms is the
software rasteriser's frame (no GPU here); 1.7 MB floor plan 226 ms; 0.9 MB sheet 188 ms. Every edit rebuilds the
whole display model (incremental rebuild is M5 work).

## Progress log

- 2026-10-16 M4: drafting (Doc, geometry, command line, commands, undo), DWG writer, CadCraft's DWG/DXF import and DWG/DXF save at byte parity; Edit + Save as in the app; deployed.
- 2026-10-16 M3: viewer (model + sheets with viewports, pan/zoom, layers, texts, xrefs) on desktop and web (WebGPU only), deployed.
- 2026-10-16 M2: cc-dwg reader in Rust at full-model parity (78/78), 2.6–4.2× faster than 0.5 JS; fuzz target; CLI `info`, `convert --json`, `parity`, `bench`.
- 2026-10-15 M1: workspace, lints, xtask gate.
