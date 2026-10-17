//! Delaunator 5.1.0 (github.com/mapbox/delaunator), ported one for one so the triangulation — triangle order,
//! half-edges, hull — is the JS library's exactly (tin.js builds its constrained TIN on top of it).
//!
//! ISC License, Copyright (c) 2026, Mapbox. Permission to use, copy, modify, and/or distribute this software for any
//! purpose with or without fee is hereby granted, provided that the above copyright notice and this permission notice
//! appear in all copies. THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS
//! SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
//! ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR
//! PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH
//! THE USE OR PERFORMANCE OF THIS SOFTWARE.

// Index arithmetic here works on positions inside arrays that are already allocated (bounded by the point count).
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use crate::predicates::orient2d;

const EPSILON: f64 = f64::EPSILON; // Math.pow(2, -52)
const EDGE_STACK_LEN: usize = 512;

/// The triangulation: `triangles` (3 vertex ids per triangle, counter-clockwise in the library's y-down sense),
/// `halfedges` (twin of each half-edge or −1), `hull`.
#[derive(Debug, Default)]
pub struct Delaunator {
    pub triangles: Vec<u32>,
    pub halfedges: Vec<i32>,
    pub hull: Vec<u32>,
}

struct State<'a> {
    coords: &'a [f64],
    triangles: Vec<u32>,
    halfedges: Vec<i32>,
    hash_size: usize,
    hull_prev: Vec<u32>,
    hull_next: Vec<u32>,
    hull_tri: Vec<u32>,
    hull_hash: Vec<i32>,
    triangles_len: usize,
    cx: f64,
    cy: f64,
    hull_start: u32,
    edge_stack: [u32; EDGE_STACK_LEN],
}

#[inline]
fn at(a: &[f64], i: usize) -> f64 {
    a.get(i).copied().unwrap_or(f64::NAN)
}
#[inline]
fn u(a: &[u32], i: usize) -> u32 {
    a.get(i).copied().unwrap_or(0)
}
#[inline]
fn set_u(a: &mut [u32], i: usize, v: u32) {
    if let Some(s) = a.get_mut(i) {
        *s = v;
    }
}

impl Delaunator {
    /// `new Delaunator(coords)` with coords = [x0, y0, x1, y1, …].
    pub fn new(coords: &[f64]) -> Delaunator {
        let n = coords.len() >> 1;
        let max_triangles = (2 * n).saturating_sub(5);
        let hash_size = (n as f64).sqrt().ceil() as usize;
        let mut st = State {
            coords,
            triangles: vec![0; max_triangles * 3],
            halfedges: vec![0; max_triangles * 3],
            hash_size,
            hull_prev: vec![0; n],
            hull_next: vec![0; n],
            hull_tri: vec![0; n],
            hull_hash: vec![0; hash_size],
            triangles_len: 0,
            cx: 0.0,
            cy: 0.0,
            hull_start: 0,
            edge_stack: [0; EDGE_STACK_LEN],
        };
        let mut ids: Vec<u32> = vec![0; n];
        let mut dists: Vec<f64> = vec![0.0; n];

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for i in 0..n {
            let x = at(coords, 2 * i);
            let y = at(coords, 2 * i + 1);
            if x < min_x {
                min_x = x;
            }
            if y < min_y {
                min_y = y;
            }
            if x > max_x {
                max_x = x;
            }
            if y > max_y {
                max_y = y;
            }
            if let Some(s) = ids.get_mut(i) {
                *s = i as u32;
            }
        }
        let cx = (min_x + max_x) / 2.0;
        let cy = (min_y + max_y) / 2.0;

        let (mut i0, mut i1, mut i2) = (0usize, 0usize, 0usize);
        let mut min_dist = f64::INFINITY;
        for i in 0..n {
            let d = dist(cx, cy, at(coords, 2 * i), at(coords, 2 * i + 1));
            if d < min_dist {
                i0 = i;
                min_dist = d;
            }
        }
        let i0x = at(coords, 2 * i0);
        let i0y = at(coords, 2 * i0 + 1);

        min_dist = f64::INFINITY;
        for i in 0..n {
            if i == i0 {
                continue;
            }
            let d = dist(i0x, i0y, at(coords, 2 * i), at(coords, 2 * i + 1));
            if d < min_dist && d > 0.0 {
                i1 = i;
                min_dist = d;
            }
        }
        let mut i1x = at(coords, 2 * i1);
        let mut i1y = at(coords, 2 * i1 + 1);

        let mut min_radius = f64::INFINITY;
        for i in 0..n {
            if i == i0 || i == i1 {
                continue;
            }
            let r = circumradius(i0x, i0y, i1x, i1y, at(coords, 2 * i), at(coords, 2 * i + 1));
            if r < min_radius {
                i2 = i;
                min_radius = r;
            }
        }
        let mut i2x = at(coords, 2 * i2);
        let mut i2y = at(coords, 2 * i2 + 1);

        if min_radius == f64::INFINITY {
            // collinear points: ordered by dx (or dy), returned as a hull
            for i in 0..n {
                let dx = at(coords, 2 * i) - at(coords, 0);
                let v = if dx != 0.0 && !dx.is_nan() { dx } else { at(coords, 2 * i + 1) - at(coords, 1) };
                if let Some(s) = dists.get_mut(i) {
                    *s = v;
                }
            }
            if n > 0 {
                quicksort(&mut ids, &dists, 0, n as isize - 1);
            }
            let mut hull = Vec::new();
            let mut d0 = f64::NEG_INFINITY;
            for i in 0..n {
                let id = u(&ids, i);
                let d = at(&dists, id as usize);
                if d > d0 {
                    hull.push(id);
                    d0 = d;
                }
            }
            return Delaunator { triangles: Vec::new(), halfedges: Vec::new(), hull };
        }

        if orient2d(i0x, i0y, i1x, i1y, i2x, i2y) < 0.0 {
            std::mem::swap(&mut i1, &mut i2);
            std::mem::swap(&mut i1x, &mut i2x);
            std::mem::swap(&mut i1y, &mut i2y);
        }

        let (ccx, ccy) = circumcenter(i0x, i0y, i1x, i1y, i2x, i2y);
        st.cx = ccx;
        st.cy = ccy;

        for i in 0..n {
            if let Some(s) = dists.get_mut(i) {
                *s = dist(at(coords, 2 * i), at(coords, 2 * i + 1), ccx, ccy);
            }
        }
        if n > 0 {
            quicksort(&mut ids, &dists, 0, n as isize - 1);
        }

        st.hull_start = i0 as u32;
        let mut hull_size = 3usize;

        set_u(&mut st.hull_next, i0, i1 as u32);
        set_u(&mut st.hull_prev, i2, i1 as u32);
        set_u(&mut st.hull_next, i1, i2 as u32);
        set_u(&mut st.hull_prev, i0, i2 as u32);
        set_u(&mut st.hull_next, i2, i0 as u32);
        set_u(&mut st.hull_prev, i1, i0 as u32);

        set_u(&mut st.hull_tri, i0, 0);
        set_u(&mut st.hull_tri, i1, 1);
        set_u(&mut st.hull_tri, i2, 2);

        st.hull_hash.fill(-1);
        st.set_hash(i0x, i0y, i0 as i32);
        st.set_hash(i1x, i1y, i1 as i32);
        st.set_hash(i2x, i2y, i2 as i32);

        st.triangles_len = 0;
        st.add_triangle(i0 as u32, i1 as u32, i2 as u32, -1, -1, -1);

        let mut xp = 0.0f64;
        let mut yp = 0.0f64;
        for k in 0..ids.len() {
            let i = u(&ids, k) as usize;
            let x = at(coords, 2 * i);
            let y = at(coords, 2 * i + 1);

            // skip near-duplicate points
            if k > 0 && (x - xp).abs() <= EPSILON && (y - yp).abs() <= EPSILON {
                continue;
            }
            xp = x;
            yp = y;

            // skip seed triangle points
            if i == i0 || i == i1 || i == i2 {
                continue;
            }

            // find a visible edge on the convex hull using edge hash
            // Spec gap: the JS reads a typed array at NaN / −1 when the point sits exactly on the seed circle's
            // centre or no hash slot is live (it then works on `undefined`); here such a point is skipped.
            let Some(key) = st.hash_key(x, y) else { continue };
            let mut start: i32 = 0;
            for j in 0..st.hash_size {
                start = st.hull_hash.get((key + j) % st.hash_size).copied().unwrap_or(-1);
                if start != -1 && start as u32 != u(&st.hull_next, start as usize) {
                    break;
                }
            }
            if start < 0 {
                continue;
            }

            let start = u(&st.hull_prev, start as usize);
            let mut e: i64 = i64::from(start);
            loop {
                let q = u(&st.hull_next, e as usize);
                let o = orient2d(
                    x,
                    y,
                    at(coords, 2 * e as usize),
                    at(coords, 2 * e as usize + 1),
                    at(coords, 2 * q as usize),
                    at(coords, 2 * q as usize + 1),
                );
                if !(o >= 0.0) {
                    break;
                }
                e = i64::from(q);
                if e == i64::from(start) {
                    e = -1;
                    break;
                }
            }
            if e == -1 {
                continue; // likely a near-duplicate point; skip it
            }
            let mut e = e as u32;

            // add the first triangle from the point
            let mut t = st.add_triangle(e, i as u32, u(&st.hull_next, e as usize), -1, -1, u(&st.hull_tri, e as usize) as i32);

            // recursively flip triangles from the point until they satisfy the Delaunay condition
            let l = st.legalize(t + 2);
            set_u(&mut st.hull_tri, i, l);
            set_u(&mut st.hull_tri, e as usize, t);
            hull_size += 1;

            // walk forward through the hull, adding more triangles and flipping recursively
            let mut nn = u(&st.hull_next, e as usize);
            loop {
                let q = u(&st.hull_next, nn as usize);
                let o = orient2d(
                    x,
                    y,
                    at(coords, 2 * nn as usize),
                    at(coords, 2 * nn as usize + 1),
                    at(coords, 2 * q as usize),
                    at(coords, 2 * q as usize + 1),
                );
                if !(o < 0.0) {
                    break;
                }
                t = st.add_triangle(nn, i as u32, q, u(&st.hull_tri, i) as i32, -1, u(&st.hull_tri, nn as usize) as i32);
                let l = st.legalize(t + 2);
                set_u(&mut st.hull_tri, i, l);
                set_u(&mut st.hull_next, nn as usize, nn); // mark as removed
                hull_size -= 1;
                nn = q;
            }

            // walk backward from the other side, adding more triangles and flipping
            if e == start {
                loop {
                    let q = u(&st.hull_prev, e as usize);
                    let o = orient2d(
                        x,
                        y,
                        at(coords, 2 * q as usize),
                        at(coords, 2 * q as usize + 1),
                        at(coords, 2 * e as usize),
                        at(coords, 2 * e as usize + 1),
                    );
                    if !(o < 0.0) {
                        break;
                    }
                    t = st.add_triangle(q, i as u32, e, -1, u(&st.hull_tri, e as usize) as i32, u(&st.hull_tri, q as usize) as i32);
                    st.legalize(t + 2);
                    set_u(&mut st.hull_tri, q as usize, t);
                    set_u(&mut st.hull_next, e as usize, e); // mark as removed
                    hull_size -= 1;
                    e = q;
                }
            }

            // update the hull indices
            st.hull_start = e;
            set_u(&mut st.hull_prev, i, e);
            set_u(&mut st.hull_next, e as usize, i as u32);
            set_u(&mut st.hull_prev, nn as usize, i as u32);
            set_u(&mut st.hull_next, i, nn);

            // save the two new edges in the hash table
            st.set_hash(x, y, i as i32);
            st.set_hash(at(coords, 2 * e as usize), at(coords, 2 * e as usize + 1), e as i32);
        }

        let mut hull = Vec::with_capacity(hull_size);
        let mut e = st.hull_start;
        for _ in 0..hull_size {
            hull.push(e);
            e = u(&st.hull_next, e as usize);
        }
        st.triangles.truncate(st.triangles_len);
        st.halfedges.truncate(st.triangles_len);
        Delaunator { triangles: st.triangles, halfedges: st.halfedges, hull }
    }
}

impl State<'_> {
    fn hash_key(&self, x: f64, y: f64) -> Option<usize> {
        let k = (pseudo_angle(x - self.cx, y - self.cy) * self.hash_size as f64).floor();
        if !k.is_finite() || k < 0.0 || self.hash_size == 0 {
            return None;
        }
        Some((k as usize) % self.hash_size)
    }
    fn set_hash(&mut self, x: f64, y: f64, v: i32) {
        // a NaN key writes to a property of the typed array in JS: nothing in the table changes
        if let Some(k) = self.hash_key(x, y)
            && let Some(s) = self.hull_hash.get_mut(k)
        {
            *s = v;
        }
    }

    fn legalize(&mut self, a0: u32) -> u32 {
        let mut a = a0 as usize;
        let mut i = 0usize;
        let mut ar;
        loop {
            let b = self.halfedges.get(a).copied().unwrap_or(-1);
            let a0 = a - a % 3;
            ar = a0 + (a + 2) % 3;

            if b == -1 {
                if i == 0 {
                    break;
                }
                i -= 1;
                a = self.edge_stack.get(i).copied().unwrap_or(0) as usize;
                continue;
            }
            let b = b as usize;
            let b0 = b - b % 3;
            let al = a0 + (a + 1) % 3;
            let bl = b0 + (b + 2) % 3;

            let p0 = u(&self.triangles, ar) as usize;
            let pr = u(&self.triangles, a) as usize;
            let pl = u(&self.triangles, al) as usize;
            let p1 = u(&self.triangles, bl) as usize;
            let c = self.coords;

            let illegal = in_circle(
                at(c, 2 * p0),
                at(c, 2 * p0 + 1),
                at(c, 2 * pr),
                at(c, 2 * pr + 1),
                at(c, 2 * pl),
                at(c, 2 * pl + 1),
                at(c, 2 * p1),
                at(c, 2 * p1 + 1),
            );

            if illegal {
                set_u(&mut self.triangles, a, p1 as u32);
                set_u(&mut self.triangles, b, p0 as u32);

                let hbl = self.halfedges.get(bl).copied().unwrap_or(-1);

                // edge swapped on the other side of the hull (rare); fix the half-edge reference
                if hbl == -1 {
                    let mut e = self.hull_start;
                    loop {
                        if u(&self.hull_tri, e as usize) == bl as u32 {
                            set_u(&mut self.hull_tri, e as usize, a as u32);
                            break;
                        }
                        e = u(&self.hull_prev, e as usize);
                        if e == self.hull_start {
                            break;
                        }
                    }
                }
                self.link(a, hbl);
                let har = self.halfedges.get(ar).copied().unwrap_or(-1);
                self.link(b, har);
                self.link(ar, bl as i32);

                let br = b0 + (b + 1) % 3;

                // don't worry about hitting the cap: it can only happen on extremely degenerate input
                if i < EDGE_STACK_LEN {
                    if let Some(s) = self.edge_stack.get_mut(i) {
                        *s = br as u32;
                    }
                    i += 1;
                }
            } else {
                if i == 0 {
                    break;
                }
                i -= 1;
                a = self.edge_stack.get(i).copied().unwrap_or(0) as usize;
            }
        }
        ar as u32
    }

    fn link(&mut self, a: usize, b: i32) {
        if let Some(s) = self.halfedges.get_mut(a) {
            *s = b;
        }
        if b != -1
            && let Some(s) = self.halfedges.get_mut(b as usize)
        {
            *s = a as i32;
        }
    }

    fn add_triangle(&mut self, i0: u32, i1: u32, i2: u32, a: i32, b: i32, c: i32) -> u32 {
        let t = self.triangles_len;
        set_u(&mut self.triangles, t, i0);
        set_u(&mut self.triangles, t + 1, i1);
        set_u(&mut self.triangles, t + 2, i2);
        self.link(t, a);
        self.link(t + 1, b);
        self.link(t + 2, c);
        self.triangles_len += 3;
        t as u32
    }
}

fn pseudo_angle(dx: f64, dy: f64) -> f64 {
    let p = dx / (dx.abs() + dy.abs());
    (if dy > 0.0 { 3.0 - p } else { 1.0 + p }) / 4.0
}

fn dist(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

#[allow(clippy::too_many_arguments)]
fn in_circle(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64, px: f64, py: f64) -> bool {
    let dx = ax - px;
    let dy = ay - py;
    let ex = bx - px;
    let ey = by - py;
    let fx = cx - px;
    let fy = cy - py;
    let ap = dx * dx + dy * dy;
    let bp = ex * ex + ey * ey;
    let cp = fx * fx + fy * fy;
    dx * (ey * cp - bp * fy) - dy * (ex * cp - bp * fx) + ap * (ex * fy - ey * fx) < 0.0
}

fn circumradius(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    let ex = cx - ax;
    let ey = cy - ay;
    let bl = dx * dx + dy * dy;
    let cl = ex * ex + ey * ey;
    let d = 0.5 / (dx * ey - dy * ex);
    let x = (ey * bl - dy * cl) * d;
    let y = (dx * cl - ex * bl) * d;
    x * x + y * y
}

fn circumcenter(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64) -> (f64, f64) {
    let dx = bx - ax;
    let dy = by - ay;
    let ex = cx - ax;
    let ey = cy - ay;
    let bl = dx * dx + dy * dy;
    let cl = ex * ex + ey * ey;
    let d = 0.5 / (dx * ey - dy * ex);
    (ax + (ey * bl - dy * cl) * d, ay + (dx * cl - ex * bl) * d)
}

fn dist_of(ids: &[u32], dists: &[f64], i: isize) -> f64 {
    usize::try_from(i).ok().and_then(|i| ids.get(i)).and_then(|&id| dists.get(id as usize)).copied().unwrap_or(f64::NAN)
}
fn swap(arr: &mut [u32], i: isize, j: isize) {
    if let (Ok(i), Ok(j)) = (usize::try_from(i), usize::try_from(j))
        && i < arr.len()
        && j < arr.len()
    {
        arr.swap(i, j);
    }
}
fn get_id(arr: &[u32], i: isize) -> u32 {
    usize::try_from(i).ok().and_then(|i| arr.get(i)).copied().unwrap_or(0)
}
fn set_id(arr: &mut [u32], i: isize, v: u32) {
    if let Some(s) = usize::try_from(i).ok().and_then(|i| arr.get_mut(i)) {
        *s = v;
    }
}

/// Sort ids by distance (the library's own quicksort: its order on ties is part of the output).
fn quicksort(ids: &mut [u32], dists: &[f64], left: isize, right: isize) {
    // explicit stack instead of recursion (the JS recurses; the order of work is the same)
    let mut stack: Vec<(isize, isize)> = vec![(left, right)];
    while let Some((left, right)) = stack.pop() {
        if right - left <= 20 {
            let mut i = left + 1;
            while i <= right {
                let temp = get_id(ids, i);
                let temp_dist = dists.get(temp as usize).copied().unwrap_or(f64::NAN);
                let mut j = i - 1;
                while j >= left && dist_of(ids, dists, j) > temp_dist {
                    let v = get_id(ids, j);
                    set_id(ids, j + 1, v);
                    j -= 1;
                }
                set_id(ids, j + 1, temp);
                i += 1;
            }
        } else {
            let median = (left + right) >> 1;
            let mut i = left + 1;
            let mut j = right;
            swap(ids, median, i);
            if dist_of(ids, dists, left) > dist_of(ids, dists, right) {
                swap(ids, left, right);
            }
            if dist_of(ids, dists, i) > dist_of(ids, dists, right) {
                swap(ids, i, right);
            }
            if dist_of(ids, dists, left) > dist_of(ids, dists, i) {
                swap(ids, left, i);
            }
            let temp = get_id(ids, i);
            let temp_dist = dists.get(temp as usize).copied().unwrap_or(f64::NAN);
            loop {
                loop {
                    i += 1;
                    if !(dist_of(ids, dists, i) < temp_dist) {
                        break;
                    }
                }
                loop {
                    j -= 1;
                    if !(dist_of(ids, dists, j) > temp_dist) {
                        break;
                    }
                }
                if j < i {
                    break;
                }
                swap(ids, i, j);
            }
            let v = get_id(ids, j);
            set_id(ids, left + 1, v);
            set_id(ids, j, temp);

            // the JS sorts the larger part first, then the smaller: pushed in reverse so they pop in that order
            if right - i + 1 >= j - left {
                stack.push((left, j - 1));
                stack.push((i, right));
            } else {
                stack.push((i, right));
                stack.push((left, j - 1));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn square() {
        let d = Delaunator::new(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]);
        assert_eq!(d.triangles.len(), 6);
        assert_eq!(d.hull.len(), 4);
    }
    #[test]
    fn collinear() {
        let d = Delaunator::new(&[0.0, 0.0, 1.0, 0.0, 2.0, 0.0]);
        assert!(d.triangles.is_empty());
        assert_eq!(d.hull, vec![0, 1, 2]);
    }
}
