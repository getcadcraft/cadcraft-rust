//! Fuzz CivilCraft's LiDAR reader (LAS 1.0–1.4 headers, VLRs / EVLRs, the coordinate system records, points, LAZ
//! through laz-rs): any bytes give points or an error, never a panic, hang or OOM.
#![no_main]

use cc_io_civil::raster::InFile;
use libfuzzer_sys::fuzz_target;

/// laz-rs can panic on damaged compressed data (a division by a zero interval, its debug assertions); cc-io-civil runs it
/// under `catch_unwind` and turns that into the JS's "stopped" note. libFuzzer's hook aborts on every panic, so panics
/// raised inside laz-rs pass through to that `catch_unwind`; a panic anywhere else still aborts (a real find).
fn guard() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.location().is_some_and(|l| l.file().contains("/laz-")) {
                return;
            }
            prev(info);
        }));
    });
}

fuzz_target!(|data: &[u8]| {
    guard();
    cc_io_civil::crs::set_catalog();
    let f = InFile { name: "f.las".into(), bytes: data.to_vec() };
    if let Ok(mut h) = cc_io_civil::las::read_las_header(&f) {
        let mut n = 0u64;
        let _ = cc_io_civil::las::scan_points(&f, &mut h, &mut |x, y, z, c| {
            n = n.wrapping_add(1);
            std::hint::black_box((x, y, z, c));
        });
    }
});
