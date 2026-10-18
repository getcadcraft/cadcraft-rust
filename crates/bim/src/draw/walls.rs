//! model-walls.js — walls you draw: wall types, joins cleaned automatically (L / T / X, a line, three at a point), and
//! the IFC they are written as (IfcWall, Axis + Body, layer set usage, type, Pset_WallCommon, quantities, CadCraft_Wall).

use std::collections::HashMap;

use cc_io_ifc::model::{Id, id_key};

use crate::api::R;
use crate::draw::levels::{Level, read_levels};
use crate::draw::m::{self, V2, add, at, cross, dist, dot, left, mul, nx, pv, sub};
use crate::draw::tr::{t, t0};
use crate::draw::writer::{D2R, HostIndex, QRow, Row, W, key_err, round6, row};
use crate::wapi::{JV, P};

pub const WALL_PSET: &str = "CadCraft_Wall";
pub const WALLTYPE_PSET: &str = "CadCraft_WallType";
const IN: f64 = 25.4;

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub mat: &'static str,
    pub t: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WallType {
    pub key: String,
    /// the starter a custom thickness is made from
    pub base: Option<&'static str>,
    pub exterior: bool,
    pub layers: Vec<Layer>,
}

fn ly(mat: &'static str, t: f64) -> Layer {
    Layer { mat, t }
}

/// The starter wall types (model.wt.<key>, model.mat.<mat>).
pub fn wall_types() -> Vec<WallType> {
    let w = |key: &str, exterior: bool, layers: Vec<Layer>| WallType { key: key.into(), base: None, exterior, layers };
    vec![
        w("ext-2x6", true, vec![ly("siding", 0.75 * IN), ly("osb", 0.5 * IN), ly("studs", 5.5 * IN), ly("gypsum", 0.5 * IN)]),
        w("ext-2x4", true, vec![ly("siding", 0.75 * IN), ly("osb", 0.5 * IN), ly("studs", 3.5 * IN), ly("gypsum", 0.5 * IN)]),
        w(
            "brick-2x4",
            true,
            vec![ly("brick", 3.625 * IN), ly("air", IN), ly("osb", 0.5 * IN), ly("studs", 3.5 * IN), ly("gypsum", 0.5 * IN)],
        ),
        w("int-2x4", false, vec![ly("gypsum", 0.5 * IN), ly("studs", 3.5 * IN), ly("gypsum", 0.5 * IN)]),
        w("int-2x6", false, vec![ly("gypsum", 0.5 * IN), ly("studs", 5.5 * IN), ly("gypsum", 0.5 * IN)]),
        w("cmu-8", true, vec![ly("cmu", 7.625 * IN)]),
        w("concrete-200", true, vec![ly("concrete", 200.0)]),
        w("ext-300", true, vec![ly("render", 15.0), ly("insulation", 100.0), ly("block", 175.0), ly("plaster", 10.0)]),
        w("int-100", false, vec![ly("gypsum", 12.5), ly("studs", 75.0), ly("gypsum", 12.5)]),
    ]
}

const STARTERS: [&str; 9] = ["ext-2x6", "ext-2x4", "brick-2x4", "int-2x4", "int-2x6", "cmu-8", "concrete-200", "ext-300", "int-100"];

/// `wallType(key)`: a starter, or a starter at another thickness ("<starter>@<mm>").
pub fn wall_type(key: &str) -> Option<WallType> {
    let base_key = key.split('@').next().unwrap_or("");
    let base = wall_types().into_iter().find(|x| x.key == base_key)?;
    if !key.contains('@') {
        return Some(base);
    }
    let want = crate::js::str_num(key.split('@').nth(1).unwrap_or(""));
    let total: f64 = base.layers.iter().fold(0.0, |s, l| s + l.t);
    let thickest = m::max_of(base.layers.iter().map(|l| l.t));
    let rest = total - thickest;
    if !(want > rest + 1.0 && want <= 2000.0) {
        return None;
    }
    // the first layer as thick as any other (reduce keeps the first of equals)
    let mut core = 0usize;
    for (i, l) in base.layers.iter().enumerate() {
        if l.t > base.layers.get(core).map_or(f64::NEG_INFINITY, |c| c.t) {
            core = i;
        }
    }
    let sk = STARTERS.iter().find(|s| **s == base.key).copied();
    Some(WallType {
        key: key.to_string(),
        base: sk,
        exterior: base.exterior,
        layers: base
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| if i == core { Layer { mat: l.mat, t: want - rest } } else { l.clone() })
            .collect(),
    })
}

/// `typeThickness(type)`
pub fn type_thickness(t: &WallType) -> f64 {
    t.layers.iter().fold(0.0, |s, l| s + l.t)
}

/// `typeAt(typeKey, mm)`: the key of the type made `mm` thick.
pub fn type_at(type_key: &str, mm: f64) -> Option<String> {
    let base = wall_type(type_key.split('@').next().unwrap_or(""))?;
    if (type_thickness(&base) - mm).abs() < 0.05 {
        return Some(base.key);
    }
    let k = format!("{}@{}", base.key, crate::js::num_str(m::round(mm * 10.0) / 10.0));
    wall_type(&k).map(|_| k)
}

/// `typeName(type)`
pub fn type_name(ty: &WallType) -> String {
    match ty.base {
        Some(b) => {
            t("model.wt.custom", &[("name", &t0(&format!("model.wt.{b}"))), ("t", &crate::js::num_str(m::round(type_thickness(ty))))])
        }
        None => t0(&format!("model.wt.{}", ty.key)),
    }
}

fn mat_name(mat: &str) -> String {
    t0(&format!("model.mat.{mat}"))
}

// ------------------------------------------------------------------------------------------------------------- joins

/// A wall for the joins: a key, centre line a → b, thickness.
#[derive(Clone, Debug, PartialEq)]
pub struct Seg {
    pub key: String,
    pub a: V2,
    pub b: V2,
    pub t: f64,
}

struct Prep {
    key: String,
    a: V2,
    b: V2,
    l: f64,
    u: V2,
    n: V2,
    h: f64,
}

fn meet(p1: V2, d1: V2, p2: V2, d2: V2) -> Option<V2> {
    let c = cross(d1, d2);
    if c.abs() < 1e-9 {
        return None;
    }
    let s = cross(sub(p2, p1), d2) / c;
    Some(add(p1, mul(d1, s)))
}

struct Node {
    p: V2,
    ends: Vec<(usize, bool)>,
    tees: Vec<(usize, f64)>,
}

fn prepare(walls: &[Seg]) -> Vec<Prep> {
    walls
        .iter()
        .map(|w| {
            let l = dist(w.a, w.b);
            let u = if l > 0.0 { mul(sub(w.b, w.a), 1.0 / l) } else { [1.0, 0.0] };
            Prep { key: w.key.clone(), a: w.a, b: w.b, l, u, n: left(u), h: w.t / 2.0 }
        })
        .filter(|w| w.l > 0.0 && w.h > 0.0)
        .collect()
}

fn wall_nodes(ws: &[Prep], tol: f64) -> Vec<Node> {
    let mut nodes: Vec<Node> = Vec::new();
    for (i, w) in ws.iter().enumerate() {
        for (end, p) in [(false, w.a), (true, w.b)] {
            let k = match nodes.iter().position(|x| dist(x.p, p) <= tol) {
                Some(k) => k,
                None => {
                    nodes.push(Node { p, ends: Vec::new(), tees: Vec::new() });
                    nodes.len().wrapping_sub(1)
                }
            };
            if let Some(n) = nodes.get_mut(k) {
                n.ends.push((i, end));
            }
        }
    }
    for n in &mut nodes {
        for (j, w) in ws.iter().enumerate() {
            if n.ends.iter().any(|e| e.0 == j) {
                continue;
            }
            let s = dot(sub(n.p, w.a), w.u);
            let off = dot(sub(n.p, w.a), w.n).abs();
            if s > tol && s < w.l - tol && off <= w.h + tol {
                n.tees.push((j, s));
            }
        }
    }
    nodes
}

#[derive(Clone)]
struct Ray {
    w: usize,
    end: bool,
    d: V2,
    o: V2,
    h: f64,
    real: bool,
    ang: f64,
    left: V2,
    right: V2,
}

#[derive(Clone, Copy)]
struct End {
    left: V2,
    right: V2,
    centre: Option<V2>,
}

/// `wallOutlines(walls, { tol })` → (key, outline anticlockwise) in the walls' order.
pub fn wall_outlines(walls: &[Seg], tol: f64) -> Vec<(String, Vec<V2>)> {
    let ws = prepare(walls);
    let nodes = wall_nodes(&ws, tol);
    let mut ends: Vec<[Option<End>; 2]> = vec![[None, None]; ws.len()];
    for n in &nodes {
        let mut rays: Vec<Ray> = Vec::new();
        for &(wi, end) in &n.ends {
            let Some(w) = ws.get(wi) else { continue };
            let d = if end { mul(w.u, -1.0) } else { w.u };
            rays.push(Ray { w: wi, end, d, o: n.p, h: w.h, real: true, ang: 0.0, left: [0.0; 2], right: [0.0; 2] });
        }
        for &(ti, s) in &n.tees {
            let Some(tw) = ws.get(ti) else { continue };
            let q = add(tw.a, mul(tw.u, s));
            rays.push(Ray { w: ti, end: false, d: tw.u, o: q, h: tw.h, real: false, ang: 0.0, left: [0.0; 2], right: [0.0; 2] });
            rays.push(Ray { w: ti, end: false, d: mul(tw.u, -1.0), o: q, h: tw.h, real: false, ang: 0.0, left: [0.0; 2], right: [0.0; 2] });
        }
        let p = n.p;
        for r in &mut rays {
            r.ang = m::atan2(r.d[1], r.d[0]);
            r.left = add(r.o, mul(left(r.d), r.h));
            r.right = add(r.o, mul(left(r.d), -r.h));
        }
        if rays.len() > 1 {
            rays.sort_by(|x, y| crate::js::sign_ord(x.ang - y.ang));
            let k = rays.len();
            for i in 0..k {
                let j = nx(i, k);
                let (Some(a), Some(b)) = (rays.get(i).cloned(), rays.get(j).cloned()) else { continue };
                if !a.real && !b.real {
                    continue;
                }
                let c = meet(add(a.o, mul(left(a.d), a.h)), a.d, add(b.o, mul(left(b.d), -b.h)), b.d);
                let turn = cross(a.d, b.d);
                let ahead = dot(a.d, b.d);
                let Some(c) = c else { continue };
                if turn <= 1e-9 && ahead > 0.0 {
                    continue;
                }
                if dist(c, p) > 4.0 * m::max(a.h, b.h) + tol {
                    continue;
                }
                if let Some(r) = rays.get_mut(i) {
                    r.left = c;
                }
                if let Some(r) = rays.get_mut(j) {
                    r.right = c;
                }
            }
        }
        let centre = if rays.len() >= 3 && n.tees.is_empty() { Some(p) } else { None };
        for r in &rays {
            if r.real
                && let Some(e) = ends.get_mut(r.w)
                && let Some(slot) = e.get_mut(usize::from(r.end))
            {
                *slot = Some(End { left: r.left, right: r.right, centre });
            }
        }
    }
    let mut out = Vec::new();
    for (i, w) in ws.iter().enumerate() {
        let (Some(ea), Some(eb)) =
            (ends.get(i).and_then(|e| e.first().copied().flatten()), ends.get(i).and_then(|e| e.get(1).copied().flatten()))
        else {
            continue;
        };
        let mut poly: Vec<V2> = Vec::new();
        poly.push(ea.right);
        poly.push(eb.left);
        if let Some(c) = eb.centre {
            poly.push(c);
        }
        poly.push(eb.right);
        poly.push(ea.left);
        if let Some(c) = ea.centre {
            poly.push(c);
        }
        let mut poly = clean(&poly);
        if !simple_ccw(&poly) {
            poly = rect(w);
        }
        out.push((w.key.clone(), poly));
    }
    out
}

/// The outline of a key in a wall_outlines result.
pub fn outline_of<'o>(o: &'o [(String, Vec<V2>)], key: &str) -> Option<&'o Vec<V2>> {
    o.iter().find(|(k, _)| k == key).map(|(_, p)| p)
}

fn rect(w: &Prep) -> Vec<V2> {
    vec![add(w.a, mul(w.n, -w.h)), add(w.b, mul(w.n, -w.h)), add(w.b, mul(w.n, w.h)), add(w.a, mul(w.n, w.h))]
}

/// Drop repeated and collinear points.
fn clean(poly: &[V2]) -> Vec<V2> {
    let mut p: Vec<V2> = Vec::new();
    for (i, q) in poly.iter().enumerate() {
        if i == 0 || dist(*q, at(poly, i.wrapping_sub(1))) > 1e-6 {
            p.push(*q);
        }
    }
    if p.len() > 1 && dist(at(&p, 0), at(&p, p.len().wrapping_sub(1))) <= 1e-6 {
        p.pop();
    }
    let mut changed = true;
    while changed && p.len() > 3 {
        changed = false;
        let n = p.len();
        for i in 0..n {
            let a = at(&p, pv(i, n));
            let b = at(&p, i);
            let c = at(&p, nx(i, n));
            let ab = sub(b, a);
            let bc = sub(c, b);
            if cross(ab, bc).abs() <= 1e-6 * (m::len(ab) * m::len(bc) + 1e-12) && dot(ab, bc) >= 0.0 {
                p.remove(i);
                changed = true;
                break;
            }
        }
    }
    p
}

/// `polyArea(p)`
pub fn poly_area(p: &[V2]) -> f64 {
    let n = p.len();
    let mut s = 0.0;
    for i in 0..n {
        s += cross(at(p, i), at(p, nx(i, n)));
    }
    s / 2.0
}

fn simple_ccw(p: &[V2]) -> bool {
    if p.len() < 3 || poly_area(p) <= 1e-6 {
        return false;
    }
    let n = p.len();
    for i in 0..n {
        for j in i.wrapping_add(1)..n {
            if j == i.wrapping_add(1) || (i == 0 && j == n.wrapping_sub(1)) {
                continue;
            }
            if seg_cross(at(p, i), at(p, nx(i, n)), at(p, j), at(p, nx(j, n))) {
                return false;
            }
        }
    }
    true
}

fn seg_cross(a: V2, b: V2, c: V2, d: V2) -> bool {
    let o = |p: V2, q: V2, r: V2| m::sign(cross(sub(q, p), sub(r, p)));
    o(a, b, c) * o(a, b, d) < 0.0 && o(c, d, a) * o(c, d, b) < 0.0
}

// ----------------------------------------------------------------------------------------------------------- reading

/// A drawn wall in its storey's plan axes (mm).
#[derive(Clone, Debug, PartialEq)]
pub struct Wall {
    pub id: Id,
    pub guid: String,
    pub storey: Option<Id>,
    pub a: V2,
    pub b: V2,
    pub len: f64,
    pub t: f64,
    pub z: f64,
    pub base_offset: f64,
    pub top_level: String,
    pub top_offset: f64,
    pub fixed_height: f64,
    pub type_key: String,
    pub type_name: String,
    pub ext_side: String,
    pub lp: Id,
    pub name: String,
    pub height: f64,
}

impl Wall {
    pub fn seg(&self) -> Seg {
        Seg { key: self.guid.clone(), a: self.a, b: self.b, t: self.t }
    }
}

/// `x || ''` of a pset value as text.
pub fn ps_str(ps: &HashMap<String, P>, k: &str) -> String {
    match ps.get(k) {
        Some(v) if v.truthy() => v.string(),
        _ => String::new(),
    }
}
/// `Number(ps[k] || 0)`
pub fn ps_num0(ps: &HashMap<String, P>, k: &str) -> f64 {
    match ps.get(k) {
        Some(v) if v.truthy() => v.number(),
        _ => 0.0,
    }
}
/// `Number(ps[k])`
pub fn ps_num(ps: &HashMap<String, P>, k: &str) -> f64 {
    ps.get(k).map_or(f64::NAN, P::number)
}

/// `readWalls(w, storey)`: every wall the tools made (pset CadCraft_Wall).
pub fn read_walls(w: &mut W<'_>, storey: Option<Id>) -> R<Vec<Wall>> {
    let mut out = Vec::new();
    let list = w.api.ids("IFCWALL", true);
    let levels = read_levels(w)?;
    let so = w.storey_of()?;
    let mut seen = std::collections::HashSet::new();
    for id in list {
        if !seen.insert(id_key(id)) {
            continue;
        }
        let Some(ps) = w.read_pset(id, WALL_PSET)? else { continue };
        let st = so.get(&id_key(id)).copied();
        if let Some(s) = storey
            && st != Some(s)
        {
            continue;
        }
        let line = w.must(id)?;
        let st_line = match st {
            Some(s) => w.line(s)?,
            None => None,
        };
        let lp = line.get("ObjectPlacement").filter(|v| v.truthy()).and_then(JV::href);
        let st_lp = st_line.as_ref().and_then(|s| s.get("ObjectPlacement")).filter(|v| v.truthy()).and_then(JV::href);
        let (Some(lp), Some(st_lp)) = (lp, st_lp) else { continue };
        let mm = w.rel_matrix(st_lp, lp)?;
        let len = ps_num(&ps, "Length") * w.unit_mm;
        let th = ps_num(&ps, "Thickness") * w.unit_mm;
        let a = [mm[12], mm[13]];
        let dir = [mm[0], mm[1]];
        let dl0 = m::hypot(dir[0], dir[1]);
        let dl = if dl0 != 0.0 && !dl0.is_nan() { dl0 } else { 1.0 };
        let b = [a[0] + dir[0] / dl * len, a[1] + dir[1] / dl * len];
        let mut wall = Wall {
            id,
            guid: line.a("GlobalId").val_str().unwrap_or_default(),
            storey: st,
            a,
            b,
            len,
            t: th,
            z: mm[14],
            base_offset: ps_num0(&ps, "BaseOffset") * w.unit_mm,
            top_level: ps_str(&ps, "TopLevel"),
            top_offset: ps_num0(&ps, "TopOffset") * w.unit_mm,
            fixed_height: ps_num0(&ps, "Height") * w.unit_mm,
            type_key: ps_str(&ps, "TypeKey"),
            type_name: crate::draw::levels::name_or(line.a("ObjectType"), line.a("Name"), ""),
            ext_side: if ps.get("ExteriorSide").and_then(P::as_str).as_deref() == Some("right") { "right".into() } else { "left".into() },
            lp,
            name: line.a("Name").text_or_empty(),
            height: 0.0,
        };
        let base = levels.iter().find(|l| Some(l.id) == st);
        wall.height = wall_height(&wall, &levels, base);
        out.push(wall);
    }
    Ok(out)
}

/// A wall's height: to the level its top follows (+ offset), else its own height.
pub fn wall_height(wall: &Wall, levels: &[Level], base: Option<&Level>) -> f64 {
    let top = if wall.top_level.is_empty() { None } else { levels.iter().find(|l| l.guid.as_deref() == Some(wall.top_level.as_str())) };
    if let (Some(top), Some(base)) = (top, base) {
        let h = top.elevation + wall.top_offset - (base.elevation + wall.base_offset);
        if h > 1.0 {
            return h;
        }
    }
    if wall.fixed_height > 0.0 { wall.fixed_height } else { 2438.4 }
}

// ----------------------------------------------------------------------------------------------------------- writing

/// The IfcWallType of a type key: found by its CadCraft_WallType Key, else made with its layer set.
/// → (type id, layer set id, the type)
pub fn wall_type_line(w: &mut W<'_>, key: &str) -> R<(Id, Option<Id>, WallType)> {
    let ty = wall_type(key).ok_or_else(|| key_err("model.err.walltype"))?;
    for id in w.ids("IFCWALLTYPE") {
        let tl = w.must(id)?;
        for h in tl.a("HasPropertySets").hrefs() {
            let Some(ps) = w.line(h)? else { continue };
            if ps.a("Name").val_str().as_deref() != Some(WALLTYPE_PSET) {
                continue;
            }
            for p in ps.a("HasProperties").hrefs() {
                if let Some(pl) = w.line(p)?
                    && pl.a("Name").val_str().as_deref() == Some("Key")
                    && pl.a("NominalValue").val_str().as_deref() == Some(key)
                {
                    let set = w.material_rel(id)?.and_then(|m| m.1);
                    return Ok((id, set, ty));
                }
            }
        }
    }
    let mut layers = Vec::new();
    for l in &ty.layers {
        let mat = w.material(&mat_name(l.mat))?;
        let mut args = vec![JV::H(mat), JV::Num(round6(w.u(l.t))), JV::Null];
        if w.v4 {
            args.extend([w.l(&mat_name(l.mat)), JV::Null, JV::Null, JV::Null]);
        }
        layers.push(w.make("IFCMATERIALLAYER", args)?);
    }
    let tn = type_name(&ty);
    let mut args = vec![JV::L(layers.iter().map(|&x| JV::H(x)).collect()), w.l(&tn)];
    if w.v4 {
        args.push(JV::Null);
    }
    let set = w.make("IFCMATERIALLAYERSET", args)?;
    let ps = w.pset(
        WALLTYPE_PSET,
        &[row("Key", "IFCIDENTIFIER", P::s(key)), row("Thickness", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(type_thickness(&ty)))))],
    )?;
    let g = w.guid();
    let oh = w.owner_history();
    let id = w.make(
        "IFCWALLTYPE",
        vec![g, oh, w.l(&tn), JV::Null, JV::Null, JV::L(vec![JV::H(ps)]), JV::Null, JV::Null, w.l(&tn), JV::s("STANDARD")],
    )?;
    w.associate_material(id, set)?;
    Ok((id, Some(set), ty))
}

/// What a new wall is.
#[derive(Clone, Debug, PartialEq)]
pub struct NewWall {
    pub storey: Id,
    pub a: V2,
    pub b: V2,
    pub type_key: String,
    pub base_offset: f64,
    pub top_level: String,
    pub top_offset: f64,
    pub height: f64,
    pub ext_side: String,
    pub guid: Option<String>,
}

/// `addWall(w, spec)` → (id, guid)
pub fn add_wall(w: &mut W<'_>, s: &NewWall) -> R<(Id, String)> {
    let l = dist(s.a, s.b);
    if !(l >= 1.0) {
        return Err(key_err("model.err.short"));
    }
    let st = w.line(s.storey)?;
    let Some(st) = st.filter(|x| x.class.to_ascii_lowercase().contains("buildingstorey")) else {
        return Err(crate::api::JsErr(t0("place.err.pick_storey")));
    };
    let (tid, set, ty) = wall_type_line(w, &s.type_key)?;
    let th = type_thickness(&ty);
    let ang = m::atan2(s.b[1] - s.a[1], s.b[0] - s.a[0]) * 180.0 / std::f64::consts::PI;
    let stp = st.a("ObjectPlacement").href().ok_or_else(|| crate::api::JsErr("TypeError: ObjectPlacement".into()))?;
    let lp = w.place_at(Some(stp), s.a[0], s.a[1], s.base_offset, ang)?;
    let g = w.gid(s.guid.as_deref());
    let name = type_name(&ty);
    let oh = w.owner_history();
    let mut args = vec![w.gid_jv(&g), oh, w.l(&name), JV::Null, w.l(&name), JV::H(lp), JV::Null, JV::Null];
    if w.v4 {
        args.push(JV::s("STANDARD"));
    }
    let id = w.make("IFCWALL", args)?;
    w.note_guid(&g, id);
    w.contain(id, s.storey)?;
    w.relate_type(id, tid)?;
    let us = usage(w, set, th, &s.ext_side)?;
    w.associate_material(id, us)?;
    let u = |w: &W<'_>, v: f64| P::Num(round6(w.u(v)));
    let rows: Vec<Row> = vec![
        row("TypeKey", "IFCIDENTIFIER", P::s(&s.type_key)),
        row("Length", "IFCPOSITIVELENGTHMEASURE", u(w, l)),
        row("Thickness", "IFCPOSITIVELENGTHMEASURE", u(w, th)),
        row("Height", "IFCPOSITIVELENGTHMEASURE", u(w, if s.height > 0.0 { s.height } else { 2438.4 })),
        row("BaseOffset", "IFCLENGTHMEASURE", u(w, s.base_offset)),
        row("TopLevel", "IFCIDENTIFIER", P::s(&s.top_level)),
        row("TopOffset", "IFCLENGTHMEASURE", u(w, s.top_offset)),
        row("ExteriorSide", "IFCLABEL", P::s(&s.ext_side)),
    ];
    w.upsert_pset(id, WALL_PSET, &rows)?;
    w.upsert_pset(
        id,
        "Pset_WallCommon",
        &[
            row("Reference", "IFCIDENTIFIER", P::s(&name)),
            row("IsExternal", "IFCBOOLEAN", P::Bool(ty.exterior)),
            row("LoadBearing", "IFCBOOLEAN", P::Bool(false)),
        ],
    )?;
    Ok((id, g))
}

fn usage(w: &mut W<'_>, set: Option<Id>, th: f64, ext_side: &str) -> R<Id> {
    let right = ext_side == "right";
    let mut args = vec![
        set.map_or(JV::Null, JV::H),
        JV::s("AXIS2"),
        JV::s(if right { "POSITIVE" } else { "NEGATIVE" }),
        JV::Num(round6(w.u(if right { -th / 2.0 } else { th / 2.0 }))),
    ];
    if w.v4 {
        args.push(JV::Null);
    }
    w.make("IFCMATERIALLAYERSETUSAGE", args)
}

/// What a wall change asks for (`updateWall` changes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WallChange {
    pub a: Option<V2>,
    pub b: Option<V2>,
    pub type_key: Option<String>,
    pub base_offset: Option<f64>,
    /// Some("") = a fixed height
    pub top_level: Option<String>,
    pub top_offset: Option<f64>,
    pub height: Option<f64>,
    pub ext_side: Option<String>,
}

fn dedupe(rows: Vec<Row>) -> Vec<Row> {
    let mut order: Vec<String> = Vec::new();
    let mut by: HashMap<String, Row> = HashMap::new();
    for r in rows {
        if !by.contains_key(&r.0) {
            order.push(r.0.clone());
        }
        by.insert(r.0.clone(), r);
    }
    order.into_iter().filter_map(|k| by.remove(&k)).collect()
}

/// `updateWall(w, guid, changes, { carryJoined })` → the walls changed.
pub fn update_wall(w: &mut W<'_>, guid: &str, ch: &WallChange, carry_joined: bool) -> R<Vec<String>> {
    let mut all = read_walls(w, None)?;
    let wi = all.iter().position(|x| x.guid == guid).ok_or_else(|| key_err("model.err.nowall"))?;
    let wall = all.get(wi).cloned().ok_or_else(|| key_err("model.err.nowall"))?;
    let mut touched = vec![guid.to_string()];
    let a = ch.a.unwrap_or(wall.a);
    let b = ch.b.unwrap_or(wall.b);
    if dist(a, wall.a) > 1e-6 || dist(b, wall.b) > 1e-6 {
        if carry_joined {
            for (old, to) in [(wall.a, a), (wall.b, b)] {
                if dist(old, to) <= 1e-6 {
                    continue;
                }
                for k in 0..all.len() {
                    let Some(o) = all.get(k).cloned() else { continue };
                    if o.guid == guid || o.storey != wall.storey {
                        continue;
                    }
                    let hit = if dist(o.a, old) <= 1.0 {
                        Some(false)
                    } else if dist(o.b, old) <= 1.0 {
                        Some(true)
                    } else {
                        None
                    };
                    if let Some(end_b) = hit {
                        move_end(w, &mut all, k, end_b, to)?;
                        if !touched.contains(&o.guid) {
                            touched.push(o.guid.clone());
                        }
                    }
                }
            }
        }
        if dist(a, b) < 1.0 {
            return Err(key_err("model.err.short"));
        }
        set_geom(w, &wall, a, b)?;
    }
    let mut rows: Vec<Row> = Vec::new();
    match &ch.type_key {
        Some(tk) if !tk.is_empty() && *tk != wall.type_key => {
            let (tid, set, ty) = wall_type_line(w, tk)?;
            let th = type_thickness(&ty);
            for rid in w.ids("IFCRELDEFINESBYTYPE") {
                let r = w.must(rid)?;
                let objs = r.a("RelatedObjects").hrefs();
                if !objs.contains(&wall.id) || r.a("RelatingType").href() == Some(tid) {
                    continue;
                }
                let rest: Vec<JV> = match r.a("RelatedObjects") {
                    JV::L(l) => l.iter().filter(|h| h.href() != Some(wall.id)).cloned().collect(),
                    _ => Vec::new(),
                };
                let mut l = w.touch(rid)?;
                if !rest.is_empty() {
                    l.set("RelatedObjects", JV::L(rest));
                    w.write(&mut l)?;
                } else {
                    w.api.delete(rid);
                }
            }
            w.relate_type(wall.id, tid)?;
            let name = type_name(&ty);
            let mut line = w.touch(wall.id)?;
            line.set("Name", w.l(&name));
            line.set("ObjectType", w.l(&name));
            w.write(&mut line)?;
            rows.push(row("TypeKey", "IFCIDENTIFIER", P::s(tk)));
            rows.push(row("Thickness", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(th)))));
            w.upsert_pset(
                wall.id,
                "Pset_WallCommon",
                &[row("Reference", "IFCIDENTIFIER", P::s(&name)), row("IsExternal", "IFCBOOLEAN", P::Bool(ty.exterior))],
            )?;
            let side = ch.ext_side.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| wall.ext_side.clone());
            set_usage(w, wall.id, set, th, &side)?;
        }
        _ => {
            if let Some(es) = ch.ext_side.as_ref().filter(|s| !s.is_empty() && **s != wall.ext_side)
                && let Some((_, Some(def))) = w.material_rel(wall.id)?
                && let Some(u) = w.line(def)?
                && let Some(set) = u.get("ForLayerSet").filter(|v| v.truthy()).and_then(JV::href)
            {
                set_usage(w, wall.id, Some(set), wall.t, es)?;
            }
        }
    }
    if let Some(es) = ch.ext_side.as_ref().filter(|s| !s.is_empty()) {
        rows.push(row("ExteriorSide", "IFCLABEL", P::s(if es == "right" { "right" } else { "left" })));
    }
    if let Some(bo) = ch.base_offset.filter(|v| v.is_finite()) {
        rows.push(row("BaseOffset", "IFCLENGTHMEASURE", P::Num(round6(w.u(bo)))));
        let st = w.must(wall.storey.ok_or_else(|| crate::api::JsErr("TypeError: storey".into()))?)?;
        let stp = st.a("ObjectPlacement").href().ok_or_else(|| crate::api::JsErr("TypeError: ObjectPlacement".into()))?;
        let mm = w.rel_matrix(stp, wall.lp)?;
        let ang = m::atan2(b[1] - a[1], b[0] - a[0]) * 180.0 / std::f64::consts::PI;
        if (mm[14] - bo).abs() > 1e-6 {
            w.move_placement(wall.lp, a[0], a[1], bo, ang)?;
        }
    }
    if let Some(tl) = &ch.top_level {
        rows.push(row("TopLevel", "IFCIDENTIFIER", P::s(tl)));
    }
    if let Some(to) = ch.top_offset.filter(|v| v.is_finite()) {
        rows.push(row("TopOffset", "IFCLENGTHMEASURE", P::Num(round6(w.u(to)))));
    }
    if let Some(h) = ch.height.filter(|v| v.is_finite() && *v > 0.0) {
        rows.push(row("Height", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(h)))));
        rows.push(row("TopLevel", "IFCIDENTIFIER", P::s("")));
    }
    if !rows.is_empty() {
        w.upsert_pset(wall.id, WALL_PSET, &dedupe(rows))?;
    }
    Ok(touched)
}

fn move_end(w: &mut W<'_>, all: &mut [Wall], k: usize, end_b: bool, to: V2) -> R<()> {
    let Some(x) = all.get(k).cloned() else { return Ok(()) };
    let na = if end_b { x.a } else { to };
    let nb = if end_b { to } else { x.b };
    if dist(na, nb) < 1.0 {
        return Err(key_err("model.err.short"));
    }
    set_geom(w, &x, na, nb)?;
    if let Some(y) = all.get_mut(k) {
        y.a = na;
        y.b = nb;
    }
    Ok(())
}

fn set_geom(w: &mut W<'_>, wall: &Wall, a: V2, b: V2) -> R<()> {
    let ang = m::atan2(b[1] - a[1], b[0] - a[0]) * 180.0 / std::f64::consts::PI;
    let st = w.must(wall.storey.ok_or_else(|| crate::api::JsErr("TypeError: storey".into()))?)?;
    let stp = st.a("ObjectPlacement").href().ok_or_else(|| crate::api::JsErr("TypeError: ObjectPlacement".into()))?;
    let mm = w.rel_matrix(stp, wall.lp)?;
    w.move_placement(wall.lp, a[0], a[1], mm[14], ang)?;
    w.upsert_pset(wall.id, WALL_PSET, &[row("Length", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(dist(a, b)))))])?;
    Ok(())
}

fn set_usage(w: &mut W<'_>, wall_id: Id, set: Option<Id>, th: f64, ext_side: &str) -> R<()> {
    match w.material_rel(wall_id)? {
        Some((rel, def)) => {
            let mut r = w.touch(rel)?;
            let us = usage(w, set, th, ext_side)?;
            r.set("RelatingMaterial", JV::H(us));
            w.write(&mut r)?;
            if let Some(d) = def
                && let Some(old) = w.line(d)?
                && old.class.to_ascii_uppercase().ends_with("USAGE")
            {
                w.touch(d)?;
                w.api.delete(d);
            }
        }
        None => {
            let us = usage(w, set, th, ext_side)?;
            w.associate_material(wall_id, us)?;
        }
    }
    Ok(())
}

/// Delete walls with what they host → the storeys touched.
pub fn delete_walls(w: &mut W<'_>, guids: &[String]) -> R<Vec<Id>> {
    let hix = w.host_index()?;
    let mut list = Vec::new();
    let mut storeys: Vec<Option<Id>> = Vec::new();
    let so = w.storey_of()?;
    for g in guids {
        let Some(id) = w.by_guid(g)? else { continue };
        list.push(id);
        let s = so.get(&id_key(id)).copied();
        if !storeys.contains(&s) {
            storeys.push(s);
        }
        for op in hix.voids.get(&id_key(id)).cloned().unwrap_or_default() {
            list.push(op);
            for f in hix.fills.get(&id_key(op)).cloned().unwrap_or_default() {
                list.push(f);
            }
        }
    }
    if !list.is_empty() {
        w.drop_product(&list)?;
    }
    Ok(storeys.into_iter().flatten().collect())
}

/// `regenWalls(w, storey, force)` → how many walls were rewritten.
pub fn regen_walls(w: &mut W<'_>, storey: Id, force: Option<&[String]>) -> R<usize> {
    let walls = read_walls(w, Some(storey))?;
    if walls.is_empty() {
        return Ok(0);
    }
    let segs: Vec<Seg> = walls.iter().map(Wall::seg).collect();
    let outlines = wall_outlines(&segs, 1.0);
    let forced: std::collections::HashSet<&str> = force.unwrap_or(&[]).iter().map(String::as_str).collect();
    let host = w.host_index()?;
    let mut n = 0usize;
    for wall in &walls {
        let Some(poly) = outline_of(&outlines, &wall.guid) else { continue };
        let local = to_local(wall, poly);
        if !forced.contains(wall.guid.as_str())
            && let Some(cur) = stored_shape(w, wall.id)?
            && (cur.depth - wall.height).abs() < 0.01
            && (cur.len - wall.len).abs() < 0.01
            && same_poly(&cur.points, &local)
        {
            continue;
        }
        write_wall_shape(w, wall, poly, &host)?;
        n = n.saturating_add(1);
    }
    Ok(n)
}

/// The outline in the wall's own axes.
pub fn to_local(wall: &Wall, poly: &[V2]) -> Vec<V2> {
    let ang = m::atan2(wall.b[1] - wall.a[1], wall.b[0] - wall.a[0]);
    let (c, s) = (m::cos(ang), m::sin(ang));
    poly.iter().map(|&[x, y]| [(x - wall.a[0]) * c + (y - wall.a[1]) * s, -(x - wall.a[0]) * s + (y - wall.a[1]) * c]).collect()
}

fn same_poly(a: &[V2], b: &[V2]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let n = b.len();
    for k in 0..a.len() {
        if a.iter().enumerate().all(|(i, p)| dist(*p, at(b, i.wrapping_add(k).checked_rem(n).unwrap_or(0))) < 0.01) {
            return true;
        }
    }
    false
}

pub struct Stored {
    pub points: Vec<V2>,
    pub depth: f64,
    pub len: f64,
}

/// What the wall's shape holds now.
pub fn stored_shape(w: &W<'_>, id: Id) -> R<Option<Stored>> {
    let Some(rep) = w.line(id)?.and_then(|l| l.a("Representation").href()) else { return Ok(None) };
    let Some(line) = w.line(rep)? else { return Ok(None) };
    let mut points: Option<Vec<V2>> = None;
    let mut depth = f64::NAN;
    let mut len = f64::NAN;
    let coords = |w: &W<'_>, pid: Id| -> R<Vec<f64>> {
        let p = w.must(pid)?;
        Ok(match p.a("Coordinates") {
            JV::L(l) => l.iter().map(|v| v.val().number() * w.unit_mm).collect(),
            _ => return Err(crate::api::JsErr("TypeError: Coordinates".into())),
        })
    };
    for h in line.a("Representations").hrefs() {
        let Some(r) = w.line(h)? else { continue };
        let ident = r.a("RepresentationIdentifier").val_str();
        let first = match r.a("Items") {
            JV::L(l) => l.first().and_then(JV::href),
            _ => None,
        };
        let item = match first {
            Some(i) => w.line(i)?,
            None => None,
        };
        let Some(item) = item else { continue };
        if ident.as_deref() == Some("Body") && item.get("SweptArea").is_some_and(JV::truthy) {
            depth = item.a("Depth").val().number() * w.unit_mm;
            let sa = item.a("SweptArea").href().map(|x| w.line(x)).transpose()?.flatten();
            let curve = match sa.and_then(|s| s.a("OuterCurve").href()) {
                Some(c) => w.line(c)?,
                None => None,
            };
            let mut pts = Vec::new();
            if let Some(c) = curve {
                for p in c.a("Points").hrefs() {
                    let v = coords(w, p)?;
                    pts.push([v.first().copied().unwrap_or(f64::NAN), v.get(1).copied().unwrap_or(f64::NAN)]);
                }
            }
            if pts.len() > 1 && dist(at(&pts, 0), at(&pts, pts.len().wrapping_sub(1))) < 1e-6 {
                pts.pop();
            }
            points = Some(pts);
        } else if ident.as_deref() == Some("Axis") && item.get("Points").is_some_and(JV::truthy) {
            let mut pts = Vec::new();
            for p in item.a("Points").hrefs() {
                let v = coords(w, p)?;
                pts.push([v.first().copied().unwrap_or(f64::NAN), v.get(1).copied().unwrap_or(f64::NAN)]);
            }
            len = if pts.len() > 1 { dist(at(&pts, 0), at(&pts, pts.len().wrapping_sub(1))) } else { 0.0 };
        }
    }
    Ok(points.map(|points| Stored { points, depth, len }))
}

/// The wall's Axis and Body from its outline and height; quantities with its openings taken off.
pub fn write_wall_shape(w: &mut W<'_>, wall: &Wall, poly: &[V2], host: &HostIndex) -> R<()> {
    let local = to_local(wall, poly);
    let pl = w.polyline2(&[[0.0, 0.0], [wall.len, 0.0]], false)?;
    let axis = w.rep("axis", "Axis", "Curve2D", &[pl])?;
    let prof = w.profile(&local, None)?;
    let ex = w.extrude(prof, 0.0, wall.height)?;
    let body = w.rep("body", "Body", "SweptSolid", &[ex])?;
    w.set_shape(wall.id, &[axis, body])?;
    let foot = poly_area(&local).abs();
    let mut open_area = 0.0;
    let mut open_vol = 0.0;
    for op in host.voids.get(&id_key(wall.id)).cloned().unwrap_or_default() {
        let Some(q) = w.read_pset(op, "CadCraft_Opening")? else { continue };
        let ow = ps_num(&q, "Width") * w.unit_mm;
        let oh = ps_num(&q, "Height") * w.unit_mm;
        if ow > 0.0 && oh > 0.0 {
            open_area += ow * oh;
            open_vol += ow * oh * wall.t;
        }
    }
    let q: Vec<QRow> = vec![
        ("Length".into(), "length", wall.len),
        ("Width".into(), "length", wall.t),
        ("Height".into(), "length", wall.height),
        ("GrossFootprintArea".into(), "area", foot),
        ("GrossSideArea".into(), "area", wall.len * wall.height),
        ("NetSideArea".into(), "area", m::max(0.0, wall.len * wall.height - open_area)),
        ("GrossVolume".into(), "volume", foot * wall.height),
        ("NetVolume".into(), "volume", m::max(0.0, foot * wall.height - open_vol)),
    ];
    w.upsert_qto(wall.id, "Qto_WallBaseQuantities", &q)?;
    let mut rows = vec![
        row("Length", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(wall.len)))),
        row("Thickness", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(wall.t)))),
    ];
    if !wall.top_level.is_empty() {
        rows.push(row("Height", "IFCPOSITIVELENGTHMEASURE", P::Num(round6(w.u(wall.height)))));
    }
    w.upsert_pset(wall.id, WALL_PSET, &rows)?;
    Ok(())
}

/// Degrees → radians (for the JS's `ang * D2R`).
pub fn rad(deg: f64) -> f64 {
    deg * D2R
}
