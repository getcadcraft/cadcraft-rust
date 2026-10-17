//! holes.js — cut-outs in an area measurement: ring math, the checks a new cut-out passes, and `cleanHoles` for
//! untrusted data.

use crate::js::{hypot, sign, to_number};
use serde_json::Value;

pub type P = [f64; 2];

/// `ringArea(pts)` (absolute).
pub fn ring_area(pts: &[P]) -> f64 {
    let n = pts.len();
    let mut a = 0.0;
    for i in 0..n {
        let (Some(p), Some(q)) = (pts.get(i), pts.get(crate::js::next_i(i, n))) else { continue };
        a += p[0] * q[1] - q[0] * p[1];
    }
    a.abs() / 2.0
}

/// `ringPerimeter(pts)`.
pub fn ring_perimeter(pts: &[P]) -> f64 {
    let n = pts.len();
    let mut s = 0.0;
    for i in 0..n {
        let (Some(p), Some(q)) = (pts.get(i), pts.get(crate::js::next_i(i, n))) else { continue };
        s += hypot(q[0] - p[0], q[1] - p[1]);
    }
    s
}

/// `inRing(p, pts)` (ray cast).
pub fn in_ring(p: P, pts: &[P]) -> bool {
    let n = pts.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        if let (Some(a), Some(b)) = (pts.get(i), pts.get(j))
            && (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < ((b[0] - a[0]) * (p[1] - a[1])) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn orient(a: P, b: P, c: P) -> f64 {
    sign((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]))
}
fn segs_cross(a: P, b: P, c: P, d: P) -> bool {
    let (o1, o2, o3, o4) = (orient(a, b, c), orient(a, b, d), orient(c, d, a), orient(c, d, b));
    o1 * o2 < 0.0 && o3 * o4 < 0.0
}
fn edges(pts: &[P]) -> Vec<(P, P)> {
    let n = pts.len();
    (0..n).filter_map(|i| Some((*pts.get(i)?, *pts.get(crate::js::next_i(i, n))?))).collect()
}

/// `crosses(a, b)`: do two closed outlines cross?
pub fn crosses(a: &[P], b: &[P]) -> bool {
    let eb = edges(b);
    edges(a).iter().any(|(p, q)| eb.iter().any(|(r, s)| segs_cross(*p, *q, *r, *s)))
}

/// `selfCrosses(pts)`.
pub fn self_crosses(pts: &[P]) -> bool {
    let e = edges(pts);
    let n = e.len();
    for i in 0..n {
        for j in i.saturating_add(2)..n {
            if i == 0 && j == n.saturating_sub(1) {
                continue;
            }
            if let (Some(x), Some(y)) = (e.get(i), e.get(j))
                && segs_cross(x.0, x.1, y.0, y.1)
            {
                return true;
            }
        }
    }
    false
}

/// `holeProblem(outer, holes, hole)` → None, or the language key of why the cut-out cannot go in.
pub fn hole_problem(outer: &[P], holes: &[Vec<P>], hole: &[P]) -> Option<&'static str> {
    if hole.len() < 3 {
        return Some("hole.err.points");
    }
    if self_crosses(hole) {
        return Some("hole.err.cross");
    }
    if ring_area(hole) < 1e-6 {
        return Some("hole.err.points");
    }
    if crosses(outer, hole) || !hole.iter().all(|p| in_ring(*p, outer)) {
        return Some("hole.err.outside");
    }
    for h in holes {
        if crosses(h, hole) || hole.iter().any(|p| in_ring(*p, h)) || h.iter().any(|p| in_ring(*p, hole)) {
            return Some("hole.err.overlap");
        }
    }
    None
}

/// `cleanHoles(v)`: untrusted → ≤ 200 holes of 3–10 000 finite points.
pub fn clean_holes(v: Option<&Value>) -> Vec<Vec<P>> {
    let Some(Value::Array(list)) = v else { return Vec::new() };
    let ok = |h: &Value| -> bool {
        let Value::Array(h) = h else { return false };
        h.len() >= 3
            && h.len() <= 10000
            && h.iter().all(|p| {
                let Value::Array(p) = p else { return false };
                if p.len() < 2 {
                    return false;
                }
                let (Some(x), Some(y)) = (p.first(), p.get(1)) else { return false };
                let (nx, ny) = (to_number(x), to_number(y));
                nx.is_finite() && ny.is_finite() && nx.abs() < 1e7 && ny.abs() < 1e7
            })
    };
    list.iter()
        .filter(|h| ok(h))
        .take(200)
        .map(|h| {
            h.as_array()
                .map(|pts| {
                    pts.iter()
                        .map(|p| {
                            let a = p.as_array();
                            let g = |i: usize| a.and_then(|a| a.get(i)).map_or(f64::NAN, to_number);
                            [g(0), g(1)]
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect()
}
