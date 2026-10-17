//! lasso-core.js — lasso selection shared by the 2D apps (window, crossing, fence): `lassoMode`, `nextMode`, `thin`,
//! `pointInPoly`, `lassoSelect`. `segsCross` is cc-civil's port of the same file.

use crate::js::{hypot, max, min};
pub use cc_civil::points::segs_cross;

/// A screen point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}
/// An item's screen box.
#[derive(Clone, Copy, Debug)]
pub struct Box2 {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

pub const LASSO_MODES: [&str; 3] = ["window", "crossing", "fence"];
/// `nextMode(mode)`.
pub fn next_mode(m: &str) -> &'static str {
    let i = LASSO_MODES.iter().position(|x| *x == m).map_or(0, |i| (i + 1) % 3);
    LASSO_MODES.get(i).copied().unwrap_or("window")
}
fn dist(a: Pt, b: Pt) -> f64 {
    hypot(a.x - b.x, a.y - b.y)
}
/// `lassoMode(start, cur, minPx)`.
pub fn lasso_mode(start: Pt, cur: Pt, min_px: f64) -> Option<&'static str> {
    let dx = cur.x - start.x;
    if dx.abs() < min_px {
        return None;
    }
    Some(if dx > 0.0 { "window" } else { "crossing" })
}
/// `thin(pts, minStep)`.
pub fn thin(pts: &[Pt], min_step: f64) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::new();
    for &p in pts {
        if out.last().is_none_or(|l| dist(*l, p) >= min_step) {
            out.push(p);
        }
    }
    if let (true, Some(&l), Some(&last)) = (pts.len() > 1, out.last(), pts.last())
        && dist(l, last) > 1e-12
    {
        out.push(last);
    }
    out
}
/// `pointInPoly(p, pts)` (even-odd; the lasso closes itself).
pub fn point_in_poly(p: Pt, pts: &[Pt]) -> bool {
    let mut inside = false;
    let n = pts.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        if let (Some(a), Some(b)) = (pts.get(i), pts.get(j))
            && (a.y > p.y) != (b.y > p.y)
            && p.x < ((b.x - a.x) * (p.y - a.y)) / (b.y - a.y) + a.x
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// `lassoSelect(items, pts, mode, { box, segs, filled })` → indices of the items it selects, in order.
pub fn lasso_select<T>(
    items: &[T],
    pts: &[Pt],
    mode: &str,
    bx: &dyn Fn(&T) -> Option<Box2>,
    segs: &dyn Fn(&T) -> Vec<[f64; 4]>,
    filled: &dyn Fn(&T) -> bool,
) -> Vec<usize> {
    let mut out = Vec::new();
    if pts.len() < 2 {
        return out;
    }
    let mut path: Vec<[f64; 4]> = Vec::new();
    let n = if mode == "fence" { pts.len().saturating_sub(1) } else { pts.len() };
    for i in 0..n {
        let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i.saturating_add(1)) % pts.len())) else { continue };
        if dist(a, b) > 1e-12 {
            path.push([a.x, a.y, b.x, b.y]);
        }
    }
    if path.is_empty() || (mode != "fence" && pts.len() < 3) {
        return out;
    }
    let (mut lx0, mut ly0, mut lx1, mut ly1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in pts {
        lx0 = min(lx0, p.x);
        ly0 = min(ly0, p.y);
        lx1 = max(lx1, p.x);
        ly1 = max(ly1, p.y);
    }
    let crosses = |ss: &[[f64; 4]]| ss.iter().any(|s| path.iter().any(|q| segs_cross(s[0], s[1], s[2], s[3], q[0], q[1], q[2], q[3])));
    for (k, it) in items.iter().enumerate() {
        let Some(b) = bx(it) else { continue };
        if !(b.max_x >= lx0 && b.min_x <= lx1 && b.max_y >= ly0 && b.min_y <= ly1) {
            continue;
        }
        let ss = segs(it);
        if mode == "fence" {
            if crosses(&ss) {
                out.push(k);
            }
            continue;
        }
        if mode == "window" {
            if !(b.min_x >= lx0 && b.max_x <= lx1 && b.min_y >= ly0 && b.max_y <= ly1) {
                continue;
            }
            if !ss.is_empty() && !crosses(&ss) && ss.iter().all(|s| point_in_poly(Pt { x: s[0], y: s[1] }, pts)) {
                out.push(k);
            }
            continue;
        }
        if crosses(&ss)
            || ss.iter().any(|s| point_in_poly(Pt { x: s[0], y: s[1] }, pts))
            || (filled(it) && pts.iter().any(|q| q.x >= b.min_x && q.x <= b.max_x && q.y >= b.min_y && q.y <= b.max_y))
        {
            out.push(k);
        }
    }
    out
}
