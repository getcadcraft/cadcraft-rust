//! CadCraft's 3D tab (ws3d.js): the drawing's 3D model in a 3D view, with the 2D model space on the ground in its
//! layer colours, ModelCraft's tools on the solids, and the command line's questions answered by clicks in 3D. The
//! workings live in the engine ([`cc_engine::ws3d`], ws3d-core.js); this module draws them and feeds them the pointer.
//!
//! Navigation as ws3d.js: middle-drag orbits, Shift + middle or right-drag pans, the wheel zooms at the cursor (about
//! the face under it), a right click without dragging is Enter, a middle double-click fits everything. While the tab
//! shows, the toolbar's 3D group (ui.inc `data-tools-3d`) runs the 3D commands; the view cube in the corner turns the
//! view; the bar at the bottom shows the tool's tip and the visual style.
//!
//! Large coordinates: the solids' meshes keep their own origins and the 2D lines are laid around the drawing's corner,
//! so survey-sized coordinates stay steady on the GPU (the 3D renderer draws relative to the camera).

use cc_engine::ws3d::{Ev3, Ws3d};
use cc_m3d::def::face_tris;
use cc_m3d::model::Model;
use cc_m3d::ops::Sel;
use cc_m3d::vec::{IDENT, P3, mmul};
use cc_render::mesh3d::{Cam3, Instance3, MeshData, Style3};
use egui::{Color32, Pos2, Rect, Sense, Stroke};

use crate::theme::Tokens;
use crate::view3d::Scene3D;
use crate::{Params, UiApp};

/// UI commands of the 3D tab.
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("tab3d.open", "3D tab", "", "{} → shows the drawing's 3D model (the 3D tab; commands3d.js runs there)"),
    ("tab3d.close", "Model tab", "", "{} → back to the 2D model"),
    ("tab3d.orbit", "Orbit the 3D view", "", "{factor: pixels across, index?: pixels down} → turns the 3D view as a middle-drag does"),
    ("tab3d.cube", "View cube", "", "{name: top|bottom|front|back|left|right|iso} → looks from that side (the view cube's faces)"),
];

/// The 3D tab's view state (the workspace itself is the engine's).
#[derive(Default)]
pub struct Tab3d {
    scene: Option<Scene3D>,
    /// what the scene was built from: (model version, model generation, doc version, line width bits, dark)
    built: Option<(u64, u64, u64, u64, bool)>,
    seq: u64,
    held: Option<egui::PointerButton>,
    press: Option<Pos2>,
    cam_moved: bool,
    /// the width of the 2D lines on the ground (m)
    line_w: f64,
    clock: Option<web_time::Instant>,
    /// the last middle click (ms), for the double click that fits
    last_mid: Option<f64>,
}

impl Tab3d {
    fn now(&mut self) -> f64 {
        let c = *self.clock.get_or_insert_with(web_time::Instant::now);
        c.elapsed().as_secs_f64() * 1000.0
    }
}

/// `tab3d.*` commands.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    let ed = app.editor.as_mut().ok_or(format!("{id}: no drawing is open for editing"))?;
    match id {
        "tab3d.open" => {
            if ed.eng.busy() && !ed.eng.in3d() {
                ed.escape();
            }
            if !ed.eng.open3d() {
                return Err("tab3d.open: the 3D tab could not be opened".into());
            }
        }
        "tab3d.close" => ed.eng.hide3d(),
        "tab3d.orbit" => {
            let dx = params.factor.filter(|f| f.is_finite()).ok_or("tab3d.orbit: factor = pixels across")?;
            let dy = params.index.map_or(0.0, |i| i as f64);
            ed.eng.with_ws3d(|ws, _| ws.tools.host.cam.orbit(dx, dy)).ok_or("tab3d.orbit: the 3D tab is not open")?;
        }
        "tab3d.cube" => {
            let name = params.name.clone().unwrap_or_else(|| "iso".into());
            let dir = cc_m3d_tools::camera::view_dir(&name).ok_or(format!("tab3d.cube: no view {name}"))?;
            ed.eng
                .with_ws3d(|ws, _| {
                    let d = [-dir[0], -dir[1], -dir[2]];
                    cc_m3d_tools::viewcube_camera::apply_view(&mut ws.tools.host.cam, d, None);
                })
                .ok_or("tab3d.cube: the 3D tab is not open")?;
        }
        other => return Err(format!("unknown command {other}")),
    }
    app.sync_editor();
    Ok(())
}

/// The editor shows the 3D tab.
pub fn showing(app: &UiApp) -> bool {
    app.editor.as_ref().is_some_and(|e| e.eng.in3d())
}

fn hex(s: &str, ink: Color32) -> [u8; 4] {
    let h = s.strip_prefix('#').filter(|h| h.len() == 6).and_then(|h| u32::from_str_radix(h, 16).ok());
    match h {
        Some(v) => [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255],
        None => [ink.r(), ink.g(), ink.b(), 255],
    }
}

/// The model's solids (one mesh per definition and colour, as ModelCraft's view builds them) and the 2D drawing's lines
/// as flat ribbons `w` wide on the ground, in their colours.
fn build(ws: &mut Ws3d, doc: &cc_cad::Doc, id: u64, dark: bool, ink: Color32, w: f64) -> Scene3D {
    let front = if dark { crate::model3d::FRONT_MODEL } else { crate::model3d::FRONT_PAPER };
    let lines = ws.lines(doc);
    let model: &Model = &ws.tools.host.model;
    let mut meshes: Vec<MeshData> = Vec::new();
    let mut index: std::collections::HashMap<(String, String), usize> = std::collections::HashMap::new();
    let mut instances: Vec<Instance3> = Vec::new();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let grow = |lo: &mut [f64; 3], hi: &mut [f64; 3], p: [f64; 3]| {
        for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
            *l = l.min(v);
            *h = h.max(v);
        }
    };
    model.walk_shown("root", &IDENT, &mut |def, m, _path, color| {
        if def.faces.is_empty() && def.edges.is_empty() {
            return true;
        }
        let key = (def.id.clone(), color.unwrap_or("").to_string());
        let mi = *index.entry(key).or_insert_with(|| {
            meshes.push(crate::model3d::def_mesh(def, color, front));
            meshes.len() - 1
        });
        for (_, p) in def.verts.iter() {
            grow(
                &mut lo,
                &mut hi,
                [
                    m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
                    m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
                    m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
                ],
            );
        }
        instances.push(Instance3 { mesh: mi, m: *m, tint: None, hidden: false });
        true
    });
    // the 2D drawing on the ground, around its own corner
    if let Some(bb) = lines.bbox {
        let o = [bb.min[0], bb.min[1], 0.0];
        let h = (w / 2.0) as f32;
        let mut mesh = MeshData { origin: o, ..MeshData::default() };
        for (c, segs) in &lines.batches {
            let col = hex(c, ink);
            for s in segs.as_chunks::<4>().0 {
                let a = [(s[0] - o[0]) as f32, (s[1] - o[1]) as f32];
                let b = [(s[2] - o[0]) as f32, (s[3] - o[1]) as f32];
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let l = (dx * dx + dy * dy).sqrt();
                let (nx, ny, ex, ey) =
                    if l > 1e-9 { (-dy / l * h, dx / l * h, dx / l * h * 0.5, dy / l * h * 0.5) } else { (h, 0.0, 0.0, h) };
                let base = mesh.positions.len() as u32;
                for p in [
                    [a[0] + nx - ex, a[1] + ny - ey, 0.0],
                    [a[0] - nx - ex, a[1] - ny - ey, 0.0],
                    [b[0] - nx + ex, b[1] - ny + ey, 0.0],
                    [b[0] + nx + ex, b[1] + ny + ey, 0.0],
                ] {
                    mesh.positions.push(p);
                    mesh.normals.push([0.0, 0.0, 1.0]);
                    mesh.colors.push(col);
                }
                mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
        if !mesh.indices.is_empty() {
            grow(&mut lo, &mut hi, bb.min);
            grow(&mut lo, &mut hi, bb.max);
            meshes.push(mesh);
            instances.push(Instance3 { mesh: meshes.len() - 1, m: IDENT, tint: None, hidden: false });
        }
    }
    // the open context's axes (view3d.js buildExtras): solid on the positive side, dashed on the negative, 4 × the
    // model's size long (at least 10 m)
    let r = model.bbox("root", &IDENT).map_or(30.0, |b| cc_m3d::vec::dist(b.min, b.max).max(10.0));
    let am = cc_m3d_tools::tools::ctx_of(model, &ws.tools.host.path).m;
    let mut axes = MeshData { origin: [am[12], am[13], am[14]], ..MeshData::default() };
    for (dir, col) in
        [([1.0, 0.0, 0.0], [0xd0, 0x22, 0x3e, 255]), ([0.0, 1.0, 0.0], [0x17, 0x9a, 0x4a, 255]), ([0.0, 0.0, 1.0], [0x2a, 0x5c, 0xe0, 255])]
    {
        let d = cc_m3d::vec::apply_dir(&am, dir);
        let l = r * 4.0;
        ribbon(&mut axes, [0.0; 3], [d[0] * l, d[1] * l, d[2] * l], w, col);
        let step = r / 60.0;
        let mut s = 0.0;
        while s < l && step > 0.0 {
            let e = (s + step).min(l);
            ribbon(&mut axes, [-d[0] * s, -d[1] * s, -d[2] * s], [-d[0] * e, -d[1] * e, -d[2] * e], w, col);
            s += step * 2.0;
        }
    }
    // axes far from everything (survey coordinates: the origin is kilometres away) cannot be seen: left out
    let o = [am[12], am[13], am[14]];
    let far = lo[0].is_finite()
        && (0..3).any(|i| o.get(i).zip(lo.get(i)).zip(hi.get(i)).is_some_and(|((o, l), h)| *o < l - r * 4.0 || *o > h + r * 4.0));
    if !far {
        meshes.push(axes);
        instances.push(Instance3 { mesh: meshes.len() - 1, m: IDENT, tint: None, hidden: false });
    }
    if !lo[0].is_finite() {
        lo = [-5.0, -5.0, 0.0];
        hi = [5.0, 5.0, 3.0];
    }
    Scene3D::new(id, meshes, instances, lo, hi)
}

/// A line `w` wide from a to b (relative to the mesh's origin) as two crossed strips, so it shows from any side.
fn ribbon(m: &mut MeshData, a: [f64; 3], b: [f64; 3], w: f64, col: [u8; 4]) {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if l < 1e-12 {
        return;
    }
    let u = [d[0] / l, d[1] / l, d[2] / l];
    // two directions across the line
    let up = if u[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let c1 = cc_m3d::vec::norm(cc_m3d::vec::cross(u, up));
    let c2 = cc_m3d::vec::cross(u, c1);
    let h = w / 2.0;
    for (c, n) in [(c1, c2), (c2, c1)] {
        let base = m.positions.len() as u32;
        for p in [
            [a[0] + c[0] * h, a[1] + c[1] * h, a[2] + c[2] * h],
            [a[0] - c[0] * h, a[1] - c[1] * h, a[2] - c[2] * h],
            [b[0] - c[0] * h, b[1] - c[1] * h, b[2] - c[2] * h],
            [b[0] + c[0] * h, b[1] + c[1] * h, b[2] + c[2] * h],
        ] {
            m.positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
            // lit as the ground is (the strips are lines, not surfaces)
            let _ = n;
            m.normals.push([0.0, 0.0, 1.0]);
            m.colors.push(col);
        }
        m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn style3(key: &str, t: &Tokens) -> Style3 {
    let bg = t.canvas_bg;
    let ink = t.ink;
    let f = |c: Color32| [f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0];
    let b = f(bg);
    let e = f(ink);
    let mut s = Style3 {
        background: [b[0], b[1], b[2], 1.0],
        edge: [e[0], e[1], e[2], 1.0],
        back: crate::model3d::BACK_MODEL,
        textures: true,
        ..Style3::default()
    };
    match key {
        "shaded" => s.edges = false,
        "hidden" => s.hidden_line = true,
        "wire" => s.faces = false,
        "xray" => s.xray = true,
        _ => {}
    }
    s
}

fn kind_color(kind: &str, t: &Tokens) -> Color32 {
    match kind {
        "end" | "axis-y" => Color32::from_rgb(0x17, 0x9a, 0x4a),
        "mid" => Color32::from_rgb(0x16, 0x9c, 0xb0),
        "center" | "face" | "axis-z" => Color32::from_rgb(0x2a, 0x5c, 0xe0),
        "edge" | "axis-x" => Color32::from_rgb(0xd0, 0x22, 0x3e),
        "origin" => Color32::from_rgb(0xb8, 0x74, 0x1a),
        "parallel" | "perp" | "lock" => Color32::from_rgb(0xc0, 0x2a, 0xa0),
        _ => t.text_dim,
    }
}

/// The toolbar's 3D group: (command line, label key, tooltip key).
const TOOLS3D: &[(&str, &str, &str)] = &[
    ("EXTRUDE", "ui.extrude", "ui.extrude_closed_2d_objects"),
    ("PRESSPULL", "ui.presspull", "ui.push_pull_face_solid"),
    ("BOX", "ui.box", "ui.box_two_corners_height"),
    ("CYLINDER", "ui.cylinder", "ui.cylinder_centre_radius_height"),
    ("WEDGE", "ui.wedge", "ui.wedge_two_corners_height"),
    ("MOVE", "ui.move", "ui.move_solids_faces_edges"),
    ("COPY", "ui.copy", "ui.copy_solids_co"),
    ("ROTATE", "ui.rotate", "ui.rotate_solids_ro"),
    ("ERASE", "ui.erase", "ui.erase_solids_faces_edges"),
    ("3DORBIT", "ui.orbit", "ui.3dorbit.title"),
];

/// Run a toolbar entry: a command, then its option ("VIEW I").
fn run_line(app: &mut UiApp, line: &str) {
    let Some(ed) = app.editor.as_mut() else { return };
    let mut parts = line.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("");
    if ed.eng.busy() {
        ed.escape();
    }
    ed.line.clear();
    ed.type_line(cmd);
    if let Some(o) = parts.next()
        && ed.eng.busy()
    {
        ed.type_line(o);
    }
    app.sync_editor();
}

fn toolbar(app: &mut UiApp, ui: &mut egui::Ui) {
    let tr = |k: &str| crate::edit::lang_text(k, &[]);
    let mut pick: Option<String> = None;
    let style_key = app.editor.as_ref().and_then(|e| e.eng.with_ws3d(|ws, _| ws.style_key.clone())).unwrap_or_default();
    let persp = app.editor.as_ref().and_then(|e| e.eng.with_ws3d(|ws, _| ws.tools.host.cam.persp)).unwrap_or(true);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("3D").small().strong().color(app.tokens.text_dim));
        for (cmd, label, title) in TOOLS3D {
            if ui.small_button(tr(label)).on_hover_text(tr(title)).clicked() {
                pick = Some((*cmd).to_string());
            }
        }
        ui.menu_button(tr("toolbar.bool"), |ui| {
            for (c, k) in [
                ("REVOLVE", "toolbar.revolve"),
                ("SWEEP", "toolbar.sweep"),
                ("UNION", "toolbar.bool.union"),
                ("SUBTRACT", "toolbar.bool.subtract"),
                ("INTERSECT", "toolbar.bool.intersect"),
            ] {
                if ui.button(tr(k)).clicked() {
                    pick = Some(c.into());
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text(tr("toolbar.bool.title"));
        ui.menu_button(tr("ui.views"), |ui| {
            for (c, k) in [
                ("VIEW I", "ui.iso"),
                ("VIEW T", "ui.top"),
                ("VIEW BO", "ui.bottom"),
                ("VIEW F", "ui.front"),
                ("VIEW BA", "ui.back"),
                ("VIEW L", "ui.left"),
                ("VIEW R", "ui.right"),
            ] {
                if ui.button(tr(k)).clicked() {
                    pick = Some(c.into());
                    ui.close();
                }
            }
            ui.separator();
            if ui.radio(persp, tr("ui.perspective")).clicked() {
                pick = Some("VIEW P".into());
                ui.close();
            }
            if ui.radio(!persp, tr("ui.parallel")).clicked() {
                pick = Some("VIEW PA".into());
                ui.close();
            }
            ui.separator();
            if ui.button(tr("ui.zoom_everything")).clicked() {
                pick = Some("VIEW E".into());
                ui.close();
            }
        })
        .response
        .on_hover_text(tr("ui.standard_views_perspective_parallel"));
        ui.menu_button(tr("ui.style"), |ui| {
            for (key, c, k) in [
                ("shaded", "VSCURRENT S", "ui.shaded"),
                ("edges", "VSCURRENT E", "ui.shaded_edges"),
                ("hidden", "VSCURRENT H", "ui.hidden_line"),
                ("wire", "VSCURRENT W", "ui.wireframe"),
                ("xray", "VSCURRENT X", "ui.x_ray"),
            ] {
                if ui.radio(style_key == key, tr(k)).clicked() {
                    pick = Some(c.into());
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text(tr("ui.visual_style_shaded_edges"));
        ui.menu_button(tr("ui.export"), |ui| {
            for (c, k) in [("GLBOUT", "ui.glb_gltf_viewers_web"), ("OBJOUT", "ui.obj"), ("STLOUT", "ui.stl_3d_printing_millimeters")] {
                if ui.button(tr(k)).clicked() {
                    pick = Some(c.into());
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text(tr("ui.export_3d_model_glb"));
    });
    if let Some(l) = pick {
        run_line(app, &l);
    }
}

/// The 3D tab in the central area: the toolbar's 3D group, the view, its overlay, the cube and the bar.
pub fn show(app: &mut UiApp, ui: &mut egui::Ui) {
    egui::Frame::new().fill(app.tokens.panel).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| toolbar(app, ui));
    let rect = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    let ppp = f64::from(ui.ctx().pixels_per_point());
    let t = app.tokens;
    let dark = app.preset == crate::theme::Preset::Dark;
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
    {
        let Some(ed) = app.editor.as_mut() else { return };
        ed.eng.with_ws3d(|ws, _| ws.tools.host.cam.set_size(w, h));
    }
    input(app, ui, &resp, rect);
    let Some(ed) = app.editor.as_mut() else { return };
    let tab = &mut app.tab3d;
    let painted = ed.eng.with_ws3d(|ws, doc| {
        // the lines on the ground: about one and a half pixels wide at the target (built again when the zoom moves on)
        let px = ws.tools.host.cam.pixel_size(ws.tools.host.cam.target);
        let want = (px * 1.5).max(1e-9);
        if tab.line_w <= 0.0 || (want / tab.line_w).ln().abs() > 0.5 {
            tab.line_w = want;
        }
        let key = (
            ws.tools.host.model.version().to_bits(),
            ws.tools.host.model_gen() ^ (ws.tools.host.path.len() as u64).wrapping_mul(0x9e37_79b9),
            doc.version,
            tab.line_w.to_bits(),
            dark,
        );
        if tab.built != Some(key) || tab.scene.is_none() {
            tab.seq = tab.seq.wrapping_add(1);
            tab.scene = Some(build(ws, doc, 0x3d00_0000 ^ tab.seq, dark, t.ink, tab.line_w));
            tab.built = Some(key);
        }
        let c = &ws.tools.host.cam;
        let cam = Cam3 {
            eye: c.eye,
            target: c.target,
            fov: c.fov,
            persp: c.persp,
            width: c.w * ppp,
            height: c.h * ppp,
            near: c.near,
            far: c.far,
        };
        let style = style3(&ws.style_key, &t);
        let sel2d = ed.hs.borrow().selection.clone();
        let extra = ws.selection_lines(doc, &sel2d);
        (cam, style, extra)
    });
    let Some((cam, style, extra)) = painted else { return };
    if let Some(sc) = app.tab3d.scene.as_ref() {
        crate::view3d::paint(ui, rect, sc, &cam, &style);
    }
    let Some(ed) = app.editor.as_ref() else { return };
    ed.eng.with_ws3d(|ws, _| overlay(ui, rect, ws, &extra, &t));
    cube(app, ui, rect);
    bar(app, ui, rect);
}

/// The overlay (ModelCraft's view3d.js drawOverlay): the selection in the open group, the tool's lines, polygon,
/// faces, grips, window, snap marker and tip; the 2D selection on the ground.
fn overlay(ui: &egui::Ui, rect: Rect, ws: &Ws3d, extra: &[cc_m3d_tools::overlay::Line], t: &Tokens) {
    let painter = ui.painter_at(rect);
    let cam = &ws.tools.host.cam;
    let to = |p: P3| -> Option<Pos2> { cam.project(p).map(|q| Pos2::new(rect.min.x + q[0] as f32, rect.min.y + q[1] as f32)) };
    let sel_col = Color32::from_rgb(0x2a, 0x6c, 0xf0);
    let h = &ws.tools.host;
    let ctx = cc_m3d_tools::tools::ctx_of(&h.model, &h.path);
    let ov = h.overlay.as_ref();
    let mut sels: Vec<(&Sel, Color32, f32)> = h.sel.iter().map(|s| (s, sel_col, 2.5)).collect();
    if let Some(Some(hv)) = ov.and_then(|o| o.hover.as_ref()) {
        sels.push((hv, Color32::from_rgb(0xff, 0xaa, 0x22), 2.0));
    }
    if let Some(many) = ov.and_then(|o| o.many.as_ref()) {
        sels.extend(many.iter().map(|s| (s, sel_col, 1.5)));
    }
    let wp = |m: &[f64; 16], p: P3| -> P3 {
        [
            m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
            m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
            m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
        ]
    };
    if let Some(def) = h.model.def(&ctx.def) {
        for (s, col, width) in sels {
            match s.t.as_str() {
                "e" => {
                    if let Some(e) = def.edges.get(s.id)
                        && let (Some(a), Some(b)) = (to(wp(&ctx.m, def.v(e.a))), to(wp(&ctx.m, def.v(e.b))))
                    {
                        painter.line_segment([a, b], Stroke::new(width, col));
                    }
                }
                "f" => {
                    if let Some(f) = def.faces.get(s.id) {
                        let fill = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 60);
                        for tri in face_tris(def, f) {
                            let pts: Vec<Pos2> = tri.iter().filter_map(|v| to(wp(&ctx.m, def.v(*v)))).collect();
                            if pts.len() == 3 {
                                painter.add(egui::Shape::convex_polygon(pts, fill, Stroke::NONE));
                            }
                        }
                    }
                }
                "i" => {
                    if let Some(inst) = def.insts.get(s.id)
                        && let Some(bb) = h.model.bbox(&inst.def, &IDENT)
                    {
                        let m = mmul(&ctx.m, &inst.m);
                        let c = |i: usize| -> P3 {
                            [
                                if i & 1 == 0 { bb.min[0] } else { bb.max[0] },
                                if i & 2 == 0 { bb.min[1] } else { bb.max[1] },
                                if i & 4 == 0 { bb.min[2] } else { bb.max[2] },
                            ]
                        };
                        for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
                            if let (Some(pa), Some(pb)) = (to(wp(&m, c(a))), to(wp(&m, c(b)))) {
                                painter.line_segment([pa, pb], Stroke::new(width.min(2.0), col));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let line = |l: &cc_m3d_tools::overlay::Line, w: f32| {
        if let (Some(a), Some(b)) = (to(l.a), to(l.b)) {
            let st = Stroke::new(w, kind_color(&l.kind, t));
            if l.dash {
                painter.extend(egui::Shape::dashed_line(&[a, b], st, 6.0, 4.0));
            } else {
                painter.line_segment([a, b], st);
            }
        }
    };
    for l in extra {
        line(l, 1.5);
    }
    let Some(o) = ov else { return };
    if let Some(fs) = &o.faces {
        let fill = Color32::from_rgba_unmultiplied(sel_col.r(), sel_col.g(), sel_col.b(), 50);
        for tri in fs {
            let pts: Vec<Pos2> = tri.iter().filter_map(|p| to(*p)).collect();
            if pts.len() == 3 {
                painter.add(egui::Shape::convex_polygon(pts, fill, Stroke::NONE));
            }
        }
    }
    for l in o.lines.iter().flatten() {
        line(l, 1.5);
    }
    if let Some(poly) = &o.poly {
        let pts: Vec<Pos2> = poly.iter().filter_map(|p| to(*p)).collect();
        if pts.len() >= 2 {
            let c = o.poly_kind.as_deref().map_or(t.text_strong, |k| kind_color(k, t));
            painter.add(egui::Shape::closed_line(pts, Stroke::new(1.5, c)));
        }
    }
    for p in o.points.iter().flatten().filter_map(|p| to(*p)) {
        painter.circle_filled(p, 2.5, t.text_strong);
    }
    for (i, p) in o.grips.iter().flatten().enumerate() {
        if let Some(q) = to(*p) {
            let hot = o.hot_grip.is_some_and(|h| h as usize == i);
            painter.rect_filled(
                Rect::from_center_size(q, egui::vec2(8.0, 8.0)),
                1.0,
                if hot { Color32::from_rgb(0xff, 0xaa, 0x22) } else { sel_col },
            );
        }
    }
    if let Some(r) = &o.rect {
        let rr = Rect::from_two_pos(
            Pos2::new(rect.min.x + r.x0 as f32, rect.min.y + r.y0 as f32),
            Pos2::new(rect.min.x + r.x1 as f32, rect.min.y + r.y1 as f32),
        );
        painter.rect_filled(rr, 0.0, Color32::from_rgba_unmultiplied(sel_col.r(), sel_col.g(), sel_col.b(), 24));
        let st = Stroke::new(1.0, sel_col);
        if r.crossing {
            let c = [rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()];
            painter.extend(egui::Shape::dashed_line(&c, st, 5.0, 4.0));
        } else {
            painter.rect_stroke(rr, 0.0, st, egui::StrokeKind::Inside);
        }
    }
    let tip = o.tip.as_deref().filter(|s| !s.is_empty());
    match &o.marker {
        Some(Some(m)) => {
            if let Some(p) = to(m.p) {
                let c = kind_color(&m.kind, t);
                painter.circle_stroke(p, 6.0, Stroke::new(2.0, c));
                painter.circle_filled(p, 2.5, c);
                if let Some(tip) = tip {
                    tip_box(&painter, p + egui::vec2(12.0, 12.0), tip, t);
                }
            }
        }
        _ => {
            if let (Some(tip), Some((x, y))) = (tip, ws.mouse) {
                tip_box(&painter, Pos2::new(rect.min.x + x as f32 + 12.0, rect.min.y + y as f32 + 12.0), tip, t);
            }
        }
    }
}

/// A tip on a filled box (view3d.js draws its tips over the view's background).
fn tip_box(painter: &egui::Painter, at: Pos2, text: &str, t: &Tokens) {
    let g = painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(12.0), t.text_strong);
    let r = Rect::from_min_size(at, g.size() + egui::vec2(8.0, 4.0));
    painter.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(t.panel_darker.r(), t.panel_darker.g(), t.panel_darker.b(), 230));
    painter.galley(at + egui::vec2(4.0, 2.0), g, t.text_strong);
}

/// The view cube (top right): the six sides as the camera sees them; a click looks from that side.
fn cube(app: &mut UiApp, ui: &mut egui::Ui, rect: Rect) {
    let Some(ed) = app.editor.as_ref() else { return };
    let Some(ax) = ed.eng.with_ws3d(|ws, _| ws.tools.host.cam.axes()) else { return };
    let t = app.tokens;
    let size = 36.0_f32;
    let c = Pos2::new(rect.max.x - 70.0, rect.min.y + 66.0);
    // screen position of a direction: camera x right, y up
    let to = |d: P3| -> Pos2 {
        let x = d[0] * ax.x[0] + d[1] * ax.x[1] + d[2] * ax.x[2];
        let y = d[0] * ax.y[0] + d[1] * ax.y[1] + d[2] * ax.y[2];
        Pos2::new(c.x + x as f32 * size, c.y - y as f32 * size)
    };
    let faces: [(&str, &str, P3, [P3; 2]); 6] = [
        ("top", "viewcube.top", [0.0, 0.0, 1.0], [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
        ("bottom", "viewcube.bottom", [0.0, 0.0, -1.0], [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0]]),
        ("front", "viewcube.front", [0.0, -1.0, 0.0], [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]),
        ("back", "viewcube.back", [0.0, 1.0, 0.0], [[-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]),
        ("left", "viewcube.left", [-1.0, 0.0, 0.0], [[0.0, -1.0, 0.0], [0.0, 0.0, 1.0]]),
        ("right", "viewcube.right", [1.0, 0.0, 0.0], [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
    ];
    let painter = ui.painter_at(rect);
    let mut clicked: Option<&str> = None;
    let (pos, click) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_clicked()));
    let facing = |n: P3| n[0] * ax.z[0] + n[1] * ax.z[1] + n[2] * ax.z[2];
    let mut order = faces;
    order.sort_by(|a, b| facing(a.2).partial_cmp(&facing(b.2)).unwrap_or(std::cmp::Ordering::Equal));
    for (name, key, n, [u, v]) in order {
        let f = facing(n);
        if f <= 0.02 {
            continue;
        }
        let corner = |su: f64, sv: f64| {
            to([
                n[0] * 0.5 + (u[0] * su + v[0] * sv) * 0.5,
                n[1] * 0.5 + (u[1] * su + v[1] * sv) * 0.5,
                n[2] * 0.5 + (u[2] * su + v[2] * sv) * 0.5,
            ])
        };
        let pts = vec![corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)];
        let shade = (150.0 + 90.0 * f) as u8;
        let fill = Color32::from_rgb(shade, shade, shade.saturating_add(8));
        let hit = pos.is_some_and(|p| egui::emath::Rect::from_points(&pts).contains(p));
        painter.add(egui::Shape::convex_polygon(
            pts.clone(),
            if hit { Color32::from_rgb(0x9f, 0xc0, 0xf5) } else { fill },
            Stroke::new(1.0, t.divider),
        ));
        if f > 0.35 {
            let label = crate::edit::lang_text(key, &[]);
            let label = if label == key { name.to_string() } else { label };
            let mid = to([n[0] * 0.5, n[1] * 0.5, n[2] * 0.5]);
            painter.text(
                mid,
                egui::Align2::CENTER_CENTER,
                label.to_uppercase(),
                egui::FontId::proportional(8.5),
                Color32::from_rgb(0x20, 0x24, 0x2a),
            );
        }
        if hit && click {
            clicked = Some(name);
        }
    }
    if let Some(n) = clicked {
        let _ = run(app, "tab3d.cube", &Params { name: Some(n.into()), ..Default::default() });
    }
}

/// The bar at the bottom of the view (ws3d.js `.cc-3d-bar`): the tool's tip, the visual style.
fn bar(app: &UiApp, ui: &egui::Ui, rect: Rect) {
    let Some(ed) = app.editor.as_ref() else { return };
    let Some((tip, style)) = ed.eng.with_ws3d(|ws, _| (ws.tools.host.tip.clone(), ws.style_key.clone())) else { return };
    let t = app.tokens;
    let painter = ui.painter_at(rect);
    let y = rect.max.y - 14.0;
    let style_label = crate::edit::lang_text(&format!("model3d.style.{style}"), &[]);
    painter.text(Pos2::new(rect.max.x - 8.0, y), egui::Align2::RIGHT_CENTER, style_label, egui::FontId::proportional(12.0), t.text_dim);
    if !tip.is_empty() {
        painter.text(Pos2::new(rect.min.x + 88.0, y), egui::Align2::LEFT_CENTER, tip, egui::FontId::proportional(12.0), t.text_dim);
    }
}

/// The pointer on the 3D view (ws3d.js): left to the workspace, middle / right to the camera, the wheel zooms.
fn input(app: &mut UiApp, ui: &egui::Ui, resp: &egui::Response, rect: Rect) {
    let (pos, mods, scroll, pressed, released, delta) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.modifiers,
            i.smooth_scroll_delta.y,
            [egui::PointerButton::Primary, egui::PointerButton::Secondary, egui::PointerButton::Middle]
                .map(|b| i.pointer.button_pressed(b)),
            [egui::PointerButton::Primary, egui::PointerButton::Secondary, egui::PointerButton::Middle]
                .map(|b| i.pointer.button_released(b)),
            i.pointer.delta(),
        )
    });
    let now = app.tab3d.now();
    let Some(ed) = app.editor.as_mut() else { return };
    let tab = &mut app.tab3d;
    let inside = pos.is_some_and(|p| rect.contains(p)) && resp.hovered();
    let ev = |p: Pos2| Ev3 {
        x: f64::from(p.x - rect.min.x),
        y: f64::from(p.y - rect.min.y),
        shift: mods.shift,
        ctrl: mods.command || mods.ctrl,
        alt: mods.alt,
        time: Some(now),
    };
    let mut changed = false;
    if inside && let Some(p) = pos {
        if pressed[0] {
            tab.held = Some(egui::PointerButton::Primary);
            ed.eng.ws_down(ev(p));
            changed = true;
        } else if pressed[1] || pressed[2] {
            tab.held = Some(if pressed[1] { egui::PointerButton::Secondary } else { egui::PointerButton::Middle });
            tab.press = Some(p);
            tab.cam_moved = false;
            if pressed[2] {
                // a middle double-click fits everything
                if tab.last_mid.is_some_and(|t| now - t < 400.0) {
                    ed.eng.with_ws3d(|ws, d| ws.fit(d, None));
                }
                tab.last_mid = Some(now);
            }
        }
    }
    match tab.held {
        Some(b @ (egui::PointerButton::Secondary | egui::PointerButton::Middle)) => {
            let (dx, dy) = (f64::from(delta.x), f64::from(delta.y));
            if let (Some(p0), Some(p)) = (tab.press, pos)
                && p0.distance(p) > 4.0
            {
                tab.cam_moved = true;
            }
            if dx != 0.0 || dy != 0.0 {
                let pan = b == egui::PointerButton::Secondary || mods.shift;
                ed.eng.with_ws3d(|ws, _| if pan { ws.tools.host.cam.pan(dx, dy) } else { ws.tools.host.cam.orbit(dx, dy) });
            }
        }
        _ => {
            if let Some(p) = pos
                && (inside || tab.held.is_some())
                && (delta.x != 0.0 || delta.y != 0.0)
            {
                ed.eng.ws_move(ev(p));
                changed = true;
            }
        }
    }
    if released.iter().any(|r| *r) {
        match tab.held {
            Some(egui::PointerButton::Primary) => {
                if let Some(p) = pos {
                    ed.eng.ws_up(ev(p));
                    changed = true;
                }
            }
            // a right click without dragging is Enter
            Some(egui::PointerButton::Secondary) if !tab.cam_moved => {
                ed.submit();
                changed = true;
            }
            _ => {}
        }
        tab.held = None;
        tab.press = None;
    }
    if inside && scroll != 0.0 {
        let p = pos.unwrap_or(rect.center()) - rect.min;
        let (x, y) = (f64::from(p.x), f64::from(p.y));
        ed.eng.with_ws3d(|ws, _| {
            // about the face under the cursor
            let ray = ws.tools.host.cam.ray(x, y);
            let sc = ws.tools.host.scene();
            let pivot = sc.hit(&ray, None).map(|h| h.p);
            let k = (-f64::from(scroll) * 0.0015 * 1.5).exp();
            ws.tools.host.cam.zoom_at(k, Some(x), Some(y), pivot);
        });
    }
    if changed {
        app.sync_editor();
    }
}
