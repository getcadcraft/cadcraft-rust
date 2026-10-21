//! Fuzz the constraints a file or the clipboard brings (gcons.js `resolveParametric`, `readClipConstraints`, M4.23) and
//! the solver behind them: any payload gives clean constraints or notes, and solving them (an edit that moves a
//! constrained object, `afterEdit`) never panics, hangs or runs out of memory.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 1 << 16 {
        return;
    }
    let text = String::from_utf8_lossy(data).into_owned();
    cc_gcons::install();
    let mut doc = cc_cad::Doc::new();
    // a few constrainable objects the payload's keys "1" … "6" refer to
    doc.load(&serde_json::json!({ "entities": [
        { "type": "line", "x1": 0, "y1": 0, "x2": 10, "y2": 0.5, "ccref": "1" },
        { "type": "line", "x1": 10, "y1": 0.5, "x2": 10.2, "y2": 8, "ccref": "2" },
        { "type": "circle", "cx": 20, "cy": 0, "r": 3, "ccref": "3" },
        { "type": "arc", "cx": 30, "cy": 0, "r": 4, "a0": 0, "a1": 2, "ccref": "4" },
        { "type": "point", "x": 5, "y": 5, "ccref": "5" },
        { "type": "poly", "closed": true, "pts": [{ "x": 40, "y": 0, "b": 0 }, { "x": 50, "y": 0, "b": 0.4 }, { "x": 50, "y": 9, "b": 0 }], "ccref": "6" }
    ], "parametric": text }));
    let _ = cc_gcons::finish_load(&mut doc);
    let tr = cc_cad::i18n::Catalog::default();
    let _ = cc_gcons::gcons::status_text(&doc, &tr);
    // move every object a little: the constraints follow, or the edit is refused
    let ids: Vec<u64> = doc.model().filter_map(|e| e.id).collect();
    for id in ids {
        let _ = doc.change("MOVE", |d| {
            if let Some(mut e) = d.get(id).cloned() {
                e = cc_cad_geom::tx::tx_entity(&e, &cc_cad_geom::geom::m_translate(0.5, 0.25));
                d.update(id, e)?;
            }
            Ok(())
        });
    }
    let _ = cc_gcons::paramsdoc::solve_all(&mut doc, &tr, "SOLVE");
    // the same JSON as clipboard constraints
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        let _ = cc_gcons::gcons::read_clip_constraints(v.get("constraints"), 6);
    }
});
