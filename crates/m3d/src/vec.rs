//! vec.js — small 3D vector and 4×4 matrix helpers. Points and vectors are `[x, y, z]`; matrices are 16 numbers in
//! column-major order (as three.js and glTF), so `m[12..15]` is the translation. Model space is Z up, lengths in
//! metres. Every function keeps the JS's order of floating-point operations.

use crate::js;
use crate::jsmath::JsMath;

pub type P3 = [f64; 3];
pub type M4 = [f64; 16];

/// Length tolerance (m): 1 µm.
pub const EPS: f64 = 1e-6;
pub const EPS2: f64 = EPS * EPS;

/// `p[k]` (NaN past the end).
#[inline]
pub fn at3(p: &P3, k: usize) -> f64 {
    p.get(k).copied().unwrap_or(f64::NAN)
}
#[inline]
pub fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
pub fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
pub fn mul(a: P3, s: f64) -> P3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
pub fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
pub fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
#[inline]
pub fn len(a: P3) -> f64 {
    js::hypot3(a[0], a[1], a[2])
}
#[inline]
pub fn dist(a: P3, b: P3) -> f64 {
    js::hypot3(a[0] - b[0], a[1] - b[1], a[2] - b[2])
}
#[inline]
pub fn lerp(a: P3, b: P3, t: f64) -> P3 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}
#[inline]
pub fn neg(a: P3) -> P3 {
    [-a[0], -a[1], -a[2]]
}
#[inline]
pub fn near(a: P3, b: P3, tol: f64) -> bool {
    // Math.hypot is never below its largest argument by more than a few ulps: a point further than tol along one axis
    // is not near (the exact test decides the rest)
    let lim = tol * 1.000_000_001;
    if (a[0] - b[0]).abs() > lim || (a[1] - b[1]).abs() > lim || (a[2] - b[2]).abs() > lim {
        return false;
    }
    dist(a, b) <= tol
}
/// Is p within `tol` of the box of a and b (with room for rounding)? False means no point of the segment a–b, as
/// computed, can be within `tol` of p: the exact tests that follow would fail.
#[inline]
pub fn near_box(p: P3, a: P3, b: P3, tol: f64) -> bool {
    for k in 0..3 {
        let (pk, ak, bk) = (at3(&p, k), at3(&a, k), at3(&b, k));
        let (lo, hi) = if ak < bk { (ak, bk) } else { (bk, ak) };
        let slack = tol * 1.000_001 + 1e-14 * (lo.abs() + hi.abs());
        if pk < lo - slack || pk > hi + slack {
            return false;
        }
    }
    true
}
/// Could segments a–b and c–d come within `tol` of each other (boxes, with room for rounding)?
#[inline]
pub fn boxes_near(a: P3, b: P3, c: P3, d: P3, tol: f64) -> bool {
    for k in 0..3 {
        let (ak, bk, ck, dk) = (at3(&a, k), at3(&b, k), at3(&c, k), at3(&d, k));
        let (lo1, hi1) = if ak < bk { (ak, bk) } else { (bk, ak) };
        let (lo2, hi2) = if ck < dk { (ck, dk) } else { (dk, ck) };
        let slack = tol * 1.000_001 + 1e-14 * (lo1.abs() + hi1.abs() + lo2.abs() + hi2.abs());
        if lo2 > hi1 + slack || lo1 > hi2 + slack {
            return false;
        }
    }
    true
}
pub fn norm(a: P3) -> P3 {
    let l = len(a);
    if l > 1e-15 { [a[0] / l, a[1] / l, a[2] / l] } else { [0.0, 0.0, 0.0] }
}
/// Any unit vector perpendicular to n.
pub fn perp(n: P3) -> P3 {
    let a = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    norm(cross(n, a))
}
pub fn parallel(a: P3, b: P3, tol: f64) -> bool {
    len(cross(norm(a), norm(b))) <= tol
}

fn newell_raw(pts: &[P3]) -> P3 {
    let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
    let n = pts.len();
    for (i, p) in pts.iter().enumerate() {
        let q = pts.get((i + 1) % n).copied().unwrap_or(*p);
        x += (p[1] - q[1]) * (p[2] + q[2]);
        y += (p[2] - q[2]) * (p[0] + q[0]);
        z += (p[0] - q[0]) * (p[1] + q[1]);
    }
    [x, y, z]
}
/// Newell normal of a polygon: unit, CCW = right-hand rule; `[0, 0, 0]` if degenerate.
pub fn newell(pts: &[P3]) -> P3 {
    norm(newell_raw(pts))
}
/// The polygon's area (half the Newell vector's length).
pub fn poly_area3(pts: &[P3]) -> f64 {
    let [x, y, z] = newell_raw(pts);
    js::hypot3(x, y, z) / 2.0
}

/// A plane {n, d}: n·p = d.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub n: P3,
    pub d: f64,
}
/// The plane through p with normal n made canonical (first non-zero component positive).
pub fn plane_from(n: P3, p: P3) -> Plane {
    let mut m = norm(n);
    let k = if m[0].abs() > 1e-9 {
        0
    } else if m[1].abs() > 1e-9 {
        1
    } else {
        2
    };
    if m.get(k).copied().unwrap_or(0.0) < 0.0 {
        m = neg(m);
    }
    Plane { n: m, d: dot(m, p) }
}
pub fn on_plane(pl: &Plane, p: P3, tol: f64) -> bool {
    (dot(pl.n, p) - pl.d).abs() <= tol
}
pub fn same_plane(a: &Plane, b: &Plane) -> bool {
    dot(a.n, b.n).abs() > 1.0 - 1e-9 && (a.d * js::sign(dot(a.n, b.n)) - b.d).abs() <= EPS
}

/// A 2D frame on a plane normal n: u × v = n.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub u: P3,
    pub v: P3,
    pub n: P3,
}
pub fn frame(n: P3) -> Frame {
    let u = if n[2].abs() > 0.999 { [1.0, 0.0, 0.0] } else { norm(cross([0.0, 0.0, 1.0], n)) };
    Frame { u, v: cross(n, u), n }
}
#[inline]
pub fn to2(f: &Frame, p: P3) -> [f64; 2] {
    [dot(p, f.u), dot(p, f.v)]
}

/// Closest points between the line o + t·d and the segment a–b: (t, s, on the ray, on the segment).
pub fn ray_to_segment(o: P3, d: P3, a: P3, b: P3) -> (f64, f64, P3, P3) {
    let u = sub(b, a);
    let w = sub(o, a);
    let (aa, bb, cc, dd, ee) = (dot(d, d), dot(d, u), dot(u, u), dot(d, w), dot(u, w));
    let den = aa * cc - bb * bb;
    let (mut t, mut s);
    if den < 1e-14 {
        s = 0.0;
        t = -dd / aa;
    } else {
        t = (bb * ee - cc * dd) / den;
        s = (aa * ee - bb * dd) / den;
    }
    if s < 0.0 {
        s = 0.0;
        t = -dd / aa;
    } else if s > 1.0 {
        s = 1.0;
        t = (bb - dd) / aa;
    }
    (t, s, add(o, mul(d, t)), add(a, mul(u, s)))
}
/// Closest point to p on the infinite line a + s·u: (s, point).
pub fn closest_on_line(p: P3, a: P3, u: P3) -> (f64, P3) {
    let s = dot(sub(p, a), u) / dot(u, u);
    (s, add(a, mul(u, s)))
}
pub fn ray_plane(o: P3, d: P3, pl: &Plane) -> Option<(f64, P3)> {
    let den = dot(pl.n, d);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = (pl.d - dot(pl.n, o)) / den;
    Some((t, add(o, mul(d, t))))
}
/// Möller–Trumbore; t or None (both sides).
pub fn ray_tri(o: P3, d: P3, a: P3, b: P3, c: P3) -> Option<f64> {
    let e1 = sub(b, a);
    let e2 = sub(c, a);
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(o, a);
    let u = dot(s, p) * inv;
    if u < -1e-9 || u > 1.0 + 1e-9 {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < -1e-9 || u + v > 1.0 + 1e-9 {
        return None;
    }
    let t = dot(e2, q) * inv;
    if t > 0.0 { Some(t) } else { None }
}

// ---- matrices (column-major) ----
pub const IDENT: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
#[inline]
pub fn ident() -> M4 {
    IDENT
}
#[inline]
fn g(m: &M4, i: usize) -> f64 {
    m.get(i).copied().unwrap_or(0.0)
}
pub fn mmul(a: &M4, b: &M4) -> M4 {
    let mut r = [0.0; 16];
    for c in 0..4 {
        for rr in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += g(a, k * 4 + rr) * g(b, c * 4 + k);
            }
            if let Some(x) = r.get_mut(c * 4 + rr) {
                *x = s;
            }
        }
    }
    r
}
pub fn translation(t: P3) -> M4 {
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, t[0], t[1], t[2], 1.0]
}
pub fn scaling(s: f64, about: P3) -> M4 {
    let sm = [s, 0.0, 0.0, 0.0, 0.0, s, 0.0, 0.0, 0.0, 0.0, s, 0.0, 0.0, 0.0, 0.0, 1.0];
    mmul(&translation(about), &mmul(&sm, &translation(neg(about))))
}
/// Rotation by angle (radians, right-hand) about the axis through `about`.
pub fn rotation(axis: P3, angle: f64, about: P3) -> M4 {
    let [x, y, z] = norm(axis);
    let c = angle.jcos();
    let s = angle.jsin();
    let t = 1.0 - c;
    let r = [
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
    ];
    mmul(&translation(about), &mmul(&r, &translation(neg(about))))
}
pub fn apply(m: &M4, p: P3) -> P3 {
    let (x, y, z) = (p[0], p[1], p[2]);
    let w = js::or(g(m, 3) * x + g(m, 7) * y + g(m, 11) * z + g(m, 15), 1.0);
    [
        (g(m, 0) * x + g(m, 4) * y + g(m, 8) * z + g(m, 12)) / w,
        (g(m, 1) * x + g(m, 5) * y + g(m, 9) * z + g(m, 13)) / w,
        (g(m, 2) * x + g(m, 6) * y + g(m, 10) * z + g(m, 14)) / w,
    ]
}
/// Direction (no translation).
pub fn apply_dir(m: &M4, v: P3) -> P3 {
    [
        g(m, 0) * v[0] + g(m, 4) * v[1] + g(m, 8) * v[2],
        g(m, 1) * v[0] + g(m, 5) * v[1] + g(m, 9) * v[2],
        g(m, 2) * v[0] + g(m, 6) * v[1] + g(m, 10) * v[2],
    ]
}
/// Normal transform for an affine m (inverse transpose of its 3×3), normalised.
pub fn apply_normal(m: &M4, n: P3) -> P3 {
    let inv = invert(m);
    norm([
        g(&inv, 0) * n[0] + g(&inv, 1) * n[1] + g(&inv, 2) * n[2],
        g(&inv, 4) * n[0] + g(&inv, 5) * n[1] + g(&inv, 6) * n[2],
        g(&inv, 8) * n[0] + g(&inv, 9) * n[1] + g(&inv, 10) * n[2],
    ])
}
pub fn det3(m: &M4) -> f64 {
    g(m, 0) * (g(m, 5) * g(m, 10) - g(m, 9) * g(m, 6)) - g(m, 4) * (g(m, 1) * g(m, 10) - g(m, 9) * g(m, 2))
        + g(m, 8) * (g(m, 1) * g(m, 6) - g(m, 5) * g(m, 2))
}
pub fn invert(m: &M4) -> M4 {
    let [a00, a01, a02, a03, a10, a11, a12, a13, a20, a21, a22, a23, a30, a31, a32, a33] = *m;
    let b00 = a00 * a11 - a01 * a10;
    let b01 = a00 * a12 - a02 * a10;
    let b02 = a00 * a13 - a03 * a10;
    let b03 = a01 * a12 - a02 * a11;
    let b04 = a01 * a13 - a03 * a11;
    let b05 = a02 * a13 - a03 * a12;
    let b06 = a20 * a31 - a21 * a30;
    let b07 = a20 * a32 - a22 * a30;
    let b08 = a20 * a33 - a23 * a30;
    let b09 = a21 * a32 - a22 * a31;
    let b10 = a21 * a33 - a23 * a31;
    let b11 = a22 * a33 - a23 * a32;
    let mut det = b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;
    if !js::truthy(det) {
        return ident();
    }
    det = 1.0 / det;
    [
        (a11 * b11 - a12 * b10 + a13 * b09) * det,
        (a02 * b10 - a01 * b11 - a03 * b09) * det,
        (a31 * b05 - a32 * b04 + a33 * b03) * det,
        (a22 * b04 - a21 * b05 - a23 * b03) * det,
        (a12 * b08 - a10 * b11 - a13 * b07) * det,
        (a00 * b11 - a02 * b08 + a03 * b07) * det,
        (a32 * b02 - a30 * b05 - a33 * b01) * det,
        (a20 * b05 - a22 * b02 + a23 * b01) * det,
        (a10 * b10 - a11 * b08 + a13 * b06) * det,
        (a01 * b08 - a00 * b10 - a03 * b06) * det,
        (a30 * b04 - a31 * b02 + a33 * b00) * det,
        (a21 * b02 - a20 * b04 - a23 * b00) * det,
        (a11 * b07 - a10 * b09 - a12 * b06) * det,
        (a00 * b09 - a01 * b07 + a02 * b06) * det,
        (a31 * b01 - a30 * b03 - a32 * b00) * det,
        (a20 * b03 - a21 * b01 + a22 * b00) * det,
    ]
}
pub fn is_ident(m: &M4) -> bool {
    m.iter().zip(IDENT.iter()).all(|(v, i)| (v - i).abs() < 1e-12)
}

/// A bounding box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBox {
    pub min: P3,
    pub max: P3,
}
/// `bboxAdd(bb, p)`.
pub fn bbox_add(bb: Option<BBox>, p: P3) -> BBox {
    match bb {
        None => BBox { min: p, max: p },
        Some(mut b) => {
            for k in 0..3 {
                let v = p.get(k).copied().unwrap_or(0.0);
                if let Some(lo) = b.min.get_mut(k)
                    && v < *lo
                {
                    *lo = v;
                }
                if let Some(hi) = b.max.get_mut(k)
                    && v > *hi
                {
                    *hi = v;
                }
            }
            b
        }
    }
}
