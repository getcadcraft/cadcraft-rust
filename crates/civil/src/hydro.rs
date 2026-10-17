//! hydro.js — storm drainage on a pipe network (0.4 V19 + V20): catchments and the rational method, Manning pipe
//! sizing, the hydraulic and energy grade lines.

use crate::geo::P2;
use crate::js::{J, JsMath, Obj, arr, hypot, max, min, pow, slice16, strip_controls, to_fixed, trim};
use crate::pipes::{Network, Pipe, Problem, per_unit};
use crate::tin::Surface;
use crate::tr;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::f64::consts::PI;

/// `{ a, b, c }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Idf {
    pub a: f64,
    pub b: f64,
    pub c: f64,
}
/// `net.hydro`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hydro {
    pub idf: Idf,
    pub n: f64,
    pub kj: f64,
    pub tailwater: Option<f64>,
    pub min_tc: f64,
    pub v_assumed: f64,
}
impl J for Hydro {
    fn j(&self) -> Value {
        crate::jo! {
            "idf" => crate::jo! { "a" => self.idf.a, "b" => self.idf.b, "c" => self.idf.c },
            "n" => self.n, "kj" => self.kj, "tailwater" => self.tailwater, "minTc" => self.min_tc, "vAssumed" => self.v_assumed,
        }
    }
}
/// `HYDRO_DEFAULTS(units)`.
pub fn hydro_defaults(units: &str) -> Hydro {
    if units == "m" {
        Hydro { idf: Idf { a: 1700.0, b: 12.0, c: 0.8 }, n: 0.013, kj: 0.5, tailwater: None, min_tc: 10.0, v_assumed: 1.0 }
    } else {
        Hydro { idf: Idf { a: 60.0, b: 10.0, c: 0.75 }, n: 0.013, kj: 0.5, tailwater: None, min_tc: 10.0, v_assumed: 3.0 }
    }
}
/// `PIPE_SIZES[units]`.
pub fn pipe_sizes(units: &str) -> Vec<f64> {
    if units == "m" {
        vec![300.0, 375.0, 450.0, 525.0, 600.0, 675.0, 750.0, 900.0, 1050.0, 1200.0, 1350.0, 1500.0, 1650.0, 1800.0]
    } else {
        vec![12.0, 15.0, 18.0, 21.0, 24.0, 27.0, 30.0, 36.0, 42.0, 48.0, 54.0, 60.0, 66.0, 72.0]
    }
}
fn fin(v: Option<&Value>) -> Option<f64> {
    crate::js::fin(v)
}

/// `cleanHydro(o, units)`.
pub fn clean_hydro(o: Option<&Value>, units: &str) -> Hydro {
    let d = hydro_defaults(units);
    let Some(o) = o.filter(|o| o.is_object() || o.is_array()) else { return d };
    let pos = |v: Option<&Value>, dv: f64, mx: f64| fin(v).filter(|&x| x > 0.0 && x < mx).unwrap_or(dv);
    let idf = o.get("idf");
    let g = |k: &str| idf.and_then(|i| i.get(k));
    Hydro {
        idf: Idf {
            a: pos(g("a"), d.idf.a, 1e6),
            b: fin(g("b")).filter(|&b| (0.0..1000.0).contains(&b)).unwrap_or(d.idf.b),
            c: pos(g("c"), d.idf.c, 5.0),
        },
        n: pos(o.get("n"), d.n, 1.0),
        kj: fin(o.get("kj")).filter(|&k| (0.0..=10.0).contains(&k)).unwrap_or(d.kj),
        tailwater: fin(o.get("tailwater")).filter(|t| t.abs() < 1e6),
        min_tc: pos(o.get("minTc"), d.min_tc, 1000.0),
        v_assumed: d.v_assumed,
    }
}
/// A hydro object as given (trusted).
pub fn read_hydro(o: &Value) -> Hydro {
    let n = |v: Option<&Value>| crate::js::numf(v);
    let idf = o.get("idf");
    Hydro {
        idf: Idf { a: n(idf.and_then(|i| i.get("a"))), b: n(idf.and_then(|i| i.get("b"))), c: n(idf.and_then(|i| i.get("c"))) },
        n: n(o.get("n")),
        kj: n(o.get("kj")),
        tailwater: o.get("tailwater").filter(|t| !t.is_null()).map(|t| n(Some(t))),
        min_tc: n(o.get("minTc")),
        v_assumed: n(o.get("vAssumed")),
    }
}

/// A catchment.
#[derive(Clone, Debug, PartialEq)]
pub struct Catchment {
    pub id: Option<String>,
    pub name: String,
    pub pts: Vec<P2>,
    pub c: f64,
    pub tc: f64,
    pub to: Option<String>,
}
impl J for Catchment {
    fn j(&self) -> Value {
        crate::jo! { "id" => self.id, "name" => self.name, "pts" => self.pts, "C" => self.c, "tc" => self.tc, "to" => self.to }
    }
}
/// `cleanCatchment(o)`.
pub fn clean_catchment(o: &Value) -> Option<Catchment> {
    if !o.is_object() {
        return None;
    }
    let pts_v = o.get("pts")?.as_array()?;
    let pts: Vec<P2> = pts_v
        .iter()
        .filter_map(|p| {
            let x = fin(p.get("x"))?;
            let y = fin(p.get("y"))?;
            (x.abs() < 1e9 && y.abs() < 1e9).then_some(P2 { x, y })
        })
        .take(10000)
        .collect();
    if pts.len() < 3 {
        return None;
    }
    let id_s = crate::js::string_of(o.get("id"));
    let id_ok = id_s.len() >= 2
        && id_s.len() <= 10
        && id_s.starts_with('k')
        && id_s.get(1..).is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit()));
    let name = slice16(trim(&strip_controls(&crate::js::string_or_empty(o.get("name")), " ")), 120);
    Some(Catchment {
        id: id_ok.then_some(id_s),
        name: if name.is_empty() { "Catchment".into() } else { name },
        pts,
        c: fin(o.get("C")).filter(|c| (0.0..=1.0).contains(c)).unwrap_or(0.5),
        tc: fin(o.get("tc")).filter(|&t| t > 0.0 && t < 1000.0).unwrap_or(10.0),
        to: o.get("to").and_then(Value::as_str).filter(|s| s.encode_utf16().count() < 40).map(str::to_string),
    })
}
/// A catchment as given (trusted).
pub fn read_catchment(o: &Value) -> Catchment {
    Catchment {
        id: o.get("id").and_then(Value::as_str).map(str::to_string),
        name: crate::js::string_or_empty(o.get("name")),
        pts: crate::geo::read_p2s(o.get("pts")),
        c: crate::js::numf(o.get("C")),
        tc: crate::js::numf(o.get("tc")),
        to: o.get("to").and_then(Value::as_str).map(str::to_string),
    }
}

/// `catchmentArea(pts, units)` → acres (ft) or hectares (m).
pub fn catchment_area(pts: &[P2], units: &str) -> f64 {
    let n = pts.len();
    let mut a = 0.0;
    if n > 0 {
        let mut j = n - 1;
        for i in 0..n {
            let (pi, pj) = (pts.get(i).copied().unwrap_or_default(), pts.get(j).copied().unwrap_or_default());
            a += (pj.x + pi.x) * (pj.y - pi.y);
            j = i;
        }
    }
    a = a.abs() / 2.0;
    if units == "m" { a / 10000.0 } else { a / 43560.0 }
}
/// `idf(tc, h)` = a/(tc+b)^c.
pub fn idf(tc: f64, h: &Hydro) -> f64 {
    h.idf.a / pow(tc + h.idf.b, h.idf.c)
}
fn kk(units: &str) -> f64 {
    if units == "m" { 1.0 } else { 1.486 }
}
fn gg(units: &str) -> f64 {
    if units == "m" { 9.81 } else { 32.174 }
}
fn dia_of(p: &Pipe, units: &str) -> f64 {
    (if p.dia != 0.0 && !p.dia.is_nan() { p.dia } else { 0.0 }) / per_unit(units)
}

/// `manningFull(D, S, n, units)` → (Q, V, A).
pub fn manning_full(d: f64, s: f64, n: f64, units: &str) -> (f64, f64, f64) {
    if !(d > 0.0) || !(s > 0.0) || !(n > 0.0) {
        return (0.0, 0.0, (PI * d * d) / 4.0);
    }
    let a = (PI * d * d) / 4.0;
    let r = d / 4.0;
    let q = (kk(units) / n) * a * pow(r, 2.0 / 3.0) * s.sqrt();
    (q, q / a, a)
}
/// `partFull(y, D)` → (A, P, T).
pub fn part_full(y: f64, d: f64) -> (f64, f64, f64) {
    let yy = max(0.0, min(d, y));
    let th = 2.0 * max(-1.0, min(1.0, 1.0 - (2.0 * yy) / d)).jacos();
    ((d * d / 8.0) * (th - th.jsin()), (d * th) / 2.0, d * (th / 2.0).jsin())
}
fn q_at(y: f64, d: f64, s: f64, n: f64, units: &str) -> f64 {
    let (a, p, _) = part_full(y, d);
    if p > 0.0 { (kk(units) / n) * a * pow(a / p, 2.0 / 3.0) * s.sqrt() } else { 0.0 }
}
/// `normalDepth(Q, D, S, n, units)` → y | None (more than the pipe carries).
pub fn normal_depth(q: f64, d: f64, s: f64, n: f64, units: &str) -> Option<f64> {
    if !(q > 0.0) {
        return Some(0.0);
    }
    if !(s > 0.0) {
        return None;
    }
    let mut hi = d;
    let mut best = 0.0;
    for k in 0..=200 {
        let y = (d * f64::from(k)) / 200.0;
        let qq = q_at(y, d, s, n, units);
        if qq > best {
            best = qq;
            hi = y;
        }
    }
    if q > best {
        return None;
    }
    let mut lo = 0.0;
    for _ in 0..80 {
        let m = (lo + hi) / 2.0;
        if q_at(m, d, s, n, units) < q {
            lo = m;
        } else {
            hi = m;
        }
    }
    Some((lo + hi) / 2.0)
}

/// Pipe indices in an order where every pipe comes after the ones flowing into its upper structure; None on a loop.
fn order(net: &Network) -> Option<Vec<usize>> {
    let mut ins: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, p) in net.pipes.iter().enumerate() {
        ins.entry(p.to.as_str()).or_default().push(i);
    }
    let mut done: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut out = Vec::new();
    while out.len() < net.pipes.len() {
        let mut moved = false;
        for (i, p) in net.pipes.iter().enumerate() {
            if done.contains(p.id.as_str()) {
                continue;
            }
            let ok = ins
                .get(p.from.as_str())
                .is_none_or(|l| l.iter().all(|&q| net.pipes.get(q).is_some_and(|qq| done.contains(qq.id.as_str()))));
            if ok {
                done.insert(p.id.as_str());
                out.push(i);
                moved = true;
            }
        }
        if !moved {
            return None;
        }
    }
    Some(out)
}

/// A structure's flow `{ CA, tc }`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SFlow {
    pub ca: f64,
    pub tc: f64,
}
/// A pipe's flow.
#[derive(Clone, Copy, Debug, Default)]
pub struct PFlow {
    pub ca: f64,
    pub tc: f64,
    pub i: f64,
    pub q: f64,
    pub travel: f64,
    pub len: f64,
    pub s: f64,
}
/// `rationalFlows(…)`.
#[derive(Clone, Debug)]
pub struct Flows {
    pub structs: Vec<(String, SFlow)>,
    pub pipes: Vec<(String, PFlow)>,
    pub problems: Vec<Problem>,
    pub hydro: Hydro,
}
impl Flows {
    pub fn pipe(&self, id: &str) -> Option<&PFlow> {
        self.pipes.iter().find(|(k, _)| k == id).map(|(_, v)| v)
    }
}
impl J for Flows {
    fn j(&self) -> Value {
        let mut s = Map::new();
        for (k, v) in &self.structs {
            s.insert(k.clone(), crate::jo! { "CA" => v.ca, "tc" => v.tc });
        }
        let mut p = Map::new();
        for (k, v) in &self.pipes {
            p.insert(
                k.clone(),
                crate::jo! { "CA" => v.ca, "tc" => v.tc, "i" => v.i, "Q" => v.q, "travel" => v.travel, "len" => v.len, "S" => v.s },
            );
        }
        crate::jo! { "structs" => Value::Object(s), "pipes" => Value::Object(p), "problems" => self.problems, "hydro" => self.hydro }
    }
}

/// Insertion-ordered map helpers (JS Map semantics).
fn map_set<T>(v: &mut Vec<(String, T)>, k: &str, x: T) {
    match v.iter_mut().find(|(kk, _)| kk == k) {
        Some(slot) => slot.1 = x,
        None => v.push((k.to_string(), x)),
    }
}
fn map_get<'a, T>(v: &'a [(String, T)], k: &str) -> Option<&'a T> {
    v.iter().find(|(kk, _)| kk == k).map(|(_, x)| x)
}
fn map_get_mut<'a, T>(v: &'a mut [(String, T)], k: &str) -> Option<&'a mut T> {
    v.iter_mut().find(|(kk, _)| kk == k).map(|(_, x)| x)
}

/// `rationalFlows(net, units)`.
pub fn rational_flows(net: &Network, units: &str) -> Flows {
    let h = match &net.hydro {
        Some(hy) => clean_hydro(Some(&hy.j()), units),
        None => clean_hydro(None, units),
    };
    let mut problems = Vec::new();
    let mut structs: Vec<(String, SFlow)> = Vec::new();
    for s in &net.structures {
        map_set(&mut structs, &s.id, SFlow::default());
    }
    for c in net.catchments.iter().flatten() {
        let to = c.to.as_deref().unwrap_or("\u{0}none");
        match map_get_mut(&mut structs, to) {
            None => problems.push(Problem { id: c.id.clone(), kind: "catchment", msg: tr!("hy.no.outlet", name = c.name.as_str()) }),
            Some(s) => {
                s.ca += c.c * catchment_area(&c.pts, units);
                s.tc = max(s.tc, c.tc);
            }
        }
    }
    let mut pipes: Vec<(String, PFlow)> = Vec::new();
    let mut by_id: HashMap<&str, &crate::pipes::Structure> = HashMap::new();
    for x in &net.structures {
        by_id.insert(x.id.as_str(), x);
    }
    let Some(ord) = order(net) else {
        problems.push(Problem { id: None, kind: "network", msg: tr!("hy.loop") });
        return Flows { structs, pipes, problems, hydro: h };
    };
    for pi in ord {
        let Some(p) = net.pipes.get(pi) else { continue };
        let (Some(up), Some(_)) = (map_get(&structs, &p.from).copied(), map_get(&structs, &p.to)) else { continue };
        let tc = max(up.tc, h.min_tc);
        let i = idf(tc, &h);
        let q = if units == "m" { (up.ca * i) / 360.0 } else { up.ca * i };
        let (Some(a), Some(b)) = (by_id.get(p.from.as_str()), by_id.get(p.to.as_str())) else { continue };
        let len = hypot(b.x - a.x, b.y - a.y);
        let s = match (p.inv_up.filter(|v| v.is_finite()), p.inv_dn.filter(|v| v.is_finite())) {
            (Some(u), Some(d)) if len > 0.0 => (u - d) / len,
            _ => 0.0,
        };
        let v = if s > 0.0 { manning_full(dia_of(p, units), s, h.n, units).1 } else { 0.0 };
        let travel = len / max(if v > 0.0 { v } else { h.v_assumed }, 1e-6) / 60.0;
        map_set(&mut pipes, &p.id, PFlow { ca: up.ca, tc, i, q, travel, len, s });
        if let Some(dn) = map_get_mut(&mut structs, &p.to) {
            dn.ca += up.ca;
            dn.tc = max(dn.tc, tc + travel);
        }
    }
    Flows { structs, pipes, problems, hydro: h }
}

/// One of `sizePipes`.
#[derive(Clone, Copy, Debug)]
pub struct Size {
    pub dia: f64,
    pub q: f64,
    pub cap: f64,
    pub ok: bool,
    pub flat: bool,
}

/// `sizePipes(net, flows, units, { sizes })` → [{ id, dia, Q, cap, ok, flat? }].
pub fn size_pipes(net: &Network, flows: &Flows, units: &str, sizes: Option<&[f64]>) -> Vec<(String, Size)> {
    let per = per_unit(units);
    let h = flows.hydro;
    let def = pipe_sizes(units);
    let sizes = sizes.unwrap_or(&def);
    let mut out = Vec::new();
    for p in &net.pipes {
        let Some(f) = flows.pipe(&p.id) else { continue };
        if !(f.s > 0.0) {
            out.push((p.id.clone(), Size { dia: p.dia, q: f.q, cap: 0.0, ok: false, flat: true }));
            continue;
        }
        let mut pick: Option<(f64, f64, bool)> = None;
        for &d in sizes {
            let cap = manning_full(d / per, f.s, h.n, units).0;
            if cap >= f.q {
                pick = Some((d, cap, false));
                break;
            }
        }
        let (dia, cap, none) = match pick {
            Some(p) => p,
            None => {
                let d = sizes.last().copied().unwrap_or(f64::NAN);
                (d, manning_full(d / per, f.s, h.n, units).0, true)
            }
        };
        out.push((p.id.clone(), Size { dia, q: f.q, cap, ok: !none, flat: false }));
    }
    out
}
/// JSON of sizePipes' result.
pub fn sizes_json(v: &[(String, Size)]) -> Value {
    Value::Array(
        v.iter()
            .map(|(id, s)| {
                let o = Obj::new().set("id", id).set("dia", s.dia).set("Q", s.q).set("cap", s.cap).set("ok", s.ok);
                if s.flat { o.set("flat", true).done() } else { o.done() }
            })
            .collect(),
    )
}

/// A pipe's grade lines.
#[derive(Clone, Copy, Debug)]
pub struct PGrade {
    pub hgl_up: f64,
    pub hgl_dn: f64,
    pub egl_up: f64,
    pub egl_dn: f64,
    pub yn: Option<f64>,
    pub full: bool,
    pub v: f64,
    pub sf: f64,
}
/// A structure's grade lines.
#[derive(Clone, Copy, Debug)]
pub struct SGrade {
    pub hgl: f64,
    pub egl: f64,
    pub rim: Option<f64>,
}
/// `hydraulicGrade(…)`.
#[derive(Clone, Debug, Default)]
pub struct Grades {
    pub pipes: Vec<(String, PGrade)>,
    pub structs: Vec<(String, SGrade)>,
    pub problems: Vec<Problem>,
}
impl J for Grades {
    fn j(&self) -> Value {
        let mut p = Map::new();
        for (k, v) in &self.pipes {
            p.insert(
                k.clone(),
                crate::jo! { "hglUp" => v.hgl_up, "hglDn" => v.hgl_dn, "eglUp" => v.egl_up, "eglDn" => v.egl_dn, "yn" => v.yn, "full" => v.full, "V" => v.v, "Sf" => v.sf },
            );
        }
        let mut s = Map::new();
        for (k, v) in &self.structs {
            s.insert(k.clone(), crate::jo! { "hgl" => v.hgl, "egl" => v.egl, "rim" => v.rim });
        }
        crate::jo! { "pipes" => Value::Object(p), "structs" => Value::Object(s), "problems" => self.problems }
    }
}

/// `hydraulicGrade(net, flows, units, surface)`.
pub fn hydraulic_grade(net: &Network, flows: &Flows, units: &str, surface: Option<&Surface>) -> Grades {
    let h = flows.hydro;
    let g = gg(units);
    let mut out = Grades::default();
    let Some(ord) = order(net) else {
        out.problems.push(Problem { id: None, kind: "network", msg: tr!("hy.loop") });
        return out;
    };
    let mut s_by_id: HashMap<&str, &crate::pipes::Structure> = HashMap::new();
    for s in &net.structures {
        s_by_id.insert(s.id.as_str(), s);
    }
    let rim_of = |id: &str| -> Option<f64> {
        let s = s_by_id.get(id)?;
        match s.rim.filter(|r| r.is_finite()) {
            Some(r) => Some(r),
            None => surface.and_then(|sf| sf.elevation_at(s.x, s.y, 1e-6)),
        }
    };
    for &pi in ord.iter().rev() {
        let Some(p) = net.pipes.get(pi) else { continue };
        let Some(f) = flows.pipe(&p.id).copied() else { continue };
        let (Some(inv_up), Some(inv_dn)) = (p.inv_up.filter(|v| v.is_finite()), p.inv_dn.filter(|v| v.is_finite())) else { continue };
        let d = dia_of(p, units);
        let full = manning_full(d, f.s, h.n, units);
        let below = map_get(&out.structs, &p.to).copied();
        let outlet = !net.pipes.iter().any(|q| q.from == p.to);
        let yn = if f.s > 0.0 { normal_depth(f.q, d, f.s, h.n, units) } else { None };
        let surch = yn.is_none();
        let other = if outlet { h.tailwater.unwrap_or(f64::NEG_INFINITY) } else { below.map_or(f64::NEG_INFINITY, |b| b.hgl) };
        let start_dn = max(inv_dn + if surch { d } else { yn.unwrap_or(f64::NAN) }, other);
        // friction slope with the pipe full (pressure flow), from Manning
        let a = full.2;
        let r = d / 4.0;
        let sf = if a > 0.0 { pow((f.q * h.n) / (kk(units) * a * pow(r, 2.0 / 3.0)), 2.0) } else { 0.0 };
        let full_flow = surch || start_dn > inv_dn + d + 1e-9;
        let hgl_dn = start_dn;
        let hgl_up = if full_flow { max(hgl_dn + sf * f.len, inv_up + d) } else { inv_up + yn.unwrap_or(f64::NAN) };
        let a_flow = if full_flow { a } else { part_full(yn.unwrap_or(f64::NAN), d).0 };
        let v = if a_flow > 0.0 { f.q / a_flow } else { 0.0 };
        let hv = (v * v) / (2.0 * g);
        map_set(&mut out.pipes, &p.id, PGrade { hgl_up, hgl_dn, egl_up: hgl_up + hv, egl_dn: hgl_dn + hv, yn, full: full_flow, v, sf });
        if full_flow {
            out.problems.push(Problem { id: Some(p.id.clone()), kind: "pipe", msg: tr!("hy.full", name = p.name.as_str()) });
        }
        // the structure above: the outgoing pipe's upper HGL + the junction loss
        let sh = hgl_up + h.kj * hv;
        let prev = map_get(&out.structs, &p.from).copied();
        if prev.is_none_or(|pr| sh > pr.hgl) {
            map_set(&mut out.structs, &p.from, SGrade { hgl: sh, egl: sh + hv, rim: rim_of(&p.from) });
        }
        if map_get(&out.structs, &p.to).is_none() && outlet {
            map_set(&mut out.structs, &p.to, SGrade { hgl: hgl_dn, egl: hgl_dn + hv, rim: rim_of(&p.to) });
        }
    }
    let flood: Vec<Problem> = out
        .structs
        .iter()
        .filter_map(|(id, s)| {
            let rim = s.rim.filter(|r| r.is_finite())?;
            if s.hgl > rim + 1e-9 {
                let name = s_by_id.get(id.as_str()).map(|x| x.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone());
                Some(Problem { id: Some(id.clone()), kind: "structure", msg: tr!("hy.flood", name = name, by = to_fixed(s.hgl - rim, 2)) })
            } else {
                None
            }
        })
        .collect();
    out.problems.extend(flood);
    out
}

/// Catchments from JSON (trusted list).
pub fn read_catchments(v: Option<&Value>) -> Vec<Catchment> {
    arr(v).iter().map(read_catchment).collect()
}
