//! CadCraft L2: coordinate reference systems — a one-for-one Rust port of proj4js 2.22.0 (MIT,
//! github.com/proj4js/proj4js, Copyright (c) 2014, Mike Adair, Richard Greenwood, Didier Richard, Stephen Irons, Olivier
//! Terral, Calvin Metcalf and contributors) with the wkt-parser it bundles (MIT), and the GeoTIFF reading of geotiff.js
//! 3.0.5 (MIT) that its grid shift files and CivilCraft's elevation grids go through. Numbers are the vendored proj4's:
//! the same operations in the same order, V8's `Math` functions ([`math`]), checked by `forks/tools/cvdiff.mjs`.
//!
//! - [`defs`]: proj strings, the built-in names, `parse`; [`wkt`]: WKT 1 / WKT 2 → a definition
//! - [`proj::Proj`]: a projection with its datum; [`projections`]: every projection's init / forward / inverse
//! - [`transform`]: datum shifts, grid shifts, `proj4(from, to)`; [`nadgrid`]: NTv2 / GeoTIFF grids
//! - [`tiff`]: the GeoTIFF reader

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
// the projections are floating-point kernels ported line for line; `!(x > y)` keeps the JS's NaN behaviour
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::nonminimal_bool, clippy::excessive_precision, clippy::approx_constant)]
#![allow(clippy::cast_precision_loss, clippy::many_single_char_names, clippy::too_many_arguments)]
// kept in proj4's own form so the port reads against the source: `-1 * x`, `a = a + b`, index loops over the series
// tables, late-initialised `var`s; `clamp` is not used because it differs from the JS's `min`/`max` on NaN
#![allow(clippy::neg_multiply, clippy::assign_op_pattern, clippy::needless_range_loop, clippy::manual_clamp, clippy::needless_late_init)]
#![allow(clippy::field_reassign_with_default, clippy::collapsible_if, clippy::collapsible_match, clippy::type_complexity)]

pub mod common;
pub mod defs;
pub mod math;
pub mod nadgrid;
pub mod proj;
pub mod projections;
pub mod tables;
pub mod tiff;
pub mod transform;
pub mod val;
pub mod wkt;

pub use proj::{Out, Proj, Pt};
pub use transform::{Converter, JsNum, transformer};
