# Architecture

Last reviewed 2026-10-16 · Change: drafting (M4): cc-cad, cc-cad-geom, the command engine, the DWG writer, cc-io-cad.

## Toolchain and pins

| Item | Pin |
|---|---|
| Rust | 1.99.0 (`rust-toolchain.toml`), edition 2024, resolver 3, rust-version 1.95 |
| Fuzzing | nightly + cargo-fuzz |
| Web | trunk, wasm32-unknown-unknown, WebGPU only |

| UI | egui / eframe / egui-wgpu / egui_kittest 0.37, wgpu 30, lyon_tessellation 1, rfd 0.17, trunk 0.21 |

## Layers

| Layer | Crates |
|---|---|
| L0 | `cc-geom` |
| L1 | `cc-drawing` (display model), `cc-cad` (CadCraft's editable Doc, doc.js) |
| L2 | `cc-cad-geom` (geom.js, dims, dimstyle, units, walk, font, leader, linetypes, hatch; `display` = Doc → `cc-drawing`) |
| L3 | `cc-io-dwg` (DWG reader + writer, DXF reader, compile to `cc-drawing`), `cc-render` (wgpu) |
| L4 | `cc-tools`, `cc-io-cad` (CadCraft's DWG import, DXF R12 read/write, DWG save, DXF 2000; L4 because it uses `cc-io-dwg`) |
| L5 | `cc-engine` (prompt Runner, command line, drafting commands) |
| L6 | `cc-ui-egui`, `cc-mcp` |
| L7 | `cadcraft`, `cadcraft-cli`, `cadcraft-web`, `cadcraft-shot` |

`cargo xtask layers` enforces "strictly lower layers only" and "no UI toolkit below L6" from
`[package.metadata.layer]`.

## cc-dwg (`crates/io-dwg`, L3)

Port of CadCraft's clean-room `lib/dwg` reader (ODA spec v5.4.1). One module per JS file: `bits` (bit codes),
`container` / `lz2004` / `r2007` (sections), `header_vars`, `objects` (framing, EED, common data, the `Record`),
`dec_entities` / `dec_objects` / `dec_objects2` / `dec_mleader` / `table_read` / `solid_read` / `surface_read` (type
decoders), `acis` / `acds` (ACIS data, R2013+ data storage), `mleader_read` / `dynblock_read` (model fields), `read`
(`read_dwg` + the model assembly), `passthrough` (raw data kept for writers), `messages` (the `ccdwg.*` English).

- **Model = the JS model.** `read_dwg` returns `value::Obj`, the exact shape of `readDwg`'s model (lib/dwg/model.js), so
  the parity kit compares the whole model. Numbers are f64 like JS; objects are insertion-ordered with `&'static` keys.
  The viewer (M3) converts this model into the typed `cc-drawing` document; it never edits the Obj.
- **Records** mirror the JS records: `f` = fields (`_`-prefixed are private), `h` = handle references; common entity data
  in typed fields. Decoders are `fn(&mut BitReader, &mut BitReader, &mut Record, &Ctx) -> Res<()>`, registered by name in
  each module's `decoder()`; precedence as objects-read.js's `Object.assign` order (table-read wins).
- **Parallelism (native):** objects decode in parallel (each object is independent), R2004 section pages decompress in
  parallel, and entity conversion runs as one parallel batch over every block; results are merged in the JS order so
  notes, counts and output order are unchanged. wasm32 runs the same code sequentially.
- **Never crash:** the crate denies unwrap/expect/panic/indexing/unchecked integer arithmetic/lossy casts; release builds
  keep overflow checks; each object decode is wrapped in `catch_unwind` (native). A damaged object becomes a note, as in JS.
- **Diagnostics:** `CCDWG_PHASES=1` prints phase timings to stderr.

## The 2D viewer (M3)

open → `read_dwg` / `read_dxf_model` (the JS model) → `compile_drawing` (+ xref models) → `cc_drawing::Drawing` →
`build_geometry` per space (parallel) → uploaded on the first paint → per frame: one uniform per view + one draw call
per visible batch, texts as egui shapes.

- **GPU, not a CPU raster (deliberate deviation from AGENTS.md § 6 "2D apps: vello_cpu").** Speed decides (owner): a CAD
  drawing pans and zooms continuously; uploading the lines once and drawing them on the GPU costs one uniform write per
  frame, where a CPU raster re-renders the view on every zoom step. Overlays stay vector (egui) on top, as § 6 asks.
- **Large coordinates:** mesh positions are f32 relative to the space's bounds centre; the shader receives the view
  centre relative to the same origin (computed in f64), so a survey drawing at x = 500 000 m stays steady.
- **Sheets:** the paper (theme colour), then each viewport: the model mesh drawn with its own view
  (zoom × viewHeight ratio, twist, centre mapped as dwg-view.js's `vpMatrix`) under a scissor rectangle, then the
  sheet's own entities. Per-viewport frozen layers are not applied yet (as in the JS viewer).
- **Web:** WebGPU only (`Backends::BROWSER_WEBGPU`; egui-wgpu's WebGL feature is off), `profile.web` = fat LTO, one
  codegen unit, opt-level 3, `+simd128`; trunk + wasm-opt -O3; no inline script.

## Drafting (M4)

`cc-cad::Doc` is doc.js: typed entities in spaces (model + one per sheet), blocks, layouts, layers, `change(label, f)`
= one undo step (snapshots of what changed), events drained by the host. `cc-engine` runs commands as async blocks that
ask questions (`Prompter::ask`), answered by a runtime-free `Runner` (the host feeds clicks and typed text). The UI
(`cc-ui-egui::edit`) owns the Editor: pick, window, object snap (view.js priorities), ortho, preview; after each
change the canvas rebuilds its display model with `cc_cad_geom::display::doc_drawing`.

Files: `cc_io_cad::import_dwg` (dwg-import.js over `read_dwg`) / `read_dxf` → `Doc::load`; `write_dxf` (R12),
`write_dxf2000`, `dwg_save::save_dwg` (dwg-save.js → `cc_io_dwg::write_dwg_opts`). All byte-identical to the 0.5 JS
(harnesses outside the repo: gdiff, kdiff, wdiff, idiff). V8's sin/cos are fdlibm's: `cc-cad-geom::jsmath` ports them.
