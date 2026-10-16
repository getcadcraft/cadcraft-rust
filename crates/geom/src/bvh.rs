//! A bounding-box tree over items that each have a box (ModelCraft's bvh.js, ported one for one: same splits, same
//! visiting order, so picking and snapping find the same item first): for finding the few of a million that matter.
//! Built once; items are found by box, by ray or by any test.

/// Items per leaf.
const LEAF: usize = 6;

/// The tree. `boxes` holds 6 numbers per item: min x y z, max x y z.
#[derive(Clone, Debug)]
pub struct BoxTree {
    pub n: usize,
    boxes: Vec<f64>,
    idx: Vec<usize>,
    /// node boxes
    nb: Vec<[f64; 6]>,
    /// inner: (left, right); leaf: (-1 - start, count)
    kid: Vec<(i64, i64)>,
    /// the box of everything (None when empty)
    pub min: Option<[f64; 3]>,
    pub max: Option<[f64; 3]>,
}

fn at(v: &[f64], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(f64::NAN)
}

fn i64_of(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

impl BoxTree {
    pub fn new(boxes: Vec<f64>) -> BoxTree {
        let n = boxes.len() / 6;
        let idx: Vec<usize> = (0..n).collect();
        let mut cen = vec![0.0; n.saturating_mul(3)];
        for i in 0..n {
            for k in 0..3 {
                if let Some(c) = cen.get_mut(i * 3 + k) {
                    *c = (at(&boxes, i * 6 + k) + at(&boxes, i * 6 + 3 + k)) / 2.0;
                }
            }
        }
        let mut t = BoxTree { n, boxes, idx, nb: Vec::with_capacity((2 * n).div_ceil(LEAF) + 2), kid: Vec::new(), min: None, max: None };
        if n > 0 {
            t.build(0, n, &cen);
            if let Some(b) = t.nb.first() {
                t.min = Some([b[0], b[1], b[2]]);
                t.max = Some([b[3], b[4], b[5]]);
            }
        } else {
            t.node();
        }
        t
    }

    fn node(&mut self) -> usize {
        self.nb.push([0.0; 6]);
        self.kid.push((0, 0));
        self.nb.len() - 1
    }

    fn build(&mut self, lo: usize, hi: usize, cen: &[f64]) -> usize {
        let id = self.node();
        let (mut x0, mut y0, mut z0) = (f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let (mut x1, mut y1, mut z1) = (f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for k in lo..hi {
            let o = self.idx.get(k).copied().unwrap_or(0) * 6;
            let b = &self.boxes;
            if at(b, o) < x0 {
                x0 = at(b, o);
            }
            if at(b, o + 1) < y0 {
                y0 = at(b, o + 1);
            }
            if at(b, o + 2) < z0 {
                z0 = at(b, o + 2);
            }
            if at(b, o + 3) > x1 {
                x1 = at(b, o + 3);
            }
            if at(b, o + 4) > y1 {
                y1 = at(b, o + 4);
            }
            if at(b, o + 5) > z1 {
                z1 = at(b, o + 5);
            }
        }
        if let Some(nb) = self.nb.get_mut(id) {
            *nb = [x0, y0, z0, x1, y1, z1];
        }
        if hi - lo <= LEAF {
            if let Some(k) = self.kid.get_mut(id) {
                *k = (-1 - i64_of(lo), i64_of(hi - lo));
            }
            return id;
        }
        // split at the median of the centres along the longest side of the centres' box
        let mut c0 = [f64::INFINITY; 3];
        let mut c1 = [f64::NEG_INFINITY; 3];
        for k in lo..hi {
            let i = self.idx.get(k).copied().unwrap_or(0);
            for ((l, h), a) in c0.iter_mut().zip(c1.iter_mut()).zip(0..3) {
                let v = at(cen, i * 3 + a);
                if v < *l {
                    *l = v;
                }
                if v > *h {
                    *h = v;
                }
            }
        }
        let (dx, dy, dz) = (c1[0] - c0[0], c1[1] - c0[1], c1[2] - c0[2]);
        let ax = if dx >= dy && dx >= dz {
            0
        } else if dy >= dz {
            1
        } else {
            2
        };
        let mid = (lo + hi) >> 1;
        select(&mut self.idx, cen, ax, lo, hi - 1, mid);
        let l = self.build(lo, mid, cen);
        let r = self.build(mid, hi, cen);
        if let Some(k) = self.kid.get_mut(id) {
            *k = (i64_of(l), i64_of(r));
        }
        id
    }

    /// `cb(i)` for every item whose box passes `test(lo, hi)` (nodes are tested the same way); `cb` returning true
    /// stops the walk (and `walk` returns true).
    pub fn walk(&self, mut test: impl FnMut(&[f64; 3], &[f64; 3]) -> bool, mut cb: impl FnMut(usize) -> bool) -> bool {
        if self.n == 0 {
            return false;
        }
        let mut stack: Vec<usize> = vec![0];
        while let Some(id) = stack.pop() {
            let Some(b) = self.nb.get(id) else { continue };
            if !test(&[b[0], b[1], b[2]], &[b[3], b[4], b[5]]) {
                continue;
            }
            let Some(&(a, c)) = self.kid.get(id) else { continue };
            if a < 0 {
                let s = usize::try_from(-1 - a).unwrap_or(0);
                let cnt = usize::try_from(c).unwrap_or(0);
                for k in s..s.saturating_add(cnt) {
                    let Some(&i) = self.idx.get(k) else { continue };
                    let q = i * 6;
                    let bx = &self.boxes;
                    let lo = [at(bx, q), at(bx, q + 1), at(bx, q + 2)];
                    let hi = [at(bx, q + 3), at(bx, q + 4), at(bx, q + 5)];
                    if test(&lo, &hi) && cb(i) {
                        return true;
                    }
                }
            } else {
                stack.push(usize::try_from(c).unwrap_or(0));
                stack.push(usize::try_from(a).unwrap_or(0));
            }
        }
        false
    }

    /// `cb(i)` for every item whose box touches [min, max].
    pub fn find_box(&self, min: [f64; 3], max: [f64; 3], cb: impl FnMut(usize) -> bool) -> bool {
        self.walk(
            |lo, hi| lo[0] <= max[0] && hi[0] >= min[0] && lo[1] <= max[1] && hi[1] >= min[1] && lo[2] <= max[2] && hi[2] >= min[2],
            cb,
        )
    }

    /// `cb(i)` for every item whose box the ray o + t·d (0 ≤ t ≤ tmax) passes through; `tmax` is read as the walk goes
    /// (a nearest-hit search shortens it through the cell).
    pub fn ray(&self, o: [f64; 3], d: [f64; 3], tmax: &std::cell::Cell<f64>, cb: impl FnMut(usize) -> bool) -> bool {
        let inv = [1.0 / d[0], 1.0 / d[1], 1.0 / d[2]];
        self.walk(
            |lo, hi| {
                let mut t0 = 0.0;
                let mut t1 = tmax.get();
                for (((&l, &h), &oa), &ia) in lo.iter().zip(hi).zip(&o).zip(&inv) {
                    let mut n = (l - oa) * ia;
                    let mut f = (h - oa) * ia;
                    if n.is_nan() || f.is_nan() {
                        // along a side
                        if oa < l || oa > h {
                            return false;
                        }
                        continue;
                    }
                    if n > f {
                        std::mem::swap(&mut n, &mut f);
                    }
                    if n > t0 {
                        t0 = n;
                    }
                    if f < t1 {
                        t1 = f;
                    }
                    if t0 > t1 {
                        return false;
                    }
                }
                true
            },
            cb,
        )
    }
}

/// Put the k-th smallest (by cen[i*3+ax]) at idx[k], smaller before it and larger after (lo..=hi).
fn select(idx: &mut [usize], cen: &[f64], ax: usize, mut lo: usize, mut hi: usize, k: usize) {
    let key = |idx: &[usize], i: usize| idx.get(i).map_or(f64::NAN, |&j| at(cen, j * 3 + ax));
    while hi > lo {
        let pv = key(idx, (lo + hi) >> 1);
        let (mut i, mut j) = (lo, hi);
        // i, j as signed: j may step below lo (to lo − 1) as in the JS
        let mut jj = i64_of(j);
        while i64_of(i) <= jj {
            while key(idx, i) < pv {
                i += 1;
            }
            while jj >= 0 && key(idx, usize::try_from(jj).unwrap_or(0)) > pv {
                jj -= 1;
            }
            if i64_of(i) <= jj {
                j = usize::try_from(jj).unwrap_or(0);
                idx.swap(i, j);
                i += 1;
                jj -= 1;
            }
        }
        if i64_of(k) <= jj {
            hi = usize::try_from(jj).unwrap_or(0);
        } else if k >= i {
            lo = i;
        } else {
            return;
        }
    }
}

/// Boxes (6 per item) of lists of points, grown by `pad`.
pub fn boxes_of<'a>(lists: impl IntoIterator<Item = &'a [[f64; 3]]>, pad: f64) -> Vec<f64> {
    let mut b = Vec::new();
    for pts in lists {
        let (mut x0, mut y0, mut z0) = (f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let (mut x1, mut y1, mut z1) = (f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in pts {
            if p[0] < x0 {
                x0 = p[0];
            }
            if p[1] < y0 {
                y0 = p[1];
            }
            if p[2] < z0 {
                z0 = p[2];
            }
            if p[0] > x1 {
                x1 = p[0];
            }
            if p[1] > y1 {
                y1 = p[1];
            }
            if p[2] > z1 {
                z1 = p[2];
            }
        }
        b.extend_from_slice(&[x0 - pad, y0 - pad, z0 - pad, x1 + pad, y1 + pad, z1 + pad]);
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(n: usize) -> Vec<f64> {
        let mut b = Vec::new();
        for i in 0..n {
            let (x, y) = ((i % 100) as f64, (i / 100) as f64);
            b.extend_from_slice(&[x, y, 0.0, x + 0.5, y + 0.5, 1.0]);
        }
        b
    }

    #[test]
    fn finds_what_brute_force_finds() {
        let b = grid(10_000);
        let t = BoxTree::new(b.clone());
        let (min, max) = ([10.2, 20.2, 0.5], [13.1, 22.7, 0.6]);
        let mut found = Vec::new();
        t.find_box(min, max, |i| {
            found.push(i);
            false
        });
        found.sort_unstable();
        let brute: Vec<usize> = (0..10_000)
            .filter(|&i| b[i * 6] <= max[0] && b[i * 6 + 3] >= min[0] && b[i * 6 + 1] <= max[1] && b[i * 6 + 4] >= min[1])
            .collect();
        assert_eq!(found, brute);
        assert_eq!(t.min, Some([0.0, 0.0, 0.0]));
        assert_eq!(t.max, Some([99.5, 99.5, 1.0]));
    }

    #[test]
    fn ray_hits_the_boxes_it_crosses_and_stops() {
        let t = BoxTree::new(grid(10_000));
        let tmax = std::cell::Cell::new(f64::INFINITY);
        let mut hits = Vec::new();
        // straight down onto (5.25, 7.25)
        t.ray([5.25, 7.25, 10.0], [0.0, 0.0, -1.0], &tmax, |i| {
            hits.push(i);
            false
        });
        assert_eq!(hits, vec![705]);
        assert!(!t.find_box([1e9; 3], [2e9; 3], |_| true));
        assert!(t.ray([5.25, 7.25, 10.0], [0.0, 0.0, -1.0], &tmax, |_| true), "a callback returning true stops the walk");
        assert!(!BoxTree::new(Vec::new()).walk(|_, _| true, |_| true), "an empty tree finds nothing");
    }
}
