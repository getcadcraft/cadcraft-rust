//! DetailCraft's Drawings dialog (detailcraft.js `openDrawings` … `issueSheets`, ui.inc `data-dwg`): the GA plans and
//! elevations and the assembly drawings made from the model when it opens, a sheet list with each sheet's revision
//! tag, the sheet size, a preview of the chosen sheet (detailcraft.js `drawList`: white sheet, black ink, y up), the
//! issue log (0.4 D17: the sheet's status, a description, Issue this sheet, Issue the n changed) and the downloads
//! (this sheet / all sheets as PDF, DWG 2000…2018 or DXF), which run `detail.export` as the bar did.
//!
//! Headless: `detail.drawings` = `open` | `close` | `sheet:<i>` | `note:<text>` | `issue:one[@ms]` | `issue:all[@ms]`
//! (the issue date is the UTC day of that time, else now, as `new Date().toISOString()`).

use cc_detail::drawings::{Set, Sheet};
use cc_detail::i18n::{F, t, t0};
use egui::{Color32, Pos2, Rect, RichText, Stroke};
use serde_json::Value;

use crate::detailpanel::Act;
use crate::detailview::DetailView;
use crate::theme::Tokens;

/// The open dialog (detailcraft.js `dwgSet` and its fields).
pub struct DwgDlg {
    pub set: Set,
    /// the sheet shown
    pub at: usize,
    /// the issue's description (`data-issue-note`, at most 60 characters)
    pub note: String,
}

fn sheet_refs(set: &Set) -> Vec<cc_detail::issues::SheetRef<'_>> {
    set.sheets
        .iter()
        .map(|s| cc_detail::issues::SheetRef { id: &s.id, number: &s.number, hash: &s.hash, prints: Some(&s.prints) })
        .collect()
}

fn val(v: &Value) -> String {
    v.as_str().map_or_else(|| cc_detail::js::to_js_string(v), str::to_string)
}

impl DetailView {
    /// detailcraft.js `makeDrawings()` on the day of `now` (ms).
    fn make_drawings(&mut self, now: f64) -> Set {
        self.recompute();
        let date = crate::detailexport::Clock::of_ms(now).date();
        let t1 = web_time::Instant::now();
        let set = cc_detail::drawings::drawing_set(
            &self.model.state,
            &self.built,
            &self.marks,
            Some(&self.footing_marks),
            &date,
            &self.settings.paper,
            true,
        );
        self.set_timing("drawings", t1.elapsed().as_secs_f64() * 1000.0);
        set
    }

    /// detailcraft.js `fillDrawings(at)`: the set made again, the sheet at `at` kept (false: nothing to draw, said).
    fn fill_drawings(&mut self, at: usize, now: f64) -> bool {
        let set = self.make_drawings(now);
        if set.sheets.is_empty() {
            self.message = t0("app.dwg.empty");
            self.dwg = None;
            return false;
        }
        let at = at.min(set.sheets.len().saturating_sub(1));
        let note = self.dwg.as_ref().map_or_else(|| t0("issue.note.default"), |d| d.note.clone());
        self.dwg = Some(DwgDlg { set, at, note });
        true
    }

    /// The issue statuses of the open set (sheet order).
    pub fn dwg_status(&self) -> Vec<(String, cc_detail::issues::Status)> {
        let Some(d) = &self.dwg else { return Vec::new() };
        cc_detail::issues::sheet_status(&sheet_refs(&d.set), cc_detail::js::arr(self.model.state.get("issues")))
    }

    /// A sheet's line in the list (`number — title (scale)` and its revision tag).
    pub fn dwg_label(&self, sh: &Sheet, st: &[(String, cc_detail::issues::Status)]) -> String {
        let tag = match st.iter().find(|(k, _)| *k == sh.id).map(|x| &x.1) {
            Some(x) if x.issued && x.changed => format!(" {}", t("issue.tag.changed", &[("rev", F::S(val(&x.rev)))])),
            Some(x) if x.issued => format!(" {}", t("issue.tag.rev", &[("rev", F::S(val(&x.rev)))])),
            _ => String::new(),
        };
        format!("{} — {} ({}){tag}", sh.number, sh.title, sh.scale)
    }

    /// detailcraft.js `issueSheets(ids)`.
    fn issue_sheets(&mut self, ids: &[String], now: f64) {
        let Some(d) = &self.dwg else { return };
        let date = Value::from(crate::detailexport::Clock::of_ms(now).date());
        let list = cc_detail::issues::plan_issue(
            &sheet_refs(&d.set),
            cc_detail::js::arr(self.model.state.get("issues")),
            ids,
            &date,
            &d.note,
            false,
        );
        if list.is_empty() {
            return;
        }
        let one = list.first().cloned();
        let n = list.len();
        let arr = Value::Array(list);
        if self.edit(&t0("issue.undo"), |tx| tx.add_issues(Some(&arr))).is_none() {
            return;
        }
        let at = self.dwg.as_ref().map_or(0, |d| d.at);
        self.fill_drawings(at, now);
        self.message = match (n, one) {
            (1, Some(x)) => t(
                "issue.done.one",
                &[("number", F::S(val(x.get("number").unwrap_or(&Value::Null)))), ("rev", F::S(val(x.get("rev").unwrap_or(&Value::Null))))],
            ),
            _ => t("issue.done.all", &[("n", F::N(n as f64))]),
        };
    }

    /// `detail.drawings`.
    pub fn drawings_cmd(&mut self, spec: &str, now: f64) -> Result<(), String> {
        let (spec, now) = match spec.rsplit_once('@') {
            Some((s, ms)) if s.starts_with("issue:") => {
                (s, ms.trim().parse::<f64>().ok().filter(|x| x.is_finite()).ok_or("detail.drawings: issue:…@<time ms>")?)
            }
            _ => (spec, now),
        };
        let (k, v) = spec.split_once(':').unwrap_or((spec, ""));
        match k {
            "open" => {
                self.recompute();
                if self.built.parts.is_empty() {
                    self.message = t0("app.dwg.nothing");
                    return Ok(());
                }
                self.dwg = None;
                self.fill_drawings(0, now);
            }
            "close" => self.dwg = None,
            _ if self.dwg.is_none() => return Err("detail.drawings: open the Drawings first (detail.drawings=open)".into()),
            "sheet" => {
                let i: usize = v.trim().parse().map_err(|_| "detail.drawings: sheet:<index>")?;
                let n = self.dwg.as_ref().map_or(0, |d| d.set.sheets.len());
                if i >= n {
                    return Err(format!("detail.drawings: there are {n} sheets"));
                }
                if let Some(d) = self.dwg.as_mut() {
                    d.at = i;
                }
            }
            "note" => {
                if let Some(d) = self.dwg.as_mut() {
                    d.note = cc_detail::js::slice16(v, 60);
                }
            }
            "paper" => {
                let before = self.settings.clone();
                self.settings.set(&format!("paper={v}"))?;
                self.setting_changed(&before);
                let at = self.dwg.as_ref().map_or(0, |d| d.at);
                self.fill_drawings(at, now);
            }
            "issue" => {
                let ids: Vec<String> = match v {
                    "one" => self.dwg.as_ref().and_then(|d| d.set.sheets.get(d.at)).map(|s| vec![s.id.clone()]).unwrap_or_default(),
                    "all" => {
                        let st = self.dwg_status();
                        self.dwg
                            .as_ref()
                            .map(|d| {
                                d.set
                                    .sheets
                                    .iter()
                                    .filter(|s| st.iter().any(|(k, x)| *k == s.id && x.changed))
                                    .map(|s| s.id.clone())
                                    .collect()
                            })
                            .unwrap_or_default()
                    }
                    o => return Err(format!("detail.drawings: issue:one | issue:all, not {o}")),
                };
                self.issue_sheets(&ids, now);
            }
            o => return Err(format!("detail.drawings: open | close | sheet:<i> | note:<text> | paper:<size> | issue:one|all, not {o}")),
        }
        Ok(())
    }
}

/// detailcraft.js `drawList(canvas, list)`: a sheet on white, fitted and centred, black ink, y up.
pub fn draw_sheet(painter: &egui::Painter, rect: Rect, sh: &Sheet) {
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    if !(sh.w > 0.0 && sh.h > 0.0) {
        return;
    }
    let k = (f64::from(rect.width()) / sh.w).min(f64::from(rect.height()) / sh.h);
    let (ox, oy) = (
        f64::from(rect.left()) + (f64::from(rect.width()) - sh.w * k) / 2.0,
        f64::from(rect.top()) + (f64::from(rect.height()) + sh.h * k) / 2.0,
    );
    let at = |p: [f64; 2]| Pos2::new((ox + p[0] * k) as f32, (oy - p[1] * k) as f32);
    let ink = Color32::BLACK;
    let pen = |w: f64| Stroke::new((w * k).max(0.5) as f32, ink);
    for it in sh.items.iter().take(200_000) {
        match it {
            cc_detail::display::Item::Path { pts, w, dash, closed, fill } => {
                let mut ps: Vec<Pos2> = pts.iter().map(|p| at(*p)).collect();
                if ps.len() < 2 {
                    continue;
                }
                if *fill && ps.len() >= 3 {
                    painter.add(egui::Shape::convex_polygon(ps, ink, Stroke::NONE));
                    continue;
                }
                if *closed && let Some(f) = ps.first().copied() {
                    ps.push(f);
                }
                match dash.as_ref().filter(|d| d.len() >= 2) {
                    Some(d) => {
                        let (a, b) = (d.first().copied().unwrap_or(1.0), d.get(1).copied().unwrap_or(1.0));
                        painter.extend(egui::Shape::dashed_line(&ps, pen(*w), ((a * k) as f32).max(1.0), ((b * k) as f32).max(1.0)));
                    }
                    None => {
                        painter.add(egui::Shape::line(ps, pen(*w)));
                    }
                }
            }
            cc_detail::display::Item::Circle { c, r, w, fill, .. } => {
                let (p, rr) = (at(*c), (r * k) as f32);
                if *fill == Some(true) {
                    painter.circle_filled(p, rr, ink);
                } else {
                    painter.circle_stroke(p, rr, pen(*w));
                }
            }
            cc_detail::display::Item::Text { s, x, y, h, align, rot, bold } => {
                let size = (h * k) as f32;
                if size < 1.5 {
                    continue;
                }
                let font =
                    if *bold { egui::FontId::new(size, egui::FontFamily::Name("bold".into())) } else { egui::FontId::proportional(size) };
                let font =
                    if painter.ctx().fonts(|f| f.families().contains(&font.family)) { font } else { egui::FontId::proportional(size) };
                let g = painter.layout_no_wrap(s.clone(), font, ink);
                let wdt = f64::from(g.size().x);
                let dx = match *align {
                    "center" => -wdt / 2.0,
                    "right" | "end" => -wdt,
                    _ => 0.0,
                };
                // the baseline at (x, y): the galley's top-left sits one ascent above it, turned with the text
                let asc = f64::from(g.rows.first().map_or(size * 0.8, |r| r.row.height() * 0.8));
                let (sn, cs) = (-rot).sin_cos();
                let base = at([*x, *y]);
                let (lx, ly) = (dx, -asc);
                let tl = Pos2::new(base.x + (lx * cs - ly * sn) as f32, base.y + (lx * sn + ly * cs) as f32);
                painter.add(egui::epaint::TextShape::new(tl, g, ink).with_angle(-*rot as f32));
            }
        }
    }
}

/// The Drawings dialog.
pub fn window(dv: &mut DetailView, ctx: &egui::Context, tk: &Tokens, acts: &mut Vec<Act>) {
    if dv.dwg.is_none() {
        return;
    }
    let mut open = true;
    let st = dv.dwg_status();
    let run = |acts: &mut Vec<Act>, id: &'static str, name: String| acts.push(Act::Run(id, name));
    crate::shell::Dialog::new(RichText::new(t0("page.dlg.drawings.title")).strong())
        .collapsible(false)
        .resizable(true)
        .default_width(900.0)
        .default_height(660.0)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .open(&mut open)
        .show(ctx, |ui| {
            let labels: Vec<String> =
                dv.dwg.as_ref().map(|d| d.set.sheets.iter().map(|s| dv.dwg_label(s, &st)).collect()).unwrap_or_default();
            let Some(d) = dv.dwg.as_mut() else { return };
            let at = d.at;
            // the head: the sheet and the sheet size
            ui.horizontal_wrapped(|ui| {
                let l = ui.label(RichText::new(t0("page.dlg.drawings.sheet")).color(tk.text_dim));
                let mut pick = at;
                egui::ComboBox::new("dc_dwg_sheet", "")
                    .selected_text(labels.get(at).cloned().unwrap_or_default())
                    .width(420.0)
                    .height(420.0)
                    .show_ui(ui, |ui| {
                        for (i, s) in labels.iter().enumerate() {
                            ui.selectable_value(&mut pick, i, s);
                        }
                    })
                    .response
                    .labelled_by(l.id);
                if pick != at {
                    run(acts, "detail.drawings", format!("sheet:{pick}"));
                }
                let pl = ui.label(RichText::new(t0("dwg.paper.label")).color(tk.text_dim));
                let mut paper = d.set.paper.to_string();
                egui::ComboBox::new("dc_dwg_paper", "")
                    .selected_text(cc_detail::paper::paper_label(&paper))
                    .show_ui(ui, |ui| {
                        for k in cc_detail::paper::PAPER_KEYS {
                            ui.selectable_value(&mut paper, k.to_string(), cc_detail::paper::paper_label(k));
                        }
                    })
                    .response
                    .on_hover_text(t0("dwg.paper.title"))
                    .labelled_by(pl.id);
                if paper != d.set.paper {
                    run(acts, "detail.drawings", format!("paper:{paper}"));
                }
            });
            // the issue log (D17)
            let sh_id = d.set.sheets.get(at).map(|s| s.id.clone()).unwrap_or_default();
            let x = st.iter().find(|(k, _)| *k == sh_id).map(|x| x.1.clone());
            let changed_n = d.set.sheets.iter().filter(|s| st.iter().any(|(k, x)| *k == s.id && x.changed)).count();
            ui.horizontal_wrapped(|ui| {
                let (text, warn) = match &x {
                    Some(x) if x.issued && x.changed => {
                        (t("issue.st.changed", &[("rev", F::S(val(&x.rev))), ("date", F::S(val(&x.date)))]), true)
                    }
                    Some(x) if x.issued => (t("issue.st.current", &[("rev", F::S(val(&x.rev))), ("date", F::S(val(&x.date)))]), false),
                    _ => (t0("issue.st.new"), false),
                };
                // detailcraft.css `.dc-issue-st.is-changed`: the layer red, semibold
                ui.label(if warn {
                    RichText::new(text).color(Color32::from_rgb(0xd0, 0x22, 0x3e)).strong()
                } else {
                    RichText::new(text).color(tk.text)
                });
                let nl = ui.label(RichText::new(t0("issue.note.label")).color(tk.text_dim));
                let before = d.note.clone();
                ui.add(crate::shell::field(&mut d.note).desired_width(220.0).char_limit(60).hint_text(t0("issue.note.placeholder")))
                    .labelled_by(nl.id);
                if d.note != before {
                    let v = d.note.clone();
                    d.note = before;
                    run(acts, "detail.drawings", format!("note:{v}"));
                }
                let done = x.as_ref().is_some_and(|x| x.issued && !x.changed);
                let one = ui.add_enabled(!done, crate::shell::Btn::new(t0("issue.one")));
                let one = if done { one.on_disabled_hover_text(t0("issue.one.why")) } else { one };
                if one.clicked() {
                    run(acts, "detail.drawings", "issue:one".into());
                }
                if ui.add_enabled(changed_n > 0, crate::shell::Btn::new(t("issue.all", &[("n", F::N(changed_n as f64))]))).clicked() {
                    run(acts, "detail.drawings", "issue:all".into());
                }
            });
            // the foot: what the set is, the format, the downloads
            let foot = |ui: &mut egui::Ui, dv_fmt: &mut String, d: &DwgDlg, acts: &mut Vec<Act>| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(t(
                            "dwg.note",
                            &[
                                ("n", F::N(d.set.sheets.len() as f64)),
                                ("paper", F::S(cc_detail::paper::paper_label(d.set.paper))),
                                ("ref", F::S(d.set.r#ref.clone())),
                            ],
                        ))
                        .small()
                        .color(tk.text_dim),
                    );
                    let fl = ui.label(RichText::new(t0("dwg.format.label")).color(tk.text_dim));
                    let mut f = dv_fmt.clone();
                    egui::ComboBox::new("dc_dwg_format", "")
                        .selected_text(crate::detailexport::format_name(&f).unwrap_or_default())
                        .show_ui(ui, |ui| {
                            for k in ["pdf", "dwg-AC1032", "dwg-AC1027", "dwg-AC1024", "dwg-AC1021", "dwg-AC1018", "dwg-AC1015", "dxf"] {
                                ui.selectable_value(&mut f, k.to_string(), crate::detailexport::format_name(k).unwrap_or_default());
                            }
                        })
                        .response
                        .on_hover_text(t0("dwg.format.title"))
                        .labelled_by(fl.id);
                    *dv_fmt = f;
                    let fname = crate::detailexport::format_name(dv_fmt).unwrap_or_default();
                    if ui.add(crate::shell::Btn::new(t("dwg.one", &[("format", F::S(fname.clone()))]))).clicked() {
                        acts.push(Act::Run("detail.export", format!("sheet:{}:{}", d.at, dv_fmt)));
                    }
                    if ui.add(crate::shell::Btn::primary(t("dwg.all", &[("format", F::S(fname))]))).clicked() {
                        acts.push(Act::Run("detail.export", format!("drawings:{dv_fmt}")));
                    }
                });
            };
            // the preview fills what is left above the foot
            let room = (ui.available_height() - 44.0).max(160.0);
            let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), room), egui::Sense::hover());
            if let Some(sh) = d.set.sheets.get(at) {
                draw_sheet(&ui.painter_at(r), r, sh);
            }
            ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, tk.divider), egui::StrokeKind::Outside);
            let mut fmt = dv.dwg_format.clone();
            if let Some(d) = dv.dwg.as_ref() {
                foot(ui, &mut fmt, d, acts);
            }
            dv.dwg_format = fmt;
        });
    if !open {
        run(acts, "detail.drawings", "close".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detailexport::Settings;
    use serde_json::json;

    #[test]
    fn the_issue_log_issues_once_until_the_sheet_changes() {
        let mut dv = DetailView::sample().expect("sample");
        assert!(dv.drawings_cmd("sheet:0", 0.0).is_err(), "not open yet");
        dv.drawings_cmd("open", 1_760_700_000_000.0).expect("open");
        let n = dv.dwg.as_ref().map_or(0, |d| d.set.sheets.len());
        assert!(n > 2);
        dv.drawings_cmd("issue:one@1760700000000", 0.0).expect("issue");
        let issues = cc_detail::js::arr(dv.model.state.get("issues")).len();
        assert_eq!(issues, 1);
        dv.drawings_cmd("issue:one@1760700000000", 0.0).expect("again");
        assert_eq!(cc_detail::js::arr(dv.model.state.get("issues")).len(), 1, "unchanged: not issued again");
        dv.drawings_cmd("issue:all@1760700000000", 0.0).expect("all");
        assert_eq!(cc_detail::js::arr(dv.model.state.get("issues")).len(), n);
        let st = dv.dwg_status();
        assert!(st.iter().all(|(_, x)| x.issued && !x.changed));
        assert!(dv.drawings_cmd(&format!("sheet:{n}"), 0.0).is_err());
        assert!(dv.drawings_cmd("issue:x", 0.0).is_err());
        dv.drawings_cmd("close", 0.0).expect("close");
        assert!(dv.dwg.is_none());
    }

    #[test]
    fn kept_settings_read_back_and_bad_values_keep_their_defaults() {
        let mut s = Settings::default();
        s.set("steel=eu").expect("steel");
        s.set("corner=axes").expect("corner");
        s.set("offset=1,2,3").expect("offset");
        let back = Settings::load(&s.json());
        assert_eq!(back.json(), s.json());
        let bad = Settings::load(&json!({
            "steel": "mars", "offset": [1, 2, "x"], "corner": "sphere", "conns": "yes",
            "nest": {"stock": [50, 7000, "9000"], "kerf": 99, "trim": -1, "keep": 600}
        }));
        let d = Settings::default();
        assert_eq!(
            (bad.steel.as_str(), bad.offset, bad.corner.as_str(), bad.conns),
            (d.steel.as_str(), d.offset, d.corner.as_str(), d.conns)
        );
        assert_eq!(bad.nest_stock.first().copied(), Some(7000.0));
        assert_eq!((bad.nest_kerf, bad.nest_trim, bad.nest_keep), (3.0, 0.0, 600.0));
        assert_eq!(Settings::load(&json!("nonsense")).json(), d.json());
    }
}
