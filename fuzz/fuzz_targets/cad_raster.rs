//! Fuzz the picture decoders (IMAGE, M4.19: PNG / JPEG through the `image` crate with limits; M4.21: pictures.js's own
//! PNG decoder and JPEG header reader for plotted PDFs, with and without a dpi cap): any bytes give pixels within the
//! caps or an error, never a panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(r) = cc_io_cad::raster::decode(data, 1024) {
        assert!(r.w <= 1024 && r.h <= 1024 && r.px.len() as u64 == u64::from(r.w) * u64::from(r.h) * 4);
    }
    if let Some((mime, w, h)) = cc_cad::pictures::sniff_picture(data) {
        let rec = cc_cad::pictures::PicRec { name: "f".into(), bytes: data.into(), mime, w, h };
        for (dpi, mm) in [(0.0, 10.0), (150.0, 5.0)] {
            if let Some(im) = cc_io_cad::pdfimage::pdf_image(&rec, dpi, mm, mm) {
                // a decoded PNG has pixels; a JPEG goes in as its header says (0 × 0 included, as pictures.js does)
                assert!(im.filter != "FlateDecode" || (im.w > 0 && im.h > 0));
            }
        }
    }
    let _ = cc_io_cad::pdfimage::decode_png(data);
    let _ = cc_io_cad::pdfimage::jpeg_info(data);
});
