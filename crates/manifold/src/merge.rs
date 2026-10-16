// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! sort.cpp `MergeMeshGLP` (`MeshGL::Merge`): vertices on open edges that lie within tolerance are merged through the
//! merge vectors. Input path: strict lints.
//!
//! Where the C++ reads or writes out of bounds (a triangle or merge index past the vertices, `numProp` < 3), this port
//! changes nothing and returns false; constructing the Manifold then reports the error.

use std::collections::BTreeMap;

use crate::Mesh;
use crate::collider::{Collider, morton_code};
use crate::disjoint::DisjointSets;
use crate::linalg::{Box3, V3};
use crate::util::{Fault, Get, GetMut, R, std_max, std_min};

pub(crate) fn merge(mesh: &mut Mesh) -> bool {
    merge_inner(mesh).unwrap_or(false)
}

fn merge_inner(mesh: &mut Mesh) -> R<bool> {
    if mesh.num_prop < 3 || mesh.num_prop > crate::MAX_NUM_PROP {
        return Ok(false);
    }
    let num_vert = mesh.num_vert();
    let num_tri = mesh.num_tri();
    if num_vert > crate::MAX_VERTS || num_tri > crate::MAX_TRIS {
        return Ok(false);
    }
    let in_range = |v: u32| usize::try_from(v).is_ok_and(|v| v < num_vert);
    if !mesh.tri_verts.iter().take(num_tri.checked_mul(3).ok_or(Fault::TooLarge)?).all(|&v| in_range(v))
        || !mesh.merge_from_vert.iter().all(|&v| in_range(v))
        || !mesh.merge_to_vert.iter().all(|&v| in_range(v))
    {
        return Ok(false);
    }

    let mut merge: Vec<u32> = (0..u32::try_from(num_vert).map_err(|_| Fault::TooLarge)?).collect();
    for (&from, &to) in mesh.merge_from_vert.iter().zip(mesh.merge_to_vert.iter()) {
        merge.setu(usize::try_from(from).map_err(|_| Fault::Index)?, to)?;
    }
    let mv = |v: u32| -> R<i32> {
        let m = merge.atu(usize::try_from(v).map_err(|_| Fault::Index)?)?;
        i32::try_from(m).map_err(|_| Fault::TooLarge)
    };

    // std::multiset<pair<int, int>>: a count per key, iterated in key order
    let mut open: BTreeMap<(i32, i32), usize> = BTreeMap::new();
    for tri in 0..num_tri {
        let base = tri.checked_mul(3).ok_or(Fault::TooLarge)?;
        for (i, ni) in [(0usize, 1usize), (1, 2), (2, 0)] {
            let a = mv(mesh.tri_verts.atu(base.checked_add(ni).ok_or(Fault::TooLarge)?)?)?;
            let b = mv(mesh.tri_verts.atu(base.checked_add(i).ok_or(Fault::TooLarge)?)?)?;
            match open.get_mut(&(a, b)) {
                Some(c) => {
                    *c = c.checked_sub(1).ok_or(Fault::Index)?;
                    if *c == 0 {
                        open.remove(&(a, b));
                    }
                }
                None => {
                    let c = open.entry((b, a)).or_insert(0);
                    *c = c.checked_add(1).ok_or(Fault::TooLarge)?;
                }
            }
        }
    }
    if open.is_empty() {
        return Ok(false);
    }

    let mut open_verts: Vec<i32> = Vec::new();
    for (&(first, _), &count) in &open {
        for _ in 0..count {
            open_verts.push(first);
        }
    }
    let num_open = open_verts.len();

    // the bounding box of every position (StridedRange over vertProperties: ceil((len - i) / numProp) values)
    let stride = usize::try_from(mesh.num_prop).map_err(|_| Fault::TooLarge)?;
    let len = mesh.vert_properties.len();
    let mut bbox = Box3::default();
    for i in 0..3usize {
        let mut mn = f64::INFINITY;
        let mut mx = f64::NEG_INFINITY;
        let mut k = i;
        while k < len {
            let f = f64::from(mesh.vert_properties.atu(k)?);
            mn = std_min(mn, f);
            mx = std_max(mx, f);
            k = k.checked_add(stride).ok_or(Fault::TooLarge)?;
        }
        bbox.min.set(i, mn);
        bbox.max.set(i, mx);
    }

    let tolerance = std_max(f64::from(mesh.tolerance), f64::from(f32::EPSILON) * bbox.scale());

    let mut vert_box: Vec<Box3> = Vec::with_capacity(num_open);
    let mut vert_morton: Vec<u32> = Vec::with_capacity(num_open);
    for &vert in &open_verts {
        let o = usize::try_from(vert).map_err(|_| Fault::Index)?.checked_mul(stride).ok_or(Fault::TooLarge)?;
        let at = |k: usize| -> R<f64> { Ok(f64::from(mesh.vert_properties.atu(o.checked_add(k).ok_or(Fault::TooLarge)?)?)) };
        let center = V3 { x: at(0)?, y: at(1)?, z: at(2)? };
        let h = tolerance / 2.0;
        vert_box.push(Box3 {
            min: V3 { x: center.x - h, y: center.y - h, z: center.z - h },
            max: V3 { x: center.x + h, y: center.y + h, z: center.z + h },
        });
        vert_morton.push(if center.x.is_nan() { 0xFFFF_FFFF } else { morton_code(center, &bbox) });
    }

    let mut new2old: Vec<usize> = (0..num_open).collect();
    new2old.sort_by_key(|&a| vert_morton.get(a).copied().unwrap_or(u32::MAX));
    let pm = |v: &Vec<u32>| -> R<Vec<u32>> { new2old.iter().map(|&o| v.atu(o)).collect() };
    let vert_morton = pm(&vert_morton)?;
    let vert_box: Vec<Box3> = new2old.iter().map(|&o| vert_box.atu(o)).collect::<R<_>>()?;
    let open_verts: Vec<i32> = new2old.iter().map(|&o| open_verts.atu(o)).collect::<R<_>>()?;

    let collider = Collider::new(&vert_box, &vert_morton)?;
    let mut uf = DisjointSets::new(num_vert)?;
    collider.collisions_box(
        true,
        num_open,
        |i| vert_box.at(i),
        |a, b| {
            let va = usize::try_from(open_verts.at(a)?).map_err(|_| Fault::Index)?;
            let vb = usize::try_from(open_verts.at(b)?).map_err(|_| Fault::Index)?;
            uf.unite(va, vb).map(|_| ())
        },
    )?;
    for (&from, &to) in mesh.merge_from_vert.iter().zip(mesh.merge_to_vert.iter()) {
        uf.unite(usize::try_from(from).map_err(|_| Fault::Index)?, usize::try_from(to).map_err(|_| Fault::Index)?)?;
    }

    let mut from = Vec::new();
    let mut to = Vec::new();
    for v in 0..num_vert {
        let m = uf.find(v)?;
        if m != v {
            from.push(u32::try_from(v).map_err(|_| Fault::TooLarge)?);
            to.push(u32::try_from(m).map_err(|_| Fault::TooLarge)?);
        }
    }
    mesh.merge_from_vert = from;
    mesh.merge_to_vert = to;
    Ok(true)
}
