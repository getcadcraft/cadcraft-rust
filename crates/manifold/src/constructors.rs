// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! constructors.cpp (Cube, Sphere, Extrude, Revolve), impl.cpp's `Impl(Shape)` and the triangle path of
//! subdivision.cpp's `Subdivide` (the sphere refines an octahedron), with `Quality::GetCircularSegments`.
//!
//! Inputs are capped: at most [`MAX_POLY_VERTS`] polygon vertices, [`MAX_SEGMENTS`] circular segments, divisions such
//! that the result stays under the crate's vertex / triangle caps; past them the result is an empty Manifold with
//! `ResultTooLarge`.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use crate::Status;
use crate::csg::Manifold;
use crate::halfedge::next3;
use crate::imp::Impl;
use crate::linalg::{V2, V3, V4, length2, length3, normalize3, v2, v3};
use crate::mat::{IDENTITY, Mat34};
use crate::math::{cosd, sind};
use crate::polygon::{PolyVert, triangulate_idx};
use crate::util::{Fault, Get, GetMut, R, ix, std_max, ux};

/// Largest number of polygon vertices accepted by Extrude / Revolve.
pub const MAX_POLY_VERTS: usize = 1 << 20;
/// Largest number of circular segments (Revolve, Sphere).
pub const MAX_SEGMENTS: i32 = 1 << 14;

/// manifold.cpp `Quality::GetCircularSegments` with the default settings (no fixed count, 10°, edge length 1).
pub(crate) fn circular_segments(radius: f64) -> i32 {
    let n_seg_a = (360.0f64 / 10.0) as i32;
    let n_seg_l = (2.0 * radius.abs() * crate::imp::K_PI / 1.0) as i32;
    let mut n = (libm::fmin(f64::from(n_seg_a), f64::from(n_seg_l)) + 3.0) as i32;
    n -= n % 4;
    n.max(4)
}

/// impl.cpp `Impl(Shape, m)`: the unit cube or the octahedron, transformed.
fn shape(octahedron: bool, m: &Mat34) -> R<Impl> {
    let (pos, tris): (&[[f64; 3]], &[[i32; 3]]) = if octahedron {
        (
            &[[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]],
            &[[0, 2, 4], [1, 5, 3], [2, 1, 4], [3, 5, 0], [1, 3, 4], [0, 5, 2], [3, 0, 4], [2, 5, 1]],
        )
    } else {
        (
            &[
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0],
                [0.0, 1.0, 1.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 1.0],
                [1.0, 1.0, 0.0],
                [1.0, 1.0, 1.0],
            ],
            &[
                [1, 0, 4],
                [2, 4, 0],
                [1, 3, 0],
                [3, 1, 5],
                [3, 2, 0],
                [3, 7, 2],
                [5, 4, 6],
                [5, 1, 4],
                [6, 4, 2],
                [7, 6, 2],
                [7, 3, 5],
                [7, 5, 6],
            ],
        )
    };
    let mut imp = Impl { vert_pos: pos.iter().map(|p| m.apply(v3(p[0], p[1], p[2]))).collect(), ..Impl::default() };
    imp.create_halfedges(tris, &[])?;
    imp.finish_original()?;
    Ok(imp)
}

impl Impl {
    /// The tail every constructor shares: InitializeOriginal, CalculateBBox, SetEpsilon, SortGeometry,
    /// SetNormalsAndCoplanar.
    pub(crate) fn finish_original(&mut self) -> R<()> {
        self.initialize_original();
        self.calculate_bbox();
        self.set_epsilon(-1.0, false);
        self.sort_geometry()?;
        self.set_normals_and_coplanar()
    }
}

fn built(r: R<Impl>) -> Manifold {
    match r {
        Ok(imp) => Manifold::from_impl(imp),
        Err(f) => Manifold::with_status(f.status()),
    }
}

/// `Manifold::Cube(size, center)`.
pub(crate) fn cube(size: V3, center: bool) -> Manifold {
    if size.x < 0.0 || size.y < 0.0 || size.z < 0.0 || length3(size) == 0.0 {
        return Manifold::with_status(Status::InvalidConstruction);
    }
    let t = if center { (-size) / 2.0 } else { V3::splat(0.0) };
    let m = Mat34([v3(size.x, 0.0, 0.0), v3(0.0, size.y, 0.0), v3(0.0, 0.0, size.z), t]);
    built(shape(false, &m))
}

/// `Manifold::Sphere(radius, circularSegments)`: an octahedron with every edge split into n pieces, pushed out.
pub(crate) fn sphere(radius: f64, circular_segments: i32) -> Manifold {
    if radius <= 0.0 || radius.is_nan() {
        return Manifold::with_status(Status::InvalidConstruction);
    }
    let n = if circular_segments > 0 { (circular_segments.min(MAX_SEGMENTS) + 3) / 4 } else { circular_segments_capped(radius) / 4 };
    built(sphere_impl(radius, n))
}

fn circular_segments_capped(radius: f64) -> i32 {
    circular_segments(radius).min(MAX_SEGMENTS)
}

fn sphere_impl(radius: f64, n: i32) -> R<Impl> {
    let mut imp = shape(true, &IDENTITY)?;
    imp.subdivide_uniform(n - 1)?;
    // common.h kHalfPi, the same double as FRAC_PI_2
    const K_HALF_PI: f64 = core::f64::consts::FRAC_PI_2;
    for v in &mut imp.vert_pos {
        let c = v3(libm::cos(K_HALF_PI * (1.0 - v.x)), libm::cos(K_HALF_PI * (1.0 - v.y)), libm::cos(K_HALF_PI * (1.0 - v.z)));
        let mut p = normalize3(c) * radius;
        // `radius * la::normalize(v)` multiplies the same way (IEEE multiplication commutes)
        if p.x.is_nan() {
            p = V3::splat(0.0);
        }
        *v = p;
    }
    imp.finish_original()?;
    Ok(imp)
}

/// One sub-triangulation of a triangle (subdivision.cpp `Partition`, triangle case).
#[derive(Clone, Default)]
struct Partition {
    idx: [i32; 4],
    sorted: [i32; 4],
    vert_bary: Vec<V4>,
    tri_vert: Vec<[i32; 3]>,
}

fn lerp4(a: V4, b: V4, t: f64) -> V4 {
    a * (1.0 - t) + b * t
}

fn bary_axis(i: usize) -> V4 {
    let mut v = [0.0; 4];
    if let Some(x) = v.get_mut(i) {
        *x = 1.0;
    }
    V4 { x: v[0], y: v[1], z: v[2], w: v[3] }
}

fn get4(v: &[i32; 4], i: i32) -> R<i32> {
    v.get(ux(i)?).copied().ok_or(Fault::Index)
}

impl Partition {
    fn interior_offset(&self) -> usize {
        self.sorted.iter().map(|&x| x.max(0) as usize).sum()
    }
    fn num_interior(&self) -> usize {
        self.vert_bary.len().saturating_sub(self.interior_offset())
    }

    /// `GetPartition` for a triangle (`divisions[3] == 0`).
    fn get(div: [i32; 4]) -> R<Partition> {
        if div[0] == 0 {
            return Ok(Partition::default());
        }
        let mut s = div;
        let mut idx = [0, 1, 2, 3];
        if s[2] > s[1] {
            s.swap(2, 1);
            idx.swap(2, 1);
        }
        if s[1] > s[0] {
            s.swap(1, 0);
            idx.swap(1, 0);
            if s[2] > s[1] {
                s.swap(2, 1);
                idx.swap(2, 1);
            }
        }
        let mut p = Partition::cached(s)?;
        p.idx = idx;
        Ok(p)
    }

    /// `GetCachedPartition` (triangle): purely topological, from the sorted divisions n0 ≥ n1 ≥ n2 > 0.
    fn cached(n: [i32; 4]) -> R<Partition> {
        let mut p = Partition { sorted: n, ..Partition::default() };
        p.vert_bary.extend([bary_axis(0), bary_axis(1), bary_axis(2)]);
        for (i, &ni) in n.iter().take(3).enumerate() {
            let next = p.vert_bary.atu((i + 1) % 3)?;
            for j in 1..ni {
                let b = lerp4(p.vert_bary.atu(i)?, next, f64::from(j) / f64::from(ni));
                p.vert_bary.push(b);
            }
        }
        let eo = [3, 3 + n[0] - 1, 3 + n[0] - 1 + n[1] - 1];
        let f = f64::from(n[2] * n[2] + n[0] * n[0]);
        let mut tv = Vec::new();
        let mut vb = core::mem::take(&mut p.vert_bary);
        if n[1] == 1 {
            if n[0] == 1 {
                tv.push([0, 1, 2]);
            } else {
                partition_fan(&mut tv, [0, 1, 2], n[0] - 1, eo[0]);
            }
        } else if f64::from(n[1] * n[1]) > f - 2.0f64.sqrt() * f64::from(n[0]) * f64::from(n[2]) {
            tv.push([eo[1] - 1, 1, eo[1]]);
            partition_quad(
                &mut tv,
                &mut vb,
                [eo[1] - 1, eo[1], 2, 0],
                [-1, eo[1] + 1, eo[2], eo[0]],
                [0, n[1] - 2, n[2] - 1, n[0] - 2],
                [true; 4],
                0,
            )?;
        } else {
            let ns = (n[0] - 2).min(((f - f64::from(n[1] * n[1])) / f64::from(2 * n[0])).round() as i32);
            let nh = std_max(1.0, (f64::from(n[2] * n[2] - ns * ns)).sqrt().round()) as i32;
            let h_offset = ix(vb.len())?;
            let middle = vb.at(eo[0] + ns - 1)?;
            for j in 1..nh {
                let b = lerp4(vb.atu(2)?, middle, f64::from(j) / f64::from(nh));
                vb.push(b);
            }
            tv.push([eo[1] - 1, 1, eo[1]]);
            partition_quad(
                &mut tv,
                &mut vb,
                [eo[1] - 1, eo[1], 2, eo[0] + ns - 1],
                [-1, eo[1] + 1, h_offset, eo[0] + ns],
                [0, n[1] - 2, nh - 1, n[0] - ns - 2],
                [true; 4],
                0,
            )?;
            if n[2] == 1 {
                partition_fan(&mut tv, [0, eo[0] + ns - 1, 2], ns - 1, eo[0]);
            } else if ns == 1 {
                tv.push([h_offset, 2, eo[2]]);
                partition_quad(
                    &mut tv,
                    &mut vb,
                    [h_offset, eo[2], 0, eo[0]],
                    [-1, eo[2] + 1, -1, h_offset + nh - 2],
                    [0, n[2] - 2, ns - 1, nh - 2],
                    [true, true, true, false],
                    0,
                )?;
            } else {
                tv.push([h_offset - 1, 0, eo[0]]);
                partition_quad(
                    &mut tv,
                    &mut vb,
                    [h_offset - 1, eo[0], eo[0] + ns - 1, 2],
                    [-1, eo[0] + 1, h_offset + nh - 2, eo[2]],
                    [0, ns - 2, nh - 1, n[2] - 2],
                    [true, true, false, true],
                    0,
                )?;
            }
        }
        p.vert_bary = vb;
        p.tri_vert = tv;
        Ok(p)
    }

    /// `Reindex` for a triangle: the partition's vertices mapped to the mesh's.
    fn reindex(&self, tri_verts: [i32; 4], edge_offsets: [i32; 4], edge_fwd: [bool; 4], interior_offset: i32) -> R<Vec<[i32; 3]>> {
        let mut new_verts: Vec<i32> = Vec::with_capacity(self.vert_bary.len());
        let mut tri_idx = self.idx;
        let mut out_tri = [0usize, 1, 2, 3];
        let mut fwd = edge_fwd;
        if tri_verts[3] < 0 && self.idx[1] != next3(ux(self.idx[0])?) as i32 {
            tri_idx = [self.idx[2], self.idx[0], self.idx[1], self.idx[3]];
            fwd = [!fwd[0], !fwd[1], !fwd[2], !fwd[3]];
            out_tri.swap(0, 1);
        }
        for &ti in &tri_idx {
            let v = get4(&tri_verts, ti)?;
            if v >= 0 {
                new_verts.push(v);
            }
        }
        for (&sd, &ii) in self.sorted.iter().zip(&self.idx) {
            let n = sd - 1;
            if n <= 0 {
                continue;
            }
            let k = ux(ii)?;
            let f = fwd.get(k).copied().ok_or(Fault::Index)?;
            let mut offset = edge_offsets.get(k).copied().ok_or(Fault::Index)? + if f { 0 } else { n - 1 };
            for _ in 0..n {
                new_verts.push(offset);
                offset += if f { 1 } else { -1 };
            }
        }
        let old = new_verts.len();
        for (next, _) in (interior_offset..).zip(old..self.vert_bary.len()) {
            new_verts.push(next);
        }
        let mut out = Vec::with_capacity(self.tri_vert.len());
        for t in &self.tri_vert {
            let mut nt = [0i32; 3];
            for (&o, &tj) in out_tri.iter().zip(t) {
                *nt.get_mut(o).ok_or(Fault::Index)? = new_verts.at(tj)?;
            }
            out.push(nt);
        }
        Ok(out)
    }
}

/// `PartitionFan`: side 0 has added edges, sides 1 and 2 do not; the fan spreads from vertex 2.
fn partition_fan(tv: &mut Vec<[i32; 3]>, corner: [i32; 3], added: i32, edge_offset: i32) {
    let mut last = corner[0];
    for i in 0..added {
        let next = edge_offset + i;
        tv.push([last, next, corner[2]]);
        last = next;
    }
    tv.push([last, corner[1], corner[2]]);
}

/// `ivec4[i]` / `bvec4[i]` with a checked index.
#[inline]
fn a4(v: &[i32; 4], i: usize) -> R<i32> {
    v.get(i).copied().ok_or(Fault::Index)
}
#[inline]
fn b4(v: &[bool; 4], i: usize) -> R<bool> {
    v.get(i).copied().ok_or(Fault::Index)
}

/// `PartitionQuad` (recursive in the C++; the depth is bounded here).
#[allow(clippy::too_many_arguments)]
fn partition_quad(
    tv: &mut Vec<[i32; 3]>,
    vb: &mut Vec<V4>,
    corner: [i32; 4],
    eo: [i32; 4],
    added: [i32; 4],
    fwd: [bool; 4],
    depth: u32,
) -> R<()> {
    if depth > 64 {
        return Err(Fault::Loop);
    }
    let edge_vert = |edge: usize, idx: i32| -> R<i32> { Ok(a4(&eo, edge)? + if b4(&fwd, edge)? { 1 } else { -1 } * idx) };
    let mut corner_i: i32 = -1;
    let mut last = 3usize;
    let mut max_edge: i32 = -1;
    for (i, &ai) in added.iter().enumerate() {
        if corner_i == -1 && ai == 0 && a4(&added, last)? == 0 {
            corner_i = i as i32;
        }
        if ai > 0 {
            max_edge = if max_edge == -1 { i as i32 } else { -2 };
        }
        last = i;
    }
    if corner_i >= 0 {
        if max_edge >= 0 {
            let me = max_edge as usize;
            let edge = [me % 4, (1 + me) % 4, (2 + me) % 4, (3 + me) % 4];
            let am = a4(&added, me)?;
            let middle = am / 2;
            tv.push([a4(&corner, edge[2])?, a4(&corner, edge[3])?, edge_vert(me, middle)?]);
            let mut l = a4(&corner, edge[0])?;
            for i in 0..=middle {
                let next = edge_vert(me, i)?;
                tv.push([a4(&corner, edge[3])?, l, next]);
                l = next;
            }
            l = a4(&corner, edge[1])?;
            let mut i = am - 1;
            while i >= middle {
                let next = edge_vert(me, i)?;
                tv.push([a4(&corner, edge[2])?, next, l]);
                l = next;
                i -= 1;
            }
        } else {
            let c = corner_i as usize;
            let cc = a4(&corner, c)?;
            let mut side_vert = corner[0];
            for j in [1usize, 2] {
                let side = (c + j) % 4;
                let as_ = a4(&added, side)?;
                if j == 2 && as_ > 0 {
                    tv.push([a4(&corner, side)?, edge_vert(side, 0)?, side_vert]);
                } else {
                    side_vert = a4(&corner, side)?;
                }
                for i in 0..as_ {
                    let next_vert = edge_vert(side, i)?;
                    tv.push([cc, side_vert, next_vert]);
                    side_vert = next_vert;
                }
                if j == 2 || as_ == 0 {
                    tv.push([cc, side_vert, a4(&corner, (c + j + 1) % 4)?]);
                }
            }
        }
        return Ok(());
    }
    let partitions = 1 + added[1].min(added[3]);
    let mut nc = [corner[1], -1, -1, corner[0]];
    let mut neo = [eo[1], -1, edge_vert(3, added[3] + 1)?, eo[0]];
    let mut nadd = [0, -1, 0, added[0]];
    let mut nfwd = [fwd[1], true, fwd[3], fwd[0]];
    for i in 1..partitions {
        let c1 = (added[1] * i) / partitions;
        let c3 = added[3] - 1 - (added[3] * i) / partitions;
        let next1 = edge_vert(1, c1 + 1)?;
        let next3v = edge_vert(3, c3 + 1)?;
        let t = f64::from(i) / f64::from(partitions);
        let n_added = (f64::from(added[0]) * (1.0 - t) + f64::from(added[2]) * t).round() as i32;
        nc[1] = edge_vert(1, c1)?;
        nc[2] = edge_vert(3, c3)?;
        nadd[0] = (next1 - neo[0]).abs() - 1;
        nadd[1] = n_added;
        nadd[2] = (next3v - neo[2]).abs() - 1;
        neo[1] = ix(vb.len())?;
        neo[2] = next3v;
        for j in 0..n_added {
            let b = lerp4(vb.at(nc[1])?, vb.at(nc[2])?, (f64::from(j) + 1.0) / (f64::from(n_added) + 1.0));
            vb.push(b);
        }
        partition_quad(tv, vb, nc, neo, nadd, nfwd, depth + 1)?;
        nc[0] = nc[1];
        nc[3] = nc[2];
        nadd[3] = nadd[1];
        neo[0] = next1;
        neo[3] = neo[1] + nadd[1] - 1;
        nfwd[3] = false;
    }
    nc[1] = corner[2];
    nc[2] = corner[3];
    neo[1] = eo[2];
    nadd[0] = added[1] - (neo[0] - eo[1]).abs();
    nadd[1] = added[2];
    nadd[2] = (neo[2] - eo[3]).abs() - 1;
    neo[2] = eo[3];
    nfwd[1] = fwd[2];
    partition_quad(tv, vb, nc, neo, nadd, nfwd, depth + 1)
}

impl Impl {
    /// `Subdivide` with the same number of added vertices on every edge, for a mesh without quads, tangents or
    /// properties (what `Sphere` hands it).
    fn subdivide_uniform(&mut self, added: i32) -> R<()> {
        if !self.halfedge_tangent.is_empty() || self.num_prop != 0 {
            return Err(Fault::Index);
        }
        let added = added.max(0);
        let num_vert = self.num_vert();
        let num_tri = self.num_tri();
        // CreateTmpEdges: the forward halfedges in order
        let mut edges: Vec<i32> = Vec::new();
        for h in 0..ix(self.halfedge.len())? {
            if self.halfedge.start(h)? < self.halfedge.end(h)? {
                edges.push(h);
            }
        }
        let num_edge = edges.len();
        let mut half2edge = vec![0i32; 2 * num_edge];
        for (e, &h) in edges.iter().enumerate() {
            half2edge.set(h, ix(e)?)?;
            half2edge.set(self.halfedge.pair(h)?, ix(e)?)?;
        }
        let total = (num_vert as u64) + (num_edge as u64) * (added as u64) + (num_tri as u64) * (added as u64) * (added as u64);
        if total > crate::MAX_RESULT_VERTS as u64 {
            return Err(Fault::TooLarge);
        }
        let edge_added = vec![added; num_edge];
        let mut edge_offset = vec![0i32; num_edge];
        let mut acc = ix(num_vert)?;
        for (o, &a) in edge_offset.iter_mut().zip(&edge_added) {
            *o = acc;
            acc += a;
        }
        let nbary = match (edge_offset.last(), edge_added.last()) {
            (Some(&o), Some(&a)) => ux(o + a)?,
            _ => num_vert,
        };
        let mut vert_bary: Vec<(i32, V4)> = vec![(0, V4::default()); nbary];
        // FillRetainedVerts
        for tri in 0..ix(num_tri)? {
            for i in 0..3 {
                let uvw = bary_axis(i);
                vert_bary.set(self.halfedge.start(3 * tri + i as i32)?, (tri, uvw))?;
            }
        }
        for (i, &h) in edges.iter().enumerate() {
            let n = edge_added.atu(i)?;
            let offset = edge_offset.atu(i)?;
            let (tri, start4, end4) = (h / 3, (h % 3) as usize, next3((h % 3) as usize));
            let frac = 1.0 / f64::from(n + 1);
            for k in 0..n {
                let mut uvw = [0.0f64; 4];
                let e = f64::from(k + 1) * frac;
                *uvw.get_mut(end4).ok_or(Fault::Index)? = e;
                *uvw.get_mut(start4).ok_or(Fault::Index)? = 1.0 - e;
                vert_bary.set(offset + k, (tri, V4 { x: uvw[0], y: uvw[1], z: uvw[2], w: uvw[3] }))?;
            }
        }
        let mut sub = Vec::with_capacity(num_tri);
        for tri in 0..ix(num_tri)? {
            let mut div = [0i32; 4];
            for (i, d) in div.iter_mut().take(3).enumerate() {
                *d = edge_added.at(half2edge.at(3 * tri + i as i32)?)? + 1;
            }
            sub.push(Partition::get(div)?);
        }
        let mut tri_offset = Vec::with_capacity(num_tri);
        let mut acc = 0usize;
        for p in &sub {
            tri_offset.push(acc);
            acc += p.tri_vert.len();
        }
        let num_new_tri = acc;
        let mut interior_offset = Vec::with_capacity(num_tri);
        let mut acc = vert_bary.len();
        for p in &sub {
            interior_offset.push(acc);
            acc += p.num_interior();
        }
        vert_bary.resize(acc, (0, V4::default()));
        let mut tri_verts: Vec<[i32; 3]> = vec![[0; 3]; num_new_tri];
        let mut tri_ref = vec![Default::default(); num_new_tri];
        let mut face_normal = vec![V3::default(); num_new_tri];
        for tri in 0..num_tri {
            let t = ix(tri)?;
            let mut tri3 = [-1i32; 4];
            let mut eo = [0i32; 4];
            let mut fwd = [false; 4];
            for (i, ((t3, e), f)) in tri3.iter_mut().zip(eo.iter_mut()).zip(fwd.iter_mut()).take(3).enumerate() {
                let h = 3 * t + i as i32;
                *t3 = self.halfedge.start(h)?;
                *e = edge_offset.at(half2edge.at(h)?)?;
                *f = self.halfedge.start(h)? < self.halfedge.end(h)?;
            }
            let p = sub.get(tri).ok_or(Fault::Index)?;
            let new_tris = p.reindex(tri3, eo, fwd, ix(interior_offset.atu(tri)?)?)?;
            let base = tri_offset.atu(tri)?;
            let r = self.mesh_relation.tri_ref.atu(tri)?;
            let fnrm = self.face_normal.atu(tri)?;
            for (k, nt) in new_tris.iter().enumerate() {
                tri_verts.setu(base + k, *nt)?;
                tri_ref.setu(base + k, r)?;
                face_normal.setu(base + k, fnrm)?;
            }
            let idx = p.idx;
            let v_idx = if idx[1] == next3(ux(idx[0])?) as i32 { idx } else { [idx[2], idx[0], idx[1], idx[3]] };
            let mut r_idx = [0usize; 4];
            for (i, &v) in v_idx.iter().enumerate() {
                *r_idx.get_mut(ux(v)?).ok_or(Fault::Index)? = i;
            }
            let io = interior_offset.atu(tri)?;
            for (k, b) in p.vert_bary.iter().skip(p.interior_offset()).enumerate() {
                let arr = [b.x, b.y, b.z, b.w];
                let g = |i: usize| r_idx.get(i).and_then(|&r| arr.get(r)).copied().unwrap_or(0.0);
                vert_bary.setu(io + k, (t, V4 { x: g(0), y: g(1), z: g(2), w: g(3) }))?;
            }
        }
        self.mesh_relation.tri_ref = tri_ref;
        self.face_normal = face_normal;
        let mut new_pos = Vec::with_capacity(vert_bary.len());
        for &(tri, uvw) in &vert_bary {
            let p0 = self.vert_pos.at(self.halfedge.start(3 * tri)?)?;
            let p1 = self.vert_pos.at(self.halfedge.start(3 * tri + 1)?)?;
            let p2 = self.vert_pos.at(self.halfedge.start(3 * tri + 2)?)?;
            new_pos.push(p0 * uvw.x + p1 * uvw.y + p2 * uvw.z);
        }
        self.vert_pos = new_pos;
        self.create_halfedges(&tri_verts, &[])
    }
}

/// `Manifold::Extrude`.
pub(crate) fn extrude(polys: &[Vec<[f64; 2]>], height: f64, n_divisions: i32, twist_degrees: f64, scale_top: [f64; 2]) -> Manifold {
    // `height <= 0` (NaN passes through, as in the C++, and is caught as a non-finite vertex below)
    if polys.is_empty() || height <= 0.0 {
        return Manifold::with_status(Status::InvalidConstruction);
    }
    let total: usize = polys.iter().map(Vec::len).sum();
    let n_div = n_divisions.max(0);
    if total > MAX_POLY_VERTS || (total as u64) * (u64::from(n_div.cast_unsigned()) + 2) > crate::MAX_RESULT_VERTS as u64 {
        return Manifold::with_status(Status::ResultTooLarge);
    }
    built(extrude_impl(polys, height, n_divisions, twist_degrees, scale_top))
}

fn extrude_impl(polys: &[Vec<[f64; 2]>], height: f64, n_divisions: i32, twist_degrees: f64, scale_top: [f64; 2]) -> R<Impl> {
    let scale_top = v2(std_max(scale_top[0], 0.0), std_max(scale_top[1], 0.0));
    let mut imp = Impl::default();
    let n_divisions = n_divisions + 1;
    let mut tri_verts: Vec<[i32; 3]> = Vec::new();
    let mut n_cs: i32 = 0;
    let is_cone = scale_top.x == 0.0 && scale_top.y == 0.0;
    let mut idx = 0i32;
    let mut indexed: Vec<Vec<PolyVert>> = Vec::with_capacity(polys.len());
    for poly in polys {
        n_cs += ix(poly.len())?;
        let mut simple = Vec::with_capacity(poly.len());
        for p in poly {
            imp.vert_pos.push(v3(p[0], p[1], 0.0));
            simple.push(PolyVert { pos: v2(p[0], p[1]), idx });
            idx += 1;
        }
        indexed.push(simple);
    }
    for i in 1..n_divisions + 1 {
        let alpha = f64::from(i) / f64::from(n_divisions);
        let phi = alpha * twist_degrees;
        // la::lerp(vec2(1.0), scaleTop, alpha)
        let scale = V2::splat(1.0) * (1.0 - alpha) + scale_top * alpha;
        let (c, s) = (cosd(phi), sind(phi));
        let rot = [v2(c, s), v2(-s, c)];
        let sm = [v2(scale.x, 0.0), v2(0.0, scale.y)];
        let mul = |m: &[V2; 2], v: V2| m[0] * v.x + m[1] * v.y;
        let tf = [mul(&sm, rot[0]), mul(&sm, rot[1])];
        let mut idx = 0i32;
        for (j, poly) in (0i32..).zip(polys) {
            let plen = ix(poly.len())?;
            for vert in 0..plen {
                let offset = idx + n_cs * i;
                let this_vert = vert + offset;
                let last_vert = (if vert == 0 { plen } else { vert }) - 1 + offset;
                if i == n_divisions && is_cone {
                    tri_verts.push([n_cs * i + j, last_vert - n_cs, this_vert - n_cs]);
                } else {
                    let p = poly.get(vert as usize).ok_or(Fault::Index)?;
                    let pos = mul(&tf, v2(p[0], p[1]));
                    imp.vert_pos.push(v3(pos.x, pos.y, height * alpha));
                    tri_verts.push([this_vert, last_vert, this_vert - n_cs]);
                    tri_verts.push([last_vert, last_vert - n_cs, this_vert - n_cs]);
                }
            }
            idx += plen;
        }
    }
    if is_cone {
        for _ in polys {
            imp.vert_pos.push(v3(0.0, 0.0, height));
        }
    }
    let top = triangulate_idx(&indexed, -1.0, true)?;
    for t in &top {
        tri_verts.push([t[0], t[2], t[1]]);
        if !is_cone {
            let o = n_cs * n_divisions;
            tri_verts.push([t[0] + o, t[1] + o, t[2] + o]);
        }
    }
    imp.create_halfedges(&tri_verts, &[])?;
    imp.finish_original()?;
    Ok(imp)
}

/// `Manifold::Revolve`.
pub(crate) fn revolve(polys: &[Vec<[f64; 2]>], circular_segments: i32, revolve_degrees: f64) -> Manifold {
    let total: usize = polys.iter().map(Vec::len).sum();
    if total > MAX_POLY_VERTS {
        return Manifold::with_status(Status::ResultTooLarge);
    }
    built(revolve_impl(polys, circular_segments, revolve_degrees))
}

fn revolve_impl(cross: &[Vec<[f64; 2]>], circular_segments: i32, revolve_degrees: f64) -> R<Impl> {
    let mut polygons: Vec<Vec<V2>> = Vec::new();
    let mut radius = 0.0f64;
    for poly in cross {
        let pt = |i: usize| -> R<V2> { poly.get(i).map(|p| v2(p[0], p[1])).ok_or(Fault::Index) };
        let mut i = 0usize;
        while i < poly.len() && pt(i)?.x < 0.0 {
            i += 1;
        }
        if i == poly.len() {
            continue;
        }
        let mut out = Vec::new();
        let start = i;
        let mut guard = crate::util::Guard::new(poly.len());
        loop {
            guard.tick()?;
            let pi = pt(i)?;
            if pi.x >= 0.0 {
                out.push(pi);
                radius = std_max(radius, pi.x);
            }
            let next = if i + 1 == poly.len() { 0 } else { i + 1 };
            let pn = pt(next)?;
            if (pn.x < 0.0) != (pi.x < 0.0) {
                let y = pn.y - pn.x * (pi.y - pn.y) / (pi.x - pn.x);
                out.push(v2(0.0, y));
            }
            i = next;
            if i == start {
                break;
            }
        }
        polygons.push(out);
    }
    if polygons.is_empty() {
        return Ok(Impl::with_status(Status::InvalidConstruction));
    }
    let revolve_degrees = if revolve_degrees > 360.0 { 360.0 } else { revolve_degrees };
    let full = revolve_degrees == 360.0;
    let n_divisions = if circular_segments > 2 {
        circular_segments
    } else {
        (f64::from(circular_segments_capped(radius)) * revolve_degrees / 360.0) as i32
    };
    if n_divisions > MAX_SEGMENTS {
        return Err(Fault::TooLarge);
    }
    let total: usize = polygons.iter().map(Vec::len).sum();
    if (total as u64) * (n_divisions.max(0) as u64 + 1) > crate::MAX_RESULT_VERTS as u64 {
        return Err(Fault::TooLarge);
    }
    let mut imp = Impl::default();
    let mut tri_verts: Vec<[i32; 3]> = Vec::new();
    let mut start_poses: Vec<i32> = Vec::new();
    let mut end_poses: Vec<i32> = Vec::new();
    let d_phi = revolve_degrees / f64::from(n_divisions);
    let n_slices = if full { n_divisions } else { n_divisions + 1 };
    for poly in &polygons {
        let mut n_pos = 0i32;
        let mut n_axis = 0i32;
        for p in poly {
            if p.x > 0.0 {
                n_pos += 1;
            } else {
                n_axis += 1;
            }
        }
        for pv in 0..poly.len() {
            let start_pos = ix(imp.vert_pos.len())?;
            if !full {
                start_poses.push(start_pos);
            }
            let curr = poly.atu(pv)?;
            let prev = poly.atu(if pv == 0 { poly.len() - 1 } else { pv - 1 })?;
            let prev_start = start_pos + if pv == 0 { n_axis + n_slices * n_pos } else { 0 } + if prev.x == 0.0 { -1 } else { -n_slices };
            for slice in 0..n_slices {
                let phi = f64::from(slice) * d_phi;
                if slice == 0 || curr.x > 0.0 {
                    imp.vert_pos.push(v3(curr.x * cosd(phi), curr.x * sind(phi), curr.y));
                }
                if full || slice > 0 {
                    let last_slice = (if slice == 0 { n_divisions } else { slice }) - 1;
                    if curr.x > 0.0 {
                        tri_verts.push([
                            start_pos + slice,
                            start_pos + last_slice,
                            if prev.x == 0.0 { prev_start } else { prev_start + last_slice },
                        ]);
                    }
                    if prev.x > 0.0 {
                        tri_verts.push([
                            prev_start + last_slice,
                            prev_start + slice,
                            if curr.x == 0.0 { start_pos } else { start_pos + slice },
                        ]);
                    }
                }
            }
            if !full {
                end_poses.push(ix(imp.vert_pos.len())? - 1);
            }
        }
    }
    if !full {
        let mut idx = 0i32;
        let indexed: Vec<Vec<PolyVert>> = polygons
            .iter()
            .map(|p| {
                p.iter()
                    .map(|&pos| {
                        let v = PolyVert { pos, idx };
                        idx += 1;
                        v
                    })
                    .collect()
            })
            .collect();
        let front = triangulate_idx(&indexed, imp.epsilon, true)?;
        for t in &front {
            tri_verts.push([start_poses.at(t[0])?, start_poses.at(t[1])?, start_poses.at(t[2])?]);
        }
        for t in &front {
            tri_verts.push([end_poses.at(t[2])?, end_poses.at(t[1])?, end_poses.at(t[0])?]);
        }
    }
    imp.create_halfedges(&tri_verts, &[])?;
    imp.finish_original()?;
    Ok(imp)
}

#[allow(dead_code)]
fn unused(_: V2) -> f64 {
    length2(V2::splat(0.0))
}
