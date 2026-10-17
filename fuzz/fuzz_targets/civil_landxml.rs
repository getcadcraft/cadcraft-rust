//! Fuzz the LandXML reader: any bytes (lossy UTF-8) → read_land_xml → scale_import, and the surfaces it finds built.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Ok(d) = cc_io_civil::landxml::read_land_xml(&text) {
        let _ = cc_io_civil::landxml::scale_import(d, 0.3048);
    }
});
