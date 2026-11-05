//! S.16: the model's records (members, connections, footings) held compactly. A [`JVal`] is a JSON value as
//! `serde_json::Value` is one (the same kinds, the same insertion order, the same text), but an object is a short list
//! of `(key, value)` pairs with no hash table, and the keys and the words the model uses (types, materials, profile
//! names, param names, grades …) point at one static copy instead of an allocation each. Reading a model made a map,
//! an allocation per key and one per word for every member, hole, connection, param and footing; V8 makes those
//! objects for almost nothing, so they were most of what the read cost.
//!
//! The semantics are `Value`'s, method for method (`get`, `as_str`, `insert` keeping a key's place, `remove` as
//! serde_json's `swap_remove`, equality of objects ignoring order): [`JVal::from_value`] / [`JVal::to_value`] go both
//! ways without loss, and the text written ([`JVal::write`]) is `stringify` of the `Value` byte for byte
//! (`jval_round_trips` and the model's tests check both on every kind of value).

use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::fmt;

// ---------------------------------------------------------------------------------------------------------------
// Words

/// A string: one of the model's own words (static) or any other text.
#[derive(Clone)]
pub enum JStr {
    S(&'static str),
    H(Box<str>),
}
impl JStr {
    /// A string, pointing at the static copy when it is one of the model's words.
    #[inline]
    pub fn new(s: &str) -> JStr {
        match word(s) {
            Some(w) => JStr::S(w),
            None => JStr::H(s.into()),
        }
    }
    /// An owned string, kept (or dropped for the static copy).
    #[inline]
    pub fn from_string(s: String) -> JStr {
        match word(&s) {
            Some(w) => JStr::S(w),
            None => JStr::H(s.into_boxed_str()),
        }
    }
    #[inline]
    pub fn as_str(&self) -> &str {
        match self {
            JStr::S(s) => s,
            JStr::H(s) => s,
        }
    }
}
impl std::ops::Deref for JStr {
    type Target = str;
    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}
impl PartialEq for JStr {
    #[inline]
    fn eq(&self, o: &JStr) -> bool {
        self.as_str() == o.as_str()
    }
}
impl Eq for JStr {}
impl PartialEq<str> for JStr {
    fn eq(&self, o: &str) -> bool {
        self.as_str() == o
    }
}
impl PartialEq<&str> for JStr {
    fn eq(&self, o: &&str) -> bool {
        self.as_str() == *o
    }
}
impl std::hash::Hash for JStr {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.as_str().hash(h);
    }
}
impl fmt::Debug for JStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}
impl fmt::Display for JStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl From<&str> for JStr {
    fn from(s: &str) -> JStr {
        JStr::new(s)
    }
}
impl From<String> for JStr {
    fn from(s: String) -> JStr {
        JStr::from_string(s)
    }
}

/// The model's words: the keys of its records, member types, materials, positions, faces, connection kinds and
/// their params' names and texts, footing fields, bar sizes and the section table's names.
fn words() -> &'static std::collections::HashSet<&'static str, cc_m3d::omap::FxBuild> {
    static W: std::sync::OnceLock<std::collections::HashSet<&'static str, cc_m3d::omap::FxBuild>> = std::sync::OnceLock::new();
    W.get_or_init(|| {
        let mut w: std::collections::HashSet<&'static str, cc_m3d::omap::FxBuild> = std::collections::HashSet::default();
        const FIXED: &[&str] = &[
            // members, copes, holes
            "id",
            "type",
            "profile",
            "material",
            "start",
            "end",
            "rotation",
            "position",
            "name",
            "cuts",
            "holes",
            "phase",
            "kind",
            "side",
            "length",
            "depth",
            "face",
            "x",
            "y",
            "d",
            "beam",
            "column",
            "brace",
            "plate",
            "top",
            "middle",
            "bottom",
            "web",
            "flange",
            "cope",
            "notch",
            "left",
            "right",
            "both",
            "",
            "A992",
            "A36",
            "A572 Gr 50",
            "A500 Gr C",
            "S235",
            "S275",
            "S355",
            // connections
            "endplate",
            "sheartab",
            "baseplate",
            "splice",
            "gusset",
            "a",
            "aEnd",
            "b",
            "support",
            "params",
            // footings
            "L",
            "B",
            "H",
            "concrete",
            "mat",
            "starters",
            "barX",
            "sX",
            "barY",
            "sY",
            "cover",
            "hooks",
            "on",
            "bar",
            "nx",
            "ny",
            "sx",
            "sy",
            "projection",
            "hook",
            "4000 psi",
        ];
        w.extend(FIXED.iter().copied());
        for kind in ["endplate", "sheartab", "baseplate", "splice", "gusset"] {
            if let Some(m) = crate::connections::defaults_ref(kind).and_then(Value::as_object) {
                for (k, v) in m {
                    w.insert(k.as_str());
                    if let Some(s) = v.as_str() {
                        w.insert(s);
                    }
                }
            }
        }
        for p in crate::sections::section_rows() {
            w.insert(p.name.as_str());
        }
        for b in crate::rebar::bars() {
            w.insert(b.name.as_str());
        }
        w
    })
}
/// The static copy of one of the model's words.
#[inline]
fn word(s: &str) -> Option<&'static str> {
    if s.len() > 24 {
        return None;
    }
    words().get(s).copied()
}

// ---------------------------------------------------------------------------------------------------------------
// Values

/// A JSON value (see the module's head).
#[derive(Clone, Default)]
pub enum JVal {
    #[default]
    Null,
    Bool(bool),
    /// always finite (as a `Value`'s number)
    Num(f64),
    Str(JStr),
    Arr(Vec<JVal>),
    Obj(JObj),
}

/// An object: its entries in order.
#[derive(Clone, Default)]
pub struct JObj(pub Vec<(JStr, JVal)>);

static NULL: JVal = JVal::Null;

impl JObj {
    pub fn new() -> JObj {
        JObj(Vec::new())
    }
    pub fn with_capacity(n: usize) -> JObj {
        JObj(Vec::with_capacity(n))
    }
    #[inline]
    fn pos(&self, k: &str) -> Option<usize> {
        self.0.iter().position(|(x, _)| x.as_str() == k)
    }
    #[inline]
    pub fn get(&self, k: &str) -> Option<&JVal> {
        self.0.iter().find(|(x, _)| x.as_str() == k).map(|(_, v)| v)
    }
    #[inline]
    pub fn get_mut(&mut self, k: &str) -> Option<&mut JVal> {
        self.0.iter_mut().find(|(x, _)| x.as_str() == k).map(|(_, v)| v)
    }
    pub fn contains_key(&self, k: &str) -> bool {
        self.pos(k).is_some()
    }
    /// `o[k] = v`: a key given before keeps its place.
    pub fn insert(&mut self, k: impl Into<JStr>, v: JVal) -> Option<JVal> {
        let k = k.into();
        match self.pos(&k) {
            Some(i) => self.0.get_mut(i).map(|(_, x)| std::mem::replace(x, v)),
            None => {
                self.0.push((k, v));
                None
            }
        }
    }
    /// As serde_json's `Map::remove` (preserve_order): the last entry takes the removed one's place.
    pub fn remove(&mut self, k: &str) -> Option<JVal> {
        let i = self.pos(k)?;
        Some(self.0.swap_remove(i).1)
    }
    /// `delete o[k]` (the order of the others kept).
    pub fn shift_remove(&mut self, k: &str) -> Option<JVal> {
        let i = self.pos(k)?;
        Some(self.0.remove(i).1)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&JStr, &JVal)> {
        self.0.iter().map(|(k, v)| (k, v))
    }
    pub fn keys(&self) -> impl Iterator<Item = &JStr> {
        self.0.iter().map(|(k, _)| k)
    }
    pub fn values(&self) -> impl Iterator<Item = &JVal> {
        self.0.iter().map(|(_, v)| v)
    }
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut JVal> {
        self.0.iter_mut().map(|(_, v)| v)
    }
    pub fn from_map(m: &Map<String, Value>) -> JObj {
        JObj(m.iter().map(|(k, v)| (JStr::new(k), JVal::from_value(v))).collect())
    }
    pub fn from_map_owned(m: Map<String, Value>) -> JObj {
        let mut o = JObj::with_capacity(m.len());
        for (k, v) in m {
            o.0.push((JStr::from_string(k), JVal::from(v)));
        }
        o
    }
    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::with_capacity(self.0.len());
        for (k, v) in &self.0 {
            m.insert(k.as_str().to_string(), v.to_value());
        }
        m
    }
}
impl<'a> IntoIterator for &'a JObj {
    type Item = (&'a JStr, &'a JVal);
    type IntoIter = std::iter::Map<std::slice::Iter<'a, (JStr, JVal)>, fn(&'a (JStr, JVal)) -> (&'a JStr, &'a JVal)>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter().map(|(k, v)| (k, v))
    }
}
impl PartialEq for JObj {
    /// As two maps: the same keys with equal values, in any order.
    fn eq(&self, o: &JObj) -> bool {
        self.0.len() == o.0.len() && self.0.iter().all(|(k, v)| o.get(k).is_some_and(|w| v == w))
    }
}
impl fmt::Debug for JObj {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.0.iter().map(|(k, v)| (k, v))).finish()
    }
}

impl JVal {
    /// A number (non-finite → null, as `Value::from(f64)`).
    #[inline]
    pub fn num(x: f64) -> JVal {
        if x.is_finite() { JVal::Num(x) } else { JVal::Null }
    }
    #[inline]
    pub fn str(s: &str) -> JVal {
        JVal::Str(JStr::new(s))
    }
    /// `v[k]` of an object (None for anything else).
    #[inline]
    pub fn get(&self, k: &str) -> Option<&JVal> {
        match self {
            JVal::Obj(o) => o.get(k),
            _ => None,
        }
    }
    #[inline]
    pub fn get_mut(&mut self, k: &str) -> Option<&mut JVal> {
        match self {
            JVal::Obj(o) => o.get_mut(k),
            _ => None,
        }
    }
    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JVal::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }
    #[inline]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            JVal::Num(x) => Some(*x),
            _ => None,
        }
    }
    #[inline]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JVal::Bool(b) => Some(*b),
            _ => None,
        }
    }
    #[inline]
    pub fn as_array(&self) -> Option<&Vec<JVal>> {
        match self {
            JVal::Arr(a) => Some(a),
            _ => None,
        }
    }
    #[inline]
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<JVal>> {
        match self {
            JVal::Arr(a) => Some(a),
            _ => None,
        }
    }
    #[inline]
    pub fn as_object(&self) -> Option<&JObj> {
        match self {
            JVal::Obj(o) => Some(o),
            _ => None,
        }
    }
    #[inline]
    pub fn as_object_mut(&mut self) -> Option<&mut JObj> {
        match self {
            JVal::Obj(o) => Some(o),
            _ => None,
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, JVal::Null)
    }
    pub fn is_number(&self) -> bool {
        matches!(self, JVal::Num(_))
    }
    pub fn is_string(&self) -> bool {
        matches!(self, JVal::Str(_))
    }
    pub fn is_boolean(&self) -> bool {
        matches!(self, JVal::Bool(_))
    }
    pub fn is_array(&self) -> bool {
        matches!(self, JVal::Arr(_))
    }
    pub fn is_object(&self) -> bool {
        matches!(self, JVal::Obj(_))
    }
    pub fn take(&mut self) -> JVal {
        std::mem::take(self)
    }

    /// The same value as a `Value`.
    pub fn to_value(&self) -> Value {
        match self {
            JVal::Null => Value::Null,
            JVal::Bool(b) => Value::Bool(*b),
            JVal::Num(x) => Value::from(*x),
            JVal::Str(s) => Value::String(s.as_str().to_string()),
            JVal::Arr(a) => Value::Array(a.iter().map(JVal::to_value).collect()),
            JVal::Obj(o) => Value::Object(o.to_map()),
        }
    }
    /// A `Value` as a [`JVal`].
    pub fn from_value(v: &Value) -> JVal {
        match v {
            Value::Null => JVal::Null,
            Value::Bool(b) => JVal::Bool(*b),
            Value::Number(n) => JVal::num(n.as_f64().unwrap_or(f64::NAN)),
            Value::String(s) => JVal::Str(JStr::new(s)),
            Value::Array(a) => JVal::Arr(a.iter().map(JVal::from_value).collect()),
            Value::Object(o) => JVal::Obj(JObj::from_map(o)),
        }
    }

    /// `JSON.stringify` of the value into `o` (as `Out::value` writes a `Value`).
    pub fn write(&self, o: &mut cc_m3d::json::Out) {
        match self {
            JVal::Null => o.raw("null"),
            JVal::Bool(b) => o.bool(*b),
            JVal::Num(x) => o.num(*x),
            JVal::Str(s) => o.str(s),
            JVal::Arr(a) => {
                o.ch('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        o.ch(',');
                    }
                    x.write(o);
                }
                o.ch(']');
            }
            JVal::Obj(m) => {
                o.ch('{');
                for (i, (k, x)) in m.0.iter().enumerate() {
                    if i > 0 {
                        o.ch(',');
                    }
                    o.key(k);
                    x.write(o);
                }
                o.ch('}');
            }
        }
    }
    /// `JSON.stringify(v, null, indent)`'s text of the value at `depth` (as `js::stringify_indent`).
    pub fn write_indent(&self, o: &mut cc_m3d::json::Out, depth: usize, pad: &str) {
        match self {
            JVal::Arr(a) => write_list_indent(a, o, depth, pad),
            JVal::Obj(m) if !m.is_empty() && depth < 512 => {
                o.ch('{');
                for (i, (k, x)) in m.0.iter().enumerate() {
                    if i > 0 {
                        o.ch(',');
                    }
                    nl(o, depth.saturating_add(1), pad);
                    o.str(k);
                    o.raw(": ");
                    x.write_indent(o, depth.saturating_add(1), pad);
                }
                nl(o, depth, pad);
                o.ch('}');
            }
            x => x.write(o),
        }
    }
    /// `JSON.stringify(v)`.
    pub fn stringify(&self) -> String {
        let mut o = cc_m3d::json::Out::new(false);
        self.write(&mut o);
        o.s
    }
}

fn nl(o: &mut cc_m3d::json::Out, d: usize, pad: &str) {
    o.ch('\n');
    for _ in 0..d {
        o.raw(pad);
    }
}
/// A list's `JSON.stringify(list, null, indent)` text at `depth`.
pub fn write_list_indent(a: &[JVal], o: &mut cc_m3d::json::Out, depth: usize, pad: &str) {
    if a.is_empty() || depth >= 512 {
        o.ch('[');
        for (i, x) in a.iter().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            x.write(o);
        }
        o.ch(']');
        return;
    }
    o.ch('[');
    for (i, x) in a.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        nl(o, depth.saturating_add(1), pad);
        x.write_indent(o, depth.saturating_add(1), pad);
    }
    nl(o, depth, pad);
    o.ch(']');
}

impl From<Value> for JVal {
    /// A `Value` as a [`JVal`], its strings kept (or dropped for the static copy).
    fn from(v: Value) -> JVal {
        match v {
            Value::Null => JVal::Null,
            Value::Bool(b) => JVal::Bool(b),
            Value::Number(n) => JVal::num(n.as_f64().unwrap_or(f64::NAN)),
            Value::String(s) => JVal::Str(JStr::from_string(s)),
            Value::Array(a) => JVal::Arr(a.into_iter().map(JVal::from).collect()),
            Value::Object(o) => JVal::Obj(JObj::from_map_owned(o)),
        }
    }
}
impl From<&Value> for JVal {
    fn from(v: &Value) -> JVal {
        JVal::from_value(v)
    }
}
impl From<f64> for JVal {
    fn from(x: f64) -> JVal {
        JVal::num(x)
    }
}
impl From<bool> for JVal {
    fn from(b: bool) -> JVal {
        JVal::Bool(b)
    }
}
impl From<&str> for JVal {
    fn from(s: &str) -> JVal {
        JVal::str(s)
    }
}
impl From<String> for JVal {
    fn from(s: String) -> JVal {
        JVal::Str(JStr::from_string(s))
    }
}
impl From<JStr> for JVal {
    fn from(s: JStr) -> JVal {
        JVal::Str(s)
    }
}
impl From<JObj> for JVal {
    fn from(o: JObj) -> JVal {
        JVal::Obj(o)
    }
}
impl From<Vec<JVal>> for JVal {
    fn from(a: Vec<JVal>) -> JVal {
        JVal::Arr(a)
    }
}

impl PartialEq for JVal {
    /// As `Value == Value`, numbers by value.
    fn eq(&self, o: &JVal) -> bool {
        match (self, o) {
            (JVal::Null, JVal::Null) => true,
            (JVal::Bool(a), JVal::Bool(b)) => a == b,
            (JVal::Num(a), JVal::Num(b)) => a == b,
            (JVal::Str(a), JVal::Str(b)) => a == b,
            (JVal::Arr(a), JVal::Arr(b)) => a == b,
            (JVal::Obj(a), JVal::Obj(b)) => a == b,
            _ => false,
        }
    }
}
impl PartialEq<str> for JVal {
    fn eq(&self, o: &str) -> bool {
        self.as_str() == Some(o)
    }
}
impl PartialEq<&str> for JVal {
    fn eq(&self, o: &&str) -> bool {
        self.as_str() == Some(*o)
    }
}
impl std::ops::Index<&str> for JVal {
    type Output = JVal;
    /// `v[k]` (null when absent, as `Value`'s index).
    fn index(&self, k: &str) -> &JVal {
        self.get(k).unwrap_or(&NULL)
    }
}
impl std::ops::Index<usize> for JVal {
    type Output = JVal;
    fn index(&self, i: usize) -> &JVal {
        self.as_array().and_then(|a| a.get(i)).unwrap_or(&NULL)
    }
}
impl fmt::Debug for JVal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JVal::Null => f.write_str("null"),
            JVal::Bool(b) => write!(f, "{b}"),
            JVal::Num(x) => write!(f, "{x}"),
            JVal::Str(s) => write!(f, "{s:?}"),
            JVal::Arr(a) => f.debug_list().entries(a).finish(),
            JVal::Obj(o) => fmt::Debug::fmt(o, f),
        }
    }
}
impl fmt::Display for JVal {
    /// The value's JSON text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.stringify())
    }
}

impl serde::Serialize for JVal {
    /// As the `Value` would serialize (numbers as f64).
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeSeq};
        match self {
            JVal::Null => ser.serialize_unit(),
            JVal::Bool(b) => ser.serialize_bool(*b),
            JVal::Num(x) => ser.serialize_f64(*x),
            JVal::Str(s) => ser.serialize_str(s),
            JVal::Arr(a) => {
                let mut q = ser.serialize_seq(Some(a.len()))?;
                for x in a {
                    q.serialize_element(x)?;
                }
                q.end()
            }
            JVal::Obj(o) => {
                let mut m = ser.serialize_map(Some(o.len()))?;
                for (k, v) in &o.0 {
                    m.serialize_entry(k.as_str(), v)?;
                }
                m.end()
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Straight from text (the undo snapshots and the model's records): what `Value`'s parse takes, as a JVal

impl<'de> serde::Deserialize<'de> for JVal {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<JVal, D::Error> {
        d.deserialize_any(JValV)
    }
}
struct JValV;
impl<'de> Visitor<'de> for JValV {
    type Value = JVal;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any JSON value")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<JVal, E> {
        Ok(JVal::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<JVal, E> {
        Ok(JVal::num(v as f64))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<JVal, E> {
        Ok(JVal::num(v as f64))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<JVal, E> {
        // serde_json's Value keeps a non-finite float as null
        Ok(JVal::num(v))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<JVal, E> {
        Ok(JVal::Str(JStr::new(v)))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<JVal, E> {
        Ok(JVal::Str(JStr::from_string(v)))
    }
    fn visit_unit<E: de::Error>(self) -> Result<JVal, E> {
        Ok(JVal::Null)
    }
    fn visit_none<E: de::Error>(self) -> Result<JVal, E> {
        Ok(JVal::Null)
    }
    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<JVal, D::Error> {
        serde::Deserialize::deserialize(d)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<JVal, A::Error> {
        let mut out = Vec::with_capacity(a.size_hint().unwrap_or(0).min(4096));
        while let Some(x) = a.next_element::<JVal>()? {
            out.push(x);
        }
        Ok(JVal::Arr(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<JVal, A::Error> {
        let mut o = JObj::new();
        while let Some(k) = a.next_key::<KeyStr>()? {
            let v = a.next_value::<JVal>()?;
            // a key given twice: the last value in the first one's place (as Value's map)
            o.insert(k.0, v);
        }
        Ok(JVal::Obj(o))
    }
}
/// An object key as a [`JStr`].
struct KeyStr(JStr);
impl<'de> serde::Deserialize<'de> for KeyStr {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<KeyStr, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = KeyStr;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a key")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<KeyStr, E> {
                Ok(KeyStr(JStr::new(v)))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<KeyStr, E> {
                Ok(KeyStr(JStr::from_string(v)))
            }
        }
        d.deserialize_str(V)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// JS semantics of a value, for either kind (the js.rs helpers are generic over this)

/// What the JS helpers ask of a JSON value: its kind and contents.
pub trait Jsv {
    /// what an array of it holds
    type Item: Jsv;
    fn js_str(&self) -> Option<&str>;
    fn js_f64(&self) -> Option<f64>;
    fn js_bool(&self) -> Option<bool>;
    fn js_null(&self) -> bool;
    fn js_arr(&self) -> Option<&[Self::Item]>;
    /// `v[k]` of an object (None for anything else)
    fn js_get(&self, k: &str) -> Option<&Self::Item>;
    /// an array or an object
    fn js_composite(&self) -> bool;
    /// the value as a `Value` (for what only the `Value` helpers do: `String([..])`, `Number([..])`)
    fn js_value(&self) -> std::borrow::Cow<'_, Value>;
}
impl<T: Jsv> Jsv for &T {
    type Item = T::Item;
    #[inline]
    fn js_str(&self) -> Option<&str> {
        (**self).js_str()
    }
    #[inline]
    fn js_f64(&self) -> Option<f64> {
        (**self).js_f64()
    }
    #[inline]
    fn js_bool(&self) -> Option<bool> {
        (**self).js_bool()
    }
    #[inline]
    fn js_null(&self) -> bool {
        (**self).js_null()
    }
    #[inline]
    fn js_arr(&self) -> Option<&[T::Item]> {
        (**self).js_arr()
    }
    #[inline]
    fn js_get(&self, k: &str) -> Option<&T::Item> {
        (**self).js_get(k)
    }
    #[inline]
    fn js_composite(&self) -> bool {
        (**self).js_composite()
    }
    #[inline]
    fn js_value(&self) -> std::borrow::Cow<'_, Value> {
        (**self).js_value()
    }
}
impl Jsv for Value {
    type Item = Value;
    #[inline]
    fn js_str(&self) -> Option<&str> {
        self.as_str()
    }
    #[inline]
    fn js_f64(&self) -> Option<f64> {
        self.as_f64()
    }
    #[inline]
    fn js_bool(&self) -> Option<bool> {
        self.as_bool()
    }
    #[inline]
    fn js_null(&self) -> bool {
        self.is_null()
    }
    #[inline]
    fn js_arr(&self) -> Option<&[Value]> {
        self.as_array().map(Vec::as_slice)
    }
    #[inline]
    fn js_get(&self, k: &str) -> Option<&Value> {
        self.as_object().and_then(|m| m.get(k))
    }
    #[inline]
    fn js_composite(&self) -> bool {
        self.is_array() || self.is_object()
    }
    #[inline]
    fn js_value(&self) -> std::borrow::Cow<'_, Value> {
        std::borrow::Cow::Borrowed(self)
    }
}
impl Jsv for JVal {
    type Item = JVal;
    #[inline]
    fn js_str(&self) -> Option<&str> {
        self.as_str()
    }
    #[inline]
    fn js_f64(&self) -> Option<f64> {
        self.as_f64()
    }
    #[inline]
    fn js_bool(&self) -> Option<bool> {
        self.as_bool()
    }
    #[inline]
    fn js_null(&self) -> bool {
        self.is_null()
    }
    #[inline]
    fn js_arr(&self) -> Option<&[JVal]> {
        self.as_array().map(Vec::as_slice)
    }
    #[inline]
    fn js_get(&self, k: &str) -> Option<&JVal> {
        self.get(k)
    }
    #[inline]
    fn js_composite(&self) -> bool {
        self.is_array() || self.is_object()
    }
    #[inline]
    fn js_value(&self) -> std::borrow::Cow<'_, Value> {
        std::borrow::Cow::Owned(self.to_value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cases() -> Vec<Value> {
        let mut out = vec![
            serde_json::json!(null),
            serde_json::json!(true),
            serde_json::json!(-0.0),
            serde_json::json!(1e21),
            serde_json::json!(12345678901234567u64),
            serde_json::json!(-7),
            serde_json::json!("A992"),
            serde_json::json!("a \"q\" \u{0}\u{1f} é 𝄞"),
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!({"id": "m1", "type": "beam", "start": [0, 1.5, -2], "holes": [{"face": "web", "x": 1}], "zz": {"a": [[{}]]}}),
        ];
        if let Ok(m) = crate::sample::sample_model("t") {
            out.push(m.state.to_value());
        }
        out
    }

    #[test]
    fn jval_round_trips() {
        for v in cases() {
            let j = JVal::from_value(&v);
            // numbers come back as f64 (a Value keeps 1 and 1.0 apart; JSON text and JS do not)
            let st = crate::js::stringify;
            assert_eq!(st(&j.to_value()), st(&v), "{v}");
            assert_eq!(st(&JVal::from(v.clone()).to_value()), st(&v));
            assert_eq!(j.stringify(), crate::js::stringify(&v));
            let mut o = cc_m3d::json::Out::new(false);
            j.write_indent(&mut o, 0, " ");
            assert_eq!(o.s, crate::js::stringify_indent(&v, 1));
            let text = crate::js::stringify(&v);
            let back: JVal = serde_json::from_str(&text).expect("parse");
            assert_eq!(st(&back.to_value()), st(&crate::js::parse(&text).expect("parse")));
            assert!(JVal::from_value(&back.to_value()) == back);
        }
        // a key twice keeps the first place and the last value, as Value's map does
        let t = r#"{"a":1,"b":2,"a":3}"#;
        let j: JVal = serde_json::from_str(t).expect("parse");
        assert_eq!(j.stringify(), crate::js::stringify(&crate::js::parse(t).expect("parse")));
        let mut o = JObj::new();
        o.insert("a", JVal::num(1.0));
        o.insert("b", JVal::num(2.0));
        o.insert("c", JVal::num(3.0));
        o.remove("a");
        let mut m = Map::new();
        m.insert("a".into(), Value::from(1.0));
        m.insert("b".into(), Value::from(2.0));
        m.insert("c".into(), Value::from(3.0));
        m.remove("a");
        assert_eq!(JVal::Obj(o).stringify(), crate::js::stringify(&Value::Object(m)));
        assert!(matches!(JStr::new("A992"), JStr::S(_)));
        assert!(matches!(JStr::new("m17"), JStr::H(_)));
        assert!(JVal::num(f64::NAN).is_null());
    }
}
