# Third-party crates

Every direct dependency, why it is here, and its licence. `cargo deny check` enforces the licence allowlist.

| Crate | Used by | Why | Licence |
|---|---|---|---|
| serde, serde_json | io-dwg, engine, mcp, m3d, io-m3d, xtask | JSON for `convert --json`, commands, MCP | MIT OR Apache-2.0 |
| thiserror | io-dwg, engine | error types | MIT OR Apache-2.0 |
| encoding_rs | io-dwg | DWG code-page text (windows-125x, Shift-JIS, GBK, Big5, EUC-KR …) | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| rayon | io-dwg, io-skp (native only) | parallel object decoding / conversion / page decompression | MIT OR Apache-2.0 |
| libm | cad-geom, m3d, io-m3d | fdlibm maths (tan, atan, atan2, acos) bit-identical to V8, so geometry matches the 0.5 JS exactly | MIT |
| unicode-general-category | io-m3d | Unicode general categories for the Collada writer's NCName check (the JS uses `\p{L}`/`\p{N}`… regex classes); build.rs only writes a lookup table into OUT_DIR | Apache-2.0 |
| libfuzzer-sys | fuzz/ (not shipped) | cargo-fuzz targets | MIT OR Apache-2.0 OR NCSA |
| zlib-rs | io-skp | raw-deflate inflate of SketchUp 2021+ ZIP entries (fastest pure-Rust inflater measured: 16 ms vs 21 ms for miniz_oxide on a 16 MB model.dat); `rust-allocator` feature, no C | Zlib |
| crc32fast | io-skp | CRC-32 check of unpacked ZIP entries (SIMD at run time) | MIT OR Apache-2.0 |
| memchr | io-skp | substring search for record markers in classic SketchUp files | Unlicense OR MIT (used under MIT) |

## Ported source code (no crate dependency)

| Source | Ported into | Why | Licence |
|---|---|---|---|
| Manifold 3.5.4 (github.com/elalish/manifold): src/boolean3, boolean_result, csg_tree, edge_op, face_op, impl, polygon, properties, sort, tree2d, collider.h, shared.h, utils.h, parallel.h (sequential paths), include/manifold/{common,mesh,math,linalg}.h | cc-manifold (crates/manifold) | the mesh boolean behind IFC openings (cc-ifc geo-bool.js) and ModelCraft's solid tools, with output identical to the manifold-3d 3.5.4 wasm the 0.5 apps use | Apache-2.0 (notices kept in each file; see NOTICE) |
| — its math.h `acos` (FreeBSD msun via musl) | crates/manifold/src/math.rs | deterministic vertex pseudo-normals | SunPro notice (permissive), see NOTICE |
| — its disjoint_sets.h (github.com/wjakob/dset) | crates/manifold/src/disjoint.rs | union-find | Zlib |
| — its linalg.h (based on linalg.h 2.2, Sterling Orsten) | crates/manifold/src/linalg.rs | the vector operations, same evaluation order | Apache-2.0 / public domain |

