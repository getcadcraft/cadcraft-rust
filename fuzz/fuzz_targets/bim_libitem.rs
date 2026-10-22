//! Fuzz the Library panel's file adapters (libitem.js, M7.17): any bytes as a CSV, glTF / GLB, OBJ, STL or Revit family
//! (and any JSON as an item to validate) give items and notes, never a panic, hang or OOM.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 1 << 20 {
        return;
    }
    let (k, rest) = data.split_first().map_or((0, data), |(k, r)| (*k, r));
    use cc_bim::libitem as li;
    match k % 7 {
        0 => drop(li::items_from_csv(&String::from_utf8_lossy(rest), None, None)),
        1 => drop(li::mesh::item_from_mesh(rest, "glb", None, None, "x")),
        2 => drop(li::mesh::item_from_mesh(rest, "gltf", Some("mm"), Some("z"), "")),
        3 => drop(li::mesh::item_from_mesh(rest, "obj", None, None, "")),
        4 => drop(li::mesh::item_from_mesh(rest, "stl", Some("in"), Some("y"), "")),
        5 => {
            if let Ok(f) = li::rfa::read_revit_family(rest) {
                drop(li::rfa::item_from_rfa(f.as_ref(), "x.rfa"));
            }
        }
        _ => {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(rest) {
                let _ = li::validate_item(&v, false);
                let _ = li::item_hash(&v);
                let _ = li::plan_bounds(v.get("plan"));
            }
        }
    }
});
