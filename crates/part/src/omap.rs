//! A string-keyed map that keeps JS `Map` insertion order: `set` on an existing key replaces the value in place.

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct OMap<V> {
    items: Vec<(String, V)>,
    index: HashMap<String, usize>,
}

impl<V> Default for OMap<V> {
    fn default() -> Self {
        OMap { items: Vec::new(), index: HashMap::new() }
    }
}

impl<V> OMap<V> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn get(&self, k: &str) -> Option<&V> {
        self.index.get(k).and_then(|&i| self.items.get(i)).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, k: &str) -> Option<&mut V> {
        match self.index.get(k) {
            Some(&i) => self.items.get_mut(i).map(|(_, v)| v),
            None => None,
        }
    }
    pub fn has(&self, k: &str) -> bool {
        self.index.contains_key(k)
    }
    /// `map.set(k, v)`.
    pub fn set(&mut self, k: &str, v: V) {
        if let Some(&i) = self.index.get(k)
            && let Some(slot) = self.items.get_mut(i)
        {
            slot.1 = v;
            return;
        }
        self.index.insert(k.to_string(), self.items.len());
        self.items.push((k.to_string(), v));
    }
    /// `map.delete(k)` (later keys keep their order).
    pub fn delete(&mut self, k: &str) -> Option<V> {
        let i = self.index.remove(k)?;
        if i >= self.items.len() {
            return None;
        }
        let (_, v) = self.items.remove(i);
        for (j, (key, _)) in self.items.iter().enumerate().skip(i) {
            self.index.insert(key.clone(), j);
        }
        Some(v)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.items.iter().map(|(k, v)| (k.as_str(), v))
    }
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(|(k, _)| k.as_str())
    }
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.items.iter().map(|(_, v)| v)
    }
    pub fn key_vec(&self) -> Vec<String> {
        self.items.iter().map(|(k, _)| k.clone()).collect()
    }
    pub fn into_items(self) -> Vec<(String, V)> {
        self.items
    }
}

impl<V: Clone> OMap<V> {
    pub fn items(&self) -> Vec<(String, V)> {
        self.items.clone()
    }
}

impl crate::expr::Values for OMap<f64> {
    fn get(&self, name: &str) -> Option<f64> {
        OMap::get(self, name).copied()
    }
}
