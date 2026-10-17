# Third-party crates

Every direct dependency, why it is here, and its licence. `cargo deny check` enforces the licence allowlist.

| Crate | Used by | Why | Licence |
|---|---|---|---|
| serde, serde_json | io-dwg, engine, mcp, m3d, io-m3d, takeoff, io-takeoff, xtask | JSON for `convert --json`, commands, MCP | MIT OR Apache-2.0 |
| thiserror | io-dwg, engine | error types | MIT OR Apache-2.0 |
| encoding_rs | io-dwg, io-civil | DWG code-page text (windows-125x, Shift-JIS, GBK, Big5, EUC-KR …); .dbf code pages from the .cpg (TextDecoder labels) | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| rayon | io-dwg, io-skp (native only) | parallel object decoding / conversion / page decompression | MIT OR Apache-2.0 |
| libm | cad-geom, m3d, io-m3d, takeoff, io-takeoff | fdlibm maths (tan, atan, atan2, acos) bit-identical to V8, so geometry matches the 0.5 JS exactly | MIT |
| unicode-general-category | io-m3d | Unicode general categories for the Collada writer's NCName check (the JS uses `\p{L}`/`\p{N}`… regex classes); build.rs only writes a lookup table into OUT_DIR | Apache-2.0 |
| hayro (0.8) | io-takeoff | pure-Rust PDF reader and renderer: page boxes, labels and attachments for the plan set, ink rasters for auto count, the vector paths snapping reads (the JS uses pdf.js) | Apache-2.0 OR MIT |
| sha2 | io-takeoff | SHA-256 of a PDF's bytes: a sheet is known by its file's hash, as `pdfset.js` does with SubtleCrypto | MIT OR Apache-2.0 |
| miniz_oxide | io-takeoff, markup, io-markup, proj, io-civil | raw deflate for the XLSX zip entries (the JS uses CompressionStream); deflate / inflate for PDF streams (pdf-lib's flate streams), PNG pictures and the XLSX / BCF zips; zlib inflate of DEFLATE GeoTIFF blocks (geotiff.js uses pako); zlib deflate of the GeoTIFF export's blocks and of PDF page streams (plot.js uses CompressionStream), and their inflate for the harness's PDF comparison | MIT OR Zlib OR Apache-2.0 |
| unicode-normalization | takeoff, io-civil | NFC/NFD for the ICU-style `localeCompare` the item and division sorts use; NFKD for .dbf field names and GIS export file names; NFD for the PDF font metrics (font.js) | MIT OR Apache-2.0 |
| libfuzzer-sys | fuzz/ (not shipped) | cargo-fuzz targets | MIT OR Apache-2.0 OR NCSA |
| zlib-rs | io-skp, bim, io-civil | raw-deflate inflate of SketchUp 2021+ ZIP entries, BCF zips, CivilCraft's GIS zips / KMZ (inflate and deflate) (fastest pure-Rust inflater measured: 16 ms vs 21 ms for miniz_oxide on a 16 MB model.dat); `rust-allocator` feature, no C; raw-deflate inflate of SketchUp 2021+ ZIP entries; CivilCraft's GIS zips / KMZ (inflate and deflate) (fastest pure-Rust inflater measured: 16 ms vs 21 ms for miniz_oxide on a 16 MB model.dat); `rust-allocator` feature, no C | Zlib |
| crc32fast | io-skp | CRC-32 check of unpacked ZIP entries (SIMD at run time) | MIT OR Apache-2.0 |
| memchr | io-skp | substring search for record markers in classic SketchUp files | Unlicense OR MIT (used under MIT) |
| hayro-syntax | io-markup | PDF object syntax for MarkupCraft's pdf.js-side reading (xref tables and streams, object streams, repair, decryption); `std` feature only, no rendering | Apache-2.0 OR MIT |
| indexmap | markup, io-markup | insertion-ordered maps (the JS Map / object order of markups, fields and refs) | Apache-2.0 OR MIT |
| url | io-civil | WHATWG URL parsing / serialising for the base-map service URLs (the JS uses `new URL` and `URLSearchParams`) | MIT OR Apache-2.0 (idna / ICU data crates: Unicode-3.0) |
| laz (laz-rs) | io-civil | LAZ (LASzip) point decompression for CivilCraft's LiDAR import (the JS uses laz-perf); deps byteorder, num-traits; no build.rs, no default features | Apache-2.0 |
| regex | io-civil | crs.js / crs-pick.js / raster.js / las.js patterns (.prj matching, lat/long text, GDAL metadata, vertical units) | MIT OR Apache-2.0 |
| robust | civil | Shewchuk's adaptive exact `incircle` for the TIN's Delaunay flips (tin.js reads only its sign) | MIT OR Apache-2.0 |
| fancy-regex | civil | JS regular expressions with lookaround and backtracking, for the deed-call, traverse and offset parsers that copy the 0.5 patterns as written | MIT |

## Ported source code (no crate dependency)

| Source | Ported into | Why | Licence |
|---|---|---|---|
| delaunator 5.1.0 (github.com/mapbox/delaunator) | crates/civil/src/delaunator.rs | the TIN's Delaunay triangulation, giving the same triangles as the 0.5 JS | ISC (see NOTICE) |
| robust-predicates 3.0.3 `orient2d` (github.com/mourner/robust-predicates, after Shewchuk) | crates/civil/src/predicates.rs | the exact orientation test delaunator and tin.js use | Unlicense (public domain) |
| Manifold 3.5.4 (github.com/elalish/manifold): src/boolean3, boolean_result, csg_tree, edge_op, face_op, impl, polygon, properties, sort, tree2d, collider.h, shared.h, utils.h, parallel.h (sequential paths), include/manifold/{common,mesh,math,linalg}.h | cc-manifold (crates/manifold) | the mesh boolean behind IFC openings (cc-ifc geo-bool.js) and ModelCraft's solid tools, with output identical to the manifold-3d 3.5.4 wasm the 0.5 apps use | Apache-2.0 (notices kept in each file; see NOTICE) |
| — its math.h `acos` (FreeBSD msun via musl) | crates/manifold/src/math.rs | deterministic vertex pseudo-normals | SunPro notice (permissive), see NOTICE |
| — its disjoint_sets.h (github.com/wjakob/dset) | crates/manifold/src/disjoint.rs | union-find | Zlib |
| — its linalg.h (based on linalg.h 2.2, Sterling Orsten) | crates/manifold/src/linalg.rs | the vector operations, same evaluation order | Apache-2.0 / public domain |
| Adobe Core14 font metrics (Helvetica, Helvetica-Bold AFM) as pdf-lib 1.17.1 ships them | crates/io-takeoff/src/stdfont.rs (generated widths + kerning for WinAnsi) | the marked PDF and the proposal measure and wrap text exactly as pdf-lib's `widthOfTextAtSize` | Adobe AFM notice (redistribution permitted with the notice; kept in the file); pdf-lib MIT |
| proj4js 2.22.0 (github.com/proj4js/proj4js): lib/ (defs, projString, parseCode, Proj, datum, datumUtils, datum_transform, adjust_axis, transform, core, nadgrid, common/*, constants/*, projections: merc, longlat, tmerc, etmerc, utm, sterea, gauss, stere, somerc, omerc, lcc, krovak, cass, laea, aea, gnom, cea, eqc, poly, nzmg, mill, sinu, moll, eqdc, aeqd, eqearth) and its bundled wkt-parser 1.5 | cc-proj (crates/proj) | coordinate systems with V8-identical numbers (CivilCraft's crs.js runs on proj4js) | MIT (see crates/proj/NOTICE) |
| geotiff.js 3.0.5 (github.com/geotiffjs/geotiff.js): src/geotiff, geotiffimage, imagefiledirectory, globals, resample, rgb, predictor, compression/{lzw,packbits,deflate,raw} | crates/proj/src/tiff.rs | GeoTIFF elevation grids and PROJ grid shift files | MIT (see crates/proj/NOTICE) |
| fdlibm 5.3 e_log.c | crates/proj/src/math.rs | V8's Math.log (libm's log differs in the last bit for ~0.3 % of arguments) | SunPro notice (permissive), see NOTICE |

