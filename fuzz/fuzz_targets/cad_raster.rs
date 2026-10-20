//! Fuzz the picture decoder (IMAGE, M4.19: PNG / JPEG through the `image` crate with limits): any bytes give pixels
//! within the caps or an error, never a panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(r) = cc_io_cad::raster::decode(data, 1024) {
        assert!(r.w <= 1024 && r.h <= 1024 && r.px.len() as u64 == u64::from(r.w) * u64::from(r.h) * 4);
    }
    let _ = cc_cad::pictures::sniff_picture(data);
});
