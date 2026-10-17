//! ModelCraft in the 3D view: the cc-m3d model edited through cc-m3d-tools (ModelCraft's tools.js, camera.js and
//! infer.js at parity), drawn through the shared 3D view, with the tools' overlay (inference markers, rubber bands,
//! previews, the selection window, grips, dimensions) painted on top the way view3d.js paints its overlay canvas.
//!
//! Mouse as ModelCraft's default controls: left = the tool, middle drag = orbit (Shift = pan), right drag = orbit,
//! wheel = zoom about the pointer. Keys: the tool letters (L line, R rectangle, C circle, P push/pull, M move, Q
//! rotate, S scale, E erase, T tape, Space select …), Esc, arrows lock an axis, Ctrl+Z / Ctrl+Y undo / redo; typing a
//! number fills the value box, Enter gives it to the tool.

use cc_m3d::def::face_tris;
use cc_m3d::model::Model;
use cc_m3d::ops::Sel;
use cc_m3d::vec::P3;
use cc_m3d_tools::tools::{Ev, Host, ToolArg, Tools};
use cc_m3d_tools::{Camera, overlay::Overlay};
use cc_render::mesh3d::{Cam3, Style3};
use egui::{Color32, Pos2, Rect, Sense, Stroke};

use crate::model3d;
use crate::theme::Tokens;
use crate::view3d::{self, Scene3D};

static EN: &str = include_str!("../../../assets/lang/modelcraft/en.json");

/// A word of ModelCraft's own catalog (English), `{name}` fills from `vars`; the key itself when missing.
pub fn mt(key: &str, vars: &[(&str, &str)]) -> String {
    static CAT: std::sync::OnceLock<serde_json::Map<String, serde_json::Value>> = std::sync::OnceLock::new();
    let cat =
        CAT.get_or_init(|| serde_json::from_str::<serde_json::Value>(EN).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default());
    let mut s = cat.get(key).and_then(serde_json::Value::as_str).unwrap_or(key).to_string();
    for (k, v) in vars {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// The tools on the palette: (tools.js name, label).
pub const PALETTE: &[(&str, &str)] = &[
    ("select", "Select"),
    ("line", "Line"),
    ("rect", "Rectangle"),
    ("circle", "Circle"),
    ("polygon", "Polygon"),
    ("arc", "Arc"),
    ("push", "Push/Pull"),
    ("offset", "Offset"),
    ("follow", "Follow me"),
    ("move", "Move"),
    ("rotate", "Rotate"),
    ("scale", "Scale"),
    ("erase", "Erase"),
    ("paint", "Paint"),
    ("tape", "Tape"),
    ("dim", "Dimension"),
    ("protractor", "Protractor"),
    ("section", "Section"),
];

/// One open ModelCraft model and its tools.
pub struct ModelView {
    pub tools: Tools,
    pub name: String,
    scene: Scene3D,
    /// (model version, model generation) the scene was built from
    built: (f64, u64),
    seq: u64,
    dark: bool,
    pub style: Style3,
    /// the button held in the view, if any
    held: Option<egui::PointerButton>,
    clock: web_time::Instant,
    /// the value box being typed
    pub vcb: String,
    /// the last message the tools printed
    pub message: String,
    /// the scene last gone to or saved (-1: none)
    pub scene_index: isize,
    /// a scene transition under way: (from, to, start time in ms)
    anim: Option<(Camera, Camera, f64)>,
}

/// How long a scene transition takes (goScene's 650 ms).
const SCENE_MS: f64 = 650.0;

/// What the Scenes panel asked for this frame.
enum SceneAct {
    Add,
    Go(usize),
    Update(usize),
    Delete(usize),
    Step(isize),
}

impl std::fmt::Debug for ModelView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelView").field("name", &self.name).field("tool", &self.tools.name).finish()
    }
}

/// The colour of an inference kind (view3d.js KIND_COLOR, in the theme's layer colours).
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

impl ModelView {
    pub fn new(model: Model, name: &str, dark: bool) -> Result<ModelView, String> {
        let tools = Tools::new(Host::new(model, Camera::new())).map_err(|e| format!("{e:?}"))?;
        let scene = model3d::scene_of(&tools.host.model, 1, dark);
        let mut v = ModelView {
            tools,
            name: name.to_string(),
            scene,
            built: (f64::NAN, 0),
            seq: 1,
            dark,
            style: Style3::default(),
            held: None,
            clock: web_time::Instant::now(),
            vcb: String::new(),
            message: String::new(),
            scene_index: -1,
            anim: None,
        };
        v.rebuild();
        v.fit();
        Ok(v)
    }

    /// The model's box as modelcraft.js takes it (model.bbox(): every vertex of every placement).
    fn model_box(&self, empty: Option<cc_m3d_tools::camera::Box3>) -> Option<cc_m3d_tools::camera::Box3> {
        match self.tools.host.model.bbox("root", &cc_m3d::vec::IDENT) {
            Some(b) => Some(cc_m3d_tools::camera::Box3 { min: b.min, max: b.max }),
            None => empty,
        }
    }
    /// Zoom Extents (modelcraft.js zoomExtents: cam.fit(model.bbox())).
    pub fn fit(&mut self) {
        let bb = self.model_box(None);
        self.tools.host.cam.fit(bb.as_ref(), None);
    }
    /// A standard view (modelcraft.js stdView: cam.standard(name, model.bbox() or a 10 m square)).
    pub fn standard(&mut self, name: &str) {
        let bb = self.model_box(Some(cc_m3d_tools::camera::Box3 { min: [-5.0, -5.0, 0.0], max: [5.0, 5.0, 3.0] }));
        self.tools.host.cam.standard(name, bb.as_ref());
    }

    pub fn set_dark(&mut self, dark: bool) {
        if dark != self.dark {
            self.dark = dark;
            self.built = (f64::NAN, 0);
        }
    }

    /// The scene again when the model changed (a new version, undo, another model).
    fn rebuild(&mut self) {
        let key = (self.tools.host.model.version(), self.tools.host.model_gen());
        if key.0 == self.built.0 && key.1 == self.built.1 {
            return;
        }
        self.seq = self.seq.wrapping_add(1);
        self.scene = model3d::scene_of(&self.tools.host.model, self.seq, self.dark);
        self.built = key;
    }

    fn now(&self) -> f64 {
        self.clock.elapsed().as_secs_f64() * 1000.0
    }

    fn ev(&self, p: Pos2, rect: Rect, m: egui::Modifiers) -> Ev {
        Ev {
            x: f64::from(p.x - rect.min.x),
            y: f64::from(p.y - rect.min.y),
            shift: m.shift,
            ctrl: m.command || m.ctrl,
            alt: m.alt,
            time: Some(self.now()),
            ..Ev::default()
        }
    }

    /// Choose a tool by its tools.js name.
    pub fn set_tool(&mut self, name: &str) {
        let _ = self.tools.set(name, ToolArg::None);
        self.vcb.clear();
    }

    pub fn undo(&mut self) {
        if self.tools.busy() {
            let _ = self.tools.escape();
        }
        if let Ok(Some(l)) = self.tools.host.model.undo() {
            self.message = format!("Undo {l}");
        }
        self.tools.host.sel.clear();
    }
    pub fn redo(&mut self) {
        if let Ok(Some(l)) = self.tools.host.model.redo() {
            self.message = format!("Redo {l}");
        }
    }

    /// Show the model in `rect`, feed the tools, draw the overlay.
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, t: &Tokens) {
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        self.tools.host.cam.set_size(f64::from(rect.width()), f64::from(rect.height()));
        self.input(ui, &resp, rect);
        self.step_anim(ui);
        self.rebuild();
        let c = &self.tools.host.cam;
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
        view3d::paint(ui, rect, &self.scene, &cam, &self.style);
        self.overlay(ui, rect, t);
        if let Some(m) = self.tools.host.msgs.pop() {
            self.message = m;
            self.tools.host.msgs.clear();
        }
    }

    fn input(&mut self, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect) {
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
        let inside = pos.is_some_and(|p| rect.contains(p));
        // presses that start in the view
        if inside && let Some(p) = pos {
            if pressed[0] {
                self.held = Some(egui::PointerButton::Primary);
                let e = self.ev(p, rect, mods);
                let _ = self.tools.down(&e);
            } else if pressed[1] {
                self.held = Some(egui::PointerButton::Secondary);
            } else if pressed[2] {
                self.held = Some(egui::PointerButton::Middle);
            }
        }
        match self.held {
            Some(egui::PointerButton::Secondary | egui::PointerButton::Middle) => {
                let (dx, dy) = (f64::from(delta.x), f64::from(delta.y));
                if dx != 0.0 || dy != 0.0 {
                    if mods.shift {
                        self.tools.host.cam.pan(dx, dy);
                    } else {
                        self.tools.host.cam.orbit(dx, dy);
                    }
                }
            }
            _ => {
                if let Some(p) = pos
                    && (inside || self.held.is_some())
                    && (delta.x != 0.0 || delta.y != 0.0 || pressed[0])
                {
                    let e = self.ev(p, rect, mods);
                    let _ = self.tools.mv(&e);
                }
            }
        }
        if released.iter().any(|r| *r) {
            if self.held == Some(egui::PointerButton::Primary)
                && let Some(p) = pos
            {
                let e = self.ev(p, rect, mods);
                let _ = self.tools.up(&e);
            }
            self.held = None;
        }
        if inside && scroll != 0.0 {
            let p = pos.unwrap_or(rect.center()) - rect.min;
            let factor = (-f64::from(scroll) * 0.002).exp();
            self.tools.host.cam.zoom_at(factor, Some(f64::from(p.x)), Some(f64::from(p.y)), None);
        }
        if !(resp.hovered() || self.held.is_some()) || ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        // keys
        let events: Vec<egui::Event> = ui.input(|i| i.events.clone());
        for ev in events {
            match ev {
                egui::Event::Key { key, pressed: true, modifiers, .. } => self.key(key, modifiers),
                egui::Event::Key { key: egui::Key::Escape, pressed: false, .. } => {}
                egui::Event::Text(s) => {
                    for ch in s.chars() {
                        if ch.is_ascii_digit() || ".,-+/*<>@[]'\"xXs ".contains(ch) || (!self.vcb.is_empty() && ch.is_ascii_alphabetic()) {
                            self.vcb.push(ch);
                        } else if let Some(name) = cc_m3d_tools::tools::tool_key(&ch.to_ascii_lowercase().to_string()) {
                            self.set_tool(name);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn key(&mut self, key: egui::Key, m: egui::Modifiers) {
        use egui::Key;
        if m.command && key == Key::Z {
            if m.shift {
                self.redo()
            } else {
                self.undo()
            }
            return;
        }
        if m.command && key == Key::Y {
            self.redo();
            return;
        }
        if key == Key::PageUp || key == Key::PageDown {
            self.step_scene(if key == Key::PageUp { -1 } else { 1 });
            return;
        }
        if m.command && key == Key::A {
            self.select_all();
            return;
        }
        if m.command && m.shift && key == Key::G {
            self.make_group("component");
            return;
        }
        if m.command && key == Key::G {
            self.make_group("group");
            return;
        }
        match key {
            Key::Escape => {
                if !self.vcb.is_empty() {
                    self.vcb.clear();
                } else if !self.tools.escape().unwrap_or(false) {
                    if self.tools.name == "select" {
                        self.tools.host.sel.clear();
                    } else {
                        self.set_tool("select");
                    }
                }
            }
            Key::Enter => {
                if !self.vcb.is_empty() {
                    let text = std::mem::take(&mut self.vcb);
                    let _ = self.tools.value(&text);
                }
            }
            Key::Backspace if !self.vcb.is_empty() => {
                self.vcb.pop();
            }
            Key::Delete | Key::Backspace if self.vcb.is_empty() && !self.tools.busy() => self.erase_selection(),
            Key::Delete => {
                let _ = self.tools.key("Delete");
            }
            Key::ArrowRight => {
                let _ = self.tools.key("ArrowRight");
            }
            Key::ArrowLeft => {
                let _ = self.tools.key("ArrowLeft");
            }
            Key::ArrowUp => {
                let _ = self.tools.key("ArrowUp");
            }
            Key::ArrowDown => {
                let _ = self.tools.key("ArrowDown");
            }
            _ => {}
        }
    }

    /// The overlay: selection, hover, previews, inference marker and tip.
    fn overlay(&self, ui: &egui::Ui, rect: Rect, t: &Tokens) {
        let painter = ui.painter_at(rect);
        let cam = &self.tools.host.cam;
        let to = |p: P3| -> Option<Pos2> { cam.project(p).map(|q| Pos2::new(rect.min.x + q[0] as f32, rect.min.y + q[1] as f32)) };
        let sel_col = Color32::from_rgb(0x2a, 0x6c, 0xf0);
        // the selection and the hover, in the open context
        let ctx = cc_m3d_tools::tools::ctx_of(&self.tools.host.model, &self.tools.host.path);
        let ov: Option<&Overlay> = self.tools.host.overlay.as_ref();
        let mut sels: Vec<(&Sel, Color32, f32)> = self.tools.host.sel.iter().map(|s| (s, sel_col, 2.5)).collect();
        if let Some(Some(h)) = ov.and_then(|o| o.hover.as_ref()) {
            sels.push((h, Color32::from_rgb(0xff, 0xaa, 0x22), 2.0));
        }
        if let Some(many) = ov.and_then(|o| o.many.as_ref()) {
            sels.extend(many.iter().map(|s| (s, sel_col, 1.5)));
        }
        if let Some(def) = self.tools.host.model.def(&ctx.def) {
            let w = |p: P3| -> P3 {
                let m = &ctx.m;
                [
                    m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
                    m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
                    m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
                ]
            };
            for (s, col, width) in sels {
                match s.t.as_str() {
                    "e" => {
                        if let Some(e) = def.edges.get(s.id)
                            && let (Some(a), Some(b)) = (to(w(def.v(e.a))), to(w(def.v(e.b))))
                        {
                            painter.line_segment([a, b], Stroke::new(width, col));
                        }
                    }
                    "f" => {
                        if let Some(f) = def.faces.get(s.id) {
                            let fill = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 60);
                            for tri in face_tris(def, f) {
                                let pts: Vec<Pos2> = tri.iter().filter_map(|v| to(w(def.v(*v)))).collect();
                                if pts.len() == 3 {
                                    painter.add(egui::Shape::convex_polygon(pts, fill, Stroke::NONE));
                                }
                            }
                            for l in &f.loops {
                                let pts: Vec<Pos2> = l.iter().filter_map(|v| to(w(def.v(*v)))).collect();
                                if pts.len() >= 2 {
                                    painter.add(egui::Shape::closed_line(pts, Stroke::new(width.min(1.5), col)));
                                }
                            }
                        }
                    }
                    "i" => {
                        // a group or component: its box, in the placement's frame
                        if let Some(inst) = def.insts.get(s.id)
                            && let Some(bb) = self.tools.host.model.bbox(&inst.def, &cc_m3d::vec::IDENT)
                        {
                            let m = cc_m3d::vec::mmul(&ctx.m, &inst.m);
                            let wp = |p: P3| -> P3 {
                                [
                                    m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
                                    m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
                                    m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
                                ]
                            };
                            let c = |i: usize| -> P3 {
                                [
                                    if i & 1 == 0 { bb.min[0] } else { bb.max[0] },
                                    if i & 2 == 0 { bb.min[1] } else { bb.max[1] },
                                    if i & 4 == 0 { bb.min[2] } else { bb.max[2] },
                                ]
                            };
                            for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
                                if let (Some(pa), Some(pb)) = (to(wp(c(a))), to(wp(c(b)))) {
                                    painter.line_segment([pa, pb], Stroke::new(width.min(2.0), col));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
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
        if let Some(ls) = &o.lines {
            for l in ls {
                if let (Some(a), Some(b)) = (to(l.a), to(l.b)) {
                    let st = Stroke::new(1.5, kind_color(&l.kind, t));
                    if l.dash {
                        painter.extend(egui::Shape::dashed_line(&[a, b], st, 6.0, 4.0));
                    } else {
                        painter.line_segment([a, b], st);
                    }
                }
            }
        }
        if let Some(poly) = &o.poly {
            let pts: Vec<Pos2> = poly.iter().filter_map(|p| to(*p)).collect();
            if pts.len() >= 2 {
                painter.add(egui::Shape::closed_line(pts, Stroke::new(1.5, t.text_strong)));
            }
        }
        if let Some(ps) = &o.points {
            for p in ps.iter().filter_map(|p| to(*p)) {
                painter.circle_filled(p, 2.5, t.text_strong);
            }
        }
        if let Some(gs) = &o.grips {
            for (i, p) in gs.iter().enumerate() {
                if let Some(q) = to(*p) {
                    let hot = o.hot_grip.is_some_and(|h| h as usize == i);
                    let r = Rect::from_center_size(q, egui::vec2(8.0, 8.0));
                    painter.rect_filled(r, 1.0, if hot { Color32::from_rgb(0xff, 0xaa, 0x22) } else { sel_col });
                }
            }
        }
        if let Some(r) = &o.rect {
            let a = Pos2::new(rect.min.x + r.x0 as f32, rect.min.y + r.y0 as f32);
            let b = Pos2::new(rect.min.x + r.x1 as f32, rect.min.y + r.y1 as f32);
            let rr = Rect::from_two_pos(a, b);
            painter.rect_filled(rr, 0.0, Color32::from_rgba_unmultiplied(sel_col.r(), sel_col.g(), sel_col.b(), 24));
            let st = Stroke::new(1.0, sel_col);
            if r.crossing {
                let c = [rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()];
                painter.extend(egui::Shape::dashed_line(&c, st, 5.0, 4.0));
            } else {
                painter.rect_stroke(rr, 0.0, st, egui::StrokeKind::Inside);
            }
        }
        if let Some(Some(m)) = &o.marker
            && let Some(p) = to(m.p)
        {
            let c = kind_color(&m.kind, t);
            painter.circle_stroke(p, 6.0, Stroke::new(2.0, c));
            painter.circle_filled(p, 2.5, c);
            if let Some(tip) = o.tip.as_deref().filter(|s| !s.is_empty()) {
                painter.text(p + egui::vec2(12.0, 12.0), egui::Align2::LEFT_TOP, tip, egui::FontId::proportional(12.0), c);
            }
        }
    }

    /// modelcraft.js makeGroup(kind): what is selected (not guides, dimensions or sections) into a new group / component.
    pub fn make_group(&mut self, kind: &str) {
        let sel: Vec<Sel> = self.tools.host.sel.iter().filter(|s| !matches!(s.t.as_str(), "g" | "d" | "s")).cloned().collect();
        if sel.is_empty() {
            self.message = mt("app.group.none", &[]);
            return;
        }
        let path = self.tools.host.path.clone();
        let label = mt(if kind == "group" { "app.undo.make_group" } else { "app.undo.make_component" }, &[]);
        let k = kind.to_string();
        let r = self.tools.host.model.change(&label, |m| {
            let def = m.context(&path).map(|c| c.def).unwrap_or_else(|| "root".into());
            m.make_group(&def, &sel, &k, None)
        });
        if let Ok(Some(iid)) = r {
            self.tools.host.set_sel(vec![Sel::new("i", iid)]);
            let ctx = cc_m3d_tools::tools::ctx_of(&self.tools.host.model, &self.tools.host.path);
            let name = self
                .tools
                .host
                .model
                .def(&ctx.def)
                .and_then(|d| d.insts.get(iid))
                .and_then(|i| self.tools.host.model.def(&i.def))
                .map(|d| d.name.clone())
                .unwrap_or_default();
            self.message = mt(if kind == "group" { "app.group.done" } else { "app.component.done" }, &[("name", &name)]);
        }
    }

    /// modelcraft.js selectAll: what is visible in the open context (soft edges left out).
    pub fn select_all(&mut self) {
        let ctx = cc_m3d_tools::tools::ctx_of(&self.tools.host.model, &self.tools.host.path);
        let Some(def) = self.tools.host.model.def(&ctx.def) else { return };
        let mut sel: Vec<Sel> = def.faces.iter().filter(|(_, f)| !f.hidden).map(|(id, _)| Sel::new("f", id)).collect();
        sel.extend(def.edges.iter().filter(|(_, e)| !e.soft && !e.hidden).map(|(id, _)| Sel::new("e", id)));
        sel.extend(def.insts.iter().filter(|(_, i)| !i.hidden).map(|(id, _)| Sel::new("i", id)));
        self.tools.host.set_sel(sel);
    }

    /// modelcraft.js explodeSel: the selected groups and components back into their contents.
    pub fn explode_sel(&mut self) {
        let insts: Vec<f64> = self.tools.host.sel.iter().filter(|s| s.t == "i").map(|s| s.id).collect();
        if insts.is_empty() {
            self.message = mt("app.explode.none", &[]);
            return;
        }
        let path = self.tools.host.path.clone();
        let out = self.tools.host.model.change(&mt("app.undo.explode", &[]), |m| {
            let def = m.context(&path).map(|c| c.def).unwrap_or_else(|| "root".into());
            let mut all = Vec::new();
            for iid in &insts {
                all.extend(m.explode(&def, *iid)?);
            }
            Ok(all)
        });
        self.tools.host.set_sel(out.unwrap_or_default());
    }

    /// modelcraft.js eraseSelection (Delete): one undo step, then purge.
    pub fn erase_selection(&mut self) {
        if self.tools.host.sel.is_empty() {
            return;
        }
        let sel = self.tools.host.sel.clone();
        let path = self.tools.host.path.clone();
        let _ = self.tools.host.model.change(&mt("app.undo.erase", &[]), |m| {
            let def = m.context(&path).map(|c| c.def).unwrap_or_else(|| "root".into());
            let geo: Vec<Sel> = sel.iter().filter(|s| !matches!(s.t.as_str(), "g" | "d" | "s")).cloned().collect();
            if let Some(d) = m.defs.get_mut(&def) {
                cc_m3d::ops::erase_sel(d, &geo);
            }
            for s in &sel {
                match s.t.as_str() {
                    "g" => {
                        m.guides.delete(s.id);
                    }
                    "d" => {
                        m.dims.delete(s.id);
                    }
                    "s" => m.delete_section(s.id),
                    _ => {}
                }
            }
            m.purge();
            Ok(())
        });
        self.tools.host.set_sel(Vec::new());
    }

    /// The style a scene keeps (cleanScene's mode / edges), from the view's.
    fn style_mode(&self) -> &'static str {
        if !self.style.faces {
            "wire"
        } else if self.style.xray {
            "xray"
        } else if self.style.hidden_line {
            "hidden"
        } else {
            "shaded"
        }
    }

    fn scene_here(&self, name: String) -> cc_m3d::model::Scene {
        let c = &self.tools.host.cam;
        let d = cc_m3d::model::Scene {
            vis: Some(self.tools.host.model.capture_vis()),
            name,
            eye: c.eye,
            target: c.target,
            fov: c.fov,
            persp: c.persp,
            height: c.height(),
            mode: self.style_mode().to_string(),
            edges: self.style.edges,
            shadows: false,
            month: 6.0,
            day: 21.0,
            hour: 15.0,
            lat: 40.0,
            lon: -105.0,
            tz: -7.0,
        };
        // the sun keeps what the file has (this view has no sun settings yet)
        match self.tools.host.model.scenes.first() {
            Some(f) => {
                cc_m3d::model::Scene { shadows: f.shadows, month: f.month, day: f.day, hour: f.hour, lat: f.lat, lon: f.lon, tz: f.tz, ..d }
            }
            None => d,
        }
    }

    /// modelcraft.js "Add scene": this camera, style and what is hidden, as a new scene.
    pub fn add_scene(&mut self) {
        let name = mt("app.scene.default", &[("n", &(self.tools.host.model.scenes.len() + 1).to_string())]);
        let sc = self.scene_here(name.clone());
        let _ = self.tools.host.model.change(&mt("ui.scenes.add", &[]), |m| {
            m.scenes.push(sc);
            Ok(())
        });
        self.scene_index = self.tools.host.model.scenes.len() as isize - 1;
        self.message = mt("app.scene.saved", &[("name", &name)]);
    }

    /// goScene(i) without the animation: the scene's style, what it shows and its camera.
    pub fn go_scene(&mut self, i: usize) {
        self.go_scene_with(i, true);
    }

    /// goScene(i, animate).
    pub fn go_scene_with(&mut self, i: usize, animate: bool) {
        let Some(sc) = self.tools.host.model.scenes.get(i).cloned() else { return };
        self.scene_index = i as isize;
        self.style.faces = sc.mode != "wire";
        self.style.xray = sc.mode == "xray";
        self.style.hidden_line = sc.mode == "hidden";
        self.style.edges = sc.edges;
        if let Some(v) = &sc.vis {
            self.tools.host.model.apply_vis(v);
            self.tools.host.sel.retain(|x| x.t != "i");
        }
        // goScene: move there over 650 ms (smoothstep), unless asked to go straight there
        let from = self.tools.host.cam.clone();
        let mut to = from.clone();
        to.set(sc.eye, sc.target, Some(sc.fov), Some(sc.persp));
        if animate {
            self.anim = Some((from, to, self.now()));
        } else {
            self.anim = None;
            self.tools.host.cam = to;
        }
        self.message = mt("app.scene.go", &[("name", &sc.name)]);
    }

    /// One step of a scene transition (any input in the view ends it where it is).
    fn step_anim(&mut self, ui: &egui::Ui) {
        let Some((from, to, t0)) = &self.anim else { return };
        if self.held.is_some() {
            self.anim = None;
            return;
        }
        let k = ((self.now() - t0) / SCENE_MS).clamp(0.0, 1.0);
        let (eye, target, fov, persp) = cc_m3d_tools::camera::lerp_cam(from, to, k);
        let (w, h) = (self.tools.host.cam.w, self.tools.host.cam.h);
        if k >= 1.0 {
            self.tools.host.cam = to.clone();
            self.anim = None;
        } else {
            self.tools.host.cam.set(eye, target, Some(fov), Some(persp));
            ui.ctx().request_repaint();
        }
        self.tools.host.cam.set_size(w, h);
    }

    /// Go straight to the end of a scene transition (scripts, shots).
    pub fn finish_anim(&mut self) {
        if let Some((_, to, _)) = self.anim.take() {
            let (w, h) = (self.tools.host.cam.w, self.tools.host.cam.h);
            self.tools.host.cam = to;
            self.tools.host.cam.set_size(w, h);
        }
    }

    /// Whether a scene transition is under way.
    pub fn animating(&self) -> bool {
        self.anim.is_some()
    }

    /// stepScene(d): the next / previous scene, wrapping round.
    pub fn step_scene(&mut self, d: isize) {
        let n = self.tools.host.model.scenes.len() as isize;
        if n == 0 {
            return;
        }
        let from = if self.scene_index < 0 { if d > 0 { -1 } else { 0 } } else { self.scene_index };
        self.go_scene((from + d).rem_euclid(n) as usize);
    }

    /// updateScene(i): the current camera, style and visibility saved into scene i.
    pub fn update_scene(&mut self, i: usize) {
        let Some(name) = self.tools.host.model.scenes.get(i).map(|x| x.name.clone()) else { return };
        let sc = self.scene_here(name.clone());
        let _ = self.tools.host.model.change(&mt("app.undo.scene_update", &[]), |m| {
            if let Some(x) = m.scenes.get_mut(i) {
                *x = sc;
            }
            Ok(())
        });
        self.scene_index = i as isize;
        self.message = mt("app.scene.updated", &[("name", &name)]);
    }

    /// deleteScene(i).
    pub fn delete_scene(&mut self, i: usize) {
        let Some(name) = self.tools.host.model.scenes.get(i).map(|x| x.name.clone()) else { return };
        let _ = self.tools.host.model.change(&mt("app.undo.scene_delete", &[]), |m| {
            if i < m.scenes.len() {
                m.scenes.remove(i);
            }
            Ok(())
        });
        let n = self.tools.host.model.scenes.len() as isize;
        if self.scene_index >= n {
            self.scene_index = n - 1;
        }
        self.message = mt("app.scene.deleted", &[("name", &name)]);
    }

    /// The Components panel's Place: the Place tool with that definition.
    pub fn place_component(&mut self, def_id: &str) {
        let name = self.tools.host.model.def(def_id).map(|d| d.name.clone()).unwrap_or_default();
        let _ = self.tools.set("place", ToolArg::Place { def_id: def_id.to_string(), align: true, hint: None });
        self.message = mt("app.comp.place.where", &[("name", &name)]);
    }

    /// Show or hide a tag (modelcraft.js showTag): one undo step; what hides cannot stay selected, and a group being
    /// edited inside closes above the first hidden one.
    pub fn show_tag(&mut self, name: &str, on: bool) {
        let _ = self.tools.escape();
        let label = mt(if on { "app.undo.tag_show" } else { "app.undo.tag_hide" }, &[]);
        let n = name.to_string();
        let _ = self.tools.host.model.change(&label, |m| {
            m.set_tag_hidden(&n, !on);
            Ok(())
        });
        if !on {
            let mut k = 0;
            let mut def = "root".to_string();
            while k < self.tools.host.path.len() {
                let Some(iid) = self.tools.host.path.get(k).copied() else { break };
                let Some(inst) = self.tools.host.model.def(&def).and_then(|d| d.insts.get(iid)) else { break };
                if inst.tag.as_deref() == Some(name) {
                    break;
                }
                def = inst.def.clone();
                k += 1;
            }
            while self.tools.host.path.len() > k {
                self.tools.host.close_context();
            }
            let ctx = cc_m3d_tools::tools::ctx_of(&self.tools.host.model, &self.tools.host.path);
            let d = self.tools.host.model.def(&ctx.def);
            self.tools
                .host
                .sel
                .retain(|s| !(s.t == "i" && d.and_then(|d| d.insts.get(s.id)).is_some_and(|i| i.tag.as_deref() == Some(name))));
        }
        self.message = mt(if on { "app.tag.shown" } else { "app.tag.hidden" }, &[("name", name)]);
    }

    /// The right-hand panel: Entity Info, Outliner, Tags.
    pub fn side_panel(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        use egui::RichText;
        let ctx = cc_m3d_tools::tools::ctx_of(&self.tools.host.model, &self.tools.host.path);
        let mut select: Option<Vec<Sel>> = None;
        let mut open: Option<f64> = None;
        let mut close = false;
        let mut tag_toggle: Option<(String, bool)> = None;
        let mut scene_act: Option<SceneAct> = None;
        let mut place: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Entity Info
            egui::CollapsingHeader::new(RichText::new("Entity Info").color(t.text_strong)).default_open(true).show(ui, |ui| {
                let sel = &self.tools.host.sel;
                let model = &self.tools.host.model;
                let def = model.def(&ctx.def);
                if sel.is_empty() {
                    ui.label(RichText::new("Nothing selected").color(t.text_dim));
                } else if sel.len() > 1 {
                    let count = |k: &str| sel.iter().filter(|s| s.t == k).count();
                    ui.label(format!(
                        "{} selected: {} faces, {} edges, {} groups or components",
                        sel.len(),
                        count("f"),
                        count("e"),
                        count("i")
                    ));
                } else if let (Some(s), Some(d)) = (sel.first(), def) {
                    let scale = |v: f64| self.tools.host.fmt(v);
                    match s.t.as_str() {
                        "f" => {
                            if let Some(f) = d.faces.get(s.id) {
                                ui.label(RichText::new("Face").strong());
                                let area = cc_m3d::def::face_area(d, f);
                                ui.label(format!("Area: {}", cc_m3d_tools::units::format_area(area, &self.tools.host.units)));
                                if let Some(c) = &f.color {
                                    ui.label(format!("Paint: {c}"));
                                }
                            }
                        }
                        "e" => {
                            if let Some(e) = d.edges.get(s.id) {
                                let (a, b) = (d.v(e.a), d.v(e.b));
                                let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt();
                                ui.label(RichText::new("Edge").strong());
                                ui.label(format!("Length: {}", scale(len)));
                                if e.soft {
                                    ui.label("Soft");
                                }
                            }
                        }
                        "i" => {
                            if let Some(inst) = d.insts.get(s.id) {
                                let sub = model.def(&inst.def);
                                let kind = sub.map_or("group", |x| x.kind.as_str());
                                let name =
                                    inst.name.clone().filter(|n| !n.is_empty()).or_else(|| sub.map(|x| x.name.clone())).unwrap_or_default();
                                ui.label(RichText::new(if kind == "component" { "Component" } else { "Group" }).strong());
                                ui.label(format!("Name: {name}"));
                                if let Some(tag) = inst.tag.as_deref().filter(|x| !x.is_empty()) {
                                    ui.label(format!("Tag: {tag}"));
                                }
                                if let Some(x) = sub {
                                    ui.label(format!("{} faces, {} edges", x.faces.len(), x.edges.len()));
                                }
                            }
                        }
                        other => {
                            ui.label(format!("Selected: {other}"));
                        }
                    }
                }
            });
            // Edit: grouping and erasing what is selected; the Paint tool's colour
            let mut act: Option<&str> = None;
            ui.horizontal_wrapped(|ui| {
                let any = !self.tools.host.sel.is_empty();
                if ui.add_enabled(any, egui::Button::new("Make group")).clicked() {
                    act = Some("group");
                }
                if ui.add_enabled(any, egui::Button::new("Make component")).clicked() {
                    act = Some("component");
                }
                if ui.add_enabled(self.tools.host.sel.iter().any(|s| s.t == "i"), egui::Button::new("Explode")).clicked() {
                    act = Some("explode");
                }
                if ui.add_enabled(any, egui::Button::new("Delete")).clicked() {
                    act = Some("erase");
                }
            });
            match act {
                Some("group") => self.make_group("group"),
                Some("component") => self.make_group("component"),
                Some("explode") => self.explode_sel(),
                Some("erase") => self.erase_selection(),
                _ => {}
            }
            if self.tools.name == "paint" {
                ui.label(RichText::new("Paint colour").color(t.text_strong));
                ui.horizontal_wrapped(|ui| {
                    for (_name, hex) in cc_m3d::paint::PALETTE {
                        let c = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0);
                        let col = Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
                        let on = self.tools.paint_color.as_deref() == Some(hex);
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::click());
                        ui.painter().rect_filled(r, 2.0, col);
                        if on {
                            ui.painter().rect_stroke(r, 2.0, Stroke::new(2.0, t.text_strong), egui::StrokeKind::Outside);
                        }
                        if resp.clicked() {
                            self.tools.paint_color = Some(hex.to_string());
                        }
                    }
                });
            }
            // Outliner: the open context's groups and components
            egui::CollapsingHeader::new(RichText::new("Outliner").color(t.text_strong)).default_open(true).show(ui, |ui| {
                if !self.tools.host.path.is_empty() && ui.small_button("Close group").clicked() {
                    close = true;
                }
                let model = &self.tools.host.model;
                if let Some(d) = model.def(&ctx.def) {
                    if d.insts.is_empty() {
                        ui.label(RichText::new("No groups or components here").color(t.text_dim));
                    }
                    for (iid, inst) in d.insts.iter() {
                        let sub = model.def(&inst.def);
                        let kind = sub.map_or("group", |x| x.kind.as_str());
                        let name = inst.name.clone().filter(|n| !n.is_empty()).or_else(|| sub.map(|x| x.name.clone())).unwrap_or_default();
                        let label = format!(
                            "{} {}",
                            if kind == "component" { "◆" } else { "▢" },
                            if name.is_empty() { "(unnamed)".into() } else { name }
                        );
                        let on = self.tools.host.sel.iter().any(|s| s.t == "i" && s.id == iid);
                        let r = ui.selectable_label(on, RichText::new(label).color(if inst.hidden { t.text_dim } else { t.text }));
                        if r.clicked() {
                            select = Some(vec![Sel::new("i", iid)]);
                        }
                        if r.double_clicked() {
                            open = Some(iid);
                        }
                    }
                }
            });
            // Tags
            egui::CollapsingHeader::new(RichText::new("Tags").color(t.text_strong)).default_open(false).show(ui, |ui| {
                let counts = self.tools.host.model.tag_counts();
                if self.tools.host.model.tags.is_empty() {
                    ui.label(RichText::new("No tags").color(t.text_dim));
                }
                for g in &self.tools.host.model.tags {
                    let n = counts.iter().find(|c| c.0 == g.name).map_or(0.0, |c| c.1);
                    let mut on = !g.hidden;
                    if ui.checkbox(&mut on, format!("{}  ({n})", g.name)).changed() {
                        tag_toggle = Some((g.name.clone(), on));
                    }
                }
            });
            // Components: every component definition, how many are placed, Place
            egui::CollapsingHeader::new(RichText::new(mt("ui.comps", &[])).color(t.text_strong)).default_open(false).show(ui, |ui| {
                let model = &self.tools.host.model;
                let counts = model.instance_counts();
                let mut any = false;
                for d in model.defs.values().filter(|d| d.kind == "component") {
                    any = true;
                    ui.horizontal(|ui| {
                        ui.label(&d.name);
                        ui.label(RichText::new(format!("{}×", counts.get(&d.id).copied().unwrap_or(0.0))).color(t.text_dim));
                        if ui.small_button(mt("app.comp.place", &[])).clicked() {
                            place = Some(d.id.clone());
                        }
                    });
                }
                if !any {
                    ui.label(RichText::new(mt("ui.comps.empty", &[("key", "Ctrl+Shift+G")])).color(t.text_dim));
                }
            });
            // Scenes: saved cameras with their style and what they hide
            egui::CollapsingHeader::new(RichText::new(mt("ui.scenes", &[])).color(t.text_strong)).default_open(true).show(ui, |ui| {
                let scenes = &self.tools.host.model.scenes;
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button(mt("ui.scenes.add", &[])).clicked() {
                        scene_act = Some(SceneAct::Add);
                    }
                    if !scenes.is_empty() {
                        if ui.small_button(mt("ui.scenes.prev", &[])).clicked() {
                            scene_act = Some(SceneAct::Step(-1));
                        }
                        if ui.small_button(mt("ui.scenes.next", &[])).clicked() {
                            scene_act = Some(SceneAct::Step(1));
                        }
                    }
                });
                if scenes.is_empty() {
                    ui.label(RichText::new(mt("ui.scenes.empty", &[("up", "Page Up"), ("down", "Page Down")])).color(t.text_dim));
                }
                for (i, sc) in scenes.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let cur = i as isize == self.scene_index;
                        if ui.selectable_label(cur, &sc.name).on_hover_text(mt("app.scene.go.title", &[])).clicked() {
                            scene_act = Some(SceneAct::Go(i));
                        }
                        if ui
                            .small_button(mt("app.scene.update.aria", &[("name", "")]).trim())
                            .on_hover_text(mt("app.scene.update.title", &[("name", &sc.name)]))
                            .clicked()
                        {
                            scene_act = Some(SceneAct::Update(i));
                        }
                        if ui.small_button(mt("app.scene.delete", &[])).clicked() {
                            scene_act = Some(SceneAct::Delete(i));
                        }
                    });
                }
            });
        });
        if let Some(a) = scene_act {
            match a {
                SceneAct::Add => self.add_scene(),
                SceneAct::Go(i) => self.go_scene(i),
                SceneAct::Update(i) => self.update_scene(i),
                SceneAct::Delete(i) => self.delete_scene(i),
                SceneAct::Step(d) => self.step_scene(d),
            }
        }
        if let Some(d) = place {
            self.place_component(&d);
        }
        if let Some(s) = select {
            self.tools.host.set_sel(s);
        }
        if let Some(iid) = open {
            let mut p = self.tools.host.path.clone();
            p.push(iid);
            self.tools.host.open_context(p);
        }
        if close {
            self.tools.host.close_context();
        }
        if let Some((n, on)) = tag_toggle {
            self.show_tag(&n, on);
        }
    }

    /// What the status bar shows: the tool's tip, the value box (label + what is typed), the last message.
    pub fn status(&self) -> String {
        let (label, text) = &self.tools.host.vcb;
        let typed = if self.vcb.is_empty() { text.clone() } else { self.vcb.clone() };
        let mut out = self.tools.host.tip.clone();
        if !label.is_empty() {
            out.push_str(&format!("   {label}: {typed}"));
        }
        if !self.message.is_empty() {
            out.push_str(&format!("   · {}", self.message));
        }
        out
    }
}
