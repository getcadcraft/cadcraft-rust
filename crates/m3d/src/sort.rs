//! `Array.prototype.sort(cmp)` exactly as V8 does it: TimSort (V8's `third_party/v8/builtins/array-sort.tq`, itself
//! CPython's listsort): runs found and extended by binary insertion sort to the minimum run length, merged by the
//! run-stack rules with galloping (minGallop 7, adapted). The comparator's result is a number; NaN counts as 0, as
//! the spec says. With a consistent comparator any stable sort gives the same order; the kernel's comparators are
//! not always consistent (coordinates can be NaN: a zero-length edge is split at a NaN point, as the JS does), and
//! then only this algorithm gives the JS's order.

#![deny(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

const MIN_GALLOP_WINS: i64 = 7;

/// `order` of `cmp(x, y)` (NaN → 0).
#[inline]
fn ord<T>(cmp: &mut impl FnMut(&T, &T) -> f64, x: &T, y: &T) -> f64 {
    let v = cmp(x, y);
    if v.is_nan() { 0.0 } else { v }
}

#[inline]
fn ix(i: i64) -> usize {
    usize::try_from(i).unwrap_or(usize::MAX)
}

struct S<'a, T, F> {
    w: &'a mut [T],
    cmp: F,
    min_gallop: i64,
    runs: Vec<(i64, i64)>,
}

impl<T: Clone, F: FnMut(&T, &T) -> f64> S<'_, T, F> {
    #[inline]
    fn get(&self, i: i64) -> Option<T> {
        self.w.get(ix(i)).cloned()
    }
    #[inline]
    fn set(&mut self, i: i64, v: T) {
        if let Some(x) = self.w.get_mut(ix(i)) {
            *x = v;
        }
    }
    fn compare(&mut self, x: &T, y: &T) -> f64 {
        ord(&mut self.cmp, x, y)
    }

    fn binary_insertion_sort(&mut self, low: i64, start_arg: i64, high: i64) {
        let mut start = if low == start_arg { start_arg.saturating_add(1) } else { start_arg };
        while start < high {
            let mut left = low;
            let mut right = start;
            let Some(pivot) = self.get(start) else { return };
            while left < right {
                let mid = left.saturating_add(right.saturating_sub(left) >> 1);
                let Some(m) = self.get(mid) else { return };
                let order = self.compare(&pivot, &m);
                if order < 0.0 {
                    right = mid;
                } else {
                    left = mid.saturating_add(1);
                }
            }
            let mut p = start;
            while p > left {
                let Some(v) = self.get(p.saturating_sub(1)) else { return };
                self.set(p, v);
                p = p.saturating_sub(1);
            }
            self.set(left, pivot);
            start = start.saturating_add(1);
        }
    }

    fn count_and_make_run(&mut self, low_arg: i64, high: i64) -> i64 {
        let low = low_arg.saturating_add(1);
        if low == high {
            return 1;
        }
        let mut run = 2i64;
        let (Some(el), Some(elp)) = (self.get(low), self.get(low_arg)) else { return 1 };
        let mut order = self.compare(&el, &elp);
        let desc = order < 0.0;
        let mut prev = el;
        let mut idx = low.saturating_add(1);
        while idx < high {
            let Some(cur) = self.get(idx) else { break };
            order = self.compare(&cur, &prev);
            if desc {
                if order >= 0.0 {
                    break;
                }
            } else if order < 0.0 {
                break;
            }
            prev = cur;
            run = run.saturating_add(1);
            idx = idx.saturating_add(1);
        }
        if desc {
            let (a, b) = (ix(low_arg), ix(low_arg.saturating_add(run)));
            if let Some(r) = self.w.get_mut(a..b) {
                r.reverse();
            }
        }
        run
    }

    fn merge_collapse(&mut self) {
        while self.runs.len() > 1 {
            let mut n = i64::try_from(self.runs.len()).unwrap_or(0).saturating_sub(2);
            if !self.invariant(n.saturating_add(1)) || !self.invariant(n) {
                if self.len_of(n.saturating_sub(1)) < self.len_of(n.saturating_add(1)) {
                    n = n.saturating_sub(1);
                }
                self.merge_at(n);
            } else if self.len_of(n) <= self.len_of(n.saturating_add(1)) {
                self.merge_at(n);
            } else {
                break;
            }
        }
    }
    fn merge_force_collapse(&mut self) {
        while self.runs.len() > 1 {
            let mut n = i64::try_from(self.runs.len()).unwrap_or(0).saturating_sub(2);
            if n > 0 && self.len_of(n.saturating_sub(1)) < self.len_of(n.saturating_add(1)) {
                n = n.saturating_sub(1);
            }
            self.merge_at(n);
        }
    }
    #[inline]
    fn len_of(&self, i: i64) -> i64 {
        self.runs.get(ix(i)).map_or(0, |r| r.1)
    }
    fn invariant(&self, n: i64) -> bool {
        if n < 2 {
            return true;
        }
        let (a, b, c) = (self.len_of(n), self.len_of(n.saturating_sub(1)), self.len_of(n.saturating_sub(2)));
        c > b.saturating_add(a)
    }

    fn merge_at(&mut self, i: i64) {
        let size = self.runs.len();
        let (Some(&(mut base_a, mut len_a)), Some(&(base_b, mut len_b))) = (self.runs.get(ix(i)), self.runs.get(ix(i.saturating_add(1))))
        else {
            self.runs.clear();
            return;
        };
        if let Some(r) = self.runs.get_mut(ix(i)) {
            r.1 = len_a.saturating_add(len_b);
        }
        if ix(i) == size.saturating_sub(3)
            && let Some(&next) = self.runs.get(ix(i.saturating_add(2)))
            && let Some(r) = self.runs.get_mut(ix(i.saturating_add(1)))
        {
            *r = next;
        }
        self.runs.truncate(size.saturating_sub(1));
        // where does b start in a? elements in a before that are already in place
        let Some(key_right) = self.get(base_b) else { return };
        let k = gallop_right(&mut self.cmp, self.w, &key_right, base_a, len_a, 0);
        base_a = base_a.saturating_add(k);
        len_a = len_a.saturating_sub(k);
        if len_a == 0 {
            return;
        }
        // where does a end in b? elements in b after that are already in place
        let Some(key_left) = self.get(base_a.saturating_add(len_a).saturating_sub(1)) else { return };
        len_b = gallop_left(&mut self.cmp, self.w, &key_left, base_b, len_b, len_b.saturating_sub(1));
        if len_b == 0 {
            return;
        }
        if len_a <= len_b {
            self.merge_low(base_a, len_a, base_b, len_b);
        } else {
            self.merge_high(base_a, len_a, base_b, len_b);
        }
    }

    fn merge_low(&mut self, base_a: i64, len_a0: i64, base_b: i64, len_b0: i64) {
        let mut len_a = len_a0;
        let mut len_b = len_b0;
        let temp: Vec<T> = self.w.get(ix(base_a)..ix(base_a.saturating_add(len_a))).map(<[T]>::to_vec).unwrap_or_default();
        let tg = |i: i64| temp.get(ix(i)).cloned();
        let mut dest = base_a;
        let mut ct = 0i64;
        let mut cb = base_b;
        if let Some(v) = self.get(cb) {
            self.set(dest, v);
        }
        dest = dest.saturating_add(1);
        cb = cb.saturating_add(1);
        // 0: Succeed, 1: CopyB
        let how: u8 = 'outer: {
            len_b = len_b.saturating_sub(1);
            if len_b == 0 {
                break 'outer 0;
            }
            if len_a == 1 {
                break 'outer 1;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let mut wins_a = 0i64;
                let mut wins_b = 0i64;
                loop {
                    let (Some(x), Some(y)) = (self.get(cb), tg(ct)) else { break 'outer 0 };
                    let order = self.compare(&x, &y);
                    if order < 0.0 {
                        self.set(dest, x);
                        dest = dest.saturating_add(1);
                        cb = cb.saturating_add(1);
                        wins_b = wins_b.saturating_add(1);
                        len_b = len_b.saturating_sub(1);
                        wins_a = 0;
                        if len_b == 0 {
                            break 'outer 0;
                        }
                        if wins_b >= min_gallop {
                            break;
                        }
                    } else {
                        self.set(dest, y);
                        dest = dest.saturating_add(1);
                        ct = ct.saturating_add(1);
                        wins_a = wins_a.saturating_add(1);
                        len_a = len_a.saturating_sub(1);
                        wins_b = 0;
                        if len_a == 1 {
                            break 'outer 1;
                        }
                        if wins_a >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop = min_gallop.saturating_add(1);
                let mut first = true;
                while wins_a >= MIN_GALLOP_WINS || wins_b >= MIN_GALLOP_WINS || first {
                    first = false;
                    min_gallop = (min_gallop.saturating_sub(1)).max(1);
                    self.min_gallop = min_gallop;
                    let Some(kb_) = self.get(cb) else { break 'outer 0 };
                    wins_a = gallop_right(&mut self.cmp, &temp, &kb_, ct, len_a, 0);
                    if wins_a > 0 {
                        for q in 0..wins_a {
                            if let Some(v) = tg(ct.saturating_add(q)) {
                                self.set(dest.saturating_add(q), v);
                            }
                        }
                        dest = dest.saturating_add(wins_a);
                        ct = ct.saturating_add(wins_a);
                        len_a = len_a.saturating_sub(wins_a);
                        if len_a == 1 {
                            break 'outer 1;
                        }
                        // impossible with a consistent comparator, but it may not be
                        if len_a == 0 {
                            break 'outer 0;
                        }
                    }
                    if let Some(v) = self.get(cb) {
                        self.set(dest, v);
                    }
                    dest = dest.saturating_add(1);
                    cb = cb.saturating_add(1);
                    len_b = len_b.saturating_sub(1);
                    if len_b == 0 {
                        break 'outer 0;
                    }
                    let Some(kt) = tg(ct) else { break 'outer 0 };
                    wins_b = gallop_left(&mut self.cmp, self.w, &kt, cb, len_b, 0);
                    if wins_b > 0 {
                        for q in 0..wins_b {
                            if let Some(v) = self.get(cb.saturating_add(q)) {
                                self.set(dest.saturating_add(q), v);
                            }
                        }
                        dest = dest.saturating_add(wins_b);
                        cb = cb.saturating_add(wins_b);
                        len_b = len_b.saturating_sub(wins_b);
                        if len_b == 0 {
                            break 'outer 0;
                        }
                    }
                    if let Some(v) = tg(ct) {
                        self.set(dest, v);
                    }
                    dest = dest.saturating_add(1);
                    ct = ct.saturating_add(1);
                    len_a = len_a.saturating_sub(1);
                    if len_a == 1 {
                        break 'outer 1;
                    }
                }
                min_gallop = min_gallop.saturating_add(1);
                self.min_gallop = min_gallop;
            }
        };
        if how == 0 {
            for q in 0..len_a.max(0) {
                if let Some(v) = tg(ct.saturating_add(q)) {
                    self.set(dest.saturating_add(q), v);
                }
            }
        } else {
            // the last element of run A belongs at the end of the merge
            for q in 0..len_b.max(0) {
                if let Some(v) = self.get(cb.saturating_add(q)) {
                    self.set(dest.saturating_add(q), v);
                }
            }
            if let Some(v) = tg(ct) {
                self.set(dest.saturating_add(len_b), v);
            }
        }
    }

    fn merge_high(&mut self, base_a: i64, len_a0: i64, base_b: i64, len_b0: i64) {
        let mut len_a = len_a0;
        let mut len_b = len_b0;
        let temp: Vec<T> = self.w.get(ix(base_b)..ix(base_b.saturating_add(len_b))).map(<[T]>::to_vec).unwrap_or_default();
        let tg = |i: i64| temp.get(ix(i)).cloned();
        let mut dest = base_b.saturating_add(len_b).saturating_sub(1);
        let mut ct = len_b.saturating_sub(1);
        let mut ca = base_a.saturating_add(len_a).saturating_sub(1);
        if let Some(v) = self.get(ca) {
            self.set(dest, v);
        }
        dest = dest.saturating_sub(1);
        ca = ca.saturating_sub(1);
        // 0: Succeed, 1: CopyA
        let how: u8 = 'outer: {
            len_a = len_a.saturating_sub(1);
            if len_a == 0 {
                break 'outer 0;
            }
            if len_b == 1 {
                break 'outer 1;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let mut wins_a = 0i64;
                let mut wins_b = 0i64;
                loop {
                    let (Some(x), Some(y)) = (tg(ct), self.get(ca)) else { break 'outer 0 };
                    let order = self.compare(&x, &y);
                    if order < 0.0 {
                        self.set(dest, y);
                        dest = dest.saturating_sub(1);
                        ca = ca.saturating_sub(1);
                        wins_a = wins_a.saturating_add(1);
                        len_a = len_a.saturating_sub(1);
                        wins_b = 0;
                        if len_a == 0 {
                            break 'outer 0;
                        }
                        if wins_a >= min_gallop {
                            break;
                        }
                    } else {
                        self.set(dest, x);
                        dest = dest.saturating_sub(1);
                        ct = ct.saturating_sub(1);
                        wins_b = wins_b.saturating_add(1);
                        len_b = len_b.saturating_sub(1);
                        wins_a = 0;
                        if len_b == 1 {
                            break 'outer 1;
                        }
                        if wins_b >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop = min_gallop.saturating_add(1);
                let mut first = true;
                while wins_a >= MIN_GALLOP_WINS || wins_b >= MIN_GALLOP_WINS || first {
                    first = false;
                    min_gallop = (min_gallop.saturating_sub(1)).max(1);
                    self.min_gallop = min_gallop;
                    let Some(kt) = tg(ct) else { break 'outer 0 };
                    let k = gallop_right(&mut self.cmp, self.w, &kt, base_a, len_a, len_a.saturating_sub(1));
                    wins_a = len_a.saturating_sub(k);
                    if wins_a > 0 {
                        dest = dest.saturating_sub(wins_a);
                        ca = ca.saturating_sub(wins_a);
                        // copy backwards: the ranges may overlap
                        let mut q = wins_a;
                        while q > 0 {
                            q = q.saturating_sub(1);
                            if let Some(v) = self.get(ca.saturating_add(1).saturating_add(q)) {
                                self.set(dest.saturating_add(1).saturating_add(q), v);
                            }
                        }
                        len_a = len_a.saturating_sub(wins_a);
                        if len_a == 0 {
                            break 'outer 0;
                        }
                    }
                    if let Some(v) = tg(ct) {
                        self.set(dest, v);
                    }
                    dest = dest.saturating_sub(1);
                    ct = ct.saturating_sub(1);
                    len_b = len_b.saturating_sub(1);
                    if len_b == 1 {
                        break 'outer 1;
                    }
                    let Some(ka) = self.get(ca) else { break 'outer 0 };
                    let k = gallop_left(&mut self.cmp, &temp, &ka, 0, len_b, len_b.saturating_sub(1));
                    wins_b = len_b.saturating_sub(k);
                    if wins_b > 0 {
                        dest = dest.saturating_sub(wins_b);
                        ct = ct.saturating_sub(wins_b);
                        for q in 0..wins_b {
                            if let Some(v) = tg(ct.saturating_add(1).saturating_add(q)) {
                                self.set(dest.saturating_add(1).saturating_add(q), v);
                            }
                        }
                        len_b = len_b.saturating_sub(wins_b);
                        if len_b == 1 {
                            break 'outer 1;
                        }
                        // impossible with a consistent comparator, but it may not be
                        if len_b == 0 {
                            break 'outer 0;
                        }
                    }
                    if let Some(v) = self.get(ca) {
                        self.set(dest, v);
                    }
                    dest = dest.saturating_sub(1);
                    ca = ca.saturating_sub(1);
                    len_a = len_a.saturating_sub(1);
                    if len_a == 0 {
                        break 'outer 0;
                    }
                }
                min_gallop = min_gallop.saturating_add(1);
                self.min_gallop = min_gallop;
            }
        };
        if how == 0 {
            if len_b > 0 {
                let at = dest.saturating_sub(len_b.saturating_sub(1));
                for q in 0..len_b {
                    if let Some(v) = tg(q) {
                        self.set(at.saturating_add(q), v);
                    }
                }
            }
        } else {
            // the first element of run B belongs at the front of the merge
            dest = dest.saturating_sub(len_a);
            ca = ca.saturating_sub(len_a);
            let mut q = len_a;
            while q > 0 {
                q = q.saturating_sub(1);
                if let Some(v) = self.get(ca.saturating_add(1).saturating_add(q)) {
                    self.set(dest.saturating_add(1).saturating_add(q), v);
                }
            }
            if let Some(v) = tg(ct) {
                self.set(dest, v);
            }
        }
    }
}

/// Where key goes in the sorted arr[base..base+length], leftmost (a[k-1] < key <= a[k]), starting from `hint`.
fn gallop_left<T, F: FnMut(&T, &T) -> f64>(cmp: &mut F, arr: &[T], key: &T, base: i64, length: i64, hint: i64) -> i64 {
    let at = |i: i64| arr.get(ix(base.saturating_add(i)));
    let mut last = 0i64;
    let mut off = 1i64;
    let Some(h) = at(hint) else { return 0 };
    let mut order = ord(cmp, h, key);
    if order < 0.0 {
        let max = length.saturating_sub(hint);
        while off < max {
            let Some(x) = at(hint.saturating_add(off)) else { break };
            order = ord(cmp, x, key);
            if order >= 0.0 {
                break;
            }
            last = off;
            off = (off << 1).saturating_add(1);
            if off <= 0 {
                off = max;
            }
        }
        if off > max {
            off = max;
        }
        last = last.saturating_add(hint);
        off = off.saturating_add(hint);
    } else {
        let max = hint.saturating_add(1);
        while off < max {
            let Some(x) = at(hint.saturating_sub(off)) else { break };
            order = ord(cmp, x, key);
            if order < 0.0 {
                break;
            }
            last = off;
            off = (off << 1).saturating_add(1);
            if off <= 0 {
                off = max;
            }
        }
        if off > max {
            off = max;
        }
        let tmp = last;
        last = hint.saturating_sub(off);
        off = hint.saturating_sub(tmp);
    }
    last = last.saturating_add(1);
    while last < off {
        let m = last.saturating_add(off.saturating_sub(last) >> 1);
        let Some(x) = at(m) else { break };
        order = ord(cmp, x, key);
        if order < 0.0 {
            last = m.saturating_add(1);
        } else {
            off = m;
        }
    }
    off
}

/// Where key goes in the sorted arr[base..base+length], rightmost (a[k-1] <= key < a[k]), starting from `hint`.
fn gallop_right<T, F: FnMut(&T, &T) -> f64>(cmp: &mut F, arr: &[T], key: &T, base: i64, length: i64, hint: i64) -> i64 {
    let at = |i: i64| arr.get(ix(base.saturating_add(i)));
    let mut last = 0i64;
    let mut off = 1i64;
    let Some(h) = at(hint) else { return 0 };
    let mut order = ord(cmp, key, h);
    if order < 0.0 {
        let max = hint.saturating_add(1);
        while off < max {
            let Some(x) = at(hint.saturating_sub(off)) else { break };
            order = ord(cmp, key, x);
            if order >= 0.0 {
                break;
            }
            last = off;
            off = (off << 1).saturating_add(1);
            if off <= 0 {
                off = max;
            }
        }
        if off > max {
            off = max;
        }
        let tmp = last;
        last = hint.saturating_sub(off);
        off = hint.saturating_sub(tmp);
    } else {
        let max = length.saturating_sub(hint);
        while off < max {
            let Some(x) = at(hint.saturating_add(off)) else { break };
            order = ord(cmp, key, x);
            if order < 0.0 {
                break;
            }
            last = off;
            off = (off << 1).saturating_add(1);
            if off <= 0 {
                off = max;
            }
        }
        if off > max {
            off = max;
        }
        last = last.saturating_add(hint);
        off = off.saturating_add(hint);
    }
    last = last.saturating_add(1);
    while last < off {
        let m = last.saturating_add(off.saturating_sub(last) >> 1);
        let Some(x) = at(m) else { break };
        order = ord(cmp, key, x);
        if order < 0.0 {
            off = m;
        } else {
            last = m.saturating_add(1);
        }
    }
    off
}

fn min_run(n: i64) -> i64 {
    let mut n = n;
    let mut r = 0i64;
    while n >= 64 {
        r |= n & 1;
        n >>= 1;
    }
    n.saturating_add(r)
}

/// `list.sort(cmp)` as V8 does it.
pub fn timsort<T: Clone>(list: &mut [T], cmp: impl FnMut(&T, &T) -> f64) {
    let Ok(length) = i64::try_from(list.len()) else { return };
    if length < 2 {
        return;
    }
    let mut s = S { w: list, cmp, min_gallop: MIN_GALLOP_WINS, runs: Vec::new() };
    let mut remaining = length;
    let mut low = 0i64;
    let min = min_run(remaining);
    while remaining != 0 {
        let mut run = s.count_and_make_run(low, low.saturating_add(remaining));
        if run < min {
            let forced = min.min(remaining);
            s.binary_insertion_sort(low, low.saturating_add(run), low.saturating_add(forced));
            run = forced;
        }
        s.runs.push((low, run));
        s.merge_collapse();
        low = low.saturating_add(run);
        remaining = remaining.saturating_sub(run);
    }
    s.merge_force_collapse();
}

#[cfg(test)]
mod tests {
    use super::*;
    fn lcg(seed: &mut u64) -> u64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *seed >> 33
    }
    #[test]
    fn sorts_and_is_stable() {
        let mut seed = 7u64;
        for n in [0usize, 1, 2, 5, 31, 63, 64, 65, 100, 257, 1000, 5000] {
            let mut v: Vec<(u64, usize)> = (0..n).map(|i| (lcg(&mut seed) % 50, i)).collect();
            let mut w = v.clone();
            timsort(&mut v, |a, b| a.0 as f64 - b.0 as f64);
            w.sort_by_key(|x| x.0);
            assert_eq!(v, w, "n = {n}");
        }
        // runs that gallop: long ascending pieces
        let mut v: Vec<f64> = (0..2000).map(|i| f64::from((i * 7919) % 2000)).collect();
        v.extend((0..500).map(f64::from));
        let mut w = v.clone();
        timsort(&mut v, |a, b| a - b);
        w.sort_by(f64::total_cmp);
        assert_eq!(v, w);
    }
}
