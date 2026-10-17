//! The JavaScript semantics BimCraft's modules lean on, gathered in one place: number ↔ string (`String(n)`,
//! `toFixed`, `toPrecision`, `Number(s)`), `Math.round` / `Math.hypot`, `localeCompare(…, { numeric: true })`, a
//! Float32Array store, and the plain values the property index holds (a number or a string).
//!
//! The number formatting comes from `cc-cad::js` / `cc-m3d::js`, V8's Math from `cc-io-ifc::jsmath`, the collation
//! from `cc-io-dwg::compile::collate` (an ICU-root-like natural order); nothing is copied here.

use std::cmp::Ordering;
use std::rc::Rc;

use serde_json::Value as J;

/// `String(n)`.
pub fn num_str(x: f64) -> String {
    cc_cad::js::num_to_string(x)
}

/// `x.toFixed(f)`.
pub fn to_fixed(x: f64, f: usize) -> String {
    cc_cad::js::to_fixed(x, f)
}

/// `Number(s)` for a string.
pub fn str_num(s: &str) -> f64 {
    cc_cad::js::str_to_number(s)
}

/// `Math.round`.
pub fn round(x: f64) -> f64 {
    cc_io_ifc::jsmath::round(x)
}

/// `Math.hypot(a, b, c)` (V8).
pub fn hypot3(a: f64, b: f64, c: f64) -> f64 {
    cc_io_ifc::jsmath::hypot3(a, b, c)
}

/// `Math.min(a, b)` / `Math.max(a, b)` (NaN wins, signed zeros ordered).
pub fn min(a: f64, b: f64) -> f64 {
    cc_io_ifc::jsmath::min(a, b)
}
pub fn max(a: f64, b: f64) -> f64 {
    cc_io_ifc::jsmath::max(a, b)
}

/// A store into a Float32Array (round to nearest, ties to even).
#[allow(clippy::cast_possible_truncation)]
pub fn f32_of(x: f64) -> f32 {
    x as f32
}

/// `Number(n.toPrecision(12))` for a finite n (ifc-qto.js `tidy`), n itself otherwise.
pub fn tidy(n: f64) -> f64 {
    if n.is_finite() { str_num(&cc_m3d::js::to_precision(n, 12)) } else { n }
}

/// `a.localeCompare(b, undefined, { numeric: true })` as an ordering.
pub fn locale_cmp(a: &str, b: &str) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }
    cc_io_dwg::compile::collate(a, b)
}

/// `a.localeCompare(b)` (no numeric option). Spec gap: the class names and set names this is used on carry no
/// digit runs where the two orders part, so the numeric collation stands in for it.
pub fn locale_cmp_plain(a: &str, b: &str) -> Ordering {
    locale_cmp(a, b)
}

/// `x - y` of a comparator as an ordering (NaN → Equal: the JS sort keeps the pair as it was).
pub fn num_cmp(x: f64, y: f64) -> Ordering {
    x.partial_cmp(&y).unwrap_or(Ordering::Equal)
}

/// The sign of a JS comparator's number result as an ordering (`0`, `NaN` → Equal).
pub fn sign_ord(d: f64) -> Ordering {
    if d < 0.0 {
        Ordering::Less
    } else if d > 0.0 {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

/// A value of the property index: numbers stay numbers, the rest is text.
#[derive(Clone, Debug, PartialEq)]
pub enum PV {
    Num(f64),
    Str(Rc<str>),
}

impl PV {
    pub fn s(s: &str) -> PV {
        PV::Str(Rc::from(s))
    }
    /// `String(v)`.
    pub fn text(&self) -> String {
        match self {
            PV::Num(n) => num_str(*n),
            PV::Str(s) => s.to_string(),
        }
    }
    pub fn as_num(&self) -> Option<f64> {
        if let PV::Num(n) = self { Some(*n) } else { None }
    }
    pub fn is_num(&self) -> bool {
        matches!(self, PV::Num(_))
    }
    /// `v === ''`
    pub fn is_empty_str(&self) -> bool {
        matches!(self, PV::Str(s) if s.is_empty())
    }
    /// `a === b` (numbers by value: NaN never equal, +0 === −0).
    pub fn strict_eq(&self, o: &PV) -> bool {
        match (self, o) {
            (PV::Num(a), PV::Num(b)) => a == b,
            (PV::Str(a), PV::Str(b)) => a == b,
            _ => false,
        }
    }
    pub fn json(&self) -> J {
        match self {
            PV::Num(n) => jn(*n),
            PV::Str(s) => J::String(s.to_string()),
        }
    }
    /// A Map key that tells numbers from strings (SameValueZero: NaN equals NaN, +0 equals −0).
    pub fn key(&self) -> PKey {
        match self {
            PV::Num(n) => PKey::Num(if *n == 0.0 {
                0
            } else if n.is_nan() {
                u64::MAX
            } else {
                n.to_bits()
            }),
            PV::Str(s) => PKey::Str(s.clone()),
        }
    }
}

/// [`PV::key`]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PKey {
    Num(u64),
    Str(Rc<str>),
}

/// A JSON number as the kit's canon writes it (−0 → 0, NaN / ±∞ → "$NaN" / "$Inf" / "$-Inf").
pub fn jn(x: f64) -> J {
    if x.is_nan() {
        J::String("$NaN".into())
    } else if x == f64::INFINITY {
        J::String("$Inf".into())
    } else if x == f64::NEG_INFINITY {
        J::String("$-Inf".into())
    } else if x == 0.0 {
        J::from(0)
    } else {
        serde_json::Number::from_f64(x).map_or(J::Null, J::Number)
    }
}

/// A count as a JSON number.
pub fn ju(n: usize) -> J {
    J::from(n)
}

/// JS white space (`trim`).
pub fn trim(s: &str) -> &str {
    cc_cad::js::trim(s)
}

/// `s.slice(0, n)` in UTF-16 code units.
pub fn slice16(s: &str, n: usize) -> String {
    cc_cad::js::slice16(s, n)
}

/// `s.length` in UTF-16 code units.
pub fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `s.toLowerCase()`.
pub fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// A Map with JS insertion order (no deletions).
#[derive(Clone, Debug)]
pub struct OMap<K: std::hash::Hash + Eq + Clone, V> {
    pub keys: Vec<K>,
    pub vals: Vec<V>,
    at: std::collections::HashMap<K, usize>,
}

impl<K: std::hash::Hash + Eq + Clone, V> Default for OMap<K, V> {
    fn default() -> Self {
        OMap { keys: Vec::new(), vals: Vec::new(), at: std::collections::HashMap::new() }
    }
}

impl<K: std::hash::Hash + Eq + Clone, V> OMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.keys.len()
    }
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
    pub fn get(&self, k: &K) -> Option<&V> {
        self.at.get(k).and_then(|&i| self.vals.get(i))
    }
    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        match self.at.get(k) {
            Some(&i) => self.vals.get_mut(i),
            None => None,
        }
    }
    pub fn has(&self, k: &K) -> bool {
        self.at.contains_key(k)
    }
    /// `m.set(k, v)` (an existing key keeps its place).
    pub fn set(&mut self, k: K, v: V) {
        match self.at.get(&k) {
            Some(&i) => {
                if let Some(slot) = self.vals.get_mut(i) {
                    *slot = v;
                }
            }
            None => {
                self.at.insert(k.clone(), self.keys.len());
                self.keys.push(k);
                self.vals.push(v);
            }
        }
    }
    /// The value at k, inserted with `f()` first when missing (always Some).
    pub fn entry(&mut self, k: K, f: impl FnOnce() -> V) -> Option<&mut V> {
        match self.at.get(&k) {
            Some(&i) => self.vals.get_mut(i),
            None => {
                self.at.insert(k.clone(), self.keys.len());
                self.keys.push(k);
                self.vals.push(f());
                self.vals.last_mut()
            }
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.keys.iter().zip(self.vals.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidy_and_strings() {
        assert_eq!(tidy(0.1 + 0.2), 0.3);
        assert_eq!(num_str(1250.5), "1250.5");
        assert_eq!(to_fixed(0.0005, 3), "0.001");
        assert_eq!(locale_cmp("a2", "a10"), Ordering::Less);
    }
}
