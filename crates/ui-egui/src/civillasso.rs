//! CivilCraft's lasso and point settings (M11.14), as civilcraft.js 0.4 W2.7 and Settings › CivilCraft do them: a
//! Shift+drag in Select stays a box while it runs close to the line from its start; once it bends more than 12 px off that
//! line it is a freehand lasso whose mode follows the first sideways move (right = window, left = crossing) until Space
//! cycles window → crossing → fence; Esc cancels. The lasso picks through points.js `lassoPoints` (lasso-core.js). The
//! point settings: the column order of copied and pasted point rows (PNEZD / PENZD), how pasted points are numbered
//! (keep free numbers / always new ones) and where new numbers start.

use cc_civil::i18n::t;
use cc_civil::tr;
use cc_detail::lasso_core::{Pt as SPt, lasso_mode, next_mode, thin};

use crate::civilview::CivilView;

/// Settings › CivilCraft's point fields (civilcraft.js `settings.order / renumber / start`).
#[derive(Clone, Debug)]
pub struct PtSettings {
    pub order: String,
    pub renumber: String,
    pub start: Option<f64>,
}

impl Default for PtSettings {
    fn default() -> Self {
        PtSettings { order: "PNEZD".into(), renumber: "clash".into(), start: None }
    }
}

/// A Shift+drag in Select (civilcraft.js `drag` of kind box / lasso), in CSS px of the plan.
#[derive(Clone, Debug)]
pub struct LassoDrag {
    pub s: SPt,
    pub last: SPt,
    pub path: Vec<SPt>,
    pub moved: bool,
    pub lasso: bool,
    pub mode: Option<&'static str>,
    pub spaced: bool,
    pub said: Option<&'static str>,
}

impl LassoDrag {
    pub fn new(s: SPt) -> LassoDrag {
        LassoDrag { s, last: s, path: vec![s], moved: false, lasso: false, mode: None, spaced: false, said: None }
    }

    /// pointermove: past the 4 px slack the drag is a box; each move goes on the path (civilcraft.js `lassoTrack`).
    /// Returns what to say when the mode changes.
    pub fn track(&mut self, s: SPt) -> Option<String> {
        if !self.moved {
            if (s.x - self.s.x).hypot(s.y - self.s.y) <= 4.0 {
                return None;
            }
            self.moved = true;
        }
        self.last = s;
        self.path.push(s);
        if !self.lasso {
            let (a, b) = (self.s, self.last);
            let l = (b.x - a.x).hypot(b.y - a.y);
            let off = |q: &SPt| {
                if l < 1.0 { (q.x - a.x).hypot(q.y - a.y) } else { ((b.x - a.x) * (a.y - q.y) - (a.x - q.x) * (b.y - a.y)).abs() / l }
            };
            if !self.path.iter().any(|q| off(q) > 12.0) {
                return None;
            }
            self.lasso = true;
        }
        if !self.spaced {
            let first = self.path.iter().copied().find(|q| lasso_mode(self.s, *q, 6.0).is_some()).unwrap_or(self.last);
            self.mode = lasso_mode(self.s, first, 6.0).or(self.mode).or(Some("window"));
        }
        self.say_mode()
    }

    /// Space while dragging: a box becomes a window lasso, a lasso takes the next mode.
    pub fn space(&mut self) -> Option<String> {
        if self.lasso {
            self.mode = Some(next_mode(self.mode.unwrap_or("window")));
        } else {
            self.lasso = true;
            self.mode = Some("window");
        }
        self.spaced = true;
        self.said = None;
        self.say_mode()
    }

    fn say_mode(&mut self) -> Option<String> {
        if self.said == self.mode {
            return None;
        }
        self.said = self.mode;
        self.mode.map(|m| t(&format!("cv.lasso.{m}"), &[]))
    }
}

impl CivilView {
    /// civilcraft.js `lassoPick(path, mode)`: the path (CSS px) thinned to 2 px, in world units; points.js `lassoPoints`
    /// with a 4 px reach; the points it picks join the selection.
    pub fn lasso_pick(&mut self, path_css: &[SPt], mode: &str) {
        let z = self.view.zoom / self.ppp.max(1e-9);
        let pts: Vec<cc_civil::geo::P2> = thin(path_css, 2.0)
            .iter()
            .map(|q| {
                let (x, y) = self.view.to_world(q.x * self.ppp, q.y * self.ppp);
                cc_civil::geo::P2 { x, y }
            })
            .collect();
        let got = cc_civil::points::lasso_points(&self.project.points, &pts, mode, 4.0 / z);
        let mut set = self.picked_numbers();
        let mut added = 0usize;
        for i in got {
            if let Some(p) = self.project.points.get(i)
                && set.insert(p.n.clone())
            {
                added += 1;
            }
        }
        let total = set.len();
        self.set_picked(set);
        let count = |n: usize| cc_civil::units::grouped(n as f64, 0);
        self.message = if added > 0 {
            tr!(
                "cv.pts.box",
                added = tr!("cv.pts.n", n = added, count = count(added)),
                total = tr!("cv.pts.n", n = total, count = count(total))
            )
        } else {
            t("cv.pts.lasso.none", &[])
        };
    }

    /// `civil.lasso=<window|crossing|fence>:x,y;x,y;…` (world coordinates, the path as drawn).
    pub fn lasso_command(&mut self, spec: &str) -> Result<(), String> {
        let (mode, rest) = spec.split_once(':').ok_or("civil.lasso: name = \"window|crossing|fence:x,y;x,y;…\"")?;
        let mode = mode.trim();
        if !["window", "crossing", "fence"].contains(&mode) {
            return Err(format!("civil.lasso: mode window, crossing or fence (not {mode})"));
        }
        let mut path = Vec::new();
        for xy in rest.split(';').map(str::trim).filter(|s| !s.is_empty()).take(100_000) {
            let (x, y) = xy.split_once(',').ok_or_else(|| format!("civil.lasso: {xy} is not x,y"))?;
            let (x, y) = (x.trim().parse::<f64>().map_err(|_| "civil.lasso: x")?, y.trim().parse::<f64>().map_err(|_| "civil.lasso: y")?);
            if !x.is_finite() || !y.is_finite() {
                return Err("civil.lasso: coordinates must be numbers".into());
            }
            let (sx, sy) = self.view.to_screen(x, y);
            path.push(SPt { x: sx / self.ppp, y: sy / self.ppp });
        }
        if path.len() < 2 {
            return Err("civil.lasso: two points or more".into());
        }
        self.lasso_pick(&path, mode);
        Ok(())
    }

    /// `civil.settings=order=PNEZD|PENZD,renumber=clash|always,start=<n>|""` (Settings › CivilCraft's point fields).
    pub fn settings_command(&mut self, spec: &str) -> Result<(), String> {
        for kv in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (k, v) = kv.split_once('=').map_or((kv, ""), |(a, b)| (a.trim(), b.trim()));
            match k {
                "order" => self.pt_settings.order = if v == "PENZD" { "PENZD" } else { "PNEZD" }.into(),
                "renumber" => self.pt_settings.renumber = if v == "always" { "always" } else { "clash" }.into(),
                "start" => {
                    let n = cc_civil::js::str_to_number(v);
                    if v.is_empty() {
                        self.pt_settings.start = None;
                    } else if n.fract() == 0.0 && n > 0.0 && n < 1e9 {
                        self.pt_settings.start = Some(n);
                    } else {
                        return Err(format!("civil.settings: start = a whole number from 1 (not {v})"));
                    }
                }
                o => return Err(format!("civil.settings: unknown setting {o} (order, renumber, start)")),
            }
        }
        Ok(())
    }
}

/// Settings › CivilCraft's point fields as a block of the left panel.
pub fn settings_ui(cv: &mut CivilView, ui: &mut egui::Ui, tk: &crate::theme::Tokens, acts: &mut Vec<(&'static str, String)>) {
    egui::CollapsingHeader::new(egui::RichText::new(t("set.renum", &[])).color(tk.text_strong)).id_salt("cv_set_pts").show(ui, |ui| {
        ui.label(egui::RichText::new(t("set.order", &[])).color(tk.text_dim));
        ui.horizontal(|ui| {
            for (k, label) in [("PNEZD", "P, N, E, Z, D"), ("PENZD", "P, E, N, Z, D")] {
                if ui.radio(cv.pt_settings.order == k, label).clicked() {
                    acts.push(("civil.settings", format!("order={k}")));
                }
            }
        });
        ui.label(egui::RichText::new(t("set.order.hint", &[])).small().color(tk.text_dim));
        ui.label(egui::RichText::new(t("set.renum", &[])).color(tk.text_dim));
        ui.horizontal(|ui| {
            for (k, key) in [("clash", "set.renum.clash"), ("always", "set.renum.always")] {
                if ui.radio(cv.pt_settings.renumber == k, t(key, &[])).clicked() {
                    acts.push(("civil.settings", format!("renumber={k}")));
                }
            }
        });
        ui.horizontal(|ui| {
            let l = ui.label(t("set.start", &[]));
            let shown = cv.pt_settings.start.map_or(String::new(), cc_civil::js::num_to_string);
            let mut text = cv.lasso_start_text.clone().unwrap_or_else(|| shown.clone());
            let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(90.0).hint_text(t("set.start.ph", &[]))).labelled_by(l.id);
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
        ui.label(egui::RichText::new(t("set.renum.hint", &[])).small().color(tk.text_dim));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_is_a_box_until_it_bends_then_a_lasso_whose_mode_space_cycles() {
        let mut d = LassoDrag::new(SPt { x: 100.0, y: 100.0 });
        assert!(d.track(SPt { x: 102.0, y: 101.0 }).is_none(), "inside the slack");
        assert!(d.track(SPt { x: 160.0, y: 130.0 }).is_none());
        assert!(!d.lasso, "a straight drag stays a box");
        let said = d.track(SPt { x: 200.0, y: 60.0 });
        assert!(d.lasso && d.mode == Some("window"), "bent: a window lasso (it went right)");
        assert_eq!(said, Some(t("cv.lasso.window", &[])));
        assert_eq!(d.space(), Some(t("cv.lasso.crossing", &[])));
        assert_eq!(d.space(), Some(t("cv.lasso.fence", &[])));
        assert!(d.track(SPt { x: 10.0, y: 10.0 }).is_none(), "Space keeps the mode");
        let mut l = LassoDrag::new(SPt { x: 100.0, y: 100.0 });
        l.track(SPt { x: 60.0, y: 110.0 });
        l.track(SPt { x: 40.0, y: 160.0 });
        assert_eq!(l.mode, Some("crossing"), "it went left first");
        let mut b = LassoDrag::new(SPt { x: 0.0, y: 0.0 });
        assert_eq!(b.space(), Some(t("cv.lasso.window", &[])), "Space on a box: a window lasso");
    }

    #[test]
    fn settings_read_and_refuse() {
        let mut cv = CivilView::sample();
        cv.settings_command("order=PENZD,renumber=always,start=500").unwrap();
        assert_eq!(
            (cv.pt_settings.order.as_str(), cv.pt_settings.renumber.as_str(), cv.pt_settings.start),
            ("PENZD", "always", Some(500.0))
        );
        assert!(cv.settings_command("start=1.5").is_err());
        cv.settings_command("start=").unwrap();
        assert_eq!(cv.pt_settings.start, None);
        assert!(cv.settings_command("colour=red").is_err());
    }
}
