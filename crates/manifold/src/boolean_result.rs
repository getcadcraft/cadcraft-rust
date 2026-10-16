// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! boolean_result.cpp `Boolean3::Result`: winding numbers become inclusion counts, retained and new vertices are laid
//! out, partial / new / whole edges are distributed to the result's polygonal faces, which are triangulated, given
//! properties and references, simplified and sorted.
//!
//! `std::map` iteration (key order) becomes `BTreeMap`; `std::partition` in `PairUp` follows libc++'s bidirectional
//! algorithm exactly (it is not stable, and the stable sorts after it keep its order among equal keys).
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::collections::BTreeMap;

use crate::boolean3::Boolean3;
use crate::halfedge::{Halfedge, Halfedges, TriRef, next_halfedge, next3, prev3};
use crate::imp::{Impl, mesh_id_counter};
use crate::linalg::{Box3, V3, dot3};
use crate::util::{Fault, Get, GetMut, R, by_less, ix, std_max, ux};
use crate::{OpType, Status};

#[derive(Clone, Copy, Debug)]
struct EdgePos {
    edge_pos: f64,
    vert: i32,
    collision_id: i32,
    is_start: bool,
}

#[inline]
fn edge_pos_less(a: &EdgePos, b: &EdgePos) -> bool {
    a.edge_pos < b.edge_pos || (a.edge_pos == b.edge_pos && a.collision_id < b.collision_id)
}

/// `std::partition` (libc++, bidirectional): returns the split point.
fn libcxx_partition<T>(v: &mut [T], pred: impl Fn(&T) -> bool) -> usize {
    let mut first = 0usize;
    let mut last = v.len();
    loop {
        loop {
            if first == last {
                return first;
            }
            if !v.get(first).is_some_and(&pred) {
                break;
            }
            first += 1;
        }
        loop {
            last -= 1;
            if first == last {
                return first;
            }
            if v.get(last).is_some_and(&pred) {
                break;
            }
        }
        v.swap(first, last);
        first += 1;
    }
}

/// `PairUp`: starts paired with ends, each half ordered by position along the edge.
fn pair_up(edge_pos: &mut [EdgePos], mut f: impl FnMut(Halfedge) -> R<()>) -> R<()> {
    let n_edges = edge_pos.len() / 2;
    let middle = libcxx_partition(edge_pos, |x| x.is_start);
    // On geometrically invalid input the halves differ in size ("Non-manifold edge!", a debug assert): the C++ then
    // pairs element i with element i + nEdges regardless, and so does this.
    let (starts, ends) = edge_pos.split_at_mut(middle);
    starts.sort_by(by_less(edge_pos_less));
    ends.sort_by(by_less(edge_pos_less));
    for i in 0..n_edges {
        let s = edge_pos.atu(i)?;
        let e = edge_pos.atu(i + n_edges)?;
        f(Halfedge { start_vert: s.vert, end_vert: e.vert, paired_halfedge: -1, prop_vert: 0 })?;
    }
    Ok(())
}

/// `AbsSum`.
#[inline]
fn abs_sum(a: i32, b: i32) -> R<i32> {
    a.checked_abs().and_then(|x| b.checked_abs().and_then(|y| x.checked_add(y))).ok_or(Fault::TooLarge)
}

/// `exclusive_scan(in, out, init, AbsSum)`; returns `AbsSum(out.back(), in.back())` (or `init` when empty).
fn abs_scan(input: &[i32], init: i32) -> R<(Vec<i32>, i32)> {
    let mut out = Vec::with_capacity(input.len());
    let mut acc = init;
    for &x in input {
        out.push(acc);
        acc = abs_sum(acc, x)?;
    }
    Ok((out, acc))
}

type EdgesP = BTreeMap<i32, Vec<EdgePos>>;
type EdgesNew = BTreeMap<(i32, i32), Vec<EdgePos>>;

/// `AddNewEdgeVerts`.
#[allow(clippy::too_many_arguments)]
fn add_new_edge_verts(
    edges_p: &mut EdgesP,
    edges_new: &mut EdgesNew,
    p1q2: &[[i32; 2]],
    i12: &[i32],
    v12r: &[i32],
    halfedge_p: &Halfedges,
    forward: bool,
    offset: usize,
) -> R<()> {
    for i in 0..p1q2.len() {
        let pq = p1q2.atu(i)?;
        let edge_p = if forward { pq[0] } else { pq[1] };
        let face_q = if forward { pq[1] } else { pq[0] };
        let vert = v12r.atu(i)?;
        let inclusion = i12.atu(i)?;

        let mut key_right = (halfedge_p.pair(edge_p)? / 3, face_q);
        if !forward {
            key_right = (key_right.1, key_right.0);
        }
        let mut key_left = (edge_p / 3, face_q);
        if !forward {
            key_left = (key_left.1, key_left.0);
        }
        // the three flags are taken before the C++ loop toggles `direction` (the toggle has no effect on them)
        let direction = inclusion < 0;
        let cid = ix(i + offset)?;
        let n = inclusion.checked_abs().ok_or(Fault::TooLarge)?;
        let push = |list: &mut Vec<EdgePos>, is_start: bool| {
            for j in 0..n {
                list.push(EdgePos { edge_pos: 0.0, vert: vert + j, collision_id: cid, is_start });
            }
        };
        push(edges_p.entry(edge_p).or_default(), direction);
        push(edges_new.entry(key_right).or_default(), direction ^ !forward);
        push(edges_new.entry(key_left).or_default(), direction ^ forward);
    }
    Ok(())
}

struct Output {
    face_he: Vec<Halfedge>,
    halfedge_ref: Vec<TriRef>,
    face_ptr: Vec<i32>,
}

impl Output {
    fn emit(&mut self, face_left: i32, face_right: i32, mut e: Halfedge, fwd: TriRef, bwd: TriRef) -> R<()> {
        let forward_edge = self.face_ptr.at(face_left)?;
        *self.face_ptr.at_mut(face_left)? += 1;
        let backward_edge = self.face_ptr.at(face_right)?;
        *self.face_ptr.at_mut(face_right)? += 1;
        e.paired_halfedge = backward_edge;
        self.face_he.set(forward_edge, e)?;
        self.halfedge_ref.set(forward_edge, fwd)?;
        core::mem::swap(&mut e.start_vert, &mut e.end_vert);
        e.paired_halfedge = forward_edge;
        self.face_he.set(backward_edge, e)?;
        self.halfedge_ref.set(backward_edge, bwd)
    }
}

/// `AppendPartialEdges`.
#[allow(clippy::too_many_arguments)]
fn append_partial_edges(
    out_r: &Impl,
    o: &mut Output,
    whole_he_p: &mut [bool],
    edges_p: &EdgesP,
    in_p: &Impl,
    i03: &[i32],
    vp2r: &[i32],
    face_p2r: &[i32],
    forward: bool,
) -> R<()> {
    let vert_pos_p = &in_p.vert_pos;
    let he_p = &in_p.halfedge;
    for (&edge_p, list) in edges_p {
        let mut ep: Vec<EdgePos> = list.clone();
        ep.sort_by(by_less(edge_pos_less));
        let pair_p = he_p.pair(edge_p)?;
        whole_he_p.set(edge_p, false)?;
        whole_he_p.set(pair_p, false)?;
        let v_start = he_p.start(edge_p)?;
        let v_end = he_p.end(edge_p)?;
        let edge_vec = vert_pos_p.at(v_end)? - vert_pos_p.at(v_start)?;
        for e in ep.iter_mut() {
            e.edge_pos = dot3(out_r.vert_pos.at(e.vert)?, edge_vec);
        }
        let inclusion = i03.at(v_start)?;
        let mut epos = EdgePos {
            edge_pos: dot3(out_r.vert_pos.at(vp2r.at(v_start)?)?, edge_vec),
            vert: vp2r.at(v_start)?,
            collision_id: i32::MAX,
            is_start: inclusion > 0,
        };
        for _ in 0..inclusion.checked_abs().ok_or(Fault::TooLarge)? {
            ep.push(epos);
            epos.vert += 1;
        }
        let inclusion = i03.at(v_end)?;
        let mut epos = EdgePos {
            edge_pos: dot3(out_r.vert_pos.at(vp2r.at(v_end)?)?, edge_vec),
            vert: vp2r.at(v_end)?,
            collision_id: i32::MAX,
            is_start: inclusion < 0,
        };
        for _ in 0..inclusion.checked_abs().ok_or(Fault::TooLarge)? {
            ep.push(epos);
            epos.vert += 1;
        }
        let face_left_p = edge_p / 3;
        let face_left = face_p2r.at(face_left_p)?;
        let face_right_p = pair_p / 3;
        let face_right = face_p2r.at(face_right_p)?;
        let fwd = TriRef { mesh_id: if forward { 0 } else { 1 }, original_id: -1, face_id: face_left_p, coplanar_id: -1 };
        let bwd = TriRef { mesh_id: if forward { 0 } else { 1 }, original_id: -1, face_id: face_right_p, coplanar_id: -1 };
        pair_up(&mut ep, |e| o.emit(face_left, face_right, e, fwd, bwd))?;
    }
    Ok(())
}

/// `AppendNewEdges`.
fn append_new_edges(out_r: &Impl, o: &mut Output, edges_new: &mut EdgesNew, face_pq2r: &[i32], num_face_p: i32) -> R<()> {
    for (&(face_p, face_q), ep) in edges_new.iter_mut() {
        ep.sort_by(by_less(edge_pos_less));
        let mut bbox = Box3::default();
        for e in ep.iter() {
            bbox.union_pt(out_r.vert_pos.at(e.vert)?);
        }
        let size = bbox.size();
        let i = if size.x > size.y && size.x > size.z {
            0
        } else if size.y > size.z {
            1
        } else {
            2
        };
        for e in ep.iter_mut() {
            e.edge_pos = out_r.vert_pos.at(e.vert)?.get(i);
        }
        let face_left = face_pq2r.at(face_p)?;
        let face_right = face_pq2r.at(num_face_p + face_q)?;
        let fwd = TriRef { mesh_id: 0, original_id: -1, face_id: face_p, coplanar_id: -1 };
        let bwd = TriRef { mesh_id: 1, original_id: -1, face_id: face_q, coplanar_id: -1 };
        pair_up(ep, |e| o.emit(face_left, face_right, e, fwd, bwd))?;
    }
    Ok(())
}

/// `AppendWholeEdges` (`DuplicateHalfedges` over every halfedge of P, in order).
#[allow(clippy::too_many_arguments)]
fn append_whole_edges(
    o: &mut Output,
    in_p: &Impl,
    whole_he_p: &[bool],
    i03: &[i32],
    vp2r: &[i32],
    face_p2r: &[i32],
    forward: bool,
) -> R<()> {
    let he_p = &in_p.halfedge;
    for idx in 0..ix(he_p.len())? {
        if !whole_he_p.at(idx)? {
            continue;
        }
        let mut start_vert = he_p.start(idx)?;
        let mut end_vert = he_p.start(next_halfedge(idx))?;
        if start_vert >= end_vert {
            continue;
        }
        let inclusion = i03.at(start_vert)?;
        if inclusion == 0 {
            continue;
        }
        if inclusion < 0 {
            core::mem::swap(&mut start_vert, &mut end_vert);
        }
        start_vert = vp2r.at(start_vert)?;
        end_vert = vp2r.at(end_vert)?;
        let prop_vert = he_p.prop(idx)?;
        let pair = he_p.pair(idx)?;
        let pair_prop = he_p.prop(pair)?;
        let face_left_p = idx / 3;
        let new_face = face_p2r.at(face_left_p)?;
        let face_right_p = pair / 3;
        let face_right = face_p2r.at(face_right_p)?;
        let fwd = TriRef { mesh_id: if forward { 0 } else { 1 }, original_id: -1, face_id: face_left_p, coplanar_id: -1 };
        let bwd = TriRef { mesh_id: if forward { 0 } else { 1 }, original_id: -1, face_id: face_right_p, coplanar_id: -1 };
        for _ in 0..inclusion.checked_abs().ok_or(Fault::TooLarge)? {
            let forward_edge = o.face_ptr.at(new_face)?;
            *o.face_ptr.at_mut(new_face)? += 1;
            let backward_edge = o.face_ptr.at(face_right)?;
            *o.face_ptr.at_mut(face_right)? += 1;
            o.face_he.set(forward_edge, Halfedge { start_vert, end_vert, paired_halfedge: backward_edge, prop_vert })?;
            o.face_he.set(
                backward_edge,
                Halfedge { start_vert: end_vert, end_vert: start_vert, paired_halfedge: forward_edge, prop_vert: pair_prop },
            )?;
            o.halfedge_ref.set(forward_edge, fwd)?;
            o.halfedge_ref.set(backward_edge, bwd)?;
            start_vert += 1;
            end_vert += 1;
        }
    }
    Ok(())
}

/// utils.h `GetBarycentric`.
fn get_barycentric(v: V3, tri: [V3; 3], tolerance: f64) -> V3 {
    use crate::linalg::cross3;
    let edges = [tri[2] - tri[1], tri[0] - tri[2], tri[1] - tri[0]];
    let d2 = [dot3(edges[0], edges[0]), dot3(edges[1], edges[1]), dot3(edges[2], edges[2])];
    let long_side = if d2[0] > d2[1] && d2[0] > d2[2] {
        0
    } else if d2[1] > d2[2] {
        1
    } else {
        2
    };
    let cross_p = cross3(edges[0], edges[1]);
    let area2 = dot3(cross_p, cross_p);
    let tol2 = tolerance * tolerance;
    let mut uvw = V3::splat(0.0);
    for i in 0..3 {
        let dv = v - tri.get(i).copied().unwrap_or_default();
        if dot3(dv, dv) < tol2 {
            uvw.set(i, 1.0);
            return uvw;
        }
    }
    let d2l = d2.get(long_side).copied().unwrap_or_default();
    if d2l < tol2 {
        V3 { x: 1.0, y: 0.0, z: 0.0 }
    } else if area2 > d2l * tol2 {
        for i in 0..3 {
            let j = next3(i);
            let cpv = cross3(edges.get(i).copied().unwrap_or_default(), v - tri.get(j).copied().unwrap_or_default());
            let area2v = dot3(cpv, cpv);
            uvw.set(i, if area2v < d2.get(i).copied().unwrap_or_default() * tol2 { 0.0 } else { dot3(cpv, cross_p) });
        }
        uvw / (uvw.x + uvw.y + uvw.z)
    } else {
        let next_v = next3(long_side);
        let alpha = dot3(v - tri.get(next_v).copied().unwrap_or_default(), edges.get(long_side).copied().unwrap_or_default()) / d2l;
        uvw.set(long_side, 0.0);
        uvw.set(next_v, 1.0 - alpha);
        let last_v = next3(next_v);
        uvw.set(last_v, alpha);
        uvw
    }
}

/// `CreateProperties`: barycentric interpolation of the inputs' properties onto the result.
fn create_properties(out_r: &mut Impl, in_p: &Impl, in_q: &Impl, invert_q: bool) -> R<()> {
    let num_prop_p = in_p.num_prop;
    let num_prop_q = in_q.num_prop;
    let num_prop = num_prop_p.max(num_prop_q);
    out_r.num_prop = num_prop;
    if num_prop == 0 {
        return Ok(());
    }
    let num_tri = out_r.num_tri();
    let mut bary = vec![V3::default(); out_r.halfedge.len()];
    for tri in 0..ix(num_tri)? {
        let r = out_r.mesh_relation.tri_ref.at(tri)?;
        if out_r.halfedge.start(3 * tri)? < 0 {
            continue;
        }
        let pq = r.mesh_id == 0;
        let (vp, he) = if pq { (&in_p.vert_pos, &in_p.halfedge) } else { (&in_q.vert_pos, &in_q.halfedge) };
        let tri_pos = [vp.at(he.start(3 * r.face_id)?)?, vp.at(he.start(3 * r.face_id + 1)?)?, vp.at(he.start(3 * r.face_id + 2)?)?];
        for i in 0..3 {
            let vert = out_r.halfedge.start(3 * tri + i)?;
            bary.set(3 * tri + i, get_barycentric(out_r.vert_pos.at(vert)?, tri_pos, out_r.epsilon))?;
        }
    }

    let id_miss_prop = ix(out_r.num_vert())?;
    let mut prop_idx: Vec<Vec<([i32; 3], i32)>> = vec![Vec::new(); out_r.num_vert() + 1];
    let mut prop_miss_idx = [vec![-1i32; in_q.num_prop_vert()], vec![-1i32; in_p.num_prop_vert()]];
    let np = ux(num_prop)?;
    out_r.properties = Vec::with_capacity(out_r.num_vert().saturating_mul(np).min(1 << 26));
    let mut idx = 0i32;

    for tri in 0..ix(num_tri)? {
        if out_r.halfedge.start(3 * tri)? < 0 {
            continue;
        }
        let r = out_r.mesh_relation.tri_ref.at(tri)?;
        let pq = r.mesh_id == 0;
        let old_num_prop = if pq { num_prop_p } else { num_prop_q };
        let (properties, he) = if pq { (&in_p.properties, &in_p.halfedge) } else { (&in_q.properties, &in_q.halfedge) };
        let negate_normals = !pq
            && invert_q
            && old_num_prop >= 3
            && in_q
                .mesh_relation
                .mesh_id_transform
                .get(&in_q.mesh_relation.tri_ref.at(r.face_id)?.mesh_id)
                .is_some_and(|rel| rel.has_normals);
        for i in 0..3 {
            let vert = out_r.halfedge.start(3 * tri + i)?;
            let uvw = bary.at(3 * tri + i)?;
            let mut key = [i32::from(pq), id_miss_prop, -1, -1];
            if old_num_prop > 0 {
                let mut edge: i32 = -2;
                for j in 0..3 {
                    if uvw.get(j) == 1.0 {
                        // On a retained vert, the propVert must also match
                        key[2] = he.prop(3 * r.face_id + j as i32)?;
                        edge = -1;
                        break;
                    }
                    if uvw.get(j) == 0.0 {
                        edge = j as i32;
                    }
                }
                if edge >= 0 {
                    // On an edge, both propVerts must match
                    let e = ux(edge)?;
                    let p0 = he.prop(3 * r.face_id + next3(e) as i32)?;
                    let p1 = he.prop(3 * r.face_id + prev3(e) as i32)?;
                    key[1] = vert;
                    key[2] = p0.min(p1);
                    key[3] = p0.max(p1);
                } else if edge == -2 {
                    key[1] = vert;
                }
            }

            if key[1] == id_miss_prop && key[2] >= 0 {
                let entry = prop_miss_idx.get_mut(ux(key[0])?).ok_or(Fault::Index)?.at_mut(key[2])?;
                if *entry >= 0 {
                    let e = *entry;
                    out_r.halfedge.set_prop(3 * tri + i, e)?;
                    continue;
                }
                *entry = idx;
            } else {
                let bin = prop_idx.get_mut(ux(key[1])?).ok_or(Fault::Index)?;
                let k3 = [key[0], key[2], key[3]];
                if let Some(&(_, found)) = bin.iter().find(|b| b.0 == k3) {
                    out_r.halfedge.set_prop(3 * tri + i, found)?;
                    continue;
                }
                bin.push((k3, idx));
            }
            out_r.halfedge.set_prop(3 * tri + i, idx)?;
            idx += 1;
            for p in 0..num_prop {
                if p < old_num_prop {
                    let mut old = V3::default();
                    for j in 0..3 {
                        old.set(j, properties.at(old_num_prop * he.prop(3 * r.face_id + j as i32)? + p)?);
                    }
                    let mut val = dot3(uvw, old);
                    if negate_normals && p < 3 {
                        val = -val;
                    }
                    out_r.properties.push(val);
                } else {
                    out_r.properties.push(0.0);
                }
            }
        }
    }
    Ok(())
}

/// `UpdateReference`: triangle references mapped back to the inputs (Q's mesh IDs offset by the ID counter).
fn update_reference(out_r: &mut Impl, in_p: &Impl, in_q: &Impl, invert_q: bool) -> R<()> {
    let offset_q = mesh_id_counter() as i32;
    let nt = out_r.num_tri();
    for t in 0..nt {
        let r = out_r.mesh_relation.tri_ref.atu(t)?;
        let tri = r.face_id;
        let pq = r.mesh_id == 0;
        let mut nr = if pq { in_p.mesh_relation.tri_ref.at(tri)? } else { in_q.mesh_relation.tri_ref.at(tri)? };
        if !pq {
            nr.mesh_id = nr.mesh_id.wrapping_add(offset_q);
        }
        out_r.mesh_relation.tri_ref.setu(t, nr)?;
    }
    for (&k, rel) in &in_p.mesh_relation.mesh_id_transform {
        out_r.mesh_relation.mesh_id_transform.insert(k, *rel);
    }
    for (&k, rel) in &in_q.mesh_relation.mesh_id_transform {
        let mut r = *rel;
        r.back_side ^= invert_q;
        out_r.mesh_relation.mesh_id_transform.insert(k.wrapping_add(offset_q), r);
    }
    Ok(())
}

impl Boolean3<'_> {
    /// `Boolean3::Result(op)`.
    pub(crate) fn result(&self, op: OpType) -> R<Impl> {
        let in_p = self.in_p;
        let in_q = self.in_q;
        let c1 = if op == OpType::Intersect { 0 } else { 1 };
        let c2 = if op == OpType::Add { 1 } else { 0 };
        let c3 = if op == OpType::Intersect { 1 } else { -1 };

        if in_p.status != Status::NoError {
            return Ok(Impl::with_status(in_p.status));
        }
        if in_q.status != Status::NoError {
            return Ok(Impl::with_status(in_q.status));
        }
        if in_p.is_empty() {
            if !in_q.is_empty() && op == OpType::Add {
                return Ok(in_q.clone());
            }
            return Ok(Impl::default());
        } else if in_q.is_empty() {
            if op == OpType::Intersect {
                return Ok(Impl::default());
            }
            return Ok(in_p.clone());
        }
        if !self.valid {
            return Ok(Impl::with_status(Status::ResultTooLarge));
        }
        let invert_q = op == OpType::Subtract;

        let mul = |a: i32, b: i32| a.checked_mul(b).ok_or(Fault::TooLarge);
        let i12: Vec<i32> = self.xv12.x12.iter().map(|&v| mul(c3, v)).collect::<R<_>>()?;
        let i21: Vec<i32> = self.xv21.x12.iter().map(|&v| mul(c3, v)).collect::<R<_>>()?;
        let i03: Vec<i32> =
            self.w03.iter().map(|&v| mul(c3, v).and_then(|x| x.checked_add(c1).ok_or(Fault::TooLarge))).collect::<R<_>>()?;
        let i30: Vec<i32> =
            self.w30.iter().map(|&v| mul(c3, v).and_then(|x| x.checked_add(c2).ok_or(Fault::TooLarge))).collect::<R<_>>()?;

        let (vp2r, n_pv) = abs_scan(&i03, 0)?;
        let (vq2r, after_q) = abs_scan(&i30, n_pv)?;
        let mut num_vert_r = after_q;
        let n_qv = num_vert_r - n_pv;
        let mut v12r = Vec::new();
        if !self.xv12.v12.is_empty() {
            let (o, n) = abs_scan(&i12, num_vert_r)?;
            v12r = o;
            num_vert_r = n;
        }
        let mut v21r = Vec::new();
        if !self.xv21.v12.is_empty() {
            let (o, n) = abs_scan(&i21, num_vert_r)?;
            v21r = o;
            num_vert_r = n;
        }
        if ux(num_vert_r)? > crate::MAX_RESULT_VERTS {
            return Err(Fault::TooLarge);
        }

        let mut out_r = Impl::default();
        if num_vert_r == 0 {
            return Ok(out_r);
        }
        out_r.epsilon = std_max(in_p.epsilon, in_q.epsilon);
        out_r.tolerance = std_max(in_p.tolerance, in_q.tolerance);

        out_r.vert_pos = vec![V3::default(); ux(num_vert_r)?];
        let dup = |out: &mut Vec<V3>, inc: &[i32], vr: &[i32], pos: &[V3]| -> R<()> {
            for vert in 0..inc.len() {
                let n = inc.atu(vert)?.checked_abs().ok_or(Fault::TooLarge)?;
                let base = vr.atu(vert)?;
                let p = pos.atu(vert)?;
                for i in 0..n {
                    out.set(base + i, p)?;
                }
            }
            Ok(())
        };
        dup(&mut out_r.vert_pos, &i03, &vp2r, &in_p.vert_pos)?;
        dup(&mut out_r.vert_pos, &i30, &vq2r, &in_q.vert_pos)?;
        dup(&mut out_r.vert_pos, &i12, &v12r, &self.xv12.v12)?;
        dup(&mut out_r.vert_pos, &i21, &v21r, &self.xv21.v12)?;

        let mut edges_p: EdgesP = BTreeMap::new();
        let mut edges_q: EdgesP = BTreeMap::new();
        let mut edges_new: EdgesNew = BTreeMap::new();
        add_new_edge_verts(&mut edges_p, &mut edges_new, &self.xv12.p1q2, &i12, &v12r, &in_p.halfedge, true, 0)?;
        add_new_edge_verts(&mut edges_q, &mut edges_new, &self.xv21.p1q2, &i21, &v21r, &in_q.halfedge, false, self.xv12.p1q2.len())?;

        // SizeOutput
        let ntp = in_p.num_tri();
        let ntq = in_q.num_tri();
        let mut sides = vec![0i32; ntp + ntq];
        for i in 0..ntp {
            let mut c = 0i32;
            for j in 0..3 {
                c = c
                    .checked_add(i03.at(in_p.halfedge.start(3 * i as i32 + j)?)?.checked_abs().ok_or(Fault::TooLarge)?)
                    .ok_or(Fault::TooLarge)?;
            }
            *sides.atu_mut(i)? += c;
        }
        for i in 0..ntq {
            let mut c = 0i32;
            for j in 0..3 {
                c = c
                    .checked_add(i30.at(in_q.halfedge.start(3 * i as i32 + j)?)?.checked_abs().ok_or(Fault::TooLarge)?)
                    .ok_or(Fault::TooLarge)?;
            }
            *sides.atu_mut(ntp + i)? += c;
        }
        let count_new = |sides: &mut Vec<i32>, inc: &[i32], pq: &[[i32; 2]], he: &Halfedges, inverted: bool| -> R<()> {
            let (off_p, off_q) = if inverted { (ntp, 0) } else { (0, ntp) };
            for idx in 0..inc.len() {
                let pair = pq.atu(idx)?;
                let edge_p = if inverted { pair[1] } else { pair[0] };
                let face_q = if inverted { pair[0] } else { pair[1] };
                let inclusion = inc.atu(idx)?.checked_abs().ok_or(Fault::TooLarge)?;
                let add = |s: &mut Vec<i32>, k: usize| -> R<()> {
                    let c = s.atu_mut(k)?;
                    *c = c.checked_add(inclusion).ok_or(Fault::TooLarge)?;
                    Ok(())
                };
                add(sides, off_q + ux(face_q)?)?;
                add(sides, off_p + ux(edge_p / 3)?)?;
                add(sides, off_p + ux(he.pair(edge_p)? / 3)?)?;
            }
            Ok(())
        };
        count_new(&mut sides, &i12, &self.xv12.p1q2, &in_p.halfedge, false)?;
        count_new(&mut sides, &i21, &self.xv21.p1q2, &in_q.halfedge, true)?;

        let mut face_pq2r = vec![0i32; ntp + ntq];
        let mut num_face_r = 0i32;
        for (i, &s) in sides.iter().enumerate() {
            face_pq2r.setu(i, num_face_r)?;
            if s > 0 {
                num_face_r += 1;
            }
        }
        let mut face_normal = Vec::with_capacity(ux(num_face_r)?);
        for (i, &s) in sides.iter().enumerate().take(ntp) {
            if s > 0 {
                face_normal.push(in_p.face_normal.atu(i)?);
            }
        }
        for i in 0..ntq {
            if sides.atu(ntp + i)? > 0 {
                let n = in_q.face_normal.atu(i)?;
                face_normal.push(if invert_q { -n } else { n });
            }
        }
        out_r.face_normal = face_normal;
        let mut face_edge = Vec::with_capacity(ux(num_face_r)? + 1);
        face_edge.push(0i32);
        let mut acc = 0i32;
        for &s in sides.iter().filter(|&&s| s != 0) {
            acc = acc.checked_add(s).ok_or(Fault::TooLarge)?;
            face_edge.push(acc);
        }
        let total_he = ux(*face_edge.last().ok_or(Fault::Index)?)?;
        if total_he > 4 * crate::MAX_RESULT_VERTS {
            return Err(Fault::TooLarge);
        }

        let mut o = Output {
            face_he: vec![Halfedge::default(); total_he],
            halfedge_ref: vec![TriRef::default(); total_he],
            face_ptr: face_edge.clone(),
        };
        let mut whole_p = vec![true; in_p.halfedge.len()];
        let mut whole_q = vec![true; in_q.halfedge.len()];
        let (fp2r, fq2r) = face_pq2r.split_at(ntp);
        append_partial_edges(&out_r, &mut o, &mut whole_p, &edges_p, in_p, &i03, &vp2r, fp2r, true)?;
        append_partial_edges(&out_r, &mut o, &mut whole_q, &edges_q, in_q, &i30, &vq2r, fq2r, false)?;
        append_new_edges(&out_r, &mut o, &mut edges_new, &face_pq2r, ix(ntp)?)?;
        append_whole_edges(&mut o, in_p, &whole_p, &i03, &vp2r, fp2r, true)?;
        append_whole_edges(&mut o, in_q, &whole_q, &i30, &vq2r, fq2r, false)?;

        out_r.face2tri(&face_edge, &o.face_he, &o.halfedge_ref)?;
        out_r.reorder_halfedges()?;
        create_properties(&mut out_r, in_p, in_q, invert_q)?;
        update_reference(&mut out_r, in_p, in_q, invert_q)?;

        out_r.simplify_topology(n_pv + n_qv)?;
        out_r.remove_unreferenced_verts()?;
        out_r.calculate_bbox();
        out_r.sort_geometry()?;
        out_r.increment_mesh_ids()?;
        Ok(out_r)
    }
}

#[cfg(test)]
mod tests {
    use super::libcxx_partition;

    #[test]
    fn partition_matches_libcxx() {
        // libc++: swap the first false from the front with the last true from the back
        let mut v = [false, true, false, true, true];
        let m = libcxx_partition(&mut v, |&x| x);
        assert_eq!(m, 3);
        assert_eq!(v, [true, true, true, false, false]);
        let mut w = [(1, true), (2, false), (3, true), (4, false), (5, true)];
        let m = libcxx_partition(&mut w, |x| x.1);
        assert_eq!(m, 3);
        assert_eq!(w, [(1, true), (5, true), (3, true), (4, false), (2, false)]);
    }
}
