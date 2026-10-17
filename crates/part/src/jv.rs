//! The part document is plain JSON, as in the JS: a `serde_json::Value` with insertion-ordered objects (so a file
//! written back keeps the JS key order). These helpers ask a value the questions the JS asks (`typeof`, truthiness,
//! `Number.isFinite`) and write `JSON.stringify` text (numbers as `String(x)`, the same escapes, the `null, 1`
//! indentation of a part file).

use crate::js;
use serde_json::{Map, Value};

pub type Obj = Map<String, Value>;

/// The mark of a non-finite number held in a document: JSON numbers cannot be NaN or ±∞, so such a value is kept as a
/// string starting with this private-use character (`"\u{F8FF}NaN"`, `…Inf`, `…-Inf`). It reads back as the number
/// ([`as_num`], [`truthy`]), `JSON.stringify` writes it as `null` (as the JS does), and the parity harness's output
/// spells it "$NaN" / "$Inf" / "$-Inf" (the kit's canonical form). Spec gap: a string that really begins with U+F8FF
/// and spells one of those is taken for the number.
pub const NONFINITE: char = '\u{F8FF}';

/// A JSON number from an f64 (non-finite numbers become the [`NONFINITE`] mark).
pub fn num(x: f64) -> Value {
    match serde_json::Number::from_f64(x) {
        Some(n) => Value::Number(n),
        None => Value::String(format!(
            "{NONFINITE}{}",
            if x.is_nan() {
                "NaN"
            } else if x > 0.0 {
                "Inf"
            } else {
                "-Inf"
            }
        )),
    }
}
/// The non-finite number a [`NONFINITE`] mark holds.
pub fn nonfinite(v: &Value) -> Option<f64> {
    let s = v.as_str()?.strip_prefix(NONFINITE)?;
    match s {
        "NaN" => Some(f64::NAN),
        "Inf" => Some(f64::INFINITY),
        "-Inf" => Some(f64::NEG_INFINITY),
        _ => None,
    }
}
/// A copy with every [`NONFINITE`] mark spelled the parity kit's way ("$NaN", "$Inf", "$-Inf").
pub fn canon_marks(v: &Value) -> Value {
    match v {
        Value::String(_) => match nonfinite(v) {
            Some(x) if x.is_nan() => Value::String("$NaN".into()),
            Some(x) if x > 0.0 => Value::String("$Inf".into()),
            Some(_) => Value::String("$-Inf".into()),
            None => v.clone(),
        },
        Value::Array(a) => Value::Array(a.iter().map(canon_marks).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), canon_marks(x))).collect()),
        _ => v.clone(),
    }
}
/// A JSON number from a count.
pub fn unum(n: usize) -> Value {
    Value::from(n)
}
/// `[x, y]`.
pub fn pt2(p: [f64; 2]) -> Value {
    Value::Array(vec![num(p[0]), num(p[1])])
}
/// `[x, y, z]`.
pub fn pt3(p: [f64; 3]) -> Value {
    Value::Array(vec![num(p[0]), num(p[1]), num(p[2])])
}
/// An array of numbers.
pub fn nums(xs: &[f64]) -> Value {
    Value::Array(xs.iter().map(|&x| num(x)).collect())
}
/// An array of strings.
pub fn strs<S: AsRef<str>>(xs: &[S]) -> Value {
    Value::Array(xs.iter().map(|x| Value::String(x.as_ref().to_string())).collect())
}

/// `typeof v === 'number'` → the number.
pub fn as_num(v: Option<&Value>) -> Option<f64> {
    v.and_then(|x| x.as_f64().or_else(|| nonfinite(x)))
}
/// `Number.isFinite(v)` → the number.
pub fn finite(v: Option<&Value>) -> Option<f64> {
    as_num(v).filter(|x| x.is_finite())
}
/// `Number.isInteger(v)` → the number.
pub fn int(v: Option<&Value>) -> Option<f64> {
    as_num(v).filter(|x| js::is_integer(*x))
}
/// `typeof v === 'string'`.
pub fn as_str(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str)
}
/// `v === true`.
pub fn is_true(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Bool(true)))
}
/// JS truthiness (absent = falsy).
pub fn truthy(v: Option<&Value>) -> bool {
    if let Some(x) = v.and_then(nonfinite) {
        return !x.is_nan();
    }
    cc_cad::js::truthy(v)
}
/// `v === undefined || v === null`.
pub fn nullish(v: Option<&Value>) -> bool {
    matches!(v, None | Some(Value::Null))
}
/// `v && typeof v === 'object'` (arrays included, as in JS).
pub fn is_obj_like(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Object(_) | Value::Array(_)))
}
/// A plain object (not an array).
pub fn as_obj(v: Option<&Value>) -> Option<&Obj> {
    v.and_then(Value::as_object)
}
/// `Array.isArray(v)` → the items.
pub fn as_arr(v: Option<&Value>) -> Option<&Vec<Value>> {
    v.and_then(Value::as_array)
}
/// The items of an array, or none.
pub fn arr(v: Option<&Value>) -> &[Value] {
    as_arr(v).map_or(&[], Vec::as_slice)
}
/// `a === b` for JSON scalars (numbers by value, ±0 equal); objects and arrays never equal (identity).
pub fn strict_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Value::Null), Some(Value::Null)) => true,
        (Some(Value::Bool(x)), Some(Value::Bool(y))) => x == y,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(Value::String(x)), Some(Value::String(y))) => x == y,
        _ => false,
    }
}
/// `v === s` for a string.
pub fn is_str(v: Option<&Value>, s: &str) -> bool {
    as_str(v) == Some(s)
}
/// `obj[k] = v` (keeps the key's place when it exists, appends it otherwise — JS property order).
pub fn set(o: &mut Value, k: &str, v: Value) {
    if let Some(m) = o.as_object_mut() {
        m.insert(k.to_string(), v);
    }
}
/// `delete obj[k]` (the other keys keep their order).
pub fn del(o: &mut Value, k: &str) {
    if let Some(m) = o.as_object_mut() {
        m.shift_remove(k);
    }
}
/// A string from a value's field.
pub fn s<'a>(o: &'a Value, k: &str) -> &'a str {
    o.get(k).and_then(Value::as_str).unwrap_or("")
}
/// A number from a value's field (NaN when not a number: `undefined` in arithmetic).
pub fn n(o: &Value, k: &str) -> f64 {
    as_num(o.get(k)).unwrap_or(f64::NAN)
}
/// `{...}` from pairs.
pub fn obj(pairs: Vec<(&str, Value)>) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    Value::Object(m)
}

// ---------------------------------------------------------------- JSON.stringify

fn push_str(out: &mut String, t: &str) {
    out.push('"');
    for c in t.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn push_num(out: &mut String, x: f64) {
    if x.is_finite() {
        cc_cad::js::push_num(out, x);
    } else {
        out.push_str("null");
    }
}

fn write(out: &mut String, v: &Value, indent: usize, level: usize) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => push_num(out, n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => {
            if nonfinite(v).is_some() {
                out.push_str("null");
            } else {
                push_str(out, s);
            }
        }
        Value::Array(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, level.saturating_add(1));
                write(out, x, indent, level.saturating_add(1));
            }
            newline(out, indent, level);
            out.push(']');
        }
        Value::Object(o) => {
            if o.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, x)) in o.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, level.saturating_add(1));
                push_str(out, k);
                out.push(':');
                if indent > 0 {
                    out.push(' ');
                }
                write(out, x, indent, level.saturating_add(1));
            }
            newline(out, indent, level);
            out.push('}');
        }
    }
}

fn newline(out: &mut String, indent: usize, level: usize) {
    if indent == 0 {
        return;
    }
    out.push('\n');
    for _ in 0..indent.saturating_mul(level) {
        out.push(' ');
    }
}

/// `JSON.stringify(v)`.
pub fn stringify(v: &Value) -> String {
    let mut out = String::new();
    write(&mut out, v, 0, 0);
    out
}
/// `JSON.stringify(v, null, indent)`.
pub fn stringify_pretty(v: &Value, indent: usize) -> String {
    let mut out = String::new();
    write(&mut out, v, indent, 0);
    out
}

/// `JSON.parse(text)`; nesting deeper than serde_json's 128 levels is refused (a JS engine would go further).
pub fn parse(text: &str) -> Option<Value> {
    serde_json::from_str(text).ok()
}

/// `JSON.parse(JSON.stringify(v))`: a deep copy with the JSON round trip's losses (non-finite numbers → null).
pub fn json_copy(v: &Value) -> Value {
    v.clone()
}
