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

/// A word of ModelCraft's own catalog (English) as i18n.js t() gives it: `{name}` fills from `vars`, a plural entry
/// picks "one" when the fill `n` is exactly 1 (Intl.PluralRules('en')), else "other"; a key the catalog lacks comes
/// from the tools' and the kernel's catalogs, else the key itself.
pub fn mt(key: &str, vars: &[(&str, &str)]) -> String {
    static CAT: std::sync::OnceLock<serde_json::Map<String, serde_json::Value>> = std::sync::OnceLock::new();
    let cat =
        CAT.get_or_init(|| serde_json::from_str::<serde_json::Value>(EN).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default());
    match cat.get(key) {
        Some(serde_json::Value::String(s)) => cc_m3d::i18n::fill(s, vars),
        Some(serde_json::Value::Object(forms)) => {
            let n = vars.iter().find(|(k, _)| *k == "n").map(|(_, v)| *v);
            let exact = n.and_then(|n| forms.get(&format!("={n}"))).and_then(serde_json::Value::as_str);
            let one = n.is_some_and(|v| v.trim().parse::<f64>().is_ok_and(|x| x == 1.0));
            let text = exact
                .or_else(|| forms.get(if one { "one" } else { "other" }).and_then(serde_json::Value::as_str))
                .or_else(|| forms.get("other").and_then(serde_json::Value::as_str))
                .unwrap_or(key);
            cc_m3d::i18n::fill(text, vars)
        }
        _ => cc_m3d_tools::i18n::t(key, vars),
    }
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
    pub(crate) anim: Option<(Camera, Camera, f64)>,
    /// view3d.js style: mode (shaded, hidden, wire, xray), edges, shadows, sun [month, day, hour, lat, lon, tz]
    pub mode: String,
    pub edges: bool,
    pub shadows: bool,
    pub sun: [f64; 6],
    /// the open context the page last saw (the tools may open or close one themselves)
    pub(crate) seen_path: Vec<f64>,
    /// the Outliner's search (lower case)
    pub tree_filter: String,
    /// the Soften box's angle
    pub soften_deg: f64,
    /// the last Check for 3D printing
    pub print_report: Option<crate::mcprint::PrintReport>,
    /// a file to hand to the host's save / download (name, bytes)
    pub pending_save: Option<(String, Vec<u8>)>,
    /// what the panels' text boxes hold while typed (box id → text)
    pub(crate) boxes: std::collections::HashMap<String, String>,
    /// the Outliner row being renamed (definition id)
    pub(crate) renaming: Option<String>,
    /// the modifier keys last seen
    mods: egui::Modifiers,
    /// Entity info's heading as last laid out
    pub(crate) info_heading: std::cell::RefCell<String>,
    /// the clipboard (model.copyClip's JSON), the pointer over the view, the 3D text dialog and the last text placed,
    /// the time given to scripted clicks (mcedit.rs)
    pub clip: Option<serde_json::Value>,
    pub mouse: Option<(f64, f64)>,
    pub text_dlg: Option<crate::mcedit::Text3dDlg>,
    pub text_last: crate::mcedit::TextLast,
    pub(crate) click_clock: f64,
    /// the camera's size pinned by `view-size` (harnesses), else the view's
    pub(crate) pinned_size: Option<(f64, f64)>,
    /// the IFC storeys dialog; Subtract's cutter while its two clicks are under way (Some(None): none yet) (mcsolids.rs)
    pub storeys_dlg: Option<crate::mcsolids::StoreysDlg>,
    pub(crate) sub_cutter: Option<Option<f64>>,
    /// the Stretch this copy dialog (mcstretch.rs)
    pub stretch_dlg: Option<crate::mcstretch::StretchDlg>,
    /// the lamp picked in the Lights panel (mclights.rs)
    pub sel_light: Option<f64>,
    /// Render a picture…: the dialog and the settings it keeps while the app runs (mcrender.rs)
    pub render_dlg: Option<crate::mcrender::RenderDlg>,
    pub render_settings: cc_m3d_tools::looks::Settings,
}

/// view3d.js's default sun: 21 June, 15:00, 40° N 105° W, UTC−7.
pub const SUN0: [f64; 6] = [6.0, 21.0, 15.0, 40.0, -105.0, -7.0];

/// How long a scene transition takes (goScene's 650 ms).
const SCENE_MS: f64 = 650.0;

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
        let built = (tools.host.model.version(), tools.host.model_gen());
        let mut v = ModelView {
            tools,
            name: name.to_string(),
            scene,
            built,
            seq: 1,
            dark,
            style: Style3::default(),
            held: None,
            clock: web_time::Instant::now(),
            vcb: String::new(),
            message: String::new(),
            scene_index: -1,
            anim: None,
            mode: "shaded".into(),
            edges: true,
            shadows: false,
            sun: SUN0,
            seen_path: Vec::new(),
            tree_filter: String::new(),
            soften_deg: 20.0,
            print_report: None,
            pending_save: None,
            boxes: std::collections::HashMap::new(),
            renaming: None,
            mods: egui::Modifiers::NONE,
            info_heading: std::cell::RefCell::new(String::new()),
            clip: None,
            mouse: None,
            text_dlg: None,
            text_last: crate::mcedit::TextLast::default(),
            click_clock: 0.0,
            pinned_size: None,
            storeys_dlg: None,
            sub_cutter: None,
            stretch_dlg: None,
            sel_light: None,
            render_dlg: None,
            render_settings: cc_m3d_tools::looks::Settings::default(),
        };
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

    pub(crate) fn now(&self) -> f64 {
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

    /// An action from a key: its error becomes the status.
    fn run_act(&mut self, a: &str) {
        if let Err(e) = self.act(a) {
            self.message = e;
        }
    }

    /// Choose a tool by its tools.js name.
    pub fn set_tool(&mut self, name: &str) {
        let _ = self.tools.set(name, ToolArg::None);
        self.vcb.clear();
    }

    /// modelcraft.js act('undo'): Esc, undo, say what, afterHistory.
    pub fn undo(&mut self) {
        let _ = self.tools.escape();
        let l = self.tools.host.model.undo().ok().flatten();
        self.message = match l {
            Some(l) => mt("app.undid", &[("what", &l)]),
            None => mt("app.undo.nothing", &[]),
        };
        self.after_history();
    }
    /// modelcraft.js act('redo').
    pub fn redo(&mut self) {
        let _ = self.tools.escape();
        let l = self.tools.host.model.redo().ok().flatten();
        self.message = match l {
            Some(l) => mt("app.redid", &[("what", &l)]),
            None => mt("app.redo.nothing", &[]),
        };
        self.after_history();
    }

    /// The renderer's style from view3d.js's (wire draws its edges whatever the switch says).
    pub fn apply_style(&mut self) {
        self.style.faces = self.mode != "wire";
        self.style.xray = self.mode == "xray";
        self.style.hidden_line = self.mode == "hidden";
        self.style.edges = self.edges || self.mode == "wire";
        // view3d.js: library materials are textured in the shaded style
        self.style.textures = self.mode == "shaded";
        // updateLights: with shadows the sun of the style's date, time and place lights the model and casts shadows
        self.style.sun = self.shadows.then(|| {
            let [month, day, hour, lat, lon, tz] = self.sun;
            let s = cc_m3d_tools::sun::sun_position(&cc_m3d_tools::sun::When { month, day, hour, lat, lon, tz, year: 2026.0 });
            cc_render::mesh3d::Sun3 { dir: s.dir, up: s.up }
        });
    }

    /// uiToStyle's sun: [month, day, hour, lat, lon, tz], each clamped; a value that is not a number keeps the old one.
    pub fn set_sun(&mut self, v: &[f64]) {
        let lim = [(1.0, 12.0), (1.0, 31.0), (0.0, 23.99), (-90.0, 90.0), (-180.0, 180.0), (-14.0, 14.0)];
        for (k, (lo, hi)) in lim.iter().enumerate() {
            if let (Some(x), Some(slot)) = (v.get(k).copied().filter(|x| x.is_finite()), self.sun.get_mut(k)) {
                let c = x.clamp(*lo, *hi);
                // `| 0` for the month and the day
                *slot = if k < 2 { c.trunc() } else { c };
            }
        }
        self.apply_style();
    }

    /// The file's text as saveModel writes it (modelText): the model, the view (camera and style), imported images.
    pub fn save_text(&mut self) -> String {
        let mut o = cc_m3d::json::Out::new(false);
        self.tools.host.model.to_json_out(&mut o, false);
        if o.s.ends_with('}') {
            o.s.pop();
            let c = &self.tools.host.cam;
            o.raw(",\"view\":{\"cam\":{\"eye\":");
            o.p3(&c.eye);
            o.raw(",\"target\":");
            o.p3(&c.target);
            o.raw(",\"fov\":");
            o.num(c.fov);
            o.raw(",\"persp\":");
            o.bool(c.persp);
            o.raw(",\"height\":");
            o.num(c.height());
            o.raw("},\"style\":{\"mode\":");
            o.str(&self.mode);
            o.raw(",\"edges\":");
            o.bool(self.edges);
            o.raw(",\"shadows\":");
            o.bool(self.shadows);
            for (k, (name, x)) in ["month", "day", "hour", "lat", "lon", "tz"].iter().zip(self.sun).enumerate() {
                o.raw(if k == 0 { ",\"sun\":{\"" } else { ",\"" });
                o.raw(name);
                o.raw("\":");
                o.num(x);
            }
            o.raw("}}}");
            let imgs = self.tools.host.model.images_json();
            if !imgs.is_empty() {
                o.raw(",\"images\":[");
                for (i, (id, im)) in imgs.iter().enumerate() {
                    if i > 0 {
                        o.ch(',');
                    }
                    o.raw("{\"id\":");
                    o.str(id);
                    o.raw(",\"name\":");
                    o.str(&im.name);
                    o.raw(",\"mime\":");
                    o.str(&im.mime);
                    o.raw(",\"data\":");
                    o.str(&im.data);
                    o.raw(",\"avg\":");
                    match &im.avg {
                        Some(a) => o.str(a),
                        None => o.raw("null"),
                    }
                    o.ch('}');
                }
                o.ch(']');
            }
            o.ch('}');
        }
        o.s
    }

    /// The file's saved view (j.view through cleanScene): its camera and style; false when it has none.
    pub fn apply_view(&mut self, j: &serde_json::Value) -> bool {
        let Some(v) = j.get("view").filter(|v| v.is_object()) else { return false };
        let probe = serde_json::json!({ "name": "v", "cam": v.get("cam").cloned().unwrap_or_default(), "style": v.get("style").cloned().unwrap_or_default() });
        let Some(sc) = cc_m3d::file::clean_scene(&probe) else { return false };
        self.tools.host.cam.set(sc.eye, sc.target, Some(sc.fov), Some(sc.persp));
        self.mode = sc.mode.clone();
        self.edges = sc.edges;
        self.shadows = sc.shadows;
        self.sun = [sc.month, sc.day, sc.hour, sc.lat, sc.lon, sc.tz];
        self.apply_style();
        true
    }

    /// Show the model in `rect`, feed the tools, draw the overlay.
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, t: &Tokens) {
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let (w, h) = self.pinned_size.unwrap_or((f64::from(rect.width()), f64::from(rect.height())));
        self.tools.host.cam.set_size(w, h);
        self.input(ui, &resp, rect);
        self.sync_ctx();
        self.after_pointer();
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
        self.take_tool_message();
        self.dialogs(ui.ctx());
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
        if inside && let Some(p) = pos {
            self.mouse = Some((f64::from(p.x - rect.min.x), f64::from(p.y - rect.min.y)));
        }
        // presses that start in the view
        if inside && let Some(p) = pos {
            if pressed[0]
                && self.tools.name == "select"
                && let Some(id) = self.light_at(f64::from(p.x - rect.min.x), f64::from(p.y - rect.min.y))
            {
                // a click on a lamp's mark picks the lamp before the tools see it
                let _ = self.act(&format!("light-sel:{}", crate::mcact::ns(id)));
            } else if pressed[0] {
                self.place_guard();
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
        self.modifier_keys(mods);
        let events: Vec<egui::Event> = ui.input(|i| i.events.clone());
        for ev in events {
            match ev {
                egui::Event::Key { key, pressed: true, modifiers, .. } => self.key(key, modifiers),
                egui::Event::Key { key: egui::Key::Escape, pressed: false, .. } => {}
                // Ctrl+C / Ctrl+X / Ctrl+V (Ctrl+Shift+V in place) arrive as clipboard events
                egui::Event::Copy => self.run_act("copy"),
                egui::Event::Cut => self.run_act("cut"),
                egui::Event::Paste(_) => self.run_act(if mods.shift { "paste-in-place" } else { "paste" }),
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
        if m.command && key == Key::D {
            self.run_act("duplicate");
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
                } else if self.tools.escape().unwrap_or(false) {
                } else if self.tools.name != "select" {
                    self.set_tool("select");
                } else if !self.tools.host.sel.is_empty() {
                    self.tools.host.sel.clear();
                } else if !self.tools.host.path.is_empty() {
                    self.close_ctx();
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
            Key::ArrowRight | Key::ArrowLeft | Key::ArrowUp | Key::ArrowDown => {
                // a busy tool that locks takes the arrow (red, green, blue); otherwise the arrows orbit (Shift pans)
                let name = match key {
                    Key::ArrowRight => "ArrowRight",
                    Key::ArrowLeft => "ArrowLeft",
                    Key::ArrowUp => "ArrowUp",
                    _ => "ArrowDown",
                };
                if self.lockable() && self.tools.busy() && self.tools.key(name).unwrap_or(false) {
                    self.say_lock();
                    return;
                }
                let s = 18.0;
                let (l, r, u, d) = (key == Key::ArrowLeft, key == Key::ArrowRight, key == Key::ArrowUp, key == Key::ArrowDown);
                let cam = &mut self.tools.host.cam;
                if m.shift {
                    cam.pan(
                        if l {
                            s
                        } else if r {
                            -s
                        } else {
                            0.0
                        },
                        if u {
                            s
                        } else if d {
                            -s
                        } else {
                            0.0
                        },
                    );
                } else {
                    cam.orbit(
                        if l {
                            -s
                        } else if r {
                            s
                        } else {
                            0.0
                        },
                        if u {
                            -s
                        } else if d {
                            s
                        } else {
                            0.0
                        },
                    );
                }
            }
            _ => {}
        }
    }

    /// Whether the current tool locks to an axis (arrows, Shift).
    fn lockable(&mut self) -> bool {
        self.tools.with_tool(|t, _| Ok(cc_m3d_tools::tools::ToolImpl::lockable(t)), false).unwrap_or(false)
    }

    /// sayLock: what the arrow or Shift locked to.
    fn say_lock(&mut self) {
        let m = match &self.tools.lock {
            Some(l) => {
                let w = match l.kind.as_str() {
                    "axis-x" => "red",
                    "axis-y" => "green",
                    "axis-z" => "blue",
                    _ => "edge",
                };
                mt(&format!("app.lock.{w}"), &[])
            }
            None => mt("app.lock.off", &[]),
        };
        self.message = m;
    }

    /// Shift, Ctrl and Alt pressed or let go: the tools' inference lock (Shift) and their modifier toggles.
    fn modifier_keys(&mut self, now: egui::Modifiers) {
        let was = self.mods;
        self.mods = now;
        if now.shift && !was.shift {
            let _ = self.tools.key("Shift");
        } else if !now.shift && was.shift {
            let _ = self.tools.key_up("Shift");
        }
        if now.alt && !was.alt {
            let _ = self.tools.key("Alt");
        }
        if (now.command || now.ctrl) && !(was.command || was.ctrl) {
            let _ = self.tools.key("Control");
        }
    }

    /// The overlay: selection, hover, previews, inference marker and tip.
    fn overlay(&self, ui: &egui::Ui, rect: Rect, t: &Tokens) {
        let painter = ui.painter_at(rect);
        let cam = &self.tools.host.cam;
        let to = |p: P3| -> Option<Pos2> { cam.project(p).map(|q| Pos2::new(rect.min.x + q[0] as f32, rect.min.y + q[1] as f32)) };
        // the lamps' marks (view.drawExtra)
        self.light_marks(&painter, &to, t);
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

    /// cleanScene({name, cam: cam.toJSON(), style: view.style, vis: model.captureVis()}).
    fn scene_here(&self, name: String) -> Option<cc_m3d::model::Scene> {
        let c = &self.tools.host.cam;
        let vis = self.tools.host.model.capture_vis();
        let num = |x: f64| serde_json::Number::from_f64(x).map_or(serde_json::Value::Null, serde_json::Value::Number);
        let p3 = |p: [f64; 3]| serde_json::Value::Array(p.iter().map(|x| num(*x)).collect());
        let j = serde_json::json!({
            "name": name,
            "cam": { "eye": p3(c.eye), "target": p3(c.target), "fov": num(c.fov), "persp": c.persp, "height": num(c.height()) },
            "style": { "mode": self.mode, "edges": self.edges, "shadows": self.shadows,
                "sun": { "month": num(self.sun[0]), "day": num(self.sun[1]), "hour": num(self.sun[2]), "lat": num(self.sun[3]), "lon": num(self.sun[4]), "tz": num(self.sun[5]) } },
            "vis": {
                "insts": vis.insts.iter().map(|p| serde_json::Value::Array(p.iter().map(|x| num(*x)).collect())).collect::<Vec<_>>(),
                "tags": vis.tags,
                "section": vis.section.map_or(serde_json::Value::Null, num),
                "cuts": vis.cuts,
            },
        });
        cc_m3d::file::clean_scene(&j)
    }

    /// modelcraft.js "Add scene": this camera, style and what is hidden, as a new scene.
    pub fn add_scene(&mut self) {
        let name = mt("app.scene.default", &[("n", &(self.tools.host.model.scenes.len() + 1).to_string())]);
        let Some(sc) = self.scene_here(name.clone()) else { return };
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
        self.mode = sc.mode.clone();
        self.edges = sc.edges;
        self.shadows = sc.shadows;
        self.sun = [sc.month, sc.day, sc.hour, sc.lat, sc.lon, sc.tz];
        self.apply_style();
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
        let Some(sc) = self.scene_here(name.clone()) else { return };
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
