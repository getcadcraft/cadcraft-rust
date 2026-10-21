//! TakeoffCraft's editing on the sheet, as takeoffcraft.js has it: arcs (R) and circles in runs and outlines, dragging
//! a corner of the selected mark (or the middle of one of its arcs to bend it), several marks selected (Shift+click,
//! Select all, a typical group's marks) with their panel, delete, and the clipboard (copy, cut, paste on the pointer,
//! paste in place, duplicate) on clipboard.js. Every edit is one `Takeoff` change with 0.5's undo label, and says what
//! 0.5's status line says.

use cc_takeoff::clipboard::{CopyCtx, PasteOpts, copy_marks, paste_clip, read_clip, read_clip_owned, scale_problem, size_problem};
use cc_takeoff::geom::{self as G, P};
use cc_takeoff::i18n::{F, t, tf};
use egui::{Color32, Pos2, RichText};
use serde_json::{Map, Value, json};

use crate::takeofftools::{TakeoffMode, pt_json};
use crate::theme::Tokens;

/// Screen pixels for picking marks and handles (takeoffcraft.js HIT).
pub const HIT: f64 = 7.0;
/// The modifier key the status lines name.
const MOD: &str = "Ctrl";

fn s(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(x)) => x.clone(),
        _ => String::new(),
    }
}

/// A corner (or an arc's middle) of the selected mark being dragged: the mark's rings as they will be.
#[derive(Debug, Clone)]
pub struct Drag {
    pub id: String,
    /// 0 = the outline / run, 1.. = a cut-out
    pub ring: usize,
    pub i: usize,
    /// the arc starting at corner `i` is bent (its middle was grabbed)
    pub arc: bool,
    pub pts: Vec<P>,
    pub holes: Vec<Vec<P>>,
    pub moved: bool,
}

/// An arc's middle handle: its ring, its start corner and where the handle sits.
#[derive(Debug, Clone, Copy)]
pub struct ArcHandle {
    pub ring: usize,
    pub i: usize,
    pub at: P,
}

/// segMid: the middle of a segment, on the arc when it is one.
pub fn seg_mid(p: &P, q: &P) -> P {
    match G::arc_of(p, q, p.b) {
        None => P::new(f64::midpoint(p.x, q.x), f64::midpoint(p.y, q.y)),
        Some(a) => {
            let th = a.a0 + a.sweep / 2.0;
            P::new(a.cx + a.r * th.cos(), a.cy + a.r * th.sin())
        }
    }
}

/// arcHandles: the arcs of a mark, each with its middle point.
pub fn arc_handles(m: &Value) -> Vec<ArcHandle> {
    let kind = s(m.get("kind"));
    if kind == "count" {
        return Vec::new();
    }
    let closed = kind == "area";
    let rings: Vec<Vec<P>> =
        std::iter::once(cc_takeoff::model::pts_of(m.get("pts"))).chain(cc_takeoff::model::holes_of(m.get("holes"))).collect();
    let mut out = Vec::new();
    for (r, ring) in rings.iter().enumerate() {
        let n = ring.len();
        for (i, p) in ring.iter().enumerate() {
            if !p.has_b() || (!closed && i + 1 == n) {
                continue;
            }
            if let Some(q) = ring.get((i + 1) % n) {
                out.push(ArcHandle { ring: r, i, at: seg_mid(p, q) });
            }
        }
    }
    out
}

fn ring_json(r: &[P]) -> Value {
    Value::Array(r.iter().map(pt_json).collect())
}

impl TakeoffMode {
    // ---- the selection ----

    /// `app.sel = id`: a new selection of one mark (or none).
    pub fn set_sel(&mut self, id: Option<String>) {
        self.sel = id;
        self.sel_more.clear();
    }

    /// setSelection(ids): the first is the main one.
    pub fn set_selection(&mut self, ids: &[String]) {
        self.set_sel(ids.first().cloned());
        for id in ids.iter().skip(1) {
            if !self.sel_more.contains(id) {
                self.sel_more.push(id.clone());
            }
        }
    }

    /// selIds(): the selected marks that still exist, the main one first.
    pub fn sel_ids(&self) -> Vec<String> {
        let have: std::collections::HashSet<&str> = self.tk.marks().iter().filter_map(|m| m.get("id").and_then(Value::as_str)).collect();
        self.sel.iter().chain(self.sel_more.iter()).filter(|id| have.contains(id.as_str())).cloned().collect()
    }

    pub fn is_sel(&self, id: &str) -> bool {
        self.sel.as_deref() == Some(id) || self.sel_more.iter().any(|x| x == id)
    }

    /// After any change (refresh): selected marks that are gone leave the selection.
    pub fn prune_sel(&mut self) {
        let gone: Vec<String> = self.sel_more.iter().filter(|id| self.tk.mark(id).is_none()).cloned().collect();
        self.sel_more.retain(|id| !gone.contains(id));
        if self.sel.as_deref().is_some_and(|m| self.tk.mark(m).is_none()) {
            let rest = self.sel_more.clone();
            self.set_selection(&rest);
        }
        if self.sel_note.as_deref().is_some_and(|n| self.tk.note(n).is_none()) {
            self.sel_note = None;
        }
    }

    /// selectAt: a click with Select — a markup on top first (not with Shift), else the mark under the pointer; with
    /// Shift the mark joins the selection or leaves it.
    pub fn select_at_mod(&mut self, sheet: &str, p: [f64; 2], additive: bool) {
        let q = P::new(p[0], p[1]);
        if !additive && let Some(n) = self.note_hit(sheet, q) {
            self.set_sel(None);
            let kind = s(n.get("kind"));
            self.sel_note = Some(s(n.get("id")));
            self.message = tf("note.selected", &[("kind", F::S(t(&format!("tool.{kind}")).to_lowercase()))]);
            return;
        }
        self.sel_note = None;
        let m = self.mark_at(sheet, p, self.upx(HIT));
        if additive && (self.sel.is_some() || !self.sel_more.is_empty()) {
            let Some(m) = m else { return };
            if self.is_sel(&m) {
                if self.sel.as_deref() == Some(m.as_str()) {
                    let rest = self.sel_more.clone();
                    self.set_selection(&rest);
                } else {
                    self.sel_more.retain(|x| *x != m);
                }
            } else {
                self.sel_more.push(m);
            }
            #[allow(clippy::cast_precision_loss)]
            let n = self.sel_ids().len() as f64;
            self.message = tf("sel.several", &[("n", F::N(n)), ("mod", F::S(MOD.into()))]);
        } else {
            self.select(m);
        }
    }

    /// Select all: every mark on the sheet.
    pub fn select_all(&mut self, sheet: &str) {
        self.end_paste(true);
        let ids: Vec<String> =
            self.tk.marks().iter().filter(|m| m.get("sheet").and_then(Value::as_str) == Some(sheet)).map(|m| s(m.get("id"))).collect();
        if self.tool != "select" {
            self.set_tool("select");
        }
        self.set_selection(&ids);
        #[allow(clippy::cast_precision_loss)]
        let n = ids.len() as f64;
        self.message = if ids.is_empty() { t("sel.all.none") } else { tf("sel.all", &[("n", F::N(n))]) };
    }

    /// Select none (the menu's: it says so).
    pub fn select_none(&mut self) {
        self.end_paste(true);
        self.set_sel(None);
        self.message = t("sel.none");
    }

    /// deleteSelected: the selected markup, else the selected marks (one undo step).
    pub fn delete_selected(&mut self) {
        if let Some(n) = self.sel_note.take() {
            let kind = self.tk.note(&n).map(|x| s(x.get("kind"))).filter(|k| !k.is_empty()).unwrap_or_else(|| "note".into());
            if let Err(e) = self.tk.remove_note(&n) {
                self.message = e;
                return;
            }
            let fills = [("kind", F::S(t(&format!("tool.{kind}")).to_lowercase())), ("mod", F::S(MOD.into()))];
            self.message = tf("note.deleted", &fills);
            return;
        }
        let ids = self.sel_ids();
        if ids.is_empty() {
            self.message = t("del.none");
            return;
        }
        if let Err(e) = self.tk.remove_marks(&ids) {
            self.message = e;
            return;
        }
        self.set_sel(None);
        #[allow(clippy::cast_precision_loss)]
        let n = ids.len() as f64;
        self.message = tf("del.done", &[("n", F::N(n)), ("mod", F::S(MOD.into()))]);
    }

    /// Undo / Redo (the toolbar and Ctrl+Z / Ctrl+Y): a paste on the pointer and the draft go first.
    pub fn history(&mut self, redo: bool) {
        self.end_paste(true);
        self.cancel_draft();
        let l = if redo { self.tk.redo() } else { self.tk.undo() };
        self.message = match (l, redo) {
            (Some(w), false) => tf("undo.did", &[("what", F::S(w))]),
            (Some(w), true) => tf("redo.did", &[("what", F::S(w))]),
            (None, false) => t("undo.nothing"),
            (None, true) => t("redo.nothing"),
        };
        self.prune_sel();
    }

    /// dropLastPoint (Backspace): the pending arc point first, then the last point (and the arc that led to it).
    pub fn drop_last_point(&mut self) {
        if self.mid.take().is_some() {
            return;
        }
        if self.pts.pop().is_none() {
            return;
        }
        self.ded.remove(&self.pts.len().saturating_sub(1));
        if let Some(l) = self.pts.last_mut() {
            l.b = 0.0;
        }
        if self.pts.is_empty() {
            self.last_click = None;
        }
    }

    // ---- arcs ----

    /// toggleArc (R): after a point, a point on the curve, then where the arc ends.
    pub fn toggle_arc(&mut self, on: bool) {
        self.arc = on;
        if !on {
            self.mid = None;
        }
        self.message = t(if on { "arc.on" } else { "arc.off" });
    }

    // ---- dragging corners ----

    /// handleAt: a corner of the selected mark (only one selected) under `p`, else the middle of one of its arcs.
    pub fn handle_at(&self, sheet: &str, p: [f64; 2]) -> Option<Drag> {
        if !self.sel_more.is_empty() {
            return None;
        }
        let m = self.sel.as_deref().and_then(|i| self.tk.mark(i))?;
        if m.get("sheet").and_then(Value::as_str) != Some(sheet) {
            return None;
        }
        let hit = self.upx(HIT);
        let q = P::new(p[0], p[1]);
        let pts = cc_takeoff::model::pts_of(m.get("pts"));
        let holes = cc_takeoff::model::holes_of(m.get("holes"));
        let id = s(m.get("id"));
        for (r, ring) in std::iter::once(&pts).chain(holes.iter()).enumerate() {
            for (i, c) in ring.iter().enumerate() {
                if G::dist(c, &q) <= hit {
                    return Some(Drag { id, ring: r, i, arc: false, pts: pts.clone(), holes: holes.clone(), moved: false });
                }
            }
        }
        for h in arc_handles(m) {
            if G::dist(&h.at, &q) <= hit {
                return Some(Drag { id, ring: h.ring, i: h.i, arc: true, pts, holes, moved: false });
            }
        }
        None
    }

    /// dragTo: the grabbed corner follows the pointer (keeping its arc), or the arc goes through it (a straight line
    /// drops the arc).
    pub fn drag_to(&mut self, p: [f64; 2]) {
        let Some(g) = self.drag.as_mut() else { return };
        let ring = if g.ring == 0 { Some(&mut g.pts) } else { g.holes.get_mut(g.ring - 1) };
        let Some(ring) = ring else { return };
        let n = ring.len();
        if g.arc {
            let (Some(a), Some(z)) = (ring.get(g.i).copied(), ring.get((g.i + 1) % n.max(1)).copied()) else { return };
            let b = G::bulge_through(&a, &P::new(p[0], p[1]), &z);
            if let Some(c) = ring.get_mut(g.i) {
                *c = if b.abs() > 1e-6 && b.abs() < 1e3 { P::arc(a.x, a.y, b) } else { P::new(a.x, a.y) };
            }
        } else if let Some(c) = ring.get_mut(g.i) {
            *c = P::arc(p[0], p[1], c.b);
        }
        g.moved = true;
    }

    /// commitDrag: one undo step; an outline that would cross itself (or a cut-out that would leave it) is put back.
    pub fn commit_drag(&mut self) {
        let Some(g) = self.drag.take() else { return };
        let Some(m) = self.tk.mark(&g.id) else { return };
        if !g.moved {
            return;
        }
        if s(m.get("kind")) == "area" {
            let why = G::outline_problem(&g.pts, "area").or_else(|| {
                g.holes.iter().enumerate().find_map(|(i, h)| {
                    let others: Vec<Vec<P>> = g.holes.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, x)| x.clone()).collect();
                    G::hole_problem(&g.pts, &others, h)
                })
            });
            if let Some(why) = why {
                self.message = format!("{why} {}", t("drag.back"));
                return;
            }
        }
        let mut patch = Map::new();
        patch.insert("pts".into(), ring_json(&g.pts));
        patch.insert("holes".into(), Value::Array(g.holes.iter().map(|h| ring_json(h)).collect()));
        let label = t(if g.arc { "undo.arc.bend" } else { "undo.point.move" });
        if let Err(e) = self.tk.update_mark(&g.id, &patch, Some(&label)) {
            self.message = e;
        }
    }

    /// A press at `a`, the pointer moved to `b`, the release (the pointer handlers): with Select a corner or an arc's
    /// middle is dragged; a press that does not move is a click; any other drag pans (nothing here).
    pub fn press_drag(&mut self, sheet: &str, a: [f64; 2], b: [f64; 2]) {
        self.drag = if self.tool == "select" && self.paste.is_none() { self.handle_at(sheet, a) } else { None };
        let moved = (b[0] - a[0]).hypot(b[1] - a[1]) > self.upx(4.0);
        if !moved {
            self.drag = None;
            self.click(sheet, b);
            return;
        }
        if self.drag.is_some() {
            self.drag_to(b);
            self.commit_drag();
        }
    }

    // ---- the clipboard ----

    fn copy_ctx(&self, sheet: &str) -> CopyCtx {
        let (view, uu) = self.page_info.get(sheet).copied().map_or((None, None), |(v, u)| (Some(json!(v)), Some(json!(u))));
        CopyCtx { view, user_unit: uu, source: "rust".into(), now: 0.0 }
    }

    /// copySelection.
    pub fn copy_selection(&mut self, quiet: bool) -> bool {
        self.end_paste(false);
        let ids = self.sel_ids();
        let Some(first) = ids.first().and_then(|i| self.tk.mark(i)) else {
            self.message = tf("copy.none", &[("mod", F::S(MOD.into()))]);
            return false;
        };
        let ctx = self.copy_ctx(&s(first.get("sheet")));
        let Some(wire) = copy_marks(&self.tk.state, &ids, &ctx) else { return false };
        let n = wire.get("marks").and_then(Value::as_array).map_or(0, Vec::len);
        let names = if quiet { String::new() } else { item_list(wire.get("items")) };
        // storeClip: the clip as read back (what a paste uses, as clipMem.clip); the wire form is not kept
        let clip = match read_clip_owned(wire) {
            Ok(r) => r.clip,
            Err(e) => {
                self.message = e;
                return false;
            }
        };
        if !quiet {
            #[allow(clippy::cast_precision_loss)]
            let fills = [("n", F::N(n as f64)), ("items", F::S(names)), ("mod", F::S(MOD.into()))];
            self.message = tf("copy.done", &fills);
        }
        self.clip = Some(clip);
        true
    }

    /// cutSelection: copy, then the marks go (one undo step).
    pub fn cut_selection(&mut self) {
        self.end_paste(true);
        let ids = self.sel_ids();
        if !self.copy_selection(true) {
            return;
        }
        #[allow(clippy::cast_precision_loss)]
        let n = ids.len() as f64;
        let label = cc_takeoff::i18n::tn("undo.cut", n);
        let set: std::collections::HashSet<&String> = ids.iter().collect();
        let r = self.tk.change(&label, |st| {
            if let Some(Value::Array(ms)) = st.get_mut("marks") {
                ms.retain(|m| m.get("id").and_then(Value::as_str).is_none_or(|i| !set.contains(&i.to_string())));
            }
            Ok(cc_takeoff::model::Ch::Commit(()))
        });
        if let Err(e) = r {
            self.message = e;
            return;
        }
        self.set_sel(None);
        self.message = tf("cut.done", &[("n", F::N(n)), ("mod", F::S(MOD.into()))]);
    }

    /// pasteRefused: can these marks go on the sheet?
    fn paste_refused(&self, clip: &Value, sheet: &str, in_place: bool) -> Option<String> {
        if clip.get("marks").and_then(Value::as_array).is_none_or(Vec::is_empty) {
            return None;
        }
        let (view, uu) = self.page_info.get(sheet).copied().unwrap_or(([0.0, 0.0, 612.0, 792.0], 1.0));
        let target = self.tk.scale_of(sheet).map(|sc| {
            let fpu = sc.get("ftPerUnit").cloned().unwrap_or(Value::Null);
            json!({ "ftPerUnit": fpu, "label": scale_text(sc, uu) })
        });
        if let Some(why) = scale_problem(clip, target.as_ref()) {
            return Some(why);
        }
        if in_place {
            return size_problem(clip, Some(&json!({ "view": view, "userUnit": uu })));
        }
        None
    }

    /// startPaste: items only at once; marks on the pointer (or in place).
    pub fn start_paste(&mut self, sheet: &str, in_place: bool) {
        self.end_paste(false);
        // the clip this window copied (currentClip), lent out while it is pasted (only the pointer keeps a copy)
        let Some(clip) = self.clip.take() else {
            self.message = tf("paste.empty", &[("mod", F::S(MOD.into()))]);
            return;
        };
        self.paste_from(&clip, sheet, in_place);
        self.clip = Some(clip);
    }

    fn paste_from(&mut self, clip: &Value, sheet: &str, in_place: bool) {
        if clip.get("marks").and_then(Value::as_array).is_none_or(Vec::is_empty) {
            let opts = PasteOpts { label: t("undo.item.paste"), ..PasteOpts::default() };
            match paste_clip(&mut self.tk, clip, &opts) {
                Ok(Some(out)) if !out.items.is_empty() => {
                    self.item = out.items.first().cloned();
                    let names: Vec<String> = out.items.iter().filter_map(|i| self.tk.item(i)).map(|i| s(i.get("name"))).collect();
                    #[allow(clippy::cast_precision_loss)]
                    let fills = [("n", F::N(out.items.len() as f64)), ("items", F::S(quote_list(&names)))];
                    self.message = tf("paste.items", &fills);
                }
                Ok(_) => {}
                Err(e) => self.message = e,
            }
            return;
        }
        if let Some(why) = self.paste_refused(clip, sheet, in_place) {
            self.message = why;
            return;
        }
        if in_place {
            self.finish_paste(sheet, clip, 0.0, 0.0, "inplace");
        } else {
            self.begin_placing(clip.clone(), "paste");
        }
    }

    /// duplicateSelection: a copy of the selected marks on the pointer.
    pub fn duplicate_selection(&mut self, sheet: &str) {
        self.end_paste(false);
        let ids = self.sel_ids();
        if ids.is_empty() {
            self.message = t("dup.none");
            return;
        }
        let ctx = self.copy_ctx(sheet);
        let Some(clip) = copy_marks(&self.tk.state, &ids, &ctx).and_then(|w| read_clip(&w).ok()).map(|r| r.clip) else { return };
        if self.tk.mark(ids.first().map_or("", String::as_str)).is_none_or(|m| s(m.get("sheet")) != sheet) {
            self.message = t("dup.sheet");
            return;
        }
        self.begin_placing(clip, "duplicate");
    }

    fn begin_placing(&mut self, clip: Value, kind: &'static str) {
        if self.tool != "select" {
            self.set_tool("select");
        }
        self.ac = None;
        let n = clip.get("marks").and_then(Value::as_array).map_or(0, Vec::len);
        self.paste = Some((clip, kind));
        #[allow(clippy::cast_precision_loss)]
        let fills = [("n", F::N(n as f64))];
        self.message = tf("paste.pointer", &fills);
    }

    /// placePaste: the click that puts the marks on the pointer down.
    pub fn place_paste(&mut self, sheet: &str, p: P) {
        let Some((clip, kind)) = self.paste.clone() else { return };
        if let Some(why) = self.paste_refused(&clip, sheet, false) {
            self.end_paste(false);
            self.message = why;
            return;
        }
        self.end_paste(true);
        let a = clip.get("anchor");
        let (ax, ay) = (
            a.and_then(|a| a.get("x")).and_then(Value::as_f64).unwrap_or(0.0),
            a.and_then(|a| a.get("y")).and_then(Value::as_f64).unwrap_or(0.0),
        );
        self.finish_paste(sheet, &clip, p.x - ax, p.y - ay, kind);
    }

    fn finish_paste(&mut self, sheet: &str, clip: &Value, dx: f64, dy: f64, kind: &str) {
        let opts =
            PasteOpts { sheet: Some(sheet.to_string()), dx, dy, label: t(&format!("undo.{kind}")), area: Some(self.cur_area.clone()) };
        let out = match paste_clip(&mut self.tk, clip, &opts) {
            Ok(Some(o)) => o,
            Ok(None) => {
                self.message = t("paste.nothing");
                return;
            }
            Err(e) => {
                self.message = tf("paste.failed", &[("reason", F::S(e))]);
                return;
            }
        };
        self.set_selection(&out.marks);
        #[allow(clippy::cast_precision_loss)]
        let mut msg = tf(&format!("paste.done.{kind}"), &[("n", F::N(out.marks.len() as f64))]);
        if !out.items.is_empty() {
            let names: Vec<String> = out.items.iter().filter_map(|i| self.tk.item(i)).map(|i| s(i.get("name"))).collect();
            #[allow(clippy::cast_precision_loss)]
            let fills = [("n", F::N(out.items.len() as f64)), ("items", F::S(quote_list(&names)))];
            msg = format!("{msg} {}", tf("paste.items", &fills));
        }
        self.message = format!("{msg} {}", tf("undo.hint", &[("mod", F::S(MOD.into()))]));
    }

    /// endPaste: stop carrying a paste (nothing is added).
    pub fn end_paste(&mut self, quiet: bool) {
        if self.paste.take().is_some() && !quiet {
            self.message = t("paste.canceled");
        }
    }

    /// The marks on the pointer, dashed where a click would put them.
    pub(crate) fn draw_paste(&self, painter: &egui::Painter, at: &dyn Fn(f64, f64) -> Pos2) {
        let (Some((clip, _)), Some(cur)) = (self.paste.as_ref(), self.cursor) else { return };
        let a = clip.get("anchor");
        let dx = cur[0] - a.and_then(|a| a.get("x")).and_then(Value::as_f64).unwrap_or(0.0);
        let dy = cur[1] - a.and_then(|a| a.get("y")).and_then(Value::as_f64).unwrap_or(0.0);
        let colors: std::collections::HashMap<String, Color32> = clip
            .get("items")
            .and_then(Value::as_array)
            .map(|l| l.iter().filter_map(|i| Some((s(i.get("id")), crate::pdfview::hex_color(i.get("color")?.as_str()?, 0.75)?))).collect())
            .unwrap_or_default();
        for m in clip.get("marks").and_then(Value::as_array).into_iter().flatten() {
            let c = colors.get(&s(m.get("item"))).copied().unwrap_or(Color32::from_rgb(0x1e, 0x48, 0xc8));
            let stroke = egui::Stroke::new(2.0, c);
            let mv = |r: Vec<P>| -> Vec<P> { r.into_iter().map(|p| P::arc(p.x + dx, p.y + dy, p.b)).collect() };
            let kind = s(m.get("kind"));
            let pts = mv(cc_takeoff::model::pts_of(m.get("pts")));
            if kind == "count" {
                if let Some(p) = pts.first() {
                    painter.circle_stroke(at(p.x, p.y), 6.0, stroke);
                }
                continue;
            }
            let closed = kind == "area";
            let holes = if closed { cc_takeoff::model::holes_of(m.get("holes")) } else { Vec::new() };
            for r in std::iter::once(pts).chain(holes.into_iter().map(mv)) {
                let mut line: Vec<Pos2> = G::flatten(&r, closed).iter().map(|p| at(p.x, p.y)).collect();
                if closed && let Some(f) = line.first().copied() {
                    line.push(f);
                }
                painter.add(egui::Shape::dashed_line(&line, stroke, 6.0, 4.0));
            }
        }
    }

    /// The editing command (`takeoff.edit`): copy, cut, paste, paste-in-place, duplicate, select-all, select-none,
    /// delete, back (Backspace), escape (Esc).
    pub fn edit(&mut self, sheet: &str, what: &str) -> Result<(), String> {
        match what {
            "copy" => {
                self.copy_selection(false);
            }
            "cut" => self.cut_selection(),
            "paste" => self.start_paste(sheet, false),
            "paste-in-place" => self.start_paste(sheet, true),
            "duplicate" => self.duplicate_selection(sheet),
            "select-all" => self.select_all(sheet),
            "select-none" => self.select_none(),
            "delete" => {
                self.end_paste(true);
                self.delete_selected();
            }
            "escape" => self.escape(),
            "back" => {
                // Backspace: the draft's last point, else the selection
                if self.mid.is_some() || !self.pts.is_empty() {
                    self.drop_last_point();
                } else {
                    self.end_paste(true);
                    self.delete_selected();
                }
            }
            _ => {
                return Err(format!(
                    "takeoff.edit: unknown action {what} (copy, cut, paste, paste-in-place, duplicate, select-all, select-none, delete, back, escape)"
                ));
            }
        }
        Ok(())
    }

    /// Esc, as takeoffcraft.js's keydown: Compare's alignment or Compare, a paste, Auto count, the pending arc point,
    /// the draft, the tool, the selection — the first that applies.
    pub fn escape(&mut self) {
        if let Some(c) = self.cmp.as_mut() {
            if c.align.take().is_some() {
                self.message = String::new();
            } else {
                self.cmp = None;
            }
            return;
        }
        if self.paste.is_some() {
            self.end_paste(false);
        } else if self.ac.is_some() {
            self.ac = None;
            self.message = t("ac.discarded");
        } else if self.mid.is_some() {
            self.mid = None;
        } else if !self.pts.is_empty() || self.note_start.is_some() {
            self.cancel_draft();
        } else if self.tool != "select" {
            self.set_tool("select");
        } else {
            self.end_paste(true);
            self.set_sel(None);
        }
    }

    /// Several marks selected (renderMultiProps): how many of each item, the bid area and typical group of them all,
    /// copy / duplicate / cut / delete.
    pub fn multi_props(&mut self, ui: &mut egui::Ui, tok: &Tokens, sheet: &str) {
        let ids = self.sel_ids();
        ui.label(RichText::new(t("mark.h.many")).strong().color(tok.text_strong));
        let mut by: Vec<(String, usize)> = Vec::new();
        for id in &ids {
            let item = self.tk.mark(id).map(|m| s(m.get("item"))).unwrap_or_default();
            match by.iter_mut().find(|x| x.0 == item) {
                Some(x) => x.1 += 1,
                None => by.push((item, 1)),
            }
        }
        let kv = |ui: &mut egui::Ui, k: String, v: String| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(k).color(tok.text_dim));
                ui.label(v);
            });
        };
        kv(ui, t("mark.f.marks"), ids.len().to_string());
        for (item, n) in &by {
            let name = self.tk.item(item).map(|i| s(i.get("name"))).unwrap_or_else(|| t("mark.f.item"));
            kv(ui, name, n.to_string());
        }
        let mut set: Vec<(String, String)> = Vec::new();
        self.area_group_fields(ui, &ids, &mut set);
        let mut act: Option<&str> = None;
        ui.horizontal_wrapped(|ui| {
            for (a, k) in [("copy", "edit.copy"), ("duplicate", "edit.duplicate"), ("cut", "edit.cut"), ("delete", "mark.delete.many")] {
                if ui.small_button(t(k)).clicked() {
                    act = Some(a);
                }
            }
        });
        for (k, v) in set {
            let _ = self.mark_field(&k, &v);
        }
        if let Some(a) = act {
            let _ = self.edit(sheet, a);
        }
    }
}

/// `itemList(names)`: each name in quotes, comma separated.
fn quote_list(names: &[String]) -> String {
    names.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(", ")
}

fn item_list(items: Option<&Value>) -> String {
    let names: Vec<String> = items.and_then(Value::as_array).map(|l| l.iter().map(|i| s(i.get("name"))).collect()).unwrap_or_default();
    quote_list(&names)
}

/// scaleText(sc, userUnit, true): a preset's label, else the preset it matches, else "1\" = N'".
pub fn scale_text(sc: &Value, user_unit: f64) -> String {
    if sc.get("method").and_then(Value::as_str) == Some("preset") {
        return s(sc.get("label"));
    }
    let fpu = sc.get("ftPerUnit").and_then(Value::as_f64).unwrap_or(0.0);
    match cc_takeoff::units::match_preset(fpu, user_unit, 0.005) {
        Some((m, _)) => m.label.to_string(),
        None => cc_takeoff::units::describe_scale(fpu, user_unit),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode() -> (TakeoffMode, String) {
        let mut m = TakeoffMode::new("abc");
        m.add_file("abc", "Plan.pdf", 10, 1);
        let sheet = m.sheet(0);
        m.cur_sheet = sheet.clone();
        m.set_preset(&sheet, "a1/4", 1.0);
        (m, sheet)
    }

    #[test]
    fn an_arc_in_a_run_and_a_circle() {
        let (mut m, sheet) = mode();
        m.add_item("Curb", "linear");
        m.toggle_arc(true);
        m.click(&sheet, [0.0, 0.0]);
        m.click(&sheet, [50.0, 50.0]); // on the curve
        m.click(&sheet, [100.0, 0.0]); // the end
        m.finish(&sheet);
        let mk = m.tk.marks().first().cloned().unwrap();
        assert!(mk["pts"][0]["b"].as_f64().unwrap().abs() > 0.5, "{mk}");
        assert!(mk["pts"][1].get("b").is_none());
        m.add_item("Pad", "area");
        m.set_tool("circle");
        m.click(&sheet, [300.0, 300.0]);
        m.click(&sheet, [301.0, 300.0]);
        assert_eq!(m.message, t("circle.err.small"));
        m.click(&sheet, [300.0, 300.0]);
        m.click(&sheet, [336.0, 300.0]);
        assert_eq!(m.tk.marks().len(), 2);
    }

    #[test]
    fn drag_a_corner_then_several_selected_and_deleted() {
        let (mut m, sheet) = mode();
        m.add_item("Walls", "linear");
        m.click(&sheet, [0.0, 0.0]);
        m.click(&sheet, [100.0, 0.0]);
        m.finish(&sheet);
        m.set_tool("select");
        m.press_drag(&sheet, [100.0, 0.0], [100.0, 50.0]);
        let mk = m.tk.marks().first().cloned().unwrap();
        assert_eq!(mk["pts"][1]["y"].as_f64(), Some(50.0));
        assert_eq!(m.tk.undo().as_deref(), Some(t("undo.point.move").as_str()));
        m.set_tool("draw");
        m.click(&sheet, [0.0, 200.0]);
        m.click(&sheet, [100.0, 200.0]);
        m.finish(&sheet);
        m.select_all(&sheet);
        assert_eq!(m.sel_ids().len(), 2);
        m.delete_selected();
        assert!(m.tk.marks().is_empty());
        assert!(m.message.starts_with("2 marks deleted"), "{}", m.message);
    }

    #[test]
    fn copy_and_paste_on_the_pointer() {
        let (mut m, sheet) = mode();
        m.page_info.insert(sheet.clone(), ([0.0, 0.0, 612.0, 792.0], 1.0));
        m.add_item("Outlets", "count");
        m.click(&sheet, [10.0, 10.0]);
        m.set_tool("select");
        m.click(&sheet, [10.0, 10.0]);
        assert!(m.copy_selection(false));
        m.start_paste(&sheet, false);
        assert!(m.paste.is_some());
        m.click(&sheet, [200.0, 200.0]);
        assert_eq!(m.tk.marks().len(), 2);
        assert!(m.paste.is_none());
    }
}
