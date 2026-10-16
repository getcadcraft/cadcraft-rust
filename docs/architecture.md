# Architecture

Last reviewed 2026-10-16 · Change: cc-dwg (M2).

## Toolchain and pins

| Item | Pin |
|---|---|
| Rust | 1.99.0 (`rust-toolchain.toml`), edition 2024, resolver 3, rust-version 1.95 |
| Fuzzing | nightly + cargo-fuzz |
| Web | trunk, wasm32-unknown-unknown, WebGPU only |

UI crates (egui/eframe/wgpu) are pinned when they enter in M3.

## Layers

| Layer | Crates |
|---|---|
| L0 | `cc-geom` |
| L1 | `cc-drawing` |
| L3 | `cc-io-dwg`, `cc-render` |
| L4 | `cc-tools` |
| L5 | `cc-engine` |
| L6 | `cc-ui-egui`, `cc-mcp` |
| L7 | `cadcraft`, `cadcraft-cli`, `cadcraft-web` |

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
