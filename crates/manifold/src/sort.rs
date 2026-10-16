// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! sort.cpp: vertices and faces sorted by Morton code (removed ones last, then dropped), properties compacted,
//! halfedges reordered for determinism, and the face collider built.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use crate::collider::{Collider, morton_code};
use crate::halfedge::{Halfedge, Halfedges};
use crate::imp::Impl;
use crate::linalg::{Box3, V3};
use crate::util::{Get, GetMut, R, ix, permute, ux};

const K_NO_CODE: u32 = 0xFFFF_FFFF;

/// sort.cpp `MortonCode`: removed (NaN) vertices sort last.
#[inline]
fn morton(p: V3, bbox: &Box3) -> u32 {
    if p.x.is_nan() { K_NO_CODE } else { morton_code(p, bbox) }
}

impl Impl {
    /// `SortGeometry`.
    pub(crate) fn sort_geometry(&mut self) -> R<()> {
        if self.halfedge.is_empty() {
            self.collider = Collider::default();
            return Ok(());
        }
        self.sort_verts()?;
        let (mut face_box, mut face_morton) = self.face_box_morton()?;
        self.sort_faces(&mut face_box, &mut face_morton)?;
        if self.halfedge.is_empty() {
            self.collider = Collider::default();
            return Ok(());
        }
        self.collider = Collider::new(&face_box, &face_morton)?;
        self.bbox = self.collider.bounding_box();
        self.compact_props()
    }

    /// `SortVerts`.
    fn sort_verts(&mut self) -> R<()> {
        let num_vert = self.num_vert();
        let codes: Vec<u32> = self.vert_pos.iter().map(|&p| morton(p, &self.bbox)).collect();
        let mut new2old: Vec<i32> = (0..ix(num_vert)?).collect();
        new2old.sort_by_key(|&v| codes.get(v as usize).copied().unwrap_or(K_NO_CODE));
        self.reindex_verts(&new2old, num_vert)?;
        let new_num = new2old.partition_point(|&v| codes.get(v as usize).copied().unwrap_or(K_NO_CODE) < K_NO_CODE);
        new2old.truncate(new_num);
        permute(&mut self.vert_pos, &new2old)?;
        if self.vert_normal.len() == num_vert {
            permute(&mut self.vert_normal, &new2old)?;
        }
        Ok(())
    }

    /// `ReindexVerts`.
    fn reindex_verts(&mut self, new2old: &[i32], old_num_vert: usize) -> R<()> {
        let mut old2new = vec![0i32; old_num_vert];
        for (i, &o) in new2old.iter().enumerate() {
            old2new.set(o, i as i32)?;
        }
        let has_prop = self.num_prop > 0;
        for idx in 0..self.halfedge.len() {
            let s = self.halfedge.start.atu(idx)?;
            if s < 0 {
                continue;
            }
            let ns = old2new.at(s)?;
            self.halfedge.start.setu(idx, ns)?;
            if !has_prop {
                self.halfedge.prop.setu(idx, ns)?;
            }
        }
        Ok(())
    }

    /// `CompactProps`: unreferenced property vertices removed.
    fn compact_props(&mut self) -> R<()> {
        if self.num_prop == 0 {
            return Ok(());
        }
        let np = ux(self.num_prop)?;
        let num_verts = self.properties.len() / np;
        let mut keep = vec![0i32; num_verts];
        for &p in &self.halfedge.prop {
            keep.set(p, 1)?;
        }
        let mut old2new = vec![0i32; num_verts + 1];
        let mut acc = 0i32;
        for (i, &k) in keep.iter().enumerate() {
            acc += k;
            old2new.setu(i + 1, acc)?;
        }
        let old_prop = core::mem::take(&mut self.properties);
        let num_new = ux(old2new.atu(num_verts)?)?;
        self.properties = vec![0.0; np * num_new];
        for old in 0..num_verts {
            if keep.atu(old)? == 0 {
                continue;
            }
            let n = ux(old2new.atu(old)?)?;
            for p in 0..np {
                self.properties.setu(n * np + p, old_prop.atu(old * np + p)?)?;
            }
        }
        for idx in 0..self.halfedge.len() {
            let p = self.halfedge.prop.atu(idx)?;
            self.halfedge.prop.setu(idx, old2new.at(p)?)?;
        }
        Ok(())
    }

    /// `GetFaceBoxMorton`: removed faces (pair < 0) get the no-code and sort last.
    pub(crate) fn face_box_morton(&self) -> R<(Vec<Box3>, Vec<u32>)> {
        let nt = self.num_tri();
        let mut face_box = vec![Box3::default(); nt];
        let mut face_morton = vec![0u32; nt];
        for face in 0..nt {
            let f = face as i32;
            if self.halfedge.pair(3 * f)? < 0 {
                face_morton.setu(face, K_NO_CODE)?;
                continue;
            }
            let mut center = V3::splat(0.0);
            let fb = face_box.atu_mut(face)?;
            for i in 0..3 {
                let pos = self.vert_pos.at(self.halfedge.start(3 * f + i)?)?;
                center += pos;
                fb.union_pt(pos);
            }
            center = center / 3.0;
            face_morton.setu(face, morton(center, &self.bbox))?;
        }
        Ok((face_box, face_morton))
    }

    /// `SortFaces`.
    fn sort_faces(&mut self, face_box: &mut Vec<Box3>, face_morton: &mut Vec<u32>) -> R<()> {
        let codes = face_morton.clone();
        let mut new2old: Vec<i32> = (0..ix(self.num_tri())?).collect();
        new2old.sort_by_key(|&f| codes.get(f as usize).copied().unwrap_or(K_NO_CODE));
        let new_num = new2old.partition_point(|&f| codes.get(f as usize).copied().unwrap_or(K_NO_CODE) < K_NO_CODE);
        new2old.truncate(new_num);
        permute(face_morton, &new2old)?;
        permute(face_box, &new2old)?;
        self.gather_faces(&new2old)
    }

    /// `GatherFaces(faceNew2Old)`.
    fn gather_faces(&mut self, new2old: &[i32]) -> R<()> {
        let num_tri = new2old.len();
        let old_num_tri = self.num_tri();
        if self.mesh_relation.tri_ref.len() == old_num_tri {
            permute(&mut self.mesh_relation.tri_ref, new2old)?;
        }
        if self.face_normal.len() == old_num_tri {
            permute(&mut self.face_normal, new2old)?;
        }
        let old = core::mem::take(&mut self.halfedge);
        let old_tangent = core::mem::take(&mut self.halfedge_tangent);
        // faces dropped here are never referenced by kept ones (their neighbours were re-paired when they collapsed)
        let mut old2new = vec![0i32; old.len() / 3];
        for (i, &o) in new2old.iter().enumerate() {
            old2new.set(o, i as i32)?;
        }
        let mut he = Halfedges::default();
        he.resize_nofill(3 * num_tri);
        if !old_tangent.is_empty() {
            self.halfedge_tangent = vec![Default::default(); 3 * num_tri];
        }
        for (new_face, &old_face) in new2old.iter().enumerate() {
            for i in 0..3 {
                let old_edge = 3 * old_face + i;
                let e: Halfedge = old.get(old_edge)?;
                let paired_face = e.paired_halfedge / 3;
                let offset = e.paired_halfedge - 3 * paired_face;
                let pair = 3 * old2new.at(paired_face)? + offset;
                let new_edge = 3 * new_face as i32 + i;
                he.set(new_edge, e.start_vert, pair, e.prop_vert)?;
                if !old_tangent.is_empty() {
                    self.halfedge_tangent.set(new_edge, old_tangent.at(old_edge)?)?;
                }
            }
        }
        self.halfedge = he;
        Ok(())
    }

    /// `ReorderHalfedges`: each face starts at its smallest vertex; pairs fixed up after.
    pub(crate) fn reorder_halfedges(&mut self) -> R<()> {
        let nt = self.num_tri();
        for tri in 0..nt {
            let t = tri as i32;
            let face = [self.halfedge.get(3 * t)?, self.halfedge.get(3 * t + 1)?, self.halfedge.get(3 * t + 2)?];
            if face[0].start_vert < 0 {
                continue;
            }
            let mut index = 0usize;
            for i in 1..3 {
                if face.atu(i)?.start_vert < face.atu(index)?.start_vert {
                    index = i;
                }
            }
            for i in 0..3 {
                let f = face.atu((index + i) % 3)?;
                self.halfedge.set(3 * t + i as i32, f.start_vert, f.paired_halfedge, f.prop_vert)?;
            }
        }
        for tri in 0..nt {
            let t = tri as i32;
            for i in 0..3 {
                let curr = 3 * t + i;
                let start = self.halfedge.start(curr)?;
                if start < 0 {
                    break;
                }
                let opposite = self.halfedge.pair(curr)? / 3;
                let mut index = -1;
                for j in 0..3 {
                    if start == self.halfedge.end(opposite * 3 + j)? {
                        index = j;
                    }
                }
                self.halfedge.set_pair(curr, opposite * 3 + index)?;
            }
        }
        Ok(())
    }
}
