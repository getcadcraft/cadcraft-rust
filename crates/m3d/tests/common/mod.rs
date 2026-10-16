//! Helpers shared by the kernel tests (the frozen bin/test-modelcraft.mjs's own helpers, and the kernel calls that
//! stand in for the tools it drives).
#![allow(dead_code)]

use cc_m3d::def::{Def, face_normal};
use cc_m3d::edges::{EdgeOpts, add_edge};
use cc_m3d::ops::{Sel, add_polygon, arc_pts, connected, ngon_pts, rect_pts, tag_curve};
use cc_m3d::vec::{P3, dot};
use cc_m3d::{Model, Res};

pub fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}
pub fn c6(a: f64, b: f64) -> bool {
    close(a, b, 1e-6)
}
pub fn root(m: &mut Model) -> &mut Def {
    m.root_mut().unwrap()
}
pub fn faces_facing(d: &Def, n: P3) -> Vec<f64> {
    d.faces.iter().filter(|(_, f)| dot(face_normal(d, f), n) > 1.0 - 1e-9).map(|(id, _)| id).collect()
}
pub fn euler(d: &Def) -> i64 {
    d.verts.len() as i64 - d.edges.len() as i64 + d.faces.len() as i64
}
pub fn first_face(d: &Def) -> f64 {
    d.faces.first_key().unwrap()
}
pub fn last_face(d: &Def) -> f64 {
    d.faces.keys().last().unwrap()
}
pub fn rect(d: &mut Def, p: P3, u: P3, v: P3, w: f64, h: f64) -> Vec<f64> {
    add_polygon(d, &rect_pts(p, u, v, w, h), None).unwrap()
}
pub fn rect_hint(d: &mut Def, p: P3, u: P3, v: P3, w: f64, h: f64, hint: P3) -> Vec<f64> {
    add_polygon(d, &rect_pts(p, u, v, w, h), Some(hint)).unwrap()
}
pub fn line(d: &mut Def, p: P3, q: P3) -> Vec<f64> {
    add_edge(d, p, q, EdgeOpts::default())
}
pub fn line_nf(d: &mut Def, p: P3, q: P3) -> Vec<f64> {
    add_edge(d, p, q, EdgeOpts { no_faces: true, hint: None })
}
/// `M.connected(d, [first face])`.
pub fn connected0(d: &Def) -> Vec<Sel> {
    connected(d, &[Sel::new("f", first_face(d))])
}
pub fn all_faces(d: &Def) -> Vec<Sel> {
    d.faces.keys().map(|id| Sel::new("f", id)).collect()
}
pub fn all_faces_edges(d: &Def) -> Vec<Sel> {
    d.faces.keys().map(|id| Sel::new("f", id)).chain(d.edges.keys().map(|id| Sel::new("e", id))).collect()
}
/// The Arc tool: points of the arc, edges between them (hint n), tagged as one arc.
pub fn draw_arc(d: &mut Def, c: P3, n: P3, u: P3, r: f64, sweep: f64, segs: f64) -> String {
    let pts = arc_pts(c, n, u, r, sweep, segs);
    for w in pts.windows(2) {
        add_edge(d, w[0], w[1], EdgeOpts { no_faces: false, hint: Some(n) });
    }
    tag_curve(d, &pts, "arc", false)
}
/// The Circle tool: a polygon of `sides`, closed into a face, tagged as one circle.
pub fn draw_circle(d: &mut Def, c: P3, n: P3, u: P3, r: f64, sides: f64) -> String {
    let pts = ngon_pts(c, n, u, r, sides);
    add_polygon(d, &pts, Some(n)).unwrap();
    tag_curve(d, &pts, "circle", true)
}
/// The curve ids of a kind ('arc', 'circle', 'curve').
pub fn curves(d: &Def, kind: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in d.edges.values() {
        if let Some(c) = &e.curve
            && c.starts_with(&format!("{kind}:"))
            && !out.contains(c)
        {
            out.push(c.clone());
        }
    }
    out
}
pub fn ok<R>(r: Res<R>) -> R {
    r.unwrap()
}
pub fn err_msg<R>(r: Res<R>) -> String {
    match r {
        Ok(_) => String::new(),
        Err(e) => e.to_string(),
    }
}
