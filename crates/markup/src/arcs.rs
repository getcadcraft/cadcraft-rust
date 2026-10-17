//! arcs.js — the arc markup and the angle measurement: arcs through three points, Bézier pieces, angles.

use crate::js::{JsMath, hypot, round, to_fixed};
use std::f64::consts::PI;

pub type P = [f64; 2];

/// A circle arc: centre, radius, start angle and signed sweep (radians; positive = counterclockwise, y up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    pub c: P,
    pub r: f64,
    pub a0: f64,
    pub sweep: f64,
}

/// `arcThrough(a, m, b)` (None when the points are in a line).
pub fn arc_through(a: P, m: P, b: P) -> Option<Arc> {
    let d = 2.0 * (a[0] * (m[1] - b[1]) + m[0] * (b[1] - a[1]) + b[0] * (a[1] - m[1]));
    let span = crate::js::max(crate::js::max(hypot(b[0] - a[0], b[1] - a[1]), hypot(m[0] - a[0], m[1] - a[1])), 1e-9);
    if d.abs() < 1e-9 * span * span {
        return None;
    }
    let s = |p: P| p[0] * p[0] + p[1] * p[1];
    let cx = (s(a) * (m[1] - b[1]) + s(m) * (b[1] - a[1]) + s(b) * (a[1] - m[1])) / d;
    let cy = (s(a) * (b[0] - m[0]) + s(m) * (a[0] - b[0]) + s(b) * (m[0] - a[0])) / d;
    let r = hypot(a[0] - cx, a[1] - cy);
    if !(r < 1e7) {
        return None;
    }
    let ang = |p: P| (p[1] - cy).jatan2(p[0] - cx);
    let (a0, am, a1) = (ang(a), ang(m), ang(b));
    let ccw = |x: f64| ((x % (2.0 * PI)) + 2.0 * PI) % (2.0 * PI);
    let to_m = ccw(am - a0);
    let to_b = ccw(a1 - a0);
    let sweep = if to_m <= to_b { to_b } else { -(2.0 * PI - to_b) };
    Some(Arc { c: [cx, cy], r, a0, sweep })
}

/// `arcBeziers(arc)` → [[p0, c1, c2, p3], …] (≤ 90° each).
pub fn arc_beziers(arc: &Arc) -> Vec<[P; 4]> {
    let Arc { c, r, a0, sweep } = *arc;
    let nf = crate::js::max(1.0, (sweep.abs() / (PI / 2.0) - 1e-9).ceil());
    let step = sweep / nf;
    let k = (4.0 / 3.0) * (step / 4.0).jtan();
    // the count of pieces: at most 4 for a sweep within one turn, but a NaN or huge sweep must not allocate wildly
    let n = if nf.is_finite() && nf <= 4096.0 { crate::js::f2usize(nf) } else { 0 };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        #[allow(clippy::cast_precision_loss)]
        let fi = i as f64;
        let t0 = a0 + fi * step;
        let t1 = t0 + step;
        let p0 = [c[0] + r * t0.jcos(), c[1] + r * t0.jsin()];
        let p3 = [c[0] + r * t1.jcos(), c[1] + r * t1.jsin()];
        out.push([p0, [p0[0] - k * r * t0.jsin(), p0[1] + k * r * t0.jcos()], [p3[0] + k * r * t1.jsin(), p3[1] - k * r * t1.jcos()], p3]);
    }
    out
}

/// `arcLength(arc)`.
pub fn arc_length(arc: Option<&Arc>) -> f64 {
    arc.map_or(0.0, |a| a.r * a.sweep.abs())
}

/// `angleAt(a, v, b)` in degrees, 0–180.
pub fn angle_at(a: P, v: P, b: P) -> f64 {
    let u = [a[0] - v[0], a[1] - v[1]];
    let w = [b[0] - v[0], b[1] - v[1]];
    let lu = hypot(u[0], u[1]);
    let lw = hypot(w[0], w[1]);
    if lu == 0.0 || lw == 0.0 || lu.is_nan() || lw.is_nan() {
        // `!lu || !lw` — NaN is falsy too
        return 0.0;
    }
    let cos = crate::js::max(-1.0, crate::js::min(1.0, (u[0] * w[0] + u[1] * w[1]) / (lu * lw)));
    (cos.jacos() * 180.0) / PI
}

/// `angleArc(a, v, b, r)`.
pub fn angle_arc(a: P, v: P, b: P, r: f64) -> Arc {
    let t0 = (a[1] - v[1]).jatan2(a[0] - v[0]);
    let t1 = (b[1] - v[1]).jatan2(b[0] - v[0]);
    let mut sweep = t1 - t0;
    while sweep > PI {
        sweep -= 2.0 * PI;
    }
    while sweep < -PI {
        sweep += 2.0 * PI;
    }
    Arc { c: v, r, a0: t0, sweep }
}

/// `formatAngle(deg, places)` → "37.5°".
pub fn format_angle(deg: f64, places: usize) -> String {
    let p = 10f64.jpow(places as f64);
    format!("{}°", to_fixed(round(deg * p) / p, places))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }
    #[test]
    fn arcs() {
        let q = arc_through([100.0, 0.0], [0.0, 100.0], [-100.0, 0.0]).unwrap();
        assert!(near(q.c[0], 0.0) && near(q.c[1], 0.0) && near(q.r, 100.0) && near(q.sweep, PI));
        assert!(near(arc_through([-100.0, 0.0], [0.0, 100.0], [100.0, 0.0]).unwrap().sweep, -PI));
        assert!(near(arc_through([100.0, 0.0], [0.0, -100.0], [0.0, 100.0]).unwrap().sweep.abs(), 1.5 * PI));
        assert!(arc_through([0.0, 0.0], [5.0, 5.0], [10.0, 10.0]).is_none());
        let bz = arc_beziers(&q);
        assert_eq!(bz.len(), 2);
        assert!(near(arc_length(Some(&q)), PI * 100.0));
        assert!(near(angle_at([10.0, 0.0], [0.0, 0.0], [0.0, 10.0]), 90.0));
        assert_eq!(format_angle(37.46, 1), "37.5°");
    }
}
