//! Ported from the frozen Node test (bin/test-takeoffcraft.mjs at 594ab3e): "Quantities, wall area, waste and cost",
//! "Undo / redo", "Project file round trip", "Arcs in the project file, batch marks", "Linear deducts", "Bid areas",
//! "Back-out areas", "Slope", "Multi-condition items", "Typical groups", "Alternates", "Crew labor", "Bid summary",
//! "Earthwork hand-off", "Markups", the model half of "Revision overlay" and "Clipboard".

use cc_takeoff::bid;
use cc_takeoff::clipboard::{self, CopyCtx, PasteOpts};
use cc_takeoff::jv::{self, Obj};
use cc_takeoff::model::{self, QOpts, Takeoff};
use cc_takeoff::overlay;
use cc_takeoff::units as u;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::f64::consts::PI;

fn near(a: f64, b: f64, t: f64) -> bool {
    (a - b).abs() <= t * 1f64.max(a.abs()).max(b.abs())
}
fn o(v: Value) -> Obj {
    v.as_object().cloned().unwrap_or_default()
}
fn rate(s: &str) -> f64 {
    u::parse_rate(s).unwrap()
}
fn rect(x: f64, y: f64, w: f64, h: f64) -> Value {
    json!([{ "x": x, "y": y }, { "x": x + w, "y": y }, { "x": x + w, "y": y + h }, { "x": x, "y": y + h }])
}
fn preset(k: &str, label: &str) -> Value {
    json!({ "method": "preset", "preset": k, "ftPerUnit": u::preset_ft_per_unit(k, 1.0).unwrap(), "label": label, "cal": null })
}
fn q(t: &Takeoff) -> model::Quantities {
    model::quantities(&t.state, &QOpts::default())
}
fn row<'a>(e: &'a model::Estimate, name: &str) -> &'a Value {
    e.rows.iter().find(|r| model::rstr(r, "name") == name).unwrap()
}
fn n(r: &Value, k: &str) -> f64 {
    model::rnum(r, k)
}
fn mark(t: &mut Takeoff, v: Value) -> String {
    t.add_mark(&o(v)).unwrap().unwrap()
}
fn item(t: &mut Takeoff, v: Value) -> String {
    t.add_item(&o(v)).unwrap().unwrap()
}

fn kitchen_style() -> (Takeoff, String, String, String, String, String, String, String) {
    let mut t = Takeoff::default();
    let hash = "a".repeat(64);
    t.add_file(&o(json!({ "hash": hash, "name": "set.pdf", "size": 100, "pages": 2 })));
    let s0 = model::sheet_key(&hash, 0.0);
    let s1 = model::sheet_key(&hash, 1.0);
    t.set_scale(&s0, &preset("a1/4", "1/4\" = 1'-0\""), None).unwrap();
    let wall = item(
        &mut t,
        json!({ "name": "Partition type A", "type": "linear", "heightFt": 9, "waste": 1000, "mat": rate("2.10"), "lab": rate("3.333") }),
    );
    mark(&mut t, json!({ "item": wall, "sheet": s0, "pts": [{ "x": 0, "y": 0 }, { "x": 36, "y": 0 }, { "x": 36, "y": 54 }] }));
    mark(&mut t, json!({ "item": wall, "sheet": s0, "pts": [{ "x": 100, "y": 100 }, { "x": 280, "y": 100 }] }));
    t.add_component(&wall, &o(json!({ "name": "Studs 16\" o.c.", "unit": "EA", "formula": "ceil(L / 1.333) + N", "mat": rate("4.25") })))
        .unwrap();
    t.add_component(&wall, &o(json!({ "name": "Gypsum board 4x8", "unit": "sheets", "formula": "ceil(A * 2 / 32)", "waste": 1000, "mat": rate("14.98"), "lab": rate("22") }))).unwrap();
    t.add_component(&wall, &o(json!({ "name": "Broken", "unit": "EA", "formula": "L / (H - 9)", "mat": 10000 }))).unwrap();
    let floor = item(
        &mut t,
        json!({ "name": "VCT flooring", "type": "area", "unit": "SF", "waste": 500, "mat": rate("1.85"), "lab": rate("1.1") }),
    );
    mark(&mut t, json!({ "item": floor, "sheet": s0, "pts": rect(0.0, 0.0, 360.0, 180.0), "holes": [rect(36.0, 36.0, 36.0, 36.0)] }));
    t.add_component(&floor, &o(json!({ "name": "Base cove", "unit": "LF", "formula": "P", "mat": rate("0.89") }))).unwrap();
    let sy = item(&mut t, json!({ "name": "Carpet", "type": "area", "unit": "SY", "mat": rate("27") }));
    mark(&mut t, json!({ "item": sy, "sheet": s0, "pts": rect(0.0, 0.0, 54.0, 54.0) }));
    let dev = item(&mut t, json!({ "name": "Duplex outlets", "type": "count", "lab": rate("45"), "mat": rate("3.5") }));
    for i in 0..7 {
        mark(&mut t, json!({ "item": dev, "sheet": s0, "pts": [{ "x": f64::from(i) * 10.0, "y": 5 }] }));
    }
    mark(&mut t, json!({ "item": dev, "sheet": s1, "pts": [{ "x": 1, "y": 1 }] }));
    mark(&mut t, json!({ "item": wall, "sheet": s1, "pts": [{ "x": 0, "y": 0 }, { "x": 100, "y": 0 }] }));
    (t, hash, s0, s1, wall, floor, sy, dev)
}

#[test]
fn quantities_waste_cost_undo_file() {
    let (mut t, hash, s0, s1, wall, floor, _sy, dev) = kitchen_style();
    let qq = q(&t);
    let qw = qq.get(&wall).unwrap();
    assert!(near(qw.tot.length_ft, 15.0, 1e-9));
    assert_eq!(qw.unscaled, 1.0);
    assert_eq!(qw.tot.count, 2.0);
    assert!(near(qq.get(&floor).unwrap().tot.area_sqft, 196.0, 1e-9));
    assert!(near(qq.get(&floor).unwrap().tot.perimeter_ft, 68.0, 1e-9));
    assert_eq!(qq.get(&dev).unwrap().tot.count, 8.0);
    assert_eq!(qq.get(&dev).unwrap().sheet(&s1).unwrap().count, 1.0);
    let vars = model::item_vars(t.item(&wall).unwrap(), &qw.tot);
    assert!(near(vars.iter().find(|(k, _)| k == "A").unwrap().1, 135.0, 1e-9));
    let est = model::estimate_all(&t.state);
    let wr = row(&est, "Partition type A");
    assert!(near(n(wr, "qty"), 15.0, 1e-9) && near(n(wr, "qtyW"), 16.5, 1e-9));
    assert_eq!(n(wr, "matExt"), 3465.0);
    assert_eq!(n(wr, "labExt"), 5499.0);
    assert_eq!(wr["wallArea"]["unit"], "SF");
    assert_eq!(n(row(&est, "Studs 16\" o.c."), "qty"), 14.0);
    assert_eq!(n(row(&est, "Studs 16\" o.c."), "matExt"), 5950.0);
    let gb = row(&est, "Gypsum board 4x8");
    assert_eq!(n(gb, "qty"), 9.0);
    assert!(near(n(gb, "qtyW"), 9.9, 1e-9));
    assert_eq!(n(gb, "matExt"), 14830.0);
    assert_eq!(n(gb, "labExt"), 21780.0);
    assert!(model::rstr(row(&est, "Broken"), "error").contains("divides by zero"));
    let fr = row(&est, "VCT flooring");
    assert!(near(n(fr, "qtyW"), 205.8, 1e-9));
    assert_eq!((n(fr, "matExt"), n(fr, "labExt")), (38073.0, 22638.0));
    assert_eq!(n(row(&est, "Base cove"), "matExt"), 6052.0);
    assert!(near(n(row(&est, "Carpet"), "qty"), 1.0, 1e-9));
    assert_eq!(n(row(&est, "Carpet"), "total"), 2700.0);
    assert_eq!(n(row(&est, "Duplex outlets"), "qty"), 8.0);
    assert_eq!(n(row(&est, "Duplex outlets"), "total"), 8.0 * 4500.0 + 8.0 * 350.0);
    let sum: f64 = est.rows.iter().filter(|r| r.get("error").is_none()).map(|r| n(r, "total")).sum();
    assert_eq!(est.total, sum);
    assert_eq!(est.total, 3465.0 + 5499.0 + 5950.0 + 14830.0 + 21780.0 + 38073.0 + 22638.0 + 6052.0 + 2700.0 + 38800.0);
    assert!(est.incomplete && est.errors == 1.0 && est.unscaled == 1.0);
    // metric
    let mut t2 = Takeoff::default();
    t2.add_file(&o(json!({ "hash": hash, "name": "m.pdf", "size": 1, "pages": 1 })));
    t2.set_scale(&s0, &preset("m100", "1:100"), None).unwrap();
    let mw = item(&mut t2, json!({ "name": "Wall", "type": "linear", "unit": "m", "heightFt": u::parse_length("3 m", "ft").unwrap() }));
    mark(&mut t2, json!({ "item": mw, "sheet": s0, "pts": [{ "x": 0, "y": 0 }, { "x": 72.0 / 25.4 * 50.0, "y": 0 }] }));
    t2.add_component(&mw, &o(json!({ "name": "Board m²", "unit": "m²", "formula": "A" }))).unwrap();
    let ma = item(&mut t2, json!({ "name": "Slab", "type": "area", "unit": "m²", "heightFt": u::parse_length("150 mm", "ft").unwrap() }));
    let k = 72.0 / 25.4;
    mark(
        &mut t2,
        json!({ "item": ma, "sheet": s0, "pts": [{ "x": 0, "y": 0 }, { "x": k * 40.0, "y": 0 }, { "x": k * 40.0, "y": k * 25.0 }, { "x": 0, "y": k * 25.0 }] }),
    );
    t2.add_component(&ma, &o(json!({ "name": "Concrete m³", "unit": "m³", "formula": "A * H" }))).unwrap();
    t2.add_component(&ma, &o(json!({ "name": "Edge form m", "unit": "m", "formula": "P" }))).unwrap();
    let e2 = model::estimate_all(&t2.state);
    let qs: Vec<f64> = e2.rows.iter().map(|r| n(r, "qty")).collect();
    for (a, b) in qs.iter().zip([5.0, 15.0, 10.0, 1.5, 13.0]) {
        assert!(near(*a, b, 1e-9), "{qs:?}");
    }
    // undo / redo
    let before = jv::stringify(&t.state);
    let nmarks = t.marks().len();
    let first = jv::to_string(t.marks()[0].get("id"));
    t.remove_marks(&[first]).unwrap();
    assert_eq!(t.marks().len(), nmarks - 1);
    t.undo();
    assert_eq!(jv::stringify(&t.state), before);
    t.redo();
    assert_eq!(t.marks().len(), nmarks - 1);
    t.undo();
    t.update_item(&wall, &o(json!({ "heightFt": 10 })), None).unwrap();
    let qw = q(&t);
    assert!(near(model::item_vars(t.item(&wall).unwrap(), &qw.get(&wall).unwrap().tot)[2].1, 150.0, 1e-9));
    t.undo();
    t.set_scale(&s0, &preset("a1/8", "1/8\""), None).unwrap();
    assert!(near(q(&t).get(&wall).unwrap().tot.length_ft, 30.0, 1e-9));
    t.undo();
    assert!(near(q(&t).get(&wall).unwrap().tot.length_ft, 15.0, 1e-9));
    assert!(t.update_item(&wall, &o(json!({ "type": "area" })), None).is_err() && t.item(&wall).unwrap()["type"] == "linear");
    assert!(t.dirty());
    // the project file
    let text = jv::stringify(&t.to_json("T"));
    let mut t3 = Takeoff::default();
    let w = t3.load(&serde_json::from_str(&text).unwrap()).unwrap();
    assert!(w.is_empty() && !t3.dirty());
    assert_eq!(jv::stringify(&model::estimate_all(&t3.state).to_json()), jv::stringify(&model::estimate_all(&t.state).to_json()));
    let id2 = t3.add_item(&o(json!({ "name": "After reopen" }))).unwrap().unwrap();
    assert!(t.item(&id2).is_none() && t3.mark(&id2).is_none());
    assert!(Takeoff::default().load(&json!({ "format": "x" })).is_err());
    assert!(Takeoff::default().load(&json!({ "format": model::FORMAT, "version": 99 })).is_err());
    let mut hostile: Value = serde_json::from_str(&text).unwrap();
    hostile["items"][0]["name"] = json!("<img src=x onerror=alert(1)>\u{0}");
    hostile["items"][0]["color"] = json!("red;background:url(x)");
    hostile["items"][0]["waste"] = json!(-5);
    hostile["items"][1]["type"] = json!("evil");
    hostile["marks"][0]["pts"] = json!([[1e308, 0], [0, 0]]);
    hostile["marks"][1]["kind"] = json!("area");
    hostile["marks"].as_array_mut().unwrap().push(json!({ "id": "m999", "item": "nope", "sheet": s0, "kind": "count", "pts": [[0, 0]] }));
    hostile["sheets"]["zz:0"] = json!({ "scale": { "method": "preset", "ftPerUnit": 1 } });
    hostile["sheets"][&s0]["scale"]["ftPerUnit"] = json!(-1);
    let mut t4 = Takeoff::default();
    let hw = t4.load(&hostile).unwrap();
    assert_eq!(hw.len(), 1, "{hw:?}");
    assert_eq!(t4.items()[0]["color"], model::PALETTE[0]);
    assert_eq!(t4.items()[0]["waste"].as_f64(), Some(0.0));
    assert!(!jv::to_string(t4.items()[0].get("name")).contains('\u{0}'));
    assert!(t4.state["sheets"][&s0]["scale"].is_null());
    assert!(t4.state["sheets"].get("zz:0").is_none());
    assert!(t4.mark("m999").is_none());
}

#[test]
fn arcs_in_file_and_batch_marks() {
    let hash = "a".repeat(64);
    let s0 = model::sheet_key(&hash, 0.0);
    let mut ta = Takeoff::default();
    ta.add_file(&o(json!({ "hash": hash, "name": "a.pdf", "size": 1, "pages": 1 })));
    ta.set_scale(&s0, &json!({ "method": "preset", "preset": "a1/4", "ftPerUnit": 1.0 / 18.0, "label": "1/4\"", "cal": null }), None)
        .unwrap();
    let room = item(&mut ta, json!({ "name": "Round room", "type": "area" }));
    let run = item(&mut ta, json!({ "name": "Curved wall", "type": "linear", "heightFt": 10 }));
    mark(
        &mut ta,
        json!({ "item": room, "sheet": s0, "pts": [{ "x": 136, "y": 100, "b": 1 }, { "x": 64, "y": 100, "b": 1 }], "holes": [[{ "x": 104.5, "y": 100, "b": 1 }, { "x": 95.5, "y": 100, "b": 1 }]] }),
    );
    mark(&mut ta, json!({ "item": run, "sheet": s0, "pts": [{ "x": 0, "y": 0 }, { "x": 18, "y": 0, "b": 1 }, { "x": 18, "y": 36 }] }));
    let qa = q(&ta);
    assert!(near(qa.get(&room).unwrap().tot.area_sqft, PI * 4.0 - PI * 0.0625, 1e-9));
    assert!(near(qa.get(&room).unwrap().tot.perimeter_ft, 2.0 * PI * 2.0 + 2.0 * PI * 0.25, 1e-9));
    assert!(near(qa.get(&run).unwrap().tot.length_ft, 1.0 + PI, 1e-9));
    let mut back = Takeoff::default();
    back.load(&ta.to_json("T")).unwrap();
    assert_eq!(jv::stringify(&back.state["marks"]), jv::stringify(&ta.state["marks"]));
    let pj = ta.to_json("T");
    assert_eq!(jv::stringify(&pj["marks"][0]["pts"][0]), "[136,100,1]");
    assert_eq!(jv::stringify(&pj["marks"][1]["pts"][0]), "[0,0]");
    let mut bad = pj.clone();
    bad["marks"].as_array_mut().unwrap().push(json!({ "id": "m900", "item": room, "sheet": s0, "kind": "area", "pts": [[0, 0], [1, 0]] }));
    bad["marks"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "id": "m901", "item": room, "sheet": s0, "kind": "area", "pts": [[0, 0, 5000], [1, 0, 1]] }));
    let br = model::read_project(&bad).unwrap();
    assert!(br.warnings.len() == 1 && br.state["marks"].as_array().unwrap().len() == 2);
    let dev = item(&mut ta, json!({ "name": "Sprinkler heads", "type": "count" }));
    let n0 = ta.marks().len();
    let list: Vec<Value> = (1..=5).map(|i| json!({ "sheet": s0, "pts": [{ "x": i * 10, "y": 5 }] })).collect();
    let ids = ta.add_marks(&dev, &list, Some("Auto count: 5"), None).unwrap();
    assert!(ids.len() == 5 && ta.marks().len() == n0 + 5);
    assert_eq!(ta.undo().as_deref(), Some("Auto count: 5"));
    assert_eq!(ta.marks().len(), n0);
}

#[test]
fn deducts_areas_backouts_slopes() {
    // linear deducts
    let hd = "d".repeat(64);
    let sk = model::sheet_key(&hd, 0.0);
    let mut td = Takeoff::default();
    td.add_file(&o(json!({ "hash": hd, "name": "D.pdf", "size": 1, "pages": 1 })));
    td.set_scale(&sk, &json!({ "method": "preset", "ftPerUnit": 1.0 / 18.0, "preset": "a1/4", "label": "1/4\"", "cal": null }), None)
        .unwrap();
    let wall = item(&mut td, json!({ "name": "Wall", "type": "linear", "heightFt": 10, "mat": rate("2") }));
    let pts = json!([{ "x": 0, "y": 0 }, { "x": 180, "y": 0 }, { "x": 234, "y": 0 }, { "x": 414, "y": 0 }, { "x": 414, "y": 90 }]);
    let id = mark(&mut td, json!({ "item": wall, "sheet": sk, "pts": pts, "deducts": [1] }));
    let r = model::measure_mark(td.mark(&id).unwrap(), 1.0 / 18.0, None).unwrap().to_json();
    assert!(
        near(r["lengthFt"].as_f64().unwrap(), 25.0, 1e-9)
            && near(r["deductFt"].as_f64().unwrap(), 3.0, 1e-9)
            && near(r["grossFt"].as_f64().unwrap(), 28.0, 1e-9)
    );
    assert_eq!(jv::stringify(&r["deducts"]), "[1]");
    let est = model::estimate_all(&td.state);
    let wr = est.rows.iter().find(|x| model::rstr(x, "itemId") == wall && model::rstr(x, "kind") == "item").unwrap();
    assert!(near(wr["wallArea"]["value"].as_f64().unwrap(), 250.0, 1e-9) && n(wr, "matExt") == 5000.0);
    td.update_mark(&id, &o(json!({ "deducts": [1, 3] })), None).unwrap();
    assert!(near(model::measure_mark(td.mark(&id).unwrap(), 1.0 / 18.0, None).unwrap().length_ft(), 20.0, 1e-9));
    td.undo();
    td.update_mark(&id, &o(json!({ "deducts": [0, 1, 2, 3] })), None).unwrap();
    assert!(td.mark(&id).unwrap().get("deducts").is_none());
    td.undo();
    td.update_mark(&id, &o(json!({ "deducts": [7, -1, 1.5, "x", 1, 1] })), None).unwrap();
    assert_eq!(jv::stringify(&td.mark(&id).unwrap()["deducts"]), "[1]");
    let mut longer = pts.as_array().unwrap().clone();
    longer.push(json!({ "x": 500, "y": 90 }));
    td.update_mark(&id, &o(json!({ "pts": longer })), None).unwrap();
    assert!(td.mark(&id).unwrap().get("deducts").is_none());
    td.undo();
    td.undo();
    let count = item(&mut td, json!({ "name": "Doors", "type": "count" }));
    let cm = mark(&mut td, json!({ "item": count, "sheet": sk, "pts": [{ "x": 1, "y": 1 }], "deducts": [0] }));
    assert!(td.mark(&cm).unwrap().get("deducts").is_none());
    let clip = clipboard::copy_marks(
        &td.state,
        std::slice::from_ref(&id),
        &CopyCtx { view: Some(json!([0, 0, 612, 792])), user_unit: Some(json!(1)), source: String::new(), now: 0.0 },
    )
    .unwrap();
    assert_eq!(jv::stringify(&clip["marks"][0]["deducts"]), "[1]");
    let rc = clipboard::read_clip(&clip).unwrap();
    let pasted = clipboard::paste_clip(&mut td, &rc.clip, &PasteOpts { sheet: Some(sk.clone()), dy: 200.0, ..PasteOpts::default() })
        .unwrap()
        .unwrap();
    assert_eq!(jv::stringify(&td.mark(&pasted.marks[0]).unwrap()["deducts"]), "[1]");
    td.undo();
    // bid areas
    let hb = "b".repeat(64);
    let (b1, b2) = (model::sheet_key(&hb, 0.0), model::sheet_key(&hb, 1.0));
    let mut tb = Takeoff::default();
    tb.add_file(&o(json!({ "hash": hb, "name": "B.pdf", "size": 1, "pages": 2 })));
    for s in [&b1, &b2] {
        tb.set_scale(s, &json!({ "method": "preset", "ftPerUnit": 1.0 / 18.0, "preset": "a1/4", "label": "1/4\"", "cal": null }), None)
            .unwrap();
    }
    let a1 = tb.add_area(Some(&json!("Building A"))).unwrap().unwrap();
    let a2 = tb.add_area(Some(&json!("  Building B  "))).unwrap().unwrap();
    assert_eq!(tb.state["areas"][1]["name"], "Building B");
    assert!(tb.add_area(Some(&json!("   "))).unwrap().is_none());
    let w = item(&mut tb, json!({ "name": "Wall", "type": "linear", "mat": rate("10") }));
    let studs = tb.add_component(&w, &o(json!({ "name": "Studs", "unit": "EA", "formula": "ceil(L / 1.333)" }))).unwrap().unwrap();
    let w1 = mark(&mut tb, json!({ "item": w, "sheet": b1, "pts": [{ "x": 0, "y": 0 }, { "x": 180, "y": 0 }], "area": a1 }));
    mark(&mut tb, json!({ "item": w, "sheet": b2, "pts": [{ "x": 0, "y": 0 }, { "x": 90, "y": 0 }], "area": a2 }));
    let w3 = mark(&mut tb, json!({ "item": w, "sheet": b2, "pts": [{ "x": 0, "y": 0 }, { "x": 36, "y": 0 }] }));
    let w4 = mark(&mut tb, json!({ "item": w, "sheet": b1, "pts": [{ "x": 0, "y": 0 }, { "x": 18, "y": 0 }], "area": "nope" }));
    assert!(tb.mark(&w4).unwrap().get("area").is_none());
    let qa = |t: &Takeoff, a: Option<&str>| {
        model::quantities(&t.state, &QOpts { area: Some(a.map(str::to_string)), exclude: None }).get(&w).unwrap().tot.length_ft
    };
    assert!(near(qa(&tb, Some(&a1)), 10.0, 1e-9) && near(qa(&tb, Some(&a2)), 5.0, 1e-9) && near(qa(&tb, None), 3.0, 1e-9));
    let ae = model::area_estimates(&tb.state);
    assert_eq!(ae.len(), 3);
    let sq = |e: &model::Estimate| e.rows.iter().find(|r| model::rstr(r, "compId") == studs).map(|r| n(r, "qty"));
    assert_eq!((sq(&ae[0].1), sq(&ae[1].1)), (Some(8.0), Some(4.0)));
    assert!(ae[0].1.total == 10000.0 && ae[1].1.total == 5000.0);
    assert!(tb.set_marks_area(&[w3.clone(), w4.clone()], Some(&a2), None).unwrap());
    assert!(near(qa(&tb, Some(&a2)), 8.0, 1e-9));
    tb.undo();
    assert!(tb.rename_area(&a1, Some(&json!("Tower A"))).unwrap());
    assert!(tb.move_area(&a2, -1.0).unwrap() && tb.state["areas"][0]["id"] == a2.as_str());
    tb.undo();
    tb.undo();
    let pj = tb.to_json("T");
    assert!(pj["marks"].as_array().unwrap().iter().any(|m| m["id"] == w1.as_str() && m["area"] == a1.as_str()));
    let mut evil = pj.clone();
    evil["areas"].as_array_mut().unwrap().extend([
        json!({ "id": "b1", "name": "Dup" }),
        json!({ "id": "<script>", "name": "x" }),
        json!({ "id": "zz", "name": "" }),
    ]);
    evil["marks"][0]["area"] = json!("zz");
    let ev = model::read_project(&evil).unwrap();
    assert!(ev.state["areas"].as_array().unwrap().len() == 2 && ev.state["marks"][0].get("area").is_none());
    // back-outs
    let sq2 = |x: f64, y: f64, w: f64| {
        vec![
            cc_takeoff::geom::P::new(x, y),
            cc_takeoff::geom::P::new(x + w, y),
            cc_takeoff::geom::P::new(x + w, y + w),
            cc_takeoff::geom::P::new(x, y + w),
        ]
    };
    use cc_takeoff::geom;
    assert_eq!(geom::overlap_area(&sq2(0.0, 0.0, 100.0), &[], &sq2(10.0, 10.0, 20.0)), 400.0);
    assert_eq!(geom::overlap_area(&sq2(0.0, 0.0, 100.0), &[], &sq2(90.0, 90.0, 20.0)), 100.0);
    assert_eq!(geom::overlap_area(&sq2(0.0, 0.0, 100.0), &[sq2(80.0, 80.0, 15.0)], &sq2(90.0, 90.0, 20.0)), 75.0);
    assert_eq!(geom::overlap_area(&sq2(0.0, 0.0, 100.0), &[], &sq2(200.0, 200.0, 5.0)), 0.0);
    let col = [geom::P::arc(60.0, 50.0, 1.0), geom::P::arc(40.0, 50.0, 1.0)];
    assert_eq!(geom::overlap_area(&sq2(0.0, 0.0, 100.0), &[], &col), PI * 100.0);
    assert!(near(geom::overlap_area(&sq2(0.0, 0.0, 50.0), &[], &col), PI * 25.0, 2e-5));
    let hc = "c".repeat(64);
    let (c0, c1) = (model::sheet_key(&hc, 0.0), model::sheet_key(&hc, 1.0));
    let mut tk = Takeoff::default();
    tk.add_file(&o(json!({ "hash": hc, "name": "C.pdf", "size": 1, "pages": 2 })));
    for s in [&c0, &c1] {
        tk.set_scale(s, &json!({ "method": "preset", "ftPerUnit": 1.0 / 18.0, "preset": "a1/4", "label": "1/4\"", "cal": null }), None)
            .unwrap();
    }
    let fl = item(&mut tk, json!({ "name": "Floor tile", "type": "area", "mat": rate("5") }));
    let ce = item(&mut tk, json!({ "name": "Ceiling", "type": "area" }));
    let pa = item(&mut tk, json!({ "name": "Paint", "type": "area" }));
    let cols = item(&mut tk, json!({ "name": "Columns", "type": "area", "backout": true, "backoutOf": [fl, ce, "nope"] }));
    for (s, it) in [(&c0, &fl), (&c0, &ce), (&c0, &pa), (&c1, &fl)] {
        mark(&mut tk, json!({ "item": it, "sheet": s, "pts": rect(0.0, 0.0, 360.0, 180.0) }));
    }
    mark(&mut tk, json!({ "item": cols, "sheet": c0, "pts": rect(90.0, 90.0, 18.0, 18.0) }));
    mark(&mut tk, json!({ "item": cols, "sheet": c0, "pts": [{ "x": 279, "y": 90, "b": 1 }, { "x": 261, "y": 90, "b": 1 }] }));
    mark(&mut tk, json!({ "item": cols, "sheet": c0, "pts": rect(350.0, 170.0, 36.0, 36.0) }));
    let want = 200.0 - 1.0 - PI / 4.0 - (10.0f64 / 18.0).powi(2);
    let qk = q(&tk);
    assert!(near(qk.get(&fl).unwrap().tot.area_sqft, want + 200.0, 1e-6));
    assert!(near(qk.get(&ce).unwrap().tot.area_sqft, want, 1e-6));
    assert!(near(qk.get(&pa).unwrap().tot.area_sqft, 200.0, 1e-9));
    assert!(near(qk.get(&cols).unwrap().tot.area_sqft, 1.0 + PI / 4.0 + 4.0, 1e-9));
    assert!(near(qk.get(&fl).unwrap().backout_sqft, 200.0 - want, 1e-6));
    tk.update_item(&cols, &o(json!({ "backout": false })), None).unwrap();
    assert!(tk.item(&cols).unwrap().get("backoutOf").is_none() && near(q(&tk).get(&fl).unwrap().tot.area_sqft, 400.0, 1e-9));
    tk.undo();
    let back = model::read_project(&tk.to_json("T")).unwrap();
    let bo = back.state["items"].as_array().unwrap().iter().find(|i| i["id"] == cols.as_str()).unwrap();
    assert_eq!(jv::stringify(&bo["backoutOf"]), jv::stringify(&json!([fl, ce])));
    // slopes
    let hs = "e".repeat(64);
    let ss = model::sheet_key(&hs, 0.0);
    let mut ts = Takeoff::default();
    ts.add_file(&o(json!({ "hash": hs, "name": "Roof.pdf", "size": 1, "pages": 1 })));
    ts.set_scale(&ss, &json!({ "method": "preset", "ftPerUnit": 1.0 / 18.0, "preset": "a1/4", "label": "1/4\"", "cal": null }), None)
        .unwrap();
    let roof = item(&mut ts, json!({ "name": "Roofing", "type": "area", "pitch": 6 }));
    let plane = mark(&mut ts, json!({ "item": roof, "sheet": ss, "pts": rect(0.0, 0.0, 360.0, 180.0) }));
    assert!(near(q(&ts).get(&roof).unwrap().tot.area_sqft, 200.0 * 1.25f64.sqrt(), 1e-9));
    ts.update_mark(&plane, &o(json!({ "pitch": 12 })), None).unwrap();
    assert!(near(q(&ts).get(&roof).unwrap().tot.area_sqft, 200.0 * 2f64.sqrt(), 1e-9));
    ts.update_mark(&plane, &o(json!({ "pitch": null })), None).unwrap();
    assert!(ts.mark(&plane).unwrap().get("pitch").is_none());
    ts.update_item(&roof, &o(json!({ "pitch": 0 })), None).unwrap();
    assert!(ts.item(&roof).unwrap().get("pitch").is_none() && near(q(&ts).get(&roof).unwrap().tot.area_sqft, 200.0, 1e-9));
    ts.undo();
    let edge = item(&mut ts, json!({ "name": "Rake edge", "type": "linear", "slope": "rake", "pitch": 12 }));
    let run = mark(
        &mut ts,
        json!({ "item": edge, "sheet": ss, "pts": [{ "x": 0, "y": 0 }, { "x": 180, "y": 0 }, { "x": 180, "y": 180 }, { "x": 360, "y": 180 }, { "x": 360, "y": 360 }] }),
    );
    ts.update_mark(&run, &o(json!({ "slopes": [null, { "k": "hip" }, { "k": "flat" }, null], "deducts": [3] })), None).unwrap();
    let mr = model::measure_mark(ts.mark(&run).unwrap(), 1.0 / 18.0, ts.item(&edge)).unwrap().to_json();
    let wantl = 10.0 * 2f64.sqrt() + 10.0 * 1.5f64.sqrt() + 10.0;
    assert!(
        near(mr["lengthFt"].as_f64().unwrap(), wantl, 1e-9)
            && near(mr["planFt"].as_f64().unwrap(), 30.0, 1e-9)
            && near(mr["deductFt"].as_f64().unwrap(), 10.0 * 2f64.sqrt(), 1e-9)
    );
    let kinds: Vec<String> = mr["slopes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| format!("{}{}", x["k"].as_str().unwrap(), jv::num_str(x["p"].as_f64().unwrap())))
        .collect();
    assert_eq!(kinds.join(" "), "rake12 hip12 flat0 rake12");
}

#[test]
fn conditions_groups_alternates_crew_bid() {
    // multi-condition items
    let hash = "b".repeat(64);
    let s0 = model::sheet_key(&hash, 0.0);
    let mut t = Takeoff::default();
    t.add_file(&o(json!({ "hash": hash, "name": "room.pdf", "size": 1, "pages": 1 })));
    t.set_scale(&s0, &preset("a1/4", "1/4\""), None).unwrap();
    let room = item(&mut t, json!({ "name": "Room 101", "type": "area", "heightFt": 0.25, "mat": rate("2"), "pitch": 6 }));
    let err = t.add_mark(&o(json!({ "item": room, "sheet": s0, "pts": [{ "x": 0, "y": 0 }], "kind": "count" }))).unwrap_err();
    assert!(err.contains("Also measure"), "{err}");
    t.set_cond(&room, "linear", true).unwrap();
    t.set_cond(&room, "count", true).unwrap();
    assert_eq!(model::kinds_of(t.item(&room)).join(","), "area,linear,count");
    t.update_cond(&room, "linear", &o(json!({ "mat": rate("1.5"), "waste": 1000 })), None).unwrap();
    t.update_cond(&room, "count", &o(json!({ "lab": rate("80") })), None).unwrap();
    mark(&mut t, json!({ "item": room, "sheet": s0, "pts": rect(0.0, 0.0, 216.0, 144.0) }));
    mark(
        &mut t,
        json!({ "item": room, "sheet": s0, "pts": [{ "x": 0, "y": 0 }, { "x": 216, "y": 0 }, { "x": 216, "y": 144 }], "kind": "linear" }),
    );
    t.add_marks(
        &room,
        &[json!({ "sheet": s0, "pts": [{ "x": 10, "y": 10 }] }), json!({ "sheet": s0, "pts": [{ "x": 20, "y": 10 }] })],
        Some("doors"),
        Some("count"),
    )
    .unwrap();
    let qr = q(&t);
    let qq = qr.get(&room).unwrap();
    assert!(near(qq.tot.area_sqft, 96.0 * u::slope_factor(6.0), 1e-9) && qq.tot.count == 1.0);
    assert!(near(qq.tot.cond("linear").unwrap().length_ft, 20.0, 1e-9) && qq.tot.cond("count").unwrap().count == 2.0);
    let est = model::estimate_all(&t.state);
    let rows: Vec<&Value> = est.rows.iter().filter(|r| model::rstr(r, "itemId") == room).collect();
    assert!(near(n(rows[1], "qtyW"), 22.0, 1e-9) && n(rows[1], "matExt") == 3300.0);
    assert!(n(rows[2], "labExt") == 16000.0 && n(rows[2], "marks") == 2.0);
    assert_eq!(n(rows[0], "marks"), 1.0);
    let e = t.set_cond(&room, "count", false).unwrap_err();
    assert!(e.contains("still has marks of that kind"));
    // typical groups
    let hc = "c".repeat(64);
    let g0 = model::sheet_key(&hc, 0.0);
    let mut tg = Takeoff::default();
    tg.add_file(&o(json!({ "hash": hc, "name": "units.pdf", "size": 1, "pages": 1 })));
    tg.set_scale(&g0, &preset("a1/4", "1/4\""), None).unwrap();
    let wall = item(&mut tg, json!({ "name": "Wall", "type": "linear", "heightFt": 9, "mat": 10000 }));
    let floor = item(&mut tg, json!({ "name": "Carpet", "type": "area" }));
    let outlet = item(&mut tg, json!({ "name": "Outlets", "type": "count" }));
    let w1 = mark(&mut tg, json!({ "item": wall, "sheet": g0, "pts": [{ "x": 0, "y": 0 }, { "x": 180, "y": 0 }] }));
    let f1 = mark(&mut tg, json!({ "item": floor, "sheet": g0, "pts": rect(0.0, 0.0, 180.0, 144.0) }));
    let o1 = mark(&mut tg, json!({ "item": outlet, "sheet": g0, "pts": [{ "x": 5, "y": 5 }] }));
    let o2 = mark(&mut tg, json!({ "item": outlet, "sheet": g0, "pts": [{ "x": 15, "y": 5 }] }));
    mark(&mut tg, json!({ "item": wall, "sheet": g0, "pts": [{ "x": 0, "y": 300 }, { "x": 90, "y": 300 }] }));
    assert!(tg.add_group(Some(&json!("Unit A")), Some(&json!(0)), &[]).unwrap_err().contains("1 to 10000"));
    let g = tg.add_group(Some(&json!("Unit A")), Some(&json!(24)), &[w1.clone(), f1, o1, o2.clone()]).unwrap().unwrap();
    let qg = q(&tg);
    assert!(near(qg.get(&wall).unwrap().tot.length_ft, 245.0, 1e-9) && qg.get(&wall).unwrap().tot.count == 25.0);
    assert!(near(qg.get(&floor).unwrap().tot.area_sqft, 80.0 * 24.0, 1e-9));
    assert_eq!(qg.get(&outlet).unwrap().tot.count, 48.0);
    tg.update_group(&g, None, Some(&json!(12))).unwrap();
    tg.set_marks_group(&[o2], None, None).unwrap();
    assert_eq!(q(&tg).get(&outlet).unwrap().tot.count, 13.0);
    // alternates
    let he = "e".repeat(64);
    let e0 = model::sheet_key(&he, 0.0);
    let mut ta = Takeoff::default();
    ta.add_file(&o(json!({ "hash": he, "name": "alt.pdf", "size": 1, "pages": 1 })));
    ta.set_scale(&e0, &preset("a1/4", "1/4\""), None).unwrap();
    let vct = item(&mut ta, json!({ "name": "VCT", "type": "area", "mat": rate("2") }));
    let carpet = item(&mut ta, json!({ "name": "Carpet", "type": "area", "mat": rate("5") }));
    ta.add_component(&carpet, &o(json!({ "name": "Pad", "unit": "SF", "formula": "A", "mat": rate("1") }))).unwrap();
    let canopy = item(&mut ta, json!({ "name": "Canopy", "type": "count", "mat": rate("1000") }));
    ta.add_component(&canopy, &o(json!({ "name": "Permit", "unit": "EA", "formula": "1", "mat": rate("50") }))).unwrap();
    let base = ta.add_area(Some(&json!("Base"))).unwrap().unwrap();
    let a1 = ta.add_area(Some(&json!("Alt 1 carpet upgrade"))).unwrap().unwrap();
    let a2 = ta.add_area(Some(&json!("Alt 2 delete canopy"))).unwrap().unwrap();
    mark(&mut ta, json!({ "item": vct, "sheet": e0, "pts": rect(0.0, 0.0, 180.0, 180.0), "area": base }));
    mark(&mut ta, json!({ "item": carpet, "sheet": e0, "pts": rect(0.0, 200.0, 180.0, 90.0), "area": a1 }));
    mark(&mut ta, json!({ "item": canopy, "sheet": e0, "pts": [{ "x": 300, "y": 300 }], "area": a2 }));
    let plain = model::estimate_all(&ta.state).total;
    assert!(
        ta.set_area_alt(&a1, Some("add")).unwrap()
            && ta.set_area_alt(&a2, Some("deduct")).unwrap()
            && !ta.set_area_alt(&a2, Some("deduct")).unwrap()
    );
    let b = model::base_estimate(&ta.state);
    assert_eq!(b.total, plain - 30000.0);
    assert_eq!(b.total, 125000.0);
    let alts = model::alternates(&ta.state);
    let s: Vec<String> = alts.iter().map(|a| format!("{} {} {}", a.area["name"].as_str().unwrap(), a.alt, jv::num_str(a.amount))).collect();
    assert_eq!(s.join(" | "), "Alt 1 carpet upgrade add 30000 | Alt 2 delete canopy deduct -105000");
    // crew labor
    let hf = "f".repeat(64);
    let f0 = model::sheet_key(&hf, 0.0);
    let mut tc = Takeoff::default();
    tc.add_file(&o(json!({ "hash": hf, "name": "crew.pdf", "size": 1, "pages": 1 })));
    tc.set_scale(&f0, &preset("a1/4", "1/4\""), None).unwrap();
    let pw = item(
        &mut tc,
        json!({ "name": "Partition", "type": "linear", "waste": 1000, "lab": rate("9"), "crew": { "prod": 20, "size": 2, "wage": rate("45") } }),
    );
    mark(&mut tc, json!({ "item": pw, "sheet": f0, "pts": [{ "x": 0, "y": 0 }, { "x": 1800, "y": 0 }] }));
    let est = model::estimate_all(&tc.state);
    let r = est.rows.iter().find(|x| model::rstr(x, "itemId") == pw).unwrap();
    assert!(near(r["work"]["hours"].as_f64().unwrap(), 5.5, 1e-9) && near(r["work"]["workerHours"].as_f64().unwrap(), 11.0, 1e-9));
    assert_eq!((n(r, "labExt"), n(r, "lab")), (49500.0, 45000.0));
    assert!(near(est.worker_hours, 11.0, 1e-9));
    tc.update_item(&pw, &o(json!({ "crew": null })), None).unwrap();
    assert_eq!(n(model::estimate_all(&tc.state).rows.iter().find(|x| model::rstr(x, "itemId") == pw).unwrap(), "labExt"), 99000.0);
    // bid summary
    let h9 = "9".repeat(64);
    let n0 = model::sheet_key(&h9, 0.0);
    let mut tb = Takeoff::default();
    tb.add_file(&o(json!({ "hash": h9, "name": "bid.pdf", "size": 1, "pages": 1 })));
    tb.set_scale(&n0, &preset("a1/4", "1/4\""), None).unwrap();
    let bw = item(&mut tb, json!({ "name": "Partition", "type": "linear", "mat": rate("10"), "lab": rate("5") }));
    mark(&mut tb, json!({ "item": bw, "sheet": n0, "pts": [{ "x": 0, "y": 0 }, { "x": 1800, "y": 0 }] }));
    let alt = tb.add_area(Some(&json!("Alt 1 more wall"))).unwrap().unwrap();
    tb.set_area_alt(&alt, Some("add")).unwrap();
    mark(&mut tb, json!({ "item": bw, "sheet": n0, "pts": [{ "x": 0, "y": 100 }, { "x": 180, "y": 100 }], "area": alt }));
    tb.set_bid(&o(json!({ "tax": 825, "overhead": 1000, "profit": 1000, "bond": 150, "to": "Owner LLC\n1 Main St", "by": "ACME Drywall", "incl": "- Taping and finishing\n- Cleanup", "excl": "Paint\nPermits", "terms": "Valid for 30 days." })), None).unwrap();
    let est = model::base_estimate(&tb.state);
    let ins: Vec<bid::AltIn> = model::alternates(&tb.state).iter().map(model::Alternate::bid_in).collect();
    let sum = bid::bid_summary(est.mat_total, est.lab_total, est.incomplete, Some(&tb.bid()), &ins);
    let s: Vec<String> = sum.lines.iter().map(|l| format!("{}:{}", l.key, jv::num_str(l.cents))).collect();
    assert_eq!(s.join(" "), "mat:100000 lab:50000 cost:150000 tax:8250 overhead:15825 profit:17408 bond:2872 total:194355");
    assert_eq!(sum.alts[0].with_markup, bid::bid_summary(10000.0, 5000.0, false, Some(&tb.bid()), &[]).total);
    let nb = bid::normal_bid(Some(&json!({ "tax": -1, "overhead": 1.5, "profit": 200000, "to": "a\u{0}b" })));
    assert!(nb["tax"] == 0.0 && nb["overhead"] == 0.0 && nb["to"] == "a b");
    assert!(Takeoff::default().to_json("T").get("bid").is_none());
}

#[test]
fn earthwork_markups_overlay_model() {
    let hash = "7".repeat(64);
    let s0 = model::sheet_key(&hash, 0.0);
    let mut t = Takeoff::default();
    t.add_file(&o(json!({ "hash": hash, "name": "site.pdf", "size": 1, "pages": 1 })));
    let spot = item(&mut t, json!({ "name": "Existing grade", "type": "count" }));
    let wall = item(&mut t, json!({ "name": "Wall", "type": "linear" }));
    t.set_scale(&s0, &json!({ "method": "preset", "preset": "e20", "ftPerUnit": 20.0 / 72.0, "label": "1\" = 20'", "cal": null }), None)
        .unwrap();
    for (x, y, z) in [(72.0, 144.0, Some(101.25)), (360.0, 144.0, Some(-2.0)), (216.0, 432.0, Some(0.0)), (500.0, 500.0, None)] {
        let mut m = json!({ "item": spot, "sheet": s0, "pts": [{ "x": x, "y": y }] });
        if let Some(z) = z {
            m["z"] = json!(z);
        }
        mark(&mut t, m);
    }
    mark(&mut t, json!({ "item": wall, "sheet": s0, "pts": [{ "x": 0, "y": 0 }, { "x": 72, "y": 0 }], "z": 5 }));
    let zs: Vec<String> = t.marks().iter().filter_map(|m| m.get("z").and_then(Value::as_f64)).map(jv::num_str).collect();
    assert_eq!(zs.join(","), "101.25,-2,0");
    // markups
    let hn = "cd".repeat(32);
    let key = model::sheet_key(&hn, 0.0);
    let mut tn = Takeoff::default();
    tn.add_file(&o(json!({ "hash": hn, "name": "plan.pdf", "size": 1, "pages": 1 })));
    let n1 = tn
        .add_note(&o(
            json!({ "kind": "note", "sheet": key, "pts": [{ "x": 100, "y": 700 }], "text": "Verify the door swing\nwith the owner" }),
        ))
        .unwrap();
    let c1 = tn
        .add_note(&o(
            json!({ "kind": "cloud", "sheet": key, "pts": [{ "x": 300, "y": 300 }, { "x": 200, "y": 400 }], "text": "Rev 2: wall moved" }),
        ))
        .unwrap()
        .unwrap();
    let h1 = tn
        .add_note(&o(json!({ "kind": "highlight", "sheet": key, "pts": [{ "x": 50, "y": 50 }, { "x": 150, "y": 80 }], "text": "" })))
        .unwrap();
    assert!(n1.is_some() && h1.is_some() && tn.state["notes"].as_array().unwrap().len() == 3);
    assert!(tn.add_note(&o(json!({ "kind": "cloud", "sheet": key, "pts": [{ "x": 1, "y": 1 }, { "x": 1, "y": 9 }] }))).unwrap().is_none());
    tn.update_note(&c1, &o(json!({ "status": "answered", "kind": "note", "sheet": "other" })), None).unwrap();
    let nc = tn.note(&c1).unwrap();
    assert!(nc["status"] == "answered" && nc["kind"] == "cloud" && nc["sheet"] == key.as_str());
    assert!(model::estimate_all(&tn.state).rows.is_empty());
    // overlay: carry marks
    let (ho, hnw) = ("1".repeat(64), "2".repeat(64));
    let mut tr = Takeoff::default();
    tr.add_file(&o(json!({ "hash": ho, "name": "Old.pdf", "size": 1, "pages": 1 })));
    tr.add_file(&o(json!({ "hash": hnw, "name": "New.pdf", "size": 1, "pages": 1 })));
    let (so, sn) = (model::sheet_key(&ho, 0.0), model::sheet_key(&hnw, 0.0));
    tr.set_scale(&so, &json!({ "method": "preset", "preset": "a1/4", "ftPerUnit": 1.0 / 18.0, "label": "1/4\"", "cal": null }), None)
        .unwrap();
    let it = item(&mut tr, json!({ "name": "Wall", "type": "linear" }));
    let fl = item(&mut tr, json!({ "name": "Floor", "type": "area" }));
    let m1 = mark(
        &mut tr,
        json!({ "item": it, "sheet": so, "pts": [{ "x": 0, "y": 0 }, { "x": 180, "y": 0, "b": 0.5 }, { "x": 360, "y": 0 }] }),
    );
    let m2 = mark(
        &mut tr,
        json!({ "item": fl, "sheet": so, "pts": [{ "x": 0, "y": 0 }, { "x": 90, "y": 0 }, { "x": 90, "y": 90 }], "holes": [[{ "x": 10, "y": 10 }, { "x": 20, "y": 10 }, { "x": 20, "y": 20 }]] }),
    );
    let before: Vec<f64> = model::estimate_all(&tr.state).rows.iter().map(|r| n(r, "qty")).collect();
    let a = [1.0, 0.0, 0.0, 1.0, 20.0, 10.0];
    let mv = |pts: &[Value]| -> Vec<Value> {
        let ps = model::pts_of(Some(&Value::Array(pts.to_vec())));
        overlay::move_pts(&ps, &a)
            .iter()
            .map(|p| if p.has_b() { json!({ "x": p.x, "y": p.y, "b": p.b }) } else { json!({ "x": p.x, "y": p.y }) })
            .collect()
    };
    let flagged: HashSet<String> = [m2.clone()].into_iter().collect();
    let sc = tr.scale_of(&so).cloned();
    assert_eq!(tr.carry_marks(&[m1.clone(), m2.clone()], &sn, &mv, &flagged, sc.as_ref(), None).unwrap(), 2.0);
    assert!(
        tr.mark(&m1).unwrap()["sheet"] == sn.as_str()
            && tr.mark(&m1).unwrap()["pts"][0]["x"] == 20.0
            && tr.mark(&m1).unwrap()["pts"][1]["b"] == 0.5
    );
    assert!(tr.mark(&m2).unwrap()["flag"] == "rev" && tr.mark(&m1).unwrap().get("flag").is_none());
    assert_eq!(tr.scale_of(&sn).unwrap()["ftPerUnit"].as_f64(), Some(1.0 / 18.0));
    let after: Vec<f64> = model::estimate_all(&tr.state).rows.iter().map(|r| n(r, "qty")).collect();
    assert_eq!(before, after);
    assert!(tr.clear_flags(std::slice::from_ref(&m2)).unwrap() && tr.mark(&m2).unwrap().get("flag").is_none());
    tr.undo();
    tr.undo();
    assert!(tr.mark(&m1).unwrap()["sheet"] == so.as_str() && tr.scale_of(&sn).is_none());
}

#[test]
fn clipboard_copy_paste() {
    let hash = "a".repeat(64);
    let (h2, h3) = ("b".repeat(64), "c".repeat(64));
    let mut tc = Takeoff::default();
    tc.add_file(&o(json!({ "hash": hash, "name": "a.pdf", "size": 1, "pages": 2 })));
    tc.add_file(&o(json!({ "hash": h2, "name": "b.pdf", "size": 1, "pages": 1 })));
    tc.add_file(&o(json!({ "hash": h3, "name": "c.pdf", "size": 1, "pages": 1 })));
    let (sa, sb, sc) = (model::sheet_key(&hash, 0.0), model::sheet_key(&hash, 1.0), model::sheet_key(&h2, 0.0));
    let q4 = preset("a1/4", "1/4\" = 1'-0\"");
    tc.set_scale(&sa, &q4, None).unwrap();
    tc.set_scale(&sb, &q4, None).unwrap();
    tc.set_scale(&sc, &preset("a1/8", "1/8\" = 1'-0\""), None).unwrap();
    let ctx = CopyCtx { view: Some(json!([0, 0, 2592, 1728])), user_unit: Some(json!(1)), source: "t1".into(), now: 1000.0 };
    let wall = item(&mut tc, json!({ "name": "Wall C", "type": "linear", "heightFt": 9, "waste": 500, "mat": 21000 }));
    tc.add_component(&wall, &o(json!({ "name": "Studs", "unit": "EA", "formula": "ceil(L / 1.333) + N", "mat": 42500 }))).unwrap();
    let roomc = item(&mut tc, json!({ "name": "Room C", "type": "area" }));
    let devc = item(&mut tc, json!({ "name": "Outlets", "type": "count" }));
    let run = mark(
        &mut tc,
        json!({ "item": wall, "sheet": sa, "pts": [{ "x": 0, "y": 0 }, { "x": 72, "y": 0 }, { "x": 72, "y": 36, "b": 0.5 }, { "x": 0, "y": 36 }] }),
    );
    let room = mark(
        &mut tc,
        json!({ "item": roomc, "sheet": sa, "pts": rect(100.0, 100.0, 72.0, 72.0), "holes": [rect(120.0, 120.0, 10.0, 10.0)] }),
    );
    let dots = tc
        .add_marks(
            &devc,
            &[json!({ "sheet": sa, "pts": [{ "x": 10, "y": 200 }] }), json!({ "sheet": sa, "pts": [{ "x": 20, "y": 200 }] })],
            None,
            None,
        )
        .unwrap();
    let len0 = q(&tc).get(&wall).unwrap().tot.length_ft;
    let mut ids = vec![run.clone(), room.clone()];
    ids.extend(dots.iter().cloned());
    let wire = clipboard::copy_marks(&tc.state, &ids, &ctx).unwrap();
    assert!(
        wire["format"] == "takeoffcraft-clip"
            && wire["marks"].as_array().unwrap().len() == 4
            && wire["items"].as_array().unwrap().len() == 3
    );
    assert_eq!(jv::stringify(&wire["marks"][0]["pts"][2]), "[72,36,0.5]");
    assert_eq!(jv::stringify(&wire["anchor"]), "{\"x\":86,\"y\":100}");
    let rc = clipboard::read_clip(&wire).unwrap();
    let clip = rc.clip;
    let nm = tc.marks().len();
    let out =
        clipboard::paste_clip(&mut tc, &clip, &PasteOpts { sheet: Some(sa.clone()), dx: 300.0, ..PasteOpts::default() }).unwrap().unwrap();
    assert!(out.marks.len() == 4 && out.items.is_empty() && out.reused.len() == 3);
    assert_eq!(tc.marks().len(), nm + 4);
    assert!(near(q(&tc).get(&wall).unwrap().tot.length_ft, 2.0 * len0, 1e-9));
    assert_eq!(tc.undo().as_deref(), Some("Paste"));
    assert!(clipboard::size_problem(&clip, Some(&json!({ "view": [0, 0, 2592, 1728], "userUnit": 1 }))).is_none());
    assert!(
        clipboard::scale_problem(&clip, Some(&json!({ "ftPerUnit": u::preset_ft_per_unit("a1/4", 1.0).unwrap(), "label": "1/4\"" })))
            .is_none()
    );
    let why8 = clipboard::scale_problem(
        &clip,
        Some(&json!({ "ftPerUnit": u::preset_ft_per_unit("a1/8", 1.0).unwrap(), "label": "1/8\" = 1'-0\"" })),
    )
    .unwrap();
    assert!(why8.contains("lengths and areas"));
    assert!(clipboard::scale_problem(&clip, None).unwrap().contains("no scale"));
    let sp = clipboard::size_problem(&clip, Some(&json!({ "view": [0, 0, 612, 792], "userUnit": 1 }))).unwrap();
    assert!(sp.contains("same size") && sp.contains("36\" × 24\""), "{sp}");
    assert!(clipboard::size_problem(&clip, Some(&json!({ "view": [0, 0, 1296, 864], "userUnit": 2 }))).is_none());
    // into another takeoff
    let mut t2 = Takeoff::default();
    t2.add_file(&o(json!({ "hash": hash, "name": "a.pdf", "size": 1, "pages": 2 })));
    t2.set_scale(&sa, &q4, None).unwrap();
    item(&mut t2, json!({ "name": "Room C", "type": "linear" }));
    let o2 = clipboard::paste_clip(&mut t2, &clip, &PasteOpts { sheet: Some(sa.clone()), ..PasteOpts::default() }).unwrap().unwrap();
    assert!(o2.items.len() == 3 && o2.reused.is_empty());
    assert!(t2.items().iter().any(|i| i["name"] == "Room C (copy)" && i["type"] == "area"));
    assert!(near(model::estimate_all(&t2.state).total, model::estimate_all(&tc.state).total, 1e-9));
    assert_eq!(clipboard::free_name("A", &[json!({ "name": "A" }), json!({ "name": "A (copy)" })]), "A (copy 2)");
    assert_eq!(clipboard::free_name("A (copy)", &[json!({ "name": "A" }), json!({ "name": "A (copy)" })]), "A (copy 2)");
    // a tampered clipboard
    let mut bad = wire.clone();
    bad["items"][0]["name"] = json!("<script>alert(1)</script>\u{7}");
    bad["items"][0]["color"] = json!("red;background:url(x)");
    bad["items"][0]["waste"] = json!(-1);
    bad["items"][1]["components"] = json!([{ "id": "c1", "formula": "constructor.constructor(\"x\")()" }]);
    bad["marks"][0]["pts"] = json!([[1e308, 0], [1, 1]]);
    bad["marks"][1]["kind"] = json!("count");
    bad["marks"][2]["item"] = json!("nope");
    let wi2 = wire["items"][2]["id"].clone();
    bad["marks"].as_array_mut().unwrap().extend([json!({ "item": wi2, "kind": "count", "pts": [[0, 0, 9]] }), Value::Null, json!("x")]);
    bad["sheet"] = json!({ "ftPerUnit": -4, "view": [0, 0, "x", 1], "label": "x".repeat(500) });
    bad["anchor"] = json!({ "x": null, "y": 0 });
    let rb = clipboard::read_clip(&bad).unwrap();
    // S.6: the owned read (a copy just made) gives the same clip and warnings as the borrowed one
    for c in [&wire, &bad] {
        let (a, b) = (clipboard::read_clip(c).unwrap(), clipboard::read_clip_owned(c.clone()).unwrap());
        assert_eq!(jv::stringify(&a.clip), jv::stringify(&b.clip));
        assert_eq!(a.warnings, b.warnings);
    }
    let mut odd = wire.clone();
    odd["marks"] = json!("not a list");
    assert_eq!(clipboard::read_clip(&odd).map(|r| r.clip), clipboard::read_clip_owned(odd.clone()).map(|r| r.clip));
    assert_eq!(rb.warnings.len(), 1);
    assert!(rb.clip["marks"].as_array().unwrap().len() == 1 && rb.clip["marks"][0]["kind"] == "count");
    assert!(
        rb.clip["sheet"]["ftPerUnit"].is_null()
            && rb.clip["sheet"]["view"].is_null()
            && rb.clip["sheet"]["label"].as_str().unwrap().len() <= 80
    );
    for junk in [
        Value::Null,
        json!("text"),
        json!({ "format": "takeoffcraft-project" }),
        json!({ "format": "takeoffcraft-clip", "version": 9, "items": [] }),
        json!({ "format": "takeoffcraft-clip", "version": 1, "items": [] }),
    ] {
        assert!(clipboard::read_clip(&junk).is_err());
        assert!(clipboard::read_clip_owned(junk).is_err());
    }
    let far = clipboard::read_clip(&clipboard::copy_marks(&tc.state, &dots, &ctx).unwrap()).unwrap().clip;
    let n0 = tc.marks().len();
    assert!(clipboard::paste_clip(&mut tc, &far, &PasteOpts { sheet: Some(sa.clone()), dx: 2e7, ..PasteOpts::default() }).is_err());
    assert_eq!(tc.marks().len(), n0);
}

#[test]
fn hostile_inputs_never_panic() {
    // deep nesting, wrong types everywhere, huge counts
    let deep = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
    assert!(jv::parse_json(&deep, 512).is_err());
    for v in [
        json!(null),
        json!([]),
        json!("x"),
        json!(1),
        json!({ "format": model::FORMAT, "version": "1", "items": "x", "marks": {} }),
        json!({ "format": model::FORMAT, "version": [1], "items": [1, null, "x", { "id": 5 }], "marks": [[], null, { "id": "m1" }], "sheets": [1, 2] }),
    ] {
        let _ = model::read_project(&v);
    }
    let mut t = Takeoff::default();
    let _ = t.add_mark(&o(json!({ "item": "nope" })));
    let _ = t.update_mark("x", &o(json!({ "pts": "x", "holes": 5 })), None);
    let _ = t.move_item("x", f64::NAN);
    let _ = t.add_group(Some(&json!(null)), Some(&json!("x")), &[]);
}

#[test]
fn the_project_is_written_as_json_stringify_with_indent_1() {
    // node -e 'console.log(JSON.stringify({a:[1,{b:[]},{}],c:"x\"y",d:{e:null,f:1.5}},null,1))'
    let v = serde_json::json!({ "a": [1, { "b": [] }, {}], "c": "x\"y", "d": { "e": null, "f": 1.5 } });
    let want =
        "{\n \"a\": [\n  1,\n  {\n   \"b\": []\n  },\n  {}\n ],\n \"c\": \"x\\\"y\",\n \"d\": {\n  \"e\": null,\n  \"f\": 1.5\n }\n}";
    assert_eq!(cc_takeoff::jv::stringify_indent(&v, 1), want);
}
