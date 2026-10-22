//! The 3D tab's workings without a screen: ws3d-core.js `Workspace3D`, one for one. The window (ui-egui) adds the view
//! and the pointer, wheel and key events; the CLI and the parity harness drive it the same way.
//!
//! The workspace holds ModelCraft's tools (cc-m3d-tools, tools.js) on the drawing's 3D model (`Doc::m3d`, model3d.js)
//! and the camera. What a click means follows the command line (commands.js `engine.waiting`):
//!
//! - a point is asked → the point under the cursor (ModelCraft's inference: ends, edges, faces, else the ground), in
//!   drawing units with z ([`State::z3`](crate::engine::State::z3)); previews of 2D commands lie on the ground
//! - a height is asked (prompt `opts.height`) → the height along Z from the base, following the cursor
//! - objects are asked → the 2D object under the cursor on the ground (its outline, or the closed shape around it)
//! - a ModelCraft tool (prompt `opts.ws_tool`) → the tool's own state machine: push/pull, move, rotate, orbit, select;
//!   when it makes its edit the waiting prompt gets [`Reply::Done3d`]
//! - nothing asked → ModelCraft's Select tool (click, window, crossing; double-click opens a group)
//!
//! The 3D selection is the tools' `host.sel` in the open group `host.path`, separate from the 2D one.
//!
//! Unified undo (model3d.js `Model3D`): the tools edit the model through the kernel's history API; every step they make
//! goes into the drawing's history as `Op::M3d` (one undo step, or part of the running command's), and the kernel's own
//! stack is emptied again. A tool's own "undo the step I just made" (Move's 3x array) undoes the drawing's last step
//! when that is a 3D one, as `Model3D.undo` does: the step is lent to the kernel as its only undo step while the tool
//! runs.

use std::rc::Rc;
use std::sync::Arc;

use cc_cad::Doc;
use cc_cad::doc::Op;
use cc_cad::entity::{Entity, Geo};
use cc_cad::js;
use cc_cad_geom::geom::{K, Prim};
use cc_cad_geom::pick::{dist_to_entity, point_in_poly};
use cc_cad_geom::walk::{Opts as WOpts, WalkOpts, WalkOut, walk};
use cc_geom::P2;
use cc_geom::bvh::{BoxTree, boxes_of};
use cc_io_cad::m3dmodel as m3;
use cc_m3d::model::{SnapRef, Step};
use cc_m3d::ops::Sel;
use cc_m3d::vec::{BBox, IDENT, P3, Plane, add, bbox_add, dist, mul, norm, ray_plane, sub};
use cc_m3d_tools::camera::{Camera, view_dir};
use cc_m3d_tools::infer::{InferOpts, WORLD_AXES, along_line, infer, pick};
use cc_m3d_tools::overlay::{Line, Marker, Overlay};
use cc_m3d_tools::scene::{LEdge, Local};
use cc_m3d_tools::tools::{Ev, Host as THost, Tool, ToolArg, Tools};
use serde_json::{Value, json};

use crate::engine::{Engine, Reply, Shared};
use crate::prompt::Kind;

/// 2D line pieces the 3D view snaps to (a huge drawing: the first ones).
const MAX_SNAP_2D: usize = 50_000;
/// The serial of the drawing's step lent to the kernel (see the module header).
const LENT: u64 = u64::MAX - 7;
const TAU: f64 = std::f64::consts::PI * 2.0;

/// Visual styles of the 3D tab (model3d.js `STYLES`): key → ModelCraft's view mode and edges.
pub const STYLES: [(&str, &str, bool); 5] =
    [("shaded", "shaded", false), ("edges", "shaded", true), ("hidden", "hidden", true), ("wire", "wire", true), ("xray", "xray", true)];
/// `VIEW_NAMES`
pub const VIEW_NAMES: [&str; 7] = ["iso", "top", "bottom", "front", "back", "left", "right"];

/// `{rings, z0}` of a height question (or a base point).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeightSpec {
    pub rings: Vec<Vec<[f64; 2]>>,
    pub z0: f64,
    pub base: Option<P2>,
}

/// A pointer event on the 3D view (CSS pixels from its top-left).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ev3 {
    pub x: f64,
    pub y: f64,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// ms (double clicks); None = a second after the last press
    pub time: Option<f64>,
}
impl Ev3 {
    fn ev(&self) -> Ev {
        Ev { x: self.x, y: self.y, shift: self.shift, ctrl: self.ctrl, alt: self.alt, time: self.time, ..Ev::default() }
    }
}

/// What a pointer event means now (`inputMode()`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    None,
    Tool,
    Point,
    Height,
    Flat,
    Select2d,
    Entity2d,
}

/// The 2D drawing as lines on the ground (`lines()`): batches by colour (meters, x1 y1 x2 y2 …) and their box.
#[derive(Clone, Debug, Default)]
pub struct Lines2d {
    pub batches: Vec<(String, Vec<f64>)>,
    pub bbox: Option<BBox>,
}

/// The 3D tab.
#[derive(Debug)]
pub struct Ws3d {
    pub tools: Tools,
    /// the tab shows
    pub active: bool,
    pub style_key: String,
    /// a command drives a ModelCraft tool: (tool, copy)
    pub driving: Option<(String, bool)>,
    pub done: bool,
    pub recorded: bool,
    /// the 2D object under the cursor while objects are asked
    pub hover2d: Option<u64>,
    pub last_ev: Option<Ev3>,
    /// the pointer over the view (`th.mouse`)
    pub mouse: Option<(f64, f64)>,
    press_at: Option<(f64, f64)>,
    mode: Mode,
    opened_for_push: bool,
    first: bool,
    /// the drawing's model snapshot the tools' model matches
    synced: Option<Arc<str>>,
    lent: bool,
    /// the drawing's `m3d_records` last seen
    records_seen: u64,
    /// the kernel's own copies of model states the drawing's history holds (by snapshot): an undo or redo restores from
    /// them without reading the JSON again (the definitions that did not change are kept as they are)
    snaps: Vec<(Arc<str>, SnapRef)>,
    lines_for: Option<(u64, u64)>,
    lines: Rc<Lines2d>,
    /// what the workspace printed (the engine hands it to the host)
    pub out: Vec<Msg>,
}

/// A line the workspace prints: text, or a catalog message the engine words.
#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Text(String),
    /// `ws3d.editing_context`: a group opened for editing
    Editing(String),
}

fn units_of(doc: &Doc) -> String {
    doc.meta.get("units").filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_default()
}

/// `fmtNum` / commands3d.js `fmt`: 4 decimals, trailing zeros dropped.
pub fn fmt_num(n: f64) -> String {
    crate::input::fmt(n)
}

/// A walk output that hands over line pieces (strokes as primitives, fills as loop edges, points) by colour.
struct Segs<'a> {
    seg: &'a mut dyn FnMut(&str, f64, f64, f64, f64),
    fills: bool,
    points: bool,
    /// arcs as ws3d-core.js `lines2d` cuts them (true) or as `segsOf` (false)
    snap_arcs: bool,
}
impl WalkOut for Segs<'_> {
    fn stroke(&mut self, color: &str, prims: Vec<Prim>, _lw: f64, _o: Option<WOpts>) {
        for q in &prims {
            if q.k == K::L {
                (self.seg)(color, q.x1, q.y1, q.x2, q.y2);
                continue;
            }
            if self.snap_arcs {
                let mut sw = q.a1 - q.a0;
                let mut guard = 0u32;
                while sw <= 0.0 && guard < 64 {
                    sw += TAU;
                    guard = guard.saturating_add(1);
                }
                let m = js::max(2.0, (sw / (std::f64::consts::PI / 24.0)).ceil());
                let m = if m.is_finite() { m.min(1.0e5) } else { 2.0 };
                let mut i = 0.0;
                while i < m {
                    let a = q.a0 + sw * i / m;
                    let b = q.a0 + sw * (i + 1.0) / m;
                    (self.seg)(color, q.cx + q.r * a.cos(), q.cy + q.r * a.sin(), q.cx + q.r * b.cos(), q.cy + q.r * b.sin());
                    i += 1.0;
                }
            } else {
                segs_of(q, &mut |x1, y1, x2, y2| (self.seg)(color, x1, y1, x2, y2));
            }
        }
    }
    fn fill(&mut self, color: &str, loops: Vec<Vec<f64>>, _solid: bool, _o: Option<WOpts>) {
        if !self.fills {
            return;
        }
        for l in &loops {
            let n = l.len();
            let mut i = 0usize;
            while i.saturating_add(1) < n {
                let j = i.saturating_add(2).checked_rem(n).unwrap_or(0);
                let g = |k: usize| l.get(k).copied().unwrap_or(f64::NAN);
                (self.seg)(color, g(i), g(i.saturating_add(1)), g(j), g(j.saturating_add(1)));
                i = i.saturating_add(2);
            }
        }
    }
    fn text(&mut self, _color: &str, _e: &Entity, _o: Option<WOpts>) {}
    fn point(&mut self, color: &str, x: f64, y: f64) {
        if self.points {
            (self.seg)(color, x, y, x, y);
        }
    }
}

/// `segsOf(p, put)`: a primitive as segments (arcs 48 to a turn).
fn segs_of(p: &Prim, put: &mut dyn FnMut(f64, f64, f64, f64)) {
    if p.k == K::L {
        put(p.x1, p.y1, p.x2, p.y2);
        return;
    }
    let sweep = p.a1 - p.a0;
    let n = js::max(2.0, (sweep.abs() / (TAU / 48.0)).ceil());
    let n = if n.is_finite() { n.min(1.0e5) } else { 2.0 };
    let mut px = p.cx + p.r * p.a0.cos();
    let mut py = p.cy + p.r * p.a0.sin();
    let mut i = 1.0;
    while i <= n {
        let a = p.a0 + (sweep * i) / n;
        let x = p.cx + p.r * a.cos();
        let y = p.cy + p.r * a.sin();
        put(px, py, x, y);
        px = x;
        py = y;
        i += 1.0;
    }
}

/// `segPts(q)`: points along a primitive (an arc in short pieces), drawing units.
fn seg_pts(q: &Prim) -> Vec<P2> {
    if q.k == K::L {
        return vec![P2::new(q.x1, q.y1), P2::new(q.x2, q.y2)];
    }
    let mut sw = q.a1 - q.a0;
    let mut guard = 0u32;
    while sw <= 0.0 && guard < 64 {
        sw += TAU;
        guard = guard.saturating_add(1);
    }
    let m = js::max(2.0, (sw / (std::f64::consts::PI / 24.0)).ceil());
    let m = if m.is_finite() { m.min(1.0e5) } else { 2.0 };
    let mut out = Vec::new();
    let mut i = 0.0;
    while i <= m {
        let a = q.a0 + sw * i / m;
        out.push(P2::new(q.cx + q.r * a.cos(), q.cy + q.r * a.sin()));
        i += 1.0;
    }
    out
}

/// `crossesPoly(p, q, poly)`: does segment p–q cross any edge of the polygon?
fn crosses_poly(p: P2, q: P2, poly: &[P2]) -> bool {
    let o = |a: P2, b: P2, c: P2| js::sign((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x));
    let n = poly.len();
    for i in 0..n {
        let (Some(&a), Some(&b)) = (poly.get(i), poly.get(nxt(i, n))) else { continue };
        if o(p, q, a) * o(p, q, b) < 0.0 && o(a, b, p) * o(a, b, q) < 0.0 {
            return true;
        }
    }
    false
}

/// `withUnit(text, units)`: a bare number typed in 3D is in drawing units: give it the unit ModelCraft's parser knows.
pub fn with_unit(text: &str, units: &str) -> String {
    let t = js::trim(text);
    if !is_plain_decimal(t) {
        return t.to_string();
    }
    let n = js::str_to_number(t) * m3::m_per_unit(units);
    format!("{}m", js::num_to_string(n))
}

/// `/^[-+]?(\d+\.?\d*|\.\d+)$/`
fn is_plain_decimal(s: &str) -> bool {
    let b = s.strip_prefix(['-', '+']).unwrap_or(s);
    if let Some(rest) = b.strip_prefix('.') {
        return !rest.is_empty() && rest.bytes().all(|c| c.is_ascii_digit());
    }
    let digits = b.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return false;
    }
    let rest = b.get(digits..).unwrap_or("");
    match rest.strip_prefix('.') {
        Some(r) => r.bytes().all(|c| c.is_ascii_digit()),
        None => rest.is_empty(),
    }
}

/// `convertLengths(s, units)`: bare numbers in a typed value (also inside <…>, […] and lists) are drawing units.
pub fn convert_lengths(s: &str, units: &str) -> String {
    let split = |t: &str| t.split([',', ';']).map(|x| with_unit(x, units)).collect::<Vec<_>>().join(",");
    let first = s.chars().next();
    let last = s.chars().last();
    if s.chars().count() >= 2
        && matches!(first, Some('[' | '<'))
        && matches!(last, Some(']' | '>'))
        && let (Some(f), Some(l)) = (first, last)
    {
        let inner = s.get(f.len_utf8()..s.len().saturating_sub(l.len_utf8())).unwrap_or("");
        return format!("{f}{}{l}", split(inner));
    }
    if s.contains([',', ';']) {
        return split(s);
    }
    with_unit(s, units)
}

/// `/^@?\s*([-+.\d]+)\s*,\s*([-+.\d]+)\s*(?:,\s*([-+.\d]+))?$/` → the numbers.
fn displacement(s: &str) -> Option<(String, String, Option<String>)> {
    let mut rest = s.strip_prefix('@').unwrap_or(s);
    let tok = |r: &mut &str| -> Option<String> {
        let n = r.bytes().take_while(|c| c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.')).count();
        if n == 0 {
            return None;
        }
        let t = r.get(..n)?.to_string();
        *r = r.get(n..)?;
        Some(t)
    };
    let ws = |r: &mut &str| *r = r.trim_start_matches(js::is_ws);
    ws(&mut rest);
    let a = tok(&mut rest)?;
    ws(&mut rest);
    rest = rest.strip_prefix(',')?;
    ws(&mut rest);
    let b = tok(&mut rest)?;
    ws(&mut rest);
    if rest.is_empty() {
        return Some((a, b, None));
    }
    rest = rest.strip_prefix(',')?;
    ws(&mut rest);
    let c = tok(&mut rest)?;
    rest.is_empty().then_some((a, b, Some(c)))
}

fn ev_json(e: &Ev3) -> Value {
    let mut o = json!({"x": e.x, "y": e.y, "shift": e.shift, "ctrl": e.ctrl, "alt": e.alt});
    if let (Some(t), Some(m)) = (e.time, o.as_object_mut()) {
        m.insert("time".into(), json!(t));
    }
    o
}

impl Ws3d {
    /// `new Workspace3D({doc, engine, host})`
    pub fn new(doc: &Doc, faces_name: &str) -> Option<Ws3d> {
        let model = m3::model_of(doc, faces_name);
        let host = THost::new(model, Camera::new());
        let mut tools = Tools::new(host).ok()?;
        tools.collide = false;
        // ws3d-core.js sets `this.tip = ''` after making its Tools
        tools.host.tip = String::new();
        let mut ws = Ws3d {
            tools,
            active: false,
            style_key: "edges".into(),
            driving: None,
            done: false,
            recorded: false,
            hover2d: None,
            last_ev: None,
            mouse: None,
            press_at: None,
            mode: Mode::None,
            opened_for_push: false,
            first: true,
            synced: doc.m3d.clone(),
            lent: false,
            records_seen: doc.m3d_records,
            snaps: Vec::new(),
            lines_for: None,
            lines: Rc::new(Lines2d::default()),
            out: Vec::new(),
        };
        ws.sync(doc, faces_name);
        Some(ws)
    }

    /// The tools' model now matches the drawing's snapshot (an edit made outside `tcall`).
    pub fn sync_to(&mut self, doc: &Doc) {
        self.synced = doc.m3d.clone();
    }

    /// Meters per drawing unit.
    pub fn k(doc: &Doc) -> f64 {
        m3::k_of(doc)
    }

    /// The model the drawing holds now (after undo, redo or a command's 3D edit), the units, the 2D lines to snap to.
    pub fn sync(&mut self, doc: &Doc, faces_name: &str) {
        // Model3D.onRecord: a 3D edit was recorded (a command's too): the next idle tool event says done3d
        if doc.m3d_records != self.records_seen {
            self.records_seen = doc.m3d_records;
            self.recorded = true;
        }
        let cur = doc.m3d.clone();
        let same = match (&self.synced, &cur) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            let ok = match &cur {
                Some(s) => {
                    let kept = self.snaps.iter().find(|(a, _)| Arc::ptr_eq(a, s)).map(|(_, r)| r.clone());
                    match kept {
                        Some(r) => self.tools.host.model.restore(&r).is_ok(),
                        None => self.tools.host.model.restore(&SnapRef::Str(Rc::from(&**s))).is_ok(),
                    }
                }
                None => false,
            };
            if !ok {
                self.tools.host.set_model(m3::model_of(doc, faces_name));
            }
            self.synced = cur;
            self.validate();
        }
        self.tools.host.units = m3::display_unit(&units_of(doc)).to_string();
        // a sheet opened: the 3D tab hides (ws3d.js listens for 'space')
        if self.active && !doc.in_model() {
            self.hide();
        }
    }

    /// The 2D lines in the scene the tools snap to, made when the scene is first needed after a 2D edit (as
    /// `th.scene()` makes them).
    fn ensure_lines2d(&mut self, doc: &Doc) {
        let key = hash2((doc.version, Ws3d::k(doc).to_bits()));
        if self.tools.host.extra_key() != Some(key) {
            let l = self.lines2d(doc);
            self.tools.host.set_extra(Some((key, Rc::new(l))));
        }
    }

    /// `lines2d()`: the model space's visible 2D lines as soft edges (meters, z = 0; arcs in short pieces), at most
    /// MAX_SNAP_2D, as one definition's faces and edges.
    fn lines2d(&self, doc: &Doc) -> Local {
        let k = Ws3d::k(doc);
        let mut edges: Vec<LEdge> = Vec::new();
        {
            let mut seg = |_c: &str, x1: f64, y1: f64, x2: f64, y2: f64| {
                if edges.len() >= MAX_SNAP_2D {
                    return;
                }
                let id = edges.len() as f64 + 1.0;
                edges.push(LEdge { id, a: [x1 * k, y1 * k, 0.0], b: [x2 * k, y2 * k, 0.0], soft: true });
            };
            let ents: Vec<&Entity> = doc.model().collect();
            let mut out = Segs { seg: &mut seg, fills: false, points: true, snap_arcs: true };
            walk(doc, &ents, &mut out, &WalkOpts::default());
        }
        let lists: Vec<[P3; 2]> = edges.iter().map(|e| [e.a, e.b]).collect();
        let etree = BoxTree::new(boxes_of(lists.iter().map(|l| &l[..]), 0.0));
        Local {
            faces: Vec::new(),
            edges,
            tri: Vec::new(),
            tri_face: Vec::new(),
            ftree: BoxTree::new(Vec::new()),
            etree,
            boxes: std::cell::RefCell::new(std::collections::HashMap::new()),
        }
    }

    /// `lines()`: the 2D model-space drawing as line batches by colour (meters, z = 0), and their box. Layers that are
    /// off or frozen are left out; blocks and dimensions come expanded; text is left out.
    pub fn lines(&mut self, doc: &Doc) -> Rc<Lines2d> {
        let key = (doc.version, Ws3d::k(doc).to_bits());
        if self.lines_for == Some(key) {
            return Rc::clone(&self.lines);
        }
        let k = Ws3d::k(doc);
        let mut by: Vec<(String, Vec<f64>)> = Vec::new();
        {
            let mut seg = |c: &str, x1: f64, y1: f64, x2: f64, y2: f64| {
                let i = match by.iter().position(|(cc, _)| cc == c) {
                    Some(i) => i,
                    None => {
                        by.push((c.to_string(), Vec::new()));
                        by.len().saturating_sub(1)
                    }
                };
                if let Some((_, a)) = by.get_mut(i) {
                    a.extend_from_slice(&[x1 * k, y1 * k, x2 * k, y2 * k]);
                }
            };
            let ents: Vec<&Entity> = doc.model().collect();
            let mut out = Segs { seg: &mut seg, fills: true, points: false, snap_arcs: false };
            walk(doc, &ents, &mut out, &WalkOpts::default());
        }
        let mut bb: Option<BBox> = None;
        for (_, a) in &by {
            for c in a.as_chunks::<2>().0 {
                bb = Some(bbox_add(bb, [c[0], c[1], 0.0]));
            }
        }
        self.lines = Rc::new(Lines2d { batches: by, bbox: bb });
        self.lines_for = Some(key);
        Rc::clone(&self.lines)
    }

    /// `validate()`: the selection and the open group still exist (after undo, redo, open).
    pub fn validate(&mut self) {
        let h = &mut self.tools.host;
        if h.model.context(&h.path).is_none() {
            h.path = Vec::new();
        }
        let Some(ctx) = h.model.context(&h.path) else { return };
        let Some(def) = h.model.def(&ctx.def) else { return };
        let keep: Vec<Sel> = h
            .sel
            .iter()
            .filter(|s| match s.t.as_str() {
                "e" => def.edges.get(s.id).is_some(),
                "f" => def.faces.get(s.id).is_some(),
                "i" => def.insts.get(s.id).is_some(),
                _ => h.model.guides.get(s.id).is_some(),
            })
            .cloned()
            .collect();
        h.sel = keep;
    }

    // ---- the drawing's history ----

    /// Before the tools run: the kernel's stacks emptied, and the drawing's last step lent to it when that is a 3D one.
    fn prep(&mut self, doc: &Doc) {
        self.ensure_lines2d(doc);
        let m = &mut self.tools.host.model;
        // th.units reads model.syncUnits()
        m.units = m3::display_unit(&units_of(doc)).to_string();
        m.undo_stack.clear();
        m.redo_stack.clear();
        self.lent = false;
        let Some(top) = doc.undo_stack.last() else { return };
        if top.ops.is_empty() || !top.ops.iter().all(|o| matches!(o, Op::M3d { .. })) {
            return;
        }
        let (Some(Op::M3d { before: Some(b), .. }), Some(Op::M3d { after: Some(a), .. })) = (top.ops.first(), top.ops.last()) else {
            return;
        };
        // the kernel's own copies when it has them (an undo then keeps the definitions that did not change)
        let snaps = &self.snaps;
        let of =
            |x: &Arc<str>| snaps.iter().find(|(k, _)| Arc::ptr_eq(k, x)).map_or_else(|| SnapRef::Str(Rc::from(&**x)), |(_, r)| r.clone());
        m.undo_stack.push(Step { label: top.label.clone(), b: of(b), a: of(a), serial: LENT });
        self.lent = true;
    }

    /// After the tools ran: their steps go into the drawing's history (`Model3D.record`); the lent step undone by a
    /// tool is undone in the drawing (`Model3D.undo`).
    fn finish(&mut self, doc: &mut Doc) {
        let m = &mut self.tools.host.model;
        let steps = std::mem::take(&mut m.undo_stack);
        m.redo_stack.clear();
        let undone = self.lent && steps.first().map(|s| s.serial) != Some(LENT);
        self.lent = false;
        if undone {
            doc.undo();
        }
        for s in steps.into_iter().filter(|s| s.serial != LENT) {
            let before = s.b.s();
            let after = s.a.s();
            let b = doc.m3d.clone().filter(|x| **x == *before).unwrap_or_else(|| Arc::from(before.as_str()));
            let a: Arc<str> = Arc::from(after.as_str());
            if doc.record_m3d(&s.label, Some(Arc::clone(&b)), Arc::clone(&a)).is_ok() {
                self.recorded = true;
                self.keep_snap(b, s.b.clone());
                self.keep_snap(a, s.a.clone());
            }
        }
        self.synced = doc.m3d.clone();
        self.records_seen = doc.m3d_records;
        if undone {
            self.validate();
        }
    }

    /// Run the tools (`f`) with the drawing's history around them; their messages go to `out`. Opening a group prints
    /// what is being edited (th.openContext); closing one selects it (th.closeContext).
    pub fn tcall<R>(&mut self, doc: &mut Doc, f: impl FnOnce(&mut Tools) -> cc_m3d::Res<R>) -> Option<R> {
        self.prep(doc);
        let path0 = self.tools.host.path.clone();
        let sel0 = self.tools.host.sel.clone();
        let r = f(&mut self.tools);
        self.drain();
        let path = self.tools.host.path.clone();
        if path != path0 {
            if path.len() > path0.len() {
                let name = self.tools.host.model.context(&path).and_then(|c| self.tools.host.model.def(&c.def).map(|d| d.name.clone()));
                match name {
                    Some(n) => self.out.push(Msg::Editing(n)),
                    None => {
                        self.tools.host.path = path0;
                        self.tools.host.sel = sel0;
                    }
                }
            } else if let Some(leaving) = path0.last() {
                self.tools.host.sel = vec![Sel::new("i", *leaving)];
            }
        }
        self.finish(doc);
        r.ok()
    }

    /// Keep the kernel's copy of a state the drawing's history holds (the newest 64).
    fn keep_snap(&mut self, key: Arc<str>, snap: SnapRef) {
        if !matches!(snap, SnapRef::Snap(_)) || self.snaps.iter().any(|(a, _)| Arc::ptr_eq(a, &key)) {
            return;
        }
        self.snaps.push((key, snap));
        if self.snaps.len() > 64 {
            self.snaps.remove(0);
        }
    }

    fn drain(&mut self) {
        self.out.extend(self.tools.host.msgs.drain(..).map(Msg::Text));
    }

    /// `check()`: a tool made its edit and is idle again → true (the waiting tool prompt gets `done3d`).
    pub fn check(&mut self) -> bool {
        if !self.recorded || self.tools.busy() {
            return false;
        }
        self.recorded = false;
        self.done = true;
        true
    }
    pub fn consume_done(&mut self) -> bool {
        let d = self.done;
        self.done = false;
        self.recorded = false;
        d
    }

    // ---- showing ----

    /// `ws.show()` (headless part): the tab shows; the first time, the drawing's saved 3D view, else Iso.
    pub fn show(&mut self, doc: &Doc) {
        self.active = true;
        if self.first {
            self.first = false;
            if !self.apply_saved(doc) {
                self.view(doc, "iso");
            }
        }
    }
    /// `ws.hide()`
    pub fn hide(&mut self) {
        if self.tools.busy() {
            let _ = self.tools.escape();
            self.drain();
        }
        self.active = false;
    }

    /// `applySaved()`: the camera and visual style a drawing saved; false when it has none.
    pub fn apply_saved(&mut self, doc: &Doc) -> bool {
        let Some(v) = doc.view3d.as_ref() else { return false };
        let p3 = |k: &str| -> Option<P3> {
            let a = v.get(k)?.as_array()?;
            let g = |i: usize| a.get(i).and_then(Value::as_f64);
            Some([g(0)?, g(1)?, g(2)?])
        };
        let (Some(eye), Some(target)) = (p3("eye"), p3("target")) else { return false };
        let cam = &mut self.tools.host.cam;
        cam.eye = eye;
        cam.target = target;
        cam.persp = v.get("persp").is_some_and(|x| js::truthy(Some(x)));
        if let Some(f) = v.get("fov").and_then(Value::as_f64).filter(|f| *f != 0.0) {
            cam.fov = f;
        }
        if let Some(s) = v.get("style").and_then(Value::as_str) {
            self.set_style(s);
        }
        true
    }
    /// `viewState()`: what is saved with the drawing.
    pub fn view_state(&self) -> Value {
        let c = &self.tools.host.cam;
        json!({"eye": c.eye, "target": c.target, "persp": c.persp, "fov": c.fov, "style": self.style_key})
    }

    /// Everything there is to see: the solids and the drawing (meters).
    pub fn bbox(&mut self, doc: &Doc) -> Option<BBox> {
        let mut bb = self.tools.host.model.bbox("root", &IDENT);
        if let Some(l) = self.lines(doc).bbox {
            bb = Some(bbox_add(bb, l.min));
            bb = Some(bbox_add(bb, l.max));
        }
        bb
    }
    /// `fit(dir)`: look at everything from the current direction (or `dir`), without Camera.fit's half-meter floor.
    pub fn fit(&mut self, doc: &Doc, dir: Option<P3>) {
        let bb = self.bbox(doc).unwrap_or(BBox { min: [-5.0, -5.0, 0.0], max: [5.0, 5.0, 3.0] });
        let cam = &mut self.tools.host.cam;
        let d = norm(dir.unwrap_or_else(|| sub(cam.eye, cam.target)));
        let c = mul(add(bb.min, bb.max), 0.5);
        let r = js::max(dist(bb.min, bb.max) / 2.0, 1e-3);
        let fov_min = (js::min(cam.fov, cam.fov * cam.aspect()) * std::f64::consts::PI) / 360.0;
        cam.target = c;
        cam.eye = add(c, mul(d, (r / fov_min.sin()) * 1.08));
        cam.clip(r);
    }
    /// `view(name)`: a standard view, Perspective / Parallel, or Extents.
    pub fn view(&mut self, doc: &Doc, name: &str) {
        if name == "persp" || name == "parallel" {
            self.tools.host.cam.persp = name == "persp";
            return;
        }
        if name == "extents" {
            self.fit(doc, None);
            return;
        }
        let n = if VIEW_NAMES.contains(&name) { name } else { "iso" };
        self.fit(doc, view_dir(n));
    }
    /// `setStyle(key)`
    pub fn set_style(&mut self, key: &str) -> bool {
        if !STYLES.iter().any(|s| s.0 == key) {
            return false;
        }
        self.style_key = key.to_string();
        true
    }
    /// The view mode and edges of the visual style.
    pub fn style(&self) -> (&'static str, bool) {
        STYLES.iter().find(|s| s.0 == self.style_key).map_or(("shaded", true), |s| (s.1, s.2))
    }

    // ---- geometry under the cursor ----

    fn to_units(p: P3, k: f64) -> [f64; 3] {
        [p[0] / k, p[1] / k, p[2] / k]
    }
    fn infer_at(&mut self, doc: &Doc, ev: &Ev3) -> Option<cc_m3d_tools::infer::Snap> {
        self.ensure_lines2d(doc);
        let sc = self.tools.host.scene();
        let snap = self.tools.snapping;
        let r = infer(&sc, &self.tools.host.cam, ev.x, ev.y, &InferOpts { axes: Some(WORLD_AXES), snap, ..InferOpts::default() });
        r.p.iter().all(|x| x.is_finite()).then_some(r)
    }
    /// The point a click stands for (drawing units, with z).
    fn point_at(&mut self, doc: &Doc, ev: &Ev3) -> Option<[f64; 3]> {
        let k = Ws3d::k(doc);
        match self.infer_at(doc, ev) {
            Some(r) => Some(Ws3d::to_units(r.p, k)),
            None => self.ground_at(doc, ev.x, ev.y, 0.0),
        }
    }
    /// `groundAt(ev, z)`: where the cursor's ray meets the level plane z (drawing units).
    pub fn ground_at(&self, doc: &Doc, x: f64, y: f64, z: f64) -> Option<[f64; 3]> {
        let k = Ws3d::k(doc);
        let ray = self.tools.host.cam.ray(x, y);
        let (t, p) = ray_plane(ray.o, ray.d, &Plane { n: [0.0, 0.0, 1.0], d: z * k })?;
        (t > 0.0).then_some([p[0] / k, p[1] / k, z])
    }
    /// `heightAt(ev, spec)`: a height along Z from the base (the first ring's middle at z0) following the cursor.
    fn height_at(&self, doc: &Doc, ev: &Ev3, spec: &HeightSpec) -> Option<f64> {
        let k = Ws3d::k(doc);
        let (mut cx, mut cy) = (0.0, 0.0);
        match spec.rings.first().filter(|r| !r.is_empty()) {
            Some(ring) => {
                for [x, y] in ring {
                    cx += x;
                    cy += y;
                }
                cx /= ring.len() as f64;
                cy /= ring.len() as f64;
            }
            None => {
                if let Some(b) = spec.base {
                    cx = b.x;
                    cy = b.y;
                }
            }
        }
        let (_, s) = along_line(&self.tools.host.cam, ev.x, ev.y, [cx * k, cy * k, spec.z0 * k], [0.0, 0.0, 1.0])?;
        Some(s / k)
    }

    /// `pick2d(ev)`: the 2D object under the cursor on the ground: its outline within a few pixels, else the smallest
    /// closed one around it.
    fn pick2d(&self, doc: &Doc, ev: &Ev3) -> Option<u64> {
        let g = self.ground_at(doc, ev.x, ev.y, 0.0)?;
        let k = Ws3d::k(doc);
        let q = P2::new(g[0], g[1]);
        let tol = (self.tools.host.cam.pixel_size([g[0] * k, g[1] * k, 0.0]) * 6.0) / k;
        let mut best: Option<u64> = None;
        let mut best_d = tol;
        let mut inside: Option<u64> = None;
        let mut inside_a = f64::INFINITY;
        for e in doc.model() {
            if !doc.editable(e) {
                continue;
            }
            let d = dist_to_entity(e, q, Some(&doc.blocks));
            if d <= best_d {
                best_d = d;
                best = e.id;
            }
            if best.is_none()
                && matches!(e.geo, Geo::Poly(_) | Geo::Circle(_) | Geo::Ellipse(_))
                && let Ok(ring) = m3::outline2d(e)
            {
                let area = ring_area(&ring);
                let pts: Vec<P2> = ring.iter().map(|[x, y]| P2::new(*x, *y)).collect();
                if area < inside_a && point_in_poly(q, &pts) {
                    inside = e.id;
                    inside_a = area;
                }
            }
        }
        best.or(inside)
    }

    /// `entityLines(out, e, z, kind, dash)`: an object as overlay lines (meters).
    fn entity_lines(doc: &Doc, out: &mut Vec<Line>, e: &Entity, z: f64, kind: &str, dash: bool) {
        let k = Ws3d::k(doc);
        let zz = z * k;
        let mut seg = |_c: &str, x1: f64, y1: f64, x2: f64, y2: f64| {
            out.push(Line { a: [x1 * k, y1 * k, zz], b: [x2 * k, y2 * k, zz], kind: kind.to_string(), dash });
        };
        let mut o = Segs { seg: &mut seg, fills: true, points: false, snap_arcs: false };
        walk(doc, &[e], &mut o, &WalkOpts::default());
    }
    /// `prismLines(out, rings, z0, h)`
    fn prism_lines(doc: &Doc, out: &mut Vec<Line>, rings: &[Vec<[f64; 2]>], z0: f64, h: f64) {
        let k = Ws3d::k(doc);
        for r in rings {
            let n = r.len();
            let step = (n as f64 / 8.0).ceil();
            for i in 0..n {
                let (Some(&[x1, y1]), Some(&[x2, y2])) = (r.get(i), r.get(nxt(i, n))) else { continue };
                out.push(Line::new([x1 * k, y1 * k, z0 * k], [x2 * k, y2 * k, z0 * k], "face"));
                out.push(Line::new([x1 * k, y1 * k, (z0 + h) * k], [x2 * k, y2 * k, (z0 + h) * k], "face"));
                if n <= 8 || (i as f64) % step == 0.0 {
                    out.push(Line::dashed([x1 * k, y1 * k, z0 * k], [x1 * k, y1 * k, (z0 + h) * k], "axis-z"));
                }
            }
        }
    }
    /// `selectionLines()`: the 2D selection and the hovered object, drawn over the 3D view (meters).
    pub fn selection_lines(&self, doc: &Doc, selection: &[u64]) -> Vec<Line> {
        let mut out = Vec::new();
        for id in selection {
            if let Some(e) = doc.get(*id)
                && doc.in_model()
            {
                Ws3d::entity_lines(doc, &mut out, e, 0.0, "face", false);
            }
        }
        if let Some(h) = self.hover2d.filter(|h| !selection.contains(h))
            && let Some(e) = doc.model().find(|e| e.id == Some(h))
        {
            Ws3d::entity_lines(doc, &mut out, e, 0.0, "edge", true);
        }
        out
    }

    // ---- driving ModelCraft's tools for a command ----

    /// `begin(tool, {copy})`
    pub fn begin(&mut self, doc: &mut Doc, tool: &str, copy: bool) {
        self.driving = Some((tool.to_string(), copy));
        self.done = false;
        self.recorded = false;
        self.tcall(doc, |t| t.set(tool, ToolArg::None));
    }
    /// `end()`
    pub fn end(&mut self, doc: &mut Doc) {
        self.tcall(doc, |t| {
            if t.busy() {
                t.escape()?;
            }
            Ok(())
        });
        self.driving = None;
        if self.opened_for_push {
            self.tools.host.path = Vec::new();
            self.tools.host.sel = Vec::new();
            self.opened_for_push = false;
        }
        self.tcall(doc, |t| t.set("select", ToolArg::None));
        self.tools.host.overlay = None;
        self.done = false;
        self.recorded = false;
    }
    /// The select tool (a command asks for the 3D selection).
    pub fn set_tool(&mut self, doc: &mut Doc, name: &str) {
        self.tcall(doc, |t| t.set(name, ToolArg::None));
    }

    /// `value(text, {angle})`: a value typed at the command line while a tool runs. Lengths read in drawing units;
    /// ROTATE reads degrees. Err: what the JS throws (its message).
    pub fn value(&mut self, doc: &mut Doc, text: &str, angle: bool) -> Result<bool, String> {
        let units = units_of(doc);
        let mut s = js::trim(text).to_string();
        let name = self.tools.name.clone();
        if name == "move"
            && let Some((a, b, c)) = displacement(&s)
            && (s.starts_with('@') || c.is_some())
        {
            s = format!("<{a},{b},{}>", c.unwrap_or_else(|| "0".into()));
        }
        let ev = self.last_ev.unwrap_or(Ev3 { x: self.tools.host.cam.w / 2.0, y: self.tools.host.cam.h / 2.0, ..Ev3::default() });
        if name == "move" && s.starts_with(['<', '[']) {
            let p0 = match &self.tools.tool {
                Tool::Move(m) => m.p0(),
                _ => None,
            };
            if p0.is_none() {
                // ws3d-core.js prints through `t`, which names the tool there: the JS throws
                let Some(c) = self.tools.sel_center() else { return Err("t is not a function".into()) };
                let copy = self.driving.as_ref().is_some_and(|d| d.1);
                self.tcall(doc, |t| t.call("begin", &[json!(c), json!(copy)]));
            }
        }
        if name == "rotate" && angle {
            let (stage, c1) = match &self.tools.tool {
                Tool::Rotate(r) => (r.stage, r.center()),
                _ => (2, [f64::NAN; 3]),
            };
            if stage < 2 {
                let c = if stage == 1 { Some(c1) } else { self.tools.sel_center() };
                let Some(c) = c else { return Err("t is not a function".into()) };
                let e = ev_json(&ev);
                self.tcall(doc, |t| t.call("startAt", &[json!(c), e]));
            }
        }
        let typed = if angle { s } else { convert_lengths(&s, &units) };
        let ok = self.tcall(doc, |t| t.value(&typed)).unwrap_or(false);
        if ok {
            self.check();
        }
        Ok(ok)
    }

    // ---- selection ----

    /// `selectAll()` in the open group → how many.
    pub fn select_all(&mut self) -> usize {
        let h = &mut self.tools.host;
        let Some(ctx) = h.model.context(&h.path) else { return 0 };
        let Some(def) = h.model.def(&ctx.def) else { return 0 };
        let mut s: Vec<Sel> = def.insts.iter().map(|(id, _)| Sel::new("i", id)).collect();
        s.extend(def.faces.iter().map(|(id, _)| Sel::new("f", id)));
        s.extend(def.edges.iter().filter(|(_, e)| !e.soft).map(|(id, _)| Sel::new("e", id)));
        h.sel = s;
        h.sel.len()
    }
    /// `clearSel()` → how many were selected.
    pub fn clear_sel(&mut self) -> usize {
        std::mem::take(&mut self.tools.host.sel).len()
    }
    /// `eraseSel()` → how many were erased.
    pub fn erase_sel(&mut self, doc: &mut Doc) -> usize {
        let sel: Vec<Sel> = self.tools.host.sel.iter().filter(|s| s.t != "g").cloned().collect();
        let guides: Vec<f64> = self.tools.host.sel.iter().filter(|s| s.t == "g").map(|s| s.id).collect();
        if sel.is_empty() && guides.is_empty() {
            return 0;
        }
        let path = self.tools.host.path.clone();
        self.tcall(doc, |t| {
            t.host.model.change("Erase", |m| {
                if let Some(c) = m.context(&path)
                    && let Some(d) = m.def_mut(&c.def)
                {
                    cc_m3d::ops::erase_sel(d, &sel);
                }
                for g in &guides {
                    m.guides.delete(*g);
                }
                m.purge();
                Ok(())
            })
        });
        self.tools.host.sel = Vec::new();
        sel.len().saturating_add(guides.len())
    }
}

/// The index after i in a ring of n.
fn nxt(i: usize, n: usize) -> usize {
    i.saturating_add(1).checked_rem(n).unwrap_or(0)
}

fn hash2(k: (u64, u64)) -> u64 {
    k.0.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ k.1
}

/// The shoelace area of a ring, as model3d.js `outline2d` sums it.
fn ring_area(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    let mut a = 0.0;
    for i in 0..n {
        let (Some(p), Some(q)) = (ring.get(i), ring.get(nxt(i, n))) else { continue };
        a += p[0] * q[1] - q[0] * p[1];
    }
    a.abs() / 2.0
}

// ---------------------------------------------------------------------------------------------------------------
// The engine's side: the workspace, clicks for the command line, the 3D tab opened and closed

impl Shared {
    fn faces_name(&self) -> String {
        self.t("model3d.faces_file")
    }
    /// commands3d.js `in3d(c)`: the 3D tab shows.
    pub fn in3d(&self) -> bool {
        let active = self.ws3d.borrow().as_ref().is_some_and(|w| w.active);
        // a sheet opened: the 3D tab hides (ws3d.js listens for the drawing's 'space')
        if active && !self.doc.borrow().in_model() {
            self.with_ws(|ws, _| ws.hide());
            return false;
        }
        active
    }
    /// Run `f` with the workspace (in step with the drawing) and the drawing; what it printed goes to the host after.
    /// None when there is no workspace.
    pub fn with_ws<R>(&self, f: impl FnOnce(&mut Ws3d, &mut Doc) -> R) -> Option<R> {
        let r = {
            let mut g = self.ws3d.borrow_mut();
            let ws = g.as_mut()?;
            let mut d = self.doc.borrow_mut();
            ws.sync(&d, &self.faces_name());
            let r = f(ws, &mut d);
            d.view3d = Some(ws.view_state());
            Some(r)
        };
        self.flush_ws();
        r
    }
    fn flush_ws(&self) {
        let out = match self.ws3d.try_borrow_mut() {
            Ok(mut g) => g.as_mut().map(|w| std::mem::take(&mut w.out)).unwrap_or_default(),
            Err(_) => return,
        };
        for m in out {
            match m {
                Msg::Text(s) => self.print(&s),
                Msg::Editing(n) => {
                    let s = self.tv("ws3d.editing_context", &[("name", n)]);
                    self.print(&s);
                }
            }
        }
    }
    /// cadcraft.js `open3d()`: the host switches to its 3D tab (false: it has none); the model space opens, the
    /// workspace is made the first time and shown.
    pub fn open3d(&self) -> bool {
        if self.in3d() {
            return true;
        }
        if !self.host.borrow_mut().open3d() {
            return false;
        }
        {
            let mut d = self.doc.borrow_mut();
            if !d.in_model() {
                d.set_space(None);
            }
        }
        if self.ws3d.borrow().is_none() {
            let ws = Ws3d::new(&self.doc.borrow(), &self.faces_name());
            *self.ws3d.borrow_mut() = ws.map(Box::new);
        }
        self.with_ws(|ws, d| ws.show(d));
        self.in3d()
    }
}

impl Engine {
    /// What the engine and its commands share (the Properties panel's drawing units call [`crate::cmd::solid3d::set_units`]).
    pub fn shared(&self) -> &Shared {
        &self.sh
    }
    /// The 3D tab shows.
    pub fn in3d(&self) -> bool {
        self.sh.in3d()
    }
    /// Open the 3D tab (the window's tab, or `need3d`): false when the host has none.
    pub fn open3d(&mut self) -> bool {
        self.sh.open3d()
    }
    /// Leave the 3D tab (another tab clicked).
    pub fn hide3d(&mut self) {
        self.sh.with_ws(|ws, _| ws.hide());
    }
    /// Work with the 3D tab's workspace (the window: camera, drawing, overlay): None when it was never opened.
    pub fn with_ws3d<R>(&self, f: impl FnOnce(&mut Ws3d, &mut Doc) -> R) -> Option<R> {
        self.sh.with_ws(f)
    }

    /// `inputMode()`
    fn ws_mode(&self) -> Mode {
        let st = self.sh.st.borrow();
        let Some(w) = st.waiting.as_ref() else { return if st.running.is_some() { Mode::None } else { Mode::Tool } };
        if w.opts.ws_tool {
            return Mode::Tool;
        }
        match w.kind {
            Kind::Point => Mode::Point,
            Kind::Number | Kind::Distance if w.opts.height.is_some() => Mode::Height,
            Kind::Distance | Kind::Angle => Mode::Flat,
            Kind::Select => Mode::Select2d,
            Kind::Entity => Mode::Entity2d,
            _ => Mode::None,
        }
    }

    /// A tool made its edit: the waiting tool prompt gets `done3d`.
    fn ws_check(&mut self) {
        let fin = self.sh.with_ws(|ws, _| ws.check()).unwrap_or(false);
        if !fin {
            return;
        }
        if self.sh.st.borrow().waiting.as_ref().is_some_and(|w| w.opts.ws_tool) {
            self.sh.with_ws(|ws, _| ws.done = false);
            self.resolve(Reply::Done3d);
        }
    }

    /// `down(ev)`
    pub fn ws_down(&mut self, ev: Ev3) {
        let mode = self.ws_mode();
        let go = self.sh.with_ws(|ws, d| {
            ws.last_ev = Some(ev);
            ws.mode = mode;
            ws.press_at = Some((ev.x, ev.y));
            if mode != Mode::Tool {
                return false;
            }
            let mut e = ev.ev();
            // copy: the move tool takes Ctrl at the base-point click as "copy"
            if ws.driving.as_ref().is_some_and(|x| x.1) && matches!(&ws.tools.tool, Tool::Move(m) if m.p0().is_none()) {
                e.ctrl = true;
            }
            // push/pull on a solid: open the solid (a group) the face belongs to, then let the tool pick the face
            if matches!(&ws.tools.tool, Tool::Push(p) if p.fid().is_none()) && ws.tools.host.path.is_empty() {
                ws.ensure_lines2d(d);
                let sc = ws.tools.host.scene();
                if let Some(s) = pick(&sc, &ws.tools.host.cam, ev.x, ev.y, &[], 7.0).filter(|s| s.t == "i") {
                    ws.tools.host.path = vec![s.id];
                    ws.tools.host.sel = Vec::new();
                    ws.opened_for_push = true;
                }
            }
            ws.tcall(d, |t| t.down(&e));
            true
        });
        if go == Some(true) {
            self.ws_check();
        }
    }

    /// `move(ev)`
    pub fn ws_move(&mut self, ev: Ev3) {
        let now = self.ws_mode();
        let mode = self.sh.with_ws(|ws, d| {
            ws.last_ev = Some(ev);
            ws.mouse = Some((ev.x, ev.y));
            let mode = if ws.press_at.is_some() { ws.mode } else { now };
            if mode == Mode::Tool {
                ws.tcall(d, |t| t.mv(&ev.ev()));
            }
            mode
        });
        if let Some(m) = mode.filter(|m| *m != Mode::Tool) {
            self.ws_hover(m, ev);
        }
    }

    /// `up(ev)`
    pub fn ws_up(&mut self, ev: Ev3) {
        let r = self.sh.with_ws(|ws, d| {
            ws.last_ev = Some(ev);
            let mode = ws.mode;
            let at = ws.press_at.take();
            if mode == Mode::Tool {
                ws.tcall(d, |t| t.up(&ev.ev()));
            }
            (mode, at)
        });
        let Some((mode, at)) = r else { return };
        if mode == Mode::Tool {
            self.ws_check();
            return;
        }
        let moved = at.map(|(x, y)| js::hypot(ev.x - x, ev.y - y));
        if let Some(a) = at
            && mode == Mode::Select2d
            && moved.is_some_and(|m| m > 6.0)
        {
            self.ws_window2d(a, ev);
            return;
        }
        if moved.is_none_or(|m| m > 6.0) {
            return;
        }
        self.ws_click_for(mode, ev);
    }

    /// `click(x, y, mods)`: down and up in place (tests, keyboard users).
    pub fn ws_click(&mut self, ev: Ev3) {
        self.ws_move(ev);
        self.ws_down(ev);
        self.ws_up(ev);
    }

    /// The z of a base point picked on the 3D tab (the JS points carry it).
    fn base_z(&self, base: Option<P2>) -> Option<f64> {
        let st = self.sh.st.borrow();
        let b = base?;
        st.z3_pts.iter().rev().find(|(p, _)| p.x == b.x && p.y == b.y).map(|(_, z)| *z)
    }

    /// `clickFor(mode, ev)`: a click answers the command line.
    fn ws_click_for(&mut self, mode: Mode, ev: Ev3) {
        let Some((kind, opts, _)) = self.waiting_info() else { return };
        let _ = kind;
        match mode {
            Mode::Point => {
                let p = self.sh.with_ws(|ws, d| ws.point_at(d, &ev)).flatten();
                if let Some(p) = p {
                    self.sh.st.borrow_mut().z3_pts.push((P2::new(p[0], p[1]), p[2]));
                    self.resolve_z(Reply::Point(P2::new(p[0], p[1]), None), Some(p[2]));
                }
            }
            Mode::Height => {
                let spec = opts.height.clone().unwrap_or_default();
                let h = self.sh.with_ws(|ws, d| ws.height_at(d, &ev, &spec)).flatten();
                if let Some(h) = h {
                    self.resolve(Reply::Num(h));
                }
            }
            Mode::Flat => {
                let st_base = self.sh.st.borrow().base;
                let z = self.base_z(opts.base).or_else(|| self.base_z(st_base)).unwrap_or(0.0);
                let g = self.sh.with_ws(|ws, d| ws.ground_at(d, ev.x, ev.y, z)).flatten();
                if let Some(g) = g {
                    let p = P2::new(g[0], g[1]);
                    self.click(p, None, p);
                }
            }
            Mode::Select2d | Mode::Entity2d => {
                let id = self.sh.with_ws(|ws, d| ws.pick2d(d, &ev)).flatten();
                let Some(id) = id else {
                    let m = self.sh.t("ws3d-core.nothing_found_there_click");
                    self.sh.print(&m);
                    return;
                };
                if mode == Mode::Entity2d {
                    let g = self.sh.with_ws(|ws, d| ws.ground_at(d, ev.x, ev.y, 0.0)).flatten();
                    let at = g.map_or(P2::new(f64::NAN, f64::NAN), |g| P2::new(g[0], g[1]));
                    self.resolve(Reply::Pick { id, at, shift: false });
                    return;
                }
                let set = self.with_set(|s| {
                    if s.has(id) && ev.shift {
                        s.delete(id);
                    } else {
                        s.add(id);
                    }
                });
                self.sh.host.borrow_mut().set_selection(&set.0);
                if opts.single {
                    self.resolve(Reply::Set(set));
                }
            }
            Mode::None | Mode::Tool => {}
        }
    }

    /// `hover(mode, ev)`: the cursor and the overlay while the command line asks.
    fn ws_hover(&mut self, mode: Mode, ev: Ev3) {
        let Some((_, opts, _)) = self.waiting_info() else {
            // nothing asked and no tool (a command between questions): the overlay goes
            self.sh.with_ws(|ws, _| ws.tools.host.overlay = None);
            return;
        };
        let st_base = self.sh.st.borrow().base;
        let base_z = self.base_z(opts.base).or_else(|| self.base_z(st_base));
        let ebase_z = self.base_z(st_base);
        let r = self.sh.with_ws(|ws, d| {
            let mut o = Overlay { lines: Some(Vec::new()), ..Overlay::default() };
            let mut cursor: Option<[f64; 3]> = None;
            match mode {
                Mode::Point => {
                    if let Some(r) = ws.infer_at(d, &ev) {
                        cursor = Some(Ws3d::to_units(r.p, Ws3d::k(d)));
                        o.marker = Some(Some(Marker { p: r.p, kind: r.kind.clone() }));
                        o.tip = Some(r.tip.clone());
                    }
                }
                Mode::Flat => cursor = ws.ground_at(d, ev.x, ev.y, base_z.unwrap_or(0.0)),
                Mode::Height => {
                    let spec = opts.height.clone().unwrap_or_default();
                    if let Some(h) = ws.height_at(d, &ev, &spec)
                        && let Some(l) = o.lines.as_mut()
                    {
                        Ws3d::prism_lines(d, l, &spec.rings, spec.z0, h);
                        o.tip = Some(fmt_num(h));
                    }
                }
                Mode::Select2d | Mode::Entity2d => ws.hover2d = ws.pick2d(d, &ev),
                Mode::None | Mode::Tool => {}
            }
            (o, cursor)
        });
        let Some((mut o, cursor)) = r else { return };
        if let Some(c) = cursor {
            let p = P2::new(c[0], c[1]);
            self.sh.host.borrow_mut().set_cursor(p);
            let z = ebase_z.unwrap_or(c[2]);
            let z = if z.is_finite() { z } else { 0.0 };
            let ents = self.sh.preview_at(p);
            let d = self.sh.doc.borrow();
            if let Some(l) = o.lines.as_mut() {
                for e in &ents {
                    Ws3d::entity_lines(&d, l, e, z, "axis", false);
                }
            }
        }
        let show = o.lines.as_ref().is_some_and(|l| !l.is_empty()) || o.marker.is_some() || o.tip.as_ref().is_some_and(|t| !t.is_empty());
        self.sh.with_ws(|ws, _| ws.tools.host.overlay = show.then_some(o));
    }

    /// `window2d(a, b)`: a drag while 2D objects are asked: the screen rectangle laid on the ground; left to right takes
    /// what is wholly inside (window), right to left what it touches (crossing). Shift takes them out again.
    fn ws_window2d(&mut self, a: (f64, f64), b: Ev3) {
        let Some((Kind::Select, opts, _)) = self.waiting_info() else { return };
        let crossing = b.x < a.0;
        let r = self.sh.with_ws(|ws, d| {
            let corners = [(a.0, a.1), (b.x, a.1), (b.x, b.y), (a.0, b.y)];
            let quad: Vec<Option<[f64; 3]>> = corners.iter().map(|(x, y)| ws.ground_at(d, *x, *y, 0.0)).collect();
            if quad.iter().any(Option::is_none) {
                return None;
            }
            let poly: Vec<P2> = quad.iter().flatten().map(|q| P2::new(q[0], q[1])).collect();
            let mut ids = Vec::new();
            for e in d.model() {
                if !d.editable(e) {
                    continue;
                }
                let mut pts: Vec<P2> = Vec::new();
                let mut segs: Vec<(P2, P2)> = Vec::new();
                {
                    let mut out = Gather { pts: &mut pts, segs: &mut segs };
                    walk(d, &[e], &mut out, &WalkOpts::default());
                }
                if pts.is_empty() {
                    continue;
                }
                let inside: Vec<bool> = pts.iter().map(|p| point_in_poly(*p, &poly)).collect();
                let take = if crossing {
                    inside.iter().any(|x| *x) || segs.iter().any(|(p, q)| crosses_poly(*p, *q, &poly))
                } else {
                    inside.iter().all(|x| *x)
                };
                if take && let Some(id) = e.id {
                    ids.push(id);
                }
            }
            Some(ids)
        });
        let Some(found) = r else { return };
        let Some(ids) = found else {
            let m = self.sh.t("ws3d.window.sky");
            self.sh.print(&m);
            return;
        };
        let set = self.with_set(|s| {
            for id in &ids {
                if b.shift {
                    s.delete(*id);
                } else {
                    s.add(*id);
                }
            }
        });
        self.sh.host.borrow_mut().set_selection(&set.0);
        let m = self.sh.tv("ws3d.window.found", &[("n", ids.len().to_string())]);
        self.sh.print(&m);
        if opts.single && !set.is_empty() {
            self.resolve(Reply::Set(set));
        }
    }

    /// ws3d.js `onKey` (the 3D tab's keys): arrows, Control, Alt and Shift to a busy tool; Esc stops the tool, closes
    /// the open group or drops the selection; Delete erases the 3D selection. → handled.
    pub fn ws_key(&mut self, key: &str, typing_text: bool) -> bool {
        let busy_engine = self.busy();
        let r = self.sh.with_ws(|ws, d| {
            let tool_busy = ws.tools.busy();
            if matches!(key, "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown") && tool_busy && !typing_text {
                ws.tcall(d, |t| t.key(key));
                return Some(true);
            }
            if matches!(key, "Control" | "Alt") && tool_busy {
                ws.tcall(d, |t| t.key(key));
                return Some(true);
            }
            if key == "Shift" && tool_busy {
                ws.tcall(d, |t| t.key("Shift"));
                return Some(false);
            }
            if key == "Escape" && !busy_engine {
                if tool_busy {
                    ws.tcall(d, Tools::escape);
                    return Some(true);
                }
                if let Some(leaving) = ws.tools.host.path.last().copied() {
                    ws.tools.host.path.pop();
                    ws.tools.host.sel = vec![Sel::new("i", leaving)];
                    return Some(true);
                }
                if !ws.tools.host.sel.is_empty() {
                    ws.clear_sel();
                    return Some(true);
                }
                return Some(false);
            }
            if matches!(key, "Delete" | "Backspace") && !typing_text && !busy_engine && !ws.tools.host.sel.is_empty() {
                return None;
            }
            Some(false)
        });
        match r {
            Some(Some(h)) => h,
            Some(None) => {
                self.run("ERASE");
                true
            }
            None => false,
        }
    }
    /// Shift let go (ws3d.js keyup).
    pub fn ws_key_up(&mut self, key: &str) {
        self.sh.with_ws(|ws, d| {
            if ws.active {
                ws.tcall(d, |t| t.key_up(key));
            }
        });
    }
}

/// window2d's walk: every point drawn, and the strokes' segments.
struct Gather<'a> {
    pts: &'a mut Vec<P2>,
    segs: &'a mut Vec<(P2, P2)>,
}
impl WalkOut for Gather<'_> {
    fn stroke(&mut self, _c: &str, prims: Vec<Prim>, _lw: f64, _o: Option<WOpts>) {
        for q in &prims {
            let s = seg_pts(q);
            for w in s.windows(2) {
                if let [a, b] = w {
                    self.segs.push((*a, *b));
                }
            }
            self.pts.extend(s);
        }
    }
    fn fill(&mut self, _c: &str, loops: Vec<Vec<f64>>, _solid: bool, _o: Option<WOpts>) {
        for l in &loops {
            for c in l.as_chunks::<2>().0 {
                self.pts.push(P2::new(c[0], c[1]));
            }
        }
    }
    fn text(&mut self, _c: &str, e: &Entity, _o: Option<WOpts>) {
        if let Geo::Text(t) = &e.geo {
            self.pts.push(P2::new(t.x, t.y));
        }
    }
    fn point(&mut self, _c: &str, x: f64, y: f64) {
        self.pts.push(P2::new(x, y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_lengths_are_drawing_units() {
        assert_eq!(with_unit("12", "millimeters"), "0.012m");
        assert_eq!(with_unit(" 2.5 ", "meters"), "2.5m");
        assert_eq!(with_unit("3'", "inches"), "3'");
        assert_eq!(convert_lengths("<1,2;3>", "meters"), "<1m,2m,3m>");
        assert_eq!(convert_lengths("10,20", ""), "0.01m,0.02m");
        assert_eq!(with_unit(".5", "meters"), "0.5m");
        assert_eq!(with_unit("5.", "meters"), "5m");
        assert_eq!(with_unit("-", "meters"), "-");
    }

    #[test]
    fn displacements() {
        assert_eq!(displacement("@1,2"), Some(("1".into(), "2".into(), None)));
        assert_eq!(displacement("1 , 2 ,3"), Some(("1".into(), "2".into(), Some("3".into()))));
        assert_eq!(displacement("@ 1,2,x"), None);
        assert_eq!(displacement("12"), None);
    }
}
