//! The JavaScript value semantics the writer relies on (lib/dwg/write.js and the *-write.js modules work on plain
//! model objects): `Number.isFinite`, `x | 0`, `+x`, `String(x)`, truthiness, `??`, `hexKey`, `Object.entries`
//! order, `String.prototype.trim`, `parseInt(s, 16)` — so a ported line reads like the JS one and behaves the same.

use crate::util::{to_i32, u64_to_f64};
use crate::value::{Obj, V};

/// `Number.isFinite(v) ? v : d` (only a number counts: `true` is not finite in that sense).
pub fn num(v: Option<&V>, d: f64) -> f64 {
    match v {
        Some(V::Num(n)) if n.is_finite() => *n,
        _ => d,
    }
}

/// `v ?? x`: `None` when `v` is undefined or null.
pub fn nn(v: Option<&V>) -> Option<&V> {
    match v {
        None | Some(V::Null) => None,
        x => x,
    }
}

/// JavaScript truthiness (`undefined` is falsy).
pub fn truthy(v: Option<&V>) -> bool {
    v.is_some_and(V::truthy)
}

/// `v === undefined` (absent) — a `null` is not undefined.
pub fn undef(v: Option<&V>) -> bool {
    v.is_none()
}

/// The JS whitespace and line terminators (`\s`, `trim`).
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// `String.prototype.trim`.
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// JS StringToNumber.
pub fn str_to_number(s: &str) -> f64 {
    let t = js_trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let (neg, body) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let radix = |p: &str, r: u32| -> Option<f64> {
        let d = t.get(2..)?;
        if d.is_empty() || !t.starts_with(p) && !t.starts_with(&p.to_uppercase()) {
            return None;
        }
        let mut v = 0.0f64;
        for c in d.chars() {
            v = v * f64::from(r) + f64::from(c.to_digit(r)?);
        }
        Some(v)
    };
    if t.len() > 2 {
        for (p, r) in [("0x", 16), ("0o", 8), ("0b", 2)] {
            if t.get(..2).is_some_and(|h| h.eq_ignore_ascii_case(p)) {
                return radix(p, r).unwrap_or(f64::NAN);
            }
        }
    }
    if body == "Infinity" {
        return if neg { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    // a decimal literal: digits, one '.', an exponent; nothing else (Rust's parser also takes "inf" / "nan")
    let ok =
        body.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')) && body.chars().any(|c| c.is_ascii_digit());
    if !ok {
        return f64::NAN;
    }
    match body.parse::<f64>() {
        Ok(v) => {
            if neg {
                -v
            } else {
                v
            }
        }
        Err(_) => f64::NAN,
    }
}

/// JS ToNumber (`+v`); undefined → NaN.
pub fn to_number(v: Option<&V>) -> f64 {
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
        Some(V::Str(s)) => str_to_number(s),
        Some(V::Arr(_)) | Some(V::Bytes(_)) => str_to_number(&js_string(v)),
        Some(V::Obj(_)) => f64::NAN,
    }
}

/// `v | 0`.
pub fn int32(v: Option<&V>) -> i32 {
    to_i32(to_number(v))
}

/// `Math.trunc(+v) || 0` style integer of a value (NaN → 0).
pub fn int(v: Option<&V>) -> f64 {
    f64::from(int32(v))
}

/// JS `Number.prototype.toString()` (radix 10).
pub fn num_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if n == 0.0 {
        return "0".into();
    }
    let neg = n < 0.0;
    let a = n.abs();
    // shortest round-trip digits and the decimal exponent
    let e = format!("{a:e}");
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let k = i32::try_from(digits.len()).unwrap_or(1);
    let n10 = exp.saturating_add(1);
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    if k <= n10 && n10 <= 21 {
        out.push_str(&digits);
        for _ in 0..n10.saturating_sub(k) {
            out.push('0');
        }
    } else if 0 < n10 && n10 <= 21 {
        let cut = usize::try_from(n10).unwrap_or(0);
        out.push_str(digits.get(..cut).unwrap_or(""));
        out.push('.');
        out.push_str(digits.get(cut..).unwrap_or(""));
    } else if -6 < n10 && n10 <= 0 {
        out.push_str("0.");
        for _ in 0..n10.saturating_neg() {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        out.push_str(digits.get(..1).unwrap_or(""));
        if k > 1 {
            out.push('.');
            out.push_str(digits.get(1..).unwrap_or(""));
        }
        out.push('e');
        let x = n10.saturating_sub(1);
        if x >= 0 {
            out.push('+');
        }
        out.push_str(&x.to_string());
    }
    out
}

/// `String(v)` (undefined → "undefined").
pub fn js_string(v: Option<&V>) -> String {
    match v {
        None => "undefined".into(),
        Some(V::Null) => "null".into(),
        Some(V::Bool(b)) => b.to_string(),
        Some(V::Num(n)) => num_to_string(*n),
        Some(V::Str(s)) => s.clone(),
        Some(V::Arr(a)) => {
            let parts: Vec<String> = a.iter().map(|x| if matches!(x, V::Null) { String::new() } else { js_string(Some(x)) }).collect();
            parts.join(",")
        }
        Some(V::Bytes(b)) => {
            let parts: Vec<String> = b.iter().map(u8::to_string).collect();
            parts.join(",")
        }
        Some(V::Obj(_)) => "[object Object]".into(),
    }
}

/// `String(v ?? '')`.
pub fn str_or(v: Option<&V>, d: &str) -> String {
    match nn(v) {
        None => d.to_string(),
        x => js_string(x),
    }
}

/// V8's DoubleToRadixCString (`n.toString(radix)`), lowercase. Integers ≥ 2^53 print exactly.
pub fn radix_string(n: f64, radix: u32) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    let chars = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let r = f64::from(radix.clamp(2, 36));
    let neg = n < 0.0;
    let value = n.abs();
    let mut integer = value.floor();
    let mut fraction = value - integer;
    let next = f64::from_bits(value.to_bits().saturating_add(1));
    let mut delta = (0.5 * (next - value)).max(f64::from_bits(1));
    let mut frac: Vec<u8> = Vec::new();
    if fraction >= delta {
        loop {
            fraction *= r;
            delta *= r;
            let digit = crate::util::to_usize(fraction.trunc()).min(35);
            frac.push(chars.get(digit).copied().unwrap_or(b'0'));
            fraction -= crate::util::usize_to_f64(digit);
            if (fraction > 0.5 || (fraction == 0.5 && digit & 1 == 1)) && fraction + delta > 1.0 {
                // back trace the written digits for the carry
                loop {
                    let Some(c) = frac.pop() else {
                        integer += 1.0;
                        break;
                    };
                    let d = if c > b'9' { usize::from(c.wrapping_sub(b'a')).saturating_add(10) } else { usize::from(c.wrapping_sub(b'0')) };
                    if crate::util::usize_to_f64(d.saturating_add(1)) < r {
                        frac.push(chars.get(d.saturating_add(1)).copied().unwrap_or(b'0'));
                        break;
                    }
                }
                break;
            }
            if fraction < delta || frac.len() > 1100 {
                break;
            }
        }
    }
    let int_part = if radix == 16 { crate::util::hex_of(integer).to_lowercase() } else { format!("{integer:.0}") };
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    out.push_str(&int_part);
    if !frac.is_empty() {
        out.push('.');
        out.push_str(&String::from_utf8_lossy(&frac));
    }
    out
}

/// `n.toString(16).toUpperCase()` for an integer-valued number.
pub fn hex_upper(n: f64) -> String {
    crate::util::hex_of(n)
}

/// write.js hexKey: `String(typeof h === 'number' ? h.toString(16) : h).toUpperCase().replace(/^0+(?=.)/, '')`,
/// null for undefined/null.
pub fn hex_key(v: Option<&V>) -> Option<String> {
    let s = match v {
        None | Some(V::Null) => return None,
        Some(V::Num(n)) => radix_string(*n, 16),
        x => js_string(x),
    };
    let up = s.to_uppercase();
    let trimmed = up.trim_start_matches('0');
    Some(if trimmed.is_empty() && !up.is_empty() { "0".into() } else { trimmed.to_string() })
}

/// hexKey of a plain handle number.
pub fn hex_key_n(n: f64) -> String {
    hex_key(Some(&V::Num(n))).unwrap_or_default()
}

/// Is `k` an array index (JS orders such keys first, ascending)?
fn index_key(k: &str) -> Option<u32> {
    if k.is_empty() || (k.len() > 1 && k.starts_with('0')) || !k.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u64 = k.parse().ok()?;
    if n < 4_294_967_295 { u32::try_from(n).ok() } else { None }
}

/// `Object.entries(o)` in JS order: array-index keys ascending, then the others in insertion order.
pub fn js_entries(o: &Obj) -> Vec<(&str, &V)> {
    let mut idx: Vec<(u32, &str, &V)> = Vec::new();
    let mut rest: Vec<(&str, &V)> = Vec::new();
    for (k, v) in o.iter() {
        match index_key(k) {
            Some(n) => idx.push((n, k, v)),
            None => rest.push((k, v)),
        }
    }
    if idx.is_empty() {
        return rest;
    }
    idx.sort_by_key(|x| x.0);
    let mut out: Vec<(&str, &V)> = idx.into_iter().map(|(_, k, v)| (k, v)).collect();
    out.extend(rest);
    out
}

/// An object re-ordered as JS would print it (recursively), for report output.
pub fn js_order(v: V) -> V {
    match v {
        V::Obj(o) => {
            let has_idx = o.keys().any(|k| index_key(k).is_some());
            let mut items: Vec<(crate::value::Key, V)> = o.0.into_iter().map(|(k, v)| (k, js_order(v))).collect();
            if has_idx {
                let mut idx: Vec<(u32, (crate::value::Key, V))> = Vec::new();
                let mut rest = Vec::new();
                for it in items {
                    match index_key(&it.0) {
                        Some(n) => idx.push((n, it)),
                        None => rest.push(it),
                    }
                }
                idx.sort_by_key(|x| x.0);
                items = idx.into_iter().map(|x| x.1).collect();
                items.extend(rest);
            }
            V::Obj(Obj(items))
        }
        V::Arr(a) => V::Arr(a.into_iter().map(js_order).collect()),
        x => x,
    }
}

/// `parseInt(s, 16)` (NaN when no digit).
pub fn parse_int16(s: &str) -> f64 {
    let t = s.trim_start_matches(is_js_space);
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let t = if t.len() >= 2 && t.get(..2).is_some_and(|h| h.eq_ignore_ascii_case("0x")) { t.get(2..).unwrap_or("") } else { t };
    let mut v = 0.0f64;
    let mut any = false;
    for c in t.chars() {
        match c.to_digit(16) {
            Some(d) => {
                v = v * 16.0 + f64::from(d);
                any = true;
            }
            None => break,
        }
    }
    if !any {
        return f64::NAN;
    }
    if neg { -v } else { v }
}

/// `JSON.parse` → a model value (serde_json keeps the key order; JS integer keys are handled by [`js_entries`]).
pub fn from_json(v: &serde_json::Value, depth: usize) -> V {
    if depth > 512 {
        return V::Null;
    }
    match v {
        serde_json::Value::Null => V::Null,
        serde_json::Value::Bool(b) => V::Bool(*b),
        serde_json::Value::Number(n) => V::Num(n.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::String(s) => V::Str(s.clone()),
        serde_json::Value::Array(a) => V::Arr(a.iter().map(|x| from_json(x, depth.saturating_add(1))).collect()),
        serde_json::Value::Object(m) => {
            let mut o = Obj::with_capacity(m.len());
            for (k, x) in m {
                o.set(k.clone(), from_json(x, depth.saturating_add(1)));
            }
            V::Obj(o)
        }
    }
}

/// u64 → number (a JS number holding a handle).
pub fn h_num(n: u64) -> f64 {
    u64_to_f64(n)
}

/// `{x, y, z}` from a value with `num()` on each part (write.js P3: absent → origin).
pub fn p3_of(p: Option<&V>) -> V {
    match p {
        Some(V::Obj(o)) => crate::value::p3(num(o.get("x"), 0.0), num(o.get("y"), 0.0), num(o.get("z"), 0.0)),
        Some(x) if x.truthy() => crate::value::p3(0.0, 0.0, 0.0),
        _ => crate::value::p3(0.0, 0.0, 0.0),
    }
}

/// `p && fin(p.x, p.y)` (write.js okP).
pub fn ok_p(p: Option<&V>) -> bool {
    match p {
        Some(V::Obj(o)) => matches!(o.get("x"), Some(V::Num(x)) if x.is_finite()) && matches!(o.get("y"), Some(V::Num(y)) if y.is_finite()),
        _ => false,
    }
}

/// Field of an object value (`v?.k`; an array's or string's `length` is not a field here: see [`length_of`]).
pub fn field<'a>(v: Option<&'a V>, k: &str) -> Option<&'a V> {
    match v {
        Some(V::Obj(o)) => o.get(k),
        Some(V::Arr(a)) => index_key(k).and_then(|i| a.get(usize::try_from(i).unwrap_or(usize::MAX))),
        _ => None,
    }
}
/// `v?.length` (arrays and strings have one; JS counts a string in UTF-16 units).
pub fn length_of(v: Option<&V>) -> Option<V> {
    match v {
        Some(V::Arr(a)) => Some(V::from(a.len())),
        Some(V::Str(s)) => Some(V::from(s.encode_utf16().count())),
        Some(V::Bytes(b)) => Some(V::from(b.len())),
        x => field(x, "length").cloned(),
    }
}

/// Array items of a value (`v || []` where v should be an array).
pub fn arr(v: Option<&V>) -> &[V] {
    match v {
        Some(V::Arr(a)) => a,
        _ => &[],
    }
}

/// UTF-16 code units of a string (JS strings are UTF-16).
pub fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn js_semantics() {
        assert_eq!(num_to_string(1e21), "1e+21");
        assert_eq!(num_to_string(1.5e-7), "1.5e-7");
        assert_eq!(num_to_string(0.000001), "0.000001");
        assert_eq!(num_to_string(123.25), "123.25");
        assert_eq!(num_to_string(-42.0), "-42");
        assert_eq!(str_to_number(" 12 "), 12.0);
        assert!(str_to_number("inf").is_nan());
        assert_eq!(str_to_number("0x1F"), 31.0);
        assert_eq!(hex_key(Some(&V::from("00a1"))), Some("A1".into()));
        assert_eq!(hex_key(Some(&V::from("000"))), Some("0".into()));
        assert_eq!(hex_key(Some(&V::Num(255.0))), Some("FF".into()));
        assert_eq!(parse_int16("1fz"), 31.0);
        assert_eq!(radix_string(0.5, 16), "0.8");
        assert_eq!(radix_string(255.75, 16), "ff.c");
        assert_eq!(radix_string(-0.1, 16), "-0.1999999999999a");
        assert_eq!(hex_key(Some(&V::Num(0.5))), Some(".8".into()));
        assert!(parse_int16("zz").is_nan());
        let o = crate::obj! { "b" => 1, "10" => 2, "a" => 3, "2" => 4 };
        let k: Vec<&str> = js_entries(&o).into_iter().map(|x| x.0).collect();
        assert_eq!(k, vec!["2", "10", "b", "a"]);
        assert_eq!(int32(Some(&V::from("7"))), 7);
        assert_eq!(int32(None), 0);
    }
}
