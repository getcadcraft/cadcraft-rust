//! CadCraft L0: points, vectors, transforms, tolerances.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

pub mod bvh;

/// Model-space length tolerance (drawing units). One documented constant for every comparison.
pub const TOL: f64 = 1e-9;

/// A 2D point or vector in drawing units (f64 throughout).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct P2 {
    pub x: f64,
    pub y: f64,
}

/// A 3D point or vector in drawing units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct P3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl P2 {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn dist(self, o: P2) -> f64 {
        (self.x - o.x).hypot(self.y - o.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dist() {
        assert!((P2::new(0.0, 0.0).dist(P2::new(3.0, 4.0)) - 5.0).abs() < TOL);
    }
}
