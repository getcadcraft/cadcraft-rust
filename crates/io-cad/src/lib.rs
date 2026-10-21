//! cc-io-cad: the CadCraft drafting app's drawing import and export, ported from the 0.5 JavaScript
//! (`dwg-import.js`, `dxf.js`, `dwg-save.js`, `dxf2000.js` and their helpers) at exact output parity.
//!
//! - [`dwg_import::import_dwg`]: a drawing model (cc-io-dwg's `read_dwg`, or `dxf_model`) → what `Doc::load` takes.
//! - [`dxf_read::read_dxf`]: ASCII DXF text → what `Doc::load` takes.
//! - [`dxf_write::write_dxf`]: a `Doc` → DXF R12 text (byte-identical to the JS `writeDxf`) and its report.
//! - [`dwg_save::save_dwg`]: a `Doc` → DWG 2000 … 2018 through cc-io-dwg's writer (byte-identical to the JS `saveDwg`
//!   with the same creation time and GUID source) and its report.
//! - [`dxf2000::write_dxf2000`]: a `Doc` → DXF R2000 text (byte-identical to the JS `writeDxf2000`) and its report.
//! - [`raster`]: a picture file (PNG / JPEG, hostile bytes) → RGBA pixels for the screen, under size caps.
//! - [`plot`]: plot.js — a layout or the model on a paper → a vector PDF (and agents.js `plotPdf`).
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
pub mod pdfimage;
pub mod plot;
pub mod props;
pub mod raster;
pub mod small;
pub mod text;
pub mod vports;

pub use dwg_import::import_dwg;
pub use jsv::JsErr;

/// The save formats of CadCraft's Save as: the DWG versions of [`dwg_save`], then `dxf12` and `dxf2000`.
pub const SAVE_FORMATS: &[&str] = &["2018", "2013", "2010", "2007", "2004", "2000", "dxf2000", "dxf12"];

/// A drawing file (DWG, or DXF when `name` ends in `.dxf`) → what `Doc::load` takes, as the 0.5 app opens a file to edit
/// (`importDwg(readDwg(bytes), { versionTag })` / `readDxf(text)`).
pub fn import_bytes(name: &str, bytes: &[u8]) -> Result<serde_json::Value, JsErr> {
    if name.to_ascii_lowercase().ends_with(".dxf") {
        let text = cc_io_dwg::dxf_model::decode_dxf_bytes(bytes);
        return dxf_read::read_dxf(&text);
    }
    let tag = String::from_utf8_lossy(bytes.get(..6).unwrap_or(bytes)).into_owned();
    let model = cc_io_dwg::read_dwg(bytes, &cc_io_dwg::ReadOpts::default());
    import_dwg(&model, &tag)
}

/// Save a document in one of [`SAVE_FORMATS`]: (bytes, the file extension, the save report).
pub fn save_bytes(doc: &cc_cad::Doc, format: &str, created_ms: Option<f64>) -> Result<(Vec<u8>, &'static str, serde_json::Value), JsErr> {
    match format {
        "dxf12" => dxf_write::write_dxf(doc).map(|(t, r)| (t.into_bytes(), "dxf", r)),
        "dxf2000" => dxf2000::write_dxf2000(doc).map(|(t, r)| (t.into_bytes(), "dxf", r)),
        v if dwg_save::dwg_version_key(v).is_some() => {
            let opts = dwg_save::SaveOpts { version: v.to_string(), created_ms, ..Default::default() };
            dwg_save::save_dwg(doc, &opts).map(|s| (s.bytes, "dwg", s.report))
        }
        other => Err(JsErr(format!("unknown save format {other}"))),
    }
}
