//! JavaScript semantics over the dynamic value ([`V`], the same type cc-dwg's reader builds): property reads that may
//! be `undefined`, truthiness, `+v` / `Number(v)`, `String(v)`, `parseFloat`, `parseInt`, `Number.isFinite`, and the
//! conversion of a finished value to `serde_json` (what `doc.load` takes; NaN / ±Infinity as the parity kit's
//! "$NaN" / "$Inf" / "$-Inf", as `cc_cad::entity::num` writes them).
//!
//! Keys that start with [`HIDDEN`] are private marks (dwg-import.js's WeakSet of multileader / picture objects): they
//! never reach the JSON.

use cc_cad::js;
use cc_io_dwg::value::{Obj, V};
use serde_json::Value;

/// The first character of a private key (left out of the JSON).
pub const HIDDEN: char = '\u{1}';

/// `undefined` for an absent key.
pub type Ov<'a> = Option<&'a V>;

/// `o[k]` (None = undefined).
pub fn get<'a>(o: Ov<'a>, k: &str) -> Ov<'a> {
    match o {
        Some(V::Obj(m)) => m.get(k),
        _ => None,
    }
}
/// `o?.[i]` on an array.
pub fn at(o: Ov<'_>, i: usize) -> Ov<'_> {
    match o {
        Some(V::Arr(a)) => a.get(i),
        _ => None,
    }
}
/// The array at `o` (`o || []`), empty when it is not an array.
pub fn arr(o: Ov<'_>) -> &[V] {
    match o {
        Some(V::Arr(a)) => a.as_slice(),
        _ => &[],
    }
}
/// The object at `o`, if it is one.
pub fn obj(o: Ov<'_>) -> Option<&Obj> {
    match o {
        Some(V::Obj(m)) => Some(m),
        _ => None,
    }
}

/// JS truthiness (`!!v`).
pub fn truthy(v: Ov) -> bool {
    v.is_some_and(V::truthy)
}
/// `v === undefined || v === null`.
pub fn nullish(v: Ov) -> bool {
    matches!(v, None | Some(V::Null))
}
/// `typeof v === 'number'` → the number.
pub fn num(v: Ov) -> Option<f64> {
    match v {
        Some(V::Num(n)) => Some(*n),
        _ => None,
    }
}
/// `Number.isFinite(v)` → the number.
pub fn fin(v: Ov) -> Option<f64> {
    num(v).filter(|x| x.is_finite())
}
/// `typeof v === 'string'` → the string.
pub fn s<'a>(v: Ov<'a>) -> Option<&'a str> {
    match v {
        Some(V::Str(x)) => Some(x.as_str()),
        _ => None,
    }
}

/// `+v` / `Number(v)` (undefined → NaN).
pub fn to_num(v: Ov) -> f64 {
    match v {
        None => f64::NAN,
        Some(V::Null) => 0.0,
        Some(V::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(V::Num(n)) => *n,
        Some(V::Str(x)) => js::str_to_number(x),
        Some(V::Arr(a)) => match a.as_slice() {
            [] => 0.0,
            [x] => js::str_to_number(&to_str(Some(x))),
            _ => f64::NAN,
        },
        Some(V::Obj(_) | V::Bytes(_)) => f64::NAN,
    }
}
/// `String(v)` (undefined → "undefined").
pub fn to_str(v: Ov) -> String {
    match v {
        None => "undefined".into(),
        Some(V::Null) => "null".into(),
        Some(V::Bool(b)) => b.to_string(),
        Some(V::Num(n)) => js::num_to_string(*n),
        Some(V::Str(x)) => x.clone(),
        Some(V::Arr(a)) => {
            a.iter().map(|x| if matches!(x, V::Null) { String::new() } else { to_str(Some(x)) }).collect::<Vec<_>>().join(",")
        }
        Some(V::Obj(_)) => "[object Object]".into(),
        Some(V::Bytes(b)) => b.iter().map(u8::to_string).collect::<Vec<_>>().join(","),
    }
}
/// `String(v || '')`.
pub fn str_or_empty(v: Ov) -> String {
    if truthy(v) { to_str(v) } else { String::new() }
}
/// `String(v ?? '')`.
pub fn str_nn(v: Ov) -> String {
    if nullish(v) { String::new() } else { to_str(v) }
}
/// `v || d` for a number-ish read (the value's `+v` when truthy).
pub fn or(v: Ov, d: f64) -> f64 {
    if truthy(v) { to_num(v) } else { d }
}
/// `v ?? d` for a number-ish read.
pub fn nn(v: Ov, d: f64) -> f64 {
    if nullish(v) { d } else { to_num(v) }
}

/// `parseFloat(s)` (the longest decimal prefix after white space; Infinity; else NaN).
pub fn parse_float(st: &str) -> f64 {
    let t = js::trim_start(st);
    let b = t.as_bytes();
    let mut i = 0usize;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i = 1;
    }
    if t.get(i..).is_some_and(|r| r.starts_with("Infinity")) {
        return if b.first() == Some(&b'-') { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let start = i;
    let mut digits = 0usize;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i = i.saturating_add(1);
        digits = digits.saturating_add(1);
    }
    if b.get(i) == Some(&b'.') {
        let mut j = i.saturating_add(1);
        let mut fd = 0usize;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j = j.saturating_add(1);
            fd = fd.saturating_add(1);
        }
        if digits > 0 || fd > 0 {
            i = j;
            digits = digits.saturating_add(fd);
        }
    }
    if digits == 0 {
        return f64::NAN;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i.saturating_add(1);
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j = j.saturating_add(1);
        }
        let es = j;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j = j.saturating_add(1);
        }
        if j > es {
            i = j;
        }
    }
    let _ = start;
    js::str_to_number(t.get(..i).unwrap_or(""))
}

/// `parseInt(s, 10)` (NaN when no digits).
pub fn parse_int(st: &str) -> f64 {
    let t = js::trim_start(st);
    let b = t.as_bytes();
    let mut i = 0usize;
    let neg = b.first() == Some(&b'-');
    if matches!(b.first(), Some(b'+' | b'-')) {
        i = 1;
    }
    let mut v = 0.0f64;
    let mut any = false;
    while let Some(c) = b.get(i).filter(|c| c.is_ascii_digit()) {
        v = v * 10.0 + f64::from(c.wrapping_sub(b'0'));
        any = true;
        i = i.saturating_add(1);
    }
    if !any {
        return f64::NAN;
    }
    if neg { -v } else { v }
}

/// `parseInt(s, 16)` (NaN when no hex digits; an optional sign and 0x prefix).
pub fn parse_int_radix16(st: &str) -> f64 {
    let t = js::trim_start(st);
    let (neg, t) = match t.as_bytes().first() {
        Some(b'-') => (true, t.get(1..).unwrap_or("")),
        Some(b'+') => (false, t.get(1..).unwrap_or("")),
        _ => (false, t),
    };
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    let mut v = 0.0f64;
    let mut any = false;
    for c in t.chars() {
        let Some(d) = c.to_digit(16) else { break };
        v = v * 16.0 + f64::from(d);
        any = true;
    }
    if !any {
        return f64::NAN;
    }
    if neg { -v } else { v }
}

/// `x | 0` as an i64 (ToInt32).
pub fn i32_of(x: f64) -> i64 {
    f2i(js::int32(x))
}
/// `x >>> 0` (ToUint32).
pub fn u32_of(x: f64) -> u64 {
    if !x.is_finite() {
        return 0;
    }
    let m = x.trunc().rem_euclid(4_294_967_296.0);
    f2u(m)
}
/// A float that is an integer in range → i64 (callers pass integral values; out of range saturates).
#[allow(clippy::cast_possible_truncation)]
pub fn f2i(x: f64) -> i64 {
    if x.is_nan() { 0 } else { x as i64 }
}
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn f2u(x: f64) -> u64 {
    if x.is_nan() || x <= 0.0 { 0 } else { x as u64 }
}
/// A float count → usize (NaN / negative → 0, saturating).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn f2us(x: f64) -> usize {
    if x.is_nan() || x <= 0.0 { 0 } else { x as usize }
}
/// usize → f64.
#[allow(clippy::cast_precision_loss)]
pub fn us2f(n: usize) -> f64 {
    n as f64
}
#[allow(clippy::cast_precision_loss)]
pub fn i2f(n: i64) -> f64 {
    n as f64
}

/// Case-insensitive ASCII equality (the JS `/^…$/i` tests on names).
pub fn ieq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}
/// JS `toUpperCase()` (full Unicode, as V8).
pub fn upper(st: &str) -> String {
    st.to_uppercase()
}

/// A point `{x, y}`.
pub fn p2(x: f64, y: f64) -> V {
    cc_io_dwg::value::p2(x, y)
}

/// `[].concat(c)`: an array's items, or the value itself.
pub fn flat1(v: V) -> Vec<V> {
    match v {
        V::Arr(a) => a,
        other => vec![other],
    }
}

/// A number as JSON: `cc_cad::entity::num`, but -0 stays -0 (the DWG writer keeps its sign bit, as the JS does).
fn jnum(n: f64) -> Value {
    if n == 0.0 && n.is_sign_negative() {
        return serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number);
    }
    cc_cad::entity::num(n)
}

/// The value as JSON for doc.load (private keys left out).
pub fn to_json(v: &V) -> Value {
    match v {
        V::Null => Value::Null,
        V::Bool(b) => Value::Bool(*b),
        V::Num(n) => jnum(*n),
        V::Str(x) => Value::String(x.clone()),
        V::Bytes(b) => Value::String(cc_io_dwg::util::b64(b)),
        V::Arr(a) => Value::Array(a.iter().map(to_json).collect()),
        V::Obj(o) => {
            let mut m = serde_json::Map::with_capacity(o.len());
            for (k, x) in o.iter() {
                if k.starts_with(HIDDEN) {
                    continue;
                }
                m.insert(k.to_string(), to_json(x));
            }
            Value::Object(m)
        }
    }
}

/// JSON → the dynamic value ("$NaN" / "$Inf" / "$-Inf" strings back to numbers).
pub fn from_json(v: &Value) -> V {
    match v {
        Value::Null => V::Null,
        Value::Bool(b) => V::Bool(*b),
        Value::Number(n) => V::Num(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(x) => match x.as_str() {
            "$NaN" => V::Num(f64::NAN),
            "$Inf" => V::Num(f64::INFINITY),
            "$-Inf" => V::Num(f64::NEG_INFINITY),
            _ => V::Str(x.clone()),
        },
        Value::Array(a) => V::Arr(a.iter().map(from_json).collect()),
        Value::Object(m) => {
            let mut o = Obj::with_capacity(m.len());
            for (k, x) in m {
                o.0.push((std::borrow::Cow::Owned(k.clone()), from_json(x)));
            }
            V::Obj(o)
        }
    }
}

/// Shallow copy of an object with more keys (`{ ...a, k: v }`).
pub fn spread(a: &Obj, more: Vec<(&'static str, V)>) -> Obj {
    let mut o = a.clone();
    for (k, v) in more {
        o.set(k, v);
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses() {
        assert_eq!(parse_float(" 12.5abc"), 12.5);
        assert_eq!(parse_float("-.5"), -0.5);
        assert_eq!(parse_float("1e3x"), 1000.0);
        assert_eq!(parse_float("1e"), 1.0);
        assert!(parse_float("abc").is_nan());
        assert!(parse_float(".").is_nan());
        assert_eq!(parse_float("5."), 5.0);
        assert_eq!(parse_int(" 42x"), 42.0);
        assert_eq!(parse_int("-7"), -7.0);
        assert!(parse_int("x").is_nan());
        assert_eq!(parse_int("3.9"), 3.0);
        assert_eq!(u32_of(-1.0), 4_294_967_295);
    }
}

/// `Object.keys(o)` order: array-index keys ascending first, then the rest in insertion order.
pub fn keys_js(o: &Obj) -> Vec<&str> {
    let index = |k: &str| -> Option<u32> {
        if k == "0" {
            return Some(0);
        }
        if k.starts_with('0') || k.is_empty() || !k.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        k.parse::<u32>().ok().filter(|n| *n != u32::MAX)
    };
    let mut idx: Vec<(u32, &str)> = o.keys().filter_map(|k| index(k).map(|n| (n, k))).collect();
    idx.sort_by_key(|(n, _)| *n);
    let mut out: Vec<&str> = idx.into_iter().map(|(_, k)| k).collect();
    out.extend(o.keys().filter(|k| index(k).is_none()));
    out
}

/// `v || d` keeping the value (any type).
pub fn orv(v: Ov, d: V) -> V {
    match v {
        Some(x) if x.truthy() => x.clone(),
        _ => d,
    }
}
/// `v ?? d` keeping the value.
pub fn nnv(v: Ov, d: V) -> V {
    match v {
        None | Some(V::Null) => d,
        Some(x) => x.clone(),
    }
}

/// `+v || d` (the number, or d when it is 0 or NaN).
pub fn pn_or(v: Ov, d: f64) -> f64 {
    let n = to_num(v);
    if n != 0.0 && !n.is_nan() { n } else { d }
}

/// A JavaScript exception (the V8 message) where the JS would throw: the importer stops, as the JS one does.
#[derive(Clone, Debug, PartialEq)]
pub struct JsErr(pub String);
impl std::fmt::Display for JsErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for JsErr {}
pub type R<T> = Result<T, JsErr>;

/// `o.k` where o must not be undefined / null (V8's TypeError otherwise).
pub fn prop<'a>(o: Ov<'a>, k: &str) -> R<Ov<'a>> {
    match o {
        None => Err(JsErr(format!("Cannot read properties of undefined (reading '{k}')"))),
        Some(V::Null) => Err(JsErr(format!("Cannot read properties of null (reading '{k}')"))),
        Some(_) => Ok(get(o, k)),
    }
}
/// `o.k` as an optional value clone for an object literal (`{ k: o.k }`; undefined → absent).
pub fn pv(o: Ov, k: &str) -> R<Option<V>> {
    Ok(prop(o, k)?.cloned())
}

/// `(i + 1) % n` for a ring index (0 when n is 0).
pub fn wrap_next(i: usize, n: usize) -> usize {
    i.saturating_add(1).checked_rem(n).unwrap_or(0)
}

/// [`to_json`] consuming the value (strings and owned keys are moved, not copied).
pub fn into_json(v: V) -> Value {
    match v {
        V::Null => Value::Null,
        V::Bool(b) => Value::Bool(b),
        V::Num(n) => jnum(n),
        V::Str(x) => Value::String(x),
        V::Bytes(b) => Value::String(cc_io_dwg::util::b64(&b)),
        V::Arr(a) => Value::Array(a.into_iter().map(into_json).collect()),
        V::Obj(o) => {
            let mut m = serde_json::Map::with_capacity(o.len());
            for (k, x) in o.0 {
                if k.starts_with(HIDDEN) {
                    continue;
                }
                m.insert(k.into_owned(), into_json(x));
            }
            Value::Object(m)
        }
    }
}
