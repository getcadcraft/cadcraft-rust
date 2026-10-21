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

pub mod autocount;
pub mod bimedit;
pub mod bimgrips;
pub mod bimplace;
pub mod bimplan;
pub mod bimtools;
pub mod cadprops;
pub mod canvas;
pub mod civil3d;
pub mod civildialogs;
pub mod civilexport;
pub mod civillasso;
pub mod civilpanel;
pub mod civilplan;
pub mod civilpoints;
pub mod civilprofile;
pub mod civilpropedit;
pub mod civilprops;
pub mod civilsections;
pub mod civilsheets;
pub mod civiltools;
pub mod civilview;
pub mod detail3d;
pub mod detailexport;
pub mod detailpanel;
pub mod detailprops;
pub mod detailtools;
pub mod detailview;
pub mod document;
pub mod edit;
pub mod hub;
pub mod ifc3d;
pub mod markupbcf;
pub mod markupchest;
pub mod markupcompare;
pub mod markupedit;
pub mod markupplus;
pub mod markupprops;
pub mod markuptools;
pub mod mcact;
pub mod mcedit;
pub mod mcmats;
pub mod mcpanels;
pub mod mcprint;
pub mod mcsolids;
pub mod model3d;
pub mod modelcraft;
pub mod partfeat;
pub mod partpanel;
pub mod partsketch;
pub mod partsketchui;
pub mod partview;
pub mod pdfcmd;
pub mod pdfview;
pub mod picmesh;
pub mod plotdlg;
pub mod qsdlg;
pub mod takeoff3d;
pub mod takeoffedit;
pub mod takeoffnotes;
pub mod takeoffprops;
pub mod takeoffrev;
pub mod takeofftools;
pub mod theme;
pub mod tkbench;
pub mod view3d;

use std::collections::{HashMap, HashSet};
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

/// web: read a dropped file asynchronously.
#[cfg(target_arch = "wasm32")]
pub type ReadDropped = Box<dyn FnMut(Arc<dyn egui::DroppedFile + Send + Sync>)>;

/// Save a file: (suggested name, bytes) → Ok(Some(where it went)), Ok(None) = cancelled (desktop: a save dialog and
/// a write; web: a download).
pub type SaveFile = Box<dyn FnMut(&str, Vec<u8>) -> Result<Option<String>, String>>;

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
    /// Save / Save as (desktop: dialog + write; web: download)
    pub save: Option<SaveFile>,
    /// web: read a dropped file asynchronously into `inbox`
    #[cfg(target_arch = "wasm32")]
    pub read_dropped: Option<ReadDropped>,
    /// desktop: read a file by its path (the start screen's recent files)
    pub read_path: Option<ReadPath>,
    /// pick files for a command: ("drawing" | "pictures", several?) → desktop: Some(files) at once (empty = cancelled);
    /// web: None, the picker is open and the files arrive in `picked`
    pub pick_files: Option<PickFiles>,
    /// web: what the last `pick_files` picker read (an empty list = cancelled)
    pub picked: Option<Arc<Mutex<Option<Vec<ImportedFile>>>>>,
}

/// Pick files for a command (see `Services::pick_files`).
pub type PickFiles = Box<dyn FnMut(&str, bool) -> Option<Vec<ImportedFile>>>;

/// Read a file by its path → (name, bytes, folder).
pub type ReadPath = Box<dyn FnMut(&str) -> Result<ImportedFile, String>>;

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
    (
        "view.demo3d",
        "3D test scene",
        "",
        "{factor?: number} → shows the 3D viewport on a test scene of N×N boxes (factor = N, default 12) at survey coordinates",
    ),
    ("cmd.line", "Type at the command line", "", "{name: string} → as if typed and Enter pressed (drawing open for editing)"),
    ("edit.undo", "Undo", "Ctrl+Z", "{}"),
    ("edit.redo", "Redo", "Ctrl+Y", "{}"),
    ("edit.osnap", "Object snap on / off", "F3", "{}"),
    ("edit.ortho", "Ortho on / off", "F8", "{}"),
    ("edit.polar", "Polar tracking on / off", "F10", "{} → points line up on multiples of the POLARANG step"),
    ("file.edit", "Edit this drawing", "", "{} → imports the open DWG / DXF into an editable drawing (CadCraft's importer)"),
    (
        "file.save",
        "Save as",
        "Ctrl+S",
        "{name?: \"2018\"|\"2013\"|\"2010\"|\"2007\"|\"2004\"|\"2000\"|\"dxf2000\"|\"dxf12\"} → saves the drawing open for editing (default: the last format, else 2018)",
    ),
];

/// The built-in sample (CadCraft's own public fixture, base-plan.dwg).
pub static SAMPLE_DWG: &[u8] = include_bytes!("../../../assets/samples/kitchen-equipment.dwg");
/// The drawing the sample links (xref "base-plan").
pub static SAMPLE_XREF: &[u8] = include_bytes!("../../../assets/samples/base-plan.dwg");
/// The IFC sample (CadCraft's public showcase house).
pub static SAMPLE_IFC: &[u8] = include_bytes!("../../../assets/samples/bimcraft-house.ifc");

pub struct UiApp {
    pub services: Services,
    /// a 3D scene shown instead of the drawing (the 3D viewport, M6)
    pub scene3: Option<(view3d::Scene3D, view3d::View3)>,
    /// the ModelCraft model open (shown through `scene3`)
    pub model3: Option<cc_m3d::model::Model>,
    /// the ModelCraft model being edited with its tools (replaces `scene3` while open)
    pub mc: Option<modelcraft::ModelView>,
    /// how the last model open split its time (benchmarks)
    pub open_split: String,
    /// scene ids: a new id = new GPU meshes
    scene_seq: u64,
    /// the IFC file shown (for properties) and each scene instance's product line
    ifc3: Option<(std::sync::Arc<Vec<u8>>, Vec<f64>)>,
    /// each scene instance's (storey, class) for the Model panel, and what is hidden there ("s:<storey>", "c:<class>")
    ifc_groups: Vec<(String, String)>,
    pub hidden3: HashSet<String>,
    /// Find's result on the IFC view: (show only these, product expressIDs as bits); None = no filter
    iso3: Option<(bool, HashSet<u64>)>,
    /// BimCraft's Find and Quantity totals on the open IFC model
    pub bim: bimtools::BimTools,
    /// the open IFC file's name
    ifc_name: String,
    /// an open PDF plan set
    pub pdf: Option<pdfview::PdfView>,
    /// what MarkupCraft keeps between runs (the tool chest, the picture and signature, tool settings): the desktop
    /// app reads and writes it as `markupcraft.json`
    pub mc_prefs: serde_json::Value,
    /// the horizontal section cut of the 3D view: on, and its height as a fraction of the model's height
    pub cut3: (bool, f64),
    /// the instance clicked in the 3D view, and what the Properties panel shows for it
    pub sel3: Option<usize>,
    pub sel_props: Vec<cc_io_ifc::props::PropGroup>,
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
    /// the drafting editor's Properties panel (M4.11)
    pub cadprops: cadprops::CadProps,
    /// the drafting editor's Plot dialog (M4.12)
    pub plot: plotdlg::PlotDlg,
    /// the drafting editor's Quick select and Find and replace dialogs (M4.15)
    pub caddlg: qsdlg::CadDialogs,
    /// the clipboard keys go to the clipboard commands (the command line is empty and has the focus)
    clip_keys: bool,
    /// a clipboard command the keys asked for (COPYCLIP, CUTCLIP, PASTECLIP, PASTEORIG)
    clip_cmd: Option<&'static str>,
    /// the sheet whose viewport MSPACE entered (PSPACE goes back to it)
    vp_sheet: cc_cad::SpaceId,
    /// web: a command waits for the picker `Services::pick_files` opened
    pick_waiting: Option<edit::PickReq>,
    /// picture files found next to the drawing the viewer shows (desktop: `Services::read_path` in its folder)
    pub viewer_pictures: cc_cad::pictures::Pictures,
    /// decoded pictures as textures, by the name the drawing gives the file (None = could not be decoded), and what
    /// they were made for (drawing id, picture store version)
    pic_tex: HashMap<String, Option<egui::TextureHandle>>,
    pic_for: (u64, u64),
    /// views before each ZOOM (ZOOM Previous)
    prev_views: Vec<View2D>,
    /// primary-button drag start (world) while drafting: a selection window
    drag_from: Option<(f64, f64)>,
    /// the file the viewer has open (for Edit)
    pub source: Option<ImportedFile>,
    /// the format of the last save (Ctrl+S)
    pub save_format: String,
    /// input events held back to the next frame (everything after an Enter: one answer per frame)
    held: Vec<egui::Event>,
    /// an open CivilCraft project (civilview.rs)
    pub civil: Option<civilview::CivilView>,
    /// an open DetailCraft model (detailview.rs)
    pub detail: Option<detailview::DetailView>,
    /// an open PartCraft part (partview.rs)
    pub part: Option<partview::PartView>,
    /// CadCraft Studio's start screen (hub.rs): recent files, language
    pub hub: hub::Hub,
    /// BimCraft's editing and Draw on the IFC view (bimedit.rs, bimplan.rs)
    pub bimh: bimedit::Host,
}

impl Default for UiApp {
    fn default() -> Self {
        // modelcraft.js: library materials export, print and show as their colour
        cc_io_m3d::util::set_material_colors(|id| mcmats::material(id).map(|m| m.color));
        UiApp {
            services: Services::default(),
            scene3: None,
            model3: None,
            mc: None,
            open_split: String::new(),
            scene_seq: 0,
            ifc3: None,
            ifc_groups: Vec::new(),
            hidden3: HashSet::new(),
            iso3: None,
            bim: bimtools::BimTools::default(),
            ifc_name: String::new(),
            pdf: None,
            mc_prefs: serde_json::Value::Null,
            cut3: (false, 0.5),
            sel3: None,
            sel_props: Vec::new(),
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
            cadprops: cadprops::CadProps::default(),
            plot: plotdlg::PlotDlg::default(),
            caddlg: qsdlg::CadDialogs::default(),
            clip_keys: false,
            clip_cmd: None,
            vp_sheet: 0,
            pick_waiting: None,
            viewer_pictures: cc_cad::pictures::Pictures::new(),
            pic_tex: HashMap::new(),
            pic_for: (u64::MAX, 0),
            prev_views: Vec::new(),
            drag_from: None,
            source: None,
            save_format: "2018".into(),
            held: Vec::new(),
            civil: None,
            detail: None,
            part: None,
            hub: hub::Hub::default(),
            bimh: bimedit::Host::default(),
        }
    }
}

impl UiApp {
    pub fn new(services: Services) -> Self {
        UiApp { services, ..Default::default() }
    }

    /// Which app the window shows now (the control channel's `windows_list`; "studio" = the start screen).
    pub fn active_app(&self) -> &'static str {
        if self.part.is_some() {
            "partcraft"
        } else if self.detail.is_some() {
            "detailcraft"
        } else if self.civil.is_some() {
            "civilcraft"
        } else if let Some(p) = &self.pdf {
            if p.mode == "takeoff" { "takeoffcraft" } else { "markupcraft" }
        } else if self.mc.is_some() || self.model3.is_some() {
            "modelcraft"
        } else if self.ifc3.is_some() {
            "bimcraft"
        } else if self.editor.is_some() || self.doc.is_some() {
            "cadcraft"
        } else {
            "studio"
        }
    }

    /// Open bytes as a drawing (any failure becomes a message, never a crash).
    pub fn open_bytes(&mut self, name: &str, bytes: &[u8]) {
        self.open_file(&ImportedFile { name: name.to_string(), bytes: bytes.to_vec(), dir: None });
    }

    /// Open a file; linked drawings are looked for in its folder (desktop) through `Services::read_xref`.
    pub fn open_file(&mut self, f: &ImportedFile) {
        // a library item onto the model being edited: placing starts (the model stays)
        if self.bimh.ed.is_some() && bimplace::open_item(self, f) {
            return;
        }
        self.hub.note(&f.name, f.dir.as_deref(), hub::app_for(&f.name), hub::now_ms());
        bimedit::opened(self);
        if detailview::open_file(self, f) {
            return;
        }
        let lower = f.name.to_ascii_lowercase();
        if partview::is_part_file(&lower, &f.bytes) {
            partview::open_file(self, f);
            return;
        }
        self.part = None;
        if civilview::is_civil_file(&lower, &f.bytes) {
            civilview::open_file(self, f);
            return;
        }
        self.civil = None;
        if lower.ends_with(".pdf")
            && let Some(p) = self.pdf.as_mut()
        {
            // a plan set is open: the PDF joins it, as 0.5's Open does
            if let Err(e) = p.add_file(&f.name, &f.bytes) {
                self.error = Some(format!("{}: {e}", f.name));
            }
            return;
        }
        if lower.ends_with(".pdf") {
            match pdfview::PdfView::open(&f.name, &f.bytes) {
                Ok(mut v) => {
                    v.tools.load_prefs(&self.mc_prefs);
                    self.doc = None;
                    self.editor = None;
                    self.source = None;
                    self.error = None;
                    self.model3 = None;
                    self.mc = None;
                    self.scene3 = None;
                    self.ifc3 = None;
                    self.ifc_groups.clear();
                    self.civil = None;
                    self.pdf = Some(v);
                }
                Err(e) => self.error = Some(format!("{}: {e}", f.name)),
            }
            return;
        }
        self.pdf = None;
        if lower.ends_with(".modelcraft") {
            self.open_model(f);
            return;
        }
        if lower.ends_with(".ifc") {
            self.open_ifc(f);
            return;
        }
        if let Some(fmt) = cc_m3d_files::import_format(&lower).filter(|x| !matches!(x.as_str(), "dxf" | "dwg")) {
            self.import_model(f, &fmt);
            return;
        }
        let dir = f.dir.clone();
        let reader = &mut self.services.read_xref;
        let mut resolve = |x: &str| -> Option<Vec<u8>> {
            match (dir.as_deref(), reader.as_mut()) {
                (Some(d), Some(r)) => r(d, x),
                _ => None,
            }
        };
        let res = document::open_with(&f.name, &f.bytes, &mut resolve);
        let ok = res.is_ok();
        self.loaded(res);
        if ok {
            self.source = Some(f.clone());
            self.find_pictures(f.dir.as_deref());
        }
    }

    /// A ModelCraft model file (`.modelcraft`: Model.toJSON) → the 3D view.
    fn open_model(&mut self, f: &ImportedFile) {
        let t0 = web_time::Instant::now();
        let parsed = std::str::from_utf8(&f.bytes).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok());
        let Some(j) = parsed else {
            self.error = Some(format!("{}: not a ModelCraft model (JSON expected)", f.name));
            return;
        };
        let tp = t0.elapsed().as_secs_f64() * 1000.0;
        let mut model = cc_m3d::model::Model::new();
        // openFile: the file's imported textures, the model, then its saved view (camera and style)
        model.set_images(j.get("images").unwrap_or(&serde_json::Value::Null));
        match model.load(&j, false) {
            Ok(()) => {
                let t1 = web_time::Instant::now();
                self.show_model(model, &f.name);
                self.open_split = format!(
                    "JSON {tp:.1} ms, parse + load {:.1} ms, view {:.1} ms",
                    (t1 - t0).as_secs_f64() * 1000.0,
                    t1.elapsed().as_secs_f64() * 1000.0
                );
                if let Some(mc) = self.mc.as_mut() {
                    mc.apply_view(&j);
                    mc.message = modelcraft::mt("app.open.done", &[("file", &f.name)]);
                }
            }
            Err(e) => self.error = Some(format!("{}: {e}", f.name)),
        }
    }

    /// An IFC model (cc-ifc with cc-manifold for openings) → the 3D view.
    fn open_ifc(&mut self, f: &ImportedFile) {
        let t0 = web_time::Instant::now();
        match cc_io_ifc::view::view(&f.bytes, &cc_io_ifc::geo::bool_ops::ManifoldOps) {
            Ok(v) => {
                let read_ms = t0.elapsed().as_secs_f64() * 1000.0;
                self.scene_seq = self.scene_seq.wrapping_add(1);
                let scene = ifc3d::scene_of(&v, self.scene_seq, 1_500_000);
                let mut view = view3d::View3::default();
                self.style3(&mut view);
                let tris: usize = v.pieces.iter().filter_map(|p| v.meshes.get(p.mesh)).map(|m| m.idx.len() / 3).sum();
                self.message = format!(
                    "{}: {} products, {} pieces, {} triangles · read + geometry {:.0} ms · middle / right drag orbits, Shift pans, wheel zooms",
                    f.name,
                    v.products,
                    v.pieces.len(),
                    tris,
                    read_ms
                );
                self.doc = None;
                self.editor = None;
                self.source = None;
                self.error = None;
                self.model3 = None;
                self.mc = None;
                self.scene3 = Some((scene, view));
                self.sel3 = None;
                self.sel_props.clear();
                self.ifc3 = Some((std::sync::Arc::new(f.bytes.clone()), v.pieces.iter().map(|p| p.product).collect()));
                self.ifc_groups = v.pieces.iter().map(|p| (p.storey.clone(), p.ty.clone())).collect();
                self.hidden3.clear();
                self.iso3 = None;
                self.bim = bimtools::BimTools::default();
                self.ifc_name = f.name.clone();
            }
            Err(e) => self.error = Some(format!("{}: {}", f.name, e.0)),
        }
    }

    /// A SketchUp / OBJ / glTF / Collada file → a group in the open model (modelcraft.js importFile), or a new model.
    fn import_model(&mut self, f: &ImportedFile, fmt: &str) {
        let t0 = web_time::Instant::now();
        let input = match cc_m3d_files::decode_input(fmt, f.bytes.clone()) {
            Ok(i) => i,
            Err(e) => {
                self.error = Some(format!("{}: {e}", f.name));
                return;
            }
        };
        let name = cc_m3d_files::import_name(&f.name);
        let into_open = self.mc.is_some();
        let mut model = match self.mc.as_ref() {
            Some(mc) => mc.tools.host.model.clone(),
            None => cc_m3d::model::Model::new(),
        };
        match cc_m3d_files::run_import(&mut model, fmt, &input, &name, None) {
            Ok(_) => {
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                if into_open {
                    if let Some(mc) = self.mc.as_mut() {
                        mc.tools.host.set_model(model);
                        mc.message = format!("Imported {} in {ms:.0} ms", f.name);
                    }
                } else {
                    self.show_model(model, &name);
                    self.message = format!("Opened {} in {ms:.0} ms", f.name);
                }
            }
            Err(e) => self.error = Some(format!("{}: {}", f.name, cc_m3d_files::err_text(&e))),
        }
    }

    /// saveModel: the open model as a `.modelcraft` file (model, view, images), through the host's save.
    fn save_model(&mut self, as_name: Option<&str>) -> Result<(), String> {
        let mc = self.mc.as_mut().ok_or("file.save: open a model first")?;
        let _ = mc.tools.escape();
        // a name ending in .modelcraft saves under that name (scripts); otherwise the model's own
        let name = match as_name.filter(|n| n.to_lowercase().ends_with(".modelcraft")) {
            Some(n) => n.to_string(),
            None => format!("{}.modelcraft", mc.base_name()),
        };
        let text = mc.save_text();
        let save = self.services.save.as_mut().ok_or("file.save: saving is not available here")?;
        let done = save(&name, text.into_bytes())?;
        if let Some(mc) = self.mc.as_mut() {
            mc.message = match done {
                Some(to) if to != name => modelcraft::mt("app.save.written", &[("file", &to)]),
                _ => modelcraft::mt("app.save.downloaded", &[("file", &name)]),
            };
            mc.tools.host.model.dirty = false;
        }
        Ok(())
    }

    /// A file a ModelCraft panel made (3D printing's STL / 3MF) goes to the host's save / download.
    fn flush_model_save(&mut self) -> Result<(), String> {
        let Some((name, bytes)) = self.mc.as_mut().and_then(|m| m.pending_save.take()) else { return Ok(()) };
        let save = self.services.save.as_mut().ok_or("saving is not available here")?;
        save(&name, bytes)?;
        Ok(())
    }

    /// Export the open model in one of ModelCraft's formats (a download on the web, a save dialog on the desktop).
    fn export_model(&mut self, format: &str) -> Result<(), String> {
        let mc = self.mc.as_mut().ok_or("file.export: open a model first")?;
        let ext = cc_m3d_files::EXPORT_FORMATS.iter().find(|x| x.0 == format).map(|x| x.2).ok_or("file.export: unknown format")?;
        let t0 = web_time::Instant::now();
        let opts = cc_m3d_files::ExportOpts { name: Some(mc.name.clone()), ..Default::default() };
        let (bytes, _report) = cc_m3d_files::export(&mut mc.tools.host.model, format, &opts)?;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let base = cc_m3d_files::import_name(&mc.name);
        let name = format!("{base}.{ext}");
        let size = bytes.len();
        let save = self.services.save.as_mut().ok_or("file.export: saving is not available here")?;
        if let Some(to) = save(&name, bytes)? {
            self.message = format!("Exported {to} ({:.0} KB) in {ms:.0} ms", size as f64 / 1024.0);
        }
        Ok(())
    }

    /// Show a model in the 3D view (any drawing open is closed).
    fn show_model(&mut self, model: cc_m3d::model::Model, name: &str) {
        // the model goes straight to ModelCraft's view (no copy, one scene build)
        let (defs, faces) = (model.defs.len(), model.defs.values().map(|d| d.faces.len()).sum::<usize>());
        self.doc = None;
        self.editor = None;
        self.source = None;
        self.error = None;
        self.part = None;
        self.scene3 = None;
        self.model3 = None;
        self.ifc3 = None;
        self.ifc_groups.clear();
        self.hidden3.clear();
        self.sel3 = None;
        self.sel_props.clear();
        match modelcraft::ModelView::new(model, name, self.preset == Preset::Dark) {
            Ok(mut mv) => {
                let mut v = view3d::View3::default();
                self.style3(&mut v);
                mv.mode = if !v.style.faces {
                    "wire"
                } else if v.style.xray {
                    "xray"
                } else if v.style.hidden_line {
                    "hidden"
                } else {
                    "shaded"
                }
                .into();
                mv.edges = v.style.edges;
                mv.apply_style();
                self.message = format!("{name}: {defs} definitions, {faces} faces · middle / right drag orbits, Shift pans, wheel zooms");
                self.mc = Some(mv);
            }
            Err(e) => self.error = Some(format!("{name}: {e}")),
        }
    }

    /// Hide or show a storey ("s:<name>") or class ("c:<name>") of the IFC model; "*" shows everything.
    pub fn toggle3(&mut self, key: &str) {
        if key == "*" {
            self.hidden3.clear();
        } else if !self.hidden3.remove(key) {
            self.hidden3.insert(key.to_string());
        }
        self.apply_hidden3();
    }

    /// What the IFC view hides: storeys / classes switched off, and Find's isolate or hide.
    fn apply_hidden3(&mut self) {
        let groups = &self.ifc_groups;
        let hidden = &self.hidden3;
        let iso = &self.iso3;
        let products = self.ifc3.as_ref().map(|(_, p)| p);
        if let Some((scene, _)) = self.scene3.as_mut() {
            scene.set_hidden(&|i| {
                let by_group =
                    groups.get(i).is_some_and(|(st, ty)| hidden.contains(&format!("s:{st}")) || hidden.contains(&format!("c:{ty}")));
                let by_find = match (iso, products.and_then(|p| p.get(i))) {
                    (Some((only, set)), Some(p)) => set.contains(&p.to_bits()) != *only,
                    _ => false,
                };
                by_group || by_find
            });
        }
    }

    /// Carry out what the BimCraft panels asked for.
    fn bim_act(&mut self, a: bimtools::BimAct) {
        match a {
            bimtools::BimAct::Isolate(keys) => self.iso3 = Some((true, keys.iter().map(|k| k.to_bits()).collect())),
            bimtools::BimAct::Hide(keys) => self.iso3 = Some((false, keys.iter().map(|k| k.to_bits()).collect())),
            bimtools::BimAct::ShowAll => {
                self.iso3 = None;
                self.hidden3.clear();
            }
            bimtools::BimAct::Save(name, bytes) => {
                match self.services.save.as_mut().map(|save| save(&name, bytes)) {
                    Some(Ok(Some(to))) => self.message = bimtools::bt("qto.downloaded", &[("name", &to)]),
                    Some(Ok(None)) => {}
                    Some(Err(e)) => self.error = Some(e),
                    None => self.error = Some("Saving is not available here".into()),
                }
                return;
            }
        }
        self.apply_hidden3();
    }

    fn tool_palette(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let cur = self.mc.as_ref().map(|m| m.tools.name.clone()).unwrap_or_default();
        let mut pick: Option<&str> = None;
        let mut undo = false;
        let mut redo = false;
        let mut edit: Option<String> = None;
        let frame = egui::Frame::new().fill(t.panel).inner_margin(6);
        egui::Panel::left("mc_tools").exact_size(116.0).frame(frame).show(ui, |ui| {
            ui.label(RichText::new("Tools").strong().color(t.text_strong));
            ui.separator();
            for (name, label) in modelcraft::PALETTE {
                let on = cur == *name;
                if ui.add_sized([104.0, 22.0], egui::Button::new(*label).selected(on)).clicked() {
                    pick = Some(name);
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                undo = ui.button("Undo").clicked();
                redo = ui.button("Redo").clicked();
            });
            ui.separator();
            edit = mcedit::palette_menus(ui);
        });
        if let Some(mc) = self.mc.as_mut() {
            if let Some(a) = edit
                && let Err(e) = mc.act(&a)
            {
                mc.message = e;
            }
            if let Some(n) = pick {
                mc.set_tool(n);
            }
            if undo {
                mc.undo();
            }
            if redo {
                mc.redo();
            }
        }
    }

    fn model_panel(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        if self.ifc_groups.is_empty() {
            return;
        }
        // first-seen order, with counts of pieces
        let mut storeys: Vec<(String, usize)> = Vec::new();
        let mut classes: Vec<(String, usize)> = Vec::new();
        for (st, ty) in &self.ifc_groups {
            match storeys.iter_mut().find(|x| &x.0 == st) {
                Some(x) => x.1 += 1,
                None => storeys.push((st.clone(), 1)),
            }
            match classes.iter_mut().find(|x| &x.0 == ty) {
                Some(x) => x.1 += 1,
                None => classes.push((ty.clone(), 1)),
            }
        }
        classes.sort_by(|a, b| a.0.cmp(&b.0));
        let mut toggle: Option<String> = None;
        let mut bim_act: Option<bimtools::BimAct> = None;
        let bytes = self.ifc3.as_ref().map(|(b, _)| b.clone());
        let mut cut = self.cut3;
        let frame = egui::Frame::new().fill(t.panel).inner_margin(6);
        egui::Panel::left("model3").default_size(230.0).size_range(160.0..=380.0).resizable(true).frame(frame).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Model").strong().color(t.text_strong));
                if ui.small_button("Show all").clicked() {
                    toggle = Some("*".into());
                }
            });
            ui.checkbox(&mut cut.0, RichText::new("Section cut").color(t.text)).on_hover_text("Cut the model with a horizontal plane");
            if cut.0 {
                ui.add(egui::Slider::new(&mut cut.1, 0.0..=1.0).show_value(false).text("height"));
            }
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (title, list, prefix) in [("Storeys", &storeys, "s:"), ("Classes", &classes, "c:")] {
                    egui::CollapsingHeader::new(RichText::new(title).color(t.text_strong)).default_open(true).show(ui, |ui| {
                        for (name, n) in list.iter() {
                            let key = format!("{prefix}{name}");
                            let mut on = !self.hidden3.contains(&key);
                            let label = if name.is_empty() { "(none)".to_string() } else { name.clone() };
                            if ui.checkbox(&mut on, RichText::new(format!("{label}  ({n})")).color(t.text)).changed() {
                                toggle = Some(key);
                            }
                        }
                    });
                }
                if let Some(bytes) = &bytes {
                    bim_act = self.bim.panels(ui, &t, bytes, &self.ifc_name);
                }
            });
        });
        if let Some(a) = bim_act {
            self.bim_act(a);
        }
        if let Some(k) = toggle {
            self.toggle3(&k);
        }
        if cut != self.cut3 {
            self.cut3 = cut;
            self.apply_cut3();
        }
    }

    /// The section cut on the 3D view's style (a horizontal plane at the chosen fraction of the model's height).
    fn apply_cut3(&mut self) {
        let (on, f) = self.cut3;
        if let Some((scene, view)) = self.scene3.as_mut() {
            view.style.section = on.then(|| ([0.0, 0.0, 1.0], scene.min[2] + (scene.max[2] - scene.min[2]) * f));
        }
    }

    /// Select what is under a pixel of the 3D view (None = clear): the whole product (IFC) or placement is tinted.
    pub fn select3(&mut self, at: Option<(f64, f64)>) {
        let Some((scene, view)) = self.scene3.as_mut() else { return };
        let hit = at.and_then(|(x, y)| view3d::pick(scene, &view.cam, x, y));
        self.sel3 = hit.map(|h| h.instance);
        self.sel_props.clear();
        const SEL: [u8; 4] = [70, 130, 240, 255];
        match (&self.ifc3, hit) {
            (Some((bytes, products)), Some(h)) => {
                let pid = products.get(h.instance).copied();
                scene.set_tints(&|i| (products.get(i).copied() == pid && pid.is_some()).then_some(SEL));
                if let (Some(pid), Ok(file)) = (pid, cc_io_ifc::model::IfcFile::open(bytes)) {
                    self.sel_props = cc_io_ifc::props::properties(&file, pid);
                    let name =
                        self.sel_props.first().and_then(|g| g.rows.iter().find(|r| r.0 == "Name")).map(|r| r.1.clone()).unwrap_or_default();
                    let class = self
                        .sel_props
                        .first()
                        .and_then(|g| g.rows.iter().find(|r| r.0 == "Class"))
                        .map(|r| r.1.clone())
                        .unwrap_or_default();
                    self.message = format!("Selected {class} {name} (#{pid})");
                }
            }
            (None, Some(h)) => {
                scene.set_tints(&|i| (i == h.instance).then_some(SEL));
                self.message = format!("Selected placement {} at {:.3}, {:.3}, {:.3}", h.instance, h.point[0], h.point[1], h.point[2]);
            }
            (_, None) => {
                scene.set_tints(&|_| None);
                self.message = "Nothing selected".into();
            }
        }
    }

    fn props_panel(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        if self.sel_props.is_empty() {
            return;
        }
        let groups = self.sel_props.clone();
        // the panel never grows past half the window; values wrap anywhere (point lists have no spaces) and a huge one
        // (a terrain's survey points in one string) shows its start and its length
        let max_w = (ui.available_width() * 0.45).clamp(220.0, 520.0);
        let frame = egui::Frame::new().fill(t.panel).inner_margin(6);
        egui::Panel::right("props3").default_size(300.0).size_range(200.0..=max_w).resizable(true).frame(frame).show(ui, |ui| {
            ui.label(RichText::new("Properties").strong().color(t.text_strong));
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                bimedit::props_top(self, ui);
                for g in &groups {
                    egui::CollapsingHeader::new(RichText::new(&g.title).color(t.text_strong)).default_open(true).show(ui, |ui| {
                        // name above value (a grid needs a frame to settle its column widths)
                        let col = ui.available_width().max(80.0);
                        for (k, v) in &g.rows {
                            ui.label(RichText::new(k).small().color(t.text_dim));
                            let n = v.chars().count();
                            let shown = if n > 160 {
                                format!("{}… ({n} characters)", v.chars().take(160).collect::<String>())
                            } else {
                                v.clone()
                            };
                            let mut job = egui::text::LayoutJob::single_section(
                                shown,
                                egui::TextFormat::simple(egui::TextStyle::Body.resolve(ui.style()), t.text),
                            );
                            job.wrap = egui::text::TextWrapping { max_width: col, break_anywhere: true, ..Default::default() };
                            ui.add(egui::Label::new(job));
                            ui.add_space(3.0);
                        }
                    });
                }
            });
        });
    }

    /// The 3D view's colours from the theme (view3d.js's paper / model colours).
    fn style3(&self, view: &mut view3d::View3) {
        let dark = self.preset == Preset::Dark;
        let t = self.tokens;
        let c = t.canvas_bg;
        view.style.background = [f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0, 1.0];
        view.style.edge = if dark { [0.85, 0.87, 0.9, 1.0] } else { [0.09, 0.13, 0.17, 1.0] };
        view.style.back = if dark { model3d::BACK_MODEL } else { model3d::BACK_PAPER };
    }

    /// The open file imported into an editable drawing (dwg-import.js / dxf.js readDxf → Doc.load).
    fn edit_source(&mut self) -> Result<(), String> {
        let f = self.source.clone().ok_or("file.edit: open a DWG or DXF first")?;
        let t0 = web_time::Instant::now();
        let input = cc_io_cad::import_bytes(&f.name, &f.bytes).map_err(|e| format!("{}: {}", f.name, e.0))?;
        let t1 = web_time::Instant::now();
        let mut doc = cc_cad::Doc::new();
        doc.load(&input);
        drop(input);
        // the picture files found next to the drawing come along (IMAGE lists them as shown)
        for (k, rec) in self.viewer_pictures.iter() {
            doc.pictures.insert(k.clone(), std::sync::Arc::clone(rec));
        }
        let t2 = web_time::Instant::now();
        let stem = f.name.rsplit_once('.').map_or(f.name.as_str(), |(a, _)| a).to_string();
        let keep_space = self.space;
        let hidden = std::mem::take(&mut self.hidden);
        self.editor = Some(edit::Editor::new(doc, &stem));
        self.doc = None;
        self.sync_editor();
        let t3 = web_time::Instant::now();
        let n = self.doc.as_ref().map_or(0, |d| d.drawing.spaces.len());
        self.views = vec![None; n];
        self.space = if keep_space < n { keep_space } else { 0 };
        self.hidden = hidden;
        self.pending_fit = true;
        self.error = None;
        let ms = |a: web_time::Instant, b: web_time::Instant| b.duration_since(a).as_secs_f64() * 1000.0;
        self.message =
            format!("Editing {} · import {:.0} ms · load {:.0} ms · display {:.0} ms", f.name, ms(t0, t1), ms(t1, t2), ms(t2, t3));
        Ok(())
    }

    /// Save the drawing open for editing in `format` (one of `cc_io_cad::SAVE_FORMATS`) through `Services::save`.
    fn save_as(&mut self, format: &str) -> Result<(), String> {
        if !cc_io_cad::SAVE_FORMATS.contains(&format) {
            return Err(format!("file.save: unknown format {format} (one of {})", cc_io_cad::SAVE_FORMATS.join(", ")));
        }
        let ed = self.editor.as_ref().ok_or("file.save: no drawing is open for editing")?;
        let t0 = web_time::Instant::now();
        let (bytes, ext, _report) = cc_io_cad::save_bytes(&ed.eng.doc(), format, None).map_err(|e| format!("file.save: {}", e.0))?;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let name = format!("{}.{ext}", ed.name);
        let size = bytes.len();
        let save = self.services.save.as_mut().ok_or("file.save: saving is not available here")?;
        if let Some(to) = save(&name, bytes)? {
            self.save_format = format.to_string();
            let label = if format.starts_with("dxf") { format.replace("dxf", "DXF ") } else { format!("DWG {format}") };
            self.message = format!("Saved {to} ({label}, {:.0} KB) in {ms:.0} ms", size as f64 / 1024.0);
            if let Some(ed) = self.editor.as_mut() {
                ed.eng.doc_mut().mark_saved();
            }
        }
        Ok(())
    }

    /// The picture files the open drawing names, looked for in its folder (desktop: `Services::read_path`; the web
    /// has no folders: IMAGE in the editor takes the files, as 0.5 does in the browser).
    fn find_pictures(&mut self, dir: Option<&str>) {
        self.viewer_pictures = cc_cad::pictures::Pictures::new();
        let (Some(dir), Some(read)) = (dir, self.services.read_path.as_mut()) else { return };
        let files: Vec<String> = self.doc.as_ref().map(|d| d.drawing.images.iter().map(|i| i.file.clone()).collect()).unwrap_or_default();
        for file in files {
            let base = cc_cad::pictures::base_name(&file);
            if base.is_empty() || base.contains("..") {
                continue;
            }
            if let Ok(got) = read(&format!("{dir}/{base}")) {
                let _ = cc_cad::pictures::add_picture(&mut self.viewer_pictures, &got.name, &got.bytes);
            }
        }
    }

    /// Decode the pictures the shown drawing uses into textures (once each; again when the picture files change).
    fn picture_textures(&mut self, ctx: &egui::Context) -> HashMap<String, egui::TextureId> {
        let Some(doc) = &self.doc else { return HashMap::new() };
        let version = self.editor.as_ref().map_or(self.viewer_pictures.len() as u64, |ed| ed.eng.doc().picture_version);
        if self.pic_for != (doc.id, version) {
            self.pic_tex.clear();
            self.pic_for = (doc.id, version);
        }
        let max_side = u32::try_from(ctx.input(|i| i.max_texture_side)).unwrap_or(4096).clamp(256, 8192);
        let mut out = HashMap::new();
        for im in doc.drawing.spaces.iter().flat_map(|s| s.images.iter()) {
            if out.contains_key(&im.file) {
                continue;
            }
            if !self.pic_tex.contains_key(&im.file) {
                let rec = match self.editor.as_ref() {
                    Some(ed) => cc_cad::pictures::find_picture(&ed.eng.doc().pictures, &im.file).cloned(),
                    None => cc_cad::pictures::find_picture(&self.viewer_pictures, &im.file).cloned(),
                };
                let Some(rec) = rec else { continue };
                let tex = match cc_io_cad::raster::decode(&rec.bytes, max_side) {
                    Ok(px) => {
                        let size = [usize::try_from(px.w).unwrap_or(0), usize::try_from(px.h).unwrap_or(0)];
                        let img = egui::ColorImage::from_rgba_unmultiplied(size, &px.px);
                        Some(ctx.load_texture(format!("picture:{}", rec.name), img, egui::TextureOptions::LINEAR))
                    }
                    Err(e) => {
                        self.message = format!("{}: {e}", rec.name);
                        None
                    }
                };
                self.pic_tex.insert(im.file.clone(), tex);
            }
            if let Some(Some(t)) = self.pic_tex.get(&im.file) {
                out.insert(im.file.clone(), t.id());
            }
        }
        out
    }

    fn loaded(&mut self, res: Result<Loaded, String>) {
        self.civil = None;
        self.part = None;
        self.scene3 = None;
        self.model3 = None;
        self.mc = None;
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
        detailview::before_run(self, id);
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
                self.source = Some(ImportedFile { name: "kitchen-equipment.dwg".into(), bytes: SAMPLE_DWG.to_vec(), dir: None });
            }
            "file.edit" => self.edit_source()?,
            "file.save" if self.mc.is_some() && self.editor.is_none() => self.save_model(params.name.as_deref())?,
            "file.save" => {
                let f = params.name.clone().unwrap_or_else(|| self.save_format.clone());
                self.save_as(&f)?;
            }
            "view.fit" => {
                self.pending_fit = true;
                if let Some(pv) = self.part.as_mut() {
                    pv.fit();
                }
                if let Some(c) = self.civil.as_mut() {
                    c.fit();
                }
                if let Some(d) = self.detail.as_mut() {
                    d.fit();
                }
                if let Some(p) = self.pdf.as_mut() {
                    p.fit();
                }
                if let Some(mc) = self.mc.as_mut() {
                    mc.fit();
                }
                if let Some((_, v)) = self.scene3.as_mut() {
                    v.fit_pending = true;
                }
            }
            "view.standard" if self.part.is_some() => {
                let n = params.name.clone().unwrap_or_else(|| "iso".into());
                if let Some(pv) = self.part.as_mut() {
                    pv.standard(&n);
                }
            }
            "view.standard" if self.mc.is_some() => {
                let n = params.name.clone().unwrap_or_else(|| "iso".into());
                if let Some(mc) = self.mc.as_mut() {
                    mc.standard(&n);
                }
            }
            "view.standard" => {
                let n = params.name.clone().unwrap_or_else(|| "iso".into());
                let dir = match n.as_str() {
                    "top" => [0.0, -1e-4, 1.0],
                    "bottom" => [0.0, -1e-4, -1.0],
                    "front" => [0.0, -1.0, 0.0],
                    "back" => [0.0, 1.0, 0.0],
                    "left" => [-1.0, 0.0, 0.0],
                    "right" => [1.0, 0.0, 0.0],
                    _ => [1.0, -1.15, 0.85],
                };
                if let Some((scene, view)) = self.scene3.as_mut() {
                    view.cam.fit(scene.min, scene.max, Some(dir));
                }
            }
            "view.projection" if self.mc.is_some() => {
                if let Some(mc) = self.mc.as_mut() {
                    mc.tools.host.cam.persp = !mc.tools.host.cam.persp;
                }
            }
            "view.projection" => {
                if let Some((_, view)) = self.scene3.as_mut() {
                    view.cam.persp = !view.cam.persp;
                }
            }
            "file.export" => {
                let f = params.name.clone().ok_or("file.export: name is required")?;
                self.export_model(&f)?;
            }
            "tool.set" => {
                let n = params.name.clone().ok_or("tool.set: name is required")?;
                self.mc.as_mut().ok_or("tool.set: open a model first")?.set_tool(&n);
            }
            "tool.value" => {
                let n = params.name.clone().ok_or("tool.value: name is required")?;
                let mc = self.mc.as_mut().ok_or("tool.value: open a model first")?;
                let _ = mc.tools.value(&n);
            }
            "model.edit" => {
                // ModelCraft's menu and panel actions (mcact.rs): select-all, make-group, tree-eye:<path>:<iid>, tag-add, …
                let n = params.name.clone().ok_or("model.edit: name is required")?;
                let mc = self.mc.as_mut().ok_or("model.edit: open a model first")?;
                mc.act(&n)?;
                self.flush_model_save()?;
            }
            "bim.find" => {
                // BimCraft Find with one rule "field:op:value" (e.g. "cls:is:IfcWall"); shows only what it finds
                let spec = params.name.clone().ok_or("bim.find: name is required (field:op:value)")?;
                let bytes = self.ifc3.as_ref().map(|(b, _)| b.clone()).ok_or("bim.find: open an IFC model first")?;
                let keys = self.bim.find_rule(&bytes, &spec)?;
                self.bim_act(bimtools::BimAct::Isolate(keys));
            }
            "bim.totals" => {
                // BimCraft Quantity totals grouped by a field, optionally adding up a number field ("cls:Qto….NetVolume")
                let spec = params.name.clone().ok_or("bim.totals: name is required (group[:sum])")?;
                let bytes = self.ifc3.as_ref().map(|(b, _)| b.clone()).ok_or("bim.totals: open an IFC model first")?;
                self.bim.totals_by(&bytes, &spec);
            }
            "markup.tool" => {
                // MarkupCraft tool on the sheet view: select, pan, pen, highlight, line, arrow, rect, ellipse, cloudrect, callout,
                // text, measure, area, calibrate
                let n = params.name.clone().ok_or("markup.tool: name is required")?;
                if !markuptools::TOOLS.contains(&n.as_str()) {
                    return Err(format!("markup.tool: unknown tool {n}"));
                }
                let p = self.pdf.as_mut().ok_or("markup.tool: open a PDF first")?;
                let key = p.sheet_key();
                p.tools.set_tool(&mut p.store, &key, &n);
            }
            "takeoff.mode" => {
                // the sheet view's TakeoffCraft tools ("markup" goes back)
                let p = self.pdf.as_mut().ok_or("takeoff.mode: open a PDF first")?;
                p.mode = if params.name.as_deref() == Some("markup") { "markup" } else { "takeoff" };
            }
            "takeoff.scale" => {
                // a preset scale for the open sheet (a1/4, m100, …)
                let k = params.name.clone().ok_or("takeoff.scale: name = preset key")?;
                let p = self.pdf.as_mut().ok_or("takeoff.scale: open a PDF first")?;
                let (key, uu) = (p.takeoff.sheet(p.page), p.cur.pages.get(p.page).map_or(1.0, |i| i.user_unit));
                p.takeoff.set_preset(&key, &k, uu);
            }
            "takeoff.item" => {
                // add an item "type:name" (count, linear, area) and make it current
                let spec = params.name.clone().ok_or("takeoff.item: name = type:name")?;
                let (ty, name) = spec.split_once(':').unwrap_or(("count", spec.as_str()));
                self.pdf.as_mut().ok_or("takeoff.item: open a PDF first")?.takeoff.add_item(name, ty);
            }
            "takeoff.finish" => {
                // end the length / area being clicked (as Enter)
                let p = self.pdf.as_mut().ok_or("takeoff.finish: open a PDF first")?;
                let key = p.takeoff.sheet(p.page);
                p.takeoff.finish(&key);
            }
            "markup.scale" => {
                // the open sheet's MarkupCraft scale: a SCALE_PRESETS label (1/4" = 1'-0", 1:100, …) or "none"
                let n = params.name.clone().ok_or("markup.scale: name = preset label or none")?;
                let p = self.pdf.as_mut().ok_or("markup.scale: open a PDF first")?;
                let key = p.sheet_key();
                let cal = if n == "none" {
                    None
                } else {
                    let hit = cc_markup::units::scale_presets()
                        .into_iter()
                        .find_map(|(_, list, unit)| list.into_iter().find(|(l, _)| *l == n).map(|(l, r)| (l, r, unit)));
                    let (l, r, u) = hit.ok_or_else(|| format!("markup.scale: unknown scale {n}"))?;
                    Some(cc_markup::units::preset_calibration(l, r, u))
                };
                p.tools.set_scale(&mut p.store, &key, cal);
            }
            "takeoff.autocount" => {
                // Auto count on the open sheet: "tool" switches the box tool (a drag searches on a thread); name = "x0,y0,x1,y1" (the symbol's box, PDF user space; "any:" first for
                // every 15°), then "accept" adds the kept matches, "discard" drops them; a number (or factor) sets the match threshold
                let spec = params.name.clone().ok_or("takeoff.autocount: name = x0,y0,x1,y1 | accept | discard")?;
                let p = self.pdf.as_mut().ok_or("takeoff.autocount: open a PDF first")?;
                match spec.as_str() {
                    // a bare number is the match threshold (the review's slider)
                    _ if spec.parse::<f64>().is_ok() => {}
                    "tool" => p.takeoff.ac_tool = !p.takeoff.ac_tool,
                    "accept" => p.takeoff.accept_autocount(),
                    "discard" => p.takeoff.ac = None,
                    _ => {
                        let (any, nums) = spec.strip_prefix("any:").map_or((false, spec.as_str()), |r| (true, r));
                        let v: Vec<f64> = nums.split(',').filter_map(|x| x.trim().parse().ok()).filter(|x: &f64| x.is_finite()).collect();
                        let [x0, y0, x1, y1] = v[..] else { return Err("takeoff.autocount: the box is x0,y0,x1,y1".into()) };
                        p.takeoff.ac_any = any;
                        let key = p.takeoff.sheet(p.page);
                        p.run_autocount(&key, [x0, y0], [x1, y1], false);
                    }
                }
                if let (Some(f), Some(ac)) = (params.factor, p.takeoff.ac.as_mut()) {
                    ac.threshold = f.clamp(crate::autocount::FLOOR, 1.0);
                }
            }
            "takeoff.export" => {
                // a TakeoffCraft export of the open plan set written to `path`: name = xlsx, csv, pdf (marked) or project
                let kind = params.name.clone().ok_or("takeoff.export: name = xlsx, csv, pdf or project")?;
                let p = self.pdf.as_ref().ok_or("takeoff.export: open a PDF first")?;
                let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64);
                let outs = p.takeoff_exports(&kind, now)?;
                let save = self.services.save.as_mut().ok_or("takeoff.export: saving is not available here")?;
                for (name, bytes) in outs {
                    if let Some(to) = save(&name, bytes)? {
                        self.message = format!("Saved {to}");
                    }
                }
            }
            "markup.finish" => {
                // end the area outline being clicked (as Enter)
                let p = self.pdf.as_mut().ok_or("markup.finish: open a PDF first")?;
                let key = p.sheet_key();
                p.tick();
                p.tools.enter(&mut p.store, &key);
            }
            "markup.undo" => {
                let p = self.pdf.as_mut().ok_or("markup.undo: open a PDF first")?;
                p.history(false);
            }
            "pdf.page" => {
                // go to a sheet of the open PDF (1-based)
                let n = params.factor.filter(|f| f.is_finite() && *f >= 1.0).ok_or("pdf.page: factor = page number (1-based)")?;
                self.pdf.as_mut().ok_or("pdf.page: open a PDF first")?.go(n as usize - 1);
            }
            "view.show_all" => self.bim_act(bimtools::BimAct::ShowAll),
            "view.style" => {
                // modelcraft.js style modes (shaded, hidden, wire, xray) and the edges switch; BimCraft's 3D view too
                let n = params.name.clone().ok_or("view.style: name is required")?;
                if self.part.is_none()
                    && let Some(mc) = self.mc.as_mut()
                {
                    let a = if n == "edges" { format!("edges:{}", if mc.edges { "off" } else { "on" }) } else { format!("style:{n}") };
                    return mc.act(&a);
                }
                let style = match (self.part.as_mut(), self.mc.as_mut(), self.scene3.as_mut()) {
                    (Some(pv), _, _) => &mut pv.view.style,
                    (None, Some(mc), _) => &mut mc.style,
                    (None, None, Some((_, v))) => &mut v.style,
                    _ => return Err("view.style: open a model first".into()),
                };
                match n.as_str() {
                    "edges" => style.edges = !style.edges,
                    "shaded" | "hidden" | "wire" | "xray" => {
                        style.faces = n != "wire";
                        style.xray = n == "xray";
                        style.hidden_line = n == "hidden";
                        if n == "wire" {
                            style.edges = true;
                        }
                    }
                    other => return Err(format!("view.style: unknown style {other}")),
                }
            }
            "model.undo" => {
                if let Some(mc) = self.mc.as_mut() {
                    mc.undo();
                }
            }
            "view.cut" => {
                self.cut3 = match params.factor.filter(|f| f.is_finite()) {
                    Some(f) => (true, f.clamp(0.0, 1.0)),
                    None => (false, self.cut3.1),
                };
                self.apply_cut3();
            }
            "model.toggle" => {
                let n = params.name.clone().ok_or("model.toggle: name is required")?;
                self.toggle3(&n);
            }
            "file.sample.ifc" => {
                self.open_bytes("bimcraft-house.ifc", SAMPLE_IFC);
            }
            "view.model.sample" => {
                self.show_model(model3d::sample_model(), "Sample model");
            }
            "view.demo3d" => {
                self.part = None;
                let n = params.factor.filter(|f| f.is_finite() && *f >= 1.0).map_or(12, |f| f.min(400.0) as usize);
                self.scene3 = Some((view3d::demo_scene(n, [500_000.0, 4_000_000.0, 120.0]), view3d::View3::default()));
                self.message = format!(
                    "3D test scene: {} boxes at x = 500 000, y = 4 000 000 · middle / right drag orbits, Shift pans, wheel zooms",
                    n * n
                );
            }
            "file.new" => {
                self.civil = None;
                self.part = None;
                self.scene3 = None;
                self.model3 = None;
                self.model3 = None;
                self.source = None;
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
            "edit.polar" => {
                let ed = self.editor.as_mut().ok_or("edit.polar: no drawing is open for editing")?;
                ed.polar = !ed.polar;
                let inc = ed.hs.borrow().polar_inc;
                self.message = format!("Polar tracking {} (every {}°)", if ed.polar { "on" } else { "off" }, cc_engine::input::fmt(inc));
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
                if let Some(ed) = self.editor.as_mut() {
                    if ed.eng.busy() {
                        ed.escape();
                    }
                    ed.hs.borrow_mut().entered = None;
                    let name = if i == 0 { None } else { ed.eng.doc().layouts.get(i.saturating_sub(1)).map(|l| l.name.clone()) };
                    ed.eng.doc_mut().set_space(name.as_deref());
                    self.sync_editor();
                }
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
                if self.mc.is_none()
                    && let Some(model) = self.model3.take()
                {
                    let view = self.scene3.as_ref().map(|(_, v)| *v);
                    self.show_model(model, "Model");
                    if let (Some(v), Some((_, nv))) = (view, self.scene3.as_mut()) {
                        let style = nv.style;
                        *nv = v;
                        nv.style = style;
                    }
                }
            }
            id if id.starts_with("civil.") => return civilview::run(self, id, params),
            id if id.starts_with("takeoff.") || id.starts_with("markup.") || matches!(id, "pdf.zoom" | "pdf.file") => {
                return pdfcmd::run(self, id, params);
            }
            id if id.starts_with("detail.") => return detailview::run(self, id, params),
            id if id.starts_with("hub.") => return hub::run(self, id, params),
            id if id.starts_with("props.") => return cadprops::run(self, id, params),
            id if id.starts_with("plot.") => return plotdlg::run(self, id, params),
            id if id.starts_with("qs.") || id.starts_with("find.") => return qsdlg::run(self, id, params),
            id if bimedit::COMMANDS.iter().any(|c| c.0 == id) => return bimedit::run(self, id, params),
            id if bimplan::COMMANDS.iter().any(|c| c.0 == id) => return bimplan::run(self, id, params),
            id if bimplace::COMMANDS.iter().any(|c| c.0 == id) => return bimplace::run(self, id, params),
            id if id.starts_with("part.") => {
                partview::run(self, id, params)?;
                self.error = None;
            }
            other => return Err(format!("unknown command {other}")),
        }
        Ok(())
    }

    /// After any input to the editor: rebuild the display model if the drawing changed, apply ZOOM requests.
    pub fn sync_editor(&mut self) {
        let Some(ed) = self.editor.as_mut() else { return };
        if let Some(open) = ed.hs.borrow_mut().props_open.take() {
            self.cadprops.open = open;
        }
        let (plot_req, plot_win) = {
            let mut h = ed.hs.borrow_mut();
            (std::mem::take(&mut h.plot_open), h.plot_window.take())
        };
        if let Some(w) = plot_win {
            if let Some(b) = w {
                self.plot.win = Some(b);
                self.plot.area = "window".into();
            }
            self.plot.open = true;
        }
        if plot_req {
            self.plot.open = true;
        }
        let (qs_req, find_req) = {
            let mut h = ed.hs.borrow_mut();
            (std::mem::take(&mut h.qs_open), std::mem::take(&mut h.find_open))
        };
        if qs_req {
            qsdlg::open_qs(self);
        }
        if find_req {
            qsdlg::open_find(self);
        }
        let Some(ed) = self.editor.as_mut() else { return };
        // MSPACE / PSPACE: the model through the viewport, and back to its sheet
        let enter = ed.hs.borrow_mut().enter_req.take();
        let mut fit_to: Option<(f64, f64, f64, f64)> = None;
        match enter {
            Some(Some((space, id))) => {
                let win = {
                    let d = ed.eng.doc();
                    d.space_map(space).and_then(|m| m.get(&id)).and_then(|e| match &e.geo {
                        cc_cad::entity::Geo::Viewport(v) if v.s > 0.0 => {
                            Some((v.cx - v.w / v.s / 2.0, v.cy - v.h / v.s / 2.0, v.cx + v.w / v.s / 2.0, v.cy + v.h / v.s / 2.0))
                        }
                        _ => None,
                    })
                };
                ed.eng.doc_mut().set_space_id(cc_cad::MODEL);
                fit_to = win;
                self.vp_sheet = space;
            }
            Some(None) => {
                let back = ed.eng.doc().layouts.iter().map(|l| l.space).find(|s| *s == self.vp_sheet);
                if let Some(s) = back {
                    ed.eng.doc_mut().set_space_id(s);
                }
            }
            None => {}
        }
        if ed.take_changed() || self.doc.is_none() {
            let drawing = ed.display.drawing(&ed.eng.doc());
            let n = drawing.spaces.len();
            self.doc = Some(document::from_drawing(&ed.name, drawing));
            self.views.resize(n, None);
            // the tab follows the drawing's open space (LAYOUT opens its sheet; MSPACE the model)
            let d = ed.eng.doc();
            self.space =
                if d.in_model() { 0 } else { d.layouts.iter().position(|l| l.space == d.space).map_or(0, |i| i.saturating_add(1)) };
            if self.space >= n {
                self.space = 0;
            }
        }
        // UCS / PLAN: the view turns so the X axis is level
        if let Some(ang) = ed.hs.borrow_mut().turn_req.take()
            && let Some(Some(v)) = self.views.get_mut(self.space)
        {
            v.twist = -ang;
        }
        // XATTACH / XREF Reload / IMAGEATTACH / IMAGE: the picker (desktop answers at once, web later through `picked`)
        ed.hs.borrow_mut().can_pick = self.services.pick_files.is_some();
        let req = ed.hs.borrow_mut().pick_req.take();
        if let Some(req) = req {
            let (kind, multiple) = match req {
                edit::PickReq::Drawing => ("drawing", false),
                edit::PickReq::Pictures { multiple } => ("pictures", multiple),
            };
            match self.services.pick_files.as_mut().map(|f| f(kind, multiple)) {
                Some(Some(files)) => deliver_pick(ed, req, files),
                Some(None) => self.pick_waiting = Some(req),
                None => deliver_pick(ed, req, Vec::new()),
            }
        }
        if let Some(req) = self.pick_waiting
            && let Some(files) = self.services.picked.as_ref().and_then(|p| p.lock().unwrap_or_else(PoisonError::into_inner).take())
        {
            self.pick_waiting = None;
            deliver_pick(ed, req, files);
        }
        // COUNT's CSV
        let blob = ed.hs.borrow_mut().blob.take();
        if let Some((name, bytes)) = blob {
            match self.services.save.as_mut().map(|f| f(&name, bytes)) {
                Some(Ok(Some(to))) => self.message = format!("Saved {to}"),
                Some(Ok(None)) => {}
                Some(Err(e)) => self.error = Some(e),
                None => self.error = Some("Saving is not available here".into()),
            }
        }
        let Some(ed) = self.editor.as_mut() else { return };
        if let Some((x0, y0, x1, y1)) = fit_to
            && let Some(slot) = self.views.get_mut(self.space)
        {
            let mut v = slot.unwrap_or_default();
            v.width = self.canvas_px.0;
            v.height = self.canvas_px.1;
            v.fit(x0, y0, x1, y1, 0.0);
            *slot = Some(v);
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

    /// `run_ui` for the panels in other modules.
    pub(crate) fn run_ui_pub(&mut self, id: &str, params: &Params) {
        self.run_ui(id, params);
    }

    fn run_ui(&mut self, id: &str, params: &Params) {
        if let Err(e) = self.run(id, params) {
            self.error = Some(e);
        }
    }

    /// Before each frame (hosts call it from `eframe::App::raw_input_hook`): while a drawing is open for editing, a
    /// frame takes the events up to and including the first Enter; the rest wait for the next frame. A fast typist
    /// (or a paste, or a slow frame) can put "LINE⏎0,0⏎" into one frame, and the command line answers one question
    /// per Enter.
    pub fn raw_input_hook(&mut self, raw: &mut egui::RawInput) {
        if !self.held.is_empty() {
            let mut ev = std::mem::take(&mut self.held);
            ev.append(&mut raw.events);
            raw.events = ev;
        }
        if self.editor.is_none() {
            return;
        }
        if self.clip_keys {
            let shift = raw.events.iter().any(|e| matches!(e, egui::Event::Key { modifiers, .. } if modifiers.shift && modifiers.command));
            let mut pick: Option<&'static str> = None;
            raw.events.retain(|e| {
                let c = match e {
                    egui::Event::Copy => Some("COPYCLIP"),
                    egui::Event::Cut => Some("CUTCLIP"),
                    egui::Event::Paste(_) => Some(if shift { "PASTEORIG" } else { "PASTECLIP" }),
                    // an empty system clipboard sends no Paste: the key itself
                    egui::Event::Key { key: egui::Key::V, pressed: true, modifiers, .. } if modifiers.command => {
                        Some(if modifiers.shift { "PASTEORIG" } else { "PASTECLIP" })
                    }
                    _ => None,
                };
                if c.is_some() {
                    pick = c;
                }
                c.is_none()
            });
            if pick.is_some() {
                self.clip_cmd = pick;
            }
        }
        let enter = raw.events.iter().position(|e| matches!(e, egui::Event::Key { key: egui::Key::Enter, pressed: true, .. }));
        if let Some(i) = enter
            && i.saturating_add(1) < raw.events.len()
        {
            self.held = raw.events.split_off(i.saturating_add(1));
        }
    }

    /// Every frame: drain the inbox, then draw.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let t0 = web_time::Instant::now();
        let ctx = ui.ctx().clone();
        if !self.held.is_empty() {
            ctx.request_repaint();
        }
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
        if self.editor.is_some() && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            self.run_ui("file.save", &Params::default());
        }
        if self.editor.is_some() && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::P)) {
            self.run_ui("plot.open", &Params::default());
        }
        if self.editor.is_some() && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Num1)) {
            self.cadprops.open = !self.cadprops.open;
        }
        self.app_bar(ui);
        self.status_bar(ui);
        bimedit::new_dialog(self, &ctx);
        bimplace::dialog(self, &ctx);
        if self.editor.is_some() {
            self.command_line(ui);
        }
        if self.doc.is_some() && self.show_layers {
            self.layers_panel(ui);
        }
        if self.editor.is_some() && self.cadprops.open {
            cadprops::panel(self, ui);
        }
        plotdlg::window(self, &ctx);
        qsdlg::window(self, &ctx);
        self.tabs(ui);
        if self.part.is_some() {
            partpanel::ui(self, ui);
        } else if self.detail.is_some() {
            detailview::ui(self, ui);
        } else if let Some(pdf) = self.pdf.as_mut() {
            let t = self.tokens;
            let frame = egui::Frame::new().fill(t.panel).inner_margin(6);
            egui::Panel::left("pdf_sheets").default_size(180.0).size_range(120.0..=320.0).resizable(true).frame(frame).show(ui, |ui| {
                ui.label(RichText::new("Sheets").strong().color(t.text_strong));
                ui.separator();
                pdf.sheets_and_spaces(ui, &t);
            });
            if pdf.mode != "takeoff" {
                egui::Panel::right("mc_props").default_size(260.0).size_range(200.0..=420.0).resizable(true).frame(frame).show(ui, |ui| {
                    pdf.right_panel(ui, &t);
                });
            }
            if pdf.mode == "takeoff" {
                egui::Panel::right("tk_side").default_size(260.0).size_range(200.0..=420.0).resizable(true).frame(frame).show(ui, |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| pdf.takeoff_panel(ui, &t));
                });
            }
            let mut save = false;
            egui::Panel::top("pdf_tools").frame(egui::Frame::new().fill(t.panel).inner_margin(4)).show(ui, |ui| {
                save = pdf.tool_bar(ui, &t);
            });
            egui::CentralPanel::no_frame().show(ui, |ui| {
                let rect = ui.max_rect();
                pdf.show(ui, rect, &t);
            });
            pdf.take_space();
            pdf.dialogs(ui.ctx(), &t);
            if let Some(what) = pdf.want_pick.take() {
                // a file for an action: Insert pages from a PDF, a picture for the stamp, a photo, a tool chest
                match self.services.pick_open.as_mut().and_then(|f| f()) {
                    Some(Ok(f)) => {
                        let r = match what {
                            "insert-pdf" => pdf.pages_op("insert-pdf", "", Some((&f.name, &f.bytes))),
                            "picture" => {
                                pdf.set_picture(&f.name, &f.bytes);
                                Ok(())
                            }
                            "photo" => {
                                let caption = pdf.tools.selected().and_then(|id| pdf.reply_drafts.get(id)).cloned().unwrap_or_default();
                                pdf.photo_reply(&f.name, &f.bytes, &caption)
                            }
                            "chest-import" => {
                                pdf.import_chest(&f.name, &f.bytes);
                                Ok(())
                            }
                            "bcf-import" => {
                                pdf.import_bcf(&f.name, &f.bytes);
                                Ok(())
                            }
                            _ => Ok(()),
                        };
                        if let Err(e) = r {
                            pdf.tools.message = e;
                        }
                    }
                    Some(Err(e)) => pdf.tools.message = e,
                    None => {}
                }
            }
            if pdf.tools.prefs_dirty {
                pdf.tools.prefs_dirty = false;
                self.mc_prefs = pdf.tools.prefs_json();
            }
            for (name, bytes) in std::mem::take(&mut pdf.outbox) {
                match self.services.save.as_mut().map(|f| f(&name, bytes)) {
                    Some(Ok(_)) => {}
                    Some(Err(e)) => self.error = Some(e),
                    None => self.error = Some("Saving is not available here".into()),
                }
            }
            self.message = pdf.status();
            let said = if pdf.mode == "takeoff" { &pdf.takeoff.message } else { &pdf.tools.message };
            if !said.is_empty() {
                self.message = format!("{}   · {}", self.message, said);
            }
            let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64);
            if let Some(kind) = pdf.takeoff.export.take() {
                match pdf.takeoff_exports(kind, now) {
                    Ok(outs) => {
                        for (name, bytes) in outs {
                            match self.services.save.as_mut().map(|f| f(&name, bytes)) {
                                Some(Ok(Some(to))) => self.message = format!("Saved {to}"),
                                Some(Ok(None)) => {}
                                Some(Err(e)) => self.error = Some(e),
                                None => self.error = Some("Saving is not available here".into()),
                            }
                        }
                    }
                    Err(e) => pdf.takeoff.message = e,
                }
            }
            if save {
                match pdf.save_files(now) {
                    Ok(outs) => {
                        for (name, bytes) in outs {
                            match self.services.save.as_mut().map(|f| f(&name, bytes)) {
                                Some(Ok(Some(to))) => self.message = format!("Saved {to}"),
                                Some(Ok(None)) => {}
                                Some(Err(e)) => self.error = Some(e),
                                None => self.error = Some("Saving is not available here".into()),
                            }
                        }
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        } else if self.mc.is_some() {
            self.tool_palette(ui);
            let t = self.tokens;
            if let Some(mc) = self.mc.as_mut() {
                let frame = egui::Frame::new().fill(t.panel).inner_margin(6);
                egui::Panel::right("mc_side").default_size(260.0).size_range(200.0..=420.0).resizable(true).frame(frame).show(ui, |ui| {
                    mc.side_panel(ui, &t);
                });
                if let Err(e) = self.flush_model_save() {
                    self.error = Some(e);
                }
                let Some(mc) = self.mc.as_mut() else { return };
                mc.set_dark(self.preset == Preset::Dark);
                egui::CentralPanel::no_frame().show(ui, |ui| {
                    let rect = ui.max_rect();
                    mc.show(ui, rect, &t);
                });
                self.message = mc.status();
            }
        } else if self.scene3.is_some() && self.ifc3.is_some() {
            bimedit::ifc_branch(self, ui);
        } else if self.scene3.is_some() {
            self.model_panel(ui);
            self.props_panel(ui);
            let mut clicked: Option<Option<(f64, f64)>> = None;
            if let Some((scene, view)) = self.scene3.as_mut() {
                egui::CentralPanel::no_frame().show(ui, |ui| {
                    let rect = ui.max_rect();
                    let resp = view3d::show(ui, rect, scene, view);
                    if resp.clicked_by(egui::PointerButton::Primary) {
                        let ppp = f64::from(ui.ctx().pixels_per_point());
                        clicked = Some(
                            resp.interact_pointer_pos().map(|p| (f64::from(p.x - rect.min.x) * ppp, f64::from(p.y - rect.min.y) * ppp)),
                        );
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        clicked = Some(None);
                    }
                });
            }
            if let Some(at) = clicked {
                self.select3(at);
                // the Properties panel was laid out before the click in this frame: draw it in the next one
                ui.ctx().request_repaint();
            }
        } else if self.civil.is_some() {
            civilview::ui(self, ui);
        } else if self.doc.is_none() && self.editor.is_none() {
            hub::ui(self, ui);
        } else {
            egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));
        }
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
                    if ui
                        .button(RichText::new("CadCraft").strong().color(t.text_strong))
                        .on_hover_text(hub::t(&self.hub.locale, "window.hub.title"))
                        .clicked()
                    {
                        self.run_ui("hub.home", &Params::default());
                    }
                    ui.label(RichText::new("Rust test version").small().color(t.text_dim));
                    ui.separator();
                    if ui.button("Open…").on_hover_text("Open a DWG or DXF (Ctrl+O)").clicked() {
                        self.run_ui("file.open", &Params::default());
                    }
                    if ui.button("Sample").on_hover_text("Open the built-in sample drawing").clicked() {
                        self.run_ui("file.sample", &Params::default());
                    }
                    if ui.button("3D sample").on_hover_text("Open a sample 3D model (ModelCraft's model kernel)").clicked() {
                        self.run_ui("view.model.sample", &Params::default());
                    }
                    if ui.button("Site sample").on_hover_text("Open CivilCraft's sample site (survey points, surface, contours)").clicked()
                    {
                        self.run_ui("civil.sample", &Params::default());
                    }
                    if ui.button("Part sample").on_hover_text("Open PartCraft's sample part (a bracket with parameters)").clicked() {
                        self.run_ui("part.sample", &Params::default());
                    }
                    if ui
                        .button("Steel sample")
                        .on_hover_text("Open DetailCraft's sample frame (members, connections, footings, rebar)")
                        .clicked()
                    {
                        self.run_ui("detail.sample", &Params::default());
                    }
                    if ui.button(bimtools::bt("new.button", &[])).on_hover_text(bimtools::bt("new.title", &[])).clicked() {
                        self.bimh.new_dlg = Some(bimedit::NewDlg::default());
                    }
                    if ui.button("New").on_hover_text("A new drawing to draft in (Ctrl+N)").clicked() {
                        self.run_ui("file.new", &Params::default());
                    }
                    if self.editor.is_none()
                        && ui
                            .add_enabled(self.source.is_some(), egui::Button::new("Edit"))
                            .on_hover_text("Edit this drawing: draft, then save it as DWG or DXF")
                            .clicked()
                    {
                        self.run_ui("file.edit", &Params::default());
                    }
                    if self.mc.is_some() {
                        let mut pick: Option<&str> = None;
                        ui.menu_button("Export", |ui| {
                            for (f, label, _) in cc_m3d_files::EXPORT_FORMATS {
                                if ui.button(*label).clicked() {
                                    pick = Some(f);
                                    ui.close();
                                }
                            }
                        })
                        .response
                        .on_hover_text("Save the model as a ModelCraft file, glTF, OBJ, STL, Collada, IFC, DXF or DWG");
                        if let Some(f) = pick {
                            self.run_ui("file.export", &Params { name: Some(f.into()), ..Default::default() });
                        }
                    }
                    if self.editor.is_some() {
                        let mut pick: Option<&str> = None;
                        ui.menu_button("Save as", |ui| {
                            for f in cc_io_cad::SAVE_FORMATS {
                                let label = match *f {
                                    "dxf2000" => "DXF 2000".to_string(),
                                    "dxf12" => "DXF R12".to_string(),
                                    v => format!("DWG {v}"),
                                };
                                if ui.button(label).clicked() {
                                    pick = Some(f);
                                    ui.close();
                                }
                            }
                        })
                        .response
                        .on_hover_text("Save the drawing as DWG (2000–2018) or DXF (Ctrl+S: the last format)");
                        if let Some(f) = pick {
                            self.run_ui("file.save", &Params { name: Some(f.to_string()), ..Default::default() });
                        }
                    }
                    if ui
                        .add_enabled(
                            self.doc.is_some()
                                || self.scene3.is_some()
                                || self.mc.is_some()
                                || self.pdf.is_some()
                                || self.civil.is_some()
                                || self.part.is_some(),
                            egui::Button::new("Fit"),
                        )
                        .on_hover_text("Zoom to the drawing or model (Home)")
                        .clicked()
                    {
                        self.run_ui("view.fit", &Params::default());
                    }
                    if self.scene3.is_some() || self.mc.is_some() {
                        ui.separator();
                        for (name, label) in
                            [("iso", "Iso"), ("top", "Top"), ("front", "Front"), ("right", "Right"), ("back", "Back"), ("left", "Left")]
                        {
                            if ui.button(label).on_hover_text(format!("{label} view")).clicked() {
                                self.run_ui("view.standard", &Params { name: Some(name.into()), ..Default::default() });
                            }
                        }
                        let persp = self
                            .scene3
                            .as_ref()
                            .map_or_else(|| self.mc.as_ref().is_some_and(|m| m.tools.host.cam.persp), |(_, v)| v.cam.persp);
                        if ui.button(if persp { "Parallel" } else { "Perspective" }).on_hover_text("Switch the projection").clicked() {
                            self.run_ui("view.projection", &Params::default());
                        }
                        let st = self.mc.as_ref().map(|m| m.style).or_else(|| self.scene3.as_ref().map(|(_, v)| v.style));
                        if let Some(st) = st {
                            let cur = if !st.faces {
                                "Wireframe"
                            } else if st.xray {
                                "X-ray"
                            } else if st.hidden_line {
                                "Hidden line"
                            } else {
                                "Shaded"
                            };
                            let mut pick: Option<&str> = None;
                            egui::ComboBox::new("style3", "Style").selected_text(cur).width(96.0).show_ui(ui, |ui| {
                                for (id, label) in
                                    [("shaded", "Shaded"), ("hidden", "Hidden line"), ("wire", "Wireframe"), ("xray", "X-ray")]
                                {
                                    if ui.selectable_label(cur == label, label).clicked() {
                                        pick = Some(id);
                                    }
                                }
                            });
                            let mut edges = st.edges;
                            if ui.checkbox(&mut edges, "Edges").changed() {
                                pick = Some("edges");
                            }
                            if let Some(p) = pick {
                                self.run_ui("view.style", &Params { name: Some(p.into()), ..Default::default() });
                            }
                        }
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
        let editing = self.editor.is_some();
        if names.len() <= 1 && !editing {
            return;
        }
        let mut pick = None;
        let mut add = false;
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
                        // cadcraft.js's "+" tab: a new sheet (LAYOUT)
                        if editing && ui.small_button("+").on_hover_text(crate::edit::lang_text("ui.new_sheet_layout_title", &[])).clicked()
                        {
                            add = true;
                        }
                    });
                });
            });
        if let Some(i) = pick {
            self.run_ui("space.select", &Params { index: Some(i), ..Default::default() });
        }
        if add {
            self.run_ui("cmd.line", &Params { name: Some("LAYOUT".into()), ..Default::default() });
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
        let pictures = self.picture_textures(ui.ctx());
        let Some(doc) = &self.doc else { return };
        let frame = canvas::Frame {
            pictures: &pictures,
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
            let (x0, y0) = view.to_world(0.0, 0.0);
            let (x1, y1) = view.to_world(view.width, view.height);
            h.view_box = Some(cc_cad_geom::geom::Bounds { min_x: x0.min(x1), min_y: y0.min(y1), max_x: x0.max(x1), max_y: y0.max(y1) });
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
        let draw = |ents: &[&cc_cad::Entity], stroke: egui::Stroke| {
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
        // borrowed, not copied: SELECT ALL on a big drawing drew a copy of every object each frame
        let sel_ids = ed.hs.borrow().selection.clone();
        let sel: Vec<&cc_cad::Entity> = sel_ids.iter().filter_map(|id| doc.get(*id)).collect();
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
        {
            let hs = ed.hs.borrow();
            if !hs.preview.is_empty() {
                let preview: Vec<&cc_cad::Entity> = hs.preview.iter().collect();
                draw(&preview, egui::Stroke::new(1.0, t.ink));
            }
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
        let mut cmd_id: Option<egui::Id> = None;
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
                    // the field keeps the focus on Enter (no return key: Enter is read above). A text field lets go
                    // of the focus on Enter, and on the web the keys typed in the next frame were lost with it
                    let id = ui.make_persistent_id("cmdline.input");
                    cmd_id = Some(id);
                    if ctx.memory(|m| m.focused()).is_none_or(|f| f == id) {
                        ctx.memory_mut(|m| m.request_focus(id));
                    }
                    let te = egui::TextEdit::singleline(&mut ed.line)
                        .id(id)
                        .return_key(None)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .frame(egui::Frame::NONE);
                    let r = ui.add(te).labelled_by(lab.id);
                    if enter && r.has_focus() {
                        submit = true;
                    }
                });
            });
        // Space sends the line too, except while a command asks for text
        if ed.line.ends_with(' ') && ed.eng.expects() != Some(cc_engine::prompt::Kind::Text) {
            ed.line.pop();
            submit = true;
        }
        // Ctrl+C / X / V, Ctrl+Shift+V (taken out of the input in raw_input_hook while the line is empty)
        if let Some(cmd) = self.clip_cmd.take()
            && ed.line.is_empty()
            && !ed.eng.busy()
        {
            ed.type_line(cmd);
        }
        let elsewhere0 = ctx.memory(|m| m.focused()).is_some_and(|f| Some(f) != cmd_id);
        self.clip_keys = ed.line.is_empty()
            && !ed.eng.busy()
            && !elsewhere0
            && self.caddlg.qs.is_none()
            && self.caddlg.find.is_none()
            && !self.plot.open;
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
        if ctx.input(|i| i.key_pressed(egui::Key::F10)) {
            self.run_ui("edit.polar", &Params::default());
            return;
        }
        let Some(ed) = self.editor.as_mut() else { return };
        if submit {
            ed.submit();
        }
        // keys typed into another field (the Properties panel, the Plot dialog) stay there
        let elsewhere = ctx.memory(|m| m.focused()).is_some_and(|f| Some(f) != cmd_id);
        if esc && !elsewhere {
            ed.escape();
            ed.line.clear();
        }
        if del && ed.line.is_empty() && !elsewhere {
            ed.delete();
        }
        if undo && !elsewhere {
            ed.undo();
        }
        if redo && !elsewhere {
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

/// Hand the picked files to the command waiting for them (XATTACH / XREF Reload read the drawing as the page does).
fn deliver_pick(ed: &mut edit::Editor, req: edit::PickReq, files: Vec<ImportedFile>) {
    match req {
        edit::PickReq::Drawing => {
            let got = files.into_iter().next().and_then(|f| {
                let ext = f.name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
                if ext != "dwg" && ext != "dxf" {
                    ed.hs.borrow_mut().push_log(&edit::lang_text("app.insertf.not_dwg", &[("file", f.name.clone())]));
                    return None;
                }
                match cc_io_cad::import_bytes(&f.name, &f.bytes) {
                    Ok(drawing) => Some(cc_engine::PickedDrawing::from_value(&f.name, &drawing)),
                    Err(e) => {
                        ed.hs.borrow_mut().push_log(&edit::lang_text("app.insertf.read_failed", &[("file", f.name.clone()), ("why", e.0)]));
                        None
                    }
                }
            });
            ed.eng.deliver_drawing(got);
        }
        edit::PickReq::Pictures { .. } => {
            ed.eng.deliver_files(files.into_iter().map(|f| cc_engine::PickedFile { name: f.name, bytes: f.bytes }).collect());
        }
    }
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

    #[test]
    fn one_enter_per_frame() {
        let mut app = UiApp::default();
        app.run("file.new", &Params::default()).unwrap();
        let text = |s: &str| egui::Event::Text(s.into());
        let enter =
            egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        let mut raw =
            egui::RawInput { events: vec![text("LINE"), enter.clone(), text("0,0"), enter.clone(), text("5")], ..Default::default() };
        app.raw_input_hook(&mut raw);
        assert_eq!(raw.events, vec![text("LINE"), enter.clone()]);
        let mut raw = egui::RawInput { events: vec![text("6")], ..Default::default() };
        app.raw_input_hook(&mut raw);
        assert_eq!(raw.events, vec![text("0,0"), enter.clone()], "held events come first, still one Enter");
        let mut raw = egui::RawInput::default();
        app.raw_input_hook(&mut raw);
        assert_eq!(raw.events, vec![text("5"), text("6")]);
    }

    #[test]
    fn edit_the_sample_then_save_every_format() {
        type Saved = Arc<Mutex<Vec<(String, Vec<u8>)>>>;
        let saved: Saved = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&saved);
        let services = Services {
            save: Some(Box::new(move |name, bytes| {
                sink.lock().unwrap().push((name.to_string(), bytes));
                Ok(Some(name.to_string()))
            })),
            ..Default::default()
        };
        let mut app = UiApp::new(services);
        assert!(app.run("file.edit", &Params::default()).is_err(), "nothing open yet");
        assert!(app.run("file.save", &Params::default()).is_err(), "nothing open for editing");
        app.run("file.sample", &Params::default()).unwrap();
        app.run("file.edit", &Params::default()).unwrap();
        let before = app.editor.as_ref().unwrap().eng.doc().every_entity().count();
        assert!(before > 100, "the sample imported: {before} objects");
        for t in ["LINE", "0,0", "100,100", ""] {
            app.run("cmd.line", &Params { name: Some(t.into()), ..Default::default() }).unwrap();
        }
        assert_eq!(app.editor.as_ref().unwrap().eng.doc().every_entity().count(), before + 1);
        assert!(app.editor.as_ref().unwrap().eng.doc().dirty());
        assert!(app.run("file.save", &Params { name: Some("dxf9".into()), ..Default::default() }).is_err());
        for f in cc_io_cad::SAVE_FORMATS {
            app.run("file.save", &Params { name: Some((*f).to_string()), ..Default::default() }).unwrap();
        }
        assert!(!app.editor.as_ref().unwrap().eng.doc().dirty(), "saved");
        assert_eq!(app.save_format, "dxf12");
        let saved = saved.lock().unwrap();
        assert_eq!(saved.len(), cc_io_cad::SAVE_FORMATS.len());
        for (name, bytes) in saved.iter() {
            // what was saved opens again with the new line in it
            let input = cc_io_cad::import_bytes(name, bytes).unwrap();
            let mut d = cc_cad::Doc::new();
            d.load(&input);
            let n = d.every_entity().count();
            // DWG: every object back; DXF R12 / 2000 write some objects as several (as the 0.5 writers do)
            assert!(if name.ends_with(".dwg") { n == before + 1 } else { n > before }, "{name}: {n} objects");
            let has_line = d.snapshot()["model"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["type"] == "line" && e["x1"] == 0.0 && e["y1"] == 0.0 && e["x2"] == 100.0 && e["y2"] == 100.0);
            assert!(has_line, "{name}: the new line");
        }
        assert!(saved.iter().any(|(n, _)| n == "kitchen-equipment.dwg") && saved.iter().any(|(n, _)| n == "kitchen-equipment.dxf"));
    }
}
