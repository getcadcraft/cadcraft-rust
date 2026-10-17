// Copyright 2021 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on
// an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.
//
//! cc-manifold: the mesh boolean of [Manifold](https://github.com/elalish/manifold) 3.5.4 (C++, Apache-2.0), ported to
//! pure Rust with **the same output as the manifold-3d 3.5.4 WebAssembly build** the CadCraft 0.5 apps use
//! (`vendor/manifold-3d/3.5.4`): same vertex and triangle counts, same order, bit-equal floats.
//!
//! What is ported is what CadCraft calls (cc-ifc's geo-bool.js for IFC openings and boolean results, ModelCraft's
//! solids.js, PartCraft's kernel.js, drawing.js `meshSection` and assembly.js interference / motion checks):
//! `MeshGL::Merge`, the `Manifold(MeshGL)` constructor with its validation and status codes, the boolean
//! (boolean3.cpp, boolean_result.cpp), edge collapse and swap (edge_op.cpp), face triangulation (face_op.cpp,
//! polygon.cpp incl. the convex fast path, tree2d.cpp), sorting (sort.cpp), the collider, the lazy CSG tree with lazy
//! transforms and batch union / intersection of a list (csg_tree.cpp), `GetMeshGL`; transforms (`Transform`,
//! `Translate`, `Rotate`, `Scale`, `Mirror`, impl.cpp `Impl::Transform` with svd.h's spectral norm), `AsOriginal`,
//! `OriginalID`, the properties (`Volume`, `SurfaceArea`, `Genus`, `BoundingBox`, `NumTri`, the `Decompose` count),
//! the constructors `Cube`, `Sphere` (subdivision.cpp's triangle refinement), `Extrude`, `Revolve`, `TrimByPlane`,
//! `MinGap` (tri_dist.h), the convex hull (quickhull.cpp) and `MinkowskiSum` / `MinkowskiDifference` (minkowski.cpp).
//! Not ported: SDF, smoothing and refinement, `Split`, `Slice`, `Project`, ray casts, `Warp`, `SetProperties`.
//!
//! Parity notes:
//! - The wasm build is single-threaded (`MANIFOLD_PAR=OFF`) with `-ffp-contract=off`: this port follows the sequential
//!   paths of parallel.h and never fuses a multiply-add; Manifold's own deterministic `math::acos` is ported.
//! - Mesh IDs come from one process-wide counter starting at 1, as in the C++; they appear in `run_original_id` and
//!   order the output runs, so a sequence of calls numbers its results as the same sequence does in the wasm (every
//!   constructor, hull and `AsOriginal` takes its IDs in the C++ order).
//! - Transforms stay lazy as in the C++ (composed as `m * Mat4(t)`, applied when a mesh is needed, read unapplied by
//!   `Compose` and the batch union's box tests), so the same chain of calls gives the same floats.
//! - The lazy CSG tree collapses nested operations only when no other [`Manifold`] holds the inner node, exactly as
//!   the C++ checks `use_count`: dropping a `Manifold` here matches calling `.delete()` on it in JavaScript.
//! - Where the C++ reads out of bounds or would loop forever (only on hostile input), this port stops with
//!   [`Status::Internal`]; where it would overflow an `int` or run out of memory, with [`Status::ResultTooLarge`].
//!   Inputs past [`MAX_TRIS`], [`MAX_VERTS`], [`MAX_NUM_PROP`] are refused with `ResultTooLarge`.
//! - Undefined behaviour in the C++ on malformed input, answered here: `Mesh::merge` with a triangle or merge index
//!   past the vertices (the wasm reads out of bounds, sometimes traps) changes nothing and returns false, so the
//!   constructor then reports `VertexOutOfBounds` / `MergeIndexOutOfBounds`; `num_prop == 0` (the wasm divides by
//!   zero and traps) is `MissingPositionProperties`; run indices past the triangles, or fewer run boundaries than
//!   runs, are `RunIndexWrongLength`; triangles outside every run start with a zeroed reference (uninitialised in
//!   the C++).
//! - Same results, less work: the importer skips one of the C++'s two vertex-normal passes (its result is overwritten
//!   before anything reads it); hash maps only serve lookups, so their hasher does not matter.
//! - libc++ version: `AssembleHalfedges` relies on `std::multimap::find` returning the first remaining edge of equal
//!   start vertices (libc++ up to 21; libc++ 22 returns whichever equal node its tree search meets first). It only
//!   matters for a result face whose outline visits one vertex twice, which no parity case produced.
//!
//! ```
//! use cc_manifold::{Manifold, Mesh};
//! let cube = |o: f32| -> Mesh {
//!     let mut v = Vec::new();
//!     for x in [0.0, 1.0] { for y in [0.0, 1.0] { for z in [0.0, 1.0] { v.extend([o + x, o + y, o + z]); } } }
//!     let t = vec![1, 0, 4, 2, 4, 0, 1, 3, 0, 3, 1, 5, 3, 2, 0, 3, 7, 2, 5, 4, 6, 5, 1, 4, 6, 4, 2, 7, 6, 2, 7, 3, 5, 7, 5, 6];
//!     Mesh::new(3, v, t)
//! };
//! let a = Manifold::from_mesh(&cube(0.0)).unwrap();
//! let b = Manifold::from_mesh(&cube(0.5)).unwrap();
//! let r = a.subtract(&b).get_mesh();
//! // the same counts, order and IDs manifold-3d 3.5.4 gives (a fresh process numbers meshes from 1)
//! assert_eq!((r.num_tri(), r.num_vert()), (24, 14));
//! assert_eq!(r.run_original_id, [1, 2]);
//! assert_eq!(r.run_index, [0, 54, 72]);
//! ```

// Lints: the whole crate denies panics and unchecked indexing. The input and output paths (this file, import.rs,
// merge.rs, export.rs, util.rs) also deny unchecked integer arithmetic and lossy casts. The algorithm modules allow
// those two (their indices are bounded by the input caps below: every count stays far under i32::MAX) and say so at
// their top.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

mod boolean3;
mod boolean_result;
mod collider;
mod constructors;
mod csg;
mod disjoint;
mod edge_op;
mod export;
mod face_op;
mod halfedge;
mod hull;
mod imp;
mod import;
mod linalg;
mod mat;
mod math;
mod merge;
mod minkowski;
mod polygon;
mod quickhull;
mod sort;
mod transform;
mod util;

pub use csg::Manifold;

/// Largest accepted input: triangles per mesh.
pub const MAX_TRIS: usize = 1 << 23;
/// Largest accepted input: property vertices per mesh.
pub const MAX_VERTS: usize = 1 << 23;
/// Largest accepted number of properties per vertex (`num_prop`).
pub const MAX_NUM_PROP: u32 = 64;
/// Largest number of edge × face intersections one boolean may produce (each direction).
pub const MAX_INTERSECTIONS: usize = 1 << 23;
/// Largest number of vertices a boolean result may have before triangulation.
pub const MAX_RESULT_VERTS: usize = 1 << 24;

/// `Manifold::Error`: why a mesh could not become a Manifold, or why an operation failed. The names (`as_str`) are the
/// ones manifold-3d's `status()` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    NoError,
    NonFiniteVertex,
    NotManifold,
    VertexOutOfBounds,
    PropertiesWrongLength,
    MissingPositionProperties,
    MergeVectorsDifferentLengths,
    MergeIndexOutOfBounds,
    TransformWrongLength,
    RunIndexWrongLength,
    FaceIDWrongLength,
    InvalidConstruction,
    ResultTooLarge,
    InvalidTangents,
    Cancelled,
    /// Rust only: the input drove the algorithm where the C++ reads out of bounds or never terminates (hostile input).
    Internal,
}

impl Status {
    /// The manifold-3d status string (`"NoError"`, `"NotManifold"`, …; `"Internal"` is this port's own).
    pub fn as_str(self) -> &'static str {
        match self {
            Status::NoError => "NoError",
            Status::NonFiniteVertex => "NonFiniteVertex",
            Status::NotManifold => "NotManifold",
            Status::VertexOutOfBounds => "VertexOutOfBounds",
            Status::PropertiesWrongLength => "PropertiesWrongLength",
            Status::MissingPositionProperties => "MissingPositionProperties",
            Status::MergeVectorsDifferentLengths => "MergeVectorsDifferentLengths",
            Status::MergeIndexOutOfBounds => "MergeIndexOutOfBounds",
            Status::TransformWrongLength => "TransformWrongLength",
            Status::RunIndexWrongLength => "RunIndexWrongLength",
            Status::FaceIDWrongLength => "FaceIDWrongLength",
            Status::InvalidConstruction => "InvalidConstruction",
            Status::ResultTooLarge => "ResultTooLarge",
            Status::InvalidTangents => "InvalidTangents",
            Status::Cancelled => "Cancelled",
            Status::Internal => "Internal",
        }
    }
}

impl core::fmt::Display for Status {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `OpType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpType {
    Add,
    Subtract,
    Intersect,
}

/// `MeshGL` (single precision): the mesh handed in and out. `vert_properties` holds `num_prop` floats per vertex, the
/// first three the position; `tri_verts` three vertex indices per triangle, counter-clockwise from outside. The other
/// fields are optional on input and filled on output, with manifold-3d's meaning.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    pub num_prop: u32,
    pub vert_properties: Vec<f32>,
    pub tri_verts: Vec<u32>,
    pub merge_from_vert: Vec<u32>,
    pub merge_to_vert: Vec<u32>,
    pub run_index: Vec<u32>,
    pub run_original_id: Vec<u32>,
    pub run_transform: Vec<f32>,
    pub run_flags: Vec<u8>,
    pub face_id: Vec<u32>,
    pub halfedge_tangent: Vec<f32>,
    pub tolerance: f32,
}

impl Mesh {
    /// A mesh from positions (+ properties) and triangles, the other fields empty (what CadCraft builds).
    pub fn new(num_prop: u32, vert_properties: Vec<f32>, tri_verts: Vec<u32>) -> Mesh {
        Mesh { num_prop, vert_properties, tri_verts, ..Mesh::default() }
    }

    /// Number of vertices (`vertProperties.length / numProp`).
    pub fn num_vert(&self) -> usize {
        match usize::try_from(self.num_prop) {
            Ok(n) => self.vert_properties.len().checked_div(n).unwrap_or(0),
            _ => 0,
        }
    }

    /// Number of triangles.
    pub fn num_tri(&self) -> usize {
        self.tri_verts.len() / 3
    }

    /// `MeshGL::Merge`: fills `merge_from_vert` / `merge_to_vert` so open edges that meet within tolerance are joined.
    /// Returns false (and changes nothing) when the mesh is already closed. manifold-3d's `mesh.merge()`.
    pub fn merge(&mut self) -> bool {
        merge::merge(self)
    }
}
