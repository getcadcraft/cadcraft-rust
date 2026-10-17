//! build.js — the stored model made into the parts a fabricator gets: members cut back by their connections, the
//! plates, holes, bolts and welds the connections make, stair stringers and treads, footings with their bars, and the
//! problems found on the way. Nothing here is stored.

use crate::connections::{
    Bolt, Draft, Drafts, Made, PlateDraft, WHole, Weld, build_base_plate, build_end_plate, build_gusset, build_shear_tab, build_splice,
};
use crate::geom::{Frame, V3, add, frame, mul, to_local, voff_for};
use crate::i18n::{F, t, t0};
use crate::js::{Obj, Res, or, p3v};
use crate::profiles::{P, get_profile_str};
use crate::rebar::{BarGroup, Footing, Problem, footing_bars, footing_problems};
use crate::stair::{Stair, stair_geometry};
use crate::state::{Conn, Cut, Mem};
use serde_json::Value;
use std::collections::HashMap;

/// `IFC_CLASS[type]`.
pub fn ifc_class(kind: &str) -> Option<&'static str> {
    match kind {
        "beam" => Some("IfcBeam"),
        "column" => Some("IfcColumn"),
        "brace" | "stringer" => Some("IfcMember"),
        "plate" => Some("IfcPlate"),
        _ => None,
    }
}
fn role_key(kind: &str) -> Option<&'static str> {
    match kind {
        "beam" => Some("build.role.beam"),
        "column" => Some("build.role.column"),
        "brace" => Some("build.role.brace"),
        "plate" => Some("build.role.plate"),
        _ => None,
    }
}

/// A hole on a part: x along from the cut start, y across the face.
#[derive(Clone, Debug, PartialEq)]
pub struct Hole {
    pub face: String,
    pub x: f64,
    pub y: f64,
    pub d: f64,
    pub id: Option<String>,
    pub conn: Option<String>,
}
impl Hole {
    pub fn j(&self) -> Value {
        Obj::new()
            .set("face", self.face.as_str())
            .set("x", self.x)
            .set("y", self.y)
            .set("d", self.d)
            .opt("id", self.id.clone())
            .opt("conn", self.conn.clone())
            .done()
    }
}

/// A part.
#[derive(Clone, Debug)]
pub struct Part {
    pub id: String,
    pub member: Option<String>,
    pub conn: Option<String>,
    pub stair: Option<String>,
    pub kind: String,
    pub role: Option<String>,
    pub ifc: Option<&'static str>,
    pub name: Option<String>,
    pub profile: P,
    pub profile_name: String,
    pub material: String,
    pub frame: Frame,
    pub voff: f64,
    pub length: f64,
    pub start: V3,
    pub end: V3,
    pub position: String,
    pub rotation: f64,
    pub phase: f64,
    pub holes: Vec<Hole>,
    pub cuts: Vec<Cut>,
    pub setback: [f64; 2],
    pub attach_to: Option<String>,
    /// plates only (`!!p.loose`)
    pub loose: Option<bool>,
    pub weight: f64,
}

/// A build problem.
#[derive(Clone, Debug)]
pub struct BProblem {
    pub r#ref: Option<String>,
    pub text: String,
    pub hard: Option<bool>,
}
impl BProblem {
    pub fn j(&self) -> Value {
        Obj::new().set("ref", self.r#ref.clone()).set("text", self.text.as_str()).opt("hard", self.hard).done()
    }
}

/// A footing with its bars and checks.
#[derive(Clone, Debug)]
pub struct BuiltFooting {
    pub f: Footing,
    pub bars: Vec<BarGroup>,
    pub problems: Vec<Problem>,
}

/// `build(state)`.
#[derive(Clone, Debug, Default)]
pub struct Built {
    pub parts: Vec<Part>,
    pub by_id: HashMap<String, usize>,
    pub bolts: Vec<Bolt>,
    pub welds: Vec<Weld>,
    pub footings: Vec<BuiltFooting>,
    pub problems: Vec<BProblem>,
}
impl Built {
    pub fn get(&self, id: &str) -> Option<&Part> {
        self.by_id.get(id).and_then(|&i| self.parts.get(i))
    }
}

/// V8's message for `null.name`.
fn null_name() -> String {
    "Cannot read properties of null (reading 'name')".into()
}

pub fn build(state: &Value) -> Res<Built> {
    let custom = crate::js::arr(state.get("profiles"));
    let members = crate::state::members(state);
    let conns = crate::state::conns(state);
    let mut problems = Vec::new();
    let mut drafts = Drafts::default();
    for m in members {
        let Some(profile) = get_profile_str(&m.profile, custom) else {
            problems.push(BProblem {
                r#ref: Some(m.id.clone()),
                text: t("build.err.profile", &[("name", F::from(m.profile.as_str()))]),
                hard: None,
            });
            continue;
        };
        let f = frame(m.start, m.end, m.rotation);
        let voff = voff_for(&m.position, profile.depth);
        drafts.insert(Draft { m, profile, frame: f, voff, setback: [0.0, 0.0] });
    }
    let mut plates: Vec<(PlateDraft, String)> = Vec::new();
    let mut world_holes: Vec<(WHole, String)> = Vec::new();
    let mut bolts: Vec<Bolt> = Vec::new();
    let mut welds: Vec<Weld> = Vec::new();
    for c in &conns {
        let r: Made = match c.kind.as_str() {
            "endplate" => build_end_plate(c, &drafts),
            "sheartab" => build_shear_tab(c, &drafts),
            "baseplate" => build_base_plate(c, &drafts),
            "splice" => build_splice(c, &drafts),
            "gusset" => build_gusset(c, &drafts, &conns),
            _ => Made { built: false, problems: vec![t0("build.err.connection")], ..Made::default() },
        };
        for text in &r.problems {
            problems.push(BProblem { r#ref: Some(c.id.clone()), text: text.clone(), hard: Some(!r.built) });
        }
        if !r.built {
            continue;
        }
        for s in &r.setbacks {
            if let Some(d) = drafts.get_mut(&s.member) {
                let i = if s.end == "start" { 0 } else { 1 };
                if let Some(slot) = d.setback.get_mut(i) {
                    *slot = crate::js::max(*slot, s.by);
                }
            }
        }
        for p in r.plates {
            plates.push((p, c.id.clone()));
        }
        for h in r.holes {
            world_holes.push((h, c.id.clone()));
        }
        let phase = drafts.get(c.main_of()).map_or(1.0, |d| d.m.phase1());
        for mut b in r.bolts {
            b.phase = phase;
            bolts.push(b);
        }
        welds.extend(r.welds);
    }

    let mut out = Built::default();
    let finish = |out: &mut Built, part: Part| {
        out.by_id.insert(part.id.clone(), out.parts.len());
        out.parts.push(part);
    };
    for d in &drafts.list {
        let (l0, a, b) = (d.frame.len, d.setback[0], d.setback[1]);
        if l0 - a - b < 1.0 {
            problems.push(BProblem { r#ref: Some(d.m.id.clone()), text: t0("build.err.cut-to-nothing"), hard: Some(true) });
            continue;
        }
        let o = add(d.frame.o, mul(d.frame.x, a));
        let f = Frame { o, len: l0 - a - b, ..d.frame };
        let m: &Mem = &d.m;
        finish(
            &mut out,
            Part {
                id: m.id.clone(),
                member: Some(m.id.clone()),
                conn: None,
                stair: None,
                kind: m.kind.clone(),
                role: role_key(&m.kind).map(t0),
                ifc: ifc_class(&m.kind),
                name: Some(m.name.clone()),
                profile: d.profile.clone(),
                profile_name: d.profile.name.clone(),
                material: m.material.clone(),
                frame: f,
                voff: d.voff,
                length: f.len,
                start: o,
                end: add(o, mul(f.x, f.len)),
                position: m.position.clone(),
                rotation: m.rotation,
                phase: m.phase1(),
                holes: m
                    .holes
                    .iter()
                    .map(|h| Hole { face: h.face.clone(), x: h.x, y: h.y, d: h.d, id: h.id.clone(), conn: None })
                    .collect(),
                cuts: m.cuts.clone(),
                setback: d.setback,
                attach_to: None,
                loose: None,
                weight: 0.0,
            },
        );
    }
    for (p, cid) in &plates {
        let profile = get_profile_str(&p.profile_name, custom).ok_or_else(null_name)?;
        let f = frame(p.start, p.end, p.rotation);
        let phase = p.attach_to.as_deref().and_then(|a| drafts.get(Some(a))).map_or(1.0, |d| d.m.phase1());
        finish(
            &mut out,
            Part {
                id: p.key.clone(),
                member: None,
                conn: Some(cid.clone()),
                stair: None,
                kind: "plate".into(),
                role: Some(p.role.clone()),
                ifc: Some("IfcPlate"),
                name: Some(p.role.clone()),
                profile_name: profile.name.clone(),
                profile,
                material: p.material.clone(),
                frame: f,
                voff: 0.0,
                length: f.len,
                start: p.start,
                end: p.end,
                position: "middle".into(),
                rotation: p.rotation,
                phase,
                holes: vec![],
                cuts: vec![],
                setback: [0.0, 0.0],
                attach_to: p.attach_to.clone(),
                loose: Some(p.loose),
                weight: 0.0,
            },
        );
    }
    for b in crate::js::arr(state.get("bolts")) {
        bolts.push(Bolt {
            conn: None,
            loose: Some(crate::js::s(b.get("id")).to_string()),
            d: crate::js::f(b.get("d")),
            length: crate::js::f(b.get("length")),
            grip: 0.0,
            grade: crate::js::s(b.get("grade")).to_string(),
            head: crate::js::p3_of(b.get("head")),
            axis: crate::js::p3_of(b.get("axis")),
            kind: crate::js::s(b.get("kind")).to_string(),
            phase: or(crate::js::f(b.get("phase")), 1.0),
        });
    }
    for sv in crate::js::arr(state.get("stairs")) {
        let st = Stair::from_value(sv);
        let g = stair_geometry(&st, &|name: &str| get_profile_str(name, custom));
        for pr in &g.problems {
            problems.push(BProblem { r#ref: Some(st.id.clone()), text: t(pr.text, &pr.fills), hard: Some(pr.code == "stringer") });
        }
        if g.problems.iter().any(|pr| pr.code == "stringer") {
            continue;
        }
        for q in &g.parts {
            let profile = get_profile_str(&q.profile_name, custom).ok_or_else(null_name)?;
            let f = frame(q.start, q.end, q.rotation);
            finish(
                &mut out,
                Part {
                    id: format!("{}.{}", st.id, q.key),
                    member: None,
                    conn: None,
                    stair: Some(st.id.clone()),
                    kind: q.kind.into(),
                    role: Some(q.role.clone()),
                    ifc: Some(if q.kind == "stringer" { "IfcMember" } else { "IfcPlate" }),
                    name: Some(q.role.clone()),
                    profile_name: profile.name.clone(),
                    profile,
                    material: q.material.clone(),
                    frame: f,
                    voff: 0.0,
                    length: f.len,
                    start: q.start,
                    end: q.end,
                    position: "middle".into(),
                    rotation: q.rotation,
                    phase: st.phase,
                    holes: vec![],
                    cuts: vec![],
                    setback: [0.0, 0.0],
                    attach_to: q.attach_to.as_ref().map(|a| format!("{}.{}", st.id, a)),
                    loose: None,
                    weight: 0.0,
                },
            );
        }
        for w in &g.welds {
            welds.push(Weld {
                conn: None,
                stair: Some(st.id.clone()),
                kind: "fillet",
                size: w.size,
                between: [format!("{}.{}", st.id, w.between[0]), format!("{}.{}", st.id, w.between[1])],
                length: w.length,
                r#where: w.r#where.clone(),
                all_round: false,
            });
        }
    }
    for (h, cid) in &world_holes {
        let Some(&i) = out.by_id.get(&h.target) else { continue };
        let Some(part) = out.parts.get_mut(i) else { continue };
        let l = to_local(&part.frame, h.world, part.voff);
        part.holes.push(Hole {
            face: h.face.into(),
            x: l.x,
            y: if h.face == "web" { l.v } else { l.u },
            d: h.d,
            id: None,
            conn: Some(cid.clone()),
        });
    }
    for p in &mut out.parts {
        p.holes.sort_by(|a, b| {
            let c = a.x - b.x;
            crate::js::ord(if c == 0.0 || c.is_nan() { a.y - b.y } else { c })
        });
        p.weight = p.profile.mass * p.length / 1000.0;
        for h in &p.holes {
            if h.x < -0.5 || h.x > p.length + 0.5 {
                let label = format!("{} {}", p.role.as_deref().unwrap_or("undefined"), p.name.as_deref().unwrap_or(""));
                problems.push(BProblem {
                    r#ref: p.member.clone().or_else(|| p.conn.clone()).or_else(|| p.stair.clone()),
                    text: t("build.err.hole-off", &[("part", F::from(label))]),
                    hard: None,
                });
            }
        }
    }
    for fv in crate::js::arr(state.get("footings")) {
        let f = Footing::from_value(fv);
        let bars = footing_bars(&f);
        let probs = footing_problems(&f);
        for pr in &probs {
            problems.push(BProblem { r#ref: Some(f.id.clone()), text: pr.text.clone(), hard: None });
        }
        out.footings.push(BuiltFooting { f, bars, problems: probs });
    }
    out.bolts = bolts;
    out.welds = welds;
    out.problems = problems;
    Ok(out)
}

/// The JSON of a frame.
pub fn frame_json(f: &Frame) -> Value {
    Obj::new().set("o", p3v(f.o)).set("x", p3v(f.x)).set("u", p3v(f.u)).set("v", p3v(f.v)).set("len", f.len).done()
}
/// The JSON of a part (the profile by name: the record itself is checked by the profile cases).
pub fn part_json(p: &Part) -> Value {
    let mut o = Obj::new()
        .set("id", p.id.as_str())
        .set("member", p.member.clone())
        .set("conn", p.conn.clone())
        .opt("stair", p.stair.clone())
        .set("type", p.kind.as_str())
        .opt("role", p.role.clone())
        .opt("ifc", p.ifc)
        .opt("name", p.name.clone())
        .set("profile", p.profile.name.as_str())
        .set("profileName", p.profile_name.as_str())
        .set("material", p.material.as_str())
        .set("frame", frame_json(&p.frame))
        .set("voff", p.voff)
        .set("length", p.length)
        .set("start", p3v(p.start))
        .set("end", p3v(p.end))
        .set("position", p.position.as_str())
        .set("rotation", p.rotation)
        .set("phase", p.phase)
        .set("holes", Value::Array(p.holes.iter().map(Hole::j).collect()))
        .set("cuts", Value::Array(p.cuts.iter().map(Cut::j).collect()))
        .set("setback", Obj::new().set("start", p.setback[0]).set("end", p.setback[1]).done())
        .set("attachTo", p.attach_to.clone())
        .set("weight", p.weight);
    if let Some(l) = p.loose {
        o.put("loose", l);
    }
    o.done()
}
/// The JSON of a bolt.
pub fn bolt_json(b: &Bolt) -> Value {
    Obj::new()
        .set("conn", b.conn.clone())
        .opt("loose", b.loose.clone())
        .set("d", b.d)
        .set("length", b.length)
        .set("grip", b.grip)
        .set("grade", b.grade.as_str())
        .set("head", p3v(b.head))
        .set("axis", p3v(b.axis))
        .set("kind", b.kind.as_str())
        .set("phase", b.phase)
        .done()
}
/// The JSON of a weld.
pub fn weld_json(w: &Weld) -> Value {
    let mut o = Obj::new();
    if let Some(s) = &w.stair {
        o.put("stair", s.as_str());
    } else {
        o.put("conn", w.conn.clone());
    }
    let o = o
        .set("kind", w.kind)
        .set("size", w.size)
        .set("between", vec![w.between[0].clone(), w.between[1].clone()])
        .set("length", w.length)
        .set("where", w.r#where.as_str());
    if w.all_round { o.set("allRound", true).done() } else { o.done() }
}
/// The JSON of a build (parts, bolts, welds, footings with bars, problems).
pub fn built_json(b: &Built) -> Value {
    Obj::new()
        .set("parts", Value::Array(b.parts.iter().map(part_json).collect()))
        .set("bolts", Value::Array(b.bolts.iter().map(bolt_json).collect()))
        .set("welds", Value::Array(b.welds.iter().map(weld_json).collect()))
        .set(
            "footings",
            Value::Array(
                b.footings
                    .iter()
                    .map(|f| {
                        Obj::new()
                            .set("id", f.f.id.as_str())
                            .set("bars", Value::Array(f.bars.iter().map(BarGroup::j).collect()))
                            .set("problems", Value::Array(f.problems.iter().map(Problem::j).collect()))
                            .done()
                    })
                    .collect(),
            ),
        )
        .set("problems", Value::Array(b.problems.iter().map(BProblem::j).collect()))
        .done()
}

/// The `Conn` views a build reads (for callers that hold a state).
pub fn conns_of(state: &Value) -> Vec<Conn> {
    crate::state::conns(state)
}
