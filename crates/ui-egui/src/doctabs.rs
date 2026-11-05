//! Several drawings open in one window (cadcraft.js's doctabs.js, 0.4 part 2 Q.3): tabs over the drawing, and Tile side
//! by side / stacked, each pane of the split screen then showing its own drawing.
//!
//! The open drawing is the window's (`UiApp::editor`, its display, views, open space, hidden layers, file); every other
//! one waits in a [`Slot`] of its own, which the split screen can still draw in a pane. A switch trades the window's
//! drawing for a slot's (doctabs.js `doc.swapContent`); undo, the open space, the views, the layers shown and the file
//! travel with each drawing; the clipboard is the window's, so copy and paste work between them. New and Open make a
//! tab of their own unless the open drawing is a new one nobody has touched. The strip shows while two or more are
//! open: a tab per drawing (● unsaved, × closes: unsaved asks first), New, Tile side by side, Tile stacked, Link views.

use std::collections::HashSet;

use cc_render::View2D;
use serde_json::{Value, json};

use crate::{ImportedFile, Params, UiApp, document::Loaded, edit, lang, paneview};

/// One drawing waiting in its tab: what the window holds of the open one.
#[derive(Default)]
pub struct Slot {
    editor: Option<edit::Editor>,
    doc: Option<Loaded>,
    views: Vec<Option<View2D>>,
    space: usize,
    hidden: HashSet<String>,
    source: Option<ImportedFile>,
    save_format: String,
    prev_views: Vec<View2D>,
    pending_fit: bool,
    viewer_pictures: cc_cad::pictures::Pictures,
}

impl Slot {
    /// The window's drawing taken out of it.
    fn take(app: &mut UiApp) -> Slot {
        Slot {
            editor: app.editor.take(),
            doc: app.doc.take(),
            views: std::mem::take(&mut app.views),
            space: std::mem::take(&mut app.space),
            hidden: std::mem::take(&mut app.hidden),
            source: app.source.take(),
            save_format: std::mem::take(&mut app.save_format),
            prev_views: std::mem::take(&mut app.prev_views),
            pending_fit: std::mem::take(&mut app.pending_fit),
            viewer_pictures: std::mem::take(&mut app.viewer_pictures),
        }
    }
    /// … and put back into it.
    fn put(self, app: &mut UiApp) {
        app.editor = self.editor;
        app.doc = self.doc;
        app.views = self.views;
        app.space = self.space;
        app.hidden = self.hidden;
        app.source = self.source;
        app.save_format = self.save_format;
        app.prev_views = self.prev_views;
        app.pending_fit = self.pending_fit;
        app.viewer_pictures = self.viewer_pictures;
    }
    /// What a pane needs to draw this drawing: its display, its editor, the layers it hides.
    pub fn parts(&self) -> (Option<&Loaded>, Option<&edit::Editor>, &HashSet<String>) {
        (self.doc.as_ref(), self.editor.as_ref(), &self.hidden)
    }
}

/// One tab: its id (kept while it is open) and, unless it is the open one, its drawing.
pub struct Tab {
    pub id: u32,
    pub store: Option<Box<Slot>>,
}

/// The window's open drawings (doctabs.js `tabs`).
pub struct DocTabs {
    pub list: Vec<Tab>,
    pub active: u32,
    next: u32,
    /// a tab with unsaved changes asked to close: the dialog (doctabs.close.title) is open for it
    pub asking: Option<u32>,
}

impl Default for DocTabs {
    fn default() -> Self {
        DocTabs { list: vec![Tab { id: 1, store: None }], active: 1, next: 2, asking: None }
    }
}

/// UI commands (the control channel, scripts, the strip's buttons).
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("doctabs.new", "New drawing in its own tab", "", "{} → doctabs.js tabs.add(): a new drawing in a tab of its own, opened"),
    ("doctabs.switch", "Open drawing", "", "{index: integer} → that tab's drawing becomes the open one (its own view and space)"),
    (
        "doctabs.close",
        "Close drawing",
        "",
        "{index: integer, name?: \"discard\" | \"save\"} → closes it; unsaved: the dialog asks unless name says",
    ),
    (
        "doctabs.tile",
        "Tile the open drawings",
        "",
        "{name: \"v\" | \"h\"} → panes for the open drawings (up to four), side by side or stacked, each fitted",
    ),
];

impl DocTabs {
    fn index_of(&self, id: u32) -> Option<usize> {
        self.list.iter().position(|t| t.id == id)
    }
    /// The slot of a tab that is not the open one.
    pub fn slot(&self, id: u32) -> Option<&Slot> {
        if id == self.active {
            return None;
        }
        self.list.iter().find(|t| t.id == id).and_then(|t| t.store.as_deref())
    }
}

/// The editors of the drawings waiting in their tabs (recovery copies follow them too).
pub fn waiting_editors(app: &UiApp) -> impl Iterator<Item = &edit::Editor> {
    app.doctabs.list.iter().filter(move |t| t.id != app.doctabs.active).filter_map(|t| t.store.as_deref()).filter_map(|s| s.editor.as_ref())
}

/// A tab's drawing name (the open one's from the window).
pub fn name_of(app: &UiApp, id: u32) -> String {
    let ed = if id == app.doctabs.active { app.editor.as_ref() } else { app.doctabs.slot(id).and_then(|s| s.editor.as_ref()) };
    ed.map_or_else(|| "Drawing1".to_string(), |e| e.name.clone())
}

/// A tab's drawing has unsaved changes.
pub fn dirty_of(app: &UiApp, id: u32) -> bool {
    let ed = if id == app.doctabs.active { app.editor.as_ref() } else { app.doctabs.slot(id).and_then(|s| s.editor.as_ref()) };
    ed.is_some_and(|e| e.eng.doc().dirty())
}

/// doctabs.js `pristine()`: the open drawing is a new one nobody has touched (New and Open use it).
pub fn pristine(app: &UiApp) -> bool {
    let Some(ed) = app.editor.as_ref() else { return true };
    let d = ed.eng.doc();
    !d.dirty() && d.count() == 0 && d.layouts.is_empty() && app.source.is_none() && d.undo_stack.is_empty()
}

/// doctabs.js `freshName()`: Drawing1, Drawing2 … that no open drawing has.
fn fresh_name(app: &UiApp) -> String {
    let used: HashSet<String> = app.doctabs.list.iter().map(|t| name_of(app, t.id)).collect();
    let mut n = 1u32;
    while used.contains(&format!("Drawing{n}")) {
        n = n.saturating_add(1);
    }
    format!("Drawing{n}")
}

/// A new empty drawing (a slot of its own) named `name`.
fn new_slot(app: &UiApp, name: &str) -> Slot {
    let mut ed = edit::Editor::new(cc_cad::Doc::new(), name);
    edit::carry_history(&ed, app.editor.as_ref());
    ed.eng.set_zone(Some(app.zone));
    Slot { editor: Some(ed), views: vec![None], pending_fit: true, save_format: app.save_format.clone(), ..Slot::default() }
}

/// doctabs.js `tabs.add({ name })`: a new drawing in its own tab, opened → its id.
pub fn add(app: &mut UiApp, name: Option<&str>) -> u32 {
    let name = name.map_or_else(|| fresh_name(app), str::to_string);
    let slot = new_slot(app, &name);
    let id = app.doctabs.next;
    app.doctabs.next = app.doctabs.next.saturating_add(1);
    app.doctabs.list.push(Tab { id, store: Some(Box::new(slot)) });
    switch_to(app, id, false);
    app.sync_editor();
    id
}

/// doctabs.js `tabs.switchTo(id, { keepView })`: that tab's drawing becomes the open one, shown as it was last seen
/// (or whole) unless a pane brings its own view (`keep_view`). → false when it already was, or there is no such tab.
pub fn switch_to(app: &mut UiApp, id: u32, keep_view: bool) -> bool {
    let from = app.doctabs.active;
    if id == from {
        return false;
    }
    let Some(i) = app.doctabs.index_of(id) else { return false };
    let Some(to) = app.doctabs.list.get_mut(i).and_then(|t| t.store.take()) else { return false };
    // the 3D tab and an edit in progress belong to the drawing left
    if crate::cad3dtab::showing(app) {
        app.run_ui("tab3d.close", &Params::default());
    }
    if let Some(ed) = app.editor.as_mut() {
        ed.escape();
    }
    // panes made before a second drawing opened show the one being left; the live view goes into the active pane
    let l = paneview::live(app);
    if let Some(p) = app.panes.split.panes.as_mut() {
        for q in p.list.iter_mut() {
            if q.doc.is_none() {
                q.doc = Some(from);
            }
        }
    }
    app.panes.split.sync(&l);
    let left = Slot::take(app);
    to.put(app);
    if let Some(j) = app.doctabs.index_of(from)
        && let Some(t) = app.doctabs.list.get_mut(j)
    {
        t.store = Some(Box::new(left));
    }
    app.doctabs.active = id;
    if !keep_view {
        if app.views.get(app.space).is_none_or(Option::is_none) {
            app.pending_fit = true;
        }
        let vs = paneview::live(app).vs;
        let space = app.space;
        if let Some(p) = app.panes.split.panes.as_mut()
            && let Some(q) = p.list.get_mut(p.active)
        {
            q.doc = Some(id);
            q.space = space;
            q.vs = vs;
        }
    }
    app.sync_editor();
    true
}

/// cadcraft.js `newDrawing()`: a new drawing in its own tab, unless the open one is untouched (then it is used).
pub fn new_drawing(app: &mut UiApp) {
    if app.editor.is_some() && !pristine(app) {
        add(app, None);
        let name = name_of(app, app.doctabs.active);
        app.message = lang::t(&lang::CADCRAFT, "doctabs.new.done", &[("name", name)]);
        return;
    }
    let name = app.editor.as_ref().map_or_else(|| "Drawing1".to_string(), |e| e.name.clone());
    let slot = new_slot(app, &name);
    slot.put(app);
    app.message = lang::cad("app.new_drawing_2");
}

/// An open of a drawing to edit while one is open: a tab of its own unless the open one is untouched (cadcraft.js
/// openLocal, "its own tab, as AutoCAD").
pub fn before_open(app: &mut UiApp, file: &str) {
    if app.editor.is_some() && !pristine(app) {
        let stem = file.rsplit_once('.').map_or(file, |(a, _)| a).to_string();
        add(app, Some(&stem));
    }
}

/// doctabs.js `tabs.close(id)` after its question (`ok`: discard the unsaved changes, or they were saved) → closed.
pub fn close(app: &mut UiApp, id: u32) -> bool {
    let Some(i) = app.doctabs.index_of(id) else { return false };
    app.doctabs.asking = None;
    if app.doctabs.list.len() == 1 {
        // the last drawing: a new empty one takes its place
        let slot = new_slot(app, "Drawing1");
        slot.put(app);
        paneview::set_single(app);
        app.sync_editor();
        return true;
    }
    if id == app.doctabs.active {
        let next = app.doctabs.list.get(i + 1).or_else(|| i.checked_sub(1).and_then(|k| app.doctabs.list.get(k))).map(|t| t.id);
        if let Some(n) = next {
            switch_to(app, n, false);
        }
    }
    if let Some(i) = app.doctabs.index_of(id) {
        app.doctabs.list.remove(i);
    }
    // panes that showed it show the open drawing now, fitted
    let active = app.doctabs.active;
    let space = app.space;
    let shown: Vec<usize> = app
        .panes
        .split
        .panes
        .as_ref()
        .map(|p| p.list.iter().enumerate().filter(|(_, q)| q.doc == Some(id)).map(|(k, _)| k).collect())
        .unwrap_or_default();
    for k in shown {
        if let Some(q) = app.panes.split.panes.as_mut().and_then(|p| p.list.get_mut(k)) {
            q.doc = Some(active);
            q.space = space;
        }
        fit_pane(app, k);
    }
    true
}

/// doctabs.js `tabs.tile(dir)`: panes for the open drawings (up to four) side by side ('v') or stacked ('h'), each
/// fitted to its own drawing; the open one stays active. → false when fewer than two are open.
pub fn tile(app: &mut UiApp, dir: &str) -> bool {
    let mut ids: Vec<u32> = app.doctabs.list.iter().map(|t| t.id).take(4).collect();
    let active = app.doctabs.active;
    if !ids.contains(&active)
        && let Some(last) = ids.last_mut()
    {
        *last = active;
    }
    if ids.len() < 2 {
        return false;
    }
    let layout = match (ids.len(), dir == "h") {
        (2, true) => "2h",
        (2, false) => "2v",
        (3, true) => "3l",
        (3, false) => "3r",
        _ => "4",
    };
    let at = ids.iter().position(|x| *x == active).unwrap_or(0);
    paneview::set_panes(app, layout, at);
    for (k, id) in ids.iter().enumerate() {
        let space = if *id == active { app.space } else { app.doctabs.slot(*id).map_or(0, |s| s.space) };
        if let Some(q) = app.panes.split.panes.as_mut().and_then(|p| p.list.get_mut(k)) {
            q.doc = Some(*id);
            q.space = space;
        }
        fit_pane(app, k);
    }
    true
}

/// view.js `fitPaneTo(p)`: pane k fitted to its own drawing's space (the active pane's view becomes the window's).
fn fit_pane(app: &mut UiApp, k: usize) {
    let Some(q) = app.panes.split.panes.as_ref().and_then(|p| p.list.get(k).copied()) else { return };
    let Some(r) = q.rect else { return };
    let mine = q.doc.is_none_or(|id| id == app.doctabs.active);
    if mine && app.doc.is_none() {
        // the open drawing's display is not made yet: the window fits it when it draws
        if app.panes.split.panes.as_ref().is_some_and(|p| p.active == k) {
            app.pending_fit = true;
        }
        return;
    }
    let doc = if mine { app.doc.as_ref() } else { q.doc.and_then(|id| app.doctabs.slot(id)).and_then(|s| s.doc.as_ref()) };
    let ppp = paneview::ppp(app);
    let mut v = View2D { width: r.w * ppp, height: r.h * ppp, ..View2D::default() };
    match doc.and_then(|d| d.drawing.spaces.get(q.space)) {
        Some(s) => {
            let b = s.paper.unwrap_or(s.bounds);
            if b.is_empty() {
                v.fit(0.0, 0.0, 100.0, 60.0, 0.03);
            } else {
                v.fit(b.min_x, b.min_y, b.max_x, b.max_y, 0.03);
            }
        }
        None => v.fit(0.0, 0.0, 100.0, 60.0, 0.03),
    }
    let vs = paneview::vs_of(&v, ppp);
    let active = app.panes.split.panes.as_ref().is_some_and(|p| p.active == k);
    if let Some(qq) = app.panes.split.panes.as_mut().and_then(|p| p.list.get_mut(k)) {
        qq.vs = vs;
    }
    if active {
        paneview::load_active(app);
    }
}

/// A press into pane i (cadcraft.js `switchPane`): a pane showing another open drawing opens that drawing, the pane
/// keeping its view; → whether pane i was another drawing's.
pub fn pane_pressed(app: &mut UiApp, i: usize) -> bool {
    let Some(doc) = app.panes.split.panes.as_ref().and_then(|p| p.list.get(i)).and_then(|q| q.doc) else { return false };
    if doc == app.doctabs.active || app.doctabs.index_of(doc).is_none() {
        return false;
    }
    switch_to(app, doc, true);
    paneview::activate_nosync(app, i);
    true
}

/// Run one of the strip's commands.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    // the tab by index (or a number as the name / factor: `--run doctabs.switch=1`)
    let idx = || {
        params
            .index
            .or_else(|| params.factor.filter(|f| f.is_finite() && *f >= 0.0 && *f < 1e6).map(|f| f as usize))
            .ok_or(format!("{id}: index is required"))
    };
    match id {
        "doctabs.new" => {
            if app.editor.is_none() {
                return Err("doctabs.new: no drawing is open for editing (file.new)".into());
            }
            add(app, None);
            let name = name_of(app, app.doctabs.active);
            app.message = lang::t(&lang::CADCRAFT, "doctabs.new.done", &[("name", name)]);
        }
        "doctabs.switch" => {
            let i = idx()?;
            let t = app.doctabs.list.get(i).map(|t| t.id).ok_or(format!("{id}: no tab {i}"))?;
            switch_to(app, t, false);
        }
        "doctabs.close" => {
            let i = idx()?;
            let t = app.doctabs.list.get(i).map(|t| t.id).ok_or(format!("{id}: no tab {i}"))?;
            match params.name.as_deref() {
                Some("discard") => {
                    close(app, t);
                }
                Some("save") => {
                    if t != app.doctabs.active {
                        switch_to(app, t, false);
                    }
                    app.run_ui("file.save", &Params::default());
                    if !dirty_of(app, t) {
                        close(app, t);
                    }
                }
                _ if dirty_of(app, t) => app.doctabs.asking = Some(t),
                _ => {
                    close(app, t);
                }
            }
        }
        "doctabs.tile" => {
            tile(app, params.name.as_deref().unwrap_or("v"));
        }
        "doctabs.info" => {
            // the tabs and panes as JSON, each pane's view too (scripts, agents, the harness)
            let mut st = state(app);
            let views: Vec<Value> = app
                .panes
                .split
                .shown(&paneview::live(app))
                .iter()
                .map(|q| json!({ "cx": q.vs.cx, "cy": q.vs.cy, "zoom": q.vs.zoom }))
                .collect();
            if let Some(o) = st.as_object_mut() {
                o.insert("views".into(), Value::Array(views));
            }
            app.message = st.to_string();
        }
        _ => return Err(format!("unknown command {id}")),
    }
    Ok(())
}

/// The tabs and panes as the parity harness reads them (doctabsdiff.mjs).
pub fn state(app: &UiApp) -> Value {
    let ix = |id: u32| app.doctabs.index_of(id).map_or(-1, |i| i as i64);
    let active = app.doctabs.active;
    // the active pane as the window shows it (its record is synced when another pane becomes active)
    let shown = app.panes.split.shown(&paneview::live(app));
    let panes = app.panes.split.panes.as_ref().map(|p| {
        json!({
            "layout": p.layout, "fx": p.fx, "fy": p.fy, "active": p.active,
            "list": shown.iter().map(|q| json!({ "doc": ix(q.doc.unwrap_or(active)), "space": q.space })).collect::<Vec<_>>(),
        })
    });
    json!({
        "tabs": app.doctabs.list.iter().map(|t| name_of(app, t.id)).collect::<Vec<_>>(),
        "active": ix(active),
        "space": app.space,
        "panes": panes,
    })
}

/// Time spent in the tab operations (switch, close, tile, a press into a pane) of the cases run, ms (the harness's timing).
pub static TAB_OPS_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// One doctabsdiff.mjs case: `{W, H, ops}` with ops new | open <file> <doc JSON> | sheet <k> | switch <i> | close <i> |
/// tile v|h | split <layout> | act <pane> | state → the state after each op.
pub fn run_case(case: &Value) -> Value {
    let mut app = UiApp::default();
    let num = |v: Option<&Value>| v.and_then(Value::as_f64).unwrap_or(0.0);
    app.panes.split.w = num(case.get("W"));
    app.panes.split.h = num(case.get("H"));
    app.panes.ppp = 1.0;
    let _ = app.run("file.new", &Params::default());
    let mut out = Vec::new();
    for op in case.get("ops").and_then(Value::as_array).into_iter().flatten() {
        let a = |i: usize| op.get(i);
        let int = |i: usize| a(i).and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(usize::MAX);
        let kind = a(0).and_then(Value::as_str).unwrap_or("");
        app.message.clear();
        let t0 = web_time::Instant::now();
        let timed = matches!(kind, "switch" | "close" | "tile" | "act");
        match kind {
            "new" => {
                let _ = app.run("file.new", &Params::default());
            }
            "open" => {
                let name = a(1).and_then(Value::as_str).unwrap_or("x.dxf").to_string();
                let mut d = cc_cad::Doc::new();
                d.load(a(2).unwrap_or(&Value::Null));
                if let Ok((text, _)) = cc_io_cad::dxf_write::write_dxf(&d) {
                    app.open_file(&ImportedFile { name, bytes: text.into_bytes(), dir: None });
                }
            }
            "sheet" => {
                let _ = app.run("space.select", &Params { index: Some(int(1)), ..Default::default() });
            }
            "switch" | "close" => {
                let name = (kind == "close").then(|| "discard".to_string());
                let _ = run(&mut app, &format!("doctabs.{kind}"), &Params { index: Some(int(1)), name, ..Default::default() });
            }
            "tile" => {
                tile(&mut app, a(1).and_then(Value::as_str).unwrap_or("v"));
            }
            "split" => {
                let l = a(1).and_then(Value::as_str).unwrap_or("single").to_string();
                paneview::set_panes(&mut app, &l, 0);
            }
            "act" => paneview::activate(&mut app, int(1)),
            _ => {}
        }
        if timed {
            let ns = u64::try_from(t0.elapsed().as_nanos()).unwrap_or(u64::MAX);
            TAB_OPS_NS.fetch_add(ns, std::sync::atomic::Ordering::Relaxed);
        }
        let mut st = state(&app);
        if let Some(o) = st.as_object_mut() {
            o.insert("said".into(), json!(if kind == "new" { app.message.clone() } else { String::new() }));
        }
        out.push(st);
    }
    Value::Array(out)
}

/// A small tool of the strip: a line icon drawn on the 20 px grid (`stacked`: two panes one over the other).
fn tool(ui: &mut egui::Ui, t: &crate::theme::Tokens, icon: &str, tip: &str, on: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(29.0, 29.0), egui::Sense::click());
    if on {
        ui.painter().rect_filled(r.shrink(3.0), t.radius, t.panel_darker);
    } else if resp.hovered() {
        ui.painter().rect_filled(r.shrink(3.0), t.radius, t.hover);
    }
    let ir = egui::Rect::from_center_size(r.center(), egui::vec2(16.0, 16.0));
    if icon == "stacked" {
        let st = egui::Stroke::new(1.3, t.text);
        let a = egui::Rect::from_min_max(ir.min + egui::vec2(1.5, 2.0), egui::pos2(ir.max.x - 1.5, ir.center().y - 0.8));
        let b = egui::Rect::from_min_max(egui::pos2(ir.min.x + 1.5, ir.center().y + 0.8), ir.max - egui::vec2(1.5, 2.0));
        ui.painter().rect_stroke(a, 1.0, st, egui::StrokeKind::Inside);
        ui.painter().rect_stroke(b, 1.0, st, egui::StrokeKind::Inside);
    } else {
        crate::ribbon::paint_icon(ui.painter(), ir, icon, t.text);
    }
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The strip over the drawing while two or more drawings are open (doctabs.js `render()`): a tab per drawing (its name,
/// ● when unsaved, × to close), then New, Tile side by side, Tile stacked and Link views (with panes).
pub fn strip(app: &mut UiApp, ui: &mut egui::Ui) {
    if app.editor.is_none() || app.doctabs.list.len() < 2 {
        return;
    }
    let t = app.tokens;
    let cad = |k: &str, v: &[(&str, String)]| lang::t(&lang::CADCRAFT, k, v);
    let tabs: Vec<(u32, String, bool, Option<String>)> = app
        .doctabs
        .list
        .iter()
        .map(|tb| {
            let src = if tb.id == app.doctabs.active {
                app.source.as_ref().map(|s| s.name.clone())
            } else {
                app.doctabs.slot(tb.id).and_then(|s| s.source.as_ref()).map(|s| s.name.clone())
            };
            (tb.id, name_of(app, tb.id), dirty_of(app, tb.id), src)
        })
        .collect();
    let active = app.doctabs.active;
    let panes = app.panes.split.panes.is_some();
    let linked = app.panes.split.linked;
    let mut act: Option<(&'static str, Params)> = None;
    let mut link = false;
    egui::Panel::top("cad_doctabs").exact_size(30.0).frame(egui::Frame::new().fill(t.panel)).show(ui, |ui| {
        egui::ScrollArea::horizontal().id_salt("cad_doctabs_scroll").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, (id, name, dirty, src)) in tabs.iter().enumerate() {
                    let on = *id == active;
                    let label = if *dirty { format!("{name} ●") } else { name.clone() };
                    let tip = src.clone().unwrap_or_else(|| name.clone());
                    let tip = if *dirty { format!("{tip} · {}", cad("doctabs.unsaved", &[])) } else { tip };
                    if crate::shell::tab(ui, &t, &label, on).on_hover_text(tip).clicked() && !on {
                        act = Some(("doctabs.switch", Params { index: Some(i), ..Default::default() }));
                    }
                    let close = cad("doctabs.close", &[("name", name.clone())]);
                    if tool(ui, &t, "x-lg", &close, false).clicked() {
                        act = Some(("doctabs.close", Params { index: Some(i), ..Default::default() }));
                    }
                    ui.add_space(6.0);
                }
                ui.separator();
                if tool(ui, &t, "plus-lg", &cad("doctabs.new", &[]), false).clicked() {
                    act = Some(("doctabs.new", Params::default()));
                }
                if tool(ui, &t, "layout-split", &cad("doctabs.tile.v", &[]), false).clicked() {
                    act = Some(("doctabs.tile", Params { name: Some("v".into()), ..Default::default() }));
                }
                if tool(ui, &t, "stacked", &cad("doctabs.tile.h", &[]), false).clicked() {
                    act = Some(("doctabs.tile", Params { name: Some("h".into()), ..Default::default() }));
                }
                if panes && tool(ui, &t, if linked { "link-45deg" } else { "link" }, &cad("doctabs.link", &[]), linked).clicked() {
                    link = true;
                }
            });
        });
    });
    if link && let Some(ed) = app.editor.as_ref() {
        let mut h = ed.hs.borrow_mut();
        h.linked = !h.linked;
    }
    if let Some((id, p)) = act {
        app.run_ui(id, &p);
    }
}

/// doctabs.js's question before closing a drawing with unsaved changes: Cancel, Close without saving, Save a copy first.
pub fn close_dialog(app: &mut UiApp, ctx: &egui::Context) {
    let Some(id) = app.doctabs.asking else { return };
    let Some(i) = app.doctabs.index_of(id) else {
        app.doctabs.asking = None;
        return;
    };
    let tok = app.tokens;
    let cad = |k: &str, v: &[(&str, String)]| lang::t(&lang::CADCRAFT, k, v);
    let name = name_of(app, id);
    let mut open = true;
    let mut pick: Option<&'static str> = None;
    crate::shell::Dialog::new(cad("doctabs.close.title", &[]))
        .shell_head()
        .id(egui::Id::unique("doctabs_close"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(440.0)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(cad("doctabs.close.text", &[("name", name.clone())])).color(tok.text));
            crate::shell::foot(ui, |ui| {
                if ui.add(crate::shell::Btn::new(cad("doctabs.close.cancel", &[]))).clicked() {
                    pick = Some("cancel");
                }
                if ui.add(crate::shell::Btn::new(cad("doctabs.close.discard", &[]))).clicked() {
                    pick = Some("discard");
                }
                if ui.add(crate::shell::Btn::primary(cad("doctabs.close.save", &[]))).clicked() {
                    pick = Some("save");
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                pick = Some("cancel");
            }
        });
    if !open {
        pick = Some("cancel");
    }
    match pick {
        Some("cancel") => app.doctabs.asking = None,
        Some(k) => {
            app.doctabs.asking = None;
            app.run_ui("doctabs.close", &Params { index: Some(i), name: Some(k.into()), ..Default::default() });
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dxf(lines: &[[f64; 4]]) -> Vec<u8> {
        let ents: Vec<Value> =
            lines.iter().map(|l| json!({"type": "line", "layer": "0", "x1": l[0], "y1": l[1], "x2": l[2], "y2": l[3]})).collect();
        let mut d = cc_cad::Doc::new();
        d.load(&json!({ "layers": [{"name": "0", "color": 7}], "entities": ents }));
        cc_io_cad::dxf_write::write_dxf(&d).unwrap().0.into_bytes()
    }

    /// M4.63: two drawings open, tiled; a press into the other drawing's pane opens it with the pane's view.
    #[test]
    fn tiled_drawings_and_a_press_into_the_other_one() {
        let mut app = UiApp::default();
        app.panes.split.w = 1000.0;
        app.panes.split.h = 600.0;
        app.panes.ppp = 1.0;
        app.run("file.new", &Params::default()).unwrap();
        app.open_file(&ImportedFile { name: "a.dxf".into(), bytes: dxf(&[[0.0, 0.0, 100.0, 50.0]]), dir: None });
        app.sync_editor();
        app.open_file(&ImportedFile { name: "b.dxf".into(), bytes: dxf(&[[1000.0, 1000.0, 1400.0, 1300.0]]), dir: None });
        app.sync_editor();
        assert_eq!(app.doctabs.list.len(), 2);
        assert!(tile(&mut app, "v"));
        let p = app.panes.split.panes.clone().unwrap();
        assert_eq!(p.list.iter().map(|q| q.doc).collect::<Vec<_>>(), vec![Some(app.doctabs.list[0].id), Some(app.doctabs.list[1].id)]);
        let a_view = p.list[0].vs;
        assert!(a_view.zoom > 0.0 && a_view.cx > 0.0 && a_view.cx < 100.0, "{a_view:?}");
        assert!(pane_pressed(&mut app, 0));
        assert_eq!(app.doctabs.active, app.doctabs.list[0].id);
        assert_eq!(name_of(&app, app.doctabs.active), "a");
        let v = app.views.get(app.space).copied().flatten().expect("the window's view is the pane's");
        assert!((v.cx - a_view.cx).abs() < 1e-9 && (v.zoom - a_view.zoom).abs() < 1e-9, "{v:?} vs {a_view:?}");
        assert!(!app.pending_fit);
    }
}
