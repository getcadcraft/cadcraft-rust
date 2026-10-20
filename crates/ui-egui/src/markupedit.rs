//! MarkupCraft's editing around the drawing tools (M9.16), ported from markupcraft.js: the clipboard (copySelection,
//! cutSelection, paste, duplicate, selectAll — clip.js), replies on a markup's thread (threadSection's Reply), the sheet's
//! number and title typed by hand (renameSheet, applySheetNames), the Pages menu (pagesOp: rotate, move, insert a blank
//! page or another PDF's pages, delete, extract — the markups are baked into the file, the pages changed and the file
//! read back, so every markup, thread, scale and sheet name moves with its page), and the Markups list (filters, order,
//! jump to, renderList's count line) with its exports (CSV, XLSX workbook, PDF summary). Each action is the JS
//! handler: the same checks, one `Store::change` with the 0.5 undo label, the same status line (`tools.message`).

use cc_io_markup::read::SheetInfo;
use cc_markup::csv::{Cell, CsvColumn, SheetRef};
use cc_markup::i18n::{Fill, t, t0};
use cc_markup::model::{Markup, STATUSES, type_name};
use cc_markup::shapes::{LegendRow, ShapeEnv};
use egui::RichText;
use serde_json::{Map, Value, json};

use crate::pdfview::{PdfFile, PdfView};
use crate::theme::Tokens;

type P = [f64; 2];

/// clip.js `source` of this window's copies (markupcraft.js names its browser tab the same way).
pub const TAB: &str = "tab-cadcraft";
/// `PASTE_STEP`: screen pixels down and right for "beside the copy" (each paste of the same copy steps again).
const PASTE_STEP: f64 = 12.0;
/// The list shows at most this many rows (renderList's `slice(0, 2000)`).
const LIST_MAX: usize = 2000;
/// `\u0001none`: the filter value for "no layer" / "no space".
pub const NONE: &str = "\u{1}none";

/// The Markups list's filters (`filters`) and order (`data-sort`).
#[derive(Debug, Clone, Default)]
pub struct ListState {
    pub sheet: String,
    pub author: String,
    pub status: String,
    pub kind: String,
    pub layer: String,
    pub space: String,
    pub field: String,
    pub sort: String,
}

/// A dialog over the sheet.
#[derive(Debug, Clone)]
pub enum Dialog {
    /// renameSheet: the number and title being typed
    Rename { number: String, title: String },
    /// askPages for Delete pages or Extract pages: the page list being typed and the check's error
    Pages { kind: &'static str, text: String, error: String },
    /// Save as a tool: the tool's name being typed
    ChestName { name: String, error: String },
    /// Remove a tool from the chest (confirm)
    RemoveTool { index: usize },
    /// the signature pad: the strokes drawn (pad coordinates)
    Signature { strokes: Vec<Vec<[f64; 2]>>, error: String },
}

/// One sheet of the plan set (markupcraft.js `sheets[i]`): files in the order they were opened, pages in order.
pub struct Sheet<'a> {
    pub file: &'a PdfFile,
    pub index: usize,
    pub key: String,
    pub info: SheetInfo,
}

/// `lowFirst(s)`: an undo label inside a sentence.
fn low_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().chain(c).collect(),
        None => String::new(),
    }
}

/// `rangeText([0,1,2,4])` → "1-3, 5".
pub fn range_text(idx: &[f64]) -> String {
    let mut v: Vec<f64> = idx.iter().map(|k| k + 1.0).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = Vec::new();
    let mut k = 0;
    while k < v.len() {
        let mut j = k;
        while j + 1 < v.len() && v.get(j + 1).copied() == v.get(j).map(|x| x + 1.0) {
            j += 1;
        }
        let a = v.get(k).copied().unwrap_or(0.0);
        let b = v.get(j).copied().unwrap_or(0.0);
        out.push(if j > k { format!("{}-{}", cc_markup::js::ns(a), cc_markup::js::ns(b)) } else { cc_markup::js::ns(a) });
        k = j + 1;
    }
    out.join(", ")
}

/// A legend's rows (`legendOf(m)`: legendRows of the store for its sheet or the whole set).
pub fn legend_of(store: &cc_markup::model::Store, m: &Markup) -> Vec<LegendRow> {
    let all = store.all();
    let scope = m.legend.as_deref().unwrap_or("");
    cc_markup::spaces::legend_rows(&all, m.sheet.as_deref(), scope, &|x| type_name(&x.kind))
        .into_iter()
        .map(|r| LegendRow { color: r.color, label: r.label, n: r.n })
        .collect()
}

impl PdfView {
    /// markupcraft.js `sheets`.
    pub fn sheet_list(&self) -> Vec<Sheet<'_>> {
        let mut out = Vec::new();
        for f in self.files() {
            for i in 0..f.pages.len() {
                out.push(Sheet { file: f, index: i, key: f.key_of(i), info: f.info(&self.store, i) });
            }
        }
        out
    }

    /// The file and page index of a sheet key.
    pub fn locate(&self, key: &str) -> Option<(&PdfFile, usize)> {
        self.files().into_iter().find_map(|f| f.page_keys.iter().position(|k| k == key).map(|i| (f, i)))
    }

    /// `badgeFor(key)`.
    pub fn badge_for(&self, key: &str) -> f64 {
        self.locate(key).map_or(9.0, |(f, i)| f.badge_of(i))
    }

    /// `envOf(m)`: the sheet's scale, its count badge size, and a legend's rows.
    pub fn env_of(&self, m: &Markup) -> ShapeEnv {
        ShapeEnv {
            cal: self.store.calibrations.get(m.sheet()).cloned(),
            badge: self.badge_for(m.sheet()),
            legend: (m.kind == "legend").then(|| legend_of(&self.store, m)),
        }
    }

    /// `hasSpaces()`.
    pub fn has_spaces(&self) -> bool {
        self.store.markups.values().any(|m| m.kind == "space")
    }

    /// `markupSpace(m)`: the space holding the middle of its bounds ('' = none).
    pub fn markup_space(&self, m: &Markup) -> String {
        let spaces: Vec<&Markup> = self.store.on_sheet(m.sheet()).into_iter().filter(|x| x.kind == "space").collect();
        cc_markup::spaces::space_of(m, &spaces, &|x| cc_markup::geom::bounds_of(x, &self.env_of(x)))
    }

    /// `valueRows(m)`: a markup's measured values ([label, value]).
    pub fn value_rows(&self, m: &Markup) -> Vec<(String, String)> {
        let cal = self.store.calibrations.get(m.sheet());
        let p = m.pts();
        let none = || t0("value.no_scale");
        let space_row = || {
            let v = if self.has_spaces() { self.markup_space(m) } else { String::new() };
            if v.is_empty() { Vec::new() } else { vec![(t0("filter.space"), v)] }
        };
        match m.kind.as_str() {
            "measure" => {
                let d = match p {
                    [a, b, ..] => (b[0] - a[0]).hypot(b[1] - a[1]),
                    _ => f64::NAN,
                };
                vec![(t0("value.length"), if cal.is_some() { cc_markup::units::measure(d, cal) } else { none() })]
            }
            "area" => {
                let holes = m.holes.clone().unwrap_or_default();
                let (a, per) = cc_markup::units::measure_area(p, cal, &holes);
                if cal.is_none() {
                    return vec![(t0("value.area"), none())];
                }
                let mut v = vec![(t0("value.area"), a), (t0("value.perimeter"), per)];
                if !holes.is_empty() {
                    v.push((t0("value.cutouts"), holes.len().to_string()));
                }
                v
            }
            "count" => {
                let mut v = vec![(t0("value.count"), p.len().to_string())];
                v.extend(space_row());
                v
            }
            "angle" if p.len() == 3 => match p {
                [a, b, c] => vec![(t0("value.angle"), cc_markup::arcs::format_angle(cc_markup::arcs::angle_at(*a, *b, *c), 1))],
                _ => Vec::new(),
            },
            "arc" if p.len() == 3 => match p {
                [a, b, c] => {
                    let arc = cc_markup::arcs::arc_through(*a, *b, *c);
                    let v = match (cal, arc.as_ref()) {
                        (Some(cal), Some(arc)) => cc_markup::units::measure(cc_markup::arcs::arc_length(Some(arc)), Some(cal)),
                        _ => none(),
                    };
                    vec![(t0("value.length"), v)]
                }
                _ => Vec::new(),
            },
            _ => space_row(),
        }
    }

    /// `valueOf(m)`: the value as one line (lists, CSV, BCF).
    pub fn value_of(&self, m: &Markup) -> String {
        let has_cal = self.store.calibrations.contains_key(m.sheet());
        match m.kind.as_str() {
            "count" => {
                let g = if m.label.is_empty() { t0("count.default") } else { m.label.clone() };
                t("value.count.line", &[("group", Fill::S(g)), ("n", Fill::N(m.pts().len() as f64))])
            }
            "area" => match self.value_rows(m).as_slice() {
                [(_, a), (_, p)] => t("value.area.line", &[("area", Fill::S(a.clone())), ("perimeter", Fill::S(p.clone()))]),
                _ => String::new(),
            },
            "measure" | "arc" if has_cal => self.value_rows(m).into_iter().next().map(|r| r.1).unwrap_or_default(),
            "angle" => self.value_rows(m).into_iter().next().map(|r| r.1).unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// `setName()`: the first file's name without ".pdf" and " (marked up)".
    pub fn set_name(&self) -> String {
        let n = self.files().first().map(|f| f.name.clone()).unwrap_or_else(|| "MarkupCraft".into());
        let strip = |s: &str, suf: &str| -> String {
            if s.len() >= suf.len() && s.get(s.len() - suf.len()..).is_some_and(|e| e.eq_ignore_ascii_case(suf)) {
                s.get(..s.len() - suf.len()).unwrap_or(s).to_string()
            } else {
                s.to_string()
            }
        };
        strip(&strip(&n, ".pdf"), " (marked up)")
    }

    /// The clock for files made now (a fixed one in scripts and parity runs).
    pub(crate) fn now(&self) -> f64 {
        self.clock.unwrap_or_else(|| web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64))
    }

    /// The selected markups that are not read-only (`editable()`), in selection order.
    fn editable_list(&self) -> Vec<Markup> {
        self.tools.sel.iter().filter_map(|id| self.store.get(id)).filter(|m| !m.locked).cloned().collect()
    }

    /// `commitEdit(); cancelOps()`.
    fn settle(&mut self) {
        let key = self.sheet_key();
        self.tick();
        self.tools.commit_edit(&mut self.store, &key);
        self.tools.cancel();
    }

    /// `screenStep(px)`: a move of `px` screen pixels down and to the right, in user space.
    fn screen_step(&self, px: f64) -> P {
        let a = self.user_of([0.0, 0.0]);
        let b = self.user_of([px, px]);
        [b[0] - a[0], b[1] - a[1]]
    }

    /// Whether a point of the sheet is on screen (before the view was ever drawn: yes).
    fn in_view(&self, p: P) -> bool {
        let Some(sz) = self.view_size else { return true };
        let s = self.screen_of(p);
        let (x, y) = (s[0] + f64::from(self.pan.x), s[1] + f64::from(self.pan.y));
        x >= 0.0 && y >= 0.0 && x <= f64::from(sz.x) && y <= f64::from(sz.y)
    }

    /// The middle of the view in user space.
    fn view_center(&self) -> P {
        let sz = self.view_size.unwrap_or(egui::vec2(0.0, 0.0));
        self.user_of([f64::from(sz.x / 2.0 - self.pan.x), f64::from(sz.y / 2.0 - self.pan.y)])
    }

    /// `afterHistory`: undo (or redo) one step; the status line says what.
    pub fn history(&mut self, redo: bool) {
        self.settle();
        let label = if redo { self.store.redo() } else { self.store.undo() };
        let store = &self.store;
        self.tools.sel.retain(|id| store.get(id).is_some());
        let which = if redo { "redo" } else { "undo" };
        self.tools.message = match label {
            Some(l) => t(&format!("history.{which}"), &[("what", Fill::S(low_first(&l)))]),
            None => t0(&format!("history.{which}.none")),
        };
    }

    // ---- the clipboard ----

    /// `copySelection(verb)`: the selection (not read-only) becomes the clipboard.
    pub fn copy(&mut self, verb: &str) -> bool {
        let list = self.editable_list();
        if list.is_empty() {
            self.tools.message = t0(&format!("clip.{verb}.{}", if self.tools.sel.is_empty() { "need" } else { "locked" }));
            return false;
        }
        self.tick();
        let refs: Vec<&Markup> = list.iter().collect();
        self.clip = cc_markup::clip::make_clip(&self.store.env, &refs, TAB, &self.sheet_key());
        self.paste_run = (String::new(), 0);
        self.tools.message = t(&format!("clip.{verb}.done"), &[("n", Fill::N(list.len() as f64))]);
        true
    }

    /// `cutSelection()`: copy, then delete them (one undo step).
    pub fn cut(&mut self) {
        let list = self.editable_list();
        if !self.copy("cut") {
            return;
        }
        let r = self.store.change(&t("undo.cut", &[("n", Fill::N(list.len() as f64))]), |tx| {
            for m in &list {
                tx.remove(&m.id);
            }
            Ok(())
        });
        if let Err(e) = r {
            self.tools.message = e.to_string();
        }
        self.tools.sel.clear();
    }

    /// `paste(how)`: the clipboard on this sheet as one undo step. `how` = offset (beside the copy: a step down and
    /// right on its own sheet, a step more each time; where it was on another sheet; the middle of the view when that
    /// is off screen), place (the same sheet coordinates), pointer (centred on `at`).
    pub fn paste(&mut self, how: &str, at: Option<P>) {
        let Some(c) = self.clip.clone() else {
            self.tools.message = t0("paste.empty");
            return;
        };
        self.settle();
        if self.tools.tool != "select" {
            let key = self.sheet_key();
            self.tools.set_tool(&mut self.store, &key, "select");
        }
        let key = self.sheet_key();
        let rot = Some(self.page_rot());
        let mut opts = cc_markup::clip::PlaceOpts { sheet: key.clone(), author: self.tools.author.clone(), rot, ..Default::default() };
        let how = match (how, at) {
            ("place", _) => {
                opts.delta = Some([0.0, 0.0]);
                "place"
            }
            ("pointer", Some(p)) => {
                opts.center = Some(p);
                "pointer"
            }
            _ => {
                let run = format!("{}|{}|{key}", c.source, c.at);
                self.paste_run = if self.paste_run.0 == run { (run, self.paste_run.1.saturating_add(1)) } else { (run, 1) };
                let n = f64::from(self.paste_run.1);
                let steps = if c.source == TAB && c.sheet == key { n } else { n - 1.0 };
                let d = self.screen_step(PASTE_STEP * steps);
                opts.delta = Some(d);
                if let Some(b) = cc_markup::clip::clip_bounds(&c.items)
                    && !self.in_view([(b[0] + b[2]) / 2.0 + d[0], (b[1] + b[3]) / 2.0 + d[1]])
                {
                    opts.delta = None;
                    opts.center = Some(self.view_center());
                }
                "offset"
            }
        };
        self.tick();
        let made = cc_markup::clip::place_clip(&mut self.store.env, &c, &opts);
        let n = made.len() as f64;
        let label = t(if how == "place" { "undo.paste_in_place" } else { "undo.paste" }, &[("n", Fill::N(n))]);
        let mut ids = Vec::new();
        let r = self.store.change(&label, |tx| {
            for m in made {
                ids.push(tx.add(m)?.id);
            }
            Ok(())
        });
        if let Err(e) = r {
            self.tools.message = e.to_string();
            return;
        }
        self.tools.sel = ids;
        let mut msg = t(&format!("paste.done.{how}"), &[("n", Fill::N(n))]);
        if let Some(sk) = c.skipped.filter(|s| *s > 0) {
            msg = format!("{msg} {}", t("paste.skipped", &[("n", Fill::N(sk as f64))]));
        }
        self.tools.message = format!("{msg} {}", t("paste.undo", &[("n", Fill::N(n))]));
    }

    /// `duplicate()`: copies of the selection a step down and right, one undo step; the clipboard is left alone.
    pub fn duplicate(&mut self) {
        let list = self.editable_list();
        if list.is_empty() {
            self.tools.message = t0(if self.tools.sel.is_empty() { "dup.need" } else { "dup.locked" });
            return;
        }
        self.settle();
        let key = self.sheet_key();
        let refs: Vec<&Markup> = list.iter().collect();
        let made = match cc_markup::clip::make_clip(&self.store.env, &refs, TAB, &key) {
            Some(c) => {
                let opts = cc_markup::clip::PlaceOpts {
                    sheet: key.clone(),
                    author: self.tools.author.clone(),
                    rot: Some(self.page_rot()),
                    delta: Some(self.screen_step(PASTE_STEP)),
                    center: None,
                };
                cc_markup::clip::place_clip(&mut self.store.env, &c, &opts)
            }
            None => Vec::new(),
        };
        let n = made.len() as f64;
        let mut ids = Vec::new();
        let r = self.store.change(&t("undo.duplicate", &[("n", Fill::N(n))]), |tx| {
            for m in made {
                ids.push(tx.add(m)?.id);
            }
            Ok(())
        });
        if let Err(e) = r {
            self.tools.message = e.to_string();
            return;
        }
        self.tools.sel = ids;
        self.tools.message = t("dup.done", &[("n", Fill::N(n))]);
    }

    /// `selectAll()`: every markup on this sheet that can be picked.
    pub fn select_all(&mut self) {
        let key = self.sheet_key();
        self.tools.sel = self.store.on_sheet(&key).into_iter().filter(|m| self.store.open(m)).map(|m| m.id.clone()).collect();
        let n = self.tools.sel.len();
        self.tools.message = if n > 0 { t("select.all", &[("n", Fill::N(n as f64))]) } else { t0("select.all.none") };
    }

    // ---- replies ----

    /// threadSection's Reply: a comment on the selected markup's thread (one undo step that does not touch its date).
    pub fn reply(&mut self, text: &str) -> Result<(), String> {
        let id = self.tools.selected().map(str::to_string).ok_or("markup.reply: select one markup")?;
        let text = cc_markup::js::trim(text).to_string();
        if text.is_empty() {
            return Ok(());
        }
        self.tick();
        let author = self.tools.author.clone();
        let r = cc_markup::model::clean_reply(&mut self.store.env, &json!({ "author": author, "text": text, "kind": "comment" }))
            .ok_or("markup.reply: not a usable reply")?;
        let Some(cur) = self.store.get(&id).cloned() else { return Ok(()) };
        let mut replies: Vec<Value> = cur.replies.iter().map(crate::markupprops::reply_value).collect();
        replies.push(crate::markupprops::reply_value(&r));
        let mut patch = Map::new();
        patch.insert("replies".into(), Value::Array(replies));
        self.store.change(&t0("undo.reply"), |tx| Ok(tx.update(&id, &patch, false))).map_err(|e| e.to_string())?;
        self.reply_drafts.remove(&id);
        self.tools.message =
            if author.is_empty() { t0("thread.added") } else { t("thread.added.as", &[("author", Fill::S(author.clone()))]) };
        Ok(())
    }

    // ---- the sheet's name ----

    /// renameSheet's Save: the number and title typed by hand (both empty: back to what the file says).
    pub fn rename_sheet(&mut self, number: &str, title: &str) {
        let key = self.sheet_key();
        let number: String = cc_markup::js::trim(number).chars().take(40).collect();
        let title: String = cc_markup::js::trim(title).chars().take(200).collect();
        let any = !number.is_empty() || !title.is_empty();
        let name = any.then_some(cc_markup::model::SheetName { number, title });
        let r = self.store.change(&t0("props.rename"), |tx| {
            tx.set_sheet_name(&key, name);
            Ok(())
        });
        self.tools.message = match r {
            Ok(()) => t0(if any { "rename.done" } else { "rename.cleared" }),
            Err(e) => e.to_string(),
        };
    }

    /// What the rename dialog starts with (`now.number ?? s.page.sheetNo`, `now.title ?? s.page.name`).
    pub fn rename_start(&self) -> (String, String) {
        let key = self.sheet_key();
        let now = self.store.sheet_names.get(&key);
        let info = self.cur.info(&self.store, self.page);
        (now.map_or(info.sheet_no, |n| n.number.clone()), now.map_or(info.name, |n| n.title.clone()))
    }

    // ---- pages ----

    /// `labelsOf(f)`: each page's label as the file will carry it, or None when none has one.
    fn labels_of(&self, f: &PdfFile) -> Option<Vec<String>> {
        let l: Vec<String> = (0..f.pages.len())
            .map(|i| {
                let n = self.store.sheet_names.get(&f.key_of(i)).map(|n| n.number.clone()).filter(|n| !n.is_empty());
                n.unwrap_or_else(|| f.recs.get(i).map(|r| r.info.label.clone()).unwrap_or_default())
            })
            .collect();
        l.iter().any(|x| !x.is_empty()).then_some(l)
    }

    /// pagesOp on the open sheet's PDF. `kind` = rotate-cw, rotate-ccw, move-up, move-down, insert-blank, add-blank,
    /// delete (`arg` = the page list), extract (`arg` = the page list; the new PDF goes to `outbox`), insert-pdf (`src` =
    /// the picked file's name and bytes). Undo history starts again after a page change (said in the status line).
    pub fn pages_op(&mut self, kind: &str, arg: &str, src: Option<(&str, &[u8])>) -> Result<(), String> {
        self.settle();
        let (fid, n, i) = (self.cur.id.clone(), self.cur.pages.len(), self.page);
        let title = self.cur.title(&self.store, i);
        let fname = self.cur.name.clone();
        let labels = self.labels_of(&self.cur);
        let lab = labels.as_deref();
        let now = self.now();
        let rot_of = |d: f64| -> f64 { ((self.page_rot() + d) % 360.0 + 360.0) % 360.0 };
        let (out, show, done): (Vec<u8>, usize, String) = match kind {
            "rotate-cw" | "rotate-ccw" => {
                let d = if kind == "rotate-cw" { 90.0 } else { -90.0 };
                let (_, baked) = self.cur.bake(&self.store, now, Some((i, rot_of(d))))?;
                let out = cc_io_markup::pages::rotate_pages(&baked, &[i as f64], d)?;
                let key = if d > 0.0 { "pages.done.rotate_cw" } else { "pages.done.rotate_ccw" };
                (out, i, t(key, &[("sheet", Fill::S(title))]))
            }
            "move-up" | "move-down" => {
                let to = if kind == "move-up" { i as f64 - 1.0 } else { i as f64 + 1.0 };
                if to < 0.0 || to >= n as f64 {
                    self.tools.message = t(if to < 0.0 { "pages.first" } else { "pages.last" }, &[("name", Fill::S(fname))]);
                    return Ok(());
                }
                let (_, baked) = self.cur.bake(&self.store, now, None)?;
                let out = cc_io_markup::pages::move_pages(&baked, &[i as f64], to, lab)?;
                let to_u = cc_markup::js::f2usize(to);
                let done = t("pages.done.move", &[("sheet", Fill::S(title)), ("page", Fill::N(to + 1.0)), ("n", Fill::N(n as f64))]);
                (out, to_u, done)
            }
            "insert-blank" | "add-blank" => {
                let at = if kind == "insert-blank" { i + 1 } else { n };
                let (_, baked) = self.cur.bake(&self.store, now, None)?;
                let grid = self.cur.blank.as_ref().map(|b| {
                    (b.get("grid").and_then(Value::as_str).unwrap_or("none"), b.get("metric").and_then(Value::as_bool).unwrap_or(false))
                });
                let out = cc_io_markup::pages::insert_blank(&baked, at as f64, None, lab, grid)?;
                let done = if kind == "insert-blank" {
                    t("pages.done.blank", &[("after", Fill::S(title))])
                } else {
                    t("pages.done.add_blank", &[("name", Fill::S(fname)), ("n", Fill::N((n + 1) as f64))])
                };
                (out, at, done)
            }
            "insert-pdf" => {
                let (sname, sbytes) = src.ok_or("markup.pages: insert-pdf needs a PDF")?;
                if !sname.to_ascii_lowercase().ends_with(".pdf") {
                    self.tools.message = t("pages.insert.not_pdf", &[("file", Fill::S(sname.to_string()))]);
                    return Ok(());
                }
                let (_, baked) = self.cur.bake(&self.store, now, None)?;
                let (out, count) = cc_io_markup::pages::insert_pages(&baked, sbytes, (i + 1) as f64, None, lab)?;
                let done = t(
                    "pages.done.insert",
                    &[("n", Fill::N(count as f64)), ("file", Fill::S(sname.to_string())), ("after", Fill::S(title))],
                );
                (out, i + 1, done)
            }
            "delete" => {
                let idx = cc_markup::pages::parse_page_list(arg, n as f64)?;
                if idx.len() >= n {
                    return Err(t0("pages.err.all"));
                }
                let (_, baked) = self.cur.bake(&self.store, now, None)?;
                let out = cc_io_markup::pages::delete_pages(&baked, &idx, lab)?;
                let left = cc_markup::pages::order_delete(n as f64, &idx);
                let show = left.iter().position(|k| *k >= i as f64).unwrap_or(left.len().saturating_sub(1));
                let done = t("pages.done.delete", &[("n", Fill::N(idx.len() as f64)), ("name", Fill::S(fname))]);
                (out, show, done)
            }
            "extract" => {
                let idx = cc_markup::pages::parse_page_list(arg, n as f64)?;
                let (_, baked) = self.cur.bake(&self.store, now, None)?;
                let out = cc_io_markup::pages::extract_pages(&baked, &idx, now)?;
                let stem =
                    if fname.to_ascii_lowercase().ends_with(".pdf") { fname.get(..fname.len() - 4).unwrap_or(&fname) } else { &fname };
                let name = format!(
                    "{}.pdf",
                    t("pages.extract.file", &[("name", Fill::S(stem.to_string())), ("pages", Fill::S(range_text(&idx)))])
                );
                self.tools.message = t("pages.done.extract", &[("n", Fill::N(idx.len() as f64)), ("file", Fill::S(name.clone()))]);
                self.outbox.push((name, out));
                return Ok(());
            }
            _ => return Err(format!("markup.pages: unknown action {kind}")),
        };
        self.replace_file(&fid, &out, show)?;
        self.tools.message = format!("{done} {}", t0("pages.no_undo"));
        Ok(())
    }

    /// `replaceFile(f, bytes, show)`: a changed copy of a file in its place, read back; its sheets are unsaved.
    fn replace_file(&mut self, id: &str, bytes: &[u8], show: usize) -> Result<(), String> {
        // a copy that cannot be read leaves everything as it was
        let probe = cc_io_takeoff::pdfread::open(bytes)?;
        if cc_io_takeoff::pdfread::pages(&probe).is_empty() {
            return Err("the PDF has no pages".into());
        }
        self.compare_stop();
        let pre = format!("{id}:");
        self.store.dirty_sheets.retain(|k| !k.starts_with(&pre));
        self.store.drop_sheets(&pre);
        self.cmp.segs.retain(|k, _| !k.starts_with(&pre));
        let name = self.files().into_iter().find(|f| f.id == id).map(|f| f.name.clone()).unwrap_or_default();
        let f = PdfFile::read(id, &name, bytes, &mut self.store)?;
        for k in &f.page_keys {
            self.store.dirty_sheets.insert(k.clone());
        }
        self.takeoff.add_file(f.hash(), &name, bytes.len(), f.pages.len());
        let last = f.pages.len().saturating_sub(1);
        if self.cur.id == id {
            self.cur = f;
        } else if let Some(o) = self.others.iter_mut().find(|o| o.id == id) {
            *o = f;
        }
        self.tools.sel.clear();
        self.tex = None;
        self.go_file(id, show.min(last));
        self.takeoff.set_file(self.cur.hash());
        Ok(())
    }

    // ---- the Markups list ----

    /// `filteredMarkups()`.
    pub fn filtered(&self) -> Vec<&Markup> {
        let f = &self.list;
        let cur = self.sheet_key();
        let spaces = !f.space.is_empty();
        self.store
            .all()
            .into_iter()
            .filter(|m| {
                if f.sheet == "current" && m.sheet() != cur {
                    return false;
                }
                if !f.sheet.is_empty() && f.sheet != "current" && m.sheet() != f.sheet {
                    return false;
                }
                if !f.author.is_empty() && m.author != f.author {
                    return false;
                }
                if !f.status.is_empty() && m.status != f.status {
                    return false;
                }
                if !f.kind.is_empty() && m.kind != f.kind {
                    return false;
                }
                if !f.layer.is_empty() && (if f.layer == NONE { !m.layer.is_empty() } else { m.layer != f.layer }) {
                    return false;
                }
                if !f.field.is_empty() {
                    let (ck, cv) = f.field.split_once('\u{1}').unwrap_or((f.field.as_str(), ""));
                    if m.fields.get(ck).map_or("", String::as_str) != cv {
                        return false;
                    }
                }
                if spaces {
                    let sp = self.markup_space(m);
                    if if f.space == NONE { !sp.is_empty() } else { sp != f.space } {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// `sheetOrder(m)`.
    fn sheet_order(keys: &[String], m: &Markup) -> f64 {
        keys.iter().position(|k| k == m.sheet()).map_or(-1.0, |i| i as f64)
    }

    /// The list as shown: filtered, in `listOrder(sort)`.
    pub fn list_rows(&self) -> Vec<&Markup> {
        let keys: Vec<String> = self.sheet_list().into_iter().map(|s| s.key).collect();
        let mut list = self.filtered();
        let base = |a: &&Markup, b: &&Markup| -> f64 {
            let d = Self::sheet_order(&keys, a) - Self::sheet_order(&keys, b);
            if d != 0.0 { d } else { cc_markup::collate::locale_compare(&a.created, &b.created) }
        };
        let sort = self.list.sort.clone();
        if sort.is_empty() {
            cc_m3d::js::sort_by(&mut list, base);
        } else if let Some(ck) = sort.strip_prefix("col:") {
            let col = self.store.columns.iter().find(|c| c.key == ck).cloned();
            cc_m3d::js::sort_by(&mut list, |a, b| {
                let r = col.as_ref().map_or(0.0, |c| cc_markup::columns::compare_by(c, a, b));
                if r != 0.0 && !r.is_nan() { r } else { base(a, b) }
            });
        } else {
            let desc = sort == "created" || sort == "modified";
            enum K {
                N(f64),
                S(String),
            }
            let get = |m: &Markup| -> K {
                match sort.as_str() {
                    "author" => K::S(m.author.clone()),
                    "status" => K::N(STATUSES.iter().position(|s| *s == m.status).map_or(-1.0, |i| i as f64)),
                    "type" => K::S(type_name(&m.kind)),
                    "created" => K::S(m.created.clone()),
                    "modified" => K::S(m.modified.clone()),
                    _ => K::S(String::new()),
                }
            };
            cc_m3d::js::sort_by(&mut list, |a, b| {
                let r = match (get(a), get(b)) {
                    (K::N(x), K::N(y)) => x - y,
                    (K::S(x), K::S(y)) => cc_markup::collate::locale_compare(&x, &y),
                    _ => 0.0,
                };
                let r = if desc { -r } else { r };
                if r != 0.0 && !r.is_nan() { r } else { base(a, b) }
            });
        }
        list
    }

    /// Set a list filter (`key` = sheet, author, status, type, layer, space, field) or the order (`sort`).
    pub fn set_filter(&mut self, key: &str, v: &str) -> Result<(), String> {
        let f = &mut self.list;
        let slot = match key {
            "sheet" => &mut f.sheet,
            "author" => &mut f.author,
            "status" => &mut f.status,
            "type" => &mut f.kind,
            "layer" => &mut f.layer,
            "space" => &mut f.space,
            "field" => &mut f.field,
            "sort" => &mut f.sort,
            _ => return Err(format!("markup.filter: unknown filter {key} (sheet, author, status, type, layer, space, field, sort)")),
        };
        // "none" stands for the list's "No layer" / "No space" entries
        *slot = if v == "none" && (key == "layer" || key == "space") { NONE.to_string() } else { v.replace("\\u0001", "\u{1}") };
        Ok(())
    }

    /// `jumpTo(id)`: select a markup and show its sheet (zoomed to it when it is off screen or tiny).
    pub fn jump_to(&mut self, id: &str) -> Result<(), String> {
        let m = self.store.get(id).cloned().ok_or_else(|| format!("markup.jump: no markup {id}"))?;
        let Some((fid, i)) = self.locate(m.sheet()).map(|(f, i)| (f.id.clone(), i)) else { return Ok(()) };
        self.tools.sel = vec![id.to_string()];
        if fid != self.cur.id || i != self.page {
            let sel = self.tools.sel.clone();
            self.go_file(&fid, i);
            self.tools.sel = sel;
        }
        if let Some(b) = cc_markup::geom::bounds_of(&m, &self.env_of(&m)) {
            let a = self.screen_of([b[0], b[1]]);
            let c = self.screen_of([b[2], b[3]]);
            let tiny = (c[0] - a[0]).abs() < 12.0 && (c[1] - a[1]).abs() < 12.0;
            let off = !self.in_view([b[0], b[1]]) && !self.in_view([b[2], b[3]]);
            if off || tiny {
                self.zoom_to(b, 0.35);
            }
        }
        Ok(())
    }

    /// `view.zoomTo(box, pad)`: the box filling the view with `pad` of it as margin, centred.
    pub fn zoom_to(&mut self, b: [f64; 4], pad: f64) {
        let Some(sz) = self.view_size else { return };
        let (w, h) = ((b[2] - b[0]).abs().max(1.0), (b[3] - b[1]).abs().max(1.0));
        let z = (f64::from(sz.x) / (w * (1.0 + 2.0 * pad))).min(f64::from(sz.y) / (h * (1.0 + 2.0 * pad)));
        self.set_zoom(z);
        let c = self.screen_of([(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0]);
        self.pan = egui::vec2((f64::from(sz.x) / 2.0 - c[0]) as f32, (f64::from(sz.y) / 2.0 - c[1]) as f32);
    }

    /// `sheetOf(m)` for the exports.
    fn sheet_ref(&self, m: &Markup) -> Option<SheetRef> {
        self.locate(m.sheet()).map(|(f, i)| SheetRef {
            title: f.title(&self.store, i),
            file: f.name.clone(),
            page: Some(Cell::N((i + 1) as f64)),
        })
    }

    /// The exports of the Markups list as filtered (by sheet): `csv` (exportListCsv), `xlsx` / `pdf` (exportSummary).
    /// → the file to save; the status line says what was written.
    pub fn export_list(&mut self, kind: &str) -> Result<Option<(String, Vec<u8>)>, String> {
        let keys: Vec<String> = self.sheet_list().into_iter().map(|s| s.key).collect();
        let mut list = self.filtered();
        cc_m3d::js::sort_by(&mut list, |a, b| Self::sheet_order(&keys, a) - Self::sheet_order(&keys, b));
        if list.is_empty() {
            self.tools.message = t0("export.none");
            return Ok(None);
        }
        let n = list.len() as f64;
        let cols: Vec<CsvColumn> = self.store.columns.iter().map(|c| CsvColumn { key: c.key.clone(), name: c.name.clone() }).collect();
        let sheet_of = |m: &Markup| self.sheet_ref(m);
        let measure_of = |m: &Markup| self.value_of(m);
        let space_fn = |m: &Markup| self.markup_space(m);
        let space_of: Option<&dyn Fn(&Markup) -> String> = if self.has_spaces() { Some(&space_fn) } else { None };
        let set = self.set_name();
        let now = self.now();
        let (file, bytes, msg) = match kind {
            "csv" => {
                let rows = cc_markup::csv::markup_rows(&list, &sheet_of, &measure_of, space_of, &cols);
                let name = format!("{}.csv", t("export.list.file", &[("set", Fill::S(set))]));
                (name, cc_markup::csv::to_csv(&rows).into_bytes(), t("export.csv.done", &[("n", Fill::N(n))]))
            }
            "xlsx" => {
                let name = t("sum.file", &[("set", Fill::S(set))]);
                let sheets = cc_markup::summary::summary_sheets(&list, &sheet_of, &measure_of, space_of, &cols);
                let bytes = cc_markup::xlsx::write_xlsx(&sheets, &name, true, now);
                let file = format!("{name}.xlsx");
                let msg = t("sum.saved.xlsx", &[("n", Fill::N(n)), ("file", Fill::S(file.clone()))]);
                (file, bytes, msg)
            }
            "pdf" => {
                let name = t("sum.file", &[("set", Fill::S(set.clone()))]);
                let title = format!("{}: {set}", t0("sum.title"));
                let r = cc_io_markup::summary::summary_pdf(&list, &sheet_of, &measure_of, &title, now, now);
                let file = format!("{name}.pdf");
                let mut msg = t("sum.saved.pdf", &[("n", Fill::N(n)), ("file", Fill::S(file.clone())), ("pages", Fill::N(r.pages as f64))]);
                if r.missing > 0 {
                    msg.push_str(&t("sum.missing", &[("n", Fill::N(r.missing as f64))]));
                }
                (file, r.bytes, msg)
            }
            _ => return Err(format!("markup.export: unknown export {kind} (csv, xlsx, pdf)")),
        };
        self.tools.message = msg;
        Ok(Some((file, bytes)))
    }

    /// An export to the host's save (through `outbox`).
    pub fn export_to_outbox(&mut self, kind: &str) {
        match self.export_list(kind) {
            Ok(Some(f)) => self.outbox.push(f),
            Ok(None) => {}
            Err(e) => self.tools.message = e,
        }
    }

    // ---- panels ----

    /// The right panel: Properties, Markups, Tools, Compare (tabs as markupcraft's).
    pub fn right_panel(&mut self, ui: &mut egui::Ui, tok: &Tokens) {
        ui.horizontal_wrapped(|ui| {
            for k in ["props", "list", "chest", "compare"] {
                if ui.selectable_label(self.tab == k, t0(&format!("tab.{k}"))).clicked() {
                    self.tab = k;
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.tab {
            "list" => self.list_panel(ui, tok),
            "chest" => self.chest_panel(ui, tok),
            "compare" => self.compare_panel(ui, tok),
            _ => self.props_panel(ui, tok),
        });
    }

    /// The Markups tab (renderList): filters, order, the count line, a row per markup (a click jumps to it), exports.
    pub fn list_panel(&mut self, ui: &mut egui::Ui, tok: &Tokens) {
        let all: Vec<Markup> = self.store.all().into_iter().cloned().collect();
        let sheets: Vec<(String, String)> =
            self.sheet_list().into_iter().filter(|s| all.iter().any(|m| m.sheet() == s.key)).map(|s| (s.key, s.info.title)).collect();
        let mut authors: Vec<String> = all.iter().map(|m| m.author.clone()).filter(|a| !a.is_empty()).collect();
        authors.sort();
        authors.dedup();
        let mut kinds: Vec<String> = Vec::new();
        for m in &all {
            if !kinds.contains(&m.kind) {
                kinds.push(m.kind.clone());
            }
        }
        let mut spaces: Vec<String> = all.iter().filter(|m| m.kind == "space" && !m.label.is_empty()).map(|m| m.label.clone()).collect();
        spaces.sort();
        spaces.dedup();
        let combo = |ui: &mut egui::Ui, id: &str, label: &str, cur: &mut String, opts: Vec<(String, String)>| {
            let shown =
                opts.iter().find(|(v, _)| v == cur).map_or_else(|| opts.first().map(|o| o.1.clone()).unwrap_or_default(), |o| o.1.clone());
            let l = ui.label(RichText::new(label).small());
            egui::ComboBox::new(id, "")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for (v, l) in &opts {
                        ui.selectable_value(cur, v.clone(), l);
                    }
                })
                .response
                .labelled_by(l.id);
        };
        let mut f = self.list.clone();
        egui::Grid::new("mc-list-filters").num_columns(2).show(ui, |ui| {
            let mut o = vec![(String::new(), t0("filter.sheet.all")), ("current".into(), t0("filter.sheet.current"))];
            o.extend(sheets.clone());
            combo(ui, "mc-f-sheet", &t0("filter.sheet"), &mut f.sheet, o);
            ui.end_row();
            let mut o = vec![(String::new(), t0("filter.author.any"))];
            o.extend(authors.iter().map(|a| (a.clone(), a.clone())));
            combo(ui, "mc-f-author", &t0("filter.author"), &mut f.author, o);
            ui.end_row();
            let mut o = vec![(String::new(), t0("filter.status.any"))];
            o.extend(STATUSES.iter().map(|s| (s.to_string(), cc_markup::model::status_name(s))));
            combo(ui, "mc-f-status", &t0("filter.status"), &mut f.status, o);
            ui.end_row();
            let mut o = vec![(String::new(), t0("filter.type.any"))];
            o.extend(kinds.iter().map(|k| (k.clone(), type_name(k))));
            combo(ui, "mc-f-type", &t0("filter.type"), &mut f.kind, o);
            ui.end_row();
            let mut o = vec![(String::new(), t0("filter.space.any")), (NONE.into(), t0("space.none"))];
            o.extend(spaces.iter().map(|s| (s.clone(), s.clone())));
            combo(ui, "mc-f-space", &t0("filter.space"), &mut f.space, o);
            ui.end_row();
            let mut o = vec![(String::new(), t0("filter.layer.any")), (NONE.into(), t0("layer.none"))];
            o.extend(self.store.layer_names().into_iter().map(|n| (n.clone(), n)));
            combo(ui, "mc-f-layer", &t0("filter.layer"), &mut f.layer, o);
            ui.end_row();
            let mut o = vec![(String::new(), t0("filter.field.any"))];
            for c in &self.store.columns {
                let mut vals: Vec<String> = all.iter().filter_map(|m| m.fields.get(&c.key).cloned()).filter(|v| !v.is_empty()).collect();
                vals.sort();
                vals.dedup();
                o.extend(vals.into_iter().take(100).map(|v| (format!("{}\u{1}{v}", c.key), format!("{}: {v}", c.name))));
            }
            for c in &self.store.columns {
                o.push((format!("{}\u{1}", c.key), t("filter.field.empty", &[("col", Fill::S(c.name.clone()))])));
            }
            combo(ui, "mc-f-field", &t0("filter.field"), &mut f.field, o);
            ui.end_row();
            let mut o = vec![
                (String::new(), t0("sort.sheet")),
                ("author".into(), t0("filter.author")),
                ("status".into(), t0("filter.status")),
                ("type".into(), t0("filter.type")),
                ("created".into(), t0("csv.created")),
                ("modified".into(), t0("csv.modified")),
            ];
            o.extend(self.store.columns.iter().map(|c| (format!("col:{}", c.key), c.name.clone())));
            combo(ui, "mc-f-sort", &t0("sort.label"), &mut f.sort, o);
            ui.end_row();
        });
        self.list = f;
        let mut export: Option<&str> = None;
        ui.horizontal_wrapped(|ui| {
            for (k, label) in [("csv", "list.csv"), ("xlsx", "list.xlsx"), ("pdf", "list.pdf")] {
                if ui.button(t0(label)).clicked() {
                    export = Some(k);
                }
            }
        });
        if let Some(k) = export {
            self.export_to_outbox(k);
        }
        let rows: Vec<Markup> = self.list_rows().into_iter().cloned().collect();
        let count = if rows.len() == all.len() {
            t("save.markups", &[("n", Fill::N(all.len() as f64))])
        } else {
            t("list.count.filtered", &[("shown", Fill::N(rows.len() as f64)), ("n", Fill::N(all.len() as f64))])
        };
        ui.label(RichText::new(count).color(tok.text_dim));
        let mut jump: Option<String> = None;
        for m in rows.iter().take(LIST_MAX) {
            let text = if m.kind == "stamp" {
                m.label.clone()
            } else if crate::markuptools::text_type(&m.kind) {
                m.text.clone()
            } else {
                self.value_of(m)
            };
            let title = format!(
                "{}{}",
                m.punch.as_ref().map(|p| format!("#{} ", cc_markup::js::ns(p.n))).unwrap_or_default(),
                if !m.subject.is_empty() {
                    m.subject.clone()
                } else if m.locked {
                    m.src.as_ref().map(|s| s.subtype.clone()).unwrap_or_default()
                } else {
                    type_name(&m.kind)
                }
            );
            let sheet = self.locate(m.sheet()).map(|(f, i)| f.title(&self.store, i)).unwrap_or_default();
            let meta: Vec<String> = [sheet, m.author.clone(), cc_markup::js::short_date_en(cc_markup::js::parse_date(&m.created))]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
            let chip = if m.locked { t0("props.read_only") } else { cc_markup::model::status_name(&m.status) };
            let on = self.tools.sel.contains(&m.id);
            let colour = crate::pdfview::hex_color(&m.style.color, 1.0).unwrap_or(tok.text);
            let resp = ui
                .scope(|ui| {
                    let r = ui.selectable_label(on, RichText::new(&title).strong().color(tok.text_strong));
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("■").color(colour));
                        let body: Vec<&str> = [text.as_str(), m.comment.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
                        if !body.is_empty() {
                            ui.label(RichText::new(body.join(" — ")).small());
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(meta.join(", ")).small().color(tok.text_dim));
                        ui.label(RichText::new(&chip).small().color(tok.accent));
                        if !m.replies.is_empty() {
                            ui.label(
                                RichText::new(t("list.replies", &[("n", Fill::N(m.replies.len() as f64))])).small().color(tok.text_dim),
                            );
                        }
                    });
                    r
                })
                .inner;
            if resp.clicked() {
                jump = Some(m.id.clone());
            }
            ui.separator();
        }
        if rows.is_empty() {
            ui.label(RichText::new(t0(if all.is_empty() { "list.empty" } else { "list.no_match" })).color(tok.text_dim));
        }
        if let Some(id) = jump {
            let _ = self.jump_to(&id);
        }
    }

    /// The replies of the selected markup and the box to reply (threadSection).
    pub fn thread_section(&mut self, ui: &mut egui::Ui, tok: &Tokens, m: &Markup) {
        let n = m.replies.len();
        ui.label(
            RichText::new(if n > 0 { t("thread.h.count", &[("n", Fill::N(n as f64))]) } else { t0("thread.label") })
                .strong()
                .color(tok.text_strong),
        );
        if n == 0 {
            ui.label(RichText::new(t0("thread.none")).color(tok.text_dim));
        }
        for r in &m.replies {
            let who = if r.author.is_empty() { t0("thread.unknown") } else { r.author.clone() };
            let date = r.date.replace('T', " ").trim_end_matches('Z').to_string();
            ui.label(RichText::new(t("thread.meta", &[("author", Fill::S(who)), ("date", Fill::S(date))])).small().color(tok.text_dim));
            if !r.text.is_empty() {
                ui.label(&r.text);
            }
        }
        let draft = self.reply_drafts.entry(m.id.clone()).or_default();
        let l = ui.label(RichText::new(t0("thread.reply")).small());
        let w = ui.available_width();
        ui.add(egui::TextEdit::multiline(draft).desired_rows(2).desired_width(w).hint_text(t0("thread.placeholder"))).labelled_by(l.id);
        let (send, photo) = ui
            .horizontal(|ui| {
                (ui.button(t0("thread.reply")).clicked(), ui.button(t0("photo.add")).on_hover_text(t0("photo.add.title")).clicked())
            })
            .inner;
        if photo {
            self.want_pick = Some("photo");
        }
        if send {
            let text = self.reply_drafts.get(&m.id).cloned().unwrap_or_default();
            if let Err(e) = self.reply(&text) {
                self.tools.message = e;
            }
        }
    }

    /// The Pages menu and Rename sheet (on the tool bar).
    pub fn pages_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button(t0("pages.menu"), |ui| {
            let mut act: Option<&'static str> = None;
            for (k, label) in [
                ("rotate-cw", "pages.page_rotate_cw"),
                ("rotate-ccw", "pages.page_rotate_ccw"),
                ("move-up", "pages.page_move_up"),
                ("move-down", "pages.page_move_down"),
                ("insert-pdf", "pages.page_insert_pdf"),
                ("insert-blank", "pages.page_insert_blank"),
                ("delete", "pages.page_delete"),
                ("extract", "pages.page_extract"),
            ] {
                if ui.button(t0(label)).clicked() {
                    act = Some(k);
                }
            }
            if self.cur.blank.is_some() && ui.button(t0("sheets.add_blank")).clicked() {
                act = Some("add-blank");
            }
            ui.separator();
            if ui.button(t0("props.rename")).clicked() {
                act = Some("rename");
            }
            if let Some(a) = act {
                ui.close();
                match a {
                    "rename" => {
                        let (number, title) = self.rename_start();
                        self.dlg = Some(Dialog::Rename { number, title });
                    }
                    "delete" | "extract" => {
                        self.dlg = Some(Dialog::Pages { kind: a, text: (self.page + 1).to_string(), error: String::new() })
                    }
                    "insert-pdf" => self.want_pick = Some("insert-pdf"),
                    k => {
                        if let Err(e) = self.pages_op(k, "", None) {
                            self.tools.message = e;
                        }
                    }
                }
            }
        });
    }

    /// The dialog over the sheet, if one is open.
    pub fn dialogs(&mut self, ctx: &egui::Context, tok: &Tokens) {
        let Some(d) = self.dlg.clone() else { return };
        let mut close = false;
        match d {
            Dialog::Signature { .. } => {
                self.signature_dialog(ctx, tok);
                return;
            }
            Dialog::ChestName { mut name, mut error } => {
                let mut save = false;
                let kind = self.tools.selected().and_then(|id| self.store.get(id)).map(|m| m.kind.clone()).unwrap_or_default();
                egui::Window::new(t0("chest.save.one")).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(t0(if kind == "stamp" { "chest.dlg.text.stamp" } else { "chest.dlg.text" }));
                    let l = ui.label(t0("chest.dlg.name"));
                    ui.text_edit_singleline(&mut name).labelled_by(l.id);
                    if !error.is_empty() {
                        ui.colored_label(ui.visuals().error_fg_color, &error);
                    }
                    ui.horizontal(|ui| {
                        close = ui.button(t0("dlg.cancel")).clicked();
                        save = ui.button(t0("chest.dlg.save")).clicked();
                    });
                });
                if save {
                    match self.save_to_chest(&name) {
                        Ok(()) => close = true,
                        Err(e) => error = e,
                    }
                }
                if !close {
                    self.dlg = Some(Dialog::ChestName { name, error });
                }
            }
            Dialog::RemoveTool { index } => {
                let name = self.tools.chest.get(index).map(|t| crate::markupchest::tstr(t, "name")).unwrap_or_default();
                let mut ok = false;
                egui::Window::new(t0("chest.remove.h")).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(t("chest.remove.text", &[("tool", Fill::S(name))]));
                    ui.horizontal(|ui| {
                        close = ui.button(t0("dlg.cancel")).clicked();
                        ok = ui.button(t0("chest.remove.ok")).clicked();
                    });
                });
                if ok {
                    let _ = self.remove_tool(index);
                    close = true;
                }
            }
            Dialog::Rename { mut number, mut title } => {
                let mut save = false;
                egui::Window::new(t0("props.rename")).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(t0("rename.text"));
                    let l = ui.label(t0("props.sheet_number"));
                    ui.text_edit_singleline(&mut number).labelled_by(l.id);
                    let l = ui.label(t0("rename.title"));
                    ui.text_edit_singleline(&mut title).labelled_by(l.id);
                    ui.horizontal(|ui| {
                        close = ui.button(t0("dlg.cancel")).clicked();
                        save = ui.button(t0("dlg.save")).clicked();
                    });
                });
                if save {
                    self.rename_sheet(&number, &title);
                    close = true;
                } else if !close {
                    self.dlg = Some(Dialog::Rename { number, title });
                }
            }
            Dialog::Pages { kind, mut text, mut error } => {
                let n = self.cur.pages.len();
                let (h, body, ok) = if kind == "delete" {
                    ("pages.delete.title", "pages.delete.text", "pages.delete.ok")
                } else {
                    ("pages.extract.title", "pages.extract.text", "pages.extract.ok")
                };
                let mut go = false;
                egui::Window::new(t0(h)).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(t0(body));
                    let l = ui.label(t("pages.ask.label", &[("n", Fill::N(n as f64))]));
                    ui.text_edit_singleline(&mut text).labelled_by(l.id);
                    ui.label(
                        RichText::new(t("pages.ask.hint", &[("name", Fill::S(self.cur.name.clone())), ("n", Fill::N(n as f64))])).small(),
                    );
                    if !error.is_empty() {
                        ui.colored_label(ui.visuals().error_fg_color, &error);
                    }
                    ui.horizontal(|ui| {
                        close = ui.button(t0("dlg.cancel")).clicked();
                        go = ui.button(t0(ok)).clicked();
                    });
                });
                if go {
                    match cc_markup::pages::parse_page_list(&text, n as f64) {
                        Err(e) => error = e,
                        Ok(_) => match self.pages_op(kind, &text, None) {
                            Ok(()) => close = true,
                            Err(e) => error = e,
                        },
                    }
                }
                if !close {
                    self.dlg = Some(Dialog::Pages { kind, text, error });
                }
            }
        }
        if close {
            self.dlg = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(pages: u32) -> Vec<u8> {
        cc_io_markup::pages::blank_pdf(Some(&json!({ "paper": "letter", "grid": "none", "pages": pages })), "", 0.0)
    }

    fn rect(v: &mut PdfView, a: P, b: P) {
        let key = v.sheet_key();
        v.tools.set_tool(&mut v.store, &key, "rect");
        v.gesture_down(a, v.screen_of(a), false, false);
        v.tools.mv(&v.store, b, v.screen_of(b), false);
        v.gesture_up(b, v.screen_of(b), false);
    }

    #[test]
    fn range_text_joins_runs() {
        assert_eq!(range_text(&[0.0, 1.0, 2.0, 4.0]), "1-3, 5");
        assert_eq!(range_text(&[3.0]), "4");
    }

    #[test]
    fn copy_paste_duplicate_and_cut_are_one_step_each() {
        let mut v = PdfView::open("a.pdf", &blank(2)).unwrap();
        v.set_zoom(1.0);
        v.clock = Some(1_791_806_400_000.0);
        rect(&mut v, [100.0, 100.0], [200.0, 160.0]);
        assert!(!v.copy("copy") || v.clip.is_some());
        v.paste("offset", None);
        assert_eq!(v.store.on_sheet("f1:0").len(), 2);
        // beside the copy: 12 screen pixels down and right
        let m = v.store.get(&v.tools.sel[0]).unwrap();
        assert_eq!(m.rect, Some([112.0, 88.0, 212.0, 148.0]));
        v.paste("place", None);
        v.duplicate();
        assert_eq!(v.store.on_sheet("f1:0").len(), 4);
        v.select_all();
        assert_eq!(v.tools.sel.len(), 4);
        v.cut();
        assert!(v.store.on_sheet("f1:0").is_empty());
        v.history(false);
        assert_eq!(v.store.on_sheet("f1:0").len(), 4);
        assert!(v.tools.message.starts_with("Undid cut"), "{}", v.tools.message);
        // nothing selected: the status line says so
        v.tools.sel.clear();
        assert!(!v.copy("copy"));
        assert_eq!(v.tools.message, t0("clip.copy.need"));
    }

    #[test]
    fn replies_rename_and_the_list() {
        let mut v = PdfView::open("a.pdf", &blank(2)).unwrap();
        v.set_zoom(1.0);
        rect(&mut v, [100.0, 100.0], [200.0, 160.0]);
        let id = v.tools.sel[0].clone();
        let modified = v.store.get(&id).unwrap().modified.clone();
        v.reply("Looks right").unwrap();
        let m = v.store.get(&id).unwrap();
        assert_eq!(m.replies.len(), 1);
        assert_eq!(m.modified, modified, "a reply does not touch the markup");
        v.rename_sheet("A-101", "Floor plan");
        assert_eq!(v.cur.title(&v.store, 0), "A-101 — Floor plan");
        v.rename_sheet("", "");
        assert_eq!(v.tools.message, t0("rename.cleared"));
        v.go_file("f1", 1);
        rect(&mut v, [10.0, 10.0], [50.0, 50.0]);
        v.set_filter("sheet", "current").unwrap();
        assert_eq!(v.filtered().len(), 1);
        v.set_filter("sheet", "").unwrap();
        assert_eq!(v.list_rows().len(), 2);
        let (name, bytes) = v.export_list("csv").unwrap().unwrap();
        assert_eq!(name, "a markups.csv");
        assert!(String::from_utf8(bytes).unwrap().lines().count() == 3);
        v.jump_to(&id).unwrap();
        assert_eq!(v.sheet_key(), "f1:0");
    }

    #[test]
    fn pages_ops_keep_the_markups_with_their_pages() {
        let mut v = PdfView::open("a.pdf", &blank(3)).unwrap();
        v.set_zoom(1.0);
        v.clock = Some(1_791_806_400_000.0);
        rect(&mut v, [100.0, 100.0], [200.0, 160.0]);
        v.pages_op("move-down", "", None).unwrap();
        assert_eq!(v.page, 1);
        assert_eq!(v.store.on_sheet("f1:1").len(), 1, "the markup moved with its page");
        assert!(v.store.undo_stack.is_empty());
        v.pages_op("insert-blank", "", None).unwrap();
        assert_eq!(v.cur.pages.len(), 4);
        v.pages_op("delete", "3", None).unwrap();
        assert_eq!(v.cur.pages.len(), 3);
        assert!(v.pages_op("delete", "1-3", None).is_err(), "a PDF keeps one page");
        v.pages_op("rotate-cw", "", None).unwrap();
        v.pages_op("extract", "1-2", None).unwrap();
        assert_eq!(v.outbox.len(), 1);
        assert_eq!(v.outbox[0].0, "a (pages 1-2).pdf");
        v.go_file("f1", 0);
        v.pages_op("move-up", "", None).unwrap();
        assert_eq!(v.tools.message, t("pages.first", &[("name", Fill::S("a.pdf".into()))]));
    }
}
