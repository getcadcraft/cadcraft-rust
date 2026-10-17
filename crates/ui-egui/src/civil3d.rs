//! CivilCraft's 3D site scene (site-view3d.js) in the shared wgpu viewport: cc-civil `build_site3d` (terrain with
//! elevation bands, the graded surface, corridors, pads, pipes and structures, contours, breaklines, alignments) made
//! into cc-render meshes relative to the site origin the engine picks, so survey coordinates stay steady. The 3D
//! renderer draws edges in one colour; the scene's coloured lines (contours, alignments, breaklines…) become thin
//! crossed ribbons in their own colours instead. Labels are egui text projected through the camera, decluttered by the
//! engine's `declutter_labels`.

use cc_civil::project::Project;
use cc_civil::site3d::{self, Label, Opts, Scene, Show};
use cc_render::mesh3d::{Cam3, Instance3, MeshData};
use egui::{Color32, Pos2, Rect};

use crate::view3d::{Scene3D, View3};

/// One built site scene with its camera.
pub struct Site3 {
    pub scene: Scene3D,
    pub view: View3,
    /// labels with world positions (origin + exaggerated z)
    pub labels: Vec<(Label, [f64; 3])>,
    pub origin: [f64; 3],
    pub exag: f64,
    pub notes: Vec<String>,
    pub triangles: u64,
    pub segments: u64,
    pub build_ms: f64,
    /// the project version it was built from
    pub version: u64,
}

fn hex_rgba(s: &str) -> [u8; 4] {
    let c = site3d::rgb(s);
    let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [b(c[0]), b(c[1]), b(c[2]), 255]
}

/// Crossed ribbons (one flat, one upright) for segment pairs in `pos` (f32 xyz pairs, origin-relative).
fn ribbons(pos: &[f32], w: f64, col: [u8; 4], out: &mut MeshData) {
    let h = (w / 2.0) as f32;
    for seg in pos.as_chunks::<6>().0 {
        let (a, b) = ([seg[0], seg[1], seg[2] + h * 0.5], [seg[3], seg[4], seg[5] + h * 0.5]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = (dx * dx + dy * dy).sqrt();
        let (nx, ny) = if l > 1e-9 { (-dy / l * h, dx / l * h) } else { (h, 0.0) };
        let quads = [
            (
                [a[0] + nx, a[1] + ny, a[2]],
                [a[0] - nx, a[1] - ny, a[2]],
                [b[0] - nx, b[1] - ny, b[2]],
                [b[0] + nx, b[1] + ny, b[2]],
                [0.0, 0.0, 1.0],
            ),
            (
                [a[0], a[1], a[2] + h],
                [a[0], a[1], a[2] - h],
                [b[0], b[1], b[2] - h],
                [b[0], b[1], b[2] + h],
                [ny / h.max(1e-9), -nx / h.max(1e-9), 0.0],
            ),
        ];
        for (p0, p1, p2, p3, n) in quads {
            let base = out.positions.len() as u32;
            for p in [p0, p1, p2, p3] {
                out.positions.push(p);
                out.normals.push(n);
                out.colors.push(col);
            }
            out.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
}

/// Build the site scene of `p` (civilcraft.js 3D view: `siteInput` → `buildSite3D`).
pub fn build(p: &mut Project, dark: bool, exag: f64, id: u64) -> Site3 {
    let t0 = web_time::Instant::now();
    let input = site3d::site_input(p);
    let opts = Opts {
        exag: Some(serde_json::json!(exag)),
        color: Some("bands".into()),
        band_count: None,
        dark,
        colors: Vec::new(),
        show: Show::default(),
        drape: None,
    };
    let sc: Scene = site3d::build_site3d(&input, &opts);
    drop(input);
    let origin = [sc.origin.x, sc.origin.y, sc.origin.z];
    let mut meshes: Vec<MeshData> = Vec::new();
    let mut tris = 0u64;
    for m in &sc.meshes {
        let mut d = MeshData { origin, ..MeshData::default() };
        let pos = m.positions.as_chunks::<3>().0;
        let nrm = m.normals.as_chunks::<3>().0;
        let col = m.colors.as_chunks::<3>().0;
        for (i, q) in pos.iter().enumerate() {
            d.positions.push(*q);
            d.normals.push(nrm.get(i).copied().unwrap_or([0.0, 0.0, 1.0]));
            let c = col.get(i).copied().unwrap_or([0.8, 0.8, 0.8]);
            let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            d.colors.push([b(c[0]), b(c[1]), b(c[2]), 255]);
        }
        let n = pos.len() as u32;
        d.indices = m.indices.iter().copied().filter(|&i| i < n).collect();
        d.indices.truncate(d.indices.len() / 3 * 3);
        tris += (d.indices.len() / 3) as u64;
        meshes.push(d);
    }
    let (lo, hi) = sc.bbox.unwrap_or(([-5.0, -5.0, 0.0], [5.0, 5.0, 3.0]));
    let diag = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2)).sqrt().max(1.0);
    let w = (diag / 1400.0).max(0.02);
    let mut lines = MeshData { origin, ..MeshData::default() };
    let mut segs = 0u64;
    for l in &sc.lines {
        segs += (l.positions.len() / 6) as u64;
        ribbons(&l.positions, w, hex_rgba(&l.color), &mut lines);
    }
    if let Some(pts) = &sc.points {
        let col = hex_rgba(&pts.color);
        let k = (w * 3.0) as f32;
        let mut cross = Vec::new();
        for q in pts.positions.as_chunks::<3>().0 {
            cross.extend_from_slice(&[q[0] - k, q[1], q[2], q[0] + k, q[1], q[2], q[0], q[1] - k, q[2], q[0], q[1] + k, q[2]]);
        }
        ribbons(&cross, w, col, &mut lines);
    }
    if !lines.indices.is_empty() {
        tris += (lines.indices.len() / 3) as u64;
        meshes.push(lines);
    }
    let instances: Vec<Instance3> =
        (0..meshes.len()).map(|i| Instance3 { mesh: i, m: cc_render::mesh3d::IDENT, tint: None, hidden: false }).collect();
    let min = [origin[0] + lo[0], origin[1] + lo[1], origin[2] + lo[2]];
    let max = [origin[0] + hi[0], origin[1] + hi[1], origin[2] + hi[2]];
    let labels = sc.labels.iter().map(|l| (l.clone(), [origin[0] + l.at[0], origin[1] + l.at[1], origin[2] + l.at[2]])).collect();
    let mut view = View3::default();
    view.cam.eye = [0.6, -1.0, 0.75];
    view.cam.target = [0.0, 0.0, 0.0];
    Site3 {
        scene: Scene3D::new(id, meshes, instances, min, max),
        view,
        labels,
        origin,
        exag: sc.exag,
        notes: sc.notes.clone(),
        triangles: tris,
        segments: segs,
        build_ms: t0.elapsed().as_secs_f64() * 1000.0,
        version: p.version,
    }
}

/// World → physical pixels of the camera's drawing area (x, y, depth in 0..1 reversed); None behind the eye.
pub fn project(cam: &Cam3, p: [f64; 3]) -> Option<[f64; 3]> {
    let m = cc_render::mesh3d::mmul(&cam.proj(), &cam.view_rot());
    let q = [p[0] - cam.eye[0], p[1] - cam.eye[1], p[2] - cam.eye[2]];
    let c = |r: usize| -> f64 {
        m.get(r).copied().unwrap_or(0.0) * q[0]
            + m.get(r + 4).copied().unwrap_or(0.0) * q[1]
            + m.get(r + 8).copied().unwrap_or(0.0) * q[2]
            + m.get(r + 12).copied().unwrap_or(0.0)
    };
    let (x, y, z, w) = (c(0), c(1), c(2), c(3));
    if w.is_nan() || w <= 1e-12 {
        return None;
    }
    let (nx, ny) = (x / w, y / w);
    Some([(nx + 1.0) / 2.0 * cam.width, (1.0 - ny) / 2.0 * cam.height, z / w])
}

/// The scene's labels over the 3D view, decluttered (the engine's priorities: alignments first, points last).
pub fn paint_labels(ui: &egui::Ui, rect: Rect, s: &Site3, text: Color32, halo: Color32) -> usize {
    let ppp = f64::from(ui.ctx().pixels_per_point());
    let cam = s.view.cam;
    let rel: Vec<Label> = s.labels.iter().map(|(l, w)| Label { at: *w, ..l.clone() }).collect();
    let keep = site3d::declutter_labels(&rel, &|p| project(&cam, p), cam.width, cam.height, 14.0 * ppp, 6.5 * ppp, 4.0 * ppp);
    let painter = ui.painter_at(rect);
    let mut n = 0;
    for (i, l) in rel.iter().enumerate() {
        if keep.get(i).copied() != Some(1) {
            continue;
        }
        let Some(q) = project(&cam, l.at) else { continue };
        let pos = Pos2::new(rect.min.x + (q[0] / ppp) as f32, rect.min.y + (q[1] / ppp) as f32);
        if !rect.contains(pos) {
            continue;
        }
        // the scene's colours are 0.5's paper colours: a dark one on a dark halo gives way to the theme's text
        let col = if l.color.is_empty() {
            text
        } else {
            let c = hex_rgba(&l.color);
            let c = Color32::from_rgb(c[0], c[1], c[2]);
            if crate::theme::contrast(c, halo) < 3.0 { text } else { c }
        };
        let font = egui::FontId::proportional(11.0);
        let g = painter.layout_no_wrap(l.text.clone(), font, col);
        let r = egui::Rect::from_center_size(pos - egui::vec2(0.0, g.size().y * 0.7), g.size() + egui::vec2(6.0, 2.0));
        painter.rect_filled(r, 2.0, halo.gamma_multiply(0.82));
        painter.galley(r.min + egui::vec2(3.0, 1.0), g, col);
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_centres_the_target_at_survey_coordinates() {
        let mut cam = Cam3::default();
        cam.fit([500_000.0, 4_000_000.0, 100.0], [500_400.0, 4_000_300.0, 130.0], Some([0.5, -1.0, 0.8]));
        let q = project(&cam, cam.target).unwrap();
        assert!((q[0] - cam.width / 2.0).abs() < 1e-6 && (q[1] - cam.height / 2.0).abs() < 1e-6, "{q:?}");
        // a millimetre to the right stays a steady fraction of a pixel to the right
        let r = project(&cam, [cam.target[0] + 0.001, cam.target[1], cam.target[2]]).unwrap();
        assert!(r[0] != q[0]);
    }

    #[test]
    fn ribbons_make_two_quads_per_segment() {
        let mut m = MeshData::default();
        ribbons(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0.1, [1, 2, 3, 255], &mut m);
        assert_eq!(m.indices.len(), 12);
        assert_eq!(m.positions.len(), 8);
    }
}
