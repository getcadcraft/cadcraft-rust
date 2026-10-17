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

/// The shortest round-trip digits of a finite positive number (what `Number.prototype.toString` uses), on the stack:
/// x = 0.d1d2… × 10^n.
#[derive(Clone, Copy, Debug)]
pub struct Digits {
    buf: [u8; 24],
    len: usize,
    /// the decimal exponent n
    pub n: i64,
}

impl Digits {
    /// The digits, no leading zeros.
    pub fn digits(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }
    /// Digits given as ASCII (at most 24, no leading zeros) and the exponent n; None if they do not fit.
    pub fn from_parts(d: &[u8], n: i64) -> Option<Self> {
        let mut out = Digits { buf: [0; 24], len: d.len(), n };
        out.buf.get_mut(..d.len())?.copy_from_slice(d);
        Some(out)
    }
}

/// A fixed buffer `fmt::Write` (formatting a float's `{:e}` without a heap allocation).
struct StackW {
    buf: [u8; 48],
    len: usize,
}

impl std::fmt::Write for StackW {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(std::fmt::Error)?;
        self.buf.get_mut(self.len..end).ok_or(std::fmt::Error)?.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// [`Digits`] of a finite positive x (Rust's shortest `{:e}`; the same digits V8 prints).
pub fn shortest_digits(x: f64) -> Digits {
    shortest_digits_with(x, true)
}

/// `quick`: skip the exact tie check when the binary value cannot be a tie (see `even_tie`).
fn shortest_digits_with(x: f64, quick: bool) -> Digits {
    use std::fmt::Write as _;
    let mut w = StackW { buf: [0; 48], len: 0 };
    let mut d = Digits { buf: [0; 24], len: 0, n: 1 };
    if write!(w, "{x:e}").is_err() {
        return d;
    }
    let text = w.buf.get(..w.len).unwrap_or(&[]);
    let (mant, exp) = match text.iter().position(|&c| c == b'e') {
        Some(i) => (text.get(..i).unwrap_or(&[]), text.get(i.saturating_add(1)..).unwrap_or(&[])),
        None => (text, &b"0"[..]),
    };
    for &c in mant {
        if c.is_ascii_digit()
            && let Some(slot) = d.buf.get_mut(d.len)
        {
            *slot = c;
            d.len = d.len.saturating_add(1);
        }
    }
    let e: i64 = std::str::from_utf8(exp).ok().and_then(|t| t.parse().ok()).unwrap_or(0);
    d.n = e.saturating_add(1);
    even_tie(x, &mut d, quick);
    d
}

/// ECMAScript Number::toString picks, of two shortest digit strings equally close to x, the even one (V8 does);
/// Rust's shortest form takes the upper. Ties happen when x sits exactly halfway, e.g. f32 values such as
/// 248.032257080078125 → "248.03225708007812".
fn even_tie(x: f64, d: &mut Digits, quick: bool) {
    use std::fmt::Write as _;
    let k = d.len;
    let km1 = k.saturating_sub(1);
    let Some(&last) = d.buf.get(k.wrapping_sub(1)) else { return };
    // two different strings of 15 digits or fewer never name the same double, so ties need 16 or 17
    if k < 16 || last % 2 == 0 {
        return;
    }
    // A tie needs x's exact decimal expansion to end one digit past the k shown: x = m·2^e2 (m odd) has -e2 fractional
    // digits, so when those reach past k + 1 significant digits there is no tie; this skips the exact expansion below
    // for nearly every number (only short binary fractions, e.g. f32 values, and big integers go on)
    let bits = x.to_bits();
    let bexp = i64::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
    let mut m = bits & ((1u64 << 52) - 1);
    let e = if bexp == 0 {
        -1074
    } else {
        m |= 1u64 << 52;
        bexp.saturating_sub(1075)
    };
    if m == 0 {
        return;
    }
    let e2 = e.saturating_add(i64::from(m.trailing_zeros()));
    let lead_zeros = 0i64.saturating_sub(d.n).max(0);
    let k_i = i64::try_from(k).unwrap_or(i64::MAX);
    if quick && e2 < 0 && 0i64.saturating_sub(e2).saturating_sub(lead_zeros) > k_i.saturating_add(1) {
        return;
    }
    // x's exact decimal digits (every f64 has a finite expansion; 800 places is past the longest)
    let mut exact = String::new();
    if write!(exact, "{x:.800e}").is_err() {
        return;
    }
    let mant: Vec<u8> = exact.split('e').next().unwrap_or("").bytes().filter(u8::is_ascii_digit).collect();
    let same_exp = exact.split('e').nth(1).and_then(|t| t.parse::<i64>().ok()) == Some(d.n.saturating_sub(1));
    if !same_exp || mant.get(..km1) != d.buf.get(..km1) {
        return;
    }
    let half_zero = mant.get(k) == Some(&b'5') && mant.get(k.saturating_add(1)..).is_some_and(|r| r.iter().all(|&c| c == b'0'));
    if !half_zero {
        return;
    }
    let e_last = mant.get(km1).copied().unwrap_or(0);
    if e_last == last.saturating_sub(1) {
        // exact = (last − 1).5: the lower neighbour is as close and even
        if let Some(slot) = d.buf.get_mut(km1) {
            *slot = last.saturating_sub(1);
        }
    } else if e_last == last && last < b'9' {
        // exact = last.5: the upper neighbour is as close and even
        if let Some(slot) = d.buf.get_mut(km1) {
            *slot = last.saturating_add(1);
        }
    }
}

fn push_digits(out: &mut String, d: &[u8]) {
    for &c in d {
        out.push(char::from(c));
    }
}

/// `String(x)` for a number (ECMAScript Number::toString).
pub fn num_to_string(x: f64) -> String {
    let mut out = String::new();
    push_num(&mut out, x);
    out
}

/// `String(x)` appended to `out`.
pub fn push_num(out: &mut String, x: f64) {
    if x.is_nan() {
        out.push_str("NaN");
        return;
    }
    if x == 0.0 {
        out.push('0');
        return;
    }
    if x.is_infinite() {
        out.push_str(if x > 0.0 { "Infinity" } else { "-Infinity" });
        return;
    }
    push_digits_js(out, x < 0.0, &shortest_digits(x.abs()));
}

/// `String(x)` from x's sign and shortest digits.
pub fn push_digits_js(out: &mut String, neg: bool, dg: &Digits) {
    if neg {
        out.push('-');
    }
    let d = dg.digits();
    let n = dg.n;
    let k = i64::try_from(d.len()).unwrap_or(0);
    if k <= n && n <= 21 {
        push_digits(out, d);
        for _ in 0..n.saturating_sub(k) {
            out.push('0');
        }
    } else if 0 < n && n <= 21 {
        let nu = usize::try_from(n).unwrap_or(0);
        push_digits(out, d.get(..nu).unwrap_or(&[]));
        out.push('.');
        push_digits(out, d.get(nu..).unwrap_or(&[]));
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        for _ in 0..n.saturating_neg() {
            out.push('0');
        }
        push_digits(out, d);
    } else {
        let e = n.saturating_sub(1);
        push_digits(out, d.get(..1).unwrap_or(&[]));
        if d.len() > 1 {
            out.push('.');
            push_digits(out, d.get(1..).unwrap_or(&[]));
        }
        out.push('e');
        out.push(if e < 0 { '-' } else { '+' });
        out.push_str(&e.unsigned_abs().to_string());
    }
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

    /// The quick test in `even_tie` never skips a real tie: on f32 values (where ties live), short binary fractions,
    /// big integers and random doubles, the digits equal those of the full exact check.
    #[test]
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::arithmetic_side_effects)]
    fn the_quick_tie_test_skips_no_tie() {
        let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let mut ties = 0;
        for i in 0..300_000u32 {
            let r = next();
            let x = match i % 4 {
                0 => f64::from(f32::from_bits((r as u32) & 0x7f7f_ffff)),
                1 => (r >> 11) as f64 / (1u64 << (r % 60)) as f64,
                2 => ((r >> 8) | 1) as f64,
                _ => f64::from_bits(r & 0x7fef_ffff_ffff_ffff),
            };
            if !(x.is_finite() && x > 0.0) {
                continue;
            }
            let (a, b) = (shortest_digits_with(x, true), shortest_digits_with(x, false));
            assert_eq!((a.buf, a.len, a.n), (b.buf, b.len, b.n), "{x:e}");
            let mut plain = Digits { buf: [0; 24], len: 0, n: 1 };
            let s = format!("{x:e}");
            let mant = s.split('e').next().unwrap_or("");
            for c in mant.bytes().filter(u8::is_ascii_digit) {
                plain.buf[plain.len] = c;
                plain.len += 1;
            }
            if plain.buf[..plain.len] != a.buf[..a.len] {
                ties += 1;
            }
        }
        // the f32 case has many ties: the check is exercised, not vacuous
        assert!(ties > 100, "{ties} ties");
    }
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
