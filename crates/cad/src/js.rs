//! JavaScript number and string semantics the ported modules depend on: V8's `Math.hypot`, `Math.round`,
//! NaN-propagating `Math.min` / `Math.max`, `Number.prototype.toString` / `toFixed` / `toExponential`, `+v`
//! (ToNumber), `String(v)`, truthiness, and `trim` over JS white space. Exact to the JS engine's output wherever the
//! spec fixes it.

use serde_json::Value;

/// V8's `Math.hypot(a, b)`: the Kahan-compensated sum of squares, scaled by the larger argument.
pub fn hypot(a: f64, b: f64) -> f64 {
    let (a, b) = (a.abs(), b.abs());
    if a.is_infinite() || b.is_infinite() {
        return f64::INFINITY;
    }
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    let max = if a > b { a } else { b };
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut comp = 0.0f64;
    for v in [a, b] {
        let n = v / max;
        let summand = n * n - comp;
        let pre = sum + summand;
        comp = (pre - sum) - summand;
        sum = pre;
    }
    sum.sqrt() * max
}

/// `Math.round`: the nearest integer, ties toward +∞ (−0 kept for −0.5 ≤ x < 0).
pub fn round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let r = x.floor();
    let v = if x - r >= 0.5 { r + 1.0 } else { r };
    if v == 0.0 && x < 0.0 { -0.0 } else { v }
}

/// `Math.max(a, b)` (NaN wins).
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a > b || (a == b && b.is_sign_negative()) {
        a
    } else {
        b
    }
}
/// `Math.min(a, b)` (NaN wins).
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a < b || (a == b && a.is_sign_negative()) {
        a
    } else {
        b
    }
}
/// `Math.max(...list)` (−∞ for an empty list).
pub fn max_of(list: impl IntoIterator<Item = f64>) -> f64 {
    list.into_iter().fold(f64::NEG_INFINITY, max)
}
/// `Math.min(...list)` (+∞ for an empty list).
pub fn min_of(list: impl IntoIterator<Item = f64>) -> f64 {
    list.into_iter().fold(f64::INFINITY, min)
}
/// `Math.sign`.
pub fn sign(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 {
        x
    } else if x > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// JS truthiness of a number (`x || d` keeps x when it is not 0 or NaN).
pub fn or(x: Option<f64>, d: f64) -> f64 {
    match x {
        Some(v) if v != 0.0 && !v.is_nan() => v,
        _ => d,
    }
}

/// `x | 0` (ToInt32) as a float.
pub fn int32(x: f64) -> f64 {
    if !x.is_finite() {
        return 0.0;
    }
    let t = x.trunc();
    let m = t.rem_euclid(4_294_967_296.0);
    if m >= 2_147_483_648.0 { m - 4_294_967_296.0 } else { m }
}

/// The digits (no leading zeros) and the decimal exponent n of a finite positive x: x = 0.d1d2… × 10^n, shortest
/// round-trip digits (what `Number.prototype.toString` uses).
fn shortest(x: f64) -> (Vec<u8>, i64) {
    let s = format!("{x:e}");
    let (mant, exp) = s.split_once('e').unwrap_or((s.as_str(), "0"));
    let digits: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).collect();
    let e: i64 = exp.parse().unwrap_or(0);
    (digits, e.saturating_add(1))
}

fn push_digits(out: &mut String, d: &[u8]) {
    for &c in d {
        out.push(char::from(c));
    }
}

/// `String(x)` for a number (ECMAScript Number::toString).
pub fn num_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    let (d, n) = shortest(x.abs());
    let k = i64::try_from(d.len()).unwrap_or(0);
    if k <= n && n <= 21 {
        push_digits(&mut out, &d);
        for _ in 0..n.saturating_sub(k) {
            out.push('0');
        }
    } else if 0 < n && n <= 21 {
        let nu = usize::try_from(n).unwrap_or(0);
        push_digits(&mut out, d.get(..nu).unwrap_or(&[]));
        out.push('.');
        push_digits(&mut out, d.get(nu..).unwrap_or(&[]));
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        for _ in 0..n.saturating_neg() {
            out.push('0');
        }
        push_digits(&mut out, &d);
    } else {
        let e = n.saturating_sub(1);
        push_digits(&mut out, d.get(..1).unwrap_or(&[]));
        if d.len() > 1 {
            out.push('.');
            push_digits(&mut out, d.get(1..).unwrap_or(&[]));
        }
        out.push('e');
        out.push(if e < 0 { '-' } else { '+' });
        out.push_str(&e.unsigned_abs().to_string());
    }
    out
}

/// Add one to a run of ASCII digits (true = it carried out of the first digit).
fn bump(d: &mut [u8]) -> bool {
    for c in d.iter_mut().rev() {
        if *c == b'9' {
            *c = b'0';
        } else {
            *c = c.saturating_add(1);
            return false;
        }
    }
    true
}

/// `x.toFixed(f)` (0 ≤ f ≤ 100): the exact value rounded half up, as the spec says.
pub fn to_fixed(x: f64, f: usize) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.abs() >= 1e21 {
        return num_to_string(x);
    }
    let neg = x < 0.0;
    // the exact decimal expansion (a double has at most 1074 digits after the point)
    let s = format!("{:.1100}", x.abs());
    let (ip, fp) = s.split_once('.').unwrap_or((s.as_str(), ""));
    let mut digits: Vec<u8> = ip.bytes().chain(fp.bytes().take(f)).collect();
    let next = fp.as_bytes().get(f).copied().unwrap_or(b'0');
    let mut int_len = ip.len();
    if next >= b'5' && bump(&mut digits) {
        digits.insert(0, b'1');
        int_len = int_len.saturating_add(1);
    }
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    push_digits(&mut out, digits.get(..int_len).unwrap_or(&[]));
    if f > 0 {
        out.push('.');
        push_digits(&mut out, digits.get(int_len..).unwrap_or(&[]));
    }
    out
}

/// `x.toExponential(f)`: f + 1 significant digits, the exact value rounded half up.
pub fn to_exponential(x: f64, f: usize) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    let (mut digits, mut e) = if x == 0.0 {
        (vec![b'0'; f.saturating_add(1)], 0i64)
    } else {
        let s = format!("{:.1100e}", x.abs());
        let (mant, exp) = s.split_once('e').unwrap_or((s.as_str(), "0"));
        let all: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).collect();
        let e: i64 = exp.parse().unwrap_or(0);
        let mut d: Vec<u8> = all.iter().copied().take(f.saturating_add(1)).collect();
        let next = all.get(f.saturating_add(1)).copied().unwrap_or(b'0');
        let mut e = e;
        if next >= b'5' && bump(&mut d) {
            d.insert(0, b'1');
            d.truncate(f.saturating_add(1));
            e = e.saturating_add(1);
        }
        (d, e)
    };
    if digits.is_empty() {
        digits.push(b'0');
    }
    push_digits(&mut out, digits.get(..1).unwrap_or(&[]));
    if f > 0 {
        out.push('.');
        push_digits(&mut out, digits.get(1..).unwrap_or(&[]));
    }
    out.push('e');
    out.push(if e < 0 { '-' } else { '+' });
    if e < 0 {
        e = e.saturating_neg();
    }
    out.push_str(&e.to_string());
    out
}

/// JS `WhiteSpace` + `LineTerminator` (what `trim` removes and `\s` matches).
pub fn is_ws(c: char) -> bool {
    matches!(
        c,
        '\u{09}'..='\u{0D}'
            | ' '
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}
/// `s.trim()`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_ws)
}
/// `s.trimEnd()`.
pub fn trim_end(s: &str) -> &str {
    s.trim_end_matches(is_ws)
}
/// `s.trimStart()`.
pub fn trim_start(s: &str) -> &str {
    s.trim_start_matches(is_ws)
}

/// `s.slice(0, n)` in UTF-16 code units (a pair cut in half is dropped whole).
pub fn slice16(s: &str, n: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for c in s.chars() {
        let w = c.len_utf16();
        if used.saturating_add(w) > n {
            break;
        }
        used = used.saturating_add(w);
        out.push(c);
    }
    out
}

/// StringToNumber (`+s`): trimmed; '' → 0; decimal literals, Infinity, 0x / 0o / 0b integers; else NaN.
pub fn str_to_number(s: &str) -> f64 {
    let t = trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let radix = |p: &str, r: u32| -> f64 {
        if p.is_empty() || !p.chars().all(|c| c.is_digit(r)) {
            return f64::NAN;
        }
        p.chars().fold(0.0, |acc, c| acc * f64::from(r) + f64::from(c.to_digit(r).unwrap_or(0)))
    };
    let lower = t.get(..2).map(str::to_ascii_lowercase);
    match lower.as_deref() {
        Some("0x") => return radix(t.get(2..).unwrap_or(""), 16),
        Some("0o") => return radix(t.get(2..).unwrap_or(""), 8),
        Some("0b") => return radix(t.get(2..).unwrap_or(""), 2),
        _ => {}
    }
    let (sgn, body) = match t.as_bytes().first() {
        Some(b'-') => (-1.0, t.get(1..).unwrap_or("")),
        Some(b'+') => (1.0, t.get(1..).unwrap_or("")),
        _ => (1.0, t),
    };
    if body == "Infinity" {
        return sgn * f64::INFINITY;
    }
    // StrUnsignedDecimalLiteral: digits [. digits] [e[+-]digits] | . digits [exp]
    let b = body.as_bytes();
    let mut i = 0usize;
    let mut int_digits = 0usize;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i = i.saturating_add(1);
        int_digits = int_digits.saturating_add(1);
    }
    let mut frac_digits = 0usize;
    if b.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i = i.saturating_add(1);
            frac_digits = frac_digits.saturating_add(1);
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return f64::NAN;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i.saturating_add(1);
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j = j.saturating_add(1);
        }
        let start = j;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j = j.saturating_add(1);
        }
        if j == start {
            return f64::NAN;
        }
        i = j;
    }
    if i != b.len() {
        return f64::NAN;
    }
    let mut lit = String::from(body);
    if lit.starts_with('.') {
        lit.insert(0, '0');
    }
    if lit.ends_with('.') {
        lit.push('0');
    }
    lit = lit.replace(".e", ".0e").replace(".E", ".0E");
    sgn * lit.parse::<f64>().unwrap_or(f64::NAN)
}

/// `+v` (ToNumber) for a JSON value.
pub fn to_number(v: &Value) -> f64 {
    match v {
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => str_to_number(s),
        Value::Array(a) => match a.as_slice() {
            [] => 0.0,
            [x] => str_to_number(&to_js_string(x)),
            _ => f64::NAN,
        },
        Value::Object(_) => f64::NAN,
    }
}
/// `+v` for a possibly absent value (`+undefined` = NaN).
pub fn to_number_opt(v: Option<&Value>) -> f64 {
    v.map_or(f64::NAN, to_number)
}

/// `String(v)`.
pub fn to_js_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => num_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(|x| if x.is_null() { String::new() } else { to_js_string(x) }).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// JS truthiness of a JSON value (absent = falsy).
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// `Number.isFinite(v)` for a JSON value.
pub fn finite(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn number_strings() {
        assert_eq!(num_to_string(150.5), "150.5");
        assert_eq!(num_to_string(1e21), "1e+21");
        assert_eq!(num_to_string(1e-7), "1e-7");
        assert_eq!(num_to_string(0.000001), "0.000001");
        assert_eq!(num_to_string(-12.0), "-12");
        assert_eq!(num_to_string(123456789012345680000.0), "123456789012345680000");
        assert_eq!(num_to_string(1.5e-10), "1.5e-10");
    }
    #[test]
    fn fixed_and_exp() {
        assert_eq!(to_fixed(0.5, 0), "1");
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(1.125, 2), "1.13");
        assert_eq!(to_fixed(-0.001, 2), "-0.00");
        assert_eq!(to_fixed(-0.0, 2), "0.00");
        assert_eq!(to_fixed(9.995, 2), "9.99");
        assert_eq!(to_fixed(99.5, 0), "100");
        assert_eq!(to_exponential(150.5, 4), "1.5050e+2");
        assert_eq!(to_exponential(0.0, 2), "0.00e+0");
        assert_eq!(to_exponential(9.99995, 4), "1.0000e+1");
        assert_eq!(to_exponential(1.25e-5, 1), "1.3e-5");
    }
    #[test]
    fn rounding() {
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(0.49999999999999994), 0.0);
        assert_eq!(hypot(3.0, 4.0), 5.0);
    }
    #[test]
    fn to_numbers() {
        assert_eq!(str_to_number(" 12 "), 12.0);
        assert_eq!(str_to_number(".5"), 0.5);
        assert_eq!(str_to_number("5."), 5.0);
        assert!(str_to_number("1/2").is_nan());
        assert_eq!(str_to_number("1e3"), 1000.0);
        assert_eq!(str_to_number("0x1F"), 31.0);
    }
}
