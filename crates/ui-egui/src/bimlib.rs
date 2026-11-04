//! BimCraft's Library panel (library-view.js, docs/LIBRARY-SPEC.md §3.1): its sources — "This file" (the blocks of the
//! open DWG / DXF), "Files…" (a DWG, DXF, IFC, glTF / GLB, OBJ, RFA or CSV read here by `cc_bim::libitem`'s adapters,
//! with the file's units and a mesh's up axis), LibraryCraft and other feeds (getcadcraft.com's: this window says so) —
//! search and category filter, cards (a plan preview drawn from the item, name, maker / model, "From a file"), and a
//! detail view whose Place hands the item to the placing tool (bimplace.rs) when an IFC model is open.
//!
//! Library data is untrusted: names and notes are plain text, previews are drawn from the plan entities (never an
//! SVG from the file). Every control is a `bim.lib.*` command.

use std::sync::Arc;

use cc_bim::libitem::{self as li, category_label, dwg::Candidate};
use egui::{Color32, Pos2, RichText, Stroke};
use serde_json::Value as J;

use crate::bimplace::LibEntry;
use crate::bimtools::bt;
use crate::{ImportedFile, Params, UiApp};

/// The Library commands (id, label, shortcut, params doc).
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("bim.lib", "Library", "", "{name?: \"on\" | \"off\"} → the Library panel (toggles without a name)"),
    ("bim.lib.source", "Library source", "", "{name: \"librarycraft\" | \"this\" | \"files\" | \"feed\"}"),
    ("bim.lib.search", "Search the library", "", "{name: words} → cards whose name, block, maker or model have every word"),
    ("bim.lib.category", "Category", "", "{name: \"\" | a taxonomy key such as \"foodservice/sinks\"}"),
    ("bim.lib.units", "Units of the file", "", "{name: \"\" (as the file says) | \"mm\" | \"cm\" | \"m\" | \"in\" | \"ft\"}"),
    ("bim.lib.up", "Up axis of a mesh", "", "{name: \"\" (Y) | \"z\"}"),
    ("bim.lib.files", "Choose a file…", "", "{} → the file picker (DWG, DXF, IFC, glTF / GLB, OBJ, RFA, CSV)"),
    ("bim.lib.read", "Read a file into the library", "", "{name: path} → reads it from disk (desktop) as Files… does"),
    ("bim.lib.open", "Open a card", "", "{name: \"<n>\" (1 = the first card shown)} → its detail view"),
    ("bim.lib.back", "Back to the list", "", "{}"),
    ("bim.lib.place", "Place in the model", "", "{} → the detail view's item starts placing on the plan (an IFC model must be open)"),
    (
        "bim.lib.swap",
        "Swap to this",
        "",
        "{} → the placed item picked with Swap item… becomes the detail view's item (place, number, quantity, notes kept)",
    ),
    ("bim.lib.page", "Next page", "", "{} → LibraryCraft / a feed: the next page of cards"),
    ("bim.lib.feed.add", "Add a feed", "", "{name: \"https://…/api/v1/library\"} → kept first in the list and picked (web only)"),
    ("bim.lib.feed", "Pick a feed", "", "{name: an added feed's address}"),
    ("bim.lib.feed.remove", "Remove a feed", "", "{name: an added feed's address}"),
];

/// LibraryCraft or a feed (the server searches)?
fn api_source(lib: &Lib) -> bool {
    matches!(lib.source.as_str(), "librarycraft" | "feed")
}

/// A card (`entries[]`).
#[derive(Clone, Debug, Default)]
pub struct Card {
    pub key: String,
    pub name: String,
    pub sub: String,
    pub maker: String,
    pub model: String,
    pub category: String,
    /// Files…: the item itself (and its file's notes about it)
    pub item: Option<Arc<J>>,
    pub notes: Vec<String>,
    /// This file: the block to make it from
    pub block: Option<String>,
    /// LibraryCraft / a feed: the item's uuid, its licence, version, withdrawn, team-only, the API base
    pub uuid: String,
    pub license: String,
    pub version: Option<f64>,
    pub withdrawn: bool,
    pub team: bool,
    pub base: String,
}

/// Where an item came from (`meta`).
#[derive(Clone, Debug, Default)]
pub struct Meta {
    pub library: String,
    pub source: String,
    pub source_file: String,
    pub sha256: String,
    pub notes: Vec<String>,
    /// LibraryCraft: the item's page name, and whether this version was withdrawn (and why)
    pub slug: String,
    pub withdrawn: bool,
    pub withdrawn_reason: String,
}

/// The detail view.
#[derive(Clone, Debug)]
pub struct Detail {
    pub card: Card,
    pub item: Option<Arc<J>>,
    pub meta: Meta,
    pub error: Option<String>,
    /// the item is being read from the library
    pub loading: bool,
}

/// The panel's state.
#[derive(Clone, Debug)]
pub struct Lib {
    pub open: bool,
    pub source: String,
    pub query: String,
    pub category: String,
    pub units: String,
    pub up: String,
    pub file_cards: Vec<Card>,
    pub file_notes: Vec<String>,
    /// the file Files… read last
    pub file_name: String,
    /// This file: the drawing it lists (name, bytes) and its cards and notes
    pub this_of: Option<(String, usize)>,
    pub this_cards: Vec<Card>,
    pub this_notes: Vec<String>,
    pub this_model: Option<Arc<cc_io_dwg::Obj>>,
    pub detail: Option<Detail>,
    pub reading: bool,
    pub error: Option<String>,
    /// LibraryCraft / feeds (bimlibnet.rs): the cards of this page, how many there are, the page, loading
    pub api_cards: Vec<Card>,
    pub total: f64,
    pub page: u32,
    pub loading: bool,
    /// the feed picked, the feeds added (newest first), the address being typed
    pub feed: Option<String>,
    pub feeds: Vec<String>,
    pub feed_input: String,
    /// requests waiting for their answers, the newest list request's number, a search to run after typing pauses
    pub want: Vec<crate::bimlibnet::Want>,
    pub load_seq: u64,
    /// words typed in the search field, searched for once typing pauses (300 ms, as 0.5)
    pub search_due: Option<(String, f64)>,
}
impl Default for Lib {
    fn default() -> Self {
        Lib {
            open: false,
            source: "files".into(),
            query: String::new(),
            category: String::new(),
            units: String::new(),
            up: String::new(),
            file_cards: Vec::new(),
            file_notes: Vec::new(),
            file_name: String::new(),
            this_of: None,
            this_cards: Vec::new(),
            this_notes: Vec::new(),
            this_model: None,
            detail: None,
            reading: false,
            error: None,
            api_cards: Vec::new(),
            total: 0.0,
            page: 1,
            loading: false,
            feed: None,
            feeds: Vec::new(),
            feed_input: String::new(),
            want: Vec::new(),
            load_seq: 0,
            search_due: None,
        }
    }
}

/// `host.mode()`: 'ifc' (an IFC model is open), 'dwg' (a drawing), 'none'.
fn mode(app: &UiApp) -> &'static str {
    if app.ifc3_bytes().is_some() {
        "ifc"
    } else if app.doc.is_some() && app.source.as_ref().is_some_and(|f| is_drawing(&f.name, &f.bytes)) {
        "dwg"
    } else {
        "none"
    }
}
fn is_drawing(name: &str, bytes: &[u8]) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".dwg") || l.ends_with(".dxf") || bytes.starts_with(b"AC10")
}
fn read_model(name: &str, bytes: &[u8]) -> cc_io_dwg::Obj {
    let dwg = bytes.starts_with(b"AC10") || name.to_ascii_lowercase().ends_with(".dwg");
    if dwg {
        cc_io_dwg::read_dwg(bytes, &cc_io_dwg::ReadOpts::default())
    } else {
        cc_io_dwg::dxf_model::read_dxf_model(&cc_io_dwg::dxf_model::decode_dxf_bytes(bytes))
    }
}

fn s_of(item: &J, k: &str) -> String {
    item.get(k).and_then(J::as_str).unwrap_or("").to_string()
}

/// `dwgEntries(db, fileName)` for the open drawing (read again only when another drawing is open).
fn refresh_this(app: &mut UiApp) {
    if mode(app) != "dwg" {
        app.bimh.lib.this_cards.clear();
        app.bimh.lib.this_of = None;
        app.bimh.lib.this_model = None;
        return;
    }
    let Some(src) = app.source.clone() else { return };
    let tag = (src.name.clone(), src.bytes.len());
    if app.bimh.lib.this_of.as_ref() == Some(&tag) {
        return;
    }
    let db = Arc::new(read_model(&src.name, &src.bytes));
    let lib = &mut app.bimh.lib;
    lib.this_of = Some(tag);
    lib.this_cards.clear();
    lib.this_notes.clear();
    lib.error = None;
    match li::dwg::items_from_dwg(&db, None, None, &src.name, true) {
        Ok((r, cands)) => {
            lib.this_notes = r.notes;
            lib.this_cards = cands.iter().filter(|c| c.entities > 0).map(this_card).collect();
        }
        Err(e) => lib.error = Some(e),
    }
    lib.this_model = Some(db);
}
fn this_card(c: &Candidate) -> Card {
    let a = |k: &str| c.attrs.iter().find(|(x, _)| x == k).map_or(String::new(), |(_, v)| v.clone());
    let cat = a("KCL-CATEGORY");
    let mfr = {
        let x = a("KCL-MFR-SCHDNAME");
        if x.is_empty() { a("KCL-MFR") } else { x }
    };
    Card {
        key: format!("this:{}", c.key),
        name: if cat.is_empty() { c.name.clone() } else { cat },
        sub: c.name.clone(),
        maker: mfr,
        model: a("KCL-MODEL"),
        category: String::new(),
        item: None,
        notes: Vec::new(),
        block: Some(c.name.clone()),
        ..Card::default()
    }
}

/// `readFile(file)`: Files… through the adapters → the cards; Err = the status line's error.
pub fn read_file(app: &mut UiApp, f: &ImportedFile) -> Result<(), String> {
    let ext = f.name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let head = &f.bytes.get(..16).unwrap_or(&f.bytes);
    let lib = &app.bimh.lib;
    let units = (!lib.units.is_empty()).then(|| lib.units.clone());
    let up = (!lib.up.is_empty()).then(|| lib.up.clone());
    let base = f.name.rsplit_once('.').map_or(f.name.as_str(), |(b, _)| b).to_string();
    let r: li::Read = if head.starts_with(b"AC10") || ext == "dwg" || ext == "dxf" {
        let db = read_model(&f.name, &f.bytes);
        if db.obj("report").is_some_and(|r| r.get("error").is_some()) && !db.arr("blocks").is_some_and(|b| !b.is_empty()) {
            return Err(bt("lib.err.read_here", &[("file", &f.name)]));
        }
        li::dwg::items_from_dwg(&db, units.as_deref(), None, &f.name, false)?.0
    } else if head.starts_with(b"ISO-10303-21") || ext == "ifc" {
        li::ifc::items_from_ifc(Arc::from(f.bytes.as_slice()), false, None).map_err(|_| bt("lib.err.read_here", &[("file", &f.name)]))?.0
    } else if ext == "glb" || ext == "gltf" || head.starts_with(b"glTF") {
        li::mesh::item_from_mesh(&f.bytes, if ext == "gltf" { "gltf" } else { "glb" }, units.as_deref(), up.as_deref(), &base)
    } else if ext == "obj" {
        li::mesh::item_from_mesh(&f.bytes, "obj", units.as_deref(), up.as_deref(), &base)
    } else if ext == "rfa" || f.bytes.starts_with(&[0xd0, 0xcf]) {
        let fam = li::rfa::read_revit_family(&f.bytes)?;
        li::rfa::item_from_rfa(fam.as_ref(), &f.name)
    } else if ext == "csv" {
        li::items_from_csv(&String::from_utf8_lossy(&f.bytes), None, None)
    } else {
        return Err(bt("lib.err.unsupported", &[("file", &f.name)]));
    };
    let notes = r.notes.clone();
    let cards: Vec<Card> = r
        .items
        .iter()
        .enumerate()
        .map(|(i, raw)| {
            let item = li::clean_item(raw);
            let raw_name = s_of(raw, "name");
            Card {
                key: format!("file:{i}:{}", s_of(&item, "name")),
                name: s_of(&item, "name"),
                sub: String::new(),
                maker: s_of(&item, "maker"),
                model: s_of(&item, "model"),
                category: s_of(&item, "category"),
                notes: notes.iter().filter(|n| n.starts_with(&raw_name) || !n.contains(": ")).cloned().collect(),
                item: Some(Arc::new(item)),
                block: None,
                ..Card::default()
            }
        })
        .collect();
    let n = cards.len();
    let lib = &mut app.bimh.lib;
    lib.file_notes = notes;
    lib.file_cards = cards;
    lib.file_name = f.name.clone();
    lib.source = "files".into();
    lib.detail = None;
    lib.error = None;
    app.message = bt("lib.read_file", &[("file", &f.name), ("n", &n.to_string())]);
    Ok(())
}

/// `filtered()`: the cards of this source the search words and category let through.
fn shown(lib: &Lib) -> Vec<&Card> {
    let list: &[Card] = match lib.source.as_str() {
        "files" => &lib.file_cards,
        "this" => &lib.this_cards,
        // LibraryCraft and feeds search on the server: their cards as they came
        _ => return lib.api_cards.iter().take(300).collect(),
    };
    let words: Vec<String> = lib.query.to_lowercase().split_whitespace().map(str::to_string).collect();
    list.iter()
        .filter(|e| lib.category.is_empty() || e.category == lib.category)
        .filter(|e| {
            let hay = [e.name.as_str(), e.sub.as_str(), e.maker.as_str(), e.model.as_str()].join(" ").to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .take(300)
        .collect()
}

/// `showDetail(entry)`: the card's item (a block is made into one now).
fn open_card(app: &mut UiApp, card: Card) {
    if !card.uuid.is_empty() && card.item.is_none() {
        crate::bimlibnet::open_card(app, card);
        return;
    }
    let mut d = Detail { card: card.clone(), item: None, meta: Meta::default(), error: None, loading: false };
    if let Some(item) = &card.item {
        let file = app.bimh.lib.file_name.clone();
        d.meta = Meta {
            library: "file".into(),
            source: file.clone(),
            source_file: file,
            sha256: li::item_hash(item),
            notes: card.notes.clone(),
            ..Meta::default()
        };
        d.item = Some(item.clone());
    } else if let (Some(block), Some(db)) = (&card.block, app.bimh.lib.this_model.clone()) {
        let fname = app.bimh.lib.this_of.as_ref().map(|t| t.0.clone()).unwrap_or_default();
        match li::dwg::item_from_block(&db, block, None, &fname, None) {
            Ok((Some(raw), notes)) => {
                let item = li::clean_item(&raw);
                d.meta = Meta {
                    library: "file".into(),
                    source: bt("lib.source.drawing", &[]),
                    source_file: fname,
                    sha256: li::item_hash(&item),
                    notes,
                    ..Meta::default()
                };
                d.item = Some(Arc::new(item));
            }
            Ok((None, notes)) => d.error = Some(notes.join(" ")),
            Err(e) => d.error = Some(e),
        }
    }
    app.bimh.lib.detail = Some(d);
}

/// Run a Library command.
pub fn run(app: &mut UiApp, id: &str, p: &Params) -> Result<(), String> {
    let name = p.name.clone().unwrap_or_default();
    match id {
        "bim.lib" => {
            app.bimh.lib.open = match name.as_str() {
                "on" => true,
                "off" => false,
                _ => !app.bimh.lib.open,
            };
            refresh_this(app);
        }
        "bim.lib.source" => {
            if !matches!(name.as_str(), "librarycraft" | "this" | "files" | "feed") {
                return Err("bim.lib.source: librarycraft | this | files | feed".into());
            }
            let api = matches!(name.as_str(), "librarycraft" | "feed");
            app.bimh.lib.source = name;
            app.bimh.lib.detail = None;
            app.bimh.lib.open = true;
            app.bimh.lib.page = 1;
            refresh_this(app);
            if api {
                crate::bimlibnet::load(app);
            }
        }
        "bim.lib.search" => {
            app.bimh.lib.query = cc_bim::js::trim(&name).to_string();
            app.bimh.lib.search_due = None;
            if api_source(&app.bimh.lib) {
                app.bimh.lib.page = 1;
                crate::bimlibnet::load(app);
            }
        }
        "bim.lib.category" => {
            if !name.is_empty() && category_label(&name).is_none() {
                return Err(format!("bim.lib.category: no category \"{name}\""));
            }
            app.bimh.lib.category = name;
            if api_source(&app.bimh.lib) {
                app.bimh.lib.page = 1;
                crate::bimlibnet::load(app);
            }
        }
        "bim.lib.page" => {
            if api_source(&app.bimh.lib) {
                app.bimh.lib.page = app.bimh.lib.page.saturating_add(1);
                crate::bimlibnet::load(app);
            }
        }
        "bim.lib.feed.add" => crate::bimlibnet::add_feed(app, &name)?,
        "bim.lib.feed" => {
            if !app.bimh.lib.feeds.contains(&name) {
                return Err(format!("bim.lib.feed: no feed {name}"));
            }
            app.bimh.lib.feed = Some(name);
            app.bimh.lib.page = 1;
            crate::bimlibnet::load(app);
        }
        "bim.lib.feed.remove" => {
            let lib = &mut app.bimh.lib;
            lib.feeds.retain(|f| *f != name);
            if lib.feed.as_deref() == Some(name.as_str()) {
                lib.feed = None;
                lib.api_cards.clear();
            }
        }
        "bim.lib.units" => {
            if !matches!(name.as_str(), "" | "mm" | "cm" | "m" | "in" | "ft") {
                return Err("bim.lib.units: \"\" | mm | cm | m | in | ft".into());
            }
            app.bimh.lib.units = name;
        }
        "bim.lib.up" => {
            if !matches!(name.as_str(), "" | "z") {
                return Err("bim.lib.up: \"\" | z".into());
            }
            app.bimh.lib.up = name;
        }
        "bim.lib.files" => {
            let pick = app.services.pick_files.as_mut().ok_or("bim.lib.files: this window cannot open files")?;
            match pick("library", false) {
                Some(files) => {
                    if let Some(f) = files.into_iter().next() {
                        read_file(app, &f)?;
                    }
                }
                None => app.bimh.lib.reading = true,
            }
        }
        "bim.lib.read" => {
            let read = app.services.read_path.as_mut().ok_or("bim.lib.read: this window cannot read files by path")?;
            let f = read(&name)?;
            app.bimh.lib.open = true;
            read_file(app, &f)?;
        }
        "bim.lib.open" => {
            let n: usize = name.parse().map_err(|_| "bim.lib.open: name = the card's number (1 = first)".to_string())?;
            let card =
                shown(&app.bimh.lib).get(n.saturating_sub(1)).map(|c| (*c).clone()).ok_or(format!("bim.lib.open: there is no card {n}"))?;
            open_card(app, card);
        }
        "bim.lib.back" => app.bimh.lib.detail = None,
        "bim.lib.place" => {
            let d = app.bimh.lib.detail.clone().ok_or("bim.lib.place: no item is open")?;
            let item = d.item.ok_or("bim.lib.place: no item is open")?;
            if mode(app) != "ifc" {
                return Err(bt("lib.needs_ifc", &[]));
            }
            crate::bimedit::doc(app)?;
            let entry = LibEntry {
                name: s_of(&item, "name"),
                item,
                library: d.meta.library.clone(),
                sha256: d.meta.sha256.clone(),
                source_file: d.meta.source_file.clone(),
            };
            crate::bimplace::start(app, entry);
        }
        "bim.lib.swap" => {
            let d = app.bimh.lib.detail.clone().ok_or("bim.lib.swap: no item is open")?;
            let item = d.item.ok_or("bim.lib.swap: no item is open")?;
            if mode(app) != "ifc" {
                return Err(bt("lib.needs_ifc", &[]));
            }
            let entry = LibEntry {
                name: s_of(&item, "name"),
                item,
                library: d.meta.library.clone(),
                sha256: d.meta.sha256.clone(),
                source_file: d.meta.source_file.clone(),
            };
            crate::bimclip::do_swap(app, entry)?;
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// web: the file the browser's picker read for Files…
pub fn sync(app: &mut UiApp) {
    if app.bimh.lib.reading
        && let Some(files) = app.services.picked.as_ref().and_then(|p| p.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take())
    {
        app.bimh.lib.reading = false;
        if let Some(f) = files.into_iter().next()
            && let Err(e) = read_file(app, &f)
        {
            app.error = Some(e);
        }
    }
    crate::bimlibnet::sync(app);
}

/// The item's plan in a rectangle (the card preview, the detail's big one): drawn from its entities.
fn preview(ui: &mut egui::Ui, item: &J, size: f32, ink: Color32, bg: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, bg);
    let plan = item.get("plan");
    let Some(b) = li::plan_bounds(plan) else { return };
    let (w, h) = ((b[2] - b[0]).max(1e-6), (b[3] - b[1]).max(1e-6));
    let inner = rect.shrink(size * 0.1);
    let s = f64::from(inner.width()).min(f64::from(inner.height())) / w.max(h);
    let ox = f64::from(inner.center().x) - (b[0] + w / 2.0) * s;
    let oy = f64::from(inner.center().y) + (b[1] + h / 2.0) * s;
    #[allow(clippy::cast_possible_truncation)]
    let to = |p: [f64; 2]| Pos2::new((ox + p[0] * s) as f32, (oy - p[1] * s) as f32);
    let stroke = Stroke::new(1.0, ink);
    for e in plan.and_then(|p| p.get("entities")).and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().take(5000) {
        let Some(o) = e.as_object() else { continue };
        let ty = o.get("type").and_then(J::as_str).unwrap_or("");
        if ty == "text" {
            continue;
        }
        let pts = li::entity_points(o);
        if ty == "circle" {
            let (x, y, r) = (o.get("x").and_then(J::as_f64), o.get("y").and_then(J::as_f64), o.get("r").and_then(J::as_f64));
            if let (Some(x), Some(y), Some(r)) = (x, y, r) {
                #[allow(clippy::cast_possible_truncation)]
                painter.circle_stroke(to([x, y]), (r * s) as f32, stroke);
            }
            continue;
        }
        if ty == "point" {
            if let Some(p) = pts.first() {
                painter.circle_filled(to(*p), 1.5, ink);
            }
            continue;
        }
        let closed = ty == "hatch" || (ty == "poly" && o.get("closed") == Some(&J::Bool(true)));
        let mut line: Vec<Pos2> = pts.iter().filter(|p| p[0].is_finite() && p[1].is_finite()).map(|p| to(*p)).collect();
        if closed && let Some(f) = line.first().copied() {
            line.push(f);
        }
        if line.len() >= 2 {
            painter.add(egui::Shape::line(line, stroke));
        }
    }
}

/// `licenseLabel(id)`: Creative Commons names as they are, the other two as words, else the id, else "No license".
pub fn license_label(id: &str) -> String {
    match id {
        "CC0-1.0" => "CC0".into(),
        "CC-BY-4.0" => "CC BY".into(),
        "CC-BY-SA-4.0" => "CC BY-SA".into(),
        "MAKER-TERMS" => bt("lib.license.maker", &[]),
        "TEAM-ONLY" => bt("lib.license.team", &[]),
        "" => bt("lib.license.none", &[]),
        other => other.to_string(),
    }
}

/// A small pill (`bc-pill`): plain, info (the action colour) or bad (the warning colour).
fn pill(ui: &mut egui::Ui, t: &crate::theme::Tokens, text: &str, kind: u8) {
    let c = match kind {
        1 => t.accent,
        2 => ui.visuals().error_fg_color,
        _ => t.text_dim,
    };
    egui::Frame::new().stroke(Stroke::new(1.0, c)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(6, 0)).show(ui, |ui| {
        ui.label(RichText::new(text).small().color(c));
    });
}

/// The Library window.
pub fn window(app: &mut UiApp, ctx: &egui::Context) {
    if !app.bimh.lib.open {
        return;
    }
    if app.bimh.lib.search_due.is_some() || app.bimh.lib.loading {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    let t = app.tokens;
    let lib = app.bimh.lib.clone();
    let mode = mode(app);
    let swap = app.bimh.swap_target.clone();
    let mut open = true;
    let mut act: Vec<(&'static str, String)> = Vec::new();
    let mut typed: Option<String> = None;
    let mut feed_typed: Option<String> = None;
    crate::shell::Dialog::new(bt("ui.library", &[]))
        .modeless()
        .id(egui::Id::unique("bim_library"))
        .open(&mut open)
        .resizable(true)
        // 0.5 keeps it in the right-hand pane: it opens at the right, under the toolbar
        .pivot(egui::Align2::RIGHT_TOP)
        .default_pos([ctx.content_rect().right() - 16.0, 140.0])
        .default_size([380.0, 520.0])
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (k, label, tip) in [
                    ("librarycraft", "LibraryCraft".to_string(), bt("lib.src.librarycraft.title", &[])),
                    ("this", bt("lib.src.this", &[]), bt("lib.src.this.title", &[])),
                    ("files", bt("lib.src.files", &[]), bt("lib.src.files.title", &[])),
                    ("feed", bt("lib.src.feed", &[]), bt("lib.src.feed.title", &[])),
                ] {
                    if crate::shell::tab(ui, &t, &label, lib.source == k).on_hover_text(tip).clicked() {
                        act.push(("bim.lib.source", k.into()));
                    }
                }
            });
            ui.separator();
            match lib.source.as_str() {
                "feed" => {
                    ui.label(RichText::new(bt("lib.feed.intro", &[("uuid", "{uuid}")])).color(t.text_dim).small());
                    ui.horizontal(|ui| {
                        let mut f = lib.feed_input.clone();
                        let r = ui.add(crate::shell::field(&mut f).hint_text("https://example.org/api/v1/library").desired_width(230.0));
                        if r.changed() {
                            feed_typed = Some(f.clone());
                        }
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if ui.add(crate::shell::Btn::new(bt("lib.feed.add", &[]))).clicked() || enter {
                            act.push(("bim.lib.feed.add", f));
                        }
                    });
                    for f in &lib.feeds {
                        ui.horizontal(|ui| {
                            let on = lib.feed.as_deref() == Some(f.as_str());
                            if crate::shell::link(ui, &t, "", f).clicked() && !on {
                                act.push(("bim.lib.feed", f.clone()));
                            }
                            if on {
                                ui.label(RichText::new("●").small().color(t.accent));
                            }
                            if ui.small_button("✕").on_hover_text(bt("lib.feed.remove", &[("feed", f)])).clicked() {
                                act.push(("bim.lib.feed.remove", f.clone()));
                            }
                        });
                    }
                }
                "files" => {
                    ui.horizontal(|ui| {
                        if ui.add(crate::shell::Btn::new(bt("lib.files.choose", &[]))).clicked() {
                            act.push(("bim.lib.files", String::new()));
                        }
                    });
                    ui.label(RichText::new(bt("lib.files.intro", &[])).color(t.text_dim).small());
                    ui.horizontal(|ui| {
                        let units = [
                            ("", "lib.files.units.file"),
                            ("mm", "lib.files.units.mm"),
                            ("cm", "lib.files.units.cm"),
                            ("m", "lib.files.units.m"),
                            ("in", "lib.files.units.in"),
                            ("ft", "lib.files.units.ft"),
                        ];
                        let cur = units.iter().find(|u| u.0 == lib.units).map_or(String::new(), |u| bt(u.1, &[]));
                        egui::ComboBox::new("bim_lib_units", "").selected_text(cur).show_ui(ui, |ui| {
                            for (k, key) in units {
                                if ui.selectable_label(lib.units == k, bt(key, &[])).clicked() {
                                    act.push(("bim.lib.units", k.into()));
                                }
                            }
                        });
                        let ups = [("", "lib.files.up.y"), ("z", "lib.files.up.z")];
                        let cur = ups.iter().find(|u| u.0 == lib.up).map_or(String::new(), |u| bt(u.1, &[]));
                        egui::ComboBox::new("bim_lib_up", "").selected_text(cur).show_ui(ui, |ui| {
                            for (k, key) in ups {
                                if ui.selectable_label(lib.up == k, bt(key, &[])).clicked() {
                                    act.push(("bim.lib.up", k.into()));
                                }
                            }
                        });
                    });
                }
                "this" => {
                    ui.label(
                        RichText::new(bt(if mode == "dwg" { "lib.this.intro" } else { "lib.this.needs_dwg" }, &[]))
                            .color(t.text_dim)
                            .small(),
                    );
                }
                _ => {}
            }
            if let Some(d) = &lib.detail {
                detail(ui, &t, d, mode, swap.as_ref().map(|s| s.1.as_str()), &mut act);
                return;
            }
            ui.horizontal(|ui| {
                let mut q = lib.search_due.as_ref().map_or_else(|| lib.query.clone(), |(q, _)| q.clone());
                let r = ui.add(crate::shell::field(&mut q).hint_text(bt("lib.search.placeholder", &[])).desired_width(170.0));
                if r.changed() {
                    typed = Some(q);
                }
                let cur = if lib.category.is_empty() { bt("lib.category.any", &[]) } else { cat_text(&lib.category) };
                egui::ComboBox::new("bim_lib_cat", "").selected_text(cur).width(150.0).show_ui(ui, |ui| {
                    if ui.selectable_label(lib.category.is_empty(), bt("lib.category.any", &[])).clicked() {
                        act.push(("bim.lib.category", String::new()));
                    }
                    for k in li::CATEGORY_KEYS {
                        if ui.selectable_label(lib.category == k, cat_text(k)).clicked() {
                            act.push(("bim.lib.category", k.into()));
                        }
                    }
                });
            });
            let cards = shown(&lib);
            if lib.loading {
                ui.label(RichText::new(bt("lib.loading", &[])).color(t.text_dim));
            } else if let Some(e) = &lib.error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            } else if cards.is_empty() {
                let empty = match lib.source.as_str() {
                    "files" => bt("lib.empty.files", &[]),
                    "feed" if lib.feed.is_some() => bt("lib.empty.feed", &[]),
                    "feed" => bt("lib.empty.nofeed", &[]),
                    "this" if mode == "dwg" => bt("lib.empty.noblocks", &[]),
                    "this" => String::new(),
                    _ if !lib.query.is_empty() || !lib.category.is_empty() => bt("lib.empty.nomatch", &[]),
                    _ => bt("lib.empty.library", &[]),
                };
                if !empty.is_empty() {
                    ui.label(RichText::new(empty).color(t.text_dim));
                }
            }
            egui::ScrollArea::vertical().max_height(330.0).auto_shrink([false, true]).show(ui, |ui| {
                for (i, c) in cards.iter().enumerate() {
                    let r = egui::Frame::new().stroke(Stroke::new(1.0, t.divider)).inner_margin(4.0).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            match &c.item {
                                Some(it) => preview(ui, it, 56.0, t.text, t.panel_darker),
                                None => {
                                    // library-view.js `bc-lib-noprev`: a box icon (a block's item is made when it opens)
                                    let (rect, _) = ui.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::hover());
                                    ui.painter().rect_filled(rect, 2.0, t.panel_darker);
                                    let r = egui::Rect::from_center_size(rect.center(), egui::vec2(22.0, 22.0));
                                    crate::ribbon::paint_icon(ui.painter(), r, "box", t.text_dim);
                                }
                            }
                            ui.vertical(|ui| {
                                let name = if c.name.is_empty() { bt("clip.unnamed", &[]) } else { c.name.clone() };
                                ui.label(RichText::new(name).strong().color(t.text_strong));
                                if !c.sub.is_empty() && c.sub != c.name {
                                    ui.label(RichText::new(&c.sub).small().color(t.text_dim));
                                }
                                let mm = [c.maker.as_str(), c.model.as_str()]
                                    .iter()
                                    .filter(|x| !x.is_empty())
                                    .copied()
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                if !mm.is_empty() {
                                    ui.label(RichText::new(mm).small().color(t.text_dim));
                                }
                                ui.horizontal_wrapped(|ui| {
                                    if c.uuid.is_empty() {
                                        pill(ui, &t, &bt("lib.card.fromfile", &[]), 1);
                                    } else {
                                        pill(ui, &t, &license_label(&c.license), 0);
                                    }
                                    if let Some(v) = c.version.filter(|v| *v != 0.0) {
                                        pill(ui, &t, &bt("lib.card.version", &[("version", &cc_bim::js::num_str(v))]), 1);
                                    }
                                    if c.team {
                                        pill(ui, &t, &bt("lib.card.team", &[]), 1);
                                    }
                                    if c.withdrawn {
                                        pill(ui, &t, &bt("lib.card.withdrawn", &[]), 2);
                                    }
                                });
                            });
                        });
                    });
                    if r.response.interact(egui::Sense::click()).clicked() {
                        act.push(("bim.lib.open", (i + 1).to_string()));
                    }
                }
            });
            if api_source(&lib)
                && lib.total > f64::from(lib.page) * 24.0
                && ui.add(crate::shell::Btn::new(bt("lib.next_page", &[]))).clicked()
            {
                act.push(("bim.lib.page", String::new()));
            }
            let notes = if lib.source == "files" { &lib.file_notes } else { &lib.this_notes };
            if matches!(lib.source.as_str(), "files" | "this") && !notes.is_empty() {
                egui::CollapsingHeader::new(bt("lib.notes.file", &[("n", &notes.len().to_string())])).id_salt("bim_lib_notes").show(
                    ui,
                    |ui| {
                        for n in notes.iter().take(200) {
                            ui.label(RichText::new(n).small());
                        }
                    },
                );
            }
        });
    if !open {
        app.bimh.lib.open = false;
    }
    if let Some(q) = typed {
        // the search runs once typing pauses (300 ms), as 0.5's
        app.bimh.lib.search_due = Some((q, cc_clock::now_ms() + 300.0));
    }
    if let Some(f) = feed_typed {
        app.bimh.lib.feed_input = f;
    }
    for (id, name) in act {
        let p = Params { name: Some(name), ..Default::default() };
        app.run_ui(id, &p);
    }
}

fn cat_text(k: &str) -> String {
    let g = k.split('/').next().unwrap_or("");
    let mut c = g.chars();
    let group = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
    match category_label(k) {
        Some(l) => bt("lib.category.option", &[("group", &group), ("label", &l)]),
        None => k.to_string(),
    }
}

fn detail(ui: &mut egui::Ui, t: &crate::theme::Tokens, d: &Detail, mode: &str, swap: Option<&str>, act: &mut Vec<(&'static str, String)>) {
    if crate::shell::link(ui, t, "", &format!("← {}", bt("lib.back", &[]))).clicked() {
        act.push(("bim.lib.back", String::new()));
    }
    if d.loading {
        ui.label(RichText::new(bt("lib.detail.reading", &[])).color(t.text_dim));
        return;
    }
    if let Some(e) = &d.error {
        ui.colored_label(ui.visuals().error_fg_color, e);
        return;
    }
    let Some(it) = &d.item else { return };
    preview(ui, it, 200.0, t.text, t.panel_darker);
    ui.label(RichText::new(s_of(it, "name")).strong().color(t.text_strong));
    let withdrawn = d.meta.withdrawn || d.card.withdrawn;
    if withdrawn {
        let w = if d.meta.withdrawn_reason.is_empty() {
            bt("lib.withdrawn.default", &[])
        } else {
            bt("lib.withdrawn.reason", &[("reason", &d.meta.withdrawn_reason)])
        };
        ui.colored_label(ui.visuals().error_fg_color, w);
    }
    let p = it.get("params");
    let fmt = |k: &str| {
        p.and_then(|p| p.get(k))
            .and_then(J::as_f64)
            .filter(|v| *v > 0.0)
            .map_or(String::new(), |v| format!("{} mm", cc_bim::js::num_str(cc_bim::draw::m::round(v))))
    };
    let join = |k: &str, up: bool| {
        p.and_then(|p| p.get(k)).and_then(J::as_object).map_or(String::new(), |o| {
            o.iter()
                .map(|(a, b)| format!("{} {}", if up { a.to_uppercase() } else { a.clone() }, b.as_str().unwrap_or("")))
                .collect::<Vec<_>>()
                .join(", ")
        })
    };
    let body = match it.get("body") {
        Some(b) if b.get("type").and_then(J::as_str) == Some("box") => bt("lib.body.box", &[]),
        Some(b) if b.is_object() => {
            let n = b.get("indices").and_then(J::as_array).map_or(0, |a| a.len() / 3);
            bt("lib.body.mesh", &[("n", &n.to_string())])
        }
        _ => bt("lib.body.none", &[]),
    };
    let cat = s_of(it, "category");
    let version = it.get("version").and_then(J::as_f64).filter(|v| *v != 0.0).map_or(String::new(), cc_bim::js::num_str);
    let rows = [
        (bt("lib.row.category", &[]), if category_label(&cat).is_some() { cat_text(&cat) } else { cat.clone() }),
        (bt("lib.row.kind", &[]), s_of(it, "kind")),
        (bt("lib.row.maker", &[]), s_of(it, "maker")),
        (bt("lib.row.model", &[]), s_of(it, "model")),
        (bt("lib.row.width", &[]), fmt("width")),
        (bt("lib.row.depth", &[]), fmt("depth")),
        (bt("lib.row.height", &[]), fmt("height")),
        (bt("lib.row.elec", &[]), join("elec", false)),
        (bt("lib.row.plumb", &[]), join("plumb", true)),
        (bt("lib.row.remarks", &[]), p.and_then(|p| p.get("remarks")).and_then(J::as_str).unwrap_or("").to_string()),
        (bt("lib.row.body", &[]), body),
        (bt("lib.row.version", &[]), version),
        (bt("lib.row.from", &[]), d.meta.source.clone()),
    ];
    egui::Grid::new("bim_lib_rows").num_columns(2).show(ui, |ui| {
        for (k, v) in rows.iter().filter(|(_, v)| !v.is_empty()) {
            ui.label(RichText::new(k).color(t.text_dim));
            ui.label(v);
            ui.end_row();
        }
    });
    ui.label(RichText::new(bt("lib.license.heading", &[])).strong());
    match it.get("license").filter(|l| l.is_object()) {
        Some(l) => {
            let g = |k: &str| l.get(k).and_then(J::as_str).unwrap_or("").to_string();
            ui.horizontal_wrapped(|ui| {
                pill(ui, t, &license_label(&g("id")), 0);
                let who = if g("attribution").is_empty() { g("holder") } else { g("attribution") };
                ui.label(who);
            });
            let url = g("url");
            if url.starts_with("https://") || url.starts_with("http://") {
                ui.hyperlink_to(bt("lib.license.link", &[]), url);
            }
        }
        None => {
            ui.label(RichText::new(bt("lib.license.fromfile", &[])).color(t.text_dim).small());
        }
    }
    if !d.meta.notes.is_empty() {
        egui::CollapsingHeader::new(bt("lib.notes", &[("n", &d.meta.notes.len().to_string())])).id_salt("bim_lib_dnotes").show(ui, |ui| {
            for n in &d.meta.notes {
                ui.label(RichText::new(n).small());
            }
        });
    }
    ui.horizontal_wrapped(|ui| {
        if mode == "ifc" && !withdrawn && ui.add(crate::shell::Btn::primary(bt("lib.place", &[]))).clicked() {
            act.push(("bim.lib.place", String::new()));
        }
        if mode == "ifc"
            && !withdrawn
            && let Some(name) = swap
            && ui.add(crate::shell::Btn::new(bt("lib.swap", &[("name", name)]))).clicked()
        {
            act.push(("bim.lib.swap", String::new()));
        }
        if !d.card.uuid.is_empty() && d.meta.library == "librarycraft" {
            ui.hyperlink_to(bt("lib.open_on", &[]), format!("/library/item/{}", crate::bimlibnet::enc(&d.meta.slug)));
        }
    });
    if mode != "ifc" {
        ui.label(RichText::new(bt("lib.needs_ifc", &[])).color(t.text_dim));
    }
}
