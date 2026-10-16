//! geo-mesh.js — the triangle mesh every representation item is built into. Vertices carry a normal; a planar face
//! keeps its own vertices (its normal), so edges between faces stay sharp.

use super::math::{M4, V3, cross, det3, invert, newell, norm, sub};
use super::tri::plane_triangulate;
use crate::jsmath::hypot3;

/// The longest array V8 grows (FixedDoubleArray / FixedArray kMaxLength: (1 GiB − 16) / 8): a mesh whose position or
/// index list would pass it makes the JS throw RangeError "Invalid array length". Also this crate's allocation cap.
pub const MAX_ARRAY: usize = 134_217_726;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub pos: Vec<f64>,
    pub nrm: Vec<f64>,
    pub idx: Vec<u32>,
    /// a push went past MAX_ARRAY (the JS threw there; callers turn it into the error where the JS would see it)
    pub overflow: bool,
}

#[inline]
fn u(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

impl Mesh {
    pub fn new() -> Mesh {
        Mesh::default()
    }
    pub fn vertex_count(&self) -> usize {
        self.pos.len() / 3
    }
    pub fn triangles(&self) -> usize {
        self.idx.len() / 3
    }
    pub fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }
    /// Room for n more vertices (and as many indexes) under the cap; past it the mesh is marked overflowed.
    pub fn room(&mut self, n: usize) -> bool {
        let ok = !self.overflow
            && self.pos.len().saturating_add(n.saturating_mul(3)) <= MAX_ARRAY
            && self.idx.len().saturating_add(n.saturating_mul(3)) <= MAX_ARRAY;
        if !ok {
            self.overflow = true;
        }
        ok
    }

    /// Err("Invalid array length") when the mesh overflowed (where the JS would have thrown).
    pub fn check(self) -> Result<Mesh, super::ctx::GeoErr> {
        if self.overflow { Err(super::ctx::GeoErr("Invalid array length".into())) } else { Ok(self) }
    }

    pub fn vertex(&mut self, p: &V3, n: &V3) -> u32 {
        self.pos.extend_from_slice(p);
        self.nrm.extend_from_slice(n);
        u(self.vertex_count().saturating_sub(1))
    }

    pub fn tri(&mut self, a: &V3, b: &V3, c: &V3) {
        if !self.room(3) {
            return;
        }
        let n = norm(&cross(&sub(b, a), &sub(c, a)));
        let i = self.vertex(a, &n);
        let j = self.vertex(b, &n);
        let k = self.vertex(c, &n);
        self.idx.extend_from_slice(&[i, j, k]);
    }

    /// A planar face with holes; nothing for fewer than three points. Returns the number of triangles added.
    pub fn polygon(&mut self, rings: &[Vec<V3>]) -> usize {
        let Some(first) = rings.first() else { return 0 };
        if first.len() < 3 {
            return 0;
        }
        let all = rings.iter().all(|x| x.len() >= 3);
        let filtered: Vec<Vec<V3>>;
        let r: &[Vec<V3>] = if all {
            rings
        } else {
            filtered = rings.iter().filter(|x| x.len() >= 3).cloned().collect();
            &filtered
        };
        if r.len() == 1
            && let [a, b, c] = first.as_slice()
        {
            self.tri(a, b, c);
            return 1;
        }
        let n = norm(&newell(first));
        if super::math::is_zero(&n) {
            return 0;
        }
        let tris = plane_triangulate(r);
        if tris.is_empty() {
            return 0;
        }
        let total: usize = r.iter().map(Vec::len).sum();
        if !self.room(total) {
            return 0;
        }
        let base = self.vertex_count();
        for ring in r {
            for p in ring {
                self.vertex(p, &n);
            }
        }
        self.idx.extend(tris.iter().map(|&k| u(base.saturating_add(k))));
        tris.len() / 3
    }

    /// Triangles over a shared point list (normals per vertex, or flat when absent).
    pub fn add_indexed(&mut self, pos: &[V3], idx: &[usize], nrm: Option<&[V3]>) {
        if let Some(nrm) = nrm {
            if !self.room(pos.len()) {
                return;
            }
            let base = self.vertex_count();
            for (i, p) in pos.iter().enumerate() {
                self.vertex(p, nrm.get(i).unwrap_or(&[0.0, 0.0, 1.0]));
            }
            self.idx.extend(idx.iter().map(|&k| u(base.saturating_add(k))));
            return;
        }
        for t in idx.as_chunks::<3>().0.iter() {
            if let (Some(a), Some(b), Some(c)) = (pos.get(t[0]), pos.get(t[1]), pos.get(t[2])) {
                self.tri(a, b, c);
            }
        }
    }

    /// Another mesh, moved (normals by the inverse transpose; a mirror flips the winding).
    pub fn append(&mut self, other: &Mesh, m: Option<&M4>) -> &mut Mesh {
        if other.overflow {
            self.overflow = true;
        }
        if !self.room(other.vertex_count()) {
            return self;
        }
        let base = u(self.vertex_count());
        let Some(m) = m else {
            self.pos.extend_from_slice(&other.pos);
            self.nrm.extend_from_slice(&other.nrm);
            self.idx.extend(other.idx.iter().map(|&k| base.saturating_add(k)));
            return self;
        };
        let nm = normal_matrix(m);
        let mirror = det3(m) < 0.0;
        self.pos.reserve(other.pos.len());
        self.nrm.reserve(other.nrm.len());
        for (p, n) in other.pos.as_chunks::<3>().0.iter().zip(other.nrm.as_chunks::<3>().0.iter()) {
            let (x, y, z) = (p[0], p[1], p[2]);
            self.pos.extend_from_slice(&[
                m[0] * x + m[4] * y + m[8] * z + m[12],
                m[1] * x + m[5] * y + m[9] * z + m[13],
                m[2] * x + m[6] * y + m[10] * z + m[14],
            ]);
            let (nx, ny, nz) = (n[0], n[1], n[2]);
            let tx = nm[0] * nx + nm[3] * ny + nm[6] * nz;
            let ty = nm[1] * nx + nm[4] * ny + nm[7] * nz;
            let tz = nm[2] * nx + nm[5] * ny + nm[8] * nz;
            let l = super::math::or(hypot3(tx, ty, tz), 1.0);
            self.nrm.extend_from_slice(&[tx / l, ty / l, tz / l]);
        }
        self.idx.reserve(other.idx.len());
        for t in other.idx.as_chunks::<3>().0.iter() {
            let (a, b, c) = (base.saturating_add(t[0]), base.saturating_add(t[1]), base.saturating_add(t[2]));
            if mirror {
                self.idx.extend_from_slice(&[a, c, b]);
            } else {
                self.idx.extend_from_slice(&[a, b, c]);
            }
        }
        self
    }

    pub fn transform(&mut self, m: &M4) -> &mut Mesh {
        let mut out = Mesh::new();
        out.append(self, Some(m));
        *self = out;
        self
    }

    pub fn flip(&mut self) -> &mut Mesh {
        for t in self.idx.as_chunks_mut::<3>().0.iter_mut() {
            t.swap(1, 2);
        }
        for v in &mut self.nrm {
            *v = -*v;
        }
        self
    }

    pub fn bounds(&self) -> Option<(V3, V3)> {
        if self.pos.is_empty() {
            return None;
        }
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in self.pos.as_chunks::<3>().0.iter() {
            for ((l, h), &v) in lo.iter_mut().zip(hi.iter_mut()).zip(p.iter()) {
                if v < *l {
                    *l = v;
                }
                if v > *h {
                    *h = v;
                }
            }
        }
        Some((lo, hi))
    }

    /// Signed volume (divergence theorem); meaningful for closed, outward-facing meshes.
    pub fn volume(&self) -> f64 {
        let p = |k: u32, o: usize| {
            self.pos.get(usize::try_from(k).unwrap_or(usize::MAX).saturating_mul(3).saturating_add(o)).copied().unwrap_or(f64::NAN)
        };
        let mut v = 0.0;
        for t in self.idx.as_chunks::<3>().0.iter() {
            let (a, b, c) = (t[0], t[1], t[2]);
            v += p(a, 0) * (p(b, 1) * p(c, 2) - p(b, 2) * p(c, 1)) - p(a, 1) * (p(b, 0) * p(c, 2) - p(b, 2) * p(c, 0))
                + p(a, 2) * (p(b, 0) * p(c, 1) - p(b, 1) * p(c, 0));
        }
        v / 6.0
    }

    /// Shared vertices: points closer than eps (per axis grid) become one; degenerate triangles dropped (for booleans).
    pub fn welded(&self, eps: f64) -> (Vec<f64>, Vec<u32>) {
        let mut map: std::collections::HashMap<(i64, i64, i64), u32> = std::collections::HashMap::new();
        let mut pos = Vec::new();
        let mut remap = Vec::with_capacity(self.vertex_count());
        let inv = 1.0 / eps;
        let key = |x: f64| -> i64 {
            let r = crate::jsmath::round(x * inv);
            // a grid key; non-finite and huge values share the extremes
            if r.is_nan() {
                0
            } else if r >= 9.2e18 {
                i64::MAX
            } else if r <= -9.2e18 {
                i64::MIN
            } else {
                // in range by the checks above
                #[allow(clippy::cast_possible_truncation)]
                let k = r as i64;
                k
            }
        };
        for p in self.pos.as_chunks::<3>().0.iter() {
            let k = (key(p[0]), key(p[1]), key(p[2]));
            let id = *map.entry(k).or_insert_with(|| {
                let id = u(pos.len() / 3);
                pos.extend_from_slice(p);
                id
            });
            remap.push(id);
        }
        let mut idx = Vec::new();
        for t in self.idx.as_chunks::<3>().0.iter() {
            let g = |k: u32| remap.get(usize::try_from(k).unwrap_or(usize::MAX)).copied().unwrap_or(u32::MAX);
            let (a, b, c) = (g(t[0]), g(t[1]), g(t[2]));
            if a != b && b != c && a != c {
                idx.extend_from_slice(&[a, b, c]);
            }
        }
        (pos, idx)
    }

    /// Back from a boolean result (flat normals).
    pub fn from_welded(pos: &[f64], idx: &[u32]) -> Mesh {
        let mut m = Mesh::new();
        let p = |k: u32| -> Option<V3> {
            let i = usize::try_from(k).ok()?.checked_mul(3)?;
            Some([*pos.get(i)?, *pos.get(i.checked_add(1)?)?, *pos.get(i.checked_add(2)?)?])
        };
        for t in idx.as_chunks::<3>().0.iter() {
            if let (Some(a), Some(b), Some(c)) = (p(t[0]), p(t[1]), p(t[2])) {
                m.tri(&a, &b, &c);
            }
        }
        m
    }
}

/// The 3 × 3 inverse transpose of m's linear part (column-major), for normals.
fn normal_matrix(m: &M4) -> [f64; 9] {
    let inv = invert(m).unwrap_or(*m);
    [inv[0], inv[4], inv[8], inv[1], inv[5], inv[9], inv[2], inv[6], inv[10]]
}
