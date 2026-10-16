//! earcut.js — polygon-with-holes triangulation for faces, ModelCraft's copy of the mapbox/earcut algorithm (holes
//! bridged into the outer ring, then ears clipped; when clipping stalls, local self-intersections are cured and the
//! polygon split along a valid diagonal; no z-order hash). Ported node for node over an arena of nodes.
//!
//! The functions below follow mapbox/earcut closely, so its licence applies to them:
//! ISC License — Copyright (c) 2016, Mapbox.
//! Permission to use, copy, modify, and/or distribute this software for any purpose with or without
//! fee is hereby granted, provided that the above copyright notice and this permission notice appear
//! in all copies. THE SOFTWARE IS PROVIDED "AS IS" AND ISC DISCLAIMS ALL WARRANTIES WITH REGARD TO
//! THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL
//! ISC BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
//! WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT,
//! NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE
//! OF THIS SOFTWARE.

use crate::js;

#[derive(Clone, Copy)]
struct Node {
    i: usize,
    x: f64,
    y: f64,
    prev: usize,
    next: usize,
    steiner: bool,
}

/// Deepest `earcutLinked` recursion followed. Spec gap: the JS recurses until V8's stack runs out (a RangeError, about
/// ten thousand levels); the port stops splitting deeper than this, keeping the triangles made so far.
const MAX_DEPTH: u32 = 1000;

struct E<'a> {
    n: Vec<Node>,
    data: &'a [f64],
    tris: Vec<usize>,
}

impl E<'_> {
    #[inline]
    fn nd(&self, p: usize) -> Node {
        self.n.get(p).copied().unwrap_or(Node { i: 0, x: f64::NAN, y: f64::NAN, prev: p, next: p, steiner: false })
    }
    #[inline]
    fn next(&self, p: usize) -> usize {
        self.nd(p).next
    }
    #[inline]
    fn prev(&self, p: usize) -> usize {
        self.nd(p).prev
    }
    #[inline]
    fn set_next(&mut self, p: usize, v: usize) {
        if let Some(x) = self.n.get_mut(p) {
            x.next = v;
        }
    }
    #[inline]
    fn set_prev(&mut self, p: usize, v: usize) {
        if let Some(x) = self.n.get_mut(p) {
            x.prev = v;
        }
    }
    #[inline]
    fn d(&self, i: usize) -> f64 {
        self.data.get(i).copied().unwrap_or(f64::NAN)
    }
    fn create(&mut self, i: usize, x: f64, y: f64) -> usize {
        let id = self.n.len();
        self.n.push(Node { i, x, y, prev: id, next: id, steiner: false });
        id
    }
    fn insert(&mut self, i: usize, x: f64, y: f64, last: Option<usize>) -> usize {
        let p = self.create(i, x, y);
        if let Some(l) = last {
            let ln = self.next(l);
            self.set_next(p, ln);
            self.set_prev(p, l);
            self.set_prev(ln, p);
            self.set_next(l, p);
        }
        p
    }
    fn remove(&mut self, p: usize) {
        let (pp, pn) = (self.prev(p), self.next(p));
        self.set_prev(pn, pp);
        self.set_next(pp, pn);
    }
    #[inline]
    fn area(&self, p: usize, q: usize, r: usize) -> f64 {
        let (p, q, r) = (self.nd(p), self.nd(q), self.nd(r));
        (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y)
    }
    #[inline]
    fn equals(&self, a: usize, b: usize) -> bool {
        let (a, b) = (self.nd(a), self.nd(b));
        a.x == b.x && a.y == b.y
    }

    fn signed_area(&self, start: usize, end: usize, dim: usize) -> f64 {
        let mut sum = 0.0;
        let mut i = start;
        let mut j = end.wrapping_sub(dim);
        while i < end {
            sum += (self.d(j) - self.d(i)) * (self.d(i + 1) + self.d(j + 1));
            j = i;
            i += dim;
        }
        sum
    }

    fn linked_list(&mut self, start: usize, end: usize, dim: usize, clockwise: bool) -> Option<usize> {
        let mut last: Option<usize> = None;
        if clockwise == (self.signed_area(start, end, dim) > 0.0) {
            let mut i = start;
            while i < end {
                last = Some(self.insert(i / dim, self.d(i), self.d(i + 1), last));
                i += dim;
            }
        } else if end >= dim {
            let mut i = end - dim;
            loop {
                if i < start {
                    break;
                }
                last = Some(self.insert(i / dim, self.d(i), self.d(i + 1), last));
                if i < dim {
                    break;
                }
                i -= dim;
            }
        }
        if let Some(l) = last
            && self.equals(l, self.next(l))
        {
            self.remove(l);
            last = Some(self.next(l));
        }
        last
    }

    fn filter_points(&mut self, start: Option<usize>, end: Option<usize>) -> Option<usize> {
        let start = start?;
        let mut end = end.unwrap_or(start);
        let mut p = start;
        loop {
            let mut again = false;
            let np = self.nd(p);
            if !np.steiner && (self.equals(p, np.next) || self.area(np.prev, p, np.next) == 0.0) {
                self.remove(p);
                p = np.prev;
                end = p;
                if p == self.next(p) {
                    break;
                }
                again = true;
            } else {
                p = np.next;
            }
            if !(again || p != end) {
                break;
            }
        }
        Some(end)
    }

    fn earcut_linked(&mut self, ear: Option<usize>, dim: usize, pass: u8, depth: u32) {
        let Some(mut ear) = ear else { return };
        if depth > MAX_DEPTH {
            return;
        }
        let mut stop = ear;
        while self.prev(ear) != self.next(ear) {
            let prev = self.prev(ear);
            let next = self.next(ear);
            if self.is_ear(ear) {
                let (pi, ei, ni) = (self.nd(prev).i, self.nd(ear).i, self.nd(next).i);
                self.tris.push(pi);
                self.tris.push(ei);
                self.tris.push(ni);
                self.remove(ear);
                ear = self.next(next);
                stop = self.next(next);
                continue;
            }
            ear = next;
            if ear == stop {
                if pass == 0 {
                    let f = self.filter_points(Some(ear), None);
                    self.earcut_linked(f, dim, 1, depth.saturating_add(1));
                } else if pass == 1 {
                    let f = self.filter_points(Some(ear), None);
                    let e = self.cure_local_intersections(f);
                    self.earcut_linked(e, dim, 2, depth.saturating_add(1));
                } else if pass == 2 {
                    self.split_earcut(ear, dim, depth);
                }
                break;
            }
        }
    }

    fn is_ear(&self, ear: usize) -> bool {
        let b = self.nd(ear);
        let a = self.nd(b.prev);
        let c = self.nd(b.next);
        if self.area(b.prev, ear, b.next) >= 0.0 {
            return false;
        }
        let (ax, bx, cx, ay, by, cy) = (a.x, b.x, c.x, a.y, b.y, c.y);
        let x0 = js::min(js::min(ax, bx), cx);
        let y0 = js::min(js::min(ay, by), cy);
        let x1 = js::max(js::max(ax, bx), cx);
        let y1 = js::max(js::max(ay, by), cy);
        let mut p = c.next;
        let mut guard = 0usize;
        while p != b.prev {
            let pn = self.nd(p);
            if pn.x >= x0
                && pn.x <= x1
                && pn.y >= y0
                && pn.y <= y1
                && point_in_triangle_except_first(ax, ay, bx, by, cx, cy, pn.x, pn.y)
                && self.area(pn.prev, p, pn.next) >= 0.0
            {
                return false;
            }
            p = pn.next;
            guard = guard.saturating_add(1);
            if guard > self.n.len() {
                break;
            }
        }
        true
    }

    fn cure_local_intersections(&mut self, start: Option<usize>) -> Option<usize> {
        let mut start = start?;
        let mut p = start;
        loop {
            let a = self.prev(p);
            let b = self.next(self.next(p));
            if !self.equals(a, b) && self.intersects(a, p, self.next(p), b) && self.locally_inside(a, b) && self.locally_inside(b, a) {
                let (ai, pi, bi) = (self.nd(a).i, self.nd(p).i, self.nd(b).i);
                self.tris.push(ai);
                self.tris.push(pi);
                self.tris.push(bi);
                self.remove(p);
                let pn = self.next(p);
                self.remove(pn);
                p = b;
                start = b;
            }
            p = self.next(p);
            if p == start {
                break;
            }
        }
        self.filter_points(Some(p), None)
    }

    fn split_earcut(&mut self, start: usize, dim: usize, depth: u32) {
        let mut a = start;
        loop {
            let mut b = self.next(self.next(a));
            while b != self.prev(a) {
                if self.nd(a).i != self.nd(b).i && self.is_valid_diagonal(a, b) {
                    let c = self.split_polygon(a, b);
                    let an = self.next(a);
                    let a2 = self.filter_points(Some(a), Some(an));
                    let cn = self.next(c);
                    let c2 = self.filter_points(Some(c), Some(cn));
                    self.earcut_linked(a2, dim, 0, depth.saturating_add(1));
                    self.earcut_linked(c2, dim, 0, depth.saturating_add(1));
                    return;
                }
                b = self.next(b);
            }
            a = self.next(a);
            if a == start {
                break;
            }
        }
    }

    fn eliminate_holes(&mut self, holes: &[usize], mut outer: usize, dim: usize) -> usize {
        let mut queue = Vec::new();
        let n = holes.len();
        for (i, &h) in holes.iter().enumerate() {
            let start = h.saturating_mul(dim);
            let end = if i + 1 < n { holes.get(i + 1).copied().unwrap_or(0).saturating_mul(dim) } else { self.data.len() };
            let Some(list) = self.linked_list(start, end, dim, false) else { continue };
            if list == self.next(list)
                && let Some(x) = self.n.get_mut(list)
            {
                x.steiner = true;
            }
            queue.push(self.get_leftmost(list));
        }
        let nodes = &self.n;
        let at = |p: usize| nodes.get(p).map_or((f64::NAN, f64::NAN), |x| (x.x, x.y));
        js::sort_by(&mut queue, |&a, &b| {
            let (ax, ay) = at(a);
            let (bx, by) = at(b);
            js::or(ax - bx, ay - by)
        });
        for h in queue {
            outer = self.eliminate_hole(h, outer);
        }
        outer
    }

    fn eliminate_hole(&mut self, hole: usize, outer: usize) -> usize {
        let Some(bridge) = self.find_hole_bridge(hole, outer) else { return outer };
        let rev = self.split_polygon(bridge, hole);
        let rn = self.next(rev);
        self.filter_points(Some(rev), Some(rn));
        let bn = self.next(bridge);
        self.filter_points(Some(bridge), Some(bn)).unwrap_or(outer)
    }

    fn find_hole_bridge(&self, hole: usize, outer: usize) -> Option<usize> {
        let mut p = outer;
        let h = self.nd(hole);
        let (hx, hy) = (h.x, h.y);
        let mut qx = f64::NEG_INFINITY;
        let mut m: Option<usize> = None;
        if self.equals(hole, p) {
            return Some(p);
        }
        loop {
            let pn = self.nd(p);
            if self.equals(hole, pn.next) {
                return Some(pn.next);
            }
            let nx = self.nd(pn.next);
            if hy <= pn.y && hy >= nx.y && nx.y != pn.y {
                let x = pn.x + (hy - pn.y) * (nx.x - pn.x) / (nx.y - pn.y);
                if x <= hx && x > qx {
                    qx = x;
                    let mm = if pn.x < nx.x { p } else { pn.next };
                    m = Some(mm);
                    if x == hx {
                        return Some(mm);
                    }
                }
            }
            p = pn.next;
            if p == outer {
                break;
            }
        }
        let mut m = m?;
        let stop = m;
        let mn = self.nd(m);
        let (mx, my) = (mn.x, mn.y);
        let mut tan_min = f64::INFINITY;
        p = m;
        loop {
            let pn = self.nd(p);
            if hx >= pn.x
                && pn.x >= mx
                && hx != pn.x
                && point_in_triangle(if hy < my { hx } else { qx }, hy, mx, my, if hy < my { qx } else { hx }, hy, pn.x, pn.y)
            {
                let tan = (hy - pn.y).abs() / (hx - pn.x);
                let mc = self.nd(m);
                if self.locally_inside(p, hole)
                    && (tan < tan_min || (tan == tan_min && (pn.x > mc.x || (pn.x == mc.x && self.sector_contains_sector(m, p)))))
                {
                    m = p;
                    tan_min = tan;
                }
            }
            p = pn.next;
            if p == stop {
                break;
            }
        }
        Some(m)
    }

    fn sector_contains_sector(&self, m: usize, p: usize) -> bool {
        self.area(self.prev(m), m, self.prev(p)) < 0.0 && self.area(self.next(p), m, self.next(m)) < 0.0
    }

    fn get_leftmost(&self, start: usize) -> usize {
        let mut p = start;
        let mut left = start;
        loop {
            let (pn, ln) = (self.nd(p), self.nd(left));
            if pn.x < ln.x || (pn.x == ln.x && pn.y < ln.y) {
                left = p;
            }
            p = pn.next;
            if p == start {
                break;
            }
        }
        left
    }

    fn is_valid_diagonal(&self, a: usize, b: usize) -> bool {
        let (an, bn) = (self.nd(a), self.nd(b));
        self.nd(an.next).i != bn.i
            && self.nd(an.prev).i != bn.i
            && !self.intersects_polygon(a, b)
            && ((self.locally_inside(a, b)
                && self.locally_inside(b, a)
                && self.middle_inside(a, b)
                && (js::truthy(self.area(an.prev, a, bn.prev)) || js::truthy(self.area(a, bn.prev, b))))
                || (self.equals(a, b) && self.area(an.prev, a, an.next) > 0.0 && self.area(bn.prev, b, bn.next) > 0.0))
    }

    fn intersects(&self, p1: usize, q1: usize, p2: usize, q2: usize) -> bool {
        let o1 = sgn(self.area(p1, q1, p2));
        let o2 = sgn(self.area(p1, q1, q2));
        let o3 = sgn(self.area(p2, q2, p1));
        let o4 = sgn(self.area(p2, q2, q1));
        if o1 != o2 && o3 != o4 {
            return true;
        }
        if o1 == 0 && self.on_segment(p1, p2, q1) {
            return true;
        }
        if o2 == 0 && self.on_segment(p1, q2, q1) {
            return true;
        }
        if o3 == 0 && self.on_segment(p2, p1, q2) {
            return true;
        }
        if o4 == 0 && self.on_segment(p2, q1, q2) {
            return true;
        }
        false
    }
    fn on_segment(&self, p: usize, q: usize, r: usize) -> bool {
        let (p, q, r) = (self.nd(p), self.nd(q), self.nd(r));
        q.x <= js::max(p.x, r.x) && q.x >= js::min(p.x, r.x) && q.y <= js::max(p.y, r.y) && q.y >= js::min(p.y, r.y)
    }
    fn intersects_polygon(&self, a: usize, b: usize) -> bool {
        let (ai, bi) = (self.nd(a).i, self.nd(b).i);
        let mut p = a;
        loop {
            let pn = self.nd(p);
            let ni = self.nd(pn.next).i;
            if pn.i != ai && ni != ai && pn.i != bi && ni != bi && self.intersects(p, pn.next, a, b) {
                return true;
            }
            p = pn.next;
            if p == a {
                break;
            }
        }
        false
    }
    fn locally_inside(&self, a: usize, b: usize) -> bool {
        let an = self.nd(a);
        if self.area(an.prev, a, an.next) < 0.0 {
            self.area(a, b, an.next) >= 0.0 && self.area(a, an.prev, b) >= 0.0
        } else {
            self.area(a, b, an.prev) < 0.0 || self.area(a, an.next, b) < 0.0
        }
    }
    fn middle_inside(&self, a: usize, b: usize) -> bool {
        let (an, bn) = (self.nd(a), self.nd(b));
        let mut p = a;
        let mut inside = false;
        let px = (an.x + bn.x) / 2.0;
        let py = (an.y + bn.y) / 2.0;
        loop {
            let pn = self.nd(p);
            let nx = self.nd(pn.next);
            if ((pn.y > py) != (nx.y > py)) && nx.y != pn.y && (px < (nx.x - pn.x) * (py - pn.y) / (nx.y - pn.y) + pn.x) {
                inside = !inside;
            }
            p = pn.next;
            if p == a {
                break;
            }
        }
        inside
    }
    fn split_polygon(&mut self, a: usize, b: usize) -> usize {
        let (an_, bn_) = (self.nd(a), self.nd(b));
        let a2 = self.create(an_.i, an_.x, an_.y);
        let b2 = self.create(bn_.i, bn_.x, bn_.y);
        let an = an_.next;
        let bp = bn_.prev;
        self.set_next(a, b);
        self.set_prev(b, a);
        self.set_next(a2, an);
        self.set_prev(an, a2);
        self.set_next(b2, a2);
        self.set_prev(a2, b2);
        self.set_next(bp, b2);
        self.set_prev(b2, bp);
        b2
    }
}

#[inline]
fn sgn(n: f64) -> i32 {
    if n > 0.0 {
        1
    } else if n < 0.0 {
        -1
    } else {
        0
    }
}

#[allow(clippy::too_many_arguments)]
fn point_in_triangle(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64, px: f64, py: f64) -> bool {
    (cx - px) * (ay - py) >= (ax - px) * (cy - py)
        && (ax - px) * (by - py) >= (bx - px) * (ay - py)
        && (bx - px) * (cy - py) >= (cx - px) * (by - py)
}
#[allow(clippy::too_many_arguments)]
fn point_in_triangle_except_first(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64, px: f64, py: f64) -> bool {
    !(ax == px && ay == py) && point_in_triangle(ax, ay, bx, by, cx, cy, px, py)
}

/// `earcut(flat, holeIndices)` → triangle corner indices (dim 2).
pub fn earcut(data: &[f64], holes: &[usize]) -> Vec<usize> {
    let dim = 2usize;
    let mut e = E { n: Vec::with_capacity(data.len() / dim + 8), data, tris: Vec::new() };
    let outer_len = match holes.first() {
        Some(&h) => h.saturating_mul(dim),
        None => data.len(),
    };
    let Some(mut outer) = e.linked_list(0, outer_len.min(data.len()), dim, true) else { return e.tris };
    if e.next(outer) == e.prev(outer) {
        return e.tris;
    }
    if !holes.is_empty() {
        outer = e.eliminate_holes(holes, outer, dim);
    }
    e.earcut_linked(Some(outer), dim, 0, 0);
    e.tris
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn square_with_hole() {
        let sq = [0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0, 3.0, 3.0, 3.0, 6.0, 6.0, 6.0, 6.0, 3.0];
        assert_eq!(earcut(&sq, &[4]).len(), 8 * 3);
        assert_eq!(earcut(&sq[..8], &[]).len(), 6);
    }
}
