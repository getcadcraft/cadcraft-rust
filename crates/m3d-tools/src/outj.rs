//! Output JSON in the parity kit's canonical form: numbers as numbers (−0 as 0), NaN / ±∞ as "$NaN" / "$Inf" /
//! "$-Inf" (canon.mjs), so the JS reference and this port print comparable records.

use serde_json::Value;

/// A number.
pub fn num(x: f64) -> Value {
    if x.is_nan() {
        return Value::String("$NaN".into());
    }
    if x == f64::INFINITY {
        return Value::String("$Inf".into());
    }
    if x == f64::NEG_INFINITY {
        return Value::String("$-Inf".into());
    }
    let x = if x == 0.0 { 0.0 } else { x };
    serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
}
/// A point.
pub fn p3(p: &[f64; 3]) -> Value {
    Value::Array(p.iter().map(|x| num(*x)).collect())
}
/// A list of numbers.
pub fn nums(xs: &[f64]) -> Value {
    Value::Array(xs.iter().map(|x| num(*x)).collect())
}
/// A list of points.
pub fn pts(ps: &[[f64; 3]]) -> Value {
    Value::Array(ps.iter().map(p3).collect())
}
/// An optional point (null when None).
pub fn opt_p3(p: Option<&[f64; 3]>) -> Value {
    p.map_or(Value::Null, p3)
}
/// An optional number (null when None).
pub fn opt_num(x: Option<f64>) -> Value {
    x.map_or(Value::Null, num)
}
