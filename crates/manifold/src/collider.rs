// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! collider.h: the radix-tree BVH over Morton-sorted boxes (Karras 2012), traversed depth first in the C++ order
//! (child 1 before child 2, the second saved on an explicit stack). Queries run in index order, as the sequential
//! (MANIFOLD_PAR=OFF) build does.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use crate::linalg::{Box3, V3, la_max, la_min};
use crate::util::{Fault, Get, GetMut, R, ix};

const INITIAL_LENGTH: i32 = 128;
const LENGTH_MULTIPLE: i32 = 4;
const ROOT: i32 = 1;

#[inline]
fn is_leaf(node: i32) -> bool {
    node % 2 == 0
}
#[inline]
fn is_internal(node: i32) -> bool {
    node % 2 == 1
}
#[inline]
fn node2internal(node: i32) -> i32 {
    (node - 1) / 2
}
#[inline]
fn internal2node(internal: i32) -> i32 {
    internal * 2 + 1
}
#[inline]
fn node2leaf(node: i32) -> i32 {
    node / 2
}
#[inline]
fn leaf2node(leaf: i32) -> i32 {
    leaf * 2
}

/// `SpreadBits3`: 10 bits spread to every third bit.
#[inline]
fn spread_bits3(v: u32) -> u32 {
    let v = 0xFF00_00FF & v.wrapping_mul(0x0001_0001);
    let v = 0x0F00_F00F & v.wrapping_mul(0x0000_0101);
    let v = 0xC30C_30C3 & v.wrapping_mul(0x0000_0011);
    0x4924_9249 & v.wrapping_mul(0x0000_0005)
}

/// `Collider::MortonCode`.
pub(crate) fn morton_code(position: V3, bbox: &Box3) -> u32 {
    let xyz = (position - bbox.min).div_v(bbox.max - bbox.min);
    let s = |c: f64| -> u32 {
        // la::min(1023, la::max(0, 1024·c)): NaN → 0
        let m = la_min(1023.0, la_max(0.0, 1024.0 * c));
        // in [0, 1023]: exact truncation, as static_cast<uint32_t>
        m as u32
    };
    let x = spread_bits3(s(xyz.x));
    let y = spread_bits3(s(xyz.y));
    let z = spread_bits3(s(xyz.z));
    x.wrapping_mul(4).wrapping_add(y.wrapping_mul(2)).wrapping_add(z)
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Collider {
    node_bbox: Vec<Box3>,
    node_parent: Vec<i32>,
    internal_children: Vec<(i32, i32)>,
}

struct RadixTree<'a> {
    morton: &'a [u32],
}

impl RadixTree<'_> {
    #[inline]
    fn prefix_length_bits(a: u32, b: u32) -> i32 {
        // __builtin_clz; wasm's i32.clz(0) is 32, as is leading_zeros(0)
        (a ^ b).leading_zeros() as i32
    }

    fn prefix_length(&self, i: i32, j: i32) -> R<i32> {
        if j < 0 || j >= ix(self.morton.len())? {
            return Ok(-1);
        }
        let mi = self.morton.at(i)?;
        let mj = self.morton.at(j)?;
        Ok(if mi == mj { 32 + Self::prefix_length_bits(i as u32, j as u32) } else { Self::prefix_length_bits(mi, mj) })
    }

    fn range_end(&self, i: i32) -> R<i32> {
        let mut dir = self.prefix_length(i, i + 1)? - self.prefix_length(i, i - 1)?;
        dir = i32::from(dir > 0) - i32::from(dir < 0);
        let common = self.prefix_length(i, i - dir)?;
        let mut max_length = INITIAL_LENGTH;
        while self.prefix_length(i, i.saturating_add(dir.saturating_mul(max_length)))? > common {
            max_length = max_length.checked_mul(LENGTH_MULTIPLE).ok_or(Fault::TooLarge)?;
        }
        let mut length = 0;
        let mut step = max_length / 2;
        while step > 0 {
            if self.prefix_length(i, i + dir * (length + step))? > common {
                length += step;
            }
            step /= 2;
        }
        Ok(i + dir * length)
    }

    fn find_split(&self, first: i32, last: i32) -> R<i32> {
        let common = self.prefix_length(first, last)?;
        let mut split = first;
        let mut step = last - first;
        loop {
            step = (step + 1) >> 1;
            let new_split = split + step;
            if new_split < last && self.prefix_length(first, new_split)? > common {
                split = new_split;
            }
            if step <= 1 {
                break;
            }
        }
        Ok(split)
    }
}

impl Collider {
    /// `Collider(leafBB, leafMorton)`; the leaves must be sorted by Morton code.
    pub(crate) fn new(leaf_bb: &[Box3], leaf_morton: &[u32]) -> R<Collider> {
        let mut c = Collider::default();
        if leaf_bb.is_empty() {
            return Ok(c);
        }
        if leaf_bb.len() != leaf_morton.len() {
            return Err(Fault::Index);
        }
        let n = leaf_bb.len();
        let num_nodes = 2 * n - 1;
        c.node_bbox = vec![Box3::default(); num_nodes];
        c.node_parent = vec![-1; num_nodes];
        c.internal_children = vec![(-1, -1); n - 1];
        let tree = RadixTree { morton: leaf_morton };
        for internal in 0..ix(n - 1)? {
            let mut first = internal;
            let mut last = tree.range_end(first)?;
            if first > last {
                core::mem::swap(&mut first, &mut last);
            }
            let mut split = tree.find_split(first, last)?;
            let child1 = if split == first { leaf2node(split) } else { internal2node(split) };
            split += 1;
            let child2 = if split == last { leaf2node(split) } else { internal2node(split) };
            c.internal_children.set(internal, (child1, child2))?;
            let node = internal2node(internal);
            c.node_parent.set(child1, node)?;
            c.node_parent.set(child2, node)?;
        }
        c.update_boxes(leaf_bb)?;
        Ok(c)
    }

    /// `GetBoundingBox` (the root box).
    pub(crate) fn bounding_box(&self) -> Box3 {
        if self.node_bbox.is_empty() {
            return Box3::default();
        }
        // a single leaf has no internal node: the C++ reads past the end here; the leaf box is the natural answer
        self.node_bbox.at(internal2node(0)).or_else(|_| self.node_bbox.at(0)).unwrap_or_default()
    }

    fn num_internal(&self) -> usize {
        self.internal_children.len()
    }

    fn num_leaves(&self) -> usize {
        if self.internal_children.is_empty() { 0 } else { self.num_internal() + 1 }
    }

    /// `UpdateBoxes`: leaf boxes in, internal boxes built bottom-up (the second child to arrive builds the union).
    pub(crate) fn update_boxes(&mut self, leaf_bb: &[Box3]) -> R<()> {
        for (i, b) in leaf_bb.iter().enumerate() {
            self.node_bbox.setu(2 * i, *b)?;
        }
        let mut counter = vec![0i32; self.num_internal()];
        let leaves = self.num_leaves();
        for leaf in 0..leaves {
            let mut node = leaf2node(ix(leaf)?);
            loop {
                node = self.node_parent.at(node)?;
                let internal = node2internal(node);
                let c = counter.at_mut(internal)?;
                let old = *c;
                *c += 1;
                if old == 0 {
                    break;
                }
                let (c1, c2) = self.internal_children.at(internal)?;
                let b = self.node_bbox.at(c1)?.union(&self.node_bbox.at(c2)?);
                self.node_bbox.set(node, b)?;
                if node == ROOT {
                    break;
                }
            }
        }
        Ok(())
    }

    /// `Collisions` with box queries: `record(query, leaf)` for each overlap, queries in order. `self_collision`
    /// skips `leaf == query`. Empty query boxes (min.x = +inf) are skipped.
    pub(crate) fn collisions_box<F, Q>(&self, self_collision: bool, n: usize, query: Q, mut record: F) -> R<()>
    where
        Q: Fn(i32) -> R<Box3>,
        F: FnMut(i32, i32) -> R<()>,
    {
        if self.internal_children.is_empty() {
            return Ok(());
        }
        let mut stack: Vec<i32> = Vec::with_capacity(64);
        for qi in 0..ix(n)? {
            let q = query(qi)?;
            if q.min.x == f64::INFINITY {
                continue;
            }
            stack.clear();
            let mut node = ROOT;
            loop {
                let internal = node2internal(node);
                let (c1, c2) = self.internal_children.at(internal)?;
                let t1 = self.record_box(&q, c1, qi, self_collision, &mut record)?;
                let t2 = self.record_box(&q, c2, qi, self_collision, &mut record)?;
                if !t1 && !t2 {
                    match stack.pop() {
                        Some(s) => node = s,
                        None => break,
                    }
                } else {
                    node = if t1 { c1 } else { c2 };
                    if t1 && t2 {
                        stack.push(c2);
                    }
                }
            }
        }
        Ok(())
    }

    #[inline]
    fn record_box<F: FnMut(i32, i32) -> R<()>>(&self, q: &Box3, node: i32, qi: i32, self_collision: bool, record: &mut F) -> R<bool> {
        let b = self.node_bbox.at(node)?;
        let overlaps = b.does_overlap(q);
        if overlaps && is_leaf(node) {
            let leaf = node2leaf(node);
            if !self_collision || leaf != qi {
                record(qi, leaf)?;
            }
        }
        Ok(overlaps && is_internal(node))
    }

    /// `Collisions` with point queries (overlap projected along z).
    pub(crate) fn collisions_pt<F, Q>(&self, n: usize, query: Q, mut record: F) -> R<()>
    where
        Q: Fn(i32) -> R<V3>,
        F: FnMut(i32, i32) -> R<()>,
    {
        if self.internal_children.is_empty() {
            return Ok(());
        }
        let mut stack: Vec<i32> = Vec::with_capacity(64);
        for qi in 0..ix(n)? {
            let p = query(qi)?;
            stack.clear();
            let mut node = ROOT;
            loop {
                let internal = node2internal(node);
                let (c1, c2) = self.internal_children.at(internal)?;
                let t1 = self.record_pt(p, c1, qi, &mut record)?;
                let t2 = self.record_pt(p, c2, qi, &mut record)?;
                if !t1 && !t2 {
                    match stack.pop() {
                        Some(s) => node = s,
                        None => break,
                    }
                } else {
                    node = if t1 { c1 } else { c2 };
                    if t1 && t2 {
                        stack.push(c2);
                    }
                }
            }
        }
        Ok(())
    }

    #[inline]
    fn record_pt<F: FnMut(i32, i32) -> R<()>>(&self, p: V3, node: i32, qi: i32, record: &mut F) -> R<bool> {
        let b = self.node_bbox.at(node)?;
        let overlaps = b.does_overlap_pt(p);
        if overlaps && is_leaf(node) {
            record(qi, node2leaf(node))?;
        }
        Ok(overlaps && is_internal(node))
    }
}
