//! geo-math.js — vectors and column-major 4 × 4 matrices (m[12..14] is the translation), with the JS's exact
//! operation order (so every result is bit-equal).

use crate::jsmath::{JsMath, hypot3};

pub type V3 = [f64; 3];
pub type V2 = [f64; 2];
pub type M4 = [f64; 16];

pub const I4: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

pub fn mul(a: &M4, b: &M4) -> M4 {
    let mut o = [0.0; 16];
    for (oc, bc) in o.as_chunks_mut::<4>().0.iter_mut().zip(b.as_chunks::<4>().0.iter()) {
        {
            let ([o0, o1, o2, o3], &[b0, b1, b2, b3]) = (oc, bc);
            *o0 = a[0] * b0 + a[4] * b1 + a[8] * b2 + a[12] * b3;
            *o1 = a[1] * b0 + a[5] * b1 + a[9] * b2 + a[13] * b3;
            *o2 = a[2] * b0 + a[6] * b1 + a[10] * b2 + a[14] * b3;
            *o3 = a[3] * b0 + a[7] * b1 + a[11] * b2 + a[15] * b3;
        }
    }
    o
}

#[inline]
pub fn apply(m: &M4, x: f64, y: f64, z: f64) -> V3 {
    [m[0] * x + m[4] * y + m[8] * z + m[12], m[1] * x + m[5] * y + m[9] * z + m[13], m[2] * x + m[6] * y + m[10] * z + m[14]]
}
#[inline]
pub fn apply_v(m: &M4, p: &V3) -> V3 {
    apply(m, p[0], p[1], p[2])
}

pub fn translation(x: f64, y: f64, z: f64) -> M4 {
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0]
}
pub fn scaling(x: f64, y: f64, z: f64) -> M4 {
    [x, 0.0, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, 0.0, z, 0.0, 0.0, 0.0, 0.0, 1.0]
}

/// A frame from origin o and unit axes x, y, z (columns).
pub fn frame(o: &V3, x: &V3, y: &V3, z: &V3) -> M4 {
    [x[0], x[1], x[2], 0.0, y[0], y[1], y[2], 0.0, z[0], z[1], z[2], 0.0, o[0], o[1], o[2], 1.0]
}

/// The inverse, or None when the determinant is 0 (NaN determinants divide through, as in the JS).
pub fn invert(a: &M4) -> Option<M4> {
    let mut o = [0.0; 16];
    o[0] = a[5] * a[10] * a[15] - a[5] * a[11] * a[14] - a[9] * a[6] * a[15] + a[9] * a[7] * a[14] + a[13] * a[6] * a[11]
        - a[13] * a[7] * a[10];
    o[4] = -a[4] * a[10] * a[15] + a[4] * a[11] * a[14] + a[8] * a[6] * a[15] - a[8] * a[7] * a[14] - a[12] * a[6] * a[11]
        + a[12] * a[7] * a[10];
    o[8] =
        a[4] * a[9] * a[15] - a[4] * a[11] * a[13] - a[8] * a[5] * a[15] + a[8] * a[7] * a[13] + a[12] * a[5] * a[11] - a[12] * a[7] * a[9];
    o[12] = -a[4] * a[9] * a[14] + a[4] * a[10] * a[13] + a[8] * a[5] * a[14] - a[8] * a[6] * a[13] - a[12] * a[5] * a[10]
        + a[12] * a[6] * a[9];
    o[1] = -a[1] * a[10] * a[15] + a[1] * a[11] * a[14] + a[9] * a[2] * a[15] - a[9] * a[3] * a[14] - a[13] * a[2] * a[11]
        + a[13] * a[3] * a[10];
    o[5] = a[0] * a[10] * a[15] - a[0] * a[11] * a[14] - a[8] * a[2] * a[15] + a[8] * a[3] * a[14] + a[12] * a[2] * a[11]
        - a[12] * a[3] * a[10];
    o[9] = -a[0] * a[9] * a[15] + a[0] * a[11] * a[13] + a[8] * a[1] * a[15] - a[8] * a[3] * a[13] - a[12] * a[1] * a[11]
        + a[12] * a[3] * a[9];
    o[13] =
        a[0] * a[9] * a[14] - a[0] * a[10] * a[13] - a[8] * a[1] * a[14] + a[8] * a[2] * a[13] + a[12] * a[1] * a[10] - a[12] * a[2] * a[9];
    o[2] =
        a[1] * a[6] * a[15] - a[1] * a[7] * a[14] - a[5] * a[2] * a[15] + a[5] * a[3] * a[14] + a[13] * a[2] * a[7] - a[13] * a[3] * a[6];
    o[6] =
        -a[0] * a[6] * a[15] + a[0] * a[7] * a[14] + a[4] * a[2] * a[15] - a[4] * a[3] * a[14] - a[12] * a[2] * a[7] + a[12] * a[3] * a[6];
    o[10] =
        a[0] * a[5] * a[15] - a[0] * a[7] * a[13] - a[4] * a[1] * a[15] + a[4] * a[3] * a[13] + a[12] * a[1] * a[7] - a[12] * a[3] * a[5];
    o[14] =
        -a[0] * a[5] * a[14] + a[0] * a[6] * a[13] + a[4] * a[1] * a[14] - a[4] * a[2] * a[13] - a[12] * a[1] * a[6] + a[12] * a[2] * a[5];
    o[3] = -a[1] * a[6] * a[11] + a[1] * a[7] * a[10] + a[5] * a[2] * a[11] - a[5] * a[3] * a[10] - a[9] * a[2] * a[7] + a[9] * a[3] * a[6];
    o[7] = a[0] * a[6] * a[11] - a[0] * a[7] * a[10] - a[4] * a[2] * a[11] + a[4] * a[3] * a[10] + a[8] * a[2] * a[7] - a[8] * a[3] * a[6];
    o[11] = -a[0] * a[5] * a[11] + a[0] * a[7] * a[9] + a[4] * a[1] * a[11] - a[4] * a[3] * a[9] - a[8] * a[1] * a[7] + a[8] * a[3] * a[5];
    o[15] = a[0] * a[5] * a[10] - a[0] * a[6] * a[9] - a[4] * a[1] * a[10] + a[4] * a[2] * a[9] + a[8] * a[1] * a[6] - a[8] * a[2] * a[5];
    let det = a[0] * o[0] + a[1] * o[4] + a[2] * o[8] + a[3] * o[12];
    // `if (!det)`: 0, −0 and NaN
    if det == 0.0 || det.is_nan() {
        return None;
    }
    for v in &mut o {
        *v /= det;
    }
    Some(o)
}

pub fn det3(m: &M4) -> f64 {
    m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2]) + m[8] * (m[1] * m[6] - m[5] * m[2])
}

#[inline]
pub fn sub(a: &V3, b: &V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
pub fn add(a: &V3, b: &V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
pub fn scale(a: &V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
pub fn dot(a: &V3, b: &V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
pub fn cross(a: &V3, b: &V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
#[inline]
pub fn len(a: &V3) -> f64 {
    hypot3(a[0], a[1], a[2])
}
pub fn norm(a: &V3) -> V3 {
    let l = len(a);
    if l > 0.0 { [a[0] / l, a[1] / l, a[2] / l] } else { [0.0, 0.0, 0.0] }
}
/// `!v[0] && !v[1] && !v[2]` (each 0, −0 or NaN)
#[inline]
pub fn is_zero(v: &V3) -> bool {
    falsy(v[0]) && falsy(v[1]) && falsy(v[2])
}
/// JS falsiness of a number.
#[inline]
pub fn falsy(x: f64) -> bool {
    x == 0.0 || x.is_nan()
}
/// `x || d` for a number.
#[inline]
pub fn or(x: f64, d: f64) -> f64 {
    if falsy(x) { d } else { x }
}

/// `a[d]` of a 2-vector (d = 0 or 1)
#[inline]
pub fn g2(a: &[f64; 2], d: usize) -> f64 {
    if d == 0 { a[0] } else { a[1] }
}
/// `&mut a[d]` of a 2-vector
#[inline]
pub fn s2(a: &mut [f64; 2], d: usize) -> &mut f64 {
    if d == 0 { &mut a[0] } else { &mut a[1] }
}
/// `a[k]` of a 3-vector (k = 0, 1 or 2)
#[inline]
pub fn g3(a: &V3, k: usize) -> f64 {
    match k {
        0 => a[0],
        1 => a[1],
        _ => a[2],
    }
}
/// `&mut a[k]` of a 3-vector
#[inline]
pub fn s3(a: &mut V3, k: usize) -> &mut f64 {
    match k {
        0 => &mut a[0],
        1 => &mut a[1],
        _ => &mut a[2],
    }
}

/// `(i + 1) % n` for i < n
#[inline]
pub fn next(i: usize, n: usize) -> usize {
    let j = i.saturating_add(1);
    if j >= n { 0 } else { j }
}

/// A unit vector perpendicular to n (any).
pub fn perp(n: &V3) -> V3 {
    let a = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    norm(&cross(n, &a))
}

/// Rotation about a unit axis through the origin by angle a (radians).
pub fn rotation(axis: &V3, a: f64) -> M4 {
    let [x, y, z] = norm(axis);
    let c = a.jcos();
    let s = a.jsin();
    let t = 1.0 - c;
    [
        t * x * x + c,
        t * x * y + s * z,
        t * x * z - s * y,
        0.0,
        t * x * y - s * z,
        t * y * y + c,
        t * y * z + s * x,
        0.0,
        t * x * z + s * y,
        t * y * z - s * x,
        t * z * z + c,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

/// Newell normal of a 3D polygon (not normalised; length = 2 × area).
pub fn newell(pts: &[V3]) -> V3 {
    let (mut nx, mut ny, mut nz) = (0.0, 0.0, 0.0);
    let n = pts.len();
    for (i, a) in pts.iter().enumerate() {
        let b = pts.get(next(i, n)).unwrap_or(a);
        nx += (a[1] - b[1]) * (a[2] + b[2]);
        ny += (a[2] - b[2]) * (a[0] + b[0]);
        nz += (a[0] - b[0]) * (a[1] + b[1]);
    }
    [nx, ny, nz]
}

/// Signed area of a 2D polygon (counter-clockwise positive).
pub fn area2(pts: &[V2]) -> f64 {
    let mut s = 0.0;
    let n = pts.len();
    for (i, a) in pts.iter().enumerate() {
        let b = pts.get(next(i, n)).unwrap_or(a);
        s += a[0] * b[1] - b[0] * a[1];
    }
    s / 2.0
}
