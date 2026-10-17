// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! impl.cpp `Impl::Transform` (and the mesh_fixes.h helpers it shares with `Compose`), properties.cpp `GetProperty`
//! (volume, surface area) and `MinGap`, and the triangle distance of tri_dist.h (from NVIDIA PhysX, BSD-3-Clause, via
//! Manifold).
//
// tri_dist.h: from NVIDIA-Omniverse PhysX (BSD 3-Clause "New" or "Revised" License,
// https://github.com/NVIDIA-Omniverse/PhysX/blob/main/LICENSE.md), GuSweepCapsuleCapsule.cpp and
// GuDistanceTriangleTriangle.cpp, with minor modifications by the Manifold Authors. Redistribution and use in source
// and binary forms, with or without modification, are permitted provided that the conditions of that licence are met;
// NVIDIA's name may not be used to endorse or promote products derived from this software.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use crate::Status;
use crate::collider::Collider;
use crate::disjoint::DisjointSets;
use crate::halfedge::Halfedges;
use crate::imp::{Impl, MeshRelation};
use crate::linalg::{Box3, V3, V4, cross3, dot3, la_max, la_min, length3, normalize3, safe_normalize3, v3};
use crate::mat::{Mat3, Mat34, normal_transform, spectral_norm};
use crate::util::{Get, GetMut, R, ix, std_min, ux};

/// mesh_fixes.h `FlipHalfedge`.
#[inline]
fn flip_halfedge(h: i32) -> i32 {
    let tri = h / 3;
    let vert = 2 - (h - 3 * tri);
    3 * tri + vert
}

/// mesh_fixes.h `TransformNormals`: the normal under `m`, normalised; NaN becomes zero.
#[inline]
pub(crate) fn transform_normal(m: &Mat3, n: V3) -> V3 {
    let r = normalize3(m.mul_v(n));
    if r.x.is_nan() { V3::splat(0.0) } else { r }
}

/// mesh_fixes.h `TransformTangents` over every old tangent: `out[edge + offset] = (m · old[in].xyz, old[in].w)`,
/// `in` the flipped edge's pair when the transform mirrors.
pub(crate) fn transform_tangents(out: &mut [V4], offset: usize, m: &Mat3, invert: bool, old: &[V4], halfedge: &Halfedges) -> R<()> {
    for edge_out in 0..old.len() {
        let eo = ix(edge_out)?;
        let edge_in = if invert { halfedge.pair(flip_halfedge(eo))? } else { eo };
        let t = old.at(edge_in)?;
        let v = m.mul_v(v3(t.x, t.y, t.z));
        out.setu(edge_out + offset, V4 { x: v.x, y: v.y, z: v.z, w: t.w })?;
    }
    Ok(())
}

/// mesh_fixes.h `FlipTris` for one triangle: reversed, each halfedge's pair flipped.
pub(crate) fn flip_tri(h: &mut Halfedges, tri: i32) -> R<()> {
    let face = [h.get(3 * tri + 2)?, h.get(3 * tri + 1)?, h.get(3 * tri)?];
    for (i, f) in face.iter().enumerate() {
        let e = 3 * tri + i as i32;
        // the start becomes the old end
        h.set_start(e, f.end_vert)?;
        h.set_pair(e, flip_halfedge(f.paired_halfedge))?;
        h.set_prop(e, f.prop_vert)?;
    }
    Ok(())
}

/// impl.h `TriHasNormals`.
fn tri_has_normals(rel: &MeshRelation, tri: i32) -> R<bool> {
    let id = rel.tri_ref.at(tri)?.mesh_id;
    Ok(rel.mesh_id_transform.get(&id).is_some_and(|r| r.has_normals))
}

/// `EagerTransformPropNormals`: property channels 0..2 of every vertex of a mesh that records normals, turned by the
/// normal transform and renormalised.
pub(crate) fn eager_transform_prop_normals(
    halfedge: &Halfedges,
    rel: &MeshRelation,
    nt: &Mat3,
    properties: &mut [f64],
    num_prop_vert: usize,
    stride: usize,
    offset: usize,
) -> R<()> {
    if !rel.mesh_id_transform.values().any(|r| r.has_normals) {
        return Ok(());
    }
    let mut visited = vec![false; num_prop_vert];
    for e in 0..ix(halfedge.len())? {
        if !tri_has_normals(rel, e / 3)? {
            continue;
        }
        let prop = halfedge.prop(e)?;
        if prop < 0 || visited.at(prop)? {
            continue;
        }
        visited.set(prop, true)?;
        let base = (offset + ux(prop)?) * stride;
        let n = v3(properties.atu(base)?, properties.atu(base + 1)?, properties.atu(base + 2)?);
        let n = safe_normalize3(nt.mul_v(n));
        properties.setu(base, n.x)?;
        properties.setu(base + 1, n.y)?;
        properties.setu(base + 2, n.z)?;
    }
    Ok(())
}

impl Box3 {
    /// common.h `Box::Transform`: both corners transformed, then min / max.
    pub(crate) fn transform(&self, t: &Mat34) -> Box3 {
        let a = t.apply(self.min);
        let b = t.apply(self.max);
        Box3 { min: crate::linalg::min3(a, b), max: crate::linalg::max3(a, b) }
    }
}

impl Impl {
    /// `Impl::Transform`: a transformed copy (relations, normals, tangents, flipped triangles when mirrored, epsilon
    /// scaled by the spectral norm, the collider moved or refitted). A failure inside gives an empty Impl with the
    /// status.
    pub(crate) fn transform(&self, t: &Mat34) -> Impl {
        if t.is_identity() {
            return self.clone();
        }
        if self.status != Status::NoError {
            return Impl::with_status(self.status);
        }
        if !t.is_finite() {
            let mut r = Impl::default();
            r.make_empty(Status::NonFiniteVertex);
            return r;
        }
        self.transform_inner(t).unwrap_or_else(|f| Impl::with_status(f.status()))
    }

    fn transform_inner(&self, t: &Mat34) -> R<Impl> {
        let mut r = Impl {
            mesh_relation: self.mesh_relation.clone(),
            epsilon: self.epsilon,
            tolerance: self.tolerance,
            num_prop: self.num_prop,
            properties: self.properties.clone(),
            bbox: self.bbox,
            halfedge: self.halfedge.clone(),
            halfedge_tangent: vec![V4::default(); self.halfedge_tangent.len()],
            ..Impl::default()
        };
        r.mesh_relation.original_id = -1;
        for rel in r.mesh_relation.mesh_id_transform.values_mut() {
            rel.transform = t.mul_mat4(&Mat34::from_array(&rel.transform)).to_array();
        }
        r.vert_pos = self.vert_pos.iter().map(|&p| t.apply(p)).collect();
        let nt = normal_transform(t);
        r.face_normal = self.face_normal.iter().map(|&n| transform_normal(&nt, n)).collect();
        r.vert_normal = self.vert_normal.iter().map(|&n| transform_normal(&nt, n)).collect();
        if self.num_prop >= 3 {
            let np = ux(self.num_prop)?;
            eager_transform_prop_normals(&self.halfedge, &self.mesh_relation, &nt, &mut r.properties, self.num_prop_vert(), np, 0)?;
        }
        let lin = t.linear();
        let invert = lin.determinant() < 0.0;
        if !self.halfedge_tangent.is_empty() {
            transform_tangents(&mut r.halfedge_tangent, 0, &lin, invert, &self.halfedge_tangent, &self.halfedge)?;
        }
        if invert {
            for tri in 0..ix(r.num_tri())? {
                flip_tri(&mut r.halfedge, tri)?;
            }
        }
        r.calculate_bbox();
        r.epsilon *= spectral_norm(&lin);
        let e = r.epsilon;
        r.set_epsilon(e, false);
        if !r.is_empty() {
            r.collider = self.collider.clone();
            if Collider::is_axis_aligned(t) {
                r.collider.transform(t);
            } else {
                let (face_box, _) = r.face_box_morton()?;
                r.collider.update_boxes(&face_box)?;
            }
        }
        Ok(r)
    }

    /// `GetProperty`: the volume (`area` false) or surface area, Kahan-summed over the triangles in order.
    pub(crate) fn get_property(&self, area: bool) -> f64 {
        if self.is_empty() {
            return 0.0;
        }
        let tri_value = |tri: i32| -> R<f64> {
            let v = self.vert_pos.at(self.halfedge.start(3 * tri)?)?;
            let c =
                cross3(self.vert_pos.at(self.halfedge.start(3 * tri + 1)?)? - v, self.vert_pos.at(self.halfedge.start(3 * tri + 2)?)? - v);
            Ok(if area { length3(c) / 2.0 } else { dot3(c, v) / 6.0 })
        };
        let mut value = 0.0f64;
        let mut comp = 0.0f64;
        for tri in 0..self.num_tri() {
            let Ok(t) = i32::try_from(tri) else { return f64::NAN };
            let Ok(v1) = tri_value(t) else { return f64::NAN };
            let s = value + v1;
            comp += (value - s) + v1;
            value = s;
        }
        value + comp
    }

    /// The number of `Decompose()` pieces: vertex components (forward edges joined) that own a triangle.
    pub(crate) fn component_count(&self) -> R<usize> {
        let mut uf = DisjointSets::new(self.num_vert())?;
        for e in 0..ix(self.halfedge.len())? {
            let (s, t) = (self.halfedge.start(e)?, self.halfedge.end(e)?);
            if s < t {
                uf.unite(ux(s)?, ux(t)?)?;
            }
        }
        let mut labels = Vec::new();
        let n = ux(uf.connected_components(&mut labels)?)?;
        if n == 1 {
            return Ok(1);
        }
        let mut has_face = vec![false; n];
        for face in 0..ix(self.num_tri())? {
            let l = labels.at(self.halfedge.start(3 * face)?)?;
            has_face.set(l, true)?;
        }
        Ok(has_face.iter().filter(|&&b| b).count())
    }

    /// `MinGap(other, searchLength)` of two disjoint solids: the smallest triangle-triangle distance among the pairs
    /// whose boxes (the other's grown by `search`) overlap, capped at `search`.
    pub(crate) fn min_gap(&self, other: &Impl, search: f64) -> R<f64> {
        let (mut boxes, _) = other.face_box_morton()?;
        for b in &mut boxes {
            *b = Box3 { min: b.min - V3::splat(search), max: b.max + V3::splat(search) };
        }
        let mut best = f64::INFINITY;
        self.collider.collisions_box(
            false,
            boxes.len(),
            |i| boxes.at(i),
            |to: i32, t: i32| -> R<()> {
                let p = [
                    self.vert_pos.at(self.halfedge.start(3 * t)?)?,
                    self.vert_pos.at(self.halfedge.start(3 * t + 1)?)?,
                    self.vert_pos.at(self.halfedge.start(3 * t + 2)?)?,
                ];
                let q = [
                    other.vert_pos.at(other.halfedge.start(3 * to)?)?,
                    other.vert_pos.at(other.halfedge.start(3 * to + 1)?)?,
                    other.vert_pos.at(other.halfedge.start(3 * to + 2)?)?,
                ];
                best = std_min(best, distance_triangle_triangle_squared(&p, &q));
                Ok(())
            },
        )?;
        Ok(std_min(best, search * search).sqrt())
    }
}

#[inline]
fn clamp01(x: f64) -> f64 {
    la_min(la_max(x, 0.0), 1.0)
}

/// tri_dist.h `EdgeEdgeDist`: the closest points of segments (p, p + a) and (q, q + b).
fn edge_edge_dist(p: V3, a: V3, q: V3, b: V3) -> (V3, V3) {
    let tt = q - p;
    let a_a = dot3(a, a);
    let b_b = dot3(b, b);
    let a_b = dot3(a, b);
    let a_t = dot3(a, tt);
    let b_t = dot3(b, tt);
    let denom = a_a * b_b - a_b * a_b;
    let mut t = if denom != 0.0 { clamp01((a_t * b_b - b_t * a_b) / denom) } else { 0.0 };
    let u;
    if b_b != 0.0 {
        let mut uu = (t * a_b - b_t) / b_b;
        if uu < 0.0 {
            uu = 0.0;
            t = if a_a != 0.0 { clamp01(a_t / a_a) } else { 0.0 };
        } else if uu > 1.0 {
            uu = 1.0;
            t = if a_a != 0.0 { clamp01((a_b + a_t) / a_a) } else { 0.0 };
        }
        u = uu;
    } else {
        u = 0.0;
        t = if a_a != 0.0 { clamp01(a_t / a_a) } else { 0.0 };
    }
    (p + a * t, q + b * u)
}

/// tri_dist.h `DistanceTriangleTriangleSquared`.
pub(crate) fn distance_triangle_triangle_squared(p: &[V3; 3], q: &[V3; 3]) -> f64 {
    let sv = [p[1] - p[0], p[2] - p[1], p[0] - p[2]];
    let tv = [q[1] - q[0], q[2] - q[1], q[0] - q[2]];
    // (fixed-size arrays indexed by constants: in range)
    let mut shown_disjoint = false;
    let mut mindd = f64::MAX;
    let g = |a: &[V3; 3], i: usize| a.get(i % 3).copied().unwrap_or_default();
    for i in 0..3 {
        for j in 0..3 {
            let (cp, cq) = edge_edge_dist(g(p, i), g(&sv, i), g(q, j), g(&tv, j));
            let vv = cq - cp;
            let dd = dot3(vv, vv);
            if dd <= mindd {
                mindd = dd;
                let z = g(p, i + 2) - cp;
                let mut a = dot3(z, vv);
                let z = g(q, j + 2) - cq;
                let mut b = dot3(z, vv);
                if a <= 0.0 && b >= 0.0 {
                    return dot3(vv, vv);
                }
                if a <= 0.0 {
                    a = 0.0;
                } else if b > 0.0 {
                    b = 0.0;
                }
                if (mindd - a + b) > 0.0 {
                    shown_disjoint = true;
                }
            }
        }
    }
    let side = |p: &[V3; 3], q: &[V3; 3], sv: &[V3; 3], flip: bool, shown: &mut bool| -> Option<f64> {
        let [p0, p1, p2] = *p;
        let [q0, q1, q2] = *q;
        let [s0, s1, s2] = *sv;
        let sn = cross3(s0, s1);
        let snl = dot3(sn, sn);
        if snl > 1e-15 {
            let tp = [dot3(p0 - q0, sn), dot3(p0 - q1, sn), dot3(p0 - q2, sn)];
            let mut index: i32 = -1;
            if tp[0] > 0.0 && tp[1] > 0.0 && tp[2] > 0.0 {
                index = if tp[0] < tp[1] { 0 } else { 1 };
                if tp[2] < tp.get(index as usize).copied().unwrap_or(0.0) {
                    index = 2;
                }
            } else if tp[0] < 0.0 && tp[1] < 0.0 && tp[2] < 0.0 {
                index = if tp[0] > tp[1] { 0 } else { 1 };
                if tp[2] > tp.get(index as usize).copied().unwrap_or(0.0) {
                    index = 2;
                }
            }
            if index >= 0 {
                *shown = true;
                let k = index as usize;
                let qi = q.get(k).copied().unwrap_or_default();
                let tk = tp.get(k).copied().unwrap_or(0.0);
                let mut vv = qi - p0;
                let mut z = cross3(sn, s0);
                if dot3(vv, z) > 0.0 {
                    vv = qi - p1;
                    z = cross3(sn, s1);
                    if dot3(vv, z) > 0.0 {
                        vv = qi - p2;
                        z = cross3(sn, s2);
                        if dot3(vv, z) > 0.0 {
                            let moved = qi + sn * tk / snl;
                            let (cp, cq) = if flip { (qi, moved) } else { (moved, qi) };
                            return Some(dot3(cp - cq, cp - cq));
                        }
                    }
                }
            }
        }
        None
    };
    if let Some(d) = side(p, q, &sv, false, &mut shown_disjoint) {
        return d;
    }
    if let Some(d) = side(q, p, &tv, true, &mut shown_disjoint) {
        return d;
    }
    if shown_disjoint { mindd } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_distance() {
        let p = [v3(0.0, 0.0, 0.0), v3(1.0, 0.0, 0.0), v3(0.0, 1.0, 0.0)];
        let q = [v3(0.0, 0.0, 2.0), v3(1.0, 0.0, 2.0), v3(0.0, 1.0, 2.0)];
        assert_eq!(distance_triangle_triangle_squared(&p, &q), 4.0);
    }

    #[test]
    fn axis_aligned() {
        let mut t = crate::mat::IDENTITY;
        assert!(Collider::is_axis_aligned(&t));
        t.0[0] = v3(0.0, 1.0, 0.0);
        t.0[1] = v3(-1.0, 0.0, 0.0);
        assert!(Collider::is_axis_aligned(&t));
        t.0[0] = v3(0.5, 0.5, 0.0);
        assert!(!Collider::is_axis_aligned(&t));
    }
}
