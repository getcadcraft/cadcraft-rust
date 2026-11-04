//! Fuzz BimCraft's ground (M7.21, res-terrain.js): any bytes as survey text (parsePointText), as a DXF or DWG whose
//! polylines, lines and points become contours (drawingContours), then the points as a terrain placed in a small model
//! with a pad, paving and LandXML — a ground, an error or nothing, never a panic, hang or OOM.
#![no_main]

use std::sync::Arc;

use cc_bim::draw::ops::Op;
use cc_bim::draw::resops::ResOp;
use libfuzzer_sys::fuzz_target;
use serde_json::json;

fn small_model() -> Option<cc_bim::draw::editor::Editor> {
    use cc_bim::draw::newmodel::{NewBuilding, NewLevel, new_building};
    let mut rand = cc_bim::wapi::Rand::new(11);
    let levels = vec![NewLevel { name: "L1".into(), elevation: 0.0, height: 3000.0 }];
    let nb = NewBuilding { name: "G".into(), site_name: "S".into(), levels, author: String::new(), file_name: "g.ifc".into(), now_ms: 0.0, version: String::new() };
    let text = new_building(&nb, &mut rand).ok()?;
    cc_bim::draw::editor::Editor::open(Arc::from(text.into_bytes().into_boxed_slice()), 3).ok()
}

fuzz_target!(|data: &[u8]| {
    if data.len() > 1 << 20 {
        return;
    }
    let (k, rest) = data.split_first().map_or((0, data), |(k, r)| (*k, r));
    let (mut pts, lines) = match k % 3 {
        0 => (cc_bim::draw::resground::parse_point_text(&String::from_utf8_lossy(rest), "auto", 4000), Vec::new()),
        1 => {
            let db = cc_io_dwg::dxf_model::read_dxf_model(&cc_io_dwg::dxf_model::decode_dxf_bytes(rest));
            let (l, p) = cc_bim::draw::resground::drawing_contours(&db, 4000);
            (p, l)
        }
        _ => {
            let db = cc_io_dwg::read_dwg(rest, &cc_io_dwg::ReadOpts::default());
            let (l, p) = cc_bim::draw::resground::drawing_contours(&db, 4000);
            (p, l)
        }
    };
    let off = cc_bim::draw::resground::auto_offset(&pts, 1000.0, [0.0, 0.0]);
    pts = cc_bim::draw::resground::thin_to_grid(&cc_bim::draw::resground::to_model(&pts, 1000.0, off, 0.0), 500.0);
    if pts.len() < 3 {
        return;
    }
    cc_bim::draw::resterrain::set_ground_loaded(true);
    let Some(mut ed) = small_model() else { return };
    let j = |v: &[cc_civil::geo::P3]| v.iter().take(3000).map(|p| json!({ "x": p.x, "y": p.y, "z": p.z })).collect::<Vec<_>>();
    let spec = json!({ "points": j(&pts), "lines": lines.iter().take(50).map(|l| j(l)).collect::<Vec<_>>(), "interval": 304.8 });
    if ed.model_change(Op::Res(ResOp::new("placeTerrain", vec![spec])), "t", "p").is_err() {
        return;
    }
    let level = ed.api.ids("IFCBUILDINGSTOREY", false).first().copied();
    let lg = level.and_then(|l| ed.api.line(l).ok().flatten()).and_then(|l| l.a("GlobalId").val_str()).unwrap_or_default();
    let hard = json!({ "level": lg, "points": [[0, 0], [3000, 0], [3000, 2000]], "kind": "walk" });
    let _ = ed.model_change(Op::Res(ResOp::new("placeHardscape", vec![hard])), "h", "p");
    if let Ok(mut w) = cc_bim::draw::writer::W::new(&mut ed.api)
        && let Ok(Some(t)) = cc_bim::draw::resterrain::read_terrain(&mut w)
        && let Some(ex) = cc_bim::draw::resterrain::existing_surface(&t)
    {
        let _ = ex.contours(304.8, 0.0, 2000.0);
        let _ = cc_bim::draw::resground::land_xml(&[("e".into(), &ex)], "n", "ft", "2026-01-01");
    }
    let _ = ed.undo();
});
