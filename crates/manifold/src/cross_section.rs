// Copyright 2023 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
//! cross_section.cpp: Manifold's 2D `CrossSection` on Clipper2 (crate::clipper), with the same output as manifold-3d
//! 3.5.4's wasm build: polygons are unioned (or combined) by Clipper2 at precision 8 (coordinates rounded to 2^-27),
//! and transforms are kept lazily as a 2×3 matrix and applied when the paths are next read, as the C++ does.
//!
//! Errors: Clipper2 throws on coordinates past its range (|x| · 2^27 > 2^61), which manifold-3d passes to JavaScript as
//! a thrown exception; the `try_*` functions return [`ClipError`] then, the plain ones an empty CrossSection.
//! Non-finite coordinates: NaN becomes 0 (the wasm's saturating float → int conversion), ±∞ is a range error.
//! Inputs past [`MAX_CS_POINTS`] points are refused ([`ClipError::TooLarge`]).
//!
//! Ported: the constructor (fill rules), `Square`, `Circle`, booleans and `BatchBoolean`, `Area`, `IsEmpty`,
//! `NumVert`, `NumContour`, `Bounds`, `ToPolygons`, `Translate` / `Rotate` / `Scale` / `Mirror` / `Transform`.
//! Not ported: `Offset`, `Simplify`, `Hull`, `Decompose`, `Warp`.

use std::sync::Arc;

use crate::OpType;
pub use crate::clipper::{ClipError, FillRule};
use crate::clipper::{ClipType, PD, PathD, area_d, boolean_op_d};
use crate::linalg::{dot2, la_max, la_min, v2};
use crate::math::{cosd, sind};

/// Largest accepted input, in points over all contours.
pub const MAX_CS_POINTS: usize = 1 << 22;
/// Largest number of circle segments accepted (`Circle` with more returns an empty CrossSection).
pub const MAX_CIRCLE_SEGMENTS: i32 = 1 << 20;

/// cross_section.cpp `precision_`.
const PRECISION: i32 = 8;

/// The 2×3 identity, column-major.
const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `Quality::GetCircularSegments(radius)` with the default settings (angle 10°, edge length 1, no fixed count).
// static_cast<int>(double) as the wasm executes it: truncating and saturating (non-trapping conversion), as `as` does
#[allow(clippy::cast_possible_truncation)]
pub fn circular_segments(radius: f64) -> i32 {
    // common.h kPi
    const K_PI: f64 = core::f64::consts::PI;
    let n_seg_a = (360.0f64 / 10.0) as i32;
    // static_cast<int>: the wasm build saturates (non-trapping conversion)
    let n_seg_l = (2.0 * radius.abs() * K_PI / 1.0) as i32;
    let mut n_seg = (f64::from(n_seg_a).min(f64::from(n_seg_l)) + 3.0) as i32;
    n_seg = n_seg.wrapping_sub(n_seg.wrapping_rem(4));
    n_seg.max(4)
}

/// A 2D region: non-overlapping polygons (outer contours counter-clockwise, holes clockwise).
#[derive(Clone, Debug)]
pub struct CrossSection {
    paths: Arc<Vec<PathD>>,
    /// column-major 2×3, applied lazily
    transform: [f64; 6],
}

impl Default for CrossSection {
    /// The empty CrossSection.
    fn default() -> Self {
        CrossSection { paths: Arc::new(Vec::new()), transform: IDENTITY }
    }
}

/// `transform(paths, m)`: every point through `m`, contours reversed when `m` mirrors.
fn transform_paths(ps: &[PathD], m: &[f64; 6]) -> Vec<PathD> {
    let [a, b, c, d, e, f] = *m;
    let invert = a * d - b * c < 0.0;
    ps.iter()
        .map(|path| {
            let mut s: Vec<PD> = path.iter().map(|p| PD { x: a * p.x + c * p.y + e * 1.0, y: b * p.x + d * p.y + f * 1.0 }).collect();
            if invert {
                s.reverse();
            }
            s
        })
        .collect()
}

fn to_pathd(polys: &[Vec<[f64; 2]>]) -> Vec<PathD> {
    polys.iter().map(|p| p.iter().map(|q| PD { x: q[0], y: q[1] }).collect()).collect()
}

impl CrossSection {
    fn from_paths(paths: Vec<PathD>) -> CrossSection {
        CrossSection { paths: Arc::new(paths), transform: IDENTITY }
    }

    /// `GetPaths()`: the paths with the pending transform applied.
    fn get_paths(&self) -> Arc<Vec<PathD>> {
        if self.transform == IDENTITY {
            return Arc::clone(&self.paths);
        }
        Arc::new(transform_paths(&self.paths, &self.transform))
    }

    /// `CrossSection(polygons, fillRule)`: the contours unioned with that fill rule (manifold-3d
    /// `new CrossSection(polygons, fillRule)`).
    pub fn try_new(polys: &[Vec<[f64; 2]>], fill: FillRule) -> Result<CrossSection, ClipError> {
        let total = polys.iter().fold(0usize, |n, p| n.saturating_add(p.len()));
        if total > MAX_CS_POINTS {
            return Err(ClipError::TooLarge);
        }
        let ps = to_pathd(polys);
        Ok(CrossSection::from_paths(boolean_op_d(ClipType::Union, fill, &ps, &[], PRECISION)?))
    }

    /// [`CrossSection::try_new`], empty on error.
    pub fn new(polys: &[Vec<[f64; 2]>], fill: FillRule) -> CrossSection {
        CrossSection::try_new(polys, fill).unwrap_or_default()
    }

    /// `Square(size, center)`: empty when a size is negative or both are zero.
    pub fn square(size: [f64; 2], center: bool) -> CrossSection {
        let [x, y] = size;
        if x < 0.0 || y < 0.0 || dot2(v2(x, y), v2(x, y)).sqrt() == 0.0 {
            return CrossSection::default();
        }
        let p = if center {
            let w = x / 2.0;
            let h = y / 2.0;
            vec![PD { x: w, y: h }, PD { x: -w, y: h }, PD { x: -w, y: -h }, PD { x: w, y: -h }]
        } else {
            vec![PD { x: 0.0, y: 0.0 }, PD { x, y: 0.0 }, PD { x, y }, PD { x: 0.0, y }]
        };
        CrossSection::from_paths(vec![p])
    }

    /// `Circle(radius, circularSegments)` (segments ≤ 2: the default count for the radius).
    pub fn circle(radius: f64, segments: i32) -> CrossSection {
        if radius.is_nan() || radius <= 0.0 {
            return CrossSection::default();
        }
        let n = if segments > 2 { segments } else { circular_segments(radius) };
        if n > MAX_CIRCLE_SEGMENTS {
            return CrossSection::default();
        }
        let d_phi = 360.0 / f64::from(n);
        let circle = (0..n).map(|i| PD { x: radius * cosd(d_phi * f64::from(i)), y: radius * sind(d_phi * f64::from(i)) }).collect();
        CrossSection::from_paths(vec![circle])
    }

    /// `Boolean(second, op)` (Positive fill).
    pub fn try_boolean(&self, other: &CrossSection, op: OpType) -> Result<CrossSection, ClipError> {
        let ct = clip_type(op);
        let a = self.get_paths();
        let b = other.get_paths();
        Ok(CrossSection::from_paths(boolean_op_d(ct, FillRule::Positive, &a, &b, PRECISION)?))
    }

    /// [`CrossSection::try_boolean`], empty on error.
    pub fn boolean(&self, other: &CrossSection, op: OpType) -> CrossSection {
        self.try_boolean(other, op).unwrap_or_default()
    }

    /// manifold-3d `add`.
    pub fn add(&self, other: &CrossSection) -> CrossSection {
        self.boolean(other, OpType::Add)
    }

    /// manifold-3d `subtract`.
    pub fn subtract(&self, other: &CrossSection) -> CrossSection {
        self.boolean(other, OpType::Subtract)
    }

    /// manifold-3d `intersect`.
    pub fn intersect(&self, other: &CrossSection) -> CrossSection {
        self.boolean(other, OpType::Intersect)
    }

    /// `BatchBoolean(list, op)`: none → empty, one → that one; union / difference take every later contour as a clip
    /// in one pass, intersection chains pairwise.
    pub fn try_batch_boolean(list: &[CrossSection], op: OpType) -> Result<CrossSection, ClipError> {
        let Some((first, rest)) = list.split_first() else { return Ok(CrossSection::default()) };
        if rest.is_empty() {
            return Ok(first.clone());
        }
        let subjs = first.get_paths();
        if op == OpType::Intersect {
            let mut res: Vec<PathD> = (*subjs).clone();
            for cs in rest {
                res = boolean_op_d(ClipType::Intersection, FillRule::Positive, &res, &cs.get_paths(), PRECISION)?;
            }
            return Ok(CrossSection::from_paths(res));
        }
        let mut clips: Vec<PathD> = Vec::new();
        for cs in rest {
            clips.extend(cs.get_paths().iter().cloned());
            if clips.iter().fold(0usize, |n, p| n.saturating_add(p.len())) > MAX_CS_POINTS {
                return Err(ClipError::TooLarge);
            }
        }
        Ok(CrossSection::from_paths(boolean_op_d(clip_type(op), FillRule::Positive, &subjs, &clips, PRECISION)?))
    }

    /// [`CrossSection::try_batch_boolean`], empty on error.
    pub fn batch_boolean(list: &[CrossSection], op: OpType) -> CrossSection {
        CrossSection::try_batch_boolean(list, op).unwrap_or_default()
    }

    /// `Area()`: signed (holes negative), summed over the contours.
    pub fn area(&self) -> f64 {
        self.get_paths().iter().fold(0.0, |a, p| a + area_d(p))
    }

    /// `IsEmpty()`: no contours.
    pub fn is_empty(&self) -> bool {
        self.get_paths().is_empty()
    }

    /// `NumVert()`.
    pub fn num_vert(&self) -> usize {
        self.get_paths().iter().map(Vec::len).sum()
    }

    /// `NumContour()`.
    pub fn num_contour(&self) -> usize {
        self.get_paths().len()
    }

    /// `Bounds()` as (min, max).
    pub fn bounds(&self) -> ([f64; 2], [f64; 2]) {
        let (mut left, mut top, mut right, mut bottom) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for path in self.get_paths().iter() {
            for p in path {
                if p.x < left {
                    left = p.x;
                }
                if p.x > right {
                    right = p.x;
                }
                if p.y < top {
                    top = p.y;
                }
                if p.y > bottom {
                    bottom = p.y;
                }
            }
        }
        // Rect({left, bottom}, {right, top})
        ([la_min(left, right), la_min(bottom, top)], [la_max(left, right), la_max(bottom, top)])
    }

    /// `ToPolygons()`.
    pub fn to_polygons(&self) -> Vec<Vec<[f64; 2]>> {
        self.get_paths().iter().map(|p| p.iter().map(|q| [q.x, q.y]).collect()).collect()
    }

    /// `Transform(m)`: m is column-major 2×3 (`[m00, m10, m01, m11, tx, ty]`), composed as `m * Mat3(transform_)`.
    pub fn transform(&self, m: [f64; 6]) -> CrossSection {
        let t = self.transform;
        let [a, b, c, d, e, f] = m;
        // column j of the result = m.c0 * t_j.x + m.c1 * t_j.y + m.c2 * (0, 0, 1)_j
        let col = |x: f64, y: f64, z: f64| -> (f64, f64) { (a * x + c * y + e * z, b * x + d * y + f * z) };
        let (r0, r1) = col(t[0], t[1], 0.0);
        let (r2, r3) = col(t[2], t[3], 0.0);
        let (r4, r5) = col(t[4], t[5], 1.0);
        CrossSection { paths: Arc::clone(&self.paths), transform: [r0, r1, r2, r3, r4, r5] }
    }

    /// `Translate(v)`.
    pub fn translate(&self, v: [f64; 2]) -> CrossSection {
        self.transform([1.0, 0.0, 0.0, 1.0, v[0], v[1]])
    }

    /// `Rotate(degrees)` (counter-clockwise).
    pub fn rotate(&self, degrees: f64) -> CrossSection {
        let s = sind(degrees);
        let c = cosd(degrees);
        self.transform([c, s, -s, c, 0.0, 0.0])
    }

    /// `Scale(s)`.
    pub fn scale(&self, s: [f64; 2]) -> CrossSection {
        self.transform([s[0], 0.0, 0.0, s[1], 0.0, 0.0])
    }

    /// `Mirror(axis)`: over the line through the origin square to `axis`; empty for a zero axis.
    pub fn mirror(&self, axis: [f64; 2]) -> CrossSection {
        let a = v2(axis[0], axis[1]);
        let len = dot2(a, a).sqrt();
        if len == 0.0 {
            return CrossSection::default();
        }
        let n = v2(a.x / len, a.y / len);
        // mat2(identity) - 2.0 * outerprod(n, n)
        let m = [1.0 - 2.0 * (n.x * n.x), 0.0 - 2.0 * (n.y * n.x), 0.0 - 2.0 * (n.x * n.y), 1.0 - 2.0 * (n.y * n.y), 0.0, 0.0];
        self.transform(m)
    }
}

fn clip_type(op: OpType) -> ClipType {
    match op {
        OpType::Add => ClipType::Union,
        OpType::Subtract => ClipType::Difference,
        OpType::Intersect => ClipType::Intersection,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_union_and_area() {
        let cs = CrossSection::new(&[vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]], FillRule::Positive);
        // the wasm returns [[1,1],[0,1],[0,0],[1,0]]
        assert_eq!(cs.to_polygons(), vec![vec![[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]]]);
        assert_eq!(cs.area(), 1.0);
        assert_eq!(cs.bounds(), ([0.0, 0.0], [1.0, 1.0]));
        assert_eq!((cs.num_vert(), cs.num_contour(), cs.is_empty()), (4, 1, false));
    }

    #[test]
    fn default_circle_has_eight_segments_for_unit_radius() {
        let c = CrossSection::circle(1.0, 0);
        assert_eq!(c.num_vert(), 8);
        assert_eq!(c.to_polygons()[0][2], [0.0, 1.0]);
        assert_eq!(circular_segments(1.0), 8);
        assert_eq!(circular_segments(1e300), 36);
        assert_eq!(circular_segments(f64::NAN), 4);
    }

    #[test]
    fn range_error_and_caps() {
        assert_eq!(CrossSection::try_new(&[vec![[0.0, 0.0], [1.0, 0.0], [1e12, 1.0]]], FillRule::Positive).err(), Some(ClipError::Range));
        assert!(CrossSection::new(&[vec![[0.0, 0.0], [1.0, 0.0], [f64::INFINITY, 1.0]]], FillRule::Positive).is_empty());
        assert!(CrossSection::circle(1.0, i32::MAX).is_empty());
        assert!(CrossSection::circle(f64::NAN, 5).is_empty());
        assert!(CrossSection::default().mirror([0.0, 0.0]).is_empty());
    }

    #[test]
    fn rotate_then_translate_as_the_wasm() {
        let cs = CrossSection::new(&[vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]], FillRule::Positive)
            .rotate(30.0)
            .translate([1.0, 2.0]);
        let p = cs.to_polygons();
        // [[1.3660254037844388,3.3660254037844384],[0.5,2.866025403784439],[1,2],[1.8660254037844388,2.5]]
        assert_eq!(p[0][0], [1.366_025_403_784_438_8, 3.366_025_403_784_438_4]);
        assert_eq!(p[0][2], [1.0, 2.0]);
    }
}
