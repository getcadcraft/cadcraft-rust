//! ModelCraft's components that stretch (0.4 M26) as modelcraft.js does them: inside an open component, a part's rule
//! per axis (auto, stretch, start, end, move, scale, repeat) and a repeated part's spacing; Entity info's Stretch this
//! copy… dialog (width, depth, height: a new stretched definition for that copy only, through cc-m3d-tools'
//! dynamic.js port). One undo step each, 0.5's undo labels and status words (`forks/tools/mcuidiff.mjs --edit`).
//!
//! Actions (`model.edit=<action>`): rule:<iid>:<x|y|z>=<rule>, rule-spacing:<iid>:<x|y|z>=<length>, stretch:<iid>
//! (opens the dialog), stretch-go:<iid>:[width, depth, height] (its Stretch, as typed), stretch-cancel.

use crate::modelcraft::{ModelView, mt};

/// The part rules (modelcraft.js RULES).
pub const RULES: [&str; 7] = ["auto", "stretch", "start", "end", "move", "scale", "repeat"];

/// The stretch dialog: the placement, the source's name, width / depth / height as typed, the error shown.
#[derive(Clone, Debug)]
pub struct StretchDlg {
    pub iid: f64,
    pub name: String,
    pub size: [String; 3],
    pub err: String,
}

impl ModelView {
    /// The stretch actions; None when `name` is not one of them.
    pub fn stretch_act(&mut self, name: &str, arg: &str) -> Option<Result<(), String>> {
        let r = match name {
            "rule" | "rule-spacing" => {
                let bad = || format!("model.edit: {name}:<iid>:<x|y|z>=<value>");
                let Some((iid, rest)) = arg.split_once(':') else { return Some(Err(bad())) };
                let Some((ax, v)) = rest.split_once('=') else { return Some(Err(bad())) };
                let Ok(iid) = iid.trim().parse::<f64>() else { return Some(Err(bad())) };
                if !matches!(ax, "x" | "y" | "z") {
                    return Some(Err(bad()));
                }
                if name == "rule" {
                    if !RULES.contains(&v) {
                        return Some(Err(format!("model.edit: rule: unknown rule {v}")));
                    }
                    self.set_rule(iid, ax, v);
                } else {
                    self.set_rule_spacing(iid, ax, v);
                }
                Ok(())
            }
            "stretch" => {
                let Ok(iid) = arg.trim().parse::<f64>() else { return Some(Err("model.edit: stretch:<iid>".into())) };
                self.stretch_dialog(iid);
                Ok(())
            }
            "stretch-go" => {
                let bad = || "model.edit: stretch-go:<iid>:[width, depth, height]".to_string();
                let Some((iid, j)) = arg.split_once(':') else { return Some(Err(bad())) };
                let (Ok(iid), Ok(size)) = (iid.trim().parse::<f64>(), serde_json::from_str::<[String; 3]>(j)) else {
                    return Some(Err(bad()));
                };
                self.stretch_go(iid, size);
                Ok(())
            }
            "stretch-cancel" => {
                self.stretch_dlg = None;
                Ok(())
            }
            _ => return None,
        };
        Some(r)
    }

    // ---- components that stretch ----

    /// A part's rule along one axis (partRules' select): auto takes the key away; no keys left, no rule.
    pub fn set_rule(&mut self, iid: f64, ax: &str, v: &str) {
        let comp = self.ctx_def();
        if self.tools.host.model.def(&comp).and_then(|d| d.insts.get(iid)).is_none() {
            return;
        }
        let ax = match ax {
            "x" => "x",
            "y" => "y",
            _ => "z",
        };
        let _ = self.tools.host.model.change(&mt("app.undo.rule", &[]), |m| {
            let Some(i) = m.def_mut(&comp).and_then(|d| d.insts.get_mut(iid)) else { return Ok(()) };
            let mut r = i.rule.take().unwrap_or_default();
            r.set(ax, Some(v.to_string()), None);
            if v == "auto" {
                r.set(ax, None, None);
            }
            i.rule = if r.keys().is_empty() { None } else { Some(r) };
            Ok(())
        });
    }

    /// A repeated part's spacing along one axis (partRules' Every box).
    pub fn set_rule_spacing(&mut self, iid: f64, ax: &str, text: &str) {
        let comp = self.ctx_def();
        let u = self.tools.host.model.units.clone();
        let Some(v) = cc_m3d_tools::units::parse_length(text, &u).filter(|v| *v > 0.0) else {
            self.print(mt("app.len.bad", &[]));
            return;
        };
        let key = match ax {
            "x" => "sx",
            "y" => "sy",
            _ => "sz",
        };
        let _ = self.tools.host.model.change(&mt("app.undo.rule", &[]), |m| {
            let Some(i) = m.def_mut(&comp).and_then(|d| d.insts.get_mut(iid)) else { return Ok(()) };
            // the box shows only for a repeated part, so the rule is there
            let Some(r) = i.rule.as_mut() else { return Err(cc_m3d::Error::Type) };
            r.set(key, None, Some(v));
            Ok(())
        });
    }

    /// stretchDialog(iid): the copy's width, depth and height now.
    pub fn stretch_dialog(&mut self, iid: f64) {
        let model = &self.tools.host.model;
        let ctx = self.ctx_def();
        let Some(inst) = model.def(&ctx).and_then(|d| d.insts.get(iid)) else { return };
        let Some(cur) = model.def(&inst.def) else { return };
        let src = cur.dyn_.as_ref().and_then(|d| model.def(&d.src)).unwrap_or(cur);
        let Some(bb) = model.bbox(&cur.id, &cc_m3d::vec::IDENT) else { return };
        let u = model.units.clone();
        let f = |k: usize| cc_m3d::units::format_length(bb.max.get(k).copied().unwrap_or(0.0) - bb.min.get(k).copied().unwrap_or(0.0), &u);
        self.stretch_dlg = Some(StretchDlg { iid, name: src.name.clone(), size: [f(0), f(1), f(2)], err: String::new() });
    }

    /// The dialog's Stretch: a new stretched definition for this copy only.
    pub fn stretch_go(&mut self, iid: f64, size: [String; 3]) {
        let model = &self.tools.host.model;
        let ctx = self.ctx_def();
        let Some(inst) = model.def(&ctx).and_then(|d| d.insts.get(iid)) else { return };
        let Some(cur) = model.def(&inst.def) else { return };
        let src = cur.dyn_.as_ref().and_then(|d| model.def(&d.src)).unwrap_or(cur);
        let (src_id, src_name) = (src.id.clone(), src.name.clone());
        let u = model.units.clone();
        let s: Vec<Option<f64>> = size.iter().map(|x| cc_m3d_tools::units::parse_length(x, &u)).collect();
        let (Some(Some(w)), Some(Some(d)), Some(Some(h))) = (s.first(), s.get(1), s.get(2)) else {
            let name = src_name.clone();
            self.stretch_dlg = Some(StretchDlg { iid, name, size, err: mt("app.dyn.bad", &[]) });
            return;
        };
        let sz = [*w, *d, *h];
        if sz.iter().any(|v| v.is_nan() || *v < 0.0) {
            self.stretch_dlg = Some(StretchDlg { iid, name: src_name, size, err: mt("app.dyn.bad", &[]) });
            return;
        }
        self.stretch_dlg = None;
        let path = self.tools.host.path.clone();
        let base = strip_copy_number(&src_name);
        let r = self.tools.host.model.change(&mt("app.undo.stretch", &[]), |m| {
            let nm = m.unique_name(&base);
            let nd = cc_m3d_tools::dynamic::stretch_def(m, &src_id, sz, &nm)?;
            let def = m.context(&path).map(|c| c.def).ok_or(cc_m3d::Error::Type)?;
            if let Some(i) = m.def_mut(&def).and_then(|d| d.insts.get_mut(iid)) {
                i.def = nd;
            }
            m.purge();
            Ok(())
        });
        match r {
            Ok(()) => {
                let txt: Vec<String> = sz.iter().map(|v| cc_m3d::units::format_length(*v, &u)).collect();
                self.print(mt("app.dyn.done", &[("name", &src_name), ("size", &txt.join(" × "))]));
            }
            Err(e) => self.print(e.to_string()),
        }
        self.after_history();
    }
    /// The stretch dialog.
    pub fn stretch_dialogs(&mut self, ctx: &egui::Context, act: &mut Option<String>) {
        if let Some(StretchDlg { iid, name, mut size, err }) = self.stretch_dlg.clone() {
            let mut a: Option<String> = None;
            egui::Window::new(mt("app.dyn.h", &[("name", &name)]))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Grid::new("mc-dy").num_columns(2).show(ui, |ui| {
                        for (k, key) in ["app.info.width", "app.info.depth", "app.info.height"].iter().enumerate() {
                            let l = ui.label(mt(key, &[]));
                            if let Some(v) = size.get_mut(k) {
                                ui.text_edit_singleline(v).labelled_by(l.id);
                            }
                            ui.end_row();
                        }
                    });
                    ui.label(egui::RichText::new(mt("app.dyn.note", &[])).small());
                    if !err.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(0xd0, 0x22, 0x3e), &err);
                    }
                    ui.horizontal(|ui| {
                        if ui.button(mt("app.btn.cancel", &[])).clicked() {
                            a = Some("stretch-cancel".into());
                        }
                        if ui.button(mt("app.dyn.go", &[])).clicked() {
                            a = Some(format!("stretch-go:{}:{}", crate::mcact::ns(iid), serde_json::json!(size)));
                        }
                    });
                });
            if a.is_none() {
                self.stretch_dlg = Some(StretchDlg { iid, name, size, err });
            }
            if a.is_some() {
                *act = a;
            }
        }
    }
}

/// `name.replace(/#\d+$/, '')`.
fn strip_copy_number(name: &str) -> String {
    let digits = name.chars().rev().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return name.to_string();
    }
    let cut = name.len() - digits;
    match name.get(..cut).and_then(|r| r.strip_suffix('#')) {
        Some(rest) => rest.to_string(),
        None => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use crate::mcact::ns;
    use crate::mcedit::testkit::{groups, root_insts};

    #[test]
    fn stretch_and_rules() {
        // a component made of two groups
        let mut v = groups(&[0.0, 2.0]);
        v.act("make-component").unwrap();
        let iid = root_insts(&v)[0];
        let comp = v.tools.host.model.root().unwrap().insts.get(iid).unwrap().def.clone();
        let parts = v.tools.host.model.def(&comp).unwrap().insts.key_vec();
        v.act(&format!("stretch:{}", ns(iid))).unwrap();
        assert_eq!(v.stretch_dlg.as_ref().map(|d| d.size.clone()), Some(["3 m".to_string(), "1 m".to_string(), "1 m".to_string()]));
        v.act(&format!(r#"stretch-go:{}:["x","1","1"]"#, ns(iid))).unwrap();
        assert_eq!(v.stretch_dlg.as_ref().map(|d| d.err.as_str()), Some("Type a width, depth and height of zero or more."));
        v.act(&format!(r#"stretch-go:{}:["6","1","2"]"#, ns(iid))).unwrap();
        assert!(v.stretch_dlg.is_none());
        assert!(v.message.starts_with("Stretched this copy of Component#1 to 6 m × 1 m × 2 m"), "{}", v.message);
        let now = v.tools.host.model.root().unwrap().insts.get(iid).unwrap().def.clone();
        assert_ne!(now, comp);
        assert!(v.tools.host.model.def(&now).unwrap().dyn_.is_some());
        v.act("undo").unwrap();
        // part rules inside the component: kept in the order they are set (the file writes them so)
        v.act(&format!("open:{}", ns(iid))).unwrap();
        let p = parts[0];
        v.act(&format!("rule:{}:z=start", ns(p))).unwrap();
        v.act(&format!("rule:{}:x=repeat", ns(p))).unwrap();
        v.act(&format!("rule-spacing:{}:x=0.5", ns(p))).unwrap();
        v.act(&format!("rule-spacing:{}:x=0", ns(p))).unwrap();
        assert_eq!(v.message, "Type a length above zero, such as 3m or 10'6\".");
        let r = v.tools.host.model.def(&comp).unwrap().insts.get(p).unwrap().rule.clone().unwrap();
        assert_eq!(r.keys(), vec!["z", "x", "sx"]);
        let text = v.save_text();
        assert!(text.contains(r#""rule":{"z":"start","x":"repeat","sx":0.5}"#), "{text}");
        v.act(&format!("rule:{}:z=auto", ns(p))).unwrap();
        v.act(&format!("rule:{}:x=auto", ns(p))).unwrap();
        let i = v.tools.host.model.def(&comp).unwrap().insts.get(p).unwrap().clone();
        assert_eq!(i.rule.map(|r| r.keys()), Some(vec!["sx"]), "the spacing stays, as in the JS");
        assert!(v.act(&format!("rule:{}:x=wobble", ns(p))).is_err());
    }
}
