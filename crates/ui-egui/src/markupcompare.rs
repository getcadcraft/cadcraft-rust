//! MarkupCraft's Compare sheets (markupcraft.js startCompare / applyCompare / vectorCompare / cloudChanges, 0.4 K11–K12,
//! and compareSets / cloudSet, K23): any sheet of any open file against the open one, shown as the picture overlay
//! (compare.js `combine`: both sheets rendered at the same scale, the old one moved by the alignment, ink only on the
//! old sheet red, only on the new blue, on both grey, the balance fading between them) or as changed lines (cc-io-markup
//! `page_ops` → cc-markup `page_segments`, cached per sheet, `diffSegments` at 1.5 pt through the alignment). Line up:
//! nudge the old sheet by a step, or align by two points clicked on each sheet (`similarity`); Reset. "Cloud the
//! changes": a revision cloud round each cluster of changed lines (`clusters`, gap 18, at least 6 pt) in one undo step,
//! each with the comment "Changed since <old sheet>". Compare two sets: every sheet of an old file paired by number with
//! the new file's (`matchSheets`), ranked by how much changed, all clouded in one step.

use std::collections::HashMap;

use cc_markup::i18n::{Fill, t, t0};
use cc_markup::vecdiff::{Diff, Seg, clusters, diff_segments, page_segments};
use serde_json::{Map, json};

use crate::pdfview::PdfView;

type P = [f64; 2];

/// The picture overlay's longest side (pixels).
const PIC_MAX: f64 = 2400.0;

/// The picture overlay rendered for one view: the two renderings (kept for balance changes) and the mixed texture.
pub struct PicCache {
    pub key: String,
    pub size: [usize; 2],
    pub old: Vec<u8>,
    pub new: Vec<u8>,
    pub balance: f64,
    pub tex: Option<egui::TextureHandle>,
}

impl std::fmt::Debug for PicCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PicCache").field("key", &self.key).field("size", &self.size).finish()
    }
}

/// One pair of a set compare (`setResult`).
#[derive(Debug, Clone)]
pub struct SetRow {
    pub old_key: String,
    pub new_key: String,
    pub d: Diff,
    pub len: f64,
}

/// The compare under way.
#[derive(Debug)]
pub struct Compare {
    pub old_key: String,
    pub new_key: String,
    /// comparing (`cmp.on`), how ("picture" | "lines")
    pub on: bool,
    pub mode: &'static str,
    pub vec: Option<Diff>,
    /// sheet key → its segments (`segCache`)
    pub segs: HashMap<String, Vec<Seg>>,
    /// the panel's status line (`cmpStatus`)
    pub status: String,
    /// the line-up: nudge offset, two-point fit, nudge step
    pub dx: f64,
    pub dy: f64,
    pub fit: Option<[f64; 6]>,
    pub step: f64,
    /// the balance slider (0 old only … 1 new only)
    pub balance: f64,
    /// align by two points: the points clicked so far
    pub aligning: Option<Vec<P>>,
    pub pic: Option<PicCache>,
    /// Compare two sets: the files picked and the ranked pairs
    pub set_old: String,
    pub set_new: String,
    pub set_rows: Vec<SetRow>,
    pub set_status: String,
}

impl Default for Compare {
    fn default() -> Self {
        Compare {
            old_key: String::new(),
            new_key: String::new(),
            on: false,
            mode: "picture",
            vec: None,
            segs: HashMap::new(),
            status: String::new(),
            dx: 0.0,
            dy: 0.0,
            fit: None,
            step: 1.0,
            balance: 0.5,
            aligning: None,
            pic: None,
            set_old: String::new(),
            set_new: String::new(),
            set_rows: Vec::new(),
            set_status: String::new(),
        }
    }
}

impl Compare {
    /// `alignment(cmp)`: old user space → new.
    pub fn transform(&self) -> [f64; 6] {
        cc_markup::compare::alignment(self.fit, self.dx, self.dy)
    }
    /// The balance shown now (aligning shows the old sheet, then the new one).
    pub fn balance_now(&self) -> f64 {
        match &self.aligning {
            Some(p) if p.len() < 2 => 0.0,
            Some(_) => 1.0,
            None => self.balance,
        }
    }
}

fn len(l: &[Seg]) -> f64 {
    l.iter().map(|q| (q[2] - q[0]).hypot(q[3] - q[1])).sum::<f64>().round()
}

/// JS `x.toFixed(2)`.
fn f2(x: f64) -> String {
    cc_markup::js::to_fixed(x, 2)
}

impl PdfView {
    /// A sheet's title (`s.page.title`).
    pub fn sheet_title(&self, key: &str) -> String {
        for f in self.files() {
            if let Some(i) = f.page_keys.iter().position(|k| k == key) {
                return f.title(&self.store, i);
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

    /// vectorCompare: the changed lines through the alignment; the panel says how much.
    fn vector_compare(&mut self) -> Result<(), String> {
        let (old, new) = (self.cmp.old_key.clone(), self.cmp.new_key.clone());
        let a = self.segs_of(&old)?;
        let b = self.segs_of(&new)?;
        if a.is_empty() && b.is_empty() {
            self.cmp.vec = None;
            self.cmp.status = t0("compare.lines.none");
            return Ok(());
        }
        let tr = self.cmp.transform();
        let d = diff_segments(&a, &b, 1.5, Some(&tr), 2.0);
        self.cmp.status = t("compare.lines.done", &[("removed", Fill::N(len(&d.removed))), ("added", Fill::N(len(&d.added)))]);
        self.cmp.vec = Some(d);
        Ok(())
    }

    /// applyCompare: lines mode recomputes the changed lines; the picture is drawn again.
    fn apply_compare(&mut self) -> Result<(), String> {
        if !self.cmp.on {
            return Ok(());
        }
        self.cmp.pic = None;
        if self.cmp.mode == "lines" {
            return self.vector_compare();
        }
        self.cmp.vec = None;
        Ok(())
    }

    /// startCompare: compare sheet `old_key` with the open sheet.
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
        if self.locate(old_key).is_none() {
            return Err(format!("compare: no sheet {old_key}"));
        }
        if self.cmp.old_key != old_key || self.cmp.new_key != new_key {
            self.cmp.dx = 0.0;
            self.cmp.dy = 0.0;
            self.cmp.fit = None;
        }
        self.cmp.old_key = old_key.to_string();
        self.cmp_pick = old_key.to_string();
        self.cmp.new_key = new_key.clone();
        self.cmp.on = true;
        self.cmp.aligning = None;
        self.cmp.status = t("compare.on", &[("old", Fill::S(self.sheet_title(old_key))), ("new", Fill::S(self.sheet_title(&new_key)))]);
        self.apply_compare()
    }

    /// stopCompare.
    pub fn compare_stop(&mut self) {
        self.cmp.on = false;
        self.cmp.aligning = None;
        self.cmp.vec = None;
        self.cmp.pic = None;
        self.cmp.status.clear();
    }

    /// The picture overlay or changed lines.
    pub fn compare_mode(&mut self, mode: &str) -> Result<(), String> {
        self.cmp.mode = if mode == "lines" { "lines" } else { "picture" };
        self.apply_compare()
    }

    /// A nudge button: the old sheet moved by the step.
    pub fn compare_nudge(&mut self, x: f64, y: f64) -> Result<(), String> {
        if !self.cmp.on {
            return Ok(());
        }
        let step = if self.cmp.step > 0.0 { self.cmp.step } else { 1.0 };
        self.cmp.dx += x * step;
        self.cmp.dy += y * step;
        self.apply_compare()?;
        self.cmp.status = t("compare.moved", &[("dx", Fill::S(f2(self.cmp.dx))), ("dy", Fill::S(f2(self.cmp.dy)))]);
        Ok(())
    }

    /// Reset the line-up.
    pub fn compare_reset(&mut self) -> Result<(), String> {
        self.cmp.dx = 0.0;
        self.cmp.dy = 0.0;
        self.cmp.fit = None;
        self.apply_compare()
    }

    /// startAlign: the next four clicks are the two features on the old sheet, then on the new one.
    pub fn compare_align(&mut self) {
        if !self.cmp.on {
            self.cmp.status = t0("compare.align.need");
            return;
        }
        self.tools.cancel();
        self.cmp.aligning = Some(Vec::new());
        self.cmp.pic = None;
        self.cmp.status = t0("compare.align.1");
        self.tools.message = t0("compare.align.status");
    }

    /// alignClick.
    pub fn align_click(&mut self, p: P) {
        let Some(pts) = self.cmp.aligning.as_mut() else { return };
        pts.push(p);
        let n = pts.len();
        let all = pts.clone();
        match n {
            1 => self.cmp.status = t0("compare.align.2"),
            2 => {
                self.cmp.pic = None;
                self.cmp.status = t0("compare.align.3");
            }
            3 => self.cmp.status = t0("compare.align.4"),
            _ => {
                let tr = self.cmp.transform();
                let ti = cc_markup::geom::inv(&tr);
                let (Some(p0), Some(p1), Some(p2), Some(p3)) = (all.first(), all.get(1), all.get(2), all.get(3)) else { return };
                let o1 = cc_markup::geom::apply(&ti, *p0);
                let o2 = cc_markup::geom::apply(&ti, *p1);
                let fit = cc_markup::geom::similarity(o1, o2, *p2, *p3);
                self.cmp.aligning = None;
                self.cmp.pic = None;
                let Some(fit) = fit else {
                    self.cmp.status = t0("compare.align.same");
                    let _ = self.apply_compare();
                    return;
                };
                self.cmp.fit = Some(fit);
                self.cmp.dx = 0.0;
                self.cmp.dy = 0.0;
                let sc = fit[0].hypot(fit[1]);
                let ang = fit[1].atan2(fit[0]) * 180.0 / std::f64::consts::PI;
                let _ = self.apply_compare();
                self.cmp.status = t("compare.align.done", &[("scale", Fill::S(f2(sc * 100.0))), ("angle", Fill::S(f2(ang)))]);
                self.tools.message = t0("compare.aligned");
            }
        }
    }

    /// cloudChanges: a revision cloud round each cluster of changed lines, one undo step; they are selected.
    pub fn compare_cloud(&mut self) -> Result<(), String> {
        if !self.cmp.on || self.cmp.new_key != self.sheet_key() {
            return Err(t0("compare.cloud.none"));
        }
        if self.cmp.vec.is_none() {
            self.vector_compare()?;
        }
        let Some(d) = self.cmp.vec.clone() else { return Ok(()) };
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

    /// compareSets: every sheet of the old file paired by number with the new file's, the most changed first.
    pub fn compare_sets(&mut self, old_id: &str, new_id: &str) -> Result<(), String> {
        let facts = |id: &str| -> Vec<cc_markup::slip::SlipSheet> {
            let files = self.files();
            let fi = files.iter().position(|f| f.id == id).map_or(-1.0, |i| i as f64);
            self.sheet_list()
                .into_iter()
                .filter(|s| s.file.id == id)
                .map(|s| {
                    let (w, h) = s.file.pages.get(s.index).map_or((0.0, 0.0), |p| (p.width, p.height));
                    let (view, rotate) = s.file.page_views.get(s.index).copied().unwrap_or(([0.0, 0.0, w, h], 0.0));
                    cc_markup::slip::SlipSheet {
                        key: s.key,
                        no: s.info.sheet_no,
                        file: fi,
                        page: s.index as f64,
                        width: w,
                        height: h,
                        view,
                        rotate,
                    }
                })
                .collect()
        };
        if old_id == new_id || !self.files().iter().any(|f| f.id == old_id) || !self.files().iter().any(|f| f.id == new_id) {
            self.cmp.set_status = t0("setcmp.need");
            return Ok(());
        }
        let pairs: Vec<(String, String)> = cc_markup::slip::match_sheets(&facts(old_id), &facts(new_id))
            .into_iter()
            .filter_map(|p| p.old_key.map(|o| (o, p.new_key)))
            .collect();
        if pairs.is_empty() {
            self.cmp.set_status = t0("setcmp.none");
            return Ok(());
        }
        let mut rows = Vec::new();
        for (o, n) in &pairs {
            let d = diff_segments(&self.segs_of(o)?, &self.segs_of(n)?, 1.5, None, 2.0);
            let l = d.removed.iter().chain(d.added.iter()).map(|q| (q[2] - q[0]).hypot(q[3] - q[1])).sum::<f64>().round();
            rows.push(SetRow { old_key: o.clone(), new_key: n.clone(), d, len: l });
        }
        cc_m3d::js::sort_by(&mut rows, |a, b| b.len - a.len);
        let changed = rows.iter().filter(|r| r.len >= 6.0).count();
        self.cmp.set_status = t("setcmp.done", &[("n", Fill::N(pairs.len() as f64)), ("changed", Fill::N(changed as f64))]);
        self.cmp.set_old = old_id.to_string();
        self.cmp.set_new = new_id.to_string();
        self.cmp.set_rows = rows;
        Ok(())
    }

    /// cloudSet: the changes of every changed pair clouded, one undo step.
    pub fn cloud_set(&mut self) -> Result<(), String> {
        let changed: Vec<SetRow> = self.cmp.set_rows.iter().filter(|r| r.len >= 6.0).cloned().collect();
        if changed.is_empty() {
            return Ok(());
        }
        self.tick();
        let author = self.tools.author.clone();
        let style = self.tools.style_for("cloudrect");
        let mut made = Vec::new();
        for r in &changed {
            let rot = self.locate(&r.new_key).and_then(|(f, i)| f.page_views.get(i).map(|v| v.1)).unwrap_or(0.0);
            let comment = t("compare.cloud.comment", &[("old", Fill::S(self.sheet_title(&r.old_key)))]);
            let all: Vec<Seg> = r.d.removed.iter().chain(r.d.added.iter()).copied().collect();
            for b in clusters(&all, 18.0, 6.0) {
                let mut f = Map::new();
                f.insert("sheet".into(), json!(r.new_key));
                f.insert("type".into(), json!("cloudrect"));
                f.insert("rot".into(), json!(rot));
                f.insert("rect".into(), json!([b[0] - 8.0, b[1] - 8.0, b[2] + 8.0, b[3] + 8.0]));
                f.insert("author".into(), json!(author));
                f.insert("comment".into(), json!(comment));
                let mut m = cc_markup::model::make_markup(&mut self.store.env, &f);
                m.style = style.clone();
                made.push(m);
            }
        }
        let n = made.len() as f64;
        self.store
            .change(&t0("undo.setcmp.cloud"), |tx| {
                for m in made {
                    tx.add(m)?;
                }
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        self.cmp.set_status =
            t("setcmp.clouded", &[("n", Fill::N(n)), ("sheets", Fill::S(t("find.sheets", &[("n", Fill::N(changed.len() as f64))])))]);
        self.tools.message = self.cmp.set_status.clone();
        Ok(())
    }

    /// The picture overlay of the open sheet (when it is the compare's new sheet): both sheets rendered at one scale,
    /// the old one through the alignment, mixed by `combine`. Rendered again when the view's scale or the line-up
    /// changes; a balance change only mixes again.
    pub fn compare_picture(&mut self, ctx: &egui::Context, want_scale: f64) -> Option<egui::TextureHandle> {
        if !self.cmp.on || self.cmp.mode != "picture" || self.cmp.new_key != self.sheet_key() {
            return None;
        }
        let (w, h) = self.cur.pages.get(self.page).map(|p| (p.width, p.height))?;
        let s = want_scale.min(PIC_MAX / w.max(h).max(1.0)).max(0.05);
        let tr = self.cmp.transform();
        let key = format!("{}|{}|{s}|{tr:?}", self.cmp.old_key, self.cmp.new_key);
        let balance = self.cmp.balance_now();
        if self.cmp.pic.as_ref().is_none_or(|p| p.key != key) {
            let (nb, nw, nh) = cc_io_takeoff::pdfread::render_rgba(&self.cur.doc, self.page, s, false).ok()?;
            let new_tr = self.cur.pages.get(self.page)?.transform;
            let (odoc_bytes, oi) = self.locate(&self.cmp.old_key).map(|(f, i)| (std::sync::Arc::clone(&f.bytes), i))?;
            let odoc = cc_io_takeoff::pdfread::open(&odoc_bytes).ok()?;
            let opages = cc_io_takeoff::pdfread::pages(&odoc);
            let old_tr = opages.get(oi)?.transform;
            let (ob, ow, oh) = cc_io_takeoff::pdfread::render_rgba(&odoc, oi, s, false).ok()?;
            let ti = cc_markup::geom::inv(&tr);
            let mut moved = vec![255u8; nw.checked_mul(nh)?.checked_mul(4)?];
            for y in 0..nh {
                for x in 0..nw {
                    let (ux, uy) = cc_io_takeoff::pdfread::to_pdf(&new_tr, (x as f64 + 0.5) / s, (y as f64 + 0.5) / s);
                    let o = cc_markup::geom::apply(&ti, [ux, uy]);
                    let (vx, vy) = cc_io_takeoff::pdfread::to_viewport(&old_tr, o[0], o[1]);
                    let (px, py) = ((vx * s).floor(), (vy * s).floor());
                    if px < 0.0 || py < 0.0 || px >= ow as f64 || py >= oh as f64 {
                        continue;
                    }
                    let si = ((py as usize) * ow + px as usize) * 4;
                    let di = (y * nw + x) * 4;
                    if let (Some(src), Some(dst)) = (ob.get(si..si + 4), moved.get_mut(di..di + 4)) {
                        dst.copy_from_slice(src);
                    }
                }
            }
            self.cmp.pic = Some(PicCache { key, size: [nw, nh], old: moved, new: nb, balance: f64::NAN, tex: None });
        }
        let pic = self.cmp.pic.as_mut()?;
        if pic.tex.is_none() || pic.balance != balance {
            let colors = cc_markup::compare::MixColors {
                bg: [255.0, 255.0, 255.0],
                same: [140.0, 140.0, 140.0],
                old: [208.0, 2.0, 27.0],
                new: [30.0, 72.0, 200.0],
            };
            let mut out = vec![0u8; pic.new.len()];
            cc_markup::compare::combine(&pic.old, &pic.new, &mut out, &colors, balance);
            let img = egui::ColorImage::from_rgba_unmultiplied(pic.size, &out);
            match pic.tex.as_mut() {
                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                None => pic.tex = Some(ctx.load_texture("mc-compare", img, egui::TextureOptions::LINEAR)),
            }
            pic.balance = balance;
        }
        pic.tex.clone()
    }

    /// The Compare tab.
    pub fn compare_panel(&mut self, ui: &mut egui::Ui, tok: &crate::theme::Tokens) {
        ui.label(egui::RichText::new(t0("compare.h")).strong().color(tok.text_strong));
        ui.label(egui::RichText::new(t0("compare.note")).small().color(tok.text_dim));
        let mut sheets: Vec<(String, String)> = Vec::new();
        let several = !self.others.is_empty();
        for f in self.files() {
            for (i, k) in f.page_keys.iter().enumerate() {
                let l = f.title(&self.store, i);
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
        let mut act: Option<String> = None;
        ui.horizontal_wrapped(|ui| {
            if ui.button(t0("compare.start")).clicked() {
                act = Some("start".into());
            }
            if self.cmp.on && ui.button(t0("compare.stop")).clicked() {
                act = Some("stop".into());
            }
        });
        if self.cmp.on {
            ui.horizontal_wrapped(|ui| {
                for (m, k) in [("picture", "compare.mode.picture"), ("lines", "compare.mode.lines")] {
                    if ui.radio(self.cmp.mode == m, t0(k)).clicked() {
                        act = Some(format!("mode:{m}"));
                    }
                }
            });
            if ui.button(t0("compare.cloud")).on_hover_text(t0("compare.cloud.title")).clicked() {
                act = Some("cloud".into());
            }
            let l = ui.label(egui::RichText::new(t0("compare.balance")).small());
            let mut b = self.cmp.balance * 100.0;
            if ui.add(egui::Slider::new(&mut b, 0.0..=100.0).show_value(false)).labelled_by(l.id).changed() {
                self.cmp.balance = b / 100.0;
            }
            ui.label(egui::RichText::new(t0("compare.balance.note")).small().color(tok.text_dim));
            ui.label(egui::RichText::new(t0("compare.lineup")).small());
            ui.horizontal_wrapped(|ui| {
                for (d, w) in [("-1,0", "left"), ("0,1", "up"), ("0,-1", "down"), ("1,0", "right")] {
                    let arrow = match w {
                        "left" => "←",
                        "up" => "↑",
                        "down" => "↓",
                        _ => "→",
                    };
                    if ui.button(arrow).on_hover_text(t0(&format!("compare.nudge.{w}.title"))).clicked() {
                        act = Some(format!("nudge:{d}"));
                    }
                }
                egui::ComboBox::new("mc-cmp-step", "")
                    .selected_text(t("compare.step.pt", &[("n", Fill::S(cc_markup::js::ns(self.cmp.step)))]))
                    .show_ui(ui, |ui| {
                        for s in [0.25, 1.0, 5.0, 20.0] {
                            ui.selectable_value(&mut self.cmp.step, s, t("compare.step.pt", &[("n", Fill::S(cc_markup::js::ns(s)))]));
                        }
                    });
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button(t0("compare.align")).clicked() {
                    act = Some("align".into());
                }
                if ui.button(t0("compare.reset")).clicked() {
                    act = Some("reset".into());
                }
            });
            if self.cmp.mode == "lines" {
                ui.label(egui::RichText::new(t0("compare.legend.old")).color(ui.visuals().error_fg_color));
                ui.label(egui::RichText::new(t0("compare.legend.new")).color(tok.accent));
            } else {
                ui.label(egui::RichText::new(t0("compare.legend.old")).color(egui::Color32::from_rgb(208, 2, 27)));
                ui.label(egui::RichText::new(t0("compare.legend.new")).color(egui::Color32::from_rgb(30, 72, 200)));
                ui.label(egui::RichText::new(t0("compare.legend.same")).color(egui::Color32::from_gray(140)));
            }
        }
        if !self.cmp.status.is_empty() {
            ui.label(egui::RichText::new(&self.cmp.status).color(tok.text_dim));
        }
        // Compare two sets
        if several {
            ui.separator();
            ui.label(egui::RichText::new(t0("setcmp.h")).strong().color(tok.text_strong));
            ui.label(egui::RichText::new(t0("setcmp.note")).small().color(tok.text_dim));
            let files: Vec<(String, String)> = self.files().iter().map(|f| (f.id.clone(), f.name.clone())).collect();
            if self.cmp.set_old.is_empty() {
                self.cmp.set_old = files.first().map(|f| f.0.clone()).unwrap_or_default();
            }
            if self.cmp.set_new.is_empty() {
                self.cmp.set_new = files.last().map(|f| f.0.clone()).unwrap_or_default();
            }
            for (id, label, slot) in [("mc-set-old", "setcmp.old", 0), ("mc-set-new", "setcmp.new", 1)] {
                let cur = if slot == 0 { self.cmp.set_old.clone() } else { self.cmp.set_new.clone() };
                let shown = files.iter().find(|f| f.0 == cur).map(|f| f.1.clone()).unwrap_or_default();
                let l = ui.label(egui::RichText::new(t0(label)).small());
                let mut pickf = cur.clone();
                egui::ComboBox::new(id, "")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for (fid, name) in &files {
                            ui.selectable_value(&mut pickf, fid.clone(), name);
                        }
                    })
                    .response
                    .labelled_by(l.id);
                if slot == 0 {
                    self.cmp.set_old = pickf;
                } else {
                    self.cmp.set_new = pickf;
                }
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button(t0("setcmp.run")).clicked() {
                    act = Some("set".into());
                }
                if self.cmp.set_rows.iter().any(|r| r.len >= 6.0) && ui.button(t0("setcmp.cloud")).clicked() {
                    act = Some("set-cloud".into());
                }
            });
            if !self.cmp.set_status.is_empty() {
                ui.label(egui::RichText::new(&self.cmp.set_status).color(tok.text_dim));
            }
            let mut open: Option<(String, String)> = None;
            for r in &self.cmp.set_rows {
                let what = if r.len >= 6.0 { t("setcmp.row", &[("n", Fill::N(r.len))]) } else { t0("setcmp.same") };
                if ui.link(format!("{} {what}", self.sheet_title(&r.new_key))).clicked() {
                    open = Some((r.old_key.clone(), r.new_key.clone()));
                }
            }
            if let Some((o, n)) = open {
                if let Some((fid, i)) = self.locate(&n).map(|(f, i)| (f.id.clone(), i)) {
                    self.go_file(&fid, i);
                }
                self.cmp.mode = "lines";
                let _ = self.compare_start(&o);
            }
        }
        if let Some(a) = act {
            let r = match a.as_str() {
                "start" => self.compare_start(&pick),
                "stop" => {
                    self.compare_stop();
                    Ok(())
                }
                "cloud" => self.compare_cloud(),
                "align" => {
                    self.compare_align();
                    Ok(())
                }
                "reset" => self.compare_reset(),
                "set" => {
                    let (o, n) = (self.cmp.set_old.clone(), self.cmp.set_new.clone());
                    self.compare_sets(&o, &n)
                }
                "set-cloud" => self.cloud_set(),
                m if m.starts_with("mode:") => self.compare_mode(m.trim_start_matches("mode:")),
                n => {
                    let v: Vec<f64> = n.trim_start_matches("nudge:").split(',').filter_map(|x| x.parse().ok()).collect();
                    match v[..] {
                        [x, y] => self.compare_nudge(x, y),
                        _ => Ok(()),
                    }
                }
            };
            if let Err(e) = r {
                self.tools.message = e;
            }
        }
    }

    /// The changed lines over the sheet (lines mode, when the open sheet is the compare's new one), and the align marks.
    pub fn draw_compare(&self, painter: &egui::Painter, at: &dyn Fn(f64, f64) -> egui::Pos2, old: egui::Color32, new: egui::Color32) {
        if let Some(pts) = &self.cmp.aligning {
            for (i, p) in pts.iter().enumerate() {
                let c = at(p[0], p[1]);
                let col = if i < 2 { old } else { new };
                painter.line_segment([c - egui::vec2(8.0, 0.0), c + egui::vec2(8.0, 0.0)], egui::Stroke::new(2.0, col));
                painter.line_segment([c - egui::vec2(0.0, 8.0), c + egui::vec2(0.0, 8.0)], egui::Stroke::new(2.0, col));
            }
        }
        if !self.cmp.on || self.cmp.mode != "lines" || self.cmp.new_key != self.sheet_key() {
            return;
        }
        let Some(d) = self.cmp.vec.as_ref() else { return };
        for (list, c) in [(&d.removed, old), (&d.added, new)] {
            for s in list {
                painter.line_segment([at(s[0], s[1]), at(s[2], s[3])], egui::Stroke::new(2.0, c));
            }
        }
    }
}
