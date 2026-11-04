//! Fuzz the rest of BimCraft's Library sources and what is done with an item (M7.18–M7.20): any bytes as a DXF or DWG
//! whose blocks become items (libitem.js itemsFromDwg / itemFromBlock), as an IFC whose types become items
//! (libitem-ifc.js), and any JSON as an item placed into a small model, swapped, copied as a clip and pasted — items,
//! notes or an error, never a panic, hang or OOM.
#![no_main]

use std::sync::Arc;

use libfuzzer_sys::fuzz_target;

fn small_model() -> Option<cc_bim::draw::editor::Editor> {
    use cc_bim::draw::newmodel::{NewBuilding, NewLevel, new_building};
    let mut rand = cc_bim::wapi::Rand::new(7);
    let levels = vec![NewLevel { name: "L1".into(), elevation: 0.0, height: 3000.0 }, NewLevel { name: "L2".into(), elevation: 3000.0, height: 3000.0 }];
    let nb = NewBuilding { name: "F".into(), site_name: "S".into(), levels, author: String::new(), file_name: "f.ifc".into(), now_ms: 0.0, version: String::new() };
    let text = new_building(&nb, &mut rand).ok()?;
    cc_bim::draw::editor::Editor::open(Arc::from(text.into_bytes().into_boxed_slice()), 3).ok()
}

fuzz_target!(|data: &[u8]| {
    if data.len() > 1 << 20 {
        return;
    }
    let (k, rest) = data.split_first().map_or((0, data), |(k, r)| (*k, r));
    use cc_bim::libitem as li;
    match k % 4 {
        0 | 1 => {
            let db = if k % 4 == 0 {
                cc_io_dwg::dxf_model::read_dxf_model(&cc_io_dwg::dxf_model::decode_dxf_bytes(rest))
            } else {
                cc_io_dwg::read_dwg(rest, &cc_io_dwg::ReadOpts::default())
            };
            if let Ok((_, cands)) = li::dwg::items_from_dwg(&db, Some("mm"), None, "x.dwg", true) {
                for c in cands.iter().take(8) {
                    let _ = li::dwg::item_from_block(&db, &c.name, None, "x.dwg", None);
                }
            }
            let _ = li::dwg::items_from_dwg(&db, None, None, "x.dxf", false);
        }
        2 => {
            let _ = li::ifc::items_from_ifc(Arc::from(rest), false, None);
        }
        _ => {
            let Ok(item) = serde_json::from_slice::<serde_json::Value>(rest) else { return };
            let _ = cc_bim::draw::clip::box_from_item(&item);
            let Some(mut ed) = small_model() else { return };
            let Some(storey) = ed.api.ids("IFCBUILDINGSTOREY", false).first().copied() else { return };
            let opts = cc_bim::draw::place::PlaceOpts { storey, x: 100.0, y: 200.0, rot: 30.0, item_number: "1".into(), quantity: 1.0, stand_in_height: 900.0, ..Default::default() };
            let item = Arc::new(item);
            if let Ok(p) = ed.place_item(item.clone(), opts.clone()) {
                let _ = ed.swap_item(p.id, item.clone(), opts.clone(), None, "swap");
                let _ = ed.place_like(p.id, opts, "");
                if let Ok(snap) = ed.snapshot(&[p.id]) {
                    let _ = ed.paste_elements(Arc::new(snap), cc_bim::draw::place::PasteOpts { dx: 500.0, rot: 90.0, ..Default::default() }, "");
                }
                let _ = ed.remove_elements(&[p.id], "");
                let _ = ed.undo();
                let _ = ed.redo();
            }
        }
    }
});
