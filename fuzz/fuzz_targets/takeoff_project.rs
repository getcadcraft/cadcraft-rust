//! Fuzz TakeoffCraft's project-file reader (model.js `readProject`; a .takeoffcraft file is user data) and what runs on
//! the state it keeps: any bytes must parse (depth-capped JSON) and load or be refused, then quantities, the estimate,
//! the area / alternate estimates, the CSV and workbook sheets, the 3D scene of every sheet, the file written back and
//! read again, and the clipboard reader on the same JSON. Never a panic, a hang or an out-of-memory.
#![no_main]

use cc_takeoff::{clipboard, jv, model, scene3d};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let Ok(v) = jv::parse_json(text, 512) else { return };
    let _ = clipboard::read_clip(&v);
    let mut t = model::Takeoff::default();
    if t.load(&v).is_err() {
        return;
    }
    let st = &t.state;
    let q = model::quantities(st, &model::QOpts::default());
    let est = model::estimate(st, &q);
    let _ = jv::stringify(&q.to_json(st));
    let base = model::base_estimate(st);
    let _ = model::area_estimates(st);
    let _ = model::alternates(st);
    let label = |k: &str| k.to_string();
    let _ = cc_io_takeoff::export::estimate_csv(st, &base);
    let _ = cc_io_takeoff::export::estimate_sheets(st, &label, &est);
    let sheets: Vec<String> = st.get("sheets").and_then(|s| s.as_object()).map(|m| m.keys().cloned().collect()).unwrap_or_default();
    for k in sheets.iter().take(8) {
        if let Ok(sc) = scene3d::build_scene(st, k, &scene3d::View::default()) {
            let _ = cc_io_takeoff::glb::scene_to_glb(&sc.to_json(), "fuzz");
        }
    }
    let back = jv::stringify(&t.to_json("T"));
    if let Ok(j) = jv::parse_json(&back, 512) {
        let mut t2 = model::Takeoff::default();
        let _ = t2.load(&j);
    }
});
