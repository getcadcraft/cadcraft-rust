//! `geom.js` — plane geometry for takeoff marks, in PDF user space. A point may carry a bulge `b` (DXF style: the
//! segment that STARTS at it is an arc, `b = tan(θ/4)`, positive = counter-clockwise); `b == 0` is a straight segment.
//! Lengths and areas of arcs are exact; arcs are flattened only for picking and the crossing/inside checks.
// Integer arithmetic here is point indices within the point lists (`i + 1`, `% n` inside `0..n`); the readers cap list sizes: it cannot overflow, so the crate-wide
// `arithmetic_side_effects` deny is relaxed for this module only.
#![allow(clippy::arithmetic_side_effects)]

use crate::i18n::t;
use crate::jsmath::{atan, atan2, cos, sin, tan};
use cc_m3d::js::{hypot2, max, min};
use std::f64::consts::PI;

/// A point `{x, y, b?}`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct P {
    pub x: f64,
    pub y: f64,
    /// the bulge of the segment starting here (0 = straight)
    pub b: f64,
}
impl P {
    #[inline]
    pub const fn new(x: f64, y: f64) -> Self {
        P { x, y, b: 0.0 }
    }
    #[inline]
    pub const fn arc(x: f64, y: f64, b: f64) -> Self {
        P { x, y, b }
    }
    /// JS truthiness of `p.b`.
    #[inline]
    pub fn has_b(&self) -> bool {
        self.b != 0.0 && !self.b.is_nan()
    }
}

#[inline]
pub fn dist(a: &P, b: &P) -> f64 {
    hypot2(b.x - a.x, b.y - a.y)
}

pub fn has_arcs(pts: &[P]) -> bool {
    pts.iter().any(P::has_b)
}

/// Exact length of one segment p → q with bulge b.
pub fn seg_length(p: &P, q: &P, b: f64) -> f64 {
    let c = dist(p, q);
    if b == 0.0 || b.is_nan() || c == 0.0 || c.is_nan() {
        return c;
    }
    let th = 4.0 * atan(b);
    ((c / 2.0) / sin(th / 2.0) * th).abs()
}

/// Signed area between the chord p → q and its arc.
pub fn seg_area(p: &P, q: &P, b: f64) -> f64 {
    if b == 0.0 || b.is_nan() {
        return 0.0;
    }
    let c = dist(p, q);
    if c == 0.0 || c.is_nan() {
        return 0.0;
    }
    let th = 4.0 * atan(b);
    let r = c / (2.0 * sin(th / 2.0));
    r * r * (th - sin(th)) / 2.0
}

/// An arc's centre, radius, start angle and sweep.
#[derive(Debug, Clone, Copy)]
pub struct Arc {
    pub cx: f64,
    pub cy: f64,
    pub r: f64,
    pub a0: f64,
    pub sweep: f64,
}
pub fn arc_of(p: &P, q: &P, b: f64) -> Option<Arc> {
    if b == 0.0 || b.is_nan() {
        return None;
    }
    let c = dist(p, q);
    if c == 0.0 || c.is_nan() {
        return None;
    }
    let th = 4.0 * atan(b);
    let mx = (p.x + q.x) / 2.0;
    let my = (p.y + q.y) / 2.0;
    let k = (c / 2.0) / tan(th / 2.0);
    let cx = mx - (q.y - p.y) / c * k;
    let cy = my + (q.x - p.x) / c * k;
    let r = hypot2(p.x - cx, p.y - cy);
    Some(Arc { cx, cy, r, a0: atan2(p.y - cy, p.x - cx), sweep: th })
}

/// Bulge of the arc that starts at p, passes through m and ends at q (0 if they are in a line).
pub fn bulge_through(p: &P, m: &P, q: &P) -> f64 {
    let o = (m.x - p.x) * (q.y - p.y) - (m.y - p.y) * (q.x - p.x);
    let span = max(max(dist(p, m), dist(m, q)), dist(p, q));
    if span == 0.0 || span.is_nan() || o.abs() <= 1e-9 * span * span {
        return 0.0;
    }
    let (ax, ay, bx, by, cx, cy) = (p.x, p.y, m.x, m.y, q.x, q.y);
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    let ux = ((ax * ax + ay * ay) * (by - cy) + (bx * bx + by * by) * (cy - ay) + (cx * cx + cy * cy) * (ay - by)) / d;
    let uy = ((ax * ax + ay * ay) * (cx - bx) + (bx * bx + by * by) * (ax - cx) + (cx * cx + cy * cy) * (bx - ax)) / d;
    let a0 = atan2(ay - uy, ax - ux);
    let a2 = atan2(cy - uy, cx - ux);
    let mut sweep = a2 - a0;
    // bounded: sweep is finite here (a NaN would leave both loops at once)
    let mut guard = 0u32;
    if o > 0.0 {
        while sweep <= 0.0 && guard < 64 {
            sweep += 2.0 * PI;
            guard = guard.saturating_add(1);
        }
    } else {
        while sweep >= 0.0 && guard < 64 {
            sweep -= 2.0 * PI;
            guard = guard.saturating_add(1);
        }
    }
    tan(sweep / 4.0)
}

/// Points along the outline/run with arcs broken into chords of at most `step` radians.
pub fn flatten_step(pts: &[P], closed: bool, step: f64) -> Vec<P> {
    let n = pts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    let mut out = Vec::with_capacity(n.saturating_mul(2));
    for i in 0..segs {
        let (Some(p), Some(q)) = (pts.get(i), pts.get((i + 1) % n.max(1))) else { continue };
        out.push(P::new(p.x, p.y));
        let Some(a) = arc_of(p, q, p.b) else { continue };
        let kf = max(2.0, (a.sweep.abs() / step).ceil());
        // a bulge is capped at |b| ≤ 1000 by every reader: the sweep stays under 2π, so k ≤ 2π / step
        let k = if kf.is_finite() && kf < 1e6 { kf } else { 2.0 };
        let mut j = 1.0;
        while j < k {
            let tt = a.a0 + a.sweep * j / k;
            out.push(P::new(a.cx + a.r * cos(tt), a.cy + a.r * sin(tt)));
            j += 1.0;
        }
    }
    if !closed && let Some(last) = pts.last() {
        out.push(P::new(last.x, last.y));
    }
    out
}
/// `flatten(pts, closed)` (5° chords).
pub fn flatten(pts: &[P], closed: bool) -> Vec<P> {
    flatten_step(pts, closed, PI / 36.0)
}

/// A drawing op of a run or outline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    M(P),
    L(P),
    C(P, P, P),
    Z,
}
/// A run or outline as drawing commands (arcs as cubic Béziers of at most 90° each).
pub fn path_ops(pts: &[P], closed: bool) -> Vec<Op> {
    let mut ops = Vec::new();
    let n = pts.len();
    let Some(first) = pts.first() else { return ops };
    ops.push(Op::M(P::new(first.x, first.y)));
    let segs = if closed { n } else { n.saturating_sub(1) };
    for i in 0..segs {
        let (Some(p), Some(q)) = (pts.get(i), pts.get((i + 1) % n)) else { continue };
        let Some(a) = arc_of(p, q, p.b) else {
            ops.push(Op::L(P::new(q.x, q.y)));
            continue;
        };
        let pf = max(1.0, (a.sweep.abs() / (PI / 2.0) - 1e-9).ceil());
        let pieces = if pf.is_finite() && pf < 1e6 { pf } else { 1.0 };
        let phi = a.sweep / pieces;
        let k = 4.0 / 3.0 * tan(phi / 4.0);
        let mut j = 0.0;
        while j < pieces {
            let t0 = a.a0 + phi * j;
            let t1 = t0 + phi;
            let p0 = P::new(a.cx + a.r * cos(t0), a.cy + a.r * sin(t0));
            let p3 = if j == pieces - 1.0 { P::new(q.x, q.y) } else { P::new(a.cx + a.r * cos(t1), a.cy + a.r * sin(t1)) };
            ops.push(Op::C(
                P::new(p0.x - k * a.r * sin(t0), p0.y + k * a.r * cos(t0)),
                P::new(p3.x + k * a.r * sin(t1), p3.y - k * a.r * cos(t1)),
                p3,
            ));
            j += 1.0;
        }
    }
    if closed {
        ops.push(Op::Z);
    }
    ops
}

/// Each segment's length of an open run (arcs exact).
pub fn segment_lengths(pts: &[P]) -> Vec<f64> {
    pts.windows(2).filter_map(|w| if let [a, b] = w { Some(seg_length(a, b, a.b)) } else { None }).collect()
}
pub fn polyline_length(pts: &[P]) -> f64 {
    segment_lengths(pts).iter().fold(0.0, |s, v| s + v)
}
/// Closed outline's perimeter.
pub fn perimeter(pts: &[P]) -> f64 {
    if pts.len() < 2 {
        return 0.0;
    }
    let (Some(last), Some(first)) = (pts.last(), pts.first()) else { return 0.0 };
    polyline_length(pts) + seg_length(last, first, last.b)
}
/// Signed area (positive counter-clockwise): shoelace + each arc's segment.
pub fn signed_area(pts: &[P]) -> f64 {
    let n = pts.len();
    let mut a = 0.0;
    let mut s = 0.0;
    for i in 0..n {
        let (Some(p), Some(q)) = (pts.get(i), pts.get((i + 1) % n)) else { continue };
        a += p.x * q.y - q.x * p.y;
        if p.has_b() {
            s += seg_area(p, q, p.b);
        }
    }
    a / 2.0 + s
}
pub fn polygon_area(pts: &[P]) -> f64 {
    signed_area(pts).abs()
}

/// `{ gross, cut, net, perimeter, outerPerimeter }` of an area mark.
#[derive(Debug, Clone, Copy)]
pub struct AreaParts {
    pub gross: f64,
    pub cut: f64,
    pub net: f64,
    pub perimeter: f64,
    pub outer_perimeter: f64,
}
pub fn area_with_holes(outer: &[P], holes: &[Vec<P>]) -> AreaParts {
    let gross = polygon_area(outer);
    let cut = holes.iter().fold(0.0, |s, h| s + polygon_area(h));
    let p = perimeter(outer) + holes.iter().fold(0.0, |s, h| s + perimeter(h));
    AreaParts { gross, cut, net: gross - cut, perimeter: p, outer_perimeter: perimeter(outer) }
}

/// Ray-cast point in polygon (arcs followed).
pub fn point_in_polygon(p: &P, pts0: &[P]) -> bool {
    let flat;
    let pts: &[P] = if has_arcs(pts0) {
        flat = flatten(pts0, true);
        &flat
    } else {
        pts0
    };
    let mut inside = false;
    let n = pts.len();
    if n == 0 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (Some(a), Some(b)) = (pts.get(i), pts.get(j)) else { break };
        if (a.y > p.y) != (b.y > p.y) && p.x < ((b.x - a.x) * (p.y - a.y)) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[inline]
fn cross(o: &P, a: &P, b: &P) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Do segments ab and cd cross or touch?
pub fn segments_intersect(a: &P, b: &P, c: &P, d: &P, eps: f64) -> bool {
    let s = max(max(dist(a, b), dist(c, d)), 1.0) * eps;
    let d1 = cross(c, d, a);
    let d2 = cross(c, d, b);
    let d3 = cross(a, b, c);
    let d4 = cross(a, b, d);
    if ((d1 > s && d2 < -s) || (d1 < -s && d2 > s)) && ((d3 > s && d4 < -s) || (d3 < -s && d4 > s)) {
        return true;
    }
    let on = |p: &P, q: &P, r: &P, dv: f64| {
        dv.abs() <= s && min(p.x, q.x) - s <= r.x && r.x <= max(p.x, q.x) + s && min(p.y, q.y) - s <= r.y && r.y <= max(p.y, q.y) + s
    };
    on(c, d, a, d1) || on(c, d, b, d2) || on(a, b, c, d3) || on(a, b, d, d4)
}

/// Does a closed outline cross itself?
pub fn self_intersects(pts0: &[P]) -> bool {
    let flat;
    let pts: &[P] = if has_arcs(pts0) {
        flat = flatten(pts0, true);
        &flat
    } else {
        pts0
    };
    let n = pts.len();
    if n < 4 {
        return false;
    }
    for i in 0..n {
        let (Some(a), Some(b)) = (pts.get(i), pts.get((i + 1) % n)) else { continue };
        for j in i.saturating_add(1)..n {
            if j == i || (j + 1) % n == i || (i + 1) % n == j {
                continue;
            }
            let (Some(c), Some(d)) = (pts.get(j), pts.get((j + 1) % n)) else { continue };
            if segments_intersect(a, b, c, d, 1e-9) {
                return true;
            }
        }
    }
    false
}

/// Do two closed outlines' edges cross or touch?
pub fn outlines_cross(p0: &[P], q0: &[P]) -> bool {
    let (fp, fq);
    let p: &[P] = if has_arcs(p0) {
        fp = flatten(p0, true);
        &fp
    } else {
        p0
    };
    let q: &[P] = if has_arcs(q0) {
        fq = flatten(q0, true);
        &fq
    } else {
        q0
    };
    for i in 0..p.len() {
        let (Some(a), Some(b)) = (p.get(i), p.get((i + 1) % p.len())) else { continue };
        for j in 0..q.len() {
            let (Some(c), Some(d)) = (q.get(j), q.get((j + 1) % q.len())) else { continue };
            if segments_intersect(a, b, c, d, 1e-9) {
                return true;
            }
        }
    }
    false
}

/// Why an outline can't be measured, or None.
pub fn outline_problem(pts: &[P], what: &str) -> Option<String> {
    if pts.len() < 3 && !(pts.len() == 2 && has_arcs(pts)) {
        return Some(t(&format!("geom.{what}.min")));
    }
    if self_intersects(pts) {
        return Some(t(&format!("geom.{what}.cross")));
    }
    if polygon_area(pts) < 1e-9 {
        return Some(t(&format!("geom.{what}.empty")));
    }
    None
}

/// Why a cut-out can't go into an area, or None.
pub fn hole_problem(outer: &[P], holes: &[Vec<P>], hole: &[P]) -> Option<String> {
    if let Some(p) = outline_problem(hole, "hole") {
        return Some(p);
    }
    let fh = flatten(hole, true);
    if outlines_cross(outer, hole) || !fh.iter().all(|v| point_in_polygon(v, outer)) {
        return Some(t("geom.hole.inside"));
    }
    for h in holes {
        if outlines_cross(h, hole)
            || fh.iter().any(|v| point_in_polygon(v, h))
            || flatten(h, true).iter().any(|v| point_in_polygon(v, hole))
        {
            return Some(t("geom.hole.overlap"));
        }
    }
    None
}

/// Snap p to the nearest 45° direction from base, keeping its distance along that direction.
pub fn snap45(base: &P, p: &P) -> P {
    let dx = p.x - base.x;
    let dy = p.y - base.y;
    let ang = cc_m3d::js::round(atan2(dy, dx) / (PI / 4.0)) * (PI / 4.0);
    let ux = cc_m3d::js::round(cos(ang) * 1e12) / 1e12;
    let uy = cc_m3d::js::round(sin(ang) * 1e12) / 1e12;
    let tt = dx * ux + dy * uy;
    P::new(base.x + ux * tt, base.y + uy * tt)
}

/// Distance from p to segment ab.
pub fn dist_to_segment(p: &P, a: &P, b: &P) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let l2 = dx * dx + dy * dy;
    if l2 == 0.0 || l2.is_nan() {
        return dist(p, a);
    }
    let tt = max(0.0, min(1.0, ((p.x - a.x) * dx + (p.y - a.y) * dy) / l2));
    hypot2(p.x - (a.x + tt * dx), p.y - (a.y + tt * dy))
}

/// Distance from p to an open polyline (or a closed outline when closed).
pub fn dist_to_path(p: &P, pts0: &[P], closed0: bool) -> f64 {
    let mut closed = closed0;
    let flat;
    let pts: &[P] = if has_arcs(pts0) {
        flat = flatten(pts0, closed);
        closed = false;
        &flat
    } else {
        pts0
    };
    let mut d = f64::INFINITY;
    for w in pts.windows(2) {
        if let [a, b] = w {
            d = min(d, dist_to_segment(p, a, b));
        }
    }
    if closed
        && pts.len() > 2
        && let (Some(l), Some(f)) = (pts.last(), pts.first())
    {
        d = min(d, dist_to_segment(p, l, f));
    }
    if pts.len() == 1
        && let Some(f) = pts.first()
    {
        d = dist(p, f);
    }
    d
}

/// `{ minX, minY, maxX, maxY }`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}
pub fn bounds(pts0: &[P]) -> Bounds {
    let flat;
    let pts: &[P] = if has_arcs(pts0) {
        flat = flatten(pts0, true);
        &flat
    } else {
        pts0
    };
    let mut b = Bounds { min_x: f64::INFINITY, min_y: f64::INFINITY, max_x: f64::NEG_INFINITY, max_y: f64::NEG_INFINITY };
    for p in pts {
        b.min_x = min(b.min_x, p.x);
        b.min_y = min(b.min_y, p.y);
        b.max_x = max(b.max_x, p.x);
        b.max_y = max(b.max_y, p.y);
    }
    b
}

pub fn centroid(pts0: &[P]) -> P {
    let flat;
    let pts: &[P] = if has_arcs(pts0) {
        flat = flatten(pts0, true);
        &flat
    } else {
        pts0
    };
    let a = signed_area(pts);
    if a.abs() < 1e-12 {
        let b = bounds(pts);
        return P::new((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0);
    }
    let n = pts.len();
    let (mut cx, mut cy) = (0.0, 0.0);
    for i in 0..n {
        let (Some(p), Some(q)) = (pts.get(i), pts.get((i + 1) % n)) else { continue };
        let f = p.x * q.y - q.x * p.y;
        cx += (p.x + q.x) * f;
        cy += (p.y + q.y) * f;
    }
    P::new(cx / (6.0 * a), cy / (6.0 * a))
}

/// A 2D affine matrix `[a, b, c, d, e, f]` (PDF order).
pub type M6 = [f64; 6];
pub fn apply(m: &M6, p: &P) -> P {
    P::new(m[0] * p.x + m[2] * p.y + m[4], m[1] * p.x + m[3] * p.y + m[5])
}
pub fn multiply(m: &M6, n: &M6) -> M6 {
    [
        m[0] * n[0] + m[2] * n[1],
        m[1] * n[0] + m[3] * n[1],
        m[0] * n[2] + m[2] * n[3],
        m[1] * n[2] + m[3] * n[3],
        m[0] * n[4] + m[2] * n[5] + m[4],
        m[1] * n[4] + m[3] * n[5] + m[5],
    ]
}
pub fn invert(m: &M6) -> M6 {
    let det = m[0] * m[3] - m[1] * m[2];
    [m[3] / det, -m[1] / det, -m[2] / det, m[0] / det, (m[2] * m[5] - m[3] * m[4]) / det, (m[1] * m[4] - m[0] * m[5]) / det]
}

/// A stretch of a run: counted or deducted.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub deduct: bool,
    pub pts: Vec<P>,
}
/// A run cut into its counted and deducted stretches.
pub fn spans(pts: &[P], ded: &dyn Fn(usize) -> bool) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    for i in 0..pts.len().saturating_sub(1) {
        let d = ded(i);
        let (Some(pi), Some(pn)) = (pts.get(i), pts.get(i + 1)) else { continue };
        let new_span = out.last().is_none_or(|c| c.deduct != d);
        if new_span {
            if let Some(cur) = out.last_mut()
                && let Some(l) = cur.pts.last_mut()
            {
                *l = P::new(pi.x, pi.y);
            }
            out.push(Span { deduct: d, pts: vec![*pi] });
        }
        if let Some(cur) = out.last_mut() {
            cur.pts.push(*pn);
        }
    }
    if let Some(cur) = out.last_mut()
        && let Some(l) = cur.pts.last_mut()
    {
        *l = P::new(l.x, l.y);
    }
    out
}

const FINE: f64 = PI / 360.0;

/// Is a closed outline convex (arcs flattened at 0.5°)?
pub fn is_convex(pts0: &[P]) -> bool {
    let pts = flatten_step(pts0, true, FINE);
    let n = pts.len();
    if n < 3 {
        return false;
    }
    let mut sign = 0.0f64;
    let scale = max(pts.iter().fold(f64::NEG_INFINITY, |m, p| max(m, p.x.abs() + p.y.abs())), 1.0);
    for i in 0..n {
        let (Some(a), Some(b), Some(c)) = (pts.get(i), pts.get((i + 1) % n), pts.get((i + 2) % n)) else { continue };
        let c = cross(a, b, c);
        if c.abs() <= 1e-12 * scale * scale {
            continue;
        }
        if sign == 0.0 || sign.is_nan() {
            sign = cc_m3d::js::sign(c);
        } else if cc_m3d::js::sign(c) != sign {
            return false;
        }
    }
    sign != 0.0
}

/// Sutherland–Hodgman: the part of `subject` inside the CONVEX polygon `clip`.
pub fn clip_convex(subject: &[P], clip: &[P]) -> Vec<P> {
    let ccw: Vec<P> = if signed_area(clip) > 0.0 { clip.to_vec() } else { clip.iter().rev().copied().collect() };
    let mut out: Vec<P> = subject.to_vec();
    let mut i = 0usize;
    while i < ccw.len() && !out.is_empty() {
        let (Some(a), Some(b)) = (ccw.get(i), ccw.get((i + 1) % ccw.len())) else { break };
        let inside = |p: &P| cross(a, b, p) >= 0.0;
        let meet = |p: &P, q: &P| {
            let d1 = cross(a, b, p);
            let d2 = cross(a, b, q);
            let tt = d1 / (d1 - d2);
            P::new(p.x + (q.x - p.x) * tt, p.y + (q.y - p.y) * tt)
        };
        let inp = std::mem::take(&mut out);
        for j in 0..inp.len() {
            let (Some(p), Some(q)) = (inp.get(j), inp.get((j + 1) % inp.len())) else { continue };
            let pin = inside(p);
            let qin = inside(q);
            if pin {
                out.push(*p);
            }
            if pin != qin {
                out.push(meet(p, q));
            }
        }
        i = i.saturating_add(1);
    }
    out
}

/// How much of an area mark (outline minus holes) a convex back-out covers, in square PDF units.
pub fn overlap_area(outer: &[P], holes: &[Vec<P>], b: &[P]) -> f64 {
    let ob = bounds(outer);
    let bb = bounds(b);
    if bb.min_x >= ob.max_x || bb.max_x <= ob.min_x || bb.min_y >= ob.max_y || bb.max_y <= ob.min_y {
        return 0.0;
    }
    let fb = flatten_step(b, true, FINE);
    let clear = |h: &Vec<P>| {
        !outlines_cross(h, b) && !fb.iter().any(|v| point_in_polygon(v, h)) && !flatten(h, true).iter().any(|v| point_in_polygon(v, b))
    };
    if !outlines_cross(outer, b) && fb.iter().all(|v| point_in_polygon(v, outer)) && holes.iter().all(clear) {
        return polygon_area(b);
    }
    let part = |ring: &[P]| polygon_area(&clip_convex(&flatten_step(ring, true, FINE), &fb));
    max(0.0, part(outer) - holes.iter().fold(0.0, |s, h| s + part(h)))
}
