//! Fuzz PartCraft's mesh kernel (kernel.js `regenerate`, cc-part kernel.rs on cc-manifold): any bytes that read as a part
//! (part.js `readPart`: a `.partcraft.json` is user data) must rebuild into a mesh or into feature errors — never a
//! panic, a hang or an allocation past the kernel's caps. The resolution is held low and the feature count capped so a
//! case stays quick (a modelled thread or a big pattern is the kernel's slowest path, tested by pcdiff instead).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let raw = match serde_json::from_slice::<serde_json::Value>(data) {
        Ok(v) => v,
        Err(_) => return,
    };
    let Ok(mut part) = cc_part::part::read_part(&raw) else { return };
    if cc_part::part::features(&part).len() > 12 {
        return;
    }
    if let Some(o) = part.as_object_mut() {
        o.insert("resolution".into(), serde_json::json!(8));
    }
    let r = cc_part::kernel::regenerate(&part, None);
    let _ = r.to_json();
    if let Some(m) = &r.raw_mesh {
        let dm = cc_part::drawing::DMesh { positions: m.positions.clone(), indices: m.indices.clone() };
        let _ = cc_part::kernel_section::mesh_views_section(&dm, false, Some(("front", None)));
    }
});
