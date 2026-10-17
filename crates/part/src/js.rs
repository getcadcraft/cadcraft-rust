//! JavaScript number and string semantics PartCraft's modules depend on, gathered in one place: `String(x)`,
//! `toFixed` / `toPrecision`, V8's `Math` functions, `Math.hypot` / `Math.round` / NaN-propagating `Math.min` /
//! `Math.max`, `String.prototype.trim` / `slice` (UTF-16 units), the file readers' control-character cleaning.
//! Everything here wraps cc-cad's and cc-m3d's helpers (the same functions the other ports use).

pub use cc_m3d::jsmath::JsMath;

/// `String(x)` for a number (ECMAScript Number::toString, V8's even digit on 17-digit ties).
pub fn num_str(x: f64) -> String {
    cc_cad::js::num_to_string(x)
}
/// The shortest round-trip digits of a positive finite x and its decimal exponent (x = d.ddd × 10^e).
fn shortest(ax: f64) -> (String, i64) {
    let e = format!("{ax:e}");
    let (m, ex) = e.split_once('e').unwrap_or((e.as_str(), "0"));
    (m.chars().filter(char::is_ascii_digit).collect(), ex.parse().unwrap_or(0))
}
/// Is the tail of a longer rounding exactly "5000…"? Then the value may be an exact tie, which Rust rounds half to
/// even and JS half up, and the exact (slow) path decides.
fn maybe_tie(tail: &str) -> bool {
    let mut it = tail.bytes();
    it.next() == Some(b'5') && it.all(|b| b == b'0')
}
/// `x.toFixed(f)`. Fast path: Rust's `{:.f}` rounds the exact binary value, as JS does, except on exact ties
/// (detected, then cc-cad's exact expansion answers).
pub fn to_fixed(x: f64, f: usize) -> String {
    if x.is_finite() && x.abs() < 1e21 && f <= 60 {
        let ax = x.abs();
        // an exact tie needs an expansion ending at digit f + 1, so the shortest digits are at most that long
        let short = shortest(ax);
        let frac = usize::try_from(i64::try_from(short.0.len()).unwrap_or(0).saturating_sub(short.1.saturating_add(1)).max(0)).unwrap_or(0);
        let no_tie = frac > f.saturating_add(1) || !format!("{:.*}", f.saturating_add(1), ax).ends_with('5') || {
            let long = format!("{:.*}", f.saturating_add(20), ax);
            let cut = long.find('.').map_or(long.len(), |d| d.saturating_add(1).saturating_add(f));
            !maybe_tie(long.get(cut..).unwrap_or(""))
        };
        if no_tie {
            let s = format!("{ax:.f$}");
            return if x < 0.0 { format!("-{s}") } else { s };
        }
    }
    cc_cad::js::to_fixed(x, f)
}
/// `x.toPrecision(p)`. Fast path as [`to_fixed`], from Rust's `{:.Ne}`.
pub fn to_precision(x: f64, p: usize) -> String {
    if x.is_finite() && x != 0.0 && (1..=60).contains(&p) {
        let ax = x.abs();
        let no_tie =
            shortest(ax).0.len() > p.saturating_add(1) || !format!("{:.*e}", p, ax).split('e').next().unwrap_or("").ends_with('5') || {
                let long = format!("{:.*e}", p.saturating_add(19), ax);
                !maybe_tie(long.split('e').next().and_then(|m| m.get(p.saturating_add(1)..)).unwrap_or(""))
            };
        if no_tie {
            let e_s = format!("{:.*e}", p.saturating_sub(1), ax);
            if let Some((mant, ex)) = e_s.split_once('e')
                && let Ok(e) = ex.parse::<i64>()
            {
                let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
                let pi = i64::try_from(p).unwrap_or(i64::MAX);
                let mut out = String::new();
                if x < 0.0 {
                    out.push('-');
                }
                if e < -6 || e >= pi {
                    out.push_str(digits.get(..1).unwrap_or(""));
                    if p > 1 {
                        out.push('.');
                        out.push_str(digits.get(1..).unwrap_or(""));
                    }
                    out.push('e');
                    out.push(if e < 0 { '-' } else { '+' });
                    out.push_str(&e.unsigned_abs().to_string());
                } else if e >= 0 {
                    let k = usize::try_from(e).unwrap_or(0).saturating_add(1);
                    out.push_str(digits.get(..k).unwrap_or(""));
                    if k < p {
                        out.push('.');
                        out.push_str(digits.get(k..).unwrap_or(""));
                    }
                } else {
                    out.push_str("0.");
                    for _ in 0..e.unsigned_abs().saturating_sub(1) {
                        out.push('0');
                    }
                    out.push_str(&digits);
                }
                return out;
            }
        }
    }
    cc_m3d::js::to_precision(x, p)
}
/// `Number(s)` / `+s`.
pub fn str_num(s: &str) -> f64 {
    cc_cad::js::str_to_number(s)
}
/// `+x.toFixed(f)` (`Number(x.toFixed(f))`).
pub fn fixed_num(x: f64, f: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    str_num(&to_fixed(x, f))
}
/// `String(+Number(v).toFixed(f))`: a number to at most f decimals, trailing zeros dropped.
pub fn fmt_fixed(x: f64, f: usize) -> String {
    num_str(fixed_num(x, f))
}
/// `+x.toPrecision(p)`.
pub fn prec_num(x: f64, p: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    str_num(&to_precision(x, p))
}

/// `Math.hypot(...xs)` (V8's compensated sum).
pub fn hypot(xs: &[f64]) -> f64 {
    cc_m3d::js::hypot(xs)
}
/// `Math.hypot(a, b)`.
pub fn hypot2(a: f64, b: f64) -> f64 {
    cc_m3d::js::hypot2(a, b)
}
/// `Math.hypot(a, b, c)`.
pub fn hypot3(a: f64, b: f64, c: f64) -> f64 {
    cc_m3d::js::hypot3(a, b, c)
}
/// `Math.round`.
pub fn round(x: f64) -> f64 {
    cc_m3d::js::round(x)
}
/// `Math.max(a, b)`.
pub fn max(a: f64, b: f64) -> f64 {
    cc_m3d::js::max(a, b)
}
/// `Math.min(a, b)`.
pub fn min(a: f64, b: f64) -> f64 {
    cc_m3d::js::min(a, b)
}
/// `Math.max(...xs)` (−∞ for none).
pub fn max_of(xs: impl IntoIterator<Item = f64>) -> f64 {
    xs.into_iter().fold(f64::NEG_INFINITY, max)
}
/// `Math.min(...xs)` (+∞ for none).
pub fn min_of(xs: impl IntoIterator<Item = f64>) -> f64 {
    xs.into_iter().fold(f64::INFINITY, min)
}
/// `Math.sign`.
pub fn sign(x: f64) -> f64 {
    cc_m3d::js::sign(x)
}
/// `Math.trunc`.
pub fn trunc(x: f64) -> f64 {
    x.trunc()
}
/// `Number.isInteger`.
pub fn is_integer(x: f64) -> bool {
    x.is_finite() && x.trunc() == x
}
/// `Math.sin` (fdlibm, as V8).
pub fn sin(x: f64) -> f64 {
    x.jsin()
}
/// `Math.cos`.
pub fn cos(x: f64) -> f64 {
    x.jcos()
}
/// `Math.tan`.
pub fn tan(x: f64) -> f64 {
    x.jtan()
}
/// `Math.atan`.
pub fn atan(x: f64) -> f64 {
    x.jatan()
}
/// `Math.atan2(y, x)`.
pub fn atan2(y: f64, x: f64) -> f64 {
    y.jatan2(x)
}
/// `Math.acos`.
pub fn acos(x: f64) -> f64 {
    x.jacos()
}
/// `Math.asin`.
pub fn asin(x: f64) -> f64 {
    libm::asin(x)
}
/// `Math.exp`.
pub fn exp(x: f64) -> f64 {
    crate::fdlibm::exp(x)
}
/// `Math.log`.
pub fn ln(x: f64) -> f64 {
    crate::fdlibm::log(x)
}
/// `Math.log10`.
pub fn log10(x: f64) -> f64 {
    crate::fdlibm::log10(x)
}
/// `a ** b` / `Math.pow(a, b)`.
pub fn pow(a: f64, b: f64) -> f64 {
    a.jpow(b)
}
/// `Math.sqrt`.
pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// JS `\s`.
pub fn is_ws(c: char) -> bool {
    cc_cad::js::is_ws(c)
}
/// `s.trim()`.
pub fn trim(s: &str) -> &str {
    cc_cad::js::trim(s)
}
/// `s.trimStart()`.
pub fn trim_start(s: &str) -> &str {
    cc_cad::js::trim_start(s)
}
/// `s.slice(0, n)` in UTF-16 code units.
pub fn slice16(s: &str, n: usize) -> String {
    cc_cad::js::slice16(s, n)
}
/// `s.length` (UTF-16 code units).
pub fn len16(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}
/// `s.toLowerCase()`.
pub fn lower(s: &str) -> String {
    s.to_lowercase()
}
/// `s.toUpperCase()`.
pub fn upper(s: &str) -> String {
    s.to_uppercase()
}
/// `s.replace(/[\u0000-\u001f\u007f]/g, '')`.
pub fn strip_ctrl(s: &str) -> String {
    s.chars().filter(|&c| !(c <= '\u{1f}' || c == '\u{7f}')).collect()
}
/// `s.replace(/[\u0000-\u001f\u007f-\u009f]/g, '')`.
pub fn strip_ctrl_c1(s: &str) -> String {
    s.chars().filter(|&c| !(c <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&c))).collect()
}
/// `String(v ?? '')` for a JSON value (absent / null → '').
pub fn str_of(v: Option<&serde_json::Value>) -> String {
    match v {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(x) => cc_cad::js::to_js_string(x),
    }
}
/// `String(v)` for a JSON value (absent → "undefined").
pub fn string_of(v: Option<&serde_json::Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(x) => cc_cad::js::to_js_string(x),
    }
}
/// The readers' `text(s, max)`: `String(s ?? '').replace(controls, '').slice(0, max)`.
pub fn text(v: Option<&serde_json::Value>, max: usize) -> String {
    slice16(&strip_ctrl(&str_of(v)), max)
}
/// `x.replace(/\.$/, '')`.
pub fn drop_final_dot(s: &str) -> String {
    s.strip_suffix('.').unwrap_or(s).to_string()
}
/// JS `a.localeCompare(b)` is not needed; a plain `<` on strings compares UTF-16 units.
pub fn cmp16(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// `a % n` for indices (0 when n is 0).
pub fn rem(a: usize, n: usize) -> usize {
    a.checked_rem(n).unwrap_or(0)
}
/// An f64 as i64 (truncated, saturating; NaN → 0).
#[allow(clippy::cast_possible_truncation)]
pub fn i64_of(x: f64) -> i64 {
    if x.is_nan() { 0 } else { x as i64 }
}
/// An i64 as usize (negative → 0).
pub fn usize_of(x: i64) -> usize {
    usize::try_from(x.max(0)).unwrap_or(usize::MAX)
}
/// usize → f64 (counts are JS numbers).
#[allow(clippy::cast_precision_loss)]
pub fn f(n: usize) -> f64 {
    n as f64
}
/// A JS number used as an array index: Some(i) for a non-negative integer that fits.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn idx(x: f64) -> Option<usize> {
    if x.is_finite() && x >= 0.0 && x.trunc() == x && x < 9.0e15 { Some(x as usize) } else { None }
}
/// `Math.ceil(x)` as a count, clamped to [0, cap] (NaN → 0).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn count(x: f64, cap: usize) -> usize {
    if x.is_nan() || x <= 0.0 {
        return 0;
    }
    let c = f(cap);
    if x >= c { cap } else { x as usize }
}

#[cfg(test)]
mod fast_fmt_tests {
    /// The fast toFixed / toPrecision paths against the exact expansions.
    #[test]
    fn fast_paths_match_exact() {
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut xs: Vec<f64> = vec![0.5, 1.5, 2.5, -2.5, 0.125, 1.005, 123.456, 1e-7, 9.995, 0.05, -0.0, 1e20, 5e-324, 0.000001, 999999.5];
        for _ in 0..20000 {
            let r = next();
            let m = (r >> 11) as f64 / (1u64 << 53) as f64;
            let e = ((r & 0x3f) as i32) - 30;
            let v = m * 10f64.powi(e);
            xs.push(if r & 0x40 != 0 { -v } else { v });
            xs.push(((r >> 20) % 100000) as f64 / 1000.0);
        }
        for x in xs {
            for f in [0usize, 1, 2, 3, 4, 6, 9, 12] {
                assert_eq!(super::to_fixed(x, f), cc_cad::js::to_fixed(x, f), "toFixed({x}, {f})");
            }
            for p in [1usize, 2, 4, 6, 9, 12, 15, 17] {
                assert_eq!(super::to_precision(x, p), cc_m3d::js::to_precision(x, p), "toPrecision({x}, {p})");
            }
        }
    }
}
