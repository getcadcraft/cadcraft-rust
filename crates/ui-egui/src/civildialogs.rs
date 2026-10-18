//! CivilCraft's typed-entry dialogs (M11.7): the Traverse dialog (traverse-ui.js) and the parcel-from-deed-calls dialog
//! (parcels-ui.js `openCalls`). The form is read as it is typed — the engine's `parse_traverse` / `run_traverse` and
//! `parse_calls` / `run_calls` (at parity with traverse.js and parcels.js) — and its result is added as survey points or
//! a parcel in one undo step, with the words and JSON 0.5 uses. Both are commands too (`civil.traverse`, `civil.calls`,
//! a JSON form in `name`), so an agent or a test fills and adds them without the window.

use cc_civil::geo::P2;
use cc_civil::i18n::t;
use cc_civil::jo;
use cc_civil::js::{JsMath, arr, num_to_string, str_to_number};
use cc_civil::parcels::{Corner, area_words, parcel_area, parse_angle, parse_calls, run_calls};
use cc_civil::points::Pt;
use cc_civil::tr;
use cc_civil::traverse::{Traverse, parse_traverse, run_traverse, traverse_report};
use cc_civil::units::{bearing, dms, fixed, grouped};
use egui::RichText;
use serde_json::Value;

use crate::civilview::{CivilView, Sel};
use crate::theme::Tokens;

/// The Traverse dialog's fields (traverse-ui.js `open`).
#[derive(Clone, Debug, PartialEq)]
pub struct TraverseForm {
    pub open: bool,
    pub start: String,
    pub north: String,
    pub east: String,
    /// "angles" | "bearings"
    pub mode: String,
    pub bs: String,
    pub az: String,
    /// "loop" | "pt"
    pub ends: String,
    pub end: String,
    pub legs: String,
}
impl Default for TraverseForm {
    fn default() -> Self {
        TraverseForm {
            open: false,
            start: String::new(),
            north: String::new(),
            east: String::new(),
            mode: "angles".into(),
            bs: String::new(),
            az: String::new(),
            ends: "loop".into(),
            end: String::new(),
            legs: String::new(),
        }
    }
}

/// The deed calls dialog's fields (parcels-ui.js `openCalls`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CallsForm {
    pub open: bool,
    /// empty = the next default name
    pub name: String,
    pub start: String,
    pub north: String,
    pub east: String,
    pub calls: String,
}

/// What the form reads as now.
#[derive(Clone, Debug)]
pub enum Read<T> {
    /// nothing typed yet: the help text (a language key)
    Help(&'static str),
    Errors(Vec<String>),
    Ok(T),
}

/// A traverse read: the run and the survey point it starts on.
#[derive(Clone, Debug)]
pub struct TraverseRead {
    pub res: Traverse,
    pub start_n: Option<String>,
    pub loop_: bool,
}

/// Deed calls read: the corners, whether they close, where they start (for the point of beginning).
#[derive(Clone, Debug)]
pub struct CallsRead {
    pub pts: Vec<Corner>,
    pub closes: bool,
    pub from: Option<String>,
    pub courses: usize,
    pub length: f64,
    pub mis: (f64, f64),
    pub precision: f64,
}

/// Both dialogs' state.
#[derive(Clone, Debug, Default)]
pub struct Dialogs {
    pub traverse: TraverseForm,
    pub calls: CallsForm,
}

fn point_by<'a>(points: &'a [Pt], n: &str) -> Option<&'a Pt> {
    let n = n.trim();
    points.iter().find(|q| q.n == n)
}

/// traverse-ui.js `update()`.
pub fn read_traverse(points: &[Pt], f: &TraverseForm) -> Read<TraverseRead> {
    let start: Result<(P2, Option<String>), String> = if !f.start.trim().is_empty() {
        match point_by(points, &f.start) {
            Some(q) => Ok((P2 { x: q.x, y: q.y }, Some(q.n.clone()))),
            None => Err(tr!("pc.dlg.nopt", n = f.start.trim())),
        }
    } else if f.north.trim().is_empty() && f.east.trim().is_empty() {
        Err(t("pc.dlg.nostart", &[]))
    } else {
        let (x, y) = (str_to_number(&f.east), str_to_number(&f.north));
        if x.is_finite() && y.is_finite() { Ok((P2 { x, y }, None)) } else { Err(t("pc.dlg.badne", &[])) }
    };
    let angles = f.mode == "angles";
    let (legs, errors, closing) = parse_traverse(&f.legs, if angles { "angles" } else { "bearings" });
    let mut errs = Vec::new();
    if let Err(e) = &start {
        errs.push(e.clone());
    }
    let mut backsight = 0.0;
    if angles {
        if !f.bs.trim().is_empty() {
            match point_by(points, &f.bs) {
                None => errs.push(tr!("pc.dlg.nopt", n = f.bs.trim())),
                Some(q) => {
                    if let Ok((s, _)) = &start {
                        backsight = (90.0 - (q.y - s.y).jatan2(q.x - s.x) * 180.0 / std::f64::consts::PI + 360.0) % 360.0;
                    }
                }
            }
        } else if !f.az.trim().is_empty() {
            match parse_angle(&f.az) {
                Some(a) if a < 360.0 => backsight = a,
                _ => errs.push(t("tv.err.bsaz", &[])),
            }
        } else {
            errs.push(t("tv.err.bs", &[]));
        }
    }
    let mut close: Option<P2> = None;
    let pt_end = f.ends == "pt";
    if pt_end {
        match point_by(points, &f.end) {
            None => errs.push(if f.end.trim().is_empty() { t("tv.err.end", &[]) } else { tr!("pc.dlg.nopt", n = f.end.trim()) }),
            Some(q) => close = Some(P2 { x: q.x, y: q.y }),
        }
    }
    for e in errors.iter().take(20) {
        errs.push(format!("{} {}", tr!("pc.dlg.line", n = e.line), e.msg));
    }
    if legs.is_empty() && errors.is_empty() {
        return Read::Help(if angles { "tv.help.angles" } else { "tv.help.bearings" });
    }
    if legs.len() < 2 {
        errs.push(t("tv.few", &[]));
    }
    let Ok((s, start_n)) = start else { return Read::Errors(errs) };
    if !errs.is_empty() || (pt_end && close.is_none()) {
        return Read::Errors(errs);
    }
    let res = run_traverse(s, if angles { "angles" } else { "bearings" }, backsight, &legs, close, closing);
    Read::Ok(TraverseRead { res, start_n, loop_: !pt_end })
}

/// parcels-ui.js `openCalls` → `update()`.
pub fn read_calls(points: &[Pt], f: &CallsForm) -> Read<CallsRead> {
    let start: Result<(P2, Option<String>), String> = {
        let n = f.start.trim();
        if !n.is_empty() {
            match points.iter().find(|x| x.n == n) {
                Some(q) => Ok((P2 { x: q.x, y: q.y }, Some(tr!("pc.dlg.from.pt", n = n)))),
                None => Err(tr!("pc.dlg.nopt", n = n)),
            }
        } else {
            let (y, x) = (str_to_number(&f.north), str_to_number(&f.east));
            if f.north.trim().is_empty() && f.east.trim().is_empty() {
                Err(t("pc.dlg.nostart", &[]))
            } else if x.is_finite() && y.is_finite() {
                Ok((P2 { x, y }, None))
            } else {
                Err(t("pc.dlg.badne", &[]))
            }
        }
    };
    let (calls, errors) = parse_calls(&f.calls);
    let mut lines = Vec::new();
    if let Err(e) = &start {
        lines.push(e.clone());
    }
    for e in errors.iter().take(20) {
        lines.push(format!("{} {}", tr!("pc.dlg.line", n = e.line), e.msg));
    }
    if calls.is_empty() && errors.is_empty() {
        return Read::Help("pc.dlg.help");
    }
    let Ok((s, from)) = start else { return Read::Errors(lines) };
    if !lines.is_empty() {
        return Read::Errors(lines);
    }
    let run = run_calls(s, &calls);
    let closes = run.dist <= 0.0005 * (run.length / 1000.0).max(1.0);
    let mut pts = run.pts.clone();
    if closes {
        pts.pop();
    }
    if pts.len() < 3 && !pts.iter().any(|q| q.bulge().is_some()) {
        return Read::Errors(vec![t("pc.few", &[])]);
    }
    Read::Ok(CallsRead { pts, closes, from, courses: calls.len(), length: run.length, mis: (run.dist, run.dir), precision: run.precision })
}

impl CivilView {
    /// parcels-ui.js `add(name, pts, label, pob)`: one undo step, the parcel selected.
    fn add_parcel(&mut self, name: &str, pts: &[Corner], label: &str, pob: Option<&str>) {
        let id = self.project.next_id("r");
        let mut p = jo! { "id" => id, "name" => name,
        "pts" => Value::Array(pts.iter().map(|q| match q.bulge() { Some(b) => jo! { "x" => q.x, "y" => q.y, "b" => b }, None => jo! { "x" => q.x, "y" => q.y } }).collect()) };
        if let (Some(pob), Some(m)) = (pob, p.as_object_mut()) {
            m.insert("pob".into(), Value::String(pob.to_string()));
        }
        self.project.change(label, &mut |s: &mut Value| {
            if let Some(m) = s.as_object_mut() {
                if !m.get("parcels").is_some_and(Value::is_array) {
                    m.insert("parcels".into(), Value::Array(Vec::new()));
                }
                if let Some(l) = m.get_mut("parcels").and_then(Value::as_array_mut) {
                    l.push(p.clone());
                }
            }
        });
        self.sel = Some(Sel::Parcel(id));
    }

    /// The traverse's stations as survey points (traverse-ui.js Add as survey points).
    pub fn traverse_add_points(&mut self, r: &TraverseRead) {
        let mut taken: std::collections::HashSet<String> = self.project.points.iter().map(|q| q.n.clone()).collect();
        let mut next = self
            .project
            .points
            .iter()
            .filter(|q| !q.n.is_empty() && q.n.bytes().all(|b| b.is_ascii_digit()))
            .fold(0.0f64, |m, q| m.max(str_to_number(&q.n)))
            + 1.0;
        let sts = if r.loop_ { r.res.stations.get(..r.res.stations.len().saturating_sub(1)).unwrap_or(&[]) } else { &r.res.stations[..] };
        let mut pts: Vec<Pt> = Vec::with_capacity(sts.len());
        for st in sts {
            let n = if !st.name.is_empty() && !taken.contains(&st.name) {
                st.name.clone()
            } else {
                while taken.contains(&num_to_string(next)) {
                    next += 1.0;
                }
                let s = num_to_string(next);
                next += 1.0;
                s
            };
            taken.insert(n.clone());
            pts.push(Pt { n, x: st.ax, y: st.ay, z: None, d: "TRAV".into(), blk: None });
        }
        if pts.is_empty() {
            return;
        }
        let label = tr!("tv.undo.points", n = pts.len());
        let add = pts.clone();
        self.project.edit_points(&label, &mut |mut all, _| {
            all.extend(add.iter().cloned());
            Some(all)
        });
        self.sel = None;
        self.message = tr!(
            "tv.added",
            n = pts.len(),
            first = pts.first().map_or("", |p| p.n.as_str()),
            last = pts.last().map_or("", |p| p.n.as_str())
        );
    }

    /// A loop traverse as a parcel (traverse-ui.js Add as a parcel → parcels-ui.js `addParcel`).
    pub fn traverse_add_parcel(&mut self, r: &TraverseRead) {
        if !r.loop_ {
            return;
        }
        let mut pts = vec![Corner { x: r.res.start.x, y: r.res.start.y, b: None }];
        let n = r.res.stations.len().saturating_sub(1);
        pts.extend(r.res.stations.iter().take(n).map(|st| Corner { x: st.ax, y: st.ay, b: None }));
        let pob = r.start_n.as_ref().map(|n| tr!("pc.dlg.from.pt", n = n.as_str()));
        let nm = format!("{} {}", t("tv.parcel", &[]), arr(self.project.state.get("parcels")).len() + 1);
        self.add_parcel(&nm, &pts, &t("pc.undo.traverse", &[]), pob.as_deref());
        let units = self.project.units().to_string();
        self.message = tr!("pc.drawn", name = nm.as_str(), area = area_words(parcel_area(&pts), &units));
    }

    /// The calls as a parcel (parcels-ui.js Add the parcel).
    pub fn calls_add(&mut self, r: &CallsRead, name: &str) {
        let typed: String = name.trim().chars().take(120).collect();
        let name =
            if typed.is_empty() { format!("{} {}", t("pc.default", &[]), arr(self.project.state.get("parcels")).len() + 1) } else { typed };
        self.add_parcel(&name, &r.pts, &t("pc.undo.calls", &[]), r.from.as_deref());
        let units = self.project.units().to_string();
        let area = area_words(parcel_area(&r.pts), &units);
        self.message = tr!(if r.closes { "pc.dlg.done" } else { "pc.dlg.done.gap" }, name = name.as_str(), area = area);
    }
}

fn s_of(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.chars().take(200_000).collect(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// `civil.traverse` {name: JSON form} → the form filled (and the dialog open); `add`: "points" | "parcel" | "csv".
/// Returns a file to save for "csv".
pub fn traverse_command(cv: &mut CivilView, spec: &str) -> Result<Option<(String, Vec<u8>)>, String> {
    let v: Value = if spec.trim().is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_str(spec).map_err(|e| format!("civil.traverse: {e}"))?
    };
    let f = &mut cv.dialogs.traverse;
    f.open = true;
    for (k, slot) in [
        ("start", &mut f.start),
        ("north", &mut f.north),
        ("east", &mut f.east),
        ("bs", &mut f.bs),
        ("az", &mut f.az),
        ("end", &mut f.end),
        ("legs", &mut f.legs),
    ] {
        if v.get(k).is_some() {
            *slot = s_of(&v, k);
        }
    }
    if let Some(m) = v.get("mode").and_then(Value::as_str) {
        if m != "angles" && m != "bearings" {
            return Err("civil.traverse: mode = angles or bearings".into());
        }
        f.mode = m.into();
    }
    if let Some(e) = v.get("ends").and_then(Value::as_str) {
        if e != "loop" && e != "pt" {
            return Err("civil.traverse: ends = loop or pt".into());
        }
        f.ends = e.into();
    }
    let add = v.get("add").and_then(Value::as_str).unwrap_or("");
    if add.is_empty() {
        return Ok(None);
    }
    let form = cv.dialogs.traverse.clone();
    let r = match read_traverse(&cv.project.points, &form) {
        Read::Ok(r) => r,
        Read::Help(k) => return Err(t(k, &[])),
        Read::Errors(e) => return Err(e.join(" ")),
    };
    match add {
        "points" => cv.traverse_add_points(&r),
        "parcel" if r.loop_ => cv.traverse_add_parcel(&r),
        "parcel" => return Err("civil.traverse: only a loop becomes a parcel".into()),
        "csv" => {
            let units = cv.project.units().to_string();
            let name = format!(
                "{} - traverse.csv",
                crate::civilexport::file_stem(cv.project.state.get("name").and_then(Value::as_str).unwrap_or(""))
            );
            return Ok(Some((name, format!("\u{feff}{}", traverse_report(&r.res, &units)).into_bytes())));
        }
        o => return Err(format!("civil.traverse: add = points, parcel or csv (not {o})")),
    }
    cv.dialogs.traverse.open = false;
    Ok(None)
}

/// `civil.calls` {name: JSON form {name, start, north, east, calls, add?: true}}.
pub fn calls_command(cv: &mut CivilView, spec: &str) -> Result<(), String> {
    let v: Value = if spec.trim().is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_str(spec).map_err(|e| format!("civil.calls: {e}"))?
    };
    let f = &mut cv.dialogs.calls;
    f.open = true;
    for (k, slot) in
        [("name", &mut f.name), ("start", &mut f.start), ("north", &mut f.north), ("east", &mut f.east), ("calls", &mut f.calls)]
    {
        if v.get(k).is_some() {
            *slot = s_of(&v, k);
        }
    }
    if v.get("add") != Some(&Value::Bool(true)) {
        return Ok(());
    }
    let form = cv.dialogs.calls.clone();
    let r = match read_calls(&cv.project.points, &form) {
        Read::Ok(r) => r,
        Read::Help(k) => return Err(t(k, &[])),
        Read::Errors(e) => return Err(e.join(" ")),
    };
    cv.calls_add(&r, &form.name);
    cv.dialogs.calls = CallsForm::default();
    Ok(())
}

/// What a dialog asks the app to do after the frame.
pub enum DlgAct {
    Save(String, Vec<u8>),
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, width: f32) {
    let l = ui.label(label);
    ui.add(egui::TextEdit::singleline(value).desired_width(width)).labelled_by(l.id);
}

/// The two dialogs as windows over the plan.
pub fn show(cv: &mut CivilView, ctx: &egui::Context, tk: &Tokens, acts: &mut Vec<DlgAct>) {
    let units = cv.project.units().to_string();
    let u = if units == "m" { "m" } else { "ft" };
    if cv.dialogs.traverse.open {
        let mut open = true;
        let mut add: Option<&str> = None;
        let mut cancel = false;
        egui::Window::new(t("tv.title", &[]))
            .open(&mut open)
            .collapsible(false)
            .resizable([true, false])
            .default_width(620.0)
            .default_pos([300.0, 80.0])
            .show(ctx, |ui| {
                let f = &mut cv.dialogs.traverse;
                ui.horizontal_wrapped(|ui| {
                    field(ui, &t("pc.dlg.pt", &[]), &mut f.start, 60.0);
                    field(ui, &t("pc.dlg.n", &[]), &mut f.north, 100.0);
                    field(ui, &t("pc.dlg.e", &[]), &mut f.east, 100.0);
                });
                ui.horizontal(|ui| {
                    ui.label(t("tv.mode", &[]));
                    egui::ComboBox::new("civil_tv_mode", "")
                        .selected_text(t(if f.mode == "angles" { "tv.mode.angles" } else { "tv.mode.bearings" }, &[]))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut f.mode, "angles".to_string(), t("tv.mode.angles", &[]));
                            ui.selectable_value(&mut f.mode, "bearings".to_string(), t("tv.mode.bearings", &[]));
                        });
                });
                if f.mode == "angles" {
                    ui.horizontal(|ui| {
                        field(ui, &t("tv.bs.pt", &[]), &mut f.bs, 70.0);
                        field(ui, &t("tv.bs.az", &[]), &mut f.az, 110.0);
                    });
                }
                ui.horizontal(|ui| {
                    ui.label(t("tv.ends", &[]));
                    egui::ComboBox::new("civil_tv_ends", "")
                        .selected_text(t(if f.ends == "loop" { "tv.ends.loop" } else { "tv.ends.pt" }, &[]))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut f.ends, "loop".to_string(), t("tv.ends.loop", &[]));
                            ui.selectable_value(&mut f.ends, "pt".to_string(), t("tv.ends.pt", &[]));
                        });
                    if f.ends == "pt" {
                        ui.add(egui::TextEdit::singleline(&mut f.end).desired_width(70.0).hint_text(t("tv.ends.pt.num", &[])));
                    }
                });
                let l = ui.label(t("tv.legs", &[]));
                ui.add(egui::TextEdit::multiline(&mut f.legs).desired_rows(8).desired_width(f32::INFINITY).code_editor()).labelled_by(l.id);
                let form = f.clone();
                let read = read_traverse(&cv.project.points, &form);
                match &read {
                    Read::Help(k) => {
                        ui.add(egui::Label::new(RichText::new(t(k, &[])).small().color(tk.text_dim)).wrap());
                    }
                    Read::Errors(e) => {
                        for m in e {
                            ui.add(egui::Label::new(RichText::new(m).color(ui.visuals().error_fg_color)).wrap());
                        }
                    }
                    Read::Ok(r) => {
                        let res = &r.res;
                        egui::Grid::new("civil_tv_facts").num_columns(2).show(ui, |ui| {
                            ui.label(t("tv.r.angular", &[]));
                            ui.label(match res.misclosure {
                                None => t(if form.mode == "angles" { "tv.noclose" } else { "tv.nobearing" }, &[]),
                                Some(am) => format!(
                                    "{}{} {}",
                                    if am < 0.0 { "−" } else { "+" },
                                    dms(am.abs()),
                                    tr!(
                                        "tv.per",
                                        a = format!("{}{}", if res.per_angle < 0.0 { "−" } else { "+" }, dms(res.per_angle.abs()))
                                    )
                                ),
                            });
                            ui.end_row();
                            ui.label(tr!("tv.r.linear", u = u));
                            ui.label(format!(
                                "{} {u}{}",
                                fixed(res.dist, 3),
                                if res.dist > 1e-9 { format!(", {}", bearing(res.dir)) } else { String::new() }
                            ));
                            ui.end_row();
                            ui.label(tr!("tv.r.length", u = u));
                            ui.label(format!("{} {u}", fixed(res.length, 2)));
                            ui.end_row();
                            ui.label(t("tv.r.precision", &[]));
                            ui.label(if res.precision.is_finite() && res.precision < 1e9 {
                                format!("1 : {}", grouped(res.precision, 0))
                            } else {
                                t("tv.r.exact", &[])
                            });
                            ui.end_row();
                        });
                        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                            egui::Grid::new("civil_tv_table").striped(true).num_columns(4).show(ui, |ui| {
                                for h in [t("tv.r.station", &[]), t("tv.r.an", &[]), t("tv.r.ae", &[]), tr!("tv.r.corr", u = u)] {
                                    ui.label(RichText::new(h).strong());
                                }
                                ui.end_row();
                                for (i, st) in res.stations.iter().take(60).enumerate() {
                                    ui.label(if st.name.is_empty() { (i + 1).to_string() } else { st.name.clone() });
                                    ui.label(fixed(st.ay, 3));
                                    ui.label(fixed(st.ax, 3));
                                    ui.label(fixed(st.cx.hypot(st.cy), 3));
                                    ui.end_row();
                                }
                            });
                        });
                        if res.stations.len() > 60 {
                            ui.label(RichText::new(tr!("tv.more", n = res.stations.len() - 60)).small());
                        }
                        ui.add(egui::Label::new(RichText::new(t("tv.compass", &[])).small().color(tk.text_dim)).wrap());
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t("pc.dlg.cancel", &[])).clicked() {
                        cancel = true;
                    }
                    let ok = matches!(read, Read::Ok(_));
                    let lp = matches!(&read, Read::Ok(r) if r.loop_);
                    if ui.add_enabled(ok, egui::Button::new(t("tv.csv", &[]))).clicked() {
                        add = Some("csv");
                    }
                    if ui.add_enabled(lp, egui::Button::new(t("tv.add.parcel", &[]))).clicked() {
                        add = Some("parcel");
                    }
                    if ui.add_enabled(ok, egui::Button::new(t("tv.add.points", &[]))).clicked() {
                        add = Some("points");
                    }
                });
            });
        if !open || cancel {
            cv.dialogs.traverse.open = false;
        }
        if let Some(a) = add {
            match traverse_command(cv, &serde_json::json!({ "add": a }).to_string()) {
                Ok(Some((n, b))) => acts.push(DlgAct::Save(n, b)),
                Ok(None) => {}
                Err(e) => cv.message = e,
            }
        }
    }
    if cv.dialogs.calls.open {
        let mut open = true;
        let mut go = false;
        let mut cancel = false;
        egui::Window::new(t("pc.dlg.title", &[]))
            .open(&mut open)
            .collapsible(false)
            .resizable([true, false])
            .default_width(560.0)
            .default_pos([300.0, 80.0])
            .show(ctx, |ui| {
                let next = format!("{} {}", t("pc.default", &[]), arr(cv.project.state.get("parcels")).len() + 1);
                let f = &mut cv.dialogs.calls;
                ui.horizontal(|ui| {
                    let l = ui.label(t("pc.name", &[]));
                    ui.add(egui::TextEdit::singleline(&mut f.name).hint_text(next).desired_width(200.0)).labelled_by(l.id);
                });
                ui.horizontal(|ui| field(ui, &t("pc.dlg.pt", &[]), &mut f.start, 90.0));
                ui.label(RichText::new(t("pc.dlg.or", &[])).small());
                ui.horizontal(|ui| {
                    field(ui, &t("pc.dlg.n", &[]), &mut f.north, 120.0);
                    field(ui, &t("pc.dlg.e", &[]), &mut f.east, 120.0);
                });
                let l = ui.label(t("pc.dlg.calls", &[]));
                ui.add(egui::TextEdit::multiline(&mut f.calls).desired_rows(9).desired_width(f32::INFINITY).code_editor())
                    .labelled_by(l.id);
                let form = f.clone();
                let read = read_calls(&cv.project.points, &form);
                match &read {
                    Read::Help(k) => {
                        ui.add(egui::Label::new(RichText::new(t(k, &[])).small().color(tk.text_dim)).wrap());
                    }
                    Read::Errors(e) => {
                        for m in e {
                            ui.add(egui::Label::new(RichText::new(m).color(ui.visuals().error_fg_color)).wrap());
                        }
                    }
                    Read::Ok(r) => {
                        egui::Grid::new("civil_pc_facts").num_columns(2).show(ui, |ui| {
                            ui.label(t("pc.dlg.courses", &[]));
                            ui.label(r.courses.to_string());
                            ui.end_row();
                            ui.label(t("pc.dlg.length", &[]));
                            ui.label(format!("{} {u}", fixed(r.length, 2)));
                            ui.end_row();
                            ui.label(t("pc.dlg.mis", &[]));
                            ui.label(if r.closes {
                                t("pc.dlg.closes", &[])
                            } else {
                                format!("{} {u}, {}", fixed(r.mis.0, 3), bearing(r.mis.1))
                            });
                            ui.end_row();
                            ui.label(t("pc.dlg.prec", &[]));
                            ui.label(if r.closes { "—".to_string() } else { format!("1 : {}", grouped(r.precision, 0)) });
                            ui.end_row();
                            ui.label(t("pc.area", &[]));
                            ui.label(area_words(parcel_area(&r.pts), &units));
                            ui.end_row();
                        });
                        if !r.closes {
                            ui.add(egui::Label::new(RichText::new(t("pc.dlg.gap", &[])).small().color(tk.text_dim)).wrap());
                        }
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t("pc.dlg.cancel", &[])).clicked() {
                        cancel = true;
                    }
                    if ui.add_enabled(matches!(read, Read::Ok(_)), egui::Button::new(t("pc.dlg.add", &[]))).clicked() {
                        go = true;
                    }
                });
            });
        if !open || cancel {
            cv.dialogs.calls.open = false;
        }
        if go && let Err(e) = calls_command(cv, "{\"add\":true}") {
            cv.message = e;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_traverse_closes_and_adds_points_and_a_parcel() {
        let mut cv = CivilView::sample();
        let p0 = cv.project.points.first().cloned().unwrap();
        let spec = serde_json::json!({ "start": p0.n, "mode": "bearings", "legs": "N 0 E 100 A\nN 90 E 100 B\nS 0 E 100 C\nS 90 W 100", "add": "points" });
        traverse_command(&mut cv, &spec.to_string()).unwrap();
        assert_eq!(cv.project.points.len(), 279, "a loop's last station is its start");
        assert!(cv.message.contains("A to"), "{}", cv.message);
        let spec = serde_json::json!({ "add": "parcel" });
        traverse_command(&mut cv, &spec.to_string()).unwrap();
        let pc = &cv.project.state["parcels"][0];
        assert_eq!(pc["name"], "Traverse 1");
        assert_eq!(arr(pc.get("pts")).len(), 4);
        assert_eq!(cv.project.undo_stack.len(), 2);
    }

    #[test]
    fn calls_read_as_typed_and_refuse_what_cannot_be_read() {
        let mut cv = CivilView::sample();
        assert!(matches!(read_calls(&cv.project.points, &CallsForm::default()), Read::Help(_)));
        let bad = CallsForm { north: "x".into(), east: "1".into(), calls: "N 10 E 50".into(), ..CallsForm::default() };
        assert!(matches!(read_calls(&cv.project.points, &bad), Read::Errors(_)));
        let spec = serde_json::json!({ "north": "2080000", "east": "6150000", "calls": "N 0°00'00\" E 200.00\nN 90°00'00\" E 100.00\nS 0°00'00\" W 200.00\nS 90°00'00\" W 100.00", "add": true });
        calls_command(&mut cv, &spec.to_string()).unwrap();
        assert_eq!(cv.project.state["parcels"][0]["name"], "Parcel 1");
        assert!(cv.message.contains("they close"), "{}", cv.message);
        assert!(calls_command(&mut cv, "{not json").is_err());
        assert!(traverse_command(&mut cv, "{\"mode\":\"sideways\"}").is_err());
    }
}
