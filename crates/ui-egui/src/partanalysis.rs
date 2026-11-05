//! PartCraft's surface analyses in the app (M13.10): draft-ui.js — View › Views › Draft analysis / Zebra stripes /
//! Curvature, one card over the view: the part coloured by its draft angle against a pull (±X, ±Y, ±Z or square to a
//! picked flat face, Reverse, the minimum draft, each class's share of the area), by reflection stripes seen along the
//! view (redrawn as the view turns), or by its mean curvature (analysis.rs on the part's mesh). Display only: nothing is
//! saved or changed. Command `part.analysis` (toggle:<mode> | mode:<m> | close | pull:<k> | reverse:on/off | min:<deg> |
//! stripes:<n> | pick | face:<name> | escape | rows).

use cc_part::{analysis as AN, jv};

use crate::partview::{Mode, PartView, tr, trv};

/// The pulls in draft-ui.js's order (analysis.js `PULLS`).
pub const PULLS: [(&str, [f64; 3]); 6] = [
    ("+x", [1.0, 0.0, 0.0]),
    ("-x", [-1.0, 0.0, 0.0]),
    ("+y", [0.0, 1.0, 0.0]),
    ("-y", [0.0, -1.0, 0.0]),
    ("+z", [0.0, 0.0, 1.0]),
    ("-z", [0.0, 0.0, -1.0]),
];
const MODES: [&str; 3] = ["draft", "zebra", "curvature"];

/// draft-ui.js's state (`st`), the colours it gave the part and the colours it uses.
#[derive(Clone, Debug)]
pub struct Analysis {
    pub active: bool,
    pub mode: &'static str,
    pub picking: bool,
    pub key: String,
    pub face: Option<[f64; 3]>,
    pub reverse: bool,
    pub min: f64,
    /// positive, needs, negative, total (mm²)
    pub areas: Option<[f64; 4]>,
    pub stripes: f64,
    pub lim: Option<f64>,
    /// one colour per triangle corner (3 per triangle of the part's mesh), None = the body colour
    pub colors: Option<Vec<[u8; 4]>>,
    /// the view direction (part frame) the zebra stripes were drawn for
    pub cam: Option<[f64; 3]>,
    /// the theme's colours: positive, needs, negative, concave, convex, dark, light (0…1)
    pub palette: [[f64; 3]; 7],
    /// the minimum and stripes fields while they are typed
    pub typing: Option<(String, String)>,
}

impl Default for Analysis {
    fn default() -> Self {
        Analysis {
            active: false,
            mode: "draft",
            picking: false,
            key: "+y".into(),
            face: None,
            reverse: false,
            min: 1.0,
            areas: None,
            stripes: 16.0,
            lim: None,
            colors: None,
            cam: None,
            palette: [
                [0.07, 0.37, 0.19],
                [0.45, 0.32, 0.0],
                [0.62, 0.06, 0.16],
                [0.1, 0.25, 0.69],
                [0.62, 0.06, 0.16],
                [0.11, 0.11, 0.13],
                [0.95, 0.95, 0.93],
            ],
            typing: None,
        }
    }
}

fn rgba(c: [f64; 3]) -> [u8; 4] {
    let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [b(c[0]), b(c[1]), b(c[2]), 255]
}

impl PartView {
    /// `ctx.blocked()`: the analyses work on the part only.
    fn analysis_blocked(&self) -> String {
        if self.mode == Mode::Sketch || self.drawing.is_some() { tr("draft.blocked") } else { String::new() }
    }

    fn analysis_pull(&self) -> [f64; 3] {
        let a = &self.analysis;
        let d = match (a.key.as_str(), a.face) {
            ("face", Some(f)) => f,
            (k, _) => PULLS.iter().find(|(p, _)| *p == k).map_or([0.0, 1.0, 0.0], |(_, d)| *d),
        };
        if a.reverse { [-d[0], -d[1], -d[2]] } else { d }
    }

    /// draft-ui.js `apply()` without the scene: the areas or the curvature limit and the corner colours.
    pub fn analysis_compute(&mut self) {
        let mesh = if self.analysis.active { self.regen.raw_mesh.as_ref() } else { None };
        let Some(m) = mesh else {
            let a = &mut self.analysis;
            a.areas = None;
            a.lim = None;
            a.colors = None;
            return;
        };
        let pal = self.analysis.palette;
        let flat: Vec<f64> = match self.analysis.mode {
            "zebra" => {
                let view = self.analysis.cam.unwrap_or([0.0, 0.0, -1.0]);
                AN::zebra_colors(m, None, view, self.analysis.stripes, pal[5], pal[6])
            }
            "curvature" => {
                let (c, lim) = AN::curvature_colors(m, pal[3], pal[6], pal[4]);
                self.analysis.lim = Some(lim);
                // per vertex → per corner
                let mut out = Vec::with_capacity(m.indices.len() * 3);
                for &vi in &m.indices {
                    let i = vi as usize * 3;
                    out.extend((0..3).map(|k| c.get(i + k).copied().unwrap_or(0.5)));
                }
                out
            }
            _ => {
                let (c, areas) = AN::draft_analysis(m, None, self.analysis_pull(), self.analysis.min, [pal[0], pal[1], pal[2]]);
                self.analysis.areas = Some(areas);
                c
            }
        };
        self.analysis.colors = Some(flat.as_chunks::<3>().0.iter().map(|c| rgba(*c)).collect());
    }

    /// `apply()`: colours computed and the part drawn with them.
    pub fn analysis_apply(&mut self) {
        self.analysis_compute();
        self.build_scene_now();
    }

    fn analysis_start(&mut self, mode: &str) {
        let why = self.analysis_blocked();
        if !why.is_empty() {
            self.note = why;
            return;
        }
        if self.regen.mesh.is_none() {
            self.note = tr("draft.no_part");
            return;
        }
        if let Some(m) = MODES.iter().find(|m| **m == mode) {
            self.analysis.mode = m;
        }
        self.analysis.active = true;
        self.analysis.picking = false;
        self.analysis_apply();
    }

    pub fn analysis_stop(&mut self) {
        if !self.analysis.active {
            return;
        }
        self.analysis.active = false;
        self.analysis.picking = false;
        self.analysis_apply();
    }

    fn analysis_mode(&mut self, m: &str) {
        let Some(m) = MODES.iter().find(|x| **x == m) else { return };
        self.analysis.mode = m;
        self.analysis.picking = false;
        self.analysis_apply();
    }

    /// draft-ui.js `refresh()` after a rebuild (colours only; the caller draws the scene).
    pub fn analysis_refresh(&mut self) {
        if !self.analysis.active {
            return;
        }
        if !self.analysis_blocked().is_empty() {
            self.analysis.active = false;
            self.analysis.picking = false;
        }
        self.analysis_compute();
    }

    /// Zebra stripes follow the view: the view direction (the part's frame) changed.
    pub fn analysis_follow_view(&mut self, dir: [f64; 3]) {
        if !(self.analysis.active && self.analysis.mode == "zebra") {
            return;
        }
        let same = self.analysis.cam.is_some_and(|c| (c[0] - dir[0]).abs() + (c[1] - dir[1]).abs() + (c[2] - dir[2]).abs() < 1e-6);
        if !same {
            self.analysis.cam = Some(dir);
            self.analysis_apply();
        }
    }

    /// The legend's percentage of a class (`pct`).
    fn analysis_pct(&self, i: usize) -> String {
        match self.analysis.areas {
            Some(a) if a[3] > 0.0 => cc_part::js::to_fixed(100.0 * a.get(i).copied().unwrap_or(0.0) / a[3], 1),
            _ => "0.0".into(),
        }
    }

    /// The curvature note's limit and radius (`+lim.toPrecision(3)`).
    pub fn analysis_lim_words(&self) -> (String, String) {
        match self.analysis.lim {
            Some(l) => (cc_part::js::num_str(cc_part::js::str_num(&cc_part::js::to_precision(l, 3))), {
                cc_part::js::num_str(cc_part::js::str_num(&cc_part::js::to_precision(1.0 / l, 3)))
            }),
            None => ("—".into(), "—".into()),
        }
    }

    /// The legend rows (draft), the curvature note, or the stripes, in words.
    pub fn analysis_rows(&self) -> Vec<(String, String)> {
        let min = cc_part::js::num_str(self.analysis.min);
        match self.analysis.mode {
            "draft" => ["positive", "needs", "negative"]
                .iter()
                .enumerate()
                .map(|(i, c)| (trv(&format!("draft.class.{c}"), &[("min", min.clone())]), format!("{} %", self.analysis_pct(i))))
                .collect(),
            "curvature" => {
                let (lim, rad) = self.analysis_lim_words();
                vec![(trv("analysis.curv.note", &[("lim", lim), ("rad", rad)]), String::new())]
            }
            _ => vec![(tr("analysis.stripes"), cc_part::js::num_str(self.analysis.stripes))],
        }
    }

    /// `part.analysis=<op>`.
    pub fn analysis_cmd(&mut self, name: &str) -> Result<(), String> {
        let (k, v) = name.split_once(':').unwrap_or((name, ""));
        match k {
            "toggle" | "" => {
                let mode = if v.is_empty() { "draft" } else { v };
                if self.analysis.active && self.analysis.mode == mode {
                    self.analysis_stop();
                } else if self.analysis.active {
                    self.analysis_mode(mode);
                } else {
                    self.analysis_start(mode);
                }
            }
            "mode" => self.analysis_mode(v),
            "close" => self.analysis_stop(),
            "pull" => {
                if !(PULLS.iter().any(|(p, _)| *p == v) || v == "face") {
                    return Err(format!("part.analysis: pull is one of +x -x +y -y +z -z face, not {v}"));
                }
                self.analysis.key = v.to_string();
                self.analysis_apply();
            }
            "reverse" => {
                self.analysis.reverse = v == "on";
                self.analysis_apply();
            }
            "min" => {
                let x = cc_part::js::str_num(v);
                if !v.trim().is_empty() && x.is_finite() && (0.0..=45.0).contains(&x) {
                    self.analysis.min = x;
                    self.analysis_apply();
                }
            }
            "stripes" => {
                let x = cc_part::js::str_num(v).round();
                if (4.0..=64.0).contains(&x) {
                    self.analysis.stripes = x;
                    self.analysis_apply();
                }
            }
            "pick" => {
                self.analysis.picking = !self.analysis.picking;
                self.note = if self.analysis.picking { tr("draft.pick.hint") } else { String::new() };
            }
            "face" => {
                if !(self.analysis.active && self.analysis.picking) {
                    return Ok(());
                }
                let (fname, _) = v.split_once('|').unwrap_or((v, ""));
                let rec = self.faces().iter().find(|f| jv::s(f, "name") == fname).cloned();
                match rec.filter(|f| jv::s(f, "type") == "plane") {
                    Some(f) => {
                        self.analysis.face = Some(crate::partview::p3(f.get("n").unwrap_or(&serde_json::Value::Null)));
                        self.analysis.key = "face".into();
                        self.analysis.picking = false;
                        self.note = String::new();
                        self.analysis_apply();
                    }
                    None => self.note = tr("draft.need_flat"),
                }
            }
            "escape" => {
                if self.analysis.active {
                    if self.analysis.picking {
                        self.analysis.picking = false;
                        self.note = String::new();
                    } else {
                        self.analysis_stop();
                    }
                }
            }
            "rows" => {
                self.note = if !self.analysis.active {
                    String::new()
                } else {
                    self.analysis_rows()
                        .into_iter()
                        .map(|(k, v)| if v.is_empty() { k } else { format!("{k} {v}") })
                        .collect::<Vec<_>>()
                        .join(" · ")
                };
            }
            _ => return Err(format!("part.analysis: unknown {name}")),
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------- the card

use crate::partpanel::{Cmd, cmd};
use crate::theme::Tokens;
use egui::RichText;

/// The analysis card over the view (draft-ui.js `render()`).
pub fn card(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    let a = pv.analysis.clone();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(tr(&format!("analysis.title.{}", a.mode))).strong().family(crate::theme::bold_family()).color(t.text_strong),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(crate::shell::Btn::new(tr("draft.close"))).on_hover_text(tr("draft.close.title")).clicked() {
                *act = cmd("part.analysis", "close");
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label(tr("analysis.show"));
        egui::ComboBox::from_id_salt("pc-analysis-mode").selected_text(tr(&format!("analysis.mode.{}", a.mode))).width(170.0).show_ui(
            ui,
            |ui| {
                for m in MODES {
                    if ui.selectable_label(a.mode == m, tr(&format!("analysis.mode.{m}"))).clicked() && a.mode != m {
                        *act = cmd("part.analysis", format!("mode:{m}"));
                    }
                }
            },
        );
    });
    let (mut min_t, mut str_t) = a.typing.clone().unwrap_or_else(|| (cc_part::js::num_str(a.min), cc_part::js::num_str(a.stripes)));
    match a.mode {
        "zebra" => {
            ui.horizontal(|ui| {
                ui.label(tr("analysis.stripes"));
                let r = ui.add_sized([60.0, 26.0], crate::shell::field(&mut str_t));
                if r.has_focus() {
                    pv.analysis.typing = Some((min_t.clone(), str_t.clone()));
                }
                if r.lost_focus() {
                    pv.analysis.typing = None;
                    *act = cmd("part.analysis", format!("stripes:{}", str_t.trim()));
                }
            });
            crate::shell::note(ui, t, &tr("analysis.zebra.note"));
        }
        "curvature" => {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 10.0), egui::Sense::hover());
            let c = |v: [f64; 3]| egui::Color32::from_rgb((v[0] * 255.0) as u8, (v[1] * 255.0) as u8, (v[2] * 255.0) as u8);
            let (l, m, r) = (c(a.palette[3]), c(a.palette[6]), c(a.palette[4]));
            let mut mesh = egui::Mesh::default();
            let (x0, x1, xm) = (rect.left(), rect.right(), rect.center().x);
            for (xa, xb, ca, cb) in [(x0, xm, l, m), (xm, x1, m, r)] {
                let i = mesh.vertices.len() as u32;
                mesh.colored_vertex(egui::pos2(xa, rect.top()), ca);
                mesh.colored_vertex(egui::pos2(xb, rect.top()), cb);
                mesh.colored_vertex(egui::pos2(xb, rect.bottom()), cb);
                mesh.colored_vertex(egui::pos2(xa, rect.bottom()), ca);
                mesh.add_triangle(i, i + 1, i + 2);
                mesh.add_triangle(i, i + 2, i + 3);
            }
            ui.painter().add(egui::Shape::mesh(mesh));
            // the three words under the bar's ends and middle (.pc-curv-ends)
            ui.columns(3, |c| {
                if let [a, b, d] = c {
                    a.label(RichText::new(tr("analysis.curv.concave")).small());
                    b.vertical_centered(|ui| ui.label(RichText::new(tr("analysis.curv.flat")).small()));
                    d.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.label(RichText::new(tr("analysis.curv.convex")).small())
                    });
                }
            });
            let (lim, rad) = pv.analysis_lim_words();
            ui.label(RichText::new(trv("analysis.curv.note", &[("lim", lim), ("rad", rad)])).small().color(t.text_dim));
        }
        _ => {
            ui.horizontal(|ui| {
                ui.label(tr("draft.pull"));
                let shown = |k: &str| tr(&format!("draft.pull.{}", k.replace('+', "p").replace('-', "m")));
                let cur = if a.key == "face" { tr("draft.pull.face") } else { shown(&a.key) };
                egui::ComboBox::from_id_salt("pc-draft-pull").selected_text(cur).width(140.0).show_ui(ui, |ui| {
                    for (k, _) in PULLS {
                        if ui.selectable_label(a.key == k, shown(k)).clicked() {
                            *act = cmd("part.analysis", format!("pull:{k}"));
                        }
                    }
                    if ui.add_enabled(a.face.is_some(), egui::Button::selectable(a.key == "face", tr("draft.pull.face"))).clicked() {
                        *act = cmd("part.analysis", "pull:face");
                    }
                });
                let words = if a.picking { tr("draft.picking") } else { tr("draft.pick") };
                if ui.add(crate::shell::Btn::new(words)).clicked() {
                    *act = cmd("part.analysis", "pick");
                }
            });
            let mut rev = a.reverse;
            if ui.checkbox(&mut rev, tr("draft.reverse")).changed() {
                *act = cmd("part.analysis", if rev { "reverse:on" } else { "reverse:off" });
            }
            ui.horizontal(|ui| {
                ui.label(tr("draft.min"));
                let r = ui.add_sized([60.0, 26.0], crate::shell::field(&mut min_t));
                if r.has_focus() {
                    pv.analysis.typing = Some((min_t.clone(), str_t.clone()));
                }
                if r.lost_focus() {
                    pv.analysis.typing = None;
                    *act = cmd("part.analysis", format!("min:{}", min_t.trim()));
                }
                ui.label(RichText::new("°").color(t.text_dim));
            });
            for (i, (words, pct)) in pv.analysis_rows().into_iter().enumerate() {
                ui.horizontal(|ui| {
                    let c = a.palette.get(i).copied().unwrap_or([0.5; 3]);
                    let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    ui.painter().rect_filled(
                        r,
                        2.0,
                        egui::Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8),
                    );
                    ui.label(RichText::new(words).small());
                    ui.label(RichText::new(pct).small().color(t.text_dim));
                });
            }
            let note = if a.picking { tr("draft.pick.hint") } else { tr("draft.note") };
            ui.label(RichText::new(note).small().color(t.text_dim));
        }
    }
}

/// The theme's colours for the analyses (draft-ui.js `ctx.color(token)`).
pub fn palette(t: &Tokens) -> [[f64; 3]; 7] {
    let f = |c: egui::Color32| [f64::from(c.r()) / 255.0, f64::from(c.g()) / 255.0, f64::from(c.b()) / 255.0];
    let l = |i: usize| t.layers.get(i).copied().unwrap_or(t.accent);
    [f(l(2)), f(l(1)), f(l(0)), f(l(4)), f(l(0)), f(t.ink), f(t.sheet)]
}

#[cfg(test)]
mod tests {
    use crate::partview::PartView;

    #[test]
    fn draft_zebra_curvature_on_the_sample() {
        let mut pv = PartView::sample().unwrap();
        pv.analysis_cmd("toggle:draft").unwrap();
        assert!(pv.analysis.active);
        let a = pv.analysis.areas.unwrap();
        assert!((a[0] + a[1] + a[2] - a[3]).abs() < 1e-6 * a[3]);
        let nt = pv.regen.raw_mesh.as_ref().unwrap().tri_count();
        assert_eq!(pv.analysis.colors.as_ref().map(Vec::len), Some(nt * 3));
        // hostile values are refused or ignored
        assert!(pv.analysis_cmd("pull:../x").is_err());
        pv.analysis_cmd("min:NaN").unwrap();
        pv.analysis_cmd("min:1e308").unwrap();
        assert_eq!(pv.analysis.min, 1.0);
        pv.analysis_cmd("stripes:1e300").unwrap();
        assert_eq!(pv.analysis.stripes, 16.0);
        pv.analysis_cmd("toggle:curvature").unwrap();
        assert!(pv.analysis.lim.is_some_and(f64::is_finite));
        pv.analysis_cmd("toggle:zebra").unwrap();
        pv.analysis_follow_view([0.0, 0.0, -1.0]);
        pv.analysis_cmd("close").unwrap();
        assert!(pv.analysis.colors.is_none());
    }
}
