//! A PDF plan set in the app (the sheet view MarkupCraft and TakeoffCraft are built on): pages listed by their labels,
//! the page rendered by hayro (cc-io-takeoff `render_rgba`, the renderer auto count reads) into one texture, pan and
//! zoom about the pointer. The texture is re-rendered when the zoom moves to another half-octave step, capped at
//! `MAX_SIDE` pixels a side; between renders the last one is drawn scaled.

use cc_io_takeoff::pdfread::{self, Doc, PageInfo};
use egui::{Color32, Pos2, Rect, Sense, Vec2};

use cc_markup::i18n::t0;

use crate::theme::Tokens;

/// The largest side of a page texture (GPU limits allow more; memory and render time say this).
const MAX_SIDE: f64 = 8192.0;
/// Zoom limits (screen pixels per PDF point).
const ZOOM_MIN: f64 = 0.02;
const ZOOM_MAX: f64 = 64.0;

/// One PDF of the project: what MarkupCraft's openPdf and the renderer read from it.
pub struct PdfFile {
    /// MarkupCraft's file id ("f1", "f2", …): its sheets are "<id>:<page index>"
    pub id: String,
    pub name: String,
    pub(crate) doc: Doc,
    pub pages: Vec<PageInfo>,
    /// the file as opened (Save writes a marked-up copy of it)
    pub(crate) bytes: std::sync::Arc<Vec<u8>>,
    /// each page's key ("f1:<index>") markups name their sheet by
    pub(crate) page_keys: Vec<String>,
    /// pages whose annotations MarkupCraft draws itself (annotMode "disable"): rendered without them while markups show
    pub(crate) annots_drawn: Vec<bool>,
    /// each page's view box and rotation as MarkupCraft read them (badge size, the text frame of new markups)
    pub(crate) page_views: Vec<([f64; 4], f64)>,
    /// annotations of the file that MarkupCraft rewrites on save (`f.removeRefs`)
    pub(crate) remove_refs: indexmap::IndexSet<String>,
    /// each page as MarkupCraft read it (its label and title block: the sheet's number and title, `sheetInfo`)
    pub(crate) recs: Vec<cc_io_markup::read::PageRec>,
    /// a blank set's paper and grid (blank.js), so an added blank page matches
    pub(crate) blank: Option<serde_json::Value>,
}

impl PdfFile {
    /// Read a PDF as file `id` into the project: its markups, scales, sheet names, layers and columns join `store`
    /// (`store.load`), with the store's id sequence.
    pub(crate) fn read(id: &str, name: &str, bytes: &[u8], store: &mut cc_markup::model::Store) -> Result<PdfFile, String> {
        let doc = pdfread::open(bytes)?;
        let pages = pdfread::pages(&doc);
        if pages.is_empty() {
            return Err("the PDF has no pages".into());
        }
        // MarkupCraft's reading of the same file (a file it cannot read still shows as a plan set)
        let opts = cc_io_markup::read::OpenOpts { file_id: id.into(), name: name.to_string(), read_text: false };
        let (page_keys, annots_drawn, page_views, remove_refs, recs, blank) =
            match cc_io_markup::read::open_pdf(&mut store.env, bytes, &opts) {
                Ok(r) => {
                    let names = r
                        .sheet_names
                        .iter()
                        .map(|(k, (n, t))| (k.clone(), cc_markup::model::SheetName { number: n.clone(), title: t.clone() }))
                        .collect();
                    store.load(r.markups.clone(), r.calibrations.clone(), names, Some(&r.layers), Some(&r.columns));
                    (
                        r.pages.iter().map(|p| p.key.clone()).collect(),
                        r.pages.iter().map(|p| p.annot_mode == "disable").collect(),
                        r.pages.iter().map(|p| (p.view, p.rotate)).collect(),
                        r.remove_refs.clone(),
                        r.pages.clone(),
                        r.blank.clone(),
                    )
                }
                Err(_) => (Vec::new(), Vec::new(), Vec::new(), indexmap::IndexSet::new(), Vec::new(), None),
            };
        Ok(PdfFile {
            id: id.to_string(),
            name: name.to_string(),
            doc,
            pages,
            bytes: std::sync::Arc::new(bytes.to_vec()),
            page_keys,
            annots_drawn,
            page_views,
            remove_refs,
            recs,
            blank,
        })
    }

    /// markupcraft.js `applySheetNames` for page `i`: `sheetInfo` of its label and title block with the number and
    /// title typed by hand (undoable, in the store).
    pub fn info(&self, store: &cc_markup::model::Store, i: usize) -> cc_io_markup::read::SheetInfo {
        let rec = self.recs.get(i);
        let manual = store.sheet_names.get(&self.key_of(i)).map(|n| (n.number.clone(), n.title.clone()));
        cc_io_markup::read::sheet_info(rec.map(|r| r.info.label.as_str()), &self.name, i, rec.and_then(|r| r.tb.as_ref()), manual.as_ref())
    }

    /// Page `i`'s title (`s.page.title`).
    pub fn title(&self, store: &cc_markup::model::Store, i: usize) -> String {
        self.info(store, i).title
    }

    /// The file's SHA-256 (TakeoffCraft knows a sheet as "<hash>:<page>").
    pub fn hash(&self) -> &str {
        &self.doc.hash
    }

    pub(crate) fn key_of(&self, i: usize) -> String {
        self.page_keys.get(i).cloned().unwrap_or_else(|| format!("{}:{i}", self.id))
    }

    pub(crate) fn badge_of(&self, i: usize) -> f64 {
        self.page_views.get(i).map_or(9.0, |(v, _)| ((v[2] - v[0]).hypot(v[3] - v[1]) / 160.0).clamp(7.0, 18.0))
    }

    /// markupcraft.js `bake(f, lib, {rot})`: the marked-up copy of this file; `rot` (page index, degrees) turns the text
    /// of that page's markups to read upright on a page being rotated.
    pub(crate) fn bake(&self, store: &cc_markup::model::Store, now: f64, rot: Option<(usize, f64)>) -> Result<(String, Vec<u8>), String> {
        let sheet_name = |k: &str| store.sheet_names.get(k).map(|n| serde_json::json!({ "number": n.number, "title": n.title }));
        let pages = (0..self.pages.len())
            .map(|i| {
                let key = self.key_of(i);
                let cal = store.calibrations.get(&key).cloned();
                cc_io_markup::write::WritePage {
                    index: i,
                    markups: store
                        .on_sheet(&key)
                        .into_iter()
                        .map(|m| {
                            let mut m = m.clone();
                            if let Some((_, r)) = rot.filter(|(p, _)| *p == i && !m.locked) {
                                m.rot = r;
                            }
                            m
                        })
                        .collect(),
                    cal_cleared: cal.is_none(),
                    cal,
                    cal_json: None,
                    badge: self.badge_of(i),
                    sheet_name: sheet_name(&key),
                    legends: Vec::new(),
                }
            })
            .collect();
        let renamed = (0..self.pages.len()).any(|i| store.sheet_names.get(&self.key_of(i)).is_some_and(|n| !n.number.is_empty()));
        let labels = renamed.then(|| {
            (0..self.pages.len())
                .map(|i| {
                    let n = store.sheet_names.get(&self.key_of(i)).map(|n| n.number.clone()).filter(|n| !n.is_empty());
                    let l = self.pages.get(i).map(|p| p.label.clone()).filter(|l| !l.is_empty());
                    serde_json::Value::String(n.or(l).unwrap_or_else(|| (i + 1).to_string()))
                })
                .collect()
        });
        let layers = store
            .layer_names()
            .into_iter()
            .map(|n| {
                let st = store.layer_state(&n);
                cc_io_markup::write::LayerIn { name: serde_json::Value::String(n), hidden: st.hidden, locked: st.locked }
            })
            .collect();
        let opts = cc_io_markup::write::WriteOpts {
            pages,
            remove_refs: self.remove_refs.clone(),
            labels,
            layers,
            columns: Some(cc_markup::columns::columns_to_value(&store.columns)),
            now_ms: now,
            ..Default::default()
        };
        let (pdf, _report) = cc_io_markup::write::write_marked_up(&self.bytes, &opts)?;
        let stem = self.name.rsplit_once('.').map_or(self.name.as_str(), |(a, _)| a);
        Ok((format!("{stem}-marked.pdf"), pdf))
    }
}

/// One open PDF plan set (one or several files).
pub struct PdfView {
    /// the open file (its sheets are the ones shown); `others` are the rest of the project, in the order opened
    pub cur: PdfFile,
    pub others: Vec<PdfFile>,
    file_seq: usize,
    pub page: usize,
    /// screen pixels (points of the UI) per PDF point
    pub(crate) zoom: f64,
    /// the page's top-left corner, from the view's top-left
    pub(crate) pan: Vec2,
    pub(crate) fitted: bool,
    /// the texture, the page it shows and the scale it was rendered at
    pub(crate) tex: Option<(egui::TextureHandle, usize, f64)>,
    /// how long the last render took (ms)
    pub render_ms: f64,
    pub error: Option<String>,
    /// the plan set's markups (read back with MarkupCraft's openPdf; the tools add to it with undo)
    pub store: cc_markup::model::Store,
    /// MarkupCraft's drawing tools
    pub tools: crate::markuptools::MarkupTools,
    /// "markup" or "takeoff": which app's tools the sheet view has
    pub mode: &'static str,
    /// TakeoffCraft's takeoff of this plan set
    pub takeoff: crate::takeofftools::TakeoffMode,
    /// draw the markups over the sheet
    pub show_markups: bool,
    /// the last primary press (time, screen point): a second one close by is a double-click (`e.detail`)
    last_press: Option<(f64, [f64; 2])>,
    /// a fixed clock for new markups' ids and dates (`markup.clock`; scripts and parity runs), else the wall clock
    pub clock: Option<f64>,
    /// the Properties panel's text fields while typed in: key → (value when typing began, text)
    pub edits: std::collections::HashMap<String, (String, String)>,
    /// Compare sheets by changed lines, and the old sheet picked for it
    pub cmp: crate::markupcompare::Compare,
    pub cmp_pick: String,
    /// the clipboard (clip.js: this window's copy) and the run of "beside the copy" pastes on one sheet
    pub clip: Option<cc_markup::clip::Clip>,
    pub(crate) paste_run: (String, u32),
    /// where the pointer is over the sheet (PDF user space), None when it is elsewhere (Ctrl+V pastes there)
    pub pointer: Option<[f64; 2]>,
    /// the view's size (points) the last time it was drawn (None before that)
    pub(crate) view_size: Option<Vec2>,
    /// the right panel's tab: props, list, punch, chest, compare
    pub tab: &'static str,
    /// the Markups list's filters and order
    pub list: crate::markupedit::ListState,
    /// replies being typed, by markup id
    pub(crate) reply_drafts: std::collections::HashMap<String, String>,
    /// the dialog open over the sheet (sheet name, pages)
    pub dlg: Option<crate::markupedit::Dialog>,
    /// files an action made (exports, extracted pages), waiting for the host's save
    pub outbox: Vec<(String, Vec<u8>)>,
    /// a file the person is to pick for an action (`insert-pdf`, `picture`, `photo`, `chest-import`), handled by the host
    pub want_pick: Option<&'static str>,
    /// pictures on the screen (image markups, the stamp's picture): texture by a hash of the data URL (None = unreadable)
    pub(crate) pics: std::collections::HashMap<u64, Option<egui::TextureHandle>>,
}

impl std::fmt::Debug for PdfView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfView").field("name", &self.cur.name).field("pages", &self.cur.pages.len()).field("page", &self.page).finish()
    }
}

/// The render scale for a zoom: half-octave steps (so small zooms do not re-render), capped by `MAX_SIDE`.
pub fn render_scale(zoom_px: f64, page_w: f64, page_h: f64) -> f64 {
    let step = (zoom_px.max(1e-3).log2() * 2.0).ceil() / 2.0;
    let s = 2f64.powf(step);
    let cap = MAX_SIDE / page_w.max(page_h).max(1.0);
    s.min(cap).max(0.05)
}

impl PdfView {
    /// Open the bytes as a PDF (no scripts, no forms: the page tree and content only).
    pub fn open(name: &str, bytes: &[u8]) -> Result<PdfView, String> {
        let mut store = cc_markup::model::Store::new();
        let cur = PdfFile::read("f1", name, bytes, &mut store)?;
        let mut takeoff = crate::takeofftools::TakeoffMode::new(cur.hash());
        takeoff.add_file(cur.hash(), name, bytes.len(), cur.pages.len());
        Ok(PdfView {
            store,
            tools: crate::markuptools::MarkupTools::default(),
            mode: "markup",
            takeoff,
            cur,
            others: Vec::new(),
            file_seq: 1,
            show_markups: true,
            last_press: None,
            clock: None,
            edits: std::collections::HashMap::new(),
            cmp: crate::markupcompare::Compare::default(),
            cmp_pick: String::new(),
            clip: None,
            paste_run: (String::new(), 0),
            pointer: None,
            view_size: None,
            tab: "props",
            list: crate::markupedit::ListState::default(),
            reply_drafts: std::collections::HashMap::new(),
            dlg: None,
            outbox: Vec::new(),
            want_pick: None,
            pics: std::collections::HashMap::new(),
            page: 0,
            zoom: 1.0,
            pan: Vec2::ZERO,
            fitted: false,
            tex: None,
            render_ms: 0.0,
            error: None,
        })
    }

    /// Another PDF joins the project (markupcraft.js / takeoffcraft.js Open with a plan set open): file "f<n>", its
    /// markups into the same store, its sheets into TakeoffCraft's file list; it becomes the open file.
    pub fn add_file(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        self.tick();
        let id = format!("f{}", self.file_seq.saturating_add(1));
        let f = PdfFile::read(&id, name, bytes, &mut self.store)?;
        self.file_seq = self.file_seq.saturating_add(1);
        self.takeoff.add_file(f.hash(), name, bytes.len(), f.pages.len());
        self.others.push(f);
        self.switch(self.others.len() - 1);
        self.go(0);
        Ok(())
    }

    /// Make one of the other files the open one (its first sheet).
    pub fn switch(&mut self, j: usize) {
        if let Some(o) = self.others.get_mut(j) {
            std::mem::swap(&mut self.cur, o);
            // keep the others in the order they were opened
            self.others.sort_by_key(|f| f.id.trim_start_matches('f').parse::<usize>().unwrap_or(0));
            self.page = 0;
            self.fitted = false;
            self.tex = None;
            self.takeoff.set_file(self.cur.hash());
            self.tools.cancel();
        }
    }

    /// The project's files in the order they were opened (the open one among them).
    pub fn files(&self) -> Vec<&PdfFile> {
        let mut v: Vec<&PdfFile> = std::iter::once(&self.cur).chain(self.others.iter()).collect();
        v.sort_by_key(|f| f.id.trim_start_matches('f').parse::<usize>().unwrap_or(0));
        v
    }

    /// Open file `id`'s page `i` (0-based).
    pub fn go_file(&mut self, id: &str, i: usize) {
        if self.cur.id != id
            && let Some(j) = self.others.iter().position(|f| f.id == id)
        {
            self.switch(j);
        }
        self.page = usize::MAX;
        if i < self.cur.pages.len() {
            self.page = i;
        } else {
            self.page = 0;
        }
        self.fitted = false;
    }

    /// The open page's rotation as MarkupCraft sees it (`view.seenRotate`).
    pub fn page_rot(&self) -> f64 {
        self.cur.page_views.get(self.page).map_or(0.0, |v| v.1)
    }

    /// `badgeFor(key)`: the count badge size of a page.
    pub fn badge_of(&self, i: usize) -> f64 {
        self.cur.badge_of(i)
    }

    /// A PDF point → the view's screen pixels (from the page's top-left; what "moved" and window direction use).
    pub fn screen_of(&self, p: [f64; 2]) -> [f64; 2] {
        let tr = self.cur.pages.get(self.page).map_or([1.0, 0.0, 0.0, -1.0, 0.0, 0.0], |i| i.transform);
        let (x, y) = pdfread::to_viewport(&tr, p[0], p[1]);
        [x * self.zoom, y * self.zoom]
    }

    /// Screen pixels (from the page's top-left) → PDF user space.
    pub fn user_of(&self, s: [f64; 2]) -> [f64; 2] {
        let tr = self.cur.pages.get(self.page).map_or([1.0, 0.0, 0.0, -1.0, 0.0, 0.0], |i| i.transform);
        let (x, y) = pdfread::to_pdf(&tr, s[0] / self.zoom, s[1] / self.zoom);
        [x, y]
    }

    /// Set the zoom exactly (screen pixels per PDF point; scripts and parity runs).
    pub fn set_zoom(&mut self, z: f64) {
        self.zoom = z.clamp(ZOOM_MIN, ZOOM_MAX);
        self.fitted = true;
        self.tools.px = 1.0 / self.zoom;
        self.takeoff.px = 1.0 / self.zoom;
    }

    /// The clock new markups are stamped with.
    pub(crate) fn tick(&mut self) {
        let now = self
            .clock
            .unwrap_or_else(|| web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64));
        self.store.env.now_ms = now;
    }

    /// A press with the markup tool at `q` (screen `s`).
    pub fn gesture_down(&mut self, q: [f64; 2], s: [f64; 2], shift: bool, dbl: bool) {
        self.tick();
        let key = self.sheet_key();
        let badge = self.badge_of(self.page);
        self.tools.rot = self.page_rot();
        self.tools.down(&mut self.store, &key, badge, q, s, shift, dbl);
        if dbl {
            self.tools.dblclick(&mut self.store, &key);
        }
    }

    /// The release.
    pub fn gesture_up(&mut self, q: [f64; 2], s: [f64; 2], shift: bool) {
        self.tick();
        let key = self.sheet_key();
        let badge = self.badge_of(self.page);
        let me = &*self;
        let (zoom, tr) = (me.zoom, me.cur.pages.get(me.page).map_or([1.0, 0.0, 0.0, -1.0, 0.0, 0.0], |i| i.transform));
        let to_user = move |s: [f64; 2]| {
            let (x, y) = pdfread::to_pdf(&tr, s[0] / zoom, s[1] / zoom);
            [x, y]
        };
        self.tools.up(&mut self.store, &key, badge, q, s, shift, &to_user);
    }

    fn size(&self) -> (f64, f64) {
        self.cur.pages.get(self.page).map_or((612.0, 792.0), |p| (p.width, p.height))
    }

    /// Go to a page (0-based) and fit it.
    pub fn go(&mut self, i: usize) {
        if i < self.cur.pages.len() && i != self.page {
            self.page = i;
            self.fitted = false;
        }
    }

    /// Fit the page in the view on the next frame.
    pub fn fit(&mut self) {
        self.fitted = false;
    }

    fn fit_to(&mut self, view: Vec2) {
        let (w, h) = self.size();
        let z = (f64::from(view.x) / w).min(f64::from(view.y) / h) * 0.95;
        self.zoom = z.clamp(ZOOM_MIN, ZOOM_MAX);
        let (sw, sh) = (w * self.zoom, h * self.zoom);
        self.pan = Vec2::new(((f64::from(view.x) - sw) / 2.0) as f32, ((f64::from(view.y) - sh) / 2.0) as f32);
        self.fitted = true;
    }

    /// Render the page at `scale` into the texture.
    fn render(&mut self, ctx: &egui::Context, scale: f64) {
        let t0 = web_time::Instant::now();
        let own = self.show_markups && self.cur.annots_drawn.get(self.page).copied().unwrap_or(false);
        match pdfread::render_rgba(&self.cur.doc, self.page, scale, !own) {
            Ok((rgba, w, h)) => {
                let img = egui::ColorImage::from_rgba_premultiplied([w, h], &rgba);
                let opts = egui::TextureOptions::LINEAR;
                match &mut self.tex {
                    Some((t, _, _)) => t.set(img, opts),
                    None => self.tex = Some((ctx.load_texture("pdf-page", img, opts), self.page, scale)),
                }
                if let Some(t) = &mut self.tex {
                    t.1 = self.page;
                    t.2 = scale;
                }
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        self.render_ms = t0.elapsed().as_secs_f64() * 1000.0;
    }

    /// The sheet list (page labels).
    pub fn sheets(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        if !self.store.markups.is_empty() {
            let label = format!("Markups ({} on this sheet, {} in all)", self.page_markups(), self.store.markups.len());
            if ui.checkbox(&mut self.show_markups, egui::RichText::new(label).color(t.text)).changed() {
                // the page is drawn with or without its annotations' own appearances
                self.tex = None;
            }
            ui.separator();
        }
        let mut go_to: Option<(String, usize)> = None;
        let several = !self.others.is_empty();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for f in self.files() {
                if several {
                    ui.label(egui::RichText::new(&f.name).small().strong().color(t.text_dim));
                }
                for (i, p) in f.pages.iter().enumerate() {
                    let label = if p.label.is_empty() { format!("Page {}", i + 1) } else { p.label.clone() };
                    let on = f.id == self.cur.id && i == self.page;
                    if ui.selectable_label(on, egui::RichText::new(label).color(t.text)).clicked() {
                        go_to = Some((f.id.clone(), i));
                    }
                }
            }
        });
        if let Some((id, i)) = go_to {
            self.go_file(&id, i);
        }
    }

    /// The page in `rect`: drag pans (any button), the wheel zooms about the pointer, Page Up / Down turn pages.
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, t: &Tokens) {
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        ui.painter().rect_filled(rect, 0.0, t.canvas_bg);
        if !self.fitted {
            self.fit_to(rect.size());
        }
        let origin = rect.min + self.pan;
        self.view_size = Some(rect.size());
        self.pointer = resp.hover_pos().map(|p| self.to_pdf(origin, p));
        self.poll_autocount(ui.ctx());
        if self.mode == "takeoff" && self.autocount_busy() {
            if resp.dragged() {
                self.pan += resp.drag_delta();
            }
        } else if self.mode == "takeoff" && self.takeoff.ac_tool && self.takeoff.ac.is_none() {
            // Auto count: a primary drag boxes the symbol, release searches this sheet
            let key = self.takeoff.sheet(self.page);
            if resp.drag_started_by(egui::PointerButton::Primary)
                && let Some(p) = ui.input(|i| i.pointer.press_origin()).or_else(|| resp.interact_pointer_pos())
            {
                let q = self.to_pdf(origin, p);
                self.takeoff.ac_box = Some((q, q));
            } else if resp.dragged_by(egui::PointerButton::Primary)
                && let (Some(p), Some((a, _))) = (resp.interact_pointer_pos(), self.takeoff.ac_box)
            {
                self.takeoff.ac_box = Some((a, self.to_pdf(origin, p)));
            } else if resp.drag_stopped()
                && let Some((a, b)) = self.takeoff.ac_box.take()
            {
                self.run_autocount(&key, a, b, true);
            } else if resp.dragged() {
                self.pan += resp.drag_delta();
            }
        } else if self.mode == "takeoff" && self.takeoff.ac.is_some() {
            // the review: a click rejects or restores a match, Enter adds them, Esc discards
            if resp.dragged() {
                self.pan += resp.drag_delta();
            } else if resp.clicked()
                && let Some(p) = resp.interact_pointer_pos()
            {
                let q = self.to_pdf(origin, p);
                if let Some(ac) = self.takeoff.ac.as_mut() {
                    ac.toggle(q);
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.takeoff.accept_autocount();
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.takeoff.ac = None;
            }
        } else if self.mode == "takeoff" {
            let key = self.takeoff.sheet(self.page);
            if resp.dragged() {
                self.pan += resp.drag_delta();
            } else if resp.double_clicked() {
                self.takeoff.finish(&key);
            } else if resp.clicked()
                && let Some(p) = resp.interact_pointer_pos()
            {
                let q = self.to_pdf(origin, p);
                self.takeoff.click(&key, q);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.takeoff.finish(&key);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.takeoff.pts.clear();
                self.takeoff.ded.clear();
            }
            if ui.input(|i| i.key_pressed(egui::Key::E)) && !ui.ctx().egui_wants_keyboard_input() {
                let d = !self.takeoff.deduct;
                self.takeoff.toggle_deduct(d);
            }
        }
        let pan_drag = self.mode != "takeoff"
            && (resp.dragged_by(egui::PointerButton::Middle)
                || resp.dragged_by(egui::PointerButton::Secondary)
                || (resp.dragged_by(egui::PointerButton::Primary) && self.tools.tool == "pan"));
        self.tools.px = 1.0 / self.zoom;
        self.tools.rot = self.page_rot();
        self.takeoff.px = 1.0 / self.zoom;
        if self.mode == "takeoff" {
        } else if pan_drag {
            self.pan += resp.drag_delta();
        } else if self.tools.tool != "pan" {
            // the tools take raw pointer events, as markupcraft.js's pointerdown / move / up / dblclick
            let (pressed, released, pos, shift, time) = ui
                .input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.latest_pos(), i.modifiers.shift, i.time));
            if let Some(pos) = pos {
                let s = [f64::from(pos.x - origin.x), f64::from(pos.y - origin.y)];
                let q = self.to_pdf(origin, pos);
                if pressed && resp.hovered() {
                    let dbl = self.last_press.is_some_and(|(t0, s0)| time - t0 < 0.4 && (s0[0] - s[0]).hypot(s0[1] - s[1]) < 5.0);
                    self.last_press = Some((time, s));
                    self.gesture_down(q, s, shift, dbl);
                } else if released {
                    self.gesture_up(q, s, shift);
                } else if resp.hovered() || self.tools.busy() {
                    self.tools.mv(&self.store, q, s, shift);
                }
            }
        }
        if self.mode != "takeoff" && !ui.ctx().egui_wants_keyboard_input() {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.tools.cancel();
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let key = self.sheet_key();
                self.tools.enter(&mut self.store, &key);
            }
        }
        let typing = ui.ctx().egui_wants_keyboard_input();
        if !typing && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
            if self.mode == "takeoff" {
                let _ = self.takeoff.mark_field("delete", "");
            } else {
                self.tools.delete_selected(&mut self.store);
            }
        }
        if !typing && ui.input(|i| i.modifiers.command && (i.key_pressed(egui::Key::Z) || i.key_pressed(egui::Key::Y))) {
            let redo = ui.input(|i| i.modifiers.shift || i.key_pressed(egui::Key::Y));
            if self.mode == "takeoff" {
                let r = if redo { self.takeoff.tk.redo() } else { self.takeoff.tk.undo() };
                self.takeoff.message = r.unwrap_or_default();
            } else {
                self.history(redo);
            }
        }
        if !typing && self.mode != "takeoff" {
            // Ctrl+C / X / V (egui hands them over as events), Ctrl+Shift+V pastes in place, Ctrl+D duplicates, Ctrl+A
            let (copy, cut, paste, shift, dup, all) = ui.input(|i| {
                let ev = |f: &dyn Fn(&egui::Event) -> bool| i.events.iter().any(f);
                (
                    ev(&|e| matches!(e, egui::Event::Copy)),
                    ev(&|e| matches!(e, egui::Event::Cut)),
                    ev(&|e| matches!(e, egui::Event::Paste(_))),
                    i.modifiers.shift,
                    i.modifiers.command && i.key_pressed(egui::Key::D),
                    i.modifiers.command && i.key_pressed(egui::Key::A),
                )
            });
            if copy && !self.tools.sel.is_empty() {
                self.copy("copy");
            }
            if cut && !self.tools.sel.is_empty() {
                self.cut();
            }
            if paste {
                let at = self.pointer;
                if shift {
                    self.paste("place", None);
                } else {
                    self.paste(if at.is_some() { "pointer" } else { "offset" }, at);
                }
            }
            if dup {
                self.duplicate();
            }
            if all {
                self.select_all();
            }
        }
        if let Some(p) = resp.hover_pos() {
            let (scroll, zoom_delta) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let f = f64::from(zoom_delta) * (f64::from(scroll) / 400.0).exp();
            if (f - 1.0).abs() > 1e-6 {
                let nz = (self.zoom * f).clamp(ZOOM_MIN, ZOOM_MAX);
                let k = (nz / self.zoom) as f32;
                let at = p - rect.min;
                self.pan = at - (at - self.pan) * k;
                self.zoom = nz;
            }
        }
        let (prev, next) = ui.input(|i| (i.key_pressed(egui::Key::PageUp), i.key_pressed(egui::Key::PageDown)));
        if prev && self.page > 0 {
            self.go(self.page - 1);
        }
        if next {
            self.go(self.page + 1);
        }
        let (w, h) = self.size();
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let want = render_scale(self.zoom * ppp, w, h);
        let stale = self.tex.as_ref().is_none_or(|(_, pg, s)| *pg != self.page || (*s - want).abs() > 1e-9);
        if stale {
            self.render(ui.ctx(), want);
        }
        let page_rect = Rect::from_min_size(rect.min + self.pan, Vec2::new((w * self.zoom) as f32, (h * self.zoom) as f32));
        let painter = ui.painter_at(rect);
        painter.rect_filled(page_rect.expand(1.0), 0.0, Color32::from_gray(90));
        if let Some((tex, _, _)) = &self.tex {
            painter.image(tex.id(), page_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        if self.show_markups || crate::markuptools::draws(&self.tools.tool) {
            // picture stamps: their textures first (decoded once, under limits)
            let key = self.sheet_key();
            let srcs: Vec<String> = self.store.on_sheet(&key).iter().filter(|m| m.kind == "image").map(|m| m.img.clone()).collect();
            for s in srcs {
                self.picture_texture(ui.ctx(), &s);
            }
            self.draw_markups(&painter, page_rect.min);
        }
        if self.mode != "takeoff"
            && let Some(info) = self.cur.pages.get(self.page)
        {
            let (tr, z, o) = (info.transform, self.zoom, page_rect.min);
            let at = move |x: f64, y: f64| {
                let (vx, vy) = pdfread::to_viewport(&tr, x, y);
                o + Vec2::new((vx * z) as f32, (vy * z) as f32)
            };
            self.draw_compare(&painter, &at, ui.visuals().error_fg_color, t.accent);
        }
        if self.mode != "takeoff" {
            if let Some((a, b)) = self.tools.window() {
                let o = page_rect.min;
                let r = Rect::from_two_pos(o + Vec2::new(a[0] as f32, a[1] as f32), o + Vec2::new(b[0] as f32, b[1] as f32));
                let crossing = b[0] < a[0];
                let c = if crossing { t.accent.gamma_multiply(0.6) } else { t.accent };
                painter.rect_filled(r, 0.0, c.gamma_multiply(0.12));
                painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, c), egui::StrokeKind::Inside);
            }
            self.editor(ui, page_rect.min);
        }
        if let Some(info) = self.cur.pages.get(self.page) {
            let (tr, z, o) = (info.transform, self.zoom, page_rect.min);
            let at = move |x: f64, y: f64| {
                let (vx, vy) = pdfread::to_viewport(&tr, x, y);
                o + Vec2::new((vx * z) as f32, (vy * z) as f32)
            };
            self.takeoff.draw(&painter, &self.takeoff.sheet(self.page), &at);
        }
        if let Some(e) = &self.error {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, e, egui::FontId::proportional(14.0), t.text);
        }
    }

    /// The text editor over a new or edited text, callout or note (markupcraft.js's textarea): typing grows the box,
    /// Ctrl+Enter or a click on the sheet commits, Esc drops it.
    fn editor(&mut self, ui: &mut egui::Ui, origin: Pos2) {
        let Some(ed) = self.tools.editing.as_ref() else { return };
        let (Some(info), z) = (self.cur.pages.get(self.page), self.zoom) else { return };
        let tr = info.transform;
        let at = |x: f64, y: f64| {
            let (vx, vy) = pdfread::to_viewport(&tr, x, y);
            origin + Vec2::new((vx * z) as f32, (vy * z) as f32)
        };
        let (pos, size) = if ed.m.kind == "note" {
            let p = ed.m.pts().first().copied().unwrap_or([0.0, 0.0]);
            (at(p[0], p[1]) + Vec2::new(26.0, 0.0), Vec2::new(260.0, 110.0))
        } else {
            let r = ed.m.rect.unwrap_or([0.0, 0.0, 100.0, 20.0]);
            let rr = Rect::from_two_pos(at(r[0], r[1]), at(r[2], r[3]));
            (rr.min, Vec2::new(rr.width().max(120.0), rr.height().max(24.0)))
        };
        let mut text = ed.value.clone();
        let mut commit = false;
        let mut cancel = false;
        egui::Area::new(ui.make_persistent_id("mc-editor")).fixed_pos(pos).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            let r = ui.add_sized(
                size,
                egui::TextEdit::multiline(&mut text).hint_text(t0(if ed.m.kind == "note" { "edit.note" } else { "edit.text" })),
            );
            r.request_focus();
            commit = ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
            cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
        });
        if text != ed.value {
            self.tools.edit_input(&text);
        }
        let key = self.sheet_key();
        if cancel {
            self.tools.cancel_edit();
        } else if commit {
            self.tick();
            self.tools.commit_edit(&mut self.store, &key);
        }
    }

    /// The markups of this page, drawn from MarkupCraft's display lists (shapes.js: the same list the saved PDF's
    /// appearance streams are made from), PDF user space → the screen through the page's viewport transform.
    fn draw_markups(&self, painter: &egui::Painter, origin: Pos2) {
        use cc_markup::shapes::{Item, ShapeEnv, flatten, place_text, shapes};
        let (Some(key), Some(info)) = (self.cur.page_keys.get(self.page), self.cur.pages.get(self.page)) else { return };
        let tr = info.transform;
        let z = self.zoom;
        let at = |x: f64, y: f64| {
            let (vx, vy) = pdfread::to_viewport(&tr, x, y);
            origin + Vec2::new((vx * z) as f32, (vy * z) as f32)
        };
        // the sheet's scale: dimensions and areas read in real units once it is calibrated
        let env = ShapeEnv { cal: self.store.calibrations.get(key).cloned(), badge: self.badge_of(self.page), legend: None };
        let preview = self.tools.preview(&self.store, key, None);
        let hidden = self.tools.hidden();
        let list: Vec<&cc_markup::model::Markup> = self
            .store
            .on_sheet(key)
            .into_iter()
            .filter(|m| self.store.shown(m) && m.kind != "foreign" && !hidden.contains(&m.id))
            .chain(preview.iter())
            .collect();
        for m in list {
            if self.tools.sel.contains(&m.id)
                && let Some(b) = cc_markup::shapes::list_bounds(&shapes(m, &env))
            {
                let r = Rect::from_two_pos(at(b[0], b[1]), at(b[2], b[3])).expand(4.0);
                painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, Color32::from_rgb(0x1f, 0x6f, 0xeb)), egui::StrokeKind::Outside);
            }
            for item in shapes(m, &env) {
                match item {
                    Item::Path(p) => {
                        for sub in flatten(&p.d) {
                            let pts: Vec<Pos2> = sub.pts.iter().map(|q| at(q[0], q[1])).collect();
                            if pts.len() < 2 {
                                continue;
                            }
                            if sub.closed
                                && pts.len() >= 3
                                && let Some(f) = p.fill.as_deref().and_then(|c| hex_color(c, p.falpha))
                            {
                                painter.add(egui::Shape::convex_polygon(pts.clone(), f, egui::Stroke::NONE));
                            }
                            if let Some(c) = p.stroke.as_deref().and_then(|c| hex_color(c, p.alpha)) {
                                let mut line = pts;
                                if sub.closed
                                    && let Some(first) = line.first().copied()
                                {
                                    line.push(first);
                                }
                                let w = ((p.w * z) as f32).max(1.0);
                                painter.add(egui::Shape::line(line, egui::Stroke::new(w, c)));
                            }
                        }
                    }
                    Item::Text(t) => {
                        let Some(c) = hex_color(&t.color, t.alpha) else { continue };
                        let (lines, _angle) = place_text(&t);
                        let size = ((t.size * z) as f32).max(4.0);
                        for l in lines {
                            painter.text(at(l.x, l.y), egui::Align2::LEFT_BOTTOM, &l.s, egui::FontId::proportional(size), c);
                        }
                    }
                    Item::Image(im) => {
                        let (a, b) = (at(im.bbox[0], im.bbox[1]), at(im.bbox[2], im.bbox[3]));
                        let r = Rect::from_two_pos(a, b);
                        match self.pics.get(&pic_key(&im.src)).cloned().flatten() {
                            Some(tex) => {
                                let alpha = if im.alpha.is_finite() { im.alpha.clamp(0.0, 1.0) } else { 1.0 };
                                let tint = Color32::from_white_alpha((alpha * 255.0).round() as u8);
                                painter.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
                            }
                            None => {
                                painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, Color32::from_gray(140)), egui::StrokeKind::Inside);
                            }
                        }
                    }
                }
            }
        }
    }

    /// A picture's texture (a data URL decoded once, under cc-io-markup `picture`'s limits; None when unreadable).
    pub fn picture_texture(&mut self, ctx: &egui::Context, src: &str) -> Option<egui::TextureHandle> {
        let k = pic_key(src);
        if let Some(t) = self.pics.get(&k) {
            return t.clone();
        }
        if self.pics.len() > 256 {
            self.pics.clear();
        }
        let tex = cc_io_markup::picture::decode_rgba(src).and_then(|(w, h, rgba)| {
            let size = [usize::try_from(w).ok()?, usize::try_from(h).ok()?];
            (rgba.len() == size[0].checked_mul(size[1])?.checked_mul(4)?).then(|| {
                let img = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                ctx.load_texture(format!("mc-pic-{k}"), img, egui::TextureOptions::LINEAR)
            })
        });
        self.pics.insert(k, tex.clone());
        tex
    }

    /// How many markups the open page has.
    pub fn page_markups(&self) -> usize {
        self.cur.page_keys.get(self.page).map_or(0, |k| self.store.on_sheet(k).len())
    }

    /// TakeoffCraft's panel for the open sheet.
    pub fn takeoff_panel(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let key = self.takeoff.sheet(self.page);
        let uu = self.cur.pages.get(self.page).map_or(1.0, |p| p.user_unit);
        self.takeoff.panel(ui, t, &key, uu);
    }

    /// Auto count on the open sheet for the symbol boxed from `a` to `b` (PDF user space). The search runs here
    /// (scripts, the web build) or, with `background` on the desktop, on its own thread (as the JS uses a worker): the
    /// result is picked up by `poll_autocount`.
    pub fn run_autocount(&mut self, sheet: &str, a: [f64; 2], b: [f64; 2], background: bool) {
        if !self.takeoff.ensure_count() {
            return;
        }
        let Some(info) = self.cur.pages.get(self.page).cloned() else { return };
        let item = self.takeoff.item.clone().unwrap_or_default();
        let own = self.takeoff.own_counts(sheet);
        let any = self.takeoff.ac_any;
        #[cfg(not(target_arch = "wasm32"))]
        if background {
            // the thread opens its own copy of the PDF (the reader's document is not shared across threads)
            let bytes = std::sync::Arc::clone(&self.cur.bytes);
            let sheet = sheet.to_string();
            let (tx, rx) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new().name("autocount".into()).spawn(move || {
                let r = pdfread::open(&bytes).and_then(|doc| crate::autocount::run(&doc, &info, &item, &sheet, (a, b), any, &own));
                let _ = tx.send(r);
            });
            match spawned {
                Ok(_) => {
                    self.takeoff.ac_job = Some(rx);
                    self.takeoff.message = cc_takeoff::i18n::t("ac.searching.one");
                }
                Err(e) => self.takeoff.message = e.to_string(),
            }
            return;
        }
        let _ = background;
        let r = crate::autocount::run(&self.cur.doc, &info, &item, sheet, (a, b), any, &own);
        self.finish_autocount(r);
    }

    fn finish_autocount(&mut self, r: Result<crate::autocount::AutoCount, String>) {
        match r {
            Ok(ac) => {
                self.takeoff.message = if ac.visible().is_empty() {
                    cc_takeoff::i18n::t("ac.none")
                } else {
                    format!("{} ({:.0} ms)", cc_takeoff::i18n::t("ac.done"), ac.ms)
                };
                self.takeoff.ac = Some(ac);
            }
            Err(e) => self.takeoff.message = e,
        }
    }

    /// A background search that has finished becomes the review; while one runs the view keeps repainting.
    pub fn poll_autocount(&mut self, ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(rx) = self.takeoff.ac_job.as_ref() {
            match rx.try_recv() {
                Ok(r) => {
                    self.takeoff.ac_job = None;
                    self.finish_autocount(r);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(50)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.takeoff.ac_job = None;
                    self.takeoff.message = cc_takeoff::i18n::tf("ac.failed", &[("reason", cc_takeoff::i18n::F::S("stopped".into()))]);
                }
            }
        }
        let _ = ctx;
    }

    /// Whether a background search is running.
    pub fn autocount_busy(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return self.takeoff.ac_job.is_some();
        #[cfg(target_arch = "wasm32")]
        false
    }

    /// A TakeoffCraft export of this plan set (see `TakeoffMode::export_file`).
    pub fn takeoff_export(&self, kind: &str, now_ms: f64) -> Result<(String, Vec<u8>), String> {
        self.takeoff.export_file(kind, self.cur.hash(), &self.cur.bytes, &self.cur.name, now_ms)
    }

    /// TakeoffCraft's exports as the JS writes them: the marked PDF once per file that has marks (exportPdf), the
    /// others once.
    pub fn takeoff_exports(&self, kind: &str, now_ms: f64) -> Result<Vec<(String, Vec<u8>)>, String> {
        if kind != "pdf" {
            return self.takeoff_export(kind, now_ms).map(|x| vec![x]);
        }
        let with: Vec<&PdfFile> = self.files().into_iter().filter(|f| self.takeoff.has_marks(f.hash())).collect();
        if with.is_empty() {
            return Err(cc_takeoff::i18n::t("export.pdf.none"));
        }
        with.iter().map(|f| self.takeoff.export_file("pdf", f.hash(), &f.bytes, &f.name, now_ms)).collect()
    }

    /// The open sheet's key ("f1:<index>"): what its markups name.
    pub fn sheet_key(&self) -> String {
        self.cur.page_keys.get(self.page).cloned().unwrap_or_else(|| format!("f1:{}", self.page))
    }

    /// A screen point → PDF user space of the open page.
    pub(crate) fn to_pdf(&self, origin: Pos2, p: Pos2) -> [f64; 2] {
        let tr = self.cur.pages.get(self.page).map_or([1.0, 0.0, 0.0, -1.0, 0.0, 0.0], |i| i.transform);
        let (x, y) = pdfread::to_pdf(&tr, f64::from(p.x - origin.x) / self.zoom, f64::from(p.y - origin.y) / self.zoom);
        [x, y]
    }

    /// The marked-up copy, as markupcraft.js `bake`: every sheet's markups as annotations with its scale (a sheet
    /// without one clears the file's), its badge size and typed sheet name; typed sheet numbers become page labels;
    /// the file's own annotations MarkupCraft read are rewritten; layers and columns travel. "<name>-marked.pdf".
    pub fn save_marked(&mut self, now_ms: f64) -> Result<(String, Vec<u8>), String> {
        let now = self.clock.unwrap_or(now_ms);
        self.cur.bake(&self.store, now, None)
    }

    /// Save as markupcraft.js saveFiles: one file → it; several → the files with changes (or the open one when none
    /// has): a marked-up copy of each. Saved sheets are no longer dirty.
    pub fn save_files(&mut self, now_ms: f64) -> Result<Vec<(String, Vec<u8>)>, String> {
        let now = self.clock.unwrap_or(now_ms);
        let files = self.files();
        let dirty = |f: &PdfFile| self.store.dirty_sheets.iter().any(|k| k.starts_with(&format!("{}:", f.id)));
        let mut targets: Vec<&PdfFile> = if files.len() == 1 { files } else { files.into_iter().filter(|f| dirty(f)).collect() };
        if targets.is_empty() {
            targets.push(&self.cur);
        }
        let out = targets.iter().map(|f| f.bake(&self.store, now, None)).collect::<Result<Vec<_>, String>>()?;
        let ids: Vec<String> = targets.iter().map(|f| format!("{}:", f.id)).collect();
        self.store.dirty_sheets.retain(|k| !ids.iter().any(|p| k.starts_with(p)));
        Ok(out)
    }

    /// The markup tool bar (above the sheet): tools, colour, the text a Text markup gets, undo / redo, delete.
    pub fn tool_bar(&mut self, ui: &mut egui::Ui, t: &Tokens) -> bool {
        let mut save = false;
        ui.horizontal_wrapped(|ui| {
            for (m, label) in [("markup", "Markup"), ("takeoff", "Takeoff")] {
                if ui.selectable_label(self.mode == m, egui::RichText::new(label).strong()).clicked() {
                    self.mode = m;
                    self.takeoff.pts.clear();
                }
            }
            ui.separator();
            if self.mode == "takeoff" {
                ui.label(egui::RichText::new("Pick an item on the right, then click the sheet: a count at each click; double-click or Enter ends a length or area; Esc drops it").color(t.text_dim));
                return;
            }
            let key = self.sheet_key();
            for id in crate::markuptools::TOOLS {
                if ui
                    .selectable_label(self.tools.tool == *id, crate::markuptools::tool_label(id))
                    .on_hover_text(t0(&format!("tool.{id}.title")))
                    .clicked()
                {
                    self.tools.set_tool(&mut self.store, &key, id);
                }
            }
            ui.separator();
            // quick colours: the Properties panel's colour for the selection, or for the tool's next markups
            for c in crate::markuptools::COLORS {
                let col = hex_color(c, 1.0).unwrap_or(Color32::RED);
                let (r, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                ui.painter().rect_filled(r, 2.0, col);
                if resp.on_hover_text(t0("props.color")).clicked() {
                    let _ = self.tools.style_edit(&mut self.store, "color", c);
                }
            }
            ui.separator();
            self.scale_row(ui);
            if ui.add_enabled(!self.store.undo_stack.is_empty(), egui::Button::new(t0("tool.undo"))).clicked() {
                self.history(false);
            }
            if ui.add_enabled(!self.store.redo_stack.is_empty(), egui::Button::new(t0("tool.redo"))).clicked() {
                self.history(true);
            }
            if ui.add_enabled(!self.tools.sel.is_empty(), egui::Button::new(t0("menu.copy"))).clicked() {
                self.copy("copy");
            }
            if ui.add_enabled(self.clip.is_some(), egui::Button::new(t0("menu.paste"))).clicked() {
                self.paste("offset", None);
            }
            ui.menu_button(t0("menu.more"), |ui| {
                let sel = !self.tools.sel.is_empty();
                if ui.add_enabled(sel, egui::Button::new(t0("menu.cut"))).clicked() {
                    self.cut();
                    ui.close();
                }
                if ui.add_enabled(self.clip.is_some(), egui::Button::new(t0("menu.paste_in_place"))).clicked() {
                    self.paste("place", None);
                    ui.close();
                }
                if ui.add_enabled(sel, egui::Button::new(t0("menu.duplicate"))).clicked() {
                    self.duplicate();
                    ui.close();
                }
                if ui.add_enabled(sel, egui::Button::new(t0("menu.delete"))).clicked() {
                    self.tools.delete_selected(&mut self.store);
                    ui.close();
                }
                if ui.button(t0("menu.select_all")).clicked() {
                    self.select_all();
                    ui.close();
                }
                if ui.add_enabled(sel, egui::Button::new(t0("menu.select_none"))).clicked() {
                    self.tools.sel.clear();
                    ui.close();
                }
            });
            self.pages_menu(ui);
            if ui.button(t0("tool.save")).on_hover_text(t0("tool.save.title")).clicked() {
                save = true;
            }
        });
        save
    }

    /// The sheet's scale: the calibrate line's real distance (once one is drawn), a preset, or remove it.
    fn scale_row(&mut self, ui: &mut egui::Ui) {
        use cc_markup::units::{preset_calibration, scale_presets, units};
        let key = self.sheet_key();
        let cal = self.store.calibrations.get(&key).cloned();
        if self.tools.cal_len.is_some() {
            let label = ui.label(t0("cal.real"));
            let field = ui
                .add(egui::TextEdit::singleline(&mut self.tools.cal_text).hint_text(t0("cal.placeholder")).desired_width(110.0))
                .labelled_by(label.id);
            let unit_name = units().into_iter().find(|(u, _)| *u == self.tools.cal_unit).map(|(_, n)| n).unwrap_or_default();
            egui::ComboBox::new("mc-cal-unit", t0("set.unit")).selected_text(unit_name).show_ui(ui, |ui| {
                for (u, name) in units() {
                    ui.selectable_value(&mut self.tools.cal_unit, u.to_string(), name);
                }
            });
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button(t0("cal.set")).clicked() || enter {
                self.tools.apply_calibration(&mut self.store, &key);
            }
        }
        let shown = cal.as_ref().map_or_else(|| t0("status.scale.none"), |c| c.label.clone());
        let mut pick: Option<cc_markup::units::Cal> = None;
        egui::ComboBox::new("mc-scale", t0("props.scale")).selected_text(shown).show_ui(ui, |ui| {
            for (group, list, unit) in scale_presets() {
                ui.label(egui::RichText::new(group).strong());
                for (label, ratio) in list {
                    let on = cal.as_ref().is_some_and(|c| c.label == label);
                    if ui.selectable_label(on, label).clicked() {
                        pick = Some(preset_calibration(label, ratio, unit));
                    }
                }
            }
        });
        if let Some(c) = pick {
            self.tools.set_scale(&mut self.store, &key, Some(c));
        }
        if cal.is_some() && ui.button(t0("cal.remove")).clicked() {
            self.tools.set_scale(&mut self.store, &key, None);
        }
    }

    /// What the status bar says.
    pub fn status(&self) -> String {
        let p = self.cur.pages.get(self.page);
        let label = p.map(|p| p.label.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| format!("{}", self.page + 1));
        format!(
            "{}: sheet {label} ({} of {}) · {:.0} × {:.0} pt · zoom {:.0}% · render {:.0} ms · drag pans, wheel zooms, Page Up / Down",
            self.cur.name,
            self.page + 1,
            self.cur.pages.len(),
            p.map_or(0.0, |p| p.width),
            p.map_or(0.0, |p| p.height),
            self.zoom * 100.0,
            self.render_ms
        )
    }
}

/// Timings of the sheet view's heavy paths (cadcraft-shot pdfbench; forks/tools/mkbench.mjs runs the JS the same way):
/// open, page 1 rendered at 1× and 2× (the file opened again each time, as pdf.js does), Save with 50 markups on page
/// 1, TakeoffCraft's marked PDF with 50 count marks. Best of `runs`, ms, as JSON.
pub fn bench(name: &str, bytes: &[u8], runs: usize) -> Result<String, String> {
    let best = |f: &mut dyn FnMut() -> Result<(), String>| -> Result<f64, String> {
        let mut b = f64::INFINITY;
        for _ in 0..runs.max(1) {
            let t0 = web_time::Instant::now();
            f()?;
            b = b.min(t0.elapsed().as_secs_f64() * 1000.0);
        }
        Ok(b)
    };
    let mut v: Option<PdfView> = None;
    let open = best(&mut || {
        v = Some(PdfView::open(name, bytes)?);
        Ok(())
    })?;
    let mut v = v.ok_or("pdfbench: no view")?;
    let mut render = [0.0; 2];
    for (i, sc) in [1.0, 2.0].into_iter().enumerate() {
        let ms = best(&mut || {
            let doc = pdfread::open(bytes)?;
            pdfread::render_rgba(&doc, 0, sc, true).map(|_| ())
        })?;
        if let Some(r) = render.get_mut(i) {
            *r = ms;
        }
    }
    let key = v.sheet_key();
    for i in 0..50u32 {
        let (cx, cy) = (f64::from(i % 10) * 60.0, f64::from(i / 10) * 60.0);
        let mut f = serde_json::Map::new();
        f.insert("id".into(), serde_json::json!(format!("b{i}")));
        f.insert("sheet".into(), serde_json::json!(key));
        f.insert("type".into(), serde_json::json!("rect"));
        f.insert("rect".into(), serde_json::json!([100.0 + cx, 100.0 + cy, 140.0 + cx, 140.0 + cy]));
        f.insert("created".into(), serde_json::json!("2026-10-18T00:00:00Z"));
        let m = cc_markup::model::make_markup(&mut v.store.env, &f);
        v.store.markups.insert(m.id.clone(), m);
    }
    let save = best(&mut || v.save_marked(0.0).map(|_| ()))?;
    let sheet = v.takeoff.sheet(0);
    v.takeoff.add_item("Outlets", "count");
    for i in 0..50u32 {
        v.takeoff.click(&sheet, [100.0 + f64::from(i) * 10.0, 200.0]);
    }
    let export = best(&mut || v.takeoff_export("pdf", 0.0).map(|_| ()))?;
    Ok(format!(
        "{{\"pages\":{},\"open\":{open:.3},\"render1\":{:.3},\"render2\":{:.3},\"save\":{save:.3},\"export\":{export:.3}}}",
        v.cur.pages.len(),
        render[0],
        render[1]
    ))
}

/// The key a picture's texture is kept under.
fn pic_key(src: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    h.finish()
}

/// "#rrggbb" (or "#rgb") with an alpha → a colour; None for anything else (a markup's colours are data from the file).
pub fn hex_color(s: &str, alpha: f64) -> Option<Color32> {
    // shapes.js names the sheet's own white "paper" (callout and note boxes)
    let h = if s == "paper" { "fff" } else { s.strip_prefix('#')? };
    let full: String = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect(),
        6 => h.to_string(),
        _ => return None,
    };
    let v = u32::from_str_radix(&full, 16).ok()?;
    let a = if alpha.is_finite() { alpha.clamp(0.0, 1.0) } else { 1.0 };
    Some(Color32::from_rgba_unmultiplied((v >> 16) as u8, (v >> 8) as u8, v as u8, (a * 255.0).round() as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_colours() {
        assert_eq!(hex_color("#e5322d", 1.0), Some(Color32::from_rgb(0xe5, 0x32, 0x2d)));
        assert_eq!(hex_color("#fff", 1.0), Some(Color32::WHITE));
        assert_eq!(hex_color("red", 1.0), None);
        assert_eq!(hex_color("#12345", 1.0), None);
    }

    #[test]
    fn several_pdfs_one_project_and_compare() {
        let blank =
            |grid: &str| cc_io_markup::pages::blank_pdf(Some(&serde_json::json!({ "paper": "letter", "grid": grid, "pages": 2 })), "", 0.0);
        let mut v = PdfView::open("a.pdf", &blank("square")).unwrap();
        v.set_zoom(1.0);
        v.add_file("b.pdf", &blank("none")).unwrap();
        assert_eq!(v.cur.id, "f2");
        assert_eq!(v.files().iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["f1", "f2"]);
        assert_eq!(v.sheet_key(), "f2:0");
        // TakeoffCraft knows both files and marks the open one's sheets
        assert_eq!(v.takeoff.tk.state["files"].as_array().map(Vec::len), Some(2));
        assert!(v.takeoff.sheet(0).starts_with(v.cur.hash()));
        // a rectangle on b, nothing on a: Save writes b only (the file with changes)
        v.tools.set_tool(&mut v.store, "f2:0", "rect");
        v.gesture_down([100.0, 100.0], [100.0, 100.0], false, false);
        v.tools.mv(&v.store, [200.0, 160.0], [200.0, 160.0], false);
        v.gesture_up([200.0, 160.0], [200.0, 160.0], false);
        let out = v.save_files(0.0).unwrap();
        assert_eq!(out.iter().map(|o| o.0.as_str()).collect::<Vec<_>>(), ["b-marked.pdf"]);
        // the grid sheet against the plain one: only removed lines, clouded in one step
        v.compare_start("f1:0").unwrap();
        let d = v.cmp.vec.clone().unwrap();
        assert!(!d.removed.is_empty() && d.added.is_empty());
        let n0 = v.store.on_sheet("f2:0").len();
        v.compare_cloud().unwrap();
        assert!(v.store.on_sheet("f2:0").len() > n0);
        assert!(v.compare_start("f2:0").is_err(), "a sheet against itself");
        v.go_file("f1", 1);
        assert_eq!(v.sheet_key(), "f1:1");
    }

    #[test]
    fn render_scale_steps_and_caps() {
        assert_eq!(render_scale(1.0, 612.0, 792.0), 1.0);
        assert_eq!(render_scale(1.1, 612.0, 792.0), 2f64.sqrt());
        // a huge zoom on an A0 sheet stops at the side cap
        let s = render_scale(40.0, 3370.0, 2384.0);
        assert!((3370.0 * s - MAX_SIDE).abs() < 1e-6, "{s}");
    }
}
