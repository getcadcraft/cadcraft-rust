# Architecture

Last reviewed 2026-10-15 · Change: first version.

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
