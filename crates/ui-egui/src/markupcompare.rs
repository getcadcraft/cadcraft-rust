//! MarkupCraft's Compare sheets by changed lines (markupcraft.js vectorCompare / cloudChanges, 0.4 K12): any sheet of
//! any open file against the open one; the vector paths of both (cc-io-markup `page_ops` → cc-markup `page_segments`,
//! cached per sheet) diffed at 1.5 pt (`diffSegments`), the lines only on the old sheet drawn red and those only on the
//! new one blue over it, and "Cloud the changes": a revision cloud round each cluster of changed lines (`clusters`, gap
//! 18, at least 6 pt), all in one undo step, each with the comment "Changed since <old sheet>". The picture overlay mode
//! is not here yet.

use std::collections::HashMap;

use cc_markup::i18n::{Fill, t, t0};
use cc_markup::vecdiff::{Diff, Seg, clusters, diff_segments, page_segments};
use serde_json::{Map, json};

use crate::pdfview::PdfView;

/// The compare under way.
#[derive(Debug, Clone, Default)]
pub struct Compare {
    pub old_key: String,
    pub new_key: String,
    pub vec: Option<Diff>,
    /// sheet key → its segments (`segCache`)
    pub segs: HashMap<String, Vec<Seg>>,
    pub status: String,
}

fn len(l: &[Seg]) -> f64 {
    l.iter().map(|q| (q[2] - q[0]).hypot(q[3] - q[1])).sum::<f64>().round()
}

impl PdfView {
    /// A sheet's title (`s.page.title`).
    pub fn sheet_title(&self, key: &str) -> String {
        for f in self.files() {
            if let Some(i) = f.page_keys.iter().position(|k| k == key) {
                return f.titles.get(i).cloned().unwrap_or_default();
            }
        }
        key.to_string()
    }

    fn segs_of(&mut self, key: &str) -> Result<Vec<Seg>, String> {
        if let Some(s) = self.cmp.segs.get(key) {
            return Ok(s.clone());
        }
        let found =
            self.files().into_iter().find_map(|f| f.page_keys.iter().position(|k| k == key).map(|i| (std::sync::Arc::clone(&f.bytes), i)));
        let (bytes, i) = found.ok_or_else(|| format!("compare: no sheet {key}"))?;
        let doc = cc_io_markup::jsdoc::JsDoc::open(&bytes).map_err(|e| format!("{e:?}"))?;
        let segs = page_segments(&cc_io_markup::ops::page_ops(&doc, i), 400_000);
        self.cmp.segs.insert(key.to_string(), segs.clone());
        Ok(segs)
    }

    /// startCompare + vectorCompare: compare sheet `old_key` with the open sheet.
    pub fn compare_start(&mut self, old_key: &str) -> Result<(), String> {
        let new_key = self.sheet_key();
        if old_key.is_empty() {
            self.cmp.status = t0("compare.need");
            return Err(self.cmp.status.clone());
        }
        if old_key == new_key {
            self.cmp.status = t0("compare.need.two");
            return Err(self.cmp.status.clone());
        }
        let a = self.segs_of(old_key)?;
        let b = self.segs_of(&new_key)?;
        self.cmp.old_key = old_key.to_string();
        self.cmp_pick = old_key.to_string();
        self.cmp.new_key = new_key;
        if a.is_empty() && b.is_empty() {
            self.cmp.vec = None;
            self.cmp.status = t0("compare.lines.none");
            return Ok(());
        }
        let identity = cc_markup::compare::alignment(None, 0.0, 0.0);
        let d = diff_segments(&a, &b, 1.5, Some(&identity), 2.0);
        self.cmp.status = t("compare.lines.done", &[("removed", Fill::N(len(&d.removed))), ("added", Fill::N(len(&d.added)))]);
        self.cmp.vec = Some(d);
        Ok(())
    }

    /// Stop comparing.
    pub fn compare_stop(&mut self) {
        self.cmp.vec = None;
        self.cmp.old_key.clear();
        self.cmp.new_key.clear();
        self.cmp.status.clear();
    }

    /// cloudChanges: a revision cloud round each cluster of changed lines, one undo step; they are selected.
    pub fn compare_cloud(&mut self) -> Result<(), String> {
        let Some(d) = self.cmp.vec.clone().filter(|_| self.cmp.new_key == self.sheet_key()) else {
            return Err(t0("compare.cloud.none"));
        };
        let all: Vec<Seg> = d.removed.iter().chain(d.added.iter()).copied().collect();
        let boxes = clusters(&all, 18.0, 6.0);
        if boxes.is_empty() {
            self.tools.message = t0("compare.cloud.none");
            return Ok(());
        }
        self.tick();
        let key = self.sheet_key();
        let comment = t("compare.cloud.comment", &[("old", Fill::S(self.sheet_title(&self.cmp.old_key)))]);
        let author = self.tools.author.clone();
        self.tools.rot = self.page_rot();
        let made: Vec<cc_markup::model::Markup> = boxes
            .iter()
            .map(|b| {
                let mut g = Map::new();
                g.insert("rect".into(), json!([b[0] - 8.0, b[1] - 8.0, b[2] + 8.0, b[3] + 8.0]));
                g.insert("author".into(), json!(author));
                g.insert("comment".into(), json!(comment));
                self.tools.new_markup(&mut self.store, &key, "cloudrect", g)
            })
            .collect();
        let n = made.len() as f64;
        let ids: Vec<String> = made.iter().map(|m| m.id.clone()).collect();
        let r = self.store.change(&t("undo.compare.cloud", &[("n", Fill::N(n))]), |tx| {
            for m in made {
                tx.add(m)?;
            }
            Ok(())
        });
        r.map_err(|e| e.to_string())?;
        self.tools.sel = ids;
        self.tools.message = t("compare.cloud.done", &[("n", Fill::N(n))]);
        Ok(())
    }

    /// The Compare part of the Properties panel.
    pub fn compare_panel(&mut self, ui: &mut egui::Ui, tok: &crate::theme::Tokens) {
        ui.label(egui::RichText::new(t0("compare.h")).strong().color(tok.text_strong));
        ui.label(egui::RichText::new(t0("compare.note")).small().color(tok.text_dim));
        let mut sheets: Vec<(String, String)> = Vec::new();
        let several = !self.others.is_empty();
        for f in self.files() {
            for (i, k) in f.page_keys.iter().enumerate() {
                let l = f.titles.get(i).cloned().unwrap_or_default();
                let label =
                    if several { t("compare.sheet_in_file", &[("sheet", Fill::S(l)), ("file", Fill::S(f.name.clone()))]) } else { l };
                sheets.push((k.clone(), label));
            }
        }
        let cur = self.sheet_key();
        let mut pick = self.cmp_pick.clone();
        let shown = sheets.iter().find(|(k, _)| *k == pick).map_or_else(|| t0("choose_one"), |(_, l)| l.clone());
        let l = ui.label(egui::RichText::new(t0("compare.old")).small());
        egui::ComboBox::new("mc-cmp-old", "")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for (k, label) in &sheets {
                    if *k != cur && ui.selectable_label(pick == *k, label).clicked() {
                        pick = k.clone();
                    }
                }
            })
            .response
            .labelled_by(l.id);
        self.cmp_pick = pick.clone();
        let mut act: Option<&str> = None;
        ui.horizontal_wrapped(|ui| {
            if ui.button(t0("compare.start")).clicked() {
                act = Some("start");
            }
            if self.cmp.vec.is_some() {
                if ui.button(t0("compare.cloud")).on_hover_text(t0("compare.cloud.title")).clicked() {
                    act = Some("cloud");
                }
                if ui.button(t0("compare.stop")).clicked() {
                    act = Some("stop");
                }
            }
        });
        match act {
            Some("start") => {
                let _ = self.compare_start(&pick);
            }
            Some("cloud") => {
                if let Err(e) = self.compare_cloud() {
                    self.tools.message = e;
                }
            }
            Some("stop") => self.compare_stop(),
            _ => {}
        }
        if self.cmp.vec.is_some() {
            ui.label(egui::RichText::new(t0("compare.legend.old")).color(ui.visuals().error_fg_color));
            ui.label(egui::RichText::new(t0("compare.legend.new")).color(tok.accent));
        }
        if !self.cmp.status.is_empty() {
            ui.label(egui::RichText::new(&self.cmp.status).color(tok.text_dim));
        }
    }

    /// The changed lines over the sheet (when the open sheet is the compare's new one).
    pub fn draw_compare(&self, painter: &egui::Painter, at: &dyn Fn(f64, f64) -> egui::Pos2, old: egui::Color32, new: egui::Color32) {
        let Some(d) = self.cmp.vec.as_ref().filter(|_| self.cmp.new_key == self.sheet_key()) else { return };
        for (list, c) in [(&d.removed, old), (&d.added, new)] {
            for s in list {
                painter.line_segment([at(s[0], s[1]), at(s[2], s[3])], egui::Stroke::new(2.0, c));
            }
        }
    }
}
