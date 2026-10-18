//! `JSON.stringify` as V8 writes it (number text, string escaping, the indent form), so the messages this server sends
//! are byte for byte what the 0.5 JavaScript server sends for the same values.

use serde_json::Value;

/// Nesting deeper than this is written compact (no message the dispatcher builds is near it; it bounds recursion on
/// values a client sent back to us).
const MAX_DEPTH: usize = 128;

/// `JSON.stringify(v)`.
pub fn stringify(v: &Value) -> String {
    let mut out = String::new();
    write(&mut out, v, 0);
    out
}

/// `JSON.stringify(v, null, indent)`.
pub fn stringify_indent(v: &Value, indent: usize) -> String {
    let mut out = String::new();
    write_indent(&mut out, v, indent.min(10), 0);
    out
}

/// A JSON number as JavaScript prints it (`String(x)`; integers stay integers).
pub fn push_number(out: &mut String, n: &serde_json::Number) {
    if let Some(i) = n.as_i64() {
        // exact for |i| ≤ 2^53; beyond that JavaScript would have rounded it already, as as_f64 does
        if i.unsigned_abs() <= (1u64 << 53) {
            out.push_str(&i.to_string());
            return;
        }
    } else if let Some(u) = n.as_u64()
        && u <= (1u64 << 53)
    {
        out.push_str(&u.to_string());
        return;
    }
    let f = n.as_f64().unwrap_or(f64::NAN);
    if f.is_finite() {
        cc_cad::js::push_num(out, if f == 0.0 { 0.0 } else { f });
    } else {
        out.push_str("null");
    }
}

fn write(out: &mut String, v: &Value, depth: usize) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => push_number(out, n),
        Value::String(s) => quote(out, s),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if depth >= MAX_DEPTH {
                    out.push_str("null");
                } else {
                    write(out, x, depth.saturating_add(1));
                }
            }
            out.push(']');
        }
        Value::Object(o) => {
            out.push('{');
            for (i, (k, x)) in o.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                quote(out, k);
                out.push(':');
                if depth >= MAX_DEPTH {
                    out.push_str("null");
                } else {
                    write(out, x, depth.saturating_add(1));
                }
            }
            out.push('}');
        }
    }
}

fn newline(out: &mut String, indent: usize, depth: usize) {
    out.push('\n');
    for _ in 0..indent.saturating_mul(depth) {
        out.push(' ');
    }
}

fn write_indent(out: &mut String, v: &Value, indent: usize, depth: usize) {
    if indent == 0 || depth >= MAX_DEPTH {
        return write(out, v, depth);
    }
    let inner = depth.saturating_add(1);
    match v {
        Value::Array(a) if !a.is_empty() => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, inner);
                write_indent(out, x, indent, inner);
            }
            newline(out, indent, depth);
            out.push(']');
        }
        Value::Object(o) if !o.is_empty() => {
            out.push('{');
            for (i, (k, x)) in o.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, inner);
                quote(out, k);
                out.push_str(": ");
                write_indent(out, x, indent, inner);
            }
            newline(out, indent, depth);
            out.push('}');
        }
        _ => write(out, v, depth),
    }
}

/// String quoting as `JSON.stringify` does it (lone surrogates cannot occur in a Rust string).
pub fn quote(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
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

/// JavaScript's `String(value)` for the values schema messages print (numbers and strings).
pub fn js_string(v: &Value) -> String {
    match v {
        Value::Number(n) => {
            let mut s = String::new();
            push_number(&mut s, n);
            s
        }
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Array(a) => a.iter().map(js_string).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// The length of a string as JavaScript counts it (UTF-16 code units).
pub fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `s.slice(0, n)` in UTF-16 code units (a surrogate pair cut in half is dropped rather than split).
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn like_v8() {
        let v = json!({"a": 1, "b": [1.5, -0.0, 1e21, "x\"\n\u{1}"], "c": {}, "d": []});
        assert_eq!(stringify(&v), r#"{"a":1,"b":[1.5,0,1e+21,"x\"\n\u0001"],"c":{},"d":[]}"#);
        assert_eq!(
            stringify_indent(&json!({"a": [1, {"b": null}], "c": {}}), 1),
            "{\n \"a\": [\n  1,\n  {\n   \"b\": null\n  }\n ],\n \"c\": {}\n}"
        );
        assert_eq!(js_string(&json!(0.1)), "0.1");
        assert_eq!(slice16("a😀b", 2), "a");
    }
}
