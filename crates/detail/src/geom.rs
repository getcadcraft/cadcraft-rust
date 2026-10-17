//! geom.js — 3D vectors and member frames. Millimetres, z up. A member runs from `start` to `end` along its local x;
//! its section lies in the (u, v) plane: non-vertical members v = global Z made square to x, vertical ones v = +X;
//! `rotation` turns (u, v) about +x. `position` places the section on the line (voff = the v shift of its centre).

use crate::js::{cos, hypot3, round, sin};
use std::f64::consts::PI;

pub type V3 = [f64; 3];

#[inline]
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
pub fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
#[inline]
pub fn len(a: V3) -> f64 {
    hypot3(a[0], a[1], a[2])
}
pub fn norm(a: V3) -> V3 {
    let l = len(a);
    if l > 0.0 { mul(a, 1.0 / l) } else { [0.0, 0.0, 0.0] }
}
pub fn dist(a: V3, b: V3) -> f64 {
    len(sub(a, b))
}
pub fn lerp(a: V3, b: V3, t: f64) -> V3 {
    add(a, mul(sub(b, a), t))
}
/// `near(a, b, tol = 0.5)`.
pub fn near(a: V3, b: V3, tol: f64) -> bool {
    dist(a, b) <= tol
}

/// A member frame: origin, axes, length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub o: V3,
    pub x: V3,
    pub u: V3,
    pub v: V3,
    pub len: f64,
}

/// `frame(s, e, rotation)`.
pub fn frame(s: V3, e: V3, rotation: f64) -> Frame {
    let d = sub(e, s);
    let l = len(d);
    let x = if l > 0.0 { mul(d, 1.0 / l) } else { [1.0, 0.0, 0.0] };
    let mut v = if x[2].abs() > 0.999_999 { [1.0, 0.0, 0.0] } else { norm(sub([0.0, 0.0, 1.0], mul(x, x[2]))) };
    let mut u = cross(v, x);
    if rotation != 0.0 && !rotation.is_nan() {
        let a = rotation * PI / 180.0;
        let (c, sn) = (cos(a), sin(a));
        let u2 = add(mul(u, c), mul(v, sn));
        let v2 = add(mul(u, -sn), mul(v, c));
        u = u2;
        v = v2;
    }
    Frame { o: s, x, u, v, len: l }
}

/// `voffFor(position, depth)`.
pub fn voff_for(position: &str, depth: f64) -> f64 {
    match position {
        "top" => -depth / 2.0,
        "bottom" => depth / 2.0,
        _ => 0.0,
    }
}

/// `toWorld(f, x, u, v, voff)`.
pub fn to_world(f: &Frame, x: f64, u: f64, v: f64, voff: f64) -> V3 {
    add(add(add(f.o, mul(f.x, x)), mul(f.u, u)), mul(f.v, v + voff))
}

/// A point in a frame's local coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Local {
    pub x: f64,
    pub u: f64,
    pub v: f64,
}
/// `toLocal(f, p, voff)`.
pub fn to_local(f: &Frame, p: V3, voff: f64) -> Local {
    let d = sub(p, f.o);
    Local { x: dot(d, f.x), u: dot(d, f.u), v: dot(d, f.v) - voff }
}

/// `q(v, tol)`.
pub fn q(v: f64, tol: f64) -> f64 {
    round(v / tol) * tol + 0.0
}

/// `sectionBoxAt(f, x, w, h, voff)`.
pub fn section_box_at(f: &Frame, x: f64, w: f64, h: f64, voff: f64) -> [V3; 4] {
    [[-w / 2.0, -h / 2.0], [w / 2.0, -h / 2.0], [w / 2.0, h / 2.0], [-w / 2.0, h / 2.0]].map(|[u, v]| to_world(f, x, u, v, voff))
}

/// `clipLoopV(loop, limit, keepBelow)`.
pub fn clip_loop_v(lp: &[[f64; 2]], limit: f64, keep_below: bool) -> Vec<[f64; 2]> {
    let inside = |p: [f64; 2]| if keep_below { p[1] <= limit + 1e-9 } else { p[1] >= limit - 1e-9 };
    let n = lp.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (Some(&a), Some(&b)) = (lp.get(i), lp.get((i + 1) % n)) else { continue };
        let (ia, ib) = (inside(a), inside(b));
        if ia {
            out.push(a);
        }
        if ia != ib {
            let t = (limit - a[1]) / (b[1] - a[1]);
            out.push([a[0] + (b[0] - a[0]) * t, limit]);
        }
    }
    if out.len() >= 3 { out } else { Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frames() {
        let f = frame([0.0, 0.0, 0.0], [6000.0, 0.0, 0.0], 0.0);
        assert_eq!(f.v, [0.0, 0.0, 1.0]);
        let c = frame([0.0, 0.0, 0.0], [0.0, 0.0, 4000.0], 0.0);
        assert_eq!(c.v, [1.0, 0.0, 0.0]);
    }
}
