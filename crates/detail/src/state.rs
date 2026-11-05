//! Typed views of the stored model (the [`State`]'s records: model.js keeps them as plain data and so does the port,
//! so the untrusted-file checks, the undo snapshots and the saved file are the JS's own). The derived modules (build,
//! connections, marks, drawings …) read the state once through these views: members, holes, copes, connections.

use crate::geom::V3;
use crate::js::{f, or, p3_of, s};
use crate::jv::{JObj, JStr, JVal, Jsv};
use crate::store::State;
use serde_json::{Map, Value};

/// A stored hole: `{ face, x, y, d, id }`.
#[derive(Clone, Debug, PartialEq)]
pub struct HoleS {
    pub id: Option<String>,
    pub face: String,
    pub x: f64,
    pub y: f64,
    pub d: f64,
}
/// A stored cope: `{ kind, end, side, length, depth, id }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    pub id: Option<String>,
    pub kind: String,
    pub end: String,
    pub side: String,
    pub length: f64,
    pub depth: f64,
}
impl Cut {
    pub fn from_value<T: Jsv>(v: &T) -> Cut {
        Cut {
            id: v.js_get("id").and_then(Jsv::js_str).map(str::to_string),
            kind: s(v.js_get("kind")).into(),
            end: s(v.js_get("end")).into(),
            side: s(v.js_get("side")).into(),
            length: f(v.js_get("length")),
            depth: f(v.js_get("depth")),
        }
    }
    /// `{ ...c }` in the stored key order.
    pub fn j(&self) -> Value {
        let mut m = Map::new();
        m.insert("kind".into(), Value::from(self.kind.clone()));
        m.insert("end".into(), Value::from(self.end.clone()));
        m.insert("side".into(), Value::from(self.side.clone()));
        m.insert("length".into(), Value::from(self.length));
        m.insert("depth".into(), Value::from(self.depth));
        if let Some(id) = &self.id {
            m.insert("id".into(), Value::from(id.clone()));
        }
        Value::Object(m)
    }
}

/// The text of a string value, else "" (a record's own copy when it has one: a word is never copied).
fn word_of<T: Jsv>(v: Option<&T>) -> JStr {
    match v.filter(|v| v.js_str().is_some()).map(Jsv::js_jval) {
        Some(JVal::Str(w)) => w,
        _ => JStr::S(""),
    }
}
/// A stored member.
#[derive(Clone, Debug, PartialEq)]
pub struct Mem {
    pub id: String,
    pub kind: JStr,
    pub profile: JStr,
    pub material: JStr,
    pub start: V3,
    pub end: V3,
    pub rotation: f64,
    pub position: JStr,
    pub name: JStr,
    pub cuts: Vec<Cut>,
    pub holes: Vec<HoleS>,
    /// `m.phase` as stored (`|| 1` where the JS says so).
    pub phase: f64,
}
impl Mem {
    pub fn from_value<T: Jsv>(v: &T) -> Mem {
        Mem {
            id: s(v.js_get("id")).into(),
            kind: word_of(v.js_get("type")),
            profile: word_of(v.js_get("profile")),
            material: word_of(v.js_get("material")),
            start: p3_of(v.js_get("start")),
            end: p3_of(v.js_get("end")),
            rotation: f(v.js_get("rotation")),
            position: word_of(v.js_get("position")),
            name: word_of(v.js_get("name")),
            cuts: crate::js::arr(v.js_get("cuts")).iter().map(Cut::from_value).collect(),
            holes: crate::js::arr(v.js_get("holes"))
                .iter()
                .map(|h| HoleS {
                    id: h.js_get("id").and_then(Jsv::js_str).map(str::to_string),
                    face: s(h.js_get("face")).into(),
                    x: f(h.js_get("x")),
                    y: f(h.js_get("y")),
                    d: f(h.js_get("d")),
                })
                .collect(),
            phase: f(v.js_get("phase")),
        }
    }
    /// `m[end]` ('start' | anything else = 'end').
    pub fn at(&self, end: &str) -> V3 {
        if end == "start" { self.start } else { self.end }
    }
    /// `m.phase || 1`.
    pub fn phase1(&self) -> f64 {
        or(self.phase, 1.0)
    }
}

/// A stored connection.
#[derive(Clone, Debug)]
pub struct Conn {
    pub id: String,
    pub kind: String,
    pub beam: Option<String>,
    pub end: String,
    pub column: Option<String>,
    pub a: Option<String>,
    pub a_end: String,
    pub b: Option<String>,
    pub brace: Option<String>,
    pub support: Option<String>,
    pub params: Params,
}

/// A connection's params as stored, for lookups by name (S.15: the names the connection kinds' defaults use are not
/// copied, only pointed at; a build used to copy every connection's params map).
#[derive(Clone, Debug, Default)]
pub struct Params(Vec<(crate::jv::JStr, JVal)>);
impl Params {
    /// The entries of a params object (the record's words are static: nothing is copied but other text).
    pub fn of(m: &JObj) -> Params {
        Params(m.0.clone())
    }
    /// The entries of a params object given as JSON.
    pub fn of_map(m: &Map<String, Value>) -> Params {
        Params::of(&JObj::from_map(m))
    }
    /// `p[k]`.
    pub fn get(&self, k: &str) -> Option<&JVal> {
        self.0.iter().find(|(n, _)| n.as_str() == k).map(|(_, v)| v)
    }
}
/// A param name's number in [`param_names`].
#[inline]
pub(crate) fn param_index(k: &str) -> Option<u16> {
    static IX: std::sync::OnceLock<std::collections::HashMap<&'static str, u16, cc_m3d::omap::FxBuild>> = std::sync::OnceLock::new();
    IX.get_or_init(|| param_names().iter().enumerate().filter_map(|(i, n)| Some((*n, u16::try_from(i).ok()?))).collect()).get(k).copied()
}
/// A connection kind's defaults in their order: (name, default, the name's number in [`param_names`]).
pub(crate) fn kind_params(kind: &str) -> &'static [(&'static str, &'static Value, u16)] {
    type Table = Vec<(&'static str, Vec<(&'static str, &'static Value, u16)>)>;
    static T: std::sync::OnceLock<Table> = std::sync::OnceLock::new();
    let t = T.get_or_init(|| {
        ["endplate", "sheartab", "baseplate", "splice", "gusset"]
            .into_iter()
            .map(|kind| {
                let list = crate::connections::defaults_ref(kind)
                    .and_then(Value::as_object)
                    .into_iter()
                    .flatten()
                    .filter_map(|(k, v)| Some((k.as_str(), v, param_index(k)?)))
                    .collect();
                (kind, list)
            })
            .collect()
    });
    t.iter().find(|(k, _)| *k == kind).map_or(&[], |(_, l)| l.as_slice())
}
/// Every param name of the connection kinds' defaults.
pub(crate) fn param_names() -> &'static [&'static str] {
    static NAMES: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| {
        let mut out: Vec<&'static str> = Vec::new();
        for kind in ["endplate", "sheartab", "baseplate", "splice", "gusset"] {
            for k in crate::connections::defaults_ref(kind).and_then(Value::as_object).into_iter().flat_map(|m| m.keys()) {
                if !out.contains(&k.as_str()) {
                    out.push(k.as_str());
                }
            }
        }
        out
    })
}
impl Conn {
    /// A stored connection as JSON (see [`Conn::from_value`]).
    pub fn from_json(v: &Value) -> Conn {
        Conn::from_value(&JVal::from_value(v))
    }
    pub fn from_value(v: &JVal) -> Conn {
        let o = |k: &str| v.get(k).and_then(JVal::as_str).map(str::to_string);
        Conn {
            id: s(v.get("id")).into(),
            kind: s(v.get("type")).into(),
            beam: o("beam"),
            end: s(v.get("end")).into(),
            column: o("column"),
            a: o("a"),
            a_end: s(v.get("aEnd")).into(),
            b: o("b"),
            brace: o("brace"),
            support: o("support"),
            params: v.get("params").and_then(JVal::as_object).map(Params::of).unwrap_or_default(),
        }
    }
    /// A numeric parameter (`p.k`; NaN when absent).
    pub fn p(&self, k: &str) -> f64 {
        f(self.params.get(k))
    }
    /// A text parameter `p.k || d`.
    pub fn ps(&self, k: &str, d: &str) -> String {
        match self.params.get(k) {
            Some(v) if crate::js::truthy(Some(v)) => crate::js::to_js_string(v),
            _ => d.to_string(),
        }
    }
    /// `p.k` as text (`String(p.k)`, "" when absent: what the JS writes into a bolt's grade).
    pub fn pv(&self, k: &str) -> String {
        self.params.get(k).map_or_else(String::new, crate::js::to_js_string)
    }
    /// `isBeamEnd(c)`.
    pub fn is_beam_end(&self) -> bool {
        self.kind == "endplate" || self.kind == "sheartab"
    }
    /// `membersOf(c)`, main first.
    pub fn members_of(&self) -> Vec<Option<&str>> {
        match self.kind.as_str() {
            "splice" => vec![self.a.as_deref(), self.b.as_deref()],
            "gusset" => vec![self.support.as_deref(), self.brace.as_deref()],
            _ if self.is_beam_end() => vec![self.beam.as_deref(), self.column.as_deref()],
            _ => vec![self.column.as_deref()],
        }
    }
    /// `mainOf(c)`.
    pub fn main_of(&self) -> Option<&str> {
        self.members_of().first().copied().flatten()
    }
}

/// `isBeamEnd(c)` of a stored value.
pub fn is_beam_end_v<T: Jsv>(c: &T) -> bool {
    matches!(c.js_get("type").and_then(Jsv::js_str), Some("endplate" | "sheartab"))
}
/// `membersOf(c)` of a stored value (the ids as values; absent = `None`).
pub fn members_of_v<T: Jsv>(c: &T) -> Vec<Option<&T::Item>> {
    let g = |k: &str| c.js_get(k);
    match c.js_get("type").and_then(Jsv::js_str) {
        Some("splice") => vec![g("a"), g("b")],
        Some("gusset") => vec![g("support"), g("brace")],
        _ if is_beam_end_v(c) => vec![g("beam"), g("column")],
        _ => vec![g("column")],
    }
}

/// The members of a state.
pub fn members(state: &State) -> Vec<Mem> {
    state.list("members").iter().map(Mem::from_value).collect()
}
/// The connections of a state.
pub fn conns(state: &State) -> Vec<Conn> {
    state.list("connections").iter().map(Conn::from_value).collect()
}
