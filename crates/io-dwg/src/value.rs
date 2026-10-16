//! The dynamic value the reader builds: the same shape as the JavaScript model (lib/dwg/model.js), so
//! `convert --json` matches `JSON.stringify(readDwg(bytes))` field for field.
//!
//! * Numbers are `f64` like JavaScript numbers; integers print without a fraction.
//! * Objects keep insertion order; setting an existing key replaces it in place (as a JS object does).
//! * JavaScript `undefined` is "absent": [`Obj::set_opt`] with `None` removes the key.
//! * [`V::Bytes`] (a `Uint8Array` in the JS record) prints as base64, as `plain()` in read.js does.

use std::borrow::Cow;
use std::fmt::Write as _;

pub type Key = Cow<'static, str>;

#[derive(Clone, Debug, PartialEq, Default)]
pub enum V {
    #[default]
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<V>),
    Obj(Obj),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Obj(pub Vec<(Key, V)>);

impl Obj {
    pub fn new() -> Self {
        Obj(Vec::new())
    }
    pub fn with_capacity(n: usize) -> Self {
        Obj(Vec::with_capacity(n))
    }
    pub fn get(&self, k: &str) -> Option<&V> {
        self.0.iter().find(|(kk, _)| kk == k).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, k: &str) -> Option<&mut V> {
        self.0.iter_mut().find(|(kk, _)| kk == k).map(|(_, v)| v)
    }
    pub fn has(&self, k: &str) -> bool {
        self.0.iter().any(|(kk, _)| kk == k)
    }
    /// Set (replacing in place when present, like a JS property assignment).
    pub fn set(&mut self, k: impl Into<Key>, v: impl Into<V>) {
        let k = k.into();
        let v = v.into();
        if let Some(slot) = self.0.iter_mut().find(|(kk, _)| *kk == k) {
            slot.1 = v;
        } else {
            self.0.push((k, v));
        }
    }
    /// `o.k = v` where v may be `undefined` (None removes the key: JSON drops undefined).
    pub fn set_opt(&mut self, k: impl Into<Key>, v: Option<V>) {
        match v {
            Some(v) => self.set(k, v),
            None => {
                let k = k.into();
                self.remove(&k);
            }
        }
    }
    pub fn remove(&mut self, k: &str) -> Option<V> {
        let i = self.0.iter().position(|(kk, _)| kk == k)?;
        Some(self.0.remove(i).1)
    }
    /// `Object.assign(this, other)`.
    pub fn assign(&mut self, other: Obj) {
        for (k, v) in other.0 {
            self.set(k, v);
        }
    }
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_ref())
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.0.iter().map(|(k, v)| (k.as_ref(), v))
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    // ---- typed reads (absent or wrong type = None, like `undefined`)
    pub fn num(&self, k: &str) -> Option<f64> {
        self.get(k).and_then(V::as_f64)
    }
    pub fn str(&self, k: &str) -> Option<&str> {
        self.get(k).and_then(V::as_str)
    }
    pub fn obj(&self, k: &str) -> Option<&Obj> {
        self.get(k).and_then(V::as_obj)
    }
    pub fn obj_mut(&mut self, k: &str) -> Option<&mut Obj> {
        match self.get_mut(k) {
            Some(V::Obj(o)) => Some(o),
            _ => None,
        }
    }
    pub fn arr(&self, k: &str) -> Option<&Vec<V>> {
        self.get(k).and_then(V::as_arr)
    }
    /// JavaScript truthiness of `o.k`.
    pub fn truthy(&self, k: &str) -> bool {
        self.get(k).is_some_and(V::truthy)
    }
}

impl V {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            V::Num(n) => Some(*n),
            V::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            V::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_obj(&self) -> Option<&Obj> {
        match self {
            V::Obj(o) => Some(o),
            _ => None,
        }
    }
    pub fn as_arr(&self) -> Option<&Vec<V>> {
        match self {
            V::Arr(a) => Some(a),
            _ => None,
        }
    }
    /// JavaScript truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            V::Null => false,
            V::Bool(b) => *b,
            V::Num(n) => *n != 0.0 && !n.is_nan(),
            V::Str(s) => !s.is_empty(),
            V::Arr(_) | V::Obj(_) | V::Bytes(_) => true,
        }
    }
}

impl From<f64> for V {
    fn from(n: f64) -> Self {
        V::Num(n)
    }
}
impl From<bool> for V {
    fn from(b: bool) -> Self {
        V::Bool(b)
    }
}
impl From<&str> for V {
    fn from(s: &str) -> Self {
        V::Str(s.to_string())
    }
}
impl From<String> for V {
    fn from(s: String) -> Self {
        V::Str(s)
    }
}
impl From<Obj> for V {
    fn from(o: Obj) -> Self {
        V::Obj(o)
    }
}
impl From<Vec<V>> for V {
    fn from(a: Vec<V>) -> Self {
        V::Arr(a)
    }
}
macro_rules! from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for V {
            #[allow(clippy::cast_precision_loss, clippy::cast_lossless)]
            fn from(n: $t) -> Self { V::Num(n as f64) }
        }
    )*};
}
from_int!(i8, i16, i32, i64, u8, u16, u32, u64, usize);

/// `obj!{ "x" => 1.0, "y" => 2.0 }` builds an [`Obj`] in that key order.
#[macro_export]
macro_rules! obj {
    () => { $crate::value::Obj::new() };
    ($($k:expr => $v:expr),+ $(,)?) => {{
        let mut o = $crate::value::Obj::with_capacity(8);
        $( o.0.push((::std::borrow::Cow::from($k), $crate::value::V::from($v))); )+
        o
    }};
}

/// `{x, y}` (2D point, JS shape).
pub fn p2(x: f64, y: f64) -> V {
    V::Obj(Obj(vec![(Cow::Borrowed("x"), V::Num(x)), (Cow::Borrowed("y"), V::Num(y))]))
}
/// `{x, y, z}` (3D point, JS shape).
pub fn p3(x: f64, y: f64, z: f64) -> V {
    V::Obj(Obj(vec![(Cow::Borrowed("x"), V::Num(x)), (Cow::Borrowed("y"), V::Num(y)), (Cow::Borrowed("z"), V::Num(z))]))
}

// ---------------------------------------------------------------- JSON output

/// JavaScript `String(n)` for the numbers JSON prints (integers without a fraction).
pub fn js_num(n: f64, out: &mut String) {
    if !n.is_finite() {
        out.push_str("null"); // JSON.stringify(NaN / Infinity) → null
    } else if n == n.trunc() && n.abs() < 1e21 {
        if n == 0.0 {
            out.push('0');
        } else {
            let _ = write!(out, "{n:.0}");
        }
    } else {
        // Shortest round-trip form; JS uses exponents for very small/large values (parsers read both the same).
        let a = n.abs();
        if !(1e-7..1e21).contains(&a) {
            let _ = write!(out, "{n:e}");
        } else {
            let _ = write!(out, "{n}");
        }
    }
}

pub fn json_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

impl V {
    pub fn write_json(&self, out: &mut String) {
        match self {
            V::Null => out.push_str("null"),
            V::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            V::Num(n) => js_num(*n, out),
            V::Str(s) => json_str(s, out),
            V::Bytes(b) => json_str(&crate::util::b64(b), out),
            V::Arr(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write_json(out);
                }
                out.push(']');
            }
            V::Obj(o) => o.write_json(out),
        }
    }
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        self.write_json(&mut s);
        s
    }
}

impl Obj {
    pub fn write_json(&self, out: &mut String) {
        out.push('{');
        for (i, (k, v)) in self.0.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            json_str(k, out);
            out.push(':');
            v.write_json(out);
        }
        out.push('}');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_shapes() {
        let mut o = obj! { "a" => 1.0, "b" => "x" };
        o.set("a", 2.5);
        o.set_opt("b", None);
        o.set("c", vec![V::Num(-0.0), V::Num(1e-9), V::Bool(true)]);
        assert_eq!(V::Obj(o).to_json(), r#"{"a":2.5,"c":[0,1e-9,true]}"#);
    }
}
