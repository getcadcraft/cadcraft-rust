//! PartCraft's drawing sheet in the app (M13.9): partcraft.js's drawing mode — `enterDrawing`, `drawingRefresh`
//! (the mesh kernel's views and section, kernel_section.rs `mesh_views_section`; thread callouts), `composeShow`
//! (drawing.rs `compose_sheet` with the part's GD&T notes, hole table, title block and revisions), the sheet drawn
//! as sheetview.js draws it (paper, ink, fills, text, note boxes; fit, wheel zoom, pan), the tools (datum, feature
//! control frame, surface finish, detail view: click a view), note dragging, `renderDrawingProps` (the sheet panel:
//! paper, section A-A and its cut, detail B, hole table, title block, revisions; a note's own panel) and
//! `exportDrawing` (drawing.rs `to_pdf`). Every change is one undo step (`commitDrawing`, no rebuild). Commands
//! `part.drawing` (enter / back / pdf[@ms] / tool:<t> / click:x,y / sel:<id> / drag:<id>:dx,dy / delete) and
//! `part.drawing.set` (sheet / note fields).

use cc_part::{drawing as DR, gdt as G, jv, kernel_section as KS, part as P, threads as TH, vec as V};
use serde_json::Value;

use crate::partview::{Mode, PartView, tr, trv};

/// The part's views (key, view) and its thread notes.
pub type ViewsData = (Vec<(String, DR::View)>, Vec<Value>);

/// The drawing mode's state (partcraft.js `drawingState`).
#[derive(Default)]
pub struct DrawSt {
    /// the views of the part (key, view) and the thread notes, for the key they were made for
    pub data: Option<ViewsData>,
    pub key: String,
    /// the composed sheet (drawing.js display list, mm, y up)
    pub sheet: Option<Value>,
    pub tool: Option<String>,
    pub sel: Option<String>,
    pub hover: Option<String>,
    /// a note being dragged: id, dx, dy (sheet mm)
    pub drag: Option<(String, f64, f64)>,
    /// sheetview.js `view`: screen = (sheet − (x, y)) · k with y down on screen
    pub view: Option<(f64, f64, f64)>,
    pub user_moved: bool,
    /// text typed into the panel's fields, by key
    pub edits: std::collections::HashMap<String, String>,
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    jv::obj(pairs)
}

/// The drawing's own data (partcraft.js `drawingOf`).
pub fn drawing_of(part: &Value) -> Value {
    part.get("drawing")
        .filter(|d| d.is_object())
        .cloned()
        .unwrap_or_else(|| obj(vec![("section", Value::Null), ("detail", Value::Null), ("notes", Value::Array(vec![]))]))
}

/// A sheet view's name in words (partcraft.js `viewWord`).
pub fn view_word(k: &str) -> String {
    match k {
        "front" | "top" | "right" => tr(&format!("drawing.view.{k}")),
        "section" | "iso" | "detail" => tr(&format!("app.drawing.view_{k}")),
        _ => k.to_string(),
    }
}

/// `Number(v.toFixed(dp))`.
fn round_to(v: f64, dp: usize) -> f64 {
    cc_cad::js::str_to_number(&cc_part::js::to_fixed(v, dp))
}

fn ymd(ms: f64) -> String {
    crate::civilexport::iso_time(ms).chars().take(10).collect()
}

impl PartView {
    pub fn in_drawing(&self) -> bool {
        self.drawing.is_some()
    }

    /// The clock for dates (today; scripts and the parity kit pin it with `part.drawing=clock:<ms>`).
    fn today_ms(&self) -> f64 {
        self.clock_ms
            .unwrap_or_else(|| web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64))
    }

    /// partcraft.js `enterDrawing()`.
    pub fn enter_drawing(&mut self) {
        if self.mode == Mode::Sketch {
            self.exit_sketch();
        }
        if self.mode == Mode::Pick {
            self.end_pick();
        }
        if self.regen.mesh.is_none() {
            self.note = tr("app.drawing.build_first");
            return;
        }
        self.section_stop();
        self.measure_stop();
        self.drawing = Some(DrawSt::default());
        self.pick = None;
        self.multi.clear();
        self.sel_face = None;
        self.retint();
        self.drawing_refresh();
        if self.drawing.is_some() && self.note.is_empty() || self.drawing.as_ref().is_some_and(|d| d.data.is_some()) {
            self.note = tr("kernel.drawing.status");
        }
    }

    /// partcraft.js `leaveDrawing()`.
    pub fn leave_drawing(&mut self) {
        if self.drawing.take().is_some() {
            self.note = String::new();
        }
    }

    fn drawing_views(&self, d: &Value) -> Result<ViewsData, String> {
        let m = self.regen.raw_mesh.as_ref().ok_or_else(|| tr("app.drawing.no_solid"))?;
        let mesh = DR::DMesh { positions: m.positions.clone(), indices: m.indices.clone() };
        let sec = d.get("section").filter(|s| s.is_object());
        let of = sec.map(|s| jv::s(s, "of").to_string());
        let at = sec.and_then(|s| jv::as_num(s.get("at")));
        let views = KS::mesh_views_section(&mesh, true, of.as_deref().map(|o| (o, at)))?;
        let threads: Vec<(String, Value)> = self.regen.threads.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        Ok((views, TH::thread_notes(&threads)))
    }

    /// partcraft.js `drawingRefresh()`: new views when the features, the section or the resolution changed.
    pub fn drawing_refresh(&mut self) {
        let Some(st) = self.drawing.as_ref() else { return };
        let d = drawing_of(&self.part);
        let key = format!(
            "m{}|{}|{}",
            jv::stringify(self.part.get("features").unwrap_or(&Value::Null)),
            jv::stringify(d.get("section").unwrap_or(&Value::Null)),
            cc_part::js::string_of(self.part.get("resolution"))
        );
        if st.key != key {
            let data = self.drawing_views(&d);
            if let Some(st) = self.drawing.as_mut() {
                st.key = key;
                match data {
                    Ok(x) => st.data = Some(x),
                    Err(e) => {
                        st.data = None;
                        self.note = trv("app.drawing.views_failed", &[("error", e)]);
                    }
                }
            }
        }
        self.compose_show();
    }

    fn sheet_now(&self, drag: Option<&(String, f64, f64)>) -> Option<Value> {
        let st = self.drawing.as_ref()?;
        let (views, threads) = st.data.as_ref()?;
        self.compose(views, threads, drag).ok()
    }

    fn compose(&self, views: &[(String, DR::View)], threads: &[Value], drag: Option<&(String, f64, f64)>) -> Result<Value, String> {
        let mut d = drawing_of(&self.part);
        if let Some((id, dx, dy)) = drag
            && let Some(notes) = d.get_mut("notes").and_then(Value::as_array_mut)
        {
            for n in notes.iter_mut().filter(|n| jv::is_str(n.get("id"), id)) {
                let off = jv::arr(n.get("off"));
                let (ox, oy) = (off.first().and_then(Value::as_f64).unwrap_or(0.0), off.get(1).and_then(Value::as_f64).unwrap_or(0.0));
                jv::set(n, "off", jv::nums(&[ox + dx, oy + dy]));
            }
        }
        let faces = self.regen.faces.as_ref().and_then(Value::as_array).map(Vec::as_slice);
        let callouts = |fid: &str| self.regen.threads.get(fid).and_then(|th| th.get("callout")).and_then(Value::as_str).map(str::to_string);
        let holes = DR::part_holes(&self.part, self.regen.faces.is_some(), faces, &callouts);
        let o = DR::SheetOpts {
            name: jv::s(&self.part, "name").to_string(),
            date_ymd: ymd(self.today_ms()),
            paper: None,
            drawing: d,
            annotate: true,
            material: self.part.get("material").map(|m| jv::s(m, "name").to_string()).unwrap_or_default(),
            holes: holes.as_deref(),
        };
        DR::compose_sheet(views, false, threads, &o)
    }

    /// partcraft.js `composeShow()`.
    pub fn compose_show(&mut self) {
        let drag = self.drawing.as_ref().and_then(|s| s.drag.clone());
        let sheet = self.sheet_now(drag.as_ref());
        if let Some(st) = self.drawing.as_mut()
            && sheet.is_some()
        {
            st.sheet = sheet;
        }
    }

    /// partcraft.js `commitDrawing(label, fn, { refetch })`: one undo step, no rebuild.
    pub fn commit_drawing(&mut self, label: &str, refetch: bool, f: impl FnOnce(&mut Value)) {
        let before = self.part.clone();
        if !self.part.get("drawing").is_some_and(Value::is_object) {
            jv::set(
                &mut self.part,
                "drawing",
                obj(vec![("section", Value::Null), ("detail", Value::Null), ("notes", Value::Array(vec![]))]),
            );
        }
        if let Some(d) = self.part.get_mut("drawing") {
            f(d);
        }
        self.push_undo(label, before);
        self.dirty = true;
        if refetch {
            self.drawing_refresh();
        } else {
            self.compose_show();
        }
    }

    /// partcraft.js `drawingTool(tool)`.
    pub fn drawing_tool(&mut self, tool: &str) {
        let Some(st) = self.drawing.as_mut() else { return };
        st.tool = if st.tool.as_deref() == Some(tool) { None } else { Some(tool.to_string()) };
        st.sel = None;
        let on = st.tool.clone();
        self.compose_show();
        self.note = match on.as_deref() {
            Some("datum") => tr("app.drawing.click_datum"),
            Some("fcf") => tr("app.drawing.click_fcf"),
            Some("finish") => tr("app.drawing.click_finish"),
            Some("detail") => tr("app.drawing.click_detail"),
            _ => String::new(),
        };
    }

    /// partcraft.js `viewAt(x, y)`: the view under a sheet point and its view coordinates (snapped within 2.5 mm).
    fn view_at(&self, x: f64, y: f64) -> Option<(String, [f64; 2])> {
        let st = self.drawing.as_ref()?;
        let sh = st.sheet.as_ref()?;
        let place = sh.get("place").and_then(Value::as_object)?;
        for (k, p) in place {
            let b = p.get("box").unwrap_or(&Value::Null);
            let (u0, u1, v0, v1) = (jv::n(b, "u0"), jv::n(b, "u1"), jv::n(b, "v0"), jv::n(b, "v1"));
            let (px, py, s) = (jv::n(p, "x"), jv::n(p, "y"), jv::n(p, "s"));
            let (w, h) = ((u1 - u0) * s, (v1 - v0) * s);
            if x < px - 3.0 || x > px + w + 3.0 || y < py - 3.0 || y > py + h + 3.0 {
                continue;
            }
            let (mut u, mut v) = (u0 + (x - px) / s, v0 + (y - py) / s);
            let src =
                if k == "detail" { None } else { st.data.as_ref().and_then(|(vs, _)| vs.iter().find(|(n, _)| n == k)).map(|(_, v)| v) };
            if let Some(src) = src {
                let mut best = 2.5 / s;
                let mut at: Option<[f64; 2]> = None;
                for g in src.visible.iter().chain(src.cut.iter().flatten()) {
                    let (dx, dy) = (g[2] - g[0], g[3] - g[1]);
                    let l = dx * dx + dy * dy;
                    let t = if l > 0.0 { (((u - g[0]) * dx + (v - g[1]) * dy) / l).clamp(0.0, 1.0) } else { 0.0 };
                    let (qx, qy) = (g[0] + dx * t, g[1] + dy * t);
                    let dd = (u - qx).hypot(v - qy);
                    if dd < best {
                        best = dd;
                        at = Some([qx, qy]);
                    }
                }
                if let Some([a, b2]) = at {
                    u = a;
                    v = b2;
                }
            }
            return Some((k.clone(), [round_to(u, 4), round_to(v, 4)]));
        }
        None
    }

    /// partcraft.js `drawingClick(x, y)` (sheet mm).
    pub fn drawing_click(&mut self, x: f64, y: f64) {
        let Some(st) = self.drawing.as_mut() else { return };
        let Some(tool) = st.tool.clone() else {
            if st.sel.take().is_some() {
                self.compose_show();
            }
            return;
        };
        let Some((view, at)) = self.view_at(x, y) else {
            self.note = tr("app.drawing.click_on_view");
            return;
        };
        if tool == "detail" {
            if view == "detail" || view == "iso" {
                self.note = tr("app.drawing.detail_from");
                return;
            }
            let b = self
                .drawing
                .as_ref()
                .and_then(|s| s.sheet.as_ref())
                .and_then(|sh| sh.get("place"))
                .and_then(|p| p.get(&view))
                .and_then(|p| p.get("box"))
                .cloned()
                .unwrap_or(Value::Null);
            let r = round_to(cc_part::js::max(jv::n(&b, "u1") - jv::n(&b, "u0"), jv::n(&b, "v1") - jv::n(&b, "v0")) / 8.0, 2);
            let v2 = view.clone();
            self.commit_drawing(&tr("app.undo.detail_view"), false, |d| {
                jv::set(d, "detail", obj(vec![("of", v2.as_str().into()), ("at", jv::nums(&at)), ("r", jv::num(r)), ("scale", 2.into())]));
            });
            if let Some(st) = self.drawing.as_mut() {
                st.tool = None;
            }
            self.note = trv("app.drawing.detail_taken", &[("view", view_word(&view))]);
            return;
        }
        let d = drawing_of(&self.part);
        let notes = jv::arr(d.get("notes"));
        let mut k = 1;
        while notes.iter().any(|n| jv::is_str(n.get("id"), &format!("g{k}"))) {
            k += 1;
        }
        let mut props = obj(vec![("id", format!("g{k}").into()), ("view", view.as_str().into()), ("at", jv::nums(&at))]);
        if tool == "datum" {
            let used: Vec<String> = notes.iter().filter(|n| jv::s(n, "kind") == "datum").map(|n| jv::s(n, "letter").to_string()).collect();
            let letter = G::DATUM_LETTERS.chars().map(|c| c.to_string()).find(|l| !used.contains(l)).unwrap_or_else(|| "A".into());
            jv::set(&mut props, "letter", letter.into());
        }
        let note = match G::new_annotation(&tool, &props) {
            Ok(n) => n,
            Err(e) => {
                self.note = e;
                return;
            }
        };
        let id = jv::s(&note, "id").to_string();
        let (label, done) = match tool.as_str() {
            "datum" => ("app.undo.add_datum", "app.drawing.placed_datum"),
            "fcf" => ("app.undo.add_fcf", "app.drawing.placed_fcf"),
            _ => ("app.undo.add_finish", "app.drawing.placed_finish"),
        };
        self.commit_drawing(&tr(label), false, |d| {
            let mut l = jv::arr(d.get("notes")).to_vec();
            l.push(note);
            jv::set(d, "notes", Value::Array(l));
        });
        if let Some(st) = self.drawing.as_mut() {
            st.sel = Some(id);
            st.tool = None;
        }
        self.note = tr(done);
    }

    /// partcraft.js `deleteNote()`.
    pub fn delete_note(&mut self) {
        let Some(id) = self.drawing.as_ref().and_then(|s| s.sel.clone()) else { return };
        self.commit_drawing(&tr("app.undo.delete_note"), false, |d| {
            let l: Vec<Value> = jv::arr(d.get("notes")).iter().filter(|n| !jv::is_str(n.get("id"), &id)).cloned().collect();
            jv::set(d, "notes", Value::Array(l));
        });
        if let Some(st) = self.drawing.as_mut() {
            st.sel = None;
        }
        self.compose_show();
        self.note = tr("app.drawing.note_deleted");
    }

    /// sheetview.js `onNoteDrop`: the note moved by (dx, dy) mm.
    pub fn drop_note(&mut self, id: &str, dx: f64, dy: f64) {
        if let Some(st) = self.drawing.as_mut() {
            st.drag = None;
        }
        let id = id.to_string();
        self.commit_drawing(&tr("app.undo.move_note"), false, |d| {
            if let Some(notes) = d.get_mut("notes").and_then(Value::as_array_mut) {
                for n in notes.iter_mut().filter(|n| jv::is_str(n.get("id"), &id)) {
                    let off = jv::arr(n.get("off"));
                    let (ox, oy) = (off.first().and_then(Value::as_f64).unwrap_or(0.0), off.get(1).and_then(Value::as_f64).unwrap_or(0.0));
                    jv::set(n, "off", jv::nums(&[round_to(ox + dx, 2), round_to(oy + dy, 2)]));
                }
            }
        });
    }

    /// partcraft.js keydown in drawing mode: Escape (the tool, the selection, then the sheet), Delete / Backspace, the
    /// arrows nudge the selected note by 1 mm (one undo step each), F fits the sheet.
    pub fn drawing_key(&mut self, key: &str) {
        let Some(st) = self.drawing.as_mut() else { return };
        match key {
            "Escape" => {
                if let Some(tool) = st.tool.clone() {
                    self.drawing_tool(&tool);
                } else if st.sel.take().is_some() {
                    self.compose_show();
                } else {
                    self.leave_drawing();
                }
            }
            "Delete" | "Backspace" if st.sel.is_some() => self.delete_note(),
            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" if st.sel.is_some() => {
                let id = st.sel.clone().unwrap_or_default();
                let (dx, dy) = match key {
                    "ArrowLeft" => (-1.0, 0.0),
                    "ArrowRight" => (1.0, 0.0),
                    "ArrowUp" => (0.0, 1.0),
                    _ => (0.0, -1.0),
                };
                self.commit_drawing(&tr("app.undo.nudge_note"), false, |d| {
                    if let Some(notes) = d.get_mut("notes").and_then(Value::as_array_mut) {
                        for n in notes.iter_mut().filter(|n| jv::is_str(n.get("id"), &id)) {
                            let off = jv::arr(n.get("off"));
                            let (ox, oy) =
                                (off.first().and_then(Value::as_f64).unwrap_or(0.0), off.get(1).and_then(Value::as_f64).unwrap_or(0.0));
                            jv::set(n, "off", jv::nums(&[round_to(ox + dx, 2), round_to(oy + dy, 2)]));
                        }
                    }
                });
            }
            "f" | "F" => {
                st.user_moved = false;
                st.view = None;
            }
            _ => {}
        }
    }

    /// partcraft.js `sectionCentre(of)`.
    fn section_centre(&self, of: &str) -> f64 {
        let Some(b) = self.regen.stats.as_ref().and_then(|s| s.get("bbox")) else { return 0.0 };
        let (mn, mx) =
            (crate::partview::p3(b.get("min").unwrap_or(&Value::Null)), crate::partview::p3(b.get("max").unwrap_or(&Value::Null)));
        let dir = match of {
            "front" => [0.0, 0.0, 1.0],
            "top" => [0.0, 1.0, 0.0],
            _ => [1.0, 0.0, 0.0],
        };
        round_to(V::dot(dir, [(mn[0] + mx[0]) / 2.0, (mn[1] + mx[1]) / 2.0, (mn[2] + mx[2]) / 2.0]), 4)
    }

    /// `part.drawing.set=<field>=<value>`: the sheet panel (no note selected) or the selected note's panel.
    pub fn drawing_set(&mut self, spec: &str) -> Result<(), String> {
        let (field, value) = spec.split_once('=').unwrap_or((spec, ""));
        let value = value.trim().to_string();
        let st = self.drawing.as_ref().ok_or("part.drawing.set: open the drawing first")?;
        let num = |s: &str| -> Option<f64> {
            cc_part::js::str_num(s).is_finite().then(|| cc_part::js::str_num(s)).filter(|_| !s.trim().is_empty())
        };
        let d = drawing_of(&self.part);
        // the selected note's fields (renderDrawingProps: a selection whose note is gone shows the sheet's)
        if let Some(id) = st.sel.clone().filter(|id| jv::arr(d.get("notes")).iter().any(|q| jv::is_str(q.get("id"), id))) {
            return self.note_set(&id, field, &value);
        }
        match field {
            "sheet" => {
                if !P::SHEET_KEYS.contains(&value.as_str()) {
                    return Err(format!("part.drawing.set: sheet is one of {}", P::SHEET_KEYS.join(", ")));
                }
                self.commit_drawing(&tr("drawing.undo.sheet"), false, |dd| {
                    if value == "a3" {
                        jv::del(dd, "sheet");
                    } else {
                        jv::set(dd, "sheet", value.as_str().into());
                    }
                });
            }
            "section" => {
                if !matches!(value.as_str(), "" | "front" | "top" | "right") {
                    return Err("part.drawing.set: section is front, top, right or empty".into());
                }
                self.commit_drawing(&tr("app.undo.section_view"), true, |dd| {
                    jv::set(dd, "section", if value.is_empty() { Value::Null } else { obj(vec![("of", value.as_str().into())]) });
                    if value.is_empty() && dd.get("detail").is_some_and(|x| jv::is_str(x.get("of"), "section")) {
                        jv::set(dd, "detail", Value::Null);
                    }
                });
            }
            "at" => {
                if !d.get("section").is_some_and(Value::is_object) {
                    return Err("part.drawing.set: no section".into());
                }
                let Some(v) = num(&value) else {
                    self.note = tr("app.input.number");
                    return Ok(());
                };
                self.commit_drawing(&tr("app.undo.section_position"), true, |dd| {
                    if let Some(s) = dd.get_mut("section") {
                        jv::set(s, "at", jv::num(v));
                    }
                });
            }
            "detail" => {
                if value.is_empty() {
                    self.commit_drawing(&tr("app.undo.detail_view"), false, |dd| jv::set(dd, "detail", Value::Null));
                } else {
                    self.drawing_tool("detail");
                    self.note = trv("app.drawing.click_detail_centre", &[("view", view_word(&value))]);
                }
            }
            "detail_r" | "detail_scale" => {
                if !d.get("detail").is_some_and(Value::is_object) {
                    return Err("part.drawing.set: no detail view".into());
                }
                if field == "detail_r" {
                    let Some(v) = num(&value).filter(|v| *v >= 0.001) else {
                        self.note = trv("app.input.number_from", &[("min", "0.001".into())]);
                        return Ok(());
                    };
                    self.commit_drawing(&tr("app.undo.detail_size"), false, |dd| {
                        if let Some(x) = dd.get_mut("detail") {
                            jv::set(x, "r", jv::num(v));
                        }
                    });
                } else {
                    let v = cc_part::js::str_num(&value);
                    if !P::DETAIL_SCALES.contains(&v) {
                        return Err("part.drawing.set: detail_scale is 2, 4, 5 or 10".into());
                    }
                    self.commit_drawing(&tr("app.drawing.detail_scale"), false, |dd| {
                        if let Some(x) = dd.get_mut("detail") {
                            jv::set(x, "scale", jv::num(v));
                        }
                    });
                }
            }
            "holes" => {
                if !matches!(value.as_str(), "" | "front" | "top" | "right") {
                    return Err("part.drawing.set: holes is front, top, right or empty".into());
                }
                self.commit_drawing(&tr("drawing.holes.undo"), false, |dd| {
                    if value.is_empty() {
                        jv::del(dd, "holeTable");
                    } else {
                        jv::set(dd, "holeTable", obj(vec![("view", value.as_str().into())]));
                    }
                });
            }
            "rev_add" => {
                let revs = jv::arr(d.get("revisions")).to_vec();
                if revs.len() >= P::MAX_REVISIONS {
                    return Ok(());
                }
                let rev = DR::next_revision(&revs);
                let date = ymd(self.today_ms());
                self.commit_drawing(&tr("drawing.undo.revision_add"), false, |dd| {
                    let mut l = jv::arr(dd.get("revisions")).to_vec();
                    l.push(obj(vec![("rev", rev.into()), ("desc", "".into()), ("date", date.into()), ("by", "".into())]));
                    jv::set(dd, "revisions", Value::Array(l));
                });
            }
            f if f.starts_with("rev_delete:") => {
                let i: usize = f.trim_start_matches("rev_delete:").parse().map_err(|_| format!("part.drawing.set: {f}"))?;
                self.commit_drawing(&tr("drawing.undo.revision_delete"), false, |dd| {
                    let mut l = jv::arr(dd.get("revisions")).to_vec();
                    if i < l.len() {
                        l.remove(i);
                    }
                    if l.is_empty() {
                        jv::del(dd, "revisions");
                    } else {
                        jv::set(dd, "revisions", Value::Array(l));
                    }
                });
            }
            f if f.starts_with("rev:") => {
                // rev:<i>:<rev|desc|date|by>
                let mut it = f.splitn(3, ':').skip(1);
                let i: usize = it.next().and_then(|x| x.parse().ok()).ok_or_else(|| format!("part.drawing.set: {f}"))?;
                let k = it
                    .next()
                    .filter(|k| matches!(*k, "rev" | "desc" | "date" | "by"))
                    .ok_or_else(|| format!("part.drawing.set: {f}"))?
                    .to_string();
                let max = match k.as_str() {
                    "rev" => 8,
                    "desc" => 120,
                    "date" => 10,
                    _ => 40,
                };
                let v = clean(&value, max);
                if k == "date" && !v.is_empty() && !is_ymd(&v) {
                    self.note = tr("drawing.panel.date_bad");
                    return Ok(());
                }
                self.commit_drawing(&tr("drawing.undo.revision"), false, |dd| {
                    if let Some(r) = dd.get_mut("revisions").and_then(Value::as_array_mut).and_then(|l| l.get_mut(i)) {
                        jv::set(r, &k, v.into());
                    }
                });
            }
            f if f.starts_with("title:") => {
                let k = f.trim_start_matches("title:").to_string();
                let max = P::TITLE_FIELDS
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, m)| *m)
                    .ok_or_else(|| format!("part.drawing.set: no title field {k}"))?;
                let v = clean(&value, max);
                if k == "date" && !v.is_empty() && !is_ymd(&v) {
                    self.note = tr("drawing.panel.date_bad");
                    return Ok(());
                }
                self.commit_drawing(&tr("drawing.undo.title"), false, |dd| {
                    let mut n = dd.get("title").and_then(Value::as_object).cloned().unwrap_or_default();
                    if v.is_empty() {
                        n.remove(&k);
                    } else {
                        n.insert(k.clone(), v.into());
                    }
                    if n.is_empty() {
                        jv::del(dd, "title");
                    } else {
                        jv::set(dd, "title", Value::Object(n));
                    }
                });
            }
            other => return Err(format!("part.drawing.set: no field {other}")),
        }
        Ok(())
    }

    /// The selected note's panel fields (partcraft.js `renderDrawingProps` with `n`).
    fn note_set(&mut self, id: &str, field: &str, value: &str) -> Result<(), String> {
        let d = drawing_of(&self.part);
        let n =
            jv::arr(d.get("notes")).iter().find(|q| jv::is_str(q.get("id"), id)).cloned().ok_or("part.drawing.set: the note is gone")?;
        let id = id.to_string();
        let set = |pv: &mut PartView, label: &str, f: &dyn Fn(&mut Value)| {
            pv.commit_drawing(&tr(label), false, |dd| {
                if let Some(q) =
                    dd.get_mut("notes").and_then(Value::as_array_mut).and_then(|l| l.iter_mut().find(|q| jv::is_str(q.get("id"), &id)))
                {
                    f(q);
                }
            });
        };
        let ch = G::CHARACTERISTICS.iter().find(|c| c.key == jv::s(&n, "char"));
        let letter_ok = |v: &str| v.chars().count() == 1 && G::DATUM_LETTERS.contains(v);
        let v = value.to_string();
        match (jv::s(&n, "kind"), field) {
            ("datum", "letter") => {
                if !letter_ok(value) {
                    return Err(format!("part.drawing.set: a datum letter, not {value}"));
                }
                set(self, "app.undo.datum_letter", &|q| jv::set(q, "letter", v.as_str().into()));
            }
            ("fcf", "char") => {
                let c = G::CHARACTERISTICS
                    .iter()
                    .find(|c| c.key == value)
                    .ok_or_else(|| format!("part.drawing.set: no characteristic {value}"))?;
                set(self, "app.undo.characteristic", &|q| {
                    jv::set(q, "char", c.key.into());
                    if !c.dia {
                        jv::set(q, "dia", false.into());
                    }
                    if !c.md {
                        jv::set(q, "mod", "".into());
                        let ds: Vec<Value> = jv::arr(q.get("datums"))
                            .iter()
                            .map(|x| obj(vec![("letter", x.get("letter").cloned().unwrap_or(Value::Null)), ("mod", "".into())]))
                            .collect();
                        jv::set(q, "datums", Value::Array(ds));
                    }
                    if c.datums == "none" {
                        jv::set(q, "datums", Value::Array(vec![]));
                    } else if c.datums == "required" && jv::arr(q.get("datums")).is_empty() {
                        jv::set(q, "datums", Value::Array(vec![obj(vec![("letter", "A".into()), ("mod", "".into())])]));
                    }
                });
            }
            ("fcf", "tol") | ("finish", "ra") => {
                let x = cc_part::js::str_num(value);
                if value.trim().is_empty() || !x.is_finite() || x < 0.000001 {
                    self.note = trv("app.input.number_from", &[("min", "0.000001".into())]);
                    return Ok(());
                }
                let (k, label) = if field == "tol" { ("tol", "app.undo.tolerance_value") } else { ("ra", "app.undo.roughness") };
                set(self, label, &|q| jv::set(q, k, jv::num(x)));
            }
            ("fcf", "dia") if ch.is_some_and(|c| c.dia) => {
                let on = matches!(value, "true" | "1" | "on" | "yes");
                set(self, "app.undo.dia_zone", &|q| jv::set(q, "dia", on.into()));
            }
            ("fcf", "mod") if ch.is_some_and(|c| c.md) => {
                if !matches!(value, "" | "M" | "L") {
                    return Err("part.drawing.set: mod is '', M or L".into());
                }
                set(self, "app.undo.material_condition", &|q| jv::set(q, "mod", v.as_str().into()));
            }
            ("fcf", f) if f.starts_with("datum:") || f.starts_with("dmod:") => {
                let i: usize =
                    f.split(':').nth(1).and_then(|x| x.parse().ok()).filter(|i| *i < 3).ok_or_else(|| format!("part.drawing.set: {f}"))?;
                if f.starts_with("datum:") {
                    if !value.is_empty() && !letter_ok(value) {
                        return Err(format!("part.drawing.set: a datum letter, not {value}"));
                    }
                    set(self, "app.undo.datum_reference", &|q| {
                        let mut list: Vec<Value> = jv::arr(q.get("datums")).iter().take(3).cloned().collect();
                        if v.is_empty() {
                            list.truncate(i);
                        } else if let Some(x) = list.get_mut(i) {
                            jv::set(x, "letter", v.as_str().into());
                        } else if i == list.len() {
                            list.push(obj(vec![("letter", v.as_str().into()), ("mod", "".into())]));
                        }
                        jv::set(q, "datums", Value::Array(list));
                    });
                } else {
                    if !matches!(value, "" | "M" | "L") {
                        return Err("part.drawing.set: a datum modifier is '', M or L".into());
                    }
                    set(self, "app.undo.datum_modifier", &|q| {
                        if let Some(x) = q.get_mut("datums").and_then(Value::as_array_mut).and_then(|l| l.get_mut(i)) {
                            jv::set(x, "mod", v.as_str().into());
                        }
                    });
                }
            }
            ("finish", "process") => {
                if !matches!(value, "any" | "removal" | "noRemoval") {
                    return Err("part.drawing.set: process is any, removal or noRemoval".into());
                }
                set(self, "app.undo.finish_process", &|q| jv::set(q, "process", v.as_str().into()));
            }
            ("finish", "note") => {
                let c: String = value.chars().filter(|c| (*c as u32) >= 0x20).take(40).collect();
                set(self, "app.undo.finish_note", &|q| jv::set(q, "note", c.as_str().into()));
            }
            (k, f) => return Err(format!("part.drawing.set: a {k} note has no field {f}")),
        }
        Ok(())
    }

    /// partcraft.js `exportDrawing()`: the sheet as a PDF → (file, bytes, words).
    pub fn export_drawing(&mut self, now_ms: f64) -> Result<Option<(String, Vec<u8>, String)>, String> {
        if self.mode == Mode::Sketch {
            self.exit_sketch();
        }
        if self.mode == Mode::Pick {
            self.end_pick();
        }
        if self.regen.mesh.is_none() {
            self.note = tr("app.drawing.no_solid");
            return Ok(None);
        }
        let saved = self.clock_ms;
        self.clock_ms = Some(now_ms);
        let sheet = match self.drawing.as_ref().filter(|s| s.drag.is_none()).and_then(|_| self.sheet_now(None)) {
            Some(s) => Ok(s),
            None => self.drawing_views(&drawing_of(&self.part)).and_then(|(v, th)| self.compose(&v, &th, None)),
        };
        self.clock_ms = saved;
        let sheet = match sheet {
            Ok(s) => s,
            Err(e) => {
                self.note = trv("app.drawing.failed", &[("error", e)]);
                return Ok(None);
            }
        };
        let date = crate::civilexport::iso_time(now_ms);
        let pdf_date = format!("D:{}Z", date.chars().filter(char::is_ascii_digit).take(14).collect::<String>());
        let (bytes, _) = DR::to_pdf(&sheet, &pdf_date);
        let file = format!("{} - drawing.pdf", self.file_base());
        let mut views = vec![tr("app.drawing.v_front"), tr("app.drawing.v_top"), tr("app.drawing.v_right")];
        if let Some(o) = sheet.get("views").and_then(Value::as_object) {
            for k in o.keys().filter(|k| matches!(k.as_str(), "section" | "detail" | "iso")) {
                views.push(tr(&format!("app.drawing.v_{k}")));
            }
        }
        let nn = jv::arr(sheet.get("notes")).iter().filter(|q| jv::truthy(q.get("box"))).count();
        let words = trv(
            "app.drawing.exported",
            &[
                ("scale", DR::scale_label(jv::n(&sheet, "scale"))),
                ("views", views.join(", ")),
                ("notes", if nn > 0 { trv("app.drawing.gdt_notes", &[("n", nn.to_string())]) } else { String::new() }),
                ("exact", String::new()),
            ],
        );
        Ok(Some((file, bytes, words)))
    }
}

fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| (*c as u32) >= 0x20 && *c != '\u{7f}').collect::<String>().trim().chars().take(max).collect()
}

fn is_ymd(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

// ---------------------------------------------------------------------------------------------- the window

use crate::partpanel::{Cmd, cmd};
use crate::theme::Tokens;
use egui::{Pos2, Rect, RichText, Stroke};

/// sheetview.js: the sheet on screen (paper, ink, fills, text, note boxes), fit / wheel zoom / middle or right drag pan,
/// clicks in sheet mm, notes dragged.
pub fn sheet_ui(ui: &mut egui::Ui, rect: Rect, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, t.canvas_bg);
    let Some(st) = pv.drawing.as_mut() else { return };
    let Some(sheet) = st.sheet.clone() else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            tr("app.drawing.making_views"),
            egui::FontId::proportional(14.0),
            t.text_dim,
        );
        return;
    };
    let (sw, sh) = (jv::n(&sheet, "w"), jv::n(&sheet, "h"));
    let (w, h) = (f64::from(rect.width()).max(1.0), f64::from(rect.height()).max(1.0));
    if st.view.is_none() || !st.user_moved {
        let k = (w / (sw + 10.0)).min(h / (sh + 10.0));
        st.view = Some((sw / 2.0 - w / 2.0 / k, sh / 2.0 + h / 2.0 / k, k));
    }
    let (vx, vy, k) = st.view.unwrap_or((0.0, 0.0, 1.0));
    let to = |x: f64, y: f64| Pos2::new(rect.min.x + ((x - vx) * k) as f32, rect.min.y + ((vy - y) * k) as f32);
    let to_sheet = |p: Pos2| (vx + f64::from(p.x - rect.min.x) / k, vy - f64::from(p.y - rect.min.y) / k);
    // zoom and pan
    if let Some(p) = resp.hover_pos() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let f = if scroll > 0.0 { 1.15 } else { 1.0 / 1.15 };
            let (sx, sy) = to_sheet(p);
            let nk = (k * f).clamp(0.2, 80.0);
            st.view = Some((sx - f64::from(p.x - rect.min.x) / nk, sy + f64::from(p.y - rect.min.y) / nk, nk));
            st.user_moved = true;
        }
    }
    if resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary) {
        let d = resp.drag_delta();
        if let Some((x, y, kk)) = st.view {
            st.view = Some((x - f64::from(d.x) / kk, y + f64::from(d.y) / kk, kk));
            st.user_moved = true;
        }
    }
    let paper = egui::Color32::from_rgb(0xfb, 0xfb, 0xf8);
    let ink = egui::Color32::from_rgb(0x1c, 0x1d, 0x20);
    painter.rect_filled(Rect::from_two_pos(to(0.0, 0.0), to(sw, sh)), 0.0, paper);
    for s in jv::arr(sheet.get("strokes")) {
        let width = (jv::n(s, "width").max(0.13) * k) as f32;
        let dash: Vec<f32> = jv::arr(s.get("dash")).iter().filter_map(Value::as_f64).map(|d| (d * k) as f32).collect();
        for g in jv::arr(s.get("segs")) {
            let q = jv::arr(Some(g));
            let n = |i: usize| q.get(i).and_then(Value::as_f64).unwrap_or(0.0);
            let (a, b) = (to(n(0), n(1)), to(n(2), n(3)));
            if let (Some(on), Some(off)) = (dash.first(), dash.get(1)) {
                painter.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(width, ink), on.max(0.5), off.max(0.5)));
            } else {
                painter.line_segment([a, b], Stroke::new(width, ink));
            }
        }
    }
    for f in jv::arr(sheet.get("fills")) {
        let pts: Vec<Pos2> = jv::arr(f.get("pts"))
            .iter()
            .map(|p| to(p.get(0).and_then(Value::as_f64).unwrap_or(0.0), p.get(1).and_then(Value::as_f64).unwrap_or(0.0)))
            .collect();
        if pts.len() >= 3 {
            painter.add(egui::Shape::convex_polygon(pts, ink, Stroke::NONE));
        }
    }
    for tx in jv::arr(sheet.get("texts")) {
        let size = (jv::n(tx, "size") * k * 1.25) as f32;
        if size < 2.0 {
            continue;
        }
        let align = match jv::s(tx, "anchor") {
            "middle" => egui::Align2::CENTER_BOTTOM,
            "end" => egui::Align2::RIGHT_BOTTOM,
            _ => egui::Align2::LEFT_BOTTOM,
        };
        let font = if jv::truthy(tx.get("bold")) {
            egui::FontId::new(size, egui::FontFamily::Proportional)
        } else {
            egui::FontId::proportional(size)
        };
        let rot = jv::n(tx, "rot");
        let at = to(jv::n(tx, "x"), jv::n(tx, "y"));
        if rot.abs() > 1e-9 {
            let galley = painter.layout_no_wrap(jv::s(tx, "text").to_string(), font, ink);
            let mut shape = egui::epaint::TextShape::new(at - egui::vec2(0.0, galley.size().y), galley, ink);
            shape.angle = (-rot.to_radians()) as f32;
            painter.add(shape);
        } else {
            painter.text(at, align, jv::s(tx, "text"), font, ink);
        }
    }
    let sel = st.sel.clone();
    let hover = st.hover.clone();
    for nt in jv::arr(sheet.get("notes")) {
        let b = jv::arr(nt.get("box"));
        if b.len() < 4 {
            continue;
        }
        let g = |i: usize| b.get(i).and_then(Value::as_f64).unwrap_or(0.0);
        let r = Rect::from_two_pos(to(g(0) - 1.0, g(1) - 1.0), to(g(2) + 1.0, g(3) + 1.0));
        let id = jv::s(nt, "id");
        let c = if sel.as_deref() == Some(id) {
            t.accent
        } else if hover.as_deref() == Some(id) {
            t.accent.gamma_multiply(0.6)
        } else {
            egui::Color32::TRANSPARENT
        };
        painter.rect_stroke(r, 0.0, Stroke::new(1.2, c), egui::StrokeKind::Outside);
    }
    // notes under the pointer, dragged; clicks to the tools
    let note_at = |x: f64, y: f64| -> Option<String> {
        jv::arr(sheet.get("notes")).iter().rev().find_map(|nt| {
            let b = jv::arr(nt.get("box"));
            let g = |i: usize| b.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN);
            (b.len() >= 4 && x >= g(0) - 1.5 && x <= g(2) + 1.5 && y >= g(1) - 1.5 && y <= g(3) + 1.5).then(|| jv::s(nt, "id").to_string())
        })
    };
    if let Some(p) = resp.hover_pos() {
        let (x, y) = to_sheet(p);
        let h = note_at(x, y);
        if h != st.hover {
            st.hover = h;
        }
    }
    if resp.drag_started_by(egui::PointerButton::Primary)
        && let Some(o) = ui.input(|i| i.pointer.press_origin())
    {
        let (x, y) = to_sheet(o);
        if let Some(id) = note_at(x, y) {
            st.sel = Some(id.clone());
            st.tool = None;
            st.drag = Some((id, 0.0, 0.0));
        }
    }
    if resp.dragged_by(egui::PointerButton::Primary)
        && let (Some((id, _, _)), Some(o), Some(p)) = (st.drag.clone(), ui.input(|i| i.pointer.press_origin()), resp.interact_pointer_pos())
    {
        let (dx, dy) = (f64::from(p.x - o.x) / k, -f64::from(p.y - o.y) / k);
        st.drag = Some((id, dx, dy));
        pv.compose_show();
    }
    if resp.drag_stopped_by(egui::PointerButton::Primary)
        && let Some((id, dx, dy)) = pv.drawing.as_ref().and_then(|s| s.drag.clone())
    {
        *act = cmd("part.drawing", format!("drag:{id}:{},{}", round_to(dx, 2), round_to(dy, 2)));
    } else if resp.clicked_by(egui::PointerButton::Primary)
        && let Some(p) = resp.interact_pointer_pos()
    {
        let (x, y) = to_sheet(p);
        match note_at(x, y) {
            Some(id) if pv.drawing.as_ref().is_some_and(|s| s.tool.is_none()) => *act = cmd("part.drawing", format!("sel:{id}")),
            _ => *act = cmd("part.drawing", format!("click:{},{}", round_to(x, 4), round_to(y, 4))),
        }
    }
    // the sheet's keys (partcraft.js keydown), while no field has the focus
    if ui.ctx().memory(|m| m.focused().is_none()) {
        let key = ui.input(|i| {
            [
                (egui::Key::Escape, "Escape"),
                (egui::Key::Delete, "Delete"),
                (egui::Key::Backspace, "Backspace"),
                (egui::Key::ArrowLeft, "ArrowLeft"),
                (egui::Key::ArrowRight, "ArrowRight"),
                (egui::Key::ArrowUp, "ArrowUp"),
                (egui::Key::ArrowDown, "ArrowDown"),
                (egui::Key::F, "f"),
            ]
            .into_iter()
            .find(|(k, _)| i.key_pressed(*k) && !i.modifiers.shift && !i.modifiers.command && !i.modifiers.alt)
            .map(|(_, n)| n)
        });
        if let Some(k) = key {
            *act = cmd("part.drawing", format!("key:{k}"));
        }
    }
}

fn set(act: &mut Cmd, field: &str, v: impl Into<String>) {
    *act = cmd("part.drawing.set", format!("{field}={}", v.into()));
}

fn combo(ui: &mut egui::Ui, id: &str, label: &str, cur: &str, opts: &[(String, String)], field: &str, act: &mut Cmd) {
    ui.label(label);
    let shown = opts.iter().find(|(v, _)| v == cur).map_or_else(|| cur.to_string(), |(_, l)| l.clone());
    egui::ComboBox::new(id, "").selected_text(shown).width(150.0).show_ui(ui, |ui| {
        for (v, l) in opts {
            if ui.selectable_label(v == cur, l).clicked() && v != cur {
                set(act, field, v.clone());
            }
        }
    });
    ui.end_row();
}

#[allow(clippy::too_many_arguments)]
fn text_row(ui: &mut egui::Ui, st: &mut DrawSt, key: &str, label: &str, shown: &str, hint: &str, field: &str, act: &mut Cmd) {
    if !label.is_empty() {
        ui.label(label);
    }
    let mut text = st.edits.get(key).cloned().unwrap_or_else(|| shown.to_string());
    let r = ui.add_sized([150.0, 26.0], crate::shell::field(&mut text).hint_text(hint));
    if r.has_focus() {
        st.edits.insert(key.to_string(), text.clone());
    }
    if r.lost_focus() {
        st.edits.remove(key);
        if text != shown && !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            set(act, field, text);
        }
    }
    if !label.is_empty() {
        ui.end_row();
    }
}

/// partcraft.js `renderDrawingProps()`.
pub fn drawing_panel(ui: &mut egui::Ui, pv: &mut PartView, t: &Tokens, act: &mut Cmd) {
    let d = drawing_of(&pv.part);
    let sel = pv.drawing.as_ref().and_then(|s| s.sel.clone());
    let note = sel.as_deref().and_then(|id| jv::arr(d.get("notes")).iter().find(|q| jv::is_str(q.get("id"), id)).cloned());
    if let Some(n) = note {
        let kind = jv::s(&n, "kind").to_string();
        let head = match kind.as_str() {
            "datum" => trv("app.gdt.datum_head", &[("letter", jv::s(&n, "letter").to_string())]),
            "fcf" => tr("app.gdt.fcf_head"),
            _ => tr("app.gdt.finish_head"),
        };
        ui.label(RichText::new(head).strong().family(crate::theme::bold_family()).color(t.text_strong));
        if let Some(bad) = G::validate(Some(&n)) {
            ui.label(RichText::new(bad).small().color(t.text_strong));
        }
        let letters: Vec<(String, String)> = G::DATUM_LETTERS.chars().map(|c| (c.to_string(), c.to_string())).collect();
        egui::Grid::new("pc-dr-note").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| match kind.as_str() {
            "datum" => combo(ui, "pc-dr-letter", &tr("app.gdt.letter"), jv::s(&n, "letter"), &letters, "letter", act),
            "fcf" => {
                let chars: Vec<(String, String)> = G::CHARACTERISTICS.iter().map(|c| (c.key.to_string(), tr(c.label_key))).collect();
                combo(ui, "pc-dr-char", &tr("app.gdt.tolerance"), jv::s(&n, "char"), &chars, "char", act);
                let c = G::CHARACTERISTICS.iter().find(|c| c.key == jv::s(&n, "char"));
                if let Some(st) = pv.drawing.as_mut() {
                    text_row(ui, st, "tol", &tr("app.gdt.zone_mm"), &cc_part::js::num_str(jv::n(&n, "tol")), "", "tol", act);
                }
                if c.is_some_and(|c| c.dia) {
                    ui.label("");
                    let mut on = jv::truthy(n.get("dia"));
                    if ui.checkbox(&mut on, tr("app.gdt.dia_zone")).changed() {
                        set(act, "dia", on.to_string());
                    }
                    ui.end_row();
                }
                if c.is_some_and(|c| c.md) {
                    let mods = vec![(String::new(), tr("app.gdt.rfs")), ("M".into(), "MMC (Ⓜ)".into()), ("L".into(), "LMC (Ⓛ)".into())];
                    combo(ui, "pc-dr-mod", &tr("app.gdt.material"), jv::s(&n, "mod"), &mods, "mod", act);
                }
                if c.is_some_and(|c| c.datums != "none") {
                    let ds = jv::arr(n.get("datums")).to_vec();
                    let mut opts = vec![(String::new(), "—".to_string())];
                    opts.extend(letters.iter().cloned());
                    for (i, k) in ["app.gdt.primary", "app.gdt.secondary", "app.gdt.tertiary"].iter().enumerate() {
                        let cur = ds.get(i).map(|x| jv::s(x, "letter").to_string()).unwrap_or_default();
                        combo(ui, &format!("pc-dr-datum{i}"), &tr(k), &cur, &opts, &format!("datum:{i}"), act);
                        if let Some(x) = ds.get(i).filter(|_| c.is_some_and(|c| c.md)) {
                            let mods = vec![
                                (String::new(), tr("app.gdt.mod_none")),
                                ("M".into(), "MMB (Ⓜ)".into()),
                                ("L".into(), "LMB (Ⓛ)".into()),
                            ];
                            combo(
                                ui,
                                &format!("pc-dr-dmod{i}"),
                                &tr("app.gdt.modifier"),
                                jv::s(x, "mod"),
                                &mods,
                                &format!("dmod:{i}"),
                                act,
                            );
                        }
                    }
                }
            }
            _ => {
                if let Some(st) = pv.drawing.as_mut() {
                    text_row(ui, st, "ra", &tr("app.gdt.ra"), &cc_part::js::num_str(jv::n(&n, "ra")), "", "ra", act);
                }
                let procs: Vec<(String, String)> = [
                    ("any", "model.gdt.process.any"),
                    ("removal", "model.gdt.process.removal"),
                    ("noRemoval", "model.gdt.process.no_removal"),
                ]
                .iter()
                .map(|(k, l)| ((*k).to_string(), tr(l)))
                .collect();
                combo(ui, "pc-dr-proc", &tr("app.gdt.process"), jv::s(&n, "process"), &procs, "process", act);
                if let Some(st) = pv.drawing.as_mut() {
                    text_row(ui, st, "note", &tr("app.gdt.note"), jv::s(&n, "note"), "", "note", act);
                }
            }
        });
        if matches!(jv::s(&n, "char"), "concentricity" | "symmetry") && kind == "fcf" {
            crate::shell::note(ui, t, &tr("app.gdt.legacy"));
        }
        egui::Grid::new("pc-dr-facts").num_columns(2).show(ui, |ui| {
            ui.label(RichText::new(tr("app.gdt.reads")).color(t.text_dim));
            ui.label(G::describe(&n).unwrap_or_default());
            ui.end_row();
            ui.label(RichText::new(tr("app.gdt.on")).color(t.text_dim));
            ui.label(trv("app.gdt.on_view", &[("view", view_word(jv::s(&n, "view")))]));
            ui.end_row();
        });
        crate::shell::note(ui, t, &tr("app.gdt.drag_note"));
        ui.horizontal(|ui| {
            if ui.add(crate::shell::Btn::new(tr("app.btn.delete"))).clicked() {
                *act = cmd("part.drawing", "delete");
            }
            if ui.add(crate::shell::Btn::new(tr("app.btn.done"))).clicked() {
                *act = cmd("part.drawing", "sel:");
            }
        });
        return;
    }
    ui.label(RichText::new(tr("app.drawing.head")).strong().family(crate::theme::bold_family()).color(t.text_strong));
    let sheets: Vec<(String, String)> = P::SHEET_KEYS.iter().map(|k| ((*k).to_string(), DR::sheet_label(k))).collect();
    let none = tr("app.drawing.none");
    let sec_of = d.get("section").filter(|s| s.is_object()).map(|s| jv::s(s, "of").to_string());
    egui::Grid::new("pc-dr-sheet").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        combo(ui, "pc-dr-paper", &tr("drawing.panel.sheet"), d.get("sheet").and_then(Value::as_str).unwrap_or("a3"), &sheets, "sheet", act);
        let mut so = vec![(String::new(), none.clone())];
        so.extend(["front", "top", "right"].iter().map(|k| ((*k).to_string(), tr(&format!("app.drawing.of_{k}")))));
        combo(ui, "pc-dr-sec", &tr("app.drawing.section_aa"), sec_of.as_deref().unwrap_or(""), &so, "section", act);
        if let Some(of) = &sec_of {
            let at = d.get("section").and_then(|s| jv::as_num(s.get("at"))).unwrap_or_else(|| pv.section_centre(of));
            if let Some(st) = pv.drawing.as_mut() {
                text_row(ui, st, "at", &tr("app.drawing.cut_at"), &cc_part::js::num_str(at), "", "at", act);
            }
        }
        let mut dop = vec![(String::new(), none.clone())];
        dop.extend(["front", "top", "right"].iter().map(|k| ((*k).to_string(), tr(&format!("app.drawing.from_{k}")))));
        if sec_of.is_some() {
            dop.push(("section".into(), tr("app.drawing.from_section")));
        }
        let det = d.get("detail").filter(|x| x.is_object()).cloned();
        combo(ui, "pc-dr-det", &tr("app.drawing.detail_b"), det.as_ref().map_or("", |x| jv::s(x, "of")), &dop, "detail", act);
        if let Some(det) = &det {
            if let Some(st) = pv.drawing.as_mut() {
                text_row(ui, st, "detail_r", &tr("app.drawing.detail_radius"), &cc_part::js::num_str(jv::n(det, "r")), "", "detail_r", act);
            }
            let sc: Vec<(String, String)> = P::DETAIL_SCALES
                .iter()
                .map(|k| (cc_part::js::num_str(*k), trv("app.drawing.times_sheet", &[("k", cc_part::js::num_str(*k))])))
                .collect();
            combo(ui, "pc-dr-dsc", &tr("app.drawing.detail_scale"), &cc_part::js::num_str(jv::n(det, "scale")), &sc, "detail_scale", act);
        }
        let mut ho = vec![(String::new(), tr("drawing.holes.none"))];
        ho.extend(
            ["front", "top", "right"]
                .iter()
                .map(|k| ((*k).to_string(), trv("drawing.holes.of", &[("view", tr(&format!("drawing.view.{k}")))]))),
        );
        let hv = d.get("holeTable").map(|x| jv::s(x, "view").to_string()).unwrap_or_default();
        combo(ui, "pc-dr-holes", &tr("drawing.holes.panel"), &hv, &ho, "holes", act);
        let ht = pv.drawing.as_ref().and_then(|s| s.sheet.as_ref()).and_then(|s| s.get("holeTable")).filter(|x| x.is_object()).cloned();
        if !hv.is_empty()
            && let Some(ht) = ht
        {
            let n = jv::arr(ht.get("rows")).len();
            let sk = jv::n(&ht, "skipped");
            ui.label("");
            let mut w = trv("drawing.holes.count", &[("n", n.to_string())]);
            if sk > 0.0 {
                w.push(' ');
                w.push_str(&trv("drawing.holes.skipped", &[("n", cc_part::js::num_str(sk))]));
            }
            ui.label(RichText::new(w).small().color(t.text_dim));
            ui.end_row();
        }
    });
    // title block
    ui.separator();
    ui.label(RichText::new(tr("drawing.panel.title_block")).strong().family(crate::theme::bold_family()));
    let title = d.get("title").cloned().unwrap_or(Value::Null);
    let revs = jv::arr(d.get("revisions")).to_vec();
    let today = ymd(pv.today_ms());
    let name = jv::s(&pv.part, "name").to_string();
    egui::Grid::new("pc-dr-title").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        for (k, _) in P::TITLE_FIELDS {
            let ph = match k {
                "title" => name.clone(),
                "rev" => revs.last().map(|r| jv::s(r, "rev").to_string()).unwrap_or_default(),
                "date" => today.clone(),
                "tolerance" => tr("drawing.panel.tolerance_ph"),
                _ => String::new(),
            };
            let label_key = format!(
                "drawing.panel.{}",
                k.chars()
                    .flat_map(|c| if c.is_ascii_uppercase() { vec!['_', c.to_ascii_lowercase()] } else { vec![c] })
                    .collect::<String>()
            );
            if let Some(st) = pv.drawing.as_mut() {
                text_row(ui, st, &format!("title:{k}"), &tr(&label_key), jv::s(&title, k), &ph, &format!("title:{k}"), act);
            }
        }
    });
    let mat = match pv.part.get("material") {
        Some(m) => trv("drawing.panel.material_is", &[("name", jv::s(m, "name").to_string())]),
        None => tr("drawing.panel.material_none"),
    };
    ui.label(RichText::new(mat).small().color(t.text_dim));
    // revisions
    ui.separator();
    ui.label(RichText::new(tr("drawing.panel.revisions")).strong().family(crate::theme::bold_family()));
    if revs.is_empty() {
        crate::shell::note(ui, t, &tr("drawing.panel.no_revisions"));
    }
    for (i, r) in revs.iter().enumerate() {
        ui.horizontal(|ui| {
            for (k, wdt) in [("rev", 36.0), ("desc", 110.0), ("date", 80.0), ("by", 44.0)] {
                if let Some(st) = pv.drawing.as_mut() {
                    let key = format!("rev:{i}:{k}");
                    let mut text = st.edits.get(&key).cloned().unwrap_or_else(|| jv::s(r, k).to_string());
                    let resp =
                        ui.add_sized([wdt, 26.0], crate::shell::field(&mut text)).on_hover_text(tr(&format!("drawing.panel.rev_{k}")));
                    if resp.has_focus() {
                        st.edits.insert(key.clone(), text.clone());
                    }
                    if resp.lost_focus() {
                        st.edits.remove(&key);
                        if text != jv::s(r, k) {
                            set(act, &key, text);
                        }
                    }
                }
            }
            if ui.add(crate::shell::Btn::new(tr("drawing.panel.rev_delete"))).clicked() {
                set(act, &format!("rev_delete:{i}"), "");
            }
        });
    }
    let full = revs.len() >= P::MAX_REVISIONS;
    let b = ui
        .add_enabled(!full, crate::shell::Btn::new(tr("drawing.panel.rev_add")))
        .on_disabled_hover_text(trv("drawing.panel.rev_full", &[("n", P::MAX_REVISIONS.to_string())]));
    if b.clicked() {
        set(act, "rev_add", "");
    }
    if let Some(sh) = pv.drawing.as_ref().and_then(|s| s.sheet.as_ref()) {
        let om: Vec<String> = jv::arr(sh.get("omitted")).iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
        if !om.is_empty() {
            ui.label(RichText::new(trv("app.drawing.no_room", &[("list", om.join(", "))])).small().color(t.text_strong));
        }
        crate::shell::note(ui, t, &tr("kernel.drawing.note"));
    }
    let notes = jv::arr(d.get("notes")).to_vec();
    if !notes.is_empty() {
        ui.label(RichText::new(tr("app.drawing.notes_on_sheet")).strong().family(crate::theme::bold_family()));
        for q in &notes {
            ui.horizontal(|ui| {
                ui.label(RichText::new(G::describe(q).unwrap_or_default()).small());
                if ui.add(crate::shell::Btn::new(tr("app.btn.select"))).clicked() {
                    *act = cmd("part.drawing", format!("sel:{}", jv::s(q, "id")));
                }
            });
        }
    }
    ui.label(RichText::new(tr("app.drawing.notes_help")).small().color(t.text_dim));
    ui.horizontal(|ui| {
        if ui.add(crate::shell::Btn::primary(tr("app.drawing.export_pdf"))).clicked() {
            *act = cmd("part.drawing", "pdf");
        }
        if ui.add(crate::shell::Btn::new(tr("app.drawing.back"))).clicked() {
            *act = cmd("part.drawing", "back");
        }
    });
}

#[cfg(test)]
mod tests {
    use crate::{Params, UiApp};
    use std::sync::{Arc, Mutex};

    fn run(app: &mut UiApp, id: &str, name: &str) -> Result<(), String> {
        app.run(id, &Params { name: Some(name.into()), ..Default::default() })
    }

    #[test]
    fn notes_on_the_sheet_undo_and_the_pdf() {
        type Saved = Arc<Mutex<Vec<(String, Vec<u8>)>>>;
        let saved: Saved = Arc::default();
        let mut app = UiApp::default();
        let s2 = saved.clone();
        app.services.save = Some(Box::new(move |name: &str, bytes: Vec<u8>| {
            s2.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((name.to_string(), bytes));
            Ok(Some(name.to_string()))
        }));
        run(&mut app, "part.sample", "").unwrap();
        run(&mut app, "part.drawing", "clock:1760700000000").unwrap();
        run(&mut app, "part.drawing", "enter").unwrap();
        run(&mut app, "part.drawing", "tool:datum").unwrap();
        run(&mut app, "part.drawing", "click:60,82.5").unwrap();
        let notes =
            |app: &UiApp| app.part.as_ref().and_then(|p| p.part.get("drawing")).map_or(0, |d| cc_part::jv::arr(d.get("notes")).len());
        assert_eq!(notes(&app), 1);
        run(&mut app, "part.drawing", "key:ArrowUp").unwrap();
        run(&mut app, "part.undo", "").unwrap();
        run(&mut app, "part.undo", "").unwrap();
        assert_eq!(notes(&app), 0);
        // hostile fields: refused or a note, never a panic
        assert!(run(&mut app, "part.drawing.set", "sheet=../../etc").is_err());
        run(&mut app, "part.drawing.set", "at=1e999").unwrap_or(());
        run(&mut app, "part.drawing", "click:NaN,1").unwrap_or(());
        run(&mut app, "part.drawing.set", &format!("title:company={}", "x".repeat(10_000))).unwrap();
        run(&mut app, "part.drawing", "pdf").unwrap();
        let s = saved.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (name, bytes) = s.last().unwrap();
        assert_eq!(name, "Bracket - drawing.pdf");
        assert!(bytes.starts_with(b"%PDF-1.4"));
    }
}
