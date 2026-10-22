//! CadCraft's Properties panel for the drafting editor (props.js on cc-engine's propdefs / propexpr port): the group
//! picker when several kinds of object are selected ("All (12)", "Line (8)"), then the fields of the group by section
//! — layer, colour, linetype, lineweight, transparency and the rest of General, then Geometry, Text, Pattern,
//! dimension style … Each change is ONE undo step through the `props.set` command (`name` = "key=value"); number
//! fields read units, feet and inches and `=expressions` as 0.5 does; a refused entry shows its reason and changes
//! nothing. Opened by PROPERTIES (Ctrl+1), folded by PROPERTIESCLOSE; with nothing selected it shows what new
//! objects get (the current layer, colour, linetype, lineweight).
//!
//! Not here (yet): Quick Properties by the cursor (QPMODE), the point pick buttons (_PROPPICK), the Attributes and
//! Library sections, the sheet and view sections, the vertex browser's arrows beyond previous / next.

use cc_engine::props::{PCtx, PVal, ReadCtx, apply_value, defs_for, edit_text, group_label, groups_of, read_value};
use egui::RichText;
use serde_json::Value;

use crate::{Params, UiApp};

/// The panel's state: open, the group shown, the polyline vertex, and the text being typed in each field.
#[derive(Default)]
pub struct CadProps {
    pub open: bool,
    pub group: Option<String>,
    pub vertex: i64,
    /// field key → text being edited (absent = shows the value)
    pub edits: std::collections::HashMap<String, String>,
    /// the reason the last entry was refused
    pub note: String,
    /// the selection the panel last showed (a new selection drops the edits and the vertex)
    seen: Vec<u64>,
}

/// UI commands of the panel.
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("props.open", "Properties", "Ctrl+1", "{} → opens the Properties panel (the PROPERTIES command does the same)"),
    ("props.close", "Close Properties", "", "{}"),
    ("props.group", "Properties: group", "", "{name: \"all\" | a type (line, circle, …)} → the group the panel shows"),
    ("props.vertex", "Properties: vertex", "", "{index: n} → the polyline vertex the panel shows (0 = the first)"),
    (
        "props.units",
        "Properties: drawing units",
        "",
        "{name: \"\" | millimeters | centimeters | meters | inches | feet} → what one drawing unit stands for; with solids the 3D model is scaled to stay on the drawing (one undo step)",
    ),
    (
        "props.set",
        "Properties: set a field",
        "",
        "{name: \"<key>=<value>\"} → sets the field on every object of the group (one undo step); number fields read text as typed (12'6\", 900mm, =2*45); choices take the value; colour: \"\" by layer, 0 by block, 1–255, #rrggbb",
    ),
];

fn lang() -> cc_engine::Lang {
    crate::edit::lang_en_pub()
}

/// A typed value for a non-number field (what the panel's selects hand over).
fn control_value(kind: &str, s: &str) -> Value {
    match kind {
        "bool" => Value::Bool(matches!(s, "true" | "1" | "yes" | "on")),
        "color" => {
            if s.is_empty() || s.eq_ignore_ascii_case("bylayer") {
                Value::Null
            } else if s == "none" {
                Value::String("none".into())
            } else if s.starts_with('#') {
                Value::String(s.into())
            } else {
                s.parse::<i64>().map_or(Value::Null, Value::from)
            }
        }
        "lt" | "mat" => {
            if s.is_empty() {
                Value::Null
            } else {
                Value::String(s.into())
            }
        }
        "lw" => {
            if s.is_empty() {
                Value::Null
            } else {
                s.parse::<f64>().map_or(Value::Null, cc_cad::num)
            }
        }
        "tr" => {
            if s.is_empty() {
                Value::Null
            } else if s == "ByBlock" {
                Value::String("ByBlock".into())
            } else {
                s.parse::<f64>().map_or(Value::Null, |v| cc_cad::num(v.round().clamp(0.0, 90.0)))
            }
        }
        _ => Value::String(s.into()),
    }
}

/// `props.*` commands.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    match id {
        "props.open" => app.cadprops.open = true,
        "props.close" => app.cadprops.open = false,
        "props.group" => app.cadprops.group = params.name.clone(),
        "props.vertex" => app.cadprops.vertex = i64::try_from(params.index.unwrap_or(0)).unwrap_or(0),
        "props.units" => set_units(app, params.name.as_deref().unwrap_or(""))?,
        "props.set" => {
            let spec = params.name.clone().ok_or("props.set: name = \"<key>=<value>\" is required")?;
            let (key, raw) = spec.split_once('=').ok_or("props.set: name = \"<key>=<value>\"")?;
            set_field(app, key, raw)?;
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// Set one field on the group the panel shows (a number field reads `raw` as typed; others take it as the value).
pub fn set_field(app: &mut UiApp, key: &str, raw: &str) -> Result<(), String> {
    let lang = lang();
    let vertex = app.cadprops.vertex;
    let want = app.cadprops.group.clone();
    let ed = app.editor.as_mut().ok_or("props.set: no drawing is open for editing")?;
    let sel = ed.hs.borrow().selection.clone();
    let (gkey, gids, value) = {
        let doc = ed.eng.doc();
        let ents: Vec<cc_cad::Entity> = sel.iter().filter_map(|id| doc.get(*id).cloned()).collect();
        if ents.is_empty() {
            return Err("props.set: nothing is selected".into());
        }
        let groups = groups_of(&ents, &lang);
        let g = want.as_ref().and_then(|k| groups.iter().find(|g| &g.key == k)).or(groups.first()).ok_or("props.set: no group")?;
        let gents: Vec<cc_cad::Entity> = g.ids.iter().filter_map(|id| doc.get(*id).cloned()).collect();
        let c = PCtx { doc: &doc, ents: &gents, vertex, lang: &lang };
        let defs = defs_for(&g.key, &c);
        let d = defs.iter().find(|d| d.key == key).ok_or(format!("props.set: the group has no field {key}"))?;
        let value = if matches!(d.kind, "len" | "ang" | "num" | "int" | "area") {
            let rc = ReadCtx {
                lunits: doc.meta.get("lunits").and_then(Value::as_f64),
                units: doc.meta.get("units").and_then(Value::as_str).unwrap_or("").to_string(),
                paper: if doc.layout().is_some() && !d.model { doc.layout().and_then(|l| l.unit()).map(str::to_string) } else { None },
                gt0: d.gt0,
                min: d.min,
                max: d.max,
            };
            read_value(raw, if d.kind == "area" { "num" } else { d.kind }, &rc, &lang).map(cc_cad::num)?
        } else {
            control_value(d.kind, raw)
        };
        (g.key.clone(), g.ids.clone(), value)
    };
    let r = {
        let mut doc = ed.eng.doc_mut();
        apply_value(&mut doc, key, &gkey, &gids, &value, vertex, &lang)
    };
    app.sync_editor();
    if let Some(e) = r.error {
        app.cadprops.note = e.clone();
        return Err(e);
    }
    app.cadprops.note = if r.locked > 0 { format!("{} on locked layers left as they were", r.locked) } else { String::new() };
    Ok(())
}

/// The units the panel offers (props.js unitsSelect).
const UNITS: [&str; 6] = ["", "millimeters", "centimeters", "meters", "inches", "feet"];

/// props.js unitsSelect's change: model3d.js setUnits, the status line, and the note about the scaled 3D model.
pub fn set_units(app: &mut UiApp, units: &str) -> Result<(), String> {
    if !UNITS.contains(&units) {
        return Err(format!("props.units: {units} is not one of the drawing units (\"\", millimeters, centimeters, meters, inches, feet)"));
    }
    let l = lang();
    let ed = app.editor.as_mut().ok_or("props.units: no drawing is open for editing")?;
    let (scaled, ratio) = cc_engine::cmd::solid3d::set_units(ed.eng.shared(), units);
    let name = l.s(&format!("props.units.{}", if units.is_empty() { "none" } else { units }));
    app.message = l.t(if scaled { "props.units.set.scaled" } else { "props.units.set" }, &[("units", name.clone())]);
    if scaled {
        let k: f64 = format!("{ratio:.5e}").parse().unwrap_or(ratio);
        let m = l.t("props.units.set.3d", &[("units", name), ("k", cc_cad::js::num_to_string(k))]);
        ed.hs.borrow_mut().push_log(&m);
    }
    app.sync_editor();
    Ok(())
}

/// `fmt(n)` of ui-util.js: 4 decimals, trailing zeros dropped.
fn fmt4(n: f64) -> String {
    cc_engine::input::fmt(n)
}

/// How a value reads in its field (props.js `show`).
fn show(kind: &str, v: &Value, doc: &cc_cad::Doc) -> String {
    let n = || v.as_f64();
    match kind {
        "len" | "ro-len" => n().map_or_else(String::new, |x| {
            let (code, prec) = cc_cad_geom::units::units_of(Some(&doc.meta));
            cc_cad_geom::units::format_length(x, f64::from(code), Some(&Value::from(prec)))
        }),
        "area" | "ro-area" => n().map_or_else(String::new, |x| {
            let (code, prec) = cc_cad_geom::units::units_of(Some(&doc.meta));
            let l = lang();
            cc_cad_geom::units::format_area(x, f64::from(code), Some(&Value::from(prec)), &|k| l.has(k).then(|| l.s(k)))
        }),
        "ang" => n().map_or_else(String::new, |x| format!("{}°", fmt4(x))),
        "num" | "int" => n().map_or_else(String::new, fmt4),
        _ => cc_engine::props::value_text(v),
    }
}

fn color_label(v: &Value, l: &cc_engine::Lang) -> String {
    match v {
        Value::Null => l.s("props.color.bylayer"),
        Value::String(s) if s == "none" => l.s("props.color.none"),
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            let c = n.as_f64().unwrap_or(f64::NAN);
            if c == 0.0 {
                return l.s("props.color.byblock");
            }
            let names = [
                "app.aci.red",
                "app.aci.yellow",
                "app.aci.green",
                "app.aci.cyan",
                "app.aci.blue",
                "app.aci.magenta",
                "app.ink_black_white",
                "app.aci.gray",
                "app.light_gray",
            ];
            match usize::try_from(n.as_i64().unwrap_or(-1)).ok().and_then(|i| i.checked_sub(1)).and_then(|i| names.get(i)) {
                Some(k) => format!("{} ({})", l.s(k), js_num(c)),
                None => format!("Colour {}", js_num(c)),
            }
        }
        v => v.to_string(),
    }
}
fn js_num(x: f64) -> String {
    cc_cad::js::num_to_string(x)
}

/// The choices a select field offers: (value text for `props.set`, label).
fn select_choices(d: &cc_engine::props::Def, c: &PCtx, l: &cc_engine::Lang) -> Vec<(String, String)> {
    match d.kind {
        "layer" => {
            let mut v: Vec<String> = c.doc.layers.keys().cloned().collect();
            v.sort();
            v.into_iter().map(|n| (n.clone(), n)).collect()
        }
        "color" => {
            let mut v = Vec::new();
            if d.none {
                v.push(("none".to_string(), l.s("props.color.none")));
            }
            v.push((String::new(), l.s("props.color.bylayer")));
            v.push(("0".into(), l.s("props.color.byblock")));
            for i in 1..=9i64 {
                v.push((i.to_string(), color_label(&Value::from(i), l)));
            }
            v
        }
        "lt" => {
            let mut v = vec![
                (String::new(), l.s("lt.bylayer")),
                ("ByBlock".into(), l.s("lt.byblock")),
                ("Continuous".into(), l.s("lt.continuous")),
            ];
            for n in c.doc.linetypes.keys() {
                if !n.eq_ignore_ascii_case("continuous") {
                    v.push((n.clone(), n.clone()));
                }
            }
            for n in cc_cad_geom::linetypes::standard_names() {
                if !v.iter().any(|(k, _)| k.eq_ignore_ascii_case(&n)) {
                    v.push((n.clone(), n));
                }
            }
            v
        }
        "lw" => {
            let mut v = vec![(String::new(), l.s("lt.bylayer")), ("-2".into(), l.s("lt.byblock")), ("-3".into(), l.s("lw.default"))];
            for w in cc_cad_geom::linetypes::LINEWEIGHTS {
                v.push((js_num(w), l.t("lw.mm", &[("mm", cc_cad::js::to_fixed(w, 2))])));
            }
            v
        }
        "tr" => {
            let mut v = vec![(String::new(), l.s("props.tr.bylayer")), ("ByBlock".into(), l.s("props.tr.byblock"))];
            for n in (0..=90).step_by(10) {
                v.push((n.to_string(), l.t("props.tr.pct", &[("n", n.to_string())])));
            }
            v
        }
        "mat" => vec![
            (String::new(), l.s("props.mat.bylayer")),
            ("ByBlock".into(), l.s("props.mat.byblock")),
            ("Global".into(), l.s("props.mat.global")),
        ],
        _ => d
            .choices
            .as_ref()
            .map(|f| f(c).into_iter().map(|(v, lab)| (cc_engine::props::value_text(&v), lab)).collect())
            .unwrap_or_default(),
    }
}

fn value_key(kind: &str, v: &Value) -> String {
    match (kind, v) {
        (_, Value::Null) => String::new(),
        (_, Value::Bool(b)) => b.to_string(),
        _ => cc_engine::props::value_text(v),
    }
}

/// The panel (a right-hand side panel while a drawing is open for editing).
pub fn panel(app: &mut UiApp, ui: &mut egui::Ui) {
    let t = app.tokens;
    let l = lang();
    let mut act: Option<(String, String)> = None;
    let mut pick_group: Option<String> = None;
    let mut vertex_to: Option<i64> = None;
    let mut close = false;
    let mut pick_units: Option<String> = None;
    let frame = egui::Frame::new().fill(t.panel).inner_margin(6);
    egui::Panel::right("cad_props").default_size(270.0).size_range(200.0..=440.0).resizable(true).frame(frame).show(ui, |ui| {
        let Some(ed) = app.editor.as_ref() else { return };
        let sel = ed.hs.borrow().selection.clone();
        if sel != app.cadprops.seen {
            app.cadprops.seen = sel.clone();
            app.cadprops.edits.clear();
            app.cadprops.vertex = 0;
            app.cadprops.note.clear();
        }
        let doc = ed.eng.doc();
        ui.horizontal(|ui| {
            ui.label(RichText::new(l.s("props.title")).strong().color(t.text_strong));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").on_hover_text("PROPERTIESCLOSE").clicked() {
                    close = true;
                }
            });
        });
        ui.separator();
        let ents: Vec<cc_cad::Entity> = sel.iter().filter_map(|id| doc.get(*id).cloned()).collect();
        if ents.is_empty() {
            // nothing selected: what new objects get
            ui.label(RichText::new(l.s("props.sec.current")).color(t.text_dim));
            let cur = &doc.current;
            egui::Grid::new("props_current").num_columns(2).striped(false).show(ui, |ui| {
                ui.label(l.s("props.f.layer"));
                ui.label(&cur.layer);
                ui.end_row();
                ui.label(l.s("props.f.color"));
                ui.label(color_label(&cur.color.to_value(), &l));
                ui.end_row();
                ui.label(l.s("props.f.lt"));
                ui.label(cur.linetype.clone().unwrap_or_else(|| l.s("lt.bylayer")));
                ui.end_row();
                ui.label(l.s("props.f.lw"));
                ui.label(cur.lineweight.map_or_else(|| l.s("lt.bylayer"), |w| l.t("lw.mm", &[("mm", cc_cad::js::to_fixed(w, 2))])));
                ui.end_row();
                // props.js unitsSelect: what one drawing unit stands for (with solids, the 3D model follows)
                let cur_u = doc.meta.get("units").filter(|v| cc_cad::js::truthy(Some(v))).map(cc_cad::js::to_js_string).unwrap_or_default();
                let name = |u: &str| l.s(&format!("props.units.{}", if u.is_empty() { "none" } else { u }));
                let lab = ui.label(l.s("props.cur.units")).on_hover_text(l.s("props.cur.units.title"));
                egui::ComboBox::new("props_units", "")
                    .selected_text(if UNITS.contains(&cur_u.as_str()) { name(&cur_u) } else { cur_u.clone() })
                    .show_ui(ui, |ui| {
                        for u in UNITS {
                            if ui.selectable_label(u == cur_u, name(u)).clicked() && u != cur_u {
                                pick_units = Some(u.to_string());
                            }
                        }
                    })
                    .response
                    .labelled_by(lab.id);
                ui.end_row();
            });
            return;
        }
        let groups = groups_of(&ents, &l);
        let gkey = app
            .cadprops
            .group
            .clone()
            .filter(|k| groups.iter().any(|g| &g.key == k))
            .or_else(|| groups.first().map(|g| g.key.clone()))
            .unwrap_or_default();
        if groups.len() > 1 {
            let cur_label = groups.iter().find(|g| g.key == gkey).map(|g| group_label(g, &l)).unwrap_or_default();
            egui::ComboBox::new("props_group", "").selected_text(cur_label).show_ui(ui, |ui| {
                for g in &groups {
                    if ui.selectable_label(g.key == gkey, group_label(g, &l)).clicked() {
                        pick_group = Some(g.key.clone());
                    }
                }
            });
        } else if let Some(g) = groups.first() {
            ui.label(RichText::new(group_label(g, &l)).color(t.text));
        }
        let gids: Vec<u64> = groups.iter().find(|g| g.key == gkey).map(|g| g.ids.clone()).unwrap_or_default();
        let gents: Vec<cc_cad::Entity> = gids.iter().filter_map(|id| doc.get(*id).cloned()).collect();
        let c = PCtx { doc: &doc, ents: &gents, vertex: app.cadprops.vertex, lang: &l };
        let defs = defs_for(&gkey, &c);
        if !app.cadprops.note.is_empty() {
            ui.label(RichText::new(&app.cadprops.note).color(egui::Color32::from_rgb(0xe0, 0x80, 0x60)));
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut sec = "";
            for d in &defs {
                if d.sec != sec {
                    sec = d.sec;
                    ui.add_space(4.0);
                    ui.label(RichText::new(l.s(&format!("props.sec.{sec}"))).strong().color(t.text_dim));
                }
                let v = cc_engine::props::value_of(d, &c);
                let label = d.label.clone().unwrap_or_else(|| d.key.clone());
                ui.horizontal(|ui| {
                    let lab = ui.add_sized([104.0, 18.0], egui::Label::new(RichText::new(&label).color(t.text)).truncate());
                    if let Some(tip) = d.title {
                        lab.clone().on_hover_text(l.s(tip));
                    }
                    let varies = v == PVal::Varies;
                    let val = match &v {
                        PVal::V(x) => x.clone(),
                        PVal::Varies => Value::Null,
                    };
                    let ro = d.set.is_none();
                    // what is left of the row after the label, kept off the panel's right edge
                    let w = (ui.available_width() - 8.0).max(40.0);
                    match d.kind {
                        "ro" | "ro-len" | "ro-area" => {
                            let txt = if varies { l.s("props.varies") } else { show(d.kind, &val, &doc) };
                            ui.label(RichText::new(txt).color(t.text_dim)).labelled_by(lab.id);
                        }
                        "bool" => {
                            let mut on = val.as_bool().unwrap_or(false);
                            let r = ui.checkbox(&mut on, if varies { l.s("props.varies") } else { String::new() }).labelled_by(lab.id);
                            if r.changed() && !ro {
                                act = Some((d.key.clone(), on.to_string()));
                            }
                        }
                        "vertex" => {
                            let n = val.as_f64().unwrap_or(1.0);
                            if ui.small_button("◀").clicked() {
                                vertex_to = Some(app.cadprops.vertex.saturating_sub(1).max(0));
                            }
                            ui.label(js_num(n));
                            if ui.small_button("▶").clicked() {
                                vertex_to = Some(app.cadprops.vertex.saturating_add(1));
                            }
                        }
                        "len" | "ang" | "num" | "int" | "area" | "text" | "textarea" => {
                            let shown = if varies {
                                String::new()
                            } else if matches!(d.kind, "text" | "textarea") {
                                cc_engine::props::value_text(&val)
                            } else {
                                show(d.kind, &val, &doc)
                            };
                            let editing = app.cadprops.edits.contains_key(&d.key);
                            let mut buf = app.cadprops.edits.get(&d.key).cloned().unwrap_or_else(|| shown.clone());
                            let te = if d.kind == "textarea" {
                                egui::TextEdit::multiline(&mut buf).desired_rows(2)
                            } else {
                                egui::TextEdit::singleline(&mut buf)
                            };
                            let te =
                                te.desired_width(w).hint_text(if varies { l.s("props.varies") } else { String::new() }).interactive(!ro);
                            let r = ui.add(te).labelled_by(lab.id);
                            if r.gained_focus() && !editing && matches!(d.kind, "len" | "ang" | "num" | "int" | "area") {
                                // full precision while editing (propexpr.js editText)
                                buf = val.as_f64().map(edit_text).unwrap_or(buf);
                            }
                            if r.has_focus() || r.changed() {
                                app.cadprops.edits.insert(d.key.clone(), buf.clone());
                            }
                            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            let commit = enter || (r.lost_focus() && d.kind == "textarea");
                            if commit && !ro && buf != shown {
                                act = Some((d.key.clone(), buf.clone()));
                            }
                            if r.lost_focus() {
                                app.cadprops.edits.remove(&d.key);
                            }
                        }
                        _ => {
                            // selects: layer, colour, linetype, lineweight, transparency, material, choices
                            let choices = select_choices(d, &c, &l);
                            let key = value_key(d.kind, &val);
                            let current = if varies {
                                l.s("props.varies")
                            } else if d.kind == "color" {
                                color_label(&val, &l)
                            } else {
                                choices
                                    .iter()
                                    .find(|(k, _)| k.eq_ignore_ascii_case(&key))
                                    .map_or_else(|| key.clone(), |(_, lab)| lab.clone())
                            };
                            egui::ComboBox::new(("props_sel", &d.key), "").width((w - 16.0).max(30.0)).selected_text(current).show_ui(
                                ui,
                                |ui| {
                                    for (k, lab) in &choices {
                                        if ui.selectable_label(!varies && k.eq_ignore_ascii_case(&key), lab).clicked() && !ro {
                                            act = Some((d.key.clone(), k.clone()));
                                        }
                                    }
                                },
                            );
                        }
                    }
                });
            }
        });
    });
    if close {
        app.run_ui_pub("props.close", &Params::default());
    }
    if let Some(u) = pick_units {
        app.run_ui_pub("props.units", &Params { name: Some(u), ..Default::default() });
    }
    if let Some(g) = pick_group {
        app.run_ui_pub("props.group", &Params { name: Some(g), ..Default::default() });
    }
    if let Some(v) = vertex_to {
        app.run_ui_pub("props.vertex", &Params { index: usize::try_from(v).ok(), ..Default::default() });
    }
    if let Some((k, raw)) = act {
        app.run_ui_pub("props.set", &Params { name: Some(format!("{k}={raw}")), ..Default::default() });
    }
}
