# Third-party crates

Every direct dependency, why it is here, and its licence. `cargo deny check` enforces the licence allowlist.

| Crate | Used by | Why | Licence |
|---|---|---|---|
| serde, serde_json | io-dwg, engine, mcp, xtask | JSON for `convert --json`, commands, MCP | MIT OR Apache-2.0 |
| thiserror | io-dwg, engine | error types | MIT OR Apache-2.0 |
| encoding_rs | io-dwg | DWG code-page text (windows-125x, Shift-JIS, GBK, Big5, EUC-KR …) | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| rayon | io-dwg, io-skp (native only) | parallel object decoding / conversion / page decompression | MIT OR Apache-2.0 |
| libm | cad-geom | fdlibm maths (tan, atan, atan2, acos) bit-identical to V8, so geometry matches the 0.5 JS exactly | MIT |
| libfuzzer-sys | fuzz/ (not shipped) | cargo-fuzz targets | MIT OR Apache-2.0 OR NCSA |
| zlib-rs | io-skp | raw-deflate inflate of SketchUp 2021+ ZIP entries (fastest pure-Rust inflater measured: 16 ms vs 21 ms for miniz_oxide on a 16 MB model.dat); `rust-allocator` feature, no C | Zlib |
| crc32fast | io-skp | CRC-32 check of unpacked ZIP entries (SIMD at run time) | MIT OR Apache-2.0 |
| memchr | io-skp | substring search for record markers in classic SketchUp files | Unlicense OR MIT (used under MIT) |
