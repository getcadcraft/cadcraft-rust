# Third-party crates

Every direct dependency, why it is here, and its licence. `cargo deny check` enforces the licence allowlist.

| Crate | Used by | Why | Licence |
|---|---|---|---|
| serde, serde_json | io-dwg, engine, mcp, xtask | JSON for `convert --json`, commands, MCP | MIT OR Apache-2.0 |
| thiserror | io-dwg, engine | error types | MIT OR Apache-2.0 |
| encoding_rs | io-dwg | DWG code-page text (windows-125x, Shift-JIS, GBK, Big5, EUC-KR …) | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| rayon | io-dwg (native only) | parallel object decoding / conversion / page decompression | MIT OR Apache-2.0 |
| libfuzzer-sys | fuzz/ (not shipped) | cargo-fuzz targets | MIT OR Apache-2.0 OR NCSA |
