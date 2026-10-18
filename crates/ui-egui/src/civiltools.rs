//! CivilCraft's drawing tools on the plan (M11.7): Alignment, Pad, Breakline, Boundary, Pipes, Feature line and Parcel,
//! as civilcraft.js has them — a click per point (`click`), Enter or a double click finishes (`finish`), Backspace drops
//! the last point, Esc cancels — and the grips of the selection (M11.8): PIs, pad corners, feature line vertices,
//! structures and the one selected survey point move by dragging, one undo step on release; Delete removes the
//! selection.
//!
//! Each finished draft becomes one `Project::change` (or `editPoints`) with the same JSON, labels, ids and words as
//! civilcraft.js `finishDraft`, features-ui.js `finish` and parcels-ui.js `finish`, so the saved project is the one the
//! 0.5 app saves after the same clicks (forks/tools/cvtooldiff.mjs checks that byte for byte).
//!
//! Screen tolerances are CSS pixels as in 0.5 (`nearestPoint(s, 14, …)`, the 3 px repeat guard, the 8 px close ring,
//! 7 px grips); distances are measured in f64 screen space from f64 world coordinates, so survey coordinates
//! (x ≈ 500 000, y ≈ 4 000 000) pick and snap to the millimetre at any zoom.

use cc_civil::i18n::t;
use cc_civil::jo;
use cc_civil::js::{J, JsMath, arr, hypot, jnum, numf, string_of};
use cc_civil::tr;
use cc_civil::units::fixed;
use serde_json::{Map, Value};

use crate::civilview::{CivilView, Sel};

/// A drawing tool (civilcraft.js `tool`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Select,
    Alignment,
    Pad,
    Breakline,
    Boundary,
    Pipe,
    Feature,
    Parcel,
}

/// The tools in toolbar order.
pub const TOOLS: [Tool; 8] =
    [Tool::Select, Tool::Alignment, Tool::Pad, Tool::Pipe, Tool::Breakline, Tool::Boundary, Tool::Feature, Tool::Parcel];

impl Tool {
    pub fn parse(s: &str) -> Option<Tool> {
        Some(match s {
            "select" => Tool::Select,
            "alignment" => Tool::Alignment,
            "pad" => Tool::Pad,
            "breakline" => Tool::Breakline,
            "boundary" => Tool::Boundary,
            "pipe" => Tool::Pipe,
            "feature" => Tool::Feature,
            "parcel" => Tool::Parcel,
            _ => return None,
        })
    }
    pub fn id(self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Alignment => "alignment",
            Tool::Pad => "pad",
            Tool::Breakline => "breakline",
            Tool::Boundary => "boundary",
            Tool::Pipe => "pipe",
            Tool::Feature => "feature",
            Tool::Parcel => "parcel",
        }
    }
    /// civilcraft.js `TOOL_HINT` (language keys).
    pub fn hint_key(self) -> Option<&'static str> {
        Some(match self {
            Tool::Select => return None,
            Tool::Alignment => "cv.hint.alignment",
            Tool::Pad => "cv.hint.pad",
            Tool::Breakline => "cv.hint.breakline",
            Tool::Boundary => "cv.hint.boundary",
            Tool::Pipe => "cv.hint.pipe",
            Tool::Feature => "fl.tool.hint",
            Tool::Parcel => "pc.hint",
        })
    }
    /// The toolbar label and tooltip keys (ui.inc).
    pub fn label_keys(self) -> (&'static str, &'static str) {
        match self {
            Tool::Select => ("ui.tb.select", "ui.tb.select.title"),
            Tool::Alignment => ("ui.tb.alignment", "ui.tb.alignment.title"),
            Tool::Pad => ("ui.tb.pad", "ui.tb.pad.title"),
            Tool::Breakline => ("ui.tb.breakline", "ui.tb.breakline.title"),
            Tool::Boundary => ("ui.tb.boundary", "ui.tb.boundary.title"),
            Tool::Pipe => ("ui.tb.pipe", "ui.tb.pipe.title"),
            Tool::Feature => ("toolbar.feature", "toolbar.feature.title"),
            Tool::Parcel => ("pc.draw", "pc.hint"),
        }
    }
    /// The single-key shortcut (civilcraft.js keydown `keyCmd`; the Parcel tool has none).
    pub fn key(self) -> Option<egui::Key> {
        Some(match self {
            Tool::Select => egui::Key::V,
            Tool::Alignment => egui::Key::A,
            Tool::Pad => egui::Key::G,
            Tool::Breakline => egui::Key::B,
            Tool::Boundary => egui::Key::O,
            Tool::Pipe => egui::Key::P,
            Tool::Feature => egui::Key::L,
            Tool::Parcel => return None,
        })
    }
}

/// A draft vertex (civilcraft.js `draft` entries: `{x, y}`, `{x, y, z, n}` from a survey point, `{x, y, sid}` on a
/// structure).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DraftPt {
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
    pub n: Option<String>,
    pub sid: Option<String>,
}

/// What a grip moves (civilcraft.js `gripAt` + the one-selected-point drag).
#[derive(Clone, Debug, PartialEq)]
pub enum Grip {
    Pi(String, usize),
    Feature(String, usize),
    Corner(String, usize),
    Structure(String),
    Point(String),
}

/// A grip being dragged: where it is now (world), whether it moved past the slack, Esc pressed.
#[derive(Clone, Debug, PartialEq)]
pub struct GripDrag {
    pub grip: Grip,
    pub from: (f64, f64),
    pub at: (f64, f64),
    pub press: (f64, f64),
    pub moved: bool,
    pub cancelled: bool,
}

/// The tool state of one open project.
#[derive(Clone, Debug, Default)]
pub struct Tools {
    pub tool: Tool,
    pub draft: Vec<DraftPt>,
    /// the Feature line tool closed its line on the first vertex (civilcraft.js `draftClosed`)
    pub closed: bool,
    pub drag: Option<GripDrag>,
    /// the survey point the Breakline tool would take (civilcraft.js `snapPt`)
    pub snap: Option<usize>,
}

/// civilcraft.js `DEFAULT_R`.
pub fn default_r(units: &str) -> f64 {
    if units == "m" { 30.0 } else { 100.0 }
}

/// civilcraft.js `fitRadius(pts, i, R)`: the default radius, or the largest whole radius whose curve fits half of each
/// tangent.
pub fn fit_radius(pts: &[DraftPt], i: usize, r: f64) -> f64 {
    let (Some(a), Some(b), Some(c)) = (i.checked_sub(1).and_then(|k| pts.get(k)), pts.get(i), pts.get(i + 1)) else { return 0.0 };
    let d = cc_civil::alignment::norm_ang((c.y - b.y).jatan2(c.x - b.x) - (b.y - a.y).jatan2(b.x - a.x)).abs();
    if d < 1e-6 || d > std::f64::consts::PI - 1e-3 {
        return 0.0;
    }
    let half = hypot(b.x - a.x, b.y - a.y).min(hypot(c.x - b.x, c.y - b.y)) / 2.0;
    r.min((half / (d / 2.0).jtan()).floor())
}

/// `nextId(prefix)` on a state being changed (the pipe tool takes ids inside its change).
fn next_id_in(state: &mut Value, prefix: &str) -> String {
    let seq = state.get("seq").map_or(f64::NAN, cc_civil::js::to_number);
    if let Some(m) = state.as_object_mut() {
        m.insert("seq".into(), jnum(seq + 1.0));
    }
    format!("{prefix}{}", cc_civil::js::num_to_string(seq))
}

fn list_mut<'a>(state: &'a mut Value, key: &str) -> Option<&'a mut Vec<Value>> {
    let m = state.as_object_mut()?;
    if !m.get(key).is_some_and(Value::is_array) {
        m.insert(key.to_string(), Value::Array(Vec::new()));
    }
    m.get_mut(key).and_then(Value::as_array_mut)
}

fn find_in<'a>(list: &'a [Value], id: &str) -> Option<&'a Value> {
    list.iter().find(|o| o.get("id").and_then(Value::as_str) == Some(id))
}

fn set_xy(o: &mut Value, x: f64, y: f64) {
    if let Some(m) = o.as_object_mut() {
        m.insert("x".into(), jnum(x));
        m.insert("y".into(), jnum(y));
    }
}

/// The grip half-size in CSS pixels (civilcraft.js `gripR`: 7, 14 on touch screens).
pub const GRIP_R: f64 = 7.0;

impl CivilView {
    // ------------------------------------------------------------------------------------------- screen helpers

    /// World → CSS pixels of the plan.
    pub fn css(&self, x: f64, y: f64) -> (f64, f64) {
        let (sx, sy) = self.view.to_screen(x, y);
        (sx / self.ppp, sy / self.ppp)
    }
    /// CSS pixels → world.
    pub fn world(&self, sx: f64, sy: f64) -> (f64, f64) {
        self.view.to_world(sx * self.ppp, sy * self.ppp)
    }
    /// view.js `viewBounds()` (min x, min y, max x, max y).
    pub fn view_bounds(&self) -> (f64, f64, f64, f64) {
        let (x0, y0) = self.view.to_world(0.0, self.view.height);
        let (x1, y1) = self.view.to_world(self.view.width, 0.0);
        (x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1))
    }

    /// civilcraft.js `nearestPoint(s, px, needZ)`: the survey point in view nearest the CSS point `s` within `px`.
    pub fn nearest_px(&self, s: (f64, f64), px: f64, need_z: bool) -> Option<usize> {
        let (bx0, by0, bx1, by1) = self.view_bounds();
        let zoom = (self.view.zoom / self.ppp).max(1e-300);
        let (wx, wy) = self.world(s.0, s.1);
        // a box test in world units first (a little wider than px), then JS's own screen distance
        let r = px / zoom * 1.0001 + 1e-9;
        let mut best = None;
        let mut bd = px;
        for (i, p) in self.project.points.iter().enumerate() {
            if need_z && p.z.is_none() {
                continue;
            }
            if (p.x - wx).abs() > r || (p.y - wy).abs() > r {
                continue;
            }
            if p.x < bx0 || p.x > bx1 || p.y < by0 || p.y > by1 {
                continue;
            }
            let q = self.css(p.x, p.y);
            let d = hypot(q.0 - s.0, q.1 - s.1);
            if d < bd {
                bd = d;
                best = Some(i);
            }
        }
        best
    }

    /// civilcraft.js `structAt(s, 10)`: the structure nearest the CSS point within 10 px → (id, name, x, y).
    pub fn struct_at(&self, s: (f64, f64)) -> Option<(String, String, f64, f64)> {
        let net = self.project.state.get("network")?;
        let mut best = None;
        let mut bd = 10.0;
        for x in arr(net.get("structures")) {
            let (px, py) = (numf(x.get("x")), numf(x.get("y")));
            let q = self.css(px, py);
            let d = hypot(q.0 - s.0, q.1 - s.1);
            if d < bd {
                bd = d;
                best = Some((string_of(x.get("id")), string_of(x.get("name")), px, py));
            }
        }
        best
    }

    // ------------------------------------------------------------------------------------------- tools

    /// civilcraft.js `setTool(tl)`.
    pub fn set_tool(&mut self, tl: Tool) {
        if tl != self.tools.tool {
            self.tools.draft.clear();
            self.tools.closed = false;
        }
        self.tools.tool = tl;
        self.tools.snap = None;
        if matches!(tl, Tool::Breakline | Tool::Boundary | Tool::Pad) && self.project.eg().surface.is_none() {
            self.message = t("cv.surface.need", &[]);
        }
    }

    /// civilcraft.js `command(c)` for a tool: the same tool again goes back to Select; the plan comes back from 3D.
    pub fn tool_command(&mut self, tl: Tool) {
        self.show3d = false;
        if tl == Tool::Select {
            self.set_tool(Tool::Select);
        } else {
            self.set_tool(if self.tools.tool == tl { Tool::Select } else { tl });
        }
    }

    /// A click on the plan at world (x, y) (civilcraft.js `click(w, s)`): Select picks, a drawing tool adds a point.
    pub fn click(&mut self, x: f64, y: f64) {
        let s = self.css(x, y);
        let tool = self.tools.tool;
        match tool {
            Tool::Select => {
                let zoom = self.view.zoom / self.ppp;
                self.sel = self.pick_at(x, y, zoom);
                self.profile.sel_pvi = None;
                if let Some(Sel::Alignment(id)) = &self.sel
                    && self.profile.open
                {
                    self.profile.want = Some(id.clone());
                }
            }
            Tool::Feature => {
                if self.close_ring_hit(s) {
                    self.tools.closed = true;
                    self.finish();
                    return;
                }
                match self.nearest_px(s, 10.0, true).and_then(|i| self.project.points.get(i)).cloned() {
                    Some(p) => {
                        let z = p.z.unwrap_or(f64::NAN);
                        self.tools.draft.push(DraftPt { x: p.x, y: p.y, z: p.z, n: Some(p.n.clone()), sid: None });
                        self.message = tr!("fl.click.point", n = p.n.as_str(), z = fixed(z, 2));
                    }
                    None => {
                        self.tools.draft.push(DraftPt { x, y, ..DraftPt::default() });
                        self.message = tr!("fl.click.ground", n = self.tools.draft.len());
                    }
                }
            }
            Tool::Breakline => {
                let Some(p) = self.nearest_px(s, 14.0, true).and_then(|i| self.project.points.get(i)).cloned() else {
                    self.message = t("cv.bl.click.need", &[]);
                    return;
                };
                if self.tools.draft.last().is_some_and(|l| l.n.as_deref() == Some(p.n.as_str())) {
                    return;
                }
                let z = p.z.unwrap_or(f64::NAN);
                self.tools.draft.push(DraftPt { x: p.x, y: p.y, z: p.z, n: Some(p.n.clone()), sid: None });
                self.message = tr!("cv.bl.click", n = p.n.as_str(), z = fixed(z, 2));
            }
            Tool::Parcel => {
                if self.close_ring_hit(s) {
                    self.finish();
                    return;
                }
                match self.nearest_px(s, 10.0, false).and_then(|i| self.project.points.get(i)).cloned() {
                    Some(p) => {
                        self.tools.draft.push(DraftPt { x: p.x, y: p.y, ..DraftPt::default() });
                        self.message = tr!("pc.click.point", n = p.n.as_str(), k = self.tools.draft.len());
                    }
                    None => {
                        self.tools.draft.push(DraftPt { x, y, ..DraftPt::default() });
                        self.message = tr!("pc.click", k = self.tools.draft.len());
                    }
                }
            }
            Tool::Pipe => {
                let st = self.struct_at(s);
                if let Some((id, _, _, _)) = &st
                    && self.tools.draft.last().is_some_and(|l| l.sid.as_deref() == Some(id.as_str()))
                {
                    return;
                }
                match st {
                    Some((id, name, sx, sy)) => {
                        self.tools.draft.push(DraftPt { x: sx, y: sy, sid: Some(id), ..DraftPt::default() });
                        self.message = tr!("cv.pp.joined", name = name);
                    }
                    None => {
                        self.tools.draft.push(DraftPt { x, y, ..DraftPt::default() });
                        self.message = t("cv.pp.placed", &[]);
                    }
                }
            }
            Tool::Alignment | Tool::Pad | Tool::Boundary => {
                if let Some(l) = self.tools.draft.last() {
                    let q = self.css(l.x, l.y);
                    if hypot(q.0 - s.0, q.1 - s.1) < 3.0 {
                        return;
                    }
                }
                self.tools.draft.push(DraftPt { x, y, ..DraftPt::default() });
            }
        }
    }

    /// The first vertex again closes a feature line or a parcel (3 or more vertices, within 8 px).
    fn close_ring_hit(&self, s: (f64, f64)) -> bool {
        if self.tools.draft.len() < 3 {
            return false;
        }
        let Some(p0) = self.tools.draft.first() else { return false };
        let q = self.css(p0.x, p0.y);
        hypot(q.0 - s.0, q.1 - s.1) < 8.0
    }

    /// A double click (the browser's two clicks, then `dblclick`): the second click's point goes, the draft finishes.
    pub fn double_click(&mut self, x: f64, y: f64) {
        self.click(x, y);
        self.click(x, y);
        if self.tools.tool != Tool::Select && !self.tools.draft.is_empty() {
            self.tools.draft.pop();
            self.finish();
        }
    }

    /// Backspace while drawing: the last point goes (civilcraft.js keydown says nothing).
    pub fn back(&mut self) {
        self.tools.draft.pop();
    }

    /// Esc (civilcraft.js keydown): a point drag is put back; a draft is dropped; otherwise Select and nothing selected.
    pub fn cancel(&mut self) {
        if let Some(d) = self.tools.drag.as_mut()
            && matches!(d.grip, Grip::Point(_))
        {
            d.cancelled = true;
            self.message = t("pt.drag.cancel", &[]);
            return;
        }
        if self.tools.draft.is_empty() {
            self.set_tool(Tool::Select);
            self.sel = None;
        } else {
            self.tools.draft.clear();
            self.tools.closed = false;
            self.message = t("cv.cancelled", &[]);
        }
    }

    /// Enter: civilcraft.js `finishDraft()` — the draft becomes one undo step (or stays, with what is missing said).
    pub fn finish(&mut self) {
        if self.tools.draft.is_empty() {
            return;
        }
        let pts = std::mem::take(&mut self.tools.draft);
        let units = self.project.units().to_string();
        match self.tools.tool {
            Tool::Select => {}
            Tool::Alignment => self.finish_alignment(&pts, &units),
            Tool::Pad => self.finish_pad(&pts),
            Tool::Pipe => self.finish_pipe(&pts, &units),
            Tool::Parcel => {
                if !self.finish_parcel(&pts, &units) {
                    self.tools.draft = pts;
                } else {
                    self.set_tool(Tool::Select);
                }
            }
            Tool::Feature => {
                let closed = self.tools.closed;
                self.tools.closed = false;
                if self.finish_feature(&pts, closed, &units) {
                    self.set_tool(Tool::Select);
                } else {
                    self.tools.draft = pts;
                }
            }
            Tool::Breakline => {
                if pts.len() < 2 {
                    self.message = t("cv.bl.need2", &[]);
                    return;
                }
                let n = arr(self.project.state.get("surface").and_then(|s| s.get("breaklines"))).len() + 1;
                let line = jo! { "name" => tr!("cv.bl.name", n = n),
                "pts" => Value::Array(pts.iter().map(|p| jo! { "x" => p.x, "y" => p.y, "z" => p.z }).collect()) };
                self.project.change(&t("cv.undo.bl.draw", &[]), &mut |s: &mut Value| {
                    if let Some(l) = s.get_mut("surface").and_then(|x| list_mut(x, "breaklines")) {
                        l.push(line.clone());
                    }
                });
                self.message = tr!("cv.bl.added", n = n);
            }
            Tool::Boundary => {
                if pts.len() < 3 {
                    self.message = t("cv.bnd.need3", &[]);
                    return;
                }
                let ring = Value::Array(pts.iter().map(|p| jo! { "x" => p.x, "y" => p.y }).collect());
                self.project.change(&t("cv.undo.bnd.draw", &[]), &mut |s: &mut Value| {
                    if let Some(m) = s.get_mut("surface").and_then(Value::as_object_mut) {
                        m.insert("boundary".into(), ring.clone());
                    }
                });
                self.message = t("cv.bnd.set", &[]);
            }
        }
    }

    fn finish_alignment(&mut self, pts: &[DraftPt], units: &str) {
        if pts.len() < 2 {
            self.message = t("cv.al.need2", &[]);
            return;
        }
        let r = default_r(units);
        let last = pts.len() - 1;
        let pis: Vec<Value> = pts
            .iter()
            .enumerate()
            .map(|(i, p)| jo! { "x" => p.x, "y" => p.y, "r" => if i == 0 || i == last { 0.0 } else { fit_radius(pts, i, r) } })
            .collect();
        let id = self.project.next_id("a");
        let n = arr(self.project.state.get("alignments")).len() + 1;
        let al = jo! { "id" => id, "name" => tr!("cv.al.name", n = n), "start" => 0.0, "pis" => Value::Array(pis), "pvis" => Value::Array(vec![]) };
        let a2 = al.clone();
        self.project.change(&t("cv.undo.al.draw", &[]), &mut |s: &mut Value| {
            if let Some(l) = list_mut(s, "alignments") {
                l.push(a2.clone());
            }
        });
        self.sel = Some(Sel::Alignment(id.clone()));
        self.profile.sel_pvi = None;
        self.set_tool(Tool::Select);
        let saved = find_in(arr(self.project.state.get("alignments")), &id).cloned().unwrap_or(al);
        let len = self.project.alignment(&saved).length;
        self.message = tr!("cv.al.drawn", n = n, len = fixed(len, 2), u = cc_civil::units::unit_of(units).short, r = r);
    }

    fn finish_pad(&mut self, pts: &[DraftPt]) {
        if pts.len() < 3 {
            self.message = t("cv.pad.need3", &[]);
            return;
        }
        let k = pts.len() as f64;
        let (mut cx, mut cy) = (0.0, 0.0);
        for p in pts {
            cx += p.x / k;
            cy += p.y / k;
        }
        let z0 = self.project.eg().surface.as_ref().and_then(|s| s.z_at(cx, cy)).unwrap_or(0.0);
        let id = self.project.next_id("p");
        let n = arr(self.project.state.get("pads")).len() + 1;
        let pad = jo! { "id" => id, "name" => tr!("cv.pad.name", n = n),
        "pts" => Value::Array(pts.iter().map(|p| jo! { "x" => p.x, "y" => p.y }).collect()),
        "z" => cc_civil::js::fixed_num(z0, 1), "cut" => 2.0, "fill" => 3.0 };
        self.project.change(&t("cv.undo.pad.draw", &[]), &mut |s: &mut Value| {
            if let Some(l) = list_mut(s, "pads") {
                l.push(pad.clone());
            }
        });
        self.sel = Some(Sel::Pad(id));
        self.set_tool(Tool::Select);
        self.message = tr!("cv.pad.drawn", n = n, z = fixed(z0, 1));
    }

    fn finish_pipe(&mut self, pts: &[DraftPt], units: &str) {
        if pts.len() < 2 {
            self.message = t("cv.pp.need2", &[]);
            return;
        }
        // the surface pipes are checked against (project.pipeSurface(): graded, else EG); pipes are not in its inputs
        let g = self.project.graded();
        let eg = self.project.eg();
        let surface = g.surface.as_deref().or(eg.surface.as_ref());
        let mut new_pipes: Vec<String> = Vec::new();
        let (sdia, pdia) = if units == "m" { (1200.0, 300.0) } else { (48.0, 12.0) };
        self.project.change(&t("cv.undo.pp.draw", &[]), &mut |st: &mut Value| {
            new_pipes.clear();
            if !cc_civil::js::truthy(st.get("network")) {
                let net = cc_civil::pipes::default_network(units).j();
                if let Some(m) = st.as_object_mut() {
                    m.insert("network".into(), net);
                }
            }
            let mut ids: Vec<String> = Vec::with_capacity(pts.len());
            for p in pts {
                let known = p.sid.as_ref().filter(|sid| {
                    st.get("network").is_some_and(|n| arr(n.get("structures")).iter().any(|x| x.get("id").and_then(Value::as_str) == Some(sid)))
                });
                if let Some(sid) = known {
                    ids.push(sid.clone());
                    continue;
                }
                let id = next_id_in(st, "s");
                if let Some(l) = st.get_mut("network").and_then(|n| list_mut(n, "structures")) {
                    let name = tr!("cv.st.name", n = l.len() + 1);
                    l.push(jo! { "id" => id, "name" => name, "x" => p.x, "y" => p.y, "rim" => Value::Null, "sumpDepth" => 0.0, "dia" => sdia });
                }
                ids.push(id);
            }
            for w in ids.windows(2) {
                let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
                if a == b {
                    continue;
                }
                let id = next_id_in(st, "q");
                if let Some(l) = st.get_mut("network").and_then(|n| list_mut(n, "pipes")) {
                    let name = tr!("cv.pp.name", n = l.len() + 1);
                    l.push(jo! { "id" => id, "name" => name, "from" => a, "to" => b, "dia" => pdia, "invUp" => Value::Null, "invDn" => Value::Null });
                }
                new_pipes.push(id);
            }
            if let Some(n) = st.get_mut("network") {
                let net = cc_civil::pipes::read_network(n);
                let (pipes, _) = cc_civil::pipes::auto_inverts(&net, surface, units, Some(&new_pipes));
                if let Some(l) = n.get_mut("pipes").and_then(Value::as_array_mut) {
                    for (o, p) in l.iter_mut().zip(pipes.iter()) {
                        if let Some(m) = o.as_object_mut() {
                            m.insert("invUp".into(), p.inv_up.j());
                            m.insert("invDn".into(), p.inv_dn.j());
                        }
                    }
                }
            }
        });
        self.set_tool(Tool::Select);
        self.sel = None;
        let problems = self.project.network_check().map_or(0, |c| c.problems.len());
        let pipes = tr!("cv.net.pipes", n = new_pipes.len());
        let dia = if units == "m" { "300 mm".to_string() } else { "12\"".to_string() };
        self.message = if problems > 0 {
            tr!("cv.pp.drawn.bad", pipes = pipes, dia = dia, problems = tr!("cv.problems.n", n = problems))
        } else {
            tr!("cv.pp.drawn.ok", pipes = pipes, dia = dia)
        };
    }

    /// features-ui.js `finish(pts, closed)`.
    fn finish_feature(&mut self, pts: &[DraftPt], closed: bool, units: &str) -> bool {
        let min = if closed { 3 } else { 2 };
        if pts.len() < min {
            self.message = t(if closed { "fl.need3" } else { "fl.need2" }, &[]);
            return false;
        }
        let eg = self.project.eg();
        let sf = eg.surface.as_ref();
        let mut guessed = 0usize;
        let mut prev: Option<f64> = None;
        let out: Vec<Value> = pts
            .iter()
            .map(|p| {
                let z = match p.z.filter(|z| z.is_finite()) {
                    Some(z) => Some(z),
                    None => sf.and_then(|s| s.elevation_at(p.x, p.y, 1e-6)),
                };
                let z = match z {
                    Some(z) => z,
                    None => {
                        guessed += 1;
                        prev.unwrap_or(0.0)
                    }
                };
                prev = Some(z);
                jo! { "x" => p.x, "y" => p.y, "z" => z }
            })
            .collect();
        let id = self.project.next_id("f");
        let n = arr(self.project.state.get("features")).len() + 1;
        let (use_, _, cut, fill, _) = cc_civil::featureline::feature_defaults(units);
        let name = tr!("fl.name", n = n);
        let count = out.len();
        let f = jo! { "id" => id, "name" => name, "pts" => Value::Array(out), "closed" => closed, "use" => use_, "daylight" => false, "cut" => cut, "fill" => fill };
        self.project.change(&t("fl.undo.draw", &[]), &mut |s: &mut Value| {
            if let Some(l) = list_mut(s, "features") {
                l.push(f.clone());
            }
        });
        self.sel = Some(Sel::Feature(id));
        let mut m = tr!("fl.drawn", name = name.as_str(), n = count);
        if guessed > 0 {
            m.push(' ');
            m.push_str(&tr!("fl.drawn.guessed", n = guessed));
        }
        self.message = m;
        true
    }

    /// parcels-ui.js `finish(pts)`.
    fn finish_parcel(&mut self, pts: &[DraftPt], units: &str) -> bool {
        if pts.len() < 3 {
            self.message = t("pc.few", &[]);
            return false;
        }
        let name = format!("{} {}", t("pc.default", &[]), arr(self.project.state.get("parcels")).len() + 1);
        let id = self.project.next_id("r");
        let p = jo! { "id" => id, "name" => name, "pts" => Value::Array(pts.iter().map(|q| jo! { "x" => q.x, "y" => q.y }).collect()) };
        self.project.change(&t("pc.undo.draw", &[]), &mut |s: &mut Value| {
            if let Some(l) = list_mut(s, "parcels") {
                l.push(p.clone());
            }
        });
        self.sel = Some(Sel::Parcel(id));
        let corners: Vec<cc_civil::parcels::Corner> = pts.iter().map(|q| cc_civil::parcels::Corner { x: q.x, y: q.y, b: None }).collect();
        let area = cc_civil::parcels::area_words(cc_civil::parcels::parcel_area(&corners), units);
        self.message = tr!("pc.drawn", name = name, area = area);
        true
    }

    // ------------------------------------------------------------------------------------------- delete

    /// civilcraft.js `deleteSelected()` (the profile's PVI aside): the selected item or survey point, one undo step.
    pub fn delete_selected(&mut self) {
        if self.delete_pvi() {
            return;
        }
        let Some(sel) = self.sel.clone() else {
            self.message = t("cv.delete.none", &[]);
            return;
        };
        let name_of = |list: &[Value], id: &str| find_in(list, id).map(|o| string_of(o.get("name")));
        let st = &self.project.state;
        match sel {
            Sel::Alignment(id) => {
                let Some(name) = name_of(arr(st.get("alignments")), &id) else { return self.nothing() };
                self.remove_from("alignments", &id, "cv.undo.al.delete");
                self.message = tr!("fl.deleted", name = name);
            }
            Sel::Pad(id) => {
                let Some(name) = name_of(arr(st.get("pads")), &id) else { return self.nothing() };
                self.remove_from("pads", &id, "cv.undo.pad.delete");
                self.message = tr!("fl.deleted", name = name);
            }
            Sel::Structure(id) => {
                let Some(name) = st.get("network").and_then(|n| name_of(arr(n.get("structures")), &id)) else { return self.nothing() };
                self.project.change(&t("cv.undo.st.delete", &[]), &mut |s: &mut Value| {
                    if let Some(n) = s.get_mut("network") {
                        if let Some(l) = n.get_mut("structures").and_then(Value::as_array_mut) {
                            l.retain(|x| x.get("id").and_then(Value::as_str) != Some(id.as_str()));
                        }
                        if let Some(l) = n.get_mut("pipes").and_then(Value::as_array_mut) {
                            l.retain(|p| {
                                p.get("from").and_then(Value::as_str) != Some(id.as_str())
                                    && p.get("to").and_then(Value::as_str) != Some(id.as_str())
                            });
                        }
                    }
                });
                self.message = tr!("cv.st.deleted", name = name);
            }
            Sel::Feature(id) => {
                let Some(name) = name_of(arr(st.get("features")), &id) else { return self.nothing() };
                self.remove_from("features", &id, "fl.undo.delete");
                self.message = tr!("fl.deleted", name = name);
            }
            Sel::Parcel(id) => {
                let Some(name) = name_of(arr(st.get("parcels")), &id) else { return self.nothing() };
                self.remove_from("parcels", &id, "pc.undo.delete");
                self.message = tr!("pc.deleted", name = name);
            }
            Sel::Pipe(id) => {
                let Some(name) = st.get("network").and_then(|n| name_of(arr(n.get("pipes")), &id)) else { return self.nothing() };
                self.project.change(&t("cv.undo.pp.delete", &[]), &mut |s: &mut Value| {
                    if let Some(l) = s.get_mut("network").and_then(|n| n.get_mut("pipes")).and_then(Value::as_array_mut) {
                        l.retain(|p| p.get("id").and_then(Value::as_str) != Some(id.as_str()));
                    }
                });
                self.message = tr!("fl.deleted", name = name);
            }
            Sel::Point(n) => {
                if !self.project.points.iter().any(|p| p.n == n) {
                    return self.nothing();
                }
                let label = tr!("cv.undo.pts.delete", n = 1usize, count = "1");
                self.project.edit_points(&label, &mut |pts, _| Some(pts.into_iter().filter(|p| p.n != n).collect()));
                let surface = t(
                    if self.project.state.get("surface").and_then(|s| s.get("tin")).is_some_and(|t| !t.is_null()) {
                        "cv.surface.tin.kept"
                    } else {
                        "cv.surface.rebuilt"
                    },
                    &[],
                );
                self.message = tr!("cv.pts.deleted", n = 1usize, count = "1", surface = surface);
            }
        }
        self.sel = None;
    }

    fn nothing(&mut self) {
        self.sel = None;
        self.message = t("cv.delete.none", &[]);
    }

    fn remove_from(&mut self, list: &str, id: &str, label: &str) {
        self.project.change(&t(label, &[]), &mut |s: &mut Value| {
            if let Some(l) = s.get_mut(list).and_then(Value::as_array_mut) {
                l.retain(|x| x.get("id").and_then(Value::as_str) != Some(id));
            }
        });
    }

    /// The Remove button of a PI row (alignments with three or more PIs): one undo step.
    pub fn remove_pi(&mut self, id: &str, i: usize) -> Result<(), String> {
        let n = find_in(arr(self.project.state.get("alignments")), id).map(|a| arr(a.get("pis")).len()).ok_or("no such alignment")?;
        if n <= 2 || i >= n {
            return Err(format!("civil.pi.remove: PI {} of {n} cannot go (an alignment keeps two)", i + 1));
        }
        self.project.change(&t("cv.undo.pi.remove", &[]), &mut |s: &mut Value| {
            if let Some(a) = s
                .get_mut("alignments")
                .and_then(Value::as_array_mut)
                .and_then(|l| l.iter_mut().find(|a| a.get("id").and_then(Value::as_str) == Some(id)))
                && let Some(p) = a.get_mut("pis").and_then(Value::as_array_mut)
                && i < p.len()
            {
                p.remove(i);
            }
        });
        Ok(())
    }

    // ------------------------------------------------------------------------------------------- grips

    /// The vertices a grip can be on, for the selection: (grip, world x, world y).
    pub fn grips(&self) -> Vec<(Grip, f64, f64)> {
        let st = &self.project.state;
        let xy = |v: &Value| (numf(v.get("x")), numf(v.get("y")));
        let mut out = Vec::new();
        match &self.sel {
            Some(Sel::Alignment(id)) => {
                if let Some(a) = find_in(arr(st.get("alignments")), id) {
                    for (i, p) in arr(a.get("pis")).iter().enumerate() {
                        let (x, y) = xy(p);
                        out.push((Grip::Pi(id.clone(), i), x, y));
                    }
                }
            }
            Some(Sel::Feature(id)) => {
                if let Some(f) = find_in(arr(st.get("features")), id) {
                    for (i, p) in arr(f.get("pts")).iter().enumerate() {
                        let (x, y) = xy(p);
                        out.push((Grip::Feature(id.clone(), i), x, y));
                    }
                }
            }
            Some(Sel::Pad(id)) => {
                if let Some(d) = find_in(arr(st.get("pads")), id) {
                    for (i, p) in arr(d.get("pts")).iter().enumerate() {
                        let (x, y) = xy(p);
                        out.push((Grip::Corner(id.clone(), i), x, y));
                    }
                }
            }
            Some(Sel::Structure(id)) => {
                if let Some(x) = st.get("network").and_then(|n| find_in(arr(n.get("structures")), id)) {
                    let (a, b) = xy(x);
                    out.push((Grip::Structure(id.clone()), a, b));
                }
            }
            Some(Sel::Point(n)) => {
                if let Some(p) = self.project.points.iter().find(|p| &p.n == n) {
                    out.push((Grip::Point(n.clone()), p.x, p.y));
                }
            }
            _ => {}
        }
        out
    }

    /// civilcraft.js `gripAt(s)` (then the one selected survey point): the first grip whose square holds the CSS point.
    pub fn grip_at(&self, s: (f64, f64)) -> Option<(Grip, f64, f64)> {
        self.grips().into_iter().find(|(_, x, y)| {
            let q = self.css(*x, *y);
            (q.0 - s.0).abs() <= GRIP_R && (q.1 - s.1).abs() <= GRIP_R
        })
    }

    /// Press on the plan in Select at world (x, y): starts a grip drag when a grip is there.
    pub fn grip_press(&mut self, x: f64, y: f64) -> bool {
        if self.tools.tool != Tool::Select {
            return false;
        }
        let s = self.css(x, y);
        match self.grip_at(s) {
            Some((grip, gx, gy)) => {
                self.tools.drag = Some(GripDrag { grip, from: (gx, gy), at: (gx, gy), press: s, moved: false, cancelled: false });
                true
            }
            None => false,
        }
    }

    /// The pointer moved to world (x, y) with a grip held (4 px of slack before it moves).
    pub fn grip_move(&mut self, x: f64, y: f64) {
        let s = self.css(x, y);
        let Some(d) = self.tools.drag.as_mut() else { return };
        if !d.moved && hypot(s.0 - d.press.0, s.1 - d.press.1) > 4.0 {
            d.moved = true;
        }
        if d.moved {
            d.at = (x, y);
            if let Grip::Point(n) = &d.grip {
                self.message = tr!("pt.drag", n = n.as_str(), e = fixed(x, 3), nn = fixed(y, 3));
            }
        }
    }

    /// Release: a moved grip becomes one undo step (civilcraft.js endPointer `grip` / `point`).
    pub fn grip_release(&mut self) {
        let Some(d) = self.tools.drag.take() else { return };
        if !d.moved || d.cancelled {
            return;
        }
        let (x, y) = d.at;
        let set = |list: &mut Vec<Value>, i: usize| {
            if let Some(o) = list.get_mut(i) {
                set_xy(o, x, y);
            }
        };
        match d.grip {
            Grip::Pi(id, i) => {
                self.project.change(&t("cv.undo.pi.move", &[]), &mut |s: &mut Value| {
                    if let Some(l) = item_list(s, "alignments", &id, "pis") {
                        set(l, i);
                    }
                });
            }
            Grip::Feature(id, i) => {
                self.project.change(&t("fl.undo.move", &[]), &mut |s: &mut Value| {
                    if let Some(l) = item_list(s, "features", &id, "pts") {
                        set(l, i);
                    }
                });
            }
            Grip::Corner(id, i) => {
                self.project.change(&t("cv.undo.pad.corner", &[]), &mut |s: &mut Value| {
                    if let Some(l) = item_list(s, "pads", &id, "pts") {
                        set(l, i);
                    }
                });
            }
            Grip::Structure(id) => {
                self.project.change(&t("cv.undo.st.move", &[]), &mut |s: &mut Value| {
                    if let Some(l) = s.get_mut("network").and_then(|n| n.get_mut("structures")).and_then(Value::as_array_mut)
                        && let Some(o) = l.iter_mut().find(|o| o.get("id").and_then(Value::as_str) == Some(id.as_str()))
                    {
                        set_xy(o, x, y);
                    }
                });
            }
            Grip::Point(n) => {
                let mut patch = Map::new();
                patch.insert("x".into(), jnum(x));
                patch.insert("y".into(), jnum(y));
                let r = self.project.edit_point(&n, &patch, Some(&tr!("pt.move.undo", n = n.as_str())));
                let m = r.get("message").and_then(Value::as_str).unwrap_or("");
                if !m.is_empty() {
                    self.message = m.to_string();
                }
            }
        }
    }

    /// A whole grip drag in world coordinates (the `civil.grip` command): press, move, release.
    pub fn grip_drag(&mut self, from: (f64, f64), to: (f64, f64)) -> Result<(), String> {
        if !self.grip_press(from.0, from.1) {
            return Err(format!("civil.grip: no grip of the selection at {}, {}", from.0, from.1));
        }
        self.grip_move(to.0, to.1);
        self.grip_release();
        Ok(())
    }
}

/// `s[list].find(id)[key]` as a mutable array.
fn item_list<'a>(s: &'a mut Value, list: &str, id: &str, key: &str) -> Option<&'a mut Vec<Value>> {
    s.get_mut(list)?.as_array_mut()?.iter_mut().find(|o| o.get("id").and_then(Value::as_str) == Some(id))?.get_mut(key)?.as_array_mut()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> CivilView {
        let mut cv = CivilView::sample();
        cv.view.width = 1000.0;
        cv.view.height = 800.0;
        cv.view.cx = 6_150_200.0;
        cv.view.cy = 2_080_150.0;
        cv.view.zoom = 1.5;
        cv
    }

    #[test]
    fn an_alignment_is_one_undo_step_with_fitted_radii() {
        let mut cv = site();
        cv.tool_command(Tool::Alignment);
        for (x, y) in [(6_150_000.0, 2_080_000.0), (6_150_200.0, 2_080_100.0), (6_150_400.0, 2_080_050.0)] {
            cv.click(x, y);
        }
        cv.click(6_150_400.5, 2_080_050.0); // within 3 px of the last: ignored
        assert_eq!(cv.tools.draft.len(), 3);
        cv.finish();
        assert_eq!(cv.tools.tool, Tool::Select);
        let a = &cv.project.state["alignments"][0];
        assert_eq!(a["name"], "Alignment 1");
        assert_eq!(a["pis"][1]["r"], serde_json::json!(100.0));
        assert_eq!(cv.project.undo_stack.len(), 1);
        assert!(cv.message.starts_with("Alignment 1: "), "{}", cv.message);
        cv.undo();
        assert!(arr(cv.project.state.get("alignments")).is_empty());
    }

    #[test]
    fn breaklines_take_survey_points_and_stay_in_the_tool() {
        let mut cv = site();
        cv.tool_command(Tool::Breakline);
        let p: Vec<(f64, f64, String)> = cv.project.points.iter().take(3).map(|p| (p.x, p.y, p.n.clone())).collect();
        for (x, y, _) in &p {
            cv.click(x + 0.3, y - 0.2);
        }
        assert_eq!(
            cv.tools.draft.iter().map(|d| d.n.clone().unwrap_or_default()).collect::<Vec<_>>(),
            p.iter().map(|q| q.2.clone()).collect::<Vec<_>>()
        );
        cv.finish();
        assert_eq!(cv.tools.tool, Tool::Breakline);
        assert_eq!(arr(cv.project.state["surface"].get("breaklines")).len(), 1);
        cv.click(6_000_000.0, 2_000_000.0);
        assert_eq!(cv.message, t("cv.bl.click.need", &[]));
    }

    #[test]
    fn pipes_join_structures_and_get_inverts() {
        let mut cv = site();
        cv.tool_command(Tool::Pipe);
        cv.click(6_150_100.0, 2_080_100.0);
        cv.click(6_150_200.0, 2_080_120.0);
        cv.click(6_150_300.0, 2_080_100.0);
        cv.finish();
        let n = &cv.project.state["network"];
        assert_eq!(arr(n.get("structures")).len(), 3);
        assert_eq!(arr(n.get("pipes")).len(), 2);
        assert!(n["pipes"][0]["invUp"].is_number(), "{}", n["pipes"][0]);
        // a second run from the last structure joins it
        cv.tool_command(Tool::Pipe);
        cv.click(6_150_300.0, 2_080_100.0);
        cv.click(6_150_350.0, 2_080_050.0);
        cv.finish();
        let n = &cv.project.state["network"];
        assert_eq!(arr(n.get("structures")).len(), 4);
        assert_eq!(n["pipes"][2]["from"], n["structures"][2]["id"]);
    }

    #[test]
    fn a_feature_line_closes_on_its_first_vertex() {
        let mut cv = site();
        cv.tool_command(Tool::Feature);
        for (x, y) in [(6_150_050.0, 2_080_050.0), (6_150_150.0, 2_080_060.0), (6_150_120.0, 2_080_150.0)] {
            cv.click(x, y);
        }
        cv.click(6_150_051.0, 2_080_050.5);
        assert_eq!(cv.tools.tool, Tool::Select);
        let f = &cv.project.state["features"][0];
        assert_eq!(f["closed"], Value::Bool(true));
        assert_eq!(arr(f.get("pts")).len(), 3);
        assert_eq!(cv.sel, Some(Sel::Feature(string_of(f.get("id")))));
    }

    #[test]
    fn too_few_points_keep_the_draft_or_say_so() {
        let mut cv = site();
        cv.tool_command(Tool::Parcel);
        cv.click(6_150_050.0, 2_080_050.0);
        cv.click(6_150_150.0, 2_080_050.0);
        cv.finish();
        assert_eq!(cv.tools.draft.len(), 2);
        assert_eq!(cv.message, t("pc.few", &[]));
        cv.tool_command(Tool::Pad);
        cv.click(6_150_050.0, 2_080_050.0);
        cv.finish();
        assert!(cv.tools.draft.is_empty());
        assert_eq!(cv.message, t("cv.pad.need3", &[]));
        assert!(cv.project.undo_stack.is_empty());
    }

    #[test]
    fn grips_move_a_pad_corner_as_one_step_and_delete_removes_it() {
        let mut cv = site();
        cv.tool_command(Tool::Pad);
        for (x, y) in [(6_150_050.0, 2_080_050.0), (6_150_150.0, 2_080_050.0), (6_150_150.0, 2_080_150.0)] {
            cv.click(x, y);
        }
        cv.finish();
        let id = string_of(cv.project.state["pads"][0].get("id"));
        cv.grip_drag((6_150_150.0, 2_080_150.0), (6_150_170.0, 2_080_160.0)).unwrap();
        assert_eq!(cv.project.state["pads"][0]["pts"][2]["x"], serde_json::json!(6_150_170.0));
        assert_eq!(cv.project.undo_stack.len(), 2);
        assert!(cv.grip_drag((6_000_000.0, 2_000_000.0), (1.0, 1.0)).is_err());
        cv.sel = Some(Sel::Pad(id));
        cv.delete_selected();
        assert!(arr(cv.project.state.get("pads")).is_empty());
        cv.delete_selected();
        assert_eq!(cv.message, t("cv.delete.none", &[]));
    }

    #[test]
    fn survey_coordinates_snap_steadily_a_millimetre_apart() {
        let text = "1,4000000.000,500000.000,10\n2,4000000.000,500000.001,10.5\n3,4000010,500010,11\n4,4000010,500000,12\n";
        let mut cv = CivilView::import_points(text, "near.csv");
        cv.view.width = 800.0;
        cv.view.height = 600.0;
        cv.view.cx = 500_000.0;
        cv.view.cy = 4_000_000.0;
        cv.view.zoom = 20_000.0; // 20 px per mm
        cv.tool_command(Tool::Breakline);
        cv.click(500_000.000_8, 4_000_000.000_1);
        cv.click(499_999.999_7, 3_999_999.999_9);
        let ns: Vec<String> = cv.tools.draft.iter().filter_map(|d| d.n.clone()).collect();
        assert_eq!(ns, vec!["2".to_string(), "1".to_string()]);
        // a point drag at survey magnitude keeps millimetres
        cv.set_tool(Tool::Select);
        cv.sel = Some(Sel::Point("1".into()));
        cv.grip_drag((500_000.0, 4_000_000.0), (499_999.998, 4_000_000.003)).unwrap();
        let p = cv.project.points.iter().find(|p| p.n == "1").unwrap();
        assert!((p.x - 499_999.998).abs() < 1e-9 && (p.y - 4_000_000.003).abs() < 1e-9, "{p:?}");
    }

    #[test]
    fn esc_drops_the_draft_then_the_tool() {
        let mut cv = site();
        cv.tool_command(Tool::Boundary);
        cv.click(6_150_050.0, 2_080_050.0);
        cv.cancel();
        assert!(cv.tools.draft.is_empty());
        assert_eq!(cv.tools.tool, Tool::Boundary);
        cv.cancel();
        assert_eq!(cv.tools.tool, Tool::Select);
    }
}
