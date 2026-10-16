// Copyright 2026 CadCraft. Licensed under the Apache License, Version 2.0.
//! Internal error type and fallible indexing.
//!
//! The C++ indexes its arrays without checks; on valid input every index is in range, on hostile input the C++ reads
//! out of bounds (undefined behaviour, in the wasm build: arbitrary memory). Every array access here goes through
//! [`Get`] / [`GetMut`] and turns an out-of-range index into [`Fault::Index`], which ends the operation with
//! `Status::Internal`. Loops the C++ assumes terminate carry a [`Guard`].

use crate::Status;

/// Why an operation stopped early (never visible as a panic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    /// An index out of range (the C++ would read or write out of bounds).
    Index,
    /// A loop ran past its bound (the C++ would loop forever on this input).
    Loop,
    /// A size past the crate's caps (the C++ would overflow an `int` or exhaust memory).
    TooLarge,
}

impl Fault {
    pub(crate) fn status(self) -> Status {
        match self {
            Fault::TooLarge => Status::ResultTooLarge,
            Fault::Index | Fault::Loop => Status::Internal,
        }
    }
}

pub(crate) type R<T> = Result<T, Fault>;

/// `i32` index → `usize`, negative → [`Fault::Index`].
#[inline]
pub(crate) fn ux(i: i32) -> R<usize> {
    usize::try_from(i).map_err(|_| Fault::Index)
}

/// `usize` → `i32`, too large → [`Fault::TooLarge`].
#[inline]
pub(crate) fn ix(u: usize) -> R<i32> {
    i32::try_from(u).map_err(|_| Fault::TooLarge)
}

/// Checked reads by `i32` (the C++ `int` indices) or `usize`.
pub(crate) trait Get<T: Copy> {
    fn at(&self, i: i32) -> R<T>;
    fn atu(&self, i: usize) -> R<T>;
}

impl<T: Copy> Get<T> for [T] {
    #[inline]
    fn at(&self, i: i32) -> R<T> {
        self.get(ux(i)?).copied().ok_or(Fault::Index)
    }
    #[inline]
    fn atu(&self, i: usize) -> R<T> {
        self.get(i).copied().ok_or(Fault::Index)
    }
}

/// Checked writes.
pub(crate) trait GetMut<T> {
    fn at_mut(&mut self, i: i32) -> R<&mut T>;
    fn atu_mut(&mut self, i: usize) -> R<&mut T>;
    fn set(&mut self, i: i32, v: T) -> R<()>;
    fn setu(&mut self, i: usize, v: T) -> R<()>;
}

impl<T> GetMut<T> for [T] {
    #[inline]
    fn at_mut(&mut self, i: i32) -> R<&mut T> {
        self.get_mut(ux(i)?).ok_or(Fault::Index)
    }
    #[inline]
    fn atu_mut(&mut self, i: usize) -> R<&mut T> {
        self.get_mut(i).ok_or(Fault::Index)
    }
    #[inline]
    fn set(&mut self, i: i32, v: T) -> R<()> {
        *self.at_mut(i)? = v;
        Ok(())
    }
    #[inline]
    fn setu(&mut self, i: usize, v: T) -> R<()> {
        *self.atu_mut(i)? = v;
        Ok(())
    }
}

/// A loop bound: `tick()` fails with [`Fault::Loop`] once the budget is spent.
pub(crate) struct Guard(u64);

impl Guard {
    pub(crate) fn new(budget: usize) -> Guard {
        Guard(u64::try_from(budget).unwrap_or(u64::MAX).saturating_add(16))
    }
    #[inline]
    pub(crate) fn tick(&mut self) -> R<()> {
        self.0 = self.0.checked_sub(1).ok_or(Fault::Loop)?;
        Ok(())
    }
}

/// A `std::stable_sort` comparator (`less(a, b)`) as a Rust ordering. For a strict weak order the stable sorts agree.
#[inline]
pub(crate) fn by_less<T>(less: impl Fn(&T, &T) -> bool) -> impl FnMut(&T, &T) -> core::cmp::Ordering {
    move |a, b| {
        if less(a, b) {
            core::cmp::Ordering::Less
        } else if less(b, a) {
            core::cmp::Ordering::Greater
        } else {
            core::cmp::Ordering::Equal
        }
    }
}

/// `Permute(inOut, new2Old)`: `out[i] = in[new2Old[i]]`.
pub(crate) fn permute<T: Copy>(v: &mut Vec<T>, new2old: &[i32]) -> R<()> {
    let mut out = Vec::with_capacity(new2old.len());
    for &o in new2old {
        out.push(v.at(o)?);
    }
    *v = out;
    Ok(())
}

/// `std::min` (returns `a` unless `b < a`).
#[inline]
pub(crate) fn std_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `std::max` (returns `a` unless `a < b`).
#[inline]
pub(crate) fn std_max(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// A fast hasher for integer keys (FxHash's rotate-multiply). Only lookups go through the hash maps of this crate
/// (never their iteration order), so the hasher cannot change any result.
#[derive(Default, Clone, Copy)]
pub(crate) struct FxHasher(u64);

impl core::hash::Hasher for FxHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.write_u64(u64::from(i));
    }
    #[inline]
    fn write_i32(&mut self, i: i32) {
        self.write_u64(u64::from(i.cast_unsigned()));
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
}

pub(crate) type FxMap<K, V> = std::collections::HashMap<K, V, core::hash::BuildHasherDefault<FxHasher>>;
