//! A sliver of a surface (one side near zero) asked the locate grid for millions of cells along the other side
//! (bim_ground fuzz find, M7.22): the grid is capped at 16 n cells a side.

use cc_civil::tin::{ZPt, build_surface};

#[test]
fn sliver_grid_is_bounded() {
    let pts = [ZPt { x: 0.0, y: 0.0, z: Some(1.0) }, ZPt { x: 1.0e7, y: 0.0, z: Some(2.0) }, ZPt { x: 0.5e7, y: 1.0e-3, z: Some(3.0) }];
    let s = build_surface(&pts, &[], None, 0.0).surface.expect("a surface of one triangle");
    let g = s.grid();
    assert!(g.nx * g.ny <= 256, "{} × {} cells", g.nx, g.ny);
    assert!(s.z_at(5.0e6, 0.0001).is_some());
}
