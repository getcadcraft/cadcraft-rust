//! The corridor block's results and superelevation's per-curve table and diagram (civilcraft.js `corridorProps` after
//! its fields, super-ui.js `superProps` after the rate table and `drawDiagram`): the corridor's problems (the ones
//! superelevation does not say itself) or its facts; superelevation's problems, each curve's radius, rate, runoff, runout
//! and full-superelevation stations, and the diagram of both lanes' cross slopes along the stations. Everything comes
//! from the graded corridor item (`Project::graded`, its `super` plan); `civil.se.info` says the same on the status line.

use cc_civil::js::{arr, numf, string_of};
use cc_civil::tr;
use cc_civil::units::{fixed, format_station, volume_text};
use egui::{Color32, Pos2, RichText, Stroke};
use serde_json::Value;

use crate::civilview::{CivilView, t0};
use crate::theme::Tokens;

/// What the corridor block shows under its fields.
pub struct CorridorFacts {
    /// the corridor's problems superelevation does not list itself
    pub others: Vec<String>,
    /// the facts line (when there are no other problems and the corridor was built)
    pub facts: Option<String>,
    /// superelevation's plan: None = no graded item
    pub plan: Option<Value>,
}

impl CivilView {
    /// civilcraft.js corridorProps: `it = graded().items.find(corridor of a)`, its problems less super's, else facts.
    pub fn corridor_facts(&mut self, a: &Value) -> CorridorFacts {
        self.refresh_facts();
        let units = self.project.units().to_string();
        let g = self.project.graded();
        let id = a.get("id").cloned().unwrap_or(Value::Null);
        let Some(it) = g.items.iter().find(|x| x.kind == "corridor" && x.id == id) else {
            return CorridorFacts { others: Vec::new(), facts: None, plan: None };
        };
        let plan = it.info.get("super").filter(|p| !p.is_null()).cloned();
        let se: Vec<String> =
            plan.as_ref().map(|p| arr(p.get("problems")).iter().map(|x| string_of(Some(x))).collect()).unwrap_or_default();
        let others: Vec<String> = it.problems.iter().filter(|p| !se.contains(p)).cloned().collect();
        let mut facts = None;
        if others.is_empty()
            && let Some(secs) = it.info.get("sections").and_then(Value::as_array)
        {
            let st = |k: &str| format_station(numf(it.info.get(k)), &units, None);
            let vi = self.facts.volumes.as_ref().and_then(|v| arr(v.get("items")).iter().find(|x| x.get("id") == Some(&id)).cloned());
            facts = Some(match vi {
                Some(vi) => tr!(
                    "cv.co.facts.vol",
                    n = secs.len(),
                    from = st("from"),
                    to = st("to"),
                    cut = volume_text(numf(vi.get("cut")), &units, 1),
                    fill = volume_text(numf(vi.get("fill")), &units, 1)
                ),
                None => tr!("cv.co.facts", n = secs.len(), from = st("from"), to = st("to")),
            });
        }
        CorridorFacts { others, facts, plan }
    }

    /// `civil.se.info`: the selected alignment's corridor facts or problems, superelevation's problems, its curve rows
    /// ("#  R  e  Lr  Lt  fs0 – fs1") or "none", as the Properties panel shows them.
    pub fn se_info(&mut self) -> Result<String, String> {
        let a = match &self.sel {
            Some(crate::civilview::Sel::Alignment(id)) => {
                arr(self.project.state.get("alignments")).iter().find(|x| x.get("id").and_then(Value::as_str) == Some(id.as_str())).cloned()
            }
            _ => None,
        }
        .ok_or("civil.se.info: select an alignment first")?;
        let co = a.get("corridor").cloned().unwrap_or(Value::Null);
        if co.get("on") != Some(&Value::Bool(true)) {
            return Ok(t0("cv.co.off.note"));
        }
        let f = self.corridor_facts(&a);
        let mut out: Vec<String> = Vec::new();
        out.extend(f.others.iter().cloned());
        out.extend(f.facts.iter().cloned());
        let se_on = co.get("super").and_then(|s| s.get("on")) == Some(&Value::Bool(true));
        if !se_on {
            out.push(t0("se.off.note"));
            return Ok(out.join(" | "));
        }
        let units = self.project.units().to_string();
        let plan = f.plan.unwrap_or(Value::Null);
        out.extend(arr(plan.get("problems")).iter().map(|x| string_of(Some(x))));
        let rows = curve_rows(&plan, &units);
        if rows.is_empty() {
            out.push(t0("se.none"));
        } else {
            out.extend(rows.into_iter().map(|r| r.join("  ")));
        }
        Ok(out.join(" | "))
    }
}

/// The curves table's rows: #, R (0 dp), e % (1 dp), runoff, runout (1 dp), "fs0 – fs1".
fn curve_rows(plan: &Value, units: &str) -> Vec<[String; 6]> {
    arr(plan.get("curves"))
        .iter()
        .map(|c| {
            let st = c.get("stations");
            let s = |k: &str| format_station(numf(st.and_then(|x| x.get(k))), units, None);
            [
                cc_civil::js::num_to_string(numf(c.get("i"))),
                fixed(numf(c.get("R")), 0),
                format!("{}%", fixed(numf(c.get("e")) * 100.0, 1)),
                fixed(numf(c.get("Lr")), 1),
                fixed(numf(c.get("Lt")), 1),
                format!("{} – {}", s("fs0"), s("fs1")),
            ]
        })
        .collect()
}

/// The lines under the corridor's fields: other problems (red report) or the facts note.
pub fn corridor_block(ui: &mut egui::Ui, tk: &Tokens, f: &CorridorFacts) {
    if !f.others.is_empty() {
        crate::civilpanel::report_pub(ui, tk, &f.others, true);
    } else if let Some(s) = &f.facts {
        ui.label(RichText::new(s).small().color(tk.text_dim));
    }
}

/// super-ui.js after the rate note: problems, the curves table, the diagram (or "none").
pub fn super_block(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, a: &Value, f: &CorridorFacts) {
    let units = cv.project.units().to_string();
    let plan = f.plan.clone().unwrap_or(Value::Null);
    let probs: Vec<String> = arr(plan.get("problems")).iter().map(|x| string_of(Some(x))).collect();
    crate::civilpanel::report_pub(ui, tk, &probs, true);
    let rows = curve_rows(&plan, &units);
    if rows.is_empty() {
        ui.label(RichText::new(t0("se.none")).small().color(tk.text_dim));
        return;
    }
    let aid = string_of(a.get("id"));
    egui::Grid::new(("cv_se_curves", aid.as_str())).num_columns(6).spacing([10.0, 2.0]).striped(true).show(ui, |ui| {
        for h in ["#".to_string(), t0("se.c.r"), "e".to_string(), t0("se.c.lr"), t0("se.c.lt"), t0("se.c.full")] {
            ui.label(RichText::new(h).small().strong().color(tk.text_dim));
        }
        ui.end_row();
        for r in &rows {
            for (k, c) in r.iter().enumerate() {
                let txt = RichText::new(c).small().color(tk.text);
                if (1..=4).contains(&k) {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(txt));
                } else {
                    ui.label(txt);
                }
            }
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    ui.label(RichText::new(t0("se.diagram")).strong().color(tk.text_strong));
    // the typed plan for slopesAt: the same superPlan the corridor ran (its assembly over the defaults)
    let asm = cc_civil::corridor::read_assembly(a.get("corridor").unwrap_or(&Value::Null), &units);
    let al = cv.project.alignment(a);
    if !al.ok() {
        return;
    }
    let tp = cc_civil::superelev::super_plan(&al, asm.sup.as_ref(), asm.lane_slope, asm.lane);
    diagram(ui, tk, cv.dark, &tp, al.start, al.end, asm.lane_slope, &units);
}

/// super-ui.js `drawDiagram`: station across, the lane cross slope up (falls away = down), left lane solid red, right
/// lane dashed blue, every curve's key stations ticked, the first and last stations under the axis.
#[allow(clippy::too_many_arguments)]
fn diagram(ui: &mut egui::Ui, tk: &Tokens, dark: bool, plan: &cc_civil::superelev::Plan, start: f64, end: f64, nc: f64, units: &str) {
    let w = ui.available_width().max(200.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 176.0), egui::Sense::hover());
    resp.on_hover_text(t0("se.diagram.label"));
    let painter = ui.painter_at(rect);
    let pal = crate::civilplan::Palette::of(tk, dark);
    painter.rect_filled(rect, 0.0, pal.bg);
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, pal.line), egui::StrokeKind::Inside);
    let emax = plan.curves.iter().fold(nc, |m, c| m.max(c.e)) * 1.25;
    let (pl, pr, pt, pb) = (40.0f64, 8.0f64, 10.0f64, 22.0f64);
    let (wf, hf) = (f64::from(rect.width()), f64::from(rect.height()));
    let x = |s: f64| rect.min.x + (pl + ((s - start) / (end - start).max(1e-9)) * (wf - pl - pr)) as f32;
    let y = |v: f64| rect.min.y + (pt + ((v + emax) / (2.0 * emax).max(1e-12)) * (hf - pt - pb)) as f32;
    let font = egui::FontId::proportional(11.0);
    let grid = Stroke::new(1.0, pal.line);
    for v in [-emax / 1.25, -nc, 0.0, nc, emax / 1.25] {
        let yy = y(v);
        painter.line_segment([Pos2::new(rect.min.x + pl as f32, yy), Pos2::new(rect.max.x - pr as f32, yy)], grid);
        let sign = if v > 0.0 {
            "−"
        } else if v < 0.0 {
            "+"
        } else {
            ""
        };
        painter.text(
            Pos2::new(rect.min.x + pl as f32 - 4.0, yy),
            egui::Align2::RIGHT_CENTER,
            format!("{sign}{}%", fixed(v.abs() * 100.0, 1)),
            font.clone(),
            pal.ink,
        );
    }
    let n = 400usize;
    let line = |left: bool, col: Color32| {
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let s = start + ((end - start) * i as f64) / n as f64;
                let (l, r) = cc_civil::superelev::slopes_at(plan, s, nc);
                Pos2::new(x(s), y(if left { l } else { r }))
            })
            .collect();
        if left {
            painter.add(egui::Shape::line(pts, Stroke::new(2.0, col)));
        } else {
            painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(2.0, col), 6.0, 4.0));
        }
    };
    line(true, pal.red);
    line(false, pal.blue);
    let base = rect.max.y - pb as f32;
    for c in &plan.curves {
        let s = &c.stations;
        for k in [s.lc0, s.fs0, s.fs1, s.lc1] {
            let xx = x(k);
            painter.line_segment([Pos2::new(xx, base), Pos2::new(xx, base + 4.0)], Stroke::new(1.0, pal.ink));
        }
    }
    painter.text(
        Pos2::new(rect.min.x + pl as f32, base + 6.0),
        egui::Align2::LEFT_TOP,
        format_station(start, units, Some(0)),
        font.clone(),
        pal.ink,
    );
    painter.text(
        Pos2::new(rect.max.x - pr as f32, base + 6.0),
        egui::Align2::RIGHT_TOP,
        format_station(end, units, Some(0)),
        font,
        pal.ink,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civiltools::Tool;

    #[test]
    fn the_curve_table_lists_each_superelevated_curve() {
        let mut cv = CivilView::sample();
        cv.view.width = 1000.0;
        cv.view.height = 800.0;
        cv.view.cx = 6_150_200.0;
        cv.view.cy = 2_080_130.0;
        cv.view.zoom = 2.4;
        cv.tool_command(Tool::Alignment);
        for (i, j) in [(-4.0, -2.5), (-1.5, 2.5), (1.5, -2.5), (4.0, 2.5)] {
            cv.click(6_150_200.0 + i * 40.0, 2_080_130.0 + j * 40.0);
        }
        cv.finish();
        assert!(cv.se_info().unwrap().contains(&t0("cv.co.off.note")));
        cv.prop_set_more("corridor", "on").unwrap();
        cv.start_profile().unwrap();
        cv.prop_set_more("se", "on").unwrap();
        cv.pi_set(1, "r", "60").unwrap();
        cv.pi_set(2, "r", "45").unwrap();
        cv.prop_set_more("se.delta", "1.2").unwrap();
        let info = cv.se_info().unwrap();
        assert!(info.contains("1  60  6.0%  60.0  20.0"), "{info}");
        cv.sel = None;
        assert!(cv.se_info().is_err());
    }
}
