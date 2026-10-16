//! The Model sections of ModelCraft's frozen Node test (bin/test-modelcraft.mjs at 594ab3e): undo and previews,
//! components, dimensions, tags, section planes, drawing axes, the outliner's move, scenes — the parts that use
//! model.js only (dimension layout, picking and the scene graph live in infer.js and are left out).

mod common;

use cc_m3d::Sel;
use cc_m3d::def::*;
use cc_m3d::file::clean_scene;
use cc_m3d::model::{Dim, place};
use cc_m3d::ops::*;
use cc_m3d::solids::*;
use cc_m3d::vec::*;
use cc_m3d::{Model, Res};
use common::*;
use serde_json::{Value, json};
use std::f64::consts::PI;

fn vol(d: &Def) -> f64 {
    solid_info(d).volume.unwrap_or(f64::NAN)
}
fn reload(m: &mut Model) -> Model {
    let mut b = Model::new();
    let j: Value = serde_json::from_str(&m.to_json_string(false)).unwrap();
    ok(b.load(&j, false));
    b
}
fn tag_count(m: &Model, name: &str) -> f64 {
    m.tag_counts().into_iter().find(|(k, _)| k == name).map_or(f64::NAN, |(_, v)| v)
}

// ---------------------------------------------------------------------------------------------------- undo

#[test]
fn undo_and_preview() {
    let mut m = Model::new();
    ok(m.change("Rectangle", |mm| Ok(rect(root(mm), [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 4.0, 3.0))));
    ok(m.change("Push/pull", |mm| {
        let d = root(mm);
        let f = first_face(d);
        Ok(push_pull(d, f, 2.0))
    }));
    assert_eq!(m.root().unwrap().faces.len(), 6, "two steps: a box");
    assert!(ok(m.undo()).as_deref() == Some("Push/pull") && m.root().unwrap().faces.len() == 1, "undo → back to the rectangle");
    assert!(
        ok(m.undo()).as_deref() == Some("Rectangle") && m.root().unwrap().faces.is_empty() && m.root().unwrap().edges.is_empty(),
        "undo → empty"
    );
    assert_eq!(ok(m.undo()), None, "nothing more to undo");
    ok(m.redo());
    ok(m.redo());
    assert!(m.root().unwrap().faces.len() == 6 && close(vol(m.root().unwrap()), 24.0, 1e-6), "redo twice → the box again");
    m.begin();
    let top = faces_facing(m.root().unwrap(), [0.0, 0.0, 1.0])[0];
    for h in [0.5, 1.0, 1.5] {
        ok(m.preview(None, move |mm| Ok(push_pull(root(mm), top, h))));
    }
    ok(m.commit("Push/pull"));
    assert!(close(vol(m.root().unwrap()), 42.0, 1e-6), "a live preview commits only its last state (24 + 12 × 1.5 = 42 m³)");
    ok(m.undo());
    assert!(close(vol(m.root().unwrap()), 24.0, 1e-6), "and undoes in one step");
}

// ---------------------------------------------------------------------------------------------------- components

#[test]
fn components() {
    let mut m = Model::new();
    ok(m.change("col", |mm| {
        let d = root(mm);
        rect(d, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.4, 0.4);
        let f = first_face(d);
        Ok(push_pull(d, f, 3.0))
    }));
    let iid = ok(m.change("Make component", |mm| {
        let sel = connected0(mm.root().unwrap());
        mm.make_group("root", &sel, "component", Some("Column"))
    }))
    .unwrap();
    let def = m.root().unwrap().insts.get(iid).unwrap().def.clone();
    let dd = m.def(&def).unwrap();
    assert!(dd.kind == "component" && dd.name == "Column", "make component: named definition");
    let d2 = def.clone();
    ok(m.change("Place 7 more", move |mm| {
        for k in 1..8 {
            place(root(mm), &d2, &translation([f64::from(k) * 3.0, 0.0, 0.0]));
        }
        Ok(())
    }));
    assert_eq!(m.instance_counts().get(&def).copied(), Some(8.0), "one column placed eight times");
    let d3 = def.clone();
    ok(m.change("Edit column", move |mm| {
        let d = mm.def_mut(&d3).unwrap();
        let top = faces_facing(d, [0.0, 0.0, 1.0])[0];
        Ok(push_pull(d, top, 1.0))
    }));
    let mut vols = Vec::new();
    m.walk("root", &IDENT, &mut |d, _, p, _| {
        if p.len() == 1 {
            vols.push(vol(d));
        }
        true
    });
    assert!(
        vols.len() == 8 && vols.iter().all(|v| close(*v, 0.16 * 4.0, 1e-6)),
        "edit the definition once: all eight columns are 4 m tall"
    );
    let bb = m.bbox("root", &IDENT).unwrap();
    assert!(c6(bb.max[2], 4.0) && c6(bb.max[0], 21.4), "model extents follow (21.4 × 4 m)");
    let one = m.root().unwrap().insts.key_vec()[3];
    ok(m.change("Make unique", move |mm| mm.make_unique("root", one)));
    ok(m.change("Edit one", move |mm| {
        let nd = mm.root().unwrap().insts.get(one).unwrap().def.clone();
        let d = mm.def_mut(&nd).unwrap();
        let top = faces_facing(d, [0.0, 0.0, 1.0])[0];
        Ok(push_pull(d, top, 1.0))
    }));
    assert!(
        c6(m.bbox("root", &IDENT).unwrap().max[2], 5.0) && m.instance_counts().get(&def).copied() == Some(7.0),
        "make unique: editing it leaves the other seven alone"
    );
    ok(m.change("Explode", move |mm| mm.explode("root", one)));
    assert!(m.root().unwrap().faces.len() == 6 && close(vol(m.root().unwrap()), 0.16 * 5.0, 1e-6), "explode: its geometry joins the model");
    assert!(!m.defs.values().any(|d| d.kind == "component" && d.id != def), "purged");
    let j: Value = serde_json::from_str(&m.snapshot()).unwrap();
    let mut m3 = Model::new();
    ok(m3.load(&j, false));
    assert_eq!(m3.snapshot(), m.snapshot(), "native file round trip is exact");
}

// ---------------------------------------------------------------------------------------------------- dimensions

fn set_move(m: &mut Model, id: f64, how: &str) {
    let how = how.to_string();
    ok(m.change("move", move |mm| {
        if let Some(Dim::Lin { mv, .. }) = mm.dims.get_mut(id) {
            *mv = how;
        }
        Ok(())
    }));
}
fn dim_len(m: &Model, id: f64) -> f64 {
    let mut dm = m.dims.get(id).unwrap().clone();
    let e = m.dim_ends(&mut dm);
    dist(e[0], e[1])
}

#[test]
fn dimensions() {
    let mut model = Model::new();
    model.units = "m".into();
    let bb = |m: &Model| m.bbox("root", &IDENT).unwrap();
    ok(model.change("box", |m| {
        let d = root(m);
        rect_hint(d, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 4.0, 3.0, [0.0, 0.0, 1.0]);
        let f = first_face(d);
        Ok(push_pull(d, f, 2.5))
    }));
    assert!(c6(vol(model.root().unwrap()), 30.0), "a 4 × 3 × 2.5 m box");
    // the Dimension tool's two corners and line offset
    let did = ok(model.change("Dimension", |m| Ok(m.add_dim([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, -1.0, 0.0], &[]))));
    let Some(Dim::Lin { a, b, off, .. }) = model.dims.get(did).cloned() else { panic!("a linear dimension") };
    assert!(model.dims.len() == 1 && a.at.is_some() && b.at.is_some(), "two corners → a dimension attached to both vertices");
    assert!(c6(dim_len(&model, did), 4.0) && close(off[1], -1.0, 1e-6), "it measures 4 m, its line 1 m in front");
    ok(model.change("Dimension", move |m| m.set_dim_length(did, 6.0)));
    let s = solid_info(model.root().unwrap());
    assert!(
        c6(bb(&model).max[0], 6.0) && c6(bb(&model).min[0], 0.0) && c6(s.volume.unwrap(), 45.0) && s.closed,
        "typing 6m: the far side moves, 6 m wide, still closed"
    );
    assert!(c6(dim_len(&model, did), 6.0), "the dimension follows the geometry: it reads 6 m");
    ok(model.change("Dimension", move |m| m.set_dim_length(did, 5.0)));
    assert!(c6(bb(&model).max[0], 5.0) && c6(vol(model.root().unwrap()), 37.5), "selected dimension + \"5\" → 5 m wide");
    set_move(&mut model, did, "both");
    ok(model.change("len", move |m| m.set_dim_length(did, 7.0)));
    assert!(c6(bb(&model).min[0], -1.0) && c6(bb(&model).max[0], 6.0), "moving both ends: it grows 1 m each way, centred");
    set_move(&mut model, did, "a");
    ok(model.change("len", move |m| m.set_dim_length(did, 6.0)));
    assert!(c6(bb(&model).min[0], 0.0) && c6(bb(&model).max[0], 6.0), "moving the first end: the near side moves");
    // a height dimension, an edge length, a refusal
    let hid = ok(model.change("h", |m| Ok(m.add_dim([0.0, 0.0, 0.0], [0.0, 0.0, 2.5], [-1.0, 0.0, 0.0], &[]))));
    ok(model.change("h", move |m| m.set_dim_length(hid, 3.0)));
    assert!(c6(bb(&model).max[2], 3.0) && solid_info(model.root().unwrap()).closed, "height dimension → 3 m tall, still closed");
    let d = model.root().unwrap();
    let ey = d
        .edges
        .iter()
        .find(|(_, e)| {
            let (a, b) = (d.v(e.a), d.v(e.b));
            c6(a[0], 0.0) && c6(b[0], 0.0) && c6(a[2], 0.0) && c6(b[2], 0.0)
        })
        .unwrap()
        .0;
    ok(model.change("Length", move |m| m.set_edge_length(&[], ey, 4.0)));
    assert!(
        c6(bb(&model).max[1], 4.0) && c6(bb(&model).min[1], 0.0) && c6(vol(model.root().unwrap()), 6.0 * 4.0 * 3.0),
        "selected edge + \"4\" → the box is 4 m deep"
    );
    let before = model.snapshot();
    let msg = err_msg(model.change("bad", move |m| m.set_dim_length(did, -2.0)));
    assert!(!msg.is_empty() && model.snapshot() == before, "a length of zero or less is refused and nothing changes");
    let free = ok(model.change("free", |m| Ok(m.add_dim([2.0, -5.0, 0.0], [3.0, -5.0, 0.0], [0.0, -1.0, 0.0], &[]))));
    let msg = err_msg(model.change("free", move |m| m.set_dim_length(free, 2.0)));
    let free_a = match model.dims.get(free) {
        Some(Dim::Lin { a, .. }) => a.at.is_none(),
        _ => false,
    };
    assert!(free_a && msg.contains("not on a corner"), "a dimension in empty space measures but refuses to resize");
    // groups: the dimension stays attached; the group's size is editable too
    let gi = ok(model.change("g", |m| {
        let sel = all_faces(m.root().unwrap());
        m.make_group("root", &sel, "group", None)
    }))
    .unwrap();
    let battached = match model.dims.get(did) {
        Some(Dim::Lin { b, .. }) => b.at.as_ref().and_then(|(p, _)| p.first().copied()) == Some(gi),
        _ => false,
    };
    assert!(battached, "after Make group the dimension re-attaches to the vertex inside the group");
    set_move(&mut model, did, "b");
    ok(model.change("len", move |m| m.set_dim_length(did, 8.0)));
    assert!(c6(bb(&model).max[0], 8.0), "resizing through a dimension inside a group stretches the group");
    let gd = model.root().unwrap().insts.get(gi).unwrap().def.clone();
    let gd2 = gd.clone();
    ok(model.change("size", move |m| m.set_def_size(&gd2, 2, 2.0)));
    assert!(c6(bb(&model).max[2], 2.0) && solid_info(model.def(&gd).unwrap()).closed, "group size: height 2 m, still closed");
    // two groups: a dimension between them moves the second group whole
    let g2 = ok(model.change("g2", |m| {
        let id = m.new_def("group", None);
        let d = m.def_mut(&id).unwrap();
        rect_hint(d, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0, 1.0, [0.0, 0.0, 1.0]);
        let f = first_face(d);
        push_pull(d, f, 1.0);
        Ok(place(root(m), &id, &translation([10.0, 0.0, 0.0])))
    }));
    let gap = ok(model.change("gap", |m| Ok(m.add_dim([8.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, -2.0, 0.0], &[]))));
    ok(model.change("gap", move |m| m.set_dim_length(gap, 3.0)));
    assert!(
        c6(model.root().unwrap().insts.get(g2).unwrap().m[12], 11.0) && c6(bb(&model).max[0], 12.0),
        "a dimension between two groups: the second group moves (gap 2 → 3 m)"
    );
    // save, reopen, undo
    let j: Value = serde_json::from_str(&model.snapshot()).unwrap();
    let mut m2 = Model::new();
    ok(m2.load(&j, false));
    assert!(m2.dims.len() == model.dims.len() && c6(dim_len(&m2, did), 8.0), "dimensions are saved in the file and reopen attached");
    let mut bad: Value = serde_json::from_str(&model.snapshot()).unwrap();
    let dims = bad["dims"].as_array_mut().unwrap();
    dims.push(json!({ "id": "x" }));
    dims.push(json!({ "id": 99999, "a": { "p": [0, 0] }, "b": { "p": [1, 1, 1] }, "off": [0, 0, 0] }));
    dims.push(Value::Null);
    let mut m3 = Model::new();
    ok(m3.load(&bad, false));
    assert_eq!(m3.dims.len(), model.dims.len(), "broken dimensions in a file are skipped");
    ok(model.undo());
    assert!(c6(model.root().unwrap().insts.get(g2).unwrap().m[12], 10.0), "undo puts the group back");
}

// ---------------------------------------------------------------------------------------------------- radius, diameter and angle dimensions (0.4 M5)

fn radius_of(m: &Model, cid: &str) -> f64 {
    curve_info(m.root().unwrap(), cid).unwrap().radius
}

#[test]
fn radial_and_angle_dimensions() {
    let mut model = Model::new();
    model.units = "m".into();
    let cid = ok(model.change("Circle", |m| Ok(draw_circle(root(m), [2.0, 2.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 2.0, 24.0))));
    let c2 = cid.clone();
    let id = ok(model.change("Dimension", move |m| m.add_radial_dim(&[], &c2, [3.5, 0.0, 0.0], "dia")));
    let r = match model.dims.get(id).cloned() {
        Some(Dim::Rad { kind, mut c, .. }) => {
            assert_eq!(kind, "dia", "a diameter");
            model.radial_at(&mut c).unwrap()
        }
        _ => panic!("a radial dimension"),
    };
    assert!(close(2.0 * r.r, 4.0, 1e-9) && r.live, "a click places it: Ø 4 m");
    ok(model.change("Dimension", move |m| m.set_dim_length(id, 3.0)));
    let ci = curve_info(model.root().unwrap(), &cid).unwrap();
    assert!(
        close(ci.radius, 1.5, 1e-9) && dist(ci.center.unwrap(), [2.0, 2.0, 0.0]) < 1e-9,
        "typing 3 → the circle is 3 m across, about the same centre"
    );
    assert_eq!(model.root().unwrap().faces.len(), 1, "its disc follows");
    ok(model.change("Dimension", move |m| m.set_dim_length(id, 5.0)));
    assert!(close(radius_of(&model, &cid), 2.5, 1e-9), "selected later, typing 5 → 5 m across");
    ok(model.change("kind", move |m| {
        if let Some(Dim::Rad { kind, .. }) = m.dims.get_mut(id) {
            *kind = "rad".into();
        }
        Ok(())
    }));
    ok(model.change("Dimension", move |m| m.set_dim_length(id, 1.0)));
    assert!(close(radius_of(&model, &cid), 1.0, 1e-9), "shown as a radius, typing 1 sets the radius to 1");
    ok(model.undo());
    assert!(close(radius_of(&model, &cid), 2.5, 1e-9), "undo puts the size back");
    // an arc (the 2-point arc from (6,0,0) to (10,0,0), bulge 1): a radius
    let s = (0.8f64).asin() * 2.0;
    let aid = ok(model.change("Arc", move |m| Ok(draw_arc(root(m), [8.0, -1.5, 0.0], [0.0, 0.0, 1.0], [0.8, 0.6, 0.0], 2.5, s, 12.0))));
    let ar = radius_of(&model, &aid);
    let a2 = aid.clone();
    let rid = ok(model.change("Dimension", move |m| m.add_radial_dim(&[], &a2, [0.0, -3.0, 0.0], "rad")));
    if let Some(Dim::Rad { kind, mut c, .. }) = model.dims.get(rid).cloned() {
        assert!(kind == "rad" && close(model.radial_at(&mut c).unwrap().r, ar, 1e-9), "on an arc: a radius R");
    }
    // saved and opened: the radial dimensions come back on their curves
    let back = reload(&mut model);
    let live: Vec<bool> = back
        .dims
        .values()
        .filter_map(|d| match d {
            Dim::Rad { c, .. } => Some(back.radial_at(&mut c.clone()).is_some_and(|g| g.live)),
            _ => None,
        })
        .collect();
    assert!(live.len() == 2 && live.iter().all(|x| *x), "saved and opened, both still sit on their curves");
    // an angle dimension measures only
    let tri: [P3; 3] = [[0.0, 6.0, 0.0], [6.0, 6.0, 0.0], [6.0, 6.0 + 6.0 * (PI / 6.0).tan(), 0.0]];
    ok(model.change("Line", move |m| {
        let d = root(m);
        line(d, tri[0], tri[1]);
        line(d, tri[1], tri[2]);
        line(d, tri[2], tri[0]);
        Ok(())
    }));
    let ang = ok(model.change("Dimension", move |m| {
        Ok(m.add_angle_dim(tri[0], lerp(tri[0], tri[1], 0.37), lerp(tri[0], tri[2], 0.62), [3.0, 0.6, 0.0], &[]))
    }));
    let msg = err_msg(model.set_dim_length(ang, 2.0));
    assert!(msg.contains("measures only"), "an angle dimension does not resize: {msg}");
    // a pushed circle (a cylinder) measures but cannot resize
    let cyl = ok(model.change("Circle", |m| Ok(draw_circle(root(m), [12.0, 8.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 1.0, 24.0))));
    let fid = last_face(model.root().unwrap());
    ok(model.change("push", move |m| Ok(push_pull(root(m), fid, 2.0))));
    let cyli = curve_info(model.root().unwrap(), &cyl).unwrap();
    let cy2 = cyl.clone();
    let did = ok(model.change("d", move |m| m.add_radial_dim(&[], &cy2, [2.0, 0.0, 0.0], "dia")));
    let msg = err_msg(model.set_dim_length(did, 4.0));
    assert!(!cyli.flat && !msg.is_empty() && close(radius_of(&model, &cyl), 1.0, 1e-9), "a pushed circle keeps its size: {msg}");
    // erased curve: the dimension stays where it was and no longer resizes
    let a3 = aid.clone();
    let lone = ok(model.change("d2", move |m| m.add_radial_dim(&[], &a3, [0.0, -3.0, 0.0], "rad")));
    let a4 = aid.clone();
    ok(model.change("erase arc", move |m| {
        let d = root(m);
        for eid in curve_edges(d, &a4) {
            d.edges.delete(eid);
        }
        Ok(())
    }));
    let live = match model.dims.get(lone).cloned() {
        Some(Dim::Rad { mut c, .. }) => model.radial_at(&mut c).map(|g| g.live),
        _ => None,
    };
    assert_eq!(live, Some(false), "its curve erased, a radius dimension keeps its last place");
}

// ---------------------------------------------------------------------------------------------------- tags (0.4 M6)

fn tag_box(model: &mut Model, x: f64) -> f64 {
    ok(model.change("box", move |m| {
        let d = root(m);
        add_polygon(d, &[[x, 0.0, 0.0], [x + 1.0, 0.0, 0.0], [x + 1.0, 1.0, 0.0], [x, 1.0, 0.0]], Some([0.0, 0.0, 1.0]))?;
        let f = last_face(d);
        push_pull(d, f, 1.0);
        let sel = all_faces_edges(d);
        m.make_group("root", &sel, "group", None)
    }))
    .unwrap()
}

#[test]
fn tags() {
    let mut model = Model::new();
    model.units = "m".into();
    let (g1, g2, g3) = (tag_box(&mut model, 0.0), tag_box(&mut model, 3.0), tag_box(&mut model, 6.0));
    let furn = ok(model.change("add", |m| Ok(m.add_tag("Furniture"))));
    let walls = ok(model.change("add", |m| Ok(m.add_tag("Walls"))));
    let f2 = ok(model.change("add", |m| Ok(m.add_tag("furniture"))));
    assert!(furn == "Furniture" && walls == "Walls" && f2 == "furniture 2", "tags have unique names (case aside)");
    ok(model.change("tag", move |m| m.set_tag("root", &[g1, g2], Some("Furniture"))));
    ok(model.change("tag", move |m| m.set_tag("root", &[g3], Some("Walls"))));
    assert!(tag_count(&model, "Furniture") == 2.0 && tag_count(&model, "Walls") == 1.0 && tag_count(&model, "") == 0.0, "counts per tag");
    ok(model.change("hide", |m| {
        m.set_tag_hidden("Furniture", true);
        Ok(())
    }));
    assert!(model.hidden_tags().contains("Furniture"), "hiding Furniture");
    ok(model.undo());
    assert!(model.hidden_tags().is_empty(), "undo shows it again");
    ok(model.redo());
    ok(model.change("ren", |m| m.rename_tag("Furniture", "Chairs")));
    let ci = model.tag("chairs").unwrap();
    assert!(
        model.root().unwrap().insts.get(g1).unwrap().tag.as_deref() == Some("Chairs") && model.tags[ci].hidden,
        "renaming keeps the placements and the hidden state"
    );
    let msg = err_msg(model.rename_tag("Chairs", "walls"));
    assert!(msg.contains("already a tag called walls"), "a name another tag has is refused: {msg}");
    let back = reload(&mut model);
    assert!(
        back.tags.len() == 3
            && back.tags[back.tag("Chairs").unwrap()].hidden
            && back.root().unwrap().insts.get(g3).unwrap().tag.as_deref() == Some("Walls"),
        "saved in the file: tags, hidden state, each placement's tag"
    );
    // a file naming a tag it does not list
    let mut j: Value = serde_json::from_str(&model.to_json_string(false)).unwrap();
    j.as_object_mut().unwrap().remove("tags");
    let defs = j["defs"].as_array_mut().unwrap();
    let r = defs.iter_mut().find(|d| d["id"] == "root").unwrap();
    r["insts"][0]["tag"] = json!("Doors");
    let mut odd = Model::new();
    ok(odd.load(&j, false));
    assert!(
        odd.tag("Doors").is_some_and(|i| !odd.tags[i].hidden),
        "a placement tagged with a name the list lacks: the tag is added, shown"
    );
    // copy into another model: the tag comes along
    ok(model.change("show", |m| {
        m.set_tag_hidden("Chairs", false);
        Ok(())
    }));
    let clip = ok(model.copy_clip(&[], &[Sel::new("i", g1)])).unwrap();
    let mut other = Model::new();
    other.units = "m".into();
    ok(other.change("paste", move |m| m.paste_clip(&[], &clip, &json!([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]))));
    assert!(
        other.tag("Chairs").is_some() && other.root().unwrap().insts.values().any(|i| i.tag.as_deref() == Some("Chairs")),
        "pasted into another model, a group keeps its tag and the tag is added there"
    );
    // delete: the placements become untagged
    let n = ok(model.change("del", |m| Ok(m.delete_tag("Chairs"))));
    assert!(
        n == 2.0
            && model.tag("Chairs").is_none()
            && tag_count(&model, "") == 2.0
            && model.root().unwrap().insts.get(g1).unwrap().tag.is_none(),
        "deleting a tag untags its 2 placements"
    );
    // nested: grouped, a placement keeps its tag
    let outer = ok(model.change("grp", move |m| m.make_group("root", &[Sel::new("i", g1), Sel::new("i", g3)], "group", None))).unwrap();
    let inner = model.root().unwrap().insts.get(outer).unwrap().def.clone();
    assert!(model.def(&inner).unwrap().insts.values().any(|i| i.tag.as_deref() == Some("Walls")), "grouped, a placement keeps its tag");
}

// ---------------------------------------------------------------------------------------------------- section planes (0.4 M7)

#[test]
fn section_planes() {
    let mut model = Model::new();
    model.units = "m".into();
    ok(model.change("box", |m| {
        let d = root(m);
        add_polygon(d, &[[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [4.0, 3.0, 0.0], [0.0, 3.0, 0.0]], Some([0.0, 0.0, 1.0]))?;
        let f = first_face(d);
        Ok(push_pull(d, f, 2.0))
    }));
    // the Section tool on the front face: a plane there, arrows toward the eye (−green)
    let sid = ok(model.change("Section", |m| Ok(m.add_section([1.3, 0.0, 0.7], [0.0, -1.0, 0.0], ""))));
    let sp = model.sections.get(sid).unwrap().clone();
    assert!(
        sp.p[1].abs() < 1e-9 && dist(sp.n, [0.0, -1.0, 0.0]) < 1e-9 && model.active_section == Some(sid),
        "a plane in the front face, arrows toward you, active"
    );
    assert_eq!(sp.name, "Section 1", "named Section 1");
    ok(model.change("move", move |m| {
        let s = m.sections.get_mut(sid).unwrap();
        s.p = [s.p[0], 1.0, s.p[2]];
        Ok(())
    }));
    ok(model.change("off", |m| {
        m.section_view.cuts = false;
        Ok(())
    }));
    assert!(model.active_cut().is_none(), "Show section cuts off: nothing cuts");
    ok(model.undo());
    assert_eq!(model.active_cut().map(|c| c.0), Some(sid), "undo turns it back on");
    ok(model.change("rev", move |m| {
        let s = m.sections.get_mut(sid).unwrap();
        s.n = mul(s.n, -1.0);
        Ok(())
    }));
    let back = reload(&mut model);
    let bs = back.sections.get(sid).unwrap();
    assert!(
        back.sections.len() == 1 && back.active_section == Some(sid) && bs.name == "Section 1" && dist(bs.n, [0.0, 1.0, 0.0]) < 1e-9,
        "saved in the file: the plane, its name and which one is active"
    );
    ok(model.change("planes", |m| {
        m.section_view.planes = false;
        Ok(())
    }));
    let b2 = reload(&mut model);
    assert!(!b2.section_view.planes && b2.section_view.cuts, "hidden planes stay hidden in the file");
    ok(model.change("planes", |m| {
        m.section_view.planes = true;
        Ok(())
    }));
    let sid2 = ok(model.change("Section", |m| Ok(m.add_section([4.0, 1.5, 1.0], [1.0, 0.0, 0.0], ""))));
    assert!(
        sid2 != sid
            && model.sections.len() == 2
            && model.sections.get(sid2).unwrap().name == "Section 2"
            && model.active_section == Some(sid2),
        "a second plane becomes the active one"
    );
    ok(model.change("del", move |m| {
        m.delete_section(sid2);
        Ok(())
    }));
    assert!(model.active_section.is_none() && model.active_cut().is_none(), "deleting the active plane: nothing cuts");
}

// ---------------------------------------------------------------------------------------------------- drawing axes (0.4 M20)

#[test]
fn drawing_axes() {
    let mut model = Model::new();
    assert!(model.axes_m.is_none(), "a new model draws on its own axes");
    let c45 = std::f64::consts::FRAC_1_SQRT_2;
    ok(model.change("Axes", move |m| m.set_axes([2.0, 1.0, 0.0], [c45, c45, 0.0], [-c45, c45, 0.0])));
    let a = model.axes_m.unwrap();
    assert!(
        dist([a[12], a[13], a[14]], [2.0, 1.0, 0.0]) < 1e-9
            && dist([a[0], a[1], a[2]], [c45, c45, 0.0]) < 1e-9
            && dist([a[4], a[5], a[6]], [-c45, c45, 0.0]) < 1e-9
            && dist([a[8], a[9], a[10]], [0.0, 0.0, 1.0]) < 1e-9,
        "Place axes: the origin at (2, 1) with red turned 45°"
    );
    let back = reload(&mut model);
    let ba = back.axes_m.unwrap();
    assert!(dist([ba[12], ba[13], ba[14]], [2.0, 1.0, 0.0]) < 1e-12, "saved in the file");
    let mut bad: Value = serde_json::from_str(&model.to_json_string(false)).unwrap();
    bad["axes"][0] = json!(5);
    let mut b2 = Model::new();
    ok(b2.load(&bad, false));
    assert!(b2.axes_m.is_none(), "a file with axes that are not a proper frame opens with the model's own axes");
    // align to a face: the front of a box
    let mut m2 = Model::new();
    ok(m2.change("box", |m| {
        let d = root(m);
        add_polygon(d, &[[0.0, 0.0, 0.0], [3.0, 0.0, 0.0], [3.0, 2.0, 0.0], [0.0, 2.0, 0.0]], Some([0.0, 0.0, 1.0]))?;
        let f = first_face(d);
        Ok(push_pull(d, f, 1.0))
    }));
    let d = m2.root().unwrap();
    let front = d.faces.values().find(|f| dist(face_normal(d, f), [0.0, -1.0, 0.0]) < 1e-9).unwrap();
    let lp: Vec<P3> = front.loops[0].iter().map(|&v| d.v(v)).collect();
    let n = lp.len();
    let mut bi = 0;
    for i in 1..n {
        if dist(lp[i], lp[(i + 1) % n]) > dist(lp[bi], lp[(bi + 1) % n]) {
            bi = i;
        }
    }
    let (o, x) = (lp[bi], sub(lp[(bi + 1) % n], lp[bi]));
    ok(m2.change("ax", move |m| m.set_axes(o, x, cross([0.0, -1.0, 0.0], x))));
    let f = m2.axes_frame(&[]);
    assert!(
        dist([f[8], f[9], f[10]], [0.0, -1.0, 0.0]) < 1e-9 && (f[0].abs() - 1.0).abs() < 1e-9,
        "aligned to the front face: blue out of it, red along its 3 m edge"
    );
    let g = ok(m2.change("g", |m| {
        let sel = all_faces(m.root().unwrap());
        m.make_group("root", &sel, "group", None)
    }))
    .unwrap();
    let ctx = m2.context(&[g]).unwrap();
    assert!(m2.axes_frame(&[g]).iter().zip(ctx.m.iter()).all(|(a, b)| (a - b).abs() < 1e-12), "inside an open group its own axes are used");
    ok(m2.change("r", |m| {
        m.reset_axes();
        Ok(())
    }));
    let j: Value = serde_json::from_str(&m2.to_json_string(false)).unwrap();
    assert!(m2.axes_m.is_none() && j.get("axes").is_none(), "Reset axes: back to the model's own, and nothing extra in the file");
    let msg = err_msg(m2.set_axes([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]));
    assert!(msg.contains("must not lie on one line"), "red and green on one line are refused: {msg}");
}

// ---------------------------------------------------------------------------------------------------- outliner (0.4 M22)

fn group_box(model: &mut Model, x: f64) -> f64 {
    ok(model.change("b", move |m| {
        let d = root(m);
        add_polygon(d, &[[x, 0.0, 0.0], [x + 1.0, 0.0, 0.0], [x + 1.0, 1.0, 0.0], [x, 1.0, 0.0]], Some([0.0, 0.0, 1.0]))?;
        let f = last_face(d);
        push_pull(d, f, 1.0);
        let sel = all_faces(d);
        m.make_group("root", &sel, "group", None)
    }))
    .unwrap()
}

#[test]
fn outliner_move_into() {
    let mut model = Model::new();
    let a = group_box(&mut model, 0.0);
    let b = group_box(&mut model, 3.0);
    ok(model.change("turn", move |m| {
        let i = root(m).insts.get_mut(b).unwrap();
        i.m = mmul(&translation([0.5, 2.0, 0.0]), &mmul(&i.m, &rotation([0.0, 0.0, 1.0], PI / 6.0, [3.5, 0.5, 0.0])));
        Ok(())
    }));
    let ia = model.root().unwrap().insts.get(a).unwrap().clone();
    let bb_a = model.bbox(&ia.def, &ia.m).unwrap();
    ok(model.change("tag", move |m| {
        root(m).insts.get_mut(a).unwrap().color = Some("#2e9d57".into());
        Ok(())
    }));
    let nid = ok(model.change("mv", move |m| m.move_into(&[], a, &[b])));
    let bi = model.root().unwrap().insts.get(b).unwrap().clone();
    let ai = model.def(&bi.def).unwrap().insts.get(nid).unwrap().clone();
    let bb_a2 = model.bbox(&ai.def, &mmul(&bi.m, &ai.m)).unwrap();
    assert!(
        !model.root().unwrap().insts.has(a)
            && dist(bb_a.min, bb_a2.min) < 1e-9
            && dist(bb_a.max, bb_a2.max) < 1e-9
            && ai.color.as_deref() == Some("#2e9d57"),
        "a group dragged onto another goes inside it, stays where it was, and keeps its colour"
    );
    let msg = err_msg(model.move_into(&[], b, &[b]));
    assert!(msg.contains("cannot go inside itself"), "onto itself: refused");
    let msg = err_msg(model.move_into(&[b], nid, &[b, nid]));
    assert!(msg.contains("cannot go inside itself"), "into a group inside itself: refused");
    let back = ok(model.change("out", move |m| m.move_into(&[b], nid, &[])));
    let ib = model.root().unwrap().insts.get(back).unwrap().clone();
    let bb_a3 = model.bbox(&ib.def, &ib.m).unwrap();
    assert!(
        dist(bb_a.min, bb_a3.min) < 1e-9 && model.def(&bi.def).unwrap().insts.is_empty(),
        "and back out to the top level, still in the same place"
    );
}

// ---------------------------------------------------------------------------------------------------- scenes remember what is hidden (0.4 M23)

#[test]
fn scenes_remember() {
    let mut model = Model::new();
    let a = group_box(&mut model, 0.0);
    let b = group_box(&mut model, 3.0);
    ok(model.change("tags", move |m| {
        m.add_tag("Roof");
        m.set_tag("root", &[b], Some("Roof"))
    }));
    let sec = ok(model.change("s", |m| Ok(m.add_section([0.0, 0.5, 0.0], [0.0, -1.0, 0.0], ""))));
    ok(model.change("v", |m| -> Res<()> {
        m.active_section = None;
        Ok(())
    }));
    let v1 = model.capture_vis();
    ok(model.change("v", move |m| {
        root(m).insts.get_mut(a).unwrap().hidden = true;
        m.set_tag_hidden("Roof", true);
        m.active_section = Some(sec);
        Ok(())
    }));
    let v2 = model.capture_vis();
    assert!(
        v2.insts.len() == 1
            && v2.insts[0][0] == a
            && v2.tags.join(",") == "Roof"
            && v2.section == Some(sec)
            && v1.insts.is_empty()
            && v1.section.is_none(),
        "a scene captures the hidden group, the hidden tag and the active cut"
    );
    let vis_json = |v: &cc_m3d::model::SceneVis| json!({ "insts": v.insts, "tags": v.tags, "section": v.section, "cuts": v.cuts });
    let sc = clean_scene(&json!({ "name": "Cut", "cam": { "eye": [1, -5, 3], "target": [0, 0, 0] }, "vis": vis_json(&v2) })).unwrap();
    let svis = sc.vis.clone().unwrap();
    assert!(svis.tags[0] == "Roof" && svis.section == Some(sec), "cleanScene keeps it");
    let undo_n = model.undo_stack.len();
    model.apply_vis(&v1);
    assert!(
        !model.root().unwrap().insts.get(a).unwrap().hidden
            && !model.tags[model.tag("Roof").unwrap()].hidden
            && model.active_cut().is_none()
            && model.undo_stack.len() == undo_n,
        "going to scene 1 shows everything again, with no undo step"
    );
    model.apply_vis(&svis);
    assert!(
        model.root().unwrap().insts.get(a).unwrap().hidden
            && model.tags[model.tag("Roof").unwrap()].hidden
            && model.active_cut().map(|c| c.0) == Some(sec),
        "going to scene 2 hides them again and cuts"
    );
    let all = clean_scene(&json!({ "name": "All", "cam": { "eye": [1, -5, 3], "target": [0, 0, 0] }, "vis": vis_json(&v1) })).unwrap();
    ok(model.change("scenes", move |m| {
        m.scenes = vec![all, sc];
        Ok(())
    }));
    let back = reload(&mut model);
    assert!(
        back.scenes[1].vis.as_ref().is_some_and(|v| v.insts[0][0] == a) && back.scenes[0].vis.as_ref().is_some_and(|v| v.insts.is_empty()),
        "saved in the file with the scenes"
    );
    let old = clean_scene(&json!({ "name": "Old", "cam": { "eye": [1, -5, 3], "target": [0, 0, 0] } })).unwrap();
    assert!(old.vis.is_none(), "a scene from an older file has no visibility");
}
