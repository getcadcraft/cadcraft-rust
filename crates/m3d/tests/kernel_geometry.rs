//! The geometry sections of ModelCraft's frozen Node test (bin/test-modelcraft.mjs at 594ab3e), the parts that use
//! model.js only. Where the JS drives a tool (tools.js + camera + infer), the same geometry is made with the kernel
//! calls the tool makes; assertions about picking, snapping, overlays and exporters are left out (see the notes).

mod common;

use cc_m3d::Sel;
use cc_m3d::def::*;
use cc_m3d::edges::*;
use cc_m3d::mesh::{intersect_faces, world_triangles};
use cc_m3d::model::{hidden_count, unhide_all};
use cc_m3d::offset::*;
use cc_m3d::ops::*;
use cc_m3d::solids::*;
use cc_m3d::vec::*;
use cc_m3d::{Model, Res};
use common::*;
use std::f64::consts::PI;

fn vol(d: &Def) -> f64 {
    solid_info(d).volume.unwrap_or(f64::NAN)
}
fn ngon_area(r: f64, n: f64) -> f64 {
    0.5 * n * r * r * (2.0 * PI / n).sin()
}

// ---------------------------------------------------------------------------------------------------- edges make faces

#[test]
fn edges_make_faces() {
    let mut m = Model::new();
    let d = root(&mut m);
    line(d, [0.0, 0.0, 0.0], [4.0, 0.0, 0.0]);
    line(d, [4.0, 0.0, 0.0], [4.0, 3.0, 0.0]);
    line(d, [4.0, 3.0, 0.0], [0.0, 3.0, 0.0]);
    assert_eq!(d.faces.len(), 0, "three lines: no face yet");
    line(d, [0.0, 3.0, 0.0], [0.0, 0.0, 0.0]);
    assert_eq!(d.faces.len(), 1, "the fourth line closes the loop: one face");
    let f = d.faces.values().next().unwrap().clone();
    assert!(c6(face_area(d, &f), 12.0), "face area 12 m²");
    assert!(c6(face_normal(d, &f)[2], 1.0), "a ground face faces up");
    line(d, [2.0, 0.0, 0.0], [2.0, 3.0, 0.0]);
    assert_eq!(d.faces.len(), 2, "a line across the face from edge to edge splits it in two");
    assert!(d.faces.values().all(|g| c6(face_area(d, g), 6.0)), "two halves of 6 m²");
    assert_eq!(d.edges.len(), 7, "the split edges: 7 edges");
    line(d, [1.0, 1.5, 0.0], [1.5, 1.5, 0.0]);
    assert_eq!(d.faces.len(), 2, "a dangling line on a face makes no face");
    let er: Vec<f64> = d
        .edges
        .iter()
        .filter(|(_, e)| dist(d.v(e.a), [2.0, 0.0, 0.0]) < 1e-9 || dist(d.v(e.b), [2.0, 0.0, 0.0]) < 1e-9)
        .filter(|(_, e)| (d.v(e.a)[0] - d.v(e.b)[0]).abs() < 1e-9)
        .map(|(id, _)| id)
        .collect();
    erase_edges(d, &er);
    assert!(d.faces.len() == 1 && c6(face_area(d, d.faces.values().next().unwrap()), 12.0), "erasing the splitting edge merges the halves");
}

#[test]
fn holes() {
    let mut m = Model::new();
    let d = root(&mut m);
    rect(d, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 10.0, 10.0);
    rect(d, [3.0, 3.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2.0, 2.0);
    assert_eq!(d.faces.len(), 2, "a rectangle inside a face: two faces");
    let big = d.faces.values().find(|f| f.loops.len() == 2).cloned();
    assert!(big.is_some_and(|b| c6(face_area(d, &b), 96.0)), "the outer face gets a hole (96 m²)");
    add_polygon(d, &ngon_pts([7.0, 7.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 1.0, 24.0), None).unwrap();
    assert_eq!(d.faces.len(), 3, "a circle (24 sides) on the face: another face + hole");
    assert!(d.faces.values().any(|f| f.loops.len() == 3), "the outer face now has two holes");
}

// ---------------------------------------------------------------------------------------------------- push/pull

#[test]
fn push_pull_box() {
    let mut m = Model::new();
    let d = root(&mut m);
    rect(d, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 4.0, 3.0);
    let fid = first_face(d);
    let r = push_pull(d, fid, 2.5);
    assert!(r.ok && r.mode == "extrude", "a lone face extrudes");
    let s = solid_info(d);
    assert!(s.faces == 6.0 && s.edges == 12.0 && s.verts == 8.0, "box: 6 faces, 12 edges, 8 vertices");
    assert!(s.closed, "the box is closed (every edge has two faces)");
    assert!(c6(s.volume.unwrap(), 30.0), "volume 30 m³ with outward faces");
    assert_eq!(euler(d), 2, "Euler V − E + F = 2");
    let top = faces_facing(d, [0.0, 0.0, 1.0]);
    assert_eq!(top.len(), 1, "one face looks up");
    let r2 = push_pull(d, top[0], 1.5);
    assert_eq!(r2.mode, "move", "pulling the top of a box moves it");
    let s = solid_info(d);
    assert!(s.faces == 6.0 && s.closed && c6(s.volume.unwrap(), 48.0), "taller box: 6 faces, volume 48");
    let r3 = push_pull(d, top[0], -1.5);
    assert!(r3.mode == "move" && c6(vol(d), 30.0), "pushing back by 1.5 restores 30 m³");
    let t = faces_facing(d, [0.0, 0.0, 1.0])[0];
    let r4 = push_pull(d, t, -10.0);
    assert!(c6(r4.distance, -2.5), "pushing further than the height stops at zero");
    assert_eq!(d.faces.len(), 0, "pushed back to zero: the solid is gone");
}

#[test]
fn push_pull_opening() {
    let mut m = Model::new();
    let d = root(&mut m);
    rect(d, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 4.0, 1.0);
    let f = first_face(d);
    push_pull(d, f, 3.0);
    assert!(!faces_facing(d, [0.0, -1.0, 0.0]).is_empty(), "the wall has a front face");
    rect(d, [1.0, 0.0, 0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0, 1.5);
    assert_eq!(faces_facing(d, [0.0, -1.0, 0.0]).len(), 2, "a rectangle drawn on the front face splits it (2 faces look forward)");
    let win = faces_facing(d, [0.0, -1.0, 0.0]).into_iter().find(|&id| c6(face_area(d, d.faces.get(id).unwrap()), 1.5));
    assert!(win.is_some(), "the window face is 1.5 m²");
    let r = push_pull(d, win.unwrap(), -5.0);
    assert!(c6(r.distance, -1.0), "pushing in stops at the back face");
    assert!(solid_info(d).closed, "the wall with an opening is still closed");
    assert!(c6(vol(d), 12.0 - 1.5), "volume = 12 − 1.5 m³");
    let back = faces_facing(d, [0.0, 1.0, 0.0]);
    assert!(back.len() == 1 && d.faces.get(back[0]).unwrap().loops.len() == 2, "the back face has a hole");
    let rings: i64 = d.faces.values().map(|f| f.loops.len() as i64 - 1).sum();
    assert_eq!(euler(d) - rings, 0, "a solid with a hole through it: V − E + F − rings = 0 (genus 1)");
}

#[test]
fn push_pull_bump_and_recess() {
    let mut m = Model::new();
    let d = root(&mut m);
    rect(d, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 4.0, 4.0);
    let f = first_face(d);
    push_pull(d, f, 2.0);
    rect(d, [1.0, 1.0, 2.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2.0, 2.0);
    let inner = faces_facing(d, [0.0, 0.0, 1.0]).into_iter().find(|&id| c6(face_area(d, d.faces.get(id).unwrap()), 4.0)).unwrap();
    let r = push_pull(d, inner, 1.0);
    assert!(r.mode == "extrude" && solid_info(d).closed && c6(vol(d), 36.0), "a bump on the top: closed, 32 + 4 = 36 m³");
    let r2 = push_pull(d, r.face.unwrap(), -1.0);
    assert!(r2.mode == "move" && solid_info(d).closed && c6(vol(d), 32.0), "the bump pushed back to zero: 32 m³ again");
    let recess = faces_facing(d, [0.0, 0.0, 1.0]).into_iter().find(|&id| c6(face_area(d, d.faces.get(id).unwrap()), 4.0));
    assert!(recess.is_some(), "inner face still there after push back");
    push_pull(d, recess.unwrap(), -0.5);
    assert!(solid_info(d).closed && c6(vol(d), 30.0), "a recess 0.5 deep: 30 m³");
}

#[test]
fn push_lone_face_down() {
    let mut m = Model::new();
    let d = root(&mut m);
    rect(d, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2.0, 2.0);
    let f = first_face(d);
    let r = push_pull(d, f, -1.0);
    assert!(r.ok && solid_info(d).closed && c6(vol(d), 4.0), "pushing a lone face down makes a box below it");
}

// ---------------------------------------------------------------------------------------------------- arcs and circles (0.4 M3)

#[test]
fn arcs_and_circles() {
    let mut model = Model::new();
    model.units = "m".into();
    // the Arc tool's 2-point half circle on the ground: ends (0,0,0) and (4,0,0), bulge 2 toward +green
    let aid = ok(model.change("Arc", |m| Ok(draw_arc(root(m), [2.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 2.0, PI, 12.0))));
    let d = model.root().unwrap();
    assert_eq!(curves(d, "arc").len(), 1);
    let ci = curve_info(d, &aid).unwrap();
    assert!(
        ci.segs == 12.0
            && ci.round
            && close(ci.radius, 2.0, 1e-9)
            && close(ci.sweep, PI, 1e-9)
            && dist(ci.center.unwrap(), [2.0, 0.0, 0.0]) < 1e-9
            && ci.verts.iter().all(|&v| c6(d.v(v)[2], 0.0)),
        "Arc: two ends, bulge 2 → a half circle of radius 2 in 12 segments on the ground"
    );
    assert!(d.verts.values().map(|p| p[1]).fold(f64::MIN, f64::max) > 1.9, "it bulges toward the pointer (+green)");
    // a line closing it makes a face (half disc)
    ok(model.change("Line", |m| Ok(line(root(m), [0.0, 0.0, 0.0], [4.0, 0.0, 0.0]))));
    let d = model.root().unwrap();
    assert!(
        d.faces.len() == 1 && close(face_area(d, d.faces.values().next().unwrap()), 0.5 * 12.0 * 4.0 * (PI / 12.0).sin(), 1e-9),
        "a line across its ends closes a half-disc face"
    );
    // radius: 2 → 3 (one undo step); the half-disc face follows
    let a2 = aid.clone();
    ok(model.change("Radius", move |m| set_curve_radius(root(m), &a2, 3.0)));
    let ci = curve_info(model.root().unwrap(), &aid).unwrap();
    assert!(
        close(ci.radius, 3.0, 1e-9) && model.root().unwrap().faces.len() == 1,
        "radius 3: the arc grows about its centre; its face follows"
    );
    ok(model.undo());
    assert!(close(curve_info(model.root().unwrap(), &aid).unwrap().radius, 2.0, 1e-9), "one undo puts it back");

    // arc from its centre (centre, radius 2, 90°), then a circle; a pushed circle keeps its radius
    let mut m3 = Model::new();
    m3.units = "m".into();
    let qa = ok(m3.change("Arc", |m| Ok(draw_arc(root(m), [10.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 2.0, PI / 2.0, 12.0))));
    let ci = curve_info(m3.root().unwrap(), &qa).unwrap();
    assert!(
        close(ci.radius, 2.0, 1e-9)
            && close(ci.sweep, PI / 2.0, 1e-9)
            && dist(ci.center.unwrap(), [10.0, 0.0, 0.0]) < 1e-9
            && ci.segs == 12.0,
        "Arc from center: center, radius 2, 90° → a quarter arc"
    );
    let circ = ok(m3.change("Circle", |m| Ok(draw_circle(root(m), [0.0, 5.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 1.0, 24.0))));
    let ci = curve_info(m3.root().unwrap(), &circ).unwrap();
    assert!(ci.closed && ci.segs == 24.0 && close(ci.radius, 1.0, 1e-9) && ci.flat, "Circle: one closed curve of 24 segments, radius 1");
    let c2 = circ.clone();
    ok(m3.change("r", move |m| set_curve_radius(root(m), &c2, 1.5)));
    let d = m3.root().unwrap();
    let disc = d.faces.values().find(|f| f.loops[0].len() == 24).unwrap();
    assert!(close(face_area(d, disc), 2.25 * 0.5 * 24.0 * (2.0 * PI / 24.0).sin(), 1e-9), "its radius 1 → 1.5: the disc grows to 2.25×");
    let dfid = d.faces.iter().find(|(_, f)| f.loops[0].len() == 24).unwrap().0;
    ok(m3.change("push", move |m| Ok(push_pull(root(m), dfid, 1.0))));
    let c3 = circ.clone();
    let e = err_msg(m3.change("r", move |m| set_curve_radius(root(m), &c3, 2.0)));
    assert!(
        !curve_info(m3.root().unwrap(), &circ).unwrap().flat && e.contains("pushed or pulled"),
        "after push/pull its radius can no longer change (says why)"
    );
    // the file keeps the curves; a copy is a curve of its own
    let mut m4 = Model::new();
    let j: serde_json::Value = serde_json::from_str(&m3.to_json_string(false)).unwrap();
    ok(m4.load(&j, false));
    assert!(
        curve_info(m4.root().unwrap(), &qa).map(|c| c.segs) == Some(12.0)
            && curve_info(m4.root().unwrap(), &circ).map(|c| c.segs) == Some(24.0),
        "saved and reopened: the arc and the circle are still curves"
    );
    let before = curves(m3.root().unwrap(), "arc").len() + curves(m3.root().unwrap(), "circle").len();
    let q2 = qa.clone();
    ok(m3.change("copy", move |m| {
        let d = root(m);
        let sel: Vec<Sel> = curve_edges(d, &q2).into_iter().map(|id| Sel::new("e", id)).collect();
        Ok(copy_sel(d, &sel))
    }));
    let after = curves(m3.root().unwrap(), "arc").len() + curves(m3.root().unwrap(), "circle").len();
    assert_eq!(after, before + 1, "a copied arc gets a curve id of its own");
}

// ---------------------------------------------------------------------------------------------------- offset (0.4 M4)

#[test]
fn offset() {
    let mut model = Model::new();
    let top_at = |d: &Def, z: f64| -> Vec<f64> {
        d.faces.iter().filter(|(_, f)| c6(face_normal(d, f)[2], 1.0) && c6(d.v(f.loops[0][0])[2], z)).map(|(id, _)| id).collect()
    };
    // the user's test: a 16:9 screen, its front offset inward for the glass, the glass pushed in
    ok(model.change("screen", |m| {
        let d = root(m);
        rect(d, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.6, 0.9);
        let f = first_face(d);
        Ok(push_pull(d, f, 0.05))
    }));
    let front = top_at(model.root().unwrap(), 0.05)[0];
    ok(model.change("Offset", move |m| offset_face(root(m), front, 0.03)));
    let d = model.root().unwrap();
    let tops = top_at(d, 0.05);
    let glass = tops.iter().copied().find(|&id| {
        let f = d.faces.get(id).unwrap();
        f.loops.len() == 1 && close(face_area(d, f), 1.54 * 0.84, 1e-9)
    });
    assert!(tops.len() == 2 && glass.is_some(), "Offset: 0.03 → a 1.54 × 0.84 glass face inside a 30 mm frame");
    let g = glass.unwrap();
    ok(model.change("Push/pull", move |m| Ok(push_pull(root(m), g, -0.005))));
    let si = solid_info(model.root().unwrap());
    assert!(
        si.closed && close(si.volume.unwrap(), 1.6 * 0.9 * 0.05 - 1.54 * 0.84 * 0.005, 1e-9),
        "…then the glass pushed in 5 mm: a closed screen with a recess"
    );

    // outward: a ring face around a lone rectangle; inward; again; too far
    let mut m2 = Model::new();
    ok(m2.change("r", |m| Ok(rect(root(m), [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 4.0, 3.0))));
    let f0 = first_face(m2.root().unwrap());
    ok(m2.change("Offset", move |m| offset_face(root(m), f0, -0.5)));
    let d = m2.root().unwrap();
    let mut areas: Vec<f64> = d.faces.values().map(|f| face_area(d, f)).collect();
    areas.sort_by(f64::total_cmp);
    assert!(
        areas.len() == 2 && close(areas[0], 5.0 * 4.0 - 12.0, 1e-9) && close(areas[1], 12.0, 1e-9),
        "offset outward 0.5: a ring around it"
    );
    let r = ok(m2.change("Offset", move |m| offset_face(root(m), f0, 0.5)));
    let d = m2.root().unwrap();
    assert!(d.faces.len() == 3 && d.faces.values().any(|f| close(face_area(d, f), 6.0, 1e-9)), "inward 0.5: a 3 × 2 face inside");
    let inner = r.unwrap();
    let r2 = ok(m2.change("Offset", move |m| offset_face(root(m), inner, 0.5)));
    let d = m2.root().unwrap();
    assert!(
        d.faces.len() == 4 && d.faces.values().any(|f| close(face_area(d, f), 2.0, 1e-9)),
        "the last distance again: a 2 × 1 face inside that"
    );
    let inner2 = r2.unwrap();
    let e = err_msg(m2.change("Offset", move |m| offset_face(root(m), inner2, 3.0)));
    assert!(
        e.contains("further than this face allows") && m2.root().unwrap().faces.len() == 4,
        "an offset larger than the face allows is refused with the reason"
    );

    // a circle offsets to a circle
    let mut m3 = Model::new();
    ok(m3.change("Circle", |m| Ok(draw_circle(root(m), [2.0, 1.5, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 1.0, 24.0))));
    let disc = first_face(m3.root().unwrap());
    ok(m3.change("Offset", move |m| offset_face(root(m), disc, 0.25)));
    let d = m3.root().unwrap();
    let cs: Vec<CurveInfo> = curves(d, "circle").iter().map(|c| curve_info(d, c).unwrap()).collect();
    assert!(
        cs.len() == 2 && cs.iter().any(|c| c.round && close(c.radius, 1.0 - 0.25 / (PI / 24.0).cos(), 1e-9)),
        "a circle offsets inward to a circle of its own, 0.25 in from each side"
    );

    // a chain of edges (an L of two lines) offsets to a parallel L
    let mut m4 = Model::new();
    ok(m4.change("L", |m| {
        let d = root(m);
        line(d, [0.0, 0.0, 0.0], [3.0, 0.0, 0.0]);
        line(d, [3.0, 0.0, 0.0], [3.0, 2.0, 0.0]);
        Ok(())
    }));
    ok(m4.change("Offset", |m| {
        let d = root(m);
        let eids = d.edges.key_vec();
        let r = offset_chain_pts(d, &eids, 0.2, [0.0, 0.0, 1.0])?;
        for w in r.pts.windows(2) {
            line(d, w[0], w[1]);
        }
        Ok(())
    }));
    let d = m4.root().unwrap();
    let segs: Vec<(P3, P3)> = d.edges.values().map(|e| (d.v(e.a), d.v(e.b))).collect();
    assert!(
        segs.len() == 4
            && segs.iter().any(|(a, b)| c6(a[1], 0.2) && c6(b[1], 0.2) && c6((a[0] - b[0]).abs(), 2.8))
            && segs.iter().any(|(a, b)| c6(a[0], 2.8) && c6(b[0], 2.8) && c6((a[1] - b[1]).abs(), 1.8)),
        "selected edges (an L): offset 0.2 → a parallel L, corner mitred"
    );
}

// ---------------------------------------------------------------------------------------------------- smooth curves (0.4 M27)

#[test]
fn smooth_curves() {
    let mut model = Model::new();
    let p: Vec<P3> = vec![[0.0, 0.0, 0.0], [2.0, 3.0, 0.0], [5.0, 1.0, 0.0], [8.0, 4.0, 0.0]];
    let passes = |d: &Def, pts: &[P3]| pts.iter().all(|q| d.verts.values().any(|v| dist(*q, *v) < 1e-9));
    let pc = p.clone();
    let cid = ok(model.change("Curve", move |m| add_spline(root(m), &pc, false, SPLINE_SEGS, None)));
    let d = model.root().unwrap();
    let ci = curve_info(d, &cid).unwrap();
    assert!(
        curves(d, "curve").len() == 1 && ci.segs == 24.0 && !ci.closed && passes(d, &p) && d.splines.get(&cid).unwrap().pts.len() == 4,
        "Curve: one smooth curve through every point (24 segments, 8 per span), its points kept"
    );
    let straight: f64 = p.windows(2).map(|w| dist(w[0], w[1])).sum();
    assert!(ci.length > straight, "it is longer than the straight lines between the points");
    // a cable going up (the tool's typed 3D offsets): a 3D curve
    let c2 =
        ok(model.change("Curve", |m| add_spline(root(m), &[[0.0, 6.0, 0.0], [2.0, 6.0, 1.0], [4.0, 7.0, 2.0]], false, SPLINE_SEGS, None)));
    let sp = model.root().unwrap().splines.get(&c2).unwrap();
    assert!(sp.pts.len() >= 3 && sp.pts.iter().map(|q| q[2]).fold(f64::MIN, f64::max) >= 2.0 - 1e-9, "points off the ground: a 3D curve");
    // a closed curve
    let c3 =
        ok(model
            .change("Curve", |m| add_spline(root(m), &[[10.0, 0.0, 0.0], [12.0, 0.0, 0.0], [12.0, 2.0, 0.0]], true, SPLINE_SEGS, None)));
    let d = model.root().unwrap();
    assert!(curve_info(d, &c3).unwrap().closed && d.splines.get(&c3).unwrap().closed, "a closed curve");
    assert_eq!(d.faces.len(), 1, "a closed flat curve makes a face, as any closed outline does");
    // edit: one point moved (one undo step); the curve still passes through all its points
    let pe: Vec<P3> = vec![p[0], p[1], [5.0, -1.0, 0.0], p[3]];
    let (ce, pe2) = (cid.clone(), pe.clone());
    ok(model.change("Edit", move |m| edit_spline(root(m), &ce, &pe2, None, None)));
    let d = model.root().unwrap();
    assert!(
        passes(d, &pe)
            && !passes(d, &[[5.0, 1.0, 0.0]])
            && curve_edges(d, &cid).len() == 24
            && dist(d.splines.get(&cid).unwrap().pts[2], [5.0, -1.0, 0.0]) < 1e-9,
        "Edit points: the curve is redrawn through the new place (same curve)"
    );
    ok(model.undo());
    assert!(passes(model.root().unwrap(), &p), "one undo puts the old curve back");
    // a curve that bounds a face cannot be edited
    let c3b = c3.clone();
    let e = err_msg(
        model.change("e", move |m| edit_spline(root(m), &c3b, &[[10.0, 0.0, 0.0], [12.0, 0.0, 0.0], [12.0, 3.0, 0.0]], None, None)),
    );
    assert!(e.contains("bounds a face"), "a curve that bounds a face is not edited (says why)");
    // moved whole, it stays editable and its points move with it
    let cm = cid.clone();
    ok(model.change("mv", move |m| {
        let d = root(m);
        let sel: Vec<Sel> = curve_edges(d, &cm).into_iter().map(|id| Sel::new("e", id)).collect();
        transform_sel(d, &sel, &translation([0.0, 0.0, 3.0]));
        Ok(())
    }));
    let sp = model.root().unwrap().splines.get(&cid).unwrap();
    assert!(
        sp.pts.iter().zip(p.iter()).all(|(a, b)| dist(*a, add(*b, [0.0, 0.0, 3.0])) < 1e-9),
        "Move the whole curve: its points move with it"
    );
    // file, group, copy
    let mut m2 = Model::new();
    let j: serde_json::Value = serde_json::from_str(&model.to_json_string(false)).unwrap();
    ok(m2.load(&j, false));
    assert!(
        m2.root().unwrap().splines.len() == 3 && m2.root().unwrap().splines.get(&cid).unwrap().pts.len() == 4,
        "saved and reopened: the curves keep their points"
    );
    let cg = cid.clone();
    let gid = ok(model.change("g", move |m| {
        let sel: Vec<Sel> = curve_edges(m.root().unwrap(), &cg).into_iter().map(|id| Sel::new("e", id)).collect();
        m.make_group("root", &sel, "group", None)
    }))
    .unwrap();
    let gdid = model.root().unwrap().insts.get(gid).unwrap().def.clone();
    let gd = model.def(&gdid).unwrap();
    assert!(
        gd.splines.has(&cid) && passes(gd, &gd.splines.get(&cid).unwrap().pts),
        "made into a group: the curve and its points move into the group"
    );
    let cc = c2.clone();
    ok(model.change("c", move |m| {
        let d = root(m);
        let sel: Vec<Sel> = curve_edges(d, &cc).into_iter().map(|id| Sel::new("e", id)).collect();
        Ok(copy_sel(d, &sel))
    }));
    let d = model.root().unwrap();
    assert!(d.splines.len() == 3 && curves(d, "curve").len() == 3, "a copied curve is a curve of its own, editable too");
}

// ---------------------------------------------------------------------------------------------------- follow me (0.4 M10)

#[test]
fn follow_me_sweeps() {
    // a straight pipe
    let mut m = Model::new();
    ok(m.change("pipe", |mm| {
        let d = root(mm);
        add_polygon(d, &ngon_pts([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.1, 24.0), None)?;
        line(d, [0.0, 0.0, 1.0], [3.0, 0.0, 1.0]);
        Ok(())
    }));
    let d = m.root().unwrap();
    let fid = first_face(d);
    let path1: Vec<f64> = d
        .edges
        .iter()
        .filter(|(_, e)| {
            let (a, b) = (d.v(e.a), d.v(e.b));
            a[1].abs() < 1e-12 && b[1].abs() < 1e-12 && (a[2] - 1.0).abs() < 1e-12 && (b[2] - 1.0).abs() < 1e-12
        })
        .map(|(id, _)| id)
        .collect();
    ok(m.change("fm", move |mm| follow_me(root(mm), fid, &path1)));
    let d = m.root().unwrap();
    let si = solid_info(d);
    assert!(
        si.closed && close(si.volume.unwrap(), ngon_area(0.1, 24.0) * 3.0, 1e-9),
        "Follow me: a circle along a straight line → a closed pipe"
    );
    assert_eq!(d.edges.values().filter(|e| e.soft).count(), 24, "the pipe's 24 sides are smooth");

    // a square along an L
    let mut m = Model::new();
    ok(m.change("L", |mm| {
        let d = root(mm);
        rect(d, [0.0, -0.1, -0.1], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 0.2, 0.2);
        line_nf(d, [0.0, 0.0, 0.0], [2.0, 0.0, 0.0]);
        line_nf(d, [2.0, 0.0, 0.0], [2.0, 2.0, 0.0]);
        Ok(())
    }));
    let d = m.root().unwrap();
    let fid = first_face(d);
    let path_l: Vec<f64> = d
        .edges
        .iter()
        .filter(|(_, e)| {
            let (a, b) = (d.v(e.a), d.v(e.b));
            a[2].abs() < 1e-12
                && b[2].abs() < 1e-12
                && a[1].abs() + b[1].abs() + (a[0] - 2.0).abs() + (b[0] - 2.0).abs() > 0.0
                && !(a[0].abs() < 1e-12 && b[0].abs() < 1e-12)
        })
        .map(|(id, _)| id)
        .collect();
    ok(m.change("fm", move |mm| follow_me(root(mm), fid, &path_l)));
    let d = m.root().unwrap();
    let si = solid_info(d);
    assert!(si.closed && close(si.volume.unwrap(), 0.04 * 4.0, 1e-9), "a square along an L: one closed solid (0.16 m³)");
    assert!(
        d.faces.values().all(|f| {
            let pl = face_plane(d, f);
            f.loops[0].iter().all(|&v| (dot(pl.n, d.v(v)) - pl.d).abs() < 1e-9)
        }),
        "every face it made is flat"
    );

    // a closed path: a ring (Pappus)
    let mut m = Model::new();
    ok(m.change("ring", |mm| {
        let d = root(mm);
        let pp = ngon_pts([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 1.0, 48.0);
        for i in 0..48 {
            line_nf(d, pp[i], pp[(i + 1) % 48]);
        }
        add_polygon(d, &ngon_pts([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], 0.1, 12.0), None)?;
        Ok(())
    }));
    let d = m.root().unwrap();
    let fid = first_face(d);
    let ring: Vec<f64> = d
        .edges
        .iter()
        .filter(|(_, e)| (len(d.v(e.a)) - 1.0).abs() < 1e-9 && (len(d.v(e.b)) - 1.0).abs() < 1e-9)
        .map(|(id, _)| id)
        .collect();
    let r1 = ok(m.change("fm", move |mm| follow_me(root(mm), fid, &ring)));
    let d = m.root().unwrap();
    let si = solid_info(d);
    let pappus = 2.0 * PI * ngon_area(0.1, 12.0);
    assert!(
        r1.closed && si.closed && (si.volume.unwrap() - pappus).abs() / pappus < 0.01 && !d.faces.has(fid),
        "a closed path makes a closed ring; the profile face is used up"
    );

    // the user's cable: a circle swept along a smooth curve
    let mut m = Model::new();
    let cp: Vec<P3> = vec![[0.0, 0.0, 0.5], [2.0, 1.0, 0.5], [4.0, -0.5, 1.0], [6.0, 1.0, 0.5]];
    let cp2 = cp.clone();
    ok(m.change("cable", move |mm| {
        let d = root(mm);
        add_spline(d, &cp2, false, SPLINE_SEGS, None)?;
        let s = smooth_pts(&cp2, false, SPLINE_SEGS);
        add_polygon(d, &ngon_pts([0.0, 0.0, 0.5], norm(sub(s[1], [0.0, 0.0, 0.5])), [0.0, 0.0, 1.0], 0.05, 16.0), None)?;
        Ok(())
    }));
    let d = m.root().unwrap();
    let cid = d.splines.keys().next().unwrap().clone();
    let disc = first_face(d);
    let path: Vec<f64> = curve_edges(d, &cid);
    ok(m.change("Follow me", move |mm| follow_me(root(mm), disc, &path)));
    let d = m.root().unwrap();
    let si = solid_info(d);
    assert!(
        si.closed && si.volume.unwrap() > 0.0 && d.edges.values().filter(|e| e.soft).count() > 100,
        "the user's cable: a circle swept along a smooth Curve → a closed, smooth tube"
    );

    // refused: the face lies along the path
    let mut m = Model::new();
    ok(m.change("flat", |mm| {
        let d = root(mm);
        rect(d, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0, 1.0);
        line_nf(d, [0.0, 0.0, 0.0], [-2.0, 0.0, 0.0]);
        Ok(())
    }));
    let e = err_msg(m.change("fm", |mm| {
        let d = root(mm);
        let f = first_face(d);
        let path: Vec<f64> = d
            .edges
            .iter()
            .filter(|(_, e)| d.v(e.a)[0] <= 0.0 && d.v(e.b)[0] <= 0.0 && (d.v(e.a)[0] < 0.0 || d.v(e.b)[0] < 0.0))
            .map(|(id, _)| id)
            .collect();
        follow_me(d, f, &path)
    }));
    assert!(e.contains("lies along the path") && m.root().unwrap().faces.len() == 1, "a face lying along the path is refused, saying why");
}

// ---------------------------------------------------------------------------------------------------- flip (0.4 M15)

fn l_solid(m: &mut Model) {
    ok(m.change("L", |mm| {
        let d = root(mm);
        add_polygon(d, &[[0.0, 0.0, 0.0], [3.0, 0.0, 0.0], [3.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 2.0, 0.0], [0.0, 2.0, 0.0]], None)?;
        let f = first_face(d);
        Ok(push_pull(d, f, 1.0))
    }));
}
fn world_volume(m: &Model) -> f64 {
    world_triangles(m, "root", &IDENT).iter().map(|t| dot(t.p[0], cross(t.p[1], t.p[2])) / 6.0).sum()
}

#[test]
fn flip() {
    let mut m = Model::new();
    l_solid(&mut m);
    let v0 = vol(m.root().unwrap());
    let all = connected0(m.root().unwrap());
    ok(m.change("flip", move |mm| Ok(flip_sel(root(mm), &all, [1.5, 1.0, 0.5], [1.0, 0.0, 0.0], false))));
    let d = m.root().unwrap();
    let si = solid_info(d);
    let xs: Vec<f64> = d.verts.values().map(|p| p[0]).collect();
    assert!(
        si.closed
            && close(si.volume.unwrap(), v0, 1e-9)
            && c6(xs.iter().copied().fold(f64::MAX, f64::min), 0.0)
            && c6(xs.iter().copied().fold(f64::MIN, f64::max), 3.0)
            && d.verts.values().any(|p| c6(p[0], 2.0) && c6(p[1], 2.0)),
        "Flip along red: the L mirrors in place, still a closed solid with the same positive volume"
    );
    // a group: its placement mirrors
    let mut g = Model::new();
    let iid = ok(g.change("g", |mm| {
        let d = root(mm);
        rect(d, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2.0, 1.0);
        let f = first_face(d);
        push_pull(d, f, 1.0);
        let sel = connected0(d);
        mm.make_group("root", &sel, "group", None)
    }))
    .unwrap();
    ok(g.change("flip", move |mm| Ok(flip_sel(root(mm), &[Sel::new("i", iid)], [1.0, 0.5, 0.5], [0.0, 0.0, 1.0], false))));
    let im = g.root().unwrap().insts.get(iid).unwrap().m;
    assert!(
        det3(&im) < 0.0 && c6(im[10], -1.0) && c6(im[14], 1.0),
        "Flip a group along blue: its placement mirrors, it stays where it was"
    );
    assert!(close(world_volume(&g), 2.0, 1e-9), "its faces still face out in world space (volume +2)");
    // a flipped copy beside the original
    let mut c = Model::new();
    l_solid(&mut c);
    let sel = connected0(c.root().unwrap());
    let n = sel.len();
    let out = ok(c.change("fc", move |mm| Ok(flip_sel(root(mm), &sel, [3.0, 1.0, 0.5], [1.0, 0.0, 0.0], true))));
    let xs2 = c.root().unwrap().verts.values().map(|p| p[0]).fold(f64::MIN, f64::max);
    assert!(out.len() == n && c6(xs2, 6.0), "Flipped copy along red: a mirrored copy lands beside the original");
    let mut gc = Model::new();
    l_solid(&mut gc);
    let gi = ok(gc.change("g", |mm| {
        let sel = connected0(mm.root().unwrap());
        mm.make_group("root", &sel, "group", None)
    }))
    .unwrap();
    let out_g = ok(gc.change("fc", move |mm| Ok(flip_sel(root(mm), &[Sel::new("i", gi)], [3.0, 1.0, 0.5], [1.0, 0.0, 0.0], true))));
    assert!(
        out_g.len() == 1
            && gc.root().unwrap().insts.len() == 2
            && close(world_volume(&gc), 2.0 * v0, 1e-9)
            && det3(&gc.root().unwrap().insts.get(out_g[0].id).unwrap().m) < 0.0,
        "a flipped copy of a group: a second, mirrored placement beside it; both face out"
    );
    assert!(det3(&mirror_matrix([1.0, 2.0, 3.0], [0.0, 1.0, 0.0])) < 0.0, "mirrorMatrix is a reflection");
}

// ---------------------------------------------------------------------------------------------------- hide and isolate (0.4 M17)

#[test]
fn hide_and_isolate() {
    let mut m = Model::new();
    let (ga, _gb) = ok(m.change("two", |mm| {
        let mk = |mm: &mut Model, y: f64| -> Res<f64> {
            let d = root(mm);
            rect(d, [0.0, y, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2.0, 0.2);
            let f = last_face(d);
            push_pull(d, f, 1.0);
            let sel = connected0(d);
            Ok(mm.make_group("root", &sel, "group", None)?.unwrap())
        };
        let a = mk(mm, 0.0)?;
        let b = mk(mm, 0.25)?;
        Ok((a, b))
    }));
    ok(m.change("hide", move |mm| {
        hide_sel(root(mm), &[Sel::new("i", ga)], true);
        Ok(())
    }));
    let mut m2 = Model::new();
    let j: serde_json::Value = serde_json::from_str(&m.to_json_string(false)).unwrap();
    ok(m2.load(&j, false));
    assert!(m2.root().unwrap().insts.get(ga).unwrap().hidden && hidden_count(&m2) == 1.0, "saved and reopened: still hidden");
    let n = ok(m.change("u", |mm| Ok(unhide_all(mm))));
    assert!(n == 1.0 && !m.root().unwrap().insts.get(ga).unwrap().hidden, "Unhide all shows it again");
    // isolate
    let mut m3 = Model::new();
    ok(m3.change("b", |mm| {
        let d = root(mm);
        rect(d, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2.0, 1.0);
        let f = first_face(d);
        Ok(push_pull(d, f, 1.0))
    }));
    let top = faces_facing(m3.root().unwrap(), [0.0, 0.0, 1.0])[0];
    let n = ok(m3.change("iso", move |mm| Ok(isolate_sel(root(mm), &[Sel::new("f", top)]))));
    let d = m3.root().unwrap();
    assert!(
        n == 13.0 && d.faces.values().filter(|f| !f.hidden).count() == 1 && d.edges.values().filter(|e| !e.hidden).count() == 4,
        "Isolate the top face: the other 5 faces and 8 edges hide; the top and its 4 edges stay"
    );
    // a hidden edge stays hidden when a line splits it
    let mut m4 = Model::new();
    ok(m4.change("e", |mm| Ok(line_nf(root(mm), [0.0, 0.0, 0.0], [4.0, 0.0, 0.0]))));
    ok(m4.change("h", |mm| {
        let d = root(mm);
        let e = d.edges.first_key().unwrap();
        hide_sel(d, &[Sel::new("e", e)], true);
        Ok(())
    }));
    ok(m4.change("x", |mm| Ok(line_nf(root(mm), [2.0, -1.0, 0.0], [2.0, 1.0, 0.0]))));
    let d = m4.root().unwrap();
    let along: Vec<&Edge> = d.edges.values().filter(|e| c6(d.v(e.a)[1], 0.0) && c6(d.v(e.b)[1], 0.0)).collect();
    assert!(along.len() == 2 && along.iter().all(|e| e.hidden), "a hidden edge split by a line: both pieces stay hidden");
}

// ---------------------------------------------------------------------------------------------------- more geometry

#[test]
fn more_geometry() {
    let mut m = Model::new();
    let d = root(&mut m);
    let l: [P3; 6] = [[0.0, 0.0, 0.0], [6.0, 0.0, 0.0], [6.0, 2.0, 0.0], [2.0, 2.0, 0.0], [2.0, 5.0, 0.0], [0.0, 5.0, 0.0]];
    for i in 0..6 {
        line(d, l[i], l[(i + 1) % 6]);
    }
    assert!(d.faces.len() == 1 && c6(face_area(d, d.faces.values().next().unwrap()), 18.0), "an L of six lines closes into an 18 m² face");
    let f = first_face(d);
    push_pull(d, f, 3.0);
    let s = solid_info(d);
    assert!(s.closed && s.faces == 8.0 && c6(s.volume.unwrap(), 54.0), "pulled L: 8 faces, 54 m³");
    rect(d, [0.0, 0.0, 3.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2.0, 2.0);
    let pad = faces_facing(d, [0.0, 0.0, 1.0]).into_iter().find(|&id| c6(face_area(d, d.faces.get(id).unwrap()), 4.0)).unwrap();
    push_pull(d, pad, 1.0);
    let s2 = solid_info(d);
    assert!(
        s2.closed && c6(s2.volume.unwrap(), 58.0) && faces_facing(d, [0.0, -1.0, 0.0]).len() == 1,
        "a pad on the corner pulled up: closed, 58 m³, its front merged with the wall"
    );
    // move a face: the connected geometry stretches and stays closed
    let mut m4 = Model::new();
    let d4 = root(&mut m4);
    rect(d4, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2.0, 2.0);
    let f = first_face(d4);
    push_pull(d4, f, 2.0);
    let right = faces_facing(d4, [1.0, 0.0, 0.0])[0];
    transform_sel(d4, &[Sel::new("f", right)], &translation([1.0, 0.0, 0.0]));
    let s4 = solid_info(d4);
    assert!(s4.closed && c6(s4.volume.unwrap(), 12.0), "moving a face stretches the box (12 m³)");
    let topf = faces_facing(d4, [0.0, 0.0, 1.0])[0];
    let corner = d4.faces.get(topf).unwrap().loops[0][0];
    let e0 = d4.edges.iter().find(|(_, e)| e.a == corner || e.b == corner).unwrap().0;
    transform_sel(d4, &[Sel::new("e", e0)], &translation([0.0, 0.0, 0.0]));
    assert!(solid_info(d4).closed, "a zero move leaves it closed");
    // lifting one top corner folds the top into two triangles
    let mut m5 = Model::new();
    let d5 = root(&mut m5);
    rect(d5, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2.0, 2.0);
    let f = first_face(d5);
    push_pull(d5, f, 2.0);
    let before = d5.faces.len();
    let cv = d5.faces.get(faces_facing(d5, [0.0, 0.0, 1.0])[0]).unwrap().loops[0][0];
    let vs = cc_m3d::omap::IdSet::from_iter_ids([cv]);
    transform_verts(d5, &vs, &translation([0.0, 0.0, 0.5]));
    let s5 = solid_info(d5);
    assert!(s5.closed && d5.faces.len() == before + 1, "lifting one top corner folds the top into two triangles and stays closed");
    assert!(s5.volume.unwrap() > 8.0 + 0.3, "volume grows by the lifted corner");
}

// ---------------------------------------------------------------------------------------------------- soften and smooth (0.4 M18)

#[test]
fn soften_and_smooth() {
    let mut model = Model::new();
    let ring: Vec<P3> = (0..12).map(|i| [(f64::from(i) * PI / 6.0).cos(), (f64::from(i) * PI / 6.0).sin(), 0.0]).collect();
    ok(model.change("p", move |m| {
        let d = root(m);
        add_polygon(d, &ring, Some([0.0, 0.0, 1.0]))?;
        let f = first_face(d);
        push_pull(d, f, 2.0);
        for e in d.edges.values_mut() {
            e.soft = false;
        }
        Ok(())
    }));
    let all = model.root().unwrap().edges.key_vec();
    let soft_n = |m: &Model| m.root().unwrap().edges.values().filter(|e| e.soft).count();
    let a1 = all.clone();
    let n = ok(model.change("s", move |m| Ok(soften_edges(root(m), &a1, 20.0))));
    assert!(n == 0.0 && soft_n(&model) == 0, "soften up to 20°: the sides meet at 30°, so nothing changes");
    let a2 = all.clone();
    let n = ok(model.change("s", move |m| Ok(soften_edges(root(m), &a2, 31.0))));
    assert!(n == 12.0 && soft_n(&model) == 12, "soften up to 31°: the 12 side edges go soft, the 24 rim edges stay hard");
    let d = model.root().unwrap();
    let sm = smooth_normals(d);
    let (sid, side) = d.faces.iter().find(|(_, f)| face_normal(d, f)[2].abs() < 1e-9).unwrap();
    let corner = side.loops[0][0];
    let q = *sm.get(&format!("{}|{}", cc_m3d::js::num_str(sid), cc_m3d::js::num_str(corner))).unwrap();
    let p = d.v(corner);
    assert!(
        close(q[0].hypot(q[1]), 1.0, 1e-9) && q[2].abs() < 1e-9 && close((q[0] * p[0] + q[1] * p[1]) / p[0].hypot(p[1]), 1.0, 1e-9),
        "a side face's corner normal points straight out from the axis"
    );
    let (tid, top) = d.faces.iter().find(|(_, f)| face_normal(d, f)[2] > 0.9).unwrap();
    assert!(
        !sm.has(&format!("{}|{}", cc_m3d::js::num_str(tid), cc_m3d::js::num_str(top.loops[0][0]))),
        "the caps stay flat (no soft edge joins them)"
    );
    let a3 = all.clone();
    let n = ok(model.change("h", move |m| Ok(soften_edges(root(m), &a3, 0.0))));
    assert!(
        n == 12.0 && soft_n(&model) == 0 && smooth_normals(model.root().unwrap()).is_empty(),
        "Harden all: every edge hard, flat again"
    );
    ok(model.undo());
    assert_eq!(soft_n(&model), 12, "undo softens them again");
}

// ---------------------------------------------------------------------------------------------------- intersect faces (0.4 M19)

fn prism(d: &mut Def, o: P3, w: f64, dd: f64, h: f64) {
    let before = d.faces.key_vec();
    add_polygon(
        d,
        &[[o[0], o[1], o[2]], [o[0] + w, o[1], o[2]], [o[0] + w, o[1] + dd, o[2]], [o[0], o[1] + dd, o[2]]],
        Some([0.0, 0.0, 1.0]),
    )
    .unwrap();
    let f = d.faces.keys().find(|id| !before.contains(id)).unwrap();
    push_pull(d, f, h);
}

#[test]
fn intersect() {
    let mut model = Model::new();
    let wall = ok(model.change("w", |m| {
        prism(root(m), [0.0, 0.0, 0.0], 4.0, 0.2, 3.0);
        let sel = all_faces_edges(m.root().unwrap());
        m.make_group("root", &sel, "group", None)
    }))
    .unwrap();
    let beam = ok(model.change("b", |m| {
        prism(root(m), [1.5, -1.0, 1.0], 1.0, 2.2, 1.0);
        let sel = all_faces_edges(m.root().unwrap());
        m.make_group("root", &sel, "group", None)
    }))
    .unwrap();
    let wdef = model.root().unwrap().insts.get(wall).unwrap().def.clone();
    let f0 = model.def(&wdef).unwrap().faces.len();
    let wsel = all_faces(model.def(&wdef).unwrap());
    let n = ok(model.change("x", move |m| Ok(intersect_faces(m, &[wall], &wsel, "model"))));
    let wd = model.def(&wdef).unwrap();
    assert!(n == 8.0 && wd.faces.len() == f0 + 2, "inside the wall, Intersect with model draws the beam's outline: 8 edges, 2 new faces");
    assert_eq!(wd.faces.values().filter(|f| close(face_area(wd, f), 1.0, 1e-9)).count(), 2, "the two new faces are 1 × 1 m squares");
    ok(model.undo());
    assert_eq!(model.def(&wdef).unwrap().faces.len(), f0, "one undo step");
    let n2 = ok(model.change("x", move |m| Ok(intersect_faces(m, &[], &[Sel::new("i", wall), Sel::new("i", beam)], "selection"))));
    let r = model.root().unwrap();
    assert!(
        n2 == 8.0 && r.edges.len() == 8 && r.faces.len() == 2 && model.def(&wdef).unwrap().faces.len() == f0,
        "two groups selected, Intersect with selection: the 8 lines are drawn in the model, the groups untouched"
    );
    ok(model.undo());
    // a loose box sunk into a slab group
    let mut m2 = Model::new();
    ok(m2.change("s", |m| {
        prism(root(m), [0.0, 0.0, 0.0], 4.0, 4.0, 0.5);
        Ok(())
    }));
    let slab = m2.root().unwrap().faces.len();
    ok(m2.change("c", |m| {
        let sel = all_faces_edges(m.root().unwrap());
        m.make_group("root", &sel, "group", None)
    }));
    let slab_i = m2.root().unwrap().insts.first_key().unwrap();
    ok(m2.change("box", |m| {
        prism(root(m), [1.0, 1.0, 0.25], 1.0, 1.0, 1.0);
        Ok(())
    }));
    let n3 = ok(m2.change("x", |m| {
        let sel = all_faces(m.root().unwrap());
        Ok(intersect_faces(m, &[], &sel, "model"))
    }));
    let d = m2.root().unwrap();
    let sides = d.faces.values().filter(|f| face_normal(d, f)[2].abs() < 1e-9).count();
    assert!(n3 == 8.0 && sides == 8, "a loose box sunk into a slab: its 4 sides are split where they meet the slab's top");
    let sd = m2.root().unwrap().insts.get(slab_i).unwrap().def.clone();
    assert!(slab == 6 && m2.def(&sd).unwrap().faces.len() == 6, "the slab group itself is not changed");
    // parallel faces never cross
    let mut m3 = Model::new();
    ok(m3.change("p", |m| {
        prism(root(m), [0.0, 0.0, 0.0], 1.0, 1.0, 1.0);
        prism(root(m), [3.0, 0.0, 0.0], 1.0, 1.0, 1.0);
        Ok(())
    }));
    let n = ok(m3.change("x", |m| {
        let sel = all_faces(m.root().unwrap());
        Ok(intersect_faces(m, &[], &sel, "model"))
    }));
    assert_eq!(n, 0.0, "boxes apart: nothing to draw");
}

// ---------------------------------------------------------------------------------------------------- solid checker (0.4 M21)

fn box_model() -> Model {
    let mut m = Model::new();
    ok(m.change("b", |mm| {
        let d = root(mm);
        add_polygon(d, &[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 2.0, 0.0], [0.0, 2.0, 0.0]], Some([0.0, 0.0, 1.0]))?;
        let f = first_face(d);
        Ok(push_pull(d, f, 2.0))
    }));
    m
}

#[test]
fn solid_checker() {
    let mut m = box_model();
    let d = root(&mut m);
    let p = solid_problems(d);
    assert!(
        p.holes.is_empty()
            && p.extra.is_empty()
            && p.inner.is_empty()
            && p.stray.is_empty()
            && p.flipped.is_empty()
            && close(vol(d), 8.0, 1e-9),
        "a pushed box has no problems (8 m³)"
    );
    let top = d.faces.iter().find(|(_, f)| face_normal(d, f)[2] > 0.9).unwrap().0;
    d.faces.delete(top);
    let p = solid_problems(d);
    assert!(p.holes.len() == 4 && !solid_info(d).closed, "top face erased: 4 edges border the hole");
    let r = fix_solid(d);
    assert!(r.holes == 1.0 && solid_info(d).closed && close(vol(d), 8.0, 1e-9), "Fix closes the flat hole with a face facing out");
    // one face turned the wrong way
    let mut m = box_model();
    let d = root(&mut m);
    let side = d.faces.key_vec()[2];
    reverse_faces(d, &[side]);
    let p = solid_problems(d);
    assert!(p.flipped.len() == 1 && p.flipped[0] == side, "a face turned the other way is found");
    let r = fix_solid(d);
    assert!(r.flipped == 1.0 && solid_problems(d).flipped.is_empty() && close(vol(d), 8.0, 1e-9), "Fix turns it back");
    // the whole box inside out
    let mut m = box_model();
    let d = root(&mut m);
    let all = d.faces.key_vec();
    reverse_faces(d, &all);
    assert!(
        close(solid_info(d).volume.unwrap(), -8.0, 1e-9) && solid_problems(d).flipped.len() == 6,
        "a box inside out: all 6 faces need turning"
    );
    fix_solid(d);
    assert!(close(vol(d), 8.0, 1e-9), "Fix turns it right way out (+8)");
    // a stray edge
    let mut m = box_model();
    let d = root(&mut m);
    line(d, [3.0, 3.0, 0.0], [4.0, 3.0, 0.0]);
    assert_eq!(solid_problems(d).stray.len(), 1, "a loose edge beside it is a stray edge");
    let r = fix_solid(d);
    assert!(r.stray == 1.0 && d.edges.len() == 12, "Fix removes it");
    // a wall inside the box
    let mut m = box_model();
    let d = root(&mut m);
    add_polygon(d, &[[1.0, 0.0, 0.0], [1.0, 2.0, 0.0], [1.0, 2.0, 2.0], [1.0, 0.0, 2.0]], Some([1.0, 0.0, 0.0])).unwrap();
    let p = solid_problems(d);
    assert!(
        p.inner.len() == 1 && p.extra.len() == 4 && !solid_info(d).closed,
        "a wall inside the box: 1 inner face, 4 edges with three faces"
    );
    let r = fix_solid(d);
    assert!(r.inner == 1.0 && solid_info(d).closed && close(vol(d), 8.0, 1e-9), "Fix removes the inner face: a solid again");
}

// ---------------------------------------------------------------------------------------------------- push/pull to a face's level (0.5 DR.4)

#[test]
fn push_pull_to_a_faces_level() {
    // tangentsoft's model: a box, a circle on top pulled up into a post, its top offset inward, the inner face pushed in
    // a little; then that innermost face pushed down to the box's level (what the Push/pull tool does when the cursor
    // points at the box's top: a push by the distance between the two planes)
    let mut model = Model::new();
    model.units = "m".into();
    let up_at = |d: &Def, z: f64| -> Vec<(f64, usize, f64)> {
        d.faces
            .iter()
            .filter(|(_, f)| close(face_normal(d, f)[2], 1.0, 1e-9) && close(d.v(f.loops[0][0])[2], z, 1e-9))
            .map(|(id, f)| (id, f.loops.len(), face_area(d, f)))
            .collect()
    };
    let flat = |d: &Def| d.faces.values().filter(|f| face_area(d, f) < 1e-9).count();
    ok(model.change("Rectangle", |m| Ok(rect(root(m), [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2.0, 2.0))));
    ok(model.change("Push/pull", |m| {
        let d = root(m);
        let f = first_face(d);
        Ok(push_pull(d, f, 2.0))
    }));
    ok(model.change("Circle", |m| Ok(draw_circle(root(m), [1.0, 1.0, 2.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 0.5, 24.0))));
    let disc = up_at(model.root().unwrap(), 2.0).into_iter().find(|x| x.1 == 1).unwrap().0;
    ok(model.change("Push/pull", move |m| Ok(push_pull(root(m), disc, 1.0))));
    let post_top = up_at(model.root().unwrap(), 3.0)[0].0;
    ok(model.change("Offset", move |m| offset_face(root(m), post_top, 0.1)));
    let ups = up_at(model.root().unwrap(), 3.0);
    let inner = ups.iter().find(|x| x.1 == 1).copied();
    let ring = ups.iter().find(|x| x.1 == 2).copied();
    let top0 = 0.5 * 24.0 * 0.5 * 0.5 * (2.0 * PI / 24.0).sin();
    assert!(
        inner.is_some_and(|i| ring.is_some_and(|r| i.2 < top0 - 0.2 && close(i.2 + r.2, top0, 1e-9))),
        "the post's top offset 0.1 inward: an inner face inside a ring"
    );
    let (iid, _, inner_a) = inner.unwrap();
    let ring_a = ring.unwrap().2;
    ok(model.change("recess", move |m| Ok(push_pull(root(m), iid, -0.3))));
    let v0 = vol(model.root().unwrap());
    assert_eq!(up_at(model.root().unwrap(), 2.7).len(), 1, "the inner face pushed in 0.3 (a recess in the post)");
    let low_id = up_at(model.root().unwrap(), 2.7)[0].0;
    ok(model.change("Push/pull", move |m| Ok(push_pull(root(m), low_id, -0.7))));
    let d = model.root().unwrap();
    let s = solid_info(d);
    let low: Vec<_> = up_at(d, 2.0).into_iter().filter(|x| x.1 == 1).collect();
    assert!(low.len() == 1 && close(low[0].2, inner_a, 1e-9), "the inner face is level with the box's top (z = 2 exactly)");
    assert!(
        s.closed && close(s.volume.unwrap(), v0 - 0.7 * inner_a, 1e-9) && flat(d) == 0,
        "a closed solid, no zero-height sides left over"
    );
    assert!(close(s.volume.unwrap(), 8.0 + ring_a, 1e-9), "box + a tube of the ring's area, 1 m tall");
    let sv = s.volume.unwrap();
    // pull it back up to the ring's level: flush again
    let lid = low[0].0;
    ok(model.change("Push/pull", move |m| Ok(push_pull(root(m), lid, 1.0))));
    let d = model.root().unwrap();
    let s = solid_info(d);
    assert!(
        s.closed && flat(d) == 0 && up_at(d, 3.0).len() == 2 && close(s.volume.unwrap(), 8.0 + (ring_a + inner_a), 1e-9),
        "flush with the ring again: closed, no zero-height sides, the ring and the inner face"
    );
    assert!(sv < s.volume.unwrap());
    // typed distances are unchanged: a typed 0.5 goes 0.5
    let top_inner = up_at(model.root().unwrap(), 3.0).into_iter().find(|x| x.1 == 1).unwrap().0;
    ok(model.change("Push/pull", move |m| Ok(push_pull(root(m), top_inner, -0.5))));
    assert_eq!(up_at(model.root().unwrap(), 2.5).len(), 1, "typed 0.5: pushed 0.5");
}
