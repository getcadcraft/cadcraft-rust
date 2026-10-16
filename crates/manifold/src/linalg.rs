// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! The parts of linalg.h (as bundled with Manifold) and common.h (Box, Rect) the boolean uses, with the same
//! floating-point operations in the same order: `dot` folds from `0.0` (`((0 + x·x') + y·y') + z·z'`, which turns a
//! `-0` product into `+0`), `normalize` divides each component by the length, `la::min/max` are `a < b ? a : b` /
//! `a < b ? b : a` (not `f64::min`), matrix × vector sums the scaled columns left to right.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use core::ops::{Add, AddAssign, Div, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct V2 {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct V4 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

#[inline]
pub(crate) const fn v2(x: f64, y: f64) -> V2 {
    V2 { x, y }
}
#[inline]
pub(crate) const fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3 { x, y, z }
}

/// `la::min` on scalars.
#[inline]
pub(crate) fn la_min(a: f64, b: f64) -> f64 {
    if a < b { a } else { b }
}
/// `la::max` on scalars.
#[inline]
pub(crate) fn la_max(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

impl V2 {
    #[inline]
    pub(crate) const fn splat(v: f64) -> V2 {
        V2 { x: v, y: v }
    }
}

impl V3 {
    #[inline]
    pub(crate) const fn splat(v: f64) -> V3 {
        V3 { x: v, y: v, z: v }
    }
    #[inline]
    pub(crate) fn get(self, i: usize) -> f64 {
        match i {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }
    #[inline]
    pub(crate) fn set(&mut self, i: usize, v: f64) {
        match i {
            0 => self.x = v,
            1 => self.y = v,
            _ => self.z = v,
        }
    }
    #[inline]
    pub(crate) fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
    #[inline]
    pub(crate) fn abs(self) -> V3 {
        v3(self.x.abs(), self.y.abs(), self.z.abs())
    }
    /// Component-wise division by another vector.
    #[inline]
    pub(crate) fn div_v(self, o: V3) -> V3 {
        v3(self.x / o.x, self.y / o.y, self.z / o.z)
    }
}

/// Component-wise `la::min`.
#[inline]
pub(crate) fn min3(a: V3, b: V3) -> V3 {
    v3(la_min(a.x, b.x), la_min(a.y, b.y), la_min(a.z, b.z))
}
/// Component-wise `la::max`.
#[inline]
pub(crate) fn max3(a: V3, b: V3) -> V3 {
    v3(la_max(a.x, b.x), la_max(a.y, b.y), la_max(a.z, b.z))
}
#[inline]
pub(crate) fn min2(a: V2, b: V2) -> V2 {
    v2(la_min(a.x, b.x), la_min(a.y, b.y))
}
#[inline]
pub(crate) fn max2(a: V2, b: V2) -> V2 {
    v2(la_max(a.x, b.x), la_max(a.y, b.y))
}

macro_rules! vec_ops {
    ($t:ident, $($f:ident),+) => {
        impl Add for $t {
            type Output = $t;
            #[inline]
            fn add(self, o: $t) -> $t { $t { $($f: self.$f + o.$f),+ } }
        }
        impl Sub for $t {
            type Output = $t;
            #[inline]
            fn sub(self, o: $t) -> $t { $t { $($f: self.$f - o.$f),+ } }
        }
        impl Mul<f64> for $t {
            type Output = $t;
            #[inline]
            fn mul(self, s: f64) -> $t { $t { $($f: self.$f * s),+ } }
        }
        impl Mul<$t> for f64 {
            type Output = $t;
            #[inline]
            fn mul(self, v: $t) -> $t { $t { $($f: self * v.$f),+ } }
        }
        impl Div<f64> for $t {
            type Output = $t;
            #[inline]
            fn div(self, s: f64) -> $t { $t { $($f: self.$f / s),+ } }
        }
        impl Neg for $t {
            type Output = $t;
            #[inline]
            fn neg(self) -> $t { $t { $($f: -self.$f),+ } }
        }
        impl Add<f64> for $t {
            type Output = $t;
            #[inline]
            fn add(self, s: f64) -> $t { $t { $($f: self.$f + s),+ } }
        }
        impl Sub<f64> for $t {
            type Output = $t;
            #[inline]
            fn sub(self, s: f64) -> $t { $t { $($f: self.$f - s),+ } }
        }
        impl AddAssign for $t {
            #[inline]
            fn add_assign(&mut self, o: $t) { $(self.$f = self.$f + o.$f;)+ }
        }
    };
}
vec_ops!(V2, x, y);
vec_ops!(V3, x, y, z);
vec_ops!(V4, x, y, z, w);

/// `la::dot` for 2-vectors: `(0 + x·x') + y·y'`.
#[inline]
pub(crate) fn dot2(a: V2, b: V2) -> f64 {
    0.0 + a.x * b.x + a.y * b.y
}
/// `la::dot` for 3-vectors: `((0 + x·x') + y·y') + z·z'`.
#[inline]
pub(crate) fn dot3(a: V3, b: V3) -> f64 {
    0.0 + a.x * b.x + a.y * b.y + a.z * b.z
}
#[inline]
pub(crate) fn length2_3(a: V3) -> f64 {
    dot3(a, a)
}
#[inline]
pub(crate) fn length3(a: V3) -> f64 {
    dot3(a, a).sqrt()
}
#[inline]
pub(crate) fn length2(a: V2) -> f64 {
    dot2(a, a).sqrt()
}
#[inline]
pub(crate) fn normalize3(a: V3) -> V3 {
    a / length3(a)
}
#[inline]
pub(crate) fn normalize2(a: V2) -> V2 {
    a / length2(a)
}
#[inline]
pub(crate) fn cross3(a: V3, b: V3) -> V3 {
    v3(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x)
}

/// shared.h `SafeNormalize`.
#[inline]
pub(crate) fn safe_normalize3(v: V3) -> V3 {
    let n = normalize3(v);
    if n.x.is_finite() { n } else { V3::splat(0.0) }
}

/// polygon.cpp `determinant2x2`.
#[inline]
pub(crate) fn det2(a: V2, b: V2) -> f64 {
    a.x * b.y - a.y * b.x
}

/// utils.h `CCW`: 1 for CCW, -1 for CW, 0 within `tol` of colinear.
#[inline]
pub(crate) fn ccw(p0: V2, p1: V2, p2: V2, tol: f64) -> i32 {
    let v1 = p1 - p0;
    let v2 = p2 - p0;
    let area = v1.x * v2.y - v1.y * v2.x;
    let base2 = la_max(dot2(v1, v1), dot2(v2, v2));
    if area * area * 4.0 <= base2 * tol * tol {
        0
    } else if area > 0.0 {
        1
    } else {
        -1
    }
}

/// A 2×3 matrix (three columns of 2-vectors): the axis-aligned projections of shared.h.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Proj {
    pub c0: V2,
    pub c1: V2,
    pub c2: V2,
}

impl Proj {
    /// `mat2x3 * vec3` = `c0·v.x + c1·v.y + c2·v.z`.
    #[inline]
    pub(crate) fn apply(&self, v: V3) -> V2 {
        self.c0 * v.x + self.c1 * v.y + self.c2 * v.z
    }
}

/// shared.h `GetAxisAlignedProjection`: the closest axis-aligned projection, flipped to keep the winding.
pub(crate) fn axis_aligned_projection(normal: V3) -> Proj {
    let a = normal.abs();
    let (mut p0, p1, xyz_max) = if a.z > a.x && a.z > a.y {
        (v3(1.0, 0.0, 0.0), v3(0.0, 1.0, 0.0), normal.z)
    } else if a.y > a.x {
        (v3(0.0, 0.0, 1.0), v3(1.0, 0.0, 0.0), normal.y)
    } else {
        (v3(0.0, 1.0, 0.0), v3(0.0, 0.0, 1.0), normal.x)
    };
    if xyz_max < 0.0 {
        // `projection[0] *= -1.0`: the zeros become -0
        p0 = p0 * -1.0;
    }
    // la::transpose of the 3×2 (columns p0, p1)
    Proj { c0: v2(p0.x, p1.x), c1: v2(p0.y, p1.y), c2: v2(p0.z, p1.z) }
}

/// common.h `Box`: axis-aligned 3D box, empty = (+inf, -inf).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Box3 {
    pub min: V3,
    pub max: V3,
}

impl Default for Box3 {
    fn default() -> Self {
        Box3 { min: V3::splat(f64::INFINITY), max: V3::splat(f64::NEG_INFINITY) }
    }
}

impl Box3 {
    /// `Box(p1, p2)`.
    #[inline]
    pub(crate) fn from_points(p1: V3, p2: V3) -> Box3 {
        Box3 { min: min3(p1, p2), max: max3(p1, p2) }
    }
    #[inline]
    pub(crate) fn size(&self) -> V3 {
        self.max - self.min
    }
    #[inline]
    pub(crate) fn scale(&self) -> f64 {
        let abs_max = max3(self.min.abs(), self.max.abs());
        la_max(abs_max.x, la_max(abs_max.y, abs_max.z))
    }
    #[inline]
    pub(crate) fn union_pt(&mut self, p: V3) {
        self.min = min3(self.min, p);
        self.max = max3(self.max, p);
    }
    #[inline]
    pub(crate) fn union(&self, b: &Box3) -> Box3 {
        Box3 { min: min3(self.min, b.min), max: max3(self.max, b.max) }
    }
    #[inline]
    pub(crate) fn does_overlap(&self, b: &Box3) -> bool {
        self.min.x <= b.max.x
            && self.min.y <= b.max.y
            && self.min.z <= b.max.z
            && self.max.x >= b.min.x
            && self.max.y >= b.min.y
            && self.max.z >= b.min.z
    }
    /// `DoesOverlap(vec3 p)`: projected in z.
    #[inline]
    pub(crate) fn does_overlap_pt(&self, p: V3) -> bool {
        p.x <= self.max.x && p.x >= self.min.x && p.y <= self.max.y && p.y >= self.min.y
    }
    #[inline]
    pub(crate) fn is_finite(&self) -> bool {
        self.min.is_finite() && self.max.is_finite()
    }
    /// `Box::Transform` by the identity 3×4 matrix (the only transform the CadCraft paths use): `1·x + 0·y + 0·z +
    /// 0·1` per row, which turns infinite corners into NaN exactly as the C++ does.
    pub(crate) fn transform_identity(&self) -> Box3 {
        let t = |p: V3| -> V3 {
            v3(
                1.0 * p.x + 0.0 * p.y + 0.0 * p.z + 0.0 * 1.0,
                0.0 * p.x + 1.0 * p.y + 0.0 * p.z + 0.0 * 1.0,
                0.0 * p.x + 0.0 * p.y + 1.0 * p.z + 0.0 * 1.0,
            )
        };
        let a = t(self.min);
        let b = t(self.max);
        Box3 { min: min3(a, b), max: max3(a, b) }
    }
}

/// common.h `Rect`: axis-aligned 2D box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub min: V2,
    pub max: V2,
}

impl Default for Rect {
    fn default() -> Self {
        Rect { min: V2::splat(f64::INFINITY), max: V2::splat(f64::NEG_INFINITY) }
    }
}

impl Rect {
    #[inline]
    pub(crate) fn from_points(a: V2, b: V2) -> Rect {
        Rect { min: min2(a, b), max: max2(a, b) }
    }
    #[inline]
    pub(crate) fn size(&self) -> V2 {
        self.max - self.min
    }
    #[inline]
    pub(crate) fn union_pt(&mut self, p: V2) {
        self.min = min2(self.min, p);
        self.max = max2(self.max, p);
    }
    /// `Contains(p)`: inclusive on the border.
    #[inline]
    pub(crate) fn contains(&self, p: V2) -> bool {
        p.x >= self.min.x && p.y >= self.min.y && self.max.x >= p.x && self.max.y >= p.y
    }
    #[inline]
    pub(crate) fn does_overlap(&self, r: &Rect) -> bool {
        self.min.x <= r.max.x && self.min.y <= r.max.y && self.max.x >= r.min.x && self.max.y >= r.min.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_folds_from_zero() {
        // -0·1 = -0, and 0 + -0 = +0
        let d = dot3(v3(-0.0, 0.0, 0.0), v3(1.0, 1.0, 1.0));
        assert!(d == 0.0 && d.is_sign_positive());
    }

    #[test]
    fn projection_negates_zero_entries() {
        let p = axis_aligned_projection(v3(0.0, 0.0, -1.0));
        assert_eq!(p.c0.x, -1.0);
        assert!(p.c1.x.is_sign_negative() && p.c2.x.is_sign_negative());
    }

    #[test]
    fn la_min_max_nan() {
        assert_eq!(la_max(0.0, f64::NAN), 0.0);
        assert!(la_min(f64::NAN, 1.0) == 1.0);
    }
}
