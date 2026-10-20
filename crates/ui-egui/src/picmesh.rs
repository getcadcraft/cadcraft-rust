//! Pictures on the canvas (IMAGE, M4.19): a picture's texture as an egui mesh over its frame.
//!
//! pictures.js `drawPictures` clips the canvas to the frame and draws the bitmap through the picture's affine
//! transform. Here the frame (or IMAGECLIP's clip) is cut to the picture's own rectangle in pixel space, triangulated
//! (ear clipping), and each corner gets its texture coordinate: the same picture, as triangles. Frames with more than
//! [`MAX_POINTS`] points are left out.

use cc_drawing::SpaceImage;
use egui::{Color32, Mesh, Pos2, TextureId};

/// The most frame points a picture's mesh is made from.
pub const MAX_POINTS: usize = 4096;

type Pt = (f64, f64);

/// Cut a polygon to the rectangle [0, w] × [0, h] (Sutherland–Hodgman; the rectangle is convex).
fn clip_rect(poly: Vec<Pt>, w: f64, h: f64) -> Vec<Pt> {
    let edges: [fn(Pt, f64, f64) -> f64; 4] = [|p, _, _| p.0, |p, w, _| w - p.0, |p, _, _| p.1, |p, _, h| h - p.1];
    let mut pts = poly;
    for d in edges {
        if pts.is_empty() {
            break;
        }
        let inp = std::mem::take(&mut pts);
        let n = inp.len();
        for k in 0..n {
            let (Some(&a), Some(&b)) = (inp.get(k), inp.get((k + 1) % n)) else { break };
            let (da, db) = (d(a, w, h), d(b, w, h));
            if da >= 0.0 {
                pts.push(a);
            }
            if (da >= 0.0) != (db >= 0.0) {
                let t = da / (da - db);
                pts.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            }
        }
    }
    pts
}

fn cross(o: Pt, a: Pt, b: Pt) -> f64 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

fn in_tri(p: Pt, a: Pt, b: Pt, c: Pt) -> bool {
    let (d1, d2, d3) = (cross(a, b, p), cross(b, c, p), cross(c, a, p));
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// Ear clipping of a simple polygon → triangles as index triples (a fan when the polygon is not simple).
pub fn triangulate(pts: &[Pt]) -> Vec<[u32; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let area: f64 =
        (0..n).map(|i| cross((0.0, 0.0), pts.get(i).copied().unwrap_or_default(), pts.get((i + 1) % n).copied().unwrap_or_default())).sum();
    let ccw = area > 0.0;
    let mut idx: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n.saturating_sub(2));
    let mut guard = 0usize;
    while idx.len() > 3 && guard < n.saturating_mul(n).saturating_add(8) {
        guard = guard.saturating_add(1);
        let m = idx.len();
        let mut cut = None;
        for i in 0..m {
            let (Some(&ia), Some(&ib), Some(&ic)) = (idx.get((i + m - 1) % m), idx.get(i), idx.get((i + 1) % m)) else { continue };
            let (Some(&a), Some(&b), Some(&c)) = (pts.get(ia), pts.get(ib), pts.get(ic)) else { continue };
            let turn = cross(a, b, c);
            if (ccw && turn <= 0.0) || (!ccw && turn >= 0.0) {
                continue;
            }
            let blocked = idx.iter().any(|&j| j != ia && j != ib && j != ic && pts.get(j).is_some_and(|&p| in_tri(p, a, b, c)));
            if !blocked {
                cut = Some((i, [ia, ib, ic]));
                break;
            }
        }
        match cut {
            Some((i, t)) => {
                out.push(t.map(|v| u32::try_from(v).unwrap_or(0)));
                idx.remove(i);
            }
            None => break,
        }
    }
    if idx.len() == 3 {
        if let (Some(&a), Some(&b), Some(&c)) = (idx.first(), idx.get(1), idx.get(2)) {
            out.push([a, b, c].map(|v| u32::try_from(v).unwrap_or(0)));
        }
    } else if idx.len() > 3 {
        // not simple: a fan over what is left
        if let Some(&first) = idx.first() {
            for w in idx.windows(2).skip(1) {
                if let [b, c] = w {
                    out.push([first, *b, *c].map(|v| u32::try_from(v).unwrap_or(0)));
                }
            }
        }
    }
    out
}

/// The mesh of one picture: `to` maps world → screen points; None when it shows nothing.
pub fn picture_mesh(im: &SpaceImage, tex: TextureId, to: &dyn Fn(f64, f64) -> Pos2) -> Option<Mesh> {
    let [a, b, c, d, e, f] = im.m;
    let det = a * d - b * c;
    if !(det.is_finite() && det.abs() > 1e-300 && im.w > 0.0 && im.h > 0.0) {
        return None;
    }
    // world → pixel
    let px = |x: f64, y: f64| -> Pt {
        let (dx, dy) = (x - e, y - f);
        ((d * dx - c * dy) / det, (a * dy - b * dx) / det)
    };
    let poly: Vec<Pt> = match &im.clip {
        Some(cl) if cl.len() >= 6 => {
            let mut v: Vec<Pt> = cl.as_chunks::<2>().0.iter().map(|[x, y]| px(*x, *y)).collect();
            if v.len() > 1 && v.first() == v.last() {
                v.pop();
            }
            v
        }
        _ => vec![(0.0, 0.0), (im.w, 0.0), (im.w, im.h), (0.0, im.h)],
    };
    if poly.len() > MAX_POINTS || poly.iter().any(|p| !(p.0.is_finite() && p.1.is_finite())) {
        return None;
    }
    let poly = clip_rect(poly, im.w, im.h);
    let tris = triangulate(&poly);
    if tris.is_empty() {
        return None;
    }
    let alpha = im.alpha.map_or(1.0, |v| v.clamp(0.0, 1.0)) as f32;
    let color = Color32::WHITE.gamma_multiply(alpha);
    let mut mesh = Mesh::with_texture(tex);
    for &(u, v) in &poly {
        let (wx, wy) = (a * u + c * v + e, b * u + d * v + f);
        let uv = Pos2::new((u / im.w) as f32, (v / im.h) as f32);
        mesh.vertices.push(egui::epaint::Vertex { pos: to(wx, wy), uv, color });
    }
    for t in tris {
        mesh.indices.extend_from_slice(&t);
    }
    Some(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_is_two_triangles_and_a_concave_frame_clips() {
        assert_eq!(triangulate(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]).len(), 2);
        let l = [(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)];
        assert_eq!(triangulate(&l).len(), 4);
        let c = clip_rect(vec![(-1.0, -1.0), (3.0, -1.0), (3.0, 3.0), (-1.0, 3.0)], 2.0, 1.0);
        assert_eq!(c.len(), 4);
        assert!(c.iter().all(|p| p.0 >= 0.0 && p.0 <= 2.0 && p.1 >= 0.0 && p.1 <= 1.0));
    }

    #[test]
    fn a_picture_mesh_maps_corners_to_uvs() {
        let im = SpaceImage {
            m: [1.0, 0.0, 0.0, -1.0, 10.0, 20.0],
            w: 4.0,
            h: 2.0,
            file: "x.png".into(),
            layer: "0".into(),
            clip: None,
            alpha: Some(0.5),
        };
        let m = picture_mesh(&im, TextureId::default(), &|x, y| Pos2::new(x as f32, y as f32)).unwrap();
        assert_eq!(m.indices.len(), 6);
        assert_eq!(m.vertices[2].uv, Pos2::new(1.0, 1.0));
        assert_eq!(m.vertices[2].pos, Pos2::new(14.0, 18.0));
        let bad = SpaceImage { m: [0.0; 6], ..im };
        assert!(picture_mesh(&bad, TextureId::default(), &|x, y| Pos2::new(x as f32, y as f32)).is_none());
    }
}
