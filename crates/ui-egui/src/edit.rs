//! Drafting (M4): the editable drawing (`cc-cad` Doc) behind the command line (`cc-engine`), and the pointer rules of
//! CadCraft 0.5's `cadcraft.js`: what a click means depends on what the command waits for (a point: object snap, then
//! ortho; objects: pick or window; idle: pick, Shift removes, a click on nothing starts a window).
//! Pure of egui: the UI feeds it world points and keys; tests drive it the same way.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::OnceLock;

use cc_cad::Doc;
use cc_cad::entity::Entity;
use cc_cad_geom::geom::{Bounds, intersect, prims};
use cc_cad_geom::pick::{Snap, dist_to_entity, entity_bounds, in_window, snap_points, valid_bounds};
use cc_engine::prompt::Kind;
use cc_engine::{Engine, Host, Lang};
use cc_geom::P2;

/// CadCraft 0.5's own catalogs (lang/*.json, unchanged).
static EN: &str = include_str!("../../../assets/lang/cadcraft/en.json");

/// The English catalog (the Properties panel reads it too).
pub(crate) fn lang_en_pub() -> Lang {
    lang_en()
}

fn lang_en() -> Lang {
    static V: OnceLock<serde_json::Value> = OnceLock::new();
    Lang::new(V.get_or_init(|| serde_json::from_str(EN).unwrap_or(serde_json::Value::Null)))
}

/// What a command asked the view to do (applied by the UI after the input that caused it).
#[derive(Clone, Debug, PartialEq)]
pub enum Zoom {
    Extents,
    Window(P2, P2, bool),
    By(f64),
    Previous,
}

/// Lines kept in the command history.
const LOG_LINES: usize = 200;

/// Object snap kinds on by default (cadcraft.js settings.kinds).
pub const SNAP_KINDS: &[&str] = &["end", "mid", "cen", "quad", "int", "perp", "node", "ins"];

/// view.js SNAP_PRIORITY
fn snap_priority(kind: &str) -> u8 {
    match kind {
        "end" | "int" | "node" => 0,
        "mid" | "cen" | "ins" => 1,
        "quad" | "perp" | "tan" => 2,
        "near" => 9,
        _ => 5,
    }
}

/// The host's side of the engine: what the UI shows and what the commands read back.
#[derive(Default)]
pub struct HostState {
    pub cursor: Option<P2>,
    pub shift: bool,
    pub selection: Vec<u64>,
    pub preview: Vec<Entity>,
    pub log: VecDeque<String>,
    pub prompt: String,
    pub zoom: Vec<Zoom>,
    /// the view on screen in drawing units
    pub view_wh: Option<(f64, f64)>,
    /// the model area on screen (HATCH looks for boundaries there)
    pub view_box: Option<Bounds>,
    /// PROPERTIES / PROPERTIESCLOSE asked the window to open / fold the Properties panel
    pub props_open: Option<bool>,
    /// pick tolerance in drawing units (6 px at the current zoom)
    pub pick_tol: f64,
}

impl HostState {
    fn push_log(&mut self, s: &str) {
        for line in s.split('\n') {
            self.log.push_back(line.to_string());
        }
        while self.log.len() > LOG_LINES {
            self.log.pop_front();
        }
    }
}

/// view.js `pick`: the nearest editable object within the tolerance.
pub fn pick(doc: &Doc, p: P2, tol: f64) -> Option<u64> {
    let mut best = None;
    let mut bd = tol;
    for e in doc.entities() {
        if !doc.editable(e) {
            continue;
        }
        let b = entity_bounds(e, Some(&doc.blocks));
        if valid_bounds(&b) && (p.x < b.min_x - tol || p.x > b.max_x + tol || p.y < b.min_y - tol || p.y > b.max_y + tol) {
            continue;
        }
        let d = dist_to_entity(e, p, Some(&doc.blocks));
        if d <= bd {
            bd = d;
            best = e.id;
        }
    }
    best
}

/// view.js `pickWindow`.
pub fn pick_window(doc: &Doc, w: &Bounds, crossing: bool) -> Vec<u64> {
    doc.entities().filter(|e| doc.editable(e) && in_window(e, w, crossing, Some(&doc.blocks))).filter_map(|e| e.id).collect()
}

/// view.js `snap`: the best object snap within `r` drawing units of `p`.
pub fn snap(doc: &Doc, p: P2, r: f64, kinds: &[&str], base: Option<P2>) -> Option<Snap> {
    let mut nearby: Vec<&Entity> = Vec::new();
    for e in doc.entities() {
        if !doc.visible(e) {
            continue;
        }
        let b = entity_bounds(e, Some(&doc.blocks));
        let far = if valid_bounds(&b) {
            p.x < b.min_x - r || p.x > b.max_x + r || p.y < b.min_y - r || p.y > b.max_y + r
        } else {
            dist_to_entity(e, p, None) > r
        };
        if far {
            continue;
        }
        nearby.push(e);
        if nearby.len() > 400 {
            break;
        }
    }
    let mut best: Option<(Snap, u8, f64)> = None;
    let mut consider = |c: Snap| {
        let d = (c.x - p.x).hypot(c.y - p.y);
        if d.is_nan() || d > r {
            return;
        }
        let pr = snap_priority(c.kind);
        if best.as_ref().is_none_or(|(_, bp, bd)| pr < *bp || (pr == *bp && d < *bd)) {
            best = Some((c, pr, d));
        }
    };
    for e in &nearby {
        for c in snap_points(e, p, r, kinds, Some(&doc.blocks), base) {
            consider(c);
        }
    }
    if kinds.contains(&"int") && nearby.len() > 1 {
        let ps: Vec<Vec<cc_cad_geom::geom::Prim>> = nearby
            .iter()
            .take(60)
            .map(|e| prims(e, Some(&doc.blocks)).into_iter().filter(|q| !q.boxed && cc_cad_geom::geom::dist_to_prim(q, p) <= r).collect())
            .collect();
        for (i, a) in ps.iter().enumerate() {
            for b in ps.iter().skip(i.saturating_add(1)) {
                for qa in a {
                    for qb in b {
                        for x in intersect(qa, qb, false, false) {
                            consider(Snap { kind: "int", x: x.x, y: x.y, defer: None });
                        }
                    }
                }
            }
        }
    }
    best.map(|(s, _, _)| s)
}

struct UiHost(Rc<RefCell<HostState>>);

impl Host for UiHost {
    fn print(&mut self, s: &str) {
        self.0.borrow_mut().push_log(s);
    }
    fn prompt(&mut self, s: &str) {
        self.0.borrow_mut().prompt = s.to_string();
    }
    fn cursor(&self) -> Option<P2> {
        self.0.borrow().cursor
    }
    fn shift(&self) -> bool {
        self.0.borrow().shift
    }
    fn selection(&self) -> Vec<u64> {
        self.0.borrow().selection.clone()
    }
    fn set_selection(&mut self, ids: &[u64]) {
        self.0.borrow_mut().selection = ids.to_vec();
    }
    fn pick_at(&self, doc: &Doc, p: P2) -> Option<u64> {
        pick(doc, p, self.0.borrow().pick_tol)
    }
    fn pick_window(&self, doc: &Doc, w: &Bounds, crossing: bool) -> Vec<u64> {
        pick_window(doc, w, crossing)
    }
    fn preview(&mut self, ents: &[Entity]) {
        self.0.borrow_mut().preview = ents.to_vec();
    }
    fn on_command(&mut self, name: Option<&str>) {
        if name.is_none() {
            let mut s = self.0.borrow_mut();
            s.preview.clear();
        }
    }
    fn zoom_extents(&mut self) {
        self.0.borrow_mut().zoom.push(Zoom::Extents);
    }
    fn zoom_window(&mut self, a: P2, b: P2, exact: bool) {
        self.0.borrow_mut().zoom.push(Zoom::Window(a, b, exact));
    }
    fn zoom_previous(&mut self) -> bool {
        self.0.borrow_mut().zoom.push(Zoom::Previous);
        true
    }
    fn zoom_by(&mut self, f: f64) {
        self.0.borrow_mut().zoom.push(Zoom::By(f));
    }
    fn view_rect(&self) -> Option<(f64, f64)> {
        self.0.borrow().view_wh
    }
    fn view_extents(&self) -> Option<Bounds> {
        self.0.borrow().view_box
    }
    fn props(&mut self, open: bool) {
        self.0.borrow_mut().props_open = Some(open);
    }
}

/// The open editable drawing and its command line.
pub struct Editor {
    pub eng: Engine,
    pub hs: Rc<RefCell<HostState>>,
    /// the command line being typed
    pub line: String,
    pub name: String,
    pub osnap: bool,
    pub ortho: bool,
    /// the snap marker under the cursor
    pub snap: Option<Snap>,
    /// a selection window being drawn: its first corner (world)
    pub pending_box: Option<P2>,
    seen: Option<u64>,
    seen_space: Option<cc_cad::SpaceId>,
    /// the doc version at the last save (None = never saved)
    saved: Option<u64>,
    /// each object's display, kept between edits (only what an edit changed is walked again)
    pub display: cc_cad_geom::display::DisplayCache,
}

impl Editor {
    pub fn new(doc: Doc, name: &str) -> Self {
        let hs = Rc::new(RefCell::new(HostState { pick_tol: 1.0, ..HostState::default() }));
        let eng = Engine::new(doc, Box::new(UiHost(Rc::clone(&hs))), lang_en());
        let mut ed = Editor {
            eng,
            hs,
            line: String::new(),
            name: name.to_string(),
            osnap: true,
            ortho: false,
            snap: None,
            pending_box: None,
            seen: None,
            seen_space: None,
            saved: None,
            display: cc_cad_geom::display::DisplayCache::new(),
        };
        ed.saved = Some(ed.version());
        ed.refresh_prompt();
        ed
    }

    pub fn version(&self) -> u64 {
        self.eng.doc().version
    }
    /// Has the document (or the open space: BEDIT / BCLOSE) changed since the last call? (the canvas rebuilds its
    /// geometry then)
    pub fn take_changed(&mut self) -> bool {
        let v = self.version();
        let sp = self.eng.doc().space;
        let changed = self.seen != Some(v) || self.seen_space != Some(sp);
        self.seen = Some(v);
        self.seen_space = Some(sp);
        changed
    }
    pub fn dirty(&self) -> bool {
        self.saved != Some(self.version())
    }
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.version());
    }

    /// The prompt shown in front of the command line.
    pub fn prompt(&self) -> String {
        if self.eng.busy() { self.eng.prompt_line() } else { "Command:".to_string() }
    }
    fn refresh_prompt(&mut self) {
        let p = self.prompt();
        self.hs.borrow_mut().prompt = p;
    }

    /// The question wants a point (distance and angle questions take one too).
    pub fn wants_point(&self) -> bool {
        matches!(self.eng.expects(), Some(Kind::Point | Kind::Distance | Kind::Angle))
    }

    /// cadcraft.js `pointFor`: object snap, else ortho from the base point, else the cursor itself.
    pub fn point_for(&mut self, raw: P2, px_per_unit: f64) -> P2 {
        let base = self.eng.state().base;
        let shift = self.hs.borrow().shift;
        if self.osnap && !shift && px_per_unit > 0.0 {
            let s = snap(&self.eng.doc(), raw, 10.0 / px_per_unit, SNAP_KINDS, base);
            if let Some(s) = s {
                let p = P2::new(s.x, s.y);
                self.snap = Some(s);
                return p;
            }
        }
        self.snap = None;
        if self.ortho
            && let Some(b) = base
        {
            let (dx, dy) = (raw.x - b.x, raw.y - b.y);
            return if dx.abs() >= dy.abs() { P2::new(raw.x, b.y) } else { P2::new(b.x, raw.y) };
        }
        raw
    }

    /// The pointer moved: the cursor the commands see, the rubber band.
    pub fn hover(&mut self, raw: P2, px_per_unit: f64) {
        let p = if self.wants_point() { self.point_for(raw, px_per_unit) } else { raw };
        if !self.wants_point() {
            self.snap = None;
        }
        let preview = if self.eng.busy() { self.eng.preview(p) } else { Vec::new() };
        let mut h = self.hs.borrow_mut();
        h.cursor = Some(p);
        h.pick_tol = if px_per_unit > 0.0 { 6.0 / px_per_unit } else { 1.0 };
        h.preview = preview;
    }

    /// cadcraft.js `click`: the meaning depends on what the command waits for.
    pub fn click(&mut self, raw: P2, px_per_unit: f64) {
        if let Some(a) = self.pending_box.take() {
            self.finish_box(a, raw, px_per_unit);
            return;
        }
        if self.wants_point() {
            let p = self.point_for(raw, px_per_unit);
            self.eng.click(p, None, raw);
        } else if self.eng.expects() == Some(Kind::Select) {
            if !self.eng.click(raw, None, raw) {
                self.pending_box = Some(raw);
            }
        } else if self.eng.expects() == Some(Kind::Entity) {
            self.eng.click(raw, None, raw);
        } else if !self.eng.busy() {
            let (tol, shift) = {
                let h = self.hs.borrow();
                (h.pick_tol, h.shift)
            };
            let id = pick(&self.eng.doc(), raw, tol);
            let mut h = self.hs.borrow_mut();
            match id {
                Some(id) => {
                    if shift && h.selection.contains(&id) {
                        h.selection.retain(|x| *x != id);
                    } else if !h.selection.contains(&id) {
                        h.selection.push(id);
                    }
                }
                None => {
                    if !shift {
                        h.selection.clear();
                    }
                    drop(h);
                    self.pending_box = Some(raw);
                }
            }
        }
        self.refresh_prompt();
    }

    /// A window from `a` to `b` (world): left to right = window, right to left = crossing.
    pub fn finish_box(&mut self, a: P2, b: P2, px_per_unit: f64) {
        self.pending_box = None;
        if (a.x - b.x).hypot(a.y - b.y) * px_per_unit < 3.0 {
            return;
        }
        let rect = Bounds { min_x: a.x.min(b.x), min_y: a.y.min(b.y), max_x: a.x.max(b.x), max_y: a.y.max(b.y) };
        let crossing = b.x < a.x;
        if self.eng.expects() == Some(Kind::Select) {
            self.eng.window(rect, crossing);
        } else if !self.eng.busy() {
            let ids = pick_window(&self.eng.doc(), &rect, crossing);
            let mut h = self.hs.borrow_mut();
            let shift = h.shift;
            for id in ids {
                if shift {
                    h.selection.retain(|x| *x != id);
                } else if !h.selection.contains(&id) {
                    h.selection.push(id);
                }
            }
        }
        self.refresh_prompt();
    }

    /// Enter (or Space) on the command line.
    pub fn submit(&mut self) {
        let line = std::mem::take(&mut self.line);
        // cadcraft.js submit(): the answer is echoed after its question (a command's name is echoed by the engine)
        if self.eng.busy() {
            let echo = format!("{} {}", self.eng.prompt_line(), line);
            self.hs.borrow_mut().push_log(&echo);
        }
        self.eng.text(&line);
        self.refresh_prompt();
    }

    /// Type a whole line (headless: the CLI and tests).
    pub fn type_line(&mut self, line: &str) {
        self.line = line.to_string();
        self.submit();
    }

    /// Esc: cancel the command, else drop the selection.
    pub fn escape(&mut self) {
        self.pending_box = None;
        if !self.eng.escape() {
            self.hs.borrow_mut().selection.clear();
        }
        self.hs.borrow_mut().preview.clear();
        self.refresh_prompt();
    }

    /// Delete: erase the selection (idle only).
    pub fn delete(&mut self) {
        if !self.eng.busy() && !self.hs.borrow().selection.is_empty() {
            self.eng.run("ERASE");
            self.refresh_prompt();
        }
    }

    pub fn undo(&mut self) {
        if !self.eng.busy() {
            self.eng.run("UNDO");
            self.refresh_prompt();
        }
    }
    pub fn redo(&mut self) {
        if !self.eng.busy() {
            self.eng.run("REDO");
            self.refresh_prompt();
        }
    }

    /// Zoom requests from the last inputs.
    pub fn take_zoom(&mut self) -> Vec<Zoom> {
        std::mem::take(&mut self.hs.borrow_mut().zoom)
    }

    /// The selected objects (for the highlight).
    pub fn selected(&self) -> Vec<Entity> {
        let ids = self.hs.borrow().selection.clone();
        let doc = self.eng.doc();
        ids.iter().filter_map(|id| doc.get(*id).cloned()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> P2 {
        P2::new(x, y)
    }

    #[test]
    fn line_by_typing_and_clicking_then_undo() {
        let mut ed = Editor::new(Doc::new(), "t");
        ed.type_line("LINE");
        assert!(ed.wants_point(), "LINE asks for a point: {}", ed.prompt());
        ed.click(p(0.0, 0.0), 10.0);
        ed.type_line("@10,0");
        ed.hover(p(10.0, 5.0), 10.0);
        assert!(!ed.hs.borrow().preview.is_empty(), "rubber band while LINE waits");
        ed.click(p(10.0, 5.0), 10.0);
        ed.type_line("");
        assert!(!ed.eng.busy());
        assert_eq!(ed.eng.doc().entities().count(), 2);
        assert!(ed.take_changed() && ed.dirty());
        ed.undo();
        assert_eq!(ed.eng.doc().entities().count(), 0, "one undo step per command");
    }

    #[test]
    fn idle_pick_window_escape_delete() {
        let mut ed = Editor::new(Doc::new(), "t");
        ed.type_line("CIRCLE");
        ed.click(p(0.0, 0.0), 10.0);
        ed.type_line("5");
        ed.type_line("LINE");
        ed.type_line("20,0");
        ed.type_line("30,0");
        ed.type_line("");
        ed.hover(p(5.0, 0.0), 10.0);
        ed.click(p(5.0, 0.0), 10.0);
        assert_eq!(ed.hs.borrow().selection.len(), 1, "clicking the circle selects it");
        ed.escape();
        assert!(ed.hs.borrow().selection.is_empty());
        // a window around both from the left
        ed.click(p(-10.0, -10.0), 10.0);
        assert!(ed.pending_box.is_some(), "a click on nothing starts a window");
        ed.click(p(40.0, 10.0), 10.0);
        assert_eq!(ed.hs.borrow().selection.len(), 2);
        ed.delete();
        assert_eq!(ed.eng.doc().entities().count(), 0);
    }

    #[test]
    fn endpoint_snap_wins_over_the_cursor() {
        let mut ed = Editor::new(Doc::new(), "t");
        ed.type_line("LINE");
        ed.type_line("0,0");
        ed.type_line("10,0");
        ed.type_line("");
        ed.type_line("LINE");
        ed.click(p(10.3, 0.2), 10.0); // within 10 px of the end at 10 px/unit
        ed.type_line("10,10");
        ed.type_line("");
        let doc = ed.eng.doc();
        let starts: Vec<serde_json::Value> = doc.entities().map(|e| e.to_value()).collect();
        assert_eq!(starts[1]["x1"], serde_json::json!(10), "{starts:?}");
        assert_eq!(starts[1]["y1"], serde_json::json!(0));
    }
}
