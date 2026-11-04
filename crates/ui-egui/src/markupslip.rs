//! MarkupCraft's slip-sheet, flattened copy, print and Close the set (M9.25), ported from markupcraft.js: slipSheet (the
//! revised set opens beside the old one, each new sheet is paired with the old sheet of the same number — cc-markup
//! slip `match_sheets` — and, in one undo step, the old sheets' markups are carried onto the new ones with their
//! replies, statuses and punch items, the scale copied where the paper is the same, the old sheets stamped
//! "Superseded"), saveFlat (a copy of the open file with every markup, or the selected ones, drawn into its pages),
//! printSheets (this sheet, this file, the whole set or some pages, the markups drawn in, one PDF handed to the host to
//! print) and closeSet (the unsaved question, then everything closed). Each action is the JS handler with 0.5's words.

use cc_io_markup::write::Flatten;
use cc_markup::i18n::{Fill, t, t0};
use cc_markup::slip::{SlipSheet, carry_markups, match_sheets, superseded_stamp};
use egui::RichText;
use serde_json::{Map, Value};

use crate::markupfind::FDialog;
use crate::pdfview::PdfView;
use crate::theme::Tokens;

/// What the host does for the sheet view after a command: show a PDF to print, or close the set (after saving the
/// files with changes when `save`).
#[derive(Debug, Clone)]
pub enum HostAct {
    Print(String, Vec<u8>),
    Close { save: bool },
}

/// One row of the slip-sheet review: the new sheet, the old one it replaces (if any) and how they were paired.
#[derive(Debug, Clone)]
pub struct SlipRow {
    pub new_key: String,
    pub old_key: Option<String>,
    pub text: String,
}

/// A file name without ".pdf" and " (marked up)" (case ignored), as saveFiles / saveFlat name copies.
fn stem(name: &str) -> String {
    let strip = |s: &str, suf: &str| -> String {
        if s.len() >= suf.len() && s.get(s.len() - suf.len()..).is_some_and(|e| e.eq_ignore_ascii_case(suf)) {
            s.get(..s.len() - suf.len()).unwrap_or(s).to_string()
        } else {
            s.to_string()
        }
    };
    strip(&strip(name, ".pdf"), " (marked up)")
}

impl PdfView {
    /// `sheetFacts(s)` of every sheet of the files `ids` (all files when None), in `sheets` order.
    fn slip_facts(&self, ids: Option<&[String]>) -> Vec<SlipSheet> {
        let files = self.files();
        self.sheet_list()
            .into_iter()
            .filter(|s| ids.is_none_or(|l| l.contains(&s.file.id)))
            .map(|s| {
                let fi = files.iter().position(|f| f.id == s.file.id).map_or(-1.0, |i| i as f64);
                let (view, rotate) = s.file.page_views.get(s.index).copied().unwrap_or(([0.0, 0.0, 0.0, 0.0], 0.0));
                SlipSheet {
                    key: s.key,
                    no: s.info.sheet_no,
                    file: fi,
                    page: s.index as f64,
                    width: view[2] - view[0],
                    height: view[3] - view[1],
                    view,
                    rotate,
                }
            })
            .collect()
    }

    /// A sheet's title (`title(key)`).
    fn key_title(&self, key: &str) -> String {
        self.locate(key).map_or_else(|| key.to_string(), |(f, i)| f.title(&self.store, i))
    }

    // ---- slip-sheet ----

    /// slipSheet(list): the revised files join the set; the review of which old sheet each new one replaces opens.
    pub fn slip_open(&mut self, list: &[(String, Vec<u8>)]) {
        self.settle();
        let old = self.slip_facts(None);
        let before: Vec<String> = self.files().iter().map(|f| f.id.clone()).collect();
        let mut errors: Vec<String> = Vec::new();
        let (mut opened, mut read) = (0usize, 0usize);
        for (name, bytes) in list {
            let n0 = self.store.markups.len();
            match self.add_file(name, bytes) {
                Ok(()) => {
                    opened = opened.saturating_add(1);
                    read = read.saturating_add(self.store.markups.len().saturating_sub(n0));
                }
                Err(e) => errors.push(e),
            }
        }
        // openFiles' status line
        let mut parts: Vec<String> = Vec::new();
        if opened > 0 {
            let n = self.sheet_keys().len() as f64;
            parts.push(t("open.done", &[("n", Fill::N(opened as f64)), ("sheets", Fill::S(t("open.done.sheets", &[("n", Fill::N(n))])))]));
        }
        if read > 0 {
            parts.push(t(if opened == 1 { "open.read.one_file" } else { "open.read" }, &[("n", Fill::N(read as f64))]));
        }
        self.tools.message = parts.join(" ");
        if !errors.is_empty() {
            self.note = errors.join("\n");
            self.fdlg = Some(FDialog::Notice { title: t0("open.failed.title"), text: errors.join("\n") });
        }
        if opened == 0 {
            return;
        }
        let new_ids: Vec<String> = self.files().iter().map(|f| f.id.clone()).filter(|id| !before.contains(id)).collect();
        let news = self.slip_facts(Some(&new_ids));
        let pairs = match_sheets(&old, &news);
        let matched = pairs.iter().filter(|p| p.old_key.is_some()).count();
        if matched == 0 {
            self.notice_slip(t0("slip.title"), t("slip.none", &[("n", Fill::N(news.len() as f64))]));
            return;
        }
        let movable = |key: &str| self.store.on_sheet(key).into_iter().filter(|m| m.kind != "foreign" && !m.locked).count();
        let rows: Vec<SlipRow> = pairs
            .iter()
            .map(|p| {
                let text = match &p.old_key {
                    Some(o) => t(
                        if p.by == Some("order") { "slip.row.order" } else { "slip.row" },
                        &[("old", Fill::S(self.key_title(o))), ("n", Fill::N(movable(o) as f64))],
                    ),
                    None => t0("slip.row.new"),
                };
                SlipRow { new_key: p.new_key.clone(), old_key: p.old_key.clone(), text }
            })
            .collect();
        let text = t("slip.text", &[("n", Fill::N(matched as f64)), ("total", Fill::N(news.len() as f64))]);
        let lines: Vec<String> = rows.iter().map(|r| format!("{} {}", self.key_title(&r.new_key), r.text)).collect();
        self.note = std::iter::once(text.clone()).chain(lines).collect::<Vec<_>>().join(" | ");
        let checked = rows.iter().map(|r| r.old_key.is_some()).collect();
        self.fdlg = Some(FDialog::Slip { text, rows, checked, skip: false, stamp: true });
    }

    fn notice_slip(&mut self, title: String, text: String) {
        self.note = text.clone();
        self.fdlg = Some(FDialog::Notice { title, text });
    }

    /// The review's buttons and boxes: `skip:on|off`, `stamp:on|off`, `ok` (as ticked), `ok:I,J` (only those rows),
    /// `cancel` (the files stay open, nothing carried).
    pub fn slip_act(&mut self, act: &str) -> Result<(), String> {
        let Some(FDialog::Slip { text, rows, mut checked, mut skip, mut stamp }) = self.fdlg.clone() else {
            return Err("markup.slip: no revised set is being reviewed (markup.slip=open:PATH first)".into());
        };
        let on = |v: &str| matches!(v, "on" | "true" | "1");
        match act.split_once(':').unwrap_or((act, "")) {
            ("skip", v) => skip = on(v),
            ("stamp", v) => stamp = on(v),
            ("cancel", _) => {
                self.fdlg = None;
                self.tools.message = t0("slip.opened_only");
                return Ok(());
            }
            ("ok", list) => {
                if !list.is_empty() {
                    let pick: Vec<usize> = list.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                    for (i, c) in checked.iter_mut().enumerate() {
                        *c = pick.contains(&i) && rows.get(i).is_some_and(|r| r.old_key.is_some());
                    }
                }
                self.fdlg = None;
                return self.slip_carry(&rows, &checked, skip, stamp);
            }
            (k, _) => return Err(format!("markup.slip: unknown action {k} (open:PATH, skip:on|off, stamp:on|off, ok, ok:I,J…, cancel)")),
        }
        self.fdlg = Some(FDialog::Slip { text, rows, checked, skip, stamp });
        Ok(())
    }

    /// The carry: one undo step (undo.slip), then 0.5's status line.
    fn slip_carry(&mut self, rows: &[SlipRow], checked: &[bool], skip: bool, stamp: bool) -> Result<(), String> {
        let chosen: Vec<(String, String)> =
            rows.iter().zip(checked).filter(|(_, c)| **c).filter_map(|(r, _)| r.old_key.clone().map(|o| (o, r.new_key.clone()))).collect();
        let pairs_n = rows.len();
        let matched = rows.iter().filter(|r| r.old_key.is_some()).count();
        self.tick();
        let who = self.tools.author.clone();
        let label = t0("slip.superseded");
        let facts = self.slip_facts(None);
        let fact = |k: &str| facts.iter().find(|f| f.key == k).cloned();
        let (mut carried, mut left, mut scaled, mut closed, mut stamped) = (0usize, 0usize, 0usize, 0usize, 0usize);
        // what to do, worked out before the store is changed (the markups on each old sheet as they are now)
        struct Plan {
            items: Vec<Map<String, Value>>,
            sources: Vec<String>,
            cal: Option<(String, cc_markup::units::Cal)>,
            stamp: Option<Map<String, Value>>,
        }
        let mut plans: Vec<Plan> = Vec::new();
        for (old, new) in &chosen {
            let (Some(from), Some(to)) = (fact(old), fact(new)) else { continue };
            let list: Vec<&cc_markup::model::Markup> = self.store.on_sheet(old);
            let res = carry_markups(&list, new, Some(&from), Some(&to), skip);
            carried = carried.saturating_add(res.items.len());
            left = left.saturating_add(res.left);
            scaled = scaled.saturating_add(res.scaled);
            closed = closed.saturating_add(res.closed);
            let same = (from.width - to.width).abs() <= 1.0 && (from.height - to.height).abs() <= 1.0;
            let cal = self
                .store
                .calibrations
                .get(old)
                .filter(|_| !self.store.calibrations.contains_key(new) && same)
                .map(|c| (new.clone(), c.clone()));
            let st = stamp.then(|| superseded_stamp(&from, &label, &who, Some(old)));
            if st.is_some() {
                stamped = stamped.saturating_add(1);
            }
            plans.push(Plan { items: res.items, sources: res.sources, cal, stamp: st });
        }
        self.store
            .change(&t0("undo.slip"), |tx| {
                let mut null_punch = Map::new();
                null_punch.insert("punch".into(), Value::Null);
                for p in plans {
                    for (k, it) in p.items.iter().enumerate() {
                        tx.add_fields(it)?;
                        if it.get("punch").is_some_and(|v| v.is_object())
                            && let Some(src) = p.sources.get(k)
                        {
                            tx.update(src, &null_punch, true);
                        }
                    }
                    if let Some((k, c)) = p.cal {
                        tx.set_calibration(&k, Some(c));
                    }
                    if let Some(s) = p.stamp {
                        tx.add_fields(&s)?;
                    }
                }
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        let mut parts = vec![t(
            "slip.done",
            &[("sheets", Fill::S(t("slip.sheets", &[("n", Fill::N(chosen.len() as f64))]))), ("n", Fill::N(carried as f64))],
        )];
        if stamped > 0 {
            parts.push(t("slip.done.stamped", &[("n", Fill::N(stamped as f64)), ("stamp", Fill::S(label.clone()))]));
        }
        if closed > 0 {
            parts.push(t("slip.done.closed", &[("n", Fill::N(closed as f64))]));
        }
        if left > 0 {
            parts.push(t("slip.done.left", &[("n", Fill::N(left as f64))]));
        }
        if scaled > 0 {
            parts.push(t("slip.done.scaled", &[("n", Fill::N(scaled as f64))]));
        }
        if pairs_n > matched {
            parts.push(t("slip.done.new", &[("n", Fill::N((pairs_n - matched) as f64))]));
        }
        self.tools.message = parts.join(" ");
        Ok(())
    }

    // ---- flattened copy ----

    /// The markups of the open file that can be flattened, and the selected ones among them.
    fn flat_counts(&self) -> (usize, Vec<String>) {
        let mine = |m: &cc_markup::model::Markup| m.kind != "foreign" && !m.locked;
        let pre = format!("{}:", self.cur.id);
        let in_file = self.cur.page_keys.iter().map(|k| self.store.on_sheet(k).into_iter().filter(|m| mine(m)).count()).sum();
        let chosen = self
            .tools
            .sel
            .iter()
            .filter_map(|id| self.store.get(id))
            .filter(|m| mine(m) && m.sheet().starts_with(&pre))
            .map(|m| m.id.clone())
            .collect();
        (in_file, chosen)
    }

    /// saveFlat: the window (or "flat.none" when the file has no markups of yours).
    pub fn flat_start(&mut self) {
        self.settle();
        let (n, chosen) = self.flat_counts();
        if n == 0 {
            self.tools.message = t("flat.none", &[("name", Fill::S(self.cur.name.clone()))]);
            return;
        }
        let use_sel = !chosen.is_empty();
        self.note = t("flat.text", &[("name", Fill::S(self.cur.name.clone()))]);
        self.fdlg = Some(FDialog::Flatten { all: n, sel: chosen.len(), use_sel });
    }

    /// The window's Save the copy: `all` or `sel` (the selected markups only, when some are selected).
    pub fn flat_save(&mut self, which: &str) -> Result<(), String> {
        self.fdlg = None;
        self.settle();
        let (n, chosen) = self.flat_counts();
        if n == 0 {
            self.tools.message = t("flat.none", &[("name", Fill::S(self.cur.name.clone()))]);
            return Ok(());
        }
        let flatten = if which == "sel" && !chosen.is_empty() { Flatten::Some(chosen.into_iter().collect()) } else { Flatten::All };
        let now = self.now();
        let links = self.link_sheets();
        let (_, bytes, report) = self.cur.bake_with(&self.store, now, None, &links, flatten)?;
        let name = format!("{}.pdf", t("flat.file", &[("name", Fill::S(stem(&self.cur.name)))]));
        let mut line = t("flat.done", &[("file", Fill::S(name.clone())), ("n", Fill::N(report.flattened as f64))]);
        if report.written > 0 {
            line = format!("{line} {}", t("flat.kept", &[("n", Fill::N(report.written as f64))]));
        }
        self.outbox.push((name, bytes));
        self.tools.message = line;
        Ok(())
    }

    // ---- print ----

    /// printSheets(choose): with several sheets (or Pages… chosen) the window asks what to print; else this sheet.
    pub fn print_start(&mut self, choose: bool) -> Result<(), String> {
        self.settle();
        let several = self.sheet_keys().len() > 1;
        if choose || several {
            self.note = t0("print.text");
            self.fdlg = Some(FDialog::Print { which: "sheet", pages: (self.page + 1).to_string(), error: String::new() });
            return Ok(());
        }
        self.print("sheet", "")
    }

    /// The window's Print: `sheet`, `file`, `set` (several files) or `pages` with the page list of this file.
    pub fn print(&mut self, which: &str, pages: &str) -> Result<(), String> {
        self.settle();
        let n_pages = self.cur.pages.len();
        let idx: Vec<usize> = if which == "pages" {
            cc_markup::pages::parse_page_list(pages, n_pages as f64)?.into_iter().map(|x| x as usize).collect()
        } else {
            Vec::new()
        };
        self.fdlg = None;
        let now = self.now();
        let links = self.link_sheets();
        let parts: Vec<(&crate::pdfview::PdfFile, Vec<usize>)> = match which {
            "set" if !self.others.is_empty() => self.files().into_iter().map(|f| (f, (0..f.pages.len()).collect())).collect(),
            "file" | "set" => vec![(&self.cur, (0..n_pages).collect())],
            "pages" => vec![(&self.cur, idx)],
            _ => vec![(&self.cur, vec![self.page])],
        };
        let mut baked: Vec<(Vec<u8>, Vec<usize>)> = Vec::new();
        for (f, list) in parts {
            let (_, bytes, _) = f.bake_with(&self.store, now, None, &links, Flatten::All)?;
            baked.push((bytes, list));
        }
        let refs: Vec<(&[u8], Vec<usize>)> = baked.iter().map(|(b, l)| (b.as_slice(), l.clone())).collect();
        let (pdf, n) = cc_io_markup::pages::print_pdf(&refs, &self.set_name(), now).inspect_err(|e| self.note.clone_from(e))?;
        self.tools.message = t("print.sent", &[("n", Fill::N(n as f64))]);
        self.host_act = Some(HostAct::Print(format!("{} ({}).pdf", self.set_name(), t0("print.title")), pdf));
        Ok(())
    }

    // ---- Close the set ----

    /// closeSet: files with unsaved markups ask first (Cancel, Close without saving, Save copies first).
    pub fn close_start(&mut self) {
        self.settle();
        if self.store.dirty_sheets.is_empty() {
            self.host_act = Some(HostAct::Close { save: false });
            return;
        }
        self.note = t0("unsaved.close.text");
        self.fdlg = Some(FDialog::Unsaved);
    }

    /// The commands of this module: `markup.slip`, `markup.flatten`, `markup.print`, `markup.close`.
    pub fn slip_run(&mut self, id: &str, name: &str) -> Result<(), String> {
        match id {
            "markup.slip" => self.slip_act(name),
            "markup.flatten" => match name {
                "" => {
                    self.flat_start();
                    Ok(())
                }
                "cancel" => {
                    self.fdlg = None;
                    Ok(())
                }
                "all" | "sel" => self.flat_save(name),
                n => Err(format!("markup.flatten: unknown choice {n} (all, sel, cancel)")),
            },
            "markup.print" => match name {
                "" => self.print_start(false),
                "pages" => self.print_start(true),
                "cancel" => {
                    self.fdlg = None;
                    Ok(())
                }
                n => {
                    let (which, list) = n.split_once(':').unwrap_or((n, ""));
                    if !matches!(which, "sheet" | "file" | "set" | "pages") {
                        return Err(format!("markup.print: unknown choice {which} (sheet, file, set, pages:LIST, cancel)"));
                    }
                    self.print(which, list)
                }
            },
            "markup.close" => match name {
                "" => {
                    self.close_start();
                    Ok(())
                }
                "cancel" => {
                    self.fdlg = None;
                    Ok(())
                }
                "discard" => {
                    self.fdlg = None;
                    self.host_act = Some(HostAct::Close { save: false });
                    Ok(())
                }
                "save" => {
                    self.fdlg = None;
                    self.host_act = Some(HostAct::Close { save: true });
                    Ok(())
                }
                n => Err(format!("markup.close: unknown choice {n} (discard, save, cancel)")),
            },
            _ => Err(format!("unknown command {id}")),
        }
    }

    /// This module's windows (called from `find_dialogs`).
    pub(crate) fn slip_dialogs(&mut self, ctx: &egui::Context, tok: &Tokens, d: FDialog) {
        let mut close = false;
        match d {
            FDialog::Slip { text, rows, mut checked, mut skip, mut stamp } => {
                let mut ok = false;
                let mut cancel = false;
                crate::shell::Dialog::new(t0("slip.title")).collapsible(false).show(ctx, |ui| {
                    ui.label(&text);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for (r, c) in rows.iter().zip(checked.iter_mut()) {
                            let label = RichText::new(format!("{}  {}", self.key_title(&r.new_key), r.text)).color(tok.text);
                            ui.add_enabled(r.old_key.is_some(), egui::Checkbox::new(c, label));
                        }
                    });
                    ui.checkbox(&mut skip, t0("slip.skip_closed"));
                    ui.checkbox(&mut stamp, t("slip.stamp_old", &[("stamp", Fill::S(t0("slip.superseded")))]));
                    ui.horizontal(|ui| {
                        cancel = ui.add(crate::shell::Btn::new(t0("slip.cancel"))).clicked();
                        ok = ui.add(crate::shell::Btn::primary(t0("slip.ok"))).clicked();
                    });
                });
                self.fdlg = Some(FDialog::Slip { text, rows, checked, skip, stamp });
                if ok && let Err(e) = self.slip_act("ok") {
                    self.tools.message = e;
                }
                if cancel {
                    let _ = self.slip_act("cancel");
                }
                return;
            }
            FDialog::Flatten { all, sel, mut use_sel } => {
                let mut save = false;
                crate::shell::Dialog::new(t0("flat.title")).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(t("flat.text", &[("name", Fill::S(self.cur.name.clone()))]));
                    ui.radio_value(&mut use_sel, false, t("flat.all", &[("n", Fill::N(all as f64))]));
                    if sel > 0 {
                        ui.radio_value(&mut use_sel, true, t("flat.sel", &[("n", Fill::N(sel as f64))]));
                    }
                    ui.horizontal(|ui| {
                        close = ui.add(crate::shell::Btn::new(t0("dlg.cancel"))).clicked();
                        save = ui.add(crate::shell::Btn::primary(t0("flat.ok"))).clicked();
                    });
                });
                if save {
                    if let Err(e) = self.flat_save(if use_sel { "sel" } else { "all" }) {
                        self.tools.message = e;
                    }
                    return;
                }
                if !close {
                    self.fdlg = Some(FDialog::Flatten { all, sel, use_sel });
                }
            }
            FDialog::Print { mut which, mut pages, mut error } => {
                let mut go = false;
                let n_sheets = self.sheet_keys().len();
                crate::shell::Dialog::new(t0("print.title")).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(t0("print.text"));
                    ui.radio_value(&mut which, "sheet", t("print.this", &[("sheet", Fill::S(self.cur.title(&self.store, self.page)))]));
                    ui.radio_value(
                        &mut which,
                        "file",
                        t("print.file", &[("name", Fill::S(self.cur.name.clone())), ("n", Fill::N(self.cur.pages.len() as f64))]),
                    );
                    if !self.others.is_empty() {
                        ui.radio_value(&mut which, "set", t("print.set", &[("n", Fill::N(n_sheets as f64))]));
                    }
                    ui.radio_value(&mut which, "pages", t0("print.pages"));
                    let l = ui.label(RichText::new(t("pages.ask.label", &[("n", Fill::N(self.cur.pages.len() as f64))])).small());
                    ui.add(crate::shell::field(&mut pages)).labelled_by(l.id);
                    if !error.is_empty() {
                        ui.colored_label(ui.visuals().error_fg_color, &error);
                    }
                    ui.horizontal(|ui| {
                        close = ui.add(crate::shell::Btn::new(t0("dlg.cancel"))).clicked();
                        go = ui.add(crate::shell::Btn::primary(t0("print.ok"))).clicked();
                    });
                });
                if go {
                    match self.print(which, &pages) {
                        Ok(()) => return,
                        Err(e) => error = e,
                    }
                }
                if !close {
                    self.fdlg = Some(FDialog::Print { which, pages, error });
                }
            }
            FDialog::Unsaved => {
                let (mut discard, mut save) = (false, false);
                crate::shell::Dialog::new(t0("unsaved.title")).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(t0("unsaved.close.text"));
                    ui.horizontal(|ui| {
                        close = ui.add(crate::shell::Btn::new(t0("dlg.cancel"))).clicked();
                        discard = ui.add(crate::shell::Btn::new(t0("unsaved.close.discard"))).clicked();
                        save = ui.add(crate::shell::Btn::primary(t0("unsaved.save_first"))).clicked();
                    });
                });
                if discard || save {
                    let _ = self.slip_run("markup.close", if save { "save" } else { "discard" });
                    return;
                }
                if !close {
                    self.fdlg = Some(FDialog::Unsaved);
                }
            }
            other => self.fdlg = Some(other),
        }
        if close {
            self.fdlg = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_are_named_from_the_stem() {
        assert_eq!(stem("Set A (Marked Up).PDF"), "Set A");
        assert_eq!(stem("plan.pdf"), "plan");
        assert_eq!(stem("plan"), "plan");
    }
}
