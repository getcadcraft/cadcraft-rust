//! MarkupCraft's Find text, sheet-callout links, bookmarks and custom columns (M9.24), ported from markupcraft.js:
//! Find (runFind / renderFind / showHit / cloudFound: every sheet's own text read as pdf.js `getTextContent` gives it —
//! cc-io-markup `text_content` — indexed and searched by cc-markup textsearch; hits listed and drawn on the sheet,
//! Cloud all clouds them in one undo step), Link sheet callouts (autoLink: "3/A-501" and the like found in the drawings'
//! text, reviewed in a window, then made into thin blue link boxes in one undo step), the open file's bookmarks
//! (renderBookmarks / bookmarkAct: follow, rename, remove, build from the sheets, add one for this sheet; written with the
//! file when it is saved) and the custom columns editor (editColumns) with each column's field in Properties (setField).
//! Each action is the JS handler with the 0.5 undo label and status line; the Find panel's count line and a window's
//! text are kept in `note` for the headless trace.

use std::collections::HashMap;

use cc_io_markup::read::Bookmark;
use cc_markup::columns::{Column, columns_to_value, field_ok, new_column_key};
use cc_markup::i18n::{Fill, t, t0};
use cc_markup::model::{Markup, make_markup};
use cc_markup::slip::sheet_no_key;
use cc_markup::textsearch::{Index, Item, find_hits, hit_cloud_rect, range_box, text_index};
use egui::{Color32, Pos2, RichText};
use serde_json::{Map, Value, json};

use crate::pdfview::PdfView;
use crate::theme::Tokens;

/// The most hits one search keeps (runFind's 2000), the hits listed (500) and the callouts the review lists (400).
const FIND_MAX: usize = 2000;
const LIST_MAX: usize = 500;
const AUTOLINK_MAX: usize = 400;
/// Titles: askText keeps 200 characters.
const TITLE_MAX: usize = 200;

/// One hit of Find: its sheet, its box (PDF user space) and the words round it.
#[derive(Debug, Clone)]
pub struct FoundHit {
    pub key: String,
    pub rect: [f64; 4],
    pub snippet: String,
}

/// One callout autoLink found: the sheet it is on (key, the label the review shows, its turn), the words, the sheet it
/// names and the box round the words.
#[derive(Debug, Clone)]
pub struct LinkRow {
    pub key: String,
    pub label: String,
    pub rot: f64,
    pub words: String,
    pub no: String,
    pub bx: [f64; 4],
}

/// The windows of this module.
#[derive(Debug, Clone)]
pub enum FDialog {
    /// ask(title, text, [OK])
    Notice { title: String, text: String },
    /// autoLink's review: the text, the rows (≤ 400) and which are ticked
    AutoLink { text: String, rows: Vec<LinkRow>, checked: Vec<bool> },
    /// askText for a bookmark: Add (None) or Rename (its row), the title typed, the check's message
    BmText { rename: Option<usize>, value: String, error: String },
    /// Build from sheets over existing bookmarks (how many there are)
    BmBuild { n: usize },
    /// editColumns: the columns being edited, each choice column's typed choices, the check's message
    Columns { cols: Vec<Column>, choices: Vec<String>, error: String },
    /// slipSheet's review: its text, the pairs, which are ticked, Leave closed markups, Stamp the old sheets
    Slip { text: String, rows: Vec<crate::markupslip::SlipRow>, checked: Vec<bool>, skip: bool, stamp: bool },
    /// saveFlat: how many markups the file has and are selected, the selected ones chosen
    Flatten { all: usize, sel: usize, use_sel: bool },
    /// printSheets: what to print, the page list, the check's message
    Print { which: &'static str, pages: String, error: String },
    /// closeSet with unsaved markups
    Unsaved,
}

/// The Find panel's state, the text read from each sheet, and the panels' open / closed state.
#[derive(Default)]
pub struct FindState {
    /// the search field and its options
    pub q: String,
    pub sheet_only: bool,
    pub match_case: bool,
    pub whole_word: bool,
    /// what was found: the query, the hits, the one shown (`found`)
    pub query: String,
    pub hits: Vec<FoundHit>,
    pub at: Option<usize>,
    /// the panel's count line
    pub count: String,
    /// each sheet's text index (dropped with its file) and the read documents they came from, by file id
    texts: HashMap<String, std::sync::Arc<Index>>,
    docs: HashMap<String, cc_io_markup::jsdoc::JsDoc>,
}

impl FindState {
    /// forgetText(prefix): a file's text and hits go when it closes or its pages change ('' = every file).
    pub fn forget(&mut self, prefix: &str) {
        self.texts.retain(|k, _| !k.starts_with(prefix));
        let fid = prefix.trim_end_matches(':');
        if prefix.is_empty() {
            self.docs.clear();
            self.query.clear();
        } else {
            self.docs.remove(fid);
        }
        let n = self.hits.len();
        self.hits.retain(|h| !h.key.starts_with(prefix));
        if self.hits.len() != n || prefix.is_empty() {
            self.at = None;
        }
    }
}

/// The bookmarks as rows in walk order: (path of indices, depth, title, page).
fn bm_rows(items: &[Bookmark]) -> Vec<(Vec<usize>, usize, String, f64)> {
    fn walk(items: &[Bookmark], depth: usize, path: &mut Vec<usize>, out: &mut Vec<(Vec<usize>, usize, String, f64)>) {
        for (i, b) in items.iter().enumerate() {
            path.push(i);
            out.push((path.clone(), depth, b.title.clone(), b.page));
            walk(&b.children, depth.saturating_add(1), path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(items, 0, &mut Vec::new(), &mut out);
    out
}

/// The list a path's last index is in, and that index.
fn bm_slot<'a>(items: &'a mut Vec<Bookmark>, path: &[usize]) -> Option<(&'a mut Vec<Bookmark>, usize)> {
    let (last, head) = path.split_last()?;
    let mut list = items;
    for i in head {
        list = &mut list.get_mut(*i)?.children;
    }
    (*last < list.len()).then_some((list, *last))
}

/// `count(items)`.
fn bm_count(items: &[Bookmark]) -> usize {
    items.iter().map(|b| bm_count(&b.children).saturating_add(1)).fold(0usize, usize::saturating_add)
}

/// A text as JS `slice(0, n)` keeps it (UTF-16 units).
fn slice16(s: &str, n: usize) -> String {
    cc_markup::js::slice16(s, n)
}

impl PdfView {
    // ---- the text of the sheets ----

    /// `sheetText(s)`: the sheet's text index (read once; a sheet that cannot be read has no text).
    fn sheet_text(&mut self, key: &str) -> std::sync::Arc<Index> {
        if let Some(ix) = self.find.texts.get(key) {
            return ix.clone();
        }
        let loc = self.locate(key).map(|(f, i)| (f.id.clone(), i, f.bytes.clone()));
        let mut idx = Index::default();
        if let Some((fid, i, bytes)) = loc {
            if !self.find.docs.contains_key(&fid)
                && let Ok(d) = cc_io_markup::jsdoc::JsDoc::open(&bytes)
            {
                self.find.docs.insert(fid.clone(), d);
            }
            if let Some(doc) = self.find.docs.get(&fid) {
                let items: Vec<Item> = cc_io_markup::text::text_content(doc, i)
                    .into_iter()
                    .map(|x| Item { str: x.str, transform: Some(x.transform.to_vec()), width: x.width, has_eol: x.has_eol })
                    .collect();
                idx = text_index(items);
            }
        }
        let ix = std::sync::Arc::new(idx);
        self.find.texts.insert(key.to_string(), ix.clone());
        ix
    }

    /// A sheet's number, else its title (the hit list's and the review's bold part).
    fn sheet_label(&self, key: &str) -> String {
        self.locate(key)
            .map(|(f, i)| {
                let info = f.info(&self.store, i);
                if info.sheet_no.is_empty() { info.title } else { info.sheet_no }
            })
            .unwrap_or_default()
    }

    /// A sheet's turn as MarkupCraft read it (`page.rotate`).
    fn sheet_rot(&self, key: &str) -> f64 {
        self.locate(key).and_then(|(f, i)| f.page_views.get(i).map(|v| v.1)).unwrap_or(0.0)
    }

    // ---- Find ----

    /// runFind: the words in the field, on every sheet or this one; the hits listed and drawn.
    pub fn run_find(&mut self) {
        let q = cc_markup::js::trim(&self.find.q).to_string();
        if q.is_empty() {
            self.find.count = t0("find.need");
            self.note = self.find.count.clone();
            return;
        }
        let keys = if self.find.sheet_only { vec![self.sheet_key()] } else { self.sheet_keys() };
        let (mc, ww) = (self.find.match_case, self.find.whole_word);
        let mut hits: Vec<FoundHit> = Vec::new();
        let mut blank = 0usize;
        for k in keys {
            let idx = self.sheet_text(&k);
            if cc_markup::js::trim(&idx.text).is_empty() {
                blank = blank.saturating_add(1);
                continue;
            }
            let room = FIND_MAX.saturating_sub(hits.len());
            for h in find_hits(&idx, &q, mc, ww, room as f64) {
                hits.push(FoundHit { key: k.clone(), rect: h.rect, snippet: h.snippet });
            }
            if hits.len() >= FIND_MAX {
                break;
            }
        }
        self.find.query = q;
        self.find.hits = hits;
        self.find.at = None;
        self.render_find(blank);
    }

    /// renderFind: the count line (and the note for the trace: the line and the listed hits).
    fn render_find(&mut self, blank: usize) {
        if self.find.query.is_empty() {
            self.find.count.clear();
            return;
        }
        let mut keys: Vec<&str> = Vec::new();
        for h in &self.find.hits {
            if !keys.contains(&h.key.as_str()) {
                keys.push(&h.key);
            }
        }
        let n = self.find.hits.len() as f64;
        let mut parts =
            vec![t("find.count", &[("n", Fill::N(n)), ("sheets", Fill::S(t("find.sheets", &[("n", Fill::N(keys.len() as f64))])))])];
        if blank > 0 {
            parts.push(t("find.no_text", &[("n", Fill::N(blank as f64))]));
        }
        if self.find.hits.len() >= FIND_MAX {
            parts.push(t("find.capped", &[("n", Fill::N(n))]));
        }
        self.find.count = parts.join(" ");
        let rows: Vec<String> =
            self.find.hits.iter().take(LIST_MAX).map(|h| format!("{} {}", self.sheet_label(&h.key), h.snippet)).collect();
        self.note = std::iter::once(self.find.count.clone()).chain(rows).collect::<Vec<_>>().join(" | ");
    }

    /// showHit(i): its sheet, zoomed to the words.
    pub fn show_hit(&mut self, i: usize) -> Result<(), String> {
        let h = self.find.hits.get(i).cloned().ok_or_else(|| format!("markup.find: no hit {i}"))?;
        self.find.at = Some(i);
        if h.key != self.sheet_key()
            && let Some((fid, p)) = self.locate(&h.key).map(|(f, p)| (f.id.clone(), p))
        {
            // goTo: what was being drawn ends, the selection goes
            self.settle();
            self.tools.sel.clear();
            self.go_file(&fid, p);
        }
        self.zoom_fill(h.rect, 0.25);
        Ok(())
    }

    /// view.js `zoomTo(box, fill)`: the box takes `fill` of the view (at least 20 points a side counted), centred.
    fn zoom_fill(&mut self, b: [f64; 4], fill: f64) {
        let Some(sz) = self.view_size else { return };
        let z0 = self.zoom;
        let (a, c) = (self.screen_of([b[0], b[1]]), self.screen_of([b[2], b[3]]));
        let bw = ((c[0] - a[0]).abs() / z0).max(20.0);
        let bh = ((c[1] - a[1]).abs() / z0).max(20.0);
        let z = (f64::from(sz.x) * fill / bw).min(f64::from(sz.y) * fill / bh);
        self.set_zoom(z);
        let m = self.screen_of([(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0]);
        self.pan = egui::vec2((f64::from(sz.x) / 2.0 - m[0]) as f32, (f64::from(sz.y) / 2.0 - m[1]) as f32);
    }

    /// cloudFound: a revision cloud round every hit, one undo step; those on this sheet are selected.
    pub fn cloud_found(&mut self) -> Result<(), String> {
        if self.find.hits.is_empty() {
            self.tools.message = t0("find.cloud.none");
            return Ok(());
        }
        self.tick();
        let author = self.tools.author.clone();
        let style = self.tools.style_for("cloudrect");
        let comment = t("find.cloud.comment", &[("query", Fill::S(self.find.query.clone()))]);
        let hits = self.find.hits.clone();
        let mut keys: Vec<String> = Vec::new();
        let mut made: Vec<Markup> = Vec::new();
        for h in &hits {
            if !keys.contains(&h.key) {
                keys.push(h.key.clone());
            }
            let pad = (h.rect[3] - h.rect[1]) * 0.35;
            let pad = if pad > 2.0 { pad } else { 2.0 };
            let mut f = Map::new();
            f.insert("sheet".into(), json!(h.key));
            f.insert("type".into(), json!("cloudrect"));
            f.insert("rot".into(), json!(self.sheet_rot(&h.key)));
            f.insert("author".into(), json!(author));
            f.insert("subject".into(), json!(""));
            f.insert("rect".into(), json!(hit_cloud_rect(h.rect, pad)));
            f.insert("comment".into(), json!(comment));
            let mut m = make_markup(&mut self.store.env, &f);
            m.style = style.clone();
            made.push(m);
        }
        let n = made.len() as f64;
        let cur = self.sheet_key();
        let sel: Vec<String> = made.iter().filter(|m| m.sheet() == cur).map(|m| m.id.clone()).collect();
        self.store
            .change(&t("undo.find_cloud", &[("n", Fill::N(n))]), |tx| {
                for m in made {
                    tx.add(m)?;
                }
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        self.tools.sel = sel;
        self.tools.message =
            t("find.clouded", &[("n", Fill::N(n)), ("sheets", Fill::S(t("find.sheets", &[("n", Fill::N(keys.len() as f64))])))]);
        Ok(())
    }

    /// The hits on the sheet on screen, drawn over it (the one shown stronger).
    pub(crate) fn draw_found(&self, painter: &egui::Painter, at: &dyn Fn(f64, f64) -> Pos2) {
        let key = self.sheet_key();
        for (i, h) in self.find.hits.iter().enumerate() {
            if h.key != key {
                continue;
            }
            let r = egui::Rect::from_two_pos(at(h.rect[0], h.rect[1]), at(h.rect[2], h.rect[3])).expand(2.0);
            let c = if self.find.at == Some(i) {
                Color32::from_rgba_unmultiplied(255, 170, 0, 115)
            } else {
                Color32::from_rgba_unmultiplied(255, 200, 0, 71)
            };
            painter.rect_filled(r, 0.0, c);
        }
    }

    // ---- Link sheet callouts ----

    /// autoLink: every callout naming another open sheet, not already inside a link, for the review window.
    pub fn auto_link(&mut self) {
        self.settle();
        let sheets: Vec<(String, String, String)> = self.sheet_list().into_iter().map(|s| (s.key, s.info.sheet_no, s.info.title)).collect();
        let nos: Vec<String> = sheets.iter().map(|s| s.1.clone()).filter(|n| !n.is_empty()).collect();
        if nos.is_empty() {
            self.notice(t0("autolink.h"), t0("autolink.no_numbers"));
            return;
        }
        let mut found: Vec<LinkRow> = Vec::new();
        let mut blank = 0usize;
        for (key, no, title) in &sheets {
            let idx = self.sheet_text(key);
            if cc_markup::js::trim(&idx.text).is_empty() {
                blank = blank.saturating_add(1);
                continue;
            }
            let linked: Vec<[f64; 4]> = self
                .store
                .on_sheet(key)
                .into_iter()
                .filter(|m| !m.link.is_empty())
                .filter_map(|m| cc_markup::geom::bounds_of(m, &self.env_of(m)))
                .collect();
            let rot = self.sheet_rot(key);
            let label = if no.is_empty() { title.clone() } else { no.clone() };
            for r in cc_markup::link::callout_refs(&idx.text, &nos) {
                if sheet_no_key(&r.no) == sheet_no_key(no) {
                    continue;
                }
                let Some(bx) = range_box(&idx, r.start as f64, r.end as f64) else { continue };
                let c = [(bx[0] + bx[2]) / 2.0, (bx[1] + bx[3]) / 2.0];
                if linked.iter().any(|b| c[0] >= b[0] && c[0] <= b[2] && c[1] >= b[1] && c[1] <= b[3]) {
                    continue;
                }
                found.push(LinkRow { key: key.clone(), label: label.clone(), rot, words: r.r#ref, no: r.no, bx });
            }
        }
        if found.is_empty() {
            let mut text = t0("autolink.none");
            if blank > 0 {
                text = format!("{text} {}", t("find.no_text", &[("n", Fill::N(blank as f64))]));
            }
            self.notice(t0("autolink.h"), text);
            return;
        }
        let mut keys: Vec<&str> = Vec::new();
        for f in &found {
            if !keys.contains(&f.key.as_str()) {
                keys.push(&f.key);
            }
        }
        let mut text = t(
            "autolink.text",
            &[("n", Fill::N(found.len() as f64)), ("sheets", Fill::S(t("find.sheets", &[("n", Fill::N(keys.len() as f64))])))],
        );
        if found.len() > AUTOLINK_MAX {
            text = format!("{text} {}", t0("autolink.capped"));
        }
        found.truncate(AUTOLINK_MAX);
        let rows: Vec<String> = found.iter().map(|f| format!("{} {}", f.label, autolink_row(f))).collect();
        self.note = std::iter::once(text.clone()).chain(rows).collect::<Vec<_>>().join(" | ");
        let checked = vec![true; found.len()];
        self.fdlg = Some(FDialog::AutoLink { text, rows: found, checked });
    }

    /// The review's Create the links: the ticked callouts (`pick`: the rows to keep ticked; None = as they are)
    /// become link boxes, one undo step.
    pub fn auto_link_ok(&mut self, pick: Option<&[usize]>) -> Result<(), String> {
        let Some(FDialog::AutoLink { rows, mut checked, .. }) = self.fdlg.clone() else {
            return Err("markup.autolink: no callouts are being reviewed (run markup.autolink first)".into());
        };
        if let Some(p) = pick {
            for (i, c) in checked.iter_mut().enumerate() {
                *c = p.contains(&i);
            }
        }
        self.fdlg = None;
        let chosen: Vec<LinkRow> = rows.into_iter().zip(checked).filter(|(_, c)| *c).map(|(r, _)| r).collect();
        if chosen.is_empty() {
            return Ok(());
        }
        self.tick();
        let author = self.tools.author.clone();
        let subject = t0("autolink.subject");
        let n = chosen.len() as f64;
        let made: Vec<Markup> = chosen
            .iter()
            .map(|f| {
                let [x0, y0, x1, y1] = f.bx;
                let mut g = Map::new();
                g.insert("sheet".into(), json!(f.key));
                g.insert("type".into(), json!("rect"));
                g.insert("rect".into(), json!([x0 - 2.0, y0 - 2.0, x1 + 2.0, y1 + 2.0]));
                g.insert("rot".into(), json!(f.rot));
                g.insert("author".into(), json!(author));
                g.insert("link".into(), json!(f.no));
                g.insert("subject".into(), json!(subject));
                g.insert(
                    "style".into(),
                    json!({ "color": "#1e48c8", "width": 0.75, "fill": null, "fillOpacity": 0.25, "opacity": 0.85, "fontSize": 12 }),
                );
                make_markup(&mut self.store.env, &g)
            })
            .collect();
        self.store
            .change(&t("undo.autolink", &[("n", Fill::N(n))]), |tx| {
                for m in made {
                    tx.add(m)?;
                }
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        self.tools.message = t("autolink.done", &[("n", Fill::N(n))]);
        Ok(())
    }

    /// `ask(title, text, [OK])`.
    fn notice(&mut self, title: String, text: String) {
        self.note = text.clone();
        self.fdlg = Some(FDialog::Notice { title, text });
    }

    // ---- bookmarks ----

    /// bmChanged: the file's bookmarks are its unsaved work.
    fn bm_changed(&mut self) {
        self.cur.bookmarks_edited = true;
        for k in self.cur.page_keys.clone() {
            self.store.dirty_sheets.insert(k);
        }
        self.note = self.bm_note();
    }

    /// The Bookmarks panel's note line.
    fn bm_note(&self) -> String {
        let f = &self.cur;
        let head = if f.bookmarks.is_empty() {
            t("bm.none", &[("file", Fill::S(f.name.clone()))])
        } else {
            t("bm.of", &[("file", Fill::S(f.name.clone())), ("n", Fill::N(bm_count(&f.bookmarks) as f64))])
        };
        let rows: Vec<String> =
            bm_rows(&f.bookmarks).into_iter().map(|(_, d, title, page)| format!("{}{title} ({page})", "  ".repeat(d))).collect();
        std::iter::once(head).chain(rows).collect::<Vec<_>>().join(" | ")
    }

    /// The bookmarks of the open sheet's file: `build` (from the sheets, replacing any), `add:TITLE` (for this sheet),
    /// `rename:I:TITLE`, `remove:I`, `go:I` (I = the row, in the panel's order).
    pub fn bookmarks(&mut self, act: &str) -> Result<(), String> {
        let (k, arg) = act.split_once(':').unwrap_or((act, ""));
        let rows = bm_rows(&self.cur.bookmarks);
        let row = |s: &str| -> Result<(Vec<usize>, f64), String> {
            let i: usize = s.parse().map_err(|_| format!("markup.bookmarks: {k}:I takes a row number"))?;
            rows.get(i).map(|r| (r.0.clone(), r.3)).ok_or_else(|| format!("markup.bookmarks: no bookmark row {i}"))
        };
        let title = |s: &str| -> Result<String, String> {
            let v = cc_markup::js::trim(s);
            if v.is_empty() { Err(t0("bm.title.need")) } else { Ok(slice16(v, TITLE_MAX)) }
        };
        match k {
            "build" => {
                let titles: Vec<(f64, String)> = (0..self.cur.pages.len()).map(|i| (i as f64, self.cur.title(&self.store, i))).collect();
                self.cur.bookmarks = titles
                    .into_iter()
                    .map(|(page, ti)| Bookmark { title: cc_markup::pages::plain(&ti), page, children: Vec::new() })
                    .collect();
                self.bm_changed();
                self.tools.message = t("bm.built", &[("n", Fill::N(self.cur.bookmarks.len() as f64))]);
            }
            "add" => {
                let v = title(arg)?;
                self.cur.bookmarks.push(Bookmark { title: v, page: self.page as f64, children: Vec::new() });
                self.bm_changed();
            }
            "rename" => {
                let (i, v) = arg.split_once(':').ok_or("markup.bookmarks: rename:I:TITLE")?;
                let (path, _) = row(i)?;
                let v = title(v)?;
                if let Some((list, j)) = bm_slot(&mut self.cur.bookmarks, &path)
                    && let Some(b) = list.get_mut(j)
                {
                    b.title = v;
                }
                self.bm_changed();
            }
            "remove" => {
                let (path, _) = row(arg)?;
                if let Some((list, j)) = bm_slot(&mut self.cur.bookmarks, &path) {
                    list.remove(j);
                }
                self.bm_changed();
            }
            "go" => {
                let (_, page) = row(arg)?;
                let fid = self.cur.id.clone();
                if page >= 0.0 && (page as usize) < self.cur.pages.len() && page.fract() == 0.0 {
                    self.settle();
                    self.tools.sel.clear();
                    self.go_file(&fid, page as usize);
                } else {
                    self.tools.message = t0("bm.nowhere");
                }
            }
            _ => return Err(format!("markup.bookmarks: unknown action {k} (build, add:TITLE, rename:I:TITLE, remove:I, go:I)")),
        }
        Ok(())
    }

    // ---- custom columns ----

    /// editColumns' Save: the set's columns become `cols` (JSON: [{key?, name, type, choices}]; a column without a key
    /// gets a new one). Refused with the window's message when a name or a choice column's choices are missing.
    pub fn set_columns(&mut self, cols: &Value) -> Result<(), String> {
        let list = cols.as_array().ok_or("markup.columns: name = a JSON list of columns")?;
        let mut out: Vec<Column> = Vec::new();
        for c in list.iter().take(30) {
            let o = c.as_object().ok_or("markup.columns: each column is an object")?;
            let s = |k: &str| o.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let key = match o.get("key").and_then(Value::as_str) {
                Some(k) if cc_markup::columns::is_col_key(k) => k.to_string(),
                _ => {
                    let mut all = self.store.columns.clone();
                    all.extend(out.iter().cloned());
                    new_column_key(&all)
                }
            };
            let kind = s("type");
            let kind = if cc_markup::columns::TYPES_OF_COLUMN.contains(&kind.as_str()) { kind } else { "text".to_string() };
            let choices: Vec<String> = match o.get("choices") {
                Some(Value::Array(a)) => {
                    a.iter().filter_map(Value::as_str).map(|x| cc_markup::js::trim(x).to_string()).filter(|x| !x.is_empty()).collect()
                }
                Some(Value::String(x)) => split_choices(x),
                _ => Vec::new(),
            };
            out.push(Column { key, name: s("name"), kind, choices });
        }
        self.save_columns(out)
    }

    /// The editor's check, then `store.setColumns(cols)` and its status line.
    fn save_columns(&mut self, cols: Vec<Column>) -> Result<(), String> {
        if cols.iter().any(|c| cc_markup::js::trim(&c.name).is_empty()) {
            return Err(t0("col.err.name"));
        }
        if cols.iter().any(|c| c.kind == "choice" && c.choices.is_empty()) {
            return Err(t0("col.err.choices"));
        }
        self.store.set_columns(Some(&columns_to_value(&cols)));
        self.tools.message = t("col.saved", &[("n", Fill::N(self.store.columns.len() as f64))]);
        Ok(())
    }

    /// setField: the selected markup's value in a custom column (checked against the column's kind; '' clears it).
    pub fn field_edit(&mut self, key: &str, v: &str) -> Result<(), String> {
        let col = self.store.columns.iter().find(|c| c.key == key).cloned().ok_or_else(|| format!("markup.prop: no column {key}"))?;
        let id = self.tools.selected().map(str::to_string).ok_or("markup.prop: select one markup")?;
        let m = self.store.get(&id).filter(|m| !m.locked).cloned().ok_or("markup.prop: select one markup")?;
        let err = field_ok(&col, v);
        if !err.is_empty() {
            self.tools.message = t(err, &[("col", Fill::S(col.name.clone()))]);
            return Ok(());
        }
        self.tick();
        let mut f = m.fields.clone();
        let val = cc_markup::js::trim(v);
        if val.is_empty() {
            f.shift_remove(key);
        } else {
            f.insert(key.to_string(), val.to_string());
        }
        let obj: Map<String, Value> = f.into_iter().map(|(k, v)| (k, Value::String(v))).collect();
        let mut patch = Map::new();
        patch.insert("fields".into(), Value::Object(obj));
        self.store
            .change(&t("undo.field", &[("col", Fill::S(col.name.clone()))]), |tx| Ok(tx.update(&id, &patch, true)))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    // ---- panels and windows ----

    /// The Find and Bookmarks sections at the top of the sheets panel.
    pub fn find_panels(&mut self, ui: &mut egui::Ui, tok: &Tokens) {
        crate::shell::fold_row(ui, tok, "mc-find", "search", &t0("find.h"), &t0("find.title"), None, |ui| {
            let l = ui.label(RichText::new(t0("find.label")).small());
            let r = ui.add(crate::shell::field(&mut self.find.q).hint_text(t0("find.placeholder"))).labelled_by(l.id);
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal_wrapped(|ui| {
                let scope = if self.find.sheet_only { t0("find.scope.sheet") } else { t0("find.scope.all") };
                egui::ComboBox::from_id_salt("mc-find-scope").selected_text(scope).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.find.sheet_only, false, t0("find.scope.all"));
                    ui.selectable_value(&mut self.find.sheet_only, true, t0("find.scope.sheet"));
                });
                ui.checkbox(&mut self.find.match_case, t0("find.case"));
                ui.checkbox(&mut self.find.whole_word, t0("find.word"));
            });
            let mut cloud = false;
            ui.horizontal(|ui| {
                if ui.add(crate::shell::Btn::new(t0("find.go"))).clicked() || enter {
                    self.run_find();
                }
                cloud = ui
                    .add_enabled(!self.find.hits.is_empty(), crate::shell::Btn::primary(t0("find.cloud")))
                    .on_hover_text(t0("find.cloud.title"))
                    .clicked();
            });
            if cloud && let Err(e) = self.cloud_found() {
                self.tools.message = e;
            }
            if !self.find.count.is_empty() {
                ui.label(RichText::new(&self.find.count).small().color(tok.text_dim));
            }
            let mut show: Option<usize> = None;
            egui::ScrollArea::vertical().id_salt("mc-find-list").max_height(180.0).show(ui, |ui| {
                for (i, h) in self.find.hits.iter().take(LIST_MAX).enumerate() {
                    let label = RichText::new(format!("{}  {}", self.sheet_label(&h.key), h.snippet)).small().color(tok.text);
                    if ui.selectable_label(self.find.at == Some(i), label).clicked() {
                        show = Some(i);
                    }
                }
            });
            if let Some(i) = show {
                let _ = self.show_hit(i);
            }
            if ui.add(crate::shell::Btn::new(t0("autolink.button"))).on_hover_text(t0("autolink.title")).clicked() {
                self.auto_link();
            }
        });
        crate::shell::fold_row(ui, tok, "mc-bookmarks", "bookmark", &t0("bm.h"), &t0("bm.h.title"), None, |ui| {
            let f = &self.cur;
            let note = if f.bookmarks.is_empty() {
                t("bm.none", &[("file", Fill::S(f.name.clone()))])
            } else {
                t("bm.of", &[("file", Fill::S(f.name.clone())), ("n", Fill::N(bm_count(&f.bookmarks) as f64))])
            };
            ui.label(RichText::new(note).small().color(tok.text_dim));
            let mut act: Option<String> = None;
            for (i, (_, depth, title, page)) in bm_rows(&self.cur.bookmarks).into_iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_space(12.0 * depth as f32);
                    let tip = if page >= 0.0 { t("bm.go", &[("page", Fill::N(page + 1.0))]) } else { t0("bm.nowhere") };
                    if ui.selectable_label(false, RichText::new(&title).color(tok.text)).on_hover_text(tip).clicked() {
                        act = Some(format!("go:{i}"));
                    }
                    if icon_button(ui, tok, "pencil").on_hover_text(t("bm.rename", &[("title", Fill::S(title.clone()))])).clicked() {
                        self.fdlg = Some(FDialog::BmText { rename: Some(i), value: title.clone(), error: String::new() });
                    }
                    if icon_button(ui, tok, "x-lg").on_hover_text(t("bm.remove", &[("title", Fill::S(title.clone()))])).clicked() {
                        act = Some(format!("remove:{i}"));
                    }
                });
            }
            ui.horizontal_wrapped(|ui| {
                if ui.add(crate::shell::Btn::new(t0("bm.build"))).clicked() {
                    let n = bm_count(&self.cur.bookmarks);
                    if n > 0 {
                        self.fdlg = Some(FDialog::BmBuild { n });
                    } else {
                        act = Some("build".into());
                    }
                }
                if ui.add(crate::shell::Btn::new(t0("bm.add"))).clicked() {
                    let title = self.cur.title(&self.store, self.page);
                    self.fdlg = Some(FDialog::BmText { rename: None, value: title, error: String::new() });
                }
            });
            if let Some(a) = act
                && let Err(e) = self.bookmarks(&a)
            {
                self.tools.message = e;
            }
        });
    }

    /// Open the custom columns editor (editColumns).
    pub fn edit_columns(&mut self) {
        let cols = self.store.columns.clone();
        let choices = cols.iter().map(|c| c.choices.join(", ")).collect();
        self.fdlg = Some(FDialog::Columns { cols, choices, error: String::new() });
    }

    /// This module's windows.
    pub fn find_dialogs(&mut self, ctx: &egui::Context, tok: &Tokens) {
        let Some(d) = self.fdlg.clone() else { return };
        let mut close = false;
        match d {
            d @ (FDialog::Slip { .. } | FDialog::Flatten { .. } | FDialog::Print { .. } | FDialog::Unsaved) => {
                self.slip_dialogs(ctx, tok, d);
                return;
            }
            FDialog::Notice { title, text } => {
                crate::shell::Dialog::new(title).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(&text);
                    close = ui.add(crate::shell::Btn::primary(t0("dlg.ok"))).clicked();
                });
            }
            FDialog::AutoLink { text, rows, mut checked } => {
                let mut ok = false;
                crate::shell::Dialog::new(t0("autolink.h")).collapsible(false).show(ctx, |ui| {
                    ui.label(&text);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for (r, c) in rows.iter().zip(checked.iter_mut()) {
                            ui.checkbox(c, RichText::new(format!("{}  {}", r.label, autolink_row(r))).color(tok.text));
                        }
                    });
                    crate::shell::foot(ui, |ui| {
                        close = ui.add(crate::shell::Btn::new(t0("dlg.cancel"))).clicked();
                        ok = ui.add(crate::shell::Btn::primary(t0("autolink.ok"))).clicked();
                    });
                });
                if ok {
                    self.fdlg = Some(FDialog::AutoLink { text, rows, checked });
                    if let Err(e) = self.auto_link_ok(None) {
                        self.tools.message = e;
                    }
                    return;
                }
                if !close {
                    self.fdlg = Some(FDialog::AutoLink { text, rows, checked });
                }
            }
            FDialog::BmBuild { n } => {
                let mut ok = false;
                crate::shell::Dialog::new(t0("bm.build.title")).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.label(t("bm.build.replace", &[("n", Fill::N(n as f64))]));
                    crate::shell::foot(ui, |ui| {
                        close = ui.add(crate::shell::Btn::new(t0("dlg.cancel"))).clicked();
                        ok = ui.add(crate::shell::Btn::primary(t0("bm.build.ok"))).clicked();
                    });
                });
                if ok {
                    let _ = self.bookmarks("build");
                    close = true;
                }
            }
            FDialog::BmText { rename, mut value, mut error } => {
                let mut ok = false;
                let title = if rename.is_some() { t0("bm.rename.title") } else { t0("bm.add.title") };
                crate::shell::Dialog::new(title).collapsible(false).resizable(false).show(ctx, |ui| {
                    let l = ui.label(t0("bm.title.label"));
                    ui.add(crate::shell::field(&mut value)).labelled_by(l.id);
                    if !error.is_empty() {
                        ui.colored_label(ui.visuals().error_fg_color, &error);
                    }
                    crate::shell::foot(ui, |ui| {
                        close = ui.add(crate::shell::Btn::new(t0("dlg.cancel"))).clicked();
                        ok = ui.add(crate::shell::Btn::primary(t0("dlg.ok"))).clicked();
                    });
                });
                if ok {
                    let act = match rename {
                        Some(i) => format!("rename:{i}:{value}"),
                        None => format!("add:{value}"),
                    };
                    match self.bookmarks(&act) {
                        Ok(()) => close = true,
                        Err(e) => error = e,
                    }
                }
                if !close {
                    self.fdlg = Some(FDialog::BmText { rename, value, error });
                }
            }
            FDialog::Columns { mut cols, mut choices, mut error } => {
                let mut ok = false;
                crate::shell::Dialog::new(t0("col.title")).collapsible(false).show(ctx, |ui| {
                    ui.label(t0("col.text"));
                    let mut drop: Option<usize> = None;
                    egui::Grid::new("mc-cols").num_columns(4).show(ui, |ui| {
                        for (i, (c, ch)) in cols.iter_mut().zip(choices.iter_mut()).enumerate() {
                            ui.add(crate::shell::field(&mut c.name).hint_text(t0("col.name")).desired_width(150.0));
                            egui::ComboBox::from_id_salt(("mc-col-type", i)).selected_text(t0(&format!("col.type.{}", c.kind))).show_ui(
                                ui,
                                |ui| {
                                    for ty in cc_markup::columns::TYPES_OF_COLUMN {
                                        ui.selectable_value(&mut c.kind, ty.to_string(), t0(&format!("col.type.{ty}")));
                                    }
                                },
                            );
                            if c.kind == "choice" {
                                if ui.add(crate::shell::field(ch).hint_text(t0("col.choices")).desired_width(180.0)).changed() {
                                    c.choices = split_choices(ch);
                                }
                            } else {
                                ui.label("");
                            }
                            if ui.small_button("✕").on_hover_text(t("col.remove", &[("name", Fill::S(c.name.clone()))])).clicked() {
                                drop = Some(i);
                            }
                            ui.end_row();
                        }
                    });
                    if let Some(i) = drop {
                        cols.remove(i);
                        choices.remove(i);
                    }
                    if ui.add(crate::shell::Btn::new(format!("+ {}", t0("col.add")))).clicked() {
                        let mut all = self.store.columns.clone();
                        all.extend(cols.iter().cloned());
                        cols.push(Column { key: new_column_key(&all), name: String::new(), kind: "text".into(), choices: Vec::new() });
                        choices.push(String::new());
                    }
                    if !error.is_empty() {
                        ui.colored_label(ui.visuals().error_fg_color, &error);
                    }
                    crate::shell::foot(ui, |ui| {
                        close = ui.add(crate::shell::Btn::new(t0("dlg.cancel"))).clicked();
                        ok = ui.add(crate::shell::Btn::primary(t0("col.ok"))).clicked();
                    });
                });
                if ok {
                    match self.save_columns(cols.clone()) {
                        Ok(()) => close = true,
                        Err(e) => error = e,
                    }
                }
                if !close {
                    self.fdlg = Some(FDialog::Columns { cols, choices, error });
                }
            }
        }
        if close {
            self.fdlg = None;
        }
    }
}

/// A small icon button (the panels' rename / remove buttons): 20 px, the icon in the text colour, a well when hovered.
fn icon_button(ui: &mut egui::Ui, tok: &Tokens, icon: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, 2.0, tok.hover);
    }
    crate::ribbon::paint_icon(ui.painter(), r.shrink(3.0), icon, tok.text);
    resp
}

/// `autolink.row`: “3/A-501” → A-501.
fn autolink_row(r: &LinkRow) -> String {
    t("autolink.row", &[("ref", Fill::S(r.words.clone())), ("sheet", Fill::S(r.no.clone()))])
}

/// The choices field: split at commas, trimmed, the empty ones left out.
fn split_choices(s: &str) -> Vec<String> {
    s.split(',').map(|x| cc_markup::js::trim(x).to_string()).filter(|x| !x.is_empty()).collect()
}

/// Run `markup.find`, `markup.autolink`, `markup.bookmarks` and `markup.columns` (see pdfcmd.rs).
pub fn run(p: &mut PdfView, id: &str, name: &str) -> Result<(), String> {
    match id {
        "markup.find" => {
            let (k, v) = name.split_once('=').unwrap_or((name, ""));
            let on = |v: &str| matches!(v, "on" | "true" | "1");
            match k {
                "go" => {
                    p.find.q = v.to_string();
                    p.run_find();
                }
                "scope" => p.find.sheet_only = v == "sheet",
                "case" => p.find.match_case = on(v),
                "word" => p.find.whole_word = on(v),
                "show" => p.show_hit(v.parse().map_err(|_| "markup.find: show=I".to_string())?)?,
                "cloud" => p.cloud_found()?,
                _ => {
                    return Err(format!(
                        "markup.find: unknown action {k} (go=TEXT, scope=all|sheet, case=on|off, word=on|off, show=I, cloud)"
                    ));
                }
            }
            Ok(())
        }
        "markup.autolink" => match name {
            "" => {
                p.auto_link();
                Ok(())
            }
            "cancel" => {
                p.fdlg = None;
                Ok(())
            }
            "ok" => p.auto_link_ok(None),
            n => {
                let list = n.strip_prefix("ok:").ok_or_else(|| format!("markup.autolink: unknown action {n} (ok, ok:I,J…, cancel)"))?;
                let pick: Vec<usize> = list.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                p.auto_link_ok(Some(&pick))
            }
        },
        "markup.bookmarks" => p.bookmarks(name),
        "markup.columns" => {
            if name.is_empty() {
                p.edit_columns();
                return Ok(());
            }
            let v: Value = serde_json::from_str(name).map_err(|e| format!("markup.columns: {e}"))?;
            p.set_columns(&v)
        }
        _ => Err(format!("unknown command {id}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bm(title: &str, page: f64, children: Vec<Bookmark>) -> Bookmark {
        Bookmark { title: title.into(), page, children }
    }

    #[test]
    fn bookmark_rows_walk_in_order_and_slots_find_them() {
        let mut items = vec![bm("A", 0.0, vec![bm("A1", 1.0, vec![]), bm("A2", 2.0, vec![])]), bm("B", 3.0, vec![])];
        let rows = bm_rows(&items);
        let titles: Vec<&str> = rows.iter().map(|r| r.2.as_str()).collect();
        assert_eq!(titles, ["A", "A1", "A2", "B"]);
        assert_eq!(bm_count(&items), 4);
        let (list, j) = bm_slot(&mut items, &[0, 1]).unwrap();
        list.remove(j);
        assert_eq!(bm_count(&items), 3);
        assert!(bm_slot(&mut items, &[5]).is_none());
    }

    #[test]
    fn choices_split_at_commas() {
        assert_eq!(split_choices(" Elec, ,Mech ,"), ["Elec", "Mech"]);
    }
}
