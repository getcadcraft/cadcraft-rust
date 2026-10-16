//! JavaScript number and string semantics the port depends on (ToInt32, `Number(string)`, `trim`, `toUpperCase` over
//! UTF-16, `JSON.stringify` of a string) and small checked conversions.

/// JS ToInt32 (`x | 0`, an `Int32Array` store).
pub fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    let t = x.trunc();
    if (-2_147_483_648.0..2_147_483_648.0).contains(&t) {
        // an integer inside the i32 range: the cast is exact
        #[allow(clippy::cast_possible_truncation)]
        let v = t as i32;
        return v;
    }
    let m = t.rem_euclid(4_294_967_296.0);
    i32::from_ne_bytes(f64_to_u32(m).to_ne_bytes())
}

/// An integral f64 in [0, 2^32) → u32 (else 0).
pub fn f64_to_u32(x: f64) -> u32 {
    if !(0.0..4_294_967_296.0).contains(&x) {
        return 0;
    }
    // in range by the check above: the cast is exact for the integral part
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let v = x.trunc() as u32;
    v
}

/// A count or index held as an f64 (JS number) → usize when it is a non-negative integer below `cap`.
pub fn f64_index(x: f64, cap: usize) -> Option<usize> {
    if x.is_nan() || x < 0.0 || x.fract() != 0.0 || x >= usize_f64(cap) {
        return None;
    }
    let u = f64_to_u64(x)?;
    usize::try_from(u).ok()
}

/// An integral f64 in [0, 2^53] → u64.
pub fn f64_to_u64(x: f64) -> Option<u64> {
    if !(0.0..=9_007_199_254_740_992.0).contains(&x) || x.fract() != 0.0 {
        return None;
    }
    let hi = (x / 4_294_967_296.0).trunc();
    let lo = x - hi * 4_294_967_296.0;
    Some((u64::from(f64_to_u32(hi)) << 32) | u64::from(f64_to_u32(lo)))
}

/// usize → f64 (exact below 2^53, which every length here is).
pub fn usize_f64(n: usize) -> f64 {
    let v = u64::try_from(n).unwrap_or(u64::MAX);
    let hi = u32::try_from(v >> 32).unwrap_or(u32::MAX);
    let lo = u32::try_from(v & 0xFFFF_FFFF).unwrap_or(0);
    f64::from(hi) * 4_294_967_296.0 + f64::from(lo)
}

/// i64 → f64 (exact below 2^53).
pub fn i64_f64(n: i64) -> f64 {
    let neg = n < 0;
    let m = usize_f64(usize::try_from(n.unsigned_abs()).unwrap_or(usize::MAX));
    if neg { -m } else { m }
}

/// f64 → f32 (`Float32Array` store: round to nearest, ties to even).
#[allow(clippy::cast_possible_truncation)] // the rounding to f32 is the point (a Float32Array store)
pub fn f32_round(x: f64) -> f64 {
    f64::from(x as f32)
}

/// JS `Number(s)` over the characters a STEP number token can hold (digits, + - . E e): NaN when it is not a
/// StringNumericLiteral.
pub fn js_number(s: &str) -> f64 {
    let b = s.as_bytes();
    let mut i = 0usize;
    let n = b.len();
    let at = |k: usize| b.get(k).copied().unwrap_or(0);
    if n == 0 {
        return 0.0;
    }
    if at(0) == b'+' || at(0) == b'-' {
        i = 1;
    }
    let d0 = i;
    while at(i).is_ascii_digit() {
        i = i.saturating_add(1);
    }
    let int_digits = i.saturating_sub(d0);
    let mut frac_digits = 0usize;
    if at(i) == b'.' {
        i = i.saturating_add(1);
        let f0 = i;
        while at(i).is_ascii_digit() {
            i = i.saturating_add(1);
        }
        frac_digits = i.saturating_sub(f0);
    }
    if int_digits == 0 && frac_digits == 0 {
        return f64::NAN;
    }
    if at(i) == b'e' || at(i) == b'E' {
        i = i.saturating_add(1);
        if at(i) == b'+' || at(i) == b'-' {
            i = i.saturating_add(1);
        }
        let e0 = i;
        while at(i).is_ascii_digit() {
            i = i.saturating_add(1);
        }
        if i == e0 {
            return f64::NAN;
        }
    }
    if i != n {
        return f64::NAN;
    }
    s.parse::<f64>().unwrap_or(f64::NAN)
}

/// JS white space and line terminators (`String.prototype.trim`).
pub fn is_js_space(c: u32) -> bool {
    matches!(c, 0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
}

/// `s.trim()` on UTF-16 code units.
pub fn trim16(s: &[u16]) -> &[u16] {
    let mut a = 0usize;
    let mut b = s.len();
    while a < b && s.get(a).is_some_and(|&c| is_js_space(u32::from(c))) {
        a = a.saturating_add(1);
    }
    while b > a && s.get(b.saturating_sub(1)).is_some_and(|&c| is_js_space(u32::from(c))) {
        b = b.saturating_sub(1);
    }
    s.get(a..b).unwrap_or(&[])
}

/// `s.toUpperCase()` on UTF-16 code units (full Unicode mapping; lone surrogates kept).
pub fn upper16(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    for r in char::decode_utf16(s.iter().copied()) {
        match r {
            Ok(c) => {
                if c.is_ascii() {
                    out.push(u16::try_from(u32::from(c.to_ascii_uppercase())).unwrap_or(0));
                } else {
                    let mut buf = [0u16; 2];
                    for u in c.to_uppercase() {
                        out.extend_from_slice(u.encode_utf16(&mut buf));
                    }
                }
            }
            Err(e) => out.push(e.unpaired_surrogate()),
        }
    }
    out
}

/// `s.trim()` on a Rust string (JS white space).
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| is_js_space(u32::from(c)))
}

/// Push `JSON.stringify` of a UTF-16 string (lone surrogates as \uXXXX).
pub fn push_json_str16(out: &mut String, s: &[u16]) {
    out.push('"');
    for r in char::decode_utf16(s.iter().copied()) {
        match r {
            Ok(c) => push_json_char(out, c),
            Err(e) => push_u_escape(out, u32::from(e.unpaired_surrogate())),
        }
    }
    out.push('"');
}

/// Push `JSON.stringify` of a string.
pub fn push_json_str(out: &mut String, s: &str) {
    out.push('"');
    // fast path: runs without characters to escape
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        if c == '"' || c == '\\' || u32::from(c) < 0x20 {
            out.push_str(s.get(start..i).unwrap_or(""));
            push_json_char(out, c);
            start = i.saturating_add(c.len_utf8());
        }
    }
    out.push_str(s.get(start..).unwrap_or(""));
    out.push('"');
}

fn push_json_char(out: &mut String, c: char) {
    match c {
        '"' => out.push_str("\\\""),
        '\\' => out.push_str("\\\\"),
        '\u{8}' => out.push_str("\\b"),
        '\u{c}' => out.push_str("\\f"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        c if u32::from(c) < 0x20 => push_u_escape(out, u32::from(c)),
        c => out.push(c),
    }
}

fn push_u_escape(out: &mut String, v: u32) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push_str("\\u");
    for sh in [12u32, 8, 4, 0] {
        let d = usize::try_from((v >> sh) & 0xF).unwrap_or(0);
        out.push(char::from(HEX.get(d).copied().unwrap_or(b'0')));
    }
}

/// A JSON number for an f64 that `JSON.parse` reads back to the same double (finite only; NaN/∞ as the kit's tokens).
pub fn push_json_num(out: &mut String, x: f64) {
    use std::fmt::Write as _;
    if x.is_nan() {
        out.push_str("\"$NaN\"");
    } else if x == f64::INFINITY {
        out.push_str("\"$Inf\"");
    } else if x == f64::NEG_INFINITY {
        out.push_str("\"$-Inf\"");
    } else if x == 0.0 {
        out.push('0');
    } else {
        let a = x.abs();
        if (1e-6..1e21).contains(&a) {
            let _ = write!(out, "{x}");
        } else {
            let _ = write!(out, "{x:e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int32_wraps_like_js() {
        assert_eq!(to_int32(4_294_967_297.0), 1);
        assert_eq!(to_int32(2_147_483_648.0), i32::MIN);
        assert_eq!(to_int32(-1.0), -1);
        assert_eq!(to_int32(1e30), to_int32(1e30)); // deterministic
        assert_eq!(to_int32(f64::NAN), 0);
    }

    #[test]
    fn number_grammar() {
        assert_eq!(js_number("1.0"), 1.0);
        assert_eq!(js_number("-.5"), -0.5);
        assert_eq!(js_number("+3"), 3.0);
        assert_eq!(js_number("1.0E5"), 1e5);
        assert_eq!(js_number("1e-3"), 1e-3);
        assert!(js_number("-").is_nan());
        assert!(js_number("1.5.0").is_nan());
        assert!(js_number("1e").is_nan());
        assert!(js_number("1.0E").is_nan());
        assert!(js_number("--1").is_nan());
        assert!(js_number("1E400").is_infinite());
    }

    #[test]
    fn json_numbers_round_trip() {
        for x in [1.0, -2.5, 1e300, 1e-300, 0.1, 123456789012345680000.0, 5e-7, 1e21] {
            let mut s = String::new();
            push_json_num(&mut s, x);
            assert_eq!(s.parse::<f64>().unwrap(), x, "{s}");
        }
    }

    #[test]
    fn upper_and_trim() {
        let s: Vec<u16> = " a\u{df}x ".encode_utf16().collect();
        assert_eq!(String::from_utf16(&upper16(trim16(&s))).unwrap(), "ASSX");
        assert_eq!(upper16(&[0xD800, 0x61]), vec![0xD800, 0x41]);
    }
}
