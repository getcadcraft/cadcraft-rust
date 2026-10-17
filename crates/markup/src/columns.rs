//! columns.js — custom columns on markups: `cleanColumns`, `cleanFields`, `fieldOk`, `newColumnKey`, `compareBy`.

use crate::js::{is_ctl_keep_ws, slice16, str_or_empty, trim};
use indexmap::IndexMap;
use serde_json::Value;

/// One custom column.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub choices: Vec<String>,
}

pub const TYPES_OF_COLUMN: [&str; 4] = ["text", "choice", "number", "date"];

/// `plain(v, n)`: control characters (not tab / LF / CR) dropped, trimmed, ≤ n UTF-16 units.
pub fn plain(v: Option<&Value>, n: usize) -> String {
    let s: String = str_or_empty(v).chars().filter(|c| !is_ctl_keep_ws(*c)).collect();
    slice16(trim(&s), n)
}

/// `/^c\d{1,4}$/`
pub fn is_col_key(s: &str) -> bool {
    let b = s.as_bytes();
    b.first() == Some(&b'c') && (2..=5).contains(&b.len()) && b.iter().skip(1).all(u8::is_ascii_digit)
}

/// `cleanColumns(v)`.
pub fn clean_columns(v: Option<&Value>) -> Vec<Column> {
    let Some(Value::Array(list)) = v else { return Vec::new() };
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for c in list.iter().take(30) {
        let Some(o) = c.as_object() else { continue };
        let key = match o.get("key") {
            Some(Value::String(k)) if is_col_key(k) => Some(k.clone()),
            // `/re/.test(x)` stringifies non-strings: a number never looks like "c1"
            _ => None,
        };
        let name = plain(o.get("name"), 60);
        let Some(key) = key else { continue };
        if name.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.push(key.clone());
        let kind = match o.get("type") {
            Some(Value::String(t)) if TYPES_OF_COLUMN.contains(&t.as_str()) => t.clone(),
            _ => "text".to_string(),
        };
        let mut choices: Vec<String> = Vec::new();
        if kind == "choice"
            && let Some(Value::Array(ch)) = o.get("choices")
        {
            for x in ch {
                let p = plain(Some(x), 60);
                if !p.is_empty() && !choices.contains(&p) {
                    choices.push(p);
                }
            }
            choices.truncate(50);
        }
        out.push(Column { key, name, kind, choices });
    }
    out
}

/// `cleanFields(v)` → { key: value } (≤ 50 keys looked at, values ≤ 500 characters).
pub fn clean_fields(v: Option<&Value>) -> IndexMap<String, String> {
    let mut out = IndexMap::new();
    let Some(Value::Object(o)) = v else { return out };
    // Object.entries: integer-like keys first (ascending), then the rest in insertion order — none of ours is
    // integer-like ("c1"), so the order of the kept ones is the insertion order
    for (k, val) in js_entries(o).into_iter().take(50) {
        if is_col_key(k) {
            let s = plain(Some(val), 500);
            if !s.is_empty() {
                out.insert(k.clone(), s);
            }
        }
    }
    out
}

/// `Object.entries(o)` order: array-index keys ascending first, then the other keys in insertion order.
pub fn js_entries(o: &serde_json::Map<String, Value>) -> Vec<(&String, &Value)> {
    let is_index = |k: &str| -> Option<u32> {
        if k == "0" {
            return Some(0);
        }
        if k.starts_with('0') || k.is_empty() || !k.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        k.parse::<u32>().ok().filter(|n| *n < u32::MAX)
    };
    let mut idx: Vec<(u32, &String, &Value)> = Vec::new();
    let mut rest: Vec<(&String, &Value)> = Vec::new();
    for (k, v) in o {
        match is_index(k) {
            Some(n) => idx.push((n, k, v)),
            None => rest.push((k, v)),
        }
    }
    idx.sort_by_key(|(n, _, _)| *n);
    idx.into_iter().map(|(_, k, v)| (k, v)).chain(rest).collect()
}

/// `fieldOk(column, value)` → '' or the language key of why the value does not fit.
pub fn field_ok(col: &Column, value: &str) -> &'static str {
    let v = trim(value);
    if v.is_empty() {
        return "";
    }
    if col.kind == "number" && !crate::js::str_to_number(&v.replace(',', "")).is_finite() {
        return "col.err.number";
    }
    if col.kind == "date" {
        let b = v.as_bytes();
        let ok = b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() });
        if !ok {
            return "col.err.date";
        }
    }
    if col.kind == "choice" && !col.choices.iter().any(|c| c == v) {
        return "col.err.choice";
    }
    ""
}

/// `newColumnKey(columns)`.
pub fn new_column_key(columns: &[Column]) -> String {
    let mut n = 1u32;
    while columns.iter().any(|c| c.key == format!("c{n}")) {
        n = n.saturating_add(1);
    }
    format!("c{n}")
}

/// `columnsJson(cols)` for writing (JSON.stringify of the clean columns).
pub fn columns_to_value(cols: &[Column]) -> Value {
    Value::Array(
        cols.iter()
            .map(|c| {
                let mut m = serde_json::Map::new();
                m.insert("key".into(), Value::String(c.key.clone()));
                m.insert("name".into(), Value::String(c.name.clone()));
                m.insert("type".into(), Value::String(c.kind.clone()));
                m.insert("choices".into(), Value::Array(c.choices.iter().cloned().map(Value::String).collect()));
                Value::Object(m)
            })
            .collect(),
    )
}

/// `compareBy(col)(a, b)` on two markups' field values: empty values last, numbers as numbers, choices in their
/// order, text by [`crate::collate::locale_compare_numeric`].
pub fn compare_by(col: &Column, a: &crate::model::Markup, b: &crate::model::Markup) -> f64 {
    let get = |m: &crate::model::Markup| trim(m.fields.get(&col.key).map_or("", String::as_str)).to_string();
    let (x, y) = (get(a), get(b));
    if x.is_empty() || y.is_empty() {
        return f64::from(u8::from(x.is_empty())) - f64::from(u8::from(y.is_empty()));
    }
    match col.kind.as_str() {
        "number" => crate::js::str_to_number(&x.replace(',', "")) - crate::js::str_to_number(&y.replace(',', "")),
        "choice" => {
            let ix = |v: &str| col.choices.iter().position(|c| c == v).map_or(-1.0, |i| i as f64);
            ix(&x) - ix(&y)
        }
        _ => crate::collate::locale_compare_numeric(&x, &y),
    }
}
