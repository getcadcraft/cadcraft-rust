//! ifc-find.js — Find by class, name or any property, and named search sets (0.4 B14).
//!
//! Text compares ignore case and surrounding spaces; numbers compare as numbers when both sides are numbers (a value
//! typed "2.5" against 2.5), else as text. A rule on a field the index does not have matches nothing (except hasnot).

use std::collections::HashMap;

use cc_io_ifc::model::id_key;
use serde_json::{Map, Value as J};

use crate::js::{OMap, PKey, PV, len16, locale_cmp, lower, slice16, str_num, trim};
use crate::props::Index;

pub const OPS: [&str; 11] = ["is", "isnot", "contains", "notcontains", "starts", "gt", "lt", "gte", "lte", "has", "hasnot"];
const NEEDS_VALUE: [&str; 9] = ["is", "isnot", "contains", "notcontains", "starts", "gt", "lt", "gte", "lte"];
const MAX_SETS: usize = 200;
const MAX_RULES: usize = 20;
const MAX_NAME: usize = 80;
const MAX_FIELD: usize = 300;
const MAX_VALUE: usize = 300;

/// A rule's wanted value as the JS holds it (from storage or the page: text, a number, nothing, or anything else).
#[derive(Clone, Debug, PartialEq)]
pub enum Want {
    Undef,
    Num(f64),
    Str(String),
    /// any other JSON value: `String(v)` of it
    Other(String),
}

impl Want {
    pub fn from_json(v: Option<&J>) -> Want {
        match v {
            None | Some(J::Null) => Want::Undef,
            Some(J::Number(n)) => Want::Num(n.as_f64().unwrap_or(f64::NAN)),
            Some(J::String(s)) => Want::Str(s.clone()),
            Some(other) => Want::Other(cc_cad::js::to_js_string(other)),
        }
    }
    /// `String(v ?? '')`
    fn text(&self) -> String {
        match self {
            Want::Undef => String::new(),
            Want::Num(n) => crate::js::num_str(*n),
            Want::Str(s) | Want::Other(s) => s.clone(),
        }
    }
}

/// One rule.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    /// None = a field that is not text (it names no field of the index)
    pub field: Option<String>,
    pub field_truthy: bool,
    pub op: String,
    pub value: Want,
}

impl Rule {
    pub fn from_json(r: &J) -> Option<Rule> {
        if !cc_cad::js::truthy(Some(r)) || !r.is_object() {
            return None;
        }
        let f = r.get("field");
        Some(Rule {
            field: f.and_then(J::as_str).map(str::to_string),
            field_truthy: cc_cad::js::truthy(f),
            op: r.get("op").and_then(J::as_str).unwrap_or("").to_string(),
            value: Want::from_json(r.get("value")),
        })
    }
}

/// `norm(v)`: `String(v ?? '').trim().toLowerCase()`
fn norm(s: &str) -> String {
    lower(trim(s))
}

/// `asNum(v)` for a string: trimmed, the first ',' read as '.', a finite Number.
fn as_num_str(s: &str) -> Option<f64> {
    let t = trim(s).replacen(',', ".", 1);
    if t.is_empty() {
        return None;
    }
    let n = str_num(&t);
    n.is_finite().then_some(n)
}

fn as_num_pv(v: &PV) -> Option<f64> {
    match v {
        PV::Num(n) => Some(*n),
        PV::Str(s) => as_num_str(s),
    }
}

fn as_num_want(w: &Want) -> Option<f64> {
    match w {
        Want::Num(n) => Some(*n),
        other => as_num_str(&other.text()),
    }
}

/// `test(v, op, want)`.
pub fn test(v: Option<&PV>, op: &str, want: &Want) -> bool {
    match op {
        "has" => return v.is_some_and(|x| !x.is_empty_str()),
        "hasnot" => return v.is_none_or(PV::is_empty_str),
        _ => {}
    }
    let Some(v) = v else { return op == "isnot" || op == "notcontains" };
    let (a, b) = (as_num_pv(v), as_num_want(want));
    let both = a.zip(b);
    match op {
        "is" => match both {
            Some((a, b)) => a == b,
            None => norm(&v.text()) == norm(&want.text()),
        },
        "isnot" => match both {
            Some((a, b)) => a != b,
            None => norm(&v.text()) != norm(&want.text()),
        },
        "contains" => norm(&v.text()).contains(&norm(&want.text())),
        "notcontains" => !norm(&v.text()).contains(&norm(&want.text())),
        "starts" => norm(&v.text()).starts_with(&norm(&want.text())),
        "gt" => both.is_some_and(|(a, b)| a > b),
        "lt" => both.is_some_and(|(a, b)| a < b),
        "gte" => both.is_some_and(|(a, b)| a >= b),
        "lte" => both.is_some_and(|(a, b)| a <= b),
        _ => false,
    }
}

/// `find(ix, { rules, mode })` → keys.
pub fn find(ix: &Index, rules: &[Rule], any: bool) -> Vec<f64> {
    let rs: Vec<&Rule> = rules
        .iter()
        .filter(|r| {
            r.field_truthy && OPS.contains(&r.op.as_str()) && (!NEEDS_VALUE.contains(&r.op.as_str()) || !trim(&r.value.text()).is_empty())
        })
        .collect();
    if rs.is_empty() {
        return Vec::new();
    }
    let empty = crate::props::Vals::new();
    let vals: Vec<&crate::props::Vals> = rs.iter().map(|r| r.field.as_deref().map_or(&empty, |f| ix.values(f))).collect();
    let mut out = Vec::new();
    for &id in &ix.ids {
        let k = id_key(id);
        let mut hits = rs.iter().zip(vals.iter()).map(|(r, m)| test(m.get(&k), &r.op, &r.value));
        let hit = if any { hits.any(|h| h) } else { hits.all(|h| h) };
        if hit {
            out.push(ix.key_base + id);
        }
    }
    out
}

/// A search set as cleanSets returns it.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchSet {
    pub name: String,
    pub any: bool,
    pub rules: Vec<(String, String, String)>,
}

impl SearchSet {
    pub fn json(&self) -> J {
        let mut o = Map::new();
        o.insert("name".into(), J::String(self.name.clone()));
        o.insert("mode".into(), J::String(if self.any { "any" } else { "all" }.into()));
        o.insert(
            "rules".into(),
            J::Array(
                self.rules
                    .iter()
                    .map(|(f, op, v)| {
                        let mut r = Map::new();
                        r.insert("field".into(), J::String(f.clone()));
                        r.insert("op".into(), J::String(op.clone()));
                        r.insert("value".into(), J::String(v.clone()));
                        J::Object(r)
                    })
                    .collect(),
            ),
        );
        J::Object(o)
    }
}

/// `cleanSets(raw)`: saved search sets (untrusted text or a parsed value) → the well-formed ones, cut to size.
pub fn clean_sets(raw_text: Option<&str>, parsed: Option<&J>) -> Vec<SearchSet> {
    let owned;
    let arr = match (raw_text, parsed) {
        (Some(t), _) => match serde_json::from_str::<J>(t) {
            Ok(v) => {
                owned = v;
                &owned
            }
            Err(_) => return Vec::new(),
        },
        (None, Some(p)) => p,
        (None, None) => return Vec::new(),
    };
    let Some(arr) = arr.as_array() else { return Vec::new() };
    let mut out = Vec::new();
    for s in arr.iter().take(MAX_SETS) {
        let Some(o) = s.as_object() else { continue };
        let Some(name) = o.get("name").and_then(J::as_str) else { continue };
        if trim(name).is_empty() {
            continue;
        }
        let Some(rules) = o.get("rules").and_then(J::as_array) else { continue };
        let rules: Vec<(String, String, String)> = rules
            .iter()
            .take(MAX_RULES)
            .filter_map(|r| {
                if !cc_cad::js::truthy(Some(r)) {
                    return None;
                }
                let f = r.get("field").and_then(J::as_str)?;
                if len16(f) > MAX_FIELD {
                    return None;
                }
                let op = r.get("op").and_then(J::as_str)?;
                if !OPS.contains(&op) {
                    return None;
                }
                let v = match r.get("value") {
                    None | Some(J::Null) => String::new(),
                    Some(J::String(s)) => slice16(s, MAX_VALUE),
                    Some(J::Number(n)) => slice16(&crate::js::num_str(n.as_f64().unwrap_or(f64::NAN)), MAX_VALUE),
                    Some(_) => return None,
                };
                Some((f.to_string(), op.to_string(), v))
            })
            .collect();
        if rules.is_empty() {
            continue;
        }
        out.push(SearchSet { name: slice16(trim(name), MAX_NAME), any: o.get("mode").and_then(J::as_str) == Some("any"), rules });
    }
    out
}

/// `suggestions(ix, field, limit)`: distinct values, most common first.
pub fn suggestions(ix: &Index, field: &str, limit: usize) -> Vec<String> {
    let mut counts: OMap<PKey, (PV, usize)> = OMap::new();
    for v in ix.values(field).vals.iter() {
        if let Some(e) = counts.entry(v.key(), || (v.clone(), 0)) {
            e.1 = e.1.saturating_add(1);
        }
    }
    let mut list: Vec<&(PV, usize)> = counts.vals.iter().collect();
    // String(a[0]) is computed per comparison in the JS; cache it once (same order)
    let texts: HashMap<*const (PV, usize), String> = list.iter().map(|e| (std::ptr::from_ref(*e), e.0.text())).collect();
    let tx = |e: &(PV, usize)| texts.get(&std::ptr::from_ref(e)).cloned().unwrap_or_default();
    list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| locale_cmp(&tx(a), &tx(b))));
    list.into_iter().take(limit).map(|e| e.0.text()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        assert!(test(Some(&PV::Num(6000.0)), "is", &Want::Str("6000".into())));
        assert!(test(Some(&PV::s("IfcWall")), "is", &Want::Str(" ifcwall ".into())));
        assert!(test(None, "hasnot", &Want::Undef));
        assert!(test(Some(&PV::s("2,5")), "gt", &Want::Num(2.0)));
        assert!(!test(None, "is", &Want::Str("x".into())));
        assert!(test(None, "notcontains", &Want::Str("x".into())));
    }

    #[test]
    fn sets() {
        let s = clean_sets(
            Some(
                r#"[{"name":" External walls ","mode":"all","rules":[{"field":"p:A.B","op":"is","value":"Yes"}]},{"name":"Bad op","rules":[{"field":"cls","op":"eval"}]},42,null]"#,
            ),
            None,
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "External walls");
        assert!(clean_sets(Some("{not json"), None).is_empty());
    }
}
