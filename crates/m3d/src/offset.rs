//! Offset (0.4 M4): a face's loops, or a chain of edges, moved sideways in their plane by a distance.

use crate::def::*;
use crate::error::{Error, Res, msg};
use crate::js;
use crate::omap::IdMap;
use crate::ops::{add_polygon, tag_curve};
use crate::vec::*;

/// Offset a 2D polyline: every segment's line moves `d` to its left, corners where neighbouring lines meet (closed:
/// the loop wraps). → the new points, or None when a segment would flip.
pub fn offset2(pp: &[P2], d: f64, closed: bool) -> Option<Vec<P2>> {
    let n = pp.len();
    if n < 2 {
        return None;
    }
    let at = |i: usize| pp.get(i).copied().unwrap_or([f64::NAN; 2]);
    struct S {
        a: P2,
        u: P2,
    }
    let mut segs: Vec<S> = Vec::new();
    for i in 0..(if closed { n } else { n - 1 }) {
        let a = at(i);
        let b = at((i + 1) % n);
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let l = js::hypot2(dx, dy);
        if !(l > 1e-12) {
            return None;
        }
        let nx = -dy / l;
        let ny = dx / l;
        segs.push(S { a: [a[0] + nx * d, a[1] + ny * d], u: [dx / l, dy / l] });
    }
    let meet = |s: &S, t: &S| -> P2 {
        let den = s.u[0] * t.u[1] - s.u[1] * t.u[0];
        if den.abs() < 1e-9 {
            return [t.a[0], t.a[1]];
        }
        let k = ((t.a[0] - s.a[0]) * t.u[1] - (t.a[1] - s.a[1]) * t.u[0]) / den;
        [s.a[0] + s.u[0] * k, s.a[1] + s.u[1] * k]
    };
    let ns = segs.len();
    let mut q: Vec<P2> = Vec::with_capacity(n);
    for i in 0..n {
        if !closed && i == 0 {
            q.push(segs.first().map_or([f64::NAN; 2], |s| s.a));
            continue;
        }
        if !closed && i == n - 1 {
            let s = segs.get(n - 2)?;
            let l = js::hypot2(at(n - 1)[0] - at(n - 2)[0], at(n - 1)[1] - at(n - 2)[1]);
            q.push([s.a[0] + s.u[0] * l, s.a[1] + s.u[1] * l]);
            continue;
        }
        let (Some(s), Some(t)) = (segs.get((i + ns - 1) % ns), segs.get(i)) else { return None };
        q.push(meet(s, t));
    }
    for (i, s) in segs.iter().enumerate() {
        let a = q.get(i).copied().unwrap_or([f64::NAN; 2]);
        let b = q.get((i + 1) % n).copied().unwrap_or([f64::NAN; 2]);
        if (b[0] - a[0]) * s.u[0] + (b[1] - a[1]) * s.u[1] <= 1e-9 {
            return None;
        }
    }
    Some(q)
}

fn segs_cross2(a: P2, b: P2, c: P2, d: P2) -> bool {
    let o = |p: P2, q: P2, r: P2| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
    let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
    ((d1 > 1e-12 && d2 < -1e-12) || (d1 < -1e-12 && d2 > 1e-12)) && ((d3 > 1e-12 && d4 < -1e-12) || (d3 < -1e-12 && d4 > 1e-12))
}
/// Do these 2D loops cross themselves or each other?
pub fn loops_cross(loops: &[Vec<P2>]) -> bool {
    struct S {
        li: usize,
        i: usize,
        n: usize,
        a: P2,
        b: P2,
    }
    let mut ss = Vec::new();
    for (li, l) in loops.iter().enumerate() {
        let n = l.len();
        for (i, p) in l.iter().enumerate() {
            ss.push(S { li, i, n, a: *p, b: l.get((i + 1) % n).copied().unwrap_or(*p) });
        }
    }
    for (i, s) in ss.iter().enumerate() {
        for t in ss.iter().skip(i + 1) {
            if s.li == t.li && (s.i.abs_diff(t.i) <= 1 || s.i.abs_diff(t.i) == s.n.wrapping_sub(1)) {
                continue;
            }
            if segs_cross2(s.a, s.b, t.a, t.b) {
                return true;
            }
        }
    }
    false
}

/// The loops of face fid offset by d (positive = into the face), in the def's coordinates, in the face's loop order.
pub fn offset_face_pts(def: &Def, fid: f64, d: f64) -> Res<Vec<Vec<P3>>> {
    let Some(f) = def.faces.get(fid) else { return Err(msg("m3d.model.there_is_no_such_face")) };
    let n = face_normal(def, f);
    let fr = frame(n);
    let o = f.loops.first().and_then(|l| l.first()).map_or([f64::NAN; 3], |&v| def.v(v));
    let h = dot(o, n);
    let l2 = loops2(def, f, &fr);
    let mut out2 = Vec::new();
    for l in &l2 {
        let Some(q) = offset2(l, d, true) else { return Err(msg("m3d.model.that_is_further_than_this_face")) };
        out2.push(q);
    }
    if loops_cross(&out2) {
        return Err(msg("m3d.model.that_is_further_than_this_face_2"));
    }
    Ok(out2.iter().map(|q| q.iter().map(|&[x, y]| add(add(mul(fr.u, x), mul(fr.v, y)), mul(n, h))).collect()).collect())
}

/// Offset face fid by d as new edges: the face splits (inward) or grows a ring (outward). Arcs stay arcs. → the face
/// inside the first new loop (inward), for a push straight after.
pub fn offset_face(def: &mut Def, fid: f64, d: f64) -> Res<Option<f64>> {
    let pts = offset_face_pts(def, fid, d)?;
    let Some(f) = def.faces.get(fid).cloned() else { return Err(Error::Type) };
    let n = face_normal(def, &f);
    let mut curves_of: Vec<Vec<Option<String>>> = Vec::new();
    for l in &f.loops {
        let m = l.len();
        let mut cs = Vec::with_capacity(m);
        for (i, &v) in l.iter().enumerate() {
            let w = l.get((i + 1) % m).copied().unwrap_or(v);
            let c = edge_between(def, v, w).and_then(|e| def.edges.get(e)).and_then(|e| e.curve.clone()).filter(|c| !c.is_empty());
            cs.push(c);
        }
        curves_of.push(cs);
    }
    for p in &pts {
        add_polygon(def, p, Some(n))?;
    }
    // the offset of an arc / circle / polygon is one too
    for (li, p) in pts.iter().enumerate() {
        let cs = curves_of.get(li).cloned().unwrap_or_default();
        let m = p.len();
        if !cs.iter().any(Option::is_some) {
            continue;
        }
        let c0 = cs.first().cloned().flatten();
        if cs.iter().all(|c| c.is_some() && *c == c0) {
            let kind = c0.as_deref().unwrap_or("").split(':').next().unwrap_or("").to_string();
            tag_curve(def, p, &kind, true);
            continue;
        }
        let cg = |i: usize| cs.get(i).cloned().flatten();
        let mut start = (0..m).position(|i| cg(i) != cg((i + m - 1) % m)).unwrap_or(0);
        if m == 0 {
            start = 0;
        }
        let mut k = 0usize;
        while k < m {
            let i = (start + k) % m;
            let c = cg(i);
            let mut j = k;
            while j < m && cg((start + j) % m) == c {
                j += 1;
            }
            if let Some(c) = c {
                let piece: Vec<P3> = (0..=j - k).map(|q| p.get((start + k + q) % m).copied().unwrap_or([f64::NAN; 3])).collect();
                let kind = c.split(':').next().unwrap_or("").to_string();
                tag_curve(def, &piece, &kind, false);
            }
            k = j;
        }
    }
    // the face inside the first new loop (inward), for a push straight after
    if d > 0.0 {
        let p0 = pts.first().cloned().unwrap_or_default();
        for (id, g) in def.faces.iter() {
            let Some(l0) = g.loops.first() else { continue };
            if l0.len() == p0.len() && l0.iter().all(|&v| p0.iter().any(|p| dist(*p, def.v(v)) <= EPS)) {
                return Ok(Some(id));
            }
        }
    }
    Ok(None)
}

/// What offsetChainPts gives.
#[derive(Clone, Debug, PartialEq)]
pub struct ChainOffset {
    pub pts: Vec<P3>,
    pub closed: bool,
    pub chain: Vec<f64>,
}

/// Offset a chain of connected edges (all in one plane with normal n) by d to the left of its direction.
pub fn offset_chain_pts(def: &Def, eids: &[f64], d: f64, n: P3) -> Res<ChainOffset> {
    let mut adj: IdMap<Vec<f64>> = IdMap::new();
    for &id in eids {
        let Some(e) = def.edges.get(id) else { continue };
        for (x, y) in [(e.a, e.b), (e.b, e.a)] {
            match adj.get_mut(x) {
                Some(l) => l.push(y),
                None => adj.set(x, vec![y]),
            }
        }
    }
    if adj.values().any(|l| l.len() > 2) {
        return Err(msg("m3d.model.offset_works_on_one_chain_of"));
    }
    let ends: Vec<f64> = adj.iter().filter(|(_, l)| l.len() == 1).map(|(v, _)| v).collect();
    let closed = ends.is_empty();
    let mut chain: Vec<f64> = Vec::new();
    let mut cur = ends.first().copied().or_else(|| adj.first_key());
    let mut prev: Option<f64> = None;
    while let Some(c) = cur {
        if chain.contains(&c) {
            break;
        }
        chain.push(c);
        let nx = adj.get(c).and_then(|l| l.iter().copied().find(|&x| Some(x) != prev && !chain.contains(&x)));
        prev = Some(c);
        cur = nx;
    }
    if chain.len() != adj.len() {
        return Err(msg("m3d.model.offset_works_on_one_connected_chain"));
    }
    let p3: Vec<P3> = chain.iter().map(|&v| def.v(v)).collect();
    // no edges at all: `P3[0]` is undefined and the JS throws a TypeError
    let Some(&first) = p3.first() else { return Err(Error::Type) };
    let fr = frame(norm(n));
    let h = dot(first, fr.n);
    if p3.iter().any(|p| (dot(*p, fr.n) - h).abs() > js::max(EPS, 1e-7 * dist(*p, first))) {
        return Err(msg("m3d.model.these_edges_are_not_all_in"));
    }
    let flat: Vec<P2> = p3.iter().map(|p| to2(&fr, *p)).collect();
    let q = offset2(&flat, d, closed);
    let Some(q) = q.filter(|q| !(closed && loops_cross(std::slice::from_ref(q)))) else {
        return Err(msg("m3d.model.that_is_further_than_these_edges"));
    };
    Ok(ChainOffset { pts: q.iter().map(|&[x, y]| add(add(mul(fr.u, x), mul(fr.v, y)), mul(fr.n, h))).collect(), closed, chain })
}
