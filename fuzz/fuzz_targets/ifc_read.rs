//! Fuzz the IFC reader: any bytes → scan → every instance parsed → the geometry of every product (no boolean
//! engine). Never a panic, hang or OOM; inputs are capped at 64 KiB.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 1 << 16 {
        return;
    }
    let mut out = Vec::new();
    let _ = cc_io_ifc::engine_step::write_ndjson(data, &mut out);
    if let Ok(products) = cc_io_ifc::engine_geom::products(data, &cc_io_ifc::geo::bool_ops::NoSolidOps) {
        let _ = cc_io_ifc::engine_geom::to_json(&products);
    }
});
