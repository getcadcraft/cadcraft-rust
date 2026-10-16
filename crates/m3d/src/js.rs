//! JavaScript number and string semantics the kernel depends on: V8's `Math.hypot` (any arity), `Math.round`,
//! NaN-propagating `Math.min` / `Math.max`, `Math.sign`, truthiness, `Number.prototype.toString` / `toPrecision` /
//! `toString(36)`, `String.prototype.trim` / `slice` (UTF-16), and a stable sort driven by a JS comparator.
//! Copied from `cc-cad::js` where the same function exists there (m3d is layer 1 and may not depend on cc-cad).

/// V8's `Math.hypot(...xs)`: the Kahan-compensated sum of squares, scaled by the largest argument.
pub fn hypot(xs: &[f64]) -> f64 {
    let mut max = 0.0f64;
    let mut nan = false;
    for &x in xs {
        let v = x.abs();
        if v.is_nan() {
            nan = true;
        } else if v > max {
            max = v;
        }
    }
    if max == f64::INFINITY {
        return f64::INFINITY;
    }
    if nan {
        return f64::NAN;
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut comp = 0.0f64;
    for &x in xs {
        let n = x.abs() / max;
        let summand = n * n - comp;
        let pre = sum + summand;
        comp = (pre - sum) - summand;
        sum = pre;
    }
    sum.sqrt() * max
}
/// `Math.hypot(a, b, c)`.
#[inline]
pub fn hypot3(a: f64, b: f64, c: f64) -> f64 {
    hypot(&[a, b, c])
}
/// `Math.hypot(a, b)`.
#[inline]
pub fn hypot2(a: f64, b: f64) -> f64 {
    hypot(&[a, b])
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

/// `Math.max(a, b)` (NaN wins; +0 beats −0).
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a > b || (a == b && b.is_sign_negative()) {
        a
    } else {
        b
    }
}
/// `Math.min(a, b)` (NaN wins; −0 beats +0).
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a < b || (a == b && a.is_sign_negative()) {
        a
    } else {
        b
    }
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
/// JS truthiness of a number.
#[inline]
pub fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}
/// `x || d` for numbers.
#[inline]
pub fn or(x: f64, d: f64) -> f64 {
    if truthy(x) { x } else { d }
}
/// `Number.isInteger(x)`.
#[inline]
pub fn is_integer(x: f64) -> bool {
    x.is_finite() && x.trunc() == x
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

/// The decimal digits of an integer, appended to `out`.
#[inline]
fn push_int(out: &mut String, i: i64) {
    let mut u = i.unsigned_abs();
    let mut buf = [0u8; 21];
    let mut pos = buf.len();
    loop {
        pos = pos.saturating_sub(1);
        if let Some(b) = buf.get_mut(pos) {
            // a digit 0..9
            #[allow(clippy::cast_possible_truncation)]
            let d = (u % 10) as u8;
            *b = b'0' + d;
        }
        u /= 10;
        if u == 0 || pos <= 1 {
            break;
        }
    }
    if i < 0 {
        pos = pos.saturating_sub(1);
        if let Some(b) = buf.get_mut(pos) {
            *b = b'-';
        }
    }
    // ASCII digits (and a sign): always UTF-8
    out.push_str(std::str::from_utf8(buf.get(pos..).unwrap_or(&[])).unwrap_or(""));
}

/// m · 10^-k as positional decimal text (k ≥ 1; trailing zeros of the fraction dropped).
fn push_decimal(out: &mut String, m: i64, k: usize) {
    let mut digits = String::new();
    push_int(&mut digits, m.abs());
    if m < 0 {
        out.push('-');
    }
    let n = digits.len();
    let (ip, fp) = if n > k { digits.split_at(n - k) } else { ("", digits.as_str()) };
    if ip.is_empty() {
        out.push('0');
    } else {
        out.push_str(ip);
    }
    let fp_full: String = if n > k { fp.to_string() } else { "0".repeat(k - n) + fp };
    let fp_t = fp_full.trim_end_matches('0');
    if !fp_t.is_empty() {
        out.push('.');
        out.push_str(fp_t);
    }
}

/// `String(x)` for a number (ECMAScript Number::toString), appended to `out`.
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
    // the common case: a safe integer (the cast truncates; equal back means whole)
    if x.abs() < 9_007_199_254_740_992.0 {
        // in range by the check above
        #[allow(clippy::cast_possible_truncation)]
        let i = x as i64;
        #[allow(clippy::cast_precision_loss)]
        if i as f64 == x {
            push_int(out, i);
            return;
        }
    }
    // a short decimal (most coordinates): |x| < 1e6 with at most 6 decimals. If m = x·10^k is whole and m / 10^k
    // gives x back (a correctly rounded division: that is what parsing "m e-k" gives), m e-k is the shortest
    // round-trip text: at this size the doubles are far closer than 10^-6, so no shorter decimal reads as x
    if (1e-6..1e6).contains(&x.abs()) {
        const P10: [f64; 7] = [1.0, 10.0, 100.0, 1000.0, 10000.0, 100000.0, 1000000.0];
        for (k, &p) in P10.iter().enumerate().skip(1) {
            let s = x * p;
            // in range: |s| < 1e12 (the cast truncates; equal back means whole)
            #[allow(clippy::cast_possible_truncation)]
            let m = s as i64;
            #[allow(clippy::cast_precision_loss)]
            let whole = m as f64 == s;
            if whole && s / p == x {
                push_decimal(out, m, k);
                return;
            }
        }
    }
    // 1e-6 ≤ |x| < 1e21: Number::toString writes the shortest round-trip digits without an exponent, as Display does
    if (1e-6..1e21).contains(&x.abs()) {
        use std::fmt::Write as _;
        let _ = write!(out, "{x}");
        return;
    }
    if x < 0.0 {
        out.push('-');
    }
    let (d, n) = shortest(x.abs());
    let k = i64::try_from(d.len()).unwrap_or(0);
    if k <= n && n <= 21 {
        push_digits(out, &d);
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
        push_digits(out, &d);
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
/// `String(x)` for a number.
pub fn num_str(x: f64) -> String {
    let mut s = String::new();
    push_num(&mut s, x);
    s
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
        return num_str(x);
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

/// `x.toPrecision(p)` (1 ≤ p ≤ 100): the exact value rounded half up to p significant digits.
pub fn to_precision(x: f64, p: usize) -> String {
    if !x.is_finite() {
        return num_str(x);
    }
    let p = p.clamp(1, 100);
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    let (digits, e) = if x == 0.0 {
        (vec![b'0'; p], 0i64)
    } else {
        let s = format!("{:.1100e}", x.abs());
        let (mant, exp) = s.split_once('e').unwrap_or((s.as_str(), "0"));
        let all: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).collect();
        let mut e: i64 = exp.parse().unwrap_or(0);
        let mut d: Vec<u8> = all.iter().copied().take(p).collect();
        let next = all.get(p).copied().unwrap_or(b'0');
        if next >= b'5' && bump(&mut d) {
            d.insert(0, b'1');
            d.truncate(p);
            e = e.saturating_add(1);
        }
        (d, e)
    };
    let pi = i64::try_from(p).unwrap_or(1);
    if e < -6 || e >= pi {
        push_digits(&mut out, digits.get(..1).unwrap_or(&[]));
        if p > 1 {
            out.push('.');
            push_digits(&mut out, digits.get(1..).unwrap_or(&[]));
        }
        out.push('e');
        out.push(if e < 0 { '-' } else { '+' });
        out.push_str(&e.unsigned_abs().to_string());
        return out;
    }
    if e == pi.saturating_sub(1) {
        push_digits(&mut out, &digits);
        return out;
    }
    if e >= 0 {
        let k = usize::try_from(e.saturating_add(1)).unwrap_or(0);
        push_digits(&mut out, digits.get(..k).unwrap_or(&[]));
        out.push('.');
        push_digits(&mut out, digits.get(k..).unwrap_or(&[]));
        return out;
    }
    out.push_str("0.");
    for _ in 0..e.saturating_add(1).unsigned_abs() {
        out.push('0');
    }
    push_digits(&mut out, &digits);
    out
}

/// `n.toString(36)` for a non-negative integer.
pub fn radix36(mut n: u64) -> String {
    const D: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while n > 0 {
        let k = usize::try_from(n % 36).unwrap_or(0);
        out.push(D.get(k).copied().unwrap_or(b'0'));
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// JS `WhiteSpace` + `LineTerminator` (what `trim` removes).
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

/// `s.slice(0, n)` in UTF-16 code units. Spec gap (JS-engine quirk): where the JS would cut a surrogate pair in
/// half (leaving a lone surrogate a Rust string cannot hold), the pair is dropped whole.
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

/// `cleanText(s, max)` for a string: control characters → spaces, trimmed, at most `max` UTF-16 units.
pub fn clean_text(s: &str, max: usize) -> String {
    let r: String = s.chars().map(|c| if (c as u32) < 0x20 || c == '\u{7f}' { ' ' } else { c }).collect();
    slice16(trim(&r), max)
}

/// The order a JS comparator result gives (`< 0` before, `> 0` after, `0` or NaN: keep).
#[inline]
pub fn order(c: f64) -> std::cmp::Ordering {
    if c < 0.0 {
        std::cmp::Ordering::Less
    } else if c > 0.0 {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

/// `list.sort(cmp)` exactly as V8 sorts (TimSort, [`crate::sort`]): the same order also when the comparator is not
/// consistent (NaN coordinates).
pub fn sort_by<T: Clone>(list: &mut [T], cmp: impl FnMut(&T, &T) -> f64) {
    crate::sort::timsort(list, cmp);
}

/// `ids.slice().sort().join()` for numbers: the default sort (by their strings), joined with commas.
pub fn sorted_join(ids: &[f64]) -> String {
    let mut s: Vec<String> = ids.iter().map(|&x| num_str(x)).collect();
    s.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    s.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn number_strings() {
        assert_eq!(num_str(150.5), "150.5");
        assert_eq!(num_str(1e21), "1e+21");
        assert_eq!(num_str(1e-7), "1e-7");
        assert_eq!(num_str(0.000001), "0.000001");
        assert_eq!(num_str(-12.0), "-12");
        assert_eq!(num_str(-0.0), "0");
        assert_eq!(num_str(123456789012345680000.0), "123456789012345680000");
        assert_eq!(num_str(1.5e-10), "1.5e-10");
        assert_eq!(num_str(-2.5), "-2.5");
        assert_eq!(num_str(0.25), "0.25");
        assert_eq!(num_str(0.000123), "0.000123");
        assert_eq!(num_str(123456.75), "123456.75");
        assert_eq!(num_str(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(num_str(1.0 / 3.0), "0.3333333333333333");
        // the fast path agrees with the shortest round-trip text everywhere it applies
        let mut seed = 12345u64;
        for _ in 0..200_000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let r = (seed >> 11) as f64 / (1u64 << 53) as f64;
            let k = (seed % 7) as i32;
            let x = ((r * 2e6 - 1e6) * 10f64.powi(k)).round() / 10f64.powi(k);
            let mut fast = String::new();
            push_num(&mut fast, x);
            let slow = if x == 0.0 { "0".to_string() } else { format!("{x}") };
            assert_eq!(fast, slow, "x = {x:e}");
        }
    }
    #[test]
    fn precision() {
        assert_eq!(to_precision(1234567.5, 7), "1234568");
        assert_eq!(to_precision(0.5, 7), "0.5000000");
        assert_eq!(to_precision(1e-7, 7), "1.000000e-7");
        assert_eq!(to_precision(12345678.0, 7), "1.234568e+7");
        assert_eq!(to_precision(0.00012345678, 7), "0.0001234568");
        assert_eq!(to_precision(-3.0, 7), "-3.000000");
        assert_eq!(to_precision(0.0, 7), "0.000000");
    }
    #[test]
    fn hypot_and_round() {
        assert_eq!(hypot3(3.0, 4.0, 12.0), 13.0);
        assert_eq!(hypot2(3.0, 4.0), 5.0);
        assert!(hypot3(f64::NAN, 1.0, 0.0).is_nan());
        assert_eq!(hypot3(f64::NAN, f64::INFINITY, 0.0), f64::INFINITY);
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(radix36(35), "z");
        assert_eq!(radix36(36 * 36), "100");
    }
    #[test]
    fn default_sort() {
        assert_eq!(sorted_join(&[10.0, 9.0, 1.0]), "1,10,9");
    }
}
