//! The Parameters panel and the constraints on the drawing (params-panel.js, gview.js):
//!
//! - the panel (PARAMETERS): one table of every parameter — dimensional constraints (d1, rad1 …) and user parameters —
//!   with name, expression and value. Typing in Expression links it (a number or "width / 2"); typing a number in
//!   Value of a driven row solves the parameter behind it (two-way; several candidates → the panel asks which one).
//!   Each edit is one undo step (cc-gcons params-doc). "Add" makes a user parameter; Delete on a dimensional row removes
//!   the constraint.
//! - the overlay (model space): constraint bars (a row of glyph boxes beside each constrained line, segment, circle or
//!   point; coincident ones a small square on the point) and the dimensional constraints, drawn as dimensions in their
//!   own colour at a fixed size on screen, "d1=40" or "fx: d1=width/2". A click on a glyph or a label selects that
//!   constraint (Delete removes it); a double click on a label edits its value.
//!
//! Every control is a `par.*` command, so the CLI and assistants reach the same edits.

use std::cell::RefCell;

use cc_gcons::con::Con;
use cc_gcons::gcons::{GEOMETRIC, bars_for, cons, describe_constraint, dim_entity_for, glyph, is_dim};
use cc_gcons::gsolve::{ref_circle, ref_kind, ref_line, ref_point};
use cc_gcons::paramsdoc::{self, Res};
use egui::{Color32, RichText};

use crate::{Params, UiApp};

/// A glyph or label on screen (logical px), for picking.
#[derive(Clone, Debug)]
pub struct Hit {
    pub rect: egui::Rect,
    pub cid: String,
    pub dim: bool,
}

/// What a value typed into a driven row is waiting for: the parameter to change.
#[derive(Clone, Debug)]
pub struct Choice {
    /// "c:<id>" (a dimensional constraint's value) or the parameter's name
    pub target: String,
    pub value: f64,
    pub choices: Vec<String>,
    pub text: String,
}

/// The panel's state.
#[derive(Debug, Default)]
pub struct ParPanel {
    pub open: bool,
    /// the selected constraint (`state.selected`)
    pub selected: Option<String>,
    pub hits: RefCell<Vec<Hit>>,
    /// the last result line
    pub message: String,
    pub new_name: String,
    pub new_expr: String,
    /// row edits in progress: (name, field) → text
    pub bufs: Vec<((String, &'static str), String)>,
    pub choice: Option<Choice>,
    /// the label edit dialog: (constraint id, text, error)
    pub dc_edit: Option<(String, String, String)>,
    /// where the label edit dialog opens: under the label that was double-clicked (screen)
    pub dc_at: Option<egui::Pos2>,
}

/// UI commands of the panel.
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("par.open", "Parameters", "", "{} → opens the Parameters panel (PARAMETERS does the same)"),
    ("par.close", "Parameters: close", "", "{}"),
    ("par.set", "Parameters: expression", "", "{name: \"<parameter>=<expression>\"} → a number or an expression of other parameters"),
    (
        "par.value",
        "Parameters: value",
        "",
        "{name: \"<parameter>=<number>[|<driving parameter>]\"} → a driven row: solves the parameter behind it",
    ),
    ("par.rename", "Parameters: rename", "", "{name: \"<from>=<to>\"}"),
    ("par.delete", "Parameters: delete", "", "{name: parameter} → a dimensional one removes its constraint; a used one is refused"),
    ("par.delete.inline", "Parameters: write the value in and delete", "", "{name: parameter}"),
    ("par.add", "Parameters: add", "", "{name: \"<name>=<expression>\"} (empty expression = 0)"),
    ("par.pick", "Constraints: select", "", "{name: constraint id (k3), or empty to clear}"),
    ("par.remove", "Constraints: remove the selected one", "", "{} (the Delete key)"),
    ("par.label", "Constraints: open the value dialog", "", "{name: constraint id} (a double click on its label)"),
    (
        "par.edit",
        "Constraints: edit a dimensional one's value",
        "",
        "{name: \"<constraint id>=<text>\"} → \"40\", \"width / 2\" or \"d2=width/2\"",
    ),
];

fn t(k: &str) -> String {
    crate::edit::lang_text(k, &[])
}
fn tv(k: &str, vars: &[(&str, String)]) -> String {
    crate::edit::lang_text(k, vars)
}

/// `run(fn, done)`: an edit's result → the panel's line and the command history.
fn finish(app: &mut UiApp, r: &Res, done: Option<String>) {
    let Some(ed) = app.editor.as_ref() else { return };
    ed.eng.flush_notes();
    let mut h = ed.hs.borrow_mut();
    if r.ok {
        app.par.message = r.message.clone().or_else(|| done.clone()).unwrap_or_default();
        if let Some(d) = done {
            h.push_log(&format!("  {d}{}", r.message.as_ref().map(|m| format!(" {m}")).unwrap_or_default()));
        }
    } else {
        app.par.message = r.error.clone();
        h.push_log(&format!("  {}", r.error));
    }
}

fn split_eq(s: &str) -> Option<(String, String)> {
    let (a, b) = s.split_once('=')?;
    Some((a.trim().to_string(), b.trim().to_string()))
}

pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    let name = params.name.clone().unwrap_or_default();
    match id {
        "par.open" => {
            app.editor.as_ref().ok_or("par.open: no drawing is open for editing")?;
            app.par.open = true;
        }
        "par.close" => app.par.open = false,
        "par.label" => open_edit(app, &name),
        "par.pick" => {
            app.par.selected = if name.is_empty() { None } else { Some(name) };
            if let (Some(cid), Some(ed)) = (app.par.selected.clone(), app.editor.as_ref()) {
                let tr = ed.eng.lang().cat();
                let d = ed.eng.doc();
                let c = cons(&d).iter().find(|k| k.id == cid).cloned().ok_or(format!("par.pick: no constraint {cid}"))?;
                let line = tv(
                    if is_dim(&c) { "params.panel.picked.dim" } else { "params.panel.picked" },
                    &[("what", describe_constraint(&d, &tr, &c))],
                );
                drop(d);
                ed.hs.borrow_mut().push_log(&line);
            }
        }
        _ => {
            let ed = app.editor.as_ref().ok_or(format!("{id}: no drawing is open for editing"))?;
            if ed.eng.busy() {
                return Err(t("app.finish_cancel_current_command"));
            }
            let tr = ed.eng.lang().cat();
            let (r, done) = {
                let mut doc = ed.eng.doc_mut();
                let d = &mut *doc;
                match id {
                    "par.set" => {
                        let (n, ex) = split_eq(&name).ok_or("par.set: name = \"<parameter>=<expression>\"")?;
                        let con = cons(d).iter().find(|k| is_dim(k) && k.name() == Some(n.as_str())).cloned();
                        let r = match con {
                            Some(c) => paramsdoc::set_dim_input(d, &tr, &c.id, &ex),
                            None => paramsdoc::set_parameter(d, &tr, &n, &ex, None),
                        }
                        .map_err(|e| e.to_string())?;
                        (r, Some(format!("{n} = {ex}.")))
                    }
                    "par.value" => {
                        let (n, rest) = split_eq(&name).ok_or("par.value: name = \"<parameter>=<number>\"")?;
                        let (num, prefer) = match rest.split_once('|') {
                            Some((a, b)) => (a.trim().to_string(), Some(b.trim().to_string())),
                            None => (rest, None),
                        };
                        let Some(v) = cc_part::expr::literal(&num) else {
                            app.par.message = t("params.panel.type_number");
                            return Ok(());
                        };
                        let con = cons(d).iter().find(|k| is_dim(k) && k.name() == Some(n.as_str())).cloned();
                        let r = match &con {
                            Some(c) => paramsdoc::drive_constraint(d, &tr, &c.id, v, prefer.as_deref()),
                            None => paramsdoc::drive_parameter(d, &tr, &n, v, prefer.as_deref()),
                        }
                        .map_err(|e| e.to_string())?;
                        if !r.ok && r.choices.len() > 1 {
                            app.par.choice = Some(Choice { target: n, value: v, choices: r.choices.clone(), text: r.error.clone() });
                            app.par.message = r.error.clone();
                            return Ok(());
                        }
                        (r, Some(tv("params.panel.set_to", &[("name", n), ("value", cc_cad::js::num_to_string(v))])))
                    }
                    "par.rename" => {
                        let (from, to) = split_eq(&name).ok_or("par.rename: name = \"<from>=<to>\"")?;
                        let cur = cc_gcons::gcons::params_of(d).var(&from).cloned().ok_or(format!("par.rename: no parameter {from}"))?;
                        let r = paramsdoc::set_parameter(d, &tr, &to, &cur.expr, Some(&from)).map_err(|e| e.to_string())?;
                        (r, Some(tv("params.is_now", &[("name", from), ("value", to)])))
                    }
                    "par.delete" | "par.delete.inline" => {
                        let con = cons(d).iter().find(|k| is_dim(k) && k.name() == Some(name.as_str())).cloned();
                        if let Some(c) = con {
                            let n = paramsdoc::remove_constraints(d, std::slice::from_ref(&c.id), "DELCONSTRAINT")
                                .map_err(|e| e.to_string())?;
                            (Res { ok: n > 0, ..Res::default() }, Some(format!("{name} deleted.")))
                        } else {
                            let r = paramsdoc::remove_parameter(d, &tr, &name, id == "par.delete.inline").map_err(|e| e.to_string())?;
                            if !r.ok && r.uses {
                                let msg = format!("{} {}", r.error, t("params.panel.inline_hint"));
                                app.par.message = msg.clone();
                                return Err(msg);
                            }
                            (r, Some(format!("{name} deleted.")))
                        }
                    }
                    "par.add" => {
                        let (n, ex) = split_eq(&name).unwrap_or((name.trim().to_string(), String::new()));
                        if n.is_empty() {
                            return Err("par.add: name = \"<name>=<expression>\"".into());
                        }
                        let ex = if ex.is_empty() { "0".to_string() } else { ex };
                        let r = paramsdoc::set_parameter(d, &tr, &n, &ex, None).map_err(|e| e.to_string())?;
                        (r, Some(format!("{n} = {ex}.")))
                    }
                    "par.remove" => {
                        let Some(cid) = app.par.selected.take() else { return Ok(()) };
                        let Some(c) = cons(d).iter().find(|k| k.id == cid).cloned() else { return Ok(()) };
                        let what = describe_constraint(d, &tr, &c);
                        paramsdoc::remove_constraints(d, std::slice::from_ref(&c.id), "DELCONSTRAINT").map_err(|e| e.to_string())?;
                        drop(doc);
                        ed.eng.flush_notes();
                        ed.hs.borrow_mut().push_log(&format!("Removed {what}."));
                        return Ok(());
                    }
                    "par.edit" => {
                        let (cid, text) = split_eq(&name).ok_or("par.edit: name = \"<constraint id>=<text>\"")?;
                        let c = cons(d)
                            .iter()
                            .find(|k| k.id == cid && is_dim(k))
                            .cloned()
                            .ok_or(format!("par.edit: no dimensional constraint {cid}"))?;
                        let cname = c.name().unwrap_or_default().to_string();
                        let v = cc_gcons::gcons::params_of(d).var(&cname).cloned();
                        let body = strip_name(&text);
                        let n = cc_part::expr::literal(body);
                        let same_name =
                            !has_name(&text) || text.starts_with(&format!("{cname}=")) || text.starts_with(&format!("{cname} ="));
                        if let (Some(n), true, Some(v)) = (n, same_name, v.as_ref())
                            && cc_part::expr::literal(&v.expr).is_none()
                        {
                            let r = paramsdoc::drive_constraint(d, &tr, &cid, n, None).map_err(|e| e.to_string())?;
                            app.par.dc_edit = None;
                            if !r.ok && r.choices.len() > 1 {
                                app.par.choice =
                                    Some(Choice { target: cname, value: n, choices: r.choices.clone(), text: r.error.clone() });
                                app.par.message = r.error.clone();
                                return Ok(());
                            }
                            (r, Some(tv("params.panel.set_to", &[("name", cname), ("value", cc_cad::js::num_to_string(n))])))
                        } else {
                            let r = paramsdoc::set_dim_input(d, &tr, &cid, &text).map_err(|e| e.to_string())?;
                            if !r.ok {
                                if let Some(dc) = app.par.dc_edit.as_mut() {
                                    dc.2 = r.error.clone();
                                }
                                return Err(r.error);
                            }
                            app.par.dc_edit = None;
                            drop(doc);
                            ed.eng.flush_notes();
                            ed.hs
                                .borrow_mut()
                                .push_log(&format!("  {cname}: {text}{}", r.message.as_ref().map(|m| format!(" {m}")).unwrap_or_default()));
                            return Ok(());
                        }
                    }
                    other => return Err(format!("unknown command {other}")),
                }
            };
            finish(app, &r, done);
            if !r.ok {
                return Err(r.error);
            }
        }
    }
    Ok(())
}

/// "name = …" at the start of a typed text
fn has_name(text: &str) -> bool {
    let t = text.trim_start();
    let first = t.chars().next();
    if !first.is_some_and(|c| c.is_ascii_alphabetic() || c == '_') {
        return false;
    }
    let rest = t.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
    rest.trim_start().starts_with('=')
}
/// `text.replace(/^[A-Za-z_][A-Za-z0-9_]*\s*=(?!=)\s*/, '')`
fn strip_name(text: &str) -> &str {
    let first = text.chars().next();
    if !first.is_some_and(|c| c.is_ascii_alphabetic() || c == '_') {
        return text;
    }
    let rest = text.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
    let r2 = rest.trim_start();
    match r2.strip_prefix('=') {
        Some(after) if !after.starts_with('=') => after.trim_start(),
        _ => text,
    }
}

/// The panel window.
pub fn window(app: &mut UiApp, ctx: &egui::Context) {
    dc_edit_window(app, ctx);
    if !app.par.open {
        return;
    }
    let Some(ed) = app.editor.as_ref() else {
        app.par.open = false;
        return;
    };
    let tok = app.tokens;
    let (rows, status) = {
        let tr = ed.eng.lang().cat();
        let d = ed.eng.doc();
        paramsdoc::describe(&d, &tr)
    };
    let mut act: Vec<(&'static str, String)> = Vec::new();
    let mut open = true;
    let buf = |p: &mut ParPanel, name: &str, field: &'static str, init: &str| -> String {
        p.bufs.iter().find(|(k, _)| k.0 == name && k.1 == field).map_or_else(|| init.to_string(), |(_, v)| v.clone())
    };
    egui::Window::new(t("ui.parameters")).open(&mut open).resizable(true).default_width(520.0).show(ctx, |ui| {
        ui.label(RichText::new(&status).color(tok.text_dim));
        if rows.is_empty() {
            ui.label(RichText::new(t("params.panel.empty")).color(tok.text_dim));
        } else {
            egui::Grid::new("par_rows").num_columns(5).spacing([6.0, 3.0]).striped(true).show(ui, |ui| {
                ui.label(RichText::new(t("params.panel.name")).strong());
                ui.label(RichText::new(t("params.panel.expression")).strong());
                ui.label(RichText::new(t("params.panel.value")).strong());
                ui.label(RichText::new(t("params.panel.type")).strong());
                ui.label("");
                ui.end_row();
                for r in &rows {
                    let selected = r.constraint.as_ref().is_some_and(|c| Some(&c.id) == app.par.selected.as_ref());
                    // name
                    let mut nm = buf(&mut app.par, &r.name, "name", &r.name);
                    let re = ui.add_sized([84.0, 20.0], egui::TextEdit::singleline(&mut nm).font(egui::TextStyle::Monospace));
                    if re.changed() {
                        set_buf(&mut app.par, &r.name, "name", nm.clone());
                    }
                    if re.lost_focus() {
                        let to = nm.trim().to_string();
                        clear_buf(&mut app.par, &r.name, "name");
                        if !to.is_empty() && to != r.name {
                            act.push(("par.rename", format!("{}={to}", r.name)));
                        }
                    }
                    // expression
                    let mut ex = buf(&mut app.par, &r.name, "expr", &r.expr);
                    let xe = ui.add_sized([150.0, 20.0], egui::TextEdit::singleline(&mut ex).font(egui::TextStyle::Monospace));
                    let xe = if let Some(e) = &r.error { xe.on_hover_text(e) } else { xe };
                    if xe.changed() {
                        set_buf(&mut app.par, &r.name, "expr", ex.clone());
                    }
                    if xe.lost_focus() {
                        let v = ex.trim().to_string();
                        clear_buf(&mut app.par, &r.name, "expr");
                        if !v.is_empty() && v != r.expr {
                            act.push(("par.set", format!("{}={v}", r.name)));
                        }
                    }
                    if xe.gained_focus()
                        && let Some(c) = &r.constraint
                    {
                        act.push(("par.pick", c.id.clone()));
                    }
                    // value: driven rows take a number
                    let shown = r.value.map_or_else(String::new, |v| cc_cad::js::num_to_string(cc_cad::js::round(v * 1e6) / 1e6));
                    if r.driving {
                        let deg = if r.constraint.as_ref().is_some_and(|c| c.typ == "angle") { "°" } else { "" };
                        ui.label(RichText::new(format!("{shown}{deg}")).monospace());
                    } else {
                        let mut vv = buf(&mut app.par, &r.name, "value", &shown);
                        let ve = ui
                            .add_sized([76.0, 20.0], egui::TextEdit::singleline(&mut vv).font(egui::TextStyle::Monospace))
                            .on_hover_text(tv("params.panel.driven_by", &[("expr", r.expr.clone())]));
                        if ve.changed() {
                            set_buf(&mut app.par, &r.name, "value", vv.clone());
                        }
                        if ve.lost_focus() {
                            clear_buf(&mut app.par, &r.name, "value");
                            if vv.trim() != shown {
                                act.push(("par.value", format!("{}={}", r.name, vv.trim())));
                            }
                        }
                    }
                    let kind = if r.dim { r.type_label.clone() } else { "User".to_string() };
                    let k = ui.label(RichText::new(kind).color(if selected { tok.accent } else { tok.text_dim }));
                    if let Some(c) = &r.constraint {
                        let what = {
                            let tr = ed.eng.lang().cat();
                            let d = ed.eng.doc();
                            describe_constraint(&d, &tr, c)
                        };
                        k.on_hover_text(what);
                    } else if !r.note.is_empty() {
                        k.on_hover_text(&r.note);
                    }
                    let tip = if r.constraint.is_some() { t("params.panel.delete_constraint") } else { t("params.panel.delete_parameter") };
                    if ui.small_button("✕").on_hover_text(tip).clicked() {
                        act.push(("par.delete", r.name.clone()));
                    }
                    ui.end_row();
                }
            });
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut app.par.new_name).hint_text(t("params.panel.name")).desired_width(90.0));
            ui.add(egui::TextEdit::singleline(&mut app.par.new_expr).hint_text(t("params.panel.value_expression")).desired_width(160.0));
            if ui.button(t("params.panel.add")).clicked() && !app.par.new_name.trim().is_empty() {
                act.push(("par.add", format!("{}={}", app.par.new_name.trim(), app.par.new_expr.trim())));
            }
        });
        if let Some(ch) = app.par.choice.clone() {
            ui.label(RichText::new(t("params.panel.which_parameter_changes")).strong());
            ui.label(&ch.text);
            ui.horizontal(|ui| {
                for c in ch.choices.iter().take(6) {
                    if ui.button(c).clicked() {
                        act.push(("par.value", format!("{}={}|{c}", ch.target, cc_cad::js::num_to_string(ch.value))));
                    }
                }
                if ui.button(t("opt.cancel")).clicked() {
                    act.push(("par.cancel", String::new()));
                }
            });
        }
        if !app.par.message.is_empty() {
            ui.label(RichText::new(&app.par.message).color(tok.text_dim));
        }
    });
    if !open {
        app.par.open = false;
    }
    for (id, name) in act {
        if id == "par.cancel" {
            app.par.choice = None;
            app.par.message = t("params.panel.nothing_changed");
            continue;
        }
        if id == "par.value" {
            app.par.choice = None;
        }
        if id == "par.add" {
            app.par.new_name.clear();
            app.par.new_expr.clear();
        }
        app.run_ui_pub(id, &Params { name: Some(name), ..Default::default() });
    }
}

fn set_buf(p: &mut ParPanel, name: &str, field: &'static str, v: String) {
    match p.bufs.iter_mut().find(|(k, _)| k.0 == name && k.1 == field) {
        Some(slot) => slot.1 = v,
        None => p.bufs.push(((name.to_string(), field), v)),
    }
}
fn clear_buf(p: &mut ParPanel, name: &str, field: &'static str) {
    p.bufs.retain(|(k, _)| !(k.0 == name && k.1 == field));
}

/// The label edit dialog (a double click on a dimensional constraint's label).
fn dc_edit_window(app: &mut UiApp, ctx: &egui::Context) {
    let Some((cid, mut text, err)) = app.par.dc_edit.clone() else { return };
    let Some(ed) = app.editor.as_ref() else { return };
    let (title, what) = {
        let tr = ed.eng.lang().cat();
        let d = ed.eng.doc();
        match cons(&d).iter().find(|k| k.id == cid).cloned() {
            Some(c) => {
                (tv("params.panel.dc_title", &[("name", c.name().unwrap_or_default().to_string())]), describe_constraint(&d, &tr, &c))
            }
            None => {
                drop(d);
                app.par.dc_edit = None;
                return;
            }
        }
    };
    let mut apply = false;
    let mut cancel = false;
    // under the label that was double-clicked (kept on screen), else the middle of the window
    let screen = ctx.content_rect();
    let (pos, pivot) = match app.par.dc_at {
        Some(p) => (
            egui::pos2(p.x.clamp(screen.min.x + 160.0, screen.max.x - 160.0), (p.y + 8.0).min(screen.max.y - 140.0)),
            egui::Align2::CENTER_TOP,
        ),
        None => (screen.center(), egui::Align2::CENTER_CENTER),
    };
    egui::Window::new(title)
        .id(egui::Id::unique("dc_edit_window").with(&cid))
        .collapsible(false)
        .resizable(false)
        .pivot(pivot)
        .default_pos(pos)
        .show(ctx, |ui| {
            ui.label(t("params.panel.value_expression_number_driven"));
            let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(240.0).font(egui::TextStyle::Monospace));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                apply = true;
            }
            if !err.is_empty() {
                ui.label(RichText::new(&err).color(Color32::from_rgb(0xe0, 0x60, 0x50)));
            }
            ui.label(RichText::new(format!("{what}.")).small());
            ui.horizontal(|ui| {
                if ui.button(t("dlg.qs.cancel")).clicked() {
                    cancel = true;
                }
                if ui.button(crate::lang::t0(&cc_civil::i18n::CATALOG, "crs.d.apply")).clicked() {
                    apply = true;
                }
            });
        });
    if cancel {
        app.par.dc_edit = None;
        return;
    }
    if let Some(dc) = app.par.dc_edit.as_mut() {
        dc.1.clone_from(&text);
    }
    if apply {
        app.run_ui_pub("par.edit", &Params { name: Some(format!("{cid}={}", text.trim())), ..Default::default() });
    }
}

/// gview.js `drawParametric`: bars and dimensional constraints over the model, glyphs and labels kept for picking.
pub fn draw(app: &UiApp, painter: &egui::Painter, to_pos: &dyn Fn(f64, f64) -> egui::Pos2, zoom_px: f64) {
    app.par.hits.borrow_mut().clear();
    let Some(ed) = app.editor.as_ref() else { return };
    let doc = ed.eng.doc();
    if !doc.in_model() || doc.constraints.is_empty() {
        return;
    }
    let view = ed.eng.state().memo.param.view.clone().unwrap_or_default();
    let tok = app.tokens;
    let sel_col = Color32::from_rgb(0x3c, 0xd0, 0x70);
    let dim_col = if tok.canvas_bg.r() < 128 { Color32::from_rgb(0xb7, 0x9c, 0xf0) } else { Color32::from_rgb(0x6a, 0x3f, 0xb5) };
    let bar_col = tok.text_dim;
    let all = cons(&doc);
    let mut hits: Vec<Hit> = Vec::new();
    // the selected constraint's objects, highlighted
    if let Some(sel) = app.par.selected.as_ref().and_then(|id| all.iter().find(|c| &c.id == id)) {
        let ents: Vec<&cc_cad::Entity> =
            sel.refs.iter().filter_map(|r| doc.space_map(cc_cad::doc::MODEL).and_then(|m| m.get(&r.e))).collect();
        for b in cc_cad_geom::display::walk_batches(&doc, &ents) {
            for l in &b.lines {
                let pts: Vec<egui::Pos2> = l.as_chunks::<2>().0.iter().map(|&[x, y]| to_pos(x, y)).collect();
                if pts.len() >= 2 {
                    painter.add(egui::Shape::line(pts, egui::Stroke::new(3.0, sel_col.gamma_multiply(0.5))));
                }
            }
        }
    }
    // dimensional constraints: dimensions in their own colour, at a fixed size on screen
    if view.dims {
        for c in all.iter().filter(|c| is_dim(c)) {
            let mut st = doc.current.dim.clone();
            let txt = st.get("txt").and_then(serde_json::Value::as_f64).filter(|v| *v > 0.0).unwrap_or(2.5);
            st.insert("scale".into(), serde_json::json!(11.0 / (zoom_px * txt)));
            let Some(de) = dim_entity_for(&doc, c, Some(&st)) else { continue };
            let col = if Some(&c.id) == app.par.selected.as_ref() { sel_col } else { dim_col };
            let parts = cc_cad_geom::geom::dim_parts(&de, None, None);
            let lines: Vec<&cc_cad::Entity> =
                parts.iter().filter(|p| !matches!(p.geo, cc_cad::entity::Geo::Text(_) | cc_cad::entity::Geo::Hatch(_))).collect();
            for b in cc_cad_geom::display::walk_batches(&doc, &lines) {
                for l in &b.lines {
                    let pts: Vec<egui::Pos2> = l.as_chunks::<2>().0.iter().map(|&[x, y]| to_pos(x, y)).collect();
                    if pts.len() >= 2 {
                        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0, col)));
                    }
                }
            }
            for p in &parts {
                match &p.geo {
                    cc_cad::entity::Geo::Hatch(h) => {
                        for lp in &h.loops {
                            let pts: Vec<egui::Pos2> = lp.as_chunks::<2>().0.iter().map(|&[x, y]| to_pos(x, y)).collect();
                            if pts.len() >= 3 {
                                painter.add(egui::Shape::convex_polygon(pts, col, egui::Stroke::NONE));
                            }
                        }
                    }
                    cc_cad::entity::Geo::Text(tx) => {
                        let at = to_pos(tx.x, tx.y);
                        let font = egui::FontId::proportional(12.0);
                        let galley = painter.layout_no_wrap(tx.s.clone(), font, col);
                        let size = galley.size();
                        let r = egui::Rect::from_center_size(at, size + egui::vec2(4.0, 2.0));
                        painter.rect_filled(r, 1.0, tok.canvas_bg.gamma_multiply(0.85));
                        painter.galley(r.min + egui::vec2(2.0, 1.0), galley, col);
                        hits.push(Hit { rect: r, cid: c.id.clone(), dim: true });
                    }
                    _ => {}
                }
            }
        }
    }
    if view.bars_all {
        // coincident: a small square on the point
        for c in all.iter().filter(|c| c.typ == "coincident") {
            if c.refs.iter().any(|r| view.hidden.contains(&r.e)) {
                continue;
            }
            let Some(r0) = c.refs.first() else { continue };
            let Some(e) = doc.space_map(cc_cad::doc::MODEL).and_then(|m| m.get(&r0.e)) else { continue };
            let Some(p) = ref_point(e, &r0.k) else { continue };
            let s = to_pos(p[0], p[1]);
            let on = Some(&c.id) == app.par.selected.as_ref();
            painter.rect_filled(egui::Rect::from_center_size(s, egui::vec2(6.0, 6.0)), 0.0, if on { sel_col } else { bar_col });
            hits.push(Hit { rect: egui::Rect::from_center_size(s, egui::vec2(10.0, 10.0)), cid: c.id.clone(), dim: false });
        }
        // bars: one per constrained reference
        let mut bars: Vec<((u64, String), Vec<Con>)> = Vec::new();
        for c in all.iter().filter(|c| !is_dim(c) && c.typ != "coincident") {
            for r in c.refs.iter() {
                if view.hidden.contains(&r.e) {
                    continue;
                }
                match bars.iter_mut().find(|(k, _)| k.0 == r.e && k.1 == r.k) {
                    Some((_, l)) => {
                        if !l.iter().any(|x| x.id == c.id) {
                            l.push(c.clone());
                        }
                    }
                    None => bars.push(((r.e, r.k.clone()), vec![c.clone()])),
                }
            }
        }
        const BOX: f32 = 15.0;
        for ((id, k), list) in &bars {
            let Some(e) = doc.space_map(cc_cad::doc::MODEL).and_then(|m| m.get(id)) else { continue };
            let Some((x, y, dx, dy)) = bar_anchor(e, k) else { continue };
            let s = to_pos(x, y);
            let n = list.len() as f32;
            let x0 = s.x + (dx as f32) * 14.0 - n * BOX / 2.0;
            let y0 = s.y + (dy as f32) * 14.0 - BOX / 2.0;
            for (i, c) in list.iter().enumerate() {
                let r = egui::Rect::from_min_size(egui::pos2(x0 + i as f32 * BOX, y0), egui::vec2(BOX, BOX));
                let on = Some(&c.id) == app.par.selected.as_ref();
                painter.rect_filled(r, 0.0, if on { sel_col } else { tok.canvas_bg });
                painter.rect_stroke(r.shrink(0.5), 0.0, egui::Stroke::new(1.0, bar_col), egui::StrokeKind::Middle);
                let g = if GEOMETRIC.iter().any(|x| x.0 == c.typ) { glyph(&c.typ) } else { "?" };
                painter.text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    g,
                    egui::FontId::proportional(11.0),
                    if on { tok.canvas_bg } else { bar_col },
                );
                hits.push(Hit { rect: r, cid: c.id.clone(), dim: false });
            }
        }
    }
    *app.par.hits.borrow_mut() = hits;
}

/// gview.js `anchor(e, k)`: where a reference's bar sits (world) and which way on screen to push it off.
fn bar_anchor(e: &cc_cad::Entity, k: &str) -> Option<(f64, f64, f64, f64)> {
    let ty = e.typ();
    let (kind, kk) = if k == "*" {
        match ty {
            "line" => ('L', "L".to_string()),
            "point" => ('P', "p".to_string()),
            "poly" => ('L', "s0".to_string()),
            _ => ('R', "R".to_string()),
        }
    } else {
        (ref_kind(e, k)?, k.to_string())
    };
    match kind {
        'P' => {
            let p = ref_point(e, &kk)?;
            Some((p[0], p[1], 1.0, 1.0))
        }
        'L' => {
            let l = ref_line(e, &kk);
            let h = cc_cad::js::hypot(l[2] - l[0], l[3] - l[1]);
            let len = if h == 0.0 || h.is_nan() { 1.0 } else { h };
            Some(((l[0] + l[2]) / 2.0, (l[1] + l[3]) / 2.0, (l[3] - l[1]) / len, (l[2] - l[0]) / len))
        }
        _ => {
            let c = ref_circle(e, &kk);
            let s = std::f64::consts::FRAC_1_SQRT_2;
            Some((c[0] + c[2] * s, c[1] + c[2] * s, 1.0, -1.0))
        }
    }
}

/// `hitAt(state, sx, sy)`: the constraint under a screen point (labels first, then bars).
pub fn hit_at(app: &UiApp, p: egui::Pos2) -> Option<Hit> {
    let hits = app.par.hits.borrow();
    for dim in [true, false] {
        for h in hits.iter().rev() {
            if h.dim == dim && h.rect.expand(1.0).contains(p) {
                return Some(h.clone());
            }
        }
    }
    None
}

/// `pp.dblclick(world)`: the label edit dialog for a dimensional constraint's label.
pub fn open_edit(app: &mut UiApp, cid: &str) {
    let Some(ed) = app.editor.as_ref() else { return };
    let d = ed.eng.doc();
    let Some(c) = cons(&d).iter().find(|k| k.id == cid && is_dim(k)).cloned() else { return };
    let name = c.name().unwrap_or_default().to_string();
    let v = cc_gcons::gcons::params_of(&d).var(&name).map(|v| v.expr.clone());
    let text = format!("{name}={}", v.unwrap_or_else(|| cc_cad::js::num_to_string(c.value_num())));
    drop(d);
    app.par.dc_at = app.par.hits.borrow().iter().rev().find(|h| h.cid == cid && h.dim).map(|h| h.rect.center_bottom());
    app.par.dc_edit = Some((cid.to_string(), text, String::new()));
}

#[allow(dead_code)]
fn _bars(d: &cc_cad::Doc) -> usize {
    bars_for(d).len()
}
