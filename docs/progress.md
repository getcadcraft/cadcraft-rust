# Session log (fork-rust)

Newest first. One entry per working session: what was done, numbers, what is next.

## 2026-10-17 — M9 engines: TakeoffCraft, MarkupCraft; BimCraft's engine

- M9.1 (agent): `cc-takeoff` + `cc-io-takeoff`: TakeoffCraft's DOM-free modules at exact parity (`tools/todiff.mjs
  --seeds 1-10 --exact`: 18,442 records, 0 differences; PDF half 29/29 read back identical by the frozen pdf.js; XLSX
  stored entries byte-identical). Formulas 4.4×, auto count 2× the JS; estimate and XLSX on par (JSON-value state kept
  for parity). Rasters from hayro (match scores within 0.041 of pdf.js's).
- M9.2 (agent): `cc-markup` + `cc-io-markup`: MarkupCraft at exact parity (`tools/mkdiff.mjs` seeds 1–60: 30,994
  records; `tools/mkpdf.mjs` seeds 1–10: 2,262 comparisons; 0 differences). Open a 60-sheet set 6.2×, write 8.4×,
  display lists 33×, BCF 10× the JS. Fuzzing found a panic inside hayro-syntax (`Dict::entries` on a bad hex string):
  dictionaries are now read value by value (regression test).
- M7.9–M7.10 (agent): `cc-bim`: BimCraft's engine (model load, property index, Color by, find, quantities, schedules,
  checks, compare, clash, BCF, tree, visibility, element reading) at exact parity (`tools/bimdiff.mjs`: 38,788 cases
  on fixtures + 119 public/corpus IFC files; BCF zips byte-identical). Load 2.8–5.4×, modules 1.2–4.9× the JS.
- M9.4 (agent): `cc-civil` + `cc-io-civil` + `cc-proj` (proj4 port): CivilCraft at parity (`tools/cvdiff.mjs` seeds
  1–10: 67,101 calls, 4 accepted differences on byte-flipped LAZ / damaged rasters; seeds 2–10 move the fixtures to
  survey coordinates). TIN 2.45×, contours 2.78×, grid volumes 1.24×, LandXML 1.16× the JS. Fuzzing found two `laz`
  crate crashes (guarded, regression seeds); LAZ is not decoded in the web build, where a panic cannot be caught.
  `num_to_string` now picks the even digit on 17-digit ties as V8 does, behind a cheap binary test (proved on 300,000
  values) so the exact expansion runs almost never; wdiff 468/468, idiff 220/220, iodiff 801/801 unchanged.
- M9.12: TakeoffCraft mode on the sheet view: sheet scale (the 0.5 presets), count / linear / area items, marks clicked
  on the sheet (double-click or Enter ends a length or area), live quantities and the estimate total from cc-takeoff
  (`quantities`, `estimate`), undo / redo through `Takeoff`; `takeoff.mode`, `takeoff.scale`, `takeoff.item`,
  `takeoff.finish` commands. Kitchen sheet at 1/4" = 1'-0": the serving line measures 40.47 ft.
- M10.20–M10.22 (agent): `cc-detail` + `cc-io-detail`: DetailCraft at exact parity (`tools/dcdiff.mjs` seeds 1–10:
  55,714 calls, 0 differences; writers byte-for-byte, PDFs inflated-equal; IFC import on 117 corpus files). Mark parts
  3–8×, clash 2–3×, PDF 2–8×, IFC out 2–2.6×, IFC import 2–4× the JS; readModel / build / KISS slower (JSON-value state
  kept for parity). Fuzz targets detail_model, detail_clip, detail_nc1_read, detail_ifc_import.
- M9.11: desktop eframe without `accesskit` (screen readers are the skipped known issue): its Linux bridge pulled zbus
  crates RustSec marked unmaintained today (RUSTSEC-2026-0370/0371/0372, no upgrade), failing `cargo deny`.
- M9.10: MarkupCraft tools on the sheet view: Select (listDistance pick), Pan, Pen, Highlighter, Line, Arrow,
  Rectangle, Ellipse, Cloud, Text; colours; each gesture one `Store::change` step through makeMarkup (the 0.5 ids, styles,
  undo labels), drawn while dragging from the same display list; Undo / Redo / Delete (Ctrl+Z, Delete); Save marked-up
  PDF through cc-io-markup writeMarkedUp; `markup.tool`, `markup.undo` commands; shot tool `--mouse-drag`.
- M9.8: MarkupCraft markups on the sheet view: read with cc-io-markup openPdf when the PDF opens, drawn from cc-markup's
  display lists (shapes.js: the list the saved PDF's appearances come from) through the page's viewport transform;
  pages MarkupCraft draws itself (annotMode "disable") render without their annotations' own appearances (hayro
  `render_annotations`, which auto count keeps on); Markups switch with counts.
- M9.7: crash fix (gate fuzz smoke, `civil_las` OOM): a layered LAZ chunk listing a 4 GB layer made laz-rs size a buffer
  to it before reading; every chunk's layer sizes are now checked against the bytes left before laz-rs reads it (chunks
  found by chunk_size or the chunk table); regression fixture + unit tests + seed; crs-las parity unchanged (358 calls,
  the same 2 accepted differences on byte-flipped files).
- M9.6: PDF plan sets open in the app (the sheet view MarkupCraft and TakeoffCraft build on): sheets by page label,
  hayro render into one texture re-rendered per half-octave of zoom (capped 8192 px a side), pan / wheel zoom / Page
  Up-Down; the kitchen fixture sheet renders in ~22 ms. `pdf.page` command.
- M7.11: BimCraft's Find (rules on any property, every / any, shows only or hides what it finds) and Quantity totals
  (group by, add up a quantity, CSV as totalsCsv) on the IFC view, on cc-bim; the property index is built on first use;
  BimCraft's 0.5 catalogs (en, es-MX) shipped unchanged; `bim.find`, `bim.totals`, `view.show_all` commands.
- M6.15–M6.16: ModelCraft Select all / group / component / explode / delete / paint colours, Scenes, Components,
  view styles (see below).

## 2026-10-16 — M6 ModelCraft and M7 BimCraft in the app

- M6.8 (agent): `cc-m3d-tools` (L4): ModelCraft's camera.js, infer.js, collide.js, tools.js (all 29 tools), sun,
  value box, terrain, components that stretch, starter library; exact parity (`forks/tools/tdiff.mjs --exact`: 19 test
  sections 1,281/1,281 records, seeds 1–10 4,708/4,708); infer / pick 1.9–2.9× the JS.
- M6.10 (agent): `cc-io-m3d` (L3): every format of io.js + solids, SketchUp import, print3d; byte parity 801/801 after
  M6.11 (a signed-zero `Math.max` in the DWG writer); 1.2–15× the JS. M6.12: the open/save glue in `cc-m3d-files` (L4).
- In the app (desktop + web): `.modelcraft`, SKP, OBJ, GLB/glTF, DAE open as models; the tool palette drives
  cc-m3d-tools (left = tool, middle/right orbit, wheel zoom, tool letters, value box, arrows lock, Esc, undo/redo) with
  the tools' overlay (markers, tips, rubber bands, previews, grips, selection window, hover/selection); Export (ModelCraft,
  GLB, OBJ, STL, DAE, IFC4, DXF, DWG); standard views, parallel/perspective, Fit (model.bbox()).
- M7.4–M7.8: IFC in the 3D view: cc-ifc view export (stored meshes once, pieces placing them), feature edges,
  picking (box tree + exact ray–triangle), Properties panel (attributes, property sets, quantities, type), Model panel
  (storeys, classes, hide/show), section cut (clip plane). Clinic_Architectural (2,586 products, 168,703 triangles)
  read and drawn in ~320 ms.
- M6.14–M6.15: side panel (Entity Info, Outliner, Tags, Paint colour from `paint::PALETTE`), es-MX/en catalogs;
  Select all (Ctrl+A), Make group (Ctrl+G), Make component (Ctrl+Shift+G), Explode and Delete as modelcraft.js does them
  (one undo step each, purge after erase); scriptable as `model.edit`. Scenes panel (add, go to, update, delete, Previous / Next, Page Up / Down;
  camera, style and visibility as goScene, moving there over 650 ms as goScene does) and Components panel (count, Place). M6.16: view styles
  (Shaded, Hidden line, Wireframe, X-ray, Edges) for ModelCraft and the IFC view; hidden line fills faces with the
  background colour.
- Checked in Chromium over WebGPU on rust.getcadcraft.com: `?model` (orbit), `?ifc` (click → properties), Open → SKP.
- Measured for M9: hayro renders the Carey PDF sheet 9× faster than pdf.js at 1× (roadmap § M9 inputs).

## 2026-10-16 — M5 done; cc-ifc, cc-manifold, cc-skp done; M6 started

- M5.1 speed debt: DXF 2000 save of the 3.4 MB drawing 702 → 357 ms (JS 450: now 1.26×; DWG 2018 1.45×, DWG 2000
  1.5×): one output buffer, each number formatted once straight into it (toPrecision digits from the shortest digits,
  proven equal on 200,000 random doubles), entities built straight into the saver's values (`cc_cad::entity::ObjSink`
  instead of entity → JSON → value), no deep copy of the blocks. Save parity unchanged (byte-identical 78/78, 32/32,
  110/110 per version). Edit mode: a per-object display cache (`DisplayCache`): an edit walks only the objects that
  changed (all of them when a layer, linetype, style, block or the drawing settings changed): 127 → ~38 ms per edit on
  the 3.4 MB drawing; equal to a full walk (tested through edits, undo, layer and block changes).
- Measured: the DWG reader as single-threaded wasm (WASI under Node) reads the 3.4 MB drawing in 257 ms vs the JS's 527
  (2.05×): the browser's 1.2–1.5× (M3) came from the 174 KB sample, where fixed costs dominate.
- M5.2: golden references of the public fixtures in CI (`tests/golden`, from the kit's `ref.mjs --public-out`).
- M5.3: nightly fuzz OOM in `cad_dxf_read`: nested block inserts multiplied without end in an object's outline and the
  walk; budgets (2 M primitives per outline, 10 M visits per walk) + regression tests.
- M6.1/M6.2 groundwork: `cc-render::mesh3d` (3D viewport GPU side: meshes per definition, camera-relative f64
  instances, reversed-Z, MSAA offscreen target, styles, read-back; headless cube at survey coordinates) and
  `cc-geom::bvh` (bvh.js, same splits and visiting order).
- M5.4 (agent): `cc-m3d` = ModelCraft's model.js in full (+ vec, earcut, units formatters) at exact parity
  (`forks/tools/mdiff.mjs --exact`: 640/640 op records + 1,247/1,247 function cases per seed, seeds 1–10; V8's TimSort
  ported because NaN keys make comparators inconsistent), 3.4–8.4× the JS; fuzz `m3d_load` 5.4 M runs.
- M7.1 (agent): `cc-io-ifc`: ifc-step 118/118 exact (4.4–7.9×), ifc-geom with booleans behind `trait SolidOps`.
- M7.2 (agent): `cc-manifold`: Manifold 3.5.4's mesh boolean ported to pure Rust (Apache-2.0 port, notices kept),
  identical to the manifold-3d wasm on 11,093 cases (incl. 1,102 recorded from the IFC corpus), 1.8× the wasm; works
  on the web (no C++). Chosen over the approved C++ binding because a binding cannot build for wasm32-unknown-unknown.
- M7.3: IFC openings through cc-manifold: ifc-geom 115/115 (was 105/115), 3.5–7.9× the JS with booleans on both
  sides.
- M8.1 (agent): `cc-io-skp`: readSkp 27/27 byte-identical canonical JSON, 5.4–14.2× the JS, faster than OpenSKP's
  recorded times on every corpus file; fuzz `skp_read`.
- M6.3/M6.4: the 3D viewport in the UI (offscreen render on demand + blit, orbit / pan / zoom from camera.js) on
  desktop and web (`?demo3d`, checked in Chromium over WebGPU), deployed.
- Next (M6): camera.js / infer.js / tools.js and io.js formats on cc-m3d (agents), then ModelCraft's app shell.

## 2026-10-16 — M4 done

- M4.7: `cc-io-cad` (L4, agent-ported): CadCraft's DWG import (dwg-import.js + helpers), DXF read / write R12 (dxf.js),
  DWG save 2000–2018 (dwg-save.js over the M4.4 writer) and DXF 2000 (dxf2000.js). `forks/tools/idiff.mjs` (frozen JS
  vs `cadcraft-cli import / dxf-out / save`): 0 differences on 78 kit DWGs, 32 other DWGs, 110 DXFs (import, Doc after
  load, R12 text, re-read); saves byte-identical with equal reports, 78/78 per version (2000, 2004, 2007, 2010, 2013,
  2018, DXF 2000), also on the other DWGs, the DXFs, a synthetic fixture and 15 dynamic-block variants. Fuzz target
  `cad_dxf_read` (read → load → R12 → DXF 2000 → DWG save): 3 × 10 min, no crash.
- M4.8: Edit and Save in the app. **Edit** imports the open DWG / DXF into an editable drawing (the viewer stays the
  fast default for viewing); **Save as** DWG 2018 … 2000, DXF 2000, DXF R12 (desktop: dialog + write; web: download),
  Ctrl+S = the last format. Checked: a headless test (sample → Edit → LINE → all 8 formats → each re-opened with the new
  line), cadcraft-shot `--edit --save`, Chromium on the local build and on rust.getcadcraft.com (Edit, CIRCLE, RECTANG,
  Save as DWG 2004 → an AC1018 download holding both).
- Edit-mode display fixes found by measuring the big drawings: a linetype's dash on a circle kept the circle's `full`
  flag and was drawn as a whole circle (Carey: 328 dashed circles = 906,000 segments → 22,000); a dash budget per
  object (`DISPLAY_MAX_DASHES` = 400, past it the object draws continuous; parity paths keep the JS's 20,000); batches
  per layer (the layers panel works in edit mode); layer colours.
- Command line: the field keeps the focus on Enter, and the input hook passes one Enter per frame (what follows waits a
  frame): typed answers were lost or merged on the web when keys came faster than frames.
- `round_precision` (toPrecision) from the shortest round-trip digits instead of 40 exact digits: proven equal, checked
  on 200,000 random doubles; DXF 2000 save of the 3.4 MB drawing 834 → 702 ms.

## 2026-10-16 — M4 in progress

- M4.1: `cc-engine::prompt`: commands are async blocks that ask questions (`Prompter::ask`) and a runtime-free
  `Runner` answers them (Esc = `Cancelled`), single-threaded, wasm-safe.
- M4.2: `cc-cad` (L1, doc.js: typed entities, change/undo/redo across spaces, blocks, layouts, load, snapshot) and
  `cc-cad-geom` (L2, geom.js, dims, dimstyle, units, walk, font, leader, linetypes, hatch). Agent-ported. Parity with
  `forks/tools/gdiff.mjs --exact` (no tolerance): 0 differences on ~18,000 geometry cases per seed (seeds 1–10) and
  1,210/1,210 doc-op steps. V8's sin/cos are fdlibm's: ported (`jsmath.rs`, Sun notice kept, NOTICE updated);
  tan/atan/atan2/acos from the `libm` crate match V8 exactly.
- Fuzz target `cad_load` (any JSON → `Doc::load` → snapshot → grips, explode, walk): 648,764 runs in 10 min, no crash.
- `cc-cad-geom::display`: the editable document walked into the viewer's display model, so the drafting canvas
  renders through the M3 GPU renderer.
- M4.4: the cc-dwg writer (agent): `write_dwg` byte-identical to the 0.5 JS `writeDwg` on 78 files × 6 versions
  (468/468, reports equal), 72/72 synthetic models; 1.2–3.1× faster single-threaded; fuzz target `dwg_roundtrip`.
- M4.5: the command line and drafting commands (agent): LINE PLINE CIRCLE ARC RECTANG POLYGON ELLIPSE POINT TEXT ERASE
  MOVE COPY ROTATE SCALE MIRROR OFFSET TRIM EXTEND UNDO REDO ZOOM; `forks/tools/kdiff.mjs` (the frozen JS Engine with a
  stub host vs `cadcraft-cli cmds`): 0 differences, 26 seeds, ~70,000 records (6 seeds bit-exact).
- M4.6: drafting in the app: New drawing (Ctrl+N, `?new` on the web), the command line (history, prompt, Enter /
  Space / right click = Enter, Esc, Delete, Ctrl+Z / Ctrl+Y, F3 object snap, F8 ortho), cadcraft.js's click rules (point
  questions: object snap end/mid/cen/quad/int/perp/node/ins with markers, then ortho; object questions: pick or
  window; idle: pick, Shift removes, a click on nothing starts a window, drag = window, right-to-left = crossing),
  the rubber band, selection highlight with grips, ZOOM requests. The 0.5 language catalogs (en, es-MX) ship unchanged
  in `assets/lang/cadcraft/`. Checked in Chromium (WebGPU) with typed commands and real clicks.
- Still running: DWG import + CadCraft's DXF read/write, then DWG save (dwg-save.js) and DXF 2000 (new crate `io-cad`,
  harness idiff.mjs), by an agent in its own work copy.

## 2026-10-16 — M3 done

- `cc-drawing` (L1): the display model (spaces → batches per layer × colour × weight: polylines, fills, texts),
  serialising to the JS `compileDrawing` shape. `cc-io-dwg::compile` + `dxf_model`: BimCraft's compile step and DXF
  reader ported at parity (0 differences: 78 corpus DWGs, 19 client DWGs, 106 DXFs). Agent-ported, merged in M3.2.
- `cc-render` (L3): wgpu, instanced anti-aliased line quads + lyon-tessellated fills, positions f32 relative to each
  space's origin and the view centre subtracted in f64 (survey coordinates stay steady), one uniform slot per view
  (model, or sheet + each viewport), one draw call per visible batch.
- `cc-ui-egui` (L6): canvas through an `egui_wgpu` paint callback, sheets (paper, viewports clipped/scaled/twisted
  like dwg-view.js, viewport frames), texts on top (egui text, culled, budgeted), layers panel, sheet tabs, status bar,
  `UI_COMMANDS` + `UiApp::run`, injected `Services` (pickers, inbox, xref lookup), Dark/Light tokens with a contrast
  test, bundled OFL fonts (egui's default fonts carry the Ubuntu Font Licence: not on our list).
- Apps: desktop `cadcraft` (eframe + rfd), web `cadcraft-web` (WebGPU only; a clear message without it; panic →
  message on the page), `cadcraft-shot` (headless screenshots + frame bench through egui_kittest on wgpu/lavapipe).
  `cargo xtask web` builds with trunk and moves the inline init script into a hashed file (CSP: no inline script).
- Deployed through `forks/deploy.sh rust dist/web` → https://rust.getcadcraft.com (`?sample`, `?bench`).
- Numbers in ROADMAP.md. Honest gap: the browser build is only 1.2–1.5× the JS reader (native 2.6–4.2×).
- Dev tools outside the repo: `forks/tools/cdiff.mjs` (compile parity), `forks/tools/webshot.mjs` (Chromium +
  SwiftShader WebGPU screenshots: needs `--use-vulkan=swiftshader --use-angle=swiftshader --enable-unsafe-swiftshader`).

## 2026-10-16 — M2 done

- **cc-dwg in Rust** (`crates/io-dwg`): a line-by-line port of CadCraft's own `lib/dwg` reader (≈6,500 lines of JS →
  ≈12,900 lines of Rust incl. tests): R13–R2018 containers (R2004 LZ, R2007 Reed-Solomon + R21 LZ), bit reader, header variables,
  classes, object map, every object decoder (entities, tables, objects, ACAD_TABLE family, ACIS solids + data storage,
  surfaces, multileaders, dynamic blocks), and the model assembly of `read.js`.
- **Design:** the reader builds the JS model itself as a dynamic `value::Obj` (insertion-ordered, `&'static` keys), so
  `parity dwg` emits the same JSON as `readDwg` and the kit diffs the *whole* model, not a summary. Objects decode in
  parallel (rayon) and entities convert in parallel; R2004 pages decompress in parallel. Every decoder runs under the
  crate's deny list (no unwrap/panic/indexing/unchecked arithmetic/lossy casts); a panic in one object would cost that
  object only (`catch_unwind` guard).
- **Parity:** `tools/fork-parity run.mjs --fork rust --engine dwg` → 78/78 match. Every other DWG in testfiles: 0 diffs.
- **Speed:** 2.6–4.2× the JS on the corpus; 3.0–3.4× on the large client drawings; single-threaded 1.8–2×
  (ROADMAP.md has the table). Allocation is the remaining cost (a dynamic model allocates per field); a faster allocator
  measured ~35% more, but the good ones are C (mimalloc) — not taken (AGENTS.md: no C unless unavoidable).
- **Safety:** fuzz target `dwg_read` (libFuzzer): 30 s in every `cargo xtask ci`, a 10-minute run at M2's end (see the
  M2 report); 500 mutated corpus files (truncations, bit flips, random bursts) through the CLI: no panic.
- **How it was built:** the lead ported the core and the table decoders; five agents ported entities, objects, tables,
  solids/surfaces and multileaders/dynamic blocks in parallel work copies, each to zero differences on its types, merged
  in M2.1–M2.4; M2.5 = speed.
- Next: M3, the 2D viewer (DWG/DXF, model + sheets, pan/zoom, layers) on desktop and web (WebGPU), deployed to
  rust.getcadcraft.com.

## 2026-10-15

- M1: workspace skeleton (crates by layer, apps, xtask, fuzz), lints, configs, CI workflow. `cargo xtask ci` green.
