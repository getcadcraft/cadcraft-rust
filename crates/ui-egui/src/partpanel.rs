//! PartCraft's panels in the app (M13): the tool bar (Save, Export, Undo / Redo, Fit, views), the feature history
//! on the left (features in order with their flags, the rollback bar, the selected feature's numbers and actions,
//! the part's facts), Properties / Parameters on the right, and the 3D view with face hover and picking. Words come
//! from PartCraft's own catalog (`partview::tr`); every action goes through `UiApp::run` (`part.*` commands).

use std::collections::HashMap;

use cc_part::{jv, params_part as PP, part as P, vec as V};
use egui::{Rect, RichText};
use serde_json::Value;

use crate::partview::{PartView, tr, trv};
use crate::theme::Tokens;
use crate::{Params, UiApp, view3d};

fn f3(v: f64) -> String {
    V::fmt(v, 3)
}

/// A number as partcraft.js `numInput` shows it: `String(Number(v.toFixed(6)))`.
fn show_num(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    let s = cc_part::js::to_fixed(v, 6);
    cc_part::js::num_str(cc_cad::js::str_to_number(&s))
}

/// What a widget asked for this frame (run after the panels let go of the view).
type Cmd = Option<(&'static str, Params)>;

fn cmd(id: &'static str, name: impl Into<String>) -> Cmd {
    Some((id, Params { name: Some(name.into()), ..Default::default() }))
}

/// A text field that commits on Enter or when it loses focus (Esc keeps the old text): the typed text, if changed.
fn field(ui: &mut egui::Ui, edits: &mut HashMap<String, String>, key: &str, shown: &str, width: f32, hint: &str) -> Option<String> {
    let mut text = edits.get(key).cloned().unwrap_or_else(|| shown.to_string());
    let resp = ui.add_sized([width, 18.0], egui::TextEdit::singleline(&mut text).id_salt(("pc-field", key))).on_hover_text(hint);
    if resp.has_focus() {
        edits.insert(key.to_string(), text.clone());
    }
    if resp.lost_focus() {
        edits.remove(key);
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if !esc && text.trim() != shown.trim() && !text.trim().is_empty() {
            return Some(text.trim().to_string());
        }
    }
    None
}

/// The whole PartCraft view: bars, panels, the 3D view.
pub fn ui(app: &mut UiApp, ui: &mut egui::Ui) {
    let t = app.tokens;
    let mut act: Cmd = None;
    let Some(pv) = app.part.as_mut() else { return };
    pv.tints = ([t.accent.r(), t.accent.g(), t.accent.b(), 255], [t.row_selected.r(), t.row_selected.g(), t.row_selected.b(), 255]);
    let c = t.canvas_bg;
    pv.view.style.background = [f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0, 1.0];
    pv.view.style.back = if app.preset == crate::theme::Preset::Dark { crate::model3d::BACK_MODEL } else { crate::model3d::BACK_PAPER };
    egui::Panel::top("part_bar")
        .exact_size(30.0)
        .frame(egui::Frame::new().fill(t.panel_darker).inner_margin(egui::Margin::symmetric(8, 3)))
        .show(ui, |ui| {
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal_centered(|ui| tool_bar(ui, pv, &t, &mut act));
            });
        });
    let frame = egui::Frame::new().fill(t.panel).inner_margin(6);
    egui::Panel::left("part_history").default_size(250.0).size_range(190.0..=420.0).resizable(true).frame(frame).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| history(ui, pv, &t, &mut act));
    });
    egui::Panel::right("part_side").default_size(290.0).size_range(220.0..=460.0).resizable(true).frame(frame).show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.selectable_label(pv.tab == "props", tr("ui.right.props")).clicked() {
                pv.tab = "props";
            }
            if ui.selectable_label(pv.tab == "params", tr("ui.right.params")).clicked() {
                pv.tab = "params";
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if pv.tab == "params" {
                parameters(ui, pv, &t, &mut act);
            } else {
                properties(ui, pv, &t);
            }
        });
    });
    egui::CentralPanel::no_frame().show(ui, |ui| {
        let rect = ui.max_rect();
        stage(ui, rect, pv, &t);
    });
    app.message = pv.status();
    if let Some((id, p)) = act {
        app.run_ui(id, &p);
    }
}

fn tool_bar(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    let name = pv.part.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    ui.label(RichText::new(name).strong().color(t.text_strong));
    if pv.dirty {
        ui.label(RichText::new("•").color(t.text_dim)).on_hover_text(tr("app.file.unsaved_title"));
    }
    ui.separator();
    if ui.button(tr("ui.tb.file.save")).on_hover_text(tr("ui.tb.file.save.title")).clicked() {
        *act = cmd("part.save", "");
    }
    ui.menu_button(tr("ui.tb.export"), |ui| {
        for (k, key) in [
            ("stl", "ui.tb.export.stl"),
            ("3mf", "ui.tb.export.3mf"),
            ("step", "ui.tb.export.step_faceted"),
            ("step214", "ui.tb.export.step214_faceted"),
            ("ifc", "ui.tb.export.ifc"),
        ] {
            if ui.button(tr(key)).clicked() {
                *act = cmd("part.export", k);
                ui.close();
            }
        }
    })
    .response
    .on_hover_text(tr("ui.tb.export.title"));
    ui.separator();
    if ui.add_enabled(pv.can_undo(), egui::Button::new(tr("ui.tb.edit.undo"))).on_hover_text(tr("ui.tb.edit.undo.title")).clicked() {
        *act = cmd("part.undo", "");
    }
    if ui.add_enabled(pv.can_redo(), egui::Button::new(tr("ui.tb.edit.redo"))).on_hover_text(tr("ui.tb.edit.redo.title")).clicked() {
        *act = cmd("part.redo", "");
    }
    ui.separator();
    if ui.button(tr("ui.tb.view.view_fit")).on_hover_text(tr("ui.tb.view.view_fit.title")).clicked() {
        pv.fit();
    }
    for v in ["iso", "front", "top", "right"] {
        if ui.button(tr(&format!("ui.tb.view.{v}"))).clicked() {
            pv.standard(v);
        }
    }
    ui.separator();
    let st = pv.view.style;
    let mut edges = st.edges;
    if ui.checkbox(&mut edges, "Edges").changed() {
        pv.view.style.edges = edges;
    }
}

fn status_flag(r: Option<&Value>, f: &Value) -> String {
    let Some(r) = r else { return String::new() };
    match jv::s(r, "status") {
        "error" => tr("app.tree.flag_error"),
        "suppressed" => tr("app.tree.flag_off"),
        "skipped" => tr("app.tree.flag_not_built"),
        _ if r.get("warning").is_some() => "⚠".into(),
        _ if jv::s(f, "type") == "sketch" => match r.get("solve").map(|s| jv::s(s, "status")) {
            Some("under") => "−".into(),
            Some("over") => "+".into(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

fn history(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    let name = pv.part.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    ui.label(RichText::new(name).strong().color(t.text_strong));
    ui.label(RichText::new(tr("ui.left.aria")).small().color(t.text_dim));
    ui.separator();
    let feats = P::features(&pv.part).to_vec();
    let built = P::built_count(&pv.part);
    let rolled_back = pv.part.get("rollback").is_some_and(|r| !r.is_null());
    for (i, f) in feats.iter().enumerate() {
        if rolled_back && (i as f64) == built {
            let b = egui::Button::new(RichText::new(tr("app.tree.rollbar")).small().color(t.text_strong)).fill(t.row_selected);
            if ui.add_sized([ui.available_width(), 18.0], b).on_hover_text(tr("app.tree.rollbar_title")).clicked() {
                *act = cmd("part.rollto", "end");
            }
        }
        let id = jv::s(f, "id").to_string();
        let r = pv.regen.results.get(&id);
        let flag = status_flag(r, f);
        let status = r.map(|r| jv::s(r, "status")).unwrap_or("ok");
        let color = match status {
            "rolled" | "suppressed" => t.text_dim,
            "error" | "skipped" => t.text_strong,
            _ => t.text,
        };
        let mut text = RichText::new(jv::s(f, "name")).color(color);
        if status == "suppressed" {
            text = text.strikethrough();
        }
        let selected = pv.sel_feature.as_deref() == Some(id.as_str());
        let tip = r
            .and_then(|r| r.get("message").or_else(|| r.get("warning")))
            .and_then(Value::as_str)
            .map_or_else(|| P::feature_label(jv::s(f, "type")), str::to_string);
        ui.horizontal(|ui| {
            let resp = ui.selectable_label(selected, text).on_hover_text(tip);
            if resp.clicked() {
                *act = cmd("part.select", if selected { String::new() } else { id.clone() });
            }
            if !flag.is_empty() {
                ui.label(RichText::new(flag).small().color(t.text_strong));
            }
        });
    }
    if feats.is_empty() {
        ui.label(RichText::new(tr("app.tree.no_features")).color(t.text_dim));
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let n = feats.len() as f64;
        let b = if built.is_finite() { built } else { n };
        if ui.add_enabled(b > 0.0, egui::Button::new("◀")).on_hover_text(tr("ui.roll.back.title")).clicked() {
            let back = (b - 1.0).max(0.0) as usize;
            *act = feats.get(back).and_then(|f| cmd("part.rollto", jv::s(f, "id")));
        }
        if ui.add_enabled(rolled_back, egui::Button::new("▶")).on_hover_text(tr("ui.roll.forward.title")).clicked() {
            let fwd = (b + 1.0) as usize;
            *act = match feats.get(fwd) {
                Some(f) => cmd("part.rollto", jv::s(f, "id")),
                None => cmd("part.rollto", "end"),
            };
        }
        if ui.add_enabled(rolled_back, egui::Button::new("▶▶")).on_hover_text(tr("ui.roll.end.title")).clicked() {
            *act = cmd("part.rollto", "end");
        }
    });
    if let Some(sel) = pv.sel_feature.clone()
        && let Some(f) = P::feature_by_id(&pv.part, &sel).cloned()
    {
        ui.separator();
        feature_block(ui, pv, &f, t, act);
    }
    ui.separator();
    facts(ui, pv, t);
}

fn feature_block(ui: &mut egui::Ui, pv: &mut PartView, f: &Value, t: &Tokens, act: &mut Cmd) {
    let id = jv::s(f, "id").to_string();
    let fname = jv::s(f, "name").to_string();
    ui.label(RichText::new(format!("{fname} · {}", P::feature_label(jv::s(f, "type")))).strong().color(t.text_strong));
    if let Some(r) = pv.regen.results.get(&id) {
        let words = match jv::s(r, "status") {
            "error" | "skipped" => {
                r.get("message").and_then(Value::as_str).map(|m| trv("app.feature.not_built", &[("reason", m.to_string())]))
            }
            "suppressed" => Some(tr("app.feature.suppressed_msg")),
            "rolled" => Some(tr("app.feature.rolled_msg")),
            _ => r.get("warning").and_then(Value::as_str).map(str::to_string),
        };
        if let Some(w) = words {
            ui.label(RichText::new(w).small().color(t.text_strong));
        }
    }
    let targets = PP::targets(&pv.part);
    let p = PP::params_of(&pv.part);
    let prefix = format!("{fname} ");
    egui::Grid::new(("pc-feat", id.clone())).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        for (key, tg) in targets.iter().filter(|(_, tg)| tg.feature == id && tg.field.as_deref().is_none_or(|fl| PP::field_in_use(f, fl))) {
            let label = tg.label.strip_prefix(&prefix).unwrap_or(&tg.label).to_string();
            let link = p.link(key);
            ui.label(RichText::new(if link.is_some() { format!("{label} ƒ") } else { label }).color(t.text));
            let v = PP::target_value(&pv.part, tg);
            let hint = match link {
                Some(l) => trv("ui.params.field.driven", &[("expr", l.expr.clone())]),
                None => tr("ui.params.field.plain"),
            };
            if let Some(text) = field(ui, &mut pv.edits, key, &show_num(v), 90.0, &hint) {
                *act = cmd("part.param", format!("{key}={text}"));
            }
            ui.end_row();
        }
    });
    ui.horizontal_wrapped(|ui| {
        let off = jv::truthy(f.get("suppressed"));
        if ui.button(tr(if off { "app.history.unsuppress" } else { "app.history.suppress" })).clicked() {
            *act = cmd("part.suppress", id.clone());
        }
        if ui.button(tr("app.history.roll_here")).on_hover_text(tr("app.history.roll_here_title")).clicked() {
            *act = cmd("part.rollto", id.clone());
        }
    });
}

fn facts(ui: &mut egui::Ui, pv: &PartView, t: &Tokens) {
    ui.label(RichText::new(tr("ui.left.facts")).strong().color(t.text_strong));
    let errs = pv.failing();
    let n = P::features(&pv.part).len();
    let mut rows: Vec<(String, String)> = Vec::new();
    match pv.regen.stats.as_ref() {
        None => rows.push((tr("app.facts.solid"), tr("app.facts.none_yet"))),
        Some(s) => {
            let lo = s.get("bbox").and_then(|b| b.get("min"));
            let hi = s.get("bbox").and_then(|b| b.get("max"));
            let size: Vec<String> = (0..3)
                .map(|i| {
                    let g = |x: Option<&Value>| x.and_then(|a| a.get(i)).and_then(Value::as_f64).unwrap_or(0.0);
                    f3(g(hi) - g(lo))
                })
                .collect();
            let bodies = jv::n(s, "bodies");
            rows.push((tr("app.facts.volume"), format!("{} mm³", f3(jv::n(s, "volume")))));
            rows.push((tr("app.facts.area"), format!("{} mm²", f3(jv::n(s, "area")))));
            rows.push((tr("app.facts.size"), format!("{} mm", size.join(" × "))));
            rows.push((tr("app.facts.through_holes"), if bodies == 1.0 { cc_part::js::num_str(jv::n(s, "genus")) } else { String::new() }));
            rows.push((tr("app.facts.bodies"), cc_part::js::num_str(bodies)));
            rows.push((tr("app.facts.triangles"), group3(jv::n(s, "tris"))));
        }
    }
    rows.push((tr("app.facts.features"), n.to_string()));
    rows.push((tr("app.facts.failing"), if errs > 0 { errs.to_string() } else { String::new() }));
    egui::Grid::new("pc-facts").num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
        for (k, v) in rows {
            ui.label(RichText::new(k).small().color(t.text_dim));
            ui.label(RichText::new(v).small().color(t.text));
            ui.end_row();
        }
    });
    ui.label(RichText::new(format!("{} ms", f3(pv.regen_ms))).small().color(t.text_dim)).on_hover_text("Rebuild time (mesh kernel)");
}

/// `toLocaleString('en-US')` for a whole number.
fn group3(v: f64) -> String {
    let s = cc_part::js::num_str(v.round());
    let (sign, digits) = s.strip_prefix('-').map_or(("", s.as_str()), |d| ("-", d));
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("{sign}{out}")
}

fn properties(ui: &mut egui::Ui, pv: &PartView, t: &Tokens) {
    let shown = pv.sel_face.or(pv.hover);
    let Some(fi) = shown.and_then(|i| pv.face(i)) else {
        ui.label(RichText::new(tr("app.props.part")).strong().color(t.text_strong));
        let res = jv::n(&pv.part, "resolution");
        let curves = match res as i64 {
            32 => tr("app.part.res_coarse"),
            64 => tr("app.part.res_normal"),
            128 => tr("app.part.res_fine"),
            256 => tr("app.part.res_very_fine"),
            _ => cc_part::js::num_str(res),
        };
        egui::Grid::new("pc-part-props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label(RichText::new(tr("app.props.name")).color(t.text_dim));
            ui.label(RichText::new(pv.part.get("name").and_then(Value::as_str).unwrap_or("")).color(t.text));
            ui.end_row();
            ui.label(RichText::new(tr("app.part.curves")).color(t.text_dim));
            ui.label(RichText::new(curves).color(t.text));
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.label(RichText::new(tr("app.part.note_mesh_off")).small().color(t.text_dim));
        return;
    };
    ui.label(RichText::new(tr("app.props.face")).strong().color(t.text_strong));
    let ty = jv::s(fi, "type");
    let kind = match ty {
        "plane" => tr("app.face.flat"),
        "cylinder" => tr("app.face.cylinder"),
        "cone" => tr("app.face.cone"),
        "sphere" => tr("app.face.sphere"),
        "torus" => tr("app.face.torus"),
        "spline" => tr("app.face.spline"),
        _ => tr("app.face.curved"),
    };
    let name = jv::s(fi, "name").to_string();
    let maker = name.split('.').next().and_then(|fid| P::feature_by_id(&pv.part, fid)).map(|g| jv::s(g, "name").to_string());
    let v3 =
        |x: Option<&Value>| -> String { jv::arr(x).iter().map(|c| V::fmt(c.as_f64().unwrap_or(0.0), 4)).collect::<Vec<_>>().join(", ") };
    let r = fi.get("r").and_then(Value::as_f64).filter(|r| *r != 0.0);
    let rows = [
        (tr("app.props.kind"), trv("app.props.exact", &[("kind", kind)])),
        (tr("app.props.radius"), r.map_or(String::new(), |r| format!("{} mm (Ø{})", f3(r), f3(2.0 * r)))),
        (tr("app.props.area"), format!("{} mm²", f3(jv::n(fi, "area")))),
        (tr("app.props.made_by"), maker.clone().unwrap_or_default()),
        (tr("app.props.face"), if name.is_empty() { String::new() } else { pv.face_words(&name) }),
        (tr("app.props.normal"), if ty == "plane" { v3(fi.get("n")) } else { String::new() }),
        (tr("app.props.axis"), fi.get("axis").map_or(String::new(), |a| v3(a.get("d")))),
        (
            tr("app.props.reference"),
            if ty == "plane" {
                trv("app.face.follows", &[("name", maker.unwrap_or_else(|| tr("app.face.its_feature")))])
            } else {
                String::new()
            },
        ),
    ];
    egui::Grid::new("pc-face-props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        for (k, v) in rows.into_iter().filter(|(_, v)| !v.is_empty()) {
            ui.label(RichText::new(k).color(t.text_dim));
            ui.label(RichText::new(v).color(t.text));
            ui.end_row();
        }
    });
    ui.label(RichText::new(name).small().monospace().color(t.text_dim));
}

fn parameters(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    ui.label(RichText::new(tr("ui.params.intro")).small().color(t.text_dim));
    ui.add_space(4.0);
    ui.label(RichText::new(tr("ui.params.vars")).strong().color(t.text_strong));
    let p = PP::params_of(&pv.part);
    let ev = cc_part::params::evaluate_vars(&p, None);
    egui::Grid::new("pc-vars").num_columns(3).striped(true).spacing([6.0, 3.0]).show(ui, |ui| {
        for k in ["ui.params.col.name", "ui.params.col.expr", "ui.params.col.value"] {
            ui.label(RichText::new(tr(k)).small().color(t.text_dim));
        }
        ui.end_row();
        for v in &p.vars {
            ui.label(RichText::new(&v.name).color(t.text)).on_hover_text(v.note.clone().unwrap_or_default());
            let key = format!("var:{}", v.name);
            if let Some(text) = field(ui, &mut pv.edits, &key, &v.expr, 110.0, &trv("ui.params.var.expr_of", &[("name", v.name.clone())])) {
                *act = cmd("part.param", format!("{}={text}", v.name));
            }
            let value = match (ev.values.get(&v.name), ev.errors.get(&v.name)) {
                (_, Some(e)) => RichText::new(format!("⚠ {e}")).small().color(t.text_strong),
                (Some(x), None) => RichText::new(show_num(*x)).monospace().color(t.text),
                _ => RichText::new(""),
            };
            ui.label(value);
            ui.end_row();
        }
    });
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut pv.new_param.0).hint_text(tr("ui.params.new.name_ph")).desired_width(80.0))
            .on_hover_text(tr("ui.params.new.name"));
        ui.add(egui::TextEdit::singleline(&mut pv.new_param.1).hint_text(tr("ui.params.new.value_ph")).desired_width(110.0))
            .on_hover_text(tr("ui.params.new.value"));
        if ui.button(tr("touch.add")).clicked() {
            let (n, v) = (pv.new_param.0.trim().to_string(), pv.new_param.1.trim().to_string());
            if n.is_empty() {
                pv.message = tr("ui.params.need_name");
            } else {
                *act = cmd("part.param", format!("{n}={}", if v.is_empty() { "0".into() } else { v }));
                pv.new_param = (String::new(), String::new());
            }
        }
    });
    ui.add_space(8.0);
    ui.label(RichText::new(tr("ui.params.eqs")).strong().color(t.text_strong));
    if p.links.is_empty() {
        ui.label(RichText::new(tr("ui.params.eqs.empty")).small().color(t.text_dim));
    }
    let targets = PP::targets(&pv.part);
    egui::Grid::new("pc-eqs").num_columns(2).spacing([6.0, 3.0]).show(ui, |ui| {
        for l in &p.links {
            let label = targets.get(&l.target).map_or_else(|| l.target.clone(), |tg| tg.label.clone());
            ui.label(RichText::new(&label).color(t.text));
            let key = format!("eq:{}", l.target);
            if let Some(text) =
                field(ui, &mut pv.edits, &key, &format!("={}", l.expr), 110.0, &trv("ui.params.eq.of", &[("label", label.clone())]))
            {
                let text = if text.starts_with('=') { text } else { format!("={text}") };
                *act = cmd("part.param", format!("{}={text}", l.target));
            }
            ui.end_row();
        }
    });
}

/// The 3D view: orbit / pan / zoom (view3d), hover names a face, a click selects it.
fn stage(ui: &mut egui::Ui, rect: Rect, pv: &mut PartView, t: &Tokens) {
    if pv.fit_pending {
        pv.view.fit_pending = true;
        pv.fit_pending = false;
    }
    let resp = view3d::show(ui, rect, &pv.scene, &mut pv.view);
    let ppp = f64::from(ui.ctx().pixels_per_point());
    let pos = resp.hover_pos();
    let px = pos.map(|p| (f64::from(p.x - rect.min.x) * ppp, f64::from(p.y - rect.min.y) * ppp));
    let key = (px, pv.view.cam);
    if pv.hover_key != Some(key) {
        pv.hover_key = Some(key);
        let h = px.and_then(|(x, y)| pv.face_at(x, y));
        if h != pv.hover {
            pv.hover = h;
            pv.retint();
        }
    }
    if resp.clicked_by(egui::PointerButton::Primary) {
        pv.sel_face = pv.hover;
        pv.tab = "props";
        pv.retint();
    }
    if resp.hovered() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        pv.sel_face = None;
        pv.retint();
    }
    // the hovered face's words beside the pointer
    if let (Some(p), Some(f)) = (pos, pv.hover.and_then(|i| pv.face(i))) {
        let words = pv.face_words(jv::s(f, "name"));
        let font = egui::FontId::proportional(12.0);
        let galley = ui.painter().layout_no_wrap(words, font, t.text_strong);
        let at = p + egui::vec2(14.0, 10.0);
        let r = Rect::from_min_size(at, galley.size()).expand(4.0);
        ui.painter().rect_filled(r, f32::from(t.radius), t.panel_darker);
        ui.painter().galley(at, galley, t.text_strong);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_as_partcraft_shows_them() {
        assert_eq!(show_num(12.0), "12");
        assert_eq!(show_num(0.1 + 0.2), "0.3");
        assert_eq!(show_num(f64::NAN), "");
        assert_eq!(group3(1698.0), "1,698");
        assert_eq!(group3(1234567.0), "1,234,567");
        assert_eq!(group3(12.0), "12");
    }

    #[test]
    fn the_view_draws_headless() {
        let mut app = UiApp::default();
        app.run("part.sample", &Params::default()).unwrap();
        app.run("part.select", &Params { name: Some("Body".into()), ..Default::default() }).unwrap();
        let ctx = egui::Context::default();
        for tab in ["props", "params"] {
            if let Some(pv) = app.part.as_mut() {
                pv.tab = tab;
            }
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| app.ui(ui));
            out.textures_delta.clear();
        }
        assert!(app.message.contains("Opened"), "{}", app.message);
    }
}
