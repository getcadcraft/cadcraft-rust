//! The equipment schedule tab of the drawing viewer (BimCraft 0.5's DWG view: dwg-equipment.js + equip-view.js): the
//! equipment list hidden in a foodservice drawing's item tags, as a schedule — pick which sheet's equipment to list,
//! filter by any word, save the CSV, and jump to an item on the drawing ("Where": the item ringed on the sheet that
//! shows it). The logic and words are [`cc_dwgview`]'s; this draws them and runs the commands.
//!
//! Commands: `viewer.equip` (show the tab), `viewer.equip.pick {name: all | off | every | <sheet key>}`,
//! `viewer.equip.filter {name: words}`, `viewer.equip.csv`, `viewer.equip.show {index: row shown}`.

use std::sync::Arc;

use cc_dwgview::equipment::{EquipList, equipment_csv, equipment_list};
use cc_dwgview::page;
use egui::{Color32, RichText};

use crate::{Params, UiApp};

pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("viewer.equip", "Equipment schedule", "", "{} → shows the equipment tab (a drawing with item tags)"),
    ("viewer.equip.pick", "Which equipment", "", "{name: \"all\" | \"off\" | \"every\" | sheet key} → the schedule lists that choice"),
    ("viewer.equip.filter", "Filter the schedule", "", "{name: words} → only rows holding every word"),
    ("viewer.equip.csv", "Download CSV", "", "{} → saves the rows shown as CSV (\"<drawing> equipment (<choice>).csv\")"),
    ("viewer.equip.show", "Show an item on the drawing", "", "{index: row in the list shown} → rings it on the sheet that shows it"),
];

/// The schedule's state (per open drawing).
#[derive(Debug, Default)]
pub struct Equip {
    /// the tab shows
    pub on: bool,
    /// the choice ("" = the first)
    pub pick: String,
    pub query: String,
    /// (doc id, the list) — worked out once per drawing
    cache: Option<(u64, Option<Arc<EquipList>>)>,
}

/// The open drawing's equipment list (None: no item tags, or no drawing / model).
pub fn list(app: &mut UiApp) -> Option<Arc<EquipList>> {
    let d = app.doc.as_ref()?;
    if app.editor.is_some() {
        return None;
    }
    if let Some((id, l)) = &app.viewer.equip.cache
        && *id == d.id
    {
        return l.clone();
    }
    let l = d.model.as_ref().and_then(|m| equipment_list(m, &d.drawing)).map(Arc::new);
    app.viewer.equip = Equip { cache: Some((d.id, l.clone())), ..Equip::default() };
    l
}

/// The tab shows.
pub fn showing(app: &UiApp) -> bool {
    app.viewer.equip.on && crate::viewer::active(app)
}

fn pick_of(app: &UiApp, head: &page::EquipHead) -> String {
    if head.choices.iter().any(|c| c.value == app.viewer.equip.pick) {
        app.viewer.equip.pick.clone()
    } else {
        head.choices.first().map(|c| c.value.clone()).unwrap_or_default()
    }
}

pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    let l = list(app).ok_or_else(|| format!("{id}: the drawing has no equipment tags"))?;
    let head = page::equip_head(&l);
    match id {
        "viewer.equip" => {
            crate::dwg3d::leave(app);
            app.viewer.equip.on = true;
        }
        "viewer.equip.pick" => {
            let v = params.name.clone().ok_or("viewer.equip.pick: name is required")?;
            if !head.choices.iter().any(|c| c.value == v) {
                let all: Vec<String> = head.choices.iter().map(|c| c.value.clone()).collect();
                return Err(format!("viewer.equip.pick: no choice {v} ({})", all.join(", ")));
            }
            app.viewer.equip.pick = v;
        }
        "viewer.equip.filter" => {
            let q = params.name.clone().unwrap_or_default();
            if q.chars().count() > 500 {
                return Err("viewer.equip.filter: at most 500 characters".into());
            }
            app.viewer.equip.query = cc_cad::js::trim(&q).to_string();
        }
        "viewer.equip.csv" => {
            let pick = pick_of(app, &head);
            let label = head.choices.iter().find(|c| c.value == pick).map(|c| c.label.clone()).unwrap_or_default();
            let rows = page::equip_shown(&l, &pick, &app.viewer.equip.query);
            if rows.is_empty() {
                return Err("viewer.equip.csv: no rows to save".into());
            }
            let Some(d) = app.doc.as_ref() else { return Err("viewer.equip.csv: no drawing".into()) };
            let csv = equipment_csv(&rows, &|k| page::sheet_name(&d.drawing, k));
            let name = page::csv_name(&d.name, &label, &pick);
            let mut bytes = "\u{feff}".as_bytes().to_vec();
            bytes.extend_from_slice(csv.as_bytes());
            let n = rows.len();
            match app.services.save.as_mut().map(|f| f(&name, bytes)) {
                Some(Ok(Some(_))) => app.message = page::downloaded(&name, n),
                Some(Ok(None)) => {}
                Some(Err(e)) => return Err(e),
                None => return Err("viewer.equip.csv: saving is not available here".into()),
            }
        }
        "viewer.equip.show" => {
            let i = params
                .index
                .or_else(|| params.name.as_deref().and_then(|n| n.trim().parse().ok()))
                .ok_or("viewer.equip.show: index is required")?;
            let pick = pick_of(app, &head);
            let rows = page::equip_shown(&l, &pick, &app.viewer.equip.query);
            let r = rows.get(i).ok_or_else(|| format!("viewer.equip.show: no row {i}"))?;
            let place =
                r.places.iter().find(|p| !p.sheets.is_empty()).or(r.places.first()).ok_or("viewer.equip.show: the item has no place")?;
            let Some(d) = app.doc.as_ref() else { return Err("viewer.equip.show: no drawing".into()) };
            let (key, x, y) = page::focus_target(&d.drawing, place.x, place.y, place.sheets.first().map(String::as_str));
            let space = d.drawing.spaces.iter().position(|s| s.key == key).unwrap_or(0);
            let status = page::ringed(r, &page::sheet_name(&d.drawing, &key));
            app.viewer.equip.on = false;
            crate::viewer::focus(app, space, x, y);
            app.message = status;
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

const CLASH: Color32 = Color32::from_rgb(0xd8, 0x9a, 0x2a);

/// The schedule in the central area.
pub fn show(app: &mut UiApp, ui: &mut egui::Ui) {
    let Some(l) = list(app) else { return };
    let t = app.tokens;
    let head = page::equip_head(&l);
    let pick = pick_of(app, &head);
    let query = app.viewer.equip.query.clone();
    let rows = page::equip_shown(&l, &pick, &query);
    let names: Vec<(String, String)> =
        app.doc.as_ref().map(|d| d.drawing.spaces.iter().map(|s| (s.key.clone(), s.name.clone())).collect()).unwrap_or_default();
    let sheet_name = |k: &str| names.iter().find(|(key, _)| key == k).map_or_else(|| k.to_string(), |(_, n)| n.clone());
    let mut act: Option<(&str, Params)> = None;
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, t.panel_darker);
    egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
        ui.label(RichText::new(&head.kind).small().color(t.text_dim));
        ui.label(RichText::new(&head.title).heading().color(t.text_strong));
        ui.label(RichText::new(&head.intro).small().color(t.text_dim));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let sel = head.choices.iter().find(|c| c.value == pick).map(|c| c.text.clone()).unwrap_or_default();
            egui::ComboBox::new("equip_which", "")
                .selected_text(sel)
                .show_ui(ui, |ui| {
                    for c in &head.choices {
                        if ui.selectable_label(c.value == pick, &c.text).clicked() {
                            act = Some(("viewer.equip.pick", Params { name: Some(c.value.clone()), ..Default::default() }));
                        }
                    }
                })
                .response
                .on_hover_text(&head.which);
            let mut q = query.clone();
            let r = ui.add(crate::shell::field(&mut q).hint_text(&head.filter_placeholder).desired_width(260.0));
            let r = r.on_hover_text(&head.filter_label);
            if r.changed() {
                act = Some(("viewer.equip.filter", Params { name: Some(q), ..Default::default() }));
            }
            if ui.add_enabled(!rows.is_empty(), crate::shell::Btn::new(&head.csv)).clicked() {
                act = Some(("viewer.equip.csv", Params::default()));
            }
        });
        ui.label(RichText::new(page::count_line(rows.len(), &query)).small().color(t.text_dim));
        ui.add_space(4.0);
        let note_h = 48.0;
        let h = (ui.available_height() - note_h).max(80.0);
        egui::ScrollArea::both().max_height(h).auto_shrink([false, false]).show(ui, |ui| {
            // column widths (points): item, qty, equipment, manufacturer, model, electrical, plumbing, where
            // (shared out over the width there is, never under three quarters of it)
            const W: [f32; 8] = [56.0, 56.0, 220.0, 170.0, 140.0, 200.0, 130.0, 150.0];
            let total: f32 = W.iter().sum::<f32>() + 10.0 * 8.0;
            let k_w = (ui.available_width() / total).clamp(0.75, 1.6);
            let cell = |ui: &mut egui::Ui, k: usize, f: &mut dyn FnMut(&mut egui::Ui)| {
                let w = W.get(k).copied().unwrap_or(100.0) * k_w;
                ui.allocate_ui_with_layout(egui::vec2(w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(w);
                    f(ui);
                });
            };
            // one row = one horizontal strip as tall as its tallest cell (striped)
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 0.0);
            ui.horizontal_top(|ui| {
                for (k, hd) in head.headers.iter().enumerate() {
                    cell(ui, k, &mut |ui| {
                        ui.label(RichText::new(hd).strong().small().color(t.text_strong));
                    });
                }
            });
            ui.add_space(4.0);
            for (i, r) in rows.iter().enumerate() {
                let c = page::row_cells(r, &sheet_name);
                let main = |ui: &mut egui::Ui, s: &str| {
                    ui.label(RichText::new(s).small().color(t.text));
                };
                let sub = |ui: &mut egui::Ui, s: &Option<String>| {
                    if let Some(s) = s {
                        ui.label(RichText::new(s).small().color(t.text_dim));
                    }
                };
                let top = ui.cursor().min;
                let resp = ui.horizontal_top(|ui| {
                    cell(ui, 0, &mut |ui| {
                        ui.label(RichText::new(&c.item).small().strong().color(if r.clash { CLASH } else { t.text_strong }));
                    });
                    cell(ui, 1, &mut |ui| {
                        main(ui, &c.qty);
                        sub(ui, &c.qty_sub);
                    });
                    cell(ui, 2, &mut |ui| {
                        main(ui, &c.category);
                        if let Some(p) = &c.clash {
                            ui.label(RichText::new(p).small().color(CLASH));
                        }
                        sub(ui, &c.remark);
                    });
                    cell(ui, 3, &mut |ui| main(ui, &c.mfr));
                    cell(ui, 4, &mut |ui| main(ui, &c.model));
                    cell(ui, 5, &mut |ui| {
                        main(ui, &c.elec);
                        sub(ui, &c.el_remark);
                    });
                    cell(ui, 6, &mut |ui| {
                        main(ui, &c.plumb);
                        sub(ui, &c.pl_remark);
                    });
                    cell(ui, 7, &mut |ui| {
                        if ui.link(RichText::new(&c.where_).small()).on_hover_text(&c.where_title).clicked() {
                            act = Some(("viewer.equip.show", Params { index: Some(i), ..Default::default() }));
                        }
                        sub(ui, &c.tags);
                    });
                });
                if i % 2 == 1 {
                    let band = egui::Rect::from_min_max(top, egui::pos2(resp.response.rect.max.x, resp.response.rect.max.y + 3.0));
                    ui.painter().rect_filled(band, 0.0, Color32::from_white_alpha(6));
                }
                ui.add_space(5.0);
            }
        });
        ui.add_space(4.0);
        ui.label(RichText::new(&head.note).small().color(t.text_dim));
    });
    if let Some((id, p)) = act
        && let Err(e) = app.run(id, &p)
    {
        app.error = Some(e);
    }
}
