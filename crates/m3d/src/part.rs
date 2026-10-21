//! One definition's entry in an undo snapshot. The JS keeps `JSON.stringify(defToJSON(d))`; the port keeps the same
//! information as a structural copy of the definition and makes the text only when it is asked for (a whole-model
//! snapshot string, a preview's base). Everything the JS does with the string is done with the same meaning:
//!
//! - equality is JSON-text equality: numbers compare as their text does (−0 = 0; NaN, ∞ and −∞ are all `null`),
//!   and fields the text leaves out (an empty name, a `false` flag) compare as left out;
//! - restoring is `defOf(JSON.parse(text))`: the copy goes through the same checks as a file, with −0 read back as 0
//!   and a non-finite number read back as `null` (which those checks refuse, as the JS does).

use std::cell::OnceCell;
use std::rc::Rc;

use crate::def::{Def, DefJson, Edge, Face, Inst, Rule, Spline};
use crate::error::{Error, Res};
use crate::file::{Fail, def_of, def_string, def_value_text};

/// A snapshot entry: a structural copy, or (a part made from a preview's JSON) the text alone.
#[derive(Debug)]
pub struct Part {
    def: Option<DefJson>,
    text: OnceCell<Rc<str>>,
}

pub type PartRef = Rc<Part>;

impl Part {
    /// The entry for a definition as it is now.
    pub fn of(d: &Def) -> PartRef {
        Rc::new(Part { def: Some(DefJson::of(d)), text: OnceCell::new() })
    }
    /// An entry that is only text.
    pub fn text_only(s: Rc<str>) -> PartRef {
        let text = OnceCell::new();
        let _ = text.set(s);
        Rc::new(Part { def: None, text })
    }
    /// `JSON.stringify(defToJSON(d))`.
    pub fn text(&self) -> Rc<str> {
        self.text.get_or_init(|| self.def.as_ref().map_or_else(|| Rc::from(""), |d| Rc::from(def_string(d)))).clone()
    }
    /// `defOf(JSON.parse(text))`.
    pub fn restore(&self) -> Res<Def> {
        match &self.def {
            Some(d) => def_of(&def_value_text(d), Fail::Plain),
            None => {
                let j: serde_json::Value = serde_json::from_str(&self.text()).map_err(|_| Error::Type)?;
                def_of(&j, Fail::Plain)
            }
        }
    }
}

/// Do two entries have the same JSON text?
pub fn same(a: &PartRef, b: &PartRef) -> bool {
    if Rc::ptr_eq(a, b) {
        return true;
    }
    match (&a.def, &b.def) {
        (Some(x), Some(y)) => def_json_eq(x, y),
        _ => a.text() == b.text(),
    }
}

/// Two numbers write the same JSON text.
#[inline]
fn jeq(x: f64, y: f64) -> bool {
    x == y || (!x.is_finite() && !y.is_finite())
}
fn jeq_all(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| jeq(*x, *y))
}
/// An optional string the JSON leaves out when empty.
fn opt_text(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|x| !x.is_empty())
}
fn edge_eq(a: &Edge, b: &Edge) -> bool {
    jeq(a.a, b.a) && jeq(a.b, b.b) && a.soft == b.soft && a.curve == b.curve && a.hidden == b.hidden
}
fn face_eq(a: &Face, b: &Face) -> bool {
    a.hidden == b.hidden && a.color == b.color && a.loops.len() == b.loops.len() && a.loops.iter().zip(&b.loops).all(|(x, y)| jeq_all(x, y))
}
fn rule_eq(a: &Rule, b: &Rule) -> bool {
    let n = |x: Option<f64>, y: Option<f64>| match (x, y) {
        (None, None) => true,
        (Some(p), Some(q)) => jeq(p, q),
        _ => false,
    };
    a.x == b.x && a.y == b.y && a.z == b.z && n(a.sx, b.sx) && n(a.sy, b.sy) && n(a.sz, b.sz)
}
fn inst_eq(a: &Inst, b: &Inst) -> bool {
    a.def == b.def
        && jeq_all(&a.m, &b.m)
        && opt_text(&a.name) == opt_text(&b.name)
        && opt_text(&a.color) == opt_text(&b.color)
        && a.hidden == b.hidden
        && opt_text(&a.tag) == opt_text(&b.tag)
        && match (&a.rule, &b.rule) {
            (None, None) => true,
            (Some(x), Some(y)) => rule_eq(x, y),
            _ => false,
        }
}
fn spline_eq(a: &Spline, b: &Spline) -> bool {
    a.closed == b.closed && jeq(a.segs, b.segs) && a.pts.len() == b.pts.len() && a.pts.iter().zip(&b.pts).all(|(p, q)| jeq_all(p, q))
}

/// `JSON.stringify(defToJSON(a)) === JSON.stringify(defToJSON(b))`, without the text. The checks run cheapest first
/// (every count, then the instances and splines, then the geometry): an undo of an edit that only added or moved
/// instances of a big definition finds the difference without walking its faces.
pub fn def_json_eq(a: &DefJson, b: &DefJson) -> bool {
    a.id == b.id
        && a.kind == b.kind
        && a.name == b.name
        && jeq(a.next, b.next)
        && a.starter == b.starter
        && a.verts.len() == b.verts.len()
        && a.edges.len() == b.edges.len()
        && a.faces.len() == b.faces.len()
        && a.insts.len() == b.insts.len()
        && a.splines.len() == b.splines.len()
        && match (&a.dyn_, &b.dyn_) {
            (None, None) => true,
            (Some(x), Some(y)) => x.src == y.src && jeq_all(&x.size, &y.size),
            _ => false,
        }
        && a.insts.iter().zip(&b.insts).all(|((i, x), (j, y))| jeq(*i, *j) && inst_eq(x, y))
        && a.splines.iter().zip(&b.splines).all(|((i, x), (j, y))| i == j && spline_eq(x, y))
        && a.verts.iter().zip(&b.verts).all(|((i, p), (j, q))| jeq(*i, *j) && jeq_all(p, q))
        && a.edges.iter().zip(&b.edges).all(|((i, x), (j, y))| jeq(*i, *j) && edge_eq(x, y))
        && a.faces.iter().zip(&b.faces).all(|((i, x), (j, y))| jeq(*i, *j) && face_eq(x, y))
}
