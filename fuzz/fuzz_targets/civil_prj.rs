//! Fuzz CivilCraft's .prj reader (GIS › Coordinates, a lone .prj dropped, a shapefile's .prj: crs.js crsFromPrj over
//! cc-proj's WKT parser): any bytes give a system, a refusal or an error, never a panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cc_io_civil::crs::set_catalog();
    cc_io_civil::crs::set_wkt_catalog();
    // gis-crs-ui.js readPrj: over 64 KB is not a .prj
    if data.len() > 64 * 1024 {
        return;
    }
    let text = String::from_utf8_lossy(data);
    if let Ok(r) = cc_io_civil::crs::crs_from_prj(&text) {
        let _ = cc_io_civil::crs::crs_problem(&r.crs, "ft");
        let _ = cc_io_civil::crs_pick::describe(&r.crs);
        if let Ok(Some(f)) = cc_io_civil::crs::frame(&r.crs) {
            let _ = f.to_lon_lat(1000.0, 1000.0);
        }
    }
});
