//! An insertion-ordered map with JavaScript `Map` semantics: setting an existing key keeps its place, deleting and
//! setting again puts the key at the end. doc.js relies on that order (draw order; an undone erase comes back last).

use std::borrow::Borrow;
use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, BuildHasherDefault, Hash, Hasher};

/// A hasher for the u64 ids the document mints itself (sequential, never read from a file as keys): one multiply
/// (Fibonacci hashing) instead of SipHash. Only for keys that are not attacker-chosen; maps keyed by names from files
/// keep the default `RandomState`.
#[derive(Clone, Copy, Debug, Default)]
pub struct IdHasher(u64);
impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(*b)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0 ^ n).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}
pub type IdBuild = BuildHasherDefault<IdHasher>;

#[derive(Clone, Debug)]
pub struct OMap<K, V, S = RandomState> {
    slots: Vec<Option<(K, V)>>,
    index: HashMap<K, usize, S>,
}

impl<K, V, S: Default> Default for OMap<K, V, S> {
    fn default() -> Self {
        Self { slots: Vec::new(), index: HashMap::default() }
    }
}

impl<K: Hash + Eq + Clone, V> OMap<K, V> {
    /// An empty map with the default (SipHash) hasher; `OMap::default()` for another.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<K: Hash + Eq + Clone, V, S: BuildHasher + Default> OMap<K, V, S> {
    /// Room for `n` more entries (a whole-drawing edit adding many objects at once).
    pub fn reserve(&mut self, n: usize) {
        self.slots.reserve(n);
        self.index.reserve(n);
    }
    pub fn len(&self) -> usize {
        self.index.len()
    }
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
    pub fn get<Q: Hash + Eq + ?Sized>(&self, k: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        let i = *self.index.get(k)?;
        self.slots.get(i).and_then(|s| s.as_ref()).map(|(_, v)| v)
    }
    pub fn get_mut<Q: Hash + Eq + ?Sized>(&mut self, k: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
    {
        let i = *self.index.get(k)?;
        self.slots.get_mut(i).and_then(|s| s.as_mut()).map(|(_, v)| v)
    }
    pub fn contains_key<Q: Hash + Eq + ?Sized>(&self, k: &Q) -> bool
    where
        K: Borrow<Q>,
    {
        self.index.contains_key(k)
    }
    /// `map.set(k, v)`.
    pub fn insert(&mut self, k: K, v: V) {
        if let Some(&i) = self.index.get(&k)
            && let Some(slot) = self.slots.get_mut(i)
        {
            *slot = Some((k, v));
            return;
        }
        self.index.insert(k.clone(), self.slots.len());
        self.slots.push(Some((k, v)));
    }
    /// `map.set(k, v)` → the value it replaced (None when the key is new: it goes at the end).
    pub fn replace(&mut self, k: K, v: V) -> Option<V> {
        if let Some(&i) = self.index.get(&k)
            && let Some(Some(slot)) = self.slots.get_mut(i)
        {
            return Some(std::mem::replace(&mut slot.1, v));
        }
        self.insert(k, v);
        None
    }
    /// `map.delete(k)` → the value it held.
    pub fn remove(&mut self, k: &K) -> Option<V> {
        let i = self.index.remove(k)?;
        let out = self.slots.get_mut(i).and_then(Option::take).map(|(_, v)| v);
        if self.slots.len() > 32 && self.index.len() < self.slots.len() / 2 {
            self.compact();
        }
        out
    }
    fn compact(&mut self) {
        self.slots.retain(Option::is_some);
        self.index.clear();
        for (i, (k, _)) in self.slots.iter().flatten().enumerate() {
            self.index.insert(k.clone(), i);
        }
    }
    pub fn clear(&mut self) {
        self.slots.clear();
        self.index.clear();
    }
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.slots.iter().flatten().map(|(k, v)| (k, v))
    }
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.slots.iter().flatten().map(|(_, v)| v)
    }
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.slots.iter().flatten().map(|(k, _)| k)
    }
}

impl<K: Hash + Eq + Clone, V, S: BuildHasher + Default> FromIterator<(K, V)> for OMap<K, V, S> {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        let mut m = Self::default();
        for (k, v) in iter {
            m.insert(k, v);
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn js_map_order() {
        let mut m = OMap::new();
        for i in 0..5u64 {
            m.insert(i, i);
        }
        m.insert(2, 20);
        assert_eq!(m.values().copied().collect::<Vec<_>>(), vec![0, 1, 20, 3, 4]);
        m.remove(&1);
        m.insert(1, 10);
        assert_eq!(m.keys().copied().collect::<Vec<_>>(), vec![0, 2, 3, 4, 1]);
        for i in 0..100u64 {
            m.insert(100 + i, 0);
        }
        for i in 0..90u64 {
            m.remove(&(100 + i));
        }
        assert_eq!(m.len(), 15);
        assert_eq!(m.get(&199), Some(&0));
    }
    #[test]
    fn replace_keeps_place_and_ids_hash_fast() {
        let mut m: OMap<u64, u64, IdBuild> = OMap::default();
        for i in 0..5u64 {
            m.insert(i, i);
        }
        assert_eq!(m.replace(2, 20), Some(2));
        assert_eq!(m.replace(9, 90), None);
        assert_eq!(m.values().copied().collect::<Vec<_>>(), vec![0, 1, 20, 3, 4, 90]);
        let mut n: OMap<String, u8> = OMap::new();
        n.insert("A".into(), 1);
        assert!(n.contains_key("A") && n.get("A") == Some(&1) && n.get("B").is_none());
    }
}
