// Copyright 2024 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! minkowski.cpp: the Minkowski sum (dilation) and difference (erosion) as unions of convex hulls, and
//! `Manifold::Hull()` of a solid.
//!
//! Each hull takes one new mesh ID, in the C++ order (batches of 1000 triangles; for two non-convex solids, the face
//! pairs of each triangle of the first, reduced every 200 triangles), so a sequence of calls numbers its results as the
//! wasm does. The work is capped: at most [`MAX_HULL_POINTS`] points in the product of the two vertex sets and
//! [`MAX_FACE_PAIRS`] face pairs; past that the result is an empty Manifold with `ResultTooLarge`.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::sync::Arc;

use crate::csg::Manifold;
use crate::hull::hull_impl;
use crate::imp::Impl;
use crate::linalg::{V3, dot3};
use crate::util::{Get, R, ix};
use crate::{OpType, Status};

/// Largest number of points one Minkowski step may hand to the hull (|A| × |B| for two convex solids, 3 × |B| per
/// triangle otherwise).
pub const MAX_HULL_POINTS: usize = 1 << 22;
/// Largest number of face pairs (triangles of A × triangles of B) for two non-convex solids.
pub const MAX_FACE_PAIRS: usize = 1 << 24;

/// `evalBatch`: a batch boolean, evaluated at once.
fn eval_batch(items: &[Manifold], op: OpType) -> Manifold {
    let tree = Manifold::batch_boolean(items, op);
    let _ = tree.leaf_impl();
    tree
}

fn minkowski_impl(a: &Arc<Impl>, b: &Arc<Impl>, inset: bool) -> R<Manifold> {
    let mut a_impl = a;
    let mut b_impl = b;
    let mut a_convex = a_impl.is_convex()?;
    let mut b_convex = b_impl.is_convex()?;
    const BATCH_SIZE: usize = 1000;
    if a_convex && !b_convex {
        core::mem::swap(&mut a_impl, &mut b_impl);
        core::mem::swap(&mut a_convex, &mut b_convex);
    }
    if b_impl.is_empty() {
        return Ok(Manifold::from_impl((**a_impl).clone()));
    }
    if a_impl.is_empty() {
        return Ok(Manifold::from_impl((**b_impl).clone()));
    }
    let mut composed: Vec<Manifold> = vec![Manifold::from_impl((**a_impl).clone())];
    let verts = &b_impl.vert_pos;
    if !inset && a_convex && b_convex {
        if a_impl.vert_pos.len().saturating_mul(verts.len()) > MAX_HULL_POINTS {
            return Ok(Manifold::with_status(Status::ResultTooLarge));
        }
        let mut simple: Vec<V3> = Vec::with_capacity(verts.len() * a_impl.vert_pos.len());
        for &vertex in &a_impl.vert_pos {
            simple.extend(verts.iter().map(|&v| v + vertex));
        }
        composed.push(Manifold::from_impl(hull_impl(&simple)));
    } else if (inset || !a_convex) && b_convex {
        if verts.len().saturating_mul(3) > MAX_HULL_POINTS {
            return Ok(Manifold::with_status(Status::ResultTooLarge));
        }
        let num_tri = a_impl.num_tri();
        let mut offset = 0usize;
        while offset < num_tri {
            let num_iter = (num_tri - offset).min(BATCH_SIZE);
            let mut hulls = Vec::with_capacity(num_iter);
            for iter in 0..num_iter {
                let mut simple: Vec<V3> = Vec::with_capacity(3 * verts.len());
                for i in 0..3 {
                    let edge = ix((offset + iter) * 3 + i)?;
                    let vertex = a_impl.vert_pos.at(a_impl.halfedge.start(edge)?)?;
                    simple.extend(verts.iter().map(|&v| v + vertex));
                }
                hulls.push(Manifold::from_impl(hull_impl(&simple)));
            }
            composed.push(eval_batch(&hulls, OpType::Add));
            offset += BATCH_SIZE;
        }
    } else if !a_convex && !b_convex {
        let num_tri_a = a_impl.num_tri();
        let num_tri_b = b_impl.num_tri();
        if num_tri_a.saturating_mul(num_tri_b) > MAX_FACE_PAIRS {
            return Ok(Manifold::with_status(Status::ResultTooLarge));
        }
        const REDUCE_THRESHOLD: usize = 200;
        let mut accumulated: Vec<Manifold> = Vec::new();
        let corner = |imp: &Impl, face: usize, i: usize| -> R<V3> { imp.vert_pos.at(imp.halfedge.start(ix(face * 3 + i)?)?) };
        for a_face in 0..num_tri_a {
            let (a1, a2, a3) = (corner(a_impl, a_face, 0)?, corner(a_impl, a_face, 1)?, corner(a_impl, a_face, 2)?);
            let n_a = a_impl.face_normal.atu(a_face)?;
            let mut valid: Vec<Manifold> = Vec::new();
            for b_face in 0..num_tri_b {
                const COPLANAR_TOL: f64 = 1e-12;
                let n_b = b_impl.face_normal.atu(b_face)?;
                let same = dot3(n_a, n_b);
                let opp = dot3(n_a, -n_b);
                if (same - 1.0).abs() < COPLANAR_TOL || (opp - 1.0).abs() < COPLANAR_TOL {
                    continue;
                }
                let (b1, b2, b3) = (corner(b_impl, b_face, 0)?, corner(b_impl, b_face, 1)?, corner(b_impl, b_face, 2)?);
                let h = hull_impl(&[a1 + b1, a1 + b2, a1 + b3, a2 + b1, a2 + b2, a2 + b3, a3 + b1, a3 + b2, a3 + b3]);
                if !h.is_empty() {
                    valid.push(Manifold::from_impl(h));
                }
            }
            if !valid.is_empty() {
                accumulated.push(eval_batch(&valid, OpType::Add));
            }
            if accumulated.len() >= REDUCE_THRESHOLD {
                let reduced = eval_batch(&accumulated, OpType::Add);
                accumulated.clear();
                accumulated.push(reduced);
            }
        }
        if !accumulated.is_empty() {
            composed.push(eval_batch(&accumulated, OpType::Add));
        }
    }
    Ok(eval_batch(&composed, if inset { OpType::Subtract } else { OpType::Add }).as_original())
}

impl Manifold {
    fn minkowski(&self, other: &Manifold, inset: bool) -> Manifold {
        let a = self.leaf_impl();
        if a.status != Status::NoError {
            return Manifold::with_status(a.status);
        }
        let b = other.leaf_impl();
        if b.status != Status::NoError {
            return Manifold::with_status(b.status);
        }
        minkowski_impl(&a, &b, inset).unwrap_or_else(|f| Manifold::with_status(f.status()))
    }

    /// `MinkowskiSum(other)` (manifold-3d `minkowskiSum`): the dilation of this solid by the other.
    pub fn minkowski_sum(&self, other: &Manifold) -> Manifold {
        self.minkowski(other, false)
    }

    /// `MinkowskiDifference(other)` (manifold-3d `minkowskiDifference`): the erosion of this solid by the other.
    pub fn minkowski_difference(&self, other: &Manifold) -> Manifold {
        self.minkowski(other, true)
    }

    /// `Hull()`: the convex hull of this solid's vertices.
    pub fn hull(&self) -> Manifold {
        let src = self.leaf_impl();
        if src.status != Status::NoError {
            return Manifold::with_status(src.status);
        }
        Manifold::from_impl(hull_impl(&src.vert_pos))
    }
}
