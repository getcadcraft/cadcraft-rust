//! My tools on the window (0.5 mytools-ui.js, DR.9): the "My tools" groups on CadCraft's toolbar (the toolbar style;
//! the menu bar style lists them under More, the ribbon on an Add-ons tab, as 0.5's input styles show a toolbar's other
//! buttons), Settings › CadCraft › My tools (add / edit / delete a button, export and import JSON, import a CUIx
//! file), the edit dialog, a button's shortcut key (before the window's own keys, never while a dialog is open) and
//! running a button's macro through the command line (cc-engine `mytools`: ^C cancels, Enter, a pause waits for the
//! person's own answer, a leading * repeats until Esc). Data: cc-engine `mytools` (kept with Settings as 0.5 keeps
//! `cadcraft.mytools`); CUIx: cc-engine `cuix`. Every action is a `mytools.*` command.
//!
//! Icons: a PNG shows on its button; an SVG icon is kept in the data (and exported) but shown as the star, since the
//! window draws no SVG that comes from a file (CLAUDE.md § 7, Icons).

use std::collections::HashMap;

use cc_engine::mytools::{self as mt, Ids, Note, Step, Tools};
use egui::RichText;
use serde_json::{Value, json};

use crate::{Params, UiApp};

/// The edit dialog's fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edit {
    /// the tool being edited (None: a new one)
    pub id: Option<String>,
    pub group: String,
    pub label: String,
    pub macro_: String,
    pub key: String,
    pub icon: Option<String>,
    pub err: String,
    pub capturing: bool,
}

/// A macro being run, step by step (mytools-ui.js `run`).
#[derive(Clone, Debug)]
struct Runner {
    steps: Vec<Step>,
    pos: usize,
    repeat: bool,
    round: u32,
    /// a pause waits for an answer: the prompts seen when it started
    wait: Option<u64>,
}

/// The window's My tools.
#[derive(Default)]
pub struct MyToolsUi {
    pub data: Tools,
    pub ids: Ids,
    /// what Settings › My tools last said
    pub said: String,
    pub edit: Option<Edit>,
    run: Option<Runner>,
    /// a picker is open for ("json" | "cuix" | "icon")
    waiting: Option<&'static str>,
    /// PNG icons as textures, by their data: URL
    tex: HashMap<String, Option<egui::TextureHandle>>,
}

/// UI commands.
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("mytools.run", "My tools: a button", "", "{name: tool id} → runs its macro through the command line"),
    ("mytools.macro", "My tools: run a macro", "", "{name: macro text} → as a button with that macro would"),
    ("mytools.add", "My tools: add a button", "", "{} → the edit dialog, empty"),
    ("mytools.edit", "My tools: edit a button", "", "{name: tool id} → the edit dialog on it"),
    (
        "mytools.field",
        "My tools: a field of the edit dialog",
        "",
        "{name: \"group=…\" | \"label=…\" | \"macro=…\" | \"key=Ctrl+Shift+L\" (empty: none)}",
    ),
    ("mytools.icon", "My tools: the button's icon", "", "{name?: a PNG / SVG file's path} → no name: the picker"),
    ("mytools.icon.remove", "My tools: remove the icon", "", "{}"),
    ("mytools.save", "My tools: save the button", "", "{} → the edit dialog's Save (its checks)"),
    ("mytools.cancel", "My tools: close the dialog", "", "{}"),
    ("mytools.delete", "My tools: delete a button", "", "{name: tool id}"),
    ("mytools.import", "My tools: import JSON", "", "{name?: the file's text} → no name: the picker"),
    ("mytools.cuix", "My tools: import a CUIx file", "", "{name?: a .cuix file's path} → no name: the picker"),
    ("mytools.export", "My tools: export", "", "{} → my-tools.json through the window's save"),
];

fn t(k: &str) -> String {
    crate::edit::lang_text(k, &[])
}
fn tv(k: &str, v: &[(&str, String)]) -> String {
    crate::edit::lang_text(k, v)
}
fn note(n: &Note) -> String {
    crate::edit::lang_text(&n.key, &n.vars())
}

/// The command line hears it (mytools-ui.js `print`).
fn print(app: &mut UiApp, s: &str) {
    match app.editor.as_ref() {
        Some(ed) => ed.hs.borrow_mut().push_log(s),
        None => app.message = s.to_string(),
    }
}

/// cadcraft.js `getvarForMacro`: the system variables a DIESEL macro may read.
fn getvar(app: &UiApp) -> impl Fn(&str) -> String + use<> {
    let (ortho, polar, layer, sheet, wucs) = match app.editor.as_ref() {
        Some(ed) => {
            let d = ed.eng.doc();
            let sheet = d.layout().map(|l| l.name.clone());
            (ed.ortho, ed.polar, d.current.layer.clone(), sheet, cc_engine::ucs::active_ucs(&d).is_none())
        }
        None => (false, false, "0".into(), None, true),
    };
    move |n: &str| match n {
        "ORTHOMODE" => u8::from(ortho).to_string(),
        // 0.5's grid is on unless turned off; the window draws none
        "GRIDMODE" => "1".into(),
        "POLARMODE" => u8::from(polar).to_string(),
        "CLAYER" => layer.clone(),
        "TILEMODE" => if sheet.is_some() { "0" } else { "1" }.into(),
        "CTAB" => sheet.clone().unwrap_or_else(|| "Model".into()),
        "WORLDUCS" => u8::from(wucs).to_string(),
        _ => String::new(),
    }
}

/// `lookup(c)`: CadCraft has that command (its table, aliases, routines' commands).
fn lookup(app: &UiApp) -> impl Fn(&str) -> bool + '_ {
    move |c: &str| app.editor.as_ref().is_some_and(|ed| ed.eng.lookup(c).is_some())
}

/// The keys of SHORTCUTS (taken elsewhere): key → what it runs.
fn taken_elsewhere(app: &UiApp) -> Vec<(String, String)> {
    app.winset.keys.clone()
}

fn save(app: &mut UiApp, d: Tools) {
    app.mytools.data = d;
}

/// What Settings keeps (0.5: localStorage `cadcraft.mytools`, removed when there are no buttons).
pub fn prefs(app: &UiApp) -> Value {
    if app.mytools.data.groups.is_empty() { Value::Null } else { app.mytools.data.json() }
}
/// Read back: untrusted (what is dropped is dropped quietly, as 0.5's load).
pub fn load_prefs(app: &mut UiApp, v: &Value) {
    app.mytools.ids.now_ms = cc_clock::now_ms();
    let (d, _) = mt::clean_tools(v, &mut app.mytools.ids);
    app.mytools.data = d;
}

/// `run(macro, label)`: notes printed; a command CadCraft lacks stops it; else the steps start (a new run replaces
/// one still going).
fn start(app: &mut UiApp, macro_: &str, label: &str) -> bool {
    if app.editor.is_none() {
        return false;
    }
    let p = mt::parse_macro(macro_, &getvar(app));
    for n in &p.notes {
        let s = note(n);
        print(app, &s);
    }
    if p.steps.is_empty() {
        app.mytools.run = None;
        return false;
    }
    let missing = p.commands.iter().find(|c| !lookup(app)(c)).cloned();
    if let Some(c) = missing {
        let l = if label.is_empty() { cc_cad::js::slice16(macro_, 40) } else { label.to_string() };
        print(app, &tv("mytools.run.missing", &[("cmd", c), ("label", l)]));
        app.mytools.run = None;
        return false;
    }
    app.mytools.run = Some(Runner { steps: p.steps, pos: 0, repeat: p.repeat, round: 0, wait: None });
    pump(app);
    true
}

/// Go on with the running macro as far as it can before a pause (every frame).
pub fn pump(app: &mut UiApp) {
    for _ in 0..10_000 {
        let Some(mut r) = app.mytools.run.take() else { return };
        let Some(ed) = app.editor.as_mut() else { return };
        let busy = ed.eng.busy();
        let Some(step) = r.steps.get(r.pos).cloned() else {
            // the end: a repeating macro runs again once its command has ended (0.5: at most 1000 rounds)
            if r.repeat && r.round < 999 {
                if busy {
                    app.mytools.run = Some(r);
                    return;
                }
                r.round += 1;
                r.pos = 0;
                app.mytools.run = Some(r);
                continue;
            }
            return;
        };
        match step {
            Step::Cancel => {
                if busy {
                    ed.escape();
                }
                r.pos += 1;
            }
            Step::Toggle(what) => {
                r.pos += 1;
                app.mytools.run = Some(r);
                if what == "ortho" {
                    app.run_ui("edit.ortho", &Params::default());
                }
                // (^G: the window draws no grid)
                continue;
            }
            Step::Pause => {
                let waiting = busy && ed.eng.expects().is_some();
                match r.wait {
                    None if !waiting => r.pos += 1,
                    None => {
                        r.wait = Some(ed.hs.borrow().prompts);
                        app.mytools.run = Some(r);
                        return;
                    }
                    Some(at) => {
                        if !busy || ed.hs.borrow().prompts != at {
                            r.wait = None;
                            r.pos += 1;
                        } else {
                            app.mytools.run = Some(r);
                            return;
                        }
                    }
                }
            }
            Step::Send(text) => {
                ed.eng.text(&text);
                ed.refresh_prompt();
                r.pos += 1;
            }
            Step::Enter => {
                ed.eng.text("");
                ed.refresh_prompt();
                r.pos += 1;
            }
        }
        app.mytools.run = Some(r);
        app.sync_editor();
    }
}

/// UI commands.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    let name = params.name.clone().unwrap_or_default();
    app.mytools.ids.now_ms = cc_clock::now_ms();
    match id {
        "mytools.run" => {
            let (gi, ti) = app.mytools.data.find(&name).ok_or_else(|| format!("mytools.run: no button {name}"))?;
            let tl = app.mytools.data.groups.get(gi).and_then(|g| g.tools.get(ti)).cloned().ok_or("mytools.run: gone")?;
            start(app, &tl.macro_, &tl.label);
        }
        "mytools.macro" => {
            start(app, &name, "");
        }
        "mytools.add" | "mytools.edit" => {
            let cur = if id == "mytools.edit" {
                let (gi, ti) = app.mytools.data.find(&name).ok_or_else(|| format!("mytools.edit: no button {name}"))?;
                app.mytools.data.groups.get(gi).and_then(|g| g.tools.get(ti).map(|tl| (g.name.clone(), tl.clone())))
            } else {
                None
            };
            let first = app.mytools.data.groups.first().map(|g| g.name.clone()).unwrap_or_else(|| t("mytools.default_group"));
            app.mytools.edit = Some(match cur {
                Some((g, tl)) => Edit {
                    id: Some(tl.id),
                    group: g,
                    label: tl.label,
                    macro_: tl.macro_,
                    key: tl.key.unwrap_or_default(),
                    icon: tl.icon,
                    ..Edit::default()
                },
                None => Edit { group: first, macro_: "^C^C".into(), ..Edit::default() },
            });
        }
        "mytools.field" => {
            let e = app.mytools.edit.as_mut().ok_or("mytools.field: the dialog is not open")?;
            let (k, v) = name.split_once('=').ok_or("mytools.field: name = \"<field>=<value>\"")?;
            match k {
                "group" => e.group = cc_cad::js::slice16(v, 60),
                "label" => e.label = cc_cad::js::slice16(v, 80),
                "macro" => e.macro_ = v.to_string(),
                "key" => {
                    e.key = if v.trim().is_empty() {
                        String::new()
                    } else {
                        cc_engine::shortcuts::norm_key(v).ok_or_else(|| format!("mytools.field: {v} is not a key"))?
                    }
                }
                _ => return Err(format!("mytools.field: no field {k} (group, label, macro, key)")),
            }
        }
        "mytools.icon" => {
            if app.mytools.edit.is_none() {
                return Err("mytools.icon: the dialog is not open".into());
            }
            if !name.is_empty() {
                let read = app.services.read_path.as_mut().ok_or("mytools.icon: this window cannot read files")?;
                let f = read(&name)?;
                icon_picked(app, &f.name, &f.bytes);
            } else {
                pick(app, "icon")?;
            }
        }
        "mytools.icon.remove" => {
            if let Some(e) = app.mytools.edit.as_mut() {
                e.icon = None;
            }
        }
        "mytools.save" => save_edit(app)?,
        "mytools.cancel" => app.mytools.edit = None,
        "mytools.delete" => {
            let (gi, ti) = app.mytools.data.find(&name).ok_or_else(|| format!("mytools.delete: no button {name}"))?;
            let label = app.mytools.data.groups.get(gi).and_then(|g| g.tools.get(ti)).map(|x| x.label.clone()).unwrap_or_default();
            let mut d = app.mytools.data.clone();
            for g in &mut d.groups {
                g.tools.retain(|x| x.id != name);
            }
            d.groups.retain(|g| !g.tools.is_empty());
            save(app, d);
            app.mytools.said = tv("mytools.deleted", &[("label", label)]);
        }
        "mytools.import" => {
            if name.is_empty() {
                pick(app, "json")?;
            } else {
                import_json(app, &name);
            }
        }
        "mytools.cuix" => {
            if name.is_empty() {
                pick(app, "cuix")?;
            } else {
                let read = app.services.read_path.as_mut().ok_or("mytools.cuix: this window cannot read files")?;
                let f = read(&name)?;
                import_cuix(app, &f.bytes);
            }
        }
        "mytools.export" => {
            let text = mt::export_tools(&app.mytools.data, &mut app.mytools.ids);
            let save = app.services.save.as_mut().ok_or("mytools.export: this window cannot save files")?;
            save("my-tools.json", text.into_bytes())?;
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// Open the window's picker for a file of this kind (desktop: at once; web: it arrives in `picked`).
fn pick(app: &mut UiApp, what: &'static str) -> Result<(), String> {
    let kind = match what {
        "json" => "mytools",
        "cuix" => "cuix",
        _ => "icon",
    };
    let f = app.services.pick_files.as_mut().ok_or("my tools: this window cannot open files")?;
    match f(kind, false) {
        Some(files) => {
            if let Some(f) = files.into_iter().next() {
                picked(app, what, f);
            }
        }
        None => app.mytools.waiting = Some(what),
    }
    Ok(())
}

fn picked(app: &mut UiApp, what: &str, f: crate::ImportedFile) {
    match what {
        "json" => import_json(app, &String::from_utf8_lossy(&f.bytes)),
        "cuix" => import_cuix(app, &f.bytes),
        _ => icon_picked(app, &f.name, &f.bytes),
    }
}

fn icon_picked(app: &mut UiApp, name: &str, bytes: &[u8]) {
    let u = mt::icon_from_bytes(bytes);
    if let Some(e) = app.mytools.edit.as_mut() {
        match u {
            Some(u) => e.icon = Some(u),
            None => e.err = tv("mytools.err.icon", &[("label", name.to_string())]),
        }
    }
}

/// `importJson(text)`
fn import_json(app: &mut UiApp, text: &str) {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        app.mytools.said = t("mytools.err.format");
        return;
    };
    let ids = &mut app.mytools.ids;
    let (r, e1) = mt::clean_tools(&v, ids);
    let groups = r.json().get("groups").and_then(Value::as_array).cloned().unwrap_or_default();
    let (m, e2) = mt::merge_tools(&app.mytools.data, &groups, ids);
    save(app, m);
    let n: usize = r.groups.iter().map(|g| g.tools.len()).sum();
    let errs: Vec<String> = e1.iter().chain(&e2).map(note).collect();
    app.mytools.said =
        tv("mytools.imported", &[("n", n.to_string())]) + &if errs.is_empty() { String::new() } else { format!(" {}", errs.join(" ")) };
}

/// `importCuix(bytes)`
fn import_cuix(app: &mut UiApp, bytes: &[u8]) {
    let r = {
        let gv = getvar(app);
        let lk = lookup(app);
        cc_engine::cuix::read_cuix(bytes, &lk, &gv)
    };
    let r = match r {
        Ok(r) => r,
        Err(e) => {
            app.mytools.said = tv("mytools.cuix.bad", &[("why", t(&format!("mytools.cuix.err.{}", e.0)))]);
            return;
        }
    };
    let mut groups: Vec<Value> = r
        .groups
        .iter()
        .map(|g| {
            let tools: Vec<Value> = g
                .tools
                .iter()
                .map(|x| {
                    let mut o = serde_json::Map::new();
                    o.insert("label".into(), json!(x.label));
                    o.insert("macro".into(), json!(x.macro_));
                    if let Some(i) = &x.icon {
                        o.insert("icon".into(), json!(i));
                    }
                    if let Some(k) = &x.key {
                        o.insert("key".into(), json!(k));
                    }
                    Value::Object(o)
                })
                .collect();
            json!({ "name": if g.shortcuts { t("mytools.cuix.keys_group") } else { g.name.clone() }, "tools": tools })
        })
        .collect();
    if !r.aliases.is_empty() {
        let tools: Vec<Value> =
            r.aliases.iter().map(|(a, c)| json!({ "label": format!("{a} ({c})"), "macro": format!("^C^C{c}") })).collect();
        groups.push(json!({ "name": t("mytools.cuix.aliases_group"), "tools": tools }));
    }
    let (m, errs) = mt::merge_tools(&app.mytools.data, &groups, &mut app.mytools.ids);
    save(app, m);
    let n: usize = groups.iter().map(|g| g.get("tools").and_then(Value::as_array).map_or(0, Vec::len)).sum();
    let mut lines = vec![tv("mytools.cuix.done", &[("groups", groups.len().to_string()), ("n", n.to_string())])];
    if !r.missing.is_empty() {
        lines.push(tv("mytools.cuix.missing", &[("list", r.missing.join(", "))]));
    }
    for e in r.notes.iter().chain(&errs) {
        lines.push(note(e));
    }
    app.mytools.said = lines.join(" ");
    for l in lines {
        print(app, &l);
    }
}

/// The edit dialog's Save (mytools-ui.js's checks, then the tool in its group, cleaned again).
fn save_edit(app: &mut UiApp) -> Result<(), String> {
    let e = app.mytools.edit.clone().ok_or("mytools.save: the dialog is not open")?;
    let label = cc_cad::js::trim(&e.label).to_string();
    let macro_ = cc_cad::js::trim(&e.macro_).to_string();
    let g = cc_cad::js::trim(&e.group).to_string();
    let gname = if g.is_empty() { t("mytools.default_group") } else { g };
    let fail = |app: &mut UiApp, s: String| {
        if let Some(x) = app.mytools.edit.as_mut() {
            x.err = s;
        }
    };
    if label.is_empty() || macro_.is_empty() || macro_ == "^C^C" {
        fail(app, t("mytools.err.fill"));
        return Ok(());
    }
    // command shortcuts too (WL.18), and the other buttons' keys
    let mut taken = taken_elsewhere(app);
    for (k, gi, ti) in app.mytools.data.key_map() {
        if let Some(x) = app.mytools.data.groups.get(gi).and_then(|g| g.tools.get(ti))
            && Some(&x.id) != e.id.as_ref()
        {
            match taken.iter_mut().find(|p| p.0 == k) {
                Some(p) => p.1 = x.label.clone(),
                None => taken.push((k, x.label.clone())),
            }
        }
    }
    if !e.key.is_empty()
        && let Some(why) = cc_engine::shortcuts::key_problem(&e.key, &taken)
    {
        let other =
            cc_engine::shortcuts::norm_key(&e.key).and_then(|n| taken.iter().find(|p| p.0 == n).map(|p| p.1.clone())).unwrap_or_default();
        fail(app, tv(&format!("mytools.err.key.{why}"), &[("label", label.clone()), ("key", e.key.clone()), ("other", other)]));
        return Ok(());
    }
    let p = mt::parse_macro(&macro_, &getvar(app));
    let miss: Vec<String> = p.commands.iter().filter(|c| !lookup(app)(c)).cloned().collect();
    if !miss.is_empty() {
        fail(app, tv("mytools.err.unknown", &[("list", miss.join(", "))]));
        return Ok(());
    }
    let mut tool = serde_json::Map::new();
    if let Some(id) = &e.id {
        tool.insert("id".into(), json!(id));
    }
    tool.insert("label".into(), json!(label));
    tool.insert("macro".into(), json!(macro_));
    if let Some(i) = &e.icon {
        tool.insert("icon".into(), json!(i));
    }
    if !e.key.is_empty() {
        tool.insert("key".into(), json!(e.key));
    }
    let data = app.mytools.data.json();
    let mut groups: Vec<Value> = data.get("groups").and_then(Value::as_array).cloned().unwrap_or_default();
    let cur_group =
        e.id.as_ref().and_then(|id| app.mytools.data.find(id)).and_then(|(gi, _)| app.mytools.data.groups.get(gi)).map(|g| g.name.clone());
    let cur_index = e.id.as_ref().and_then(|id| app.mytools.data.find(id)).map(|(_, ti)| ti);
    for g in &mut groups {
        if let Some(ts) = g.get_mut("tools").and_then(Value::as_array_mut) {
            ts.retain(|x| e.id.is_none() || x.get("id").and_then(Value::as_str) != e.id.as_deref());
        }
    }
    let at = groups.iter().position(|g| g.get("name").and_then(Value::as_str).is_some_and(|n| n.to_uppercase() == gname.to_uppercase()));
    match at.and_then(|a| groups.get_mut(a)).and_then(|g| g.get_mut("tools")).and_then(Value::as_array_mut) {
        Some(ts) => {
            let same = cur_group.is_some_and(|c| c.to_uppercase() == gname.to_uppercase());
            let i = if same { cur_index.unwrap_or(ts.len()) } else { ts.len() };
            ts.insert(i.min(ts.len()), Value::Object(tool));
        }
        None => groups.push(json!({ "name": gname, "tools": [Value::Object(tool)] })),
    }
    groups.retain(|g| g.get("tools").and_then(Value::as_array).is_some_and(|a| !a.is_empty()));
    let (r, _) = mt::clean_tools(&json!({ "format": "cadcraft-mytools", "version": 1, "groups": groups }), &mut app.mytools.ids);
    save(app, r);
    app.mytools.said = tv("mytools.saved", &[("label", label)]);
    app.mytools.edit = None;
    Ok(())
}

/// Is a dialog open (a button's key waits until it closes)?
fn dialog_open(app: &UiApp) -> bool {
    app.mytools.edit.is_some()
        || app.settings.open
        || app.plot.open
        || app.winset.keys_open
        || app.winset.cmp_open
        || app.ribbon.draft.is_some()
        || app.caddlg.qs.is_some()
        || app.caddlg.find.is_some()
        || app.choose.is_some()
}

/// The keys of My tools, before the window's own (raw_input_hook); Esc ends a running macro.
pub fn key_hook(app: &mut UiApp, raw: &mut egui::RawInput) {
    let mut i = 0;
    while i < raw.events.len() {
        let Some(egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. }) = raw.events.get(i).cloned() else {
            i += 1;
            continue;
        };
        if key == egui::Key::Escape {
            app.mytools.run = None;
        }
        // the edit dialog's key box takes the key itself
        if let Some(e) = app.mytools.edit.as_mut()
            && e.capturing
        {
            if key == egui::Key::Tab {
                i += 1;
                continue;
            }
            raw.events.remove(i);
            if matches!(raw.events.get(i), Some(egui::Event::Text(_))) {
                raw.events.remove(i);
            }
            if matches!(key, egui::Key::Backspace | egui::Key::Delete) {
                e.key.clear();
            } else if let Some(k) = crate::winsetdlg::key_of(key, modifiers) {
                e.key = k;
            }
            continue;
        }
        if app.mytools.data.groups.is_empty() || dialog_open(app) {
            i += 1;
            continue;
        }
        let Some(k) = crate::winsetdlg::key_of(key, modifiers) else {
            i += 1;
            continue;
        };
        let hit = app.mytools.data.key_map().into_iter().find(|x| x.0 == k);
        let Some((_, gi, ti)) = hit else {
            i += 1;
            continue;
        };
        raw.events.remove(i);
        if matches!(raw.events.get(i), Some(egui::Event::Text(_))) {
            raw.events.remove(i);
        }
        let Some(tl) = app.mytools.data.groups.get(gi).and_then(|g| g.tools.get(ti)).cloned() else { continue };
        app.message = tv("mytools.run.key", &[("key", k), ("label", tl.label.clone())]);
        start(app, &tl.macro_, &tl.label);
    }
}

/// Every frame: a file the browser's picker read, the running macro.
pub fn frame(app: &mut UiApp) {
    if let Some(what) = app.mytools.waiting
        && let Some(files) = app.services.picked.as_ref().and_then(|p| p.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take())
    {
        app.mytools.waiting = None;
        if let Some(f) = files.into_iter().next() {
            picked(app, what, f);
        }
    }
    if app.mytools.run.is_some() {
        pump(app);
    }
}

/// A button's picture: a PNG icon as a texture (made once), else None (the star).
pub fn icon_texture(app: &mut UiApp, ctx: &egui::Context, url: Option<&str>) -> Option<egui::TextureId> {
    let u = url?;
    if !u.starts_with("data:image/png;base64,") {
        return None;
    }
    if !app.mytools.tex.contains_key(u) {
        let bytes = mt::atob(u.trim_start_matches("data:image/png;base64,")).unwrap_or_default();
        let tex = cc_io_cad::raster::decode(&bytes, 64).ok().map(|px| {
            let size = [usize::try_from(px.w).unwrap_or(0), usize::try_from(px.h).unwrap_or(0)];
            ctx.load_texture(
                format!("mytool:{}", app.mytools.tex.len()),
                egui::ColorImage::from_rgba_unmultiplied(size, &px.px),
                egui::TextureOptions::LINEAR,
            )
        });
        if app.mytools.tex.len() > 600 {
            app.mytools.tex.clear();
        }
        app.mytools.tex.insert(u.to_string(), tex);
    }
    app.mytools.tex.get(u).and_then(|t| t.as_ref().map(egui::TextureHandle::id))
}

/// Settings › CadCraft › My tools (mytools-ui.js `renderSettings`): each button with Edit / Delete, then Add, Import,
/// Import CUIx and Export, and what the last action said.
pub fn section(app: &UiApp, ui: &mut egui::Ui, act: &mut Vec<(&'static str, Option<String>)>) {
    let tk = app.tokens;
    ui.add_space(8.0);
    ui.separator();
    ui.label(RichText::new(t("mytools.legend")).strong().color(tk.text_strong));
    ui.label(RichText::new(t("mytools.hint")).small().color(tk.text_dim));
    let d = &app.mytools.data;
    if d.groups.is_empty() {
        ui.label(RichText::new(t("mytools.none")).italics().color(tk.text_dim));
    }
    for g in &d.groups {
        for tl in &g.tools {
            ui.horizontal(|ui| {
                ui.label(format!("{} › {}", g.name, tl.label));
                if let Some(k) = &tl.key {
                    ui.label(RichText::new(k).monospace().color(tk.text_dim));
                }
                if ui.link(t("mytools.edit")).clicked() {
                    act.push(("mytools.edit", Some(tl.id.clone())));
                }
                if ui.link(t("mytools.delete")).clicked() {
                    act.push(("mytools.delete", Some(tl.id.clone())));
                }
            });
        }
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button(t("mytools.add")).clicked() {
            act.push(("mytools.add", None));
        }
        if ui.button(t("mytools.import")).clicked() {
            act.push(("mytools.import", None));
        }
        if ui.button(t("mytools.import_cuix")).clicked() {
            act.push(("mytools.cuix", None));
        }
        if ui.add_enabled(!d.groups.is_empty(), egui::Button::new(t("mytools.export"))).clicked() {
            act.push(("mytools.export", None));
        }
    });
    if !app.mytools.said.is_empty() {
        ui.label(RichText::new(&app.mytools.said).small().color(tk.text));
    }
}

/// The edit dialog (drawn while open): group, label, macro, key (press it), icon.
pub fn window(app: &mut UiApp, ctx: &egui::Context) {
    let Some(mut e) = app.mytools.edit.clone() else { return };
    let tk = app.tokens;
    let icon = icon_texture(app, ctx, e.icon.as_deref());
    let mut acts: Vec<(&'static str, Option<String>)> = Vec::new();
    let mut open = true;
    egui::Window::new(t(if e.id.is_some() { "mytools.dlg.edit" } else { "mytools.dlg.add" }))
        .id(egui::Id::unique("cad_mytools_edit"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(420.0)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            egui::Grid::new("mytools_edit").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                for (k, label, val, max) in [
                    ("group", "mytools.f.group", &mut e.group, 60usize),
                    ("label", "mytools.f.label", &mut e.label, 80),
                    ("macro", "mytools.f.macro", &mut e.macro_, 4000),
                ] {
                    let lab = ui.label(t(label));
                    let r = ui.add(crate::shell::field(val).desired_width(260.0).char_limit(max)).labelled_by(lab.id);
                    if r.changed() {
                        acts.push(("mytools.field", Some(format!("{k}={val}"))));
                    }
                    ui.end_row();
                    if k == "macro" {
                        ui.label("");
                        ui.label(RichText::new(t("mytools.f.macro.hint")).small().color(tk.text_dim));
                        ui.end_row();
                    }
                }
                ui.label(t("mytools.f.key"));
                let shown = if e.key.is_empty() { t("mytools.key.placeholder") } else { e.key.clone() };
                let r = ui.add(egui::Button::new(RichText::new(shown).monospace()).selected(e.capturing).min_size(egui::vec2(260.0, 20.0)));
                if r.clicked() {
                    e.capturing = !e.capturing;
                }
                if r.clicked_elsewhere() {
                    e.capturing = false;
                }
                ui.end_row();
                ui.label("");
                ui.label(RichText::new(t("mytools.f.key.hint")).small().color(tk.text_dim));
                ui.end_row();
                ui.label(t("mytools.f.icon"));
                ui.horizontal(|ui| {
                    match (icon, e.icon.is_some()) {
                        (Some(id), _) => {
                            ui.add(egui::Image::new((id, egui::vec2(24.0, 24.0))));
                        }
                        (None, true) => {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                            crate::ribbon::paint_icon(ui.painter(), r, "star", tk.text);
                        }
                        (None, false) => {
                            ui.label(RichText::new(t("mytools.icon.none")).small().color(tk.text_dim));
                        }
                    }
                    if ui.button(t("mytools.icon.pick")).clicked() {
                        acts.push(("mytools.icon", None));
                    }
                    if e.icon.is_some() && ui.link(t("mytools.icon.remove")).clicked() {
                        acts.push(("mytools.icon.remove", None));
                    }
                });
                ui.end_row();
            });
            if !e.err.is_empty() {
                ui.label(RichText::new(&e.err).color(tk.text_strong));
            }
            ui.horizontal(|ui| {
                if ui.button(t("mytools.cancel")).clicked() {
                    acts.push(("mytools.cancel", None));
                }
                if ui.button(t("mytools.save")).clicked() {
                    acts.push(("mytools.save", None));
                }
            });
        });
    // the fields typed and the key box's state go back before the actions run
    if let Some(cur) = app.mytools.edit.as_mut() {
        cur.group = e.group.clone();
        cur.label = e.label.clone();
        cur.macro_ = e.macro_.clone();
        cur.capturing = e.capturing;
    }
    if !open {
        acts.push(("mytools.cancel", None));
    }
    for (id, name) in acts {
        if id == "mytools.field" {
            continue;
        }
        if let Err(err) = app.run(id, &Params { name, ..Default::default() }) {
            app.message = err;
        }
    }
}

/// The bar's My tools groups (the toolbar after the app's own groups, the menu bar's More, the ribbon's Add-ons tab):
/// (group name, [(tool id, label, tooltip, PNG icon)]).
#[allow(clippy::type_complexity)]
pub fn bar_items(app: &mut UiApp, ctx: &egui::Context) -> Vec<(String, Vec<(String, String, String, Option<egui::TextureId>)>)> {
    let groups: Vec<(String, Vec<(String, String, Option<String>, Option<String>)>)> = app
        .mytools
        .data
        .groups
        .iter()
        .map(|g| (g.name.clone(), g.tools.iter().map(|x| (x.id.clone(), x.label.clone(), x.key.clone(), x.icon.clone())).collect()))
        .collect();
    groups
        .into_iter()
        .map(|(name, tools)| {
            let tools = tools
                .into_iter()
                .map(|(id, label, key, icon)| {
                    let tex = icon_texture(app, ctx, icon.as_deref());
                    let tip = match &key {
                        Some(k) => tv("mytools.button.title.key", &[("label", label.clone()), ("key", k.clone())]),
                        None => label.clone(),
                    };
                    (id, label, tip, tex)
                })
                .collect();
            (name, tools)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(app: &UiApp) -> usize {
        app.editor.as_ref().map_or(0, |ed| ed.eng.doc().entities().filter(|e| matches!(e.geo, cc_cad::entity::Geo::Line(_))).count())
    }

    /// M4.59: a button made in the dialog, its macro run (with a pause), its key refused when taken, kept with the
    /// settings; a CUIx file's toolbar imported.
    #[test]
    fn buttons_macros_and_cuix() {
        cc_lang::with(cc_lang::Locale::En, || {
            let mut app = UiApp::default();
            app.run("file.new", &Params::default()).unwrap();
            let run = |app: &mut UiApp, id: &str, name: &str| app.run(id, &Params { name: Some(name.into()), ..Default::default() });
            run(&mut app, "mytools.add", "").unwrap();
            run(&mut app, "mytools.field", "label=Two lines").unwrap();
            run(&mut app, "mytools.field", r"macro=^C^C_LINE;0,0;10,0;;^C^CLINE;5,5;\;").unwrap();
            run(&mut app, "mytools.field", "key=Ctrl+S").unwrap();
            run(&mut app, "mytools.save", "").unwrap();
            assert!(app.mytools.edit.as_ref().is_some_and(|e| e.err.contains("Ctrl+S")), "a reserved key is refused");
            run(&mut app, "mytools.field", "key=ctrl+shift+l").unwrap();
            run(&mut app, "mytools.save", "").unwrap();
            assert!(app.mytools.edit.is_none());
            let id = app.mytools.data.groups[0].tools[0].id.clone();
            assert_eq!(app.mytools.data.groups[0].tools[0].key.as_deref(), Some("Ctrl+Shift+L"));
            run(&mut app, "mytools.run", &id).unwrap();
            assert_eq!(lines(&app), 1);
            // the pause waits for the person's own point
            assert!(app.mytools.run.is_some());
            run(&mut app, "cmd.line", "20,5").unwrap();
            pump(&mut app);
            assert!(app.mytools.run.is_none());
            assert_eq!(lines(&app), 2);
            // a command CadCraft lacks: the button says so
            assert!(!start(&mut app, "^C^CNOSUCH", "X"));
            // kept with the settings
            let v = app.settings_json();
            let mut b = UiApp::default();
            b.load_settings(&v);
            assert_eq!(b.mytools.data, app.mytools.data);
            // a CUIx toolbar
            let cui = r#"<CustSection><MenuGroup><MacroGroup><MenuMacro UID="M"><Macro><Name>Circle</Name><Command>^C^CCIRCLE</Command></Macro></MenuMacro></MacroGroup>
              <MenuRoot><ToolbarRoot><Toolbar><Name>Mine</Name><ToolbarButton><Name>C</Name><MenuItem><MacroRef MenuMacroID="M"/></MenuItem></ToolbarButton></Toolbar></ToolbarRoot></MenuRoot></MenuGroup></CustSection>"#;
            let z = cc_engine::sheetset_io::zip(
                &[("acad.cui".to_string(), cui.as_bytes().to_vec())],
                cc_engine::sheetset_io::DosTime { time: 0, date: 33 },
            );
            import_cuix(&mut app, &z);
            assert_eq!(app.mytools.data.groups.len(), 2);
            assert!(app.mytools.said.starts_with("CUIx imported: 1 groups, 1 buttons"), "{}", app.mytools.said);
            import_cuix(&mut app, b"not a zip");
            assert!(app.mytools.said.contains("not a ZIP"));
            run(&mut app, "mytools.delete", &id).unwrap();
            assert_eq!(app.mytools.data.groups.len(), 1);
            import_json(&mut app, "{nope");
            assert_eq!(app.mytools.said, "That is not a My tools file.");
        });
    }
}
