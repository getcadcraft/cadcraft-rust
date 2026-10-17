//! Fuzz CivilCraft's elevation grids (GeoTIFF through cc-proj's geotiff.js port, ESRI ASCII grids) and grid shift
//! files (NTv2, PROJ GeoTIFF): any bytes give a grid or an error, never a panic, hang or OOM.
#![no_main]

use cc_io_civil::raster::{self, InFile};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&sel, rest)) = data.split_first() else { return };
    cc_io_civil::crs::set_catalog();
    let name = match sel % 4 {
        0 => "f.tif",
        1 => "f.asc",
        2 => "grid.gsb",
        _ => "grid.tif",
    };
    if sel % 4 >= 2 {
        let _ = cc_io_civil::crs::load_grid(name, rest);
        return;
    }
    let files = vec![InFile { name: name.into(), bytes: rest.to_vec() }];
    if let Ok(mut src) = raster::open_raster(&files) {
        // a bounded window (a full read of a claimed 20 000 × 20 000 grid is the JS's behaviour too, but not a fuzz input)
        let _ = raster::read_window(&src, Some(0.0), Some(0.0), Some(64.0), Some(64.0), Some(f64::from(sel >> 4) + 1.0));
        if src.width * src.height <= 1e6 {
            let _ = raster::sample_stats(&mut src);
        }
    }
    if sel % 4 == 0 {
        let _ = raster::probe_tiff(rest);
        if rest.len() < 1 << 16 {
            let _ = raster::read_geo_image("f.tif", rest);
        }
    }
});
