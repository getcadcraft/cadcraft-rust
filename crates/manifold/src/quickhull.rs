// Copyright 2024 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
// Derived (by the Manifold Authors) from the public domain work of Antti Kuukka at
// https://github.com/akuukka/quickhull
//
//! quickhull.h / quickhull.cpp: the 3D convex hull of a point cloud (QuickHull), as Manifold 3.5.4 builds it, with the
//! same floating-point operations in the same order, the same face and halfedge slot reuse, and the same output
//! ordering (`buildMesh`'s halfedge reordering and vertex compaction).
//!
//! Left out because they cannot change a result: the index-vector pool (a reused vector is always cleared before it is
//! filled, so only allocations differ) and `HalfEdgeMesh` (unused by `buildMesh`).
//!
//! Hostile input: every array access is checked ([`Fault::Index`]) and every loop the C++ assumes terminates carries a
//! [`Guard`] ([`Fault::Loop`]).
// Indices are bounded by the input cap (MAX_VERTS points, at most a few halfedges per point), far below i32::MAX.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::collections::VecDeque;

use crate::halfedge::Halfedge;
use crate::linalg::{V3, dot3, normalize3, v3};
use crate::util::{Fault, Get, GetMut, Guard, R};

/// `defaultEps()`.
const DEFAULT_EPS: f64 = 0.0000001;

/// quickhull.h `Plane`.
#[derive(Clone, Copy, Debug, Default)]
struct Plane {
    n: V3,
    d: f64,
    sqr_n_length: f64,
}

impl Plane {
    fn new(n: V3, p: V3) -> Plane {
        Plane { n, d: dot3(-n, p), sqr_n_length: dot3(n, n) }
    }
    fn is_point_on_positive_side(&self, q: V3) -> bool {
        let d = dot3(self.n, q) + self.d;
        d >= 0.0
    }
}

/// `getSquaredDistanceBetweenPointAndRay`.
fn sq_dist_point_ray(p: V3, s0: V3, v: V3, v_inv_len_sq: f64) -> f64 {
    let s = p - s0;
    let t = dot3(s, v);
    dot3(s, s) - t * t * v_inv_len_sq
}

/// `getSquaredDistance`.
fn sq_dist(p1: V3, p2: V3) -> f64 {
    dot3(p1 - p2, p1 - p2)
}

/// `getSignedDistanceToPlane`.
fn signed_dist(v: V3, p: &Plane) -> f64 {
    dot3(p.n, v) + p.d
}

/// `getTriangleNormal`: normalize((a - c) × (b - c)), written out as the C++ does.
fn triangle_normal(a: V3, b: V3, c: V3) -> V3 {
    let x = a.x - c.x;
    let y = a.y - c.y;
    let z = a.z - c.z;
    let rhsx = b.x - c.x;
    let rhsy = b.y - c.y;
    let rhsz = b.z - c.z;
    let px = y * rhsz - z * rhsy;
    let py = z * rhsx - x * rhsz;
    let pz = x * rhsy - y * rhsx;
    normalize3(v3(px, py, pz))
}

/// `MeshBuilder::Face`.
#[derive(Clone, Debug)]
struct Face {
    he: i32,
    p: Plane,
    most_distant_point_dist: f64,
    most_distant_point: usize,
    visibility_checked_on_iteration: usize,
    is_visible_face_on_current_iteration: bool,
    in_face_stack: bool,
    horizon_edges_on_current_iteration: u8,
    points_on_positive_side: Option<Vec<usize>>,
}

impl Face {
    fn new(he: i32) -> Face {
        Face {
            he,
            p: Plane::default(),
            most_distant_point_dist: 0.0,
            most_distant_point: 0,
            visibility_checked_on_iteration: 0,
            is_visible_face_on_current_iteration: false,
            in_face_stack: false,
            horizon_edges_on_current_iteration: 0,
            points_on_positive_side: None,
        }
    }
    fn is_disabled(&self) -> bool {
        self.he == -1
    }
}

const fn he(start_vert: i32, end_vert: i32, paired_halfedge: i32) -> Halfedge {
    Halfedge { start_vert, end_vert, paired_halfedge, prop_vert: 0 }
}

/// `MeshBuilder`.
#[derive(Default)]
struct MeshBuilder {
    faces: Vec<Face>,
    halfedges: Vec<Halfedge>,
    halfedge_to_face: Vec<i32>,
    halfedge_next: Vec<i32>,
    disabled_faces: Vec<usize>,
    disabled_halfedges: Vec<usize>,
}

impl MeshBuilder {
    fn face(&self, i: usize) -> R<&Face> {
        self.faces.get(i).ok_or(Fault::Index)
    }
    fn face_mut(&mut self, i: usize) -> R<&mut Face> {
        self.faces.get_mut(i).ok_or(Fault::Index)
    }
    fn hedge(&self, i: usize) -> R<Halfedge> {
        self.halfedges.atu(i)
    }
    fn hedge_mut(&mut self, i: usize) -> R<&mut Halfedge> {
        self.halfedges.atu_mut(i)
    }

    fn add_face(&mut self) -> usize {
        if let Some(index) = self.disabled_faces.pop() {
            if let Some(f) = self.faces.get_mut(index) {
                f.most_distant_point_dist = 0.0;
            }
            return index;
        }
        self.faces.push(Face::new(-1));
        self.faces.len() - 1
    }

    fn add_halfedge(&mut self) -> usize {
        if let Some(index) = self.disabled_halfedges.pop() {
            return index;
        }
        self.halfedges.push(Halfedge::default());
        self.halfedge_to_face.push(0);
        self.halfedge_next.push(0);
        self.halfedges.len() - 1
    }

    fn disable_face(&mut self, face_index: usize) -> R<Option<Vec<usize>>> {
        let f = self.face_mut(face_index)?;
        f.he = -1;
        let pts = f.points_on_positive_side.take();
        self.disabled_faces.push(face_index);
        Ok(pts)
    }

    fn disable_halfedge(&mut self, he_index: usize) -> R<()> {
        self.hedge_mut(he_index)?.paired_halfedge = -1;
        self.disabled_halfedges.push(he_index);
        Ok(())
    }

    /// `setup(a, b, c, d)`: the tetrahedron ABCD.
    fn setup(&mut self, a: usize, b: usize, c: usize, d: usize) -> R<()> {
        let to = |x: usize| i32::try_from(x).map_err(|_| Fault::TooLarge);
        let (a, b, c, d) = (to(a)?, to(b)?, to(c)?, to(d)?);
        self.faces.clear();
        self.disabled_faces.clear();
        self.disabled_halfedges.clear();
        self.halfedges = vec![
            he(0, b, 6),  // AB
            he(0, c, 9),  // BC
            he(0, a, 3),  // CA
            he(0, c, 2),  // AC
            he(0, d, 11), // CD
            he(0, a, 7),  // DA
            he(0, a, 0),  // BA
            he(0, d, 5),  // AD
            he(0, b, 10), // DB
            he(0, b, 1),  // CB
            he(0, d, 8),  // BD
            he(0, c, 4),  // DC
        ];
        self.halfedge_to_face = vec![0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3];
        self.halfedge_next = vec![1, 2, 0, 4, 5, 3, 7, 8, 6, 10, 11, 9];
        self.faces = vec![Face::new(0), Face::new(3), Face::new(6), Face::new(9)];
        Ok(())
    }

    fn halfedge_indices_of_face(&self, f: &Face) -> R<[usize; 3]> {
        let h0 = usize::try_from(f.he).map_err(|_| Fault::Index)?;
        let h1 = usize::try_from(self.halfedge_next.atu(h0)?).map_err(|_| Fault::Index)?;
        let h2 = usize::try_from(self.halfedge_next.atu(h1)?).map_err(|_| Fault::Index)?;
        Ok([h0, h1, h2])
    }

    fn vertex_indices_of_face(&self, f: &Face) -> R<[usize; 3]> {
        let [h0, h1, h2] = self.halfedge_indices_of_face(f)?;
        let v = |h: usize| -> R<usize> { usize::try_from(self.hedge(h)?.end_vert).map_err(|_| Fault::Index) };
        Ok([v(h0)?, v(h1)?, v(h2)?])
    }

    fn vertex_indices_of_halfedge(&self, h: Halfedge) -> R<[usize; 2]> {
        let pair = usize::try_from(h.paired_halfedge).map_err(|_| Fault::Index)?;
        let a = usize::try_from(self.hedge(pair)?.end_vert).map_err(|_| Fault::Index)?;
        let b = usize::try_from(h.end_vert).map_err(|_| Fault::Index)?;
        Ok([a, b])
    }
}

#[derive(Clone, Copy)]
struct FaceData {
    face_index: usize,
    entered_from_halfedge: i32,
}

/// `QuickHull`.
struct QuickHull {
    epsilon: f64,
    epsilon_squared: f64,
    planar: bool,
    /// `originalVertexData` (a copy: the planar and small cases replace it by an extended cloud)
    points: Vec<V3>,
    mesh: MeshBuilder,
    extreme_values: [usize; 6],
}

fn pt(points: &[V3], i: usize) -> R<V3> {
    points.atu(i)
}

impl QuickHull {
    /// `addPointToFace`.
    fn add_point_to_face(points: &[V3], eps_sq: f64, f: &mut Face, point_index: usize) -> R<bool> {
        let d = signed_dist(pt(points, point_index)?, &f.p);
        if d > 0.0 && d * d > eps_sq * f.p.sqr_n_length {
            f.points_on_positive_side.get_or_insert_with(Vec::new).push(point_index);
            if d > f.most_distant_point_dist {
                f.most_distant_point_dist = d;
                f.most_distant_point = point_index;
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// `getExtremeValues`.
    fn get_extreme_values(&self) -> R<[usize; 6]> {
        let mut out = [0usize; 6];
        let p0 = pt(&self.points, 0)?;
        let mut ev = [p0.x, p0.x, p0.y, p0.y, p0.z, p0.z];
        for (i, pos) in self.points.iter().enumerate().skip(1) {
            for (axis, c) in [pos.x, pos.y, pos.z].into_iter().enumerate() {
                let (hi, lo) = (2 * axis, 2 * axis + 1);
                let (Some(&ehi), Some(&elo)) = (ev.get(hi), ev.get(lo)) else { return Err(Fault::Index) };
                if c > ehi {
                    *ev.get_mut(hi).ok_or(Fault::Index)? = c;
                    *out.get_mut(hi).ok_or(Fault::Index)? = i;
                } else if c < elo {
                    *ev.get_mut(lo).ok_or(Fault::Index)? = c;
                    *out.get_mut(lo).ok_or(Fault::Index)? = i;
                }
            }
        }
        Ok(out)
    }

    /// `getScale`.
    fn get_scale(&self) -> R<f64> {
        let mut s = 0.0f64;
        for (i, &e) in self.extreme_values.iter().enumerate() {
            let p = pt(&self.points, e)?;
            let a = p.get(i / 2).abs();
            if a > s {
                s = a;
            }
        }
        Ok(s)
    }

    /// `reorderHorizonEdges`.
    fn reorder_horizon_edges(&self, horizon: &mut [usize]) -> R<bool> {
        let n = horizon.len();
        let mut i = 0;
        while i + 1 < n {
            let end_vertex_check = self.mesh.hedge(horizon.atu(i)?)?.end_vert;
            let mut found = false;
            for j in i + 1..n {
                let pair = usize::try_from(self.mesh.hedge(horizon.atu(j)?)?.paired_halfedge).map_err(|_| Fault::Index)?;
                let begin_vertex = self.mesh.hedge(pair)?.end_vert;
                if begin_vertex == end_vertex_check {
                    horizon.swap(i + 1, j);
                    found = true;
                    break;
                }
            }
            if !found {
                return Ok(false);
            }
            i += 1;
        }
        Ok(true)
    }

    /// `setupInitialTetrahedron`.
    fn setup_initial_tetrahedron(&mut self) -> R<()> {
        let vertex_count = self.points.len();
        if vertex_count <= 4 {
            if vertex_count < 4 {
                let last = self.points.last().copied().ok_or(Fault::Index)?;
                while self.points.len() < 4 {
                    self.points.push(last);
                }
            }
            let mut v = [0usize, 1, 2, 3];
            let n = triangle_normal(pt(&self.points, 0)?, pt(&self.points, 1)?, pt(&self.points, 2)?);
            let plane = Plane::new(n, pt(&self.points, 0)?);
            if plane.is_point_on_positive_side(pt(&self.points, 3)?) {
                v.swap(0, 1);
            }
            return self.mesh.setup(v[0], v[1], v[2], v[3]);
        }

        // the two most distant extreme points
        let mut max_d = self.epsilon_squared;
        let mut selected = (0usize, 0usize);
        for i in 0..6 {
            for j in i + 1..6 {
                let (Some(&ei), Some(&ej)) = (self.extreme_values.get(i), self.extreme_values.get(j)) else { return Err(Fault::Index) };
                let d = sq_dist(pt(&self.points, ei)?, pt(&self.points, ej)?);
                if d > max_d {
                    max_d = d;
                    selected = (ei, ej);
                }
            }
        }
        if max_d == self.epsilon_squared {
            // a single point
            return self.mesh.setup(0, 1, 2, 3);
        }

        // the point most distant from the line through them
        let s0 = pt(&self.points, selected.0)?;
        let rv = pt(&self.points, selected.1)? - s0;
        let inv = 1.0 / dot3(rv, rv);
        max_d = self.epsilon_squared;
        let mut max_i = usize::MAX;
        for (i, &p) in self.points.iter().enumerate() {
            let d = sq_dist_point_ray(p, s0, rv, inv);
            if d > max_d {
                max_d = d;
                max_i = i;
            }
        }
        if max_d == self.epsilon_squared {
            // a 1-dimensional cloud: a degenerate tetrahedron
            let (first, second) = selected;
            let mut third = 0usize;
            while third == first || third == second {
                third += 1;
            }
            let mut fourth = third + 1;
            while fourth == first || fourth == second {
                fourth += 1;
            }
            return self.mesh.setup(first, second, third, fourth);
        }

        let mut base = [selected.0, selected.1, max_i];
        let bv = [pt(&self.points, base[0])?, pt(&self.points, base[1])?, pt(&self.points, base[2])?];

        // the point farthest from the base plane
        max_d = self.epsilon;
        max_i = 0;
        let n = triangle_normal(bv[0], bv[1], bv[2]);
        let tri_plane = Plane::new(n, bv[0]);
        for (i, &p) in self.points.iter().enumerate() {
            let d = signed_dist(p, &tri_plane).abs();
            if d > max_d {
                max_d = d;
                max_i = i;
            }
        }
        if max_d == self.epsilon {
            // planar: one extra point gives the hull a volume (moved back onto point 0 in buildMesh)
            self.planar = true;
            let n1 = triangle_normal(bv[1], bv[2], bv[0]);
            let extra = n1 + pt(&self.points, 0)?;
            self.points.push(extra);
            max_i = self.points.len() - 1;
        }

        let tri_plane2 = Plane::new(n, bv[0]);
        if tri_plane2.is_point_on_positive_side(pt(&self.points, max_i)?) {
            base.swap(0, 1);
        }

        self.mesh.setup(base[0], base[1], base[2], max_i)?;
        for fi in 0..self.mesh.faces.len() {
            let v = self.mesh.vertex_indices_of_face(self.mesh.face(fi)?)?;
            let n1 = triangle_normal(pt(&self.points, v[0])?, pt(&self.points, v[1])?, pt(&self.points, v[2])?);
            let plane = Plane::new(n1, pt(&self.points, v[0])?);
            self.mesh.face_mut(fi)?.p = plane;
        }

        // each point outside the tetrahedron goes to the first face it is in front of
        let vcount = vertex_count;
        for i in 0..vcount {
            for fi in 0..self.mesh.faces.len() {
                let f = self.mesh.faces.get_mut(fi).ok_or(Fault::Index)?;
                if Self::add_point_to_face(&self.points, self.epsilon_squared, f, i)? {
                    break;
                }
            }
        }
        Ok(())
    }

    /// `createConvexHalfedgeMesh`.
    fn create_convex_halfedge_mesh(&mut self) -> R<()> {
        self.setup_initial_tetrahedron()?;
        if self.mesh.faces.len() != 4 {
            return Err(Fault::Index);
        }
        let mut face_list: VecDeque<usize> = VecDeque::new();
        for i in 0..4 {
            let f = self.mesh.face_mut(i)?;
            if f.points_on_positive_side.as_ref().is_some_and(|p| !p.is_empty()) {
                face_list.push_back(i);
                f.in_face_stack = true;
            }
        }

        let mut horizon: Vec<usize> = Vec::new();
        let mut possibly_visible: Vec<FaceData> = Vec::new();
        let mut visible_faces: Vec<usize> = Vec::new();
        let mut new_face_indices: Vec<usize> = Vec::new();
        let mut new_halfedge_indices: Vec<usize> = Vec::new();
        let mut disabled_point_vectors: Vec<Vec<usize>> = Vec::new();

        let n = self.points.len();
        let mut guard = Guard::new(n.saturating_mul(64).saturating_add(1 << 16));
        let mut iter: usize = 0;
        while let Some(top_face_index) = face_list.pop_front() {
            guard.tick()?;
            iter = iter.wrapping_add(1);
            if iter == usize::MAX {
                iter = 0;
            }
            let (active_point_index, has_points) = {
                let tf = self.mesh.face_mut(top_face_index)?;
                tf.in_face_stack = false;
                (tf.most_distant_point, tf.points_on_positive_side.is_some() && !tf.is_disabled())
            };
            if !has_points {
                continue;
            }
            let active_point = pt(&self.points, active_point_index)?;

            // the visible faces, and the horizon
            horizon.clear();
            possibly_visible.clear();
            visible_faces.clear();
            possibly_visible.push(FaceData { face_index: top_face_index, entered_from_halfedge: -1 });
            let mut vguard = Guard::new(self.mesh.halfedges.len().saturating_mul(4).saturating_add(64));
            while let Some(fd) = possibly_visible.pop() {
                vguard.tick()?;
                let pvf = self.mesh.face(fd.face_index)?;
                let mut not_visible = true;
                if pvf.visibility_checked_on_iteration == iter {
                    if pvf.is_visible_face_on_current_iteration {
                        continue;
                    }
                } else {
                    let p = pvf.p;
                    let d = dot3(p.n, active_point) + p.d;
                    let hes = self.mesh.halfedge_indices_of_face(pvf)?;
                    let pvf = self.mesh.face_mut(fd.face_index)?;
                    pvf.visibility_checked_on_iteration = iter;
                    if d > 0.0 {
                        pvf.is_visible_face_on_current_iteration = true;
                        pvf.horizon_edges_on_current_iteration = 0;
                        visible_faces.push(fd.face_index);
                        for he_index in hes {
                            let paired = self.mesh.hedge(he_index)?.paired_halfedge;
                            if paired != fd.entered_from_halfedge {
                                let face = usize::try_from(self.mesh.halfedge_to_face.at(paired)?).map_err(|_| Fault::Index)?;
                                possibly_visible.push(FaceData { face_index: face, entered_from_halfedge: he_index as i32 });
                            }
                        }
                        not_visible = false;
                    }
                }
                if not_visible {
                    self.mesh.face_mut(fd.face_index)?.is_visible_face_on_current_iteration = false;
                    let entered = usize::try_from(fd.entered_from_halfedge).map_err(|_| Fault::Index)?;
                    horizon.push(entered);
                    let owner = usize::try_from(self.mesh.halfedge_to_face.atu(entered)?).map_err(|_| Fault::Index)?;
                    let hes = self.mesh.halfedge_indices_of_face(self.mesh.face(owner)?)?;
                    let ind = if hes[0] == entered {
                        0
                    } else if hes[1] == entered {
                        1
                    } else {
                        2
                    };
                    let f = self.mesh.face_mut(owner)?;
                    f.horizon_edges_on_current_iteration = (f.horizon_edges_on_current_iteration | (1u8 << ind)) & 7;
                }
            }
            let horizon_edge_count = horizon.len();

            if !self.reorder_horizon_edges(&mut horizon)? {
                // numerically unstable: drop this point from the face and accept a minor degeneration
                let tf = self.mesh.face_mut(top_face_index)?;
                let mut emptied = false;
                if let Some(pts) = tf.points_on_positive_side.as_mut() {
                    let mut change_flag = 0;
                    for index in 0..pts.len() {
                        if pts.atu(index)? == active_point_index {
                            change_flag = 1;
                        } else if change_flag == 1 {
                            change_flag = 2;
                            let v = pts.atu(index)?;
                            pts.setu(index - 1, v)?;
                        }
                    }
                    if change_flag == 1 {
                        pts.pop();
                    }
                    emptied = pts.is_empty();
                }
                if emptied {
                    tf.points_on_positive_side = None;
                }
                continue;
            }

            // the visible faces' halfedges (but the horizon) are reused or disabled; the faces are disabled
            new_face_indices.clear();
            new_halfedge_indices.clear();
            disabled_point_vectors.clear();
            let mut disable_counter = 0usize;
            for &face_index in &visible_faces {
                let (hes, horizon_bits) = {
                    let f = self.mesh.face(face_index)?;
                    (self.mesh.halfedge_indices_of_face(f)?, f.horizon_edges_on_current_iteration)
                };
                for (j, &h) in hes.iter().enumerate() {
                    if horizon_bits & (1u8 << j) == 0 {
                        if disable_counter < horizon_edge_count * 2 {
                            new_halfedge_indices.push(h);
                            disable_counter += 1;
                        } else {
                            self.mesh.disable_halfedge(h)?;
                        }
                    }
                }
                if let Some(t) = self.mesh.disable_face(face_index)? {
                    disabled_point_vectors.push(t);
                }
            }
            if disable_counter < horizon_edge_count * 2 {
                let needed = horizon_edge_count * 2 - disable_counter;
                for _ in 0..needed {
                    let h = self.mesh.add_halfedge();
                    new_halfedge_indices.push(h);
                }
            }

            // the new faces, fanned from the active point over the horizon loop
            for i in 0..horizon_edge_count {
                let ab = horizon.atu(i)?;
                let [a, b] = self.mesh.vertex_indices_of_halfedge(self.mesh.hedge(ab)?)?;
                let c = active_point_index;
                let new_face_index = self.mesh.add_face();
                new_face_indices.push(new_face_index);
                let ca = new_halfedge_indices.atu(2 * i)?;
                let bc = new_halfedge_indices.atu(2 * i + 1)?;
                self.mesh.halfedge_next.setu(ab, bc as i32)?;
                self.mesh.halfedge_next.setu(bc, ca as i32)?;
                self.mesh.halfedge_next.setu(ca, ab as i32)?;
                self.mesh.halfedge_to_face.setu(bc, new_face_index as i32)?;
                self.mesh.halfedge_to_face.setu(ca, new_face_index as i32)?;
                self.mesh.halfedge_to_face.setu(ab, new_face_index as i32)?;
                self.mesh.hedge_mut(ca)?.end_vert = a as i32;
                self.mesh.hedge_mut(bc)?.end_vert = c as i32;
                let normal = triangle_normal(pt(&self.points, a)?, pt(&self.points, b)?, active_point);
                let plane = Plane::new(normal, active_point);
                let nf = self.mesh.face_mut(new_face_index)?;
                nf.p = plane;
                nf.he = ab as i32;
                let ca_pair = new_halfedge_indices.atu(if i > 0 { i * 2 - 1 } else { 2 * horizon_edge_count - 1 })?;
                let bc_pair = new_halfedge_indices.atu(((i + 1) * 2) % (horizon_edge_count * 2))?;
                self.mesh.hedge_mut(ca)?.paired_halfedge = ca_pair as i32;
                self.mesh.hedge_mut(bc)?.paired_halfedge = bc_pair as i32;
            }

            // the points in front of the disabled faces go to the new faces
            for disabled_points in &disabled_point_vectors {
                for &point in disabled_points {
                    if point == active_point_index {
                        continue;
                    }
                    for j in 0..horizon_edge_count {
                        let fi = new_face_indices.atu(j)?;
                        let f = self.mesh.faces.get_mut(fi).ok_or(Fault::Index)?;
                        if Self::add_point_to_face(&self.points, self.epsilon_squared, f, point)? {
                            break;
                        }
                    }
                }
            }

            for &new_face_index in &new_face_indices {
                let f = self.mesh.face_mut(new_face_index)?;
                if f.points_on_positive_side.is_some() && !f.in_face_stack {
                    face_list.push_back(new_face_index);
                    f.in_face_stack = true;
                }
            }
        }
        Ok(())
    }

    /// `buildMesh`: the hull's halfedges (three per face, faces in builder order) and its used vertices.
    fn build_mesh(&mut self, epsilon: f64) -> R<(Vec<Halfedge>, Vec<V3>)> {
        if self.points.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        self.extreme_values = self.get_extreme_values()?;
        let scale = self.get_scale()?;
        self.epsilon = epsilon * scale;
        self.epsilon_squared = self.epsilon * self.epsilon;
        self.planar = false;
        self.create_convex_halfedge_mesh()?;
        if self.planar {
            let front = self.points.first().copied().ok_or(Fault::Index)?;
            if let Some(back) = self.points.last_mut() {
                *back = front;
            }
        }

        // reorder halfedges: each live face's three, in the order its first live halfedge appears
        let m = &self.mesh;
        let nh = m.halfedges.len();
        let mut halfedges = vec![Halfedge::default(); nh];
        let mut counts = vec![0i32; nh];
        let mut mapping = vec![0i32; nh];
        let mut j = 0usize;
        for i in 0..nh {
            let h = m.hedge(i)?;
            if h.paired_halfedge < 0 {
                continue;
            }
            let face = m.halfedge_to_face.atu(i)?;
            if m.faces.get(usize::try_from(face).map_err(|_| Fault::Index)?).ok_or(Fault::Index)?.is_disabled() {
                continue;
            }
            // AtomicAdd(counts[face], 1) > 0 (counts has nh slots, indexed by face)
            let c = counts.at_mut(face)?;
            let old = *c;
            *c += 1;
            if old > 0 {
                continue;
            }
            let curr = j;
            j += 3;
            mapping.setu(i, curr as i32)?;
            halfedges.setu(curr, h)?;
            let k1 = usize::try_from(m.halfedge_next.atu(i)?).map_err(|_| Fault::Index)?;
            mapping.setu(k1, (curr + 1) as i32)?;
            halfedges.setu(curr + 1, m.hedge(k1)?)?;
            let k2 = usize::try_from(m.halfedge_next.atu(k1)?).map_err(|_| Fault::Index)?;
            mapping.setu(k2, (curr + 2) as i32)?;
            halfedges.setu(curr + 2, m.hedge(k2)?)?;
            let e0 = halfedges.atu(curr)?.end_vert;
            let e1 = halfedges.atu(curr + 1)?.end_vert;
            let e2 = halfedges.atu(curr + 2)?.end_vert;
            halfedges.atu_mut(curr)?.start_vert = e2;
            halfedges.atu_mut(curr + 1)?.start_vert = e0;
            halfedges.atu_mut(curr + 2)?.start_vert = e1;
        }
        halfedges.truncate(j);
        for h in &mut halfedges {
            h.paired_halfedge = mapping.at(h.paired_halfedge)?;
        }

        // drop unused vertices
        let nv = self.points.len();
        let mut counts = vec![0i32; nv + 1];
        for h in &halfedges {
            *counts.at_mut(h.start_vert)? += 1;
        }
        let mut sum = 0i32;
        for c in &mut counts {
            let sat = i32::from(*c > 0);
            *c = sum;
            sum += sat;
        }
        let total = usize::try_from(counts.last().copied().unwrap_or(0)).map_err(|_| Fault::Index)?;
        let mut vertices = vec![V3::default(); total];
        for (i, &p) in self.points.iter().enumerate() {
            let (a, b) = (counts.atu(i)?, counts.atu(i + 1)?);
            if b - a > 0 {
                vertices.set(a, p)?;
            }
        }
        for h in &mut halfedges {
            h.start_vert = counts.at(h.start_vert)?;
            h.end_vert = counts.at(h.end_vert)?;
        }
        Ok((halfedges, vertices))
    }
}

/// `QuickHull(points).buildMesh()`: the hull's halfedges and vertices.
pub(crate) fn quick_hull(points: &[V3]) -> R<(Vec<Halfedge>, Vec<V3>)> {
    let mut qh = QuickHull {
        epsilon: 0.0,
        epsilon_squared: 0.0,
        planar: false,
        points: points.to_vec(),
        mesh: MeshBuilder::default(),
        extreme_values: [0; 6],
    };
    qh.build_mesh(DEFAULT_EPS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_corners_make_twelve_triangles() {
        let mut pts = Vec::new();
        for x in [0.0, 1.0] {
            for y in [0.0, 1.0] {
                for z in [0.0, 1.0] {
                    pts.push(v3(x, y, z));
                }
            }
        }
        pts.push(v3(0.5, 0.5, 0.5));
        let (he, v) = quick_hull(&pts).unwrap();
        assert_eq!(v.len(), 8);
        assert_eq!(he.len(), 36);
        for (i, h) in he.iter().enumerate() {
            let p = he[h.paired_halfedge as usize];
            assert_eq!(p.paired_halfedge as usize, i);
            assert_eq!((p.start_vert, p.end_vert), (h.end_vert, h.start_vert));
        }
    }

    #[test]
    fn hostile_input_does_not_panic() {
        let _ = quick_hull(&[v3(f64::NAN, 0.0, 0.0); 9]);
        let _ = quick_hull(&[
            v3(f64::INFINITY, 1.0, 0.0),
            v3(0.0, 0.0, 0.0),
            v3(1.0, 1.0, 1.0),
            v3(0.0, f64::NEG_INFINITY, 2.0),
            v3(3.0, 0.0, 1.0),
        ]);
        let _ = quick_hull(&[v3(1.0, 2.0, 3.0)]);
        let _ = quick_hull(&[v3(0.0, 0.0, 0.0); 7]);
    }
}
