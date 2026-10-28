//! PartCraft's panels in the app (M13): the tool bar (Save, Export, Undo / Redo, Fit, views), the feature history
//! on the left (features in order with their flags, the rollback bar, the selected feature's numbers and actions,
//! the part's facts), Properties / Parameters on the right, and the 3D view with face hover and picking. Words come
//! from PartCraft's own catalog (`partview::tr`); every action goes through `UiApp::run` (`part.*` commands).

use std::collections::HashMap;

use cc_part::{jv, params_part as PP, part as P, vec as V};
use egui::{Rect, RichText};
use serde_json::Value;

use crate::partview::{Mode, PartView, tr, trv};
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
    let dark = app.preset == crate::theme::Preset::Dark;
    let Some(pv) = app.part.as_mut() else { return };
    if pv.poll_rebuild() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    }
    pv.tints = ([t.accent.r(), t.accent.g(), t.accent.b(), 255], [t.row_selected.r(), t.row_selected.g(), t.row_selected.b(), 255]);
    let c = t.canvas_bg;
    pv.view.style.background = [f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0, 1.0];
    pv.view.style.back = if app.preset == crate::theme::Preset::Dark { crate::model3d::BACK_MODEL } else { crate::model3d::BACK_PAPER };
    egui::Panel::top("part_bar")
        .exact_size(30.0)
        .frame(egui::Frame::new().fill(t.panel_darker).inner_margin(egui::Margin::symmetric(8, 3)))
        .show(ui, |ui| {
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    if pv.mode == Mode::Sketch {
                        crate::partsketchui::sketch_bar(ui, pv, &t, &mut act);
                    } else {
                        tool_bar(ui, pv, &t, &mut act);
                    }
                });
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
            } else if pv.mode == Mode::Sketch {
                let mut num = std::mem::take(&mut pv.num_entry);
                crate::partsketchui::sketch_props(ui, pv, &t, &mut act, &mut num);
                pv.num_entry = num;
            } else {
                properties(ui, pv, &t);
            }
        });
    });
    egui::CentralPanel::no_frame().show(ui, |ui| {
        let rect = ui.max_rect();
        if pv.mode == Mode::Sketch {
            crate::partsketchui::stage(ui, rect, pv, &t, dark, &mut act);
        } else {
            stage(ui, rect, pv, &t, &mut act);
        }
    });
    confirm_dialog(ui, pv, &t, &mut act);
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
    if ui.button(tr("ui.tb.file.new")).on_hover_text(tr("ui.tb.file.new.title")).clicked() {
        *act = cmd("part.new", "");
    }
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
    if pv.mode == Mode::Pick {
        if ui
            .add(egui::Button::new(RichText::new(tr("ui.tb.features.pick_done")).strong()))
            .on_hover_text(tr("ui.tb.features.pick_done.title"))
            .clicked()
        {
            *act = cmd("part.pick.done", "");
        }
    } else {
        if ui
            .add(egui::Button::selectable(pv.pending_sketch, tr("ui.tb.sketch.sketch")))
            .on_hover_text(tr("ui.tb.sketch.sketch.title"))
            .clicked()
        {
            *act = cmd("part.sketch", "");
        }
        ui.separator();
        for (c, _, key) in crate::partfeat::FEATURE_CMDS.iter().filter(|c| c.0 != "revolve-cut") {
            if ui.button(tr(key)).on_hover_text(tr(&format!("{key}.title"))).clicked() {
                *act = cmd("part.feature", *c);
            }
        }
        ui.menu_button(tr("ui.tb.more"), |ui| {
            if ui.button(tr("ui.tb.fmore.revolve_cut")).on_hover_text(tr("ui.tb.fmore.revolve_cut.title")).clicked() {
                *act = cmd("part.feature", "revolve-cut");
                ui.close();
            }
        })
        .response
        .on_hover_text(tr("ui.tb.features.more.title"));
    }
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
    if ui.checkbox(&mut edges, cc_part::i18n::t0("app.fillet.edges")).changed() {
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
    for name in ["front", "top", "right"] {
        let on = pv.sel_plane.as_deref() == Some(name);
        if ui.selectable_label(on, RichText::new(crate::partview::plane_name(name)).color(t.text)).clicked() {
            *act = cmd("part.plane", name);
        }
    }
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
        let is_sketch = jv::s(f, "type") == "sketch";
        let picker = crate::partfeat::PICKERS.contains(&jv::s(f, "type"));
        ui.horizontal(|ui| {
            let resp = ui.selectable_label(selected, text).on_hover_text(tip);
            if resp.double_clicked() {
                if is_sketch {
                    *act = cmd("part.sketch", id.clone());
                } else if picker {
                    *act = cmd("part.pick", id.clone());
                }
            } else if resp.clicked() {
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

fn combo(ui: &mut egui::Ui, id: String, label: &str, cur: &str, opts: &[(String, String)]) -> Option<String> {
    let shown = opts.iter().find(|(v, _)| v == cur).map_or_else(|| cur.to_string(), |(_, l)| l.clone());
    let mut out = None;
    ui.label(label);
    egui::ComboBox::new(id, "").selected_text(shown).show_ui(ui, |ui| {
        for (v, l) in opts {
            if ui.selectable_label(v == cur, l).clicked() && v != cur {
                out = Some(v.clone());
            }
        }
    });
    ui.end_row();
    out
}

fn num_row(ui: &mut egui::Ui, pv: &mut PartView, id: &str, field_name: &str, label: &str, act: &mut Cmd) {
    let Some(f) = P::feature_by_id(&pv.part, id) else { return };
    let v = jv::n(f, field_name);
    let key = format!("{id}.{field_name}");
    let link = PP::params_of(&pv.part).link(&key).map(|l| l.expr.clone());
    ui.label(if link.is_some() { format!("{label} ƒ") } else { label.to_string() });
    let hint = match link {
        Some(e) => trv("ui.params.field.driven", &[("expr", e)]),
        None => tr("ui.params.field.plain"),
    };
    if let Some(text) = field(ui, &mut pv.edits, &key, &show_num(v), 90.0, &hint) {
        *act = cmd("part.set", format!("{id}:{field_name}={text}"));
    }
    ui.end_row();
}

fn check_row(ui: &mut egui::Ui, on: bool, label: &str) -> Option<bool> {
    let mut v = on;
    let r = ui.checkbox(&mut v, label).changed().then_some(v);
    ui.end_row();
    r
}

fn dims_block(ui: &mut egui::Ui, pv: &mut PartView, sid: &str, title_key: &str, t: &Tokens, act: &mut Cmd) {
    let Some(sf) = P::feature_by_id(&pv.part, sid).cloned() else { return };
    let sk = sf.get("sketch").cloned().unwrap_or(Value::Null);
    let dims: Vec<Value> =
        cc_part::sketch::constraints(&sk).iter().filter(|c| cc_part::sketch::is_dim(jv::s(c, "type"))).cloned().collect();
    if dims.is_empty() {
        return;
    }
    ui.label(
        RichText::new(trv("app.dims.title", &[("title", tr(title_key)), ("name", jv::s(&sf, "name").to_string())]))
            .strong()
            .color(t.text_strong),
    );
    egui::Grid::new(("pc-dims", sid.to_string())).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        for c in &dims {
            let ty = jv::s(c, "type");
            let cid = jv::s(c, "id");
            let label = format!("{}{}", cc_part::sketch::label_of(ty).unwrap_or_default(), if ty == "angle" { " (°)" } else { "" });
            ui.label(label);
            let key = format!("{sid}.{cid}");
            if let Some(text) = field(ui, &mut pv.edits, &key, &show_num(jv::n(c, "value")), 90.0, &tr("ui.params.field.plain")) {
                *act = cmd("part.dim", format!("{sid}:{cid}={text}"));
            }
            ui.end_row();
        }
    });
}

fn ref_list(ui: &mut egui::Ui, pv: &PartView, f: &Value, key: &str, t: &Tokens, act: &mut Cmd) {
    let id = jv::s(f, "id").to_string();
    ui.label(RichText::new(tr(if key == "edges" { "app.refs.edges_note" } else { "app.refs.faces_note" })).small().color(t.text_dim));
    let items = jv::arr(f.get(key));
    if items.is_empty() {
        ui.label(RichText::new(tr(if key == "edges" { "app.refs.no_edges" } else { "app.refs.no_faces" })).small().color(t.text_dim));
    }
    for (i, r) in items.iter().enumerate() {
        let label = if key == "edges" {
            let fs = jv::arr(r.get("faces"));
            let w = |k: usize| pv.face_words(fs.get(k).and_then(Value::as_str).unwrap_or(""));
            trv("app.refs.edge_n", &[("n", (i + 1).to_string()), ("a", w(0)), ("b", w(1))])
        } else {
            trv("app.refs.face_n", &[("n", (i + 1).to_string()), ("name", pv.face_words(jv::s(r, "name")))])
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).small().color(t.text));
            if ui.small_button(tr("app.btn.remove")).clicked() {
                *act = cmd("part.set", format!("{id}:drop:{i}"));
            }
        });
    }
    let picking = pv.mode == Mode::Pick && pv.editing.as_deref() == Some(id.as_str());
    if picking {
        if ui.button(RichText::new(tr("app.refs.done_picking")).strong()).clicked() {
            *act = cmd("part.pick.done", "");
        }
    } else if ui.button(tr(if key == "edges" { "app.refs.pick_edges" } else { "app.refs.pick_faces" })).clicked() {
        *act = cmd("part.pick", id);
    }
}

/// partcraft.js `featureProps(f)`.
fn feature_block(ui: &mut egui::Ui, pv: &mut PartView, f: &Value, t: &Tokens, act: &mut Cmd) {
    let id = jv::s(f, "id").to_string();
    let fname = jv::s(f, "name").to_string();
    let ty = jv::s(f, "type").to_string();
    ui.label(RichText::new(format!("{fname} · {}", P::feature_label(&ty))).strong().color(t.text_strong));
    if let Some(r) = pv.regen.results.get(&id) {
        let words = match jv::s(r, "status") {
            "error" => r.get("message").and_then(Value::as_str).map(str::to_string),
            "skipped" => r.get("message").and_then(Value::as_str).map(|m| trv("app.feature.not_built", &[("reason", m.to_string())])),
            "suppressed" => Some(tr("app.feature.suppressed_msg")),
            _ if pv.mode == Mode::Pick && pv.editing.as_deref() == Some(id.as_str()) => Some(trv(
                if crate::partfeat::FACE_PICKERS.contains(&ty.as_str()) {
                    "app.feature.picking_faces"
                } else {
                    "app.feature.picking_edges"
                },
                &[("name", fname.clone())],
            )),
            "rolled" => Some(tr("app.feature.rolled_msg")),
            _ => None,
        };
        if let Some(w) = words {
            ui.label(RichText::new(w).small().color(t.text_strong));
        }
        if let Some(w) = r.get("warning").and_then(Value::as_str) {
            ui.label(RichText::new(w).small().color(t.text_strong));
        }
    }
    let op_opts: Vec<(String, String)> =
        ["add", "cut", "intersect", "new"].iter().map(|k| ((*k).to_string(), tr(&format!("app.op.{k}")))).collect();
    let sketch_row = |ui: &mut egui::Ui, pv: &PartView| {
        ui.label(tr("app.props.sketch_row"));
        let s = f
            .get("sketch")
            .and_then(Value::as_str)
            .and_then(|sid| P::feature_by_id(&pv.part, sid))
            .map_or_else(|| tr("app.props.gone"), |g| jv::s(g, "name").to_string());
        ui.label(RichText::new(s).color(t.text));
        ui.end_row();
    };
    let set = |act: &mut Cmd, field_name: &str, v: String| *act = cmd("part.set", format!("{id}:{field_name}={v}"));
    match ty.as_str() {
        "sketch" => {
            let sk = f.get("sketch").cloned().unwrap_or(Value::Null);
            let r = pv.regen.results.get(&id);
            let state = r.and_then(|r| r.get("solve")).map_or(String::new(), |s| match jv::s(s, "status") {
                "full" => tr("app.sketch.state_full"),
                "under" => trv("app.sketch.state_under_title", &[("n", cc_part::js::num_str(jv::n(s, "dof")))]),
                "over" => tr("app.sketch.state_over"),
                "empty" => tr("app.sketch.state_empty"),
                _ => String::new(),
            });
            let curves = cc_part::sketch::entities(&sk).iter().filter(|e| jv::s(e, "type") != "point").count();
            let points = cc_part::sketch::loose_points(&sk).len();
            egui::Grid::new(("pc-sk-facts", id.clone())).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                for (k, v) in [
                    (tr("app.props.plane"), pv.plane_label(f.get("plane"))),
                    (tr("app.props.state"), state),
                    (
                        tr("app.props.geometry"),
                        trv("app.props.geometry_value", &[("curves", curves.to_string()), ("points", points.to_string())]),
                    ),
                    (tr("app.props.relations"), cc_part::sketch::constraints(&sk).len().to_string()),
                ] {
                    ui.label(RichText::new(k).color(t.text_dim));
                    ui.label(RichText::new(v).color(t.text));
                    ui.end_row();
                }
            });
            if ui.button(RichText::new(tr("app.btn.edit_sketch")).strong()).clicked() {
                *act = cmd("part.sketch", id.clone());
            }
            dims_block(ui, pv, &id, "app.dims.driving", t, act);
        }
        "extrude" | "revolve" | "hole" | "lpattern" | "cpattern" | "mirror" | "fillet" | "chamfer" | "shell" => {
            egui::Grid::new(("pc-feat", id.clone())).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| match ty.as_str() {
                "extrude" => {
                    sketch_row(ui, pv);
                    if let Some(v) = combo(ui, format!("pc-op-{id}"), &tr("app.props.operation"), jv::s(f, "op"), &op_opts) {
                        set(act, "op", v);
                    }
                    let ends: Vec<(String, String)> = ["blind", "mid", "through", "upto", "next", "vertex", "offset"]
                        .iter()
                        .map(|k| ((*k).to_string(), tr(&format!("ui.design.end.{k}"))))
                        .collect();
                    let end = jv::s(f, "end").to_string();
                    if let Some(v) = combo(ui, format!("pc-end-{id}"), &tr("app.props.end"), &end, &ends) {
                        set(act, "end", v);
                    }
                    if end == "blind" || end == "mid" {
                        num_row(ui, pv, &id, "depth", &tr("app.props.depth_mm"), act);
                    }
                    if end != "mid"
                        && let Some(v) = check_row(ui, jv::truthy(f.get("reverse")), &tr("app.props.reverse"))
                    {
                        set(act, "reverse", v.to_string());
                    }
                }
                "revolve" => {
                    sketch_row(ui, pv);
                    if let Some(v) = combo(ui, format!("pc-op-{id}"), &tr("app.props.operation"), jv::s(f, "op"), &op_opts) {
                        set(act, "op", v);
                    }
                    num_row(ui, pv, &id, "angle", &tr("app.props.angle_deg"), act);
                    let sk = f
                        .get("sketch")
                        .and_then(Value::as_str)
                        .and_then(|s| P::feature_by_id(&pv.part, s))
                        .and_then(|g| g.get("sketch"))
                        .cloned();
                    if let Some(sk) = sk {
                        let mut opts = vec![(String::new(), tr("app.revolve.first_centerline"))];
                        for (i, l) in cc_part::sketch::entities(&sk).iter().filter(|e| jv::s(e, "type") == "line").enumerate() {
                            let k = if jv::truthy(l.get("construction")) { "app.revolve.centerline_n" } else { "app.revolve.line_n" };
                            opts.push((
                                jv::s(l, "id").to_string(),
                                trv(k, &[("n", (i + 1).to_string()), ("id", jv::s(l, "id").to_string())]),
                            ));
                        }
                        let at = P::index_of(&pv.part, &id).unwrap_or(0);
                        for g in P::features(&pv.part).iter().take(at).filter(|g| jv::s(g, "type") == "axis") {
                            opts.push((
                                format!("ref:{}", jv::s(g, "id")),
                                trv("app.revolve.axis_feature", &[("name", jv::s(g, "name").to_string())]),
                            ));
                        }
                        let cur = match f.get("axis") {
                            Some(Value::Object(o)) => {
                                if o.get("kind").and_then(Value::as_str) == Some("ref") {
                                    format!("ref:{}", o.get("feature").and_then(Value::as_str).unwrap_or(""))
                                } else {
                                    "obj".to_string()
                                }
                            }
                            Some(Value::String(s)) => s.clone(),
                            _ => String::new(),
                        };
                        if cur == "obj" {
                            opts.push(("obj".into(), tr("app.revolve.picked_axis")));
                        }
                        if let Some(v) = combo(ui, format!("pc-axis-{id}"), &tr("app.props.axis"), &cur, &opts) {
                            set(act, "axis", v);
                        }
                    }
                }
                "hole" => {
                    sketch_row(ui, pv);
                    ui.label(tr("app.hole.holes"));
                    let n = pv.regen.results.get(&id).and_then(|r| r.get("count")).and_then(Value::as_f64).map_or_else(
                        || {
                            let sk = f
                                .get("sketch")
                                .and_then(Value::as_str)
                                .and_then(|s| P::feature_by_id(&pv.part, s))
                                .and_then(|g| g.get("sketch"));
                            sk.map_or(0, |sk| cc_part::sketch::loose_points(sk).len()).to_string()
                        },
                        cc_part::js::num_str,
                    );
                    ui.label(trv("app.hole.count", &[("n", n)]));
                    ui.end_row();
                    let kinds: Vec<(String, String)> = ["simple", "cbore", "csink", "tapped"]
                        .iter()
                        .map(|k| ((*k).to_string(), tr(&format!("ui.design.hole.{k}"))))
                        .collect();
                    if let Some(v) = combo(ui, format!("pc-kind-{id}"), &tr("app.hole.type"), jv::s(f, "kind"), &kinds) {
                        set(act, "kind", v);
                    }
                    num_row(ui, pv, &id, "d", &tr("app.hole.diameter_mm"), act);
                    let through = jv::truthy(f.get("through"));
                    if let Some(v) = check_row(ui, through, &tr("app.hole.through_all")) {
                        set(act, "through", v.to_string());
                    }
                    if !through {
                        num_row(ui, pv, &id, "depth", &tr("app.props.depth_mm"), act);
                    }
                    if jv::s(f, "kind") == "cbore" {
                        num_row(ui, pv, &id, "cboreD", &tr("app.hole.cbore_d"), act);
                        num_row(ui, pv, &id, "cboreDepth", &tr("app.hole.cbore_depth"), act);
                    }
                    if let Some(v) = check_row(ui, jv::truthy(f.get("reverse")), &tr("app.props.reverse")) {
                        set(act, "reverse", v.to_string());
                    }
                }
                "lpattern" | "cpattern" | "mirror" => {
                    let at = P::index_of(&pv.part, &id).unwrap_or(0);
                    let earlier: Vec<Value> = P::features(&pv.part)
                        .iter()
                        .take(at)
                        .filter(|g| crate::partfeat::SOLID.contains(&jv::s(g, "type")))
                        .cloned()
                        .collect();
                    ui.label(RichText::new(tr(if ty == "mirror" { "app.seeds.mirror" } else { "app.seeds.to_copy" })).color(t.text_dim));
                    ui.end_row();
                    let body = jv::is_str(f.get("seeds"), "body");
                    if ty == "mirror"
                        && let Some(v) = check_row(ui, body, &tr("app.seeds.whole_body"))
                    {
                        set(act, "body", v.to_string());
                    }
                    if !body {
                        for g in &earlier {
                            let gid = jv::s(g, "id");
                            let on = jv::arr(f.get("seeds")).iter().any(|x| x.as_str() == Some(gid));
                            if let Some(v) = check_row(ui, on, jv::s(g, "name")) {
                                set(act, &format!("seed:{gid}"), v.to_string());
                            }
                        }
                    }
                    if ty == "mirror" {
                        let mut opts: Vec<(String, String)> =
                            ["front", "top", "right"].iter().map(|k| (format!("base:{k}"), tr(&format!("app.mirror.{k}")))).collect();
                        for g in P::features(&pv.part).iter().take(at).filter(|g| jv::s(g, "type") == "plane") {
                            opts.push((format!("ref:{}", jv::s(g, "id")), jv::s(g, "name").to_string()));
                        }
                        let pl = f.get("plane").cloned().unwrap_or(Value::Null);
                        let cur = match jv::s(&pl, "kind") {
                            "base" => format!("base:{}", jv::s(&pl, "name")),
                            "ref" => format!("ref:{}", jv::s(&pl, "feature")),
                            _ => "cur".into(),
                        };
                        if cur == "cur" {
                            let w = if jv::s(&pl, "kind") == "face" {
                                let n = P::feature_by_id(&pv.part, jv::s(&pl, "feature"))
                                    .map_or_else(|| "?".into(), |g| jv::s(g, "name").to_string());
                                trv("app.mirror.a_face_of", &[("name", n)])
                            } else {
                                tr("app.mirror.a_fixed_plane")
                            };
                            opts.push(("cur".into(), w));
                        }
                        if pv.pick.as_ref().is_some_and(|p| p.face && p.flat) {
                            opts.push(("pick".into(), tr("app.mirror.picked_face")));
                        }
                        if let Some(v) = combo(ui, format!("pc-plane-{id}"), &tr("app.mirror.plane"), &cur, &opts) {
                            set(act, "plane", v);
                        }
                    } else {
                        let k = if ty == "lpattern" { "dir" } else { "axis" };
                        let mut opts: Vec<(String, String)> = vec![
                            ("world:x".into(), tr("app.dir.x")),
                            ("world:y".into(), tr("app.dir.y")),
                            ("world:z".into(), tr("app.dir.z")),
                        ];
                        for g in P::features(&pv.part).iter().take(at) {
                            let (gid, gname) = (jv::s(g, "id"), jv::s(g, "name").to_string());
                            match jv::s(g, "type") {
                                "revolve" => opts.push((format!("axis:{gid}"), trv("app.dir.axis_of", &[("name", gname)]))),
                                "axis" => opts.push((format!("ref:{gid}"), gname)),
                                "plane" => opts.push((format!("ref:{gid}"), trv("app.dir.plane_normal", &[("name", gname)]))),
                                "sketch" => {
                                    let sk = g.get("sketch").cloned().unwrap_or(Value::Null);
                                    for (n, l) in
                                        cc_part::sketch::entities(&sk).iter().filter(|e| jv::s(e, "type") == "line").take(30).enumerate()
                                    {
                                        let key =
                                            if jv::truthy(l.get("construction")) { "app.dir.centerline_of" } else { "app.dir.line_of" };
                                        opts.push((
                                            format!("line:{gid}:{}", jv::s(l, "id")),
                                            trv(key, &[("name", gname.clone()), ("n", (n + 1).to_string())]),
                                        ));
                                    }
                                }
                                _ => {}
                            }
                        }
                        let rf = f.get(k).cloned().unwrap_or(Value::Null);
                        let cur = match jv::s(&rf, "kind") {
                            "world" => format!("world:{}", jv::s(&rf, "axis")),
                            "axis" => format!("axis:{}", jv::s(&rf, "feature")),
                            "ref" => format!("ref:{}", jv::s(&rf, "feature")),
                            "line" => format!("line:{}:{}", jv::s(&rf, "sketch"), jv::s(&rf, "entity")),
                            _ => "fixed".into(),
                        };
                        if cur == "fixed" {
                            opts.push(("fixed".into(), tr("app.dir.picked_fixed")));
                        }
                        if pv.pick.as_ref().is_some_and(|p| !p.face) {
                            opts.push(("pick".into(), tr("app.dir.picked_now")));
                        }
                        let label = tr(if k == "dir" { "app.pattern.direction" } else { "app.props.axis" });
                        if let Some(v) = combo(ui, format!("pc-dir-{id}"), &label, &cur, &opts) {
                            set(act, k, v);
                        }
                        if ty == "lpattern" {
                            num_row(ui, pv, &id, "spacing", &tr("app.pattern.spacing_mm"), act);
                        } else {
                            num_row(ui, pv, &id, "angle", &tr("app.props.angle_deg"), act);
                            ui.label(
                                RichText::new(tr(if jv::n(f, "angle") == 360.0 {
                                    "app.pattern.full_circle"
                                } else {
                                    "app.pattern.spread"
                                }))
                                .small()
                                .color(t.text_dim),
                            );
                            ui.end_row();
                        }
                        num_row(ui, pv, &id, "count", &tr("app.pattern.count"), act);
                        if let Some(v) = check_row(ui, jv::truthy(f.get("reverse")), &tr("app.props.reverse")) {
                            set(act, "reverse", v.to_string());
                        }
                    }
                }
                "fillet" | "chamfer" => {
                    let k = if ty == "fillet" { "r" } else { "d" };
                    num_row(ui, pv, &id, k, &tr(if ty == "fillet" { "app.fillet.radius_mm" } else { "app.chamfer.distance_mm" }), act);
                    ui.label(tr("app.fillet.edges"));
                    ui.label(jv::arr(f.get("edges")).len().to_string());
                    ui.end_row();
                }
                _ => {
                    num_row(ui, pv, &id, "t", &tr("app.shell.wall_mm"), act);
                    if let Some(v) = check_row(ui, jv::truthy(f.get("outward")), &tr("app.shell.outward")) {
                        set(act, "outward", v.to_string());
                    }
                    ui.label(tr("app.shell.open_faces"));
                    ui.label(jv::arr(f.get("faces")).len().to_string());
                    ui.end_row();
                }
            });
            if matches!(ty.as_str(), "fillet" | "chamfer") {
                ref_list(ui, pv, f, "edges", t, act);
            } else if ty == "shell" {
                ref_list(ui, pv, f, "faces", t, act);
            }
        }
        _ => {
            // the other feature types: their numbers through the parameters (params-part.js targets)
            let targets = PP::targets(&pv.part);
            let prefix = format!("{fname} ");
            egui::Grid::new(("pc-feat", id.clone())).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                for (key, tg) in
                    targets.iter().filter(|(_, tg)| tg.feature == id && tg.field.as_deref().is_none_or(|fl| PP::field_in_use(f, fl)))
                {
                    let label = tg.label.strip_prefix(&prefix).unwrap_or(&tg.label).to_string();
                    ui.label(label);
                    let v = PP::target_value(&pv.part, tg);
                    if let Some(text) = field(ui, &mut pv.edits, key, &show_num(v), 90.0, &tr("ui.params.field.plain")) {
                        *act = cmd("part.param", format!("{key}={text}"));
                    }
                    ui.end_row();
                }
            });
        }
    }
    if ty != "sketch"
        && let Some(sid) = f.get("sketch").and_then(Value::as_str)
    {
        dims_block(ui, pv, sid, "app.dims.sketch", t, act);
    }
    let i = P::index_of(&pv.part, &id).map_or(0.0, cc_part::js::f);
    ui.horizontal_wrapped(|ui| {
        let off = jv::truthy(f.get("suppressed"));
        if ui.button(tr(if off { "app.history.unsuppress" } else { "app.history.suppress" })).clicked() {
            *act = cmd("part.suppress", id.clone());
        }
        if ui.button(tr("app.history.roll_here")).on_hover_text(tr("app.history.roll_here_title")).clicked() {
            *act = cmd("part.rollto", id.clone());
        }
        let up = P::can_move(&pv.part, &id, i - 1.0);
        let down = P::can_move(&pv.part, &id, i + 1.0);
        if ui
            .add_enabled(up.is_ok(), egui::Button::new(tr("app.history.move_up")))
            .on_hover_text(up.err().unwrap_or_else(|| tr("app.history.move_up_title")))
            .clicked()
        {
            *act = cmd("part.move", "up");
        }
        if ui
            .add_enabled(down.is_ok(), egui::Button::new(tr("app.history.move_down")))
            .on_hover_text(down.err().unwrap_or_else(|| tr("app.history.move_down_title")))
            .clicked()
        {
            *act = cmd("part.move", "down");
        }
        if ui.button(RichText::new(tr("app.btn.delete")).color(t.text_strong)).clicked() {
            *act = cmd("part.delete", "");
        }
    });
    ui.horizontal(|ui| {
        ui.label(tr("app.props.name"));
        let key = format!("name:{id}");
        if let Some(text) = field(ui, &mut pv.edits, &key, &fname, 140.0, "") {
            *act = cmd("part.set", format!("{id}:name={text}"));
        }
    });
}

/// The question Delete asks when other features use the one being deleted.
fn confirm_dialog(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    let Some((_, title, text)) = pv.confirm.clone() else { return };
    egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ui.ctx(), |ui| {
        ui.label(RichText::new(text).color(t.text));
        ui.horizontal(|ui| {
            if ui.button(tr("app.btn.cancel")).clicked() {
                pv.confirm = None;
            }
            if ui.button(RichText::new(tr("app.btn.delete_all")).strong()).clicked() {
                *act = cmd("part.delete", "yes");
            }
        });
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
    ui.label(
        RichText::new(cc_part::i18n::t("app.build.rebuilt", &[("ms", cc_part::i18n::F::S(f3(pv.regen_ms)))])).small().color(t.text_dim),
    );
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
    configurations(ui, pv, t, act);
}

/// The Parameters tab's Configurations (params-panel.js): rows with Apply / Update / Delete, a name to save the current
/// values under, Export as a design table.
fn configurations(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    ui.add_space(8.0);
    ui.label(RichText::new(tr("configs.title")).strong().color(t.text_strong));
    let cfgs = pv.configs();
    let p = PP::params_of(&pv.part);
    match &cfgs {
        None => {
            ui.label(RichText::new(tr("configs.empty")).small().color(t.text_dim));
        }
        Some(c) => {
            for r in &c.rows {
                let active = c.active.as_deref() == Some(r.name.as_str());
                ui.horizontal_wrapped(|ui| {
                    let key = format!("cfg:{}", r.name);
                    if let Some(text) = field(ui, &mut pv.edits, &key, &r.name, 110.0, &tr("configs.name_label")) {
                        *act = cmd("part.config", format!("rename:{}={text}", r.name));
                    }
                    if active {
                        let same = cc_part::configs::row_matches(Some(&p), r);
                        ui.label(
                            RichText::new(tr(if same { "configs.active" } else { "configs.active_changed" })).small().color(t.text_strong),
                        );
                    }
                    if ui.small_button(tr("configs.apply")).on_hover_text(tr("configs.apply_title")).clicked() {
                        *act = cmd("part.config", format!("apply:{}", r.name));
                    }
                    if ui.small_button(tr("configs.update")).on_hover_text(tr("configs.update_title")).clicked() {
                        *act = cmd("part.config", format!("update:{}", r.name));
                    }
                    if ui.small_button(tr("configs.delete")).on_hover_text(tr("configs.delete_title")).clicked() {
                        *act = cmd("part.config", format!("delete:{}", r.name));
                    }
                });
                let vals: Vec<String> = r.values.iter().map(|(k, v)| format!("{k} {}", cc_part::expr::format_number(v.number()))).collect();
                ui.label(RichText::new(vals.join(", ")).small().color(t.text_dim));
            }
        }
    }
    ui.horizontal(|ui| {
        let key = "cfg-new".to_string();
        let mut text = pv.edits.get(&key).cloned().unwrap_or_default();
        let r = ui.add(egui::TextEdit::singleline(&mut text).hint_text(tr("configs.new_placeholder")).desired_width(130.0));
        pv.edits.insert(key.clone(), text.clone());
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button(tr("configs.save")).clicked() || enter {
            *act = cmd("part.config", format!("save:{text}"));
            pv.edits.remove(&key);
        }
    });
    if ui.button(tr("configs.export")).clicked() {
        *act = cmd("part.config", "export");
    }
}

/// The 3D view: orbit / pan / zoom (view3d), hover names a face, a click selects it.
fn stage(ui: &mut egui::Ui, rect: Rect, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    if pv.fit_pending {
        pv.view.fit_pending = true;
        pv.fit_pending = false;
    }
    if pv.sk.size_pin.is_none() {
        pv.sk.size = (f64::from(rect.width()), f64::from(rect.height()));
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
        let shift = ui.input(|i| i.modifiers.shift);
        let edge = px.and_then(|(x, y)| pv.edge_at(x, y, 6.0 * ppp));
        let name = match (edge, pv.hover) {
            (Some(e), _) => pv.edges().get(e).map(|_| format!("#{e}")).map(|n| ("part.edge", n)),
            (None, Some(f)) => pv.face(f).map(|r| ("part.face", jv::s(r, "name").to_string())),
            _ => Some(("part.face", String::new())),
        };
        if let Some((id, n)) = name {
            *act = cmd(id, if shift && !n.is_empty() { format!("+{n}") } else { n });
        }
    }
    if resp.hovered() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        if pv.mode == Mode::Pick {
            *act = cmd("part.pick.done", "");
        } else {
            pv.pick_none();
            pv.pending_sketch = false;
        }
    }
    if resp.hovered() && pv.mode == Mode::Model && pv.sel_feature.is_some() && ui.input(|i| i.key_pressed(egui::Key::Delete)) {
        *act = cmd("part.delete", "");
    }
    // picked edges (the pick, Shift+picks, the edges of the fillet / chamfer being picked for) drawn over the view
    let mut marks: Vec<usize> = pv.multi.iter().filter(|m| !m.face).map(|m| m.index).collect();
    if let Some(p) = pv.pick.as_ref().filter(|p| !p.face) {
        marks.push(p.index);
    }
    if pv.mode == Mode::Pick
        && let Some(f) = pv.editing.as_deref().and_then(|id| P::feature_by_id(&pv.part, id))
    {
        marks.extend(jv::arr(f.get("edges")).iter().filter_map(|r| pv.find_edge(r)));
    }
    if !marks.is_empty()
        && let Some(mesh) = pv.regen.mesh.as_ref()
    {
        let segs: Vec<f64> = jv::arr(mesh.get("edgeSegs")).iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect();
        let owner = jv::arr(mesh.get("segEdge"));
        let painter = ui.painter_at(rect);
        let to = |q: (f64, f64)| egui::pos2(rect.min.x + (q.0 / ppp) as f32, rect.min.y + (q.1 / ppp) as f32);
        for (k, sgm) in segs.as_chunks::<6>().0.iter().enumerate() {
            let e = owner.get(k).and_then(Value::as_u64).and_then(|e| usize::try_from(e).ok());
            if e.is_some_and(|e| marks.contains(&e))
                && let (Some(a), Some(b)) = (pv.screen_of([sgm[0], sgm[1], sgm[2]]), pv.screen_of([sgm[3], sgm[4], sgm[5]]))
            {
                painter.line_segment([to(a), to(b)], egui::Stroke::new(3.0, t.accent));
            }
        }
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
