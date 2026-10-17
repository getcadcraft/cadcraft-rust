//! Ported from the frozen Node test (bin/test-takeoffcraft.mjs at 594ab3e): "3D: wall thickness, base, openings;
//! stacked floors; GLB (T4–T6)" and "3D" (the engine half; the m3d byte-copy and import checks are about the JS
//! files' layout and have no Rust counterpart). The Khronos validator run stays JS-side.

mod common;
use common::*;

use cc_io_takeoff::glb::scene_to_glb;
use cc_takeoff::jv;
use cc_takeoff::model::{self, QOpts, Takeoff};
use cc_takeoff::scene3d::{self as s3, Level, View};
use cc_takeoff::units as u;
use serde_json::{Value, json};
use std::f64::consts::PI;

fn near(a: f64, b: f64, t: f64) -> bool {
    (a - b).abs() <= t * 1f64.max(a.abs()).max(b.abs())
}
fn m(o: &s3::Obj3, k: &str) -> f64 {
    o.measure[k].as_f64().unwrap_or(f64::NAN)
}
fn view(t: [f64; 6], w: f64, h: f64) -> View {
    View { transform: Some(t), width: w, height: h, step: None }
}
fn glb_json(b: &[u8]) -> Value {
    let u32at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    assert!(u32at(0) == 0x4654_6C67 && u32at(4) == 2 && u32at(8) as usize == b.len(), "GLB header");
    let jlen = u32at(12) as usize;
    serde_json::from_slice(&b[20..20 + jlen]).unwrap()
}

#[test]
fn walls_slabs_stacks_glb() {
    let mut tz = Takeoff::default();
    let (hz, hz2) = ("f".repeat(64), "9".repeat(64));
    tz.add_file(&o(json!({ "hash": hz, "name": "L1.pdf", "size": 1, "pages": 1 })));
    tz.add_file(&o(json!({ "hash": hz2, "name": "L2.pdf", "size": 1, "pages": 2 })));
    let (k1, k2, k3) = (model::sheet_key(&hz, 0.0), model::sheet_key(&hz2, 0.0), model::sheet_key(&hz2, 1.0));
    for sk in [&k1, &k2] {
        tz.set_scale(sk, &json!({ "method": "preset", "preset": "a1/4", "ftPerUnit": 1.0 / 18.0, "label": "1/4\"", "cal": null }), None)
            .unwrap();
    }
    let wall = item(&mut tz, json!({ "name": "CMU", "type": "linear", "heightFt": 10, "thickFt": 2.0 / 3.0, "baseFt": 1, "headFt": 7 }));
    let wi = tz.item(&wall).unwrap();
    assert!(wi["thickFt"] == 2.0 / 3.0 && wi["baseFt"] == 1.0 && wi["headFt"] == 7.0);
    let ar = item(&mut tz, json!({ "name": "x", "type": "area", "thickFt": 1 }));
    assert!(tz.item(&ar).unwrap().get("thickFt").is_none());
    let run = mark(
        &mut tz,
        json!({ "item": wall, "sheet": k1, "pts": [{ "x": 0, "y": 0 }, { "x": 288, "y": 0 }, { "x": 360, "y": 0 }, { "x": 360, "y": 180 }], "deducts": [1] }),
    );
    let sc = s3::build_scene(&tz.state, &k1, &View::default()).unwrap();
    let w = sc.objects.iter().find(|x| x.id == run.as_str()).unwrap();
    let zs: Vec<f64> = w.tris.chunks(3).map(|c| c[2]).collect();
    assert!(zs.iter().copied().fold(f64::INFINITY, f64::min) == 1.0 && zs.iter().copied().fold(f64::NEG_INFINITY, f64::max) == 11.0);
    let want = (16.0 + 10.0) * 10.0 * (2.0 / 3.0) + 4.0 * 3.0 * (2.0 / 3.0);
    assert!(near(s3::mesh_volume(&w.tris).abs(), want, 1e-9), "{} vs {want}", s3::mesh_volume(&w.tris));
    assert_eq!(m(w, "wallSqft"), 260.0);
    let slab = item(&mut tz, json!({ "name": "Ceiling", "type": "area", "heightFt": 0.5, "baseFt": 9 }));
    let ce = mark(&mut tz, json!({ "item": slab, "sheet": k1, "pts": rect(0.0, 0.0, 90.0, 90.0) }));
    let sc2 = s3::build_scene(&tz.state, &k1, &View::default()).unwrap();
    let co = sc2.objects.iter().find(|x| x.id == ce.as_str()).unwrap();
    assert!(near(co.anchor.unwrap()[2], 9.5, 1e-9) && near(s3::mesh_volume(&co.tris).abs(), 25.0 * 0.5, 1e-9));
    tz.update_item(&wall, &o(json!({ "thickFt": 0 })), None).unwrap();
    assert!(tz.item(&wall).unwrap().get("thickFt").is_none());
    tz.undo();
    // stacking
    let flr = item(&mut tz, json!({ "name": "Floor", "type": "area" }));
    mark(&mut tz, json!({ "item": flr, "sheet": k2, "pts": rect(0.0, 0.0, 180.0, 180.0) }));
    let mut lv = cc_takeoff::jv::Obj::new();
    lv.insert(k1.clone(), json!(0));
    lv.insert(k2.clone(), json!(12));
    lv.insert(k3.clone(), json!(24));
    tz.set_levels(&lv).unwrap();
    assert!(tz.level_of(&k2) == Some(12.0) && tz.level_of(&k3) == Some(24.0));
    let level = |k: &str| Level {
        key: k.to_string(),
        elev_ft: tz.level_of(k).unwrap_or(0.0),
        view: view([1.0, 0.0, 0.0, -1.0, 0.0, 792.0], 612.0, 792.0),
    };
    let st = s3::build_stack(&tz.state, &[level(&k1), level(&k2), level(&k3)], None).unwrap();
    let sj = st.to_json();
    assert!(sj["stacked"] == true && sj["levels"].as_array().unwrap().len() == 2 && sj["groundZ"] == 0.0, "{}", sj["levels"]);
    let fl = st.objects.iter().find(|x| x.item == flr.as_str()).unwrap();
    assert!(near(fl.anchor.unwrap()[2], 12.0 + 2.0 / 12.0, 1e-9));
    assert!(st.objects.iter().any(|x| x.kind == "frame" && x.level.as_deref() == Some(k2.as_str()) && x.lines.get(2) == Some(&12.0)));
    assert!(st.notes.iter().any(|n| n.contains("1 sheet with a level has no scale")), "{:?}", st.notes);
    assert!(st.totals.walls == 1.0 && st.totals.plates == 1.0 && near(st.bbox.max[2], 12.0 + 2.0 / 12.0, 1e-9));
    assert_eq!(model::read_project(&tz.to_json("T")).unwrap().state["sheets"][&k2]["level"], 12.0);
    // GLB
    let glb = scene_to_glb(&sj, "Stack");
    let gj = glb_json(&glb);
    assert_eq!(gj["nodes"].as_array().unwrap().len(), st.objects.iter().filter(|x| x.kind != "frame").count());
    let wn = gj["nodes"].as_array().unwrap().iter().find(|n| n["extras"]["kind"] == "wall").unwrap();
    let mesh = wn["mesh"].as_u64().unwrap() as usize;
    let acc = &gj["accessors"][gj["meshes"][mesh]["primitives"][0]["attributes"]["POSITION"].as_u64().unwrap() as usize];
    assert!(near(acc["max"][1].as_f64().unwrap(), 11.0 * 0.3048, 1e-6) && near(acc["min"][1].as_f64().unwrap(), 0.3048, 1e-6));
    let mats = gj["materials"].as_array().unwrap();
    assert!(mats.iter().any(|x| x["alphaMode"] == "BLEND") && mats.iter().any(|x| x["alphaMode"] == "OPAQUE"));
    tz.undo();
    assert!(tz.level_of(&k2).is_none());
    let empty = scene_to_glb(&json!({ "objects": [] }), "");
    assert!(empty.len() > 20 && glb_json(&empty)["scenes"][0]["nodes"].as_array().unwrap().is_empty());
}

#[test]
fn scene_measures_match_quantities() {
    let mut t = Takeoff::default();
    t.add_file(&o(json!({ "hash": "h3d", "name": "Plan.pdf", "size": 1, "pages": 2 })));
    let (sh, other) = (model::sheet_key("h3d", 0.0), model::sheet_key("h3d", 1.0));
    let no = s3::build_scene(&t.state, &sh, &View::default()).unwrap_err();
    assert!(no.to_lowercase().contains("no scale"), "{no}");
    let k = u::preset_ft_per_unit("a1/4", 1.0).unwrap();
    for s in [&sh, &other] {
        t.set_scale(s, &json!({ "method": "preset", "preset": "a1/4", "ftPerUnit": k, "label": "1/4\" = 1'-0\"", "cal": null }), None)
            .unwrap();
    }
    let wall_it = item(&mut t, json!({ "name": "Wall A", "type": "linear", "heightFt": 9 }));
    let curb = item(&mut t, json!({ "name": "Curb", "type": "linear" }));
    let slab_it = item(&mut t, json!({ "name": "Slab", "type": "area", "heightFt": 0.5 }));
    let pan = item(&mut t, json!({ "name": "Pan", "type": "area" }));
    let cnt = item(&mut t, json!({ "name": "Outlets", "type": "count" }));
    let wall_m = mark(
        &mut t,
        json!({ "item": wall_it, "sheet": sh, "pts": [{ "x": 72, "y": 72 }, { "x": 288, "y": 72, "b": (PI / 8.0).tan() }, { "x": 360, "y": 144 }, { "x": 360, "y": 360 }] }),
    );
    mark(&mut t, json!({ "item": curb, "sheet": sh, "pts": [{ "x": 72, "y": 400 }, { "x": 300, "y": 400 }] }));
    let slab_m = mark(
        &mut t,
        json!({ "item": slab_it, "sheet": sh, "pts": [{ "x": 100, "y": 100 }, { "x": 460, "y": 100 }, { "x": 460, "y": 370, "b": 1 }, { "x": 100, "y": 370 }],
            "holes": [rect(150.0, 150.0, 100.0, 50.0), [{ "x": 300, "y": 250, "b": 1 }, { "x": 360, "y": 250, "b": 1 }]] }),
    );
    let pan_m = mark(&mut t, json!({ "item": pan, "sheet": sh, "pts": [{ "x": 500, "y": 500, "b": 1 }, { "x": 572, "y": 500, "b": 1 }] }));
    for (x, y) in [(50, 50), (80, 600), (500, 700)] {
        mark(&mut t, json!({ "item": cnt, "sheet": sh, "pts": [{ "x": x, "y": y }] }));
    }
    mark(&mut t, json!({ "item": wall_it, "sheet": other, "pts": [{ "x": 0, "y": 0 }, { "x": 720, "y": 0 }] }));
    let r = s3::build_scene(&t.state, &sh, &view([1.0, 0.0, 0.0, -1.0, 0.0, 792.0], 612.0, 792.0)).unwrap();
    let q = model::quantities(&t.state, &QOpts::default());
    let mut kinds: Vec<&str> = r.objects.iter().map(|x| x.kind).collect();
    kinds.sort_unstable();
    assert_eq!(kinds.join(","), "marker,marker,marker,plate,run,slab,wall");
    let find = |id: &str| r.objects.iter().find(|x| x.id == id).unwrap();
    let (w, sl, pl) = (find(&wall_m), find(&slab_m), find(&pan_m));
    let qw = q.get(&wall_it).unwrap().sheet(&sh).unwrap();
    let qs = q.get(&slab_it).unwrap().sheet(&sh).unwrap();
    let qp = q.get(&pan).unwrap().sheet(&sh).unwrap();
    assert!(near(m(w, "lengthFt"), qw.length_ft, 1e-9) && near(m(w, "wallSqft"), qw.length_ft * 9.0, 1e-9));
    let vars = model::item_vars(t.item(&wall_it).unwrap(), qw);
    assert!(near(m(w, "wallSqft"), vars.iter().find(|(k, _)| k == "A").unwrap().1, 1e-9));
    let mesh_wall = s3::mesh_area(&w.tris);
    assert!(mesh_wall < m(w, "wallSqft") && mesh_wall > m(w, "wallSqft") * 0.999);
    let want_vol = qs.area_sqft * 0.5;
    assert!(near(m(sl, "netSqft"), qs.area_sqft, 1e-9) && near(m(sl, "volumeCuft"), want_vol, 1e-9));
    let gross = (360.0 * 270.0 + PI * 180.0 * 180.0 / 2.0) * k * k;
    let cut = (100.0 * 50.0 + PI * 30.0 * 30.0) * k * k;
    assert!(near(m(sl, "grossSqft"), gross, 1e-12) && near(m(sl, "cutSqft"), cut, 1e-12) && near(m(sl, "netSqft"), gross - cut, 1e-12));
    let mv = s3::mesh_volume(&sl.tris);
    assert!(mv > 0.0 && (mv - want_vol).abs() / want_vol < 0.002);
    assert!(
        pl.measure["thin"] == true
            && pl.measure["volumeCuft"].is_null()
            && m(pl, "drawnFt") == s3::THIN_FT
            && near(m(pl, "netSqft"), qp.area_sqft, 1e-9)
    );
    let disc = PI * (36.0 * k).powi(2) * s3::THIN_FT;
    assert!((s3::mesh_volume(&pl.tris) - disc).abs() / disc < 0.002);
    assert!(r.objects.iter().find(|x| x.kind == "run").unwrap().tris.is_empty());
    assert!(
        r.notes.iter().any(|n| n.contains("1 run has no height")) && r.notes.iter().any(|n| n.contains("1 area has no depth")),
        "{:?}",
        r.notes
    );
    for (id, bi) in &r.by_item {
        let qs = q.get(id).unwrap().sheet(&sh).unwrap();
        assert!(near(bi.length_ft, qs.length_ft, 1e-9) && near(bi.net_sqft, qs.area_sqft, 1e-9) && bi.count == qs.count, "{id}");
    }
    assert_eq!(r.totals.markers, 3.0);
    assert!(r.objects.iter().all(|x| x.tris.iter().chain(&x.lines).all(|v| v.is_finite())));
    assert!(near(r.ground.0, 612.0 * k, 1e-9) && near(r.ground.1, 792.0 * k, 1e-9));
    let inside =
        |x: &s3::Obj3| x.tris.chunks(3).all(|c| c[0] >= -1e-9 && c[0] <= r.ground.0 + 1e-9 && c[1] >= -1e-9 && c[1] <= r.ground.1 + 1e-9);
    assert!(r.objects.iter().filter(|x| x.kind != "marker").all(inside));
    assert!(near(sl.anchor.unwrap()[2], 0.5, 1e-9) && near(w.anchor.unwrap()[2], 4.5, 1e-9));
    let rot = s3::build_scene(&t.state, &sh, &view([0.0, 1.0, 1.0, 0.0, 0.0, 0.0], 792.0, 612.0)).unwrap();
    let wr = rot.objects.iter().find(|x| x.id == wall_m.as_str()).unwrap();
    assert!(near(rot.ground.0, 792.0 * k, 1e-9) && near(rot.ground.1, 612.0 * k, 1e-9) && near(s3::mesh_area(&wr.tris), mesh_wall, 1e-9));
    let f2 = s3::sheet_frame(k, &view([0.0, 1.0, 1.0, 0.0, 0.0, 0.0], 792.0, 612.0));
    let (a, b) = (f2.at(&cc_takeoff::geom::P::new(0.0, 0.0)), f2.at(&cc_takeoff::geom::P::new(72.0, 0.0)));
    assert!(near((b[0] - a[0]).hypot(b[1] - a[1]), 72.0 * k, 1e-9));
    let uu = s3::sheet_frame(k, &view([2.0, 0.0, 0.0, -2.0, 0.0, 1584.0], 1224.0, 1584.0));
    let (c0, c1) = (uu.at(&cc_takeoff::geom::P::new(0.0, 0.0)), uu.at(&cc_takeoff::geom::P::new(72.0, 0.0)));
    assert!(near(c1[0] - c0[0], 72.0 * k, 1e-9) && near(uu.w, 612.0 * k, 1e-9));
    let mut empty = Takeoff::default();
    empty.add_file(&o(json!({ "hash": "e", "name": "E.pdf", "size": 1, "pages": 1 })));
    empty
        .set_scale(
            &model::sheet_key("e", 0.0),
            &json!({ "method": "preset", "preset": "a1/4", "ftPerUnit": k, "label": "", "cal": null }),
            None,
        )
        .unwrap();
    let re = s3::build_scene(&empty.state, &model::sheet_key("e", 0.0), &View::default()).unwrap();
    assert!(re.objects.is_empty() && re.marks_box.is_none());
    let _ = jv::stringify(&re.to_json());
}
