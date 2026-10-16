// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! face_op.cpp `Face2Tri`: the polygonal faces of a boolean result become triangles (triangles copied, quads split
//! on the better diagonal, larger faces through the ear-clipping triangulator), and the contour halfedges are
//! paired across faces.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use crate::halfedge::{Halfedge, Halfedges, TriRef};
use crate::imp::Impl;
use crate::linalg::{V3, axis_aligned_projection, ccw, length2_3};
use crate::polygon::{HalfedgeTriangulation, PolyVert, triangulate_idx_halfedges};
use crate::util::{Fault, Get, GetMut, Guard, R, ix, ux};

/// `AssembleHalfedges`: vertex loops of a face's halfedges (indices are halfedge indices offset by `start_idx`).
/// `std::multimap<int, int>` (start vertex → edge): equal keys in insertion order, `begin` = smallest key, `find` =
/// the first remaining of equal keys (libc++'s lower-bound `find`). Here: the (key, edge) pairs sorted once, with
/// removed flags.
fn assemble_halfedges(edges: &[Halfedge], start_idx: i32) -> R<Vec<Vec<i32>>> {
    let mut order: Vec<(i32, i32)> = Vec::with_capacity(edges.len());
    for (i, e) in edges.iter().enumerate() {
        order.push((e.start_vert, ix(i)?));
    }
    order.sort_unstable();
    let mut removed = vec![false; order.len()];
    let mut head = 0usize;
    let mut remaining = edges.len();
    let mut polys: Vec<Vec<i32>> = Vec::new();
    let mut start_edge = 0i32;
    let mut this_edge = start_edge;
    let mut guard = Guard::new(2 * edges.len());
    loop {
        guard.tick()?;
        if this_edge == start_edge {
            if remaining == 0 {
                break;
            }
            while removed.atu(head)? {
                head += 1;
            }
            start_edge = order.atu(head)?.1;
            this_edge = start_edge;
            polys.push(Vec::new());
        }
        polys.last_mut().ok_or(Fault::Index)?.push(start_idx + this_edge);
        let end_vert = edges.at(this_edge)?.end_vert;
        let mut k = order.partition_point(|&(key, _)| key < end_vert);
        while removed.get(k).copied() == Some(true) {
            k += 1;
        }
        // "non-manifold edge" when the end vertex has no outgoing edge left
        let (key, e) = order.atu(k)?;
        if key != end_vert {
            return Err(Fault::Index);
        }
        removed.setu(k, true)?;
        this_edge = e;
        remaining -= 1;
    }
    Ok(polys)
}

impl Impl {
    /// `Face2Tri(faceEdge, faceHalfedge, halfedgeRef, allowConvex = false)`.
    pub(crate) fn face2tri(&mut self, face_edge: &[i32], face_he: &[Halfedge], halfedge_ref: &[TriRef]) -> R<()> {
        let num_face = face_edge.len().checked_sub(1).ok_or(Fault::Index)?;
        let mut contour2tri = vec![-1i32; face_he.len()];

        // precompute triangle counts; triangulate general faces
        let mut tri_offset = vec![0usize; face_edge.len()];
        let mut results: Vec<Option<HalfedgeTriangulation>> = (0..num_face).map(|_| None).collect();
        for face in 0..num_face {
            let num_edge = face_edge.atu(face + 1)? - face_edge.atu(face)?;
            if num_edge == 0 {
                tri_offset.setu(face, 0)?;
                continue;
            }
            if num_edge < 3 {
                return Err(Fault::Index);
            }
            tri_offset.setu(face, ux(num_edge - 2)?)?;
            if num_edge > 4 {
                let t = self.general_triangulation(face, face_edge, face_he)?;
                tri_offset.setu(face, t.num_tri())?;
                *results.atu_mut(face)? = Some(t);
            }
        }
        // exclusive scan
        let mut acc = 0usize;
        for t in tri_offset.iter_mut() {
            let c = *t;
            *t = acc;
            acc += c;
        }
        let total = acc;
        if total > crate::MAX_RESULT_VERTS * 4 {
            return Err(Fault::TooLarge);
        }
        let mut out = Halfedges::default();
        out.resize_nofill(3 * total);
        let mut tri_normal = vec![V3::default(); total];
        let mut tri_ref = vec![TriRef::default(); total];

        for face in 0..num_face {
            let first_tri = tri_offset.atu(face)?;
            let first_edge = face_edge.atu(face)?;
            let last_edge = face_edge.atu(face + 1)?;
            let num_edge = last_edge - first_edge;
            if num_edge == 0 {
                continue;
            }
            let normal = self.face_normal.atu(face)?;
            let mut num_tri = ux(num_edge - 2)?;
            if num_edge == 3 {
                let mut tri_edge = [first_edge, first_edge + 1, first_edge + 2];
                let mut tri =
                    [face_he.at(first_edge)?.start_vert, face_he.at(first_edge + 1)?.start_vert, face_he.at(first_edge + 2)?.start_vert];
                let mut ends =
                    [face_he.at(first_edge)?.end_vert, face_he.at(first_edge + 1)?.end_vert, face_he.at(first_edge + 2)?.end_vert];
                if ends[0] == tri[2] {
                    tri_edge.swap(1, 2);
                    tri.swap(1, 2);
                    ends.swap(1, 2);
                }
                write_local_triangles(&mut out, &mut contour2tri, face_he, first_tri, &[tri_edge])?;
            } else if num_edge == 4 {
                let proj = axis_aligned_projection(normal);
                let quad = assemble_halfedges(face_he.get(ux(first_edge)?..ux(last_edge)?).ok_or(Fault::Index)?, first_edge)?
                    .into_iter()
                    .next()
                    .ok_or(Fault::Index)?;
                let q = |i: usize| quad.atu(i);
                let tris = [[[q(0)?, q(1)?, q(2)?], [q(0)?, q(2)?, q(3)?]], [[q(1)?, q(2)?, q(3)?], [q(0)?, q(1)?, q(3)?]]];
                let tri_ccw = |[t0, t1, t2]: [i32; 3]| -> R<bool> {
                    let p = |e: i32| -> R<crate::linalg::V2> { Ok(proj.apply(self.vert_pos.at(face_he.at(e)?.start_vert)?)) };
                    Ok(ccw(p(t0)?, p(t1)?, p(t2)?, self.epsilon) >= 0)
                };
                let mut choice = 0usize;
                if !(tri_ccw(tris[0][0])? && tri_ccw(tris[0][1])?) {
                    choice = 1;
                } else if tri_ccw(tris[1][0])? && tri_ccw(tris[1][1])? {
                    let pv = |i: usize| -> R<V3> { self.vert_pos.at(face_he.at(quad.atu(i)?)?.start_vert) };
                    let diag0 = pv(0)? - pv(2)?;
                    let diag1 = pv(1)? - pv(3)?;
                    if length2_3(diag0) > length2_3(diag1) {
                        choice = 1;
                    }
                }
                let chosen = if choice == 0 { tris[0] } else { tris[1] };
                write_local_triangles(&mut out, &mut contour2tri, face_he, first_tri, &chosen)?;
            } else {
                let general = results.get(face).and_then(Option::as_ref).ok_or(Fault::Index)?;
                num_tri = general.num_tri();
                write_general_triangulation(&mut out, &mut contour2tri, face_he, first_tri, general)?;
            }
            let r = halfedge_ref.at(first_edge)?;
            for t in 0..num_tri {
                tri_normal.setu(first_tri + t, normal)?;
                tri_ref.setu(first_tri + t, r)?;
            }
        }

        for (edge, &tri_edge) in contour2tri.iter().enumerate() {
            if tri_edge < 0 {
                continue;
            }
            let pair = face_he.atu(edge)?.paired_halfedge;
            if pair < 0 {
                continue;
            }
            let pair_tri = contour2tri.at(pair)?;
            out.set_pair(tri_edge, pair_tri)?;
        }
        self.halfedge = out;
        self.face_normal = tri_normal;
        self.mesh_relation.tri_ref = tri_ref;
        Ok(())
    }

    fn general_triangulation(&self, face: usize, face_edge: &[i32], face_he: &[Halfedge]) -> R<HalfedgeTriangulation> {
        let normal = self.face_normal.atu(face)?;
        let proj = axis_aligned_projection(normal);
        let first = face_edge.atu(face)?;
        let last = face_edge.atu(face + 1)?;
        let loops = assemble_halfedges(face_he.get(ux(first)?..ux(last)?).ok_or(Fault::Index)?, first)?;
        let mut polys: Vec<Vec<PolyVert>> = Vec::with_capacity(loops.len());
        for l in loops {
            let mut p = Vec::with_capacity(l.len());
            for e in l {
                p.push(PolyVert { pos: proj.apply(self.vert_pos.at(face_he.at(e)?.start_vert)?), idx: e });
            }
            polys.push(p);
        }
        triangulate_idx_halfedges(&polys, self.epsilon)
    }
}

/// `WriteLocalTriangles`: one or two triangles of a triangle / quad face, paired locally.
fn write_local_triangles(
    out: &mut Halfedges,
    contour2tri: &mut [i32],
    face_he: &[Halfedge],
    first_tri: usize,
    triangles: &[[i32; 3]],
) -> R<()> {
    let mut local: Vec<[i32; 3]> = Vec::with_capacity(6);
    let first_out = 3 * ix(first_tri)?;
    for &[a, b, c] in triangles {
        for (start, end) in [(a, b), (b, c), (c, a)] {
            let o = first_out + ix(local.len())?;
            local.push([start, end, o]);
            let fh = face_he.at(start)?;
            out.set_start(o, fh.start_vert)?;
            out.set_prop(o, fh.prop_vert)?;
            out.set_pair(o, -1)?;
        }
    }
    for e in &local {
        let mut pair = -1;
        for f in &local {
            if f[0] == e[1] && f[1] == e[0] {
                pair = f[2];
                break;
            }
        }
        if pair >= 0 {
            out.set_pair(e[2], pair)?;
        } else {
            contour2tri.set(e[0], e[2])?;
        }
    }
    Ok(())
}

/// `WriteGeneralTriangulation`.
fn write_general_triangulation(
    out: &mut Halfedges,
    contour2tri: &mut [i32],
    face_he: &[Halfedge],
    first_tri: usize,
    t: &HalfedgeTriangulation,
) -> R<()> {
    let first_out = 3 * ix(first_tri)?;
    let ce = ix(t.contour_end)?;
    let n = 3 * t.num_tri();
    for local in 0..n {
        let o = first_out + ix(local)?;
        let edge = t.halfedges.atu(t.contour_end + local)?;
        let fh = face_he.at(edge.start_vert)?;
        out.set_start(o, fh.start_vert)?;
        out.set_prop(o, fh.prop_vert)?;
        if edge.paired_halfedge >= ce {
            out.set_pair(o, first_out + edge.paired_halfedge - ce)?;
        } else {
            out.set_pair(o, -1)?;
        }
    }
    for contour in 0..t.contour_end {
        let edge = t.halfedges.atu(contour)?;
        if edge.paired_halfedge < 0 {
            continue;
        }
        contour2tri.set(edge.end_vert, first_out + edge.paired_halfedge - ce)?;
    }
    Ok(())
}
