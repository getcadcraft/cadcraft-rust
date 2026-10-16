//! Insertion-ordered maps and sets with JavaScript `Map` / `Set` semantics: setting an existing key keeps its place,
//! deleting and setting again puts the key at the end. The kernel's output (toJSON, the order faces are walked in,
//! the ids new geometry gets) depends on that order.
//!
//! [`IdMap`] is keyed by JS numbers (f64, compared with SameValueZero: −0 and +0 are one key), [`OMap`] by strings.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hash, Hasher};

/// A small, fast, deterministic hasher for the kernel's maps of ids: FxHash's mixing, then murmur3's finaliser (the
/// keys are often f64 bit patterns, whose low bits are all zero for whole numbers: without the finaliser every such
/// key would land in one bucket).
#[derive(Default, Clone, Copy)]
pub struct Fx(u64);
impl Hasher for Fx {
    fn finish(&self) -> u64 {
        let mut k = self.0;
        k ^= k >> 33;
        k = k.wrapping_mul(0xff51_afd7_ed55_8ccd);
        k ^= k >> 33;
        k = k.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        k ^ (k >> 33)
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
}
pub type FxBuild = BuildHasherDefault<Fx>;
pub type FxMap<K, V> = HashMap<K, V, FxBuild>;

/// The key bits of a JS number in a Map (SameValueZero: −0 = +0, every NaN is one key).
#[inline]
pub fn kb(x: f64) -> u64 {
    if x == 0.0 {
        0
    } else if x.is_nan() {
        0x7ff8_0000_0000_0000
    } else {
        x.to_bits()
    }
}

/// Whole ids below this are found through an array (the ids the kernel hands out); others through a hash map.
const DENSE: f64 = 1_048_576.0;

#[inline]
fn dense_of(k: f64) -> Option<usize> {
    let _ = DENSE;
    crate::dense::dense_of(k)
}

/// Key → slot: an array for small whole ids, a hash map for the rest.
#[derive(Clone, Debug, Default)]
struct Index {
    /// slot + 1 (0 = no such key)
    dense: Vec<u32>,
    sparse: FxMap<u64, usize>,
    len: usize,
}
impl Index {
    #[inline]
    fn get(&self, k: f64) -> Option<usize> {
        match dense_of(k) {
            Some(i) => match self.dense.get(i) {
                Some(&s) if s != 0 => usize::try_from(s - 1).ok(),
                _ => None,
            },
            None => self.sparse.get(&kb(k)).copied(),
        }
    }
    #[inline]
    fn has(&self, k: f64) -> bool {
        self.get(k).is_some()
    }
    /// Point k at a new slot (k must not be in the index).
    fn insert(&mut self, k: f64, slot: usize) {
        let packed = u32::try_from(slot).ok().and_then(|s| s.checked_add(1));
        match (dense_of(k), packed) {
            (Some(i), Some(s)) => {
                if self.dense.len() <= i {
                    self.dense.resize(i + 1, 0);
                }
                if let Some(x) = self.dense.get_mut(i) {
                    *x = s;
                }
            }
            _ => {
                self.sparse.insert(kb(k), slot);
            }
        }
        self.len += 1;
    }
    fn remove(&mut self, k: f64) -> Option<usize> {
        let out = match dense_of(k) {
            Some(i) => match self.dense.get_mut(i) {
                Some(x) if *x != 0 => {
                    let s = *x - 1;
                    *x = 0;
                    usize::try_from(s).ok()
                }
                // a slot past u32 lives in the hash map
                _ => self.sparse.remove(&kb(k)),
            },
            None => self.sparse.remove(&kb(k)),
        };
        if out.is_some() {
            self.len -= 1;
        }
        out
    }
    fn clear(&mut self) {
        self.dense.clear();
        self.sparse.clear();
        self.len = 0;
    }
}

/// `Map<number, V>`.
#[derive(Clone, Debug)]
pub struct IdMap<V> {
    slots: Vec<Option<(f64, V)>>,
    index: Index,
}

impl<V> Default for IdMap<V> {
    fn default() -> Self {
        Self { slots: Vec::new(), index: Index::default() }
    }
}

impl<V> IdMap<V> {
    pub fn new() -> Self {
        Self::default()
    }
    #[inline]
    pub fn len(&self) -> usize {
        self.index.len
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.index.len == 0
    }
    #[inline]
    pub fn get(&self, k: f64) -> Option<&V> {
        let i = self.index.get(k)?;
        self.slots.get(i).and_then(|s| s.as_ref()).map(|(_, v)| v)
    }
    #[inline]
    pub fn get_mut(&mut self, k: f64) -> Option<&mut V> {
        let i = self.index.get(k)?;
        self.slots.get_mut(i).and_then(|s| s.as_mut()).map(|(_, v)| v)
    }
    #[inline]
    pub fn has(&self, k: f64) -> bool {
        self.index.has(k)
    }
    /// `map.set(k, v)`.
    pub fn set(&mut self, k: f64, v: V) {
        if let Some(i) = self.index.get(k)
            && let Some(Some(slot)) = self.slots.get_mut(i)
        {
            slot.1 = v;
            return;
        }
        self.index.insert(k, self.slots.len());
        // a Map keeps the key it was first given (−0 is stored as +0)
        self.slots.push(Some((if k == 0.0 { 0.0 } else { k }, v)));
    }
    /// `map.delete(k)` → the value it held.
    pub fn delete(&mut self, k: f64) -> Option<V> {
        let i = self.index.remove(k)?;
        let out = self.slots.get_mut(i).and_then(Option::take).map(|(_, v)| v);
        if self.slots.len() > 32 && self.index.len < self.slots.len() / 2 {
            self.compact();
        }
        out
    }
    fn compact(&mut self) {
        self.slots.retain(Option::is_some);
        self.index.clear();
        let keys: Vec<f64> = self.slots.iter().flatten().map(|(k, _)| *k).collect();
        for (i, k) in keys.into_iter().enumerate() {
            self.index.insert(k, i);
        }
    }
    pub fn clear(&mut self) {
        self.slots.clear();
        self.index.clear();
    }
    pub fn iter(&self) -> impl Iterator<Item = (f64, &V)> {
        self.slots.iter().flatten().map(|(k, v)| (*k, v))
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (f64, &mut V)> {
        self.slots.iter_mut().flatten().map(|(k, v)| (*k, v))
    }
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.slots.iter().flatten().map(|(_, v)| v)
    }
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.slots.iter_mut().flatten().map(|(_, v)| v)
    }
    pub fn keys(&self) -> impl Iterator<Item = f64> + '_ {
        self.slots.iter().flatten().map(|(k, _)| *k)
    }
    /// The slot a key's entry lives in (stable while the map is not changed): a compact index for scratch arrays.
    #[inline]
    pub fn slot(&self, k: f64) -> Option<usize> {
        self.index.get(k)
    }
    /// How many slots there are (entries plus the holes deletions left).
    #[inline]
    pub fn slot_len(&self) -> usize {
        self.slots.len()
    }
    /// The entries with their slots.
    pub fn iter_slots(&self) -> impl Iterator<Item = (usize, f64, &V)> {
        self.slots.iter().enumerate().filter_map(|(i, s)| s.as_ref().map(|(k, v)| (i, *k, v)))
    }
    /// `[...map.keys()]`.
    pub fn key_vec(&self) -> Vec<f64> {
        self.keys().collect()
    }
    /// The first key (`[...map.keys()][0]`).
    pub fn first_key(&self) -> Option<f64> {
        self.keys().next()
    }
}

impl<V: Clone> IdMap<V> {
    /// `[...map]` (a copy of the entries).
    pub fn entries(&self) -> Vec<(f64, V)> {
        self.slots.iter().flatten().cloned().collect()
    }
}

/// `Set<number>` (insertion-ordered).
#[derive(Clone, Debug, Default)]
pub struct IdSet(IdMap<()>);

impl IdSet {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn from_iter_ids(it: impl IntoIterator<Item = f64>) -> Self {
        let mut s = Self::new();
        for x in it {
            s.add(x);
        }
        s
    }
    #[inline]
    pub fn add(&mut self, k: f64) {
        if !self.0.has(k) {
            self.0.set(k, ());
        }
    }
    #[inline]
    pub fn has(&self, k: f64) -> bool {
        self.0.has(k)
    }
    #[inline]
    pub fn delete(&mut self, k: f64) -> bool {
        self.0.delete(k).is_some()
    }
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = f64> + '_ {
        self.0.keys()
    }
    pub fn to_vec(&self) -> Vec<f64> {
        self.0.key_vec()
    }
}

/// `Map<string, V>` (any hashable key).
#[derive(Clone, Debug)]
pub struct OMap<K, V> {
    slots: Vec<Option<(K, V)>>,
    index: FxMap<K, usize>,
}

impl<K, V> Default for OMap<K, V> {
    fn default() -> Self {
        Self { slots: Vec::new(), index: FxMap::default() }
    }
}

impl<K: Hash + Eq + Clone, V> OMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.index.len()
    }
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
    pub fn get<Q>(&self, k: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let i = *self.index.get(k)?;
        self.slots.get(i).and_then(|s| s.as_ref()).map(|(_, v)| v)
    }
    pub fn get_mut<Q>(&mut self, k: &Q) -> Option<&mut V>
    where
        K: std::borrow::Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let i = *self.index.get(k)?;
        self.slots.get_mut(i).and_then(|s| s.as_mut()).map(|(_, v)| v)
    }
    pub fn has<Q>(&self, k: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.index.contains_key(k)
    }
    /// `map.set(k, v)`.
    pub fn set(&mut self, k: K, v: V) {
        if let Some(&i) = self.index.get(&k)
            && let Some(Some(slot)) = self.slots.get_mut(i)
        {
            slot.1 = v;
            return;
        }
        self.index.insert(k.clone(), self.slots.len());
        self.slots.push(Some((k, v)));
    }
    /// `map.delete(k)` → the value it held.
    pub fn delete<Q>(&mut self, k: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let i = self.index.remove(k)?;
        let out = self.slots.get_mut(i).and_then(Option::take).map(|(_, v)| v);
        if self.slots.len() > 32 && self.index.len() < self.slots.len() / 2 {
            self.slots.retain(Option::is_some);
            self.index.clear();
            for (i, (k, _)) in self.slots.iter().flatten().enumerate() {
                self.index.insert(k.clone(), i);
            }
        }
        out
    }
    pub fn clear(&mut self) {
        self.slots.clear();
        self.index.clear();
    }
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.slots.iter().flatten().map(|(k, v)| (k, v))
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.slots.iter_mut().flatten().map(|(k, v)| (&*k, v))
    }
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.slots.iter().flatten().map(|(_, v)| v)
    }
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.slots.iter_mut().flatten().map(|(_, v)| v)
    }
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.slots.iter().flatten().map(|(k, _)| k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn js_map_order() {
        let mut m = IdMap::new();
        for i in 0..5 {
            m.set(f64::from(i), i);
        }
        m.set(2.0, 20);
        assert_eq!(m.values().copied().collect::<Vec<_>>(), vec![0, 1, 20, 3, 4]);
        m.delete(1.0);
        m.set(1.0, 10);
        assert_eq!(m.key_vec(), vec![0.0, 2.0, 3.0, 4.0, 1.0]);
        for i in 0..100 {
            m.set(f64::from(100 + i), 0);
        }
        for i in 0..90 {
            m.delete(f64::from(100 + i));
        }
        assert_eq!(m.len(), 15);
        assert_eq!(m.get(199.0), Some(&0));
        assert!(m.has(-0.0));
    }
}
