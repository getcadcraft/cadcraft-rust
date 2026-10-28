//! Drafting (M4): the editable drawing (`cc-cad` Doc) behind the command line (`cc-engine`), and the pointer rules of
//! CadCraft 0.5's `cadcraft.js`: what a click means depends on what the command waits for (a point: object snap, then
//! ortho; objects: pick or window; idle: pick, Shift removes, a click on nothing starts a window).
//! Pure of egui: the UI feeds it world points and keys; tests drive it the same way.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use cc_cad::Doc;
use cc_cad::entity::Entity;
use cc_cad_geom::geom::{Bounds, intersect, prims};
use cc_cad_geom::pick::{Snap, dist_to_entity, entity_bounds, in_window, snap_points, valid_bounds};
use cc_engine::prompt::Kind;
use cc_engine::{Engine, Host, Lang};
use cc_geom::P2;

/// A CadCraft catalog string in the language now, with fills (hosts print with it).
pub fn lang_text(key: &str, vars: &[(&str, String)]) -> String {
    crate::lang::cad_lang().t(key, vars)
}

/// CadCraft's catalog in the language now (the Properties panel reads it too).
pub(crate) fn lang_en_pub() -> Lang {
    crate::lang::cad_lang()
}

fn lang_en() -> Lang {
    crate::lang::cad_lang()
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
        "cloud" => 3,
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
    /// PLOT asked for the Plot dialog
    pub plot_open: bool,
    /// _PLOTWIN picked a window (Some(None): cancelled) — the dialog opens again
    pub plot_window: Option<Option<Bounds>>,
    /// pick tolerance in drawing units (6 px at the current zoom)
    pub pick_tol: f64,
    /// QSELECT / FIND asked for their dialogs
    pub qs_open: bool,
    pub find_open: bool,
    /// MSPACE: the viewport worked in (its sheet's space, its id); the window shows the model through it
    pub entered: Option<(cc_cad::SpaceId, u64)>,
    /// MSPACE / PSPACE asked the window to go into a viewport (Some) or back to its sheet (None)
    pub enter_req: Option<Option<(cc_cad::SpaceId, u64)>>,
    /// UCS / PLAN asked the view to turn so an X axis at this angle is level
    pub turn_req: Option<f64>,
    /// polar tracking's step in degrees (POLARANG; cadcraft.js settings.polarInc)
    pub polar_inc: f64,
    /// COUNT handed over a file to save (name, bytes)
    pub blob: Option<(String, Vec<u8>)>,
    /// the window can pick files (Services::pick_files)
    pub can_pick: bool,
    /// XATTACH / XREF Reload / IMAGEATTACH / IMAGE asked for files
    pub pick_req: Option<PickReq>,
    /// the Plot dialog's ink ("mono" | "color") and pen (PUBLISH plots with them)
    pub plot_prefs: Option<(String, f64)>,
    /// INSERT, File asked the window to choose a block / a question asked for a button
    pub choose_req: Option<crate::libdlg::ChooseReq>,
    /// LIBRARY asked for the library panel
    pub lib_open: bool,
    /// PARAMETERS asked for the Parameters panel
    pub par_open: bool,
    /// INSERT, Library waits for the panel's pick
    pub lib_pick: bool,
    /// a card placed from the open panel, for the INSERT, Library it starts
    pub lib_queued: Option<serde_json::Value>,
    /// _PROPPICK: what the Properties panel's pick button asks for, and the point picked for it
    pub prop_pick_label: Option<String>,
    pub prop_picked: Option<P2>,
    /// QPMODE asked to turn Quick Properties on / off
    pub qp_toggle: bool,
    /// press and drag draws a rectangle (LASSOMODE 0), not a lasso (the default)
    pub lasso_box: bool,
    /// SHORTCUTS / COMPARESETTINGS asked for their dialogs
    pub keys_open: bool,
    pub cmp_open: bool,
    /// SHEETSET asked for the Sheet Set panel; the open set's name (the panel keeps it here for the command)
    pub ss_open: bool,
    pub ss_name: Option<String>,
    /// a line for the status bar (LASSOMODE says what a drag now does)
    pub status: Option<String>,
    /// APPLOAD / ALIASEDIT asked for their dialogs; DELBLOCKS for its block list (title, names)
    pub appload_req: bool,
    pub alias_req: bool,
    pub blocks_req: Option<(String, Vec<String>)>,
    /// the person's aliases file (REINIT applies it)
    pub alias_file: Option<String>,
    /// a routine's open DCL dialog; a routine waits for its next event; the status the last one closed with
    pub dcl: Option<crate::lispdlg::DclView>,
    pub dcl_waiting: bool,
    pub dcl_closed: Option<i32>,
}

/// What a command asked the window's picker for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickReq {
    /// one DWG / DXF (XATTACH, XREF Reload)
    Drawing,
    /// PNG / JPEG files (IMAGEATTACH: one; IMAGE: several)
    Pictures { multiple: bool },
    /// a LAS / LAZ / XYZ / PTS point cloud (POINTCLOUDATTACH)
    Cloud,
}

impl HostState {
    pub(crate) fn push_log(&mut self, s: &str) {
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
    /// The window has a 3D tab (cad3dtab.rs): the engine shows its workspace there.
    fn open3d(&mut self) -> bool {
        true
    }
    /// OPENIN3D shows the drawing on this window's own 3D tab (no ModelCraft handoff here).
    fn open_in_3d_tab(&self) -> bool {
        true
    }
    fn set_cursor(&mut self, p: P2) {
        self.0.borrow_mut().cursor = Some(p);
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
    fn preview(&mut self, ents: Vec<Entity>) {
        self.0.borrow_mut().preview = ents;
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
    fn lasso(&self) -> Option<bool> {
        Some(!self.0.borrow().lasso_box)
    }
    fn set_lasso(&mut self, on: bool) {
        let mut h = self.0.borrow_mut();
        h.lasso_box = !on;
        h.status = Some(lang_text(if on { "status.lasso.on" } else { "status.lasso.off" }, &[]));
    }
    fn shortcuts(&mut self) -> bool {
        self.0.borrow_mut().keys_open = true;
        true
    }
    fn sheet_set(&mut self) -> Option<Option<String>> {
        let mut h = self.0.borrow_mut();
        h.ss_open = true;
        Some(h.ss_name.clone())
    }
    fn compare_settings(&mut self) -> bool {
        self.0.borrow_mut().cmp_open = true;
        true
    }
    fn quick_props(&mut self) {
        self.0.borrow_mut().qp_toggle = true;
    }
    fn prop_pick_label(&self) -> Option<String> {
        self.0.borrow().prop_pick_label.clone()
    }
    fn prop_picked(&mut self, p: P2) {
        self.0.borrow_mut().prop_picked = Some(p);
    }
    fn plot(&mut self) -> bool {
        self.0.borrow_mut().plot_open = true;
        true
    }
    fn plot_window(&mut self, w: Option<Bounds>) {
        self.0.borrow_mut().plot_window = Some(w);
    }
    fn quick_select(&mut self) -> bool {
        self.0.borrow_mut().qs_open = true;
        true
    }
    fn find_dialog(&mut self) -> bool {
        self.0.borrow_mut().find_open = true;
        true
    }
    /// One clipboard for every drawing this window opens (copy in one, paste in another).
    fn clip_put(&mut self, clip: &cc_engine::clip::Clip) -> bool {
        CLIP_STORE.with(|s| *s.borrow_mut() = Some(clip.clone()));
        true
    }
    fn clip_get(&self) -> Option<cc_engine::clip::Shared> {
        CLIP_STORE.with(|s| s.borrow().clone()).map(cc_engine::clip::Shared::Clip)
    }
    fn now_ms(&self) -> Option<f64> {
        web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).ok().map(|d| d.as_secs_f64() * 1000.0)
    }
    fn view_center(&self) -> Option<P2> {
        self.0.borrow().view_box.map(|b| P2::new((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0))
    }
    fn has_viewports(&self) -> bool {
        true
    }
    fn viewport_entered(&self) -> Option<(cc_cad::SpaceId, u64)> {
        self.0.borrow().entered
    }
    fn viewport_at(&self, doc: &Doc, p: P2) -> Option<u64> {
        viewport_at(doc, p)
    }
    fn enter_viewport(&mut self, space: cc_cad::SpaceId, id: u64) {
        let mut h = self.0.borrow_mut();
        h.entered = Some((space, id));
        h.enter_req = Some(Some((space, id)));
    }
    fn leave_viewport(&mut self) {
        let mut h = self.0.borrow_mut();
        h.entered = None;
        h.enter_req = Some(None);
    }
    fn has_turn_view(&self) -> bool {
        true
    }
    fn turn_view(&mut self, ang: f64) {
        self.0.borrow_mut().turn_req = Some(ang);
    }
    fn polar_inc(&self) -> Option<f64> {
        Some(self.0.borrow().polar_inc)
    }
    fn set_polar_inc(&mut self, v: f64) {
        self.0.borrow_mut().polar_inc = v;
    }
    fn can_save_blob(&self) -> bool {
        true
    }
    fn plot_prefs(&self) -> Option<(String, f64)> {
        self.0.borrow().plot_prefs.clone()
    }
    fn choose_block(&mut self, file_name: &str, names: &[String]) -> cc_engine::Pick<Option<String>> {
        self.0.borrow_mut().choose_req = Some(crate::libdlg::ChooseReq::Block { file: file_name.to_string(), names: names.to_vec() });
        cc_engine::Pick::Later
    }
    fn choose(&mut self, title: &str, text: &str, buttons: &[(String, String)]) -> cc_engine::Pick<Option<String>> {
        self.0.borrow_mut().choose_req =
            Some(crate::libdlg::ChooseReq::Ask { title: title.into(), text: text.into(), buttons: buttons.to_vec() });
        cc_engine::Pick::Later
    }
    fn parameters(&mut self) -> bool {
        self.0.borrow_mut().par_open = true;
        true
    }
    fn open_library(&mut self) -> bool {
        self.0.borrow_mut().lib_open = true;
        true
    }
    fn library(&mut self) -> cc_engine::Pick<Option<(serde_json::Value, Option<String>)>> {
        let mut h = self.0.borrow_mut();
        if let Some(item) = h.lib_queued.take() {
            return cc_engine::Pick::Now(Some((item, None)));
        }
        h.lib_pick = true;
        cc_engine::Pick::Later
    }
    fn save_blob(&mut self, name: &str, data: &[u8], _mime: &str) {
        self.0.borrow_mut().blob = Some((name.to_string(), data.to_vec()));
    }
    fn can_pick_files(&self) -> bool {
        self.0.borrow().can_pick
    }
    fn appload(&mut self) -> bool {
        self.0.borrow_mut().appload_req = true;
        true
    }
    fn alias_edit(&mut self) -> bool {
        self.0.borrow_mut().alias_req = true;
        true
    }
    fn alias_text(&self) -> Option<String> {
        self.0.borrow().alias_file.clone()
    }
    fn choose_blocks(&mut self, title: &str, names: &[String]) -> cc_engine::Pick<Option<Vec<String>>> {
        self.0.borrow_mut().blocks_req = Some((title.to_string(), names.to_vec()));
        cc_engine::Pick::Later
    }
    fn pick_tolerance(&self) -> Option<f64> {
        Some(self.0.borrow().pick_tol)
    }
    fn pumps(&self) -> bool {
        true
    }
    /// dcl-ui.js `open`: a dialog already open closes with 0 first.
    fn dialog_open(&mut self, tree: &cc_lisp::host::Tile) -> bool {
        let mut h = self.0.borrow_mut();
        h.dcl = Some(crate::lispdlg::DclView::new(tree));
        h.dcl_closed = None;
        h.dcl_waiting = false;
        true
    }
    fn dialog_op(&mut self, op: cc_lisp::host::DlgOp) -> Option<Option<String>> {
        let mut h = self.0.borrow_mut();
        if let cc_lisp::host::DlgOp::Done(s) = op {
            if h.dcl.take().is_none() {
                return Some(None);
            }
            h.dcl_closed = Some(s);
            return Some(Some(String::new()));
        }
        Some(h.dcl.as_mut().and_then(|v| v.op(op)))
    }
    /// dcl-ui.js `run`: a queued event at once, the closing status once closed, else later.
    fn dialog_run(&mut self) -> cc_engine::Pick<cc_lisp::host::DlgEvent> {
        let mut h = self.0.borrow_mut();
        let ev = match h.dcl.as_mut() {
            None => Some(cc_lisp::host::DlgEvent::Done(Some(h.dcl_closed.take().unwrap_or(0)))),
            Some(v) => v.next_event(),
        };
        match ev {
            Some(ev) => cc_engine::Pick::Now(ev),
            None => {
                h.dcl_waiting = true;
                cc_engine::Pick::Later
            }
        }
    }
    fn pick_radius(&self) -> Option<f64> {
        Some(self.0.borrow().pick_tol)
    }
    /// The window's picker answers later: `UiApp::sync_editor` opens it and delivers the file.
    fn pick_drawing(&mut self) -> cc_engine::Pick<Option<cc_engine::PickedDrawing>> {
        self.0.borrow_mut().pick_req = Some(PickReq::Drawing);
        cc_engine::Pick::Later
    }
    fn pick_pictures(&mut self, multiple: bool) -> cc_engine::Pick<Vec<cc_engine::PickedFile>> {
        self.0.borrow_mut().pick_req = Some(PickReq::Pictures { multiple });
        cc_engine::Pick::Later
    }
    /// the window's point cloud (one per window: it stays when another drawing opens)
    fn cloud(&self) -> Option<Rc<RefCell<cc_engine::cloud::CloudAttach>>> {
        Some(crate::cloudview::cloud())
    }
    fn pick_cloud(&mut self) -> cc_engine::Pick<Option<cc_engine::PickedFile>> {
        if !self.0.borrow().can_pick {
            return cc_engine::Pick::Unsupported;
        }
        self.0.borrow_mut().pick_req = Some(PickReq::Cloud);
        cc_engine::Pick::Later
    }
    /// cadcraft.js `zoomCloud`: `view.fitPoints([min, max])` (margin 0.9, as a Zoom window)
    fn zoom_cloud(&mut self, b: Bounds) {
        self.0.borrow_mut().zoom.push(Zoom::Window(P2::new(b.min_x, b.min_y), P2::new(b.max_x, b.max_y), false));
    }
}

/// The viewport (turned on) of the open sheet under a point.
pub fn viewport_at(doc: &Doc, p: P2) -> Option<u64> {
    doc.entities()
        .find(|e| match &e.geo {
            cc_cad::entity::Geo::Viewport(v) => {
                e.get("on") != Some(&serde_json::Value::Bool(false)) && (p.x - v.x).abs() <= v.w / 2.0 && (p.y - v.y).abs() <= v.h / 2.0
            }
            _ => false,
        })
        .and_then(|e| e.id)
}

thread_local! {
    /// The window's shared clipboard store (cadcraft.js keeps it in localStorage so another tab pastes; one window
    /// here, so the copy itself).
    static CLIP_STORE: RefCell<Option<cc_engine::clip::Clip>> = const { RefCell::new(None) };
}

/// A lasso being dragged (cadcraft.js `drag`, kind 'lasso').
#[derive(Clone, Debug, PartialEq)]
pub struct LassoDrag {
    pub start_sx: f64,
    pub pts: Vec<P2>,
    pub mode: Option<&'static str>,
    /// the last point added, on screen
    last_s: (f64, f64),
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
    /// polar tracking (F10): points line up on multiples of the POLARANG step from the base point
    pub polar: bool,
    /// the snap marker under the cursor
    pub snap: Option<Snap>,
    /// a selection window being drawn: its first corner (world)
    pub pending_box: Option<P2>,
    /// a lasso being dragged (lasso.js): its screen start x, its points (world) and its mode once decided
    pub lasso: Option<LassoDrag>,
    seen: Option<u64>,
    seen_space: Option<cc_cad::SpaceId>,
    /// the doc version at the last save (None = never saved)
    saved: Option<u64>,
    /// each object's display, kept between edits (only what an edit changed is walked again)
    pub display: cc_cad_geom::display::DisplayCache,
}

impl Editor {
    pub fn new(doc: Doc, name: &str) -> Self {
        let hs = Rc::new(RefCell::new(HostState { pick_tol: 1.0, polar_inc: 90.0, ..HostState::default() }));
        let eng = Engine::new(doc, Box::new(UiHost(Rc::clone(&hs))), lang_en());
        let mut ed = Editor {
            eng,
            hs,
            line: String::new(),
            name: name.to_string(),
            osnap: true,
            ortho: false,
            polar: false,
            snap: None,
            pending_box: None,
            lasso: None,
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
    pub(crate) fn refresh_prompt(&mut self) {
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
            let r = 10.0 / px_per_unit;
            let s = snap(&self.eng.doc(), raw, r, SNAP_KINDS, base);
            // view.js snapExtra: a point cloud's points (Node), after the drawing's own
            let pr = s.as_ref().map_or(u8::MAX, |s| snap_priority(s.kind));
            let s = crate::cloudview::snap(self, raw, r, s.as_ref(), pr).or(s);
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
        if self.polar
            && let Some(b) = base
        {
            let step = self.hs.borrow().polar_inc.to_radians();
            let (dx, dy) = (raw.x - b.x, raw.y - b.y);
            let d = dx.hypot(dy);
            if step > 0.0 && d > 0.0 {
                let ang = dy.atan2(dx);
                let snapped = (ang / step).round() * step;
                // within 5° of a step: the point slides onto that direction
                if (ang - snapped).abs() <= 5f64.to_radians() {
                    let along = dx * snapped.cos() + dy * snapped.sin();
                    return P2::new(b.x + along * snapped.cos(), b.y + along * snapped.sin());
                }
            }
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

    /// Can a press and drag select now (cadcraft.js canSelectNow: "Select objects" waits, or no command runs)?
    pub fn can_select_now(&self) -> bool {
        self.eng.expects() == Some(Kind::Select) || !self.eng.busy()
    }

    /// cadcraft.js `startDrag('lasso', p)`: `down` / `p` world points, `down_s` / `p_s` screen points (px).
    pub fn lasso_start(&mut self, down: P2, down_s: (f64, f64), p: P2, p_s: (f64, f64)) {
        self.pending_box = None;
        self.lasso = Some(LassoDrag {
            start_sx: down_s.0,
            pts: vec![down, p],
            mode: cc_engine::lasso::lasso_mode(down_s.0, p_s.0, 6.0),
            last_s: p_s,
        });
    }
    /// cadcraft.js `dragMove(p)`: a point every 3 px; the first sideways move decides the mode.
    pub fn lasso_move(&mut self, p: P2, p_s: (f64, f64)) {
        let Some(l) = self.lasso.as_mut() else { return };
        if (l.last_s.0 - p_s.0).hypot(l.last_s.1 - p_s.1) >= 3.0 && l.pts.len() < 20_000 {
            l.pts.push(p);
            l.last_s = p_s;
        }
        if l.mode.is_none() {
            l.mode = cc_engine::lasso::lasso_mode(l.start_sx, p_s.0, 6.0);
        }
    }
    /// Space while dragging: window → crossing → fence.
    pub fn lasso_cycle(&mut self) {
        if let Some(l) = self.lasso.as_mut() {
            l.mode = Some(cc_engine::lasso::next_mode(l.mode.unwrap_or("window")));
        }
    }
    /// cadcraft.js `finishDrag()`: thinned to 2 px; "Select objects" takes it, else the selection (Shift removes).
    pub fn finish_lasso(&mut self, px_per_unit: f64) {
        let Some(l) = self.lasso.take() else { return };
        let step = if px_per_unit > 0.0 { 2.0 / px_per_unit } else { 0.0 };
        let pts = cc_engine::lasso::thin(&l.pts, step);
        let mode = l.mode.unwrap_or("window");
        if pts.len() < if mode == "fence" { 2 } else { 3 } {
            return;
        }
        if self.eng.expects() == Some(Kind::Select) {
            self.eng.lasso(&pts, mode);
        } else if !self.eng.busy() {
            let ids = {
                let d = self.eng.doc();
                let ents: Vec<Entity> = d.entities().filter(|e| d.editable(e)).cloned().collect();
                cc_engine::lasso::lasso_hits(&ents, &pts, mode, Some(&d.blocks))
            };
            let mut h = self.hs.borrow_mut();
            if h.shift {
                let off: std::collections::HashSet<u64> = ids.into_iter().collect();
                h.selection.retain(|x| !off.contains(x));
            } else {
                let mut have: std::collections::HashSet<u64> = h.selection.iter().copied().collect();
                for id in ids {
                    if have.insert(id) {
                        h.selection.push(id);
                    }
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
            // hashed, not a scan of the selection per id (a window over 9,000 objects was quadratic)
            if h.shift {
                let off: std::collections::HashSet<u64> = ids.into_iter().collect();
                h.selection.retain(|x| !off.contains(x));
            } else {
                let mut have: std::collections::HashSet<u64> = h.selection.iter().copied().collect();
                for id in ids {
                    if have.insert(id) {
                        h.selection.push(id);
                    }
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
        // "(…" and "!symbol": LISP (appload.js lispLine)
        if !self.eng.lisp_line(&line) {
            self.eng.text(&line);
        }
        self.refresh_prompt();
    }

    /// Type a whole line (headless: the CLI and tests).
    pub fn type_line(&mut self, line: &str) {
        self.line = line.to_string();
        self.submit();
    }

    /// A toolbar / ribbon button (cadcraft.js's toolbar): its command's first word runs (a running command is
    /// cancelled first), the rest answer its prompts one by one while it waits ("ZOOM E", "CIRCLE T").
    pub fn run_button(&mut self, text: &str) {
        let mut words = text.split(' ').filter(|w| !w.is_empty());
        if let Some(first) = words.next() {
            self.eng.run(first);
            for w in words {
                if self.eng.expects().is_none() {
                    break;
                }
                self.eng.text(w);
            }
        }
        self.refresh_prompt();
    }

    /// Esc: cancel the command, else drop the selection.
    pub fn escape(&mut self) {
        self.pending_box = None;
        if !self.eng.lisp_cancel() && !self.eng.escape() {
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
