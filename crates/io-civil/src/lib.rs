//! CadCraft L4: CivilCraft's files — the Rust form of the DOM-free file modules of CivilCraft 0.5
//! (`public_html/assets/apps/civilcraft/dev/`), one module per JS file, checked against the frozen JS by
//! `forks/tools/cvdiff.mjs` (`cadcraft-cli civil`). Every reader treats its input as hostile (caps on counts and
//! sizes, bounded nesting) and is fuzzed (`fuzz/fuzz_targets/civil_*`).

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]
#![deny(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]
// `!(x > y)` is how the JS says "not greater, or NaN": kept as written for parity.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::nonminimal_bool)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::type_complexity)]

pub mod basemap;
pub mod block_points;
pub mod cad_linework;
pub mod civil_ops;
pub mod crs;
pub mod crs_ops;
pub mod crs_pick;
pub mod dbf;
pub mod dwg_write;
pub mod dxf;
pub mod font;
pub mod geojson;
pub mod geotiff_write;
pub mod gis_export;
pub mod gis_geom;
pub mod gis_layers;
pub mod gis_ops;
pub mod gis_read;
pub mod gjs;
pub mod gpkg;
pub mod kml;
pub mod landxml;
pub mod las;
pub mod plot;
pub mod pointcloud;
pub mod raster;
pub mod scene;
pub mod script;
pub mod shapefile;
pub mod sheets;
pub mod site3d_export;
pub mod sqlite;
pub mod terrain;
pub mod terrain_ops;
pub mod thin;
pub mod xml;
pub mod zipfile;
