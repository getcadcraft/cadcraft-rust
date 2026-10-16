//! The value box's parsing (units.js `parseLength` / `parseValue`, one for one, the same regular expressions run by
//! [`crate::jsre`]); `formatLength` & co. are the kernel's (`cc_m3d::units`). Typed text is hostile: no allocation
//! grows with anything but the text itself, numbers go through JS `Number()` semantics, and counts (sides, copies)
//! are at most four digits, as the patterns allow.

#![deny(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::sync::OnceLock;

use cc_m3d::file::str_to_number;
use cc_m3d::js;
pub use cc_m3d::units::{FT, IN, format_area, format_length, format_volume, unit_scale};

use crate::jsre::Re;

/// What the value box was given.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// a length (m), and the text as a plain number when it is one (`num`)
    Len { v: f64, num: Option<f64> },
    /// lengths separated by commas or semicolons
    List(Vec<f64>),
    /// an absolute point `[x, y, z]`
    Abs([f64; 3]),
    /// an offset `<dx, dy, dz>`
    Rel([f64; 3]),
    /// segments / sides `24s`
    Sides(f64),
    /// copies in a row `3x` / `*3`
    Array(f64),
    /// copies between `/3`
    Divide(f64),
    /// a plain number (angles, factors)
    Num(f64),
}

impl Value {
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Len { .. } => "len",
            Value::List(_) => "list",
            Value::Abs(_) => "abs",
            Value::Rel(_) => "rel",
            Value::Sides(_) => "sides",
            Value::Array(_) => "array",
            Value::Divide(_) => "divide",
            Value::Num(_) => "num",
        }
    }
    /// `v.n` (sides, array, divide).
    pub fn n(&self) -> Option<f64> {
        match self {
            Value::Sides(n) | Value::Array(n) | Value::Divide(n) => Some(*n),
            _ => None,
        }
    }
    /// `v.v` (len, num).
    pub fn v(&self) -> Option<f64> {
        match self {
            Value::Len { v, .. } | Value::Num(v) => Some(*v),
            _ => None,
        }
    }
    /// The JSON the JS returns (for the parity harness).
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;
        let f = |x: f64| crate::outj::num(x);
        match self {
            Value::Len { v, num } => json!({"kind": "len", "v": f(*v), "num": num.map_or(serde_json::Value::Null, f)}),
            Value::List(vs) => json!({"kind": "list", "vs": vs.iter().map(|x| f(*x)).collect::<Vec<_>>()}),
            Value::Abs(p) => json!({"kind": "abs", "p": p.iter().map(|x| f(*x)).collect::<Vec<_>>()}),
            Value::Rel(p) => json!({"kind": "rel", "p": p.iter().map(|x| f(*x)).collect::<Vec<_>>()}),
            Value::Sides(n) => json!({"kind": "sides", "n": f(*n)}),
            Value::Array(n) => json!({"kind": "array", "n": f(*n)}),
            Value::Divide(n) => json!({"kind": "divide", "n": f(*n)}),
            Value::Num(v) => json!({"kind": "num", "v": f(*v)}),
        }
    }
}

const N: &str = r"(\d+(?:\.\d*)?|\.\d+)";

fn re(cell: &'static OnceLock<Re>, make: impl FnOnce() -> Re) -> &'static Re {
    cell.get_or_init(make)
}

fn suffix(s: &str) -> f64 {
    match s {
        "mm" => 0.001,
        "cm" => 0.01,
        "m" => 1.0,
        "in" | "inch" | "inches" | "\"" => IN,
        "ft" | "feet" | "foot" | "'" => FT,
        _ => f64::NAN,
    }
}

fn num_of(m: &[Option<String>], i: usize) -> f64 {
    m.get(i).and_then(|x| x.as_deref()).map_or(f64::NAN, str_to_number)
}
fn has(m: &[Option<String>], i: usize) -> bool {
    m.get(i).is_some_and(Option::is_some)
}
/// JS truthiness of a number.
fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

/// `parseLength(text, unit)` → metres, or None.
pub fn parse_length(text: &str, unit: &str) -> Option<f64> {
    let s: String = js::trim(text)
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{2032}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{2033}' => '"',
            c => c,
        })
        .collect::<String>()
        .to_lowercase();
    if s.is_empty() {
        return None;
    }
    let mut sign = 1.0;
    let mut s = s.as_str();
    if s.starts_with('-') {
        sign = -1.0;
        s = js::trim(s.get(1..).unwrap_or(""));
    } else if s.starts_with('+') {
        s = js::trim(s.get(1..).unwrap_or(""));
    }
    static R1: OnceLock<Re> = OnceLock::new();
    static R2: OnceLock<Re> = OnceLock::new();
    static R3: OnceLock<Re> = OnceLock::new();
    static R4: OnceLock<Re> = OnceLock::new();
    // feet with optional inches and fraction: 10'  10'6"  10'-6 1/2"  10' 6.5
    let r1 = re(&R1, || Re::new(&format!(r#"^{N}\s*'\s*(?:-?\s*{N}?\s*(?:(\d+)\s*/\s*(\d+))?\s*"?)?$"#), false));
    if let Some(m) = r1.exec(s) {
        let ft = num_of(&m, 1);
        let inch = if has(&m, 2) { num_of(&m, 2) } else { 0.0 };
        let fr = if has(&m, 3) && truthy(num_of(&m, 4)) { num_of(&m, 3) / num_of(&m, 4) } else { 0.0 };
        return Some(sign * (ft * FT + (inch + fr) * IN));
    }
    // inches with a fraction: 6 1/2"  6-1/2"
    let r2 = re(&R2, || Re::new(&format!(r#"^{N}\s*[- ]\s*(\d+)\s*/\s*(\d+)\s*("|in)?$"#), false));
    if let Some(m) = r2.exec(s)
        && truthy(num_of(&m, 3))
    {
        let v = num_of(&m, 1) + num_of(&m, 2) / num_of(&m, 3);
        let inches = unit == "ft-in" || unit == "in" || has(&m, 4);
        return Some(sign * if inches { v * IN } else { v * unit_scale(unit) });
    }
    // a fraction alone: 3/4"  3/4
    let r3 = re(&R3, || Re::new(r#"^(\d+)\s*/\s*(\d+)\s*("|in)?$"#, false));
    if let Some(m) = r3.exec(s)
        && truthy(num_of(&m, 2))
    {
        let v = num_of(&m, 1) / num_of(&m, 2);
        return Some(sign * v * if has(&m, 3) { IN } else { unit_scale(unit) });
    }
    // number + suffix
    let r4 = re(&R4, || Re::new(&format!(r#"^{N}\s*(mm|cm|m|inches|inch|in|feet|foot|ft|"|')?$"#), false));
    if let Some(m) = r4.exec(s) {
        let v = num_of(&m, 1);
        if !v.is_finite() {
            return None;
        }
        let k = match m.get(2).and_then(|x| x.as_deref()) {
            Some(sf) => suffix(sf),
            None => unit_scale(unit),
        };
        return Some(sign * v * k);
    }
    None
}

fn split_list(s: &str) -> Vec<&str> {
    s.split([',', ';']).collect()
}

/// `parseValue(text, unit)`.
pub fn parse_value(text: &str, unit: &str) -> Option<Value> {
    let s = js::trim(text);
    if s.is_empty() {
        return None;
    }
    static P: OnceLock<Re> = OnceLock::new();
    static S: OnceLock<Re> = OnceLock::new();
    static A: OnceLock<Re> = OnceLock::new();
    static D: OnceLock<Re> = OnceLock::new();
    static NUM: OnceLock<Re> = OnceLock::new();
    let rp = re(&P, || Re::new(r"^([[<])\s*(.+?)\s*([\]>])$", false));
    if let Some(m) = rp.exec(s) {
        let inner = m.get(2).and_then(|x| x.clone()).unwrap_or_default();
        let parts: Vec<Option<f64>> = split_list(&inner).into_iter().map(|x| parse_length(x, unit)).collect();
        let [Some(a), Some(b), Some(c)] = parts.as_slice() else { return None };
        let p = [*a, *b, *c];
        return Some(if m.get(1).and_then(|x| x.as_deref()) == Some("[") { Value::Abs(p) } else { Value::Rel(p) });
    }
    if let Some(m) = re(&S, || Re::new(r"^(\d{1,4})\s*s$", true)).exec(s) {
        return Some(Value::Sides(num_of(&m, 1)));
    }
    if let Some(m) = re(&A, || Re::new(r"^(?:(\d{1,4})\s*x|[*x]\s*(\d{1,4}))$", true)).exec(s) {
        let n = if has(&m, 1) { num_of(&m, 1) } else { num_of(&m, 2) };
        return Some(Value::Array(n));
    }
    if let Some(m) = re(&D, || Re::new(r"^/\s*(\d{1,4})$", false)).exec(s) {
        return Some(Value::Divide(num_of(&m, 1)));
    }
    if s.contains([',', ';']) {
        let vs: Vec<Option<f64>> =
            split_list(s).into_iter().map(|x| if js::trim(x).is_empty() { None } else { parse_length(x, unit) }).collect();
        if vs.iter().any(Option::is_none) {
            return None;
        }
        return Some(Value::List(vs.into_iter().map(|x| x.unwrap_or(0.0)).collect()));
    }
    let v = parse_length(s, unit);
    let num = if re(&NUM, || Re::new(r"^[-+]?(\d+(\.\d*)?|\.\d+)$", false)).test(s) { Some(str_to_number(s)) } else { None };
    if let Some(v) = v {
        return Some(Value::Len { v, num });
    }
    num.map(Value::Num)
}

/// JS `parseFloat(text)`: the longest prefix (after leading white space) that reads as a decimal number, else NaN.
pub fn parse_float(text: &str) -> f64 {
    let s = text.trim_start_matches(js::is_ws);
    let b = s.as_bytes();
    let mut i = 0usize;
    let at = |k: usize| b.get(k).copied();
    if matches!(at(i), Some(b'+' | b'-')) {
        i = i.saturating_add(1);
    }
    if s.get(i..).is_some_and(|r| r.starts_with("Infinity")) {
        return if s.starts_with('-') { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let mut digits = 0usize;
    while at(i).is_some_and(|c| c.is_ascii_digit()) {
        i = i.saturating_add(1);
        digits = digits.saturating_add(1);
    }
    if at(i) == Some(b'.') {
        let mut j = i.saturating_add(1);
        let mut frac = 0usize;
        while at(j).is_some_and(|c| c.is_ascii_digit()) {
            j = j.saturating_add(1);
            frac = frac.saturating_add(1);
        }
        if digits > 0 || frac > 0 {
            i = j;
            digits = digits.saturating_add(frac);
        }
    }
    if digits == 0 {
        return f64::NAN;
    }
    if matches!(at(i), Some(b'e' | b'E')) {
        let mut j = i.saturating_add(1);
        if matches!(at(j), Some(b'+' | b'-')) {
            j = j.saturating_add(1);
        }
        let start = j;
        while at(j).is_some_and(|c| c.is_ascii_digit()) {
            j = j.saturating_add(1);
        }
        if j > start {
            i = j;
        }
    }
    s.get(..i).and_then(|p| p.parse::<f64>().ok()).unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn close(a: Option<f64>, b: f64) -> bool {
        a.is_some_and(|a| (a - b).abs() <= 1e-9)
    }
    #[test]
    fn lengths() {
        let cases = [
            ("10'", "ft-in", 10.0 * FT),
            ("10'6\"", "ft-in", 10.0 * FT + 6.0 * IN),
            ("10'-6 1/2\"", "ft-in", 10.0 * FT + 6.5 * IN),
            ("6 1/2\"", "m", 6.5 * IN),
            ("3/4\"", "m", 0.75 * IN),
            ("18", "ft-in", 18.0 * IN),
            ("3.2m", "ft-in", 3.2),
            ("320cm", "m", 3.2),
            ("3200", "mm", 3.2),
            ("1.5ft", "m", 1.5 * FT),
            ("-2m", "m", -2.0),
        ];
        for (t, u, v) in cases {
            assert!(close(parse_length(t, u), v), "{t} {u}");
        }
        assert_eq!(parse_length("abc", "m"), None);
        assert_eq!(parse_length("", "m"), None);
    }
    #[test]
    fn values() {
        assert_eq!(parse_value("4m,3m", "m"), Some(Value::List(vec![4.0, 3.0])));
        assert_eq!(parse_value("24s", "m"), Some(Value::Sides(24.0)));
        assert_eq!(parse_value("3x", "m"), Some(Value::Array(3.0)));
        assert_eq!(parse_value("/4", "m"), Some(Value::Divide(4.0)));
        assert_eq!(parse_value("[1,2,3]", "m"), Some(Value::Abs([1.0, 2.0, 3.0])));
        assert_eq!(parse_value("<1,0,0>", "m"), Some(Value::Rel([1.0, 0.0, 0.0])));
        assert_eq!(parse_value("2", "m"), Some(Value::Len { v: 2.0, num: Some(2.0) }));
        assert_eq!(parse_value("4000,3000", "mm"), Some(Value::List(vec![4.0, 3.0])));
        assert_eq!(parse_float("30m"), 30.0);
        assert!(parse_float("m").is_nan());
        assert_eq!(parse_float(" -1.5e2x"), -150.0);
    }
}
