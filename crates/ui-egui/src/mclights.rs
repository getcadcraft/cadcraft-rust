//! ModelCraft's Lights panel (0.5 RC.11 (d)) as modelcraft.js runs it: Lamp, Spotlight and Light panel start the Light
//! tool (cc-m3d-tools, already at parity), which places one with a click (a spotlight: a second click aims it); the list
//! switches each on or off and picks one; the editor changes the picked lamp (brightness, colour and its presets, beam,
//! size, where it is) and deletes it — one undo step each, 0.5's words. The view draws each lamp's mark (a disc in its
//! colour with rays; a spotlight's aim, a panel's outline; dashed when off) and a click on a mark with Select picks it.
//! Lamps light the rendered pictures (Export › Render a picture…). World coordinates.
//!
//! Actions (`model.edit=<action>`): light-add:<point|spot|area>, light-pick:<id> (the list's name: picks or unpicks),
//! light-sel:<id> (a click on its mark), light-on:<id>=<on|off>, light-lumens:<text>, light-angle:<text>,
//! light-color:<#rrggbb>, light-preset:<warm|neutral|cool>, light-len:<w|d|b|x|y|z>=<length>, light-del.

use cc_m3d_tools::tools::ToolArg;
use egui::{Color32, Pos2, RichText, Stroke};
use serde_json::{Value, json};

use crate::modelcraft::{ModelView, mt};
use crate::theme::Tokens;

/// The colour presets (modelcraft.js renderLights).
pub const PRESETS: [(&str, &str); 3] = [("warm", "#ffd2a1"), ("neutral", "#fff1dc"), ("cool", "#eef3ff")];

impl ModelView {
    /// lightName(id, L): its own name, else "Lamp 2" (its place among the lamps of its kind).
    pub fn light_name(&self, id: f64) -> String {
        let lights = &self.tools.host.model.lights;
        let Some(l) = lights.get(id) else { return String::new() };
        if !l.name.is_empty() {
            return l.name.clone();
        }
        // findIndex + 1 (0 when not found, as the JS)
        let n = lights.iter().filter(|(_, x)| x.kind == l.kind).position(|(k, _)| k == id).map_or(0, |i| i + 1);
        mt(&format!("app.light.name.{}", l.kind), &[("n", &n.to_string())])
    }

    /// renderLights' check: the picked lamp is forgotten once it is gone (an undo, a delete).
    fn check_sel_light(&mut self) {
        if let Some(id) = self.sel_light
            && self.tools.host.model.lights.get(id).is_none()
        {
            self.sel_light = None;
        }
    }

    /// changeLight(patch): one undo step on the picked lamp.
    fn change_light(&mut self, patch: Value) {
        let Some(id) = self.sel_light.filter(|id| self.tools.host.model.lights.get(*id).is_some()) else { return };
        let r = self.tools.host.model.change(&mt("app.undo.light", &[]), |m| m.set_light(id, &patch));
        if let Err(e) = r {
            self.print(e.to_string());
        }
    }

    /// After a click of the Light tool: host.lightPlaced(id).
    pub fn poll_light_placed(&mut self) {
        if let Some(id) = self.tools.host.light_placed.take() {
            self.sel_light = Some(id);
            let name = self.light_name(id);
            self.print(mt("app.light.added", &[("name", &name)]));
        }
    }

    /// The light actions; None when `name` is not one of them.
    pub fn lights_act(&mut self, name: &str, arg: &str) -> Option<Result<(), String>> {
        if !name.starts_with("light-") {
            return None;
        }
        self.check_sel_light();
        let id_of = |s: &str| s.trim().parse::<f64>().ok().filter(|v| v.is_finite());
        let r = match name {
            "light-add" => {
                if !matches!(arg, "point" | "spot" | "area") {
                    return Some(Err(format!("model.edit: light-add: unknown kind {arg}")));
                }
                let _ = self.tools.set("light", ToolArg::Light(json!({ "kind": arg })));
                self.vcb.clear();
                Ok(())
            }
            "light-pick" => match id_of(arg) {
                Some(id) => {
                    self.sel_light = if self.sel_light == Some(id) { None } else { Some(id) };
                    Ok(())
                }
                None => Err("model.edit: light-pick:<id>".into()),
            },
            "light-sel" => match id_of(arg).filter(|id| self.tools.host.model.lights.get(*id).is_some()) {
                Some(id) => {
                    self.sel_light = Some(id);
                    let n = self.light_name(id);
                    self.print(mt("app.light.picked", &[("name", &n)]));
                    Ok(())
                }
                None => Err("model.edit: light-sel:<id of a light>".into()),
            },
            "light-on" => {
                let Some((id, v)) = arg.split_once('=').and_then(|(a, b)| Some((id_of(a)?, b))) else {
                    return Some(Err("model.edit: light-on:<id>=<on|off>".into()));
                };
                self.sel_light = Some(id);
                self.change_light(json!({ "on": v == "on" }));
                Ok(())
            }
            "light-lumens" | "light-angle" => {
                // Number(input.value): not a number reaches cleanLight as NaN (null here), which keeps its default
                let v = cc_m3d::file::str_to_number(arg);
                let key = if name == "light-lumens" { "lumens" } else { "angle" };
                self.change_light(json!({ key: v }));
                Ok(())
            }
            "light-color" => {
                self.change_light(json!({ "color": arg }));
                Ok(())
            }
            "light-preset" => match PRESETS.iter().find(|(k, _)| *k == arg) {
                Some((_, c)) => {
                    self.change_light(json!({ "color": c }));
                    Ok(())
                }
                None => Err(format!("model.edit: light-preset: unknown preset {arg}")),
            },
            "light-len" => {
                let Some((k, text)) = arg.split_once('=') else { return Some(Err("model.edit: light-len:<w|d|b|x|y|z>=<length>".into())) };
                let Some(l) = self.sel_light.and_then(|id| self.tools.host.model.lights.get(id)).cloned() else { return Some(Ok(())) };
                let u = self.tools.host.model.units.clone();
                match cc_m3d_tools::units::parse_length(text, &u).filter(|v| v.is_finite()) {
                    None => self.print(mt("app.len.bad", &[])),
                    Some(v) => {
                        let patch = match k {
                            "w" => json!({ "size": [v, l.size[1]] }),
                            "d" => json!({ "size": [l.size[0], v] }),
                            "b" => json!({ "size": [v, v] }),
                            "x" => json!({ "p": [v, l.p[1], l.p[2]] }),
                            "y" => json!({ "p": [l.p[0], v, l.p[2]] }),
                            "z" => json!({ "p": [l.p[0], l.p[1], v] }),
                            _ => return Some(Err(format!("model.edit: light-len: unknown field {k}"))),
                        };
                        self.change_light(patch);
                    }
                }
                Ok(())
            }
            "light-del" => {
                if let Some(id) = self.sel_light {
                    let nm = self.light_name(id);
                    let _ = self.tools.host.model.change(&mt("app.undo.light.delete", &[]), |m| Ok(m.delete_light(id)));
                    self.sel_light = None;
                    self.print(mt("app.light.deleted", &[("name", &nm)]));
                }
                Ok(())
            }
            _ => Err(format!("model.edit: unknown action {name}")),
        };
        Some(r)
    }

    /// The lamp whose mark is within 11 px of (x, y) in the view, nearest first (the page's capture-phase pointerdown).
    pub fn light_at(&self, x: f64, y: f64) -> Option<f64> {
        let cam = &self.tools.host.cam;
        let vp = cam.view_proj();
        let mut best = None;
        let mut bd = 11.0;
        for (id, l) in self.tools.host.model.lights.iter() {
            if let Some(s) = cam.project_vp(l.p, &vp) {
                let d = cc_m3d::js::hypot2(s[0] - x, s[1] - y);
                if d < bd {
                    bd = d;
                    best = Some(id);
                }
            }
        }
        best
    }

    /// view.drawExtra: each lamp's mark.
    pub fn light_marks(&self, painter: &egui::Painter, to: &dyn Fn([f64; 3]) -> Option<Pos2>, _t: &Tokens) {
        let sel_col = Color32::from_rgb(0x2a, 0x6c, 0xf0);
        for (id, l) in self.tools.host.model.lights.iter() {
            let Some(s) = to(l.p) else { continue };
            let sel = self.sel_light == Some(id);
            // view3d.js ink on the view's paper
            let ink = if sel { sel_col } else { Color32::from_rgb(0x17, 0x20, 0x2b) };
            let stroke = Stroke::new(if sel { 2.5 } else { 1.5 }, ink);
            let fill = crate::mcmats::rgb(&l.color).map_or(Color32::WHITE, |(r, g, b)| Color32::from_rgb(r, g, b));
            let line = |a: Pos2, b: Pos2| {
                if l.on {
                    painter.line_segment([a, b], stroke);
                } else {
                    painter.extend(egui::Shape::dashed_line(&[a, b], stroke, 3.0, 3.0));
                }
            };
            if l.kind == "area" {
                let d = l.dir;
                let ax = if d[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
                let e1 = cc_m3d::vec::norm(cc_m3d::vec::cross(d, ax));
                let e2 = cc_m3d::vec::cross(d, e1);
                let (w, h) = (l.size[0] / 2.0, l.size[1] / 2.0);
                let q: Vec<Option<Pos2>> = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
                    .iter()
                    .map(|[a, b]| to(cc_m3d::vec::add(l.p, cc_m3d::vec::add(cc_m3d::vec::mul(e1, a * w), cc_m3d::vec::mul(e2, b * h)))))
                    .collect();
                if q.iter().all(Option::is_some) {
                    let pts: Vec<Pos2> = q.into_iter().flatten().collect();
                    painter.add(egui::Shape::convex_polygon(pts.clone(), fill.gamma_multiply(0.45), Stroke::NONE));
                    for k in 0..pts.len() {
                        if let (Some(a), Some(b)) = (pts.get(k), pts.get((k + 1) % pts.len())) {
                            line(*a, *b);
                        }
                    }
                }
            }
            if l.kind == "spot"
                && let Some(a) = to(cc_m3d::vec::add(l.p, cc_m3d::vec::mul(l.dir, 0.6)))
            {
                line(s, a);
            }
            painter.circle_filled(s, 6.0, fill);
            if l.on {
                painter.circle_stroke(s, 6.0, stroke);
            } else {
                let ring: Vec<Pos2> = (0..=24)
                    .map(|k| {
                        let a = k as f32 * std::f32::consts::TAU / 24.0;
                        s + egui::vec2(a.cos() * 6.0, a.sin() * 6.0)
                    })
                    .collect();
                painter.extend(egui::Shape::dashed_line(&ring, stroke, 3.0, 3.0));
            }
            for k in 0..8 {
                let an = k as f32 * std::f32::consts::FRAC_PI_4;
                let (c, sn) = (an.cos(), an.sin());
                line(s + egui::vec2(c * 8.0, sn * 8.0), s + egui::vec2(c * 11.0, sn * 11.0));
            }
        }
    }

    // ---- the panel (renderLights) ----

    pub(crate) fn lights_section(
        &self,
        ui: &mut egui::Ui,
        t: &Tokens,
        boxes: &mut std::collections::HashMap<String, String>,
        acts: &mut Vec<String>,
    ) {
        egui::CollapsingHeader::new(RichText::new(mt("ui.lights", &[])).color(t.text_strong))
            .id_salt("mc-lights")
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for k in ["point", "spot", "area"] {
                        if ui.button(mt(&format!("ui.lights.{k}"), &[])).clicked() {
                            acts.push(format!("light-add:{k}"));
                        }
                    }
                });
                let lights = &self.tools.host.model.lights;
                let sel = self.sel_light.filter(|id| lights.get(*id).is_some());
                for (id, l) in lights.iter() {
                    ui.horizontal(|ui| {
                        let mut on = l.on;
                        if ui.checkbox(&mut on, "").on_hover_text(mt("app.light.on", &[])).changed() {
                            acts.push(format!("light-on:{}={}", crate::mcact::ns(id), if on { "on" } else { "off" }));
                        }
                        if ui.selectable_label(sel == Some(id), self.light_name(id)).clicked() {
                            acts.push(format!("light-pick:{}", crate::mcact::ns(id)));
                        }
                    });
                }
                if let Some(id) = sel
                    && let Some(l) = lights.get(id)
                {
                    let u = self.tools.host.model.units.clone();
                    egui::Grid::new("mc-light-edit").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                        let field = |ui: &mut egui::Ui,
                                     boxes: &mut std::collections::HashMap<String, String>,
                                     label: &str,
                                     key: &str,
                                     text: &str| {
                            ui.label(RichText::new(label).color(t.text_dim));
                            let r = crate::mcpanels::text_box(ui, boxes, key, text, 110.0);
                            ui.end_row();
                            r
                        };
                        if let Some(x) = field(ui, boxes, &mt("app.light.lumens", &[]), "mc-light-lm", &cc_m3d::js::num_str(l.lumens)) {
                            acts.push(format!("light-lumens:{x}"));
                        }
                        ui.label(RichText::new(mt("app.light.color", &[])).color(t.text_dim));
                        let mut rgb = crate::mcmats::rgb(&l.color).map_or([255, 231, 196], |(r, g, b)| [r, g, b]);
                        if ui.color_edit_button_srgb(&mut rgb).changed() {
                            acts.push(format!("light-color:#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]));
                        }
                        ui.end_row();
                        ui.label("");
                        ui.horizontal(|ui| {
                            for (k, _) in PRESETS {
                                if ui.small_button(mt(&format!("app.light.{k}"), &[])).clicked() {
                                    acts.push(format!("light-preset:{k}"));
                                }
                            }
                        });
                        ui.end_row();
                        if l.kind == "spot"
                            && let Some(x) = field(ui, boxes, &mt("app.light.angle", &[]), "mc-light-ang", &cc_m3d::js::num_str(l.angle))
                        {
                            acts.push(format!("light-angle:{x}"));
                        }
                        let mut rows: Vec<(&str, &str, f64, &str)> = Vec::new();
                        if l.kind == "area" {
                            rows.push(("app.light.width", "mc-light-w", l.size[0], "w"));
                            rows.push(("app.light.depth", "mc-light-d", l.size[1], "d"));
                        } else {
                            rows.push(("app.light.bulb", "mc-light-b", l.size[0], "b"));
                        }
                        rows.push(("app.light.x", "mc-light-x", l.p[0], "x"));
                        rows.push(("app.light.y", "mc-light-y", l.p[1], "y"));
                        rows.push(("app.light.height", "mc-light-z", l.p[2], "z"));
                        for (label, key, m, k) in rows {
                            if let Some(x) = field(ui, boxes, &mt(label, &[]), key, &cc_m3d::units::format_length(m, &u)) {
                                acts.push(format!("light-len:{k}={x}"));
                            }
                        }
                        ui.label("");
                        if ui.button(mt("app.light.delete", &[])).clicked() {
                            acts.push("light-del".into());
                        }
                        ui.end_row();
                    });
                }
                ui.label(RichText::new(mt("ui.lights.note", &[])).small().color(t.text_dim));
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lamp_is_placed_changed_and_deleted() {
        let mut v = ModelView::new(cc_m3d::model::Model::new(), "t", false).unwrap();
        v.act("view-size:800,600").unwrap();
        v.act("light-add:point").unwrap();
        assert_eq!(v.tools.name, "light");
        v.act("click:400,300").unwrap();
        let (id, l) = v.tools.host.model.lights.iter().next().map(|(k, l)| (k, l.clone())).expect("a lamp");
        assert_eq!(v.sel_light, Some(id));
        assert_eq!(v.message, "Added Lamp 1.");
        assert_eq!(l.lumens, 800.0);
        v.act("light-lumens:1500").unwrap();
        v.act("light-preset:cool").unwrap();
        v.act("light-len:z=2.5m").unwrap();
        let l = v.tools.host.model.lights.get(id).unwrap();
        assert_eq!((l.lumens, l.color.as_str(), l.p[2]), (1500.0, "#eef3ff", 2.5));
        v.act("light-lumens:abc").unwrap();
        assert_eq!(v.tools.host.model.lights.get(id).unwrap().lumens, 800.0, "not a number: cleanLight's default");
        v.act("light-len:x=zz").unwrap();
        assert_eq!(v.message, mt("app.len.bad", &[]));
        v.act("light-on:".to_string().as_str()).unwrap_err();
        v.act(&format!("light-on:{}=off", crate::mcact::ns(id))).unwrap();
        assert!(!v.tools.host.model.lights.get(id).unwrap().on);
        v.act("light-del").unwrap();
        assert!(v.tools.host.model.lights.get(id).is_none());
        assert_eq!(v.message, "Deleted Lamp 1.");
        v.act("undo").unwrap();
        assert!(v.tools.host.model.lights.get(id).is_some());
    }
}
