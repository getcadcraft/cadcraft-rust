//! ModelCraft's Edit menu and its dialogs as modelcraft.js does them: the clipboard (Copy, Cut, Paste — the copy
//! follows the pointer, a click puts it down — Paste in place), Duplicate and move, Flip and Flipped copy along red /
//! green / blue, and 3D text (its dialog, then the text as a component placed on the face clicked; text3d.js through
//! cc-io-m3d). Each edit is one undo step through the cc-m3d functions modelcraft.js calls, with its undo labels and
//! status words, so saved files match 0.5's (`forks/tools/mcuidiff.mjs --edit`).
//!
//! Everything is `model.edit=<action>` ([`ModelView::act`]): copy, cut, paste, paste-in-place, duplicate, flip-x … z,
//! flipcopy-x … z, text3d (opens the dialog), text3d-place:{"text","height","depth"} (the dialog's Place, as typed),
//! vcb:<text> (the value box, as typed), click:<x>,<y>, move:<x>,<y>, down / up (the pointer in the view, in view
//! pixels), view-size:<w>,<h> (pin the camera's size, for harnesses; empty unpins), probe-view (the view's size).
//!
//! The clipboard is kept in memory while the app runs (0.5 also keeps it in the browser's storage so another tab can
//! paste it).

use cc_m3d::ops::Sel;
use cc_m3d::vec::apply;
use cc_m3d_tools::tools::{Ev, ToolArg};
use serde_json::Value;

use crate::modelcraft::{ModelView, mt};

/// The 3D text dialog: text, letter height, depth (as typed), the error shown.
#[derive(Clone, Debug)]
pub struct Text3dDlg {
    pub text: String,
    pub height: String,
    pub depth: String,
    pub err: String,
}

/// What 3D text was placed last (textLast): the dialog starts from it.
#[derive(Clone, Debug, Default)]
pub struct TextLast {
    pub text: String,
    pub height: Option<f64>,
    pub depth: f64,
}

fn geo(s: &Sel) -> bool {
    !matches!(s.t.as_str(), "g" | "d" | "s")
}

impl ModelView {
    /// `cursorEv()`: the pointer where it last was over the view, else the view's middle.
    pub fn cursor_ev(&self) -> Ev {
        let (x, y) = self.mouse.unwrap_or((self.tools.host.cam.w / 2.0, self.tools.host.cam.h / 2.0));
        Ev { x, y, shift: false, ctrl: false, alt: false, ..Ev::default() }
    }

    /// The Edit menu's actions; None when `name` is not one of them.
    pub fn edit_act(&mut self, name: &str, arg: &str) -> Option<Result<(), String>> {
        let r = match name {
            "copy" => {
                self.copy_selection();
                Ok(())
            }
            "cut" => {
                if self.copy_selection() {
                    self.erase_selection();
                    self.print(mt("app.cut", &[]));
                }
                Ok(())
            }
            "paste" => {
                self.start_paste(false);
                Ok(())
            }
            "paste-in-place" => {
                self.start_paste(true);
                Ok(())
            }
            "duplicate" => {
                self.duplicate();
                Ok(())
            }
            "flip-x" | "flip-y" | "flip-z" | "flipcopy-x" | "flipcopy-y" | "flipcopy-z" => {
                let axis = name.chars().last().unwrap_or('x');
                self.flip(axis, name.starts_with("flipcopy"));
                Ok(())
            }
            "text3d" => {
                self.text3d_dialog();
                Ok(())
            }
            "text3d-place" => self.text3d_submit(arg),
            "text3d-cancel" => {
                self.text_dlg = None;
                Ok(())
            }
            "vcb" => {
                let _ = self.tools.value(arg);
                Ok(())
            }
            "click" | "move" | "down" | "up" => {
                let (x, y) = arg.split_once(',').unwrap_or((arg, ""));
                let (Ok(x), Ok(y)) = (x.trim().parse::<f64>(), y.trim().parse::<f64>()) else {
                    return Some(Err(format!("model.edit: {name}:<x>,<y>")));
                };
                if !(x.is_finite() && y.is_finite()) {
                    return Some(Err(format!("model.edit: {name}:<x>,<y>")));
                }
                self.mouse = Some((x, y));
                // clicks a second apart (no double clicks), as the harness gives the page's tools
                let time = if matches!(name, "click" | "down") {
                    self.click_clock += 1000.0;
                    Some(self.click_clock)
                } else {
                    None
                };
                let e = Ev { x, y, time, ..Ev::default() };
                if matches!(name, "click" | "down") {
                    self.place_guard();
                }
                let _ = match name {
                    "click" => self.tools.click(&e),
                    "down" => self.tools.down(&e),
                    "up" => self.tools.up(&e),
                    _ => self.tools.mv(&e),
                };
                self.take_tool_message();
                self.sync_ctx();
                self.after_pointer();
                Ok(())
            }
            "view-size" => {
                // pin the camera's size (harnesses replay pointer events in a view of a known size); empty unpins
                self.pinned_size = match arg.split_once(',') {
                    Some((w, h)) => match (w.trim().parse::<f64>(), h.trim().parse::<f64>()) {
                        (Ok(w), Ok(h)) if w >= 1.0 && h >= 1.0 && w <= 16384.0 && h <= 16384.0 => Some((w, h)),
                        _ => return Some(Err("model.edit: view-size:<w>,<h>".into())),
                    },
                    None => None,
                };
                if let Some((w, h)) = self.pinned_size {
                    self.tools.host.cam.set_size(w, h);
                }
                Ok(())
            }
            "probe-view" => {
                let c = &self.tools.host.cam;
                self.print(format!("{},{}", c.w, c.h));
                Ok(())
            }
            _ => return self.solids_act(name, arg),
        };
        Some(r)
    }

    /// Before a press reaches the Place tool: when what it places is gone (a purge dropped the unplaced 3D text or
    /// component meanwhile), back to Select. 0.5 places it anyway, and its file then names a missing definition (it
    /// cannot be opened again and Undo throws).
    pub fn place_guard(&mut self) {
        if let cc_m3d_tools::tools::Tool::Place(p) = &self.tools.tool
            && self.tools.host.model.def(p.def_id()).is_none()
        {
            let _ = self.tools.set("select", ToolArg::None);
        }
    }

    /// The tools' newest message becomes the status (as the page's host.print does at once).
    pub fn take_tool_message(&mut self) {
        if let Some(m) = self.tools.host.msgs.pop() {
            self.message = m;
            self.tools.host.msgs.clear();
        }
    }
    // ---- clipboard ----

    /// copySelection(): the selection to the clipboard. → whether something was copied.
    pub fn copy_selection(&mut self) -> bool {
        let _ = self.tools.escape();
        let sel: Vec<Sel> = self.tools.host.sel.iter().filter(|s| geo(s)).cloned().collect();
        if sel.is_empty() {
            self.print(mt("app.copy.none", &[]));
            return false;
        }
        let path = self.tools.host.path.clone();
        let c = match self.tools.host.model.copy_clip(&path, &sel) {
            Ok(Some(c)) => c,
            Ok(None) => {
                self.print(mt("app.copy.empty", &[]));
                return false;
            }
            Err(e) => {
                self.print(e.to_string());
                return false;
            }
        };
        self.clip = Some(c);
        self.print(mt("app.copy.done", &[("n", &sel.len().to_string())]));
        true
    }

    /// startPaste(inPlace): in place at once, else the Paste tool (the copy follows the pointer).
    pub fn start_paste(&mut self, in_place: bool) {
        let _ = self.tools.escape();
        let Some(c) = self.clip.clone() else {
            self.print(mt("app.paste.none", &[]));
            return;
        };
        if in_place {
            let path = self.tools.host.path.clone();
            let m = c.get("m").cloned().unwrap_or(Value::Null);
            let made = match self.tools.host.model.change(&mt("ui.editmenu.paste_in_place", &[]), |mm| mm.paste_clip(&path, &c, &m)) {
                Ok(v) => v,
                Err(e) => {
                    self.print(mt("app.paste.failed", &[("error", &e.to_string())]));
                    return;
                }
            };
            let _ = self.tools.set("select", ToolArg::None);
            self.tools.host.set_sel(made);
            self.print(mt("app.paste.inplace", &[]));
            return;
        }
        let _ = self.tools.set("paste", ToolArg::Paste(c));
        self.vcb.clear();
        self.print(mt("app.paste.where", &[]));
    }

    /// duplicate(): copy the selection and move the copy from the pointer at once.
    pub fn duplicate(&mut self) {
        let _ = self.tools.escape();
        if !self.tools.host.sel.iter().any(geo) {
            self.print(mt("app.dup.none", &[]));
            return;
        }
        let e = self.cursor_ev();
        let _ = self.tools.grab(&e, true);
        self.vcb.clear();
        self.print(mt("app.dup.done", &[]));
    }

    // ---- flip ----

    /// flip(axis, copy): mirror the selection through the middle of its box, square to red, green or blue (the open
    /// group's own); a flipped copy lands beside the original, mirrored across the far side of the box.
    pub fn flip(&mut self, axis: char, copy: bool) {
        let sel: Vec<Sel> = self.tools.host.sel.iter().filter(|x| matches!(x.t.as_str(), "e" | "f" | "i")).cloned().collect();
        if sel.is_empty() {
            self.print(mt("app.flip.none", &[]));
            return;
        }
        let _ = self.tools.escape();
        let path = self.tools.host.path.clone();
        let Some(ctx) = self.tools.host.model.context(&path) else { return };
        let Some(c) = self.tools.sel_center() else { return };
        let k = match axis {
            'y' => 1,
            'z' => 2,
            _ => 0,
        };
        let mut n = [0.0; 3];
        if let Some(v) = n.get_mut(k) {
            *v = 1.0;
        }
        let mut at = c;
        if copy && let Some((lo, hi)) = self.tools.sel_box() {
            let ax = self.tools.axes();
            let a = match k {
                1 => ax.y,
                2 => ax.z,
                _ => ax.x,
            };
            let mut far = 0.0f64;
            for x in [lo[0], hi[0]] {
                for y in [lo[1], hi[1]] {
                    for z in [lo[2], hi[2]] {
                        far = cc_m3d::js::max(far, (x - c[0]) * a[0] + (y - c[1]) * a[1] + (z - c[2]) * a[2]);
                    }
                }
            }
            at = [c[0] + a[0] * far, c[1] + a[1] * far, c[2] + a[2] * far];
        }
        let local = apply(&ctx.inv, at);
        let label = mt(if copy { "app.undo.flipcopy" } else { "app.undo.flip" }, &[]);
        let out = self.tools.host.model.change(&label, |m| {
            let d = m.def_mut(&ctx.def).ok_or(cc_m3d::Error::Type)?;
            Ok(cc_m3d::ops::flip_sel(d, &sel, local, n, copy))
        });
        match out {
            Ok(out) => {
                self.tools.host.set_sel(out);
                let key = format!("app.flip.{}.{axis}", if copy { "copy" } else { "done" });
                self.print(mt(&key, &[]));
            }
            Err(e) => self.print(e.to_string()),
        }
    }

    // ---- 3D text ----

    /// text3dDialog(): the dialog, filled from the last text placed.
    pub fn text3d_dialog(&mut self) {
        let u = self.tools.host.model.units.clone();
        let h0 = self.text_last.height.unwrap_or(if u == "ft-in" || u == "in" { 0.3048 } else { 0.3 });
        self.text_dlg = Some(Text3dDlg {
            text: self.text_last.text.clone(),
            height: cc_m3d::units::format_length(h0, &u),
            depth: cc_m3d::units::format_length(self.text_last.depth, &u),
            err: String::new(),
        });
    }

    /// The dialog's Place: `{"text", "height", "depth"}` as typed (the JSON form scripts use).
    fn text3d_submit(&mut self, arg: &str) -> Result<(), String> {
        let j: Value = serde_json::from_str(arg).map_err(|_| "model.edit: text3d-place:{\"text\",\"height\",\"depth\"}".to_string())?;
        let s = |k: &str| j.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let (text, height, depth) = (s("text"), s("height"), s("depth"));
        let u = self.tools.host.model.units.clone();
        let text = text.trim_end_matches(cc_m3d::js::is_ws).to_string();
        let h = cc_m3d_tools::units::parse_length(&height, &u);
        let d = if depth.trim_matches(cc_m3d::js::is_ws).is_empty() { Some(0.0) } else { cc_m3d_tools::units::parse_length(&depth, &u) };
        let bad = |v: &mut ModelView, key: &str| {
            v.text_dlg = Some(Text3dDlg { text: text.clone(), height: height.clone(), depth: depth.clone(), err: mt(key, &[]) });
        };
        if text.trim_matches(cc_m3d::js::is_ws).is_empty() {
            bad(self, "app.text3d.empty");
            return Ok(());
        }
        let Some(h) = h.filter(|h| *h > 0.0) else {
            bad(self, "app.text3d.badh");
            return Ok(());
        };
        let Some(d) = d.filter(|d| *d >= 0.0) else {
            bad(self, "app.text3d.badd");
            return Ok(());
        };
        self.text_dlg = None;
        self.place_text(text, h, d);
        Ok(())
    }

    /// placeText(o): the text as a component, then the Place tool (it lies on the face clicked).
    pub fn place_text(&mut self, text: String, height: f64, depth: f64) {
        self.text_last = TextLast { text: text.clone(), height: Some(height), depth };
        self.print(mt("app.text3d.loading", &[]));
        let r =
            self.tools.host.model.change(&mt("app.undo.text3d", &[]), |m| cc_io_m3d::text3d::text_def(m, &text, height, depth, 0.14, ""));
        let def_id = match r {
            Ok(id) => id,
            Err(e) => {
                self.print(e.to_string());
                return;
            }
        };
        self.after_history();
        let hint = mt("app.text3d.where", &[]);
        let _ = self.tools.set("place", ToolArg::Place { def_id, align: true, hint: Some(hint.clone()) });
        self.vcb.clear();
        self.print(hint);
    }

    // ---- the dialogs ----

    /// Draw the open dialogs; their buttons become actions.
    pub fn dialogs(&mut self, ctx: &egui::Context) {
        let mut act: Option<String> = None;
        if let Some(Text3dDlg { mut text, mut height, mut depth, err }) = self.text_dlg.clone() {
            egui::Window::new(mt("app.text3d.h", &[]))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Grid::new("mc-t3").num_columns(2).show(ui, |ui| {
                        let l = ui.label(mt("app.text3d.text", &[]));
                        ui.add(egui::TextEdit::multiline(&mut text).desired_rows(3).char_limit(400)).labelled_by(l.id);
                        ui.end_row();
                        let l = ui.label(mt("app.text3d.height", &[]));
                        ui.text_edit_singleline(&mut height).labelled_by(l.id);
                        ui.end_row();
                        let l = ui.label(mt("app.text3d.depth", &[]));
                        ui.text_edit_singleline(&mut depth).labelled_by(l.id);
                        ui.end_row();
                    });
                    ui.label(egui::RichText::new(mt("app.text3d.note", &[])).small());
                    if !err.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(0xd0, 0x22, 0x3e), &err);
                    }
                    ui.horizontal(|ui| {
                        if ui.button(mt("app.btn.cancel", &[])).clicked() {
                            act = Some("text3d-cancel".into());
                        }
                        if ui.button(mt("app.text3d.place", &[])).clicked() {
                            act = Some(format!("text3d-place:{}", serde_json::json!({ "text": text, "height": height, "depth": depth })));
                        }
                    });
                });
            if act.is_none() {
                self.text_dlg = Some(Text3dDlg { text, height, depth, err });
            }
        }
        self.more_dialogs(ctx, &mut act);
        if let Some(a) = act
            && let Err(e) = self.act(&a)
        {
            self.message = e;
        }
    }
}

/// The palette's Edit, Solids and 3D text entries (ui.inc's Edit group, the More menu, the Solids menu, File ›
/// Export › IFC storeys): → the action picked.
pub fn palette_menus(ui: &mut egui::Ui) -> Option<String> {
    let mut act: Option<String> = None;
    let item = |ui: &mut egui::Ui, key: &str, a: &str, act: &mut Option<String>| {
        if ui.button(mt(key, &[])).clicked() {
            *act = Some(a.to_string());
            ui.close();
        }
    };
    ui.horizontal_wrapped(|ui| {
        if ui.button(mt("ui.tool.copy", &[])).on_hover_text(mt("ui.tool.copy.title", &[])).clicked() {
            act = Some("copy".into());
        }
        if ui.button(mt("ui.tool.paste", &[])).on_hover_text(mt("ui.tool.paste.title", &[])).clicked() {
            act = Some("paste".into());
        }
    });
    ui.menu_button(mt("ui.editmenu", &[]), |ui| {
        for (k, a) in [
            ("ui.editmenu.cut", "cut"),
            ("ui.editmenu.copy", "copy"),
            ("ui.editmenu.paste", "paste"),
            ("ui.editmenu.paste_in_place", "paste-in-place"),
            ("ui.editmenu.duplicate", "duplicate"),
            ("ui.editmenu.delete", "delete"),
        ] {
            item(ui, k, a, &mut act);
        }
        ui.separator();
        for (k, a) in [("ui.editmenu.hide", "hide"), ("ui.editmenu.isolate", "isolate"), ("ui.editmenu.unhide_all", "unhide-all")] {
            item(ui, k, a, &mut act);
        }
        ui.separator();
        for ax in ["x", "y", "z"] {
            item(ui, &format!("ui.editmenu.flip_{ax}"), &format!("flip-{ax}"), &mut act);
        }
        for ax in ["x", "y", "z"] {
            item(ui, &format!("ui.editmenu.flipcopy_{ax}"), &format!("flipcopy-{ax}"), &mut act);
        }
        ui.separator();
        item(ui, "ui.editmenu.intersect_model", "intersect-model", &mut act);
        item(ui, "ui.editmenu.intersect_sel", "intersect-sel", &mut act);
        ui.separator();
        item(ui, "ui.editmenu.axes_place", "axes-place", &mut act);
        item(ui, "ui.editmenu.axes_face", "axes-face", &mut act);
        item(ui, "ui.editmenu.axes_reset", "axes-reset", &mut act);
        ui.separator();
        item(ui, "ui.editmenu.select_all", "select-all", &mut act);
        item(ui, "ui.editmenu.select_none", "select-none", &mut act);
    })
    .response
    .on_hover_text(mt("ui.editmenu.title", &[]));
    ui.menu_button(mt("ui.solids", &[]), |ui| {
        item(ui, "ui.solids.union", "solid-union", &mut act);
        item(ui, "ui.solids.subtract", "solid-subtract", &mut act);
        item(ui, "ui.solids.intersect", "solid-intersect", &mut act);
    })
    .response
    .on_hover_text(mt("ui.solids.title", &[]));
    if ui.button(mt("ui.tool.text3d", &[])).on_hover_text(mt("ui.tool.text3d.title", &[])).clicked() {
        act = Some("text3d".into());
    }
    if ui.button(mt("ui.export.storeys", &[])).clicked() {
        act = Some("storeys".into());
    }
    act
}

#[cfg(test)]
pub(crate) mod testkit {
    use crate::modelcraft::ModelView;
    use cc_m3d::Model;
    use cc_m3d::ops::{Sel, add_polygon, connected, push_pull, rect_pts};

    /// 1 m boxes made groups (closed solids) at these x, everything selected.
    pub fn groups(xs: &[f64]) -> ModelView {
        let mut m = Model::new();
        m.units = "m".into();
        for &x in xs {
            let _ = m.change("x", |m| {
                let d = m.defs.get_mut("root").ok_or(cc_m3d::Error::Type)?;
                let before = d.faces.key_vec();
                add_polygon(d, &rect_pts([x, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0, 1.0), Some([0.0, 0.0, 1.0]))?;
                let f = d.faces.key_vec().into_iter().find(|k| !before.contains(k)).ok_or(cc_m3d::Error::Type)?;
                push_pull(d, f, 1.0);
                let sel = connected(d, &[Sel::new("f", f)]);
                m.make_group("root", &sel, "group", None)?;
                Ok(())
            });
        }
        let mut v = ModelView::new(m, "Boxes.modelcraft", true).unwrap();
        v.act("select-all").unwrap();
        v
    }

    pub fn root_insts(v: &ModelView) -> Vec<f64> {
        v.tools.host.model.root().map(|d| d.insts.key_vec()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::{groups, root_insts};

    #[test]
    fn clipboard_and_flip() {
        let mut v = groups(&[0.0, 3.0]);
        v.act("copy").unwrap();
        assert!(v.message.starts_with("Copied 2 items."), "{}", v.message);
        v.act("paste-in-place").unwrap();
        assert_eq!(v.message, "Pasted in place.");
        assert_eq!(root_insts(&v).len(), 4, "the two groups come back beside themselves");
        assert_eq!(v.tools.host.sel.len(), 2, "the pasted copies are selected");
        v.act("flip-x").unwrap();
        assert_eq!(v.message, "Flipped along red.");
        v.act("flipcopy-z").unwrap();
        assert_eq!(v.message, "Flipped copy along blue.");
        assert_eq!(root_insts(&v).len(), 6);
        v.act("select-none").unwrap();
        v.act("flip-y").unwrap();
        assert_eq!(v.message, "Select something to flip first.");
        v.act("copy").unwrap();
        assert_eq!(v.message, "Select something to copy first.");
        // paste: the copy follows the pointer; a click puts it down
        v.act("paste").unwrap();
        assert_eq!(v.tools.name, "paste");
        v.act("view-size:1000,800").unwrap();
        v.act("click:500,400").unwrap();
        assert_eq!(root_insts(&v).len(), 8);
        v.act("cut").unwrap();
        assert_eq!(v.message, "Cut. Ctrl+V pastes it.");
        assert_eq!(root_insts(&v).len(), 6);
        // duplicate and a typed offset
        v.act("select-all").unwrap();
        v.act("duplicate").unwrap();
        assert!(v.tools.busy());
        v.act("vcb:<0,2,0>").unwrap();
        assert!(!v.tools.busy());
        assert_eq!(root_insts(&v).len(), 12);
        assert!(v.act("view-size:0,0").is_err());
        assert!(v.act("click:x,1").is_err());
    }

    #[test]
    fn text_dialog_and_place() {
        let mut v = groups(&[0.0]);
        v.act("text3d").unwrap();
        assert!(v.text_dlg.is_some());
        v.act(r#"text3d-place:{"text":"  ","height":"0.3","depth":"0"}"#).unwrap();
        assert_eq!(v.text_dlg.as_ref().map(|d| d.err.as_str()), Some("Type some text."));
        v.act(r#"text3d-place:{"text":"Hi","height":"0","depth":"0"}"#).unwrap();
        assert!(v.text_dlg.as_ref().is_some_and(|d| d.err.starts_with("Type a letter height")));
        v.act(r#"text3d-place:{"text":"Hi","height":"30cm","depth":""}"#).unwrap();
        assert!(v.text_dlg.is_none());
        assert_eq!(v.tools.name, "place");
        assert!(v.message.starts_with("Click where the text goes"));
        assert!(v.tools.host.model.defs.values().any(|d| d.name == "Hi#1" && d.kind == "component"));
        // the dialog starts from the last text
        v.act("text3d").unwrap();
        assert_eq!(v.text_dlg.as_ref().map(|d| d.text.as_str()), Some("Hi"));
        assert!(v.act("text3d-place:{").is_err());
    }

    #[test]
    fn place_guard_drops_a_purged_definition() {
        // 0.5 places 3D text whose definition a purge dropped meanwhile: the file then names a missing definition
        let mut v = groups(&[0.0]);
        v.act(r#"text3d-place:{"text":"Hi","height":"1","depth":"0"}"#).unwrap();
        let _ = v.tools.host.model.change("purge", |m| {
            m.purge();
            Ok(())
        });
        v.act("view-size:1000,800").unwrap();
        v.act("click:500,400").unwrap();
        assert_eq!(v.tools.name, "select");
        let m = &v.tools.host.model;
        assert!(m.defs.values().all(|d| d.insts.values().all(|i| m.def(&i.def).is_some())));
    }
}
