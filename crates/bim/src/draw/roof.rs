//! res-roof.js (0.5 part 4 RC.1, as the Roof tool uses it): a pitch per wall (pitched / gable / none), presets,
//! overhangs, the plate height, ridges / hips / valleys from the weighted skeleton, fascia and soffit, and the IFC the
//! roof's parts are written as.

use std::collections::HashMap;

use cc_io_ifc::model::Id;

use crate::api::R;
use crate::draw::m::{self, V2, at, cross, dot, nx, pv, sub};
use crate::draw::rooms::{Outline, label_point};
use crate::draw::skeleton::{Face, Spec, face_z, poly_area, skeleton};
use crate::draw::tr::t0;
use crate::draw::walls::Wall;
use crate::draw::writer::{D2R, QRow, W, round_d, round6, row};
use crate::util::{apply4, mul4};
use crate::wapi::{JV, P};

const IN: f64 = 25.4;

/// ROOF_DEFAULTS
pub struct RoofDefaults {
    pub pitch: f64,
    pub overhang: f64,
    pub rake: f64,
    pub thickness: f64,
    pub fascia: f64,
    pub fascia_t: f64,
    pub soffit_t: f64,
}
pub fn roof_defaults() -> RoofDefaults {
    RoofDefaults {
        pitch: m::atan(6.0 / 12.0) / D2R,
        overhang: 12.0 * IN,
        rake: 12.0 * IN,
        thickness: 8.0 * IN,
        fascia: 9.25 * IN,
        fascia_t: 1.5 * IN,
        soffit_t: 0.625 * IN,
    }
}

/// One edge's setting.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeSet {
    pub kind: String,
    pub pitch: Option<f64>,
    pub overhang: Option<f64>,
}

/// The drawn wall under each edge of the building's outline.
pub fn edge_walls(outline: &[V2], walls: &[Wall]) -> Vec<Option<String>> {
    let n = outline.len();
    (0..n)
        .map(|i| {
            let (a, b) = (at(outline, i), at(outline, nx(i, n)));
            let l = m::hypot(b[0] - a[0], b[1] - a[1]);
            if l < 1.0 {
                return None;
            }
            let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
            let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            let mut best: Option<(String, f64)> = None;
            for w in walls {
                let wl = m::hypot(w.b[0] - w.a[0], w.b[1] - w.a[1]);
                if wl < 1.0 {
                    continue;
                }
                let wu = [(w.b[0] - w.a[0]) / wl, (w.b[1] - w.a[1]) / wl];
                if cross(u, wu).abs() > 1e-4 {
                    continue;
                }
                let d = cross(wu, sub(mid, w.a)).abs();
                if (d - w.t / 2.0).abs() > 2.0 {
                    continue;
                }
                let s = dot(wu, sub(mid, w.a));
                if s < -w.t || s > wl + w.t {
                    continue;
                }
                let score = (d - w.t / 2.0).abs() + m::max(m::max(0.0, -s), s - wl);
                if best.as_ref().is_none_or(|b| score < b.1) {
                    best = Some((w.guid.clone(), score));
                }
            }
            best.map(|b| b.0)
        })
        .collect()
}

/// `parseEdges(text)` → key → setting (insertion order kept).
pub fn parse_edges(text: &str) -> Vec<(String, EdgeSet)> {
    let mut m: Vec<(String, EdgeSet)> = Vec::new();
    for part in text.split(';') {
        let mut kv = part.split('=');
        let (Some(k), Some(v)) = (kv.next(), kv.next()) else { continue };
        if k.is_empty() || v.is_empty() {
            continue;
        }
        let mut f = v.split(':');
        let c = f.next().unwrap_or("");
        let p = f.next();
        let o = f.next();
        let kind = match c {
            "g" => "gable",
            "n" => "none",
            "p" => "pitch",
            _ => continue,
        };
        let num = |s: Option<&str>| -> Option<f64> {
            let s = s?;
            if s.is_empty() {
                return None;
            }
            let x = crate::js::str_num(s);
            x.is_finite().then_some(x)
        };
        let e = EdgeSet { kind: kind.into(), pitch: if kind == "pitch" { num(p) } else { None }, overhang: num(o) };
        let key = crate::js::trim(k).to_string();
        match m.iter_mut().find(|x| x.0 == key) {
            Some(x) => x.1 = e,
            None => m.push((key, e)),
        }
    }
    m
}

/// `formatEdges(map)`
pub fn format_edges(map: &[(String, EdgeSet)]) -> String {
    let mut out = Vec::new();
    for (k, e) in map {
        if e.kind.is_empty() {
            continue;
        }
        let c = match e.kind.as_str() {
            "gable" => "g",
            "none" => "n",
            _ => "p",
        };
        let mut tail = vec![
            if c == "p" {
                e.pitch.filter(|x| x.is_finite()).map(|x| crate::js::num_str(round_d(x, 4))).unwrap_or_default()
            } else {
                String::new()
            },
            e.overhang.filter(|x| x.is_finite()).map(|x| crate::js::num_str(round_d(x, 2))).unwrap_or_default(),
        ];
        while tail.last().is_some_and(String::is_empty) {
            tail.pop();
        }
        let mut parts = vec![c.to_string()];
        parts.extend(tail);
        out.push(format!("{k}={}", parts.join(":")));
    }
    out.join(";")
}

fn key_of(guids: &[Option<String>], i: usize) -> String {
    match guids.get(i).cloned().flatten().filter(|g| !g.is_empty()) {
        Some(g) => g,
        None => format!("@{i}"),
    }
}

/// An edge's kind, pitch, overhang and wall.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeKind {
    pub kind: String,
    pub pitch: Option<f64>,
    pub overhang: f64,
    pub wall: Option<String>,
}

/// The roof settings `edgeKinds` reads.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RoofSet {
    pub pitch: f64,
    pub overhang: f64,
    pub rake: Option<f64>,
    pub gables: Vec<f64>,
    pub edges: String,
}

/// `edgeKinds(outline, walls, roof)`
pub fn edge_kinds(outline: &[V2], walls: &[Wall], roof: &RoofSet) -> Vec<EdgeKind> {
    let guids = edge_walls(outline, walls);
    let map = parse_edges(&roof.edges);
    (0..outline.len())
        .map(|i| {
            let k = key_of(&guids, i);
            let gable_i = roof.gables.contains(&(i as f64));
            let e = match map.iter().find(|x| x.0 == k) {
                Some(x) => x.1.clone(),
                None if gable_i && map.is_empty() => EdgeSet { kind: "gable".into(), pitch: None, overhang: None },
                None => EdgeSet { kind: "pitch".into(), pitch: None, overhang: None },
            };
            let pitch = if e.kind == "pitch" { Some(e.pitch.filter(|x| x.is_finite()).unwrap_or(roof.pitch)) } else { None };
            let overhang = if e.kind == "none" {
                0.0
            } else if let Some(o) = e.overhang.filter(|x| x.is_finite()) {
                o
            } else if e.kind == "gable" {
                roof.rake.filter(|x| x.is_finite()).unwrap_or(roof.overhang)
            } else {
                roof.overhang
            };
            EdgeKind { kind: e.kind, pitch, overhang, wall: guids.get(i).cloned().flatten() }
        })
        .collect()
}

/// `presetEdges(outline, walls, preset, at)` → per-wall settings, None when it can't.
pub fn preset_edges(outline: &[V2], walls: &[Wall], preset: &str, at_p: Option<V2>) -> Option<Vec<(String, EdgeSet)>> {
    let guids = edge_walls(outline, walls);
    let n = outline.len();
    let len: Vec<f64> =
        (0..n).map(|i| m::hypot(at(outline, nx(i, n))[0] - at(outline, i)[0], at(outline, nx(i, n))[1] - at(outline, i)[1])).collect();
    let mut mm: Vec<(String, EdgeSet)> = Vec::new();
    let gable = || EdgeSet { kind: "gable".into(), pitch: None, overhang: None };
    let set = |mm: &mut Vec<(String, EdgeSet)>, k: String, e: EdgeSet| match mm.iter_mut().find(|x| x.0 == k) {
        Some(x) => x.1 = e,
        None => mm.push((k, e)),
    };
    match preset {
        "hip" => Some(mm),
        "gable-short" | "gable-long" => {
            if n != 4 {
                return None;
            }
            let l = |i: usize| len.get(i).copied().unwrap_or(f64::NAN);
            let short = if l(0) + l(2) <= l(1) + l(3) { [0usize, 2] } else { [1, 3] };
            let pick = if preset == "gable-short" { short } else { [short[0].wrapping_add(1), short[1].wrapping_add(1)] };
            for i in pick {
                set(&mut mm, key_of(&guids, i), gable());
            }
            Some(mm)
        }
        "shed" => {
            let mut low = 0usize;
            if let Some(p) = at_p {
                let mut best = f64::INFINITY;
                for i in 0..n {
                    let d = seg_dist(p, at(outline, i), at(outline, nx(i, n)));
                    if d < best {
                        best = d;
                        low = i;
                    }
                }
            } else {
                let mx = m::max_of(len.iter().copied());
                low = len.iter().position(|x| *x == mx).unwrap_or(0);
            }
            for i in 0..n {
                if i != low {
                    set(&mut mm, key_of(&guids, i), gable());
                }
            }
            Some(mm)
        }
        _ => None,
    }
}

/// `segDist(p, a, b)`
pub fn seg_dist(p: V2, a: V2, b: V2) -> f64 {
    let d = sub(b, a);
    let l2 = dot(d, d);
    let l2 = if l2 != 0.0 && !l2.is_nan() { l2 } else { 1.0 };
    let t = m::max(0.0, m::min(1.0, dot(sub(p, a), d) / l2));
    m::hypot(p[0] - a[0] - d[0] * t, p[1] - a[1] - d[1] * t)
}

/// A roof face with its pitch.
#[derive(Clone, Debug, PartialEq)]
pub struct RFace {
    pub f: Face,
    pub slope: f64,
    pub pitch: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RLine {
    pub kind: &'static str,
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub len: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fascia {
    pub kind: &'static str,
    pub edge: usize,
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub n: V2,
    pub len: f64,
    pub board: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Soffit {
    pub edge: usize,
    pub poly: Vec<V2>,
    pub z: f64,
}

/// What `roofGeometry` makes.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RoofPlan {
    pub faces: Vec<RFace>,
    pub eave: Vec<V2>,
    pub top: f64,
    pub lines: Vec<RLine>,
    pub ridge: Vec<[f64; 4]>,
    pub gables: Vec<(usize, V2, V2)>,
    pub fascia: Vec<Fascia>,
    pub soffit: Vec<Soffit>,
    pub projected: f64,
    pub area: f64,
    pub lengths: [f64; 5],
}

struct E2 {
    a: V2,
    b: V2,
    l: f64,
    u: V2,
    n: V2,
}

fn edges_of(p: &[V2]) -> Vec<E2> {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (at(p, i), at(p, nx(i, n)));
            let l = m::hypot(b[0] - a[0], b[1] - a[1]);
            let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
            E2 { a, b, l, u, n: [-u[1], u[0]] }
        })
        .collect()
}

/// `roofGeometry(outline, kinds, opts)`: Err(key) when it can't be made.
pub fn roof_geometry(
    outline: &[V2],
    kinds: &[EdgeKind],
    thickness: f64,
    plate: f64,
    fascia: f64,
    fascia_t: f64,
    soffit: bool,
) -> Result<RoofPlan, &'static str> {
    let n = outline.len();
    if n < 3 {
        return Err("model.err.roofshape");
    }
    for k in kinds {
        if k.kind == "pitch" && !k.pitch.is_some_and(|p| p > 1.0 && p < 80.0) {
            return Err("model.err.pitch");
        }
    }
    if !kinds.iter().any(|k| k.kind == "pitch") {
        return Err("res.err.nopitch");
    }
    let e = edges_of(outline);
    let board = |k: &EdgeKind| if fascia > 0.0 && k.kind != "none" && k.overhang > fascia_t { fascia_t } else { 0.0 };
    let out: Vec<f64> = kinds.iter().map(|k| m::max(0.0, k.overhang - board(k))).collect();
    let eave = offset_poly(&e, &out);
    let specs: Vec<Spec> = kinds
        .iter()
        .enumerate()
        .map(|(i, k)| {
            if k.kind != "pitch" {
                return Spec { speed: 0.0, start: 0.0 };
            }
            let s = m::tan(k.pitch.unwrap_or(f64::NAN) * D2R);
            Spec { speed: 1.0 / s, start: plate - out.get(i).copied().unwrap_or(0.0) * s }
        })
        .collect();
    if !simple(&eave) {
        return Err("res.err.overhang");
    }
    let sk = skeleton(&eave, &specs);
    if let Some(err) = sk.error {
        return Err(err);
    }
    if sk.faces.is_empty() {
        return Err("model.err.roofshape");
    }
    let faces: Vec<RFace> =
        sk.faces.iter().map(|f| RFace { f: f.clone(), slope: 1.0 / f.speed, pitch: m::atan(1.0 / f.speed) / D2R }).collect();
    let mut plan = RoofPlan { eave: eave.clone(), top: sk.top, ..RoofPlan::default() };
    for l in &sk.lines {
        let (fi, fj) = (l.f[0], l.f[1]);
        let a = faces.iter().find(|f| f.f.edge == fi && on_face(&f.f, l.a, l.b));
        let b = faces.iter().find(|f| f.f.edge == fj && on_face(&f.f, l.a, l.b));
        let (Some(fa), Some(fb)) = (a, b) else { continue };
        let kind = if (l.a[2] - l.b[2]).abs() < 0.5 {
            "ridge"
        } else {
            let mid = [(l.a[0] + l.b[0]) / 2.0, (l.a[1] + l.b[1]) / 2.0];
            let cb = label_point(&fb.f.poly);
            let to_b = sub(cb, mid);
            let d0 = m::len(to_b);
            let d = if d0 != 0.0 && !d0.is_nan() { d0 } else { 1.0 };
            let q = [mid[0] + to_b[0] / d * m::min(50.0, d / 2.0), mid[1] + to_b[1] / d * m::min(50.0, d / 2.0)];
            if face_z(&fa.f, q) > face_z(&fb.f, q) { "hip" } else { "valley" }
        };
        plan.lines.push(RLine { kind, a: l.a, b: l.b, len: m::hypot3(l.b[0] - l.a[0], l.b[1] - l.a[1], l.b[2] - l.a[2]) });
        plan.ridge.push([l.a[0], l.a[1], l.b[0], l.b[1]]);
    }
    for (i, k) in kinds.iter().enumerate() {
        if k.kind == "gable"
            && let Some(ed) = e.get(i)
        {
            plan.gables.push((i, ed.a, ed.b));
        }
    }
    let ne = eave.len();
    let eave_e: Vec<E2> = (0..ne)
        .map(|i| {
            let (a, b) = (at(&eave, i), at(&eave, nx(i, ne)));
            let l0 = m::hypot(b[0] - a[0], b[1] - a[1]);
            let l = if l0 != 0.0 && !l0.is_nan() { l0 } else { 1.0 };
            let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
            E2 { a, b, l, u, n: [-u[1], u[0]] }
        })
        .collect();
    for f in &faces {
        let np = f.f.poly.len();
        for i in 0..np {
            let (p, q) = (at(&f.f.poly, i), at(&f.f.poly, nx(i, np)));
            for (j, ee) in eave_e.iter().enumerate() {
                if cross(ee.u, sub(p, ee.a)).abs() > 0.5 || cross(ee.u, sub(q, ee.a)).abs() > 0.5 {
                    continue;
                }
                let sp = dot(ee.u, sub(p, ee.a));
                let sq = dot(ee.u, sub(q, ee.a));
                if m::min(sp, sq) < -0.5 || m::max(sp, sq) > ee.l + 0.5 || (sq - sp).abs() < 1.0 {
                    continue;
                }
                let tv = thickness / m::cos(f.pitch * D2R);
                let kind = if (face_z(&f.f, p) - face_z(&f.f, q)).abs() < 0.5 { "eave" } else { "rake" };
                plan.fascia.push(Fascia {
                    kind,
                    edge: j,
                    a: [p[0], p[1], face_z(&f.f, p) + tv],
                    b: [q[0], q[1], face_z(&f.f, q) + tv],
                    n: ee.n,
                    len: m::hypot3(q[0] - p[0], q[1] - p[1], face_z(&f.f, q) - face_z(&f.f, p)),
                    board: true,
                });
                break;
            }
        }
    }
    if !(fascia > 0.0) {
        for x in &mut plan.fascia {
            x.board = false;
        }
    }
    if soffit {
        for (i, k) in kinds.iter().enumerate() {
            let oi = out.get(i).copied().unwrap_or(0.0);
            if k.kind != "pitch" || oi < 5.0 {
                continue;
            }
            let p = k.pitch.unwrap_or(f64::NAN);
            let s = m::tan(p * D2R);
            let tv = thickness / m::cos(p * D2R);
            let z_eave = plate - oi * s;
            let z = m::min(z_eave, if fascia > 0.0 { z_eave + tv - fascia } else { z_eave });
            let Some(ed) = e.get(i) else { continue };
            let poly = vec![ed.a, ed.b, at(&eave, nx(i, n)), at(&eave, i)];
            if poly_area(&poly).abs() < 1e4 {
                continue;
            }
            plan.soffit.push(Soffit { edge: i, poly, z });
        }
    }
    let ovs: Vec<f64> = kinds.iter().map(|k| k.overhang).collect();
    plan.projected = poly_area(&offset_poly(&e, &ovs)).abs();
    plan.area = faces.iter().fold(0.0, |s, f| s + poly_area(&f.f.poly).abs() / m::cos(f.pitch * D2R));
    let sum = |kind: &str| plan.lines.iter().filter(|l| l.kind == kind).fold(0.0, |s, l| s + l.len);
    let fsum = |kind: &str| plan.fascia.iter().filter(|x| x.kind == kind).fold(0.0, |s, x| s + x.len);
    plan.lengths = [sum("ridge"), sum("hip"), sum("valley"), fsum("eave"), fsum("rake")];
    plan.faces = faces;
    Ok(plan)
}

fn on_face(f: &Face, a: [f64; 3], b: [f64; 3]) -> bool {
    let n = f.poly.len();
    let near = |p: V2| (0..n).any(|i| seg_dist(p, at(&f.poly, i), at(&f.poly, nx(i, n))) < 1.0);
    near([a[0], a[1]]) && near([b[0], b[1]])
}

/// Each edge moved out by its own distance, one point per corner.
fn offset_poly(e: &[E2], d: &[f64]) -> Vec<V2> {
    let n = e.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let h = pv(i, n);
        let (Some(f), Some(ee)) = (e.get(h), e.get(i)) else { continue };
        let dh = d.get(h).copied().unwrap_or(0.0);
        let di = d.get(i).copied().unwrap_or(0.0);
        let p0 = [f.a[0] - f.n[0] * dh, f.a[1] - f.n[1] * dh];
        let p1 = [ee.a[0] - ee.n[0] * di, ee.a[1] - ee.n[1] * di];
        let c = cross(f.u, ee.u);
        if c.abs() < 1e-9 {
            out.push(p1);
            continue;
        }
        let tt = cross(sub(p1, p0), ee.u) / c;
        out.push([p0[0] + f.u[0] * tt, p0[1] + f.u[1] * tt]);
    }
    out
}

fn simple(p: &[V2]) -> bool {
    let n = p.len();
    for i in 0..n {
        for j in i.wrapping_add(1)..n {
            if i.abs_diff(j) <= 1 || (i == 0 && j == n.wrapping_sub(1)) {
                continue;
            }
            let (a, b, c, d) = (at(p, i), at(p, nx(i, n)), at(p, j), at(p, nx(j, n)));
            let d1 = cross(sub(b, a), sub(c, a));
            let d2 = cross(sub(b, a), sub(d, a));
            let d3 = cross(sub(d, c), sub(a, c));
            let d4 = cross(sub(d, c), sub(b, c));
            if ((d1 > 1e-6 && d2 < -1e-6) || (d1 < -1e-6 && d2 > 1e-6)) && ((d3 > 1e-6 && d4 < -1e-6) || (d3 < -1e-6 && d4 > 1e-6)) {
                return false;
            }
        }
    }
    poly_area(p) > 0.0
}

/// The underside's height over p (None outside).
pub fn height_at(plan: &RoofPlan, p: V2) -> Option<f64> {
    let mut best: Option<f64> = None;
    for f in &plan.faces {
        if m::in_poly(p[0], p[1], &f.f.poly) {
            let z = face_z(&f.f, p);
            if best.is_none_or(|b| z > b) {
                best = Some(z);
            }
        }
    }
    best
}

/// A roof as model-slabs.js readDerived reads it (what writeRoofParts needs).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Roof {
    pub id: Id,
    pub guid: String,
    pub storey: Option<Id>,
    /// the walls' level (GlobalId)
    pub walls: String,
    pub seed: V2,
    pub pitch: f64,
    pub overhang: f64,
    pub thickness: f64,
    pub gables: Vec<f64>,
    pub key: String,
    pub parts: Vec<Id>,
    pub edges: String,
    pub rake: Option<f64>,
    pub plate: f64,
    pub fascia: f64,
    pub soffit: bool,
}

impl Roof {
    pub fn set(&self) -> RoofSet {
        RoofSet { pitch: self.pitch, overhang: self.overhang, rake: self.rake, gables: self.gables.clone(), edges: self.edges.clone() }
    }
}

/// IfcRoofTypeEnum from the edges.
pub fn roof_type_of(kinds: &[EdgeKind]) -> &'static str {
    let p = kinds.iter().filter(|k| k.kind == "pitch").count();
    if p == 1 {
        "SHED_ROOF"
    } else if kinds.iter().any(|k| k.kind == "gable") {
        "GABLE_ROOF"
    } else {
        "HIP_ROOF"
    }
}

fn nearest_wall(walls: &[Wall], a: V2, b: V2) -> Option<&Wall> {
    let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let mut v: Vec<(&Wall, f64)> = walls.iter().map(|x| (x, seg_dist(mid, x.a, x.b))).collect();
    v.sort_by(|p, q| crate::js::sign_ord(p.1 - q.1));
    v.first().map(|x| x.0)
}

/// `writeRoofParts(w, r, { wallsLv, L, outline, top })` → the parts made, or None (the outline can't be roofed).
pub fn write_roof_parts(w: &mut W<'_>, r: &Roof, walls_lv: Id, walls: &[Wall], outline: &Outline, top: f64) -> R<Option<Vec<Id>>> {
    let d = roof_defaults();
    let kinds = edge_kinds(&outline.poly, walls, &r.set());
    let plate = if r.plate.is_finite() && r.plate > 0.0 { r.plate } else { top };
    let fascia = if r.fascia.is_finite() { r.fascia } else { d.fascia };
    let plan = match roof_geometry(&outline.poly, &kinds, r.thickness, plate, fascia, d.fascia_t, r.soffit) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let rst = r.storey.ok_or_else(|| crate::api::JsErr("TypeError: storey".into()))?;
    let host = w.must(rst)?;
    let mi = mul4(&crate::draw::writer::inv4j(&w.storey_world(rst)?)?, &w.storey_world(walls_lv)?);
    let pp = |x: f64, y: f64, z: f64| apply4(&mi, x, y, z);
    let dd = |x: f64, y: f64, z: f64| {
        let a = apply4(&mi, x, y, z);
        let o = apply4(&mi, 0.0, 0.0, 0.0);
        [a[0] - o[0], a[1] - o[1], a[2] - o[2]]
    };
    let x3 = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    if !r.parts.is_empty() {
        w.drop_product(&r.parts)?;
    }
    let hp = host.a("ObjectPlacement").href().ok_or_else(|| crate::api::JsErr("TypeError: ObjectPlacement".into()))?;
    let mut parts = Vec::new();
    let slab_name = w.l(&t0("model.roof.slab"));
    let roofing = w.surface_style(&t0("model.style.roofing"), [0.42, 0.33, 0.3], 0.0)?;
    for f in &plan.faces {
        let (cs, sn) = (m::cos(f.pitch * D2R), m::sin(f.pitch * D2R));
        let o = [f.f.n[0] * f.f.c, f.f.n[1] * f.f.c];
        let o3 = pp(o[0], o[1], f.f.start);
        let xd = dd(f.f.u[0], f.f.u[1], 0.0);
        let yd = dd(f.f.n[0] * cs, f.f.n[1] * cs, sn);
        let zd = x3(xd, yd);
        let prof: Vec<V2> =
            f.f.poly
                .iter()
                .map(|p| [(p[0] - o[0]) * f.f.u[0] + (p[1] - o[1]) * f.f.u[1], ((p[0] - o[0]) * f.f.n[0] + (p[1] - o[1]) * f.f.n[1]) / cs])
                .collect();
        let g = w.guid();
        let oh = w.owner_history();
        let lp = w.place_at(Some(hp), 0.0, 0.0, 0.0, 0.0)?;
        let sid =
            w.make("IFCSLAB", vec![g, oh, slab_name.clone(), JV::Null, slab_name.clone(), JV::H(lp), JV::Null, JV::Null, JV::s("ROOF")])?;
        let pr = w.profile(&prof, None)?;
        let fr = w.axis_frame(o3, zd, xd)?;
        let solid = w.extrude_at(pr, fr, r.thickness / cs, [0.0, sn, cs])?;
        w.style_item(solid, roofing)?;
        let rep = w.rep("body", "Body", "SweptSolid", &[solid])?;
        w.set_shape(sid, &[rep])?;
        w.upsert_pset(
            sid,
            "CadCraft_RoofFace",
            &[row("PitchDegrees", "IFCREAL", P::Num(round_d(f.pitch, 4))), row("Edge", "IFCINTEGER", P::Num(f.f.edge as f64))],
        )?;
        parts.push(sid);
    }
    let gable_name = w.l(&t0("model.roof.gable"));
    for &(edge, a, b) in &plan.gables {
        let len = m::hypot(b[0] - a[0], b[1] - a[1]);
        let u = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
        let nn = [-u[1], u[0]];
        let kw = kinds.get(edge).and_then(|k| k.wall.clone());
        let wall = walls.iter().find(|x| Some(&x.guid) == kw.as_ref()).or_else(|| nearest_wall(walls, a, b));
        let base = wall.map_or(top, |x| x.base_offset + x.height);
        let mut pts: Vec<V2> = Vec::new();
        for k in 0..=64 {
            let tt = f64::from(k) / 64.0;
            let q = [a[0] + (b[0] - a[0]) * tt + nn[0], a[1] + (b[1] - a[1]) * tt + nn[1]];
            if let Some(z) = height_at(&plan, q)
                && z > base + 1.0
            {
                pts.push([tt * len, z - base]);
            }
        }
        if pts.len() < 2 {
            continue;
        }
        let mut prof0 = vec![[at(&pts, 0)[0], 0.0], [at(&pts, pts.len().wrapping_sub(1))[0], 0.0]];
        pts.reverse();
        prof0.extend(pts);
        let np = prof0.len();
        let prof: Vec<V2> = (0..np)
            .filter(|&i| {
                if i < 2 || i == np.wrapping_sub(1) {
                    return true;
                }
                let (p, pn, pp_) = (at(&prof0, i), at(&prof0, i.wrapping_add(1)), at(&prof0, i.wrapping_sub(1)));
                ((pn[1] - p[1]) * (p[0] - pp_[0]) - (p[1] - pp_[1]) * (pn[0] - p[0])).abs() > 1e-3
            })
            .map(|i| at(&prof0, i))
            .collect();
        let th = wall.map_or(6.0 * IN, |x| x.t);
        let xd = dd(u[0], u[1], 0.0);
        let zd = dd(nn[0], nn[1], 0.0);
        let yd = x3(zd, xd);
        let flip = yd[2] < 0.0;
        let g = w.guid();
        let oh = w.owner_history();
        let lp = w.place_at(Some(hp), 0.0, 0.0, 0.0, 0.0)?;
        let mut args = vec![g, oh, gable_name.clone(), JV::Null, gable_name.clone(), JV::H(lp), JV::Null, JV::Null];
        if w.v4 {
            args.push(JV::s("STANDARD"));
        }
        let gid = w.make("IFCWALL", args)?;
        let pr: Vec<V2> = if flip { prof.iter().map(|&[x, y]| [x, -y]).collect() } else { prof };
        let prid = w.profile(&pr, None)?;
        let fr = w.axis_frame(pp(a[0], a[1], base), zd, xd)?;
        let ex = w.extrude_at(prid, fr, th, [0.0, 0.0, 1.0])?;
        let rep = w.rep("body", "Body", "SweptSolid", &[ex])?;
        w.set_shape(gid, &[rep])?;
        parts.push(gid);
    }
    let boards: Vec<&Fascia> = plan.fascia.iter().filter(|x| x.board && fascia > 0.0).collect();
    if !boards.is_empty() {
        let trim = w.surface_style(&t0("res.style.trim"), [0.93, 0.92, 0.88], 0.0)?;
        let mut items = Vec::new();
        for fb in &boards {
            let l2 = m::hypot(fb.b[0] - fb.a[0], fb.b[1] - fb.a[1]);
            let u = [(fb.b[0] - fb.a[0]) / l2, (fb.b[1] - fb.a[1]) / l2];
            let outv = [-fb.n[0], -fb.n[1]];
            let xd = dd(u[0], u[1], 0.0);
            let zd = dd(outv[0], outv[1], 0.0);
            let yd = x3(zd, xd);
            let flip = yd[2] < 0.0;
            let prof = vec![[0.0, -fascia], [l2, fb.b[2] - fb.a[2] - fascia], [l2, fb.b[2] - fb.a[2]], [0.0, 0.0]];
            let pr: Vec<V2> = if flip {
                let mut v: Vec<V2> = prof.iter().map(|&[x, y]| [x, -y]).collect();
                v.reverse();
                v
            } else {
                prof
            };
            let prid = w.profile(&pr, None)?;
            let fr = w.axis_frame(pp(fb.a[0], fb.a[1], fb.a[2]), zd, xd)?;
            let solid = w.extrude_at(prid, fr, d.fascia_t, [0.0, 0.0, 1.0])?;
            w.style_item(solid, trim)?;
            items.push(solid);
        }
        let nm = w.l(&t0("res.roof.fascia"));
        let g = w.guid();
        let oh = w.owner_history();
        let ot = w.l("Fascia");
        let lp = w.place_at(Some(hp), 0.0, 0.0, 0.0, 0.0)?;
        let mut args = vec![g, oh, nm, JV::Null, ot, JV::H(lp), JV::Null, JV::Null];
        if w.v4 {
            args.push(JV::s("USERDEFINED"));
        }
        let mid = w.make("IFCMEMBER", args)?;
        let rep = w.rep("body", "Body", "SweptSolid", &items)?;
        w.set_shape(mid, &[rep])?;
        let total = boards.iter().fold(0.0, |s, x| s + x.len);
        w.upsert_qto(mid, "Qto_MemberBaseQuantities", &[("Length".into(), "length", total)])?;
        let u = |w: &W<'_>, v: f64| P::Num(round6(w.u(v)));
        w.upsert_pset(
            mid,
            "CadCraft_Fascia",
            &[
                row("Height", "IFCPOSITIVELENGTHMEASURE", u(w, fascia)),
                row("Thickness", "IFCPOSITIVELENGTHMEASURE", u(w, d.fascia_t)),
                row("EaveLength", "IFCLENGTHMEASURE", u(w, plan.lengths[3])),
                row("RakeLength", "IFCLENGTHMEASURE", u(w, plan.lengths[4])),
            ],
        )?;
        parts.push(mid);
    }
    if !plan.soffit.is_empty() {
        let mut items = Vec::new();
        let mut area = 0.0;
        for s in &plan.soffit {
            let prid = w.profile(&s.poly, None)?;
            let fr = w.axis_frame(pp(0.0, 0.0, s.z - d.soffit_t), dd(0.0, 0.0, 1.0), dd(1.0, 0.0, 0.0))?;
            items.push(w.extrude_at(prid, fr, d.soffit_t, [0.0, 0.0, 1.0])?);
            area += poly_area(&s.poly).abs();
        }
        let sty = w.surface_style(&t0("res.style.trim"), [0.93, 0.92, 0.88], 0.0)?;
        for &it in &items {
            w.style_item(it, sty)?;
        }
        let nm = w.l(&t0("res.roof.soffit"));
        let g = w.guid();
        let oh = w.owner_history();
        let ot = w.l("Soffit");
        let lp = w.place_at(Some(hp), 0.0, 0.0, 0.0, 0.0)?;
        let cid = w.make("IFCCOVERING", vec![g, oh, nm, JV::Null, ot, JV::H(lp), JV::Null, JV::Null, JV::s("CEILING")])?;
        let rep = w.rep("body", "Body", "SweptSolid", &items)?;
        w.set_shape(cid, &[rep])?;
        w.upsert_pset(cid, "Pset_CoveringCommon", &[row("IsExternal", "IFCBOOLEAN", P::Bool(true))])?;
        let q: Vec<QRow> =
            vec![("Width".into(), "length", d.soffit_t), ("GrossArea".into(), "area", area), ("NetArea".into(), "area", area)];
        w.upsert_qto(cid, "Qto_CoveringBaseQuantities", &q)?;
        parts.push(cid);
    }
    let mut rel = None;
    for rid in w.ids("IFCRELAGGREGATES") {
        let x = w.must(rid)?;
        if x.a("RelatingObject").href() == Some(r.id) {
            rel = Some(rid);
            break;
        }
    }
    match rel {
        Some(rid) => {
            let mut x = w.touch(rid)?;
            x.set("RelatedObjects", JV::L(parts.iter().map(|&p| JV::H(p)).collect()));
            w.write(&mut x)?;
        }
        None => {
            if !parts.is_empty() {
                let g = w.guid();
                let oh = w.owner_history();
                w.make("IFCRELAGGREGATES", vec![g, oh, JV::Null, JV::Null, JV::H(r.id), JV::L(parts.iter().map(|&p| JV::H(p)).collect())])?;
            }
        }
    }
    let q: Vec<QRow> = vec![
        ("GrossArea".into(), "area", plan.area),
        ("NetArea".into(), "area", plan.area),
        ("ProjectedArea".into(), "area", plan.projected),
    ];
    w.upsert_qto(r.id, "Qto_RoofBaseQuantities", &q)?;
    let lm = |w: &W<'_>, k: &str, v: f64| row(k, "IFCLENGTHMEASURE", P::Num(round6(w.u(v))));
    let rows = vec![
        lm(w, "RidgeLength", plan.lengths[0]),
        lm(w, "HipLength", plan.lengths[1]),
        lm(w, "ValleyLength", plan.lengths[2]),
        lm(w, "EaveLength", plan.lengths[3]),
        lm(w, "RakeLength", plan.lengths[4]),
    ];
    w.upsert_pset(r.id, "CadCraft_RoofLines", &rows)?;
    if w.v4 {
        let mut l = w.touch(r.id)?;
        l.set("PredefinedType", JV::s(roof_type_of(&kinds)));
        w.write(&mut l)?;
    }
    Ok(Some(parts))
}

/// Kept for callers that want the plan's lengths by name.
pub fn lengths_map(p: &RoofPlan) -> HashMap<&'static str, f64> {
    [("ridge", p.lengths[0]), ("hip", p.lengths[1]), ("valley", p.lengths[2]), ("eave", p.lengths[3]), ("rake", p.lengths[4])]
        .into_iter()
        .collect()
}
