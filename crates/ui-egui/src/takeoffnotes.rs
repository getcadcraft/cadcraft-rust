//! TakeoffCraft's typical groups and markups (M9.21 fills this in); for now the bid area field of the selected marks.

use cc_takeoff::geom::P;
use cc_takeoff::i18n::t;
use egui::{Pos2, RichText};
use serde_json::Value;

use crate::takeofftools::TakeoffMode;

fn s(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(x)) => x.clone(),
        _ => String::new(),
    }
}

/// A markup waiting for its text.
#[derive(Debug, Clone)]
pub struct NoteAsk {
    pub kind: String,
}

/// The New typical group window.
#[derive(Debug, Clone)]
pub struct GroupAsk {
    pub ids: Vec<String>,
}

impl TakeoffMode {
    pub(crate) fn note_at(&mut self, _sheet: &str, _p: P) {}
    pub(crate) fn box_note(&mut self, _sheet: &str, _kind: &str, _a: P, _b: P) {}
    pub fn note_hit(&self, _sheet: &str, _p: P) -> Option<Value> {
        None
    }
    pub(crate) fn draw_notes(&self, _painter: &egui::Painter, _sheet: &str, _at: &dyn Fn(f64, f64) -> Pos2) {}
    pub(crate) fn draw_groups(&self, _painter: &egui::Painter, _sheet: &str, _at: &dyn Fn(f64, f64) -> Pos2) {}

    /// The bid area of the given marks (areaField): "—" when they differ.
    pub(crate) fn area_group_fields(&mut self, ui: &mut egui::Ui, ids: &[String], set: &mut Vec<(String, String)>) {
        let marks: Vec<Value> = ids.iter().filter_map(|i| self.tk.mark(i)).cloned().collect();
        let one = |k: &str| -> Option<String> {
            let first = marks.first().map(|m| s(m.get(k))).unwrap_or_default();
            marks.iter().all(|m| s(m.get(k)) == first).then_some(first)
        };
        let areas: Vec<Value> = self.tk.state.get("areas").and_then(Value::as_array).cloned().unwrap_or_default();
        if !areas.is_empty() {
            let cur = one("area");
            let shown = match &cur {
                None => "—".to_string(),
                Some(c) => areas.iter().find(|a| s(a.get("id")) == *c).map_or_else(|| t("area.none"), |a| s(a.get("name"))),
            };
            let l = ui.label(RichText::new(t("props.area")).small());
            egui::ComboBox::new("tk_m_area", "")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(cur.as_deref() == Some(""), t("area.none")).clicked() {
                        set.push(("area".into(), String::new()));
                    }
                    for a in &areas {
                        let aid = s(a.get("id"));
                        if ui.selectable_label(cur.as_deref() == Some(aid.as_str()), s(a.get("name"))).clicked() {
                            set.push(("area".into(), aid));
                        }
                    }
                })
                .response
                .labelled_by(l.id);
        }
    }
}
