# Roadmap: CadCraft in Rust

Last updated 2026-10-18 · Change: all eight engines at parity; PartCraft and DetailCraft views; wasm threads measured and not shipped (M15.1).

| Date | Change |
|---|---|
| 2026-10-15 | First version (milestones from the kick-off prompt). |
| 2026-10-16 | M1, M2 done; M2 numbers in ROADMAP.md. |
| 2026-10-16 | M3 done: viewer on desktop + web (WebGPU), live at rust.getcadcraft.com. |
| 2026-10-16 | M4 done: drafting, Edit + Save as DWG / DXF at byte parity; M5+ re-planned (below). |
| 2026-10-16 | M5 done; cc-ifc, cc-manifold, cc-skp at parity (the engines of M7/M8, run in parallel); M6 started (3D viewport). |
| 2026-10-18 | PartCraft (M13) and DetailCraft (M14) views; wasm threads built and measured slower (M15.1). |
| 2026-10-17 | M9+ engines at parity (TakeoffCraft, MarkupCraft, BimCraft, CivilCraft, DetailCraft, PartCraft without its rebuild kernel); app views for ModelCraft, BimCraft, PDF sheets with MarkupCraft and TakeoffCraft, CivilCraft. |

Speed decides. Every milestone is measured against the CadCraft 0.5 JavaScript build (reference:
`getcadcraft.com/public_html/assets/apps/<app>/dev/`, `lib/{dwg,ifc,skp,shapes}`, `docs/apps/*.md`) with
`tools/fork-parity` once forks-infra ships it, and with `cargo xtask bench` / `cargo xtask parity` until then.
Parity on the corpus is the gate (AGENTS.md section 15).

| # | Milestone | Ends with | Status |
|---|---|---|---|
| M1 | Workspace + gate | crate layout, lints, clippy.toml, deny.toml, xtask (ci, layers, wasm, assets, deny, fuzz --smoke), rust-toolchain.toml, CI workflow; `cargo xtask ci` green on the empty skeleton | done 2026-10-15 |
| M2 | cc-dwg reader (`crates/io-dwg`) | parity with `lib/dwg` readDwg on the corpus (six versions + ODA truth), one fuzz target, `cadcraft-cli info` / `convert --json`, benchmark vs JS readDwg on the largest files; parity numbers + timings | done 2026-10-16: 78/78 parity, 2.6–4.2× |
| M3 | CadCraft 2D viewer | open DWG/DXF, model + sheets, pan/zoom, layers; desktop and web (WebGPU only); deployed to rust.getcadcraft.com via forks/deploy.sh; frame-time benchmark on the big drawings | done 2026-10-16 |
| M4 | CadCraft drafting commands | command engine with commands that ask questions (LINE ... TRIM), undo, save DWG/DXF at parity with dwg-save.js | done 2026-10-16: 0 differences (gdiff, kdiff, idiff), saves byte-identical 78/78 per version |
| M5 | Speed debt + m3d kernel | (a) incremental edit display, typed fast path under the dynamic models, DXF 2000 save at least at JS speed; (b) m3d (CadCraft's 3D kernel, `assets/apps/cadcraft/dev/m3d/`, ≈13,000 lines JS) as `crates/m3d` (L2) at parity on its tests + golden meshes, Manifold behind `trait SolidOps` | done 2026-10-16: m3d exact parity (seeds 1–10), DXF 2000 save 1.26×, edit cache; Manifold ported to pure Rust instead of a C++ binding (identical to the wasm, works on the web) |
| M6 | ModelCraft | ModelCraft (≈36,000 lines JS) on m3d: 3D viewport (wgpu, BVH picking, inference), push/pull … , SKP import through M8's reader once it lands; frame time on a 1M-triangle model | in progress: kernel, tools (exact parity) and formats (801/801) done; the app edits and exports models on desktop + web; next: the panels (outliner, entity info, tags, scenes, materials), components, render, print check |
| M7 | cc-ifc + BimCraft | `lib/ifc` (≈6,300 lines, 22 files) at parity on the IFC corpus with the kit, then BimCraft's views on it (≈54,000 lines JS: plan from the DWG work, 3D from M6) | engine done 2026-10-16 (118/118, 115/115 with booleans); app started: IFC 3D view, picking, properties, storeys/classes, section cut |
| M8 | cc-skp | `lib/skp` (≈2,500 lines) at parity; must beat both cc-skp JS and OpenSKP (black-box baseline) on the big files | reader done 2026-10-16 (27/27, 5–14× JS, faster than OpenSKP on every file); import into ModelCraft with M6 |
| M9+ | The other apps | PartCraft, CivilCraft (large coordinates), DetailCraft, TakeoffCraft, MarkupCraft, then Studio; order re-planned after M6 with measured numbers | engines: TakeoffCraft 18,442 records, MarkupCraft 30,994, BimCraft 38,788 cases, CivilCraft 67,101 calls (4 accepted), DetailCraft 55,714 calls, PartCraft 239,669 records, all 0 differences; PartCraft's rebuild kernel on cc-manifold in progress (M12). App views: see below |

Task ids after M8, as they were used (some numbers overlap the milestone table; the commit titles are the record):
M8.6–M8.8 TakeoffCraft exports and Auto count in the app; M9.4 CivilCraft engine; M9.6–M9.12 PDF sheet view, markups,
MarkupCraft tools (two commits carry M9.12: TakeoffCraft mode and the measuring tools); M10.1–M10.6 PartCraft engine;
M10.20–M10.23 DetailCraft engine; M11.x CivilCraft app view; M12.x cc-manifold API + PartCraft kernel.

Next (2026-10-18): CivilCraft drawing tools, BimCraft plan / draw / IFC save, MarkupCraft and TakeoffCraft UI (agents); Studio;
web speed through fewer allocations in the readers (wasm threads measured slower, below).

## Tablets (AGENTS.md section 10, Mobile)

CadCraft 0.5 Studio runs on Android and iPad. eframe's Android/iOS support is limited, so the fork covers tablets
through the **web build in the tablet's browser** first (WebGPU: Chrome on Android, Safari 26+ on iPad): touch layout
(pinch/pan, long press = Enter, 40 px hit areas), browser file picker and download, recovery snapshots in OPFS on
`visibilitychange`. A native Android/iOS shell is re-planned after M3 with measured numbers; until then the fork
makes no tablet promise beyond "the web build in a WebGPU browser".

## Known issues

- Accessibility (screen readers) is skipped by the owner's decision (2026-10-15); see `docs/gaps.md`.

## Wasm threads (M15.1, measured 2026-10-18): built, measured, not shipped

Built in a scratch copy: the web crate with `+atomics,+bulk-memory,+mutable-globals,+simd128`, `-Z build-std=std,panic_abort`
on a pinned nightly (nightly-2026-10-14), link args `--shared-memory --import-memory --max-memory=4294967296` plus the
`__wasm_init_tls` / `__tls_size` / `__tls_align` / `__tls_base` exports (this nightly does not add them for
wasm32-unknown-unknown), `wasm-bindgen-rayon` 1.3 (`no-bundler`) started before the app when `crossOriginIsolated`.
egui-wgpu 0.37 / wgpu 30 build without `fragile-send-sync-non-atomic-wasm`. The readers' parallel paths ran only inside pool
jobs (`rayon::current_thread_index()`), since the browser's main thread may not block; a DWG open became a pool job polled
per frame. rust.getcadcraft.com already sends COOP/COEP.

Measured in headless Chromium (8 threads), the 174 KB sample, reader only, no app running:

| Reads | ms |
|---|---:|
| 1 read on the pool | 53 |
| 2 reads at once | 317 |
| 4 reads at once | 518 |
| 8 reads at once | 1,147 |

The readers allocate heavily, and every allocation in a shared-memory wasm goes through std's one global allocator lock,
so the work scales **negatively**: the threaded build opened the 3.4 MB Carey drawing in 954–3,971 ms against 383–1,569 ms
single-threaded, and single-threaded reads are also slower in the atomics build (50–173 ms vs 36–46 ms). A thread-caching
allocator needs `unsafe` (`GlobalAlloc`), which the workspace forbids, and no maintained pure-Rust one for wasm was found.
Revisit when one exists or when the readers allocate less; until then the web build stays single-threaded, and the faster
paths for the web are fewer allocations in the readers (typed fast paths instead of the dynamic `Obj` model).

## M9 inputs (measured 2026-10-16, during M6)

- **PDF apps (MarkupCraft, TakeoffCraft) need a PDF renderer in place of pdf.js.** `hayro` 0.8 (pure Rust, Apache-2.0 OR
  MIT, vello_cpu rasteriser) on the Carey plan-set sheet (18 × 24 in, vector-heavy): load + render at 1× 43 ms vs pdf.js
  (Node 24 + Skia canvas) 390 ms (9×); at 3× 201 vs 505 ms (2.5×); the picture is right (text, hatching, door swings).
  Candidate for the PDF stack; annotation read/write needs a PDF object layer as well (hayro-syntax to read, a writer of
  our own or lopdf (MIT) to write); parity there is semantic (what pdf.js and other readers read back), as pdf-lib's
  bytes cannot be matched by another library.
- The 3D viewport, picking, section cuts, IFC properties and storeys are shared by ModelCraft, BimCraft, CivilCraft's
  site view, DetailCraft and PartCraft: build them once in `cc-ui-egui::view3d` (done for M6/M7).

## After M4 (re-plan inputs, measured 2026-10-16)

- **Every engine ported so far is at exact parity** (byte-identical writers, 0-difference readers and geometry). The
  port-then-diff method (frozen JS vs `cadcraft-cli`, harnesses in `forks/tools`) works; keep it for m3d, ifc, skp.
- **Speed is uneven.** Readers 2–4× the JS natively; DWG saves 1.1–1.4×; **DXF 2000 save 0.7× (slower)**. The cost is
  the JS-shaped dynamic value model (allocation, clones, hashed maps, number formatting), not the algorithms. M5 (a)
  adds typed fast paths where a profile shows it, keeping the dynamic model as the parity reference.
- **Editing a big drawing** rebuilds the whole display model per edit (127 ms of the 3.4 MB drawing's 360 ms edit;
  the rest is this server's software GPU). M5 (a): per-object display cache keyed by object id + doc version.
- **Web:** single-threaded wasm (wgpu's objects are not Send/Sync, so the threaded build is blocked in eframe); engine
  work can still go to a worker later. The 9.0 MB wasm (2.26 MB brotli) now carries every drafting engine: load the
  savers on first use if the first paint suffers.
- **Order:** m3d before ModelCraft before BimCraft, because BimCraft's 3D and PartCraft both stand on m3d, and SKP
  import lands in ModelCraft. Tablets stay "the web build in a WebGPU browser" (section below).

## After M3 (re-plan inputs, measured 2026-10-16)

- Native reading is 2.6–4.2× the JS; **in the browser only 1.2–1.5×** (single-threaded wasm, fat LTO + SIMD). Next web
  work, before M5: wasm threads (rayon over `wasm-bindgen-rayon`, COOP/COEP already sent), fewer allocations in the
  dynamic model (or a typed fast path for the viewer that skips the JSON-shaped model), then re-measure.
- Frame time: the CPU side of a frame is under 1 ms on the 3.4 MB drawing; the GPU side still needs a measurement on
  real hardware (this server only has software rasterisers). A `?bench` frame mode for the web build is next.
