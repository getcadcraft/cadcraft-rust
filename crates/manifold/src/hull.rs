// Copyright 2024 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! `Impl::Hull` (quickhull.cpp), `Impl::IsConvex` (properties.cpp) and the public `Manifold::Hull(points)`.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use crate::Status;
use crate::csg::Manifold;
use crate::halfedge::Halfedges;
use crate::imp::Impl;
use crate::linalg::{V3, cross3, dot3, v3};
use crate::util::{Get, R, ix};

impl Impl {
    /// `Impl::Hull(vertPos)`: this (empty) Impl becomes the convex hull of the points.
    pub(crate) fn hull(&mut self, pts: &[V3]) -> R<()> {
        if pts.is_empty() {
            return Ok(());
        }
        if pts.len() > crate::MAX_VERTS {
            self.make_empty(Status::ResultTooLarge);
            return Ok(());
        }
        let (halfedges, verts) = crate::quickhull::quick_hull(pts)?;
        let mut he = Halfedges::default();
        he.start.reserve(halfedges.len());
        for h in &halfedges {
            he.push(h.start_vert, h.paired_halfedge, h.prop_vert);
        }
        self.halfedge = he;
        self.vert_pos = verts;
        self.calculate_bbox();
        self.set_epsilon(-1.0, false);
        self.initialize_original();
        self.sort_geometry()?;
        self.set_normals_and_coplanar()
    }

    /// `IsConvex`: genus 0 and no concave edge (Minkowski picks its path with it).
    pub(crate) fn is_convex(&self) -> R<bool> {
        let num_vert = ix(self.num_vert())?;
        let num_edge = ix(self.halfedge.len() / 2)?;
        let num_tri = ix(self.num_tri())?;
        let chi = num_vert - num_edge + num_tri;
        let genus = 1 - chi / 2;
        if genus != 0 {
            return Ok(false);
        }
        for idx in 0..ix(self.halfedge.len())? {
            let (s, e) = (self.halfedge.start(idx)?, self.halfedge.end(idx)?);
            if s >= e {
                continue;
            }
            let normal0 = self.face_normal.at(idx / 3)?;
            let normal1 = self.face_normal.at(self.halfedge.pair(idx)? / 3)?;
            if normal0 == normal1 {
                continue;
            }
            let edge_vec = self.vert_pos.at(e)? - self.vert_pos.at(s)?;
            if dot3(edge_vec, cross3(normal0, normal1)) <= 0.0 {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// `Impl::Hull` into a fresh Impl; a fault leaves an `Internal` / `ResultTooLarge` status.
pub(crate) fn hull_impl(pts: &[V3]) -> Impl {
    let mut imp = Impl::default();
    if let Err(f) = imp.hull(pts) {
        imp.make_empty(f.status());
    }
    imp
}

impl Manifold {
    /// `Manifold::Hull(std::vector<vec3>)` (manifold-3d `Manifold.hull(points)`): the convex hull of the points; empty
    /// when they span no volume.
    pub fn hull_points(pts: &[[f64; 3]]) -> Manifold {
        let v: Vec<V3> = pts.iter().map(|p| v3(p[0], p[1], p[2])).collect();
        Manifold::from_impl(hull_impl(&v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mesh;

    fn cube_mesh(o: [f32; 3], s: f32) -> Mesh {
        let mut v = Vec::new();
        for x in [0.0, 1.0] {
            for y in [0.0, 1.0] {
                for z in [0.0, 1.0] {
                    v.extend([o[0] + s * x, o[1] + s * y, o[2] + s * z]);
                }
            }
        }
        let t = vec![1, 0, 4, 2, 4, 0, 1, 3, 0, 3, 1, 5, 3, 2, 0, 3, 7, 2, 5, 4, 6, 5, 1, 4, 6, 4, 2, 7, 6, 2, 7, 3, 5, 7, 5, 6];
        Mesh::new(3, v, t)
    }

    #[test]
    fn convexity() {
        let cube = Impl::from_mesh(&cube_mesh([0.0; 3], 1.0));
        assert!(cube.is_convex().unwrap());
        // L shape: union of two touching boxes is not convex
        let a = Manifold::from_mesh(&cube_mesh([0.0; 3], 1.0)).unwrap();
        let b = Manifold::from_mesh(&cube_mesh([1.0, 0.0, 0.0], 1.0)).unwrap();
        let c = Manifold::from_mesh(&cube_mesh([0.0, 1.0, 0.0], 1.0)).unwrap();
        let l = Manifold::union(&[a, b, c]).get_mesh();
        let li = Impl::from_mesh(&l);
        assert_eq!(li.status, Status::NoError);
        assert!(!li.is_convex().unwrap());
        // a ring (genus 1): big box minus a through box
        let big = Manifold::from_mesh(&cube_mesh([0.0; 3], 3.0)).unwrap();
        let hole = Manifold::from_mesh(&Mesh::new(
            3,
            cube_mesh([1.0, 1.0, -1.0], 1.0).vert_properties.chunks(3).flat_map(|p| [p[0], p[1], p[2] * 5.0 + 4.0]).collect(),
            cube_mesh([0.0; 3], 1.0).tri_verts,
        ))
        .unwrap();
        let ring = Impl::from_mesh(&big.subtract(&hole).get_mesh());
        assert!(!ring.is_convex().unwrap());
        // a hull is convex
        let pts: Vec<V3> = (0..50).map(|i| v3(f64::from(i).sin(), f64::from(i * 7).cos(), f64::from(i * 3).sin() * 0.5)).collect();
        let h = hull_impl(&pts);
        assert_eq!(h.status, Status::NoError, "{:?}", crate::quickhull::quick_hull(&pts).map(|x| x.1.len()));
        assert!(h.num_tri() > 4, "{}", h.num_tri());
        assert!(h.is_convex().unwrap());
    }

    #[test]
    fn hull_hostile() {
        assert!(hull_impl(&[]).is_empty());
        let _ = hull_impl(&[v3(f64::NAN, 1.0, 2.0); 6]);
        let _ = hull_impl(&[v3(1.0, 1.0, 1.0); 3]);
    }
}
