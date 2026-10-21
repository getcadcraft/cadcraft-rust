//! Meshes from other programs (meshDef, edgesDef), world triangles for exporters (worldTriangles), and Intersect
//! Faces (0.4 M19), which reads the whole model.

use serde_json::Value;

use crate::def::*;
use crate::edges::{EdgeOpts, add_edge};
use crate::js;
use crate::model::Model;
use crate::omap::FxMap;
use crate::ops::Sel;
use crate::vec::*;

/// A point from imported data: its numbers (`p.map(x => Math.round(x * 1e5)).join(',')` keys it; vAdd keeps three).
fn pt_of(v: &Value) -> Option<Vec<f64>> {
    v.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect())
}
fn grid_key(p: &[f64]) -> String {
    let mut s = String::new();
    for (i, x) in p.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        js::push_num(&mut s, js::round(x * 1e5));
    }
    s
}
fn p3_of(p: &[f64]) -> P3 {
    [p.first().copied().unwrap_or(f64::NAN), p.get(1).copied().unwrap_or(f64::NAN), p.get(2).copied().unwrap_or(f64::NAN)]
}

/// A definition from imported polygons: each entry an array of points (one outer loop) or {loops: [outer, ...holes],
/// color}. Not flat: a fan of triangles. Edges between coplanar faces are soft. `segs` ([p, q] pairs) become plain
/// edges, welded to the faces' vertices. → the new definition's id.
pub fn mesh_def(model: &mut Model, polys: &[Value], name: Option<&str>, kind: &str, segs: &[Value]) -> String {
    let id = model.new_def(kind, name);
    let Some(mut d) = model.defs.get(&id).cloned() else { return id };
    let mut grid: FxMap<String, f64> = FxMap::default();
    let mut vid = |d: &mut Def, p: &[f64]| -> f64 {
        let k = grid_key(p);
        if let Some(&id) = grid.get(&k) {
            return id;
        }
        let id = v_add(d, p3_of(p));
        grid.insert(k, id);
        id
    };
    let ring = |d: &mut Def, vid: &mut dyn FnMut(&mut Def, &[f64]) -> f64, pts: &Value| -> Option<Vec<f64>> {
        let mut lp: Vec<f64> = Vec::new();
        for p in pts.as_array().map_or(&[][..], |a| a.as_slice()) {
            let pp = pt_of(p).unwrap_or_default();
            let v = vid(d, &pp);
            if lp.last() != Some(&v) {
                lp.push(v);
            }
        }
        while lp.len() > 1 && lp.first() == lp.last() {
            lp.pop();
        }
        let mut uniq = lp.clone();
        uniq.sort_by(f64::total_cmp);
        uniq.dedup();
        if lp.len() < 3 || uniq.len() != lp.len() {
            return None;
        }
        if poly_area_of(d, &lp) <= EPS2 { None } else { Some(lp) }
    };
    for poly in polys {
        let obj = !poly.is_array();
        let rings: Vec<Value> =
            if obj { poly.get("loops").and_then(Value::as_array).cloned().unwrap_or_default() } else { vec![poly.clone()] };
        let color = if obj { poly.get("color").and_then(Value::as_str).map(str::to_string) } else { None };
        let Some(outer) = rings.first().and_then(|r| ring(&mut d, &mut vid, r)) else { continue };
        let pp: Vec<P3> = outer.iter().map(|&v| d.v(v)).collect();
        let n = newell(&pp);
        let p0 = pp.first().copied().unwrap_or([f64::NAN; 3]);
        let mut size = 0.0;
        for p in &pp {
            size = js::max(size, dist(*p, p0));
        }
        let tol = js::max(EPS, size * 1e-6);
        if pp.iter().any(|p| dot(n, sub(*p, p0)).abs() > tol) {
            // not flat: a fan of triangles (each one flat by definition)
            let mut i = 1usize;
            while i + 1 < outer.len() {
                let t = vec![
                    outer.first().copied().unwrap_or(f64::NAN),
                    outer.get(i).copied().unwrap_or(f64::NAN),
                    outer.get(i + 1).copied().unwrap_or(f64::NAN),
                ];
                if poly_area_of(&d, &t) > EPS2 {
                    f_add(&mut d, vec![t], color.clone(), None, false);
                }
                i += 1;
            }
            continue;
        }
        let mut holes = Vec::new();
        for r in rings.iter().skip(1) {
            let Some(mut h) = ring(&mut d, &mut vid, r) else { continue };
            if h.iter().any(|v| outer.contains(v)) {
                continue;
            }
            if dot(newell_of(&d, &h), n) > 0.0 {
                h.reverse();
            }
            holes.push(h);
        }
        let mut loops = vec![outer];
        loops.extend(holes);
        f_add(&mut d, loops, color, None, false);
    }
    for s in segs {
        let (Some(p), Some(q)) = (s.get(0).and_then(pt_of), s.get(1).and_then(pt_of)) else { continue };
        let a = vid(&mut d, &p);
        let b = vid(&mut d, &q);
        if a != b {
            e_add0(&mut d, a, b);
        }
    }
    let ef = edge_faces(&d);
    let mut soft = Vec::new();
    for (eid, e) in d.edges.iter() {
        if let [f0, f1] = ef_get(&ef, e.a, e.b)
            && let (Some(fa), Some(fb)) = (d.faces.get(*f0), d.faces.get(*f1))
            && dot(face_normal(&d, fa), face_normal(&d, fb)) > 1.0 - 1e-6
        {
            soft.push(eid);
        }
    }
    for eid in soft {
        if let Some(e) = d.edges.get_mut(eid) {
            e.soft = true;
        }
    }
    if let Some(slot) = model.defs.get_mut(&id) {
        *slot = d;
    }
    id
}

/// Plain edges (no faces) from a list of [p, q] segments. → the new definition's id.
pub fn edges_def(model: &mut Model, segs: &[Value], name: Option<&str>) -> String {
    let id = model.new_def("group", name);
    let Some(d) = model.defs.get_mut(&id) else { return id };
    let mut grid: FxMap<String, f64> = FxMap::default();
    for s in segs {
        let (Some(p), Some(q)) = (s.get(0).and_then(pt_of), s.get(1).and_then(pt_of)) else { continue };
        let mut vid = |d: &mut Def, p: &[f64]| -> f64 {
            let k = grid_key(p);
            if let Some(&id) = grid.get(&k) {
                return id;
            }
            let id = v_add(d, p3_of(p));
            grid.insert(k, id);
            id
        };
        let a = vid(d, &p);
        let b = vid(d, &q);
        if a != b {
            e_add0(d, a, b);
        }
    }
    id
}

/// A world triangle: corners, normal, colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Tri {
    pub p: [P3; 3],
    pub n: P3,
    pub color: Option<String>,
}

/// World-space triangles of the whole model (or one definition) for exporters.
pub fn world_triangles(model: &Model, def: &str, m: &M4) -> Vec<Tri> {
    let mut out = Vec::new();
    model.walk(def, m, &mut |d, wm, _, pc| {
        let mirror = det3(wm) < 0.0;
        for f in d.faces.values() {
            let n = norm(apply_dir(wm, face_normal(d, f)));
            for t in face_tris(d, f) {
                let mut ps = [apply(wm, d.v(t[0])), apply(wm, d.v(t[1])), apply(wm, d.v(t[2]))];
                if mirror {
                    ps = [ps[0], ps[2], ps[1]];
                }
                let fnn = norm(cross(sub(ps[1], ps[0]), sub(ps[2], ps[0])));
                let color = f.color.clone().filter(|c| !c.is_empty()).or_else(|| pc.filter(|c| !c.is_empty()).map(str::to_string));
                out.push(Tri { p: ps, n: if js::truthy(len(fnn)) { fnn } else { n }, color });
            }
        }
        true
    });
    out
}

struct Poly {
    key: String,
    loops: Vec<Vec<P3>>,
    n: P3,
    d: f64,
    lo: P3,
    hi: P3,
}

fn polys(def: &Def, m: &M4, filter: Option<&dyn Fn(f64) -> bool>, out: &mut Vec<Poly>) {
    let mut mkey = String::new();
    for (i, x) in m.iter().enumerate() {
        if i > 0 {
            mkey.push(',');
        }
        js::push_num(&mut mkey, *x);
    }
    for (fid, f) in def.faces.iter() {
        if f.hidden || filter.is_some_and(|fl| !fl(fid)) {
            continue;
        }
        let loops: Vec<Vec<P3>> = f.loops.iter().map(|l| l.iter().map(|&v| apply(m, def.v(v))).collect()).collect();
        let l0 = loops.first().cloned().unwrap_or_default();
        let n = norm(newell(&l0));
        if !js::truthy(len(n)) {
            continue;
        }
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in &l0 {
            for (k, (l, h)) in lo.iter_mut().zip(hi.iter_mut()).enumerate() {
                *l = js::min(*l, at3(p, k));
                *h = js::max(*h, at3(p, k));
            }
        }
        let mut key = def.id.clone();
        key.push(':');
        js::push_num(&mut key, fid);
        key.push(':');
        key.push_str(&mkey);
        let d = dot(n, l0.first().copied().unwrap_or([f64::NAN; 3]));
        out.push(Poly { key, loops, n, d, lo, hi });
    }
}

/// Where the line x0 + t·u crosses a face: inside intervals of t (even-odd over its loops).
fn spans(f: &Poly, x0: P3, u: P3) -> Vec<(f64, f64)> {
    let w = cross(f.n, u);
    let mut ts = Vec::new();
    for l in &f.loops {
        let n = l.len();
        for (i, &p) in l.iter().enumerate() {
            let q = l.get((i + 1) % n).copied().unwrap_or(p);
            let sp = dot(sub(p, x0), w);
            let sq = dot(sub(q, x0), w);
            if (sp > 0.0) == (sq > 0.0) {
                continue;
            }
            let c = add(p, mul(sub(q, p), sp / (sp - sq)));
            ts.push(dot(sub(c, x0), u));
        }
    }
    js::sort_by(&mut ts, |a, b| a - b);
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 1 < ts.len() {
        let (a, b) = (ts.get(i).copied().unwrap_or(f64::NAN), ts.get(i + 1).copied().unwrap_or(f64::NAN));
        if b - a > EPS {
            out.push((a, b));
        }
        i += 2;
    }
    out
}

/// Intersect faces (0.4 M19): the selection's faces (and every face inside selected groups) are cut against
/// `others` ('model' = every face on screen, else the same faces); the lines are drawn as edges in the open context.
/// → how many edges the open context gained.
pub fn intersect_faces(model: &mut Model, path: &[f64], sel: &[Sel], others: &str) -> f64 {
    let Some(ctx) = model.context(path) else { return 0.0 };
    let mut a: Vec<Poly> = Vec::new();
    let fids: Vec<f64> = sel.iter().filter(|s| s.is("f")).map(|s| s.id).collect();
    if let Some(cd) = model.def(&ctx.def) {
        if !fids.is_empty() {
            let fl = |fid: f64| fids.iter().any(|&x| crate::omap::kb(x) == crate::omap::kb(fid));
            polys(cd, &ctx.m, Some(&fl), &mut a);
        }
        for s in sel {
            if !s.is("i") {
                continue;
            }
            let Some(inst) = cd.insts.get(s.id) else { continue };
            model.walk(&inst.def, &mmul(&ctx.m, &inst.m), &mut |d, m, _, _| {
                polys(d, m, None, &mut a);
                true
            });
        }
    }
    let mut b_own: Vec<Poly> = Vec::new();
    let use_model = others == "model";
    if use_model {
        model.walk_shown("root", &IDENT, &mut |d, m, _, _| {
            polys(d, m, None, &mut b_own);
            true
        });
    }
    let b: &[Poly] = if use_model { &b_own } else { &a };
    let mut segs: Vec<(P3, P3)> = Vec::new();
    // each face's key as a number (the same key, the same number), so a pair is two numbers, not two strings
    let mut ids: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut ida: Vec<usize> = Vec::with_capacity(a.len());
    for p in &a {
        let n = ids.len();
        ida.push(*ids.entry(p.key.as_str()).or_insert(n));
    }
    let mut idb: Vec<usize> = Vec::with_capacity(b.len());
    for p in b {
        let n = ids.len();
        idb.push(*ids.entry(p.key.as_str()).or_insert(n));
    }
    let mut seen: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    for (pa, &ka) in a.iter().zip(&ida) {
        for (pb, &kb) in b.iter().zip(&idb) {
            if ka == kb {
                continue;
            }
            // boxes apart: skipped (the test is symmetric, so the pair's other order is skipped too and the set of
            // pairs seen below is the one a seen-first order gives)
            if (0..3).any(|k| at3(&pa.hi, k) < at3(&pb.lo, k) - EPS || at3(&pb.hi, k) < at3(&pa.lo, k) - EPS) {
                continue;
            }
            if !seen.insert((ka.min(kb), ka.max(kb))) {
                continue;
            }
            let dir = cross(pa.n, pb.n);
            let l = len(dir);
            if l < 1e-9 {
                continue;
            }
            let u = mul(dir, 1.0 / l);
            let x0 = mul(cross(sub(mul(pb.n, pa.d), mul(pa.n, pb.d)), dir), 1.0 / (l * l));
            let sa = spans(pa, x0, u);
            let sb = spans(pb, x0, u);
            for &(a0, a1) in &sa {
                for &(b0, b1) in &sb {
                    let t0 = js::max(a0, b0);
                    let t1 = js::min(a1, b1);
                    if t1 - t0 > EPS {
                        segs.push((add(x0, mul(u, t0)), add(x0, mul(u, t1))));
                    }
                }
            }
        }
    }
    let Some(d) = model.def_mut(&ctx.def) else { return 0.0 };
    let before = d.edges.len() as f64;
    for (p, q) in segs {
        add_edge(d, apply(&ctx.inv, p), apply(&ctx.inv, q), EdgeOpts::default());
    }
    d.edges.len() as f64 - before
}
