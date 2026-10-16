// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! shared.h: halfedge storage (`Halfedges`, structure of arrays), the value `Halfedge`, `TriRef`, `NextHalfedge`.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use crate::util::{Get, GetMut, R};

/// shared.h `NextHalfedge` (C++ `%` semantics: -1 → 0).
#[inline]
pub(crate) fn next_halfedge(current: i32) -> i32 {
    current.wrapping_add(if current % 3 == 2 { -2 } else { 1 })
}

#[inline]
pub(crate) fn next3(i: usize) -> usize {
    match i {
        0 => 1,
        1 => 2,
        _ => 0,
    }
}

#[inline]
pub(crate) fn prev3(i: usize) -> usize {
    match i {
        0 => 2,
        1 => 0,
        _ => 1,
    }
}

/// The value form of a halfedge (shared.h `Halfedge`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Halfedge {
    pub start_vert: i32,
    pub end_vert: i32,
    pub paired_halfedge: i32,
    pub prop_vert: i32,
}

/// shared.h `TriRef`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TriRef {
    pub mesh_id: i32,
    pub original_id: i32,
    pub face_id: i32,
    pub coplanar_id: i32,
}

impl TriRef {
    #[inline]
    pub(crate) fn same_face(&self, o: &TriRef) -> bool {
        self.mesh_id == o.mesh_id && self.coplanar_id == o.coplanar_id && self.face_id == o.face_id
    }
}

/// shared.h `Halfedges`: the persistent storage; the end vertex is the next halfedge's start.
#[derive(Clone, Debug, Default)]
pub(crate) struct Halfedges {
    pub start: Vec<i32>,
    pub paired: Vec<i32>,
    pub prop: Vec<i32>,
}

impl Halfedges {
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.start.len()
    }
    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.start.is_empty()
    }
    #[inline]
    pub(crate) fn start(&self, i: i32) -> R<i32> {
        self.start.at(i)
    }
    #[inline]
    pub(crate) fn end(&self, i: i32) -> R<i32> {
        self.start.at(next_halfedge(i))
    }
    #[inline]
    pub(crate) fn pair(&self, i: i32) -> R<i32> {
        self.paired.at(i)
    }
    #[inline]
    pub(crate) fn prop(&self, i: i32) -> R<i32> {
        self.prop.at(i)
    }
    #[inline]
    pub(crate) fn set_start(&mut self, i: i32, v: i32) -> R<()> {
        self.start.set(i, v)
    }
    #[inline]
    pub(crate) fn set_end(&mut self, i: i32, v: i32) -> R<()> {
        self.start.set(next_halfedge(i), v)
    }
    #[inline]
    pub(crate) fn set_pair(&mut self, i: i32, v: i32) -> R<()> {
        self.paired.set(i, v)
    }
    #[inline]
    pub(crate) fn set_prop(&mut self, i: i32, v: i32) -> R<()> {
        self.prop.set(i, v)
    }
    #[inline]
    pub(crate) fn get(&self, i: i32) -> R<Halfedge> {
        Ok(Halfedge { start_vert: self.start(i)?, end_vert: self.end(i)?, paired_halfedge: self.pair(i)?, prop_vert: self.prop(i)? })
    }
    #[inline]
    pub(crate) fn set(&mut self, i: i32, start: i32, pair: i32, prop: i32) -> R<()> {
        self.set_start(i, start)?;
        self.set_pair(i, pair)?;
        self.set_prop(i, prop)
    }
    #[inline]
    pub(crate) fn push(&mut self, start: i32, pair: i32, prop: i32) {
        self.start.push(start);
        self.paired.push(pair);
        self.prop.push(prop);
    }
    /// `resize_nofill`: the C++ leaves the new entries uninitialised; they are always written before being read.
    pub(crate) fn resize_nofill(&mut self, n: usize) {
        self.start.resize(n, -1);
        self.paired.resize(n, -1);
        self.prop.resize(n, -1);
    }
    pub(crate) fn clear(&mut self) {
        self.start.clear();
        self.paired.clear();
        self.prop.clear();
    }
}
