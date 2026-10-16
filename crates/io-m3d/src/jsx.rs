//! JavaScript semantics the formats lean on: V8's `Math` functions, and the questions the JS asks of untrusted
//! parsed JSON (glTF files, the cc-dwg drawing model): `typeof x === 'number'`, `Number.isFinite`, truthiness,
//! `Array.isArray`, `x ?? d`.

use serde_json::Value;

/// `Math.sin` (fdlibm, as V8).
#[inline]
pub fn sin(x: f64) -> f64 {
    cc_m3d::jsmath::fd_sin(x)
}
/// `Math.cos` (fdlibm, as V8).
#[inline]
pub fn cos(x: f64) -> f64 {
    cc_m3d::jsmath::fd_cos(x)
}
/// `Math.atan2` (msun, as V8).
#[inline]
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}
/// `Math.atan`.
#[inline]
pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}
/// `Math.tan`.
#[inline]
pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}
/// `Math.pow` / `**`: the platform `pow` (glibc's matched V8 on every sRGB value tested; msun's `libm::pow` missed
/// 129 of 1 256). Spec gap: on wasm `powf` is msun's, so a colour's last bit can differ there (as in cc-m3d's jsmath).
#[inline]
pub fn pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}
/// `Math.hypot(...)`.
#[inline]
pub fn hypot(xs: &[f64]) -> f64 {
    cc_m3d::js::hypot(xs)
}
/// `Math.round`.
#[inline]
pub fn round(x: f64) -> f64 {
    cc_m3d::js::round(x)
}
/// `Math.max(a, b)`.
#[inline]
pub fn max(a: f64, b: f64) -> f64 {
    cc_m3d::js::max(a, b)
}
/// `Math.min(a, b)`.
#[inline]
pub fn min(a: f64, b: f64) -> f64 {
    cc_m3d::js::min(a, b)
}

/// `typeof v === 'number'` → its value (JSON has no NaN: a number here is finite).
#[inline]
pub fn num(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    }
}
/// `Number.isFinite(v)` → its value.
#[inline]
pub fn finite(v: Option<&Value>) -> Option<f64> {
    num(v).filter(|x| x.is_finite())
}
/// `Number.isInteger(v)` → its value.
#[inline]
pub fn int(v: Option<&Value>) -> Option<f64> {
    num(v).filter(|x| cc_m3d::js::is_integer(*x))
}
/// `fin(x, d)`: a finite number, else d.
#[inline]
pub fn fin(v: Option<&Value>, d: f64) -> f64 {
    finite(v).unwrap_or(d)
}
/// JS truthiness of a JSON value (absent = undefined = false).
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}
/// `typeof v === 'string'`.
#[inline]
pub fn str(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str)
}
/// `Array.isArray(v) ? v : []`.
#[inline]
pub fn arr(v: Option<&Value>) -> &[Value] {
    v.and_then(Value::as_array).map_or(&[][..], Vec::as_slice)
}
/// `v && typeof v === 'object'` (arrays included, as in JS).
#[inline]
pub fn is_obj(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Object(_)) | Some(Value::Array(_)))
}
/// `v === undefined` (a JSON null is not undefined).
#[inline]
pub fn undef(v: Option<&Value>) -> bool {
    v.is_none()
}
/// `v.x` of an object (arrays have no named keys).
#[inline]
pub fn get<'a>(v: Option<&'a Value>, k: &str) -> Option<&'a Value> {
    match v {
        Some(Value::Object(o)) => o.get(k),
        _ => None,
    }
}
/// `a[i]` for an array value and a JS number used as an index (None for undefined).
pub fn at(a: &[Value], i: f64) -> Option<&Value> {
    crate::num::index(i).and_then(|k| a.get(k))
}
/// The array index a property key names (`arr[k]` in JS converts k to a string first: 0, "0", -0 and [0] all
/// name element 0; 1.5, "01" or an object name nothing).
pub fn key_index(k: Option<&Value>) -> Option<usize> {
    let s = match k? {
        Value::Number(n) => {
            let x = n.as_f64()?;
            if x == 0.0 {
                return Some(0);
            }
            return crate::num::index(x).filter(|&i| i < 4_294_967_295);
        }
        Value::String(s) => s.clone(),
        v @ Value::Array(_) => to_js_string(Some(v)),
        _ => return None,
    };
    let b = s.as_bytes();
    if b.is_empty() || !b.iter().all(u8::is_ascii_digit) || (b.len() > 1 && b.first() == Some(&b'0')) || b.len() > 10 {
        return None;
    }
    let v: u64 = s.parse().ok()?;
    if v >= 4_294_967_295 {
        return None;
    }
    usize::try_from(v).ok()
}
/// `a[k]` for an array and any key value.
pub fn at_key<'a>(a: &'a [Value], k: Option<&Value>) -> Option<&'a Value> {
    key_index(k).and_then(|i| a.get(i))
}

/// `Buffer.from(s, 'base64')` as Node decodes it: the standard and URL alphabets, every other character skipped, the
/// first '=' ends it, a last group of one character gives nothing.
// c lies in the matched range before each subtraction; n ≤ 4; acc < 2^24, and the casts keep its low bytes on purpose
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn base64_node(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3 + 3);
    let mut acc: u32 = 0;
    let mut n = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => continue,
        };
        acc = (acc << 6) | u32::from(v);
        n += 1;
        if n == 4 {
            out.extend_from_slice(&[(acc >> 16) as u8, (acc >> 8) as u8, acc as u8]);
            acc = 0;
            n = 0;
        }
    }
    if n == 2 {
        out.push((acc >> 4) as u8);
    } else if n == 3 {
        out.extend_from_slice(&[(acc >> 10) as u8, (acc >> 2) as u8]);
    }
    out
}

/// `String(v)` for a JSON value (what `String(x)` gives in JS).
pub fn to_js_string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => cc_m3d::js::num_str(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => {
            a.iter().map(|x| if x.is_null() { String::new() } else { to_js_string(Some(x)) }).collect::<Vec<_>>().join(",")
        }
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}
