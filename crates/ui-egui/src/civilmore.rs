//! Smaller CivilCraft pieces (M11.20–): a parcel's legal description Copy and Download buttons (parcels-ui.js).

use cc_civil::i18n::t;
use cc_civil::js::{arr, string_of};
use serde_json::Value;

use crate::civilview::{CivilView, Sel};

impl CivilView {
    /// The selected parcel's name and legal description (parcels-ui.js `legalDescription(p, { units, pob })`).
    fn legal_of_selected(&self, cmd: &str) -> Result<(String, String), String> {
        let Some(Sel::Parcel(id)) = &self.sel else { return Err(format!("{cmd}: select a parcel first")) };
        let pj = arr(self.project.state.get("parcels"))
            .iter()
            .find(|p| p.get("id").and_then(Value::as_str) == Some(id.as_str()))
            .cloned()
            .ok_or_else(|| format!("{cmd}: no parcel {id}"))?;
        let pc = cc_civil::parcels::read_parcel(&pj);
        let text = cc_civil::parcels::legal_description(&pc, self.project.units(), pc.pob.as_deref());
        Ok((string_of(pj.get("name")), text))
    }

    /// The Copy button: the legal description on the clipboard (handed to egui on the next frame).
    pub fn pc_copy(&mut self) -> Result<(), String> {
        let (_, text) = self.legal_of_selected("civil.pc.copy")?;
        self.points.copy_out = Some(text);
        self.message = t("pc.copied", &[]);
        Ok(())
    }

    /// The Download button: `<stem> - <parcel name>.txt`, CRLF lines and a final CRLF (0.5 says nothing).
    pub fn pc_download(&self) -> Result<(String, Vec<u8>), String> {
        let (name, text) = self.legal_of_selected("civil.pc.download")?;
        let safe: String =
            name.chars().map(|c| if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).collect();
        let stem = crate::civilexport::file_stem(self.project.state.get("name").and_then(Value::as_str).unwrap_or(""));
        let body = format!("{}\r\n", text.replace('\n', "\r\n"));
        Ok((format!("{stem} - {safe}.txt"), body.into_bytes()))
    }
}

// ------------------------------------------------------------------------------------------------ Settings › CivilCraft

impl CivilView {
    /// civilcraft.js settingsDlg `change` for the fields besides the point ones (labels, corner, exag, latlon,
    /// internet, the assistants) and Reset. → false: not one of them.
    pub fn settings_more(&mut self, k: &str, v: &str) -> Result<bool, String> {
        let on_off = |v: &str| match v {
            "on" | "true" => Ok(true),
            "off" | "false" => Ok(false),
            o => Err(format!("civil.settings: {k} = on or off (not {o})")),
        };
        let s = &mut self.pt_settings;
        match k {
            "labels" => s.labels = if ["n", "nz", "nzd"].contains(&v) { v } else { "nzd" }.into(),
            "corner" => s.corner = if ["cube", "axes", "none"].contains(&v) { v } else { "cube" }.into(),
            "exag" => {
                let x = cc_civil::js::str_to_number(v);
                s.exag = if cc_civil::site3d::EXAGGERATIONS.contains(&x) { x } else { 2.0 };
                self.exag = s.exag;
            }
            "latlon" => s.latlon = if v == "dd" { "dd" } else { "dms" }.into(),
            "internet" => s.internet = on_off(v)?,
            "assist" => {
                s.assist = on_off(v)?;
                if s.assist && s.assist_groups.is_empty() {
                    s.assist_groups = vec!["read".into()];
                }
            }
            "assist.read" | "assist.edit" => {
                let g = &k[7..];
                let want = on_off(v)?;
                let mut groups: Vec<String> = Vec::new();
                for x in ["read", "edit"] {
                    let has = if x == g { want } else { s.assist_groups.iter().any(|y| y == x) };
                    if has {
                        groups.push(x.into());
                    }
                }
                s.assist_groups = groups;
            }
            "reset" => {
                *s = crate::civillasso::PtSettings::default();
                self.exag = s.exag;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// civilcraft.js `readSettings()` + masshaul-ui.js's stored factor, from the app's prefs (untrusted: every field
    /// checked, anything else the default).
    pub fn load_prefs(&mut self, v: &Value) {
        let d = crate::civillasso::PtSettings::default();
        let mut s = d.clone();
        if let Some(o) = v.get("settings").filter(|o| o.is_object()) {
            let st = |k: &str| o.get(k).and_then(Value::as_str);
            if let Some(x) = st("order").filter(|x| ["PNEZD", "PENZD"].contains(x)) {
                s.order = x.into();
            }
            if let Some(x) = st("renumber").filter(|x| ["clash", "always"].contains(x)) {
                s.renumber = x.into();
            }
            if let Some(n) = o.get("start").and_then(Value::as_f64).filter(|n| n.fract() == 0.0 && *n > 0.0 && *n < 1e9) {
                s.start = Some(n);
            }
            if let Some(x) = st("labels").filter(|x| ["n", "nz", "nzd"].contains(x)) {
                s.labels = x.into();
            }
            if let Some(x) = st("corner").filter(|x| ["cube", "axes", "none"].contains(x)) {
                s.corner = x.into();
            }
            if let Some(x) = o.get("exag").and_then(Value::as_f64).filter(|x| [1.0, 2.0, 3.0, 5.0, 10.0].contains(x)) {
                s.exag = x;
            }
            if let Some(x) = st("latlon").filter(|x| ["dms", "dd"].contains(x)) {
                s.latlon = x.into();
            }
            s.internet = o.get("internet") == Some(&Value::Bool(true));
            if let Some(a) = o.get("assistants").filter(|a| a.is_object()) {
                s.assist = a.get("enabled") == Some(&Value::Bool(true));
                s.assist_groups = arr(a.get("groups"))
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|g| *g == "read" || *g == "edit")
                    .map(str::to_string)
                    .collect();
            }
        }
        self.exag = s.exag;
        self.pt_settings = s;
        if let Some(p) = v.get("masshaul").and_then(Value::as_f64).filter(|p| *p > -60.0 && *p < 100.0) {
            self.sheets.mh_pct = p;
        }
    }

    /// What the app keeps between runs: `{ settings: <civilcraft.settings>, masshaul: <percent> }`.
    pub fn prefs_json(&self) -> Value {
        let s = &self.pt_settings;
        serde_json::json!({
            "settings": {
                "order": s.order, "renumber": s.renumber, "start": s.start, "labels": s.labels, "corner": s.corner,
                "exag": s.exag, "latlon": s.latlon, "internet": s.internet,
                "assistants": { "enabled": s.assist, "groups": s.assist_groups },
            },
            "masshaul": self.sheets.mh_pct,
        })
    }

    /// `civil.settings.info`: every setting as `key=value` (what the Settings window shows).
    pub fn settings_info(&self) -> String {
        let s = &self.pt_settings;
        format!(
            "order={} renumber={} start={} labels={} corner={} exag={} latlon={} internet={} assist={} groups={} masshaul={}",
            s.order,
            s.renumber,
            s.start.map_or(String::new(), cc_civil::js::num_to_string),
            s.labels,
            s.corner,
            cc_civil::js::num_to_string(s.exag),
            s.latlon,
            if s.internet { "on" } else { "off" },
            if s.assist { "on" } else { "off" },
            s.assist_groups.join("+"),
            cc_civil::js::num_to_string(self.sheets.mh_pct)
        )
    }
}

/// The Settings window (core's Settings dialog, its CivilCraft tab: settings.inc), every field a `civil.settings` change.
pub fn settings_window(cv: &mut CivilView, ctx: &egui::Context, tk: &crate::theme::Tokens, acts: &mut Vec<(&'static str, String)>) {
    if !cv.settings_open {
        return;
    }
    let mut open = true;
    let dim = |s: String| egui::RichText::new(s).small().color(tk.text_dim);
    egui::Window::new(format!("{} › CivilCraft", t("input.cmd.app.settings", &[])))
        .id(egui::Id::unique("cv_settings"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(420.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
                let s = cv.pt_settings.clone();
                seg(
                    ui,
                    tk,
                    acts,
                    "set.order",
                    "set.order.hint",
                    "order",
                    &s.order,
                    &[("PNEZD", "P, N, E, Z, D".into()), ("PENZD", "P, E, N, Z, D".into())],
                );
                seg(
                    ui,
                    tk,
                    acts,
                    "set.renum",
                    "",
                    "renumber",
                    &s.renumber,
                    &[("clash", t("set.renum.clash", &[])), ("always", t("set.renum.always", &[]))],
                );
                ui.horizontal(|ui| {
                    let l = ui.label(t("set.start", &[]));
                    let shown = s.start.map_or(String::new(), cc_civil::js::num_to_string);
                    let mut text = cv.lasso_start_text.clone().unwrap_or_else(|| shown.clone());
                    let r = ui
                        .add(egui::TextEdit::singleline(&mut text).desired_width(90.0).hint_text(t("set.start.ph", &[])))
                        .labelled_by(l.id);
                    if r.changed() || r.has_focus() {
                        cv.lasso_start_text = Some(text.clone());
                    }
                    if r.lost_focus()
                        && let Some(v) = cv.lasso_start_text.take()
                        && v.trim() != shown
                    {
                        acts.push(("civil.settings", format!("start={}", v.trim())));
                    }
                });
                ui.label(dim(t("set.renum.hint", &[])));
                ui.add_space(6.0);
                seg(
                    ui,
                    tk,
                    acts,
                    "set.labels",
                    "set.labels.hint",
                    "labels",
                    &s.labels,
                    &[("n", t("set.labels.n", &[])), ("nz", t("set.labels.nz", &[])), ("nzd", t("set.labels.nzd", &[]))],
                );
                seg(
                    ui,
                    tk,
                    acts,
                    "set.corner",
                    "set.corner.hint",
                    "corner",
                    &s.corner,
                    &[("cube", t("set.corner.cube", &[])), ("axes", t("set.corner.axes", &[])), ("none", t("set.corner.none", &[]))],
                );
                let ex = cc_civil::js::num_to_string(s.exag);
                let exs: Vec<(&str, String)> = ["1", "2", "3", "5", "10"].iter().map(|x| (*x, format!("{x}×"))).collect();
                seg(ui, tk, acts, "set.exag", "set.exag.hint", "exag", &ex, &exs);
                seg(
                    ui,
                    tk,
                    acts,
                    "set.latlon",
                    "set.latlon.hint",
                    "latlon",
                    &s.latlon,
                    &[("dms", "30°18′00.00″ N".into()), ("dd", "30.300000° N".into())],
                );
                let net = if s.internet { "on" } else { "off" };
                seg(
                    ui,
                    tk,
                    acts,
                    "set.internet",
                    "set.internet.hint",
                    "internet",
                    net,
                    &[("off", t("set.internet.off", &[])), ("on", t("set.internet.on", &[]))],
                );
                ui.label(egui::RichText::new(t("set.assist.legend", &[])).strong().color(tk.text_strong));
                let mut on = s.assist;
                if ui.checkbox(&mut on, t("set.assist.on", &[])).changed() {
                    acts.push(("civil.settings", format!("assist={}", if on { "on" } else { "off" })));
                }
                ui.label(dim(t("set.assist.hint", &[])));
                ui.add_enabled_ui(s.assist, |ui| {
                    for g in ["read", "edit"] {
                        let mut has = s.assist_groups.iter().any(|x| x == g);
                        if ui.checkbox(&mut has, t(&format!("set.assist.{g}"), &[])).changed() {
                            acts.push(("civil.settings", format!("assist.{g}={}", if has { "on" } else { "off" })));
                        }
                    }
                });
                ui.add_space(8.0);
                if ui.button("Reset to defaults").clicked() {
                    acts.push(("civil.settings", "reset".into()));
                }
            });
        });
    if !open {
        cv.settings_open = false;
    }
}

/// One radio group of the Settings window: legend, options, hint.
#[allow(clippy::too_many_arguments)]
fn seg(
    ui: &mut egui::Ui,
    tk: &crate::theme::Tokens,
    acts: &mut Vec<(&'static str, String)>,
    legend: &str,
    hint: &str,
    key: &str,
    cur: &str,
    opts: &[(&str, String)],
) {
    ui.label(egui::RichText::new(t(legend, &[])).strong().color(tk.text_strong));
    ui.horizontal_wrapped(|ui| {
        for (v, label) in opts {
            if ui.radio(cur == *v, label.as_str()).clicked() && cur != *v {
                acts.push(("civil.settings", format!("{key}={v}")));
            }
        }
    });
    if !hint.is_empty() {
        ui.label(egui::RichText::new(t(hint, &[])).small().color(tk.text_dim));
    }
    ui.add_space(6.0);
}

// ------------------------------------------------------------------------------------------------ the corner indicator

type V3 = [f64; 3];

/// Settings › CivilCraft › Corner indicator on the plan and the 3D view (civilcraft.js applyCorner, viewcube.js): the
/// view cube at the top right (on the plan it shows the top; its edges and corners open the 3D view from that side, in
/// 3D a click on a side looks from it), or the axis icon at the bottom left. A click asks for `civil.camera=<side>`.
pub fn corner(cv: &mut CivilView, ui: &mut egui::Ui, rect: egui::Rect, tk: &crate::theme::Tokens) {
    let plan = !cv.show3d;
    let (ax, ay, az): (V3, V3, V3) = if plan {
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0])
    } else {
        match cv.site3.as_ref() {
            Some(s) => s.view.cam.axes(),
            None => return,
        }
    };
    let dot = |a: V3, b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let painter = ui.painter_at(rect);
    match cv.pt_settings.corner.as_str() {
        "axes" => {
            let o = egui::Pos2::new(rect.min.x + 34.0, rect.max.y - 34.0);
            let len = 24.0_f32;
            let mut list: Vec<(&str, V3, egui::Color32)> = vec![
                ("X", [1.0, 0.0, 0.0], egui::Color32::from_rgb(0xd9, 0x4a, 0x4a)),
                ("Y", [0.0, 1.0, 0.0], egui::Color32::from_rgb(0x4a, 0xb8, 0x5c)),
            ];
            if !plan {
                list.push(("Z", [0.0, 0.0, 1.0], egui::Color32::from_rgb(0x4a, 0x7a, 0xe0)));
            }
            // farthest first, so the nearer arrows draw over them
            list.sort_by(|a, b| dot(a.1, az).partial_cmp(&dot(b.1, az)).unwrap_or(std::cmp::Ordering::Equal));
            for (name, d, col) in list {
                let tip = o + egui::vec2(dot(d, ax) as f32 * len, -(dot(d, ay) as f32) * len);
                painter.arrow(o, tip - o, egui::Stroke::new(2.0, col));
                let lab = o + (tip - o) * 1.3;
                painter.text(lab, egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(11.0), col);
            }
        }
        "cube" => {
            let size = 34.0_f32;
            let c = egui::Pos2::new(rect.max.x - 58.0, rect.min.y + 58.0);
            let to = |d: V3| egui::Pos2::new(c.x + dot(d, ax) as f32 * size, c.y - dot(d, ay) as f32 * size);
            let faces: [(&str, &str, V3, [V3; 2]); 6] = [
                ("top", "viewcube.top", [0.0, 0.0, 1.0], [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
                ("bottom", "viewcube.bottom", [0.0, 0.0, -1.0], [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0]]),
                ("front", "viewcube.front", [0.0, -1.0, 0.0], [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]),
                ("back", "viewcube.back", [0.0, 1.0, 0.0], [[-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]),
                ("left", "viewcube.left", [-1.0, 0.0, 0.0], [[0.0, -1.0, 0.0], [0.0, 0.0, 1.0]]),
                ("right", "viewcube.right", [1.0, 0.0, 0.0], [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            ];
            let bound = egui::Rect::from_center_size(c, egui::vec2(size * 2.4, size * 2.4));
            let resp = ui.interact(bound, egui::Id::unique("cv_corner_cube"), egui::Sense::click());
            let pos = resp.hover_pos();
            let mut order = faces;
            order.sort_by(|a, b| dot(a.2, az).partial_cmp(&dot(b.2, az)).unwrap_or(std::cmp::Ordering::Equal));
            let mut hit_name: Option<&str> = None;
            for (name, key, n, [u, v]) in order {
                let f = dot(n, az);
                if f <= 0.02 {
                    continue;
                }
                let corner = |su: f64, sv: f64| {
                    to([
                        n[0] * 0.5 + (u[0] * su + v[0] * sv) * 0.5,
                        n[1] * 0.5 + (u[1] * su + v[1] * sv) * 0.5,
                        n[2] * 0.5 + (u[2] * su + v[2] * sv) * 0.5,
                    ])
                };
                let pts = vec![corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)];
                let shade = (150.0 + 90.0 * f) as u8;
                let hot = pos.is_some_and(|p| egui::Rect::from_points(&pts).contains(p));
                if hot {
                    hit_name = Some(name);
                }
                let fill = if hot {
                    egui::Color32::from_rgb(0x9f, 0xc0, 0xf5)
                } else {
                    egui::Color32::from_rgb(shade, shade, shade.saturating_add(8))
                };
                painter.add(egui::Shape::convex_polygon(pts, fill, egui::Stroke::new(1.0, tk.divider)));
                if f > 0.35 {
                    let label = cc_civil::i18n::t(key, &[]);
                    let label = if label == key { name.to_string() } else { label };
                    painter.text(
                        to([n[0] * 0.5, n[1] * 0.5, n[2] * 0.5]),
                        egui::Align2::CENTER_CENTER,
                        label.to_uppercase(),
                        egui::FontId::proportional(8.0),
                        egui::Color32::from_rgb(0x20, 0x24, 0x2a),
                    );
                }
            }
            // on the plan the cube is seen from above: its edges (outside the top square) and corners open 3D
            if plan && let Some(p) = pos {
                let d = p - c;
                let (h, out) = (size * 0.5, size * 1.2);
                let side = if d.y > h && d.y < out && d.x.abs() <= h {
                    Some("front")
                } else if d.y < -h && d.y > -out && d.x.abs() <= h {
                    Some("back")
                } else if d.x < -h && d.x > -out && d.y.abs() <= h {
                    Some("left")
                } else if d.x > h && d.x < out && d.y.abs() <= h {
                    Some("right")
                } else if d.x.abs() > h && d.y.abs() > h && d.x.abs() < out && d.y.abs() < out {
                    Some("iso")
                } else {
                    None
                };
                if let Some(s) = side {
                    hit_name = Some(s);
                    painter.circle_filled(p, 4.0, egui::Color32::from_rgb(0x9f, 0xc0, 0xf5));
                }
            }
            if plan {
                // the edges around the top: thin bands that say what they open
                let b = egui::Stroke::new(1.0, tk.divider);
                painter.rect_stroke(egui::Rect::from_center_size(c, egui::vec2(size * 2.4, size * 2.4)), 3.0, b, egui::StrokeKind::Inside);
            }
            if resp.clicked()
                && let Some(n) = hit_name.filter(|n| !(plan && *n == "top"))
            {
                cv.corner_cmd = Some(n.to_string());
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_the_prefs_and_junk_is_refused() {
        let mut cv = CivilView::sample();
        cv.settings_command("labels=n,corner=axes,exag=5,latlon=dd,internet=on,assist=on,assist.edit=on").unwrap();
        assert!(cv.prefs_dirty);
        assert_eq!(cv.exag, 5.0);
        cv.sheets.mh_pct = 12.5;
        let p = cv.prefs_json();
        let mut b = CivilView::sample();
        b.load_prefs(&p);
        assert_eq!(b.pt_settings, cv.pt_settings);
        assert_eq!(b.exag, 5.0);
        assert_eq!(b.sheets.mh_pct, 12.5);
        assert!(cv.settings_command("internet=maybe").is_err());
        cv.settings_command("reset").unwrap();
        assert_eq!(cv.pt_settings, crate::civillasso::PtSettings::default());
        let mut c = CivilView::sample();
        c.load_prefs(&serde_json::json!({"settings": {"labels": "zz", "exag": 7, "internet": "yes", "assistants": {"enabled": 1, "groups": ["root", "read"]}}, "masshaul": 500}));
        assert_eq!(c.pt_settings.labels, "nzd");
        assert_eq!(c.exag, 2.0);
        assert!(!c.pt_settings.internet && !c.pt_settings.assist);
        assert_eq!(c.pt_settings.assist_groups, vec!["read".to_string()]);
        assert_eq!(c.sheets.mh_pct, -10.0);
    }

    #[test]
    fn the_legal_description_downloads_with_crlf_lines() {
        let mut cv = CivilView::demo().unwrap();
        assert!(cv.pc_download().is_err(), "nothing selected");
        let id = string_of(cv.project.state["parcels"][0].get("id"));
        cv.sel = Some(Sel::Parcel(id));
        let (name, bytes) = cv.pc_download().unwrap();
        assert!(name.ends_with(".txt") && name.contains(" - "), "{name}");
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.ends_with("\r\n") && !text.replace("\r\n", "").contains('\n'));
        cv.pc_copy().unwrap();
        assert_eq!(cv.message, t("pc.copied", &[]));
        assert!(cv.points.copy_out.is_some());
    }
}

// ------------------------------------------------------------------------------------------------ touch's draft bar

impl CivilView {
    /// The draft bar's buttons (civilcraft.js `draftBar` click): Finish (Enter), Undo point (says how many are left),
    /// Cancel (the draft goes, the tool stays). `civil.draft=finish|back|cancel`.
    pub fn draft_bar(&mut self, act: &str) -> Result<(), String> {
        match act {
            "finish" => {
                if !self.tools.draft.is_empty() {
                    self.finish();
                }
            }
            "back" => {
                if !self.tools.draft.is_empty() {
                    self.tools.draft.pop();
                    self.message = cc_civil::tr!("touch.back.done", n = self.tools.draft.len());
                }
            }
            "cancel" => {
                self.tools.draft.clear();
                self.tools.closed = false;
                self.message = t("cv.cancelled", &[]);
            }
            o => return Err(format!("civil.draft: finish, back or cancel (not {o})")),
        }
        Ok(())
    }
}

/// The draft bar over the plan (ui.inc `data-draftbar`): shown on a touch screen while a line is being drawn.
pub fn draft_bar_ui(cv: &CivilView, ui: &egui::Ui, rect: egui::Rect, tk: &crate::theme::Tokens, acts: &mut Vec<(&'static str, String)>) {
    if !(cv.touch && !cv.tools.draft.is_empty() && !cv.show3d) {
        return;
    }
    egui::Area::new(egui::Id::unique("cv_draftbar")).fixed_pos(egui::pos2(rect.center().x - 170.0, rect.max.y - 64.0)).show(
        ui.ctx(),
        |ui| {
            egui::Frame::new().fill(tk.panel).stroke(egui::Stroke::new(1.5, tk.text)).inner_margin(6).corner_radius(3).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(t("touch.draft", &[])).color(tk.text_dim));
                    for (k, key) in [("finish", "touch.finish"), ("back", "touch.back"), ("cancel", "touch.cancel")] {
                        // finger-sized (0.4 S.8: 44 px targets)
                        if ui.add(egui::Button::new(t(key, &[])).min_size(egui::vec2(88.0, 44.0))).clicked() {
                            acts.push(("civil.draft", k.into()));
                        }
                    }
                });
            });
        },
    );
}

#[cfg(test)]
mod draft_tests {
    use super::*;

    #[test]
    fn the_draft_bar_finishes_drops_and_cancels() {
        let mut cv = CivilView::sample();
        cv.tool_command(crate::civiltools::Tool::Pad);
        cv.click(6_150_150.0, 2_080_100.0);
        cv.click(6_150_250.0, 2_080_110.0);
        cv.draft_bar("back").unwrap();
        assert_eq!(cv.tools.draft.len(), 1);
        cv.draft_bar("cancel").unwrap();
        assert!(cv.tools.draft.is_empty());
        assert_eq!(cv.message, t("cv.cancelled", &[]));
        assert!(cv.draft_bar("nope").is_err());
    }
}
