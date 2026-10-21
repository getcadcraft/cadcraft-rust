//! MarkupCraft's cut-outs, spaces, legends, sheet links and punch list (M9.18), ported from markupcraft.js: Add a
//! cut-out to an area (startCutout / finishCutout, holes.js: one undo step, the area's new value in the status line) and
//! remove them; Spaces (startSpace / finishSpace: a named outline; markups are "in" the space holding the middle of
//! their bounds — spaces.js; the Spaces list, the list's space filter, the space row in Properties and the exports);
//! the legend (addLegend: placed at the top right of what is on screen, counting each kind of markup on the sheet or
//! the whole set — legend.js, title and scope in Properties); sheet links (a markup's Link to a sheet by number or title,
//! followed with Ctrl+click or Go to, Alt+← back — link.js; saved as a PDF link when the sheet is in the same file); and
//! the punch list (addToPunch: numbered items in sheet order; assignee, due date and item status; the Punch list tab and
//! its CSV). Each action is the JS handler with the 0.5 undo label and status line.

use cc_markup::i18n::{Fill, t, t0};
use cc_markup::link::LinkSheet;
use cc_markup::model::{Markup, PUNCH_STATUSES, type_name};
use egui::RichText;
use serde_json::{Map, Value, json};

use crate::pdfview::PdfView;
use crate::theme::Tokens;

type P = [f64; 2];

/// The most sheets "Alt+←" remembers (`backStack`).
const BACK_MAX: usize = 50;

impl PdfView {
    /// The sheets as link.js wants them, with each one's file id and page index.
    pub fn link_sheets(&self) -> Vec<(LinkSheet, String, usize)> {
        self.sheet_list()
            .into_iter()
            .map(|s| (LinkSheet { key: s.key, sheet_no: s.info.sheet_no, title: s.info.title }, s.file.id.clone(), s.index))
            .collect()
    }

    // ---- cut-outs ----

    /// startCutout: the Area tool draws a cut-out inside the selected area.
    pub fn start_cutout(&mut self) -> Result<(), String> {
        let id = self.tools.selected().map(str::to_string).ok_or("markup.cutout: select an area")?;
        let m = self.store.get(&id).filter(|m| m.kind == "area" && !m.locked).cloned().ok_or("markup.cutout: select an area")?;
        let key = self.sheet_key();
        self.tools.set_tool(&mut self.store, &key, "area");
        self.tools.cut_for = Some(m.id.clone());
        self.tools.sel = vec![m.id];
        self.tools.message = t0("hole.hint");
        Ok(())
    }

    /// Remove the selected area's cut-outs (one undo step).
    pub fn remove_cutouts(&mut self) -> Result<(), String> {
        let id = self.tools.selected().map(str::to_string).ok_or("markup.cutout: select an area")?;
        let n = self.store.get(&id).filter(|m| m.kind == "area" && !m.locked).map(|m| m.holes.as_ref().map_or(0, Vec::len)).unwrap_or(0);
        if n == 0 {
            return Ok(());
        }
        self.tick();
        let mut patch = Map::new();
        patch.insert("holes".into(), json!([]));
        self.store.change(&t0("undo.hole.remove"), |tx| Ok(tx.update(&id, &patch, true))).map_err(|e| e.to_string())?;
        self.tools.message = t("hole.removed", &[("n", Fill::N(n as f64))]);
        Ok(())
    }

    // ---- spaces ----

    /// startSpace: the Polygon tool draws the space's outline; it is named when it is finished.
    pub fn start_space(&mut self) {
        let key = self.sheet_key();
        self.tools.set_tool(&mut self.store, &key, "polygon");
        self.tools.space_draw = true;
        self.tools.message = t0("space.hint");
    }

    /// An outline finished for a space asks for its name.
    pub fn take_space(&mut self) {
        if let Some(pts) = self.tools.space_pts.take() {
            self.dlg = Some(crate::markupedit::Dialog::SpaceName { pts, name: String::new(), error: String::new() });
        }
    }

    /// finishSpace's Add space: the space with its name (one undo step); the status line says how many markups it holds.
    pub fn add_space(&mut self, pts: Vec<P>, name: &str) -> Result<(), String> {
        let label: String = cc_markup::js::trim(name).chars().take(60).collect();
        if label.is_empty() {
            return Err(t0("space.name.need"));
        }
        self.tick();
        let key = self.sheet_key();
        let mut g = Map::new();
        g.insert("pts".into(), json!(pts));
        g.insert("label".into(), json!(label));
        self.tools.rot = self.page_rot();
        let m = self.tools.new_markup(&mut self.store, &key, "space", g);
        let m = self.store.change(&t0("undo.space"), |tx| tx.add(m)).map_err(|e| e.to_string())?;
        self.tools.sel = vec![m.id.clone()];
        let n = self.store.on_sheet(m.sheet()).into_iter().filter(|x| x.kind != "space" && self.markup_space(x) == m.label).count();
        self.tools.message = t("space.added", &[("name", Fill::S(m.label.clone())), ("n", Fill::N(n as f64))]);
        self.tools.set_tool(&mut self.store, &key, "select");
        Ok(())
    }

    /// The Spaces list (renderSpaces): by sheet, then name; a click shows the space.
    pub fn spaces_panel(&mut self, ui: &mut egui::Ui, tok: &Tokens) {
        ui.label(RichText::new(t0("space.h")).strong().color(tok.text_strong)).on_hover_text(t0("space.h.title"));
        let keys: Vec<String> = self.sheet_keys();
        let order = |m: &Markup| keys.iter().position(|k| k == m.sheet()).map_or(-1.0, |i| i as f64);
        let mut spaces: Vec<Markup> = self.store.all().into_iter().filter(|m| m.kind == "space").cloned().collect();
        cc_m3d::js::sort_by(&mut spaces, |a, b| {
            let d = order(a) - order(b);
            if d != 0.0 { d } else { cc_markup::collate::locale_compare(&a.label, &b.label) }
        });
        let mut go: Option<String> = None;
        for sp in &spaces {
            let n = self.store.on_sheet(sp.sheet()).into_iter().filter(|x| x.kind != "space" && self.markup_space(x) == sp.label).count();
            let no = self.locate(sp.sheet()).map(|(f, i)| f.info(&self.store, i).sheet_no).unwrap_or_default();
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(self.tools.sel.contains(&sp.id), &sp.label)
                    .on_hover_text(t("space.go.title", &[("name", Fill::S(sp.label.clone()))]))
                    .clicked()
                {
                    go = Some(sp.id.clone());
                }
                ui.label(RichText::new(format!("{no} {n}")).small().color(tok.text_dim));
            });
        }
        if ui.button(t0("space.new")).clicked() {
            self.start_space();
        }
        if let Some(id) = go
            && let Some(m) = self.store.get(&id).cloned()
            && let Some((fid, i)) = self.locate(m.sheet()).map(|(f, i)| (f.id.clone(), i))
        {
            if fid != self.cur.id || i != self.page {
                self.go_file(&fid, i);
            }
            self.tools.sel = vec![id];
            if let Some(b) = cc_markup::geom::bounds_of(&m, &self.env_of(&m)) {
                self.zoom_to(b, 0.7);
            }
        }
    }

    // ---- legends ----

    /// addLegend: a legend at the top right of what is on screen (`at` = its top-right corner in user space; else the
    /// view's), 230 × 200 screen pixels.
    pub fn add_legend(&mut self, at: Option<P>) {
        let c1 = at.unwrap_or_else(|| {
            let sz = self.view_size.unwrap_or(egui::vec2(0.0, 0.0));
            self.user_of([f64::from(sz.x - 24.0 - self.pan.x), f64::from(24.0 - self.pan.y)])
        });
        let s1 = self.screen_of(c1);
        let c0 = self.user_of([s1[0] - 230.0, s1[1] + 200.0]);
        self.tick();
        let key = self.sheet_key();
        let mut g = Map::new();
        g.insert("rect".into(), json!([c0[0].min(c1[0]), c0[1].min(c1[1]), c0[0].max(c1[0]), c0[1].max(c1[1])]));
        g.insert("label".into(), json!(t0("legend.title")));
        g.insert("author".into(), json!(self.tools.author));
        self.tools.rot = self.page_rot();
        let m = self.tools.new_markup(&mut self.store, &key, "legend", g);
        match self.store.change(&t0("undo.legend"), |tx| tx.add(m)) {
            Ok(m) => {
                self.tools.sel = vec![m.id];
                self.tools.message = t0("legend.added");
            }
            Err(e) => self.tools.message = e.to_string(),
        }
    }

    // ---- links ----

    /// followLink: the sheet a markup links to.
    pub fn follow_link(&mut self, id: &str) -> Result<(), String> {
        let m = self.store.get(id).cloned().ok_or_else(|| format!("markup.link: no markup {id}"))?;
        let sheets = self.link_sheets();
        let list: Vec<LinkSheet> = sheets.iter().map(|s| s.0.clone()).collect();
        let Some((target, fid, i)) = cc_markup::link::link_target(&m.link, &list).and_then(|k| sheets.get(k)).cloned() else {
            self.tools.message = t("link.missing", &[("sheet", Fill::S(m.link.clone()))]);
            return Ok(());
        };
        let from = self.sheet_key();
        if target.key == from {
            self.tools.message = t("link.here", &[("sheet", Fill::S(target.title))]);
            return Ok(());
        }
        self.back_stack.push(from);
        if self.back_stack.len() > BACK_MAX {
            self.back_stack.remove(0);
        }
        self.tools.sel.clear();
        self.go_file(&fid, i);
        self.tools.message = t("link.went", &[("sheet", Fill::S(target.title))]);
        Ok(())
    }

    /// goBack (Alt+←): the sheet a link was followed from.
    pub fn go_back(&mut self) {
        let k = self.back_stack.pop().unwrap_or_default();
        match self.locate(&k).map(|(f, i)| (f.id.clone(), i, f.title(&self.store, i))) {
            Some((fid, i, title)) => {
                self.tools.sel.clear();
                self.go_file(&fid, i);
                self.tools.message = t("link.back", &[("sheet", Fill::S(title))]);
            }
            None => self.tools.message = t0("link.back.none"),
        }
    }

    // ---- punch list ----

    /// addToPunch: the selected markups (not read-only, not on it yet) as numbered items, in sheet order.
    pub fn add_to_punch(&mut self, ids: &[String]) {
        let keys: Vec<String> = self.sheet_keys();
        let order = |m: &Markup| keys.iter().position(|k| k == m.sheet()).map_or(-1.0, |i| i as f64);
        let mut list: Vec<Markup> =
            ids.iter().filter_map(|id| self.store.get(id)).filter(|m| !m.locked && m.punch.is_none()).cloned().collect();
        cc_m3d::js::sort_by(&mut list, |a, b| {
            let d = order(a) - order(b);
            if d != 0.0 { d } else { cc_markup::collate::locale_compare(&a.created, &b.created) }
        });
        if list.is_empty() {
            self.tools.message = t0(if ids.is_empty() { "punch.add.need" } else { "punch.add.already" });
            return;
        }
        self.tick();
        let r = self.store.change(&t0("punch.add.one"), |tx| {
            let mut n = tx.punch_next();
            for m in &list {
                let mut patch = Map::new();
                patch.insert("punch".into(), json!({ "n": n, "assignee": "", "due": "", "status": "Open" }));
                tx.update(&m.id, &patch, true);
                n += 1.0;
            }
            tx.set_punch_next(n);
            Ok(())
        });
        self.tools.message = match r {
            Ok(()) => t("punch.added", &[("n", Fill::N(list.len() as f64))]),
            Err(e) => e.to_string(),
        };
    }

    /// A punch item's field (`assignee`, `due` as YYYY-MM-DD, `status`) or `remove` (off the punch list).
    pub fn punch_edit(&mut self, id: &str, field: &str, v: &str) -> Result<(), String> {
        let m = self.store.get(id).filter(|m| !m.locked).cloned().ok_or_else(|| format!("markup.punch: no markup {id}"))?;
        let p = m.punch.clone().ok_or("markup.punch: the markup is not on the punch list")?;
        self.tick();
        let mut item = json!({ "n": p.n, "assignee": p.assignee, "due": p.due, "status": p.status });
        let label = if field == "remove" {
            item = Value::Null;
            t0("punch.remove")
        } else {
            let val = match field {
                "assignee" => json!(v.chars().take(80).collect::<String>()),
                "due" => json!(v),
                "status" if PUNCH_STATUSES.contains(&v) => json!(v),
                _ => return Err(format!("markup.punch: unknown field {field}={v}")),
            };
            if let Some(o) = item.as_object_mut() {
                o.insert(field.into(), val);
            }
            t0("undo.punch")
        };
        let mut patch = Map::new();
        patch.insert("punch".into(), item);
        self.store.change(&label, |tx| Ok(tx.update(id, &patch, true))).map(|_| ()).map_err(|e| e.to_string())
    }

    /// exportPunchCsv: the punch list as CSV (to the host's save).
    pub fn export_punch(&mut self) {
        let items: Vec<Markup> = self.store.punch_items().into_iter().cloned().collect();
        if items.is_empty() {
            self.tools.message = t0("punch.export.empty");
            return;
        }
        let refs: Vec<&Markup> = items.iter().collect();
        let rows = cc_markup::csv::punch_rows(&refs, &|m: &Markup| {
            self.locate(m.sheet()).map(|(f, i)| cc_markup::csv::SheetRef {
                title: f.title(&self.store, i),
                file: f.name.clone(),
                page: Some(cc_markup::csv::Cell::N((i + 1) as f64)),
            })
        });
        let name = format!("{}.csv", t("punch.file", &[("set", Fill::S(self.set_name()))]));
        self.outbox.push((name, cc_markup::csv::to_csv(&rows).into_bytes()));
        self.tools.message = t("punch.exported", &[("n", Fill::N(items.len() as f64))]);
    }

    /// The punch fields of one item (punchFields): assignee, due, item status.
    pub fn punch_fields(&mut self, ui: &mut egui::Ui, m: &Markup) {
        let Some(p) = m.punch.clone() else { return };
        let mut set: Option<(&str, String)> = None;
        let l = ui.label(RichText::new(t0("punch.assignee")).small());
        let e = self.edits.entry(format!("{}:assignee", m.id)).or_insert_with(|| (p.assignee.clone(), p.assignee.clone()));
        if e.0 != p.assignee {
            *e = (p.assignee.clone(), p.assignee.clone());
        }
        let r = ui.text_edit_singleline(&mut e.1).labelled_by(l.id);
        if r.lost_focus() && e.1 != e.0 {
            set = Some(("assignee", e.1.clone()));
        }
        let l = ui.label(RichText::new(t0("punch.due")).small());
        let due =
            if p.due.len() == 10 && p.due.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() }) {
                p.due.clone()
            } else {
                String::new()
            };
        let e = self.edits.entry(format!("{}:due", m.id)).or_insert_with(|| (due.clone(), due.clone()));
        if e.0 != due {
            *e = (due.clone(), due.clone());
        }
        let r = ui.add(egui::TextEdit::singleline(&mut e.1).hint_text("YYYY-MM-DD")).labelled_by(l.id);
        if r.lost_focus() && e.1 != e.0 {
            set = Some(("due", e.1.clone()));
        }
        let l = ui.label(RichText::new(t0("punch.item_status")).small());
        egui::ComboBox::new(format!("mc-punch-st-{}", m.id), "")
            .selected_text(cc_markup::model::punch_status_name(&p.status))
            .show_ui(ui, |ui| {
                for st in PUNCH_STATUSES {
                    if ui.selectable_label(p.status == st, cc_markup::model::punch_status_name(st)).clicked() && p.status != st {
                        set = Some(("status", st.to_string()));
                    }
                }
            })
            .response
            .labelled_by(l.id);
        if let Some((f, v)) = set
            && let Err(e) = self.punch_edit(&m.id, f, &v)
        {
            self.tools.message = e;
        }
    }

    /// The Punch list tab (renderPunch): Add selected, Export CSV, each item with its fields (a click jumps to it).
    pub fn punch_panel(&mut self, ui: &mut egui::Ui, tok: &Tokens) {
        ui.label(RichText::new(t0("punch.note")).small().color(tok.text_dim));
        let (mut add, mut export) = (false, false);
        ui.horizontal_wrapped(|ui| {
            add = ui.button(t0("punch.add")).clicked();
            export = ui.button(t0("punch.csv")).clicked();
        });
        if add {
            let sel = self.tools.sel.clone();
            self.add_to_punch(&sel);
        }
        if export {
            self.export_punch();
        }
        let items: Vec<Markup> = self.store.punch_items().into_iter().cloned().collect();
        if items.is_empty() {
            ui.label(RichText::new(t0("punch.empty")).color(tok.text_dim));
        }
        let mut jump: Option<String> = None;
        for m in &items {
            let Some(p) = m.punch.clone() else { continue };
            let title = if m.subject.is_empty() { type_name(&m.kind) } else { m.subject.clone() };
            if ui
                .selectable_label(self.tools.sel.contains(&m.id), RichText::new(format!("{}  {title}", cc_markup::js::ns(p.n))).strong())
                .clicked()
            {
                jump = Some(m.id.clone());
            }
            let text = if crate::markuptools::text_type(&m.kind) { m.text.clone() } else { m.comment.clone() };
            if !text.is_empty() {
                ui.label(RichText::new(text).small());
            }
            let sheet = self.locate(m.sheet()).map(|(f, i)| f.title(&self.store, i)).unwrap_or_default();
            let mut meta = vec![sheet];
            if !p.assignee.is_empty() {
                meta.push(t("punch.assigned", &[("name", Fill::S(p.assignee.clone()))]));
            }
            if !p.due.is_empty() {
                let d = cc_markup::js::short_date_en(
                    self.zone.local_ms(cc_markup::js::parse_date_in(&format!("{}T12:00:00", p.due), self.zone)),
                );
                meta.push(t("punch.due_on", &[("date", Fill::S(d))]));
            }
            meta.retain(|s| !s.is_empty());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(meta.join(", ")).small().color(tok.text_dim));
                ui.label(RichText::new(cc_markup::model::punch_status_name(&p.status)).small().color(tok.accent));
            });
            self.punch_fields(ui, m);
            ui.separator();
        }
        if let Some(id) = jump {
            let _ = self.jump_to(&id);
        }
    }

    /// Properties for one markup, the M9.18 parts: link, legend title and scope, space name, the punch item, cut-outs.
    pub fn props_extra(&mut self, ui: &mut egui::Ui, tok: &Tokens, m: &Markup) {
        let mut prop: Option<(&str, String)> = None;
        // the link to a sheet
        let sheets = self.link_sheets();
        let list: Vec<LinkSheet> = sheets.iter().map(|s| s.0.clone()).collect();
        let target = (!m.link.is_empty()).then(|| cc_markup::link::link_target(&m.link, &list)).flatten();
        let shown = match target.and_then(|k| list.get(k)) {
            Some(s) => s.title.clone(),
            None if !m.link.is_empty() => t("link.not_open", &[("sheet", Fill::S(m.link.clone()))]),
            None => t0("link.none"),
        };
        let l = ui.label(RichText::new(t0("props.link")).small());
        egui::ComboBox::new("mc-p-link", "")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui.selectable_label(m.link.is_empty(), t0("link.none")).clicked() && !m.link.is_empty() {
                    prop = Some(("link", String::new()));
                }
                for (k, s) in list.iter().enumerate() {
                    let v = if s.sheet_no.is_empty() { s.title.clone() } else { s.sheet_no.clone() };
                    if ui.selectable_label(target == Some(k), &s.title).clicked() {
                        prop = Some(("link", v));
                    }
                }
            })
            .response
            .labelled_by(l.id);
        if !m.link.is_empty() && ui.button(t("link.go", &[("sheet", Fill::S(m.link.clone()))])).on_hover_text(t0("link.go.title")).clicked()
        {
            let _ = self.follow_link(&m.id);
        }
        if m.kind == "legend" {
            let l = ui.label(RichText::new(t0("legend.title.field")).small());
            let e = self.edits.entry(format!("{}:title", m.id)).or_insert_with(|| (m.label.clone(), m.label.clone()));
            if e.0 != m.label {
                *e = (m.label.clone(), m.label.clone());
            }
            let r = ui.text_edit_singleline(&mut e.1).labelled_by(l.id);
            if r.lost_focus() && e.1 != e.0 {
                prop = Some(("title", e.1.clone()));
            }
            let scope = m.legend.clone().unwrap_or_default();
            let l = ui.label(RichText::new(t0("legend.scope")).small());
            egui::ComboBox::new("mc-p-scope", "")
                .selected_text(t0(if scope == "set" { "legend.scope.set" } else { "legend.scope.sheet" }))
                .show_ui(ui, |ui| {
                    for (v, k) in [("sheet", "legend.scope.sheet"), ("set", "legend.scope.set")] {
                        if ui.selectable_label((scope == "set") == (v == "set"), t0(k)).clicked() {
                            prop = Some(("scope", v.to_string()));
                        }
                    }
                })
                .response
                .labelled_by(l.id);
        }
        if m.kind == "space" {
            let l = ui.label(RichText::new(t0("space.name.field")).small());
            let e = self.edits.entry(format!("{}:space", m.id)).or_insert_with(|| (m.label.clone(), m.label.clone()));
            if e.0 != m.label {
                *e = (m.label.clone(), m.label.clone());
            }
            let r = ui.text_edit_singleline(&mut e.1).labelled_by(l.id);
            if r.lost_focus() && e.1 != e.0 {
                prop = Some(("label", e.1.clone()));
            }
        }
        if let Some((k, v)) = prop
            && let Err(e) = self.prop(k, &v)
        {
            self.tools.message = e;
        }
        // the punch item
        ui.add_space(4.0);
        ui.label(RichText::new(t0("tab.punch")).strong().color(tok.text_strong));
        if let Some(p) = &m.punch {
            ui.label(format!("{} {}", t0("punch.number"), cc_markup::js::ns(p.n)));
            self.punch_fields(ui, m);
            if ui.link(t0("punch.remove")).clicked()
                && let Err(e) = self.punch_edit(&m.id, "remove", "")
            {
                self.tools.message = e;
            }
        } else if ui.button(t0("punch.add.one")).clicked() {
            self.add_to_punch(std::slice::from_ref(&m.id));
        }
        // cut-outs of an area
        if m.kind == "area" {
            ui.horizontal_wrapped(|ui| {
                if ui.button(t0("hole.add")).clicked()
                    && let Err(e) = self.start_cutout()
                {
                    self.tools.message = e;
                }
                let n = m.holes.as_ref().map_or(0, Vec::len);
                if n > 0 && ui.button(t("hole.remove", &[("n", Fill::N(n as f64))])).clicked() {
                    let _ = self.remove_cutouts();
                }
            });
        }
    }
}

/// Timings of MarkupCraft's M9.16–M9.18 actions (cadcraft-shot mkactbench; forks/tools/mkbench.mjs --actions runs the
/// JS the same way): 200 rectangles on page 1, then copy + paste them, duplicate, select all, the list's CSV / XLSX /
/// PDF summary, rotate the page (bake, rotate, read back), extract it, add them all to the punch list, export the punch
/// CSV, export the tool chest, a legend counting them saved. Best of `runs`, ms, as JSON.
pub fn bench_actions(name: &str, bytes: &[u8], runs: usize) -> Result<String, String> {
    let fresh = || -> Result<PdfView, String> {
        let mut v = PdfView::open(name, bytes)?;
        v.set_zoom(1.0);
        v.clock = Some(1_791_810_000_000.0);
        let key = v.sheet_key();
        let ms: Vec<cc_markup::model::Markup> = (0..200u32)
            .map(|i| {
                let (cx, cy) = (f64::from(i % 20) * 25.0, f64::from(i / 20) * 25.0);
                let mut f = Map::new();
                f.insert("sheet".into(), json!(key));
                f.insert("type".into(), json!("rect"));
                f.insert("rect".into(), json!([100.0 + cx, 100.0 + cy, 120.0 + cx, 120.0 + cy]));
                cc_markup::model::make_markup(&mut v.store.env, &f)
            })
            .collect();
        let _ = v.store.change("bench", |tx| {
            for m in ms {
                tx.add(m)?;
            }
            Ok(())
        });
        v.select_all();
        Ok(v)
    };
    let mut out: Vec<(String, f64)> = Vec::new();
    let mut time = |label: &str, f: &mut dyn FnMut(&mut PdfView) -> Result<(), String>| -> Result<(), String> {
        let mut b = f64::INFINITY;
        for _ in 0..runs.max(1) {
            let mut v = fresh()?;
            let t0 = web_time::Instant::now();
            f(&mut v)?;
            b = b.min(t0.elapsed().as_secs_f64() * 1000.0);
        }
        out.push((label.to_string(), b));
        Ok(())
    };
    time("copy_paste", &mut |v| {
        v.copy("copy");
        v.paste("place", None);
        Ok(())
    })?;
    time("duplicate", &mut |v| {
        v.duplicate();
        Ok(())
    })?;
    time("select_all", &mut |v| {
        v.select_all();
        Ok(())
    })?;
    for k in ["csv", "xlsx", "pdf"] {
        time(&format!("list_{k}"), &mut |v| v.export_list(k).map(|_| ()))?;
    }
    time("rotate", &mut |v| v.pages_op("rotate-cw", "", None))?;
    time("extract", &mut |v| v.pages_op("extract", "1", None))?;
    time("punch_add", &mut |v| {
        let sel = v.tools.sel.clone();
        v.add_to_punch(&sel);
        Ok(())
    })?;
    time("punch_csv", &mut |v| {
        let sel = v.tools.sel.clone();
        v.add_to_punch(&sel);
        v.export_punch();
        Ok(())
    })?;
    time("chest_export", &mut |v| {
        v.export_chest();
        Ok(())
    })?;
    time("legend_save", &mut |v| {
        v.add_legend(Some([700.0, 700.0]));
        v.save_marked(0.0).map(|_| ())
    })?;
    Ok(format!("{{{}}}", out.iter().map(|(k, ms)| format!("\"{k}\":{ms:.3}")).collect::<Vec<_>>().join(",")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(pages: u32) -> PdfView {
        let pdf = cc_io_markup::pages::blank_pdf(Some(&json!({ "paper": "letter", "grid": "none", "pages": pages })), "", 0.0);
        let mut v = PdfView::open("a.pdf", &pdf).unwrap();
        v.set_zoom(1.0);
        v
    }
    fn click(v: &mut PdfView, p: P) {
        v.gesture_down(p, v.screen_of(p), false, false);
        v.gesture_up(p, v.screen_of(p), false);
    }
    fn outline(v: &mut PdfView, pts: &[P]) {
        for p in pts {
            click(v, *p);
        }
        let key = v.sheet_key();
        v.tools.enter(&mut v.store, &key);
    }

    #[test]
    fn a_cut_out_goes_inside_the_area_in_one_step() {
        let mut v = view(1);
        let key = v.sheet_key();
        v.tools.set_scale(&mut v.store, &key, Some(cc_markup::units::preset_calibration("1:100", 100.0, "m")));
        v.tools.set_tool(&mut v.store, &key, "area");
        outline(&mut v, &[[100.0, 100.0], [300.0, 100.0], [300.0, 300.0], [100.0, 300.0]]);
        let id = v.tools.sel[0].clone();
        v.start_cutout().unwrap();
        outline(&mut v, &[[150.0, 150.0], [200.0, 150.0], [200.0, 200.0]]);
        assert_eq!(v.store.get(&id).unwrap().holes.as_ref().map(Vec::len), Some(1));
        assert_eq!(v.tools.tool, "select");
        // one outside: refused with the reason
        v.start_cutout().unwrap();
        outline(&mut v, &[[350.0, 150.0], [400.0, 150.0], [400.0, 200.0]]);
        assert_eq!(v.tools.message, t0("hole.err.outside"));
        v.remove_cutouts().unwrap();
        assert!(v.store.get(&id).unwrap().holes.as_ref().is_none_or(Vec::is_empty));
    }

    #[test]
    fn spaces_hold_markups_and_legends_count_them() {
        let mut v = view(1);
        let key = v.sheet_key();
        v.tools.set_tool(&mut v.store, &key, "rect");
        v.gesture_down([150.0, 150.0], v.screen_of([150.0, 150.0]), false, false);
        v.tools.mv(&v.store, [180.0, 180.0], v.screen_of([180.0, 180.0]), false);
        v.gesture_up([180.0, 180.0], v.screen_of([180.0, 180.0]), false);
        v.start_space();
        outline(&mut v, &[[100.0, 100.0], [300.0, 100.0], [300.0, 300.0]]);
        v.take_space();
        let Some(crate::markupedit::Dialog::SpaceName { pts, .. }) = v.dlg.clone() else { panic!("no name asked") };
        v.add_space(pts, "Kitchen").unwrap();
        assert_eq!(v.tools.message, t("space.added", &[("name", Fill::S("Kitchen".into())), ("n", Fill::N(1.0))]));
        v.add_legend(Some([500.0, 700.0]));
        let lg = v.store.get(&v.tools.sel[0]).unwrap().clone();
        assert_eq!(lg.kind, "legend");
        assert!(!crate::markupedit::legend_of(&v.store, &lg).is_empty());
    }

    #[test]
    fn links_go_and_come_back_and_punch_items_are_numbered() {
        let mut v = view(2);
        let key = v.sheet_key();
        v.tools.set_tool(&mut v.store, &key, "rect");
        v.gesture_down([150.0, 150.0], v.screen_of([150.0, 150.0]), false, false);
        v.tools.mv(&v.store, [180.0, 180.0], v.screen_of([180.0, 180.0]), false);
        v.gesture_up([180.0, 180.0], v.screen_of([180.0, 180.0]), false);
        let id = v.tools.sel[0].clone();
        let title = v.cur.title(&v.store, 1);
        v.prop("link", &title).unwrap();
        v.follow_link(&id).unwrap();
        assert_eq!(v.page, 1);
        v.go_back();
        assert_eq!(v.page, 0);
        v.add_to_punch(std::slice::from_ref(&id));
        assert_eq!(v.store.get(&id).unwrap().punch.as_ref().map(|p| p.n), Some(1.0));
        v.punch_edit(&id, "assignee", "Sam").unwrap();
        v.punch_edit(&id, "status", "Closed").unwrap();
        v.export_punch();
        assert_eq!(v.outbox.len(), 1);
        v.add_to_punch(std::slice::from_ref(&id));
        assert_eq!(v.tools.message, t0("punch.add.already"));
    }
}
