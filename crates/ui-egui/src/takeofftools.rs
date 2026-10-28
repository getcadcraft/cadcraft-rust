//! TakeoffCraft on the sheet view: a sheet scale (the 0.5 presets), items (count, linear, area), marks clicked on the
//! sheet, and the quantities and estimate cc-takeoff computes (model.js `quantities` / `estimate`, at parity). Every
//! edit goes through `Takeoff` (one undo step, the 0.5 labels and ids); marks are kept in PDF user space and the sheet
//! is known by its file's hash, as `sheetKey(hash, page)` in the JS.

use cc_takeoff::geom::P;
use cc_takeoff::i18n::{F, t, tf};
use cc_takeoff::model::{QOpts, Takeoff, quantities, sheet_key};
use egui::{Color32, Pos2, RichText};
use serde_json::{Map, Value, json};

use crate::theme::Tokens;

/// Points a linear or area mark keeps at most while it is clicked (the model caps the file; this caps the gesture).
const MAX_PTS: usize = 10_000;

/// The takeoff of one open plan set.
pub struct TakeoffMode {
    pub tk: Takeoff,
    hash: String,
    /// the item new marks belong to
    pub item: Option<String>,
    /// the mark being clicked (linear / area / cut-out / circle centre), PDF user space; a point's `b` is the bulge of
    /// the arc that starts at it
    pub pts: Vec<P>,
    /// arc mode (R) and the pending point on the curve of the arc being clicked
    pub arc: bool,
    pub mid: Option<P>,
    /// where the last click of the draft landed (a double-click's second press adds no point)
    pub last_click: Option<P>,
    /// the pointer on the sheet (PDF user space): the rubber band, the paste preview
    pub cursor: Option<[f64; 2]>,
    /// the other selected marks (Shift+click, Select all), in the order they joined; `sel` is the main one
    pub sel_more: Vec<String>,
    /// a corner (or an arc's middle) of the selected mark being dragged
    pub drag: Option<crate::takeoffedit::Drag>,
    /// this window's clipboard (the clip clipboard.js reads back from what it wrote) and the marks on the pointer
    /// waiting for a click
    pub clip: Option<Value>,
    pub paste: Option<(Value, &'static str)>,
    /// each sheet's view box and UserUnit (what a copy records and a paste in place checks)
    pub page_info: std::collections::HashMap<String, ([f64; 4], f64)>,
    /// the selected markup (note, cloud, highlight), the first corner of a cloud / highlight, a markup waiting for its
    /// text (the ask dialog: kind, sheet, points, the text typed) and the "open only" filter of the Markups list
    pub sel_note: Option<String>,
    pub note_start: Option<P>,
    pub note_ask: Option<crate::takeoffnotes::NoteAsk>,
    pub notes_open_only: bool,
    /// the New typical group window: the marks, the name and repeat typed
    pub group_ask: Option<crate::takeoffnotes::GroupAsk>,
    /// the revision overlay (Compare): the older sheet over this one, its alignment and the changes found
    pub cmp: Option<crate::takeoffrev::Cmp>,
    /// the sheet on screen (the view sets it) and a sheet the view should go to (a markup or a group elsewhere)
    pub cur_sheet: String,
    pub goto: Option<String>,
    /// the 3D button was pressed (the view opens it: it owns the sheets)
    pub want_3d: bool,
    pub message: String,
    /// the name and type the "Add item" row uses
    new_name: String,
    new_type: String,
    /// an export the panel asked for ("xlsx", "csv", "pdf", "project"): the host writes it (it owns saving)
    pub export: Option<&'static str>,
    /// Auto count: the tool is on (a drag boxes a symbol), search at any angle, the box being dragged, the matches
    pub ac_tool: bool,
    pub ac_any: bool,
    pub ac_box: Option<([f64; 2], [f64; 2])>,
    pub ac: Option<crate::autocount::AutoCount>,
    /// a search running on its own thread (desktop): its result arrives here
    #[cfg(not(target_arch = "wasm32"))]
    pub ac_job: Option<std::sync::mpsc::Receiver<Result<crate::autocount::AutoCount, String>>>,
    /// the tool: "draw" (marks of the current item), "select" (pick a mark), "cutout" (a hole in the selected area)
    pub tool: &'static str,
    /// the selected mark
    pub sel: Option<String>,
    /// deduct mode (T2) and the segments of the run being clicked that are deducts
    pub deduct: bool,
    pub ded: std::collections::BTreeSet<usize>,
    /// the bid area new marks go in
    pub cur_area: Option<String>,
    /// PDF units per screen pixel (set by the view each frame): pick and close distances, as the JS's `1 / view.z`
    pub px: f64,
    /// text fields being typed in: key → (the model's value when typing began, the text)
    pub edits: std::collections::HashMap<String, (String, String)>,
    /// the New bid area window (its name field) and the Manage bid areas window
    pub area_dialog: Option<String>,
    pub manage_areas: bool,
    pub new_area: String,
}

impl std::fmt::Debug for TakeoffMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TakeoffMode").field("items", &self.tk.items().len()).field("marks", &self.tk.marks().len()).finish()
    }
}

/// A point as the model keeps it: `{x, y}`, with `b` when the segment starting at it is an arc.
pub(crate) fn pt_json(p: &P) -> Value {
    if p.has_b() { json!({ "x": p.x, "y": p.y, "b": p.b }) } else { json!({ "x": p.x, "y": p.y }) }
}

/// `ftIn(ft)` of takeoffcraft.js (sixteenths).
pub(crate) fn ft_in(ft: f64) -> String {
    cc_takeoff::units::format_ft_in(ft, 16.0)
}

fn obj(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(o) => o,
        _ => Map::new(),
    }
}

/// A number as the panel shows it (two decimals, no trailing zeros).
fn qty(x: f64) -> String {
    if !x.is_finite() {
        return "—".into();
    }
    let s = format!("{x:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// ui-util.js `safeName`: path and control characters become spaces, runs of space one, at most 120 characters.
pub(crate) fn safe_name(s: &str) -> String {
    let mapped: String =
        s.chars().map(|c| if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c < ' ' { ' ' } else { c }).collect();
    let joined = mapped.split_whitespace().collect::<Vec<_>>().join(" ");
    // .slice(0, 120) counts UTF-16 units; cut at a char boundary at or under that
    let mut units = 0usize;
    let cut: String = joined
        .chars()
        .take_while(|c| {
            units = units.saturating_add(c.len_utf16());
            units <= 120
        })
        .collect();
    if cut.is_empty() { "Takeoff".into() } else { cut }
}

/// The name without a trailing ".pdf" (any case), as `name.replace(/\.pdf$/i, '')`.
fn strip_pdf(n: &str) -> &str {
    match n.len().checked_sub(4).and_then(|i| n.get(i..).map(|tail| (i, tail))) {
        Some((i, tail)) if tail.eq_ignore_ascii_case(".pdf") => n.get(..i).unwrap_or(n),
        _ => n,
    }
}

/// `new Date(ms).toISOString()`.
fn iso_time(ms: f64) -> String {
    let s = cc_io_takeoff::xlsx::Stamp::from_ms(ms);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", s.year, s.month, s.day, s.hour, s.min, s.sec, s.ms)
}

impl TakeoffMode {
    /// A new takeoff of the file whose SHA-256 is `hash`.
    pub fn new(hash: &str) -> TakeoffMode {
        TakeoffMode {
            tk: Takeoff::default(),
            hash: hash.to_string(),
            item: None,
            pts: Vec::new(),
            arc: false,
            mid: None,
            last_click: None,
            cursor: None,
            sel_more: Vec::new(),
            drag: None,
            clip: None,
            paste: None,
            page_info: std::collections::HashMap::new(),
            sel_note: None,
            note_start: None,
            note_ask: None,
            notes_open_only: false,
            group_ask: None,
            cmp: None,
            cur_sheet: String::new(),
            goto: None,
            want_3d: false,
            message: String::new(),
            new_name: String::new(),
            new_type: "count".into(),
            export: None,
            ac_tool: false,
            ac_any: false,
            ac_box: None,
            ac: None,
            #[cfg(not(target_arch = "wasm32"))]
            ac_job: None,
            tool: "draw",
            sel: None,
            deduct: false,
            ded: std::collections::BTreeSet::new(),
            cur_area: None,
            px: 1.0,
            edits: std::collections::HashMap::new(),
            area_dialog: None,
            manage_areas: false,
            new_area: String::new(),
        }
    }

    /// A PDF joins the project's file list (not an undo step, as in the JS): its name labels its sheets.
    pub fn add_file(&mut self, hash: &str, name: &str, size: usize, pages: usize) {
        let f = obj(json!({ "hash": hash, "name": name, "size": size, "pages": pages }));
        self.tk.add_file(&f);
    }

    /// The file whose sheets the view shows now (its marks are "<hash>:<page>").
    pub fn set_file(&mut self, hash: &str) {
        self.hash = hash.to_string();
        self.pts.clear();
        self.ded.clear();
        self.ac = None;
        self.ac_box = None;
    }

    /// Whether any mark is on one of the file's sheets.
    pub fn has_marks(&self, hash: &str) -> bool {
        let p = format!("{hash}:");
        self.tk.marks().iter().any(|m| m.get("sheet").and_then(Value::as_str).is_some_and(|s| s.starts_with(&p)))
    }

    /// `sheetLabel(key)` for a sheet nobody named: "<file>, page <n>".
    pub(crate) fn sheet_label(&self, key: &str) -> String {
        let (hash, idx) = key.split_once(':').unwrap_or((key, "0"));
        let file = self
            .tk
            .state
            .get("files")
            .and_then(Value::as_array)
            .and_then(|fs| fs.iter().find(|f| f.get("hash").and_then(Value::as_str) == Some(hash)))
            .and_then(|f| f.get("name").and_then(Value::as_str))
            .map_or_else(|| t("sheet.unknown"), |n| strip_pdf(n).to_string());
        let n = idx.parse::<f64>().map_or(1.0, |i| i + 1.0);
        tf("sheet.page", &[("file", F::S(file)), ("n", F::N(n))])
    }

    /// One of the 0.5 exports, named as takeoffcraft.js names it: the estimate as XLSX or CSV (with a BOM), the PDF
    /// with the takeoff drawn in ("<pdf> (takeoff).pdf"), or the project ("<name>.takeoff.json", indent 1).
    /// `now_ms` dates the workbook (created in UTC, its zip entries on `zone`'s wall clock) and the project's savedAt.
    pub fn export_file(
        &self,
        kind: &str,
        hash: &str,
        pdf: &[u8],
        pdf_name: &str,
        now_ms: f64,
        zone: cc_clock::Zone,
    ) -> Result<(String, Vec<u8>), String> {
        let name = safe_name(self.tk.state.get("name").and_then(Value::as_str).unwrap_or(""));
        let has_items = !self.tk.items().is_empty();
        match kind {
            "xlsx" | "csv" if !has_items => Err(t("export.empty")),
            "xlsx" => {
                let label = |k: &str| self.sheet_label(k);
                let stamp = cc_io_takeoff::xlsx::Stamp::from_ms(now_ms);
                let local = cc_io_takeoff::xlsx::Stamp::local(now_ms, zone);
                Ok((format!("{name}.xlsx"), cc_io_takeoff::export::estimate_xlsx(&self.tk.state, &label, true, &stamp, &local)))
            }
            "csv" => {
                // exportCsv: the estimate without bid alternates (baseEstimate), as the JS writes it
                let e = cc_takeoff::model::base_estimate(&self.tk.state);
                let csv = cc_io_takeoff::export::estimate_csv(&self.tk.state, &e);
                Ok((format!("{name}.csv"), format!("\u{feff}{csv}").into_bytes()))
            }
            "pdf" => {
                let out = cc_io_takeoff::pdfmarks::pdf_with_marks(pdf, hash, &self.tk.state, true)?; // exportPdf: with the legend (the default)
                if out.marks == 0 {
                    return Err(t("export.pdf.none"));
                }
                Ok((format!("{} (takeoff).pdf", safe_name(strip_pdf(pdf_name))), out.bytes))
            }
            "project" => {
                let saved_at = iso_time(now_ms);
                let text = cc_takeoff::jv::stringify_indent(&self.tk.to_json(&saved_at), 1);
                Ok((format!("{name}.takeoff.json"), text.into_bytes()))
            }
            _ => Err(format!("takeoff export: unknown kind {kind} (xlsx, csv, pdf, project)")),
        }
    }

    /// The key marks of page `page` carry.
    pub fn sheet(&self, page: usize) -> String {
        sheet_key(&self.hash, page as f64)
    }

    pub(crate) fn item_type(&self) -> Option<String> {
        let it = self.tk.item(self.item.as_deref()?)?;
        it.get("type").and_then(Value::as_str).map(str::to_string)
    }

    /// The scale of a sheet in real feet per PDF unit, if set.
    pub fn ft_per_unit(&self, sheet: &str) -> Option<f64> {
        self.tk.scale_of(sheet).and_then(|s| s.get("ftPerUnit")).and_then(Value::as_f64).filter(|x| *x > 0.0)
    }

    /// Set a sheet's scale from a preset (one undo step).
    pub fn set_preset(&mut self, sheet: &str, key: &str, user_unit: f64) {
        let Some(p) = cc_takeoff::units::preset(key) else { return };
        match cc_takeoff::units::preset_ft_per_unit(key, user_unit) {
            Ok(fpu) => {
                let sc = json!({ "method": "preset", "ftPerUnit": fpu, "preset": key, "label": p.label, "cal": null });
                let label = tf("sheetbar.scale", &[("scale", F::S(p.label.to_string()))]);
                if let Err(e) = self.tk.set_scale(sheet, &sc, Some(&label)) {
                    self.message = e;
                } else {
                    self.message = tf("scale.set", &[("scale", F::S(p.label.to_string()))]);
                }
            }
            Err(e) => self.message = e,
        }
    }

    /// Add an item (count, linear or area) and make it the current one.
    pub fn add_item(&mut self, name: &str, ty: &str) -> Option<String> {
        let f = obj(json!({ "name": name, "type": ty }));
        match self.tk.add_item(&f) {
            Ok(Some(id)) => {
                self.item = Some(id.clone());
                self.message = tf(&format!("item.ready.{ty}"), &[("item", F::S(name.to_string()))]);
                Some(id)
            }
            Ok(None) => None,
            Err(e) => {
                self.message = e;
                None
            }
        }
    }

    /// The kind of thing the tool draws: the current item's type for Draw (linear, area, count), cutout, circle,
    /// cutcircle, a markup tool (note, cloud, highlight), or None (Select).
    pub(crate) fn tool_kind(&self) -> Option<String> {
        match self.tool {
            "draw" => self.item_type(),
            "select" => None,
            t => Some(t.to_string()),
        }
    }

    /// The pick distance in PDF units for `screen` pixels (`px / view.z`).
    pub(crate) fn upx(&self, screen: f64) -> f64 {
        screen * if self.px.is_finite() && self.px > 0.0 { self.px } else { 1.0 }
    }

    /// A click on the sheet at `p` (PDF user space), as takeoffcraft.js `click`: the alignment picks of Compare, a
    /// paste put down, Select picks a mark (Shift adds or takes out), a count mark at once, a note, a cloud's corners,
    /// a circle's centre and edge; otherwise one more point of the run / outline / cut-out (a double-click's second
    /// press, within 3 screen pixels of the last click, adds none; a click by the first corner of an outline closes it;
    /// in arc mode a point on the curve, then the arc's end).
    pub fn click(&mut self, sheet: &str, p: [f64; 2]) {
        self.click_mod(sheet, p, false);
    }

    /// `click` with Shift held (Select: add to or take out of the selection).
    pub fn click_mod(&mut self, sheet: &str, p: [f64; 2], shift: bool) {
        let p = P::new(p[0], p[1]);
        if self.cmp.as_ref().is_some_and(|c| c.align.is_some()) {
            self.align_pick(p);
            return;
        }
        if self.paste.is_some() {
            self.place_paste(sheet, p);
            return;
        }
        if self.tool == "select" {
            self.select_at_mod(sheet, [p.x, p.y], shift);
            return;
        }
        let kind = self.tool_kind().unwrap_or_default();
        let item_tool = matches!(self.tool, "draw" | "circle");
        if item_tool && self.item.as_deref().and_then(|i| self.tk.item(i)).is_none() {
            self.set_tool("select");
            self.message = t("tool.err.item");
            return;
        }
        let measures = matches!(kind.as_str(), "linear" | "area" | "cutout" | "circle" | "cutcircle");
        if measures && self.ft_per_unit(sheet).is_none() {
            self.pts.clear();
            self.mid = None;
            self.last_click = None;
            self.message = t("tool.err.noscale.measure");
            return;
        }
        if matches!(kind.as_str(), "cutout" | "cutcircle")
            && self.sel.as_deref().and_then(|m| self.tk.mark(m)).is_none_or(|m| m.get("sheet").and_then(Value::as_str) != Some(sheet))
        {
            self.set_tool("select");
            return;
        }
        let Some(item) = self.item.clone() else { return self.markup_click(sheet, &kind, p) };
        if kind == "count" {
            let mut f = obj(json!({ "item": item, "sheet": sheet, "pts": [{ "x": p.x, "y": p.y }], "kind": "count" }));
            self.in_area(&mut f);
            match self.tk.add_mark(&f) {
                Ok(id) => self.set_sel(id),
                Err(e) => self.message = e,
            }
            return;
        }
        self.markup_click(sheet, &kind, p);
    }

    /// The rest of `click`: markups, then the draft's points.
    fn markup_click(&mut self, sheet: &str, kind: &str, p: P) {
        match kind {
            "note" => {
                self.note_at(sheet, p);
                return;
            }
            "cloud" | "highlight" => {
                match self.note_start.take() {
                    None => {
                        self.note_start = Some(p);
                        self.message = t(&format!("help.{kind}.2"));
                    }
                    Some(a) => self.box_note(sheet, kind, a, p),
                }
                return;
            }
            "linear" | "area" | "cutout" | "circle" | "cutcircle" => {}
            _ => return,
        }
        let run = matches!(kind, "linear" | "area" | "cutout");
        if run
            && let Some(l) = self.last_click
            && (l.x - p.x).hypot(l.y - p.y) <= self.upx(3.0)
        {
            return;
        }
        self.last_click = Some(p);
        if matches!(kind, "circle" | "cutcircle") {
            match self.pts.first().copied() {
                None => self.pts.push(p),
                Some(c) => self.finish_circle(sheet, c, p),
            }
            return;
        }
        let outline = matches!(kind, "area" | "cutout");
        let near_first = |me: &Self| me.pts.first().is_some_and(|f0| (f0.x - p.x).hypot(f0.y - p.y) <= me.upx(9.0));
        if self.arc && !self.pts.is_empty() {
            // arc mode: the first click after a point is on the curve, the second is where it ends
            let Some(mid) = self.mid.take() else {
                self.mid = Some(p);
                return;
            };
            let Some(last) = self.pts.last().copied() else { return };
            let closing = outline && self.pts.len() >= 2 && near_first(self);
            let end = if closing { self.pts.first().copied().unwrap_or(p) } else { p };
            let b = cc_takeoff::geom::bulge_through(&last, &mid, &end);
            if let Some(l) = self.pts.last_mut() {
                *l = P::arc(last.x, last.y, if b != 0.0 && !b.is_nan() { b } else { 0.0 });
            }
            if closing {
                self.finish(sheet);
                return;
            }
            self.pts.push(P::new(end.x, end.y));
            self.note_deduct(kind);
            return;
        }
        if outline && self.pts.len() >= 3 && near_first(self) {
            self.finish(sheet);
            return;
        }
        if self.pts.len() < MAX_PTS {
            self.pts.push(p);
            self.note_deduct(kind);
        }
    }

    /// noteDeduct: in deduct mode the segment a run just gained is a deduct.
    fn note_deduct(&mut self, kind: &str) {
        if kind != "linear" || self.pts.len() < 2 {
            return;
        }
        let i = self.pts.len() - 2;
        if self.deduct {
            self.ded.insert(i);
        } else {
            self.ded.remove(&i);
        }
    }

    /// The draft is dropped (Esc, another tool).
    pub fn cancel_draft(&mut self) {
        self.pts.clear();
        self.ded.clear();
        self.mid = None;
        self.last_click = None;
        self.drag = None;
        self.note_start = None;
    }

    /// Finish the run / outline / cut-out being clicked (double-click or Enter), as `finishDraft`: points on the one
    /// before (2 screen px) are dropped, the run keeps its deducts on the right segments (its last point starts no arc),
    /// and the checks of 0.5 apply.
    pub fn finish(&mut self, sheet: &str) {
        let raw = std::mem::take(&mut self.pts);
        let ded0 = std::mem::take(&mut self.ded);
        self.mid = None;
        self.last_click = None;
        let Some(ty) = self.tool_kind().filter(|k| matches!(k.as_str(), "linear" | "area" | "cutout")) else { return };
        let Some(item) = self.item.clone().or_else(|| (ty == "cutout").then(String::new)) else { return };
        let tol = self.upx(2.0);
        let near = |a: &P, b: &P| (a.x - b.x).hypot(a.y - b.y) <= tol;
        let mut kept: Vec<P> = Vec::new();
        let mut ded: Vec<usize> = Vec::new();
        for (j, p) in raw.iter().enumerate() {
            if kept.last().is_some_and(|l| near(l, p)) {
                continue;
            }
            kept.push(*p);
            if ty == "linear" && j > 0 && kept.len() > 1 && ded0.contains(&(j - 1)) {
                let k = kept.len() - 2;
                if !ded.contains(&k) {
                    ded.push(k);
                }
            }
        }
        if ty != "linear" {
            while kept.len() > 3
                && kept.get(kept.len() - 2).is_some_and(|q| !q.has_b())
                && kept.first().zip(kept.last()).is_some_and(|(a, b)| near(a, b))
            {
                kept.pop();
            }
        } else if let Some(l) = kept.last_mut() {
            l.b = 0.0;
        }
        let pts: Vec<Value> = kept.iter().map(pt_json).collect();
        match ty.as_str() {
            "linear" => {
                if kept.len() < 2 {
                    self.message = t("run.err.points");
                    return;
                }
                if !ded.is_empty() && ded.len() >= kept.len() - 1 {
                    self.message = t("deduct.all");
                    return;
                }
                let mut f = obj(json!({ "item": item, "sheet": sheet, "pts": pts, "kind": "linear" }));
                if !ded.is_empty() {
                    f.insert("deducts".into(), json!(ded));
                }
                self.in_area(&mut f);
                match self.tk.add_mark(&f) {
                    Ok(id) => self.set_sel(id),
                    Err(e) => self.message = e,
                }
            }
            "area" => {
                if let Some(why) = cc_takeoff::geom::outline_problem(&kept, "area") {
                    self.message = why;
                    return;
                }
                let backout = self.tk.item(&item).is_some_and(|it| it.get("backout") == Some(&Value::Bool(true)));
                if backout && !cc_takeoff::geom::is_convex(&kept) {
                    self.message = t("backout.convex");
                    return;
                }
                let mut f = obj(json!({ "item": item, "sheet": sheet, "pts": pts, "kind": "area" }));
                self.in_area(&mut f);
                match self.tk.add_mark(&f) {
                    Ok(id) => self.set_sel(id),
                    Err(e) => self.message = e,
                }
            }
            _ => self.add_hole(&kept, pts, "Cut-out", "cutout.added"),
        }
    }

    /// A hole in the selected area (a cut-out or a round one), checked as holes.js does.
    fn add_hole(&mut self, ring: &[P], pts: Vec<Value>, label: &str, done: &str) {
        let Some(m) = self.sel.as_deref().and_then(|i| self.tk.mark(i)).cloned() else { return };
        let outer = cc_takeoff::model::pts_of(m.get("pts"));
        let holes = cc_takeoff::model::holes_of(m.get("holes"));
        if let Some(why) = cc_takeoff::geom::hole_problem(&outer, &holes, ring) {
            self.message = why;
            return;
        }
        let mut hl = m.get("holes").and_then(Value::as_array).cloned().unwrap_or_default();
        hl.push(Value::Array(pts));
        let id = m.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        match self.tk.update_mark(&id, &obj(json!({ "holes": hl })), Some(label)) {
            Ok(_) => self.message = t(done),
            Err(e) => self.message = e,
        }
    }

    /// finishCircle: a circle as two half-circle arcs (exact area and perimeter), a new area or a round cut-out.
    fn finish_circle(&mut self, sheet: &str, c: P, edge: P) {
        self.pts.clear();
        self.last_click = None;
        let r = cc_takeoff::geom::dist(&c, &edge);
        if r < self.upx(3.0) {
            self.message = t("circle.err.small");
            return;
        }
        let ring = [P::arc(c.x + r, c.y, 1.0), P::arc(c.x - r, c.y, 1.0)];
        let pts: Vec<Value> = ring.iter().map(pt_json).collect();
        if self.tool == "cutcircle" {
            self.add_hole(&ring, pts, &t("undo.cutout.round"), "cutout.round.added");
            return;
        }
        let Some(item) = self.item.clone() else { return };
        let mut f = obj(json!({ "item": item, "sheet": sheet, "pts": pts, "kind": "area" }));
        self.in_area(&mut f);
        match self.tk.add_mark(&f) {
            Ok(id) => self.set_sel(id),
            Err(e) => self.message = e,
        }
    }

    /// The right-hand panel: the sheet's scale, the items with their quantities, the estimate's totals.
    pub fn panel(&mut self, ui: &mut egui::Ui, t: &Tokens, sheet: &str, user_unit: f64) {
        ui.label(RichText::new(cc_takeoff::i18n::t("group.takeoff")).strong().color(t.text_strong));
        // scale
        let cur = self
            .tk
            .scale_of(sheet)
            .and_then(|s| s.get("label"))
            .and_then(Value::as_str)
            .map_or_else(|| cc_takeoff::i18n::t("sheetbar.noscale"), str::to_string);
        let mut pick: Option<&'static str> = None;
        let l = ui.label(RichText::new(cc_takeoff::i18n::t("scale.preset.aria")).small());
        let _ = egui::ComboBox::from_id_salt("tk_scale")
            .selected_text(cur)
            .width(ui.available_width().clamp(80.0, 220.0))
            .show_ui(ui, |ui| {
                for p in &cc_takeoff::units::PRESETS {
                    if ui.selectable_label(false, format!("{}  ({})", p.label, p.group)).clicked() {
                        pick = Some(p.key);
                    }
                }
            })
            .response
            .labelled_by(l.id);
        if let Some(k) = pick {
            self.set_preset(sheet, k, user_unit);
        }
        ui.horizontal_wrapped(|ui| {
            let draw = self.item_type().map_or_else(|| cc_takeoff::i18n::t("tool.linear"), |ty| cc_takeoff::i18n::t(&format!("tool.{ty}")));
            for (tool, label) in
                [("select", cc_takeoff::i18n::t("tool.select")), ("draw", draw), ("circle", cc_takeoff::i18n::t("tool.circle"))]
            {
                if ui.selectable_label(self.tool == tool, label).clicked() {
                    self.set_tool(tool);
                }
            }
            ui.menu_button(cc_takeoff::i18n::t("note.menu"), |ui| {
                for k in ["note", "cloud", "highlight"] {
                    if ui.selectable_label(self.tool == k, cc_takeoff::i18n::t(&format!("tool.{k}"))).clicked() {
                        self.set_tool(match k {
                            "note" => "note",
                            "cloud" => "cloud",
                            _ => "highlight",
                        });
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text(cc_takeoff::i18n::t("note.menu.title"));
            if ui.button(cc_takeoff::i18n::t("d3.button")).on_hover_text(cc_takeoff::i18n::t("d3.button.title")).clicked() {
                self.want_3d = true;
            }
            let mut a = self.arc;
            if ui.checkbox(&mut a, cc_takeoff::i18n::t("tool.arc")).on_hover_text(cc_takeoff::i18n::t("tool.arc.title")).changed() {
                self.toggle_arc(a);
            }
            let mut d = self.deduct;
            if ui
                .checkbox(&mut d, cc_takeoff::i18n::t("toolbar.deduct"))
                .on_hover_text(cc_takeoff::i18n::t("toolbar.deduct.title"))
                .changed()
            {
                self.toggle_deduct(d);
            }
        });
        self.areas_row(ui);
        ui.separator();
        // items
        let q = quantities(&self.tk.state, &QOpts::default());
        let mut select: Option<String> = None;
        let mut remove: Option<String> = None;
        for it in self.tk.items() {
            let id = it.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            let name = it.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let ty = it.get("type").and_then(Value::as_str).unwrap_or("");
            let color = it.get("color").and_then(Value::as_str).and_then(|c| crate::pdfview::hex_color(c, 1.0)).unwrap_or(Color32::GRAY);
            let tot = q.get(&id).map(|x| &x.tot);
            let amount = match (ty, tot) {
                ("count", Some(t)) => format!("{} ea", qty(t.count)),
                ("linear", Some(t)) => format!("{} ft", qty(t.length_ft)),
                ("area", Some(t)) => format!("{} sf", qty(t.area_sqft)),
                _ => String::new(),
            };
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, color);
                let on = self.item.as_deref() == Some(id.as_str());
                if ui.selectable_label(on, format!("{name}  ·  {amount}")).clicked() {
                    select = Some(id.clone());
                }
                if ui.small_button(cc_takeoff::i18n::t("packs.remove")).clicked() {
                    remove = Some(id.clone());
                }
            });
        }
        if let Some(id) = select {
            self.item = Some(id);
            self.pts.clear();
            self.ded.clear();
            if self.tool == "select" {
                self.tool = "draw";
            }
        }
        if let Some(id) = remove {
            let _ = self.tk.remove_item(&id);
            if self.item.as_deref() == Some(id.as_str()) {
                self.item = None;
            }
        }
        // add an item
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.new_name)
                .hint_text(cc_takeoff::i18n::t("items.new.placeholder"))
                .desired_width(ui.available_width()),
        );
        ui.horizontal(|ui| {
            for (ty, label) in [
                ("count", cc_takeoff::i18n::t("type.count")),
                ("linear", cc_takeoff::i18n::t("type.linear")),
                ("area", cc_takeoff::i18n::t("type.area")),
            ] {
                ui.radio_value(&mut self.new_type, ty.to_string(), label);
            }
        });
        if ui.button(cc_takeoff::i18n::t("items.new")).clicked() {
            let name = if self.new_name.trim().is_empty() {
                format!("Item {}", self.tk.items().len() + 1)
            } else {
                self.new_name.trim().to_string()
            };
            let ty = self.new_type.clone();
            self.add_item(&name, &ty);
            self.new_name.clear();
        }
        if self.item.is_some() {
            ui.separator();
            egui::CollapsingHeader::new(RichText::new(cc_takeoff::i18n::t("item.h")).strong())
                .id_salt("tk_item_props")
                .default_open(true)
                .show(ui, |ui| {
                    self.item_props(ui, t);
                });
        }
        if self.sel.is_some() {
            ui.separator();
            self.mark_props(ui, t);
        }
        if self.sel_note.is_some() {
            ui.separator();
            self.note_props(ui, t);
        }
        if self.tk.state.get("groups").and_then(Value::as_array).is_some_and(|g| !g.is_empty()) {
            ui.separator();
            self.groups_list(ui, t);
        }
        if self.tk.state.get("notes").and_then(Value::as_array).is_some_and(|g| !g.is_empty()) {
            ui.separator();
            self.notes_list(ui, t);
        }
        self.note_window(ui.ctx());
        self.group_window(ui.ctx());
        ui.separator();
        let e = cc_takeoff::model::base_estimate(&self.tk.state);
        ui.label(format!(
            "{} · {} ${:.2}",
            cc_takeoff::i18n::tn("n.marks", self.tk.marks().len() as f64),
            cc_takeoff::i18n::t("x.sheet.estimate"),
            e.total / 100.0
        ));
        if e.unscaled > 0.0 {
            ui.label(RichText::new(cc_takeoff::i18n::tn("qty.warn.unscaled", e.unscaled)).color(t.text_dim));
        }
        self.by_area(ui, t);
        self.autocount_panel(ui, t);
        self.area_windows(ui.ctx());
        ui.separator();
        ui.label(RichText::new(cc_takeoff::i18n::t("group.export")).small());
        ui.horizontal_wrapped(|ui| {
            for (kind, key) in
                [("xlsx", "tool.export-xlsx"), ("csv", "tool.export-csv"), ("pdf", "tool.export-pdf"), ("project", "tool.save-project")]
            {
                if ui.button(cc_takeoff::i18n::t(key)).on_hover_text(cc_takeoff::i18n::t(&format!("{key}.title"))).clicked() {
                    self.export = Some(kind);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(self.tk.can_undo(), egui::Button::new(cc_takeoff::i18n::t("tool.undo"))).clicked() {
                self.history(false);
            }
            if ui.add_enabled(self.tk.can_redo(), egui::Button::new(cc_takeoff::i18n::t("tool.redo"))).clicked() {
                self.history(true);
            }
            let mut act: Option<&str> = None;
            for (a, k) in [
                ("copy", "edit.copy"),
                ("cut", "edit.cut"),
                ("paste", "edit.paste"),
                ("paste-in-place", "edit.paste_in_place"),
                ("duplicate", "edit.duplicate"),
                ("select-all", "edit.select_all"),
                ("select-none", "edit.select_none"),
            ] {
                if ui.small_button(cc_takeoff::i18n::t(k)).clicked() {
                    act = Some(a);
                }
            }
            if let Some(a) = act {
                let _ = self.edit(sheet, a);
            }
        });
        if !self.message.is_empty() {
            ui.label(RichText::new(&self.message).color(t.text_dim));
        }
    }

    /// The current item's count marks on `sheet` (Auto count marks matches over them as already counted).
    pub fn own_counts(&self, sheet: &str) -> Vec<[f64; 2]> {
        let Some(item) = self.item.as_deref() else { return Vec::new() };
        self.tk
            .marks()
            .iter()
            .filter(|m| m.get("item").and_then(Value::as_str) == Some(item) && m.get("sheet").and_then(Value::as_str) == Some(sheet))
            .filter_map(|m| cc_takeoff::model::pts_of(m.get("pts")).first().map(|p| [p.x, p.y]))
            .collect()
    }

    /// Whether the current item takes count marks (Auto count needs one).
    pub fn counts(&self) -> bool {
        self.item
            .as_deref()
            .and_then(|i| self.tk.item(i))
            .is_some_and(|it| cc_takeoff::model::kinds_of(Some(it)).iter().any(|k| k == "count"))
    }

    /// `ensureItem('count')`: the current item if it takes counts, else the last count item, else ask for one.
    pub fn ensure_count(&mut self) -> bool {
        if self.counts() {
            return true;
        }
        let last = self.tk.items().iter().rev().find(|i| i.get("type").and_then(Value::as_str) == Some("count")).cloned();
        match last {
            Some(it) => {
                self.item = it.get("id").and_then(Value::as_str).map(str::to_string);
                let name = it.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                self.message = tf("tool.taking", &[("item", F::S(name))]);
                true
            }
            None => {
                self.message = t("tool.make.count");
                false
            }
        }
    }

    /// Auto count's part of the panel: the tool, any angle, then the review (summary, match slider, add / discard).
    fn autocount_panel(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        use cc_takeoff::i18n::{t as tr, tn};
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            let b = ui.selectable_label(self.ac_tool, tr("tool.autocount")).on_hover_text(tr("tool.autocount.title"));
            if b.clicked() {
                self.ac_tool = !self.ac_tool;
                self.ac_box = None;
                if self.ac_tool {
                    self.ensure_count();
                }
            }
            ui.checkbox(&mut self.ac_any, tr("ac.angle"));
        });
        let mut accept = false;
        let mut discard = false;
        if let Some(ac) = self.ac.as_mut() {
            ui.label(RichText::new(ac.summary()).color(t.text));
            ui.horizontal(|ui| {
                let l = ui.label(tr("ac.match"));
                ui.add(
                    egui::Slider::new(&mut ac.threshold, crate::autocount::FLOOR..=1.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                )
                .labelled_by(l.id);
            });
            let n = ac.adding().len();
            ui.horizontal(|ui| {
                if ui.add_enabled(n > 0, egui::Button::new(tn("ac.accept", n as f64))).clicked() {
                    accept = true;
                }
                if ui.button(tr("ac.discard")).clicked() {
                    discard = true;
                }
            });
        }
        if accept {
            self.accept_autocount();
        }
        if discard {
            self.ac = None;
            self.message = tr("ac.discarded");
        }
    }

    /// Add the kept matches (one undo step) and end the review.
    pub fn accept_autocount(&mut self) {
        let Some(ac) = self.ac.take() else { return };
        let name = self.tk.item(&ac.item).and_then(|it| it.get("name")).and_then(Value::as_str).unwrap_or("").to_string();
        match ac.accept(&mut self.tk) {
            Ok(0) => {}
            Ok(n) => {
                let fills = [("n", F::N(n as f64)), ("item", F::S(name)), ("mod", F::S("Ctrl".into()))];
                self.message = tf("ac.added", &fills);
            }
            Err(e) => self.message = e,
        }
    }

    /// The marks of `sheet` and everything over them (drawOverlay): flags, the marks (a dragged one where it is being
    /// dragged), markups, typical group boxes, the selection's corners and arc handles with the selected mark's
    /// measurements, the draft with its rubber band, Auto count, the paste on the pointer. `at`: PDF user space → screen.
    pub fn draw(&self, painter: &egui::Painter, sheet: &str, at: &dyn Fn(f64, f64) -> Pos2) {
        let color_of = |item: &str| {
            self.tk
                .item(item)
                .and_then(|it| it.get("color"))
                .and_then(Value::as_str)
                .and_then(|c| crate::pdfview::hex_color(c, 1.0))
                .unwrap_or(Color32::from_rgb(0x2f, 0x6f, 0xde))
        };
        let path = |pts: &[P], closed: bool| -> Vec<Pos2> {
            let mut r: Vec<Pos2> = cc_takeoff::geom::flatten(pts, closed).iter().map(|p| at(p.x, p.y)).collect();
            if closed && let Some(f) = r.first().copied() {
                r.push(f);
            }
            r
        };
        let action = Color32::from_rgb(0x1e, 0x48, 0xc8);
        let ink = Color32::from_rgb(0x17, 0x20, 0x2b);
        self.draw_compare(painter, sheet, at);
        let drag = self.drag.as_ref().filter(|d| d.moved);
        for m0 in self.tk.marks() {
            if m0.get("sheet").and_then(Value::as_str) != Some(sheet) {
                continue;
            }
            let id = m0.get("id").and_then(Value::as_str).unwrap_or("");
            let item = m0.get("item").and_then(Value::as_str).unwrap_or("");
            if self.tk.item(item).is_none() {
                continue;
            }
            let kind = m0.get("kind").and_then(Value::as_str).unwrap_or("");
            let (gp, holes) = match drag.filter(|d| d.id == id) {
                Some(d) => (d.pts.clone(), d.holes.clone()),
                None => (cc_takeoff::model::pts_of(m0.get("pts")), cc_takeoff::model::holes_of(m0.get("holes"))),
            };
            let selected = self.is_sel(id);
            // marks in another bid area than the current one are dimmed, as 0.5 does
            let dim = self.cur_area.as_deref().is_some_and(|a| m0.get("area").and_then(Value::as_str) != Some(a));
            let c = if dim { color_of(item).gamma_multiply(0.22) } else { color_of(item) };
            if m0.get("flag").is_some_and(|f| !f.is_null() && f.as_bool() != Some(false)) {
                // T13: the drawing changed under it in the revision — an orange halo until checked
                let fc = Color32::from_rgb(0xff, 0x7f, 0x0e).gamma_multiply(0.9);
                if kind == "count" {
                    if let Some(p) = gp.first() {
                        painter.circle_stroke(at(p.x, p.y), 12.0, egui::Stroke::new(9.0, fc));
                    }
                } else {
                    for r in std::iter::once(&gp).chain(if kind == "area" { holes.iter() } else { [].iter() }) {
                        painter.add(egui::Shape::dashed_line(&path(r, kind == "area"), egui::Stroke::new(9.0, fc), 8.0, 6.0));
                    }
                }
            }
            match kind {
                "count" => {
                    if let Some(p) = gp.first() {
                        painter.circle(at(p.x, p.y), if selected { 7.5 } else { 6.0 }, c, egui::Stroke::new(1.5, Color32::WHITE));
                    }
                }
                "area" => {
                    let backout = self.tk.item(item).is_some_and(|it| it.get("backout") == Some(&Value::Bool(true)));
                    if holes.is_empty() && cc_takeoff::geom::is_convex(&cc_takeoff::geom::flatten(&gp, true)) {
                        let mut fill = path(&gp, true);
                        fill.pop();
                        if fill.len() >= 3 {
                            let a = if dim {
                                0.06
                            } else if backout {
                                0.1
                            } else {
                                0.2
                            };
                            painter.add(egui::Shape::convex_polygon(fill, color_of(item).gamma_multiply(a), egui::Stroke::NONE));
                        }
                    }
                    for r in std::iter::once(&gp).chain(holes.iter()) {
                        let line = path(r, true);
                        let w = if selected { 3.5 } else { 2.0 };
                        if backout {
                            painter.add(egui::Shape::dashed_line(&line, egui::Stroke::new(w, c), 4.0, 4.0));
                        } else {
                            painter.add(egui::Shape::line(line, egui::Stroke::new(w, c)));
                        }
                    }
                }
                _ => {
                    let ded: Vec<f64> = m0
                        .get("deducts")
                        .and_then(Value::as_array)
                        .map(|l| l.iter().filter_map(Value::as_f64).collect())
                        .unwrap_or_default();
                    let w = if selected { 4.0 } else { 2.5 };
                    #[allow(clippy::cast_precision_loss)]
                    for sp in cc_takeoff::geom::spans(&gp, &|i| ded.contains(&(i as f64))) {
                        let line = path(&sp.pts, false);
                        if sp.deduct {
                            painter.add(egui::Shape::dashed_line(&line, egui::Stroke::new(w * 0.6, c), 7.0, 5.0));
                        } else {
                            painter.add(egui::Shape::line(line, egui::Stroke::new(w, c)));
                        }
                    }
                }
            }
        }
        self.draw_notes(painter, sheet, at);
        self.draw_groups(painter, sheet, at);
        let square = |p: &P, hs: f32| {
            let r = egui::Rect::from_center_size(at(p.x, p.y), egui::vec2(2.0 * hs, 2.0 * hs));
            painter.rect_filled(r, 0.0, Color32::WHITE);
            painter.rect_stroke(r, 0.0, egui::Stroke::new(1.5, action), egui::StrokeKind::Middle);
        };
        if !self.sel_more.is_empty() {
            // several selected: a small square on every corner, no handles to drag
            for id in self.sel_ids() {
                let Some(m) = self.tk.mark(&id).filter(|m| m.get("sheet").and_then(Value::as_str) == Some(sheet)) else { continue };
                for p in cc_takeoff::model::pts_of(m.get("pts")).iter().chain(cc_takeoff::model::holes_of(m.get("holes")).iter().flatten())
                {
                    square(p, 3.0);
                }
            }
        } else if let Some(m) =
            self.sel.as_deref().and_then(|i| self.tk.mark(i)).filter(|m| m.get("sheet").and_then(Value::as_str) == Some(sheet))
        {
            // the selected mark: corner handles, its measurements, a round handle in the middle of each arc
            let id = m.get("id").and_then(Value::as_str).unwrap_or("");
            let mut mm = m.clone();
            if let (Some(d), Value::Object(o)) = (drag.filter(|d| d.id == id), &mut mm) {
                o.insert("pts".into(), Value::Array(d.pts.iter().map(pt_json).collect()));
                o.insert("holes".into(), Value::Array(d.holes.iter().map(|h| Value::Array(h.iter().map(pt_json).collect())).collect()));
            }
            let gp = cc_takeoff::model::pts_of(mm.get("pts"));
            let holes = cc_takeoff::model::holes_of(mm.get("holes"));
            for p in gp.iter().chain(holes.iter().flatten()) {
                square(p, 4.0);
            }
            let item = mm.get("item").and_then(Value::as_str).unwrap_or("");
            if let Some(fpu) = self.ft_per_unit(sheet) {
                let tag = |q: Pos2, text: &str| {
                    let font = egui::FontId::proportional(12.0);
                    let g = painter.layout_no_wrap(text.to_string(), font, ink);
                    let r = egui::Rect::from_center_size(q, egui::vec2(g.size().x + 10.0, 18.0));
                    painter.rect_filled(r, 0.0, Color32::WHITE);
                    painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, ink), egui::StrokeKind::Inside);
                    painter.galley(q - g.size() / 2.0, g, ink);
                };
                match cc_takeoff::model::measure_mark(&mm, fpu, self.tk.item(item)) {
                    Some(cc_takeoff::model::Measure::Linear { sloped_ft, .. }) => {
                        for (i, w) in gp.windows(2).enumerate() {
                            let (Some(a), Some(z)) = (w.first(), w.get(1)) else { continue };
                            let (sa, sz) = (at(a.x, a.y), at(z.x, z.y));
                            if sa.distance(sz) < 40.0 {
                                continue;
                            }
                            let mid = crate::takeoffedit::seg_mid(a, z);
                            let mut q = at(mid.x, mid.y);
                            if a.has_b() {
                                let v = q - (sa + (sz - sa) / 2.0);
                                if v.length() > 1e-6 {
                                    q += v.normalized() * 18.0;
                                }
                            }
                            tag(q, &ft_in(sloped_ft.get(i).copied().unwrap_or(0.0)));
                        }
                    }
                    Some(cc_takeoff::model::Measure::Area { area_sqft, .. }) => {
                        let c = cc_takeoff::geom::centroid(&gp);
                        tag(at(c.x, c.y), &format!("{} SF", cc_takeoff::units::fixed(area_sqft, 2)));
                    }
                    _ => {}
                }
            }
            for h in crate::takeoffedit::arc_handles(&mm) {
                painter.circle(at(h.at.x, h.at.y), 5.0, Color32::WHITE, egui::Stroke::new(1.5, action));
            }
        }
        // the draft
        if !self.pts.is_empty() {
            let kind = self.tool_kind().unwrap_or_default();
            let hole = matches!(kind.as_str(), "cutout" | "cutcircle");
            let c = if hole { ink } else { self.item.as_deref().map_or(ink, color_of) };
            let stroke = egui::Stroke::new(2.0, c);
            if matches!(kind.as_str(), "circle" | "cutcircle") {
                if let (Some(c0), Some(cur)) = (self.pts.first(), self.cursor) {
                    let r = (cur[0] - c0.x).hypot(cur[1] - c0.y);
                    let ring = [P::arc(c0.x + r, c0.y, 1.0), P::arc(c0.x - r, c0.y, 1.0)];
                    painter.add(egui::Shape::line(path(&ring, true), stroke));
                    painter.add(egui::Shape::dashed_line(&[at(c0.x, c0.y), at(cur[0], cur[1])], stroke, 6.0, 4.0));
                }
            } else {
                let pts = self.preview_pts();
                let n = self.pts.len();
                let drawn = pts.get(..n).unwrap_or(&pts);
                if kind == "linear" && !self.ded.is_empty() {
                    for sp in cc_takeoff::geom::spans(drawn, &|i| self.ded.contains(&i)) {
                        let line = path(&sp.pts, false);
                        if sp.deduct {
                            painter.add(egui::Shape::dashed_line(&line, egui::Stroke::new(1.2, c), 7.0, 5.0));
                        } else {
                            painter.add(egui::Shape::line(line, stroke));
                        }
                    }
                } else {
                    painter.add(egui::Shape::line(path(drawn, false), stroke));
                }
                if self.cursor.is_some() && pts.len() >= 2 {
                    let tail = pts.get(pts.len() - 2..).unwrap_or(&[]);
                    painter.add(egui::Shape::dashed_line(&path(tail, false), stroke, 6.0, 4.0));
                }
                if let Some(m) = self.mid {
                    painter.circle_stroke(at(m.x, m.y), 4.0, stroke);
                }
            }
            for p in &self.pts {
                painter.rect_filled(egui::Rect::from_center_size(at(p.x, p.y), egui::vec2(6.0, 6.0)), 0.0, c);
            }
            if matches!(kind.as_str(), "area" | "cutout")
                && self.pts.len() >= 3
                && let Some(f) = self.pts.first()
            {
                painter.circle_stroke(at(f.x, f.y), 9.0, egui::Stroke::new(1.0, c));
            }
        }
        if let Some((a, b)) = self.ac_box {
            let r = egui::Rect::from_two_pos(at(a[0], a[1]), at(b[0], b[1]));
            let stroke = egui::Stroke::new(1.5, Color32::from_rgb(0x2f, 0x6f, 0xde));
            let ring = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.add(egui::Shape::dashed_line(&ring, stroke, 5.0, 3.0));
        }
        if let Some(ac) = self.ac.as_ref().filter(|ac| ac.sheet == sheet) {
            let c = color_of(&ac.item);
            for i in ac.visible() {
                let Some(m) = ac.matches.get(i) else { continue };
                let mut ring: Vec<Pos2> = m.corners.iter().map(|q| at(q[0], q[1])).collect();
                if let Some(f) = ring.first().copied() {
                    ring.push(f);
                }
                if m.rejected {
                    painter.add(egui::Shape::dashed_line(&ring, egui::Stroke::new(1.0, Color32::GRAY), 4.0, 3.0));
                } else {
                    painter.add(egui::Shape::line(ring, egui::Stroke::new(2.0, c)));
                }
            }
        }
        self.draw_paste(painter, at);
    }

    /// The run or outline being drawn, with the pending arc (or straight segment) to the pointer (previewPts).
    pub(crate) fn preview_pts(&self) -> Vec<P> {
        let mut pts = self.pts.clone();
        let (Some(cur), Some(last)) = (self.cursor, pts.last().copied()) else { return pts };
        let c = P::new(cur[0], cur[1]);
        if let Some(m) = self.mid {
            let b = cc_takeoff::geom::bulge_through(&last, &m, &c);
            if b != 0.0
                && let Some(l) = pts.last_mut()
            {
                l.b = b;
            }
        }
        pts.push(c);
        pts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_safe_as_the_js_does() {
        assert_eq!(safe_name("A/B: c*?  d"), "A B c d");
        assert_eq!(safe_name("  "), "Takeoff");
        assert_eq!(strip_pdf("Plans.PDF"), "Plans");
        assert_eq!(strip_pdf("pdf"), "pdf");
        assert_eq!(iso_time(0.0), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn exports_need_items_and_are_named_from_the_takeoff() {
        let mut m = TakeoffMode::new("abc");
        m.add_file("abc", "Kitchen.pdf", 10, 1);
        assert_eq!(m.sheet_label("abc:0"), "Kitchen, page 1");
        assert_eq!(m.export_file("csv", "abc", &[], "Kitchen.pdf", 0.0, cc_clock::Zone::Utc).unwrap_err(), t("export.empty"));
        m.add_item("Outlets", "count");
        let (name, bytes) = m.export_file("csv", "abc", &[], "Kitchen.pdf", 0.0, cc_clock::Zone::Utc).unwrap();
        assert!(name.ends_with(".csv"));
        assert!(bytes.starts_with("\u{feff}".as_bytes()));
        let (name, bytes) = m.export_file("xlsx", "abc", &[], "Kitchen.pdf", 0.0, cc_clock::Zone::Utc).unwrap();
        assert!(name.ends_with(".xlsx") && bytes.starts_with(b"PK"));
        let (name, bytes) = m.export_file("project", "abc", &[], "Kitchen.pdf", 0.0, cc_clock::Zone::Utc).unwrap();
        assert!(name.ends_with(".takeoff.json"));
        let back: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back["items"].as_array().map(Vec::len), Some(1));
        assert!(m.export_file("nope", "abc", &[], "", 0.0, cc_clock::Zone::Utc).is_err());
    }

    #[test]
    fn counts_and_lengths_on_a_scaled_sheet() {
        let mut m = TakeoffMode::new("abc");
        let sheet = m.sheet(0);
        // 1/4" = 1'-0": 72 PDF units an inch, so 18 units are a foot
        m.set_preset(&sheet, "a1/4", 1.0);
        let fpu = m.ft_per_unit(&sheet).unwrap();
        assert!((fpu * 18.0 - 1.0).abs() < 1e-12, "{fpu}");
        m.add_item("Outlets", "count");
        m.click(&sheet, [10.0, 10.0]);
        m.click(&sheet, [20.0, 10.0]);
        let walls = m.add_item("Walls", "linear").unwrap();
        m.click(&sheet, [0.0, 0.0]);
        m.click(&sheet, [180.0, 0.0]);
        m.finish(&sheet);
        let q = quantities(&m.tk.state, &QOpts::default());
        assert_eq!(m.tk.marks().len(), 3);
        assert!((q.get(&walls).unwrap().tot.length_ft - 10.0).abs() < 1e-9);
        assert!(m.tk.undo().is_some());
        assert_eq!(m.tk.marks().len(), 2);
    }
}
