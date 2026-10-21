//! ModelCraft's solid and drawing tools as modelcraft.js does them: Intersect faces (with the model, with the
//! selection), the Solids menu (Union, Intersect on the selected closed groups and components; Subtract — click the
//! piece to take away, then the piece to cut — through cc-io-m3d's solids.js port on cc-manifold), IFC storeys (the
//! dialog: names and elevations, Add storey, Guess from the model, Save) and the drawing axes (Place axes: three
//! clicks; Align axes to face; Reset axes). Each edit is one undo step with 0.5's undo labels and status words
//! (`forks/tools/mcuidiff.mjs --edit`).
//!
//! Actions (`model.edit=<action>`): intersect-model, intersect-sel, solid-union, solid-subtract, solid-intersect,
//! solid-subtract-pick:<cutter iid>,<iid> (Subtract's two clicks at once), storeys (opens the dialog), storeys-add,
//! storeys-guess, storeys-remove:<row>, storeys-save:[[name, elevation], …] (Save with these rows, as typed),
//! storeys-ok (Save with the rows the dialog holds), storeys-cancel, axes-place, axes-face, axes-reset.

use cc_m3d::ops::Sel;
use cc_m3d_tools::tools::ToolArg;
use serde_json::Value;

use crate::modelcraft::{ModelView, mt};

/// The IFC storeys dialog: rows (name, elevation as typed), the error shown.
#[derive(Clone, Debug)]
pub struct StoreysDlg {
    pub rows: Vec<(String, String)>,
    pub err: String,
}

impl ModelView {
    /// The solid and drawing tools' actions; None when `name` is not one of them.
    pub fn solids_act(&mut self, name: &str, arg: &str) -> Option<Result<(), String>> {
        let r = match name {
            "intersect-model" | "intersect-sel" => {
                self.intersect_sel(if name == "intersect-model" { "model" } else { "selection" });
                Ok(())
            }
            "solid-union" | "solid-subtract" | "solid-intersect" => {
                self.solid_tool(name.trim_start_matches("solid-"));
                Ok(())
            }
            "solid-subtract-pick" => {
                let (a, b) = arg.split_once(',').unwrap_or((arg, ""));
                let (Ok(cut), Ok(keep)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>()) else {
                    return Some(Err("model.edit: solid-subtract-pick:<cutter iid>,<iid>".into()));
                };
                // as the page's pick callback runs it (a Subtract's own clicks, if under way, go on)
                self.run_solid("subtract", &[keep], &[cut]);
                Ok(())
            }
            "storeys" => {
                self.storeys_dialog();
                Ok(())
            }
            "storeys-add" => {
                self.storeys_add();
                Ok(())
            }
            "storeys-guess" => {
                self.storeys_guess();
                Ok(())
            }
            "storeys-remove" => {
                if let (Ok(i), Some(StoreysDlg { rows, .. })) = (arg.trim().parse::<usize>(), self.storeys_dlg.as_mut())
                    && i < rows.len()
                {
                    rows.remove(i);
                }
                Ok(())
            }
            "storeys-save" => match serde_json::from_str::<Vec<(String, String)>>(arg) {
                Ok(rows) => {
                    self.storeys_save(rows);
                    Ok(())
                }
                Err(_) => Err("model.edit: storeys-save:[[name, elevation], …]".into()),
            },
            "storeys-ok" => {
                let rows = self.storeys_dlg.as_ref().map(|d| d.rows.clone()).unwrap_or_default();
                self.storeys_save(rows);
                Ok(())
            }
            "storeys-cancel" => {
                self.storeys_dlg = None;
                Ok(())
            }
            "axes-place" => {
                let _ = self.tools.set("axes", ToolArg::None);
                self.vcb.clear();
                Ok(())
            }
            "axes-face" => {
                self.axes_to_face();
                Ok(())
            }
            "axes-reset" => {
                if self.tools.host.model.axes_m.is_none() {
                    self.print(mt("app.axes.already", &[]));
                } else {
                    let _ = self.tools.host.model.change(&mt("m3d.undo.axes", &[]), |m| {
                        m.reset_axes();
                        Ok(())
                    });
                    self.print(mt("app.axes.reset", &[]));
                    self.after_history();
                }
                Ok(())
            }
            _ => return self.stretch_act(name, arg),
        };
        Some(r)
    }

    /// After a pointer event: Subtract's picks.
    pub fn after_pointer(&mut self) {
        self.poll_subtract();
        self.poll_light_placed();
    }

    // ---- intersect faces ----

    /// intersectSel(others): the lines where the selection cuts through faces, drawn in the open context.
    pub fn intersect_sel(&mut self, others: &str) {
        let sel: Vec<Sel> = self.tools.host.sel.iter().filter(|s| s.t == "f" || s.t == "i").cloned().collect();
        if sel.is_empty() || (others == "selection" && sel.len() < 2 && !sel.iter().any(|s| s.t == "i")) {
            self.print(mt(if others == "model" { "app.intersect.none" } else { "app.intersect.none_sel" }, &[]));
            return;
        }
        let _ = self.tools.escape();
        let path = self.tools.host.path.clone();
        let label = mt(if others == "model" { "ui.editmenu.intersect_model" } else { "ui.editmenu.intersect_sel" }, &[]);
        let n = self.tools.host.model.change(&label, |m| Ok(cc_m3d::mesh::intersect_faces(m, &path, &sel, others))).unwrap_or(0.0);
        if !cc_m3d::js::truthy(n) {
            self.print(mt("app.intersect.nothing", &[]));
            return;
        }
        self.print(mt("app.intersect.done", &[("n", &cc_m3d::js::num_str(n))]));
        self.after_history();
    }

    // ---- solids ----

    /// solidTool(op): Union and Intersect on the selected placements; Subtract asks for two clicks.
    pub fn solid_tool(&mut self, op: &str) {
        let picked: Vec<f64> = self.tools.host.sel.iter().filter(|s| s.t == "i").map(|s| s.id).collect();
        if op != "subtract" {
            if picked.len() < 2 {
                let w = mt(&format!("ui.solids.{op}"), &[]);
                self.print(mt("app.solid.need.two.sel", &[("op", &w)]));
                return;
            }
            self.run_solid(op, &picked, &[]);
            return;
        }
        // Subtract: the piece to take away, then the piece to cut
        self.tools.host.set_sel(Vec::new());
        self.sub_cutter = Some(None);
        let hint = mt("app.solid.sub.hint1", &[]);
        let arg = cc_m3d_tools::tools::PickArg { hint: Some(hint), need: usize::MAX, picked: Vec::new() };
        let _ = self.tools.set("pick", ToolArg::Pick(arg));
        self.vcb.clear();
    }

    /// Subtract's picks (the page's pick callback): the first placement clicked is the cutter, a click on another runs.
    pub fn poll_subtract(&mut self) {
        let Some(cutter) = self.sub_cutter else { return };
        let cc_m3d_tools::tools::Tool::Pick(p) = &mut self.tools.tool else {
            self.sub_cutter = None;
            return;
        };
        if p.arg.picked.is_empty() || p.arg.picked.len() == usize::from(cutter.is_some()) {
            return;
        }
        let picks: Vec<f64> = std::mem::take(&mut p.arg.picked).iter().map(|s| s.id).collect();
        let mut cutter = cutter;
        let mut run: Option<(f64, f64)> = None;
        for id in picks {
            match cutter {
                None => cutter = Some(id),
                Some(c) if c == id => {}
                Some(c) => {
                    run = Some((c, id));
                    break;
                }
            }
        }
        if let Some((c, keep)) = run {
            self.sub_cutter = None;
            let _ = self.tools.set("select", ToolArg::None);
            self.run_solid("subtract", &[keep], &[c]);
            return;
        }
        self.sub_cutter = Some(cutter);
        if let cc_m3d_tools::tools::Tool::Pick(p) = &mut self.tools.tool {
            p.arg.picked = cutter.map(|c| vec![Sel::new("i", c)]).unwrap_or_default();
            p.arg.hint = Some(mt(if cutter.is_none() { "app.solid.sub.hint1" } else { "app.solid.sub.hint2" }, &[]));
            let h = p.arg.hint.clone().unwrap_or_default();
            self.tools.host.tip(h);
        }
    }

    /// runSolid(op, keep, cut).
    pub fn run_solid(&mut self, op: &str, keep: &[f64], cut: &[f64]) {
        self.print(mt("app.solid.loading", &[]));
        let path = self.tools.host.path.clone();
        match cc_io_m3d::solids::solid_op(&mut self.tools.host.model, &path, op, keep, cut) {
            Ok(r) => {
                self.tools.host.set_sel(vec![Sel::new("i", r.iid)]);
                let v = cc_m3d::units::format_volume(r.volume, &self.tools.host.model.units);
                self.print(mt(&format!("app.solid.done.{op}"), &[("volume", &v)]));
            }
            Err(e) => self.print(e.to_string()),
        }
        self.after_history();
    }

    // ---- drawing axes ----

    /// axesToFace(): blue out of the selected face, red along its longest edge.
    pub fn axes_to_face(&mut self) {
        if !self.tools.host.path.is_empty() {
            self.print(mt("app.axes.in_group", &[]));
            return;
        }
        let ctx = self.ctx_def();
        let s = self.tools.host.sel.first().filter(|s| self.tools.host.sel.len() == 1 && s.t == "f").map(|s| s.id);
        let model = &self.tools.host.model;
        let Some((def, f)) = s.and_then(|id| model.def(&ctx).and_then(|d| d.faces.get(id).map(|f| (d, f)))) else {
            self.print(mt("app.axes.need_face", &[]));
            return;
        };
        let lp: Vec<cc_m3d::vec::P3> = f.loops.first().map(|l| l.iter().map(|&v| def.v(v)).collect()).unwrap_or_default();
        let n = lp.len();
        if n == 0 {
            return;
        }
        let at = |i: usize| lp.get(i % n).copied().unwrap_or([f64::NAN; 3]);
        let mut best = 0usize;
        for i in 1..n {
            if cc_m3d::vec::dist(at(i), at(i + 1)) > cc_m3d::vec::dist(at(best), at(best + 1)) {
                best = i;
            }
        }
        let o = at(best);
        let x = cc_m3d::vec::sub(at(best + 1), o);
        let nn = cc_m3d::def::face_normal(def, f);
        let y = cc_m3d::vec::cross(nn, x);
        match self.tools.host.model.change(&mt("m3d.undo.axes", &[]), |m| m.set_axes(o, x, y)) {
            Ok(()) => self.print(mt("app.axes.face", &[])),
            Err(e) => self.print(e.to_string()),
        }
        self.after_history();
    }

    // ---- IFC storeys ----

    /// storeysDialog(): the model's storeys as rows.
    pub fn storeys_dialog(&mut self) {
        let u = self.tools.host.model.units.clone();
        let rows = self.tools.host.model.storeys.iter().map(|s| (s.name.clone(), cc_m3d::units::format_length(s.elevation, &u))).collect();
        self.storeys_dlg = Some(StoreysDlg { rows, err: String::new() });
    }

    /// The dialog's Add storey: the next level 3 above the last row (0 with none, or when the last cannot be read).
    pub fn storeys_add(&mut self) {
        let u = self.tools.host.model.units.clone();
        let Some(StoreysDlg { rows, .. }) = self.storeys_dlg.as_mut() else { return };
        let z = rows.last().and_then(|(_, e)| cc_m3d_tools::units::parse_length(e, &u));
        let name = mt("app.storeys.level", &[("n", &(rows.len() + 1).to_string())]);
        rows.push((name, cc_m3d::units::format_length(z.map_or(0.0, |z| z + 3.0), &u)));
    }

    /// The dialog's Guess from the model: one row per height the top-level groups and components stand on (to the cm).
    pub fn storeys_guess(&mut self) {
        let model = &self.tools.host.model;
        let u = model.units.clone();
        let mut zs: Vec<f64> = Vec::new();
        if let Some(root) = model.root() {
            for inst in root.insts.values() {
                if model.def(&inst.def).is_none() {
                    continue;
                }
                if let Some(bb) = model.bbox(&inst.def, &inst.m) {
                    // a Set: -0 is stored as 0
                    let z = cc_m3d::js::round(bb.min[2] * 100.0) / 100.0 + 0.0;
                    if !zs.iter().any(|q| *q == z || (q.is_nan() && z.is_nan())) {
                        zs.push(z);
                    }
                }
            }
        }
        if zs.is_empty() {
            zs.push(0.0);
        }
        cc_m3d::js::sort_by(&mut zs, |a, b| a - b);
        let rows = zs
            .iter()
            .enumerate()
            .map(|(k, z)| (mt("app.storeys.level", &[("n", &(k + 1).to_string())]), cc_m3d::units::format_length(*z, &u)))
            .collect();
        if let Some(d) = self.storeys_dlg.as_mut() {
            d.rows = rows;
        } else {
            self.storeys_dlg = Some(StoreysDlg { rows, err: String::new() });
        }
    }

    /// The dialog's Save: each row needs a name and an elevation; no two at one elevation.
    pub fn storeys_save(&mut self, rows: Vec<(String, String)>) {
        let u = self.tools.host.model.units.clone();
        let parsed: Vec<(String, Option<f64>)> =
            rows.iter().map(|(n, e)| (cc_m3d::js::trim(n).to_string(), cc_m3d_tools::units::parse_length(e, &u))).collect();
        let fail = |v: &mut ModelView, key: &str| {
            v.storeys_dlg = Some(StoreysDlg { rows: rows.clone(), err: mt(key, &[]) });
        };
        if parsed.iter().any(|(n, e)| n.is_empty() || e.is_none()) {
            fail(self, "app.storeys.bad");
            return;
        }
        let list = Value::Array(
            parsed
                .iter()
                .map(|(n, e)| {
                    let mut o = serde_json::Map::new();
                    o.insert("name".into(), Value::String(n.clone()));
                    o.insert("elevation".into(), serde_json::Number::from_f64(e.unwrap_or(0.0)).map_or(Value::Null, Value::Number));
                    Value::Object(o)
                })
                .collect(),
        );
        let clean = cc_m3d::file::clean_storeys(Some(&list));
        if clean.len() != parsed.len() {
            fail(self, "app.storeys.same");
            return;
        }
        self.storeys_dlg = None;
        let n = clean.len();
        let _ = self.tools.host.model.change(&mt("app.undo.storeys", &[]), |m| {
            m.storeys = clean;
            Ok(())
        });
        self.print(if n > 0 { mt("app.storeys.saved", &[("n", &n.to_string())]) } else { mt("app.storeys.none", &[]) });
    }

    /// The dialogs after the Edit menu's: IFC storeys, then the stretch dialog.
    pub fn more_dialogs(&mut self, ctx: &egui::Context, act: &mut Option<String>) {
        if let Some(StoreysDlg { mut rows, err }) = self.storeys_dlg.clone() {
            let mut a: Option<String> = None;
            egui::Window::new(mt("app.storeys.h", &[]))
                .min_width(380.0)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new(mt("app.storeys.note", &[])).small());
                    egui::Grid::new("mc-st").num_columns(3).show(ui, |ui| {
                        ui.label(mt("app.storeys.name", &[]));
                        ui.label(mt("app.storeys.elevation", &[]));
                        ui.label("");
                        ui.end_row();
                        for (i, (n, e)) in rows.iter_mut().enumerate() {
                            ui.add_sized([150.0, 20.0], egui::TextEdit::singleline(n).char_limit(80));
                            ui.add_sized([90.0, 20.0], egui::TextEdit::singleline(e));
                            if ui.small_button("×").on_hover_text(mt("app.storeys.remove", &[])).clicked() {
                                a = Some(format!("storeys-remove:{i}"));
                            }
                            ui.end_row();
                        }
                    });
                    if !err.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(0xd0, 0x22, 0x3e), &err);
                    }
                    ui.horizontal(|ui| {
                        if ui.link(mt("app.storeys.add", &[])).clicked() {
                            a = Some("storeys-add".into());
                        }
                        if ui.link(mt("app.storeys.guess", &[])).clicked() {
                            a = Some("storeys-guess".into());
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button(mt("app.btn.cancel", &[])).clicked() {
                            a = Some("storeys-cancel".into());
                        }
                        if ui.button(mt("app.btn.save", &[])).clicked() {
                            a = Some("storeys-ok".into());
                        }
                    });
                });
            // the rows as typed stay in the dialog (the buttons act on them)
            self.storeys_dlg = Some(StoreysDlg { rows, err });
            if a.is_some() {
                *act = a;
            }
        }
        self.stretch_dialogs(ctx, act);
    }
}

#[cfg(test)]
mod tests {
    use crate::mcact::ns;
    use crate::mcedit::testkit::{groups, root_insts};

    #[test]
    fn solids_union_subtract_intersect() {
        let mut v = groups(&[0.0, 0.5]);
        let ids = root_insts(&v);
        v.act("solid-union").unwrap();
        assert!(v.message.starts_with("Union: one solid of 1.5"), "{}", v.message);
        assert_eq!(root_insts(&v).len(), 1);
        v.act("undo").unwrap();
        v.act(&format!("select:i:{},i:{}", ns(ids[0]), ns(ids[1]))).unwrap();
        v.act("solid-intersect").unwrap();
        assert!(v.message.starts_with("Intersect: the shared part, 0.5"), "{}", v.message);
        v.act("undo").unwrap();
        v.act(&format!("solid-subtract-pick:{},{}", ns(ids[1]), ns(ids[0]))).unwrap();
        assert!(v.message.starts_with("Subtracted: a solid of 0.5"), "{}", v.message);
        v.act("undo").unwrap();
        v.act("select-none").unwrap();
        v.act("solid-union").unwrap();
        assert_eq!(v.message, "Union: select two or more closed groups or components first.");
        // Subtract by two clicks: the tool asks for the piece to take away
        v.act("solid-subtract").unwrap();
        assert_eq!(v.tools.name, "pick");
        assert!(v.sub_cutter.is_some());
    }

    #[test]
    fn storeys_dialog() {
        let mut v = groups(&[0.0, 3.0]);
        v.act("storeys").unwrap();
        v.act("storeys-add").unwrap();
        v.act("storeys-add").unwrap();
        assert_eq!(
            v.storeys_dlg.as_ref().map(|d| d.rows.clone()).unwrap_or_default(),
            vec![("Level 1".to_string(), "0 m".to_string()), ("Level 2".to_string(), "3 m".to_string())]
        );
        v.act("storeys-ok").unwrap();
        assert_eq!(v.message, "2 storeys saved; IFC export puts each group on the storey at or below it.");
        assert_eq!(v.tools.host.model.storeys.len(), 2);
        v.act(r#"storeys-save:[["A","0"],["B","0"]]"#).unwrap();
        assert_eq!(v.storeys_dlg.as_ref().map(|d| d.err.as_str()), Some("Two storeys are at the same elevation."));
        v.act(r#"storeys-save:[["A","x"]]"#).unwrap();
        assert!(v.storeys_dlg.as_ref().is_some_and(|d| d.err.starts_with("Each storey needs")));
        v.act("storeys-save:[]").unwrap();
        assert_eq!(v.message, "No storeys: IFC export puts everything on one storey at 0.");
        v.act("storeys-guess").unwrap();
        assert_eq!(v.storeys_dlg.as_ref().map(|d| d.rows.len()), Some(1));
    }

    #[test]
    fn axes_and_intersect() {
        let mut v = groups(&[0.0, 0.5]);
        v.act("axes-reset").unwrap();
        assert_eq!(v.message, "The drawing axes are already the model’s own.");
        v.act("axes-face").unwrap();
        assert_eq!(v.message, "Select one face to align the axes to.");
        let iid = root_insts(&v)[0];
        v.act(&format!("open:{}", ns(iid))).unwrap();
        v.act("axes-face").unwrap();
        assert_eq!(v.message, "Close the group first: inside a group, its own axes are used.");
        v.act("close").unwrap();
        v.act("select-all").unwrap();
        v.act("intersect-sel").unwrap();
        assert!(v.message.starts_with("Intersected:"), "{}", v.message);
        v.act("axes-place").unwrap();
        assert_eq!(v.tools.name, "axes");
    }
}
