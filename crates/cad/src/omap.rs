//! An insertion-ordered map with JavaScript `Map` semantics: setting an existing key keeps its place, deleting and
//! setting again puts the key at the end. doc.js relies on that order (draw order; an undone erase comes back last).

use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone, Debug)]
pub struct OMap<K, V> {
    slots: Vec<Option<(K, V)>>,
    index: HashMap<K, usize>,
}

impl<K, V> Default for OMap<K, V> {
    fn default() -> Self {
        Self { slots: Vec::new(), index: HashMap::new() }
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
    pub fn get(&self, k: &K) -> Option<&V> {
        let i = *self.index.get(k)?;
        self.slots.get(i).and_then(|s| s.as_ref()).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        let i = *self.index.get(k)?;
        self.slots.get_mut(i).and_then(|s| s.as_mut()).map(|(_, v)| v)
    }
    pub fn contains_key(&self, k: &K) -> bool {
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

impl<K: Hash + Eq + Clone, V> FromIterator<(K, V)> for OMap<K, V> {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        let mut m = Self::new();
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
}
