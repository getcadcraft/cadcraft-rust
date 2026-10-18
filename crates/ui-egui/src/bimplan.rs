//! BimCraft's Draw (model-ui.js + model-planview.js): the plan of a level beside the 3D view, the model bar (level,
//! tools, wall type and height, door / window type and size, floor / ceiling / roof options, typed lengths, Plan /
//! Split / 3D, Fit, units, dimensions, Print), the Levels panel, the schedules, and the tools themselves: walls drawn
//! click by click (snapping to ends, faces, 15° steps; Shift square; a chain that comes back to its start closes a
//! room), doors and windows put in a wall where the pointer is, floors / ceilings / roofs for the room or building
//! under the pointer, Select with the drawn elements' own Properties, Delete. Every change is one `cc_bim::draw` op
//! through the editor: one undo step, the 3D view and the plan made again from the edited model.
//!
//! Gestures are commands: a click on the plan runs `bim.point` with the plan point, typed lengths `bim.typed`, so a
//! script or the shot tool draws exactly what a hand would.

use std::collections::{HashMap, HashSet};

use cc_bim::draw::m::{self, V2};
use cc_bim::draw::openings::{OpSpec, Symbol, door_types, fit_offset, opening_symbol, opening_type, opening_type_name, window_types};
use cc_bim::draw::ops::{Op, SegSpec, Snapshot, loops_of, snapshot};
use cc_bim::draw::plan::{CUT, Dim, SCut, Scene, cut_scene, make_scene, own_keys, plan_cut, viewer_to_plan};
use cc_bim::draw::roof::{edge_kinds, format_edges, preset_edges, roof_defaults, roof_geometry};
use cc_bim::draw::rooms::{label_point, loop_at};
use cc_bim::draw::slabs::{FLOOR_KINDS, NewRoof, StoreyLoops};
use cc_bim::draw::tools::{SnapOpts, SnapWall, closes_chain, loop_side, snap_point};
use cc_bim::draw::units::{Typed, fmt_angle, fmt_area, fmt_len, fmt_pitch, parse_len, parse_pitch, parse_typed};
use cc_bim::draw::walls::{Seg, WallChange, outline_of, type_name, type_thickness, wall_outlines, wall_type, wall_types};
use egui::{Color32, Pos2, Rect, RichText, Stroke};

use crate::bimtools::bt;
use crate::{Params, UiApp};

/// The Draw commands (id, label, shortcut, params doc).
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("bim.draw", "Draw", "", "{name?: \"on\" | \"off\"} → the plan of a level beside the 3D view (toggles without a name)"),
    ("bim.layout", "Views", "", "{name: \"plan\" | \"split\" | \"3d\"}"),
    ("bim.level", "Show a level", "", "{name: \"<index>\" (0 = the lowest) | \"<level name>\"}"),
    (
        "bim.tool",
        "Drawing tool",
        "W / D / N / V",
        "{name: \"select\" | \"wall\" | \"door\" | \"window\" | \"floor\" | \"ceiling\" | \"roof\"}",
    ),
    (
        "bim.set",
        "Tool option",
        "",
        "{name: \"wall.type=ext-2x6\" | \"wall.top=level|fixed\" | \"wall.height=10'\" | \"door.type=…\" | \"door.width=…\" | \"window.sill=…\" | \"floor.mode=room|building\" | \"floor.kind=wood|concrete\" | \"floor.thickness=…\" | \"ceiling.mode=…\" | \"ceiling.height=…\" | \"roof.pitch=6/12\" | \"roof.overhang=…\" | \"roof.ends=hip|gable-short|gable-long|shed\" | \"roof.fascia=on|off\"}",
    ),
    ("bim.flip", "Flip the hinges", "H", "{} → the door or window being placed hinges at the other end"),
    ("bim.units", "Units", "", "{name: \"ftin\" | \"mm\"}"),
    ("bim.dims", "Dimensions", "", "{} → dimensions on the plan on / off"),
    ("bim.fit", "Fit the plan", "F", "{}"),
    ("bim.point", "Click on the plan", "", "{name: \"x,y[,shift]\"} → plan mm: what a click there does with the current tool"),
    ("bim.move", "Point on the plan", "", "{name: \"x,y[,shift]\"} → the pointer over the plan (previews, snapping)"),
    (
        "bim.typed",
        "Typed length",
        "",
        "{name: \"12'6\\\"\" | \"3000<90\" | \"@1200,0\" | \"1200,3400\"} → as typed into the Length box and Enter",
    ),
    ("bim.end", "End the wall", "Esc / Enter", "{} → ends the chain being drawn (Esc again: back to Select)"),
    (
        "bim.delete",
        "Delete",
        "Delete",
        "{} → deletes the picked walls, doors, windows, floors, ceilings, roofs (rooms come from the walls)",
    ),
    ("bim.print", "Print the plan", "", "{} → the plan of the level as a PDF (largest standard scale that fits)"),
    ("bim.level.add", "Add a level on top", "", "{}"),
    ("bim.level.set", "Change a level", "", "{name: \"<index>:name=…\" | \"<index>:elevation=…\" | \"<index>:height=…\"}"),
    (
        "bim.edit",
        "Change the picked element",
        "",
        "{name: \"type=…\" | \"thickness=…\" | \"length=…\" | \"top=level|fixed\" | \"height=…\" | \"base=…\" | \"outside=flip\" | \"width=…\" | \"sill=…\" | \"fromstart=…\" | \"fromend=…\" | \"hinges=flip\" | \"face=flip\" | \"name=…\" | \"number=…\" | \"kind=…\" | \"offset=…\" | \"pitch=…\" | \"overhang=…\" | \"ends=…\"}",
    ),
    ("bim.schedule", "Schedule", "", "{name: \"doors\" | \"windows\" | \"walls\" | \"rooms\" [\":csv\"]} → shows it (\":csv\" saves it)"),
];

const SNAP_PX: f64 = 12.0;

/// The wall tool's settings.
#[derive(Clone, Debug)]
pub struct WallSet {
    pub type_key: String,
    pub top: String,
    pub height: f64,
}
/// A door's or window's settings.
#[derive(Clone, Debug)]
pub struct OSet {
    pub type_key: String,
    pub width: f64,
    pub height: f64,
    pub sill: f64,
    pub hand: String,
}
/// Floor / ceiling / roof settings.
#[derive(Clone, Debug)]
pub struct SlabSet {
    pub floor_mode: String,
    pub floor_kind: String,
    pub floor_t: f64,
    pub ceil_mode: String,
    pub ceil_h: Option<f64>,
    pub ceil_t: f64,
    pub pitch: f64,
    pub overhang: f64,
    pub ends: String,
    pub fascia: bool,
}

/// The wall chain being drawn.
#[derive(Clone, Debug, Default)]
pub struct Chain {
    pub points: Vec<V2>,
    pub guids: Vec<String>,
}

/// Where a door or window would go.
#[derive(Clone, Debug)]
pub struct OHover {
    pub wall: usize,
    pub offset: f64,
    pub face: String,
    pub ok: bool,
    pub ends: [f64; 2],
    pub s: f64,
}

/// What the Floor / Ceiling / Roof tool would cover.
#[derive(Clone, Debug)]
pub struct SHover {
    pub poly: Vec<V2>,
    pub area: f64,
    pub building: bool,
    pub seed: V2,
}

/// The plan view's transform: centre (plan mm), px per mm.
#[derive(Clone, Copy, Debug)]
pub struct PView {
    pub cx: f64,
    pub cy: f64,
    pub scale: f64,
    pub w: f64,
    pub h: f64,
    pub fitted: bool,
}

impl Default for PView {
    fn default() -> Self {
        PView { cx: 0.0, cy: 0.0, scale: 0.05, w: 0.0, h: 0.0, fitted: false }
    }
}

impl PView {
    pub fn to_screen(&self, p: V2, o: Pos2) -> Pos2 {
        Pos2::new(o.x + ((p[0] - self.cx) * self.scale + self.w / 2.0) as f32, o.y + (self.h / 2.0 - (p[1] - self.cy) * self.scale) as f32)
    }
    pub fn from_screen(&self, s: Pos2, o: Pos2) -> V2 {
        let (px, py) = (f64::from(s.x - o.x), f64::from(s.y - o.y));
        [(px - self.w / 2.0) / self.scale + self.cx, (self.h / 2.0 - py) / self.scale + self.cy]
    }
    pub fn px_to_mm(&self, px: f64) -> f64 {
        px / self.scale
    }
    /// model-planview.js `fit(bb)`
    pub fn fit(&mut self, bb: Option<[f64; 4]>) {
        if self.w <= 0.0 {
            return;
        }
        let b = bb.unwrap_or([-2000.0, -2000.0, 10000.0, 10000.0]);
        let w = m::max(1000.0, b[2] - b[0]);
        let h = m::max(1000.0, b[3] - b[1]);
        self.scale = m::min((self.w - 60.0) / w, (self.h - 60.0) / h);
        if self.scale.is_nan() || self.scale <= 0.0 {
            self.scale = 0.05;
        }
        self.cx = (b[0] + b[2]) / 2.0;
        self.cy = (b[1] + b[3]) / 2.0;
        self.fitted = true;
    }
    pub fn zoom_at(&mut self, s: Pos2, o: Pos2, k: f64) {
        let before = self.from_screen(s, o);
        self.scale = (self.scale * k).clamp(0.0005, 20.0);
        let after = self.from_screen(s, o);
        self.cx += before[0] - after[0];
        self.cy += before[1] - after[1];
    }
}

/// What the overlay draws (previews, snap marker, the HUD).
#[derive(Clone, Debug, Default)]
pub struct Overlay {
    pub walls: Vec<Vec<V2>>,
    pub lines: Vec<[f64; 4]>,
    pub points: Vec<V2>,
    pub dims: Vec<Dim>,
    pub symbols: Vec<Symbol>,
    pub snap: Option<(V2, &'static str)>,
    pub hud: Option<(V2, String)>,
}

/// A plan cut: (the bytes it was cut from, the level) → (cut, below).
type CutCache = ((usize, f64), Vec<SCut>, Vec<SCut>);
/// The tools' own elements among the picked: walls, doors / windows, derived (kind, GlobalId), a label.
type Picked = (Vec<String>, Vec<String>, Vec<(&'static str, String)>, String);

/// The Draw state of the IFC view.
pub struct Draw {
    pub on: bool,
    pub layout: String,
    pub level: Option<f64>,
    pub tool: String,
    pub wall: WallSet,
    pub door: OSet,
    pub window: OSet,
    pub slab: SlabSet,
    pub units: String,
    pub dims: bool,
    pub chain: Option<Chain>,
    /// the pointer's last plan point and its snap (wall tool)
    pub last: Option<(V2, cc_bim::draw::tools::Snap)>,
    pub ohover: Option<OHover>,
    pub shover: Option<SHover>,
    pub data: Option<Snapshot>,
    pub loops: Option<StoreyLoops>,
    pub scene: Scene,
    /// the plan cut of the model shown: (the bytes it was cut from, the level) → (cut, below)
    cut: Option<CutCache>,
    model: Option<(usize, cc_bim::model::Model)>,
    pub view: PView,
    pub typed: String,
    pub hover_key: Option<f64>,
    pub sched: String,
    pub show_sched: bool,
    pub pending_fit: bool,
    pub levels_open: bool,
}

impl Default for Draw {
    fn default() -> Self {
        let d0 = door_types().remove(0);
        let w1 = window_types().remove(1);
        Draw {
            on: false,
            layout: "split".into(),
            level: None,
            tool: "select".into(),
            wall: WallSet { type_key: "ext-2x6".into(), top: "level".into(), height: 2438.4 },
            door: OSet { type_key: d0.key.into(), width: d0.width, height: d0.height, sill: 0.0, hand: "a".into() },
            window: OSet { type_key: w1.key.into(), width: w1.width, height: w1.height, sill: w1.sill, hand: "a".into() },
            slab: SlabSet {
                floor_mode: "room".into(),
                floor_kind: "wood".into(),
                floor_t: FLOOR_KINDS[0].1,
                ceil_mode: "room".into(),
                ceil_h: None,
                ceil_t: 12.7,
                pitch: m::atan(6.0 / 12.0) * 180.0 / std::f64::consts::PI,
                overhang: 304.8,
                ends: "hip".into(),
                fascia: true,
            },
            units: "ftin".into(),
            dims: true,
            chain: None,
            last: None,
            ohover: None,
            shover: None,
            data: None,
            loops: None,
            scene: Scene::default(),
            cut: None,
            model: None,
            view: PView::default(),
            typed: String::new(),
            hover_key: None,
            sched: "doors".into(),
            show_sched: false,
            pending_fit: true,
            levels_open: true,
        }
    }
}

impl std::fmt::Debug for Draw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Draw").field("on", &self.on).field("tool", &self.tool).field("level", &self.level).finish()
    }
}

fn d(app: &mut UiApp) -> &mut Draw {
    &mut app.bimh.draw
}

/// Errors from the engine as the page shows them.
fn status_err(app: &mut UiApp, e: &str) {
    let text = if e.starts_with("model.") || e.starts_with("res.err.") { bt(e, &[]) } else { bt("model.err.failed", &[("why", e)]) };
    app.error = Some(text);
}

/// Draw on / off (model-ui.js `show`).
pub fn show(app: &mut UiApp, on: bool) {
    if on && app.ifc3_bytes().is_none() {
        app.message = bt("model.needs.ifc", &[]);
        return;
    }
    if on && crate::bimedit::doc(app).is_err() {
        app.message = bt("model.needs.ifc", &[]);
        return;
    }
    let dr = d(app);
    dr.on = on;
    if !on {
        dr.chain = None;
        dr.tool = "select".into();
        return;
    }
    dr.pending_fit = true;
    refresh(app, true);
}

/// After an edit, an undo or a rebuild: the plan's data and scene again.
pub fn after_change(app: &mut UiApp) {
    if app.bimh.draw.on {
        refresh(app, false);
    }
}

/// A file opened (or closed): Draw goes off and starts again on its levels.
pub fn opened(app: &mut UiApp) {
    let units = app.bimh.draw.units.clone();
    app.bimh.draw = Draw { units, ..Draw::default() };
}

/// model-ui.js `refresh()`: snapshot, levels, loops, cut, scene.
pub fn refresh(app: &mut UiApp, fit: bool) {
    let Some(doc) = app.bimh.ed.as_mut() else { return };
    let sn = match snapshot(&mut doc.ed.api) {
        Ok(s) => s,
        Err(e) => {
            app.error = Some(e.0);
            return;
        }
    };
    if sn.levels.is_empty() {
        app.error = Some(bt("model.err.nolevels", &[]));
        app.bimh.draw.data = Some(sn);
        return;
    }
    let lv = app.bimh.draw.level.filter(|l| sn.levels.iter().any(|x| x.id == *l));
    let Some(first) = sn.levels.first() else { return };
    let level = lv.unwrap_or_else(|| sn.levels.iter().find(|l| l.elevation.abs() < 1.0).unwrap_or(first).id);
    app.bimh.draw.level = Some(level);
    if app.bimh.draw.slab.ceil_h.is_none() {
        let i = sn.levels.iter().position(|l| l.id == level).unwrap_or(0);
        if let Some(here) = sn.levels.get(i) {
            let h = sn.levels.get(i.saturating_add(1)).map_or(here.height, |a| a.elevation - here.elevation);
            app.bimh.draw.slab.ceil_h = Some(m::max(2000.0, h - 304.8));
        }
    }
    let loops = loops_of(&mut doc.ed.api, level).ok();
    app.bimh.draw.data = Some(sn);
    app.bimh.draw.loops = loops;
    cut_plan(app);
    build_scene(app);
    if fit {
        app.bimh.draw.pending_fit = true;
    }
}

/// The plan cut of the model shown (what the tools did not draw), cut once per model and level.
fn cut_plan(app: &mut UiApp) {
    let Some(bytes) = app.ifc3_bytes() else { return };
    let key = (std::sync::Arc::as_ptr(&bytes) as usize, app.bimh.draw.level.unwrap_or(f64::NAN));
    if app.bimh.draw.cut.as_ref().is_some_and(|c| c.0.0 == key.0 && c.0.1 == key.1) {
        return;
    }
    let level = key.1;
    if app.bimh.draw.model.as_ref().is_none_or(|m| m.0 != key.0) {
        let Ok(f) = cc_io_ifc::model::IfcFile::open(&bytes) else { return };
        f.parse_all();
        match cc_bim::model::load(&f, &cc_io_ifc::geo::bool_ops::ManifoldOps, 0.0) {
            Ok(model) => app.bimh.draw.model = Some((key.0, model)),
            Err(e) => {
                app.error = Some(e.0);
                return;
            }
        }
    }
    let Some(doc) = app.bimh.ed.as_mut() else { return };
    let sw = match cc_bim::draw::writer::W::new(&mut doc.ed.api).and_then(|mut w| w.storey_world(level)) {
        Ok(m) => m,
        Err(_) => return,
    };
    let (Some((_, model)), Some(sn)) = (app.bimh.draw.model.as_ref(), app.bimh.draw.data.as_ref()) else { return };
    let Some(mm) = viewer_to_plan(&model.coord, &sw) else { return };
    let own = own_keys(sn, 0.0);
    let c = plan_cut(model, &mm, level, CUT, &own, 50.0, true);
    let (cut, below) = cut_scene(&c);
    app.bimh.draw.cut = Some((key, cut, below));
}

/// model-ui.js `makeScene()` for the level shown.
fn build_scene(app: &mut UiApp) {
    let (Some(level), Some(sn), Some(loops)) = (app.bimh.draw.level, app.bimh.draw.data.clone(), app.bimh.draw.loops.clone()) else {
        return;
    };
    let (cut, below) = app.bimh.draw.cut.as_ref().map(|c| (c.1.clone(), c.2.clone())).unwrap_or_default();
    let units = app.bimh.draw.units.clone();
    let dims = app.bimh.draw.dims;
    let Some(doc) = app.bimh.ed.as_mut() else { return };
    let api = &mut doc.ed.api;
    let mut other = |st: f64| loops_of(api, st).ok();
    app.bimh.draw.scene = make_scene(&sn, level, &loops.loops, &cut, &below, &units, dims, 0.0, &mut other);
}

// ------------------------------------------------------------------------------------------------------------ helpers

fn walls_here(dr: &Draw) -> Vec<cc_bim::draw::walls::Wall> {
    match (&dr.data, dr.level) {
        (Some(sn), Some(l)) => sn.walls.iter().filter(|w| w.storey == Some(l)).cloned().collect(),
        _ => Vec::new(),
    }
}
fn level_info(dr: &Draw) -> Option<(cc_bim::draw::levels::Level, Option<cc_bim::draw::levels::Level>)> {
    let sn = dr.data.as_ref()?;
    let i = sn.levels.iter().position(|l| Some(l.id) == dr.level)?;
    Some((sn.levels.get(i)?.clone(), sn.levels.get(i + 1).cloned()))
}
/// Free directions come out in 10 mm or 1/2".
fn round_step(dr: &Draw) -> f64 {
    if dr.units == "mm" { 10.0 } else { 12.7 }
}
fn snap_walls(dr: &Draw) -> Vec<SnapWall> {
    walls_here(dr).iter().map(|w| SnapWall { a: w.a, b: w.b, t: w.t }).collect()
}
fn snap_at(dr: &Draw, p: V2, shift: bool) -> cc_bim::draw::tools::Snap {
    let from = dr.chain.as_ref().and_then(|c| c.points.last().copied());
    let pts: Vec<V2> = dr.chain.as_ref().map(|c| c.points.clone()).unwrap_or_default();
    snap_point(
        p,
        &snap_walls(dr),
        &pts,
        SnapOpts {
            tol: dr.view.px_to_mm(SNAP_PX),
            from,
            ortho: shift,
            step: 15.0,
            round: if from.is_some() { round_step(dr) } else { 0.0 },
        },
    )
}
fn oset<'a>(dr: &'a Draw, kind: &str) -> &'a OSet {
    if kind == "door" { &dr.door } else { &dr.window }
}

/// `faceEnds(w, face)`: where a face of a wall starts and ends along it (its joined outline).
fn face_ends(dr: &Draw, w: &cc_bim::draw::walls::Wall, face: &str) -> [f64; 2] {
    let walls = walls_here(dr);
    let segs: Vec<Seg> = walls.iter().map(|x| x.seg()).collect();
    let outlines = wall_outlines(&segs, 1.0);
    let Some(poly) = outline_of(&outlines, &w.guid) else { return [0.0, w.len] };
    let u = [(w.b[0] - w.a[0]) / w.len, (w.b[1] - w.a[1]) / w.len];
    let sg = if face == "right" { -1.0 } else { 1.0 };
    let xs: Vec<f64> = poly
        .iter()
        .map(|&[x, y]| [(x - w.a[0]) * u[0] + (y - w.a[1]) * u[1], -(x - w.a[0]) * u[1] + (y - w.a[1]) * u[0]])
        .filter(|[_, y]| (y - sg * w.t / 2.0).abs() < 0.5)
        .map(|[x, _]| x)
        .collect();
    if xs.len() >= 2 { [m::min_of(xs.iter().copied()), m::max_of(xs.iter().copied())] } else { [0.0, w.len] }
}

/// `hoverOpening(p, kind, width)`
fn hover_opening(dr: &Draw, p: V2, kind: &str, width: f64) -> Option<OHover> {
    let tol = dr.view.px_to_mm(16.0);
    let walls = walls_here(dr);
    let mut best: Option<(usize, f64, f64, f64)> = None;
    for (i, w) in walls.iter().enumerate() {
        let l = w.len;
        let u = [(w.b[0] - w.a[0]) / l, (w.b[1] - w.a[1]) / l];
        let n = [-u[1], u[0]];
        let (rx, ry) = (p[0] - w.a[0], p[1] - w.a[1]);
        let s = rx * u[0] + ry * u[1];
        let dd = rx * n[0] + ry * n[1];
        if s < -tol || s > l + tol {
            continue;
        }
        let score = m::max(0.0, dd.abs() - w.t / 2.0);
        if score <= tol && best.is_none_or(|b| score < b.3) {
            best = Some((i, s, dd, score));
        }
    }
    let (wi, s, dd, _) = best?;
    let w = walls.get(wi)?;
    let step = round_step(dr);
    let offset = if (s - w.len / 2.0).abs() <= tol { (w.len - width) / 2.0 } else { m::round((s - width / 2.0) / step) * step };
    let fit = fit_offset(w.len, width, offset, 0.0);
    let face = if kind == "door" { if dd >= 0.0 { "left".to_string() } else { "right".to_string() } } else { w.ext_side.clone() };
    let ends = face_ends(dr, w, &face);
    Some(OHover { wall: wi, offset: fit.unwrap_or(offset), face, ok: fit.is_some(), ends, s })
}

/// `hoverSlab(p, tool)`
fn hover_slab(dr: &Draw, p: V2, tool: &str) -> Option<SHover> {
    let loops = dr.loops.as_ref()?;
    let building =
        tool == "roof" || (tool == "floor" && dr.slab.floor_mode == "building") || (tool == "ceiling" && dr.slab.ceil_mode == "building");
    if building {
        loop_at(&loops.loops.outlines, p).map(|o| SHover { poly: o.poly.clone(), area: o.area, building, seed: p })
    } else {
        loop_at(&loops.loops.rooms, p).map(|r| SHover { poly: r.poly.clone(), area: r.area, building, seed: p })
    }
}

/// The roof's per-wall settings for the chosen ends (`roofEdges`), None when it can't.
fn roof_edges(dr: &Draw, poly: &[V2], at: V2) -> Option<String> {
    preset_edges(poly, &walls_here(dr), &dr.slab.ends, Some(at)).map(|mm| format_edges(&mm))
}

// ------------------------------------------------------------------------------------------------------------ commands

fn parse_pt(s: &str) -> Option<(V2, bool)> {
    let mut it = s.split(',');
    let x = cc_bim::js::str_num(it.next()?.trim());
    let y = cc_bim::js::str_num(it.next()?.trim());
    let shift = it.next().is_some_and(|t| t.trim() == "shift");
    (x.is_finite() && y.is_finite()).then_some(([x, y], shift))
}

/// Run a Draw command.
pub fn run(app: &mut UiApp, id: &str, p: &Params) -> Result<(), String> {
    let name = p.name.clone().unwrap_or_default();
    match id {
        "bim.draw" => {
            let on = match name.as_str() {
                "on" => true,
                "off" => false,
                _ => !app.bimh.draw.on,
            };
            show(app, on);
        }
        "bim.layout" => {
            if name == "3d" {
                app.bimh.draw.layout = "split".into();
                show(app, false);
            } else if name == "plan" || name == "split" {
                app.bimh.draw.layout = name;
                app.bimh.draw.pending_fit = true;
            } else {
                return Err("bim.layout: plan, split or 3d".into());
            }
        }
        "bim.level" => {
            let sn = app.bimh.draw.data.as_ref().ok_or("Draw is off")?;
            let i = name.parse::<usize>().ok().filter(|i| *i < sn.levels.len());
            let lv = match i {
                Some(i) => sn.levels.get(i),
                None => sn.levels.iter().find(|l| l.name == name),
            };
            let id = lv.ok_or("bim.level: no such level")?.id;
            app.bimh.draw.level = Some(id);
            app.bimh.draw.chain = None;
            app.bimh.draw.cut = None;
            refresh(app, true);
        }
        "bim.tool" => {
            if !["select", "wall", "door", "window", "floor", "ceiling", "roof"].contains(&name.as_str()) {
                return Err("bim.tool: select, wall, door, window, floor, ceiling or roof".into());
            }
            let dr = d(app);
            dr.tool = name;
            dr.chain = None;
            dr.ohover = None;
            dr.shover = None;
            dr.last = None;
        }
        "bim.set" => set_option(app, &name)?,
        "bim.flip" => {
            let dr = d(app);
            let k = dr.tool.clone();
            let o = if k == "door" {
                &mut dr.door
            } else if k == "window" {
                &mut dr.window
            } else {
                return Ok(());
            };
            o.hand = if o.hand == "b" { "a".into() } else { "b".into() };
        }
        "bim.units" => {
            app.bimh.draw.units = if name == "mm" { "mm".into() } else { "ftin".into() };
            refresh(app, false);
        }
        "bim.dims" => {
            app.bimh.draw.dims = !app.bimh.draw.dims;
            build_scene(app);
        }
        "bim.fit" => app.bimh.draw.pending_fit = true,
        "bim.move" => {
            let (pt, shift) = parse_pt(&name).ok_or("bim.move: x,y")?;
            on_move(app, pt, shift);
        }
        "bim.point" => {
            let (pt, shift) = parse_pt(&name).ok_or("bim.point: x,y")?;
            on_move(app, pt, shift);
            on_click(app, pt, shift);
        }
        "bim.typed" => {
            if !apply_typed(app, &name) {
                return Ok(());
            }
            app.bimh.draw.typed.clear();
        }
        "bim.end" => {
            let dr = d(app);
            if dr.chain.is_some() {
                dr.chain = None;
                app.message = bt("model.chain.ended", &[]);
            } else if dr.tool != "select" {
                dr.tool = "select".into();
            }
        }
        "bim.delete" => delete_picked(app)?,
        "bim.print" => print(app)?,
        "bim.level.add" => {
            let sn = app.bimh.draw.data.as_ref().ok_or("Draw is off")?;
            let n = sn.levels.len() + 1;
            let top = sn.levels.last();
            let op = Op::NewLevel {
                name: bt("new.level", &[("n", &n.to_string())]),
                elevation: top.map_or(0.0, |t| t.elevation + t.height),
                height: top.map_or(3000.0, |t| t.height),
                guid: None,
            };
            commit(app, op, &bt("new.level", &[("n", &n.to_string())]), &bt("model.verb.added", &[]), None);
        }
        "bim.level.set" => level_set(app, &name)?,
        "bim.edit" => edit_picked(app, &name)?,
        "bim.schedule" => {
            let (which, csv) = name.split_once(':').map_or((name.as_str(), false), |(a, b)| (a, b == "csv"));
            if !["doors", "windows", "walls", "rooms"].contains(&which) {
                return Err("bim.schedule: doors, windows, walls or rooms".into());
            }
            app.bimh.draw.sched = which.into();
            app.bimh.draw.show_sched = true;
            if csv {
                schedule_csv(app)?;
            }
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

fn set_option(app: &mut UiApp, spec: &str) -> Result<(), String> {
    let (k, v) = spec.split_once('=').ok_or("bim.set: key=value")?;
    let dr = d(app);
    let units = dr.units.clone();
    let len = |v: &str, min: f64| -> Result<f64, String> {
        let mm = parse_len(v, &units);
        if mm >= min { Ok(mm) } else { Err(bt("model.err.length", &[])) }
    };
    match k {
        "wall.type" => {
            if wall_type(v).is_none() {
                return Err(bt("model.err.walltype", &[]));
            }
            dr.wall.type_key = v.into();
        }
        "wall.top" => dr.wall.top = if v == "fixed" { "fixed".into() } else { "level".into() },
        "wall.height" => {
            let mm = parse_len(v, &units);
            if mm > 100.0 {
                dr.wall.height = mm;
            } else {
                return Err(bt("model.err.length", &[]));
            }
        }
        "door.type" | "window.type" => {
            let ty = opening_type(v).filter(|t| t.door == (k == "door.type")).ok_or_else(|| bt("model.err.opentype", &[]))?;
            let o = if k == "door.type" { &mut dr.door } else { &mut dr.window };
            o.type_key = ty.key.into();
            o.width = ty.width;
            o.height = ty.height;
            if k == "window.type" {
                o.sill = ty.sill;
            }
        }
        "door.width" | "window.width" => {
            let mm = len(v, 100.0)?;
            if k == "door.width" { dr.door.width = mm } else { dr.window.width = mm }
        }
        "door.height" | "window.height" => {
            let mm = len(v, 100.0)?;
            if k == "door.height" { dr.door.height = mm } else { dr.window.height = mm }
        }
        "window.sill" => dr.window.sill = len(v, 0.0)?,
        "floor.mode" => dr.slab.floor_mode = if v == "building" { "building".into() } else { "room".into() },
        "floor.kind" => {
            let k2 = FLOOR_KINDS.iter().find(|x| x.0 == v).ok_or("bim.set: floor.kind wood or concrete")?;
            dr.slab.floor_kind = k2.0.into();
            dr.slab.floor_t = k2.1;
        }
        "floor.thickness" => dr.slab.floor_t = len(v, 10.0)?,
        "ceiling.mode" => dr.slab.ceil_mode = if v == "building" { "building".into() } else { "room".into() },
        "ceiling.height" => dr.slab.ceil_h = Some(len(v, 500.0)?),
        "ceiling.thickness" => dr.slab.ceil_t = len(v, 1.0)?,
        "roof.pitch" => {
            let p = parse_pitch(v);
            if p > 1.0 && p < 80.0 {
                dr.slab.pitch = p;
            } else {
                return Err(bt("model.err.pitch", &[]));
            }
        }
        "roof.overhang" => dr.slab.overhang = len(v, 0.0)?,
        "roof.ends" => {
            if !["hip", "gable-short", "gable-long", "shed"].contains(&v) {
                return Err("bim.set: roof.ends hip, gable-short, gable-long or shed".into());
            }
            dr.slab.ends = v.into();
        }
        "roof.fascia" => dr.slab.fascia = v == "on" || v == "true",
        other => return Err(format!("bim.set: unknown option {other}")),
    }
    Ok(())
}

/// model-ui.js `onMove(p, e)`: previews.
fn on_move(app: &mut UiApp, p: V2, shift: bool) {
    let dr = d(app);
    match dr.tool.as_str() {
        "door" | "window" => {
            let k = dr.tool.clone();
            let w = oset(dr, &k).width;
            dr.ohover = hover_opening(dr, p, &k, w);
        }
        "floor" | "ceiling" | "roof" => {
            let k = dr.tool.clone();
            dr.shover = hover_slab(dr, p, &k);
        }
        "wall" => {
            let s = snap_at(dr, p, shift);
            dr.last = Some((p, s));
        }
        _ => {
            let k = hit(&dr.scene, &dr.view, p, 6.0);
            dr.hover_key = k;
        }
    }
}

/// model-ui.js `onClick(p, e)`.
fn on_click(app: &mut UiApp, p: V2, shift: bool) {
    let tool = app.bimh.draw.tool.clone();
    match tool.as_str() {
        "select" => {
            let k = hit(&app.bimh.draw.scene, &app.bimh.draw.view, p, 6.0);
            match k {
                Some(id) => app.select_product(id),
                None => app.select_product_none(),
            }
        }
        "floor" | "ceiling" | "roof" => {
            let h = hover_slab(&app.bimh.draw, p, &tool);
            let Some(h) = h else {
                let building = tool == "roof"
                    || (tool == "floor" && app.bimh.draw.slab.floor_mode == "building")
                    || (tool == "ceiling" && app.bimh.draw.slab.ceil_mode == "building");
                app.error = Some(bt(if building { "model.err.nobuilding2" } else { "model.err.noroom" }, &[]));
                return;
            };
            place_slab(app, &h, &tool);
        }
        "door" | "window" => {
            let w = oset(&app.bimh.draw, &tool).width;
            match hover_opening(&app.bimh.draw, p, &tool, w) {
                Some(h) => place_here(app, &h, &tool),
                None => app.message = bt(&format!("model.hint.{tool}"), &[]),
            }
        }
        "wall" => {
            let s = snap_at(&app.bimh.draw, p, shift);
            wall_point(app, s.p);
        }
        _ => {}
    }
}

/// model-ui.js `wallPoint(q)`.
fn wall_point(app: &mut UiApp, q: V2) {
    let dr = d(app);
    let Some(chain) = dr.chain.clone() else {
        dr.chain = Some(Chain { points: vec![q], guids: Vec::new() });
        return;
    };
    let Some(&a) = chain.points.last() else { return };
    if m::hypot(q[0] - a[0], q[1] - a[1]) < 1.0 {
        return;
    }
    let closing = closes_chain(&chain.points, q, 1.0);
    let b = if closing { chain.points.first().copied().unwrap_or(q) } else { q };
    let Some(ty) = wall_type(&dr.wall.type_key) else { return };
    let Some((lv, above)) = level_info(dr) else { return };
    let (top_level, height) = if dr.wall.top == "level" && above.is_some() {
        (above.and_then(|x| x.guid).unwrap_or_default(), 0.0)
    } else {
        (
            String::new(),
            if dr.wall.top == "fixed" {
                dr.wall.height
            } else if lv.height != 0.0 {
                lv.height
            } else {
                2438.4
            },
        )
    };
    let side = if closing { loop_side(&chain.points) } else { "left" };
    let flips = if closing { chain.guids.clone() } else { Vec::new() };
    if let Some(c) = dr.chain.as_mut() {
        c.points.push(b);
    }
    let run = Op::DrawWalls {
        storey: lv.guid.clone().unwrap_or_default(),
        segs: vec![SegSpec { a, b, type_key: None, ext_side: None, guid: None }],
        type_key: dr.wall.type_key.clone(),
        base_offset: 0.0,
        top_level,
        top_offset: 0.0,
        height,
        ext_side: side.into(),
    };
    let op = if closing && !flips.is_empty() {
        Op::Seq {
            ops: vec![
                run,
                Op::ChangeWalls { guids: flips.clone(), changes: WallChange { ext_side: Some(side.into()), ..WallChange::default() } },
            ],
            keep_first: true,
        }
    } else {
        run
    };
    let ok = commit(app, op, &type_name(&ty), &bt("model.verb.drew", &[]), Some(true));
    if closing {
        app.bimh.draw.chain = None;
        if ok {
            app.message = bt("model.room.closed", &[("n", &(flips.len().saturating_add(1)).to_string())]);
        }
    }
}

/// model-ui.js `commit` → `apply`: one change through the editor; the plan and 3D follow. `chain`: a wall of the chain
/// (its point is taken back when the change is refused, its GlobalId kept for closing the room). → it worked.
fn commit(app: &mut UiApp, op: Op, label: &str, verb: &str, chain: Option<bool>) -> bool {
    let Some(doc) = app.bimh.ed.as_mut() else { return false };
    match doc.ed.model_change(op, label, verb) {
        Err(e) => {
            status_err(app, &e.0);
            if chain.is_some()
                && let Some(c) = app.bimh.draw.chain.as_mut()
            {
                c.points.pop();
            }
            refresh(app, false);
            false
        }
        Ok(r) => {
            if chain.is_some()
                && let Some(c) = app.bimh.draw.chain.as_mut()
            {
                for (g, _) in &r.made {
                    c.guids.push(g.clone());
                }
            }
            app.error = None;
            crate::bimedit::after_change(app, r.id, true);
            true
        }
    }
}

/// model-ui.js `placeHere(h, kind)`.
fn place_here(app: &mut UiApp, h: &OHover, kind: &str) {
    if !h.ok {
        app.error = Some(bt("model.err.nofit", &[]));
        return;
    }
    let walls = walls_here(&app.bimh.draw);
    let Some(w) = walls.get(h.wall) else { return };
    let o = oset(&app.bimh.draw, kind).clone();
    let Some(ty) = opening_type(&o.type_key) else { return };
    let spec = cc_bim::draw::openings::NewOpening {
        wall: w.guid.clone(),
        type_key: o.type_key.clone(),
        offset: Some(h.offset),
        width: o.width,
        height: o.height,
        sill: Some(if kind == "window" { o.sill } else { 0.0 }),
        hand: o.hand.clone(),
        face: h.face.clone(),
        guid: None,
        opening_guid: None,
    };
    commit(app, Op::PlaceOpening(spec), &opening_type_name(&ty), &bt("model.verb.placed", &[]), None);
}

/// model-ui.js `placeSlab(h, tool)`.
fn place_slab(app: &mut UiApp, h: &SHover, tool: &str) {
    let Some((lv, _)) = level_info(&app.bimh.draw) else { return };
    let st = lv.guid.clone().unwrap_or_default();
    let s = app.bimh.draw.slab.clone();
    match tool {
        "floor" => {
            let op = Op::PlaceFloor {
                storey: st,
                seed: h.seed,
                mode: s.floor_mode.clone(),
                kind: s.floor_kind.clone(),
                thickness: s.floor_t,
                offset: 0.0,
                guid: None,
            };
            commit(app, op, &bt(&format!("model.floor.{}", s.floor_kind), &[]), &bt("model.verb.placed", &[]), None);
        }
        "ceiling" => {
            let op = Op::PlaceCeiling {
                storey: st,
                seed: h.seed,
                mode: s.ceil_mode.clone(),
                height: s.ceil_h.unwrap_or(2438.4),
                thickness: s.ceil_t,
                guid: None,
            };
            commit(app, op, &bt("model.ceiling", &[]), &bt("model.verb.placed", &[]), None);
        }
        _ => {
            let Some(g) = roof_edges(&app.bimh.draw, &h.poly, h.seed) else {
                app.error = Some(bt("model.err.gable4", &[]));
                return;
            };
            let dd = roof_defaults();
            let spec = NewRoof {
                seed: h.seed,
                pitch: s.pitch,
                overhang: s.overhang,
                edges: g,
                thickness: dd.thickness,
                fascia: if s.fascia { dd.fascia } else { 0.0 },
                soffit: s.fascia,
                ..NewRoof::default()
            };
            commit(app, Op::PlaceRoof { storey: st, spec }, &bt("model.roof", &[]), &bt("model.verb.placed", &[]), None);
        }
    }
}

/// model-ui.js `applyTyped(text)` → whether it was taken.
fn apply_typed(app: &mut UiApp, text: &str) -> bool {
    let tool = app.bimh.draw.tool.clone();
    let units = app.bimh.draw.units.clone();
    if tool == "select" {
        return false;
    }
    if tool == "door" || tool == "window" {
        let dd = parse_len(text, &units);
        let h = app.bimh.draw.ohover.clone();
        let o = oset(&app.bimh.draw, &tool).clone();
        let Some(h) = h.filter(|_| dd.is_finite() && dd >= 0.0) else {
            app.error = Some(bt(if app.bimh.draw.ohover.is_some() { "model.err.length" } else { "model.typed.wall" }, &[]));
            return false;
        };
        let walls = walls_here(&app.bimh.draw);
        let Some(w) = walls.get(h.wall) else { return false };
        let [e0, e1] = h.ends;
        let near_start = h.s < (e0 + e1) / 2.0;
        let offset = if near_start { e0 + dd } else { e1 - dd - o.width };
        let fit = fit_offset(w.len, o.width, offset, 0.0);
        match fit.filter(|f| (f - offset).abs() <= 0.5) {
            Some(f) => {
                place_here(app, &OHover { offset: f, ok: true, ..h }, &tool);
                true
            }
            None => {
                app.error = Some(bt("model.err.nofit", &[]));
                false
            }
        }
    } else if tool == "wall" {
        let Some(v) = parse_typed(text, &units) else {
            app.error = Some(bt("model.err.typed", &[]));
            return false;
        };
        let last = app.bimh.draw.chain.as_ref().and_then(|c| c.points.last().copied());
        let q = match (v, last) {
            (Typed::Abs(x, y), _) => [x, y],
            (_, None) => {
                app.error = Some(bt("model.typed.first", &[]));
                return false;
            }
            (Typed::Rel(dx, dy), Some(l)) => [l[0] + dx, l[1] + dy],
            (Typed::Polar(len, ang), Some(l)) => {
                let r = ang * std::f64::consts::PI / 180.0;
                [l[0] + len * m::cos(r), l[1] + len * m::sin(r)]
            }
            (Typed::Len(len), Some(l)) => {
                let to = app.bimh.draw.last.as_ref().map_or([l[0] + 1.0, l[1]], |x| x.1.p);
                let dd = m::hypot(to[0] - l[0], to[1] - l[1]);
                let dd = if dd != 0.0 && !dd.is_nan() { dd } else { 1.0 };
                [l[0] + (to[0] - l[0]) / dd * len, l[1] + (to[1] - l[1]) / dd * len]
            }
        };
        wall_point(app, q);
        true
    } else {
        false
    }
}

/// The tools' own elements among the picked (`picked()`): (walls, openings, derived (kind, guid)), a label.
fn picked(app: &UiApp) -> Picked {
    let mut out = (Vec::new(), Vec::new(), Vec::new(), String::new());
    let Some(id) = app.picked_product() else { return out };
    let Some(sn) = app.bimh.draw.data.as_ref() else { return out };
    if let Some(w) = sn.walls.iter().find(|x| x.id == id) {
        out.0.push(w.guid.clone());
        out.3 = bt("model.wall", &[]);
    } else if let Some(o) = sn.openings.iter().find(|x| x.id == id) {
        out.1.push(o.guid.clone());
        out.3 = cc_bim::draw::openings::type_key_name(&o.type_key);
    } else if let Some(sp) = sn.derived.spaces.iter().find(|x| x.id == id) {
        out.2.push(("room", sp.guid.clone()));
        out.3 = if sp.name.is_empty() { sp.number.clone() } else { sp.name.clone() };
    } else if let Some(f) = sn.derived.floors.iter().find(|x| x.id == id) {
        out.2.push(("floor", f.guid.clone()));
        out.3 = bt(&format!("model.floor.{}", f.kind), &[]);
    } else if let Some(c) = sn.derived.ceilings.iter().find(|x| x.id == id) {
        out.2.push(("ceiling", c.guid.clone()));
        out.3 = bt("model.ceiling", &[]);
    } else if let Some(r) = sn.derived.roofs.iter().find(|x| x.id == id || x.parts.contains(&id)) {
        out.2.push(("roof", r.guid.clone()));
        out.3 = bt("model.roof", &[]);
    }
    out
}

/// `deletePicked(p)`
fn delete_picked(app: &mut UiApp) -> Result<(), String> {
    let (walls, openings, derived, label) = picked(app);
    if walls.is_empty() && openings.is_empty() && derived.is_empty() {
        return Err(bt("edit.nothing.picked", &[]));
    }
    if derived.iter().any(|x| x.0 == "room") {
        return Err(bt("model.room.nodelete", &[]));
    }
    app.select_product_none();
    let op = Op::RemoveElements { walls, openings, derived: derived.into_iter().map(|x| x.1).collect() };
    commit(app, op, &label, &bt("model.verb.deleted", &[]), None);
    Ok(())
}

fn level_set(app: &mut UiApp, spec: &str) -> Result<(), String> {
    let (i, rest) = spec.split_once(':').ok_or("bim.level.set: <index>:field=value")?;
    let (k, v) = rest.split_once('=').ok_or("bim.level.set: <index>:field=value")?;
    let sn = app.bimh.draw.data.as_ref().ok_or("Draw is off")?;
    let l = i.parse::<usize>().ok().and_then(|i| sn.levels.get(i)).cloned().ok_or("bim.level.set: no such level")?;
    let units = app.bimh.draw.units.clone();
    let g = l.guid.clone().unwrap_or_default();
    let ch = match k {
        "name" => {
            if cc_cad::js::trim(v).is_empty() {
                return Ok(());
            }
            (cc_bim::draw::levels::LevelChange { name: Some(cc_cad::js::trim(v).to_string()), ..Default::default() }, "model.verb.changed")
        }
        "elevation" => {
            let mm = parse_len(v, &units);
            if !mm.is_finite() {
                return Err(bt("model.err.length", &[]));
            }
            (cc_bim::draw::levels::LevelChange { elevation: Some(mm), shift_above: Some(true), ..Default::default() }, "model.verb.moved")
        }
        "height" => {
            let mm = parse_len(v, &units);
            if !(1000.0..=100_000.0).contains(&mm) {
                return Err(bt("new.err.height", &[]));
            }
            (cc_bim::draw::levels::LevelChange { height: Some(mm), shift_above: Some(true), ..Default::default() }, "model.verb.changed")
        }
        _ => return Err("bim.level.set: name, elevation or height".into()),
    };
    commit(app, Op::ChangeLevel { guid: g, changes: ch.0 }, &l.name, &bt(ch.1, &[]), None);
    Ok(())
}

/// The drawn element's Properties (`propsBlock`) changes.
fn edit_picked(app: &mut UiApp, spec: &str) -> Result<(), String> {
    let (k, v) = spec.split_once('=').ok_or("bim.edit: field=value")?;
    let id = app.picked_product().ok_or_else(|| bt("edit.nothing.picked", &[]))?;
    let sn = app.bimh.draw.data.clone().ok_or("Draw is off")?;
    let units = app.bimh.draw.units.clone();
    let len = |min: f64, allow_zero: bool| -> Result<f64, String> {
        let mm = parse_len(v, &units);
        if !mm.is_finite() || (if allow_zero { mm < 0.0 } else { mm < min }) { Err(bt("model.err.length", &[])) } else { Ok(mm) }
    };
    let changed = bt("model.verb.changed", &[]);
    if let Some(w0) = sn.walls.iter().find(|x| x.id == id) {
        let title = bt("model.wall", &[]);
        let above = sn.levels.iter().position(|l| Some(l.id) == w0.storey).and_then(|i| sn.levels.get(i + 1)).cloned();
        let ch = match k {
            "type" => WallChange { type_key: Some(v.to_string()), ..Default::default() },
            "thickness" => {
                let mm = len(10.0, false)?;
                let key = cc_bim::draw::walls::type_at(&w0.type_key, mm).ok_or_else(|| bt("model.err.thin", &[]))?;
                WallChange { type_key: Some(key), ..Default::default() }
            }
            "length" => {
                let mm = len(10.0, false)?;
                let u = [(w0.b[0] - w0.a[0]) / w0.len, (w0.b[1] - w0.a[1]) / w0.len];
                WallChange { b: Some([w0.a[0] + u[0] * mm, w0.a[1] + u[1] * mm]), ..Default::default() }
            }
            "top" if v == "level" => {
                let a = above.ok_or_else(|| bt("model.err.notop", &[]))?;
                WallChange { top_level: Some(a.guid.unwrap_or_default()), top_offset: Some(0.0), ..Default::default() }
            }
            "top" => WallChange { height: Some(w0.height), ..Default::default() },
            "height" => {
                let mm = len(100.0, false)?;
                if !w0.top_level.is_empty() && above.is_some() {
                    WallChange { top_offset: Some(w0.top_offset + (mm - w0.height)), ..Default::default() }
                } else {
                    WallChange { height: Some(mm), ..Default::default() }
                }
            }
            "base" => WallChange { base_offset: Some(len(0.0, true)?), ..Default::default() },
            "outside" => {
                WallChange { ext_side: Some(if w0.ext_side == "right" { "left".into() } else { "right".into() }), ..Default::default() }
            }
            _ => return Err(format!("bim.edit: a wall has no {k}")),
        };
        commit(app, Op::ChangeWall { guid: w0.guid.clone(), changes: ch, carry_joined: true }, &title, &changed, None);
        return Ok(());
    }
    if let Some(o0) = sn.openings.iter().find(|x| x.id == id) {
        let title = cc_bim::draw::openings::type_key_name(&o0.type_key);
        let mut ch = cc_bim::draw::openings::OpeningChange::default();
        match k {
            "type" => ch.type_key = Some(v.to_string()),
            "width" => ch.width = Some(len(100.0, false)?),
            "height" => ch.height = Some(len(100.0, false)?),
            "sill" => ch.sill = Some(len(0.0, true)?),
            "fromstart" | "fromend" => {
                let mm = len(0.0, true)?;
                let dr = &app.bimh.draw;
                let wall = sn.walls.iter().find(|w| w.id == o0.wall);
                let [e0, e1] = wall.map_or([0.0, o0.offset + o0.width], |w| face_ends(dr, w, &o0.face));
                ch.offset = Some(if k == "fromstart" { e0 + mm } else { e1 - mm - o0.width });
            }
            "hinges" => ch.hand = Some(if o0.hand == "b" { "a".into() } else { "b".into() }),
            "face" => ch.face = Some(if o0.face == "right" { "left".into() } else { "right".into() }),
            _ => return Err(format!("bim.edit: a door or window has no {k}")),
        }
        commit(app, Op::ChangeOpening { guid: o0.guid.clone(), changes: ch }, &title, &changed, None);
        return Ok(());
    }
    let (_, _, derived, title) = picked(app);
    let Some((kind, guid)) = derived.into_iter().next() else { return Err(bt("edit.nothing.picked", &[])) };
    let mut ch = cc_bim::draw::slabs::DerivedChange::default();
    match (kind, k) {
        ("room", "name") => ch.name = Some(v.to_string()),
        ("room", "number") => ch.number = Some(v.to_string()),
        ("floor", "kind") => ch.kind = Some(v.to_string()),
        (_, "thickness") => ch.thickness = Some(len(if kind == "ceiling" { 1.0 } else { 10.0 }, false)?),
        ("floor", "offset") => {
            let mm = parse_len(v, &units);
            if !mm.is_finite() {
                return Err(bt("model.err.length", &[]));
            }
            ch.offset = Some(mm);
        }
        ("ceiling", "height") => ch.height = Some(len(500.0, false)?),
        ("roof", "pitch") => {
            let p = parse_pitch(v);
            if !(p > 1.0 && p < 80.0) {
                return Err(bt("model.err.pitch", &[]));
            }
            ch.pitch = Some(p);
        }
        ("roof", "overhang") => ch.overhang = Some(len(0.0, true)?),
        ("roof", "ends") => {
            let r = sn.derived.roofs.iter().find(|x| x.guid == guid).ok_or_else(|| bt("model.err.gone", &[]))?;
            let wl = sn.levels.iter().find(|l| l.guid.as_deref() == Some(r.walls.as_str())).ok_or_else(|| bt("model.err.gone", &[]))?;
            let ws: Vec<_> = sn.walls.iter().filter(|w| w.storey == Some(wl.id)).cloned().collect();
            let Some(doc) = app.bimh.ed.as_mut() else { return Ok(()) };
            let lps = loops_of(&mut doc.ed.api, wl.id).map_err(|e| e.0)?;
            let o = loop_at(&lps.loops.outlines, r.seed).ok_or_else(|| bt("model.err.gable4", &[]))?;
            let mm = preset_edges(&o.poly, &ws, v, None).ok_or_else(|| bt("model.err.gable4", &[]))?;
            let f = format_edges(&mm);
            ch.edges = Some(if f.is_empty() { " ".into() } else { f });
            ch.gables = Some(Vec::new());
        }
        _ => return Err(format!("bim.edit: a {kind} has no {k}")),
    }
    commit(app, Op::ChangeDerived { guids: vec![guid], changes: ch }, &title, &changed, None);
    Ok(())
}

/// The Print button: the plan of the level as a PDF (model-pdf.js), saved through the host.
fn print(app: &mut UiApp) -> Result<(), String> {
    let dr = &app.bimh.draw;
    let (lv, _) = level_info(dr).ok_or_else(|| bt("model.needs.ifc", &[]))?;
    let mut sc = dr.scene.clone();
    for dd in &mut sc.dims {
        dd.off_px = None;
    }
    let units = dr.units.clone();
    let now = app.bimh.now();
    let when = cc_bim::draw::editor::iso_seconds(now).get(..10).unwrap_or("").to_string();
    let name = app.ifc_name().to_string();
    let base =
        if name.to_ascii_lowercase().ends_with(".ifc") { name.get(..name.len() - 4).unwrap_or(&name).to_string() } else { name.clone() };
    let lines = vec![base.clone(), bt("model.print.cut", &[("h", &fmt_len(CUT, &units))]), when];
    let r = cc_bim::draw::pdf::plan_pdf(
        &sc,
        &bt("model.print.titleof", &[("level", &lv.name)]),
        &lines,
        if units == "mm" { "a4" } else { "letter" },
        &units,
        None,
    );
    let file = format!("{base} — {}.pdf", lv.name);
    let save = app.services.save.as_mut().ok_or("Saving is not available here")?;
    if save(&file, r.bytes)?.is_some() {
        let mut msg = bt("model.print.done", &[("scale", &r.label)]);
        if r.missing > 0 {
            msg = format!("{msg} {}", bt("model.print.missing", &[("n", &r.missing.to_string())]));
        }
        app.message = msg;
    }
    Ok(())
}

fn schedule_data(app: &mut UiApp) -> Option<cc_bim::draw::schedule::Schedules> {
    let sn = app.bimh.draw.data.clone()?;
    let units = app.bimh.draw.units.clone();
    let doc = app.bimh.ed.as_mut()?;
    let mut loops = HashMap::new();
    for l in &sn.levels {
        if sn.derived.spaces.iter().any(|x| x.storey == Some(l.id))
            && let Ok(lp) = loops_of(&mut doc.ed.api, l.id)
        {
            loops.insert(cc_bim::draw::plan::key_u64(l.id), lp.loops);
        }
    }
    Some(cc_bim::draw::schedule::schedules(&sn, &loops, &units))
}

fn schedule_csv(app: &mut UiApp) -> Result<(), String> {
    let all = schedule_data(app).ok_or("Draw is off")?;
    let which = app.bimh.draw.sched.clone();
    let sc = all.get(&which).ok_or("bim.schedule: no such schedule")?;
    let name = app.ifc_name().to_string();
    let base = if name.to_ascii_lowercase().ends_with(".ifc") { name.get(..name.len() - 4).unwrap_or(&name).to_string() } else { name };
    let file = format!("{base} {}.csv", bt(&format!("model.sch.{which}"), &[]).to_lowercase());
    let mut bytes = "\u{feff}".as_bytes().to_vec();
    bytes.extend(cc_bim::draw::schedule::schedule_csv(sc).into_bytes());
    let save = app.services.save.as_mut().ok_or("Saving is not available here")?;
    if let Some(to) = save(&file, bytes)? {
        app.message = bt("qto.downloaded", &[("name", &to)]);
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------------- the plan view

fn in_poly(x: f64, y: f64, p: &[V2]) -> bool {
    m::in_poly(x, y, p)
}
fn seg_d(x: f64, y: f64, s: [f64; 4]) -> f64 {
    let (dx, dy) = (s[2] - s[0], s[3] - s[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 != 0.0 { m::max(0.0, m::min(1.0, ((x - s[0]) * dx + (y - s[1]) * dy) / l2)) } else { 0.0 };
    m::hypot(x - s[0] - t * dx, y - s[1] - t * dy)
}
fn near(x: f64, y: f64, poly: &[V2], tol: f64) -> bool {
    let n = poly.len();
    (0..n).any(|i| {
        let (a, b) = (m::at(poly, i), m::at(poly, m::nx(i, n)));
        seg_d(x, y, [a[0], a[1], b[0], b[1]]) <= tol
    })
}
fn in_loop(x: f64, y: f64, l: &[f64]) -> bool {
    let pts: Vec<V2> = l.as_chunks::<2>().0.to_vec();
    in_poly(x, y, &pts)
}

/// model-planview.js `hit(clientX, clientY)`: what a click there picks.
pub fn hit(scene: &Scene, view: &PView, p: V2, tol_px: f64) -> Option<f64> {
    let (x, y) = (p[0], p[1]);
    let tol = tol_px / view.scale;
    for s in &scene.symbols {
        if s.roof {
            continue;
        }
        if !s.sym.poly.is_empty() {
            if in_poly(x, y, &s.sym.poly) || near(x, y, &s.sym.poly, tol) {
                return Some(s.key);
            }
            continue;
        }
    }
    for w in &scene.walls {
        if w.pieces.iter().any(|p| in_poly(x, y, p)) {
            return Some(w.key);
        }
    }
    for w in &scene.walls {
        if !w.poly.is_empty() && near(x, y, &w.poly, tol) {
            return Some(w.key);
        }
    }
    for c in &scene.cut {
        if c.loops.iter().any(|l| in_loop(x, y, l)) {
            return Some(c.key);
        }
        for q in c.segs.as_chunks::<4>().0 {
            if seg_d(x, y, *q) <= tol {
                return Some(c.key);
            }
        }
    }
    for r in &scene.rooms {
        if in_poly(x, y, &r.poly) {
            return Some(r.key);
        }
    }
    for s in &scene.symbols {
        if s.roof && in_poly(x, y, &s.sym.poly) {
            return Some(s.key);
        }
    }
    None
}

/// The box round what the plan shows (for Fit).
pub fn bounds(scene: &Scene) -> Option<[f64; 4]> {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut g = |x: f64, y: f64| {
        b[0] = b[0].min(x);
        b[1] = b[1].min(y);
        b[2] = b[2].max(x);
        b[3] = b[3].max(y);
    };
    for w in &scene.walls {
        for q in &w.poly {
            g(q[0], q[1]);
        }
    }
    for c in scene.cut.iter().chain(scene.below.iter()) {
        for q in c.segs.as_chunks::<2>().0 {
            g(q[0], q[1]);
        }
    }
    for s in &scene.symbols {
        for q in &s.sym.poly {
            g(q[0], q[1]);
        }
    }
    b[0].is_finite().then_some(b)
}

/// The overlay of the current tool (`drawOverlay`).
fn overlay(dr: &Draw) -> Overlay {
    let mut o = Overlay::default();
    let units = &dr.units;
    match dr.tool.as_str() {
        "wall" => {
            if let Some((_, s)) = &dr.last {
                o.snap = Some((s.p, s.kind));
            }
            match (&dr.chain, &dr.last) {
                (Some(c), last) => {
                    o.points.extend(c.points.iter().copied());
                    if let (Some(&a), Some((_, s))) = (c.points.last(), last) {
                        let b = s.p;
                        let l = m::hypot(b[0] - a[0], b[1] - a[1]);
                        if l >= 1.0
                            && let Some(ty) = wall_type(&dr.wall.type_key)
                        {
                            let seg = Seg { key: "*".into(), a, b, t: type_thickness(&ty) };
                            let mut walls: Vec<Seg> = walls_here(dr).iter().map(|w| w.seg()).collect();
                            walls.push(seg.clone());
                            let poly = outline_of(&wall_outlines(&walls, 1.0), "*").cloned().unwrap_or_default();
                            o.walls.push(poly);
                            let ang = m::atan2(b[1] - a[1], b[0] - a[0]) * 180.0 / std::f64::consts::PI;
                            let close =
                                if closes_chain(&c.points, b, 1.0) { format!("\n{}", bt("model.hud.close", &[])) } else { String::new() };
                            o.hud = Some((b, format!("{}  {}{close}", fmt_len(l, units), fmt_angle(ang))));
                            o.dims.push(Dim {
                                a,
                                b,
                                text: fmt_len(l, units),
                                off_px: None,
                                off_mm: None,
                                off: Some(seg.t / 2.0 + dr.view.px_to_mm(18.0)),
                                key: None,
                            });
                        }
                    }
                }
                (None, Some((_, s))) => o.hud = Some((s.p, bt("model.hud.start", &[]))),
                _ => {}
            }
        }
        "door" | "window" => {
            if let Some(h) = &dr.ohover {
                let walls = walls_here(dr);
                if let Some(w) = walls.get(h.wall) {
                    let os = oset(dr, &dr.tool);
                    let spec = OpSpec {
                        door: dr.tool == "door",
                        type_key: os.type_key.clone(),
                        width: os.width,
                        height: os.height,
                        sill: os.sill,
                        hand: os.hand.clone(),
                        face: h.face.clone(),
                        offset: h.offset,
                    };
                    o.symbols.push(opening_symbol(&spec, w.a, w.b, w.t));
                    let u = [(w.b[0] - w.a[0]) / w.len, (w.b[1] - w.a[1]) / w.len];
                    let n = [-u[1], u[0]];
                    let sg = if h.face == "right" { -1.0 } else { 1.0 };
                    let pp = |x: f64| [w.a[0] + u[0] * x + n[0] * sg * w.t / 2.0, w.a[1] + u[1] * x + n[1] * sg * w.t / 2.0];
                    let off = sg * dr.view.px_to_mm(22.0);
                    let [e0, e1] = h.ends;
                    if h.offset - e0 > 0.5 {
                        o.dims.push(Dim {
                            a: pp(e0),
                            b: pp(h.offset),
                            text: fmt_len(h.offset - e0, units),
                            off_px: None,
                            off_mm: None,
                            off: Some(off),
                            key: None,
                        });
                    }
                    if e1 - h.offset - os.width > 0.5 {
                        o.dims.push(Dim {
                            a: pp(h.offset + os.width),
                            b: pp(e1),
                            text: fmt_len(e1 - h.offset - os.width, units),
                            off_px: None,
                            off_mm: None,
                            off: Some(off),
                            key: None,
                        });
                    }
                    let name = opening_type(&os.type_key).map(|t| opening_type_name(&t)).unwrap_or_default();
                    let text = if h.ok {
                        format!("{name}  {} × {}", fmt_len(os.width, units), fmt_len(os.height, units))
                    } else {
                        bt("model.err.nofit", &[])
                    };
                    o.hud = Some((pp(h.offset + os.width / 2.0), text));
                }
            }
        }
        "floor" | "ceiling" | "roof" => {
            if let Some(h) = &dr.shover {
                o.walls.push(h.poly.clone());
                let mut text = fmt_area(h.area.abs(), units);
                if dr.tool == "roof" {
                    let edges = roof_edges(dr, &h.poly, h.seed);
                    let rf = &dr.slab;
                    let dd = roof_defaults();
                    let plan = edges.ok_or("model.err.gable4").and_then(|g| {
                        let kinds = edge_kinds(
                            &h.poly,
                            &walls_here(dr),
                            &cc_bim::draw::roof::RoofSet {
                                pitch: rf.pitch,
                                overhang: rf.overhang,
                                rake: None,
                                gables: Vec::new(),
                                edges: g,
                            },
                        );
                        roof_geometry(&h.poly, &kinds, dd.thickness, 0.0, if rf.fascia { dd.fascia } else { 0.0 }, dd.fascia_t, rf.fascia)
                    });
                    match plan {
                        Err(e) => text = bt(e, &[]),
                        Ok(plan) => {
                            let ne = plan.eave.len();
                            let dashed = (0..ne)
                                .map(|i| {
                                    let (a, b) = (m::at(&plan.eave, i), m::at(&plan.eave, m::nx(i, ne)));
                                    [a[0], a[1], b[0], b[1]]
                                })
                                .collect();
                            o.symbols.push(Symbol { dashed, thin: plan.ridge.clone(), ..Symbol::default() });
                            text = format!("{}  {}", fmt_pitch(rf.pitch, units), fmt_area(plan.area, units));
                        }
                    }
                }
                o.hud = Some((label_point(&h.poly), text));
            }
        }
        _ => {}
    }
    o
}

/// Colours of the plan from the theme (model-planview.js reads the page's tokens).
struct Pal {
    sheet: Color32,
    ink: Color32,
    ink2: Color32,
    action: Color32,
    poche: Color32,
    grid: Color32,
    blue: Color32,
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}
fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// Paint the plan (`draw()`), overlay last.
fn paint(ui: &egui::Ui, rect: Rect, dr: &Draw, sel: Option<f64>, t: &crate::theme::Tokens) {
    let pal = Pal {
        sheet: t.canvas_bg,
        ink: t.text_strong,
        ink2: t.text_dim,
        action: t.accent,
        poche: mix(t.canvas_bg, t.text_strong, 0.35),
        grid: mix(t.canvas_bg, t.text_dim, 0.25),
        blue: t.accent,
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, pal.sheet);
    let v = &dr.view;
    let o = rect.min;
    let s = |p: V2| v.to_screen(p, o);
    // grid: 1 m (or 1 ft) once 12 px apart
    let unit = if dr.units == "mm" { 1000.0 } else { 304.8 };
    if unit * v.scale >= 12.0 {
        let p0 = v.from_screen(rect.min, o);
        let p1 = v.from_screen(rect.max, o);
        let (x0, x1, y0, y1) = (p0[0].min(p1[0]), p0[0].max(p1[0]), p0[1].min(p1[1]), p0[1].max(p1[1]));
        let stroke = Stroke::new(1.0, alpha(pal.grid, 0.5));
        let mut x = (x0 / unit).ceil() * unit;
        let mut n = 0;
        while x <= x1 && n < 2000 {
            let sx = s([x, 0.0]).x.round() + 0.5;
            painter.line_segment([Pos2::new(sx, rect.min.y), Pos2::new(sx, rect.max.y)], stroke);
            x += unit;
            n += 1;
        }
        let mut y = (y0 / unit).ceil() * unit;
        while y <= y1 && n < 4000 {
            let sy = s([0.0, y]).y.round() + 0.5;
            painter.line_segment([Pos2::new(rect.min.x, sy), Pos2::new(rect.max.x, sy)], stroke);
            y += unit;
            n += 1;
        }
    }
    let sc = &dr.scene;
    let is_sel = |k: f64| sel == Some(k);
    let is_hov = |k: f64| dr.hover_key == Some(k);
    let segs = |a: &[f64], c: Color32, w: f32| {
        for q in a.as_chunks::<4>().0 {
            painter.line_segment([s([q[0], q[1]]), s([q[2], q[3]])], Stroke::new(w, c));
        }
    };
    let poly_path = |p: &[V2]| -> Vec<Pos2> { p.iter().map(|q| s(*q)).collect() };
    // below the cut: thin
    for b in &sc.below {
        segs(&b.segs, alpha(if is_sel(b.key) { pal.action } else { pal.ink2 }, 0.7), 1.0);
    }
    // rooms: a faint fill
    for r in &sc.rooms {
        fill_poly(
            &painter,
            &poly_path(&r.poly),
            alpha(if is_sel(r.key) { pal.action } else { pal.blue }, if is_sel(r.key) { 0.16 } else { 0.06 }),
        );
    }
    // cut elements from elsewhere: filled loops, outlines
    for c in &sc.cut {
        let fc = alpha(if is_sel(c.key) { pal.action } else { pal.poche }, if is_sel(c.key) { 0.35 } else { 0.8 });
        for l in &c.loops {
            let pts: Vec<Pos2> = l.as_chunks::<2>().0.iter().map(|q| s(*q)).collect();
            fill_poly(&painter, &pts, fc);
        }
        segs(&c.segs, if is_sel(c.key) || is_hov(c.key) { pal.action } else { pal.ink }, 1.5);
    }
    // drawn walls: poché and one heavy outline round them
    for w in &sc.walls {
        for p in &w.pieces {
            fill_poly(&painter, &poly_path(p), if is_sel(w.key) { alpha(pal.action, 0.45) } else { pal.poche });
        }
    }
    if let Some(edges) = &sc.wall_edges {
        for p in edges {
            closed_line(&painter, &poly_path(p), Stroke::new(1.75, pal.ink));
        }
        for w in &sc.walls {
            if is_sel(w.key) || is_hov(w.key) {
                for p in &w.pieces {
                    closed_line(&painter, &poly_path(p), Stroke::new(if is_sel(w.key) { 2.5 } else { 1.75 }, pal.action));
                }
            }
        }
    }
    for sy in &sc.symbols {
        symbol(&painter, &sy.sym, if is_sel(sy.key) || is_hov(sy.key) { pal.action } else { pal.ink }, &s, v.scale);
    }
    for dd in &sc.dims {
        dim(&painter, dd, pal.ink2, pal.sheet, &s, v.scale);
    }
    for r in &sc.rooms {
        let p = s(r.at);
        let n = r.label.len() as f32;
        for (i, l) in r.label.iter().enumerate() {
            let font = if i == 0 { egui::FontId::proportional(12.0) } else { egui::FontId::proportional(11.0) };
            painter.text(
                Pos2::new(p.x, p.y + (i as f32 - (n - 1.0) / 2.0) * 15.0),
                egui::Align2::CENTER_CENTER,
                l,
                font,
                if is_sel(r.key) { pal.action } else { pal.ink },
            );
        }
    }
    // overlay
    let ov = overlay(dr);
    for p in &ov.walls {
        let pts = poly_path(p);
        fill_poly(&painter, &pts, alpha(pal.action, 0.18));
        closed_line(&painter, &pts, Stroke::new(1.5, pal.action));
    }
    for sy in &ov.symbols {
        symbol(&painter, sy, pal.action, &s, v.scale);
    }
    for dd in &ov.dims {
        dim(&painter, dd, pal.action, pal.sheet, &s, v.scale);
    }
    for q in &ov.points {
        painter.circle_filled(s(*q), 3.0, pal.action);
    }
    if let Some((p, kind)) = ov.snap {
        let q = s(p);
        let st = Stroke::new(1.5, pal.action);
        match kind {
            "end" => {
                painter.rect_stroke(Rect::from_center_size(q, egui::vec2(10.0, 10.0)), 0.0, st, egui::StrokeKind::Middle);
            }
            "mid" => {
                closed_line(&painter, &[Pos2::new(q.x, q.y - 6.0), Pos2::new(q.x + 6.0, q.y + 5.0), Pos2::new(q.x - 6.0, q.y + 5.0)], st)
            }
            "on" => {
                painter.line_segment([q + egui::vec2(-5.0, -5.0), q + egui::vec2(5.0, 5.0)], st);
                painter.line_segment([q + egui::vec2(5.0, -5.0), q + egui::vec2(-5.0, 5.0)], st);
            }
            _ => {
                painter.line_segment([q + egui::vec2(-7.0, 0.0), q + egui::vec2(7.0, 0.0)], st);
                painter.line_segment([q + egui::vec2(0.0, -7.0), q + egui::vec2(0.0, 7.0)], st);
            }
        }
    }
    if let Some((p, text)) = &ov.hud {
        let q = s(*p);
        let font = egui::FontId::proportional(12.0);
        let lines: Vec<&str> = text.split('\n').collect();
        let galleys: Vec<_> = lines.iter().map(|l| ui.fonts_mut(|f| f.layout_no_wrap((*l).to_string(), font.clone(), pal.sheet))).collect();
        let wd = galleys.iter().map(|g| g.size().x).fold(0.0f32, f32::max) + 10.0;
        let ht = lines.len() as f32 * 16.0 + 4.0;
        let x = (q.x + 14.0).min(rect.max.x - wd - 4.0);
        let y = (q.y + 14.0).min(rect.max.y - ht - 4.0);
        painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), egui::vec2(wd, ht)), 0.0, alpha(pal.ink, 0.85));
        for (i, g) in galleys.into_iter().enumerate() {
            painter.galley(Pos2::new(x + 5.0, y + 3.0 + i as f32 * 16.0), g, pal.sheet);
        }
    }
}

fn fill_poly(painter: &egui::Painter, pts: &[Pos2], c: Color32) {
    if pts.len() < 3 {
        return;
    }
    // even-odd fill of any polygon: triangulated by the mesh helper (ear clipping) in screen space
    let flat: Vec<f64> = pts.iter().flat_map(|p| [f64::from(p.x), f64::from(p.y)]).collect();
    let tris = earcut(&flat);
    let mut mesh = egui::Mesh::default();
    for p in pts {
        mesh.colored_vertex(*p, c);
    }
    for t in tris.as_chunks::<3>().0 {
        mesh.add_triangle(t[0], t[1], t[2]);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Ear clipping of a simple polygon (screen points) → triangle corner indices.
fn earcut(flat: &[f64]) -> Vec<u32> {
    let pts: Vec<V2> = flat.as_chunks::<2>().0.to_vec();
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let p = |i: usize| m::at(&pts, i);
    let mut idx: Vec<usize> = (0..n).collect();
    if m::area(&pts) < 0.0 {
        idx.reverse();
    }
    let at = |idx: &[usize], i: usize| idx.get(i).copied().unwrap_or(0);
    let u = |i: usize| u32::try_from(i).unwrap_or(0);
    let mut out = Vec::new();
    let mut guard = 0usize;
    while idx.len() > 3 && guard < n.saturating_mul(n).saturating_add(8) {
        guard = guard.saturating_add(1);
        let k = idx.len();
        let mut cut = None;
        for i in 0..k {
            let (ia, ib, ic) = (at(&idx, m::pv(i, k)), at(&idx, i), at(&idx, m::nx(i, k)));
            let (a, b, c) = (p(ia), p(ib), p(ic));
            let cr = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
            if cr <= 0.0 {
                continue;
            }
            let inside = idx.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let q = p(j);
                let s1 = (b[0] - a[0]) * (q[1] - a[1]) - (b[1] - a[1]) * (q[0] - a[0]);
                let s2 = (c[0] - b[0]) * (q[1] - b[1]) - (c[1] - b[1]) * (q[0] - b[0]);
                let s3 = (a[0] - c[0]) * (q[1] - c[1]) - (a[1] - c[1]) * (q[0] - c[0]);
                s1 > 0.0 && s2 > 0.0 && s3 > 0.0
            });
            if inside {
                continue;
            }
            out.extend([u(ia), u(ib), u(ic)]);
            cut = Some(i);
            break;
        }
        match cut {
            Some(i) => {
                idx.remove(i);
            }
            None => break,
        }
    }
    if let [a, b, c] = idx[..] {
        out.extend([u(a), u(b), u(c)]);
    }
    out
}

fn closed_line(painter: &egui::Painter, pts: &[Pos2], st: Stroke) {
    if pts.len() < 2 {
        return;
    }
    let mut p = pts.to_vec();
    if let Some(&f) = pts.first() {
        p.push(f);
    }
    painter.add(egui::Shape::line(p, st));
}

fn symbol(painter: &egui::Painter, sy: &Symbol, c: Color32, s: &dyn Fn(V2) -> Pos2, scale: f64) {
    for l in &sy.lines {
        painter.line_segment([s([l[0], l[1]]), s([l[2], l[3]])], Stroke::new(1.25, c));
    }
    for a in &sy.arcs {
        let steps = ((a.a1 - a.a0).abs() / (std::f64::consts::PI / 32.0)).ceil().clamp(8.0, 128.0) as usize;
        let pts: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let t = a.a0 + (a.a1 - a.a0) * i as f64 / steps as f64;
                s([a.c[0] + a.r * m::cos(t), a.c[1] + a.r * m::sin(t)])
            })
            .collect();
        let _ = scale;
        painter.add(egui::Shape::line(pts, Stroke::new(0.75, c)));
    }
    for l in &sy.thin {
        painter.line_segment([s([l[0], l[1]]), s([l[2], l[3]])], Stroke::new(0.75, c));
    }
    for l in &sy.dashed {
        painter.extend(egui::Shape::dashed_line(&[s([l[0], l[1]]), s([l[2], l[3]])], Stroke::new(0.75, c), 4.0, 3.0));
    }
}

/// A dimension: measured along a → b, drawn `off` to its left, ticks and the text (model-planview.js `drawDim`).
fn dim(painter: &egui::Painter, d0: &Dim, c: Color32, sheet: Color32, s: &dyn Fn(V2) -> Pos2, scale: f64) {
    let l = m::hypot(d0.b[0] - d0.a[0], d0.b[1] - d0.a[1]);
    if l < 1.0 {
        return;
    }
    let u = [(d0.b[0] - d0.a[0]) / l, (d0.b[1] - d0.a[1]) / l];
    let n = [-u[1], u[0]];
    let off = match d0.off_px {
        Some(px) => px / scale,
        None => d0.off.unwrap_or(0.0),
    };
    let a = s([d0.a[0] + n[0] * off, d0.a[1] + n[1] * off]);
    let b = s([d0.b[0] + n[0] * off, d0.b[1] + n[1] * off]);
    let (ea, eb) = (s(d0.a), s(d0.b));
    let len = (b - a).length();
    if len < 18.0 {
        return;
    }
    let st = Stroke::new(0.75, c);
    painter.line_segment([a, b], st);
    for (e, p) in [(ea, a), (eb, b)] {
        let dv = p - e;
        let l2 = dv.length();
        if l2 >= 1.0 {
            let un = dv / l2;
            painter.line_segment([e + un * 3.0, p + un * 4.0], st);
        }
    }
    let sv = (b - a) / len;
    for p in [a, b] {
        let tick = egui::vec2(sv.x - sv.y, sv.y + sv.x) * 4.0;
        painter.line_segment([p - tick, p + tick], st);
    }
    let mut ang = (b.y - a.y).atan2(b.x - a.x);
    if ang > std::f32::consts::FRAC_PI_2 || ang <= -std::f32::consts::FRAC_PI_2 {
        ang += std::f32::consts::PI;
    }
    let font = egui::FontId::proportional(11.0);
    let galley = painter.layout_no_wrap(d0.text.clone(), font, c);
    let tw = galley.size().x;
    if tw + 6.0 < len {
        let mid = Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        let rot = egui::emath::Rot2::from_angle(ang);
        let up = rot * egui::vec2(0.0, -3.0 - galley.size().y);
        let start = mid + up + rot * egui::vec2(-tw / 2.0, 0.0);
        let _ = sheet;
        painter.add(egui::epaint::TextShape::new(start, galley, c).with_angle(ang));
    }
}

/// The plan view: paint, pan / zoom, and clicks / pointer moves as `bim.point` / previews.
pub fn plan_ui(app: &mut UiApp, ui: &mut egui::Ui, rect: Rect) {
    let t = app.tokens;
    {
        let dr = &mut app.bimh.draw;
        dr.view.w = f64::from(rect.width());
        dr.view.h = f64::from(rect.height());
        if dr.pending_fit && rect.width() > 0.0 {
            let b = bounds(&dr.scene);
            dr.view.fit(b);
            dr.pending_fit = false;
        }
    }
    let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    let o = rect.min;
    let (wheel, shift) = ui.input(|i| (i.smooth_scroll_delta.y, i.modifiers.shift));
    if resp.hovered()
        && wheel != 0.0
        && let Some(p) = ui.input(|i| i.pointer.hover_pos())
    {
        let k = (f64::from(wheel) * 0.0015 * 1.3).exp();
        app.bimh.draw.view.zoom_at(p, o, k);
    }
    let pan_drag = resp.dragged_by(egui::PointerButton::Middle)
        || resp.dragged_by(egui::PointerButton::Secondary)
        || (resp.dragged_by(egui::PointerButton::Primary) && app.bimh.draw.tool == "select");
    if pan_drag {
        let dv = resp.drag_delta();
        let sc = app.bimh.draw.view.scale;
        app.bimh.draw.view.cx -= f64::from(dv.x) / sc;
        app.bimh.draw.view.cy += f64::from(dv.y) / sc;
    }
    if let Some(p) = resp.hover_pos() {
        let pt = app.bimh.draw.view.from_screen(p, o);
        on_move(app, pt, shift);
    }
    if resp.clicked_by(egui::PointerButton::Primary)
        && let Some(p) = resp.interact_pointer_pos()
    {
        let pt = app.bimh.draw.view.from_screen(p, o);
        let arg = format!("{},{}{}", cc_bim::js::num_str(pt[0]), cc_bim::js::num_str(pt[1]), if shift { ",shift" } else { "" });
        app.run_ui("bim.point", &Params { name: Some(arg), ..Default::default() });
    }
    if resp.double_clicked() && app.bimh.draw.tool == "wall" {
        app.run_ui("bim.end", &Params::default());
    }
    if resp.secondary_clicked() && !pan_drag {
        app.bimh.draw.chain = None;
    }
    let sel = app.picked_product();
    paint(ui, rect, &app.bimh.draw, sel, &t);
}

// ------------------------------------------------------------------------------------------------------- the bars

/// The model bar over the plan: level, tools, the tool's options, typed length, views, Fit, units, dimensions, Print.
pub fn bar(app: &mut UiApp, ui: &mut egui::Ui) {
    let t = app.tokens;
    let mut run: Vec<(&'static str, String)> = Vec::new();
    let st = BarState::of(&app.bimh.draw);
    let dr = &st;
    let mut typed = app.bimh.draw.typed.clone();
    let mut sched_toggle = false;
    let levels: Vec<(usize, String, f64)> = app
        .bimh
        .draw
        .data
        .as_ref()
        .map(|s| s.levels.iter().enumerate().map(|(i, l)| (i, l.name.clone(), l.id)).collect())
        .unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        let cur = levels.iter().find(|l| Some(l.2) == dr.level).map(|l| l.1.clone()).unwrap_or_default();
        egui::ComboBox::new("bim_level", bt("model.level", &[])).selected_text(cur).show_ui(ui, |ui| {
            for (i, n, id) in levels.iter().rev() {
                if ui.selectable_label(Some(*id) == dr.level, n).clicked() {
                    run.push(("bim.level", i.to_string()));
                }
            }
        });
        ui.separator();
        for (k, key) in [
            ("select", "model.tool.select"),
            ("wall", "model.tool.wall"),
            ("door", "model.tool.door"),
            ("window", "model.tool.window"),
            ("floor", "model.tool.floor"),
            ("ceiling", "model.tool.ceiling"),
            ("roof", "model.tool.roof"),
        ] {
            if ui.add(egui::Button::new(bt(key, &[])).selected(dr.tool == k)).on_hover_text(bt(&format!("{key}.title"), &[])).clicked() {
                run.push(("bim.tool", k.into()));
            }
        }
        ui.separator();
        match dr.tool.as_str() {
            "wall" => {
                let cur = wall_type(&dr.wall.type_key).map(|w| type_name(&w)).unwrap_or_default();
                egui::ComboBox::new("bim_walltype", bt("model.wall.type", &[])).selected_text(cur).show_ui(ui, |ui| {
                    for w in wall_types() {
                        let label = format!("{} ({})", type_name(&w), fmt_len(type_thickness(&w), &dr.units));
                        if ui.selectable_label(w.key == dr.wall.type_key, label).clicked() {
                            run.push(("bim.set", format!("wall.type={}", w.key)));
                        }
                    }
                });
                let top = if dr.wall.top == "fixed" { bt("model.wall.top.fixed", &[]) } else { bt("model.wall.top.level", &[]) };
                egui::ComboBox::new("bim_walltop", bt("model.wall.top", &[])).selected_text(top).show_ui(ui, |ui| {
                    if ui.selectable_label(dr.wall.top == "level", bt("model.wall.top.level", &[])).clicked() {
                        run.push(("bim.set", "wall.top=level".into()));
                    }
                    if ui.selectable_label(dr.wall.top == "fixed", bt("model.wall.top.fixed", &[])).clicked() {
                        run.push(("bim.set", "wall.top=fixed".into()));
                    }
                });
                if dr.wall.top == "fixed" {
                    ui.label(
                        RichText::new(format!("{} {}", bt("model.wall.height", &[]), fmt_len(dr.wall.height, &dr.units))).color(t.text_dim),
                    );
                }
            }
            "door" | "window" => {
                let os = oset_b(dr, &dr.tool);
                let list = if dr.tool == "door" { door_types() } else { window_types() };
                let cur = opening_type(&os.type_key).map(|t| opening_type_name(&t)).unwrap_or_default();
                let k = dr.tool.clone();
                egui::ComboBox::new(("bim_otype", k.clone()), bt("model.ot.type", &[])).selected_text(cur).show_ui(ui, |ui| {
                    for ty in list {
                        if ui.selectable_label(ty.key == os.type_key, opening_type_name(&ty)).clicked() {
                            run.push(("bim.set", format!("{k}.type={}", ty.key)));
                        }
                    }
                });
                ui.label(RichText::new(format!("{} × {}", fmt_len(os.width, &dr.units), fmt_len(os.height, &dr.units))).color(t.text_dim));
                if ui.button(bt("model.ot.flip", &[])).on_hover_text(bt("model.ot.flip.title", &[])).clicked() {
                    run.push(("bim.flip", String::new()));
                }
            }
            "floor" | "ceiling" => {
                let building = if dr.tool == "floor" { dr.slab.floor_mode == "building" } else { dr.slab.ceil_mode == "building" };
                let k = dr.tool.clone();
                for (m2, key) in [("room", "model.fl.room"), ("building", "model.fl.building")] {
                    if ui.selectable_label((m2 == "building") == building, bt(key, &[])).clicked() {
                        run.push(("bim.set", format!("{k}.mode={m2}")));
                    }
                }
                if k == "floor" {
                    for (fk, _, _) in FLOOR_KINDS {
                        if ui.selectable_label(dr.slab.floor_kind == fk, bt(&format!("model.floor.{fk}"), &[])).clicked() {
                            run.push(("bim.set", format!("floor.kind={fk}")));
                        }
                    }
                }
            }
            "roof" => {
                ui.label(RichText::new(format!("{} {}", bt("model.rf.pitch", &[]), fmt_pitch(dr.slab.pitch, &dr.units))).color(t.text_dim));
                for (e, key) in [
                    ("hip", "model.rf.hip"),
                    ("gable-short", "model.rf.gable.short"),
                    ("gable-long", "model.rf.gable.long"),
                    ("shed", "res.rf.shed"),
                ] {
                    if ui.selectable_label(dr.slab.ends == e, bt(key, &[])).clicked() {
                        run.push(("bim.set", format!("roof.ends={e}")));
                    }
                }
            }
            _ => {}
        }
        if dr.tool == "wall" || dr.tool == "door" || dr.tool == "window" {
            let l = ui.label(bt("model.typed", &[]));
            let r = ui.add(egui::TextEdit::singleline(&mut typed).desired_width(90.0).id_salt("bim_typed")).labelled_by(l.id);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                run.push(("bim.typed", typed.clone()));
            }
        }
        ui.separator();
        for (k, key) in [("plan", "model.layout.plan"), ("split", "model.layout.split"), ("3d", "model.layout.3d")] {
            if ui.selectable_label(dr.layout == k, bt(key, &[])).clicked() {
                run.push(("bim.layout", k.into()));
            }
        }
        if ui.button(bt("model.fit", &[])).on_hover_text(bt("model.fit.title", &[])).clicked() {
            run.push(("bim.fit", String::new()));
        }
        let ul = if dr.units == "mm" { "model.units.mm" } else { "model.units.ftin" };
        if ui.button(bt(ul, &[])).on_hover_text(bt("model.units", &[])).clicked() {
            run.push(("bim.units", if dr.units == "mm" { "ftin".into() } else { "mm".into() }));
        }
        if ui.add(egui::Button::new(bt("model.dims", &[])).selected(dr.dims)).on_hover_text(bt("model.dims.title", &[])).clicked() {
            run.push(("bim.dims", String::new()));
        }
        if ui.button(bt("model.print", &[])).on_hover_text(bt("model.print.title", &[])).clicked() {
            run.push(("bim.print", String::new()));
        }
        if ui.add(egui::Button::new(bt("model.sch.title", &[])).selected(dr.show_sched)).clicked() {
            sched_toggle = true;
        }
    });
    app.bimh.draw.typed = typed;
    if sched_toggle {
        app.bimh.draw.show_sched = !app.bimh.draw.show_sched;
    }
    let hint = format!("model.hint.{}", app.bimh.draw.tool);
    ui.label(RichText::new(bt(&hint, &[])).small().color(t.text_dim));
    for (id, arg) in run {
        app.run_ui(id, &Params { name: (!arg.is_empty()).then_some(arg), ..Default::default() });
    }
}

/// What the bar shows, read once per frame.
struct BarState {
    tool: String,
    wall: WallSet,
    door: OSet,
    window: OSet,
    slab: SlabSet,
    units: String,
    dims: bool,
    layout: String,
    level: Option<f64>,
    show_sched: bool,
}

impl BarState {
    fn of(d: &Draw) -> BarState {
        BarState {
            tool: d.tool.clone(),
            wall: d.wall.clone(),
            door: d.door.clone(),
            window: d.window.clone(),
            slab: d.slab.clone(),
            units: d.units.clone(),
            dims: d.dims,
            layout: d.layout.clone(),
            level: d.level,
            show_sched: d.show_sched,
        }
    }
}

fn oset_b<'a>(b: &'a BarState, kind: &str) -> &'a OSet {
    if kind == "door" { &b.door } else { &b.window }
}

/// The Levels panel: each level's name, elevation and height (one change each), Add a level on top.
pub fn levels_panel(app: &mut UiApp, ui: &mut egui::Ui) {
    let t = app.tokens;
    let Some(sn) = app.bimh.draw.data.clone() else { return };
    let units = app.bimh.draw.units.clone();
    let mut out: Vec<(&'static str, String)> = Vec::new();
    egui::CollapsingHeader::new(RichText::new(bt("model.levels", &[])).color(t.text_strong)).id_salt("bim_levels").default_open(true).show(
        ui,
        |ui| {
            for (i, l) in sn.levels.iter().enumerate().rev() {
                ui.horizontal(|ui| {
                    if ui.radio(app.bimh.draw.level == Some(l.id), "").on_hover_text(bt("model.lv.show", &[("name", &l.name)])).clicked() {
                        out.push(("bim.level", i.to_string()));
                    }
                    ui.label(RichText::new(&l.name).color(t.text));
                    ui.label(
                        RichText::new(format!("{} · {}", fmt_len(l.elevation, &units), fmt_len(l.height, &units)))
                            .small()
                            .color(t.text_dim),
                    );
                });
            }
            if ui.button(bt("model.lv.add", &[])).clicked() {
                out.push(("bim.level.add", String::new()));
            }
            ui.label(RichText::new(bt("model.levels.note", &[])).small().color(t.text_dim));
        },
    );
    for (id, arg) in out {
        app.run_ui(id, &Params { name: (!arg.is_empty()).then_some(arg), ..Default::default() });
    }
}

/// The schedules (model-schedule.js) below the levels.
pub fn schedules_panel(app: &mut UiApp, ui: &mut egui::Ui) {
    if !app.bimh.draw.show_sched {
        return;
    }
    let t = app.tokens;
    let Some(all) = schedule_data(app) else { return };
    let which = app.bimh.draw.sched.clone();
    let mut out: Vec<(&'static str, String)> = Vec::new();
    egui::CollapsingHeader::new(RichText::new(bt("model.sch.title", &[])).color(t.text_strong))
        .id_salt("bim_sched")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for k in ["doors", "windows", "walls", "rooms"] {
                    if ui.selectable_label(which == k, bt(&format!("model.sch.{k}"), &[])).clicked() {
                        out.push(("bim.schedule", k.into()));
                    }
                }
                if ui.button(bt("model.sch.csv", &[])).clicked() {
                    out.push(("bim.schedule", format!("{which}:csv")));
                }
            });
            let Some(sc) = all.get(&which) else { return };
            if sc.rows.is_empty() {
                ui.label(RichText::new(bt("model.sch.none", &[])).small().color(t.text_dim));
                return;
            }
            egui::Grid::new("bim_sched_grid").striped(true).show(ui, |ui| {
                for h in &sc.head {
                    ui.label(RichText::new(h).small().strong());
                }
                ui.end_row();
                for r in &sc.rows {
                    for c in r {
                        ui.label(RichText::new(c).small());
                    }
                    ui.end_row();
                }
                if let Some(tot) = &sc.total {
                    for (i, _) in sc.head.iter().enumerate() {
                        ui.label(RichText::new(tot.get(i).cloned().unwrap_or_default()).small().strong());
                    }
                    ui.end_row();
                }
            });
        });
    for (id, arg) in out {
        app.run_ui(id, &Params { name: Some(arg), ..Default::default() });
    }
}

/// The drawn element's own Properties (`propsBlock`): fields that are one change each.
pub fn props_block(app: &mut UiApp, ui: &mut egui::Ui) {
    let (walls, openings, derived, title) = picked(app);
    if walls.is_empty() && openings.is_empty() && derived.is_empty() {
        return;
    }
    let t = app.tokens;
    let Some(id) = app.picked_product() else { return };
    let Some(sn) = app.bimh.draw.data.clone() else { return };
    let units = app.bimh.draw.units.clone();
    let mut out: Option<String> = None;
    ui.label(RichText::new(&title).strong().color(t.text_strong));
    let row = |app: &mut UiApp, ui: &mut egui::Ui, key: &str, label: &str, cur: String| {
        let l = ui.label(RichText::new(label).small().color(t.text_dim));
        let mut text = match &app.bimh.editing {
            Some((k, v)) if k == key => v.clone(),
            _ => cur.clone(),
        };
        let r =
            ui.add(egui::TextEdit::singleline(&mut text).desired_width(ui.available_width()).id_salt(("bim_mp", key))).labelled_by(l.id);
        if r.has_focus() || r.changed() {
            app.bimh.editing = Some((key.to_string(), text.clone()));
        }
        if r.lost_focus()
            && let Some((k, v)) = app.bimh.editing.take()
            && k == key
            && v != cur
        {
            return Some(format!("{}={v}", key.trim_start_matches("mp.")));
        }
        None
    };
    if let Some(w0) = sn.walls.iter().find(|x| x.id == id) {
        let cur = wall_type(&w0.type_key).map(|w| type_name(&w)).unwrap_or_default();
        egui::ComboBox::new("bim_mp_type", bt("model.wall.type", &[])).selected_text(cur).show_ui(ui, |ui| {
            for w in wall_types() {
                if ui.selectable_label(w.key == w0.type_key, type_name(&w)).clicked() {
                    out = Some(format!("type={}", w.key));
                }
            }
        });
        for (k, key, v) in [
            ("mp.thickness", "model.prop.thickness", w0.t),
            ("mp.length", "model.prop.length", w0.len),
            ("mp.height", "model.wall.height", w0.height),
            ("mp.base", "model.prop.base", w0.base_offset),
        ] {
            if let Some(x) = row(app, ui, k, &bt(key, &[]), fmt_len(v, &units)) {
                out = Some(x);
            }
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(bt("model.prop.outside", &[])).small().color(t.text_dim));
            ui.label(bt(if w0.ext_side == "right" { "model.prop.right" } else { "model.prop.left" }, &[]));
            if ui.button(bt("model.ot.flip", &[])).clicked() {
                out = Some("outside=flip".into());
            }
        });
        let top_on = !w0.top_level.is_empty();
        if ui.selectable_label(top_on, bt("model.wall.top.level", &[])).clicked() && !top_on {
            out = Some("top=level".into());
        }
        if ui.selectable_label(!top_on, bt("model.wall.top.fixed", &[])).clicked() && top_on {
            out = Some("top=fixed".into());
        }
    } else if let Some(o0) = sn.openings.iter().find(|x| x.id == id) {
        let list = if o0.door { door_types() } else { window_types() };
        let cur = cc_bim::draw::openings::type_key_name(&o0.type_key);
        egui::ComboBox::new("bim_mp_otype", bt("model.ot.type", &[])).selected_text(cur).show_ui(ui, |ui| {
            for ty in list {
                if ui.selectable_label(ty.key == o0.type_key, opening_type_name(&ty)).clicked() {
                    out = Some(format!("type={}", ty.key));
                }
            }
        });
        let mut rows = vec![("mp.width", "model.ot.width", o0.width), ("mp.height", "model.ot.height", o0.height)];
        if !o0.door {
            rows.push(("mp.sill", "model.ot.sill", o0.sill));
        }
        for (k, key, v) in rows {
            if let Some(x) = row(app, ui, k, &bt(key, &[]), fmt_len(v, &units)) {
                out = Some(x);
            }
        }
        ui.horizontal(|ui| {
            if ui.button(format!("{}: {}", bt("model.prop.hinges", &[]), bt("model.ot.flip", &[]))).clicked() {
                out = Some("hinges=flip".into());
            }
            let fk = if o0.door { "model.prop.swing" } else { "model.prop.wface" };
            if ui.button(format!("{}: {}", bt(fk, &[]), bt("model.ot.flip", &[]))).clicked() {
                out = Some("face=flip".into());
            }
        });
    } else if let Some((kind, guid)) = derived.first() {
        match *kind {
            "room" => {
                if let Some(sp) = sn.derived.spaces.iter().find(|x| x.guid == *guid) {
                    if let Some(x) = row(app, ui, "mp.name", &bt("model.room.longname", &[]), sp.name.clone()) {
                        out = Some(x);
                    }
                    if let Some(x) = row(app, ui, "mp.number", &bt("model.room.number", &[]), sp.number.clone()) {
                        out = Some(x);
                    }
                }
            }
            "floor" => {
                if let Some(f) = sn.derived.floors.iter().find(|x| x.guid == *guid) {
                    ui.horizontal(|ui| {
                        for (fk, _, _) in FLOOR_KINDS {
                            if ui.selectable_label(f.kind == fk, bt(&format!("model.floor.{fk}"), &[])).clicked() {
                                out = Some(format!("kind={fk}"));
                            }
                        }
                    });
                    if let Some(x) = row(app, ui, "mp.thickness", &bt("model.prop.thickness", &[]), fmt_len(f.thickness, &units)) {
                        out = Some(x);
                    }
                    if let Some(x) = row(app, ui, "mp.offset", &bt("model.fl.offset", &[]), fmt_len(f.offset, &units)) {
                        out = Some(x);
                    }
                }
            }
            "ceiling" => {
                if let Some(c) = sn.derived.ceilings.iter().find(|x| x.guid == *guid) {
                    if let Some(x) = row(app, ui, "mp.height", &bt("model.ce.height", &[]), fmt_len(c.height, &units)) {
                        out = Some(x);
                    }
                    if let Some(x) = row(app, ui, "mp.thickness", &bt("model.prop.thickness", &[]), fmt_len(c.thickness, &units)) {
                        out = Some(x);
                    }
                }
            }
            _ => {
                if let Some(r) = sn.derived.roofs.iter().find(|x| x.guid == *guid) {
                    if let Some(x) = row(app, ui, "mp.pitch", &bt("model.rf.pitch", &[]), fmt_pitch(r.pitch, &units)) {
                        out = Some(x);
                    }
                    if let Some(x) = row(app, ui, "mp.overhang", &bt("model.rf.overhang", &[]), fmt_len(r.overhang, &units)) {
                        out = Some(x);
                    }
                    ui.horizontal_wrapped(|ui| {
                        for (e, key) in [
                            ("hip", "model.rf.hip"),
                            ("gable-short", "model.rf.gable.short"),
                            ("gable-long", "model.rf.gable.long"),
                            ("shed", "res.rf.shed"),
                        ] {
                            if ui.button(bt(key, &[])).clicked() {
                                out = Some(format!("ends={e}"));
                            }
                        }
                    });
                }
            }
        }
    }
    ui.label(RichText::new(bt("model.prop.note", &[])).small().color(t.text_dim));
    ui.separator();
    if let Some(arg) = out {
        app.run_ui("bim.edit", &Params { name: Some(arg), ..Default::default() });
    }
}

/// Keys while Draw is on (model-ui.js keydown capture): Esc, Enter, W / V / D / N / H / F, Delete.
pub fn keys(app: &mut UiApp, ctx: &egui::Context) {
    if !app.bimh.draw.on || ctx.egui_wants_keyboard_input() {
        return;
    }
    let (esc, enter, del, w, v, dk, n, h, f) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::Escape),
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
            i.key_pressed(egui::Key::W) && !i.modifiers.any(),
            i.key_pressed(egui::Key::V) && !i.modifiers.any(),
            i.key_pressed(egui::Key::D) && !i.modifiers.any(),
            i.key_pressed(egui::Key::N) && !i.modifiers.any(),
            i.key_pressed(egui::Key::H) && !i.modifiers.any(),
            i.key_pressed(egui::Key::F) && !i.modifiers.any(),
        )
    });
    let p = |s: &str| Params { name: Some(s.into()), ..Default::default() };
    if esc || (enter && app.bimh.draw.chain.is_some()) {
        app.run_ui("bim.end", &Params::default());
    }
    if w {
        app.run_ui("bim.tool", &p("wall"));
    }
    if v {
        app.run_ui("bim.tool", &p("select"));
    }
    if dk {
        app.run_ui("bim.tool", &p("door"));
    }
    if n {
        app.run_ui("bim.tool", &p("window"));
    }
    if h {
        app.run_ui("bim.flip", &Params::default());
    }
    if f {
        app.run_ui("bim.fit", &Params::default());
    }
    if del && app.picked_product().is_some() {
        let (a, b, c, _) = picked(app);
        if !a.is_empty() || !b.is_empty() || !c.is_empty() {
            app.run_ui("bim.delete", &Params::default());
        }
    }
}

/// The edits the plan made since the last frame need a scene: the selection drawn from the 3D pick.
pub fn picked_keys(app: &UiApp) -> HashSet<u64> {
    app.picked_product().map(cc_bim::draw::plan::key_u64).into_iter().collect()
}
