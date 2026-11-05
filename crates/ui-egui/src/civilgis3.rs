//! GIS layers' "Use for the surface" (gis-vector-ui.js useDialog, gis-layers.js useChoices / useForSurface): a layer's
//! lines become breaklines (their own elevations or a number field), its largest (or the selected) polygon the
//! boundary, its points survey points — copied into the project as one undo step, the layer left as it is. Also the
//! answers of the system picker for the GIS import dialog and for a latitude / longitude point file (0.5's askCrs).
//!
//! Commands: `civil.gis.use=<layer id>[:<feature index>]` opens the dialog, `civil.gis.use.dlg=choice=<i>|field=<name>|
//! go|cancel`, `civil.gis.use.info`; `civil.gis.import=pickcrs|pickunknown=<si>` ask for a system.

use cc_civil::gis_state::{Feature, Geom, Layer};
use cc_civil::js::{jnum, slice16, string_of, truthy};
use cc_civil::points::Pt;
use cc_civil::tr;
use cc_io_civil::gis_read::Src;
use egui::RichText;
use serde_json::{Value, json};

use crate::civilcrs::PickFor;
use crate::civilview::{CivilView, t0};
use crate::theme::Tokens;

/// The "Use for the surface" dialog.
#[derive(Clone, Debug)]
pub struct UseDlg {
    pub layer: String,
    pub feature: Option<usize>,
    pub choice: usize,
    pub field: String,
}

/// What `useForSurface` did: (ok, message).
pub type UseResult = (bool, String);

fn layer_by(cv: &CivilView, id: &str) -> Option<Layer> {
    cv.gis_layers().into_iter().find(|l| l.id == id)
}

/// The elevation field guessed first (useDialog's sync()).
fn guess_field(fields: &[String]) -> String {
    let g = fields.iter().find(|f| {
        let l = f.to_ascii_lowercase();
        ["elev", "elevation", "z", "contour", "height", "cont", "elev_ft", "elev_m"].contains(&l.as_str())
    });
    g.or(fields.first()).cloned().unwrap_or_default()
}

fn strs(v: Option<&Value>) -> Vec<String> {
    cc_civil::js::arr(v).iter().map(|x| string_of(Some(x))).collect()
}

impl CivilView {
    /// `useChoices` of a layer.
    pub fn use_choices(&self, id: &str) -> Vec<Value> {
        let Some(l) = layer_by(self, id) else { return Vec::new() };
        let fs = cc_io_civil::gis_layers::features_of(&self.project.gis_features, id);
        cc_io_civil::gis_layers::use_choices(&l, fs)
    }

    /// `civil.gis.use=<layer>[:<feature>]`: the dialog, or why the layer has nothing to use.
    pub fn use_open(&mut self, spec: &str) -> Result<(), String> {
        let (id, f) = spec.split_once(':').map_or((spec, None), |(a, b)| (a, b.parse::<usize>().ok()));
        let l = layer_by(self, id).ok_or_else(|| format!("civil.gis.use: no layer {id}"))?;
        let ch = self.use_choices(id);
        let Some(first) = ch.first() else {
            self.message = tr!("vec.use.nothing", layer = l.name.as_str());
            return Ok(());
        };
        let field = guess_field(&strs(first.get("fields")));
        self.gis3.use_dlg = Some(UseDlg { layer: id.to_string(), feature: f, choice: 0, field });
        Ok(())
    }

    /// `civil.gis.use.dlg=choice=<i>|field=<name>|go|cancel`.
    pub fn use_cmd(&mut self, spec: &str) -> Result<(), String> {
        let d = self.gis3.use_dlg.clone().ok_or("civil.gis.use.dlg: the dialog is not open")?;
        let ch = self.use_choices(&d.layer);
        let (k, v) = spec.split_once('=').unwrap_or((spec, ""));
        match k {
            "cancel" | "close" => self.gis3.use_dlg = None,
            "choice" => {
                let i: usize = v.parse().ok().filter(|i| *i < ch.len()).ok_or("civil.gis.use.dlg: choice=<index>")?;
                let field = guess_field(&strs(ch.get(i).and_then(|c| c.get("fields"))));
                self.gis3.use_dlg = Some(UseDlg { choice: i, field, ..d });
            }
            "field" => {
                let fields = strs(ch.get(d.choice).and_then(|c| c.get("fields")));
                if !fields.iter().any(|f| f == v) {
                    return Err(format!("civil.gis.use.dlg: no number field {v}"));
                }
                self.gis3.use_dlg = Some(UseDlg { field: v.to_string(), ..d });
            }
            "go" => {
                let c = ch.get(d.choice).cloned().ok_or("civil.gis.use.dlg: nothing to use")?;
                let us = string_of(c.get("use"));
                let from = c.get("from").and_then(Value::as_str).unwrap_or("z").to_string();
                let field = (from == "field").then(|| d.field.clone());
                let feature = if us == "boundary" { d.feature } else { None };
                self.gis3.use_dlg = None;
                let (_, m) = self.use_for_surface(&d.layer, &us, &from, field.as_deref(), feature);
                self.message = m;
            }
            o => return Err(format!("civil.gis.use.dlg: choice=, field=, go or cancel (not {o})")),
        }
        Ok(())
    }

    /// `civil.gis.use.info`: the dialog's choices (the chosen one marked), the field and the notes.
    pub fn use_info(&self) -> Result<String, String> {
        let d = self.gis3.use_dlg.as_ref().ok_or("civil.gis.use.info: the dialog is not open")?;
        let l = layer_by(self, &d.layer).ok_or("civil.gis.use.info: no layer")?;
        let ch = self.use_choices(&d.layer);
        let mut out = vec![tr!("vec.use.title", layer = l.name.as_str())];
        for (i, c) in ch.iter().enumerate() {
            out.push(format!("{} {}", if i == d.choice { "(•)" } else { "( )" }, string_of(c.get("label"))));
        }
        let c = ch.get(d.choice);
        if c.and_then(|c| c.get("from")).and_then(Value::as_str) == Some("field") {
            out.push(format!("{}: {} [{}]", t0("vec.use.field"), d.field, strs(c.and_then(|c| c.get("fields"))).join(", ")));
        }
        if c.and_then(|c| c.get("use")).and_then(Value::as_str) == Some("boundary") {
            out.push(match d.feature.filter(|_| l.geom == "pg") {
                Some(i) => tr!("vec.use.selected", n = i + 1),
                None if l.geom == "pg" => t0("vec.use.largest"),
                None => String::new(),
            });
        }
        out.push(t0("vec.use.note"));
        Ok(out.join(" | "))
    }

    /// `useForSurface(project, id, { use, from, field, feature })`.
    pub fn use_for_surface(&mut self, id: &str, us: &str, from: &str, field: Option<&str>, feature: Option<usize>) -> UseResult {
        let Some(l) = layer_by(self, id) else { return (false, t0("layers.use.gone")) };
        let fs: Vec<Feature> = cc_io_civil::gis_layers::features_of(&self.project.gis_features, id).to_vec();
        let field = field.unwrap_or("");
        if from == "field" && !l.fields.iter().any(|f| f.name == field && f.ty == "number") {
            return (false, t0("layers.use.nofield"));
        }
        // zFrom(f): Some(Some(z)) / Some(None) from the field, None = the geometry's own
        let z_from = |f: &Feature| -> Option<Option<f64>> {
            if from == "field" { Some(f.p.get(field).and_then(Value::as_f64).filter(|z| z.is_finite())) } else { None }
        };
        let tin = |cv: &CivilView| truthy(cv.project.state.get("surface").and_then(|s| s.get("tin")));
        match us {
            "breaklines" => {
                let mut lines: Vec<Vec<(f64, f64, f64)>> = Vec::new();
                let mut skipped = 0usize;
                for f in &fs {
                    let parts: Vec<&Vec<Vec<f64>>> = match &f.g {
                        Some(Geom::Ln(c)) => c.iter().collect(),
                        Some(Geom::Pg(c)) => c.iter().flatten().collect(),
                        _ => continue,
                    };
                    let z = z_from(f);
                    for part in parts {
                        let pts: Vec<Option<(f64, f64, f64)>> = part
                            .iter()
                            .map(|p| {
                                let zz = match z {
                                    Some(z) => z,
                                    None => p.get(2).copied(),
                                };
                                zz.filter(|z| z.is_finite())
                                    .map(|z| (p.first().copied().unwrap_or(f64::NAN), p.get(1).copied().unwrap_or(f64::NAN), z))
                            })
                            .collect();
                        if pts.len() < 2 || pts.iter().any(Option::is_none) {
                            skipped += 1;
                            continue;
                        }
                        lines.push(pts.into_iter().flatten().collect());
                    }
                }
                if lines.is_empty() {
                    return (
                        false,
                        if from == "field" {
                            tr!("layers.use.noline.field", layer = l.name.as_str(), field = field)
                        } else {
                            tr!("layers.use.noline", layer = l.name.as_str())
                        },
                    );
                }
                let n0 = cc_civil::js::arr(self.project.state.get("surface").and_then(|s| s.get("breaklines"))).len();
                let name = l.name.clone();
                self.project.change(&tr!("layers.undo.brk", layer = l.name.as_str()), &mut |s: &mut Value| {
                    if let Some(b) = s.get_mut("surface").and_then(|s| s.get_mut("breaklines")).and_then(Value::as_array_mut) {
                        for (i, pts) in lines.iter().enumerate() {
                            let pv: Vec<Value> =
                                pts.iter().map(|(x, y, z)| json!({ "x": jnum(*x), "y": jnum(*y), "z": jnum(*z) })).collect();
                            b.push(json!({ "name": slice16(&format!("{name} {}", i + 1), 120), "pts": pv }));
                        }
                    }
                });
                let verts: usize = lines.iter().map(Vec::len).sum();
                let mut m = if n0 > 0 {
                    tr!("layers.use.brk.added.more", n = lines.len(), verts = verts, layer = l.name.as_str(), before = n0)
                } else {
                    tr!("layers.use.brk.added", n = lines.len(), verts = verts, layer = l.name.as_str())
                };
                if skipped > 0 {
                    m.push(' ');
                    m.push_str(&tr!("layers.use.brk.skipped", n = skipped));
                }
                if tin(self) {
                    m.push(' ');
                    m.push_str(&t0("layers.use.tin"));
                }
                (true, m)
            }
            "boundary" => {
                let cand: Vec<&Feature> = match feature.and_then(|i| fs.get(i)).filter(|f| matches!(f.g, Some(Geom::Pg(_)))) {
                    Some(f) => vec![f],
                    None => fs.iter().collect(),
                };
                let mut best: Option<(f64, &Vec<Vec<f64>>)> = None;
                for f in cand {
                    let Some(Geom::Pg(polys)) = &f.g else { continue };
                    for poly in polys {
                        let Some(r) = poly.first() else { continue };
                        let a = cc_io_civil::gis_geom::ring_area(r).abs();
                        if best.is_none_or(|(b, _)| a > b) {
                            best = Some((a, r));
                        }
                    }
                }
                let Some((_, r)) = best else { return (false, tr!("layers.use.nopoly", layer = l.name.as_str())) };
                let pts: Vec<Value> = r
                    .iter()
                    .take(r.len().saturating_sub(1))
                    .map(|p| json!({ "x": jnum(p.first().copied().unwrap_or(f64::NAN)), "y": jnum(p.get(1).copied().unwrap_or(f64::NAN)) }))
                    .collect();
                let n = pts.len();
                self.project.change(&tr!("layers.undo.boundary", layer = l.name.as_str()), &mut |s: &mut Value| {
                    if let Some(sf) = s.get_mut("surface").and_then(Value::as_object_mut) {
                        sf.insert("boundary".into(), Value::Array(pts.clone()));
                    }
                });
                (
                    true,
                    tr!(
                        if feature.is_some() { "layers.use.boundary.selected" } else { "layers.use.boundary.largest" },
                        layer = l.name.as_str(),
                        n = n
                    ),
                )
            }
            "points" => {
                let mut pts: Vec<Pt> = Vec::new();
                let mut skipped = 0usize;
                let d = slice16(&l.name, 200);
                for f in &fs {
                    let Some(Geom::Pt(c)) = &f.g else { continue };
                    let zz = match z_from(f) {
                        Some(z) => z,
                        None => c.get(2).copied(),
                    };
                    let Some(z) = zz.filter(|z| z.is_finite()) else {
                        skipped += 1;
                        continue;
                    };
                    pts.push(Pt {
                        n: "0".into(),
                        x: c.first().copied().unwrap_or(f64::NAN),
                        y: c.get(1).copied().unwrap_or(f64::NAN),
                        z: Some(z),
                        d: d.clone(),
                        blk: None,
                    });
                }
                if pts.is_empty() {
                    return (
                        false,
                        if from == "field" {
                            tr!("layers.use.nopoint.field", layer = l.name.as_str(), field = field)
                        } else {
                            tr!("layers.use.nopoint", layer = l.name.as_str())
                        },
                    );
                }
                let existing: Vec<String> = self.project.points.iter().map(|p| p.n.clone()).collect();
                let (got, _, _) = cc_civil::clip::renumber(&pts, &existing, "always", None);
                let first = got.first().map(|p| p.n.clone()).unwrap_or_default();
                let last = got.last().map(|p| p.n.clone()).unwrap_or_default();
                let n = got.len();
                self.project.edit_points(&tr!("layers.undo.pts", layer = l.name.as_str()), &mut |mut all, _| {
                    all.extend(got.iter().cloned());
                    Some(all)
                });
                let mut m = tr!("layers.use.pts.added", n = n, layer = l.name.as_str(), first = first.as_str(), last = last.as_str());
                if skipped > 0 {
                    m.push(' ');
                    m.push_str(&tr!("layers.use.pts.skipped", n = skipped));
                }
                if tin(self) {
                    m.push(' ');
                    m.push_str(&t0("layers.use.tin"));
                }
                (true, m)
            }
            _ => (false, "Unknown use.".into()),
        }
    }

    // --------------------------------------------------------------------------------------------- askCrs answers

    /// `civil.gis.import=pickcrs`: the project's system for the layers, through the picker.
    pub fn gis_import_ask(&mut self, unknown: Option<usize>) -> Result<(), String> {
        let d = self.gis_import.as_ref().ok_or("civil.gis.import: the import dialog is not open")?;
        let near = d.plan.get("near").and_then(|n| Some([n.get(0)?.as_f64()?, n.get(1)?.as_f64()?]));
        match unknown {
            Some(si) => {
                let s = d.sources.get(si).ok_or("civil.gis.import: no such file")?;
                if !matches!(s.src, Src::None) {
                    return Err("civil.gis.import: that file says its system".into());
                }
                let reason = tr!("vec.unknown.which", file = s.file.as_str());
                self.crs_open(Some((reason, near, "source", PickFor::GisUnknown(si))));
            }
            None => {
                if truthy(self.project.state.get("crs")) {
                    return Err("civil.gis.import: the layers go into the project's system".into());
                }
                let why = d.plan.get("why").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
                let reason = why.unwrap_or_else(|| t0("vec.pick.reason"));
                self.crs_open(Some((reason, near, "project", PickFor::GisProject)));
            }
        }
        Ok(())
    }
    /// chooseProject's answer.
    pub fn gis_import_target(&mut self, v: Option<Value>) {
        let Some(c) = v else { return };
        if let Some(why) = cc_io_civil::crs::crs_problem(&c, self.project.units()) {
            self.message = why;
            return;
        }
        if let Some(d) = self.gis_import.as_mut() {
            d.target = Some(c);
            d.set_crs = true;
        }
    }
    /// The unknown file's system.
    pub fn gis_import_unknown(&mut self, si: usize, v: Option<Value>) {
        let Some(c) = v else { return };
        if let Some(d) = self.gis_import.as_mut() {
            let name = string_of(c.get("name"));
            let geographic = c.get("unit").and_then(Value::as_str) == Some("deg");
            d.overrides.retain(|(i, _)| *i != si);
            d.overrides.push((si, Src::Crs { crs: c, name, matched: None, geographic, notes: None }));
        }
    }
    /// A latitude / longitude point file's system (pointsLatLon): the points go in now, or wait no more.
    pub fn import_latlon_answer(&mut self, v: Option<Value>) {
        let Some(d) = self.gis3.latlon_wait.take() else { return };
        let name = d.name.clone();
        let Some(c) = v else {
            self.message = tr!("cv.imp.latlon.need", file = name.as_str());
            return;
        };
        if let Some(why) = cc_io_civil::crs::crs_problem(&c, self.project.units()) {
            self.message = why;
            self.message = tr!("cv.imp.latlon.need", file = name.as_str());
            return;
        }
        self.import_with_crs(d, c);
    }
}

/// The "Use for the surface" window.
pub fn use_window(cv: &CivilView, ctx: &egui::Context, tk: &Tokens, acts: &mut Vec<(&'static str, String)>) {
    let Some(d) = &cv.gis3.use_dlg else { return };
    let Some(l) = layer_by(cv, &d.layer) else { return };
    let ch = cv.use_choices(&d.layer);
    let mut open = true;
    crate::shell::Dialog::new(tr!("vec.use.title", layer = l.name.as_str()))
        .id(egui::Id::unique("cv_gis_use"))
        .pivot(egui::Align2::CENTER_TOP)
        .default_pos(egui::pos2(ctx.content_rect().center().x, ctx.content_rect().min.y + 70.0))
        .open(&mut open)
        .collapsible(false)
        .default_width(420.0)
        .show(ctx, |ui| {
            for (i, c) in ch.iter().enumerate() {
                if ui.radio(i == d.choice, string_of(c.get("label"))).clicked() {
                    acts.push(("civil.gis.use.dlg", format!("choice={i}")));
                }
            }
            let c = ch.get(d.choice);
            if c.and_then(|c| c.get("from")).and_then(Value::as_str) == Some("field") {
                let fields = strs(c.and_then(|c| c.get("fields")));
                egui::ComboBox::new("cv_gis_use_field", t0("vec.use.field")).selected_text(&d.field).show_ui(ui, |ui| {
                    for f in &fields {
                        if ui.selectable_label(*f == d.field, f).clicked() {
                            acts.push(("civil.gis.use.dlg", format!("field={f}")));
                        }
                    }
                });
            }
            if c.and_then(|c| c.get("use")).and_then(Value::as_str) == Some("boundary") && l.geom == "pg" {
                let n = match d.feature {
                    Some(i) => tr!("vec.use.selected", n = i + 1),
                    None => t0("vec.use.largest"),
                };
                ui.label(RichText::new(n).small().color(tk.text_dim));
            }
            crate::shell::note(ui, tk, &t0("vec.use.note"));
            crate::shell::foot(ui, |ui| {
                if ui.add(crate::shell::Btn::new(t0("vec.cancel"))).clicked() {
                    acts.push(("civil.gis.use.dlg", "cancel".into()));
                }
                if ui.add(crate::shell::Btn::primary(t0("vec.use.go"))).clicked() {
                    acts.push(("civil.gis.use.dlg", "go".into()));
                }
            });
        });
    if !open {
        acts.push(("civil.gis.use.dlg", "cancel".into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GJ: &str = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","properties":{"ELEV":610},"geometry":{"type":"LineString","coordinates":[[-104.99,39.74],[-104.989,39.741]]}},
        {"type":"Feature","properties":{"ELEV":612},"geometry":{"type":"LineString","coordinates":[[-104.99,39.742],[-104.989,39.743],[-104.988,39.742]]}}]}"#;

    #[test]
    fn a_layers_lines_become_breaklines_from_a_field() {
        let mut cv = CivilView::sample();
        cv.crs_set("2232").unwrap();
        cv.gis_open("contours.geojson", GJ.as_bytes().to_vec());
        cv.gis_import_cmd("ok").unwrap();
        let id = cv.gis_layers()[0].id.clone();
        cv.use_open(&id).unwrap();
        let info = cv.use_info().unwrap();
        assert!(info.contains("ELEV"), "{info}");
        let n0 = cv.project.state["surface"]["breaklines"].as_array().map_or(0, Vec::len);
        cv.use_cmd("go").unwrap();
        assert_eq!(cv.project.state["surface"]["breaklines"].as_array().unwrap().len(), n0 + 2, "{}", cv.message);
        assert_eq!(cv.project.state["surface"]["breaklines"][n0]["pts"][0]["z"].as_f64(), Some(610.0));
        cv.undo();
        assert_eq!(cv.project.state["surface"]["breaklines"].as_array().map_or(0, Vec::len), n0);
    }
}
