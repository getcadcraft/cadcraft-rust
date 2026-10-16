//! The test sections 'solid tools: union, subtract, intersect', 'SketchUp import', '3D printing: print3d.js' and
//! '3D printing from a model: print-mc.js' of ModelCraft's Node test at the frozen 0.5 commit, ported.

use cc_io_m3d::print3d::{CheckOpts, Tri, mesh_from_triangles, print_check, stl_binary, three_mf, tri_tri, weld};
use cc_io_m3d::printmc::print_triangles;
use cc_io_m3d::skp::{SkpModel, import_skp};
use cc_io_m3d::solids::{is_solid, solid_op};
use cc_m3d::def::face_normal;
use cc_m3d::model::place;
use cc_m3d::ops::{add_polygon, push_pull, rect_pts};
use cc_m3d::vec::{IDENT, translation};
use cc_m3d::{Model, Sel};
use serde_json::{Value, json};

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// A box drawn at the root and made into a group (the test's `box()`).
fn box_group(m: &mut Model, o: [f64; 3], w: f64, d: f64, h: f64) -> f64 {
    m.change("box", |mm| {
        let root = mm.root_mut().unwrap();
        let before: Vec<f64> = root.faces.keys().chain(root.edges.keys()).collect();
        add_polygon(
            root,
            &[[o[0], o[1], o[2]], [o[0] + w, o[1], o[2]], [o[0] + w, o[1] + d, o[2]], [o[0], o[1] + d, o[2]]],
            Some([0.0, 0.0, 1.0]),
        )
        .unwrap();
        let f = root.faces.keys().find(|id| !before.contains(id)).unwrap();
        push_pull(root, f, h);
        let mut sel: Vec<Sel> = root.faces.keys().filter(|id| !before.contains(id)).map(|id| Sel::new("f", id)).collect();
        sel.extend(root.edges.keys().filter(|id| !before.contains(id)).map(|id| Sel::new("e", id)));
        Ok(mm.make_group("root", &sel, "group", None)?.unwrap())
    })
    .unwrap()
}

#[test]
fn solid_tools() {
    let mut model = Model::new();
    model.units = "m".into();
    let a = box_group(&mut model, [0.0; 3], 2.0, 2.0, 2.0);
    let b = box_group(&mut model, [1.0, 0.0, 0.0], 2.0, 2.0, 2.0);
    model
        .change("tag", |m| {
            m.add_tag("Walls");
            m.set_tag("root", &[a], Some("Walls"))?;
            m.root_mut().unwrap().insts.get_mut(a).unwrap().color = Some("#c0392b".into());
            Ok(())
        })
        .unwrap();
    assert!(is_solid(&model, "root", a) && is_solid(&model, "root", b));
    let undo0 = model.undo_stack.len();
    let r = solid_op(&mut model, &[], "union", &[a, b], &[]).unwrap();
    assert!(r.closed && close(r.volume, 12.0, 1e-6) && model.root().unwrap().insts.len() == 1, "union: 12 m³ ({})", r.volume);
    let ri = model.root().unwrap().insts.get(r.iid).unwrap().clone();
    assert_eq!(ri.tag.as_deref(), Some("Walls"));
    assert_eq!(ri.color.as_deref(), Some("#c0392b"));
    assert_eq!(model.undo_stack.len(), undo0 + 1, "one undo step");
    assert_eq!(model.def(&ri.def).unwrap().faces.len(), 6, "coplanar triangles merged into 6 faces");
    model.undo().unwrap();
    assert!(model.root().unwrap().insts.has(a) && model.root().unwrap().insts.has(b));
    let r = solid_op(&mut model, &[], "intersect", &[a, b], &[]).unwrap();
    assert!(close(r.volume, 4.0, 1e-6));
    model.undo().unwrap();
    let r = solid_op(&mut model, &[], "subtract", &[a], &[b]).unwrap();
    assert!(close(r.volume, 4.0, 1e-6) && r.closed);
    model.undo().unwrap();
    let c = box_group(&mut model, [0.5, 0.5, -1.0], 1.0, 1.0, 4.0);
    let r = solid_op(&mut model, &[], "subtract", &[a], &[c]).unwrap();
    let faces = model.def(&model.root().unwrap().insts.get(r.iid).unwrap().def).unwrap().faces.len();
    assert!(close(r.volume, 6.0, 1e-6) && r.closed && faces == 10, "a post through a block: 6 m³, 10 faces ({faces})");
    model.undo().unwrap();
    // not a solid: says which, and changes nothing
    let open = model
        .change("open", |m| {
            let root = m.root_mut().unwrap();
            add_polygon(root, &[[5.0, 0.0, 0.0], [6.0, 0.0, 0.0], [6.0, 1.0, 0.0], [5.0, 1.0, 0.0]], Some([0.0, 0.0, 1.0])).unwrap();
            let mut sel: Vec<Sel> = root.faces.keys().map(|id| Sel::new("f", id)).collect();
            sel.extend(root.edges.keys().map(|id| Sel::new("e", id)));
            Ok(m.make_group("root", &sel, "group", None)?.unwrap())
        })
        .unwrap();
    let n0 = model.undo_stack.len();
    let e = solid_op(&mut model, &[], "union", &[a, open], &[]).unwrap_err().to_string();
    assert!(e.contains("is not a solid") && model.undo_stack.len() == n0, "{e}");
    let far = box_group(&mut model, [10.0, 0.0, 0.0], 1.0, 1.0, 1.0);
    let e = solid_op(&mut model, &[], "intersect", &[a, far], &[]).unwrap_err().to_string();
    assert!(e.contains("do not overlap"), "{e}");
    // inside an open group, the result stays in that group
    let outer = model.change("grp", |m| Ok(m.make_group("root", &[Sel::new("i", a), Sel::new("i", b)], "group", None)?.unwrap())).unwrap();
    let inner_id = model.root().unwrap().insts.get(outer).unwrap().def.clone();
    let ids: Vec<f64> = model.def(&inner_id).unwrap().insts.keys().collect();
    let r = solid_op(&mut model, &[outer], "union", &ids, &[]).unwrap();
    let inner = model.def(&inner_id).unwrap();
    assert!(inner.insts.len() == 1 && inner.insts.has(r.iid) && close(r.volume, 12.0, 1e-6));
}

fn skp_fixture() -> Value {
    let sq = |s: f64, z: f64| json!([[[0, 0, z], [s, 0.0, z], [s, s, z], [0.0, s, z]]]);
    let tr = |x: f64| json!([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, x, 0, 0, 1]);
    json!({
        "format": "skp", "container": "legacy", "version": "6.0.1", "major": 6,
        "materials": [{ "name": "Red", "color": [255, 0, 0], "alpha": 1, "texture": null }, { "name": "Blue", "color": [0, 0, 255], "alpha": 1, "texture": null },
            { "name": "Wood", "color": null, "alpha": 1, "texture": { "name": "wood.jpg" } }],
        "layers": [{ "name": "Layer0", "visible": true, "color": null }, { "name": "Off", "visible": false, "color": null }, { "name": "Doors", "visible": true, "color": null }],
        "defs": [
            { "name": "Model", "kind": "model", "faces": [{ "loops": sq(100.0, 0.0), "front": 2, "back": null, "layer": 0 }, { "loops": sq(50.0, 10.0), "front": null, "back": null, "layer": 1 }],
              "edges": [{ "a": [0, 0, 0], "b": [0, 0, 100], "layer": 0, "soft": false, "smooth": false, "hidden": false }, { "a": [0, 0, 0], "b": [0, 100, 100], "layer": 0, "soft": false, "smooth": false, "hidden": true }], "soft": [],
              "insts": [{ "def": 1, "m": tr(10.0), "name": "C1", "layer": 0, "hidden": false, "material": 0 }, { "def": 1, "m": tr(20.0), "name": "", "layer": 0, "hidden": false, "material": 1 },
                { "def": 1, "m": tr(30.0), "name": "", "layer": 0, "hidden": true, "material": 0 }, { "def": 2, "m": tr(0.0), "name": "G", "layer": 2, "hidden": false, "material": null },
                { "def": 2, "m": tr(40.0), "name": "OffBoard", "layer": 1, "hidden": false, "material": null }] },
            { "name": "Cap", "kind": "component", "faces": [{ "loops": sq(1.0, 0.0), "front": null, "back": null, "layer": 0 }, { "loops": [[[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]]], "front": null, "back": null, "layer": 0 }],
              "edges": [], "soft": [[[0, 0, 0], [1, 0, 0]]], "insts": [] },
            { "name": "Board", "kind": "group", "faces": [{ "loops": sq(2.0, 0.0), "front": 0, "back": null, "layer": 2 }], "edges": [], "soft": [], "insts": [] }
        ],
        "root": 0, "thumbnail": null, "units": { "length": "in" }, "notes": [], "counts": {}
    })
}

#[test]
fn sketchup_import() {
    let skp = SkpModel::from_json(&skp_fixture());
    let mut m = Model::new();
    let r = m
        .change("Import SketchUp model", |mm| {
            let x = import_skp(mm, &skp, "PIMETA")?;
            place(mm.root_mut().unwrap(), &x.def, &IDENT);
            Ok(x)
        })
        .unwrap();
    let top = m.def(&r.def).unwrap();
    assert!(top.kind == "group" && top.name == "PIMETA");
    let caps: Vec<String> = m.defs.values().filter(|d| d.name == "Cap").map(|d| d.id.clone()).collect();
    assert_eq!(caps.len(), 1, "a component painted two colours stays one component");
    let cap = m.def(&caps[0]).unwrap();
    assert_eq!(cap.kind, "component");
    let cap_insts: Vec<_> = top.insts.values().filter(|i| i.def == caps[0]).collect();
    assert!(cap_insts.len() == 3 && cap.faces.values().all(|f| f.color.is_none()));
    let mut paints: Vec<String> = cap_insts.iter().map(|i| i.color.clone().unwrap_or_default()).collect();
    paints.sort();
    assert_eq!(paints, vec!["#0000ff", "#ff0000", "#ff0000"]);
    let hid: Vec<_> = cap_insts.iter().filter(|i| i.hidden).collect();
    assert!(hid.len() == 1 && close(hid[0].m[12], 30.0 * 0.0254, 1e-12));
    let off = m.tag("Off").map(|i| m.tags[i].hidden);
    let doors = m.tag("Doors").map(|i| m.tags[i].hidden);
    assert_eq!((off, doors), (Some(true), Some(false)), "tags come over, hidden as in SketchUp");
    assert_eq!(top.insts.values().find(|i| i.name.as_deref() == Some("G")).unwrap().tag.as_deref(), Some("Doors"));
    assert_eq!(top.faces.len(), 2);
    assert_eq!(top.faces.values().filter(|f| f.hidden).count(), 1, "the face on a hidden tag comes in hidden");
    assert_eq!(top.edges.values().filter(|e| e.hidden).count(), 1);
    assert_eq!(r.report["hidden"], json!(3.0));
    assert_eq!((r.report["tags"].clone(), r.report["untagged"].clone()), (json!(2.0), json!(1.0)));
    assert_eq!(r.report["textured"], json!(1.0));
    let maxx = top.verts.values().map(|v| v[0]).fold(0.0f64, f64::max);
    assert!(close(maxx, 100.0 * 0.0254, 1e-12), "inches become metres");
    assert!(cap.edges.values().any(|e| e.soft), "soft edges stay soft");
    assert_eq!((r.report["components"].clone(), r.report["groups"].clone()), (json!(1.0), json!(1.0)));
    // a tag the model already has is reused as it is
    let mut m2 = Model::new();
    m2.tags.push(cc_m3d::model::Tag { name: "Doors".into(), hidden: true });
    let r2 = m2.change("again", |mm| import_skp(mm, &skp, "again")).unwrap();
    assert_eq!(m2.tags.iter().filter(|t| t.name.to_lowercase() == "doors").count(), 1);
    assert_eq!(r2.report["tags"], json!(1.0));
    // one undo takes the import and its tags away; an empty model is refused
    m.undo().unwrap();
    assert!(m.tag("Off").is_none() && m.tag("Doors").is_none());
    let empty = SkpModel::from_json(&json!({ "format": "skp", "defs": [], "root": 0 }));
    assert!(import_skp(&mut Model::new(), &empty, "x").is_err());
}

fn box_tris(lo: [f64; 3], hi: [f64; 3], inward: bool, color: Option<&str>) -> Vec<Tri> {
    let [x0, y0, z0] = lo;
    let [x1, y1, z1] = hi;
    let v = [[x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]];
    let quads = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]];
    let mut out = Vec::new();
    for q in quads {
        for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            let p = [v[t[0]], v[t[1]], v[t[2]]];
            out.push(Tri { p: if inward { [p[0], p[2], p[1]] } else { p }, color: color.map(str::to_string) });
        }
    }
    out
}
fn check(tris: &[Tri], o: &CheckOpts) -> Value {
    Value::Object(print_check(&mesh_from_triangles(tris), o).value)
}
fn kinds(r: &Value, k: &str) -> Vec<String> {
    r[k].as_array().unwrap().iter().map(|p| p["kind"].as_str().unwrap().to_string()).collect()
}

#[test]
fn print3d_known_answers() {
    let d = CheckOpts::default();
    let cube = check(&box_tris([0.0; 3], [20.0; 3], false, None), &d);
    assert!(cube["ok"] == json!(true) && cube["shells"] == json!(1) && cube["triangles"] == json!(12) && cube["vertices"] == json!(8));
    assert!(close(cube["volume"].as_f64().unwrap(), 8000.0, 1e-6) && close(cube["area"].as_f64().unwrap(), 2400.0, 1e-6));
    assert!(
        close(cube["bedArea"].as_f64().unwrap(), 400.0, 1e-9) && cube["overhangArea"] == json!(0.0) && kinds(&cube, "warnings").is_empty()
    );
    let open = check(&box_tris([0.0; 3], [20.0; 3], false, None)[2..], &d);
    assert!(open["ok"] == json!(false) && open["open"] == json!(4.0) && kinds(&open, "problems") == ["open"] && open["volume"].is_null());
    let mut one = box_tris([0.0; 3], [20.0; 3], false, None);
    one[4] = Tri { p: [one[4].p[0], one[4].p[2], one[4].p[1]], color: None };
    let flipped = check(&one, &d);
    assert!(flipped["flipped"] == json!(3.0) && kinds(&flipped, "problems") == ["flipped"]);
    let inside = check(&box_tris([0.0; 3], [20.0; 3], true, None), &d);
    assert!(inside["inverted"] == json!(1.0) && kinds(&inside, "problems") == ["inverted"]);
    let mut hollow = box_tris([0.0; 3], [30.0; 3], false, None);
    hollow.extend(box_tris([10.0; 3], [20.0; 3], true, None));
    let h = check(&hollow, &d);
    assert!(h["ok"] == json!(true) && h["shells"] == json!(2) && close(h["volume"].as_f64().unwrap(), 26000.0, 1e-6));
    let mut two = box_tris([0.0; 3], [20.0; 3], false, None);
    two.extend(box_tris([10.0; 3], [30.0; 3], false, None));
    let t = check(&two, &d);
    assert!(t["ok"] == json!(true) && t["overlaps"].as_f64().unwrap() > 0.0 && kinds(&t, "warnings").contains(&"overlap".to_string()));
    let mut touching = box_tris([0.0; 3], [10.0; 3], false, None);
    touching.extend(box_tris([10.0, 0.0, 0.0], [20.0, 10.0, 10.0], false, None));
    let tc = check(&touching, &d);
    assert!(tc["ok"] == json!(true) && tc["touching"] == json!(4.0) && tc["nonManifold"] == json!(0.0));
    let fold: Vec<Tri> = box_tris([0.0; 3], [20.0; 3], false, None)
        .into_iter()
        .map(|t| Tri { p: t.p.map(|p| if p == [20.0, 20.0, 20.0] { [15.0, 15.0, -10.0] } else { p }), color: None })
        .collect();
    let f = check(&fold, &d);
    assert!(f["selfIntersections"].as_f64().unwrap() > 0.0 && kinds(&f, "problems").contains(&"selfintersect".to_string()));
    assert!(kinds(&check(&box_tris([0.0; 3], [0.5; 3], false, None), &d), "warnings").contains(&"tiny".to_string()));
    let bed = CheckOpts { bed: Some([220.0, 220.0, 250.0]), ..CheckOpts::default() };
    assert!(kinds(&check(&box_tris([0.0; 3], [300.0, 100.0, 50.0], false, None), &bed), "warnings").contains(&"bed".to_string()));
    let bed2 = CheckOpts { bed: Some([220.0, 200.0, 250.0]), ..CheckOpts::default() };
    assert!(!kinds(&check(&box_tris([0.0; 3], [100.0, 210.0, 50.0], false, None), &bed2), "warnings").contains(&"bed".to_string()));
    let mut deg = box_tris([0.0; 3], [10.0; 3], false, None);
    deg.push(Tri { p: [[0.0; 3], [5.0, 0.0, 0.0], [10.0, 0.0, 0.0]], color: None });
    let dg = check(&deg, &d);
    assert!(dg["ok"] == json!(true) && dg["degenerate"] == json!(1.0));
    let empty = check(&[], &d);
    assert!(empty["ok"] == json!(false) && kinds(&empty, "problems") == ["empty"]);
    assert!(tri_tri([0.0; 3], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [2.0, 2.0, -1.0], [2.0, 2.0, 1.0], [8.0, 8.0, 0.0], 1e-9));
    assert!(!tri_tri([0.0; 3], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [20.0, 2.0, -1.0], [20.0, 2.0, 1.0], [28.0, 8.0, 0.0], 1e-9));
    // files
    let mut tris = box_tris([0.0; 3], [20.0; 3], false, Some("#ff0000"));
    tris.extend(box_tris([30.0, 0.0, 0.0], [40.0, 10.0, 10.0], false, Some("#0000ff")));
    let coloured = weld(&mesh_from_triangles(&tris), 1e-4);
    let stl = stl_binary(&coloured, "test");
    assert!(stl.len() == 84 + 24 * 50 && u32::from_le_bytes(stl[80..84].try_into().unwrap()) == 24);
    let mf = three_mf(&coloured, "Two <cubes> & co", "test");
    let txt = String::from_utf8_lossy(&mf);
    assert!(
        txt.contains("3D/3dmodel.model")
            && txt.contains("unit=\"millimeter\"")
            && txt.contains("displaycolor=\"#FF0000FF\"")
            && txt.contains("Two &lt;cubes&gt; &amp; co")
    );
    assert_eq!(&mf[0..2], b"PK");
}

#[test]
fn print_from_a_model() {
    let mut m = Model::new();
    let (a, b) = m
        .change("cubes", |mm| {
            let root = mm.root_mut().unwrap();
            add_polygon(root, &rect_pts([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.02, 0.02), None).unwrap();
            let f = root.faces.keys().next().unwrap();
            push_pull(root, f, 0.02);
            let f0 = root.faces.keys().next().unwrap();
            let sel = cc_m3d::ops::connected(root, &[Sel::new("f", f0)]);
            let a = mm.make_group("root", &sel, "component", Some("Cube"))?.unwrap();
            let root = mm.root_mut().unwrap();
            let d = root.insts.get(a).unwrap().def.clone();
            root.insts.get_mut(a).unwrap().color = Some("#ff0000".into());
            let b = place(root, &d, &translation([0.05, 0.0, 0.0]));
            root.insts.get_mut(b).unwrap().color = Some("#0000ff".into());
            place(root, &d, &[-1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.15, 0.0, 0.0, 1.0]);
            let h = place(root, &d, &translation([0.3, 0.0, 0.0]));
            root.insts.get_mut(h).unwrap().hidden = true;
            Ok((a, b))
        })
        .unwrap();
    let (all, what) = print_triangles(&m, &[], &[]);
    let r = check(&all, &CheckOpts::default());
    assert!(what == "all" && r["ok"] == json!(true) && r["shells"] == json!(3), "3 cubes, the hidden copy left out");
    assert!(close(r["volume"].as_f64().unwrap(), 24000.0, 1e-6) && r["inverted"] == json!(0.0));
    assert!(close(r["size"][0].as_f64().unwrap(), 150.0, 1e-6) && close(r["bedArea"].as_f64().unwrap(), 1200.0, 1e-6));
    let (one, what) = print_triangles(&m, &[], &[Sel::new("i", b)]);
    let r1 = check(&one, &CheckOpts::default());
    assert!(
        what == "selection"
            && close(r1["volume"].as_f64().unwrap(), 8000.0, 1e-6)
            && one.iter().all(|t| t.color.as_deref() == Some("#0000ff"))
    );
    m.change("tag", |mm| {
        mm.tags.push(cc_m3d::model::Tag { name: "Off".into(), hidden: true });
        mm.root_mut().unwrap().insts.get_mut(a).unwrap().tag = Some("Off".into());
        Ok(())
    })
    .unwrap();
    assert_eq!(check(&print_triangles(&m, &[], &[]).0, &CheckOpts::default())["shells"], json!(2));
    assert!(print_triangles(&m, &[], &[Sel::new("i", a)]).0.is_empty());
    // a face selected inside the open component: 4 open edges
    let cdef = m.root().unwrap().insts.get(b).unwrap().def.clone();
    let f0 = m.def(&cdef).unwrap().faces.keys().next().unwrap();
    let rf = check(&print_triangles(&m, &[b], &[Sel::new("f", f0)]).0, &CheckOpts::default());
    assert!(rf["ok"] == json!(false) && rf["open"] == json!(4.0));
    // a box with its top gone
    let mut m2 = Model::new();
    m2.change("box", |mm| {
        let root = mm.root_mut().unwrap();
        add_polygon(root, &rect_pts([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.02, 0.02), None).unwrap();
        let f = root.faces.keys().next().unwrap();
        push_pull(root, f, 0.02);
        let top = root.faces.iter().find(|(_, f)| face_normal(root, f)[2] > 0.99).map(|(id, _)| id).unwrap();
        root.faces.delete(top);
        Ok(())
    })
    .unwrap();
    let r2 = check(&print_triangles(&m2, &[], &[]).0, &CheckOpts::default());
    assert!(r2["ok"] == json!(false) && r2["open"] == json!(4.0));
}
