//! clash.js — hard clashes between two sets of IFC elements (0.4 B17): a sweep over element boxes, then Möller's
//! triangle–triangle test per candidate pair (faces that only touch, in one plane or along an edge, do not count).
//! clash-worker.js only runs `clashes` off the page; here it is a plain call.

use std::collections::HashSet;

use cc_io_ifc::model::id_key;
use serde_json::{Map, Value as J};

use crate::js::{OMap, f32_of, jn, max, min};
use crate::model::Batch;

/// `P[i]` of a Float32Array read as a JS number: NaN past the end or at a non-index (undefined in arithmetic).
pub fn at_f32(p: &[f32], i: f64) -> f64 {
    if !(i >= 0.0) || i.fract() != 0.0 || i >= 4_294_967_296.0 {
        return f64::NAN;
    }
    // a whole number below 2^32: exact
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let k = i as u32;
    usize::try_from(k).ok().and_then(|k| p.get(k)).map_or(f64::NAN, |&x| f64::from(x))
}

/// One element: its key, bounds (the transformed vertices before the Float32 store) and world triangles.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub key: f64,
    pub bx: [f64; 6],
    pub tris: Vec<f32>,
}

impl Element {
    pub fn json(&self, digest: bool) -> J {
        let mut o = Map::new();
        o.insert("key".into(), jn(self.key));
        o.insert("box".into(), J::Array(self.bx.iter().map(|x| jn(*x)).collect()));
        if digest {
            o.insert("tris".into(), crate::digest::f32s(&self.tris));
        } else {
            o.insert("tris".into(), J::Array(self.tris.iter().map(|x| jn(f64::from(*x))).collect()));
        }
        J::Object(o)
    }
}

/// `elementsOf(batches, keys, matrixOf)`: (batch, its matrix) pairs.
pub fn elements_of(batches: &[(&Batch, Option<&[f64; 16]>)], keys: &[f64]) -> Vec<Element> {
    let want: HashSet<u64> = keys.iter().map(|k| id_key(*k)).collect();
    let mut map: OMap<u64, Element> = OMap::new();
    for (b, m) in batches {
        let e = &b.elems;
        for (r, &key) in e.ids.iter().enumerate() {
            let kf = f64::from(key);
            let kk = id_key(kf);
            if !want.contains(&kk) {
                continue;
            }
            let Some(el) = map.entry(kk, || Element {
                key: kf,
                bx: [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY],
                tris: Vec::new(),
            }) else {
                continue;
            };
            let tn = usize::try_from(e.tn.get(r).copied().unwrap_or(0)).unwrap_or(0);
            let t0 = usize::try_from(e.t0.get(r).copied().unwrap_or(0)).unwrap_or(usize::MAX);
            el.tris.reserve(tn.saturating_mul(9));
            for k in 0..tn.saturating_mul(3) {
                let tt = t0.saturating_mul(3).saturating_add(k);
                let v = b.indices.get(tt).map_or(f64::NAN, |&i| f64::from(i) * 3.0);
                let (mut x, mut y, mut z) = (at_f32(&b.positions, v), at_f32(&b.positions, v + 1.0), at_f32(&b.positions, v + 2.0));
                if let Some(mm) = m {
                    let xx = mm[0] * x + mm[4] * y + mm[8] * z + mm[12];
                    let yy = mm[1] * x + mm[5] * y + mm[9] * z + mm[13];
                    let zz = mm[2] * x + mm[6] * y + mm[10] * z + mm[14];
                    (x, y, z) = (xx, yy, zz);
                }
                el.tris.push(f32_of(x));
                el.tris.push(f32_of(y));
                el.tris.push(f32_of(z));
                let bx = &mut el.bx;
                if x < bx[0] {
                    bx[0] = x;
                }
                if y < bx[1] {
                    bx[1] = y;
                }
                if z < bx[2] {
                    bx[2] = z;
                }
                if x > bx[3] {
                    bx[3] = x;
                }
                if y > bx[4] {
                    bx[4] = y;
                }
                if z > bx[5] {
                    bx[5] = z;
                }
            }
        }
    }
    map.vals
}

fn overlap(a: &[f64; 6], b: &[f64; 6]) -> f64 {
    let dx = min(a[3], b[3]) - max(a[0], b[0]);
    if dx < 0.0 {
        return -1.0;
    }
    min(min(dx, min(a[4], b[4]) - max(a[1], b[1])), min(a[5], b[5]) - max(a[2], b[2]))
}

/// One clash.
#[derive(Clone, Debug, PartialEq)]
pub struct Clash {
    pub a: f64,
    pub b: f64,
    pub point: [f64; 3],
    pub depth: f64,
}

impl Clash {
    pub fn json(&self) -> J {
        let mut o = Map::new();
        o.insert("a".into(), jn(self.a));
        o.insert("b".into(), jn(self.b));
        o.insert("point".into(), J::Array(self.point.iter().map(|x| jn(*x)).collect()));
        o.insert("depth".into(), jn(self.depth));
        J::Object(o)
    }
}

/// A cap on candidate pairs held at once (the JS has none; a hostile pair of sets cannot make this unbounded).
const MAX_PAIRS: usize = 50_000_000;

/// `clashes(A, B, { tolerance })`, sorted deepest first.
pub fn clashes(a: &[Element], b: &[Element], tolerance: f64) -> Vec<Clash> {
    let mut all: Vec<(&Element, u8)> = a.iter().map(|e| (e, 0u8)).chain(b.iter().map(|e| (e, 1u8))).collect();
    all.sort_by(|p, q| crate::js::sign_ord(p.0.bx[0] - q.0.bx[0]));
    let mut pairs: Vec<(&Element, &Element, f64)> = Vec::new();
    let mut active: Vec<(&Element, u8)> = Vec::new();
    for it in &all {
        active.retain(|o| !(o.0.bx[3] < it.0.bx[0]));
        for o in &active {
            if o.1 == it.1 || o.0.key == it.0.key {
                continue;
            }
            let d = overlap(&o.0.bx, &it.0.bx);
            if d > tolerance && pairs.len() < MAX_PAIRS {
                pairs.push(if o.1 == 0 { (o.0, it.0, d) } else { (it.0, o.0, d) });
            }
        }
        active.push(*it);
    }
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut out = Vec::new();
    for (ea, eb, d) in pairs {
        let (lo, hi) = if ea.key < eb.key { (ea.key, eb.key) } else { (eb.key, ea.key) };
        if !seen.insert((id_key(lo), id_key(hi))) {
            continue;
        }
        if let Some(p) = crossing(ea, eb) {
            out.push(Clash { a: ea.key, b: eb.key, point: p, depth: d });
        }
    }
    out.sort_by(|x, y| crate::js::sign_ord(y.depth - x.depth));
    out
}

fn tri_box(t: &[f32], i: usize) -> [f64; 6] {
    let g = |k: usize| t.get(i.saturating_mul(9).saturating_add(k)).map_or(f64::NAN, |&x| f64::from(x));
    [
        min(min(g(0), g(3)), g(6)),
        min(min(g(1), g(4)), g(7)),
        min(min(g(2), g(5)), g(8)),
        max(max(g(0), g(3)), g(6)),
        max(max(g(1), g(4)), g(7)),
        max(max(g(2), g(5)), g(8)),
    ]
}

/// A point where the two elements' surfaces cross, or None.
fn crossing(a: &Element, b: &Element) -> Option<[f64; 3]> {
    let ab = &a.bx;
    let nb = b.tris.len() / 9;
    let mut near = Vec::new();
    for j in 0..nb {
        let q = tri_box(&b.tris, j);
        if q[0] <= ab[3] && q[3] >= ab[0] && q[1] <= ab[4] && q[4] >= ab[1] && q[2] <= ab[5] && q[5] >= ab[2] {
            near.push((j, q));
        }
    }
    if near.is_empty() {
        return None;
    }
    let na = a.tris.len() / 9;
    for i in 0..na {
        let p = tri_box(&a.tris, i);
        for (j, q) in &near {
            if p[0] > q[3] || p[3] < q[0] || p[1] > q[4] || p[4] < q[1] || p[2] > q[5] || p[5] < q[2] {
                continue;
            }
            if let Some(hit) = tri_tri(&a.tris, i.saturating_mul(9), &b.tris, j.saturating_mul(9)) {
                return Some(hit);
            }
        }
    }
    None
}

type V = [f64; 3];

fn g3(t: &[f32], o: usize) -> V {
    let g = |k: usize| t.get(o.saturating_add(k)).map_or(f64::NAN, |&x| f64::from(x));
    [g(0), g(1), g(2)]
}
fn sub(a: &V, b: &V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: &V, b: &V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: &V, b: &V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `triTri(T, o, U, u)`: Möller's test → a point of the crossing, or None.
pub fn tri_tri(t: &[f32], o: usize, u_: &[f32], u: usize) -> Option<[f64; 3]> {
    const EPS: f64 = 1e-9;
    let (t0, t1, t2) = (g3(t, o), g3(t, o.saturating_add(3)), g3(t, o.saturating_add(6)));
    let (u0, u1, u2) = (g3(u_, u), g3(u_, u.saturating_add(3)), g3(u_, u.saturating_add(6)));
    let n2 = cross(&sub(&u1, &u0), &sub(&u2, &u0));
    let d2 = -dot(&n2, &u0);
    let snap = |x: f64| if x.abs() < EPS { 0.0 } else { x };
    let dv = [snap(dot(&n2, &t0) + d2), snap(dot(&n2, &t1) + d2), snap(dot(&n2, &t2) + d2)];
    let same_side = |d: &[f64; 3]| (d[0] > 0.0 && d[1] > 0.0 && d[2] > 0.0) || (d[0] < 0.0 && d[1] < 0.0 && d[2] < 0.0);
    if same_side(&dv) {
        return None;
    }
    let n1 = cross(&sub(&t1, &t0), &sub(&t2, &t0));
    let d1 = -dot(&n1, &t0);
    let du = [snap(dot(&n1, &u0) + d1), snap(dot(&n1, &u1) + d1), snap(dot(&n1, &u2) + d1)];
    if same_side(&du) {
        return None;
    }
    if dv.iter().all(|&x| x == 0.0) || du.iter().all(|&x| x == 0.0) {
        return None;
    }
    let straddles = |d: &[f64; 3]| d.iter().any(|&x| x > 0.0) && d.iter().any(|&x| x < 0.0);
    if !straddles(&dv) || !straddles(&du) {
        return None;
    }
    let seg = |pts3: [&V; 3], d: &[f64; 3]| -> Option<(V, V)> {
        let mut pts: Vec<V> = Vec::new();
        for (k, k1) in [(0usize, 1usize), (1, 2), (2, 0)] {
            let (Some(&ai), Some(&aj), Some(&di), Some(&dj)) = (pts3.get(k), pts3.get(k1), d.get(k), d.get(k1)) else { continue };
            if di == 0.0 {
                pts.push(*ai);
            }
            if (di > 0.0 && dj < 0.0) || (di < 0.0 && dj > 0.0) {
                let tt = di / (di - dj);
                pts.push([ai[0] + tt * (aj[0] - ai[0]), ai[1] + tt * (aj[1] - ai[1]), ai[2] + tt * (aj[2] - ai[2])]);
            }
        }
        match (pts.first(), pts.last()) {
            (Some(f), Some(l)) => Some((*f, *l)),
            _ => None,
        }
    };
    let s1 = seg([&t0, &t1, &t2], &dv)?;
    let s2 = seg([&u0, &u1, &u2], &du)?;
    let dd = cross(&n1, &n2);
    let l = dot(&dd, &dd);
    if l < 1e-24 {
        return None;
    }
    let pr = |p: &V| dot(&dd, p);
    let (a0, a1) = (min(pr(&s1.0), pr(&s1.1)), max(pr(&s1.0), pr(&s1.1)));
    let (b0, b1) = (min(pr(&s2.0), pr(&s2.1)), max(pr(&s2.0), pr(&s2.1)));
    let lo = max(a0, b0);
    let hi = min(a1, b1);
    let tol = 1e-9 * l.sqrt();
    if hi < lo - tol {
        return None;
    }
    let m = (lo + hi) / 2.0;
    let (p0, p1) = (pr(&s1.0), pr(&s1.1));
    let tt = if (p1 - p0).abs() > 1e-18 { (m - p0) / (p1 - p0) } else { 0.0 };
    let c = max(0.0, min(1.0, tt));
    Some([s1.0[0] + c * (s1.1[0] - s1.0[0]), s1.0[1] + c * (s1.1[1] - s1.0[1]), s1.0[2] + c * (s1.1[2] - s1.0[2])])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_pairs() {
        let flat = [0.0f32, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0, 0.0];
        assert!(tri_tri(&flat, 0, &[0.5, 0.5, -1.0, 0.5, 0.5, 1.0, 1.5, 0.2, 0.0], 0).is_some());
        assert!(tri_tri(&flat, 0, &[0.5, 0.5, 0.1, 1.5, 0.5, 0.1, 0.5, 1.5, 0.1], 0).is_none());
        assert!(tri_tri(&flat, 0, &[0.2, 0.2, 0.0, 1.0, 0.2, 0.0, 0.2, 1.0, 0.0], 0).is_none());
        assert!(tri_tri(&flat, 0, &[3.0, 3.0, -1.0, 3.0, 3.0, 1.0, 4.0, 3.0, 0.0], 0).is_none());
    }
}
