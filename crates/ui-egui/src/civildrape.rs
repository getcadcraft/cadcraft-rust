//! Map layers draped on the surfaces in the 3D view (0.4 V3: site-view3d.js drapeFor + gis-map-ui.js reg.drape): the
//! visible map layers painted over the surfaces' box (civilmaps.rs maps_drape, the picture the PDF and the DWG carry)
//! and laid on the meshes the engine gives uvs to (site3d.rs drapeUVs). The 3D view's textures are layers of
//! TEX_SIZE²: the picture is cut into TILES × TILES of them and each triangle is clipped to the tiles it crosses, so the
//! drape keeps TILES × TEX_SIZE pixels across.

use cc_render::mesh3d::{MeshData, TEX_SIZE};

/// The drape is cut into TILES × TILES texture layers …
pub const TILES: usize = 4;
/// … starting at this layer (ModelCraft's material textures, if the window showed them, are drawn again later).
pub const LAYER0: u32 = 16;
/// The longer side of the drape picture.
pub const DRAPE_PX: f64 = (TILES as f64) * (TEX_SIZE as f64);

/// The picture (w × h RGBA, row 0 north) → the tile layers (layer, TEX_SIZE² RGBA with rows from v = 0, south).
pub fn tiles(w: u32, h: u32, px: &[u8]) -> Vec<(u32, Vec<u8>)> {
    let n = TEX_SIZE as usize;
    let (wf, hf) = (f64::from(w), f64::from(h));
    let mut out = Vec::with_capacity(TILES * TILES);
    for r in 0..TILES {
        for c in 0..TILES {
            let mut t = vec![255u8; n * n * 4];
            for j in 0..n {
                let v = (r as f64 + (j as f64 + 0.5) / n as f64) / TILES as f64;
                let y = (((1.0 - v) * hf).floor() as i64).clamp(0, i64::from(h) - 1) as usize;
                for i in 0..n {
                    let u = (c as f64 + (i as f64 + 0.5) / n as f64) / TILES as f64;
                    let x = ((u * wf).floor() as i64).clamp(0, i64::from(w) - 1) as usize;
                    let s = (y * w as usize + x) * 4;
                    let d = (j * n + i) * 4;
                    if let (Some(src), Some(dst)) = (px.get(s..s + 4), t.get_mut(d..d + 4)) {
                        dst.copy_from_slice(src);
                    }
                }
            }
            out.push((LAYER0 + (r * TILES + c) as u32, t));
        }
    }
    out
}

/// A vertex while clipping: position, normal, colour, uv.
#[derive(Clone, Copy)]
struct Vx {
    p: [f32; 3],
    n: [f32; 3],
    c: [f32; 4],
    uv: [f32; 2],
}
fn lerp(a: &Vx, b: &Vx, t: f32) -> Vx {
    let l = |x: f32, y: f32| x + (y - x) * t;
    Vx {
        p: [l(a.p[0], b.p[0]), l(a.p[1], b.p[1]), l(a.p[2], b.p[2])],
        n: [l(a.n[0], b.n[0]), l(a.n[1], b.n[1]), l(a.n[2], b.n[2])],
        c: [l(a.c[0], b.c[0]), l(a.c[1], b.c[1]), l(a.c[2], b.c[2]), l(a.c[3], b.c[3])],
        uv: [l(a.uv[0], b.uv[0]), l(a.uv[1], b.uv[1])],
    }
}
/// Sutherland–Hodgman against one axis-aligned line in uv: keep `axis` ≥ k (`ge`) or ≤ k.
fn clip(poly: &[Vx], axis: usize, k: f32, ge: bool) -> Vec<Vx> {
    let at = |v: &Vx| if axis == 0 { v.uv[0] } else { v.uv[1] };
    let inside = |v: &Vx| if ge { at(v) >= k } else { at(v) <= k };
    let mut out = Vec::with_capacity(poly.len() + 2);
    for (i, a) in poly.iter().enumerate() {
        let Some(b) = poly.get((i + 1) % poly.len()) else { continue };
        let (ia, ib) = (inside(a), inside(b));
        if ia {
            out.push(*a);
        }
        if ia != ib {
            let d = at(b) - at(a);
            if d != 0.0 {
                out.push(lerp(a, b, (k - at(a)) / d));
            }
        }
    }
    out
}

/// A mesh with uvs (u east, v north over the drape box) → the same surface with its triangles cut at the tiles,
/// textured from the tile layers.
pub fn drape_mesh(m: &MeshData, uvs: &[f32]) -> MeshData {
    let mut out = MeshData { origin: m.origin, ..MeshData::default() };
    let vx = |i: u32| -> Option<Vx> {
        let i = i as usize;
        let c = m.colors.get(i)?;
        Some(Vx {
            p: *m.positions.get(i)?,
            n: m.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]),
            c: [f32::from(c[0]), f32::from(c[1]), f32::from(c[2]), f32::from(c[3])],
            uv: [*uvs.get(2 * i)?, *uvs.get(2 * i + 1)?],
        })
    };
    let tn = TILES as f32;
    let inset = 0.5 / TEX_SIZE as f32;
    for tri in m.indices.as_chunks::<3>().0 {
        let (Some(a), Some(b), Some(c)) =
            (tri.first().and_then(|&i| vx(i)), tri.get(1).and_then(|&i| vx(i)), tri.get(2).and_then(|&i| vx(i)))
        else {
            continue;
        };
        let g = |v: &Vx, k: usize| if k == 0 { v.uv[0] } else { v.uv[1] };
        let lo = |k: usize| g(&a, k).min(g(&b, k)).min(g(&c, k));
        let hi = |k: usize| g(&a, k).max(g(&b, k)).max(g(&c, k));
        let span = |l: f32, h: f32| ((l * tn).floor().max(0.0) as usize, ((h * tn).ceil().min(tn) as usize).max(1));
        let (c0, c1) = span(lo(0), hi(0));
        let (r0, r1) = span(lo(1), hi(1));
        for r in r0..r1.min(TILES) {
            for col in c0..c1.min(TILES) {
                let (u0, u1) = (col as f32 / tn, (col + 1) as f32 / tn);
                let (v0, v1) = (r as f32 / tn, (r + 1) as f32 / tn);
                let mut poly = vec![a, b, c];
                for (axis, k, ge) in [(0, u0, true), (0, u1, false), (1, v0, true), (1, v1, false)] {
                    poly = clip(&poly, axis, k, ge);
                    if poly.len() < 3 {
                        break;
                    }
                }
                if poly.len() < 3 {
                    continue;
                }
                let layer = (LAYER0 as usize + r * TILES + col) as f32;
                let base = out.positions.len() as u32;
                for v in &poly {
                    out.positions.push(v.p);
                    out.normals.push(v.n);
                    let b = |x: f32| x.round().clamp(0.0, 255.0) as u8;
                    out.colors.push([b(v.c[0]), b(v.c[1]), b(v.c[2]), b(v.c[3])]);
                    let tu = ((v.uv[0] * tn - col as f32).clamp(0.0, 1.0)) * (1.0 - 2.0 * inset) + inset;
                    let tv = ((v.uv[1] * tn - r as f32).clamp(0.0, 1.0)) * (1.0 - 2.0 * inset) + inset;
                    out.tex.push([tu, tv, layer]);
                }
                for k in 1..poly.len() as u32 - 1 {
                    out.indices.extend_from_slice(&[base, base + k, base + k + 1]);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_triangle_over_four_tiles_is_cut_and_keeps_its_area() {
        let m = MeshData {
            positions: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            colors: vec![[255, 0, 0, 255]; 3],
            indices: vec![0, 1, 2],
            ..MeshData::default()
        };
        let d = drape_mesh(&m, &[0.0, 0.0, 0.6, 0.0, 0.0, 0.6]);
        let area: f32 = d
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let (a, b, c) = (d.positions[t[0] as usize], d.positions[t[1] as usize], d.positions[t[2] as usize]);
                ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
            })
            .sum();
        assert!((area - 50.0).abs() < 1e-3, "{area}");
        assert!(d.tex.iter().all(|t| t[2] >= LAYER0 as f32 && t[0] > 0.0 && t[0] < 1.0));
        assert!(d.tex.iter().map(|t| t[2] as u32).collect::<std::collections::BTreeSet<_>>().len() >= 3);
    }

    #[test]
    fn tiles_cover_the_picture_north_up() {
        // 2 × 2 picture: top row red, bottom row blue
        let px = [255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255];
        let t = tiles(2, 2, &px);
        assert_eq!(t.len(), TILES * TILES);
        assert_eq!(&t[0].1[0..4], &[0, 0, 255, 255], "v = 0 is south: blue");
        assert_eq!(&t[TILES * TILES - 1].1[t[0].1.len() - 4..], &[255, 0, 0, 255]);
    }
}
