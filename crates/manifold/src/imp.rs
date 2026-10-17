// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! impl.h / impl.cpp / properties.cpp: `Manifold::Impl`, the halfedge mesh, and the parts of it the boolean uses
//! (halfedge creation, coplanar faces, vertex pseudo-normals, bounding box, epsilon, mesh IDs).
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::Status;
use crate::collider::Collider;
use crate::disjoint::DisjointSets;
use crate::halfedge::{Halfedges, TriRef, next_halfedge};
use crate::linalg::{Box3, V3, V4, cross3, dot3, length2_3, max3, min3, normalize3, safe_normalize3, v3};
use crate::util::{Fault, Get, GetMut, Guard, R, by_less, ix, ux};

/// common.h `kPrecision`.
pub(crate) const K_PRECISION: f64 = 1e-12;
/// common.h `kPi`.
pub(crate) const K_PI: f64 = core::f64::consts::PI;

/// `Manifold::Impl::meshIDCounter_` (a process-wide counter starting at 1, as in the C++).
static MESH_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

/// `Impl::ReserveIDs`: the first of `n` new mesh IDs.
pub(crate) fn reserve_ids(n: u32) -> u32 {
    MESH_ID_COUNTER.fetch_add(n, Ordering::Relaxed)
}

/// The counter's current value (`Manifold::Impl::meshIDCounter_` read directly).
pub(crate) fn mesh_id_counter() -> u32 {
    MESH_ID_COUNTER.load(Ordering::Relaxed)
}

/// The 3×4 identity, column-major (`la::identity`).
pub(crate) const IDENTITY34: [f64; 12] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

/// impl.h `Relation`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Relation {
    pub original_id: i32,
    /// column-major 3×4
    pub transform: [f64; 12],
    pub back_side: bool,
    pub has_normals: bool,
}

impl Default for Relation {
    fn default() -> Self {
        Relation { original_id: -1, transform: IDENTITY34, back_side: false, has_normals: false }
    }
}

/// impl.h `MeshRelationD`.
#[derive(Clone, Debug)]
pub(crate) struct MeshRelation {
    pub original_id: i32,
    pub mesh_id_transform: BTreeMap<i32, Relation>,
    pub tri_ref: Vec<TriRef>,
}

impl Default for MeshRelation {
    fn default() -> Self {
        MeshRelation { original_id: -1, mesh_id_transform: BTreeMap::new(), tri_ref: Vec::new() }
    }
}

/// `Manifold::Impl`.
#[derive(Clone, Debug)]
pub(crate) struct Impl {
    pub bbox: Box3,
    pub epsilon: f64,
    pub tolerance: f64,
    pub num_prop: i32,
    pub status: Status,
    pub vert_pos: Vec<V3>,
    pub halfedge: Halfedges,
    pub properties: Vec<f64>,
    pub vert_normal: Vec<V3>,
    pub face_normal: Vec<V3>,
    pub halfedge_tangent: Vec<V4>,
    pub mesh_relation: MeshRelation,
    pub collider: Collider,
}

impl Default for Impl {
    fn default() -> Self {
        Impl {
            bbox: Box3::default(),
            epsilon: -1.0,
            tolerance: -1.0,
            num_prop: 0,
            status: Status::NoError,
            vert_pos: Vec::new(),
            halfedge: Halfedges::default(),
            properties: Vec::new(),
            vert_normal: Vec::new(),
            face_normal: Vec::new(),
            halfedge_tangent: Vec::new(),
            mesh_relation: MeshRelation::default(),
            collider: Collider::default(),
        }
    }
}

/// shared.h `MaxEpsilon`.
fn max_epsilon(min_epsilon: f64, bbox: &Box3) -> f64 {
    let e = crate::util::std_max(min_epsilon, K_PRECISION * bbox.scale());
    if e.is_finite() { e } else { -1.0 }
}

impl Impl {
    /// An empty Impl with an error status (`Impl(); status_ = s`).
    pub(crate) fn with_status(status: Status) -> Impl {
        Impl { status, ..Impl::default() }
    }

    #[inline]
    pub(crate) fn num_vert(&self) -> usize {
        self.vert_pos.len()
    }
    #[inline]
    pub(crate) fn num_tri(&self) -> usize {
        self.halfedge.len() / 3
    }
    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.num_tri() == 0
    }
    #[inline]
    pub(crate) fn num_prop_vert(&self) -> usize {
        if self.num_prop == 0 { self.num_vert() } else { self.properties.len() / ux(self.num_prop).unwrap_or(1).max(1) }
    }

    /// `MakeEmpty(status)`.
    pub(crate) fn make_empty(&mut self, status: Status) {
        self.bbox = Box3::default();
        self.vert_pos.clear();
        self.halfedge.clear();
        self.vert_normal.clear();
        self.face_normal.clear();
        self.halfedge_tangent.clear();
        self.mesh_relation = MeshRelation::default();
        self.collider = Collider::default();
        self.status = status;
    }

    /// `ForVert(halfedge, func)`: orbit the start vertex of `halfedge`.
    pub(crate) fn for_vert(&self, halfedge: i32, mut f: impl FnMut(i32) -> R<()>) -> R<()> {
        let mut guard = Guard::new(self.halfedge.len());
        let mut current = halfedge;
        loop {
            guard.tick()?;
            current = next_halfedge(self.halfedge.pair(current)?);
            f(current)?;
            if current == halfedge {
                return Ok(());
            }
        }
    }

    /// `CalculateBBox`: ignores NaN (removed) vertices; a non-finite box empties the mesh.
    pub(crate) fn calculate_bbox(&mut self) {
        let mut mn = V3::splat(f64::INFINITY);
        for &b in &self.vert_pos {
            mn = if mn.x.is_nan() {
                b
            } else if b.x.is_nan() {
                mn
            } else {
                min3(mn, b)
            };
        }
        let mut mx = V3::splat(f64::NEG_INFINITY);
        for &b in &self.vert_pos {
            mx = if mx.x.is_nan() {
                b
            } else if b.x.is_nan() {
                mx
            } else {
                max3(mx, b)
            };
        }
        self.bbox = Box3 { min: mn, max: mx };
        if !self.bbox.is_finite() {
            self.make_empty(Status::NoError);
        }
    }

    /// `IsFinite`: every vertex finite.
    pub(crate) fn is_finite(&self) -> bool {
        self.vert_pos.iter().all(|v| v.is_finite())
    }

    /// `SetEpsilon(minEpsilon, useSingle)`.
    pub(crate) fn set_epsilon(&mut self, min_epsilon: f64, use_single: bool) {
        self.epsilon = max_epsilon(min_epsilon, &self.bbox);
        let mut min_tol = self.epsilon;
        if use_single {
            min_tol = crate::util::std_max(min_tol, f64::from(f32::EPSILON) * self.bbox.scale());
        }
        self.tolerance = crate::util::std_max(self.tolerance, min_tol);
    }

    /// `IsManifold`: every halfedge paired consistently (removed halfedges are all -1).
    pub(crate) fn is_manifold(&self) -> R<bool> {
        let n = self.halfedge.len();
        if n == 0 {
            return Ok(true);
        }
        if !n.is_multiple_of(3) {
            return Ok(false);
        }
        for e in 0..ix(n)? {
            let start = self.halfedge.start(e)?;
            let end = self.halfedge.end(e)?;
            let pair = self.halfedge.pair(e)?;
            if start == -1 && end == -1 && pair == -1 {
                continue;
            }
            if self.halfedge.start(next_halfedge(e))? == -1 || self.halfedge.start(next_halfedge(next_halfedge(e)))? == -1 {
                return Ok(false);
            }
            if pair == -1 {
                return Ok(false);
            }
            let Ok(pp) = self.halfedge.pair(pair) else { return Ok(false) };
            let Ok(pe) = self.halfedge.end(pair) else { return Ok(false) };
            let Ok(ps) = self.halfedge.start(pair) else { return Ok(false) };
            if !(pp == e && start != end && start == pe && end == ps) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// `CreateHalfedges(triProp, triVert)`: pairs opposite halfedges (sorting by the edge key), removing pairs of
    /// opposed triangles. With `tri_vert` empty, `tri_prop` holds the vertex indices too.
    pub(crate) fn create_halfedges(&mut self, tri_prop: &[[i32; 3]], tri_vert: &[[i32; 3]]) -> R<()> {
        #[derive(Clone, Copy, Default)]
        struct CH {
            start: i32,
            end: i32,
            prop: i32,
        }
        let num_tri = tri_prop.len();
        let num_halfedge = 3 * num_tri;
        let use_prop = tri_vert.is_empty();
        let mut halfedge = vec![CH::default(); num_halfedge];
        let mut edge = vec![0u64; num_halfedge];
        for tri in 0..num_tri {
            let props = tri_prop.atu(tri)?;
            let tv = if use_prop { props } else { tri_vert.atu(tri)? };
            for (i, ((v0, v1), prop)) in [(tv[0], tv[1]), (tv[1], tv[2]), (tv[2], tv[0])].into_iter().zip(props).enumerate() {
                let e = 3 * tri + i;
                halfedge.setu(e, CH { start: v0, end: v1, prop })?;
                // (v0 < v1) << 63 | min << 32 | max; the indices are non-negative here
                let lo = u64::from(v0.min(v1) as u32);
                let hi = u64::from(v0.max(v1) as u32);
                edge.setu(e, (u64::from(v0 < v1) << 63) | (lo << 32) | hi)?;
            }
        }
        // Both C++ paths (sort by key below 2^18 vertices, per-slice sort above) give this order: by key, ties in
        // halfedge order.
        let mut ids: Vec<i32> = (0..ix(num_halfedge)?).collect();
        ids.sort_by_key(|&a| edge.get(a as usize).copied().unwrap_or(u64::MAX));

        let num_edge = ix(num_halfedge / 2)?;
        let mut removed = vec![false; num_halfedge];

        let body = |i: i32, consecutive_start: i32, segment_end: i32, ids: &mut Vec<i32>, removed: &mut Vec<bool>| -> R<i32> {
            let pair0 = ids.at(i)?;
            let h0 = halfedge.at(pair0)?;
            let mut k = consecutive_start + num_edge;
            loop {
                let pair1 = ids.at(k)?;
                let h1 = halfedge.at(pair1)?;
                if h0.start != h1.end || h0.end != h1.start {
                    break;
                }
                if !removed.at(pair1)? && halfedge.at(next_halfedge(pair0))?.end == halfedge.at(next_halfedge(pair1))?.end {
                    removed.set(pair0, true)?;
                    removed.set(pair1, true)?;
                    if i + num_edge != k {
                        // Reorder so that remaining edges pair up, keeping the relative order (triangle id order)
                        let dir = if i + num_edge < k { 1 } else { -1 };
                        let mut a = k;
                        let mut b = k + dir;
                        let mut guard = Guard::new(2 * num_halfedge);
                        loop {
                            loop {
                                guard.tick()?;
                                a -= dir;
                                let in_range = if dir > 0 { a >= i + num_edge } else { a <= i + num_edge };
                                if !(in_range && removed.at(ids.at(a)?)?) {
                                    break;
                                }
                            }
                            let in_range = if dir > 0 { a >= i + num_edge } else { a <= i + num_edge };
                            if !in_range {
                                break;
                            }
                            loop {
                                guard.tick()?;
                                b -= dir;
                                if !(removed.at(ids.at(b)?)? && b != k) {
                                    break;
                                }
                            }
                            let va = ids.at(a)?;
                            ids.set(b, va)?;
                        }
                        ids.set(i + num_edge, pair1)?;
                    }
                    break;
                }
                k += 1;
                if k >= segment_end + num_edge {
                    break;
                }
            }
            if i + 1 == segment_end {
                return Ok(consecutive_start);
            }
            let h1 = halfedge.at(ids.at(i + 1)?)?;
            if h1.start == h0.start && h1.end == h0.end {
                return Ok(consecutive_start);
            }
            Ok(i + 1)
        };

        let mut consecutive_start = 0;
        for i in 0..num_edge {
            consecutive_start = body(i, consecutive_start, num_edge, &mut ids, &mut removed)?;
        }

        self.halfedge.clear();
        self.halfedge.resize_nofill(num_halfedge);
        for i in 0..num_edge {
            let pair0 = ids.at(i)?;
            let pair1 = ids.at(i + num_edge)?;
            if !removed.at(pair0)? {
                let h0 = halfedge.at(pair0)?;
                let h1 = halfedge.at(pair1)?;
                self.halfedge.set_start(pair0, h0.start)?;
                self.halfedge.set_prop(pair0, h0.prop)?;
                self.halfedge.set_pair(pair0, pair1)?;
                self.halfedge.set_start(pair1, h1.start)?;
                self.halfedge.set_prop(pair1, h1.prop)?;
                self.halfedge.set_pair(pair1, pair0)?;
            } else {
                self.halfedge.set(pair0, -1, -1, 0)?;
                self.halfedge.set(pair1, -1, -1, 0)?;
            }
        }
        Ok(())
    }

    /// `SetNormalsAndCoplanar`: face normals, then coplanar groups flooded from the largest triangles, then vertex
    /// normals.
    pub(crate) fn set_normals_and_coplanar(&mut self) -> R<()> {
        self.set_face_normals_and_coplanar()?;
        self.calculate_vert_normals()
    }

    /// `SetNormalsAndCoplanar` without the final `CalculateVertNormals` (for callers that recompute them before any
    /// use).
    pub(crate) fn set_face_normals_and_coplanar(&mut self) -> R<()> {
        let num_tri = self.num_tri();
        self.face_normal.resize(num_tri, V3::default());
        #[derive(Clone, Copy)]
        struct TP {
            area2: f64,
            tri: i32,
        }
        let mut tri_priority: Vec<TP> = Vec::with_capacity(num_tri);
        for tri in 0..ix(num_tri)? {
            self.mesh_relation.tri_ref.at_mut(tri)?.coplanar_id = -1;
            if self.halfedge.start(3 * tri)? < 0 {
                tri_priority.push(TP { area2: 0.0, tri });
                continue;
            }
            let v = self.vert_pos.at(self.halfedge.start(3 * tri)?)?;
            let n = cross3(self.vert_pos.at(self.halfedge.end(3 * tri)?)? - v, self.vert_pos.at(self.halfedge.end(3 * tri + 1)?)? - v);
            let mut fnrm = normalize3(n);
            if fnrm.x.is_nan() {
                fnrm = v3(0.0, 0.0, 1.0);
            }
            self.face_normal.set(tri, fnrm)?;
            tri_priority.push(TP { area2: length2_3(n), tri });
        }
        tri_priority.sort_by(by_less(|a: &TP, b: &TP| a.area2 > b.area2));

        let mut interior: Vec<i32> = Vec::new();
        for tp in &tri_priority {
            if self.mesh_relation.tri_ref.at(tp.tri)?.coplanar_id >= 0 {
                continue;
            }
            self.mesh_relation.tri_ref.at_mut(tp.tri)?.coplanar_id = tp.tri;
            if self.halfedge.start(3 * tp.tri)? < 0 {
                continue;
            }
            let base = self.vert_pos.at(self.halfedge.start(3 * tp.tri)?)?;
            let normal = self.face_normal.at(tp.tri)?;
            interior.clear();
            interior.extend_from_slice(&[3 * tp.tri, 3 * tp.tri + 1, 3 * tp.tri + 2]);
            let mut guard = Guard::new(4 * self.halfedge.len());
            while let Some(&back) = interior.last() {
                guard.tick()?;
                let h = next_halfedge(self.halfedge.pair(back)?);
                interior.pop();
                if self.mesh_relation.tri_ref.at(h / 3)?.coplanar_id >= 0 {
                    continue;
                }
                let v = self.vert_pos.at(self.halfedge.end(h)?)?;
                if dot3(v - base, normal).abs() < self.tolerance {
                    let tri = h / 3;
                    self.mesh_relation.tri_ref.at_mut(tri)?.coplanar_id = tp.tri;
                    self.face_normal.set(tri, normal)?;
                    let keep = match interior.last() {
                        None => true,
                        Some(&b) => h != self.halfedge.pair(b)?,
                    };
                    if keep {
                        interior.push(h);
                    } else {
                        interior.pop();
                    }
                    interior.push(next_halfedge(h));
                }
            }
        }
        Ok(())
    }

    /// `CalculateVertNormals`: angle-weighted pseudo-normals, orbiting from each vertex's first halfedge.
    pub(crate) fn calculate_vert_normals(&mut self) -> R<()> {
        let nv = self.num_vert();
        self.vert_normal.resize(nv, V3::default());
        let mut first = vec![i32::MAX; nv];
        for i in 0..ix(self.halfedge.len())? {
            let v = self.halfedge.start(i)?;
            if v < 0 {
                continue;
            }
            let f = first.at_mut(v)?;
            if *f > i {
                *f = i;
            }
        }
        for vert in 0..nv {
            let first_edge = first.atu(vert)?;
            if first_edge == i32::MAX {
                self.vert_normal.setu(vert, V3::splat(0.0))?;
                continue;
            }
            let mut normal = V3::splat(0.0);
            self.for_vert(first_edge, |edge| {
                let t0 = self.halfedge.start(edge)?;
                let t1 = self.halfedge.end(edge)?;
                let t2 = self.halfedge.end(next_halfedge(edge))?;
                let p0 = self.vert_pos.at(t0)?;
                let p1 = self.vert_pos.at(t1)?;
                let p2 = self.vert_pos.at(t2)?;
                let curr = normalize3(p1 - p0);
                let prev = normalize3(p0 - p2);
                if !curr.x.is_finite() || !prev.x.is_finite() {
                    return Ok(());
                }
                let d = -dot3(prev, curr);
                let phi = if d >= 1.0 {
                    0.0
                } else if d <= -1.0 {
                    K_PI
                } else {
                    crate::math::acos(d)
                };
                normal += phi * self.face_normal.at(edge / 3)?;
                Ok(())
            })?;
            self.vert_normal.setu(vert, safe_normalize3(normal))?;
        }
        Ok(())
    }

    /// `RemoveUnreferencedVerts`: unreferenced vertices become NaN (sorted away by SortGeometry).
    pub(crate) fn remove_unreferenced_verts(&mut self) -> R<()> {
        let nv = self.num_vert();
        let mut keep = vec![false; nv];
        for &s in &self.halfedge.start {
            if s >= 0 {
                keep.set(s, true)?;
            }
        }
        for (v, k) in keep.iter().enumerate() {
            if !k {
                self.vert_pos.setu(v, V3::splat(f64::NAN))?;
            }
        }
        Ok(())
    }

    /// `DedupePropVerts`: property vertices that are exactly equal across an edge of one mesh are merged.
    pub(crate) fn dedupe_prop_verts(&mut self) -> R<()> {
        let num_prop = ux(self.num_prop)?;
        if num_prop == 0 {
            return Ok(());
        }
        let n = self.halfedge.len();
        let mut vert2vert: Vec<(i32, i32)> = vec![(-1, -1); n];
        for e in 0..ix(n)? {
            let pair = self.halfedge.pair(e)?;
            if pair < 0 {
                continue;
            }
            if self.mesh_relation.tri_ref.at(e / 3)?.mesh_id != self.mesh_relation.tri_ref.at(pair / 3)?.mesh_id {
                continue;
            }
            let p0 = ux(self.halfedge.prop(e)?)?;
            let p1 = ux(self.halfedge.prop(next_halfedge(pair))?)?;
            let mut equal = true;
            for p in 0..num_prop {
                if self.properties.atu(num_prop * p0 + p)? != self.properties.atu(num_prop * p1 + p)? {
                    equal = false;
                    break;
                }
            }
            if equal {
                vert2vert.set(e, (p0 as i32, p1 as i32))?;
            }
        }
        let num_prop_vert = self.num_prop_vert();
        let mut uf = DisjointSets::new(num_prop_vert)?;
        for &(a, b) in &vert2vert {
            if a == -1 || b == -1 {
                continue;
            }
            uf.unite(ux(a)?, ux(b)?)?;
        }
        let mut labels = Vec::new();
        let num_labels = uf.connected_components(&mut labels)?;
        let mut label2vert = vec![0i32; ux(num_labels)?];
        for v in 0..num_prop_vert {
            label2vert.set(labels.atu(v)?, ix(v)?)?;
        }
        for e in 0..ix(n)? {
            let p = self.halfedge.prop(e)?;
            let nv = label2vert.at(labels.at(p)?)?;
            self.halfedge.set_prop(e, nv)?;
        }
        Ok(())
    }

    /// impl.h `AllHaveNormals`: every relation records normals (false when there are none).
    pub(crate) fn all_have_normals(&self) -> bool {
        !self.mesh_relation.mesh_id_transform.is_empty() && self.mesh_relation.mesh_id_transform.values().all(|r| r.has_normals)
    }

    /// `InitializeOriginal`: this mesh becomes an original with one new mesh ID (every triangle refers to it, the
    /// coplanar IDs stay).
    pub(crate) fn initialize_original(&mut self) {
        let mesh_id = reserve_ids(1) as i32;
        self.mesh_relation.original_id = mesh_id;
        let nt = self.num_tri();
        self.mesh_relation.tri_ref.resize(nt, TriRef::default());
        for r in &mut self.mesh_relation.tri_ref {
            *r = TriRef { mesh_id, original_id: mesh_id, face_id: -1, coplanar_id: r.coplanar_id };
        }
        let had_normals = self.all_have_normals();
        self.mesh_relation.mesh_id_transform.clear();
        self.mesh_relation
            .mesh_id_transform
            .insert(mesh_id, Relation { original_id: mesh_id, transform: IDENTITY34, back_side: false, has_normals: had_normals });
    }

    /// `IncrementMeshIDs`: new instance IDs for every mesh in this Impl.
    pub(crate) fn increment_mesh_ids(&mut self) -> R<()> {
        let old = core::mem::take(&mut self.mesh_relation.mesh_id_transform);
        let num = u32::try_from(old.len()).map_err(|_| Fault::TooLarge)?;
        let mut next = reserve_ids(num) as i32;
        let mut old2new: crate::util::FxMap<i32, i32> = crate::util::FxMap::default();
        for (k, rel) in old {
            old2new.insert(k, next);
            self.mesh_relation.mesh_id_transform.insert(next, rel);
            next = next.wrapping_add(1);
        }
        let nt = self.num_tri();
        for r in self.mesh_relation.tri_ref.iter_mut().take(nt) {
            // HashTableD::operator[] on a missing key lands on an open slot, whose value is 0
            r.mesh_id = old2new.get(&r.mesh_id).copied().unwrap_or(0);
        }
        Ok(())
    }
}
