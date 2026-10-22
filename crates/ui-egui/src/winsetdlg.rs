//! The drafting window's own settings dialogs: SHORTCUTS' keyboard-shortcuts editor (keys-ui.js over cc-engine
//! `shortcuts`: a key of your own runs what you wrote at the command line; refused keys say why) and COMPARESETTINGS'
//! Compare settings dialog (compare-ui.js over cc-engine `compare`: dimension styles, text styles, layers, sheets and
//! units side by side, the differences marked, a value copied across in one undo step). Also LASSOMODE's status line.
//! Every control is a `keys.*` / `cmp.*` command. The keys are kept with the app's preferences (`prefs_json`).
//!
//! The window holds one drawing, so Compare's right-hand drawing is this one (two items of it side by side).

use cc_engine::compare::{CATS, compare_items, copy_value, items_of, value_text};
use cc_engine::shortcuts::{clean_shortcuts, key_problem, norm_key, shortcuts_data};
use egui::RichText;
use serde_json::Value;

use crate::{Params, UiApp};

/// The dialogs' state.
#[derive(Default)]
pub struct WinSet {
    pub keys_open: bool,
    /// key → what it runs, in the order added
    pub keys: Vec<(String, String)>,
    /// the key picked in the editor, the command typed, the last message
    pub picked: String,
    pub command: String,
    pub said: String,
    capturing: bool,
    pub cmp_open: bool,
    pub cat: String,
    pub a: Option<String>,
    pub b: Option<String>,
    pub only_diff: bool,
}

/// UI commands of the dialogs.
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("keys.open", "Keyboard shortcuts", "", "{} → the editor of keys for any command (SHORTCUTS does the same)"),
    ("keys.close", "Keyboard shortcuts: close", "", "{}"),
    ("keys.add", "Keyboard shortcuts: add", "", "{name: \"<key>=<text>\"} → the key runs that text at the command line (Ctrl+Alt+L=LINE)"),
    ("keys.remove", "Keyboard shortcuts: remove", "", "{name: key}"),
    ("keys.run", "Keyboard shortcuts: press one", "", "{name: key} → runs what the key runs"),
    ("cmp.open", "Compare settings", "", "{} → the Compare settings dialog (COMPARESETTINGS does the same)"),
    ("cmp.close", "Compare settings: close", "", "{}"),
    (
        "cmp.set",
        "Compare settings: choose",
        "",
        "{name: \"cat=dimstyle|textstyle|layer|sheet|units\" | \"a=<item>\" | \"b=<item>\" | \"only=true|false\"}",
    ),
    ("cmp.copy", "Compare settings: copy a value", "", "{name: \"right:<key>\" | \"left:<key>\"} → one undo step"),
];

fn t(k: &str) -> String {
    crate::edit::lang_text(k, &[])
}
fn tv(k: &str, vars: &[(&str, String)]) -> String {
    crate::edit::lang_text(k, vars)
}

fn known(name: &str) -> bool {
    cc_engine::cmd::lookup(name).is_some()
}

impl WinSet {
    /// What the app keeps between runs (`cadcraft.shortcuts`' JSON).
    pub fn prefs_json(&self) -> Value {
        serde_json::json!({ "shortcuts": shortcuts_data(&self.keys) })
    }
    /// Read the kept keys back (untrusted: cleanShortcuts drops what it must).
    pub fn load_prefs(&mut self, v: &Value) {
        if let Some(s) = v.get("shortcuts") {
            self.keys = clean_shortcuts(s, &known, &[]).0;
        }
    }
}

/// `keys.*` / `cmp.*` commands.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    let name = params.name.clone().unwrap_or_default();
    let w = &mut app.winset;
    match id {
        "keys.open" => {
            w.keys_open = true;
            w.said.clear();
        }
        "keys.close" => w.keys_open = false,
        "keys.add" => {
            let (k, text) = name.split_once('=').ok_or("keys.add: name = \"<key>=<text>\"")?;
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let Some(k) = norm_key(k) else {
                w.said = tv("keys.err.format", &[("key", String::new())]);
                return Err(w.said.clone());
            };
            let taken: Vec<(String, String)> = w.keys.iter().filter(|(x, _)| *x != k).cloned().collect();
            if let Some(why) = key_problem(&k, &taken) {
                let other = taken.iter().find(|(x, _)| *x == k).map(|(_, o)| o.clone()).unwrap_or_default();
                w.said = tv(&format!("keys.err.{why}"), &[("key", k.clone()), ("other", other)]);
                return Err(w.said.clone());
            }
            let cmd = text.split(' ').next().unwrap_or("").to_string();
            if text.is_empty() || !known(&cmd) {
                w.said = tv("keys.err.command", &[("key", k.clone()), ("cmd", if cmd.is_empty() { "?".into() } else { cmd })]);
                return Err(w.said.clone());
            }
            match w.keys.iter_mut().find(|(x, _)| *x == k) {
                Some(slot) => slot.1.clone_from(&text),
                None => w.keys.push((k.clone(), text.clone())),
            }
            w.said = tv("keys.added", &[("key", k), ("text", text)]);
            w.picked.clear();
            w.command.clear();
        }
        "keys.remove" => {
            let k = norm_key(&name).unwrap_or(name);
            w.keys.retain(|(x, _)| *x != k);
            w.said = tv("keys.removed", &[("key", k)]);
        }
        "keys.run" => {
            let k = norm_key(&name).ok_or("keys.run: not a key")?;
            let text = w.keys.iter().find(|(x, _)| *x == k).map(|(_, t)| t.clone()).ok_or(format!("keys.run: {k} runs nothing"))?;
            run_text(app, &k, &text);
        }
        "cmp.open" => {
            w.cmp_open = true;
            if w.cat.is_empty() {
                w.cat = "dimstyle".into();
            }
        }
        "cmp.close" => w.cmp_open = false,
        "cmp.set" => {
            let (k, v) = name.split_once('=').ok_or("cmp.set: name = \"<what>=<value>\"")?;
            match k {
                "cat" if CATS.contains(&v) => {
                    w.cat = v.into();
                    w.a = None;
                    w.b = None;
                }
                "a" => w.a = Some(v.into()),
                "b" => w.b = Some(v.into()),
                "only" => w.only_diff = v == "true",
                _ => return Err(format!("cmp.set: {k} = {v} is not a choice")),
            }
        }
        "cmp.copy" => {
            let (side, key) = name.split_once(':').ok_or("cmp.copy: name = \"right:<key>\" | \"left:<key>\"")?;
            copy(app, side == "right", key)?;
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// keys-ui.js `run(text)`: Esc a running command, then the command and what to type after it.
fn run_text(app: &mut UiApp, key: &str, text: &str) {
    app.message = tv("keys.run", &[("key", key.to_string()), ("text", text.to_string())]);
    if let Some(ed) = app.editor.as_mut()
        && ed.eng.busy()
    {
        ed.eng.escape();
    }
    let mut parts = text.split(' ');
    if let Some(cmd) = parts.next() {
        app.run_ui_pub("cmd.line", &Params { name: Some(cmd.to_string()), ..Default::default() });
    }
    for r in parts {
        let waiting = app.editor.as_ref().is_some_and(|e| e.eng.expects().is_some());
        if !waiting {
            break;
        }
        app.run_ui_pub("cmd.line", &Params { name: Some(r.to_string()), ..Default::default() });
    }
}

/// The items on each side, picked as compare-ui.js `pickDefaults` picks them.
fn picks(app: &mut UiApp) -> Option<(Vec<String>, Vec<String>)> {
    let ed = app.editor.as_ref()?;
    let d = ed.eng.doc();
    let w = &mut app.winset;
    let items = items_of(&d, &w.cat);
    if !w.a.as_ref().is_some_and(|a| items.contains(a)) {
        w.a = items.first().cloned();
    }
    if !w.b.as_ref().is_some_and(|b| items.contains(b)) {
        // the right-hand drawing is this one: another item than the left one
        w.b = items.iter().find(|x| Some(*x) != w.a.as_ref()).or(items.first()).cloned();
    }
    Some((items.clone(), items))
}

fn label(cat: &str, key: &str) -> String {
    let l = crate::edit::lang_en_pub();
    let k = if cat == "dimstyle" { format!("props.dim.{key}") } else { format!("cmp.k.{cat}.{}", key.to_lowercase()) };
    if l.has(&k) { l.s(&k) } else { key.to_string() }
}
fn item_label(n: &str) -> String {
    match n {
        "*CURRENT" => t("cmp.current_dim"),
        "*DRAWING" => t("cmp.drawing_units"),
        _ => n.to_string(),
    }
}

/// compare-ui.js `copy(r, toRight)`: one value across, one undo step; the command line says so.
fn copy(app: &mut UiApp, to_right: bool, key: &str) -> Result<(), String> {
    picks(app);
    let (cat, a, b) = (app.winset.cat.clone(), app.winset.a.clone(), app.winset.b.clone());
    let ed = app.editor.as_mut().ok_or("cmp.copy: no drawing is open for editing")?;
    let row = {
        let d = ed.eng.doc();
        compare_items(&cat, &d, a.as_deref(), &d, b.as_deref()).into_iter().find(|r| r.key == key)
    };
    let row = row.ok_or(format!("cmp.copy: no property {key}"))?;
    let (item, v) = if to_right { (b, row.a) } else { (a, row.b) };
    let (Some(item), Some(v)) = (item, v) else { return Ok(()) };
    let changed = {
        let mut d = ed.eng.doc_mut();
        copy_value(&mut d, &cat, &item, key, &v)
    };
    if changed {
        ed.hs.borrow_mut().push_log(&tv("cmp.copied", &[("what", label(&cat, key)), ("item", item_label(&item))]));
    }
    app.sync_editor();
    Ok(())
}

/// After every input: the requests SHORTCUTS / COMPARESETTINGS / LASSOMODE made.
pub fn sync(app: &mut UiApp) {
    let Some(ed) = app.editor.as_ref() else { return };
    let (keys, cmp, status) = {
        let mut h = ed.hs.borrow_mut();
        (std::mem::take(&mut h.keys_open), std::mem::take(&mut h.cmp_open), h.status.take())
    };
    if keys {
        app.run_ui_pub("keys.open", &Params::default());
    }
    if cmp {
        app.run_ui_pub("cmp.open", &Params::default());
    }
    if let Some(s) = status {
        app.message = s;
    }
}

/// shortcuts.js `keyOfEvent`: an egui key press as "Ctrl+Alt+Shift+<key>" (normKey's form), None for a key CadCraft
/// does not take.
pub fn key_of(key: egui::Key, m: egui::Modifiers) -> Option<String> {
    use egui::Key as K;
    let name: String = match key {
        K::ArrowLeft => "Left".into(),
        K::ArrowRight => "Right".into(),
        K::ArrowUp => "Up".into(),
        K::ArrowDown => "Down".into(),
        K::Space => "Space".into(),
        K::Escape => "Escape".into(),
        K::Tab => "Tab".into(),
        K::Enter => "Enter".into(),
        K::Backspace => "Backspace".into(),
        K::Delete => "Delete".into(),
        K::Insert => "Insert".into(),
        K::Home => "Home".into(),
        K::End => "End".into(),
        K::PageUp => "PageUp".into(),
        K::PageDown => "PageDown".into(),
        K::Comma => ",".into(),
        K::Period => ".".into(),
        K::Semicolon => ";".into(),
        K::Quote => "'".into(),
        K::Slash => "/".into(),
        K::Backslash => "\\".into(),
        K::OpenBracket => "[".into(),
        K::CloseBracket => "]".into(),
        K::Equals => "=".into(),
        K::Backtick => "`".into(),
        K::Minus => "-".into(),
        other => {
            let n = other.name();
            // A–Z, 0–9 (egui's Num0 … Num9 are named "0" … "9"), F1–F12
            if n.chars().count() == 1 || (n.starts_with('F') && n.len() <= 3) { n.to_string() } else { return None }
        }
    };
    let mut parts = Vec::new();
    if m.ctrl || m.command || m.mac_cmd {
        parts.push("Ctrl".to_string());
    }
    if m.alt {
        parts.push("Alt".into());
    }
    if m.shift {
        parts.push("Shift".into());
    }
    parts.push(name);
    norm_key(&parts.join("+"))
}

/// The dialogs, and the keys themselves (a key of your own runs its text; not while a dialog is open).
pub fn window(app: &mut UiApp, ctx: &egui::Context) {
    if app.editor.is_none() {
        return;
    }
    // keys pressed: the capture field takes them, else a key of our own runs
    let pressed: Vec<(egui::Key, egui::Modifiers)> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
            .collect()
    });
    if app.winset.capturing {
        for (k, m) in &pressed {
            if matches!(k, egui::Key::Backspace | egui::Key::Delete) && !m.any() {
                app.winset.picked.clear();
            } else if *k != egui::Key::Tab
                && let Some(n) = key_of(*k, *m)
            {
                app.winset.picked = n;
            }
        }
    } else if !app.winset.keys_open && !app.winset.cmp_open && !app.plot.open && !app.winset.keys.is_empty() {
        for (k, m) in &pressed {
            if let Some(n) = key_of(*k, *m)
                && app.winset.keys.iter().any(|(x, _)| *x == n)
            {
                app.run_ui_pub("keys.run", &Params { name: Some(n), ..Default::default() });
            }
        }
    }
    if app.winset.keys_open {
        keys_window(app, ctx);
    } else {
        app.winset.capturing = false;
    }
    if app.winset.cmp_open {
        cmp_window(app, ctx);
    }
}

fn keys_window(app: &mut UiApp, ctx: &egui::Context) {
    let tok = app.tokens;
    let mut open = true;
    let mut acts: Vec<(&str, String)> = Vec::new();
    let w = &mut app.winset;
    egui::Window::new(t("keys.title"))
        .id(egui::Id::unique("cad_keys"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(440.0)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_max_width(440.0);
            ui.label(RichText::new(t("keys.text")).color(tok.text_dim));
            ui.add_space(4.0);
            let mut sorted = w.keys.clone();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            if sorted.is_empty() {
                ui.label(RichText::new(t("keys.none")).italics().color(tok.text_dim));
            }
            egui::Grid::new("keys_list").num_columns(3).spacing([12.0, 4.0]).show(ui, |ui| {
                for (k, text) in &sorted {
                    ui.label(RichText::new(k).monospace().color(tok.text_strong));
                    ui.label(text);
                    if ui.link(t("keys.remove")).clicked() {
                        acts.push(("keys.remove", k.clone()));
                    }
                    ui.end_row();
                }
            });
            ui.separator();
            egui::Grid::new("keys_add").num_columns(2).show(ui, |ui| {
                ui.label(t("keys.key"));
                let shown = if w.picked.is_empty() { t("keys.press") } else { w.picked.clone() };
                let r = ui.add(egui::Button::new(RichText::new(shown).monospace()).selected(w.capturing).min_size(egui::vec2(220.0, 20.0)));
                if r.clicked() {
                    w.capturing = true;
                }
                if r.clicked_elsewhere() {
                    w.capturing = false;
                }
                ui.end_row();
                ui.label(t("keys.command"));
                ui.add(egui::TextEdit::singleline(&mut w.command).hint_text(t("keys.command.placeholder")).desired_width(220.0));
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button(t("keys.add")).clicked() {
                    acts.push(("keys.add", format!("{}={}", w.picked, w.command.trim())));
                }
            });
            if !w.said.is_empty() {
                ui.label(RichText::new(&w.said).color(tok.text));
            }
        });
    if !open {
        app.winset.keys_open = false;
    }
    for (id, name) in acts {
        // a refused key is said in the dialog, not as the app's error
        let _ = run(app, id, &Params { name: Some(name), ..Default::default() });
    }
}

fn cmp_window(app: &mut UiApp, ctx: &egui::Context) {
    let tok = app.tokens;
    let Some((left, right)) = picks(app) else { return };
    let name = app.editor.as_ref().map(|e| e.name.clone()).unwrap_or_default();
    let (cat, a, b, only) = (app.winset.cat.clone(), app.winset.a.clone(), app.winset.b.clone(), app.winset.only_diff);
    let rows = match app.editor.as_ref() {
        Some(ed) if a.is_some() || b.is_some() => {
            let d = ed.eng.doc();
            compare_items(&cat, &d, a.as_deref(), &d, b.as_deref())
        }
        _ => Vec::new(),
    };
    let diff = rows.iter().filter(|r| !r.same).count();
    let mut open = true;
    let mut acts: Vec<(&str, String)> = Vec::new();
    egui::Window::new(t("cmp.title"))
        .id(egui::Id::unique("cad_cmp"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([560.0, 460.0])
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            let combo =
                |ui: &mut egui::Ui, id: &str, cur: String, items: &[(String, String)], key: &str, acts: &mut Vec<(&str, String)>| {
                    egui::ComboBox::new(id, "").selected_text(cur).width(200.0).show_ui(ui, |ui| {
                        for (v, l) in items {
                            if ui.selectable_label(false, l).clicked() {
                                acts.push(("cmp.set", format!("{key}={v}")));
                            }
                        }
                    });
                };
            egui::Grid::new("cmp_pick").num_columns(2).show(ui, |ui| {
                ui.label(t("cmp.what"));
                let cats: Vec<(String, String)> = CATS.iter().map(|c| ((*c).to_string(), t(&format!("cmp.cat.{c}")))).collect();
                combo(ui, "cmp_cat", t(&format!("cmp.cat.{cat}")), &cats, "cat", &mut acts);
                ui.end_row();
                ui.label(t("cmp.left"));
                let li: Vec<(String, String)> = left.iter().map(|n| (n.clone(), item_label(n))).collect();
                combo(ui, "cmp_a", a.as_deref().map(item_label).unwrap_or_default(), &li, "a", &mut acts);
                ui.end_row();
                ui.label(t("cmp.right_drawing"));
                ui.label(tv("cmp.this_drawing", &[("name", name.clone())]));
                ui.end_row();
                ui.label(t("cmp.right"));
                let ri: Vec<(String, String)> = right.iter().map(|n| (n.clone(), item_label(n))).collect();
                combo(ui, "cmp_b", b.as_deref().map(item_label).unwrap_or_default(), &ri, "b", &mut acts);
                ui.end_row();
            });
            let mut o = only;
            if ui.checkbox(&mut o, t("cmp.only_diff")).changed() {
                acts.push(("cmp.set", format!("only={o}")));
            }
            let note = if rows.is_empty() {
                t("cmp.nothing")
            } else if diff > 0 {
                tv("cmp.differ", &[("n", diff.to_string()), ("of", rows.len().to_string())])
            } else {
                tv("cmp.same", &[("n", rows.len().to_string())])
            };
            ui.label(RichText::new(note).color(tok.text_dim));
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                egui::Grid::new("cmp_rows").num_columns(4).striped(true).spacing([10.0, 3.0]).show(ui, |ui| {
                    ui.label(RichText::new(t("cmp.property")).strong());
                    ui.label(RichText::new(a.as_deref().map(item_label).unwrap_or_default()).strong());
                    ui.label("");
                    ui.label(RichText::new(b.as_deref().map(item_label).unwrap_or_default()).strong());
                    ui.end_row();
                    for r in rows.iter().filter(|r| !only || !r.same) {
                        let lab = label(&cat, &r.key);
                        let col = if r.same { tok.text } else { tok.accent };
                        ui.label(RichText::new(&lab).color(col));
                        ui.label(value_text(&cat, &r.key, r.a.as_ref()));
                        ui.horizontal(|ui| {
                            let can_r = !r.same && r.a.is_some() && b.is_some();
                            let can_l = !r.same && r.b.is_some() && a.is_some();
                            if ui.add_enabled(can_r, egui::Button::new("→").small()).on_hover_text(t("cmp.copy_right")).clicked() {
                                acts.push(("cmp.copy", format!("right:{}", r.key)));
                            }
                            if ui.add_enabled(can_l, egui::Button::new("←").small()).on_hover_text(t("cmp.copy_left")).clicked() {
                                acts.push(("cmp.copy", format!("left:{}", r.key)));
                            }
                        });
                        ui.label(value_text(&cat, &r.key, r.b.as_ref()));
                        ui.end_row();
                    }
                });
            });
        });
    if !open {
        app.winset.cmp_open = false;
    }
    for (id, name) in acts {
        app.run_ui_pub(id, &Params { name: Some(name), ..Default::default() });
    }
}
