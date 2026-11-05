//! ModelCraft's terrain and starter library (0.4 M24 / M25) as modelcraft.js does them: Edit › Terrain from contour
//! lines (the selected edges' points triangulated in plan, one group placed where they are), Edit › Terrain grid… (a
//! dialog for width, depth and spacing, then the Place tool), Edit › Sculpt terrain (the Sculpt tool); the
//! Components panel's Starter library… (a dialog of about thirty everyday things and the entourage, built when picked,
//! once per model, then placed on the face clicked). The geometry is cc-m3d-tools' terrain.js / starter.js port; one
//! undo step each with 0.5's labels and status words (`forks/tools/mcuidiff.mjs --terrain`).
//!
//! Actions (`model.edit=<action>`): terrain-contours, terrain-grid (opens the dialog), terrain-grid-place:{"width",
//! "depth", "spacing"} (its Place, as typed), terrain-grid-cancel, sculpt, starter (opens the dialog),
//! starter-place:<id>, starter-cancel.

use cc_m3d::ops::Sel;
use cc_m3d::vec::IDENT;
use cc_m3d_tools::tools::ToolArg;
use serde_json::Value;

use crate::modelcraft::{ModelView, mt};

/// The terrain grid dialog: width, depth and spacing as typed, the error shown.
#[derive(Clone, Debug)]
pub struct TerrainDlg {
    pub w: String,
    pub d: String,
    pub s: String,
    pub err: String,
}

impl ModelView {
    /// The terrain and starter actions; None when `name` is not one of them.
    pub fn terrain_act(&mut self, name: &str, arg: &str) -> Option<Result<(), String>> {
        let r = match name {
            "terrain-contours" => {
                self.terrain_from_selection();
                Ok(())
            }
            "terrain-grid" => {
                self.terrain_grid_dialog();
                Ok(())
            }
            "terrain-grid-place" => self.terrain_grid_submit(arg),
            "terrain-grid-cancel" => {
                self.terrain_dlg = None;
                Ok(())
            }
            "sculpt" => {
                self.set_tool("sculpt");
                Ok(())
            }
            _ => return self.starter_act(name, arg),
        };
        Some(r)
    }

    /// terrainFromSelection(): the selected edges' points (three or more) triangulated into a terrain group.
    pub fn terrain_from_selection(&mut self) {
        let def_id = self.ctx_def();
        let eids: Vec<f64> = self.tools.host.sel.iter().filter(|s| s.t == "e").map(|s| s.id).collect();
        let mut pts: Vec<f64> = Vec::new();
        if let Some(def) = self.tools.host.model.def(&def_id) {
            for id in &eids {
                if let Some(e) = def.edges.get(*id) {
                    for v in [e.a, e.b] {
                        if !pts.contains(&v) {
                            pts.push(v);
                        }
                    }
                }
                if pts.len() >= 3 {
                    break;
                }
            }
        }
        if pts.len() < 3 {
            self.print(mt("app.terrain.need", &[]));
            return;
        }
        let path = self.tools.host.path.clone();
        let nm = mt("app.terrain.name", &[]);
        let r = self.tools.host.model.change(&mt("app.undo.terrain", &[]), |m| {
            let Some(ctx) = m.context(&path) else { return Ok(None) };
            let Some(d) = cc_m3d_tools::terrain::terrain_from_contours(m, &ctx.def, &eids, &nm) else { return Ok(None) };
            let iid = m.defs.get_mut(&ctx.def).map(|cd| cc_m3d::model::place(cd, &d, &IDENT));
            Ok(iid.map(|i| (i, d)))
        });
        let Ok(Some((iid, d))) = r else {
            self.print(mt("app.terrain.flat", &[]));
            return;
        };
        self.tools.host.set_sel(vec![Sel::new("i", iid)]);
        let n = self.tools.host.model.def(&d).map_or(0, |d| d.faces.len());
        self.print(mt("app.terrain.made", &[("n", &n.to_string())]));
        self.after_history();
    }

    /// terrainGridDialog(): 20 m × 20 m at 1 m (60′ × 60′ at 2′ in feet and inches), in the model's units.
    pub fn terrain_grid_dialog(&mut self) {
        let u = self.tools.host.model.units.clone();
        let metric = !(u == "ft-in" || u == "in");
        let f = |v: f64| cc_m3d::units::format_length(v, &u);
        let (wd, sp) = if metric { (20.0, 1.0) } else { (6.096 * 3.0, 0.6096) };
        self.terrain_dlg = Some(TerrainDlg { w: f(wd), d: f(wd), s: f(sp), err: String::new() });
    }

    /// The dialog's Place: `{"width", "depth", "spacing"}` as typed; each must be a length over 0.
    fn terrain_grid_submit(&mut self, arg: &str) -> Result<(), String> {
        let j: Value =
            serde_json::from_str(arg).map_err(|_| "model.edit: terrain-grid-place:{\"width\",\"depth\",\"spacing\"}".to_string())?;
        let s = |k: &str| j.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let (w, d, sp) = (s("width"), s("depth"), s("spacing"));
        let u = self.tools.host.model.units.clone();
        let p = |x: &str| cc_m3d_tools::units::parse_length(x, &u).filter(|v| *v > 0.0);
        let (Some(wv), Some(dv), Some(sv)) = (p(&w), p(&d), p(&sp)) else {
            self.terrain_dlg = Some(TerrainDlg { w, d, s: sp, err: mt("app.terrain.grid.bad", &[]) });
            return Ok(());
        };
        self.terrain_dlg = None;
        let nm = mt("app.terrain.name", &[]);
        let r = self
            .tools
            .host
            .model
            .change(&mt("app.undo.terrain", &[]), |m| Ok(cc_m3d_tools::terrain::terrain_grid(m, wv, dv, sv, [0.0; 3], &nm)));
        let def_id = match r {
            Ok(id) => id,
            Err(e) => {
                self.print(e.to_string());
                return Ok(());
            }
        };
        self.after_history();
        let hint = mt("app.terrain.grid.where", &[]);
        let _ = self.tools.set("place", ToolArg::Place { def_id, align: false, hint: Some(hint.clone()) });
        self.vcb.clear();
        self.print(hint);
        Ok(())
    }

    /// The terrain grid dialog.
    pub fn terrain_dialogs(&mut self, ctx: &egui::Context, act: &mut Option<String>) {
        if let Some(TerrainDlg { mut w, mut d, mut s, err }) = self.terrain_dlg.clone() {
            let mut a: Option<String> = None;
            crate::shell::Dialog::new(mt("app.terrain.grid.h", &[]))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Grid::new("mc-tg").num_columns(2).show(ui, |ui| {
                        for (k, v) in [("app.terrain.grid.w", &mut w), ("app.terrain.grid.d", &mut d), ("app.terrain.grid.s", &mut s)] {
                            let l = ui.label(mt(k, &[]));
                            ui.add_sized([120.0, 20.0], crate::shell::field(v)).labelled_by(l.id);
                            ui.end_row();
                        }
                    });
                    ui.label(egui::RichText::new(mt("app.terrain.grid.note", &[])).small());
                    if !err.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(0xd0, 0x22, 0x3e), &err);
                    }
                    crate::shell::foot(ui, |ui| {
                        if ui.add(crate::shell::Btn::new(mt("app.btn.cancel", &[]))).clicked() {
                            a = Some("terrain-grid-cancel".into());
                        }
                        if ui.add(crate::shell::Btn::primary(mt("app.text3d.place", &[]))).clicked() {
                            a = Some(format!("terrain-grid-place:{}", serde_json::json!({ "width": w, "depth": d, "spacing": s })));
                        }
                    });
                });
            self.terrain_dlg = Some(TerrainDlg { w, d, s, err });
            if a.is_some() {
                *act = a;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::mcedit::testkit::groups;
    use crate::modelcraft::ModelView;

    fn empty() -> ModelView {
        ModelView::new(cc_m3d::model::Model::new(), "t", false).unwrap()
    }

    #[test]
    fn terrain_grid_dialog_checks_and_places() {
        let mut v = empty();
        v.act("units:m").unwrap();
        v.act("terrain-grid").unwrap();
        let d = v.terrain_dlg.clone().unwrap();
        assert_eq!((d.w.as_str(), d.s.as_str()), ("20 m", "1 m"));
        v.act(r#"terrain-grid-place:{"width":"x","depth":"2","spacing":"1"}"#).unwrap();
        assert_eq!(v.terrain_dlg.as_ref().map(|d| d.err.as_str()), Some(mt_("app.terrain.grid.bad").as_str()));
        v.act(r#"terrain-grid-place:{"width":"4","depth":"2","spacing":"1"}"#).unwrap();
        assert!(v.terrain_dlg.is_none());
        assert_eq!(v.tools.name, "place");
        assert_eq!(v.message, mt_("app.terrain.grid.where"));
        let t = v.tools.host.model.defs.values().find(|d| d.kind == "group").map(|d| (d.name.clone(), d.faces.len()));
        assert!(t.as_ref().is_some_and(|(n, f)| n.starts_with("Terrain") && *f == 16), "{t:?}");
        v.act("sculpt").unwrap();
        assert_eq!(v.tools.name, "sculpt");
    }

    fn mt_(k: &str) -> String {
        crate::modelcraft::mt(k, &[])
    }

    #[test]
    fn terrain_from_contours_needs_three_points() {
        let mut v = groups(&[0.0]);
        v.act("terrain-contours").unwrap();
        assert_eq!(v.message, mt_("app.terrain.need"));
    }
}
