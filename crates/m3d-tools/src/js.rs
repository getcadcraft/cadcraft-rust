//! JavaScript number semantics used here (the kernel's `cc_m3d::js`, plus `Math.min` / `Math.max` of a list).

pub use cc_m3d::js::{hypot, hypot2, hypot3, is_integer, num_str, or, round, sign, to_fixed, trim, truthy};

/// `Math.max(a, b)` (NaN wins; +0 beats −0).
#[inline]
pub fn jmax(a: f64, b: f64) -> f64 {
    cc_m3d::js::max(a, b)
}
/// `Math.min(a, b)` (NaN wins; −0 beats +0).
#[inline]
pub fn jmin(a: f64, b: f64) -> f64 {
    cc_m3d::js::min(a, b)
}
/// `Math.max(...xs)` (−∞ for none).
pub fn max_of(xs: impl IntoIterator<Item = f64>) -> f64 {
    xs.into_iter().fold(f64::NEG_INFINITY, jmax)
}
/// `Math.min(...xs)` (+∞ for none).
pub fn min_of(xs: impl IntoIterator<Item = f64>) -> f64 {
    xs.into_iter().fold(f64::INFINITY, jmin)
}
/// `p[k]` (NaN past the end): an axis of a point.
#[inline]
pub fn g3(p: &[f64; 3], k: usize) -> f64 {
    p.get(k).copied().unwrap_or(f64::NAN)
}
/// A count or index from a JS number already checked to be a non-negative integer (NaN, ±∞ and negatives give 0;
/// past usize::MAX it saturates).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn usize_of(x: f64) -> usize {
    if x.is_finite() && x >= 0.0 { x as usize } else { 0 }
}
/// `String(Math.round(x * 10) / 10)` and friends: `+x.toFixed(d)` written back as JS writes a number.
pub fn fixed_num(x: f64, d: usize) -> f64 {
    cc_m3d::file::str_to_number(&to_fixed(x, d))
}
