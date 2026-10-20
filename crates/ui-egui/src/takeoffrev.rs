//! TakeoffCraft's revision overlay (T13), as takeoffcraft.js has it: an older sheet over the one on screen (its ink red
//! and see-through), lined up by two point pairs, the changes found by comparing both sheets' ink (overlay.js on the
//! rasters, removed red, added blue), the old sheet's takeoff moved here in one undo step with the marks the changes
//! touch flagged for review (an orange halo, "N to check", next flag, Checked). The rasters are drawn by hayro where
//! 0.5 draws them by pdf.js, so the pixel counts in the status line differ from 0.5's; the marks found are the same on
//! the test sheets (tkopsdiff `revs`).

use std::collections::HashSet;

use cc_takeoff::geom::{self as G, M6, P};
use cc_takeoff::i18n::{F, t, tf, tn};
use cc_takeoff::overlay::{self as O, IDENTITY};
use egui::{Color32, Pos2, Vec2};
use serde_json::{Value, json};

use crate::pdfview::PdfView;
use crate::takeofftools::TakeoffMode;

/// Removed (old only) red, added (new only) blue.
const OLD_RGB: [u8; 3] = [214, 39, 40];
const NEW_RGB: [u8; 3] = [31, 119, 180];

/// The changes found: the marks of the old sheet they touch (in the old sheet's order), the pixel counts, the mask
/// (new sheet's display space at `px` pixels per unit).
#[derive(Debug, Clone, Default)]
pub struct Chg {
    pub flagged: Vec<String>,
    pub removed: usize,
    pub added: usize,
    pub mask: Vec<u8>,
    pub w: usize,
    pub h: usize,
    pub px: f64,
}

/// The revision overlay: the older sheet, the one it is compared with, the alignment (old user space → this sheet's),
/// the alignment picks under way, the changes found.
#[derive(Debug, Clone)]
pub struct Cmp {
    pub old: String,
    pub sheet: String,
    pub a: M6,
    pub align: Option<Vec<P>>,
    pub chg: Option<Chg>,
}

fn s(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(x)) => x.clone(),
        _ => String::new(),
    }
}

/// `n.toLocaleString()` for a count (en: thousands with commas).
fn grouped(n: usize) -> String {
    let d = n.to_string();
    let mut out = String::new();
    for (i, c) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

impl TakeoffMode {
    /// The marks on a sheet, in the takeoff's order.
    fn marks_on(&self, sheet: &str) -> Vec<Value> {
        self.tk.marks().iter().filter(|m| m.get("sheet").and_then(Value::as_str) == Some(sheet)).cloned().collect()
    }

    /// startCompare: the older sheet over this one (Select is the tool while comparing).
    pub fn compare_start(&mut self, old: &str) {
        self.end_paste(true);
        self.cancel_draft();
        self.ac = None;
        if self.tool != "select" {
            self.set_tool("select");
        }
        self.cmp = Some(Cmp { old: old.to_string(), sheet: self.cur_sheet.clone(), a: IDENTITY, align: None, chg: None });
    }

    /// startAlign: the next four clicks are old, new, old, new.
    pub fn compare_align(&mut self) {
        if let Some(c) = self.cmp.as_mut() {
            c.align = Some(Vec::new());
            self.message = t("cmp.align.step1");
        }
    }

    /// alignPick: an old pick is taken back through the current alignment into old user space.
    pub(crate) fn align_pick(&mut self, p: P) {
        let Some(c) = self.cmp.as_mut() else { return };
        let Some(picks) = c.align.as_mut() else { return };
        let q = if picks.len() % 2 == 0 { G::apply(&G::invert(&c.a), &p) } else { p };
        picks.push(q);
        if picks.len() < 4 {
            self.message = t(&format!("cmp.align.step{}", picks.len() + 1));
            return;
        }
        let (Some(o1), Some(n1), Some(o2), Some(n2)) = (picks.first(), picks.get(1), picks.get(2), picks.get(3)) else { return };
        let fit = O::fit_two_points(o1, o2, n1, n2);
        c.align = None;
        let Some(a) = fit else {
            self.message = t("cmp.align.bad");
            return;
        };
        c.a = a;
        c.chg = None;
        self.message = tf("cmp.align.done", &[("scale", F::S(format!("{:.3}", O::scale_of(&a))))]);
    }

    /// moveTakeoff with the changes found: the old sheet's marks come here (one undo step), the touched ones flagged,
    /// the old sheet's scale carried when this sheet has none.
    pub fn compare_move(&mut self, chg: &Chg, user_unit: f64) {
        let Some(c) = self.cmp.clone() else { return };
        let ids: Vec<String> = self.marks_on(&c.old).iter().map(|m| s(m.get("id"))).collect();
        let sa = O::scale_of(&c.a);
        let as_is = self.tk.scale_of(&c.old).cloned().map(|old| {
            if (sa - 1.0).abs() < 1e-6 {
                old
            } else {
                let fpu = old.get("ftPerUnit").and_then(Value::as_f64).unwrap_or(0.0) / sa;
                json!({ "method": "calibrated", "ftPerUnit": fpu, "preset": null, "label": cc_takeoff::units::describe_scale(fpu, user_unit), "cal": null })
            }
        });
        let a = c.a;
        let mv = move |pts: &[Value]| -> Vec<Value> {
            let ps: Vec<P> =
                pts.iter().map(|p| cc_takeoff::model::pts_of(Some(&json!([p]))).first().copied().unwrap_or_default()).collect();
            O::move_pts(&ps, &a).iter().map(crate::takeofftools::pt_json).collect()
        };
        let flagged: HashSet<String> = chg.flagged.iter().cloned().collect();
        match self.tk.carry_marks(&ids, &c.sheet, &mv, &flagged, as_is.as_ref(), None) {
            Ok(n) => {
                self.set_sel(None);
                self.cmp = None;
                #[allow(clippy::cast_precision_loss)]
                let f = chg.flagged.len() as f64;
                self.message = format!("{} {}", tn("cmp.moved", n), tn("cmp.flagged", f));
            }
            Err(e) => self.message = e,
        }
    }

    /// The scales the move would put side by side (cmp.scale.text), when they differ by more than 0.1 %.
    pub fn compare_scales_differ(&self, user_unit: f64) -> Option<(String, String)> {
        let c = self.cmp.as_ref()?;
        let old = self.tk.scale_of(&c.old)?;
        let sa = O::scale_of(&c.a);
        let fpu = old.get("ftPerUnit").and_then(Value::as_f64)? / sa;
        let new = self.tk.scale_of(&c.sheet)?;
        let nf = new.get("ftPerUnit").and_then(Value::as_f64)?;
        if ((nf - fpu) / fpu).abs() <= 0.001 {
            return None;
        }
        let old_text = if (sa - 1.0).abs() < 1e-6 {
            crate::takeoffedit::scale_text(old, user_unit)
        } else {
            let rec = json!({ "method": "calibrated", "ftPerUnit": fpu });
            crate::takeoffedit::scale_text(&rec, user_unit)
        };
        Some((old_text, crate::takeoffedit::scale_text(new, user_unit)))
    }

    /// nextFlag: the flagged mark after the selected one (or the first), selected and shown.
    pub fn next_flag(&mut self) {
        let list: Vec<Value> =
            self.tk.marks().iter().filter(|m| m.get("flag").is_some_and(|f| !f.is_null() && f.as_bool() != Some(false))).cloned().collect();
        if list.is_empty() {
            self.message = t("flag.none");
            return;
        }
        let at = list.iter().position(|m| self.sel.as_deref() == m.get("id").and_then(Value::as_str));
        let i = at.map_or(0, |i| (i + 1) % list.len());
        let Some(m) = list.get(i) else { return };
        let sheet = s(m.get("sheet"));
        if sheet != self.cur_sheet {
            self.goto = Some(sheet);
        }
        if self.tool != "select" {
            self.set_tool("select");
        }
        self.set_sel(Some(s(m.get("id"))));
        self.item = Some(s(m.get("item")));
        #[allow(clippy::cast_precision_loss)]
        let fills = [("i", F::N((i + 1) as f64)), ("n", F::N(list.len() as f64))];
        self.message = tf("flag.at", &fills);
    }

    /// The compare bar's line (renderCmp).
    pub fn compare_summary(&self, title: &dyn Fn(&str) -> String) -> Option<String> {
        let c = self.cmp.as_ref()?;
        let mut line = tf("cmp.summary", &[("old", F::S(title(&c.old)))]);
        if let Some(chg) = &c.chg {
            #[allow(clippy::cast_precision_loss)]
            let fills = [("n", F::N(self.marks_on(&c.old).len() as f64)), ("f", F::N(chg.flagged.len() as f64))];
            line = format!("{line} {}", tf("cmp.summary.changes", &fills));
        }
        Some(line)
    }

    /// The alignment picks (old ones through the alignment, red; new ones dark).
    pub(crate) fn draw_compare(&self, painter: &egui::Painter, sheet: &str, at: &dyn Fn(f64, f64) -> Pos2) {
        let Some(c) = self.cmp.as_ref().filter(|c| c.sheet == sheet) else { return };
        for (i, q) in c.align.iter().flatten().enumerate() {
            let p = if i % 2 == 0 { G::apply(&c.a, q) } else { *q };
            let s0 = at(p.x, p.y);
            let col = if i % 2 == 0 { Color32::from_rgb(OLD_RGB[0], OLD_RGB[1], OLD_RGB[2]) } else { Color32::from_rgb(0x17, 0x20, 0x2b) };
            let st = egui::Stroke::new(2.0, col);
            painter.circle_stroke(s0, 7.0, st);
            painter.line_segment([s0 - Vec2::new(11.0, 0.0), s0 + Vec2::new(11.0, 0.0)], st);
            painter.line_segment([s0 - Vec2::new(0.0, 11.0), s0 + Vec2::new(0.0, 11.0)], st);
        }
    }

    /// The flagged marks (the sheet bar's "N to check").
    pub fn flag_count(&self) -> usize {
        self.tk.marks().iter().filter(|m| m.get("flag").is_some_and(|f| !f.is_null() && f.as_bool() != Some(false))).count()
    }
}

impl PdfView {
    /// A sheet key ("<hash>:<page>") from a file id and page ("f1:0"), or the key itself.
    fn sheet_of(&self, spec: &str) -> Option<String> {
        let (fid, page) = spec.split_once(':')?;
        let f = self.files().into_iter().find(|f| f.id == fid || f.hash() == fid)?;
        let i: usize = page.parse().ok()?;
        (i < f.pages.len()).then(|| cc_takeoff::model::sheet_key(f.hash(), i as f64))
    }

    /// The file and page of a sheet key.
    fn page_of(&self, key: &str) -> Option<(&crate::pdfview::PdfFile, usize)> {
        let (hash, idx) = key.split_once(':')?;
        let f = self.files().into_iter().find(|f| f.hash() == hash)?;
        let i: usize = idx.parse().ok()?;
        f.pages.get(i).map(|_| (f, i))
    }

    /// The sheet's ink rendered at `px` pixels per unit (rasterize + toInk).
    fn ink_of(&self, key: &str, px: f64) -> Result<cc_takeoff::matcher::Img, String> {
        let (f, i) = self.page_of(key).ok_or("no such sheet")?;
        cc_io_takeoff::pdfread::render_ink(&f.doc, i, px)
    }

    /// `takeoff.compare`: `<file>:<page>` (the older sheet, e.g. "f1:0"), `align`, `find`, `move`, `close`.
    pub fn takeoff_compare(&mut self, op: &str) -> Result<(), String> {
        self.sync_takeoff();
        match op {
            "close" => {
                self.takeoff.cmp = None;
                Ok(())
            }
            "align" => {
                self.takeoff.compare_align();
                Ok(())
            }
            "find" => {
                self.takeoff_find_changes();
                Ok(())
            }
            "move" => {
                let Some(c) = self.takeoff.cmp.clone() else { return Ok(()) };
                if self.takeoff.marks_on(&c.old).is_empty() {
                    self.takeoff.message = t("cmp.nomarks");
                    return Ok(());
                }
                let chg = match c.chg.clone() {
                    Some(x) => x,
                    None => match self.takeoff_find_changes() {
                        Some(x) => x,
                        None => return Ok(()),
                    },
                };
                let uu = self.page_of(&c.sheet).and_then(|(f, i)| f.pages.get(i)).map_or(1.0, |p| p.user_unit);
                self.takeoff.compare_move(&chg, uu);
                Ok(())
            }
            spec => {
                let others = self.files().iter().map(|f| f.pages.len()).sum::<usize>() > 1;
                if !others {
                    self.takeoff.message = t("cmp.pick.none");
                    return Ok(());
                }
                let old = self.sheet_of(spec).ok_or_else(|| format!("takeoff.compare: no sheet {spec} (file:page, e.g. f1:0)"))?;
                if old == self.takeoff.cur_sheet {
                    self.takeoff.message = t("cmp.same_sheet");
                    return Ok(());
                }
                self.takeoff.compare_start(&old);
                self.tk_tint = None;
                Ok(())
            }
        }
    }

    /// findChanges: both sheets as ink in this sheet's display space, compared; the old sheet's marks the changes touch.
    pub fn takeoff_find_changes(&mut self) -> Option<Chg> {
        let c = self.takeoff.cmp.clone()?;
        let (Some((nf, ni)), Some((of, oi))) = (self.page_of(&c.sheet), self.page_of(&c.old)) else { return None };
        let (Some(np), Some(op)) = (nf.pages.get(ni), of.pages.get(oi)) else { return None };
        let px = (12e6 / (np.width * np.height)).sqrt().min(2.0);
        let px_old = (12e6 / (op.width * op.height)).sqrt().min(2.0);
        let (nt, ot) = (np.transform, op.transform);
        let nw = match self.ink_of(&c.sheet, px) {
            Ok(x) => x,
            Err(e) => {
                self.takeoff.message = e;
                return None;
            }
        };
        let ol = match self.ink_of(&c.old, px_old) {
            Ok(x) => x,
            Err(e) => {
                self.takeoff.message = e;
                return None;
            }
        };
        let m = O::pixel_map(&nt, &ot, &c.a, 1.0);
        let mpx = G::multiply(&[px_old, 0.0, 0.0, px_old, 0.0, 0.0], &G::multiply(&m, &[1.0 / px, 0.0, 0.0, 1.0 / px, 0.0, 0.0]));
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let r = (px * 1.5).round().max(1.0) as usize;
        let cm = O::change_mask(&O::warp(&ol, &mpx, nw.w, nw.h), &nw, r, 0.35, 0.2);
        let a = c.a;
        let to_px = |q: &P| {
            let d = G::apply(&nt, &G::apply(&a, q));
            P::new(d.x * px, d.y * px)
        };
        let mut flagged = Vec::new();
        for mk in self.takeoff.marks_on(&c.old) {
            let kind = s(mk.get("kind"));
            let pts = cc_takeoff::model::pts_of(mk.get("pts"));
            let holes = cc_takeoff::model::holes_of(mk.get("holes"));
            if O::changed(&O::mark_change(&kind, &pts, &holes, &cm, &to_px, (px * 4.0).max(3.0)), &kind, 0.15) {
                flagged.push(s(mk.get("id")));
            }
        }
        self.takeoff.message = if cm.removed + cm.added > 0 {
            #[allow(clippy::cast_precision_loss)]
            let n = flagged.len() as f64;
            let found = tf("cmp.found", &[("removed", F::S(grouped(cm.removed))), ("added", F::S(grouped(cm.added)))]);
            format!("{found} {}", tn("cmp.flagged.found", n))
        } else {
            t("cmp.same")
        };
        let chg = Chg { flagged, removed: cm.removed, added: cm.added, mask: cm.mask, w: cm.w, h: cm.h, px };
        if let Some(cc) = self.takeoff.cmp.as_mut() {
            cc.chg = Some(chg.clone());
        }
        self.tk_mask = None;
        Some(chg)
    }

    /// `takeoff.flag`: next | checked (the selected mark).
    pub fn takeoff_flag(&mut self, op: &str) -> Result<(), String> {
        match op {
            "next" => {
                self.takeoff.next_flag();
                self.sync_takeoff();
                Ok(())
            }
            _ => Err(format!("takeoff.flag: unknown {op} (next)")),
        }
    }

    /// The old sheet's ink, red and see-through (tintOf), and the changes found, drawn under the marks.
    pub(crate) fn draw_takeoff_compare(&mut self, ctx: &egui::Context, painter: &egui::Painter, origin: Pos2) {
        let Some(c) = self.takeoff.cmp.clone().filter(|c| c.sheet == self.takeoff.cur_sheet) else { return };
        let Some(info) = self.cur.pages.get(self.page).cloned() else { return };
        let z = self.zoom;
        let at = |x: f64, y: f64| {
            let (vx, vy) = cc_io_takeoff::pdfread::to_viewport(&info.transform, x, y);
            origin + Vec2::new((vx * z) as f32, (vy * z) as f32)
        };
        if self.tk_tint.as_ref().is_none_or(|(k, _, _, _)| *k != c.old) {
            let Some((f, i)) = self.page_of(&c.old) else { return };
            let Some(op) = f.pages.get(i) else { return };
            let px = (4096.0 / op.width.max(op.height)).min(2.0);
            let tr = op.transform;
            if let Ok((rgba, w, h)) = cc_io_takeoff::pdfread::render_rgba(&f.doc, i, px, true) {
                let mut img = egui::ColorImage::new([w, h], vec![Color32::TRANSPARENT; w * h]);
                for (k, &[r, g, b, _]) in rgba.as_chunks::<4>().0.iter().enumerate() {
                    let ink = 1.0 - (0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b)) / 255.0;
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let a = ((ink * 1.2).min(1.0) * 230.0).round() as u8;
                    if let Some(p) = img.pixels.get_mut(k) {
                        *p = Color32::from_rgba_unmultiplied(OLD_RGB[0], OLD_RGB[1], OLD_RGB[2], a);
                    }
                }
                let tex = ctx.load_texture("tk-tint", img, egui::TextureOptions::LINEAR);
                self.tk_tint = Some((c.old.clone(), tex, px, tr));
            }
        }
        if let Some((_, tex, px, tr)) = &self.tk_tint {
            // screen ← this sheet's user space ← (A) ← the old sheet's user space ← its display ← its raster pixels
            let [w, h] = tex.size();
            let inv = G::invert(tr);
            let corner = |u: f64, v: f64| {
                let p = G::apply(&c.a, &G::apply(&inv, &P::new(u / px, v / px)));
                at(p.x, p.y)
            };
            #[allow(clippy::cast_precision_loss)]
            let (wf, hf) = (w as f64, h as f64);
            let mut mesh = egui::Mesh::with_texture(tex.id());
            for (u, v, uv) in [
                (0.0, 0.0, Pos2::new(0.0, 0.0)),
                (wf, 0.0, Pos2::new(1.0, 0.0)),
                (wf, hf, Pos2::new(1.0, 1.0)),
                (0.0, hf, Pos2::new(0.0, 1.0)),
            ] {
                mesh.vertices.push(egui::epaint::Vertex { pos: corner(u, v), uv, color: Color32::WHITE });
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            painter.add(egui::Shape::mesh(mesh));
        }
        if let Some(chg) = c.chg.as_ref() {
            if self.tk_mask.is_none() && chg.w > 0 && chg.h > 0 {
                let mut img = egui::ColorImage::new([chg.w, chg.h], vec![Color32::TRANSPARENT; chg.w * chg.h]);
                for (k, v) in chg.mask.iter().enumerate() {
                    let rgb = match v {
                        1 => OLD_RGB,
                        2 => NEW_RGB,
                        _ => continue,
                    };
                    if let Some(p) = img.pixels.get_mut(k) {
                        *p = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                    }
                }
                self.tk_mask = Some(ctx.load_texture("tk-chg", img, egui::TextureOptions::NEAREST));
            }
            if let Some(tex) = &self.tk_mask {
                #[allow(clippy::cast_precision_loss)]
                let size = Vec2::new((chg.w as f64 / chg.px * z) as f32, (chg.h as f64 / chg.px * z) as f32);
                painter.image(
                    tex.id(),
                    egui::Rect::from_min_size(origin, size),
                    egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
    }

    /// The Compare part of the panel: the older sheet to compare with, the bar (Align by two points, Find changes, Move
    /// takeoff here, Close) with its summary, and the flagged marks ("N to check", next).
    pub(crate) fn takeoff_compare_panel(&mut self, ui: &mut egui::Ui, tok: &crate::theme::Tokens) {
        use egui::RichText;
        let mut op: Option<String> = None;
        let titles: Vec<(String, String)> = self
            .files()
            .iter()
            .flat_map(|f| (0..f.pages.len()).map(move |i| (format!("{}:{i}", f.id), cc_takeoff::model::sheet_key(f.hash(), i as f64))))
            .collect();
        let label = |me: &Self, key: &str| me.takeoff.sheet_label(key);
        match self.takeoff.cmp.clone() {
            None => {
                ui.horizontal_wrapped(|ui| {
                    let cur = self.takeoff.cur_sheet.clone();
                    let others: Vec<&(String, String)> = titles.iter().filter(|(_, k)| *k != cur).collect();
                    ui.menu_button(t("cmp.button"), |ui| {
                        if others.is_empty() {
                            ui.label(t("cmp.pick.none"));
                        }
                        for (spec, key) in &others {
                            if ui.button(label(self, key)).clicked() {
                                op = Some(spec.clone());
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text(t("cmp.button.title"));
                });
            }
            Some(c) => {
                let title = |k: &str| self.takeoff.sheet_label(k);
                if let Some(line) = self.takeoff.compare_summary(&title) {
                    ui.label(RichText::new(line).color(tok.text_dim));
                }
                if let Some((o, n)) = self.tk_scale_ask.clone() {
                    ui.label(RichText::new(t("cmp.scale.title")).strong());
                    ui.label(tf("cmp.scale.text", &[("old", F::S(o)), ("new", F::S(n))]));
                    ui.horizontal(|ui| {
                        if ui.button(t("cmp.scale.cancel")).clicked() {
                            self.tk_scale_ask = None;
                        }
                        if ui.button(t("cmp.scale.keep")).clicked() {
                            self.tk_scale_ask = None;
                            op = Some("move".into());
                        }
                    });
                } else {
                    ui.horizontal_wrapped(|ui| {
                        for (k, key) in [("align", "cmp.align"), ("find", "cmp.find"), ("move?", "cmp.move"), ("close", "cmp.close")] {
                            if ui.small_button(t(key)).clicked() {
                                op = Some(k.into());
                            }
                        }
                    });
                }
                let _ = c;
            }
        }
        let n = self.takeoff.flag_count();
        if n > 0 {
            #[allow(clippy::cast_precision_loss)]
            let text = tn("flag.count", n as f64);
            if ui.small_button(text).on_hover_text(t("flag.next.title")).clicked() {
                op = Some("flag".into());
            }
        }
        match op.as_deref() {
            Some("flag") => {
                let _ = self.takeoff_flag("next");
            }
            Some("move?") => {
                let uu = self.cur.pages.get(self.page).map_or(1.0, |p| p.user_unit);
                match self.takeoff.compare_scales_differ(uu) {
                    Some(pair) => self.tk_scale_ask = Some(pair),
                    None => {
                        let _ = self.takeoff_compare("move");
                    }
                }
            }
            Some(o) => {
                if let Err(e) = self.takeoff_compare(o) {
                    self.takeoff.message = e;
                }
            }
            None => {}
        }
    }
}
