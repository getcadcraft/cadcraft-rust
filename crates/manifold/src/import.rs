// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! impl.h `Impl(const MeshGLP&)`: the MeshGL → Manifold constructor (the input path: every count and index from the
//! caller is checked here, with the strict lint set: no unchecked arithmetic, no lossy casts).
//!
//! The checks run in the C++ order, so the same malformed mesh reports the same status. Where the C++ has undefined
//! behaviour on malformed run data (a run index past the triangles, more runs than run boundaries), this port reports
//! `RunIndexWrongLength`.

use crate::Mesh;
use crate::Status;
use crate::halfedge::TriRef;
use crate::imp::{IDENTITY34, Impl, Relation, reserve_ids};
use crate::linalg::{V3, V4};
use crate::util::{Fault, Get, GetMut};

impl Impl {
    /// `Impl(meshGL)`; the status says whether it worked (an empty Impl otherwise).
    pub(crate) fn from_mesh(m: &Mesh) -> Impl {
        let mut imp = Impl::default();
        if let Err(f) = imp.build_from_mesh(m) {
            imp.make_empty(f.status());
        }
        imp
    }

    fn build_from_mesh(&mut self, m: &Mesh) -> Result<(), Fault> {
        // numProp 0: the C++ divides by zero here (the wasm build traps); report missing positions instead
        if m.num_prop == 0 {
            self.make_empty(Status::MissingPositionProperties);
            return Ok(());
        }
        let num_vert = m.num_vert();
        let num_tri = m.num_tri();
        if num_vert == 0 && num_tri == 0 {
            self.make_empty(Status::NoError);
            return Ok(());
        }
        if num_vert < 4 || num_tri < 4 {
            self.make_empty(Status::NotManifold);
            return Ok(());
        }
        if m.num_prop < 3 {
            self.make_empty(Status::MissingPositionProperties);
            return Ok(());
        }
        if num_tri > crate::MAX_TRIS || num_vert > crate::MAX_VERTS || m.num_prop > crate::MAX_NUM_PROP {
            self.make_empty(Status::ResultTooLarge);
            return Ok(());
        }
        if m.merge_from_vert.len() != m.merge_to_vert.len() {
            self.make_empty(Status::MergeVectorsDifferentLengths);
            return Ok(());
        }
        if !m.run_transform.is_empty() && m.run_original_id.len().checked_mul(12) != Some(m.run_transform.len()) {
            self.make_empty(Status::TransformWrongLength);
            return Ok(());
        }
        let nrun = m.run_original_id.len();
        if nrun != 0 && !m.run_index.is_empty() && nrun.checked_add(1) != Some(m.run_index.len()) && nrun != m.run_index.len() {
            self.make_empty(Status::RunIndexWrongLength);
            return Ok(());
        }
        if !m.face_id.is_empty() && m.face_id.len() != num_tri {
            self.make_empty(Status::FaceIDWrongLength);
            return Ok(());
        }
        if !m.vert_properties.iter().all(|x| x.is_finite()) {
            self.make_empty(Status::NonFiniteVertex);
            return Ok(());
        }
        if !m.run_transform.iter().all(|x| x.is_finite()) || !m.halfedge_tangent.iter().all(|x| x.is_finite()) {
            self.make_empty(Status::InvalidConstruction);
            return Ok(());
        }

        let mut prop2vert: Vec<i32> = Vec::new();
        if !m.merge_from_vert.is_empty() {
            prop2vert = (0..i32::try_from(num_vert).map_err(|_| Fault::TooLarge)?).collect();
            for (&from, &to) in m.merge_from_vert.iter().zip(m.merge_to_vert.iter()) {
                let (Ok(from), Ok(to)) = (usize::try_from(from), i32::try_from(to)) else {
                    self.make_empty(Status::MergeIndexOutOfBounds);
                    return Ok(());
                };
                if from >= num_vert || usize::try_from(to).map_or(true, |t| t >= num_vert) {
                    self.make_empty(Status::MergeIndexOutOfBounds);
                    return Ok(());
                }
                prop2vert.setu(from, to)?;
            }
        }

        let stride = usize::try_from(m.num_prop).map_err(|_| Fault::TooLarge)?;
        let num_extra = stride.checked_sub(3).ok_or(Fault::Index)?;
        self.num_prop = i32::try_from(num_extra).map_err(|_| Fault::TooLarge)?;
        self.properties = Vec::with_capacity(num_vert.checked_mul(num_extra).ok_or(Fault::TooLarge)?);
        self.tolerance = f64::from(m.tolerance);
        self.vert_pos = Vec::with_capacity(num_vert);
        for i in 0..num_vert {
            let o = i.checked_mul(stride).ok_or(Fault::TooLarge)?;
            let at = |k: usize| -> Result<f64, Fault> { Ok(f64::from(m.vert_properties.atu(o.checked_add(k).ok_or(Fault::TooLarge)?)?)) };
            self.vert_pos.push(V3 { x: at(0)?, y: at(1)?, z: at(2)? });
            for j in 0..num_extra {
                self.properties.push(at(j.checked_add(3).ok_or(Fault::TooLarge)?)?);
            }
        }

        let nt = m.halfedge_tangent.len() / 4;
        self.halfedge_tangent = Vec::with_capacity(nt);
        let (chunks, _) = m.halfedge_tangent.as_chunks::<4>();
        for &[x, y, z, w] in chunks {
            self.halfedge_tangent.push(V4 { x: f64::from(x), y: f64::from(y), z: f64::from(z), w: f64::from(w) });
        }

        // triRef: the C++ leaves triangles outside every run uninitialised; here they start zeroed
        let mut tri_ref = vec![TriRef::default(); num_tri];

        let run_end = u32::try_from(m.tri_verts.len()).map_err(|_| Fault::TooLarge)?;
        let mut run_index = m.run_index.clone();
        if run_index.is_empty() {
            run_index = vec![0, run_end];
        } else if run_index.len() == nrun || run_index.len() == 1 {
            run_index.push(run_end);
        }

        let start_id = reserve_ids(u32::try_from(nrun.max(1)).map_err(|_| Fault::TooLarge)?);
        let mut run_original_id = m.run_original_id.clone();
        if run_original_id.is_empty() {
            run_original_id.push(start_id);
        }
        for (i, &orig) in run_original_id.iter().enumerate() {
            let mesh_id = start_id.wrapping_add(u32::try_from(i).map_err(|_| Fault::TooLarge)?).cast_signed();
            let original_id = orig.cast_signed();
            let backside = m.run_flags.get(i).is_some_and(|f| f & 1 != 0);
            let run_has_n = m.run_flags.get(i).is_some_and(|f| f & 2 != 0) && num_extra >= 3;
            let (Some(&r0), Some(&r1)) = (run_index.get(i), run_index.get(i.checked_add(1).ok_or(Fault::TooLarge)?)) else {
                self.make_empty(Status::RunIndexWrongLength);
                return Ok(());
            };
            let (t0, t1) = (usize::try_from(r0 / 3).map_err(|_| Fault::TooLarge)?, usize::try_from(r1 / 3).map_err(|_| Fault::TooLarge)?);
            if t1 > num_tri {
                self.make_empty(Status::RunIndexWrongLength);
                return Ok(());
            }
            for tri in t0..t1 {
                let face_id = if m.face_id.is_empty() { -1 } else { m.face_id.atu(tri)?.cast_signed() };
                let coplanar = i32::try_from(tri).map_err(|_| Fault::TooLarge)?;
                tri_ref.setu(tri, TriRef { mesh_id, original_id, face_id, coplanar_id: coplanar })?;
            }
            let rel = if m.run_transform.is_empty() {
                Relation { original_id, transform: IDENTITY34, back_side: false, has_normals: run_has_n }
            } else {
                let mut t = [0.0f64; 12];
                let base = i.checked_mul(12).ok_or(Fault::TooLarge)?;
                for (k, tk) in t.iter_mut().enumerate() {
                    *tk = f64::from(m.run_transform.atu(base.checked_add(k).ok_or(Fault::TooLarge)?)?);
                }
                Relation { original_id, transform: t, back_side: backside, has_normals: run_has_n }
            };
            self.mesh_relation.mesh_id_transform.insert(mesh_id, rel);
        }

        let needs_prop_map = num_extra > 0 && !prop2vert.is_empty();
        let mut tri_prop: Vec<[i32; 3]> = Vec::with_capacity(num_tri);
        let mut tri_vert: Vec<[i32; 3]> = Vec::new();
        if needs_prop_map {
            tri_vert.reserve(num_tri);
        }
        self.mesh_relation.tri_ref.reserve(num_tri);
        for i in 0..num_tri {
            let mut tp = [0i32; 3];
            let mut tv = [0i32; 3];
            for j in 0..3 {
                let vert = m.tri_verts.atu(i.checked_mul(3).and_then(|x| x.checked_add(j)).ok_or(Fault::TooLarge)?)?;
                let Ok(vu) = usize::try_from(vert) else {
                    self.make_empty(Status::VertexOutOfBounds);
                    return Ok(());
                };
                if vu >= num_vert {
                    self.make_empty(Status::VertexOutOfBounds);
                    return Ok(());
                }
                let v = i32::try_from(vert).map_err(|_| Fault::TooLarge)?;
                if let (Some(p), Some(q)) = (tp.get_mut(j), tv.get_mut(j)) {
                    *p = v;
                    *q = if prop2vert.is_empty() { v } else { prop2vert.atu(vu)? };
                }
            }
            if tv[0] != tv[1] && tv[1] != tv[2] && tv[2] != tv[0] {
                if needs_prop_map {
                    tri_prop.push(tp);
                    tri_vert.push(tv);
                } else {
                    tri_prop.push(tv);
                }
                self.mesh_relation.tri_ref.push(tri_ref.atu(i)?);
            }
        }

        self.create_halfedges(&tri_prop, &tri_vert)?;
        if !self.is_manifold()? {
            self.make_empty(Status::NotManifold);
            return Ok(());
        }
        self.calculate_bbox();
        self.set_epsilon(-1.0, true);
        self.cleanup_topology()?;
        self.dedupe_prop_verts()?;
        // RemoveDegenerates ends with CalculateVertNormals whenever the mesh has halfedges, and nothing in between
        // reads the vertex normals (DedupeEdge only copies them for new vertices, which are recomputed too): the C++
        // computes them twice, here once.
        if self.halfedge.is_empty() {
            self.set_normals_and_coplanar()?;
        } else {
            self.set_face_normals_and_coplanar()?;
        }
        self.remove_degenerates(0)?;
        self.remove_unreferenced_verts()?;
        self.sort_geometry()?;
        if !self.is_finite() {
            self.make_empty(Status::NonFiniteVertex);
            return Ok(());
        }
        self.mesh_relation.original_id = -1;
        Ok(())
    }
}
