//! res-terrain.js (0.5 part 4, RC.10), the parts the walls move: the terrain read back from its pset (the existing
//! ground's points and faces), building pads round the walls, the finished grade (pads flattened in and sloped out),
//! the terrain's closed body, the pads' footprints, the paving draped on the new grade, `groundAt` for footings that
//! step to the grade, and `syncTerrain`. The TIN is cc-civil's port of tin.js (the same file in BimCraft and
//! CivilCraft).
//!
//! The JS loads tin.js on first use (res-ui.js, as soon as a model with a terrain shows on the plan): until then
//! nothing of the ground is made again. [`set_ground_loaded`] is that moment here.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use cc_civil::geo::{P2, P3};
use cc_civil::tin::{Breakline, Surface, ZPt, build_surface, surface_from_faces};
use cc_io_ifc::model::Id;

use crate::api::R;
use crate::draw::levels::Level;
use crate::draw::m::{self, V2, at, hypot, in_poly, nx, pv};
use crate::draw::res::{levels_or, num, s_or};
use crate::draw::rooms::{loop_at, plan_loops};
use crate::draw::slabs::{K, shape_key_scaled};
use crate::draw::tr::t0;
use crate::draw::walls::{Seg, read_walls, wall_outlines};
use crate::draw::writer::{QRow, W, inv4j, round_d, round6, row, scale_t};
use crate::util::{M4, apply4};
use crate::wapi::{JV, P};

pub const TERRAIN_PSET: &str = "CadCraft_Terrain";
pub const PAD_PSET: &str = "CadCraft_Pad";
pub const HARD_PSET: &str = "CadCraft_Hardscape";
pub const HARD_KINDS: [&str; 3] = ["driveway", "walk", "patio"];

static TIN: AtomicBool = AtomicBool::new(false);

/// tin.js has been loaded on this page (res-ui.js `ensureTin`): the ground follows the walls from now on.
pub fn set_ground_loaded(on: bool) {
    TIN.store(on, Ordering::Relaxed);
}
pub fn ground_loaded() -> bool {
    TIN.load(Ordering::Relaxed)
}

/// `readTerrain(w)`'s terrain.
#[derive(Clone, Debug)]
pub struct Terrain {
    pub id: Id,
    pub guid: String,
    pub pts: Vec<P3>,
    pub faces: Vec<Vec<f64>>,
    pub key: String,
}

fn site_of(w: &W<'_>) -> Option<Id> {
    w.ids("IFCSITE").first().copied()
}

/// `unpack(text, n)`: "a,b,c;…" → rows of n finite numbers.
fn unpack(text: &str, n: usize) -> Vec<Vec<f64>> {
    text.split(';')
        .filter(|r| !r.is_empty())
        .map(|r| r.split(',').map(crate::js::str_num).collect::<Vec<f64>>())
        .filter(|r| r.len() == n && r.iter().all(|v| v.is_finite()))
        .collect()
}

/// `readTerrain(w)`: the first IfcGeographicElement with CadCraft_Terrain.
pub fn read_terrain(w: &mut W<'_>) -> R<Option<Terrain>> {
    for id in w.ids("IFCGEOGRAPHICELEMENT") {
        let Some(ps) = w.read_pset(id, TERRAIN_PSET)? else { continue };
        let l = w.must(id)?;
        let pts = unpack(&s_or(&ps, "Existing"), 3)
            .into_iter()
            .filter_map(|r| match r.as_slice() {
                [x, y, z] => Some(P3 { x: *x, y: *y, z: *z }),
                _ => None,
            })
            .collect();
        let faces = unpack(&s_or(&ps, "Faces"), 3);
        return Ok(Some(Terrain { id, guid: l.a("GlobalId").val_str().unwrap_or_default(), pts, faces, key: s_or(&ps, "ShapeKey") }));
    }
    Ok(None)
}

/// `existingSurface(tr)`
pub fn existing_surface(t: &Terrain) -> Option<Surface> {
    if t.pts.len() < 3 {
        return None;
    }
    let pts: Vec<Option<P3>> = t.pts.iter().map(|p| Some(*p)).collect();
    surface_from_faces(&pts, &t.faces).surface
}

/// A building pad (`readPads`).
#[derive(Clone, Debug)]
pub struct Pad {
    pub id: Id,
    pub level: String,
    pub seed: V2,
    pub margin: f64,
    pub slope: f64,
    pub depth: f64,
}

pub fn read_pads(w: &mut W<'_>) -> R<Vec<Pad>> {
    let mut out = Vec::new();
    let um = w.unit_mm;
    for id in w.ids("IFCGEOGRAPHICELEMENT") {
        let Some(ps) = w.read_pset(id, PAD_PSET)? else { continue };
        let slope = num(&ps, "Slope");
        out.push(Pad {
            id,
            level: s_or(&ps, "Level"),
            seed: [num(&ps, "SeedX") * um, num(&ps, "SeedY") * um],
            margin: num(&ps, "Margin") * um,
            slope: if slope != 0.0 && !slope.is_nan() { slope } else { 2.0 },
            depth: num(&ps, "Depth") * um,
        });
    }
    Ok(out)
}

/// Paving (`readHardscape`).
#[derive(Clone, Debug)]
pub struct Hard {
    pub id: Id,
    pub level: String,
    pub kind: String,
    pub points: Vec<V2>,
    pub thickness: f64,
}

pub fn read_hardscape(w: &mut W<'_>) -> R<Vec<Hard>> {
    let mut out = Vec::new();
    let um = w.unit_mm;
    for id in w.ids("IFCSLAB") {
        let Some(ps) = w.read_pset(id, HARD_PSET)? else { continue };
        let points = s_or(&ps, "Points")
            .split(';')
            .map(|q| q.split(',').map(crate::js::str_num).collect::<Vec<f64>>())
            .filter_map(|q| match q.as_slice() {
                [x, y] if x.is_finite() && y.is_finite() => Some([x * um, y * um]),
                _ => None,
            })
            .collect();
        let k = s_or(&ps, "Kind");
        out.push(Hard {
            id,
            level: s_or(&ps, "Level"),
            kind: if HARD_KINDS.contains(&k.as_str()) { k } else { "walk".into() },
            points,
            thickness: num(&ps, "Thickness") * um,
        });
    }
    Ok(out)
}

/// `padShape(w, pd, levels)`: the outline round its seed out by the margin (world mm), its level's z less the depth.
pub struct PadShape {
    pub poly: Vec<V2>,
    pub z: f64,
    pub slope: f64,
}

pub fn pad_shape(w: &mut W<'_>, pd: &Pad, levels: &[Level]) -> R<Option<PadShape>> {
    let Some(lv) = levels.iter().find(|l| l.guid.as_deref() == Some(pd.level.as_str())) else { return Ok(None) };
    let walls = read_walls(w, Some(lv.id))?;
    let segs: Vec<Seg> = walls.iter().map(|x| x.seg()).collect();
    let polys: Vec<Vec<V2>> = wall_outlines(&segs, 1.0).into_iter().map(|(_, p)| p).collect();
    let l = plan_loops(&polys);
    let Some(o) = loop_at(&l.outlines, pd.seed) else { return Ok(None) };
    let mm = w.storey_world(lv.id)?;
    let poly = offset_out(&o.poly, pd.margin)
        .into_iter()
        .map(|p| {
            let q = apply4(&mm, p[0], p[1], 0.0);
            [q[0], q[1]]
        })
        .collect();
    Ok(Some(PadShape { poly, z: mm[14] - pd.depth, slope: pd.slope }))
}

fn seg_dist(p: V2, a: V2, b: V2) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let l2 = if l2 != 0.0 && !l2.is_nan() { l2 } else { 1.0 };
    let t = m::max(0.0, m::min(1.0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2));
    hypot(p[0] - a[0] - dx * t, p[1] - a[1] - dy * t)
}
fn dist_to(p: V2, poly: &[V2]) -> f64 {
    if in_poly(p[0], p[1], poly) {
        return 0.0;
    }
    let n = poly.len();
    m::min_of((0..n).map(|i| seg_dist(p, at(poly, i), at(poly, nx(i, n)))))
}
fn offset_out(p: &[V2], d: f64) -> Vec<V2> {
    let n = p.len();
    let e: Vec<(V2, V2, V2)> = (0..n)
        .map(|i| {
            let (a, b) = (at(p, i), at(p, nx(i, n)));
            let l = hypot(b[0] - a[0], b[1] - a[1]);
            let l = if l != 0.0 && !l.is_nan() { l } else { 1.0 };
            let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
            (a, u, [-u[1], u[0]])
        })
        .collect();
    (0..n)
        .filter_map(|i| {
            let (ea, eu, en) = *e.get(i)?;
            let (fa, fu, fnn) = *e.get(pv(i, n))?;
            let p0 = [fa[0] - fnn[0] * d, fa[1] - fnn[1] * d];
            let p1 = [ea[0] - en[0] * d, ea[1] - en[1] * d];
            let c = fu[0] * eu[1] - fu[1] * eu[0];
            if c.abs() < 1e-9 {
                return Some(p1);
            }
            let t = ((p1[0] - p0[0]) * eu[1] - (p1[1] - p0[1]) * eu[0]) / c;
            Some([p0[0] + fu[0] * t, p0[1] + fu[1] * t])
        })
        .collect()
}

/// `gradeAt(p, e, pads)`
pub fn grade_at(p: V2, e: f64, pads: &[PadShape]) -> f64 {
    let mut z = e;
    for pd in pads {
        let d = dist_to(p, &pd.poly);
        let r = d / pd.slope;
        z = m::max(pd.z - r, m::min(pd.z + r, z));
    }
    z
}

fn area_p2(p: &[P2]) -> f64 {
    let n = p.len();
    let mut s = 0.0;
    for i in 0..n {
        let (Some(a), Some(b)) = (p.get(i), p.get(nx(i, n))) else { continue };
        s += a.x * b.y - a.y * b.x;
    }
    s / 2.0
}

/// `gradeSurface(existing, pads)` → the finished surface (None: the existing one stands).
pub fn grade_surface(existing: &Surface, pads: &[PadShape]) -> Option<Surface> {
    if pads.is_empty() {
        return None;
    }
    let step = 500.0;
    let (pts, _faces) = existing.to_faces();
    let mut out: Vec<ZPt> = Vec::with_capacity(pts.len());
    for p in &pts {
        out.push(ZPt { x: p.x, y: p.y, z: Some(grade_at([p.x, p.y], p.z, pads)) });
    }
    let mut lines: Vec<Breakline> = Vec::new();
    let s = existing.stats().clone();
    for pd in pads {
        let mut ring_pts: Vec<P3> = pd.poly.iter().map(|q| P3 { x: q[0], y: q[1], z: pd.z }).collect();
        if let Some(f) = pd.poly.first() {
            ring_pts.push(P3 { x: f[0], y: f[1], z: pd.z });
        }
        lines.push(Breakline { name: String::new(), pts: ring_pts, from_group: false });
        // rings round the pad out to where the slope can meet the ground
        let reach = m::max((s.max_z - pd.z).abs(), (pd.z - s.min_z).abs()) * pd.slope + step;
        let mut d = step;
        let mut guard = 0u32;
        while d <= reach && guard < 100_000 {
            guard = guard.saturating_add(1);
            let ring = offset_out(&pd.poly, d);
            let rn = ring.len();
            let mut l = 0.0;
            for i in 0..rn {
                let (q, b) = (at(&ring, i), at(&ring, nx(i, rn)));
                l += hypot(b[0] - q[0], b[1] - q[1]);
            }
            let n = m::max(rn as f64, (l / step).ceil());
            let mut k = 0.0;
            while k < n && k < 1_000_000.0 {
                let tt = k / n * l;
                let mut acc = 0.0;
                let mut q = at(&ring, 0);
                for i in 0..rn {
                    let (a, b) = (at(&ring, i), at(&ring, nx(i, rn)));
                    let li = hypot(b[0] - a[0], b[1] - a[1]);
                    if acc + li >= tt {
                        let f = (tt - acc) / if li != 0.0 && !li.is_nan() { li } else { 1.0 };
                        q = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
                        break;
                    }
                    acc += li;
                }
                k += 1.0;
                let Some(e) = existing.z_at(q[0], q[1]) else { continue };
                out.push(ZPt { x: q[0], y: q[1], z: Some(grade_at(q, e, pads)) });
            }
            d += step;
        }
    }
    let mut outl = existing.outline();
    outl.sort_by(|a, b| crate::js::sign_ord(area_p2(b).abs() - area_p2(a).abs()));
    let bnd = outl.into_iter().next();
    build_surface(&out, &lines, bnd.as_deref(), 0.0).surface
}

/// `cutFill(existing, finished)` → (cut, fill) in m³.
fn cut_fill(existing: &Surface, finished: &Surface) -> (f64, f64) {
    let cell = 250.0;
    let b = existing.stats().bounds;
    let (mut cut, mut fill) = (0.0, 0.0);
    let mut x = b.min_x + cell / 2.0;
    let mut guard = 0u64;
    while x < b.max_x && guard < 50_000_000 {
        let mut y = b.min_y + cell / 2.0;
        while y < b.max_y && guard < 50_000_000 {
            guard = guard.saturating_add(1);
            if let (Some(e), Some(f)) = (existing.z_at(x, y), finished.z_at(x, y)) {
                let d = (f - e) * cell * cell;
                if d > 0.0 {
                    fill += d;
                } else {
                    cut -= d;
                }
            }
            y += cell;
        }
        x += cell;
    }
    (cut / 1e9, fill / 1e9)
}

/// World mm → the site's own axes.
fn site_map(w: &mut W<'_>) -> R<Option<M4>> {
    let Some(site) = site_of(w) else { return Ok(None) };
    let sl = w.must(site)?;
    let Some(lp) = sl.a("ObjectPlacement").href() else { return Ok(None) };
    let mw = scale_t(&w.api.world(lp)?, w.unit_mm);
    Ok(Some(inv4j(&mw)?))
}
fn smap(s: Option<&M4>, x: f64, y: f64, z: f64) -> [f64; 3] {
    match s {
        Some(mi) => apply4(mi, x, y, z),
        None => [x, y, z],
    }
}

/// `closedBody(w, surface, base, lift, thickness)` → an IfcTriangulatedFaceSet.
fn closed_body(w: &mut W<'_>, surface: &Surface, base: f64, lift: f64, thickness: Option<f64>) -> R<Id> {
    let (pts, faces) = surface.to_faces();
    let n = pts.len();
    let top: Vec<[f64; 3]> = pts.iter().map(|p| [p.x, p.y, p.z + lift]).collect();
    let bot: Vec<[f64; 3]> = top
        .iter()
        .map(|p| {
            let z = match thickness {
                Some(t) => p[2] - t,
                None => base,
            };
            [p[0], p[1], z]
        })
        .collect();
    let mut tri: Vec<[usize; 3]> = Vec::new();
    for f in &faces {
        let (a, b, c) = (f[0] as usize, f[1] as usize, f[2] as usize);
        tri.push([a, b, c]);
        tri.push([n.saturating_add(a), n.saturating_add(c), n.saturating_add(b)]);
    }
    // walls round every boundary loop
    let mut order: Vec<(u32, u32)> = Vec::new();
    let mut count: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    for f in &faces {
        for k in 0..3usize {
            let e = (f.get(k).copied().unwrap_or(0), f.get(nx(k, 3)).copied().unwrap_or(0));
            if count.insert(e) {
                order.push(e);
            }
        }
    }
    for &(a, b) in &order {
        if !count.contains(&(b, a)) {
            let (a, b) = (a as usize, b as usize);
            tri.push([b, a, n.saturating_add(a)]);
            tri.push([b, n.saturating_add(a), n.saturating_add(b)]);
        }
    }
    let s = site_map(w)?;
    let mut coords = Vec::with_capacity(n.saturating_mul(2));
    for c in top.iter().chain(bot.iter()) {
        let q = smap(s.as_ref(), c[0], c[1], c[2]);
        coords.push(JV::L(q.iter().map(|v| JV::Num(round_d(w.u(*v), 3))).collect()));
    }
    let list = if w.x3 {
        w.make("IFCCARTESIANPOINTLIST3D", vec![JV::L(coords), JV::Null])?
    } else {
        w.make("IFCCARTESIANPOINTLIST3D", vec![JV::L(coords)])?
    };
    let idx: Vec<JV> =
        tri.iter().map(|t3| JV::L(t3.iter().map(|&i| w.t("IFCPOSITIVEINTEGER", P::Num(i.saturating_add(1) as f64))).collect())).collect();
    let closed = w.t("IFCBOOLEAN", P::Bool(true));
    w.make("IFCTRIANGULATEDFACESET", vec![JV::H(list), JV::Null, closed, JV::L(idx), JV::Null])
}

fn pad_key(p: &PadShape) -> K {
    K::A(vec![crate::draw::slabs::kpts(&p.poly), K::N(p.z), K::N(p.slope)])
}
fn p3_key(p: Option<&P3>) -> K {
    match p {
        Some(p) => K::O(vec![("x", K::N(p.x)), ("y", K::N(p.y)), ("z", K::N(p.z))]),
        None => K::Null,
    }
}

/// `writeTerrain(w, trn, { levels })`: the terrain's body again — the existing ground with every pad graded in.
fn write_terrain(w: &mut W<'_>, trn: &Terrain, levels: &[Level]) -> R<bool> {
    let Some(existing) = existing_surface(trn) else { return Ok(false) };
    let mut pads = Vec::new();
    for pd in read_pads(w)? {
        if let Some(s) = pad_shape(w, &pd, levels)? {
            pads.push(s);
        }
    }
    let key = shape_key_scaled(
        vec![
            K::N(trn.pts.len() as f64),
            K::N(trn.faces.len() as f64),
            p3_key(trn.pts.first()),
            p3_key(trn.pts.last()),
            K::A(pads.iter().map(pad_key).collect()),
        ],
        10.0,
    );
    if trn.key == key {
        return Ok(false);
    }
    let graded = grade_surface(&existing, &pads);
    let finished = graded.as_ref().unwrap_or(&existing);
    let s = existing.stats().clone();
    let base = m::min(s.min_z, finished.stats().min_z) - 1000.0;
    let body = closed_body(w, finished, base, 0.0, None)?;
    let st = w.surface_style(&t0("res.style.ground"), [0.55, 0.62, 0.42], 0.0)?;
    w.style_item(body, st)?;
    let rep = w.rep("body", "Body", "Tessellation", &[body])?;
    w.set_shape(trn.id, &[rep])?;
    let (cut, fill) = if pads.is_empty() { (0.0, 0.0) } else { cut_fill(&existing, finished) };
    w.upsert_pset(
        trn.id,
        TERRAIN_PSET,
        &[
            row("ShapeKey", "IFCIDENTIFIER", P::s(&key)),
            row("Cut", "IFCVOLUMEMEASURE", P::Num(round_d(cut, 3))),
            row("Fill", "IFCVOLUMEMEASURE", P::Num(round_d(fill, 3))),
            row("MinZ", "IFCLENGTHMEASURE", P::Num(round6(w.u(s.min_z)))),
            row("MaxZ", "IFCLENGTHMEASURE", P::Num(round6(w.u(s.max_z)))),
        ],
    )?;
    // each pad: its footprint and its share of the cut / fill
    let np = m::max(1.0, pads.len() as f64);
    for pd in read_pads(w)? {
        let Some(sh) = pad_shape(w, &pd, levels)? else { continue };
        let sm = site_map(w)?;
        let pts: Vec<V2> = sh
            .poly
            .iter()
            .map(|p| {
                let q = smap(sm.as_ref(), p[0], p[1], 0.0);
                [q[0], q[1]]
            })
            .collect();
        let pl = w.polyline2(&pts, true)?;
        let cs = w.make("IFCGEOMETRICCURVESET", vec![JV::L(vec![JV::H(pl)])])?;
        let rep = w.rep("foot", "FootPrint", "GeometricCurveSet", &[cs])?;
        w.set_shape(pd.id, &[rep])?;
        w.upsert_pset(
            pd.id,
            PAD_PSET,
            &[
                row("PadZ", "IFCLENGTHMEASURE", P::Num(round6(w.u(sh.z)))),
                row("Cut", "IFCVOLUMEMEASURE", P::Num(round_d(cut / np, 3))),
                row("Fill", "IFCVOLUMEMEASURE", P::Num(round_d(fill / np, 3))),
            ],
        )?;
    }
    // hardscape drapes on the new grade
    for h in read_hardscape(w)? {
        write_hardscape(w, &h, finished, levels)?;
    }
    Ok(true)
}

/// `writeHardscape(w, h, finished, levels)`
fn write_hardscape(w: &mut W<'_>, h: &Hard, finished: &Surface, levels: &[Level]) -> R<bool> {
    let Some(lv) = levels.iter().find(|l| l.guid.as_deref() == Some(h.level.as_str())) else { return Ok(false) };
    if h.points.len() < 3 {
        return Ok(false);
    }
    let mm = w.storey_world(lv.id)?;
    let world: Vec<V2> = h
        .points
        .iter()
        .map(|p| {
            let q = apply4(&mm, p[0], p[1], 0.0);
            [q[0], q[1]]
        })
        .collect();
    let mut pts: Vec<ZPt> = Vec::new();
    for p in &world {
        if let Some(z) = finished.z_at(p[0], p[1]) {
            pts.push(ZPt { x: p[0], y: p[1], z: Some(z) });
        }
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in &world {
        x0 = m::min(x0, p[0]);
        y0 = m::min(y0, p[1]);
        x1 = m::max(x1, p[0]);
        y1 = m::max(y1, p[1]);
    }
    let step = m::max(500.0, ((x1 - x0) * (y1 - y0) / 2000.0).sqrt());
    let mut x = x0 + step / 2.0;
    let mut guard = 0u64;
    while x < x1 && guard < 10_000_000 {
        let mut y = y0 + step / 2.0;
        while y < y1 && guard < 10_000_000 {
            guard = guard.saturating_add(1);
            if in_poly(x, y, &world)
                && let Some(z) = finished.z_at(x, y)
            {
                pts.push(ZPt { x, y, z: Some(z) });
            }
            y += step;
        }
        x += step;
    }
    let bnd: Vec<P2> = world.iter().map(|p| P2 { x: p[0], y: p[1] }).collect();
    let Some(surface) = build_surface(&pts, &[], Some(&bnd), 0.0).surface else { return Ok(false) };
    let body = closed_body(w, &surface, 0.0, 10.0, Some(h.thickness))?;
    let color = if h.kind == "driveway" { [0.62, 0.62, 0.6] } else { [0.78, 0.76, 0.72] };
    let st = w.surface_style(&t0(&format!("res.style.paving.{}", h.kind)), color, 0.0)?;
    w.style_item(body, st)?;
    let rep = w.rep("body", "Body", "Tessellation", &[body])?;
    w.set_shape(h.id, &[rep])?;
    let s = surface.stats();
    let q: Vec<QRow> = vec![
        ("Width".into(), "length", h.thickness),
        ("GrossArea".into(), "area", s.area2d),
        ("NetArea".into(), "area", s.area2d),
        ("GrossVolume".into(), "volume", s.area3d * h.thickness),
    ];
    w.upsert_qto(h.id, "Qto_SlabBaseQuantities", &q)?;
    Ok(true)
}

/// `finishedSurface(w, trn)`
fn finished_surface(w: &mut W<'_>, trn: &Terrain) -> R<Option<Surface>> {
    let Some(ex) = existing_surface(trn) else { return Ok(None) };
    let levels = crate::draw::levels::read_levels(w)?;
    let mut pads = Vec::new();
    for pd in read_pads(w)? {
        if let Some(s) = pad_shape(w, &pd, &levels)? {
            pads.push(s);
        }
    }
    Ok(Some(grade_surface(&ex, &pads).unwrap_or(ex)))
}

thread_local! {
    /// res-terrain.js `groundCache`: the finished surface by the terrain's shape key.
    static GROUND: RefCell<HashMap<String, Option<Rc<Surface>>>> = RefCell::new(HashMap::new());
}

/// The finished grade (`groundAt`'s surface): None when the ground isn't loaded or there is no terrain; Some(None)
/// when the terrain has no surface.
pub fn ground(w: &mut W<'_>) -> R<Option<Option<Rc<Surface>>>> {
    if !ground_loaded() {
        return Ok(None);
    }
    let Some(trn) = read_terrain(w)? else { return Ok(None) };
    if let Some(s) = GROUND.with(|g| g.borrow().get(&trn.key).cloned()) {
        return Ok(Some(s));
    }
    let s = finished_surface(w, &trn)?.map(Rc::new);
    GROUND.with(|g| {
        let mut g = g.borrow_mut();
        if g.len() > 8 {
            g.clear();
        }
        g.insert(trn.key.clone(), s.clone());
    });
    Ok(Some(s))
}

/// `groundAt(w, x, y)` on a surface from [`ground`].
pub fn ground_at(g: Option<&Option<Rc<Surface>>>, x: f64, y: f64) -> Option<f64> {
    g?.as_ref()?.z_at(x, y)
}

/// `syncTerrain(w, storey, ctx)`: pads on this level (and so the terrain and the paving) follow its walls.
pub fn sync_terrain(w: &mut W<'_>, storey: Id, levels: Option<&[Level]>) -> R<usize> {
    if !ground_loaded() {
        return Ok(0);
    }
    let Some(trn) = read_terrain(w)? else { return Ok(0) };
    let levels = levels_or(w, levels)?;
    let pads = read_pads(w)?;
    if !pads.iter().any(|p| levels.iter().find(|l| l.guid.as_deref() == Some(p.level.as_str())).is_some_and(|l| l.id == storey)) {
        return Ok(0);
    }
    Ok(usize::from(write_terrain(w, &trn, &levels)?))
}
