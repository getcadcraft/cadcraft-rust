//! Solids (0.4 M21): why a definition is not a solid, fixing the simple cases, and its volume.

use crate::def::*;
use crate::edges::{reface, remove_loose_verts};
use crate::js;
use crate::omap::{FxMap, IdMap, IdSet, kb};
use crate::ops::reverse_faces;
use crate::vec::*;

/// Why a definition is not a solid: {holes (edges with one face), extra (three faces or more), inner (faces inside
/// the shape), stray (edges with no face), flipped (faces turned the other way)}. All empty: a solid.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Problems {
    pub holes: Vec<f64>,
    pub extra: Vec<f64>,
    pub inner: Vec<f64>,
    pub stray: Vec<f64>,
    pub flipped: Vec<f64>,
}

fn signed_volume(def: &Def, f: &Face) -> f64 {
    let mut v = 0.0;
    for [a, b, c] in face_tris(def, f) {
        v += dot(def.v(a), cross(def.v(b), def.v(c))) / 6.0;
    }
    v
}

pub fn solid_problems(def: &Def) -> Problems {
    let ef = edge_faces(def);
    let mut p = Problems::default();
    for (eid, e) in def.edges.iter() {
        let n = ef_get(&ef, e.a, e.b).len();
        if n == 0 {
            p.stray.push(eid);
        } else if n == 1 {
            p.holes.push(eid);
        } else if n > 2 {
            p.extra.push(eid);
        }
    }
    let mut inner = IdSet::new();
    for (id, f) in def.faces.iter() {
        let mut all = true;
        for_segs(f, |a, b| {
            if ef_get(&ef, a, b).len() <= 2 {
                all = false;
            }
        });
        if all {
            inner.add(id);
        }
    }
    // orientation: two faces across an edge must run along it in opposite directions; flood each piece from one face
    let runs = |fid: f64, a: f64, b: f64| def.faces.get(fid).is_some_and(|g| loop_segs(g).iter().any(|&(x, y)| x == a && y == b));
    let mut sign: FxMap<u64, f64> = FxMap::default();
    let sg = |sign: &FxMap<u64, f64>, f: f64| sign.get(&kb(f)).copied().unwrap_or(f64::NAN);
    for start in def.faces.keys() {
        if sign.contains_key(&kb(start)) || inner.has(start) {
            continue;
        }
        sign.insert(kb(start), 1.0);
        let mut piece = vec![start];
        let mut stack = vec![start];
        let mut open = false;
        while let Some(f) = stack.pop() {
            let Some(face) = def.faces.get(f) else { continue };
            for (a, b) in loop_segs(face) {
                let fs: Vec<f64> = ef_get(&ef, a, b).iter().copied().filter(|g| !inner.has(*g)).collect();
                let [f0, f1] = fs.as_slice() else {
                    open = true;
                    continue;
                };
                let g = if *f0 == f { *f1 } else { *f0 };
                // g agrees with f when it runs b→a; runs a→b too means one of them is turned
                let s = if runs(g, a, b) { -sg(&sign, f) } else { sg(&sign, f) };
                if let std::collections::hash_map::Entry::Vacant(e) = sign.entry(kb(g)) {
                    e.insert(s);
                    piece.push(g);
                    stack.push(g);
                }
            }
        }
        let plus: Vec<f64> = piece.iter().copied().filter(|&f| sg(&sign, f) > 0.0).collect();
        let minus: Vec<f64> = piece.iter().copied().filter(|&f| sg(&sign, f) < 0.0).collect();
        let mut turn_minus = minus.len() <= plus.len();
        if !open {
            // a closed piece: after turning the few, it must face out (positive volume); else turn the others instead
            let mut vol = 0.0;
            let keep = if turn_minus { 1.0 } else { -1.0 };
            for &f in &piece {
                let v = def.faces.get(f).map_or(0.0, |face| signed_volume(def, face));
                vol += if sg(&sign, f) == keep { v } else { -v };
            }
            if vol < 0.0 {
                turn_minus = !turn_minus;
            }
        }
        p.flipped.extend(if turn_minus { minus } else { plus });
    }
    p.inner = inner.to_vec();
    p
}

/// What fixSolid fixed: how many of each.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fixed {
    pub stray: f64,
    pub inner: f64,
    pub flipped: f64,
    pub holes: f64,
}

/// Fix what can be fixed safely: stray edges go, faces inside the shape go, faces turn to face out, and a hole whose
/// rim lies flat gets a face.
pub fn fix_solid(def: &mut Def) -> Fixed {
    let p = solid_problems(def);
    for &eid in &p.stray {
        e_remove(def, eid);
    }
    for &fid in &p.inner {
        def.faces.delete(fid);
    }
    reverse_faces(def, &p.flipped);
    // holes: each connected rim that lies in one plane is closed by re-facing that plane
    let mut rim: IdMap<Vec<f64>> = IdMap::new();
    for eid in solid_problems(def).holes {
        let Some(e) = def.edges.get(eid) else { continue };
        for (x, y) in [(e.a, e.b), (e.b, e.a)] {
            match rim.get_mut(x) {
                Some(l) => l.push(y),
                None => rim.set(x, vec![y]),
            }
        }
    }
    let mut seen = IdSet::new();
    let mut holes = 0.0;
    for v0 in rim.key_vec() {
        if seen.has(v0) {
            continue;
        }
        let mut lp = Vec::new();
        let mut stack = vec![v0];
        seen.add(v0);
        while let Some(v) = stack.pop() {
            lp.push(v);
            for &w in rim.get(v).map_or(&[][..], |l| l.as_slice()) {
                if !seen.has(w) {
                    seen.add(w);
                    stack.push(w);
                }
            }
        }
        if lp.len() < 3 {
            continue;
        }
        let pp: Vec<P3> = lp.iter().map(|&v| def.v(v)).collect();
        let at = |i: usize| pp.get(i).copied().unwrap_or([f64::NAN; 3]);
        // the rim's plane: the best of the planes through three of its points
        let mut n = [0.0, 0.0, 0.0];
        let mut i = 1usize;
        while i + 1 < pp.len() {
            let c = cross(sub(at(i), at(0)), sub(at(i + 1), at(0)));
            n = if dot(c, n) < 0.0 { sub(n, c) } else { add(n, c) };
            i += 1;
        }
        if len(n) < 1e-12 {
            continue;
        }
        n = norm(n);
        if pp.iter().any(|q| dot(n, sub(*q, at(0))).abs() > EPS) {
            continue;
        }
        let before = def.faces.len();
        let set = IdSet::from_iter_ids(lp.iter().copied());
        let rim_edges = IdSet::from_iter_ids(def.edges.iter().filter(|(_, e)| set.has(e.a) && set.has(e.b)).map(|(id, _)| id));
        reface(def, &plane_from(n, at(0)), &rim_edges, None);
        if def.faces.len() > before {
            holes += 1.0;
        }
    }
    remove_loose_verts(def);
    // a face added over a hole may face in: turn what is still turned
    let again = solid_problems(def).flipped;
    reverse_faces(def, &again);
    Fixed { stray: p.stray.len() as f64, inner: p.inner.len() as f64, flipped: p.flipped.len() as f64, holes }
}

/// Closed (every edge of every face used by exactly two faces) and the enclosed volume (m³).
#[derive(Clone, Debug, PartialEq)]
pub struct SolidInfo {
    pub closed: bool,
    pub volume: Option<f64>,
    pub area: f64,
    pub faces: f64,
    pub edges: f64,
    pub verts: f64,
}

pub fn solid_info(def: &Def) -> SolidInfo {
    let ef = edge_faces(def);
    let mut closed = !def.faces.is_empty();
    // `for (const fs of ef.values()) if (fs.length !== 2) closed = false` (the order does not matter)
    if ef.lists().any(|fs| fs.len() != 2) {
        closed = false;
    }
    let mut vol = 0.0;
    let mut area = 0.0;
    for f in def.faces.values() {
        area += face_area(def, f);
        for [a, b, c] in face_tris(def, f) {
            vol += dot(def.v(a), cross(def.v(b), def.v(c))) / 6.0;
        }
    }
    let _ = js::truthy;
    SolidInfo {
        closed,
        volume: closed.then_some(vol),
        area,
        faces: def.faces.len() as f64,
        edges: def.edges.len() as f64,
        verts: def.verts.len() as f64,
    }
}
