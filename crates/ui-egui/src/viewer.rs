//! The drawing viewer's details (BimCraft 0.5's DWG view: bimcraft.js `openDrawing` + dwg-view.js): print view (black
//! on white, layers that never print hidden, line weights; P), the Drawing panel (file facts, linked drawings and
//! pictures with "Add linked drawings…" / "Add pictures…" through the platform picker — on the web the browser's
//! picker, so a DWG's pictures and links are found there too), Find (the drawing's texts; a hit is shown ringed, as
//! `focusModelPoint` does), touch (one finger pans, two pinch-zoom about their middle and pan, a double tap fits),
//! fit on F. Words: BimCraft's catalog (`dwg.*`, `info.*`) and CadCraft's Find dialog (`dlg.find.*`).
//!
//! BimCraft's extras (M3.9–M3.11): the equipment schedule tab (dwgequip.rs), model space in 3D with picking for
//! Properties (dwg3d.rs, key 3), the dynamic-block list, linked drawings and pictures found by name in the project
//! folder (bimcraft.js `findLinked` / `findPicture`: the drawing's own folder first, then anywhere in the folder; a
//! picture whose file name only holds its name is "Shown, similar name"). On the desktop the project folder is the
//! drawing's own folder walked with its subfolders (folder-pick.js's rules), or a folder picked with "Open a folder",
//! as 0.5 Studio opens one; files added by hand come first.
//!
//! Commands (`viewer.*`): `viewer.print`, `viewer.info`, `viewer.find {name: text}`, `viewer.find.set {name:
//! "case=true" | "whole=true"}`, `viewer.find.show {index}`, `viewer.find.close`, `viewer.add {name: "drawing" |
//! "pictures"}`, `viewer.mark.clear`, `viewer.folder {name?: path}`; `viewer.equip*` (dwgequip.rs), `viewer.3d*`
//! (dwg3d.rs).

use cc_dwgview::folder::{self as dfolder, Folder, Found};
use cc_dwgview::page;
use egui::RichText;

use crate::lang::{self, BIMCRAFT, CADCRAFT};
use crate::{ImportedFile, Params, UiApp};

/// The viewer's own state.
#[derive(Debug, Default)]
pub struct Viewer {
    /// print view (dwg-view.js `setPrint`)
    pub print: bool,
    /// the Drawing panel (right)
    pub info: bool,
    pub find: Option<Find>,
    /// a ringed point: (space index, x, y)
    pub mark: Option<(usize, f64, f64)>,
    /// files added by hand (lower-case name → bytes; bimcraft.js `xrefFiles`), looked up before the project folder
    pub added: Vec<(String, Vec<u8>)>,
    /// web: the picker is open for "drawing" or "pictures"
    pub pick: Option<&'static str>,
    /// the pictures that could not be decoded (picture key)
    pub unreadable: Vec<String>,
    /// the equipment schedule tab (dwgequip.rs)
    pub equip: crate::dwgequip::Equip,
    /// model space in 3D (dwg3d.rs)
    pub d3: crate::dwg3d::Dwg3d,
    /// desktop: the project folder (its path, its walk, opened by the person) and the open drawing's place in it
    pub folder: Option<(String, Folder, bool)>,
    pub path: Option<String>,
    /// the open drawing's linked drawings that were found: (name, where)
    pub linked: Vec<(String, String)>,
    /// the open drawing's pictures: (name, where found and whether the name matched exactly, why not shown)
    pub pictures: Vec<PicRow>,
    /// the dynamic-block list of the open drawing (doc id, list)
    dyn_view: Option<(u64, Option<page::DynView>)>,
}

/// One picture of the open drawing as the Drawing panel lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct PicRow {
    pub name: String,
    /// (where, exact name)
    pub hit: Option<(String, bool)>,
    /// None = shown; else why not (a catalog key)
    pub why: Option<&'static str>,
}

/// Find over the drawing's texts.
#[derive(Debug, Default, Clone)]
pub struct Find {
    pub query: String,
    pub case: bool,
    pub whole: bool,
    pub hits: Vec<Hit>,
    pub ran: bool,
}

/// A text that matched: where it is and what it says.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub space: usize,
    pub x: f64,
    pub y: f64,
    pub s: String,
}

pub const UI_COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("viewer.print", "Print view", "P", "{} → black on white with line weights; layers that never print are hidden (again: back)"),
    ("viewer.info", "Drawing panel", "", "{} → shows or hides the drawing's facts, linked drawings and pictures"),
    ("viewer.find", "Find text", "Ctrl+F", "{name: text} → lists the drawing's texts that hold it (every space)"),
    ("viewer.find.set", "Find: options", "", "{name: \"case=true|false\" | \"whole=true|false\"} → runs the search again"),
    ("viewer.find.show", "Find: show a hit", "", "{index} → goes to that text (its space) and rings it"),
    ("viewer.find.close", "Find: close", "", "{}"),
    (
        "viewer.add",
        "Add linked drawings or pictures",
        "",
        "{name: \"drawing\" | \"pictures\"} → the platform picker; the files found are used at once",
    ),
    ("viewer.mark.clear", "Clear the ring", "", "{}"),
    (
        "viewer.folder",
        "Open a folder",
        "",
        "{name?: folder path} → the project folder (desktop): linked drawings and pictures are looked for in it and its subfolders",
    ),
];

/// Is the plain viewer showing (a drawing open, not for editing)?
pub fn active(app: &UiApp) -> bool {
    app.doc.is_some() && app.editor.is_none()
}

/// `num(n)`: en-US grouping (ui-util.js, whatever the language).
pub fn num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `bytes(n)` (ui-util.js).
pub fn bytes(n: usize) -> String {
    let f = n as f64;
    if n < 1024 {
        format!("{n} B")
    } else if n < 1_048_576 {
        format!("{:.0} KB", f / 1024.0)
    } else if n < 1_073_741_824 {
        format!("{:.1} MB", f / 1_048_576.0)
    } else {
        format!("{:.2} GB", f / 1_073_741_824.0)
    }
}

/// The status bar's cursor readout (bimcraft.js `coord`, `dwg.coords`).
pub fn coords(x: f64, y: f64) -> String {
    let c = |v: f64| {
        if v.abs() >= 1e5 {
            format!("{v:.0}")
        } else if v.abs() >= 100.0 {
            format!("{v:.2}")
        } else {
            format!("{v:.3}")
        }
    };
    lang::t(BIMCRAFT, "dwg.coords", &[("x", c(x)), ("y", c(y))])
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Does `text` hold `q` (findtext.js `findRegExp` without wildcards: match case, whole words)?
pub fn matches(text: &str, q: &str, case: bool, whole: bool) -> bool {
    if q.is_empty() {
        return false;
    }
    let (t, q) = if case { (text.to_string(), q.to_string()) } else { (text.to_lowercase(), q.to_lowercase()) };
    let mut from = 0usize;
    while let Some(i) = t.get(from..).and_then(|r| r.find(&q)) {
        let at = from.saturating_add(i);
        let end = at.saturating_add(q.len());
        let before_ok = t.get(..at).and_then(|b| b.chars().next_back()).is_none_or(|c| !is_word(c));
        let after_ok = t.get(end..).and_then(|a| a.chars().next()).is_none_or(|c| !is_word(c));
        if !whole || (before_ok && after_ok) {
            return true;
        }
        from = at.saturating_add(t.get(at..).and_then(|r| r.chars().next()).map_or(1, char::len_utf8));
    }
    false
}

/// The drawing's texts holding the query (model first, then the sheets in tab order), at most 2,000.
pub fn search(d: &cc_drawing::Drawing, f: &Find) -> Vec<Hit> {
    let mut out = Vec::new();
    for (si, sp) in d.spaces.iter().enumerate() {
        for b in &sp.batches {
            for tx in &b.texts {
                if out.len() >= 2000 {
                    return out;
                }
                if matches(&tx.s, &f.query, f.case, f.whole) {
                    out.push(Hit { space: si, x: tx.x, y: tx.y, s: tx.s.clone() });
                }
            }
        }
    }
    out
}

pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    if id.starts_with("viewer.equip") {
        return crate::dwgequip::run(app, id, params);
    }
    if id.starts_with("viewer.3d") {
        return crate::dwg3d::run(app, id, params);
    }
    match id {
        "viewer.folder" => {
            let dir = match params.name.clone() {
                Some(d) => d,
                None => match app.services.pick_folder.as_mut() {
                    Some(p) => match p() {
                        Some(d) => d,
                        None => return Ok(()),
                    },
                    None => return Err("viewer.folder: no folder picker here".into()),
                },
            };
            open_folder(app, &dir)?;
        }
        "viewer.print" => app.viewer.print = !app.viewer.print,
        "viewer.info" => app.viewer.info = !app.viewer.info,
        "viewer.mark.clear" => app.viewer.mark = None,
        "viewer.find" => {
            let q = params.name.clone().unwrap_or_default();
            if q.chars().count() > 500 {
                return Err("viewer.find: at most 500 characters".into());
            }
            let f = app.viewer.find.get_or_insert_with(Find::default);
            f.query = q;
            refind(app);
        }
        "viewer.find.set" => {
            let spec = params.name.clone().ok_or("viewer.find.set: name = \"case=true\" or \"whole=true\"")?;
            let (k, v) = spec.split_once('=').ok_or("viewer.find.set: name = \"<option>=true|false\"")?;
            let on = match v {
                "true" | "1" | "on" => true,
                "false" | "0" | "off" => false,
                _ => return Err(format!("viewer.find.set: {v} is not true or false")),
            };
            let f = app.viewer.find.get_or_insert_with(Find::default);
            match k {
                "case" => f.case = on,
                "whole" => f.whole = on,
                _ => return Err(format!("viewer.find.set: no option {k} (case, whole)")),
            }
            refind(app);
        }
        "viewer.find.show" => {
            let i = params
                .index
                .or_else(|| params.name.as_deref().and_then(|n| n.trim().parse().ok()))
                .ok_or("viewer.find.show: index is required")?;
            let h = app.viewer.find.as_ref().and_then(|f| f.hits.get(i)).cloned().ok_or_else(|| format!("viewer.find.show: no hit {i}"))?;
            focus(app, h.space, h.x, h.y);
            app.message = h.s;
        }
        "viewer.find.close" => {
            app.viewer.find = None;
            app.viewer.mark = None;
        }
        "viewer.add" => {
            let kind: &'static str = match params.name.as_deref() {
                Some("drawing") => "drawing",
                Some("pictures") | None => "pictures",
                Some(o) => return Err(format!("viewer.add: {o} is not drawing or pictures")),
            };
            if app.doc.is_none() {
                return Err("viewer.add: open a drawing first".into());
            }
            let Some(pick) = app.services.pick_files.as_mut() else { return Err("viewer.add: no file picker here".into()) };
            match pick(kind, true) {
                Some(files) => add_files(app, files),
                None => app.viewer.pick = Some(kind),
            }
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

fn refind(app: &mut UiApp) {
    let Some(d) = app.doc.as_ref() else { return };
    let Some(f) = app.viewer.find.as_mut() else { return };
    f.hits = search(&d.drawing, f);
    f.ran = true;
    app.viewer.mark = None;
    if !f.query.is_empty() {
        app.message = lang::t(&CADCRAFT, "dlg.find.results", &[("n", f.hits.len().to_string())]);
    }
}

/// dwg-view.js `focusModelPoint` for a point in a space: zoomed in a little and ringed.
pub fn focus(app: &mut UiApp, space: usize, x: f64, y: f64) {
    let Some(d) = app.doc.as_ref() else { return };
    let Some(sp) = d.drawing.spaces.get(space) else { return };
    let paper = sp.kind == cc_drawing::SpaceKind::Paper;
    let b = if paper { sp.paper.unwrap_or(sp.bounds) } else { sp.bounds };
    let span = (b.max_x - b.min_x).max(b.max_y - b.min_y) / if paper { 8.0 } else { 30.0 };
    app.space = space;
    let (w, h) = app.canvas_px;
    let mut v = app.views.get(space).copied().flatten().unwrap_or_default();
    v.width = w;
    v.height = h;
    v.zoom = w.min(h) / if span.is_finite() && span > 0.0 { span } else { 1.0 };
    v.cx = x;
    v.cy = y;
    if let Some(slot) = app.views.get_mut(space) {
        *slot = Some(v);
    }
    app.pending_fit = false;
    app.viewer.mark = Some((space, x, y));
}

/// Files the person added for the open drawing (bimcraft.js's "Add linked drawings…" / "Add pictures…"): kept by name
/// and looked up before the project folder; the drawing is read again with them.
pub fn add_files(app: &mut UiApp, files: Vec<ImportedFile>) {
    for f in files {
        let key = f.name.to_lowercase();
        match app.viewer.added.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = f.bytes,
            None => app.viewer.added.push((key, f.bytes)),
        }
    }
    reload(app);
}

/// Read the open drawing again (files added, a folder opened), keeping the space, layers and views.
fn reload(app: &mut UiApp) {
    let Some(src) = app.source.clone() else { return };
    let (space, hidden, views) = (app.space, app.hidden.clone(), app.views.clone());
    let res = load(app, &src);
    app.loaded(res);
    if let Some(d) = app.doc.as_ref()
        && views.len() == d.drawing.spaces.len()
    {
        app.space = space;
        app.hidden = hidden;
        app.views = views;
        app.pending_fit = false;
    }
    resolve_pictures(app);
}

/// Open a drawing in the viewer (bimcraft.js `openDwg`): the project folder first (desktop), then the drawing with its
/// linked drawings found, then its pictures.
pub fn open_drawing(app: &mut UiApp, f: &ImportedFile) {
    crate::dwg3d::leave(app);
    app.viewer.equip.on = false;
    place_in_folder(app, f.dir.as_deref(), &f.name);
    let res = load(app, f);
    let ok = res.is_ok();
    app.loaded(res);
    if ok {
        app.source = Some(f.clone());
        resolve_pictures(app);
    }
}

/// `path` with "/" separators, without a trailing one.
fn norm_dir(d: &str) -> String {
    d.replace('\\', "/").trim_end_matches('/').to_string()
}

/// The drawing's place in the project folder: the folder the person opened when the drawing is in it, else (desktop)
/// the drawing's own folder walked with its subfolders.
fn place_in_folder(app: &mut UiApp, dir: Option<&str>, name: &str) {
    let Some(dir) = dir.map(norm_dir) else {
        // no folder (web, the sample): only what was opened and added
        if !app.viewer.folder.as_ref().is_some_and(|f| f.2) {
            app.viewer.folder = None;
        }
        app.viewer.path = None;
        return;
    };
    if let Some((root, _, true)) = &app.viewer.folder {
        let root = norm_dir(root);
        if dir == root || dir.starts_with(&format!("{root}/")) {
            let rel = dir.get(root.len()..).unwrap_or("").trim_start_matches('/');
            app.viewer.path = Some(if rel.is_empty() { name.to_string() } else { format!("{rel}/{name}") });
            return;
        }
    }
    let Some(list) = app.services.list_dir.as_mut() else {
        app.viewer.folder = None;
        app.viewer.path = None;
        return;
    };
    let base = dir.clone();
    let walked = dfolder::walk(dfolder::base_of(&dir), &mut |rel| {
        let abs = if rel.is_empty() { base.clone() } else { format!("{base}/{rel}") };
        list(&abs)
    });
    match walked {
        Ok(f) => {
            app.viewer.folder = Some((dir, f, false));
            app.viewer.path = Some(name.to_string());
        }
        Err(_) => {
            // too big a folder to walk: the drawing's own folder only (Services::read_xref)
            app.viewer.folder = None;
            app.viewer.path = None;
        }
    }
}

/// "Open a folder" (desktop): walk it, and read the open drawing again with what it holds.
pub fn open_folder(app: &mut UiApp, dir: &str) -> Result<(), String> {
    let dir = norm_dir(dir);
    let list = app.services.list_dir.as_mut().ok_or("viewer.folder: this host cannot list folders")?;
    let base = dir.clone();
    let f = dfolder::walk(dfolder::base_of(&dir), &mut |rel| {
        let abs = if rel.is_empty() { base.clone() } else { format!("{base}/{rel}") };
        list(&abs)
    })?;
    let n = f.files.len();
    let models = f
        .files
        .iter()
        .filter(|p| {
            let l = p.to_lowercase();
            let model = [".rvt", ".rfa", ".rte", ".rft"].iter().any(|e| l.ends_with(e));
            // numbered backups ("Tower.0004.rvt") belong to their model
            let stem = l.rsplit_once('.').map_or(l.as_str(), |(a, _)| a);
            let backup = stem.rsplit_once('.').is_some_and(|(_, k)| k.len() == 4 && k.bytes().all(|b| b.is_ascii_digit()));
            model && !backup
        })
        .count();
    app.viewer.folder = Some((dir, f, true));
    if let Some(src) = app.source.clone() {
        place_in_folder(app, src.dir.as_deref(), &src.name);
        reload(app);
    }
    let w = |k: &str, n: usize| lang::t(BIMCRAFT, k, &[("n", n.to_string()), ("count", num(n as u64))]);
    app.message = lang::t(
        BIMCRAFT,
        "folder.read",
        &[("s", "0.0".into()), ("models", w("folder.n.models", models)), ("files", w("folder.n.files", n))],
    );
    Ok(())
}

/// The bytes of a file found by [`dfolder::find_linked`] / [`dfolder::find_picture`].
fn found_bytes(app: &mut UiApp, found: &Found) -> Option<Vec<u8>> {
    match found {
        Found::Added(n) => app.viewer.added.iter().find(|(k, _)| k == n).map(|(_, b)| b.clone()),
        Found::Path(rel) => {
            let root = app.viewer.folder.as_ref().map(|f| f.0.clone())?;
            let read = app.services.read_path.as_mut()?;
            read(&format!("{root}/{rel}")).ok().map(|f| f.bytes)
        }
    }
}

/// Read a drawing with its linked drawings: added by hand, then the project folder (the drawing's folder first), then
/// (desktop without a folder walk) "<name>.dwg" beside it.
fn load(app: &mut UiApp, f: &ImportedFile) -> Result<crate::document::Loaded, String> {
    let names: Vec<String> = app.viewer.added.iter().map(|(k, _)| k.clone()).collect();
    let folder = app.viewer.folder.as_ref().map(|f| f.1.clone());
    let path = app.viewer.path.clone();
    let mut hits: Vec<(String, Found, String)> = Vec::new();
    let mut linked: Vec<(String, String)> = Vec::new();
    // the linked drawings' names come from the file; look each one up first (the reader asks by name)
    let res = {
        let mut ask = |x: &str| -> Option<Vec<u8>> {
            let hit = dfolder::find_linked(x, path.as_deref(), &names, folder.as_ref())?;
            hits.push((x.to_string(), hit.found, hit.where_));
            None
        };
        let t = web_time::Instant::now();
        let model = crate::document::read_model_checked(&f.name, &f.bytes)?;
        for x in crate::document::xref_names(&model) {
            ask(&x);
        }
        let mut found: Vec<(String, Vec<u8>)> = Vec::new();
        for (x, fd, where_) in &hits {
            if let Some(b) = found_bytes(app, fd) {
                found.push((x.to_lowercase(), b));
                linked.push((x.clone(), where_.clone()));
            }
        }
        let dir = f.dir.clone();
        let mut read = app.services.read_xref.take();
        let has_folder = folder.is_some();
        let r = crate::document::open_model_with(&f.name, &f.bytes, model, t, &mut |x| {
            found.iter().find(|(k, _)| *k == x.to_lowercase()).map(|(_, b)| b.clone()).or_else(|| match (read.as_mut(), dir.as_deref()) {
                (Some(r), Some(d)) if !has_folder => r(d, x),
                _ => None,
            })
        });
        app.services.read_xref = read;
        r
    };
    app.viewer.linked = linked;
    res
}

/// The open drawing's pictures (bimcraft.js): each looked for by name (added by hand, the project folder, a similar
/// name), read into the viewer's picture store under the drawing's own reference so the canvas draws it.
pub fn resolve_pictures(app: &mut UiApp) {
    app.viewer_pictures = cc_cad::pictures::Pictures::new();
    app.viewer.pictures.clear();
    let Some(d) = app.doc.as_ref() else { return };
    let images: Vec<(String, String)> = d.drawing.images.iter().map(|im| (im.file.clone(), im.name.clone())).collect();
    let names: Vec<String> = app.viewer.added.iter().map(|(k, _)| k.clone()).collect();
    let folder = app.viewer.folder.as_ref().map(|f| f.1.clone());
    let path = app.viewer.path.clone();
    for (file, name) in images {
        let hit = dfolder::find_picture(&name, path.as_deref(), &names, folder.as_ref());
        let mut row = PicRow { name: name.clone(), hit: None, why: Some("dwg.img.notfound") };
        if let Some(h) = hit {
            row.hit = Some((h.where_.clone(), h.exact));
            let fname = match &h.found {
                Found::Added(n) => n.clone(),
                Found::Path(p) => p.to_lowercase(),
            };
            let tiff = fname.ends_with(".tif") || fname.ends_with(".tiff");
            row.why = match found_bytes(app, &h.found) {
                Some(b) => match cc_cad::pictures::add_picture(&mut app.viewer_pictures, &file, &b) {
                    Ok(_) => None,
                    Err(_) if tiff => Some("dwg.img.tiff"),
                    Err(_) => Some("dwg.img.unreadable"),
                },
                None => Some("dwg.img.unreadable"),
            };
        }
        app.viewer.pictures.push(row);
    }
}

/// Leaving the extras for a space (a sheet or Model picked).
pub fn left_extras(app: &mut UiApp) {
    app.viewer.equip.on = false;
    crate::dwg3d::leave(app);
}

/// A tab of the viewer's own after the sheets.
#[derive(Debug, Clone)]
pub struct ExtraTab {
    pub label: String,
    pub title: String,
    pub selected: bool,
    pub cmd: &'static str,
}

/// The equipment tab (when the drawing has item tags) and the 3D switch (bimcraft.js `renderSheetTabs`).
pub fn extra_tabs(app: &mut UiApp) -> Vec<ExtraTab> {
    if !active(app) {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Some(l) = crate::dwgequip::list(app) {
        let (label, title) = page::tab_label(&l);
        out.push(ExtraTab { label, title, selected: crate::dwgequip::showing(app), cmd: "viewer.equip" });
    }
    if app.doc.as_ref().is_some_and(|d| d.model.is_some()) {
        out.push(ExtraTab {
            label: "3D".into(),
            title: lang::bim("dwg.tab.3d.title"),
            selected: crate::dwg3d::showing(app),
            cmd: "viewer.3d",
        });
    }
    out
}

/// The Drawing panel's dynamic-block list (worked out once per drawing).
fn dyn_view(app: &mut UiApp) -> Option<page::DynView> {
    let d = app.doc.as_ref()?;
    if let Some((id, v)) = &app.viewer.dyn_view
        && *id == d.id
    {
        return v.clone();
    }
    let v = d.model.as_ref().and_then(|m| {
        let defs = cc_dwgview::dynamic::dynamic_defs(m);
        page::dyn_view(&cc_dwgview::dynamic::dynamic_summary(m, &defs))
    });
    app.viewer.dyn_view = Some((d.id, v.clone()));
    v
}

/// A file dropped while the viewer shows a drawing: a picture or one of its missing linked drawings is added to it
/// (true), anything else opens as usual (false).
pub fn dropped(app: &mut UiApp, f: &ImportedFile) -> bool {
    if !active(app) {
        return false;
    }
    let lower = f.name.to_lowercase();
    let Some(d) = app.doc.as_ref() else { return false };
    let stem = lower.rsplit_once('.').map_or(lower.as_str(), |(s, _)| s).to_string();
    let link =
        (lower.ends_with(".dwg") || lower.ends_with(".dxf")) && d.drawing.xrefs.iter().any(|x| !x.loaded && x.name.to_lowercase() == stem);
    let picture = d.drawing.images.iter().any(|im| cc_cad::pictures::picture_key(&im.file) == cc_cad::pictures::picture_key(&f.name));
    if link || picture {
        add_files(app, vec![f.clone()]);
        return true;
    }
    false
}

/// Every frame: the web picker's files, and the viewer's keys (P print view, F fit, Ctrl+F find).
pub fn frame(app: &mut UiApp, ctx: &egui::Context) {
    if app.viewer.pick.is_some()
        && let Some(slot) = app.services.picked.as_ref()
    {
        let got = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        if let Some(files) = got {
            app.viewer.pick = None;
            add_files(app, files);
        }
    }
    if !active(app) {
        return;
    }
    let typing = ctx.memory(|m| m.focused().is_some());
    let (p, f, find, three, x) = ctx.input(|i| {
        (
            !i.modifiers.any() && i.key_pressed(egui::Key::P),
            !i.modifiers.any() && i.key_pressed(egui::Key::F),
            i.modifiers.command && i.key_pressed(egui::Key::F),
            !i.modifiers.any() && i.key_pressed(egui::Key::Num3),
            !i.modifiers.any() && i.key_pressed(egui::Key::X),
        )
    });
    if !typing && three {
        app.run_ui("viewer.3d", &Params::default());
    }
    if !typing && x && crate::dwg3d::showing(app) {
        app.run_ui("viewer.3d.section", &Params::default());
    }
    if find {
        app.viewer.find.get_or_insert_with(Find::default);
    }
    if typing {
        return;
    }
    if p {
        app.run_ui("viewer.print", &Params::default());
    }
    if f {
        app.run_ui("view.fit", &Params::default());
    }
}

/// Touch on the canvas (dwg-view.js 0.4 S.8): two fingers pinch-zoom about their middle and pan with it (one finger
/// pans as a drag does; a double tap fits as a double click does). Returns true while two fingers are down.
pub fn touch(ui: &egui::Ui, rect: egui::Rect, view: &mut cc_render::View2D, ppp: f64) -> bool {
    let Some(mt) = ui.input(|i| i.multi_touch()) else { return false };
    let c = mt.center_pos;
    if !rect.contains(c) {
        return false;
    }
    let sx = f64::from(c.x - rect.min.x) * ppp;
    let sy = f64::from(c.y - rect.min.y) * ppp;
    let z = f64::from(mt.zoom_delta);
    if z.is_finite() && z > 0.0 && (z - 1.0).abs() > 1e-6 {
        view.zoom_at(sx, sy, z);
    }
    let d = mt.translation_delta;
    if d.x != 0.0 || d.y != 0.0 {
        view.pan(f64::from(d.x) * ppp, f64::from(d.y) * ppp);
    }
    true
}

/// The Drawing panel (right): bimcraft.js's info list and props (hint, linked drawings, pictures, not drawn, dynamic
/// blocks); while the 3D view shows, what it draws or the objects picked (dwg3d.rs).
pub fn info_panel(app: &mut UiApp, ui: &mut egui::Ui) {
    if !app.viewer.info || !active(app) {
        return;
    }
    let equip = crate::dwgequip::list(app);
    let dynv = dyn_view(app);
    let t = app.tokens;
    if crate::dwg3d::showing(app) {
        let frame = egui::Frame::new().fill(t.panel).inner_margin(8);
        egui::Panel::right("viewer_info").default_size(270.0).size_range(200.0..=420.0).resizable(true).frame(frame).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| crate::dwg3d::panel(app, ui));
        });
        return;
    }
    let Some(d) = app.doc.as_ref() else { return };
    let dr = &d.drawing;
    let w = |k: &str| lang::bim(k);
    let sheets = dr.spaces.iter().filter(|s| s.kind == cc_drawing::SpaceKind::Paper).count();
    let format = if dr.release.is_empty() {
        dr.version.clone()
    } else {
        lang::t(BIMCRAFT, "dwg.format.release", &[("release", dr.release.clone()), ("tag", dr.version.clone())])
    };
    let mut rows: Vec<(String, String)> = vec![
        (w("info.file"), d.name.clone()),
        (w("info.size"), bytes(d.bytes)),
        (w("info.format"), format),
        (w("info.units"), dr.units.clone()),
        (w("info.entities"), num(dr.stats.entities)),
        (w("info.layers"), num(dr.layers.len() as u64)),
        (w("info.sheets"), if sheets > 0 { num(sheets as u64) } else { w("info.sheets.none") }),
    ];
    if !dr.xrefs.is_empty() {
        let loaded = dr.xrefs.iter().filter(|x| x.loaded).count();
        rows.push((
            w("info.linked"),
            lang::t(BIMCRAFT, "dwg.linked.loaded", &[("loaded", loaded.to_string()), ("total", dr.xrefs.len().to_string())]),
        ));
    }
    if let Some(l) = &equip {
        rows.push(page::info_row(l));
    }
    rows.retain(|(_, v)| !v.is_empty());
    let from_folder = app.viewer.folder.is_some() && app.viewer.path.is_some();
    let pictures: Vec<PicRow> = app
        .viewer
        .pictures
        .iter()
        .map(|p| {
            let mut p = p.clone();
            // a picture the canvas could not decode
            if p.why.is_none()
                && dr.images.iter().any(|im| im.name == p.name && app.viewer.unreadable.contains(&cc_cad::pictures::picture_key(&im.file)))
            {
                p.why = Some("dwg.img.unreadable");
            }
            p
        })
        .collect();
    let xrefs: Vec<(String, bool, Option<String>)> = dr
        .xrefs
        .iter()
        .map(|x| {
            (x.name.clone(), x.loaded, app.viewer.linked.iter().find(|(n, _)| n.eq_ignore_ascii_case(&x.name)).map(|(_, w)| w.clone()))
        })
        .collect();
    let notdrawn: Vec<(String, u64)> = dr.stats.unsupported.iter().map(|(k, v)| (k.clone(), *v)).collect();
    let truncated = dr.stats.truncated;
    let can_folder = app.services.pick_folder.is_some() && app.services.list_dir.is_some();
    let mut act: Option<(&str, &str)> = None;
    let frame = egui::Frame::new().fill(t.panel).inner_margin(8);
    egui::Panel::right("viewer_info").default_size(270.0).size_range(200.0..=420.0).resizable(true).frame(frame).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.label(RichText::new(w("info.h.drawing")).strong().color(t.text_strong));
            egui::Grid::new("viewer_info_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
                for (k, v) in &rows {
                    ui.label(RichText::new(k).small().color(t.text_dim));
                    ui.label(RichText::new(v).small().color(t.text));
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new(w("dwg.hint")).small().color(t.text_dim));
            // ok: green; similar: amber; missing: red (bc-pill is-ok / is-warn / is-bad)
            let pill = |ui: &mut egui::Ui, state: u8, text: String| {
                let c = match state {
                    0 => egui::Color32::from_rgb(0x3c, 0xb0, 0x60),
                    1 => egui::Color32::from_rgb(0xd8, 0x9a, 0x2a),
                    _ => egui::Color32::from_rgb(0xe0, 0x6c, 0x5c),
                };
                ui.label(RichText::new(text).small().color(c));
            };
            if !xrefs.is_empty() {
                ui.separator();
                ui.label(RichText::new(w("dwg.linked.title")).strong().color(t.text_strong));
                for (n, ok, at) in &xrefs {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("{n}.dwg")).small().color(t.text));
                        pill(ui, if *ok { 0 } else { 2 }, w(if *ok { "dwg.linked.shown" } else { "dwg.linked.notfound" }));
                        if let (true, Some(at)) = (*ok, at) {
                            ui.label(RichText::new(at).small().color(t.text_dim));
                        }
                    });
                }
                if xrefs.iter().any(|x| !x.1) {
                    ui.label(
                        RichText::new(w(if from_folder { "dwg.linked.missing.folder" } else { "dwg.linked.missing.file" }))
                            .small()
                            .color(t.text_dim),
                    );
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(w("dwg.linked.add")).clicked() {
                            act = Some(("viewer.add", "drawing"));
                        }
                        if can_folder && ui.button(w("ui.open.folder")).clicked() {
                            act = Some(("viewer.folder", ""));
                        }
                    });
                }
            }
            if !pictures.is_empty() {
                ui.separator();
                ui.label(RichText::new(w("dwg.images.title")).strong().color(t.text_strong));
                for p in &pictures {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(&p.name).small().color(t.text));
                        match (p.why, &p.hit) {
                            (None, Some((_, true))) => pill(ui, 0, w("dwg.img.shown")),
                            (None, _) => pill(ui, 1, w("dwg.img.similar")),
                            (Some(k), _) => pill(ui, 2, w(k)),
                        }
                        if let Some((at, _)) = &p.hit {
                            ui.label(RichText::new(at).small().color(t.text_dim));
                        }
                    });
                }
                if pictures.iter().any(|p| p.hit.is_none()) {
                    ui.label(
                        RichText::new(w(if from_folder { "dwg.img.missing.folder" } else { "dwg.img.missing.file" }))
                            .small()
                            .color(t.text_dim),
                    );
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(w("dwg.img.add")).clicked() {
                            act = Some(("viewer.add", "pictures"));
                        }
                        if can_folder && ui.button(w("ui.open.folder")).clicked() {
                            act = Some(("viewer.folder", ""));
                        }
                    });
                }
            }
            if !notdrawn.is_empty() {
                ui.separator();
                ui.label(RichText::new(w("dwg.notdrawn.title")).strong().color(t.text_strong));
                egui::Grid::new("viewer_notdrawn").num_columns(2).show(ui, |ui| {
                    for (k, n) in &notdrawn {
                        ui.label(RichText::new(k).small().color(t.text));
                        ui.label(RichText::new(num(*n)).small().color(t.text_dim));
                        ui.end_row();
                    }
                });
                ui.label(RichText::new(w("dwg.notdrawn.note")).small().color(t.text_dim));
            }
            if let Some(dv) = &dynv {
                ui.separator();
                ui.label(RichText::new(&dv.title).strong().color(t.text_strong));
                for (name, lines) in &dv.items {
                    ui.label(RichText::new(name).small().color(t.text));
                    for l in lines {
                        ui.label(RichText::new(format!("   {l}")).small().color(t.text_dim));
                    }
                }
                ui.label(RichText::new(&dv.note).small().color(t.text_dim));
            }
            if truncated {
                ui.label(RichText::new(w("dwg.truncated")).small().color(egui::Color32::from_rgb(0xe0, 0x6c, 0x5c)));
            }
        });
    });
    if let Some((id, name)) = act {
        let p = Params { name: if name.is_empty() { None } else { Some(name.into()) }, ..Default::default() };
        if let Err(e) = app.run(id, &p) {
            app.error = Some(e);
        }
    }
}

/// The Find panel (left of the drawing, under the layers): the query, match case, whole words, the hits.
pub fn find_panel(app: &mut UiApp, ui: &mut egui::Ui) {
    if !active(app) {
        return;
    }
    let Some(f) = app.viewer.find.clone() else { return };
    let t = app.tokens;
    let c = |k: &str| lang::cad(k);
    let mut q = f.query.clone();
    let mut act: Option<(String, Params)> = None;
    let frame = egui::Frame::new().fill(t.panel).inner_margin(8);
    egui::Panel::right("viewer_find").default_size(260.0).size_range(200.0..=420.0).resizable(true).frame(frame).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(c("dlg.find.what")).strong().color(t.text_strong));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").on_hover_text(c("dlg.find.close")).clicked() {
                    act = Some(("viewer.find.close".into(), Params::default()));
                }
            });
        });
        let id = ui.make_persistent_id("viewer.find.q");
        let r = ui.add(crate::shell::field(&mut q).id(id).desired_width(f32::INFINITY));
        if !f.ran && q.is_empty() {
            r.request_focus();
        }
        if r.changed() {
            act = Some(("viewer.find".into(), Params { name: Some(q.clone()), ..Default::default() }));
        }
        let (mut case, mut whole) = (f.case, f.whole);
        if ui.checkbox(&mut case, c("dlg.find.case")).changed() {
            act = Some(("viewer.find.set".into(), Params { name: Some(format!("case={case}")), ..Default::default() }));
        }
        if ui.checkbox(&mut whole, c("dlg.find.whole")).changed() {
            act = Some(("viewer.find.set".into(), Params { name: Some(format!("whole={whole}")), ..Default::default() }));
        }
        if f.ran && !f.query.is_empty() {
            ui.label(RichText::new(lang::t(&CADCRAFT, "dlg.find.results", &[("n", f.hits.len().to_string())])).small().color(t.text_dim));
        }
        ui.separator();
        let names: Vec<String> = app
            .doc
            .as_ref()
            .map(|d| {
                d.drawing
                    .spaces
                    .iter()
                    .map(|s| if s.kind == cc_drawing::SpaceKind::Model { lang::cad("app.tab.model") } else { s.name.clone() })
                    .collect()
            })
            .unwrap_or_default();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (i, h) in f.hits.iter().enumerate() {
                let on = app.viewer.mark.is_some_and(|(s, x, y)| s == h.space && x == h.x && y == h.y);
                let label = format!("{}  ·  {}", h.s.chars().take(80).collect::<String>(), names.get(h.space).cloned().unwrap_or_default());
                if ui.selectable_label(on, RichText::new(label).small()).clicked() {
                    act = Some(("viewer.find.show".into(), Params { index: Some(i), ..Default::default() }));
                }
            }
        });
    });
    if let Some((id, p)) = act
        && let Err(e) = app.run(&id, &p)
    {
        app.error = Some(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_rules() {
        assert!(matches("Walk-in COOLER", "cooler", false, false));
        assert!(!matches("Walk-in COOLER", "cooler", true, false));
        assert!(matches("K-101 sink", "sink", false, true));
        assert!(!matches("sinks", "sink", false, true));
        assert!(matches("sinks and sink", "sink", false, true));
        assert!(!matches("anything", "", false, false));
        assert!(matches("ÉTAGE 2", "étage", false, true));
        assert_eq!(num(1_234_567), "1,234,567");
        assert_eq!(num(12), "12");
        assert_eq!(bytes(1536), "2 KB");
        assert_eq!(bytes(5_000_000), "4.8 MB");
    }

    #[test]
    fn viewer_commands_on_the_sample() {
        cc_lang::with(cc_lang::Locale::En, || {
            let mut app = UiApp::default();
            app.run("file.sample", &Params::default()).unwrap();
            app.canvas_px = (1600.0, 1000.0);
            app.run("viewer.print", &Params::default()).unwrap();
            assert!(app.viewer.print);
            app.run("viewer.find", &Params { name: Some("K-".into()), ..Default::default() }).unwrap();
            let n = app.viewer.find.as_ref().map_or(0, |f| f.hits.len());
            assert!(n > 0, "the sample's texts hold K-");
            app.run("viewer.find.show", &Params { index: Some(0), ..Default::default() }).unwrap();
            assert!(app.viewer.mark.is_some());
            assert!(app.run("viewer.find.show", &Params { index: Some(99_999), ..Default::default() }).is_err());
            assert!(app.run("viewer.find.set", &Params { name: Some("case=maybe".into()), ..Default::default() }).is_err());
            app.run("viewer.find.close", &Params::default()).unwrap();
            assert!(app.viewer.find.is_none() && app.viewer.mark.is_none());
            // a picture added by hand is used (the sample names its logo); a stranger file is refused with a word
            app.services.pick_files =
                Some(Box::new(|_, _| Some(vec![ImportedFile { name: "notes.txt".into(), bytes: b"hello".to_vec(), dir: None }])));
            app.run("viewer.add", &Params { name: Some("pictures".into()), ..Default::default() }).unwrap();
            assert!(app.viewer_pictures.is_empty());
        });
    }

    /// A small DXF: a block with item tags placed twice (one KCL item), a 3D face, a line with thickness, a text, and a
    /// picture reference; layers with handles.
    fn kitchen_dxf() -> Vec<u8> {
        let mut l: Vec<String> = Vec::new();
        let mut g = |c: i32, v: &str| {
            l.push(c.to_string());
            l.push(v.to_string());
        };
        for (c, v) in [(0, "SECTION"), (2, "HEADER"), (9, "$ACADVER"), (1, "AC1015"), (9, "$INSUNITS"), (70, "1"), (0, "ENDSEC")] {
            g(c, v);
        }
        for (c, v) in [(0, "SECTION"), (2, "TABLES"), (0, "TABLE"), (2, "LAYER"), (70, "2")] {
            g(c, v);
        }
        for (h, n) in [("10", "0"), ("11", "K-EQPM")] {
            for (c, v) in [
                (0, "LAYER"),
                (5, h),
                (100, "AcDbSymbolTableRecord"),
                (100, "AcDbLayerTableRecord"),
                (2, n),
                (70, "0"),
                (62, "3"),
                (6, "CONTINUOUS"),
            ] {
                g(c, v);
            }
        }
        for (c, v) in [(0, "ENDTAB"), (0, "ENDSEC"), (0, "SECTION"), (2, "BLOCKS")] {
            g(c, v);
        }
        for (c, v) in [
            (0, "BLOCK"),
            (5, "20"),
            (100, "AcDbEntity"),
            (8, "0"),
            (100, "AcDbBlockBegin"),
            (2, "TAG"),
            (70, "2"),
            (10, "0"),
            (20, "0"),
            (30, "0"),
            (3, "TAG"),
        ] {
            g(c, v);
        }
        for (c, v) in
            [(0, "CIRCLE"), (5, "21"), (100, "AcDbEntity"), (8, "0"), (100, "AcDbCircle"), (10, "0"), (20, "0"), (30, "0"), (40, "1")]
        {
            g(c, v);
        }
        for tag in ["KCL-ITEM", "KCL-QTY", "KCL-CATEGORY"] {
            for (c, v) in [
                (0, "ATTDEF"),
                (5, "2F"),
                (100, "AcDbEntity"),
                (8, "0"),
                (100, "AcDbText"),
                (10, "0"),
                (20, "0"),
                (30, "0"),
                (40, "1"),
                (1, ""),
                (100, "AcDbAttributeDefinition"),
                (3, tag),
                (2, tag),
                (70, "1"),
            ] {
                g(c, v);
            }
        }
        for (c, v) in
            [(0, "ENDBLK"), (5, "22"), (100, "AcDbEntity"), (8, "0"), (100, "AcDbBlockEnd"), (0, "ENDSEC"), (0, "SECTION"), (2, "ENTITIES")]
        {
            g(c, v);
        }
        for (h, x, qty) in [("30", "10", "2"), ("40", "50", "3")] {
            for (c, v) in [
                (0, "INSERT"),
                (5, h),
                (100, "AcDbEntity"),
                (8, "K-EQPM"),
                (100, "AcDbBlockReference"),
                (66, "1"),
                (2, "TAG"),
                (10, x),
                (20, "5"),
                (30, "0"),
            ] {
                g(c, v);
            }
            for (tag, val) in [("KCL-ITEM", "7"), ("KCL-QTY", qty), ("KCL-CATEGORY", "SINK")] {
                for (c, v) in [
                    (0, "ATTRIB"),
                    (5, "3F"),
                    (100, "AcDbEntity"),
                    (8, "0"),
                    (100, "AcDbText"),
                    (10, x),
                    (20, "5"),
                    (30, "0"),
                    (40, "1"),
                    (1, val),
                    (100, "AcDbAttribute"),
                    (2, tag),
                    (70, "0"),
                ] {
                    g(c, v);
                }
            }
            for (c, v) in [(0, "SEQEND"), (5, "4F"), (100, "AcDbEntity"), (8, "0")] {
                g(c, v);
            }
        }
        for (c, v) in [
            (0, "3DFACE"),
            (5, "50"),
            (100, "AcDbEntity"),
            (8, "K-EQPM"),
            (100, "AcDbFace"),
            (10, "0"),
            (20, "0"),
            (30, "0"),
            (11, "10"),
            (21, "0"),
            (31, "0"),
            (12, "10"),
            (22, "10"),
            (32, "4"),
            (13, "0"),
            (23, "10"),
            (33, "4"),
        ] {
            g(c, v);
        }
        for (c, v) in [
            (0, "LINE"),
            (5, "51"),
            (100, "AcDbEntity"),
            (8, "K-EQPM"),
            (100, "AcDbLine"),
            (39, "6"),
            (10, "0"),
            (20, "0"),
            (30, "0"),
            (11, "60"),
            (21, "0"),
            (31, "0"),
        ] {
            g(c, v);
        }
        for (c, v) in [
            (0, "TEXT"),
            (5, "52"),
            (100, "AcDbEntity"),
            (8, "0"),
            (100, "AcDbText"),
            (10, "5"),
            (20, "20"),
            (30, "0"),
            (40, "2"),
            (1, "KITCHEN 101"),
            (100, "AcDbText"),
        ] {
            g(c, v);
        }
        for (c, v) in [(0, "ENDSEC"), (0, "EOF")] {
            g(c, v);
        }
        (l.join("\r\n") + "\r\n").into_bytes()
    }

    #[test]
    fn equipment_3d_and_folder_on_a_drawing() {
        cc_lang::with(cc_lang::Locale::En, || {
            let mut app = UiApp::default();
            type Saved = std::rc::Rc<std::cell::RefCell<Vec<(String, Vec<u8>)>>>;
            let saved: Saved = std::rc::Rc::default();
            let s2 = std::rc::Rc::clone(&saved);
            app.services.save = Some(Box::new(move |name, bytes| {
                s2.borrow_mut().push((name.to_string(), bytes));
                Ok(Some(name.to_string()))
            }));
            // a project folder: the drawing in Plans/, its linked drawing and its picture elsewhere
            app.services.list_dir = Some(Box::new(|d: &str| {
                Ok(match d {
                    "/job" => vec![("Plans".into(), true), ("Xrefs".into(), true), ("Job_backup".into(), true)],
                    "/job/Plans" => vec![("kitchen.dxf".into(), false)],
                    "/job/Xrefs" => vec![("Prefix-logo-1.png".into(), false)],
                    _ => vec![],
                })
            }));
            app.services.read_path = Some(Box::new(|p: &str| Err(format!("{p}: not here"))));
            app.open_file(&ImportedFile { name: "kitchen.dxf".into(), bytes: kitchen_dxf(), dir: Some("/job/Plans".into()) });
            assert!(app.error.is_none(), "{:?}", app.error);
            // the drawing's own folder was walked (layer handles read: the viewport-freeze fix)
            assert_eq!(app.viewer.path.as_deref(), Some("kitchen.dxf"));
            assert!(app.doc.as_ref().is_some_and(|d| d.layer_handles.get("11").map(String::as_str) == Some("K-EQPM")));
            // the equipment tab: one item, two tags, quantity 2 + 3
            let tabs = extra_tabs(&mut app);
            assert_eq!(tabs.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), vec!["Equipment (1)", "3D"]);
            app.run("viewer.equip", &Params::default()).unwrap();
            assert!(crate::dwgequip::showing(&app));
            let l = crate::dwgequip::list(&mut app).unwrap();
            assert_eq!((l.rows.len(), l.rows[0].qty.as_str(), l.tags), (1, "5", 2));
            app.run("viewer.equip.filter", &Params { name: Some("  sink ".into()), ..Default::default() }).unwrap();
            app.run("viewer.equip.csv", &Params::default()).unwrap();
            assert_eq!(app.message, "Downloaded kitchen equipment (Not on any sheet).csv (1 item).");
            let (name, bytes) = saved.borrow().first().cloned().unwrap();
            assert_eq!(name, "kitchen equipment (Not on any sheet).csv");
            assert!(String::from_utf8(bytes).unwrap().starts_with("\u{feff}Item,Qty,Category"));
            assert!(app.run("viewer.equip.pick", &Params { name: Some("layout:none".into()), ..Default::default() }).is_err());
            app.run("viewer.equip.show", &Params { index: Some(0), ..Default::default() }).unwrap();
            assert!(!crate::dwgequip::showing(&app));
            assert_eq!(app.message, "Item 7, SINK is ringed on Model.");
            assert!(app.viewer.mark.is_some());
            // 3D: drawn, picked, cut
            app.run("viewer.3d", &Params::default()).unwrap();
            assert!(crate::dwg3d::showing(&app));
            assert!(app.message.starts_with("Model space in 3D:"), "{}", app.message);
            app.run("viewer.3d.pick", &Params { name: Some("0".into()), ..Default::default() }).unwrap();
            app.run("viewer.3d.pick", &Params { name: Some("add:2".into()), ..Default::default() }).unwrap();
            assert_eq!(app.viewer.d3.picked, vec![0, 2]);
            assert!(app.run("viewer.3d.pick", &Params { index: Some(999), ..Default::default() }).is_err());
            for v in ["top", "persp", "extents", "zoom:0.5", "iso"] {
                app.run("viewer.3d.view", &Params { name: Some(v.into()), ..Default::default() }).unwrap();
            }
            app.run("viewer.3d.section", &Params::default()).unwrap();
            app.run("viewer.3d.section", &Params { name: Some("front".into()), factor: Some(3.0), ..Default::default() }).unwrap();
            assert_eq!(app.viewer.d3.section.map(|s| (s.axis, s.offset)), Some(("front", 3.0)));
            app.run("viewer.3d.section", &Params { name: Some("off".into()), ..Default::default() }).unwrap();
            // a sheet or Model picked: back to 2D
            app.run("space.select", &Params { index: Some(0), ..Default::default() }).unwrap();
            assert!(!crate::dwg3d::showing(&app));
            // "Open a folder": the walk skips housekeeping folders; the drawing is found inside it
            app.run("viewer.folder", &Params { name: Some("/job".into()), ..Default::default() }).unwrap();
            assert_eq!(app.viewer.path.as_deref(), Some("Plans/kitchen.dxf"));
            let f = app.viewer.folder.as_ref().map(|f| f.1.clone()).unwrap();
            assert_eq!(f.skipped, vec!["Job_backup"]);
            assert_eq!(f.files, vec!["Plans/kitchen.dxf", "Xrefs/Prefix-logo-1.png"]);
        });
    }
}
