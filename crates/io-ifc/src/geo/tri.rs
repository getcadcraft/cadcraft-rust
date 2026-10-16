//! geo-tri.js — triangulating a planar polygon with holes by ear clipping. Each hole is first joined to the outer ring
//! by a bridge to a vertex it can see (the ray-cast rule: from the hole's right-most vertex to the right), which makes
//! one ring; ears are then cut, convex ears with no other vertex inside. Duplicate and collinear points are dropped; a
//! ring that cannot be finished (self-crossing input) is closed with a fan rather than lost.
//!
//! The JS links node objects; here they live in an arena and link by index (a node removed from the ring keeps its
//! own links, as in the JS).

use super::math::{V2, V3, newell};

#[derive(Clone, Copy, Debug)]
struct Node {
    i: usize,
    x: f64,
    y: f64,
    prev: usize,
    next: usize,
}

struct Arena {
    n: Vec<Node>,
}

const NIL: usize = usize::MAX;

impl Arena {
    #[inline]
    fn g(&self, k: usize) -> Node {
        self.n.get(k).copied().unwrap_or(Node { i: NIL, x: f64::NAN, y: f64::NAN, prev: k, next: k })
    }
    #[inline]
    fn next(&self, k: usize) -> usize {
        self.g(k).next
    }
    #[inline]
    fn prev(&self, k: usize) -> usize {
        self.g(k).prev
    }
    fn set_next(&mut self, k: usize, v: usize) {
        if let Some(n) = self.n.get_mut(k) {
            n.next = v;
        }
    }
    fn set_prev(&mut self, k: usize, v: usize) {
        if let Some(n) = self.n.get_mut(k) {
            n.prev = v;
        }
    }
    fn push(&mut self, i: usize, x: f64, y: f64) -> usize {
        let k = self.n.len();
        self.n.push(Node { i, x, y, prev: k, next: k });
        k
    }
    /// unlink n (its own links stay)
    fn remove(&mut self, n: usize) {
        let (p, q) = (self.prev(n), self.next(n));
        self.set_next(p, q);
        self.set_prev(q, p);
    }
}

struct RingInfo {
    start: usize,
    n: usize,
}

/// Triangulate → indexes into [...outer, ...holes.flat()].
pub fn triangulate(outer: &[V2], holes: &[Vec<V2>]) -> Vec<usize> {
    let mut pts: Vec<V2> = Vec::with_capacity(outer.len());
    let mut rings = Vec::with_capacity(holes.len().saturating_add(1));
    let mut base = 0usize;
    for r in std::iter::once(outer).chain(holes.iter().map(Vec::as_slice)) {
        rings.push(RingInfo { start: base, n: r.len() });
        pts.extend_from_slice(r);
        base = base.saturating_add(r.len());
    }
    if outer.len() < 3 {
        return Vec::new();
    }
    let area = |r: &RingInfo| {
        let mut s = 0.0;
        let e = r.start.saturating_add(r.n);
        for i in r.start..e {
            let a = pts.get(i).copied().unwrap_or([f64::NAN; 2]);
            let j = if i.saturating_add(1) < e { i.saturating_add(1) } else { r.start };
            let b = pts.get(j).copied().unwrap_or([f64::NAN; 2]);
            s += a[0] * b[1] - b[0] * a[1];
        }
        s / 2.0
    };
    let mut ar = Arena { n: Vec::with_capacity(pts.len().saturating_mul(2)) };
    let Some(r0) = rings.first() else { return Vec::new() };
    let Some(mut head) = ring(&mut ar, &pts, r0, area(r0) > 0.0) else { return Vec::new() };
    let mut hs = Vec::new();
    for r in rings.iter().skip(1) {
        if r.n < 3 {
            continue;
        }
        if let Some(node) = ring(&mut ar, &pts, r, area(r) < 0.0) {
            hs.push(rightmost(&ar, node));
        }
    }
    // hs.sort((a, b) => b.x - a.x): stable, descending x
    hs.sort_by(|&a, &b| {
        let d = ar.g(b).x - ar.g(a).x;
        if d < 0.0 {
            std::cmp::Ordering::Less
        } else if d > 0.0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    for h in hs {
        head = bridge_hole(&mut ar, h, head);
    }
    let mut out = Vec::new();
    clip(&mut ar, head, &mut out);
    out
}

/// A closed linked ring of ring r's points, in order when keep_order (else reversed). Drops repeats.
fn ring(ar: &mut Arena, pts: &[V2], r: &RingInfo, keep_order: bool) -> Option<usize> {
    let mut last = NIL;
    let mut first = NIL;
    let mut add = |ar: &mut Arena, i: usize| {
        let p = pts.get(i).copied().unwrap_or([f64::NAN; 2]);
        if last != NIL {
            let l = ar.g(last);
            if l.x == p[0] && l.y == p[1] {
                return;
            }
        }
        let n = ar.push(i, p[0], p[1]);
        if last == NIL {
            first = n;
        } else {
            let ln = ar.next(last);
            ar.set_next(n, ln);
            ar.set_prev(n, last);
            ar.set_prev(ln, n);
            ar.set_next(last, n);
        }
        last = n;
    };
    let end = r.start.saturating_add(r.n);
    if keep_order {
        for i in r.start..end {
            add(ar, i);
        }
    } else {
        for i in (r.start..end).rev() {
            add(ar, i);
        }
    }
    if last != NIL && first != last {
        let (l, f) = (ar.g(last), ar.g(first));
        if l.x == f.x && l.y == f.y {
            ar.remove(last);
            last = ar.prev(last);
        }
    }
    let _ = last;
    if first == NIL || ar.next(first) == first || ar.next(ar.next(first)) == first {
        return None;
    }
    Some(first)
}

fn rightmost(ar: &Arena, n: usize) -> usize {
    let mut r = n;
    let mut p = n;
    let mut guard = 0usize;
    loop {
        let (pp, rr) = (ar.g(p), ar.g(r));
        if pp.x > rr.x || (pp.x == rr.x && pp.y < rr.y) {
            r = p;
        }
        p = pp.next;
        guard = guard.saturating_add(1);
        if p == n || guard > ar.n.len() {
            break;
        }
    }
    r
}

#[inline]
fn cross(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}
#[inline]
fn xy(n: Node) -> (f64, f64) {
    (n.x, n.y)
}
fn in_tri(a: (f64, f64), b: (f64, f64), c: (f64, f64), p: (f64, f64)) -> bool {
    cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
}

/// Join a hole (rightmost vertex m) to the outer ring; → the outer ring's head.
fn bridge_hole(ar: &mut Arena, m: usize, outer: usize) -> usize {
    let mm = ar.g(m);
    let guard_max = ar.n.len().saturating_add(1);
    // the nearest edge crossing the ray from m to +x
    let mut p = outer;
    let mut best = NIL;
    let mut qx = f64::INFINITY;
    let mut guard = 0usize;
    loop {
        let a = ar.g(p);
        let b = ar.g(a.next);
        if (a.y <= mm.y && b.y >= mm.y) || (b.y <= mm.y && a.y >= mm.y) {
            if a.y != b.y {
                let x = a.x + (mm.y - a.y) * (b.x - a.x) / (b.y - a.y);
                if x >= mm.x && x < qx {
                    qx = x;
                    best = if a.x > b.x { p } else { a.next };
                    if x == mm.x {
                        break;
                    }
                }
            } else {
                let mn = crate::jsmath::min(a.x, b.x);
                if mn >= mm.x && mn < qx {
                    qx = mn;
                    best = if a.x < b.x { p } else { a.next };
                }
            }
        }
        p = a.next;
        guard = guard.saturating_add(1);
        if p == outer || guard > guard_max {
            break;
        }
    }
    if best == NIL {
        return outer;
    }
    // a vertex inside the triangle (m, hit, best) with the smallest angle to the ray is visible instead
    let hit = (qx, mm.y);
    let bb = ar.g(best);
    let mut sel = best;
    let mut tan_min = f64::INFINITY;
    p = outer;
    guard = 0;
    loop {
        let pp = ar.g(p);
        if p != best && mm.x <= pp.x && pp.x <= bb.x {
            // the JS tests the two windings in an order depending on m.y < best.y; either order gives the same answer
            let inside = in_tri(xy(mm), hit, xy(bb), xy(pp)) || in_tri(xy(mm), xy(bb), hit, xy(pp));
            if inside && !(pp.x == mm.x && pp.y == mm.y) {
                let tan = (mm.y - pp.y).abs() / super::math::or(pp.x - mm.x, 1e-300);
                if tan < tan_min || (tan == tan_min && pp.x > ar.g(sel).x) {
                    tan_min = tan;
                    sel = p;
                }
            }
        }
        p = pp.next;
        guard = guard.saturating_add(1);
        if p == outer || guard > guard_max {
            break;
        }
    }
    split(ar, sel, m);
    outer
}

/// Link a and b with a bridge (two new nodes duplicate them): the rings become one.
fn split(ar: &mut Arena, a: usize, b: usize) {
    let (na, nb) = (ar.g(a), ar.g(b));
    let a2 = ar.push(na.i, na.x, na.y);
    let b2 = ar.push(nb.i, nb.x, nb.y);
    let an = na.next;
    let bp = nb.prev;
    ar.set_next(a, b);
    ar.set_prev(b, a);
    ar.set_next(a2, an);
    ar.set_prev(an, a2);
    ar.set_next(b2, a2);
    ar.set_prev(a2, b2);
    ar.set_next(bp, b2);
    ar.set_prev(b2, bp);
}

fn is_ear(ar: &Arena, e: usize) -> bool {
    let b = ar.g(e);
    let a = ar.g(b.prev);
    let c = ar.g(b.next);
    if cross(xy(a), xy(b), xy(c)) <= 0.0 {
        return false;
    }
    let mut p = c.next;
    let mut guard = 0usize;
    while p != b.prev {
        let pp = ar.g(p);
        if pp.i != a.i
            && pp.i != b.i
            && pp.i != c.i
            && (pp.x != a.x || pp.y != a.y)
            && (pp.x != c.x || pp.y != c.y)
            && (pp.x != b.x || pp.y != b.y)
            && in_tri(xy(a), xy(b), xy(c), xy(pp))
            && cross(xy(ar.g(pp.prev)), xy(pp), xy(ar.g(pp.next))) <= 0.0
        {
            return false;
        }
        p = pp.next;
        guard = guard.saturating_add(1);
        if guard > ar.n.len() {
            break;
        }
    }
    true
}

/// Is b strictly inside segment a–c (a straight run, not a spike or a repeat)?
fn between(a: Node, b: Node, c: Node) -> bool {
    if (a.x == b.x && a.y == b.y) || (c.x == b.x && c.y == b.y) {
        return false;
    }
    (b.x - a.x) * (c.x - b.x) + (b.y - a.y) * (c.y - b.y) > 0.0
}

fn clip(ar: &mut Arena, start: usize, out: &mut Vec<usize>) {
    let mut e = start;
    let mut stop = start;
    let mut guard: u64 = 0;
    let mut count: u64 = 0;
    {
        let mut p = start;
        loop {
            count = count.saturating_add(1);
            p = ar.next(p);
            if p == start || count > u64::try_from(ar.n.len()).unwrap_or(u64::MAX) {
                break;
            }
        }
    }
    let limit = count.saturating_mul(count).saturating_add(10);
    while ar.prev(e) != ar.next(e) {
        guard = guard.saturating_add(1);
        if guard > limit {
            break;
        }
        let ne = ar.g(e);
        let (a, c) = (ar.g(ne.prev), ar.g(ne.next));
        // drop repeated points and zero-width spikes; a point in a straight run stays (a neighbour face shares it:
        // removing it would leave a T-junction) and becomes an ear once a neighbour is cut away
        if cross(xy(a), xy(ne), xy(c)) == 0.0 && !between(a, ne, c) {
            ar.remove(e);
            count = count.saturating_sub(1);
            if e == stop {
                stop = ne.next;
            }
            e = ne.next;
            if count < 3 {
                return;
            }
            continue;
        }
        if is_ear(ar, e) {
            out.extend_from_slice(&[a.i, ne.i, c.i]);
            ar.remove(e);
            count = count.saturating_sub(1);
            e = ar.next(ne.next);
            stop = e;
            if count < 3 {
                return;
            }
            continue;
        }
        e = ne.next;
        if e == stop {
            // no ear found in a whole turn: self-crossing or degenerate input; finish with a fan so nothing is lost
            let mut p = ar.next(e);
            let mut g2 = 0usize;
            while ar.next(p) != e {
                let (pe, pp, pn) = (ar.g(e), ar.g(p), ar.g(ar.next(p)));
                if cross(xy(pe), xy(pp), xy(pn)) != 0.0 {
                    out.extend_from_slice(&[pe.i, pp.i, pn.i]);
                }
                p = pp.next;
                g2 = g2.saturating_add(1);
                if g2 > ar.n.len() {
                    break;
                }
            }
            return;
        }
    }
}

/// Triangulate 3D rings on (about) one plane: [outer, ...holes] → indexes into their concatenation.
pub fn plane_triangulate(rings: &[Vec<V3>]) -> Vec<usize> {
    let Some(r0) = rings.first() else { return Vec::new() };
    if r0.len() < 3 {
        return Vec::new();
    }
    let n = newell(r0);
    let (ax, ay, az) = (n[0].abs(), n[1].abs(), n[2].abs());
    // drop the largest axis; keep the winding consistent with the normal
    let proj: fn(&V3) -> V2 = if az >= ax && az >= ay {
        if n[2] >= 0.0 { |p| [p[0], p[1]] } else { |p| [p[1], p[0]] }
    } else if ax >= ay {
        if n[0] >= 0.0 { |p| [p[1], p[2]] } else { |p| [p[2], p[1]] }
    } else if n[1] >= 0.0 {
        |p| [p[2], p[0]]
    } else {
        |p| [p[0], p[2]]
    };
    if rings.len() == 1 && r0.len() == 3 {
        return vec![0, 1, 2];
    }
    let r2: Vec<Vec<V2>> = rings.iter().map(|r| r.iter().map(proj).collect()).collect();
    let (o, h) = r2.split_first().map_or((&[][..], &[][..]), |(o, h)| (o.as_slice(), h));
    triangulate(o, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_with_hole() {
        let outer = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let hole = vec![[3.0, 3.0], [3.0, 7.0], [7.0, 7.0], [7.0, 3.0]];
        let t = triangulate(&outer, &[hole]);
        assert_eq!(t.len() / 3, 8);
        let pts: Vec<V2> = [outer.clone(), vec![[3.0, 3.0], [3.0, 7.0], [7.0, 7.0], [7.0, 3.0]]].concat();
        let a: f64 = t
            .chunks(3)
            .map(|k| {
                let (p, q, r) = (pts[k[0]], pts[k[1]], pts[k[2]]);
                ((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])) / 2.0
            })
            .sum();
        assert!((a - 84.0).abs() < 1e-9);
    }

    #[test]
    fn degenerate_rings() {
        assert!(triangulate(&[[0.0, 0.0], [1.0, 1.0]], &[]).is_empty());
        assert!(triangulate(&[[0.0, 0.0], [0.0, 0.0], [0.0, 0.0]], &[]).is_empty());
        let t = triangulate(&[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [1.0, 1.0]], &[]);
        assert_eq!(t.len(), 6);
    }
}
