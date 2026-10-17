//! Typed views of the stored model (`state`, a JSON value: model.js keeps it as plain data and so does the port, so
//! the untrusted-file checks, the undo snapshots and the saved file are the JS's own). The derived modules (build,
//! connections, marks, drawings …) read the state once through these views: members, holes, copes, connections.

use crate::geom::V3;
use crate::js::{f, or, p3_of, s};
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
    pub fn from_value(v: &Value) -> Cut {
        Cut {
            id: v.get("id").and_then(Value::as_str).map(str::to_string),
            kind: s(v.get("kind")).into(),
            end: s(v.get("end")).into(),
            side: s(v.get("side")).into(),
            length: f(v.get("length")),
            depth: f(v.get("depth")),
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

/// A stored member.
#[derive(Clone, Debug, PartialEq)]
pub struct Mem {
    pub id: String,
    pub kind: String,
    pub profile: String,
    pub material: String,
    pub start: V3,
    pub end: V3,
    pub rotation: f64,
    pub position: String,
    pub name: String,
    pub cuts: Vec<Cut>,
    pub holes: Vec<HoleS>,
    /// `m.phase` as stored (`|| 1` where the JS says so).
    pub phase: f64,
}
impl Mem {
    pub fn from_value(v: &Value) -> Mem {
        Mem {
            id: s(v.get("id")).into(),
            kind: s(v.get("type")).into(),
            profile: s(v.get("profile")).into(),
            material: s(v.get("material")).into(),
            start: p3_of(v.get("start")),
            end: p3_of(v.get("end")),
            rotation: f(v.get("rotation")),
            position: s(v.get("position")).into(),
            name: s(v.get("name")).into(),
            cuts: crate::js::arr(v.get("cuts")).iter().map(Cut::from_value).collect(),
            holes: crate::js::arr(v.get("holes"))
                .iter()
                .map(|h| HoleS {
                    id: h.get("id").and_then(Value::as_str).map(str::to_string),
                    face: s(h.get("face")).into(),
                    x: f(h.get("x")),
                    y: f(h.get("y")),
                    d: f(h.get("d")),
                })
                .collect(),
            phase: f(v.get("phase")),
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
    pub params: Map<String, Value>,
}
impl Conn {
    pub fn from_value(v: &Value) -> Conn {
        let o = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
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
            params: v.get("params").and_then(Value::as_object).cloned().unwrap_or_default(),
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
pub fn is_beam_end_v(c: &Value) -> bool {
    matches!(c.get("type").and_then(Value::as_str), Some("endplate" | "sheartab"))
}
/// `membersOf(c)` of a stored value (the ids as values; absent = `None`).
pub fn members_of_v(c: &Value) -> Vec<Option<&Value>> {
    let g = |k: &str| c.get(k);
    match c.get("type").and_then(Value::as_str) {
        Some("splice") => vec![g("a"), g("b")],
        Some("gusset") => vec![g("support"), g("brace")],
        _ if is_beam_end_v(c) => vec![g("beam"), g("column")],
        _ => vec![g("column")],
    }
}

/// The members of a state.
pub fn members(state: &Value) -> Vec<Mem> {
    crate::js::arr(state.get("members")).iter().map(Mem::from_value).collect()
}
/// The connections of a state.
pub fn conns(state: &Value) -> Vec<Conn> {
    crate::js::arr(state.get("connections")).iter().map(Conn::from_value).collect()
}
