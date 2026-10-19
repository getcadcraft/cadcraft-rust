//! The drafting window's Quick select and Find and replace dialogs (cadcraft.js `quickSelectDialog` / `findDialog`),
//! over cc-engine's qselect.js / findtext.js ports. QSELECT and FIND open them (the engine asks the host); every
//! control is a `qs.*` / `find.*` command, so the CLI and assistants reach the same dialogs.

use cc_engine::cmd::qselect::{
    FindSpec, PropKind, QsVal, Spec, apply_matches, find_matches, find_scope, prop_def, props_for, quick_select, scope_pool, type_label,
    typed_value, types_present, values_of,
};
use egui::RichText;

use crate::{Params, UiApp};

/// Quick select's settings while the dialog is open.
#[derive(Clone, Debug)]
pub struct QsState {
    /// "all" | "sel"
    pub apply: String,
    pub typ: String,
    pub prop: String,
    pub op: String,
    /// typed text (text and number properties) or the picked value's index (choice and colour)
    pub value: String,
    pub exclude: bool,
    pub append: bool,
    /// the selection when the dialog opened
    pub current: Vec<u64>,
}

/// Find's settings while the dialog is open.
#[derive(Clone, Debug, Default)]
pub struct FindState {
    pub what: String,
    pub with: String,
    pub case: bool,
    pub whole: bool,
    pub wild: bool,
    /// text, attr, dim
    pub kinds: [bool; 3],
    /// the scope when the dialog opened (the selection; None = everything)
    pub scope: Option<Vec<u64>>,
    pub in_selection: bool,
}

#[derive(Default)]
pub struct CadDialogs {
    pub qs: Option<QsState>,
    pub find: Option<FindState>,
}

pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("qs.open", "Quick select", "", "{} → opens the Quick select dialog (QSELECT does the same)"),
    (
        "qs.set",
        "Quick select: set a field",
        "",
        "{name: \"apply=all|sel\" | \"type=<any|line|…>\" | \"prop=<layer|color|…>\" | \"op==|<>|>|<|*|all\" | \"value=<text, or the index of a listed value>\" | \"how=include|exclude\" | \"append=0|1\"}",
    ),
    ("qs.ok", "Quick select: select", "", "{} → selects what matches and closes the dialog"),
    ("qs.cancel", "Quick select: cancel", "", "{}"),
    ("find.open", "Find and replace", "", "{} → opens the Find and replace dialog (FIND does the same)"),
    (
        "find.set",
        "Find: set a field",
        "",
        "{name: \"what=<text>\" | \"with=<text>\" | \"case=0|1\" | \"whole=0|1\" | \"wild=0|1\" | \"text=0|1\" | \"attr=0|1\" | \"dim=0|1\"}",
    ),
    ("find.select", "Find: select what is found", "", "{} → selects the objects and closes the dialog"),
    ("find.replace", "Find: replace all", "", "{} → replaces every hit (one undo step) and closes the dialog"),
    ("find.close", "Find: close", "", "{}"),
];

fn t(k: &str) -> String {
    crate::edit::lang_text(k, &[])
}
fn tv(k: &str, vars: &[(&str, String)]) -> String {
    crate::edit::lang_text(k, vars)
}

pub fn open_qs(app: &mut UiApp) {
    let Some(ed) = app.editor.as_ref() else { return };
    let current: Vec<u64> = {
        let doc = ed.eng.doc();
        ed.hs.borrow().selection.iter().copied().filter(|id| doc.get(*id).is_some()).collect()
    };
    app.caddlg.qs = Some(QsState {
        apply: "all".into(),
        typ: "any".into(),
        prop: "layer".into(),
        op: "=".into(),
        value: String::new(),
        exclude: false,
        append: false,
        current,
    });
}

pub fn open_find(app: &mut UiApp) {
    let Some(ed) = app.editor.as_ref() else { return };
    let sel = ed.hs.borrow().selection.clone();
    let scope = find_scope(&ed.eng.doc(), &sel);
    app.caddlg.find = Some(FindState { kinds: [true; 3], in_selection: !sel.is_empty(), scope, ..FindState::default() });
}

/// The rule the dialog's fields make, and the pool (editable objects) it runs over.
fn qs_spec(app: &UiApp, q: &QsState) -> Option<(Spec, Vec<u64>)> {
    let ed = app.editor.as_ref()?;
    let doc = ed.eng.doc();
    let pool: Vec<&cc_cad::Entity> = doc.entities().filter(|e| doc.editable(e)).collect();
    let kind = prop_def(&q.prop).map_or(PropKind::Text, |p| p.1);
    let value = if q.op == "all" {
        QsVal::Null
    } else if matches!(kind, PropKind::Choice | PropKind::Color) {
        let vals = values_of(&pool, &q.typ, &q.prop);
        vals.get(q.value.parse::<usize>().unwrap_or(0)).cloned().unwrap_or(QsVal::Null)
    } else {
        let lunits = doc.meta.get("lunits").map_or(2.0, cc_cad::js::to_number);
        typed_value(kind, &q.value, lunits)
    };
    let spec = Spec {
        typ: q.typ.clone(),
        prop: q.prop.clone(),
        op: q.op.clone(),
        value,
        exclude: q.exclude,
        within: (q.apply == "sel").then(|| q.current.clone()),
        append: q.append,
    };
    let found = quick_select(&pool, &spec);
    Some((spec, found))
}

fn on(v: &str) -> Result<bool, String> {
    match v {
        "1" | "true" | "on" => Ok(true),
        "0" | "false" | "off" => Ok(false),
        _ => Err(format!("expected 0 or 1, got {v}")),
    }
}

pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    match id {
        "qs.open" => {
            app.editor.as_ref().ok_or("qs.open: no drawing is open for editing")?;
            open_qs(app);
        }
        "qs.set" => {
            let spec = params.name.clone().ok_or("qs.set: name = \"<field>=<value>\" is required")?;
            let (k, v) = spec.split_once('=').ok_or("qs.set: name = \"<field>=<value>\"")?;
            let q = app.caddlg.qs.as_mut().ok_or("qs.set: the Quick select dialog is not open")?;
            match k {
                "apply" if v == "all" || (v == "sel" && !q.current.is_empty()) => q.apply = v.into(),
                "type" => {
                    q.typ = v.into();
                    if !props_for(v).contains(&q.prop.as_str()) {
                        q.prop = "layer".into();
                    }
                    q.value = String::new();
                }
                "prop" if props_for(&q.typ).contains(&v) => {
                    q.prop = v.into();
                    q.value = String::new();
                }
                "op" if ["=", "<>", ">", "<", "*", "all"].contains(&v) => q.op = v.into(),
                "value" => q.value = v.into(),
                "how" if v == "include" || v == "exclude" => q.exclude = v == "exclude",
                "append" => q.append = on(v)?,
                _ => return Err(format!("qs.set: {k} = {v} is not an option")),
            }
        }
        "qs.ok" => {
            let q = app.caddlg.qs.clone().ok_or("qs.ok: the Quick select dialog is not open")?;
            let (spec, found) = qs_spec(app, &q).ok_or("qs.ok: no drawing is open for editing")?;
            app.caddlg.qs = None;
            if let Some(ed) = app.editor.as_mut() {
                let mut h = ed.hs.borrow_mut();
                let next = if spec.append {
                    let mut s = cc_engine::IdSet::from_vec(q.current.clone());
                    for id in &found {
                        s.add(*id);
                    }
                    s.0
                } else {
                    found.clone()
                };
                h.selection = next;
                h.push_log(&tv("cmd.qs.done", &[("n", found.len().to_string())]));
            }
        }
        "qs.cancel" => app.caddlg.qs = None,
        "find.open" => {
            app.editor.as_ref().ok_or("find.open: no drawing is open for editing")?;
            open_find(app);
        }
        "find.set" => {
            let spec = params.name.clone().ok_or("find.set: name = \"<field>=<value>\" is required")?;
            let (k, v) = spec.split_once('=').ok_or("find.set: name = \"<field>=<value>\"")?;
            let f = app.caddlg.find.as_mut().ok_or("find.set: the Find dialog is not open")?;
            match k {
                "what" => f.what = v.into(),
                "with" => f.with = v.into(),
                "case" => f.case = on(v)?,
                "whole" => f.whole = on(v)?,
                "wild" => f.wild = on(v)?,
                "text" => f.kinds[0] = on(v)?,
                "attr" => f.kinds[1] = on(v)?,
                "dim" => f.kinds[2] = on(v)?,
                _ => return Err(format!("find.set: {k} is not a field")),
            }
        }
        "find.select" | "find.replace" => {
            let f = app.caddlg.find.clone().ok_or("find: the Find dialog is not open")?;
            if f.what.is_empty() {
                return Err("find: type what to find first".into());
            }
            app.caddlg.find = None;
            let replace = id == "find.replace";
            let spec = find_spec(&f, replace);
            let ed = app.editor.as_mut().ok_or("find: no drawing is open for editing")?;
            let ms = {
                let doc = ed.eng.doc();
                find_matches(scope_pool(&doc, f.scope.as_deref()), &spec)
            };
            let line = if ms.is_empty() {
                tv("cmd.find.none", &[("find", spec.find.clone())])
            } else {
                let total: usize = ms.iter().map(|m| m.count).sum();
                if replace {
                    let n = ed.eng.doc_mut().change("FIND", |d| apply_matches(d, &ms)).map_err(|e| e.to_string())?;
                    tv("cmd.find.replaced", &[("n", total.to_string()), ("m", n.to_string())])
                } else {
                    let ids = cc_engine::IdSet::from_vec(ms.iter().map(|m| m.id).collect());
                    let m = ids.len();
                    ed.hs.borrow_mut().selection = ids.0;
                    tv("cmd.find.found", &[("n", total.to_string()), ("m", m.to_string())])
                }
            };
            ed.hs.borrow_mut().push_log(&line);
            app.sync_editor();
        }
        "find.close" => app.caddlg.find = None,
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

fn find_spec(f: &FindState, replace: bool) -> FindSpec {
    FindSpec {
        find: f.what.clone(),
        replace: replace.then(|| f.with.clone()),
        match_case: f.case,
        whole: f.whole,
        wildcard: f.wild,
        kinds: f.kinds,
    }
}

fn color_label(v: &QsVal) -> String {
    match v {
        QsVal::Null => t("props.color.bylayer"),
        QsVal::Num(n) if *n == 0.0 => t("props.color.byblock"),
        other => other.js_string(),
    }
}

/// Draw whichever dialog is open.
pub fn window(app: &mut UiApp, ctx: &egui::Context) {
    if app.editor.is_none() {
        app.caddlg = CadDialogs::default();
        return;
    }
    if app.caddlg.qs.is_some() {
        qs_window(app, ctx);
    }
    if app.caddlg.find.is_some() {
        find_window(app, ctx);
    }
}

fn combo(ui: &mut egui::Ui, id: &str, cur: &str, items: &[(String, String)], out: &mut Option<String>) {
    let shown = items.iter().find(|(k, _)| k == cur).map(|(_, l)| l.clone()).unwrap_or_default();
    egui::ComboBox::new(id, "").selected_text(shown).width(220.0).show_ui(ui, |ui| {
        for (k, l) in items {
            if ui.selectable_label(k == cur, l).clicked() {
                *out = Some(k.clone());
            }
        }
    });
}

fn qs_window(app: &mut UiApp, ctx: &egui::Context) {
    let Some(q) = app.caddlg.qs.clone() else { return };
    let tok = app.tokens;
    let Some(ed) = app.editor.as_ref() else { return };
    let lang = crate::edit::lang_en_pub();
    let (types, vals, total) = {
        let doc = ed.eng.doc();
        let pool: Vec<&cc_cad::Entity> = doc.entities().filter(|e| doc.editable(e)).collect();
        let types: Vec<(String, String)> = types_present(&pool)
            .into_iter()
            .map(|ty| {
                let n = pool.iter().filter(|e| e.typ() == ty).count();
                let label = if ty == "any" { type_label(&lang, ty) } else { format!("{} ({n})", type_label(&lang, ty)) };
                (ty.to_string(), label)
            })
            .collect();
        (types, values_of(&pool, &q.typ, &q.prop), pool.len())
    };
    let found = qs_spec(app, &q).map_or(0, |(_, f)| f.len());
    let kind = prop_def(&q.prop).map_or(PropKind::Text, |p| p.1);
    let mut set: Vec<String> = Vec::new();
    let mut ok = false;
    let mut cancel = false;
    let mut open = true;
    egui::Window::new(t("dlg.qs.title"))
        .open(&mut open)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .collapsible(false)
        .show(ctx, |ui| {
            egui::Grid::new("qs_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(t("dlg.qs.apply"));
                let mut applies = vec![("all".to_string(), t("dlg.qs.apply.all"))];
                if !q.current.is_empty() {
                    applies.push(("sel".into(), t("dlg.qs.apply.sel")));
                }
                let mut pick = None;
                combo(ui, "qs_apply", &q.apply, &applies, &mut pick);
                if let Some(v) = pick {
                    set.push(format!("apply={v}"));
                }
                ui.end_row();
                ui.label(t("dlg.qs.type"));
                let mut pick = None;
                combo(ui, "qs_type", &q.typ, &types, &mut pick);
                if let Some(v) = pick {
                    set.push(format!("type={v}"));
                }
                ui.end_row();
                ui.label(t("dlg.qs.prop"));
                let props: Vec<(String, String)> =
                    props_for(&q.typ).iter().map(|k| ((*k).to_string(), t(&format!("qs.prop.{k}")))).collect();
                let mut pick = None;
                combo(ui, "qs_prop", &q.prop, &props, &mut pick);
                if let Some(v) = pick {
                    set.push(format!("prop={v}"));
                }
                ui.end_row();
                ui.label(t("dlg.qs.op"));
                let ops: Vec<(String, String)> = [
                    ("=", "qs.op.eq"),
                    ("<>", "qs.op.ne"),
                    (">", "qs.op.gt"),
                    ("<", "qs.op.lt"),
                    ("*", "qs.op.wild"),
                    ("all", "qs.op.all"),
                ]
                .iter()
                .map(|(k, l)| ((*k).to_string(), t(l)))
                .collect();
                let mut pick = None;
                combo(ui, "qs_op", &q.op, &ops, &mut pick);
                if let Some(v) = pick {
                    set.push(format!("op={v}"));
                }
                ui.end_row();
                let vl = ui.label(t("dlg.qs.value"));
                if q.op == "all" {
                    ui.label("");
                } else if matches!(kind, PropKind::Choice | PropKind::Color) {
                    let items: Vec<(String, String)> = vals
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (i.to_string(), if kind == PropKind::Color { color_label(v) } else { v.js_string() }))
                        .collect();
                    let mut pick = None;
                    let cur = if q.value.is_empty() { "0".to_string() } else { q.value.clone() };
                    combo(ui, "qs_value", &cur, &items, &mut pick);
                    if let Some(v) = pick {
                        set.push(format!("value={v}"));
                    }
                } else {
                    let mut buf = q.value.clone();
                    let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(220.0)).labelled_by(vl.id);
                    if r.changed() {
                        set.push(format!("value={buf}"));
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                }
                ui.end_row();
                ui.label(t("dlg.qs.how"));
                let hows = vec![("include".to_string(), t("dlg.qs.include")), ("exclude".to_string(), t("dlg.qs.exclude"))];
                let mut pick = None;
                combo(ui, "qs_how", if q.exclude { "exclude" } else { "include" }, &hows, &mut pick);
                if let Some(v) = pick {
                    set.push(format!("how={v}"));
                }
                ui.end_row();
            });
            let mut append = q.append;
            if ui.checkbox(&mut append, t("dlg.qs.append")).changed() {
                set.push(format!("append={}", u8::from(append)));
            }
            ui.separator();
            ui.horizontal(|ui| {
                let total = if q.apply == "sel" { q.current.len() } else { total };
                ui.label(RichText::new(tv("dlg.qs.count", &[("n", found.to_string()), ("total", total.to_string())])).color(tok.text_dim));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new(t("dlg.qs.ok")).strong()).clicked() {
                        ok = true;
                    }
                    if ui.button(t("dlg.qs.cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
        });
    for s in set {
        app.run_ui_pub("qs.set", &Params { name: Some(s), ..Default::default() });
    }
    if ok {
        app.run_ui_pub("qs.ok", &Params::default());
    } else if cancel || !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.caddlg.qs = None;
    }
}

fn find_window(app: &mut UiApp, ctx: &egui::Context) {
    let Some(f) = app.caddlg.find.clone() else { return };
    let tok = app.tokens;
    let Some(ed) = app.editor.as_ref() else { return };
    let ms = if f.what.is_empty() {
        Vec::new()
    } else {
        let doc = ed.eng.doc();
        find_matches(scope_pool(&doc, f.scope.as_deref()), &find_spec(&f, !f.with.is_empty()))
    };
    let mut set: Vec<String> = Vec::new();
    let mut act: Option<&str> = None;
    let mut open = true;
    egui::Window::new(t("dlg.find.title"))
        .open(&mut open)
        .resizable(true)
        .default_size([420.0, 360.0])
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .collapsible(false)
        .show(ctx, |ui| {
            egui::Grid::new("find_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                let l = ui.label(t("dlg.find.what"));
                let mut what = f.what.clone();
                let r = ui.add(egui::TextEdit::singleline(&mut what).desired_width(260.0)).labelled_by(l.id);
                if r.changed() {
                    set.push(format!("what={what}"));
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !what.is_empty() {
                    act = Some("find.select");
                }
                ui.end_row();
                let l = ui.label(t("dlg.find.with"));
                let mut with = f.with.clone();
                if ui.add(egui::TextEdit::singleline(&mut with).desired_width(260.0)).labelled_by(l.id).changed() {
                    set.push(format!("with={with}"));
                }
                ui.end_row();
            });
            ui.horizontal_wrapped(|ui| {
                for (k, v, label) in
                    [("case", f.case, "dlg.find.case"), ("whole", f.whole, "dlg.find.whole"), ("wild", f.wild, "dlg.find.wild")]
                {
                    let mut b = v;
                    if ui.checkbox(&mut b, t(label)).changed() {
                        set.push(format!("{k}={}", u8::from(b)));
                    }
                }
            });
            ui.label(RichText::new(t("dlg.find.in")).strong().color(tok.text_strong));
            ui.horizontal_wrapped(|ui| {
                for (i, (k, label)) in
                    [("text", "dlg.find.in.text"), ("attr", "dlg.find.in.attr"), ("dim", "dlg.find.in.dim")].iter().enumerate()
                {
                    let mut b = f.kinds.get(i).copied().unwrap_or(true);
                    if ui.checkbox(&mut b, t(label)).changed() {
                        set.push(format!("{k}={}", u8::from(b)));
                    }
                }
            });
            if f.in_selection {
                ui.label(RichText::new(t("dlg.find.scope.sel")).color(tok.text_dim));
            }
            ui.separator();
            egui::ScrollArea::vertical().max_height(160.0).auto_shrink([false, true]).show(ui, |ui| {
                for m in ms.iter().take(200) {
                    let kind = if m.kind == "attr" {
                        tv("dlg.find.kind.attr", &[("tag", m.tag.clone().unwrap_or_default())])
                    } else {
                        t(&format!("dlg.find.kind.{}", m.kind))
                    };
                    let one = |s: &str| s.replace(['\n', '\r'], " ").chars().take(80).collect::<String>();
                    let mut line = format!("{kind}  {}", one(&m.before));
                    if let Some(a) = &m.after {
                        line.push_str(&format!(" → {}", one(a)));
                    }
                    ui.label(RichText::new(line).small().color(tok.text));
                }
            });
            ui.horizontal(|ui| {
                if !f.what.is_empty() {
                    let n: usize = ms.iter().map(|m| m.count).sum();
                    ui.label(RichText::new(tv("dlg.find.results", &[("n", n.to_string())])).color(tok.text_dim));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(!f.what.is_empty(), egui::Button::new(RichText::new(t("dlg.find.replace")).strong())).clicked() {
                        act = Some("find.replace");
                    }
                    if ui.add_enabled(!f.what.is_empty(), egui::Button::new(t("dlg.find.select"))).clicked() {
                        act = Some("find.select");
                    }
                    if ui.button(t("dlg.find.close")).clicked() {
                        act = Some("find.close");
                    }
                });
            });
        });
    for s in set {
        app.run_ui_pub("find.set", &Params { name: Some(s), ..Default::default() });
    }
    if let Some(a) = act {
        app.run_ui_pub(a, &Params::default());
    } else if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.caddlg.find = None;
    }
}
