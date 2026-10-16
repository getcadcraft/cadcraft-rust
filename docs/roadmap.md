# Roadmap: CadCraft in Rust

Last updated 2026-10-16 · Change: M3 done (viewer deployed).

| Date | Change |
|---|---|
| 2026-10-15 | First version (milestones from the kick-off prompt). |
| 2026-10-16 | M1, M2 done; M2 numbers in ROADMAP.md. |
| 2026-10-16 | M3 done: viewer on desktop + web (WebGPU), live at rust.getcadcraft.com. |

Speed decides. Every milestone is measured against the CadCraft 0.5 JavaScript build (reference:
`getcadcraft.com/public_html/assets/apps/<app>/dev/`, `lib/{dwg,ifc,skp,shapes}`, `docs/apps/*.md`) with
`tools/fork-parity` once forks-infra ships it, and with `cargo xtask bench` / `cargo xtask parity` until then.
Parity on the corpus is the gate (AGENTS.md section 15).

| # | Milestone | Ends with | Status |
|---|---|---|---|
| M1 | Workspace + gate | crate layout, lints, clippy.toml, deny.toml, xtask (ci, layers, wasm, assets, deny, fuzz --smoke), rust-toolchain.toml, CI workflow; `cargo xtask ci` green on the empty skeleton | done 2026-10-15 |
| M2 | cc-dwg reader (`crates/io-dwg`) | parity with `lib/dwg` readDwg on the corpus (six versions + ODA truth), one fuzz target, `cadcraft-cli info` / `convert --json`, benchmark vs JS readDwg on the largest files; parity numbers + timings | done 2026-10-16: 78/78 parity, 2.6–4.2× |
| M3 | CadCraft 2D viewer | open DWG/DXF, model + sheets, pan/zoom, layers; desktop and web (WebGPU only); deployed to rust.getcadcraft.com via forks/deploy.sh; frame-time benchmark on the big drawings | done 2026-10-16 |
| M4 | CadCraft drafting commands | command engine with commands that ask questions (LINE ... TRIM), undo, save DWG/DXF at parity with dwg-save.js | planned |
| M5+ | The rest | m3d kernel + ModelCraft, cc-ifc + BimCraft, cc-skp port (beat OpenSKP and cc-skp), the other apps; re-planned with measured numbers after M3 | planned |

## Tablets (AGENTS.md section 10, Mobile)

CadCraft 0.5 Studio runs on Android and iPad. eframe's Android/iOS support is limited, so the fork covers tablets
through the **web build in the tablet's browser** first (WebGPU: Chrome on Android, Safari 26+ on iPad): touch layout
(pinch/pan, long press = Enter, 40 px hit areas), browser file picker and download, recovery snapshots in OPFS on
`visibilitychange`. A native Android/iOS shell is re-planned after M3 with measured numbers; until then the fork
makes no tablet promise beyond "the web build in a WebGPU browser".

## Known issues

- Accessibility (screen readers) is skipped by the owner's decision (2026-10-15); see `docs/gaps.md`.

## After M3 (re-plan inputs, measured 2026-10-16)

- Native reading is 2.6–4.2× the JS; **in the browser only 1.2–1.5×** (single-threaded wasm, fat LTO + SIMD). Next web
  work, before M5: wasm threads (rayon over `wasm-bindgen-rayon`, COOP/COEP already sent), fewer allocations in the
  dynamic model (or a typed fast path for the viewer that skips the JSON-shaped model), then re-measure.
- Frame time: the CPU side of a frame is under 1 ms on the 3.4 MB drawing; the GPU side still needs a measurement on
  real hardware (this server only has software rasterisers). A `?bench` frame mode for the web build is next.
