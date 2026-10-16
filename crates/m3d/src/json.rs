//! JSON the way the JS kernel makes and reads it.
//!
//! Writing: [`Out`] appends JSON text with `JSON.stringify`'s rules (numbers as `String(x)`, −0 as `0`, NaN and ±∞ as
//! `null`, the same string escapes) — the undo snapshots compare and re-read these strings, so −0 and NaN go through
//! them exactly as in the JS. In `canon` mode a non-finite number is written as the parity kit's `"$NaN"` / `"$Inf"` /
//! `"$-Inf"` instead (what `canon()` makes of the live JS objects).
//!
//! Reading: helpers that ask a `serde_json::Value` the questions model.js asks of parsed JSON (`Number.isInteger`,
//! `isNum`, `isPt`, truthiness).

use serde_json::Value;

use crate::js;
use crate::vec::P3;

/// A JSON text being written.
pub struct Out {
    pub s: String,
    pub canon: bool,
}

impl Out {
    pub fn new(canon: bool) -> Self {
        Self { s: String::new(), canon }
    }
    #[inline]
    pub fn raw(&mut self, t: &str) {
        self.s.push_str(t);
    }
    #[inline]
    pub fn ch(&mut self, c: char) {
        self.s.push(c);
    }
    pub fn num(&mut self, x: f64) {
        if x.is_finite() {
            js::push_num(&mut self.s, x);
        } else if !self.canon {
            self.s.push_str("null");
        } else if x.is_nan() {
            self.s.push_str("\"$NaN\"");
        } else if x > 0.0 {
            self.s.push_str("\"$Inf\"");
        } else {
            self.s.push_str("\"$-Inf\"");
        }
    }
    pub fn str(&mut self, t: &str) {
        let s = &mut self.s;
        s.push('"');
        for c in t.chars() {
            match c {
                '"' => s.push_str("\\\""),
                '\\' => s.push_str("\\\\"),
                '\u{08}' => s.push_str("\\b"),
                '\u{0c}' => s.push_str("\\f"),
                '\n' => s.push_str("\\n"),
                '\r' => s.push_str("\\r"),
                '\t' => s.push_str("\\t"),
                c if (c as u32) < 0x20 => {
                    use std::fmt::Write as _;
                    let _ = write!(s, "\\u{:04x}", c as u32);
                }
                c => s.push(c),
            }
        }
        s.push('"');
    }
    /// `"key":`
    #[inline]
    pub fn key(&mut self, k: &str) {
        self.str(k);
        self.s.push(':');
    }
    pub fn bool(&mut self, b: bool) {
        self.s.push_str(if b { "true" } else { "false" });
    }
    pub fn p3(&mut self, p: &P3) {
        self.s.push('[');
        self.num(p[0]);
        self.s.push(',');
        self.num(p[1]);
        self.s.push(',');
        self.num(p[2]);
        self.s.push(']');
    }
    pub fn nums(&mut self, xs: &[f64]) {
        self.s.push('[');
        for (i, x) in xs.iter().enumerate() {
            if i > 0 {
                self.s.push(',');
            }
            self.num(*x);
        }
        self.s.push(']');
    }
    pub fn strs(&mut self, xs: &[String]) {
        self.s.push('[');
        for (i, x) in xs.iter().enumerate() {
            if i > 0 {
                self.s.push(',');
            }
            self.str(x);
        }
        self.s.push(']');
    }
    /// A `serde_json::Value` (numbers through `String(x)`).
    pub fn value(&mut self, v: &Value) {
        match v {
            Value::Null => self.raw("null"),
            Value::Bool(b) => self.bool(*b),
            Value::Number(n) => self.num(n.as_f64().unwrap_or(f64::NAN)),
            Value::String(s) => self.str(s),
            Value::Array(a) => {
                self.ch('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        self.ch(',');
                    }
                    self.value(x);
                }
                self.ch(']');
            }
            Value::Object(o) => {
                self.ch('{');
                for (i, (k, x)) in o.iter().enumerate() {
                    if i > 0 {
                        self.ch(',');
                    }
                    self.key(k);
                    self.value(x);
                }
                self.ch('}');
            }
        }
    }
}

/// A number a JSON value holds.
#[inline]
pub fn num(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64)
}
/// `isNum(x)`: a finite number under 1e7 in size.
#[inline]
pub fn is_num(v: Option<&Value>) -> Option<f64> {
    num(v).filter(|x| x.is_finite() && x.abs() < 1e7)
}
/// `Number.isInteger(x)`.
#[inline]
pub fn int(v: Option<&Value>) -> Option<f64> {
    num(v).filter(|x| js::is_integer(*x))
}
/// `Number.isFinite(x)`.
#[inline]
pub fn finite(v: Option<&Value>) -> Option<f64> {
    num(v).filter(|x| x.is_finite())
}
/// `isPt(p)`: an array of three `isNum` numbers.
pub fn is_pt(v: Option<&Value>) -> Option<P3> {
    let a = v?.as_array()?;
    if a.len() != 3 {
        return None;
    }
    Some([is_num(a.first())?, is_num(a.get(1))?, is_num(a.get(2))?])
}
/// JS truthiness of a value (absent = falsy).
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(js::truthy),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}
/// `typeof x === 'string'`.
#[inline]
pub fn str(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str)
}
/// `x === true`.
#[inline]
pub fn is_true(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Bool(true)))
}
/// `x !== false`.
#[inline]
pub fn not_false(v: Option<&Value>) -> bool {
    !matches!(v, Some(Value::Bool(false)))
}
/// `cleanText(x, max)` of a JSON value ('' unless a string).
pub fn clean(v: Option<&Value>, max: usize) -> String {
    str(v).map_or_else(String::new, |s| js::clean_text(s, max))
}
