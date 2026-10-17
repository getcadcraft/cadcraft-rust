//! JavaScript values over `serde_json::Value`: the takeoff's state is plain JSON data in the JS (`model.js` keeps it as
//! objects and clones it through `JSON.parse(JSON.stringify(…))`), so the port keeps it as a `Value` with the JS key
//! order (`preserve_order`) and these helpers give the JS semantics the modules rely on: `typeof x === 'number'`,
//! truthiness, `String(x)`, `Number.isInteger`, `JSON.stringify` with V8's number text.

use serde_json::{Map, Value};

pub type Obj = Map<String, Value>;

/// A JS number as a JSON value. NaN and ±Infinity become `null`, as `JSON.stringify` writes them (a state never keeps
/// them across an undo snapshot).
#[inline]
pub fn n(x: f64) -> Value {
    serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
}
/// A string value.
#[inline]
pub fn s(x: impl Into<String>) -> Value {
    Value::String(x.into())
}

/// `typeof v === 'number'` → its value.
#[inline]
pub fn num(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(x)) => x.as_f64(),
        _ => None,
    }
}
/// `typeof v === 'number' && Number.isFinite(v)`.
#[inline]
pub fn finite(v: Option<&Value>) -> Option<f64> {
    num(v).filter(|x| x.is_finite())
}
/// `Number.isInteger(v)`.
#[inline]
pub fn int(v: Option<&Value>) -> Option<f64> {
    num(v).filter(|x| x.is_finite() && x.trunc() == *x)
}
/// `typeof v === 'string'` → its value.
#[inline]
pub fn str(v: Option<&Value>) -> Option<&str> {
    match v {
        Some(Value::String(x)) => Some(x.as_str()),
        _ => None,
    }
}
/// JS truthiness of a property (absent = undefined = falsy).
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(x)) => x.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(x)) => !x.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}
/// `v` is an object (not null, not an array): `v && typeof v === 'object' && !Array.isArray(v)`.
#[inline]
pub fn obj(v: Option<&Value>) -> Option<&Obj> {
    match v {
        Some(Value::Object(o)) => Some(o),
        _ => None,
    }
}
/// `Array.isArray(v)`.
#[inline]
pub fn arr(v: Option<&Value>) -> Option<&Vec<Value>> {
    match v {
        Some(Value::Array(a)) => Some(a),
        _ => None,
    }
}
/// `v && typeof v === 'object'` (arrays too).
#[inline]
pub fn is_objlike(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Object(_) | Value::Array(_)))
}
/// `x ?? d` for a nullish property.
#[inline]
pub fn nullish(v: Option<&Value>) -> bool {
    matches!(v, None | Some(Value::Null))
}

/// `String(v)` (absent = "undefined").
pub fn to_string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => (if *b { "true" } else { "false" }).into(),
        Some(Value::Number(x)) => num_str(x.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(x)) => x.clone(),
        Some(Value::Array(a)) => {
            a.iter().map(|x| if x.is_null() { String::new() } else { to_string(Some(x)) }).collect::<Vec<_>>().join(",")
        }
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}
/// `String(v ?? '')`.
pub fn to_string_or_empty(v: Option<&Value>) -> String {
    if nullish(v) { String::new() } else { to_string(v) }
}

/// `Number(v)` for the values a JSON file can hold.
pub fn to_number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(x)) => x.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(x)) => str_to_number(x),
        Some(Value::Array(a)) => match a.as_slice() {
            [] => 0.0,
            [one] => to_number(Some(&s(to_string(Some(one))))),
            _ => f64::NAN,
        },
        Some(Value::Object(_)) => f64::NAN,
    }
}

/// `Number(string)`: StringToNumber (trimmed; empty = 0; decimal, `Infinity`, 0x / 0o / 0b integers).
pub fn str_to_number(s0: &str) -> f64 {
    let s = cc_m3d::js::trim(s0);
    if s.is_empty() {
        return 0.0;
    }
    let (sign, body) = match s.as_bytes().first() {
        Some(b'-') => (-1.0, s.get(1..).unwrap_or("")),
        Some(b'+') => (1.0, s.get(1..).unwrap_or("")),
        _ => (1.0, s),
    };
    if body == "Infinity" {
        return sign * f64::INFINITY;
    }
    let lower = s.get(..2).map(str::to_ascii_lowercase);
    if let Some(p) = lower.as_deref()
        && (p == "0x" || p == "0o" || p == "0b")
    {
        let radix = match p {
            "0x" => 16,
            "0o" => 8,
            _ => 2,
        };
        let digits = s.get(2..).unwrap_or("");
        if digits.is_empty() {
            return f64::NAN;
        }
        let mut v = 0.0f64;
        for c in digits.chars() {
            match c.to_digit(radix) {
                Some(d) => v = v * f64::from(radix) + f64::from(d),
                None => return f64::NAN,
            }
        }
        return v;
    }
    // StrDecimalLiteral: digits [. digits] [e[+-]digits] | . digits [exp]
    let b = body.as_bytes();
    let mut i = 0usize;
    let mut int_digits = 0usize;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i = i.saturating_add(1);
        int_digits = int_digits.saturating_add(1);
    }
    let mut frac_digits = 0usize;
    if b.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i = i.saturating_add(1);
            frac_digits = frac_digits.saturating_add(1);
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return f64::NAN;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i.saturating_add(1);
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j = j.saturating_add(1);
        }
        let start = j;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j = j.saturating_add(1);
        }
        if j == start {
            return f64::NAN;
        }
        i = j;
    }
    if i != b.len() {
        return f64::NAN;
    }
    body.parse::<f64>().map_or(f64::NAN, |v| sign * v)
}

/// `parseFloat(s)` of a string that starts with a decimal literal (the callers match `\d+(\.\d*)?|\.\d+` first).
pub fn parse_float(s: &str) -> f64 {
    let t = s.trim_end_matches('.');
    let t = if t.is_empty() { "0" } else { t };
    t.parse::<f64>().unwrap_or(f64::NAN)
}
/// `parseInt(s, 10)` of ASCII digits (the callers check them).
pub fn parse_int(s: &str) -> f64 {
    let mut v = 0.0f64;
    for c in s.chars() {
        match c.to_digit(10) {
            Some(d) => v = v * 10.0 + f64::from(d),
            None => break,
        }
    }
    v
}

/// The length of a string in UTF-16 code units (`s.length`).
#[inline]
pub fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `JSON.stringify(v)` with V8's number text and string escaping.
pub fn stringify(v: &Value) -> String {
    let mut out = String::new();
    write_json(&mut out, v);
    out
}
fn write_json(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(x) => {
            let f = x.as_f64().unwrap_or(f64::NAN);
            if f.is_finite() {
                out.push_str(&num_str(if f == 0.0 { 0.0 } else { f }));
            } else {
                out.push_str("null");
            }
        }
        Value::String(s) => quote(out, s),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(out, x);
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
                write_json(out, x);
            }
            out.push('}');
        }
    }
}
/// JSON string quoting as `JSON.stringify` does it.
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

/// `JSON.parse(s)` for a file: refuses nesting deeper than `max_depth` (a hostile file cannot overflow the stack).
///
/// # Errors
/// The parser's message when the text is not JSON, or "too deep".
pub fn parse_json(text: &str, max_depth: usize) -> Result<Value, String> {
    // serde_json's own recursion limit is 128; check depth first so the limit is ours and stated
    let mut depth = 0usize;
    let mut max = 0usize;
    let mut in_str = false;
    let mut esc = false;
    for b in text.bytes() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'[' | b'{' => {
                depth = depth.saturating_add(1);
                max = max.max(depth);
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    if max > max_depth {
        return Err(format!("JSON nested deeper than {max_depth}"));
    }
    serde_json::from_str(text).map_err(|e| e.to_string())
}

/// Set a key (`o.k = v`): an existing key keeps its place, a new one goes last (JS property order).
#[inline]
pub fn set(o: &mut Obj, k: &str, v: Value) {
    o.insert(k.to_string(), v);
}
/// `delete o.k` keeping the order of the other keys.
#[inline]
pub fn del(o: &mut Obj, k: &str) {
    o.shift_remove(k);
}
/// `Object.assign(target, src)`.
pub fn assign(target: &mut Obj, src: &Obj) {
    for (k, v) in src {
        target.insert(k.clone(), v.clone());
    }
}
/// `{ ...a, ...b }`.
pub fn spread(a: &Obj, b: &Obj) -> Obj {
    let mut o = a.clone();
    assign(&mut o, b);
    o
}

/// A JS `===` between two JSON values of the kinds a takeoff holds (objects and arrays by identity: never equal here).
pub fn strict_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) | (Some(Value::Null), Some(Value::Null)) => true,
        (Some(Value::Bool(x)), Some(Value::Bool(y))) => x == y,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(Value::String(x)), Some(Value::String(y))) => x == y,
        _ => false,
    }
}

/// Characters as UTF-16 code units → the same JS string index semantics (`s[i]`, `s.slice(i)`).
pub fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}
/// UTF-16 units back to a string (a lone surrogate becomes U+FFFD: Spec gap, a Rust string cannot hold it).
pub fn from16(u: &[u16]) -> String {
    String::from_utf16_lossy(u)
}

/// `s.toLowerCase()` (Unicode default case mapping, as V8 does without a locale).
pub fn lower(s: &str) -> String {
    s.to_lowercase()
}
/// `s.toUpperCase()`.
pub fn upper(s: &str) -> String {
    s.to_uppercase()
}

/// JS `\s` (WhiteSpace + LineTerminator).
#[inline]
pub fn is_space(c: char) -> bool {
    cc_m3d::js::is_ws(c)
}

/// `String(x)` for a number (ECMAScript Number::toString). The shortest round-trip digits, and where two candidates
/// are equally near (x exactly halfway between two k-digit decimals) the one with an even last digit, as the spec
/// says (Rust's formatter can pick the other one).
pub fn num_str(x: f64) -> String {
    let base = cc_m3d::js::num_str(x);
    if !x.is_finite() || x == 0.0 {
        return base;
    }
    let sh = format!("{:e}", x.abs());
    let (mant, exp) = sh.split_once('e').unwrap_or((sh.as_str(), "0"));
    let digits: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).collect();
    let k = digits.len();
    if k < 2 {
        return base;
    }
    let e: i64 = exp.parse().unwrap_or(0);
    let exact = format!("{:.1100e}", x.abs());
    let (xm, xe) = exact.split_once('e').unwrap_or((exact.as_str(), "0"));
    let xd: Vec<u8> = xm.bytes().filter(u8::is_ascii_digit).collect();
    if xe.parse::<i64>().unwrap_or(0) != e {
        return base;
    }
    let tail = xd.get(k..).unwrap_or(&[]);
    if tail.first() != Some(&b'5') || !tail.iter().skip(1).all(|c| *c == b'0') {
        return base;
    }
    // a tie: floor (the exact digits cut at k) or ceil (one more in the last place); the even one
    let floor: Vec<u8> = xd.get(..k).unwrap_or(&[]).to_vec();
    let mut ceil = floor.clone();
    let mut carry = true;
    for c in ceil.iter_mut().rev() {
        if !carry {
            break;
        }
        if *c == b'9' {
            *c = b'0';
        } else {
            *c = c.saturating_add(1);
            carry = false;
        }
    }
    let (chosen, n) = if floor.last().is_some_and(|d| d.wrapping_sub(b'0').is_multiple_of(2)) {
        (floor, e.saturating_add(1))
    } else if carry {
        let mut c = vec![b'1'];
        c.extend(ceil.iter().take(k.saturating_sub(1)));
        (c, e.saturating_add(2))
    } else {
        (ceil, e.saturating_add(1))
    };
    if chosen == digits {
        return base;
    }
    let mut ds: Vec<u8> = chosen;
    while ds.len() > 1 && ds.last() == Some(&b'0') {
        ds.pop();
    }
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    let kk = i64::try_from(ds.len()).unwrap_or(1);
    let dstr = String::from_utf8(ds.clone()).unwrap_or_default();
    if kk <= n && n <= 21 {
        out.push_str(&dstr);
        out.push_str(&"0".repeat(usize::try_from(n.saturating_sub(kk)).unwrap_or(0)));
    } else if 0 < n && n <= 21 {
        let nu = usize::try_from(n).unwrap_or(0);
        out.push_str(dstr.get(..nu).unwrap_or(""));
        out.push('.');
        out.push_str(dstr.get(nu..).unwrap_or(""));
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.push_str(&"0".repeat(usize::try_from(n.saturating_neg()).unwrap_or(0)));
        out.push_str(&dstr);
    } else {
        out.push_str(dstr.get(..1).unwrap_or(""));
        if ds.len() > 1 {
            out.push('.');
            out.push_str(dstr.get(1..).unwrap_or(""));
        }
        let ee = n.saturating_sub(1);
        out.push('e');
        out.push(if ee < 0 { '-' } else { '+' });
        out.push_str(&ee.unsigned_abs().to_string());
    }
    out
}
