//! BimCraft's editing on the IFC view (bimcraft.js): the model open in cc-bim's editor (ifc-edit.js through
//! `cc_bim::draw::editor`), Properties values changed in place (Name, Object type, Tag, Description; single property
//! values, copy-on-write where exporters share them), Undo / Redo, Save as IFC (a new file, never the original: "<name>
//! (edited).ifc"), File › New building (new-model.js). After every change the 3D view is made again from the edited
//! model (bimcraft.js `rebuild`), keeping the camera, what is hidden and the pick.
//!
//! Everything is a command (`bim.*`, see [`COMMANDS`]); the clock and the GlobalIds' seed are values the session passes
//! in (`bim.clock`), so a scripted session saves the same bytes every time.

use std::sync::Arc;

use cc_bim::draw::editor::{Change, Editor, PropRef, iso_seconds};
use cc_bim::draw::job::{PropSet, prop_refs};
use cc_bim::draw::newmodel::{NewBuilding, levels_problem, new_building, stack_levels};
use cc_bim::draw::units::parse_length;
use cc_bim::wapi::Rand;
use egui::RichText;

use crate::bimtools::bt;
use crate::{ImportedFile, Params, UiApp};

/// The editing commands (id, label, shortcut, params doc), joined into the app's command list.
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    (
        "bim.clock",
        "Fix the clock",
        "",
        "{name: \"<ms since 1970>\"} → saves, prints and new GlobalIds use this clock and seed (scripts, parity)",
    ),
    ("bim.save", "Save as IFC", "Ctrl+S", "{} → a new IFC file with every change (the original is never touched)"),
    ("bim.new", "New building", "", "{name: \"<name>|<levels>|<level height>\"} → a new building (height as typed: 10'-0\" or 3000 mm)"),
    (
        "bim.attr",
        "Change an attribute",
        "",
        "{name: \"Name=…\" | \"ObjectType=…\" | \"Tag=…\" | \"Description=…\"} → on the picked element",
    ),
    (
        "bim.prop",
        "Change a property",
        "",
        "{name: \"<set>/<property>=<value>\"} → a single value of the picked element (type sets end in \" (type)\")",
    ),
    (
        "bim.pick",
        "Pick an element",
        "",
        "{name: \"<expressID>\"} → picks it in the 3D view and the plan (Properties shows it); empty = nothing",
    ),
    ("bim.undo", "Undo", "Ctrl+Z", "{} → takes back the last change of the IFC model"),
    ("bim.redo", "Redo", "Ctrl+Y", "{} → applies the change Undo took back"),
];

/// The editing state of the app's IFC view (one field of `UiApp`).
#[derive(Default)]
pub struct Host {
    /// the open model's editor, made on the first change (opening a file never pays for it)
    pub ed: Option<BimDoc>,
    /// a fixed clock (ms since 1970) for scripts; None = the wall clock
    pub clock: Option<f64>,
    /// the Properties field being typed in (key, text)
    pub editing: Option<(String, String)>,
    /// the New building dialog
    pub new_dlg: Option<NewDlg>,
    /// Draw: the plan view and the building tools
    pub draw: crate::bimplan::Draw,
    /// the last rebuild of the 3D view (ms)
    pub rebuild_ms: f64,
    /// a picked element with no piece in the 3D view (a room picked on the plan)
    pub picked_off: Option<f64>,
}

impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host").field("ed", &self.ed.is_some()).field("clock", &self.clock).finish()
    }
}

/// The New building dialog's fields.
#[derive(Clone, Debug)]
pub struct NewDlg {
    pub name: String,
    pub levels: String,
    pub height: String,
    pub problem: String,
}

impl Default for NewDlg {
    fn default() -> Self {
        NewDlg { name: bt("new.default.name", &[]), levels: "2".into(), height: "10'-0\"".into(), problem: String::new() }
    }
}

/// The model being edited.
pub struct BimDoc {
    pub ed: Editor,
    /// the file name it was opened as
    pub name: String,
}

impl Host {
    /// The clock now (ms).
    pub fn now(&self) -> f64 {
        self.clock
            .unwrap_or_else(|| web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0))
    }
    fn seed(&self) -> u32 {
        // the low bits of the clock: a fixed clock gives the same GlobalIds every run
        let ms = self.now();
        let x = if ms.is_finite() { ms.rem_euclid(4_294_967_296.0) } else { 0.0 };
        crate::bimedit::u32_of(x)
    }
}

/// `x` as a u32 (0 for anything outside).
pub fn u32_of(x: f64) -> u32 {
    if x.is_finite() && (0.0..4_294_967_296.0).contains(&x) { cc_bim::model::u32_of(x) } else { 0 }
}

/// The editor of the open IFC model, made from its bytes on first use.
pub fn doc(app: &mut UiApp) -> Result<&mut BimDoc, String> {
    if app.bimh.ed.is_none() {
        let bytes = app.ifc3_bytes().ok_or("Open an IFC model or start a new building first.")?;
        let seed = app.bimh.seed();
        let ed = Editor::open(Arc::from(bytes.as_slice()), seed).map_err(|e| e.0)?;
        app.bimh.ed = Some(BimDoc { ed, name: app.ifc_name().to_string() });
    }
    app.bimh.ed.as_mut().ok_or_else(|| "no model".to_string())
}

/// Run an editing command.
pub fn run(app: &mut UiApp, id: &str, p: &Params) -> Result<(), String> {
    let name = p.name.clone().unwrap_or_default();
    match id {
        "bim.clock" => {
            let ms = cc_bim::js::str_num(&name);
            if !ms.is_finite() {
                return Err("bim.clock: name is the clock in ms".into());
            }
            app.bimh.clock = Some(ms);
            if let Some(d) = app.bimh.ed.as_mut() {
                d.ed.api.rand = Rand::new(u32_of(ms.rem_euclid(4_294_967_296.0)));
            }
            Ok(())
        }
        "bim.save" => save(app),
        "bim.new" => {
            let mut it = name.split('|');
            let (n, lv, h) = (
                it.next().unwrap_or("").trim().to_string(),
                it.next().unwrap_or("1").trim().to_string(),
                it.next().unwrap_or("3000 mm").trim().to_string(),
            );
            new_bldg(app, &n, &lv, &h)
        }
        "bim.attr" => {
            let (k, v) = name.split_once('=').ok_or("bim.attr: name is Attr=value")?;
            let pid = app.picked_product().ok_or("Nothing is picked.")?;
            set_attr(app, pid, k.trim(), v)
        }
        "bim.prop" => {
            let (k, v) = name.split_once('=').ok_or("bim.prop: name is Set/Prop=value")?;
            let (set, prop) = k.rsplit_once('/').ok_or("bim.prop: name is Set/Prop=value")?;
            let pid = app.picked_product().ok_or("Nothing is picked.")?;
            set_prop(app, pid, set, prop, v)
        }
        "bim.pick" => {
            let id = cc_bim::js::str_num(&name);
            if name.trim().is_empty() || !id.is_finite() {
                app.select_product_none();
            } else {
                app.select_product(id);
            }
            Ok(())
        }
        "bim.undo" => undo(app),
        "bim.redo" => redo(app),
        other => Err(format!("unknown command {other}")),
    }
}

/// "Walk-in cooler" (the element as the change list names it: class without Ifc and its name).
fn label_of(app: &UiApp, pid: f64) -> String {
    let cls = app.sel_props.first().and_then(|g| g.rows.iter().find(|r| r.0 == "Class")).map(|r| r.1.clone()).unwrap_or_default();
    let name = app.sel_props.first().and_then(|g| g.rows.iter().find(|r| r.0 == "Name")).map(|r| r.1.clone()).unwrap_or_default();
    let cls = cls.strip_prefix("Ifc").unwrap_or(&cls).to_string();
    format!("{cls} {}", if name.is_empty() { format!("#{}", cc_bim::js::num_str(pid)) } else { name })
}

fn set_attr(app: &mut UiApp, pid: f64, attr: &str, value: &str) -> Result<(), String> {
    let label = label_of(app, pid);
    let d = doc(app)?;
    let changed = d.ed.set_attribute(pid, attr, Some(value), true).map_err(|e| e.0)?;
    if changed {
        d.ed.tag_last(&label);
        after_change(app, Some(pid), true);
    }
    Ok(())
}

fn set_prop(app: &mut UiApp, pid: f64, set: &str, prop: &str, value: &str) -> Result<(), String> {
    let label = label_of(app, pid);
    let d = doc(app)?;
    let refs = prop_refs(&d.ed, pid);
    let r = refs.iter().find(|x| x.0 == set).and_then(|x| x.1.iter().find(|r| r.0 == prop)).and_then(|r| r.1.clone());
    let r = r.ok_or_else(|| format!("{set} / {prop} cannot be changed here (only single values can)."))?;
    let changed = d.ed.set_property(pid, &r, value, true).map_err(|e| e.0)?;
    if changed {
        d.ed.tag_last(&label);
        after_change(app, Some(pid), true);
    }
    Ok(())
}

/// After a change: the model rebuilt from the edited file (the 3D view, and the plan when Draw is on), the pick kept.
pub fn after_change(app: &mut UiApp, select: Option<f64>, rebuild_3d: bool) {
    if rebuild_3d {
        rebuild(app, select);
    }
    crate::bimplan::after_change(app);
}

/// bimcraft.js `rebuild(selectId)`: the 3D view made again from `editor.save()`.
pub fn rebuild(app: &mut UiApp, select: Option<f64>) {
    let ts = iso_seconds(app.bimh.now());
    let Some(d) = app.bimh.ed.as_ref() else { return };
    let bytes = match d.ed.save(&ts) {
        Ok(b) => b,
        Err(e) => {
            app.error = Some(e.0);
            return;
        }
    };
    let t0 = web_time::Instant::now();
    app.show_ifc_bytes(Arc::new(bytes), true);
    app.bimh.rebuild_ms = t0.elapsed().as_secs_f64() * 1000.0;
    if let Some(id) = select {
        app.select_product(id);
    }
}

fn undo(app: &mut UiApp) -> Result<(), String> {
    let Some(d) = app.bimh.ed.as_mut() else { return Err(bt("edit.undo.nothing", &[])) };
    let c = d.ed.undo().map_err(|e| e.0)?.cloned();
    let Some(c) = c else { return Err(bt("edit.undo.nothing", &[])) };
    let msg = match &c {
        Change::Model { label, .. } => bt("edit.undid.step", &[("what", if label.is_empty() { "model" } else { label })]),
        Change::Attr { attr, .. } => bt("edit.undid", &[("what", attr)]),
        Change::Prop { r, .. } => bt("edit.undid", &[("what", &r.prop_name)]),
        Change::Place { label, .. } => bt("edit.undid.step", &[("what", if label.is_empty() { "place" } else { label })]),
    };
    after_change(app, None, true);
    app.message = msg;
    Ok(())
}

fn redo(app: &mut UiApp) -> Result<(), String> {
    let Some(d) = app.bimh.ed.as_mut() else { return Err(bt("edit.redo.nothing", &[])) };
    let c = match d.ed.redo() {
        Ok(c) => c.cloned(),
        Err(e) => return Err(bt("edit.redo.failed", &[("why", &e.0)])),
    };
    let Some(c) = c else { return Err(bt("edit.redo.nothing", &[])) };
    let (msg, sel) = match &c {
        Change::Model { label, id, .. } => (bt("edit.redid.step", &[("what", if label.is_empty() { "model" } else { label })]), *id),
        Change::Attr { attr, id, .. } => (bt("edit.redid", &[("what", attr)]), Some(*id)),
        Change::Prop { r, id, .. } => (bt("edit.redid", &[("what", &r.prop_name)]), Some(*id)),
        Change::Place { label, id, .. } => (bt("edit.redid.step", &[("what", if label.is_empty() { "place" } else { label })]), Some(*id)),
    };
    after_change(app, sel, true);
    app.message = msg;
    Ok(())
}

/// Whether there is something to save (changes, or a new building not saved yet).
pub fn unsaved(app: &UiApp) -> bool {
    app.bimh.ed.as_ref().is_some_and(|d| !d.ed.changes.is_empty() || d.ed.is_new)
}

/// `saveIfc()`: a NEW file "<name> (edited).ifc" (a new building keeps its own name); the change list starts again.
fn save(app: &mut UiApp) -> Result<(), String> {
    if !unsaved(app) {
        app.message = bt("save.ifc", &[]);
        return Ok(());
    }
    let ts = iso_seconds(app.bimh.now());
    let Some(d) = app.bimh.ed.as_ref() else { return Ok(()) };
    let bytes = d.ed.save(&ts).map_err(|e| e.0)?;
    let base = d.name.clone();
    let name = if d.ed.is_new {
        base
    } else {
        let stem = if base.to_ascii_lowercase().ends_with(".ifc") {
            base.get(..base.len().saturating_sub(4)).unwrap_or(&base).to_string()
        } else {
            base
        };
        format!("{stem} (edited).ifc")
    };
    let n = d.ed.changes.len();
    let size = bytes.len();
    let save = app.services.save.as_mut().ok_or("Saving is not available here")?;
    match save(&name, bytes) {
        Ok(Some(to)) => {
            if let Some(d) = app.bimh.ed.as_mut() {
                d.ed.changes.clear();
                d.ed.is_new = false;
            }
            app.message = bt("save.done", &[("file", &to), ("n", &n.to_string()), ("bytes", &grouped(size))]);
            Ok(())
        }
        Ok(None) => {
            app.message = bt("save.cancelled", &[]);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// `n.toLocaleString('en-US')`
pub fn grouped(n: usize) -> String {
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

/// File › New building: check, write new-model.js's IFC, open it, Draw on.
pub fn new_bldg(app: &mut UiApp, name: &str, levels: &str, height: &str) -> Result<(), String> {
    let n = cc_bim::draw::m::round(cc_bim::js::str_num(levels));
    // the dialog's default unit: feet where the app speaks US English (bimcraft.js metricDefault)
    let h = parse_length(height, 304.8);
    let lv = if (1.0..=1e6).contains(&n) { stack_levels(u32_of(n) as usize, h) } else { Vec::new() };
    let bad = if crate::bimedit::trim(name).is_empty() { "new.err.name" } else { levels_problem(&lv) };
    if !bad.is_empty() {
        return Err(bt(bad, &[]));
    }
    let name = crate::bimedit::trim(name).to_string();
    let file_name = format!("{}.ifc", safe_name(&name));
    let mut rand = Rand::new(app.bimh.seed());
    let nb = NewBuilding {
        name: name.clone(),
        site_name: bt("new.site", &[]),
        levels: lv.clone(),
        author: String::new(),
        file_name: file_name.clone(),
        now_ms: app.bimh.now(),
        version: String::new(),
    };
    let text = new_building(&nb, &mut rand).map_err(|k| bt(k, &[]))?;
    let bytes = text.into_bytes();
    app.open_file(&ImportedFile { name: file_name.clone(), bytes: bytes.clone(), dir: None });
    let mut ed = Editor::open(Arc::from(bytes.into_boxed_slice()), 0).map_err(|e| e.0)?;
    ed.api.rand = rand;
    ed.is_new = true;
    app.bimh.ed = Some(BimDoc { ed, name: file_name });
    app.message = bt("new.done", &[("name", &name), ("n", &lv.len().to_string())]);
    crate::bimplan::show(app, true);
    Ok(())
}

/// `name.replace(/[\\/:*?"<>|]+/g, '-')`
fn safe_name(name: &str) -> String {
    let mut out = String::new();
    let mut run = false;
    for c in name.chars() {
        if "\\/:*?\"<>|".contains(c) {
            if !run {
                out.push('-');
            }
            run = true;
        } else {
            out.push(c);
            run = false;
        }
    }
    out
}

/// JS `trim`.
pub fn trim(s: &str) -> &str {
    cc_cad::js::trim(s)
}

// ------------------------------------------------------------------------------------------------------------- panels

/// The Save as IFC button and the change count, for the Model panel's header.
pub fn save_button(app: &mut UiApp, ui: &mut egui::Ui) {
    let n = app.bimh.ed.as_ref().map_or(0, |d| d.ed.changes.len());
    let label = if n > 0 { bt("save.ifc.count", &[("n", &n.to_string())]) } else { bt("save.ifc", &[]) };
    let can = unsaved(app);
    if ui.add_enabled(can, egui::Button::new(label)).on_hover_text(bt("ui.save.note", &[])).clicked() {
        app.run_ui("bim.save", &Params::default());
    }
    let undo_ok = app.bimh.ed.as_ref().is_some_and(|d| !d.ed.changes.is_empty());
    if ui.add_enabled(undo_ok, egui::Button::new(bt("ui.undo", &[]))).clicked() {
        app.run_ui("bim.undo", &Params::default());
    }
    let redo_ok = app.bimh.ed.as_ref().is_some_and(|d| d.ed.can_redo());
    if ui.add_enabled(redo_ok, egui::Button::new(bt("ui.redo", &[]))).clicked() {
        app.run_ui("bim.redo", &Params::default());
    }
}

/// The editable part of Properties for the picked element: its attributes and single property values.
pub fn props_edit(app: &mut UiApp, ui: &mut egui::Ui) {
    let Some(pid) = app.picked_product() else { return };
    let t = app.tokens;
    // what the element is now (attributes from the edited model when there is one)
    let attrs: Vec<(&str, String, String)> = {
        let line = app.bimh.ed.as_ref().and_then(|d| d.ed.api.line(pid).ok().flatten());
        let from_props =
            |k: &str| app.sel_props.first().and_then(|g| g.rows.iter().find(|r| r.0 == k)).map(|r| r.1.clone()).unwrap_or_default();
        [("Name", "prop.name"), ("ObjectType", "prop.objtype"), ("Tag", "prop.tag"), ("Description", "prop.desc")]
            .iter()
            .map(|(k, key)| {
                let v = match &line {
                    Some(l) => l.a(k).text_or_empty(),
                    None => from_props(match *k {
                        "ObjectType" => "Object type",
                        other => other,
                    }),
                };
                (*k, bt(key, &[]), v)
            })
            .collect()
    };
    ui.label(RichText::new(bt("prop.edit.hint", &[])).small().color(t.text_dim));
    let mut out: Option<(String, String)> = None;
    for (k, label, v) in &attrs {
        let l = ui.label(RichText::new(label).small().color(t.text_dim));
        if let Some(nv) = field(app, ui, &format!("a|{k}"), v, l.id) {
            out = Some(("bim.attr".to_string(), format!("{k}={nv}")));
        }
    }
    // single values of the element's sets (ifc-model.js properties rows with `edit`)
    let sets: Vec<PropSet> = match app.bimh.ed.as_ref() {
        Some(d) => prop_refs(&d.ed, pid),
        None => match doc(app) {
            Ok(d) => prop_refs(&d.ed, pid),
            Err(_) => Vec::new(),
        },
    };
    for (set, rows) in &sets {
        let editable: Vec<&(String, Option<PropRef>)> = rows.iter().filter(|r| r.1.is_some()).collect();
        if editable.is_empty() {
            continue;
        }
        egui::CollapsingHeader::new(RichText::new(set).color(t.text_strong)).id_salt(("bim_set", set.as_str())).default_open(false).show(
            ui,
            |ui| {
                if set.ends_with(" (type)") {
                    ui.label(RichText::new(bt("prop.typenote", &[])).small().color(t.text_dim));
                }
                for (pname, r) in editable {
                    let Some(r) = r else { continue };
                    let cur = app
                        .bimh
                        .ed
                        .as_ref()
                        .and_then(|d| d.ed.api.line(r.prop_id).ok().flatten())
                        .map(|p| match p.a("NominalValue").val() {
                            cc_bim::wapi::P::Bool(true) => bt("prop.yes", &[]),
                            cc_bim::wapi::P::Bool(false) => bt("prop.no", &[]),
                            cc_bim::wapi::P::Null | cc_bim::wapi::P::Undef => String::new(),
                            v => v.string(),
                        })
                        .unwrap_or_default();
                    let l = ui.label(RichText::new(pname).small().color(t.text_dim));
                    if let Some(nv) = field(app, ui, &format!("p|{set}|{pname}"), &cur, l.id) {
                        out = Some(("bim.prop".into(), format!("{set}/{pname}={nv}")));
                    }
                }
            },
        );
    }
    if let Some((cmd, arg)) = out {
        app.run_ui(&cmd, &Params { name: Some(arg), ..Default::default() });
    }
}

/// A one-line field that hands back its text on Enter or when it loses focus with a change.
fn field(app: &mut UiApp, ui: &mut egui::Ui, key: &str, current: &str, label: egui::Id) -> Option<String> {
    let mut text = match &app.bimh.editing {
        Some((k, v)) if k == key => v.clone(),
        _ => current.to_string(),
    };
    let w = ui.available_width().max(80.0);
    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(w).id_salt(("bim_field", key))).labelled_by(label);
    if r.has_focus() || r.changed() {
        app.bimh.editing = Some((key.to_string(), text.clone()));
    }
    if r.lost_focus() {
        let done = app.bimh.editing.take().filter(|(k, _)| k == key).map(|(_, v)| v);
        return done.filter(|v| v != current);
    }
    None
}

/// The New building dialog (bimcraft.js askNewBuilding).
pub fn new_dialog(app: &mut UiApp, ctx: &egui::Context) {
    let Some(mut dlg) = app.bimh.new_dlg.clone() else { return };
    let mut go = false;
    let mut cancel = false;
    egui::Window::new(RichText::new(bt("new.title", &[])).strong()).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.label(bt("new.lede", &[]));
        let l = ui.label(bt("new.name", &[]));
        ui.add(egui::TextEdit::singleline(&mut dlg.name).id_salt("bim_new_name")).labelled_by(l.id);
        let l = ui.label(bt("new.levels", &[]));
        ui.add(egui::TextEdit::singleline(&mut dlg.levels).id_salt("bim_new_levels")).labelled_by(l.id);
        let l = ui.label(bt("new.height", &[]));
        ui.add(egui::TextEdit::singleline(&mut dlg.height).id_salt("bim_new_height")).labelled_by(l.id);
        ui.label(RichText::new(bt("new.height.hint", &[])).small());
        if !dlg.problem.is_empty() {
            ui.colored_label(egui::Color32::from_rgb(0xe0, 0x6c, 0x5c), &dlg.problem);
        }
        ui.horizontal(|ui| {
            go = ui.button(bt("new.create", &[])).clicked();
            cancel = ui.button(bt("dialog.cancel", &[])).clicked();
        });
    });
    if cancel {
        app.bimh.new_dlg = None;
        return;
    }
    if go {
        let arg = format!("{}|{}|{}", dlg.name, dlg.levels, dlg.height);
        match run(app, "bim.new", &Params { name: Some(arg), ..Default::default() }) {
            Ok(()) => app.bimh.new_dlg = None,
            Err(e) => {
                dlg.problem = e;
                app.bimh.new_dlg = Some(dlg);
            }
        }
        return;
    }
    app.bimh.new_dlg = Some(dlg);
}

// ------------------------------------------------------------------------------------------------- the app's IFC view

impl UiApp {
    /// The bytes of the IFC model shown.
    pub(crate) fn ifc3_bytes(&self) -> Option<Arc<Vec<u8>>> {
        self.ifc3.as_ref().map(|(b, _)| b.clone())
    }
    /// The open IFC file's name.
    pub(crate) fn ifc_name(&self) -> &str {
        &self.ifc_name
    }
    /// The product picked in the 3D view (or the plan).
    pub(crate) fn picked_product(&self) -> Option<f64> {
        let (_, products) = self.ifc3.as_ref()?;
        match self.sel3 {
            Some(i) => products.get(i).copied(),
            None => self.bimh.picked_off,
        }
    }
    /// Pick a product (every piece of it tinted, its properties shown); none when it has no piece.
    pub(crate) fn select_product(&mut self, id: f64) {
        let Some((bytes, products)) = self.ifc3.clone() else { return };
        let Some(i) = products.iter().position(|p| *p == id) else {
            // no piece in the 3D view (a room): picked on the plan, its properties shown
            self.select_product_none();
            if let Ok(f) = cc_io_ifc::model::IfcFile::open(&bytes)
                && f.ty(id).is_some()
            {
                self.sel_props = cc_io_ifc::props::properties(&f, id);
                self.bimh.picked_off = Some(id);
            }
            return;
        };
        self.bimh.picked_off = None;
        const SEL: [u8; 4] = [70, 130, 240, 255];
        if let Some((scene, _)) = self.scene3.as_mut() {
            scene.set_tints(&|k| (products.get(k).copied() == Some(id)).then_some(SEL));
        }
        self.sel3 = Some(i);
        self.sel_props = match cc_io_ifc::model::IfcFile::open(&bytes) {
            Ok(f) => cc_io_ifc::props::properties(&f, id),
            Err(_) => Vec::new(),
        };
    }
    /// Nothing picked.
    pub(crate) fn select_product_none(&mut self) {
        if let Some((scene, _)) = self.scene3.as_mut() {
            scene.set_tints(&|_| None);
        }
        self.sel3 = None;
        self.sel_props.clear();
        self.bimh.picked_off = None;
    }
    /// The IFC model shown again from new bytes (bimcraft.js `rebuild`): the camera, the style, what is hidden and
    /// the section cut kept; Find's index made again on its next use.
    pub(crate) fn show_ifc_bytes(&mut self, bytes: Arc<Vec<u8>>, keep_camera: bool) {
        match cc_io_ifc::view::view(&bytes, &cc_io_ifc::geo::bool_ops::ManifoldOps) {
            Ok(v) => {
                self.scene_seq = self.scene_seq.wrapping_add(1);
                let scene = crate::ifc3d::scene_of(&v, self.scene_seq, 1_500_000);
                // a new building's first walls: frame the 3D view once (it had nothing to look at)
                let mut view = match (keep_camera, self.scene3.take()) {
                    (true, Some((old, view))) if !old.instances.is_empty() => view,
                    (true, Some((_, mut view))) => {
                        view.fit_pending = true;
                        view
                    }
                    _ => {
                        let mut view = crate::view3d::View3::default();
                        self.style3(&mut view);
                        view
                    }
                };
                if view.fit_pending && !scene.instances.is_empty() {
                    view.cam.fit(scene.min, scene.max, Some([1.0, -1.15, 0.85]));
                    view.fit_pending = false;
                }
                self.scene3 = Some((scene, view));
                self.ifc3 = Some((bytes, v.pieces.iter().map(|p| p.product).collect()));
                self.ifc_groups = v.pieces.iter().map(|p| (p.storey.clone(), p.ty.clone())).collect();
                self.bim = crate::bimtools::BimTools::default();
                self.sel3 = None;
                self.sel_props.clear();
                self.apply_hidden3();
                self.apply_cut3();
            }
            Err(e) => self.error = Some(e.0),
        }
    }
}

/// A file was opened: no editor, Draw off (the clock stays).
pub fn opened(app: &mut UiApp) {
    let clock = app.bimh.clock;
    app.bimh = Host { clock, ..Host::default() };
}

/// The IFC view (the app's `scene3` branch with an IFC model): the editing bar (Draw, Save as IFC, Undo, Redo, New
/// building), the Model panel, Draw's side panel (levels, schedules), Properties (with the editable values and the
/// drawn element's own fields), and the plan beside (or instead of) the 3D view.
pub fn ifc_branch(app: &mut UiApp, ui: &mut egui::Ui) {
    let t = app.tokens;
    if app.sel3.is_some() {
        app.bimh.picked_off = None;
    }
    let ctx = ui.ctx().clone();
    // keys: Ctrl+S, Ctrl+Z, Ctrl+Y (not while typing in a field)
    if !ctx.egui_wants_keyboard_input() {
        let (s, z, y) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::S),
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                i.modifiers.command && (i.key_pressed(egui::Key::Y) || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
            )
        });
        if s && unsaved(app) {
            app.run_ui("bim.save", &Params::default());
        }
        if z && app.bimh.ed.is_some() {
            app.run_ui("bim.undo", &Params::default());
        }
        if y && app.bimh.ed.is_some() {
            app.run_ui("bim.redo", &Params::default());
        }
    }
    crate::bimplan::keys(app, &ctx);
    let frame = egui::Frame::new().fill(t.panel).inner_margin(4);
    egui::Panel::top("bim_edit_bar").frame(frame).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let on = app.bimh.draw.on;
            if ui.add(egui::Button::new(bt("model.draw", &[])).selected(on)).on_hover_text(bt("model.draw.title", &[])).clicked() {
                app.run_ui("bim.draw", &Params::default());
            }
            save_button(app, ui);
            if ui.button(bt("new.button", &[])).on_hover_text(bt("new.title", &[])).clicked() {
                app.bimh.new_dlg = Some(NewDlg::default());
            }
            if app.bimh.ed.is_some()
                && ui.button(bt("lib.place", &[])).on_hover_text("Pick a library item file (.item.json) to place on the plan").clicked()
            {
                app.run_ui("bim.place.pick", &Params::default());
            }
        });
        if app.bimh.draw.on {
            ui.separator();
            crate::bimplan::bar(app, ui);
            if app.bimh.draw.placing.is_some() {
                ui.separator();
                crate::bimplace::bar(app, ui);
            }
        }
    });
    if app.bimh.draw.on {
        egui::Panel::left("bim_side").default_size(240.0).size_range(180.0..=420.0).resizable(true).frame(frame).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                crate::bimplan::levels_panel(app, ui);
                crate::bimplan::schedules_panel(app, ui);
            });
        });
    } else {
        app.model_panel(ui);
    }
    app.props_panel(ui);
    let mut clicked: Option<Option<(f64, f64)>> = None;
    egui::CentralPanel::no_frame().show(ui, |ui| {
        let rect = ui.max_rect();
        let (plan_rect, rect3) = if app.bimh.draw.on {
            if app.bimh.draw.layout == "plan" {
                (Some(rect), None)
            } else {
                let mid = rect.center().x;
                (
                    Some(egui::Rect::from_min_max(rect.min, egui::pos2(mid - 1.0, rect.max.y))),
                    Some(egui::Rect::from_min_max(egui::pos2(mid + 1.0, rect.min.y), rect.max)),
                )
            }
        } else {
            (None, Some(rect))
        };
        if let Some(pr) = plan_rect {
            crate::bimplan::plan_ui(app, ui, pr);
        }
        if let (Some(r3), Some((scene, view))) = (rect3, app.scene3.as_mut()) {
            let resp = crate::view3d::show(ui, r3, scene, view);
            if resp.clicked_by(egui::PointerButton::Primary) {
                let ppp = f64::from(ui.ctx().pixels_per_point());
                clicked = Some(resp.interact_pointer_pos().map(|p| (f64::from(p.x - r3.min.x) * ppp, f64::from(p.y - r3.min.y) * ppp)));
            }
            if !app.bimh.draw.on && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                clicked = Some(None);
            }
        }
    });
    if let Some(at) = clicked {
        app.select3(at);
        ui.ctx().request_repaint();
    }
}

/// What Properties shows above the IFC data: the drawn element's own fields, then the editable values.
pub fn props_top(app: &mut UiApp, ui: &mut egui::Ui) {
    if app.bimh.draw.on {
        crate::bimplan::props_block(app, ui);
    }
    props_edit(app, ui);
    ui.separator();
}
