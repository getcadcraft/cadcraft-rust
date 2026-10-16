//! The raw parameter values of step.js (the form every other cc-ifc module reads):
//!
//! | STEP | JS | Rust |
//! |---|---|---|
//! | `$` | `null` | [`Value::Null`] |
//! | `*` | `DERIVED` (`{derived: true}`) | [`Value::Derived`] |
//! | `1`, `-2.5`, `1.E-3` | number | [`Value::Num`] |
//! | `'text'` | string (UTF-16) | [`Value::Str`] |
//! | `#n` | `{ref: n}` | [`Value::Ref`] (a JS number: ids may be any size) |
//! | `.X.` | `{enum: 'X'}` | [`Value::Enum`] |
//! | `"0AF"` | `{bin: 'hex'}` | [`Value::Bin`] |
//! | `IFCLABEL('x')` | `{typed, value}` | [`Value::Typed`] |
//! | `( … )` | array | [`Value::List`] |

use crate::util::{push_json_num, push_json_str, push_json_str16};

/// A JS string: UTF-8 when it is valid Unicode, else its UTF-16 code units (lone surrogates from `\X2\D800\X0\`).
#[derive(Clone, Debug, PartialEq)]
pub enum JsStr {
    Utf8(Box<str>),
    Utf16(Box<[u16]>),
}

impl Default for JsStr {
    fn default() -> Self {
        JsStr::Utf8("".into())
    }
}

impl JsStr {
    /// From UTF-16 code units (UTF-8 when they are valid).
    pub fn from_utf16(u: Vec<u16>) -> JsStr {
        match String::from_utf16(&u) {
            Ok(s) => JsStr::Utf8(s.into_boxed_str()),
            Err(_) => JsStr::Utf16(u.into_boxed_slice()),
        }
    }
    /// UTF-16 code units.
    pub fn units(&self) -> Vec<u16> {
        match self {
            JsStr::Utf8(s) => s.encode_utf16().collect(),
            JsStr::Utf16(u) => u.to_vec(),
        }
    }
    /// The text (lone surrogates as U+FFFD: only for comparisons with ASCII names).
    pub fn lossy(&self) -> std::borrow::Cow<'_, str> {
        match self {
            JsStr::Utf8(s) => std::borrow::Cow::Borrowed(s),
            JsStr::Utf16(u) => std::borrow::Cow::Owned(String::from_utf16_lossy(u)),
        }
    }
    /// `s === ''`
    pub fn is_empty(&self) -> bool {
        match self {
            JsStr::Utf8(s) => s.is_empty(),
            JsStr::Utf16(u) => u.is_empty(),
        }
    }
    pub fn push_json(&self, out: &mut String) {
        match self {
            JsStr::Utf8(s) => push_json_str(out, s),
            JsStr::Utf16(u) => push_json_str16(out, u),
        }
    }
}

/// One raw parameter value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Derived,
    Num(f64),
    Str(JsStr),
    Ref(f64),
    Enum(Box<str>),
    Bin(Box<str>),
    Typed(Box<(Box<str>, Value)>),
    List(Box<[Value]>),
}

impl Value {
    /// `{ref: n}` → n
    pub fn as_ref_id(&self) -> Option<f64> {
        if let Value::Ref(n) = self { Some(*n) } else { None }
    }
    pub fn as_list(&self) -> Option<&[Value]> {
        if let Value::List(l) = self { Some(l) } else { None }
    }
    /// JS truthiness of the raw value (`null` and `''` and `0` / NaN are falsy; objects and arrays truthy).
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Num(n) => *n != 0.0 && !n.is_nan(),
            Value::Str(s) => !s.is_empty(),
            _ => true,
        }
    }

    /// The kit's canonical JSON of the raw value (`canon()`: keys sorted, numbers round-trip, `-0` → 0).
    pub fn push_json(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Derived => out.push_str("{\"derived\":true}"),
            Value::Num(n) => push_json_num(out, *n),
            Value::Str(s) => s.push_json(out),
            Value::Ref(n) => {
                out.push_str("{\"ref\":");
                push_json_num(out, *n);
                out.push('}');
            }
            Value::Enum(e) => {
                out.push_str("{\"enum\":");
                push_json_str(out, e);
                out.push('}');
            }
            Value::Bin(e) => {
                out.push_str("{\"bin\":");
                push_json_str(out, e);
                out.push('}');
            }
            Value::Typed(t) => {
                out.push_str("{\"typed\":");
                push_json_str(out, &t.0);
                out.push_str(",\"value\":");
                t.1.push_json(out);
                out.push('}');
            }
            Value::List(l) => push_list_json(out, l),
        }
    }
}

/// `[v, …]` as JSON.
pub fn push_list_json(out: &mut String, l: &[Value]) {
    out.push('[');
    for (k, v) in l.iter().enumerate() {
        if k > 0 {
            out.push(',');
        }
        v.push_json(out);
    }
    out.push(']');
}
