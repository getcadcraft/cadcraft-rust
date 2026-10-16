//! units.js: how lengths are written and read — decimal, feet and inches, fractions. Format codes are AutoCAD's
//! ($LUNITS / DIMLUNIT): 1 Scientific, 2 Decimal (default), 3 Engineering, 4 Architectural, 5 Fractional.

use crate::jsmath::JsMath;
use cc_cad::i18n::{Lookup, t};
use cc_cad::js;
use serde_json::Value;

/// The format codes' names ($LUNITS / DIMLUNIT).
pub const LUNITS: [(i32, &str); 5] = [(1, "Scientific"), (2, "Decimal"), (3, "Engineering"), (4, "Architectural"), (5, "Fractional")];
/// The default precision of every format.
pub const DEFAULT_PREC: i32 = 4;

/// A valid format code (6, "Windows desktop", and anything unknown read as decimal). `v` = `+value`.
pub fn lunit_code(v: f64) -> i32 {
    let i = js::int32(v);
    if (1.0..=5.0).contains(&i) && v == i {
        // 1..=5
        #[allow(clippy::cast_possible_truncation)]
        let c = i as i32;
        c
    } else {
        2
    }
}
/// `lunitCode` of a JSON value (`v | 0`, `Number(v)`).
pub fn lunit_code_of(v: Option<&Value>) -> i32 {
    lunit_code(v.map_or(f64::NAN, js::to_number))
}

/// precOf: `prec` rounded into 0…8, or the default (4) when it is null, '' or not a number.
pub fn prec_of(_code: i32, prec: Option<&Value>) -> i32 {
    match prec {
        None | Some(Value::Null) => 4,
        Some(Value::String(s)) if s.is_empty() => 4,
        Some(v) => {
            let n = js::to_number(v);
            if n.is_finite() {
                let r = js::max(0.0, js::min(8.0, js::round(n)));
                // 0..=8
                #[allow(clippy::cast_possible_truncation)]
                let c = r as i32;
                c
            } else {
                4
            }
        }
    }
}

/// Feet and inches: the formats that read one drawing unit as one inch.
pub fn is_imperial_format(code: i32) -> bool {
    code == 3 || code == 4
}

/// The format a drawing uses: (lunits, prec) from its meta.
pub fn units_of(meta: Option<&serde_json::Map<String, Value>>) -> (i32, i32) {
    let lu = meta.and_then(|m| m.get("lunits")).filter(|v| !v.is_null());
    let code = match lu {
        Some(v) => lunit_code(js::to_number(v)),
        None => lunit_code(2.0),
    };
    (code, prec_of(code, meta.and_then(|m| m.get("luprec"))))
}

fn pow10(p: i32) -> f64 {
    10f64.powi(p)
}

/// `Number(a.toFixed(prec)).toString()` after rounding tiny values to 0.
pub fn decimals(v: f64, prec: i32) -> String {
    let a = if v.abs() < 0.5 * 10f64.jpow(f64::from(-prec)) { 0.0 } else { v };
    let fixed = js::to_fixed(a, usize::try_from(prec.max(0)).unwrap_or(0));
    js::num_to_string(fixed.parse::<f64>().unwrap_or(f64::NAN))
}

/// A whole part and a reduced fraction of den: (whole, num, den) (num 0 = none).
fn fraction(v: f64, den: f64) -> (f64, f64, f64) {
    let n = js::round(v.abs() * den);
    let whole = (n / den).floor();
    let mut num = n - whole * den;
    let mut d = den;
    while num != 0.0 && num % 2.0 == 0.0 && d % 2.0 == 0.0 {
        num /= 2.0;
        d /= 2.0;
    }
    (whole, num, d)
}

/// A length as text in a format (`lunits` = the code as a number, `prec` as given).
pub fn format_length(v: f64, lunits: f64, prec: Option<&Value>) -> String {
    if !v.is_finite() {
        return String::new();
    }
    let code = lunit_code(lunits);
    let p = prec_of(code, prec);
    let neg = if v < 0.0 { "-" } else { "" };
    let s = js::num_to_string;
    match code {
        1 => {
            let x = js::to_exponential(if v.abs() < 1e-300 { 0.0 } else { v }, usize::try_from(p).unwrap_or(0)).to_uppercase();
            // E+2 → E+02 (a single exponent digit gets a leading zero)
            match x.rfind('E') {
                Some(i) if x.len() == i + 3 && x.as_bytes().get(i + 2).is_some_and(u8::is_ascii_digit) => {
                    format!("{}0{}", x.get(..i + 2).unwrap_or(""), x.get(i + 2..).unwrap_or(""))
                }
                _ => x,
            }
        }
        3 => {
            let r = js::round(v.abs() * pow10(p)) / pow10(p);
            let ft = (r / 12.0 + 1e-12).floor();
            let inch = r - ft * 12.0;
            if r == 0.0 {
                return "0'-0\"".into();
            }
            format!("{neg}{}'-{}\"", s(ft), decimals(inch, p))
        }
        4 => {
            let den = 2f64.powi(p);
            let (whole, num, d) = fraction(v, den);
            let ft = (whole / 12.0).floor();
            let inch = whole - ft * 12.0;
            if whole == 0.0 && num == 0.0 {
                return "0'-0\"".into();
            }
            let frac = if num != 0.0 { format!(" {}/{}", s(num), s(d)) } else { String::new() };
            format!("{neg}{}'-{}{frac}\"", s(ft), s(inch))
        }
        5 => {
            let den = 2f64.powi(p);
            let (whole, num, d) = fraction(v, den);
            if whole == 0.0 && num == 0.0 {
                return "0".into();
            }
            let w = if whole != 0.0 || num == 0.0 { s(whole) } else { String::new() };
            let frac = if num != 0.0 { format!("{}{}/{}", if whole != 0.0 { " " } else { "" }, s(num), s(d)) } else { String::new() };
            format!("{neg}{w}{frac}")
        }
        _ => format!("{neg}{}", decimals(v.abs(), p)),
    }
}

/// An area as text: square drawing units; for feet and inches, square inches and square feet (the
/// 'units.area.imperial' catalog string).
pub fn format_area(a: f64, lunits: f64, prec: Option<&Value>, lookup: Lookup) -> String {
    let code = lunit_code(lunits);
    let p = prec_of(code, prec);
    if is_imperial_format(code) {
        let q = p.max(2);
        return t(lookup, "units.area.imperial", &[("sqin", decimals(a, q)), ("sqft", decimals(a / 144.0, q))]);
    }
    if code == 1 {
        return format_length(a, 1.0, Some(&Value::from(p)));
    }
    decimals(a, if code == 5 { 4 } else { p })
}

/// `^(?:\d+(?:\.\d*)?|\.\d+)(?:e[-+]?\d+)?$` (case-insensitive)
fn is_num_lit(t: &str) -> bool {
    let b = t.as_bytes();
    let mut i = 0;
    let digits = |i: &mut usize| {
        let s = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        *i > s
    };
    if digits(&mut i) {
        if b.get(i) == Some(&b'.') {
            i += 1;
            digits(&mut i);
        }
    } else if b.get(i) == Some(&b'.') {
        i += 1;
        if !digits(&mut i) {
            return false;
        }
    } else {
        return false;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if !digits(&mut i) {
            return false;
        }
    }
    i == b.len()
}
fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
}

/// "6", "6.5", "6-1/2", "6 1/2", "1/2" → a number, else None.
fn mixed(t: &str) -> Option<f64> {
    if is_num_lit(t) {
        return Some(js::str_to_number(t));
    }
    // ^(\d+)[- ](\d+)\/(\d+)$
    if let Some(sep) = t.find(['-', ' ']) {
        let (w, rest) = (t.get(..sep).unwrap_or(""), t.get(sep + 1..).unwrap_or(""));
        if let Some((a, b)) = rest.split_once('/')
            && all_digits(w)
            && all_digits(a)
            && all_digits(b)
        {
            let (w, a, b) = (js::str_to_number(w), js::str_to_number(a), js::str_to_number(b));
            if b > 0.0 {
                return Some(w + a / b);
            }
        }
    }
    // ^(\d+)\/(\d+)$
    if let Some((a, b)) = t.split_once('/')
        && all_digits(a)
        && all_digits(b)
    {
        let (a, b) = (js::str_to_number(a), js::str_to_number(b));
        if b > 0.0 {
            return Some(a / b);
        }
    }
    None
}

/// A typed length → drawing units, or None. Plain numbers and fractions always; feet and inches (12', 12'6",
/// 12'-6-1/2", 6") when the drawing's format is engineering or architectural.
pub fn parse_length(raw: &str, lunits: f64) -> Option<f64> {
    let mut s = js::trim(raw).to_string();
    if s.is_empty() {
        return None;
    }
    let mut sign = 1.0;
    if s.starts_with('-') || s.starts_with('+') {
        if s.starts_with('-') {
            sign = -1.0;
        }
        s = js::trim(s.get(1..).unwrap_or("")).to_string();
    }
    if s.is_empty() {
        return None;
    }
    if s.contains(['\'', '"']) {
        if !is_imperial_format(lunit_code(lunits)) {
            return None;
        }
        let mut ft = 0.0;
        let mut rest = s.clone();
        if let Some(q) = s.find('\'') {
            ft = mixed(js::trim(s.get(..q).unwrap_or("")))?;
            let r = js::trim(s.get(q + 1..).unwrap_or(""));
            rest = r.strip_prefix('-').unwrap_or(r).to_string();
        }
        let r = rest.strip_suffix('"').unwrap_or(&rest);
        let rest = js::trim(r).to_string();
        if rest.contains(['\'', '"']) {
            return None;
        }
        let mut inch = 0.0;
        if !rest.is_empty() {
            inch = mixed(&rest)?;
        }
        return Some(sign * (ft * 12.0 + inch));
    }
    mixed(&s).map(|v| sign * v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn format_cases() {
        // test-cadcraft.mjs 'Units: feet and inches, fractions'
        let cases: [(f64, f64, Value, &str); 13] = [
            (150.5, 4.0, json!(4), "12'-6 1/2\""),
            (6.0, 4.0, json!(4), "0'-6\""),
            (0.0, 4.0, json!(4), "0'-0\""),
            (-150.5, 4.0, json!(4), "-12'-6 1/2\""),
            (11.99, 4.0, json!(4), "1'-0\""),
            (150.25, 4.0, json!(1), "12'-6 1/2\""),
            (150.5, 3.0, json!(4), "12'-6.5\""),
            (150.5, 5.0, json!(4), "150 1/2"),
            (0.25, 5.0, json!(4), "1/4"),
            (150.5, 1.0, json!(4), "1.5050E+02"),
            (1.23456, 2.0, json!(4), "1.2346"),
            (1.23456, 2.0, Value::Null, "1.2346"),
            (2.0, 2.0, json!(2), "2"),
        ];
        for (v, l, p, want) in cases {
            assert_eq!(format_length(v, l, Some(&p)), want, "{v} {l}");
        }
    }
    #[test]
    fn parse_cases() {
        for (t, want) in [
            ("12'6\"", 150.0),
            ("12'-6-1/2\"", 150.5),
            ("12'6-1/2", 150.5),
            ("12'", 144.0),
            ("6\"", 6.0),
            ("1/2", 0.5),
            ("6-1/2", 6.5),
            ("-1'", -12.0),
            ("1.5'", 18.0),
            ("150", 150.0),
            (".5", 0.5),
        ] {
            assert_eq!(parse_length(t, 4.0), Some(want), "{t}");
        }
        assert_eq!(parse_length("12'", 2.0), None);
        assert_eq!(parse_length("abc", 4.0), None);
        assert_eq!(parse_length("1/0", 4.0), None);
        assert_eq!(parse_length("12'6'", 4.0), None);
        let en = |k: &str| (k == "units.area.imperial").then(|| "{sqin} sq in ({sqft} sq ft)".to_string());
        assert_eq!(format_area(22500.0, 4.0, Some(&json!(4)), &en), "22500 sq in (156.25 sq ft)");
    }
}
