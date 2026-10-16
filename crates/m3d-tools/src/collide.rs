//! Keep solids from passing through each other while they move (collide.js, one for one). `Collider::new(sc, moving,
//! ignore)` sorts the scene into what travels, what is neither in the way nor travelling, and what is in the way;
//! `clamp(delta)` gives the longest part of a straight move that stays clear (`hit` = it was shortened). Touching is
//! fine; only passing into the inside counts (an edge crossing a face's interior, or a point of one solid's face
//! inside the other: three rays agree). Things already overlapping at the start are left out. The path is sampled
//! (finer for thin things) and the first contact found by bisection. Translations only.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use cc_geom::bvh::{BoxTree, boxes_of};
use cc_m3d::vec::{P3, add, cross, dot, len, mul, norm, sub};

use crate::js::{g3, jmax, jmin, num_str, round};
use crate::scene::{ObjRef, SFace, Scene};

const TOL: f64 = 1e-5;
const MAX_STEPS: f64 = 160.0;

type Bb = Option<(P3, P3)>;

/// A face ready for testing.
#[derive(Debug)]
struct Prep {
    tris: Vec<[P3; 3]>,
    n: P3,
    d: f64,
    bnd: Vec<(P3, P3)>,
    pts: Vec<P3>,
}

#[derive(Debug, Default)]
struct Obj {
    /// the placement (index into the scene) this object stands for, with its face / edge indices
    placed: Option<usize>,
    fi: Vec<usize>,
    ei: Vec<usize>,
    faces: Vec<Rc<SFace>>,
    edges: Vec<(P3, P3)>,
    bb: Bb,
    ready: Option<(Vec<Prep>, BoxTree, BoxTree)>,
}

/// The collider.
#[derive(Debug)]
pub struct Collider {
    sc: Rc<Scene>,
    mov: Obj,
    statics: Vec<Obj>,
    thin: f64,
    pub empty: bool,
}

/// What `moving(o, kind)` / `ignore(o, kind)` answer (kind 'e' or 'f').
pub type Which<'a> = &'a dyn Fn(ObjRef<'_>, char) -> bool;

impl Collider {
    pub fn new(sc: Rc<Scene>, moving: Which<'_>, ignore: Which<'_>) -> Collider {
        let mut mov = Obj::default();
        let mut objs: Vec<(String, Obj)> = Vec::new();
        if sc.keep.is_none() {
            // a scene of placements: a placement in the way gets its world faces when first reached (ready)
            for (pi, p) in sc.insts.iter().enumerate() {
                let mut o: Option<Obj> = None;
                let mut whole = true;
                for (i, lf) in p.l.faces.iter().enumerate() {
                    let q = ObjRef { path: &p.path, id: lf.id };
                    if moving(q, 'f') {
                        if let Some(f) = p.face(i) {
                            mov.faces.push(f);
                        }
                        whole = false;
                    } else if !ignore(q, 'f') {
                        o.get_or_insert_with(|| Obj { placed: Some(pi), ..Obj::default() }).fi.push(i);
                    } else {
                        whole = false;
                    }
                }
                for (i, le) in p.l.edges.iter().enumerate() {
                    let q = ObjRef { path: &p.path, id: le.id };
                    if moving(q, 'e') {
                        if let Some(e) = p.edge(i) {
                            mov.edges.push((e.a, e.b));
                        }
                        whole = false;
                    } else if !ignore(q, 'e') {
                        o.get_or_insert_with(|| Obj { placed: Some(pi), ..Obj::default() }).ei.push(i);
                    } else {
                        whole = false;
                    }
                }
                if let Some(mut o) = o {
                    o.bb = placed_box(&sc, &o, whole);
                    let key = p.path.iter().map(|x| num_str(*x)).collect::<Vec<_>>().join("/");
                    match objs.iter_mut().find(|(k, _)| *k == key) {
                        Some(slot) => slot.1 = o,
                        None => objs.push((key, o)),
                    }
                }
            }
        } else {
            let mut at: HashMap<String, usize> = HashMap::new();
            let mut obj = |path: &[f64], objs: &mut Vec<(String, Obj)>| -> usize {
                let k = path.iter().map(|x| num_str(*x)).collect::<Vec<_>>().join("/");
                *at.entry(k.clone()).or_insert_with(|| {
                    objs.push((k, Obj::default()));
                    objs.len() - 1
                })
            };
            for f in sc.faces() {
                let q = ObjRef { path: &f.path, id: f.id };
                if moving(q, 'f') {
                    mov.faces.push(f.clone());
                } else if !ignore(q, 'f') {
                    let i = obj(&f.path, &mut objs);
                    if let Some(o) = objs.get_mut(i) {
                        o.1.faces.push(f.clone());
                    }
                }
            }
            for e in sc.edges() {
                let q = ObjRef { path: &e.path, id: e.id };
                if moving(q, 'e') {
                    mov.edges.push((e.a, e.b));
                } else if !ignore(q, 'e') {
                    let i = obj(&e.path, &mut objs);
                    if let Some(o) = objs.get_mut(i) {
                        o.1.edges.push((e.a, e.b));
                    }
                }
            }
            for (_, o) in &mut objs {
                o.bb = bbox_of(o);
            }
        }
        let mut statics: Vec<Obj> = objs.into_iter().map(|(_, o)| o).filter(|o| o.bb.is_some()).collect();
        mov.bb = bbox_of(&mov);
        let empty = mov.faces.is_empty() && mov.edges.is_empty();
        let mut c = Collider { sc, mov, statics: Vec::new(), thin: 0.0, empty };
        if !empty {
            let mut kept = Vec::new();
            for mut o in statics.drain(..) {
                if !hits(&c.sc, &mut c.mov, &mut o, [0.0, 0.0, 0.0]) {
                    kept.push(o);
                }
            }
            statics = kept;
        }
        let size = |bb: &Bb| -> f64 {
            match bb {
                None => f64::INFINITY,
                Some((lo, hi)) => crate::js::min_of((0..3).map(|k| jmax(g3(hi, k) - g3(lo, k), 0.0)).filter(|v| *v > TOL * 10.0)),
            }
        };
        c.thin = jmax(0.005, jmin(size(&c.mov.bb), crate::js::min_of(statics.iter().map(|o| size(&o.bb)))));
        c.statics = statics;
        c
    }

    fn blocked(&mut self, d: P3) -> bool {
        let sc = self.sc.clone();
        for o in &mut self.statics {
            if hits(&sc, &mut self.mov, o, d) {
                return true;
            }
        }
        false
    }

    /// The longest part of `delta` that stays clear, and whether it was shortened.
    pub fn clamp(&mut self, delta: P3) -> (P3, bool) {
        if self.empty || self.statics.is_empty() || len(delta) <= TOL {
            return (delta, false);
        }
        let steps = jmin(MAX_STEPS, jmax(4.0, (len(delta) / (self.thin / 2.0)).ceil()));
        let mut lo = 0.0;
        let mut hi: Option<f64> = None;
        let mut i = 1.0;
        while i <= steps {
            let tt = i / steps;
            if self.blocked(mul(delta, tt)) {
                hi = Some(tt);
                break;
            }
            lo = tt;
            i += 1.0;
        }
        let Some(mut hi) = hi else { return (delta, false) };
        for _ in 0..24 {
            let m = (lo + hi) / 2.0;
            if self.blocked(mul(delta, m)) {
                hi = m;
            } else {
                lo = m;
            }
        }
        (mul(delta, lo), true)
    }
}

fn prep_face(f: &SFace) -> Prep {
    let pts: Vec<P3> = f.tris.iter().map(|[a, b, c]| mul(add(add(*a, *b), *c), 1.0 / 3.0)).collect();
    if let [[a, b, c]] = f.tris.as_slice() {
        return Prep { tris: f.tris.clone(), n: f.n, d: f.plane.d, bnd: vec![(*a, *b), (*b, *c), (*c, *a)], pts };
    }
    if let [s, u] = f.tris.as_slice() {
        let eq = |p: &P3, q: &P3| (p[0] - q[0]).abs() <= TOL && (p[1] - q[1]).abs() <= TOL && (p[2] - q[2]).abs() <= TOL;
        let sides = |t: &[P3; 3]| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])];
        let shared = |e: &(P3, P3), t: &[P3; 3]| sides(t).iter().any(|(p, q)| (eq(p, &e.0) && eq(q, &e.1)) || (eq(p, &e.1) && eq(q, &e.0)));
        let mut bnd: Vec<(P3, P3)> = sides(s).into_iter().filter(|e| !shared(e, u)).collect();
        bnd.extend(sides(u).into_iter().filter(|e| !shared(e, s)));
        return Prep { tris: f.tris.clone(), n: f.n, d: f.plane.d, bnd, pts };
    }
    // the outline = triangle sides used once (insertion-ordered; a side seen again is dropped)
    let k = |p: &P3| p.iter().map(|x| num_str(round(x / TOL))).collect::<Vec<_>>().join(",");
    let mut order: Vec<String> = Vec::new();
    let mut seen: HashMap<String, (P3, P3)> = HashMap::new();
    for [a, b, c] in &f.tris {
        for (p, q) in [(*a, *b), (*b, *c), (*c, *a)] {
            let mut ks = [k(&p), k(&q)];
            // JS default sort: by UTF-16 code units (the keys are ASCII)
            ks.sort();
            let key = ks.join("|");
            if seen.remove(&key).is_some() {
                order.retain(|x| *x != key);
            } else {
                seen.insert(key.clone(), (p, q));
                order.push(key);
            }
        }
    }
    let bnd = order.iter().filter_map(|key| seen.get(key).copied()).collect();
    Prep { tris: f.tris.clone(), n: f.n, d: f.plane.d, bnd, pts }
}

/// The world box of a placement's faces and edges in the way (all of them: `whole`).
fn placed_box(sc: &Scene, o: &Obj, whole: bool) -> Bb {
    let p = sc.insts.get(o.placed?)?;
    let (m, l) = (&p.m, &p.l);
    let (mut x0, mut y0, mut z0) = (f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let (mut x1, mut y1, mut z1) = (f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut put = |x: f64, y: f64, z: f64| {
        let w = crate::js::or(m[3] * x + m[7] * y + m[11] * z + m[15], 1.0);
        let xx = (m[0] * x + m[4] * y + m[8] * z + m[12]) / w;
        let yy = (m[1] * x + m[5] * y + m[9] * z + m[13]) / w;
        let zz = (m[2] * x + m[6] * y + m[10] * z + m[14]) / w;
        if xx < x0 {
            x0 = xx;
        }
        if xx > x1 {
            x1 = xx;
        }
        if yy < y0 {
            y0 = yy;
        }
        if yy > y1 {
            y1 = yy;
        }
        if zz < z0 {
            z0 = zz;
        }
        if zz > z1 {
            z1 = zz;
        }
    };
    if whole {
        let (tris, _) = l.tri.as_chunks::<3>();
        for [x, y, z] in tris {
            put(*x, *y, *z);
        }
    } else {
        for &i in &o.fi {
            if let Some(f) = l.faces.get(i) {
                for t in &f.tris {
                    for q in t {
                        put(q[0], q[1], q[2]);
                    }
                }
            }
        }
    }
    for &i in &o.ei {
        if let Some(e) = l.edges.get(i) {
            put(e.a[0], e.a[1], e.a[2]);
            put(e.b[0], e.b[1], e.b[2]);
        }
    }
    if x0 <= x1 { Some(([x0, y0, z0], [x1, y1, z1])) } else { None }
}

fn bbox_of(o: &Obj) -> Bb {
    let mut bb: Bb = None;
    let mut put = |p: &P3| {
        let b = bb.get_or_insert((*p, *p));
        for ((lo, hi), v) in b.0.iter_mut().zip(b.1.iter_mut()).zip(p.iter()) {
            if *v < *lo {
                *lo = *v;
            }
            if *v > *hi {
                *hi = *v;
            }
        }
    };
    for f in &o.faces {
        for t in &f.tris {
            for p in t {
                put(p);
            }
        }
    }
    for (a, b) in &o.edges {
        put(a);
        put(b);
    }
    bb
}

/// An object's faces ready for testing, and trees over its faces and edges.
fn ready(sc: &Scene, o: &mut Obj) {
    if o.ready.is_some() {
        return;
    }
    if let Some(p) = o.placed.and_then(|i| sc.insts.get(i)) {
        o.faces = o.fi.iter().filter_map(|&i| p.face(i)).collect();
        o.edges = o.ei.iter().filter_map(|&i| p.edge(i)).map(|e| (e.a, e.b)).collect();
    }
    let prep: Vec<Prep> = o.faces.iter().map(|f| prep_face(f)).collect();
    let flat: Vec<Vec<P3>> = prep.iter().map(|f| f.tris.iter().flat_map(|t| t.iter().copied()).collect()).collect();
    let ftree = BoxTree::new(boxes_of(flat.iter().map(Vec::as_slice), TOL * 2.0));
    let ep: Vec<[P3; 2]> = o.edges.iter().map(|(a, b)| [*a, *b]).collect();
    let etree = BoxTree::new(boxes_of(ep.iter().map(|x| &x[..]), TOL * 2.0));
    o.ready = Some((prep, ftree, etree));
}

fn lo3(a: P3, b: P3, s: Option<P3>) -> [f64; 3] {
    let s = s.unwrap_or([0.0; 3]);
    [jmin(a[0], b[0]) - s[0], jmin(a[1], b[1]) - s[1], jmin(a[2], b[2]) - s[2]]
}
fn hi3(a: P3, b: P3, s: Option<P3>) -> [f64; 3] {
    let s = s.unwrap_or([0.0; 3]);
    [jmax(a[0], b[0]) - s[0], jmax(a[1], b[1]) - s[1], jmax(a[2], b[2]) - s[2]]
}

/// Do the moving pieces, shifted by d, pass into static object o?
fn hits(sc: &Scene, mov: &mut Obj, o: &mut Obj, d: P3) -> bool {
    let (Some(mb), Some(ob)) = (mov.bb, o.bb) else { return false };
    for i in 0..3 {
        if g3(&mb.0, i) + g3(&d, i) >= g3(&ob.1, i) - TOL || g3(&mb.1, i) + g3(&d, i) <= g3(&ob.0, i) + TOL {
            return false;
        }
    }
    ready(sc, mov);
    ready(sc, o);
    let (Some((mf, mft, met)), Some((of, oft, oet))) = (&mov.ready, &o.ready) else { return false };
    let lo = [0, 1, 2].map(|i| jmax(g3(&mb.0, i) + g3(&d, i), g3(&ob.0, i)) - TOL);
    let hi = [0, 1, 2].map(|i| jmin(g3(&mb.1, i) + g3(&d, i), g3(&ob.1, i)) + TOL);
    let lo_m = sub(lo, d);
    let hi_m = sub(hi, d);
    let in_box = |p: &P3| p[0] >= lo[0] && p[0] <= hi[0] && p[1] >= lo[1] && p[1] <= hi[1] && p[2] >= lo[2] && p[2] <= hi[2];
    // an edge through the inside of a face, either way round
    if met.find_box(lo_m, hi_m, |i| {
        let Some((ea, eb)) = mov.edges.get(i) else { return false };
        let (a, b) = (add(*ea, d), add(*eb, d));
        oft.find_box(lo3(a, b, None), hi3(a, b, None), |j| of.get(j).is_some_and(|f| crosses(a, b, f, None)))
    }) {
        return true;
    }
    if oet.find_box(lo, hi, |i| {
        let Some((ea, eb)) = o.edges.get(i) else { return false };
        mft.find_box(lo3(*ea, *eb, Some(d)), hi3(*ea, *eb, Some(d)), |j| mf.get(j).is_some_and(|f| crosses(*ea, *eb, f, Some(d))))
    }) {
        return true;
    }
    // a point of one inside the other
    if mft.find_box(lo_m, hi_m, |j| {
        mf.get(j).is_some_and(|f| {
            f.pts.iter().any(|p| {
                let q = add(*p, d);
                in_box(&q) && inside(q, of, oft)
            })
        })
    }) {
        return true;
    }
    if oft.find_box(lo, hi, |j| of.get(j).is_some_and(|f| f.pts.iter().any(|p| in_box(p) && inside(sub(*p, d), mf, mft)))) {
        return true;
    }
    false
}

/// Segment p–q crosses face f's interior (f shifted by s).
fn crosses(p: P3, q: P3, f: &Prep, s: Option<P3>) -> bool {
    let dd = match s {
        Some(s) => f.d + dot(f.n, s),
        None => f.d,
    };
    let sp = dot(f.n, p) - dd;
    let sq = dot(f.n, q) - dd;
    if !((sp > TOL && sq < -TOL) || (sp < -TOL && sq > TOL)) {
        return false;
    }
    let tt = sp / (sp - sq);
    let mut x = add(p, mul(sub(q, p), tt));
    if let Some(s) = s {
        x = sub(x, s);
    }
    if !in_face(x, f) {
        return false;
    }
    !f.bnd.iter().any(|(a, b)| seg_dist(x, *a, *b) <= TOL)
}

fn in_face(x: P3, f: &Prep) -> bool {
    for [a, b, c] in &f.tris {
        let n = cross(sub(*b, *a), sub(*c, *a));
        let nn = dot(n, n);
        if nn < 1e-24 {
            continue;
        }
        let u = dot(cross(sub(*c, *b), sub(x, *b)), n) / nn;
        let v = dot(cross(sub(*a, *c), sub(x, *c)), n) / nn;
        if u >= -1e-9 && v >= -1e-9 && 1.0 - u - v >= -1e-9 {
            return true;
        }
    }
    false
}

fn seg_dist(x: P3, a: P3, b: P3) -> f64 {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    let tt = if crate::js::truthy(l2) { jmax(0.0, jmin(1.0, dot(sub(x, a), ab) / l2)) } else { 0.0 };
    len(sub(x, add(a, mul(ab, tt))))
}

fn rays() -> [P3; 3] {
    [norm([0.5773, 0.5774, 0.5775]), norm([-0.6123, 0.2189, 0.7597]), norm([0.1931, -0.8972, -0.3973])]
}

/// Is p inside the closed surface made of these faces? Three rays must all cross it an odd number of times.
fn inside(p: P3, faces: &[Prep], tree: &BoxTree) -> bool {
    for r in rays() {
        let mut n = 0u64;
        let mut on = false;
        let lim = Cell::new(f64::INFINITY);
        tree.ray(p, r, &lim, |j| {
            let Some(f) = faces.get(j) else { return false };
            for [a, b, c] in &f.tris {
                let Some(tt) = ray_tri_t(p, r, *a, *b, *c) else { continue };
                if tt <= TOL {
                    on = true;
                    return true;
                }
                n = n.saturating_add(1);
                return false;
            }
            false
        });
        if on || n.is_multiple_of(2) {
            return false;
        }
    }
    true
}

fn ray_tri_t(o: P3, d: P3, a: P3, b: P3, c: P3) -> Option<f64> {
    let e1 = sub(b, a);
    let e2 = sub(c, a);
    let pv = cross(d, e2);
    let det = dot(e1, pv);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(o, a);
    let u = dot(s, pv) * inv;
    if u < -1e-9 || u > 1.0 + 1e-9 {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < -1e-9 || u + v > 1.0 + 1e-9 {
        return None;
    }
    let tt = dot(e2, q) * inv;
    if tt >= -TOL { Some(jmax(tt, 0.0)) } else { None }
}
