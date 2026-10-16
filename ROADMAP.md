# CadCraft in Rust: one page

Stage: M3 done (2D viewer, desktop + web, live at https://rust.getcadcraft.com, unlisted). M4 next (drafting). Milestones: [docs/roadmap.md](docs/roadmap.md).
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

## Progress log

- 2026-10-16 M3: viewer (model + sheets with viewports, pan/zoom, layers, texts, xrefs) on desktop and web (WebGPU only), deployed.
- 2026-10-16 M2: cc-dwg reader in Rust at full-model parity (78/78), 2.6–4.2× faster than 0.5 JS; fuzz target; CLI `info`, `convert --json`, `parity`, `bench`.
- 2026-10-15 M1: workspace, lints, xtask gate.
