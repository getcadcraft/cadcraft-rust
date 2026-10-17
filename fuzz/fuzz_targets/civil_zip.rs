//! Fuzz CivilCraft's zip reader and the GIS file reader on top of it: any bytes → list_zip / unzip_entry (capped),
//! and read_gis_files with the bytes as a .zip, a .kmz, a .geojson and a .gpkg, must give data or an error, never a
//! panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(list) = cc_io_civil::zipfile::list_zip(data) {
        for e in list.iter().take(64) {
            let _ = cc_io_civil::zipfile::unzip_entry(data, e, 1 << 22);
        }
    }
    let name = match data.first().map(|b| b % 4) {
        Some(0) => "f.zip",
        Some(1) => "f.kmz",
        Some(2) => "f.geojson",
        _ => "f.gpkg",
    };
    let _ = cc_io_civil::gis_read::read_gis_files(&[(name.to_string(), data.to_vec())], 10_000);
});
