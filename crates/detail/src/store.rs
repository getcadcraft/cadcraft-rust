//! S.16: the model's state. model.js keeps it as one plain object; here the members, connections and footings are
//! lists of compact records ([`JVal`]: no map, no allocation per key or per word, see jv.rs) and every other field
//! (name, grids, levels, phases, profiles, stairs, bolts, issues, numbering, seq …) is a JSON value as before. The
//! fields keep their order (a list keeps its place among them), so [`State::to_value`], [`State::stringify`] and
//! [`State::stringify_indent`] are the state's own JSON text, byte for byte: what the file, the undo snapshots and
//! Studio Live are made of.

use crate::jv::JVal;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::fmt;

/// The fields held as records.
pub const LISTS: [&str; 3] = ["members", "connections", "footings"];

#[inline]
fn list_index(k: &str) -> Option<usize> {
    match k {
        "members" => Some(0),
        "connections" => Some(1),
        "footings" => Some(2),
        _ => None,
    }
}

/// The state (see the module's head).
#[derive(Clone, Debug, Default)]
pub struct State {
    /// a state that is not an object (only a script's own can be), as given
    other: Option<Value>,
    /// every field in order; a list held as records has a null in its place here
    rest: Map<String, Value>,
    /// which of [`LISTS`] are held as records (the others, when present, are a value in `rest` that is no array)
    held: [bool; 3],
    pub members: Vec<JVal>,
    pub connections: Vec<JVal>,
    pub footings: Vec<JVal>,
}

impl State {
    /// An empty object.
    pub fn new() -> State {
        State::default()
    }
    /// A state from its JSON value.
    pub fn from_value(v: Value) -> State {
        let Value::Object(m) = v else {
            return State { other: Some(v), ..State::default() };
        };
        let mut s = State { rest: Map::with_capacity(m.len()), ..State::default() };
        for (k, v) in m {
            s.insert(&k, v);
        }
        s
    }
    /// The state's JSON value.
    pub fn to_value(&self) -> Value {
        if let Some(v) = &self.other {
            return v.clone();
        }
        let mut m = Map::with_capacity(self.rest.len());
        for (k, v) in &self.rest {
            let x = match self.held_list(k) {
                Some(l) => Value::Array(l.iter().map(JVal::to_value).collect()),
                None => v.clone(),
            };
            m.insert(k.clone(), x);
        }
        Value::Object(m)
    }
    /// Whether the state is an object (as every model's is).
    pub fn is_object(&self) -> bool {
        self.other.is_none()
    }

    /// One of [`LISTS`] when it is an array (`Array.isArray(s[k]) && s[k]`).
    pub fn held_list(&self, k: &str) -> Option<&[JVal]> {
        let i = list_index(k)?;
        if !self.held.get(i).copied().unwrap_or(false) {
            return None;
        }
        Some(match i {
            0 => &self.members,
            1 => &self.connections,
            _ => &self.footings,
        })
    }
    fn list_vec_mut(&mut self, i: usize) -> &mut Vec<JVal> {
        match i {
            0 => &mut self.members,
            1 => &mut self.connections,
            _ => &mut self.footings,
        }
    }

    /// `s[k]` of a field that is not one of [`LISTS`] (those are read as [`State::list`]).
    #[inline]
    pub fn get(&self, k: &str) -> Option<&Value> {
        debug_assert!(list_index(k).is_none_or(|i| !self.held.get(i).copied().unwrap_or(false)), "{k} is held as records");
        if list_index(k).is_some_and(|i| self.held.get(i).copied().unwrap_or(false)) {
            return None;
        }
        self.rest.get(k)
    }
    /// `s[k]` to change, of a field that is not a held list.
    pub fn get_mut(&mut self, k: &str) -> Option<&mut Value> {
        if list_index(k).is_some_and(|i| self.held.get(i).copied().unwrap_or(false)) {
            return None;
        }
        self.rest.get_mut(k)
    }
    /// `Array.isArray(s[k]) ? s[k] : []` for one of [`LISTS`].
    #[inline]
    pub fn list(&self, k: &str) -> &[JVal] {
        self.held_list(k).unwrap_or(&[])
    }
    /// The field `k` as `Value` (a list made from its records): for the few readers that walk any field.
    pub fn field_value(&self, k: &str) -> Option<std::borrow::Cow<'_, Value>> {
        if let Some(l) = self.held_list(k) {
            return Some(std::borrow::Cow::Owned(Value::Array(l.iter().map(JVal::to_value).collect())));
        }
        self.rest.get(k).map(std::borrow::Cow::Borrowed)
    }
    /// The item of the list `k` (a held list or any other field's array) whose id is `id`, as JSON.
    pub fn find_value(&self, k: &str, id: &str) -> Option<Value> {
        match self.held_list(k) {
            Some(l) => l.iter().find(|x| x.get("id").and_then(JVal::as_str) == Some(id)).map(JVal::to_value),
            None => crate::js::arr(self.get(k)).iter().find(|x| x.get("id").and_then(Value::as_str) == Some(id)).cloned(),
        }
    }
    /// Whether the list `k` has an item whose id is `id`.
    pub fn has_id(&self, k: &str, id: &str) -> bool {
        match self.held_list(k) {
            Some(l) => l.iter().any(|x| x.get("id").and_then(JVal::as_str) == Some(id)),
            None => crate::js::arr(self.get(k)).iter().any(|x| x.get("id").and_then(Value::as_str) == Some(id)),
        }
    }
    /// The length of the list `k` (`Array.isArray(s[k]) ? s[k].length : 0`).
    pub fn len_of(&self, k: &str) -> usize {
        match self.held_list(k) {
            Some(l) => l.len(),
            None => crate::js::arr(self.get(k)).len(),
        }
    }
    /// Whether the field is there (as `k in s`).
    pub fn has(&self, k: &str) -> bool {
        self.other.is_none() && self.rest.contains_key(k)
    }
    /// The fields' names in order.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.rest.keys()
    }

    /// The state as an object (a state that was none becomes `{}`, as the edits' `m_of` makes it).
    fn obj(&mut self) {
        if self.other.is_some() {
            self.other = None;
            self.rest = Map::new();
            self.held = [false; 3];
            self.members.clear();
            self.connections.clear();
            self.footings.clear();
        }
    }
    /// `s[k] = v`: a field given before keeps its place; a list given as an array is held as records.
    pub fn insert(&mut self, k: &str, v: Value) {
        self.obj();
        match list_index(k) {
            Some(i) => match v {
                Value::Array(a) => {
                    let recs: Vec<JVal> = a.into_iter().map(JVal::from).collect();
                    self.set_list(i, k, recs);
                }
                v => {
                    if let Some(h) = self.held.get_mut(i) {
                        *h = false;
                    }
                    self.list_vec_mut(i).clear();
                    self.rest.insert(k.to_string(), v);
                }
            },
            None => {
                self.rest.insert(k.to_string(), v);
            }
        }
    }
    fn set_list(&mut self, i: usize, k: &str, recs: Vec<JVal>) {
        if let Some(h) = self.held.get_mut(i) {
            *h = true;
        }
        *self.list_vec_mut(i) = recs;
        self.rest.insert(k.to_string(), Value::Null);
    }
    /// `s[k] = list` for one of [`LISTS`].
    pub fn set_records(&mut self, k: &str, recs: Vec<JVal>) {
        self.obj();
        if let Some(i) = list_index(k) {
            self.set_list(i, k, recs);
        }
    }
    /// One of [`LISTS`] to change, made an array first when it is not one (as the edits' `arr_mut`: an absent list
    /// is added at the end, any other value replaced in its place).
    pub fn list_mut(&mut self, k: &str) -> &mut Vec<JVal> {
        self.obj();
        let i = list_index(k).unwrap_or(0);
        if !self.held.get(i).copied().unwrap_or(false) {
            self.set_list(i, k, Vec::new());
        }
        self.list_vec_mut(i)
    }
    /// One of [`LISTS`] to change when it is an array (nothing is made).
    pub fn held_mut(&mut self, k: &str) -> Option<&mut Vec<JVal>> {
        let i = list_index(k)?;
        if !self.held.get(i).copied().unwrap_or(false) {
            return None;
        }
        Some(self.list_vec_mut(i))
    }
    /// The state as an object to change (see `obj`).
    pub fn obj_mut(&mut self) -> &mut State {
        self.obj();
        self
    }
    /// A field that is not one of [`LISTS`], made an array first (as the edits' `arr_mut`).
    pub fn arr_mut(&mut self, k: &str) -> &mut Vec<Value> {
        self.obj();
        let e = self.rest.entry(k.to_string()).or_insert_with(|| Value::Array(vec![]));
        if !e.is_array() {
            *e = Value::Array(vec![]);
        }
        match e {
            Value::Array(a) => a,
            _ => unreachable_vec(),
        }
    }
    /// The members, made an array first (see [`State::list_mut`]).
    pub fn members_mut(&mut self) -> &mut Vec<JVal> {
        self.list_mut("members")
    }
    pub fn connections_mut(&mut self) -> &mut Vec<JVal> {
        self.list_mut("connections")
    }
    pub fn footings_mut(&mut self) -> &mut Vec<JVal> {
        self.list_mut("footings")
    }

    /// `JSON.stringify(state)`.
    pub fn stringify(&self) -> String {
        if let Some(v) = &self.other {
            return crate::js::stringify(v);
        }
        let mut o = cc_m3d::json::Out::new(false);
        o.s.reserve(64 * (self.members.len() + self.connections.len() + self.footings.len()) + 256);
        o.ch('{');
        for (i, (k, v)) in self.rest.iter().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            o.key(k);
            match self.held_list(k) {
                Some(l) => {
                    o.ch('[');
                    for (j, x) in l.iter().enumerate() {
                        if j > 0 {
                            o.ch(',');
                        }
                        x.write(&mut o);
                    }
                    o.ch(']');
                }
                None => o.value(v),
            }
        }
        o.ch('}');
        o.s
    }
    /// `JSON.stringify(state, null, indent)`.
    pub fn stringify_indent(&self, indent: usize) -> String {
        if let Some(v) = &self.other {
            return crate::js::stringify_indent(v, indent);
        }
        if self.rest.is_empty() {
            return "{}".into();
        }
        let pad = " ".repeat(indent.min(10));
        let mut o = cc_m3d::json::Out::new(false);
        o.ch('{');
        for (i, (k, v)) in self.rest.iter().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            o.ch('\n');
            o.raw(&pad);
            o.str(k);
            o.raw(": ");
            match self.held_list(k) {
                Some(l) => crate::jv::write_list_indent(l, &mut o, 1, &pad),
                None => crate::js::write_indent(v, &mut o, 1, &pad),
            }
        }
        o.ch('\n');
        o.ch('}');
        o.s
    }
    /// `JSON.parse(text)` of a state's text (an error for anything that is not JSON).
    pub fn parse(text: &str) -> Result<State, String> {
        serde_json::from_str::<State>(text).map_err(|e| e.to_string())
    }
}

static NULL: Value = Value::Null;
impl std::ops::Index<&str> for State {
    type Output = Value;
    /// `s[k]` of a field that is not a held list (null when absent, as `Value`'s index).
    fn index(&self, k: &str) -> &Value {
        self.get(k).unwrap_or(&NULL)
    }
}

impl serde::Serialize for State {
    /// As its JSON value would serialize.
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        if let Some(v) = &self.other {
            return v.serialize(ser);
        }
        let mut m = ser.serialize_map(Some(self.rest.len()))?;
        for (k, v) in &self.rest {
            match self.held_list(k) {
                Some(l) => m.serialize_entry(k, l)?,
                None => m.serialize_entry(k, v)?,
            }
        }
        m.end()
    }
}

#[allow(clippy::unreachable)]
fn unreachable_vec() -> &'static mut Vec<Value> {
    unreachable!("arr_mut makes the value an array first")
}

impl<'de> serde::Deserialize<'de> for State {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<State, D::Error> {
        d.deserialize_any(StateV)
    }
}
struct StateV;
fn other<E>(v: Value) -> Result<State, E> {
    Ok(State { other: Some(v), ..State::default() })
}
impl<'de> Visitor<'de> for StateV {
    type Value = State;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any JSON value")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<State, E> {
        other(Value::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<State, E> {
        other(Value::from(v))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<State, E> {
        other(Value::from(v))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<State, E> {
        other(Value::from(v))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<State, E> {
        other(Value::from(v))
    }
    fn visit_unit<E: de::Error>(self) -> Result<State, E> {
        other(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<State, A::Error> {
        let mut out = Vec::new();
        while let Some(x) = a.next_element::<Value>()? {
            out.push(x);
        }
        other(Value::Array(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<State, A::Error> {
        let mut s = State::default();
        while let Some(k) = a.next_key::<String>()? {
            match list_index(&k) {
                Some(i) => match a.next_value::<JVal>()? {
                    JVal::Arr(recs) => s.set_list(i, &k, recs),
                    v => s.insert(&k, v.to_value()),
                },
                None => {
                    let v = a.next_value::<Value>()?;
                    s.insert(&k, v);
                }
            }
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_text_is_the_values() {
        let mut cases = vec![
            serde_json::json!({}),
            serde_json::json!({"members": 5, "a": 1, "footings": null}),
            serde_json::json!({"z": [], "members": [], "connections": [{"id": "j1", "params": {"tp": 20}}, null, 3], "footings": "x"}),
            serde_json::json!([1, {"members": []}]),
            serde_json::json!("text"),
            serde_json::json!(null),
        ];
        if let Ok(m) = crate::sample::sample_model("t") {
            cases.push(m.state.to_value());
        }
        for v in cases {
            let s = State::from_value(v.clone());
            assert_eq!(crate::js::stringify(&s.to_value()), crate::js::stringify(&v));
            assert_eq!(s.stringify(), crate::js::stringify(&v));
            assert_eq!(s.stringify_indent(1), crate::js::stringify_indent(&v, 1));
            let back = State::parse(&s.stringify()).expect("parse");
            assert_eq!(back.stringify(), crate::js::stringify(&crate::js::parse(&s.stringify()).expect("parse")));
        }
        // a list made an array keeps its place, or goes at the end when absent
        let mut s = State::from_value(serde_json::json!({"members": 5, "b": 1}));
        s.members_mut();
        assert_eq!(s.stringify(), r#"{"members":[],"b":1}"#);
        s.footings_mut();
        assert_eq!(s.stringify(), r#"{"members":[],"b":1,"footings":[]}"#);
        let mut n = State::from_value(Value::Null);
        n.insert("a", Value::from(1));
        assert_eq!(n.stringify(), r#"{"a":1}"#);
    }
}
