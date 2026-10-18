//! CivilCraft's section panel (M11.9): sample lines along an alignment and one cross section at a time under the plan,
//! as sections-ui.js and section-view.js show them — offset across (left −, right +), elevation up, the existing ground,
//! the design, cut and fill shaded between them, corridor points marked, the grid, the station, the section's areas and
//! the exaggeration (fit, 1×, 2×, 5×, 10×); ← → Home End step through the sections; the alignment's end-area volumes;
//! the CSV. The sample lines are edited in the alignment's Properties (add, spacing, widths, extra stations, remove),
//! each one undo step with 0.5's labels. Every number comes from the engine's `Project::sections` (sections.js).

use cc_civil::i18n::t;
use cc_civil::js::{arr, jnum, numf, string_of};
use cc_civil::sections::{Run, Sec, clean_sampling, sample_defaults, sections_csv};
use cc_civil::tr;
use cc_civil::units::{fixed, format_station, parse_station, unit_of, volume_text};
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke};
use serde_json::Value;

use crate::civilplan::Palette;
use crate::civilview::{CivilView, Sel};
use crate::theme::Tokens;

/// The exaggerations the panel offers (0 = fit).
pub const VES: [f64; 5] = [0.0, 1.0, 2.0, 5.0, 10.0];

/// The section panel's state (sections-ui.js `want`, `idx`, `ve`).
#[derive(Clone, Debug, Default)]
pub struct SectionsPanel {
    pub open: bool,
    pub want: Option<String>,
    pub idx: usize,
    pub ve: f64,
}

/// The view of one section (section-view.js `fit`): offsets o0…o1, elevations z0…z1, the exaggeration used.
#[derive(Clone, Copy, Debug)]
pub struct SecPlot {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
    pub o0: f64,
    pub o1: f64,
    pub z0: f64,
    pub z1: f64,
    pub ve: f64,
}

impl SecPlot {
    /// section-view.js `fit()` for a panel `w` × `h` (pad left 58, right 16, top 26, bottom 30).
    pub fn fit(s: &Sec, w: f64, h: f64, exaggeration: f64) -> SecPlot {
        let (x0, x1, y0, y1) = (58.0, (w - 16.0).max(59.0), (h - 30.0).max(27.0), 26.0);
        let (o0, o1) = (-s.left, s.right);
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for r in s.ground.iter().chain(s.design.iter()) {
            for q in r {
                lo = lo.min(q.z);
                hi = hi.max(q.z);
            }
        }
        for m in &s.marks {
            if let Some(z) = m.z.filter(|z| z.is_finite()) {
                lo = lo.min(z);
                hi = hi.max(z);
            }
        }
        if !lo.is_finite() || !hi.is_finite() {
            lo = 0.0;
            hi = 10.0;
        }
        let px_per_o = (x1 - x0) / (o1 - o0).max(1e-9);
        let span = (hi - lo).max(1.0) * 1.3;
        let fit_ve = ((y0 - y1) / span) / px_per_o;
        let ve = if exaggeration > 0.0 { exaggeration } else { fit_ve.max(1.0) };
        let span = (y0 - y1) / (px_per_o * ve);
        let mid = (lo + hi) / 2.0;
        SecPlot { x0, x1, y0, y1, o0, o1, z0: mid - span / 2.0, z1: mid + span / 2.0, ve }
    }
    pub fn to_screen(&self, o: f64, z: f64) -> (f64, f64) {
        (
            self.x0 + ((o - self.o0) / (self.o1 - self.o0)) * (self.x1 - self.x0),
            self.y0 - ((z - self.z0) / (self.z1 - self.z0)) * (self.y0 - self.y1),
        )
    }
}

fn nice_step(raw: f64, steps: &[f64]) -> f64 {
    for s in steps {
        if *s >= raw {
            return *s;
        }
    }
    let last = steps.last().copied().unwrap_or(1.0);
    last * (raw / last).ceil().max(1.0)
}

/// section-view.js `zAt(runs, o)`.
fn z_at(runs: &[Run], o: f64) -> Option<f64> {
    for r in runs {
        let (Some(first), Some(last)) = (r.first(), r.last()) else { continue };
        if o < first.o - 1e-9 || o > last.o + 1e-9 {
            continue;
        }
        for w in r.windows(2) {
            let (Some(p), Some(q)) = (w.first(), w.get(1)) else { continue };
            if o <= q.o + 1e-9 {
                let d = q.o - p.o;
                return Some(if d > 0.0 { p.z + ((q.z - p.z) * (o - p.o)) / d } else { q.z });
            }
        }
    }
    None
}

fn find_al<'a>(state: &'a Value, id: &str) -> Option<&'a Value> {
    arr(state.get("alignments")).iter().find(|a| a.get("id").and_then(Value::as_str) == Some(id))
}

impl CivilView {
    /// sections-ui.js `alignment()`: the one asked for, else the first with sample lines, else the first.
    pub fn sections_alignment(&self) -> Option<Value> {
        let st = &self.project.state;
        let als = arr(st.get("alignments"));
        self.sections
            .want
            .as_ref()
            .and_then(|w| find_al(st, w).cloned())
            .or_else(|| als.iter().find(|a| cc_civil::js::truthy(a.get("sampling"))).cloned())
            .or_else(|| als.first().cloned())
    }

    /// sections-ui.js `show(on, alId)`: one bottom panel at a time (the profile closes).
    pub fn show_sections(&mut self, on: bool, al: Option<&str>) {
        if let Some(a) = al {
            self.sections.want = Some(a.to_string());
        }
        if on {
            self.profile.open = false;
        }
        self.sections.open = on;
    }

    /// sections-ui.js `go(i)`: the section shown, kept inside the set.
    pub fn sections_go(&mut self, i: i64) {
        let Some(a) = self.sections_alignment() else { return };
        let Some(set) = self.project.sections(&a) else { return };
        let n = set.sections.len() as i64;
        self.sections.idx = i.clamp(0, (n - 1).max(0)) as usize;
    }

    /// The sample line edits of the selected alignment's Properties (sections-ui.js `props`): "add", "remove", "step=x",
    /// "left=x", "right=x", "extra+=station", "extra-=station".
    pub fn sampling_edit(&mut self, spec: &str) -> Result<(), String> {
        let Some(Sel::Alignment(id)) = self.sel.clone() else { return Err("civil.sampling: select an alignment first".into()) };
        let a = find_al(&self.project.state, &id).cloned().ok_or("civil.sampling: no such alignment")?;
        let units = self.project.units().to_string();
        let has = cc_civil::js::truthy(a.get("sampling"));
        let edit = |cv: &mut CivilView, label: &str, f: &mut dyn FnMut(&mut serde_json::Map<String, Value>)| {
            cv.project.change(label, &mut |s: &mut Value| {
                if let Some(m) = s
                    .get_mut("alignments")
                    .and_then(Value::as_array_mut)
                    .and_then(|l| l.iter_mut().find(|q| q.get("id").and_then(Value::as_str) == Some(id.as_str())))
                    .and_then(Value::as_object_mut)
                {
                    f(m);
                }
            });
        };
        match spec.split_once('=').map_or((spec, ""), |(k, v)| (k, v)) {
            ("add", _) => {
                let d = sample_defaults(&units);
                let v = cc_civil::js::J::j(&clean_sampling(Some(&cc_civil::js::J::j(&d)), &units).unwrap_or(d));
                edit(self, &t("sec.undo.add", &[]), &mut |m| {
                    m.insert("sampling".into(), v.clone());
                });
                self.show_sections(true, Some(&id));
            }
            ("remove", _) if has => edit(self, &t("sec.undo.remove", &[]), &mut |m| {
                m.insert("sampling".into(), Value::Null);
            }),
            (k @ ("step" | "left" | "right"), v) if has => {
                let x = cc_civil::js::str_to_number(v);
                if x.is_nan() || x <= 0.0 || v.is_empty() {
                    return Err(format!("civil.sampling: {k} must be more than 0"));
                }
                let key = k.to_string();
                edit(self, &t(&format!("sec.undo.{k}"), &[]), &mut |m| {
                    if let Some(s) = m.get_mut("sampling").and_then(Value::as_object_mut) {
                        s.insert(key.clone(), jnum(x));
                    }
                });
            }
            ("extra+", v) if has => {
                let x = parse_station(v, &units).ok_or_else(|| format!("civil.sampling: {v} is not a station"))?;
                edit(self, &t("sec.undo.extra", &[]), &mut |m| {
                    if let Some(s) = m.get_mut("sampling").and_then(Value::as_object_mut) {
                        let mut list: Vec<f64> = arr(s.get("extra")).iter().map(|v| numf(Some(v))).collect();
                        if !list.contains(&x) {
                            list.push(x);
                        }
                        cc_civil::js::sort_by(&mut list, |a, b| a - b);
                        s.insert("extra".into(), Value::Array(list.into_iter().map(jnum).collect()));
                    }
                });
            }
            ("extra-", v) if has => {
                let x = cc_civil::js::str_to_number(v);
                edit(self, &t("sec.undo.extra.remove", &[]), &mut |m| {
                    if let Some(s) = m.get_mut("sampling").and_then(Value::as_object_mut) {
                        let list: Vec<Value> = arr(s.get("extra")).iter().filter(|y| numf(Some(y)) != x).cloned().collect();
                        s.insert("extra".into(), Value::Array(list));
                    }
                });
            }
            _ if !has => return Err("civil.sampling: the alignment has no sample lines (add them first)".into()),
            _ => return Err(format!("civil.sampling: unknown edit {spec}")),
        }
        Ok(())
    }

    /// The section CSV (sections-ui.js CSV button): file name and bytes, and what is said.
    pub fn sections_csv(&mut self) -> Option<(String, Vec<u8>, String)> {
        let a = self.sections_alignment()?;
        let set = self.project.sections(&a)?;
        let units = self.project.units().to_string();
        let station = |s: f64| format_station(s, &units, None);
        let text = format!("\u{feff}{}", sections_csv(&set, &units, &station));
        let stem = crate::civilexport::file_stem(self.project.state.get("name").and_then(Value::as_str).unwrap_or(""));
        let name = format!("{stem} {} sections.csv", string_of(a.get("name")));
        Some((name, text.into_bytes(), tr!("sec.csv.done", n = set.sections.len())))
    }

    /// The current section line on the plan (sections-ui.js `overlay`).
    pub fn section_line(&mut self) -> Option<((f64, f64), (f64, f64))> {
        if !self.sections.open {
            return None;
        }
        let a = self.sections_alignment()?;
        let set = self.project.sections(&a)?;
        let s = set.sections.get(self.sections.idx)?;
        Some(((s.a.x, s.a.y), (s.b.x, s.b.y)))
    }
}

/// What the panel asks for.
pub enum SecAct {
    Run(&'static str, String),
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// The section panel at the bottom of the plan area.
pub fn panel(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, pal: &Palette, acts: &mut Vec<SecAct>) {
    let units = cv.project.units().to_string();
    let a = cv.sections_alignment();
    let set = a.as_ref().and_then(|a| cv.project.sections(a));
    let n = set.as_ref().map_or(0, |s| s.sections.len());
    if cv.sections.idx >= n {
        cv.sections.idx = n.saturating_sub(1);
    }
    let idx = cv.sections.idx;
    ui.horizontal(|ui| {
        ui.label(RichText::new(t("sec.title", &[])).strong().color(tk.text_strong));
        let als: Vec<(String, String)> =
            arr(cv.project.state.get("alignments")).iter().map(|x| (string_of(x.get("id")), string_of(x.get("name")))).collect();
        let cur = a.as_ref().map(|x| string_of(x.get("id"))).unwrap_or_default();
        let cur_name = als.iter().find(|x| x.0 == cur).map(|x| x.1.clone()).unwrap_or_default();
        ui.add_enabled_ui(als.len() >= 2, |ui| {
            egui::ComboBox::new("civil_sec_al", "").selected_text(cur_name).show_ui(ui, |ui| {
                for (id, name) in &als {
                    if ui.selectable_label(*id == cur, name).clicked() {
                        acts.push(SecAct::Run("civil.sections.al", id.clone()));
                    }
                }
            });
        });
        if ui.add_enabled(idx > 0, egui::Button::new("‹")).on_hover_text(t("sec.prev", &[])).clicked() {
            acts.push(SecAct::Run("civil.sections.go", "prev".into()));
        }
        let shown = set.as_ref().and_then(|s| s.sections.get(idx)).map_or(String::new(), |s| format_station(s.sta, &units, None));
        ui.add_enabled_ui(n > 0, |ui| {
            egui::ComboBox::new("civil_sec_sta", "").selected_text(shown).show_ui(ui, |ui| {
                if let Some(s) = &set {
                    for (i, x) in s.sections.iter().enumerate() {
                        if ui.selectable_label(i == idx, format_station(x.sta, &units, None)).clicked() {
                            acts.push(SecAct::Run("civil.sections.go", i.to_string()));
                        }
                    }
                }
            });
        });
        if ui.add_enabled(n > 0 && idx + 1 < n, egui::Button::new("›")).on_hover_text(t("sec.next", &[])).clicked() {
            acts.push(SecAct::Run("civil.sections.go", "next".into()));
        }
        let ve_name = |v: f64| if v == 0.0 { t("sec.ve.fit", &[]) } else { format!("{}×", cc_civil::js::num_to_string(v)) };
        egui::ComboBox::new("civil_sec_ve", "").selected_text(ve_name(cv.sections.ve)).show_ui(ui, |ui| {
            for v in VES {
                if ui.selectable_label(cv.sections.ve == v, ve_name(v)).clicked() {
                    acts.push(SecAct::Run("civil.sections.ve", cc_civil::js::num_to_string(v)));
                }
            }
        });
        if ui.add_enabled(n > 0, egui::Button::new(t("sec.csv", &[]))).on_hover_text(t("sec.csv.title", &[])).clicked() {
            acts.push(SecAct::Run("civil.sections.csv", String::new()));
        }
        if let Some(s) = &set
            && n > 0
        {
            let read = tr!("sec.read", cut = volume_text(s.total_cut, &units, 1), fill = volume_text(s.total_fill, &units, 1));
            ui.label(RichText::new(read).small().color(tk.text_dim));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("×").on_hover_text(t("sec.close", &[])).clicked() {
                acts.push(SecAct::Run("civil.sections", "off".into()));
            }
        });
    });
    let rect = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(rect, Sense::click());
    if resp.hovered() || resp.has_focus() {
        let (l, r, home, end) = ui.ctx().input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::Home),
                i.key_pressed(egui::Key::End),
            )
        });
        for (on, what) in [(l, "prev"), (r, "next"), (home, "first"), (end, "last")] {
            if on {
                acts.push(SecAct::Run("civil.sections.go", what.into()));
            }
        }
    }
    let painter = ui.painter_at(rect);
    let bg = pal.bg;
    painter.rect_filled(rect, 0.0, bg);
    let font = egui::FontId::proportional(12.0);
    let sec = set.as_ref().and_then(|s| s.sections.get(idx));
    let Some(s) = sec else {
        let empty = match &a {
            None => t("sec.empty.alignment", &[]),
            Some(a) if !cc_civil::js::truthy(a.get("sampling")) => t("sec.empty.sampling", &[]),
            _ => t("sec.empty.none", &[]),
        };
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, empty, font, pal.ink2);
        return;
    };
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
    let p = SecPlot::fit(s, w, h, cv.sections.ve);
    let at = |o: f64, z: f64| {
        let (x, y) = p.to_screen(o, z);
        Pos2::new(rect.min.x + x as f32, rect.min.y + y as f32)
    };
    let small = egui::FontId::proportional(11.0);
    let px_per_o = (p.x1 - p.x0) / (p.o1 - p.o0);
    let px_per_z = (p.y0 - p.y1) / (p.z1 - p.z0);
    let o_step = nice_step(60.0 / px_per_o, &[1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 500.0]);
    let z_step = nice_step(24.0 / px_per_z, &[0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0]);
    let grid = Stroke::new(1.0, mix(pal.ink, bg, 0.82));
    let frame = Rect::from_min_max(
        Pos2::new(rect.min.x + p.x0 as f32, rect.min.y + p.y1 as f32),
        Pos2::new(rect.min.x + p.x1 as f32, rect.min.y + p.y0 as f32),
    );
    let mut o = (p.o0 / o_step).ceil() * o_step;
    let mut guard = 0;
    while o <= p.o1 + 1e-9 && guard < 400 {
        let x = at(o, 0.0).x;
        painter.line_segment([Pos2::new(x, frame.min.y), Pos2::new(x, frame.max.y)], grid);
        painter.text(
            Pos2::new(x, frame.max.y + 6.0),
            egui::Align2::CENTER_TOP,
            format!("{}{}", if o > 0.0 { "+" } else { "" }, fixed(o, 0)),
            small.clone(),
            pal.ink2,
        );
        o += o_step;
        guard += 1;
    }
    let mut z = (p.z0 / z_step).ceil() * z_step;
    guard = 0;
    while z <= p.z1 + 1e-9 && guard < 400 {
        let y = at(0.0, z).y;
        painter.line_segment([Pos2::new(frame.min.x, y), Pos2::new(frame.max.x, y)], grid);
        painter.text(
            Pos2::new(frame.min.x - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            fixed(z, if z_step < 1.0 { 1 } else { 0 }),
            small.clone(),
            pal.ink2,
        );
        z += z_step;
        guard += 1;
    }
    painter.rect_stroke(frame, 0.0, Stroke::new(1.0, pal.ink), egui::StrokeKind::Middle);
    let cl = at(0.0, 0.0).x;
    painter.add(egui::Shape::dashed_line(&[Pos2::new(cl, frame.min.y), Pos2::new(cl, frame.max.y)], Stroke::new(1.0, pal.red), 6.0, 3.0));
    let clip = painter.with_clip_rect(frame);
    // cut and fill: the band between the two lines over each part
    let mut os: Vec<f64> = s.ground.iter().chain(s.design.iter()).flatten().map(|q| q.o).collect();
    os.sort_by(f64::total_cmp);
    os.dedup();
    for part in &s.parts {
        let mut xs = vec![part.o0];
        xs.extend(os.iter().copied().filter(|o| *o > part.o0 && *o < part.o1));
        xs.push(part.o1);
        let pairs: Vec<(Pos2, Pos2)> =
            xs.iter().filter_map(|o| Some((at(*o, z_at(&s.design, *o)?), at(*o, z_at(&s.ground, *o)?)))).collect();
        if pairs.len() >= 2 {
            let col = (if part.cut { pal.orange } else { pal.green }).gamma_multiply(0.28);
            // the band between the design (top) and the ground (bottom): two triangles per step across
            let mut mesh = egui::Mesh::default();
            for (top, bot) in &pairs {
                mesh.colored_vertex(*top, col);
                mesh.colored_vertex(*bot, col);
            }
            for i in 0..pairs.len() - 1 {
                let k = u32::try_from(2 * i).unwrap_or(0);
                mesh.add_triangle(k, k + 2, k + 1);
                mesh.add_triangle(k + 1, k + 2, k + 3);
            }
            clip.add(egui::Shape::mesh(mesh));
        }
    }
    let egc = mix(pal.ink, bg, 0.25);
    for r in &s.ground {
        clip.add(egui::Shape::line(r.iter().map(|q| at(q.o, q.z)).collect(), Stroke::new(1.6, egc)));
    }
    if !s.same {
        for r in &s.design {
            clip.add(egui::Shape::line(r.iter().map(|q| at(q.o, q.z)).collect(), Stroke::new(2.2, pal.red)));
        }
    }
    // corridor points: labels that would touch the one before go a line higher
    let bold = egui::FontId::proportional(11.0);
    let mut last_x = f32::NEG_INFINITY;
    let mut row = 0;
    for m in &s.marks {
        let Some(z) = m.z.filter(|z| z.is_finite()) else { continue };
        let q = at(m.o, z);
        painter.circle(q, 3.5, bg, Stroke::new(1.5, pal.ochre));
        let text = m.code.strip_suffix("_L").or_else(|| m.code.strip_suffix("_R")).unwrap_or(&m.code).to_string();
        let galley = painter.layout_no_wrap(text, bold.clone(), pal.ink2);
        let wd = galley.size().x;
        row = if q.x - last_x < wd + 6.0 { (row + 1) % 2 } else { 0 };
        last_x = q.x;
        let pos = Pos2::new(q.x - wd / 2.0, q.y - 6.0 - row as f32 * 13.0 - galley.size().y);
        painter.galley(pos, galley, pal.ink2);
    }
    // header: station, areas, exaggeration
    let u = unit_of(&units);
    painter.text(
        Pos2::new(frame.min.x, rect.min.y + 6.0),
        egui::Align2::LEFT_TOP,
        tr!("sec.station", sta = format_station(s.sta, &units, None)),
        egui::FontId::proportional(12.0),
        pal.ink,
    );
    let ve = if p.ve >= 10.0 { cc_civil::js::num_to_string(p.ve.round()) } else { fixed(p.ve, 1) };
    let head = [
        tr!("sec.area.cut", a = fixed(s.cut, 2), u = u.area),
        tr!("sec.area.fill", a = fixed(s.fill, 2), u = u.area),
        tr!("sec.ve", ve = ve),
    ]
    .join("   ");
    painter.text(Pos2::new(frame.max.x, rect.min.y + 6.0), egui::Align2::RIGHT_TOP, head, egui::FontId::proportional(12.0), pal.ink2);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_lines_are_edited_as_single_steps_and_the_csv_names_the_alignment() {
        let mut cv = CivilView::demo().unwrap();
        cv.sel = Some(Sel::Alignment("a1".into()));
        assert!(cv.sampling_edit("step=0").is_err());
        cv.sampling_edit("step=25").unwrap();
        cv.sampling_edit("extra+=1+05").unwrap();
        cv.sampling_edit("extra+=1+05").unwrap();
        let smp = &cv.project.state["alignments"][0]["sampling"];
        assert_eq!(smp["step"], serde_json::json!(25.0));
        assert_eq!(arr(smp.get("extra")).len(), 1);
        cv.show_sections(true, Some("a1"));
        cv.sections_go(1_000_000);
        let n = cv.project.sections(&cv.project.state["alignments"][0].clone()).unwrap().sections.len();
        assert_eq!(cv.sections.idx, n - 1);
        let (name, bytes, said) = cv.sections_csv().unwrap();
        assert!(name.ends_with("Main Street sections.csv"), "{name}");
        assert!(bytes.starts_with("\u{feff}".as_bytes()));
        assert!(said.contains(&n.to_string()));
        assert!(cv.section_line().is_some());
        cv.sampling_edit("extra-=105").unwrap();
        cv.sampling_edit("remove").unwrap();
        assert!(cv.project.state["alignments"][0]["sampling"].is_null());
        assert!(cv.sampling_edit("left=3").is_err());
        cv.sampling_edit("add").unwrap();
        assert_eq!(cv.project.state["alignments"][0]["sampling"]["step"], serde_json::json!(50.0));
    }

    #[test]
    fn a_section_fits_its_elevations_at_least_one_to_one() {
        let mut cv = CivilView::demo().unwrap();
        let a = cv.project.state["alignments"][0].clone();
        let set = cv.project.sections(&a).unwrap();
        let s = set.sections.first().unwrap();
        let p = SecPlot::fit(s, 900.0, 260.0, 0.0);
        assert!(p.ve >= 1.0 && p.z1 > p.z0);
        let p5 = SecPlot::fit(s, 900.0, 260.0, 5.0);
        assert_eq!(p5.ve, 5.0);
        assert!(z_at(&s.ground, 0.0).is_some());
    }
}
