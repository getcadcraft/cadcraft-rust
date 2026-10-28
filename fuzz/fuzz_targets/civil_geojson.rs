//! Fuzz CivilCraft's GeoJSON reader: any text → read_geojson must give layers or the JS error message, never a panic,
//! hang or OOM; the one-pass reader gives what the JSON-tree reader gives; what it reads is written again (RFC 7946) and
//! read back.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let fast = cc_io_civil::geojson::read_geojson(&text, "f", 10_000);
    let tree = cc_io_civil::geojson::read_geojson_tree(&text, "f", 10_000);
    assert_eq!(format!("{fast:?}"), format!("{tree:?}"), "the two GeoJSON readers differ");
    if let Ok(r) = fast {
        let groups: Vec<cc_io_civil::geojson::WGroup> = r
            .layers
            .into_iter()
            .map(|l| cc_io_civil::geojson::WGroup { kind: l.name, geom: l.geom, has_z: l.has_z, fields: l.fields, features: l.features })
            .collect();
        let out = cc_io_civil::geojson::write_geojson(&groups, &|x, y| (x, y), &|z| z, Some("f"));
        let _ = cc_io_civil::geojson::read_geojson(&out, "b", 10_000);
    }
});
