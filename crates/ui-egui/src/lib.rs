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
pub mod bimclip;
pub mod bimedit;
pub mod bimequip;
pub mod bimgrips;
pub mod bimlib;
pub mod bimlibnet;
pub mod bimlive;
pub mod bimplace;
pub mod bimplan;
pub mod bimres;
pub mod bimsheets;
pub mod bimtools;
pub mod cad3dtab;
pub mod cadlive;
pub mod cadprops;
pub mod cadset;
pub mod gizmo3;
pub mod inputdlg;
pub mod keycode;
pub mod partgizmo;
pub use cc_engine::inputprof;
pub mod canvas;
pub mod civil3d;
pub mod civilcad;
pub mod civilcrs;
pub mod civildialogs;
pub mod civildrain;
pub mod civildrape;
pub mod civildrive;
pub mod civilexport;
pub mod civilgis;
pub mod civilgis3;
pub mod civilgisx;
pub mod civilimport;
pub mod civillasso;
pub mod civillive;
pub mod civilmaps;
pub mod civilmore;
pub mod civilpanel;
pub mod civilpics;
pub mod civilplan;
pub mod civilpoints;
pub mod civilprofile;
pub mod civilpropedit;
pub mod civilprops;
pub mod civilsections;
pub mod civilsheets;
pub mod civilsuper;
pub mod civilterrain;
pub mod civiltools;
pub mod civilview;
pub mod cloudview;
pub mod detail3d;
pub mod detailcorner;
pub mod detaildrag;
pub mod detaildwg;
pub mod detailexport;
pub mod detaillive;
pub mod detailpanel;
pub mod detailprops;
pub mod detailtools;
pub mod detailview;
pub mod document;
pub mod dwg3d;
pub mod dwgequip;
pub mod edit;
pub mod help;
mod help_files;
pub mod hub;
pub mod ifc3d;
pub mod lang;
pub mod libdlg;
pub mod lispdlg;
pub mod live;
pub mod liveapps;
pub mod livestate;
pub mod markupbcf;
pub mod markupchest;
pub mod markupcompare;
pub mod markupedit;
pub mod markupfind;
pub mod markuplive;
pub mod markupplus;
pub mod markupprops;
pub mod markupslip;
pub mod markuptools;
pub mod mcact;
pub mod mcedit;
mod mckeys;
pub mod mclights;
mod mcmarks;
pub mod mcmats;
pub mod mcpanels;
pub mod mcprint;
mod mcrecord;
mod mcrender;
pub mod mcset;
pub mod mcsolids;
mod mcstarter;
mod mcstretch;
mod mcterrain;
pub mod model3d;
pub mod modelcraft;
pub mod modellive;
pub mod mytoolsdlg;
pub mod panes;
pub mod paneview;
pub mod parpanel;
pub mod partanalysis;
pub mod partasm;
pub mod partasmui;
pub mod partdesign;
pub mod partdraw;
pub mod partedit;
pub mod partfeat;
pub mod partfiles;
pub mod partlive;
pub mod partmeasure;
pub mod partpanel;
pub mod partsketch;
pub mod partsketchui;
pub mod partview;
pub mod pdfcmd;
pub mod pdfview;
pub mod picmesh;
pub mod plotdlg;
pub mod qr;
pub mod qsdlg;
pub mod recover;
pub mod recoverdlg;
pub mod ribbon;
pub mod settings;
pub mod sheetsetdlg;
pub mod shell;
pub mod takeoff3d;
pub mod takeoffedit;
pub mod takeofflive;
pub mod takeoffnotes;
pub mod takeoffprops;
pub mod takeoffrev;
pub mod takeoffsnap;
pub mod takeofftools;
pub mod theme;
pub mod tkbench;
pub mod view3d;
pub mod viewer;
pub mod viewshot;
pub mod winsetdlg;

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

/// Small text values every window of this user shares, as 0.5 keeps them in the browser's localStorage (DetailCraft's
/// clipboard `detailcraft.clipboard`, so another window pastes it): desktop, a folder in the config dir
/// (`cc_recovery::dir::DirKv`); web, localStorage. Only `Get`, `Put` and `Delete` are asked of it. Every value read
/// back is hostile (another window, another version, a person's edits).
#[derive(Clone)]
pub struct SharedStore(pub std::rc::Rc<std::cell::RefCell<cc_recovery::Service>>);

/// localStorage's quota (5 MB): a value bigger than this is refused, as `setItem` throws there.
pub const SHARED_STORE_MAX: usize = 5 << 20;

impl SharedStore {
    pub fn new(service: cc_recovery::Service) -> SharedStore {
        SharedStore(std::rc::Rc::new(std::cell::RefCell::new(service)))
    }
    /// localStorage `getItem(key)`: None when missing, unreadable or too big.
    pub fn get(&self, key: &str) -> Option<String> {
        let mut s = self.0.try_borrow_mut().ok()?;
        let bytes = (*s)(cc_recovery::RecoveryOp::Get(key.to_string())).ok().flatten()?;
        if bytes.len() > SHARED_STORE_MAX {
            return None;
        }
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }
    /// localStorage `setItem(key, value)`; an error when it is refused (too big, the store failed).
    pub fn set(&self, key: &str, value: &str) -> Result<(), String> {
        if value.len() > SHARED_STORE_MAX {
            return Err("quota: too big for the shared storage".into());
        }
        let mut s = self.0.try_borrow_mut().map_err(|_| "the shared storage is busy".to_string())?;
        (*s)(cc_recovery::RecoveryOp::Put(key.to_string(), value.as_bytes().to_vec())).map(|_| ())
    }
    /// localStorage `removeItem(key)`.
    pub fn remove(&self, key: &str) {
        if let Ok(mut s) = self.0.try_borrow_mut() {
            let _ = (*s)(cc_recovery::RecoveryOp::Delete(key.to_string()));
        }
    }
}

impl std::fmt::Debug for SharedStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SharedStore")
    }
}

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
    /// the Plot dialog's "Open to print": show a PDF (name, bytes) where it can be printed → Ok(true) shown (desktop:
    /// the system's PDF viewer; web: a new tab), Ok(false) saved / downloaded instead (a blocked tab)
    pub open_pdf: Option<OpenPdf>,
    /// desktop: pick a folder (the Sheet Set panel's, the drawing viewer's project folder) → its path; None = cancelled
    pub pick_folder: Option<Box<dyn FnMut() -> Option<String>>>,
    /// desktop: a folder's entries (name, is a folder) — the drawing viewer walks the project folder for linked drawings
    /// and pictures (folder-pick.js); None = this host cannot list folders (web)
    pub list_dir: Option<ListDir>,
    /// web: open the browser's folder picker (the directory picker, or `<input webkitdirectory>`); the folder arrives
    /// in `folder_picked`: Ok(Some) its files, Ok(None) cancelled, Err why it could not be read
    pub pick_folder_async: Option<Box<dyn FnMut()>>,
    pub folder_picked: Option<FolderSlot>,
    /// where recovery copies of unsaved work go (desktop: a locked folder in the config dir; web: browser storage);
    /// None = the window cannot keep them, and says so (recover.rs)
    pub recovery: Option<cc_recovery::Service>,
    /// CivilCraft's map layers: ask a map server for a URL (no credentials, no referrer); the answer arrives in
    /// `fetched`. None = this host cannot fetch.
    pub fetch: Option<Fetch>,
    pub fetched: Option<Arc<Mutex<Vec<Fetched>>>>,
    /// values shared with this user's other windows (see [`SharedStore`]); None = this window keeps them to itself
    pub store: Option<SharedStore>,
}

/// A folder the browser picked (see `Services::pick_folder_async`): its name, every file in it ("a/b.dwg" paths) and,
/// when the folder can be written (the directory picker), how to write a file back.
pub struct PickedFolder {
    pub name: String,
    pub files: Vec<(String, Vec<u8>)>,
    pub write: Option<FolderWrite>,
}
/// Write a file into a picked folder (path, bytes).
pub type FolderWrite = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;
/// Where a picked folder lands.
pub type FolderSlot = std::rc::Rc<std::cell::RefCell<Option<Result<Option<PickedFolder>, String>>>>;

/// Ask a map server for a URL (see `Services::fetch`).
pub type Fetch = Box<dyn FnMut(&str)>;

/// A fetched URL: Ok((content type, bytes)) or why not.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub url: String,
    pub result: Result<(String, Vec<u8>), String>,
}

/// Show a PDF to print (see `Services::open_pdf`).
pub type OpenPdf = Box<dyn FnMut(&str, &[u8]) -> Result<bool, String>>;

pub use cc_recovery::RecoveryOp;

/// A folder's entries: path → (name, is a folder) in any order (see `Services::list_dir`).
pub type ListDir = Box<dyn FnMut(&str) -> Result<Vec<(String, bool)>, String>>;

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
    ("edit.grid", "Grid on / off", "F7", "{} → the drafting grid (cadcraft.js toggle('grid'); the status bar's Grid)"),
    (
        "edit.snapkind",
        "Object snap kind on / off",
        "",
        "{name: end|mid|cen|quad|int|perp|tan|near|node|ins} → that kind of object snap (the status bar's Object snaps)",
    ),
    ("layer.current", "Make a layer current", "", "{name: layer} → the layer current, turned on and thawed (cadcraft.js setCurrent)"),
    (
        "layer.flag",
        "Layer flag on / off",
        "",
        "{name: \"<layer>=on|frozen|locked|plot\"} → that flag of the layer turned over (renderLayers' flags)",
    ),
    ("layer.new", "New layer", "", "{name: layer} → a new layer (colour 7), made current (the Layers panel's New layer)"),
    ("edit.polar", "Polar tracking on / off", "F10", "{} → points line up on multiples of the POLARANG step"),
    ("edit.lwt", "Line weights on / off", "", "{} → lines drawn as wide as their weight (LWDISPLAY; the status bar's LWT)"),
    ("file.edit", "Edit this drawing", "", "{} → imports the open DWG / DXF into an editable drawing (CadCraft's importer)"),
    (
        "file.save",
        "Save as",
        "Ctrl+S",
        "{name?: \"2018\"|\"2013\"|\"2010\"|\"2007\"|\"2004\"|\"2000\"|\"dxf2000\"|\"dxf12\"} → saves the drawing open for editing (default: the last format, else 2018)",
    ),
    (
        "input.style",
        "Menus, keys and mouse",
        "",
        "{name: \"cadcraft\"|\"drafting\"|\"ribbon\"} → the drafting editor's toolbar or ribbon (CadCraft, Drafting classic, Ribbon)",
    ),
    ("input.tab", "Ribbon tab", "", "{index: integer} → the ribbon's open tab"),
    (
        "input.run",
        "Click a toolbar or ribbon button",
        "",
        "{name: command id (\"draw.line\", \"view.extents\" …) or a toolbar button's command (\"ZOOM E\")} → runs what the button runs",
    ),
    (
        "input.menu",
        "Open a toolbar or ribbon menu",
        "",
        "{name?: \"file\"|\"saveas\"|\"more:<Group>\"|\"menu:Edit\"|\"menu:Parametric\"} → no name closes it",
    ),
    ("input.saveas", "Save as (format)", "", "{name: a save format, as file.save} → the ribbon's Save as choice"),
    (
        "app.tz",
        "Time zone of exports",
        "",
        "{factor?: number, minutes east of UTC (±1560)} → the zone the local-time fields of exports and lists are written in (zip dates, LandXML date / time, KISS header, XLSX / BCF zips, MarkupCraft's list dates), fixed; no factor = the machine's zone (the default)",
    ),
];

/// modelcraft.js importFile's message for an import (`app.import.skp.done`, `app.import.dae`, `app.import.site` …):
/// what came in, what was converted, what was left out. Spec gap: 0.5's DWG note on dynamic blocks is not added.
fn import_said(fmt: &str, file: &str, x: &cc_io_m3d::obj::Imported, model: &cc_m3d::model::Model) -> String {
    use crate::mcact::ns;
    use crate::modelcraft::mt;
    let rep = &x.report;
    let num = |k: &str| rep.get(k).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
    let s = |v: f64| ns(v);
    let def = model.def(&x.def);
    let (faces, edges) = def.map_or((0.0, 0.0), |d| (d.faces.len() as f64, d.edges.len() as f64));
    let skipped = {
        let list: Vec<String> = rep
            .get("skipped")
            .and_then(serde_json::Value::as_object)
            .map(|m| {
                m.iter()
                    .filter(|(_, n)| n.as_f64().is_some_and(|n| n != 0.0))
                    .map(|(k, n)| format!("{} {k}", ns(n.as_f64().unwrap_or(0.0))))
                    .collect()
            })
            .unwrap_or_default();
        if list.is_empty() { String::new() } else { format!(" {}", mt("app.import.skipped", &[("list", &list.join(", "))])) }
    };
    let mut out = match fmt {
        "skp" | "skpjson" => {
            let version = rep.get("version").and_then(serde_json::Value::as_str).unwrap_or("");
            let mut m = mt(
                "app.import.skp.done",
                &[("file", file), ("version", version), ("faces", &s(num("faces"))), ("n", &s(num("components")))],
            );
            for (k, key) in [("tags", "app.import.skp.tags"), ("hidden", "app.import.skp.hidden"), ("untagged", "app.import.skp.untagged")]
            {
                if num(k) != 0.0 {
                    m.push(' ');
                    m.push_str(&mt(key, &[("n", &s(num(k)))]));
                }
            }
            let kept = num("texturedKept");
            if kept != 0.0 {
                m.push(' ');
                m.push_str(&mt("app.import.skp.textures_kept", &[("n", &s(kept))]));
            }
            if num("textured") > kept {
                m.push(' ');
                m.push_str(&mt("app.import.skp.textures", &[("n", &s(num("textured") - kept))]));
            }
            m
        }
        "dae" => {
            let up = rep.get("up").and_then(serde_json::Value::as_str).unwrap_or("Z_UP");
            let meter = num("meter");
            let mut conv = Vec::new();
            if up != "Z_UP" {
                conv.push(mt("app.import.dae.up", &[("axis", if up == "Y_UP" { "Y" } else { "X" })]));
            }
            if meter != 1.0 {
                conv.push(mt("app.import.dae.unit", &[("m", &s(meter))]));
            }
            let mut m = if num("components") != 0.0 {
                mt("app.import.dae.comps", &[("file", file), ("faces", &s(num("faces"))), ("n", &s(num("components")))])
            } else {
                mt("app.import.dae", &[("file", file), ("n", &s(num("faces")))])
            };
            if !conv.is_empty() {
                m.push(' ');
                m.push_str(&mt("app.import.dae.converted", &[("how", &conv.join(", "))]));
            }
            m
        }
        "dxf" | "dwg" | "dwgjson" => {
            if edges == 0.0 {
                mt("app.import.site.empty", &[("file", file)])
            } else {
                let mut m = mt("app.import.site", &[("file", file), ("edges", &s(edges))]);
                if faces != 0.0 {
                    m.push(' ');
                    m.push_str(&mt("app.import.site.faces", &[("n", &s(faces))]));
                }
                if num("texts") != 0.0 {
                    m.push(' ');
                    m.push_str(&mt("app.import.site.texts", &[("n", &s(num("texts")))]));
                }
                m.push(' ');
                m.push_str(&mt("app.import.site.group", &[]));
                m
            }
        }
        _ => mt("app.import.dae", &[("file", file), ("n", &s(faces))]),
    };
    out.push_str(&skipped);
    out
}

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
    /// ModelCraft's pictures decoded on other threads, the model shown at once (the desktop window; S.11)
    pub bg_pictures: bool,
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
    /// the Studio Live session of this window (every app but DetailCraft, which keeps its own: liveapps.rs) and its window
    pub live: Option<live::LiveSession>,
    pub live_window: bool,
    /// a joiner's takeoff while the snapshot's PDFs are not open yet (takeofflive.rs)
    pub live_hold_tk: Option<cc_takeoff::model::Takeoff>,
    /// …and a joiner's markups (markuplive.rs)
    pub live_hold_mk: Option<cc_markup::model::Store>,
    /// BimCraft live (bimlive.rs): the file's own largest id before this window's range was claimed, and a counter of
    /// what others' lines changed
    pub live_bim_max: f64,
    pub live_bim_ver: u64,
    /// CadCraft live (cadlive.rs): the sheets' live ids
    pub live_cad_lids: cadlive::Lids,
    /// the zone local-time fields are written in (0.5 writes zip dates, LandXML date / time, the KISS header … with
    /// `getHours()`): the machine's, or a fixed offset (`app.tz`: scripts and tests)
    pub zone: cc_clock::Zone,
    /// what MarkupCraft keeps between runs (the tool chest, the picture and signature, tool settings): the desktop
    /// app reads and writes it as `markupcraft.json`
    pub mc_prefs: serde_json::Value,
    /// what CivilCraft keeps between runs (Settings › CivilCraft, the mass haul factor; 0.5: localStorage)
    pub cv_prefs: serde_json::Value,
    /// Settings › DetailCraft kept between runs (detailcraft.js `detailcraft.settings`; in the app's settings file)
    pub dc_prefs: serde_json::Value,
    /// TakeoffCraft's Snap and Snap to 45° (takeoffcraft.js `takeoffcraft.settings` snap / snap45: per viewer, kept
    /// between runs): (snap on, 45° without Shift)
    pub tk_snap: (bool, bool),
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
    /// Settings › CadCraft
    pub cadset: cadset::CadSet,
    /// Settings › ModelCraft
    pub mcset: mcset::McSet,
    /// the drafting editor's 3D tab (M4.25)
    pub tab3d: cad3dtab::Tab3d,
    /// the drafting editor's Plot dialog (M4.12)
    pub plot: plotdlg::PlotDlg,
    /// INSERT, File's "Insert from" dialog / a question with buttons
    pub choose: Option<libdlg::ChooseDlg>,
    /// the library panel (LIBRARY, INSERT Library)
    pub lib: libdlg::LibPanel,
    /// APPLOAD, the alias editor, DELBLOCKS' list (lispdlg.rs)
    pub lisp: lispdlg::LispUi,
    /// the Parameters panel and the constraints on the drawing (PARAMETERS, M4.23)
    pub par: parpanel::ParPanel,
    /// SHORTCUTS' keys editor and COMPARESETTINGS' dialog (M4.32)
    pub winset: winsetdlg::WinSet,
    /// the Sheet Set panel (M4.34)
    pub sheetset: sheetsetdlg::SheetSetPanel,
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
    /// PartCraft's clipboard (clipboard.js clip): kept from one part to the next
    pub part_clip: Option<serde_json::Value>,
    /// CadCraft Studio's start screen (hub.rs): recent files, language
    pub hub: hub::Hub,
    /// BimCraft's editing and Draw on the IFC view (bimedit.rs, bimplan.rs)
    pub bimh: bimedit::Host,
    /// the drafting editor's ribbon / toolbar and its style (ribbon.rs)
    pub ribbon: ribbon::Bar,
    /// recovery copies of the drawing being edited and the "Recover your work?" dialog (recover.rs)
    pub recovery: recover::Recovery,
    /// Settings: language, theme, panel text size (settings.rs)
    pub settings: settings::Settings,
    /// the drawing viewer's print view, Drawing panel, Find, added links and pictures (viewer.rs)
    pub viewer: viewer::Viewer,
    /// the help panel (help.rs: the 0.5 Markdown help)
    pub help: help::Help,
    /// the drafting editor's split screen (SPLIT / VPORTS in the model, LINKVIEWS: paneview.rs)
    pub panes: paneview::PaneUi,
    /// My tools: buttons of one's own, their keys and macros (mytoolsdlg.rs)
    pub mytools: mytoolsdlg::MyToolsUi,
    /// the 0.7 shell's view state (shell.rs: the drafting grid)
    pub look: shell::Look,
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
            bg_pictures: false,
            scene_seq: 0,
            ifc3: None,
            ifc_groups: Vec::new(),
            hidden3: HashSet::new(),
            iso3: None,
            bim: bimtools::BimTools::default(),
            ifc_name: String::new(),
            pdf: None,
            live: None,
            live_window: false,
            live_hold_tk: None,
            live_hold_mk: None,
            live_bim_max: 0.0,
            live_bim_ver: 0,
            live_cad_lids: cadlive::Lids::default(),
            zone: cc_clock::Zone::System,
            mc_prefs: serde_json::Value::Null,
            cv_prefs: serde_json::Value::Null,
            dc_prefs: serde_json::Value::Null,
            tk_snap: (true, false),
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
            cadset: cadset::CadSet::default(),
            mcset: mcset::McSet::default(),
            tab3d: cad3dtab::Tab3d::default(),
            plot: plotdlg::PlotDlg::default(),
            choose: None,
            lib: libdlg::LibPanel::default(),
            lisp: lispdlg::LispUi::default(),
            par: parpanel::ParPanel::default(),
            winset: winsetdlg::WinSet::default(),
            sheetset: sheetsetdlg::SheetSetPanel::default(),
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
            part_clip: None,
            hub: hub::Hub::default(),
            bimh: bimedit::Host::default(),
            ribbon: ribbon::Bar::default(),
            recovery: recover::Recovery::default(),
            settings: settings::Settings::default(),
            viewer: viewer::Viewer::default(),
            help: help::Help::default(),
            panes: paneview::PaneUi::default(),
            mytools: mytoolsdlg::MyToolsUi::default(),
            look: shell::Look::default(),
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
        // a CSV onto an open part: its design table (params-panel.js Import; the web picker's file arrives here)
        if let Some(pv) = self.part.as_mut().filter(|_| f.name.to_ascii_lowercase().ends_with(".csv")) {
            pv.import_table(&f.name, &f.bytes);
            return;
        }
        // a .lsp / .dcl dropped on the drawing being edited loads (appload.js); compiled files are refused with why
        if lispdlg::open_app_file(self, f) {
            return;
        }
        // CivilCraft open: elevation grids and LiDAR (gis-terrain-ui.js opener, first), RGB GeoTIFF pictures, DWG / DXF
        // linework go into the project
        if self.civil.is_some() && civilview::is_terrain_file(&f.name.to_ascii_lowercase()) {
            civilview::open_terrain(self, f);
            return;
        }
        // a point cloud goes under the drawing at its own coordinates (cadcraft.js openLocal, 0.5 SG.1)
        if cloudview::open_cloud(self, f) {
            return;
        }
        // a picture or a missing linked drawing dropped on the drawing shown: added to it
        if viewer::dropped(self, f) {
            return;
        }
        self.viewer.unreadable.clear();
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
        // a picture and its world file go to the open CivilCraft project's map layers (gis-map-ui.js opener)
        if self.civil.is_some() && civilview::is_map_file(&lower) {
            if let Some(cv) = self.civil.as_mut() {
                cv.maps_file(&f.name, f.bytes.clone());
            }
            return;
        }
        // GIS files (and their shapefile parts) go into the open CivilCraft project (gis-vector-ui.js openers)
        if self.civil.is_some() && civilview::is_gis_file(&lower, &f.bytes) {
            civilview::open_gis(self, f);
            return;
        }
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
                    v.zone = self.zone;
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
                    self.seed_tk_snap();
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
        // the drawing viewer: linked drawings and pictures from the files added by hand and the project folder
        viewer::open_drawing(self, f);
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
                // freeing the file's JSON tree takes a while (a 1 MB model: ~3 ms): on the desktop it goes on a thread
                free_aside(j);
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
                let _ = tris;
                self.message = lang::t(
                    lang::BIMCRAFT,
                    "ifc.opened",
                    &[
                        ("file", f.name.clone()),
                        ("count", viewer::num(v.products as u64)),
                        ("n", v.products.to_string()),
                        ("s", format!("{:.1}", read_ms / 1000.0)),
                    ],
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
            Ok(x) => {
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                // modelcraft.js importFile's words for what came in
                let said = import_said(fmt, &f.name, &x, &model);
                if into_open {
                    if let Some(mc) = self.mc.as_mut() {
                        mc.tools.host.set_model(model);
                        mc.message = said;
                    }
                } else {
                    let t1 = web_time::Instant::now();
                    self.show_model(model, &name);
                    self.open_split = format!("import {ms:.1} ms, view {:.1} ms", t1.elapsed().as_secs_f64() * 1000.0);
                    self.message = said;
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
    pub(crate) fn show_model(&mut self, model: cc_m3d::model::Model, name: &str) {
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
        // the key style, Snap and Collide are the viewer's settings, not the model's: they stay from the model open
        // before (Settings › ModelCraft)
        let set = self.mcset_now();
        self.mcset = set.clone();
        let pictures = if self.bg_pictures { model3d::ImgCache::background() } else { model3d::ImgCache::new() };
        match modelcraft::ModelView::with_pictures(model, name, self.preset.is_dark(), pictures) {
            Ok(mut mv) => {
                mv.apply_settings(&set);
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
    /// Whether a 3D instance is hidden now (a storey or class turned off, Find's "show only" / "hide").
    pub(crate) fn instance_hidden3(&self, i: usize) -> bool {
        let by_group = self
            .ifc_groups
            .get(i)
            .is_some_and(|(st, ty)| self.hidden3.contains(&format!("s:{st}")) || self.hidden3.contains(&format!("c:{ty}")));
        let by_find = match (&self.iso3, self.ifc3.as_ref().and_then(|(_, p)| p.get(i))) {
            (Some((only, set)), Some(p)) => set.contains(&p.to_bits()) != *only,
            _ => false,
        };
        let by_res = self
            .ifc3
            .as_ref()
            .and_then(|(_, p)| p.get(i))
            .is_some_and(|p| self.bimh.draw.res.hidden.contains(&cc_bim::draw::plan::key_u64(*p)));
        by_group || by_find || by_res
    }

    pub(crate) fn apply_hidden3(&mut self) {
        let groups = &self.ifc_groups;
        let hidden = &self.hidden3;
        let iso = &self.iso3;
        let res_hidden = &self.bimh.draw.res.hidden;
        let products = self.ifc3.as_ref().map(|(_, p)| p);
        if let Some((scene, _)) = self.scene3.as_mut() {
            scene.set_hidden(&|i| {
                let by_group =
                    groups.get(i).is_some_and(|(st, ty)| hidden.contains(&format!("s:{st}")) || hidden.contains(&format!("c:{ty}")));
                let by_find = match (iso, products.and_then(|p| p.get(i))) {
                    (Some((only, set)), Some(p)) => set.contains(&p.to_bits()) != *only,
                    _ => false,
                };
                // the residential framing's Show / See through (bimres.rs)
                let by_res = products.and_then(|p| p.get(i)).is_some_and(|p| res_hidden.contains(&cc_bim::draw::plan::key_u64(*p)));
                by_group || by_find || by_res
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

    /// ModelCraft's tools as 0.7's toolbar (ui.inc `$__tools`: Draw, Modify, Organize, Edit, Camera; the File group's
    /// actions are the app bar's): the tool in use pressed, the Solids and Edit › More menus under their buttons.
    fn tool_palette(&mut self, ui: &mut egui::Ui) {
        use shell::{Tool, ToolGroup};
        let t = self.tokens;
        let cur = self.mc.as_ref().map(|m| m.tools.name.clone()).unwrap_or_default();
        let menus = self.mc.as_ref().map(|m| m.menu_state()).unwrap_or_default();
        let blender = menus.blender;
        let mut pick: Option<&str> = None;
        let mut undo = false;
        let mut redo = false;
        let mut live = false;
        let mut edit: Option<String> = None;
        let mut fit = false;
        let mt = |k: &str| modelcraft::mt(k, &[]);
        let tool = |name: &'static str, icon: &'static str| {
            Tool::new(name, icon, mt(&format!("ui.tool.{name}"))).tip(modelcraft::tool_title(name, blender)).on(cur == name)
        };
        let act = |id: &'static str, icon: &'static str, key: &str| Tool::new(id, icon, mt(key)).tip(mt(&format!("{key}.title")));
        let groups = vec![
            ToolGroup {
                label: mt("ui.tb.group.draw"),
                tools: vec![
                    tool("line", "slash-lg"),
                    tool("rect", "square"),
                    tool("circle", "circle"),
                    tool("polygon", "hexagon"),
                    tool("arc", "bezier2"),
                    act("act:text3d", "type", "ui.tool.text3d"),
                ],
            },
            ToolGroup {
                label: mt("ui.tb.group.modify"),
                tools: vec![
                    tool("select", "cursor"),
                    tool("push", "box-arrow-up"),
                    tool("follow", "arrow-90deg-up"),
                    tool("paint", "brush"),
                    tool("offset", "bounding-box"),
                    tool("move", "arrows-move"),
                    tool("rotate", "arrow-repeat"),
                    tool("scale", "arrows-angle-expand"),
                    tool("erase", "eraser"),
                    tool("tape", "rulers"),
                    tool("protractor", "compass"),
                    tool("dim", "arrow-left-right"),
                ],
            },
            ToolGroup {
                label: mt("ui.tb.group.organize"),
                tools: vec![
                    act("act:make-component", "box", "ui.tool.component"),
                    act("act:make-group", "collection", "ui.tool.group"),
                    act("act:explode", "grid-3x3-gap", "ui.tool.explode"),
                    Tool::new("menu:solids", "union", mt("ui.solids")).tip(mt("ui.solids.title")),
                ],
            },
            ToolGroup {
                label: mt("ui.tb.group.edit"),
                tools: vec![
                    act("undo", "arrow-counterclockwise", "ui.tool.undo"),
                    act("redo", "arrow-clockwise", "ui.tool.redo"),
                    act("act:copy", "copy", "ui.tool.copy"),
                    act("act:paste", "clipboard-plus", "ui.tool.paste"),
                    Tool::new("menu:edit", "three-dots", mt("ui.editmenu")).tip(mt("ui.editmenu.title")),
                ],
            },
            ToolGroup {
                label: mt("ui.tb.group.camera"),
                tools: vec![
                    act("fit", "arrows-fullscreen", "ui.tool.extents"),
                    tool("section", "scissors"),
                    Tool::new("menu:views", "camera", mt("ui.views")).tip(mt("ui.views.title")),
                    Tool::new("act:storeys", "layers", mt("ui.export.storeys")),
                    Tool::new("act:render", "camera", mt("ui.export.render")),
                ],
            },
        ];
        let h = shell::toolbar_height(ui, &groups, ui.available_width());
        egui::Panel::top("mc_tools").exact_size(h).frame(egui::Frame::new().fill(t.app_bar)).show(ui, |ui| {
            let app_c = t.app("modelcraft");
            for (id, resp) in shell::toolbar(ui, &t, app_c, "mc-tb", &groups) {
                match id.as_str() {
                    "menu:solids" => {
                        egui::Popup::menu(&resp).show(|ui| mcedit::solids_menu_items(ui, &mut edit));
                    }
                    "menu:edit" => {
                        egui::Popup::menu(&resp).show(|ui| mcedit::edit_menu_items(ui, &mut edit, blender));
                    }
                    "menu:views" => {
                        egui::Popup::menu(&resp).show(|ui| mcedit::views_menu_items(ui, &mut edit, menus));
                    }
                    _ if !resp.clicked() => {}
                    "undo" => undo = true,
                    "redo" => redo = true,
                    "live" => live = true,
                    "fit" => fit = true,
                    a if a.starts_with("act:") => edit = Some(a.trim_start_matches("act:").to_string()),
                    name => pick = modelcraft::PALETTE.iter().map(|(n, _)| *n).find(|n| *n == name),
                }
            }
        });
        if fit {
            self.run_ui("view.fit", &Params::default());
        }
        if live {
            self.live_window = true;
        }
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
        // 0.7's dock: the grip, then the body inset
        let app_c = t.app(self.active_app());
        let frame = shell::panel_frame(&t);
        crate::shell::side(
            ui,
            egui::Panel::left("model3").default_size(256.0).size_range(160.0..=400.0).resizable(true).frame(frame),
            |ui| {
                shell::grip(ui, &t, app_c, &lang::cad("app.tab.model"), false, false);
                ui.add_space(8.0);
                let inner = ui.available_rect_before_wrap().shrink2(egui::vec2(12.0, 0.0));
                let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(inner));
                // the section head with its link (0.7 `.cc-sec-head`: the title, Show all at the right)
                shell::section_head(ui, &t, &lang::cad("app.tab.model"), |ui| {
                    if shell::link(ui, &t, "", &lang::cad("opt.showall")).clicked() {
                        toggle = Some("*".into());
                    }
                });
                ui.add_space(4.0);
                ui.checkbox(&mut cut.0, RichText::new(lang::mc("app.undo.sec_active")).color(t.text));
                if cut.0 {
                    ui.add(egui::Slider::new(&mut cut.1, 0.0..=1.0).show_value(false).text("height"));
                }
                shell::sec_break(ui, &t);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for (title, list, prefix) in [(lang::bim("ui.storeys"), &storeys, "s:"), (lang::bim("color.field.cls"), &classes, "c:")]
                    {
                        shell::section(ui, &t, &title, |ui| {
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
            },
        );
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
        if self.sel_props.is_empty() && self.bimh.picks.len() < 2 {
            return;
        }
        let groups = self.sel_props.clone();
        // the panel never grows past half the window; values wrap anywhere (point lists have no spaces) and a huge one
        // (a terrain's survey points in one string) shows its start and its length
        let max_w = (ui.available_width() * 0.45).clamp(220.0, 520.0);
        let app_c = t.app(self.active_app());
        crate::shell::side(
            ui,
            egui::Panel::right("props3").default_size(300.0).size_range(200.0..=max_w).resizable(true).frame(shell::panel_frame(&t)),
            |ui| {
                shell::grip(ui, &t, app_c, &lang::cad("ui.properties"), false, false);
                ui.add_space(8.0);
                let inner = ui.available_rect_before_wrap().shrink2(egui::vec2(12.0, 0.0));
                let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(inner));
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    // several picked: how many, by class (bimcraft.js showMany)
                    if bimclip::many_panel(self, ui) {
                        return;
                    }
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
            },
        );
    }

    /// The 3D view's colours from the theme (view3d.js's paper / model colours).
    fn style3(&self, view: &mut view3d::View3) {
        let dark = self.preset.is_dark();
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
        let mut ed = edit::Editor::new(doc, &stem);
        edit::carry_history(&ed, self.editor.as_ref());
        // LISP's CDATE reads the window's zone (`app.tz`)
        ed.eng.set_zone(Some(self.zone));
        self.editor = Some(ed);
        self.doc = None;
        self.sync_editor();
        // Studio Live's same-file join (live.js stampFileHash): the hash of the bytes opened, while unchanged since
        if let Some(ed) = self.editor.as_mut() {
            let v = ed.eng.doc().version;
            ed.file_hash = Some((cc_live::crypto::sha256_hex(&f.bytes), v));
        }
        let t3 = web_time::Instant::now();
        let n = self.doc.as_ref().map_or(0, |d| d.drawing.spaces.len());
        self.views = vec![None; n];
        self.space = if keep_space < n { keep_space } else { 0 };
        self.hidden = hidden;
        self.pending_fit = true;
        // the split screen the file was saved with (or one pane)
        paneview::from_doc(self);
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
        if self.editor.is_none() {
            return Err("file.save: no drawing is open for editing".into());
        }
        // the split screen on screen goes into the file (its VPORT table)
        paneview::to_doc(self);
        let ed = self.editor.as_ref().ok_or("file.save: no drawing is open for editing")?;
        let t0 = web_time::Instant::now();
        let (bytes, ext, _report) = cc_io_cad::save_bytes(&ed.eng.doc(), format, None).map_err(|e| format!("file.save: {}", e.0))?;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let name = format!("{}.{ext}", ed.name);
        let size = bytes.len();
        let save = self.services.save.as_mut().ok_or("file.save: saving is not available here")?;
        if let Some(to) = save(&name, bytes)? {
            self.save_format = format.to_string();
            // cadcraft.js saveFile: `app.save.written` with FORMATS[format].label()
            let label = match format {
                "dxf12" | "dxf" => "DXF R12".to_string(),
                "dxf2000" => lang::cad("save.format.dxf2000"),
                v => lang::t(
                    &lang::CADCRAFT,
                    "save.format.dwg",
                    &[("version", lang::t(&lang::CADCRAFT, "dwg.version", &[("v", v.to_string())]))],
                ),
            };
            self.message = lang::t(&lang::CADCRAFT, "app.save.written", &[("file", to.clone()), ("format", label)]);
            let _ = (ms, size);
            if let Some(ed) = self.editor.as_mut() {
                ed.eng.doc_mut().mark_saved();
            }
            recover::saved(self, format);
        }
        Ok(())
    }

    /// The picture files the open drawing names, looked for in its folder (desktop: `Services::read_path`; the web
    /// has no folders: IMAGE in the editor takes the files, as 0.5 does in the browser).
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
                        self.viewer.unreadable.push(cc_cad::pictures::picture_key(&rec.name));
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
                let s = (l.read_ms + l.compile_ms + l.geometry_ms) / 1000.0;
                self.message = lang::t(lang::BIMCRAFT, "dwg.opened", &[("file", l.name.clone()), ("s", format!("{s:.1}"))]);
                self.viewer.mark = None;
                if let Some(f) = self.viewer.find.as_mut() {
                    f.hits.clear();
                    f.ran = false;
                }
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
    /// What a panel or window of the sheet view says after the last command (the Find count line, a window's text),
    /// taken once: headless traces print it next to the status line.
    pub fn trace_note(&mut self) -> String {
        if let Some(pv) = self.part.as_mut().filter(|p| !p.trace_note.is_empty()) {
            return std::mem::take(&mut pv.trace_note);
        }
        self.pdf.as_mut().map(|p| std::mem::take(&mut p.note)).unwrap_or_default()
    }

    pub fn run(&mut self, id: &str, params: &Params) -> Result<(), String> {
        // Studio Live (liveapps.rs): `<app>.live.*`, and after every command the app's new steps go out
        if let Some(r) = liveapps::run(self, id, params.name.as_deref().unwrap_or("")) {
            return r;
        }
        let r = self.run_one(id, params);
        if self.live.is_some() {
            liveapps::pump(self);
        }
        r
    }

    fn run_one(&mut self, id: &str, params: &Params) -> Result<(), String> {
        detailview::before_run(self, id);
        match id {
            "app.tz" => {
                let zone = match params.factor {
                    None => cc_clock::Zone::System,
                    Some(m) if m.is_finite() && m.abs() <= 1560.0 => cc_clock::Zone::from_minutes(Some(m)),
                    Some(_) => return Err("app.tz: factor = minutes east of UTC (±1560), or none for the machine's zone".into()),
                };
                self.zone = zone;
                if let Some(p) = self.pdf.as_mut() {
                    p.zone = zone;
                }
            }
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
                dwg3d::fit(self);
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
                if let Some(ed) = self.editor.as_mut() {
                    ed.eng.set_zone(Some(zone));
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
                        // takeoffcraft.js: file.written
                        self.message = cc_takeoff::i18n::tf("file.written", &[("file", cc_takeoff::i18n::F::S(to))]);
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
                let mut ed = edit::Editor::new(cc_cad::Doc::new(), "Drawing1");
                edit::carry_history(&ed, self.editor.as_ref());
                ed.eng.set_zone(Some(self.zone));
                self.editor = Some(ed);
                self.doc = None;
                self.views = vec![None];
                self.space = 0;
                self.hidden.clear();
                self.error = None;
                self.message = format!("{} {}", lang::cad("app.new_drawing_2"), lang::shell("shell.input.cl.ready", &[]));
                paneview::from_doc(self);
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
                // 0.5 shows the state on the button only: the button's word and the catalog's On / Off
                let what = lang::cad(if id == "edit.osnap" { "app.object_snap" } else { "ui.ortho" });
                self.message = format!("{what}: {}", lang::cad(if on { "props.f.on" } else { "opt.off" }));
            }
            "edit.snapkind" => {
                let k = params.name.clone().unwrap_or_default();
                let kind = cc_cad_geom::pick::SNAP_KINDS
                    .iter()
                    .copied()
                    .find(|x| *x == k)
                    .ok_or_else(|| format!("edit.snapkind: {k} is not one of {}", cc_cad_geom::pick::SNAP_KINDS.join(", ")))?;
                let ed = self.editor.as_mut().ok_or("edit.snapkind: no drawing is open for editing")?;
                if let Some(i) = ed.kinds.iter().position(|x| *x == kind) {
                    ed.kinds.remove(i);
                } else {
                    ed.kinds.push(kind);
                }
            }
            "layer.current" | "layer.flag" | "layer.new" => {
                let arg = params.name.clone().ok_or(format!("{id}: name is required"))?;
                let ed = self.editor.as_mut().ok_or(format!("{id}: no drawing is open for editing"))?;
                let msg = {
                    let mut doc = ed.eng.doc_mut();
                    let fail = |e: cc_cad::doc::DocError| format!("{id}: {e:?}");
                    match id {
                        "layer.current" => {
                            if !doc.layers.contains_key(&arg) {
                                return Err(format!("layer.current: no layer {arg}"));
                            }
                            doc.set_layer(&arg, &cc_cad::entity::Obj::new(), true).map_err(fail)?;
                            lang::t(&lang::CADCRAFT, "app.layer.current_status", &[("name", arg.clone())])
                        }
                        "layer.new" => {
                            if doc.layers.contains_key(&arg) {
                                lang::t(&lang::CADCRAFT, "app.layer.exists", &[("name", arg.clone())])
                            } else {
                                let mut patch = cc_cad::entity::Obj::new();
                                patch.insert("color".into(), serde_json::json!(7));
                                doc.set_layer(&arg, &patch, true).map_err(fail)?;
                                lang::t(&lang::CADCRAFT, "app.layer.added", &[("name", arg.clone())])
                            }
                        }
                        _ => {
                            let (name, key) = arg.rsplit_once('=').ok_or("layer.flag: name = \"<layer>=on|frozen|locked|plot\"")?;
                            let l = doc.layers.get(name).ok_or(format!("layer.flag: no layer {name}"))?;
                            let now = match key {
                                "on" => l.on,
                                "frozen" => l.frozen,
                                "locked" => l.locked,
                                "plot" => l.plot,
                                _ => return Err(format!("layer.flag: {key} is not one of on, frozen, locked, plot")),
                            };
                            if key == "on" && name == doc.current.layer && l.on {
                                lang::cad("app.current_layer_make_another")
                            } else {
                                let mut patch = cc_cad::entity::Obj::new();
                                patch.insert(key.into(), serde_json::json!(!now));
                                doc.set_layer(name, &patch, false).map_err(fail)?;
                                String::new()
                            }
                        }
                    }
                };
                self.sync_editor();
                if !msg.is_empty() {
                    self.message = msg;
                }
            }
            "edit.grid" => {
                if self.editor.is_none() {
                    return Err("edit.grid: no drawing is open for editing".into());
                }
                self.look.grid = !self.look.grid;
                self.cadset.draft.grid = self.look.grid;
                // cadcraft.js toggle(): the word is not in the catalog ("Grid on" / "Grid off")
                self.message = format!("Grid {}", if self.look.grid { "on" } else { "off" });
            }
            "edit.lwt" => {
                let ed = self.editor.as_mut().ok_or("edit.lwt: no drawing is open for editing")?;
                let mut h = ed.hs.borrow_mut();
                h.lwt = !h.lwt;
                self.message = lang::cad(if h.lwt { "status.lwt.on" } else { "status.lwt.off" });
            }
            "edit.polar" => {
                let ed = self.editor.as_mut().ok_or("edit.polar: no drawing is open for editing")?;
                ed.polar = !ed.polar;
                let inc = ed.hs.borrow().polar_inc;
                self.message = if ed.polar {
                    lang::t(&lang::CADCRAFT, "status.polar.on", &[("inc", cc_engine::input::fmt(inc))])
                } else {
                    lang::cad("status.polar.off")
                };
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
                viewer::left_extras(self);
                if let Some(ed) = self.editor.as_mut() {
                    if ed.eng.busy() {
                        ed.escape();
                    }
                    ed.eng.hide3d();
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
                let p = if self.preset.is_dark() { Preset::Light } else { Preset::Dark };
                self.settings.theme = if p == Preset::Dark { "model" } else { "paper" }.into();
                self.set_preset(p);
            }
            id if id.starts_with("civil.") => return civilview::run(self, id, params),
            id if id.starts_with("takeoff.") || id.starts_with("markup.") || matches!(id, "pdf.zoom" | "pdf.file") => {
                return pdfcmd::run(self, id, params);
            }
            id if id.starts_with("detail.") => return detailview::run(self, id, params),
            id if id.starts_with("hub.") => return hub::run(self, id, params),
            id if id.starts_with("settings.") => return settings::run(self, id, params),
            id if id.starts_with("viewer.") => return viewer::run(self, id, params),
            id if id.starts_with("help.") => return help::run(self, id, params),
            id if id.starts_with("props.") => return cadprops::run(self, id, params),
            id if id.starts_with("tab3d.") => return cad3dtab::run(self, id, params),
            id if id.starts_with("plot.") => return plotdlg::run(self, id, params),
            id if id.starts_with("choose.") || id.starts_with("lib.") => return libdlg::run(self, id, params),
            id if id.starts_with("lisp.") => return lispdlg::run(self, id, params),
            id if id.starts_with("par.") => return parpanel::run(self, id, params),
            id if id.starts_with("keys.") || id.starts_with("cmp.") => return winsetdlg::run(self, id, params),
            id if id.starts_with("ss.") => return sheetsetdlg::run(self, id, params),
            id if id.starts_with("panes.") => return paneview::run(self, id, params),
            id if id.starts_with("mytools.") => return mytoolsdlg::run(self, id, params),
            id if id.starts_with("qs.") || id.starts_with("find.") => return qsdlg::run(self, id, params),
            id if id.starts_with("input.") => return ribbon::run(self, id, params),
            id if bimedit::COMMANDS.iter().any(|c| c.0 == id) => return bimedit::run(self, id, params),
            id if bimplan::COMMANDS.iter().any(|c| c.0 == id) => return bimplan::run(self, id, params),
            id if bimequip::COMMANDS.iter().any(|c| c.0 == id) => return bimequip::run(self, id, params),
            "bim.res" => return bimres::run(self, params.name.as_deref().unwrap_or("")),
            id if bimplace::COMMANDS.iter().any(|c| c.0 == id) => return bimplace::run(self, id, params),
            id if id.starts_with("recovery.") => return recover::run(self, id, params),
            id if bimlib::COMMANDS.iter().any(|c| c.0 == id) => return bimlib::run(self, id, params),
            id if bimclip::COMMANDS.iter().any(|c| c.0 == id) => return bimclip::run(self, id, params),
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
        cadset::push(self);
        let Some(ed) = self.editor.as_mut() else { return };
        // PROPERTIES / PROPERTIESCLOSE: props.js open() / close() (close turns auto-hide on)
        let props_req = ed.hs.borrow_mut().props_open.take();
        if let Some(open) = props_req {
            let _ = cadprops::run(self, if open { "props.open" } else { "props.close" }, &Params::default());
        }
        let Some(ed) = self.editor.as_mut() else { return };
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
        libdlg::sync(self);
        cadprops::sync(self);
        winsetdlg::sync(self);
        sheetsetdlg::sync(self);
        lispdlg::sync(self);
        if let Some(ed) = self.editor.as_ref() {
            // PARAMETERS opens the panel; notes from edits the window made itself (grips, Properties)
            if std::mem::take(&mut ed.hs.borrow_mut().par_open) {
                self.par.open = true;
            }
            ed.eng.flush_notes();
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
        ed.hs.borrow_mut().plot_prefs = Some((if self.plot.mono { "mono" } else { "color" }.into(), self.plot.pen));
        let req = ed.hs.borrow_mut().pick_req.take();
        if let Some(req) = req {
            let (kind, multiple) = match req {
                edit::PickReq::Drawing => ("drawing", false),
                edit::PickReq::Pictures { multiple } => ("pictures", multiple),
                edit::PickReq::Cloud => ("cloud", false),
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
        // COUNT's CSV, PUBLISH's PDF
        let blob = ed.hs.borrow_mut().blob.take();
        if let Some((name, bytes)) = blob {
            match self.services.save.as_mut().map(|f| f(&name, bytes)) {
                // cadcraft.js COUNT / PUBLISH downloads: count.saved
                Some(Ok(Some(to))) => self.message = lang::t(&lang::CADCRAFT, "count.saved", &[("name", to)]),
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
        paneview::requests(self);
        recover::frame(self);
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
        // which physical key each new press came from (the numpad's digits: keycode.rs)
        keycode::pair(&raw.events);
        if !self.held.is_empty() {
            let mut ev = std::mem::take(&mut self.held);
            ev.append(&mut raw.events);
            raw.events = ev;
        }
        if self.editor.is_none() {
            return;
        }
        // My tools' keys, then the input style's keys (Settings › Menus, keys and mouse), before the window's own
        mytoolsdlg::key_hook(self, raw);
        inputdlg::key_hook(self, raw);
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
        shell::set_small_buttons(&ctx, self.active_app() == "detailcraft");
        // what the open sheets' Snap / Snap to 45° are now is what is kept
        if let Some(p) = self.pdf.as_ref() {
            self.tk_snap = (!p.takeoff.snap.off, p.takeoff.snap.s45);
        }
        if !self.held.is_empty() {
            ctx.request_repaint();
        }
        if lispdlg::pump(self) {
            self.sync_editor();
            ctx.request_repaint();
        }
        if !self.fonts_done {
            theme::install_fonts(&ctx);
            self.fonts_done = true;
        }
        if self.theme_dirty {
            self.tokens.apply(&ctx, self.preset.is_dark());
            self.theme_dirty = false;
        }
        let incoming: Vec<ImportedFile> = match &self.services.inbox {
            Some(inbox) => std::mem::take(&mut *inbox.lock().unwrap_or_else(PoisonError::into_inner)),
            None => Vec::new(),
        };
        for f in incoming {
            self.open_file(&f);
        }
        liveapps::frame(self, &ctx);
        recover::frame(self);
        recoverdlg::show(self, &ctx);
        if let Some(t) = self.recovery.auto.as_ref().and_then(cc_recovery::Autosave::next_due) {
            let wait = (t - self.recovery.clock.unwrap_or_else(cc_clock::now_ms)).clamp(0.0, 60_000.0);
            ctx.request_repaint_after(std::time::Duration::from_millis(wait as u64));
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
            // props.js toggle(): the auto-hidden panel opens over the drawing (or folds again)
            if self.cadprops.autohide {
                self.cadprops.flyout = !self.cadprops.flyout;
            } else {
                self.cadprops.open = !self.cadprops.open;
            }
        }
        self.app_bar(ui);
        if self.editor.is_some() {
            ribbon::bar(self, ui);
        }
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
        settings::window(self, &ctx);
        inputdlg::sync(self, ctx.input(|i| i.time));
        inputdlg::window(self, &ctx);
        viewer::frame(self, &ctx);
        plotdlg::window(self, &ctx);
        cadprops::quick_window(self, &ctx);
        cadprops::custom_window(self, &ctx);
        winsetdlg::window(self, &ctx);
        mytoolsdlg::frame(self);
        mytoolsdlg::window(self, &ctx);
        sheetsetdlg::window(self, &ctx);
        libdlg::window(self, &ctx);
        lispdlg::window(self, &ctx);
        // the Library panel's picker and network answers (any view: the panel serves the drawing viewer too)
        bimlib::sync(self);
        bimres::sync(self);
        bimlib::window(self, &ctx);
        bimsheets::dialog(self, &ctx);
        bimequip::windows(self, &ctx);
        parpanel::window(self, &ctx);
        qsdlg::window(self, &ctx);
        help::panel(self, ui);
        viewer::info_panel(self, ui);
        viewer::find_panel(self, ui);
        self.tabs(ui);
        if self.part.is_some() {
            partpanel::ui(self, ui);
        } else if self.detail.is_some() {
            detailview::ui(self, ui);
        } else if let Some(pdf) = self.pdf.as_mut() {
            let t = self.tokens;
            let app_c = t.app(if pdf.mode == "takeoff" { "takeoffcraft" } else { "markupcraft" });
            // 0.7: the toolbar across the window, then the docks under their grips
            let mut save = false;
            egui::Panel::top("pdf_tools").frame(egui::Frame::new().fill(t.app_bar)).show(ui, |ui| {
                save = pdf.tool_bar(ui, &t);
            });
            let frame = shell::panel_frame(&t);
            let inset = egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 4 });
            crate::shell::side(
                ui,
                egui::Panel::left("pdf_sheets").default_size(224.0).size_range(120.0..=360.0).resizable(true).frame(frame),
                |ui| {
                    shell::grip(ui, &t, app_c, &cc_markup::i18n::t0("sheets.h"), false, false);
                    inset.show(ui, |ui| pdf.sheets_and_spaces(ui, &t));
                },
            );
            if pdf.mode != "takeoff" {
                crate::shell::side(
                    ui,
                    egui::Panel::right("mc_props").default_size(300.0).size_range(200.0..=440.0).resizable(true).frame(frame),
                    |ui| {
                        shell::grip(ui, &t, app_c, &cc_markup::i18n::t0("panels.label"), false, false);
                        inset.show(ui, |ui| pdf.right_panel(ui, &t));
                    },
                );
            }
            if pdf.mode == "takeoff" {
                crate::shell::side(
                    ui,
                    egui::Panel::right("tk_side").default_size(300.0).size_range(200.0..=440.0).resizable(true).frame(frame),
                    |ui| {
                        shell::grip(ui, &t, app_c, &cc_takeoff::i18n::t("group.takeoff"), false, false);
                        inset.show(ui, |ui| {
                            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| pdf.takeoff_panel(ui, &t))
                        });
                    },
                );
            }
            egui::CentralPanel::no_frame().show(ui, |ui| {
                let rect = ui.max_rect();
                pdf.show(ui, rect, &t);
            });
            pdf.take_space();
            pdf.dialogs(ui.ctx(), &t);
            pdf.find_dialogs(ui.ctx(), &t);
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
                            "slip" => {
                                pdf.slip_open(&[(f.name.clone(), f.bytes.clone())]);
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
                                // takeoffcraft.js: file.written
                                Some(Ok(Some(to))) => {
                                    self.message = cc_takeoff::i18n::tf("file.written", &[("file", cc_takeoff::i18n::F::S(to))])
                                }
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
                        let mut done = Vec::new();
                        for (name, bytes) in outs {
                            match self.services.save.as_mut().map(|f| f(&name, bytes)) {
                                Some(Ok(Some(to))) => done.push(to),
                                Some(Ok(None)) => {}
                                Some(Err(e)) => self.error = Some(e),
                                None => self.error = Some("Saving is not available here".into()),
                            }
                        }
                        // markupcraft.js save: save.done with the files saved
                        if !done.is_empty() {
                            self.message = lang::t(&lang::MARKUPCRAFT, "save.done", &[("list", done.join("; "))]);
                        }
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        } else if self.mc.is_some() {
            self.tool_palette(ui);
            let t = self.tokens;
            if let Some(mc) = self.mc.as_mut() {
                // 0.7's right dock: the grip, then the panel inset 16 px
                crate::shell::side(
                    ui,
                    egui::Panel::right("mc_side")
                        .default_size(300.0)
                        .size_range(200.0..=440.0)
                        .resizable(true)
                        .frame(shell::panel_frame(&t)),
                    |ui| {
                        shell::grip(ui, &t, t.app("modelcraft"), &lang::mc("ui.right.aria"), false, false);
                        egui::Frame::new()
                            .inner_margin(egui::Margin { left: 16, right: 12, top: 8, bottom: 4 })
                            .show(ui, |ui| mc.side_panel(ui, &t));
                    },
                );
                // 0.5 / 0.7's left dock: "Outliner and components" (232 px)
                shell::side(
                    ui,
                    egui::Panel::left("mc_left")
                        .default_size(232.0)
                        .size_range(180.0..=420.0)
                        .resizable(true)
                        .frame(shell::panel_frame(&t)),
                    |ui| {
                        shell::grip(ui, &t, t.app("modelcraft"), &lang::mc("ui.left.aria"), false, false);
                        egui::Frame::new()
                            .inner_margin(egui::Margin { left: 16, right: 12, top: 8, bottom: 4 })
                            .show(ui, |ui| mc.left_panel(ui, &t));
                    },
                );
                if let Err(e) = self.flush_model_save() {
                    self.error = Some(e);
                }
                let Some(mc) = self.mc.as_mut() else { return };
                mc.set_dark(self.preset.is_dark());
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
        } else if cad3dtab::showing(self) {
            egui::CentralPanel::no_frame().show(ui, |ui| cad3dtab::show(self, ui));
        } else if dwgequip::showing(self) {
            egui::CentralPanel::no_frame().show(ui, |ui| dwgequip::show(self, ui));
        } else if dwg3d::showing(self) {
            egui::CentralPanel::no_frame().show(ui, |ui| dwg3d::show(self, ui));
        } else {
            egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));
        }
        // what the sheet view asked of the host this frame (Print, Close the set)
        if let Err(e) = pdfcmd::host(self) {
            self.error = Some(e);
        }
        if self.bench.is_some() && self.doc.is_some() {
            self.bench_step(&ctx);
        }
        self.frame_ms = t0.elapsed().as_secs_f64() * 1000.0;
    }

    fn app_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let slug = self.active_app();
        let app_colour = t.app(slug);
        // 0.7's app bar: 48 px, a 1 px ink rule under it, 16 px gutters
        egui::Panel::top("app_bar")
            .exact_size(shell::APP_BAR_H)
            .frame(egui::Frame::new().fill(t.app_bar).inner_margin(egui::Margin { left: 16, right: 16, top: 0, bottom: 0 }))
            .show(ui, |ui| {
                let full = ui.max_rect();
                ui.painter().line_segment(
                    [egui::pos2(full.left() - 16.0, full.bottom() - 0.5), egui::pos2(full.right() + 16.0, full.bottom() - 0.5)],
                    egui::Stroke::new(1.0, t.rule),
                );
                // the shell's buttons (Help, Settings, Recent files) and Paper | Model stay at the right end
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    let (paper, model) = (lang::shell("shell.bar.paper", &[]), lang::shell("shell.bar.model", &[]));
                    if let Some(dark) = shell::space_toggle(ui, &t, &paper, &model, self.preset.is_dark()) {
                        self.settings.theme = if dark { "model" } else { "paper" }.into();
                        self.set_preset(if dark { Preset::Dark } else { Preset::Light });
                    }
                    self.app_bar_shell(ui);
                    ui.add_space(8.0);
                    // a narrow window scrolls the bar rather than cutting it off
                    egui::ScrollArea::horizontal()
                        .id_salt("app_bar_scroll")
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 12.0;
                                // the mark, the app's swatch and name™ (Studio's on the start screen), the test chip
                                let (mr, mresp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                                shell::mark(ui.painter(), mr, t.text_strong);
                                let home_tip = hub::t(self.hub.loc(), "window.hub.title");
                                let mut home = mresp.on_hover_text(home_tip.clone()).clicked();
                                ui.add_space(4.0);
                                let name = hub::APPS.iter().find(|a| a.0 == slug).map_or("CadCraft Studio", |a| a.1);
                                if slug != "studio" {
                                    ui.spacing_mut().item_spacing.x = 9.0;
                                    shell::swatch(ui, &t, 18.0, app_colour);
                                }
                                home |= shell::app_name(ui, &t, name, slug != "studio").on_hover_text(home_tip).clicked();
                                ui.spacing_mut().item_spacing.x = 16.0;
                                // "Rust test version": a test version of CadCraft (CLAUDE.md § 15), in 0.7's area chip
                                shell::chip(ui, "Rust test version", app_colour);
                                if home {
                                    self.run_ui("hub.home", &Params::default());
                                }
                                if shell::bar_link(ui, &t, &lang::cad("input.cmd.file.open"), true)
                                    .on_hover_text(lang::cad("ui.open_dwg_dxf_ctrl"))
                                    .clicked()
                                {
                                    self.run_ui("file.open", &Params::default());
                                }
                                // the apps' samples (the start screen's tiles open the same)
                                let mut sample: Option<&str> = None;
                                shell::bar_menu(ui, &t, &lang::t0(&cc_detail::i18n::CATALOG, "page.tool.sample"), |ui| {
                                    for (slug, name, _, _) in hub::APPS {
                                        if let Some(cmd) = hub::sample_command(slug)
                                            && ui
                                                .add(crate::shell::Btn::new(name))
                                                .on_hover_text(hub::word(hub::loc(), slug, "short"))
                                                .clicked()
                                        {
                                            sample = Some(cmd);
                                            ui.close();
                                        }
                                    }
                                });
                                if let Some(cmd) = sample {
                                    self.run_ui(cmd, &Params::default());
                                }
                                if shell::bar_link(ui, &t, &bimtools::bt("new.button", &[]), true)
                                    .on_hover_text(bimtools::bt("new.title", &[]))
                                    .clicked()
                                {
                                    self.bimh.new_dlg = Some(bimedit::NewDlg::default());
                                }
                                if shell::bar_link(ui, &t, &lang::cad("ui.new"), true)
                                    .on_hover_text(format!("{} (Ctrl+N)", lang::cad("ui.new_drawing")))
                                    .clicked()
                                {
                                    self.run_ui("file.new", &Params::default());
                                }
                                if self.editor.is_none()
                                    && shell::bar_link(ui, &t, &lang::cad("opt.edit"), self.source.is_some())
                                        .on_hover_text(lang::cad("input.grp.edit"))
                                        .clicked()
                                {
                                    self.run_ui("file.edit", &Params::default());
                                }
                                if self.mc.is_some() {
                                    let mut pick: Option<&str> = None;
                                    shell::bar_menu(ui, &t, &lang::mc("ui.export"), |ui| {
                                        for (f, label, _) in cc_m3d_files::EXPORT_FORMATS {
                                            if ui.add(crate::shell::Btn::new(*label)).clicked() {
                                                pick = Some(f);
                                                ui.close();
                                            }
                                        }
                                    })
                                    .on_hover_text(lang::mc("ui.export.title"));
                                    if let Some(f) = pick {
                                        self.run_ui("file.export", &Params { name: Some(f.into()), ..Default::default() });
                                    }
                                }
                                if self.editor.is_some() {
                                    let mut pick: Option<&str> = None;
                                    shell::bar_menu(ui, &t, &lang::cad("ui.save_as"), |ui| {
                                        for f in cc_io_cad::SAVE_FORMATS {
                                            let label = match *f {
                                                "dxf2000" => "DXF 2000".to_string(),
                                                "dxf12" => "DXF R12".to_string(),
                                                v => format!("DWG {v}"),
                                            };
                                            if ui.add(crate::shell::Btn::new(label)).clicked() {
                                                pick = Some(f);
                                                ui.close();
                                            }
                                        }
                                    })
                                    .on_hover_text(format!("{} (Ctrl+S)", lang::cad("ui.save_as")));
                                    if let Some(f) = pick {
                                        self.run_ui("file.save", &Params { name: Some(f.to_string()), ..Default::default() });
                                    }
                                }
                                let any_open = self.doc.is_some()
                                    || self.scene3.is_some()
                                    || self.mc.is_some()
                                    || self.pdf.is_some()
                                    || self.civil.is_some()
                                    || self.part.is_some();
                                if shell::bar_link(ui, &t, &lang::bim("dwg.tab.fit"), any_open)
                                    .on_hover_text(lang::bim("dwg.tab.fit.title"))
                                    .clicked()
                                {
                                    self.run_ui("view.fit", &Params::default());
                                }
                                if self.scene3.is_some() || self.mc.is_some() {
                                    ui.spacing_mut().item_spacing.x = 10.0;
                                    // BimCraft's toolbar has the views (0.7's View group): the bar keeps Back and Left
                                    let names: &[&str] = if self.ifc3.is_some() {
                                        &["back", "left"]
                                    } else {
                                        &["iso", "top", "front", "right", "back", "left"]
                                    };
                                    for name in names.iter().copied() {
                                        if shell::bar_link(ui, &t, &lang::mc(&format!("ui.views.{name}")), true)
                                            .on_hover_text(lang::mc("ui.views.title"))
                                            .clicked()
                                        {
                                            self.run_ui("view.standard", &Params { name: Some(name.into()), ..Default::default() });
                                        }
                                    }
                                    let persp = self
                                        .scene3
                                        .as_ref()
                                        .map_or_else(|| self.mc.as_ref().is_some_and(|m| m.tools.host.cam.persp), |(_, v)| v.cam.persp);
                                    if shell::bar_link(ui, &t, &lang::cad(if persp { "ui.parallel" } else { "ui.perspective" }), true)
                                        .clicked()
                                    {
                                        self.run_ui("view.projection", &Params::default());
                                    }
                                    let st = self.mc.as_ref().map(|m| m.style).or_else(|| self.scene3.as_ref().map(|(_, v)| v.style));
                                    if let Some(st) = st {
                                        let cur = if !st.faces {
                                            "wire"
                                        } else if st.xray {
                                            "xray"
                                        } else if st.hidden_line {
                                            "hidden"
                                        } else {
                                            "shaded"
                                        };
                                        let word = |id: &str| lang::mc(&format!("ui.style.mode.{id}"));
                                        let mut pick: Option<&str> = None;
                                        egui::ComboBox::new("style3", lang::mc("ui.style")).selected_text(word(cur)).width(96.0).show_ui(
                                            ui,
                                            |ui| {
                                                for id in ["shaded", "hidden", "wire", "xray"] {
                                                    if ui.selectable_label(cur == id, word(id)).clicked() {
                                                        pick = Some(id);
                                                    }
                                                }
                                            },
                                        );
                                        let mut edges = st.edges;
                                        if ui.checkbox(&mut edges, lang::mc("ui.style.edges")).changed() {
                                            pick = Some("edges");
                                        }
                                        if let Some(p) = pick {
                                            self.run_ui("view.style", &Params { name: Some(p.into()), ..Default::default() });
                                        }
                                    }
                                }
                                ui.spacing_mut().item_spacing.x = 16.0;
                                if shell::bar_link(ui, &t, &lang::cad("ui.layers"), self.doc.is_some()).clicked() {
                                    self.show_layers = !self.show_layers;
                                }
                                self.app_bar_more(ui);
                                if let Some(d) = &self.doc {
                                    ui.label(RichText::new(&d.name).font(theme::semi(15.0)).color(t.text));
                                    let rel = if d.drawing.release.is_empty() {
                                        d.drawing.version.clone()
                                    } else {
                                        lang::t(
                                            lang::BIMCRAFT,
                                            "dwg.format.release",
                                            &[("release", d.drawing.release.clone()), ("tag", d.drawing.version.clone())],
                                        )
                                    };
                                    ui.label(RichText::new(rel).small().color(t.text_dim));
                                }
                            });
                        });
                });
            });
    }

    /// The viewer's buttons (print view, Find, the Drawing panel).
    fn app_bar_more(&mut self, ui: &mut egui::Ui) {
        let mut act: Option<(&str, Params)> = None;
        if viewer::active(self) {
            let t = self.tokens;
            if shell::bar_toggle(ui, &t, &lang::bim("dwg.tab.print"), self.viewer.print)
                .on_hover_text(lang::bim("dwg.tab.print.title"))
                .clicked()
            {
                act = Some(("viewer.print", Params::default()));
            }
            if shell::bar_toggle(ui, &t, &lang::cad("dlg.find.what"), self.viewer.find.is_some())
                .on_hover_text(format!("{} (Ctrl+F)", lang::cad("dlg.find.title")))
                .clicked()
            {
                act = Some(if self.viewer.find.is_some() {
                    ("viewer.find.close", Params::default())
                } else {
                    ("viewer.find", Params::default())
                });
            }
            if shell::bar_toggle(ui, &t, &lang::bim("info.h.drawing"), self.viewer.info).clicked() {
                act = Some(("viewer.info", Params::default()));
            }
            // BimCraft's Library panel: the drawing's blocks made into items (This file), files read here (Files…)
            if shell::bar_toggle(ui, &t, &bimtools::bt("ui.library", &[]), self.bimh.lib.open)
                .on_hover_text(bimtools::bt("ui.lib.title", &[]))
                .clicked()
            {
                act = Some(("bim.lib", Params::default()));
            }
        }
        if let Some((id, p)) = act {
            self.run_ui(id, &p);
        }
    }

    /// Help, Settings and Recent files (laid out right to left at the bar's right end).
    fn app_bar_shell(&mut self, ui: &mut egui::Ui) {
        let mut act: Option<(&str, Params)> = None;
        let t = self.tokens;
        // laid out right to left: Settings, Help, Recent files (0.7's bordered app-bar buttons)
        if shell::bar_button(ui, &t, "gear", &lang::shell("shell.bar.settings", &[]), self.settings.open)
            .on_hover_text(settings::title(self))
            .clicked()
        {
            act = Some(("settings.open", Params::default()));
        }
        // Studio Live (0.5 help "Work together live": "Choose Live in the app's bar"): the app's Live window
        if let Some(prefix) = liveapps::prefix_of(self.active_app()) {
            let on = self.live.is_some() || self.detail.as_ref().is_some_and(|d| d.live.is_some());
            if shell::bar_button(ui, &t, "globe", &live::sw("window.live.button", &[]), on)
                .on_hover_text(live::sw("window.live.button.title", &[]))
                .clicked()
            {
                act = Some((prefix, Params { name: Some("open".into()), ..Default::default() }));
            }
        }
        if shell::bar_button(ui, &t, "question-circle", &lang::cad("input.cmd.app.help"), self.help.open)
            .on_hover_text(lang::shell("shell.bar.help.title", &[("key", "F1".into())]))
            .clicked()
        {
            act = Some(("help.toggle", Params::default()));
        }
        let recent = self.hub.recent.clone();
        let rb = shell::bar_button(ui, &t, "clock-history", &lang::shell("shell.recent.title", &[]), false);
        egui::Popup::menu(&rb).show(|ui| {
            if recent.is_empty() {
                ui.label(hub::t(hub::loc(), "recent.empty"));
            }
            let now = hub::now_ms();
            for (i, r) in recent.iter().enumerate().take(15) {
                let when = lang::shell("shell.recent.opened", &[("when", hub::when(r.when, now, self.zone))]);
                let b = ui.add_enabled(r.dir.is_some(), crate::shell::Btn::new(&r.name)).on_hover_text(when);
                if b.clicked() {
                    act = Some(("hub.recent", Params { index: Some(i), ..Default::default() }));
                    ui.close();
                }
            }
            if !recent.is_empty() {
                ui.separator();
                if ui.add(crate::shell::Btn::new(lang::shell("shell.recent.clear", &[]))).clicked() {
                    act = Some(("hub.clear", Params::default()));
                    ui.close();
                }
            }
        });
        if let Some((id, p)) = act {
            self.run_ui(id, &p);
        }
    }

    /// The status bar (0.7 `.cc-status`: 32 px, a 1 px line over it): the pointer's coordinates or the error, the last
    /// message, and while editing the drafting aids as 0.7's toggles (Grid, Ortho, Polar, Snap, Track, Dyn, LWT, QP).
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let aids: Vec<(String, String, &'static str, bool)> = match self.editor.as_ref() {
            Some(ed) => {
                let lwt = ed.hs.borrow().lwt;
                let st = |k: &str| lang::cad(k);
                vec![
                    (st("ui.grid"), st("ui.grid_f7"), "toggle:grid", self.look.grid),
                    (st("ui.ortho"), st("input.cmd.aids.ortho"), "toggle:ortho", ed.ortho),
                    (st("status.polar"), st("status.polar.title"), "toggle:polar", ed.polar),
                    (st("ui.snap"), st("app.object_snap"), "toggle:osnap", ed.osnap),
                    (st("status.otrack"), st("status.otrack.title"), "toggle:otrack", false),
                    (st("status.dyn"), st("status.dyn.title"), "toggle:dyn", false),
                    (st("status.lwt"), st("status.lwt.title"), "toggle:lwt", lwt),
                    (st("status.qp"), st("status.qp.title"), "qp", self.cadprops.quick),
                ]
            }
            None => Vec::new(),
        };
        // ModelCraft's status bar switches (ui.inc .mc-status: Snap, Collide)
        let mc_aids: Vec<(String, String, &'static str, bool)> = match self.mc.as_ref().filter(|_| self.editor.is_none()) {
            Some(mc) => {
                let st = mc.menu_state();
                let m = |k: &str| modelcraft::mt(k, &[]);
                vec![
                    (m("ui.snap"), m("ui.snap.title"), "toggle:snap", st.snap),
                    (m("ui.collide"), m("ui.collide.title"), "toggle:collide", st.collide),
                ]
            }
            None => Vec::new(),
        };
        let mut mc_press: Option<&'static str> = None;
        let mut press: Option<(&'static str, String)> = None;
        let mut snapkind: Option<&'static str> = None;
        egui::Panel::bottom("status")
            .exact_size(shell::STATUS_H)
            .frame(egui::Frame::new().fill(t.app_bar).inner_margin(egui::Margin { left: 16, right: 16, top: 0, bottom: 0 }))
            .show(ui, |ui| {
                let full = ui.max_rect();
                ui.painter().line_segment(
                    [egui::pos2(full.left() - 16.0, full.top() + 0.5), egui::pos2(full.right() + 16.0, full.top() + 0.5)],
                    egui::Stroke::new(1.0, t.line),
                );
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    let coord = theme::semi(13.0);
                    if let Some(e) = &self.error {
                        ui.label(RichText::new(e).font(coord).color(t.layers[0]));
                    } else if let Some((x, y)) = self.cursor.filter(|_| viewer::active(self)) {
                        ui.label(RichText::new(viewer::coords(x, y)).font(coord).color(t.text));
                    } else if let Some((x, y)) = self.cursor {
                        ui.label(RichText::new(format!("{x:.4}, {y:.4}")).font(coord).color(t.text));
                    } else if self.editor.is_some() {
                        ui.label(RichText::new("0, 0").font(coord).color(t.text));
                    } else {
                        ui.label(RichText::new(lang::cad("ui.drop_dwg_dxf_open")).font(coord).color(t.text_dim));
                    }
                    ui.label(RichText::new(&self.message).size(13.0).color(t.text_dim));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label(
                            RichText::new(format!("UI {:.1} ms · {} texts", self.frame_ms, self.last_stats.texts))
                                .size(11.0)
                                .color(t.text_disabled),
                        );
                        ui.add_space(8.0);
                        // ui.inc .cc-snapkinds: the object snap kinds, after QP
                        if let Some(kinds) = self.editor.as_ref().map(|ed| ed.kinds.clone()) {
                            let (r, resp) = ui.allocate_exact_size(egui::vec2(26.0, 24.0), egui::Sense::click());
                            ui.painter().rect(
                                r,
                                2.0,
                                if resp.hovered() { t.hover } else { Color32::TRANSPARENT },
                                egui::Stroke::new(1.0, t.line),
                                egui::StrokeKind::Inside,
                            );
                            ribbon::paint_icon(
                                ui.painter(),
                                egui::Rect::from_center_size(r.center(), egui::vec2(12.0, 12.0)),
                                "chevron-up",
                                t.text,
                            );
                            let resp = resp.on_hover_text(lang::cad("ui.choose_snaps")).on_hover_cursor(egui::CursorIcon::PointingHand);
                            egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                                ui.label(RichText::new(lang::cad("ui.object_snaps")).font(theme::bold(13.0)).color(t.text_strong));
                                for k in cc_cad_geom::pick::SNAP_KINDS {
                                    let mut on = kinds.contains(&k);
                                    // view.js SNAP_LABELS (the JS's own English words)
                                    let label = match k {
                                        "end" => "Endpoint",
                                        "mid" => "Midpoint",
                                        "cen" => "Center",
                                        "quad" => "Quadrant",
                                        "int" => "Intersection",
                                        "perp" => "Perpendicular",
                                        "tan" => "Tangent",
                                        "near" => "Nearest",
                                        "node" => "Node",
                                        _ => "Insertion",
                                    };
                                    if ui.checkbox(&mut on, label).changed() {
                                        snapkind = Some(k);
                                    }
                                }
                            });
                        }
                        for (label, tip, cmd, on) in aids.iter().rev() {
                            if shell::toggle(ui, &t, label, *on).on_hover_text(tip).clicked() {
                                press = Some((cmd, label.clone()));
                            }
                        }
                        for (label, tip, a, on) in mc_aids.iter().rev() {
                            if shell::toggle(ui, &t, label, *on).on_hover_text(tip).clicked() {
                                mc_press = Some(a);
                            }
                        }
                    });
                });
            });
        if let Some(a) = mc_press
            && let Some(mc) = self.mc.as_mut()
            && let Err(e) = mc.act(a)
        {
            mc.message = e;
        }
        if let Some(k) = snapkind {
            self.run_ui("edit.snapkind", &Params { name: Some(k.into()), ..Default::default() });
        }
        match press {
            Some(("qp", _)) => self.run_ui("props.quick", &Params::default()),
            Some((cmd, label)) => ribbon::press_pub(self, cmd, &label),
            None => {}
        }
    }

    /// The left dock panel, as 0.7's "Layers and drawing" (cadcraft.css `.cc-left`): the grip, then the Layers
    /// section (the current layer's dot, each layer's swatch and name, its eye), and while editing Blocks and Sheet
    /// set; the Drawing section's facts last.
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
        let mut layers = layers;
        // cadcraft.js renderLayers: by name, numbers in number order
        layers.sort_by(|a, b| natural_cmp(&a.0, &b.0));
        let editing = self.editor.is_some();
        let current = self.editor.as_ref().map(|ed| ed.eng.doc().current.layer.clone());
        // the drawing's own layer flags while editing (cadcraft.js renderLayers: on, frozen, locked, plot)
        let flags: HashMap<String, [bool; 4]> = self
            .editor
            .as_ref()
            .map(|ed| ed.eng.doc().layers.iter().map(|(n, l)| (n.clone(), [l.on, l.frozen, l.locked, l.plot])).collect())
            .unwrap_or_default();
        let mut new_layer = self.look.new_layer.take();
        let mut tool_cmd: Option<(&'static str, String)> = None;
        // cadcraft.js renderBlocks / renderInfo: the named blocks (with their uses) and the drawing's facts
        let mut blocks: Vec<(String, usize)> = Vec::new();
        let facts: Vec<(String, String)> = match self.editor.as_ref() {
            Some(ed) => {
                let d = ed.eng.doc();
                let mut names: Vec<String> = d.blocks.keys().filter(|k| !k.starts_with('*')).cloned().collect();
                names.sort_by(|a, b| natural_cmp(a, b));
                blocks = names
                    .into_iter()
                    .map(|n| {
                        let u = d.block_uses(&n);
                        (n, u)
                    })
                    .collect();
                let m = |k: &str| d.meta.get(k).and_then(serde_json::Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
                let dirty = d.version != d.saved_version;
                let mut rows = vec![
                    (
                        lang::cad("app.info.file"),
                        format!(
                            "{}{}",
                            m("name").unwrap_or_default(),
                            if dirty { format!(" {}", lang::cad("app.info.changed")) } else { String::new() }
                        ),
                    ),
                    (
                        lang::cad("app.info.format"),
                        // the importer keeps no format word yet: the opened file's kind stands in for it
                        m("format")
                            .or_else(|| self.source.as_ref().and_then(|f| f.name.rsplit_once('.').map(|(_, e)| e.to_uppercase())))
                            .unwrap_or_else(|| lang::cad("app.new_drawing")),
                    ),
                    (lang::cad("app.info.units"), m("units").unwrap_or_else(|| lang::cad("app.info.unitless"))),
                    (lang::cad(if d.layout().is_some() { "app.on_this_sheet" } else { "app.info.objects" }), d.count().to_string()),
                    (lang::cad("app.info.layers"), d.layers.len().to_string()),
                ];
                if !blocks.is_empty() {
                    rows.push((lang::cad("app.info.blocks"), blocks.len().to_string()));
                }
                if !d.layouts.is_empty() {
                    rows.push((lang::cad("app.info.sheets"), d.layouts.len().to_string()));
                }
                rows
            }
            None => {
                let d = &doc.drawing;
                let format = if d.release.is_empty() && d.version.is_empty() {
                    lang::cad("app.new_drawing")
                } else if d.release.is_empty() {
                    d.version.clone()
                } else {
                    lang::t(lang::BIMCRAFT, "dwg.format.release", &[("release", d.release.clone()), ("tag", d.version.clone())])
                };
                let units = if d.units.is_empty() { lang::cad("app.info.unitless") } else { d.units.clone() };
                vec![
                    (lang::cad("app.info.file"), doc.name.clone()),
                    (lang::cad("app.info.format"), format),
                    (lang::cad("app.info.units"), units),
                    (lang::cad("app.info.objects"), d.stats.entities.to_string()),
                    (lang::cad("app.info.layers"), d.layers.len().to_string()),
                ]
            }
        };
        let app_c = t.app(self.active_app());
        let mut act: Option<(&str, Params)> = None;
        crate::shell::side(
            ui,
            egui::Panel::left("layers").default_size(248.0).size_range(180.0..=420.0).resizable(true).frame(shell::panel_frame(&t)),
            |ui| {
                if shell::grip(ui, &t, app_c, &lang::cad("ui.layers_drawing"), false, true) == shell::GripAct::Close {
                    act = Some(("layers.close", Params::default()));
                }
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    shell::section_frame().show(ui, |ui| {
                        shell::section_head(ui, &t, &lang::cad("ui.layers"), |ui| {
                            if !editing {
                                if shell::link(ui, &t, "", &lang::cad("opt.showall")).clicked() {
                                    act = Some(("layer.all", Params::default()));
                                }
                                return;
                            }
                            // ui.inc: Tools (the layer commands' menu), then + New layer (right to left here)
                            let tools = shell::link(ui, &t, "", &lang::cad("layers.tools")).on_hover_text(lang::cad("layers.tools.title"));
                            egui::Popup::menu(&tools).show(|ui| {
                                for x in [
                                    Some(("LAYISO", "layers.tools.iso")),
                                    Some(("LAYUNISO", "layers.tools.uniso")),
                                    Some(("LAYOFF", "layers.tools.off")),
                                    Some(("LAYFRZ", "layers.tools.frz")),
                                    Some(("LAYMCUR", "layers.tools.mcur")),
                                    None,
                                    Some(("LAYON", "layers.tools.on")),
                                    Some(("LAYTHW", "layers.tools.thw")),
                                    None,
                                    Some(("LAYMRG", "layers.tools.mrg")),
                                    Some(("LAYDEL", "layers.tools.del")),
                                    Some(("RENAME", "layers.tools.ren")),
                                ] {
                                    match x {
                                        None => {
                                            ui.separator();
                                        }
                                        Some((cmd, key)) => {
                                            let label = lang::cad(key);
                                            if ui.button(format!("{label}    {cmd}")).clicked() {
                                                tool_cmd = Some((cmd, label));
                                                ui.close();
                                            }
                                        }
                                    }
                                }
                            });
                            if shell::link(ui, &t, "plus", &lang::cad("ui.new_layer")).clicked() {
                                new_layer = if new_layer.is_some() { None } else { Some(String::new()) };
                            }
                        });
                        // the New layer form (ui.inc data-layer-form): a name and Add
                        if let Some(name) = new_layer.as_mut() {
                            let mut add = false;
                            ui.horizontal(|ui| {
                                let r =
                                    ui.add(shell::field(name).hint_text(lang::cad("ui.layer_name")).desired_width(140.0).char_limit(255));
                                add = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                add |= ui.add(shell::Btn::primary(lang::cad("ui.add"))).clicked();
                            });
                            if add && !name.trim().is_empty() {
                                act = Some(("layer.new", Params { name: Some(name.trim().to_string()), ..Default::default() }));
                            }
                        }
                        for (name, c) in &layers {
                            let f = flags.get(name).copied().unwrap_or([true, false, false, true]);
                            let on = if editing { f[0] && !f[1] } else { !self.hidden.contains(name) };
                            let cur = current.as_deref() == Some(name.as_str());
                            ui.horizontal(|ui| {
                                ui.set_min_height(32.0);
                                ui.spacing_mut().item_spacing.x = 8.0;
                                if editing {
                                    // the current dot: a click makes the layer current (cadcraft.js setCurrent)
                                    let (r, resp) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
                                    if cur {
                                        ui.painter().circle_filled(r.center(), 5.5, t.text_strong);
                                    } else {
                                        ui.painter().circle_stroke(r.center(), 5.0, egui::Stroke::new(1.0, t.line));
                                    }
                                    let tip = lang::cad(if cur { "app.current_layer" } else { "app.make_current" });
                                    if resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !cur {
                                        act = Some(("layer.current", Params { name: Some(name.clone()), ..Default::default() }));
                                    }
                                }
                                shell::swatch(ui, &t, 16.0, *c);
                                let font = if cur { theme::bold(14.0) } else { egui::FontId::proportional(14.0) };
                                let ink = if on { t.text } else { t.text_disabled };
                                // a long name ends in … before the flags (0.7 `.cc-layer-name`: text-overflow ellipsis)
                                let room = (ui.available_width() - if editing { 100.0 } else { 30.0 }).max(30.0);
                                let nr = ui
                                    .allocate_ui_with_layout(
                                        egui::vec2(room, 22.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.add(
                                                egui::Label::new(RichText::new(name).font(font).color(ink))
                                                    .truncate()
                                                    .sense(egui::Sense::click()),
                                            )
                                        },
                                    )
                                    .inner;
                                if editing && nr.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    act = Some(("layer.current", Params { name: Some(name.clone()), ..Default::default() }));
                                }
                                if editing {
                                    // the flags, right to left: plot, lock, freeze, on (0.7: pressed ones in the orange)
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.spacing_mut().item_spacing.x = 2.0;
                                        for (key, pressed, icon, tip) in [
                                            ("plot", !f[3], "printer", if f[3] { "app.do_not_plot_layer" } else { "app.plot_layer" }),
                                            (
                                                "locked",
                                                f[2],
                                                if f[2] { "lock" } else { "unlock" },
                                                if f[2] { "app.layer.unlock" } else { "app.layer.lock" },
                                            ),
                                            ("frozen", f[1], "snow", if f[1] { "app.layer.thaw" } else { "app.layer.freeze" }),
                                            (
                                                "on",
                                                !f[0],
                                                if f[0] { "eye" } else { "eye-slash" },
                                                if f[0] { "app.turn_off" } else { "app.turn_on" },
                                            ),
                                        ] {
                                            let (r, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                                            if resp.hovered() {
                                                ui.painter().rect(
                                                    r,
                                                    2.0,
                                                    t.hover,
                                                    egui::Stroke::new(1.0, t.line),
                                                    egui::StrokeKind::Inside,
                                                );
                                            }
                                            let col = if pressed { t.layers[6] } else { t.text_dim };
                                            ribbon::paint_icon(
                                                ui.painter(),
                                                egui::Rect::from_center_size(r.center(), egui::vec2(15.0, 15.0)),
                                                icon,
                                                col,
                                            );
                                            if resp.on_hover_text(lang::cad(tip)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
                                            {
                                                act = Some((
                                                    "layer.flag",
                                                    Params { name: Some(format!("{name}={key}")), ..Default::default() },
                                                ));
                                            }
                                        }
                                    });
                                    return;
                                }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let (r, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                                    if resp.hovered() {
                                        ui.painter().rect(r, 2.0, t.hover, egui::Stroke::new(1.0, t.line), egui::StrokeKind::Inside);
                                    }
                                    ribbon::paint_icon(
                                        ui.painter(),
                                        egui::Rect::from_center_size(r.center(), egui::vec2(15.0, 15.0)),
                                        if on { "eye" } else { "eye-slash" },
                                        t.text_dim,
                                    );
                                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                        act = Some(("layer.toggle", Params { name: Some(name.clone()), ..Default::default() }));
                                    }
                                });
                            });
                        }
                        if editing {
                            shell::note(ui, &t, &lang::cad("ui.filled_dot_current_layer"));
                        }
                    });
                    if editing {
                        shell::section_rule(ui, &t);
                        shell::section_frame().show(ui, |ui| {
                            shell::section_head(ui, &t, &lang::cad("ui.blocks"), |ui| {
                                if shell::link(ui, &t, "", &lang::cad("ui.library_2")).clicked() {
                                    act = Some(("lib.open", Params::default()));
                                }
                            });
                            if blocks.is_empty() {
                                let note = format!(
                                    "{} {} {} B {}",
                                    lang::cad("ui.no_blocks_yet_select"),
                                    lang::cad("ui.make"),
                                    lang::cad("ui.or_type"),
                                    lang::cad("ui.turn_them_into_block")
                                );
                                shell::note(ui, &t, &note);
                            } else {
                                for (name, uses) in blocks.iter().take(300) {
                                    ui.horizontal(|ui| {
                                        ui.set_min_height(26.0);
                                        ui.add(egui::Label::new(RichText::new(name).size(14.0).color(t.text)).truncate());
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            // the JS's own word for the button ("Insert", not in its catalog)
                                            if shell::link(ui, &t, "", "Insert").clicked() {
                                                act = Some(("block.insert", Params { name: Some(name.clone()), ..Default::default() }));
                                            }
                                            ui.label(RichText::new(format!("{uses}×")).size(14.0).color(t.text_dim));
                                        });
                                    });
                                }
                                if blocks.len() > 300 {
                                    shell::note(
                                        ui,
                                        &t,
                                        &lang::t(&lang::CADCRAFT, "app.blocks.more", &[("count", (blocks.len() - 300).to_string())]),
                                    );
                                }
                            }
                        });
                        shell::section_rule(ui, &t);
                        shell::section_frame().show(ui, |ui| {
                            shell::section_head(ui, &t, &lang::cad("sheetset.h"), |ui| {
                                if shell::link(ui, &t, "", &lang::cad("sheetset.menu")).clicked() {
                                    act = Some(("ss.show", Params::default()));
                                }
                            });
                            if self.sheetset.set.is_none() {
                                shell::note(ui, &t, &lang::cad("sheetset.empty"));
                            }
                        });
                    }
                    shell::section_rule(ui, &t);
                    shell::section_frame().show(ui, |ui| {
                        shell::section_head(ui, &t, &lang::cad("ui.drawing"), |_| {});
                        egui::Grid::new("cc-dl").num_columns(2).spacing(egui::vec2(12.0, 4.0)).show(ui, |ui| {
                            for (k, v) in &facts {
                                shell::note(ui, &t, k);
                                ui.label(RichText::new(v).size(14.0).color(t.text));
                                ui.end_row();
                            }
                        });
                    });
                });
            },
        );
        self.look.new_layer = if matches!(act, Some(("layer.new", _))) { None } else { new_layer };
        if let Some((cmd, label)) = tool_cmd {
            ribbon::press_pub(self, cmd, &label);
        }
        if let Some((id, p)) = act {
            if id == "layers.close" {
                self.show_layers = false;
            } else if id == "block.insert" {
                // cadcraft.js renderBlocks: INSERT, then the block's name answers its first question
                self.run_ui("cmd.line", &Params { name: Some("INSERT".into()), ..Default::default() });
                if let Some(n) = p.name {
                    self.run_ui("cmd.line", &Params { name: Some(n), ..Default::default() });
                }
            } else {
                self.run_ui(id, &p);
            }
        }
    }

    /// cadcraft.js `sheetSetHere()`: the open sheet set's sheets of this drawing, as (layout, number).
    fn sheet_set_here(&self) -> Vec<(String, String)> {
        let Some(set) = self.sheetset.set.as_ref() else { return Vec::new() };
        let src = self.editor.as_ref().map_or(String::new(), |ed| {
            let d = ed.eng.doc();
            let m = |k: &str| d.meta.get(k).and_then(serde_json::Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
            m("source").or_else(|| m("name")).unwrap_or_default().to_lowercase()
        });
        let stem = src.strip_suffix(".dwg").or_else(|| src.strip_suffix(".dxf")).unwrap_or(&src).to_string();
        let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
        set.get("sheets")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter(|sh| src.is_empty() || s(sh, "file").to_lowercase().rsplit('/').next().unwrap_or("").starts_with(&stem))
            .map(|sh| (s(sh, "layout"), s(sh, "number")))
            .collect()
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        let t = self.tokens;
        let Some(doc) = &self.doc else { return };
        // model space by the app's word for it (the file's own name for it is "Model" in every language)
        let names: Vec<(String, bool)> = doc
            .drawing
            .spaces
            .iter()
            .map(|s| (if s.kind == SpaceKind::Model { lang::cad("app.tab.model") } else { s.name.clone() }, s.kind == SpaceKind::Paper))
            .collect();
        let editing = self.editor.is_some();
        // the viewer's own tabs after the sheets: Equipment (n), 3D (viewer.rs)
        let extra = viewer::extra_tabs(self);
        if names.len() <= 1 && !editing && extra.is_empty() {
            return;
        }
        // Settings › CadCraft › Sheet tabs = numbers only (WL.9): the number on the tab, the whole name in its tooltip
        let numbers: Vec<Option<String>> = if editing && self.cadset.tabs_number {
            let here = self.sheet_set_here();
            names.iter().map(|(n, paper)| if *paper { cadset::sheet_number_of(n, &here) } else { None }).collect()
        } else {
            Vec::new()
        };
        let mut pick = None;
        let mut add = false;
        let mut open3d = false;
        let mut extra_act: Option<&'static str> = None;
        let in3d = cad3dtab::showing(self) || extra.iter().any(|e| e.selected);
        // 0.7 `.cc-tabs`: 30 px under the drawing, a 1 px line over it; the open tab on the sheet with a 2 px action rule
        egui::Panel::bottom("tabs")
            .exact_size(30.0)
            .frame(egui::Frame::new().fill(t.panel).inner_margin(egui::Margin { left: 8, right: 8, top: 1, bottom: 0 }))
            .show(ui, |ui| {
                let full = ui.max_rect();
                ui.painter().line_segment(
                    [egui::pos2(full.left() - 8.0, full.top() - 0.5), egui::pos2(full.right() + 8.0, full.top() - 0.5)],
                    egui::Stroke::new(1.0, t.line),
                );
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (i, (n, paper)) in names.iter().enumerate() {
                            let label = numbers.get(i).cloned().flatten().unwrap_or_else(|| n.clone());
                            let mut r = shell::tab(ui, &t, &label, i == self.space && !in3d);
                            if *paper && editing {
                                r = r.on_hover_text(crate::edit::lang_text("app.tab.sheet", &[("name", n.clone())]));
                            }
                            if r.clicked() {
                                pick = Some(i);
                            }
                            // cadcraft.js renderTabs: Model, 3D, then the sheets
                            if i == 0
                                && editing
                                && shell::tab(ui, &t, "3D", in3d)
                                    .on_hover_text(crate::edit::lang_text("app.model_3d_drawing_ground", &[]))
                                    .clicked()
                            {
                                open3d = true;
                            }
                        }
                        for e in &extra {
                            if shell::tab(ui, &t, &e.label, e.selected).on_hover_text(&e.title).clicked() {
                                extra_act = Some(e.cmd);
                            }
                        }
                        // cadcraft.js's "+" tab: a new sheet (LAYOUT)
                        if editing
                            && shell::tab(ui, &t, "+", false)
                                .on_hover_text(crate::edit::lang_text("ui.new_sheet_layout_title", &[]))
                                .clicked()
                        {
                            add = true;
                        }
                    });
                });
            });
        if let Some(i) = pick {
            self.run_ui("space.select", &Params { index: Some(i), ..Default::default() });
        }
        if open3d && !in3d {
            self.run_ui("tab3d.open", &Params::default());
        }
        if let Some(id) = extra_act {
            self.run_ui(id, &Params::default());
        }
        if add {
            self.run_ui("cmd.line", &Params { name: Some("LAYOUT".into()), ..Default::default() });
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        // the split screen (paneview.rs): the active pane is drawn and edited here, the others after it
        let rect = paneview::begin(self, ui, full);
        let resp = if self.panes.split.panes.is_some() {
            ui.interact(rect, egui::Id::unique("cc-pane-active"), egui::Sense::click_and_drag())
        } else {
            ui.allocate_rect(rect, egui::Sense::click_and_drag())
        };
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let (w, h) = (f64::from(rect.width()) * ppp, f64::from(rect.height()) * ppp);
        self.canvas_px = (w, h);
        let Some(doc) = &self.doc else {
            let t = self.tokens;
            ui.painter().rect_filled(rect, 0.0, t.canvas_bg);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                lang::cad("ui.drop_dwg_dxf_open"),
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
        // touch: two fingers pinch-zoom and pan (viewer.rs)
        let touching = viewer::touch(ui, rect, &mut view, ppp);
        // input: drag pans (any button; drafting: the buttons the input style pans with, middle / right), wheel zooms
        // about the pointer (drafting: the style's wheel direction and speed), double click fits
        let mouse = if editing { self.input_profile().mouse } else { None };
        let pans = |b: egui::PointerButton| match (&mouse, b) {
            (_, egui::PointerButton::Primary) => !editing,
            (Some(m), egui::PointerButton::Middle) => m.pan2d.iter().any(|x| x == "middle"),
            (Some(m), egui::PointerButton::Secondary) => m.pan2d.iter().any(|x| x == "right"),
            _ => true,
        };
        let wheel = mouse.as_ref().map_or(1.0, |m| f64::from(m.wheel) * m.zoom_speed);
        let by = [egui::PointerButton::Primary, egui::PointerButton::Secondary, egui::PointerButton::Middle];
        if !touching && resp.dragged() && by.iter().any(|b| resp.dragged_by(*b) && pans(*b)) {
            let d = resp.drag_delta();
            // whole device pixels while a point cloud shows (its overscan moves: cloudview.rs)
            let (dx, dy) = cloudview::pan_steps(f64::from(d.x) * ppp, f64::from(d.y) * ppp);
            view.pan(dx, dy);
        } else if resp.drag_stopped() {
            cloudview::pan_end();
        }
        if let Some(p) = resp.hover_pos() {
            let sx = f64::from(p.x - rect.min.x) * ppp;
            let sy = f64::from(p.y - rect.min.y) * ppp;
            let (scroll, zoom) = ui.ctx().input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            if scroll != 0.0 {
                view.zoom_at(sx, sy, (f64::from(scroll) / 200.0 * wheel).exp());
            }
            if zoom != 1.0 && !touching {
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
        let underlay = match self.editor.as_ref() {
            Some(ed) => cloudview::underlay_for(ui.ctx(), ed, rect, ppp, &doc.drawing, self.space, &view),
            None => Vec::new(),
        };
        let frame = canvas::Frame {
            underlay: &underlay,
            pictures: &pictures,
            drawing: &doc.drawing,
            geometry: &doc.geometry,
            id: doc.id,
            space: self.space,
            view,
            hidden: &self.hidden,
            tokens: &self.tokens,
            line_px: ppp as f32,
            print: self.viewer.print && self.editor.is_none(),
            lw_display: self.editor.as_ref().is_some_and(|e| e.hs.borrow().lwt),
            layer_handles: Some(&doc.layer_handles),
            mark: self.viewer.mark.filter(|m| m.0 == self.space).map(|m| (m.1, m.2)),
            slot_base: 0,
            grid: editing && self.look.grid,
        };
        self.last_stats = canvas::paint(ui, rect, &frame);
        if editing {
            // Settings › CadCraft › Corner indicator: the view cube on the Model tab takes the presses on it
            let cube = self.space == 0 && self.cadset.corner == "cube" && !cad3dtab::showing(self);
            let on_cube = cube && ui.input(|i| i.pointer.press_origin()).is_some_and(|p| cad3dtab::on_cube2d(full, p));
            if !paneview::swallowing(self, ui) && !on_cube {
                self.edit_input(ui, &resp, rect, &view, ppp);
            }
            self.edit_overlays(ui, rect, &view, ppp, true);
            paneview::corner_icon(self, ui, rect, &view, self.space);
            paneview::end(self, ui, full);
            self.start_hint(ui.ctx(), rect);
            if cube {
                cad3dtab::cube2d(self, ui, full);
            }
        }
    }

    /// 0.5's "Start drawing" card (ui.inc `[data-hint]`) over a new, empty drawing, until a command runs or the
    /// drawing has objects (cadcraft.js `hideHint`).
    fn start_hint(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let Some(ed) = self.editor.as_ref() else { return };
        if !self.look.hint_gone && (ed.eng.running().is_some() || ed.eng.doc().count() > 0) {
            self.look.hint_gone = true;
        }
        if self.look.hint_gone || cad3dtab::showing(self) {
            return;
        }
        use shell::Frag::{Kbd, Strong, Text};
        let c = |k: &str| Text(lang::cad(k));
        let items = vec![
            vec![
                c("ui.type"),
                Kbd("L"),
                c("ui.and_press"),
                Kbd("Enter"),
                c("ui.line_click_points_type"),
                Kbd("@100,0"),
                c("ui.press"),
                Kbd("Enter"),
                c("ui.to_finish"),
            ],
            vec![
                Kbd("C"),
                c("ui.circle_2"),
                Kbd("REC"),
                c("ui.rectangle_2"),
                Kbd("TR"),
                c("ui.trim_2"),
                Kbd("O"),
                c("ui.offset_type"),
                Kbd("HELP"),
                c("ui.every_command"),
            ],
            vec![c("ui.wheel_zoom_drag_wheel"), Kbd("Enter"), Text(".".into())],
            vec![
                Kbd("DLI"),
                c("ui.dimensions"),
                Kbd("B"),
                c("ui.makes_block"),
                Strong("+"),
                c("ui.under_drawing_makes_sheet"),
                Kbd("Ctrl"),
                Text("+".into()),
                Kbd("P"),
                c("ui.plots_pdf"),
            ],
            vec![c("ui.drop_dwg_dxf_here")],
        ];
        let max_w = (rect.width() - 32.0).clamp(200.0, 512.0);
        shell::hint_card(ctx, &self.tokens, rect.min + egui::vec2(16.0, 16.0), max_w, &lang::cad("ui.start_drawing"), &items);
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
        let lasso_on = !ed.hs.borrow().lasso_box;
        let scr = |p: egui::Pos2| (f64::from(p.x - rect.min.x), f64::from(p.y - rect.min.y));
        if resp.drag_started_by(egui::PointerButton::Primary)
            && let Some(p) = resp.interact_pointer_pos()
        {
            let down = ui.input(|i| i.pointer.press_origin()).unwrap_or(p - resp.drag_delta());
            if lasso_on && ed.can_select_now() && ed.pending_box.is_none() {
                // LASSOMODE 1: press and drag draws a lasso (cadcraft.js startDrag)
                ed.lasso_start(world(down), scr(down), world(p), scr(p));
            } else {
                let w = world(down);
                self.drag_from = Some((w.x, w.y));
            }
        }
        if ed.lasso.is_some() {
            if resp.dragged_by(egui::PointerButton::Primary)
                && let Some(p) = resp.interact_pointer_pos()
            {
                ed.lasso_move(world(p), scr(p));
            }
            let (space, esc) = ui.input(|i| (i.key_pressed(egui::Key::Space), i.key_pressed(egui::Key::Escape)));
            if space {
                ed.lasso_cycle();
            }
            if esc {
                ed.lasso = None;
            }
        }
        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            if ed.lasso.is_some() {
                ed.finish_lasso(pxu);
            } else if let (Some((x, y)), Some(p)) = (self.drag_from.take(), resp.interact_pointer_pos()) {
                ed.finish_box(cc_geom::P2::new(x, y), world(p), pxu);
            }
        }
        // a constraint's glyph or label (gview.js hits): select it, or edit a dimensional one's value on a double click
        let par_hit = if !ed.eng.busy() { resp.interact_pointer_pos().and_then(|p| parpanel::hit_at(self, p)) } else { None };
        if let Some(h) = par_hit.as_ref()
            && resp.double_clicked_by(egui::PointerButton::Primary)
            && h.dim
        {
            let cid = h.cid.clone();
            parpanel::open_edit(self, &cid);
            self.sync_editor();
            return;
        }
        if let Some(h) = par_hit
            && resp.clicked_by(egui::PointerButton::Primary)
        {
            self.run_ui("par.pick", &Params { name: Some(h.cid), ..Default::default() });
            self.sync_editor();
            return;
        }
        let Some(ed) = self.editor.as_mut() else { return };
        if resp.clicked_by(egui::PointerButton::Primary)
            && let Some(p) = resp.interact_pointer_pos()
        {
            // a click elsewhere clears the selected constraint (and picks as usual)
            self.par.selected = None;
            ed.click(world(p), pxu);
        }
        if resp.clicked_by(egui::PointerButton::Secondary) {
            // right click = Enter, as in AutoCAD
            ed.submit();
        }
        self.sync_editor();
    }

    /// The rubber band, the selection, the snap marker, the selection window and the crosshair (`full`; another pane
    /// showing the same space: only the selection and the rubber band, as view.js `overPane(ctx, false)`).
    fn edit_overlays(&self, ui: &egui::Ui, rect: egui::Rect, view: &View2D, ppp: f64, full: bool) {
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
            for e in sel.iter().filter(|_| full) {
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
        if !full {
            return;
        }
        parpanel::draw(self, &painter, &to_pos, view.zoom / ppp);
        // Studio Live: others' cursors, selections, holds and the host's padlocks (live.js drawOthers)
        #[cfg(not(target_arch = "wasm32"))]
        cadlive::draw_others(self, &painter, rect, &to_pos);
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
                "cloud" => {
                    // a point cloud's point: Node's marker (a circle with a cross)
                    painter.circle_stroke(c, 6.0, st);
                    painter.line_segment([c + egui::vec2(-6.0, -6.0), c + egui::vec2(6.0, 6.0)], st);
                    painter.line_segment([c + egui::vec2(-6.0, 6.0), c + egui::vec2(6.0, -6.0)], st);
                }
                _ => {
                    painter.line_segment([c + egui::vec2(-6.0, -6.0), c + egui::vec2(6.0, 6.0)], st);
                    painter.line_segment([c + egui::vec2(-6.0, 6.0), c + egui::vec2(6.0, -6.0)], st);
                }
            }
            let label = if s.kind == "cloud" { edit::lang_text("pc.snap", &[]) } else { s.kind.to_string() };
            painter.text(c + egui::vec2(10.0, 10.0), egui::Align2::LEFT_TOP, label, egui::FontId::proportional(11.0), st.color);
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
        // a lasso (lasso.js): window = solid outline, crossing = dashed, fence = a dashed open path (no fill: a freehand
        // outline is rarely convex)
        if let Some(l) = ed.lasso.as_ref() {
            let mode = l.mode.unwrap_or("window");
            let pts: Vec<egui::Pos2> = l.pts.iter().map(|q| to_pos(q.x, q.y)).collect();
            let c = if mode == "window" { egui::Color32::from_rgb(0x3c, 0x80, 0xe0) } else { egui::Color32::from_rgb(0x3c, 0xb0, 0x60) };
            let st = egui::Stroke::new(1.0, c);
            let mut path = pts.clone();
            if mode != "fence"
                && let Some(f) = pts.first()
            {
                path.push(*f);
            }
            if mode == "window" {
                painter.add(egui::Shape::line(path, st));
            } else {
                painter.extend(egui::Shape::dashed_line(&path, st, 5.0, 3.0));
            }
        }
        // crosshair
        if let Some(hp) = hover.filter(|p| rect.contains(*p)) {
            // Settings › CadCraft › Cursor (view.js CURSOR_SIZES: arm and pick box)
            let (arm, side, _) = self.cadset.cursor_spec();
            let st = egui::Stroke::new(1.0, t.ink.gamma_multiply(0.6));
            painter.line_segment([egui::pos2(hp.x - arm, hp.y), egui::pos2(hp.x + arm, hp.y)], st);
            painter.line_segment([egui::pos2(hp.x, hp.y - arm), egui::pos2(hp.x, hp.y + arm)], st);
            painter.rect_stroke(egui::Rect::from_center_size(hp, egui::vec2(side, side)), 0.0, st, egui::StrokeKind::Middle);
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
        // 0.7 `.cc-cmd`: 114 px under a 1 px ink rule; the history (13 px, ink-2), then the command line over a 1 px
        // grid rule: the prompt in 14 px bold, the field in 15 px semibold over a 1 px action underline
        egui::Panel::bottom("cmdline")
            .exact_size(114.0)
            .frame(egui::Frame::new().fill(t.panel).inner_margin(egui::Margin { left: 16, right: 16, top: 5, bottom: 0 }))
            .show(ui, |ui| {
                let full = ui.max_rect();
                let p = ui.painter();
                p.line_segment(
                    [egui::pos2(full.left() - 16.0, full.top() - 4.5), egui::pos2(full.right() + 16.0, full.top() - 4.5)],
                    egui::Stroke::new(1.0, t.rule),
                );
                p.line_segment(
                    [egui::pos2(full.left() - 16.0, full.bottom() - 39.5), egui::pos2(full.right() + 16.0, full.bottom() - 39.5)],
                    egui::Stroke::new(1.0, t.divider),
                );
                ui.spacing_mut().item_spacing.y = 0.0;
                let log: Vec<String> = ed.hs.borrow().log.iter().rev().take(4).rev().cloned().collect();
                ui.allocate_ui(egui::vec2(full.width(), 69.0), |ui| {
                    ui.set_min_height(69.0);
                    for l in &log {
                        ui.add(egui::Label::new(RichText::new(l).size(13.0).color(t.text_dim)).truncate());
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.set_min_height(36.0);
                    let lab = ui.label(RichText::new(ed.prompt()).font(theme::bold(14.0)).color(t.text_strong));
                    let under = egui::Rect::from_min_max(
                        egui::pos2(lab.rect.right() + 8.0, lab.rect.center().y + 12.0),
                        egui::pos2(full.right(), lab.rect.center().y + 13.0),
                    );
                    ui.painter().rect_filled(under, 0.0, t.accent);
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
                        .font(theme::semi(15.0))
                        .desired_width(f32::INFINITY)
                        .frame(egui::Frame::NONE);
                    let r = ui.add(te).labelled_by(lab.id);
                    if enter && r.has_focus() {
                        submit = true;
                    }
                });
            });
        // Space sends the line too, except while a command asks for text or a "(…" is typed (appload.js)
        if ed.line.ends_with(' ')
            && ed.eng.expects() != Some(cc_engine::prompt::Kind::Text)
            && !ed.eng.lisp_typing(ed.line.trim_end_matches(' '))
        {
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
        // the style's keys wait while another field or a dialog has the keyboard (app-input-ui.js inField / dialog[open])
        self.ribbon.keys_blocked =
            elsewhere0 || self.settings.open || self.plot.open || self.caddlg.qs.is_some() || self.caddlg.find.is_some();
        self.clip_keys = ed.line.is_empty()
            && !ed.eng.busy()
            && !elsewhere0
            && self.caddlg.qs.is_none()
            && self.caddlg.find.is_none()
            && !self.plot.open;
        if ctx.input(|i| i.key_pressed(egui::Key::F7)) {
            self.run_ui("edit.grid", &Params::default());
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
        // the 3D tab takes Esc and Delete first (its tool, open group, selection)
        let (mut esc, mut del) = (esc, del);
        if ed.eng.in3d() && !elsewhere {
            let typing = !ed.line.is_empty();
            if esc && ed.eng.ws_key("Escape", typing) {
                esc = false;
            }
            if del && ed.eng.ws_key("Delete", typing) {
                del = false;
            }
        }
        if esc && !elsewhere {
            ed.escape();
            ed.line.clear();
        }
        if del && ed.line.is_empty() && !elsewhere && self.par.selected.is_some() {
            self.run_ui("par.remove", &Params::default());
            return;
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

/// Names in the order `localeCompare(…, {numeric: true})` gives: runs of digits compare as numbers, the rest without
/// regard to case.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (n, m) = (take(&mut x), take(&mut y));
                let (n0, m0) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let o = n0.len().cmp(&m0.len()).then_with(|| n0.cmp(m0));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            (Some(p), Some(q)) => {
                let o = p.to_lowercase().cmp(q.to_lowercase());
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
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
        edit::PickReq::Pictures { .. } | edit::PickReq::Cloud => {
            ed.eng.deliver_files(files.into_iter().map(|f| cc_engine::PickedFile { name: f.name, bytes: f.bytes }).collect());
        }
    }
}

/// Frees on another thread (desktop): what a big open leaves behind (a model file's JSON tree) without the wait.
#[cfg(not(target_arch = "wasm32"))]
static FREES: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>> = std::sync::Mutex::new(Vec::new());

/// `x` freed on another thread on the desktop (in place on the web).
pub(crate) fn free_aside<T: Send + 'static>(x: T) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut jobs = FREES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        jobs.retain(|h| !h.is_finished());
        if let Ok(h) = std::thread::Builder::new().name("cc-free".into()).spawn(move || drop(x)) {
            jobs.push(h);
        }
    }
    #[cfg(target_arch = "wasm32")]
    drop(x);
}

/// Waits for the frees [`free_aside`] handed to other threads (benchmarks: the next step timed alone, as in the
/// window, where frames pass before the next action).
pub fn settle_frees() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let jobs = std::mem::take(&mut *FREES.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        for h in jobs {
            let _ = h.join();
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

    /// M4.54: Settings › CadCraft and the Properties head (PICKADD, P.12's field list, Auto-hide), kept with Settings.
    #[test]
    fn cadcraft_settings_and_the_properties_head() {
        cc_lang::with(cc_lang::Locale::En, || {
            let mut app = UiApp::default();
            app.run("file.new", &Params::default()).unwrap();
            let run = |app: &mut UiApp, id: &str, name: &str| app.run(id, &Params { name: Some(name.into()), ..Default::default() });
            for t in ["LINE", "0,0", "10,0", "", "CIRCLE", "0,0", "3"] {
                run(&mut app, "cmd.line", t).unwrap();
            }
            // the PICKADD button is the Clicking objects setting
            assert!(!app.editor.as_ref().unwrap().hs.borrow().pick_replace);
            run(&mut app, "props.pickadd", "").unwrap();
            assert!(app.cadset.pick_replace && app.editor.as_ref().unwrap().hs.borrow().pick_replace);
            run(&mut app, "settings.set", "cad.pick=add").unwrap();
            assert!(!app.editor.as_ref().unwrap().hs.borrow().pick_replace);
            run(&mut app, "settings.set", "cad.paste=center").unwrap();
            run(&mut app, "settings.set", "cad.cursor=large").unwrap();
            assert!(app.editor.as_ref().unwrap().hs.borrow().paste_center);
            assert_eq!(app.editor.as_ref().unwrap().hs.borrow().pick_px, 8.0);
            assert!(run(&mut app, "settings.set", "cad.cursor=enormous").is_err());
            // LASSOMODE and the setting are one
            run(&mut app, "cmd.line", "LASSOMODE").unwrap();
            run(&mut app, "cmd.line", "0").unwrap();
            app.sync_editor();
            assert!(app.cadset.lasso_box, "LASSOMODE 0 = rectangle");
            run(&mut app, "settings.set", "cad.lasso=lasso").unwrap();
            assert!(!app.editor.as_ref().unwrap().hs.borrow().lasso_box);
            // P.12: nothing selected = no list; the line's fields, Linetype left out of the panel, shown in QP
            assert!(run(&mut app, "props.custom", "").is_err());
            let ids: Vec<u64> = app.editor.as_ref().unwrap().eng.doc().entities().filter_map(|e| e.id).collect();
            app.editor.as_ref().unwrap().hs.borrow_mut().selection = ids.clone();
            assert!(run(&mut app, "props.custom", "dim").is_err(), "no dimension is selected");
            run(&mut app, "props.custom", "line").unwrap();
            assert!(run(&mut app, "props.custom.set", "panel:nope=off").is_err());
            run(&mut app, "props.custom.set", "panel:lt=off").unwrap();
            run(&mut app, "props.custom.set", "qp:lt=on").unwrap();
            run(&mut app, "props.custom.ok", "").unwrap();
            assert_eq!(app.cadprops.hide.get("line"), Some(&vec!["lt".to_string()]));
            assert_eq!(app.cadprops.qp.get("line"), Some(&vec!["layer".into(), "color".into(), "lt".into(), "len".into(), "ang".into()]));
            run(&mut app, "props.custom", "all").unwrap();
            run(&mut app, "props.custom.reset", "").unwrap();
            assert!(!app.cadprops.hide.contains_key("all"));
            // Auto-hide: PROPERTIESCLOSE turns it on (props.js close()), PROPERTIES opens the strip's panel
            run(&mut app, "cmd.line", "PROPERTIESCLOSE").unwrap();
            app.sync_editor();
            assert!(app.cadprops.autohide && app.cadprops.open && !app.cadprops.flyout);
            run(&mut app, "cmd.line", "PROPERTIES").unwrap();
            app.sync_editor();
            assert!(app.cadprops.flyout);
            run(&mut app, "props.autohide", "off").unwrap();
            assert!(!app.cadprops.autohide);
            assert!(run(&mut app, "props.autohide", "maybe").is_err());
            // all of it kept with Settings
            run(&mut app, "props.autohide", "on").unwrap();
            let v = app.settings_json();
            let mut b = UiApp::default();
            b.load_settings(&v);
            assert_eq!(b.cadset.cursor, "large");
            assert!(b.cadset.paste_center && !b.cadset.pick_replace);
            assert!(b.cadprops.autohide);
            assert_eq!(b.cadprops.hide.get("line"), Some(&vec!["lt".to_string()]));
            b.load_settings(&serde_json::json!({"cadprops": {"hide": {"line": "lt", "x": [1, "a"]}, "autohide": "yes"}}));
            assert!(
                !b.cadprops.autohide && b.cadprops.hide.get("x") == Some(&vec!["a".to_string()]) && !b.cadprops.hide.contains_key("line")
            );
        });
    }

    /// M17.13: the Layers panel's rows (cadcraft.js renderLayers / setCurrent / the New layer form) and the status
    /// bar's Object snaps, as commands.
    #[test]
    fn layer_rows_and_snap_kinds() {
        cc_lang::with(cc_lang::Locale::En, || {
            let mut app = UiApp::default();
            app.run("file.new", &Params::default()).unwrap();
            let run = |app: &mut UiApp, id: &str, name: &str| app.run(id, &Params { name: Some(name.into()), ..Default::default() });
            run(&mut app, "layer.new", "Walls").unwrap();
            assert_eq!(app.message, "Added layer Walls and made it current.");
            assert_eq!(app.editor.as_ref().unwrap().eng.doc().current.layer, "Walls");
            run(&mut app, "layer.new", "Walls").unwrap();
            assert_eq!(app.message, "There is already a layer called Walls.");
            // the current layer is not turned off; another one is
            run(&mut app, "layer.flag", "Walls=on").unwrap();
            assert_eq!(app.message, "That is the current layer. Make another layer current first.");
            run(&mut app, "layer.current", "0").unwrap();
            assert_eq!(app.message, "Current layer: 0");
            for f in ["on", "frozen", "locked", "plot"] {
                run(&mut app, "layer.flag", &format!("Walls={f}")).unwrap();
            }
            {
                let d = app.editor.as_ref().unwrap().eng.doc();
                let l = d.layers.get("Walls").unwrap();
                assert!(!l.on && l.frozen && l.locked && !l.plot);
            }
            // one undo step each
            app.run("edit.undo", &Params::default()).unwrap();
            assert!(app.editor.as_ref().unwrap().eng.doc().layers.get("Walls").unwrap().plot);
            assert!(run(&mut app, "layer.flag", "Walls=color").is_err());
            assert!(run(&mut app, "layer.current", "Nope").is_err());
            // snap kinds: the default set, then one more and one fewer
            assert_eq!(app.editor.as_ref().unwrap().kinds, crate::edit::SNAP_KINDS.to_vec());
            run(&mut app, "edit.snapkind", "tan").unwrap();
            run(&mut app, "edit.snapkind", "end").unwrap();
            let k = &app.editor.as_ref().unwrap().kinds;
            assert!(k.contains(&"tan") && !k.contains(&"end"));
            assert!(run(&mut app, "edit.snapkind", "bogus").is_err());
            // kept between runs and carried to the next drawing (0.5's cadcraft.settings kinds / ortho / grid)
            app.run("edit.ortho", &Params::default()).unwrap();
            app.run("edit.grid", &Params::default()).unwrap();
            app.sync_editor();
            let v = app.settings_json();
            let kinds = v["cadcraft"]["kinds"].as_array().unwrap();
            assert!(kinds.iter().any(|k| k == "tan") && !kinds.iter().any(|k| k == "end"));
            assert_eq!((v["cadcraft"]["ortho"].as_bool(), v["cadcraft"]["grid"].as_bool()), (Some(true), Some(false)));
            let mut b = UiApp::default();
            b.load_settings(&v);
            b.run("file.new", &Params::default()).unwrap();
            let ed = b.editor.as_ref().unwrap();
            assert!(ed.kinds.contains(&"tan") && !ed.kinds.contains(&"end") && ed.ortho && !b.look.grid);
            // a damaged list keeps only the kinds it knows; none at all is the default set
            b.load_settings(&serde_json::json!({"cadcraft": {"kinds": ["mid", 3, "zap", "mid"], "polarInc": 400}}));
            assert_eq!((b.cadset.draft.kinds.clone(), b.cadset.draft.polar_inc), (vec!["mid"], 90.0));
            b.load_settings(&serde_json::json!({"cadcraft": {}}));
            assert_eq!(b.cadset.draft.kinds, crate::edit::SNAP_KINDS.to_vec());
        });
    }

    #[test]
    fn the_history_opens_as_cadcraft_js_prints_it() {
        cc_lang::with(cc_lang::Locale::En, || {
            let mut app = UiApp::default();
            app.run("file.new", &Params::default()).unwrap();
            let first = |app: &UiApp| app.editor.as_ref().unwrap().hs.borrow().log.front().cloned();
            let ready = "CadCraft is ready. Type a command (L for line, HELP for the list) or open a drawing.";
            assert_eq!(first(&app).as_deref(), Some(ready));
            // one history for the window: a new drawing keeps the lines (0.5's print() on one list)
            app.run("cmd.line", &Params { name: Some("LINE".into()), ..Default::default() }).unwrap();
            app.run("file.new", &Params::default()).unwrap();
            let log = app.editor.as_ref().unwrap().hs.borrow().log.clone();
            assert_eq!(log.iter().filter(|l| *l == ready).count(), 1);
            assert!(log.len() > 1);
        });
    }

    #[test]
    fn takeoff_snap_is_kept_per_viewer() {
        // takeoffcraft.js settings.snap / snap45: the defaults, then what was kept, then a damaged value
        let mut a = UiApp::default();
        assert_eq!(a.settings_json()["takeoffcraft"], serde_json::json!({"snap": true, "snap45": false}));
        a.load_settings(&serde_json::json!({"takeoffcraft": {"snap": false, "snap45": true}}));
        assert_eq!(a.tk_snap, (false, true));
        let v = a.settings_json();
        let mut b = UiApp::default();
        b.load_settings(&v);
        assert_eq!(b.tk_snap, (false, true));
        b.load_settings(&serde_json::json!({"takeoffcraft": {"snap": "no", "snap45": 1}}));
        assert_eq!(b.tk_snap, (true, false));
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
