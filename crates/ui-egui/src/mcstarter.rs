//! ModelCraft's starter library (0.4 M25, the entourage 0.5 RC.11 (e)) as modelcraft.js does it: the Components
//! panel's Starter library… opens a dialog of about thirty everyday things and fifteen entourage items by category;
//! one picked is built once per model (cc-m3d-tools' starter.js / entourage.js port, a component remembering which
//! thing it is) and then placed with the Place tool on the face clicked (wall things stand in the wall). One undo step,
//! 0.5's labels and status words (`forks/tools/mcuidiff.mjs --starter`).
//!
//! Actions (`model.edit=<action>`): starter (opens the dialog), starter-place:<id>, starter-cancel.

use cc_m3d_tools::tools::ToolArg;

use crate::modelcraft::{ModelView, mt};

impl ModelView {
    /// The starter library actions; None when `name` is not one of them.
    pub fn starter_act(&mut self, name: &str, arg: &str) -> Option<Result<(), String>> {
        match name {
            "starter" => self.starter_dlg = true,
            "starter-cancel" => self.starter_dlg = false,
            "starter-place" => {
                self.starter_dlg = false;
                self.place_starter(arg.trim());
            }
            _ => return self.keys_act(name, arg),
        }
        Some(Ok(()))
    }

    /// placeStarter(id): the thing's component (built once per model), then the Place tool on the face clicked.
    pub fn place_starter(&mut self, id: &str) {
        let Some(it) = cc_m3d_tools::starter::starter_list().into_iter().find(|s| s.0 == id) else { return };
        let name = mt(&format!("app.starter.{id}"), &[]);
        let found = self
            .tools
            .host
            .model
            .defs
            .iter()
            .find(|(_, d)| d.kind == "component" && d.starter.as_deref() == Some(id))
            .map(|(k, _)| k.clone());
        let def_id = match found {
            Some(d) => d,
            None => {
                let r = self.tools.host.model.change(&mt("app.undo.starter", &[]), |m| {
                    let d = cc_m3d_tools::starter::build_starter(m, id, Some(&name))?;
                    if let Some(dd) = d.as_ref().and_then(|d| m.defs.get_mut(d)) {
                        dd.starter = Some(id.to_string());
                    }
                    Ok(d)
                });
                match r {
                    Ok(Some(d)) => d,
                    Ok(None) => return,
                    Err(e) => {
                        self.print(e.to_string());
                        return;
                    }
                }
            }
        };
        self.after_history();
        let hint = mt(if it.2 { "app.starter.where_wall" } else { "app.starter.where" }, &[("name", &name)]);
        let _ = self.tools.set("place", ToolArg::Place { def_id, align: true, hint: Some(hint.clone()) });
        self.vcb.clear();
        self.print(hint);
    }

    /// The Starter library dialog.
    pub fn starter_dialog(&mut self, ctx: &egui::Context, act: &mut Option<String>) {
        if self.starter_dlg {
            let mut a: Option<String> = None;
            let list = cc_m3d_tools::starter::starter_list();
            let mut cats: Vec<&str> = Vec::new();
            for s in &list {
                if !cats.contains(&s.1) {
                    cats.push(s.1);
                }
            }
            crate::shell::Dialog::new(mt("app.starter.h", &[]))
                .collapsible(false)
                .resizable(false)
                .default_width(560.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new(mt("app.starter.note", &[])).small());
                    egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.7).show(ui, |ui| {
                        for c in &cats {
                            ui.add_space(4.0);
                            ui.strong(mt(&format!("app.starter.cat.{c}"), &[]));
                            ui.horizontal_wrapped(|ui| {
                                for s in list.iter().filter(|s| s.1 == *c) {
                                    if ui.add(crate::shell::Btn::new(mt(&format!("app.starter.{}", s.0), &[]))).clicked() {
                                        a = Some(format!("starter-place:{}", s.0));
                                    }
                                }
                            });
                        }
                    });
                    ui.separator();
                    if ui.add(crate::shell::Btn::new(mt("app.btn.cancel", &[]))).clicked() {
                        a = Some("starter-cancel".into());
                    }
                });
            if a.is_some() {
                *act = a;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::modelcraft::ModelView;

    fn empty() -> ModelView {
        ModelView::new(cc_m3d::model::Model::new(), "t", false).unwrap()
    }

    #[test]
    fn a_starter_thing_is_built_once() {
        let mut v = empty();
        v.act("starter-place:table").unwrap();
        assert_eq!(v.tools.name, "place");
        assert_eq!(v.message, "Click where the Dining table goes: on a floor or table it stands on it.");
        let n = v.tools.host.model.defs.values().filter(|d| d.starter.as_deref() == Some("table")).count();
        v.act("starter-place:table").unwrap();
        let n2 = v.tools.host.model.defs.values().filter(|d| d.starter.as_deref() == Some("table")).count();
        assert_eq!((n, n2), (1, 1));
        v.act("starter-place:nothing").unwrap();
    }
}
