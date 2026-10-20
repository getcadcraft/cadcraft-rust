//! MarkupCraft's tool chest and picture stamps (M9.17), ported from markupcraft.js: the chest (saveToChest, renderChest:
//! use a tool, remove it, import / export the chest file — chest.js), a tool in use as the preset of its drawing tool
//! (`toolPreset`: its style, subject, stamp or count label and starting text go on every markup it draws), the stamp
//! tool's "Picture…" and "My signature…" (currentPicture / placePicture — imagestamp.js; a click places the picture at
//! its usual size, a dragged box fits it), the signature pad, and a photo on a markup's thread. What the JS keeps in the
//! browser (the chest, the picture, the signature, the tool settings) is `MarkupTools::prefs_json`, which the desktop
//! app keeps in `markupcraft.json` between runs. Pictures are read by cc-io-markup `picture` (PNG / JPEG under limits).

use cc_markup::i18n::{Fill, t, t0};
use cc_markup::model::{Markup, type_name};
use egui::RichText;
use serde_json::{Map, Value, json};

use crate::markuptools::MarkupTools;
use crate::pdfview::PdfView;
use crate::theme::Tokens;

/// The stamp list's "Picture…" and "My signature…" (`PIC`, `SIG`).
pub const PIC: &str = "\u{1}picture";
pub const SIG: &str = "\u{1}signature";

/// A tool's string field ('' when absent).
pub fn tstr(t: &Value, k: &str) -> String {
    t.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

impl MarkupTools {
    /// The settings MarkupCraft keeps between runs (localStorage in the JS): the chest, the picture, the signature,
    /// the stamp, the count group name and each tool's style.
    pub fn prefs_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("chest".into(), Value::Array(self.chest.clone()));
        if let Some((src, name)) = &self.picture {
            o.insert("picture".into(), json!({ "src": src, "name": name }));
        }
        if let Some(src) = &self.signature {
            o.insert("signature".into(), json!({ "src": src }));
        }
        o.insert("stamp".into(), json!(self.stamp));
        o.insert("countLabel".into(), json!(self.count_label));
        let styles: Map<String, Value> = self.styles.iter().map(|(k, v)| (k.clone(), Value::Object(v.clone()))).collect();
        o.insert("styles".into(), Value::Object(styles));
        Value::Object(o)
    }

    /// Take the kept settings back (each value is checked as the JS checks what it reads from the browser).
    pub fn load_prefs(&mut self, v: &Value) {
        let env = cc_markup::model::Env::default();
        if let Some(list) = v.get("chest").and_then(Value::as_array) {
            self.chest = list
                .iter()
                .take(cc_markup::chest::MAX_TOOLS)
                .enumerate()
                .filter_map(|(i, t)| cc_markup::chest::clean_tool(&env, t, i))
                .collect();
        }
        let src_of = |k: &str| {
            v.get(k).and_then(|p| p.get("src")).map(|s| cc_markup::imagestamp::clean_image_value(Some(s))).filter(|s| !s.is_empty())
        };
        if let Some(src) = src_of("picture") {
            let name: String = v.get("picture").map(|p| tstr(p, "name")).unwrap_or_default().chars().take(60).collect();
            self.picture = Some((src, name));
        }
        self.signature = src_of("signature");
        if let Some(s) = v.get("stamp").and_then(Value::as_str) {
            self.stamp = s.chars().take(60).collect();
        }
        if let Some(s) = v.get("countLabel").and_then(Value::as_str) {
            self.count_label = s.chars().take(60).collect();
        }
        if let Some(st) = v.get("styles").and_then(Value::as_object) {
            for (k, s) in st {
                if cc_markup::model::is_type(k)
                    && let Some(o) = s.as_object()
                {
                    let clean: Map<String, Value> = o
                        .iter()
                        .filter(|(f, x)| match f.as_str() {
                            "color" | "fill" => x.is_null() || x.as_str().is_some_and(cc_markup::js::is_hex_color),
                            "width" | "fillOpacity" | "opacity" | "fontSize" => x.as_f64().is_some_and(f64::is_finite),
                            _ => false,
                        })
                        .map(|(f, x)| (f.clone(), x.clone()))
                        .collect();
                    self.styles.insert(k.clone(), clean);
                }
            }
        }
    }

    /// `currentPicture()`: the picture or signature the stamp tool places (src, label), when one is chosen.
    pub fn current_picture(&self) -> Option<(String, String)> {
        match self.stamp.as_str() {
            PIC => self.picture.as_ref().map(|(src, name)| (src.clone(), if name.is_empty() { type_name("image") } else { name.clone() })),
            SIG => self.signature.as_ref().map(|src| (src.clone(), t0("image.signature.label"))),
            _ => None,
        }
        .filter(|(src, _)| !cc_markup::imagestamp::clean_image(src).is_empty())
    }
}

impl PdfView {
    /// saveToChest: the selected markup's look as a tool named `name` (the Properties' "Save as a tool" asks for it).
    pub fn save_to_chest(&mut self, name: &str) -> Result<(), String> {
        let Some(m) = self.tools.sel.iter().filter_map(|id| self.store.get(id)).find(|m| !m.locked).cloned() else {
            self.tools.message = t0("chest.save.need");
            return Ok(());
        };
        let name = cc_markup::js::trim(name).to_string();
        if name.is_empty() {
            return Err(t0("chest.dlg.need"));
        }
        self.tick();
        let made = cc_markup::chest::tool_from_markup(&self.store.env, &m, &name).ok_or_else(|| t0("chest.save.need"))?;
        let tool = tstr(&made, "name");
        self.tools.chest.push(made);
        self.tools.prefs_dirty = true;
        self.tab = "chest";
        self.tools.message = t("chest.saved", &[("tool", Fill::S(tool))]);
        Ok(())
    }

    /// The name the Save as a tool dialog starts with (the subject when it is not the type's own, else the type).
    pub fn chest_name_for(&self, m: &Markup) -> String {
        if !m.subject.is_empty() && m.subject != cc_markup::model::type_subject(&m.kind) { m.subject.clone() } else { type_name(&m.kind) }
    }

    /// A chest tool clicked: its drawing tool with the tool as the preset.
    pub fn use_tool(&mut self, i: usize) -> Result<(), String> {
        let tk = self.tools.chest.get(i).cloned().ok_or_else(|| format!("markup.chest: no tool {i}"))?;
        let ty = tstr(&tk, "type");
        if !crate::markuptools::TOOLS.contains(&ty.as_str()) {
            return Err(format!("markup.chest: {ty} is not a drawing tool"));
        }
        let key = self.sheet_key();
        self.tools.set_tool(&mut self.store, &key, &ty);
        self.tools.preset = Some(tk.clone());
        self.tools.message = t("chest.drawing", &[("tool", Fill::S(tstr(&tk, "name")))]);
        Ok(())
    }

    /// A tool taken out of the chest (the drawing tool stops using it).
    pub fn remove_tool(&mut self, i: usize) -> Result<(), String> {
        if i >= self.tools.chest.len() {
            return Err(format!("markup.chest: no tool {i}"));
        }
        let tk = self.tools.chest.remove(i);
        if self.tools.preset.as_ref().is_some_and(|p| p.get("id") == tk.get("id")) {
            self.tools.preset = None;
        }
        self.tools.prefs_dirty = true;
        Ok(())
    }

    /// Import a chest file (merged: the same id replaces, new ones are added).
    pub fn import_chest(&mut self, file: &str, bytes: &[u8]) {
        if bytes.len() > 2 * 1_048_576 {
            self.tools.message = t0("chest.import.big");
            return;
        }
        self.tick();
        let text = String::from_utf8_lossy(bytes);
        match cc_markup::chest::parse_chest(&self.store.env, &text) {
            Err(e) => self.tools.message = e.0,
            Ok((tools, _)) if tools.is_empty() => self.tools.message = t("chest.import.none", &[("file", Fill::S(file.to_string()))]),
            Ok((tools, skipped)) => {
                self.tools.chest = cc_markup::chest::merge_chest(&self.tools.chest, &tools);
                self.tools.prefs_dirty = true;
                let mut msg = t("chest.imported", &[("n", Fill::N(tools.len() as f64)), ("file", Fill::S(file.to_string()))]);
                if skipped > 0 {
                    msg = format!("{msg} {}", t("chest.import.skipped", &[("n", Fill::N(skipped as f64))]));
                }
                self.tools.message = msg;
            }
        }
    }

    /// Export the chest as its file (to the host's save).
    pub fn export_chest(&mut self) {
        self.tick();
        let text = cc_markup::chest::export_chest(&self.store.env, &self.tools.chest);
        self.outbox.push((format!("{}.json", t0("chest.file")), text.into_bytes()));
        self.tools.message = t("chest.exported", &[("n", Fill::N(self.tools.chest.len() as f64))]);
    }

    /// choosePicture: a picked PNG or JPEG becomes the stamp tool's picture.
    pub fn set_picture(&mut self, file: &str, bytes: &[u8]) {
        let lower = file.to_ascii_lowercase();
        let ok_name = [".png", ".jpg", ".jpeg"].iter().any(|e| lower.ends_with(e));
        let src = if ok_name { cc_io_markup::picture::picture_from(bytes).unwrap_or_default() } else { String::new() };
        if src.is_empty() {
            self.tools.message = t("image.bad", &[("file", Fill::S(file.to_string()))]);
            return;
        }
        let stem = file.rsplit_once('.').map_or(file, |(a, _)| a);
        let name: String = stem.chars().take(60).collect();
        self.tools.picture = Some((src, name.clone()));
        self.tools.stamp = PIC.into();
        self.tools.preset = None;
        self.tools.prefs_dirty = true;
        self.tools.message = t("image.ready", &[("name", Fill::S(name))]);
    }

    /// A signature (a transparent PNG data URL from the pad, or a picked PNG) becomes the stamp tool's signature.
    pub fn set_signature(&mut self, src: &str) -> Result<(), String> {
        let src = cc_markup::imagestamp::clean_image(src);
        if src.is_empty() {
            return Err(t0("image.signature.need"));
        }
        self.tools.signature = Some(src);
        self.tools.stamp = SIG.into();
        self.tools.preset = None;
        self.tools.prefs_dirty = true;
        self.tools.message = t0("image.signature.ready");
        Ok(())
    }

    /// A photo on the selected markup's thread (the reply's text, if any, is its caption).
    pub fn photo_reply(&mut self, file: &str, bytes: &[u8], caption: &str) -> Result<(), String> {
        let id = self.tools.selected().map(str::to_string).ok_or("markup.photo: select one markup")?;
        let lower = file.to_ascii_lowercase();
        let ok_name = [".png", ".jpg", ".jpeg"].iter().any(|e| lower.ends_with(e));
        let src = if ok_name { cc_io_markup::picture::picture_from(bytes).unwrap_or_default() } else { String::new() };
        if src.is_empty() {
            self.tools.message = t("image.bad", &[("file", Fill::S(file.to_string()))]);
            return Ok(());
        }
        self.tick();
        let author = self.tools.author.clone();
        let text = cc_markup::js::trim(caption).to_string();
        let r =
            cc_markup::model::clean_reply(&mut self.store.env, &json!({ "author": author, "text": text, "kind": "comment", "photo": src }))
                .ok_or("markup.photo: not a usable reply")?;
        let Some(cur) = self.store.get(&id).cloned() else { return Ok(()) };
        let mut replies: Vec<Value> = cur.replies.iter().map(crate::markupprops::reply_value).collect();
        replies.push(crate::markupprops::reply_value(&r));
        let mut patch = Map::new();
        patch.insert("replies".into(), Value::Array(replies));
        self.store.change(&t0("undo.photo"), |tx| Ok(tx.update(&id, &patch, false))).map_err(|e| e.to_string())?;
        self.reply_drafts.remove(&id);
        self.tools.message = t0("photo.added");
        Ok(())
    }

    /// The Tools tab (renderChest): a row per tool (a click draws with it, × removes it), Save selected as a tool, Import,
    /// Export.
    pub fn chest_panel(&mut self, ui: &mut egui::Ui, tok: &Tokens) {
        ui.label(RichText::new(t0("chest.h")).strong().color(tok.text_strong));
        ui.label(RichText::new(t0("chest.note")).small().color(tok.text_dim));
        let mut act: Option<(&str, usize)> = None;
        ui.horizontal_wrapped(|ui| {
            if ui.button(t0("chest.save")).clicked() {
                act = Some(("save", 0));
            }
            if ui.button(t0("chest.import")).clicked() {
                act = Some(("import", 0));
            }
            if ui.button(t0("chest.export")).clicked() {
                act = Some(("export", 0));
            }
        });
        if self.tools.chest.is_empty() {
            ui.label(RichText::new(t0("chest.empty")).color(tok.text_dim));
        }
        let using = self.tools.preset.as_ref().and_then(|p| p.get("id")).cloned();
        for (i, tk) in self.tools.chest.iter().enumerate() {
            let ty = tstr(tk, "type");
            let label = tstr(tk, "label");
            let meta = if label.is_empty() {
                type_name(&ty)
            } else {
                t("chest.meta.label", &[("type", Fill::S(type_name(&ty))), ("label", Fill::S(label))])
            };
            let colour =
                tk.get("style").and_then(|s| s.get("color")).and_then(Value::as_str).and_then(|c| crate::pdfview::hex_color(c, 1.0));
            ui.horizontal(|ui| {
                ui.label(RichText::new("■").color(colour.unwrap_or(tok.text)));
                let on = using.is_some() && tk.get("id") == using.as_ref();
                if ui.selectable_label(on, RichText::new(tstr(tk, "name")).strong()).on_hover_text(&meta).clicked() {
                    act = Some(("use", i));
                }
                ui.label(RichText::new(&meta).small().color(tok.text_dim));
                if ui.small_button("×").on_hover_text(t0("chest.remove.title")).clicked() {
                    act = Some(("remove", i));
                }
            });
        }
        match act {
            Some(("save", _)) => match self.tools.sel.iter().filter_map(|id| self.store.get(id)).find(|m| !m.locked).cloned() {
                Some(m) => self.dlg = Some(crate::markupedit::Dialog::ChestName { name: self.chest_name_for(&m), error: String::new() }),
                None => self.tools.message = t0("chest.save.need"),
            },
            Some(("import", _)) => self.want_pick = Some("chest-import"),
            Some(("export", _)) => self.export_chest(),
            Some(("use", i)) => {
                if let Err(e) = self.use_tool(i) {
                    self.tools.message = e;
                }
            }
            Some(("remove", i)) => self.dlg = Some(crate::markupedit::Dialog::RemoveTool { index: i }),
            _ => {}
        }
    }

    /// The signature pad (drawSignature): strokes with the mouse, a pen or a finger; Use this signature keeps it.
    pub fn signature_dialog(&mut self, ctx: &egui::Context, tok: &Tokens) {
        let Some(crate::markupedit::Dialog::Signature { mut strokes, mut error }) = self.dlg.clone() else { return };
        let (mut close, mut ok) = (false, false);
        egui::Window::new(t0("image.signature.title")).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(t0("image.signature.text"));
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(520.0, 180.0), egui::Sense::drag());
            ui.painter().rect_filled(rect, 2.0, egui::Color32::WHITE);
            ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, tok.divider), egui::StrokeKind::Inside);
            let at = |p: egui::Pos2| [f64::from(p.x - rect.min.x), f64::from(p.y - rect.min.y)];
            if resp.drag_started()
                && let Some(p) = ui.input(|i| i.pointer.press_origin())
            {
                strokes.push(vec![at(p)]);
            } else if resp.dragged()
                && let (Some(p), Some(s)) = (resp.interact_pointer_pos(), strokes.last_mut())
                && s.len() < 10_000
            {
                s.push(at(p));
            }
            let ink = egui::Color32::from_rgb(0x1b, 0x2a, 0x4a);
            for s in &strokes {
                let pts: Vec<egui::Pos2> = s.iter().map(|q| rect.min + egui::vec2(q[0] as f32, q[1] as f32)).collect();
                if pts.len() == 1 {
                    if let Some(p) = pts.first() {
                        ui.painter().circle_filled(*p, 1.3, ink);
                    }
                } else {
                    ui.painter().add(egui::Shape::line(pts, egui::Stroke::new(2.6, ink)));
                }
            }
            if !error.is_empty() {
                ui.colored_label(ui.visuals().error_fg_color, &error);
            }
            ui.horizontal(|ui| {
                if ui.button(t0("image.signature.clear")).clicked() {
                    strokes.clear();
                }
                close = ui.button(t0("dlg.cancel")).clicked();
                ok = ui.button(t0("image.signature.ok")).clicked();
            });
        });
        if ok {
            match cc_io_markup::picture::signature_png(&strokes) {
                Some(src) => {
                    if let Err(e) = self.set_signature(&src) {
                        error = e;
                    } else {
                        close = true;
                    }
                }
                None => error = t0("image.signature.need"),
            }
        }
        self.dlg = if close { None } else { Some(crate::markupedit::Dialog::Signature { strokes, error }) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        // a tiny valid PNG made by the signature pad's encoder
        let url = cc_io_markup::picture::signature_png(&[vec![[10.0, 10.0], [10.0 + f64::from(w), 10.0 + f64::from(h)]]]).unwrap();
        cc_markup::imagestamp::b64_decode(url.split_once(";base64,").unwrap().1)
    }

    fn view() -> PdfView {
        let pdf = cc_io_markup::pages::blank_pdf(Some(&json!({ "paper": "letter", "grid": "none", "pages": 1 })), "", 0.0);
        let mut v = PdfView::open("a.pdf", &pdf).unwrap();
        v.set_zoom(1.0);
        v
    }

    #[test]
    fn a_chest_tool_draws_with_its_look_and_the_chest_travels_as_a_file() {
        let mut v = view();
        assert_eq!(v.tools.chest.len(), 6);
        v.use_tool(0).unwrap();
        assert_eq!(v.tools.tool, "cloudrect");
        let (a, b) = ([100.0, 100.0], [200.0, 160.0]);
        v.gesture_down(a, v.screen_of(a), false, false);
        v.tools.mv(&v.store, b, v.screen_of(b), false);
        v.gesture_up(b, v.screen_of(b), false);
        let m = v.store.get(&v.tools.sel[0]).unwrap().clone();
        assert_eq!(m.subject, t0("chest.default.revision.subject"));
        assert_eq!(m.style.width, 1.5);
        v.save_to_chest("Mine").unwrap();
        assert_eq!(v.tools.chest.len(), 7);
        v.export_chest();
        let (name, bytes) = v.outbox.pop().unwrap();
        assert_eq!(name, "MarkupCraft tool chest.json");
        v.remove_tool(6).unwrap();
        v.import_chest("x.json", &bytes);
        assert_eq!(v.tools.chest.len(), 7);
        // a toolbar tool drops the preset
        let key = v.sheet_key();
        v.tools.set_tool(&mut v.store, &key, "rect");
        assert!(v.tools.preset.is_none());
        let p = v.tools.prefs_json();
        let mut t2 = MarkupTools::default();
        t2.load_prefs(&p);
        assert_eq!(t2.chest.len(), 7);
    }

    #[test]
    fn a_picture_stamp_is_placed_at_its_size_or_fitted_in_a_box() {
        let mut v = view();
        v.set_picture("logo.png", &png(80, 40));
        assert_eq!(v.tools.stamp, PIC);
        let key = v.sheet_key();
        v.tools.set_tool(&mut v.store, &key, "stamp");
        let a = [300.0, 300.0];
        v.gesture_down(a, v.screen_of(a), false, false);
        v.gesture_up(a, v.screen_of(a), false);
        let m = v.store.get(&v.tools.sel[0]).unwrap().clone();
        assert_eq!(m.kind, "image");
        assert_eq!(m.label, "logo");
        let r = m.rect.unwrap();
        assert!((r[2] - r[0] - 216.0).abs() < 1e-9, "3 in wide: {r:?}");
        v.set_picture("notes.txt", b"hello");
        assert_eq!(v.tools.message, t("image.bad", &[("file", Fill::S("notes.txt".into()))]));
        v.photo_reply("p.png", &png(20, 20), "Site photo").unwrap();
        assert_eq!(v.store.get(&m.id).unwrap().replies.len(), 1);
    }
}
