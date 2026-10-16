//! V8's number text and number parsing, as the exporters and readers use them: `toFixed`, `toPrecision`,
//! `toExponential`, `String(x)`, `parseFloat`, `parseInt(s, 10)`, `Number(s)`.
//!
//! `toFixed` / `toPrecision` round the exact binary value half up (ECMAScript: "if there are two such n, pick the
//! larger"). cc-m3d's `js::to_fixed` gets that by writing 1100 digits; the exporters call it millions of times, so
//! these use Rust's exact formatting with 25 extra digits and round those themselves. 25 extra digits always decide:
//! a double that is not exactly a tie lies at least about 10^-27 (relative) from it, far inside 25 digits past the
//! last kept one (checked against `js::to_fixed` / `js::to_precision` in the tests below).

use cc_m3d::js;

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

const EXTRA: usize = 25;

/// The shortest round-trip digits of a finite ax ≥ 0 (Rust's `{:e}`: shortest, and the closest of the shortest) and
/// the decimal exponent of the first digit.
fn shortest(ax: f64) -> (Vec<u8>, i64) {
    use std::fmt::Write;
    let mut s = Buf::default();
    let _ = write!(s, "{ax:e}");
    let t = s.as_str();
    let (mant, exp) = t.split_once('e').unwrap_or((t, "0"));
    (mant.bytes().filter(u8::is_ascii_digit).collect(), exp.parse().unwrap_or(0))
}

/// A small stack buffer for one number's text (`{:e}` of an f64 is at most 24 bytes; 32 leaves room).
#[derive(Default)]
struct Buf {
    b: [u8; 32],
    n: usize,
}
impl Buf {
    fn as_str(&self) -> &str {
        self.b.get(..self.n).and_then(|b| std::str::from_utf8(b).ok()).unwrap_or("")
    }
}
impl std::fmt::Write for Buf {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let end = self.n.checked_add(s.len()).ok_or(std::fmt::Error)?;
        self.b.get_mut(self.n..end).ok_or(std::fmt::Error)?.copy_from_slice(s.as_bytes());
        self.n = end;
        Ok(())
    }
}

/// One unit in the last place of a finite ax ≥ 0.
fn ulp(ax: f64) -> f64 {
    f64::from_bits(ax.to_bits().saturating_add(1)) - ax
}

/// The decimal digit of the shortest digits `d` (first digit at exponent `e`) at place value 10^q.
fn digit_at(d: &[u8], e: i64, q: i64) -> u8 {
    usize::try_from(e.saturating_sub(q)).ok().and_then(|i| d.get(i)).copied().unwrap_or(b'0')
}

/// The fast way to the exact rounding, from the shortest digits S of x (|x − S| ≤ ulp/2), or None when only exact
/// digits can decide. Kept digits end at place 10^low:
/// - S ends at or above 10^low, and ulp(x) is well under the grid step 10^low: the grid point nearest x is S itself.
/// - S ends at least two places below 10^low: no number with fewer digits than S (the grid points and the half-way
///   points between them) lies between x and S (it would be inside x's rounding interval and shorter than S), so
///   rounding S rounds x; and S's tail past 10^low is not exactly a half (S's last digit is not 0).
/// - S ends one place below 10^low with a last digit other than 5: the half-way point has as many digits as S, so
///   if it lay between x and S it would be a shortest candidate closer to x than S, and S is the closest one.
/// - otherwise (S ends in a 5 one place below 10^low, or ulp too coarse): None.
fn round_plan(d: &[u8], e: i64, low: i64, ax: f64) -> Option<bool> {
    let n = i64::try_from(d.len()).ok()?;
    let last = e.saturating_sub(n).saturating_add(1);
    if last >= low {
        let step = 10f64.powi(i32::try_from(low).ok()?);
        return (ulp(ax) < step * 0.25).then_some(false);
    }
    (last <= low.saturating_sub(2) || d.last() != Some(&b'5')).then_some(true)
}

/// `fixed`'s digits (integer part then f decimals), the next digit, and the integer length, from the shortest digits.
fn fixed_fast(ax: f64, f: usize) -> Option<(Vec<u8>, u8, usize)> {
    let (d, e) = shortest(ax);
    let low = 0i64.checked_sub(i64::try_from(f).ok()?)?;
    let round = round_plan(&d, e, low, ax)?;
    let top = e.max(0);
    let mut digits = Vec::with_capacity(usize::try_from(top.saturating_sub(low).saturating_add(1)).ok()?);
    let mut q = top;
    while q >= low {
        digits.push(digit_at(&d, e, q));
        q = q.saturating_sub(1);
    }
    let next = if round { digit_at(&d, e, low.saturating_sub(1)) } else { b'0' };
    Some((digits, next, usize::try_from(top.saturating_add(1)).ok()?))
}

/// `sig_digits`' p digits, the next digit and the exponent, from the shortest digits. The grid below 10^e is ten
/// times finer than above it, so the ulp test uses the finer step (low − 1).
fn precision_fast(ax: f64, p: usize) -> Option<(Vec<u8>, u8, i64)> {
    if ax == 0.0 || !ax.is_finite() {
        return None;
    }
    let (d, e) = shortest(ax);
    let pi = i64::try_from(p).ok()?;
    let low = e.saturating_sub(pi).saturating_add(1);
    let n = i64::try_from(d.len()).ok()?;
    let last = e.saturating_sub(n).saturating_add(1);
    let round = if last >= low {
        let step = 10f64.powi(i32::try_from(low.saturating_sub(1)).ok()?);
        if ulp(ax) >= step * 0.25 {
            return None;
        }
        false
    } else if last <= low.saturating_sub(2) || d.last() != Some(&b'5') {
        true
    } else {
        return None;
    };
    let mut digits = Vec::with_capacity(p);
    let mut q = e;
    while q >= low {
        digits.push(digit_at(&d, e, q));
        q = q.saturating_sub(1);
    }
    let next = if round { digit_at(&d, e, low.saturating_sub(1)) } else { b'0' };
    Some((digits, next, e))
}

/// `x.toFixed(f)` (f ≤ 20).
pub fn fixed(x: f64, f: usize) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.abs() >= 1e21 || x.is_infinite() {
        return js::num_str(x);
    }
    let neg = x < 0.0;
    let (mut digits, next, mut int_len) = match fixed_fast(x.abs(), f) {
        Some(r) => r,
        None => {
            let s = format!("{:.*}", f.saturating_add(EXTRA), x.abs());
            let (ip, fp) = s.split_once('.').unwrap_or((s.as_str(), ""));
            let digits: Vec<u8> = ip.bytes().chain(fp.bytes().take(f)).collect();
            (digits, fp.as_bytes().get(f).copied().unwrap_or(b'0'), ip.len())
        }
    };
    if next >= b'5' && bump(&mut digits) {
        digits.insert(0, b'1');
        int_len = int_len.saturating_add(1);
    }
    let mut out = String::with_capacity(digits.len().saturating_add(2));
    // toFixed of a negative number is "-" + toFixed(-x), also when that rounds to zero ("-0.000")
    if neg {
        out.push('-');
    }
    for &c in digits.get(..int_len).unwrap_or(&[]) {
        out.push(char::from(c));
    }
    if f > 0 {
        out.push('.');
        for &c in digits.get(int_len..).unwrap_or(&[]) {
            out.push(char::from(c));
        }
    }
    out
}

/// `+x.toFixed(f)`: the number the text reads as.
pub fn fixed_num(x: f64, f: usize) -> f64 {
    if !x.is_finite() || x.abs() >= 1e21 {
        return x;
    }
    fixed(x, f).parse::<f64>().unwrap_or(f64::NAN)
}

/// The significant digits of |x| rounded half up to p, and the decimal exponent of the first one.
fn sig_digits(ax: f64, p: usize) -> (Vec<u8>, i64) {
    let (mut d, next, mut e) = match precision_fast(ax, p) {
        Some(r) => r,
        None => {
            let s = format!("{:.*e}", p.saturating_sub(1).saturating_add(EXTRA), ax);
            let (mant, exp) = s.split_once('e').unwrap_or((s.as_str(), "0"));
            let all: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).collect();
            let d: Vec<u8> = all.iter().copied().take(p).collect();
            (d, all.get(p).copied().unwrap_or(b'0'), exp.parse().unwrap_or(0))
        }
    };
    if next >= b'5' && bump(&mut d) {
        d.insert(0, b'1');
        d.truncate(p);
        e = e.saturating_add(1);
    }
    (d, e)
}

fn push_digits(out: &mut String, d: &[u8]) {
    for &c in d {
        out.push(char::from(c));
    }
}

/// `x.toPrecision(p)` (1 ≤ p ≤ 21).
pub fn precision(x: f64, p: usize) -> String {
    if !x.is_finite() {
        return js::num_str(x);
    }
    let p = p.clamp(1, 100);
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    let (digits, e) = if x == 0.0 { (vec![b'0'; p], 0i64) } else { sig_digits(x.abs(), p) };
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

/// `+x.toPrecision(p)`.
pub fn precision_num(x: f64, p: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    precision(x, p).parse::<f64>().unwrap_or(f64::NAN)
}

/// `x.toExponential(f)`.
pub fn exponential(x: f64, f: usize) -> String {
    if !x.is_finite() {
        return js::num_str(x);
    }
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    let p = f.saturating_add(1);
    let (digits, e) = if x == 0.0 { (vec![b'0'; p], 0i64) } else { sig_digits(x.abs(), p) };
    push_digits(&mut out, digits.get(..1).unwrap_or(&[]));
    if f > 0 {
        out.push('.');
        push_digits(&mut out, digits.get(1..).unwrap_or(&[]));
    }
    out.push('e');
    out.push(if e < 0 { '-' } else { '+' });
    out.push_str(&e.unsigned_abs().to_string());
    out
}

/// `String(x)`.
#[inline]
pub fn s(x: f64) -> String {
    js::num_str(x)
}

/// `String(x)` with ECMAScript's tie rule for the shortest digits: when two shortest decimals are equally close to x
/// (x is exactly halfway, as f32 values widened to f64 often are: 28.7664337158203125 → "28.766433715820312"), the
/// even one. Rust's shortest formatting (and so `js::push_num`) takes the other in some of those cases.
pub fn js_num(x: f64, out: &mut String) {
    let start = out.len();
    js::push_num(out, x);
    if !x.is_finite() || x == 0.0 || x.fract() == 0.0 && x.abs() < 9_007_199_254_740_992.0 {
        return;
    }
    // the shortest digits Rust chose, and the exact value's digits
    let short = format!("{:e}", x.abs());
    let (mant, exp) = short.split_once('e').unwrap_or((short.as_str(), "0"));
    let digits: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).collect();
    let k = digits.len();
    let exact = format!("{:.*e}", k.saturating_add(40), x.abs());
    let (emant, eexp) = exact.split_once('e').unwrap_or((exact.as_str(), "0"));
    if eexp != exp {
        return;
    }
    let ed: Vec<u8> = emant.bytes().filter(u8::is_ascii_digit).collect();
    // a tie: the exact value has exactly k+1 significant digits and the last is 5
    let tie = ed.get(k) == Some(&b'5') && ed.iter().skip(k.saturating_add(1)).all(|&c| c == b'0');
    if !tie || digits.last().is_none_or(|d| d % 2 == 0) {
        return;
    }
    // the other candidate: down to the truncated digits when Rust rounded up, else up one
    let trunc: Vec<u8> = ed.iter().take(k).copied().collect();
    let mut alt = trunc.clone();
    if digits == trunc && bump(&mut alt) {
        return;
    }
    let e: i64 = exp.parse().unwrap_or(0);
    // write alt × 10^(e − k + 1) as Number::toString would
    let mut t = String::new();
    if x < 0.0 {
        t.push('-');
    }
    let n = e.saturating_add(1);
    let kk = i64::try_from(k).unwrap_or(0);
    let trimmed: Vec<u8> = {
        let mut a = alt.clone();
        while a.len() > 1 && a.last() == Some(&b'0') {
            a.pop();
        }
        a
    };
    let k2 = i64::try_from(trimmed.len()).unwrap_or(kk);
    if k2 <= n && n <= 21 {
        push_digits(&mut t, &trimmed);
        for _ in 0..n.saturating_sub(k2) {
            t.push('0');
        }
    } else if 0 < n && n <= 21 {
        let nu = usize::try_from(n).unwrap_or(0);
        push_digits(&mut t, trimmed.get(..nu).unwrap_or(&[]));
        t.push('.');
        push_digits(&mut t, trimmed.get(nu..).unwrap_or(&[]));
    } else if -6 < n && n <= 0 {
        t.push_str("0.");
        for _ in 0..n.saturating_neg() {
            t.push('0');
        }
        push_digits(&mut t, &trimmed);
    } else {
        let ee = n.saturating_sub(1);
        push_digits(&mut t, trimmed.get(..1).unwrap_or(&[]));
        if trimmed.len() > 1 {
            t.push('.');
            push_digits(&mut t, trimmed.get(1..).unwrap_or(&[]));
        }
        t.push('e');
        t.push(if ee < 0 { '-' } else { '+' });
        t.push_str(&ee.unsigned_abs().to_string());
    }
    // only when the other candidate reads back as x (it does at an exact tie, short of a power-of-two edge)
    if t.parse::<f64>().ok() == Some(x) {
        out.truncate(start);
        out.push_str(&t);
    }
}

/// JS WhiteSpace + LineTerminator (what `trim()` removes and `\s` matches).
#[inline]
pub fn is_space(c: char) -> bool {
    js::is_ws(c)
}

/// `parseInt(s, 10)`: leading JS whitespace, optional sign, decimal digits; NaN when there are none.
pub fn parse_int(s: &str) -> f64 {
    let t = s.trim_start_matches(is_space);
    let (neg, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, t.get(1..).unwrap_or("")),
        Some(b'+') => (false, t.get(1..).unwrap_or("")),
        _ => (false, t),
    };
    let n = rest.bytes().take_while(u8::is_ascii_digit).count();
    let digits = rest.get(..n).unwrap_or("");
    if digits.is_empty() {
        return f64::NAN;
    }
    let v: f64 = digits.parse().unwrap_or(f64::INFINITY);
    if neg { -v } else { v }
}

/// `parseFloat(s)`: the longest prefix that is a StrDecimalLiteral (after leading JS whitespace); NaN when none.
pub fn parse_float(s: &str) -> f64 {
    let t = s.trim_start_matches(is_space);
    let b = t.as_bytes();
    let mut i = 0usize;
    if matches!(b.first(), Some(b'+') | Some(b'-')) {
        i = 1;
    }
    let neg = b.first() == Some(&b'-');
    if t.get(i..).is_some_and(|r| r.starts_with("Infinity")) {
        return if neg { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let digits = |from: usize| b.get(from..).map_or(0, |r| r.iter().take_while(|c| c.is_ascii_digit()).count());
    let int = digits(i);
    let mut j = i.saturating_add(int);
    let mut frac = 0usize;
    if b.get(j) == Some(&b'.') {
        frac = digits(j.saturating_add(1));
        if int > 0 || frac > 0 {
            j = j.saturating_add(1).saturating_add(frac);
        }
    }
    if int == 0 && frac == 0 {
        return f64::NAN;
    }
    if matches!(b.get(j), Some(b'e') | Some(b'E')) {
        let mut k = j.saturating_add(1);
        if matches!(b.get(k), Some(b'+') | Some(b'-')) {
            k = k.saturating_add(1);
        }
        let e = digits(k);
        if e > 0 {
            j = k.saturating_add(e);
        }
    }
    t.get(..j).and_then(|p| p.parse::<f64>().ok()).unwrap_or(f64::NAN)
}

/// `Number(s)` for a string.
#[inline]
pub fn number(s: &str) -> f64 {
    cc_m3d::file::str_to_number(s)
}

/// `n.toString(16)` for an integer 0 ≤ n < 2^53, lower case.
pub fn hex(n: f64) -> String {
    if !(n >= 0.0) || !n.is_finite() {
        return js::num_str(n);
    }
    // a whole number below 2^53 (the callers hand integers)
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let u = n as u64;
    format!("{u:x}")
}

/// `Number.isInteger(x)`.
#[inline]
pub fn is_integer(x: f64) -> bool {
    js::is_integer(x)
}

/// A JS number used as an index or count: `Some(n)` when it is a whole number in 0 ..= usize::MAX.
pub fn index(x: f64) -> Option<usize> {
    if !(x >= 0.0) || !x.is_finite() || x.fract() != 0.0 || x > 9_007_199_254_740_991.0 {
        return None;
    }
    // whole, non-negative, below 2^53
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let u = x as u64;
    usize::try_from(u).ok()
}

/// `x | 0` (ToInt32).
pub fn to_i32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    let t = x.trunc();
    let m = t.rem_euclid(4_294_967_296.0);
    // 0 ≤ m < 2^32: fits u32 exactly
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let u = m as u32;
    i32::from_ne_bytes(u.to_ne_bytes())
}

/// `x >>> 0` (ToUint32).
pub fn to_u32(x: f64) -> u32 {
    u32::from_ne_bytes(to_i32(x).to_ne_bytes())
}

/// `Math.floor(x)` as an i64 when finite and in range (else None).
pub fn floor_i64(x: f64) -> Option<i64> {
    let f = x.floor();
    if !f.is_finite() || f.abs() > 9.0e15 {
        return None;
    }
    // finite and within ±9e15
    #[allow(clippy::cast_possible_truncation)]
    Some(f as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_matches_exact() {
        let mut seed = 12345u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let special = [0.5, 1.5, 2.5, 0.125, 0.0625, 1e-7, 5e-7, 4.9999995e-7, 123456.0000005, 0.0, -0.0, -1.5, 1e20, 0.005, 1.005];
        for &x in &special {
            for f in [0, 1, 2, 3, 6, 9] {
                assert_eq!(fixed(x, f), js::to_fixed(x, f), "{x} {f}");
            }
        }
        for k in 0..20000 {
            // decimals with few digits hit the ties and the "S ends one place below" cases
            let short = ((next() - 0.5) * 2e6).round() / 10f64.powi(k % 12);
            for f in [0, 1, 2, 3, 4, 5, 6, 9, 12, 20] {
                assert_eq!(fixed(short, f), js::to_fixed(short, f), "{short} {f}");
            }
            for p in [1, 2, 3, 4, 5, 6, 7, 10, 15, 17, 21] {
                assert_eq!(precision(short, p), js::to_precision(short, p), "{short} {p}");
            }
            let x = (next() - 0.5) * 10f64.powf((next() * 20.0).floor() - 8.0);
            for f in [0, 3, 6, 9, 17, 20] {
                assert_eq!(fixed(x, f), js::to_fixed(x, f), "{x} {f}");
            }
            for p in [1, 7, 10, 12, 15, 17, 21] {
                assert_eq!(precision(x, p), js::to_precision(x, p), "{x} {p}");
            }
        }
        assert_eq!(precision(1234567890123455.0, 15), js::to_precision(1234567890123455.0, 15));
        assert_eq!(exponential(123.456, 9), "1.234560000e+2");
        assert_eq!(exponential(-0.000123, 2), "-1.23e-4");
        let mut t = String::new();
        js_num(f64::from(f32::from_bits(0x41e6_21a8)), &mut t);
        assert_eq!(t, "28.766433715820312");
        let mut t = String::new();
        js_num(0.1, &mut t);
        assert_eq!(t, "0.1");
    }

    #[test]
    fn parsing() {
        assert_eq!(parse_float("  1.5e3x"), 1500.0);
        assert!(parse_float("x").is_nan());
        assert_eq!(parse_float("-.5"), -0.5);
        assert_eq!(parse_float("5.e"), 5.0);
        assert_eq!(parse_int(" 42abc"), 42.0);
        assert!(parse_int("abc").is_nan());
        assert_eq!(to_i32(4294967297.0), 1);
        assert_eq!(to_i32(-1.0), -1);
        assert_eq!(hex(255.0), "ff");
    }
}
