// Copyright 2026 CadCraft. Licensed under the Apache License, Version 2.0.
// The algorithm is libc++'s `std::sort` (LLVM 21, include/__algorithm/sort.h, partial_sort.h, sift_down.h,
// push_heap.h, pop_heap.h), Apache-2.0 WITH LLVM-exception, restated in Rust.
//
//! libc++ `std::sort` for a comparator that is not `std::less` on an arithmetic type (introsort: insertion sort below
//! 24 elements, median of 3 / Tukey's ninther pivots, partition with equals on the right, heap sort past
//! 2·log2(n) levels). Clipper2's `ProcessIntersectList` sorts with `std::sort`; nodes at the same point compare equal
//! and their order after the sort decides the output, so the wasm build's exact algorithm is reproduced here.
//!
//! Indices are checked (an inconsistent comparator ends the sort with an error instead of reading out of bounds).
#![allow(clippy::arithmetic_side_effects)]

use crate::util::{Fault, R};

struct S<'a, T: Copy, F: Fn(&T, &T) -> bool> {
    v: &'a mut [T],
    less: F,
}

impl<T: Copy, F: Fn(&T, &T) -> bool> S<'_, T, F> {
    #[inline]
    fn g(&self, i: usize) -> R<T> {
        self.v.get(i).copied().ok_or(Fault::Index)
    }
    #[inline]
    fn s(&mut self, i: usize, t: T) -> R<()> {
        *self.v.get_mut(i).ok_or(Fault::Index)? = t;
        Ok(())
    }
    #[inline]
    fn swap(&mut self, i: usize, j: usize) -> R<()> {
        if i >= self.v.len() || j >= self.v.len() {
            return Err(Fault::Index);
        }
        self.v.swap(i, j);
        Ok(())
    }
    /// A sub-range must be ordered and inside the slice (an inconsistent comparator could break that).
    #[inline]
    fn range(&self, first: usize, last: usize) -> R<()> {
        if first > last || last > self.v.len() { Err(Fault::Index) } else { Ok(()) }
    }

    /// `comp(v[i], v[j])`.
    #[inline]
    fn c(&self, i: usize, j: usize) -> R<bool> {
        Ok((self.less)(&self.g(i)?, &self.g(j)?))
    }
    #[inline]
    fn ct(&self, t: &T, j: usize) -> R<bool> {
        Ok((self.less)(t, &self.g(j)?))
    }
    #[inline]
    fn tc(&self, i: usize, t: &T) -> R<bool> {
        Ok((self.less)(&self.g(i)?, t))
    }

    fn sort3(&mut self, x: usize, y: usize, z: usize) -> R<bool> {
        if !self.c(y, x)? {
            if !self.c(z, y)? {
                return Ok(false);
            }
            self.swap(y, z)?;
            if self.c(y, x)? {
                self.swap(x, y)?;
            }
            return Ok(true);
        }
        if self.c(z, y)? {
            self.swap(x, z)?;
            return Ok(true);
        }
        self.swap(x, y)?;
        if self.c(z, y)? {
            self.swap(y, z)?;
        }
        Ok(true)
    }

    fn sort4(&mut self, x1: usize, x2: usize, x3: usize, x4: usize) -> R<()> {
        self.sort3(x1, x2, x3)?;
        if self.c(x4, x3)? {
            self.swap(x3, x4)?;
            if self.c(x3, x2)? {
                self.swap(x2, x3)?;
                if self.c(x2, x1)? {
                    self.swap(x1, x2)?;
                }
            }
        }
        Ok(())
    }

    fn sort5(&mut self, x1: usize, x2: usize, x3: usize, x4: usize, x5: usize) -> R<()> {
        self.sort4(x1, x2, x3, x4)?;
        if self.c(x5, x4)? {
            self.swap(x4, x5)?;
            if self.c(x4, x3)? {
                self.swap(x3, x4)?;
                if self.c(x3, x2)? {
                    self.swap(x2, x3)?;
                    if self.c(x2, x1)? {
                        self.swap(x1, x2)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn insertion_sort(&mut self, first: usize, last: usize) -> R<()> {
        self.range(first, last)?;
        if first == last {
            return Ok(());
        }
        let mut i = first + 1;
        while i != last {
            let mut j = i - 1;
            if self.c(i, j)? {
                let t = self.g(i)?;
                let mut k = j;
                j = i;
                loop {
                    let vk = self.g(k)?;
                    self.s(j, vk)?;
                    j = k;
                    if j == first {
                        break;
                    }
                    k -= 1;
                    if !self.ct(&t, k)? {
                        break;
                    }
                }
                self.s(j, t)?;
            }
            i += 1;
        }
        Ok(())
    }

    fn insertion_sort_unguarded(&mut self, first: usize, last: usize) -> R<()> {
        self.range(first, last)?;
        if first == last {
            return Ok(());
        }
        let mut i = first + 1;
        while i != last {
            let mut j = i - 1;
            if self.c(i, j)? {
                let t = self.g(i)?;
                let mut k = j;
                j = i;
                loop {
                    let vk = self.g(k)?;
                    self.s(j, vk)?;
                    j = k;
                    k = k.checked_sub(1).ok_or(Fault::Index)?;
                    if !self.ct(&t, k)? {
                        break;
                    }
                }
                self.s(j, t)?;
            }
            i += 1;
        }
        Ok(())
    }

    fn insertion_sort_incomplete(&mut self, first: usize, last: usize) -> R<bool> {
        self.range(first, last)?;
        match last - first {
            0 | 1 => return Ok(true),
            2 => {
                if self.c(last - 1, first)? {
                    self.swap(first, last - 1)?;
                }
                return Ok(true);
            }
            3 => {
                self.sort3(first, first + 1, last - 1)?;
                return Ok(true);
            }
            4 => {
                self.sort4(first, first + 1, first + 2, last - 1)?;
                return Ok(true);
            }
            5 => {
                self.sort5(first, first + 1, first + 2, first + 3, last - 1)?;
                return Ok(true);
            }
            _ => {}
        }
        let mut j = first + 2;
        self.sort3(first, first + 1, j)?;
        const LIMIT: u32 = 8;
        let mut count = 0u32;
        let mut i = j + 1;
        while i != last {
            if self.c(i, j)? {
                let t = self.g(i)?;
                let mut k = j;
                j = i;
                loop {
                    let vk = self.g(k)?;
                    self.s(j, vk)?;
                    j = k;
                    if j == first {
                        break;
                    }
                    k -= 1;
                    if !self.ct(&t, k)? {
                        break;
                    }
                }
                self.s(j, t)?;
                count += 1;
                if count == LIMIT {
                    return Ok(i + 1 == last);
                }
            }
            j = i;
            i += 1;
        }
        Ok(true)
    }

    fn partition_with_equals_on_right(&mut self, first0: usize, last0: usize) -> R<(usize, bool)> {
        self.range(first0, last0)?;
        let begin = first0;
        let mut first = first0;
        let mut last = last0;
        let pivot = self.g(first)?;
        loop {
            first += 1;
            if !self.tc(first, &pivot)? {
                break;
            }
        }
        if begin == first - 1 {
            while first < last {
                last -= 1;
                if self.tc(last, &pivot)? {
                    break;
                }
            }
        } else {
            loop {
                last = last.checked_sub(1).ok_or(Fault::Index)?;
                if self.tc(last, &pivot)? {
                    break;
                }
            }
        }
        let already = first >= last;
        while first < last {
            self.swap(first, last)?;
            loop {
                first += 1;
                if !self.tc(first, &pivot)? {
                    break;
                }
            }
            loop {
                last = last.checked_sub(1).ok_or(Fault::Index)?;
                if self.tc(last, &pivot)? {
                    break;
                }
            }
        }
        let pivot_pos = first - 1;
        if begin != pivot_pos {
            let v = self.g(pivot_pos)?;
            self.s(begin, v)?;
        }
        self.s(pivot_pos, pivot)?;
        Ok((pivot_pos, already))
    }

    fn partition_with_equals_on_left(&mut self, first0: usize, last0: usize) -> R<usize> {
        self.range(first0, last0)?;
        let begin = first0;
        let mut first = first0;
        let mut last = last0;
        let pivot = self.g(first)?;
        if self.ct(&pivot, last - 1)? {
            loop {
                first += 1;
                if self.ct(&pivot, first)? {
                    break;
                }
            }
        } else {
            loop {
                first += 1;
                if !(first < last && !self.ct(&pivot, first)?) {
                    break;
                }
            }
        }
        if first < last {
            loop {
                last = last.checked_sub(1).ok_or(Fault::Index)?;
                if !self.ct(&pivot, last)? {
                    break;
                }
            }
        }
        while first < last {
            self.swap(first, last)?;
            loop {
                first += 1;
                if self.ct(&pivot, first)? {
                    break;
                }
            }
            loop {
                last = last.checked_sub(1).ok_or(Fault::Index)?;
                if !self.ct(&pivot, last)? {
                    break;
                }
            }
        }
        let pivot_pos = first - 1;
        if begin != pivot_pos {
            let v = self.g(pivot_pos)?;
            self.s(begin, v)?;
        }
        self.s(pivot_pos, pivot)?;
        Ok(first)
    }

    fn sift_down(&mut self, first: usize, len: usize, start0: usize) -> R<()> {
        let mut start = start0;
        let mut child = start - first;
        if len < 2 || (len - 2) / 2 < child {
            return Ok(());
        }
        child = 2 * child + 1;
        let mut child_i = first + child;
        if child + 1 < len && self.c(child_i, child_i + 1)? {
            child_i += 1;
            child += 1;
        }
        if self.c(child_i, start)? {
            return Ok(());
        }
        let top = self.g(start)?;
        loop {
            let v = self.g(child_i)?;
            self.s(start, v)?;
            start = child_i;
            if (len - 2) / 2 < child {
                break;
            }
            child = 2 * child + 1;
            child_i = first + child;
            if child + 1 < len && self.c(child_i, child_i + 1)? {
                child_i += 1;
                child += 1;
            }
            if self.tc(child_i, &top)? {
                break;
            }
        }
        self.s(start, top)
    }

    fn floyd_sift_down(&mut self, first: usize, len: usize) -> R<usize> {
        let mut hole = first;
        let mut child_i = first;
        let mut child = 0usize;
        loop {
            child_i += child + 1;
            child = 2 * child + 1;
            if child + 1 < len && self.c(child_i, child_i + 1)? {
                child_i += 1;
                child += 1;
            }
            let v = self.g(child_i)?;
            self.s(hole, v)?;
            hole = child_i;
            if child > (len - 2) / 2 {
                return Ok(hole);
            }
        }
    }

    fn sift_up(&mut self, first: usize, last0: usize, len0: usize) -> R<()> {
        let mut last = last0;
        let mut len = len0;
        if len > 1 {
            len = (len - 2) / 2;
            let mut ptr = first + len;
            last -= 1;
            if self.c(ptr, last)? {
                let t = self.g(last)?;
                loop {
                    let v = self.g(ptr)?;
                    self.s(last, v)?;
                    last = ptr;
                    if len == 0 {
                        break;
                    }
                    len = (len - 1) / 2;
                    ptr = first + len;
                    if !self.tc(ptr, &t)? {
                        break;
                    }
                }
                self.s(last, t)?;
            }
        }
        Ok(())
    }

    fn pop_heap(&mut self, first: usize, last0: usize, len: usize) -> R<()> {
        if len > 1 {
            let top = self.g(first)?;
            let mut hole = self.floyd_sift_down(first, len)?;
            let last = last0 - 1;
            if hole == last {
                self.s(hole, top)?;
            } else {
                let v = self.g(last)?;
                self.s(hole, v)?;
                hole += 1;
                self.s(last, top)?;
                self.sift_up(first, hole, hole - first)?;
            }
        }
        Ok(())
    }

    /// `__partial_sort(first, last, last)`: heap sort.
    fn heap_sort(&mut self, first: usize, last: usize) -> R<()> {
        self.range(first, last)?;
        let n = last - first;
        if n > 1 {
            let mut start = (n - 2) / 2;
            loop {
                self.sift_down(first, n, first + start)?;
                if start == 0 {
                    break;
                }
                start -= 1;
            }
        }
        let mut l = last;
        let mut len = n;
        while len > 1 {
            self.pop_heap(first, l, len)?;
            l -= 1;
            len -= 1;
        }
        Ok(())
    }

    fn introsort(&mut self, first0: usize, last0: usize, depth0: usize, leftmost0: bool) -> R<()> {
        const LIMIT: usize = 24;
        const NINTHER: usize = 128;
        let (mut first, mut last, mut depth, mut leftmost) = (first0, last0, depth0, leftmost0);
        loop {
            self.range(first, last)?;
            let len = last - first;
            match len {
                0 | 1 => return Ok(()),
                2 => {
                    if self.c(last - 1, first)? {
                        self.swap(first, last - 1)?;
                    }
                    return Ok(());
                }
                3 => {
                    self.sort3(first, first + 1, last - 1)?;
                    return Ok(());
                }
                4 => return self.sort4(first, first + 1, first + 2, last - 1),
                5 => return self.sort5(first, first + 1, first + 2, first + 3, last - 1),
                _ => {}
            }
            if len < LIMIT {
                return if leftmost { self.insertion_sort(first, last) } else { self.insertion_sort_unguarded(first, last) };
            }
            if depth == 0 {
                return self.heap_sort(first, last);
            }
            depth -= 1;
            let half = len / 2;
            if len > NINTHER {
                self.sort3(first, first + half, last - 1)?;
                self.sort3(first + 1, first + (half - 1), last - 2)?;
                self.sort3(first + 2, first + (half + 1), last - 3)?;
                self.sort3(first + (half - 1), first + half, first + (half + 1))?;
                self.swap(first, first + half)?;
            } else {
                self.sort3(first + half, first, last - 1)?;
            }
            if !leftmost {
                let before = first.checked_sub(1).ok_or(Fault::Index)?;
                if !self.c(before, first)? {
                    first = self.partition_with_equals_on_left(first, last)?;
                    continue;
                }
            }
            let (mut i, partitioned) = self.partition_with_equals_on_right(first, last)?;
            if partitioned {
                let fs = self.insertion_sort_incomplete(first, i)?;
                if self.insertion_sort_incomplete(i + 1, last)? {
                    if fs {
                        return Ok(());
                    }
                    last = i;
                    continue;
                } else if fs {
                    i += 1;
                    first = i;
                    continue;
                }
            }
            self.introsort(first, i, depth, leftmost)?;
            leftmost = false;
            i += 1;
            first = i;
        }
    }
}

/// libc++ `std::sort(v.begin(), v.end(), less)`.
pub(crate) fn libcxx_sort<T: Copy>(v: &mut [T], less: impl Fn(&T, &T) -> bool) -> R<()> {
    let n = v.len();
    if n == 0 {
        return Ok(());
    }
    // 2 · __bit_log2(n)
    let depth = 2 * (usize::BITS - 1 - n.leading_zeros()) as usize;
    let mut s = S { v, less };
    s.introsort(0, n, depth, true)
}

#[cfg(test)]
mod tests {
    use super::libcxx_sort;

    fn lcg(seed: &mut u64) -> u64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *seed >> 33
    }

    #[test]
    fn sorts_like_a_sort() {
        let mut seed = 7u64;
        for n in [0usize, 1, 2, 3, 5, 7, 23, 24, 25, 100, 129, 1000, 5000] {
            let mut v: Vec<(u64, usize)> = (0..n).map(|i| (lcg(&mut seed) % 17, i)).collect();
            libcxx_sort(&mut v, |a, b| a.0 < b.0).unwrap();
            assert!(v.windows(2).all(|w| w[0].0 <= w[1].0), "n {n}");
            let mut idx: Vec<usize> = v.iter().map(|x| x.1).collect();
            idx.sort_unstable();
            assert_eq!(idx, (0..n).collect::<Vec<_>>());
        }
    }

    #[test]
    fn known_libcxx_order_of_equal_keys() {
        // three equal keys: __sort3 leaves them in place; a pivot pass over 30 equal keys keeps a fixed permutation
        let mut v: Vec<(u8, usize)> = (0..3).map(|i| (0, i)).collect();
        libcxx_sort(&mut v, |a, b| a.0 < b.0).unwrap();
        assert_eq!(v.iter().map(|x| x.1).collect::<Vec<_>>(), [0, 1, 2]);
    }

    #[test]
    fn inconsistent_comparator_never_panics() {
        let mut seed = 3u64;
        let mut v: Vec<u64> = (0..300).map(|_| lcg(&mut seed)).collect();
        let _ = libcxx_sort(&mut v, |_, _| true);
        let _ = libcxx_sort(&mut v, |a, b| (a ^ b) & 1 == 0);
    }
}
