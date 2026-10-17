//! ifc-vis.js — which IFC elements are drawn, and how: hide, isolate, see-through others (0.4 B12); each batch's
//! index buffer in draw order (solid first, see-through after, hidden left out), per-element feature edges, bounds.

use std::collections::HashMap;

use cc_io_ifc::model::id_key;
use serde_json::{Map, Value as J};

use crate::js::{hypot3, jn, round};
use crate::model::Batch;

pub const HIDDEN: u8 = 0;
pub const SOLID: u8 = 1;
pub const GHOST: u8 = 2;

/// A JS Set of keys (insertion order; a key deleted and added again goes to the end).
#[derive(Clone, Debug, Default)]
pub struct KeySet {
    slots: Vec<Option<f64>>,
    at: HashMap<u64, usize>,
}

impl KeySet {
    pub fn new() -> KeySet {
        KeySet::default()
    }
    pub fn from_keys(keys: &[f64]) -> KeySet {
        let mut s = KeySet::new();
        for &k in keys {
            s.add(k);
        }
        s
    }
    pub fn has(&self, k: f64) -> bool {
        self.at.contains_key(&id_key(k))
    }
    /// true when it was not there
    pub fn add(&mut self, k: f64) -> bool {
        let kk = id_key(k);
        if self.at.contains_key(&kk) {
            return false;
        }
        self.at.insert(kk, self.slots.len());
        self.slots.push(Some(k));
        true
    }
    pub fn delete(&mut self, k: f64) -> bool {
        match self.at.remove(&id_key(k)) {
            Some(i) => {
                if let Some(s) = self.slots.get_mut(i) {
                    *s = None;
                }
                true
            }
            None => false,
        }
    }
    pub fn len(&self) -> usize {
        self.at.len()
    }
    pub fn is_empty(&self) -> bool {
        self.at.is_empty()
    }
    pub fn keys(&self) -> Vec<f64> {
        self.slots.iter().flatten().copied().collect()
    }
    pub fn clear(&mut self) {
        self.slots.clear();
        self.at.clear();
    }
}

/// `new Visibility()`.
#[derive(Clone, Debug, Default)]
pub struct Visibility {
    pub hidden: KeySet,
    pub isolated: Option<KeySet>,
    pub ghost: bool,
    pub version: u64,
}

impl Visibility {
    pub fn plain(&self) -> bool {
        self.hidden.is_empty() && self.isolated.is_none()
    }
    pub fn state(&self, key: f64) -> u8 {
        if self.hidden.has(key) {
            return HIDDEN;
        }
        match &self.isolated {
            None => SOLID,
            Some(s) if s.has(key) => SOLID,
            Some(_) => {
                if self.ghost {
                    GHOST
                } else {
                    HIDDEN
                }
            }
        }
    }
    pub fn hide(&mut self, keys: &[f64]) -> usize {
        let n = keys.iter().filter(|&&k| self.hidden.add(k)).count();
        if n > 0 {
            self.version = self.version.saturating_add(1);
        }
        n
    }
    pub fn unhide(&mut self, keys: &[f64]) -> usize {
        let n = keys.iter().filter(|&&k| self.hidden.delete(k)).count();
        if n > 0 {
            self.version = self.version.saturating_add(1);
        }
        n
    }
    pub fn isolate(&mut self, keys: &[f64], ghost: bool) {
        let s = KeySet::from_keys(keys);
        for k in s.keys() {
            self.hidden.delete(k);
        }
        self.isolated = Some(s);
        self.ghost = ghost;
        self.version = self.version.saturating_add(1);
    }
    pub fn show_all(&mut self) {
        self.hidden.clear();
        self.isolated = None;
        self.ghost = false;
        self.version = self.version.saturating_add(1);
    }
    pub fn to_json(&self) -> J {
        let mut o = Map::new();
        o.insert("hidden".into(), J::Array(self.hidden.keys().into_iter().map(jn).collect()));
        o.insert("isolated".into(), self.isolated.as_ref().map_or(J::Null, |s| J::Array(s.keys().into_iter().map(jn).collect())));
        o.insert("ghost".into(), J::Bool(self.ghost));
        J::Object(o)
    }
    /// `load(s)` from plain data.
    pub fn load(&mut self, hidden: &[f64], isolated: Option<&[f64]>, ghost: bool) {
        self.hidden = KeySet::from_keys(hidden);
        self.isolated = isolated.map(KeySet::from_keys);
        self.ghost = ghost;
        self.version = self.version.saturating_add(1);
    }
    /// `prune(keep)`: forget keys that are gone.
    pub fn prune(&mut self, keep: impl Fn(f64) -> bool) -> usize {
        let mut n = 0usize;
        for k in self.hidden.keys() {
            if !keep(k) {
                self.hidden.delete(k);
                n = n.saturating_add(1);
            }
        }
        if let Some(iso) = &mut self.isolated {
            for k in iso.keys() {
                if !keep(k) {
                    iso.delete(k);
                    n = n.saturating_add(1);
                }
            }
        }
        if n > 0 {
            self.version = self.version.saturating_add(1);
        }
        n
    }
}

/// `batchIndex(batch, vis)`: (index, triIds, nSolid, nGhost, own = the batch's own buffers).
pub fn batch_index(batch: &Batch, vis: Option<&Visibility>) -> (Vec<u32>, Vec<u32>, u64, u64, bool) {
    let all = u64::try_from(batch.indices.len() / 3).unwrap_or(u64::MAX);
    let own = || (batch.indices.clone(), batch.tri_ids.clone(), all, 0u64, true);
    let Some(vis) = vis.filter(|v| !v.plain()) else { return own() };
    let e = &batch.elems;
    let st: Vec<u8> = e.ids.iter().map(|&k| vis.state(f64::from(k))).collect();
    let (mut n_solid, mut n_ghost) = (0u64, 0u64);
    for (r, s) in st.iter().enumerate() {
        let tn = u64::from(e.tn.get(r).copied().unwrap_or(0));
        if *s == SOLID {
            n_solid = n_solid.saturating_add(tn);
        } else if *s == GHOST {
            n_ghost = n_ghost.saturating_add(tn);
        }
    }
    if n_solid == all {
        return own();
    }
    let total = usize::try_from(n_solid.saturating_add(n_ghost)).unwrap_or(0);
    let mut index = vec![0u32; total.saturating_mul(3)];
    let mut tri_ids = vec![0u32; total];
    let (mut s, mut g) = (0usize, usize::try_from(n_solid).unwrap_or(0));
    for (r, st_r) in st.iter().enumerate() {
        if *st_r == HIDDEN {
            continue;
        }
        let t0 = usize::try_from(e.t0.get(r).copied().unwrap_or(0)).unwrap_or(0);
        let tn = usize::try_from(e.tn.get(r).copied().unwrap_or(0)).unwrap_or(0);
        let at = if *st_r == SOLID { s } else { g };
        // index.set(src.subarray(t0 * 3, (t0 + tn) * 3), at * 3): the subarray clamps to the source
        let src = batch
            .indices
            .get(t0.saturating_mul(3).min(batch.indices.len())..t0.saturating_add(tn).saturating_mul(3).min(batch.indices.len()))
            .unwrap_or(&[]);
        if let Some(dst) = index.get_mut(at.saturating_mul(3)..at.saturating_mul(3).saturating_add(src.len())) {
            dst.copy_from_slice(src);
        }
        let k = e.ids.get(r).copied().unwrap_or(0);
        let end = at.saturating_add(tn).min(tri_ids.len());
        if let Some(dst) = tri_ids.get_mut(at.min(end)..end) {
            dst.fill(k);
        }
        if *st_r == SOLID {
            s = s.saturating_add(tn);
        } else {
            g = g.saturating_add(tn);
        }
    }
    (index, tri_ids, n_solid, n_ghost, false)
}

/// Per-element edges of a batch: positions, ids, s0, sn.
#[derive(Clone, Debug, Default)]
pub struct Edges {
    pub positions: Vec<f32>,
    pub ids: Vec<u32>,
    pub s0: Vec<u32>,
    pub sn: Vec<u32>,
}

/// `batchEdges(batch, angle)`.
pub fn batch_edges(batch: &Batch, angle: f64) -> Edges {
    let e = &batch.elems;
    let mut out: Vec<f64> = Vec::new();
    let mut ed = Edges::default();
    for (r, &k) in e.ids.iter().enumerate() {
        let start = out.len() / 6;
        run_edges(&batch.positions, &batch.indices, e.t0.get(r).copied().unwrap_or(0), e.tn.get(r).copied().unwrap_or(0), angle, &mut out);
        let count = (out.len() / 6).saturating_sub(start);
        if count > 0 {
            ed.ids.push(k);
            ed.s0.push(u32::try_from(start).unwrap_or(u32::MAX));
            ed.sn.push(u32::try_from(count).unwrap_or(u32::MAX));
        }
    }
    ed.positions = out.iter().map(|x| crate::js::f32_of(*x)).collect();
    ed
}

/// A quantised vertex: `Math.round(P * 1e4)` per coordinate, compared as their strings are (−0 = 0, NaN = NaN).
fn qkey(x: f64) -> u64 {
    if x.is_nan() {
        u64::MAX
    } else if x == 0.0 {
        0
    } else {
        x.to_bits()
    }
}

/// `runEdges(P, I, t0, tn, angle, out)`: one element's feature edges appended as x y z x y z.
pub fn run_edges(p: &[f32], ix: &[u32], t0: u32, tn: u32, angle: f64, out: &mut Vec<f64>) {
    let cos = cc_io_ifc::jsmath::fd_cos(angle * std::f64::consts::PI / 180.0);
    let q = 1e4;
    let pf = |i: f64| crate::clash::at_f32(p, i);
    let ii = |t: u64| usize::try_from(t).ok().and_then(|t| ix.get(t)).map_or(f64::NAN, |&v| f64::from(v) * 3.0);
    // "a|b" → slot; slots keep the Map's insertion order (deleted ones are None)
    let mut at: HashMap<[u64; 6], usize> = HashMap::new();
    let mut slots: Vec<Option<(f64, f64, [f64; 3])>> = Vec::new();
    let end = u64::from(t0).saturating_add(u64::from(tn));
    for t in u64::from(t0)..end {
        let (a, b, c) = (ii(t.saturating_mul(3)), ii(t.saturating_mul(3).saturating_add(1)), ii(t.saturating_mul(3).saturating_add(2)));
        let (ux, uy, uz) = (pf(b) - pf(a), pf(b + 1.0) - pf(a + 1.0), pf(b + 2.0) - pf(a + 2.0));
        let (vx, vy, vz) = (pf(c) - pf(a), pf(c + 1.0) - pf(a + 1.0), pf(c + 2.0) - pf(a + 2.0));
        let mut n = [uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx];
        let l = hypot3(n[0], n[1], n[2]);
        if !(l > 0.0) {
            continue;
        }
        n = [n[0] / l, n[1] / l, n[2] / l];
        let v = [a, b, c];
        let h = v.map(|o| [qkey(round(pf(o) * q)), qkey(round(pf(o + 1.0) * q)), qkey(round(pf(o + 2.0) * q))]);
        for (k, k1) in [(0usize, 1usize), (1, 2), (2, 0)] {
            let (Some(&i0), Some(&i1), Some(&h0), Some(&h1)) = (v.get(k), v.get(k1), h.get(k), h.get(k1)) else { continue };
            if h0 == h1 {
                continue;
            }
            // the key is the unordered pair (the JS orders the two strings; equality is all that matters)
            let key = if h0 < h1 { [h0[0], h0[1], h0[2], h1[0], h1[1], h1[2]] } else { [h1[0], h1[1], h1[2], h0[0], h0[1], h0[2]] };
            match at.remove(&key) {
                None => {
                    at.insert(key, slots.len());
                    slots.push(Some((i0, i1, n)));
                }
                Some(si) => {
                    let other = slots.get_mut(si).and_then(Option::take);
                    if let Some((_, _, on)) = other
                        && on[0] * n[0] + on[1] * n[1] + on[2] * n[2] <= cos
                    {
                        out.extend_from_slice(&[pf(i0), pf(i0 + 1.0), pf(i0 + 2.0), pf(i1), pf(i1 + 1.0), pf(i1 + 2.0)]);
                    }
                }
            }
        }
    }
    for (i0, i1, _) in slots.into_iter().flatten() {
        out.extend_from_slice(&[pf(i0), pf(i0 + 1.0), pf(i0 + 2.0), pf(i1), pf(i1 + 1.0), pf(i1 + 2.0)]);
    }
}

/// `edgeIndex(edges, vis)` → (index, nSolid, nGhost, all).
pub fn edge_index(edges: &Edges, vis: Option<&Visibility>) -> (Vec<u32>, u64, u64, u64) {
    let all = u64::try_from(edges.positions.len() / 6).unwrap_or(0);
    let st: Vec<u8> = edges.ids.iter().map(|&k| vis.map_or(SOLID, |v| v.state(f64::from(k)))).collect();
    let (mut n_solid, mut n_ghost) = (0u64, 0u64);
    for (r, s) in st.iter().enumerate() {
        let sn = u64::from(edges.sn.get(r).copied().unwrap_or(0));
        if *s == SOLID {
            n_solid = n_solid.saturating_add(sn);
        } else if *s == GHOST {
            n_ghost = n_ghost.saturating_add(sn);
        }
    }
    let total = usize::try_from(n_solid.saturating_add(n_ghost)).unwrap_or(0);
    let mut index = vec![0u32; total.saturating_mul(2)];
    let (mut s, mut g) = (0u64, n_solid);
    for (r, st_r) in st.iter().enumerate() {
        if *st_r == HIDDEN {
            continue;
        }
        let at = if *st_r == SOLID { s } else { g };
        let sn = u64::from(edges.sn.get(r).copied().unwrap_or(0));
        let s0 = u64::from(edges.s0.get(r).copied().unwrap_or(0));
        for j in 0..sn {
            let seg = s0.saturating_add(j);
            let pos = usize::try_from(at.saturating_add(j).saturating_mul(2)).unwrap_or(usize::MAX);
            // Uint32Array stores wrap
            if let Some(x) = index.get_mut(pos) {
                *x = crate::model::u32_of(f64::from(u32::try_from(seg.saturating_mul(2)).unwrap_or(u32::MAX)));
            }
            if let Some(x) = index.get_mut(pos.saturating_add(1)) {
                *x = u32::try_from(seg.saturating_mul(2).saturating_add(1)).unwrap_or(u32::MAX);
            }
        }
        if *st_r == SOLID {
            s = s.saturating_add(sn);
        } else {
            g = g.saturating_add(sn);
        }
    }
    (index, n_solid, n_ghost, all)
}

/// `boundsOf(batches, keys)` → (min, max) | None.
pub fn bounds_of(batches: &[&Batch], keys: &[f64]) -> Option<([f64; 3], [f64; 3])> {
    let want: std::collections::HashSet<u64> = keys.iter().map(|k| id_key(*k)).collect();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut any = false;
    for b in batches {
        let e = &b.elems;
        for (r, &k) in e.ids.iter().enumerate() {
            if !want.contains(&id_key(f64::from(k))) {
                continue;
            }
            let v0 = usize::try_from(e.v0.get(r).copied().unwrap_or(0)).unwrap_or(0);
            let vn = usize::try_from(e.vn.get(r).copied().unwrap_or(0)).unwrap_or(0);
            for v in v0..v0.saturating_add(vn) {
                for (c, (l, h)) in lo.iter_mut().zip(hi.iter_mut()).enumerate() {
                    let x = b.positions.get(v.saturating_mul(3).saturating_add(c)).map_or(f64::NAN, |&x| f64::from(x));
                    if x < *l {
                        *l = x;
                    }
                    if x > *h {
                        *h = x;
                    }
                }
            }
            any = true;
        }
    }
    any.then_some((lo, hi))
}

/// `keysOf(batches)` in Set order.
pub fn keys_of(batches: &[&Batch]) -> Vec<f64> {
    let mut s = KeySet::new();
    for b in batches {
        for &k in &b.elems.ids {
            s.add(f64::from(k));
        }
    }
    s.keys()
}
