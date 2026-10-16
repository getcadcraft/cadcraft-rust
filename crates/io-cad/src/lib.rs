//! cc-io-cad: the CadCraft drafting app's drawing import and export, ported from the 0.5 JavaScript
//! (`dwg-import.js`, `dxf.js`, `dwg-save.js`, `dxf2000.js` and their helpers) at exact output parity.
//!
//! - [`dwg_import::import_dwg`]: a drawing model (cc-io-dwg's `read_dwg`, or `dxf_model`) → what `Doc::load` takes.
//! - [`dxf_read::read_dxf`]: ASCII DXF text → what `Doc::load` takes.
//! - [`dxf_write::write_dxf`]: a `Doc` → DXF R12 text (byte-identical to the JS `writeDxf`) and its report.
//! - [`dwg_save::save_dwg`]: a `Doc` → DWG 2000 … 2018 through cc-io-dwg's writer (byte-identical to the JS `saveDwg`
//!   with the same creation time and GUID source) and its report.
//! - [`dxf2000::write_dxf2000`]: a `Doc` → DXF R2000 text (byte-identical to the JS `writeDxf2000`) and its report.
//!
//! Values are the dynamic [`cc_io_dwg::V`] while importing (JS semantics: [`jsv`]); the result converts to
//! `serde_json` for `Doc::load` (NaN / ±Infinity as "$NaN" / "$Inf" / "$-Inf"; -0 kept). Strings come from the 0.5
//! `lang/en.json` catalog ([`i18n::Catalog::en`]) unless the caller passes another catalog.

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
#![allow(clippy::neg_cmp_op_on_partial_ord)]
// float group codes matched as `c if c == 43.0` (float literal patterns read worse)
#![allow(clippy::redundant_guards)]

pub mod attrs;
pub mod dwg_import;
pub mod dwg_save;
pub mod dxf2000;
pub mod dxf_read;
pub mod dxf_write;
pub mod dyn_blocks;
mod en;
pub mod i18n;
pub mod jsv;
pub mod m3d_file;
pub mod model3d;
pub mod props;
pub mod small;
pub mod text;
pub mod vports;

pub use dwg_import::import_dwg;
pub use jsv::JsErr;
