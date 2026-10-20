//! MarkupCraft's Properties panel (markupcraft.js renderProps / markupProps / multiProps / styleControls): with one
//! markup selected, its facts (author, dates, the measured values) and its fields — status (a status reply is logged
//! on its thread), subject, layer, the stamp, a count's group name, its text, its comment — and its style (colour,
//! line width, fill and fill opacity, opacity, text size); with several, the status and style for all of them; with a
//! drawing tool and nothing selected, the style that tool's new markups get. Every change is the JS handler: the same
//! value checks and one `Store::change` with the 0.5 label. The panel and the `markup.style` / `markup.prop` commands
//! call the same methods.

use cc_markup::i18n::{Fill, t, t0};
use cc_markup::model::{STATUSES, Store, status_reply};
use std::collections::HashMap;

use egui::RichText;
use serde_json::{Map, Value, json};

use crate::markuptools::{MarkupTools, draws, text_type};
use crate::theme::Tokens;

/// view.js `mix(a, b, t)`.
pub fn mix(a: &str, b: &str, k: f64) -> String {
    let p = |s: &str| u32::from_str_radix(s.trim_start_matches('#'), 16).ok();
    let (Some(pa), Some(pb)) = (p(a), p(b)) else { return a.to_string() };
    let ch = |v: u32, s: u32| f64::from((v >> s) & 255);
    let c = |s: u32| -> u32 {
        let v = (ch(pa, s) * (1.0 - k) + ch(pb, s) * k).round();
        if v.is_finite() { v.clamp(0.0, 255.0) as u32 } else { 0 }
    };
    format!("#{:02x}{:02x}{:02x}", c(16), c(8), c(0))
}

/// JS `String(n)` for the numbers a style holds.
fn num(x: f64) -> String {
    if x.is_finite() { format!("{x}") } else { String::new() }
}

/// JS `parseFloat`: the longest number at the start (after spaces), else NaN.
fn parse_float(v: &str) -> f64 {
    let s = v.trim_start();
    let mut end = 0;
    let b = s.as_bytes();
    let mut seen_dot = false;
    let mut seen_e = false;
    let mut i = 0;
    while i < b.len() {
        let c = b.get(i).copied().unwrap_or(0);
        let ok = c.is_ascii_digit()
            || ((c == b'-' || c == b'+') && (i == 0 || matches!(b.get(i - 1), Some(b'e' | b'E'))))
            || (c == b'.' && !seen_dot && !seen_e)
            || ((c == b'e' || c == b'E') && !seen_e && end > 0);
        if !ok {
            break;
        }
        seen_dot |= c == b'.';
        seen_e |= c == b'e' || c == b'E';
        i += 1;
        if s.get(..i).and_then(|x| x.parse::<f64>().ok()).is_some() {
            end = i;
        }
    }
    s.get(..end).and_then(|x| x.parse().ok()).unwrap_or(f64::NAN)
}

fn fillable(k: &str) -> bool {
    matches!(k, "rect" | "ellipse" | "polygon" | "cloud" | "cloudrect" | "text" | "callout" | "area")
}

/// A style control's value (styleControls' onchange handlers) → the patch, or None when the JS ignores it.
/// `key` = color, width, fill (on / off / #hex), fillOpacity (0–100), opacity (5–100), fontSize.
pub fn style_value(st: &cc_markup::model::Style, key: &str, v: &str) -> Option<Map<String, Value>> {
    let mut p = Map::new();
    let num = || -> f64 { parse_float(v) };
    match key {
        "color" => {
            p.insert("color".into(), json!(v));
        }
        "width" => {
            let x = num();
            if !(x.is_finite() && x >= 0.0) {
                return None;
            }
            p.insert("width".into(), json!(x.min(72.0)));
        }
        "fill" => {
            let f = match v {
                "off" => Value::Null,
                "on" => json!(st.fill.clone().unwrap_or_else(|| mix(&st.color, "#ffffff", 0.6))),
                hex => json!(hex),
            };
            p.insert("fill".into(), f);
        }
        "fillOpacity" | "opacity" => {
            let x = num();
            if !x.is_finite() {
                return None;
            }
            p.insert(key.into(), json!(x / 100.0));
        }
        "fontSize" => {
            let x = num();
            if !(4.0..=144.0).contains(&x) {
                return None;
            }
            p.insert("fontSize".into(), json!(x));
        }
        _ => return None,
    }
    Some(p)
}

impl MarkupTools {
    /// The selected markups that can be edited (`editable()`).
    fn editable(&self, store: &Store) -> Vec<String> {
        self.sel.iter().filter(|id| store.get(id).is_some_and(|m| !m.locked)).cloned().collect()
    }

    /// A style control changed: the selection (one undo step) or, with a drawing tool and nothing selected, that
    /// tool's style for new markups.
    pub fn style_edit(&mut self, store: &mut Store, key: &str, v: &str) -> Result<(), String> {
        let ids = self.editable(store);
        if ids.is_empty() {
            if !draws(&self.tool) {
                return Err("markup.style: select a markup or pick a drawing tool".into());
            }
            let st = self.style_for(&self.tool);
            let Some(p) = style_value(&st, key, v) else { return Ok(()) };
            // a chest tool in use keeps its own style; else the tool's settings (kept between runs)
            if let Some(ps) = self.preset.as_mut().and_then(Value::as_object_mut) {
                let style = ps.entry("style").or_insert_with(|| Value::Object(Map::new()));
                if let Some(o) = style.as_object_mut() {
                    for (k, x) in p {
                        o.insert(k, x);
                    }
                }
                return Ok(());
            }
            let own = self.styles.entry(self.tool.clone()).or_default();
            for (k, x) in p {
                own.insert(k, x);
            }
            self.prefs_dirty = true;
            return Ok(());
        }
        let first = ids.first().and_then(|i| store.get(i)).map(|m| m.style.clone()).ok_or("markup.style: nothing selected")?;
        let Some(p) = style_value(&first, key, v) else { return Ok(()) };
        // `{ style: { ...m.style, ...patch } }`: tx.update merges a style object into the markup's own
        let mut patch = Map::new();
        patch.insert("style".into(), Value::Object(p));
        let r = store.change(&t0("undo.style"), |tx| {
            for id in &ids {
                tx.update(id, &patch, true);
            }
            Ok(())
        });
        r.map_err(|e| e.to_string())
    }

    /// A field of the selected markup (or, for status, of every selected one): status, subject, layer, text, comment,
    /// label (a count's group name), stamp.
    pub fn prop_edit(&mut self, store: &mut Store, field: &str, v: &str) -> Result<(), String> {
        let ids = self.editable(store);
        if ids.is_empty() {
            if field == "label" && self.tool == "count" {
                let s: String = v.trim().chars().take(60).collect();
                let s = if s.is_empty() { t0("count.default") } else { s };
                match self.preset.as_mut().and_then(Value::as_object_mut) {
                    Some(p) => {
                        p.insert("label".into(), json!(s));
                    }
                    None => {
                        self.count_label = s;
                        self.prefs_dirty = true;
                    }
                }
                return Ok(());
            }
            if field == "stamp" && self.tool == "stamp" {
                match self.preset.as_mut().and_then(Value::as_object_mut) {
                    Some(p) => {
                        p.insert("label".into(), json!(v));
                    }
                    None => {
                        self.stamp = v.to_string();
                        self.prefs_dirty = true;
                    }
                }
                self.message = t("stamp.picked", &[("stamp", Fill::S(v.to_string()))]);
                return Ok(());
            }
            return Err("markup.prop: select a markup first".into());
        }
        if field == "status" {
            if !STATUSES.contains(&v) {
                return Err(format!("markup.prop: unknown status {v}"));
            }
            let author = self.author.clone();
            // patchSel: each markup's status reply is made in selection order (its id comes from the same sequence)
            let mut patches = Vec::new();
            for id in &ids {
                let Some(m) = store.get(id).cloned() else { continue };
                let mut replies: Vec<Value> = m.replies.iter().map(reply_value).collect();
                if let Some(r) = status_reply(&mut store.env, v, &author) {
                    replies.push(reply_value(&r));
                }
                let mut patch = Map::new();
                patch.insert("status".into(), json!(v));
                patch.insert("replies".into(), Value::Array(replies));
                patches.push((id.clone(), patch));
            }
            let r = store.change(&t0("undo.status"), |tx| {
                for (id, patch) in &patches {
                    tx.update(id, patch, true);
                }
                Ok(())
            });
            return r.map_err(|e| e.to_string());
        }
        let [id] = ids.as_slice() else { return Err("markup.prop: select one markup".into()) };
        let m = store.get(id).cloned().ok_or("markup.prop: nothing selected")?;
        let mut patch = Map::new();
        let label = match field {
            "subject" => {
                patch.insert("subject".into(), json!(v.chars().take(120).collect::<String>()));
                t0("undo.subject")
            }
            "layer" => {
                if !v.is_empty() {
                    store.set_layer(v, None, None);
                }
                patch.insert("layer".into(), json!(v));
                t0("undo.layer")
            }
            "text" if text_type(&m.kind) => {
                patch.insert("text".into(), json!(v));
                t0("undo.edit_text")
            }
            "comment" => {
                patch.insert("comment".into(), json!(v));
                t0("undo.comment")
            }
            "label" if m.kind == "count" => {
                let s: String = v.trim().chars().take(60).collect();
                patch.insert("label".into(), json!(if s.is_empty() { t0("count.default") } else { s }));
                t0("undo.count.rename")
            }
            "stamp" if m.kind == "stamp" => {
                let old = cc_markup::model::stamp_color(&m.label);
                let color = if m.style.color == old { cc_markup::model::stamp_color(v).to_string() } else { m.style.color.clone() };
                patch.insert("label".into(), json!(v));
                patch.insert("style".into(), json!({ "color": color }));
                t0("undo.stamp")
            }
            _ => return Err(format!("markup.prop: {field} does not apply to a {}", m.kind)),
        };
        store.change(&label, |tx| Ok(tx.update(id, &patch, true))).map(|_| ()).map_err(|e| e.to_string())
    }
}

/// A reply as the JS object (the key order write_reply uses).
pub(crate) fn reply_value(r: &cc_markup::model::Reply) -> Value {
    let mut o = Map::new();
    o.insert("id".into(), json!(r.id));
    o.insert("author".into(), json!(r.author));
    o.insert("date".into(), json!(r.date));
    o.insert("text".into(), json!(r.text));
    o.insert("kind".into(), json!(r.kind));
    o.insert("status".into(), json!(r.status));
    if let Some(p) = &r.photo {
        o.insert("photo".into(), json!(p));
    }
    Value::Object(o)
}

/// A text field committed when it loses focus (the JS `change` event); a model change replaces what was typed.
fn field(
    edits: &mut HashMap<String, (String, String)>,
    ui: &mut egui::Ui,
    key: &str,
    value: &str,
    label: &str,
    multi: bool,
) -> Option<String> {
    let e = edits.entry(key.to_string()).or_insert_with(|| (value.to_string(), value.to_string()));
    if e.0 != value {
        *e = (value.to_string(), value.to_string());
    }
    let l = ui.label(RichText::new(label).small());
    let w = ui.available_width();
    let r = if multi {
        ui.add(egui::TextEdit::multiline(&mut e.1).desired_rows(3).desired_width(w))
    } else {
        ui.add(egui::TextEdit::singleline(&mut e.1).desired_width(w))
    }
    .labelled_by(l.id);
    (r.lost_focus() && e.1 != e.0).then(|| e.1.clone())
}

/// A slider (0–100 %) committed when the drag ends or a click sets it.
fn percent(
    edits: &mut HashMap<String, (String, String)>,
    ui: &mut egui::Ui,
    key: &str,
    value: f64,
    lo: f64,
    label: &str,
) -> Option<String> {
    let cur = format!("{}", (value * 100.0).round());
    let e = edits.entry(key.to_string()).or_insert_with(|| (cur.clone(), cur.clone()));
    if e.0 != cur {
        *e = (cur.clone(), cur.clone());
    }
    let mut v: f64 = e.1.parse().unwrap_or(100.0);
    let l = ui.label(RichText::new(label).small());
    let r = ui.add(egui::Slider::new(&mut v, lo..=100.0).step_by(5.0).fixed_decimals(0).suffix(" %")).labelled_by(l.id);
    e.1 = format!("{v}");
    ((r.drag_stopped() || (r.changed() && !r.dragged())) && e.1 != e.0).then(|| e.1.clone())
}

impl crate::pdfview::PdfView {
    /// The Properties panel (right of the sheet in MarkupCraft mode).
    pub fn props_panel(&mut self, ui: &mut egui::Ui, tok: &Tokens) {
        let key = self.sheet_key();
        let sel: Vec<_> = self.tools.sel.iter().filter_map(|id| self.store.get(id).cloned()).collect();
        let mut style_set: Vec<(&str, String)> = Vec::new();
        let mut prop_set: Vec<(&str, String)> = Vec::new();
        let head = |ui: &mut egui::Ui, s: String| {
            ui.label(RichText::new(s).strong().color(tok.text_strong));
        };
        let kv = |ui: &mut egui::Ui, k: String, v: String| {
            if !v.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(k).color(tok.text_dim));
                    ui.label(v);
                });
            }
        };
        let style_ui = |edits: &mut HashMap<String, (String, String)>,
                        ui: &mut egui::Ui,
                        st: &cc_markup::model::Style,
                        kinds: &[String],
                        out: &mut Vec<(&str, String)>| {
            let lineless = kinds.iter().all(|k| k == "note");
            let has_text = kinds.iter().any(|k| text_type(k) || matches!(k.as_str(), "measure" | "area" | "count"));
            ui.label(RichText::new(t0("props.color")).small());
            ui.horizontal_wrapped(|ui| {
                for c in crate::markuptools::COLORS.iter().chain(["#f5e000", "#1e8c3a", "#1e48c8", "#c25e00"].iter()) {
                    let col = crate::pdfview::hex_color(c, 1.0).unwrap_or(egui::Color32::GRAY);
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
                    ui.painter().rect_filled(r, 2.0, col);
                    if st.color.eq_ignore_ascii_case(c) {
                        ui.painter().rect_stroke(r, 2.0, egui::Stroke::new(2.0, tok.text_strong), egui::StrokeKind::Outside);
                    }
                    if resp.clicked() {
                        out.push(("color", (*c).to_string()));
                    }
                }
            });
            if let Some(v) = field(edits, ui, "st:color", &st.color, "#rrggbb", false) {
                out.push(("color", v));
            }
            if !lineless {
                let label = if kinds.len() == 1 && kinds.first().is_some_and(|k| k == "highlight") {
                    t0("props.width")
                } else {
                    t0("props.line_width")
                };
                if let Some(v) = field(edits, ui, "st:width", &num(st.width), &label, false) {
                    out.push(("width", v));
                }
            }
            if kinds.iter().any(|k| fillable(k)) {
                let mut on = st.fill.is_some();
                if ui.checkbox(&mut on, t0("props.fill")).changed() {
                    out.push(("fill", if on { "on".into() } else { "off".into() }));
                }
                if let Some(f) = &st.fill {
                    if let Some(v) = field(edits, ui, "st:fill", f, &t0("props.fill_color"), false) {
                        out.push(("fill", v));
                    }
                    if let Some(v) = percent(edits, ui, "st:fop", st.fill_opacity, 0.0, &t0("props.fill_opacity")) {
                        out.push(("fillOpacity", v));
                    }
                }
            }
            if let Some(v) = percent(edits, ui, "st:op", st.opacity, 5.0, &t0("props.opacity")) {
                out.push(("opacity", v));
            }
            if has_text && let Some(v) = field(edits, ui, "st:fs", &num(st.font_size), &t0("props.text_size"), false) {
                out.push(("fontSize", v));
            }
        };
        if let [m] = sel.as_slice() {
            head(ui, if m.locked { t0("props.h.locked") } else { t0(&format!("type.{}", m.kind)) });
            kv(ui, t0("props.author"), if m.author.is_empty() { t0("props.author.none") } else { m.author.clone() });
            kv(ui, t0("props.created"), m.created.replace('T', " ").trim_end_matches('Z').to_string());
            if !m.modified.is_empty() && m.modified != m.created {
                kv(ui, t0("props.modified"), m.modified.replace('T', " ").trim_end_matches('Z').to_string());
            }
            for (k, v) in self.value_rows(m) {
                kv(ui, k, v);
            }
            if m.locked {
                kv(ui, t0("props.subject"), m.subject.clone());
                kv(ui, t0("props.comment"), m.comment.clone());
                ui.label(RichText::new(t0("props.locked.note")).color(tok.text_dim));
                let m = m.clone();
                self.thread_section(ui, tok, &m);
                return;
            }
            let l = ui.label(RichText::new(t0("props.status")).small());
            egui::ComboBox::new("mc-p-status", "")
                .selected_text(cc_markup::model::status_name(&m.status))
                .show_ui(ui, |ui| {
                    for st in STATUSES {
                        if ui.selectable_label(m.status == st, cc_markup::model::status_name(st)).clicked() && m.status != st {
                            prop_set.push(("status", st.to_string()));
                        }
                    }
                })
                .response
                .labelled_by(l.id);
            if let Some(v) = field(&mut self.edits, ui, &format!("{}:subject", m.id), &m.subject, &t0("props.subject"), false) {
                prop_set.push(("subject", v));
            }
            let l = ui.label(RichText::new(t0("props.layer")).small());
            let shown = if m.layer.is_empty() { t0("layer.none") } else { m.layer.clone() };
            egui::ComboBox::new("mc-p-layer", "")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(m.layer.is_empty(), t0("layer.none")).clicked() && !m.layer.is_empty() {
                        prop_set.push(("layer", String::new()));
                    }
                    for n in self.store.layer_names() {
                        if ui.selectable_label(m.layer == n, &n).clicked() && m.layer != n {
                            prop_set.push(("layer", n));
                        }
                    }
                })
                .response
                .labelled_by(l.id);
            if let Some(v) = field(&mut self.edits, ui, "new-layer", "", &t0("layer.new_option"), false)
                && !v.trim().is_empty()
            {
                prop_set.push(("layer", v));
            }
            if m.kind == "count"
                && let Some(v) = field(&mut self.edits, ui, &format!("{}:label", m.id), &m.label, &t0("props.group"), false)
            {
                prop_set.push(("label", v));
            }
            if m.kind == "stamp" {
                let l = ui.label(RichText::new(t0("stamp.label")).small());
                egui::ComboBox::new("mc-p-stamp", "")
                    .selected_text(cc_markup::model::stamp_label(&m.label))
                    .show_ui(ui, |ui| {
                        for s in cc_markup::model::stamps() {
                            if ui.selectable_label(m.label == s, &s).clicked() && m.label != s {
                                prop_set.push(("stamp", s));
                            }
                        }
                    })
                    .response
                    .labelled_by(l.id);
            }
            if text_type(&m.kind)
                && let Some(v) = field(&mut self.edits, ui, &format!("{}:text", m.id), &m.text, &t0("props.text"), true)
            {
                prop_set.push(("text", v));
            }
            let cl = if m.kind == "note" { t0("props.note") } else { t0("props.comment") };
            if let Some(v) = field(&mut self.edits, ui, &format!("{}:comment", m.id), &m.comment, &cl, true) {
                prop_set.push(("comment", v));
            }
            ui.add_space(4.0);
            head(ui, t0("props.style"));
            style_ui(&mut self.edits, ui, &m.style, std::slice::from_ref(&m.kind), &mut style_set);
            ui.add_space(4.0);
            let mc = m.clone();
            self.thread_section(ui, tok, &mc);
            if ui.button(t0("chest.save.one")).clicked() {
                self.dlg = Some(crate::markupedit::Dialog::ChestName { name: self.chest_name_for(&mc), error: String::new() });
            }
            if ui.button(t0("menu.delete")).clicked() {
                self.tools.delete_selected(&mut self.store);
            }
        } else if sel.len() > 1 {
            head(ui, t("save.markups", &[("n", Fill::N(sel.len() as f64))]));
            let ed: Vec<_> = sel.iter().filter(|m| !m.locked).collect();
            let mut kinds: Vec<String> = Vec::new();
            for m in &ed {
                if !kinds.contains(&m.kind) {
                    kinds.push(m.kind.clone());
                }
            }
            let names: Vec<String> = kinds.iter().map(|k| t0(&format!("type.{k}"))).collect();
            let locked = sel.len() - ed.len();
            let note = if locked > 0 {
                t("multi.types.locked", &[("types", Fill::S(names.join(", "))), ("n", Fill::N(locked as f64))])
            } else {
                t("multi.types", &[("types", Fill::S(names.join(", ")))])
            };
            ui.label(RichText::new(note).color(tok.text_dim));
            if let Some(first) = ed.first() {
                let common = ed.iter().all(|m| m.status == first.status).then(|| first.status.clone());
                let l = ui.label(RichText::new(t0("props.status")).small());
                egui::ComboBox::new("mc-p-status-n", "")
                    .selected_text(common.as_deref().map_or_else(|| t0("multi.mixed"), cc_markup::model::status_name))
                    .show_ui(ui, |ui| {
                        for st in STATUSES {
                            if ui.selectable_label(common.as_deref() == Some(st), cc_markup::model::status_name(st)).clicked() {
                                prop_set.push(("status", st.to_string()));
                            }
                        }
                    })
                    .response
                    .labelled_by(l.id);
                head(ui, t0("props.style"));
                let st = first.style.clone();
                style_ui(&mut self.edits, ui, &st, &kinds, &mut style_set);
            }
        } else if draws(&self.tools.tool) {
            let tool = self.tools.tool.clone();
            let pname = self.tools.preset_str("name");
            head(ui, t("props.h.tool", &[("tool", Fill::S(if pname.is_empty() { t0(&format!("type.{tool}")) } else { pname.clone() }))]));
            if tool == "count" {
                let pl = self.tools.preset_str("label");
                let cur = if !pl.is_empty() {
                    pl
                } else if self.tools.count_label.is_empty() {
                    t0("count.default")
                } else {
                    self.tools.count_label.clone()
                };
                if let Some(v) = field(&mut self.edits, ui, "tool:count", &cur, &t0("props.group"), false) {
                    prop_set.push(("label", v));
                }
            }
            let mut pic_act: Option<&str> = None;
            if tool == "stamp" {
                let preset = self.tools.preset.is_some();
                let pl = self.tools.preset_str("label");
                let cur = match self.tools.stamp.as_str() {
                    _ if preset => pl,
                    crate::markupchest::PIC => t0("stamp.picture"),
                    crate::markupchest::SIG => t0("stamp.signature"),
                    s => cc_markup::model::stamp_label(s),
                };
                let l = ui.label(RichText::new(t0("stamp.label")).small());
                egui::ComboBox::new("mc-t-stamp", "")
                    .selected_text(cur.clone())
                    .show_ui(ui, |ui| {
                        for s in cc_markup::model::stamps() {
                            if ui.selectable_label(cur == s, &s).clicked() {
                                prop_set.push(("stamp", s));
                            }
                        }
                        if !preset {
                            if ui.selectable_label(false, t0("stamp.picture")).clicked() {
                                pic_act = Some("picture");
                            }
                            if ui.selectable_label(false, t0("stamp.signature")).clicked() {
                                pic_act = Some("signature");
                            }
                        }
                    })
                    .response
                    .labelled_by(l.id);
                if !preset && let Some((src, label)) = self.tools.current_picture() {
                    ui.label(RichText::new(t0("image.preview")).small());
                    if let Some(tex) = self.picture_texture(ui.ctx(), &src) {
                        let sz = tex.size_vec2();
                        let k = (200.0 / sz.x.max(1.0)).min(100.0 / sz.y.max(1.0)).min(1.0);
                        ui.image((tex.id(), sz * k)).on_hover_text(label);
                    }
                    let again = if self.tools.stamp == crate::markupchest::SIG { "image.signature.again" } else { "image.pick.again" };
                    if ui.button(t0(again)).clicked() {
                        pic_act = Some(if self.tools.stamp == crate::markupchest::SIG { "sign-again" } else { "picture" });
                    }
                }
            }
            let st = self.tools.style_for(&tool);
            style_ui(&mut self.edits, ui, &st, std::slice::from_ref(&tool), &mut style_set);
            let note = if pname.is_empty() {
                t("props.tool.note", &[("type", Fill::S(t0(&format!("type.{tool}.lower"))))])
            } else {
                t("props.tool.preset", &[("tool", Fill::S(pname))])
            };
            ui.label(RichText::new(note).color(tok.text_dim));
            match pic_act {
                Some("picture") => self.want_pick = Some("picture"),
                Some("signature") => match self.tools.signature.clone() {
                    Some(src) => {
                        let _ = self.set_signature(&src);
                    }
                    None => self.dlg = Some(crate::markupedit::Dialog::Signature { strokes: Vec::new(), error: String::new() }),
                },
                Some("sign-again") => self.dlg = Some(crate::markupedit::Dialog::Signature { strokes: Vec::new(), error: String::new() }),
                _ => {}
            }
        } else {
            head(ui, t0("props.h.sheet"));
            let p = self.cur.pages.get(self.page).cloned();
            let info = self.cur.info(&self.store, self.page);
            kv(ui, t0("props.sheet"), if info.name.is_empty() { info.title.clone() } else { info.name.clone() });
            kv(ui, t0("props.sheet_number"), info.sheet_no.clone());
            kv(ui, t0("props.file"), self.cur.name.clone());
            kv(
                ui,
                t0("props.page"),
                t("props.page.of", &[("i", Fill::N((self.page + 1) as f64)), ("n", Fill::N(self.cur.pages.len() as f64))]),
            );
            if let Some(p) = &p {
                let (w, h) = if (self.page_rot() % 180.0) != 0.0 { (p.height, p.width) } else { (p.width, p.height) };
                kv(
                    ui,
                    t0("props.paper"),
                    t("props.paper.in", &[("w", Fill::S(format!("{:.2}", w / 72.0))), ("h", Fill::S(format!("{:.2}", h / 72.0)))]),
                );
            }
            kv(ui, t0("props.scale"), self.store.calibrations.get(&key).map_or_else(|| t0("status.scale.none"), |c| c.label.clone()));
            kv(ui, t0("tab.list"), self.store.on_sheet(&key).len().to_string());
            if ui.button(t0("props.rename")).clicked() {
                let (number, title) = self.rename_start();
                self.dlg = Some(crate::markupedit::Dialog::Rename { number, title });
            }
        }
        for (k, v) in style_set {
            let _ = self.tools.style_edit(&mut self.store, k, &v);
        }
        for (k, v) in prop_set {
            if let Err(e) = self.prop(k, &v) {
                self.tools.message = e;
            }
        }
    }

    /// A Properties field set (the clock is ticked first: a status reply carries the date).
    pub fn prop(&mut self, k: &str, v: &str) -> Result<(), String> {
        self.tick();
        self.tools.prop_edit(&mut self.store, k, v)
    }
}
