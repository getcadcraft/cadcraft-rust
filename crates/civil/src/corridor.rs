//! corridor.js — one symmetric assembly (lane + cross slope, shoulder + slope, cut/fill side slopes) swept along an
//! alignment and its design profile; 0.4 V18 superelevation rotates the lanes through the curves.

use crate::alignment::Alignment;
use crate::geo::P3;
use crate::grading::{Patch, cast_slope, self_crosses};
use crate::i18n::t;
use crate::js::{J, JsMath, Obj, hypot, max, min, round_fixed};
use crate::profile::Profile;
use crate::superelev::{Plan, Super, key_stations as super_stations, slopes_at, super_plan};
use crate::tin::{Breakline, Surface};
use crate::tr;
use serde_json::Value;

/// The assembly.
#[derive(Clone, Debug, PartialEq)]
pub struct Assembly {
    pub lane: f64,
    pub lane_slope: f64,
    pub shoulder: f64,
    pub shoulder_slope: f64,
    pub cut: f64,
    pub fill: f64,
    pub step: f64,
    pub on: Option<bool>,
    pub sup: Option<Super>,
}
impl J for Assembly {
    fn j(&self) -> Value {
        let mut o = Obj::new();
        if let Some(on) = self.on {
            o = o.set("on", on);
        }
        o.set("lane", self.lane)
            .set("laneSlope", self.lane_slope)
            .set("shoulder", self.shoulder)
            .set("shoulderSlope", self.shoulder_slope)
            .set("cut", self.cut)
            .set("fill", self.fill)
            .set("step", self.step)
            .opt("super", self.sup.as_ref())
            .done()
    }
}

/// `defaultAssembly(units)`.
pub fn default_assembly(units: &str) -> Assembly {
    if units == "m" {
        Assembly { lane: 3.6, lane_slope: 0.02, shoulder: 1.2, shoulder_slope: 0.04, cut: 2.0, fill: 3.0, step: 5.0, on: None, sup: None }
    } else {
        Assembly { lane: 12.0, lane_slope: 0.02, shoulder: 4.0, shoulder_slope: 0.04, cut: 2.0, fill: 3.0, step: 10.0, on: None, sup: None }
    }
}

/// A corridor point with the daylight kind on the outer one.
#[derive(Clone, Copy, Debug)]
pub struct CP {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub kind: Option<&'static str>,
}
impl J for CP {
    fn j(&self) -> Value {
        Obj::new().set("x", self.x).set("y", self.y).set("z", self.z).opt("kind", self.kind).done()
    }
}
/// One section: station, (super slopes), left and right [cl, eop, sh, dl].
#[derive(Clone, Debug)]
pub struct Section {
    pub sta: f64,
    pub slopes: Option<(f64, f64)>,
    pub l: [CP; 4],
    pub r: [CP; 4],
}
impl J for Section {
    fn j(&self) -> Value {
        Obj::new()
            .set("sta", self.sta)
            .opt("slopes", self.slopes.map(|(l, r)| crate::jo! { "L" => l, "R" => r }))
            .set("L", self.l.to_vec())
            .set("R", self.r.to_vec())
            .done()
    }
}

/// The corridor's feature lines (CL, EOP_L, SH_L, DL_L, EOP_R, SH_R, DL_R).
pub const LINE_NAMES: [&str; 7] = ["CL", "EOP_L", "SH_L", "DL_L", "EOP_R", "SH_R", "DL_R"];

/// `info` of a corridor patch.
#[derive(Clone, Debug, Default)]
pub struct Info {
    pub sections: Vec<Section>,
    /// in LINE_NAMES order (empty when the patch failed)
    pub lines: Vec<(String, Vec<CP>)>,
    pub from: Option<f64>,
    pub to: Option<f64>,
    pub assembly: Option<Assembly>,
    pub sup: Option<Plan>,
    pub full: bool,
}
impl J for Info {
    fn j(&self) -> Value {
        let mut lines = serde_json::Map::new();
        for (k, v) in &self.lines {
            lines.insert(k.clone(), v.j());
        }
        let o = Obj::new().set("sections", &self.sections).set("lines", Value::Object(lines));
        if self.full {
            o.set("from", self.from).set("to", self.to).set("assembly", &self.assembly).set("super", &self.sup).done()
        } else {
            o.done()
        }
    }
}

/// corridorPatch's result, with the typed info kept for the project (sections use `lines`).
pub struct CorridorPatch {
    pub patch: Patch,
    pub info: Info,
}

fn empty(msg: String) -> CorridorPatch {
    let info = Info::default();
    CorridorPatch { patch: Patch { problems: vec![msg], region: None, points: Vec::new(), breaklines: Vec::new(), info: info.j() }, info }
}

/// `corridorPatch(surface, alignment, profile, asm)`: `asm` is `{ ...defaultAssembly(), ...asm }` already.
pub fn corridor_patch(surface: &Surface, alignment: Option<&Alignment>, profile: Option<&Profile>, a: &Assembly) -> CorridorPatch {
    let mut problems = Vec::new();
    let Some(alignment) = alignment.filter(|a| a.ok()) else { return empty(t("corr.err.al", &[])) };
    let Some(profile) = profile.filter(|p| p.ok()) else { return empty(t("corr.err.prof", &[])) };
    if !(a.cut > 0.0 && a.fill > 0.0) {
        return empty(t("corr.err.slopes", &[]));
    }
    let from = max(alignment.start, profile.start);
    let to = min(alignment.end, profile.end);
    if !(to - from > 1e-6) {
        return empty(t("corr.err.overlap", &[]));
    }
    // stations: every step, the ends, the curve ends and the vertical curve ends
    let mut st: Vec<f64> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add = |v: f64, st: &mut Vec<f64>| {
        let k = if v == 0.0 {
            0
        } else if v.is_nan() {
            1
        } else {
            v.to_bits()
        };
        if seen.insert(k) {
            st.push(v);
        }
    };
    add(from, &mut st);
    add(to, &mut st);
    let step = if a.step > 0.0 { a.step } else { 10.0 };
    let mut s = (from / step).ceil() * step;
    let mut guard = 0u64;
    while s < to && guard < 10_000_000 {
        add(round_fixed(s, 6), &mut st);
        s += step;
        guard = guard.saturating_add(1);
    }
    for (k, _) in alignment.key_stations() {
        if k > from && k < to {
            add(k, &mut st);
        }
    }
    for c in &profile.curves {
        for s in [c.bvc, c.evc] {
            if s > from && s < to {
                add(s, &mut st);
            }
        }
    }
    let sp = match &a.sup {
        Some(sup) if sup.on => Some(super_plan(alignment, Some(sup), a.lane_slope, a.lane)),
        _ => None,
    };
    if let Some(sp) = &sp {
        for s in super_stations(sp) {
            if s > from && s < to {
                add(round_fixed(s, 6), &mut st);
            }
        }
        problems.extend(sp.problems.iter().cloned());
    }
    crate::js::sort_by(&mut st, |x, y| x - y);
    let mut stations: Vec<f64> = Vec::new();
    for (i, &s) in st.iter().enumerate() {
        if i == 0 || st.get(i - 1).is_some_and(|&p| s - p > 1e-6) {
            stations.push(s);
        }
    }

    let sb = surface.bounds();
    let far = 2.0 * hypot(sb.max_x - sb.min_x, sb.max_y - sb.min_y);
    let mut sections: Vec<Section> = Vec::new();
    let mut off = 0usize;
    for &sta in &stations {
        let Some(p) = alignment.point_at(sta) else { continue };
        let Some(z0) = profile.elevation_at(sta) else { continue };
        let rx = p.dir.jsin();
        let ry = -p.dir.jcos(); // to the right
        let sl = match &sp {
            Some(sp) => slopes_at(sp, sta, a.lane_slope),
            None => (a.lane_slope, a.lane_slope),
        };
        let mut sides: [[CP; 4]; 2] = [[CP { x: 0.0, y: 0.0, z: 0.0, kind: None }; 4]; 2];
        for (k, sx) in [(0usize, -1.0f64), (1usize, 1.0f64)] {
            let ux = rx * sx;
            let uy = ry * sx;
            let cl = CP { x: p.x, y: p.y, z: z0, kind: None };
            let ls = if k == 0 { sl.0 } else { sl.1 };
            let ss = if ls > a.shoulder_slope { ls } else { a.shoulder_slope }; // the low side's shoulder no flatter than its lane
            let eop = CP { x: p.x + ux * a.lane, y: p.y + uy * a.lane, z: z0 - a.lane * ls, kind: None };
            let sh = CP { x: eop.x + ux * a.shoulder, y: eop.y + uy * a.shoulder, z: eop.z - a.shoulder * ss, kind: None };
            let r = cast_slope(surface, crate::geo::p2(sh.x, sh.y), ux, uy, sh.z, a.cut, a.fill, 1.0, max(0.1, step / 20.0), far);
            if r.kind == "off" {
                off += 1;
            }
            if let Some(side) = sides.get_mut(k) {
                *side = [cl, eop, sh, CP { x: r.x, y: r.y, z: r.z, kind: Some(r.kind) }];
            }
        }
        let [l, r] = sides;
        sections.push(Section { sta, slopes: sp.as_ref().map(|_| sl), l, r });
    }
    if sections.len() < 2 {
        return empty(t("corr.err.two", &[]));
    }
    if off > 0 {
        problems.push(tr!("corr.off", n = off));
    }
    let cp0 = CP { x: f64::NAN, y: f64::NAN, z: f64::NAN, kind: None };
    let line =
        |left: bool, k: usize| -> Vec<CP> { sections.iter().map(|s| *(if left { &s.l } else { &s.r }).get(k).unwrap_or(&cp0)).collect() };
    let lines: Vec<(String, Vec<CP>)> = vec![
        ("CL".into(), line(true, 0)),
        ("EOP_L".into(), line(true, 1)),
        ("SH_L".into(), line(true, 2)),
        ("DL_L".into(), line(true, 3)),
        ("EOP_R".into(), line(false, 1)),
        ("SH_R".into(), line(false, 2)),
        ("DL_R".into(), line(false, 3)),
    ];
    let mut region: Vec<P3> = line(true, 3).iter().map(|q| P3 { x: q.x, y: q.y, z: q.z }).collect();
    region.extend(line(false, 3).iter().rev().map(|q| P3 { x: q.x, y: q.y, z: q.z }));
    // neighbouring sections must not cross (they do on the inside of a curve tighter than the corridor)
    let mut crossing = false;
    'outer: for w in sections.windows(2) {
        let [sa, sb] = w else { continue };
        for left in [true, false] {
            let (a1, b1) = if left { (&sa.l, &sb.l) } else { (&sa.r, &sb.r) };
            if crate::grading::proper_cross((a1[0].x, a1[0].y), (a1[3].x, a1[3].y), (b1[0].x, b1[0].y), (b1[3].x, b1[3].y)) {
                crossing = true;
                break 'outer;
            }
        }
    }
    if crossing || (problems.is_empty() && self_crosses(&region)) {
        problems.push(t("corr.cross", &[]));
    }
    let mut points = Vec::new();
    for s in &sections {
        for q in &s.l {
            points.push(P3 { x: q.x, y: q.y, z: q.z });
        }
        for q in s.r.iter().skip(1) {
            points.push(P3 { x: q.x, y: q.y, z: q.z });
        }
    }
    let mut breaklines: Vec<Breakline> = lines
        .iter()
        .map(|(name, pts)| Breakline {
            name: name.clone(),
            pts: pts.iter().map(|q| P3 { x: q.x, y: q.y, z: q.z }).collect(),
            from_group: false,
        })
        .collect();
    // every section across (so the triangles follow the sections, and the ends close the corridor)
    for s in &sections {
        let mut pts: Vec<P3> = s.l.iter().rev().map(|q| P3 { x: q.x, y: q.y, z: q.z }).collect();
        pts.extend(s.r.iter().skip(1).map(|q| P3 { x: q.x, y: q.y, z: q.z }));
        breaklines.push(Breakline { name: "section".into(), pts, from_group: false });
    }
    let info = Info { sections, lines, from: Some(from), to: Some(to), assembly: Some(a.clone()), sup: sp, full: true };
    CorridorPatch { patch: Patch { problems, region: Some(region), points, breaklines, info: info.j() }, info }
}

/// `{ ...defaultAssembly(units), ...asm }` from a corridor object (trusted / cleaned).
pub fn read_assembly(v: &Value, units: &str) -> Assembly {
    let d = default_assembly(units);
    let n = |k: &str, dv: f64| v.get(k).map_or(dv, |x| crate::js::numf(Some(x)));
    Assembly {
        lane: n("lane", d.lane),
        lane_slope: n("laneSlope", d.lane_slope),
        shoulder: n("shoulder", d.shoulder),
        shoulder_slope: n("shoulderSlope", d.shoulder_slope),
        cut: n("cut", d.cut),
        fill: n("fill", d.fill),
        step: n("step", d.step),
        on: v.get("on").and_then(Value::as_bool),
        sup: v.get("super").filter(|s| s.is_object()).map(crate::superelev::read_super),
    }
}
