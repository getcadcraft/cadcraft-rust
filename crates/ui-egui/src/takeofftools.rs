//! TakeoffCraft on the sheet view: a sheet scale (the 0.5 presets), items (count, linear, area), marks clicked on the
//! sheet, and the quantities and estimate cc-takeoff computes (model.js `quantities` / `estimate`, at parity). Every
//! edit goes through `Takeoff` (one undo step, the 0.5 labels and ids); marks are kept in PDF user space and the sheet
//! is known by its file's hash, as `sheetKey(hash, page)` in the JS.

use cc_takeoff::model::{QOpts, Takeoff, estimate, quantities, sheet_key};
use egui::{Color32, Pos2, RichText};
use serde_json::{Map, Value, json};

use crate::theme::Tokens;

/// Points a linear or area mark keeps at most while it is clicked (the model caps the file; this caps the gesture).
const MAX_PTS: usize = 10_000;

/// The takeoff of one open plan set.
pub struct TakeoffMode {
    pub tk: Takeoff,
    hash: String,
    /// the item new marks belong to
    pub item: Option<String>,
    /// the mark being clicked (linear / area), PDF user space
    pub pts: Vec<[f64; 2]>,
    pub message: String,
    /// the name and type the "Add item" row uses
    new_name: String,
    new_type: String,
}

impl std::fmt::Debug for TakeoffMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TakeoffMode").field("items", &self.tk.items().len()).field("marks", &self.tk.marks().len()).finish()
    }
}

fn obj(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(o) => o,
        _ => Map::new(),
    }
}

/// A number as the panel shows it (two decimals, no trailing zeros).
fn qty(x: f64) -> String {
    if !x.is_finite() {
        return "—".into();
    }
    let s = format!("{x:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl TakeoffMode {
    /// A new takeoff of the file whose SHA-256 is `hash`.
    pub fn new(hash: &str) -> TakeoffMode {
        TakeoffMode {
            tk: Takeoff::default(),
            hash: hash.to_string(),
            item: None,
            pts: Vec::new(),
            message: String::new(),
            new_name: String::new(),
            new_type: "count".into(),
        }
    }

    /// The key marks of page `page` carry.
    pub fn sheet(&self, page: usize) -> String {
        sheet_key(&self.hash, page as f64)
    }

    fn item_type(&self) -> Option<String> {
        let it = self.tk.item(self.item.as_deref()?)?;
        it.get("type").and_then(Value::as_str).map(str::to_string)
    }

    /// The scale of a sheet in real feet per PDF unit, if set.
    pub fn ft_per_unit(&self, sheet: &str) -> Option<f64> {
        self.tk.scale_of(sheet).and_then(|s| s.get("ftPerUnit")).and_then(Value::as_f64).filter(|x| *x > 0.0)
    }

    /// Set a sheet's scale from a preset (one undo step).
    pub fn set_preset(&mut self, sheet: &str, key: &str, user_unit: f64) {
        let Some(p) = cc_takeoff::units::preset(key) else { return };
        match cc_takeoff::units::preset_ft_per_unit(key, user_unit) {
            Ok(fpu) => {
                let sc = json!({ "method": "preset", "ftPerUnit": fpu, "preset": key, "label": p.label, "cal": null });
                if let Err(e) = self.tk.set_scale(sheet, &sc, None) {
                    self.message = e;
                } else {
                    self.message = format!("Scale {}", p.label);
                }
            }
            Err(e) => self.message = e,
        }
    }

    /// Add an item (count, linear or area) and make it the current one.
    pub fn add_item(&mut self, name: &str, ty: &str) -> Option<String> {
        let f = obj(json!({ "name": name, "type": ty }));
        match self.tk.add_item(&f) {
            Ok(Some(id)) => {
                self.item = Some(id.clone());
                Some(id)
            }
            Ok(None) => None,
            Err(e) => {
                self.message = e;
                None
            }
        }
    }

    /// A click on the sheet at `p` (PDF user space): a count mark at once; a point of the linear / area mark otherwise.
    pub fn click(&mut self, sheet: &str, p: [f64; 2]) {
        let Some(item) = self.item.clone() else {
            self.message = "Pick or add an item first".into();
            return;
        };
        if self.item_type().as_deref() == Some("count") {
            let f = obj(json!({ "item": item, "sheet": sheet, "pts": [{ "x": p[0], "y": p[1] }] }));
            if let Err(e) = self.tk.add_mark(&f) {
                self.message = e;
            }
        } else if self.pts.len() < MAX_PTS {
            self.pts.push(p);
        }
    }

    /// Finish the linear / area mark being clicked (double-click or Enter); too few points make nothing.
    pub fn finish(&mut self, sheet: &str) {
        let pts = std::mem::take(&mut self.pts);
        let Some(item) = self.item.clone() else { return };
        let need = if self.item_type().as_deref() == Some("area") { 3 } else { 2 };
        if pts.len() < need {
            return;
        }
        let pts: Vec<Value> = pts.iter().map(|p| json!({ "x": p[0], "y": p[1] })).collect();
        let f = obj(json!({ "item": item, "sheet": sheet, "pts": pts }));
        if let Err(e) = self.tk.add_mark(&f) {
            self.message = e;
        }
    }

    /// The right-hand panel: the sheet's scale, the items with their quantities, the estimate's totals.
    pub fn panel(&mut self, ui: &mut egui::Ui, t: &Tokens, sheet: &str, user_unit: f64) {
        ui.label(RichText::new("Takeoff").strong().color(t.text_strong));
        // scale
        let cur = self.tk.scale_of(sheet).and_then(|s| s.get("label")).and_then(Value::as_str).unwrap_or("Not scaled").to_string();
        let mut pick: Option<&'static str> = None;
        let l = ui.label(RichText::new("Sheet scale").small());
        let _ = egui::ComboBox::from_id_salt("tk_scale")
            .selected_text(cur)
            .width(ui.available_width().clamp(80.0, 220.0))
            .show_ui(ui, |ui| {
                for p in &cc_takeoff::units::PRESETS {
                    if ui.selectable_label(false, format!("{}  ({})", p.label, p.group)).clicked() {
                        pick = Some(p.key);
                    }
                }
            })
            .response
            .labelled_by(l.id);
        if let Some(k) = pick {
            self.set_preset(sheet, k, user_unit);
        }
        ui.separator();
        // items
        let q = quantities(&self.tk.state, &QOpts::default());
        let mut select: Option<String> = None;
        let mut remove: Option<String> = None;
        for it in self.tk.items() {
            let id = it.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            let name = it.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let ty = it.get("type").and_then(Value::as_str).unwrap_or("");
            let color = it.get("color").and_then(Value::as_str).and_then(|c| crate::pdfview::hex_color(c, 1.0)).unwrap_or(Color32::GRAY);
            let tot = q.get(&id).map(|x| &x.tot);
            let amount = match (ty, tot) {
                ("count", Some(t)) => format!("{} ea", qty(t.count)),
                ("linear", Some(t)) => format!("{} ft", qty(t.length_ft)),
                ("area", Some(t)) => format!("{} sf", qty(t.area_sqft)),
                _ => String::new(),
            };
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, color);
                let on = self.item.as_deref() == Some(id.as_str());
                if ui.selectable_label(on, format!("{name}  ·  {amount}")).clicked() {
                    select = Some(id.clone());
                }
                if ui.small_button("Remove").clicked() {
                    remove = Some(id.clone());
                }
            });
        }
        if let Some(id) = select {
            self.item = Some(id);
            self.pts.clear();
        }
        if let Some(id) = remove {
            let _ = self.tk.remove_item(&id);
            if self.item.as_deref() == Some(id.as_str()) {
                self.item = None;
            }
        }
        // add an item
        ui.add_space(4.0);
        ui.add(egui::TextEdit::singleline(&mut self.new_name).hint_text("New item name").desired_width(ui.available_width()));
        ui.horizontal(|ui| {
            for (ty, label) in [("count", "Count"), ("linear", "Linear"), ("area", "Area")] {
                ui.radio_value(&mut self.new_type, ty.to_string(), label);
            }
        });
        if ui.button("Add item").clicked() {
            let name = if self.new_name.trim().is_empty() {
                format!("Item {}", self.tk.items().len() + 1)
            } else {
                self.new_name.trim().to_string()
            };
            let ty = self.new_type.clone();
            self.add_item(&name, &ty);
            self.new_name.clear();
        }
        ui.separator();
        let e = estimate(&self.tk.state, &q);
        ui.label(format!("{} marks · estimate ${:.2}", self.tk.marks().len(), e.total / 100.0));
        if e.unscaled > 0.0 {
            ui.label(RichText::new(format!("{} marks are on sheets without a scale", qty(e.unscaled))).color(t.text_dim));
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(self.tk.can_undo(), egui::Button::new("Undo")).clicked() {
                self.message = self.tk.undo().map(|l| format!("Undo {l}")).unwrap_or_default();
            }
            if ui.add_enabled(self.tk.can_redo(), egui::Button::new("Redo")).clicked() {
                self.message = self.tk.redo().map(|l| format!("Redo {l}")).unwrap_or_default();
            }
        });
        if !self.message.is_empty() {
            ui.label(RichText::new(&self.message).color(t.text_dim));
        }
    }

    /// The marks of `sheet` (and the one being clicked), through `at` (PDF user space → screen).
    pub fn draw(&self, painter: &egui::Painter, sheet: &str, at: &dyn Fn(f64, f64) -> Pos2) {
        let color_of = |item: &str| {
            self.tk
                .item(item)
                .and_then(|it| it.get("color"))
                .and_then(Value::as_str)
                .and_then(|c| crate::pdfview::hex_color(c, 1.0))
                .unwrap_or(Color32::from_rgb(0x2f, 0x6f, 0xde))
        };
        for m in self.tk.marks() {
            if m.get("sheet").and_then(Value::as_str) != Some(sheet) {
                continue;
            }
            let item = m.get("item").and_then(Value::as_str).unwrap_or("");
            let kind = m.get("kind").and_then(Value::as_str).unwrap_or("");
            let pts: Vec<Pos2> = cc_takeoff::model::pts_of(m.get("pts")).iter().map(|p| at(p.x, p.y)).collect();
            let c = color_of(item);
            match kind {
                "count" => {
                    for p in pts {
                        painter.circle(p, 6.0, c.gamma_multiply(0.35), egui::Stroke::new(2.0, c));
                    }
                }
                "area" if pts.len() >= 3 => {
                    painter.add(egui::Shape::convex_polygon(pts.clone(), c.gamma_multiply(0.25), egui::Stroke::NONE));
                    let mut ring = pts;
                    if let Some(f) = ring.first().copied() {
                        ring.push(f);
                    }
                    painter.add(egui::Shape::line(ring, egui::Stroke::new(2.0, c)));
                }
                _ => {
                    painter.add(egui::Shape::line(pts, egui::Stroke::new(2.5, c)));
                }
            }
        }
        if !self.pts.is_empty() {
            let c = self.item.as_deref().map_or(Color32::GRAY, color_of);
            let pts: Vec<Pos2> = self.pts.iter().map(|p| at(p[0], p[1])).collect();
            for p in &pts {
                painter.circle_filled(*p, 3.0, c);
            }
            painter.add(egui::Shape::dashed_line(&pts, egui::Stroke::new(2.0, c), 6.0, 4.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_lengths_on_a_scaled_sheet() {
        let mut m = TakeoffMode::new("abc");
        let sheet = m.sheet(0);
        // 1/4" = 1'-0": 72 PDF units an inch, so 18 units are a foot
        m.set_preset(&sheet, "a1/4", 1.0);
        let fpu = m.ft_per_unit(&sheet).unwrap();
        assert!((fpu * 18.0 - 1.0).abs() < 1e-12, "{fpu}");
        m.add_item("Outlets", "count");
        m.click(&sheet, [10.0, 10.0]);
        m.click(&sheet, [20.0, 10.0]);
        let walls = m.add_item("Walls", "linear").unwrap();
        m.click(&sheet, [0.0, 0.0]);
        m.click(&sheet, [180.0, 0.0]);
        m.finish(&sheet);
        let q = quantities(&m.tk.state, &QOpts::default());
        assert_eq!(m.tk.marks().len(), 3);
        assert!((q.get(&walls).unwrap().tot.length_ft - 10.0).abs() < 1e-9);
        assert!(m.tk.undo().is_some());
        assert_eq!(m.tk.marks().len(), 2);
    }
}
