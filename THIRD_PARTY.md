# Third-party crates

Every direct dependency, why it is here, and its licence. `cargo deny check` enforces the licence allowlist.

| Crate | Used by | Why | Licence |
|---|---|---|---|
| serde, serde_json | io-dwg, engine, mcp, m3d, io-m3d, takeoff, io-takeoff, xtask | JSON for `convert --json`, commands, MCP | MIT OR Apache-2.0 |
| thiserror | io-dwg, engine | error types | MIT OR Apache-2.0 |
| encoding_rs | io-dwg, io-civil | DWG code-page text (windows-125x, Shift-JIS, GBK, Big5, EUC-KR …); .dbf code pages from the .cpg (TextDecoder labels) | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| rayon | io-dwg, io-skp, io-cad, engine (native only) | parallel object decoding / conversion / page decompression; PUBLISH's sheets (display lists, page deflate) | MIT OR Apache-2.0 |
| libm | cad-geom (and through it io-dwg's display list), m3d, io-m3d, takeoff, io-takeoff, part, lisp | fdlibm maths (tan, atan, atan2, acos) bit-identical to V8, so geometry matches the 0.5 JS exactly | MIT |
| wasmalloc (=0.1.1) | cadcraft-web (web only) | the web build's global allocator in place of std's dlmalloc: dlmalloc grows the wasm memory a little at a time and Chromium runs a garbage collection on each grow (45 % of a cold Carey open); wasmalloc grows geometrically, size classes for small blocks (first open of Carey 1,320 → 380 ms in Chromium). Pure Rust, no dependencies; build.rs only adds a link argument to its own wasi test binaries; refuses to build with wasm threads (the web build has none). Pinned exactly: young crate (2026-09), its unsafe is proved / reviewed per its soundness ledger; the web vs desktop parity sweep (149 DWGs, 156 DXFs, model + display list identical) runs on it | MIT OR Apache-2.0 |
| unicode-general-category | io-m3d | Unicode general categories for the Collada writer's NCName check (the JS uses `\p{L}`/`\p{N}`… regex classes); build.rs only writes a lookup table into OUT_DIR | Apache-2.0 |
| hayro (0.8) | io-takeoff | pure-Rust PDF reader and renderer: page boxes, labels and attachments for the plan set, ink rasters for auto count, the vector paths snapping reads (the JS uses pdf.js) | Apache-2.0 OR MIT |
| sha2 | io-takeoff | SHA-256 of a PDF's bytes: a sheet is known by its file's hash, as `pdfset.js` does with SubtleCrypto | MIT OR Apache-2.0 |
| miniz_oxide | io-takeoff, markup, io-markup, proj, io-civil, part, io-cad, cadcraft-cli | raw deflate for the XLSX zip entries (the JS uses CompressionStream); deflate / inflate for PDF streams (pdf-lib's flate streams), PNG pictures and the XLSX / BCF zips; zlib inflate of DEFLATE GeoTIFF blocks (geotiff.js uses pako); zlib deflate of the GeoTIFF export's blocks, the plot's PNG pictures' fallback and the CLI's inflate of PUBLISH's PDF for the kdiff log | MIT OR Zlib OR Apache-2.0 |
| unicode-normalization | takeoff, io-civil, part, engine | NFC/NFD for the ICU-style `localeCompare` the item and division sorts use; NFKD for .dbf field names and GIS export file names; NFD for the PDF font metrics (font.js); NFKD for a library item's block name (libplace.js `slugify`) | MIT OR Apache-2.0 |
| getrandom (0.3) | mcp (desktop only) | the control channel's token and session ids from the operating system's random source (AGENTS.md § 9) | MIT OR Apache-2.0 |
| jiff (0.2.38; `std`, `tz-system`, `tzdb-zoneinfo`, `tzdb-bundle-platform`; with jiff-core, jiff-tzdb-platform, jiff-tzdb on Windows) | clock (desktop only) | the system time zone (`TZ`, `/etc/localtime` + `/usr/share/zoneinfo`, the Windows zone through the bundled tzdb) so local-time fields (zip DOS dates, LandXML date / time, KISS headers, list dates) match the 0.5 JS's `getHours()` …; no build.rs, no unsafe in our crates; the web build uses js-sys `Date.getTimezoneOffset` instead | Unlicense OR MIT (used under MIT) |
| js-sys | clock (web only) | `Date.getTimezoneOffset` / `Date.now` in the browser build (already in the web build through wasm-bindgen) | MIT OR Apache-2.0 |
| libfuzzer-sys | fuzz/ (not shipped) | cargo-fuzz targets | MIT OR Apache-2.0 OR NCSA |
| zlib-rs | io-skp, bim, io-civil, io-cad | raw-deflate inflate of SketchUp 2021+ ZIP entries, BCF zips, CivilCraft's GIS zips / KMZ (inflate and deflate) (fastest pure-Rust inflater measured: 16 ms vs 21 ms for miniz_oxide on a 16 MB model.dat); `rust-allocator` feature, no C; raw-deflate inflate of SketchUp 2021+ ZIP entries; CivilCraft's GIS zips / KMZ (inflate and deflate) (fastest pure-Rust inflater measured: 16 ms vs 21 ms for miniz_oxide on a 16 MB model.dat); `rust-allocator` feature, no C; zlib deflate of the plot / PUBLISH PDF streams and picture data (2–3× miniz_oxide at level 6) | Zlib |
| crc32fast | io-skp | CRC-32 check of unpacked ZIP entries (SIMD at run time) | MIT OR Apache-2.0 |
| memchr | io-skp | substring search for record markers in classic SketchUp files | Unlicense OR MIT (used under MIT) |
| hayro-syntax | io-markup | PDF object syntax for MarkupCraft's pdf.js-side reading (xref tables and streams, object streams, repair, decryption); `std` feature only, no rendering; **vendored with one panic removed** (`vendor/hayro-syntax/CADCRAFT-PATCH.md`) | Apache-2.0 OR MIT |
| image (0.25, `png` + `jpeg` features: png, zune-jpeg) | io-markup | MarkupCraft's picture stamps and reply photos: PNG / JPEG decoded under `image::Limits` (header first), scaled to 1600 px and written again when bigger; BCF snapshots as PNG | MIT OR Apache-2.0 |
| indexmap | markup, io-markup | insertion-ordered maps (the JS Map / object order of markups, fields and refs) | Apache-2.0 OR MIT |
| url | io-civil, ui-egui (CivilCraft map layers' host names) | WHATWG URL parsing / serialising for the base-map service URLs (the JS uses `new URL` and `URLSearchParams`) | MIT OR Apache-2.0 (idna / ICU data crates: Unicode-3.0) |
| laz (laz-rs) | io-civil | LAZ (LASzip) point decompression for CivilCraft's LiDAR import (the JS uses laz-perf); deps byteorder, num-traits; no build.rs, no default features | Apache-2.0 |
| regex | io-civil | crs.js / crs-pick.js / raster.js / las.js patterns (.prj matching, lat/long text, GDAL metadata, vertical units) | MIT OR Apache-2.0 |
| robust | civil | Shewchuk's adaptive exact `incircle` for the TIN's Delaunay flips (tin.js reads only its sign) | MIT OR Apache-2.0 |
| fancy-regex | civil | JS regular expressions with lookaround and backtracking, for the deed-call, traverse and offset parsers that copy the 0.5 patterns as written | MIT |
| image (0.25; png, jpeg) | io-cad (also cadcraft-shot: png; ui-egui: CivilCraft's map picture in the PDF as JPEG and beside the DWG as PNG) | pictures in drawings (IMAGE, M4.19): PNG and JPEG decoded for the screen with `image::Limits`; jpeg pulls zune-jpeg + zune-core, png pulls png / fdeflate / crc32fast; default features off (no rayon, no other codecs) | MIT OR Apache-2.0 (zune-*: MIT OR Apache-2.0 OR Zlib) |

## Ported source code (no crate dependency)

| Source | Ported into | Why | Licence |
|---|---|---|---|
| delaunator 5.1.0 (github.com/mapbox/delaunator) | crates/civil/src/delaunator.rs | the TIN's Delaunay triangulation, giving the same triangles as the 0.5 JS | ISC (see NOTICE) |
| robust-predicates 3.0.3 `orient2d` (github.com/mourner/robust-predicates, after Shewchuk) | crates/civil/src/predicates.rs | the exact orientation test delaunator and tin.js use | Unlicense (public domain) |
| Manifold 3.5.4 (github.com/elalish/manifold): src/boolean3, boolean_result, csg_tree, edge_op, face_op, impl, polygon, properties, sort, tree2d, collider.h, shared.h, utils.h, parallel.h (sequential paths), include/manifold/{common,mesh,math,linalg}.h | cc-manifold (crates/manifold) | the mesh boolean behind IFC openings (cc-ifc geo-bool.js) and ModelCraft's solid tools, with output identical to the manifold-3d 3.5.4 wasm the 0.5 apps use | Apache-2.0 (notices kept in each file; see NOTICE) |
| — its math.h `acos` (FreeBSD msun via musl) | crates/manifold/src/math.rs | deterministic vertex pseudo-normals | SunPro notice (permissive), see NOTICE |
| — its math.h `sin` / `cos` / `asin` / `atan` / `atan2` (FreeBSD msun via musl) | crates/manifold/src/math.rs | deterministic trigonometry for `sind` / `cosd`, rotations, `TrimByPlane` | SunPro notice (permissive), see NOTICE |
| — its svd.h (github.com/wi-re/tbtSVD, modified by the Manifold Authors) | crates/manifold/src/mat.rs | the spectral norm that scales epsilon under a transform | MIT |
| — its tri_dist.h (from NVIDIA PhysX) | crates/manifold/src/transform.rs | triangle-triangle distance for `MinGap` | BSD-3-Clause |
| — its quickhull.cpp (derived from Antti Kuukka's public-domain QuickHull) | crates/manifold/src/quickhull.rs | the convex hull behind `MinkowskiSum` / `MinkowskiDifference` | Apache-2.0 / public domain |
| — its src/cross_section/cross_section.cpp | crates/manifold/src/cross_section.rs | the 2D `CrossSection` (constructor fill rules, booleans, lazy transforms, circle/square) | Apache-2.0 |
| Clipper2 by Angus Johnson, commit 46f6391 (github.com/AngusJohnson/Clipper2): clipper.core.h, clipper.h, clipper.engine.cpp (closed paths, `ClipperD` path output) | crates/manifold/src/clipper/ (mod.rs, engine.rs) | the polygon clipper under `CrossSection`, same arithmetic as the manifold-3d wasm | BSL-1.0 (licence text kept in clipper/mod.rs) |
| libc++ `std::sort` (LLVM 21: __algorithm/sort.h, partial_sort.h, sift_down.h, push_heap.h, pop_heap.h) | crates/manifold/src/clipper/sort.rs | Clipper2's intersection-list sort in the wasm's exact order (equal keys) | Apache-2.0 WITH LLVM-exception |
| — its disjoint_sets.h (github.com/wjakob/dset) | crates/manifold/src/disjoint.rs | union-find | Zlib |
| — its linalg.h (based on linalg.h 2.2, Sterling Orsten) | crates/manifold/src/linalg.rs | the vector operations, same evaluation order | Apache-2.0 / public domain |
| Adobe Core14 font metrics (Helvetica, Helvetica-Bold AFM) as pdf-lib 1.17.1 ships them | crates/io-takeoff/src/stdfont.rs (generated widths + kerning for WinAnsi) | the marked PDF and the proposal measure and wrap text exactly as pdf-lib's `widthOfTextAtSize` | Adobe AFM notice (redistribution permitted with the notice; kept in the file); pdf-lib MIT |
| proj4js 2.22.0 (github.com/proj4js/proj4js): lib/ (defs, projString, parseCode, Proj, datum, datumUtils, datum_transform, adjust_axis, transform, core, nadgrid, common/*, constants/*, projections: merc, longlat, tmerc, etmerc, utm, sterea, gauss, stere, somerc, omerc, lcc, krovak, cass, laea, aea, gnom, cea, eqc, poly, nzmg, mill, sinu, moll, eqdc, aeqd, eqearth) and its bundled wkt-parser 1.5 | cc-proj (crates/proj) | coordinate systems with V8-identical numbers (CivilCraft's crs.js runs on proj4js) | MIT (see crates/proj/NOTICE) |
| geotiff.js 3.0.5 (github.com/geotiffjs/geotiff.js): src/geotiff, geotiffimage, imagefiledirectory, globals, resample, rgb, predictor, compression/{lzw,packbits,deflate,raw} | crates/proj/src/tiff.rs | GeoTIFF elevation grids and PROJ grid shift files | MIT (see crates/proj/NOTICE) |
| fdlibm 5.3 e_log.c | crates/proj/src/math.rs | V8's Math.log (libm's log differs in the last bit for ~0.3 % of arguments) | SunPro notice (permissive), see NOTICE |

