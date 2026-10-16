// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! impl.h `GetMeshGLImpl<float, uint32_t>` (`Manifold::GetMeshGL`): triangles sorted into runs by (originalID,
//! meshID), vertices duplicated where their properties differ, everything narrowed to 32-bit floats. Output path:
//! strict lints.

use std::collections::BTreeMap;

use crate::Mesh;
use crate::imp::{Impl, Relation};
use crate::linalg::safe_normalize3;
use crate::util::{Fault, Get, GetMut, R, by_less};

/// f64 → f32 rounding to nearest (the C++ implicit narrowing).
#[inline]
#[allow(clippy::cast_possible_truncation)]
fn f32_of(x: f64) -> f32 {
    x as f32
}

pub(crate) fn get_mesh(imp: &Impl) -> R<Mesh> {
    let all_have_normals =
        !imp.mesh_relation.mesh_id_transform.is_empty() && imp.mesh_relation.mesh_id_transform.values().all(|r| r.has_normals);
    let normal_idx: i32 = if all_have_normals { 0 } else { -1 };

    let num_prop = usize::try_from(imp.num_prop).map_err(|_| Fault::Index)?;
    let num_vert = imp.num_prop_vert();
    let num_tri = imp.num_tri();
    let is_original = imp.mesh_relation.original_id >= 0;
    let update_normals = !is_original && normal_idx >= 0;

    let mut out =
        Mesh { num_prop: u32::try_from(num_prop.checked_add(3).ok_or(Fault::TooLarge)?).map_err(|_| Fault::TooLarge)?, ..Mesh::default() };
    out.tolerance = f32_of(imp.tolerance);
    // tolerance = max(tolerance, float(FLT_EPSILON * bBox.Scale()))
    let ft = f32_of(f64::from(f32::EPSILON) * imp.bbox.scale());
    if out.tolerance < ft {
        out.tolerance = ft;
    }
    out.tri_verts = vec![0; num_tri.checked_mul(3).ok_or(Fault::TooLarge)?];

    out.halfedge_tangent = Vec::with_capacity(imp.halfedge_tangent.len().saturating_mul(4));
    for t in &imp.halfedge_tangent {
        out.halfedge_tangent.extend([f32_of(t.x), f32_of(t.y), f32_of(t.z), f32_of(t.w)]);
    }

    out.face_id = vec![0; num_tri];
    let mut new2old: Vec<usize> = (0..num_tri).collect();
    let tri_ref = &imp.mesh_relation.tri_ref;
    if !is_original {
        new2old.sort_by(by_less(|&a: &usize, &b: &usize| {
            let (ra, rb) = (tri_ref.get(a).copied().unwrap_or_default(), tri_ref.get(b).copied().unwrap_or_default());
            if ra.original_id == rb.original_id { ra.mesh_id < rb.mesh_id } else { ra.original_id < rb.original_id }
        }));
    }

    let add_run = |out: &mut Mesh, tri: usize, rel: &Relation| -> R<()> {
        out.run_index.push(u32::try_from(tri.checked_mul(3).ok_or(Fault::TooLarge)?).map_err(|_| Fault::TooLarge)?);
        out.run_original_id.push(rel.original_id.cast_unsigned());
        out.run_flags.push(u8::from(rel.back_side) | (u8::from(rel.has_normals) << 1));
        if !is_original {
            for v in rel.transform {
                out.run_transform.push(f32_of(v));
            }
        }
        Ok(())
    };

    let mut transforms: BTreeMap<i32, Relation> = imp.mesh_relation.mesh_id_transform.clone();
    let mut last_id: Option<i32> = None;
    for (tri, &old) in new2old.iter().enumerate() {
        let r = tri_ref.atu(old)?;
        out.face_id.setu(tri, if r.face_id >= 0 { r.face_id } else { r.coplanar_id }.cast_unsigned())?;
        for i in 0..3usize {
            let he =
                i32::try_from(old.checked_mul(3).and_then(|x| x.checked_add(i)).ok_or(Fault::TooLarge)?).map_err(|_| Fault::TooLarge)?;
            let s = imp.halfedge.start(he)?;
            out.tri_verts.setu(tri.checked_mul(3).and_then(|x| x.checked_add(i)).ok_or(Fault::TooLarge)?, s.cast_unsigned())?;
        }
        if last_id != Some(r.mesh_id) {
            let rel = transforms.get(&r.mesh_id).copied().unwrap_or_default();
            add_run(&mut out, tri, &rel)?;
            transforms.remove(&r.mesh_id);
            last_id = Some(r.mesh_id);
        }
    }
    // runs for originals that did not contribute any faces
    for rel in transforms.values() {
        add_run(&mut out, num_tri, rel)?;
    }
    out.run_index.push(u32::try_from(num_tri.checked_mul(3).ok_or(Fault::TooLarge)?).map_err(|_| Fault::TooLarge)?);

    if num_prop == 0 {
        out.vert_properties = Vec::with_capacity(num_vert.saturating_mul(3));
        for i in 0..num_vert {
            let v = imp.vert_pos.atu(i)?;
            out.vert_properties.extend([f32_of(v.x), f32_of(v.y), f32_of(v.z)]);
        }
        return Ok(out);
    }

    // Duplicate verts with different props
    let stride = num_prop.checked_add(3).ok_or(Fault::TooLarge)?;
    let mut vert2idx: Vec<i64> = vec![-1; imp.num_vert()];
    let mut vert_prop_pair: Vec<Vec<(i32, u32)>> = vec![Vec::new(); imp.num_vert()];
    out.vert_properties = Vec::with_capacity(num_vert.saturating_mul(stride));
    let num_runs = out.run_original_id.len();
    for run in 0..num_runs {
        let t0 = usize::try_from(out.run_index.atu(run)? / 3).map_err(|_| Fault::TooLarge)?;
        let t1 = usize::try_from(out.run_index.atu(run.checked_add(1).ok_or(Fault::TooLarge)?)? / 3).map_err(|_| Fault::TooLarge)?;
        for tri in t0..t1 {
            for i in 0..3usize {
                let slot = tri.checked_mul(3).and_then(|x| x.checked_add(i)).ok_or(Fault::TooLarge)?;
                let he = i32::try_from(new2old.atu(tri)?.checked_mul(3).and_then(|x| x.checked_add(i)).ok_or(Fault::TooLarge)?)
                    .map_err(|_| Fault::TooLarge)?;
                let prop = imp.halfedge.prop(he)?;
                let vert = usize::try_from(out.tri_verts.atu(slot)?).map_err(|_| Fault::Index)?;
                let bin = vert_prop_pair.get_mut(vert).ok_or(Fault::Index)?;
                if let Some(&(_, idx)) = bin.iter().find(|b| b.0 == prop) {
                    out.tri_verts.setu(slot, idx)?;
                    continue;
                }
                let idx = u32::try_from(out.vert_properties.len().checked_div(stride).ok_or(Fault::Index)?).map_err(|_| Fault::TooLarge)?;
                out.tri_verts.setu(slot, idx)?;
                bin.push((prop, idx));
                let p = imp.vert_pos.atu(vert)?;
                out.vert_properties.extend([f32_of(p.x), f32_of(p.y), f32_of(p.z)]);
                let pu = usize::try_from(prop).map_err(|_| Fault::Index)?;
                for k in 0..num_prop {
                    let v = imp.properties.atu(pu.checked_mul(num_prop).and_then(|x| x.checked_add(k)).ok_or(Fault::TooLarge)?)?;
                    out.vert_properties.push(f32_of(v));
                }
                if update_normals {
                    // only with CalculateNormals (not ported): every run carries normals, so just normalize
                    let start = out.vert_properties.len().checked_sub(stride).ok_or(Fault::Index)?;
                    let ni = usize::try_from(normal_idx).map_err(|_| Fault::Index)?;
                    let at = |k: usize| -> R<f64> {
                        Ok(f64::from(out.vert_properties.atu(
                            start.checked_add(3).and_then(|x| x.checked_add(ni)).and_then(|x| x.checked_add(k)).ok_or(Fault::TooLarge)?,
                        )?))
                    };
                    let n = safe_normalize3(crate::linalg::v3(at(0)?, at(1)?, at(2)?));
                    for (k, c) in [n.x, n.y, n.z].into_iter().enumerate() {
                        let o =
                            start.checked_add(3).and_then(|x| x.checked_add(ni)).and_then(|x| x.checked_add(k)).ok_or(Fault::TooLarge)?;
                        out.vert_properties.setu(o, f32_of(c))?;
                    }
                }
                let slot2 = vert2idx.get_mut(vert).ok_or(Fault::Index)?;
                if *slot2 == -1 {
                    *slot2 = i64::from(idx);
                } else {
                    out.merge_from_vert.push(idx);
                    out.merge_to_vert.push(u32::try_from(*slot2).map_err(|_| Fault::TooLarge)?);
                }
            }
        }
    }
    Ok(out)
}
