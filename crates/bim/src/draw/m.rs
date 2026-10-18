//! V8's `Math.*` for the building tools (cc-io-ifc `jsmath`: fdlibm sin / cos, msun tan / atan / atan2, V8's hypot) and
//! the small 2D helpers the modules share.

use cc_io_ifc::jsmath::{self, JsMath};

pub fn sin(x: f64) -> f64 {
    x.jsin()
}
pub fn cos(x: f64) -> f64 {
    x.jcos()
}
pub fn tan(x: f64) -> f64 {
    x.jtan()
}
pub fn atan(x: f64) -> f64 {
    x.jatan()
}
pub fn atan2(y: f64, x: f64) -> f64 {
    y.jatan2(x)
}
/// `Math.hypot(a, b)`
pub fn hypot(a: f64, b: f64) -> f64 {
    jsmath::hypot2(a, b)
}
/// `Math.hypot(a, b, c)`
pub fn hypot3(a: f64, b: f64, c: f64) -> f64 {
    jsmath::hypot3(a, b, c)
}
pub fn round(x: f64) -> f64 {
    jsmath::round(x)
}
pub fn sign(x: f64) -> f64 {
    jsmath::sign(x)
}
/// `Math.min(a, b)` / `Math.max(a, b)`
pub fn min(a: f64, b: f64) -> f64 {
    jsmath::min(a, b)
}
pub fn max(a: f64, b: f64) -> f64 {
    jsmath::max(a, b)
}
/// `Math.max(...xs)` (−∞ for none)
pub fn max_of(xs: impl IntoIterator<Item = f64>) -> f64 {
    xs.into_iter().fold(f64::NEG_INFINITY, max)
}
/// `Math.min(...xs)` (∞ for none)
pub fn min_of(xs: impl IntoIterator<Item = f64>) -> f64 {
    xs.into_iter().fold(f64::INFINITY, min)
}

pub type V2 = [f64; 2];

pub fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}
pub fn add(a: V2, b: V2) -> V2 {
    [a[0] + b[0], a[1] + b[1]]
}
pub fn mul(a: V2, k: f64) -> V2 {
    [a[0] * k, a[1] * k]
}
pub fn dot(a: V2, b: V2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
pub fn cross(a: V2, b: V2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
/// `Math.hypot(a[0] - b[0], a[1] - b[1])`
pub fn dist(a: V2, b: V2) -> f64 {
    hypot(a[0] - b[0], a[1] - b[1])
}
/// `Math.hypot(...v)` of a 2-vector
pub fn len(v: V2) -> f64 {
    hypot(v[0], v[1])
}
pub fn left(d: V2) -> V2 {
    [-d[1], d[0]]
}
/// (i + 1) % n
pub fn nx(i: usize, n: usize) -> usize {
    i.wrapping_add(1).checked_rem(n).unwrap_or(0)
}
/// (i + n − 1) % n
pub fn pv(i: usize, n: usize) -> usize {
    i.wrapping_add(n).wrapping_sub(1).checked_rem(n).unwrap_or(0)
}
/// `p[i]` of a polygon (a point outside one reads as the origin; callers keep i in range).
pub fn at(p: &[V2], i: usize) -> V2 {
    p.get(i).copied().unwrap_or([0.0, 0.0])
}
/// Even-odd point in polygon (model-rooms.js `inPoly`).
pub fn in_poly(x: f64, y: f64, p: &[V2]) -> bool {
    let mut inside = false;
    let n = p.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (at(p, i), at(p, j));
        let (xi, yi, xj, yj) = (a[0], a[1], b[0], b[1]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}
/// Signed area (anticlockwise > 0).
pub fn area(p: &[V2]) -> f64 {
    let n = p.len();
    let mut s = 0.0;
    for i in 0..n {
        let (a, b) = (at(p, i), at(p, nx(i, n)));
        s += a[0] * b[1] - a[1] * b[0];
    }
    s / 2.0
}
/// Perimeter of a closed loop.
pub fn perim(p: &[V2]) -> f64 {
    let n = p.len();
    let mut s = 0.0;
    for i in 0..n {
        let (a, b) = (at(p, i), at(p, nx(i, n)));
        s += hypot(b[0] - a[0], b[1] - a[1]);
    }
    s
}
