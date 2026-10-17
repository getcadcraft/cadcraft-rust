// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! manifold.cpp and csg_tree.cpp: the public `Manifold`, its lazy CSG tree with lazy transforms, and the evaluation of
//! a tree (nested unions / intersections / differences collapsed into batches, disjoint pieces composed, the rest
//! reduced pairwise from the largest meshes).
//!
//! Nodes are reference-counted (`Arc`) like the C++ `shared_ptr`s, and the collapse rule reads the reference counts the
//! same way: a nested operation folds into its parent only when the parent's child list and the evaluation stack are
//! its only holders and no transformed copy shares its children. Dropping a [`Manifold`] here is `.delete()` in
//! JavaScript.
//!
//! Transforms are lazy as in the C++: a leaf carries a pending 3×4 matrix that is applied (`Impl::Transform`) the first
//! time its mesh is needed, an operation node carries one that is applied to its result; `Compose` and the bounding
//! box tests read the pending matrix without applying it. Matrices compose as `m * Mat4(t)`, multiplying through the
//! zeros, so the same chain of calls gives the same floats.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::sync::{Arc, Mutex, PoisonError};

use crate::boolean3::Boolean3;
use crate::imp::{Impl, K_PRECISION, mesh_id_counter};
use crate::linalg::{Box3, V3, V4, v3};
use crate::mat::{IDENTITY, Mat34, normal_transform};
use crate::util::{Fault, Get, GetMut, R, ix, std_max, std_min};
use crate::{Mesh, OpType, Status};

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `CsgLeafNode`: a mesh and the transform still to be applied to it.
pub(crate) struct Leaf {
    st: Mutex<(Arc<Impl>, Mat34)>,
}

impl Leaf {
    fn new(imp: Arc<Impl>, t: Mat34) -> Arc<Leaf> {
        Arc::new(Leaf { st: Mutex::new((imp, t)) })
    }

    fn of(imp: Impl) -> Arc<Leaf> {
        Leaf::new(Arc::new(imp), IDENTITY)
    }

    /// `GetImpl`: applies the pending transform once (the node keeps the result).
    pub(crate) fn get_impl(&self) -> Arc<Impl> {
        let mut g = lock(&self.st);
        if g.1.is_identity() {
            return Arc::clone(&g.0);
        }
        let imp = Arc::new(g.0.transform(&g.1));
        g.0 = Arc::clone(&imp);
        g.1 = IDENTITY;
        imp
    }

    /// `Transform(m)`: a new leaf sharing the mesh, with `m * Mat4(transform)` pending.
    fn transform(&self, m: &Mat34) -> Arc<Leaf> {
        let g = lock(&self.st);
        Leaf::new(Arc::clone(&g.0), m.mul_mat4(&g.1))
    }

    fn parts(&self) -> (Arc<Impl>, Mat34) {
        let g = lock(&self.st);
        (Arc::clone(&g.0), g.1)
    }

    /// `GetBoundingBox`: the box under the pending transform (Arvo's method), without applying it.
    fn bounding_box(&self) -> Box3 {
        let g = lock(&self.st);
        let b = g.0.bbox;
        let t = &g.1;
        if t.is_identity() {
            return b;
        }
        let mut mn = t.0[3];
        let mut mx = mn;
        for i in 0..3 {
            for j in 0..3 {
                let a = t.at(j, i) * b.min.get(j);
                let c = t.at(j, i) * b.max.get(j);
                mn.set(i, mn.get(i) + std_min(a, c));
                mx.set(i, mx.get(i) + std_max(a, c));
            }
        }
        Box3 { min: mn, max: mx }
    }

    fn num_vert(&self) -> usize {
        lock(&self.st).0.num_vert()
    }
}

/// `CsgOpNode`: a deferred operation. `children` is shared with the transformed copies of this node (`impl_`), the
/// cache (the transformed result) is not.
pub(crate) struct OpNode {
    children: Arc<Mutex<Vec<NodeRef>>>,
    op: OpType,
    transform: Mat34,
    cache: Mutex<Option<Arc<Leaf>>>,
}

impl Drop for OpNode {
    /// `~CsgOpNode`: releases a deep tree iteratively (a long chain of operations would overflow the stack).
    fn drop(&mut self) {
        if Arc::strong_count(&self.children) != 1 {
            return;
        }
        let mut pending: Vec<Arc<OpNode>> = Vec::new();
        let take = |v: &mut Vec<NodeRef>, pending: &mut Vec<Arc<OpNode>>| {
            while let Some(c) = v.pop() {
                if let NodeRef::Op(o) = c {
                    pending.push(o);
                }
            }
        };
        take(&mut lock(&self.children), &mut pending);
        while let Some(child) = pending.pop() {
            if Arc::strong_count(&child) == 1 && Arc::strong_count(&child.children) == 1 {
                let mut v = core::mem::take(&mut *lock(&child.children));
                take(&mut v, &mut pending);
            }
        }
    }
}

/// A node handle (`std::shared_ptr<CsgNode>`).
#[derive(Clone)]
pub(crate) enum NodeRef {
    Leaf(Arc<Leaf>),
    Op(Arc<OpNode>),
}

impl NodeRef {
    fn op(children: Vec<NodeRef>, op: OpType) -> NodeRef {
        NodeRef::Op(Arc::new(OpNode { children: Arc::new(Mutex::new(children)), op, transform: IDENTITY, cache: Mutex::new(None) }))
    }

    /// `CsgNode::Transform`.
    fn transform(&self, m: &Mat34) -> NodeRef {
        match self {
            NodeRef::Leaf(l) => NodeRef::Leaf(l.transform(m)),
            NodeRef::Op(o) => NodeRef::Op(Arc::new(OpNode {
                children: Arc::clone(&o.children),
                op: o.op,
                transform: m.mul_mat4(&o.transform),
                cache: Mutex::new(None),
            })),
        }
    }
}

/// A solid: an oriented 2-manifold triangle mesh, or a deferred boolean or transform of such solids (evaluated when its
/// mesh, status or a property is asked for). Cloning shares the node, as copying a C++ `Manifold` does.
pub struct Manifold {
    node: Mutex<NodeRef>,
}

impl Clone for Manifold {
    fn clone(&self) -> Self {
        Manifold { node: Mutex::new(lock(&self.node).clone()) }
    }
}

impl core::fmt::Debug for Manifold {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Manifold")
    }
}

impl Default for Manifold {
    /// An empty Manifold (`Manifold()`).
    fn default() -> Self {
        Manifold::from_impl(Impl::default())
    }
}

impl Manifold {
    fn from_node(node: NodeRef) -> Manifold {
        Manifold { node: Mutex::new(node) }
    }

    fn load(&self) -> NodeRef {
        lock(&self.node).clone()
    }

    /// `Manifold(std::shared_ptr<Impl>)`: a leaf around a finished Impl.
    pub(crate) fn from_impl(imp: Impl) -> Manifold {
        Manifold::from_node(NodeRef::Leaf(Leaf::of(imp)))
    }

    /// An empty Manifold with an error status (`Invalid()` / `PropagateStatus`).
    pub(crate) fn with_status(status: Status) -> Manifold {
        Manifold::from_impl(Impl::with_status(status))
    }

    /// `Manifold(const MeshGL&)` plus manifold-3d's status check: the solid, or why the mesh is not one.
    pub fn from_mesh(mesh: &Mesh) -> Result<Manifold, Status> {
        let imp = Impl::from_mesh(mesh);
        if imp.status != Status::NoError {
            return Err(imp.status);
        }
        Ok(Manifold::from_impl(imp))
    }

    /// `Manifold(const MeshGL&)` without the check (the status stays on the Manifold).
    pub fn from_mesh_unchecked(mesh: &Mesh) -> Manifold {
        Manifold::from_impl(Impl::from_mesh(mesh))
    }

    /// `GetCsgLeafNode()`: a deferred tree is evaluated once and replaced by its result.
    fn leaf(&self) -> Arc<Leaf> {
        let mut g = lock(&self.node);
        let leaf = match &*g {
            NodeRef::Leaf(l) => return Arc::clone(l),
            NodeRef::Op(o) => to_leaf(o).unwrap_or_else(|f| Leaf::of(Impl::with_status(f.status()))),
        };
        *g = NodeRef::Leaf(Arc::clone(&leaf));
        leaf
    }

    /// `GetCsgLeafNode().GetImpl()`.
    pub(crate) fn leaf_impl(&self) -> Arc<Impl> {
        self.leaf().get_impl()
    }

    /// `Boolean(second, op)`: a deferred operation. A union or intersection of a mesh with a deferred tree on the right
    /// joins that tree's side, as the C++ does.
    pub fn boolean(&self, other: &Manifold, op: OpType) -> Manifold {
        let a = self.load();
        let b = other.load();
        let node = match (&a, &b) {
            (NodeRef::Leaf(_), NodeRef::Op(_)) if op == OpType::Add || op == OpType::Intersect => NodeRef::op(vec![b, a], op),
            _ => NodeRef::op(vec![a, b], op),
        };
        Manifold::from_node(node)
    }

    /// `a - b` (manifold-3d `subtract`).
    pub fn subtract(&self, other: &Manifold) -> Manifold {
        self.boolean(other, OpType::Subtract)
    }

    /// `a + b` (manifold-3d `add`).
    pub fn add(&self, other: &Manifold) -> Manifold {
        self.boolean(other, OpType::Add)
    }

    /// `a ^ b` (manifold-3d `intersect`).
    pub fn intersect(&self, other: &Manifold) -> Manifold {
        self.boolean(other, OpType::Intersect)
    }

    /// `BatchBoolean(list, op)`: none → empty, one → that Manifold (shared), more → one deferred node.
    pub fn batch_boolean(list: &[Manifold], op: OpType) -> Manifold {
        match list {
            [] => Manifold::default(),
            [one] => one.clone(),
            _ => Manifold::from_node(NodeRef::op(list.iter().map(Manifold::load).collect(), op)),
        }
    }

    /// manifold-3d `Manifold.union(list)`.
    pub fn union(list: &[Manifold]) -> Manifold {
        Manifold::batch_boolean(list, OpType::Add)
    }

    /// manifold-3d `Manifold.intersection(list)`.
    pub fn intersection(list: &[Manifold]) -> Manifold {
        Manifold::batch_boolean(list, OpType::Intersect)
    }

    /// manifold-3d `Manifold.difference(list)`: the first minus all the others.
    pub fn difference(list: &[Manifold]) -> Manifold {
        Manifold::batch_boolean(list, OpType::Subtract)
    }

    /// `Transform(m)` with `m` a column-major 3×4 (`[x0 x1 x2, y0 y1 y2, z0 z1 z2, t0 t1 t2]`): lazy.
    pub fn transform(&self, m: &[f64; 12]) -> Manifold {
        Manifold::from_node(self.load().transform(&Mat34::from_array(m)))
    }

    pub(crate) fn transform_mat(&self, m: &Mat34) -> Manifold {
        Manifold::from_node(self.load().transform(m))
    }

    /// manifold-3d `transform(mat4)`: a column-major 4×4 as JavaScript passes it (the last row is ignored).
    pub fn transform_mat4(&self, m: &[f64; 16]) -> Manifold {
        self.transform_mat(&Mat34::from_mat4_js(m))
    }

    /// `Translate(v)`: `identity` with `[3] += v`.
    pub fn translate(&self, v: [f64; 3]) -> Manifold {
        let mut t = IDENTITY;
        t.0[3] += v3(v[0], v[1], v[2]);
        self.transform_mat(&t)
    }

    /// `Scale(v)`: a zero matrix with `v` on the diagonal.
    pub fn scale(&self, v: [f64; 3]) -> Manifold {
        let z = V3::default();
        let t = Mat34([v3(v[0], 0.0, 0.0), v3(0.0, v[1], 0.0), v3(0.0, 0.0, v[2]), z]);
        self.transform_mat(&t)
    }

    /// `Rotate(x, y, z)` in degrees: `rZ * rY * rX`.
    pub fn rotate(&self, x: f64, y: f64, z: f64) -> Manifold {
        self.transform_mat(&rotation(x, y, z))
    }

    /// `Mirror(normal)`: evaluates (a failed solid passes its status on); a zero normal gives an empty Manifold.
    pub fn mirror(&self, normal: [f64; 3]) -> Manifold {
        let imp = self.leaf_impl();
        if imp.status != Status::NoError {
            return Manifold::with_status(imp.status);
        }
        let n = v3(normal[0], normal[1], normal[2]);
        if crate::linalg::length3(n) == 0.0 {
            return Manifold::default();
        }
        let n = crate::linalg::normalize3(n);
        // mat3(identity) - 2.0 * outerprod(n, n), outerprod = {n * n.x, n * n.y, n * n.z}
        let id = crate::mat::Mat3::IDENTITY.0;
        let o = [n * n.x, n * n.y, n * n.z];
        let col = |k: usize| -> V3 {
            let (i, p) = (id.get(k).copied().unwrap_or_default(), o.get(k).copied().unwrap_or_default());
            i - p * 2.0
        };
        let m = Mat34([col(0), col(1), col(2), V3::default()]);
        self.transform_mat(&m)
    }

    /// `Status()` (evaluates).
    pub fn status(&self) -> Status {
        self.leaf_impl().status
    }

    /// `IsEmpty()` (evaluates).
    pub fn is_empty(&self) -> bool {
        self.leaf_impl().is_empty()
    }

    /// Number of vertices (evaluates).
    pub fn num_vert(&self) -> usize {
        self.leaf_impl().num_vert()
    }

    /// Number of edges (evaluates).
    pub fn num_edge(&self) -> usize {
        self.leaf_impl().halfedge.len() / 2
    }

    /// Number of triangles (evaluates).
    pub fn num_tri(&self) -> usize {
        self.leaf_impl().num_tri()
    }

    /// `GetMeshGL()` (manifold-3d `getMesh()`; evaluates).
    pub fn get_mesh(&self) -> Mesh {
        let imp = self.leaf_impl();
        crate::export::get_mesh(&imp).unwrap_or_else(|_| Mesh { num_prop: 3, ..Mesh::default() })
    }

    /// `BoundingBox()` as `(min, max)` (evaluates).
    pub fn bounding_box(&self) -> ([f64; 3], [f64; 3]) {
        let b = self.leaf_impl().bbox;
        ([b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z])
    }

    /// `Genus()`: `1 - (V - E + F) / 2` (integer division).
    pub fn genus(&self) -> i64 {
        let chi = self.num_vert() as i64 - self.num_edge() as i64 + self.num_tri() as i64;
        1 - chi / 2
    }

    /// `Volume()` (Kahan sum over the triangles).
    pub fn volume(&self) -> f64 {
        self.leaf_impl().get_property(false)
    }

    /// `SurfaceArea()`.
    pub fn surface_area(&self) -> f64 {
        self.leaf_impl().get_property(true)
    }

    /// `OriginalID()`: this mesh's ID if it is an original, else -1.
    pub fn original_id(&self) -> i32 {
        self.leaf_impl().mesh_relation.original_id
    }

    /// `AsOriginal()`: a new original (one new mesh ID), its faces regrouped.
    pub fn as_original(&self) -> Manifold {
        let old = self.leaf_impl();
        if old.status != Status::NoError {
            return Manifold::with_status(old.status);
        }
        let mut imp = (*old).clone();
        imp.initialize_original();
        if let Err(f) = imp.set_normals_and_coplanar() {
            return Manifold::with_status(f.status());
        }
        Manifold::from_impl(imp)
    }

    /// The number of `Decompose()` pieces: the vertex-connected components that have at least one triangle (the
    /// C++ builds a Manifold per piece; no mesh IDs are used, so counting gives the same answer).
    pub fn decompose_count(&self) -> usize {
        let imp = self.leaf_impl();
        if imp.status != Status::NoError {
            return 1;
        }
        imp.component_count().unwrap_or(1)
    }

    /// `TrimByPlane(normal, offset)`: `this ^ Halfspace(BoundingBox(), normal, offset)`.
    pub fn trim_by_plane(&self, normal: [f64; 3], origin_offset: f64) -> Manifold {
        let b = self.leaf_impl().bbox;
        let h = halfspace(b, v3(normal[0], normal[1], normal[2]), origin_offset);
        self.intersect(&h)
    }

    /// `MinGap(other, searchLength)`: 0 when they overlap (the intersection is evaluated, as in the C++), else the
    /// closest distance up to `search_length`.
    pub fn min_gap(&self, other: &Manifold, search_length: f64) -> f64 {
        let inter = self.intersect(other);
        if !inter.is_empty() {
            return 0.0;
        }
        let a = self.leaf_impl();
        let b = other.leaf_impl();
        a.min_gap(&b, search_length).unwrap_or(search_length)
    }

    /// `Cube(size, center)`.
    pub fn cube(size: [f64; 3], center: bool) -> Manifold {
        crate::constructors::cube(v3(size[0], size[1], size[2]), center)
    }

    /// `Sphere(radius, circularSegments)`.
    pub fn sphere(radius: f64, circular_segments: i32) -> Manifold {
        crate::constructors::sphere(radius, circular_segments)
    }

    /// `Extrude(polygons, height, nDivisions, twistDegrees, scaleTop)`.
    pub fn extrude(polys: &[Vec<[f64; 2]>], height: f64, n_divisions: i32, twist_degrees: f64, scale_top: [f64; 2]) -> Manifold {
        crate::constructors::extrude(polys, height, n_divisions, twist_degrees, scale_top)
    }

    /// `Revolve(polygons, circularSegments, revolveDegrees)`.
    pub fn revolve(polys: &[Vec<[f64; 2]>], circular_segments: i32, revolve_degrees: f64) -> Manifold {
        crate::constructors::revolve(polys, circular_segments, revolve_degrees)
    }
}

/// `CsgNode::Rotate` matrix: `mat3x4(rZ * rY * rX, vec3())`.
pub(crate) fn rotation(x: f64, y: f64, z: f64) -> Mat34 {
    use crate::mat::Mat3;
    use crate::math::{cosd, sind};
    let rx = Mat3([v3(1.0, 0.0, 0.0), v3(0.0, cosd(x), sind(x)), v3(0.0, -sind(x), cosd(x))]);
    let ry = Mat3([v3(cosd(y), 0.0, -sind(y)), v3(0.0, 1.0, 0.0), v3(sind(y), 0.0, cosd(y))]);
    let rz = Mat3([v3(cosd(z), sind(z), 0.0), v3(-sind(z), cosd(z), 0.0), v3(0.0, 0.0, 1.0)]);
    Mat34::from_parts(rz.mul(&ry).mul(&rx), V3::default())
}

/// manifold.cpp `Halfspace`: a cube big enough for the box, on the normal's side of the plane.
fn halfspace(b: Box3, normal: V3, origin_offset: f64) -> Manifold {
    let normal = crate::linalg::normalize3(normal);
    let cutter = Manifold::cube([2.0, 2.0, 2.0], true).translate([1.0, 0.0, 0.0]);
    let center = (b.max + b.min) * 0.5;
    let size = crate::linalg::length3(center - normal * origin_offset) + 0.5 * crate::linalg::length3(b.size());
    let cutter = cutter.scale([size, size, size]).translate([origin_offset, 0.0, 0.0]);
    let y_deg = crate::math::degrees(-crate::math::asin(normal.z));
    let z_deg = crate::math::degrees(crate::math::atan2(normal.y, normal.x));
    cutter.rotate(0.0, y_deg, z_deg)
}

/// `SimpleBoolean`.
fn simple_boolean(a: &Impl, b: &Impl, op: OpType) -> R<Arc<Leaf>> {
    let b3 = Boolean3::new(a, b, op)?;
    Ok(Leaf::of(b3.result(op)?))
}

/// `BatchBoolean`: reduce pairwise, always the two largest (vertex count, then insertion order) first.
fn batch_boolean(op: OpType, mut results: Vec<Arc<Leaf>>) -> R<Arc<Leaf>> {
    match results.len() {
        0 => return Ok(Leaf::of(Impl::default())),
        1 => return results.pop().ok_or(Fault::Index),
        2 => {
            let b = results.pop().ok_or(Fault::Index)?;
            let a = results.pop().ok_or(Fault::Index)?;
            return simple_boolean(&a.get_impl(), &b.get_impl(), op);
        }
        _ => {}
    }
    // the heap only ever yields its maximum, and the keys (numVert, serial) are unique: a plain max search
    let mut heap: Vec<(Arc<Leaf>, u64)> = results.into_iter().zip(0u64..).collect();
    let mut next_serial = heap.len() as u64;
    let pop_max = |heap: &mut Vec<(Arc<Leaf>, u64)>| -> R<Arc<Leaf>> {
        let mut best = 0usize;
        let mut best_key = (0usize, 0u64);
        for (i, (l, s)) in heap.iter().enumerate() {
            let key = (l.num_vert(), *s);
            if i == 0 || key > best_key {
                best = i;
                best_key = key;
            }
        }
        if best >= heap.len() {
            return Err(Fault::Index);
        }
        Ok(heap.swap_remove(best).0)
    };
    while heap.len() > 1 {
        let mut tmp = Vec::new();
        let mut i = 0;
        while i < 4 && heap.len() > 1 {
            let a = pop_max(&mut heap)?;
            let b = pop_max(&mut heap)?;
            tmp.push((simple_boolean(&a.get_impl(), &b.get_impl(), op)?, next_serial));
            next_serial += 1;
            i += 1;
        }
        heap.extend(tmp);
    }
    heap.pop().map(|x| x.0).ok_or(Fault::Index)
}

/// `BatchUnion`: pairwise-disjoint groups (by bounding box) are composed, the groups unioned, from the back.
fn batch_union(children: &mut Vec<Arc<Leaf>>) -> R<Arc<Leaf>> {
    const MAX_UNION_SIZE: usize = 1000;
    let mut guard = crate::util::Guard::new(children.len() + 1);
    while children.len() > 1 {
        guard.tick()?;
        let start = if children.len() > MAX_UNION_SIZE { children.len() - MAX_UNION_SIZE } else { 0 };
        let boxes: Vec<Box3> = children.iter().skip(start).map(|c| c.bounding_box()).collect();
        let mut sets: Vec<Vec<usize>> = Vec::new();
        for i in 0..boxes.len() {
            let bi = boxes.atu(i)?;
            let found = sets.iter().position(|set| set.iter().all(|&j| boxes.get(j).is_none_or(|bj| !bi.does_overlap(bj))));
            match found {
                Some(k) => sets.get_mut(k).ok_or(Fault::Index)?.push(i),
                None => sets.push(vec![i]),
            }
        }
        let mut leaves = Vec::with_capacity(sets.len());
        for set in &sets {
            if let [one] = set.as_slice() {
                leaves.push(Arc::clone(children.get(start + one).ok_or(Fault::Index)?));
            } else {
                let nodes: Vec<Arc<Leaf>> = set.iter().map(|&j| children.get(start + j).cloned().ok_or(Fault::Index)).collect::<R<_>>()?;
                leaves.push(compose(&nodes)?);
            }
        }
        children.truncate(start);
        children.push(batch_boolean(OpType::Add, leaves)?);
        let last = children.len() - 1;
        children.swap(0, last);
    }
    children.first().cloned().ok_or(Fault::Index)
}

/// `CsgLeafNode::Compose`: a union of pairwise-disjoint meshes by concatenation, each node's pending transform
/// applied on the fly.
fn compose(leaves: &[Arc<Leaf>]) -> R<Arc<Leaf>> {
    let nodes: Vec<(Arc<Impl>, Mat34, Box3)> = leaves
        .iter()
        .map(|l| {
            let (i, t) = l.parts();
            (i, t, l.bounding_box())
        })
        .collect();
    let mut epsilon = -1.0f64;
    let mut tolerance = -1.0f64;
    let mut bbox = Box3::default();
    let (mut num_vert, mut num_edge, mut num_tri, mut num_prop_vert) = (0usize, 0usize, 0usize, 0usize);
    let mut vert_idx = Vec::new();
    let mut edge_idx = Vec::new();
    let mut tri_idx = Vec::new();
    let mut prop_idx = Vec::new();
    let mut num_prop_out = 0i32;
    for (n, t, nb) in &nodes {
        if n.status != Status::NoError {
            return Ok(Leaf::of(Impl::with_status(n.status)));
        }
        let old_scale = n.bbox.scale();
        let new_scale = n.bbox.transform(t).scale();
        let mut ne = n.epsilon;
        ne *= std_max(1.0, new_scale / old_scale);
        ne = std_max(ne, K_PRECISION * new_scale);
        if !ne.is_finite() {
            ne = -1.0;
        }
        epsilon = std_max(epsilon, ne);
        tolerance = std_max(tolerance, n.tolerance);
        bbox = bbox.union(nb);
        vert_idx.push(num_vert);
        edge_idx.push(num_edge * 2);
        tri_idx.push(num_tri);
        prop_idx.push(num_prop_vert);
        num_vert += n.num_vert();
        num_edge += n.halfedge.len() / 2;
        num_tri += n.num_tri();
        num_prop_out = num_prop_out.max(n.num_prop);
        num_prop_vert += if n.num_prop == 0 { 1 } else { n.properties.len() / n.num_prop as usize };
    }
    if num_vert > crate::MAX_RESULT_VERTS || num_tri > 2 * crate::MAX_RESULT_VERTS {
        return Err(Fault::TooLarge);
    }
    let mut c = Impl { epsilon, tolerance, bbox, ..Impl::default() };
    c.vert_pos = vec![V3::default(); num_vert];
    c.vert_normal = vec![V3::default(); num_vert];
    c.halfedge.resize_nofill(2 * num_edge);
    c.face_normal = vec![V3::default(); num_tri];
    c.halfedge_tangent = vec![V4::default(); 2 * num_edge];
    c.mesh_relation.tri_ref = vec![Default::default(); num_tri];
    let npo = num_prop_out as usize;
    if num_prop_out > 0 {
        c.num_prop = num_prop_out;
        c.properties = vec![0.0; npo * num_prop_vert];
    }
    let snapshot = mesh_id_counter();
    for (i, (n, t, _)) in nodes.iter().enumerate() {
        let e0 = edge_idx.atu(i)?;
        for (k, tg) in n.halfedge_tangent.iter().enumerate() {
            c.halfedge_tangent.setu(e0 + k, *tg)?;
        }
        let next_vert = ix(vert_idx.atu(i)?)?;
        let next_edge = ix(e0)?;
        let next_prop = ix(prop_idx.atu(i)?)?;
        let has_prop = n.num_prop > 0;
        for edge in 0..n.halfedge.len() {
            let e = edge as i32;
            let ne = next_edge + e;
            c.halfedge.set_start(ne, n.halfedge.start(e)?.wrapping_add(next_vert))?;
            c.halfedge.set_pair(ne, n.halfedge.pair(e)?.wrapping_add(next_edge))?;
            let pv = if has_prop { n.halfedge.prop(e)?.wrapping_add(next_prop) } else { next_prop };
            c.halfedge.set_prop(ne, pv)?;
        }
        if has_prop {
            let np = n.num_prop as usize;
            let base = npo * prop_idx.atu(i)?;
            for p in 0..np {
                let mut k = p;
                let mut o = base + p;
                while k < n.properties.len() {
                    c.properties.setu(o, n.properties.atu(k)?)?;
                    k += np;
                    o += npo;
                }
            }
            if np >= 3 && !t.is_identity() {
                let nt = normal_transform(t);
                crate::transform::eager_transform_prop_normals(
                    &n.halfedge,
                    &n.mesh_relation,
                    &nt,
                    &mut c.properties,
                    n.properties.len() / np,
                    npo,
                    prop_idx.atu(i)?,
                )?;
            }
        }
        let v0 = vert_idx.atu(i)?;
        let t0 = tri_idx.atu(i)?;
        if t.is_identity() {
            for (k, p) in n.vert_pos.iter().enumerate() {
                c.vert_pos.setu(v0 + k, *p)?;
            }
            for (k, p) in n.vert_normal.iter().enumerate() {
                c.vert_normal.setu(v0 + k, *p)?;
            }
            for (k, f) in n.face_normal.iter().enumerate() {
                c.face_normal.setu(t0 + k, *f)?;
            }
        } else {
            let nt = normal_transform(t);
            for (k, p) in n.vert_pos.iter().enumerate() {
                c.vert_pos.setu(v0 + k, t.apply(*p))?;
            }
            for (k, p) in n.vert_normal.iter().enumerate() {
                c.vert_normal.setu(v0 + k, crate::transform::transform_normal(&nt, *p))?;
            }
            for (k, f) in n.face_normal.iter().enumerate() {
                c.face_normal.setu(t0 + k, crate::transform::transform_normal(&nt, *f))?;
            }
            let lin = t.linear();
            let invert = lin.determinant() < 0.0;
            crate::transform::transform_tangents(&mut c.halfedge_tangent, e0, &lin, invert, &n.halfedge_tangent, &n.halfedge)?;
            if invert {
                for tri in t0..t0 + n.num_tri() {
                    crate::transform::flip_tri(&mut c.halfedge, ix(tri)?)?;
                }
            }
        }
        let offset = (i as u32).wrapping_mul(snapshot) as i32;
        for (k, r) in n.mesh_relation.tri_ref.iter().enumerate() {
            let mut r = *r;
            r.mesh_id = r.mesh_id.wrapping_add(offset);
            c.mesh_relation.tri_ref.setu(t0 + k, r)?;
        }
    }
    for (i, (n, t, _)) in nodes.iter().enumerate() {
        let offset = (i as u32).wrapping_mul(snapshot) as i32;
        for (&k, rel) in &n.mesh_relation.mesh_id_transform {
            let mut rel = *rel;
            if !t.is_identity() {
                rel.transform = t.mul_mat4(&Mat34::from_array(&rel.transform)).to_array();
            }
            c.mesh_relation.mesh_id_transform.insert(k.wrapping_add(offset), rel);
        }
    }
    c.remove_degenerates(0)?;
    c.sort_geometry()?;
    c.increment_mesh_ids()?;
    Ok(Leaf::of(c))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Pos,
    Neg,
}

struct Frame {
    finalize: bool,
    parent_op: OpType,
    transform: Mat34,
    positive_dest: Option<(usize, Side)>,
    negative_dest: Option<(usize, Side)>,
    op_node: Option<Arc<OpNode>>,
    positive: Vec<Arc<Leaf>>,
    negative: Vec<Arc<Leaf>>,
}

fn push_dest(frames: &mut [Frame], dest: Option<(usize, Side)>, leaf: Arc<Leaf>) -> R<()> {
    let (fi, side) = dest.ok_or(Fault::Index)?;
    let f = frames.atu_mut(fi)?;
    match side {
        Side::Pos => f.positive.push(leaf),
        Side::Neg => f.negative.push(leaf),
    }
    Ok(())
}

/// `CsgOpNode::ToLeafNode`: depth-first over an explicit stack (frames in an arena, so destinations stay valid; a
/// popped frame lets go of its node, as the C++ frame is destroyed when popped).
fn to_leaf(root: &Arc<OpNode>) -> R<Arc<Leaf>> {
    if let Some(c) = lock(&root.cache).as_ref() {
        return Ok(Arc::clone(c));
    }
    let mut frames: Vec<Frame> = vec![Frame {
        finalize: false,
        parent_op: root.op,
        transform: IDENTITY,
        positive_dest: None,
        negative_dest: None,
        op_node: Some(Arc::clone(root)),
        positive: Vec::new(),
        negative: Vec::new(),
    }];
    let mut stack: Vec<usize> = vec![0];
    let mut guard = crate::util::Guard::new(1 << 24);
    while let Some(&fi) = stack.last() {
        guard.tick()?;
        let frame = frames.get(fi).ok_or(Fault::Index)?;
        let op_node = frame.op_node.clone().ok_or(Fault::Index)?;
        if frame.finalize {
            let cached = lock(&op_node.cache).clone();
            let result = match cached {
                Some(c) => c,
                None => {
                    let f = frames.atu_mut(fi)?;
                    let r = match op_node.op {
                        OpType::Add => batch_union(&mut f.positive)?,
                        OpType::Intersect => batch_boolean(OpType::Intersect, core::mem::take(&mut f.positive))?,
                        OpType::Subtract => {
                            if f.positive.is_empty() {
                                Leaf::of(Impl::default())
                            } else {
                                let positive = batch_union(&mut f.positive)?;
                                if f.negative.is_empty() {
                                    positive
                                } else {
                                    let negative = batch_union(&mut f.negative)?;
                                    simple_boolean(&positive.get_impl(), &negative.get_impl(), OpType::Subtract)?
                                }
                            }
                        }
                    };
                    *lock(&op_node.children) = vec![NodeRef::Leaf(Arc::clone(&r))];
                    let c = r.transform(&op_node.transform);
                    *lock(&op_node.cache) = Some(Arc::clone(&c));
                    c
                }
            };
            let f = frames.get(fi).ok_or(Fault::Index)?;
            let (dest, t) = (f.positive_dest, f.transform);
            if dest.is_some() {
                push_dest(&mut frames, dest, result.transform(&t))?;
            }
            drop(op_node);
            frames.atu_mut(fi)?.op_node = None;
            stack.pop();
        } else {
            let op = op_node.op;
            // references: the parent's child list, this frame, and the local clone above (the C++
            // `use_count() <= 2`); and no transformed copy shares the children (`impl_.UseCount() == 1`)
            let unique = Arc::strong_count(&op_node) <= 3 && Arc::strong_count(&op_node.children) == 1;
            let children: Vec<NodeRef> = lock(&op_node.children).clone();
            let can_collapse = frame.positive_dest.is_some() && ((op == frame.parent_op && unique) || children.len() == 1);
            let transform = if can_collapse { frame.transform.mul_mat4(&op_node.transform) } else { IDENTITY };
            let (pos_dest, neg_dest) =
                if can_collapse { (frame.positive_dest, frame.negative_dest) } else { (Some((fi, Side::Pos)), Some((fi, Side::Neg))) };
            if can_collapse {
                frames.atu_mut(fi)?.op_node = None;
                stack.pop();
            } else {
                frames.atu_mut(fi)?.finalize = true;
            }
            drop(op_node);
            for (i, child) in children.into_iter().enumerate() {
                let negative = op == OpType::Subtract && i != 0;
                let dest1 = if negative { neg_dest } else { pos_dest };
                let dest2 = if op == OpType::Subtract && i == 0 { neg_dest } else { None };
                match child {
                    NodeRef::Leaf(l) => push_dest(&mut frames, dest1, l.transform(&transform))?,
                    NodeRef::Op(o) => {
                        frames.push(Frame {
                            finalize: false,
                            parent_op: if negative { OpType::Add } else { op },
                            transform,
                            positive_dest: dest1,
                            negative_dest: dest2,
                            op_node: Some(o),
                            positive: Vec::new(),
                            negative: Vec::new(),
                        });
                        stack.push(frames.len() - 1);
                    }
                }
            }
        }
    }
    lock(&root.cache).clone().ok_or(Fault::Index)
}
