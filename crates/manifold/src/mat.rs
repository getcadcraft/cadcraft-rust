// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! The matrices of linalg.h the transforms use (column-major, as linalg stores them): `mat3`, `mat3x4` (an affine
//! transform: three columns of the linear part, then the translation), `mat2x3`, with the same floating-point
//! operations in the same order. Matrix × vector sums the scaled columns left to right
//! (`a.x·b.x + a.y·b.y + a.z·b.z (+ a.w·b.w)`), matrix × matrix is that per column, and `Mat4(m)` appends the row
//! `(0, 0, 0, 1)`, so `m1 * Mat4(m2)` multiplies through the zeros and the one (a `-0` entry can become `+0`, as in
//! the C++).
#![allow(clippy::arithmetic_side_effects)]

use crate::linalg::{V3, V4, v3};

/// `mat3`: three columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Mat3(pub [V3; 3]);

/// `mat3x4`: three linear columns and the translation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Mat34(pub [V3; 4]);

/// `la::identity` as a 3×4.
pub(crate) const IDENTITY: Mat34 = Mat34([v3(1.0, 0.0, 0.0), v3(0.0, 1.0, 0.0), v3(0.0, 0.0, 1.0), v3(0.0, 0.0, 0.0)]);

impl Default for Mat34 {
    fn default() -> Self {
        IDENTITY
    }
}

impl Mat3 {
    pub(crate) const IDENTITY: Mat3 = Mat3([v3(1.0, 0.0, 0.0), v3(0.0, 1.0, 0.0), v3(0.0, 0.0, 1.0)]);

    /// `mat3 * vec3` = `c0·v.x + c1·v.y + c2·v.z`.
    #[inline]
    pub(crate) fn mul_v(&self, v: V3) -> V3 {
        let [a, b, c] = self.0;
        a * v.x + b * v.y + c * v.z
    }

    /// `mat3 * mat3`.
    pub(crate) fn mul(&self, b: &Mat3) -> Mat3 {
        Mat3([self.mul_v(b.0[0]), self.mul_v(b.0[1]), self.mul_v(b.0[2])])
    }

    /// `la::transpose`.
    pub(crate) fn transpose(&self) -> Mat3 {
        let [a, b, c] = self.0;
        Mat3([v3(a.x, b.x, c.x), v3(a.y, b.y, c.y), v3(a.z, b.z, c.z)])
    }

    /// `la::determinant` (expansion along the first column, as linalg writes it).
    pub(crate) fn determinant(&self) -> f64 {
        let [x, y, z] = self.0;
        x.x * (y.y * z.z - z.y * y.z) + x.y * (y.z * z.x - z.z * y.x) + x.z * (y.x * z.y - z.x * y.y)
    }

    /// `la::adjugate`.
    pub(crate) fn adjugate(&self) -> Mat3 {
        let [x, y, z] = self.0;
        Mat3([
            v3(y.y * z.z - z.y * y.z, z.y * x.z - x.y * z.z, x.y * y.z - y.y * x.z),
            v3(y.z * z.x - z.z * y.x, z.z * x.x - x.z * z.x, x.z * y.x - y.z * x.x),
            v3(y.x * z.y - z.x * y.y, z.x * x.y - x.x * z.y, x.x * y.y - y.x * x.y),
        ])
    }

    /// `la::inverse` = `adjugate(a) / determinant(a)` (each entry divided).
    pub(crate) fn inverse(&self) -> Mat3 {
        let d = self.determinant();
        let [a, b, c] = self.adjugate().0;
        Mat3([a / d, b / d, c / d])
    }

    /// Column `i`, row `j` (`m[i][j]`); out of range reads 0.
    #[inline]
    pub(crate) fn at(&self, i: usize, j: usize) -> f64 {
        self.0.get(i).map_or(0.0, |c| c.get(j))
    }

    /// Sets `m[i][j]`.
    #[inline]
    pub(crate) fn set(&mut self, i: usize, j: usize, v: f64) {
        if let Some(c) = self.0.get_mut(i) {
            c.set(j, v);
        }
    }
}

impl Mat34 {
    /// `mat3x4(mat3, vec3)`.
    #[inline]
    pub(crate) fn from_parts(m: Mat3, t: V3) -> Mat34 {
        let [a, b, c] = m.0;
        Mat34([a, b, c, t])
    }

    /// `mat3(m)`: the linear part.
    #[inline]
    pub(crate) fn linear(&self) -> Mat3 {
        let [a, b, c, _] = self.0;
        Mat3([a, b, c])
    }

    /// `mat3x4 * vec4(p, 1)` = `c0·x + c1·y + c2·z + c3·1`.
    #[inline]
    pub(crate) fn apply(&self, p: V3) -> V3 {
        let [a, b, c, d] = self.0;
        a * p.x + b * p.y + c * p.z + d * 1.0
    }

    /// `mat3x4 * vec4`.
    #[inline]
    fn mul_v4(&self, v: V4) -> V3 {
        let [a, b, c, d] = self.0;
        a * v.x + b * v.y + c * v.z + d * v.w
    }

    /// `self * Mat4(o)`: `o`'s columns as `(c, 0)` and its translation as `(t, 1)`.
    pub(crate) fn mul_mat4(&self, o: &Mat34) -> Mat34 {
        let [a, b, c, t] = o.0;
        let col = |v: V3, w: f64| V4 { x: v.x, y: v.y, z: v.z, w };
        Mat34([self.mul_v4(col(a, 0.0)), self.mul_v4(col(b, 0.0)), self.mul_v4(col(c, 0.0)), self.mul_v4(col(t, 1.0))])
    }

    /// `transform == mat3x4(la::identity)` (IEEE comparison: `-0 == 0`).
    #[inline]
    pub(crate) fn is_identity(&self) -> bool {
        *self == IDENTITY
    }

    /// `la::all(la::isfinite(m))`.
    pub(crate) fn is_finite(&self) -> bool {
        self.0.iter().all(|c| c.is_finite())
    }

    /// The column-major 12 floats (`Relation::transform`'s storage).
    pub(crate) fn to_array(self) -> [f64; 12] {
        let [a, b, c, d] = self.0;
        [a.x, a.y, a.z, b.x, b.y, b.z, c.x, c.y, c.z, d.x, d.y, d.z]
    }

    /// From the column-major 12 floats.
    pub(crate) fn from_array(m: &[f64; 12]) -> Mat34 {
        let [a0, a1, a2, b0, b1, b2, c0, c1, c2, d0, d1, d2] = *m;
        Mat34([v3(a0, a1, a2), v3(b0, b1, b2), v3(c0, c1, c2), v3(d0, d1, d2)])
    }

    /// The JavaScript binding's `transform(mat4)`: a column-major 4×4 (16 numbers), its first three rows.
    pub(crate) fn from_mat4_js(m: &[f64; 16]) -> Mat34 {
        let g = |i: usize| m.get(i).copied().unwrap_or(0.0);
        Mat34([v3(g(0), g(1), g(2)), v3(g(4), g(5), g(6)), v3(g(8), g(9), g(10)), v3(g(12), g(13), g(14))])
    }

    /// `m[col][row]`.
    #[inline]
    pub(crate) fn at(&self, col: usize, row: usize) -> f64 {
        self.0.get(col).map_or(0.0, |c| c.get(row))
    }
}

/// shared.h `NormalTransform`: `inverse(transpose(mat3(transform)))`.
pub(crate) fn normal_transform(t: &Mat34) -> Mat3 {
    t.linear().transpose().inverse()
}

// ---------------------------------------------------------------- svd.h
//
// MIT License
// Copyright (c) 2019 wi-re
// Copyright 2023 The Manifold Authors.
// Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated
// documentation files (the "Software"), to deal in the Software without restriction, including without limitation the
// rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to the following conditions:
// The above copyright notice and this permission notice shall be included in all copies or substantial portions of the
// Software.
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
// WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
// COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
// (Modified from https://github.com/wi-re/tbtSVD by the Manifold Authors; only the largest singular value is kept here.)

const GAMMA: f64 = 5.828_427_124_746_19;
const C_STAR: f64 = 0.923_879_532_511_286_7;
const S_STAR: f64 = 0.382_683_432_365_089_8;
const SVD_EPSILON: f64 = 1e-6;
const JACOBI_STEPS: usize = 12;

#[derive(Clone, Copy)]
struct Sym3 {
    m00: f64,
    m10: f64,
    m11: f64,
    m20: f64,
    m21: f64,
    m22: f64,
}

#[inline]
fn cond_swap(c: bool, x: &mut f64, y: &mut f64) {
    let z = *x;
    *x = if c { *y } else { *x };
    *y = if c { z } else { *y };
}

#[inline]
fn cond_neg_swap(c: bool, x: &mut f64, y: &mut f64) {
    let z = -*x;
    *x = if c { *y } else { *x };
    *y = if c { z } else { *y };
}

fn approximate_givens(a: &Sym3) -> (f64, f64) {
    let ch = 2.0 * (a.m00 - a.m11);
    let sh = a.m10;
    let mut b = GAMMA * sh * sh < ch * ch;
    let w = 1.0 / (ch * ch + sh * sh).sqrt();
    if !w.is_finite() {
        b = false;
    }
    (if b { w * ch } else { C_STAR }, if b { w * sh } else { S_STAR })
}

fn jacobi_conjugation(x: usize, y: usize, z: usize, s: &mut Sym3, q: &mut V4) {
    let (ch, mut sh) = approximate_givens(s);
    let scale = 1.0 / (ch * ch + sh * sh);
    let a = (ch * ch - sh * sh) * scale;
    let b = 2.0 * sh * ch * scale;
    let s0 = *s;
    s.m00 = a * (a * s0.m00 + b * s0.m10) + b * (a * s0.m10 + b * s0.m11);
    s.m10 = a * (-b * s0.m00 + a * s0.m10) + b * (-b * s0.m10 + a * s0.m11);
    s.m11 = -b * (-b * s0.m00 + a * s0.m10) + a * (-b * s0.m10 + a * s0.m11);
    s.m20 = a * s0.m20 + b * s0.m21;
    s.m21 = -b * s0.m20 + a * s0.m21;
    s.m22 = s0.m22;
    let tmp = v3(q.x, q.y, q.z) * sh;
    sh *= q.w;
    let qv = [q.x, q.y, q.z];
    let get = |v: &[f64; 3], i: usize| v.get(i).copied().unwrap_or(0.0);
    let tv = [tmp.x, tmp.y, tmp.z];
    let mut out = qv;
    if let Some(o) = out.get_mut(z) {
        *o = get(&qv, z) * ch + sh;
    }
    q.w = q.w * ch + -get(&tv, z);
    if let Some(o) = out.get_mut(x) {
        *o = get(&qv, x) * ch + get(&tv, y);
    }
    if let Some(o) = out.get_mut(y) {
        *o = get(&qv, y) * ch + -get(&tv, x);
    }
    q.x = out[0];
    q.y = out[1];
    q.z = out[2];
    // re-arrange the matrix for the next iteration
    let r = *s;
    *s = Sym3 { m00: r.m11, m10: r.m21, m11: r.m22, m20: r.m10, m21: r.m20, m22: r.m00 };
}

fn jacobi_eigen(mut s: Sym3) -> Mat3 {
    let mut q = V4 { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };
    for _ in 0..JACOBI_STEPS {
        jacobi_conjugation(0, 1, 2, &mut s, &mut q);
        jacobi_conjugation(1, 2, 0, &mut s, &mut q);
        jacobi_conjugation(2, 0, 1, &mut s, &mut q);
    }
    Mat3([
        v3(1.0 - 2.0 * (q.y * q.y + q.z * q.z), 2.0 * (q.x * q.y + q.w * q.z), 2.0 * (q.x * q.z + -q.w * q.y)),
        v3(2.0 * (q.x * q.y + -q.w * q.z), 1.0 - 2.0 * (q.x * q.x + q.z * q.z), 2.0 * (q.y * q.z + q.w * q.x)),
        v3(2.0 * (q.x * q.z + q.w * q.y), 2.0 * (q.y * q.z + -q.w * q.x), 1.0 - 2.0 * (q.x * q.x + q.y * q.y)),
    ])
}

fn dist2(v: V3) -> f64 {
    crate::linalg::dot3(v, v)
}

fn sort_singular_values(b: &mut Mat3, v: &mut Mat3) {
    let mut rho1 = dist2(b.0[0]);
    let mut rho2 = dist2(b.0[1]);
    let mut rho3 = dist2(b.0[2]);
    let swap_cols = |b: &mut Mat3, v: &mut Mat3, c: bool, i: usize, j: usize| {
        for k in 0..3 {
            let (mut x, mut y) = (b.at(i, k), b.at(j, k));
            cond_neg_swap(c, &mut x, &mut y);
            b.set(i, k, x);
            b.set(j, k, y);
            let (mut x, mut y) = (v.at(i, k), v.at(j, k));
            cond_neg_swap(c, &mut x, &mut y);
            v.set(i, k, x);
            v.set(j, k, y);
        }
    };
    let c = rho1 < rho2;
    swap_cols(b, v, c, 0, 1);
    cond_swap(c, &mut rho1, &mut rho2);
    let c = rho1 < rho3;
    swap_cols(b, v, c, 0, 2);
    cond_swap(c, &mut rho1, &mut rho3);
    let c = rho2 < rho3;
    swap_cols(b, v, c, 1, 2);
}

fn qr_givens(a1: f64, a2: f64) -> (f64, f64) {
    let rho = (a1 * a1 + a2 * a2).sqrt();
    let mut ch = a1.abs() + libm::fmax(rho, SVD_EPSILON);
    let mut sh = if rho > SVD_EPSILON { a2 } else { 0.0 };
    let b = a1 < 0.0;
    cond_swap(b, &mut sh, &mut ch);
    let w = 1.0 / (ch * ch + sh * sh).sqrt();
    ch *= w;
    sh *= w;
    (ch, sh)
}

/// svd.h `QRDecomposition`: only R is needed for the spectral norm (`S = R`).
fn qr_r(b: &mut Mat3) -> Mat3 {
    let mut r = Mat3::IDENTITY;
    let (ch, sh) = qr_givens(b.at(0, 0), b.at(0, 1));
    let mut a = -2.0 * sh * sh + 1.0;
    let mut bb = 2.0 * ch * sh;
    for i in 0..3 {
        r.set(i, 0, a * b.at(i, 0) + bb * b.at(i, 1));
    }
    for i in 0..3 {
        r.set(i, 1, -bb * b.at(i, 0) + a * b.at(i, 1));
    }
    for i in 0..3 {
        r.set(i, 2, b.at(i, 2));
    }
    let (ch, sh) = qr_givens(r.at(0, 0), r.at(0, 2));
    a = -2.0 * sh * sh + 1.0;
    bb = 2.0 * ch * sh;
    for i in 0..3 {
        b.set(i, 0, a * r.at(i, 0) + bb * r.at(i, 2));
    }
    for i in 0..3 {
        b.set(i, 1, r.at(i, 1));
    }
    for i in 0..3 {
        b.set(i, 2, -bb * r.at(i, 0) + a * r.at(i, 2));
    }
    let (ch, sh) = qr_givens(b.at(1, 1), b.at(1, 2));
    a = -2.0 * sh * sh + 1.0;
    bb = 2.0 * ch * sh;
    for i in 0..3 {
        r.set(i, 0, b.at(i, 0));
    }
    for i in 0..3 {
        r.set(i, 1, a * b.at(i, 1) + bb * b.at(i, 2));
    }
    for i in 0..3 {
        r.set(i, 2, -bb * b.at(i, 1) + a * b.at(i, 2));
    }
    r
}

/// svd.h `SpectralNorm`: the largest singular value, `SVD(A).S[0][0]`.
pub(crate) fn spectral_norm(a: &Mat3) -> f64 {
    let ata = a.transpose().mul(a);
    let s = Sym3 { m00: ata.at(0, 0), m10: ata.at(0, 1), m11: ata.at(1, 1), m20: ata.at(0, 2), m21: ata.at(1, 2), m22: ata.at(2, 2) };
    let mut v = jacobi_eigen(s);
    let mut b = a.mul(&v);
    sort_singular_values(&mut b, &mut v);
    qr_r(&mut b).at(0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectral_norm_of_scales() {
        let m = Mat3([v3(2.0, 0.0, 0.0), v3(0.0, -3.0, 0.0), v3(0.0, 0.0, 0.5)]);
        assert!((spectral_norm(&m) - 3.0).abs() < 1e-12);
        assert!((spectral_norm(&Mat3::IDENTITY) - 1.0).abs() < 1e-15);
    }

    #[test]
    fn mat4_product_turns_negative_zero_positive() {
        let mut m = IDENTITY;
        m.0[3] = v3(-0.0, 0.0, 0.0);
        let p = IDENTITY.mul_mat4(&m);
        assert!(p.0[3].x == 0.0 && p.0[3].x.is_sign_positive());
    }
}
