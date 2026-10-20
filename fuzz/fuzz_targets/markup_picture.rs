//! MarkupCraft's picture reader (stamps, reply photos): any bytes must give a data URL or an error, never a panic or a
//! huge allocation.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(url) = cc_io_markup::picture::picture_from(data) {
        let _ = cc_io_markup::picture::decode_rgba(&url);
    }
});
