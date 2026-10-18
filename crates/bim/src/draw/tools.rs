//! model-tools.js — where a click lands in the plan view: snapping for the drawing tools (wall ends and face corners,
//! middles, directions from the last point in 15° steps or ortho, stops on walls, points on walls, rounded free
//! lengths), whether a click closes the chain, and which side is outside for a closed loop.

use crate::draw::m::{self, V2, at, dist, nx};

const D2R: f64 = std::f64::consts::PI / 180.0;

/// A wall for snapping: centre line and thickness.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapWall {
    pub a: V2,
    pub b: V2,
    pub t: f64,
}

/// What a snap found: the point, its kind ('end' | 'mid' | 'on' | 'angle' | 'ortho' | 'free'), the angle, the wall.
#[derive(Clone, Debug, PartialEq)]
pub struct Snap {
    pub p: V2,
    pub kind: &'static str,
    pub angle: Option<f64>,
    pub wall: Option<usize>,
}

/// The snap settings.
#[derive(Clone, Copy, Debug)]
pub struct SnapOpts {
    pub tol: f64,
    pub from: Option<V2>,
    pub ortho: bool,
    pub step: f64,
    pub round: f64,
}

/// `snapPoint(p, { walls, points }, opts)`
pub fn snap_point(p: V2, walls: &[SnapWall], points: &[V2], o: SnapOpts) -> Snap {
    let mut best: Option<(V2, &'static str, u8, f64, Option<usize>)> = None;
    let mut offer = |q: V2, kind: &'static str, rank: u8, wall: Option<usize>| {
        let d = dist(p, q);
        if d <= o.tol && best.is_none_or(|b| rank < b.2 || (rank == b.2 && d < b.3)) {
            best = Some((q, kind, rank, d, wall));
        }
    };
    for (wi, w) in walls.iter().enumerate() {
        offer(w.a, "end", 0, Some(wi));
        offer(w.b, "end", 0, Some(wi));
        offer([(w.a[0] + w.b[0]) / 2.0, (w.a[1] + w.b[1]) / 2.0], "mid", 1, Some(wi));
        let l = dist(w.a, w.b);
        if l > 0.0 && w.t > 0.0 {
            let n = [-(w.b[1] - w.a[1]) / l * w.t / 2.0, (w.b[0] - w.a[0]) / l * w.t / 2.0];
            for e in [w.a, w.b] {
                offer([e[0] + n[0], e[1] + n[1]], "end", 0, Some(wi));
                offer([e[0] - n[0], e[1] - n[1]], "end", 0, Some(wi));
            }
        }
    }
    for q in points {
        offer(*q, "end", 0, None);
    }
    if let Some(b) = best {
        return Snap { p: b.0, kind: b.1, angle: None, wall: b.4 };
    }
    if let Some(from) = o.from {
        let (dx, dy) = (p[0] - from[0], p[1] - from[1]);
        let len = m::hypot(dx, dy);
        if len > 0.0 {
            let mut ang = m::atan2(dy, dx) / D2R;
            let mut kind = None;
            if o.ortho {
                ang = m::round(ang / 90.0) * 90.0;
                kind = Some("ortho");
            } else if o.step > 0.0 {
                let s = m::round(ang / o.step) * o.step;
                if (s - ang).abs() <= 3.0 {
                    ang = s;
                    kind = Some("angle");
                }
            }
            if let Some(kind) = kind {
                let mut l = len * m::cos((m::atan2(dy, dx) / D2R - ang) * D2R);
                let dir = [m::cos(ang * D2R), m::sin(ang * D2R)];
                let mut hit: Option<(f64, usize)> = None;
                for (wi, w) in walls.iter().enumerate() {
                    for off in [0.0, w.t / 2.0, -w.t / 2.0] {
                        let wl = dist(w.a, w.b);
                        if wl == 0.0 || wl.is_nan() {
                            continue;
                        }
                        let u = [(w.b[0] - w.a[0]) / wl, (w.b[1] - w.a[1]) / wl];
                        let n = [-u[1], u[0]];
                        let a = [w.a[0] + n[0] * off, w.a[1] + n[1] * off];
                        let c = dir[0] * u[1] - dir[1] * u[0];
                        if c.abs() < 1e-9 {
                            continue;
                        }
                        let s = ((a[0] - from[0]) * u[1] - (a[1] - from[1]) * u[0]) / c;
                        let tt = ((a[0] - from[0]) * dir[1] - (a[1] - from[1]) * dir[0]) / c;
                        if s > 1.0
                            && tt >= -1.0
                            && tt <= wl + 1.0
                            && (s - l).abs() <= o.tol
                            && hit.is_none_or(|h| (s - l).abs() < (h.0 - l).abs())
                        {
                            hit = Some((s, wi));
                        }
                    }
                }
                if let Some((s, wi)) = hit {
                    return Snap { p: [from[0] + dir[0] * s, from[1] + dir[1] * s], kind: "on", angle: Some(ang), wall: Some(wi) };
                }
                if o.round > 0.0 {
                    l = m::max(o.round, m::round(l / o.round) * o.round);
                }
                return Snap { p: [from[0] + dir[0] * l, from[1] + dir[1] * l], kind, angle: Some(ang), wall: None };
            }
        }
    }
    let mut on: Option<(V2, f64, usize)> = None;
    for (wi, w) in walls.iter().enumerate() {
        let wl = dist(w.a, w.b);
        if wl == 0.0 || wl.is_nan() {
            continue;
        }
        let u = [(w.b[0] - w.a[0]) / wl, (w.b[1] - w.a[1]) / wl];
        let n = [-u[1], u[0]];
        for off in [0.0, w.t / 2.0, -w.t / 2.0] {
            let a = [w.a[0] + n[0] * off, w.a[1] + n[1] * off];
            let s = m::max(0.0, m::min(wl, (p[0] - a[0]) * u[0] + (p[1] - a[1]) * u[1]));
            let q = [a[0] + u[0] * s, a[1] + u[1] * s];
            let d = dist(p, q);
            if d <= o.tol * 0.6 && on.is_none_or(|x| d < x.1) {
                on = Some((q, d, wi));
            }
        }
    }
    if let Some((q, _, wi)) = on {
        return Snap { p: q, kind: "on", angle: None, wall: Some(wi) };
    }
    if let Some(from) = o.from
        && o.round > 0.0
    {
        let (dx, dy) = (p[0] - from[0], p[1] - from[1]);
        let len = m::hypot(dx, dy);
        if len > 0.0 {
            let l = m::max(o.round, m::round(len / o.round) * o.round);
            return Snap { p: [from[0] + dx / len * l, from[1] + dy / len * l], kind: "free", angle: None, wall: None };
        }
    }
    Snap { p, kind: "free", angle: None, wall: None }
}

/// `closesChain(points, p, tol)`
pub fn closes_chain(points: &[V2], p: V2, tol: f64) -> bool {
    points.len() >= 3 && points.first().is_some_and(|f| dist(*f, p) <= tol)
}

/// Signed area of a closed loop of points.
pub fn loop_area(points: &[V2]) -> f64 {
    let n = points.len();
    let mut s = 0.0;
    for i in 0..n {
        let (a, b) = (at(points, i), at(points, nx(i, n)));
        s += a[0] * b[1] - a[1] * b[0];
    }
    s / 2.0
}

/// `loopSide(points)`: an anticlockwise loop has its outside on the right.
pub fn loop_side(points: &[V2]) -> &'static str {
    if loop_area(points) > 0.0 { "right" } else { "left" }
}
