//! Inputs the fuzz targets found (fuzz/fuzz_targets/markup_*.rs); each one panicked before its fix.

use cc_io_markup::read::{OpenOpts, open_pdf};
use cc_markup::model::Env;

fn open(bytes: &[u8]) {
    let mut env = Env { now_ms: 0.0, count: 0, ..Env::default() };
    let _ = open_pdf(&mut env, bytes, &OpenOpts { file_id: "f".into(), name: "x.pdf".into(), read_text: true });
}

/// An Info dictionary holding a broken hex string (`<FEFF004?…>`): hayro's `Dict::entries` unwrapped the value.
#[test]
fn info_with_a_broken_hex_string() {
    open(include_bytes!("data/fuzz-info-bad-hex.pdf"));
}

/// The same broken hex string inside an array (hayro's array iterator unwrapped it too).
#[test]
fn array_with_a_broken_hex_string() {
    let pdf = b"%PDF-1.7\n1 0 obj\n<< /Type /Pages /Kids [ 3 0 R ] /Count 1 >>\nendobj\n2 0 obj\n<< /Type /Catalog /Pages 1 0 R /PageLabels << /Nums [ 0 << /P <FE?FF> >> ] >> >>\nendobj\n3 0 obj\n<< /Type /Page /Parent 1 0 R /MediaBox [ 0 0 612 <79?2> ] /Annots [ <00?> 4 0 R ] >>\nendobj\n4 0 obj\n<< /Type /Annot /Subtype /Square /Rect [ 1 1 <2?> 50 ] >>\nendobj\ntrailer\n<< /Root 2 0 R /Size 5 >>\n%%EOF\n";
    open(pdf);
}

/// An array hayro skimmed as valid that does not read back, reached while loading the pages (2026-10-18): its array
/// iterator unwrapped the item. Fixed in the vendored hayro-syntax (vendor/hayro-syntax/CADCRAFT-PATCH.md).
#[test]
fn array_that_does_not_read_back() {
    let data = include_bytes!("data/fuzz-array-unwrap.pdf");
    open(data);
    if let Ok(doc) = cc_io_markup::jsdoc::JsDoc::open(data) {
        let segs = cc_markup::vecdiff::page_segments(&cc_io_markup::ops::page_ops(&doc, 0), 100_000);
        std::hint::black_box((segs.len(), cc_io_markup::text::text_content(&doc, 0).len()));
    }
}
