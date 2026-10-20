//! CrossSection `Simplify` and `Decompose` (M6.19, through the ported Clipper2 polytree). Exact parity with the
//! manifold-3d 3.5.4 wasm is checked by `forks/tools/csdiff.mjs --kind simplify|text`; these pin the shapes.

use cc_manifold::{CrossSection, FillRule};

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}
fn rev(mut p: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    p.reverse();
    p
}

#[test]
fn decompose_outline_hole_island() {
    // an outline with a hole, an island in the hole, and a separate square
    let cs = CrossSection::new(
        &[rect(0.0, 0.0, 10.0, 10.0), rev(rect(2.0, 2.0, 8.0, 8.0)), rect(4.0, 4.0, 6.0, 6.0), rect(20.0, 0.0, 21.0, 1.0)],
        FillRule::Positive,
    );
    assert_eq!(cs.num_contour(), 4);
    let parts = cs.decompose();
    assert_eq!(parts.len(), 3);
    let contours: Vec<usize> = parts.iter().map(CrossSection::num_contour).collect();
    assert_eq!(contours.iter().filter(|&&n| n == 2).count(), 1, "one outline keeps its hole: {contours:?}");
    let total: f64 = parts.iter().map(CrossSection::area).sum();
    assert!((total - cs.area()).abs() < 1e-9);
    // fewer than two contours: itself
    let one = CrossSection::new(&[rect(0.0, 0.0, 1.0, 1.0)], FillRule::Positive);
    assert_eq!(one.decompose().len(), 1);
    assert!(CrossSection::default().decompose().len() == 1);
}

#[test]
fn simplify_drops_near_collinear_points_and_slivers() {
    let p = vec![[0.0, 0.0], [5.0, 0.000_001], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    let sliver = vec![[20.0, 0.0], [30.0, 0.0], [30.0, 0.000_01], [20.0, 0.000_02]];
    let cs = CrossSection::new(&[p, sliver], FillRule::Positive);
    let s = cs.simplify(0.001);
    assert_eq!(s.num_contour(), 1, "the sliver is narrower than epsilon");
    assert_eq!(s.num_vert(), 4, "the near-collinear point goes");
    assert!((s.area() - 100.0).abs() < 1e-3);
}

#[test]
fn simplify_and_decompose_survive_degenerate_input() {
    for polys in [
        vec![vec![[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]]],
        vec![vec![[0.0, 0.0], [f64::NAN, 1.0], [1.0, 0.0]]],
        vec![rect(0.0, 0.0, 1.0, 1.0), rect(0.0, 0.0, 1.0, 1.0), rev(rect(0.25, 0.25, 0.75, 0.75))],
    ] {
        let cs = CrossSection::new(&polys, FillRule::EvenOdd);
        let _ = cs.simplify(1e-6);
        let _ = cs.simplify(-1.0);
        let _ = cs.simplify(f64::NAN);
        let _ = cs.decompose();
    }
    // past Clipper2's range: an error, not a panic
    let big = CrossSection::new(&[rect(0.0, 0.0, 1.0, 1.0)], FillRule::Positive).scale([1e12, 1.0]);
    assert!(big.try_simplify(1e-6).is_err());
}
