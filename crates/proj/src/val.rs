//! A JavaScript value, for the parts of proj4js that work on plain objects (proj strings, the WKT reader and its PROJJSON
//! step): numbers, strings, booleans, arrays and objects whose keys come back in JS order (integer keys ascending first,
//! then the others in insertion order — what `Object.keys` and `for…in` give).

use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Val {
    #[default]
    Undef,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Val>),
    Obj(Props),
}

/// A plain object.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Props {
    items: Vec<(String, Val)>,
}

/// "an array index" in the JS sense: a canonical non-negative integer below 2^32 − 1.
fn index_key(k: &str) -> Option<u32> {
    if k.is_empty() || k.len() > 10 || (k.len() > 1 && k.starts_with('0')) || !k.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    k.parse::<u64>().ok().filter(|&n| n < 4_294_967_295).and_then(|n| u32::try_from(n).ok())
}

impl Props {
    pub fn new() -> Self {
        Props { items: Vec::new() }
    }
    pub fn get(&self, k: &str) -> Option<&Val> {
        self.items.iter().find(|(a, _)| a == k).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, k: &str) -> Option<&mut Val> {
        self.items.iter_mut().find(|(a, _)| a == k).map(|(_, v)| v)
    }
    /// `obj[k]` (undefined when missing).
    pub fn at(&self, k: &str) -> Val {
        self.get(k).cloned().unwrap_or(Val::Undef)
    }
    pub fn has(&self, k: &str) -> bool {
        self.items.iter().any(|(a, _)| a == k)
    }
    /// `obj[k] = v` (a new key goes last; an existing one keeps its place).
    pub fn set(&mut self, k: &str, v: Val) {
        match self.get_mut(k) {
            Some(slot) => *slot = v,
            None => self.items.push((k.to_string(), v)),
        }
    }
    /// `delete obj[k]`.
    pub fn delete(&mut self, k: &str) {
        self.items.retain(|(a, _)| a != k);
    }
    /// `Object.keys(obj)`.
    pub fn keys(&self) -> Vec<String> {
        let mut idx: Vec<(u32, &String)> = self.items.iter().filter_map(|(k, _)| index_key(k).map(|n| (n, k))).collect();
        idx.sort_by_key(|(n, _)| *n);
        let mut out: Vec<String> = idx.into_iter().map(|(_, k)| k.clone()).collect();
        out.extend(self.items.iter().filter(|(k, _)| index_key(k).is_none()).map(|(k, _)| k.clone()));
        out
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Val {
    pub fn str(s: &str) -> Val {
        Val::Str(s.to_string())
    }
    /// JS truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Val::Undef | Val::Null => false,
            Val::Bool(b) => *b,
            Val::Num(n) => *n != 0.0 && !n.is_nan(),
            Val::Str(s) => !s.is_empty(),
            Val::Arr(_) | Val::Obj(_) => true,
        }
    }
    pub fn is_undef(&self) -> bool {
        matches!(self, Val::Undef)
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_obj(&self) -> Option<&Props> {
        match self {
            Val::Obj(o) => Some(o),
            _ => None,
        }
    }
    pub fn as_obj_mut(&mut self) -> Option<&mut Props> {
        match self {
            Val::Obj(o) => Some(o),
            _ => None,
        }
    }
    pub fn as_arr(&self) -> Option<&Vec<Val>> {
        match self {
            Val::Arr(a) => Some(a),
            _ => None,
        }
    }
    /// `typeof v === 'object'` (null included, as JS says).
    pub fn is_object_like(&self) -> bool {
        matches!(self, Val::Null | Val::Arr(_) | Val::Obj(_))
    }
    /// `obj[k]` on any value (undefined unless an object; arrays answer numeric keys and `length`).
    pub fn at(&self, k: &str) -> Val {
        match self {
            Val::Obj(o) => o.at(k),
            Val::Arr(a) => {
                if k == "length" {
                    return Val::Num(a.len() as f64);
                }
                index_key(k).and_then(|i| usize::try_from(i).ok()).and_then(|i| a.get(i)).cloned().unwrap_or(Val::Undef)
            }
            Val::Str(s) if k == "length" => Val::Num(s.encode_utf16().count() as f64),
            _ => Val::Undef,
        }
    }
    /// ToNumber (`+v`).
    pub fn num(&self) -> f64 {
        match self {
            Val::Undef => f64::NAN,
            Val::Null => 0.0,
            Val::Bool(b) => f64::from(u8::from(*b)),
            Val::Num(n) => *n,
            Val::Str(s) => cc_cad::js::str_to_number(s),
            Val::Arr(a) => match a.as_slice() {
                [] => 0.0,
                [x] => cc_cad::js::str_to_number(&x.to_js_string()),
                _ => f64::NAN,
            },
            Val::Obj(_) => f64::NAN,
        }
    }
    /// ToString (`String(v)`).
    pub fn to_js_string(&self) -> String {
        match self {
            Val::Undef => "undefined".into(),
            Val::Null => "null".into(),
            Val::Bool(b) => b.to_string(),
            Val::Num(n) => cc_cad::js::num_to_string(*n),
            Val::Str(s) => s.clone(),
            Val::Arr(a) => {
                let mut out = String::new();
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if !matches!(x, Val::Undef | Val::Null) {
                        let _ = write!(out, "{}", x.to_js_string());
                    }
                }
                out
            }
            Val::Obj(_) => "[object Object]".into(),
        }
    }
}

/// `parseFloat(String(v))`: the longest decimal prefix after leading white space (Infinity too), else NaN.
pub fn parse_float(v: &Val) -> f64 {
    match v {
        Val::Num(n) => *n,
        _ => parse_float_str(&v.to_js_string()),
    }
}
pub fn parse_float_str(s: &str) -> f64 {
    let t = cc_cad::js::trim_start(s);
    let b = t.as_bytes();
    let mut i = 0usize;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i = 1;
    }
    if t.get(i..).is_some_and(|r| r.starts_with("Infinity")) {
        return if b.first() == Some(&b'-') { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let start = i;
    let mut digits = 0usize;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i = i.saturating_add(1);
        digits = digits.saturating_add(1);
    }
    if b.get(i) == Some(&b'.') {
        let mut j = i.saturating_add(1);
        let mut fd = 0usize;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j = j.saturating_add(1);
            fd = fd.saturating_add(1);
        }
        if digits > 0 || fd > 0 {
            i = j;
            digits = digits.saturating_add(fd);
        }
    }
    if digits == 0 {
        return f64::NAN;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i.saturating_add(1);
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j = j.saturating_add(1);
        }
        let es = j;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j = j.saturating_add(1);
        }
        if j > es {
            i = j;
        }
    }
    let _ = start;
    cc_cad::js::str_to_number(t.get(..i).unwrap_or(""))
}

/// `parseInt(String(v), 10)`.
pub fn parse_int(v: &Val) -> f64 {
    let s = v.to_js_string();
    let t = cc_cad::js::trim_start(&s);
    let b = t.as_bytes();
    let mut i = 0usize;
    let neg = b.first() == Some(&b'-');
    if matches!(b.first(), Some(b'+' | b'-')) {
        i = 1;
    }
    let start = i;
    let mut acc = 0.0f64;
    while let Some(d) = b.get(i).filter(|c| c.is_ascii_digit()) {
        acc = acc * 10.0 + f64::from(d.saturating_sub(b'0'));
        i = i.saturating_add(1);
    }
    if i == start {
        return f64::NAN;
    }
    // a long digit run: parse the text exactly
    if i.saturating_sub(start) > 15 {
        acc = t.get(start..i).and_then(|d| d.parse::<f64>().ok()).unwrap_or(acc);
    }
    if neg { -acc } else { acc }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn floats() {
        assert_eq!(parse_float_str(" 2.5abc"), 2.5);
        assert_eq!(parse_float_str("-.5"), -0.5);
        assert!(parse_float_str("abc").is_nan());
        assert_eq!(parse_float_str("1e3x"), 1000.0);
        assert_eq!(parse_float_str("1e"), 1.0);
        assert_eq!(parse_int(&Val::str("12.7")), 12.0);
    }
    #[test]
    fn key_order() {
        let mut p = Props::new();
        p.set("b", Val::Num(1.0));
        p.set("2", Val::Num(1.0));
        p.set("a", Val::Num(1.0));
        p.set("1", Val::Num(1.0));
        assert_eq!(p.keys(), vec!["1", "2", "b", "a"]);
    }
}
