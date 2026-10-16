//! Binary STL: io.js `exportSTL`. Z up (the printing convention) and millimetres (×1000), because slicers read STL
//! as millimetres. Edges without faces are not written (they have no surface).

use cc_m3d::Model;
use cc_m3d::vec::{IDENT, mul};

use crate::util::flat_tris;

/// f64 → f32 as `DataView.setFloat32` does (round to nearest, ties to even; out of range → ±Infinity).
#[inline]
pub fn f32_of(x: f64) -> f32 {
    // the IEEE conversion, exactly what ToFloat32 is
    #[allow(clippy::cast_possible_truncation)]
    let v = x as f32;
    v
}

/// `exportSTL(model)` → the bytes.
pub fn export_stl(model: &Model) -> Vec<u8> {
    let tris = flat_tris(model, "root", &IDENT);
    let cap = tris.len().checked_mul(50).and_then(|n| n.checked_add(84)).unwrap_or(84);
    let mut out = Vec::with_capacity(cap);
    let mut head = [0u8; 80];
    let h = b"ModelCraft STL, millimeters";
    for (d, s) in head.iter_mut().zip(h.iter()) {
        *d = *s;
    }
    out.extend_from_slice(&head);
    out.extend_from_slice(&u32::try_from(tris.len()).unwrap_or(u32::MAX).to_le_bytes());
    for t in &tris {
        for v in [t.n, mul(t.p[0], 1000.0), mul(t.p[1], 1000.0), mul(t.p[2], 1000.0)] {
            for x in v {
                out.extend_from_slice(&f32_of(x).to_le_bytes());
            }
        }
        out.extend_from_slice(&[0, 0]);
    }
    out
}
