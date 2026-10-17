//! pipes.js — a gravity pipe network (storm or sanitary): structures, pipes, inverts, and the checks against the
//! user's rules. Lengths, rims, inverts and cover are in project units; pipe and structure diameters in inches (ft
//! projects) or millimetres (m projects).

use crate::i18n::t;
use crate::js::{J, Obj, arr, hypot, max, min, min_of, num_to_string, round, to_fixed};
use crate::tin::Surface;
use crate::tr;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// `{ minSlope, minCover, minDrop }` (NaN = not given).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rules {
    pub min_slope: f64,
    pub min_cover: f64,
    pub min_drop: f64,
}
impl J for Rules {
    fn j(&self) -> Value {
        crate::jo! { "minSlope" => self.min_slope, "minCover" => self.min_cover, "minDrop" => self.min_drop }
    }
}
/// A structure.
#[derive(Clone, Debug, PartialEq)]
pub struct Structure {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub rim: Option<f64>,
    pub sump_depth: f64,
    pub dia: f64,
}
impl J for Structure {
    fn j(&self) -> Value {
        crate::jo! { "id" => self.id, "name" => self.name, "x" => self.x, "y" => self.y, "rim" => self.rim, "sumpDepth" => self.sump_depth, "dia" => self.dia }
    }
}
/// A pipe (flow runs from → to).
#[derive(Clone, Debug, PartialEq)]
pub struct Pipe {
    pub id: String,
    pub name: String,
    pub from: String,
    pub to: String,
    pub dia: f64,
    pub inv_up: Option<f64>,
    pub inv_dn: Option<f64>,
}
impl J for Pipe {
    fn j(&self) -> Value {
        crate::jo! { "id" => self.id, "name" => self.name, "from" => self.from, "to" => self.to, "dia" => self.dia, "invUp" => self.inv_up, "invDn" => self.inv_dn }
    }
}
/// A network (catchments and hydrology live in hydro.rs and ride along as JSON here).
#[derive(Clone, Debug, PartialEq)]
pub struct Network {
    pub name: String,
    pub ty: String,
    pub rules: Rules,
    pub structures: Vec<Structure>,
    pub pipes: Vec<Pipe>,
    pub catchments: Option<Vec<crate::hydro::Catchment>>,
    pub hydro: Option<crate::hydro::Hydro>,
}
impl J for Network {
    fn j(&self) -> Value {
        Obj::new()
            .set("name", &self.name)
            .set("type", &self.ty)
            .set("rules", self.rules)
            .set("structures", &self.structures)
            .set("pipes", &self.pipes)
            .opt("catchments", self.catchments.as_ref())
            .opt("hydro", self.hydro.as_ref())
            .done()
    }
}

fn opt_num(v: Option<&Value>) -> Option<f64> {
    match v {
        None | Some(Value::Null) => None,
        x => Some(crate::js::numf(x)),
    }
}
/// A network from JSON as given (trusted: the harness, a cleaned project state).
pub fn read_network(v: &Value) -> Network {
    let r = v.get("rules");
    let rn = |k: &str| r.and_then(|r| r.get(k)).map_or(f64::NAN, |x| crate::js::numf(Some(x)));
    Network {
        name: crate::js::string_or_empty(v.get("name")),
        ty: crate::js::string_or_empty(v.get("type")),
        rules: Rules { min_slope: rn("minSlope"), min_cover: rn("minCover"), min_drop: rn("minDrop") },
        structures: arr(v.get("structures"))
            .iter()
            .map(|s| Structure {
                id: crate::js::string_or_empty(s.get("id")),
                name: crate::js::string_or_empty(s.get("name")),
                x: crate::js::numf(s.get("x")),
                y: crate::js::numf(s.get("y")),
                rim: opt_num(s.get("rim")),
                sump_depth: crate::js::number(s.get("sumpDepth")),
                dia: crate::js::number(s.get("dia")),
            })
            .collect(),
        pipes: arr(v.get("pipes"))
            .iter()
            .map(|p| Pipe {
                id: crate::js::string_or_empty(p.get("id")),
                name: crate::js::string_or_empty(p.get("name")),
                from: crate::js::string_or_empty(p.get("from")),
                to: crate::js::string_or_empty(p.get("to")),
                dia: crate::js::number(p.get("dia")),
                inv_up: opt_num(p.get("invUp")),
                inv_dn: opt_num(p.get("invDn")),
            })
            .collect(),
        catchments: v.get("catchments").filter(|c| c.is_array()).map(|c| arr(Some(c)).iter().map(crate::hydro::read_catchment).collect()),
        hydro: v.get("hydro").filter(|h| h.is_object()).map(crate::hydro::read_hydro),
    }
}

/// `DIA_UNIT[units].perUnit`.
pub fn per_unit(units: &str) -> f64 {
    if units == "m" { 1000.0 } else { 12.0 }
}
/// `defaultNetwork(units)`.
pub fn default_network(units: &str) -> Network {
    Network {
        name: t("pipes.default", &[]),
        ty: "storm".into(),
        rules: if units == "m" {
            Rules { min_slope: 0.005, min_cover: 0.6, min_drop: 0.03 }
        } else {
            Rules { min_slope: 0.005, min_cover: 2.0, min_drop: 0.1 }
        },
        structures: Vec::new(),
        pipes: Vec::new(),
        catchments: None,
        hydro: None,
    }
}

fn fin(x: Option<f64>) -> Option<f64> {
    x.filter(|v| v.is_finite())
}

/// A structure's facts.
#[derive(Clone, Debug)]
pub struct StructInfo {
    pub rim: Option<f64>,
    pub rim_from: Option<&'static str>,
    pub ins: Vec<String>,
    pub outs: Vec<String>,
    pub sump: Option<f64>,
}
impl J for StructInfo {
    fn j(&self) -> Value {
        crate::jo! { "rim" => self.rim, "rimFrom" => self.rim_from.map(str::to_string), "ins" => self.ins, "outs" => self.outs, "sump" => self.sump }
    }
}
/// A sample along a pipe.
#[derive(Clone, Copy, Debug)]
pub struct GroundAt {
    pub t: f64,
    pub g: Option<f64>,
    pub crown: f64,
}
/// A pipe's facts.
#[derive(Clone, Debug)]
pub struct PipeInfo {
    pub len: f64,
    pub slope: Option<f64>,
    pub cover: Option<f64>,
    pub cover_at: Option<f64>,
    pub ground: Vec<GroundAt>,
}
impl J for PipeInfo {
    fn j(&self) -> Value {
        let g: Vec<Value> = self.ground.iter().map(|q| crate::jo! { "t" => q.t, "g" => q.g, "crown" => q.crown }).collect();
        crate::jo! { "len" => self.len, "slope" => self.slope, "cover" => self.cover, "coverAt" => self.cover_at, "ground" => g }
    }
}
/// A problem `{ id, kind, msg }`.
#[derive(Clone, Debug)]
pub struct Problem {
    pub id: Option<String>,
    pub kind: &'static str,
    pub msg: String,
}
impl J for Problem {
    fn j(&self) -> Value {
        crate::jo! { "id" => self.id, "kind" => self.kind, "msg" => self.msg }
    }
}
/// `analyzeNetwork(…)` → structs and pipes (in insertion order), problems.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub structs: Vec<(String, StructInfo)>,
    pub pipes: Vec<(String, PipeInfo)>,
    pub problems: Vec<Problem>,
}
impl J for Analysis {
    fn j(&self) -> Value {
        let mut s = Map::new();
        for (k, v) in &self.structs {
            s.insert(k.clone(), v.j());
        }
        let mut p = Map::new();
        for (k, v) in &self.pipes {
            p.insert(k.clone(), v.j());
        }
        crate::jo! { "structs" => Value::Object(s), "pipes" => Value::Object(p), "problems" => self.problems }
    }
}

fn pct_txt(g: f64) -> String {
    format!("{}%", to_fixed(g * 100.0, 2))
}

/// `analyzeNetwork(net, surface, units)`.
pub fn analyze_network(net: &Network, surface: Option<&Surface>, units: &str) -> Analysis {
    let mut problems = Vec::new();
    let pu = per_unit(units);
    let d = |dia: f64| (if dia != 0.0 && !dia.is_nan() { dia } else { 0.0 }) / pu;
    let rr = &net.rules;
    // Map semantics: a later structure with the same id replaces the value but keeps the first one's place
    let mut structs: Vec<(String, StructInfo, usize)> = Vec::new();
    let mut sidx: HashMap<String, usize> = HashMap::new();
    for (k, s) in net.structures.iter().enumerate() {
        let g = surface.and_then(|sf| sf.elevation_at(s.x, s.y, 1e-6));
        let rim = fin(s.rim).or(g);
        let info = StructInfo {
            rim,
            rim_from: if fin(s.rim).is_some() {
                Some("set")
            } else if g.is_some() {
                Some("surface")
            } else {
                None
            },
            ins: Vec::new(),
            outs: Vec::new(),
            sump: None,
        };
        match sidx.get(&s.id) {
            Some(&i) => {
                if let Some(slot) = structs.get_mut(i) {
                    *slot = (s.id.clone(), info, k);
                }
            }
            None => {
                sidx.insert(s.id.clone(), structs.len());
                structs.push((s.id.clone(), info, k));
            }
        }
        if rim.is_none() {
            problems.push(Problem { id: Some(s.id.clone()), kind: "structure", msg: tr!("pipes.p.offsurf", name = s.name.as_str()) });
        }
    }
    let mut pipes: Vec<(String, PipeInfo, usize)> = Vec::new();
    let mut pidx: HashMap<String, usize> = HashMap::new();
    for (pk, p) in net.pipes.iter().enumerate() {
        let (ai, bi) = (sidx.get(&p.from).copied(), sidx.get(&p.to).copied());
        let (Some(ai), Some(bi)) = (ai, bi) else {
            problems.push(Problem { id: Some(p.id.clone()), kind: "pipe", msg: tr!("pipes.p.unconnected", name = p.name.as_str()) });
            continue;
        };
        if p.from == p.to {
            problems.push(Problem { id: Some(p.id.clone()), kind: "pipe", msg: tr!("pipes.p.unconnected", name = p.name.as_str()) });
            continue;
        }
        if let Some(a) = structs.get_mut(ai) {
            a.1.outs.push(p.id.clone());
        }
        if let Some(b) = structs.get_mut(bi) {
            b.1.ins.push(p.id.clone());
        }
        let (Some(a), Some(b)) = (structs.get(ai), structs.get(bi)) else { continue };
        let (sa, sb) = (net.structures.get(a.2), net.structures.get(b.2));
        let (Some(sa), Some(sb)) = (sa, sb) else { continue };
        let (a_rim, a_from, b_rim, b_from) = (a.1.rim, a.1.rim_from, b.1.rim, b.1.rim_from);
        let len = hypot(sb.x - sa.x, sb.y - sa.y);
        let mut info = PipeInfo { len, slope: None, cover: None, cover_at: None, ground: Vec::new() };
        let slot = match pidx.get(&p.id) {
            Some(&i) => i,
            None => {
                pidx.insert(p.id.clone(), pipes.len());
                pipes.push((p.id.clone(), info.clone(), pk));
                pipes.len() - 1
            }
        };
        let (Some(iu), Some(idn)) = (fin(p.inv_up), fin(p.inv_dn)) else {
            if let Some(s) = pipes.get_mut(slot) {
                *s = (p.id.clone(), info, pk);
            }
            problems.push(Problem { id: Some(p.id.clone()), kind: "pipe", msg: tr!("pipes.p.noinv", name = p.name.as_str()) });
            continue;
        };
        let slope = if len > 0.0 { (iu - idn) / len } else { 0.0 };
        info.slope = Some(slope);
        if slope <= 0.0 {
            let key = if slope < 0.0 { "pipes.p.uphill" } else { "pipes.p.flat" };
            problems.push(Problem { id: Some(p.id.clone()), kind: "pipe", msg: tr!(key, name = p.name.as_str(), slope = pct_txt(slope)) });
        } else if rr.min_slope > 0.0 && slope < rr.min_slope - 1e-9 {
            problems.push(Problem {
                id: Some(p.id.clone()),
                kind: "pipe",
                msg: tr!("pipes.p.flatter", name = p.name.as_str(), slope = pct_txt(slope), min = pct_txt(rr.min_slope)),
            });
        }
        // cover along the pipe
        let n = max(2.0, min(200.0, (len / 2.0).ceil()));
        let mut min_c = f64::INFINITY;
        let mut at: Option<f64> = None;
        let mut off = 0usize;
        let mut k = 0.0;
        while k <= n {
            let tt = k / n;
            let x = sa.x + (sb.x - sa.x) * tt;
            let y = sa.y + (sb.y - sa.y) * tt;
            let mut g = surface.and_then(|sf| sf.elevation_at(x, y, 1e-6));
            if k == 0.0 && a_from == Some("set") {
                g = a_rim;
            }
            if k == n && b_from == Some("set") {
                g = b_rim;
            }
            let crown = iu + (idn - iu) * tt + d(p.dia);
            info.ground.push(GroundAt { t: tt, g, crown });
            k += 1.0;
            let Some(g) = g else {
                off += 1;
                continue;
            };
            if g - crown < min_c {
                min_c = g - crown;
                at = Some(tt);
            }
        }
        if min_c.is_finite() {
            info.cover = Some(min_c);
            info.cover_at = at;
        }
        if off > 0 {
            problems.push(Problem { id: Some(p.id.clone()), kind: "pipe", msg: tr!("pipes.p.partoff", name = p.name.as_str()) });
        }
        if min_c.is_finite() && rr.min_cover > 0.0 && min_c < rr.min_cover - 1e-9 {
            let atv = at.unwrap_or(f64::NAN);
            let cover = to_fixed(min_c, 2);
            let mc = num_to_string(rr.min_cover);
            let msg = if atv <= 0.0 {
                tr!("pipes.p.cover.up", name = p.name.as_str(), cover = cover, min = mc)
            } else if atv >= 1.0 {
                tr!("pipes.p.cover.down", name = p.name.as_str(), cover = cover, min = mc)
            } else {
                tr!("pipes.p.cover.along", name = p.name.as_str(), cover = cover, at = round(atv * len), min = mc)
            };
            problems.push(Problem { id: Some(p.id.clone()), kind: "pipe", msg });
        }
        if let Some(s) = pipes.get_mut(slot) {
            *s = (p.id.clone(), info, pk);
        }
    }
    // structures: drops, sumps, splits
    let pipe_of = |pid: &str| pidx.get(pid).and_then(|&i| pipes.get(i)).and_then(|x| net.pipes.get(x.2));
    let mut sumps: Vec<Option<f64>> = Vec::new();
    for (id, st, sk) in &structs {
        let Some(s) = net.structures.get(*sk) else {
            sumps.push(None);
            continue;
        };
        let mut inv = Vec::new();
        for pid in &st.ins {
            if let Some(v) = pipe_of(pid).and_then(|p| fin(p.inv_dn)) {
                inv.push(v);
            }
        }
        for pid in &st.outs {
            if let Some(v) = pipe_of(pid).and_then(|p| fin(p.inv_up)) {
                inv.push(v);
            }
        }
        let sd = if s.sump_depth != 0.0 && !s.sump_depth.is_nan() { s.sump_depth } else { 0.0 };
        sumps.push(if inv.is_empty() { None } else { Some(min_of(inv.iter().copied()) - sd) });
        if st.outs.len() > 1 {
            problems.push(Problem {
                id: Some(id.clone()),
                kind: "structure",
                msg: tr!("pipes.s.outs", name = s.name.as_str(), n = st.outs.len()),
            });
        }
        let drop = if rr.min_drop != 0.0 && !rr.min_drop.is_nan() { rr.min_drop } else { 0.0 };
        for o in &st.outs {
            let Some(po) = pipe_of(o) else { continue };
            let Some(pu_) = fin(po.inv_up) else { continue };
            for i in &st.ins {
                let Some(pi) = pipe_of(i) else { continue };
                let Some(pd) = fin(pi.inv_dn) else { continue };
                if pu_ > pd - drop + 1e-9 {
                    let msg = if pu_ > pd {
                        tr!(
                            "pipes.s.drop.above",
                            name = s.name.as_str(),
                            out = po.name.as_str(),
                            outInv = to_fixed(pu_, 2),
                            r#in = pi.name.as_str(),
                            inInv = to_fixed(pd, 2)
                        )
                    } else {
                        tr!(
                            "pipes.s.drop.less",
                            name = s.name.as_str(),
                            out = po.name.as_str(),
                            outInv = to_fixed(pu_, 2),
                            drop = num_to_string(rr.min_drop),
                            r#in = pi.name.as_str(),
                            inInv = to_fixed(pd, 2)
                        )
                    };
                    problems.push(Problem { id: Some(id.clone()), kind: "structure", msg });
                }
            }
        }
        if let Some(rim) = st.rim {
            for pid in st.ins.iter().chain(st.outs.iter()) {
                let Some(p) = pipe_of(pid) else { continue };
                let inv = if st.ins.contains(pid) { p.inv_dn } else { p.inv_up };
                if let Some(inv) = fin(inv)
                    && inv + d(p.dia) > rim + 1e-9
                {
                    problems.push(Problem {
                        id: Some(id.clone()),
                        kind: "structure",
                        msg: tr!("pipes.s.crown", pipe = p.name.as_str(), name = s.name.as_str()),
                    });
                }
            }
        }
    }
    for ((_, st, _), sump) in structs.iter_mut().zip(sumps) {
        st.sump = sump;
    }
    if has_cycle(net) {
        problems.push(Problem { id: None, kind: "network", msg: t("pipes.loop", &[]) });
    }
    Analysis {
        structs: structs.into_iter().map(|(k, v, _)| (k, v)).collect(),
        pipes: pipes.into_iter().map(|(k, v, _)| (k, v)).collect(),
        problems,
    }
}

/// hasCycle(net): depth-first over the flow graph from every structure (iterative; the JS recurses).
pub fn has_cycle(net: &Network) -> bool {
    let mut out: HashMap<&str, Vec<&str>> = HashMap::new();
    for p in &net.pipes {
        out.entry(p.from.as_str()).or_default().push(p.to.as_str());
    }
    let mut state: HashMap<&str, u8> = HashMap::new();
    for s in &net.structures {
        // visit(s.id)
        let mut stack: Vec<(&str, usize)> = Vec::new();
        let root = s.id.as_str();
        match state.get(root) {
            Some(1) => return true,
            Some(2) => continue,
            _ => {}
        }
        state.insert(root, 1);
        stack.push((root, 0));
        while let Some((v, i)) = stack.pop() {
            let next = out.get(v).and_then(|l| l.get(i)).copied();
            match next {
                Some(w) => {
                    stack.push((v, i.saturating_add(1)));
                    match state.get(w) {
                        Some(1) => return true,
                        Some(2) => {}
                        _ => {
                            state.insert(w, 1);
                            stack.push((w, 0));
                        }
                    }
                }
                None => {
                    state.insert(v, 2);
                }
            }
        }
    }
    false
}

/// `flowOrder(net)`: pipe indices in flow order, or None on a loop (iterative; the JS recurses).
pub fn flow_order(net: &Network) -> Option<Vec<usize>> {
    let mut into: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, p) in net.pipes.iter().enumerate() {
        into.entry(p.to.as_str()).or_default().push(i);
    }
    let mut done: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut busy: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut order = Vec::new();
    for root in 0..net.pipes.len() {
        let mut stack: Vec<(usize, usize)> = Vec::new();
        // visit(root)
        let rp = net.pipes.get(root)?;
        if done.contains(rp.id.as_str()) {
            continue;
        }
        if busy.contains(rp.id.as_str()) {
            return None;
        }
        busy.insert(rp.id.as_str());
        stack.push((root, 0));
        while let Some((pi, k)) = stack.pop() {
            let p = net.pipes.get(pi)?;
            let child = into.get(p.from.as_str()).and_then(|l| l.get(k)).copied();
            match child {
                Some(qi) => {
                    stack.push((pi, k.saturating_add(1)));
                    let q = net.pipes.get(qi)?;
                    if done.contains(q.id.as_str()) {
                        continue;
                    }
                    if busy.contains(q.id.as_str()) {
                        return None;
                    }
                    busy.insert(q.id.as_str());
                    stack.push((qi, 0));
                }
                None => {
                    busy.remove(p.id.as_str());
                    done.insert(p.id.as_str());
                    order.push(pi);
                }
            }
        }
    }
    Some(order)
}

fn round2(v: f64) -> f64 {
    (v * 100.0 + 1e-7).floor() / 100.0
}

/// `autoInverts(net, surface, units, { only })` → (pipes with inverts set, problems).
pub fn auto_inverts(net: &Network, surface: Option<&Surface>, units: &str, only: Option<&[String]>) -> (Vec<Pipe>, Vec<String>) {
    let Some(order) = flow_order(net) else { return (net.pipes.clone(), vec![t("pipes.loop.inv", &[])]) };
    let rr = &net.rules;
    let z0 = |v: f64| if v != 0.0 && !v.is_nan() { v } else { 0.0 };
    let (c, s, drop) = (z0(rr.min_cover), z0(rr.min_slope), z0(rr.min_drop));
    let per = per_unit(units);
    let by_id: HashMap<&str, &Structure> = net.structures.iter().map(|x| (x.id.as_str(), x)).collect();
    let ground = |x: f64, y: f64, st: Option<&Structure>| -> Option<f64> {
        match st.and_then(|s| fin(s.rim)) {
            Some(r) => Some(r),
            None => surface.and_then(|sf| sf.elevation_at(x, y, 1e-6)),
        }
    };
    // res: id → copy (Map semantics: the last pipe with an id wins)
    let mut res: HashMap<String, Pipe> = HashMap::new();
    for p in &net.pipes {
        res.insert(p.id.clone(), p.clone());
    }
    let mut problems = Vec::new();
    for pi in order {
        let Some(p0) = net.pipes.get(pi) else { continue };
        let Some(p) = res.get(&p0.id).cloned() else { continue };
        if let Some(only) = only
            && !only.contains(&p.id)
        {
            continue;
        }
        let (Some(a), Some(b)) = (by_id.get(p.from.as_str()), by_id.get(p.to.as_str())) else { continue };
        let dm = z0(p.dia) / per;
        let len = hypot(b.x - a.x, b.y - a.y);
        let ga = ground(a.x, a.y, Some(a));
        let incoming: Vec<f64> =
            net.pipes.iter().filter(|q| q.to == p.from).filter_map(|q| res.get(&q.id).and_then(|r| fin(r.inv_dn))).collect();
        let mut up = ga.map(|g| g - c - dm);
        if !incoming.is_empty() {
            let v = min_of(incoming.iter().copied()) - drop;
            up = Some(match up {
                None => v,
                Some(u) => min(u, v),
            });
        }
        let Some(up) = up else {
            problems.push(tr!("pipes.noground", name = p.name.as_str(), at = a.name.as_str()));
            continue;
        };
        // the lowest downstream invert that keeps the slope ≥ minimum and the cover ≥ minimum all along
        let mut dn = up - s * len;
        let n = max(2.0, min(200.0, (len / 2.0).ceil()));
        let mut k = 1.0;
        while k <= n {
            let tt = k / n;
            let x = a.x + (b.x - a.x) * tt;
            let y = a.y + (b.y - a.y) * tt;
            let g = if k == n { ground(b.x, b.y, Some(b)) } else { ground(x, y, None) };
            k += 1.0;
            let Some(g) = g else { continue };
            let lim = up + (g - c - dm - up) / tt; // invert line under (ground − cover − D) at t
            if lim < dn {
                dn = lim;
            }
        }
        let mut np = p.clone();
        np.inv_up = Some(round2(up));
        np.inv_dn = Some(round2(min(dn, up - s * len)));
        let (iu, idn) = (np.inv_up.unwrap_or(f64::NAN), np.inv_dn.unwrap_or(f64::NAN));
        if idn > iu - s * len + 1e-9 {
            np.inv_dn = Some(round2(iu - s * len));
        }
        res.insert(p.id.clone(), np);
    }
    (net.pipes.iter().map(|p| res.get(&p.id).cloned().unwrap_or_else(|| p.clone())).collect(), problems)
}
