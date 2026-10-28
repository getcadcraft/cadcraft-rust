//! CivilCraft's cross-section sheets and mass haul from an alignment's Sample lines (M11.11), as sections-ui.js
//! `sheetsDialog` and masshaul-ui.js `massHaulDialog` show them: the sheets dialog (paper, scale, exaggeration, the live
//! page count and notes) writes one PDF of every section (io-civil `section_sheets` + `to_pdf_pages`); the mass haul
//! dialog (the shrink or swell of the cut, the diagram, the balanced stretches, what is left over) writes the CSV
//! (cc-civil `mass_haul` / `mass_haul_csv`). Both are reachable headless: `civil.xs`, `civil.xs.info`, `civil.xs.go`,
//! `civil.mh`, `civil.mh.info`, `civil.mh.csv` (the two `.info` commands put the dialog's facts on the status line).

use cc_civil::i18n::t;
use cc_civil::js::{arr, num_to_string, str_to_number, string_of, trim};
use cc_civil::masshaul::{MassHaul, mass_haul, mass_haul_csv};
use cc_civil::tr;
use cc_civil::units::{fixed, format_station, unit_of};
use cc_io_civil::plot;
use cc_io_civil::sheets::{SHEET_VEXES, section_sheets};
use egui::{Color32, Pos2, RichText, Stroke};
use serde_json::Value;

use crate::civilview::{CivilView, Sel};
use crate::theme::Tokens;

/// The sheets dialog's choices (sections-ui.js `sheetsDialog`).
#[derive(Clone, Debug, Default)]
pub struct XsDialog {
    pub al: String,
    pub paper: String,
    pub scale: String,
    pub vex: f64,
    /// (project version, paper, scale, vex) → the info line (built again only when one changes)
    cache: Option<(u64, String, String, f64, String)>,
}

/// The mass haul dialog (masshaul-ui.js `massHaulDialog`): the factor as typed, the last good result.
#[derive(Clone, Debug, Default)]
pub struct MhDialog {
    pub al: String,
    pub text: String,
    pub invalid: bool,
    res: Option<MassHaul>,
    version: u64,
}

/// Both dialogs, and the factor kept between openings (0.5 keeps it in localStorage `civilcraft.masshaul`).
#[derive(Clone, Debug)]
pub struct SheetsState {
    pub xs: Option<XsDialog>,
    pub mh: Option<MhDialog>,
    pub mh_pct: f64,
}

impl Default for SheetsState {
    fn default() -> Self {
        SheetsState { xs: None, mh: None, mh_pct: -10.0 }
    }
}

fn find_al(state: &Value, id: &str) -> Option<Value> {
    arr(state.get("alignments")).iter().find(|a| a.get("id").and_then(Value::as_str) == Some(id)).cloned()
}

/// masshaul-ui.js: a percent between −60 and 100 (exclusive), not empty.
fn read_pct(s: &str) -> Option<f64> {
    let p = str_to_number(s);
    (p > -60.0 && p < 100.0 && !trim(s).is_empty()).then_some(p)
}

impl CivilView {
    fn file_stem_of(&self) -> String {
        crate::civilexport::file_stem(self.project.state.get("name").and_then(Value::as_str).unwrap_or(""))
    }

    /// The selected alignment and its sections (the Sample lines block the two buttons sit in).
    fn sel_sections(&mut self, need: usize, cmd: &str) -> Result<(Value, std::sync::Arc<cc_civil::sections::SectionSet>), String> {
        let Some(Sel::Alignment(id)) = self.sel.clone() else { return Err(format!("{cmd}: select an alignment first")) };
        let a = find_al(&self.project.state, &id).ok_or_else(|| format!("{cmd}: no alignment {id}"))?;
        let set = self.project.sections(&a).filter(|s| s.sections.len() >= need);
        let set = set.ok_or_else(|| {
            if need > 1 { format!("{cmd}: the alignment needs two or more sections") } else { format!("{cmd}: {}", t("xs.none", &[])) }
        })?;
        Ok((a, set))
    }

    /// `civil.xs`: open the sheets dialog for the selected alignment, then apply "paper=…,scale=…,vex=…".
    pub fn xs_open(&mut self, spec: &str) -> Result<(), String> {
        let units = self.project.units().to_string();
        let keep = self.sheets.xs.clone();
        let (a, _) = self.sel_sections(1, "civil.xs")?;
        let id = string_of(a.get("id"));
        let mut d = match keep {
            Some(d) if d.al == id => d,
            _ => XsDialog {
                al: id,
                paper: if units == "m" { "a3" } else { "tabloid" }.into(),
                scale: if units == "m" { "1:200" } else { "1\" = 20'" }.into(),
                vex: 5.0,
                cache: None,
            },
        };
        for kv in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (k, v) = kv.split_once('=').ok_or_else(|| format!("civil.xs: {kv} (key=value)"))?;
            let v = v.trim();
            match k.trim() {
                "paper" => {
                    if !plot::PAPERS.iter().any(|p| p.key == v) {
                        let keys: Vec<&str> = plot::PAPERS.iter().map(|p| p.key).collect();
                        return Err(format!("civil.xs: paper {v} ({})", keys.join(", ")));
                    }
                    d.paper = v.into();
                }
                "scale" => {
                    if !plot::scales_for(&units).iter().any(|s| s.label == v) {
                        return Err(format!("civil.xs: scale {v} is not one of {}'s", if units == "m" { "metres" } else { "feet" }));
                    }
                    d.scale = v.into();
                }
                "vex" => {
                    let x = str_to_number(v);
                    if !SHEET_VEXES.contains(&x) {
                        return Err("civil.xs: vex = 10, 5, 2 or 1".into());
                    }
                    d.vex = x;
                }
                o => return Err(format!("civil.xs: unknown choice {o} (paper, scale, vex)")),
            }
        }
        self.sheets.xs = Some(d);
        Ok(())
    }

    /// The sheets for the dialog's choices: (sheets, perPage, notes, sections, the alignment).
    #[allow(clippy::type_complexity)]
    fn xs_build(&mut self, made_ms: f64) -> Option<(Vec<plot::Sheet>, f64, Vec<String>, usize, Value)> {
        let d = self.sheets.xs.clone()?;
        let a = find_al(&self.project.state, &d.al)?;
        let set = self.project.sections(&a)?;
        let units = self.project.units().to_string();
        let scale = plot::scales_for(&units).into_iter().find(|s| s.label == d.scale)?;
        let title = string_of(self.project.state.get("name"));
        let name = string_of(a.get("name"));
        let (sheets, _cell, per, _vex, notes) =
            section_sheets(&set, &d.paper, true, &scale, d.vex, &units, &title, &name, f64::NEG_INFINITY, f64::INFINITY, made_ms);
        Some((sheets, per, notes, set.sections.len(), a))
    }

    /// The dialog's info line (sections-ui.js `update`): sections, per page, pages, then the notes.
    pub fn xs_info(&mut self) -> Option<String> {
        let d = self.sheets.xs.clone()?;
        let v = self.project.version;
        if let Some((cv, p, s, x, text)) = &d.cache
            && *cv == v
            && *p == d.paper
            && *s == d.scale
            && *x == d.vex
        {
            return Some(text.clone());
        }
        let (sheets, per, notes, n, _) = self.xs_build(0.0)?;
        let mut parts = vec![tr!("xs.ui.info", n = sheets.len(), sections = n, per = num_to_string(per))];
        parts.extend(notes);
        let text = parts.join(" ");
        if let Some(x) = self.sheets.xs.as_mut() {
            x.cache = Some((v, d.paper, d.scale, d.vex, text.clone()));
        }
        Some(text)
    }

    /// "Download PDF": (file name, bytes, what is said); the dialog closes.
    pub fn xs_pdf(&mut self, now_ms: f64) -> Option<(String, Vec<u8>, String)> {
        let t0 = web_time::Instant::now();
        let (sheets, _, notes, _, a) = self.xs_build(now_ms)?;
        self.timings.insert("xs.layout", t0.elapsed().as_secs_f64() * 1000.0);
        let name = string_of(a.get("name"));
        let title = format!("{} {name}", string_of(self.project.state.get("name")));
        let (bytes, _) = plot::to_pdf_pages(&sheets, false, &title, now_ms);
        let file = tr!("xs.ui.file", stem = self.file_stem_of(), name = name.as_str());
        let mut said = vec![tr!("xs.ui.done", n = sheets.len(), file = file.as_str())];
        said.extend(notes);
        self.sheets.xs = None;
        self.timings.insert("xs.pdf", t0.elapsed().as_secs_f64() * 1000.0);
        Some((file, bytes, said.join(" ")))
    }

    /// `civil.mh`: open the mass haul dialog for the selected alignment; a name types the factor (percent).
    pub fn mh_open(&mut self, factor: &str) -> Result<(), String> {
        let (a, _) = self.sel_sections(2, "civil.mh")?;
        let id = string_of(a.get("id"));
        let open_same = self.sheets.mh.as_ref().is_some_and(|m| m.al == id);
        if !open_same {
            self.sheets.mh =
                Some(MhDialog { al: id, text: num_to_string(self.sheets.mh_pct), invalid: false, res: None, version: u64::MAX });
        }
        if !factor.is_empty()
            && let Some(m) = self.sheets.mh.as_mut()
        {
            m.text = factor.to_string();
        }
        self.mh_update();
        if self.sheets.mh.as_ref().is_some_and(|m| m.invalid) {
            return Err("civil.mh: the shrink or swell is a percent above −60 and under 100".into());
        }
        Ok(())
    }

    /// masshaul-ui.js `update`: a good factor is kept and the numbers worked out again; a bad one only marks the field.
    pub fn mh_update(&mut self) {
        let Some(m) = self.sheets.mh.clone() else { return };
        let Some(p) = read_pct(&m.text) else {
            if let Some(x) = self.sheets.mh.as_mut() {
                x.invalid = true;
            }
            return;
        };
        let v = self.project.version;
        if m.res.is_some() && !m.invalid && m.version == v && self.sheets.mh_pct == p {
            return;
        }
        if self.sheets.mh_pct != p {
            self.prefs_dirty = true;
        }
        self.sheets.mh_pct = p;
        let t0 = web_time::Instant::now();
        let res =
            find_al(&self.project.state, &m.al).and_then(|a| self.project.sections(&a)).map(|set| mass_haul(&set.rows, 1.0 + p / 100.0));
        if let Some(x) = self.sheets.mh.as_mut() {
            x.invalid = false;
            x.res = res;
            x.version = v;
        }
        self.timings.insert("mh", t0.elapsed().as_secs_f64() * 1000.0);
    }

    fn mh_vol(&self, v: f64) -> String {
        let uo = unit_of(self.project.units());
        format!("{} {}", fixed(v / uo.vol_per, 1), uo.vol)
    }

    /// The dialog's facts as lines: totals and what is left over, "nothing to move" when nothing is, balance points.
    pub fn mh_facts(&self) -> Option<(String, String, String)> {
        let m = self.sheets.mh.as_ref()?;
        let r = m.res.as_ref()?;
        let units = self.project.units().to_string();
        let mut totals = tr!("mh.totals", cut = self.mh_vol(r.cut), adj = self.mh_vol(r.cut_adj), fill = self.mh_vol(r.fill));
        totals.push(' ');
        totals.push_str(&match r.kind {
            "balanced" => t("mh.balanced", &[]),
            "waste" => tr!("mh.waste", v = self.mh_vol(r.end.abs())),
            _ => tr!("mh.borrow", v = self.mh_vol(r.end.abs())),
        });
        let spans = if r.spans.is_empty() { t("mh.nospans", &[]) } else { String::new() };
        let balance = if r.balance.is_empty() {
            t("mh.nobalance", &[])
        } else {
            let list: Vec<String> = r.balance.iter().map(|s| format_station(*s, &units, None)).collect();
            tr!("mh.balance", list = list.join(", "))
        };
        Some((totals, spans, balance))
    }

    /// `civil.mh.info`: the facts on one line.
    pub fn mh_info(&self) -> Option<String> {
        let (a, b, c) = self.mh_facts()?;
        Some([a, b, c].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" "))
    }

    /// The CSV button: (file name, bytes, what is said).
    pub fn mh_csv(&self) -> Option<(String, Vec<u8>, String)> {
        let m = self.sheets.mh.as_ref()?;
        let r = m.res.as_ref()?;
        let a = find_al(&self.project.state, &m.al)?;
        let units = self.project.units().to_string();
        let station = |s: f64| format_station(s, &units, None);
        let text = format!("\u{feff}{}", mass_haul_csv(r, &units, &station));
        let file = tr!("mh.file", stem = self.file_stem_of(), name = string_of(a.get("name")));
        Some((file, text.into_bytes(), t("mh.saved", &[])))
    }
}

/// What the dialogs ask for after the frame: a command to run.
pub type ShAct = (&'static str, String);

/// The two dialogs as windows over the plan.
pub fn show(cv: &mut CivilView, ctx: &egui::Context, tk: &Tokens, dark: bool, acts: &mut Vec<ShAct>) {
    let units = cv.project.units().to_string();
    if let Some(d) = cv.sheets.xs.clone() {
        let name = find_al(&cv.project.state, &d.al).map(|a| string_of(a.get("name"))).unwrap_or_default();
        let info = cv.xs_info().unwrap_or_default();
        let mut open = true;
        let mut close = false;
        egui::Window::new(tr!("xs.ui.title", name = name.as_str()))
            .id(egui::Id::unique("civil_xs_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .default_pos([320.0, 110.0])
            .show(ctx, |ui| {
                ui.add(egui::Label::new(RichText::new(t("xs.ui.lede", &[])).small().color(tk.text_dim)).wrap());
                ui.add_space(4.0);
                egui::Grid::new("civil_xs_grid").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                    ui.label(t("pp.ui.paper", &[]));
                    let cur = plot::PAPERS.iter().find(|p| p.key == d.paper).map(plot::paper_name).unwrap_or_default();
                    egui::ComboBox::new("civil_xs_paper", "")
                        .selected_text(cur)
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for p in plot::PAPERS.iter() {
                                if ui.selectable_label(p.key == d.paper, plot::paper_name(p)).clicked() {
                                    acts.push(("civil.xs", format!("paper={}", p.key)));
                                }
                            }
                        })
                        .response
                        .on_hover_text(t("pp.ui.paper", &[]));
                    ui.end_row();
                    ui.label(t("pp.ui.scale", &[]));
                    egui::ComboBox::new("civil_xs_scale", "")
                        .selected_text(d.scale.clone())
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for s in plot::scales_for(&units) {
                                if ui.selectable_label(s.label == d.scale, s.label.clone()).clicked() {
                                    acts.push(("civil.xs", format!("scale={}", s.label)));
                                }
                            }
                        })
                        .response
                        .on_hover_text(t("pp.ui.scale", &[]));
                    ui.end_row();
                    ui.label(t("pp.ui.vex", &[]));
                    egui::ComboBox::new("civil_xs_vex", "")
                        .selected_text(format!("{}×", num_to_string(d.vex)))
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for v in SHEET_VEXES {
                                if ui.selectable_label(v == d.vex, format!("{}×", num_to_string(v))).clicked() {
                                    acts.push(("civil.xs", format!("vex={}", num_to_string(v))));
                                }
                            }
                        })
                        .response
                        .on_hover_text(t("pp.ui.vex", &[]));
                    ui.end_row();
                });
                ui.add_space(4.0);
                ui.add(egui::Label::new(RichText::new(info).small().color(tk.text)).wrap());
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t("pp.ui.cancel", &[])).clicked() {
                        close = true;
                    }
                    if ui.add(egui::Button::new(RichText::new(t("xs.ui.go", &[])).strong())).clicked() {
                        acts.push(("civil.xs.go", String::new()));
                    }
                });
            });
        if !open || close {
            cv.sheets.xs = None;
        }
    }
    if let Some(m) = cv.sheets.mh.clone() {
        cv.mh_update();
        let name = find_al(&cv.project.state, &m.al).map(|a| string_of(a.get("name"))).unwrap_or_default();
        let mut open = true;
        let mut close = false;
        egui::Window::new(tr!("mh.title", name = name.as_str()))
            .id(egui::Id::unique("civil_mh_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(680.0)
            .default_pos([260.0, 90.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let l = ui.label(t("mh.factor", &[]));
                    let mut text = cv.sheets.mh.as_ref().map(|x| x.text.clone()).unwrap_or_default();
                    let invalid = cv.sheets.mh.as_ref().is_some_and(|x| x.invalid);
                    let mut te = egui::TextEdit::singleline(&mut text).desired_width(70.0);
                    if invalid {
                        te = te.text_color(ui.visuals().error_fg_color);
                    }
                    let r = ui.add(te).labelled_by(l.id);
                    if r.changed()
                        && let Some(x) = cv.sheets.mh.as_mut()
                    {
                        x.text = text;
                    }
                    ui.label("%");
                });
                ui.add(egui::Label::new(RichText::new(t("mh.factor.note", &[])).small().color(tk.text_dim)).wrap());
                cv.mh_update();
                let res = cv.sheets.mh.as_ref().and_then(|x| x.res.clone());
                if let Some(r) = &res {
                    diagram(ui, r, &units, tk, dark);
                }
                if let (Some(r), Some((totals, nospans, balance))) = (&res, cv.mh_facts()) {
                    ui.add(egui::Label::new(RichText::new(totals).color(tk.text)).wrap());
                    if nospans.is_empty() {
                        let uo = unit_of(&units);
                        egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                            egui::Grid::new("civil_mh_spans").striped(true).num_columns(5).spacing([14.0, 2.0]).show(ui, |ui| {
                                for h in [
                                    t("mh.c.from", &[]),
                                    t("mh.c.to", &[]),
                                    tr!("mh.c.vol", u = uo.vol),
                                    t("mh.c.dir", &[]),
                                    tr!("mh.c.haul", u = if units == "m" { "m" } else { "ft" }),
                                ] {
                                    ui.label(RichText::new(h).small().strong());
                                }
                                ui.end_row();
                                for s in &r.spans {
                                    ui.label(format_station(s.from, &units, None));
                                    ui.label(format_station(s.to, &units, None));
                                    ui.label(fixed(s.volume / uo.vol_per, 1));
                                    ui.label(t(if s.ahead { "mh.dir.ahead" } else { "mh.dir.back" }, &[]));
                                    ui.label(fixed(s.haul, 0));
                                    ui.end_row();
                                }
                            });
                        });
                    } else {
                        ui.label(RichText::new(nospans).small().color(tk.text_dim));
                    }
                    ui.label(RichText::new(balance).small().color(tk.text_dim));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t("pc.dlg.cancel", &[])).clicked() {
                        close = true;
                    }
                    if ui.add_enabled(res.is_some(), egui::Button::new("CSV")).clicked() {
                        acts.push(("civil.mh.csv", String::new()));
                    }
                });
            });
        if !open || close {
            cv.sheets.mh = None;
        }
    }
}

/// masshaul-ui.js `drawMassHaul`: stations across, the mass ordinate up; above zero shaded orange, below green; the
/// balance points ticked on the zero line; volume and station labels.
fn diagram(ui: &mut egui::Ui, res: &MassHaul, units: &str, tk: &Tokens, dark: bool) {
    let w = ui.available_width().max(200.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 260.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let pal = crate::civilplan::Palette::of(tk, dark);
    painter.rect_filled(rect, 0.0, pal.bg);
    let p = &res.points;
    if p.len() < 2 {
        return;
    }
    let uo = unit_of(units);
    let (s0, s1) = (p.first().map_or(0.0, |q| q.0), p.last().map_or(0.0, |q| q.0));
    let mut lo = p.iter().fold(0.0f64, |m, q| m.min(q.1));
    let mut hi = p.iter().fold(0.0f64, |m, q| m.max(q.1));
    if hi - lo < 1e-9 {
        hi += 1.0;
        lo -= 1.0;
    }
    let (h, wf) = (f64::from(rect.height()), f64::from(rect.width()));
    let (pl, pr, pt, pb) = (64.0, 12.0, 12.0, 26.0);
    let x = |s: f64| rect.min.x + (pl + ((s - s0) / (s1 - s0).max(1e-9)) * (wf - pl - pr)) as f32;
    let y = |m: f64| rect.min.y + (pt + ((hi - m) / (hi - lo)) * (h - pt - pb)) as f32;
    let font = egui::FontId::proportional(11.0);
    let grid = Stroke::new(1.0, pal.ink.gamma_multiply(0.18));
    let span = (hi - lo) / uo.vol_per;
    let raw = span / 4.0;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].iter().map(|k| k * mag).find(|v| *v >= raw).unwrap_or(10.0 * mag) * uo.vol_per;
    if step.is_finite() && step > 0.0 {
        let mut v = (lo / step).ceil() * step;
        let mut guard = 0;
        while v <= hi + 1e-9 && guard < 200 {
            let yy = y(v);
            let stroke = if v.abs() < 1e-9 { Stroke::new(1.0, pal.ink) } else { grid };
            painter.line_segment([Pos2::new(rect.min.x + pl as f32, yy), Pos2::new(rect.max.x - pr as f32, yy)], stroke);
            painter.text(
                Pos2::new(rect.min.x + pl as f32 - 4.0, yy),
                egui::Align2::RIGHT_CENTER,
                fixed(v / uo.vol_per, 0),
                font.clone(),
                pal.ink,
            );
            v += step;
            guard += 1;
        }
    }
    let shade = |c: Color32| c.gamma_multiply(0.22);
    for w2 in p.windows(2) {
        let (Some(a), Some(b)) = (w2.first(), w2.get(1)) else { continue };
        let pieces: Vec<((f64, f64), (f64, f64))> = if (a.1 >= 0.0) == (b.1 >= 0.0) {
            vec![(*a, *b)]
        } else {
            let sx = a.0 + ((b.0 - a.0) * a.1) / (a.1 - b.1);
            vec![(*a, (sx, 0.0)), ((sx, 0.0), *b)]
        };
        for (q, r) in pieces {
            let col = shade(if (q.1 + r.1) / 2.0 >= 0.0 { pal.orange } else { pal.green });
            let pts = vec![Pos2::new(x(q.0), y(0.0)), Pos2::new(x(q.0), y(q.1)), Pos2::new(x(r.0), y(r.1)), Pos2::new(x(r.0), y(0.0))];
            let mut mesh = egui::Mesh::default();
            for v in &pts {
                mesh.colored_vertex(*v, col);
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            painter.add(egui::Shape::mesh(mesh));
        }
    }
    painter.add(egui::Shape::line(p.iter().map(|q| Pos2::new(x(q.0), y(q.1))).collect(), Stroke::new(2.0, pal.blue)));
    for s in &res.balance {
        let xx = x(*s);
        painter.line_segment([Pos2::new(xx, y(0.0) - 6.0), Pos2::new(xx, y(0.0) + 6.0)], Stroke::new(1.5, pal.ink));
        painter.text(
            Pos2::new(xx, rect.max.y - pb as f32 + 6.0),
            egui::Align2::CENTER_TOP,
            format_station(*s, units, None),
            font.clone(),
            pal.ink,
        );
    }
    let by = rect.max.y - pb as f32 + 6.0;
    painter.text(Pos2::new(rect.min.x + pl as f32, by), egui::Align2::LEFT_TOP, format_station(s0, units, None), font.clone(), pal.ink);
    painter.text(Pos2::new(rect.max.x - pr as f32, by), egui::Align2::RIGHT_TOP, format_station(s1, units, None), font, pal.ink);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheets_and_mass_haul_come_from_the_selected_alignments_sections() {
        let mut cv = CivilView::demo().unwrap();
        assert!(cv.xs_open("").is_err(), "nothing selected");
        cv.sel = Some(Sel::Alignment("a1".into()));
        cv.sampling_edit("step=25").unwrap();
        cv.xs_open("paper=letter,vex=10").unwrap();
        assert!(cv.xs_open("paper=napkin").is_err());
        assert!(cv.xs_open("vex=3").is_err());
        let info = cv.xs_info().unwrap();
        assert!(info.contains("on a page"), "{info}");
        let (file, bytes, said) = cv.xs_pdf(1_760_700_000_000.0).unwrap();
        assert!(file.ends_with("Main Street sections.pdf"), "{file}");
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(said.contains(&file), "{said}");
        assert!(cv.sheets.xs.is_none(), "the dialog closes");
        cv.mh_open("").unwrap();
        assert!(cv.mh_open("-75").is_err());
        assert_eq!(cv.sheets.mh_pct, -10.0, "a bad factor is not kept");
        cv.mh_open("5").unwrap();
        assert_eq!(cv.sheets.mh_pct, 5.0);
        let info = cv.mh_info().unwrap();
        assert!(info.starts_with("Cut "), "{info}");
        let (file, bytes, said) = cv.mh_csv().unwrap();
        assert!(file.ends_with("Main Street mass haul.csv"), "{file}");
        assert!(bytes.starts_with("\u{feff}".as_bytes()));
        assert_eq!(said, t("mh.saved", &[]));
    }
}
