//! CadCraft L6: the egui frontend of the Rust test version — the 2D viewer (M3).
//!
//! `UiApp` holds the UI state and the open drawing; `UiApp::run(id, params)` is the single entry for every
//! action (menus, keys, toolbar, the CLI's headless path) — the `UI_COMMANDS` table lists them. Platform work
//! (file pickers, reading files) comes in through [`Services`], filled by the desktop and web hosts.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

pub mod canvas;
pub mod document;
pub mod edit;
pub mod theme;

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use cc_drawing::SpaceKind;
use cc_render::View2D;
use egui::{Color32, RichText};

pub use canvas::install_gpu;
use document::Loaded;
use theme::{Preset, Tokens};

/// A file the host read for us (web: from the browser picker or a drop).
#[derive(Debug, Clone)]
pub struct ImportedFile {
    pub name: String,
    pub bytes: Vec<u8>,
    /// desktop: the folder it came from (linked drawings are looked for there)
    pub dir: Option<String>,
}

/// Linked-drawing lookup: (folder, xref name) → bytes.
pub type ReadXref = Box<dyn FnMut(&str, &str) -> Option<Vec<u8>>>;

/// A desktop picker: None = cancelled.
pub type PickOpen = Box<dyn FnMut() -> Option<Result<ImportedFile, String>>>;

/// Platform services, injected by the host (the UI never touches files or dialogs itself).
#[derive(Default)]
pub struct Services {
    /// desktop: pick a drawing and read it (name, bytes)
    pub pick_open: Option<PickOpen>,
    /// web: start a picker; the file arrives later in `inbox`
    pub open_async: Option<Box<dyn FnMut()>>,
    /// files waiting to be opened (async pickers, dropped files)
    pub inbox: Option<Arc<Mutex<Vec<ImportedFile>>>>,
    /// desktop: "<xref>.dwg" in a folder (folder, xref name) → bytes
    pub read_xref: Option<ReadXref>,
    /// web: read a dropped file asynchronously into `inbox`
    #[cfg(target_arch = "wasm32")]
    pub read_dropped: Option<Box<dyn FnMut(Arc<dyn egui::DroppedFile + Send + Sync>)>>,
}

/// The commands of the viewer (id, label, shortcut, params doc).
pub const UI_COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("file.open", "Open…", "Ctrl+O", "{} → opens the platform picker"),
    ("file.sample", "Open the sample", "", "{} → the built-in sample drawing"),
    ("view.fit", "Zoom to the drawing", "Home", "{} → fits the active space"),
    ("view.zoom", "Zoom", "", "{factor: number > 0} → zooms about the canvas centre"),
    ("space.select", "Show a space", "", "{index: integer} → 0 = model, then sheets in tab order"),
    ("layer.toggle", "Show or hide a layer", "", "{name: string}"),
    ("layer.all", "Show every layer", "", "{}"),
    ("theme.toggle", "Dark / light", "", "{}"),
    ("file.new", "New drawing", "Ctrl+N", "{} → an empty editable drawing with the command line"),
    ("cmd.line", "Type at the command line", "", "{name: string} → as if typed and Enter pressed (drawing open for editing)"),
    ("edit.undo", "Undo", "Ctrl+Z", "{}"),
    ("edit.redo", "Redo", "Ctrl+Y", "{}"),
    ("edit.osnap", "Object snap on / off", "F3", "{}"),
    ("edit.ortho", "Ortho on / off", "F8", "{}"),
];

/// The built-in sample (CadCraft's own public fixture, base-plan.dwg).
pub static SAMPLE_DWG: &[u8] = include_bytes!("../../../assets/samples/kitchen-equipment.dwg");
/// The drawing the sample links (xref "base-plan").
pub static SAMPLE_XREF: &[u8] = include_bytes!("../../../assets/samples/base-plan.dwg");

pub struct UiApp {
    pub services: Services,
    pub doc: Option<Loaded>,
    pub space: usize,
    /// one view per space (physical pixels)
    pub views: Vec<Option<View2D>>,
    pub hidden: HashSet<String>,
    pub preset: Preset,
    pub tokens: Tokens,
    theme_dirty: bool,
    pub message: String,
    pub error: Option<String>,
    pub cursor: Option<(f64, f64)>,
    pub frame_ms: f64,
    pub last_stats: canvas::FrameStats,
    canvas_px: (f64, f64),
    pub show_layers: bool,
    pending_fit: bool,
    fonts_done: bool,
    /// frame benchmark: remaining frames and the measured intervals (ms)
    bench: Option<(u32, Vec<f64>)>,
    pub bench_result: Option<String>,
    /// the drawing open for editing (M4): its display model is rebuilt into `doc` after every change
    pub editor: Option<edit::Editor>,
    /// views before each ZOOM (ZOOM Previous)
    prev_views: Vec<View2D>,
    /// primary-button drag start (world) while drafting: a selection window
    drag_from: Option<(f64, f64)>,
}

impl Default for UiApp {
    fn default() -> Self {
        UiApp {
            services: Services::default(),
            doc: None,
            space: 0,
            views: Vec::new(),
            hidden: HashSet::new(),
            preset: Preset::Dark,
            tokens: Tokens::preset(Preset::Dark),
            theme_dirty: true,
            message: String::new(),
            error: None,
            cursor: None,
            frame_ms: 0.0,
            last_stats: canvas::FrameStats::default(),
            canvas_px: (1.0, 1.0),
            show_layers: true,
            pending_fit: false,
            fonts_done: false,
            bench: None,
            bench_result: None,
            editor: None,
            prev_views: Vec::new(),
            drag_from: None,
        }
    }
}

impl UiApp {
    pub fn new(services: Services) -> Self {
        UiApp { services, ..Default::default() }
    }

    /// Open bytes as a drawing (any failure becomes a message, never a crash).
    pub fn open_bytes(&mut self, name: &str, bytes: &[u8]) {
        self.open_file(&ImportedFile { name: name.to_string(), bytes: bytes.to_vec(), dir: None });
    }

    /// Open a file; linked drawings are looked for in its folder (desktop) through `Services::read_xref`.
    pub fn open_file(&mut self, f: &ImportedFile) {
        let dir = f.dir.clone();
        let reader = &mut self.services.read_xref;
        let mut resolve = |x: &str| -> Option<Vec<u8>> {
            match (dir.as_deref(), reader.as_mut()) {
                (Some(d), Some(r)) => r(d, x),
                _ => None,
            }
        };
        let res = document::open_with(&f.name, &f.bytes, &mut resolve);
        self.loaded(res);
    }

    fn loaded(&mut self, res: Result<Loaded, String>) {
        match res {
            Ok(l) => {
                self.editor = None;
                self.message =
                    format!("{} · read {:.0} ms · compile {:.0} ms · GPU data {:.0} ms", l.name, l.read_ms, l.compile_ms, l.geometry_ms);
                self.views = vec![None; l.drawing.spaces.len()];
                self.space = 0;
                self.hidden = l.drawing.layers.iter().filter(|x| !x.on).map(|x| x.name.clone()).collect();
                self.error = None;
                self.doc = Some(l);
                self.pending_fit = true;
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Start the frame benchmark: `frames` frames of continuous zooming; the result goes to `bench_result`
    /// (and the message line): best / median / p95 frame interval in ms.
    pub fn start_bench(&mut self, frames: u32) {
        self.bench = Some((frames.clamp(10, 10_000), Vec::new()));
    }

    fn bench_step(&mut self, ctx: &egui::Context) {
        let Some((left, times)) = self.bench.as_mut() else { return };
        let dt = f64::from(ctx.input(|i| i.unstable_dt)) * 1000.0;
        if !times.is_empty() || *left > 0 {
            times.push(dt);
        }
        let zoom = if *left % 40 < 20 { 1.03 } else { 1.0 / 1.03 };
        *left = left.saturating_sub(1);
        let done = *left == 0;
        let (w, h) = self.canvas_px;
        if let Some(Some(v)) = self.views.get_mut(self.space) {
            v.zoom_at(w / 2.0, h / 2.0, zoom);
        }
        if done {
            let mut t = times.split_off(times.len().min(2)); // the first frames include start-up
            t.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let at = |q: f64| t.get(((t.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(f64::NAN);
            let r = format!("frame bench: {} frames · best {:.2} ms · median {:.2} ms · p95 {:.2} ms", t.len(), at(0.0), at(0.5), at(0.95));
            self.message = r.clone();
            self.bench_result = Some(r);
            self.bench = None;
        } else {
            ctx.request_repaint();
        }
    }

    /// The single entry for every action.
    pub fn run(&mut self, id: &str, params: &Params) -> Result<(), String> {
        match id {
            "file.open" => {
                if let Some(p) = self.services.pick_open.as_mut() {
                    match p() {
                        Some(Ok(f)) => self.open_file(&f),
                        Some(Err(e)) => self.error = Some(e),
                        None => {}
                    }
                } else if let Some(p) = self.services.open_async.as_mut() {
                    p();
                } else {
                    return Err("file.open: no file picker on this platform".into());
                }
            }
            "file.sample" => {
                let res = document::open_with("kitchen-equipment.dwg", SAMPLE_DWG, &mut |x| {
                    (x.eq_ignore_ascii_case("base-plan")).then(|| SAMPLE_XREF.to_vec())
                });
                self.loaded(res);
            }
            "view.fit" => self.pending_fit = true,
            "file.new" => {
                self.editor = Some(edit::Editor::new(cc_cad::Doc::new(), "Drawing1"));
                self.doc = None;
                self.views = vec![None];
                self.space = 0;
                self.hidden.clear();
                self.error = None;
                self.message = "New drawing: type a command (LINE, CIRCLE, …) and press Enter".into();
                self.sync_editor();
            }
            "cmd.line" => {
                let text = params.name.clone().ok_or("cmd.line: name (the text) is required")?;
                let ed = self.editor.as_mut().ok_or("cmd.line: no drawing is open for editing (file.new)")?;
                ed.type_line(&text);
                self.sync_editor();
            }
            "edit.undo" | "edit.redo" => {
                let ed = self.editor.as_mut().ok_or(format!("{id}: no drawing is open for editing"))?;
                if id == "edit.undo" {
                    ed.undo();
                } else {
                    ed.redo();
                }
                self.sync_editor();
            }
            "edit.osnap" | "edit.ortho" => {
                let ed = self.editor.as_mut().ok_or(format!("{id}: no drawing is open for editing"))?;
                let on = if id == "edit.osnap" {
                    ed.osnap = !ed.osnap;
                    ed.osnap
                } else {
                    ed.ortho = !ed.ortho;
                    ed.ortho
                };
                self.message = format!("{} {}", if id == "edit.osnap" { "Object snap" } else { "Ortho" }, if on { "on" } else { "off" });
            }
            "view.zoom" => {
                let f = params.factor.filter(|f| f.is_finite() && *f > 0.0).ok_or("view.zoom: factor must be a number > 0")?;
                let (w, h) = self.canvas_px;
                if let Some(Some(v)) = self.views.get_mut(self.space) {
                    v.zoom_at(w / 2.0, h / 2.0, f);
                }
            }
            "space.select" => {
                let i = params.index.ok_or("space.select: index is required")?;
                let n = self.doc.as_ref().map(|d| d.drawing.spaces.len()).unwrap_or(0);
                if i >= n {
                    return Err(format!("space.select: index {i} is out of range (0..{n})"));
                }
                self.space = i;
            }
            "layer.toggle" => {
                let n = params.name.clone().ok_or("layer.toggle: name is required")?;
                if !self.hidden.remove(&n) {
                    self.hidden.insert(n);
                }
            }
            "layer.all" => self.hidden.clear(),
            "theme.toggle" => {
                self.preset = if self.preset == Preset::Dark { Preset::Light } else { Preset::Dark };
                self.tokens = Tokens::preset(self.preset);
                self.theme_dirty = true;
            }
            other => return Err(format!("unknown command {other}")),
        }
        Ok(())
    }

    /// After any input to the editor: rebuild the display model if the drawing changed, apply ZOOM requests.
    pub fn sync_editor(&mut self) {
        let Some(ed) = self.editor.as_mut() else { return };
        if ed.take_changed() || self.doc.is_none() {
            let drawing = cc_cad_geom::display::doc_drawing(&ed.eng.doc());
            let n = drawing.spaces.len();
            self.doc = Some(document::from_drawing(&ed.name, drawing));
            self.views.resize(n, None);
            if self.space >= n {
                self.space = 0;
            }
        }
        let zooms = ed.take_zoom();
        let (w, h) = self.canvas_px;
        for z in zooms {
            let Some(Some(v)) = self.views.get_mut(self.space) else { continue };
            match z {
                edit::Zoom::Extents => {
                    self.prev_views.push(*v);
                    self.pending_fit = true;
                }
                edit::Zoom::Window(a, b, exact) => {
                    self.prev_views.push(*v);
                    v.width = w;
                    v.height = h;
                    v.fit(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y), if exact { 0.0 } else { 0.03 });
                }
                edit::Zoom::By(f) => {
                    self.prev_views.push(*v);
                    v.zoom_at(w / 2.0, h / 2.0, f);
                }
                edit::Zoom::Previous => {
                    if let Some(p) = self.prev_views.pop() {
                        *v = p;
                    }
                }
            }
        }
        if self.prev_views.len() > 50 {
            self.prev_views.remove(0);
        }
    }

    /// Headless: the pointer over a world point (rubber band, snap marker), as the canvas would report it.
    pub fn hover_world(&mut self, x: f64, y: f64) {
        let pxu = self.views.get(self.space).copied().flatten().map_or(1.0, |v| v.zoom);
        if let Some(ed) = self.editor.as_mut() {
            ed.hover(cc_geom::P2::new(x, y), pxu);
        }
    }
    /// Headless: a click on a world point.
    pub fn click_world(&mut self, x: f64, y: f64) {
        let pxu = self.views.get(self.space).copied().flatten().map_or(1.0, |v| v.zoom);
        if let Some(ed) = self.editor.as_mut() {
            ed.click(cc_geom::P2::new(x, y), pxu);
        }
        self.sync_editor();
    }

    fn run_ui(&mut self, id: &str, params: &Params) {
        if let Err(e) = self.run(id, params) {
            self.error = Some(e);
        }
    }

    /// Every frame: drain the inbox, then draw.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let t0 = web_time::Instant::now();
        let ctx = ui.ctx().clone();
        if !self.fonts_done {
            theme::install_fonts(&ctx);
            self.fonts_done = true;
        }
        if self.theme_dirty {
            self.tokens.apply(&ctx, self.preset == Preset::Dark);
            self.theme_dirty = false;
        }
        let incoming: Vec<ImportedFile> = match &self.services.inbox {
            Some(inbox) => std::mem::take(&mut *inbox.lock().unwrap_or_else(PoisonError::into_inner)),
            None => Vec::new(),
        };
        for f in incoming {
            self.open_file(&f);
        }
        // dropped files (native: read here; web: the host reads them asynchronously into the inbox)
        let dropped: Vec<std::sync::Arc<dyn egui::DroppedFile + Send + Sync>> = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let name = f.path().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let dir = f.path().parent().map(|p| p.to_string_lossy().into_owned());
                match f.bytes() {
                    Ok(b) => self.open_file(&ImportedFile { name, bytes: b, dir }),
                    Err(e) => self.error = Some(format!("{name}: {e}")),
                }
            }
            #[cfg(target_arch = "wasm32")]
            if let Some(read) = self.services.read_dropped.as_mut() {
                read(f);
            }
        }
        // keys
        let (open_key, fit_key) = ctx.input(|i| (i.modifiers.command && i.key_pressed(egui::Key::O), i.key_pressed(egui::Key::Home)));
        if open_key {
            self.run_ui("file.open", &Params::default());
        }
        if fit_key {
            self.run_ui("view.fit", &Params::default());
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::N)) {
            self.run_ui("file.new", &Params::default());
        }
        self.app_bar(ui);
        self.status_bar(ui);
        if self.editor.is_some() {
            self.command_line(ui);
        }
        if self.doc.is_some() && self.show_layers {
            self.layers_panel(ui);
        }
        self.tabs(ui);
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));
        if self.bench.is_some() && self.doc.is_some() {
            self.bench_step(&ctx);
        }
        self.frame_ms = t0.elapsed().as_secs_f64() * 1000.0;
    }

    fn app_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        egui::Panel::top("app_bar")
            .exact_size(34.0)
            .frame(egui::Frame::new().fill(t.app_bar).inner_margin(egui::Margin::symmetric(8, 4)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("CadCraft").strong().color(t.text_strong));
                    ui.label(RichText::new("Rust test version").small().color(t.text_dim));
                    ui.separator();
                    if ui.button("Open…").on_hover_text("Open a DWG or DXF (Ctrl+O)").clicked() {
                        self.run_ui("file.open", &Params::default());
                    }
                    if ui.button("Sample").on_hover_text("Open the built-in sample drawing").clicked() {
                        self.run_ui("file.sample", &Params::default());
                    }
                    if ui.button("New").on_hover_text("A new drawing to draft in (Ctrl+N)").clicked() {
                        self.run_ui("file.new", &Params::default());
                    }
                    if ui.add_enabled(self.doc.is_some(), egui::Button::new("Fit")).on_hover_text("Zoom to the drawing (Home)").clicked() {
                        self.run_ui("view.fit", &Params::default());
                    }
                    if ui.add_enabled(self.doc.is_some(), egui::Button::new("Layers")).clicked() {
                        self.show_layers = !self.show_layers;
                    }
                    let label = if self.preset == Preset::Dark { "Light" } else { "Dark" };
                    if ui.button(label).on_hover_text("Switch the theme").clicked() {
                        self.run_ui("theme.toggle", &Params::default());
                    }
                    if let Some(d) = &self.doc {
                        ui.separator();
                        ui.label(RichText::new(&d.name).color(t.text));
                        let rel = if d.drawing.release.is_empty() {
                            d.drawing.version.clone()
                        } else {
                            format!("AutoCAD {} format", d.drawing.release)
                        };
                        ui.label(RichText::new(rel).small().color(t.text_dim));
                    }
                });
            });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        egui::Panel::bottom("status")
            .exact_size(24.0)
            .frame(egui::Frame::new().fill(t.app_bar).inner_margin(egui::Margin::symmetric(8, 3)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    if let Some(e) = &self.error {
                        ui.label(RichText::new(e).color(Color32::from_rgb(0xe0, 0x6c, 0x5c)));
                    } else if let Some((x, y)) = self.cursor {
                        ui.label(RichText::new(format!("{x:.4}, {y:.4}")).monospace().color(t.text));
                    } else {
                        ui.label(RichText::new("Drop a DWG or DXF here, or Open…").color(t.text_dim));
                    }
                    ui.separator();
                    ui.label(RichText::new(&self.message).small().color(t.text_dim));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("UI {:.1} ms · {} texts", self.frame_ms, self.last_stats.texts))
                                .small()
                                .color(t.text_dim),
                        );
                    });
                });
            });
    }

    fn layers_panel(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let Some(doc) = &self.doc else { return };
        let layers: Vec<(String, Color32)> = doc
            .drawing
            .layers
            .iter()
            .map(|l| {
                let c = match l.color {
                    cc_drawing::Color::Ink => t.ink,
                    cc_drawing::Color::Rgb(n) => Color32::from_rgb(((n >> 16) & 0xFF) as u8, ((n >> 8) & 0xFF) as u8, (n & 0xFF) as u8),
                };
                (l.name.clone(), c)
            })
            .collect();
        let mut toggle: Option<String> = None;
        let mut all = false;
        egui::Panel::left("layers").default_size(220.0).resizable(true).frame(egui::Frame::new().fill(t.panel).inner_margin(6)).show(
            ui,
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Layers ({})", layers.len())).strong().color(t.text_strong));
                    if ui.small_button("Show all").clicked() {
                        all = true;
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for (name, c) in &layers {
                        let mut on = !self.hidden.contains(name);
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                            ui.painter().rect_filled(r, 1.0, *c);
                            if ui.checkbox(&mut on, RichText::new(name).color(t.text)).changed() {
                                toggle = Some(name.clone());
                            }
                        });
                    }
                });
            },
        );
        if all {
            self.run_ui("layer.all", &Params::default());
        }
        if let Some(n) = toggle {
            self.run_ui("layer.toggle", &Params { name: Some(n), ..Default::default() });
        }
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let Some(doc) = &self.doc else { return };
        let names: Vec<(String, bool)> = doc.drawing.spaces.iter().map(|s| (s.name.clone(), s.kind == SpaceKind::Paper)).collect();
        if names.len() <= 1 {
            return;
        }
        let mut pick = None;
        egui::Panel::bottom("tabs")
            .exact_size(26.0)
            .frame(egui::Frame::new().fill(t.panel_darker).inner_margin(egui::Margin::symmetric(6, 3)))
            .show(ui, |ui| {
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (i, (n, _)) in names.iter().enumerate() {
                            if ui.selectable_label(i == self.space, n).clicked() {
                                pick = Some(i);
                            }
                        }
                    });
                });
            });
        if let Some(i) = pick {
            self.run_ui("space.select", &Params { index: Some(i), ..Default::default() });
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let (w, h) = (f64::from(rect.width()) * ppp, f64::from(rect.height()) * ppp);
        self.canvas_px = (w, h);
        let Some(doc) = &self.doc else {
            let t = self.tokens;
            ui.painter().rect_filled(rect, 0.0, t.canvas_bg);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Open a DWG or DXF, drop one here, or open the sample.",
                egui::FontId::proportional(15.0),
                t.text_dim,
            );
            return;
        };
        let Some(space) = doc.drawing.spaces.get(self.space) else { return };
        let fit_rect = space.paper.unwrap_or(space.bounds);
        let need_fit = self.pending_fit || self.views.get(self.space).is_none_or(Option::is_none);
        let mut view = self.views.get(self.space).copied().flatten().unwrap_or_default();
        view.width = w;
        view.height = h;
        if need_fit {
            if fit_rect.is_empty() && self.editor.is_some() {
                // an empty drawing: 100 × 60 units from the origin
                view.fit(0.0, 0.0, 100.0, 60.0, 0.03);
            } else if fit_rect.is_empty() {
                view.cx = 0.0;
                view.cy = 0.0;
                view.zoom = 1.0;
            } else {
                view.fit(fit_rect.min_x, fit_rect.min_y, fit_rect.max_x, fit_rect.max_y, 0.03);
            }
            self.pending_fit = false;
        }
        let editing = self.editor.is_some();
        // input: drag pans (any button; drafting: middle / right), wheel zooms about the pointer, double click fits
        if resp.dragged() && !(editing && resp.dragged_by(egui::PointerButton::Primary)) {
            let d = resp.drag_delta();
            view.pan(f64::from(d.x) * ppp, f64::from(d.y) * ppp);
        }
        if let Some(p) = resp.hover_pos() {
            let sx = f64::from(p.x - rect.min.x) * ppp;
            let sy = f64::from(p.y - rect.min.y) * ppp;
            let (scroll, zoom) = ui.ctx().input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            if scroll != 0.0 {
                view.zoom_at(sx, sy, (f64::from(scroll) / 200.0).exp());
            }
            if zoom != 1.0 {
                view.zoom_at(sx, sy, f64::from(zoom));
            }
            self.cursor = Some(view.to_world(sx, sy));
        } else {
            self.cursor = None;
        }
        if !editing && resp.double_clicked() && !fit_rect.is_empty() {
            view.fit(fit_rect.min_x, fit_rect.min_y, fit_rect.max_x, fit_rect.max_y, 0.03);
        }
        if let Some(slot) = self.views.get_mut(self.space) {
            *slot = Some(view);
        }
        let frame = canvas::Frame {
            drawing: &doc.drawing,
            geometry: &doc.geometry,
            id: doc.id,
            space: self.space,
            view,
            hidden: &self.hidden,
            tokens: &self.tokens,
            line_px: ppp as f32,
        };
        self.last_stats = canvas::paint(ui, rect, &frame);
        if editing {
            self.edit_input(ui, &resp, rect, &view, ppp);
            self.edit_overlays(ui, rect, &view, ppp);
        }
    }

    /// Drafting input on the canvas (cadcraft.js rules, see [`edit::Editor`]).
    fn edit_input(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: egui::Rect, view: &View2D, ppp: f64) {
        let Some(ed) = self.editor.as_mut() else { return };
        let world = |p: egui::Pos2| {
            let (x, y) = view.to_world(f64::from(p.x - rect.min.x) * ppp, f64::from(p.y - rect.min.y) * ppp);
            cc_geom::P2::new(x, y)
        };
        let pxu = view.zoom / ppp; // logical px per drawing unit
        let shift = ui.ctx().input(|i| i.modifiers.shift);
        {
            let mut h = ed.hs.borrow_mut();
            h.shift = shift;
            h.view_wh = Some((view.width / view.zoom, view.height / view.zoom));
        }
        if let Some(p) = resp.hover_pos() {
            ed.hover(world(p), pxu);
        }
        if resp.drag_started_by(egui::PointerButton::Primary)
            && let Some(p) = resp.interact_pointer_pos()
        {
            let w = world(p - resp.drag_delta());
            self.drag_from = Some((w.x, w.y));
        }
        if resp.drag_stopped_by(egui::PointerButton::Primary)
            && let (Some((x, y)), Some(p)) = (self.drag_from.take(), resp.interact_pointer_pos())
        {
            ed.finish_box(cc_geom::P2::new(x, y), world(p), pxu);
        }
        if resp.clicked_by(egui::PointerButton::Primary)
            && let Some(p) = resp.interact_pointer_pos()
        {
            ed.click(world(p), pxu);
        }
        if resp.clicked_by(egui::PointerButton::Secondary) {
            // right click = Enter, as in AutoCAD
            ed.submit();
        }
        self.sync_editor();
    }

    /// The rubber band, the selection, the snap marker, the selection window and the crosshair.
    fn edit_overlays(&self, ui: &egui::Ui, rect: egui::Rect, view: &View2D, ppp: f64) {
        let Some(ed) = self.editor.as_ref() else { return };
        let t = self.tokens;
        let painter = ui.painter_at(rect);
        let to_pos = |x: f64, y: f64| {
            let (sx, sy) = view.to_screen(x, y);
            egui::pos2(rect.min.x + (sx / ppp) as f32, rect.min.y + (sy / ppp) as f32)
        };
        let doc = ed.eng.doc();
        let draw = |ents: &[cc_cad::Entity], stroke: egui::Stroke| {
            for b in cc_cad_geom::display::walk_batches(&doc, ents) {
                for l in &b.lines {
                    let pts: Vec<egui::Pos2> = l.as_chunks::<2>().0.iter().map(|&[x, y]| to_pos(x, y)).collect();
                    if pts.len() >= 2 {
                        painter.add(egui::Shape::line(pts, stroke));
                    }
                }
                for tx in &b.texts {
                    let (sx, sy) = view.to_screen(tx.x, tx.y);
                    let px = (tx.h * view.zoom / ppp) as f32;
                    if px > 2.0 {
                        let pos = egui::pos2(rect.min.x + (sx / ppp) as f32, rect.min.y + (sy / ppp) as f32 - px);
                        painter.text(pos, egui::Align2::LEFT_TOP, &tx.s, egui::FontId::proportional(px * 1.3), stroke.color);
                    }
                }
            }
        };
        let sel = ed.selected();
        if !sel.is_empty() {
            draw(&sel, egui::Stroke::new(3.0, t.accent.gamma_multiply(0.55)));
            // grips
            for e in &sel {
                for g in cc_cad_geom::tx::grips(e) {
                    let c = to_pos(g.x, g.y);
                    painter.rect_filled(egui::Rect::from_center_size(c, egui::vec2(7.0, 7.0)), 0.0, t.accent);
                }
            }
        }
        let preview = ed.hs.borrow().preview.clone();
        if !preview.is_empty() {
            draw(&preview, egui::Stroke::new(1.0, t.ink));
        }
        drop(doc);
        if let Some(s) = &ed.snap {
            let c = to_pos(s.x, s.y);
            let st = egui::Stroke::new(1.5, egui::Color32::from_rgb(0x3c, 0xd0, 0x70));
            match s.kind {
                "end" => {
                    painter.rect_stroke(egui::Rect::from_center_size(c, egui::vec2(11.0, 11.0)), 0.0, st, egui::StrokeKind::Middle);
                }
                "mid" => {
                    let pts = vec![c + egui::vec2(0.0, -7.0), c + egui::vec2(6.5, 5.0), c + egui::vec2(-6.5, 5.0)];
                    painter.add(egui::Shape::closed_line(pts, st));
                }
                "cen" => {
                    painter.circle_stroke(c, 6.0, st);
                }
                _ => {
                    painter.line_segment([c + egui::vec2(-6.0, -6.0), c + egui::vec2(6.0, 6.0)], st);
                    painter.line_segment([c + egui::vec2(-6.0, 6.0), c + egui::vec2(6.0, -6.0)], st);
                }
            }
            painter.text(c + egui::vec2(10.0, 10.0), egui::Align2::LEFT_TOP, s.kind, egui::FontId::proportional(11.0), st.color);
        }
        // the selection window: blue solid (window) / green dashed-looking (crossing), as CadCraft draws it
        let hover = ui.ctx().pointer_hover_pos();
        let from = ed.pending_box.map(|p| (p.x, p.y)).or(self.drag_from);
        if let (Some((ax, ay)), Some(hp)) = (from, hover) {
            let a = to_pos(ax, ay);
            let r = egui::Rect::from_two_pos(a, hp);
            let crossing = hp.x < a.x;
            let c = if crossing { egui::Color32::from_rgb(0x3c, 0xb0, 0x60) } else { egui::Color32::from_rgb(0x3c, 0x80, 0xe0) };
            painter.rect_filled(r, 0.0, c.gamma_multiply(0.12));
            painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, c), egui::StrokeKind::Middle);
        }
        // crosshair
        if let Some(hp) = hover.filter(|p| rect.contains(*p)) {
            let st = egui::Stroke::new(1.0, t.ink.gamma_multiply(0.6));
            painter.line_segment([egui::pos2(hp.x - 40.0, hp.y), egui::pos2(hp.x + 40.0, hp.y)], st);
            painter.line_segment([egui::pos2(hp.x, hp.y - 40.0), egui::pos2(hp.x, hp.y + 40.0)], st);
            painter.rect_stroke(egui::Rect::from_center_size(hp, egui::vec2(9.0, 9.0)), 0.0, st, egui::StrokeKind::Middle);
        }
    }

    /// The command line (history above, prompt + input), CadCraft 0.5's command line at the bottom.
    fn command_line(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let ctx = ui.ctx().clone();
        let Some(ed) = self.editor.as_mut() else { return };
        // read Enter before the text field consumes it
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let mut submit = false;
        egui::Panel::bottom("cmdline")
            .exact_size(96.0)
            .frame(egui::Frame::new().fill(t.panel).inner_margin(egui::Margin::symmetric(8, 4)))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let log: Vec<String> = ed.hs.borrow().log.iter().rev().take(3).rev().cloned().collect();
                for _ in log.len()..3 {
                    ui.label(RichText::new(" ").small());
                }
                for l in &log {
                    ui.label(RichText::new(l).small().monospace().color(t.text_dim));
                }
                ui.horizontal(|ui| {
                    let lab = ui.label(RichText::new(ed.prompt()).monospace().color(t.text_strong));
                    let te = egui::TextEdit::singleline(&mut ed.line)
                        .id_salt("cmdline.input")
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .frame(egui::Frame::NONE);
                    let r = ui.add(te).labelled_by(lab.id);
                    let other_focus = ctx.memory(|m| m.focused()).is_some_and(|f| f != r.id);
                    if !other_focus {
                        r.request_focus();
                    }
                    if enter && (r.has_focus() || r.lost_focus()) {
                        submit = true;
                    }
                });
            });
        // Space sends the line too, except while a command asks for text
        if ed.line.ends_with(' ') && ed.eng.expects() != Some(cc_engine::prompt::Kind::Text) {
            ed.line.pop();
            submit = true;
        }
        let (esc, del, undo, redo, f3, f8) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::Delete),
                i.modifiers.command && i.key_pressed(egui::Key::Z),
                i.modifiers.command && i.key_pressed(egui::Key::Y),
                i.key_pressed(egui::Key::F3),
                i.key_pressed(egui::Key::F8),
            )
        });
        if submit {
            ed.submit();
        }
        if esc {
            ed.escape();
            ed.line.clear();
        }
        if del && ed.line.is_empty() {
            ed.delete();
        }
        if undo {
            ed.undo();
        }
        if redo {
            ed.redo();
        }
        if f3 {
            self.run_ui("edit.osnap", &Params::default());
        }
        if f8 {
            self.run_ui("edit.ortho", &Params::default());
        }
        self.sync_editor();
    }
}

/// Command parameters (validated by each command).
#[derive(Clone, Debug, Default)]
pub struct Params {
    pub factor: Option<f64>,
    pub index: Option<usize>,
    pub name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commands_are_unique_and_validated() {
        let mut ids: Vec<&str> = UI_COMMANDS.iter().map(|c| c.0).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), UI_COMMANDS.len());
        let mut app = UiApp::default();
        assert!(app.run("view.zoom", &Params { factor: Some(-1.0), ..Default::default() }).is_err());
        assert!(app.run("space.select", &Params { index: Some(3), ..Default::default() }).is_err());
        assert!(app.run("nope", &Params::default()).is_err());
        app.run("layer.toggle", &Params { name: Some("A".into()), ..Default::default() }).unwrap();
        assert!(app.hidden.contains("A"));
    }
}
