//! The 3D faces of a drawing as a file writes them (model3d.js `Model3D.fromFile` for loose faces + `forFile3d`),
//! with the m3d kernel pieces that needs (m3d/model.js `meshDef`, `faceTris`, `toJSON`; m3d/vec.js; m3d/earcut.js
//! after mapbox/earcut, ISC licence © Mapbox). Only the loose-faces path: a model CadCraft saved itself (block
//! CADCRAFT_3D) is not restored here.

use std::collections::HashMap;

use cc_cad::js;
use cc_io_dwg::value::V;

use crate::jsv;
use crate::small::hypot3;

const EPS: f64 = 1e-6;
const EPS2: f64 = EPS * EPS;
/// Earcut steps at most (the JS has no cap; a hostile ring must not hang).
const MAX_EAR_STEPS: usize = 20_000_000;

type P3 = [f64; 3];

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: P3) -> f64 {
    hypot3(a[0], a[1], a[2])
}
fn dist(a: P3, b: P3) -> f64 {
    hypot3(a[0] - b[0], a[1] - b[1], a[2] - b[2])
}
fn norm(a: P3) -> P3 {
    let l = len(a);
    if l > 1e-15 { [a[0] / l, a[1] / l, a[2] / l] } else { [0.0, 0.0, 0.0] }
}
fn newell_raw(pts: &[P3]) -> P3 {
    let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
    let n = pts.len();
    for i in 0..n {
        let p = pts.get(i).copied().unwrap_or([f64::NAN; 3]);
        let q = pts.get(jsv::wrap_next(i, n)).copied().unwrap_or([f64::NAN; 3]);
        x += (p[1] - q[1]) * (p[2] + q[2]);
        y += (p[2] - q[2]) * (p[0] + q[0]);
        z += (p[0] - q[0]) * (p[1] + q[1]);
    }
    [x, y, z]
}
fn newell(pts: &[P3]) -> P3 {
    norm(newell_raw(pts))
}
fn poly_area3(pts: &[P3]) -> f64 {
    let [x, y, z] = newell_raw(pts);
    hypot3(x, y, z) / 2.0
}

/// One definition (insertion-ordered maps; ids from one counter, nothing removed).
struct Def {
    id: String,
    kind: &'static str,
    name: String,
    next: u64,
    verts: Vec<(u64, P3)>,
    vidx: HashMap<u64, usize>,
    edges: Vec<(u64, u64, u64, bool)>,
    ek: HashMap<(u64, u64), u64>,
    faces: Vec<(u64, Vec<Vec<u64>>)>,
    insts: Vec<(u64, String, [f64; 16])>,
}

fn key(a: u64, b: u64) -> (u64, u64) {
    if a < b { (a, b) } else { (b, a) }
}

impl Def {
    fn new(id: &str, kind: &'static str, name: &str) -> Def {
        Def {
            id: id.into(),
            kind,
            name: name.into(),
            next: 1,
            verts: Vec::new(),
            vidx: HashMap::new(),
            edges: Vec::new(),
            ek: HashMap::new(),
            faces: Vec::new(),
            insts: Vec::new(),
        }
    }
    fn nid(&mut self) -> u64 {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        id
    }
    fn v(&self, id: u64) -> P3 {
        self.vidx.get(&id).and_then(|i| self.verts.get(*i)).map_or([f64::NAN; 3], |v| v.1)
    }
    fn v_add(&mut self, p: P3) -> u64 {
        let id = self.nid();
        self.vidx.insert(id, self.verts.len());
        self.verts.push((id, p));
        id
    }
    fn e_add(&mut self, a: u64, b: u64) {
        if a == b || self.ek.contains_key(&key(a, b)) {
            return;
        }
        let id = self.nid();
        self.edges.push((id, a, b, false));
        self.ek.insert(key(a, b), id);
    }
    fn f_add(&mut self, loops: Vec<Vec<u64>>) {
        let fid = self.nid();
        for l in &loops {
            let n = l.len();
            for i in 0..n {
                if let (Some(a), Some(b)) = (l.get(i), l.get(jsv::wrap_next(i, n))) {
                    self.e_add(*a, *b);
                }
            }
        }
        self.faces.push((fid, loops));
    }
    fn face_normal(&self, loops: &[Vec<u64>]) -> P3 {
        let pts: Vec<P3> = loops.first().map(|l| l.iter().map(|v| self.v(*v)).collect()).unwrap_or_default();
        newell(&pts)
    }
}

/// m3d `meshDef(model, polys, name)` for plain point rings.
fn mesh_def(polys: &[Vec<P3>], name: &str) -> Def {
    let mut d = Def::new("d1", "group", name);
    let mut grid: HashMap<String, u64> = HashMap::new();
    let mut vid = |d: &mut Def, p: P3| -> u64 {
        let k = p.iter().map(|x| js::num_to_string(js::round(x * 1e5))).collect::<Vec<_>>().join(",");
        if let Some(id) = grid.get(&k) {
            return *id;
        }
        let id = d.v_add(p);
        grid.insert(k, id);
        id
    };
    for poly in polys {
        let mut lp: Vec<u64> = Vec::new();
        for p in poly {
            let v = vid(&mut d, *p);
            if lp.last() != Some(&v) {
                lp.push(v);
            }
        }
        while lp.len() > 1 && lp.first() == lp.last() {
            lp.pop();
        }
        let mut u = lp.clone();
        u.sort_unstable();
        u.dedup();
        if lp.len() < 3 || u.len() != lp.len() {
            continue;
        }
        let pts: Vec<P3> = lp.iter().map(|v| d.v(*v)).collect();
        if poly_area3(&pts) <= EPS2 {
            continue;
        }
        let n = newell(&pts);
        let p0 = pts.first().copied().unwrap_or([f64::NAN; 3]);
        let mut size = 0.0;
        for p in &pts {
            size = js::max(size, dist(*p, p0));
        }
        let tol = js::max(EPS, size * 1e-6);
        if pts.iter().any(|p| dot(n, sub(*p, p0)).abs() > tol) {
            for i in 1..lp.len().saturating_sub(1) {
                let t =
                    [lp.first().copied().unwrap_or(0), lp.get(i).copied().unwrap_or(0), lp.get(i.saturating_add(1)).copied().unwrap_or(0)];
                let tp: Vec<P3> = t.iter().map(|v| d.v(*v)).collect();
                if poly_area3(&tp) > EPS2 {
                    d.f_add(vec![t.to_vec()]);
                }
            }
            continue;
        }
        d.f_add(vec![lp]);
    }
    // edges between two faces of one plane are soft
    let mut ef: HashMap<(u64, u64), Vec<usize>> = HashMap::new();
    for (fi, (_, loops)) in d.faces.iter().enumerate() {
        for l in loops {
            let n = l.len();
            for i in 0..n {
                if let (Some(a), Some(b)) = (l.get(i), l.get(jsv::wrap_next(i, n))) {
                    ef.entry(key(*a, *b)).or_default().push(fi);
                }
            }
        }
    }
    let normals: Vec<P3> = d.faces.iter().map(|(_, l)| d.face_normal(l)).collect();
    for e in d.edges.iter_mut() {
        if let Some(fs) = ef.get(&key(e.1, e.2))
            && let [f0, f1] = fs.as_slice()
            && let (Some(n0), Some(n1)) = (normals.get(*f0), normals.get(*f1))
            && dot(*n0, *n1) > 1.0 - 1e-6
        {
            e.3 = true;
        }
    }
    d
}

const IDENT: [f64; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
fn mmul(a: &[f64; 16], b: &[f64; 16]) -> [f64; 16] {
    let mut r = [0.0; 16];
    for c in 0..4usize {
        for rr in 0..4usize {
            let mut s = 0.0;
            for k in 0..4usize {
                s += a.get(k.wrapping_mul(4).wrapping_add(rr)).copied().unwrap_or(0.0)
                    * b.get(c.wrapping_mul(4).wrapping_add(k)).copied().unwrap_or(0.0);
            }
            if let Some(slot) = r.get_mut(c.wrapping_mul(4).wrapping_add(rr)) {
                *slot = s;
            }
        }
    }
    r
}
fn apply(m: &[f64; 16], p: P3) -> P3 {
    let (x, y, z) = (p[0], p[1], p[2]);
    let w0 = m[3] * x + m[7] * y + m[11] * z + m[15];
    let w = if w0 != 0.0 && !w0.is_nan() { w0 } else { 1.0 };
    [
        (m[0] * x + m[4] * y + m[8] * z + m[12]) / w,
        (m[1] * x + m[5] * y + m[9] * z + m[13]) / w,
        (m[2] * x + m[6] * y + m[10] * z + m[14]) / w,
    ]
}

// ---------------------------------------------------------------------------------------------- earcut

#[derive(Clone)]
struct Node {
    i: usize,
    x: f64,
    y: f64,
    prev: usize,
    next: usize,
    steiner: bool,
}

struct Ear {
    n: Vec<Node>,
    tris: Vec<usize>,
    steps: usize,
}

impl Ear {
    fn nd(&self, p: usize) -> Node {
        self.n.get(p).cloned().unwrap_or(Node { i: 0, x: f64::NAN, y: f64::NAN, prev: p, next: p, steiner: false })
    }
    fn next(&self, p: usize) -> usize {
        self.n.get(p).map_or(p, |q| q.next)
    }
    fn prev(&self, p: usize) -> usize {
        self.n.get(p).map_or(p, |q| q.prev)
    }
    fn set_next(&mut self, p: usize, v: usize) {
        if let Some(q) = self.n.get_mut(p) {
            q.next = v;
        }
    }
    fn set_prev(&mut self, p: usize, v: usize) {
        if let Some(q) = self.n.get_mut(p) {
            q.prev = v;
        }
    }
    fn tick(&mut self) -> bool {
        self.steps = self.steps.saturating_add(1);
        self.steps < MAX_EAR_STEPS
    }
    fn create(&mut self, i: usize, x: f64, y: f64) -> usize {
        let id = self.n.len();
        self.n.push(Node { i, x, y, prev: id, next: id, steiner: false });
        id
    }
    fn insert(&mut self, i: usize, x: f64, y: f64, last: Option<usize>) -> usize {
        let p = self.create(i, x, y);
        if let Some(l) = last {
            let ln = self.next(l);
            self.set_next(p, ln);
            self.set_prev(p, l);
            self.set_prev(ln, p);
            self.set_next(l, p);
        }
        p
    }
    fn remove(&mut self, p: usize) {
        let (pp, pn) = (self.prev(p), self.next(p));
        self.set_prev(pn, pp);
        self.set_next(pp, pn);
    }
    fn area(&self, p: usize, q: usize, r: usize) -> f64 {
        let (p, q, r) = (self.nd(p), self.nd(q), self.nd(r));
        (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y)
    }
    fn equals(&self, a: usize, b: usize) -> bool {
        let (a, b) = (self.nd(a), self.nd(b));
        a.x == b.x && a.y == b.y
    }
    fn linked_list(&mut self, data: &[f64], start: usize, end: usize, clockwise: bool) -> Option<usize> {
        let mut last: Option<usize> = None;
        let d = |k: usize| data.get(k).copied().unwrap_or(f64::NAN);
        if clockwise == (signed_area(data, start, end) > 0.0) {
            let mut i = start;
            while i < end {
                last = Some(self.insert(i / 2, d(i), d(i.saturating_add(1)), last));
                i = i.saturating_add(2);
            }
        } else {
            let mut i = end.saturating_sub(2);
            loop {
                if i < start || end < 2 {
                    break;
                }
                last = Some(self.insert(i / 2, d(i), d(i.saturating_add(1)), last));
                if i < 2 {
                    break;
                }
                i = i.saturating_sub(2);
            }
        }
        if let Some(l) = last
            && self.equals(l, self.next(l))
        {
            self.remove(l);
            last = Some(self.next(l));
        }
        last
    }
    fn filter(&mut self, start: Option<usize>, end: Option<usize>) -> Option<usize> {
        let start = start?;
        let mut end = end.unwrap_or(start);
        let mut p = start;
        loop {
            if !self.tick() {
                return Some(end);
            }
            let mut again = false;
            let (pp, pn) = (self.prev(p), self.next(p));
            if !self.nd(p).steiner && (self.equals(p, pn) || self.area(pp, p, pn) == 0.0) {
                self.remove(p);
                p = self.prev(p);
                end = p;
                if p == self.next(p) {
                    break;
                }
                again = true;
            } else {
                p = self.next(p);
            }
            if !(again || p != end) {
                break;
            }
        }
        Some(end)
    }
    fn linked(&mut self, ear: Option<usize>, pass: u8) {
        let Some(mut ear) = ear else { return };
        let mut stop = ear;
        while self.prev(ear) != self.next(ear) {
            if !self.tick() {
                return;
            }
            let (prev, next) = (self.prev(ear), self.next(ear));
            if self.is_ear(ear) {
                let (a, b, c) = (self.nd(prev).i, self.nd(ear).i, self.nd(next).i);
                self.tris.extend([a, b, c]);
                self.remove(ear);
                ear = self.next(next);
                stop = self.next(next);
                continue;
            }
            ear = next;
            if ear == stop {
                if pass == 0 {
                    let f = self.filter(Some(ear), None);
                    self.linked(f, 1);
                } else if pass == 1 {
                    let f = self.filter(Some(ear), None);
                    let e = self.cure(f);
                    self.linked(e, 2);
                } else {
                    self.split(ear);
                }
                break;
            }
        }
    }
    fn is_ear(&self, ear: usize) -> bool {
        let (a, b, c) = (self.prev(ear), ear, self.next(ear));
        if self.area(a, b, c) >= 0.0 {
            return false;
        }
        let (an, bn, cn) = (self.nd(a), self.nd(b), self.nd(c));
        let x0 = js::min_of([an.x, bn.x, cn.x]);
        let y0 = js::min_of([an.y, bn.y, cn.y]);
        let x1 = js::max_of([an.x, bn.x, cn.x]);
        let y1 = js::max_of([an.y, bn.y, cn.y]);
        let mut p = self.next(c);
        let mut guard = 0usize;
        while p != a {
            guard = guard.saturating_add(1);
            if guard > self.n.len().saturating_mul(2).saturating_add(8) {
                return false;
            }
            let pn = self.nd(p);
            if pn.x >= x0
                && pn.x <= x1
                && pn.y >= y0
                && pn.y <= y1
                && !(an.x == pn.x && an.y == pn.y)
                && point_in_triangle(an.x, an.y, bn.x, bn.y, cn.x, cn.y, pn.x, pn.y)
                && self.area(pn.prev, p, pn.next) >= 0.0
            {
                return false;
            }
            p = pn.next;
        }
        true
    }
    fn intersects(&self, p1: usize, q1: usize, p2: usize, q2: usize) -> bool {
        let sgn = |n: f64| {
            if n > 0.0 {
                1
            } else if n < 0.0 {
                -1
            } else {
                0
            }
        };
        let o1 = sgn(self.area(p1, q1, p2));
        let o2 = sgn(self.area(p1, q1, q2));
        let o3 = sgn(self.area(p2, q2, p1));
        let o4 = sgn(self.area(p2, q2, q1));
        if o1 != o2 && o3 != o4 {
            return true;
        }
        let on = |p: usize, q: usize, r: usize| {
            let (p, q, r) = (self.nd(p), self.nd(q), self.nd(r));
            q.x <= js::max(p.x, r.x) && q.x >= js::min(p.x, r.x) && q.y <= js::max(p.y, r.y) && q.y >= js::min(p.y, r.y)
        };
        (o1 == 0 && on(p1, p2, q1)) || (o2 == 0 && on(p1, q2, q1)) || (o3 == 0 && on(p2, p1, q2)) || (o4 == 0 && on(p2, q1, q2))
    }
    fn locally_inside(&self, a: usize, b: usize) -> bool {
        let (ap, an) = (self.prev(a), self.next(a));
        if self.area(ap, a, an) < 0.0 {
            self.area(a, b, an) >= 0.0 && self.area(a, ap, b) >= 0.0
        } else {
            self.area(a, b, ap) < 0.0 || self.area(a, an, b) < 0.0
        }
    }
    fn cure(&mut self, start: Option<usize>) -> Option<usize> {
        let mut start = start?;
        let mut p = start;
        loop {
            if !self.tick() {
                break;
            }
            let a = self.prev(p);
            let b = self.next(self.next(p));
            if !self.equals(a, b) && self.intersects(a, p, self.next(p), b) && self.locally_inside(a, b) && self.locally_inside(b, a) {
                let (ai, pi, bi) = (self.nd(a).i, self.nd(p).i, self.nd(b).i);
                self.tris.extend([ai, pi, bi]);
                self.remove(p);
                let pn = self.next(p);
                self.remove(pn);
                p = b;
                start = b;
            }
            p = self.next(p);
            if p == start {
                break;
            }
        }
        self.filter(Some(p), None)
    }
    fn middle_inside(&self, a: usize, b: usize) -> bool {
        let (an, bn) = (self.nd(a), self.nd(b));
        let (px, py) = ((an.x + bn.x) / 2.0, (an.y + bn.y) / 2.0);
        let mut p = a;
        let mut inside = false;
        let mut guard = 0usize;
        loop {
            let pn = self.nd(p);
            let q = self.nd(pn.next);
            if ((pn.y > py) != (q.y > py)) && q.y != pn.y && (px < (q.x - pn.x) * (py - pn.y) / (q.y - pn.y) + pn.x) {
                inside = !inside;
            }
            p = pn.next;
            guard = guard.saturating_add(1);
            if p == a || guard > self.n.len().saturating_mul(2).saturating_add(8) {
                break;
            }
        }
        inside
    }
    fn intersects_polygon(&self, a: usize, b: usize) -> bool {
        let (ai, bi) = (self.nd(a).i, self.nd(b).i);
        let mut p = a;
        let mut guard = 0usize;
        loop {
            let pn = self.nd(p);
            let qi = self.nd(pn.next).i;
            if pn.i != ai && qi != ai && pn.i != bi && qi != bi && self.intersects(p, pn.next, a, b) {
                return true;
            }
            p = pn.next;
            guard = guard.saturating_add(1);
            if p == a || guard > self.n.len().saturating_mul(2).saturating_add(8) {
                return false;
            }
        }
    }
    fn valid_diagonal(&self, a: usize, b: usize) -> bool {
        let (an, bn) = (self.nd(a), self.nd(b));
        self.nd(an.next).i != bn.i
            && self.nd(an.prev).i != bn.i
            && !self.intersects_polygon(a, b)
            && ((self.locally_inside(a, b) && self.locally_inside(b, a) && self.middle_inside(a, b) && {
                let x = self.area(an.prev, a, bn.prev);
                let y = self.area(a, bn.prev, b);
                (x != 0.0 && !x.is_nan()) || (y != 0.0 && !y.is_nan())
            }) || (self.equals(a, b) && self.area(an.prev, a, an.next) > 0.0 && self.area(bn.prev, b, bn.next) > 0.0))
    }
    fn split_polygon(&mut self, a: usize, b: usize) -> usize {
        let (an_, bn_) = (self.nd(a), self.nd(b));
        let a2 = self.create(an_.i, an_.x, an_.y);
        let b2 = self.create(bn_.i, bn_.x, bn_.y);
        let an = self.next(a);
        let bp = self.prev(b);
        self.set_next(a, b);
        self.set_prev(b, a);
        self.set_next(a2, an);
        self.set_prev(an, a2);
        self.set_next(b2, a2);
        self.set_prev(a2, b2);
        self.set_next(bp, b2);
        self.set_prev(b2, bp);
        b2
    }
    fn split(&mut self, start: usize) {
        let mut a = start;
        loop {
            let mut b = self.next(self.next(a));
            while b != self.prev(a) {
                if !self.tick() {
                    return;
                }
                if self.nd(a).i != self.nd(b).i && self.valid_diagonal(a, b) {
                    let c = self.split_polygon(a, b);
                    let an = self.next(a);
                    let af = self.filter(Some(a), Some(an));
                    let cn = self.next(c);
                    let cf = self.filter(Some(c), Some(cn));
                    self.linked(af, 0);
                    self.linked(cf, 0);
                    return;
                }
                b = self.next(b);
            }
            a = self.next(a);
            if a == start || !self.tick() {
                break;
            }
        }
    }
}

fn signed_area(data: &[f64], start: usize, end: usize) -> f64 {
    let d = |k: usize| data.get(k).copied().unwrap_or(f64::NAN);
    let mut sum = 0.0;
    let mut i = start;
    let mut j = end.saturating_sub(2);
    while i < end {
        sum += (d(j) - d(i)) * (d(i.saturating_add(1)) + d(j.saturating_add(1)));
        j = i;
        i = i.saturating_add(2);
    }
    sum
}
#[allow(clippy::too_many_arguments)]
fn point_in_triangle(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64, px: f64, py: f64) -> bool {
    (cx - px) * (ay - py) >= (ax - px) * (cy - py)
        && (ax - px) * (by - py) >= (bx - px) * (ay - py)
        && (bx - px) * (cy - py) >= (cx - px) * (by - py)
}

/// earcut without holes (the faces here have none).
fn earcut(data: &[f64]) -> Vec<usize> {
    let mut e = Ear { n: Vec::new(), tris: Vec::new(), steps: 0 };
    let outer = e.linked_list(data, 0, data.len(), true);
    let Some(o) = outer else { return e.tris };
    if e.next(o) == e.prev(o) {
        return e.tris;
    }
    e.linked(Some(o), 0);
    e.tris
}

/// m3d `faceTris(def, f)`: a face's triangles as vertex-id triples, on its front.
fn face_tris(d: &Def, loops: &[Vec<u64>]) -> Vec<[u64; 3]> {
    let n = d.face_normal(loops);
    if len(n) == 0.0 {
        return Vec::new();
    }
    let u = if n[2].abs() > 0.999 { [1.0, 0.0, 0.0] } else { norm(cross([0.0, 0.0, 1.0], n)) };
    let v = cross(n, u);
    let mut flat = Vec::new();
    let mut ids = Vec::new();
    for l in loops.iter().take(1) {
        for vid in l {
            let p = d.v(*vid);
            flat.push(dot(p, u));
            flat.push(dot(p, v));
            ids.push(*vid);
        }
    }
    let t = earcut(&flat);
    let mut out = Vec::new();
    for c in t.chunks(3) {
        if let [a, b, cc] = c {
            let tri = [ids.get(*a).copied().unwrap_or(0), ids.get(*b).copied().unwrap_or(0), ids.get(*cc).copied().unwrap_or(0)];
            let (pa, pb, pc) = (d.v(tri[0]), d.v(tri[1]), d.v(tri[2]));
            out.push(if dot(cross(sub(pb, pa), sub(pc, pa)), n) < 0.0 { [tri[0], tri[2], tri[1]] } else { tri });
        }
    }
    out
}

fn sgn(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}
/// model3d.js `convexQuad(P)`
fn convex_quad(p: &[P3]) -> bool {
    let g = |i: usize| p.get(i & 3).copied().unwrap_or([f64::NAN; 3]);
    let mut n = [0.0; 3];
    for i in 0..4 {
        let (a, b) = (g(i), g(i.wrapping_add(1)));
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let mut sign = 0.0;
    for i in 0..4 {
        let (a, b, c) = (g(i), g(i.wrapping_add(1)), g(i.wrapping_add(2)));
        let u = sub(b, a);
        let v = sub(c, b);
        let x = cross(u, v);
        let s = sgn(x[0] * n[0] + x[1] * n[1] + x[2] * n[2]);
        if s == 0.0 || s.is_nan() {
            return false;
        }
        if sign != 0.0 && s != sign {
            return false;
        }
        sign = s;
    }
    true
}

/// model3d.js UNIT_M / mPerUnit
fn m_per_unit(units: &str) -> f64 {
    match units {
        "" | "millimeters" => 0.001,
        "inches" => 0.0254,
        "feet" => 0.3048,
        "miles" => 1609.344,
        "centimeters" => 0.01,
        "meters" => 1.0,
        "kilometers" => 1000.0,
        "microinches" => 2.54e-8,
        "mils" => 2.54e-5,
        "yards" => 0.9144,
        _ => 0.001,
    }
}
fn display_unit(units: &str) -> &'static str {
    match units {
        "inches" | "mils" | "microinches" => "in",
        "feet" | "yards" | "miles" => "ft-in",
        "centimeters" => "cm",
        "meters" | "kilometers" => "m",
        _ => "mm",
    }
}

/// One 3DFACE: four corners and the invisible-edges flags.
pub struct Face {
    pub pts: [P3; 4],
    pub invis: u32,
}
/// `forFile3d(doc)` for loose faces: { layer, faces, points (CADCRAFT xdata strings per POINT) }.
pub struct File3d {
    pub layer: &'static str,
    pub faces: Vec<Face>,
    pub points: Vec<Vec<String>>,
}

fn num_json(x: f64) -> String {
    if x.is_finite() { js::num_to_string(x) } else { "null".into() }
}

/// The 3D model a Doc gets from its loose faces (Model3D.fromFile) as a file writes it (forFile3d), or None.
pub fn for_file3d(faces3d: &[serde_json::Value], units: &str, faces_name: &str) -> Option<File3d> {
    let k = m_per_unit(units);
    let mut polys: Vec<Vec<P3>> = Vec::new();
    for f in faces3d.iter().take(200_000) {
        let fv = jsv::from_json(f);
        let mut pts: Vec<P3> = Vec::new();
        for p in jsv::arr(jsv::get(Some(&fv), "pts")) {
            let a = jsv::arr(Some(p));
            let ok = matches!(p, V::Arr(_)) && a.len() >= 3 && a.iter().all(|x| jsv::fin(Some(x)).is_some_and(|x| (x * k).abs() < 1e7));
            if !ok {
                pts.clear();
                break;
            }
            let n = |i: usize| jsv::to_num(a.get(i));
            let q = [n(0) * k, n(1) * k, n(2) * k];
            let far = match pts.last() {
                None => true,
                Some(l) => l.iter().zip(q.iter()).any(|(x, y)| (x - y).abs() > 1e-9),
            };
            if far {
                pts.push(q);
            }
        }
        if pts.len() > 1
            && let (Some(f0), Some(l)) = (pts.first(), pts.last())
            && f0.iter().zip(l.iter()).all(|(x, y)| (x - y).abs() <= 1e-9)
        {
            pts.pop();
        }
        if pts.len() >= 3 {
            polys.push(pts);
        }
    }
    if polys.is_empty() {
        return None;
    }
    let d = mesh_def(&polys, faces_name);
    let mut root = Def::new("root", "root", "Model");
    let iid = root.nid();
    root.insts.push((iid, "d1".into(), IDENT));
    // walk: root (no faces), then d1 at ident × ident
    let wm = mmul(&IDENT, &IDENT);
    let w = |v: u64| {
        let p = apply(&wm, d.v(v));
        [p[0] / k, p[1] / k, p[2] / k]
    };
    let mut faces = Vec::new();
    for (_, loops) in &d.faces {
        let Some(l0) = loops.first() else { continue };
        let quad = l0.len() == 4 && convex_quad(&l0.iter().map(|v| d.v(*v)).collect::<Vec<_>>());
        if loops.len() == 1 && (l0.len() == 3 || quad) {
            let p: Vec<P3> = l0.iter().map(|v| w(*v)).collect();
            let g = |i: usize| p.get(i).copied().unwrap_or([f64::NAN; 3]);
            let fourth = if p.len() == 3 { g(2) } else { g(3) };
            faces.push(Face { pts: [g(0), g(1), g(2), fourth], invis: 0 });
            continue;
        }
        let mut along: std::collections::HashSet<(u64, u64)> = std::collections::HashSet::new();
        for l in loops {
            let n = l.len();
            for i in 0..n {
                if let (Some(a), Some(b)) = (l.get(i), l.get(jsv::wrap_next(i, n))) {
                    along.insert((*a, *b));
                    along.insert((*b, *a));
                }
            }
        }
        for [a, b, c] in face_tris(&d, loops) {
            let vis = |p: u64, q: u64| along.contains(&(p, q));
            let invis = (if vis(a, b) { 0 } else { 1 }) | (if vis(b, c) { 0 } else { 2 }) | (if vis(c, a) { 0 } else { 8 });
            let cc = w(c);
            faces.push(Face { pts: [w(a), w(b), cc, cc], invis });
        }
    }
    // the model's JSON (toJSON), every character outside ! … ~ as \uXXXX
    let mut j = String::new();
    j.push_str(&format!("{{\"format\":\"modelcraft\",\"version\":1,\"units\":\"{}\",\"nextId\":2,\"defs\":[", display_unit(units)));
    for (di, def) in [&root, &d].iter().enumerate() {
        if di > 0 {
            j.push(',');
        }
        let mut name = String::new();
        crate::props::stringify(&V::Str(def.name.clone())).clone_into(&mut name);
        j.push_str(&format!("{{\"id\":\"{}\",\"kind\":\"{}\",\"name\":{},\"next\":{},\"verts\":[", def.id, def.kind, name, def.next));
        let vs: Vec<String> =
            def.verts.iter().map(|(id, p)| format!("[{id},{},{},{}]", num_json(p[0]), num_json(p[1]), num_json(p[2]))).collect();
        j.push_str(&vs.join(","));
        j.push_str("],\"edges\":[");
        let es: Vec<String> =
            def.edges.iter().map(|(id, a, b, soft)| if *soft { format!("[{id},{a},{b},1]") } else { format!("[{id},{a},{b}]") }).collect();
        j.push_str(&es.join(","));
        j.push_str("],\"faces\":[");
        let fs: Vec<String> = def
            .faces
            .iter()
            .map(|(id, loops)| {
                let ls: Vec<String> =
                    loops.iter().map(|l| format!("[{}]", l.iter().map(u64::to_string).collect::<Vec<_>>().join(","))).collect();
                format!("{{\"id\":{id},\"loops\":[{}]}}", ls.join(","))
            })
            .collect();
        j.push_str(&fs.join(","));
        j.push_str("],\"insts\":[");
        let is: Vec<String> = def
            .insts
            .iter()
            .map(|(id, dd, m)| {
                format!("{{\"id\":{id},\"def\":\"{dd}\",\"m\":[{}]}}", m.iter().map(|x| num_json(*x)).collect::<Vec<_>>().join(","))
            })
            .collect();
        j.push_str(&is.join(","));
        j.push_str("]}");
    }
    j.push_str("],\"guides\":[],\"dims\":[],\"scenes\":[]}");
    let mut text = String::new();
    for u in j.encode_utf16() {
        if (0x21..=0x7e).contains(&u) {
            text.push(char::from(u8::try_from(u).unwrap_or(b'?')));
        } else {
            text.push_str(&format!("\\u{u:04x}"));
        }
    }
    let mut points = Vec::new();
    let n = text.len();
    let mut i = 0usize;
    while i < n {
        let piece = text.get(i..i.saturating_add(230).min(n)).unwrap_or("");
        points.push(vec![format!("CC3D:{}:{piece}", points.len().saturating_add(1))]);
        i = i.saturating_add(230);
    }
    Some(File3d { layer: "CC-3D", faces, points })
}
