//! The drawing's model space in 3D (BimCraft 0.5's DWG view: the "3D" switch, key 3; dwg-compile3d.js batches drawn as
//! dwg-view3d.js draws them, on cc-render's 3D path), picking objects for their properties (dwg-props.js: a click picks,
//! Ctrl / Cmd + click adds or takes out, Esc or a click on nothing goes back), the standard views, perspective or
//! parallel, extents, and the section plane (Top / Front / Side, offset, flip; key X).
//!
//! As dwg-view3d.js: the geometry is built round the drawing's centre and scaled so its extents are 100 units across —
//! survey or millimetre coordinates keep their precision (the centre is taken off in f64 before anything becomes f32,
//! and the renderer draws relative to the camera) and ModelCraft's camera limits hold. Faces in their batch colour a
//! little toward the background, lines in their layer colour (coloured edges), both sides lit alike. Navigation:
//! left- or middle-drag orbits (Shift pans), right-drag pans, the wheel zooms at the pointer (toward the face under
//! it), a pinch zooms, two fingers pan. Text and the pick's highlight are drawn over the view (the highlight over
//! everything, as the JS draws it without depth; text is not hidden behind faces here — a known difference).

use std::collections::HashSet;
use std::sync::Arc;

use cc_drawing::Color;
use cc_dwgview::compile3d::{Model3, compile_model3d};
use cc_dwgview::page;
use cc_m3d_tools::camera::{Box3, Camera};
use cc_render::mesh3d::{Cam3, Instance3, MeshData, Style3};
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke};

use crate::lang;
use crate::view3d::Scene3D;
use crate::{Params, UiApp};

pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("viewer.3d", "3D", "3", "{} → model space in 3D (again: back to 2D)"),
    (
        "viewer.3d.view",
        "3D view",
        "",
        "{name: iso|top|bottom|front|back|left|right|persp|extents|zoom:F} → a standard view, perspective on / off, the whole drawing, or a zoom by F at the middle",
    ),
    (
        "viewer.3d.pick",
        "Pick in 3D",
        "",
        "{index?: object (model-space entity number), name?: \"add\" | \"N\" | \"add:N\"} → Properties shows it; add = Ctrl+click; none: back",
    ),
    ("viewer.3d.orbit", "Orbit the 3D view", "", "{factor: pixels across, index?: pixels down} → turns the view as a drag does"),
    (
        "viewer.3d.section",
        "Section plane",
        "X",
        "{name?: top|front|side|flip|off, factor?: offset in drawing units} → cuts the drawing in 3D (no name: on / off)",
    ),
];

/// What dwg-view3d.js `SCENE` is: the drawing's extents in scene units.
const SCENE: f64 = 100.0;
const PICK: Color32 = Color32::from_rgb(0x2f, 0x6f, 0xd6);

/// The section plane: axis, offset (drawing units from the bottom / front / side), flip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sec {
    pub axis: &'static str,
    pub offset: f64,
    pub flip: bool,
}

struct Built {
    doc: u64,
    model: Arc<Model3>,
    origin: [f64; 3],
    scale: f64,
    /// scene-space bounds (None: nothing to show)
    bx: Option<Box3>,
    scene: Scene3D,
    /// the layer per mesh (instances hide by layer)
    layers: Vec<String>,
    dark: bool,
}

/// The 3D view's state.
#[derive(Default)]
pub struct Dwg3d {
    pub on: bool,
    built: Option<Built>,
    pub cam: Camera,
    /// picked objects (model-space entity numbers), in pick order
    pub picked: Vec<usize>,
    pub section: Option<Sec>,
    seq: u64,
    /// the hidden layers the instances were made for
    hidden_for: Option<Vec<String>>,
    /// the Iso view fitted again once the view's size is known (setModel's standard('iso'))
    fit_pending: bool,
    /// Properties for the pick, worked out once per pick (doc id, picked, language)
    view_cache: std::cell::RefCell<Option<ViewCache>>,
}

type ViewCache = ((u64, Vec<usize>, cc_lang::Locale), Option<page::PickView>);

impl std::fmt::Debug for Dwg3d {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dwg3d").field("on", &self.on).field("picked", &self.picked).finish()
    }
}

/// The 3D view shows.
pub fn showing(app: &UiApp) -> bool {
    app.viewer.d3.on && crate::viewer::active(app)
}

/// Back to the 2D view (a sheet, Model, the equipment list; a new drawing).
pub fn leave(app: &mut UiApp) {
    let d = &mut app.viewer.d3;
    d.on = false;
    d.section = None;
    d.picked.clear();
}

/// The compiled model of the open drawing (compiled once, on the first switch to 3D).
pub fn model(app: &UiApp) -> Option<Arc<Model3>> {
    let d = app.doc.as_ref()?;
    app.viewer.d3.built.as_ref().filter(|b| b.doc == d.id).map(|b| Arc::clone(&b.model))
}

fn hex(c: Color) -> [u8; 3] {
    match c {
        Color::Rgb(n) => [(n >> 16) as u8, (n >> 8) as u8, n as u8],
        Color::Ink => [0, 0, 0],
    }
}
fn lum(c: [u8; 3]) -> f64 {
    let f = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
}
fn mix(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    let m = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}
/// dwg-view3d.js `color(c, kind)`: ink from the theme; pale colours darkened on a light screen, dark ones lightened on
/// a dark one; faces a little toward the background.
fn color3(c: Color, face: bool, bg: Color32, ink: Color32) -> [u8; 4] {
    let bgc = [bg.r(), bg.g(), bg.b()];
    let dark = lum(bgc) < 0.4;
    let v = match c {
        Color::Ink if face => {
            if dark {
                [0x9a, 0xa4, 0xb0]
            } else {
                [0xc3, 0xca, 0xd2]
            }
        }
        Color::Ink => [ink.r(), ink.g(), ink.b()],
        Color::Rgb(_) => {
            let c = hex(c);
            let l = lum(c);
            let v = if !dark && l > 0.6 {
                mix(c, [0, 0, 0], 0.45)
            } else if dark && l < 0.12 {
                mix(c, [255, 255, 255], 0.45)
            } else {
                c
            };
            if face { mix(v, bgc, 0.3) } else { v }
        }
    };
    [v[0], v[1], v[2], 255]
}

fn build(m: Arc<Model3>, doc: u64, id: u64, bg: Color32, ink: Color32) -> Built {
    let has = m.has_bounds();
    let c = if has { [(m.min[0] + m.max[0]) / 2.0, (m.min[1] + m.max[1]) / 2.0, (m.min[2] + m.max[2]) / 2.0] } else { [0.0; 3] };
    let span = if has { (m.max[0] - m.min[0]).max(m.max[1] - m.min[1]).max(m.max[2] - m.min[2]) } else { 0.0 };
    let span = if span == 0.0 || span.is_nan() { 1.0 } else { span };
    let s = SCENE / span;
    let to = |v: &[f64]| -> Vec<f64> {
        v.as_chunks::<3>().0.iter().flat_map(|p| [(p[0] - c[0]) * s, (p[1] - c[1]) * s, (p[2] - c[2]) * s]).collect()
    };
    let mut meshes = Vec::new();
    let mut layers = Vec::new();
    for b in &m.batches {
        if b.tris.is_empty() && b.lines.is_empty() {
            continue;
        }
        let fc = color3(b.color, true, bg, ink);
        let lc = color3(b.color, false, bg, ink);
        let mut md = MeshData::from_triangles(&to(&b.tris), &vec![fc; b.tris.len() / 9], &to(&b.lines));
        md.edge_colors = vec![lc; md.edges.len()];
        meshes.push(md);
        layers.push(b.layer.clone());
    }
    let instances: Vec<Instance3> =
        (0..meshes.len()).map(|i| Instance3 { mesh: i, m: cc_m3d::vec::IDENT, tint: None, hidden: false }).collect();
    let (lo, hi) = if has {
        (
            [(m.min[0] - c[0]) * s, (m.min[1] - c[1]) * s, (m.min[2] - c[2]) * s],
            [(m.max[0] - c[0]) * s, (m.max[1] - c[1]) * s, (m.max[2] - c[2]) * s],
        )
    } else {
        ([0.0; 3], [0.0; 3])
    };
    let scene =
        Scene3D::new(id, meshes, instances, if has { lo } else { [f64::INFINITY; 3] }, if has { hi } else { [f64::NEG_INFINITY; 3] });
    let bg_dark = lum([bg.r(), bg.g(), bg.b()]) < 0.4;
    Built { doc, model: m, origin: c, scale: s, bx: has.then_some(Box3 { min: lo, max: hi }), scene, layers, dark: bg_dark }
}

/// Switch to 3D (compiling the drawing on the first switch), or back.
fn toggle(app: &mut UiApp) -> Result<(), String> {
    if !crate::viewer::active(app) {
        return Err("viewer.3d: open a drawing first".into());
    }
    if app.viewer.d3.on {
        leave(app);
        return Ok(());
    }
    ensure_built(app);
    app.viewer.equip.on = false;
    let d3 = &mut app.viewer.d3;
    d3.on = true;
    d3.picked.clear();
    d3.section = None;
    app.viewer.info = true;
    if let Some(m) = model(app) {
        let (s, _) = page::shown_status(&m);
        app.message = s;
    }
    Ok(())
}

fn ensure_built(app: &mut UiApp) {
    let Some(d) = app.doc.as_ref() else { return };
    let dark = app.preset == crate::theme::Preset::Dark;
    let (bg, ink) = (app.tokens.canvas_bg, app.tokens.ink);
    let need = !app.viewer.d3.built.as_ref().is_some_and(|b| b.doc == d.id);
    if need {
        let Some(mdl) = d.model.as_ref() else { return };
        let m = Arc::new(compile_model3d(mdl, &d.xref_models));
        let d3 = &mut app.viewer.d3;
        d3.seq = d3.seq.wrapping_add(1);
        let b = build(m, d.id, 0x0d3d_0000_0000 ^ d3.seq, bg, ink);
        d3.cam = Camera::new();
        if let Some(bx) = &b.bx {
            d3.cam.standard("iso", Some(bx));
        }
        d3.fit_pending = true;
        d3.built = Some(b);
        d3.hidden_for = None;
    } else if app.viewer.d3.built.as_ref().is_some_and(|b| b.dark != dark) {
        // the theme changed: colours again (same model)
        let d3 = &mut app.viewer.d3;
        if let Some(old) = d3.built.take() {
            d3.seq = d3.seq.wrapping_add(1);
            d3.built = Some(build(old.model, old.doc, 0x0d3d_0000_0000 ^ d3.seq, bg, ink));
            d3.hidden_for = None;
        }
    }
    // layers off in the 2D view are off here too (syncLayers)
    let mut hidden: Vec<String> = app.hidden.iter().cloned().collect();
    hidden.sort();
    let d3 = &mut app.viewer.d3;
    if d3.hidden_for.as_ref() != Some(&hidden)
        && let Some(b) = d3.built.as_mut()
    {
        let set: HashSet<&String> = hidden.iter().collect();
        let layers = b.layers.clone();
        b.scene.set_hidden(&|i| layers.get(i).is_some_and(|l| set.contains(l)));
        d3.hidden_for = Some(hidden);
    }
}

fn layer_hidden(app: &UiApp, layer: &str) -> bool {
    app.hidden.contains(layer)
}

/// `SECTION`: which coordinate and which side Top / Front / Side keep.
fn sec_axis(axis: &str) -> (usize, f64) {
    match axis {
        "front" => (1, 1.0),
        "side" => (0, -1.0),
        _ => (2, -1.0),
    }
}
/// The plane in scene space: (axis index, the kept side's sign, where) for a section on this box.
fn sec_plane(sec: &Sec, bx: &Box3, scale: f64) -> (usize, f64, f64) {
    let (i, keep) = sec_axis(sec.axis);
    let (lo, hi) = (ax(&bx.min, i), ax(&bx.max, i));
    let at = lo + (sec.offset * scale).clamp(0.0, (hi - lo).max(0.0));
    let sign = if sec.flip { -1.0 } else { 1.0 } * keep;
    (i, sign, at)
}
/// `sectionRange(axis)`: [0, size] in drawing units.
fn sec_max(b: &Built, axis: &str) -> f64 {
    let Some(bx) = &b.bx else { return 0.0 };
    let (i, _) = sec_axis(axis);
    (ax(&bx.max, i) - ax(&bx.min, i)) / b.scale
}
/// `keeps(p)`.
fn keeps(app: &UiApp, b: &Built, p: [f64; 3]) -> bool {
    match (&app.viewer.d3.section, &b.bx) {
        (Some(sec), Some(bx)) => {
            let (i, sign, at) = sec_plane(sec, bx, b.scale);
            sign * (ax(&p, i) - at) >= -1e-6
        }
        _ => true,
    }
}

/// `p[i]` for an axis number (0 when out of range).
fn ax(p: &[f64; 3], i: usize) -> f64 {
    p.get(i).copied().unwrap_or(0.0)
}
fn set_ax(p: &mut [f64; 3], i: usize, v: f64) {
    if let Some(x) = p.get_mut(i) {
        *x = v;
    }
}
/// A triangle's corners (9 numbers) as points.
fn corners(t: &[f64; 9]) -> [[f64; 3]; 3] {
    [[t[0], t[1], t[2]], [t[3], t[4], t[5]], [t[6], t[7], t[8]]]
}
/// A segment's ends (6 numbers) as points.
fn ends(t: &[f64; 6]) -> [[f64; 3]; 2] {
    [[t[0], t[1], t[2]], [t[3], t[4], t[5]]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
/// Ray × triangle (both sides): the distance along the ray, or None.
fn ray_tri(o: [f64; 3], d: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<f64> {
    let (e1, e2) = (sub(b, a), sub(c, a));
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let tv = sub(o, a);
    let u = dot(tv, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(tv, e1);
    let v = dot(d, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = dot(e2, q) * inv;
    (t > 0.0).then_some(t)
}

/// A text line's quad corners in scene space (dwg-view3d.js `addTexts`, with the atlas's sizes estimated: 0.6 of the
/// height per character, ascent 1, descent 0.3).
fn text_quad(t: &cc_dwgview::compile3d::Text3, c: [f64; 3], s: f64) -> [[f64; 3]; 4] {
    let w = t.s.chars().count() as f64 * 0.6;
    let (asc, desc) = (1.0, 0.3);
    let ha = t.ha.unwrap_or(0.0);
    let dx = if ha == 1.0 {
        -w / 2.0
    } else if ha == 2.0 {
        -w
    } else {
        0.0
    };
    let dy = if t.va == 1.0 {
        desc
    } else if t.va == 2.0 {
        -0.5
    } else if t.va == 3.0 {
        -1.0
    } else {
        0.0
    };
    let corner = |u: f64, v: f64| -> [f64; 3] {
        let k = |o: f64, x: f64, y: f64, c: f64| (o + x * (u + dx) + y * (v + dy) - c) * s;
        [k(t.o[0], t.xd[0], t.yd[0], c[0]), k(t.o[1], t.xd[1], t.yd[1], c[1]), k(t.o[2], t.xd[2], t.yd[2], c[2])]
    };
    [corner(0.0, -desc), corner(w, -desc), corner(w, asc), corner(0.0, asc)]
}

/// `pick(x, y)`: the object under a view point (points from the view's top left): faces by ray (text too), lines within
/// 6 px; a line wins when it is not behind the face. Hidden layers and what the section cuts away are skipped.
pub fn pick_at(app: &UiApp, x: f64, y: f64) -> Option<usize> {
    let b = app.viewer.d3.built.as_ref()?;
    let bx = b.bx.as_ref()?;
    let cam = &app.viewer.d3.cam;
    let (c, s) = (b.origin, b.scale);
    let to = |p: &[f64; 3]| -> [f64; 3] { [(p[0] - c[0]) * s, (p[1] - c[1]) * s, (p[2] - c[2]) * s] };
    let r = cam.ray(x, y);
    let mut best: Option<(u32, f64)> = None;
    for bt in &b.model.batches {
        if layer_hidden(app, &bt.layer) {
            continue;
        }
        for (k, t) in bt.tris.as_chunks::<9>().0.iter().enumerate() {
            let [qa, qb, qc] = corners(t);
            let (pa, pb, pc) = (to(&qa), to(&qb), to(&qc));
            if let Some(d) = ray_tri(r.o, r.d, pa, pb, pc)
                && best.is_none_or(|(_, bd)| d < bd)
            {
                let p = [r.o[0] + r.d[0] * d, r.o[1] + r.d[1] * d, r.o[2] + r.d[2] * d];
                if keeps(app, b, p) {
                    best = Some((bt.tri_owner.get(k).copied().unwrap_or(0), d));
                }
            }
        }
        for t in &bt.texts {
            let q = text_quad(t, c, s);
            for (a1, a2, a3) in [(q[0], q[1], q[2]), (q[0], q[2], q[3])] {
                if let Some(d) = ray_tri(r.o, r.d, a1, a2, a3)
                    && best.is_none_or(|(_, bd)| d < bd)
                {
                    let p = [r.o[0] + r.d[0] * d, r.o[1] + r.d[1] * d, r.o[2] + r.d[2] * d];
                    if keeps(app, b, p) {
                        best = Some((t.owner, d));
                    }
                }
            }
        }
    }
    let vp = cam.view_proj();
    let mut line: Option<(u32, f64, f64)> = None;
    for bt in &b.model.batches {
        if layer_hidden(app, &bt.layer) {
            continue;
        }
        for (k, seg) in bt.lines.as_chunks::<6>().0.iter().enumerate() {
            let [e0, e1] = ends(seg);
            let (p0, p1) = (to(&e0), to(&e1));
            let (Some(a), Some(bb)) = (cam.project_vp(p0, &vp), cam.project_vp(p1, &vp)) else { continue };
            let (dx, dy) = (bb[0] - a[0], bb[1] - a[1]);
            let l2 = dx * dx + dy * dy;
            let tt = if l2 > 0.0 { (((x - a[0]) * dx + (y - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
            let d = (a[0] + tt * dx - x).hypot(a[1] + tt * dy - y);
            if d > 6.0 || line.is_some_and(|(_, px, _)| d >= px) {
                continue;
            }
            let q = [p0[0] + tt * (p1[0] - p0[0]), p0[1] + tt * (p1[1] - p0[1]), p0[2] + tt * (p1[2] - p0[2])];
            if !keeps(app, b, q) {
                continue;
            }
            let depth = dot(sub(q, r.o), r.d);
            line = Some((bt.line_owner.get(k).copied().unwrap_or(0), d, depth));
        }
    }
    let span = dot(sub(bx.max, bx.min), sub(bx.max, bx.min)).sqrt();
    let owner = match (line, best) {
        (Some((lo, _, ld)), Some((_, bd))) if ld <= bd + span * 1e-3 => Some(lo),
        (Some((lo, _, _)), None) => Some(lo),
        (_, Some((fo, _))) => Some(fo),
        (None, None) => None,
    };
    owner.map(|o| o as usize)
}

/// `pick3d(owner, add)`: Properties follows the pick.
fn pick(app: &mut UiApp, owner: Option<usize>, add: bool) {
    let cur = app.viewer.d3.picked.clone();
    let next: Vec<usize> = if add {
        let Some(o) = owner else { return };
        if cur.contains(&o) { cur.into_iter().filter(|x| *x != o).collect() } else { cur.into_iter().chain([o]).collect() }
    } else {
        owner.into_iter().collect()
    };
    if next.is_empty() {
        app.message.clear();
    } else {
        app.viewer.info = true;
    }
    app.viewer.d3.picked = next;
}

pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    if id == "viewer.3d" {
        return toggle(app);
    }
    if !showing(app) {
        return Err(format!("{id}: the 3D view is not showing (viewer.3d)"));
    }
    ensure_built(app);
    match id {
        "viewer.3d.view" => {
            let name = params.name.clone().unwrap_or_else(|| "iso".into());
            let bx = app.viewer.d3.built.as_ref().and_then(|b| b.bx);
            let cam = &mut app.viewer.d3.cam;
            match name.as_str() {
                "persp" => cam.persp = !cam.persp,
                // zoom:F[:x:y] — the wheel's zoom by F (< 1 in) at the view's middle or at x, y (view points)
                z if z.starts_with("zoom:") => {
                    let v: Vec<f64> = z.get(5..).unwrap_or("").split(':').filter_map(|v| v.parse::<f64>().ok()).collect();
                    let f = v.first().copied().filter(|f| f.is_finite() && *f > 0.0).ok_or("viewer.3d.view: zoom:F needs F > 0")?;
                    cam.zoom_at(f, v.get(1).copied(), v.get(2).copied(), None);
                }
                "extents" => {
                    if let Some(bx) = &bx {
                        cam.fit(Some(bx), None);
                    }
                }
                v if cc_m3d_tools::camera::view_dir(v).is_some() => {
                    if let Some(bx) = &bx {
                        cam.standard(v, Some(bx));
                    }
                }
                other => return Err(format!("viewer.3d.view: no view {other}")),
            }
        }
        "viewer.3d.pick" => {
            let n = model(app).map_or(0, |m| m.owners.len());
            // the object may come as the name ("5", "add:5"); "add" alone adds the index
            let (add, from_name) = match params.name.as_deref() {
                Some(v) => {
                    let (a, rest) = v.strip_prefix("add").map_or((false, v), |r| (true, r.trim_start_matches(':')));
                    (a, rest.trim().parse::<usize>().ok())
                }
                None => (false, None),
            };
            let params =
                &Params { index: params.index.or(from_name), name: if add { Some("add".into()) } else { None }, ..Default::default() };
            if let Some(i) = params.index
                && i >= n
            {
                return Err(format!("viewer.3d.pick: no object {i} (0..{n})"));
            }
            pick(app, params.index, params.name.as_deref() == Some("add"));
        }
        "viewer.3d.orbit" => {
            let dx = params.factor.filter(|f| f.is_finite()).ok_or("viewer.3d.orbit: factor = pixels across")?;
            let dy = params.index.map_or(0.0, |i| i as f64);
            app.viewer.d3.cam.orbit(dx, dy);
        }
        "viewer.3d.section" => {
            let max_of = |app: &UiApp, axis: &str| app.viewer.d3.built.as_ref().map_or(0.0, |b| sec_max(b, axis));
            match params.name.as_deref() {
                None if params.factor.is_none() => {
                    app.viewer.d3.section = match app.viewer.d3.section {
                        Some(_) => None,
                        None => Some(Sec { axis: "top", offset: max_of(app, "top") * 0.5, flip: false }),
                    };
                }
                Some("off") => app.viewer.d3.section = None,
                Some("flip") => {
                    let s = app.viewer.d3.section.get_or_insert(Sec { axis: "top", offset: 0.0, flip: false });
                    s.flip = !s.flip;
                }
                Some(a @ ("top" | "front" | "side")) => {
                    let axis: &'static str = match a {
                        "front" => "front",
                        "side" => "side",
                        _ => "top",
                    };
                    let max = max_of(app, axis);
                    let flip = app.viewer.d3.section.is_some_and(|s| s.flip);
                    app.viewer.d3.section = Some(Sec { axis, offset: max * 0.5, flip });
                }
                None => {}
                Some(o) => return Err(format!("viewer.3d.section: no {o} (top, front, side, flip, off)")),
            }
            if let Some(f) = params.factor.filter(|f| f.is_finite()) {
                let axis = app.viewer.d3.section.map_or("top", |s| s.axis);
                let max = max_of(app, axis);
                let s = app.viewer.d3.section.get_or_insert(Sec { axis, offset: 0.0, flip: false });
                s.offset = f.clamp(0.0, max);
            }
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// F (and Fit): the whole drawing.
pub fn fit(app: &mut UiApp) {
    if showing(app)
        && let Some(bx) = app.viewer.d3.built.as_ref().and_then(|b| b.bx)
    {
        app.viewer.d3.cam.fit(Some(&bx), None);
    }
}

/// The 3D bar: standard views, perspective, extents, section; and the section bar under it when a section is on.
fn bar(app: &mut UiApp, ui: &mut egui::Ui) {
    let t = app.tokens;
    let w = |k: &str| lang::bim(k);
    let mut act: Option<(&str, Params)> = None;
    let persp = app.viewer.d3.cam.persp;
    let sec = app.viewer.d3.section;
    egui::Frame::new().fill(t.panel).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for (v, label, title) in [
                ("iso", "ui.v3.iso", "ui.v3.iso.title"),
                ("top", "viewcube.face.top", "ui.view.top.title"),
                ("bottom", "viewcube.face.bottom", "ui.v3.bottom.title"),
                ("front", "viewcube.face.front", "ui.view.front.title"),
                ("back", "viewcube.face.back", "ui.v3.back.title"),
                ("left", "viewcube.face.left", "ui.v3.left.title"),
                ("right", "viewcube.face.right", "ui.v3.right.title"),
            ] {
                if ui.button(w(label)).on_hover_text(w(title)).clicked() {
                    act = Some(("viewer.3d.view", Params { name: Some(v.into()), ..Default::default() }));
                }
            }
            ui.separator();
            if ui.selectable_label(persp, w("ui.v3.persp")).on_hover_text(w("ui.v3.persp.title")).clicked() {
                act = Some(("viewer.3d.view", Params { name: Some("persp".into()), ..Default::default() }));
            }
            if ui.button(w("ui.v3.extents")).on_hover_text(w("ui.v3.extents.title")).clicked() {
                act = Some(("viewer.3d.view", Params { name: Some("extents".into()), ..Default::default() }));
            }
            ui.separator();
            if ui.selectable_label(sec.is_some(), w("sec.button")).on_hover_text(w("sec.dwg.title")).clicked() {
                act = Some(("viewer.3d.section", Params::default()));
            }
        });
        if let Some(s) = sec {
            let max = app.viewer.d3.built.as_ref().map_or(0.0, |b| sec_max(b, s.axis));
            let units = model(app).map(|m| m.units.clone()).filter(|u| !u.is_empty()).unwrap_or_else(|| w("sec.units"));
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(w("ui.sec.cutfrom")).small().color(t.text_dim));
                for (a, label, title) in [
                    ("top", "viewcube.face.top", "ui.sec.top.title"),
                    ("front", "viewcube.face.front", "ui.sec.front.title"),
                    ("side", "ui.view.side", "ui.sec.side.title"),
                ] {
                    if ui.selectable_label(s.axis == a, w(label)).on_hover_text(w(title)).clicked() {
                        act = Some(("viewer.3d.section", Params { name: Some(a.into()), ..Default::default() }));
                    }
                }
                ui.label(RichText::new(w("sec.offset.label")).small().color(t.text_dim));
                let mut v = s.offset;
                let hi = if max > 0.0 { max } else { 1.0 };
                if ui.add(egui::Slider::new(&mut v, 0.0..=hi).show_value(false)).changed() {
                    act = Some(("viewer.3d.section", Params { factor: Some(v), ..Default::default() }));
                }
                // metres(): three decimals, trailing zeros dropped
                let shown = format!("{:.3}", (s.offset * 1000.0).round() / 1000.0);
                let shown = shown.trim_end_matches('0').trim_end_matches('.').to_string();
                ui.label(RichText::new(format!("{} {units}", if shown.is_empty() { "0".into() } else { shown })).small().color(t.text));
                let hint = match s.axis {
                    "front" => "sec.hint.front",
                    "side" => "sec.hint.side",
                    _ => "sec.hint.top",
                };
                ui.label(RichText::new(w(hint)).small().color(t.text_dim));
                if ui.selectable_label(s.flip, w("ui.sec.flip")).on_hover_text(w("ui.sec.flip.title")).clicked() {
                    act = Some(("viewer.3d.section", Params { name: Some("flip".into()), ..Default::default() }));
                }
            });
        }
    });
    if let Some((id, p)) = act
        && let Err(e) = app.run(id, &p)
    {
        app.error = Some(e);
    }
}

/// The 3D view in the central area: the bar, the view, text and the pick over it.
pub fn show(app: &mut UiApp, ui: &mut egui::Ui) {
    ensure_built(app);
    bar(app, ui);
    let rect = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    let ppp = f64::from(ui.ctx().pixels_per_point());
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
    app.viewer.d3.cam.set_size(w, h);
    if app.viewer.d3.fit_pending {
        app.viewer.d3.fit_pending = false;
        if let Some(bx) = app.viewer.d3.built.as_ref().and_then(|b| b.bx) {
            app.viewer.d3.cam.standard("iso", Some(&bx));
        }
    }
    input(app, ui, &resp, rect);
    let t = app.tokens;
    let Some(b) = app.viewer.d3.built.as_ref() else { return };
    let mut cam = app.viewer.d3.cam.clone();
    if let Some(bx) = &b.bx {
        let d = sub(bx.max, bx.min);
        cam.clip(dot(d, d).sqrt() / 2.0);
    }
    app.viewer.d3.cam.near = cam.near;
    app.viewer.d3.cam.far = cam.far;
    let c3 = Cam3 {
        eye: cam.eye,
        target: cam.target,
        fov: cam.fov,
        persp: cam.persp,
        width: w * ppp,
        height: h * ppp,
        near: cam.near,
        far: cam.far,
    };
    let f = |c: Color32| [f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0];
    let bgc = f(t.canvas_bg);
    let section = match (&app.viewer.d3.section, &b.bx) {
        (Some(s), Some(bx)) => {
            let (i, sign, at) = sec_plane(s, bx, b.scale);
            let mut n = [0.0; 3];
            set_ax(&mut n, i, -sign);
            Some((n, -sign * at))
        }
        _ => None,
    };
    let style = Style3 { background: [bgc[0], bgc[1], bgc[2], 1.0], back_own: true, textures: false, section, ..Style3::default() };
    crate::view3d::paint(ui, rect, &b.scene, &c3, &style);
    overlay(app, ui, rect, &cam);
}

fn input(app: &mut UiApp, ui: &egui::Ui, resp: &egui::Response, rect: Rect) {
    let (mods, scroll, zoom, hover, mt) =
        ui.input(|i| (i.modifiers, i.smooth_scroll_delta.y, i.zoom_delta(), i.pointer.hover_pos(), i.multi_touch()));
    let local = |p: Pos2| (f64::from(p.x - rect.min.x), f64::from(p.y - rect.min.y));
    if let Some(m) = mt.filter(|m| rect.contains(m.center_pos)) {
        let (cx, cy) = local(m.center_pos);
        let d = m.translation_delta;
        let cam = &mut app.viewer.d3.cam;
        cam.pan(f64::from(d.x), f64::from(d.y));
        let z = f64::from(m.zoom_delta);
        if z.is_finite() && z > 0.0 && (z - 1.0).abs() > 1e-6 {
            cam.zoom_at(1.0 / z, Some(cx), Some(cy), None);
        }
        return;
    }
    let d = resp.drag_delta();
    let (dx, dy) = (f64::from(d.x), f64::from(d.y));
    if dx != 0.0 || dy != 0.0 {
        let cam = &mut app.viewer.d3.cam;
        if resp.dragged_by(egui::PointerButton::Secondary)
            || (mods.shift && (resp.dragged_by(egui::PointerButton::Primary) || resp.dragged_by(egui::PointerButton::Middle)))
        {
            cam.pan(dx, dy);
        } else if resp.dragged_by(egui::PointerButton::Primary) || resp.dragged_by(egui::PointerButton::Middle) {
            cam.orbit(dx, dy);
        }
    }
    if resp.hovered()
        && let Some(p) = hover
    {
        let (x, y) = local(p);
        if scroll != 0.0 {
            let k = (-f64::from(scroll) * 0.003).exp();
            let pivot = face_hit(app, x, y);
            app.viewer.d3.cam.zoom_at(k, Some(x), Some(y), pivot);
        }
        if zoom != 1.0 && zoom > 0.0 {
            app.viewer.d3.cam.zoom_at(1.0 / f64::from(zoom), Some(x), Some(y), None);
        }
    }
    if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let (x, y) = local(p);
        let o = pick_at(app, x, y);
        pick(app, o, mods.command);
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) && ui.ctx().memory(|m| m.focused().is_none()) {
        pick(app, None, false);
    }
}

/// `hit(x, y)`: where the ray through a point first meets a face (scene space).
fn face_hit(app: &UiApp, x: f64, y: f64) -> Option<[f64; 3]> {
    let b = app.viewer.d3.built.as_ref()?;
    let r = app.viewer.d3.cam.ray(x, y);
    let (c, s) = (b.origin, b.scale);
    let to = |p: &[f64; 3]| -> [f64; 3] { [(p[0] - c[0]) * s, (p[1] - c[1]) * s, (p[2] - c[2]) * s] };
    let mut best: Option<f64> = None;
    let mut budget = 400_000usize;
    for bt in &b.model.batches {
        if layer_hidden(app, &bt.layer) {
            continue;
        }
        for t in bt.tris.as_chunks::<9>().0 {
            budget = budget.saturating_sub(1);
            if budget == 0 {
                break;
            }
            let [qa, qb, qc] = corners(t);
            if let Some(d) = ray_tri(r.o, r.d, to(&qa), to(&qb), to(&qc))
                && best.is_none_or(|bd| d < bd)
            {
                best = Some(d);
            }
        }
    }
    best.map(|d| [r.o[0] + r.d[0] * d, r.o[1] + r.d[1] * d, r.o[2] + r.d[2] * d])
}

/// Text (screen-facing quads' stand-in: each line drawn at its place, turned along its baseline), the pick's lines and
/// faces in the pick colour, and the section plane's outline.
fn overlay(app: &UiApp, ui: &egui::Ui, rect: Rect, cam: &Camera) {
    let Some(b) = app.viewer.d3.built.as_ref() else { return };
    let painter = ui.painter_at(rect);
    let (c, s) = (b.origin, b.scale);
    let vp = cam.view_proj();
    let proj = |p: [f64; 3]| cam.project_vp(p, &vp).map(|q| Pos2::new(rect.min.x + q[0] as f32, rect.min.y + q[1] as f32));
    let to = |p: &[f64; 3]| -> [f64; 3] { [(p[0] - c[0]) * s, (p[1] - c[1]) * s, (p[2] - c[2]) * s] };
    let t = app.tokens;
    let mut shown = 0usize;
    for bt in &b.model.batches {
        if bt.texts.is_empty() || layer_hidden(app, &bt.layer) {
            continue;
        }
        let col = color3(bt.color, false, t.canvas_bg, t.ink);
        let col = Color32::from_rgb(col[0], col[1], col[2]);
        for tx in &bt.texts {
            if shown > 20_000 {
                break;
            }
            let o = to(&tx.o);
            if !keeps(app, b, o) {
                continue;
            }
            let up = [o[0] + tx.yd[0] * s, o[1] + tx.yd[1] * s, o[2] + tx.yd[2] * s];
            let along = [o[0] + tx.xd[0] * s, o[1] + tx.xd[1] * s, o[2] + tx.xd[2] * s];
            let (Some(p0), Some(pu), Some(pa)) = (proj(o), proj(up), proj(along)) else { continue };
            let hpx = (pu - p0).length();
            if !(2.0..=2000.0).contains(&hpx) || !rect.expand(hpx * 20.0).contains(p0) {
                continue;
            }
            let size = (hpx / 0.72 * 2.0).round() / 2.0;
            let g = painter.layout_no_wrap(tx.s.clone(), egui::FontId::proportional(size), col);
            let (gw, gh) = (g.size().x, g.size().y);
            let ha = tx.ha.unwrap_or(0.0);
            let ox = if ha == 1.0 {
                -gw / 2.0
            } else if ha == 2.0 {
                -gw
            } else {
                0.0
            };
            let oy = if tx.va == 1.0 {
                -gh
            } else if tx.va == 2.0 {
                -gh / 2.0
            } else if tx.va == 3.0 {
                0.0
            } else {
                -gh * 0.8
            };
            let dir = pa - p0;
            let ang = dir.y.atan2(dir.x);
            let (sn, cs) = ang.sin_cos();
            let pos = p0 + egui::vec2(ox * cs - oy * sn, ox * sn + oy * cs);
            painter.add(egui::epaint::TextShape::new(pos, g, col).with_angle(ang));
            shown += 1;
        }
    }
    // the pick, over everything
    let picked: HashSet<u32> = app.viewer.d3.picked.iter().filter_map(|o| u32::try_from(*o).ok()).collect();
    if !picked.is_empty() {
        let fill = Color32::from_rgba_unmultiplied(PICK.r(), PICK.g(), PICK.b(), 115);
        for bt in &b.model.batches {
            if layer_hidden(app, &bt.layer) {
                continue;
            }
            for (k, tri) in bt.tris.as_chunks::<9>().0.iter().enumerate() {
                let [qa, qb, qc] = corners(tri);
                if bt.tri_owner.get(k).is_some_and(|o| picked.contains(o))
                    && let (Some(a), Some(bb), Some(cc)) = (proj(to(&qa)), proj(to(&qb)), proj(to(&qc)))
                {
                    painter.add(egui::Shape::convex_polygon(vec![a, bb, cc], fill, Stroke::NONE));
                }
            }
            for (k, seg) in bt.lines.as_chunks::<6>().0.iter().enumerate() {
                let [e0, e1] = ends(seg);
                if bt.line_owner.get(k).is_some_and(|o| picked.contains(o))
                    && let (Some(a), Some(bb)) = (proj(to(&e0)), proj(to(&e1)))
                {
                    painter.line_segment([a, bb], Stroke::new(1.5, PICK));
                }
            }
            for tx in &bt.texts {
                if picked.contains(&tx.owner) {
                    let q = text_quad(tx, c, s);
                    let pts: Vec<Pos2> = q.iter().filter_map(|p| proj(*p)).collect();
                    if pts.len() == 4 {
                        painter.add(egui::Shape::convex_polygon(pts, fill, Stroke::new(1.0, PICK)));
                    }
                }
            }
        }
    }
    // the section plane's outline, on the kept side
    if let (Some(sec), Some(bx)) = (&app.viewer.d3.section, &b.bx) {
        let (i, sign, at) = sec_plane(sec, bx, b.scale);
        let (j, l) = ((i + 1) % 3, (i + 2) % 3);
        let eps = (ax(&bx.max, i) - ax(&bx.min, i)) * 1e-4 + 1e-4;
        let put = at + sign * eps;
        let mut pts = Vec::new();
        for (u, v) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
            let mut p = [0.0; 3];
            set_ax(&mut p, i, put);
            set_ax(&mut p, j, if u == 1 { ax(&bx.max, j) } else { ax(&bx.min, j) });
            set_ax(&mut p, l, if v == 1 { ax(&bx.max, l) } else { ax(&bx.min, l) });
            if let Some(q) = proj(p) {
                pts.push(q);
            }
        }
        if pts.len() == 4 {
            painter.add(egui::Shape::closed_line(pts, Stroke::new(1.0, PICK)));
        }
    }
}

/// The Drawing panel's content while the 3D view shows: what is drawn (props3d), or the pick.
pub fn panel(app: &UiApp, ui: &mut egui::Ui) {
    let t = app.tokens;
    let Some(m) = model(app) else { return };
    let small = |ui: &mut egui::Ui, s: &str, c: Color32| {
        ui.label(RichText::new(s).small().color(c));
    };
    let picked = &app.viewer.d3.picked;
    let key = (app.doc.as_ref().map_or(0, |d| d.id), picked.clone(), cc_lang::locale());
    let cached = app.viewer.d3.view_cache.borrow().as_ref().filter(|c| c.0 == key).map(|c| c.1.clone());
    let view = match cached {
        Some(v) => v,
        None => {
            let v = if picked.is_empty() {
                None
            } else {
                app.doc.as_ref().and_then(|d| d.model.as_ref()).and_then(|db| {
                    let defs = cc_dwgview::dynamic::dynamic_defs(db);
                    page::pick_view(page::model_space(db), &m.units, picked, &|e| cc_dwgview::dynamic::dynamic_of(db, e, &defs))
                })
            };
            *app.viewer.d3.view_cache.borrow_mut() = Some((key, v.clone()));
            v
        }
    };
    match view {
        None => {
            let p = page::props3d(&m);
            small(ui, &p.hint, t.text_dim);
            small(ui, &p.intro, t.text_dim);
            grid(ui, "dwg3d_rows", &p.rows, &t);
            for c in [&p.flat, &p.missing].into_iter().flatten() {
                ui.separator();
                ui.label(RichText::new(&c.title).strong().color(t.text_strong));
                grid(ui, &c.title, &c.rows, &t);
                small(ui, &c.note, t.text_dim);
            }
        }
        Some(page::PickView::Many { title, hint, counts }) => {
            ui.label(RichText::new(title).strong().color(t.text_strong));
            small(ui, &hint, t.text_dim);
            grid(ui, "dwg3d_many", &counts, &t);
        }
        Some(page::PickView::One { cls, name, rows, attrs, back }) => {
            small(ui, &cls, t.text_dim);
            ui.label(RichText::new(name).strong().color(t.text_strong));
            grid(ui, "dwg3d_props", &rows, &t);
            if let Some((summary, list)) = attrs {
                egui::CollapsingHeader::new(RichText::new(summary).small())
                    .default_open(true)
                    .show(ui, |ui| grid(ui, "dwg3d_attrs", &list, &t));
            }
            small(ui, &back, t.text_dim);
        }
    }
}

fn grid(ui: &mut egui::Ui, id: &str, rows: &[(String, String)], t: &crate::theme::Tokens) {
    egui::Grid::new(id).num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
        for (k, v) in rows {
            ui.label(RichText::new(k).small().color(t.text_dim));
            ui.label(RichText::new(v).small().color(t.text));
            ui.end_row();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_follow_the_theme() {
        let dark_bg = Color32::from_rgb(0x1e, 0x1e, 0x1e);
        let light_bg = Color32::from_rgb(0xf3, 0xf6, 0xf8);
        // yellow darkens on a light screen; blue lightens on a dark one; ink faces are grey
        assert_eq!(color3(Color::Rgb(0xffff00), false, light_bg, Color32::BLACK), [140, 140, 0, 255]);
        assert_eq!(color3(Color::Rgb(0x0000ff), false, dark_bg, Color32::WHITE), [115, 115, 255, 255]);
        assert_eq!(color3(Color::Ink, true, dark_bg, Color32::WHITE), [0x9a, 0xa4, 0xb0, 255]);
        assert!(
            ray_tri([0.0, 0.0, 5.0], [0.0, 0.0, -1.0], [-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0])
                .is_some_and(|d| (d - 5.0).abs() < 1e-12)
        );
    }

    /// Survey coordinates (§ 12): lines 2 mm apart at x = 500 000 m, y = 4 000 000 m, z = 120 m in a drawing 10 m
    /// across, seen from the top and zoomed in: each line picks itself at its own pixel.
    #[test]
    fn survey_coordinates_pick_steadily() {
        let mut l: Vec<String> = Vec::new();
        let mut g = |c: i32, v: String| {
            l.push(c.to_string());
            l.push(v);
        };
        for (c, v) in [(0, "SECTION"), (2, "ENTITIES")] {
            g(c, v.into());
        }
        let (x0, y0, z0) = (500_000.0_f64, 4_000_000.0_f64, 120.0_f64);
        for (k, dx) in [0.0, 0.002, 10.0].iter().enumerate() {
            for (c, v) in [
                (0, "LINE".to_string()),
                (5, format!("{:X}", 0x40 + k)),
                (100, "AcDbEntity".into()),
                (8, "0".into()),
                (100, "AcDbLine".into()),
            ] {
                g(c, v);
            }
            for (c, v) in [(10, x0 + dx), (20, y0), (30, z0), (11, x0 + dx), (21, y0 + 5.0), (31, z0)] {
                g(c, format!("{v:.4}"));
            }
        }
        for (c, v) in [(0, "ENDSEC"), (0, "EOF")] {
            g(c, v.into());
        }
        let bytes = (l.join("\r\n") + "\r\n").into_bytes();
        let mut app = UiApp::default();
        app.open_bytes("survey.dxf", &bytes);
        assert!(app.error.is_none(), "{:?}", app.error);
        app.run("viewer.3d", &Params::default()).unwrap();
        ensure_built(&mut app);
        app.viewer.d3.cam.set_size(800.0, 600.0);
        app.run("viewer.3d.view", &Params { name: Some("top".into()), ..Default::default() }).unwrap();
        let (c, s) = {
            let b = app.viewer.d3.built.as_ref().unwrap();
            (b.origin, b.scale)
        };
        let to = |x: f64, y: f64, z: f64| [(x - c[0]) * s, (y - c[1]) * s, (z - c[2]) * s];
        // zoom in at the pair until 2 mm is several pixels
        for _ in 0..8 {
            let p = app.viewer.d3.cam.project(to(x0 + 0.001, y0 + 2.5, z0)).unwrap();
            app.viewer.d3.cam.zoom_at(0.35, Some(p[0]), Some(p[1]), None);
        }
        let a = app.viewer.d3.cam.project(to(x0, y0 + 2.5, z0)).unwrap();
        let b = app.viewer.d3.cam.project(to(x0 + 0.002, y0 + 2.5, z0)).unwrap();
        assert!(b[0] - a[0] > 4.0, "2 mm apart is {} px", b[0] - a[0]);
        assert_eq!(pick_at(&app, a[0], a[1]), Some(0));
        assert_eq!(pick_at(&app, b[0], b[1]), Some(1));
    }
}
