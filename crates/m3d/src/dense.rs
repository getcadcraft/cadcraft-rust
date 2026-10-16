//! Scratch structures keyed by ids, for the kernel's hot loops: a set ([`Mark`]), a map ([`DMap`]) and an adjacency
//! list ([`Adj`], compressed rows: one allocation for all lists instead of one Vec per key). They are hashed (ids grow
//! with every edit, so arrays indexed by id would be mostly empty). Lists keep the order entries were added in, so code
//! built on them visits things in the JS's order.

use std::hash::Hash;

use crate::omap::{FxMap, IdMap, kb};
use crate::vec::P3;

/// The array index of a small whole id (IdMap's fast path).
#[inline]
pub fn dense_of(k: f64) -> Option<usize> {
    if (0.0..1_048_576.0).contains(&k) {
        // in range by the check above: 0 ≤ k < 2^20
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let i = k as u32;
        if f64::from(i) == k {
            return usize::try_from(i).ok();
        }
    }
    None
}

/// A set of ids.
#[derive(Default)]
pub struct Mark(FxMap<u64, ()>);
impl Mark {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_capacity(n: usize) -> Self {
        Self(FxMap::with_capacity_and_hasher(n, Default::default()))
    }
    #[inline]
    pub fn add(&mut self, k: f64) {
        self.0.insert(kb(k), ());
    }
    #[inline]
    pub fn has(&self, k: f64) -> bool {
        self.0.contains_key(&kb(k))
    }
}

/// A map from ids to values.
pub struct DMap<T>(FxMap<u64, T>);
impl<T> Default for DMap<T> {
    fn default() -> Self {
        Self(FxMap::default())
    }
}
impl<T> DMap<T> {
    pub fn new() -> Self {
        Self::default()
    }
    #[inline]
    pub fn insert(&mut self, k: f64, v: T) {
        self.0.insert(kb(k), v);
    }
    #[inline]
    pub fn get(&self, k: f64) -> Option<&T> {
        self.0.get(&kb(k))
    }
    #[inline]
    pub fn contains(&self, k: f64) -> bool {
        self.0.contains_key(&kb(k))
    }
}

/// Lists of values per key, in the order they were added, stored as compressed rows.
pub struct Rows<K> {
    slot: FxMap<K, u32>,
    /// per slot: (start, len) into `items`
    rows: Vec<(u32, u32)>,
    items: Vec<f64>,
}
impl<K: Hash + Eq + Copy> Rows<K> {
    /// Build from (key, value) pairs; `dedupe_last`: skip a value equal to the last one already in its key's list.
    pub fn build(pairs: &[(K, f64)], dedupe_last: bool) -> Self {
        let mut slot: FxMap<K, u32> = FxMap::with_capacity_and_hasher(pairs.len() / 2 + 1, Default::default());
        let mut counts: Vec<u32> = Vec::new();
        let mut last: Vec<u64> = Vec::new();
        // the slot of each pair, or u32::MAX when it is left out
        let mut at: Vec<u32> = Vec::with_capacity(pairs.len());
        for &(k, v) in pairs {
            let n = u32::try_from(counts.len()).unwrap_or(u32::MAX);
            let s = *slot.entry(k).or_insert(n);
            if s == n {
                counts.push(0);
                last.push(0);
            }
            let i = usize::try_from(s).unwrap_or(usize::MAX);
            let (Some(c), Some(l)) = (counts.get_mut(i), last.get_mut(i)) else {
                at.push(u32::MAX);
                continue;
            };
            if dedupe_last && *c > 0 && *l == kb(v) {
                at.push(u32::MAX);
                continue;
            }
            *c = c.saturating_add(1);
            *l = kb(v);
            at.push(s);
        }
        let mut rows: Vec<(u32, u32)> = Vec::with_capacity(counts.len());
        let mut start = 0u32;
        for &c in &counts {
            rows.push((start, 0));
            start = start.saturating_add(c);
        }
        let mut items = vec![0.0; usize::try_from(start).unwrap_or(0)];
        for (&(_, v), &s) in pairs.iter().zip(at.iter()) {
            let Some(r) = rows.get_mut(usize::try_from(s).unwrap_or(usize::MAX)) else { continue };
            let p = usize::try_from(r.0.saturating_add(r.1)).unwrap_or(usize::MAX);
            if let Some(x) = items.get_mut(p) {
                *x = v;
            }
            r.1 = r.1.saturating_add(1);
        }
        Self { slot, rows, items }
    }
    /// The list of key k (empty when none).
    #[inline]
    pub fn get(&self, k: &K) -> &[f64] {
        let Some(&s) = self.slot.get(k) else { return &[] };
        match self.rows.get(usize::try_from(s).unwrap_or(usize::MAX)) {
            Some(&(a, n)) => {
                let a = usize::try_from(a).unwrap_or(0);
                let e = a.saturating_add(usize::try_from(n).unwrap_or(0));
                self.items.get(a..e).unwrap_or(&[])
            }
            None => &[],
        }
    }
    /// Is there a list for k?
    #[inline]
    pub fn has(&self, k: &K) -> bool {
        self.slot.contains_key(k)
    }
    /// Every list (in no particular order).
    pub fn lists(&self) -> impl Iterator<Item = &[f64]> {
        self.rows.iter().map(|&(a, n)| {
            let a = usize::try_from(a).unwrap_or(0);
            self.items.get(a..a.saturating_add(usize::try_from(n).unwrap_or(0))).unwrap_or(&[])
        })
    }
}

/// Lists of ids per id (e.g. vertex → edges).
pub struct Adj(Rows<u64>);
impl Adj {
    /// Build from (key, value) pairs; `dedupe_last`: skip a value equal to the last one already in its key's list.
    pub fn build(pairs: &[(f64, f64)], dedupe_last: bool) -> Self {
        let p: Vec<(u64, f64)> = pairs.iter().map(|&(k, v)| (kb(k), v)).collect();
        Self(Rows::build(&p, dedupe_last))
    }
    /// The list of key k (empty when none).
    #[inline]
    pub fn get(&self, k: f64) -> &[f64] {
        self.0.get(&kb(k))
    }
}

/// A set of vertex ids, kept as a bit per slot of the definition's vertex table (ids not in the table: hashed).
pub struct VSet<'a> {
    verts: &'a IdMap<P3>,
    bits: Vec<bool>,
    other: FxMap<u64, ()>,
}
impl<'a> VSet<'a> {
    pub fn new(verts: &'a IdMap<P3>) -> Self {
        Self { verts, bits: vec![false; verts.slot_len()], other: FxMap::default() }
    }
    #[inline]
    pub fn add_slot(&mut self, s: usize) {
        if let Some(b) = self.bits.get_mut(s) {
            *b = true;
        }
    }
    #[inline]
    pub fn add(&mut self, k: f64) {
        match self.verts.slot(k) {
            Some(s) => self.add_slot(s),
            None => {
                self.other.insert(kb(k), ());
            }
        }
    }
    #[inline]
    pub fn has(&self, k: f64) -> bool {
        match self.verts.slot(k) {
            Some(s) => self.bits.get(s).copied().unwrap_or(false),
            None => self.other.contains_key(&kb(k)),
        }
    }
    #[inline]
    pub fn has_slot(&self, s: usize) -> bool {
        self.bits.get(s).copied().unwrap_or(false)
    }
}

/// Lists of ids per vertex (vertex → edges, vertex → faces), rows by the vertex table's slots.
pub struct VAdj<'a> {
    verts: &'a IdMap<P3>,
    rows: Vec<(u32, u32)>,
    items: Vec<f64>,
    other: FxMap<u64, Vec<f64>>,
}
impl<'a> VAdj<'a> {
    /// Build from (vertex, value) pairs; `dedupe_last`: skip a value equal to the last one already in its list.
    pub fn build(verts: &'a IdMap<P3>, pairs: &[(f64, f64)], dedupe_last: bool) -> Self {
        let n = verts.slot_len();
        let mut counts = vec![0u32; n];
        let mut last = vec![0u64; if dedupe_last { n } else { 0 }];
        let mut other: FxMap<u64, Vec<f64>> = FxMap::default();
        let mut at: Vec<u32> = Vec::with_capacity(pairs.len());
        for &(k, v) in pairs {
            let Some(s) = verts.slot(k) else {
                let e = other.entry(kb(k)).or_default();
                if !(dedupe_last && e.last().is_some_and(|&x| kb(x) == kb(v))) {
                    e.push(v);
                }
                at.push(u32::MAX);
                continue;
            };
            let Some(c) = counts.get_mut(s) else {
                at.push(u32::MAX);
                continue;
            };
            if dedupe_last {
                let Some(l) = last.get_mut(s) else {
                    at.push(u32::MAX);
                    continue;
                };
                if *c > 0 && *l == kb(v) {
                    at.push(u32::MAX);
                    continue;
                }
                *l = kb(v);
            }
            *c = c.saturating_add(1);
            at.push(u32::try_from(s).unwrap_or(u32::MAX));
        }
        let mut rows: Vec<(u32, u32)> = Vec::with_capacity(n);
        let mut start = 0u32;
        for &c in &counts {
            rows.push((start, 0));
            start = start.saturating_add(c);
        }
        let mut items = vec![0.0; usize::try_from(start).unwrap_or(0)];
        for (&(_, v), &s) in pairs.iter().zip(at.iter()) {
            let Some(r) = rows.get_mut(usize::try_from(s).unwrap_or(usize::MAX)) else { continue };
            let p = usize::try_from(r.0.saturating_add(r.1)).unwrap_or(usize::MAX);
            if let Some(x) = items.get_mut(p) {
                *x = v;
            }
            r.1 = r.1.saturating_add(1);
        }
        Self { verts, rows, items, other }
    }
    /// The list of vertex k (empty when none).
    #[inline]
    pub fn get(&self, k: f64) -> &[f64] {
        match self.verts.slot(k) {
            Some(s) => match self.rows.get(s) {
                Some(&(a, n)) => {
                    let a = usize::try_from(a).unwrap_or(0);
                    self.items.get(a..a.saturating_add(usize::try_from(n).unwrap_or(0))).unwrap_or(&[])
                }
                None => &[],
            },
            None => self.other.get(&kb(k)).map_or(&[][..], |v| v.as_slice()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adjacency_keeps_order() {
        let a = Adj::build(&[(1.0, 10.0), (2.0, 20.0), (1.0, 11.0), (1.5, 7.0), (1.0, 11.0), (1.5, 8.0)], true);
        assert_eq!(a.get(1.0), &[10.0, 11.0]);
        assert_eq!(a.get(2.0), &[20.0]);
        assert_eq!(a.get(1.5), &[7.0, 8.0]);
        assert!(a.get(3.0).is_empty());
        let mut m = Mark::new();
        m.add(5.0);
        m.add(-0.0);
        m.add(2.5);
        assert!(m.has(5.0) && m.has(0.0) && m.has(2.5) && !m.has(4.0));
    }
}
