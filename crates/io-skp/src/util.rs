//! Shared helpers for cc-skp (port of `lib/skp/util.js`): `SkpError`, the limits for untrusted input, the
//! bounds-checked little-endian `Reader`, and the few JavaScript semantics the output depends on (`Math.hypot`,
//! `String.prototype.trim`, `parseInt`, `parseFloat`, `TextDecoder`).

use std::fmt;
use std::hash::{BuildHasherDefault, Hasher};

use crate::messages::{damaged_text, msg};

/// `SkpError(code, message)`: codes `not_skp` | `too_new` | `unsupported` | `damaged` | `too_big`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkpError {
    pub code: &'static str,
    pub message: String,
}

impl SkpError {
    pub fn new(code: &'static str, message: String) -> Self {
        SkpError { code, message }
    }
}

impl fmt::Display for SkpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SkpError {}

pub type Res<T> = Result<T, SkpError>;

/// `new SkpError('damaged', msg('ccskp.damaged', { reason: msg(key) }))`.
pub fn damaged(key: &str) -> SkpError {
    SkpError::new("damaged", damaged_text(key, &[]))
}

/// `damaged(key, { name })` (zip.js).
pub fn damaged_name(key: &str, name: &str) -> SkpError {
    SkpError::new("damaged", damaged_text(key, &[("name", name)]))
}

/// `new SkpError(code, msg(key, fills))`.
pub fn err(code: &'static str, key: &str, fills: &[(&str, &str)]) -> SkpError {
    SkpError::new(code, msg(key, fills))
}

/// Caps for untrusted input (`LIMITS` in util.js). `read_skp` uses [`LIMITS`]; tests may pass smaller ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub file_bytes: u64,
    pub entry_bytes: u64,
    pub inflated_bytes: u64,
    pub vertices: u64,
    pub depth: usize,
}

pub const LIMITS: Limits = Limits {
    file_bytes: 1024 * 1024 * 1024,
    entry_bytes: 512 * 1024 * 1024,
    inflated_bytes: 2 * 1024 * 1024 * 1024,
    vertices: 5_000_000,
    depth: 64,
};

/// Finite and within ±1e9 inches — anything else is a damaged file (`okNum`).
#[inline]
pub fn ok_num(x: f64) -> bool {
    x.is_finite() && x.abs() < 1e9
}

/// Bounds-checked little-endian reader over the whole file (`Reader` in util.js).
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    pub b: &'a [u8],
    pub pos: usize,
    pub end: usize,
}

impl<'a> Reader<'a> {
    pub fn new(b: &'a [u8], pos: usize) -> Self {
        Reader { b, pos, end: b.len() }
    }
    #[inline]
    pub fn left(&self) -> usize {
        self.end.saturating_sub(self.pos)
    }
    /// `need(n)`: damaged (past_end) when fewer than n bytes are left.
    #[inline]
    pub fn need(&self, n: usize) -> Res<()> {
        match self.pos.checked_add(n) {
            Some(e) if e <= self.end => Ok(()),
            _ => Err(damaged("ccskp.damaged.past_end")),
        }
    }
    #[inline]
    pub fn skip(&mut self, n: usize) -> Res<()> {
        self.need(n)?;
        self.pos = self.pos.saturating_add(n);
        Ok(())
    }
    #[inline]
    fn take<const N: usize>(&mut self) -> Res<[u8; N]> {
        let e = self.pos.checked_add(N).filter(|&e| e <= self.end).ok_or_else(|| damaged("ccskp.damaged.past_end"))?;
        let s = self.b.get(self.pos..e).ok_or_else(|| damaged("ccskp.damaged.past_end"))?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        self.pos = e;
        Ok(a)
    }
    #[inline]
    pub fn u8(&mut self) -> Res<u8> {
        Ok(self.take::<1>()?[0])
    }
    #[inline]
    pub fn u16(&mut self) -> Res<u16> {
        Ok(u16::from_le_bytes(self.take::<2>()?))
    }
    #[inline]
    pub fn u32(&mut self) -> Res<u32> {
        Ok(u32::from_le_bytes(self.take::<4>()?))
    }
    #[inline]
    pub fn i32(&mut self) -> Res<i32> {
        Ok(i32::from_le_bytes(self.take::<4>()?))
    }
    #[inline]
    pub fn f64(&mut self) -> Res<f64> {
        Ok(f64::from_le_bytes(self.take::<8>()?))
    }
    /// `bytes(n)` → the byte range [start, start + n) (borrowed from the file).
    pub fn bytes(&mut self, n: usize) -> Res<&'a [u8]> {
        self.need(n)?;
        let e = self.pos.saturating_add(n);
        let s = self.b.get(self.pos..e).ok_or_else(|| damaged("ccskp.damaged.past_end"))?;
        self.pos = e;
        Ok(s)
    }
    /// UTF-16LE text of n code units (lone surrogates become U+FFFD; JS keeps them as they are).
    pub fn utf16(&mut self, n: usize) -> Res<String> {
        let raw = self.bytes(n.checked_mul(2).ok_or_else(|| damaged("ccskp.damaged.past_end"))?)?;
        Ok(utf16_text(raw))
    }
    /// Each byte as one char (Latin-1), like `String.fromCharCode` per byte.
    pub fn ascii(&mut self, n: usize) -> Res<String> {
        Ok(latin1(self.bytes(n)?))
    }
}

/// UTF-16LE code units → text (`String.fromCharCode` per unit; a lone surrogate becomes U+FFFD).
pub fn utf16_text(raw: &[u8]) -> String {
    if raw.as_chunks::<2>().0.iter().all(|&[lo, hi]| hi == 0 && lo < 0x80) {
        return raw.as_chunks::<2>().0.iter().map(|&[lo, _]| char::from(lo)).collect();
    }
    char::decode_utf16(raw.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)))
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

pub fn latin1(raw: &[u8]) -> String {
    raw.iter().map(|&c| char::from(c)).collect()
}

/// `new TextDecoder('utf-8', { fatal: true }).decode(raw)` (a leading BOM is dropped), else Latin-1.
pub fn utf8_or_latin1(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(s) => s.strip_prefix('\u{feff}').unwrap_or(s).to_string(),
        Err(_) => latin1(raw),
    }
}

/// `new TextDecoder('utf-8').decode(raw)`: replacement characters for bad sequences (the WHATWG rule, which
/// `from_utf8_lossy` follows), a leading BOM dropped.
pub fn utf8_lossy(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    match s.strip_prefix('\u{feff}') {
        Some(t) => t.to_string(),
        None => s.into_owned(),
    }
}

/// JS WhiteSpace + LineTerminator (what `trim()` removes and `\s` matches).
#[inline]
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// `String.prototype.trim`.
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// `parseInt(s, 10)`: leading JS whitespace, optional sign, decimal digits; `None` = NaN. Too many digits → ±Infinity.
pub fn parse_int10(s: &str) -> Option<f64> {
    let t = s.trim_start_matches(is_js_space);
    let (neg, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, t.get(1..).unwrap_or("")),
        Some(b'+') => (false, t.get(1..).unwrap_or("")),
        _ => (false, t),
    };
    let n = rest.bytes().take_while(u8::is_ascii_digit).count();
    let digits = rest.get(..n).unwrap_or("");
    if digits.is_empty() {
        return None;
    }
    let v: f64 = digits.parse().unwrap_or(f64::INFINITY);
    Some(if neg { -v } else { v })
}

/// `parseFloat(s)`: the longest prefix that is a StrDecimalLiteral (after leading JS whitespace); `None` = NaN.
pub fn parse_float(s: &str) -> Option<f64> {
    let t = s.trim_start_matches(is_js_space);
    let b = t.as_bytes();
    let mut i = 0usize;
    if matches!(b.first(), Some(b'+') | Some(b'-')) {
        i = 1;
    }
    let neg = b.first() == Some(&b'-');
    if t.get(i..).is_some_and(|r| r.starts_with("Infinity")) {
        return Some(if neg { f64::NEG_INFINITY } else { f64::INFINITY });
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
        return None;
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
    // Rust's grammar accepts every prefix built above ("1.", ".5", "1.e5").
    t.get(..j).and_then(|p| p.parse::<f64>().ok())
}

/// `Math.hypot(...v)` as V8 computes it (scaled, Kahan-compensated sum), so decisions on the result match bit for bit.
pub fn hypot(v: &[f64]) -> f64 {
    let mut max = 0.0f64;
    let mut nan = false;
    for &x in v {
        let a = x.abs();
        if a.is_infinite() {
            return f64::INFINITY;
        }
        if a.is_nan() {
            nan = true;
        } else if a > max {
            max = a;
        }
    }
    if nan {
        return f64::NAN;
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut comp = 0.0f64;
    for &x in v {
        let n = x.abs() / max;
        let summand = n * n - comp;
        let pre = sum + summand;
        comp = (pre - sum) - summand;
        sum = pre;
    }
    sum.sqrt() * max
}

/// `Math.round` for the non-negative values cc-skp rounds (MB figures): ties go up, like `f64::round` there.
pub fn round_pos(x: f64) -> f64 {
    let r = x.floor();
    if x - r >= 0.5 { r + 1.0 } else { r }
}

/// A small, fast hasher for integer keys (ids, slots): the multiply-rotate mix used by rustc's FxHash.
#[derive(Debug, Default, Clone, Copy)]
pub struct FxHasher {
    h: u64,
}

const FX_K: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.h = (self.h.rotate_left(5) ^ u64::from(b)).wrapping_mul(FX_K);
        }
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.h = (self.h.rotate_left(5) ^ i).wrapping_mul(FX_K);
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.write_u64(u64::from(i));
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.h
    }
}

pub type FxBuild = BuildHasherDefault<FxHasher>;
pub type FxMap<K, V> = std::collections::HashMap<K, V, FxBuild>;
pub type FxSet<K> = std::collections::HashSet<K, FxBuild>;

/// `n.toLocaleString('en-US')` for a non-negative integer.
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len().saturating_add(s.len() / 3));
    let len = s.len();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (len.saturating_sub(i)) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A JS number as `String(v)` prints it, for the integer-valued fills cc-skp passes.
pub fn js_num_text(v: f64) -> String {
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if v == 0.0 {
        return "0".to_string();
    }
    if v == v.trunc() && v.abs() < 1e21 {
        return format!("{v:.0}");
    }
    format!("{v}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_semantics() {
        assert_eq!(parse_int10("  42px"), Some(42.0));
        assert_eq!(parse_int10("-7"), Some(-7.0));
        assert_eq!(parse_int10("x1"), None);
        assert_eq!(parse_int10(&"9".repeat(400)), Some(f64::INFINITY));
        assert_eq!(parse_float(" 0.25abc"), Some(0.25));
        assert_eq!(parse_float("1.e2"), Some(100.0));
        assert_eq!(parse_float(".5"), Some(0.5));
        assert_eq!(parse_float("1e"), Some(1.0));
        assert_eq!(parse_float("-Infinityx"), Some(f64::NEG_INFINITY));
        assert_eq!(parse_float("."), None);
        assert_eq!(parse_float("abc"), None);
        assert_eq!(js_trim("\u{feff} a b\u{3000}\n"), "a b");
        assert_eq!(js_trim("\u{85}a"), "\u{85}a");
        assert_eq!(thousands(5_000_000), "5,000,000");
        assert_eq!(thousands(999), "999");
        assert_eq!(js_num_text(3.0), "3");
        assert_eq!(js_num_text(-0.0), "0");
        assert_eq!(hypot(&[3.0, 4.0]), 5.0);
        assert!(hypot(&[f64::NAN, f64::INFINITY]).is_infinite());
        assert_eq!(utf8_or_latin1(b"\xef\xbb\xbfab"), "ab");
        assert_eq!(utf8_or_latin1(b"a\xffb"), "a\u{ff}b");
        assert_eq!(utf16_text(&[0x41, 0, 0x00, 0xd8]), "A\u{fffd}");
    }
}
