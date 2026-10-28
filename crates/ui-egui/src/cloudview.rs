//! A point cloud under the drawing (cloud-cad.js in cadcraft.js, 0.5 SG.1): the window keeps one cloud, under
//! whichever drawing shows ([`cloud`]); the Model tab draws it under the linework (every point seen from above, the
//! highest on top, or only the slice) and each sheet viewport draws it through its window; object snap Node snaps to
//! the drawn points after the drawing's own ends, middles and centres. Opening a LAS / LAZ / PTS file (or an XYZ
//! while a drawing is open) attaches it at its own coordinates, as the page's Open and drop do.
//!
//! The picture is the engine's ([`cc_engine::cloud::CloudAttach::draw`], cloud-cad.js `draw`), made again when the
//! view, the pane or the cloud changes, and shown as a texture.

use std::cell::RefCell;
use std::rc::Rc;

use cc_engine::cloud::CloudAttach;
use cc_geom::P2;
use cc_render::View2D;
use egui::{Color32, Pos2, Rect};

use crate::{ImportedFile, UiApp, edit};

thread_local! {
    /// The window's cloud (`host.cloud`: one per window).
    static CLOUD: Rc<RefCell<CloudAttach>> = Rc::default();
    /// The cloud's pictures on screen and what they were made for.
    static TEX: RefCell<(Vec<u64>, Vec<egui::TextureHandle>)> = RefCell::default();
}

/// The window's point cloud.
pub fn cloud() -> Rc<RefCell<CloudAttach>> {
    CLOUD.with(Rc::clone)
}

/// A file the window opens that is a point cloud: LAS, LAZ, PTS, or XYZ while a drawing is open (an XYZ with nothing
/// open is a CivilCraft point file). It goes under the drawing at its own coordinates; a new drawing is made for it
/// when none is open. → true when it was taken.
pub fn open_cloud(app: &mut UiApp, f: &ImportedFile) -> bool {
    let lower = f.name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    let cloud_ext = matches!(ext, "las" | "laz" | "pts") || (ext == "xyz" && app.editor.is_some());
    if !cloud_ext {
        return false;
    }
    if app.editor.is_none() {
        app.run_ui("file.new", &crate::Params::default());
    }
    let Some(ed) = app.editor.as_mut() else { return false };
    ed.eng.open_cloud(cc_engine::PickedFile { name: f.name.clone(), bytes: f.bytes.clone() });
    app.sync_editor();
    true
}

/// The drawing's units as the cloud reads them (`doc.meta.units || ''`).
fn units(ed: &edit::Editor) -> String {
    cc_engine::cmd::cloud::units_of(&ed.eng.doc())
}

/// view.js `snapExtra`: the cloud's drawn point nearest `p` within `r`, when it beats `best` (a cloud point snaps with
/// priority 3: after ends, middles, centres and quadrants, before Nearest).
pub fn snap(ed: &edit::Editor, p: P2, r: f64, best: Option<&cc_cad_geom::pick::Snap>, best_pr: u8) -> Option<cc_cad_geom::pick::Snap> {
    let u = units(ed);
    let q = cloud().borrow_mut().snap(p, r, &u)?;
    let d = (q.x - p.x).hypot(q.y - p.y);
    if d.is_nan() || d > r {
        return None;
    }
    let wins = match best {
        None => true,
        Some(b) => 3 < best_pr || (3 == best_pr && d < (b.x - p.x).hypot(b.y - p.y)),
    };
    wins.then_some(cc_cad_geom::pick::Snap { kind: "cloud", x: q.x, y: q.y, defer: None })
}

/// A model view (physical pixels) → cloud-cad.js's `T` (drawing → CSS px) for a pane of `ppp` pixels per point.
fn t_of(v: &View2D, ppp: f64) -> [f64; 6] {
    let (s, c) = v.twist.sin_cos();
    let z = v.zoom;
    let e = v.width / 2.0 - z * (c * v.cx - s * v.cy);
    let f = v.height / 2.0 + z * (s * v.cx + c * v.cy);
    [z * c / ppp, -z * s / ppp, -z * s / ppp, -z * c / ppp, e / ppp, f / ppp]
}

/// The cloud's pictures for this frame: (texture, where it goes, its clip), in draw order — the whole pane in a
/// model, each viewport's window on a sheet (`vps`: each viewport's model view and its screen rectangle).
pub fn underlay(
    ctx: &egui::Context,
    ed: &edit::Editor,
    rect: Rect,
    ppp: f64,
    model: Option<&View2D>,
    vps: &[(View2D, Rect)],
) -> Vec<(egui::TextureId, Rect, Rect)> {
    let pc = cloud();
    if !pc.borrow().has() || !pc.borrow().visible {
        TEX.with(|t| *t.borrow_mut() = (Vec::new(), Vec::new()));
        return Vec::new();
    }
    let u = units(ed);
    let mut views: Vec<(View2D, Rect)> = model.map(|v| vec![(*v, rect)]).unwrap_or_default();
    views.extend_from_slice(vps);
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
    let version = pc.borrow().version;
    let keys: Vec<u64> = views
        .iter()
        .map(|(v, _)| {
            let mut k: u64 = 0xcbf2_9ce4_8422_2325;
            for x in [v.cx, v.cy, v.zoom, v.twist, v.width, v.height, w, h, ppp] {
                k = (k ^ x.to_bits()).wrapping_mul(0x0100_0000_01b3);
            }
            k = (k ^ version).wrapping_mul(0x0100_0000_01b3);
            for b in u.bytes() {
                k = (k ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
            k
        })
        .collect();
    TEX.with(|t| {
        let mut t = t.borrow_mut();
        if t.0 != keys {
            let mut texs = Vec::new();
            for (i, (v, _)) in views.iter().enumerate() {
                let mut c = pc.borrow_mut();
                let Some(r) = c.draw(t_of(v, ppp), w, h, ppp, &u) else { continue };
                // opaque points or nothing: premultiplied is the colour itself
                let px: Vec<Color32> =
                    r.px.iter()
                        .map(|q| {
                            let [cr, cg, cb, ca] = q.to_le_bytes();
                            Color32::from_rgba_premultiplied(cr, cg, cb, ca)
                        })
                        .collect();
                let img = egui::ColorImage::new([r.w, r.h], px);
                let opts = egui::TextureOptions::NEAREST;
                match t.1.get_mut(i) {
                    Some(h) => {
                        h.set(img, opts);
                        texs.push(h.clone());
                    }
                    None => texs.push(ctx.load_texture(format!("pointcloud:{i}"), img, opts)),
                }
            }
            *t = (keys, texs);
        }
        t.1.iter()
            .zip(&views)
            .map(|(tex, (_, clip))| (tex.id(), Rect::from_min_size(rect.min, rect.size()), clip.intersect(rect)))
            .collect()
    })
}

/// Paint the pictures (canvas.rs calls this under the linework).
pub fn paint(painter: &egui::Painter, items: &[(egui::TextureId, Rect, Rect)]) {
    for (tex, at, clip) in items {
        let p = painter.with_clip_rect(*clip);
        p.image(*tex, *at, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
}

/// [`underlay`] for the space shown: the model's pane, or each (turned-on) viewport of a sheet through its window.
pub fn underlay_for(
    ctx: &egui::Context,
    ed: &edit::Editor,
    rect: Rect,
    ppp: f64,
    drawing: &cc_drawing::Drawing,
    space: usize,
    view: &View2D,
) -> Vec<(egui::TextureId, Rect, Rect)> {
    if !cloud().borrow().has() {
        return Vec::new();
    }
    let Some(sp) = drawing.spaces.get(space) else { return Vec::new() };
    if sp.kind == cc_drawing::SpaceKind::Model {
        return underlay(ctx, ed, rect, ppp, Some(view), &[]);
    }
    let pt = ppp as f32;
    let vps: Vec<(View2D, Rect)> = sp
        .viewports
        .iter()
        .map(|vp| {
            let corners = [
                view.to_screen(vp.cx - vp.w / 2.0, vp.cy - vp.h / 2.0),
                view.to_screen(vp.cx + vp.w / 2.0, vp.cy + vp.h / 2.0),
                view.to_screen(vp.cx - vp.w / 2.0, vp.cy + vp.h / 2.0),
                view.to_screen(vp.cx + vp.w / 2.0, vp.cy - vp.h / 2.0),
            ];
            let pts: Vec<Pos2> = corners.iter().map(|(x, y)| Pos2::new(rect.min.x + *x as f32 / pt, rect.min.y + *y as f32 / pt)).collect();
            (crate::canvas::viewport_view(view, vp), Rect::from_points(&pts))
        })
        .collect();
    underlay(ctx, ed, rect, ppp, None, &vps)
}

/// The cloud for CadCraft's 3D tab (ws3d.js `cloudChanged`): placed by `matrix3d` in metres, its slice as a band of
/// the same height, its points `size` CSS pixels; None when there is none or it is hidden.
pub fn layer3d(ed: &edit::Editor, ppp: f64) -> Option<crate::view3d::CloudLayer> {
    let pc = cloud();
    let pc = pc.borrow();
    if !pc.visible {
        return None;
    }
    let (c, colors) = (pc.cloud.clone()?, pc.colors.clone()?);
    let u = units(ed);
    let k = cc_engine::cloud::m_per_unit(&u);
    let model = pc.matrix3d(k, &u)?;
    let (h, thick) = pc.slice_spec(&u);
    let slice = pc.slice.on.then_some((h * k, thick * k / 2.0));
    Some(crate::view3d::CloudLayer { cloud: c, colors, draw: cc_render::points3::CloudDraw { model, size: pc.size * ppp, slice } })
}

/// The camera's far plane takes the cloud in (ws3d.js: "a point cloud must not fall outside the clip range").
pub fn reach(cam: &mut cc_render::mesh3d::Cam3, c: &crate::view3d::CloudLayer) {
    let m = &c.draw.model;
    let cl = &c.cloud;
    let mut far = cam.far;
    for &x in &[0.0, cl.max[0] - cl.origin[0]] {
        for &y in &[0.0, cl.max[1] - cl.origin[1]] {
            for &z in &[0.0, cl.max[2] - cl.origin[2]] {
                let p = [m[0] * x + m[12], m[5] * y + m[13], m[10] * z + m[14]];
                let d = ((p[0] - cam.eye[0]).powi(2) + (p[1] - cam.eye[1]).powi(2) + (p[2] - cam.eye[2]).powi(2)).sqrt();
                if d.is_finite() {
                    far = far.max(d * 1.1);
                }
            }
        }
    }
    cam.far = far;
}
