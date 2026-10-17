//! The JavaScript semantics DetailCraft's modules lean on, gathered in one place. Nothing is re-implemented that
//! another crate already has: V8's `Math.*` and number text come from cc-cad's `js` and cc-m3d's `jsmath`, ICU-order
//! `localeCompare` and UTF-16 slicing from cc-civil's `js`. What is here is the glue: `Number(v)` / `String(v)` of a
//! JSON value, `cleanText`, and the JSON text the model's snapshots and files are made of (`JSON.stringify`).

use serde_json::{Map, Value};

pub use cc_cad::js::{hypot, max, min, num_to_string, round, sign, slice16, str_to_number, to_fixed, to_js_string, to_number, trim};
pub use cc_civil::js::{J, Obj, arr, as_str, is_integer, jnum, locale_compare, locale_compare_num, ord, sort_by};

/// `Math.sin` / `Math.cos` (fdlibm, bit for bit with V8).
#[inline]
pub fn sin(x: f64) -> f64 {
    cc_m3d::jsmath::fd_sin(x)
}
#[inline]
pub fn cos(x: f64) -> f64 {
    cc_m3d::jsmath::fd_cos(x)
}
/// `Math.atan2`, `Math.atan`, `Math.acos`, `Math.asin` (msun, as V8's ieee754).
#[inline]
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}
#[inline]
pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}
#[inline]
pub fn acos(x: f64) -> f64 {
    libm::acos(x)
}
#[inline]
pub fn asin(x: f64) -> f64 {
    libm::asin(x)
}
/// `Math.hypot(a, b, c)`.
#[inline]
pub fn hypot3(a: f64, b: f64, c: f64) -> f64 {
    cc_m3d::js::hypot3(a, b, c)
}
/// `Math.hypot(...xs)`.
pub fn hypot_n(xs: &[f64]) -> f64 {
    cc_m3d::js::hypot(xs)
}

/// `Number(x.toFixed(d))`.
pub fn fixed_num(x: f64, d: usize) -> f64 {
    str_to_number(&to_fixed(x, d))
}

/// `Math.max(...xs)` (−∞ for none, NaN wins).
pub fn max_of(xs: impl IntoIterator<Item = f64>) -> f64 {
    xs.into_iter().fold(f64::NEG_INFINITY, max)
}
/// `Math.min(...xs)` (+∞ for none, NaN wins).
pub fn min_of(xs: impl IntoIterator<Item = f64>) -> f64 {
    xs.into_iter().fold(f64::INFINITY, min)
}

/// `Number(v)` (undefined → NaN).
pub fn num(v: Option<&Value>) -> f64 {
    v.map_or(f64::NAN, to_number)
}
/// `Number(v) || 0`.
pub fn num_or0(v: Option<&Value>) -> f64 {
    let x = num(v);
    if x == 0.0 || x.is_nan() { 0.0 } else { x }
}
/// A number field the model wrote itself (a number, else NaN).
pub fn f(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(f64::NAN)
}
/// `x || d` for a number.
pub fn or(x: f64, d: f64) -> f64 {
    if x == 0.0 || x.is_nan() { d } else { x }
}
/// JS truthiness of a JSON value (absent = falsy).
pub fn truthy(v: Option<&Value>) -> bool {
    cc_cad::js::truthy(v)
}
/// `String(v)` (undefined → "undefined").
pub fn string_of(v: Option<&Value>) -> String {
    v.map_or_else(|| "undefined".to_string(), to_js_string)
}
/// `String(v ?? '')`.
pub fn string_or_empty(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(x) => to_js_string(x),
    }
}
/// The text of a string value, else "".
pub fn s(v: Option<&Value>) -> &str {
    v.and_then(Value::as_str).unwrap_or("")
}
/// `v ?? d` (absent or null → d).
pub fn nullish<'a>(v: Option<&'a Value>, d: &'a Value) -> &'a Value {
    match v {
        None | Some(Value::Null) => d,
        Some(x) => x,
    }
}

/// `s.replace(/[\u0000-\u001f\u007f]/g, '')`.
pub fn strip_controls(s: &str) -> String {
    s.chars().filter(|&c| !((c as u32) < 0x20 || c == '\u{7f}')).collect()
}
/// `s.replace(/[\u0000-\u001f]/g, '')` (DEL kept).
pub fn strip_c0(s: &str) -> String {
    s.chars().filter(|&c| (c as u32) >= 0x20).collect()
}
/// `String(s ?? '').replace(/[\u0000-\u001f\u007f]/g, '').trim().slice(0, max)` — model.js `cleanText`.
pub fn clean_text(v: Option<&Value>, max: usize) -> String {
    slice16(trim(&strip_controls(&string_or_empty(v))), max)
}
/// `cleanText` of a Rust string.
pub fn clean_str(v: &str, max: usize) -> String {
    slice16(trim(&strip_controls(v)), max)
}
/// `s.toUpperCase()` (full Unicode mapping, as V8).
pub fn upper(s: &str) -> String {
    s.to_uppercase()
}
/// `s.length` in UTF-16 code units.
pub fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `JSON.stringify(v)` (numbers as `String(x)`, NaN and ±∞ as null).
pub fn stringify(v: &Value) -> String {
    let mut o = cc_m3d::json::Out::new(false);
    o.value(v);
    o.s
}
/// `JSON.stringify(v, null, indent)`.
pub fn stringify_indent(v: &Value, indent: usize) -> String {
    fn go(o: &mut cc_m3d::json::Out, v: &Value, depth: usize, pad: &str) {
        let nl = |o: &mut cc_m3d::json::Out, d: usize| {
            o.ch('\n');
            for _ in 0..d {
                o.raw(pad);
            }
        };
        match v {
            Value::Array(a) if !a.is_empty() && depth < 512 => {
                o.ch('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        o.ch(',');
                    }
                    nl(o, depth.saturating_add(1));
                    go(o, x, depth.saturating_add(1), pad);
                }
                nl(o, depth);
                o.ch(']');
            }
            Value::Object(m) if !m.is_empty() && depth < 512 => {
                o.ch('{');
                for (i, (k, x)) in m.iter().enumerate() {
                    if i > 0 {
                        o.ch(',');
                    }
                    nl(o, depth.saturating_add(1));
                    o.str(k);
                    o.raw(": ");
                    go(o, x, depth.saturating_add(1), pad);
                }
                nl(o, depth);
                o.ch('}');
            }
            x => o.value(x),
        }
    }
    let pad = " ".repeat(indent.min(10));
    let mut o = cc_m3d::json::Out::new(false);
    go(&mut o, v, 0, &pad);
    o.s
}
/// `JSON.parse(text)` (an error for anything that is not JSON).
pub fn parse(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

/// A JSON number from an f64 (non-finite → null, as `JSON.stringify` would write it).
#[inline]
pub fn n(x: f64) -> Value {
    Value::from(x)
}
/// A JSON point `[x, y, z]` (non-finite numbers in the parity kit's form).
pub fn p3v(p: [f64; 3]) -> Value {
    Value::Array(vec![jnum(p[0]), jnum(p[1]), jnum(p[2])])
}
/// An object's map (empty for anything else).
pub fn obj(v: Option<&Value>) -> Option<&Map<String, Value>> {
    v.and_then(Value::as_object)
}
/// `Array.isArray(v) ? v : []`.
pub fn list(v: Option<&Value>) -> &[Value] {
    arr(v)
}
/// `{...a, ...b}` for two objects (non-objects spread nothing).
pub fn spread(a: Option<&Value>, b: Option<&Value>) -> Map<String, Value> {
    let mut m = Map::new();
    for src in [a, b] {
        match src {
            Some(Value::Object(o)) => {
                for (k, v) in o {
                    m.insert(k.clone(), v.clone());
                }
            }
            Some(Value::String(t)) => {
                // spreading a string gives its characters by index
                for (i, c) in t.encode_utf16().collect::<Vec<u16>>().iter().enumerate() {
                    m.insert(i.to_string(), Value::String(String::from_utf16_lossy(&[*c])));
                }
            }
            Some(Value::Array(a)) => {
                for (i, v) in a.iter().enumerate() {
                    m.insert(i.to_string(), v.clone());
                }
            }
            _ => {}
        }
    }
    m
}
/// A point read as the model stores it (`[x, y, z]` numbers).
pub fn p3_of(v: Option<&Value>) -> [f64; 3] {
    let a = arr(v);
    [f(a.first()), f(a.get(1)), f(a.get(2))]
}

/// The default `Array.prototype.sort` order of two strings (UTF-16 code units).
pub fn utf16_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// A thrown error's message.
pub type Res<T> = Result<T, String>;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn texts() {
        assert_eq!(clean_text(Some(&Value::from(" a\u{0}b ")), 80), "ab");
        assert_eq!(clean_text(None, 3), "");
        assert_eq!(stringify_indent(&serde_json::json!({"a": [1, 2.5], "b": {}}), 1), "{\n \"a\": [\n  1,\n  2.5\n ],\n \"b\": {}\n}");
        assert_eq!(fixed_num(1.005, 2), 1.0);
    }
}
