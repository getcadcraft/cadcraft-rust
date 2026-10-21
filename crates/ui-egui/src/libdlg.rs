//! INSERT's dialogs and the library panel (cadcraft.js `chooseBlock` / `ask`, libpanel.js):
//!
//! - "Insert from <file>": INSERT, File picks a drawing; the dialog lists its blocks (with a filter) under "The whole
//!   drawing"; Insert hands the choice to the command (`host.chooseBlock`).
//! - a question with buttons (`host.choose`): block names already used → Cancel / Keep existing / Rename.
//! - the Library panel (LIBRARY; INSERT, Library asks it for an item): the Files… source reads an `item.json`
//!   (at most 8 MB; libplace.js checks the item), shows a card per item with Place, and the drawing's library credits
//!   (copy, CSV). LibraryCraft, other feeds and the Standard parts need getcadcraft.com's library or stdlib.js: the
//!   panel says so.
//!
//! Every control is a `choose.*` / `lib.*` command, so the CLI and assistants reach the same dialogs.

use cc_engine::cmd::libplace::{credits_csv, credits_text, item_problem, library_uses};
use egui::RichText;
use serde_json::Value;

use crate::{ImportedFile, Params, UiApp};

/// What a command asked the window to choose.
#[derive(Clone, Debug, PartialEq)]
pub enum ChooseReq {
    /// INSERT, File: the blocks of `file`
    Block { file: String, names: Vec<String> },
    /// a question with buttons (key, label)
    Ask { title: String, text: String, buttons: Vec<(String, String)> },
}

/// The open choice dialog.
#[derive(Clone, Debug)]
pub struct ChooseDlg {
    pub req: ChooseReq,
    pub filter: String,
    /// the block picked in the list (None: the whole drawing)
    pub sel: Option<String>,
}

/// One card of the panel.
#[derive(Clone, Debug)]
pub struct Card {
    pub name: String,
    pub maker: String,
    pub model: String,
    pub license: String,
    pub version: String,
    pub category: String,
    pub item: Value,
}

/// The library panel.
#[derive(Clone, Debug, Default)]
pub struct LibPanel {
    pub open: bool,
    /// INSERT, Library is waiting for a pick
    pub picking: bool,
    /// "files" | "librarycraft" | "standard" | "feeds"
    pub source: String,
    /// "browse" | "credits"
    pub view: String,
    pub cards: Vec<Card>,
    pub category: String,
    pub msg: String,
    /// a card placed from the open panel: INSERT, Library takes it at once
    pub queued: Option<Value>,
    /// a file picker for an item file is open (web)
    pub reading: bool,
}

pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    (
        "choose.answer",
        "Insert from a file / question: answer",
        "",
        "{name: block name | \"whole\" | \"cancel\" (Insert from a file); a button key, \"\" = close (a question)}",
    ),
    ("choose.filter", "Insert from a file: filter the blocks", "", "{name: text}"),
    ("lib.open", "Library", "", "{} → opens the library panel (LIBRARY does the same)"),
    ("lib.close", "Library: close", "", "{} → closes the panel (INSERT, Library then places nothing)"),
    ("lib.source", "Library: source", "", "{name: \"files\"|\"librarycraft\"|\"standard\"|\"feeds\"}"),
    ("lib.files", "Library: choose an item file", "", "{} → the file picker (item.json)"),
    ("lib.read", "Library: read an item file", "", "{name: path} → reads an item.json from disk (desktop)"),
    ("lib.category", "Library: category", "", "{name: category key, \"\" = every category}"),
    ("lib.place", "Library: place an item", "", "{name: card number (1 = first)} → INSERT, Library with that item"),
    ("lib.credits", "Library: credits", "", "{} → the library items this drawing uses, their attributions"),
    ("lib.back", "Library: back to the library", "", "{}"),
    ("lib.credits.csv", "Library: credits as CSV", "", "{} → saves \"<drawing> library credits.csv\""),
];

/// libpanel.js `licenseLabel`: the short names of the open licences, the language's words for the two kinds of terms.
fn license_label(id: &str) -> String {
    match id {
        "CC0-1.0" => "CC0".into(),
        "CC-BY-4.0" => "CC BY".into(),
        "CC-BY-SA-4.0" => "CC BY-SA".into(),
        "MAKER-TERMS" => t("lib.license.maker"),
        "TEAM-ONLY" => t("lib.license.team"),
        "" => t("lib.license.none"),
        other => other.to_string(),
    }
}

fn t(k: &str) -> String {
    crate::edit::lang_text(k, &[])
}
fn tv(k: &str, vars: &[(&str, String)]) -> String {
    crate::edit::lang_text(k, vars)
}

/// The engine waits for the dialog: hand it the answer (None: cancelled / closed).
fn answer(app: &mut UiApp, k: Option<String>) {
    app.choose = None;
    if let Some(ed) = app.editor.as_mut() {
        ed.eng.deliver_choice(k);
    }
    app.sync_editor();
}

/// The panel closes: a waiting INSERT, Library gets nothing.
fn close_panel(app: &mut UiApp) {
    app.lib.open = false;
    if std::mem::take(&mut app.lib.picking)
        && let Some(ed) = app.editor.as_mut()
    {
        ed.eng.deliver_library_item(None);
    }
    app.sync_editor();
}

/// libpanel.js `readFile` for an item.json → cards and the line that says what was read.
pub fn read_item_file(app: &mut UiApp, f: ImportedFile) {
    let ext = f.name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let lang = cc_engine::Lang::new(&Value::Null);
    let res: Result<Vec<Value>, String> = if ext == "json" {
        if f.bytes.len() > 8 * 1024 * 1024 {
            Err(t("lib.panel.item_file_most_8"))
        } else {
            serde_json::from_slice::<Value>(&f.bytes).map(|v| vec![v]).map_err(|_| tv("lib.panel.not_json", &[("file", f.name.clone())]))
        }
    } else {
        Err(tv("lib.panel.cannot_make", &[("ext", ext.to_uppercase())]))
    };
    match res {
        Err(e) => {
            app.lib.cards.clear();
            app.lib.msg = e;
        }
        Ok(items) => {
            let mut notes = Vec::new();
            let mut cards = Vec::new();
            for (k, it) in items.into_iter().enumerate() {
                if let Some(why) = item_problem(&it, &lang) {
                    let named = it.get("name").filter(|n| cc_cad::js::truthy(Some(n))).map(cc_cad::js::to_js_string);
                    notes.push(match named {
                        Some(n) => format!("\"{}\": {why}", cc_cad::js::slice16(&n, 80)),
                        None => why,
                    });
                    continue;
                }
                let s = |key: &str| it.get(key).filter(|v| cc_cad::js::truthy(Some(v))).map(cc_cad::js::to_js_string).unwrap_or_default();
                let name = it
                    .get("name")
                    .filter(|v| cc_cad::js::truthy(Some(v)))
                    .map_or_else(|| format!("Item {}", k.saturating_add(1)), cc_cad::js::to_js_string);
                let version = match it.get("version").and_then(Value::as_f64) {
                    Some(v) if v.fract() == 0.0 => cc_cad::js::num_to_string(v),
                    _ => "1".into(),
                };
                let license = it
                    .get("license")
                    .and_then(|l| l.get("id"))
                    .filter(|v| cc_cad::js::truthy(Some(v)))
                    .map(cc_cad::js::to_js_string)
                    .unwrap_or_default();
                cards.push(Card { name, maker: s("maker"), model: s("model"), license, version, category: s("category"), item: it });
            }
            let mut line = vec![if cards.is_empty() {
                tv("lib.panel.no_items_in", &[("file", f.name.clone())])
            } else {
                tv("lib.panel.items_in", &[("n", cards.len().to_string()), ("file", f.name.clone())])
            }];
            line.extend(notes.iter().take(3).cloned());
            if notes.len() > 3 {
                line.push(tv("lib.panel.more_notes", &[("n", (notes.len().saturating_sub(3)).to_string())]));
            }
            app.lib.cards = cards;
            app.lib.msg = line.join(" ");
        }
    }
    app.lib.view = "browse".into();
}

/// `choose.*` and `lib.*` commands.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    match id {
        "choose.answer" => {
            let dlg = app.choose.clone().ok_or("choose.answer: no question is open")?;
            let name = params.name.clone().unwrap_or_default();
            match dlg.req {
                ChooseReq::Block { names, .. } => {
                    let k = match name.as_str() {
                        "cancel" => None,
                        "" | "whole" => Some(cc_engine::engine::WHOLE.to_string()),
                        n => Some(
                            names.iter().find(|x| *x == n).cloned().ok_or(format!("choose.answer: no block called \"{n}\" in the list"))?,
                        ),
                    };
                    answer(app, k);
                }
                ChooseReq::Ask { buttons, .. } => {
                    if name.is_empty() {
                        answer(app, None);
                    } else {
                        let k = buttons
                            .iter()
                            .find(|(k, _)| *k == name)
                            .map(|(k, _)| k.clone())
                            .ok_or(format!("choose.answer: no button \"{name}\""))?;
                        answer(app, Some(k));
                    }
                }
            }
        }
        "choose.filter" => {
            let d = app.choose.as_mut().ok_or("choose.filter: no list is open")?;
            d.filter = params.name.clone().unwrap_or_default();
        }
        "lib.open" => {
            app.editor.as_ref().ok_or("lib.open: no drawing is open for editing")?;
            open_panel(app);
        }
        "lib.close" => close_panel(app),
        "lib.source" => {
            let s = params.name.clone().unwrap_or_default();
            if !matches!(s.as_str(), "files" | "librarycraft" | "standard" | "feeds") {
                return Err("lib.source: files | librarycraft | standard | feeds".into());
            }
            app.lib.source = s;
            app.lib.view = "browse".into();
            app.lib.msg.clear();
        }
        "lib.files" => {
            let pick = app.services.pick_files.as_mut().ok_or("lib.files: this window cannot open files")?;
            match pick("library", false) {
                Some(files) => {
                    if let Some(f) = files.into_iter().next() {
                        read_item_file(app, f);
                    }
                }
                None => app.lib.reading = true,
            }
        }
        "lib.read" => {
            let path = params.name.clone().ok_or("lib.read: name = the item file's path")?;
            let read = app.services.read_path.as_mut().ok_or("lib.read: this window cannot read files by path")?;
            let f = read(&path)?;
            read_item_file(app, f);
        }
        "lib.category" => app.lib.category = params.name.clone().unwrap_or_default(),
        "lib.place" => {
            let n: usize = params.name.as_deref().and_then(|s| s.parse().ok()).ok_or("lib.place: name = the card's number (1 = first)")?;
            let card = shown(&app.lib).get(n.saturating_sub(1)).cloned().cloned().ok_or(format!("lib.place: there is no card {n}"))?;
            place(app, card.item);
        }
        "lib.credits" => app.lib.view = "credits".into(),
        "lib.back" => app.lib.view = "browse".into(),
        "lib.credits.csv" => {
            let ed = app.editor.as_ref().ok_or("lib.credits.csv: no drawing is open for editing")?;
            let (csv, name) = {
                let d = ed.eng.doc();
                let uses = library_uses(&d);
                let lang = ed.eng.lang();
                let dn =
                    d.meta.get("name").filter(|v| cc_cad::js::truthy(Some(v))).map_or_else(|| "Drawing1".into(), cc_cad::js::to_js_string);
                (credits_csv(&uses, lang), tv("lib.panel.credits_file", &[("name", dn)]))
            };
            let save = app.services.save.as_mut().ok_or("Saving is not available here.")?;
            if let Some(to) = save(&name, csv.into_bytes())? {
                app.message = format!("Saved {to}");
            }
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

fn open_panel(app: &mut UiApp) {
    if app.lib.source.is_empty() {
        app.lib.source = "files".into();
    }
    app.lib.view = "browse".into();
    app.lib.open = true;
}

/// The cards the category filter lets through.
fn shown(lib: &LibPanel) -> Vec<&Card> {
    lib.cards.iter().filter(|c| lib.category.is_empty() || c.category == lib.category).collect()
}

/// Place: the waiting INSERT, Library gets the item; from the open panel, INSERT, Library starts with it.
fn place(app: &mut UiApp, item: Value) {
    app.lib.open = false;
    if std::mem::take(&mut app.lib.picking) {
        if let Some(ed) = app.editor.as_mut() {
            ed.eng.deliver_library_item(Some((item, None)));
        }
        app.sync_editor();
        return;
    }
    app.lib.queued = Some(item);
    app.run_ui_pub("cmd.line", &Params { name: Some("INSERT".into()), ..Default::default() });
    app.run_ui_pub("cmd.line", &Params { name: Some("L".into()), ..Default::default() });
}

/// The host side, once a frame: open what the engine asked for.
pub fn sync(app: &mut UiApp) {
    let Some(ed) = app.editor.as_ref() else { return };
    let (choose, open, pick) = {
        let mut h = ed.hs.borrow_mut();
        (h.choose_req.take(), std::mem::take(&mut h.lib_open), std::mem::take(&mut h.lib_pick))
    };
    if let Some(req) = choose {
        app.choose = Some(ChooseDlg { req, filter: String::new(), sel: None });
    }
    if open {
        open_panel(app);
    }
    if pick {
        open_panel(app);
        app.lib.picking = true;
    }
    if let Some(q) = app.lib.queued.take()
        && let Some(ed) = app.editor.as_ref()
    {
        ed.hs.borrow_mut().lib_queued = Some(q);
    }
    // web: the item file the browser's picker read
    if app.lib.reading
        && let Some(files) = app.services.picked.as_ref().and_then(|p| p.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take())
    {
        app.lib.reading = false;
        if let Some(f) = files.into_iter().next() {
            read_item_file(app, f);
        }
    }
}

/// The dialogs.
pub fn window(app: &mut UiApp, ctx: &egui::Context) {
    if app.editor.is_none() {
        return;
    }
    choose_window(app, ctx);
    if app.lib.open {
        lib_window(app, ctx);
    }
}

fn choose_window(app: &mut UiApp, ctx: &egui::Context) {
    let Some(dlg) = app.choose.clone() else { return };
    let tok = app.tokens;
    let mut act: Option<(&str, String)> = None;
    let mut open = true;
    match &dlg.req {
        ChooseReq::Block { file, names } => {
            egui::Window::new(tv("app.insertf.title", &[("file", file.clone())]))
                .id(egui::Id::unique("cad_insertf"))
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_size([420.0, 360.0])
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center())
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(if names.is_empty() {
                            t("app.file_has_no_named")
                        } else {
                            tv("app.insertf.count", &[("n", names.len().to_string())])
                        })
                        .color(tok.text_dim),
                    );
                    let mut filter = dlg.filter.clone();
                    if !names.is_empty() {
                        let r =
                            ui.add(egui::TextEdit::singleline(&mut filter).hint_text(t("app.filter_blocks")).desired_width(f32::INFINITY));
                        if r.changed() {
                            act = Some(("filter", filter.clone()));
                        }
                    }
                    let q = filter.trim().to_lowercase();
                    let whole = tv("app.insertf.whole", &[("name", file.rsplit_once('.').map_or(file.as_str(), |(a, _)| a).to_string())]);
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        let r = ui.selectable_label(dlg.sel.is_none(), whole);
                        if r.clicked() {
                            act = Some(("sel", String::new()));
                        }
                        if r.double_clicked() {
                            act = Some(("answer", "whole".into()));
                        }
                        for n in names.iter().filter(|n| q.is_empty() || n.to_lowercase().contains(&q)).take(2000) {
                            let r = ui.selectable_label(dlg.sel.as_deref() == Some(n.as_str()), n);
                            if r.clicked() {
                                act = Some(("sel", n.clone()));
                            }
                            if r.double_clicked() {
                                act = Some(("answer", n.clone()));
                            }
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(t("opt.cancel")).clicked() {
                            act = Some(("answer", "cancel".into()));
                        }
                        if ui.button(t("app.insertf.insert")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            act = Some(("answer", dlg.sel.clone().unwrap_or_else(|| "whole".into())));
                        }
                    });
                });
            if !open {
                act = Some(("answer", "cancel".into()));
            }
        }
        ChooseReq::Ask { title, text, buttons } => {
            egui::Window::new(title.clone())
                .id(egui::Id::unique("cad_ask"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(420.0)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center())
                .show(ctx, |ui| {
                    ui.label(text);
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        for (k, l) in buttons {
                            if ui.button(l).clicked() {
                                act = Some(("answer", k.clone()));
                            }
                        }
                    });
                });
            if !open {
                act = Some(("answer", String::new()));
            }
        }
    }
    match act {
        Some(("filter", f)) => {
            if let Some(d) = app.choose.as_mut() {
                d.filter = f;
            }
        }
        Some(("sel", s)) => {
            if let Some(d) = app.choose.as_mut() {
                d.sel = if s.is_empty() { None } else { Some(s) };
            }
        }
        Some(("answer", a)) => app.run_ui_pub("choose.answer", &Params { name: Some(a), ..Default::default() }),
        _ => {}
    }
}

fn lib_window(app: &mut UiApp, ctx: &egui::Context) {
    let tok = app.tokens;
    let mut open = true;
    let mut act: Vec<(&str, String)> = Vec::new();
    let mut copy: Option<String> = None;
    let lib = app.lib.clone();
    let uses = app.editor.as_ref().map(|e| library_uses(&e.eng.doc())).unwrap_or_default();
    egui::Window::new(t("lib.panel.library"))
        .id(egui::Id::unique("cad_library"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([620.0, 460.0])
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (k, l) in [
                    ("librarycraft", "LibraryCraft".to_string()),
                    ("files", t("lib.panel.files")),
                    ("standard", t("lib.panel.standard")),
                    ("feeds", t("lib.panel.other_feeds")),
                ] {
                    if ui.selectable_label(lib.source == k && lib.view == "browse", l).clicked() {
                        act.push(("lib.source", k.into()));
                    }
                }
                ui.separator();
                if lib.source == "files" && lib.view == "browse" && ui.button(t("lib.panel.choose_file").trim()).clicked() {
                    act.push(("lib.files", String::new()));
                }
                if ui.link(t("lib.panel.library_credits")).clicked() {
                    act.push(("lib.credits", String::new()));
                }
            });
            ui.separator();
            if lib.view == "credits" {
                ui.label(RichText::new(t("lib.panel.library_credits")).strong());
                if uses.is_empty() {
                    ui.label(t("lib.panel.drawing_has_no_library"));
                } else {
                    egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                        for u in &uses {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(if u.numbers.is_empty() { "–".to_string() } else { u.numbers.join(", ") }).strong());
                                ui.label(format!("{}  {}", u.name, u.license));
                                if ui.link(t("lib.panel.copy_attribution")).clicked() {
                                    copy = Some(u.attribution.clone());
                                }
                            });
                            ui.label(RichText::new(&u.attribution).color(tok.text_dim));
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button(t("lib.panel.copy_all")).clicked() {
                            copy = Some(credits_text(&uses));
                        }
                        if ui.button(t("lib.panel.download_csv")).clicked() {
                            act.push(("lib.credits.csv", String::new()));
                        }
                    });
                }
                if ui.link(t("lib.panel.back_library")).clicked() {
                    act.push(("lib.back", String::new()));
                }
            } else if lib.source != "files" {
                // LibraryCraft, other feeds and the Standard tab need getcadcraft.com's library API or stdlib.js
                ui.label(t("lib.edition.off"));
            } else {
                let cats: Vec<String> = {
                    let mut v: Vec<String> = lib.cards.iter().map(|c| c.category.clone()).filter(|c| !c.is_empty()).collect();
                    v.sort();
                    v.dedup();
                    v
                };
                if !cats.is_empty() {
                    let cur = if lib.category.is_empty() { t("lib.panel.every_category") } else { lib.category.clone() };
                    egui::ComboBox::new("lib_cat", "").selected_text(cur).show_ui(ui, |ui| {
                        if ui.selectable_label(lib.category.is_empty(), t("lib.panel.every_category")).clicked() {
                            act.push(("lib.category", String::new()));
                        }
                        for c in &cats {
                            if ui.selectable_label(lib.category == *c, c).clicked() {
                                act.push(("lib.category", c.clone()));
                            }
                        }
                    });
                }
                let cards = shown(&lib);
                if cards.is_empty() {
                    ui.label(t("lib.panel.choose_item_file_item"));
                    ui.label(RichText::new(t("lib.panel.files_read_here_computer")).color(tok.text_dim));
                } else {
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for (i, c) in cards.iter().enumerate() {
                            egui::Frame::new().stroke(egui::Stroke::new(1.0, tok.divider)).inner_margin(6.0).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new(&c.name).strong());
                                        let mm = [c.maker.as_str(), c.model.as_str()]
                                            .iter()
                                            .filter(|s| !s.is_empty())
                                            .copied()
                                            .collect::<Vec<_>>()
                                            .join(" · ");
                                        if !mm.is_empty() {
                                            ui.label(RichText::new(mm).color(tok.text_dim));
                                        }
                                        ui.label(
                                            RichText::new(format!("{}  v{}", license_label(&c.license), c.version)).color(tok.text_dim),
                                        );
                                    });
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button(t("lib.panel.place")).clicked() {
                                            act.push(("lib.place", i.saturating_add(1).to_string()));
                                        }
                                    });
                                });
                            });
                        }
                    });
                }
            }
            ui.separator();
            if !lib.msg.is_empty() {
                ui.label(RichText::new(&lib.msg).color(tok.text_dim));
            }
        });
    if let Some(s) = copy {
        ctx.copy_text(s);
    }
    if !open {
        act.push(("lib.close", String::new()));
    }
    for (id, name) in act {
        app.run_ui_pub(id, &Params { name: Some(name), ..Default::default() });
    }
}
