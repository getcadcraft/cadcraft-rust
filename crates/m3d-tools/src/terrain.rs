//! Terrain (terrain.js, one for one): Delaunay triangulation (Bowyer–Watson), terrain from contour edges, and a flat
//! grid of triangles to sculpt. Both return the new group definition's id (call inside `Model::change`).

#![deny(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::collections::HashMap;

use cc_m3d::def::{Def, edge_faces, ef_get};
use cc_m3d::mesh::mesh_def;
use cc_m3d::model::Model;
use cc_m3d::vec::P3;
use serde_json::Value;

use crate::js::{jmax, jmin, num_str, round};

/// Most points a terrain is made from (contours are thinned evenly above that).
pub const MAX_POINTS: usize = 8000;

#[derive(Clone, Copy, Debug)]
struct Circ {
    x: f64,
    y: f64,
    r2: f64,
}

/// `delaunay(pts)` → counter-clockwise triangles (indices into pts); duplicates dropped.
pub fn delaunay(pts: &[[f64; 2]]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in pts {
        x0 = jmin(x0, p[0]);
        y0 = jmin(y0, p[1]);
        x1 = jmax(x1, p[0]);
        y1 = jmax(y1, p[1]);
    }
    let d = crate::js::or(jmax(x1 - x0, y1 - y0), 1.0);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let mut pp: Vec<[f64; 2]> = pts.to_vec();
    pp.extend([[cx - 20.0 * d, cy - d], [cx, cy + 20.0 * d], [cx + 20.0 * d, cy - d]]);
    let at = |i: usize| pp.get(i).copied().unwrap_or([f64::NAN; 2]);
    let circ = |a: usize, b: usize, c: usize| -> Circ {
        let ([ax, ay], [bx, by], [qx, qy]) = (at(a), at(b), at(c));
        let dd = 2.0 * (ax * (by - qy) + bx * (qy - ay) + qx * (ay - by));
        if dd.abs() < 1e-18 {
            return Circ { x: 0.0, y: 0.0, r2: f64::INFINITY };
        }
        let (a2, b2, c2) = (ax * ax + ay * ay, bx * bx + by * by, qx * qx + qy * qy);
        let x = (a2 * (by - qy) + b2 * (qy - ay) + c2 * (ay - by)) / dd;
        let y = (a2 * (qx - bx) + b2 * (ax - qx) + c2 * (bx - ax)) / dd;
        let (ex, ey) = (ax - x, ay - y);
        Circ { x, y, r2: ex * ex + ey * ey }
    };
    let (s0, s1, s2) = (n, n.saturating_add(1), n.saturating_add(2));
    let mut tris: Vec<([usize; 3], Circ)> = vec![([s0, s1, s2], circ(s0, s1, s2))];
    for i in 0..n {
        let [px, py] = at(i);
        let mut bad = Vec::new();
        let mut keep = Vec::new();
        for tr in tris {
            let (ex, ey) = (px - tr.1.x, py - tr.1.y);
            if ex * ex + ey * ey < tr.1.r2 * (1.0 - 1e-12) {
                bad.push(tr);
            } else {
                keep.push(tr);
            }
        }
        // the hole's rim: edges of the bad triangles that only one of them has
        let mut count: HashMap<(usize, usize), u32> = HashMap::new();
        let ekey = |a: usize, b: usize| if a < b { (a, b) } else { (b, a) };
        let sides = |v: &[usize; 3]| [(v[0], v[1]), (v[1], v[2]), (v[2], v[0])];
        for tr in &bad {
            for (a, b) in sides(&tr.0) {
                let c = count.entry(ekey(a, b)).or_insert(0);
                *c = c.saturating_add(1);
            }
        }
        tris = keep;
        for tr in &bad {
            for (a, b) in sides(&tr.0) {
                if count.get(&ekey(a, b)) == Some(&1) {
                    tris.push(([a, b, i], circ(a, b, i)));
                }
            }
        }
    }
    let mut out = Vec::new();
    for (v, _) in tris {
        if v.iter().any(|k| *k >= n) {
            continue;
        }
        let [a, b, c] = v;
        let ([ax, ay], [bx, by], [qx, qy]) = (at(a), at(b), at(c));
        let cr = (bx - ax) * (qy - ay) - (by - ay) * (qx - ax);
        if cr.abs() < 1e-14 {
            continue;
        }
        out.push(if cr > 0.0 { [a, b, c] } else { [a, c, b] });
    }
    out
}

/// Soften every edge between two faces (the inside of a terrain).
fn soft_inside(def: &mut Def) {
    let ef = edge_faces(def);
    let soft: Vec<f64> = def.edges.iter().filter(|(_, e)| ef_get(&ef, e.a, e.b).len() == 2).map(|(id, _)| id).collect();
    for id in soft {
        if let Some(e) = def.edges.get_mut(id) {
            e.soft = true;
        }
    }
}

fn poly_value(pts: &[P3]) -> Value {
    Value::Array(pts.iter().map(|p| Value::Array(p.iter().map(|x| cc_m3d::file::jn(*x)).collect())).collect())
}

/// `terrainFromContours(model, def, eids, {name})` → the new definition's id, or None (fewer than 3 points).
pub fn terrain_from_contours(model: &mut Model, def_id: &str, eids: &[f64], name: &str) -> Option<String> {
    let def = model.def(def_id)?;
    let mut keys: Vec<String> = Vec::new();
    let mut seen: HashMap<String, P3> = HashMap::new();
    for eid in eids {
        let Some(e) = def.edges.get(*eid) else { continue };
        for v in [e.a, e.b] {
            let p = def.v(v);
            let k = format!("{},{}", num_str(round(p[0] * 100.0)), num_str(round(p[1] * 100.0)));
            if !seen.contains_key(&k) {
                seen.insert(k.clone(), p);
                keys.push(k);
            }
        }
    }
    let mut pts: Vec<P3> = keys.iter().filter_map(|k| seen.get(k).copied()).collect();
    if pts.len() > MAX_POINTS {
        let step = pts.len() as f64 / MAX_POINTS as f64;
        pts = (0..MAX_POINTS)
            .map(|i| {
                let j = (i as f64 * step).floor();
                // Math.floor(i * step) < pts.length always
                let j = crate::js::usize_of(j);
                pts.get(j).copied().unwrap_or([f64::NAN; 3])
            })
            .collect();
    }
    let tris = delaunay(&pts.iter().map(|p| [p[0], p[1]]).collect::<Vec<_>>());
    if tris.is_empty() {
        return None;
    }
    let polys: Vec<Value> = tris.iter().map(|tr| poly_value(&tr.map(|i| pts.get(i).copied().unwrap_or([f64::NAN; 3])))).collect();
    let uname = model.unique_name(name);
    let id = mesh_def(model, &polys, Some(&uname), "group", &[]);
    if let Some(d) = model.def_mut(&id) {
        soft_inside(d);
    }
    Some(id)
}

/// `terrainGrid(model, {width, depth, spacing, origin, name})` → the new definition's id.
pub fn terrain_grid(model: &mut Model, width: f64, depth: f64, spacing: f64, origin: P3, name: &str) -> String {
    let cells = |len: f64| {
        let n = jmax(1.0, jmin(200.0, round(len / spacing)));
        // NaN (a spacing of 0 for a length of 0) reads as 1 cell
        if n.is_finite() && n >= 1.0 { crate::js::usize_of(n) } else { 1 }
    };
    let (nx, ny) = (cells(width), cells(depth));
    let (fx, fy) = (nx as f64, ny as f64);
    let pnt = |i: usize, j: usize| [origin[0] + (width * i as f64) / fx, origin[1] + (depth * j as f64) / fy, origin[2]];
    let mut polys = Vec::new();
    for i in 0..nx {
        for j in 0..ny {
            let (i1, j1) = (i.saturating_add(1), j.saturating_add(1));
            let (a, b, c, e) = (pnt(i, j), pnt(i1, j), pnt(i1, j1), pnt(i, j1));
            if !i.wrapping_add(j).is_multiple_of(2) {
                polys.push(poly_value(&[a, b, c]));
                polys.push(poly_value(&[a, c, e]));
            } else {
                polys.push(poly_value(&[a, b, e]));
                polys.push(poly_value(&[b, c, e]));
            }
        }
    }
    let uname = model.unique_name(name);
    let id = mesh_def(model, &polys, Some(&uname), "group", &[]);
    if let Some(d) = model.def_mut(&id) {
        soft_inside(d);
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn square_and_centre() {
        let sq = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [1.0, 1.0]];
        let dt = delaunay(&sq);
        assert_eq!(dt.len(), 4);
    }
}
