//! The Sheet Set panel (sheetset-panel.js over cc-engine `sheetset` / `sheetset_io`): the open set's subsets and sheets
//! as a tree; a double click opens that drawing at that sheet; Alt+↑ / ↓ moves a sheet; the Set menu opens or starts a
//! set (a folder the person picks: the set file and the drawings are read and saved in place), adds sheets from the
//! folder's drawings, makes a new sheet from the set's template, subsets, rename and renumber, a numbering rule, the
//! set's fields, Publish to PDF, Update drawings (title blocks and tab names written into each file), the sheet list
//! on a sheet, Archive as a zip, and undo / redo of set changes. SHEETSET (SSM) shows it. Every action is an `ss.*`
//! command.
//!
//! Folders come through `Services::pick_folder` (desktop); the web build has none yet and says so.

use cc_engine::Lang;
use cc_engine::sheetset as ss;
use cc_engine::sheetset_io::{self as io, Folder};
use egui::RichText;
use serde_json::{Value, json};

use crate::{ImportedFile, Params, UiApp};

/// A dialog the panel has open.
#[derive(Clone, Debug, PartialEq)]
pub enum Form {
    New { name: String, pname: String, pnumber: String },
    Which { sets: Vec<String> },
    Add { files: Vec<String> },
    Layouts { file: String, layouts: Vec<(String, bool)> },
    NewSheet { number: String, title: String, template: String, choices: Vec<(String, String)> },
    Subset { name: String },
    RenameSubset { id: String, name: String },
    RenameSheet { id: String, number: String, title: String, description: String, fields: String },
    Renumber { which: String, prefix: String, start: String, step: String, digits: String },
    Numbering { rule: String, start: String, auto: bool },
    Props { name: String, pname: String, pnumber: String, custom: String, layout_names: String },
    Remove { kind: String, id: String, text: String },
    Publish { subset: String, name: String },
    Update { n: usize },
}

/// The panel's state (sheetset-panel.js `state`).
#[derive(Default)]
pub struct SheetSetPanel {
    pub open: bool,
    pub set: Option<Value>,
    pub folder: Option<String>,
    pub set_file: String,
    pub writable: bool,
    pub unsaved: bool,
    undo: Vec<Value>,
    redo: Vec<Value>,
    /// ("set" | "subset" | "sheet", id)
    pub sel: Option<(String, String)>,
    pub form: Option<Form>,
}

/// UI commands of the panel.
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("ss.show", "Sheet set", "", "{} → the Sheet Set panel (SHEETSET does the same)"),
    ("ss.hide", "Sheet set: close the panel", "", "{}"),
    ("ss.open", "Sheet set: open", "", "{name?: folder | \"folder|set file\"} → opens the set in that folder (no name: the folder picker)"),
    ("ss.new", "Sheet set: new", "", "{name: \"<set name>|<folder>|<project name>|<project number>\"} → starts a set in the folder"),
    ("ss.close", "Sheet set: close the set", "", "{}"),
    ("ss.select", "Sheet set: select", "", "{name: \"set\" | \"subset:<id>\" | \"sheet:<id>\"}"),
    ("ss.add", "Sheet set: add sheets", "", "{name: \"<file>|<layout>,<layout>…\"} → those sheets of a drawing in the folder"),
    ("ss.newsheet", "Sheet set: new sheet from template", "", "{name: \"<number>|<title>|<template file>|<template layout>\"}"),
    ("ss.subset", "Sheet set: new subset", "", "{name} → under the selected subset"),
    ("ss.rename", "Sheet set: rename", "", "{name: subset: \"<name>\"; sheet: JSON {number, title, description, fields}}"),
    ("ss.remove", "Sheet set: remove the selected sheet or subset", "", "{}"),
    ("ss.move", "Sheet set: move the selected sheet", "", "{name: \"up\" | \"down\" | \"subset:<id>\" | \"top\"}"),
    ("ss.renumber", "Sheet set: renumber", "", "{name: \"all|subset|<prefix>|<start>|<step>|<digits>\"}"),
    ("ss.numbering", "Sheet set: numbering rule", "", "{name: \"<rule>|<start>|yes|no\"}"),
    ("ss.props", "Sheet set: the set's fields", "", "{name: JSON {name, PROJECTNAME, PROJECTNUMBER, custom, layoutNames}}"),
    ("ss.publish", "Sheet set: publish to PDF", "", "{name?: \"all\" | \"subset\"} → one PDF of the set (or the selected subset)"),
    ("ss.update", "Sheet set: update drawings", "", "{} → title blocks and tab names written into the drawings"),
    ("ss.list", "Sheet set: put the sheet list on the selected sheet", "", "{}"),
    ("ss.archive", "Sheet set: archive as a zip", "", "{}"),
    ("ss.undo", "Sheet set: undo", "", "{}"),
    ("ss.redo", "Sheet set: redo", "", "{}"),
    ("ss.opensheet", "Sheet set: open a sheet", "", "{name: sheet id} → opens its drawing at that sheet"),
];

fn lang() -> Lang {
    crate::edit::lang_en_pub()
}
fn t(k: &str) -> String {
    crate::edit::lang_text(k, &[])
}
fn tv(k: &str, v: &[(&str, String)]) -> String {
    crate::edit::lang_text(k, v)
}

/// The command line hears the panel (sheetset-panel.js `print`).
fn print(app: &mut UiApp, s: &str) {
    match app.editor.as_ref() {
        Some(ed) => ed.hs.borrow_mut().push_log(s),
        None => app.message = s.to_string(),
    }
}

fn folder_of(app: &UiApp) -> Option<io::DirFolder> {
    let p = app.sheetset.folder.as_ref()?;
    Some(io::DirFolder { root: std::path::PathBuf::from(p), writable: app.sheetset.writable })
}
fn folder_name(app: &UiApp) -> String {
    folder_of(app).map(|f| f.name()).unwrap_or_default()
}
/// A name made safe for a file (`replace(/[\\/:*?"<>|]+/g, '-')`).
fn safe_name(s: &str) -> String {
    let mut o = String::new();
    let mut run = false;
    for c in s.chars() {
        if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            if !run {
                o.push('-');
            }
            run = true;
        } else {
            o.push(c);
            run = false;
        }
    }
    o
}
fn now_ms() -> f64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| (d.as_secs_f64() * 1000.0).floor())
}

fn subset_ids(set: &Value) -> Vec<(String, String)> {
    fn walk(l: &[Value], out: &mut Vec<(String, String)>, depth: usize) {
        if depth > 64 {
            return;
        }
        for g in l {
            out.push((
                g.get("id").and_then(Value::as_str).unwrap_or("").into(),
                g.get("name").and_then(Value::as_str).unwrap_or("").into(),
            ));
            walk(g.get("subsets").and_then(Value::as_array).map_or(&[][..], Vec::as_slice), out, depth + 1);
        }
    }
    let mut out = Vec::new();
    walk(set.get("subsets").and_then(Value::as_array).map_or(&[][..], Vec::as_slice), &mut out, 0);
    out
}

/// sheetset-panel.js `selectedSubset()`: the subset selected, or the one the selected sheet is in.
fn selected_subset(p: &SheetSetPanel) -> Option<String> {
    let set = p.set.as_ref()?;
    match &p.sel {
        Some((k, id)) if k == "subset" => Some(id.clone()),
        Some((k, id)) if k == "sheet" => ss::all_sheets(set)
            .into_iter()
            .find(|x| x.sheet.get("id").and_then(Value::as_str) == Some(id.as_str()))
            .and_then(|x| x.path.last().and_then(|g| g.get("id")).and_then(Value::as_str).map(str::to_string)),
        _ => None,
    }
}

/// `saveSet()`: the set file written in its folder (a read-only folder keeps it unsaved).
fn save_set(app: &mut UiApp) {
    let Some(set) = app.sheetset.set.clone() else { return };
    let text = ss::write_sheet_set(&set);
    if app.sheetset.writable
        && let Some(mut f) = folder_of(app)
    {
        let file = app.sheetset.set_file.clone();
        match f.write(&file, text.as_bytes()) {
            Some(Ok(())) => return,
            Some(Err(e)) => print(app, &tv("sheetset.save.failed", &[("file", file), ("why", e)])),
            None => {}
        }
    }
    app.sheetset.unsaved = true;
}

/// `edit(next, said)`: one set change (numbered again when the rule renumbers after every edit), undoable, saved.
fn edit(app: &mut UiApp, next: Value, said: Option<String>) {
    let next = if next.get("numbering").and_then(|n| n.get("auto")).and_then(Value::as_bool) == Some(true) {
        ss::number_by_rule(&next, None)
    } else {
        next
    };
    if app.sheetset.set.as_ref() == Some(&next) {
        return;
    }
    if let Some(cur) = app.sheetset.set.take() {
        app.sheetset.undo.push(cur);
        if app.sheetset.undo.len() > 100 {
            app.sheetset.undo.remove(0);
        }
    }
    app.sheetset.redo.clear();
    app.sheetset.set = Some(next);
    if let Some(s) = said {
        print(app, &s);
    }
    save_set(app);
}

fn open_from_folder(app: &mut UiApp, folder: &str, file: &str) -> bool {
    let l = lang();
    let mut f = io::DirFolder { root: std::path::PathBuf::from(folder), writable: true };
    let text = match f.read(file) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) => {
            print(app, &tv("sheetset.open.failed", &[("file", file.into()), ("why", e)]));
            return false;
        }
    };
    let (set, notes) = match ss::read_sheet_set(&text, &l) {
        Ok(x) => x,
        Err(e) => {
            print(app, &tv("sheetset.open.failed", &[("file", file.into()), ("why", e.0)]));
            return false;
        }
    };
    let n = ss::all_sheets(&set).len();
    let name = set.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let p = &mut app.sheetset;
    *p = SheetSetPanel {
        open: true,
        set: Some(set),
        folder: Some(folder.into()),
        set_file: file.into(),
        writable: true,
        ..Default::default()
    };
    print(app, &tv("sheetset.opened", &[("name", name), ("n", n.to_string()), ("folder", f.name())]));
    for x in notes {
        print(app, &format!("  {x}"));
    }
    true
}

fn need_set(app: &mut UiApp) -> Result<Value, String> {
    match app.sheetset.set.clone() {
        Some(s) => Ok(s),
        None => {
            let m = t("sheetset.noset");
            print(app, &m);
            Err(m)
        }
    }
}

/// `ss.*` commands.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    let l = lang();
    let name = params.name.clone().unwrap_or_default();
    match id {
        "ss.show" => app.sheetset.open = true,
        "ss.hide" => app.sheetset.open = false,
        "ss.open" => {
            let (folder, file) = match name.split_once('|') {
                Some((a, b)) => (a.to_string(), Some(b.to_string())),
                None if !name.is_empty() => (name.clone(), None),
                None => match app.services.pick_folder.as_mut() {
                    Some(pick) => match pick() {
                        Some(p) => (p, None),
                        None => return Ok(()),
                    },
                    None => {
                        print(app, "Sheet sets need a folder: the desktop app opens one (this build cannot yet).");
                        return Ok(());
                    }
                },
            };
            let file = match file {
                Some(f) => f,
                None => {
                    let mut f = io::DirFolder { root: std::path::PathBuf::from(&folder), writable: true };
                    let sets: Vec<String> = f
                        .list()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|p| p.to_lowercase().ends_with(ss::EXT) && !p.contains('/'))
                        .collect();
                    match sets.as_slice() {
                        [] => {
                            print(app, &tv("sheetset.none", &[("folder", f.name())]));
                            return Ok(());
                        }
                        [one] => one.clone(),
                        _ => {
                            app.sheetset.folder = Some(folder);
                            app.sheetset.form = Some(Form::Which { sets });
                            return Ok(());
                        }
                    }
                }
            };
            open_from_folder(app, &folder, &file);
        }
        "ss.new" => {
            let parts: Vec<&str> = name.split('|').collect();
            let sname = parts.first().copied().unwrap_or("").trim().to_string();
            let folder = parts.get(1).copied().unwrap_or("").to_string();
            if sname.is_empty() || folder.is_empty() {
                return Err("ss.new: name = \"<set name>|<folder>|<project name>|<project number>\"".into());
            }
            let mut set = ss::new_sheet_set(&sname, &l);
            let mut props = serde_json::Map::new();
            for (i, k) in [(2, "PROJECTNAME"), (3, "PROJECTNUMBER")] {
                if let Some(v) = parts.get(i).filter(|v| !v.is_empty()) {
                    props.insert(k.into(), json!(v));
                }
            }
            if let Some(o) = set.as_object_mut() {
                o.insert("properties".into(), Value::Object(props));
            }
            let file = format!("{}{}", cc_cad::js::slice16(&safe_name(&sname), 120), ss::EXT);
            let fname = io::DirFolder { root: std::path::PathBuf::from(&folder), writable: true }.name();
            let shown = set.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            app.sheetset = SheetSetPanel {
                open: true,
                set: Some(set),
                folder: Some(folder),
                set_file: file.clone(),
                writable: true,
                ..Default::default()
            };
            save_set(app);
            print(app, &tv("sheetset.created", &[("name", shown), ("file", file), ("folder", fname)]));
        }
        "ss.close" => {
            let open = app.sheetset.open;
            app.sheetset = SheetSetPanel { open, ..Default::default() };
        }
        "ss.select" => {
            app.sheetset.sel = match name.split_once(':') {
                Some((k, v)) => Some((k.to_string(), v.to_string())),
                None if name == "set" => Some(("set".into(), "set".into())),
                None => None,
            };
        }
        "ss.add" => {
            let set = need_set(app)?;
            let (file, list) = name.split_once('|').ok_or("ss.add: name = \"<file>|<layout>,<layout>…\"")?;
            let chosen: Vec<&str> = list.split(',').map(str::trim).filter(|x| !x.is_empty()).collect();
            if chosen.is_empty() {
                return Ok(());
            }
            let sub = selected_subset(&app.sheetset);
            let mut next = set;
            for lay in &chosen {
                next = ss::add_sheet(&next, &json!({ "file": file, "layout": lay, "title": lay }), sub.as_deref(), None, &l)
                    .map_err(|e| e.0)?
                    .0;
            }
            let said = tv("sheetset.add.done", &[("n", chosen.len().to_string()), ("file", file.to_string())]);
            edit(app, next, Some(said));
        }
        "ss.newsheet" => {
            let set = need_set(app)?;
            let p: Vec<&str> = name.split('|').collect();
            let (number, title, tfile, tlayout) = (
                p.first().copied().unwrap_or("").trim(),
                p.get(1).copied().unwrap_or("").trim(),
                p.get(2).copied().unwrap_or(""),
                p.get(3).copied().unwrap_or(""),
            );
            if number.is_empty() && title.is_empty() {
                return Ok(());
            }
            if tfile.is_empty() {
                print(app, &t("sheetset.newsheet.notemplate"));
                return Ok(());
            }
            if !app.sheetset.writable {
                print(app, &t("sheetset.readonly"));
                return Ok(());
            }
            let Some(mut f) = folder_of(app) else { return Ok(()) };
            let r = (|| -> Result<(Value, String), String> {
                let mut got = io::open_drawing(&mut f, tfile, &l)?;
                let lay = got.doc.layouts.iter().find(|x| x.name == tlayout).or(got.doc.layouts.first()).cloned();
                let Some(lay) = lay else { return Err(tv("sheetset.newsheet.nolayout", &[("file", tfile.into())])) };
                let sheet = json!({ "file": "", "layout": lay.name, "number": number, "title": title });
                let lname = ss::layout_name_for(&set, &sheet);
                let only = vec![cc_cad::doc::Layout { name: lname.clone(), ..lay.clone() }];
                let _ = got.doc.change(&l.s("sheetset.undo.rename"), |d| d.set_layouts(only));
                let dir = tfile.rfind('/').map_or(String::new(), |i| tfile.get(..=i).unwrap_or("").to_string());
                let base = cc_cad::js::slice16(&safe_name(&lname), 120);
                let path = format!("{dir}{}.dwg", if base.is_empty() { "Sheet".to_string() } else { base });
                let bytes = cc_io_cad::dwg_save::save_dwg(
                    &got.doc,
                    &cc_io_cad::dwg_save::SaveOpts { version: "2018".into(), ..Default::default() },
                )
                .map_err(|e| e.0)?
                .bytes;
                f.write(&path, &bytes).unwrap_or(Err(l.s("sheetset.io.readonly")))?;
                let sub = selected_subset(&app.sheetset);
                let (mut next, _) = ss::add_sheet(
                    &set,
                    &json!({ "file": path, "layout": lname, "number": number, "title": title }),
                    sub.as_deref(),
                    None,
                    &l,
                )
                .map_err(|e| e.0)?;
                let tpl = set.get("template");
                let same = tpl.and_then(|x| x.get("file")).and_then(Value::as_str) == Some(tfile)
                    && tpl.and_then(|x| x.get("layout")).and_then(Value::as_str) == Some(lay.name.as_str());
                if !same && let Some(o) = next.as_object_mut() {
                    o.insert("template".into(), json!({ "file": tfile, "layout": lay.name }));
                }
                Ok((next, path))
            })();
            match r {
                Ok((next, path)) => edit(app, next, Some(tv("sheetset.newsheet.done", &[("file", path)]))),
                Err(e) => print(app, &tv("sheetset.newsheet.failed", &[("why", e)])),
            }
        }
        "ss.subset" => {
            let set = need_set(app)?;
            if name.trim().is_empty() {
                return Ok(());
            }
            let parent = app.sheetset.sel.as_ref().filter(|(k, _)| k == "subset").map(|(_, i)| i.clone());
            let (next, _) = ss::add_subset(&set, &name, parent.as_deref(), &l);
            edit(app, next, None);
        }
        "ss.rename" => {
            let set = need_set(app)?;
            let Some((kind, sid)) = app.sheetset.sel.clone() else { return Ok(()) };
            if kind == "subset" {
                if !name.trim().is_empty() {
                    edit(app, ss::rename_subset(&set, &sid, &name), None);
                }
            } else if kind == "sheet" {
                let v: Value =
                    serde_json::from_str(&name).map_err(|_| "ss.rename: a sheet takes JSON {number, title, description, fields}")?;
                let mut p = serde_json::Map::new();
                for line in v.get("fields").and_then(Value::as_str).unwrap_or("").split('\n') {
                    if let Some((k, val)) = field_line(line) {
                        p.insert(k, json!(val));
                    }
                }
                let mut patch = serde_json::Map::new();
                for k in ["number", "title", "description"] {
                    if let Some(x) = v.get(k) {
                        patch.insert(k.into(), x.clone());
                    }
                }
                patch.insert("properties".into(), Value::Object(p));
                let s = ss::find_sheet(&set, &sid).unwrap_or(Value::Null);
                let mut shown = s.clone();
                if let (Some(o), Value::Object(pm)) = (shown.as_object_mut(), Value::Object(patch.clone())) {
                    for (k, x) in pm {
                        if k != "properties" {
                            o.insert(k, x);
                        }
                    }
                }
                let said = tv("sheetset.rename.done", &[("sheet", ss::sheet_label(&shown))]);
                edit(app, ss::update_sheet(&set, &sid, &Value::Object(patch), &l), Some(said));
            }
        }
        "ss.remove" => {
            let set = need_set(app)?;
            let Some((kind, sid)) = app.sheetset.sel.clone() else { return Ok(()) };
            app.sheetset.sel = None;
            if kind == "subset" {
                edit(app, ss::remove_subset(&set, &sid), None);
            } else if kind == "sheet" {
                let label = ss::find_sheet(&set, &sid).map(|s| ss::sheet_label(&s)).unwrap_or_default();
                edit(app, ss::remove_sheet(&set, &sid), Some(tv("sheetset.remove.done", &[("sheet", label)])));
            }
        }
        "ss.move" => {
            let set = need_set(app)?;
            let Some((kind, sid)) = app.sheetset.sel.clone().filter(|(k, _)| k == "sheet") else { return Ok(()) };
            let _ = kind;
            if let Some(g) = name.strip_prefix("subset:") {
                edit(app, ss::move_sheet(&set, &sid, Some(g), None), None);
            } else if name == "top" {
                edit(app, ss::move_sheet(&set, &sid, None, Some(0.0)), None);
            } else {
                let x = ss::all_sheets(&set).into_iter().find(|x| x.sheet.get("id").and_then(Value::as_str) == Some(sid.as_str()));
                let Some(x) = x else { return Ok(()) };
                let owner = x.path.last().cloned().unwrap_or_else(|| set.clone());
                let list = owner.get("sheets").and_then(Value::as_array).cloned().unwrap_or_default();
                let at = list.iter().position(|y| y.get("id").and_then(Value::as_str) == Some(sid.as_str())).unwrap_or(0);
                let to = if name == "up" { at.checked_sub(1) } else { Some(at.saturating_add(1)).filter(|i| *i < list.len()) };
                if let Some(to) = to {
                    let g = x.path.last().and_then(|g| g.get("id")).and_then(Value::as_str).map(str::to_string);
                    edit(app, ss::move_sheet(&set, &sid, g.as_deref(), Some(to as f64)), None);
                }
            }
        }
        "ss.renumber" => {
            let set = need_set(app)?;
            let p: Vec<&str> = name.split('|').collect();
            let num = |i: usize, d: f64| p.get(i).map_or(d, |s| cc_cad::js::str_to_number(s));
            let sub = selected_subset(&app.sheetset);
            let ids: Option<Vec<String>> = if p.first().copied() == Some("subset") {
                sub.map(|g| {
                    ss::all_sheets(&set)
                        .into_iter()
                        .filter(|x| x.path.iter().any(|y| y.get("id").and_then(Value::as_str) == Some(g.as_str())))
                        .filter_map(|x| x.sheet.get("id").and_then(Value::as_str).map(str::to_string))
                        .collect()
                })
            } else {
                None
            };
            let next = ss::renumber(&set, ids.as_deref(), p.get(1).copied().unwrap_or(""), num(2, 1.0), num(3, 1.0), num(4, 0.0));
            edit(app, next, Some(t("sheetset.renumber.done")));
        }
        "ss.numbering" => {
            let set = need_set(app)?;
            let p: Vec<&str> = name.split('|').collect();
            let rule = p.first().copied().unwrap_or("");
            if let Some(why) = ss::rule_problem(Some(&json!(rule))) {
                print(app, &t(why));
                return Ok(());
            }
            let nb = json!({ "rule": rule, "start": cc_cad::js::str_to_number(p.get(1).copied().unwrap_or("1")), "auto": p.get(2).copied() == Some("yes") });
            let n = ss::all_sheets(&set).len();
            edit(app, ss::number_by_rule(&set, Some(&nb)), Some(tv("sheetset.rule.done", &[("n", n.to_string())])));
        }
        "ss.props" => {
            let set = need_set(app)?;
            let v: Value =
                serde_json::from_str(&name).map_err(|_| "ss.props: JSON {name, PROJECTNAME, PROJECTNUMBER, custom, layoutNames}")?;
            let mut p = serde_json::Map::new();
            p.insert("PROJECTNAME".into(), v.get("PROJECTNAME").cloned().unwrap_or(json!("")));
            p.insert("PROJECTNUMBER".into(), v.get("PROJECTNUMBER").cloned().unwrap_or(json!("")));
            for line in v.get("custom").and_then(Value::as_str).unwrap_or("").split('\n') {
                if let Some((k, val)) = field_line(line) {
                    p.insert(k, json!(val));
                }
            }
            let mut next = ss::set_properties(&set, &Value::Object(p), &l);
            if let Some(o) = next.as_object_mut() {
                let nm = cc_cad::js::trim(v.get("name").and_then(Value::as_str).unwrap_or("")).to_string();
                if !nm.is_empty() {
                    o.insert("name".into(), json!(nm));
                }
                if let Some(ln) = v.get("layoutNames") {
                    o.insert("layoutNames".into(), ln.clone());
                }
            }
            edit(app, next, Some(t("sheetset.props.done")));
        }
        "ss.publish" => publish(app, name == "subset")?,
        "ss.update" => update(app)?,
        "ss.list" => {
            let set = need_set(app)?;
            if !app.sheetset.writable {
                print(app, &t("sheetset.readonly"));
                return Ok(());
            }
            let s = app.sheetset.sel.clone().filter(|(k, _)| k == "sheet").and_then(|(_, i)| ss::find_sheet(&set, &i));
            let Some(s) = s else {
                print(app, &t("sheetset.list.pick"));
                return Ok(());
            };
            let Some(mut f) = folder_of(app) else { return Ok(()) };
            match io::add_sheet_list(&set, &mut f, &s, io::Stamp::default(), &l) {
                Ok(file) => print(app, &tv("sheetset.list.done", &[("sheet", ss::sheet_label(&s)), ("file", file)])),
                Err(e) => print(app, &tv("sheetset.update.failed", &[("why", e)])),
            }
        }
        "ss.archive" => archive(app)?,
        "ss.undo" => {
            if let Some(prev) = app.sheetset.undo.pop() {
                if let Some(cur) = app.sheetset.set.replace(prev) {
                    app.sheetset.redo.push(cur);
                }
                save_set(app);
                print(app, &t("sheetset.undone"));
            }
        }
        "ss.redo" => {
            if let Some(next) = app.sheetset.redo.pop() {
                if let Some(cur) = app.sheetset.set.replace(next) {
                    app.sheetset.undo.push(cur);
                }
                save_set(app);
                print(app, &t("sheetset.redone"));
            }
        }
        "ss.opensheet" => open_sheet(app, &name)?,
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// A "KEY = value" line of the fields boxes (`/^\s*([A-Za-z][A-Za-z0-9_ ]{0,63})\s*=\s*(.*)$/`).
fn field_line(line: &str) -> Option<(String, String)> {
    let (k, v) = line.split_once('=')?;
    let k = cc_cad::js::trim(k);
    let mut cs = k.chars();
    if !cs.next().is_some_and(|c| c.is_ascii_alphabetic())
        || k.chars().count() > 64
        || !cs.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ')
    {
        return None;
    }
    let key = k.to_uppercase().split_whitespace().collect::<Vec<_>>().join("_");
    Some((key, cc_cad::js::trim(v).to_string()))
}

fn publish(app: &mut UiApp, only_subset: bool) -> Result<(), String> {
    let l = lang();
    let set = need_set(app)?;
    let subset = if only_subset { selected_subset(&app.sheetset) } else { None };
    print(app, &tv("sheetset.publish.working", &[("n", ss::all_sheets(&set).len().to_string())]));
    let Some(mut f) = folder_of(app) else { return Ok(()) };
    let (pen, mono) = (app.plot.pen, app.plot.mono);
    let p = io::publish_set(&set, &mut f, subset.as_deref(), None, pen, mono, true, now_ms(), &l);
    for n in &p.notes {
        print(app, &format!("  {n}"));
    }
    let Some(bytes) = p.bytes else {
        print(app, &t("sheetset.publish.nothing"));
        return Ok(());
    };
    let sname = set.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let sub_name =
        subset.and_then(|g| subset_ids(&set).into_iter().find(|(i, _)| *i == g)).map(|(_, n)| format!(" - {n}")).unwrap_or_default();
    let file = format!("{}{sub_name}.pdf", safe_name(&sname));
    let saved = match app.services.save.as_mut() {
        Some(save) => save(&file, bytes),
        None => Err("Saving is not available here.".into()),
    };
    match saved {
        Ok(Some(_)) => print(app, &tv("sheetset.publish.done", &[("n", p.pages.to_string()), ("file", file)])),
        Ok(None) => {}
        Err(e) => print(app, &tv("sheetset.publish.failed", &[("why", e)])),
    }
    Ok(())
}

fn update(app: &mut UiApp) -> Result<(), String> {
    let l = lang();
    let set = need_set(app)?;
    if !app.sheetset.writable {
        print(app, &t("sheetset.readonly"));
        return Ok(());
    }
    let Some(mut f) = folder_of(app) else { return Ok(()) };
    match io::update_drawings(&set, &mut f, true, true, io::Stamp::default(), &l) {
        Ok((next, files, notes)) => {
            for n in &notes {
                print(app, &format!("  {n}"));
            }
            if next != set {
                edit(app, next, None);
            }
            print(app, &tv("sheetset.update.done", &[("n", files.len().to_string())]));
            for x in files {
                print(
                    app,
                    &format!(
                        "  {}",
                        tv(
                            "sheetset.update.file",
                            &[("file", x.path), ("fields", x.fields.to_string()), ("renamed", x.renamed.to_string())]
                        )
                    ),
                );
            }
        }
        Err(e) => print(app, &tv("sheetset.update.failed", &[("why", e)])),
    }
    Ok(())
}

fn archive(app: &mut UiApp) -> Result<(), String> {
    let l = lang();
    let set = need_set(app)?;
    print(app, &t("sheetset.archive.working"));
    let Some(mut f) = folder_of(app) else { return Ok(()) };
    let read_model = |bytes: &[u8], p: &str| -> Result<cc_io_dwg::value::Obj, String> {
        if p.to_ascii_lowercase().ends_with(".dxf") {
            cc_io_dwg::dxf_model::try_read_dxf_model(&cc_io_dwg::dxf_model::decode_dxf_bytes(bytes))
        } else {
            Ok(cc_io_dwg::read_dwg(bytes, &cc_io_dwg::ReadOpts::default()))
        }
    };
    let now = now_ms();
    // the note's date as toISOString gives it (UTC); the zip's time and date on the local clock (as JS's getHours …)
    let today = iso_date(now);
    let local = cc_clock::Zone::System.local_ms(now);
    let (y, mo, d, h, mi, s) = civil(local);
    let when = io::DosTime {
        time: u16::try_from((h << 11) | (mi << 5) | (s >> 1)).unwrap_or(0),
        date: u16::try_from(((y.saturating_sub(1980).max(0)) << 9) | (mo << 5) | d).unwrap_or(0),
    };
    let text = ss::write_sheet_set(&set);
    let a = io::archive_set(&set, &mut f, &app.sheetset.set_file.clone(), Some(&text), &read_model, &today, when, &l);
    for n in &a.notes {
        print(app, &format!("  {n}"));
    }
    let sname = set.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let file = format!("{}.zip", safe_name(&sname));
    let saved = match app.services.save.as_mut() {
        Some(save) => save(&file, a.bytes),
        None => Err("Saving is not available here.".into()),
    };
    match saved {
        Ok(Some(_)) => {
            print(app, &tv("sheetset.archive.done", &[("n", a.files.len().to_string()), ("file", file)]));
            for m in a.missing {
                print(app, &format!("  {}", tv("sheetset.archive.missing.row", &[("ref", m.r#ref), ("from", m.from)])));
            }
        }
        Ok(None) => {}
        Err(e) => print(app, &tv("sheetset.archive.failed", &[("why", e)])),
    }
    Ok(())
}

/// Civil fields (y, m, d, h, min, s) of a time value in ms read as UTC.
fn civil(ms: f64) -> (i64, i64, i64, i64, i64, i64) {
    let ms = if ms.is_finite() { ms } else { 0.0 };
    // a time value fits i64 milliseconds
    #[allow(clippy::cast_possible_truncation)]
    let t = (ms / 1000.0).floor() as i64;
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    // Howard Hinnant's days → civil
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d, secs / 3600, (secs % 3600) / 60, secs % 60)
}
fn iso_date(ms: f64) -> String {
    let (y, m, d, ..) = civil(ms);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Open the sheet's drawing at that sheet (the window holds one drawing: an unsaved one is not dropped).
fn open_sheet(app: &mut UiApp, sid: &str) -> Result<(), String> {
    let set = need_set(app)?;
    let Some(s) = ss::find_sheet(&set, sid) else { return Ok(()) };
    if app.editor.as_ref().is_some_and(crate::edit::Editor::dirty) {
        print(app, "Save the open drawing first: the window holds one drawing at a time.");
        return Ok(());
    }
    let file = s.get("file").and_then(Value::as_str).unwrap_or("").to_string();
    let lay = s.get("layout").and_then(Value::as_str).unwrap_or("").to_string();
    let Some(mut f) = folder_of(app) else { return Ok(()) };
    let bytes = match f.read(&file) {
        Ok(b) => b,
        Err(e) => {
            print(app, &tv("sheetset.open.sheet.failed", &[("sheet", ss::sheet_label(&s)), ("file", file), ("why", e)]));
            return Ok(());
        }
    };
    let name = file.rsplit('/').next().unwrap_or(&file).to_string();
    let dir = std::path::Path::new(app.sheetset.folder.as_deref().unwrap_or("")).join(file.rsplit_once('/').map_or("", |x| x.0));
    let keep = std::mem::take(&mut app.sheetset);
    app.open_file(&ImportedFile { name, bytes, dir: Some(dir.display().to_string()) });
    app.sheetset = keep;
    app.run_ui_pub("file.edit", &Params::default());
    let idx = app.editor.as_ref().and_then(|e| e.eng.doc().layouts.iter().position(|x| x.name == lay));
    if let Some(i) = idx {
        app.run_ui_pub("space.select", &Params { index: Some(i.saturating_add(1)), ..Default::default() });
    }
    Ok(())
}

/// After every input: SHEETSET's request to show the panel.
pub fn sync(app: &mut UiApp) {
    let Some(ed) = app.editor.as_ref() else { return };
    if std::mem::take(&mut ed.hs.borrow_mut().ss_open) {
        app.sheetset.open = true;
    }
    let name = app.sheetset.set.as_ref().and_then(|s| s.get("name")).and_then(Value::as_str).map(str::to_string);
    if let Some(ed) = app.editor.as_ref() {
        ed.hs.borrow_mut().ss_name = name;
    }
}

// ------------------------------------------------------------------------------------------------- the panel

/// The panel (a window beside the drawing) and its dialogs.
pub fn window(app: &mut UiApp, ctx: &egui::Context) {
    if !app.sheetset.open {
        return;
    }
    let tok = app.tokens;
    let mut acts: Vec<(&'static str, String)> = Vec::new();
    let mut open = true;
    let p = &app.sheetset;
    let fname = folder_name(app);
    egui::Window::new(t("sheetset.h"))
        .id(egui::Id::unique("cad_sheetset"))
        .open(&mut open)
        .default_pos(egui::pos2(240.0, 80.0))
        .default_size([320.0, 420.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.menu_button(t("sheetset.menu"), |ui| {
                    let has = p.set.is_some();
                    let mut item = |ui: &mut egui::Ui, key: &str, id: &'static str, on: bool| {
                        if ui.add_enabled(on, egui::Button::new(t(key))).clicked() {
                            acts.push((id, String::new()));
                            ui.close();
                        }
                    };
                    item(ui, "sheetset.m.open", "ss.open", true);
                    item(ui, "sheetset.m.new", "$new", true);
                    ui.separator();
                    item(ui, "sheetset.m.add", "$add", has);
                    item(ui, "sheetset.m.newsheet", "$newsheet", has && p.writable);
                    item(ui, "sheetset.m.subset", "$subset", has);
                    item(ui, "sheetset.m.rename", "$rename", has && p.sel.is_some());
                    item(ui, "sheetset.m.remove", "$remove", has && p.sel.as_ref().is_some_and(|(k, _)| k != "set"));
                    item(ui, "sheetset.m.renumber", "$renumber", has);
                    item(ui, "sheetset.m.numbering", "$numbering", has);
                    item(ui, "sheetset.m.props", "$props", has);
                    ui.separator();
                    item(ui, "sheetset.m.publish", "$publish", has);
                    item(ui, "sheetset.m.update", "$update", has && p.writable);
                    item(ui, "sheetset.m.list", "ss.list", has && p.writable);
                    item(ui, "sheetset.m.archive", "ss.archive", has);
                    ui.separator();
                    item(ui, "sheetset.m.undo", "ss.undo", !p.undo.is_empty());
                    item(ui, "sheetset.m.redo", "ss.redo", !p.redo.is_empty());
                    item(ui, "sheetset.m.close", "ss.close", has);
                });
            });
            ui.separator();
            let Some(set) = p.set.as_ref() else {
                ui.label(RichText::new(t("sheetset.empty")).color(tok.text_dim));
                return;
            };
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let sel = |k: &str, id: &str| p.sel.as_ref().is_some_and(|(a, b)| a == k && b == id);
                let mut sub = fname.clone();
                if !p.writable {
                    sub.push_str(&format!(" · {}", t("sheetset.readonly.short")));
                }
                if p.unsaved {
                    sub.push_str(&format!(" · {}", t("sheetset.unsaved")));
                }
                let name = set.get("name").and_then(Value::as_str).unwrap_or("");
                let r = ui.selectable_label(sel("set", "set"), RichText::new(name).strong());
                if r.clicked() {
                    acts.push(("ss.select", "set".into()));
                }
                ui.label(RichText::new(sub).small().color(tok.text_dim));
                tree(ui, set.get("sheets"), set.get("subsets"), 1, &sel, &mut acts, &tok);
            });
            // Alt+↑ / ↓ moves the selected sheet; Delete removes (no text field focused)
            if p.sel.as_ref().is_some_and(|(k, _)| k == "sheet") && ui.memory(|m| m.focused().is_none()) {
                let (up, down) = ui.input(|i| {
                    (i.modifiers.alt && i.key_pressed(egui::Key::ArrowUp), i.modifiers.alt && i.key_pressed(egui::Key::ArrowDown))
                });
                if up {
                    acts.push(("ss.move", "up".into()));
                }
                if down {
                    acts.push(("ss.move", "down".into()));
                }
            }
        });
    if !open {
        app.sheetset.open = false;
    }
    for (id, name) in acts {
        if let Some(rest) = id.strip_prefix('$') {
            start_form(app, rest);
        } else {
            app.run_ui_pub(id, &Params { name: Some(name), ..Default::default() });
        }
    }
    form_window(app, ctx);
}

fn tree(
    ui: &mut egui::Ui,
    sheets: Option<&Value>,
    subsets: Option<&Value>,
    depth: usize,
    sel: &dyn Fn(&str, &str) -> bool,
    acts: &mut Vec<(&'static str, String)>,
    tok: &crate::theme::Tokens,
) {
    if depth > 16 {
        return;
    }
    let indent = depth as f32 * 14.0;
    for s in sheets.and_then(Value::as_array).into_iter().flatten() {
        let id = s.get("id").and_then(Value::as_str).unwrap_or("");
        ui.horizontal(|ui| {
            ui.add_space(indent);
            let r = ui.selectable_label(sel("sheet", id), ss::sheet_label(s));
            let r = r.on_hover_text(format!(
                "{} › {}{}",
                s.get("file").and_then(Value::as_str).unwrap_or(""),
                s.get("layout").and_then(Value::as_str).unwrap_or(""),
                s.get("description").and_then(Value::as_str).filter(|d| !d.is_empty()).map(|d| format!("\n{d}")).unwrap_or_default()
            ));
            if r.clicked() {
                acts.push(("ss.select", format!("sheet:{id}")));
            }
            if r.double_clicked() {
                acts.push(("ss.opensheet", id.to_string()));
            }
        });
    }
    for g in subsets.and_then(Value::as_array).into_iter().flatten() {
        let id = g.get("id").and_then(Value::as_str).unwrap_or("");
        let n = ss::all_sheets(
            &json!({ "sheets": g.get("sheets").cloned().unwrap_or(json!([])), "subsets": g.get("subsets").cloned().unwrap_or(json!([])) }),
        )
        .len();
        ui.horizontal(|ui| {
            ui.add_space(indent);
            let r = ui.selectable_label(
                sel("subset", id),
                RichText::new(format!("▾ {}", g.get("name").and_then(Value::as_str).unwrap_or(""))).strong(),
            );
            if r.clicked() {
                acts.push(("ss.select", format!("subset:{id}")));
            }
            if r.double_clicked() {
                acts.push(("$rename", String::new()));
            }
            ui.label(RichText::new(tv("sheetset.subset.count", &[("n", n.to_string())])).small().color(tok.text_dim));
        });
        tree(ui, g.get("sheets"), g.get("subsets"), depth + 1, sel, acts, tok);
    }
}

/// The dialog a menu item opens, filled as sheetset-panel.js fills it.
fn start_form(app: &mut UiApp, what: &str) {
    let p = &app.sheetset;
    let set = p.set.clone().unwrap_or(Value::Null);
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let form = match what {
        "new" => Some(Form::New { name: t("sheetset.h"), pname: String::new(), pnumber: String::new() }),
        "add" => {
            let mut f = folder_of(app);
            let files: Vec<String> = f
                .as_mut()
                .and_then(Folder::list)
                .unwrap_or_default()
                .into_iter()
                .filter(|x| {
                    let l = x.to_ascii_lowercase();
                    l.ends_with(".dwg") || l.ends_with(".dxf")
                })
                .collect();
            if files.is_empty() {
                print(app, &tv("sheetset.add.nofiles", &[("folder", folder_name(app))]));
                None
            } else {
                Some(Form::Add { files })
            }
        }
        "newsheet" => {
            let tpl = set.get("template").filter(|v| !v.is_null()).cloned();
            let mut f = folder_of(app);
            let files: Vec<String> = f
                .as_mut()
                .and_then(Folder::list)
                .unwrap_or_default()
                .into_iter()
                .filter(|x| x.to_ascii_lowercase().ends_with(".dwg"))
                .collect();
            let mut choices = vec![(String::new(), t("sheetset.f.template.pick"))];
            if let Some(tp) = &tpl {
                choices.push((format!("{}|{}", s(tp, "file"), s(tp, "layout")), format!("{} › {}", s(tp, "file"), s(tp, "layout"))));
            }
            for x in files {
                if tpl.as_ref().is_none_or(|tp| s(tp, "file") != x) {
                    choices.push((format!("{x}|"), x));
                }
            }
            let template = tpl.as_ref().map(|tp| format!("{}|{}", s(tp, "file"), s(tp, "layout"))).unwrap_or_default();
            Some(Form::NewSheet { number: String::new(), title: String::new(), template, choices })
        }
        "subset" => Some(Form::Subset { name: String::new() }),
        "rename" => match p.sel.clone() {
            Some((k, id)) if k == "subset" => {
                let name = subset_ids(&set).into_iter().find(|(i, _)| *i == id).map(|x| x.1).unwrap_or_default();
                Some(Form::RenameSubset { id, name })
            }
            Some((k, id)) if k == "sheet" => ss::find_sheet(&set, &id).map(|sh| {
                let fields = sh
                    .get("properties")
                    .and_then(Value::as_object)
                    .map(|m| m.iter().map(|(k, v)| format!("{k} = {}", cc_cad::js::to_js_string(v))).collect::<Vec<_>>().join("\n"))
                    .unwrap_or_default();
                Form::RenameSheet { id, number: s(&sh, "number"), title: s(&sh, "title"), description: s(&sh, "description"), fields }
            }),
            Some((k, _)) if k == "set" => Some(props_form(&set)),
            _ => None,
        },
        "remove" => match p.sel.clone() {
            Some((k, id)) if k == "subset" => {
                let name = subset_ids(&set).into_iter().find(|(i, _)| *i == id).map(|x| x.1).unwrap_or_default();
                Some(Form::Remove { kind: k, id, text: tv("sheetset.remove.subset.text", &[("name", name)]) })
            }
            Some((k, id)) if k == "sheet" => ss::find_sheet(&set, &id).map(|sh| Form::Remove {
                kind: k,
                id,
                text: tv("sheetset.remove.sheet.text", &[("sheet", ss::sheet_label(&sh)), ("file", s(&sh, "file"))]),
            }),
            _ => None,
        },
        "renumber" => Some(Form::Renumber {
            which: if selected_subset(p).is_some() { "subset".into() } else { "all".into() },
            prefix: "A-".into(),
            start: "101".into(),
            step: "1".into(),
            digits: "0".into(),
        }),
        "numbering" => {
            let nb = set
                .get("numbering")
                .filter(|v| !v.is_null())
                .cloned()
                .unwrap_or_else(|| json!({ "rule": "{DISCIPLINE}-{LEVEL}{SEQ:2}", "start": 1, "auto": true }));
            Some(Form::Numbering {
                rule: s(&nb, "rule"),
                start: nb.get("start").map_or_else(|| "1".into(), cc_cad::js::to_js_string),
                auto: nb.get("auto").and_then(Value::as_bool).unwrap_or(false),
            })
        }
        "props" => Some(props_form(&set)),
        "publish" => match selected_subset(p) {
            Some(g) => {
                let name = subset_ids(&set).into_iter().find(|(i, _)| *i == g).map(|x| x.1).unwrap_or_default();
                Some(Form::Publish { subset: g, name })
            }
            None => {
                app.run_ui_pub("ss.publish", &Params::default());
                None
            }
        },
        "update" => {
            let mut files: Vec<String> =
                ss::all_sheets(&set).iter().filter_map(|x| x.sheet.get("file").and_then(Value::as_str).map(str::to_string)).collect();
            files.sort();
            files.dedup();
            Some(Form::Update { n: files.len() })
        }
        _ => None,
    };
    app.sheetset.form = form;
}

fn props_form(set: &Value) -> Form {
    let props = set.get("properties").and_then(Value::as_object).cloned().unwrap_or_default();
    let custom = props
        .iter()
        .filter(|(k, _)| *k != "PROJECTNAME" && *k != "PROJECTNUMBER")
        .map(|(k, v)| format!("{k} = {}", cc_cad::js::to_js_string(v)))
        .collect::<Vec<_>>()
        .join("\n");
    let g = |k: &str| props.get(k).map(cc_cad::js::to_js_string).unwrap_or_default();
    Form::Props {
        name: set.get("name").and_then(Value::as_str).unwrap_or("").into(),
        pname: g("PROJECTNAME"),
        pnumber: g("PROJECTNUMBER"),
        custom,
        layout_names: set.get("layoutNames").and_then(Value::as_str).unwrap_or("number-title").into(),
    }
}

fn field(ui: &mut egui::Ui, label: &str, v: &mut String, multi: bool) {
    ui.label(label);
    if multi {
        ui.add(egui::TextEdit::multiline(v).desired_rows(4).desired_width(300.0));
    } else {
        ui.add(egui::TextEdit::singleline(v).desired_width(300.0));
    }
}

/// The open dialog: OK runs its `ss.*` command; Cancel or the close button drops it.
fn form_window(app: &mut UiApp, ctx: &egui::Context) {
    let Some(mut form) = app.sheetset.form.clone() else { return };
    let mut ok: Option<(&'static str, String)> = None;
    let mut done = false;
    let mut extra: Option<Form> = None;
    let title = match &form {
        Form::New { .. } => t("sheetset.new.title"),
        Form::Which { .. } => t("sheetset.which.title"),
        Form::Add { .. } => t("sheetset.add.title"),
        Form::Layouts { file, .. } => tv("sheetset.add.layouts", &[("file", file.clone())]),
        Form::NewSheet { .. } => t("sheetset.newsheet.title"),
        Form::Subset { .. } => t("sheetset.subset.title"),
        Form::RenameSubset { .. } => t("sheetset.rename.subset"),
        Form::RenameSheet { .. } => t("sheetset.rename.sheet"),
        Form::Renumber { .. } => t("sheetset.renumber.title"),
        Form::Numbering { .. } => t("sheetset.rule.title"),
        Form::Props { .. } => t("sheetset.props.title"),
        Form::Remove { kind, .. } => t(if kind == "subset" { "sheetset.remove.subset.title" } else { "sheetset.remove.sheet.title" }),
        Form::Publish { .. } => t("sheetset.publish.title"),
        Form::Update { .. } => t("sheetset.update.title"),
    };
    let folder = app.sheetset.folder.clone().unwrap_or_default();
    let fname = folder_name(app);
    let mut picked_folder = false;
    egui::Window::new(title)
        .id(egui::Id::unique("cad_sheetset_form"))
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_max_width(380.0);
            match &mut form {
                Form::New { name, pname, pnumber } => {
                    ui.label(t("sheetset.new.text"));
                    field(ui, &t("sheetset.f.name"), name, false);
                    field(ui, &t("sheetset.f.projectname"), pname, false);
                    field(ui, &t("sheetset.f.projectnumber"), pnumber, false);
                    if ui.button(t("sheetset.ok")).clicked() && !name.trim().is_empty() {
                        picked_folder = true;
                    }
                }
                Form::Which { sets } => {
                    ui.label(tv("sheetset.which.text", &[("folder", fname.clone())]));
                    for s in sets.iter() {
                        if ui.button(s).clicked() {
                            ok = Some(("ss.open", format!("{folder}|{s}")));
                        }
                    }
                }
                Form::Add { files } => {
                    ui.label(t("sheetset.add.text"));
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for f in files.iter() {
                            if ui.button(f).clicked() {
                                extra = Some(Form::Layouts { file: f.clone(), layouts: Vec::new() });
                            }
                        }
                    });
                }
                Form::Layouts { file, layouts } => {
                    for (l, on) in layouts.iter_mut() {
                        ui.checkbox(on, l.as_str());
                    }
                    if ui.button(t("sheetset.ok")).clicked() {
                        let chosen: Vec<String> = layouts.iter().filter(|x| x.1).map(|x| x.0.clone()).collect();
                        ok = Some(("ss.add", format!("{file}|{}", chosen.join(","))));
                    }
                }
                Form::NewSheet { number, title, template, choices } => {
                    ui.label(t("sheetset.newsheet.text"));
                    field(ui, &t("sheetset.f.number"), number, false);
                    field(ui, &t("sheetset.f.title"), title, false);
                    ui.label(t("sheetset.f.template"));
                    let cur = choices.iter().find(|(v, _)| v == template).map(|x| x.1.clone()).unwrap_or_default();
                    egui::ComboBox::new("ss_tpl", "").selected_text(cur).width(300.0).show_ui(ui, |ui| {
                        for (v, l) in choices.iter() {
                            if ui.selectable_label(v == template, l).clicked() {
                                *template = v.clone();
                            }
                        }
                    });
                    if ui.button(t("sheetset.ok")).clicked() {
                        let (tf, tl) = template.split_once('|').unwrap_or((template.as_str(), ""));
                        ok = Some(("ss.newsheet", format!("{}|{}|{tf}|{tl}", number.trim(), title.trim())));
                    }
                }
                Form::Subset { name } => {
                    field(ui, &t("sheetset.f.name"), name, false);
                    if ui.button(t("sheetset.ok")).clicked() {
                        ok = Some(("ss.subset", name.clone()));
                    }
                }
                Form::RenameSubset { name, .. } => {
                    field(ui, &t("sheetset.f.name"), name, false);
                    if ui.button(t("sheetset.ok")).clicked() {
                        ok = Some(("ss.rename", name.clone()));
                    }
                }
                Form::RenameSheet { number, title, description, fields, .. } => {
                    ui.label(t("sheetset.rename.text"));
                    field(ui, &t("sheetset.f.number"), number, false);
                    field(ui, &t("sheetset.f.title"), title, false);
                    field(ui, &t("sheetset.f.description"), description, true);
                    field(ui, &t("sheetset.f.sheetfields"), fields, true);
                    ui.label(RichText::new(t("sheetset.f.custom.hint")).small());
                    if ui.button(t("sheetset.ok")).clicked() {
                        ok = Some(("ss.rename", json!({ "number": number, "title": title, "description": description, "fields": fields }).to_string()));
                    }
                }
                Form::Renumber { which, prefix, start, step, digits } => {
                    ui.label(t("sheetset.renumber.text"));
                    ui.horizontal(|ui| {
                        ui.radio_value(which, "all".to_string(), t("sheetset.renumber.all"));
                        ui.radio_value(which, "subset".to_string(), t("sheetset.renumber.which"));
                    });
                    field(ui, &t("sheetset.renumber.prefix"), prefix, false);
                    field(ui, &t("sheetset.renumber.start"), start, false);
                    field(ui, &t("sheetset.renumber.step"), step, false);
                    field(ui, &t("sheetset.renumber.digits"), digits, false);
                    if ui.button(t("sheetset.ok")).clicked() {
                        ok = Some(("ss.renumber", format!("{which}|{prefix}|{start}|{step}|{digits}")));
                    }
                }
                Form::Numbering { rule, start, auto } => {
                    ui.label(t("sheetset.rule.text"));
                    field(ui, &t("sheetset.rule.rule"), rule, false);
                    ui.label(RichText::new(t("sheetset.rule.hint")).small());
                    field(ui, &t("sheetset.rule.start"), start, false);
                    ui.checkbox(auto, t("sheetset.rule.auto.yes"));
                    if ui.button(t("sheetset.ok")).clicked() {
                        ok = Some(("ss.numbering", format!("{rule}|{start}|{}", if *auto { "yes" } else { "no" })));
                    }
                }
                Form::Props { name, pname, pnumber, custom, layout_names } => {
                    ui.label(t("sheetset.props.text"));
                    field(ui, &t("sheetset.f.name"), name, false);
                    field(ui, &t("sheetset.f.projectname"), pname, false);
                    field(ui, &t("sheetset.f.projectnumber"), pnumber, false);
                    field(ui, &t("sheetset.f.custom"), custom, true);
                    ui.label(RichText::new(t("sheetset.f.custom.hint")).small());
                    ui.label(t("sheetset.f.layoutnames"));
                    let opts = [("number-title", "sheetset.ln.numbertitle"), ("number", "sheetset.ln.number"), ("title", "sheetset.ln.title"), ("keep", "sheetset.ln.keep")];
                    let cur = opts.iter().find(|(v, _)| *v == layout_names.as_str()).map(|x| t(x.1)).unwrap_or_default();
                    egui::ComboBox::new("ss_ln", "").selected_text(cur).width(300.0).show_ui(ui, |ui| {
                        for (v, k) in opts {
                            if ui.selectable_label(layout_names.as_str() == v, t(k)).clicked() {
                                *layout_names = v.into();
                            }
                        }
                    });
                    if ui.button(t("sheetset.ok")).clicked() {
                        ok = Some(("ss.props", json!({ "name": name, "PROJECTNAME": pname, "PROJECTNUMBER": pnumber, "custom": custom, "layoutNames": layout_names }).to_string()));
                    }
                }
                Form::Remove { text, .. } => {
                    ui.label(text.as_str());
                    if ui.button(t("sheetset.remove")).clicked() {
                        ok = Some(("ss.remove", String::new()));
                    }
                }
                Form::Publish { name, .. } => {
                    ui.label(tv("sheetset.publish.which", &[("name", name.clone())]));
                    ui.horizontal(|ui| {
                        if ui.button(t("sheetset.publish.subset")).clicked() {
                            ok = Some(("ss.publish", "subset".into()));
                        }
                        if ui.button(t("sheetset.publish.all")).clicked() {
                            ok = Some(("ss.publish", "all".into()));
                        }
                    });
                }
                Form::Update { n } => {
                    ui.label(tv("sheetset.update.text", &[("n", n.to_string())]));
                    if ui.button(t("sheetset.update.ok")).clicked() {
                        ok = Some(("ss.update", String::new()));
                    }
                }
            }
            if ui.button(t("sheetset.cancel")).clicked() {
                done = true;
            }
        });
    if let Some(Form::Layouts { file, .. }) = extra {
        // the file's sheets (read without opening it), those not in the set yet ticked
        let l = lang();
        print(app, &tv("sheetset.add.reading", &[("file", file.clone())]));
        let got = folder_of(app).map(|mut f| io::layouts_in(&mut f, &file, &l));
        match got {
            Some(Ok(ls)) => {
                let set = app.sheetset.set.clone().unwrap_or(Value::Null);
                let taken: Vec<String> = ss::all_sheets(&set)
                    .iter()
                    .filter(|x| x.sheet.get("file").and_then(Value::as_str) == Some(file.as_str()))
                    .filter_map(|x| x.sheet.get("layout").and_then(Value::as_str).map(str::to_string))
                    .collect();
                let free: Vec<(String, bool)> = ls.into_iter().filter(|x| !taken.contains(x)).map(|x| (x, true)).collect();
                if free.is_empty() {
                    print(app, &tv("sheetset.add.nolayouts", &[("file", file)]));
                    app.sheetset.form = None;
                } else {
                    app.sheetset.form = Some(Form::Layouts { file, layouts: free });
                }
            }
            Some(Err(e)) => {
                print(app, &tv("sheetset.add.failed", &[("file", file), ("why", e)]));
                app.sheetset.form = None;
            }
            None => app.sheetset.form = None,
        }
        return;
    }
    if picked_folder && let Form::New { name, pname, pnumber } = &form {
        let folder = app.services.pick_folder.as_mut().and_then(|p| p());
        app.sheetset.form = None;
        match folder {
            Some(f) => {
                app.run_ui_pub("ss.new", &Params { name: Some(format!("{}|{f}|{pname}|{pnumber}", name.trim())), ..Default::default() })
            }
            None if app.services.pick_folder.is_none() => {
                print(app, "Sheet sets need a folder: the desktop app opens one (this build cannot yet).")
            }
            None => {}
        }
        return;
    }
    if done {
        app.sheetset.form = None;
        return;
    }
    if let Some((id, name)) = ok {
        app.sheetset.form = None;
        app.run_ui_pub(id, &Params { name: Some(name), ..Default::default() });
        return;
    }
    app.sheetset.form = Some(form);
}
