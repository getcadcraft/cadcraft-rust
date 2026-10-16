// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! edge_op.cpp: topology cleanup (pinched verts, duplicate edges), short and colinear edge collapse, and swaps of
//! degenerate triangles. Flags are collected for all edges first, then acted on in index order (the sequential
//! `FlagStore::run_seq`). Every orbit and repeat loop carries a bound (the C++ assumes valid topology).
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use crate::halfedge::{TriRef, next_halfedge};
use crate::imp::Impl;
use crate::linalg::{V2, axis_aligned_projection, ccw, dot2, dot3, length2};
use crate::util::{Fault, Get, GetMut, Guard, R, ix, std_max, std_min};

#[inline]
fn tri_of(edge: i32) -> [i32; 3] {
    let e1 = next_halfedge(edge);
    [edge, e1, next_halfedge(e1)]
}

#[inline]
fn is01_longest(v0: V2, v1: V2, v2: V2) -> bool {
    let e = [v1 - v0, v2 - v1, v0 - v2];
    let l = [dot2(e[0], e[0]), dot2(e[1], e[1]), dot2(e[2], e[2])];
    l[0] > l[1] && l[0] > l[2]
}

impl Impl {
    /// `CleanupTopology`.
    pub(crate) fn cleanup_topology(&mut self) -> R<()> {
        if self.halfedge.is_empty() {
            return Ok(());
        }
        self.split_pinched_verts()?;
        self.dedupe_edges()
    }

    /// `SimplifyTopology(firstNewVert)`.
    pub(crate) fn simplify_topology(&mut self, first_new_vert: i32) -> R<()> {
        if self.halfedge.is_empty() {
            return Ok(());
        }
        self.cleanup_topology()?;
        self.collapse_short_edges(first_new_vert)?;
        self.collapse_colinear_edges(first_new_vert)?;
        self.swap_degenerates(first_new_vert)?;
        self.calculate_vert_normals()
    }

    /// `RemoveDegenerates(firstNewVert)`.
    pub(crate) fn remove_degenerates(&mut self, first_new_vert: i32) -> R<()> {
        if self.halfedge.is_empty() {
            return Ok(());
        }
        self.cleanup_topology()?;
        self.collapse_short_edges(first_new_vert)?;
        self.swap_degenerates(first_new_vert)?;
        self.calculate_vert_normals()
    }

    fn collapse_short_edges(&mut self, first_new_vert: i32) -> R<()> {
        let nb = self.halfedge.len();
        let tol = if first_new_vert == 0 { self.epsilon } else { self.tolerance };
        let mut flagged = Vec::new();
        for e in 0..ix(nb)? {
            let pair = self.halfedge.pair(e)?;
            if pair < 0 {
                continue;
            }
            let start = self.halfedge.start(e)?;
            let end = self.halfedge.end(e)?;
            if start < first_new_vert && end < first_new_vert {
                continue;
            }
            let delta = self.vert_pos.at(end)? - self.vert_pos.at(start)?;
            let len_sq = dot3(delta, delta);
            let max_len = if end < first_new_vert { tol * tol } else { self.epsilon * self.epsilon };
            if len_sq < max_len {
                flagged.push(e);
            }
        }
        let mut scratch = Vec::with_capacity(10);
        for e in flagged {
            self.collapse_edge(e, &mut scratch, tol, first_new_vert)?;
            scratch.clear();
        }
        Ok(())
    }

    fn collapse_colinear_edges(&mut self, first_new_vert: i32) -> R<()> {
        let nb = self.halfedge.len();
        let mut scratch = Vec::with_capacity(10);
        let mut rounds = Guard::new(nb + 1);
        loop {
            rounds.tick()?;
            let mut flagged = Vec::new();
            for e in 0..ix(nb)? {
                if self.colinear_edge(e, first_new_vert)? {
                    flagged.push(e);
                }
            }
            let mut num = 0usize;
            for e in flagged {
                if self.collapse_edge(e, &mut scratch, -1.0, 0)? {
                    num += 1;
                }
                scratch.clear();
            }
            if num == 0 {
                return Ok(());
            }
        }
    }

    fn colinear_edge(&self, edge: i32, first_new_vert: i32) -> R<bool> {
        let pair = self.halfedge.pair(edge)?;
        if pair < 0 || self.halfedge.start(edge)? < first_new_vert {
            return Ok(false);
        }
        let tri_ref = &self.mesh_relation.tri_ref;
        let ref0 = tri_ref.at(edge / 3)?;
        let mut current = next_halfedge(pair);
        let mut ref1 = tri_ref.at(current / 3)?;
        let mut ref1_updated = !ref0.same_face(&ref1);
        let mut guard = Guard::new(self.halfedge.len());
        while current != edge {
            guard.tick()?;
            current = next_halfedge(self.halfedge.pair(current)?);
            let r = tri_ref.at(current / 3)?;
            if !r.same_face(&ref0) && !r.same_face(&ref1) {
                if !ref1_updated {
                    ref1 = r;
                    ref1_updated = true;
                } else {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    fn swappable_edge(&self, edge: i32, first_new_vert: i32) -> R<bool> {
        let pair = self.halfedge.pair(edge)?;
        if pair < 0 {
            return Ok(false);
        }
        let te = tri_of(edge);
        let pte = tri_of(pair);
        if self.halfedge.start(te[0])? < first_new_vert
            && self.halfedge.start(te[1])? < first_new_vert
            && self.halfedge.start(te[2])? < first_new_vert
            && self.halfedge.start(pte[2])? < first_new_vert
        {
            return Ok(false);
        }
        let proj = axis_aligned_projection(self.face_normal.at(edge / 3)?);
        let mut v = [V2::default(); 3];
        for (i, vi) in v.iter_mut().enumerate() {
            *vi = proj.apply(self.vert_pos.at(self.halfedge.start(te.atu(i)?)?)?);
        }
        if ccw(v[0], v[1], v[2], self.tolerance) > 0 || !is01_longest(v[0], v[1], v[2]) {
            return Ok(false);
        }
        let proj = axis_aligned_projection(self.face_normal.at(pair / 3)?);
        for (i, vi) in v.iter_mut().enumerate() {
            *vi = proj.apply(self.vert_pos.at(self.halfedge.start(pte.atu(i)?)?)?);
        }
        Ok(ccw(v[0], v[1], v[2], self.tolerance) > 0 || is01_longest(v[0], v[1], v[2]))
    }

    fn swap_degenerates(&mut self, first_new_vert: i32) -> R<()> {
        let nb = self.halfedge.len();
        let mut flagged = Vec::new();
        for e in 0..ix(nb)? {
            if self.swappable_edge(e, first_new_vert)? {
                flagged.push(e);
            }
        }
        let mut stack: Vec<i32> = Vec::new();
        let mut visited = vec![-1i32; nb];
        let mut tag = 0i32;
        let mut scratch = Vec::with_capacity(10);
        for e in flagged {
            // within one tag each edge pair swaps at most once; bound the work per flagged edge generously
            let mut guard = Guard::new(8 * nb + 64);
            tag += 1;
            self.recursive_edge_swap(e, &mut tag, &mut visited, &mut stack, &mut scratch)?;
            while let Some(last) = stack.pop() {
                guard.tick()?;
                self.recursive_edge_swap(last, &mut tag, &mut visited, &mut stack, &mut scratch)?;
            }
        }
        Ok(())
    }

    /// `DedupeEdge`: split a 4-manifold edge by duplicating its end vertex (and the start, if it becomes pinched).
    fn dedupe_edge(&mut self, edge: i32) -> R<()> {
        let next_edge = next_halfedge(edge);
        let start_vert = self.halfedge.start(edge)?;
        let end_vert = self.halfedge.start(next_edge)?;
        let end_prop = self.halfedge.prop(next_edge)?;
        let mut current = self.halfedge.pair(next_edge)?;
        let mut guard = Guard::new(self.halfedge.len());
        while current != edge {
            guard.tick()?;
            let vert = self.halfedge.start(current)?;
            if vert == start_vert {
                // Single topological unit needs 2 faces added to be split
                let new_vert = ix(self.vert_pos.len())?;
                let p = self.vert_pos.at(end_vert)?;
                self.vert_pos.push(p);
                if !self.vert_normal.is_empty() {
                    let n = self.vert_normal.at(end_vert)?;
                    self.vert_normal.push(n);
                }
                current = self.halfedge.pair(next_halfedge(current))?;
                let opposite = self.halfedge.pair(next_edge)?;
                self.update_vert(new_vert, current, opposite)?;

                let mut new_he = ix(self.halfedge.len())?;
                let mut old_face = current / 3;
                let mut outside = self.halfedge.start(current)?;
                let pc = self.halfedge.prop(current)?;
                self.halfedge.push(end_vert, -1, end_prop);
                self.halfedge.push(new_vert, -1, end_prop);
                self.halfedge.push(outside, -1, pc);
                let pcur = self.halfedge.pair(current)?;
                self.pair_up(new_he + 2, pcur)?;
                self.pair_up(new_he + 1, current)?;
                if !self.mesh_relation.tri_ref.is_empty() {
                    let r = self.mesh_relation.tri_ref.at(old_face)?;
                    self.mesh_relation.tri_ref.push(r);
                }
                if !self.face_normal.is_empty() {
                    let f = self.face_normal.at(old_face)?;
                    self.face_normal.push(f);
                }

                new_he += 3;
                old_face = opposite / 3;
                outside = self.halfedge.start(opposite)?;
                let po = self.halfedge.prop(opposite)?;
                self.halfedge.push(new_vert, -1, end_prop);
                self.halfedge.push(end_vert, -1, end_prop);
                self.halfedge.push(outside, -1, po);
                let popp = self.halfedge.pair(opposite)?;
                self.pair_up(new_he + 2, popp)?;
                self.pair_up(new_he + 1, opposite)?;
                self.pair_up(new_he, new_he - 3)?;
                if !self.mesh_relation.tri_ref.is_empty() {
                    let r = self.mesh_relation.tri_ref.at(old_face)?;
                    self.mesh_relation.tri_ref.push(r);
                }
                if !self.face_normal.is_empty() {
                    let f = self.face_normal.at(old_face)?;
                    self.face_normal.push(f);
                }
                break;
            }
            current = self.halfedge.pair(next_halfedge(current))?;
        }

        if current == edge {
            // Separate topological unit needs no new faces to be split
            let new_vert = ix(self.vert_pos.len())?;
            let p = self.vert_pos.at(end_vert)?;
            self.vert_pos.push(p);
            if !self.vert_normal.is_empty() {
                let n = self.vert_normal.at(end_vert)?;
                self.vert_normal.push(n);
            }
            self.relabel_orbit(next_halfedge(current), new_vert)?;
        }

        // Orbit startVert
        let pair = self.halfedge.pair(edge)?;
        current = self.halfedge.pair(next_halfedge(pair))?;
        let mut guard = Guard::new(self.halfedge.len());
        while current != pair {
            guard.tick()?;
            let vert = self.halfedge.start(current)?;
            if vert == end_vert {
                break;
            }
            current = self.halfedge.pair(next_halfedge(current))?;
        }
        if current == pair {
            // Split the pinched vert the previous split created.
            let new_vert = ix(self.vert_pos.len())?;
            let p = self.vert_pos.at(end_vert)?;
            self.vert_pos.push(p);
            if !self.vert_normal.is_empty() {
                let n = self.vert_normal.at(end_vert)?;
                self.vert_normal.push(n);
            }
            self.relabel_orbit(next_halfedge(current), new_vert)?;
        }
        Ok(())
    }

    /// `ForVert(e, {SetStart(e, v); SetEnd(Pair(e), v)})`.
    fn relabel_orbit(&mut self, halfedge: i32, vert: i32) -> R<()> {
        let mut guard = Guard::new(self.halfedge.len());
        let mut current = halfedge;
        loop {
            guard.tick()?;
            current = next_halfedge(self.halfedge.pair(current)?);
            self.halfedge.set_start(current, vert)?;
            let p = self.halfedge.pair(current)?;
            self.halfedge.set_end(p, vert)?;
            if current == halfedge {
                return Ok(());
            }
        }
    }

    pub(crate) fn pair_up(&mut self, e0: i32, e1: i32) -> R<()> {
        self.halfedge.set_pair(e0, e1)?;
        self.halfedge.set_pair(e1, e0)
    }

    /// `UpdateVert`: CW around startEdge's end vertex from startEdge to endEdge, pointing each edge at `vert`.
    fn update_vert(&mut self, vert: i32, start_edge: i32, end_edge: i32) -> R<()> {
        let mut current = start_edge;
        let mut guard = Guard::new(self.halfedge.len());
        while current != end_edge {
            guard.tick()?;
            self.halfedge.set_end(current, vert)?;
            current = next_halfedge(current);
            self.halfedge.set_start(current, vert)?;
            current = self.halfedge.pair(current)?;
        }
        Ok(())
    }

    /// `FormLoop`: duplicate both verts and attach the manifolds the other way across this edge.
    fn form_loop(&mut self, current: i32, end: i32) -> R<()> {
        let start_vert = ix(self.vert_pos.len())?;
        let p = self.vert_pos.at(self.halfedge.start(current)?)?;
        self.vert_pos.push(p);
        let end_vert = ix(self.vert_pos.len())?;
        let p = self.vert_pos.at(self.halfedge.end(current)?)?;
        self.vert_pos.push(p);

        let old_match = self.halfedge.pair(current)?;
        let new_match = self.halfedge.pair(end)?;
        self.update_vert(start_vert, old_match, new_match)?;
        self.update_vert(end_vert, end, current)?;
        self.pair_up(current, new_match)?;
        self.pair_up(end, old_match)?;
        self.remove_if_folded(end)
    }

    fn collapse_tri(&mut self, tri_edge: [i32; 3]) -> R<()> {
        if self.halfedge.pair(tri_edge[1])? == -1 {
            return Ok(());
        }
        let p1 = self.halfedge.pair(tri_edge[1])?;
        let p2 = self.halfedge.pair(tri_edge[2])?;
        self.pair_up(p1, p2)?;
        for e in tri_edge {
            let pr = self.halfedge.prop(e)?;
            self.halfedge.set(e, -1, -1, pr)?;
        }
        Ok(())
    }

    fn remove_if_folded(&mut self, edge: i32) -> R<()> {
        let t0 = tri_of(edge);
        let t1 = tri_of(self.halfedge.pair(edge)?);
        if self.halfedge.pair(t0[1])? == -1 {
            return Ok(());
        }
        if self.halfedge.start(t0[2])? == self.halfedge.start(t1[2])? {
            let nan = crate::linalg::V3::splat(f64::NAN);
            if self.halfedge.pair(t0[1])? == t1[2] {
                if self.halfedge.pair(t0[2])? == t1[1] {
                    for e in t0 {
                        let s = self.halfedge.start(e)?;
                        self.vert_pos.set(s, nan)?;
                    }
                } else {
                    let s = self.halfedge.start(t0[1])?;
                    self.vert_pos.set(s, nan)?;
                }
            } else if self.halfedge.pair(t0[2])? == t1[1] {
                let s = self.halfedge.start(t1[1])?;
                self.vert_pos.set(s, nan)?;
            }
            let a = self.halfedge.pair(t0[1])?;
            let b = self.halfedge.pair(t1[2])?;
            self.pair_up(a, b)?;
            let a = self.halfedge.pair(t0[2])?;
            let b = self.halfedge.pair(t1[1])?;
            self.pair_up(a, b)?;
            for (e0, e1) in t0.into_iter().zip(t1) {
                self.halfedge.set(e0, -1, -1, -1)?;
                self.halfedge.set(e1, -1, -1, -1)?;
            }
        }
        Ok(())
    }

    /// `CollapseEdge`: remove the start vertex, merging it into the end vertex. False when it cannot.
    pub(crate) fn collapse_edge(&mut self, edge: i32, edges: &mut Vec<i32>, tol: f64, first_new_vert: i32) -> R<bool> {
        let tol = if tol < 0.0 { self.epsilon } else { tol };
        let pair = self.halfedge.pair(edge)?;
        if pair < 0 {
            return Ok(false);
        }
        let t0 = tri_of(edge);
        let t1 = tri_of(pair);
        let start_vert = self.halfedge.start(t0[0])?;
        let end_vert = self.halfedge.start(t0[1])?;

        let p_new = self.vert_pos.at(end_vert)?;
        let p_old = self.vert_pos.at(start_vert)?;
        let delta = p_new - p_old;
        let max_len = if end_vert < first_new_vert { tol * tol } else { self.epsilon * self.epsilon };
        let short_edge = dot3(delta, delta) < max_len;

        // Orbit startVert
        let mut start = self.halfedge.pair(t1[1])?;
        let len = self.halfedge.len();
        if !short_edge {
            let mut current = start;
            let mut ref_check: TriRef = self.mesh_relation.tri_ref.at(pair / 3)?;
            let mut p_last = self.vert_pos.at(self.halfedge.start(t1[2])?)?;
            let mut guard = Guard::new(len);
            while current != t1[0] {
                guard.tick()?;
                current = next_halfedge(current);
                let p_next = self.vert_pos.at(self.halfedge.end(current)?)?;
                let tri = current / 3;
                let r = self.mesh_relation.tri_ref.at(tri)?;
                let proj = axis_aligned_projection(self.face_normal.at(tri)?);
                // Don't collapse if the edge is not redundant (this may have changed due to the collapse of neighbors).
                if !r.same_face(&ref_check) {
                    let old_ref = ref_check;
                    ref_check = self.mesh_relation.tri_ref.at(edge / 3)?;
                    if !r.same_face(&ref_check) {
                        return Ok(false);
                    }
                    if (r.mesh_id != old_ref.mesh_id
                        || r.face_id != old_ref.face_id
                        || dot3(self.face_normal.at(pair / 3)?, self.face_normal.at(tri)?) < -0.5)
                        && ccw(proj.apply(p_last), proj.apply(p_old), proj.apply(p_new), tol) != 0
                    {
                        // Restrict collapse to colinear edges when the edge separates faces or the edge is sharp.
                        return Ok(false);
                    }
                }
                // Don't collapse edge if it would cause a triangle to invert.
                if ccw(proj.apply(p_next), proj.apply(p_last), proj.apply(p_new), self.epsilon) < 0 {
                    return Ok(false);
                }
                p_last = p_next;
                current = self.halfedge.pair(current)?;
            }
        }

        // Orbit endVert
        {
            let mut current = self.halfedge.pair(t0[1])?;
            let mut guard = Guard::new(len);
            while current != t1[2] {
                guard.tick()?;
                current = next_halfedge(current);
                edges.push(current);
                current = self.halfedge.pair(current)?;
            }
        }

        // Remove toRemove.startVert and replace with endVert.
        self.vert_pos.set(start_vert, crate::linalg::V3::splat(f64::NAN))?;
        self.collapse_tri(t1)?;

        // Orbit startVert
        let tri0 = edge / 3;
        let tri1 = pair / 3;
        let mut current = start;
        let mut guard = Guard::new(len);
        while current != t0[2] {
            guard.tick()?;
            current = next_halfedge(current);
            if self.num_prop > 0 {
                // Update the shifted triangles to the vertBary of endVert
                let tri = current / 3;
                let rt = self.mesh_relation.tri_ref.at(tri)?;
                if rt.same_face(&self.mesh_relation.tri_ref.at(tri0)?) {
                    let p = self.halfedge.prop(next_halfedge(edge))?;
                    self.halfedge.set_prop(current, p)?;
                } else if rt.same_face(&self.mesh_relation.tri_ref.at(tri1)?) {
                    let p = self.halfedge.prop(pair)?;
                    self.halfedge.set_prop(current, p)?;
                }
            }
            let vert = self.halfedge.end(current)?;
            let next = self.halfedge.pair(current)?;
            for i in 0..edges.len() {
                let ei = edges.atu(i)?;
                if vert == self.halfedge.end(ei)? {
                    self.form_loop(ei, current)?;
                    start = next;
                    edges.truncate(i);
                    break;
                }
            }
            current = next;
        }

        self.update_vert(end_vert, start, t0[2])?;
        self.collapse_tri(t0)?;
        self.remove_if_folded(start)?;
        Ok(true)
    }

    fn recursive_edge_swap(&mut self, edge: i32, tag: &mut i32, visited: &mut [i32], stack: &mut Vec<i32>, edges: &mut Vec<i32>) -> R<()> {
        if edge < 0 {
            return Ok(());
        }
        let pair = self.halfedge.pair(edge)?;
        if pair < 0 {
            return Ok(());
        }
        // avoid infinite recursion
        if visited.at(edge)? == *tag && visited.at(pair)? == *tag {
            return Ok(());
        }
        let t0 = tri_of(edge);
        let t1 = tri_of(pair);

        let proj = axis_aligned_projection(self.face_normal.at(edge / 3)?);
        let mut v = [V2::default(); 4];
        for (vi, e) in v.iter_mut().zip(t0) {
            *vi = proj.apply(self.vert_pos.at(self.halfedge.start(e)?)?);
        }
        // Only operate on the long edge of a degenerate triangle.
        if ccw(v[0], v[1], v[2], self.tolerance) > 0 || !is01_longest(v[0], v[1], v[2]) {
            return Ok(());
        }
        // Switch to neighbor's projection.
        let proj = axis_aligned_projection(self.face_normal.at(pair / 3)?);
        for (vi, e) in v.iter_mut().zip(t0) {
            *vi = proj.apply(self.vert_pos.at(self.halfedge.start(e)?)?);
        }
        v[3] = proj.apply(self.vert_pos.at(self.halfedge.start(t1[2])?)?);

        if ccw(v[1], v[0], v[3], self.tolerance) <= 0 {
            if !is01_longest(v[1], v[0], v[3]) {
                return Ok(());
            }
            // Two facing, long-edge degenerates can swap.
            self.swap_edge(t0, t1, &v)?;
            let e23 = v[3] - v[2];
            if dot2(e23, e23) < self.tolerance * self.tolerance {
                *tag += 1;
                self.collapse_edge(t0[2], edges, -1.0, 0)?;
                edges.clear();
            } else {
                visited.set(edge, *tag)?;
                visited.set(pair, *tag)?;
                stack.extend_from_slice(&[t1[1], t1[0], t0[1], t0[0]]);
            }
            return Ok(());
        } else if ccw(v[0], v[3], v[2], self.tolerance) <= 0 || ccw(v[1], v[2], v[3], self.tolerance) <= 0 {
            return Ok(());
        }
        // Normal path
        self.swap_edge(t0, t1, &v)?;
        visited.set(edge, *tag)?;
        visited.set(pair, *tag)?;
        let a = self.halfedge.pair(t1[0])?;
        let b = self.halfedge.pair(t0[1])?;
        stack.push(a);
        stack.push(b);
        Ok(())
    }

    /// The `SwapEdge` lambda of `RecursiveEdgeSwap`.
    fn swap_edge(&mut self, t0: [i32; 3], t1: [i32; 3], v: &[V2; 4]) -> R<()> {
        // The 0-verts are swapped to the opposite 2-verts.
        let v0 = self.halfedge.start(t0[2])?;
        let v1 = self.halfedge.start(t1[2])?;
        self.halfedge.set_start(t0[0], v1)?;
        self.halfedge.set_end(t0[2], v1)?;
        self.halfedge.set_start(t1[0], v0)?;
        self.halfedge.set_end(t1[2], v0)?;
        let p = self.halfedge.pair(t1[2])?;
        self.pair_up(t0[0], p)?;
        let p = self.halfedge.pair(t0[2])?;
        self.pair_up(t1[0], p)?;
        self.pair_up(t0[2], t1[2])?;
        // Both triangles are now subsets of the neighboring triangle.
        let tri0 = t0[0] / 3;
        let tri1 = t1[0] / 3;
        let fnrm = self.face_normal.at(tri1)?;
        self.face_normal.set(tri0, fnrm)?;
        let r = self.mesh_relation.tri_ref.at(tri1)?;
        self.mesh_relation.tri_ref.set(tri0, r)?;
        let l01 = length2(v[1] - v[0]);
        let l02 = length2(v[2] - v[0]);
        let a = std_max(0.0, std_min(1.0, l02 / l01));
        // Update properties if applicable
        if !self.properties.is_empty() {
            let p = self.halfedge.prop(t1[0])?;
            self.halfedge.set_prop(t0[1], p)?;
            let p = self.halfedge.prop(t1[2])?;
            self.halfedge.set_prop(t0[0], p)?;
            self.halfedge.set_prop(t0[2], p)?;
            let np = self.num_prop;
            if np <= 0 {
                return Err(Fault::Index);
            }
            let new_prop = ix(self.properties.len())? / np;
            let i0 = self.halfedge.prop(t1[0])?;
            let i1 = self.halfedge.prop(t1[1])?;
            for p in 0..np {
                let val = a * self.properties.at(np * i0 + p)? + (1.0 - a) * self.properties.at(np * i1 + p)?;
                self.properties.push(val);
            }
            self.halfedge.set_prop(t1[0], new_prop)?;
            self.halfedge.set_prop(t0[2], new_prop)?;
        }

        // if the new edge already exists, duplicate the verts and split the mesh.
        let mut current = self.halfedge.pair(t1[0])?;
        let end_vert = self.halfedge.end(t1[1])?;
        let mut guard = Guard::new(self.halfedge.len());
        while current != t0[1] {
            guard.tick()?;
            current = next_halfedge(current);
            if self.halfedge.end(current)? == end_vert {
                self.form_loop(t0[2], current)?;
                self.remove_if_folded(t0[2])?;
                return Ok(());
            }
            current = self.halfedge.pair(current)?;
        }
        Ok(())
    }

    /// `SplitPinchedVerts` (sequential form).
    fn split_pinched_verts(&mut self) -> R<()> {
        let nb = self.halfedge.len();
        let mut vert_processed = vec![false; self.num_vert()];
        let mut he_processed = vec![false; nb];
        for i in 0..ix(nb)? {
            if he_processed.at(i)? {
                continue;
            }
            let mut vert = self.halfedge.start(i)?;
            if vert == -1 {
                continue;
            }
            if vert_processed.at(vert)? {
                let p = self.vert_pos.at(vert)?;
                self.vert_pos.push(p);
                vert = ix(self.vert_pos.len() - 1)?;
                let mut guard = Guard::new(nb);
                let mut current = i;
                loop {
                    guard.tick()?;
                    current = next_halfedge(self.halfedge.pair(current)?);
                    he_processed.set(current, true)?;
                    self.halfedge.set_start(current, vert)?;
                    let pc = self.halfedge.pair(current)?;
                    self.halfedge.set_end(pc, vert)?;
                    if current == i {
                        break;
                    }
                }
            } else {
                vert_processed.set(vert, true)?;
                let mut guard = Guard::new(nb);
                let mut current = i;
                loop {
                    guard.tick()?;
                    current = next_halfedge(self.halfedge.pair(current)?);
                    he_processed.set(current, true)?;
                    if current == i {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    /// `DedupeEdges`: repeat until no vertex has two outgoing halfedges to the same vertex.
    fn dedupe_edges(&mut self) -> R<()> {
        let mut rounds = Guard::new(self.halfedge.len() + 1);
        loop {
            rounds.tick()?;
            let nb = self.halfedge.len();
            let mut duplicates: Vec<i32> = Vec::new();
            let mut local = vec![false; nb];
            let mut end_verts: Vec<(i32, i32)> = Vec::new();
            let mut end_set: crate::util::FxMap<i32, i32> = crate::util::FxMap::default();
            for i in 0..ix(nb)? {
                if local.at(i)? {
                    continue;
                }
                let sv = self.halfedge.start(i)?;
                let ev = self.halfedge.end(i)?;
                if sv == -1 || ev == -1 {
                    continue;
                }
                end_verts.clear();
                end_set.clear();
                // first pass: the smallest halfedge per end vertex
                let mut guard = Guard::new(nb);
                let mut current = i;
                loop {
                    guard.tick()?;
                    current = next_halfedge(self.halfedge.pair(current)?);
                    local.set(current, true)?;
                    let s = self.halfedge.start(current)?;
                    let e = self.halfedge.end(current)?;
                    if s != -1 && e != -1 {
                        if end_set.is_empty() {
                            if let Some(entry) = end_verts.iter_mut().find(|p| p.0 == e) {
                                entry.1 = entry.1.min(current);
                            } else {
                                end_verts.push((e, current));
                                if end_verts.len() > 32 {
                                    for &(k, v) in &end_verts {
                                        end_set.entry(k).or_insert(v);
                                    }
                                    end_verts.clear();
                                }
                            }
                        } else {
                            let entry = end_set.entry(e).or_insert(current);
                            *entry = (*entry).min(current);
                        }
                    }
                    if current == i {
                        break;
                    }
                }
                // second pass: report every duplicate except the smallest
                let mut guard = Guard::new(nb);
                let mut current = i;
                loop {
                    guard.tick()?;
                    current = next_halfedge(self.halfedge.pair(current)?);
                    let s = self.halfedge.start(current)?;
                    let e = self.halfedge.end(current)?;
                    if s != -1 && e != -1 {
                        let smallest =
                            if end_set.is_empty() { end_verts.iter().find(|p| p.0 == e).map(|p| p.1) } else { end_set.get(&e).copied() };
                        let smallest = smallest.ok_or(Fault::Index)?;
                        if smallest != current {
                            duplicates.push(current);
                        }
                    }
                    if current == i {
                        break;
                    }
                }
            }
            let n = duplicates.len();
            for d in duplicates {
                self.dedupe_edge(d)?;
            }
            if n == 0 {
                return Ok(());
            }
        }
    }
}
