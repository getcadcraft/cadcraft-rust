//! TakeoffCraft's typical groups (T16: marks taken off once, counted `repeat` times) and markups (T22: notes, revision
//! clouds and highlights with a status; never counted, drawn in the marked PDF), as takeoffcraft.js has them: the group
//! field of the selected marks, the New typical group window, the groups list (rename, repeats, select its marks,
//! delete), the dashed box round a group's marks; the Markup tools, the text asked for a new markup, the selected
//! markup's panel (status, text, delete), the Markups list. Each edit is the `Takeoff` call 0.5 makes, with its status
//! line.

use cc_takeoff::geom::P;
use cc_takeoff::i18n::{F, t, tf, tn};
use egui::{Color32, Pos2, RichText};
use serde_json::{Map, Value, json};

use crate::takeofftools::TakeoffMode;
use crate::theme::Tokens;

const MAX_REPEAT: f64 = 10_000.0;

fn s(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(x)) => x.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// A markup waiting for its text (askNoteText): the kind, the sheet, its points, what is typed.
#[derive(Debug, Clone)]
pub struct NoteAsk {
    pub kind: String,
    pub sheet: String,
    pub pts: Vec<P>,
    pub text: String,
}

/// The New typical group window: the marks, the name and repeat typed, the error it shows.
#[derive(Debug, Clone)]
pub struct GroupAsk {
    pub ids: Vec<String>,
    pub name: String,
    pub repeat: String,
    pub error: String,
}

/// NOTE_COLORS of pdfmarks.js.
pub fn note_color(k: &str) -> Color32 {
    let hex = match k {
        "open" => "#D9480F",
        "answered" => "#1F6FEB",
        "closed" => "#6E7781",
        "cloud" => "#D62728",
        _ => "#FFD400",
    };
    crate::pdfview::hex_color(hex, 1.0).unwrap_or(Color32::RED)
}

/// readRepeat: "3", "x3", "×3" → 3 (1 to 10 000), else None.
pub fn read_repeat(v: &str) -> Option<f64> {
    let t0 = v.trim();
    let t1 = t0.strip_prefix(['x', 'X', '×']).map_or(t0, str::trim_start);
    let ok = !t1.is_empty() && t1.len() <= 5 && t1.chars().all(|c| c.is_ascii_digit());
    let n: f64 = if ok { t1.parse().ok()? } else { return None };
    (1.0..=MAX_REPEAT).contains(&n).then_some(n)
}

fn first_line(text: &str, max: usize) -> String {
    // .split('\n')[0].slice(0, max) counts UTF-16 units
    let line = text.split('\n').next().unwrap_or("");
    let mut units = 0usize;
    line.chars()
        .take_while(|c| {
            units = units.saturating_add(c.len_utf16());
            units <= max
        })
        .collect()
}

impl TakeoffMode {
    // ---- markups ----

    /// noteAt: a note where the click is; its text is asked first.
    pub(crate) fn note_at(&mut self, sheet: &str, p: P) {
        self.note_ask = Some(NoteAsk { kind: "note".into(), sheet: sheet.to_string(), pts: vec![p], text: String::new() });
    }

    /// boxNote: a cloud or highlight between two corners (at least 4 screen pixels apart each way); its text is asked.
    pub(crate) fn box_note(&mut self, sheet: &str, kind: &str, a: P, b: P) {
        let min = self.upx(4.0);
        if (a.x - b.x).abs() < min || (a.y - b.y).abs() < min {
            self.message = t("note.err.small");
            return;
        }
        self.note_ask = Some(NoteAsk { kind: kind.to_string(), sheet: sheet.to_string(), pts: vec![a, b], text: String::new() });
    }

    /// The asked text arrives (Add) — or None (Cancel): the markup is made and selected.
    pub fn note_answer(&mut self, text: Option<&str>) {
        let Some(ask) = self.note_ask.take() else { return };
        let Some(text) = text else { return };
        if ask.kind == "note" && text.trim().is_empty() {
            self.message = t("note.err.empty");
            return;
        }
        let pts: Vec<Value> = ask.pts.iter().map(|p| json!({ "x": p.x, "y": p.y })).collect();
        let f: Map<String, Value> =
            [("kind", json!(ask.kind)), ("sheet", json!(ask.sheet)), ("pts", Value::Array(pts)), ("text", json!(text))]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect();
        match self.tk.add_note(&f) {
            Ok(Some(id)) => {
                self.sel_note = Some(id);
                self.message = t(&format!("note.added.{}", ask.kind));
            }
            Ok(None) => {}
            Err(e) => self.message = e,
        }
    }

    /// noteHit: the markup under a point (the topmost): a note's pin, or inside a cloud's or highlight's box.
    pub fn note_hit(&self, sheet: &str, p: P) -> Option<Value> {
        let list: Vec<&Value> = self
            .tk
            .state
            .get("notes")
            .and_then(Value::as_array)
            .map(|l| l.iter().filter(|n| n.get("sheet").and_then(Value::as_str) == Some(sheet)).collect())
            .unwrap_or_default();
        for n in list.iter().rev() {
            let pts = cc_takeoff::model::pts_of(n.get("pts"));
            if n.get("kind").and_then(Value::as_str) == Some("note") {
                if pts.first().is_some_and(|q| (q.x - p.x).hypot(q.y - p.y) <= self.upx(crate::takeoffedit::HIT + 4.0)) {
                    return Some((*n).clone());
                }
                continue;
            }
            let (Some(a), Some(b)) = (pts.first(), pts.get(1)) else { continue };
            let m = self.upx(3.0);
            if p.x >= a.x.min(b.x) - m && p.x <= a.x.max(b.x) + m && p.y >= a.y.min(b.y) - m && p.y <= a.y.max(b.y) + m {
                return Some((*n).clone());
            }
        }
        None
    }

    /// The selected markup's fields and the list (`\n` in a text is a new line): `select:<id>` (a row of the Markups list), `text:<text>`,
    /// `status:<open|answered|closed>`, `remove` (the panel's Delete), `add:<text>` / `cancel` (the asked text),
    /// `open-only:on|off` (the list's filter).
    pub fn note_edit(&mut self, spec: &str) -> Result<(), String> {
        let (op, rest) = spec.split_once(':').unwrap_or((spec, ""));
        // a command's text writes a new line as \n
        let typed = rest.replace("\\n", "\n");
        let rest = typed.as_str();
        match op {
            "add" => {
                if self.note_ask.is_none() {
                    return Err("takeoff.note: no markup is waiting for its text".into());
                }
                self.note_answer(Some(rest));
                return Ok(());
            }
            "cancel" => {
                self.note_answer(None);
                return Ok(());
            }
            "open-only" => {
                self.notes_open_only = rest == "on";
                return Ok(());
            }
            "select" => {
                if self.tk.note(rest).is_none() {
                    return Err(format!("takeoff.note: no markup {rest}"));
                }
                let sheet = self.tk.note(rest).map(|n| s(n.get("sheet"))).unwrap_or_default();
                self.goto = Some(sheet);
                self.set_sel(None);
                self.sel_note = Some(rest.to_string());
                return Ok(());
            }
            _ => {}
        }
        let id = self.sel_note.clone().ok_or("takeoff.note: no markup is selected")?;
        let r = match op {
            "text" => self.tk.update_note(&id, &obj([("text", json!(rest))]), None),
            "status" => self.tk.update_note(&id, &obj([("status", json!(rest))]), Some(&t("undo.note.status"))),
            "remove" => {
                let r = self.tk.remove_note(&id);
                self.sel_note = None;
                r
            }
            _ => return Err(format!("takeoff.note: unknown {op} (add, cancel, select, text, status, remove, open-only)")),
        };
        r.map(|_| ()).inspect_err(|e| self.message = e.clone())
    }

    /// The selected markup's panel (renderNoteProps).
    pub fn note_props(&mut self, ui: &mut egui::Ui, tok: &Tokens) {
        let Some(id) = self.sel_note.clone() else { return };
        let Some(n) = self.tk.note(&id).cloned() else {
            self.sel_note = None;
            return;
        };
        ui.label(RichText::new(t("note.h")).strong().color(tok.text_strong));
        let kv = |ui: &mut egui::Ui, k: String, v: String| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(k).color(tok.text_dim));
                ui.label(v);
            });
        };
        kv(ui, t("note.f.kind"), t(&format!("tool.{}", s(n.get("kind")))));
        kv(ui, t("mark.f.sheet"), self.sheet_label(&s(n.get("sheet"))));
        let mut edit: Option<String> = None;
        let st = s(n.get("status"));
        let l = ui.label(RichText::new(t("note.f.status")).small());
        egui::ComboBox::new("tk_n_status", "")
            .selected_text(t(&format!("note.status.{st}")))
            .show_ui(ui, |ui| {
                for k in ["open", "answered", "closed"] {
                    if ui.selectable_label(st == k, t(&format!("note.status.{k}"))).clicked() && st != k {
                        edit = Some(format!("status:{k}"));
                    }
                }
            })
            .response
            .labelled_by(l.id);
        let text = s(n.get("text"));
        let key = format!("n-text:{id}");
        let e = self.edits.entry(key).or_insert_with(|| (text.clone(), text.clone()));
        if e.0 != text {
            *e = (text.clone(), text.clone());
        }
        let l = ui.label(RichText::new(t("note.f.text")).small());
        let r = ui.add(egui::TextEdit::multiline(&mut e.1).desired_rows(4).desired_width(ui.available_width())).labelled_by(l.id);
        if r.lost_focus() && e.1 != e.0 {
            edit = Some(format!("text:{}", e.1));
        }
        if ui.small_button(t("note.delete")).clicked() {
            edit = Some("remove".into());
        }
        if let Some(x) = edit {
            let _ = self.note_edit(&x);
        }
    }

    /// The Markups list (renderNotes): every markup, or only the open ones.
    pub fn notes_list(&mut self, ui: &mut egui::Ui, tok: &Tokens) {
        let all: Vec<Value> = self.tk.state.get("notes").and_then(Value::as_array).cloned().unwrap_or_default();
        if all.is_empty() {
            return;
        }
        let open = all.iter().filter(|n| n.get("status").and_then(Value::as_str) == Some("open")).count();
        #[allow(clippy::cast_precision_loss)]
        let head = tf("notes.h.n", &[("n", F::N(all.len() as f64)), ("open", F::N(open as f64))]);
        ui.label(RichText::new(head).strong().color(tok.text_strong));
        ui.checkbox(&mut self.notes_open_only, t("notes.open.only"));
        let mut pick: Option<String> = None;
        for n in &all {
            let st = s(n.get("status"));
            if self.notes_open_only && st != "open" {
                continue;
            }
            let id = s(n.get("id"));
            let kind = s(n.get("kind"));
            let line = first_line(&s(n.get("text")), 80);
            let line = if line.is_empty() { t("note.untitled") } else { line };
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 4.0, note_color(if kind == "note" { &st } else { &kind }));
                if ui
                    .selectable_label(
                        self.sel_note.as_deref() == Some(id.as_str()),
                        format!("{line}  ·  {}", t(&format!("note.status.{st}"))),
                    )
                    .clicked()
                {
                    pick = Some(id.clone());
                }
            });
        }
        if let Some(id) = pick {
            let _ = self.note_edit(&format!("select:{id}"));
        }
    }

    /// The asked text of a new markup (a small window): Add / Cancel.
    pub fn note_window(&mut self, ctx: &egui::Context) {
        let Some(ask) = self.note_ask.as_mut() else { return };
        let title = t(&format!("note.new.{}", ask.kind));
        let mut done: Option<Option<String>> = None;
        egui::Window::new(title).collapsible(false).resizable(false).show(ctx, |ui| {
            let l = ui.label(t("note.f.text"));
            ui.add(egui::TextEdit::multiline(&mut ask.text).desired_rows(4).char_limit(2000)).labelled_by(l.id);
            ui.horizontal(|ui| {
                if ui.button(t("btn.cancel")).clicked() {
                    done = Some(None);
                }
                if ui.button(t("note.add")).clicked() {
                    done = Some(Some(ask.text.clone()));
                }
            });
        });
        if let Some(d) = done {
            self.note_answer(d.as_deref());
        }
    }

    /// Markups on the open sheet (drawNotesLayer), and the box being clicked for a cloud or highlight.
    pub(crate) fn draw_notes(&self, painter: &egui::Painter, sheet: &str, at: &dyn Fn(f64, f64) -> Pos2) {
        let action = Color32::from_rgb(0x1e, 0x48, 0xc8);
        let ink = Color32::from_rgb(0x17, 0x20, 0x2b);
        let tag = |q: Pos2, text: &str, edge: Color32, left: bool| {
            let g = painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(12.0), ink);
            let w = g.size().x + 10.0;
            let r = if left {
                egui::Rect::from_min_size(q + egui::vec2(12.0, -9.0), egui::vec2(w, 18.0))
            } else {
                egui::Rect::from_center_size(q, egui::vec2(w, 18.0))
            };
            painter.rect_filled(r, 0.0, Color32::WHITE);
            painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, edge), egui::StrokeKind::Inside);
            painter.galley(r.left_center() + egui::vec2(5.0, -g.size().y / 2.0), g, ink);
        };
        for n in self.tk.state.get("notes").and_then(Value::as_array).into_iter().flatten() {
            if n.get("sheet").and_then(Value::as_str) != Some(sheet) {
                continue;
            }
            let sel = self.sel_note.as_deref() == n.get("id").and_then(Value::as_str);
            let pts = cc_takeoff::model::pts_of(n.get("pts"));
            let kind = s(n.get("kind"));
            let st = s(n.get("status"));
            let line = first_line(&s(n.get("text")), 40);
            if kind == "note" {
                let Some(p) = pts.first() else { continue };
                let c = at(p.x, p.y);
                painter.circle_filled(c, 8.0, note_color(&st));
                painter.text(c, egui::Align2::CENTER_CENTER, "!", egui::FontId::proportional(11.0), Color32::WHITE);
                if !line.is_empty() {
                    tag(c, &line, note_color(&st), true);
                }
                if sel {
                    painter.circle_stroke(c, 12.0, egui::Stroke::new(2.0, action));
                }
                continue;
            }
            let (Some(a), Some(b)) = (pts.first(), pts.get(1)) else { continue };
            let r = egui::Rect::from_two_pos(at(a.x, a.y), at(b.x, b.y));
            if kind == "highlight" {
                painter.rect_filled(r, 0.0, note_color("highlight").gamma_multiply(0.35));
            } else {
                let c = note_color(if st == "closed" { "closed" } else { "cloud" });
                let d = f64::from(r.width().min(r.height()) / 4.0).clamp(10.0, 24.0);
                let mut line_pts: Vec<Pos2> = Vec::new();
                let mut cur = [0.0f64; 2];
                cc_io_takeoff::pdfmarks::cloud_path(
                    f64::from(r.min.x),
                    f64::from(r.min.y),
                    f64::from(r.max.x),
                    f64::from(r.max.y),
                    d,
                    &mut |op, v| {
                        if op == 'M' {
                            cur = [v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0)];
                            line_pts.push(Pos2::new(cur[0] as f32, cur[1] as f32));
                        } else if let [x1, y1, x2, y2, x3, y3] = v[..] {
                            for k in 1..=8 {
                                let u = f64::from(k) / 8.0;
                                let w = 1.0 - u;
                                let x = w * w * w * cur[0] + 3.0 * w * w * u * x1 + 3.0 * w * u * u * x2 + u * u * u * x3;
                                let y = w * w * w * cur[1] + 3.0 * w * w * u * y1 + 3.0 * w * u * u * y2 + u * u * u * y3;
                                line_pts.push(Pos2::new(x as f32, y as f32));
                            }
                            cur = [x3, y3];
                        }
                    },
                );
                painter.add(egui::Shape::line(line_pts, egui::Stroke::new(2.0, c)));
            }
            if !line.is_empty() {
                tag(Pos2::new(r.center().x, r.min.y - 16.0), &line, ink, false);
            }
            if sel {
                let rr = r.expand(6.0);
                let ring = [rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()];
                painter.add(egui::Shape::dashed_line(&ring, egui::Stroke::new(1.5, action), 5.0, 4.0));
            }
        }
        if let (Some(a), Some(c)) = (self.note_start, self.cursor) {
            let col = if self.tool == "cloud" { note_color("cloud") } else { ink };
            let r = egui::Rect::from_two_pos(at(a.x, a.y), at(c[0], c[1]));
            let ring = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.add(egui::Shape::dashed_line(&ring, egui::Stroke::new(1.5, col), 6.0, 4.0));
        }
    }

    // ---- typical groups ----

    /// The dashed box round each group's marks on the sheet, with its name and repeat count.
    pub(crate) fn draw_groups(&self, painter: &egui::Painter, sheet: &str, at: &dyn Fn(f64, f64) -> Pos2) {
        let ink = Color32::from_rgb(0x17, 0x20, 0x2b);
        let mut boxes: Vec<(String, egui::Rect)> = Vec::new();
        for m in self.tk.marks() {
            if m.get("sheet").and_then(Value::as_str) != Some(sheet) {
                continue;
            }
            let Some(g) = m.get("group").and_then(Value::as_str) else { continue };
            for p in cc_takeoff::model::pts_of(m.get("pts")) {
                let q = at(p.x, p.y);
                match boxes.iter_mut().find(|b| b.0 == g) {
                    Some(b) => b.1.extend_with(q),
                    None => boxes.push((g.to_string(), egui::Rect::from_min_max(q, q))),
                }
            }
        }
        for (gid, b) in boxes {
            let Some(g) = self.tk.group(&gid) else { continue };
            let r = b.expand(10.0);
            let ring = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.add(egui::Shape::dashed_line(&ring, egui::Stroke::new(1.0, ink.gamma_multiply(0.7)), 6.0, 4.0));
            let text =
                tf("group.tag", &[("name", F::S(s(g.get("name")))), ("n", F::N(g.get("repeat").and_then(Value::as_f64).unwrap_or(1.0)))]);
            let gl = painter.layout_no_wrap(text, egui::FontId::proportional(12.0), ink);
            let tr = egui::Rect::from_center_size(Pos2::new(r.center().x, r.min.y - 10.0), egui::vec2(gl.size().x + 10.0, 18.0));
            painter.rect_filled(tr, 0.0, Color32::WHITE);
            painter.rect_stroke(tr, 0.0, egui::Stroke::new(1.0, ink), egui::StrokeKind::Inside);
            painter.galley(tr.center() - gl.size() / 2.0, gl, ink);
        }
    }

    /// The selected marks' group (groupField's change): a group id, "" (not typical), or the New group window.
    pub fn set_marks_group(&mut self, ids: &[String], g: &str) -> Result<(), String> {
        if g == "new" {
            #[allow(clippy::cast_precision_loss)]
            let n = self.tk.state.get("groups").and_then(Value::as_array).map_or(0, Vec::len) as f64 + 1.0;
            self.group_ask = Some(GroupAsk {
                ids: ids.to_vec(),
                name: tf("group.default", &[("n", F::N(n))]),
                repeat: "2".into(),
                error: String::new(),
            });
            return Ok(());
        }
        let group = (!g.is_empty()).then_some(g);
        match self.tk.set_marks_group(ids, group, None) {
            Ok(true) => {
                #[allow(clippy::cast_precision_loss)]
                let n = ids.len() as f64;
                self.message = match group.and_then(|x| self.tk.group(x)) {
                    Some(gr) => {
                        let fills = [
                            ("n", F::N(n)),
                            ("name", F::S(s(gr.get("name")))),
                            ("k", F::N(gr.get("repeat").and_then(Value::as_f64).unwrap_or(1.0))),
                        ];
                        tf("group.moved", &fills)
                    }
                    None => tn("group.left", n),
                };
                Ok(())
            }
            Ok(false) => Ok(()),
            Err(e) => {
                self.message = e.clone();
                Err(e)
            }
        }
    }

    /// The New typical group window's OK (newGroup's onOk): the name and repeat checked, the group made with the marks.
    /// An error stays in the window.
    pub fn group_ok(&mut self, name: &str, repeat: &str) -> Result<(), String> {
        let ids = self.group_ask.as_ref().map(|a| a.ids.clone()).unwrap_or_else(|| self.sel_ids());
        let fail = |me: &mut Self, e: String| -> Result<(), String> {
            if let Some(a) = me.group_ask.as_mut() {
                a.error = e.clone();
            }
            Err(e)
        };
        let n = read_repeat(repeat);
        if name.trim().is_empty() {
            return fail(self, t("group.err.name"));
        }
        let Some(n) = n else { return fail(self, tf("err.group.repeat", &[("max", F::N(MAX_REPEAT))])) };
        match self.tk.add_group(Some(&json!(name)), Some(&json!(n)), &ids) {
            Ok(Some(gid)) => {
                self.group_ask = None;
                let gr = self.tk.group(&gid).cloned().unwrap_or(Value::Null);
                #[allow(clippy::cast_precision_loss)]
                let fills = [
                    ("name", F::S(s(gr.get("name")))),
                    ("n", F::N(ids.len() as f64)),
                    ("k", F::N(gr.get("repeat").and_then(Value::as_f64).unwrap_or(1.0))),
                ];
                self.message = tf("group.made", &fills);
                Ok(())
            }
            Ok(None) => {
                self.group_ask = None;
                Ok(())
            }
            Err(e) => fail(self, e),
        }
    }

    /// The groups: `new:<name>|<repeat>` (the window's OK, with the selected marks), `cancel`, `rename:<id>:<name>`,
    /// `repeat:<id>:<n>`, `select:<id>`, `delete:<id>`.
    pub fn group_edit(&mut self, spec: &str) -> Result<(), String> {
        let (op, rest) = spec.split_once(':').unwrap_or((spec, ""));
        match op {
            "new" => {
                let (name, rep) = rest.split_once('|').unwrap_or((rest, "2"));
                if self.group_ask.is_none() {
                    let ids = self.sel_ids();
                    self.set_marks_group(&ids, "new")?;
                }
                self.group_ok(name, rep)
            }
            "cancel" => {
                self.group_ask = None;
                Ok(())
            }
            "rename" | "repeat" => {
                let (id, v) = rest.split_once(':').ok_or("takeoff.group: rename:<id>:<name> | repeat:<id>:<n>")?;
                if op == "rename" {
                    if v.trim().is_empty() {
                        self.message = t("group.err.name");
                        return Ok(());
                    }
                    self.tk.update_group(id, Some(&json!(v)), None).map(|_| ()).map_err(|e| self.say(e))
                } else {
                    let Some(k) = read_repeat(v) else {
                        self.message = tf("err.group.repeat", &[("max", F::N(MAX_REPEAT))]);
                        return Ok(());
                    };
                    self.tk.update_group(id, None, Some(&json!(k))).map(|_| ()).map_err(|e| self.say(e))
                }
            }
            "select" => {
                self.select_group(rest);
                Ok(())
            }
            "delete" => {
                let name = self.tk.group(rest).map(|g| s(g.get("name"))).unwrap_or_default();
                if self.tk.remove_group(rest).map_err(|e| self.say(e))? {
                    self.message = tf("group.deleted", &[("name", F::S(name))]);
                }
                Ok(())
            }
            _ => Err(format!("takeoff.group: unknown {op} (new, cancel, rename, repeat, select, delete)")),
        }
    }

    fn say(&mut self, e: String) -> String {
        self.message = e.clone();
        e
    }

    /// selectGroup: its marks on this sheet if it has some, else on the first sheet that has.
    pub fn select_group(&mut self, id: &str) {
        let all: Vec<(String, String)> = self
            .tk
            .marks()
            .iter()
            .filter(|m| m.get("group").and_then(Value::as_str) == Some(id))
            .map(|m| (s(m.get("id")), s(m.get("sheet"))))
            .collect();
        let Some(first) = all.first() else {
            self.message = t("group.empty");
            return;
        };
        let cur = self.cur_sheet.clone();
        let sheet = if all.iter().any(|(_, sh)| *sh == cur) { cur.clone() } else { first.1.clone() };
        if sheet != cur {
            self.goto = Some(sheet.clone());
        }
        if self.tool != "select" {
            self.set_tool("select");
        }
        let ids: Vec<String> = all.into_iter().filter(|(_, sh)| *sh == sheet).map(|(i, _)| i).collect();
        self.set_selection(&ids);
    }

    /// The bid area and typical group of the given marks (areaField / groupField): "—" when they differ.
    pub(crate) fn area_group_fields(&mut self, ui: &mut egui::Ui, ids: &[String], set: &mut Vec<(String, String)>) {
        let marks: Vec<Value> = ids.iter().filter_map(|i| self.tk.mark(i)).cloned().collect();
        let one = |k: &str| -> Option<String> {
            let first = marks.first().map(|m| s(m.get(k))).unwrap_or_default();
            marks.iter().all(|m| s(m.get(k)) == first).then_some(first)
        };
        let areas: Vec<Value> = self.tk.state.get("areas").and_then(Value::as_array).cloned().unwrap_or_default();
        if !areas.is_empty() {
            let cur = one("area");
            let shown = match &cur {
                None => "—".to_string(),
                Some(c) => areas.iter().find(|a| s(a.get("id")) == *c).map_or_else(|| t("area.none"), |a| s(a.get("name"))),
            };
            let l = ui.label(RichText::new(t("props.area")).small());
            egui::ComboBox::new("tk_m_area", "")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(cur.as_deref() == Some(""), t("area.none")).clicked() {
                        set.push(("area".into(), String::new()));
                    }
                    for a in &areas {
                        let aid = s(a.get("id"));
                        if ui.selectable_label(cur.as_deref() == Some(aid.as_str()), s(a.get("name"))).clicked() {
                            set.push(("area".into(), aid));
                        }
                    }
                })
                .response
                .labelled_by(l.id);
        }
        let groups: Vec<Value> = self.tk.state.get("groups").and_then(Value::as_array).cloned().unwrap_or_default();
        let cur = one("group");
        let opt = |g: &Value| {
            tf("group.option", &[("name", F::S(s(g.get("name")))), ("n", F::N(g.get("repeat").and_then(Value::as_f64).unwrap_or(1.0)))])
        };
        let shown = match &cur {
            None => "—".to_string(),
            Some(c) => groups.iter().find(|g| s(g.get("id")) == *c).map_or_else(|| t("group.none"), opt),
        };
        let l = ui.label(RichText::new(t("props.group")).small()).on_hover_text(t("group.field.title"));
        egui::ComboBox::new("tk_m_group", "")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui.selectable_label(cur.as_deref() == Some(""), t("group.none")).clicked() {
                    set.push(("group".into(), String::new()));
                }
                for g in &groups {
                    let gid = s(g.get("id"));
                    if ui.selectable_label(cur.as_deref() == Some(gid.as_str()), opt(g)).clicked() {
                        set.push(("group".into(), gid));
                    }
                }
                if ui.selectable_label(false, t("group.new")).clicked() {
                    set.push(("group".into(), "new".into()));
                }
            })
            .response
            .labelled_by(l.id);
    }

    /// The typical groups list (renderGroups): name, repeats, how many marks, select, delete.
    pub fn groups_list(&mut self, ui: &mut egui::Ui, tok: &Tokens) {
        let groups: Vec<Value> = self.tk.state.get("groups").and_then(Value::as_array).cloned().unwrap_or_default();
        if groups.is_empty() {
            return;
        }
        ui.label(RichText::new(t("group.h")).strong().color(tok.text_strong));
        let mut ops: Vec<String> = Vec::new();
        for g in &groups {
            let id = s(g.get("id"));
            let n = self.tk.marks().iter().filter(|m| m.get("group").and_then(Value::as_str) == Some(id.as_str())).count();
            ui.horizontal(|ui| {
                for (k, field, w) in [("name", s(g.get("name")), 110.0), ("repeat", s(g.get("repeat")), 40.0)] {
                    let key = format!("g:{id}:{k}");
                    let e = self.edits.entry(key).or_insert_with(|| (field.clone(), field.clone()));
                    if e.0 != field {
                        *e = (field.clone(), field.clone());
                    }
                    let r = ui.add(egui::TextEdit::singleline(&mut e.1).desired_width(w));
                    if r.lost_focus() && e.1 != e.0 {
                        ops.push(format!("{k}:{id}:{}", e.1));
                    }
                    if k == "name" {
                        ui.label("×");
                    }
                }
                #[allow(clippy::cast_precision_loss)]
                ui.label(RichText::new(tn("group.marks", n as f64)).color(tok.text_dim));
                if ui.small_button("⛶").on_hover_text(t("group.select")).clicked() {
                    ops.push(format!("select:{id}"));
                }
                if ui.small_button("✕").on_hover_text(t("group.delete")).clicked() {
                    ops.push(format!("delete:{id}"));
                }
            });
        }
        for op in ops {
            let op = op.replacen("name:", "rename:", 1);
            let _ = self.group_edit(&op);
        }
    }

    /// The New typical group window.
    pub fn group_window(&mut self, ctx: &egui::Context) {
        let Some(ask) = self.group_ask.as_mut() else { return };
        let mut act: Option<bool> = None;
        #[allow(clippy::cast_precision_loss)]
        let text = tn("group.dlg.text", ask.ids.len() as f64);
        egui::Window::new(t("group.dlg")).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(text);
            let l = ui.label(t("group.f.name"));
            ui.add(egui::TextEdit::singleline(&mut ask.name).char_limit(200)).labelled_by(l.id);
            let l = ui.label(t("group.f.repeat"));
            ui.add(egui::TextEdit::singleline(&mut ask.repeat)).labelled_by(l.id);
            if !ask.error.is_empty() {
                ui.label(RichText::new(&ask.error).color(ui.visuals().error_fg_color));
            }
            ui.horizontal(|ui| {
                if ui.button(t("btn.cancel")).clicked() {
                    act = Some(false);
                }
                if ui.button(t("group.dlg.ok")).clicked() {
                    act = Some(true);
                }
            });
        });
        match act {
            Some(true) => {
                let (n, r) = (ask.name.clone(), ask.repeat.clone());
                let _ = self.group_ok(&n, &r);
            }
            Some(false) => self.group_ask = None,
            None => {}
        }
    }
}

fn obj<const N: usize>(pairs: [(&str, Value); N]) -> Map<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeats_are_read_as_the_js_does() {
        assert_eq!(read_repeat(" x3 "), Some(3.0));
        assert_eq!(read_repeat("×12"), Some(12.0));
        assert_eq!(read_repeat("0"), None);
        assert_eq!(read_repeat("10001"), None);
        assert_eq!(read_repeat("2.5"), None);
        assert_eq!(read_repeat("123456"), None);
    }

    #[test]
    fn groups_and_notes() {
        let mut m = TakeoffMode::new("abc");
        m.add_file("abc", "Plan.pdf", 10, 1);
        let sheet = m.sheet(0);
        m.cur_sheet = sheet.clone();
        m.add_item("Outlets", "count");
        m.click(&sheet, [10.0, 10.0]);
        m.click(&sheet, [30.0, 10.0]);
        m.select_all(&sheet);
        m.group_edit("new:Unit A|4").unwrap();
        assert!(m.message.contains("×4"), "{}", m.message);
        assert!(m.group_edit("repeat:g4:0").is_ok());
        m.set_tool("note");
        m.click(&sheet, [100.0, 100.0]);
        m.note_edit("add:   ").unwrap();
        assert_eq!(m.message, t("note.err.empty"));
        m.click(&sheet, [100.0, 100.0]);
        m.note_edit("add:Check this").unwrap();
        assert_eq!(m.tk.state["notes"].as_array().map(Vec::len), Some(1));
        m.note_edit("status:closed").unwrap();
        m.delete_selected();
        assert!(m.tk.state["notes"].as_array().is_none_or(Vec::is_empty));
    }
}
