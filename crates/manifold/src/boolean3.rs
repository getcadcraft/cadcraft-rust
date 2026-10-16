// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! boolean3.cpp: the symbolic-perturbation kernels (Shadow01, Kernel11, Kernel02, Kernel12), the edge × face
//! intersections in both directions (Intersect12) and the vertex winding numbers by flood fill (Winding03).
//!
//! Notation as in the C++: P and Q are the inputs, R the result; 0 = vertex, 1 = edge, 2 = face. `expand_p` and
//! `forward` are runtime flags here (template parameters in the C++); the arithmetic is unchanged.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use crate::OpType;
use crate::disjoint::DisjointSets;
use crate::halfedge::{Halfedges, next3};
use crate::imp::Impl;
use crate::linalg::{Box3, V2, V3, V4, v2, v3};
use crate::util::{Fault, Get, GetMut, R, ix, ux};

/// The k-th of the two left / right slots (k < 2 is checked by every caller; anything else lands in the second).
#[inline(always)]
fn slot<T>(arr: &mut [T; 2], k: usize) -> &mut T {
    let [a, b] = arr;
    if k == 0 { a } else { b }
}

#[inline]
fn with_sign(pos: bool, v: f64) -> f64 {
    if pos { v } else { -v }
}

/// shared.h `Shadows`: `p < q` with `dir < 0` as the tiebreaker when they are equal.
#[inline]
fn shadows(p: f64, q: f64, dir: f64) -> bool {
    if p == q { dir < 0.0 } else { p < q }
}

/// shared.h `Interpolate`: (y, z) of segment aL-aR at x, from the closer end.
#[inline(always)]
fn interpolate(a_l: V3, a_r: V3, x: f64) -> V2 {
    let dx_l = x - a_l.x;
    let dx_r = x - a_r.x;
    let use_l = dx_l.abs() < dx_r.abs();
    let d_lr = a_r - a_l;
    let lambda = (if use_l { dx_l } else { dx_r }) / d_lr.x;
    if !lambda.is_finite() || !d_lr.y.is_finite() || !d_lr.z.is_finite() {
        return v2(a_l.y, a_l.z);
    }
    v2(lambda * d_lr.y + if use_l { a_l.y } else { a_r.y }, lambda * d_lr.z + if use_l { a_l.z } else { a_r.z })
}

/// boolean3.cpp `Intersect`: the crossing of two segments in the xy plane, with both z values.
#[inline(always)]
fn intersect(a_l: V3, a_r: V3, b_l: V3, b_r: V3) -> V4 {
    let dy_l = b_l.y - a_l.y;
    let dy_r = b_r.y - a_r.y;
    let use_l = dy_l.abs() < dy_r.abs();
    let dx = a_r.x - a_l.x;
    let mut lambda = (if use_l { dy_l } else { dy_r }) / (dy_l - dy_r);
    if !lambda.is_finite() {
        lambda = 0.0;
    }
    let x = lambda * dx + if use_l { a_l.x } else { a_r.x };
    let a_dy = a_r.y - a_l.y;
    let b_dy = b_r.y - b_l.y;
    let use_a = a_dy.abs() < b_dy.abs();
    let y = lambda * (if use_a { a_dy } else { b_dy })
        + if use_l {
            if use_a { a_l.y } else { b_l.y }
        } else if use_a {
            a_r.y
        } else {
            b_r.y
        };
    let z = lambda * (a_r.z - a_l.z) + if use_l { a_l.z } else { a_r.z };
    let w = lambda * (b_r.z - b_l.z) + if use_l { b_l.z } else { b_r.z };
    V4 { x, y, z, w }
}

#[derive(Clone, Copy, Default)]
struct FaceEdge {
    edge: i32,
    start: i32,
    end: i32,
    is_forward: bool,
}

#[inline]
fn load_face_edges(he: &Halfedges, tri: i32) -> R<[FaceEdge; 3]> {
    let mut out = [FaceEdge::default(); 3];
    for (i, o) in out.iter_mut().enumerate() {
        let halfedge = 3 * tri + i as i32;
        let start = he.start(halfedge)?;
        let end = he.start(3 * tri + next3(i) as i32)?;
        *o = if start < end {
            FaceEdge { edge: halfedge, start, end, is_forward: true }
        } else {
            FaceEdge { edge: he.pair(halfedge)?, start: end, end: start, is_forward: false }
        };
    }
    Ok(out)
}

/// `Shadow01<expandP, forward>`.
#[inline(always)]
fn shadow01<const EXPAND_P: bool, const FORWARD: bool>(a0: i32, b1: i32, b1s: i32, b1e: i32, in_a: &Impl, in_b: &Impl) -> R<(i32, V2)> {
    let (expand_p, forward) = (EXPAND_P, FORWARD);
    let a0p = in_a.vert_pos.at(a0)?;
    let a0x = a0p.x;
    let b1sp = in_b.vert_pos.at(b1s)?;
    let b1ep = in_b.vert_pos.at(b1e)?;
    let b1sx = b1sp.x;
    let b1ex = b1ep.x;
    let a0xp = in_a.vert_normal.at(a0)?.x;
    let b1sxp = in_b.vert_normal.at(b1s)?.x;
    let b1exp = in_b.vert_normal.at(b1e)?.x;
    let mut s01 = if forward {
        i32::from(shadows(a0x, b1ex, with_sign(expand_p, a0xp) - b1exp)) - i32::from(shadows(a0x, b1sx, with_sign(expand_p, a0xp) - b1sxp))
    } else {
        i32::from(shadows(b1sx, a0x, with_sign(expand_p, b1sxp) - a0xp)) - i32::from(shadows(b1ex, a0x, with_sign(expand_p, b1exp) - a0xp))
    };
    let mut yz01 = V2::splat(f64::NAN);
    if s01 != 0 {
        yz01 = interpolate(b1sp, b1ep, a0x);
        let b1pair = in_b.halfedge.pair(b1)?;
        let dir = in_b.face_normal.at(b1 / 3)?.y + in_b.face_normal.at(b1pair / 3)?.y;
        if forward {
            if !shadows(a0p.y, yz01.x, -dir) {
                s01 = 0;
            }
        } else if !shadows(yz01.x, a0p.y, with_sign(expand_p, dir)) {
            s01 = 0;
        }
    }
    Ok((s01, yz01))
}

/// `Kernel11<expandP>`: edge of P × edge of Q.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn kernel11<const EXPAND_P: bool>(in_p: &Impl, in_q: &Impl, p1: i32, p1s: i32, p1e: i32, q1: i32, q1s: i32, q1e: i32) -> R<(i32, V4)> {
    let expand_p = EXPAND_P;
    let mut s11 = 0;
    let mut k = 0usize;
    let mut p_rl = [V3::default(); 2];
    let mut q_rl = [V3::default(); 2];
    let mut shadows_flag = false;

    for (p0, sign) in [(p1s, -1), (p1e, 1)] {
        let (s01, yz01) = shadow01::<EXPAND_P, true>(p0, q1, q1s, q1e, in_p, in_q)?;
        if yz01.x.is_finite() {
            s11 += s01 * sign;
            if k < 2 && (k == 0 || (s01 != 0) != shadows_flag) {
                shadows_flag = s01 != 0;
                let pp = in_p.vert_pos.at(p0)?;
                *slot(&mut p_rl, k) = pp;
                *slot(&mut q_rl, k) = v3(pp.x, yz01.x, yz01.y);
                k += 1;
            }
        }
    }
    for (q0, sign) in [(q1s, -1), (q1e, 1)] {
        let (s10, yz10) = shadow01::<EXPAND_P, false>(q0, p1, p1s, p1e, in_q, in_p)?;
        if yz10.x.is_finite() {
            s11 += s10 * sign;
            if k < 2 && (k == 0 || (s10 != 0) != shadows_flag) {
                shadows_flag = s10 != 0;
                let qp = in_q.vert_pos.at(q0)?;
                *slot(&mut q_rl, k) = qp;
                *slot(&mut p_rl, k) = v3(qp.x, yz10.x, yz10.y);
                k += 1;
            }
        }
    }
    if s11 == 0 {
        return Ok((0, V4 { x: f64::NAN, y: f64::NAN, z: f64::NAN, w: f64::NAN }));
    }
    // k == 2 on valid input ("Boolean manifold error: s11" otherwise); the C++ reads stale values, here they are zero
    let xyzz = intersect(p_rl[0], p_rl[1], q_rl[0], q_rl[1]);
    let p1pair = in_p.halfedge.pair(p1)?;
    let dir_p = in_p.face_normal.at(p1 / 3)?.z + in_p.face_normal.at(p1pair / 3)?.z;
    let q1pair = in_q.halfedge.pair(q1)?;
    let dir_q = in_q.face_normal.at(q1 / 3)?.z + in_q.face_normal.at(q1pair / 3)?.z;
    if !shadows(xyzz.z, xyzz.w, with_sign(expand_p, dir_p) - dir_q) {
        s11 = 0;
    }
    Ok((s11, xyzz))
}

/// `Kernel02<expandP, forward>`: vertex of A × face of B.
#[inline(always)]
fn kernel02<const EXPAND_P: bool, const FORWARD: bool>(
    in_a: &Impl,
    in_b: &Impl,
    a0: i32,
    b2: i32,
    edge_b: &[FaceEdge; 3],
) -> R<(i32, f64)> {
    let (expand_p, forward) = (EXPAND_P, FORWARD);
    let mut s02 = 0;
    let mut k = 0usize;
    let mut yzz_rl = [V3::default(); 2];
    let mut shadows_flag = false;
    for eb in edge_b {
        let (s01, yz01) = shadow01::<EXPAND_P, FORWARD>(a0, eb.edge, eb.start, eb.end, in_a, in_b)?;
        if yz01.x.is_finite() {
            s02 += s01 * if forward == eb.is_forward { -1 } else { 1 };
            if k < 2 && (k == 0 || (s01 != 0) != shadows_flag) {
                shadows_flag = s01 != 0;
                *slot(&mut yzz_rl, k) = v3(yz01.x, yz01.y, yz01.y);
                k += 1;
            }
        }
    }
    if s02 == 0 {
        return Ok((0, f64::NAN));
    }
    let a = in_a.vert_pos.at(a0)?;
    let z02 = interpolate(yzz_rl[0], yzz_rl[1], a.y).y;
    let fz = in_b.face_normal.at(b2)?.z;
    if forward {
        if !shadows(a.z, z02, -fz) {
            s02 = 0;
        }
    } else if !shadows(z02, a.z, with_sign(expand_p, fz)) {
        s02 = 0;
    }
    Ok((s02, z02))
}

/// `Kernel12<expandP, forward>`: edge of A × face of B.
#[inline]
fn kernel12<const EXPAND_P: bool, const FORWARD: bool>(in_p: &Impl, in_q: &Impl, a1: i32, b2: i32) -> R<(i32, V3)> {
    let forward = FORWARD;
    let (in_a, in_b) = if forward { (in_p, in_q) } else { (in_q, in_p) };
    let mut x12 = 0;
    let mut k = 0usize;
    let mut xzy_lr0 = [V3::default(); 2];
    let mut xzy_lr1 = [V3::default(); 2];
    let mut shadows_flag = false;

    let edge_a_start = in_a.halfedge.start(a1)?;
    let edge_a_end = in_a.halfedge.end(a1)?;
    let edge_b = load_face_edges(&in_b.halfedge, b2)?;

    for vert_a in [edge_a_start, edge_a_end] {
        let (s, z) = kernel02::<EXPAND_P, FORWARD>(in_a, in_b, vert_a, b2, &edge_b)?;
        if z.is_finite() {
            x12 += s * if (vert_a == edge_a_start) == forward { 1 } else { -1 };
            if k < 2 && (k == 0 || (s != 0) != shadows_flag) {
                shadows_flag = s != 0;
                let mut p = in_a.vert_pos.at(vert_a)?;
                core::mem::swap(&mut p.y, &mut p.z);
                *slot(&mut xzy_lr0, k) = p;
                let l1 = slot(&mut xzy_lr1, k);
                *l1 = p;
                l1.y = z;
                k += 1;
            }
        }
    }
    for eb in &edge_b {
        let (s, xyzz) = if forward {
            kernel11::<EXPAND_P>(in_p, in_q, a1, edge_a_start, edge_a_end, eb.edge, eb.start, eb.end)?
        } else {
            kernel11::<EXPAND_P>(in_p, in_q, eb.edge, eb.start, eb.end, a1, edge_a_start, edge_a_end)?
        };
        if xyzz.x.is_finite() {
            x12 -= s * if eb.is_forward { 1 } else { -1 };
            if k < 2 && (k == 0 || (s != 0) != shadows_flag) {
                shadows_flag = s != 0;
                let mut l0 = v3(xyzz.x, xyzz.z, xyzz.y);
                let mut l1 = l0;
                l1.y = xyzz.w;
                if !forward {
                    core::mem::swap(&mut l0.y, &mut l1.y);
                }
                *slot(&mut xzy_lr0, k) = l0;
                *slot(&mut xzy_lr1, k) = l1;
                k += 1;
            }
        }
    }
    if x12 == 0 {
        return Ok((0, V3::splat(f64::NAN)));
    }
    let xzyy = intersect(xzy_lr0[0], xzy_lr0[1], xzy_lr1[0], xzy_lr1[1]);
    Ok((x12, v3(xzyy.x, xzyy.z, xzyy.y)))
}

/// boolean3.h `Intersections`: in forward mode edges of P × faces of Q; in reverse faces of P × edges of Q.
#[derive(Clone, Debug, Default)]
pub(crate) struct Intersections {
    pub p1q2: Vec<[i32; 2]>,
    pub x12: Vec<i32>,
    pub v12: Vec<V3>,
}

/// `Intersect12<forward>`.
fn intersect12<const EXPAND_P: bool, const FORWARD: bool>(in_p: &Impl, in_q: &Impl) -> R<Intersections> {
    let forward = FORWARD;
    let (a, b) = if forward { (in_p, in_q) } else { (in_q, in_p) };
    let mut res = Intersections::default();
    let n = a.halfedge.len();
    b.collider.collisions_box(
        false,
        n,
        |i| {
            let s = a.halfedge.start(i)?;
            let e = a.halfedge.end(i)?;
            Ok(if s < e { Box3::from_points(a.vert_pos.at(s)?, a.vert_pos.at(e)?) } else { Box3::default() })
        },
        |q, leaf| {
            let (x12, v12) = kernel12::<EXPAND_P, FORWARD>(in_p, in_q, q, leaf)?;
            if v12.x.is_finite() {
                if res.x12.len() >= crate::MAX_INTERSECTIONS {
                    return Err(Fault::TooLarge);
                }
                res.p1q2.push(if forward { [q, leaf] } else { [leaf, q] });
                res.x12.push(x12);
                res.v12.push(v12);
            }
            Ok(())
        },
    )?;
    // sort by edge, then face
    let p1q2 = &res.p1q2;
    let mut order: Vec<i32> = (0..ix(p1q2.len())?).collect();
    order.sort_by_key(|&i| {
        let [p, q] = p1q2.get(i as usize).copied().unwrap_or([i32::MAX, i32::MAX]);
        if forward { (p, q) } else { (q, p) }
    });
    crate::util::permute(&mut res.p1q2, &order)?;
    crate::util::permute(&mut res.x12, &order)?;
    crate::util::permute(&mut res.v12, &order)?;
    Ok(res)
}

/// `Winding03<forward>`: winding numbers of A's vertices with respect to B, one Kernel02 test per connected
/// component of unbroken edges, then flood-filled.
fn winding03<const EXPAND_P: bool, const FORWARD: bool>(in_p: &Impl, in_q: &Impl, p1q2: &[[i32; 2]]) -> R<Vec<i32>> {
    let forward = FORWARD;
    let (a, b) = if forward { (in_p, in_q) } else { (in_q, in_p) };
    let edge_of = |cp: &[i32; 2]| if forward { cp[0] } else { cp[1] };
    let nv = a.vert_pos.len();
    let mut ua = DisjointSets::new(nv)?;
    for edge in 0..ix(a.halfedge.len())? {
        let start = a.halfedge.start(edge)?;
        let end = a.halfedge.end(edge)?;
        if start >= end {
            continue;
        }
        // is the edge broken (intersected)?
        let it = p1q2.partition_point(|cp| edge_of(cp) < edge);
        if it == p1q2.len() || edge_of(&p1q2.atu(it)?) != edge {
            ua.unite(ux(start)?, ux(end)?)?;
        }
    }
    // one representative per component; the order of the set does not change the per-vertex sums
    let mut is_root = vec![false; nv];
    for v in 0..nv {
        let r = ua.find(v)?;
        is_root.setu(r, true)?;
    }
    let verts: Vec<i32> = (0..nv).filter(|&v| is_root.get(v).copied().unwrap_or(false)).map(|v| v as i32).collect();

    let mut w03 = vec![0i32; a.num_vert()];
    b.collider.collisions_pt(
        verts.len(),
        |i| a.vert_pos.at(verts.at(i)?),
        |i, b2| {
            let edge_b = load_face_edges(&b.halfedge, b2)?;
            let v = verts.at(i)?;
            let (s02, z02) = kernel02::<EXPAND_P, FORWARD>(a, b, v, b2, &edge_b)?;
            if z02.is_finite() {
                *w03.at_mut(v)? += s02 * if forward { 1 } else { -1 };
            }
            Ok(())
        },
    )?;
    for i in 0..w03.len() {
        let root = ua.find(i)?;
        if root == i {
            continue;
        }
        let w = w03.atu(root)?;
        w03.setu(i, w)?;
    }
    Ok(w03)
}

/// `Boolean3`: the intersections and winding numbers of P against Q (and Q against P).
pub(crate) struct Boolean3<'a> {
    pub in_p: &'a Impl,
    pub in_q: &'a Impl,
    pub xv12: Intersections,
    pub xv21: Intersections,
    pub w03: Vec<i32>,
    pub w30: Vec<i32>,
    pub valid: bool,
}

impl<'a> Boolean3<'a> {
    pub(crate) fn new(in_p: &'a Impl, in_q: &'a Impl, op: OpType) -> R<Boolean3<'a>> {
        let expand_p = op == OpType::Add;
        let mut b = Boolean3 {
            in_p,
            in_q,
            xv12: Intersections::default(),
            xv21: Intersections::default(),
            w03: Vec::new(),
            w30: Vec::new(),
            valid: true,
        };
        if in_p.is_empty() || in_q.is_empty() || !in_p.bbox.does_overlap(&in_q.bbox) {
            b.w03 = vec![0; in_p.num_vert()];
            b.w30 = vec![0; in_q.num_vert()];
            return Ok(b);
        }
        if expand_p {
            b.xv12 = intersect12::<true, true>(in_p, in_q)?;
            b.xv21 = intersect12::<true, false>(in_p, in_q)?;
            b.w03 = winding03::<true, true>(in_p, in_q, &b.xv12.p1q2)?;
            b.w30 = winding03::<true, false>(in_p, in_q, &b.xv21.p1q2)?;
        } else {
            b.xv12 = intersect12::<false, true>(in_p, in_q)?;
            b.xv21 = intersect12::<false, false>(in_p, in_q)?;
            b.w03 = winding03::<false, true>(in_p, in_q, &b.xv12.p1q2)?;
            b.w30 = winding03::<false, false>(in_p, in_q, &b.xv21.p1q2)?;
        }
        Ok(b)
    }
}
