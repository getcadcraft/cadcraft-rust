//! CadCraft's customizing windows (appload.js, lisp/dcl-ui.js): APPLOAD's Load applications dialog (load files, the
//! loaded list with Unload, the Startup Suite, History), the alias editor (ALIASEDIT), DELBLOCKS' block list, and a
//! routine's DCL dialogs. Dropped or opened .lsp / .dcl files load; compiled and native files are refused with why.
//!
//! The dialogs' state that the engine reads back at once (a DCL tile's value) lives in the editor's `HostState`; the
//! rest here. Everything is reachable headless as `lisp.*` UI commands.

use std::collections::VecDeque;

use cc_lisp::host::{DclVal, DlgEvent, DlgOp, Tile};
use egui::RichText;

use crate::{ImportedFile, Params, UiApp, edit};

fn t(key: &str) -> String {
    edit::lang_text(key, &[])
}
fn tv(key: &str, vars: &[(&str, String)]) -> String {
    edit::lang_text(key, vars)
}

/// One application file kept by the History or the Startup Suite.
#[derive(Clone, Debug, PartialEq)]
pub struct Kept {
    pub name: String,
    pub text: String,
    pub kind: String,
    /// when it was loaded (ms since 1970)
    pub at: f64,
}

/// The windows' state.
#[derive(Clone, Debug, Default)]
pub struct LispUi {
    pub appload: bool,
    /// the last line the dialog said
    pub msg: String,
    pub history: Vec<Kept>,
    pub startup: Vec<Kept>,
    /// the alias editor's text (Some: open)
    pub alias: Option<String>,
    /// the person's aliases file (REINIT reads it)
    pub alias_file: String,
    /// DELBLOCKS' list: title, names, filter, picked
    pub blocks: Option<(String, Vec<String>, String, Vec<String>)>,
    /// web: APPLOAD's picker is open
    pub reading: bool,
}

/// Files above this size are refused (one application's text).
const MAX_FILE: usize = 2 * 1024 * 1024;

/// What a file is for LISP: Some(kind) to load, Err(why) refused, None not an application file.
fn app_kind(name: &str, size: usize) -> Option<Result<String, String>> {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let refused = match ext.as_str() {
        "fas" | "vlx" => Some("compiled"),
        "arx" | "crx" | "dbx" => Some("native"),
        "dvb" => Some("vba"),
        "craft" | "lua" | "py" => Some("craftscript"),
        _ => None,
    };
    if let Some(r) = refused {
        return Some(Err(tv(&format!("appload.refuse.{r}"), &[("name", name.to_string())])));
    }
    if ext != "lsp" && ext != "dcl" {
        return None;
    }
    if size > MAX_FILE {
        return Some(Err(tv("appload.refuse.size", &[("name", name.to_string())])));
    }
    Some(Ok(ext))
}

/// Load application files through _LISPLOAD (labelled APPLOAD), remembering them in the History.
fn load_files(app: &mut UiApp, files: Vec<ImportedFile>, startup: bool) {
    let mut apps = Vec::new();
    for f in files {
        match app_kind(&f.name, f.bytes.len()) {
            Some(Ok(kind)) => {
                apps.push(cc_engine::lisp::AppFile { name: f.name.clone(), text: cc_lisp::reader::decode_source(&f.bytes), kind })
            }
            Some(Err(why)) => say(app, &why),
            None => say(app, &tv("appload.refuse.type", &[("name", f.name.clone())])),
        }
    }
    if apps.is_empty() {
        return;
    }
    let now = cc_clock::now_ms();
    if !startup {
        for a in &apps {
            app.lisp.history.retain(|k| k.name != a.name);
            app.lisp.history.insert(0, Kept { name: a.name.clone(), text: a.text.clone(), kind: a.kind.clone(), at: now });
        }
        app.lisp.history.truncate(20);
    }
    if let Some(ed) = app.editor.as_mut() {
        ed.eng.lisp_load(apps, "APPLOAD");
        ed.refresh_prompt();
    }
    app.sync_editor();
}
fn say(app: &mut UiApp, s: &str) {
    app.lisp.msg = s.to_string();
    if let Some(ed) = app.editor.as_ref() {
        ed.hs.borrow_mut().push_log(s);
    }
}

/// A dropped or opened file: an application loads (true), anything else is not ours (false).
pub fn open_app_file(app: &mut UiApp, f: &ImportedFile) -> bool {
    if app.editor.is_none() {
        return false;
    }
    let ext = f.name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    if ext == "pgp" {
        app.lisp.alias = Some(cc_lisp::reader::decode_source(&f.bytes));
        return true;
    }
    if app_kind(&f.name, f.bytes.len()).is_none() {
        return false;
    }
    load_files(app, vec![f.clone()], false);
    true
}

/// `lisp.*` commands.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    let name = params.name.clone().unwrap_or_default();
    match id {
        "lisp.appload" => app.lisp.appload = true,
        "lisp.appload.close" => app.lisp.appload = false,
        "lisp.load" => match app.services.pick_files.as_mut().map(|f| f("lisp", true)) {
            Some(Some(files)) => load_files(app, files, false),
            Some(None) => app.lisp.reading = true,
            None => return Err("lisp.load: this window cannot open files".into()),
        },
        "lisp.load.text" => {
            // headless: name = "file.lsp\n<text>"
            let (file, text) = name.split_once('\n').ok_or("lisp.load.text: name = \"<file>\\n<text>\"")?;
            load_files(app, vec![ImportedFile { name: file.into(), bytes: text.as_bytes().to_vec(), dir: None }], false);
        }
        "lisp.unload" => {
            let ed = app.editor.as_mut().ok_or("lisp.unload: no drawing is being edited")?;
            let gone = ed.eng.lisp_session().unload(&name);
            let msg = tv("appload.unloaded", &[("name", name.clone()), ("n", gone.len().to_string())]);
            say(app, &msg);
        }
        "lisp.history.load" => {
            let k = app.lisp.history.iter().find(|k| k.name == name).cloned().ok_or("lisp.history.load: not in the History")?;
            load_files(app, vec![ImportedFile { name: k.name, bytes: k.text.into_bytes(), dir: None }], false);
        }
        "lisp.history.remove" => app.lisp.history.retain(|k| k.name != name),
        "lisp.startup.add" => {
            let ed = app.editor.as_mut().ok_or("lisp.startup.add: no drawing is being edited")?;
            let s = ed.eng.lisp_session();
            let f = s.loaded.borrow().iter().find(|f| f.name == name).cloned().ok_or("lisp.startup.add: not loaded")?;
            if !app.lisp.startup.iter().any(|k| k.name == name) {
                app.lisp.startup.push(Kept { name: f.name, text: f.text, kind: f.kind, at: cc_clock::now_ms() });
            }
            say(app, &tv("appload.startup.added", &[("name", name)]));
        }
        "lisp.startup.remove" => app.lisp.startup.retain(|k| k.name != name),
        "lisp.startup.run" => {
            let files = app
                .lisp
                .startup
                .iter()
                .map(|k| ImportedFile { name: k.name.clone(), bytes: k.text.clone().into_bytes(), dir: None })
                .collect();
            load_files(app, files, true);
        }
        "lisp.line" => {
            let ed = app.editor.as_mut().ok_or("lisp.line: no drawing is being edited")?;
            if !ed.eng.lisp_line(&name) {
                ed.eng.text(&name);
            }
            ed.refresh_prompt();
            app.sync_editor();
        }
        "lisp.alias.open" => app.lisp.alias = Some(app.lisp.alias_file.clone()),
        "lisp.alias.text" => app.lisp.alias = Some(name),
        "lisp.alias.cancel" => app.lisp.alias = None,
        "lisp.alias.save" => {
            let text = app.lisp.alias.take().ok_or("lisp.alias.save: the alias editor is not open")?;
            app.lisp.alias_file = text.clone();
            if let Some(ed) = app.editor.as_mut() {
                ed.hs.borrow_mut().alias_file = Some(text);
                ed.eng.run("REINIT");
                ed.refresh_prompt();
            }
        }
        "lisp.blocks.answer" => {
            // name: the names picked, comma separated ("" = cancel)
            let picked: Vec<String> = name.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            app.lisp.blocks = None;
            if let Some(ed) = app.editor.as_mut() {
                ed.eng.deliver_choice(if picked.is_empty() { None } else { Some(picked.join("\n")) });
                ed.refresh_prompt();
            }
            app.sync_editor();
        }
        "lisp.dcl" => {
            // a tile used: name = its key; factor = a value for it (list index, toggle 0 / 1)
            let ed = app.editor.as_mut().ok_or("lisp.dcl: no drawing is being edited")?;
            let val = params.factor.map(cc_cad::js::num_to_string);
            let mut h = ed.hs.borrow_mut();
            let v = h.dcl.as_mut().ok_or("lisp.dcl: no dialog is open")?;
            v.use_tile(&name, val);
            drop(h);
            deliver_dcl(app);
        }
        _ => return Err(format!("unknown command {id}")),
    }
    Ok(())
}

/// The host side, once a frame: open what the engine asked for, hand a waiting routine its dialog's events.
pub fn sync(app: &mut UiApp) {
    let Some(ed) = app.editor.as_mut() else { return };
    let (appload, alias, blocks) = {
        let mut h = ed.hs.borrow_mut();
        h.alias_file = Some(app.lisp.alias_file.clone());
        (std::mem::take(&mut h.appload_req), std::mem::take(&mut h.alias_req), h.blocks_req.take())
    };
    if appload {
        app.lisp.appload = true;
    }
    if alias {
        app.lisp.alias = Some(if app.lisp.alias_file.is_empty() { ed.eng.alias_template() } else { app.lisp.alias_file.clone() });
    }
    if let Some((title, names)) = blocks {
        app.lisp.blocks = Some((title, names, String::new(), Vec::new()));
    }
    if app.lisp.reading
        && let Some(files) = app.services.picked.as_ref().and_then(|p| p.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take())
    {
        app.lisp.reading = false;
        load_files(app, files, false);
    }
    deliver_dcl(app);
}

/// Once a frame: a long routine pauses every few thousand steps; run it on (Esc gets through between frames).
/// → true while it is still busy (the window paints again).
pub fn pump(app: &mut UiApp) -> bool {
    let Some(ed) = app.editor.as_mut() else { return false };
    let busy = ed.eng.pump();
    if busy {
        ed.refresh_prompt();
    }
    busy
}

/// A routine waiting for its dialog: hand it the next event (or the status the dialog closed with).
fn deliver_dcl(app: &mut UiApp) {
    let Some(ed) = app.editor.as_mut() else { return };
    let ev = {
        let mut h = ed.hs.borrow_mut();
        if !h.dcl_waiting {
            return;
        }
        let ev = match h.dcl.as_mut() {
            Some(v) => v.next_event(),
            None => Some(DlgEvent::Done(Some(h.dcl_closed.take().unwrap_or(0)))),
        };
        if ev.is_some() {
            h.dcl_waiting = false;
        }
        ev
    };
    if let Some(ev) = ev {
        ed.eng.deliver_dialog_event(ev);
        ed.refresh_prompt();
    }
}

// ---- DCL dialogs (dcl-ui.js) ----

/// One tile's state.
#[derive(Clone, Debug, Default)]
pub struct TileState {
    pub kind: String,
    pub key: Option<String>,
    pub label: String,
    pub value: String,
    pub items: Vec<String>,
    pub disabled: bool,
    pub group: Option<String>,
    pub multiple: bool,
}

/// An open DCL dialog: its tree, its tiles' values, the events not yet taken.
#[derive(Clone, Debug)]
pub struct DclView {
    pub tree: Tile,
    pub tiles: Vec<TileState>,
    pub events: VecDeque<DlgEvent>,
}

fn sattr(t: &Tile, k: &str) -> Option<String> {
    t.attr(k).map(|v| match v {
        DclVal::Str(s) | DclVal::Id(s) => s.clone(),
        DclVal::Num(n) => cc_cad::js::num_to_string(*n),
        DclVal::Bool(b) => b.to_string(),
    })
}
fn battr(t: &Tile, k: &str) -> bool {
    matches!(t.attr(k), Some(DclVal::Bool(true))) || matches!(t.attr(k), Some(DclVal::Str(s) | DclVal::Id(s)) if s == "true")
}

impl DclView {
    pub fn new(tree: &Tile) -> DclView {
        let mut v = DclView { tree: tree.clone(), tiles: Vec::new(), events: VecDeque::new() };
        for c in &tree.children {
            v.collect(c, None);
        }
        v
    }
    fn collect(&mut self, t: &Tile, group: Option<String>) {
        let key = sattr(t, "key");
        let radio = t.kind.contains("radio") && t.kind != "radio_button";
        let next_group = if radio { Some(key.clone().unwrap_or_else(|| format!("__radio{}", self.tiles.len()))) } else { group.clone() };
        self.tiles.push(TileState {
            kind: t.kind.clone(),
            key: key.clone(),
            label: sattr(t, "label").unwrap_or_default(),
            value: sattr(t, "value").unwrap_or_default(),
            items: Vec::new(),
            disabled: false,
            group,
            multiple: battr(t, "multiple_select"),
        });
        for c in &t.children {
            self.collect(c, next_group.clone());
        }
    }
    fn tile_mut(&mut self, key: &str) -> Option<&mut TileState> {
        self.tiles.iter_mut().find(|x| x.key.as_deref() == Some(key))
    }
    fn tile(&self, key: &str) -> Option<&TileState> {
        self.tiles.iter().find(|x| x.key.as_deref() == Some(key))
    }
    /// A tile's value as get_tile reads it.
    pub fn get(&self, key: &str) -> String {
        let Some(tl) = self.tile(key) else { return String::new() };
        match tl.kind.as_str() {
            "button" | "image" | "image_button" => String::new(),
            k if k.contains("radio") && k != "radio_button" => self
                .tiles
                .iter()
                .find(|x| x.kind == "radio_button" && x.group.as_deref() == Some(key) && x.value == "1")
                .and_then(|x| x.key.clone())
                .unwrap_or_default(),
            "text" | "text_part" | "paragraph" | "errtile" => tl.label.clone(),
            _ => tl.value.clone(),
        }
    }
    pub fn op(&mut self, op: DlgOp) -> Option<String> {
        match op {
            DlgOp::Done(_) => Some(String::new()),
            DlgOp::Set(k, v) => {
                let group_member = self.tiles.iter().any(|x| x.kind == "radio_button" && x.group.as_deref() == Some(k.as_str()));
                if group_member {
                    for x in self.tiles.iter_mut().filter(|x| x.kind == "radio_button" && x.group.as_deref() == Some(k.as_str())) {
                        x.value = if x.key.as_deref() == Some(v.as_str()) { "1".into() } else { "0".into() };
                    }
                } else if let Some(tl) = self.tile_mut(&k) {
                    match tl.kind.as_str() {
                        "button" | "text" | "text_part" | "paragraph" | "errtile" => tl.label = v,
                        _ => tl.value = v,
                    }
                }
                Some(String::new())
            }
            DlgOp::Get(k) => Some(self.get(&k)),
            DlgOp::Attr(..) => Some(String::new()),
            DlgOp::Mode(k, m) => {
                if let Some(tl) = self.tile_mut(&k)
                    && (m == 0 || m == 1)
                {
                    tl.disabled = m == 1;
                }
                Some(String::new())
            }
            DlgOp::List { key, op, index, items } => {
                if let Some(tl) = self.tile_mut(&key) {
                    match op {
                        3 => tl.items = items,
                        1 => {
                            if let (Some(slot), Some(first)) =
                                (usize::try_from(index).ok().and_then(|i| tl.items.get_mut(i)), items.into_iter().next())
                            {
                                *slot = first;
                            }
                        }
                        _ => tl.items.extend(items),
                    }
                }
                Some(String::new())
            }
        }
    }
    /// The person used a tile (a press, a pick, a check): its value and an event for the routine.
    pub fn use_tile(&mut self, key: &str, val: Option<String>) {
        let Some(tl) = self.tile(key).cloned() else {
            if key == "accept" || key == "cancel" {
                self.events.push_back(DlgEvent::Event { key: key.into(), value: String::new(), reason: 1, x: 0, y: 0 });
            }
            return;
        };
        let value = match tl.kind.as_str() {
            "toggle" => {
                let v = val.unwrap_or_else(|| if tl.value == "1" { "0".into() } else { "1".into() });
                if let Some(x) = self.tile_mut(key) {
                    x.value = v.clone();
                }
                v
            }
            "radio_button" => {
                for x in self.tiles.iter_mut().filter(|x| x.kind == "radio_button" && x.group == tl.group) {
                    x.value = if x.key.as_deref() == Some(key) { "1".into() } else { "0".into() };
                }
                "1".into()
            }
            "list_box" | "popup_list" | "edit_box" | "slider" => {
                let v = val.unwrap_or(tl.value.clone());
                if let Some(x) = self.tile_mut(key) {
                    x.value = v.clone();
                }
                v
            }
            _ => String::new(),
        };
        self.events.push_back(DlgEvent::Event { key: key.into(), value, reason: 1, x: 0, y: 0 });
        if tl.kind == "radio_button"
            && let Some(g) = tl.group.clone()
            && self.tile(&g).is_some()
        {
            self.events.push_back(DlgEvent::Event { key: g, value: key.into(), reason: 1, x: 0, y: 0 });
        }
    }
    pub(crate) fn next_event(&mut self) -> Option<DlgEvent> {
        self.events.pop_front()
    }
}

// ---- the windows ----

pub fn window(app: &mut UiApp, ctx: &egui::Context) {
    if app.editor.is_none() {
        return;
    }
    if app.lisp.appload {
        appload_window(app, ctx);
    }
    if app.lisp.alias.is_some() {
        alias_window(app, ctx);
    }
    if app.lisp.blocks.is_some() {
        blocks_window(app, ctx);
    }
    dcl_window(app, ctx);
}

fn dialog<'a>(title: String, id: &str, ctx: &egui::Context) -> egui::Window<'a> {
    egui::Window::new(title)
        .id(egui::Id::unique(id))
        .collapsible(false)
        .resizable(true)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
}

fn appload_window(app: &mut UiApp, ctx: &egui::Context) {
    let tok = app.tokens;
    let loaded = app.editor.as_mut().map(|e| e.eng.lisp_session().loaded.borrow().clone()).unwrap_or_default();
    let mut act: Option<(&str, String)> = None;
    let mut open = true;
    dialog(t("appload.title"), "cad_appload", ctx).open(&mut open).default_size([520.0, 440.0]).show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui.button(t("appload.loadfile")).clicked() {
                act = Some(("lisp.load", String::new()));
            }
            ui.label(RichText::new(t("appload.drop")).small().color(tok.text_dim));
        });
        if !app.lisp.msg.is_empty() {
            ui.label(RichText::new(&app.lisp.msg).color(tok.text));
        }
        ui.separator();
        ui.label(RichText::new(t("appload.loaded.title")).strong());
        if loaded.is_empty() {
            ui.label(RichText::new(t("appload.loaded.none")).color(tok.text_dim));
        }
        for f in &loaded {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&f.name).monospace());
                let meta = if f.kind == "dcl" {
                    t("appload.dialogfile")
                } else if f.commands.is_empty() {
                    t("appload.nocommands")
                } else {
                    f.commands.join(", ")
                };
                ui.label(RichText::new(meta).small().color(tok.text_dim));
                if ui.small_button(t("appload.unload")).clicked() {
                    act = Some(("lisp.unload", f.name.clone()));
                }
                if app.lisp.startup.iter().any(|k| k.name == f.name) {
                    ui.label(RichText::new(t("appload.instartup")).small().color(tok.accent));
                } else if ui.small_button(t("appload.addstartup")).clicked() {
                    act = Some(("lisp.startup.add", f.name.clone()));
                }
            });
        }
        ui.separator();
        ui.label(RichText::new(t("appload.startup.title")).strong());
        ui.label(RichText::new(t("appload.startup.note")).small().color(tok.text_dim));
        if app.lisp.startup.is_empty() {
            ui.label(RichText::new(t("appload.startup.none")).color(tok.text_dim));
        }
        for k in &app.lisp.startup {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&k.name).monospace());
                if ui.small_button(t("appload.remove")).clicked() {
                    act = Some(("lisp.startup.remove", k.name.clone()));
                }
            });
        }
        ui.separator();
        ui.label(RichText::new(t("appload.history.title")).strong());
        if app.lisp.history.is_empty() {
            ui.label(RichText::new(t("appload.history.none")).color(tok.text_dim));
        }
        egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
            for k in &app.lisp.history {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&k.name).monospace());
                    if ui.small_button(t("appload.loadagain")).clicked() {
                        act = Some(("lisp.history.load", k.name.clone()));
                    }
                    if ui.small_button(t("appload.remove")).clicked() {
                        act = Some(("lisp.history.remove", k.name.clone()));
                    }
                });
            }
        });
        ui.collapsing(t("appload.refused.title"), |ui| {
            ui.label(RichText::new(t("appload.refused.text")).small().color(tok.text_dim));
        });
        ui.separator();
        if ui.button(t("appload.close")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            act = Some(("lisp.appload.close", String::new()));
        }
    });
    if !open {
        act = Some(("lisp.appload.close", String::new()));
    }
    if let Some((id, name)) = act {
        let r = run(app, id, &Params { name: Some(name), ..Params::default() });
        if let Err(e) = r {
            app.lisp.msg = e;
        }
    }
}

fn alias_window(app: &mut UiApp, ctx: &egui::Context) {
    let tok = app.tokens;
    let mut text = app.lisp.alias.clone().unwrap_or_default();
    let mut act: Option<&str> = None;
    let mut open = true;
    dialog(t("aliases.editor.title"), "cad_aliases", ctx).open(&mut open).default_size([520.0, 420.0]).show(ctx, |ui| {
        ui.label(RichText::new(t("aliases.editor.note")).small().color(tok.text_dim));
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            ui.add(egui::TextEdit::multiline(&mut text).font(egui::TextStyle::Monospace).desired_rows(14).desired_width(f32::INFINITY));
        });
        ui.horizontal(|ui| {
            if ui.button(t("aliases.cancel")).clicked() {
                act = Some("lisp.alias.cancel");
            }
            if ui.button(t("aliases.save")).clicked() {
                act = Some("lisp.alias.save");
            }
        });
    });
    if !open {
        act = Some("lisp.alias.cancel");
    }
    app.lisp.alias = Some(text);
    if let Some(id) = act {
        let _ = run(app, id, &Params::default());
    }
}

fn blocks_window(app: &mut UiApp, ctx: &egui::Context) {
    let Some((title, names, mut filter, mut picked)) = app.lisp.blocks.clone() else { return };
    let mut answer: Option<String> = None;
    let mut open = true;
    dialog(title, "cad_delblocks", ctx).open(&mut open).default_size([360.0, 420.0]).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(t("delblocks.dialog.filter"));
            ui.add(egui::TextEdit::singleline(&mut filter).hint_text("*").desired_width(160.0));
        });
        let q = filter.trim().to_uppercase().replace('*', "");
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for n in names.iter().filter(|n| q.is_empty() || n.to_uppercase().contains(&q)) {
                let on = picked.contains(n);
                let r = ui.selectable_label(on, n);
                if r.clicked() {
                    if on {
                        picked.retain(|x| x != n);
                    } else {
                        picked.push(n.clone());
                    }
                }
                if r.double_clicked() {
                    answer = Some(n.clone());
                }
            }
        });
        ui.label(RichText::new(t("delblocks.dialog.note")).small());
        ui.horizontal(|ui| {
            if ui.button(t("aliases.cancel")).clicked() {
                answer = Some(String::new());
            }
            if ui.button(t("delblocks.dialog.ok")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                answer = Some(picked.join(","));
            }
        });
    });
    if !open {
        answer = Some(String::new());
    }
    match answer {
        Some(a) => {
            let _ = run(app, "lisp.blocks.answer", &Params { name: Some(a), ..Params::default() });
        }
        None => app.lisp.blocks = Some((app.lisp.blocks.clone().map(|b| b.0).unwrap_or_default(), names, filter, picked)),
    }
}

fn dcl_window(app: &mut UiApp, ctx: &egui::Context) {
    let Some(view) = app.editor.as_ref().and_then(|e| e.hs.borrow().dcl.clone()) else { return };
    let title = sattr(&view.tree, "label").unwrap_or_default();
    let mut used: Option<(String, Option<String>)> = None;
    let mut v = view;
    egui::Window::new(if title.is_empty() { " ".to_string() } else { title })
        .id(egui::Id::unique("cad_dcl"))
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            let kids = v.tree.children.clone();
            for c in &kids {
                tile_ui(ui, c, &mut v, &mut used);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                used = Some(("cancel".into(), None));
            }
        });
    if let Some(ed) = app.editor.as_ref() {
        let mut h = ed.hs.borrow_mut();
        if let Some(d) = h.dcl.as_mut() {
            d.tiles = v.tiles.clone();
        }
    }
    if let Some((key, val)) = used {
        let _ = run(app, "lisp.dcl", &Params { name: Some(key), factor: val.and_then(|s| s.parse().ok()), ..Params::default() });
    }
}

fn tile_ui(ui: &mut egui::Ui, t: &Tile, v: &mut DclView, used: &mut Option<(String, Option<String>)>) {
    let key = sattr(t, "key").unwrap_or_default();
    let label = v.tile(&key).map(|x| x.label.clone()).filter(|_| !key.is_empty()).unwrap_or_else(|| sattr(t, "label").unwrap_or_default());
    let enabled = !v.tile(&key).is_some_and(|x| x.disabled);
    match t.kind.as_str() {
        "row" | "boxed_row" | "radio_row" | "boxed_radio_row" | "concatenation" => {
            let body = |ui: &mut egui::Ui, v: &mut DclView, used: &mut Option<(String, Option<String>)>| {
                ui.horizontal(|ui| {
                    for c in &t.children {
                        tile_ui(ui, c, v, used);
                    }
                });
            };
            if t.kind.starts_with("boxed") {
                ui.group(|ui| {
                    if !label.is_empty() {
                        ui.label(RichText::new(&label).small());
                    }
                    body(ui, v, used);
                });
            } else {
                body(ui, v, used);
            }
        }
        "column" | "boxed_column" | "radio_column" | "boxed_radio_column" | "paragraph" | "dialog" => {
            let body = |ui: &mut egui::Ui, v: &mut DclView, used: &mut Option<(String, Option<String>)>| {
                ui.vertical(|ui| {
                    for c in &t.children {
                        tile_ui(ui, c, v, used);
                    }
                });
            };
            if t.kind.starts_with("boxed") {
                ui.group(|ui| {
                    if !label.is_empty() {
                        ui.label(RichText::new(&label).small());
                    }
                    body(ui, v, used);
                });
            } else {
                body(ui, v, used);
            }
        }
        "button" => {
            let text = if label.is_empty() { key.clone() } else { label };
            if ui.add_enabled(enabled, egui::Button::new(text)).clicked() && !key.is_empty() {
                *used = Some((key, None));
            }
        }
        "image_button" => {
            if ui.add_enabled(enabled, egui::Button::new("  ").min_size(egui::vec2(60.0, 30.0))).clicked() && !key.is_empty() {
                *used = Some((key, None));
            }
        }
        "edit_box" => {
            ui.horizontal(|ui| {
                let lab = ui.label(&label);
                if let Some(x) = v.tile_mut(&key) {
                    let r = ui.add_enabled(enabled, egui::TextEdit::singleline(&mut x.value).desired_width(160.0)).labelled_by(lab.id);
                    if r.lost_focus() {
                        *used = Some((key.clone(), None));
                    }
                }
            });
        }
        "toggle" => {
            let mut on = v.tile(&key).is_some_and(|x| x.value == "1");
            if ui.add_enabled(enabled, egui::Checkbox::new(&mut on, &label)).changed() {
                *used = Some((key, Some(if on { "1".into() } else { "0".into() })));
            }
        }
        "radio_button" => {
            let on = v.tile(&key).is_some_and(|x| x.value == "1");
            if ui.add_enabled(enabled, egui::RadioButton::new(on, &label)).clicked() {
                *used = Some((key, None));
            }
        }
        "list_box" | "popup_list" => {
            let (items, cur) = v.tile(&key).map(|x| (x.items.clone(), x.value.clone())).unwrap_or_default();
            ui.vertical(|ui| {
                if !label.is_empty() {
                    ui.label(&label);
                }
                if t.kind == "popup_list" {
                    let shown = cur.parse::<usize>().ok().and_then(|i| items.get(i)).cloned().unwrap_or_default();
                    egui::ComboBox::from_id_salt(("dcl", key.clone())).selected_text(shown).show_ui(ui, |ui| {
                        for (i, it) in items.iter().enumerate() {
                            if ui.selectable_label(cur == i.to_string(), it).clicked() {
                                *used = Some((key.clone(), Some(i.to_string())));
                            }
                        }
                    });
                } else {
                    egui::ScrollArea::vertical().id_salt(("dcl", key.clone())).max_height(140.0).show(ui, |ui| {
                        for (i, it) in items.iter().enumerate() {
                            if ui.selectable_label(cur.split_whitespace().any(|s| s == i.to_string()), it).clicked() {
                                *used = Some((key.clone(), Some(i.to_string())));
                            }
                        }
                    });
                }
            });
        }
        "slider" => {
            let lo = sattr(t, "min_value").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            let hi = sattr(t, "max_value").and_then(|s| s.parse::<f64>().ok()).unwrap_or(10000.0);
            let mut x = v.tile(&key).and_then(|x| x.value.parse::<f64>().ok()).unwrap_or(lo);
            if ui.add_enabled(enabled, egui::Slider::new(&mut x, lo.min(hi)..=hi.max(lo))).drag_stopped() {
                *used = Some((key, Some(cc_cad::js::num_to_string(x))));
            }
        }
        "spacer" => {
            ui.add_space(6.0);
        }
        "image" => {
            ui.add_space(20.0);
        }
        _ => {
            if !label.is_empty() {
                ui.label(label);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(app: &mut UiApp, s: &str) {
        app.run("cmd.line", &Params { name: Some(s.into()), ..Params::default() }).unwrap();
    }
    fn count(app: &UiApp) -> usize {
        app.editor.as_ref().map_or(0, |e| e.eng.doc().entities().count())
    }

    #[test]
    fn appload_dcl_and_typed_lisp() {
        let mut app = UiApp::default();
        app.run("file.new", &Params::default()).unwrap();
        let dcl = "d : dialog { label = \"T\"; : edit_box { key = \"tag\"; value = \"A1\"; } : toggle { key = \"t\"; } ok_cancel; }";
        let lsp = "(defun c:D (/ id) (setq id (load_dialog \"d.dcl\")) (new_dialog \"d\" id) (action_tile \"accept\" \"(setq tag (get_tile \\\"tag\\\") tg (get_tile \\\"t\\\")) (done_dialog 1)\") (if (= (start_dialog) 1) (entmake (list '(0 . \"POINT\") '(10 1 1)))) (princ))";
        for (n, t) in [("d.dcl", dcl), ("d.lsp", lsp)] {
            app.run("lisp.load.text", &Params { name: Some(format!("{n}\n{t}")), ..Params::default() }).unwrap();
        }
        assert_eq!(app.lisp.history.len(), 2);
        line(&mut app, "APPLOAD");
        assert!(app.lisp.appload);
        line(&mut app, "D");
        assert!(app.editor.as_ref().unwrap().hs.borrow().dcl.is_some());
        app.run("lisp.dcl", &Params { name: Some("t".into()), factor: Some(1.0), ..Params::default() }).unwrap();
        app.run("lisp.dcl", &Params { name: Some("accept".into()), ..Params::default() }).unwrap();
        assert!(app.editor.as_ref().unwrap().hs.borrow().dcl.is_none());
        assert_eq!(count(&app), 1);
        line(&mut app, "!tg");
        let ed = app.editor.as_ref().unwrap();
        assert!(ed.hs.borrow().log.iter().any(|l| l == "\"1\""), "{:?}", ed.hs.borrow().log);
        // a cancelled dialog: Esc presses cancel
        line(&mut app, "D");
        app.run("lisp.dcl", &Params { name: Some("cancel".into()), ..Params::default() }).unwrap();
        assert_eq!(count(&app), 1);
        assert!(!app.editor.as_ref().unwrap().eng.busy());
    }

    #[test]
    fn refused_files_say_why() {
        let mut app = UiApp::default();
        app.run("file.new", &Params::default()).unwrap();
        assert!(open_app_file(&mut app, &ImportedFile { name: "x.fas".into(), bytes: vec![1, 2], dir: None }));
        assert!(!app.lisp.msg.is_empty());
        assert!(!open_app_file(&mut app, &ImportedFile { name: "x.dwg".into(), bytes: vec![], dir: None }));
        app.run("lisp.unload", &Params { name: Some("none.lsp".into()), ..Params::default() }).unwrap();
    }
}
