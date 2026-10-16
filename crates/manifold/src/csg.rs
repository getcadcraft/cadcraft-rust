// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! manifold.cpp and csg_tree.cpp: the public `Manifold`, its lazy CSG tree, and the evaluation of a tree (nested
//! unions / intersections / differences collapsed into batches, disjoint pieces composed, the rest reduced pairwise
//! from the largest meshes).
//!
//! Nodes are reference-counted (`Arc`) like the C++ `shared_ptr`s, and the collapse rule reads the reference count the
//! same way: a nested operation folds into its parent only when the parent's child list and the evaluation stack are
//! its only holders. Dropping a [`Manifold`] here is `.delete()` in JavaScript. Transforms are not ported (every node
//! carries the identity), so the C++ transform paths reduce to plain copies.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::sync::{Arc, Mutex, PoisonError};

use crate::boolean3::Boolean3;
use crate::imp::{Impl, K_PRECISION, mesh_id_counter};
use crate::linalg::{Box3, V3, V4};
use crate::util::{Fault, Get, GetMut, R, ix, std_max};
use crate::{Mesh, OpType, Status};

/// A CSG node: a mesh, or a deferred operation on child nodes.
pub(crate) enum Node {
    Leaf(Arc<Impl>),
    Op(OpNode),
}

pub(crate) struct OpNode {
    children: Mutex<Vec<Arc<Node>>>,
    op: OpType,
    cache: Mutex<Option<Arc<Impl>>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Node {
    fn leaf(imp: Impl) -> Arc<Node> {
        Arc::new(Node::Leaf(Arc::new(imp)))
    }
    fn op(children: Vec<Arc<Node>>, op: OpType) -> Arc<Node> {
        Arc::new(Node::Op(OpNode { children: Mutex::new(children), op, cache: Mutex::new(None) }))
    }
}

/// A solid: an oriented 2-manifold triangle mesh, or a deferred boolean of such solids (evaluated when its mesh,
/// status or emptiness is asked for). Cloning shares the node, as copying a C++ `Manifold` does.
pub struct Manifold {
    node: Mutex<Arc<Node>>,
}

impl Clone for Manifold {
    fn clone(&self) -> Self {
        Manifold { node: Mutex::new(Arc::clone(&lock(&self.node))) }
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
        Manifold { node: Mutex::new(Node::leaf(Impl::default())) }
    }
}

impl Manifold {
    fn from_node(node: Arc<Node>) -> Manifold {
        Manifold { node: Mutex::new(node) }
    }

    fn load(&self) -> Arc<Node> {
        Arc::clone(&lock(&self.node))
    }

    /// `Manifold(const MeshGL&)` plus manifold-3d's status check: the solid, or why the mesh is not one.
    pub fn from_mesh(mesh: &Mesh) -> Result<Manifold, Status> {
        let imp = Impl::from_mesh(mesh);
        if imp.status != Status::NoError {
            return Err(imp.status);
        }
        Ok(Manifold::from_node(Node::leaf(imp)))
    }

    /// `Manifold(const MeshGL&)` without the check (the status stays on the Manifold).
    pub fn from_mesh_unchecked(mesh: &Mesh) -> Manifold {
        Manifold::from_node(Node::leaf(Impl::from_mesh(mesh)))
    }

    /// The evaluated mesh (`GetCsgLeafNode().GetImpl()`); a deferred tree is evaluated once and replaced by its result.
    fn leaf_impl(&self) -> Arc<Impl> {
        let mut g = lock(&self.node);
        let imp = match &**g {
            Node::Leaf(i) => return Arc::clone(i),
            Node::Op(_) => to_leaf(&g).unwrap_or_else(|f| Arc::new(Impl::with_status(f.status()))),
        };
        *g = Arc::new(Node::Leaf(Arc::clone(&imp)));
        imp
    }

    /// `Boolean(second, op)`: a deferred operation. A union or intersection with a deferred tree on the right joins
    /// that tree's side, as the C++ does.
    pub fn boolean(&self, other: &Manifold, op: OpType) -> Manifold {
        let a = self.load();
        let b = other.load();
        let node = match (&*a, &*b) {
            (Node::Leaf(_), Node::Op(_)) if op == OpType::Add || op == OpType::Intersect => Node::op(vec![b, a], op),
            _ => Node::op(vec![a, b], op),
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
            _ => Manifold::from_node(Node::op(list.iter().map(Manifold::load).collect(), op)),
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

    /// Number of triangles (evaluates).
    pub fn num_tri(&self) -> usize {
        self.leaf_impl().num_tri()
    }

    /// `GetMeshGL()` (manifold-3d `getMesh()`; evaluates).
    pub fn get_mesh(&self) -> Mesh {
        let imp = self.leaf_impl();
        crate::export::get_mesh(&imp).unwrap_or_else(|_| Mesh { num_prop: 3, ..Mesh::default() })
    }
}

/// `SimpleBoolean`.
fn simple_boolean(a: &Impl, b: &Impl, op: OpType) -> R<Arc<Impl>> {
    let b3 = Boolean3::new(a, b, op)?;
    Ok(Arc::new(b3.result(op)?))
}

/// `BatchBoolean`: reduce pairwise, always the two largest (vertex count, then insertion order) first.
fn batch_boolean(op: OpType, mut results: Vec<Arc<Impl>>) -> R<Arc<Impl>> {
    match results.len() {
        0 => return Ok(Arc::new(Impl::default())),
        1 => return results.pop().ok_or(Fault::Index),
        2 => {
            let b = results.pop().ok_or(Fault::Index)?;
            let a = results.pop().ok_or(Fault::Index)?;
            return simple_boolean(&a, &b, op);
        }
        _ => {}
    }
    // the heap only ever yields its maximum, and the keys (numVert, serial) are unique: a plain max search
    let mut heap: Vec<(Arc<Impl>, u64)> = results.into_iter().zip(0u64..).collect();
    let mut next_serial = heap.len() as u64;
    let pop_max = |heap: &mut Vec<(Arc<Impl>, u64)>| -> R<Arc<Impl>> {
        let mut best = 0usize;
        for (i, (imp, s)) in heap.iter().enumerate() {
            let (bi, bs) = heap.get(best).map(|(x, y)| (x.num_vert(), *y)).ok_or(Fault::Index)?;
            if (imp.num_vert(), *s) > (bi, bs) {
                best = i;
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
            tmp.push((simple_boolean(&a, &b, op)?, next_serial));
            next_serial += 1;
            i += 1;
        }
        heap.extend(tmp);
    }
    heap.pop().map(|x| x.0).ok_or(Fault::Index)
}

/// `BatchUnion`: pairwise-disjoint groups (by bounding box) are composed, the groups unioned, from the back.
fn batch_union(children: &mut Vec<Arc<Impl>>) -> R<Arc<Impl>> {
    const MAX_UNION_SIZE: usize = 1000;
    let mut guard = crate::util::Guard::new(children.len() + 1);
    while children.len() > 1 {
        guard.tick()?;
        let start = if children.len() > MAX_UNION_SIZE { children.len() - MAX_UNION_SIZE } else { 0 };
        let boxes: Vec<Box3> = children.iter().skip(start).map(|c| c.bbox).collect();
        let mut sets: Vec<Vec<usize>> = Vec::new();
        for i in 0..boxes.len() {
            let bi = boxes.atu(i)?;
            let found = sets.iter().position(|set| set.iter().all(|&j| boxes.get(j).is_none_or(|bj| !bi.does_overlap(bj))));
            match found {
                Some(k) => sets.get_mut(k).ok_or(Fault::Index)?.push(i),
                None => sets.push(vec![i]),
            }
        }
        let mut impls = Vec::with_capacity(sets.len());
        for set in &sets {
            if let [one] = set.as_slice() {
                impls.push(Arc::clone(children.get(start + one).ok_or(Fault::Index)?));
            } else {
                let nodes: Vec<Arc<Impl>> = set.iter().map(|&j| children.get(start + j).cloned().ok_or(Fault::Index)).collect::<R<_>>()?;
                impls.push(compose(&nodes)?);
            }
        }
        children.truncate(start);
        children.push(batch_boolean(OpType::Add, impls)?);
        let last = children.len() - 1;
        children.swap(0, last);
    }
    children.first().cloned().ok_or(Fault::Index)
}

/// `CsgLeafNode::Compose`: a union of pairwise-disjoint meshes by concatenation.
fn compose(nodes: &[Arc<Impl>]) -> R<Arc<Impl>> {
    let mut epsilon = -1.0f64;
    let mut tolerance = -1.0f64;
    let mut bbox = Box3::default();
    let (mut num_vert, mut num_edge, mut num_tri, mut num_prop_vert) = (0usize, 0usize, 0usize, 0usize);
    let mut vert_idx = Vec::new();
    let mut edge_idx = Vec::new();
    let mut tri_idx = Vec::new();
    let mut prop_idx = Vec::new();
    let mut num_prop_out = 0i32;
    for n in nodes {
        if n.status != Status::NoError {
            return Ok(Arc::new(Impl::with_status(n.status)));
        }
        let old_scale = n.bbox.scale();
        let new_scale = n.bbox.transform_identity().scale();
        let mut ne = n.epsilon;
        ne *= std_max(1.0, new_scale / old_scale);
        ne = std_max(ne, K_PRECISION * new_scale);
        if !ne.is_finite() {
            ne = -1.0;
        }
        epsilon = std_max(epsilon, ne);
        tolerance = std_max(tolerance, n.tolerance);
        bbox = bbox.union(&n.bbox);
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
    for (i, n) in nodes.iter().enumerate() {
        let e0 = edge_idx.atu(i)?;
        for (k, t) in n.halfedge_tangent.iter().enumerate() {
            c.halfedge_tangent.setu(e0 + k, *t)?;
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
        }
        let v0 = vert_idx.atu(i)?;
        for (k, p) in n.vert_pos.iter().enumerate() {
            c.vert_pos.setu(v0 + k, *p)?;
        }
        for (k, p) in n.vert_normal.iter().enumerate() {
            c.vert_normal.setu(v0 + k, *p)?;
        }
        let t0 = tri_idx.atu(i)?;
        for (k, f) in n.face_normal.iter().enumerate() {
            c.face_normal.setu(t0 + k, *f)?;
        }
        let offset = (i as u32).wrapping_mul(snapshot) as i32;
        for (k, r) in n.mesh_relation.tri_ref.iter().enumerate() {
            let mut r = *r;
            r.mesh_id = r.mesh_id.wrapping_add(offset);
            c.mesh_relation.tri_ref.setu(t0 + k, r)?;
        }
    }
    for (i, n) in nodes.iter().enumerate() {
        let offset = (i as u32).wrapping_mul(snapshot) as i32;
        for (&k, rel) in &n.mesh_relation.mesh_id_transform {
            c.mesh_relation.mesh_id_transform.insert(k.wrapping_add(offset), *rel);
        }
    }
    c.remove_degenerates(0)?;
    c.sort_geometry()?;
    c.increment_mesh_ids()?;
    Ok(Arc::new(c))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Pos,
    Neg,
}

struct Frame {
    finalize: bool,
    parent_op: OpType,
    positive_dest: Option<(usize, Side)>,
    negative_dest: Option<(usize, Side)>,
    op_node: Arc<Node>,
    positive: Vec<Arc<Impl>>,
    negative: Vec<Arc<Impl>>,
}

fn push_dest(frames: &mut [Frame], dest: Option<(usize, Side)>, imp: Arc<Impl>) -> R<()> {
    let (fi, side) = dest.ok_or(Fault::Index)?;
    let f = frames.atu_mut(fi)?;
    match side {
        Side::Pos => f.positive.push(imp),
        Side::Neg => f.negative.push(imp),
    }
    Ok(())
}

fn release(frames: &mut [Frame], fi: usize, dummy: &Arc<Node>) -> R<()> {
    frames.atu_mut(fi)?.op_node = Arc::clone(dummy);
    Ok(())
}

/// `CsgOpNode::ToLeafNode`: depth-first over an explicit stack (frames in an arena, so destinations stay valid).
fn to_leaf(root: &Arc<Node>) -> R<Arc<Impl>> {
    let Node::Op(root_op) = &**root else {
        return match &**root {
            Node::Leaf(i) => Ok(Arc::clone(i)),
            Node::Op(_) => Err(Fault::Index),
        };
    };
    if let Some(c) = lock(&root_op.cache).as_ref() {
        return Ok(Arc::clone(c));
    }
    let mut frames: Vec<Frame> = vec![Frame {
        finalize: false,
        parent_op: root_op.op,
        positive_dest: None,
        negative_dest: None,
        op_node: Arc::clone(root),
        positive: Vec::new(),
        negative: Vec::new(),
    }];
    let mut stack: Vec<usize> = vec![0];
    // a popped frame lets go of its node, as the C++ frame is destroyed when popped
    let dummy = Arc::new(Node::Leaf(Arc::new(Impl::default())));
    let mut guard = crate::util::Guard::new(1 << 24);
    while let Some(&fi) = stack.last() {
        guard.tick()?;
        let frame = frames.get(fi).ok_or(Fault::Index)?;
        let Node::Op(op_node) = &*frame.op_node else { return Err(Fault::Index) };
        if frame.finalize {
            let node = Arc::clone(&frame.op_node);
            let Node::Op(op_node) = &*node else { return Err(Fault::Index) };
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
                                Arc::new(Impl::default())
                            } else {
                                let positive = batch_union(&mut f.positive)?;
                                if f.negative.is_empty() {
                                    positive
                                } else {
                                    let negative = batch_union(&mut f.negative)?;
                                    simple_boolean(&positive, &negative, OpType::Subtract)?
                                }
                            }
                        }
                    };
                    *lock(&op_node.children) = vec![Arc::new(Node::Leaf(Arc::clone(&r)))];
                    *lock(&op_node.cache) = Some(Arc::clone(&r));
                    r
                }
            };
            let dest = frames.get(fi).ok_or(Fault::Index)?.positive_dest;
            if dest.is_some() {
                push_dest(&mut frames, dest, result)?;
            }
            release(&mut frames, fi, &dummy)?;
            stack.pop();
        } else {
            let op = op_node.op;
            // references: the parent's child list and this frame (the C++ `use_count() <= 2`)
            let unique = Arc::strong_count(&frame.op_node) <= 2;
            let children: Vec<Arc<Node>> = lock(&op_node.children).clone();
            let can_collapse = frame.positive_dest.is_some() && ((op == frame.parent_op && unique) || children.len() == 1);
            let (pos_dest, neg_dest) =
                if can_collapse { (frame.positive_dest, frame.negative_dest) } else { (Some((fi, Side::Pos)), Some((fi, Side::Neg))) };
            if can_collapse {
                release(&mut frames, fi, &dummy)?;
                stack.pop();
            } else {
                frames.atu_mut(fi)?.finalize = true;
            }
            for (i, child) in children.into_iter().enumerate() {
                let negative = op == OpType::Subtract && i != 0;
                let dest1 = if negative { neg_dest } else { pos_dest };
                let dest2 = if op == OpType::Subtract && i == 0 { neg_dest } else { None };
                match &*child {
                    Node::Leaf(imp) => push_dest(&mut frames, dest1, Arc::clone(imp))?,
                    Node::Op(_) => {
                        frames.push(Frame {
                            finalize: false,
                            parent_op: if negative { OpType::Add } else { op },
                            positive_dest: dest1,
                            negative_dest: dest2,
                            op_node: child,
                            positive: Vec::new(),
                            negative: Vec::new(),
                        });
                        stack.push(frames.len() - 1);
                    }
                }
            }
        }
    }
    lock(&root_op.cache).clone().ok_or(Fault::Index)
}
