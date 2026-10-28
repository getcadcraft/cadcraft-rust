//! Storm drainage on the pipe network (drainage-ui.js, 0.4 V19 + V20): the Drainage block under the network (the IDF
//! curve, Manning's n, the junction loss, the shortest tc, the tailwater; the catchments table; the results table with
//! the problems; Size pipes), the drainage rows of a selected structure and pipe, a selected catchment's properties, the
//! Catchment tool's finish, picking inside a catchment, Delete, and the plan overlay (catchments dashed and filled, the
//! arrow to their outlet, a ring on a structure whose HGL is over its rim). The numbers are cc-civil's hydro.rs
//! (hydro.js). Every change is one undo step with 0.5's label; `civil.dr.info` says what the panel shows.

use std::sync::Arc;

use cc_civil::i18n::t;
use cc_civil::jo;
use cc_civil::js::{J, arr, numf, slice16_range, str_to_number, string_of, trim};
use cc_civil::tr;
use cc_civil::units::fixed;
use egui::{Pos2, RichText, Stroke};
use serde_json::{Map, Value};

use crate::civilpanel::{Act, head, link, note, prop_fact, prop_row, report_pub};
use crate::civiltools::DraftPt;
use crate::civilview::{CivilView, Sel, t0};
use crate::theme::Tokens;

/// The flows and grade lines of the current network (drainage-ui.js `results()`).
pub struct DrResults {
    pub flows: cc_civil::hydro::Flows,
    pub grade: cc_civil::hydro::Grades,
    pub problems: Vec<cc_civil::pipes::Problem>,
}

/// `results()`'s memo: the project's version and units it was worked out for.
#[derive(Default)]
pub struct DrainState {
    memo: Option<(u64, String, Option<Arc<DrResults>>)>,
}

fn q_u(units: &str) -> &'static str {
    if units == "m" { "m³/s" } else { "cfs" }
}
fn a_u(units: &str) -> &'static str {
    if units == "m" { "ha" } else { "ac" }
}
fn i_u(units: &str) -> &'static str {
    if units == "m" { "mm/h" } else { "in/h" }
}
fn l_u(units: &str) -> &'static str {
    if units == "m" { "m" } else { "ft" }
}

/// hydro.js / drainage-ui.js `inPoly`.
pub fn in_poly(x: f64, y: f64, pts: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let n = pts.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (Some(a), Some(b)) = (pts.get(i), pts.get(j)) else { break };
        if (a.1 > y) != (b.1 > y) && x < ((b.0 - a.0) * (y - a.1)) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn xy_list(v: Option<&Value>) -> Vec<(f64, f64)> {
    arr(v).iter().map(|p| (numf(p.get("x")), numf(p.get("y")))).collect()
}

fn find<'a>(list: Option<&'a Value>, id: &str) -> Option<&'a Value> {
    arr(list).iter().find(|q| q.get("id").and_then(Value::as_str) == Some(id))
}

fn find_mut<'a>(list: Option<&'a mut Value>, id: &str) -> Option<&'a mut Map<String, Value>> {
    list?.as_array_mut()?.iter_mut().find(|q| q.get("id").and_then(Value::as_str) == Some(id))?.as_object_mut()
}

impl CivilView {
    fn network_json(&self) -> Option<&Value> {
        self.project.state.get("network").filter(|n| n.is_object())
    }

    fn has_catch(&self) -> bool {
        self.network_json().is_some_and(|n| !arr(n.get("catchments")).is_empty())
    }

    /// drainage-ui.js `results()`: null without a network or pipes; cached on the project version and units.
    pub fn dr_results(&mut self) -> Option<Arc<DrResults>> {
        let units = self.project.units().to_string();
        let v = self.project.version;
        if let Some((mv, mu, r)) = &self.drain.memo
            && *mv == v
            && *mu == units
        {
            return r.clone();
        }
        let r = (|| {
            let n = self.network_json()?.clone();
            if arr(n.get("pipes")).is_empty() {
                return None;
            }
            let net = cc_civil::pipes::read_network(&n);
            let flows = cc_civil::hydro::rational_flows(&net, &units);
            let g = self.project.graded();
            let eg = self.project.eg();
            let surface = g.surface.as_deref().or(eg.surface.as_ref());
            let grade = cc_civil::hydro::hydraulic_grade(&net, &flows, &units, surface);
            let mut problems = flows.problems.clone();
            problems.extend(grade.problems.iter().cloned());
            Some(Arc::new(DrResults { flows, grade, problems }))
        })();
        self.drain.memo = Some((v, units, r.clone()));
        r
    }

    fn hydro(&self) -> cc_civil::hydro::Hydro {
        cc_civil::hydro::clean_hydro(self.network_json().and_then(|n| n.get("hydro")), self.project.units())
    }

    /// A pipe's diameter in project units (drainage-ui.js `dia`).
    fn dr_dia(&self, p: &Value) -> f64 {
        numf(p.get("dia")) / cc_civil::pipes::per_unit(self.project.units())
    }

    /// drainage-ui.js `capOf(p, f)`: the full-flow capacity at the pipe's slope.
    fn cap_of(&self, p: &Value, f: &cc_civil::hydro::PFlow) -> f64 {
        let h = self.hydro();
        let d = self.dr_dia(p);
        let a = (std::f64::consts::PI * d * d) / 4.0;
        if f.s > 0.0 {
            ((if self.project.units() == "m" { 1.0 } else { 1.486 }) / h.n) * a * cc_civil::js::pow(d / 4.0, 2.0 / 3.0) * f.s.sqrt()
        } else {
            0.0
        }
    }

    /// drainage-ui.js `change(label, fn)`: `network.hydro` made from the defaults first when missing.
    fn hydro_change(&mut self, label: &str, f: &mut dyn FnMut(&mut Map<String, Value>)) {
        let units = self.project.units().to_string();
        self.project.change(label, &mut |st: &mut Value| {
            let Some(n) = st.get_mut("network").and_then(Value::as_object_mut) else { return };
            if !n.get("hydro").is_some_and(|v| cc_civil::js::truthy(Some(v))) {
                n.insert("hydro".into(), cc_civil::hydro::clean_hydro(None, &units).j());
            }
            if let Some(h) = n.get_mut("hydro").and_then(Value::as_object_mut) {
                f(h);
            }
        });
    }

    /// `civil.prop=dr.<field>=<value>`: the Drainage block's fields (a, b, c, minTc, n, kj, tw). Refused values change
    /// nothing (an error).
    pub fn dr_hydro_prop(&mut self, field: &str, v: &str) -> Result<(), String> {
        if self.network_json().is_none() {
            return Err("civil.prop: draw a pipe network first".into());
        }
        let bad = || format!("civil.prop: {field} = {v} is refused");
        let x = str_to_number(v);
        if field != "dr.tw" && (v.is_empty() || !x.is_finite()) {
            return Err(bad());
        }
        match field {
            "dr.a" | "dr.b" | "dr.c" => {
                let k = &field[3..];
                if x.is_nan() || x < 0.0 || (x == 0.0 && k != "b") {
                    return Err(bad());
                }
                let k = k.to_string();
                self.hydro_change(&t("dr.undo.idf", &[]), &mut |h| {
                    if let Some(i) = h.get_mut("idf").and_then(Value::as_object_mut) {
                        i.insert(k.clone(), cc_civil::js::jnum(x));
                    }
                });
            }
            "dr.n" => {
                if !(x > 0.0 && x < 1.0) {
                    return Err(bad());
                }
                self.hydro_change(&t("dr.undo.n", &[]), &mut |h| {
                    h.insert("n".into(), cc_civil::js::jnum(x));
                });
            }
            "dr.kj" => {
                if !((0.0..=10.0).contains(&x)) {
                    return Err(bad());
                }
                self.hydro_change(&t("dr.undo.kj", &[]), &mut |h| {
                    h.insert("kj".into(), cc_civil::js::jnum(x));
                });
            }
            "dr.minTc" => {
                if x.is_nan() || x <= 0.0 {
                    return Err(bad());
                }
                self.hydro_change(&t("dr.undo.tc", &[]), &mut |h| {
                    h.insert("minTc".into(), cc_civil::js::jnum(x));
                });
            }
            "dr.tw" => {
                let s = trim(v);
                let val = if s.is_empty() {
                    Value::Null
                } else {
                    let x = str_to_number(s);
                    if !x.is_finite() {
                        return Err(bad());
                    }
                    cc_civil::js::jnum(x)
                };
                self.hydro_change(&t("dr.undo.tw", &[]), &mut |h| {
                    h.insert("tailwater".into(), val.clone());
                });
            }
            _ => return Err(format!("civil.prop: drainage has no field {field}")),
        }
        Ok(())
    }

    /// `civil.prop=<field>=<value>` with a catchment selected: name, C, tc, to (a structure id, empty = none).
    pub fn dr_catchment_prop(&mut self, id: &str, field: &str, v: &str) -> Result<(), String> {
        let c = self.network_json().and_then(|n| find(n.get("catchments"), id)).cloned().ok_or("civil.prop: no such catchment")?;
        let bad = || format!("civil.prop: {field} = {v} is refused");
        let edit = |cv: &mut CivilView, label: &str, key: &str, val: Value| {
            let id = id.to_string();
            cv.project.change(label, &mut |st: &mut Value| {
                if let Some(m) = st.get_mut("network").and_then(|n| find_mut(n.get_mut("catchments"), &id)) {
                    m.insert(key.to_string(), val.clone());
                }
            });
        };
        match field {
            "name" => {
                let s = slice16_range(trim(v), 0, Some(120));
                let name = if s.is_empty() { t("dr.c.default", &[]) } else { s };
                edit(self, &t("dr.undo.rename", &[]), "name", Value::String(name));
            }
            "C" => {
                let x = str_to_number(v);
                if v.is_empty() || !((0.0..=1.0).contains(&x)) {
                    return Err(bad());
                }
                edit(self, &t("dr.undo.c", &[]), "C", cc_civil::js::jnum(x));
            }
            "tc" => {
                let x = str_to_number(v);
                if v.is_empty() || !(x > 0.0 && x < 1000.0) {
                    return Err(bad());
                }
                edit(self, &t("dr.undo.ctc", &[]), "tc", cc_civil::js::jnum(x));
            }
            "to" => {
                let s = v.trim();
                let val = if s.is_empty() {
                    Value::Null
                } else {
                    if self.network_json().and_then(|n| find(n.get("structures"), s)).is_none() {
                        return Err(bad());
                    }
                    Value::String(s.to_string())
                };
                edit(self, &t("dr.undo.to", &[]), "to", val);
            }
            _ => return Err(format!("civil.prop: a catchment has no field {field}")),
        }
        let _ = c;
        Ok(())
    }

    /// drainage-ui.js `finish(pts)`: the catchment tool's outline becomes a catchment (one undo step) flowing to the
    /// structure inside it (the first listed), else the nearest to its middle. → false: too few corners, keep drawing.
    pub fn dr_finish(&mut self, pts: &[DraftPt]) -> bool {
        if pts.len() < 3 {
            self.message = t("dr.c.few", &[]);
            return false;
        }
        let units = self.project.units().to_string();
        let structs: Vec<Value> = self.network_json().map(|n| arr(n.get("structures")).to_vec()).unwrap_or_default();
        if structs.is_empty() {
            self.message = t("dr.c.nonet", &[]);
            return true;
        }
        let n = pts.len() as f64;
        let cx = pts.iter().fold(0.0, |m, p| m + p.x) / n;
        let cy = pts.iter().fold(0.0, |m, p| m + p.y) / n;
        let ring: Vec<(f64, f64)> = pts.iter().map(|p| (p.x, p.y)).collect();
        let sxy = |s: &Value| (numf(s.get("x")), numf(s.get("y")));
        let inside = structs.iter().find(|s| {
            let (x, y) = sxy(s);
            in_poly(x, y, &ring)
        });
        let to_s = match inside {
            Some(s) => s.clone(),
            None => {
                let mut best = structs.first().cloned().unwrap_or(Value::Null);
                for b in structs.iter().skip(1) {
                    let (bx, by) = sxy(b);
                    let (ax, ay) = sxy(&best);
                    if (bx - cx).hypot(by - cy) < (ax - cx).hypot(ay - cy) {
                        best = b.clone();
                    }
                }
                best
            }
        };
        let to = string_of(to_s.get("id"));
        let id = self.project.next_id("k");
        let k = self.network_json().map_or(0, |n| arr(n.get("catchments")).len()) + 1;
        let name = format!("{} {k}", t("dr.c.default", &[]));
        let min_tc = cc_civil::hydro::hydro_defaults(&units).min_tc;
        let c = jo! { "id" => id.as_str(), "name" => name.as_str(), "pts" => Value::Array(pts.iter().map(|p| jo! { "x" => p.x, "y" => p.y }).collect()),
        "C" => 0.5, "tc" => min_tc, "to" => to.as_str() };
        self.project.change(&t("dr.undo.draw", &[]), &mut |st: &mut Value| {
            let Some(n) = st.get_mut("network").and_then(Value::as_object_mut) else { return };
            if !n.get("catchments").is_some_and(|v| cc_civil::js::truthy(Some(v))) {
                n.insert("catchments".into(), Value::Array(Vec::new()));
            }
            if let Some(l) = n.get_mut("catchments").and_then(Value::as_array_mut) {
                l.push(c.clone());
            }
        });
        self.sel = Some(Sel::Catchment(id));
        self.points.picked.clear();
        self.profile.sel_pvi = None;
        let p2: Vec<cc_civil::geo::P2> = pts.iter().map(|p| cc_civil::geo::P2 { x: p.x, y: p.y }).collect();
        let area = format!("{} {}", fixed(cc_civil::hydro::catchment_area(&p2, &units), 2), a_u(&units));
        self.message = tr!("dr.c.drawn", name = name.as_str(), a = area, to = string_of(to_s.get("name")));
        true
    }

    /// drainage-ui.js `pick(w)`: the last catchment the point is inside.
    pub fn dr_pick(&self, x: f64, y: f64) -> Option<Sel> {
        let n = self.network_json()?;
        arr(n.get("catchments"))
            .iter()
            .rev()
            .find(|c| in_poly(x, y, &xy_list(c.get("pts"))))
            .map(|c| Sel::Catchment(string_of(c.get("id"))))
    }

    /// drainage-ui.js `remove(id)`: one undo step. → false when there is no such catchment.
    pub fn dr_remove(&mut self, id: &str) -> bool {
        let Some(name) = self.network_json().and_then(|n| find(n.get("catchments"), id)).map(|c| string_of(c.get("name"))) else {
            return false;
        };
        let id = id.to_string();
        self.project.change(&t("dr.undo.delete", &[]), &mut |st: &mut Value| {
            if let Some(l) = st.get_mut("network").and_then(|n| n.get_mut("catchments")).and_then(Value::as_array_mut) {
                l.retain(|x| x.get("id").and_then(Value::as_str) != Some(id.as_str()));
            }
        });
        self.message = tr!("dr.c.deleted", name = name);
        true
    }

    /// drainage-ui.js `sizeAll()`: every pipe the smallest standard size that carries its flow (one undo step).
    pub fn dr_size(&mut self) {
        let Some(r) = self.dr_results() else { return };
        let units = self.project.units().to_string();
        let Some(n) = self.network_json().cloned() else { return };
        let net = cc_civil::pipes::read_network(&n);
        let sz = cc_civil::hydro::size_pipes(&net, &r.flows, &units, None);
        let changed: Vec<(String, f64)> = sz
            .iter()
            .filter(|(id, s)| net.pipes.iter().find(|p| p.id == *id).is_some_and(|p| s.dia != p.dia))
            .map(|(id, s)| (id.clone(), s.dia))
            .collect();
        if changed.is_empty() {
            self.message = t("dr.size.same", &[]);
            return;
        }
        self.project.change(&t("dr.undo.size", &[]), &mut |st: &mut Value| {
            for (id, d) in &changed {
                if let Some(m) = st.get_mut("network").and_then(|n| find_mut(n.get_mut("pipes"), id)) {
                    m.insert("dia".into(), cc_civil::js::jnum(*d));
                }
            }
        });
        let bad = sz.iter().filter(|(_, s)| !s.ok).count();
        let mut m = tr!("dr.size.done", n = changed.len());
        if bad > 0 {
            m.push(' ');
            m.push_str(&tr!("dr.size.bad", n = bad));
        }
        self.message = m;
    }

    /// The catchments table's rows: name, area (2 dp), C, tc, outlet name.
    fn catchment_rows(&self) -> Vec<(String, [String; 5])> {
        let units = self.project.units();
        let Some(n) = self.network_json() else { return Vec::new() };
        arr(n.get("catchments"))
            .iter()
            .map(|c| {
                let p2 = cc_civil::geo::read_p2s(c.get("pts"));
                let to = c.get("to").and_then(Value::as_str).and_then(|s| find(n.get("structures"), s)).map(|s| string_of(s.get("name")));
                (
                    string_of(c.get("id")),
                    [
                        string_of(c.get("name")),
                        fixed(cc_civil::hydro::catchment_area(&p2, units), 2),
                        fixed(numf(c.get("C")), 2),
                        fixed(numf(c.get("tc")), 1),
                        to.unwrap_or_else(|| "—".into()),
                    ],
                )
            })
            .collect()
    }

    /// The results table's rows: pipe, Q, capacity, V, HGL up – down, state.
    fn result_rows(&mut self, r: &DrResults) -> Vec<(String, bool, [String; 6])> {
        let pipes: Vec<Value> = self.network_json().map(|n| arr(n.get("pipes")).to_vec()).unwrap_or_default();
        pipes
            .iter()
            .map(|p| {
                let id = string_of(p.get("id"));
                let f = r.flows.pipe(&id);
                let g = r.grade.pipes.iter().find(|(k, _)| *k == id).map(|(_, g)| *g);
                let state = match g {
                    None => "—".to_string(),
                    Some(g) if g.full => t("dr.r.full", &[]),
                    Some(g) => tr!("dr.r.part", pct = fixed((g.yn.unwrap_or(f64::NAN) / self.dr_dia(p)) * 100.0, 0)),
                };
                (
                    id.clone(),
                    g.is_some_and(|g| g.full),
                    [
                        string_of(p.get("name")),
                        f.map_or("—".into(), |f| fixed(f.q, 2)),
                        match (f, g) {
                            (Some(f), Some(_)) => fixed(self.cap_of(p, f), 2),
                            _ => "—".into(),
                        },
                        g.map_or("—".into(), |g| fixed(g.v, 2)),
                        g.map_or("—".into(), |g| format!("{} – {}", fixed(g.hgl_up, 2), fixed(g.hgl_dn, 2))),
                        state,
                    ],
                )
            })
            .collect()
    }

    /// drainage-ui.js `structureRows(x)`: (label, value) pairs.
    pub fn dr_structure_rows(&mut self, id: &str) -> Vec<(String, String)> {
        if !self.has_catch() {
            return Vec::new();
        }
        let Some(r) = self.dr_results() else { return Vec::new() };
        let units = self.project.units().to_string();
        let mut rows = Vec::new();
        if let Some((_, f)) = r.flows.structs.iter().find(|(k, _)| k == id) {
            rows.push((t("dr.s.ca", &[]), format!("{} {}", fixed(f.ca, 3), a_u(&units))));
            rows.push((t("dr.s.tc", &[]), format!("{} min", fixed(f.tc, 1))));
        }
        if let Some((_, g)) = r.grade.structs.iter().find(|(k, _)| k == id) {
            rows.push((t("dr.s.hgl", &[]), fixed(g.hgl, 2)));
            rows.push((t("dr.s.egl", &[]), fixed(g.egl, 2)));
            rows.push((
                t("dr.s.free", &[]),
                match g.rim.filter(|r| r.is_finite()) {
                    Some(rim) => format!("{} {}", fixed(rim - g.hgl, 2), l_u(&units)),
                    None => "—".into(),
                },
            ));
        }
        rows
    }

    /// drainage-ui.js `pipeRows(p)`: (label, value) pairs and the pipe's problems.
    pub fn dr_pipe_rows(&mut self, id: &str) -> (Vec<(String, String)>, Vec<String>) {
        if !self.has_catch() {
            return (Vec::new(), Vec::new());
        }
        let Some(r) = self.dr_results() else { return (Vec::new(), Vec::new()) };
        let Some(f) = r.flows.pipe(id).copied() else { return (Vec::new(), Vec::new()) };
        let units = self.project.units().to_string();
        let (qu, iu, lu) = (q_u(&units), i_u(&units), l_u(&units));
        let p = self.network_json().and_then(|n| find(n.get("pipes"), id)).cloned().unwrap_or(Value::Null);
        let mut rows = vec![
            (
                t("dr.p.q", &[]),
                format!("{} {qu} (C·A {}, i {} {iu}, tc {} min)", fixed(f.q, 3), fixed(f.ca, 3), fixed(f.i, 2), fixed(f.tc, 1)),
            ),
            (t("dr.p.cap", &[]), format!("{} {qu}", fixed(self.cap_of(&p, &f), 3))),
        ];
        if let Some((_, g)) = r.grade.pipes.iter().find(|(k, _)| k == id) {
            rows.push((
                t("dr.p.depth", &[]),
                if g.full { t("dr.r.full", &[]) } else { format!("{} {lu}", fixed(g.yn.unwrap_or(f64::NAN), 2)) },
            ));
            rows.push((t("dr.p.v", &[]), format!("{} {lu}/s", fixed(g.v, 2))));
            rows.push((t("dr.p.hgl", &[]), format!("{} – {}", fixed(g.hgl_up, 2), fixed(g.hgl_dn, 2))));
            rows.push((t("dr.p.egl", &[]), format!("{} – {}", fixed(g.egl_up, 2), fixed(g.egl_dn, 2))));
        }
        let probs = r.problems.iter().filter(|q| q.id.as_deref() == Some(id)).map(|q| q.msg.clone()).collect();
        (rows, probs)
    }

    /// drainage-ui.js `props(c)`'s read-only rows: area (3 dp) and corners.
    fn catchment_facts(&self, c: &Value) -> (String, String) {
        let p2 = cc_civil::geo::read_p2s(c.get("pts"));
        (fixed(cc_civil::hydro::catchment_area(&p2, self.project.units()), 3), arr(c.get("pts")).len().to_string())
    }

    /// `civil.dr.info`: what the panel shows for the selection — a structure's or pipe's drainage rows, a catchment's
    /// fields — else the Drainage block's catchments and results tables, problems or "ok" (" | " between lines).
    pub fn dr_info(&mut self) -> Result<String, String> {
        if self.network_json().is_none() {
            return Err("civil.dr.info: no pipe network".into());
        }
        let units = self.project.units().to_string();
        let join = |rows: Vec<(String, String)>| rows.into_iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>();
        let out: Vec<String> = match self.sel.clone() {
            Some(Sel::Structure(id)) => join(self.dr_structure_rows(&id)),
            Some(Sel::Pipe(id)) => {
                let (rows, probs) = self.dr_pipe_rows(&id);
                let mut o = join(rows);
                o.extend(probs);
                o
            }
            Some(Sel::Catchment(id)) => {
                let c =
                    self.network_json().and_then(|n| find(n.get("catchments"), &id)).cloned().ok_or("civil.dr.info: no such catchment")?;
                let (area, corners) = self.catchment_facts(&c);
                let to = c.get("to").and_then(Value::as_str).unwrap_or("");
                vec![
                    format!("{}: {}", t("dr.c.name", &[]), string_of(c.get("name"))),
                    format!("{}: {}", t("dr.c.coef", &[]), fixed(numf(c.get("C")), 2)),
                    format!("{}: {}", t("dr.c.tc", &[]), fixed(numf(c.get("tc")), 1)),
                    format!("{}: {}", t("dr.c.to", &[]), to),
                    format!("{}: {area}", tr!("dr.c.area", u = a_u(&units))),
                    format!("{}: {corners}", t("dr.c.corners", &[])),
                ]
            }
            _ => {
                let h = self.hydro();
                let mut o = vec![format!(
                    "idf {} {} {} · tc {} · n {} · kj {} · tw {}",
                    fixed(h.idf.a, 2),
                    fixed(h.idf.b, 2),
                    fixed(h.idf.c, 3),
                    fixed(h.min_tc, 1),
                    fixed(h.n, 3),
                    fixed(h.kj, 2),
                    h.tailwater.map_or(String::new(), |w| fixed(w, 2))
                )];
                let cs = self.catchment_rows();
                if cs.is_empty() {
                    o.push(t("dr.none", &[]));
                } else {
                    o.extend(cs.into_iter().map(|(_, r)| r.join("  ")));
                }
                if let Some(r) = self.dr_results().filter(|_| self.has_catch()) {
                    o.extend(
                        self.result_rows(&r)
                            .into_iter()
                            .map(|(_, full, row)| format!("{}{}", row.join("  "), if full { " !" } else { "" })),
                    );
                    if r.problems.is_empty() {
                        o.push(t("dr.ok", &[]));
                    } else {
                        o.extend(r.problems.iter().take(40).map(|q| q.msg.clone()));
                    }
                }
                o
            }
        };
        Ok(out.join(" | "))
    }
}

/// The Drainage block under the network (drainage-ui.js `block()`).
pub fn block(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, acts: &mut Vec<Act>) {
    head(ui, tk, &t0("dr.head"));
    if cv.network_json().is_none() {
        return;
    }
    let units = cv.project.units().to_string();
    let h = cv.hydro();
    note(ui, tk, &t0("dr.idf.lead"));
    egui::Grid::new("cv_dr_settings").num_columns(3).spacing([8.0, 3.0]).show(ui, |ui| {
        prop_row(cv, ui, tk, "dr:a", &t0("dr.idf.a"), &fixed(h.idf.a, 2), "", "dr.a", acts);
        prop_row(cv, ui, tk, "dr:b", &t0("dr.idf.b"), &fixed(h.idf.b, 2), "min", "dr.b", acts);
        prop_row(cv, ui, tk, "dr:c", &t0("dr.idf.c"), &fixed(h.idf.c, 3), "", "dr.c", acts);
        prop_row(cv, ui, tk, "dr:mintc", &t0("dr.mintc"), &fixed(h.min_tc, 1), "min", "dr.minTc", acts);
        prop_row(cv, ui, tk, "dr:n", &t0("dr.n"), &fixed(h.n, 3), "", "dr.n", acts);
        prop_row(cv, ui, tk, "dr:kj", &t0("dr.kj"), &fixed(h.kj, 2), "", "dr.kj", acts);
        prop_row(cv, ui, tk, "dr:tw", &t0("dr.tw"), &h.tailwater.map_or(String::new(), |w| fixed(w, 2)), l_u(&units), "dr.tw", acts);
    });
    note(ui, tk, &t0("dr.idf.formula"));
    note(ui, tk, &tr!("dr.note", i = i_u(&units), q = q_u(&units), a = a_u(&units)));
    if ui.button(t0("dr.draw")).clicked() {
        acts.push(Act::Run("civil.tool", "catchment!".into()));
    }
    let cs = cv.catchment_rows();
    if cs.is_empty() {
        note(ui, tk, &t0("dr.none"));
    } else {
        egui::Grid::new("cv_dr_catchments").num_columns(5).spacing([10.0, 2.0]).striped(true).show(ui, |ui| {
            for h in [t0("dr.c.name"), tr!("dr.c.area", u = a_u(&units)), "C".into(), t0("dr.c.tc"), t0("dr.c.to")] {
                ui.label(RichText::new(h).small().strong().color(tk.text_dim));
            }
            ui.end_row();
            for (id, r) in &cs {
                for (k, c) in r.iter().enumerate() {
                    if k == 0 {
                        if link(ui, tk, c, false) {
                            acts.push(Act::Select(Some(Sel::Catchment(id.clone()))));
                        }
                    } else {
                        ui.label(RichText::new(c).small().color(tk.text));
                    }
                }
                ui.end_row();
            }
        });
    }
    let Some(r) = cv.dr_results().filter(|_| cv.has_catch()) else { return };
    head(ui, tk, &t0("dr.results"));
    let rows = cv.result_rows(&r);
    egui::Grid::new("cv_dr_results").num_columns(6).spacing([10.0, 2.0]).striped(true).show(ui, |ui| {
        for h in [
            t0("dr.r.pipe"),
            tr!("dr.r.q", u = q_u(&units)),
            tr!("dr.r.cap", u = q_u(&units)),
            tr!("dr.r.v", u = l_u(&units)),
            t0("dr.r.hgl"),
            t0("dr.r.state"),
        ] {
            ui.label(RichText::new(h).small().strong().color(tk.text_dim));
        }
        ui.end_row();
        for (id, full, row) in &rows {
            for (k, c) in row.iter().enumerate() {
                if k == 0 {
                    if link(ui, tk, c, false) {
                        acts.push(Act::Select(Some(Sel::Pipe(id.clone()))));
                    }
                } else {
                    let col = if *full { egui::Color32::from_rgb(0xc0, 0x96, 0x2e) } else { tk.text };
                    ui.label(RichText::new(c).small().color(col));
                }
            }
            ui.end_row();
        }
    });
    if r.problems.is_empty() {
        note(ui, tk, &t0("dr.ok"));
    } else {
        let list: Vec<String> = r.problems.iter().take(40).map(|q| q.msg.clone()).collect();
        report_pub(ui, tk, &list, true);
        for q in r.problems.iter().take(40) {
            if let Some(id) = q.id.as_ref().filter(|_| q.kind != "network") {
                let s = match q.kind {
                    "pipe" => Sel::Pipe(id.clone()),
                    "catchment" => Sel::Catchment(id.clone()),
                    _ => Sel::Structure(id.clone()),
                };
                if link(ui, tk, &format!("→ {}", q.msg), false) {
                    acts.push(Act::Select(Some(s)));
                }
            }
        }
    }
    if ui.button(t0("dr.size")).clicked() {
        acts.push(Act::Run("civil.dr.size", String::new()));
    }
    note(ui, tk, &t0("dr.size.note"));
}

/// A structure's or pipe's Drainage rows (drainage-ui.js `structureRows` / `pipeRows`).
pub fn rows_block(ui: &mut egui::Ui, tk: &Tokens, rows: &[(String, String)], probs: &[String], id: &str) {
    if rows.is_empty() {
        return;
    }
    head(ui, tk, &t0("dr.head"));
    egui::Grid::new(("cv_dr_rows", id)).num_columns(3).spacing([8.0, 3.0]).show(ui, |ui| {
        for (k, v) in rows {
            prop_fact(ui, tk, k, v);
        }
    });
    report_pub(ui, tk, probs, true);
}

/// A selected catchment's Properties (drainage-ui.js `props(c)`) and the Delete button.
pub fn props(cv: &mut CivilView, ui: &mut egui::Ui, tk: &Tokens, c: &Value, acts: &mut Vec<Act>) {
    let units = cv.project.units().to_string();
    let id = string_of(c.get("id"));
    head(ui, tk, &t0("dr.c.head"));
    let structs: Vec<Value> = cv.network_json().map(|n| arr(n.get("structures")).to_vec()).unwrap_or_default();
    let to = c.get("to").and_then(Value::as_str).unwrap_or("").to_string();
    let (area, corners) = cv.catchment_facts(c);
    egui::Grid::new(("cv_dr_c", id.as_str())).num_columns(3).spacing([8.0, 3.0]).show(ui, |ui| {
        prop_row(cv, ui, tk, &format!("dr:{id}:name"), &t0("dr.c.name"), &string_of(c.get("name")), "", "name", acts);
        prop_row(cv, ui, tk, &format!("dr:{id}:C"), &t0("dr.c.coef"), &fixed(numf(c.get("C")), 2), "", "C", acts);
        prop_row(cv, ui, tk, &format!("dr:{id}:tc"), &t0("dr.c.tc"), &fixed(numf(c.get("tc")), 1), "min", "tc", acts);
        ui.label(RichText::new(t0("dr.c.to")).color(tk.text_dim));
        let shown = structs
            .iter()
            .find(|s| s.get("id").and_then(Value::as_str) == Some(to.as_str()))
            .map_or_else(|| t0("dr.c.to.none"), |s| string_of(s.get("name")));
        egui::ComboBox::new(("cv_dr_to", id.as_str()), "").selected_text(shown).show_ui(ui, |ui| {
            if ui.selectable_label(to.is_empty(), t0("dr.c.to.none")).clicked() {
                acts.push(Act::Run("civil.prop", "to=".into()));
            }
            for s in &structs {
                let sid = string_of(s.get("id"));
                if ui.selectable_label(sid == to, string_of(s.get("name"))).clicked() {
                    acts.push(Act::Run("civil.prop", format!("to={sid}")));
                }
            }
        });
        ui.label("");
        ui.end_row();
        prop_fact(ui, tk, &tr!("dr.c.area", u = a_u(&units)), &area);
        prop_fact(ui, tk, &t0("dr.c.corners"), &corners);
    });
    note(ui, tk, &t0("dr.c.note"));
    ui.horizontal(|ui| {
        if ui.button(t0("dr.draw.more")).clicked() {
            acts.push(Act::Run("civil.tool", "catchment!".into()));
        }
        if ui.button(RichText::new(t0("fl.props.delete")).color(egui::Color32::from_rgb(0xd0, 0x4a, 0x5a))).clicked() {
            acts.push(Act::Run("civil.delete", String::new()));
        }
    });
}

/// drainage-ui.js `overlay`: catchments (dashed outline, light fill, name and C; the selected one solid and heavier), a
/// dotted line from each to its outlet, a red ring on a structure whose HGL is over its rim.
pub fn overlay(cv: &mut CivilView, painter: &egui::Painter, s: &dyn Fn(f64, f64) -> Pos2, pal: &crate::civilplan::Palette) {
    let Some(n) = cv.network_json().cloned() else { return };
    let cyan = pal.cyan;
    let font = egui::FontId::proportional(12.0);
    for c in arr(n.get("catchments")) {
        let on = matches!(&cv.sel, Some(Sel::Catchment(id)) if Some(id.as_str()) == c.get("id").and_then(Value::as_str));
        let pts: Vec<Pos2> = xy_list(c.get("pts")).iter().map(|q| s(q.0, q.1)).collect();
        if pts.len() < 3 {
            continue;
        }
        let fill = cyan.gamma_multiply(if on { 0.16 } else { 0.08 });
        crate::civildrain::fill_poly(painter, &pts, fill);
        let mut ring = pts.clone();
        if let Some(p0) = pts.first().copied() {
            ring.push(p0);
        }
        if on {
            painter.add(egui::Shape::line(ring, Stroke::new(2.5, pal.action)));
        } else {
            painter.extend(egui::Shape::dashed_line(&ring, Stroke::new(1.5, cyan), 8.0, 5.0));
        }
        let k = xy_list(c.get("pts"));
        let nn = k.len() as f64;
        let (cx, cy) = (k.iter().fold(0.0, |m, p| m + p.0) / nn, k.iter().fold(0.0, |m, p| m + p.1) / nn);
        let q = s(cx, cy);
        let top = pts.iter().fold(f32::INFINITY, |m, p| m.min(p.y));
        let lq = Pos2::new(q.x, q.y.min(top + 14.0));
        if let Some(st) = c.get("to").and_then(Value::as_str).and_then(|to| find(n.get("structures"), to)) {
            let r = s(numf(st.get("x")), numf(st.get("y")));
            painter.extend(egui::Shape::dashed_line(&[q, r], Stroke::new(1.0, cyan), 3.0, 3.0));
        }
        painter.text(
            lq,
            egui::Align2::CENTER_CENTER,
            format!("{}  C {}", string_of(c.get("name")), fixed(numf(c.get("C")), 2)),
            font.clone(),
            pal.ink,
        );
    }
    if !cv.has_catch() {
        return;
    }
    let Some(r) = cv.dr_results() else { return };
    for (id, g) in &r.grade.structs {
        let Some(rim) = g.rim.filter(|r| r.is_finite()) else { continue };
        if g.hgl <= rim {
            continue;
        }
        if let Some(st) = find(n.get("structures"), id) {
            painter.circle_stroke(s(numf(st.get("x")), numf(st.get("y"))), 15.0, Stroke::new(2.5, pal.red));
        }
    }
}

/// A filled polygon of any shape (ear-cut by the plan's tessellator, else a fan).
pub fn fill_poly(painter: &egui::Painter, pts: &[Pos2], fill: egui::Color32) {
    let flat: Vec<f64> = pts.iter().flat_map(|p| [f64::from(p.x), f64::from(p.y)]).collect();
    let tris = cc_m3d::earcut::earcut(&flat, &[]);
    let mut mesh = egui::Mesh::default();
    for p in pts {
        mesh.colored_vertex(*p, fill);
    }
    for [a, b, c] in tris.as_chunks::<3>().0 {
        if let (Ok(a), Ok(b), Ok(c)) = (u32::try_from(*a), u32::try_from(*b), u32::try_from(*c)) {
            mesh.add_triangle(a, b, c);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civiltools::Tool;

    fn site() -> CivilView {
        let mut cv = CivilView::sample();
        cv.view.width = 1000.0;
        cv.view.height = 800.0;
        cv.view.cx = 6_150_200.0;
        cv.view.cy = 2_080_150.0;
        cv.view.zoom = 1.5;
        cv
    }

    fn pipes(cv: &mut CivilView) {
        cv.tool_command(Tool::Pipe);
        for (x, y) in [(6_150_120.0, 2_080_180.0), (6_150_190.0, 2_080_150.0), (6_150_270.0, 2_080_110.0)] {
            cv.click(x, y);
        }
        cv.finish();
    }

    #[test]
    fn a_catchment_drains_to_the_structure_inside_and_flows_size_the_pipes() {
        let mut cv = site();
        cv.tool_command(Tool::Catchment);
        for (x, y) in [(6_150_100.0, 2_080_160.0), (6_150_140.0, 2_080_160.0), (6_150_140.0, 2_080_200.0)] {
            cv.click(x, y);
        }
        cv.finish();
        assert_eq!(cv.message, t("dr.c.nonet", &[]), "no network: said, and the tool goes back to Select");
        assert_eq!(cv.tools.tool, Tool::Select);
        pipes(&mut cv);
        cv.tool_command(Tool::Catchment);
        cv.click(6_150_100.0, 2_080_160.0);
        cv.click(6_150_150.0, 2_080_160.0);
        cv.finish();
        assert_eq!(cv.message, t("dr.c.few", &[]));
        assert_eq!(cv.tools.tool, Tool::Catchment, "too few corners: keep drawing");
        cv.click(6_150_150.0, 2_080_210.0);
        cv.click(6_150_100.0, 2_080_210.0);
        cv.finish();
        let c = cv.project.state["network"]["catchments"][0].clone();
        let s0 = cv.project.state["network"]["structures"][0]["id"].clone();
        assert_eq!(c["to"], s0, "the structure inside the outline");
        assert!(matches!(&cv.sel, Some(Sel::Catchment(_))));
        cv.prop_set_more("C", "2").unwrap_err();
        cv.prop_set_more("C", "0.9").unwrap();
        cv.prop_set_more("dr.n", "0").unwrap_err();
        cv.prop_set_more("dr.tw", "").unwrap();
        assert!(cv.project.state["network"]["hydro"]["tailwater"].is_null());
        let info = cv.dr_info().unwrap();
        assert!(info.contains("0.90"), "{info}");
        cv.sel = None;
        let info = cv.dr_info().unwrap();
        assert!(info.starts_with("idf "), "{info}");
        cv.dr_size();
        assert!(!cv.message.is_empty());
        // picking inside, Delete, undo
        let id = string_of(c.get("id"));
        cv.sel = Some(Sel::Catchment(id.clone()));
        cv.delete_selected();
        assert!(arr(cv.project.state["network"].get("catchments")).is_empty());
        cv.undo();
        assert_eq!(cv.dr_pick(6_150_120.0, 2_080_190.0), Some(Sel::Catchment(id)));
    }

    #[test]
    fn in_poly_is_the_even_odd_rule() {
        let sq = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        assert!(in_poly(5.0, 5.0, &sq));
        assert!(!in_poly(15.0, 5.0, &sq));
        assert!(!in_poly(5.0, 5.0, &[]));
    }
}
