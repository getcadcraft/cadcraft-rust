//! PartCraft's sketch editor in the app (M13.3): the Rust form of 0.5's sketchview.js, drawn over the part view while a
//! sketch is open. Sketch coordinates map to the stage as `x = w/2 + (u − cx)·s`, `y = h/2 − (v − cy)·s`, as in the JS.
//!
//! Tools (the same gestures and words as 0.5): select (click, Shift+click adds, drag a point or curve with the solver
//! live, drag on empty space for a window, drag a dimension's label), line (click, click, … chains; Esc, Enter, right
//! click or double click ends), centerline, rect, circle, arc (centre, start, end — follows the way the pointer turns),
//! point, polygon, slot, spline / cspline, trim, extend, offset (B: both sides), mirror, linear / circular patterns,
//! fillet, dimension (click one or two items, then where the label goes; type the value), paste. The geometry work is
//! cc-part's sketch / sketch_tools (at parity with sketch.js / sketch-tools.js); each tool is one undo step.
//!
//! The editor never owns the part: every method takes the part view (`PartView`) as its host — `sketch()`,
//! `sketch_commit(label, …)`, `drag_start` / `drag_solve` / `drag_end`, `set_note` — the host functions sketchview.js
//! is given. Value boxes (a dimension's value, a polygon's sides, an offset's distance …) are `Ask`s the panel shows
//! and `part.sk.value` answers. Hit boxes of dimension labels use 7 px per character so a scripted click finds the same
//! label in the app and in the JS harness (forks/tools/pcuidiff.mjs measures text the same way).

// `!(x > 0.0)` is how the JS says "not above zero, or NaN"; 0.7071 is sketchview.js's own constant (kept for parity)
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::approx_constant)]

use cc_part::sketch::{self as S, At, Ent};
use cc_part::sketch_tools as TL;
use cc_part::{i18n, js, jv, spline as SP};
use serde_json::Value;

use crate::partview::{PartView, tr};

pub type P2 = [f64; 2];

const PICK: f64 = 7.0;
const SNAP_PT: f64 = 10.0;
const SNAP_CURVE: f64 = 7.0;
const PI: f64 = std::f64::consts::PI;

/// The sketch tools (sketchview.js `setTool` names) and their catalog words.
pub const TOOLS: &[(&str, &str)] = &[
    ("select", "ui.tb.sketch.tool_select"),
    ("line", "ui.tb.sketch.tool_line"),
    ("rect", "ui.tb.sketch.tool_rect"),
    ("circle", "ui.tb.sketch.tool_circle"),
    ("arc", "ui.tb.sketch.tool_arc"),
    ("point", "ui.tb.sketch.tool_point"),
    ("centerline", "ui.tb.sketch.tool_centerline"),
    ("fillet", "ui.tb.sketch.tool_fillet"),
    ("spline", "ui.tb.sketch.tool_spline"),
    ("trim", "ui.tb.sketch.tool_trim"),
    ("dimension", "ui.tb.sketch.tool_dimension"),
    ("cspline", "ui.tb.sketchmenu.tool_cspline"),
    ("extend", "ui.tb.sketchmenu.tool_extend"),
    ("offset", "ui.tb.sketchmenu.tool_offset"),
    ("mirror", "ui.tb.sketchmenu.tool_mirror"),
    ("polygon", "ui.tb.sketchmenu.tool_polygon"),
    ("slot", "ui.tb.sketchmenu.tool_slot"),
    ("lpattern", "ui.tb.sketchmenu.tool_lpattern"),
    ("cpattern", "ui.tb.sketchmenu.tool_cpattern"),
];

fn t(key: &str, vars: &[(&str, i18n::F)]) -> String {
    i18n::t(key, vars)
}
fn fs(s: impl Into<String>) -> i18n::F {
    i18n::F::S(s.into())
}
fn fnum(v: f64) -> i18n::F {
    i18n::F::N(v)
}
/// vec.js `fmt(n, dp)`.
pub fn fmt(n: f64, dp: usize) -> String {
    cc_part::vec::fmt(n, dp)
}

/// A snapped pointer position (sketchview.js `snap`): where, and the point (`rf`) or curve (`on`) it snapped to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sn {
    pub xy: P2,
    pub rf: Option<String>,
    pub on: Option<String>,
}

/// What is under the pointer: an entity or a relation / dimension label.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub constraint: bool,
    pub id: String,
    pub d: f64,
}

/// A tool in progress (sketchview.js `step`, one object with the fields the tools use).
#[derive(Clone, Debug, Default)]
pub struct Step {
    pub a: Option<Sn>,
    pub b: Option<Sn>,
    pub c: Option<Sn>,
    pub s: Option<Sn>,
    pub turn: f64,
    pub last_ang: Option<f64>,
    pub exclude: Vec<String>,
    pub pts: Vec<Sn>,
    pub picks: Vec<String>,
    pub axis: bool,
    pub chain: Option<TL::Chain>,
}

/// What a value box answers.
#[derive(Clone, Debug)]
pub enum AskJob {
    Polygon { c: Sn, v: P2 },
    Pattern { circular: bool, ids: Vec<String>, at: P2 },
    Offset { chain: TL::Chain, side: f64, both: bool },
    Fillet { point: String },
    Dimension { id: String, fresh: bool },
}

/// An open value box (sketchview.js `askValue`): where, the text in it, its label and what it does.
#[derive(Clone, Debug)]
pub struct Ask {
    pub at: [f64; 2],
    pub text: String,
    pub label: String,
    pub job: AskJob,
}

#[derive(Clone, Debug)]
enum DragKind {
    Label,
    Radius,
    Geom,
}
#[derive(Clone, Debug)]
struct Drag {
    kind: DragKind,
    id: String,
    pins: Vec<(String, P2)>,
    from: P2,
    start: [f64; 2],
    moved: bool,
}
#[derive(Clone, Debug)]
struct WinBox {
    a: [f64; 2],
    b: [f64; 2],
    add: bool,
}

/// A dimension label's or relation glyph's box on the stage (screen px).
#[derive(Clone, Debug, PartialEq)]
pub struct LabelBox {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// sketchview.js `SketchEditor`.
#[derive(Clone, Debug)]
pub struct SketchEd {
    pub view: (f64, f64, f64),
    /// the stage size (w, h) in points
    pub size: (f64, f64),
    /// a stage size set by `part.sk.view=cx,cy,s,w,h` (scripts): kept until the sketch closes, whatever the window
    pub size_pin: Option<(f64, f64)>,
    pub tool: String,
    /// selection in the order things were picked (a JS Set)
    pub sel: Vec<String>,
    pub hover: Option<Hit>,
    pub step: Option<Step>,
    pub cursor: Option<P2>,
    pub snap_at: Option<Sn>,
    pub active: bool,
    pub auto_relations: bool,
    pub paste_clip: Option<(Value, String)>,
    pub tool_sel: Vec<String>,
    pub offset_both: bool,
    pub poly_sides: Option<f64>,
    pub ask: Option<Ask>,
    drag: Option<Drag>,
    win: Option<WinBox>,
    press_hit: Option<Hit>,
    down_at: Option<[f64; 2]>,
    pan: Option<([f64; 2], (f64, f64, f64))>,
    /// the offset preview: (key, ghost sketch, made ids, error)
    offset_ghost: Option<(String, Value, Vec<String>, Option<String>)>,
}

impl Default for SketchEd {
    fn default() -> Self {
        SketchEd {
            view: (0.0, 0.0, 2.0),
            size: (800.0, 600.0),
            size_pin: None,
            tool: "select".into(),
            sel: Vec::new(),
            hover: None,
            step: None,
            cursor: None,
            snap_at: None,
            active: false,
            auto_relations: true,
            paste_clip: None,
            tool_sel: Vec::new(),
            offset_both: false,
            poly_sides: None,
            ask: None,
            drag: None,
            win: None,
            press_hit: None,
            down_at: None,
            pan: None,
            offset_ghost: None,
        }
    }
}

/// Pointer modifiers of an event.
#[derive(Clone, Copy, Debug, Default)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
}
impl Mods {
    fn add(self) -> bool {
        self.shift || self.ctrl
    }
}

fn has(v: &[String], id: &str) -> bool {
    v.iter().any(|x| x == id)
}
fn toggle(v: &mut Vec<String>, id: &str) {
    if let Some(i) = v.iter().position(|x| x == id) {
        v.remove(i);
    } else {
        v.push(id.to_string());
    }
}
fn add_once(v: &mut Vec<String>, id: &str) {
    if !has(v, id) {
        v.push(id.to_string());
    }
}

fn tool_hint(ed: &SketchEd, tool: &str) -> Option<String> {
    let k = match tool {
        "spline" => "ui.sketch.hint.spline",
        "cspline" => "ui.sketch.hint.cspline",
        "trim" => "ui.sketch.hint.trim",
        "extend" => "ui.sketch.hint.extend",
        "offset" => {
            if ed.tool_sel.len() > 1 {
                "ui.sketch.hint.offset_selected"
            } else {
                "ui.sketch.hint.offset"
            }
        }
        "mirror" => {
            if ed.tool_sel.is_empty() {
                "ui.sketch.hint.mirror"
            } else {
                "ui.sketch.hint.mirror_axis"
            }
        }
        "project" => "ui.sketch.hint.project",
        "polygon" => "ui.sketch.hint.polygon",
        "slot" => "ui.sketch.hint.slot",
        "lpattern" => {
            if ed.tool_sel.is_empty() {
                "ui.sketch.hint.lpattern_select"
            } else {
                "ui.sketch.hint.lpattern"
            }
        }
        "cpattern" => {
            if ed.tool_sel.is_empty() {
                "ui.sketch.hint.cpattern_select"
            } else {
                "ui.sketch.hint.cpattern"
            }
        }
        _ => return None,
    };
    Some(tr(k))
}

fn pick_tool(tool: &str) -> bool {
    matches!(tool, "trim" | "extend" | "offset" | "mirror")
}

impl SketchEd {
    // ------------------------------------------------------------------ geometry helpers
    pub fn to_screen(&self, p: P2) -> [f64; 2] {
        let (w, h) = self.size;
        let (cx, cy, s) = self.view;
        [w / 2.0 + (p[0] - cx) * s, h / 2.0 - (p[1] - cy) * s]
    }
    pub fn to_sketch(&self, x: f64, y: f64) -> P2 {
        let (w, h) = self.size;
        let (cx, cy, s) = self.view;
        [cx + (x - w / 2.0) / s, cy - (y - h / 2.0) / s]
    }

    /// `fit()`: the sketch (and the origin, and the dimension labels) in view.
    pub fn fit(&mut self, sk: &Value) {
        let (w, h) = self.size;
        let Some(b) = S::bounds(sk) else {
            self.view = (0.0, 0.0, self.view.2);
            return;
        };
        let (mut x0, mut y0, mut x1, mut y1) = (js::min(b[0], 0.0), js::min(b[1], 0.0), js::max(b[2], 0.0), js::max(b[3], 0.0));
        for c in S::constraints(sk) {
            if !S::is_dim(jv::s(c, "type")) {
                continue;
            }
            if let Some(g) = dim_geometry(sk, c)
                && let Some(at) = g.at
            {
                x0 = js::min(x0, at[0]);
                y0 = js::min(y0, at[1]);
                x1 = js::max(x1, at[0]);
                y1 = js::max(y1, at[1]);
            }
        }
        let s = js::min((w - 120.0) / js::max(x1 - x0, 1e-6), (h - 120.0) / js::max(y1 - y0, 1e-6));
        self.view = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, js::max(1e-4, js::min(s, 1e5)));
    }

    /// `setTool(tool)`.
    pub fn set_tool(&mut self, host: &mut PartView, tool: &str) {
        if tool != "paste" {
            self.paste_clip = None;
        }
        self.tool_sel = if matches!(tool, "mirror" | "lpattern" | "cpattern" | "offset") {
            let sk = host.sketch();
            self.sel.iter().filter(|id| id.as_str() != S::ORIGIN && S::by_id(sk, id).is_some()).cloned().collect()
        } else {
            Vec::new()
        };
        self.tool = tool.to_string();
        self.step = None;
        self.ask = None;
        if tool != "select" {
            self.sel.clear();
        }
        if tool == "mirror" && !self.tool_sel.is_empty() {
            self.step = Some(Step { axis: true, ..Default::default() });
        }
        if let Some(h) = tool_hint(self, tool) {
            host.set_note(&h);
        }
    }

    /// `runTool(label, fn, done)`: one undo step; the status says what it did and what it dropped.
    fn run_tool<T>(
        &mut self,
        host: &mut PartView,
        label: &str,
        f: impl FnOnce(&mut Value) -> Result<T, String>,
        done: impl FnOnce(&T) -> String,
        dropped: impl Fn(&T) -> usize,
    ) -> Option<T> {
        let r = match host.sketch_commit(label, false, f) {
            Ok(r) => r,
            Err(e) => {
                host.set_note(&e);
                return None;
            }
        };
        let n = dropped(&r);
        let extra = if n > 0 { format!(" {}", t("ui.sketch.dropped", &[("n", fnum(n as f64))])) } else { String::new() };
        let words = done(&r);
        host.set_note(&format!("{words}{extra}"));
        Some(r)
    }

    /// `startPaste(clip, label)`.
    pub fn start_paste(&mut self, host: &mut PartView, clip: Value, label: &str) {
        self.set_tool(host, "paste");
        self.paste_clip = Some((clip, label.to_string()));
        host.set_note(&tr("ui.sketch.paste.where"));
    }

    /// `R(sn)`: the point id when relations are made automatically, else the position.
    fn r(&self, sn: &Sn) -> At {
        match (&sn.rf, self.auto_relations) {
            (Some(id), true) => At::Id(id.clone()),
            _ => At::Xy(sn.xy),
        }
    }

    // ------------------------------------------------------------------ hit testing (screen px)
    /// The dimension label boxes and relation glyph boxes as `draw` puts them (`drawConstraints`).
    pub fn label_boxes(&self, sk: &Value, bad: &[String]) -> (Vec<LabelBox>, Vec<LabelBox>) {
        let mut dims = Vec::new();
        let mut glyphs = Vec::new();
        if !self.active {
            return (dims, glyphs);
        }
        let mut stack: Vec<(String, usize)> = Vec::new();
        for c in S::constraints(sk) {
            let ty = jv::s(c, "type");
            let id = jv::s(c, "id").to_string();
            if S::def_of(ty).is_none() {
                continue;
            }
            if S::is_dim(ty) {
                if let Some(g) = dim_geometry(sk, c)
                    && let Some(at) = g.at
                {
                    let [tx, ty2] = self.to_screen(at);
                    let tw = text_width(&g.text);
                    dims.push(LabelBox { id, x: tx - tw / 2.0 - 4.0, y: ty2 - 10.0, w: tw + 8.0, h: 19.0 });
                }
                continue;
            }
            let is_sel = has(&self.sel, &id);
            if ty == "coincident" && !is_sel && !has(bad, &id) {
                continue;
            }
            let Some(anchor) = glyph_anchor(sk, c) else { continue };
            let key = anchor.iter().map(|v| js::num_str(js::round(v * 1000.0))).collect::<Vec<_>>().join(",");
            let n = match stack.iter_mut().find(|(k, _)| *k == key) {
                Some((_, n)) => {
                    let m = *n;
                    *n += 1;
                    m
                }
                None => {
                    stack.push((key, 1));
                    0
                }
            };
            let [x, y] = self.to_screen(anchor);
            glyphs.push(LabelBox { id, x: x + 8.0 + n as f64 * 17.0, y: y + 6.0, w: 15.0, h: 15.0 });
        }
        (dims, glyphs)
    }

    /// `hitAt(x, y, { points, curves, labels })`.
    pub fn hit_at(&self, host: &PartView, x: f64, y: f64, points: bool, curves: bool, labels: bool) -> Option<Hit> {
        let sk = host.sketch();
        if labels {
            let (dims, glyphs) = self.label_boxes(sk, &host.bad_ids());
            for b in &dims {
                if x >= b.x - 2.0 && x <= b.x + b.w + 2.0 && y >= b.y - 2.0 && y <= b.y + b.h + 2.0 {
                    return Some(Hit { constraint: true, id: b.id.clone(), d: 0.0 });
                }
            }
            for b in &glyphs {
                if x >= b.x && x <= b.x + b.w && y >= b.y && y <= b.y + b.h {
                    return Some(Hit { constraint: true, id: b.id.clone(), d: 0.0 });
                }
            }
        }
        let mut best: Option<Hit> = None;
        if points {
            let ids = std::iter::once(S::ORIGIN.to_string())
                .chain(S::entities(sk).iter().filter(|e| jv::s(e, "type") == "point").map(|e| jv::s(e, "id").to_string()));
            for id in ids {
                let [px, py] = self.to_screen(S::pxy(sk, &id));
                let d = js::hypot2(px - x, py - y);
                if d <= PICK + 1.0 && best.as_ref().is_none_or(|b| d < b.d) {
                    best = Some(Hit { constraint: false, id, d });
                }
            }
            if best.is_some() {
                return best;
            }
        }
        if curves {
            let p = self.to_sketch(x, y);
            for e in S::entities(sk) {
                if jv::s(e, "type") == "point" {
                    continue;
                }
                let d = curve_dist(sk, &Ent::E(e), p) * self.view.2;
                if d <= PICK && best.as_ref().is_none_or(|b| d < b.d) {
                    best = Some(Hit { constraint: false, id: jv::s(e, "id").to_string(), d });
                }
            }
        }
        best
    }

    /// `snap(x, y, exclude)`: end points and the origin, then curves, else the plain position.
    pub fn snap(&self, sk: &Value, x: f64, y: f64, exclude: &[String]) -> Sn {
        let mut best: Option<(Sn, f64)> = None;
        let ids = std::iter::once(S::ORIGIN.to_string())
            .chain(S::entities(sk).iter().filter(|e| jv::s(e, "type") == "point").map(|e| jv::s(e, "id").to_string()));
        for id in ids {
            if has(exclude, &id) {
                continue;
            }
            let q = S::pxy(sk, &id);
            let [px, py] = self.to_screen(q);
            let d = js::hypot2(px - x, py - y);
            if d <= SNAP_PT && best.as_ref().is_none_or(|b| d < b.1) {
                best = Some((Sn { xy: q, rf: Some(id), on: None }, d));
            }
        }
        if let Some((s, _)) = best {
            return s;
        }
        let p = self.to_sketch(x, y);
        for e in S::entities(sk) {
            let id = jv::s(e, "id");
            if jv::s(e, "type") == "point" || has(exclude, id) {
                continue;
            }
            let en = Ent::E(e);
            let d = curve_dist(sk, &en, p) * self.view.2;
            if d <= SNAP_CURVE && best.as_ref().is_none_or(|b| d < b.1) {
                best = Some((Sn { xy: project(sk, &en, p), rf: None, on: Some(id.to_string()) }, d));
            }
        }
        best.map_or(Sn { xy: p, rf: None, on: None }, |b| b.0)
    }

    // ------------------------------------------------------------------ events
    /// `zoomAt(x, y, k)`.
    pub fn zoom_at(&mut self, x: f64, y: f64, k: f64) {
        let before = self.to_sketch(x, y);
        let s = js::max(1e-4, js::min(self.view.2 * k, 1e5));
        let (w, h) = self.size;
        self.view = (before[0] - (x - w / 2.0) / s, before[1] + (y - h / 2.0) / s, s);
    }

    /// Pointer down. `button` 0 primary, 1 middle, 2 secondary.
    pub fn down(&mut self, host: &mut PartView, x: f64, y: f64, button: u8, m: Mods) {
        if !self.active {
            return;
        }
        if button == 1 || button == 2 {
            if button == 2 && self.tool != "select" && self.step.is_some() {
                self.finish_step(host, false);
                return;
            }
            self.pan = Some(([x, y], self.view));
            return;
        }
        if button != 0 {
            return;
        }
        self.down_at = Some([x, y]);
        if self.tool == "select" {
            self.select_down(host, x, y, m);
        }
    }

    /// Pointer move (with or without a button down).
    pub fn move_to(&mut self, host: &mut PartView, x: f64, y: f64) {
        if !self.active {
            return;
        }
        if let Some((at, v)) = self.pan {
            self.view = (v.0 - (x - at[0]) / v.2, v.1 + (y - at[1]) / v.2, v.2);
            return;
        }
        let cur = self.to_sketch(x, y);
        self.cursor = Some(cur);
        host.coords = Some(cur);
        if self.tool == "select" {
            self.select_move(host, x, y);
            return;
        }
        let picking = pick_tool(&self.tool) && !(self.tool == "offset" && self.step.is_some());
        let exclude = self.step.as_ref().map(|s| s.exclude.clone()).unwrap_or_default();
        self.snap_at = if self.tool == "dimension" || picking { None } else { Some(self.snap(host.sketch(), x, y, &exclude)) };
        self.hover = if self.tool == "dimension" || self.tool == "fillet" {
            self.hit_at(host, x, y, true, self.tool == "dimension", false)
        } else if picking {
            self.hit_at(host, x, y, false, true, false)
        } else {
            None
        };
        if self.tool == "arc"
            && let Some(st) = self.step.as_mut()
            && let (Some(c), Some(s)) = (&st.c, &st.s)
        {
            let c = c.xy;
            let s0 = st.last_ang.unwrap_or_else(|| js::atan2(s.xy[1] - c[1], s.xy[0] - c[0]));
            let a = js::atan2(cur[1] - c[1], cur[0] - c[0]);
            let mut d = a - s0;
            while d > PI {
                d -= 2.0 * PI;
            }
            while d < -PI {
                d += 2.0 * PI;
            }
            st.turn += d;
            st.last_ang = Some(a);
        }
    }

    /// Pointer up: a drawing tool's click happens here.
    pub fn up(&mut self, host: &mut PartView, x: f64, y: f64, button: u8, m: Mods) {
        if !self.active {
            return;
        }
        if self.pan.is_some() {
            self.pan = None;
            return;
        }
        if button != 0 {
            return;
        }
        if self.tool == "select" {
            self.select_up(host, x, y, m);
            return;
        }
        if self.down_at.take().is_none() {
            return;
        }
        self.click(host, x, y);
    }

    /// Double click.
    pub fn dbl(&mut self, host: &mut PartView, x: f64, y: f64) {
        if !self.active {
            return;
        }
        if matches!(self.tool.as_str(), "line" | "spline" | "cspline") && self.step.is_some() {
            self.finish_step(host, false);
            return;
        }
        if let Some(h) = self.hit_at(host, x, y, false, false, true)
            && h.constraint
        {
            let is_dim =
                S::constraints(host.sketch()).iter().find(|c| jv::is_str(c.get("id"), &h.id)).is_some_and(|c| S::is_dim(jv::s(c, "type")));
            if is_dim {
                self.edit_dimension(host, &h.id, false);
            }
        }
    }

    /// `key(e)`: true when the editor used the key. `k` is the key's name (Escape, Enter, Delete, Backspace, or a letter).
    pub fn key(&mut self, host: &mut PartView, k: &str, shift: bool) -> bool {
        if !self.active {
            return false;
        }
        if k == "Escape" {
            if self.ask.is_some() {
                self.input_done(host, false);
                return true;
            }
            if self.step.is_some() && matches!(self.tool.as_str(), "offset" | "mirror" | "polygon" | "slot" | "lpattern" | "cpattern") {
                self.step = None;
                host.set_note("");
            } else if self.step.is_some() {
                self.finish_step(host, false);
            } else if self.tool != "select" {
                self.set_tool(host, "select");
            } else if !self.sel.is_empty() {
                self.sel.clear();
            } else {
                return false;
            }
            return true;
        }
        if k == "Enter" && (self.step.is_some() || self.tool == "mirror") {
            self.finish_step(host, false);
            return true;
        }
        if (k == "Delete" || k == "Backspace") && !self.sel.is_empty() {
            self.delete_selection(host);
            return true;
        }
        let lk = k.to_lowercase();
        if self.tool == "offset" && lk == "b" {
            self.offset_both = !self.offset_both;
            host.set_note(&tr(if self.offset_both { "ui.sketch.offset.both_on" } else { "ui.sketch.offset.both_off" }));
            return true;
        }
        let tool = match lk.as_str() {
            "l" => Some("line"),
            "n" => Some("centerline"),
            "r" => Some("rect"),
            "c" => Some("circle"),
            "a" => Some("arc"),
            "p" => Some("point"),
            "d" => Some("dimension"),
            "f" => Some("fillet"),
            "s" => Some("select"),
            "b" => Some("spline"),
            "t" => Some("trim"),
            "e" => Some("extend"),
            "o" => Some("offset"),
            "m" => Some("mirror"),
            _ => None,
        };
        if let Some(tl) = tool
            && !shift
        {
            self.set_tool(host, tl);
            return true;
        }
        if lk == "q" {
            self.toggle_construction(host);
            return true;
        }
        false
    }

    // ------------------------------------------------------------------ select tool
    fn select_down(&mut self, host: &mut PartView, x: f64, y: f64, m: Mods) {
        let h = self.hit_at(host, x, y, true, true, true);
        self.drag = None;
        if let Some(hh) = h.as_ref().filter(|h| h.constraint) {
            let sk = host.sketch();
            let is_dim = S::constraints(sk).iter().find(|c| jv::is_str(c.get("id"), &hh.id)).is_some_and(|c| S::is_dim(jv::s(c, "type")));
            if is_dim && self.label_boxes(sk, &host.bad_ids()).0.iter().any(|b| b.id == hh.id) {
                self.drag =
                    Some(Drag { kind: DragKind::Label, id: hh.id.clone(), pins: Vec::new(), from: [0.0; 2], start: [x, y], moved: false });
            }
            self.press_hit = h;
            return;
        }
        self.press_hit = h.clone();
        match h {
            Some(hh) if hh.id != S::ORIGIN => {
                let sk = host.sketch();
                let Some(e0) = S::by_id(sk, &hh.id) else { return };
                let at = self.to_sketch(x, y);
                let mut pins: Vec<(String, P2)> = Vec::new();
                if e0.ty() == "point" {
                    pins.push((e0.id().to_string(), [e0.num("x"), e0.num("y")]));
                } else if e0.ty() == "circle" && self.near_circle_edge(sk, &e0, at) {
                    self.drag =
                        Some(Drag { kind: DragKind::Radius, id: hh.id.clone(), pins: Vec::new(), from: at, start: [x, y], moved: false });
                } else {
                    for p in S::points_of(&e0) {
                        if !pins.iter().any(|(q, _)| *q == p) {
                            let xy = S::pxy(sk, &p);
                            pins.push((p, xy));
                        } else if let Some(slot) = pins.iter_mut().find(|(q, _)| *q == p) {
                            slot.1 = S::pxy(sk, &p);
                        }
                    }
                }
                if self.drag.is_none() {
                    self.drag = Some(Drag { kind: DragKind::Geom, id: hh.id, pins, from: at, start: [x, y], moved: false });
                }
            }
            Some(_) => {}
            None => {
                self.win = Some(WinBox { a: [x, y], b: [x, y], add: m.add() });
            }
        }
    }

    fn near_circle_edge(&self, sk: &Value, c: &Ent, p: P2) -> bool {
        let ce = S::pxy(sk, c.str("c"));
        (js::hypot2(p[0] - ce[0], p[1] - ce[1]) - c.num("r")).abs() * self.view.2 < PICK
    }

    fn select_move(&mut self, host: &mut PartView, x: f64, y: f64) {
        if let Some(mut d) = self.drag.take() {
            if !d.moved && js::hypot2(x - d.start[0], y - d.start[1]) < 3.0 {
                self.drag = Some(d);
                return;
            }
            if !d.moved {
                d.moved = true;
                host.drag_start(&tr(if matches!(d.kind, DragKind::Label) { "ui.sketch.op.move_dimension" } else { "ui.sketch.op.drag" }));
            }
            let p = self.to_sketch(x, y);
            match d.kind {
                DragKind::Label => host.with_sketch(|sk| {
                    let at = S::constraints(sk).iter().position(|c| jv::is_str(c.get("id"), &d.id));
                    if let Some(i) = at {
                        set_label_at(sk, i, p);
                    }
                }),
                DragKind::Radius => {
                    let sk = host.sketch();
                    let ce = S::by_id(sk, &d.id).map(|c| S::pxy(sk, c.str("c"))).unwrap_or([f64::NAN; 2]);
                    let mut radius = cc_part::omap::OMap::new();
                    radius.set(&d.id, js::hypot2(p[0] - ce[0], p[1] - ce[1]));
                    host.drag_solve(None, Some(&radius));
                }
                DragKind::Geom => {
                    let (dx, dy) = (p[0] - d.from[0], p[1] - d.from[1]);
                    let mut pins = cc_part::omap::OMap::new();
                    for (id, q) in &d.pins {
                        pins.set(id, [q[0] + dx, q[1] + dy]);
                    }
                    host.drag_solve(Some(&pins), None);
                }
            }
            self.drag = Some(d);
            return;
        }
        if let Some(b) = self.win.as_mut() {
            b.b = [x, y];
            return;
        }
        self.hover = self.hit_at(host, x, y, true, true, true);
    }

    fn select_up(&mut self, host: &mut PartView, _x: f64, _y: f64, m: Mods) {
        let add = m.add();
        if let Some(d) = self.drag.take()
            && d.moved
        {
            host.drag_end();
            return;
        }
        if let Some(b) = self.win.take()
            && js::hypot2(b.b[0] - b.a[0], b.b[1] - b.a[1]) > 4.0
        {
            if !b.add {
                self.sel.clear();
            }
            let p0 = self.to_sketch(js::min(b.a[0], b.b[0]), js::max(b.a[1], b.b[1]));
            let p1 = self.to_sketch(js::max(b.a[0], b.b[0]), js::min(b.a[1], b.b[1]));
            let inside = |q: P2| q[0] >= p0[0] && q[0] <= p1[0] && q[1] >= p0[1] && q[1] <= p1[1];
            let sk = host.sketch();
            let mut add_ids = Vec::new();
            for e in S::entities(sk) {
                let bb = entity_box(sk, &Ent::E(e));
                if inside([bb[0], bb[1]]) && inside([bb[2], bb[3]]) {
                    add_ids.push(jv::s(e, "id").to_string());
                }
            }
            for id in add_ids {
                add_once(&mut self.sel, &id);
            }
            return;
        }
        let h = self.press_hit.take();
        match h {
            None => {
                if !add {
                    self.sel.clear();
                }
            }
            Some(h) if add => toggle(&mut self.sel, &h.id),
            Some(h) => {
                self.sel.clear();
                self.sel.push(h.id);
            }
        }
    }

    // ------------------------------------------------------------------ drawing tools
    /// `click(x, y)`: what the active drawing tool does with a click.
    pub fn click(&mut self, host: &mut PartView, x: f64, y: f64) {
        let tl = self.tool.clone();
        let exclude = self.step.as_ref().map(|s| s.exclude.clone()).unwrap_or_default();
        let sn = if tl == "dimension" || tl == "fillet" { Sn::default() } else { self.snap(host.sketch(), x, y, &exclude) };
        let s = self.view.2;
        let dist = |a: P2, b: P2| js::hypot2(a[0] - b[0], a[1] - b[1]);
        if tl == "paste"
            && let Some((clip, label)) = self.paste_clip.clone()
        {
            let anchor = jv::arr(clip.get("anchor"));
            let an = [anchor.first().and_then(Value::as_f64).unwrap_or(0.0), anchor.get(1).and_then(Value::as_f64).unwrap_or(0.0)];
            let d = [sn.xy[0] - an[0], sn.xy[1] - an[1]];
            let made = host.sketch_commit(&label, false, |sk| Ok(cc_part::clipboard::paste_sketch(sk, &clip, d)));
            self.set_tool(host, "select");
            if let Ok(made) = made {
                self.sel = jv::arr(made.get("ids")).iter().map(|v| js::str_of(Some(v))).collect();
                host.pasted(&label, &clip, &made);
            }
            return;
        }
        match tl.as_str() {
            "point" => {
                let auto = self.auto_relations;
                let _ = host.sketch_commit(&tr("ui.sketch.op.point"), false, |sk| {
                    let id = S::add_point(sk, sn.xy[0], sn.xy[1], false);
                    join_snap(auto, sk, &id, &sn, false);
                    Ok(())
                });
            }
            "line" | "centerline" => {
                let Some(st) = self.step.as_ref() else {
                    self.step = Some(Step { a: Some(sn), ..Default::default() });
                    return;
                };
                let Some(a) = st.a.clone() else { return };
                let (b, hv) = self.hv_snap(&a, &sn);
                if dist(b.xy, a.xy) * s < 2.0 {
                    return;
                }
                let (ra, rb, auto) = (self.r(&a), self.r(&b), self.auto_relations);
                let cons = tl == "centerline";
                let label = tr(if tl == "line" { "ui.sketch.op.line" } else { "ui.sketch.op.centerline" });
                let made = host.sketch_commit(&label, false, |sk| {
                    let made = S::add_line(sk, &ra, &rb, cons)?;
                    if a.rf.is_none() {
                        join_snap(auto, sk, jv::s(&made, "p1"), &a, false);
                    }
                    if b.rf.is_none() {
                        join_snap(auto, sk, jv::s(&made, "p2"), &b, false);
                    }
                    if let Some(hv) = hv {
                        let _ = S::add_constraint(sk, hv, &[jv::s(&made, "id").to_string()], None, None);
                    }
                    Ok(made)
                });
                let Ok(made) = made else { return };
                if tl == "line" && b.rf.is_none() {
                    let p2 = jv::s(&made, "p2").to_string();
                    let xy = S::pxy(host.sketch(), &p2);
                    self.step = Some(Step {
                        a: Some(Sn { xy, rf: Some(p2.clone()), on: None }),
                        exclude: vec![jv::s(&made, "id").to_string(), p2],
                        ..Default::default()
                    });
                } else {
                    self.step = None;
                }
            }
            "rect" => {
                let Some(a) = self.step.as_ref().and_then(|st| st.a.clone()) else {
                    self.step = Some(Step { a: Some(sn), ..Default::default() });
                    return;
                };
                if (sn.xy[0] - a.xy[0]).abs() * s < 3.0 || (sn.xy[1] - a.xy[1]).abs() * s < 3.0 {
                    return;
                }
                let auto = self.auto_relations;
                let r = host.sketch_commit(&tr("ui.sketch.op.rectangle"), false, |sk| {
                    let r = S::add_rect(sk, a.xy, sn.xy, false)?;
                    let lines = jv::arr(r.get("lines"));
                    let p1_of = |sk: &Value, i: usize| {
                        let id = lines.get(i).map(|v| js::str_of(Some(v))).unwrap_or_default();
                        S::by_id(sk, &id).map(|l| l.str("p1").to_string()).unwrap_or_default()
                    };
                    let (q0, q2) = (p1_of(sk, 0), p1_of(sk, 2));
                    if a.rf.is_some() || a.on.is_some() {
                        join_snap(auto, sk, &q0, &a, true);
                    }
                    if sn.rf.is_some() || sn.on.is_some() {
                        join_snap(auto, sk, &q2, &sn, true);
                    }
                    Ok(())
                });
                if r.is_ok() {
                    self.step = None;
                }
            }
            "circle" => {
                let Some(c) = self.step.as_ref().and_then(|st| st.c.clone()) else {
                    self.step = Some(Step { c: Some(sn), ..Default::default() });
                    return;
                };
                let r = dist(sn.xy, c.xy);
                if r * s < 2.0 {
                    return;
                }
                let (rc, auto) = (self.r(&c), self.auto_relations);
                let res = host.sketch_commit(&tr("ui.sketch.op.circle"), false, |sk| {
                    let m = S::add_circle(sk, &rc, r, false)?;
                    if c.rf.is_none() {
                        join_snap(auto, sk, jv::s(&m, "c"), &c, false);
                    }
                    Ok(())
                });
                if res.is_ok() {
                    self.step = None;
                }
            }
            "arc" => {
                let Some(st) = self.step.as_mut() else {
                    self.step = Some(Step { c: Some(sn), ..Default::default() });
                    return;
                };
                let Some(c) = st.c.clone() else { return };
                let Some(s0) = st.s.clone() else {
                    if dist(sn.xy, c.xy) * s < 2.0 {
                        return;
                    }
                    st.s = Some(sn);
                    st.turn = 0.0;
                    st.last_ang = None;
                    return;
                };
                let cw = st.turn < 0.0;
                let (rc, rs, re, auto) = (self.r(&c), self.r(&s0), self.r(&sn), self.auto_relations);
                let res = host.sketch_commit(&tr("ui.sketch.op.arc"), false, |sk| {
                    let m = if cw { S::add_arc(sk, &rc, &re, &rs, false)? } else { S::add_arc(sk, &rc, &rs, &re, false)? };
                    if c.rf.is_none() {
                        join_snap(auto, sk, jv::s(&m, "c"), &c, false);
                    }
                    Ok(())
                });
                if res.is_ok() {
                    self.step = None;
                }
            }
            "spline" | "cspline" => self.spline_click(host, x, y, sn),
            "trim" | "extend" => {
                let Some(h) = self.hit_at(host, x, y, false, true, false) else {
                    if let Some(w) = tool_hint(self, &tl) {
                        host.set_note(&w);
                    }
                    return;
                };
                let at = self.to_sketch(x, y);
                if tl == "trim" {
                    self.run_tool(
                        host,
                        &tr("ui.sketch.op.trim"),
                        |sk| TL::trim(sk, &h.id, at),
                        |r| tr(if r.removed == Some("curve") { "ui.sketch.trim.whole" } else { "ui.sketch.trim.done" }),
                        |r| r.dropped,
                    );
                } else {
                    self.run_tool(
                        host,
                        &tr("ui.sketch.op.extend"),
                        |sk| TL::extend(sk, &h.id, at),
                        |_| tr("ui.sketch.extend.done"),
                        |r| r.dropped,
                    );
                }
                self.hover = None;
            }
            "offset" => self.offset_click(host, x, y),
            "project" => {
                // Spec gap: projecting a model edge needs the exact kernel's edge records (0.5 says the same without it)
                host.set_note(&tr("ui.sketch.hint.project"));
            }
            "mirror" => self.mirror_click(host, x, y),
            "polygon" => {
                let Some(c) = self.step.as_ref().and_then(|st| st.c.clone()) else {
                    self.step = Some(Step { c: Some(sn), ..Default::default() });
                    host.set_note(&tr("ui.sketch.polygon.corner"));
                    return;
                };
                if dist(sn.xy, c.xy) * s < 3.0 {
                    return;
                }
                self.step = None;
                let n = self.poly_sides.unwrap_or(6.0);
                self.ask_value([x, y], n, &tr("ui.sketch.polygon.sides"), AskJob::Polygon { c, v: sn.xy });
            }
            "slot" => {
                let Some(st) = self.step.as_mut() else {
                    self.step = Some(Step { a: Some(sn), ..Default::default() });
                    host.set_note(&tr("ui.sketch.slot.other_end"));
                    return;
                };
                let Some(a) = st.a.clone() else { return };
                let Some(b) = st.b.clone() else {
                    if dist(sn.xy, a.xy) * s < 3.0 {
                        return;
                    }
                    st.b = Some(sn);
                    host.set_note(&tr("ui.sketch.slot.width"));
                    return;
                };
                self.step = None;
                let w = 2.0 * line_dist(sn.xy, a.xy, b.xy);
                let auto = self.auto_relations;
                self.run_tool(
                    host,
                    &tr("ui.sketch.op.slot"),
                    |sk| {
                        let r = TL::add_slot(sk, a.xy, b.xy, w)?;
                        if auto {
                            let cs = jv::arr(r.get("centres")).iter().map(|v| js::str_of(Some(v))).collect::<Vec<_>>();
                            for (q, cid) in [(&a, cs.first()), (&b, cs.get(1))] {
                                if let (Some(rf), Some(cid)) = (&q.rf, cid) {
                                    S::add_constraint(sk, "coincident", &[cid.clone(), rf.clone()], None, None)?;
                                }
                            }
                        }
                        Ok(r)
                    },
                    |_| t("ui.sketch.slot.done", &[("w", fs(fmt(w, 2)))]),
                    |_| 0,
                );
            }
            "lpattern" | "cpattern" => {
                let ids = self.tool_sel.clone();
                if ids.is_empty() {
                    if let Some(w) = tool_hint(self, &tl) {
                        host.set_note(&w);
                    }
                    return;
                }
                if tl == "cpattern" {
                    self.ask_value([x, y], 6.0, &tr("ui.sketch.pattern.copies"), AskJob::Pattern { circular: true, ids, at: sn.xy });
                    return;
                }
                let Some(a) = self.step.as_ref().and_then(|st| st.a.clone()) else {
                    self.step = Some(Step { a: Some(sn), ..Default::default() });
                    host.set_note(&tr("ui.sketch.pattern.first_copy"));
                    return;
                };
                self.step = None;
                let d = [sn.xy[0] - a.xy[0], sn.xy[1] - a.xy[1]];
                if js::hypot2(d[0], d[1]) * s < 3.0 {
                    return;
                }
                self.ask_value([x, y], 3.0, &tr("ui.sketch.pattern.copies"), AskJob::Pattern { circular: false, ids, at: d });
            }
            "dimension" => self.dimension_click(host, x, y),
            "fillet" => {
                let h = self.hit_at(host, x, y, true, false, false);
                let Some(h) = h.filter(|h| h.id != S::ORIGIN) else {
                    host.set_note(&tr("ui.sketch.fillet.pick"));
                    return;
                };
                let sk = host.sketch();
                let lines: Vec<&Value> = S::entities(sk)
                    .iter()
                    .filter(|e| {
                        jv::s(e, "type") == "line"
                            && !jv::truthy(e.get("construction"))
                            && [jv::s(e, "p1"), jv::s(e, "p2")].iter().any(|p| same_pos(sk, p, &h.id))
                    })
                    .collect();
                if lines.len() != 2 {
                    host.set_note(&tr("ui.sketch.fillet.two_lines"));
                    return;
                }
                let len = js::min_of(lines.iter().map(|l| {
                    let (a, b) = (S::pxy(sk, jv::s(l, "p1")), S::pxy(sk, jv::s(l, "p2")));
                    js::hypot2(b[0] - a[0], b[1] - a[1])
                }));
                let guess = nice_number(len / 5.0);
                let at = self.to_screen(S::pxy(sk, &h.id));
                self.ask_value(at, guess, &tr("ui.sketch.fillet.radius"), AskJob::Fillet { point: h.id });
            }
            _ => {}
        }
    }

    /// `hvSnap(a, b)`: a line within 3° of horizontal or vertical snaps to it.
    fn hv_snap(&self, a: &Sn, b: &Sn) -> (Sn, Option<&'static str>) {
        if b.rf.is_some() || !self.auto_relations {
            return (b.clone(), None);
        }
        let (dx, dy) = (b.xy[0] - a.xy[0], b.xy[1] - a.xy[1]);
        let ang = (js::atan2(dy, dx) * 180.0 / PI).abs();
        if js::min(ang, 180.0 - ang) < 3.0 {
            return (Sn { xy: [b.xy[0], a.xy[1]], rf: b.rf.clone(), on: None }, Some("horizontal"));
        }
        if (ang - 90.0).abs() < 3.0 {
            return (Sn { xy: [a.xy[0], b.xy[1]], rf: b.rf.clone(), on: None }, Some("vertical"));
        }
        (b.clone(), None)
    }

    /// `finishStep(closed)`.
    pub fn finish_step(&mut self, host: &mut PartView, closed: bool) {
        let tl = self.tool.clone();
        let st = self.step.take();
        if (tl == "spline" || tl == "cspline")
            && let Some(st) = st.as_ref().filter(|s| s.pts.len() >= 2)
        {
            self.commit_spline(host, &st.pts, if tl == "cspline" { "control" } else { "fit" }, closed);
            return;
        }
        if tl == "mirror" && st.is_none() && !self.tool_sel.is_empty() {
            self.step = Some(Step { axis: true, ..Default::default() });
            if let Some(w) = tool_hint(self, "mirror") {
                host.set_note(&w);
            }
            return;
        }
        host.set_note("");
    }

    // ------------------------------------------------------------------ spline, offset, mirror, dimension
    fn spline_click(&mut self, host: &mut PartView, x: f64, y: f64, sn: Sn) {
        let Some(st) = self.step.as_ref() else {
            self.step = Some(Step { pts: vec![sn], ..Default::default() });
            return;
        };
        let (Some(f0), Some(l0)) = (st.pts.first(), st.pts.last()) else { return };
        let first = self.to_screen(f0.xy);
        let last = self.to_screen(l0.xy);
        if st.pts.len() >= 3 && js::hypot2(first[0] - x, first[1] - y) <= SNAP_PT {
            self.finish_step(host, true);
            return;
        }
        if js::hypot2(last[0] - x, last[1] - y) < 3.0 {
            return;
        }
        if let Some(st) = self.step.as_mut() {
            st.pts.push(sn);
        }
    }

    fn commit_spline(&mut self, host: &mut PartView, pts: &[Sn], kind: &str, closed: bool) {
        let auto = self.auto_relations;
        let label = tr(if kind == "control" { "ui.sketch.op.cspline" } else { "ui.sketch.op.spline" });
        let pts = pts.to_vec();
        let kind_s = kind.to_string();
        self.run_tool(
            host,
            &label,
            |sk| {
                let mut ids = Vec::new();
                for q in &pts {
                    let id = S::add_point(sk, q.xy[0], q.xy[1], false);
                    if auto && let Some(rf) = &q.rf {
                        S::add_constraint(sk, "coincident", &[id.clone(), rf.clone()], None, None)?;
                    } else {
                        join_snap(auto, sk, &id, q, false);
                    }
                    ids.push(At::Id(id));
                }
                let mut opts = jv::obj(vec![("kind", kind_s.as_str().into())]);
                if closed {
                    jv::set(&mut opts, "closed", true.into());
                }
                S::add_spline(sk, &ids, &opts)
            },
            |_| {
                tr(match (closed, kind == "fit") {
                    (true, true) => "ui.sketch.spline.closed_fit",
                    (true, false) => "ui.sketch.spline.closed_control",
                    (false, true) => "ui.sketch.spline.fit",
                    (false, false) => "ui.sketch.spline.control",
                })
            },
            |_| 0,
        );
    }

    fn offset_click(&mut self, host: &mut PartView, x: f64, y: f64) {
        let Some(chain) = self.step.as_ref().and_then(|s| s.chain.clone()) else {
            let Some(h) = self.hit_at(host, x, y, false, true, false) else {
                if let Some(w) = tool_hint(self, "offset") {
                    host.set_note(&w);
                }
                return;
            };
            let limit = (self.tool_sel.len() > 1 && has(&self.tool_sel, &h.id)).then(|| self.tool_sel.clone());
            match TL::chain_of(host.sketch(), &h.id, limit.as_deref()) {
                Ok(c) => self.step = Some(Step { chain: Some(c), ..Default::default() }),
                Err(e) => {
                    host.set_note(&e);
                    return;
                }
            }
            self.offset_ghost = None;
            host.set_note(&tr("ui.sketch.offset.side"));
            return;
        };
        let both = self.offset_both;
        let p = self.to_sketch(x, y);
        let side = match TL::offset_side(host.sketch(), &chain, p) {
            Ok(Some(s)) => s,
            Ok(None) => {
                host.set_note("Cannot read properties of undefined (reading 'd')");
                return;
            }
            Err(e) => {
                host.set_note(&e);
                return;
            }
        };
        let guess = js::prec_num(side.0, 3);
        let guess = if guess == 0.0 || guess.is_nan() { 1.0 } else { guess };
        self.ask_value([x, y], guess, &tr("ui.sketch.offset.distance"), AskJob::Offset { chain, side: side.1, both });
    }

    fn mirror_click(&mut self, host: &mut PartView, x: f64, y: f64) {
        if !self.step.as_ref().is_some_and(|s| s.axis) {
            let h = self.hit_at(host, x, y, true, true, false);
            let Some(h) = h.filter(|h| h.id != S::ORIGIN) else {
                if let Some(w) = tool_hint(self, "mirror") {
                    host.set_note(&w);
                }
                return;
            };
            toggle(&mut self.tool_sel, &h.id);
            host.set_note(&t("ui.sketch.mirror.items", &[("n", fnum(self.tool_sel.len() as f64))]));
            return;
        }
        let h = self.hit_at(host, x, y, false, true, false);
        let axis = h.and_then(|h| S::by_id(host.sketch(), &h.id).filter(|e| e.ty() == "line").map(|e| e.id().to_string()));
        let Some(axis) = axis else {
            host.set_note(&tr("ui.sketch.mirror.axis"));
            return;
        };
        let ids = self.tool_sel.clone();
        let r = self.run_tool(
            host,
            &tr("ui.sketch.op.mirror"),
            |sk| TL::mirror(sk, &ids, &axis),
            |m| t("ui.sketch.mirror.done", &[("n", fnum(m.1 as f64))]),
            |_| 0,
        );
        if let Some((made, _)) = r {
            self.set_tool(host, "select");
            let sk = host.sketch();
            self.sel = made.into_iter().filter(|id| S::by_id(sk, id).is_some_and(|e| e.ty() != "point")).collect();
        }
    }

    fn dimension_click(&mut self, host: &mut PartView, x: f64, y: f64) {
        let h = self.hit_at(host, x, y, true, true, false);
        let st = self.step.get_or_insert_with(Step::default);
        if let Some(h) = h.filter(|h| !h.constraint)
            && st.picks.len() < 2
            && !has(&st.picks, &h.id)
        {
            st.picks.push(h.id);
            let n = st.picks.len();
            host.set_note(&tr(if n == 1 { "ui.sketch.dim.second" } else { "ui.sketch.dim.place" }));
            return;
        }
        if st.picks.is_empty() {
            host.set_note(&tr("ui.sketch.dim.pick"));
            return;
        }
        let refs = st.picks.clone();
        let at = self.to_sketch(x, y);
        let ty = pick_dim_type(host.sketch(), &refs, Some(at));
        self.step = None;
        let Some(ty) = ty else {
            host.set_note(&tr("ui.sketch.dim.cannot"));
            return;
        };
        let made = host.sketch_commit(&tr("ui.sketch.op.dimension"), false, |sk| {
            let c = S::add_constraint(sk, ty, &refs, None, None)?;
            let id = jv::s(&c, "id").to_string();
            if let Some(i) = S::constraints(sk).iter().position(|k| jv::is_str(k.get("id"), &id)) {
                set_label_at(sk, i, at);
            }
            Ok(id)
        });
        match made {
            Ok(id) => {
                host.set_note("");
                self.edit_dimension(host, &id, true);
            }
            Err(e) => host.set_note(&e),
        }
    }

    /// `editDimension(id, fresh)`: the value box over a dimension; `fresh` = the value joins the dimension's undo step.
    pub fn edit_dimension(&mut self, host: &mut PartView, id: &str, fresh: bool) {
        let sk = host.sketch();
        let Some(c) = S::constraints(sk).iter().find(|k| jv::is_str(k.get("id"), id)) else { return };
        let label = S::label_of(jv::s(c, "type")).unwrap_or_default();
        let value = jv::n(c, "value");
        let boxes = self.label_boxes(sk, &host.bad_ids()).0;
        let at = match boxes.iter().find(|b| b.id == id) {
            Some(b) => [b.x + b.w / 2.0, b.y + b.h / 2.0],
            None => self.to_screen(dim_geometry(sk, c).and_then(|g| g.at).unwrap_or([0.0, 0.0])),
        };
        self.ask_value(at, value, &label, AskJob::Dimension { id: id.to_string(), fresh });
    }

    fn ask_value(&mut self, at: [f64; 2], value: f64, label: &str, job: AskJob) {
        self.ask = Some(Ask { at, text: fmt(value, 4), label: label.to_string(), job });
    }

    /// `inputDone(apply)`: Enter (apply) or Esc on the value box. The box's text is `ask.text`.
    pub fn input_done(&mut self, host: &mut PartView, apply: bool) {
        let Some(ask) = self.ask.take() else { return };
        if !apply {
            return;
        }
        let raw = ask.text.trim().to_string();
        if let AskJob::Dimension { id, .. } = &ask.job
            && host.dim_typed(id, &raw)
        {
            return;
        }
        let Some(v) = parse_number(&raw) else {
            host.set_note(&tr("ui.sketch.not_number"));
            return;
        };
        self.answer(host, ask.job, v);
    }

    fn answer(&mut self, host: &mut PartView, job: AskJob, v: f64) {
        match job {
            AskJob::Polygon { c, v: corner } => {
                if !js::is_integer(v) || !(3.0..=64.0).contains(&v) {
                    host.set_note(&tr("ui.sketch.polygon.bad_sides"));
                    return;
                }
                self.poly_sides = Some(v);
                let auto = self.auto_relations;
                self.run_tool(
                    host,
                    &tr("ui.sketch.op.polygon"),
                    |sk| {
                        let r = TL::add_polygon(sk, c.xy, corner, v)?;
                        if let Some(rf) = c.rf.as_ref().filter(|_| auto) {
                            let circ = js::str_of(r.get("circle"));
                            let cc = S::by_id(sk, &circ).map(|e| e.str("c").to_string()).unwrap_or_default();
                            S::add_constraint(sk, "coincident", &[cc, rf.clone()], None, None)?;
                        }
                        Ok(r)
                    },
                    |_| t("ui.sketch.polygon.done", &[("n", fnum(v))]),
                    |_| 0,
                );
            }
            AskJob::Pattern { circular, ids, at } => {
                if !js::is_integer(v) || !(2.0..=200.0).contains(&v) {
                    host.set_note(&tr("ui.sketch.pattern.bad_copies"));
                    return;
                }
                let label = tr(if circular { "ui.sketch.op.cpattern" } else { "ui.sketch.op.lpattern" });
                self.run_tool(
                    host,
                    &label,
                    |sk| if circular { TL::pattern_circular(sk, &ids, at, v, 360.0) } else { TL::pattern_linear(sk, &ids, at, v) },
                    |_| t("ui.sketch.pattern.done", &[("n", fnum(v))]),
                    |_| 0,
                );
            }
            AskJob::Offset { chain, side, both } => {
                if !(v > 0.0) {
                    host.set_note(&tr("ui.sketch.offset.positive"));
                    return;
                }
                self.run_tool(
                    host,
                    &tr("ui.sketch.op.offset"),
                    |sk| {
                        let mut r = TL::offset(sk, &chain, side * v, None)?;
                        if both {
                            let r2 = TL::offset(sk, &chain, -side * v, r.master.as_deref())?;
                            r.made.extend(r2.made);
                            r.err = js::max(r.err, r2.err);
                        }
                        Ok(r)
                    },
                    |r| {
                        if r.parametric {
                            t(if both { "ui.sketch.offset.done_both" } else { "ui.sketch.offset.done" }, &[("d", fs(fmt(v, 4)))])
                        } else {
                            t(
                                "ui.sketch.offset.done_spline",
                                &[("d", fs(fmt(v, 4))), ("tol", fnum(TL::OFFSET_TOL)), ("gap", fs(fmt(r.err, 6)))],
                            )
                        }
                    },
                    |_| 0,
                );
                self.step = None;
            }
            AskJob::Fillet { point } => {
                if let Err(e) = host.sketch_commit(&tr("ui.sketch.op.fillet"), false, |sk| S::fillet_corner(sk, &point, v)) {
                    host.set_note(&e);
                }
            }
            AskJob::Dimension { id, fresh } => {
                let ty = S::constraints(host.sketch()).iter().find(|k| jv::is_str(k.get("id"), &id)).map(|c| jv::s(c, "type").to_string());
                let Some(ty) = ty else { return };
                if !(v >= 0.0) || (ty == "angle" && v > 180.0) || ((ty == "radius" || ty == "diameter") && v <= 0.0) {
                    host.set_note(&tr(if ty == "angle" { "ui.sketch.dim.bad_angle" } else { "ui.sketch.dim.positive" }));
                    return;
                }
                let r = host.sketch_commit(&tr("ui.sketch.op.change_dimension"), fresh, |sk| {
                    if let Some(k) = S::constraints_mut(sk).and_then(|cs| cs.iter_mut().find(|k| jv::is_str(k.get("id"), &id))) {
                        jv::set(k, "value", jv::num(v));
                    }
                    Ok(())
                });
                if let Err(e) = r {
                    host.set_note(&e);
                }
            }
        }
    }

    // ------------------------------------------------------------------ edits
    /// `deleteSelection()`.
    pub fn delete_selection(&mut self, host: &mut PartView) {
        let sk = host.sketch();
        let cons: Vec<String> =
            self.sel.iter().filter(|id| S::constraints(sk).iter().any(|c| jv::is_str(c.get("id"), id))).cloned().collect();
        let ents: Vec<String> = self.sel.iter().filter(|id| !has(&cons, id) && id.as_str() != S::ORIGIN).cloned().collect();
        if cons.is_empty() && ents.is_empty() {
            return;
        }
        let _ = host.sketch_commit(&tr("ui.sketch.op.delete"), false, |sk| {
            S::remove_constraints(sk, &cons);
            S::remove_entities(sk, &ents);
            Ok(())
        });
        self.sel.clear();
    }

    /// `toggleConstruction()`.
    pub fn toggle_construction(&mut self, host: &mut PartView) {
        let sk = host.sketch();
        let ids: Vec<String> = self
            .sel
            .iter()
            .filter(|id| S::by_id(sk, id).is_some_and(|e| matches!(e, Ent::E(_)) && !jv::truthy(e.val("origin")) && e.ty() != "point"))
            .cloned()
            .collect();
        if ids.is_empty() {
            host.set_note(&tr("ui.sketch.construction.select"));
            return;
        }
        let _ = host.sketch_commit(&tr("ui.sketch.op.construction"), false, |sk| {
            for id in &ids {
                if let Some(e) = S::by_id_mut(sk, id) {
                    if jv::truthy(e.get("construction")) {
                        jv::del(e, "construction");
                    } else {
                        jv::set(e, "construction", true.into());
                    }
                }
            }
            Ok(())
        });
    }

    /// `addRelation(type, refs)`.
    pub fn add_relation(&mut self, host: &mut PartView, ty: &str, refs: &[String]) {
        let label = S::label_of(ty).unwrap_or_else(|| ty.to_string());
        if let Err(e) = host.sketch_commit(&label, false, |sk| S::add_constraint(sk, ty, refs, None, None)) {
            host.set_note(&e);
        }
    }

    /// What a drawing tool previews (for the painter): polylines in sketch coordinates and a tag.
    pub fn preview(&mut self, sk: &Value) -> Preview {
        let mut out = Preview::default();
        let Some(cur) = self.snap_at.as_ref().map(|s| s.xy).or(self.cursor) else { return out };
        let tool = self.tool.clone();
        let tl = tool.as_str();
        if tl == "select" {
            return out;
        }
        let step = self.step.clone();
        let circle = |c: P2, r: f64| -> Vec<P2> {
            (0..=64).map(|i| js::f(i) / 64.0 * 2.0 * PI).map(|a| [c[0] + r * a.cos(), c[1] + r * a.sin()]).collect()
        };
        match (tl, step.as_ref()) {
            ("line" | "centerline", Some(st)) => {
                if let Some(a) = &st.a {
                    let (b, hv) = self.hv_snap(a, self.snap_at.as_ref().unwrap_or(&Sn { xy: cur, rf: None, on: None }));
                    out.lines.push(vec![a.xy, b.xy]);
                    let len = js::hypot2(b.xy[0] - a.xy[0], b.xy[1] - a.xy[1]);
                    out.tag = Some((
                        b.xy,
                        match hv {
                            Some(h) => t(
                                if h == "horizontal" { "ui.sketch.tag.horizontal" } else { "ui.sketch.tag.vertical" },
                                &[("len", fs(fmt(len, 2)))],
                            ),
                            None => fmt(len, 2),
                        },
                    ));
                }
            }
            ("rect", Some(st)) => {
                if let Some(a) = &st.a {
                    let a = a.xy;
                    out.lines.push(vec![a, [cur[0], a[1]], cur, [a[0], cur[1]], a]);
                    out.tag = Some((cur, format!("{} × {}", fmt((cur[0] - a[0]).abs(), 2), fmt((cur[1] - a[1]).abs(), 2))));
                }
            }
            ("circle", Some(st)) => {
                if let Some(c) = &st.c {
                    let r = js::hypot2(cur[0] - c.xy[0], cur[1] - c.xy[1]);
                    out.lines.push(circle(c.xy, r));
                    out.tag = Some((cur, format!("⌀{}", fmt(2.0 * r, 2))));
                }
            }
            ("arc", Some(st)) => {
                if let Some(c) = &st.c {
                    match &st.s {
                        None => out.lines.push(vec![c.xy, cur]),
                        Some(s0) => {
                            let r = js::hypot2(s0.xy[0] - c.xy[0], s0.xy[1] - c.xy[1]);
                            let a0 = (s0.xy[1] - c.xy[1]).atan2(s0.xy[0] - c.xy[0]);
                            let a1 = (cur[1] - c.xy[1]).atan2(cur[0] - c.xy[0]);
                            let mut sweep = a1 - a0;
                            if st.turn >= 0.0 {
                                while sweep < 0.0 {
                                    sweep += 2.0 * PI;
                                }
                            } else {
                                while sweep > 0.0 {
                                    sweep -= 2.0 * PI;
                                }
                            }
                            out.lines.push(
                                (0..=48)
                                    .map(|i| a0 + sweep * js::f(i) / 48.0)
                                    .map(|a| [c.xy[0] + r * a.cos(), c.xy[1] + r * a.sin()])
                                    .collect(),
                            );
                        }
                    }
                }
            }
            ("spline" | "cspline", Some(st)) => {
                let mut q: Vec<P2> = st.pts.iter().map(|p| p.xy).collect();
                q.push(cur);
                if q.len() >= 2 {
                    let n = q.len();
                    if let (Some(l), Some(m)) = (q.get(n - 1), q.get(n - 2))
                        && js::hypot2(l[0] - m[0], l[1] - m[1]) < 1e-9
                    {
                        q.pop();
                    }
                }
                if q.len() >= 2 {
                    let c =
                        if tl == "spline" { SP::interp_fit(&q, false, None, None).ok() } else { Some(SP::control_curve(&q, false, None)) };
                    if let Some(c) = c {
                        out.lines.push(SP::tessellate(&c, 24));
                    }
                    if tl == "cspline" {
                        out.thin.push(q.clone());
                    }
                }
                out.dots.extend(st.pts.iter().map(|p| p.xy));
            }
            ("polygon", Some(st)) => {
                if let Some(c) = &st.c {
                    let n = self.poly_sides.unwrap_or(6.0);
                    let r = js::hypot2(cur[0] - c.xy[0], cur[1] - c.xy[1]);
                    let a0 = (cur[1] - c.xy[1]).atan2(cur[0] - c.xy[0]);
                    let k = js::count(n, 64).max(3);
                    let mut pts: Vec<P2> = (0..k)
                        .map(|i| a0 + 2.0 * PI * js::f(i) / js::f(k))
                        .map(|a| [c.xy[0] + r * a.cos(), c.xy[1] + r * a.sin()])
                        .collect();
                    if let Some(f) = pts.first().copied() {
                        pts.push(f);
                    }
                    out.lines.push(pts);
                    out.tag = Some((cur, t("ui.sketch.tag.polygon", &[("n", fnum(n)), ("r", fs(fmt(r, 2)))])));
                }
            }
            ("slot", Some(st)) => {
                if let Some(a) = &st.a {
                    let a = a.xy;
                    let b = st.b.as_ref().map_or(cur, |b| b.xy);
                    let l = js::hypot2(b[0] - a[0], b[1] - a[1]);
                    if st.b.is_none() || l < 1e-9 {
                        out.lines.push(vec![a, b]);
                    } else {
                        let r = line_dist(cur, a, b);
                        let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
                        let ang = u[0].atan2(-u[1]);
                        let mut pts = Vec::new();
                        for i in 0..=24 {
                            let th = ang - PI * js::f(i) / 24.0;
                            pts.push([b[0] + r * th.cos(), b[1] + r * th.sin()]);
                        }
                        for i in 0..=24 {
                            let th = ang + PI - PI * js::f(i) / 24.0;
                            pts.push([a[0] + r * th.cos(), a[1] + r * th.sin()]);
                        }
                        if let Some(f) = pts.first().copied() {
                            pts.push(f);
                        }
                        out.lines.push(pts);
                        out.tag = Some((cur, t("ui.sketch.tag.width", &[("w", fs(fmt(2.0 * r, 2)))])));
                    }
                }
            }
            ("lpattern", Some(st)) => {
                if let Some(a) = &st.a {
                    out.lines.push(vec![a.xy, cur]);
                }
            }
            ("offset", Some(st)) => {
                if let Some(chain) = &st.chain {
                    out.outline.extend(chain.items.iter().map(|(id, _)| id.clone()));
                    if let Ok(Some((d, side))) = TL::offset_side(sk, chain, cur) {
                        let key = format!("{side}:{}:{}", js::to_precision(d, 4), self.offset_both);
                        if self.offset_ghost.as_ref().is_none_or(|g| g.0 != key) {
                            let mut ghost = sk.clone();
                            let mut made = Vec::new();
                            let mut error = None;
                            match TL::offset(&mut ghost, chain, side * d, None) {
                                Ok(r) => made.extend(r.made),
                                Err(e) => error = Some(e),
                            }
                            if self.offset_both && error.is_none() {
                                match TL::offset(&mut ghost, chain, -side * d, None) {
                                    Ok(r) => made.extend(r.made),
                                    Err(e) => error = Some(e),
                                }
                            }
                            self.offset_ghost = Some((key, ghost, made, error));
                        }
                        if let Some((_, ghost, made, error)) = &self.offset_ghost {
                            for id in made {
                                if let Some(e) = S::by_id(ghost, id) {
                                    out.lines.push(entity_polyline(ghost, &e));
                                }
                            }
                            let tag = error.clone().unwrap_or_else(|| {
                                if self.offset_both { t("ui.sketch.tag.both_sides", &[("d", fs(fmt(d, 3)))]) } else { fmt(d, 3) }
                            });
                            out.tag = Some((cur, tag));
                        }
                    }
                }
            }
            ("dimension", Some(st)) => out.outline.extend(st.picks.iter().cloned()),
            _ => {}
        }
        if matches!(tl, "mirror" | "lpattern" | "cpattern") {
            out.outline.extend(self.tool_sel.iter().cloned());
        }
        if tl == "trim"
            && let Some(h) = self.hover.as_ref()
            && let Some(e) = S::by_id(sk, &h.id).filter(|e| e.ty() != "point")
            && let Some((a, b, hi, period)) = TL::trim_piece(sk, e.id(), cur)
            && let Ok(Some(cv)) = cc_part::sketch_intersect::curve_of(sk, &e)
        {
            let mut pts = Vec::new();
            for i in 0..=96 {
                let mut u = a + (b - a) * js::f(i) / 96.0;
                if period != 0.0 && u > hi {
                    u -= period;
                }
                pts.push(cc_part::sketch_intersect::point_on(&cv, u));
            }
            out.trim = Some(pts);
        }
        if matches!(tl, "extend" | "offset" | "mirror" | "dimension" | "fillet") && step.as_ref().is_none_or(|_| tl != "offset") {
            out.hover = self.hover.as_ref().map(|h| h.id.clone());
        }
        if let Some(sn) = self.snap_at.as_ref()
            && (sn.rf.is_some() || sn.on.is_some())
        {
            out.snap = Some((sn.xy, sn.rf.is_some()));
        }
        if tl == "paste"
            && let Some((clip, _)) = &self.paste_clip
        {
            let mut ghost = S::new_sketch();
            let anchor = jv::arr(clip.get("anchor"));
            let an = [anchor.first().and_then(Value::as_f64).unwrap_or(0.0), anchor.get(1).and_then(Value::as_f64).unwrap_or(0.0)];
            cc_part::clipboard::paste_sketch(&mut ghost, clip, [cur[0] - an[0], cur[1] - an[1]]);
            for e in S::entities(&ghost) {
                let en = Ent::E(e);
                if en.ty() == "point" {
                    continue;
                }
                out.lines.push(entity_polyline(&ghost, &en));
            }
            out.dots.extend(S::loose_points(&ghost).iter().map(|p| [jv::n(p, "x"), jv::n(p, "y")]));
        }
        out
    }

    /// The window box being dragged (screen px).
    pub fn window(&self) -> Option<([f64; 2], [f64; 2])> {
        self.win.as_ref().map(|b| (b.a, b.b))
    }
}

/// A tool preview (sketch coordinates).
#[derive(Clone, Debug, Default)]
pub struct Preview {
    pub lines: Vec<Vec<P2>>,
    pub thin: Vec<Vec<P2>>,
    pub dots: Vec<P2>,
    pub outline: Vec<String>,
    pub hover: Option<String>,
    pub trim: Option<Vec<P2>>,
    pub snap: Option<(P2, bool)>,
    pub tag: Option<(P2, String)>,
}

/// The text width the label hit boxes use (7 px a character; the JS harness measures the same).
pub fn text_width(s: &str) -> f64 {
    s.encode_utf16().count() as f64 * 7.0
}

/// `joinSnap(sk, id, sn, alsoPoint)`: join a new point to what it snapped to.
fn join_snap(auto: bool, sk: &mut Value, id: &str, sn: &Sn, also_point: bool) {
    if !auto {
        return;
    }
    if let (Some(rf), true) = (&sn.rf, also_point) {
        let _ = S::add_constraint(sk, "coincident", &[id.to_string(), rf.clone()], None, None);
    } else if let Some(on) = &sn.on {
        let _ = S::add_constraint(sk, "coincident", &[id.to_string(), on.clone()], None, None);
    }
}

// ---------------------------------------------------------------------- pure helpers (sketchview.js)

/// `parseNumber(s)`: "12", "⌀8", "R5", "3mm", "45°" → a number.
pub fn parse_number(s: &str) -> Option<f64> {
    let mut t: String = s.chars().filter(|c| !js::is_ws(*c)).collect();
    if let Some(c) = t.chars().next()
        && matches!(c, '⌀' | 'Ø' | 'ø' | 'R' | 'r')
    {
        t = t.chars().skip(1).collect();
    }
    let lower = t.to_lowercase();
    if lower.ends_with("mm") {
        t = t.chars().take(t.chars().count().saturating_sub(2)).collect();
    } else if t.ends_with('°') {
        t = t.chars().take(t.chars().count().saturating_sub(1)).collect();
    }
    // /^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$/i
    let b = t.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'-' | b'+')) {
        i += 1;
    }
    let digits = |i: &mut usize| {
        let s0 = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        *i - s0
    };
    let int = digits(&mut i);
    if int > 0 {
        if b.get(i) == Some(&b'.') {
            i += 1;
            digits(&mut i);
        }
    } else {
        if b.get(i) != Some(&b'.') {
            return None;
        }
        i += 1;
        if digits(&mut i) == 0 {
            return None;
        }
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'-' | b'+')) {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return None;
        }
    }
    if i != b.len() {
        return None;
    }
    let v = js::str_num(&t);
    v.is_finite().then_some(v)
}

/// `niceNumber(v)`: one significant figure.
pub fn nice_number(v: f64) -> f64 {
    if !(v > 0.0) {
        return 1.0;
    }
    let p = js::pow(10.0, js::log10(v).floor());
    js::round(v / p) * p
}

fn same_pos(sk: &Value, a: &str, b: &str) -> bool {
    let (p, q) = (S::pxy(sk, a), S::pxy(sk, b));
    js::hypot2(p[0] - q[0], p[1] - q[1]) < 1e-6 * js::max(1.0, S::extent(sk))
}

/// `lineDist(p, a, b)`.
pub fn line_dist(p: P2, a: P2, b: P2) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = js::hypot2(dx, dy);
    if l > 0.0 { (dx * (p[1] - a[1]) - dy * (p[0] - a[0])).abs() / l } else { js::hypot2(p[0] - a[0], p[1] - a[1]) }
}

fn spline_nearest(sk: &Value, e: &Ent, p: P2) -> Option<P2> {
    let c = S::spline_curve(sk, e).ok()?;
    Some(SP::point_at(&c, SP::closest_param(&c, p, 12)))
}

/// `curveDist(sk, e, p)`.
pub fn curve_dist(sk: &Value, e: &Ent, p: P2) -> f64 {
    match e.ty() {
        "spline" => spline_nearest(sk, e, p).map_or(f64::INFINITY, |q| js::hypot2(p[0] - q[0], p[1] - q[1])),
        "line" => {
            let (a, b) = (S::pxy(sk, e.str("p1")), S::pxy(sk, e.str("p2")));
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dy * dy;
            let l2 = if l2 == 0.0 || l2.is_nan() { 1e-300 } else { l2 };
            let tt = js::max(0.0, js::min(1.0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2));
            js::hypot2(p[0] - a[0] - tt * dx, p[1] - a[1] - tt * dy)
        }
        "circle" => {
            let c = S::pxy(sk, e.str("c"));
            (js::hypot2(p[0] - c[0], p[1] - c[1]) - e.num("r")).abs()
        }
        "arc" => {
            let (c, r, a0, sweep) = S::arc_angles(sk, e);
            let mut a = js::atan2(p[1] - c[1], p[0] - c[0]) - a0;
            let mut guard = 0;
            while a < 0.0 && guard < 64 {
                a += 2.0 * PI;
                guard += 1;
            }
            if a <= sweep {
                return (js::hypot2(p[0] - c[0], p[1] - c[1]) - r).abs();
            }
            let (s, q) = (S::pxy(sk, e.str("s")), S::pxy(sk, e.str("e")));
            js::min(js::hypot2(p[0] - s[0], p[1] - s[1]), js::hypot2(p[0] - q[0], p[1] - q[1]))
        }
        _ => f64::INFINITY,
    }
}

/// `project(sk, e, p)`: the nearest point of a curve.
pub fn project(sk: &Value, e: &Ent, p: P2) -> P2 {
    match e.ty() {
        "spline" => spline_nearest(sk, e, p).unwrap_or(p),
        "line" => {
            let (a, b) = (S::pxy(sk, e.str("p1")), S::pxy(sk, e.str("p2")));
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dy * dy;
            let l2 = if l2 == 0.0 || l2.is_nan() { 1e-300 } else { l2 };
            let tt = ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2;
            [a[0] + tt * dx, a[1] + tt * dy]
        }
        _ => {
            let c = S::pxy(sk, e.str("c"));
            let r = S::radius_of(sk, e);
            let d = js::hypot2(p[0] - c[0], p[1] - c[1]);
            let d = if d == 0.0 || d.is_nan() { 1.0 } else { d };
            [c[0] + (p[0] - c[0]) / d * r, c[1] + (p[1] - c[1]) / d * r]
        }
    }
}

/// `entityBox(sk, e)` → [x0, y0, x1, y1].
pub fn entity_box(sk: &Value, e: &Ent) -> [f64; 4] {
    let span = |q: &[P2]| -> [f64; 4] {
        [
            js::min_of(q.iter().map(|v| v[0])),
            js::min_of(q.iter().map(|v| v[1])),
            js::max_of(q.iter().map(|v| v[0])),
            js::max_of(q.iter().map(|v| v[1])),
        ]
    };
    match e.ty() {
        "point" => [e.num("x"), e.num("y"), e.num("x"), e.num("y")],
        "spline" => {
            let q = S::tess_spline(sk, e, 8).unwrap_or_else(|_| S::points_of(e).iter().map(|id| S::pxy(sk, id)).collect());
            span(&q)
        }
        "line" => span(&[S::pxy(sk, e.str("p1")), S::pxy(sk, e.str("p2"))]),
        "circle" => {
            let c = S::pxy(sk, e.str("c"));
            let r = e.num("r");
            span(&[[c[0] - r, c[1] - r], [c[0] + r, c[1] + r]])
        }
        _ => span(&S::tess_arc(sk, e, 32.0)),
    }
}

/// A curve as a polyline (drawing).
pub fn entity_polyline(sk: &Value, e: &Ent) -> Vec<P2> {
    match e.ty() {
        "line" => vec![S::pxy(sk, e.str("p1")), S::pxy(sk, e.str("p2"))],
        "circle" => {
            let mut p = S::tess_circle(S::pxy(sk, e.str("c")), e.num("r"), 96.0);
            if let Some(f) = p.first().copied() {
                p.push(f);
            }
            p
        }
        "arc" => S::tess_arc(sk, e, 96.0),
        "spline" => S::tess_spline(sk, e, 24).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// `pickDimType(sk, refs, at)`: which dimension a dimension-tool pick makes.
pub fn pick_dim_type(sk: &Value, refs: &[String], at: Option<P2>) -> Option<&'static str> {
    let ents: Vec<Ent> = refs.iter().map(|r| S::by_id(sk, r)).collect::<Option<Vec<_>>>()?;
    let mut kinds: Vec<char> = ents
        .iter()
        .map(|e| match e.ty() {
            "point" => 'P',
            "line" => 'L',
            "circle" => 'C',
            _ => 'A',
        })
        .collect();
    kinds.sort_unstable();
    let kinds: String = kinds.into_iter().collect();
    match kinds.as_str() {
        "C" => Some("diameter"),
        "A" => Some("radius"),
        "L" | "PP" => {
            let (a, b) = if kinds == "L" {
                let e = ents.first()?;
                (S::pxy(sk, e.str("p1")), S::pxy(sk, e.str("p2")))
            } else {
                (S::pxy(sk, refs.first()?), S::pxy(sk, refs.get(1)?))
            };
            let (x0, x1, y0, y1) = (js::min(a[0], b[0]), js::max(a[0], b[0]), js::min(a[1], b[1]), js::max(a[1], b[1]));
            let slanted = (b[0] - a[0]).abs() > 1e-9 && (b[1] - a[1]).abs() > 1e-9;
            if slanted && let Some(at) = at {
                if at[0] >= x0 && at[0] <= x1 && (at[1] > y1 || at[1] < y0) {
                    return Some("hdistance");
                }
                if at[1] >= y0 && at[1] <= y1 && (at[0] > x1 || at[0] < x0) {
                    return Some("vdistance");
                }
            }
            Some("distance")
        }
        "LP" => Some("distance"),
        "LL" => Some(if S::applicable(sk, refs).contains(&"angle") { "angle" } else { "distance" }),
        _ => None,
    }
}

fn refs_of(c: &Value) -> Vec<String> {
    jv::arr(c.get("refs")).iter().map(|v| js::str_of(Some(v))).collect()
}

/// `glyphAnchor(sk, c)`: where a relation's glyph sits.
pub fn glyph_anchor(sk: &Value, c: &Value) -> Option<P2> {
    let refs = refs_of(c);
    let e = refs.first().and_then(|r| S::by_id(sk, r));
    let target =
        if refs.len() > 1 && e.as_ref().is_some_and(|e| e.ty() == "point") { refs.get(1).and_then(|r| S::by_id(sk, r)) } else { e };
    let at = |x: Option<Ent>| -> Option<P2> {
        let x = x?;
        match x.ty() {
            "point" => Some([x.num("x"), x.num("y")]),
            "line" => {
                let (a, b) = (S::pxy(sk, x.str("p1")), S::pxy(sk, x.str("p2")));
                Some([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0])
            }
            "spline" => match S::spline_curve(sk, &x) {
                Ok(cv) => {
                    let [lo, hi] = SP::domain(&cv);
                    Some(SP::point_at(&cv, (lo + hi) / 2.0))
                }
                Err(_) => S::points_of(&x).first().map(|p| S::pxy(sk, p)),
            },
            _ => {
                let ce = S::pxy(sk, x.str("c"));
                let r = S::radius_of(sk, &x);
                Some([ce[0] + r * 0.7071, ce[1] + r * 0.7071])
            }
        }
    };
    let ty = jv::s(c, "type");
    if ty == "midpoint" || (ty == "concentric" && e.as_ref().is_some_and(|e| e.ty() == "point")) {
        return at(e);
    }
    if ty == "tangent"
        && refs.iter().any(|r| S::by_id(sk, r).is_some_and(|x| x.ty() == "spline"))
        && let (Some(a), Some(b)) = (refs.first().and_then(|r| S::by_id(sk, r)), refs.get(1).and_then(|r| S::by_id(sk, r)))
        && let Some(m) = S::end_meet(sk, &a, &b)
    {
        return Some(S::pxy(sk, &m.1));
    }
    if ty == "symmetric" {
        let (a, b) = (S::pxy(sk, refs.first()?), S::pxy(sk, refs.get(1)?));
        return Some([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]);
    }
    at(target)
}

/// `dimAnchor(sk, c)`: what a dimension's label is measured from.
pub fn dim_anchor(sk: &Value, c: &Value) -> Option<P2> {
    let ty = jv::s(c, "type");
    let r = refs_of(c);
    let (sig, _) = S::match_sig(sk, ty, &r).ok().flatten()?;
    let p = |i: usize| S::pxy(sk, r.get(i).map_or("", String::as_str));
    let e = |i: usize| S::by_id(sk, r.get(i).map_or("", String::as_str));
    let mid = |a: P2, b: P2| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    if ty == "radius" || ty == "diameter" || (ty == "offset" && sig == "RR") {
        return Some(S::pxy(sk, e(0)?.str("c")));
    }
    if ty == "angle" {
        let (l1, l2) = (e(0)?, e(1)?);
        let (a1, b1, a2, b2) = (S::pxy(sk, l1.str("p1")), S::pxy(sk, l1.str("p2")), S::pxy(sk, l2.str("p1")), S::pxy(sk, l2.str("p2")));
        let (d1, d2) = ([b1[0] - a1[0], b1[1] - a1[1]], [b2[0] - a2[0], b2[1] - a2[1]]);
        let den = d1[0] * d2[1] - d1[1] * d2[0];
        if den.abs() < 1e-12 {
            return Some(mid(a1, a2));
        }
        let tt = ((a2[0] - a1[0]) * d2[1] - (a2[1] - a1[1]) * d2[0]) / den;
        return Some([a1[0] + d1[0] * tt, a1[1] + d1[1] * tt]);
    }
    if sig == "L" {
        let l = e(0)?;
        return Some(mid(S::pxy(sk, l.str("p1")), S::pxy(sk, l.str("p2"))));
    }
    if sig == "PP" {
        return Some(mid(p(0), p(1)));
    }
    let q = if sig == "PL" { p(0) } else { S::pxy(sk, e(1)?.str("p1")) };
    let l = if sig == "PL" { e(1)? } else { e(0)? };
    Some(mid(q, mid(S::pxy(sk, l.str("p1")), S::pxy(sk, l.str("p2")))))
}

/// `setLabel(sk, c, p)` on constraint `i`: the label at p, stored relative to its anchor.
pub fn set_label_at(sk: &mut Value, i: usize, p: P2) {
    let Some(c) = S::constraints(sk).get(i) else { return };
    let Some(a) = dim_anchor(sk, c) else { return };
    if let Some(c) = S::constraints_mut(sk).and_then(|cs| cs.get_mut(i)) {
        jv::set(c, "off", jv::pt2([p[0] - a[0], p[1] - a[1]]));
        jv::del(c, "at");
    }
}

/// A dimension's drawing: lines, arcs (c, r, a0, a1), arrowheads (tip, direction), the text and where it goes.
#[derive(Clone, Debug, Default)]
pub struct DimGeo {
    pub lines: Vec<[P2; 2]>,
    pub arcs: Vec<(P2, f64, f64, f64)>,
    pub arrows: Vec<(P2, P2)>,
    pub text: String,
    pub at: Option<P2>,
}

/// `dimGeometry(sk, c)`.
pub fn dim_geometry(sk: &Value, c: &Value) -> Option<DimGeo> {
    let ty = jv::s(c, "type");
    let mut at: Option<P2> = jv::as_arr(c.get("at"))
        .map(|a| [a.first().and_then(Value::as_f64).unwrap_or(f64::NAN), a.get(1).and_then(Value::as_f64).unwrap_or(f64::NAN)]);
    if jv::truthy(c.get("off"))
        && let Some(a) = dim_anchor(sk, c)
    {
        let off = jv::arr(c.get("off"));
        let o = [off.first().and_then(Value::as_f64).unwrap_or(f64::NAN), off.get(1).and_then(Value::as_f64).unwrap_or(f64::NAN)];
        at = Some([a[0] + o[0], a[1] + o[1]]);
    }
    let r = refs_of(c);
    let (sig, _) = S::match_sig(sk, ty, &r).ok().flatten()?;
    let p = |i: usize| S::pxy(sk, r.get(i).map_or("", String::as_str));
    let e = |i: usize| S::by_id(sk, r.get(i).map_or("", String::as_str));
    let scale = js::max(1.0, S::extent(sk));
    let off = scale * 0.08;
    let mut out = DimGeo::default();
    let v = fmt(jv::n(c, "value"), 3);
    let unit = |d: P2| {
        let l = js::hypot2(d[0], d[1]);
        let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
        [d[0] / l, d[1] / l]
    };
    if ty == "radius" || ty == "diameter" {
        let en = e(0)?;
        let ce = S::pxy(sk, en.str("c"));
        let rad = S::radius_of(sk, &en);
        let mut dir = at.map_or([0.7071, 0.7071], |a| [a[0] - ce[0], a[1] - ce[1]]);
        if en.ty() == "arc" && at.is_none() {
            let (_, _, a0, sweep) = S::arc_angles(sk, &en);
            dir = [(a0 + sweep / 2.0).cos(), (a0 + sweep / 2.0).sin()];
        }
        let dir = unit(dir);
        let edge = [ce[0] + dir[0] * rad, ce[1] + dir[1] * rad];
        let at2 = at.unwrap_or([ce[0] + dir[0] * (rad + off), ce[1] + dir[1] * (rad + off)]);
        if ty == "radius" {
            out.lines.push([ce, edge]);
            out.lines.push([edge, at2]);
            out.arrows.push((edge, dir));
            out.text = format!("R{v}");
        } else {
            let back = [ce[0] - dir[0] * rad, ce[1] - dir[1] * rad];
            out.lines.push([back, edge]);
            out.lines.push([edge, at2]);
            out.arrows.push((edge, dir));
            out.arrows.push((back, [-dir[0], -dir[1]]));
            out.text = format!("⌀{v}");
        }
        out.at = Some(at2);
        return Some(out);
    }
    if ty == "angle" {
        let (l1, l2) = (e(0)?, e(1)?);
        let (a1, b1, a2, b2) = (S::pxy(sk, l1.str("p1")), S::pxy(sk, l1.str("p2")), S::pxy(sk, l2.str("p1")), S::pxy(sk, l2.str("p2")));
        let (d1, d2) = ([b1[0] - a1[0], b1[1] - a1[1]], [b2[0] - a2[0], b2[1] - a2[1]]);
        let den = d1[0] * d2[1] - d1[1] * d2[0];
        if den.abs() < 1e-12 {
            return None;
        }
        let tt = ((a2[0] - a1[0]) * d2[1] - (a2[1] - a1[1]) * d2[0]) / den;
        let x = [a1[0] + d1[0] * tt, a1[1] + d1[1] * tt];
        let dx = |q: P2| js::hypot2(q[0] - x[0], q[1] - x[1]);
        let far = |a: P2, b: P2| if dx(a) > dx(b) { a } else { b };
        let (f1, f2) = (far(a1, b1), far(a2, b2));
        let (ang1, ang2) = ((f1[1] - x[1]).atan2(f1[0] - x[0]), (f2[1] - x[1]).atan2(f2[0] - x[0]));
        let mut s = ang1;
        let mut d = ang2 - s;
        while d > PI {
            d -= 2.0 * PI;
        }
        while d < -PI {
            d += 2.0 * PI;
        }
        if d < 0.0 {
            s = ang2;
            d = -d;
        }
        let rad = match at {
            Some(a) => dx(a),
            None => {
                let r0 = js::min(dx(f1), dx(f2)) * 0.6;
                if r0 == 0.0 || r0.is_nan() { off } else { r0 }
            }
        };
        out.arcs.push((x, rad, s, s + d));
        let m = s + d / 2.0;
        out.at = Some(at.unwrap_or([x[0] + m.cos() * rad, x[1] + m.sin() * rad]));
        out.text = format!("{v}°");
        out.arrows.push(([x[0] + s.cos() * rad, x[1] + s.sin() * rad], [s.sin(), -s.cos()]));
        out.arrows.push(([x[0] + (s + d).cos() * rad, x[1] + (s + d).sin() * rad], [-(s + d).sin(), (s + d).cos()]));
        return Some(out);
    }
    if ty == "offset" && sig == "RR" {
        let (a, b) = (e(0)?, e(1)?);
        let ce = S::pxy(sk, a.str("c"));
        let (ra, rb) = (S::radius_of(sk, &a), S::radius_of(sk, &b));
        let mut dir = at.map_or([0.7071, 0.7071], |q| [q[0] - ce[0], q[1] - ce[1]]);
        if b.ty() == "arc" && at.is_none() {
            let (_, _, a0, sweep) = S::arc_angles(sk, &b);
            dir = [(a0 + sweep / 2.0).cos(), (a0 + sweep / 2.0).sin()];
        }
        let dir = unit(dir);
        let (pa, pb) = ([ce[0] + dir[0] * ra, ce[1] + dir[1] * ra], [ce[0] + dir[0] * rb, ce[1] + dir[1] * rb]);
        out.lines.push([pa, pb]);
        out.arrows.push((pb, [pb[0] - pa[0], pb[1] - pa[1]]));
        out.arrows.push((pa, [pa[0] - pb[0], pa[1] - pb[1]]));
        out.at = Some(at.unwrap_or([(pa[0] + pb[0]) / 2.0 + dir[0] * off * 0.3, (pa[1] + pb[1]) / 2.0 + dir[1] * off * 0.3]));
        out.text = v;
        return Some(out);
    }
    if (ty == "distance" || ty == "offset") && (sig == "PL" || sig == "LL") {
        let q0 = if sig == "PL" { p(0) } else { S::pxy(sk, e(1)?.str("p1")) };
        let l = if sig == "PL" { e(1)? } else { e(0)? };
        let (a, b) = (S::pxy(sk, l.str("p1")), S::pxy(sk, l.str("p2")));
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dy * dy;
        let l2 = if l2 == 0.0 || l2.is_nan() { 1e-300 } else { l2 };
        let tt = ((q0[0] - a[0]) * dx + (q0[1] - a[1]) * dy) / l2;
        let q = [a[0] + tt * dx, a[1] + tt * dy];
        out.lines.push([q0, q]);
        if !(0.0..=1.0).contains(&tt) {
            out.lines.push([if tt < 0.0 { a } else { b }, q]);
        }
        let u = [q[0] - q0[0], q[1] - q0[1]];
        out.arrows.push((q, u));
        out.arrows.push((q0, [-u[0], -u[1]]));
        let m = [(q0[0] + q[0]) / 2.0, (q0[1] + q[1]) / 2.0];
        out.at = Some(at.unwrap_or(m));
        if let Some(a) = at {
            out.lines.push([m, a]);
        }
        out.text = v;
        return Some(out);
    }
    let (a, b) = if sig == "L" {
        let l = e(0)?;
        (S::pxy(sk, l.str("p1")), S::pxy(sk, l.str("p2")))
    } else {
        (p(0), p(1))
    };
    if ty == "hdistance" || ty == "vdistance" {
        let k = usize::from(ty == "hdistance");
        let get = |q: P2| if k == 1 { q[1] } else { q[0] };
        let lvl = at.map_or(js::max(get(a), get(b)) + off, get);
        let (pa, pb) = if k == 1 { ([a[0], lvl], [b[0], lvl]) } else { ([lvl, a[1]], [lvl, b[1]]) };
        out.lines.push([a, pa]);
        out.lines.push([b, pb]);
        out.lines.push([pa, pb]);
        let u = [pb[0] - pa[0], pb[1] - pa[1]];
        out.arrows.push((pa, [-u[0], -u[1]]));
        out.arrows.push((pb, u));
        out.at = Some(at.unwrap_or([(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0]));
        out.text = v;
        return Some(out);
    }
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = js::hypot2(d[0], d[1]);
    let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
    let n = [-d[1] / l, d[0] / l];
    let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let o = at.map_or(off, |q| (q[0] - a[0]) * n[0] + (q[1] - a[1]) * n[1]);
    let (pa, pb) = ([a[0] + n[0] * o, a[1] + n[1] * o], [b[0] + n[0] * o, b[1] + n[1] * o]);
    out.lines.push([a, pa]);
    out.lines.push([b, pb]);
    out.lines.push([pa, pb]);
    out.arrows.push((pa, [-d[0], -d[1]]));
    out.arrows.push((pb, d));
    out.at = Some(at.unwrap_or([m[0] + n[0] * o, m[1] + n[1] * o]));
    out.text = v;
    Some(out)
}

/// `S.describe(c, f3)`: a relation or dimension in words, numbers to 3 places (partcraft.js `describeC`).
pub fn describe3(c: &Value) -> String {
    let ty = jv::s(c, "type");
    let Some(Ok(d)) = S::def_of(ty) else { return S::describe(c) };
    if !d.dim {
        return S::describe(c);
    }
    let v = fmt(jv::n(c, "value"), 3);
    match ty {
        "angle" => t("model.sketch.describe.angle", &[("value", fs(v))]),
        "diameter" => t("model.sketch.describe.diameter", &[("value", fs(v))]),
        "radius" => t("model.sketch.describe.radius", &[("value", fs(v))]),
        "offset" => t("model.sketch.describe.offset", &[("value", fs(v))]),
        _ => t("model.sketch.describe.dim", &[("label", fs(S::label_of(ty).unwrap_or_default())), ("value", fs(v))]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_as_the_value_box_reads_them() {
        assert_eq!(parse_number("12"), Some(12.0));
        assert_eq!(parse_number(" ⌀ 8 "), Some(8.0));
        assert_eq!(parse_number("R5mm"), Some(5.0));
        assert_eq!(parse_number("45°"), Some(45.0));
        assert_eq!(parse_number(".5e1"), Some(5.0));
        assert_eq!(parse_number("-3."), Some(-3.0));
        for bad in ["", "abc", "1e", "1.2.3", ".", "1e999", "12 mm x"] {
            assert_eq!(parse_number(bad), None, "{bad}");
        }
        assert_eq!(nice_number(23.0), 20.0);
        assert_eq!(nice_number(0.0), 1.0);
        assert_eq!(nice_number(f64::NAN), 1.0);
        assert_eq!(nice_number(0.27), 3.0 * 0.1, "round(2.7) × 0.1, as the JS computes it");
    }

    #[test]
    fn screen_and_sketch_round_trip() {
        let ed = SketchEd { view: (10.0, -5.0, 4.0), size: (800.0, 600.0), ..Default::default() };
        let p = ed.to_screen([12.5, 3.0]);
        assert_eq!(ed.to_sketch(p[0], p[1]), [12.5, 3.0]);
    }

    #[test]
    fn dimension_types_from_picks() {
        let mut sk = S::new_sketch();
        let l = S::add_line(&mut sk, &At::Xy([0.0, 0.0]), &At::Xy([10.0, 5.0]), false).unwrap();
        let c = S::add_circle(&mut sk, &At::Xy([30.0, 0.0]), 4.0, false).unwrap();
        let lid = jv::s(&l, "id").to_string();
        assert_eq!(pick_dim_type(&sk, std::slice::from_ref(&lid), Some([5.0, 20.0])), Some("hdistance"));
        assert_eq!(pick_dim_type(&sk, std::slice::from_ref(&lid), Some([20.0, 2.0])), Some("vdistance"));
        assert_eq!(pick_dim_type(&sk, &[lid], Some([5.0, 2.5])), Some("distance"));
        assert_eq!(pick_dim_type(&sk, &[jv::s(&c, "id").to_string()], None), Some("diameter"));
        assert_eq!(pick_dim_type(&sk, &["nope".into()], None), None);
    }
}
