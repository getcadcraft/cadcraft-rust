//! JavaScript semantics the ported modules depend on, gathered in one place: V8's `Math.*` (through cc-cad's `js` and
//! cc-m3d's `jsmath`, never re-implemented here), number text (`String(x)`, `toFixed`), `localeCompare` (ICU root
//! collation, as Node ships it), stable sorting with a JS comparator, and the JSON form of results (the parity kit's
//! canonical form: NaN → `"$NaN"`, ±∞ → `"$Inf"` / `"$-Inf"`).

use serde_json::{Map, Value};
use std::cmp::Ordering;

pub use cc_cad::js::{
    hypot, int32, is_ws, max, max_of, min, min_of, num_to_string, round, sign, slice16, str_to_number, to_exponential, to_fixed,
    to_js_string, to_number, trim, trim_end, trim_start, truthy,
};
pub use cc_m3d::jsmath::JsMath;

/// `Math.hypot(a, b, c)`.
pub fn hypot3(a: f64, b: f64, c: f64) -> f64 {
    cc_m3d::js::hypot(&[a, b, c])
}

/// `Math.log10` (fdlibm, as V8).
pub fn log10(x: f64) -> f64 {
    libm::log10(x)
}

/// `Math.pow`.
pub fn pow(x: f64, y: f64) -> f64 {
    x.jpow(y)
}

/// `Number(x.toFixed(d))`.
pub fn round_fixed(x: f64, d: usize) -> f64 {
    str_to_number(&to_fixed(x, d))
}

/// `+x.toFixed(d)` / `Number(…)` of a fixed text, with the fast path for numbers that are exact already.
pub fn fixed_num(x: f64, d: usize) -> f64 {
    round_fixed(x, d)
}

/// A comparator result (`a - b` …) as an ordering: NaN and ±0 are "equal" (the spec's ToNumber, NaN → +0).
pub fn ord(c: f64) -> Ordering {
    if c < 0.0 {
        Ordering::Less
    } else if c > 0.0 {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

/// `list.sort((a, b) => cmp(a, b))`: V8's sort is stable (TimSort), so is `sort_by`; with a consistent comparator
/// the two give the same order.
pub fn sort_by<T>(list: &mut [T], mut cmp: impl FnMut(&T, &T) -> f64) {
    list.sort_by(|a, b| ord(cmp(a, b)));
}

/// `Number.isInteger(x)`.
pub fn is_integer(x: f64) -> bool {
    x.is_finite() && x.trunc() == x
}

/// `String(x).padStart(n, c)`.
pub fn pad_start(s: &str, n: usize, c: char) -> String {
    let len = s.encode_utf16().count();
    if len >= n {
        return s.to_string();
    }
    let mut out: String = std::iter::repeat_n(c, n.saturating_sub(len)).collect();
    out.push_str(s);
    out
}

/// `s.toUpperCase()` (full Unicode mapping, as V8).
pub fn upper(s: &str) -> String {
    s.to_uppercase()
}
/// `s.toLowerCase()`.
pub fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// Length in UTF-16 code units (`s.length`).
pub fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `s.slice(a, b)` in UTF-16 code units (negative = from the end; a surrogate pair cut in half is dropped whole).
pub fn slice16_range(s: &str, a: i64, b: Option<i64>) -> String {
    let n = i64::try_from(len16(s)).unwrap_or(i64::MAX);
    let fix = |v: i64| if v < 0 { n.saturating_add(v).max(0) } else { v.min(n) };
    let from = fix(a);
    let to = b.map_or(n, fix);
    if to <= from {
        return String::new();
    }
    let mut out = String::new();
    let mut pos = 0i64;
    for c in s.chars() {
        let w = i64::try_from(c.len_utf16()).unwrap_or(1);
        if pos >= from && pos.saturating_add(w) <= to {
            out.push(c);
        }
        pos = pos.saturating_add(w);
        if pos >= to {
            break;
        }
    }
    out
}

/// `s.replace(/[\u0000-\u001f\u007f]/g, rep)`.
pub fn strip_controls(s: &str, rep: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if (c as u32) < 0x20 || c == '\u{7f}' {
            out.push_str(rep);
        } else {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// localeCompare (ICU root collation, the part these modules meet: Latin text, digits, punctuation)

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Prim {
    /// white space and punctuation, then symbols (ICU's variable characters, non-ignorable by default)
    Sym(u16),
    /// a digit (numeric: false) or a run of digits by value (numeric: true): (significant digits, digits)
    Num(usize, String),
    Letter(u8),
    Other(u32),
}

// ICU root order of the ASCII punctuation and symbols (CLDR root, DUCET): white space, then _ - , ; : ! ? . ' " ( ) [ ] { }
// @ * / \ & # % ` ^ + < = > | ~ $ (currency after the other symbols)
const SYMBOLS: &str = "\t\n\u{b}\u{c}\r \u{a0}_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

fn latin1_base(c: char) -> Option<(&'static str, bool)> {
    let (b, up) = match c {
        'À'..='Å' => ("a", true),
        'à'..='å' => ("a", false),
        'Æ' => ("ae", true),
        'æ' => ("ae", false),
        'Ç' => ("c", true),
        'ç' => ("c", false),
        'È'..='Ë' => ("e", true),
        'è'..='ë' => ("e", false),
        'Ì'..='Ï' => ("i", true),
        'ì'..='ï' => ("i", false),
        'Ñ' => ("n", true),
        'ñ' => ("n", false),
        'Ò'..='Ö' | 'Ø' => ("o", true),
        'ò'..='ö' | 'ø' => ("o", false),
        'Ù'..='Ü' => ("u", true),
        'ù'..='ü' => ("u", false),
        'Ý' => ("y", true),
        'ý' | 'ÿ' => ("y", false),
        'ß' => ("ss", false),
        _ => return None,
    };
    Some((b, up))
}

fn collation_elements(s: &str, numeric: bool) -> Vec<(Prim, u32, u8)> {
    let mut out = Vec::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    while i < cs.len() {
        let Some(&c) = cs.get(i) else { break };
        if c.is_ascii_digit() {
            if numeric {
                let mut j = i;
                while cs.get(j).is_some_and(char::is_ascii_digit) {
                    j = j.saturating_add(1);
                }
                let digits: String = cs.get(i..j).unwrap_or(&[]).iter().collect();
                let sig = digits.trim_start_matches('0').to_string();
                out.push((Prim::Num(sig.len(), sig), 0, 0));
                i = j;
            } else {
                out.push((Prim::Num(1, c.to_string()), 0, 0));
                i = i.saturating_add(1);
            }
            continue;
        }
        i = i.saturating_add(1);
        let cp = u32::from(c);
        if let Some(k) = SYMBOLS.chars().position(|x| x == c) {
            out.push((Prim::Sym(u16::try_from(k).unwrap_or(u16::MAX)), 0, 0));
            continue;
        }
        if cp < 0x20 || (0x7F..=0x9F).contains(&cp) {
            continue; // other control characters are ignorable
        }
        if c.is_ascii_alphabetic() {
            let l = c.to_ascii_lowercase();
            out.push((Prim::Letter(u8::try_from(u32::from(l)).unwrap_or(0)), 0, u8::from(c.is_ascii_uppercase())));
            continue;
        }
        if let Some((base, up)) = latin1_base(c) {
            let sharp = c == 'ß';
            for (n, b) in base.chars().enumerate() {
                let sec = if base.len() == 1 { cp } else { 0 };
                let ter = u8::from(up) | if sharp && n == 0 { 2 } else { 0 };
                out.push((Prim::Letter(u8::try_from(u32::from(b)).unwrap_or(0)), sec, ter));
            }
            continue;
        }
        out.push((Prim::Other(cp), 0, 0));
    }
    out
}

fn collate_with(a: &str, b: &str, numeric: bool) -> Ordering {
    let (ea, eb) = (collation_elements(a, numeric), collation_elements(b, numeric));
    let p = ea.iter().map(|e| &e.0).cmp(eb.iter().map(|e| &e.0));
    if p.is_ne() {
        return p;
    }
    let s = ea.iter().map(|e| e.1).filter(|&x| x != 0).cmp(eb.iter().map(|e| e.1).filter(|&x| x != 0));
    if s.is_ne() {
        return s;
    }
    let t = ea.iter().map(|e| e.2).cmp(eb.iter().map(|e| e.2));
    if t.is_ne() {
        return t;
    }
    // identical at every level: ICU then compares code points (only for strings that differ in ignorables)
    a.cmp(b)
}

/// `a.localeCompare(b)` as a number (−1, 0, 1).
pub fn locale_compare(a: &str, b: &str) -> f64 {
    match collate_with(a, b, false) {
        Ordering::Less => -1.0,
        Ordering::Equal => 0.0,
        Ordering::Greater => 1.0,
    }
}
/// `a.localeCompare(b, 'en', { numeric: true })`.
pub fn locale_compare_num(a: &str, b: &str) -> f64 {
    match collate_with(a, b, true) {
        Ordering::Less => -1.0,
        Ordering::Equal => 0.0,
        Ordering::Greater => 1.0,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Results as JSON (the parity kit's canonical form)

/// A number as the kit's canon writes it: NaN → "$NaN", ±∞ → "$Inf" / "$-Inf", −0 → 0.
pub fn jnum(x: f64) -> Value {
    if x.is_nan() {
        Value::String("$NaN".into())
    } else if x.is_infinite() {
        Value::String(if x > 0.0 { "$Inf" } else { "$-Inf" }.into())
    } else {
        let x = if x == 0.0 { 0.0 } else { x };
        serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
    }
}

/// Something with a JSON form.
pub trait J {
    fn j(&self) -> Value;
}
impl J for f64 {
    fn j(&self) -> Value {
        jnum(*self)
    }
}
impl J for usize {
    fn j(&self) -> Value {
        Value::from(*self as u64)
    }
}
impl J for u32 {
    fn j(&self) -> Value {
        Value::from(*self)
    }
}
impl J for i32 {
    fn j(&self) -> Value {
        Value::from(*self)
    }
}
impl J for i64 {
    fn j(&self) -> Value {
        Value::from(*self)
    }
}
impl J for bool {
    fn j(&self) -> Value {
        Value::Bool(*self)
    }
}
impl J for str {
    fn j(&self) -> Value {
        Value::String(self.to_string())
    }
}
impl J for String {
    fn j(&self) -> Value {
        Value::String(self.clone())
    }
}
impl J for Value {
    fn j(&self) -> Value {
        self.clone()
    }
}
impl<T: J> J for Option<T> {
    fn j(&self) -> Value {
        self.as_ref().map_or(Value::Null, J::j)
    }
}
impl<T: J> J for Vec<T> {
    fn j(&self) -> Value {
        Value::Array(self.iter().map(J::j).collect())
    }
}
impl<T: J> J for [T] {
    fn j(&self) -> Value {
        Value::Array(self.iter().map(J::j).collect())
    }
}
impl<T: J, const N: usize> J for [T; N] {
    fn j(&self) -> Value {
        Value::Array(self.iter().map(J::j).collect())
    }
}
impl<T: J + ?Sized> J for &T {
    fn j(&self) -> Value {
        (**self).j()
    }
}
impl<T: J + ?Sized> J for Box<T> {
    fn j(&self) -> Value {
        (**self).j()
    }
}

/// An object builder that keeps insertion order.
#[derive(Default, Debug)]
pub struct Obj(pub Map<String, Value>);
impl Obj {
    pub fn new() -> Self {
        Obj(Map::new())
    }
    pub fn set(mut self, k: &str, v: impl J) -> Self {
        self.0.insert(k.to_string(), v.j());
        self
    }
    /// Set only when present (JS `undefined` = no key).
    pub fn opt<T: J>(mut self, k: &str, v: Option<T>) -> Self {
        if let Some(v) = v {
            self.0.insert(k.to_string(), v.j());
        }
        self
    }
    pub fn put(&mut self, k: &str, v: impl J) {
        self.0.insert(k.to_string(), v.j());
    }
    pub fn done(self) -> Value {
        Value::Object(self.0)
    }
}
impl J for Obj {
    fn j(&self) -> Value {
        Value::Object(self.0.clone())
    }
}

/// `jo!{ "a" => x, "b" => y }` → a JSON object (insertion order kept).
#[macro_export]
macro_rules! jo {
    ($($k:expr => $v:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut m = serde_json::Map::new();
        $( m.insert(($k).to_string(), $crate::js::J::j(&$v)); )*
        serde_json::Value::Object(m)
    }};
}

// ---------------------------------------------------------------------------------------------------------------
// Reading JS values out of JSON input

/// `typeof v === 'number' && Number.isFinite(v)`.
pub fn fin(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite())
}
/// A number field as the JS reads it in arithmetic: a number, else NaN (undefined / null / text are not expected
/// in typed input; text never reaches the typed modules).
pub fn numf(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => match s.as_str() {
            "$NaN" => f64::NAN,
            "$Inf" => f64::INFINITY,
            "$-Inf" => f64::NEG_INFINITY,
            _ => f64::NAN,
        },
        _ => f64::NAN,
    }
}
/// `Number(v)` (ToNumber; undefined → NaN).
pub fn number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::String(s)) if s == "$NaN" => f64::NAN,
        Some(Value::String(s)) if s == "$Inf" => f64::INFINITY,
        Some(Value::String(s)) if s == "$-Inf" => f64::NEG_INFINITY,
        Some(x) => to_number(x),
    }
}
/// `Number(v) || 0`.
pub fn number_or0(v: Option<&Value>) -> f64 {
    let x = number(v);
    if x == 0.0 || x.is_nan() { 0.0 } else { x }
}
/// `String(v ?? '')`.
pub fn string_or_empty(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(x) => to_js_string(x),
    }
}
/// `String(v)` (undefined → "undefined").
pub fn string_of(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(x) => to_js_string(x),
    }
}
/// The text of a string value.
pub fn as_str(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str)
}
/// An array value's items (empty when not an array).
pub fn arr(v: Option<&Value>) -> &[Value] {
    v.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}
/// `v && typeof v === 'object'` (arrays too).
pub fn is_object(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Object(_) | Value::Array(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collation() {
        let mut v = vec!["b", "A", "a", "(no description)", "10", "9", "GRD", "grd", "_x", "EP"];
        v.sort_by(|a, b| ord(locale_compare(a, b)));
        assert_eq!(v, vec!["_x", "(no description)", "10", "9", "a", "A", "b", "EP", "grd", "GRD"]);
        assert_eq!(locale_compare_num("p10", "p9"), 1.0);
    }
    #[test]
    fn slices() {
        assert_eq!(slice16_range("abcdef", 1, Some(3)), "bc");
        assert_eq!(slice16_range("abcdef", -2, None), "ef");
        assert_eq!(pad_start("5", 3, '0'), "005");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Typed arrays

/// A JS typed array: reading past the end gives the array's "undefined" value (`dflt`, e.g. NaN or −1) and writing
/// past the end changes nothing — exactly what Float64Array / Int32Array do, and never a panic. Kernels index it like
/// the JS (`x[i]`).
#[derive(Clone, Debug, Default)]
pub struct TA<T: Copy> {
    v: Vec<T>,
    dflt: T,
    scratch: T,
}
impl<T: Copy> TA<T> {
    pub fn new(v: Vec<T>, dflt: T) -> Self {
        TA { v, dflt, scratch: dflt }
    }
    pub fn filled(n: usize, x: T, dflt: T) -> Self {
        TA { v: vec![x; n], dflt, scratch: dflt }
    }
    pub fn push(&mut self, x: T) {
        self.v.push(x);
    }
    pub fn pop(&mut self) -> Option<T> {
        self.v.pop()
    }
    pub fn into_vec(self) -> Vec<T> {
        self.v
    }
    pub fn as_vec(&self) -> &Vec<T> {
        &self.v
    }
    pub fn truncate(&mut self, n: usize) {
        self.v.truncate(n);
    }
}
impl<T: Copy> std::ops::Index<usize> for TA<T> {
    type Output = T;
    #[inline]
    fn index(&self, i: usize) -> &T {
        self.v.get(i).unwrap_or(&self.dflt)
    }
}
impl<T: Copy> std::ops::IndexMut<usize> for TA<T> {
    #[inline]
    fn index_mut(&mut self, i: usize) -> &mut T {
        match self.v.get_mut(i) {
            Some(x) => x,
            None => {
                self.scratch = self.dflt;
                &mut self.scratch
            }
        }
    }
}
impl<T: Copy> std::ops::Deref for TA<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.v
    }
}
impl<T: Copy> std::ops::DerefMut for TA<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.v
    }
}
/// `TA` over f64 (undefined = NaN).
pub fn fa(v: Vec<f64>) -> TA<f64> {
    TA::new(v, f64::NAN)
}
/// `TA` over u32 indices (undefined = 0, the value a Uint32Array stores for `undefined`).
pub fn ua(v: Vec<u32>) -> TA<u32> {
    TA::new(v, 0)
}
/// `TA` over i32 (undefined = −1, "none" in the half-edge arrays).
pub fn ia(v: Vec<i32>) -> TA<i32> {
    TA::new(v, -1)
}

/// A list that reads like a JS array: out of range = the default (a clone), writes past the end ignored.
#[derive(Clone, Debug, Default)]
pub struct JA<T: Clone> {
    pub v: Vec<T>,
    dflt: T,
    scratch: T,
}
impl<T: Clone> JA<T> {
    pub fn new(v: Vec<T>, dflt: T) -> Self {
        JA { v, scratch: dflt.clone(), dflt }
    }
}
impl<T: Clone> std::ops::Index<usize> for JA<T> {
    type Output = T;
    fn index(&self, i: usize) -> &T {
        self.v.get(i).unwrap_or(&self.dflt)
    }
}
impl<T: Clone> std::ops::IndexMut<usize> for JA<T> {
    fn index_mut(&mut self, i: usize) -> &mut T {
        match self.v.get_mut(i) {
            Some(x) => x,
            None => {
                self.scratch = self.dflt.clone();
                &mut self.scratch
            }
        }
    }
}
impl<T: Clone> std::ops::Deref for JA<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Vec<T> {
        &self.v
    }
}
impl<T: Clone> std::ops::DerefMut for JA<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.v
    }
}

/// `a[i]` for a slice of Copy items with a fallback (the JS's undefined).
#[inline]
pub fn at<T: Copy>(a: &[T], i: usize, d: T) -> T {
    a.get(i).copied().unwrap_or(d)
}
/// `a[i]` for f64 (NaN past the end).
#[inline]
pub fn atf(a: &[f64], i: usize) -> f64 {
    a.get(i).copied().unwrap_or(f64::NAN)
}

/// The value a JS read past the end of an array stands for in these modules (`undefined` → NaN in arithmetic,
/// −1 / 0 for index arrays).
pub trait Und {
    fn und() -> Self;
}
impl Und for f64 {
    fn und() -> Self {
        f64::NAN
    }
}
macro_rules! und_zero {
    ($($t:ty),*) => { $(impl Und for $t { fn und() -> Self { 0 as $t } })* };
}
und_zero!(u8, u16, u32, u64, usize, i64);
impl Und for i32 {
    fn und() -> Self {
        -1
    }
}
impl<T> Und for Option<T> {
    fn und() -> Self {
        None
    }
}
/// `Und` through `Default` for plain structs (a read past the end never happens in a correct port; this keeps it
/// from panicking if it does).
#[macro_export]
macro_rules! und_default {
    ($($t:ty),*) => { $(impl $crate::js::Und for $t { fn und() -> Self { <$t>::default() } })* };
}
impl Und for String {
    fn und() -> Self {
        String::new()
    }
}
impl Und for bool {
    fn und() -> Self {
        false
    }
}
impl<A: Und, B: Und> Und for (A, B) {
    fn und() -> Self {
        (A::und(), B::und())
    }
}
impl<A: Und, B: Und, C: Und> Und for (A, B, C) {
    fn und() -> Self {
        (A::und(), B::und(), C::und())
    }
}
impl<T: Und + Copy, const N: usize> Und for [T; N] {
    fn und() -> Self {
        [T::und(); N]
    }
}
impl Und for crate::geo::P2 {
    fn und() -> Self {
        crate::geo::p2(f64::NAN, f64::NAN)
    }
}
impl Und for crate::geo::P3 {
    fn und() -> Self {
        crate::geo::p3(f64::NAN, f64::NAN, f64::NAN)
    }
}
impl Und for crate::geo::PZ {
    fn und() -> Self {
        crate::geo::PZ { x: f64::NAN, y: f64::NAN, z: None }
    }
}

/// `a[i]` / `a[i] = v` on slices without a panic: a read past the end gives `Und::und()`, a write past the end is
/// dropped (a JS typed array's behaviour).
pub trait G<T> {
    fn g(&self, i: usize) -> T;
    fn s(&mut self, i: usize, v: T);
}
impl<T: Clone + Und> G<T> for [T] {
    #[inline]
    fn g(&self, i: usize) -> T {
        self.get(i).cloned().unwrap_or_else(T::und)
    }
    #[inline]
    fn s(&mut self, i: usize, v: T) {
        if let Some(x) = self.get_mut(i) {
            *x = v;
        }
    }
}
impl<T: Copy> TA<T> {
    #[inline]
    pub fn g(&self, i: usize) -> T {
        self.v.get(i).copied().unwrap_or(self.dflt)
    }
    #[inline]
    pub fn s(&mut self, i: usize, x: T) {
        if let Some(s) = self.v.get_mut(i) {
            *s = x;
        }
    }
}
impl<T: Clone> JA<T> {
    #[inline]
    pub fn g(&self, i: usize) -> T {
        self.v.get(i).cloned().unwrap_or_else(|| self.dflt.clone())
    }
    #[inline]
    pub fn s(&mut self, i: usize, x: T) {
        if let Some(s) = self.v.get_mut(i) {
            *s = x;
        }
    }
    /// `&a[i]` without a clone.
    #[inline]
    pub fn r(&self, i: usize) -> &T {
        self.v.get(i).unwrap_or(&self.dflt)
    }
}

/// A JS number used as an index or count (finite, ≥ 0, truncated), else None.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn usize_of(x: f64) -> Option<usize> {
    if (0.0..9.0e15).contains(&x) { Some(x.trunc() as usize) } else { None }
}
/// The same for i64 (any finite value in range, truncated toward zero).
#[allow(clippy::cast_possible_truncation)]
pub fn i64_of(x: f64) -> Option<i64> {
    if x.is_finite() && x.abs() < 9.0e15 { Some(x.trunc() as i64) } else { None }
}

// ---------------------------------------------------------------------------------------------------------------
// JS regular expressions (the deed-call, traverse and offset parsers copy the JS patterns as written)

const JS_WS: &str = "\\t\\n\\x0B\\x0C\\r \\u{A0}\\u{1680}\\u{2000}-\\u{200A}\\u{2028}\\u{2029}\\u{202F}\\u{205F}\\u{3000}\\u{FEFF}";
const ASCII_B: &str = "(?:(?<=[A-Za-z0-9_])(?![A-Za-z0-9_])|(?<![A-Za-z0-9_])(?=[A-Za-z0-9_]))";

/// A JS pattern (non-unicode mode) in fancy-regex syntax: `\s` `\d` `\w` `\b` and `.` mean what they mean in JS
/// (ASCII digits and word characters, JS white space, `.` stops at line terminators).
pub fn js_pattern(p: &str) -> String {
    let mut out = String::with_capacity(p.len().saturating_mul(2));
    let mut in_class = false;
    let mut it = p.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\\' => {
                let Some(n) = it.next() else {
                    out.push('\\');
                    break;
                };
                match (n, in_class) {
                    ('s', false) => out.push_str(&format!("[{JS_WS}]")),
                    ('s', true) => out.push_str(JS_WS),
                    ('S', false) => out.push_str(&format!("[^{JS_WS}]")),
                    ('d', false) => out.push_str("[0-9]"),
                    ('d', true) => out.push_str("0-9"),
                    ('D', false) => out.push_str("[^0-9]"),
                    ('w', false) => out.push_str("[A-Za-z0-9_]"),
                    ('w', true) => out.push_str("A-Za-z0-9_"),
                    ('W', false) => out.push_str("[^A-Za-z0-9_]"),
                    ('b', false) => out.push_str(ASCII_B),
                    ('/', _) => out.push('/'),
                    _ => {
                        out.push('\\');
                        out.push(n);
                    }
                }
            }
            '[' if !in_class => {
                in_class = true;
                out.push('[');
                if it.peek() == Some(&'^') {
                    out.push('^');
                    it.next();
                }
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            '.' if !in_class => out.push_str("[^\\n\\r\\u{2028}\\u{2029}]"),
            '&' | '~' if in_class => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// A compiled JS regex (None if it does not compile, which these fixed patterns never do).
pub fn js_regex(p: &str, ci: bool) -> Option<fancy_regex::Regex> {
    let pat = js_pattern(p);
    let pat = if ci { format!("(?i){pat}") } else { pat };
    fancy_regex::Regex::new(&pat).ok()
}

/// `re!("pattern")` / `re!("pattern", i)` → `Option<&'static fancy_regex::Regex>`, compiled once.
#[macro_export]
macro_rules! re {
    ($p:expr) => {{
        static R: std::sync::OnceLock<Option<fancy_regex::Regex>> = std::sync::OnceLock::new();
        R.get_or_init(|| $crate::js::js_regex($p, false)).as_ref()
    }};
    ($p:expr, i) => {{
        static R: std::sync::OnceLock<Option<fancy_regex::Regex>> = std::sync::OnceLock::new();
        R.get_or_init(|| $crate::js::js_regex($p, true)).as_ref()
    }};
}

/// `re.exec(s)` → the groups (None = no match; a group that took no part = None).
pub fn exec(re: Option<&fancy_regex::Regex>, s: &str) -> Option<Vec<Option<String>>> {
    let c = re?.captures(s).ok()??;
    Some(c.iter().map(|m| m.map(|m| m.as_str().to_string())).collect())
}
/// `re.test(s)`.
pub fn test(re: Option<&fancy_regex::Regex>, s: &str) -> bool {
    re.is_some_and(|r| r.is_match(s).unwrap_or(false))
}
/// `s.replace(re, rep)` (rep literal; `global` = /g).
pub fn replace(re: Option<&fancy_regex::Regex>, s: &str, rep: &str, global: bool) -> String {
    let Some(r) = re else { return s.to_string() };
    let rep = fancy_regex::NoExpand(rep);
    if global { r.replace_all(s, rep).into_owned() } else { r.replace(s, rep).into_owned() }
}
