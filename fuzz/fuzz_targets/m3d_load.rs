//! Fuzz the 3D model loader (ModelCraft's model.js `load`: a native file is user data) and what reads the model it
//! keeps: any JSON must load or be refused, write back (toJSON), triangulate (worldTriangles) and be checked as a
//! solid (solidProblems in every definition), never panic, hang or run out of memory.
#![no_main]

use cc_m3d::Model;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(data) else { return };
    let mut m = Model::new();
    if m.load(&v, false).is_err() {
        return;
    }
    let s = m.to_json_string(false);
    // what it writes reads back (a round trip may still refuse: numbers past ±1e7 are not taken back)
    if let Ok(j) = serde_json::from_str::<serde_json::Value>(&s) {
        let mut m2 = Model::new();
        let _ = m2.load(&j, false);
    }
    let _ = cc_m3d::mesh::world_triangles(&m, "root", &cc_m3d::vec::IDENT);
    for d in m.defs.values() {
        let _ = cc_m3d::solids::solid_problems(d);
        let _ = cc_m3d::solids::solid_info(d);
    }
    let _ = m.snapshot();
});
