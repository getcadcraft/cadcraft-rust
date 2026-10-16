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
            Key::Backspace => {
                self.vcb.pop();
            }
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
        });
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
