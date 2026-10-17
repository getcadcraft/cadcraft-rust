//! point-edit.js — edit one survey point at a time (0.4 V2): the check (untrusted typed text or numbers); the edit
//! itself (one undo step that also moves breakline vertices on the old spot) is `Project::edit_point`.

use crate::js::{J, Obj, slice16, str_to_number, strip_controls, trim};
use crate::points::Pt;
use crate::tr;
use crate::units::is_num_str;
use serde_json::{Map, Value};

const LIMIT_XY: f64 = 1e9;
const LIMIT_Z: f64 = 1e6;

fn number_of(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().filter(|x| x.is_finite()).unwrap_or(f64::NAN),
        _ => {
            let s: String = trim(&crate::js::string_or_empty(Some(v))).chars().filter(|&c| c != ',').collect();
            // /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/
            if is_num_str(&s) && s == trim(&s) { str_to_number(&s) } else { f64::NAN }
        }
    }
}

/// The check's result.
#[derive(Clone, Debug)]
pub struct Check {
    pub ok: bool,
    pub index: Option<usize>,
    pub old: Option<Pt>,
    pub point: Option<Pt>,
    pub errors: Vec<(String, String)>,
    pub message: String,
}
impl J for Check {
    fn j(&self) -> Value {
        let mut errs = Map::new();
        for (k, v) in &self.errors {
            errs.insert(k.clone(), Value::String(v.clone()));
        }
        let mut o = Obj::new().set("ok", self.ok);
        if self.ok {
            o = o.opt("index", self.index).opt("old", self.old.as_ref()).opt("point", self.point.as_ref());
        }
        o.set("errors", Value::Object(errs)).set("message", &self.message).done()
    }
}

/// `checkPointEdit(points, n, patch)`.
pub fn check_point_edit(points: &[Pt], n: &str, patch: &Map<String, Value>) -> Check {
    let Some(i) = points.iter().position(|p| p.n == n) else {
        let m = tr!("pt.edit.gone", n = n);
        return Check { ok: false, index: None, old: None, point: None, errors: vec![("n".into(), m.clone())], message: m };
    };
    let Some(old) = points.get(i).cloned() else {
        return Check { ok: false, index: None, old: None, point: None, errors: Vec::new(), message: String::new() };
    };
    let mut next = old.clone();
    let mut errors: Vec<(String, String)> = Vec::new();
    if let Some(v) = patch.get("n") {
        let v = trim(&strip_controls(&crate::js::string_or_empty(Some(v)), "")).to_string();
        if v.is_empty() {
            errors.push(("n".into(), tr!("pt.edit.n.empty")));
        } else if v.encode_utf16().count() > 40 {
            errors.push(("n".into(), tr!("pt.edit.n.long")));
        } else if v != old.n && points.iter().any(|p| p.n == v) {
            errors.push(("n".into(), tr!("pt.edit.n.taken", n = v.as_str())));
        } else {
            next.n = v;
        }
    }
    for k in ["x", "y"] {
        let Some(raw) = patch.get(k) else { continue };
        let v = number_of(raw);
        if !v.is_finite() || v.abs() >= LIMIT_XY {
            errors.push((k.into(), tr!("pt.edit.coord")));
        } else if k == "x" {
            next.x = v;
        } else {
            next.y = v;
        }
    }
    if let Some(raw) = patch.get("z") {
        let empty = match raw {
            Value::Null => true,
            Value::String(s) => trim(s).is_empty(),
            _ => false,
        };
        if empty {
            next.z = None;
        } else {
            let v = number_of(raw);
            if !v.is_finite() || v.abs() >= LIMIT_Z {
                errors.push(("z".into(), tr!("pt.edit.z")));
            } else {
                next.z = Some(v);
            }
        }
    }
    if let Some(raw) = patch.get("d") {
        let v = trim(&strip_controls(&crate::js::string_or_empty(Some(raw)), " ")).to_string();
        if v.encode_utf16().count() > 200 {
            errors.push(("d".into(), tr!("pt.edit.d.long")));
        } else {
            next.d = v;
        }
    }
    if !errors.is_empty() {
        let message = errors.first().map(|e| e.1.clone()).unwrap_or_default();
        return Check { ok: false, index: None, old: None, point: None, errors, message };
    }
    if next.n == old.n && next.x == old.x && next.y == old.y && next.z == old.z && next.d == old.d {
        return Check { ok: false, index: None, old: None, point: None, errors: Vec::new(), message: String::new() };
    }
    let _ = slice16;
    Check { ok: true, index: Some(i), old: Some(old), point: Some(next), errors: Vec::new(), message: String::new() }
}
