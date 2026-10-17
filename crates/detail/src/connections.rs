//! connections.js — the connections DetailCraft builds, as generators (nothing derived is stored): end plates (beam to
//! column flange or web), shear tabs (square, skewed, sloped), base plates (with grout, stiffeners, a shear lug, plate
//! washers), bolted splices with cover plates, brace gussets; the joint finders (`findJoints`, `findSplices`,
//! `findBraceEnds` …) and "apply to similar". Defaults are starting values, not design; checks are EN 1993-1-8 Table
//! 3.3 minimums and fit checks, as warnings.

use crate::geom::{Frame, V3, add, dot, frame, len, mul, norm, sub};
use crate::i18n::{F, t, t0};
use crate::js::{Obj, acos, asin, atan2, fixed_num, max, max_of, min, min_of, num_to_string, round, sign};
use crate::profiles::P;
use crate::state::{Conn, Mem};
use serde_json::Value;
use std::collections::HashMap;
use std::f64::consts::PI;

const TOL: f64 = 1.0;
/// Loop cap for counts taken from connection parameters (the model keeps them ≤ 10 000).
const MAX_N: f64 = 10_001.0;

/// The defaults of a connection type, as the JS objects (key order kept).
pub fn defaults_of(kind: &str) -> Option<Value> {
    Some(match kind {
        "endplate" => {
            serde_json::json!({ "tp": 20, "width": 180, "extTop": 0, "extBottom": 0, "boltD": 20, "hole": 22, "grade": "A325", "rows": 4, "pitch": 75, "gauge": 100, "allowance": 30, "weld": 8, "plateGrade": "A36" })
        }
        "sheartab" => {
            serde_json::json!({ "tp": 10, "width": 115, "boltD": 20, "hole": 22, "grade": "A325", "rows": 3, "pitch": 75, "edge": 38, "ev": 0, "gap": 13, "eh": 65, "allowance": 30, "weld": 6, "plateGrade": "A36" })
        }
        "baseplate" => {
            serde_json::json!({ "tp": 25, "length": 450, "width": 400, "rodD": 20, "hole": 33, "grade": "F1554 Gr 36", "sx": 350, "sy": 300, "projection": 100, "embed": 450, "weld": 8, "plateGrade": "A36", "grout": 0, "stiffeners": 0, "stiffT": 12, "stiffH": 150, "lugD": 0, "lugT": 25, "lugW": 150, "washers": 0, "washerSize": 0, "washerT": 0 })
        }
        "splice" => {
            serde_json::json!({ "jointGap": 10, "boltD": 20, "hole": 22, "grade": "A325", "allowance": 30, "plateGrade": "A36", "flangeT": 12, "flangeRows": 3, "flangePitch": 75, "flangeGauge": 0, "flangeEdge": 40, "webT": 8, "webRows": 3, "webCols": 2, "webPitch": 75, "webPitchX": 75, "webEdge": 40, "webH": 0 })
        }
        "gusset" => {
            serde_json::json!({ "tp": 12, "braceGap": 25, "boltD": 20, "hole": 22, "grade": "A325", "bolts": 3, "bracePitch": 75, "braceEdge": 40, "braceGauge": 0, "allowance": 30, "lap": 200, "weld": 6, "plateGrade": "A36" })
        }
        _ => return None,
    })
}

/// AISC Design Guide 1, 2nd ed., Table 2.3: [rod, hole, min. washer, min. washer thickness] (in).
pub const DG1_TABLE_2_3: [[f64; 4]; 8] = [
    [0.75, 1.0 + 5.0 / 16.0, 2.0, 0.25],
    [0.875, 1.0 + 9.0 / 16.0, 2.5, 5.0 / 16.0],
    [1.0, 1.0 + 13.0 / 16.0, 3.0, 0.375],
    [1.25, 2.0 + 1.0 / 16.0, 3.0, 0.5],
    [1.5, 2.0 + 5.0 / 16.0, 3.5, 0.5],
    [1.75, 2.75, 4.0, 0.625],
    [2.0, 3.25, 5.0, 0.75],
    [2.5, 3.25, 5.5, 0.875],
];
/// `dg1Row(rodD)`.
#[derive(Clone, Copy, Debug)]
pub struct Dg1 {
    pub rod: f64,
    pub hole: f64,
    pub washer: f64,
    pub washer_t: f64,
    pub inch: [f64; 4],
}
pub fn dg1_row(rod_d: f64) -> Option<Dg1> {
    let r = DG1_TABLE_2_3.iter().find(|r| r[0] * 25.4 >= rod_d - 0.5)?;
    Some(Dg1 { rod: r[0] * 25.4, hole: r[1] * 25.4, washer: r[2] * 25.4, washer_t: r[3] * 25.4, inch: *r })
}

/// `PARAM_LABEL_KEYS[name]` for the two the splice check names.
fn param_label_key(name: &str) -> &'static str {
    match name {
        "flangeEdge" => "conn.param.flange-edge",
        _ => "conn.param.web-edge",
    }
}

/// `num(v)` = `String(Number(Number(v).toFixed(1)))`.
pub fn num1(v: f64) -> String {
    num_to_string(fixed_num(v, 1))
}
fn fs(v: f64) -> F {
    F::S(num1(v))
}

// ---------------------------------------------------------------------------------------------------------------
// what a connection makes

/// A plate a connection adds.
#[derive(Clone, Debug)]
pub struct PlateDraft {
    pub key: String,
    pub role: String,
    pub profile_name: String,
    pub material: String,
    pub start: V3,
    pub end: V3,
    pub rotation: f64,
    pub attach_to: Option<String>,
    pub loose: bool,
}
/// A hole in world space, for a member or a plate.
#[derive(Clone, Debug)]
pub struct WHole {
    pub target: String,
    pub world: V3,
    pub face: &'static str,
    pub d: f64,
}
/// A bolt or anchor rod.
#[derive(Clone, Debug)]
pub struct Bolt {
    pub conn: Option<String>,
    pub loose: Option<String>,
    pub d: f64,
    pub length: f64,
    pub grip: f64,
    pub grade: String,
    pub head: V3,
    pub axis: V3,
    pub kind: String,
    pub phase: f64,
}
/// A weld.
#[derive(Clone, Debug)]
pub struct Weld {
    pub conn: Option<String>,
    pub stair: Option<String>,
    pub kind: &'static str,
    pub size: f64,
    pub between: [String; 2],
    pub length: f64,
    pub r#where: String,
    pub all_round: bool,
}
/// A cut-back of a member end.
#[derive(Clone, Debug)]
pub struct Setback {
    pub member: String,
    pub end: String,
    pub by: f64,
}
/// What a builder returns: `built` false = `{ problems }` only (it cannot be built).
#[derive(Clone, Debug, Default)]
pub struct Made {
    pub built: bool,
    pub setbacks: Vec<Setback>,
    pub plates: Vec<PlateDraft>,
    pub holes: Vec<WHole>,
    pub bolts: Vec<Bolt>,
    pub welds: Vec<Weld>,
    pub problems: Vec<String>,
    pub info: Option<Value>,
}
fn fail(problems: Vec<String>) -> Made {
    Made { built: false, problems, ..Made::default() }
}
fn uniq(v: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(v.len());
    for s in v {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

/// A member as a connection sees it while building (`{ m, profile, frame, voff }`).
#[derive(Clone, Debug)]
pub struct Draft {
    pub m: Mem,
    pub profile: P,
    pub frame: Frame,
    pub voff: f64,
    pub setback: [f64; 2],
}
/// `ctx.parts`: drafts by member id, in the state's order.
#[derive(Default)]
pub struct Drafts {
    pub list: Vec<Draft>,
    pub by: HashMap<String, usize>,
}
impl Drafts {
    pub fn get(&self, id: Option<&str>) -> Option<&Draft> {
        id.and_then(|k| self.by.get(k)).and_then(|&i| self.list.get(i))
    }
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Draft> {
        let i = *self.by.get(id)?;
        self.list.get_mut(i)
    }
    pub fn insert(&mut self, d: Draft) {
        match self.by.get(&d.m.id) {
            Some(&i) => {
                if let Some(slot) = self.list.get_mut(i) {
                    *slot = d;
                }
            }
            None => {
                self.by.insert(d.m.id.clone(), self.list.len());
                self.list.push(d);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// joints

/// Distance from P to the segment a–b.
fn seg_dist(p: V3, a: V3, b: V3) -> f64 {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    let k = if l2 > 0.0 { max(0.0, min(1.0, dot(sub(p, a), ab) / l2)) } else { 0.0 };
    len(sub(p, add(a, mul(ab, k))))
}

/// `isVertical(m)`.
pub fn is_vertical(m: &Mem) -> bool {
    (m.start[0] - m.end[0]).abs() < TOL && (m.start[1] - m.end[1]).abs() < TOL && (m.start[2] - m.end[2]).abs() >= TOL
}

/// A brace end on a member.
#[derive(Clone, Debug)]
pub struct BraceEnd {
    pub brace: String,
    pub end: &'static str,
    pub support: String,
    pub others: Vec<String>,
}
/// `findBraceEnds(state, ids)`.
pub fn find_brace_ends(ms: &[Mem], ids: Option<&[String]>) -> Vec<BraceEnd> {
    let mut out = Vec::new();
    for b in ms {
        if b.kind != "brace" || ids.is_some_and(|ids| !ids.contains(&b.id)) {
            continue;
        }
        for end in ["start", "end"] {
            let p = b.at(end);
            let at: Vec<&Mem> =
                ms.iter().filter(|m| m.id != b.id && seg_dist(p, m.start, m.end) <= TOL).filter(|m| m.kind != "brace").collect();
            let Some(&first) = at.first() else { continue };
            let sup = at.iter().copied().find(|m| m.kind == "column").unwrap_or(first);
            out.push(BraceEnd {
                brace: b.id.clone(),
                end,
                support: sup.id.clone(),
                others: at.iter().filter(|m| !std::ptr::eq(**m, sup)).map(|m| m.id.clone()).collect(),
            });
        }
    }
    out
}
/// `freeBraceEnds(state, ids)`.
pub fn free_brace_ends(ms: &[Mem], cs: &[Conn], ids: Option<&[String]>) -> Vec<BraceEnd> {
    let taken: Vec<String> =
        cs.iter().filter(|c| c.kind == "gusset").map(|c| format!("{}:{}", c.brace.as_deref().unwrap_or("undefined"), c.end)).collect();
    find_brace_ends(ms, ids).into_iter().filter(|j| !taken.contains(&format!("{}:{}", j.brace, j.end))).collect()
}
/// `similarBraceEnds(state, conn)` (conn: `{ brace, support }`).
pub fn similar_brace_ends(ms: &[Mem], cs: &[Conn], brace: Option<&str>, support: Option<&str>) -> Vec<BraceEnd> {
    let by: HashMap<&str, &Mem> = ms.iter().map(|m| (m.id.as_str(), m)).collect();
    let up = |id: Option<&str>| id.and_then(|i| by.get(i)).map_or_else(|| "undefined".to_string(), |m| crate::js::upper(&m.profile));
    let k = format!("{}|{}", up(brace), up(support));
    free_brace_ends(ms, cs, None).into_iter().filter(|j| format!("{}|{}", up(Some(&j.brace)), up(Some(&j.support))) == k).collect()
}

/// Two members end to end in line.
#[derive(Clone, Debug)]
pub struct SpliceJoint {
    pub a: String,
    pub a_end: &'static str,
    pub b: String,
    pub b_end: &'static str,
    pub gap: f64,
}
/// `findSplices(state, ids)`.
pub fn find_splices(ms: &[Mem], ids: Option<&[String]>) -> Vec<SpliceJoint> {
    let sel: Vec<&Mem> = ms.iter().filter(|m| ids.is_none_or(|ids| ids.contains(&m.id))).collect();
    let mut out = Vec::new();
    for (i, a) in sel.iter().enumerate() {
        for b in sel.iter().skip(i.saturating_add(1)) {
            let da = norm(sub(a.end, a.start));
            let db = norm(sub(b.end, b.start));
            if dot(da, db).abs() < 0.9999 {
                continue;
            }
            for a_end in ["start", "end"] {
                for b_end in ["start", "end"] {
                    let (p, q) = (a.at(a_end), b.at(b_end));
                    let d = sub(q, p);
                    let along = dot(d, da);
                    let off = len(sub(d, mul(da, along)));
                    if off > TOL || len(d) > 50.0 {
                        continue;
                    }
                    let out_ = if a_end == "end" { 1.0 } else { -1.0 };
                    let b_other = b.at(if b_end == "start" { "end" } else { "start" });
                    if dot(sub(b_other, p), da) * out_ <= 0.0 {
                        continue;
                    }
                    out.push(SpliceJoint { a: a.id.clone(), a_end, b: b.id.clone(), b_end, gap: along.abs() });
                }
            }
        }
    }
    out
}

/// A beam end at a column.
#[derive(Clone, Debug)]
pub struct Joint {
    pub beam: String,
    pub end: &'static str,
    pub column: String,
    pub face: &'static str,
    pub away: V3,
}
/// `findJoints(state)`.
pub fn find_joints(ms: &[Mem]) -> Vec<Joint> {
    let cols: Vec<&Mem> = ms.iter().filter(|m| m.kind == "column" && is_vertical(m)).collect();
    let mut out = Vec::new();
    for b in ms {
        if b.kind != "beam" {
            continue;
        }
        for end in ["start", "end"] {
            let n = b.at(end);
            let o = if end == "start" { b.end } else { b.start };
            for c in &cols {
                let z0 = min(c.start[2], c.end[2]);
                let z1 = max(c.start[2], c.end[2]);
                if (n[0] - c.start[0]).abs() > TOL || (n[1] - c.start[1]).abs() > TOL || n[2] < z0 - TOL || n[2] > z1 + TOL {
                    continue;
                }
                let away = norm([o[0] - n[0], o[1] - n[1], 0.0]);
                if len(away) < 0.5 {
                    continue;
                }
                let fc = frame(c.start, c.end, c.rotation);
                let (dv, du) = (dot(away, fc.v), dot(away, fc.u));
                let face = if dv.abs() > 0.9999 {
                    "flange"
                } else if du.abs() > 0.9999 {
                    "web"
                } else {
                    "skew"
                };
                out.push(Joint { beam: b.id.clone(), end, column: c.id.clone(), face, away });
                break;
            }
        }
    }
    out
}
/// `similarJoints(state, conn)` (conn: `{ beam, end }`).
pub fn similar_joints(ms: &[Mem], cs: &[Conn], beam: Option<&str>, end: &str) -> Vec<Joint> {
    let by: HashMap<&str, &Mem> = ms.iter().map(|m| (m.id.as_str(), m)).collect();
    let joints = find_joints(ms);
    let Some(src) = joints.iter().find(|j| Some(j.beam.as_str()) == beam && j.end == end) else { return vec![] };
    let key = |j: &Joint| {
        let p = |id: &str| by.get(id).map_or_else(String::new, |m| crate::js::upper(&m.profile));
        format!("{}|{}|{}", p(&j.beam), p(&j.column), j.face)
    };
    let k = key(src);
    let taken: Vec<String> =
        cs.iter().filter(|c| c.is_beam_end()).map(|c| format!("{}:{}", c.beam.as_deref().unwrap_or("undefined"), c.end)).collect();
    joints.iter().filter(|j| key(j) == k && !taken.contains(&format!("{}:{}", j.beam, j.end))).cloned().collect()
}
/// `freeSplices(state, ids)`.
pub fn free_splices(ms: &[Mem], cs: &[Conn], ids: Option<&[String]>) -> Vec<SpliceJoint> {
    let by: HashMap<&str, &Mem> = ms.iter().map(|m| (m.id.as_str(), m)).collect();
    let mut taken: Vec<String> = Vec::new();
    for c in cs.iter().filter(|c| c.kind == "splice") {
        let (a, b) = (c.a.as_deref().unwrap_or("undefined"), c.b.as_deref().unwrap_or("undefined"));
        taken.push(format!("{a}|{b}"));
        taken.push(format!("{b}|{a}"));
    }
    let mut seen: Vec<String> = Vec::new();
    find_splices(ms, None)
        .into_iter()
        .filter(|j| {
            if ids.is_some_and(|ids| !ids.contains(&j.a) && !ids.contains(&j.b)) {
                return false;
            }
            let (Some(a), Some(b)) = (by.get(j.a.as_str()), by.get(j.b.as_str())) else { return false };
            if crate::js::upper(&a.profile) != crate::js::upper(&b.profile) || taken.contains(&format!("{}|{}", j.a, j.b)) {
                return false;
            }
            let mut pair = [j.a.clone(), j.b.clone()];
            pair.sort_by(|x, y| crate::js::utf16_cmp(x, y));
            let k = pair.join("|");
            if seen.contains(&k) {
                return false;
            }
            seen.push(k);
            true
        })
        .collect()
}
/// `similarSplices(state, conn)` (conn: `{ a }`).
pub fn similar_splices(ms: &[Mem], cs: &[Conn], a: Option<&str>) -> Vec<SpliceJoint> {
    let Some(am) = ms.iter().find(|m| Some(m.id.as_str()) == a) else { return vec![] };
    let pa = crate::js::upper(&am.profile);
    free_splices(ms, cs, None)
        .into_iter()
        .filter(|j| ms.iter().find(|m| m.id == j.a).is_some_and(|m| crate::js::upper(&m.profile) == pa))
        .collect()
}
/// `similarColumns(state, conn)` (conn: `{ column }`).
pub fn similar_columns(ms: &[Mem], cs: &[Conn], column: Option<&str>) -> Vec<String> {
    let Some(src) = ms.iter().find(|m| Some(m.id.as_str()) == column) else { return vec![] };
    let taken: Vec<&str> = cs.iter().filter(|c| c.kind == "baseplate").filter_map(|c| c.column.as_deref()).collect();
    let cols: Vec<&Mem> = ms.iter().filter(|m| m.kind == "column" && is_vertical(m)).collect();
    let bottom_of = |m: &Mem| if m.start[2] <= m.end[2] { m.start } else { m.end };
    let top_of = |m: &Mem| if m.start[2] <= m.end[2] { m.end } else { m.start };
    let stands_on = |m: &Mem| {
        cols.iter().any(|o| {
            !std::ptr::eq(*o, m)
                && (top_of(o)[0] - bottom_of(m)[0]).abs() < TOL
                && (top_of(o)[1] - bottom_of(m)[1]).abs() < TOL
                && (top_of(o)[2] - bottom_of(m)[2]).abs() < TOL
        })
    };
    let pu = crate::js::upper(&src.profile);
    cols.iter()
        .filter(|m| m.id != src.id && !taken.contains(&m.id.as_str()) && crate::js::upper(&m.profile) == pu && !stands_on(m))
        .map(|m| m.id.clone())
        .collect()
}

fn ceil5(v: f64) -> f64 {
    (v / 5.0 - 1e-9).ceil() * 5.0
}
fn pl(tk: f64, w: f64) -> String {
    format!("PL{}x{}", num1(tk), num1(w))
}
fn deg(a: f64) -> f64 {
    a * 180.0 / PI
}

// ---------------------------------------------------------------------------------------------------------------
// builders

/// `buildEndPlate(conn, ctx)`.
pub fn build_end_plate(conn: &Conn, parts: &Drafts) -> Made {
    let (Some(bb), Some(cc)) = (parts.get(conn.beam.as_deref()), parts.get(conn.column.as_deref())) else {
        return fail(vec![t0("conn.err.part-missing")]);
    };
    let p = |k: &str| conn.p(k);
    if bb.profile.kind != "I" {
        return fail(vec![t0("conn.err.ep-beam-i")]);
    }
    if cc.profile.kind != "I" {
        return fail(vec![t0("conn.err.ep-column-i")]);
    }
    if !is_vertical(&cc.m) {
        return fail(vec![t0("conn.err.column-not-vertical")]);
    }
    let n = bb.m.at(&conn.end);
    let o = if conn.end == "start" { bb.m.end } else { bb.m.start };
    let z0 = min(cc.m.start[2], cc.m.end[2]);
    let z1 = max(cc.m.start[2], cc.m.end[2]);
    if (n[0] - cc.m.start[0]).abs() > TOL || (n[1] - cc.m.start[1]).abs() > TOL || n[2] < z0 - TOL || n[2] > z1 + TOL {
        return fail(vec![t0("conn.err.beam-off-column")]);
    }
    if (n[2] - o[2]).abs() > TOL {
        return fail(vec![t0("conn.err.ep-level-only")]);
    }
    if (bb.frame.v[2] - 1.0).abs() > 1e-6 {
        return fail(vec![t0("conn.err.ep-upright")]);
    }
    let away = norm([o[0] - n[0], o[1] - n[1], 0.0]);
    let fc = &cc.frame;
    let dv = dot(away, fc.v);
    let du = dot(away, fc.u);
    let on_web = du.abs() > 0.9999;
    if dv.abs() < 0.9999 && !on_web {
        return fail(vec![t0("conn.err.skew")]);
    }
    let mut problems = Vec::new();
    let (bp, cp) = (&bb.profile, &cc.profile);
    let face_off = if on_web { cp.tw() / 2.0 } else { cp.d() / 2.0 };
    let beam_top = n[2] + bb.voff + bp.d() / 2.0;
    let hp = bp.d() + p("extTop") + p("extBottom");
    let z_bot = beam_top - bp.d() - p("extBottom");
    let z_top = beam_top + p("extTop");
    let ctr = add(n, mul(away, face_off + p("tp") / 2.0));
    let rot = deg(atan2(away[1], away[0]));
    let plate = PlateDraft {
        key: format!("{}.plate", conn.id),
        role: t0("conn.role.endplate"),
        profile_name: pl(p("tp"), p("width")),
        material: conn.ps("plateGrade", "A36"),
        start: [ctr[0], ctr[1], z_bot],
        end: [ctr[0], ctr[1], z_top],
        rotation: rot,
        attach_to: conn.beam.clone(),
        loose: false,
    };
    let w = norm([away[1], -away[0], 0.0]);
    let zc = (z_bot + z_top) / 2.0;
    let rows = max(1.0, round(p("rows")));
    let (mut holes, mut bolts) = (Vec::new(), Vec::new());
    let face_name = if on_web {
        "web"
    } else if dv > 0.0 {
        "top"
    } else {
        "bottom"
    };
    let flange_face = add(n, mul(away, face_off));
    let grip = p("tp") + if on_web { cp.tw() } else { cp.tf() };
    let bolt_len = ceil5(grip + p("allowance"));
    let col = conn.column.clone().unwrap_or_default();
    let mut i = 0.0;
    while i < rows && i < MAX_N {
        let z = zc + ((rows - 1.0) / 2.0 - i) * p("pitch");
        for sgn in [1.0, -1.0] {
            let on_face = [flange_face[0] + w[0] * sgn * p("gauge") / 2.0, flange_face[1] + w[1] * sgn * p("gauge") / 2.0, z];
            holes.push(WHole { target: plate.key.clone(), world: add(on_face, mul(away, p("tp") / 2.0)), face: "plate", d: p("hole") });
            holes.push(WHole { target: col.clone(), world: on_face, face: face_name, d: p("hole") });
            bolts.push(Bolt {
                conn: Some(conn.id.clone()),
                loose: None,
                d: p("boltD"),
                length: bolt_len,
                grip,
                grade: conn.pv("grade"),
                head: add(on_face, mul(away, p("tp"))),
                axis: mul(away, -1.0),
                kind: "BOLT".into(),
                phase: f64::NAN,
            });
        }
        i += 1.0;
    }
    let d0 = p("hole");
    let e1 = (hp - (rows - 1.0) * p("pitch")) / 2.0;
    let e2 = (p("width") - p("gauge")) / 2.0;
    if e1 < 1.2 * d0 {
        problems.push(t("conn.warn.e1", &[("e", fs(e1)), ("min", fs(1.2 * d0))]));
    }
    if e2 < 1.2 * d0 {
        problems.push(t("conn.warn.e2", &[("e", fs(e2)), ("min", fs(1.2 * d0))]));
    }
    if rows > 1.0 && p("pitch") < 2.2 * d0 {
        problems.push(t("conn.warn.pitch", &[("pitch", fs(p("pitch"))), ("min", fs(2.2 * d0))]));
    }
    if p("gauge") < 2.4 * d0 {
        problems.push(t("conn.warn.gauge", &[("gauge", fs(p("gauge"))), ("min", fs(2.4 * d0))]));
    }
    if on_web {
        let clear = cp.d() - 2.0 * cp.tf() - 2.0 * cp.r();
        if p("width") > clear + 1e-6 {
            problems.push(t("conn.warn.web-plate-wide", &[("w", fs(p("width"))), ("clear", fs(clear))]));
        }
        if bp.bf() > cp.d() - 2.0 * cp.tf() + 1e-6 {
            problems.push(t("conn.warn.web-beam-wide", &[("bf", fs(bp.bf())), ("clear", fs(cp.d() - 2.0 * cp.tf()))]));
        }
    } else {
        if p("gauge") / 2.0 - d0 / 2.0 < cp.tw() / 2.0 + cp.r() {
            problems.push(t0("conn.warn.holes-web"));
        }
        if p("gauge") / 2.0 + d0 / 2.0 > cp.bf() / 2.0 {
            problems.push(t0("conn.warn.holes-off-flange"));
        }
        if p("width") > cp.bf() + 1e-6 {
            problems.push(t0("conn.warn.plate-wide"));
        }
    }
    let mut i = 0.0;
    while i < rows && i < MAX_N {
        let z = zc + ((rows - 1.0) / 2.0 - i) * p("pitch");
        let in_beam = z > beam_top - bp.d() && z < beam_top;
        if in_beam && ((z - (beam_top - bp.d() / 2.0)).abs() + d0 / 2.0 > bp.d() / 2.0 - bp.tf() - p("weld")) {
            problems.push(t("conn.warn.row-near-flange", &[("row", F::N(i + 1.0))]));
        }
        if in_beam && p("gauge") / 2.0 - d0 / 2.0 < bp.tw() / 2.0 + p("weld") {
            problems.push(t0("conn.warn.gauge-tight"));
        }
        i += 1.0;
    }
    let beam = conn.beam.clone().unwrap_or_default();
    Made {
        built: true,
        setbacks: vec![Setback { member: beam.clone(), end: conn.end.clone(), by: face_off + p("tp") }],
        welds: vec![Weld {
            conn: Some(conn.id.clone()),
            stair: None,
            kind: "fillet",
            size: p("weld"),
            between: [beam, plate.key.clone()],
            length: bp.perimeter,
            r#where: t0("conn.weld.beam-end"),
            all_round: true,
        }],
        plates: vec![plate],
        holes,
        bolts,
        problems: uniq(problems),
        info: None,
    }
}

/// `buildShearTab(conn, ctx)`.
pub fn build_shear_tab(conn: &Conn, parts: &Drafts) -> Made {
    let (Some(bb), Some(cc)) = (parts.get(conn.beam.as_deref()), parts.get(conn.column.as_deref())) else {
        return fail(vec![t0("conn.err.part-missing")]);
    };
    let p = |k: &str| conn.p(k);
    if bb.profile.kind != "I" && bb.profile.kind != "C" {
        return fail(vec![t0("conn.err.st-beam")]);
    }
    if cc.profile.kind != "I" {
        return fail(vec![t0("conn.err.st-column-i")]);
    }
    if !is_vertical(&cc.m) {
        return fail(vec![t0("conn.err.column-not-vertical")]);
    }
    let n = bb.m.at(&conn.end);
    let o = if conn.end == "start" { bb.m.end } else { bb.m.start };
    let z0 = min(cc.m.start[2], cc.m.end[2]);
    let z1 = max(cc.m.start[2], cc.m.end[2]);
    if (n[0] - cc.m.start[0]).abs() > TOL || (n[1] - cc.m.start[1]).abs() > TOL || n[2] < z0 - TOL || n[2] > z1 + TOL {
        return fail(vec![t0("conn.err.beam-off-column")]);
    }
    let dir = norm(sub(o, n));
    let away_h = norm([dir[0], dir[1], 0.0]);
    if len(away_h) < 0.5 {
        return fail(vec![t0("conn.err.st-vertical-beam")]);
    }
    let (fc, bp, cp) = (&cc.frame, &bb.profile, &cc.profile);
    let dv = dot(away_h, fc.v);
    let du = dot(away_h, fc.u);
    let on_web = du.abs() > dv.abs();
    let nn = if on_web { mul(fc.u, sign(du)) } else { mul(fc.v, sign(dv)) };
    let face_off = if on_web { cp.tw() / 2.0 } else { cp.d() / 2.0 };
    let cos_a = dot(dir, nn);
    if cos_a < 0.5 {
        return fail(vec![t0("conn.err.st-too-skewed")]);
    }
    let mut problems = Vec::new();
    let bf = &bb.frame;
    let (uu, vv) = (bf.u, bf.v);
    let (w2, d2) = (bp.width / 2.0, bp.depth / 2.0);
    let mut setback = 0.0;
    for [u, v] in [[-w2, -d2], [w2, -d2], [w2, d2], [-w2, d2]] {
        let off = dot(add(mul(uu, u), mul(vv, v + bb.voff)), nn);
        setback = max(setback, (face_off + p("gap") - off) / cos_a);
    }
    let at = |s: f64, u: f64, v: f64| add(n, add(mul(dir, s), add(mul(uu, u), mul(vv, v + bb.voff))));
    let u_web = if bp.kind == "C" { -bp.bf() / 2.0 + bp.tw() } else { bp.tw() / 2.0 };
    let u_mid = u_web + p("tp") / 2.0;
    let mut s_face = 0.0;
    for u in [u_web, u_web + p("tp")] {
        s_face = max(s_face, (face_off - dot(mul(uu, u), nn)) / cos_a);
    }
    let rows = max(1.0, round(p("rows")));
    let vc = p("ev");
    let len_ = (rows - 1.0) * p("pitch") + 2.0 * p("edge");
    let s_mid = s_face + p("width") / 2.0;
    let s_bolt = s_face + p("eh");
    let c0 = at(s_mid, u_mid, vc - len_ / 2.0);
    let c1 = at(s_mid, u_mid, vc + len_ / 2.0);
    let f0 = frame(c0, c1, 0.0);
    let rot = deg(atan2(-dot(uu, f0.u), dot(uu, f0.v)));
    let plate = PlateDraft {
        key: format!("{}.plate", conn.id),
        role: t0("conn.role.sheartab"),
        profile_name: pl(p("tp"), p("width")),
        material: conn.ps("plateGrade", "A36"),
        start: c0,
        end: c1,
        rotation: rot,
        attach_to: conn.column.clone(),
        loose: false,
    };
    let (mut holes, mut bolts) = (Vec::new(), Vec::new());
    let grip = p("tp") + bp.tw();
    let bolt_len = ceil5(grip + p("allowance"));
    let beam = conn.beam.clone().unwrap_or_default();
    let mut i = 0.0;
    while i < rows && i < MAX_N {
        let v = vc + ((rows - 1.0) / 2.0 - i) * p("pitch");
        holes.push(WHole { target: beam.clone(), world: at(s_bolt, u_web - bp.tw() / 2.0, v), face: "web", d: p("hole") });
        holes.push(WHole { target: plate.key.clone(), world: at(s_bolt, u_mid, v), face: "plate", d: p("hole") });
        bolts.push(Bolt {
            conn: Some(conn.id.clone()),
            loose: None,
            d: p("boltD"),
            length: bolt_len,
            grip,
            grade: conn.pv("grade"),
            head: at(s_bolt, u_web + p("tp"), v),
            axis: mul(uu, -1.0),
            kind: "BOLT".into(),
            phase: f64::NAN,
        });
        i += 1.0;
    }
    let d0 = p("hole");
    if p("edge") < 1.2 * d0 {
        problems.push(t("conn.warn.e1", &[("e", fs(p("edge"))), ("min", fs(1.2 * d0))]));
    }
    if p("width") - p("eh") < 1.2 * d0 {
        problems.push(t("conn.warn.st-e2", &[("e", fs(p("width") - p("eh"))), ("min", fs(1.2 * d0))]));
    }
    if rows > 1.0 && p("pitch") < 2.2 * d0 {
        problems.push(t("conn.warn.pitch", &[("pitch", fs(p("pitch"))), ("min", fs(2.2 * d0))]));
    }
    let clear_depth = bp.depth - 2.0 * (bp.tf() + bp.r_or0());
    if vc.abs() + len_ / 2.0 > clear_depth / 2.0 + 1e-6 {
        problems.push(t("conn.warn.st-tab-deep", &[("l", fs(len_)), ("clear", fs(clear_depth))]));
    }
    if p("eh") - setback + s_face < 1.5 * d0 {
        problems.push(t("conn.warn.st-beam-edge", &[("e", fs(p("eh") - (setback - s_face)))]));
    }
    if on_web && bp.bf() > cp.d() - 2.0 * cp.tf() + 1e-6 {
        problems.push(t("conn.warn.web-beam-wide", &[("bf", fs(bp.bf())), ("clear", fs(cp.d() - 2.0 * cp.tf()))]));
    }
    if !on_web && p("tp") > cp.bf() {
        problems.push(t0("conn.warn.plate-wide"));
    }
    let skew = deg(acos(min(1.0, dot(away_h, nn))));
    let slope = deg(asin(min(1.0, dir[2].abs())));
    let colid = conn.column.clone().unwrap_or_default();
    Made {
        built: true,
        setbacks: vec![Setback { member: beam, end: conn.end.clone(), by: setback }],
        welds: vec![Weld {
            conn: Some(conn.id.clone()),
            stair: None,
            kind: "fillet",
            size: p("weld"),
            between: [colid, plate.key.clone()],
            length: 2.0 * len_,
            r#where: t0("conn.weld.tab"),
            all_round: false,
        }],
        plates: vec![plate],
        holes,
        bolts,
        problems: uniq(problems),
        info: Some(Obj::new().set("face", if on_web { "web" } else { "flange" }).set("skew", skew).set("slope", slope).done()),
    }
}

/// `buildBasePlate(conn, ctx)`.
pub fn build_base_plate(conn: &Conn, parts: &Drafts) -> Made {
    let Some(cc) = parts.get(conn.column.as_deref()) else { return fail(vec![t0("conn.err.column-missing")]) };
    let p = |k: &str| conn.p(k);
    if !is_vertical(&cc.m) {
        return fail(vec![t0("conn.err.bp-vertical")]);
    }
    let bottom_end = if cc.m.start[2] <= cc.m.end[2] { "start" } else { "end" };
    let bn = cc.m.at(bottom_end);
    let fc = &cc.frame;
    let cv = norm([fc.v[0], fc.v[1], 0.0]);
    let cu = norm([fc.u[0], fc.u[1], 0.0]);
    let zc = bn[2] + p("tp") / 2.0;
    let s = add(add(bn, mul(cv, -p("length") / 2.0)), [0.0, 0.0, p("tp") / 2.0]);
    let e = add(add(bn, mul(cv, p("length") / 2.0)), [0.0, 0.0, p("tp") / 2.0]);
    let plate_grade = conn.ps("plateGrade", "A36");
    let col = conn.column.clone().unwrap_or_default();
    let plate = PlateDraft {
        key: format!("{}.plate", conn.id),
        role: t0("conn.role.baseplate"),
        profile_name: pl(p("tp"), p("width")),
        material: plate_grade.clone(),
        start: [s[0], s[1], zc],
        end: [e[0], e[1], zc],
        rotation: 0.0,
        attach_to: conn.column.clone(),
        loose: false,
    };
    let (mut holes, mut bolts) = (Vec::new(), Vec::new());
    let grout = crate::js::or(p("grout"), 0.0);
    let rod_len = p("embed") + grout + p("tp") + p("projection");
    for a in [-1.0, 1.0] {
        for b in [-1.0, 1.0] {
            let pt = add(add(bn, mul(cv, a * p("sx") / 2.0)), mul(cu, b * p("sy") / 2.0));
            holes.push(WHole { target: plate.key.clone(), world: [pt[0], pt[1], zc], face: "plate", d: p("hole") });
            bolts.push(Bolt {
                conn: Some(conn.id.clone()),
                loose: None,
                d: p("rodD"),
                length: rod_len,
                grip: p("tp"),
                grade: conn.pv("grade"),
                head: [pt[0], pt[1], bn[2] + p("tp") + p("projection")],
                axis: [0.0, 0.0, -1.0],
                kind: "ANCHORBOLT".into(),
                phase: f64::NAN,
            });
        }
    }
    let mut problems = Vec::new();
    let cp = &cc.profile;
    let d0 = p("hole");
    let mut plates = vec![plate.clone()];
    let mut welds = vec![Weld {
        conn: Some(conn.id.clone()),
        stair: None,
        kind: "fillet",
        size: p("weld"),
        between: [col.clone(), plate.key.clone()],
        length: cp.perimeter,
        r#where: t0("conn.weld.column-base"),
        all_round: true,
    }];
    let top = bn[2] + p("tp");
    let stiff = p("stiffeners");
    if stiff == 2.0 || stiff == 4.0 {
        let hd = cp.depth / 2.0;
        let hw = if cp.kind == "I" || cp.kind == "C" { cp.tw() / 2.0 } else { cp.width / 2.0 };
        let mut runs = vec![(cv, hd, p("length") / 2.0), (mul(cv, -1.0), hd, p("length") / 2.0)];
        if stiff == 4.0 {
            runs.push((cu, hw, p("width") / 2.0));
            runs.push((mul(cu, -1.0), hw, p("width") / 2.0));
        }
        for (i, (dir, from, to)) in runs.into_iter().enumerate() {
            if to - from < 20.0 {
                problems.push(t0("conn.warn.stiff-room"));
                continue;
            }
            let a = add(bn, mul(dir, from));
            let b = add(bn, mul(dir, to));
            let z = top + p("stiffH") / 2.0;
            let st = PlateDraft {
                key: format!("{}.st{}", conn.id, i.saturating_add(1)),
                role: t0("conn.role.stiffener"),
                profile_name: pl(p("stiffT"), p("stiffH")),
                material: plate_grade.clone(),
                start: [a[0], a[1], z],
                end: [b[0], b[1], z],
                rotation: 90.0,
                attach_to: conn.column.clone(),
                loose: false,
            };
            welds.push(Weld {
                conn: Some(conn.id.clone()),
                stair: None,
                kind: "fillet",
                size: p("weld"),
                between: [col.clone(), st.key.clone()],
                length: 2.0 * (p("stiffH") + (to - from)),
                r#where: t0("conn.weld.stiffener"),
                all_round: false,
            });
            plates.push(st);
        }
    } else if stiff != 0.0 && !stiff.is_nan() {
        problems.push(t0("conn.warn.stiff-count"));
    }
    if p("lugD") > 0.0 {
        let a = add(bn, mul(cv, -p("lugW") / 2.0));
        let b = add(bn, mul(cv, p("lugW") / 2.0));
        let z = bn[2] - p("lugD") / 2.0;
        let lug = PlateDraft {
            key: format!("{}.lug", conn.id),
            role: t0("conn.role.lug"),
            profile_name: pl(p("lugT"), p("lugD")),
            material: plate_grade.clone(),
            start: [a[0], a[1], z],
            end: [b[0], b[1], z],
            rotation: 90.0,
            attach_to: conn.column.clone(),
            loose: false,
        };
        welds.push(Weld {
            conn: Some(conn.id.clone()),
            stair: None,
            kind: "fillet",
            size: p("weld"),
            between: [col.clone(), lug.key.clone()],
            length: 2.0 * (p("lugW") + p("lugT")),
            r#where: t0("conn.weld.lug"),
            all_round: true,
        });
        plates.push(lug);
        if p("lugD") <= grout + 1e-6 {
            problems.push(t("conn.warn.lug-grout", &[("d", fs(p("lugD"))), ("g", fs(grout))]));
        }
        if p("lugW") > p("length") || p("lugT") > p("width") {
            problems.push(t0("conn.warn.lug-big"));
        }
    }
    let washers = p("washers");
    if washers != 0.0 && !washers.is_nan() {
        let row = dg1_row(p("rodD"));
        let ws = crate::js::or(p("washerSize"), row.map_or(f64::NAN, |r| r.washer));
        let wt = crate::js::or(p("washerT"), row.map_or(f64::NAN, |r| r.washer_t));
        if ws == 0.0 || ws.is_nan() || wt == 0.0 || wt.is_nan() {
            problems.push(t0("conn.warn.washer-size"));
        } else {
            let heads: Vec<V3> = bolts.iter().map(|b| b.head).collect();
            for (i, head) in heads.into_iter().enumerate() {
                let c0 = [head[0], head[1]];
                let z = top + wt / 2.0;
                let w = PlateDraft {
                    key: format!("{}.w{}", conn.id, i.saturating_add(1)),
                    role: t0("conn.role.washer"),
                    profile_name: pl(wt, ws),
                    material: plate_grade.clone(),
                    start: [c0[0] - cv[0] * ws / 2.0, c0[1] - cv[1] * ws / 2.0, z],
                    end: [c0[0] + cv[0] * ws / 2.0, c0[1] + cv[1] * ws / 2.0, z],
                    rotation: 0.0,
                    attach_to: conn.column.clone(),
                    loose: true,
                };
                holes.push(WHole { target: w.key.clone(), world: [c0[0], c0[1], z], face: "plate", d: p("rodD") + 25.4 / 16.0 });
                plates.push(w);
            }
            let ws_set = p("washerSize");
            if row.is_some() && (ws_set == 0.0 || ws_set.is_nan()) && ws < p("hole") + 1e-6 {
                problems.push(t0("conn.warn.washer-hole"));
            }
            if p("projection") < wt + p("rodD") {
                problems.push(t("conn.warn.washer-projection", &[("min", fs(wt + p("rodD")))]));
            }
        }
    }
    if (p("length") - p("sx")) / 2.0 < 1.2 * d0 || (p("width") - p("sy")) / 2.0 < 1.2 * d0 {
        problems.push(t("conn.warn.rod-edge", &[("min", fs(1.2 * d0))]));
    }
    if p("sx") / 2.0 - d0 / 2.0 < cp.depth / 2.0 && p("sy") / 2.0 - d0 / 2.0 < cp.width / 2.0 {
        problems.push(t0("conn.warn.rods-inside"));
    }
    if p("length") < cp.depth || p("width") < cp.width {
        problems.push(t0("conn.warn.plate-small"));
    }
    Made {
        built: true,
        setbacks: vec![Setback { member: col, end: bottom_end.into(), by: p("tp") }],
        plates,
        holes,
        bolts,
        welds,
        problems: uniq(problems),
        info: None,
    }
}

/// `buildSplice(conn, ctx)`.
pub fn build_splice(conn: &Conn, parts: &Drafts) -> Made {
    let (Some(aa), Some(bb)) = (parts.get(conn.a.as_deref()), parts.get(conn.b.as_deref())) else {
        return fail(vec![t0("conn.err.part-missing")]);
    };
    let p = |k: &str| conn.p(k);
    if aa.profile.kind != "I" || bb.profile.kind != "I" {
        return fail(vec![t0("conn.err.sp-i")]);
    }
    let (ap, bpf) = (&aa.profile, &bb.profile);
    if (ap.d() - bpf.d()).abs() > 1.0 || (ap.bf() - bpf.bf()).abs() > 1.0 {
        return fail(vec![t0("conn.err.sp-size")]);
    }
    let j = aa.m.at(&conn.a_end);
    let ax = aa.frame.x;
    let out_ = if conn.a_end == "end" { ax } else { mul(ax, -1.0) };
    let b_ax = bb.frame.x;
    if dot(ax, b_ax).abs() < 0.9999 {
        return fail(vec![t0("conn.err.sp-line")]);
    }
    let b_near = if dot(sub(bb.m.start, j), out_) < dot(sub(bb.m.end, j), out_) { "start" } else { "end" };
    let dj = sub(bb.m.at(b_near), j);
    if len(sub(dj, mul(ax, dot(dj, ax)))) > TOL || len(dj) > 50.0 {
        return fail(vec![t0("conn.err.sp-line")]);
    }
    let mut problems = Vec::new();
    if dot(aa.frame.v, bb.frame.v) < 0.9999 || (aa.voff - bb.voff).abs() > 0.5 {
        problems.push(t0("conn.warn.sp-turned"));
    }
    let (uu, vv) = (aa.frame.u, aa.frame.v);
    let mid = add(j, mul(out_, dot(dj, out_) / 2.0));
    let wp = |s: f64, u: f64, v: f64| add(mid, add(mul(out_, s), add(mul(uu, u), mul(vv, v + aa.voff))));
    let d2 = ap.d() / 2.0;
    let gap = p("jointGap");
    let a_id = conn.a.clone().unwrap_or_default();
    let b_id = conn.b.clone().unwrap_or_default();
    let setbacks = vec![
        Setback { member: a_id.clone(), end: conn.a_end.clone(), by: max(0.0, gap / 2.0 - dot(dj, out_) / 2.0) },
        Setback { member: b_id.clone(), end: b_near.into(), by: max(0.0, gap / 2.0 - dot(dj, out_) / 2.0) },
    ];
    let (mut plates, mut holes, mut bolts) = (Vec::new(), Vec::new(), Vec::new());
    let plate_grade = conn.ps("plateGrade", "A36");
    let plate_along =
        |plates: &mut Vec<PlateDraft>, key: &str, role: String, t_: f64, width: f64, length: f64, u: f64, v: f64, wdir: V3| -> String {
            let c0 = wp(-length / 2.0, u, v);
            let c1 = wp(length / 2.0, u, v);
            let f0 = frame(c0, c1, 0.0);
            let rot = deg(atan2(dot(wdir, f0.v), dot(wdir, f0.u)));
            let k = format!("{}.{}", conn.id, key);
            plates.push(PlateDraft {
                key: k.clone(),
                role,
                profile_name: pl(t_, width),
                material: plate_grade.clone(),
                start: c0,
                end: c1,
                rotation: rot,
                attach_to: conn.a.clone(),
                loose: false,
            });
            k
        };
    let face_of = |part: &Draft, normal: V3| if dot(normal, part.frame.v) > 0.0 { "top" } else { "bottom" };
    let n_f = max(1.0, round(p("flangeRows")));
    let lf = gap + 2.0 * (2.0 * p("flangeEdge") + (n_f - 1.0) * p("flangePitch"));
    let g_f = crate::js::or(p("flangeGauge"), ap.bf() - 80.0);
    for sg in [1.0, -1.0] {
        let key = plate_along(
            &mut plates,
            if sg > 0.0 { "ft" } else { "fb" },
            t0(if sg > 0.0 { "conn.role.sp-flange-top" } else { "conn.role.sp-flange-bottom" }),
            p("flangeT"),
            ap.bf(),
            lf,
            0.0,
            sg * (d2 + p("flangeT") / 2.0),
            uu,
        );
        let grip = ap.tf() + p("flangeT");
        let bl = ceil5(grip + p("allowance"));
        for side in [-1.0, 1.0] {
            let mut i = 0.0;
            while i < n_f && i < MAX_N {
                for gu in [-g_f / 2.0, g_f / 2.0] {
                    let sx = side * (gap / 2.0 + p("flangeEdge") + i * p("flangePitch"));
                    let (member_id, part) = if side < 0.0 { (&a_id, aa) } else { (&b_id, bb) };
                    holes.push(WHole {
                        target: member_id.clone(),
                        world: wp(sx, gu, sg * (d2 - ap.tf() / 2.0)),
                        face: face_of(part, mul(vv, sg)),
                        d: p("hole"),
                    });
                    holes.push(WHole {
                        target: key.clone(),
                        world: wp(sx, gu, sg * (d2 + p("flangeT") / 2.0)),
                        face: "plate",
                        d: p("hole"),
                    });
                    bolts.push(Bolt {
                        conn: Some(conn.id.clone()),
                        loose: None,
                        d: p("boltD"),
                        length: bl,
                        grip,
                        grade: conn.pv("grade"),
                        head: wp(sx, gu, sg * (d2 + p("flangeT"))),
                        axis: mul(vv, -sg),
                        kind: "BOLT".into(),
                        phase: f64::NAN,
                    });
                }
                i += 1.0;
            }
        }
    }
    let clear = ap.d() - 2.0 * (ap.tf() + ap.r_or0());
    let n_w = max(1.0, round(p("webRows")));
    let c_w = max(1.0, round(p("webCols")));
    // Spec gap: the JS has no limit on webRows × webCols (each up to 10 000 in a file: 10⁸ bolts, which no page
    // survives); past 100 000 web bolts the splice is refused here instead of exhausting memory.
    if n_w * c_w > 100_000.0 {
        return fail(vec![t0("conn.warn.sp-web-rows")]);
    }
    let hw = crate::js::or(p("webH"), ((clear - 20.0) / 5.0).floor() * 5.0);
    let lw = gap + 2.0 * (2.0 * p("webEdge") + (c_w - 1.0) * p("webPitchX"));
    let mut wps = Vec::new();
    for sg in [1.0, -1.0] {
        wps.push(plate_along(
            &mut plates,
            if sg > 0.0 { "w1" } else { "w2" },
            t0("conn.role.sp-web"),
            p("webT"),
            hw,
            lw,
            sg * (ap.tw() / 2.0 + p("webT") / 2.0),
            0.0,
            vv,
        ));
    }
    let grip_w = ap.tw() + 2.0 * p("webT");
    let bl_w = ceil5(grip_w + p("allowance"));
    for side in [-1.0, 1.0] {
        let mut jj = 0.0;
        while jj < c_w && jj < MAX_N {
            let mut i = 0.0;
            while i < n_w && i < MAX_N {
                let sx = side * (gap / 2.0 + p("webEdge") + jj * p("webPitchX"));
                let v = ((n_w - 1.0) / 2.0 - i) * p("webPitch");
                holes.push(WHole {
                    target: if side < 0.0 { a_id.clone() } else { b_id.clone() },
                    world: wp(sx, 0.0, v),
                    face: "web",
                    d: p("hole"),
                });
                for (k, key) in wps.iter().enumerate() {
                    let sgn = if k == 0 { 1.0 } else { -1.0 };
                    holes.push(WHole {
                        target: key.clone(),
                        world: wp(sx, sgn * (ap.tw() / 2.0 + p("webT") / 2.0), v),
                        face: "plate",
                        d: p("hole"),
                    });
                }
                bolts.push(Bolt {
                    conn: Some(conn.id.clone()),
                    loose: None,
                    d: p("boltD"),
                    length: bl_w,
                    grip: grip_w,
                    grade: conn.pv("grade"),
                    head: wp(sx, ap.tw() / 2.0 + p("webT"), v),
                    axis: mul(uu, -1.0),
                    kind: "BOLT".into(),
                    phase: f64::NAN,
                });
                i += 1.0;
            }
            jj += 1.0;
        }
    }
    let d0 = p("hole");
    for (e, name) in [(p("flangeEdge"), "flangeEdge"), (p("webEdge"), "webEdge")] {
        if e < 1.2 * d0 {
            problems.push(t("conn.warn.sp-edge", &[("name", F::S(t0(param_label_key(name)))), ("e", fs(e)), ("min", fs(1.2 * d0))]));
        }
    }
    if n_f > 1.0 && p("flangePitch") < 2.2 * d0 {
        problems.push(t("conn.warn.pitch", &[("pitch", fs(p("flangePitch"))), ("min", fs(2.2 * d0))]));
    }
    if n_w > 1.0 && p("webPitch") < 2.4 * d0 {
        problems.push(t("conn.warn.pitch", &[("pitch", fs(p("webPitch"))), ("min", fs(2.4 * d0))]));
    }
    if g_f < 2.4 * d0 {
        problems.push(t("conn.warn.sp-gauge", &[("g", fs(g_f)), ("min", fs(2.4 * d0))]));
    }
    if (ap.bf() - g_f) / 2.0 < 1.2 * d0 {
        problems.push(t("conn.warn.sp-gauge-edge", &[("e", fs((ap.bf() - g_f) / 2.0)), ("min", fs(1.2 * d0))]));
    }
    if g_f / 2.0 - d0 / 2.0 < ap.tw() / 2.0 + ap.r_or0() {
        problems.push(t0("conn.warn.sp-gauge-web"));
    }
    if hw > clear + 1e-6 {
        problems.push(t("conn.warn.sp-web-deep", &[("h", fs(hw)), ("clear", fs(clear))]));
    }
    if (n_w - 1.0) * p("webPitch") + 2.0 * 1.2 * d0 > hw + 1e-6 {
        problems.push(t0("conn.warn.sp-web-rows"));
    }
    Made { built: true, setbacks, plates, holes, bolts, welds: vec![], problems: uniq(problems), info: None }
}

/// `buildGusset(conn, ctx)` (`conns`: the model's connections, for a base plate under the support).
pub fn build_gusset(conn: &Conn, parts: &Drafts, conns: &[Conn]) -> Made {
    let (Some(bb), Some(ss)) = (parts.get(conn.brace.as_deref()), parts.get(conn.support.as_deref())) else {
        return fail(vec![t0("conn.err.part-missing")]);
    };
    let p = |k: &str| conn.p(k);
    let bp = &bb.profile;
    let kind = bp.kind.as_str();
    if !["L", "PL", "RHS", "CHS"].contains(&kind) {
        return fail(vec![t0("conn.err.gu-brace")]);
    }
    let n = bb.m.at(&conn.end);
    if seg_dist(n, ss.m.start, ss.m.end) > TOL {
        return fail(vec![t0("conn.err.gu-off")]);
    }
    let d = norm(sub(if conn.end == "start" { bb.m.end } else { bb.m.start }, n));
    let sx = ss.frame.x;
    let cr = [d[1] * sx[2] - d[2] * sx[1], d[2] * sx[0] - d[0] * sx[2], d[0] * sx[1] - d[1] * sx[0]];
    if len(cr) < 0.2 {
        return fail(vec![t0("conn.err.gu-along")]);
    }
    let np = norm(cr);
    let mut wv = norm([np[1] * sx[2] - np[2] * sx[1], np[2] * sx[0] - np[0] * sx[2], np[0] * sx[1] - np[1] * sx[0]]);
    if dot(wv, d) < 0.0 {
        wv = mul(wv, -1.0);
    }
    let (uu, vv, bv) = (bb.frame.u, bb.frame.v, bb.voff);
    let mut problems = Vec::new();
    let tg = p("tp");
    let bolt_mode;
    let mut o = 0.0;
    let mut hole: Option<(&'static str, f64, f64)> = None;
    let mut leg_t = 0.0;
    let at = |s_: f64, u: f64, v: f64| add(n, add(mul(d, s_), add(mul(uu, u), mul(vv, v + bv))));
    if kind == "RHS" || kind == "CHS" {
        bolt_mode = false;
    } else if kind == "PL" {
        if dot(vv, np).abs() < 0.99 {
            return fail(vec![t0("conn.err.gu-turn")]);
        }
        bolt_mode = true;
        leg_t = bp.t();
        o = dot(vv, np) * (-bp.t() / 2.0 + bv - tg / 2.0);
        hole = Some(("plate", crate::js::or(p("braceGauge"), 0.0), 0.0));
    } else {
        let leg1 = dot(vv, np).abs() > 0.99;
        let leg2 = dot(uu, np).abs() > 0.99;
        if !leg1 && !leg2 {
            return fail(vec![t0("conn.err.gu-turn")]);
        }
        bolt_mode = true;
        leg_t = bp.t();
        let g = crate::js::or(p("braceGauge"), bp.b() / 2.0);
        let h = bp.b() / 2.0;
        if leg1 {
            o = dot(vv, np) * (-h + bv - tg / 2.0);
            hole = Some(("bottom", -h + g, -h + bp.t() / 2.0));
        } else {
            o = dot(uu, np) * (-h - tg / 2.0) + dot(vv, np) * bv;
            hole = Some(("web", -h + bp.t() / 2.0, -h + g));
        }
        if bp.b() - g < 1.2 * p("hole") {
            problems.push(t("conn.warn.gu-toe", &[("e", fs(bp.b() - g)), ("min", fs(1.2 * p("hole")))]));
        }
        if g - p("hole") / 2.0 < bp.t() {
            problems.push(t0("conn.warn.gu-heel"));
        }
    }
    let (w2, h2) = (bp.width / 2.0, bp.depth / 2.0);
    let corners: Vec<V3> = [[-w2, -h2], [w2, -h2], [w2, h2], [-w2, h2]].iter().map(|&[u, v]| add(mul(uu, u), mul(vv, v + bv))).collect();
    let others: Vec<&Draft> = parts.list.iter().filter(|x| x.m.id != bb.m.id && seg_dist(n, x.m.start, x.m.end) <= TOL).collect();
    let mut setback = 0.0;
    let need = |nv: V3, off: f64| -> f64 {
        let dn = dot(d, nv);
        if dn < 0.05 {
            return f64::INFINITY;
        }
        max(0.0, max_of(corners.iter().map(|&c| (off + p("braceGap") - dot(c, nv)) / dn)))
    };
    for x in &others {
        let xa = x.frame.x;
        let nl = sub(d, mul(xa, dot(d, xa)));
        let xc: Vec<V3> = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
            .iter()
            .map(|&[a, b]| add(mul(x.frame.u, a * x.profile.width / 2.0), mul(x.frame.v, b * x.profile.depth / 2.0 + x.voff)))
            .collect();
        let mut s_x = f64::INFINITY;
        if len(nl) > 1e-6 {
            let nv = norm(nl);
            s_x = need(nv, max_of(xc.iter().map(|&c| dot(c, nv))));
        }
        for e in ["start", "end"] {
            if len(sub(x.m.at(e), n)) > TOL {
                continue;
            }
            let axv = if e == "end" { xa } else { mul(xa, -1.0) };
            s_x = min(s_x, need(axv, 0.0));
        }
        if s_x.is_finite() {
            setback = max(setback, s_x);
        }
    }
    let base = conns.iter().find(|c| c.kind == "baseplate" && c.column.as_deref() == Some(ss.m.id.as_str()));
    let s_bottom = if ss.m.start[2] <= ss.m.end[2] { "start" } else { "end" };
    let on_base = base.is_some() && len(sub(ss.m.at(s_bottom), n)) <= TOL;
    let base_tp = base.map_or(f64::NAN, |b| b.p("tp"));
    if on_base {
        setback = max(setback, need([0.0, 0.0, 1.0], base_tp));
    }
    let brace_id = conn.brace.clone().unwrap_or_default();
    let support_id = conn.support.clone().unwrap_or_default();
    let setbacks = vec![Setback { member: brace_id.clone(), end: conn.end.clone(), by: setback }];
    let gpt = |q: V3| [dot(sub(q, n), sx), dot(sub(q, n), wv)];
    let on_plane = |q: V3| add(q, mul(np, o - dot(sub(q, n), np)));
    let (mut holes, mut bolts, mut welds) = (Vec::new(), Vec::new(), Vec::new());
    let mut pts: Vec<[f64; 2]> = Vec::new();
    let gkey = format!("{}.gusset", conn.id);
    if bolt_mode {
        let (hface, hu, hv) = hole.unwrap_or(("plate", 0.0, 0.0));
        let nb = max(1.0, round(p("bolts")));
        let away = {
            let sg = sign(o - dot(at(0.0, hu, hv), np));
            if sg == 0.0 || sg.is_nan() { 1.0 } else { sg }
        };
        let grip = leg_t + tg;
        let bl = ceil5(grip + p("allowance"));
        let mut k = 0.0;
        while k < nb && k < MAX_N {
            let s_k = setback + p("braceEdge") + k * p("bracePitch");
            let pp = at(s_k, hu, hv);
            let qq = on_plane(pp);
            holes.push(WHole { target: brace_id.clone(), world: pp, face: hface, d: p("hole") });
            holes.push(WHole { target: gkey.clone(), world: qq, face: "plate", d: p("hole") });
            bolts.push(Bolt {
                conn: Some(conn.id.clone()),
                loose: None,
                d: p("boltD"),
                length: bl,
                grip,
                grade: conn.pv("grade"),
                head: add(qq, mul(np, -away * (tg / 2.0 + leg_t))),
                axis: mul(np, away),
                kind: "BOLT".into(),
                phase: f64::NAN,
            });
            pts.push(gpt(qq));
            k += 1.0;
        }
        if p("braceEdge") < 1.2 * p("hole") {
            problems.push(t("conn.warn.e1", &[("e", fs(p("braceEdge"))), ("min", fs(1.2 * p("hole")))]));
        }
        if nb > 1.0 && p("bracePitch") < 2.2 * p("hole") {
            problems.push(t("conn.warn.pitch", &[("pitch", fs(p("bracePitch"))), ("min", fs(2.2 * p("hole")))]));
        }
    } else {
        for s_ in [setback, setback + p("lap")] {
            for &c in &corners {
                pts.push(gpt(on_plane(add(add(n, mul(d, s_)), c))));
            }
        }
        welds.push(Weld {
            conn: Some(conn.id.clone()),
            stair: None,
            kind: "fillet",
            size: p("weld"),
            between: [brace_id.clone(), gkey.clone()],
            length: 4.0 * p("lap"),
            r#where: t0("conn.weld.knife"),
            all_round: false,
        });
    }
    let sc: Vec<V3> = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
        .iter()
        .map(|&[a, b]| add(mul(ss.frame.u, a * ss.profile.width / 2.0), mul(ss.frame.v, b * ss.profile.depth / 2.0 + ss.voff)))
        .collect();
    let w0 = max_of(sc.iter().map(|&c| dot(c, wv)));
    let mg = if bolt_mode { p("braceEdge") } else { 2.0 * p("weld") + 10.0 };
    let mut a0 = min_of(pts.iter().map(|q| q[0])) - mg;
    let mut a1 = max_of(pts.iter().map(|q| q[0])) + mg;
    let w1 = max_of(pts.iter().map(|q| q[1])) + mg;
    let need0 = min_of(pts.iter().map(|q| q[0]));
    let need1 = max_of(pts.iter().map(|q| q[0]));
    let s_a = [dot(sub(ss.m.start, n), sx), dot(sub(ss.m.end, n), sx)];
    let mut lo = min(s_a[0], s_a[1]);
    let mut hi = max(s_a[0], s_a[1]);
    if on_base {
        if s_bottom == "start" {
            lo = max(lo, dot(sub(ss.m.start, n), sx) + base_tp);
        } else {
            hi = min(hi, dot(sub(ss.m.end, n), sx) - base_tp);
        }
    }
    for x in &others {
        if x.m.id == ss.m.id || dot(x.frame.x, wv).abs() < 0.5 {
            continue;
        }
        let xs: Vec<f64> = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
            .iter()
            .map(|&[a, b]| {
                dot(
                    add(
                        add(sub(x.m.start, n), mul(x.frame.u, a * x.profile.width / 2.0)),
                        mul(x.frame.v, b * x.profile.depth / 2.0 + x.voff),
                    ),
                    sx,
                )
            })
            .collect();
        let xlo = min_of(xs.iter().copied());
        let xhi = max_of(xs.iter().copied());
        if need1 <= xlo {
            hi = min(hi, xlo);
        } else if need0 >= xhi {
            lo = max(lo, xhi);
        }
    }
    if on_base && s_bottom == "start" && dot(d, sx) > 0.0 {
        a0 = lo;
    }
    if on_base && s_bottom == "end" && dot(d, sx) < 0.0 {
        a1 = hi;
    }
    a0 = max(a0, lo);
    a1 = min(a1, hi);
    let room = if bolt_mode { min(p("braceEdge"), 1.2 * p("hole")) } else { p("weld") };
    if a0 > need0 - room + 1e-6 || a1 < need1 + room - 1e-6 || a1 - a0 < 20.0 {
        problems.push(t0("conn.warn.gu-fit"));
    }
    if a1 - a0 < 20.0 {
        problems.push(t0("conn.err.gu-room"));
        return fail(problems);
    }
    let wm = (w0 + w1) / 2.0;
    let qa = |a: f64| add(n, add(mul(np, o), add(mul(sx, a), mul(wv, wm))));
    let (c0, c1) = (qa(a0), qa(a1));
    let f0 = frame(c0, c1, 0.0);
    let rot = deg(atan2(dot(wv, f0.v), dot(wv, f0.u)));
    let plates = vec![PlateDraft {
        key: gkey.clone(),
        role: t0("conn.role.gusset"),
        profile_name: pl(tg, round(w1 - w0)),
        material: conn.ps("plateGrade", "A36"),
        start: c0,
        end: c1,
        rotation: rot,
        attach_to: conn.support.clone(),
        loose: false,
    }];
    welds.insert(
        0,
        Weld {
            conn: Some(conn.id.clone()),
            stair: None,
            kind: "fillet",
            size: p("weld"),
            between: [support_id, gkey],
            length: 2.0 * (a1 - a0),
            r#where: t0("conn.weld.gusset"),
            all_round: false,
        },
    );
    Made {
        built: true,
        setbacks,
        plates,
        holes,
        bolts,
        welds,
        problems: uniq(problems),
        info: Some(Obj::new().set("mode", if bolt_mode { "bolt" } else { "knife" }).set("setback", setback).done()),
    }
}
