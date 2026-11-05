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

/// A pane's overscan in use (key without the centre, it, its texture) and the last frame's key.
type OverSlot = (Option<(u64, Over, egui::TextureHandle)>, Option<u64>);

thread_local! {
    /// The window's cloud (`host.cloud`: one per window).
    static CLOUD: Rc<RefCell<CloudAttach>> = Rc::default();
    /// The cloud's pictures on screen and what they were made for, per pane (0: the active one).
    static TEX: RefCell<Vec<(Vec<u64>, Vec<egui::TextureHandle>)>> = RefCell::default();
    /// Per pane: the overscan in use (what it was made for but the centre, it, its texture) and the last frame's
    /// view but the centre (a frame with the same one and another centre is a pan).
    static OVER: RefCell<Vec<OverSlot>> = RefCell::default();
    /// What a drag's pan has left below a whole device pixel (the pan moves in whole pixels while a cloud shows).
    static PAN_REST: std::cell::Cell<(f64, f64)> = const { std::cell::Cell::new((0.0, 0.0)) };
}

/// A drag's pan in device pixels as the view takes it while a cloud shows: whole pixels, the rest kept for the next
/// frame (the overscan can then show the moved picture, provably the redraw); without a cloud, as it came.
pub fn pan_steps(dx: f64, dy: f64) -> (f64, f64) {
    if !cloud().borrow().has() {
        return (dx, dy);
    }
    let (rx, ry) = PAN_REST.with(std::cell::Cell::get);
    let (tx, ty) = (rx + dx, ry + dy);
    let (sx, sy) = (tx.round(), ty.round());
    PAN_REST.with(|c| c.set((tx - sx, ty - sy)));
    (sx, sy)
}

/// The drag ended: nothing is carried to the next.
pub fn pan_end() {
    PAN_REST.with(|c| c.set((0.0, 0.0)));
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
    slot: usize,
) -> Vec<(egui::TextureId, Rect, Rect)> {
    let pc = cloud();
    if !pc.borrow().has() || !pc.borrow().visible {
        TEX.with(|t| t.borrow_mut().clear());
        OVER.with(|t| t.borrow_mut().clear());
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
    // one model pane panning: the overscan, moved, when it is provably the redraw (M4.62)
    if let (Some(v), true) = (model, vps.is_empty()) {
        let mut k: u64 = 0xcbf2_9ce4_8422_2325;
        for x in [v.zoom, v.twist, v.width, v.height, w, h, ppp] {
            k = (k ^ x.to_bits()).wrapping_mul(0x0100_0000_01b3);
        }
        k = (k ^ version).wrapping_mul(0x0100_0000_01b3);
        for b in u.bytes() {
            k = (k ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
        let t = t_of(v, ppp);
        let max_side = ctx.input(|i| i.max_texture_side);
        let shown = OVER.with(|all| {
            let mut all = all.borrow_mut();
            while all.len() <= slot {
                all.push((None, None));
            }
            let entry = all.get_mut(slot)?;
            let mut c = pc.borrow_mut();
            let prev = entry.1.replace(k);
            if let Some((base, o, tex)) = &entry.0
                && *base == k
                && let Some((x, y)) = shift_of(o, &mut c, t, w, h, ppp, &u)
            {
                return Some((tex.id(), over_rect(rect, o, x, y)));
            }
            entry.0 = None;
            // a pan (the last frame had this view but its centre): make the overscan
            let vk = keys_of_view(v, w, h, ppp, version, &u);
            let moved = TEX.with(|tx| tx.borrow().get(slot).is_some_and(|(keys, _)| keys.len() == 1 && *keys != vk));
            if prev != Some(k) || !moved {
                return None;
            }
            let o = build_over(&mut c, t, w, h, ppp, &u)?;
            if o.w > max_side || o.h > max_side {
                return None;
            }
            let (x, y) = shift_of(&o, &mut c, t, w, h, ppp, &u)?;
            let px: Vec<Color32> =
                o.px.iter()
                    .map(|q| {
                        let [cr, cg, cb, ca] = q.to_le_bytes();
                        Color32::from_rgba_premultiplied(cr, cg, cb, ca)
                    })
                    .collect();
            let tex =
                ctx.load_texture(format!("pointcloud-over:{slot}"), egui::ColorImage::new([o.w, o.h], px), egui::TextureOptions::NEAREST);
            let at = over_rect(rect, &o, x, y);
            let id = tex.id();
            entry.0 = Some((k, o, tex));
            Some((id, at))
        });
        if let Some((id, at)) = shown {
            return vec![(id, at, rect)];
        }
    }
    TEX.with(|all| {
        let mut all = all.borrow_mut();
        while all.len() <= slot {
            all.push((Vec::new(), Vec::new()));
        }
        let Some(t) = all.get_mut(slot) else { return Vec::new() };
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
                    None => texs.push(ctx.load_texture(format!("pointcloud:{slot}:{i}"), img, opts)),
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

/// One view's key for the picture cache (as `underlay` makes them).
fn keys_of_view(v: &View2D, w: f64, h: f64, ppp: f64, version: u64, u: &str) -> Vec<u64> {
    let mut k: u64 = 0xcbf2_9ce4_8422_2325;
    for x in [v.cx, v.cy, v.zoom, v.twist, v.width, v.height, w, h, ppp] {
        k = (k ^ x.to_bits()).wrapping_mul(0x0100_0000_01b3);
    }
    k = (k ^ version).wrapping_mul(0x0100_0000_01b3);
    for b in u.bytes() {
        k = (k ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    vec![k]
}

/// Where the overscan's texture goes so that its pane part at (x, y) lies on the pane at the full picture's scale.
fn over_rect(rect: Rect, o: &Over, x: usize, y: usize) -> Rect {
    let (kx, ky) = (rect.width() / o.pw as f32, rect.height() / o.ph as f32);
    Rect::from_min_size(rect.min - egui::vec2(x as f32 * kx, y as f32 * ky), egui::vec2(o.w as f32 * kx, o.h as f32 * ky))
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
    underlay_slot(ctx, ed, rect, ppp, drawing, space, view, 0)
}

/// Pane `slot`'s pictures are kept apart (a split screen: 0 the active pane, then the others).
pub fn keep_slots(n: usize) {
    TEX.with(|t| t.borrow_mut().truncate(n));
    OVER.with(|t| t.borrow_mut().truncate(n));
}

/// [`underlay_for`] for pane `slot` of a split screen.
#[allow(clippy::too_many_arguments)]
pub fn underlay_slot(
    ctx: &egui::Context,
    ed: &edit::Editor,
    rect: Rect,
    ppp: f64,
    drawing: &cc_drawing::Drawing,
    space: usize,
    view: &View2D,
    slot: usize,
) -> Vec<(egui::TextureId, Rect, Rect)> {
    if !cloud().borrow().has() {
        return Vec::new();
    }
    let Some(sp) = drawing.spaces.get(space) else { return Vec::new() };
    if sp.kind == cc_drawing::SpaceKind::Model {
        return underlay(ctx, ed, rect, ppp, Some(view), &[], slot);
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
    underlay(ctx, ed, rect, ppp, None, &vps, slot)
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

// ---------------------------------------------------------------------------------------------- overscan (M4.62)
//
// While the model pane pans, the picture is not made again for every frame: one picture of the pane with a margin
// round it (the overscan) is made when a pan starts, and each later frame of the pan shows it moved — but only
// when that is provably the picture a full redraw would make. The engine places a point at
// `round(a·x + c·y + tx)` (and the same in y); a pan changes only `tx`, `ty`. For a pan of a whole number of device
// pixels S (the window pans in whole pixels while dragging), the new `tx'` is `tx − S + δ` with δ a rounding
// residue, so a point lands S pixels over unless its value lies within |δ| (plus the last bit of the add) of a
// half-pixel, where `round` flips. The overscan records, for every point that touches it, how far its value is from
// the nearest half-pixel: points nearer than [`RISK`] are kept in a list and placed again exactly (the engine's
// expression) on every frame; for all the others the smallest distance σ must exceed |δ| + the rounding of the add.
// The highest point still wins each pixel (the same points, in the same order, on the same pixels), so the moved
// picture is the full redraw's pixel for pixel. Anything else (a zoom, another cloud setting, a pan past the margin,
// a δ too large, a risky point that lands elsewhere) draws the picture again as before.

/// Device pixels of margin on each side of the overscanned picture.
pub const MARGIN: usize = 256;
/// Points whose value is nearer than this to a half-pixel are placed again on every frame.
pub const RISK: f64 = 1e-6;
/// At most this many such points (more: the picture is drawn again instead).
const RISK_CAP: usize = 200_000;

/// The engine's placement for one picture (cloud.rs `draw`'s `Proj`), recomputed with the same expressions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub e: f64,
    pub tx: f64,
    pub ty: f64,
    pub h0: i64,
    pub s: i64,
}

/// cloud.rs `draw`'s projection for `t` (drawing → CSS px), `d` device px per CSS px.
pub fn place_of(c: &CloudAttach, t: [f64; 6], d: f64, units: &str) -> Option<Place> {
    let cl = c.cloud.as_ref()?;
    let k = c.scale(units);
    let (ox, oy) = match c.insert {
        Some(i) => (i.x + (cl.origin[0] - cl.min[0]) * k, i.y + (cl.origin[1] - cl.min[1]) * k),
        None => (cl.origin[0] * k, cl.origin[1] * k),
    };
    let s = cc_cad::js::max(1.0, cc_cad::js::round(c.size * d * 0.75));
    if s.is_nan() || s > 512.0 {
        return None;
    }
    Some(Place {
        a: d * t[0] * k,
        b: d * t[1] * k,
        c: d * t[2] * k,
        e: d * t[3] * k,
        tx: d * (t[0] * ox + t[2] * oy + t[4]),
        ty: d * (t[1] * ox + t[3] * oy + t[5]),
        h0: (s as i64) >> 1,
        s: s as i64,
    })
}

/// A point's pixel (the square's corner) as the engine computes it, or None when it is not a number.
fn pixel(p: &Place, x: f64, y: f64) -> Option<(i64, i64)> {
    let fx = cc_cad::js::round(p.a * x + p.c * y + p.tx);
    let fy = cc_cad::js::round(p.b * x + p.e * y + p.ty);
    if !(fx.is_finite() && fy.is_finite() && fx.abs() <= 1e15 && fy.abs() <= 1e15) {
        return None;
    }
    Some(((fx as i64).saturating_sub(p.h0), (fy as i64).saturating_sub(p.h0)))
}

/// How far a value is from where `round` flips (a half-pixel), less its last bit.
fn margin_of(u: f64) -> f64 {
    let tie = u.floor() + 0.5;
    (u - tie).abs() - ulp(u)
}

fn ulp(u: f64) -> f64 {
    let a = u.abs();
    if !a.is_finite() {
        return f64::INFINITY;
    }
    f64::from_bits(a.to_bits().saturating_add(1)) - a
}

/// An overscanned picture and what proves a moved copy of it.
#[derive(Clone, Debug)]
pub struct Over {
    pub place: Place,
    /// the picture: the pane plus [`MARGIN`] device pixels each side
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
    /// the pane's picture size (device px)
    pub pw: usize,
    pub ph: usize,
    /// points near a half-pixel: (index, corner x, corner y) in the overscan
    pub risky: Vec<(usize, i64, i64)>,
    /// the smallest distance to a half-pixel of every other point that touches the overscan
    pub sigma: f64,
}

/// The overscan for a pane drawn with `t` (CSS px `w` × `h`, `d` device px per CSS px).
pub fn build_over(c: &mut CloudAttach, t: [f64; 6], w: f64, h: f64, d: f64, units: &str) -> Option<Over> {
    let m = MARGIN as f64 / d;
    let mut to = t;
    to[4] += m;
    to[5] += m;
    let (ow, oh) = (w + 2.0 * m, h + 2.0 * m);
    let place = place_of(c, to, d, units)?;
    let r = c.draw(to, ow, oh, d, units)?;
    let (rw, rh, px) = (r.w, r.h, r.px.clone());
    let pw = cc_cad::js::max(1.0, (w * d).ceil()) as usize;
    let ph = cc_cad::js::max(1.0, (h * d).ceil()) as usize;
    let cl = c.cloud.clone()?;
    let shown = c.shown(units);
    let n = shown.as_ref().map_or(cl.n, |v| v.len());
    let (wi, hi) = (rw as i64, rh as i64);
    let mut risky = Vec::new();
    let mut sigma = f64::INFINITY;
    for k in 0..n {
        let i = match &shown {
            Some(v) => v.get(k).map_or(usize::MAX, |x| *x as usize),
            None => k,
        };
        let Some(&[x, y, _]) = i.checked_mul(3).and_then(|o| cl.pos.get(o..o.saturating_add(3))) else { continue };
        let (x, y) = (f64::from(x), f64::from(y));
        let Some((sx, sy)) = pixel(&place, x, y) else { continue };
        // only points whose square reaches the overscan matter (the others stay off the pane)
        if sx.saturating_add(place.s) <= 0 || sy.saturating_add(place.s) <= 0 || sx >= wi || sy >= hi {
            continue;
        }
        let mu = margin_of(place.a * x + place.c * y + place.tx).min(margin_of(place.b * x + place.e * y + place.ty));
        if mu < RISK {
            if risky.len() >= RISK_CAP {
                return None;
            }
            risky.push((i, sx, sy));
        } else {
            sigma = sigma.min(mu);
        }
    }
    Some(Over { place, w: rw, h: rh, px, pw, ph, risky, sigma })
}

/// Where the pane's picture for `t` sits in the overscan (its top-left, device px) when the moved overscan is
/// provably that picture; None to draw it again.
pub fn shift_of(o: &Over, c: &mut CloudAttach, t: [f64; 6], w: f64, h: f64, d: f64, units: &str) -> Option<(usize, usize)> {
    let p = place_of(c, t, d, units)?;
    let q = &o.place;
    // only a pan: the same scale, turn, point size and pane
    let same = p.a.to_bits() == q.a.to_bits()
        && p.b.to_bits() == q.b.to_bits()
        && p.c.to_bits() == q.c.to_bits()
        && p.e.to_bits() == q.e.to_bits()
        && p.s == q.s;
    let pw = cc_cad::js::max(1.0, (w * d).ceil()) as usize;
    let ph = cc_cad::js::max(1.0, (h * d).ceil()) as usize;
    if !same || pw != o.pw || ph != o.ph {
        return None;
    }
    let sx = (q.tx - p.tx).round();
    let sy = (q.ty - p.ty).round();
    // the pane inside the overscan with 2 px to spare (a point off it by one pixel stays off the pane)
    let fits = |s: f64, pane: usize, over: usize| s >= 2.0 && s + pane as f64 + 2.0 <= over as f64;
    if !(fits(sx, pw, o.w) && fits(sy, ph, o.h)) {
        return None;
    }
    // δ: what the pan left over beyond whole pixels, with the bits the two sums can lose
    let dx = ((p.tx + sx) - q.tx).abs() + 2.0 * (ulp(p.tx) + ulp(q.tx) + ulp(p.tx + sx));
    let dy = ((p.ty + sy) - q.ty).abs() + 2.0 * (ulp(p.ty) + ulp(q.ty) + ulp(p.ty + sy));
    // the add of the value in the pane can lose a last bit of its own (values under 2^20: 2^-32)
    let slack = 2f64.powi(-30);
    let need = dx.max(dy) + slack;
    if need.is_nan() || need >= o.sigma.min(RISK) {
        return None;
    }
    let cl = c.cloud.clone()?;
    let (si, sj) = (sx as i64, sy as i64);
    for &(i, ox, oy) in &o.risky {
        let Some(&[x, y, _]) = i.checked_mul(3).and_then(|k| cl.pos.get(k..k.saturating_add(3))) else { return None };
        if pixel(&p, f64::from(x), f64::from(y)) != Some((ox - si, oy - sj)) {
            return None;
        }
    }
    Some((sx as usize, sy as usize))
}

/// The pane's picture cut out of the overscan at `(x, y)` (the probe compares it with a full redraw).
pub fn crop(o: &Over, x: usize, y: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(o.pw.saturating_mul(o.ph));
    for r in y..y.saturating_add(o.ph) {
        let a = r.saturating_mul(o.w).saturating_add(x);
        out.extend_from_slice(o.px.get(a..a.saturating_add(o.pw)).unwrap_or(&[]));
    }
    out
}

#[cfg(test)]
mod overscan_tests {
    use super::*;
    use cc_io_civil::pointcloud as pcl;
    use cc_io_civil::raster::InFile;

    fn attach(name: &str, bytes: Vec<u8>) -> CloudAttach {
        let tr = cc_cad::i18n::catalog();
        let f = InFile { name: name.into(), bytes };
        let info = pcl::read_cloud_info(&tr, &f).unwrap();
        let cloud = pcl::load_cloud(&tr, &f, &info, 2e6).unwrap();
        let mut a = CloudAttach::default();
        a.attach(cloud, None);
        a
    }

    /// Random whole-pixel pans (the window's drag) from a fitted view: every frame the moved overscan is used it must
    /// be the full redraw, pixel for pixel. → (frames, frames shown moved, overscans made).
    fn pans(a: &mut CloudAttach, zooms: &[f64], n: usize, seed: u64, ppp: f64) -> (usize, usize, usize) {
        pans_timed(a, zooms, n, seed, ppp).0
    }

    /// [`pans`] and the time (ms) of the full redraws and of the moved frames' checks.
    fn pans_timed(a: &mut CloudAttach, zooms: &[f64], n: usize, seed: u64, ppp: f64) -> ((usize, usize, usize), f64, f64) {
        let (mut t_full, mut t_moved) = (0.0, 0.0);
        let b = a.bbox("").unwrap();
        let (w, h) = (900.0_f64, 600.0_f64);
        let mut v = View2D { cx: (b[0] + b[2]) / 2.0, cy: (b[1] + b[3]) / 2.0, zoom: 1.0, twist: 0.0, width: w * ppp, height: h * ppp };
        let z0 = w * ppp / (b[2] - b[0]) * 0.8;
        let mut s = seed;
        let mut rnd = move || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 11) as f64 / (1u64 << 53) as f64
        };
        let (mut frames, mut moved, mut built) = (0, 0, 0);
        for &zk in zooms {
            v.zoom = z0 * zk;
            let mut over: Option<Over> = None;
            for _ in 0..n {
                let (dx, dy) = ((rnd() * 60.0 - 30.0).round(), (rnd() * 60.0 - 30.0).round());
                v.pan(dx, dy);
                frames += 1;
                let t = t_of(&v, ppp);
                let c0 = std::time::Instant::now();
                let full = a.draw(t, w, h, ppp, "").unwrap().px.clone();
                t_full += c0.elapsed().as_secs_f64() * 1000.0;
                let c1 = std::time::Instant::now();
                let at = over.as_ref().and_then(|o| shift_of(o, a, t, w, h, ppp, ""));
                if at.is_some() {
                    t_moved += c1.elapsed().as_secs_f64() * 1000.0;
                }
                let hit = over.as_ref().zip(at).map(|(o, (x, y))| (crop(o, x, y), x, y));
                match hit {
                    Some((pic, x, y)) => {
                        moved += 1;
                        let bad = pic.iter().zip(&full).filter(|(p, q)| p != q).count();
                        assert!(bad == 0 && pic.len() == full.len(), "moved overscan ({x}, {y}) differs from the redraw: {bad} pixels");
                    }
                    None => {
                        over = build_over(a, t, w, h, ppp, "");
                        built += 1;
                        if let Some(o) = &over {
                            let (x, y) = shift_of(o, a, t, w, h, ppp, "").expect("a fresh overscan holds its own pane");
                            assert_eq!(crop(o, x, y), full, "the fresh overscan's pane is the redraw");
                        }
                    }
                }
            }
        }
        ((frames, moved, built), t_full, t_moved)
    }

    #[test]
    fn moved_overscan_is_the_redraw() {
        // a grid of points on quarter and half units: many values land exactly on half-pixels (the risky list)
        let mut xyz = String::new();
        for i in 0..120 {
            for j in 0..80 {
                xyz.push_str(&format!("{} {} {}\n", f64::from(i) * 0.25, f64::from(j) * 0.5 + 0.125, (i * 7 + j * 3) % 11));
            }
        }
        for ppp in [1.0, 1.25, 2.0] {
            let mut a = attach("grid.xyz", xyz.clone().into_bytes());
            let (frames, moved, built) = pans(&mut a, &[1.0, 2.0, 4.0], 60, 7, ppp);
            assert!(moved > frames / 2, "most frames moved: {frames} {moved} {built}");
        }
    }

    /// The probe over the room scans (testfiles/generated/pointcloud-room): thousands of pans.
    #[test]
    #[ignore]
    fn overscan_probe() {
        let dir = std::env::var("PROBE_DIR").unwrap_or_else(|_| "/var/www/CADCRAFT/testfiles/generated/pointcloud-room".into());
        let n: usize = std::env::var("PROBE_PANS").ok().and_then(|v| v.parse().ok()).unwrap_or(400);
        let mut all = (0, 0, 0);
        for f in [
            "room-f0-12-nocrs.las",
            "room-f2-12.las",
            "room-f3-12.las",
            "room-f7-14.las",
            "room-f8-14.las",
            "room.xyz",
            "../pointcloud/big-90mb-3000000.laz",
        ] {
            let Ok(bytes) = std::fs::read(format!("{dir}/{f}")) else { continue };
            let mut a = attach(f, bytes);
            for (ppp, seed) in [(1.0, 1u64), (2.0, 2), (1.5, 3)] {
                let t0 = std::time::Instant::now();
                let (r, tf, tm) = pans_timed(&mut a, &[1.0, 1.7, 3.0, 6.0], n, seed, ppp);
                let pts = a.cloud.as_ref().map_or(0, |c| c.n);
                println!(
                    "{f} ({pts} points) ppp {ppp}: {} frames, {} moved, {} overscans ({:.0} ms); a redraw {:.3} ms, a moved frame's proof {:.4} ms",
                    r.0,
                    r.1,
                    r.2,
                    t0.elapsed().as_secs_f64() * 1000.0,
                    tf / r.0.max(1) as f64,
                    tm / r.1.max(1) as f64
                );
                all = (all.0 + r.0, all.1 + r.1, all.2 + r.2);
            }
        }
        println!("all: {} frames, {} shown moved (each the redraw pixel for pixel), {} overscans", all.0, all.1, all.2);
    }
}
