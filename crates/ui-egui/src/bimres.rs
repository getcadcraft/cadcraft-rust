//! BimCraft's residential tools in the plan (res-ui.js, 0.5 part 4 track RC): the Build tools roof plane, foundation,
//! countertop, stair, railing, framing, section line and electrical, their option rows in the model bar, their plan
//! symbols and previews, picking, Properties, Delete, and the Lumber / Devices / Circuits schedules. Every change is one
//! `cc_bim::draw::resops` op through the editor (one undo step), the same ops `cadcraft-cli bimdraw` runs at parity.
//!
//! Gestures are commands: the plan's clicks go through `bim.point`, the rest through `bim.res` (finish, the framing
//! buttons, linking lights, Properties changes), so a script draws exactly what a hand would.

use std::collections::HashSet;

use cc_bim::draw::m::{self, V2};
use cc_bim::draw::openings::{Arc, Symbol};
use cc_bim::draw::ops::Op;
use cc_bim::draw::plan::{SRoom, SSym, Scene};
use cc_bim::draw::rescounter::{Cabinet, Countertop, Run, counter_cabinets, counter_runs, point_in, read_countertops, run_key};
use cc_bim::draw::reselec::{Device, is_light, read_devices, symbol_plan, symbol_scale, wall_mounted, wall_spot};
use cc_bim::draw::resfound::{FOUNDATION_KINDS, Foundation, PKEYS, Params, Part, foundation_geometry, foundation_params, loads_on};
use cc_bim::draw::resframe::{Framing, Shape, WallCtx, frame_wall, read_framing};
use cc_bim::draw::resops::{
    DEVICE_KINDS, Railing, ResOp, RoofPlane, Section, is_switch, lumber_list, metric_name, plane_frame, read_railings, read_roof_planes,
    read_sections, section_dir, stair_defaults,
};
use cc_bim::draw::resstair::{Layout, Piece, STAIR_KINDS, Stair, railing_parts, read_stairs, stair_height, stair_layout};
use cc_bim::draw::rooms::{label_point, loop_at};
use cc_bim::draw::schedule::Schedule;
use cc_bim::draw::tools::{Snap, SnapOpts, snap_point};
use cc_bim::draw::units::{fmt_area, fmt_len, fmt_pitch, parse_len, parse_pitch};
use cc_bim::draw::walls::wall_type;
use cc_bim::draw::writer::W;
use egui::RichText;
use serde_json::{Value as J, json};

use crate::bimplan::{Draw, Overlay, SNAP_PX, commit, round_step, snap_walls, walls_here};
use crate::bimtools::bt;
use crate::{Params as UParams, UiApp};

const D2R: f64 = std::f64::consts::PI / 180.0;
const IN: f64 = 25.4;
/// The plan's cut height (model-ui.js): steps above it are dashed.
const CUT: f64 = 1200.0;

/// The Build menu's tools (ui.inc `[data-res-tools]`): (tool, label key, icon). Terrain, pad and paving come with the
/// ground (M7.21).
pub const RES_TOOLS: [(&str, &str, &str); 8] = [
    ("roofplane", "res.tool.roofplane", "triangle"),
    ("foundation", "res.tool.foundation", "bounding-box"),
    ("countertop", "res.tool.countertop", "square-half"),
    ("stair", "res.tool.stair", "reception-4"),
    ("railing", "res.tool.railing", "distribute-horizontal"),
    ("framing", "res.tool.framing", "grid-3x3"),
    ("section", "res.tool.section", "slash-square"),
    ("electrical", "res.tool.electrical", "plug"),
];
/// Every tool this module answers for (the Build tools and linking lights to a switch).
pub fn is_res_tool(name: &str) -> bool {
    name == "ellink" || RES_TOOLS.iter().any(|t| t.0 == name)
}

/// The residential commands (id, label, shortcut, params doc).
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[(
    "bim.res",
    "Residential tool action",
    "Enter / Esc",
    "{name: \"finish\" | \"dbl\" | \"escape\" | \"frameall\" | \"frshow\" | \"frsee\" | \"link\" | \"linkdone\" | \"fit\" | \"keep\" | \"edit:<field>=<value>\"} → finishes a roof plane or railing, frames every wall, shows / hides the framing, sees through framed hosts, links lights to the picked switch, fits a countertop again, keeps a framing piece, changes the picked roof plane / foundation / countertop / stair / railing / framing / section line / device (fields: pitch, base, thickness, kind, slabT, edgeW, edgeD, stemT, footW, footD, bottom, stemDrop, basementH, overhang, turn, width, riserMax, tread, railings, railHeight, well, height, spacing, corners, blocking, roofMode, name, look, depth, tag, circuit, load)",
)];

// ------------------------------------------------------------------------------------------------------------ state

/// A foundation and what the plan draws of it.
#[derive(Clone, Debug)]
pub struct FoundView {
    pub f: Foundation,
    pub outline: Option<Vec<V2>>,
    pub prm: Params,
    pub geo: Vec<Part>,
    pub stories: f64,
}
/// A countertop and the run of cabinets under it now.
#[derive(Clone, Debug)]
pub struct CounterView {
    pub c: Countertop,
    pub run: Option<Run>,
    pub stale: bool,
}
/// A stair, its layout, the level its top reaches.
#[derive(Clone, Debug)]
pub struct StairView {
    pub s: Stair,
    pub layout: Option<Layout>,
    pub outline: Vec<Vec<V2>>,
    pub top: Option<f64>,
}
/// A framing, its quantities and (walls) the studs the plan's cut passes through.
#[derive(Clone, Debug)]
pub struct FrameView {
    pub f: Framing,
    pub pieces: f64,
    pub bf: f64,
    pub notes: String,
    pub studs: Vec<Vec<V2>>,
}

/// The roof plane tool.
#[derive(Clone, Debug)]
pub struct PlaneT {
    pub pitch: f64,
    pub base: Option<f64>,
    pub pts: Option<Vec<V2>>,
    pub at: Option<Snap>,
}
/// The stair tool.
#[derive(Clone, Debug)]
pub struct StairT {
    pub kind: String,
    pub turn: String,
    pub width: f64,
    pub railings: String,
    pub start: Option<V2>,
    pub at: Option<Snap>,
    pub spec: Option<(Stair, f64)>,
}
/// The railing tool.
#[derive(Clone, Debug)]
pub struct RailT {
    pub kind: String,
    pub height: f64,
    pub pts: Option<Vec<V2>>,
    pub at: Option<Snap>,
}
/// What the framing tool points at: (kind, GlobalId, outline, label).
type FrameHover = (&'static str, String, Option<Vec<V2>>, String);
/// The framing tool.
#[derive(Clone, Debug)]
pub struct FrameT {
    pub spacing: f64,
    pub units: String,
    pub corners: String,
    pub roof_mode: String,
    pub blocking: bool,
    pub show: bool,
    pub see: bool,
    pub hover: Option<FrameHover>,
}
/// The electrical tool.
#[derive(Clone, Debug)]
pub struct ElT {
    pub kind: String,
    pub circuit: String,
    pub height: Option<f64>,
    pub at: Option<V2>,
    pub link: Option<String>,
}
/// Where a device goes: (on a wall: its GlobalId, side, offset), the point, the angle.
type ElSpot = (Option<(String, f64, f64)>, V2, f64);
/// A row of the circuit schedule: circuit, devices, watts, volts, kinds with their counts.
type CircuitRow = (String, usize, f64, f64, Vec<(String, usize)>);
/// What the foundation tool points at: outline, sizes, parts.
type FoundHover = (Vec<V2>, Params, Vec<Part>);

/// The residential tools' state (res-ui.js `R`).
#[derive(Clone, Debug)]
pub struct Res {
    pub planes: Vec<RoofPlane>,
    pub founds: Vec<FoundView>,
    pub counters: Vec<CounterView>,
    pub cabinets: Vec<(f64, Cabinet)>,
    pub stairs: Vec<StairView>,
    pub rails: Vec<Railing>,
    pub frames: Vec<FrameView>,
    pub secs: Vec<Section>,
    pub devs: Vec<Device>,
    pub plane: PlaneT,
    pub found_kind: String,
    pub found_hover: Option<FoundHover>,
    pub counter_over: f64,
    pub counter_thick: f64,
    pub counter_hover: Option<(Run, Option<String>)>,
    pub stair: StairT,
    pub rail: RailT,
    pub frame: FrameT,
    pub el: ElT,
    pub sec_a: Option<V2>,
    pub sec_at: Option<Snap>,
    /// 3D instances hidden by the framing's Show / See through (product ids)
    pub hidden: HashSet<u64>,
}

impl Default for Res {
    fn default() -> Self {
        let sd = stair_defaults();
        Res {
            planes: Vec::new(),
            founds: Vec::new(),
            counters: Vec::new(),
            cabinets: Vec::new(),
            stairs: Vec::new(),
            rails: Vec::new(),
            frames: Vec::new(),
            secs: Vec::new(),
            devs: Vec::new(),
            plane: PlaneT { pitch: m::atan(4.0 / 12.0) / D2R, base: None, pts: None, at: None },
            found_kind: "slab".into(),
            found_hover: None,
            counter_over: IN,
            counter_thick: 1.5 * IN,
            counter_hover: None,
            stair: StairT { kind: sd.kind, turn: sd.turn, width: sd.width, railings: sd.railings, start: None, at: None, spec: None },
            rail: RailT { kind: "guard".into(), height: 36.0 * IN, pts: None, at: None },
            frame: FrameT {
                spacing: 406.4,
                units: "ftin".into(),
                corners: "three-stud".into(),
                roof_mode: "stick".into(),
                blocking: true,
                show: true,
                see: false,
                hover: None,
            },
            el: ElT { kind: "socket.double".into(), circuit: String::new(), height: None, at: None, link: None },
            sec_a: None,
            sec_at: None,
            hidden: HashSet::new(),
        }
    }
}

fn r(app: &mut UiApp) -> &mut Res {
    &mut app.bimh.draw.res
}
fn units(dr: &Draw) -> &str {
    &dr.units
}
fn level_guid(dr: &Draw) -> String {
    dr.data.as_ref().and_then(|sn| sn.levels.iter().find(|l| Some(l.id) == dr.level)).and_then(|l| l.guid.clone()).unwrap_or_default()
}
fn snap(dr: &Draw, p: V2, shift: bool, pts: &[V2]) -> Snap {
    let from = pts.last().copied();
    snap_point(
        p,
        &snap_walls(dr),
        pts,
        SnapOpts {
            tol: dr.view.px_to_mm(SNAP_PX),
            from,
            ortho: shift,
            step: 15.0,
            round: if from.is_some() { round_step(dr) } else { 0.0 },
        },
    )
}
fn pp(poly: &[V2]) -> Vec<[f64; 4]> {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (m::at(poly, i), m::at(poly, m::nx(i, n)));
            [a[0], a[1], b[0], b[1]]
        })
        .collect()
}
fn run_res(app: &mut UiApp, name: &str, args: Vec<J>, label: &str, verb: &str) -> bool {
    commit(app, Op::Res(ResOp::new(name, args)), label, verb, None)
}
fn placed() -> String {
    bt("model.verb.placed", &[])
}
fn changed() -> String {
    bt("model.verb.changed", &[])
}
fn rail_label(kind: &str) -> String {
    bt(if kind == "handrail" { "res.rail.handrail" } else { "res.rail.guard" }, &[])
}
fn spacing_opts(units: &str) -> [(f64, &'static str); 2] {
    // i18n-ignore: lengths
    if units == "mm" { [(400.0, "400 mm"), (600.0, "600 mm")] } else { [(406.4, "16\" o.c."), (609.6, "24\" o.c.")] }
}
fn nearest_spacing(units: &str, v: f64) -> (f64, &'static str) {
    let o = spacing_opts(units);
    if (o[1].0 - v).abs() < (o[0].0 - v).abs() { o[1] } else { o[0] }
}

// ------------------------------------------------------------------------------------------------------------ reading

/// res-ui.js `refresh()`: everything the tools drew, read back from the model.
pub fn refresh(app: &mut UiApp) {
    // bindRows: the spacing list follows the units (16" / 24", or 400 / 600 mm), the first one picked
    if app.bimh.draw.res.frame.units != app.bimh.draw.units {
        let u = app.bimh.draw.units.clone();
        let f = &mut app.bimh.draw.res.frame;
        f.spacing = spacing_opts(&u).first().map_or(406.4, |o| o.0);
        f.units = u;
    }
    let Some(sn) = app.bimh.draw.data.clone() else { return };
    let Some(doc) = app.bimh.ed.as_mut() else { return };
    let Ok(mut w) = W::new(&mut doc.ed.api) else { return };
    let res = &mut app.bimh.draw.res;
    res.planes = read_roof_planes(&mut w).unwrap_or_default();
    // foundations: their outline, loads, sizes and parts now
    let mut outl: Vec<(f64, cc_bim::draw::resfound::Outlines)> = Vec::new();
    res.founds = Vec::new();
    for f in read_foundations_or_none(&mut w) {
        let Some(storey) = f.storey else { continue };
        if !outl.iter().any(|o| o.0 == storey)
            && let Ok(o) = cc_bim::draw::resfound::storey_outlines(&mut w, Some(storey))
        {
            outl.push((storey, o));
        }
        let Some((_, ol)) = outl.iter().find(|o| o.0 == storey) else { continue };
        let outline = loop_at(&ol.outlines, f.seed).map(|o| o.poly.clone());
        let (stories, heavy) = loads_on(&mut w, storey, Some(&ol.walls)).unwrap_or((1.0, false));
        let prm = foundation_params(&f.kind, &f.stored, stories, heavy);
        let geo = outline.as_ref().map(|o| foundation_geometry(o, &prm, None)).unwrap_or_default();
        res.founds.push(FoundView { f, outline, prm, geo, stories });
    }
    // countertops and the cabinets they stand on, per level
    res.cabinets = Vec::new();
    for lv in &sn.levels {
        for cb in counter_cabinets(&mut w, lv.id).unwrap_or_default() {
            res.cabinets.push((lv.id, cb));
        }
    }
    res.counters = read_countertops(&mut w)
        .unwrap_or_default()
        .into_iter()
        .map(|c| {
            let cabs: Vec<Cabinet> = res.cabinets.iter().filter(|(s, _)| Some(*s) == c.storey).map(|(_, cb)| cb.clone()).collect();
            let run = counter_runs(&cabs, c.overhang).into_iter().find(|r| point_in(c.seed, &r.poly));
            let stale = run.as_ref().is_some_and(|r| run_key(r, c.thickness) != c.key);
            CounterView { c, run, stale }
        })
        .collect();
    res.stairs = read_stairs(&mut w)
        .unwrap_or_default()
        .into_iter()
        .map(|s| {
            let (h, _) = stair_height(&s, &sn.levels);
            let layout = stair_layout(&s, h).ok();
            let outline = layout.as_ref().map(Layout::outline).unwrap_or_default();
            let i = sn.levels.iter().position(|l| Some(l.id) == s.storey);
            let top = if s.top_level.is_empty() {
                i.and_then(|i| sn.levels.get(i.saturating_add(1))).map(|l| l.id)
            } else {
                sn.levels.iter().find(|l| l.guid.as_deref() == Some(s.top_level.as_str())).map(|l| l.id)
            };
            StairView { s, layout, outline, top }
        })
        .collect();
    res.rails = read_railings(&mut w).unwrap_or_default();
    let mut frames = Vec::new();
    let ops = cc_bim::draw::openings::read_openings(&mut w, None, &sn.walls).unwrap_or_default();
    for f in read_framing(&mut w).unwrap_or_default() {
        let qty = w.read_pset(f.id, "CadCraft_FramingQuantities").ok().flatten().unwrap_or_default();
        let notes = w
            .read_pset(f.id, cc_bim::draw::resframe::FRAME_PSET)
            .ok()
            .flatten()
            .and_then(|p| p.get("Notes").map(cc_bim::wapi::P::string))
            .unwrap_or_else(|| "ok".into());
        let num = |k: &str| qty.get(k).map_or(f64::NAN, cc_bim::wapi::P::number);
        let mut studs = Vec::new();
        if f.kind == "wall"
            && let Some(wall) = sn.walls.iter().find(|x| x.guid == f.host)
        {
            let ctx = WallCtx {
                walls: sn.walls.iter().filter(|x| x.storey == wall.storey).collect(),
                openings: ops.iter().filter(|o| o.wall_guid == wall.guid).collect(),
                spacing: f.spacing,
                corners: &f.corners,
                blocking: f.blocking,
                bearing: wall_type(&wall.type_key).is_none_or(|t| t.exterior),
            };
            let fr = frame_wall(wall, &ctx);
            let l = if wall.len > 0.0 { wall.len } else { 1.0 };
            let u = [(wall.b[0] - wall.a[0]) / l, (wall.b[1] - wall.a[1]) / l];
            let n = [-u[1], u[0]];
            let to_plan = |x: f64, y: f64| -> V2 { [wall.a[0] + u[0] * x + n[0] * y, wall.a[1] + u[1] * x + n[1] * y] };
            for pc in &fr.pieces {
                if let Shape::Box { x0, x1, y0, y1, z0, z1 } = pc.shape
                    && ["stud", "king", "jack"].iter().any(|r| pc.role.starts_with(r))
                    && z0 <= CUT
                    && z1 >= CUT
                {
                    studs.push(vec![to_plan(x0, y0), to_plan(x1, y0), to_plan(x1, y1), to_plan(x0, y1)]);
                }
            }
        }
        let pieces = if num("Pieces").is_finite() { num("Pieces") } else { f.parts.len() as f64 };
        let bf = if num("BoardFeet").is_finite() { num("BoardFeet") } else { 0.0 };
        frames.push(FrameView { f, pieces, bf, notes, studs });
    }
    res.frames = frames;
    res.secs = read_sections(&mut w).unwrap_or_default();
    res.devs = read_devices(&mut w).unwrap_or_default();
    frame_vis(app);
}

fn read_foundations_or_none(w: &mut W<'_>) -> Vec<Foundation> {
    cc_bim::draw::resfound::read_foundations(w).unwrap_or_default()
}

/// `ownKeys(set)`: what the tools draw themselves (the plan's cut leaves them out).
pub fn own_keys(res: &Res, set: &mut HashSet<u64>) {
    let k = cc_bim::draw::plan::key_u64;
    for d in &res.devs {
        set.insert(k(d.id));
    }
    for p in &res.planes {
        set.extend(p.parts.iter().map(|x| k(*x)));
    }
    for f in &res.founds {
        set.extend(f.f.parts.iter().map(|x| k(*x)));
    }
    for s in &res.stairs {
        set.extend(s.s.parts.iter().map(|x| k(*x)));
    }
    for r in &res.rails {
        set.insert(k(r.id));
    }
    for f in &res.frames {
        set.extend(f.f.parts.iter().map(|x| k(*x)));
    }
}

/// `applyFrameVis()`: framing shown or hidden, framed walls / floors / roofs seen through, in the 3D view.
pub fn frame_vis(app: &mut UiApp) {
    let k = cc_bim::draw::plan::key_u64;
    let mut hide = HashSet::new();
    let (res, data) = (&app.bimh.draw.res, &app.bimh.draw.data);
    if !res.frame.show {
        for f in &res.frames {
            hide.extend(f.f.parts.iter().map(|x| k(*x)));
        }
    }
    if res.frame.see
        && let Some(sn) = data
    {
        for f in &res.frames {
            if let Some(w) = sn.walls.iter().find(|x| x.guid == f.f.host) {
                hide.insert(k(w.id));
            } else if let Some(fl) = sn.derived.floors.iter().find(|x| x.guid == f.f.host) {
                hide.insert(k(fl.id));
            } else if let Some(rf) = sn.derived.roofs.iter().find(|x| x.guid == f.f.host) {
                hide.extend(rf.parts.iter().map(|x| k(*x)));
            }
        }
    }
    if hide != app.bimh.draw.res.hidden {
        app.bimh.draw.res.hidden = hide;
        app.apply_hidden3();
    }
}

// ------------------------------------------------------------------------------------------------------------ the plan

fn sym(lines: Vec<[f64; 4]>, thin: Vec<[f64; 4]>, dashed: Vec<[f64; 4]>, poly: Vec<V2>) -> Symbol {
    Symbol { lines, thin, dashed, poly, ..Symbol::default() }
}
fn ssym(key: f64, s: Symbol, roof: bool) -> SSym {
    SSym { key, guid: None, sym: s, roof }
}

/// `stairSymbols(L, onTop)`: treads (dashed above the cut), the outline, the walk line with its arrow.
fn stair_symbol(lay: &Layout, outline: &[Vec<V2>], on_top: bool) -> Symbol {
    let (mut thin, mut dashed, mut lines) = (Vec::new(), Vec::new(), Vec::new());
    for pc in &lay.pieces {
        let treads: Vec<&cc_bim::draw::resstair::Tread> = match pc {
            Piece::Landing(t) => vec![t],
            Piece::Flight(ts) | Piece::Winders(ts) => ts.iter().collect(),
        };
        for t in treads {
            if on_top || t.z <= CUT { thin.extend(pp(&t.poly)) } else { dashed.extend(pp(&t.poly)) }
        }
    }
    for o in outline {
        lines.extend(pp(o));
    }
    let mut wk = lay.walk.clone();
    if on_top {
        wk.reverse();
    }
    for q in wk.windows(2) {
        if let [a, b] = q {
            thin.push([a[0], a[1], b[0], b[1]]);
        }
    }
    if let Some(&b) = wk.last() {
        let a = wk.len().checked_sub(2).and_then(|i| wk.get(i)).or(wk.first()).copied().unwrap_or(b);
        let dl0 = m::hypot(b[0] - a[0], b[1] - a[1]);
        let dl = if dl0 > 0.0 { dl0 } else { 1.0 };
        let (ux, uy, hs) = ((b[0] - a[0]) / dl, (b[1] - a[1]) / dl, 200.0);
        thin.push([b[0], b[1], b[0] - ux * hs - uy * hs / 2.0, b[1] - uy * hs + ux * hs / 2.0]);
        thin.push([b[0], b[1], b[0] - ux * hs + uy * hs / 2.0, b[1] - uy * hs - ux * hs / 2.0]);
    }
    sym(lines, thin, dashed, outline.first().cloned().unwrap_or_default())
}

/// `bandOf(P, h)`: a thin polygon round a path (picking a railing or a section line).
fn band_of(p: &[V2], h: f64) -> Vec<V2> {
    let n = p.len();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for i in 0..n {
        let a = m::at(p, i.saturating_sub(1));
        let b = m::at(p, m::min(n.saturating_sub(1) as f64, (i + 1) as f64) as usize);
        let d0 = m::hypot(b[0] - a[0], b[1] - a[1]);
        let d = if d0 > 0.0 { d0 } else { 1.0 };
        let nn = [-(b[1] - a[1]) / d, (b[0] - a[0]) / d];
        let q = m::at(p, i);
        left.push([q[0] + nn[0] * h, q[1] + nn[1] * h]);
        right.push([q[0] - nn[0] * h, q[1] - nn[1] * h]);
    }
    right.reverse();
    left.extend(right);
    left
}

/// The tools' elements on the level shown (`scene(scene)`): roof planes, foundations, countertops, stairs, railings,
/// framing studs, section lines, devices with their tags and switch legs.
pub fn scene(dr: &Draw, scene: &mut Scene) {
    let Some(level) = dr.level else { return };
    let res = &dr.res;
    let levels = dr.data.as_ref().map(|s| s.levels.clone()).unwrap_or_default();
    // roof planes, on the level they sit on; first: a plane over a roof picks before it
    for rp in &res.planes {
        if rp.storey != Some(level) || rp.points.len() < 3 || !levels.iter().any(|l| l.guid.as_deref() == Some(rp.level.as_str())) {
            continue;
        }
        let p = &rp.points;
        let (p0, p1) = (m::at(p, 0), m::at(p, 1));
        let eave = [p0[0], p0[1], p1[0], p1[1]];
        let dashed: Vec<[f64; 4]> = pp(p).into_iter().skip(1).collect();
        let (_, fu, fnn) = plane_frame(p);
        let mid = [(p0[0] + p1[0]) / 2.0, (p0[1] + p1[1]) / 2.0];
        let len = m::min(900.0, m::hypot(p1[0] - p0[0], p1[1] - p0[1]) / 3.0);
        let tip = [mid[0] + fnn[0] * len, mid[1] + fnn[1] * len];
        let hw = len / 5.0;
        let thin = vec![
            eave,
            [mid[0], mid[1], tip[0], tip[1]],
            [tip[0], tip[1], tip[0] - fnn[0] * hw + fu[0] * hw / 2.0, tip[1] - fnn[1] * hw + fu[1] * hw / 2.0],
            [tip[0], tip[1], tip[0] - fnn[0] * hw - fu[0] * hw / 2.0, tip[1] - fnn[1] * hw - fu[1] * hw / 2.0],
        ];
        scene.symbols.insert(0, ssym(rp.id, sym(Vec::new(), thin, dashed, p.clone()), true));
    }
    for f in &res.founds {
        if f.f.storey != Some(level) || f.geo.is_empty() {
            continue;
        }
        let (mut dashed, mut thin) = (Vec::new(), Vec::new());
        for g in &f.geo {
            let both = |v: &mut Vec<[f64; 4]>| {
                v.extend(pp(&g.outer));
                if let Some(i) = &g.inner {
                    v.extend(pp(i));
                }
            };
            match g.role {
                "footing" | "edge" => both(&mut dashed),
                "stem" => both(&mut thin),
                _ => {}
            }
        }
        let outer = f.geo.iter().find(|g| g.role == "footing" || g.role == "edge").map(|g| g.outer.clone()).or_else(|| f.outline.clone());
        scene.symbols.push(ssym(f.f.id, sym(Vec::new(), thin, dashed, outer.unwrap_or_default()), true));
    }
    for c in &res.counters {
        let Some(run) = c.run.as_ref().filter(|_| c.c.storey == Some(level)) else { continue };
        // out of date (a cabinet placed or removed since): dashed, until it is fitted again
        let (thin, dashed) = if c.stale { (Vec::new(), pp(&run.poly)) } else { (pp(&run.poly), Vec::new()) };
        scene.symbols.push(ssym(c.c.id, sym(Vec::new(), thin, dashed, run.poly.clone()), false));
    }
    for st in &res.stairs {
        let Some(lay) = &st.layout else { continue };
        if st.s.storey == Some(level) {
            scene.symbols.push(ssym(st.s.id, stair_symbol(lay, &st.outline, false), false));
        } else if st.top == Some(level) {
            scene.symbols.push(ssym(st.s.id, stair_symbol(lay, &st.outline, true), false));
        }
    }
    for rl in &res.rails {
        if rl.storey != Some(level) || rl.points.len() < 2 {
            continue;
        }
        let lines: Vec<[f64; 4]> =
            rl.points.windows(2).filter_map(|q| if let [a, b] = q { Some([a[0], a[1], b[0], b[1]]) } else { None }).collect();
        scene.symbols.push(ssym(rl.id, sym(lines, Vec::new(), Vec::new(), band_of(&rl.points, 60.0)), false));
    }
    if res.frame.show {
        for f in &res.frames {
            if f.f.kind != "wall" || f.f.storey != Some(level) || f.studs.is_empty() {
                continue;
            }
            let thin: Vec<[f64; 4]> = f.studs.iter().flat_map(|q| pp(q)).collect();
            scene.symbols.push(ssym(f64::NAN, sym(Vec::new(), thin, Vec::new(), Vec::new()), false));
        }
    }
    for sl in &res.secs {
        if sl.storey != Some(level) {
            continue;
        }
        let (_, dir, u) = section_dir(sl);
        let ext = 600.0;
        let a = [sl.a[0] - u[0] * ext, sl.a[1] - u[1] * ext];
        let b = [sl.b[0] + u[0] * ext, sl.b[1] + u[1] * ext];
        let mut lines = vec![[a[0], a[1], b[0], b[1]]];
        for e in [a, b] {
            let tip = [e[0] + dir[0] * 500.0, e[1] + dir[1] * 500.0];
            lines.push([e[0], e[1], tip[0], tip[1]]);
            lines.push([tip[0], tip[1], tip[0] - dir[0] * 160.0 + u[0] * 120.0, tip[1] - dir[1] * 160.0 + u[1] * 120.0]);
            lines.push([tip[0], tip[1], tip[0] - dir[0] * 160.0 - u[0] * 120.0, tip[1] - dir[1] * 160.0 - u[1] * 120.0]);
        }
        scene.symbols.push(ssym(sl.id, sym(lines, Vec::new(), Vec::new(), band_of(&[a, b], 150.0)), false));
        for e in [a, b] {
            let c = [e[0] - dir[0] * 350.0, e[1] - dir[1] * 350.0];
            let poly = vec![
                [c[0] - 250.0, c[1] - 250.0],
                [c[0] + 250.0, c[1] - 250.0],
                [c[0] + 250.0, c[1] + 250.0],
                [c[0] - 250.0, c[1] + 250.0],
            ];
            scene.rooms.push(SRoom { key: sl.id, poly, at: c, label: vec![sl.name.clone()] });
        }
    }
    let sc = symbol_scale(units(dr));
    let here: Vec<&Device> = res.devs.iter().filter(|d| d.storey == Some(level)).collect();
    for d in &here {
        let Some(sp) = symbol_plan(&d.kind, d.at, d.angle, sc) else { continue };
        let arcs = sp.arcs.iter().map(|a| Arc { c: a.c, r: a.r, a0: a.a0, a1: a.a1 }).collect();
        scene.symbols.push(ssym(d.id, Symbol { lines: sp.lines, arcs, thin: sp.thin, poly: sp.poly, ..Symbol::default() }, false));
        // the tag beside it, on the room side
        let f = [m::cos(d.angle), m::sin(d.angle)];
        let at = if wall_mounted(&d.kind) {
            [d.at[0] + f[0] * 7.0 * sc, d.at[1] + f[1] * 7.0 * sc]
        } else {
            [d.at[0] + 4.5 * sc, d.at[1] + 4.5 * sc]
        };
        scene.rooms.push(SRoom { key: d.id, poly: Vec::new(), at, label: vec![d.tag.clone()] });
        // switch legs: a curved dashed line to each light it switches
        if is_switch(&d.kind) {
            for g in &d.controls {
                let Some(lt) = here.iter().find(|x| x.guid == *g) else { continue };
                let (a, b) = (d.at, lt.at);
                let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                let c = [mid[0] - (b[1] - a[1]) * 0.2, mid[1] + (b[0] - a[0]) * 0.2];
                let pts: Vec<V2> = (0..=16)
                    .map(|i| {
                        let t = f64::from(i) / 16.0;
                        [
                            (1.0 - t) * (1.0 - t) * a[0] + 2.0 * t * (1.0 - t) * c[0] + t * t * b[0],
                            (1.0 - t) * (1.0 - t) * a[1] + 2.0 * t * (1.0 - t) * c[1] + t * t * b[1],
                        ]
                    })
                    .collect();
                let dashed = pts.windows(2).filter_map(|q| if let [p, n] = q { Some([p[0], p[1], n[0], n[1]]) } else { None }).collect();
                scene.symbols.push(ssym(f64::NAN, sym(Vec::new(), Vec::new(), dashed, Vec::new()), false));
            }
        }
    }
}

/// The tools' previews (`overlay(o)`).
pub fn overlay(dr: &Draw, o: &mut Overlay) {
    let res = &dr.res;
    let u = units(dr);
    let close_px = dr.view.px_to_mm(10.0);
    match dr.tool.as_str() {
        "roofplane" => {
            let cur = &res.plane.at;
            if let Some(c) = cur {
                o.snap = Some((c.p, c.kind));
            }
            let pts = res.plane.pts.clone().unwrap_or_default();
            if pts.is_empty() {
                if let Some(c) = cur {
                    o.hud = Some((c.p, bt("res.rp.hud.start", &[])));
                }
                return;
            }
            let mut all = pts.clone();
            if let Some(c) = cur {
                all.push(c.p);
            }
            o.points.extend(pts.iter().copied());
            let (mut lines, mut dashed) = (Vec::new(), Vec::new());
            for (i, q) in all.windows(2).enumerate() {
                if let [a, b] = q {
                    if i == 0 { lines.push([a[0], a[1], b[0], b[1]]) } else { dashed.push([a[0], a[1], b[0], b[1]]) }
                }
            }
            if all.len() >= 3
                && let (Some(l), Some(f)) = (all.last(), all.first())
            {
                dashed.push([l[0], l[1], f[0], f[1]]);
            }
            o.symbols.push(Symbol { lines, dashed, ..Symbol::default() });
            if let (Some(c), Some(&a)) = (cur, pts.last()) {
                let b = c.p;
                o.dims.push(cc_bim::draw::plan::Dim {
                    a,
                    b,
                    text: fmt_len(m::hypot(b[0] - a[0], b[1] - a[1]), u),
                    off_px: None,
                    off_mm: None,
                    off: Some(dr.view.px_to_mm(16.0)),
                    key: None,
                });
                let first = m::at(&pts, 0);
                let mut text = if pts.len() == 1 { bt("res.rp.hud.eave", &[]) } else { fmt_pitch(res.plane.pitch, u) };
                if pts.len() >= 3 && m::hypot(b[0] - first[0], b[1] - first[1]) <= close_px {
                    text = format!("{text}\n{}", bt("model.hud.close", &[]));
                }
                o.hud = Some((b, text));
            }
        }
        "foundation" => {
            if let Some((outline, prm, geo)) = &res.found_hover {
                o.walls.push(outline.clone());
                let mut dashed = Vec::new();
                for g in geo.iter().filter(|g| g.role != "slab") {
                    dashed.extend(pp(&g.outer));
                    if let Some(i) = &g.inner {
                        dashed.extend(pp(i));
                    }
                }
                o.symbols.push(Symbol { dashed, ..Symbol::default() });
                let kind = bt(&format!("res.fd.kind.{}", prm.kind), &[]);
                o.hud = Some((
                    label_point(outline),
                    bt("res.fd.hud", &[("kind", &kind), ("w", &fmt_len(prm.foot_w(), u)), ("d", &fmt_len(prm.foot_d(), u))]),
                ));
            }
        }
        "countertop" => {
            if let Some((run, have)) = &res.counter_hover {
                o.walls.push(run.poly.clone());
                let n = run.cabinets.len().to_string();
                let key = if have.is_some() { "res.ct.hud.fit" } else { "res.ct.hud" };
                o.hud = Some((label_point(&run.poly), bt(key, &[("n", &n), ("area", &fmt_area(run.area, u))])));
            }
        }
        "stair" => {
            let st = &res.stair;
            if let Some(a) = &st.at {
                o.snap = Some((a.p, a.kind));
            }
            let Some(start) = st.start else {
                if let Some(a) = &st.at {
                    o.hud = Some((a.p, bt("res.st.hud.start", &[])));
                }
                return;
            };
            o.points.push(start);
            let Some((spec, h)) = &st.spec else { return };
            let at = st.at.as_ref().map_or(start, |a| a.p);
            match stair_layout(spec, *h) {
                Err(e) => o.hud = Some((at, bt(e, &[]))),
                Ok(lay) => {
                    let s = stair_symbol(&lay, &lay.outline(), false);
                    let mut thin = s.thin;
                    thin.extend(s.dashed);
                    let mut dashed = Vec::new();
                    for side in &lay.sides {
                        for q in side.windows(2) {
                            if let [a, b] = q {
                                dashed.push([a[0], a[1], b[0], b[1]]);
                            }
                        }
                    }
                    o.symbols.push(Symbol { lines: s.lines, thin, dashed, ..Symbol::default() });
                    o.hud = Some((
                        at,
                        bt(
                            "res.st.hud",
                            &[
                                ("n", &cc_bim::js::num_str(lay.n)),
                                ("r", &fmt_len(lay.riser, u)),
                                ("m", &cc_bim::js::num_str(lay.treads)),
                                ("d", &fmt_len(lay.tread, u)),
                            ],
                        ),
                    ));
                }
            }
        }
        "railing" => {
            let rl = &res.rail;
            if let Some(a) = &rl.at {
                o.snap = Some((a.p, a.kind));
            }
            let pts = rl.pts.clone().unwrap_or_default();
            o.points.extend(pts.iter().copied());
            let mut all = pts.clone();
            if let Some(a) = &rl.at {
                all.push(a.p);
            }
            let lines: Vec<[f64; 4]> =
                all.windows(2).filter_map(|q| if let [a, b] = q { Some([a[0], a[1], b[0], b[1]]) } else { None }).collect();
            o.symbols.push(Symbol { lines, ..Symbol::default() });
            if let Some(a) = &rl.at {
                let text = if pts.is_empty() {
                    bt("res.rl.hud.start", &[])
                } else {
                    fmt_len(all.windows(2).map(|q| if let [a, b] = q { m::hypot(b[0] - a[0], b[1] - a[1]) } else { 0.0 }).sum(), u)
                };
                o.hud = Some((a.p, text));
            }
        }
        "framing" => {
            if let Some((_, guid, poly, label)) = &res.frame.hover {
                if let Some(p) = poly {
                    o.walls.push(p.clone());
                }
                let have = res.frames.iter().any(|f| f.f.host == *guid);
                let at = poly.as_ref().map_or([0.0, 0.0], |p| label_point(p));
                o.hud = Some((at, bt(if have { "res.fr.hud.again" } else { "res.fr.hud" }, &[("what", label)])));
            }
        }
        "section" => {
            if let Some(a) = &res.sec_at {
                o.snap = Some((a.p, a.kind));
            }
            let Some(a) = res.sec_a else {
                if let Some(at) = &res.sec_at {
                    o.hud = Some((at.p, bt("res.sec.hud.a", &[])));
                }
                return;
            };
            o.points.push(a);
            if let Some(at) = &res.sec_at {
                let b = at.p;
                let d = [b[0] - a[0], b[1] - a[1]];
                let l0 = m::hypot(d[0], d[1]);
                let l = if l0 > 0.0 { l0 } else { 1.0 };
                let n = [-d[1] / l, d[0] / l];
                let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                o.symbols.push(Symbol {
                    lines: vec![[a[0], a[1], b[0], b[1]], [mid[0], mid[1], mid[0] + n[0] * 600.0, mid[1] + n[1] * 600.0]],
                    ..Symbol::default()
                });
                o.hud = Some((b, bt("res.sec.hud.b", &[])));
            }
        }
        "electrical" => {
            let Some(at) = res.el.at else { return };
            match el_spot(dr, at) {
                None => o.hud = Some((at, bt("res.el.hud.wall", &[]))),
                Some((_, sp, ang)) => {
                    if let Some(s) = symbol_plan(&res.el.kind, sp, ang, symbol_scale(u)) {
                        let arcs = s.arcs.iter().map(|a| Arc { c: a.c, r: a.r, a0: a.a0, a1: a.a1 }).collect();
                        o.symbols.push(Symbol { lines: s.lines, arcs, thin: s.thin, ..Symbol::default() });
                    }
                    o.hud = Some((sp, bt(&format!("res.el.kind.{}", res.el.kind), &[])));
                }
            }
        }
        "ellink" => {
            if let (Some(sw), Some(at)) = (res.devs.iter().find(|d| Some(&d.guid) == res.el.link.as_ref()), res.el.at) {
                o.hud = Some((at, bt("res.el.hud.link", &[("tag", &sw.tag)])));
            }
        }
        _ => {}
    }
}

/// `elSpot(p)`: a wall device on the nearest wall face (None when no wall is near), a ceiling one where the pointer is.
/// → (the wall's spot: wall GlobalId, side, offset), the point, the angle.
fn el_spot(dr: &Draw, p: V2) -> Option<ElSpot> {
    if !wall_mounted(&dr.res.el.kind) {
        return Some((None, p, std::f64::consts::FRAC_PI_2));
    }
    let walls = walls_here(dr);
    wall_spot(&walls, p, m::max(600.0, dr.view.px_to_mm(30.0))).map(|(g, side, off, at, ang)| (Some((g, side, off)), at, ang))
}

/// `foundHover(p)`
fn found_hover(app: &mut UiApp, p: V2) -> Option<FoundHover> {
    let dr = &app.bimh.draw;
    let o = loop_at(&dr.loops.as_ref()?.loops.outlines, p)?.poly.clone();
    let level = dr.level?;
    let walls = walls_here(dr);
    let kind = dr.res.found_kind.clone();
    let doc = app.bimh.ed.as_mut()?;
    let mut w = W::new(&mut doc.ed.api).ok()?;
    let (stories, heavy) = loads_on(&mut w, level, Some(&walls)).unwrap_or((1.0, false));
    let prm = foundation_params(&kind, &[None; 9], stories, heavy);
    let geo = foundation_geometry(&o, &prm, None);
    Some((o, prm, geo))
}

/// `counterHover(p)`
fn counter_hover(dr: &Draw, p: V2) -> Option<(Run, Option<String>)> {
    let level = dr.level?;
    let res = &dr.res;
    let cabs: Vec<Cabinet> = res.cabinets.iter().filter(|(s, _)| *s == level).map(|(_, c)| c.clone()).collect();
    let run = counter_runs(&cabs, res.counter_over).into_iter().find(|r| point_in(p, &r.poly))?;
    let have = res
        .counters
        .iter()
        .find(|c| c.c.storey == Some(level) && c.run.is_some() && point_in(c.c.seed, &run.poly))
        .map(|c| c.c.guid.clone());
    Some((run, have))
}

/// `stairSpecAt(p)`: the stair from its start to the pointer, and the height it climbs.
fn stair_spec_at(dr: &Draw, p: V2, shift: bool) -> Option<(Stair, f64)> {
    let st = &dr.res.stair;
    let start = st.start?;
    let q = snap(dr, p, shift, &[start]).p;
    let (dx, dy) = (q[0] - start[0], q[1] - start[1]);
    if m::hypot(dx, dy) < 50.0 {
        return None;
    }
    let sn = dr.data.as_ref()?;
    let i = sn.levels.iter().position(|l| Some(l.id) == dr.level)?;
    let lv = sn.levels.get(i)?;
    let height = sn.levels.get(i.saturating_add(1)).map_or(lv.height, |a| a.elevation - lv.elevation);
    let s = Stair {
        kind: st.kind.clone(),
        turn: st.turn.clone(),
        width: st.width,
        railings: st.railings.clone(),
        start,
        angle: m::atan2(dy, dx) * 180.0 / std::f64::consts::PI,
        ..stair_defaults()
    };
    Some((s, height))
}

/// `frameHostAt(p)`: the wall, roof or floor of the level under the pointer.
fn frame_host_at(dr: &Draw, p: V2) -> Option<FrameHover> {
    let tol = dr.view.px_to_mm(14.0);
    let mut best: Option<(f64, FrameHover)> = None;
    for w in walls_here(dr) {
        let l = if w.len > 0.0 { w.len } else { 1.0 };
        let u = [(w.b[0] - w.a[0]) / l, (w.b[1] - w.a[1]) / l];
        let n = [-u[1], u[0]];
        let s = (p[0] - w.a[0]) * u[0] + (p[1] - w.a[1]) * u[1];
        let d = ((p[0] - w.a[0]) * n[0] + (p[1] - w.a[1]) * n[1]).abs();
        if s < -tol || s > l + tol {
            continue;
        }
        let sc = m::max(0.0, d - w.t / 2.0);
        if sc <= tol && best.as_ref().is_none_or(|b| sc < b.0) {
            let poly = dr.scene.walls.iter().find(|x| x.guid == w.guid).map(|x| x.poly.clone());
            best = Some((sc, ("wall", w.guid.clone(), poly, bt("res.fr.of.wall", &[]))));
        }
    }
    if let Some(b) = best {
        return Some(b.1);
    }
    let sn = dr.data.as_ref()?;
    let level = dr.level?;
    let loops = dr.loops.as_ref()?;
    for rf in sn.derived.roofs.iter().filter(|x| x.storey == Some(level)) {
        // the roof's outline is its walls' level's (the same level here, or the one below)
        let wl = sn.levels.iter().find(|l| l.guid.as_deref() == Some(rf.walls.as_str()));
        if wl.is_some_and(|l| l.id == level)
            && let Some(o) = loop_at(&loops.loops.outlines, rf.seed)
            && point_in(p, &o.poly)
        {
            return Some(("roof", rf.guid.clone(), Some(o.poly.clone()), bt("res.fr.of.roof", &[])));
        }
    }
    for fl in sn.derived.floors.iter().filter(|x| x.storey == Some(level)) {
        let lp = if fl.mode == "building" {
            loop_at(&loops.loops.outlines, fl.seed).map(|o| o.poly.clone())
        } else {
            loop_at(&loops.loops.rooms, fl.seed).map(|o| o.poly.clone())
        };
        if let Some(lp) = lp
            && point_in(p, &lp)
        {
            return Some(("floor", fl.guid.clone(), Some(lp), bt("res.fr.of.floor", &[])));
        }
    }
    None
}

/// The pointer over the plan with a residential tool (`move(p, e)`) → handled.
pub fn on_move(app: &mut UiApp, p: V2, shift: bool) -> bool {
    let tool = app.bimh.draw.tool.clone();
    if !is_res_tool(&tool) {
        return false;
    }
    match tool.as_str() {
        "roofplane" => {
            let pts = app.bimh.draw.res.plane.pts.clone().unwrap_or_default();
            let s = snap(&app.bimh.draw, p, shift, &pts);
            r(app).plane.at = Some(s);
        }
        "foundation" => {
            let h = found_hover(app, p);
            r(app).found_hover = h;
        }
        "countertop" => {
            let h = counter_hover(&app.bimh.draw, p);
            r(app).counter_hover = h;
        }
        "stair" => {
            let pts: Vec<V2> = app.bimh.draw.res.stair.start.into_iter().collect();
            let s = snap(&app.bimh.draw, p, shift, &pts);
            let spec = stair_spec_at(&app.bimh.draw, p, shift);
            let st = &mut r(app).stair;
            st.at = Some(s);
            st.spec = spec;
        }
        "railing" => {
            let pts = app.bimh.draw.res.rail.pts.clone().unwrap_or_default();
            let s = snap(&app.bimh.draw, p, shift, &pts);
            r(app).rail.at = Some(s);
        }
        "framing" => {
            let h = frame_host_at(&app.bimh.draw, p);
            r(app).frame.hover = h;
        }
        "section" => {
            let pts: Vec<V2> = app.bimh.draw.res.sec_a.into_iter().collect();
            let s = snap(&app.bimh.draw, p, shift, &pts);
            r(app).sec_at = Some(s);
        }
        _ => r(app).el.at = Some(p),
    }
    true
}

/// A click on the plan with a residential tool (`click(p, e)`) → handled.
pub fn on_click(app: &mut UiApp, p: V2, shift: bool) -> bool {
    let tool = app.bimh.draw.tool.clone();
    if !is_res_tool(&tool) {
        return false;
    }
    let lg = level_guid(&app.bimh.draw);
    let level = app.bimh.draw.level;
    match tool.as_str() {
        "roofplane" => {
            let pts = app.bimh.draw.res.plane.pts.clone().unwrap_or_default();
            let s = snap(&app.bimh.draw, p, shift, &pts).p;
            let close = app.bimh.draw.view.px_to_mm(10.0);
            if let Some(f) = pts.first()
                && pts.len() >= 3
                && m::hypot(s[0] - f[0], s[1] - f[1]) <= close
            {
                finish_plane(app);
                return true;
            }
            if pts.last().is_some_and(|l| m::hypot(s[0] - l[0], s[1] - l[1]) < 1.0) {
                return true;
            }
            r(app).plane.pts.get_or_insert_with(Vec::new).push(s);
        }
        "foundation" => {
            let Some((_, _, _)) = found_hover(app, p) else {
                app.error = Some(bt("model.err.nobuilding2", &[]));
                return true;
            };
            // a building that has one already: pick it (Properties changes it) instead of a second one
            let have = app
                .bimh
                .draw
                .res
                .founds
                .iter()
                .find(|f| f.f.storey == level && f.outline.as_ref().is_some_and(|o| point_in(p, o)))
                .and_then(|f| f.f.parts.first().copied());
            if let Some(k) = have {
                app.select_product(k);
                app.message = bt("res.fd.picked", &[]);
                return true;
            }
            let kind = app.bimh.draw.res.found_kind.clone();
            run_res(
                app,
                "placeFoundation",
                vec![json!({ "level": lg, "seed": [p[0], p[1]], "kind": kind })],
                &bt(&format!("res.fd.kind.{kind}"), &[]),
                &placed(),
            );
        }
        "countertop" => {
            let Some((_, have)) = counter_hover(&app.bimh.draw, p) else {
                app.error = Some(bt("res.err.nocabinet", &[]));
                return true;
            };
            if let Some(g) = have {
                run_res(app, "fitCountertop", vec![J::String(g)], &bt("res.counter", &[]), &bt("res.verb.fitted", &[]));
                return true;
            }
            let (o, t) = (app.bimh.draw.res.counter_over, app.bimh.draw.res.counter_thick);
            run_res(
                app,
                "placeCountertop",
                vec![json!({ "level": lg, "seed": [p[0], p[1]], "overhang": o, "thickness": t })],
                &bt("res.counter", &[]),
                &placed(),
            );
        }
        "stair" => {
            if app.bimh.draw.res.stair.start.is_none() {
                let s = snap(&app.bimh.draw, p, shift, &[]).p;
                r(app).stair.start = Some(s);
                return true;
            }
            let Some((spec, h)) = stair_spec_at(&app.bimh.draw, p, shift) else { return true };
            if let Err(e) = stair_layout(&spec, h) {
                app.error = Some(bt(e, &[]));
                return true;
            }
            let st = &mut r(app).stair;
            st.start = None;
            st.spec = None;
            run_res(
                app,
                "placeStair",
                vec![json!({
                    "level": lg, "start": [spec.start[0], spec.start[1]], "angle": spec.angle, "kind": spec.kind,
                    "turn": spec.turn, "width": spec.width, "railings": spec.railings,
                })],
                &bt("res.stair", &[]),
                &placed(),
            );
        }
        "railing" => {
            let pts = app.bimh.draw.res.rail.pts.clone().unwrap_or_default();
            let q = snap(&app.bimh.draw, p, shift, &pts).p;
            if pts.last().is_some_and(|l| m::hypot(q[0] - l[0], q[1] - l[1]) < 1.0) {
                return true;
            }
            r(app).rail.pts.get_or_insert_with(Vec::new).push(q);
        }
        "framing" => {
            let Some((_, guid, _, label)) = frame_host_at(&app.bimh.draw, p) else {
                app.error = Some(bt("res.err.noframe", &[]));
                return true;
            };
            let opts = frame_opts(&app.bimh.draw.res);
            let mut spec = json!({ "hosts": [guid] });
            merge(&mut spec, &opts);
            run_res(app, "frameHosts", vec![spec], &label, &bt("res.verb.framed", &[]));
        }
        "section" => {
            let pts: Vec<V2> = app.bimh.draw.res.sec_a.into_iter().collect();
            let q = snap(&app.bimh.draw, p, shift, &pts).p;
            let Some(a) = app.bimh.draw.res.sec_a else {
                r(app).sec_a = Some(q);
                return true;
            };
            r(app).sec_a = None;
            run_res(
                app,
                "placeSectionLine",
                vec![json!({ "level": lg, "a": [a[0], a[1]], "b": [q[0], q[1]] })],
                &bt("res.sec.line", &[]),
                &placed(),
            );
        }
        "electrical" => {
            let Some((wall, at, angle)) = el_spot(&app.bimh.draw, p) else {
                app.error = Some(bt("res.err.elwall", &[]));
                return true;
            };
            let el = app.bimh.draw.res.el.clone();
            let mut spec = json!({ "level": lg, "kind": el.kind, "units": app.bimh.draw.units, "circuit": el.circuit, "at": [at[0], at[1]], "angle": angle });
            if let Some(h) = el.height {
                merge(&mut spec, &json!({ "height": h }));
            }
            if let Some((g, side, off)) = wall {
                merge(&mut spec, &json!({ "wall": g, "side": side, "offset": off }));
            }
            run_res(app, "placeDevice", vec![spec], &bt(&format!("res.el.kind.{}", el.kind), &[]), &placed());
        }
        _ => {
            // ellink: the nearest light within reach links to (or unlinks from) the switch
            let reach = m::max(400.0, app.bimh.draw.view.px_to_mm(20.0));
            let res = &app.bimh.draw.res;
            let lt = res
                .devs
                .iter()
                .filter(|d| d.storey == level && is_light(&d.kind))
                .map(|d| (m::hypot(d.at[0] - p[0], d.at[1] - p[1]), d))
                .filter(|x| x.0 <= reach)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|x| x.1.guid.clone());
            let Some(lt) = lt else {
                app.error = Some(bt("res.err.ellight", &[]));
                return true;
            };
            let Some(sw) = res.devs.iter().find(|d| Some(&d.guid) == res.el.link.as_ref()).cloned() else {
                end_link(app);
                return true;
            };
            let verb = bt(if sw.controls.contains(&lt) { "res.el.verb.unlinked" } else { "res.el.verb.linked" }, &[]);
            run_res(app, "linkLight", vec![J::String(sw.guid.clone()), J::String(lt)], &sw.tag, &verb);
        }
    }
    true
}

fn merge(into: &mut J, from: &J) {
    if let (Some(a), Some(b)) = (into.as_object_mut(), from.as_object()) {
        for (k, v) in b {
            a.insert(k.clone(), v.clone());
        }
    }
}
fn frame_opts(res: &Res) -> J {
    json!({ "spacing": res.frame.spacing, "corners": res.frame.corners, "blocking": res.frame.blocking, "roofMode": res.frame.roof_mode })
}

fn finish_plane(app: &mut UiApp) {
    let pts = r(app).plane.pts.take().unwrap_or_default();
    if pts.len() < 3 {
        app.error = Some(bt("res.err.plane", &[]));
        return;
    }
    let lg = level_guid(&app.bimh.draw);
    let pl = &app.bimh.draw.res.plane;
    let mut spec = json!({ "level": lg, "points": pts.iter().map(|p| json!([p[0], p[1]])).collect::<Vec<_>>(), "pitch": pl.pitch });
    if let Some(b) = pl.base {
        merge(&mut spec, &json!({ "base": b }));
    }
    run_res(app, "placeRoofPlane", vec![spec], &bt("res.roofplane", &[]), &placed());
}
fn finish_railing(app: &mut UiApp) {
    let pts = r(app).rail.pts.take().unwrap_or_default();
    if pts.len() < 2 {
        app.error = Some(bt("res.err.rail", &[]));
        return;
    }
    let lg = level_guid(&app.bimh.draw);
    let rl = app.bimh.draw.res.rail.clone();
    let spec =
        json!({ "level": lg, "points": pts.iter().map(|p| json!([p[0], p[1]])).collect::<Vec<_>>(), "height": rl.height, "kind": rl.kind });
    run_res(app, "placeRailing", vec![spec], &rail_label(&rl.kind), &placed());
}
fn end_link(app: &mut UiApp) {
    let g = r(app).el.link.take();
    app.bimh.draw.tool = "select".into();
    if let Some(id) = g.and_then(|g| app.bimh.draw.res.devs.iter().find(|d| d.guid == g).map(|d| d.id)) {
        app.select_product(id);
    }
}
fn frame_all(app: &mut UiApp) {
    let hosts: Vec<String> = walls_here(&app.bimh.draw)
        .iter()
        .filter(|w| wall_type(&w.type_key).is_some_and(|t| t.layers.iter().any(|l| l.mat == "studs")))
        .map(|w| w.guid.clone())
        .collect();
    if hosts.is_empty() {
        app.error = Some(bt("res.err.noframe", &[]));
        return;
    }
    let n = hosts.len().to_string();
    let mut spec = json!({ "hosts": hosts });
    merge(&mut spec, &frame_opts(&app.bimh.draw.res));
    run_res(app, "frameHosts", vec![spec], &bt("res.fr.walls", &[("n", &n)]), &bt("res.verb.framed", &[]));
}

/// The tool changed (`toolChanged(name)`): what was being drawn goes.
pub fn tool_changed(app: &mut UiApp, name: &str) {
    let res = r(app);
    res.plane.pts = None;
    res.plane.at = None;
    res.found_hover = None;
    res.counter_hover = None;
    res.stair.start = None;
    res.stair.spec = None;
    res.rail.pts = None;
    res.el.at = None;
    res.sec_a = None;
    res.frame.hover = None;
    if name != "ellink" {
        res.el.link = None;
    }
}

/// Esc (`escape()`) → handled.
pub fn escape(app: &mut UiApp) -> bool {
    let res = r(app);
    if res.plane.pts.take().is_some() {
        return true;
    }
    if res.stair.start.take().is_some() {
        res.stair.spec = None;
        return true;
    }
    if res.rail.pts.take().is_some() || res.sec_a.take().is_some() {
        return true;
    }
    if app.bimh.draw.tool == "ellink" {
        end_link(app);
        return true;
    }
    false
}

/// Enter (`key(e)`) → handled.
pub fn enter(app: &mut UiApp) -> bool {
    let tool = app.bimh.draw.tool.clone();
    match tool.as_str() {
        "roofplane" if app.bimh.draw.res.plane.pts.is_some() => finish_plane(app),
        "railing" if app.bimh.draw.res.rail.pts.is_some() => finish_railing(app),
        "ellink" => end_link(app),
        _ => return false,
    }
    true
}

/// A double click (`dbl()`): the click's own point goes, the shape is finished.
pub fn dbl(app: &mut UiApp) -> bool {
    let tool = app.bimh.draw.tool.clone();
    match tool.as_str() {
        "roofplane" if app.bimh.draw.res.plane.pts.is_some() => {
            if let Some(p) = r(app).plane.pts.as_mut() {
                p.pop();
            }
            finish_plane(app);
        }
        "railing" if app.bimh.draw.res.rail.pts.is_some() => {
            if let Some(p) = r(app).rail.pts.as_mut() {
                p.pop();
            }
            finish_railing(app);
        }
        _ => return false,
    }
    true
}

// ------------------------------------------------------------------------------------------------------------ options

/// `bim.set` for the residential tools' rows → None when the key is not theirs.
pub fn set_option(app: &mut UiApp, k: &str, v: &str) -> Option<Result<(), String>> {
    let u = app.bimh.draw.units.clone();
    let len = |min: f64| -> Result<f64, String> {
        let mm = parse_len(v, &u);
        if mm >= min { Ok(mm) } else { Err(bt("model.err.length", &[])) }
    };
    let res = r(app);
    let out = match k {
        "rp.pitch" => {
            let d = parse_pitch(v);
            if (0.0..80.0).contains(&d) {
                res.plane.pitch = d;
                Ok(())
            } else {
                Err(bt("model.err.pitch", &[]))
            }
        }
        "rp.base" => {
            if cc_cad::js::trim(v).is_empty() {
                res.plane.base = None;
                Ok(())
            } else {
                let mm = parse_len(v, &u);
                if mm.is_finite() {
                    res.plane.base = Some(mm);
                    Ok(())
                } else {
                    Err(bt("model.err.length", &[]))
                }
            }
        }
        "fd.kind" => {
            if FOUNDATION_KINDS.contains(&v) {
                res.found_kind = v.into();
                Ok(())
            } else {
                Err(bt("res.err.fdkind", &[]))
            }
        }
        "ct.overhang" => len(0.0).map(|mm| res.counter_over = mm),
        "ct.thickness" => len(5.0).map(|mm| res.counter_thick = mm),
        "st.kind" => {
            if STAIR_KINDS.contains(&v) {
                res.stair.kind = v.into();
                Ok(())
            } else {
                Err(bt("res.err.stairkind", &[]))
            }
        }
        "st.turn" => {
            res.stair.turn = if v == "right" { "right".into() } else { "left".into() };
            Ok(())
        }
        "st.width" => {
            let mm = parse_len(v, &u);
            if mm >= 500.0 {
                res.stair.width = mm;
                Ok(())
            } else {
                Err(bt("res.err.stairwidth", &[]))
            }
        }
        "st.railings" => {
            if ["both", "left", "right", "none"].contains(&v) {
                res.stair.railings = v.into();
                Ok(())
            } else {
                Err("bim.set: st.railings both, left, right or none".into())
            }
        }
        "rl.kind" => {
            res.rail.kind = if v == "handrail" { "handrail".into() } else { "guard".into() };
            // a handrail's height and a guard's are both 36" (res-railing.js RAIL_DEFAULTS)
            res.rail.height = 36.0 * IN;
            Ok(())
        }
        "rl.height" => len(300.0).map(|mm| res.rail.height = mm),
        "fr.spacing" => {
            let x = cc_bim::js::str_num(v);
            if x.is_finite() && x > 0.0 {
                res.frame.spacing = nearest_spacing(&u, x).0;
                Ok(())
            } else {
                Err(bt("model.err.length", &[]))
            }
        }
        "fr.corners" => {
            res.frame.corners = if v == "california" { "california".into() } else { "three-stud".into() };
            Ok(())
        }
        "fr.roof" => {
            res.frame.roof_mode = if v == "truss" { "truss".into() } else { "stick".into() };
            Ok(())
        }
        "fr.blocking" => {
            res.frame.blocking = v == "on" || v == "true";
            Ok(())
        }
        "el.kind" => {
            if DEVICE_KINDS.iter().any(|d| d.0 == v) {
                res.el.kind = v.into();
                res.el.height = None;
                Ok(())
            } else {
                Err(bt("res.err.elkind", &[]))
            }
        }
        "el.circuit" => {
            res.el.circuit = cc_cad::js::trim(v).chars().take(20).collect();
            Ok(())
        }
        "el.height" => {
            if cc_cad::js::trim(v).is_empty() {
                res.el.height = None;
                Ok(())
            } else {
                len(f64::MIN_POSITIVE).map(|mm| res.el.height = Some(mm))
            }
        }
        _ => return None,
    };
    Some(out)
}

/// The tool's options row (ui.inc `[data-mopts]`), in the model bar.
pub fn bar_row(app: &mut UiApp, ui: &mut egui::Ui, run: &mut Vec<(&'static str, String)>) {
    let t = app.tokens;
    let u = app.bimh.draw.units.clone();
    let tool = app.bimh.draw.tool.clone();
    let res = app.bimh.draw.res.clone();
    let field = |app: &mut UiApp,
                 ui: &mut egui::Ui,
                 run: &mut Vec<(&'static str, String)>,
                 key: &'static str,
                 label: String,
                 cur: String,
                 w: f32| {
        let l = ui.label(RichText::new(label).small().color(t.text_dim));
        let ek = format!("res.{key}");
        let mut text = match &app.bimh.editing {
            Some((k, v)) if *k == ek => v.clone(),
            _ => cur.clone(),
        };
        let r = ui.add(crate::shell::field(&mut text).desired_width(w).id_salt(("bim_res", key))).labelled_by(l.id);
        if r.has_focus() || r.changed() {
            app.bimh.editing = Some((ek.clone(), text.clone()));
        }
        if r.lost_focus()
            && let Some((k, v)) = app.bimh.editing.take()
            && k == ek
            && v != cur
        {
            run.push(("bim.set", format!("{key}={v}")));
        }
    };
    let combo = |ui: &mut egui::Ui,
                 id: &str,
                 label: String,
                 cur: &str,
                 opts: &[(String, String)],
                 key: &'static str,
                 run: &mut Vec<(&'static str, String)>| {
        let shown = opts.iter().find(|o| o.0 == cur).map(|o| o.1.clone()).unwrap_or_default();
        egui::ComboBox::new(("bim_res_c", id.to_string()), label).selected_text(shown).show_ui(ui, |ui| {
            for (v, l) in opts {
                if ui.selectable_label(v == cur, l).clicked() {
                    run.push(("bim.set", format!("{key}={v}")));
                }
            }
        });
    };
    let keyed = |list: &[&str], pre: &str| -> Vec<(String, String)> {
        list.iter().map(|k| ((*k).to_string(), bt(&format!("{pre}{k}"), &[]))).collect()
    };
    match tool.as_str() {
        "roofplane" => {
            field(app, ui, run, "rp.pitch", bt("model.rf.pitch", &[]), fmt_pitch(res.plane.pitch, &u), 60.0);
            let base = res.plane.base.map(|b| fmt_len(b, &u)).unwrap_or_default();
            field(app, ui, run, "rp.base", bt("res.rp.base", &[]), base, 80.0);
            if res.plane.base.is_none() {
                ui.label(RichText::new(bt("res.rp.base.auto", &[])).small().color(t.text_dim));
            }
        }
        "foundation" => combo(ui, "fd", bt("res.fd.kind", &[]), &res.found_kind, &keyed(&FOUNDATION_KINDS, "res.fd.kind."), "fd.kind", run),
        "countertop" => {
            field(app, ui, run, "ct.overhang", bt("res.ct.overhang", &[]), fmt_len(res.counter_over, &u), 60.0);
            field(app, ui, run, "ct.thickness", bt("model.prop.thickness", &[]), fmt_len(res.counter_thick, &u), 60.0);
        }
        "stair" => {
            combo(ui, "stk", bt("res.st.kind", &[]), &res.stair.kind, &keyed(&STAIR_KINDS, "res.st.kind."), "st.kind", run);
            combo(ui, "stt", bt("res.st.turn", &[]), &res.stair.turn, &keyed(&["left", "right"], "res.st.turn."), "st.turn", run);
            field(app, ui, run, "st.width", bt("res.st.width", &[]), fmt_len(res.stair.width, &u), 60.0);
            combo(
                ui,
                "str",
                bt("res.st.railings", &[]),
                &res.stair.railings,
                &keyed(&["both", "left", "right", "none"], "res.st.rail."),
                "st.railings",
                run,
            );
        }
        "railing" => {
            let opts = vec![("guard".to_string(), bt("res.rail.guard", &[])), ("handrail".to_string(), bt("res.rail.handrail", &[]))];
            combo(ui, "rlk", bt("res.rl.kind", &[]), &res.rail.kind, &opts, "rl.kind", run);
            field(app, ui, run, "rl.height", bt("res.rl.height", &[]), fmt_len(res.rail.height, &u), 60.0);
        }
        "framing" => {
            let opts: Vec<(String, String)> = spacing_opts(&u).iter().map(|(v, l)| (cc_bim::js::num_str(*v), (*l).to_string())).collect();
            let cur = cc_bim::js::num_str(nearest_spacing(&u, res.frame.spacing).0);
            combo(ui, "frs", bt("res.fr.spacing", &[]), &cur, &opts, "fr.spacing", run);
            let co = vec![
                ("three-stud".to_string(), bt("res.fr.corner.three", &[])),
                ("california".to_string(), bt("res.fr.corner.california", &[])),
            ];
            combo(ui, "frc", bt("res.fr.corners", &[]), &res.frame.corners, &co, "fr.corners", run);
            let ro = vec![("stick".to_string(), bt("res.fr.roof.stick", &[])), ("truss".to_string(), bt("res.fr.roof.truss", &[]))];
            combo(ui, "frr", bt("res.fr.roofmode", &[]), &res.frame.roof_mode, &ro, "fr.roof", run);
            let mut b = res.frame.blocking;
            if ui.checkbox(&mut b, bt("res.fr.blocking.on", &[])).changed() {
                run.push(("bim.set", format!("fr.blocking={}", if b { "on" } else { "off" })));
            }
            if ui.add(crate::shell::Btn::new(bt("res.fr.all", &[]))).clicked() {
                run.push(("bim.res", "frameall".into()));
            }
            if ui.add(egui::Button::new(bt("res.fr.show", &[])).selected(res.frame.show)).clicked() {
                run.push(("bim.res", "frshow".into()));
            }
            if ui.add(egui::Button::new(bt("res.fr.see", &[])).selected(res.frame.see)).clicked() {
                run.push(("bim.res", "frsee".into()));
            }
        }
        "electrical" => {
            let shown = bt(&format!("res.el.kind.{}", res.el.kind), &[]);
            egui::ComboBox::new("bim_res_elk", bt("res.el.device", &[])).selected_text(shown).show_ui(ui, |ui| {
                for g in cc_bim::draw::reselec::GROUPS {
                    ui.label(RichText::new(bt(&format!("res.el.group.{g}"), &[])).small().strong().color(t.text_dim));
                    for k in cc_bim::draw::reselec::kinds_in(g) {
                        if ui.selectable_label(k == res.el.kind, bt(&format!("res.el.kind.{k}"), &[])).clicked() {
                            run.push(("bim.set", format!("el.kind={k}")));
                        }
                    }
                }
            });
            field(app, ui, run, "el.circuit", bt("res.el.sch.circuit", &[]), res.el.circuit.clone(), 50.0);
            let h = res.el.height.map(|h| fmt_len(h, &u)).unwrap_or_default();
            field(app, ui, run, "el.height", bt("res.el.height", &[]), h, 70.0);
            if res.el.height.is_none() {
                let ph = if wall_mounted(&res.el.kind) {
                    DEVICE_KINDS
                        .iter()
                        .find(|d| d.0 == res.el.kind)
                        .map(|d| fmt_len(if u == "mm" { d.6 } else { d.5 }, &u))
                        .unwrap_or_default()
                } else {
                    bt("res.el.height.ceiling", &[])
                };
                ui.label(RichText::new(ph).small().color(t.text_dim));
            }
        }
        "ellink" => {
            ui.label(RichText::new(bt("res.el.link.row", &[])).small().color(t.text_dim));
            if ui.add(crate::shell::Btn::new(bt("res.el.done", &[]))).clicked() {
                run.push(("bim.res", "linkdone".into()));
            }
        }
        _ => {}
    }
}

/// `bim.res` (see [`COMMANDS`]).
pub fn run(app: &mut UiApp, name: &str) -> Result<(), String> {
    match name {
        "finish" => {
            enter(app);
        }
        "dbl" => {
            dbl(app);
        }
        "escape" => {
            escape(app);
        }
        "frameall" => frame_all(app),
        "frshow" => {
            let f = &mut r(app).frame;
            f.show = !f.show;
            frame_vis(app);
            crate::bimplan::build_scene(app);
        }
        "frsee" => {
            let f = &mut r(app).frame;
            f.see = !f.see;
            frame_vis(app);
        }
        "linkdone" => end_link(app),
        "link" => {
            let p = pick(app).ok_or_else(|| bt("edit.nothing.picked", &[]))?;
            let d = app
                .bimh
                .draw
                .res
                .devs
                .iter()
                .find(|d| d.guid == p.guid && is_switch(&d.kind))
                .cloned()
                .ok_or_else(|| bt("res.err.elnotswitch", &[]))?;
            app.bimh.draw.tool = "ellink".into();
            tool_changed(app, "ellink");
            r(app).el.link = Some(d.guid.clone());
            app.message = bt("res.el.link.status", &[("tag", &d.tag)]);
        }
        "fit" => {
            let p = pick(app).filter(|p| p.kind == "countertop").ok_or_else(|| bt("edit.nothing.picked", &[]))?;
            run_res(app, "fitCountertop", vec![J::String(p.guid.clone())], &bt("res.counter", &[]), &changed());
            reselect(app, &p.guid);
        }
        "keep" => {
            let p = pick(app).and_then(|p| p.piece).ok_or_else(|| bt("edit.nothing.picked", &[]))?;
            run_res(app, "keepPiece", vec![J::String(p.guid)], &bt("res.fr.piece", &[]), &changed());
        }
        _ => {
            let spec = name.strip_prefix("edit:").ok_or("bim.res: see the params doc")?;
            let (k, v) = spec.split_once('=').ok_or("bim.res: edit:<field>=<value>")?;
            return edit(app, k, v);
        }
    }
    app.error.take().map_or(Ok(()), Err)
}

// ------------------------------------------------------------------------------------------------------------ picking

/// A framing piece picked (`pick(key).piece`).
#[derive(Clone, Debug)]
pub struct PieceInfo {
    pub guid: String,
    pub role: String,
    pub size: String,
    pub length: f64,
    pub generated: bool,
    pub check: String,
}
/// What `pick(key)` found: the kind, its GlobalId, a label, a framing piece.
#[derive(Clone, Debug)]
pub struct RPick {
    pub kind: &'static str,
    pub guid: String,
    pub label: String,
    pub piece: Option<PieceInfo>,
}

/// The tools' element a key (a product or one of its parts) belongs to.
pub fn pick_of(app: &mut UiApp, key: f64) -> Option<RPick> {
    let res = &app.bimh.draw.res;
    let mk = |kind: &'static str, guid: &str, label: String| Some(RPick { kind, guid: guid.to_string(), label, piece: None });
    if let Some(x) = res.planes.iter().find(|x| x.id == key || x.parts.contains(&key)) {
        return mk("roofplane", &x.guid, bt("res.roofplane", &[]));
    }
    if let Some(x) = res.founds.iter().find(|x| x.f.id == key || x.f.parts.contains(&key)) {
        return mk("foundation", &x.f.guid, bt(&format!("res.fd.kind.{}", x.f.kind), &[]));
    }
    if let Some(x) = res.counters.iter().find(|x| x.c.id == key) {
        return mk("countertop", &x.c.guid, bt("res.counter", &[]));
    }
    if let Some(x) = res.stairs.iter().find(|x| x.s.id == key || x.s.parts.contains(&key)) {
        return mk("stair", &x.s.guid, bt("res.stair", &[]));
    }
    if let Some(x) = res.rails.iter().find(|x| x.id == key) {
        return mk("railing", &x.guid, rail_label(&x.kind));
    }
    if let Some(x) = res.devs.iter().find(|x| x.id == key) {
        return mk("device", &x.guid, format!("{} · {}", x.tag, bt(&format!("res.el.kind.{}", x.kind), &[])));
    }
    if let Some(x) = res.secs.iter().find(|x| x.id == key) {
        return mk("section", &x.guid, bt("res.sec.name", &[("name", &x.name)]));
    }
    let fr = res.frames.iter().find(|x| x.f.id == key || x.f.parts.contains(&key))?.f.clone();
    let mut piece = None;
    if fr.parts.contains(&key)
        && let Some(doc) = app.bimh.ed.as_mut()
        && let Ok(mut w) = W::new(&mut doc.ed.api)
        && let Ok(Some(ps)) = w.read_pset(key, cc_bim::draw::resframe::LUMBER_PSET)
    {
        let s = |k: &str| ps.get(k).map(cc_bim::wapi::P::string).unwrap_or_default();
        let gen_ = ps.get("Generated").is_some_and(|g| match g {
            cc_bim::wapi::P::Bool(b) => *b,
            other => {
                let t = other.string().to_ascii_lowercase();
                t == "t" || t == ".t." || t == "true"
            }
        });
        let length = ps.get("Length").map_or(f64::NAN, cc_bim::wapi::P::number) * w.unit_mm;
        let guid = w.must(key).ok().and_then(|l| l.a("GlobalId").val_str()).unwrap_or_default();
        piece = Some(PieceInfo { guid, role: s("Role"), size: s("Size"), length, generated: gen_, check: s("Check") });
    }
    Some(RPick { kind: "framing", guid: fr.guid.clone(), label: bt(&format!("res.fr.of.{}", fr.kind), &[]), piece })
}

/// The picked element's [`pick_of`].
pub fn pick(app: &mut UiApp) -> Option<RPick> {
    let id = app.picked_product()?;
    pick_of(app, id)
}

/// After a change: the element picked again by its GlobalId (its parts were made again).
fn reselect(app: &mut UiApp, guid: &str) {
    let res = &app.bimh.draw.res;
    let first = |id: f64, parts: &[f64]| parts.first().copied().unwrap_or(id);
    let id = res
        .planes
        .iter()
        .find(|x| x.guid == guid)
        .map(|x| first(x.id, &x.parts))
        .or_else(|| res.founds.iter().find(|x| x.f.guid == guid).map(|x| first(x.f.id, &x.f.parts)))
        .or_else(|| res.stairs.iter().find(|x| x.s.guid == guid).map(|x| first(x.s.id, &x.s.parts)))
        .or_else(|| res.frames.iter().find(|x| x.f.guid == guid).map(|x| first(x.f.id, &x.f.parts)))
        .or_else(|| res.counters.iter().find(|x| x.c.guid == guid).map(|x| x.c.id))
        .or_else(|| res.devs.iter().find(|x| x.guid == guid).map(|x| x.id))
        .or_else(|| res.rails.iter().find(|x| x.guid == guid).map(|x| x.id))
        .or_else(|| res.secs.iter().find(|x| x.guid == guid).map(|x| x.id));
    if let Some(id) = id {
        app.select_product(id);
    }
}

/// Delete of the picked (`remove(items)`): one change for every residential element among them → None when some
/// picked element is not one of them.
pub fn remove_op(app: &mut UiApp, keys: &[f64]) -> Option<(Op, String)> {
    if keys.is_empty() {
        return None;
    }
    let mut picks = Vec::new();
    for &k in keys {
        picks.push(pick_of(app, k)?);
    }
    let of = |kind: &str| -> Vec<J> {
        let mut v: Vec<String> = picks.iter().filter(|p| p.kind == kind).map(|p| p.guid.clone()).collect();
        v.dedup();
        v.into_iter().map(J::String).collect()
    };
    let mut ops = Vec::new();
    for (kind, name) in [
        ("roofplane", "removeRoofPlanes"),
        ("foundation", "removeFoundations"),
        ("countertop", "removeCountertops"),
        ("stair", "removeStairs"),
        ("railing", "removeRailings"),
        ("framing", "removeFraming"),
        ("section", "removeSectionLines"),
        ("device", "removeDevices"),
    ] {
        let g = of(kind);
        if !g.is_empty() {
            ops.push(Op::Res(ResOp::new(name, vec![J::Array(g)])));
        }
    }
    let label = if picks.len() == 1 {
        picks.first().map(|p| p.label.clone()).unwrap_or_default()
    } else {
        bt("model.elements", &[("n", &picks.len().to_string())])
    };
    let op = if ops.len() == 1 { ops.pop()? } else { Op::Seq { ops, keep_first: false } };
    Some((op, label))
}

// ------------------------------------------------------------------------------------------------------------ Properties

/// `bim.res=edit:<field>=<value>`: a change of the picked element (`props(items, kit)`'s fields).
fn edit(app: &mut UiApp, k: &str, v: &str) -> Result<(), String> {
    let p = pick(app).ok_or_else(|| bt("edit.nothing.picked", &[]))?;
    let u = app.bimh.draw.units.clone();
    let len = |min: f64, allow_zero: bool| -> Result<f64, String> {
        let mm = parse_len(v, &u);
        if !mm.is_finite() || (if allow_zero { mm < 0.0 } else { mm < min }) { Err(bt("model.err.length", &[])) } else { Ok(mm) }
    };
    let any_len = || -> Result<f64, String> {
        let mm = parse_len(v, &u);
        if mm.is_finite() { Ok(mm) } else { Err(bt("model.err.length", &[])) }
    };
    let s = |x: &str| J::String(x.to_string());
    let (op, ch, title): (&str, J, String) = match p.kind {
        "roofplane" => {
            let ch = match k {
                "pitch" => {
                    let d = parse_pitch(v);
                    if !(0.0..80.0).contains(&d) {
                        return Err(bt("model.err.pitch", &[]));
                    }
                    json!({ "pitch": d })
                }
                "base" => json!({ "base": any_len()? }),
                "thickness" => json!({ "thickness": len(10.0, false)? }),
                _ => return Err(format!("bim.res: a roof plane has no field {k}")),
            };
            ("changeRoofPlane", ch, bt("res.roofplane", &[]))
        }
        "foundation" => {
            let ch = if k == "kind" {
                if !FOUNDATION_KINDS.contains(&v) {
                    return Err(bt("res.err.fdkind", &[]));
                }
                json!({ "kind": v })
            } else if PKEYS.contains(&k) {
                json!({ k: len(0.0, true)? })
            } else {
                return Err(format!("bim.res: a foundation has no field {k}"));
            };
            ("changeFoundation", ch, p.label.clone())
        }
        "countertop" => {
            let ch = match k {
                "overhang" => json!({ "overhang": len(0.0, true)? }),
                "thickness" => json!({ "thickness": len(5.0, false)? }),
                _ => return Err(format!("bim.res: a countertop has no field {k}")),
            };
            ("changeCountertop", ch, bt("res.counter", &[]))
        }
        "stair" => {
            let ch = match k {
                "kind" | "turn" | "railings" => json!({ k: v }),
                "width" => json!({ "width": len(500.0, false)? }),
                "riserMax" => json!({ "riserMax": len(100.0, false)? }),
                "tread" => json!({ "tread": len(150.0, false)? }),
                "railHeight" => json!({ "railHeight": len(300.0, false)? }),
                "well" => json!({ "well": v == "on" || v == "true" }),
                _ => return Err(format!("bim.res: a stair has no field {k}")),
            };
            ("changeStair", ch, bt("res.stair", &[]))
        }
        "railing" => {
            let ch = match k {
                "kind" => json!({ "kind": if v == "handrail" { "handrail" } else { "guard" } }),
                "height" => json!({ "height": len(300.0, false)? }),
                "base" => json!({ "base": any_len()? }),
                _ => return Err(format!("bim.res: a railing has no field {k}")),
            };
            ("changeRailing", ch, bt("res.rail.guard", &[]))
        }
        "framing" => {
            let ch = match k {
                "spacing" => json!({ "spacing": cc_bim::js::str_num(v) }),
                "corners" | "roofMode" => json!({ k: v }),
                "blocking" => json!({ "blocking": v == "on" || v == "true" }),
                _ => return Err(format!("bim.res: a framing has no field {k}")),
            };
            ("changeFraming", ch, p.label.clone())
        }
        "section" => {
            let sl = app.bimh.draw.res.secs.iter().find(|x| x.guid == p.guid).cloned().ok_or_else(|| bt("model.err.gone", &[]))?;
            let ch = match k {
                "name" => {
                    let n = cc_cad::js::trim(v);
                    if n.is_empty() {
                        return Ok(());
                    }
                    json!({ "name": n })
                }
                "look" => {
                    json!({ "look": if v == "flip" { if sl.look == "left" { "right" } else { "left" } } else if v == "right" { "right" } else { "left" } })
                }
                "depth" => json!({ "depth": len(0.0, true)? }),
                _ => return Err(format!("bim.res: a section line has no field {k}")),
            };
            ("changeSectionLine", ch, bt("res.sec.name", &[("name", &sl.name)]))
        }
        "device" => {
            let ch = match k {
                "tag" => {
                    if cc_cad::js::trim(v).is_empty() {
                        return Ok(());
                    }
                    json!({ "tag": v })
                }
                "circuit" => json!({ "circuit": v }),
                "load" => {
                    let x = cc_bim::js::str_num(v);
                    if x.is_nan() || x < 0.0 {
                        return Err(bt("model.err.length", &[]));
                    }
                    json!({ "load": x })
                }
                "height" => json!({ "height": len(50.0, false)? }),
                _ => return Err(format!("bim.res: a device has no field {k}")),
            };
            ("changeDevice", ch, p.label.clone())
        }
        _ => return Err(bt("edit.nothing.picked", &[])),
    };
    run_res(app, op, vec![s(&p.guid), ch], &title, &changed());
    reselect(app, &p.guid);
    app.error.take().map_or(Ok(()), Err)
}

/// The picked residential element's Properties block (`props(items, kit)`) → shown.
pub fn props(app: &mut UiApp, ui: &mut egui::Ui) -> bool {
    let Some(p) = pick(app) else { return false };
    let t = app.tokens;
    let u = app.bimh.draw.units.clone();
    let res = app.bimh.draw.res.clone();
    let mut out: Option<String> = None;
    let title = match res.devs.iter().find(|d| p.kind == "device" && d.guid == p.guid) {
        Some(d) => format!("{} · {}", d.tag, bt(&format!("res.el.kind.{}", d.kind), &[])),
        None => p.label.clone(),
    };
    let row = |app: &mut UiApp, ui: &mut egui::Ui, key: &str, label: &str, cur: String| -> Option<String> {
        let l = ui.label(RichText::new(label).small().color(t.text_dim));
        let ek = format!("res.mp.{key}");
        let mut text = match &app.bimh.editing {
            Some((k, v)) if *k == ek => v.clone(),
            _ => cur.clone(),
        };
        let r = ui
            .add(crate::shell::field(&mut text).desired_width(ui.available_width()).id_salt(("bim_resmp", key.to_string())))
            .labelled_by(l.id);
        if r.has_focus() || r.changed() {
            app.bimh.editing = Some((ek.clone(), text.clone()));
        }
        if r.lost_focus()
            && let Some((k, v)) = app.bimh.editing.take()
            && k == ek
            && v != cur
        {
            return Some(format!("edit:{key}={v}"));
        }
        None
    };
    let choice = |ui: &mut egui::Ui, label: &str, cur: &str, opts: &[(&str, String)], key: &str, out: &mut Option<String>| {
        ui.label(RichText::new(label).small().color(t.text_dim));
        ui.horizontal_wrapped(|ui| {
            for (v, l) in opts {
                if ui.selectable_label(*v == cur, l).clicked() && *v != cur {
                    *out = Some(format!("edit:{key}={v}"));
                }
            }
        });
    };
    let info = |ui: &mut egui::Ui, label: &str, value: String| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).small().color(t.text_dim));
            ui.label(RichText::new(value).small());
        });
    };
    let note = |ui: &mut egui::Ui, text: String| {
        ui.label(RichText::new(text).small().color(t.text_dim));
    };
    let mut body = |app: &mut UiApp, ui: &mut egui::Ui| match p.kind {
        "roofplane" => {
            let Some(rp) = res.planes.iter().find(|x| x.guid == p.guid) else { return };
            out = out.take().or(row(app, ui, "pitch", &bt("model.rf.pitch", &[]), fmt_pitch(rp.pitch, &u)));
            out = out.take().or(row(app, ui, "base", &bt("res.rp.base", &[]), fmt_len(rp.base, &u)));
            out = out.take().or(row(app, ui, "thickness", &bt("model.prop.thickness", &[]), fmt_len(rp.thickness, &u)));
            let a = m::area(&rp.points).abs() / m::cos(rp.pitch * D2R);
            info(ui, &bt("res.rp.area", &[]), fmt_area(a, &u));
        }
        "foundation" => {
            let Some(f) = res.founds.iter().find(|x| x.f.guid == p.guid) else { return };
            let kinds: Vec<(&str, String)> = FOUNDATION_KINDS.iter().map(|k| (*k, bt(&format!("res.fd.kind.{k}"), &[]))).collect();
            choice(ui, &bt("res.fd.kind", &[]), &f.f.kind, &kinds, "kind", &mut out);
            let k = f.f.kind.as_str();
            for (key, label, show) in [
                ("footW", "res.fd.footw", k != "slab"),
                ("footD", "res.fd.footd", k != "slab"),
                ("bottom", "res.fd.bottom", k == "stemslab" || k == "crawl"),
                ("stemT", "res.fd.stemt", k != "slab"),
                ("stemDrop", "res.fd.stemdrop", k == "crawl" || k == "basement"),
                ("basementH", "res.fd.basementh", k == "basement"),
                ("slabT", "res.fd.slabt", k != "crawl"),
                ("edgeW", "res.fd.edgew", k == "slab"),
                ("edgeD", "res.fd.edged", k == "slab"),
            ] {
                if !show {
                    continue;
                }
                let i = PKEYS.iter().position(|x| *x == key).unwrap_or(0);
                let v = f.prm.v.get(i).copied().unwrap_or(0.0);
                let auto = f.prm.auto.get(i).copied().unwrap_or(false);
                let lab = if auto { format!("{} {}", bt(label, &[]), bt("res.fd.auto", &[])) } else { bt(label, &[]) };
                out = out.take().or(row(app, ui, key, &lab, fmt_len(v, &u)));
            }
            note(ui, bt("res.fd.autonote", &[("n", &cc_bim::js::num_str(f.stories))]));
        }
        "countertop" => {
            let Some(c) = res.counters.iter().find(|x| x.c.guid == p.guid) else { return };
            out = out.take().or(row(app, ui, "overhang", &bt("res.ct.overhang", &[]), fmt_len(c.c.overhang, &u)));
            out = out.take().or(row(app, ui, "thickness", &bt("model.prop.thickness", &[]), fmt_len(c.c.thickness, &u)));
            if let Some(run) = &c.run {
                info(ui, &bt("res.ct.area", &[]), fmt_area(run.area, &u));
                info(ui, &bt("res.ct.cabinets", &[]), run.cabinets.len().to_string());
            }
            let state = if c.run.is_none() {
                "res.ct.gone"
            } else if c.stale {
                "res.ct.stale"
            } else {
                "res.ct.fits"
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(bt("res.ct.cabinetsnow", &[])).small().color(t.text_dim));
                ui.label(RichText::new(bt(state, &[])).small());
                if ui.add_enabled(c.stale, crate::shell::Btn::new(bt("res.ct.fit", &[]))).clicked() {
                    out = Some("fit".into());
                }
            });
        }
        "stair" => {
            let Some(st) = res.stairs.iter().find(|x| x.s.guid == p.guid) else { return };
            let s = &st.s;
            let kinds: Vec<(&str, String)> = STAIR_KINDS.iter().map(|k| (*k, bt(&format!("res.st.kind.{k}"), &[]))).collect();
            choice(ui, &bt("res.st.kind", &[]), &s.kind, &kinds, "kind", &mut out);
            if s.kind != "straight" {
                let turns = [("left", bt("res.st.turn.left", &[])), ("right", bt("res.st.turn.right", &[]))];
                choice(ui, &bt("res.st.turn", &[]), &s.turn, &turns, "turn", &mut out);
            }
            out = out.take().or(row(app, ui, "width", &bt("res.st.width", &[]), fmt_len(s.width, &u)));
            out = out.take().or(row(app, ui, "riserMax", &bt("res.st.risermax", &[]), fmt_len(s.riser_max, &u)));
            out = out.take().or(row(app, ui, "tread", &bt("res.st.tread", &[]), fmt_len(s.tread, &u)));
            let rails: Vec<(&str, String)> =
                ["both", "left", "right", "none"].iter().map(|k| (*k, bt(&format!("res.st.rail.{k}"), &[]))).collect();
            choice(ui, &bt("res.st.railings", &[]), &s.railings, &rails, "railings", &mut out);
            if s.railings != "none" {
                out = out.take().or(row(app, ui, "railHeight", &bt("res.st.railheight", &[]), fmt_len(s.rail_height, &u)));
            }
            let mut well = s.well;
            if ui.checkbox(&mut well, bt("res.st.well.on", &[])).changed() {
                out = Some(format!("edit:well={}", if well { "on" } else { "off" }));
            }
            if let Some(l) = &st.layout {
                let v = |n: f64, r: f64| bt("res.st.risers.v", &[("n", &cc_bim::js::num_str(n)), ("r", &fmt_len(r, &u))]);
                info(ui, &bt("res.st.risers", &[]), v(l.n, l.riser));
                info(ui, &bt("res.st.treads", &[]), v(l.treads, l.tread));
                let bad: Vec<String> = l.checks.iter().filter(|c| !c.1).map(|c| bt(&format!("res.st.check.{}", c.0), &[])).collect();
                note(ui, if bad.is_empty() { bt("res.st.checks.ok", &[]) } else { bt("res.st.checks.bad", &[("list", &bad.join(", "))]) });
            }
        }
        "railing" => {
            let Some(rl) = res.rails.iter().find(|x| x.guid == p.guid) else { return };
            let kinds = [("guard", bt("res.rail.guard", &[])), ("handrail", bt("res.rail.handrail", &[]))];
            choice(ui, &bt("res.rl.kind", &[]), &rl.kind, &kinds, "kind", &mut out);
            out = out.take().or(row(app, ui, "height", &bt("res.rl.height", &[]), fmt_len(rl.height, &u)));
            out = out.take().or(row(app, ui, "base", &bt("res.rl.base", &[]), fmt_len(rl.base, &u)));
            let path: Vec<[f64; 3]> = rl.points.iter().map(|q| [q[0], q[1], rl.base]).collect();
            let parts = railing_parts(&path, rl.height);
            info(ui, &bt("res.rl.length", &[]), fmt_len(parts.length, &u));
            info(ui, &bt("res.rl.balusters", &[]), parts.balusters.len().to_string());
        }
        "framing" => {
            let Some(fr) = res.frames.iter().find(|x| x.f.guid == p.guid) else { return };
            let opts: Vec<(String, &str)> = spacing_opts(&u).iter().map(|(v, l)| (cc_bim::js::num_str(*v), *l)).collect();
            let cur = cc_bim::js::num_str(nearest_spacing(&u, fr.f.spacing).0);
            let o2: Vec<(&str, String)> = opts.iter().map(|(v, l)| (v.as_str(), (*l).to_string())).collect();
            choice(ui, &bt("res.fr.spacing", &[]), &cur, &o2, "spacing", &mut out);
            if fr.f.kind == "wall" {
                let co = [("three-stud", bt("res.fr.corner.three", &[])), ("california", bt("res.fr.corner.california", &[]))];
                choice(ui, &bt("res.fr.corners", &[]), &fr.f.corners, &co, "corners", &mut out);
                let mut b = fr.f.blocking;
                if ui.checkbox(&mut b, bt("res.fr.blocking.on", &[])).changed() {
                    out = Some(format!("edit:blocking={}", if b { "on" } else { "off" }));
                }
            }
            if fr.f.kind == "roof" {
                let ro = [("stick", bt("res.fr.roof.stick", &[])), ("truss", bt("res.fr.roof.truss", &[]))];
                choice(ui, &bt("res.fr.roofmode", &[]), &fr.f.roof_mode, &ro, "roofMode", &mut out);
            }
            info(ui, &bt("res.fr.pieces", &[]), cc_bim::js::num_str(fr.pieces));
            info(ui, &bt("res.fr.bf", &[]), cc_bim::js::num_str(fr.bf.round()));
            if fr.notes != "ok" && !fr.notes.is_empty() {
                note(ui, fr.notes.split(',').map(|x| bt(&format!("res.fr.note.{x}"), &[])).collect::<Vec<_>>().join(" "));
            }
            if let Some(pc) = &p.piece {
                let size = if u == "mm" { metric_size(&pc.size) } else { pc.size.clone() };
                info(
                    ui,
                    &bt("res.fr.piece", &[]),
                    format!("{}, {size}, {}", bt(&format!("res.fr.role.{}", pc.role), &[]), fmt_len(pc.length, &u)),
                );
                if !pc.check.is_empty() {
                    note(ui, pc.check.clone());
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(bt("res.fr.keepq", &[])).small().color(t.text_dim));
                    let label = bt(if pc.generated { "res.fr.keep" } else { "res.fr.kept" }, &[]);
                    if ui.add_enabled(pc.generated, crate::shell::Btn::new(label)).clicked() {
                        out = Some("keep".into());
                    }
                });
            }
            note(ui, bt("res.fr.note", &[]));
        }
        "section" => {
            let Some(sl) = res.secs.iter().find(|x| x.guid == p.guid) else { return };
            out = out.take().or(row(app, ui, "name", &bt("res.sec.label", &[]), sl.name.clone()));
            ui.horizontal(|ui| {
                ui.label(RichText::new(bt("res.sec.look", &[])).small().color(t.text_dim));
                if ui.add(crate::shell::Btn::new(bt("res.sec.flip", &[]))).clicked() {
                    out = Some("edit:look=flip".into());
                }
            });
            out = out.take().or(row(app, ui, "depth", &bt("res.sec.depth", &[]), fmt_len(sl.depth, &u)));
            note(ui, bt("res.sec.depth.note", &[]));
        }
        "device" => {
            let Some(d) = res.devs.iter().find(|x| x.guid == p.guid) else { return };
            out = out.take().or(row(app, ui, "tag", &bt("res.el.sch.tag", &[]), d.tag.clone()));
            out = out.take().or(row(app, ui, "circuit", &bt("res.el.sch.circuit", &[]), d.circuit.clone()));
            if d.kind != "board" {
                out = out.take().or(row(app, ui, "load", &format!("{} (W)", bt("res.el.load", &[])), cc_bim::js::num_str(d.load.round())));
            }
            let hl = if wall_mounted(&d.kind) { "res.el.height" } else { "res.el.height.at" };
            out = out.take().or(row(app, ui, "height", &bt(hl, &[]), fmt_len(d.height, &u)));
            let tags = |gs: Vec<&String>| -> String {
                let v: Vec<String> = gs.iter().filter_map(|g| res.devs.iter().find(|x| x.guid == **g).map(|x| x.tag.clone())).collect();
                if v.is_empty() { bt("res.el.controls.none", &[]) } else { v.join(", ") }
            };
            if is_switch(&d.kind) {
                info(ui, &bt("res.el.controls", &[]), tags(d.controls.iter().collect()));
                if ui.add(crate::shell::Btn::new(bt("res.el.link", &[]))).clicked() {
                    out = Some("link".into());
                }
            }
            if is_light(&d.kind) {
                let by: Vec<&String> = res.devs.iter().filter(|x| x.controls.contains(&d.guid)).map(|x| &x.guid).collect();
                info(ui, &bt("res.el.switchedby", &[]), tags(by));
            }
        }
        _ => {}
    };
    ui.label(RichText::new(&title).strong().color(t.text_strong));
    body(app, ui);
    ui.separator();
    if let Some(name) = out {
        app.run_ui("bim.res", &UParams { name: Some(name), ..Default::default() });
    }
    true
}

/// `sizeLabel(sz)` in a metric model: 2x6 → 38x140.
fn metric_size(sz: &str) -> String {
    let base: String = sz.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    match metric_name(&base) {
        Some(mtr) => format!("{mtr}{}", sz.get(base.len()..).unwrap_or("")),
        None => sz.to_string(),
    }
}

// ------------------------------------------------------------------------------------------------------------ schedules

/// The residential schedules the Schedules panel offers: Lumber (always), Devices and Circuits (once there are devices).
pub fn schedule_kinds(app: &UiApp) -> Vec<(&'static str, String)> {
    let mut v = vec![("lumber", bt("res.fr.sch.lumber", &[]))];
    if !app.bimh.draw.res.devs.is_empty() {
        v.push(("devices", bt("res.el.sch.devicesched", &[])));
        v.push(("circuits", bt("res.el.sch.circuitsched", &[])));
    }
    v
}

/// One of the residential schedules (`lumberSchedule`, `deviceSchedule`, `circuitSchedule`).
pub fn schedule(app: &mut UiApp, which: &str) -> Option<Schedule> {
    let u = app.bimh.draw.units.clone();
    let total = bt("model.sch.total", &[]);
    match which {
        "lumber" => {
            let doc = app.bimh.ed.as_mut()?;
            let mut w = W::new(&mut doc.ed.api).ok()?;
            let (rows, pieces, bf) = lumber_list(&mut w, u == "mm").ok()?;
            let head = ["res.fr.sch.size", "res.fr.sch.length", "res.fr.sch.count", "res.fr.sch.bf"].iter().map(|k| bt(k, &[])).collect();
            let total = (!rows.is_empty()).then(|| vec![total, String::new(), cc_bim::js::num_str(pieces), format!("{bf:.1}")]);
            let rows = rows
                .iter()
                .map(|r| vec![r.label.clone(), fmt_len(r.stock, &u), cc_bim::js::num_str(r.count), format!("{:.1}", r.bf)])
                .collect();
            Some(Schedule { head, rows, total, nums: Vec::new() })
        }
        "devices" => {
            let devs = &app.bimh.draw.res.devs;
            let levels = app.bimh.draw.data.as_ref().map(|s| s.levels.clone()).unwrap_or_default();
            let tag_of = |g: &String| devs.iter().find(|d| d.guid == *g).map(|d| d.tag.clone());
            let rows = devs
                .iter()
                .map(|d| {
                    vec![
                        d.tag.clone(),
                        bt(&format!("res.el.kind.{}", d.kind), &[]),
                        levels.iter().find(|l| Some(l.id) == d.storey).map(|l| l.name.clone()).unwrap_or_default(),
                        d.circuit.clone(),
                        if d.load != 0.0 && !d.load.is_nan() { cc_bim::js::num_str(d.load.round()) } else { String::new() },
                        fmt_len(d.height, &u),
                        d.controls.iter().filter_map(tag_of).collect::<Vec<_>>().join(", "),
                    ]
                })
                .collect();
            let watts: f64 = devs.iter().map(|d| if d.load.is_finite() { d.load } else { 0.0 }).sum();
            let head = [
                "res.el.sch.tag",
                "res.el.sch.device",
                "res.el.sch.level",
                "res.el.sch.circuit",
                "res.el.sch.load",
                "res.el.sch.height",
                "res.el.sch.controls",
            ]
            .iter()
            .map(|k| bt(k, &[]))
            .collect();
            let total = vec![
                total,
                devs.len().to_string(),
                String::new(),
                String::new(),
                cc_bim::js::num_str(watts.round()),
                String::new(),
                String::new(),
            ];
            Some(Schedule { head, rows, total: Some(total), nums: Vec::new() })
        }
        "circuits" => {
            let devs = &app.bimh.draw.res.devs;
            // (circuit, devices, watts, volts, kinds in order with their counts)
            let mut list: Vec<CircuitRow> = Vec::new();
            for d in devs.iter().filter(|d| d.kind != "board") {
                let volts = if d.kind == "socket.3ph" {
                    if u == "mm" { 400.0 } else { 240.0 }
                } else if u == "mm" {
                    230.0
                } else {
                    120.0
                };
                let i = match list.iter().position(|x| x.0 == d.circuit) {
                    Some(i) => i,
                    None => {
                        list.push((d.circuit.clone(), 0, 0.0, 0.0, Vec::new()));
                        list.len().saturating_sub(1)
                    }
                };
                let Some(row) = list.get_mut(i) else { continue };
                row.1 = row.1.saturating_add(1);
                row.2 += if d.load.is_finite() { d.load } else { 0.0 };
                row.3 = m::max(row.3, volts);
                match row.4.iter_mut().find(|k| k.0 == d.kind) {
                    Some(k) => k.1 = k.1.saturating_add(1),
                    None => row.4.push((d.kind.clone(), 1)),
                }
            }
            list.sort_by(|a, b| a.0.is_empty().cmp(&b.0.is_empty()).then_with(|| cc_bim::draw::reselec::cmp_tag(&a.0, &b.0)));
            let rows = list
                .iter()
                .map(|r| {
                    vec![
                        if r.0.is_empty() { bt("res.el.nocircuit", &[]) } else { r.0.clone() },
                        r.1.to_string(),
                        r.4.iter().map(|(k, n)| format!("{n} × {}", bt(&format!("res.el.kind.{k}"), &[]))).collect::<Vec<_>>().join(", "),
                        cc_bim::js::num_str(r.2.round()),
                        format!("{:.1}", r.2 / r.3),
                        cc_bim::js::num_str(r.3),
                    ]
                })
                .collect();
            let watts: f64 = list.iter().map(|r| r.2).sum();
            let n: usize = list.iter().map(|r| r.1).sum();
            let head =
                ["res.el.sch.circuit", "res.el.sch.count", "res.el.sch.devices", "res.el.sch.load", "res.el.sch.amps", "res.el.sch.volts"]
                    .iter()
                    .map(|k| bt(k, &[]))
                    .collect();
            let total = vec![total, n.to_string(), String::new(), cc_bim::js::num_str(watts.round()), String::new(), String::new()];
            Some(Schedule { head, rows, total: Some(total), nums: Vec::new() })
        }
        _ => None,
    }
}
