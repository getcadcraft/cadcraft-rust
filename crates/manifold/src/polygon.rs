// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! polygon.cpp, polygon_internal.h and tree2d.cpp: the ear-clipping triangulator (Eberly's method with epsilon-aware
//! convexity, holes keyholed into their outer polygons, ears clipped sharpest first) and its 2D k-d tree.
//!
//! The circular list is a vector of `Vert` linked by index (the C++ uses iterators into a vector reserved up front,
//! which never reallocates, so pointer order = index order). The ear queue (`std::multiset` ordered by cost, equal
//! costs in insertion order) is a `BTreeMap` keyed by (cost, insertion number). Only the general (non-convex) path is
//! ported: the boolean triangulates with `allowConvex = false`.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::halfedge::Halfedge;
use crate::imp::K_PRECISION;
use crate::linalg::{Rect, V2, ccw, det2, dot2, length2, normalize2, v2};
use crate::util::{Fault, FxMap, Get, GetMut, Guard, R, by_less, ix, std_max, std_min};

const K_BEST: f64 = f64::NEG_INFINITY;
const NONE: usize = usize::MAX;

/// polygon.h `PolyVert`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PolyVert {
    pub pos: V2,
    pub idx: i32,
}

/// polygon_internal.h `HalfedgeTriangulation`: contour halfedges (opposite the filled contour) then triangles.
/// `edge2halfedge` (an edge key → a stack of unpaired halfedges) is the top of each stack in a map, the rest linked
/// through `below` (no allocation per key; same push / pop-back order).
#[derive(Debug, Default)]
pub(crate) struct HalfedgeTriangulation {
    pub halfedges: Vec<Halfedge>,
    pub contour_end: usize,
    top: FxMap<u64, i32>,
    below: Vec<i32>,
}

impl HalfedgeTriangulation {
    fn add_contours(&mut self, polys: &[Vec<PolyVert>]) -> R<()> {
        for poly in polys {
            let n = poly.len();
            for i in 0..n {
                let start = poly.atu(i)?.idx;
                let end = poly.atu(if i + 1 < n { i + 1 } else { 0 })?.idx;
                self.add_halfedge(end, start)?;
            }
        }
        self.contour_end = self.halfedges.len();
        Ok(())
    }

    fn add_triangle(&mut self, a: i32, b: i32, c: i32) -> R<()> {
        self.add_halfedge(a, b)?;
        self.add_halfedge(b, c)?;
        self.add_halfedge(c, a)
    }

    pub(crate) fn num_tri(&self) -> usize {
        (self.halfedges.len() - self.contour_end) / 3
    }

    #[inline]
    fn key(start: i32, end: i32) -> u64 {
        (u64::from(start as u32) << 32) | u64::from(end as u32)
    }

    fn add_halfedge(&mut self, start: i32, end: i32) -> R<()> {
        let he = ix(self.halfedges.len())?;
        let mut data = Halfedge { start_vert: start, end_vert: end, paired_halfedge: -1, prop_vert: -1 };
        let rk = Self::key(end, start);
        let mut below = -1;
        if let Some(&p) = self.top.get(&rk) {
            // pop the reverse edge's stack
            data.paired_halfedge = p;
            self.halfedges.at_mut(p)?.paired_halfedge = he;
            let next = self.below.at(p)?;
            if next < 0 {
                self.top.remove(&rk);
            } else {
                self.top.insert(rk, next);
            }
        } else {
            let k = Self::key(start, end);
            below = self.top.get(&k).copied().unwrap_or(-1);
            self.top.insert(k, he);
        }
        self.below.push(below);
        self.halfedges.push(data);
        Ok(())
    }
}

/// tree2d.cpp `BuildTwoDTreeImpl`: alternating x / y stable sorts, median split (depth ≤ log2 n).
fn build_tree_impl(points: &mut [PolyVert], sort_x: bool, depth: u32) -> R<()> {
    if depth > 64 {
        return Err(Fault::Loop);
    }
    if sort_x {
        points.sort_by(by_less(|a: &PolyVert, b: &PolyVert| a.pos.x < b.pos.x));
    } else {
        points.sort_by(by_less(|a: &PolyVert, b: &PolyVert| a.pos.y < b.pos.y));
    }
    let n = points.len();
    if n < 2 {
        return Ok(());
    }
    let (lo, rest) = points.split_at_mut(n / 2);
    build_tree_impl(lo, !sort_x, depth + 1)?;
    let hi = rest.get_mut(1..).ok_or(Fault::Index)?;
    build_tree_impl(hi, !sort_x, depth + 1)
}

/// tree2d.cpp `BuildTwoDTree`.
fn build_two_d_tree(points: &mut [PolyVert]) -> R<()> {
    if points.len() <= 8 {
        return Ok(());
    }
    build_tree_impl(points, true, 0)
}

/// tree2d.h `QueryTwoDTree`.
fn query_two_d_tree(points: &[PolyVert], r: Rect, mut f: impl FnMut(PolyVert) -> R<()>) -> R<()> {
    if points.len() <= 8 {
        for p in points {
            if r.contains(p.pos) {
                f(*p)?;
            }
        }
        return Ok(());
    }
    let mut current = Rect { min: V2::splat(f64::NEG_INFINITY), max: V2::splat(f64::INFINITY) };
    let mut level = 0usize;
    let mut view: (usize, usize) = (0, points.len());
    let mut stack: Vec<(Rect, (usize, usize), usize)> = Vec::with_capacity(64);
    loop {
        let len = view.1 - view.0;
        if len <= 8 {
            for p in points.get(view.0..view.1).ok_or(Fault::Index)? {
                if r.contains(p.pos) {
                    f(*p)?;
                }
            }
            match stack.pop() {
                Some((rc, v, l)) => {
                    current = rc;
                    view = v;
                    level = l;
                    continue;
                }
                None => return Ok(()),
            }
        }
        let mut left = current;
        let mut right = current;
        let middle = points.atu(view.0 + len / 2)?;
        if level.is_multiple_of(2) {
            left.max.x = middle.pos.x;
            right.min.x = middle.pos.x;
        } else {
            left.max.y = middle.pos.y;
            right.min.y = middle.pos.y;
        }
        if r.contains(middle.pos) {
            f(middle)?;
        }
        if left.does_overlap(&r) {
            if right.does_overlap(&r) {
                if stack.len() >= 64 {
                    return Err(Fault::Loop);
                }
                stack.push((right, (view.0 + len / 2 + 1, view.1), level + 1));
            }
            current = left;
            view = (view.0, view.0 + len / 2);
        } else {
            current = right;
            view = (view.0 + len / 2 + 1, view.1);
        }
        level += 1;
    }
}

/// An f64 ordered by `<` (equal values, including ±0, compare equal; costs are never NaN on valid input).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Cost(f64);
impl Eq for Cost {}
impl PartialOrd for Cost {
    fn partial_cmp(&self, o: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Cost {
    fn cmp(&self, o: &Self) -> core::cmp::Ordering {
        if self.0 < o.0 {
            core::cmp::Ordering::Less
        } else if o.0 < self.0 {
            core::cmp::Ordering::Greater
        } else {
            self.0.is_nan().cmp(&o.0.is_nan())
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Vert {
    mesh_idx: i32,
    cost: f64,
    ear: Option<(Cost, u64)>,
    pos: V2,
    right_dir: V2,
    left: usize,
    right: usize,
}

/// polygon.cpp `EarClip`.
struct EarClip {
    polygon: Vec<Vert>,
    holes: Vec<usize>,
    outers: Vec<usize>,
    simples: Vec<usize>,
    hole2bbox: FxMap<usize, Rect>,
    /// `std::multiset<VertItr, MinCost>`: a min-heap on (cost, insertion number); an entry is live while its vert
    /// still names that key (`Vert::ear`), so removal is lazy and the order is the multiset's.
    ears: BinaryHeap<Reverse<(Cost, u64, usize)>>,
    ear_seq: u64,
    scratch: Vec<usize>,
    result: HalfedgeTriangulation,
    bbox: Rect,
    epsilon: f64,
}

/// `EarClip::SafeNormalize` (2D).
#[inline]
fn safe_normalize2(v: V2) -> V2 {
    let n = normalize2(v);
    if n.x.is_finite() { n } else { v2(0.0, 0.0) }
}

impl EarClip {
    #[inline]
    fn v(&self, i: usize) -> R<&Vert> {
        self.polygon.get(i).ok_or(Fault::Index)
    }
    #[inline]
    fn vm(&mut self, i: usize) -> R<&mut Vert> {
        self.polygon.get_mut(i).ok_or(Fault::Index)
    }
    #[inline]
    fn pos(&self, i: usize) -> R<V2> {
        Ok(self.v(i)?.pos)
    }
    #[inline]
    fn left(&self, i: usize) -> R<usize> {
        Ok(self.v(i)?.left)
    }
    #[inline]
    fn right(&self, i: usize) -> R<usize> {
        Ok(self.v(i)?.right)
    }

    fn new(polys: &[Vec<PolyVert>], epsilon: f64) -> R<EarClip> {
        let num_vert: usize = polys.iter().map(Vec::len).sum();
        let mut ec = EarClip {
            polygon: Vec::with_capacity(num_vert + 2 * polys.len()),
            holes: Vec::new(),
            outers: Vec::new(),
            simples: Vec::new(),
            hole2bbox: FxMap::default(),
            ears: BinaryHeap::new(),
            ear_seq: 0,
            scratch: Vec::new(),
            result: HalfedgeTriangulation::default(),
            bbox: Rect::default(),
            epsilon,
        };
        let starts = ec.initialize(polys)?;
        for v in 0..ec.polygon.len() {
            ec.clip_if_degenerate(v)?;
        }
        for first in starts {
            ec.find_start(first)?;
        }
        Ok(ec)
    }

    fn triangulate(mut self) -> R<HalfedgeTriangulation> {
        // std::multiset<VertItr, MaxX>: by x descending, equal x in insertion order
        let mut holes = core::mem::take(&mut self.holes);
        let polygon = &self.polygon;
        holes.sort_by(by_less(|a: &usize, b: &usize| {
            let ax = polygon.get(*a).map_or(f64::NAN, |v| v.pos.x);
            let bx = polygon.get(*b).map_or(f64::NAN, |v| v.pos.x);
            ax > bx
        }));
        for start in holes {
            self.cut_keyhole(start)?;
        }
        let mut i = 0;
        while i < self.simples.len() {
            let start = self.simples.atu(i)?;
            self.triangulate_poly(start)?;
            i += 1;
        }
        let mut result = self.result;
        result.top.clear();
        result.below.clear();
        Ok(result)
    }

    /// `Link`.
    fn link(&mut self, left: usize, right: usize) -> R<()> {
        let rp = self.pos(right)?;
        let lp = self.pos(left)?;
        self.vm(left)?.right = right;
        self.vm(right)?.left = left;
        self.vm(left)?.right_dir = safe_normalize2(rp - lp);
        Ok(())
    }

    /// `Clipped`.
    #[inline]
    fn clipped(&self, v: usize) -> R<bool> {
        Ok(self.left(self.right(v)?)? != v)
    }

    /// `Loop`: the un-clipped verts of the polygon through `first` (traversal only; the callers' functions never
    /// relink), and the C++ return value (`NONE` for a polygon degenerated to two points).
    fn loop_verts(&self, mut first: usize) -> R<(Vec<usize>, usize)> {
        let mut out = Vec::new();
        let mut v = first;
        let mut guard = Guard::new(2 * self.polygon.len());
        loop {
            guard.tick()?;
            if self.clipped(v)? {
                first = self.left(self.right(v)?)?;
                if !self.clipped(first)? {
                    v = first;
                    if self.right(v)? == self.left(v)? {
                        return Ok((out, NONE));
                    }
                    out.push(v);
                }
            } else {
                if self.right(v)? == self.left(v)? {
                    return Ok((out, NONE));
                }
                out.push(v);
            }
            v = self.right(v)?;
            if v == first {
                return Ok((out, v));
            }
        }
    }

    /// `ClipEar`: unlink and emit the triangle (unless topologically degenerate).
    fn clip_ear(&mut self, ear: usize) -> R<()> {
        let (l, r) = (self.left(ear)?, self.right(ear)?);
        self.link(l, r)?;
        let (li, ei, ri) = (self.v(l)?.mesh_idx, self.v(ear)?.mesh_idx, self.v(r)?.mesh_idx);
        if li != ei && ei != ri && ri != li {
            self.result.add_triangle(li, ei, ri)?;
        }
        Ok(())
    }

    /// `ClipIfDegenerate` (recursive in the C++: left subtree first, then right; same order with an explicit stack).
    fn clip_if_degenerate(&mut self, ear: usize) -> R<()> {
        let mut stack = core::mem::take(&mut self.scratch);
        stack.clear();
        stack.push(ear);
        let mut guard = Guard::new(4 * self.polygon.len());
        while let Some(e) = stack.pop() {
            guard.tick()?;
            if self.clipped(e)? {
                continue;
            }
            let (l, r) = (self.left(e)?, self.right(e)?);
            if l == r {
                continue;
            }
            let (lp, p, rp) = (self.pos(l)?, self.pos(e)?, self.pos(r)?);
            if self.is_short(e)? || (ccw(lp, p, rp, self.epsilon) == 0 && dot2(lp - p, rp - p) > 0.0) {
                self.clip_ear(e)?;
                // the clipped ear keeps its own links
                stack.push(r);
                stack.push(l);
            }
        }
        self.scratch = stack;
        Ok(())
    }

    /// `Initialize`: the circular lists, the bounding box, epsilon, the contour halfedges.
    fn initialize(&mut self, polys: &[Vec<PolyVert>]) -> R<Vec<usize>> {
        let mut starts = Vec::new();
        for poly in polys {
            let Some(first_pv) = poly.first() else { continue };
            let mk =
                |pv: &PolyVert| Vert { mesh_idx: pv.idx, cost: 0.0, ear: None, pos: pv.pos, right_dir: v2(0.0, 0.0), left: 0, right: 0 };
            self.polygon.push(mk(first_pv));
            let first = self.polygon.len() - 1;
            self.bbox.union_pt(first_pv.pos);
            let mut last = first;
            starts.push(first);
            for pv in poly.iter().skip(1) {
                self.bbox.union_pt(pv.pos);
                self.polygon.push(mk(pv));
                let next = self.polygon.len() - 1;
                self.link(last, next)?;
                last = next;
            }
            self.link(last, first)?;
        }
        if self.epsilon < 0.0 {
            let b = &self.bbox;
            let abs_max = crate::linalg::max2(v2(b.min.x.abs(), b.min.y.abs()), v2(b.max.x.abs(), b.max.y.abs()));
            self.epsilon = crate::linalg::la_max(abs_max.x, abs_max.y) * K_PRECISION;
        }
        self.result.add_contours(polys)?;
        Ok(starts)
    }

    /// `FindStart`: classify a contour (hole / outer / simple) and find a hole's right-most reflex start.
    fn find_start(&mut self, first: usize) -> R<()> {
        let origin = self.pos(first)?;
        let mut start = first;
        let mut max_x = f64::NEG_INFINITY;
        let mut bbox = Rect::default();
        let mut area = 0.0f64;
        let mut comp = 0.0f64;
        let (verts, end) = self.loop_verts(first)?;
        for v in verts {
            let p = self.pos(v)?;
            bbox.union_pt(p);
            let area1 = det2(p - origin, self.pos(self.right(v)?)? - origin);
            let t1 = area + area1;
            comp += (area - t1) + area1;
            area = t1;
            if p.x > max_x && self.is_reflex(v)? {
                max_x = p.x;
                start = v;
            }
        }
        if end == NONE {
            return Ok(());
        }
        area += comp;
        let size = bbox.size();
        let min_area = self.epsilon * std_max(size.x, size.y);
        if max_x.is_finite() && area < -min_area {
            self.holes.push(start);
            self.hole2bbox.entry(start).or_insert(bbox);
        } else {
            self.simples.push(start);
            if area > min_area {
                self.outers.push(start);
            }
        }
        Ok(())
    }

    /// `Vert::IsShort`.
    fn is_short(&self, v: usize) -> R<bool> {
        let e = self.pos(self.right(v)?)? - self.pos(v)?;
        Ok(dot2(e, e) * 4.0 < self.epsilon * self.epsilon)
    }

    /// `Vert::IsConvex`.
    fn is_convex(&self, v: usize, eps: f64) -> R<bool> {
        Ok(ccw(self.pos(self.left(v)?)?, self.pos(v)?, self.pos(self.right(v)?)?, eps) >= 0)
    }

    /// `Vert::IsReflex`.
    fn is_reflex(&self, v: usize) -> R<bool> {
        let l = self.left(v)?;
        Ok(!self.inside_edge(l, self.right(l)?, self.epsilon, true)?)
    }

    /// `Vert::InsideEdge` (`this` = `me`).
    fn inside_edge(&self, me: usize, tail: usize, eps: f64, to_left: bool) -> R<bool> {
        let p2 = eps * eps;
        let mut next_l = self.right(self.left(me)?)?;
        let mut next_r = self.right(tail)?;
        let mut center = tail;
        let mut last = center;
        let stop = if to_left { self.right(me)? } else { self.left(me)? };
        let step_l = |s: &Self, n: usize| -> R<usize> { if to_left { s.left(n) } else { s.right(n) } };
        let mut guard = Guard::new(4 * self.polygon.len());
        while next_l != next_r && tail != next_r && next_l != stop {
            guard.tick()?;
            let cp = self.pos(center)?;
            let edge_l = self.pos(next_l)? - cp;
            let l2 = dot2(edge_l, edge_l);
            if l2 <= p2 {
                next_l = step_l(self, next_l)?;
                continue;
            }
            let edge_r = self.pos(next_r)? - cp;
            let r2 = dot2(edge_r, edge_r);
            if r2 <= p2 {
                next_r = self.right(next_r)?;
                continue;
            }
            let vec_lr = self.pos(next_r)? - self.pos(next_l)?;
            let lr2 = dot2(vec_lr, vec_lr);
            if lr2 <= p2 {
                last = center;
                center = next_l;
                next_l = step_l(self, next_l)?;
                if next_l == next_r {
                    break;
                }
                next_r = self.right(next_r)?;
                continue;
            }
            let mut convexity = ccw(self.pos(next_l)?, cp, self.pos(next_r)?, eps);
            if center != last {
                let lp = self.pos(last)?;
                convexity += ccw(lp, cp, self.pos(next_l)?, eps) + ccw(self.pos(next_r)?, cp, lp, eps);
            }
            if convexity != 0 {
                return Ok(convexity > 0);
            }
            if l2 < r2 {
                center = next_l;
                next_l = step_l(self, next_l)?;
            } else {
                center = next_r;
                next_r = self.right(next_r)?;
            }
            last = center;
        }
        Ok(true)
    }

    /// `Vert::InterpY2X`.
    fn interp_y2x(&self, me: usize, start: V2, on_top: i32, eps: f64) -> R<f64> {
        let p = self.pos(me)?;
        let rp = self.pos(self.right(me)?)?;
        Ok(if (p.y - start.y).abs() <= eps {
            if rp.y <= start.y + eps || on_top == 1 { f64::NAN } else { p.x }
        } else if p.y < start.y - eps {
            if rp.y > start.y + eps {
                p.x + (start.y - p.y) * (rp.x - p.x) / (rp.y - p.y)
            } else if rp.y < start.y - eps || on_top == -1 {
                f64::NAN
            } else {
                rp.x
            }
        } else {
            f64::NAN
        })
    }

    /// `Vert::SignedDist`.
    fn signed_dist(&self, me: usize, v: usize, unit: V2, eps: f64) -> R<f64> {
        let p = self.pos(me)?;
        let d = det2(unit, self.pos(v)? - p);
        if d.abs() < eps {
            let dr = det2(unit, self.pos(self.right(v)?)? - p);
            if dr.abs() > eps {
                return Ok(dr);
            }
            let dl = det2(unit, self.pos(self.left(v)?)? - p);
            if dl.abs() > eps {
                return Ok(dl);
            }
        }
        Ok(d)
    }

    /// `Vert::Cost`.
    fn cost(&self, me: usize, v: usize, open_side: V2, eps: f64) -> R<f64> {
        let rd = self.v(me)?.right_dir;
        let lrd = self.v(self.left(me)?)?.right_dir;
        let c = std_min(self.signed_dist(me, v, rd, eps)?, self.signed_dist(me, v, lrd, eps)?);
        let open_cost = det2(open_side, self.pos(v)? - self.pos(self.right(me)?)?);
        Ok(std_min(c, open_cost))
    }

    /// `Vert::EarCost`.
    fn ear_cost(&self, me: usize, eps: f64, points: &[PolyVert], itr: &[usize]) -> R<f64> {
        let l = self.left(me)?;
        let r = self.right(me)?;
        let lp = self.pos(l)?;
        let rp = self.pos(r)?;
        let p = self.pos(me)?;
        let mut open_side = lp - rp;
        let center = 0.5 * (lp + rp);
        let scale = 4.0 / dot2(open_side, open_side);
        let radius = length2(open_side) / 2.0;
        open_side = normalize2(open_side);

        let mut total = dot2(self.v(l)?.right_dir, self.v(me)?.right_dir) - 1.0 - eps;
        if ccw(p, lp, rp, eps) == 0 {
            return Ok(total);
        }
        let mut ear_box = Rect::from_points(v2(center.x - radius, center.y - radius), v2(center.x + radius, center.y + radius));
        ear_box.union_pt(p);
        ear_box.min = ear_box.min - eps;
        ear_box.max = ear_box.max + eps;

        let mid = self.v(me)?.mesh_idx;
        let lid = self.v(l)?.mesh_idx;
        let rid = self.v(r)?.mesh_idx;
        query_two_d_tree(points, ear_box, |pt| {
            let test = itr.at(pt.idx)?;
            let tv = self.v(test)?;
            if !self.clipped(test)? && tv.mesh_idx != mid && tv.mesh_idx != lid && tv.mesh_idx != rid {
                let mut c = self.cost(me, test, open_side, eps)?;
                if c < -eps {
                    let diff = tv.pos - center;
                    c = -eps - scale * dot2(diff, diff);
                }
                if c > total {
                    total = c;
                }
            }
            Ok(())
        })?;
        Ok(total)
    }

    /// `CutKeyhole`: join a hole to the outer polygon it sits in.
    fn cut_keyhole(&mut self, start: usize) -> R<()> {
        let bbox = self.hole2bbox.get(&start).copied().unwrap_or_default();
        let sp = self.pos(start)?;
        let eps = self.epsilon;
        let on_top = if sp.y >= bbox.max.y - eps {
            1
        } else if sp.y <= bbox.min.y + eps {
            -1
        } else {
            0
        };
        let mut connector = NONE;
        for oi in 0..self.outers.len() {
            let first = self.outers.atu(oi)?;
            let (verts, _) = self.loop_verts(first)?;
            for edge in verts {
                let x = self.interp_y2x(edge, sp, on_top, eps)?;
                if x.is_finite()
                    && self.inside_edge(start, edge, eps, true)?
                    && (connector == NONE
                        || ccw(v2(x, sp.y), self.pos(connector)?, self.pos(self.right(connector)?)?, eps) == 1
                        || (if self.pos(connector)?.y < self.pos(edge)?.y {
                            self.inside_edge(edge, connector, eps, false)?
                        } else {
                            !self.inside_edge(connector, edge, eps, false)?
                        }))
                {
                    connector = edge;
                }
            }
        }
        if connector == NONE {
            self.simples.push(start);
            return Ok(());
        }
        let connector = self.find_closer_bridge(start, connector)?;
        self.join_polygons(start, connector)
    }

    /// `FindCloserBridge`.
    fn find_closer_bridge(&self, start: usize, edge: usize) -> R<usize> {
        let sp = self.pos(start)?;
        let ep = self.pos(edge)?;
        let er = self.right(edge)?;
        let erp = self.pos(er)?;
        let connector0 = if ep.x < sp.x {
            er
        } else if erp.x < sp.x || erp.y - sp.y > sp.y - ep.y {
            edge
        } else {
            er
        };
        let mut connector = connector0;
        let eps = self.epsilon;
        if (self.pos(connector)?.y - sp.y).abs() <= eps {
            return Ok(connector);
        }
        let above: f64 = if self.pos(connector)?.y > sp.y { 1.0 } else { -1.0 };
        for &first in &self.outers {
            let (verts, _) = self.loop_verts(first)?;
            for vert in verts {
                let vp = self.pos(vert)?;
                let cp = self.pos(connector)?;
                let inside = above * f64::from(ccw(sp, vp, cp, eps));
                if vp.x > sp.x - eps
                    && vp.y * above > sp.y * above - eps
                    && (inside > 0.0 || (inside == 0.0 && vp.x < cp.x && vp.y * above < cp.y * above))
                    && self.inside_edge(vert, edge, eps, true)?
                    && self.is_reflex(vert)?
                {
                    connector = vert;
                }
            }
        }
        Ok(connector)
    }

    /// `JoinPolygons`: both verts duplicated and reattached; degenerate ears clipped.
    fn join_polygons(&mut self, start: usize, connector: usize) -> R<()> {
        let s = *self.v(start)?;
        self.polygon.push(s);
        let new_start = self.polygon.len() - 1;
        let c = *self.v(connector)?;
        self.polygon.push(c);
        let new_connector = self.polygon.len() - 1;

        let sr = self.right(start)?;
        self.vm(sr)?.left = new_start;
        let cl = self.left(connector)?;
        self.vm(cl)?.right = new_connector;
        self.link(start, connector)?;
        self.link(new_connector, new_start)?;

        self.clip_if_degenerate(start)?;
        self.clip_if_degenerate(new_start)?;
        self.clip_if_degenerate(connector)?;
        self.clip_if_degenerate(new_connector)
    }

    fn insert_ear(&mut self, v: usize) -> R<()> {
        let c = self.v(v)?.cost;
        // ±0 compare equal, as `a->cost < b->cost` does
        let key = (Cost(if c == 0.0 { 0.0 } else { c }), self.ear_seq);
        self.ear_seq += 1;
        self.ears.push(Reverse((key.0, key.1, v)));
        self.vm(v)?.ear = Some(key);
        Ok(())
    }

    /// `ProcessEar`: recompute the ear's cost and requeue it (reflex verts leave the queue).
    fn process_ear(&mut self, v: usize, points: &[PolyVert], itr: &[usize]) -> R<()> {
        if self.v(v)?.ear.is_some() {
            self.vm(v)?.ear = None;
        }
        if self.is_short(v)? {
            self.vm(v)?.cost = K_BEST;
            self.insert_ear(v)?;
        } else if self.is_convex(v, 2.0 * self.epsilon)? {
            let c = self.ear_cost(v, self.epsilon, points, itr)?;
            self.vm(v)?.cost = c;
            self.insert_ear(v)?;
        } else {
            self.vm(v)?.cost = 1.0;
        }
        Ok(())
    }

    /// `TriangulatePoly`: the ear-clipping loop for one simple polygon.
    fn triangulate_poly(&mut self, start: usize) -> R<()> {
        // VertCollider
        let (verts, _) = self.loop_verts(start)?;
        let mut points: Vec<PolyVert> = Vec::with_capacity(verts.len());
        let mut itr: Vec<usize> = Vec::with_capacity(verts.len());
        for v in verts {
            points.push(PolyVert { pos: self.pos(v)?, idx: ix(itr.len())? });
            itr.push(v);
        }
        build_two_d_tree(&mut points)?;
        if itr.is_empty() {
            return Ok(());
        }

        let mut num_tri: i64 = -2;
        self.ears.clear();
        let (verts, end) = self.loop_verts(start)?;
        for v in verts {
            self.process_ear(v, &points, &itr)?;
            num_tri += 1;
        }
        if end == NONE {
            return Ok(());
        }
        let mut v = end;
        while num_tri > 0 {
            // earsQueue_.begin(): the smallest live entry
            while let Some(&Reverse((c, seq, e))) = self.ears.peek() {
                self.ears.pop();
                if self.v(e)?.ear == Some((c, seq)) {
                    v = e;
                    self.vm(v)?.ear = None;
                    break;
                }
            }
            self.clip_ear(v)?;
            num_tri -= 1;
            let (l, r) = (self.left(v)?, self.right(v)?);
            self.process_ear(l, &points, &itr)?;
            self.process_ear(r, &points, &itr)?;
            v = self.right(v)?;
        }
        Ok(())
    }
}

/// `TriangulateIdxHalfedges(polys, epsilon, allowConvex = false)`.
pub(crate) fn triangulate_idx_halfedges(polys: &[Vec<PolyVert>], epsilon: f64) -> R<HalfedgeTriangulation> {
    EarClip::new(polys, epsilon)?.triangulate()
}
