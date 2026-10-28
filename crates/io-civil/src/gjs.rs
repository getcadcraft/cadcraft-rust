//! JavaScript object semantics the GIS modules meet: `Object.keys` order (array-index keys first, ascending, then
//! the rest in insertion order), `JSON.stringify` (that key order, V8's number text and string escapes), and
//! little-endian / big-endian byte access that never panics.

use serde_json::{Map, Value};

/// Is `k` an array index ("0" … "4294967294", canonical)?
pub fn is_index(k: &str) -> bool {
    if !k.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return false;
    }
    if k.is_empty() || k.len() > 10 || !k.bytes().all(|b| b.is_ascii_digit()) || (k.len() > 1 && k.starts_with('0')) {
        return false;
    }
    k.parse::<u64>().is_ok_and(|n| n < 4_294_967_295)
}

/// `Object.keys(o)` order.
pub fn js_keys(m: &Map<String, Value>) -> Vec<&String> {
    let mut idx: Vec<&String> = m.keys().filter(|k| is_index(k)).collect();
    idx.sort_by_key(|k| k.parse::<u64>().unwrap_or(0));
    idx.extend(m.keys().filter(|k| !is_index(k)));
    idx
}

/// Insertion of `k` into a JS-ordered key list (for objects built field by field): position is decided at output.
pub fn js_entries(m: &Map<String, Value>) -> Vec<(&String, &Value)> {
    js_keys(m).into_iter().filter_map(|k| m.get(k).map(|v| (k, v))).collect()
}

/// `JSON.stringify(s)` for a string.
pub fn push_str_json(out: &mut String, s: &str) {
    out.push('"');
    if !s.bytes().any(|b| b < 0x20 || b == b'"' || b == b'\\') {
        out.push_str(s);
        out.push('"');
        return;
    }
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = std::fmt::Write::write_fmt(out, format_args!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `JSON.stringify(v)` (no spaces). NaN / ±∞ (as the kit's "$NaN" strings never appear here) print as null.
pub fn stringify(v: &Value) -> String {
    let mut out = String::new();
    push_json(&mut out, v);
    out
}
pub fn push_json(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            if x.is_finite() {
                cc_cad::js::push_num(out, x);
            } else {
                out.push_str("null");
            }
        }
        Value::String(s) => push_str_json(out, s),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_json(out, x);
            }
            out.push(']');
        }
        Value::Object(m) => {
            out.push('{');
            let mut put = |i: usize, k: &str, x: &Value| {
                if i > 0 {
                    out.push(',');
                }
                push_str_json(out, k);
                out.push(':');
                push_json(out, x);
            };
            // most objects have no array-index keys: their order is the insertion order
            if m.keys().any(|k| is_index(k)) {
                for (i, (k, x)) in js_entries(m).into_iter().enumerate() {
                    put(i, k, x);
                }
            } else {
                for (i, (k, x)) in m.iter().enumerate() {
                    put(i, k, x);
                }
            }
            out.push('}');
        }
    }
}

/// A JS number as a JSON value (None for NaN / ±∞, which JSON cannot hold).
pub fn num(x: f64) -> Value {
    serde_json::Number::from_f64(if x == 0.0 { 0.0 } else { x }).map_or(Value::Null, Value::Number)
}
/// A JS number as a JSON value keeping −0 (JSON.parse gives −0 back).
pub fn num_keep(x: f64) -> Value {
    serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
}

// ---------------------------------------------------------------------------- bytes

pub fn u8_at(b: &[u8], i: usize) -> u8 {
    b.get(i).copied().unwrap_or(0)
}
fn arr4(b: &[u8], i: usize) -> [u8; 4] {
    let mut a = [0u8; 4];
    if let Some(s) = b.get(i..i.saturating_add(4)) {
        a.copy_from_slice(s);
    }
    a
}
fn arr8(b: &[u8], i: usize) -> [u8; 8] {
    let mut a = [0u8; 8];
    if let Some(s) = b.get(i..i.saturating_add(8)) {
        a.copy_from_slice(s);
    }
    a
}
pub fn u16le(b: &[u8], i: usize) -> u16 {
    let mut a = [0u8; 2];
    if let Some(s) = b.get(i..i.saturating_add(2)) {
        a.copy_from_slice(s);
    }
    u16::from_le_bytes(a)
}
pub fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(arr4(b, i))
}
pub fn u32be(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes(arr4(b, i))
}
pub fn i32le(b: &[u8], i: usize) -> i32 {
    i32::from_le_bytes(arr4(b, i))
}
pub fn i32be(b: &[u8], i: usize) -> i32 {
    i32::from_be_bytes(arr4(b, i))
}
pub fn f64le(b: &[u8], i: usize) -> f64 {
    f64::from_le_bytes(arr8(b, i))
}
pub fn f64be(b: &[u8], i: usize) -> f64 {
    f64::from_be_bytes(arr8(b, i))
}
pub fn u32_at(b: &[u8], i: usize, le: bool) -> u32 {
    if le { u32le(b, i) } else { u32be(b, i) }
}
pub fn f64_at(b: &[u8], i: usize, le: bool) -> f64 {
    if le { f64le(b, i) } else { f64be(b, i) }
}

/// Writes into a byte buffer that never panics (out of range = dropped, as a DataView write past the end throws in
/// JS — never reached by the writers here, which size their buffers first).
pub fn put(b: &mut [u8], i: usize, bytes: &[u8]) {
    if let Some(s) = b.get_mut(i..i.saturating_add(bytes.len())) {
        s.copy_from_slice(bytes);
    }
}

/// Lossy UTF-8 as `new TextDecoder().decode(bytes)` (BOM dropped, bad sequences → U+FFFD).
pub fn utf8_lossy(b: &[u8]) -> String {
    let b = b.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(b);
    String::from_utf8_lossy(b).into_owned()
}

/// `String(v).replace(/[\u0000-\u001f\u007f]/g, rep)` with rep = "".
pub fn drop_controls(s: &str) -> String {
    if !s.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return s.to_string();
    }
    s.chars().filter(|&c| !((c as u32) < 0x20 || c == '\u{7f}')).collect()
}
/// `.replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, ' ')` (tab, LF and CR kept).
pub fn soft_controls(s: &str) -> String {
    if !s.bytes().any(|b| (b < 0x20 && b != b'\t' && b != b'\n' && b != b'\r') || b == 0x7f) {
        return s.to_string();
    }
    s.chars()
        .map(|c| {
            let u = c as u32;
            if u <= 8 || u == 0x0b || u == 0x0c || (0x0e..=0x1f).contains(&u) || u == 0x7f { ' ' } else { c }
        })
        .collect()
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
/// `s.slice(0, n)` in UTF-16 units (cc_civil's slice16), without the walk when the bytes already fit.
pub fn cut16(s: &str, n: usize) -> String {
    if s.len() <= n { s.to_string() } else { cc_civil::js::slice16(s, n) }
}

/// Standard base64 with padding.
pub fn b64(b: &[u8]) -> String {
    let mut out = String::with_capacity((b.len().saturating_mul(4) / 3).saturating_add(4));
    for ch in b.chunks(3) {
        let n = (u32::from(ch.first().copied().unwrap_or(0)) << 16)
            | (u32::from(ch.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(ch.get(2).copied().unwrap_or(0));
        let c = |s: u32| char::from(B64.get(usize::try_from((n >> s) & 63).unwrap_or(0)).copied().unwrap_or(b'A'));
        out.push(c(18));
        out.push(c(12));
        out.push(if ch.len() > 1 { c(6) } else { '=' });
        out.push(if ch.len() > 2 { c(0) } else { '=' });
    }
    out
}
/// Base64 decode (standard alphabet; bad characters skipped).
pub fn unb64(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len().saturating_mul(3) / 4);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c.saturating_sub(b'A'),
            b'a'..=b'z' => c.saturating_sub(b'a').saturating_add(26),
            b'0'..=b'9' => c.saturating_sub(b'0').saturating_add(52),
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        };
        acc = ((acc << 6) | u32::from(v)) & 0x00ff_ffff;
        bits = bits.saturating_add(6);
        if bits >= 8 {
            bits = bits.saturating_sub(8);
            out.push(u8::try_from((acc >> bits) & 0xff).unwrap_or(0));
        }
    }
    out
}

pub fn u16be_at(b: &[u8], i: usize) -> u16 {
    let mut a = [0u8; 2];
    if let Some(s) = b.get(i..i.saturating_add(2)) {
        a.copy_from_slice(s);
    }
    u16::from_be_bytes(a)
}
pub fn u32be_at(b: &[u8], i: usize) -> u32 {
    u32be(b, i)
}
