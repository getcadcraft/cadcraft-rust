//! S.15: a typed read of a model file's text. `read_model` on text used to parse the whole file into serde_json
//! values (a map for every member, hole, connection and its params) and then read those; here the members and the
//! connections are read straight from the text into fixed slots for the fields the reader asks for (everything else is
//! still parsed in full, so a file this refuses is one the JSON parse refuses), and the rest of the file is the same
//! JSON values as before. [`crate::model::read_model`] then reads both the same way through [`Src`]
//! (`typed_read_reads_as_the_json_does`: 20,000+ random and broken files give the same state and warnings, or the
//! same error).

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::fmt;

/// What the reader asks of a JSON value it walks (a member, a hole, a cope, a connection): `truthy(v)`, `v === null`
/// and `v.k` (only objects have fields).
pub trait Src {
    fn is_null(&self) -> bool;
    fn truthy(&self) -> bool;
    fn get(&self, k: &str) -> Option<&Value>;
}
impl Src for Value {
    fn is_null(&self) -> bool {
        Value::is_null(self)
    }
    fn truthy(&self) -> bool {
        crate::js::truthy(Some(self))
    }
    fn get(&self, k: &str) -> Option<&Value> {
        self.as_object().and_then(|m| m.get(k))
    }
}
/// A member: its fields, and the cuts / holes lists (the first `cap` of an array; nothing for anything else).
pub trait MemberSrc: Src {
    type Item: Src;
    fn list(&self, k: &str, cap: usize) -> &[Self::Item];
}
impl MemberSrc for Value {
    type Item = Value;
    fn list(&self, k: &str, cap: usize) -> &[Value] {
        let a = crate::js::arr(Src::get(self, k));
        a.get(..a.len().min(cap)).unwrap_or(a)
    }
}
/// A connection: its fields, and `c.params?.[k]` (only an object's).
pub trait ConnSrc: Src {
    fn param(&self, k: &str) -> Option<&Value>;
}
impl ConnSrc for Value {
    fn param(&self, k: &str) -> Option<&Value> {
        Src::get(self, "params").and_then(|p| p.as_object()).and_then(|m| m.get(k))
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The typed parse

/// What a value was, beside the fields kept.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Kind {
    #[default]
    Null,
    Obj,
    Other(bool),
}

/// An object key, borrowed from the text when it has no escapes.
struct Key<'de>(Cow<'de, str>);
impl<'de> Deserialize<'de> for Key<'de> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Key<'de>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a key")
            }
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Key<'de>, E> {
                Ok(Key(Cow::Borrowed(v)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Key<'de>, E> {
                Ok(Key(Cow::Owned(v.to_string())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Key<'de>, E> {
                Ok(Key(Cow::Owned(v)))
            }
        }
        d.deserialize_str(V)
    }
}

/// Any JSON value, parsed in full (numbers, strings, depth: as a `Value` would be) and dropped.
struct Skip;
impl<'de> Deserialize<'de> for Skip {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(SkipV).map(|_| Skip)
    }
}
struct SkipV;
impl<'de> Visitor<'de> for SkipV {
    type Value = bool;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any value")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<bool, E> {
        Ok(v)
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<bool, E> {
        Ok(v != 0)
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<bool, E> {
        Ok(v != 0)
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<bool, E> {
        Ok(v != 0.0 && !v.is_nan())
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<bool, E> {
        Ok(!v.is_empty())
    }
    fn visit_unit<E: de::Error>(self) -> Result<bool, E> {
        Ok(false)
    }
    fn visit_none<E: de::Error>(self) -> Result<bool, E> {
        Ok(false)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<bool, A::Error> {
        while a.next_element::<Skip>()?.is_some() {}
        Ok(true)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<bool, A::Error> {
        while a.next_key::<Key<'de>>()?.is_some() {
            a.next_value::<Skip>()?;
        }
        Ok(true)
    }
}

/// An object read into fixed slots: `KEYS[i]`'s value (the last one given, as JSON.parse keeps) in `v[i]`.
pub trait Slots: Default {
    const KEYS: &'static [&'static str];
    fn kind_mut(&mut self) -> &mut Kind;
    fn slot(&mut self, i: usize) -> Option<&mut Option<Value>>;
    /// A key with its own reader (lists, params): read it and say so, else false.
    fn special<'de, A: MapAccess<'de>>(&mut self, _k: &str, _a: &mut A) -> Result<bool, A::Error> {
        Ok(false)
    }
}

struct SlotsV<T>(std::marker::PhantomData<T>);
impl<'de, T: Slots> Visitor<'de> for SlotsV<T> {
    type Value = T;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any value")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<T, E> {
        other(v)
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<T, E> {
        other(v != 0)
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<T, E> {
        other(v != 0)
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<T, E> {
        other(v != 0.0 && !v.is_nan())
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<T, E> {
        other(!v.is_empty())
    }
    fn visit_unit<E: de::Error>(self) -> Result<T, E> {
        Ok(T::default())
    }
    fn visit_none<E: de::Error>(self) -> Result<T, E> {
        Ok(T::default())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<T, A::Error> {
        while a.next_element::<Skip>()?.is_some() {}
        other(true)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<T, A::Error> {
        let mut t = T::default();
        *t.kind_mut() = Kind::Obj;
        while let Some(Key(k)) = a.next_key::<Key<'de>>()? {
            if t.special(&k, &mut a)? {
                continue;
            }
            match T::KEYS.iter().position(|x| *x == k) {
                Some(i) => {
                    let v = a.next_value::<Value>()?;
                    if let Some(s) = t.slot(i) {
                        *s = Some(v);
                    }
                }
                None => {
                    a.next_value::<Skip>()?;
                }
            }
        }
        Ok(t)
    }
}
fn other<T: Slots, E>(truthy: bool) -> Result<T, E> {
    let mut t = T::default();
    *t.kind_mut() = Kind::Other(truthy);
    Ok(t)
}
fn slots<'de, T: Slots, D: Deserializer<'de>>(d: D) -> Result<T, D::Error> {
    d.deserialize_any(SlotsV::<T>(std::marker::PhantomData))
}

macro_rules! slotted {
    ($name:ident, $n:expr, [$($k:expr),*]) => {
        #[derive(Default, Debug)]
        pub struct $name {
            kind: Kind,
            v: [Option<Value>; $n],
        }
        impl Slots for $name {
            const KEYS: &'static [&'static str] = &[$($k),*];
            fn kind_mut(&mut self) -> &mut Kind {
                &mut self.kind
            }
            fn slot(&mut self, i: usize) -> Option<&mut Option<Value>> {
                self.v.get_mut(i)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                slots(d)
            }
        }
        impl Src for $name {
            fn is_null(&self) -> bool {
                self.kind == Kind::Null
            }
            fn truthy(&self) -> bool {
                match self.kind {
                    Kind::Null => false,
                    Kind::Obj => true,
                    Kind::Other(t) => t,
                }
            }
            fn get(&self, k: &str) -> Option<&Value> {
                let i = Self::KEYS.iter().position(|x| *x == k)?;
                self.v.get(i).and_then(Option::as_ref)
            }
        }
    };
}

slotted!(Part, 10, ["id", "kind", "end", "side", "length", "depth", "face", "x", "y", "d"]);

/// A list as `Array.isArray(v) ? v.slice(0, CAP) : []` keeps it (what is past the cap is parsed, then dropped).
#[derive(Debug)]
pub struct List<T, const CAP: usize>(pub Vec<T>);
impl<T, const CAP: usize> Default for List<T, CAP> {
    fn default() -> Self {
        List(Vec::new())
    }
}
impl<'de, T: Deserialize<'de>, const CAP: usize> Deserialize<'de> for List<T, CAP> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<T, const CAP: usize>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>, const CAP: usize> Visitor<'de> for V<T, CAP> {
            type Value = List<T, CAP>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any value")
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(List::default())
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Ok(List::default())
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Ok(List::default())
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(List::default())
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(List::default())
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(List::default())
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(List::default())
            }
            fn visit_map<A: MapAccess<'de>>(self, a: A) -> Result<Self::Value, A::Error> {
                SkipV.visit_map(a)?;
                Ok(List::default())
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                // the model's lists start with room for some (a member's holes and copes start empty)
                let mut out = Vec::with_capacity(if CAP >= 20000 { 64 } else { 0 });
                while out.len() < CAP {
                    match a.next_element::<T>()? {
                        Some(x) => out.push(x),
                        None => return Ok(List(out)),
                    }
                }
                while a.next_element::<Skip>()?.is_some() {}
                Ok(List(out))
            }
        }
        d.deserialize_any(V::<T, CAP>(std::marker::PhantomData))
    }
}

const MEM_KEYS: [&str; 10] = ["id", "type", "profile", "material", "start", "end", "rotation", "position", "name", "phase"];

/// A member as the reader asks for it.
#[derive(Default, Debug)]
pub struct MemberIn {
    kind: Kind,
    v: [Option<Value>; 10],
    cuts: Option<List<Part, 50>>,
    holes: Option<List<Part, 2000>>,
}
impl Slots for MemberIn {
    const KEYS: &'static [&'static str] = &MEM_KEYS;
    fn kind_mut(&mut self) -> &mut Kind {
        &mut self.kind
    }
    fn slot(&mut self, i: usize) -> Option<&mut Option<Value>> {
        self.v.get_mut(i)
    }
    fn special<'de, A: MapAccess<'de>>(&mut self, k: &str, a: &mut A) -> Result<bool, A::Error> {
        match k {
            "cuts" => self.cuts = Some(a.next_value()?),
            "holes" => self.holes = Some(a.next_value()?),
            _ => return Ok(false),
        }
        Ok(true)
    }
}
impl<'de> Deserialize<'de> for MemberIn {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        slots(d)
    }
}
impl Src for MemberIn {
    fn is_null(&self) -> bool {
        self.kind == Kind::Null
    }
    fn truthy(&self) -> bool {
        match self.kind {
            Kind::Null => false,
            Kind::Obj => true,
            Kind::Other(t) => t,
        }
    }
    fn get(&self, k: &str) -> Option<&Value> {
        let i = MEM_KEYS.iter().position(|x| *x == k)?;
        self.v.get(i).and_then(Option::as_ref)
    }
}
impl MemberSrc for MemberIn {
    type Item = Part;
    fn list(&self, k: &str, cap: usize) -> &[Part] {
        let l: &[Part] = match k {
            "cuts" => self.cuts.as_ref().map_or(&[], |l| l.0.as_slice()),
            "holes" => self.holes.as_ref().map_or(&[], |l| l.0.as_slice()),
            _ => &[],
        };
        l.get(..l.len().min(cap)).unwrap_or(l)
    }
}

const CONN_KEYS: [&str; 10] = ["id", "type", "a", "aEnd", "b", "brace", "support", "end", "column", "beam"];

/// A connection as the reader asks for it: its fields, and its params as given (an object's entries; last one wins).
#[derive(Default, Debug)]
pub struct ConnIn {
    kind: Kind,
    v: [Option<Value>; 10],
    params: Option<Params>,
}
impl Slots for ConnIn {
    const KEYS: &'static [&'static str] = &CONN_KEYS;
    fn kind_mut(&mut self) -> &mut Kind {
        &mut self.kind
    }
    fn slot(&mut self, i: usize) -> Option<&mut Option<Value>> {
        self.v.get_mut(i)
    }
    fn special<'de, A: MapAccess<'de>>(&mut self, k: &str, a: &mut A) -> Result<bool, A::Error> {
        if k == "params" {
            self.params = Some(a.next_value()?);
            return Ok(true);
        }
        Ok(false)
    }
}
impl<'de> Deserialize<'de> for ConnIn {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        slots(d)
    }
}
impl Src for ConnIn {
    fn is_null(&self) -> bool {
        self.kind == Kind::Null
    }
    fn truthy(&self) -> bool {
        match self.kind {
            Kind::Null => false,
            Kind::Obj => true,
            Kind::Other(t) => t,
        }
    }
    fn get(&self, k: &str) -> Option<&Value> {
        let i = CONN_KEYS.iter().position(|x| *x == k)?;
        self.v.get(i).and_then(Option::as_ref)
    }
}
impl ConnSrc for ConnIn {
    fn param(&self, k: &str) -> Option<&Value> {
        let i = crate::state::param_names().iter().position(|n| *n == k)?;
        self.params.as_ref()?.0.iter().rev().find(|(x, _)| usize::from(*x) == i).map(|(_, v)| v)
    }
}

/// A connection's params: an object's entries for the defaults' names (by their number in
/// [`crate::state::param_names`], in order; the reader asks for no other name), nothing for anything but an object.
#[derive(Default, Debug)]
pub struct Params(Vec<(u16, Value)>);
impl<'de> Deserialize<'de> for Params {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Params;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any value")
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Params, E> {
                Ok(Params::default())
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Params, E> {
                Ok(Params::default())
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Params, E> {
                Ok(Params::default())
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Params, E> {
                Ok(Params::default())
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Params, E> {
                Ok(Params::default())
            }
            fn visit_unit<E: de::Error>(self) -> Result<Params, E> {
                Ok(Params::default())
            }
            fn visit_none<E: de::Error>(self) -> Result<Params, E> {
                Ok(Params::default())
            }
            fn visit_seq<A: SeqAccess<'de>>(self, a: A) -> Result<Params, A::Error> {
                SkipV.visit_seq(a)?;
                Ok(Params::default())
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Params, A::Error> {
                let names = crate::state::param_names();
                let mut out = Vec::with_capacity(names.len().min(24));
                while let Some(Key(k)) = a.next_key::<Key<'de>>()? {
                    match names.iter().position(|n| *n == k).and_then(|i| u16::try_from(i).ok()) {
                        Some(i) => out.push((i, a.next_value::<Value>()?)),
                        None => {
                            a.next_value::<Skip>()?;
                        }
                    }
                }
                Ok(Params(out))
            }
        }
        d.deserialize_any(V)
    }
}

/// The file: its members and connections typed, every other field as JSON (only an object is a file here).
#[derive(Default, Debug)]
pub struct FileIn {
    pub rest: Map<String, Value>,
    pub members: List<MemberIn, 20000>,
    pub connections: List<ConnIn, 20000>,
}
impl<'de> Deserialize<'de> for FileIn {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = FileIn;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a model object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<FileIn, A::Error> {
                let mut out = FileIn::default();
                while let Some(Key(k)) = a.next_key::<Key<'de>>()? {
                    match &*k {
                        "members" => out.members = a.next_value()?,
                        "connections" => out.connections = a.next_value()?,
                        _ => {
                            let v = a.next_value::<Value>()?;
                            out.rest.insert(k.into_owned(), v);
                        }
                    }
                }
                Ok(out)
            }
        }
        d.deserialize_map(V)
    }
}

/// The typed parse of a file's text; `None` when it is not a JSON object (the caller then reads it as JSON, which
/// gives the same error or result).
pub fn parse_file(text: &str) -> Option<FileIn> {
    serde_json::from_str::<FileIn>(text).ok()
}
