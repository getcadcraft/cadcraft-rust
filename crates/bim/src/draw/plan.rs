//! model-plan.js and the scene half of model-ui.js — what a level's plan view shows, in the storey's own plan axes
//! (mm, x right, y up): elements cut by the plane 1200 mm above the level (from the rebuilt model's meshes), feature
//! edges of what lies below, the drawn walls from their joined outlines with the openings the cut passes through
//! taken out, door / window symbols, room labels, roofs' eaves and ridges, and dimensions.

use cc_io_ifc::model::Id;

use crate::draw::m::{self, V2, at, nx};
use crate::draw::openings::{OpSpec, Symbol, opening_symbol};
use crate::draw::ops::Snapshot;
use crate::draw::roof::{edge_kinds, roof_defaults, roof_geometry};
use crate::draw::rooms::{Loops, label_point, loop_at, plan_loops};
use crate::draw::units::{fmt_area, fmt_len};
use crate::draw::walls::{Wall, outline_of, wall_outlines, wall_type};
use crate::js::OMap;
use crate::model::Model;
use crate::util::{M4, inv4, mul4};

/// Where the plan cuts: 1200 mm above the level (4'-0").
pub const CUT: f64 = 1200.0;

/// viewer (Y up, m) → model mm (Z up, file axes) → storey plan (mm).
pub fn viewer_to_plan(c: &M4, storey_world_mm: &M4) -> Option<M4> {
    let s: M4 = [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1000.0, 0.0, 0.0, -1000.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    Some(mul4(&inv4(storey_world_mm)?, &mul4(&s, &inv4(c)?)))
}

/// What planCut found: cut segments and the edges below, per element key (first-seen order), and their bounds.
#[derive(Clone, Debug, Default)]
pub struct Cut {
    pub cut: OMap<u64, Vec<f64>>,
    pub below: OMap<u64, Vec<f64>>,
    pub bounds: Option<[f64; 4]>,
}

/// `planCut(model, { M, storey, cutZ, skip, depth, edges })`
pub fn plan_cut(model: &Model, mm: &M4, storey: Id, cut_z: f64, skip: &std::collections::HashSet<u64>, depth: f64, edges: bool) -> Cut {
    let mut out = Cut::default();
    let mut bb = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut grow = |x: f64, y: f64| {
        if x < bb[0] {
            bb[0] = x;
        }
        if x > bb[2] {
            bb[2] = x;
        }
        if y < bb[1] {
            bb[1] = y;
        }
        if y > bb[3] {
            bb[3] = y;
        }
    };
    for b in &model.batches {
        if b.space || b.storey != storey {
            continue;
        }
        let p = &b.positions;
        let n = p.as_chunks::<3>().0.len();
        let (mut xs, mut ys, mut zs) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
        for v in p.as_chunks::<3>().0 {
            let [x, y, z] = v.map(f64::from);
            xs.push(mm[0] * x + mm[4] * y + mm[8] * z + mm[12]);
            ys.push(mm[1] * x + mm[5] * y + mm[9] * z + mm[13]);
            zs.push(mm[2] * x + mm[6] * y + mm[10] * z + mm[14]);
        }
        let e = &b.elems;
        for r in 0..e.ids.len() {
            let Some(&key) = e.ids.get(r) else { continue };
            let k = u64::from(key);
            if skip.contains(&k) {
                continue;
            }
            let (t0, tn, v0, vn) = (
                e.t0.get(r).copied().unwrap_or(0),
                e.tn.get(r).copied().unwrap_or(0),
                e.v0.get(r).copied().unwrap_or(0),
                e.vn.get(r).copied().unwrap_or(0),
            );
            let (mut zmin, mut zmax) = (f64::INFINITY, f64::NEG_INFINITY);
            for v in v0..v0.saturating_add(vn) {
                let z = usize::try_from(v).ok().and_then(|v| zs.get(v)).copied().unwrap_or(f64::NAN);
                if z < zmin {
                    zmin = z;
                }
                if z > zmax {
                    zmax = z;
                }
            }
            if zmin <= cut_z && zmax >= cut_z {
                let mut seg: Vec<f64> = Vec::new();
                for t in t0..t0.saturating_add(tn) {
                    let i = usize::try_from(t).unwrap_or(usize::MAX).saturating_mul(3);
                    let (Some(&a), Some(&c), Some(&d)) =
                        (b.indices.get(i), b.indices.get(i.saturating_add(1)), b.indices.get(i.saturating_add(2)))
                    else {
                        continue;
                    };
                    let u = |x: u32| usize::try_from(x).unwrap_or(usize::MAX);
                    tri_cut(&xs, &ys, &zs, u(a), u(c), u(d), cut_z, &mut seg);
                }
                if !seg.is_empty() {
                    for &[x, y] in seg.as_chunks::<2>().0 {
                        grow(x, y);
                    }
                    match out.cut.get_mut(&k) {
                        Some(v) => v.extend(seg),
                        None => out.cut.set(k, seg),
                    }
                }
            } else if edges && zmax < cut_z && zmax > -depth {
                let mut raw = Vec::new();
                crate::vis::run_edges(p, &b.indices, t0, tn, 30.0, &mut raw);
                let mut flat = Vec::with_capacity(raw.len());
                for &[x, y, z] in raw.as_chunks::<3>().0 {
                    flat.push(mm[0] * x + mm[4] * y + mm[8] * z + mm[12]);
                    flat.push(mm[1] * x + mm[5] * y + mm[9] * z + mm[13]);
                }
                let mut keep = Vec::new();
                for q in flat.as_chunks::<4>().0 {
                    let [x0, y0, x1, y1] = *q;
                    if m::hypot(x1 - x0, y1 - y0) > 0.5 {
                        keep.extend_from_slice(q);
                        grow(x0, y0);
                    }
                }
                if !keep.is_empty() {
                    match out.below.get_mut(&k) {
                        Some(v) => v.extend(keep),
                        None => out.below.set(k, keep),
                    }
                }
            }
        }
    }
    out.bounds = bb[0].is_finite().then_some(bb);
    out
}

/// One triangle against z = h: the segment where they cross, its ends ordered by position (as an edge shared by two
/// triangles gives the same point from either).
#[allow(clippy::too_many_arguments)]
fn tri_cut(xs: &[f64], ys: &[f64], zs: &[f64], a: usize, b: usize, c: usize, h: f64, out: &mut Vec<f64>) {
    let g = |v: &[f64], i: usize| v.get(i).copied().unwrap_or(f64::NAN);
    let v = [a, b, c];
    let above = v.map(|i| g(zs, i) > h);
    let n = above.iter().filter(|x| **x).count();
    if n == 0 || n == 3 {
        return;
    }
    let mut pts: Vec<f64> = Vec::with_capacity(4);
    for (k, (&p, &q)) in v.iter().zip(v.iter().cycle().skip(1)).enumerate() {
        let (ak, ak1) = (above.get(k).copied().unwrap_or(false), above.get(nx(k, 3)).copied().unwrap_or(false));
        if ak == ak1 {
            continue;
        }
        let (xp, xq, yp, yq, zp, zq) = (g(xs, p), g(xs, q), g(ys, p), g(ys, q), g(zs, p), g(zs, q));
        let first = xp < xq || (xp == xq && (yp < yq || (yp == yq && zp < zq)));
        let (i, j) = if first { (p, q) } else { (q, p) };
        let t = (h - g(zs, i)) / (g(zs, j) - g(zs, i));
        pts.push(g(xs, i) + (g(xs, j) - g(xs, i)) * t);
        pts.push(g(ys, i) + (g(ys, j) - g(ys, i)) * t);
    }
    if let [x0, y0, x1, y1] = pts[..]
        && (x0 != x1 || y0 != y1)
    {
        out.extend_from_slice(&pts);
    }
}

/// Join cut segments into closed loops (points match to 0.01 mm).
pub fn chain_loops(segs: &[f64]) -> Vec<Vec<f64>> {
    let key = |x: f64, y: f64| format!("{},{}", crate::js::num_str(m::round(x * 100.0)), crate::js::num_str(m::round(y * 100.0)));
    let sg: &[[f64; 4]] = segs.as_chunks::<4>().0;
    let n = sg.len();
    let end = |h: usize| -> (f64, f64) {
        let q = sg.get(h >> 1).copied().unwrap_or([f64::NAN; 4]);
        if h & 1 == 0 { (q[0], q[1]) } else { (q[2], q[3]) }
    };
    let mut adj: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
    for i in 0..n {
        for e in 0..2usize {
            let h = i.wrapping_mul(2).wrapping_add(e);
            let (x, y) = end(h);
            adj.entry(key(x, y)).or_default().push(h);
        }
    }
    let mut used = vec![false; n];
    let mut loops = Vec::new();
    for st in 0..n {
        if used.get(st).copied().unwrap_or(true) {
            continue;
        }
        if let Some(u) = used.get_mut(st) {
            *u = true;
        }
        let q0 = sg.get(st).copied().unwrap_or([f64::NAN; 4]);
        let mut lp = vec![q0[0], q0[1]];
        let start_k = key(q0[0], q0[1]);
        let (mut x, mut y) = (q0[2], q0[3]);
        for _ in 0..n.saturating_add(2) {
            let k = key(x, y);
            lp.push(x);
            lp.push(y);
            if k == start_k {
                break;
            }
            let next = adj.get(&k).and_then(|v| v.iter().copied().find(|h| !used.get(h >> 1).copied().unwrap_or(true)));
            let Some(h) = next else { break };
            if let Some(u) = used.get_mut(h >> 1) {
                *u = true;
            }
            (x, y) = end(h ^ 1);
        }
        let first = (lp.first().copied().unwrap_or(f64::NAN), lp.get(1).copied().unwrap_or(f64::NAN));
        let l = lp.len();
        let last = (lp.get(l.wrapping_sub(2)).copied().unwrap_or(f64::NAN), lp.last().copied().unwrap_or(f64::NAN));
        if l >= 8 && key(first.0, first.1) == key(last.0, last.1) {
            loops.push(lp);
        }
    }
    loops
}

/// An opening as wallPieces reads it (along the wall, the near edge; sill and height above the level's floor).
#[derive(Clone, Copy, Debug)]
pub struct Hole {
    pub offset: f64,
    pub width: f64,
    pub sill: f64,
    pub height: f64,
}

/// `wallPieces(wall, poly, openings, cutZ)`: the joined outline with the openings the cut passes through taken out.
pub fn wall_pieces(a: V2, b: V2, poly: &[V2], openings: &[Hole], cut_z: f64) -> Vec<Vec<V2>> {
    let ang = m::atan2(b[1] - a[1], b[0] - a[0]);
    let (c, s) = (m::cos(ang), m::sin(ang));
    let to_l = |p: V2| [(p[0] - a[0]) * c + (p[1] - a[1]) * s, -(p[0] - a[0]) * s + (p[1] - a[1]) * c];
    let to_w = |p: V2| [a[0] + p[0] * c - p[1] * s, a[1] + p[0] * s + p[1] * c];
    let mut pieces: Vec<Vec<V2>> = vec![poly.iter().map(|p| to_l(*p)).collect()];
    for o in openings {
        if !(o.width > 0.0) || cut_z < o.sill || cut_z > o.sill + o.height {
            continue;
        }
        let mut next = Vec::new();
        for p in &pieces {
            let l = clip_x(p, o.offset, -1.0);
            let r = clip_x(p, o.offset + o.width, 1.0);
            if l.len() >= 3 {
                next.push(l);
            }
            if r.len() >= 3 {
                next.push(r);
            }
        }
        pieces = next;
    }
    pieces.into_iter().map(|p| p.into_iter().map(to_w).collect()).collect()
}

fn clip_x(p: &[V2], k: f64, side: f64) -> Vec<V2> {
    let inside = |q: V2| if side < 0.0 { q[0] <= k + 1e-9 } else { q[0] >= k - 1e-9 };
    let n = p.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (a, b) = (at(p, i), at(p, nx(i, n)));
        let (ia, ib) = (inside(a), inside(b));
        if ia {
            out.push(a);
        }
        if ia != ib {
            let t = (k - a[0]) / (b[0] - a[0]);
            out.push([k, a[1] + (b[1] - a[1]) * t]);
        }
    }
    out
}

/// A dimension: a and b on a face, its text, how far out (screen px / paper mm; the sign puts it on the face's side).
#[derive(Clone, Debug, PartialEq)]
pub struct Dim {
    pub a: V2,
    pub b: V2,
    pub text: String,
    pub off_px: Option<f64>,
    pub off_mm: Option<f64>,
    /// a plan-mm offset (previews)
    pub off: Option<f64>,
    pub key: Option<f64>,
}

/// `wallDims(walls, outlines, openings, fmt, { exteriorOf })`
pub fn wall_dims(walls: &[Wall], outlines: &[(String, Vec<V2>)], openings: &[crate::draw::openings::Opening], units: &str) -> Vec<Dim> {
    let mut out = Vec::new();
    for w in walls {
        let Some(poly) = outline_of(outlines, &w.guid) else { continue };
        let l = m::hypot(w.b[0] - w.a[0], w.b[1] - w.a[1]);
        if l < 1.0 {
            continue;
        }
        let u = [(w.b[0] - w.a[0]) / l, (w.b[1] - w.a[1]) / l];
        let n = [-u[1], u[0]];
        let exterior = wall_type(&w.type_key).is_none_or(|t| t.exterior);
        let side = if exterior { if w.ext_side == "right" { -1.0 } else { 1.0 } } else { 1.0 };
        let local: Vec<V2> =
            poly.iter().map(|&[x, y]| [(x - w.a[0]) * u[0] + (y - w.a[1]) * u[1], (x - w.a[0]) * n[0] + (y - w.a[1]) * n[1]]).collect();
        let xs: Vec<f64> = local.iter().filter(|[_, y]| (y - side * w.t / 2.0).abs() < 0.5).map(|[x, _]| *x).collect();
        if xs.len() < 2 {
            continue;
        }
        let (e0, e1) = (m::min_of(xs.iter().copied()), m::max_of(xs.iter().copied()));
        let pp = |x: f64| [w.a[0] + u[0] * x + n[0] * side * w.t / 2.0, w.a[1] + u[1] * x + n[1] * side * w.t / 2.0];
        let mut mine: Vec<&crate::draw::openings::Opening> = openings.iter().filter(|o| o.wall == w.id).collect();
        mine.sort_by(|a, b| crate::js::sign_ord(a.offset - b.offset));
        let mut stops = vec![e0];
        for o in &mine {
            stops.push(m::max(e0, o.offset));
            stops.push(m::min(e1, o.offset + o.width));
        }
        stops.push(e1);
        let mut st: Vec<f64> = Vec::new();
        for x in stops {
            let r = m::round(x * 100.0) / 100.0;
            if !st.contains(&r) {
                st.push(r);
            }
        }
        st.sort_by(|a, b| crate::js::sign_ord(a - b));
        if !mine.is_empty() {
            for pair in st.windows(2) {
                let [s0, s1] = [pair.first().copied().unwrap_or(0.0), pair.get(1).copied().unwrap_or(0.0)];
                if s1 - s0 > 0.5 {
                    out.push(Dim {
                        a: pp(s0),
                        b: pp(s1),
                        text: fmt_len(s1 - s0, units),
                        off_px: Some(side * 22.0),
                        off_mm: Some(side * 5.0),
                        off: None,
                        key: Some(w.id),
                    });
                }
            }
        }
        let many = !mine.is_empty();
        out.push(Dim {
            a: pp(e0),
            b: pp(e1),
            text: fmt_len(e1 - e0, units),
            off_px: Some(side * if many { 44.0 } else { 22.0 }),
            off_mm: Some(side * if many { 10.0 } else { 5.0 }),
            off: None,
            key: Some(w.id),
        });
    }
    out
}

/// A drawn wall in the scene.
#[derive(Clone, Debug, PartialEq)]
pub struct SWall {
    pub key: f64,
    pub guid: String,
    pub poly: Vec<V2>,
    pub pieces: Vec<Vec<V2>>,
}
/// A door / window / roof symbol in the scene.
#[derive(Clone, Debug, PartialEq)]
pub struct SSym {
    pub key: f64,
    pub guid: Option<String>,
    pub sym: Symbol,
    pub roof: bool,
}
/// An element cut from the meshes.
#[derive(Clone, Debug, PartialEq)]
pub struct SCut {
    pub key: f64,
    pub segs: Vec<f64>,
    pub loops: Vec<Vec<f64>>,
}
/// A room label.
#[derive(Clone, Debug, PartialEq)]
pub struct SRoom {
    pub key: f64,
    pub poly: Vec<V2>,
    pub at: V2,
    pub label: Vec<String>,
}

/// What the plan view (and the plan PDF) draws.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Scene {
    pub walls: Vec<SWall>,
    pub symbols: Vec<SSym>,
    pub rooms: Vec<SRoom>,
    pub dims: Vec<Dim>,
    pub cut: Vec<SCut>,
    pub below: Vec<SCut>,
    pub wall_edges: Option<Vec<Vec<V2>>>,
}

/// An element key (keyBase + expressID, a whole number below 2^32) as a map key.
pub fn key_u64(k: f64) -> u64 {
    crate::model::u32_of(k).into()
}
fn key_f64(k: u64) -> f64 {
    u32::try_from(k).map_or(f64::NAN, f64::from)
}

/// The keys of what the tools drew (walls, doors / windows, roofs' parts): drawn from their parameters, not cut.
pub fn own_keys(sn: &Snapshot, key_base: f64) -> std::collections::HashSet<u64> {
    let mut s = std::collections::HashSet::new();
    let k = |id: f64| key_u64(key_base + id);
    for w in &sn.walls {
        s.insert(k(w.id));
    }
    for o in &sn.openings {
        s.insert(k(o.id));
    }
    for r in &sn.derived.roofs {
        for p in &r.parts {
            s.insert(k(*p));
        }
    }
    s
}

/// The cut of the rebuilt model for a level (`cutPlan`), with each element's closed loops.
pub fn cut_scene(c: &Cut) -> (Vec<SCut>, Vec<SCut>) {
    let cut = c.cut.iter().map(|(k, segs)| SCut { key: key_f64(*k), loops: chain_loops(segs), segs: segs.clone() }).collect();
    let below = c.below.iter().map(|(k, segs)| SCut { key: key_f64(*k), segs: segs.clone(), loops: Vec::new() }).collect();
    (cut, below)
}

/// Loops of another level (`loopsOf`), asked for when a roof's walls are on it.
pub type LoopsOf<'a> = dyn FnMut(Id) -> Option<crate::draw::slabs::StoreyLoops> + 'a;

/// `makeScene()`: the plan scene of a level.
#[allow(clippy::too_many_arguments)]
pub fn make_scene(
    sn: &Snapshot,
    level: Id,
    loops: &Loops,
    cut: &[SCut],
    below: &[SCut],
    units: &str,
    dims: bool,
    key_base: f64,
    loops_of: &mut LoopsOf<'_>,
) -> Scene {
    let walls: Vec<&Wall> = sn.walls.iter().filter(|w| w.storey == Some(level)).collect();
    let segs: Vec<_> = walls.iter().map(|w| w.seg()).collect();
    let outlines = wall_outlines(&segs, 1.0);
    let own = own_keys(sn, key_base);
    let ops: Vec<&crate::draw::openings::Opening> = sn.openings.iter().filter(|o| o.storey == Some(level)).collect();
    let mut scene = Scene::default();
    for w in &walls {
        let poly = outline_of(&outlines, &w.guid).cloned().unwrap_or_default();
        let holes: Vec<Hole> = ops
            .iter()
            .filter(|o| o.wall == w.id)
            .map(|o| Hole { offset: o.offset, width: o.width, sill: o.sill, height: o.height })
            .collect();
        let pieces = if poly.is_empty() { Vec::new() } else { wall_pieces(w.a, w.b, &poly, &holes, CUT) };
        scene.walls.push(SWall { key: key_base + w.id, guid: w.guid.clone(), poly, pieces });
    }
    for o in &ops {
        let Some(w) = walls.iter().find(|w| w.id == o.wall) else { continue };
        let spec = OpSpec {
            door: o.door,
            type_key: o.type_key.clone(),
            width: o.width,
            height: o.height,
            sill: o.sill,
            hand: o.hand.clone(),
            face: o.face.clone(),
            offset: o.offset,
        };
        scene.symbols.push(SSym {
            key: key_base + o.id,
            guid: Some(o.guid.clone()),
            sym: opening_symbol(&spec, w.a, w.b, w.t),
            roof: false,
        });
    }
    for sp in sn.derived.spaces.iter().filter(|x| x.storey == Some(level)) {
        let Some(lp) = loop_at(&loops.rooms, sp.seed) else { continue };
        let name = if sp.name.is_empty() { sp.number.clone() } else { sp.name.clone() };
        scene.rooms.push(SRoom {
            key: key_base + sp.id,
            poly: lp.poly.clone(),
            at: label_point(&lp.poly),
            label: vec![name, fmt_area(lp.area, units)],
        });
    }
    scene.cut = cut.iter().filter(|c| !own.contains(&key_u64(c.key))).cloned().collect();
    scene.below = below.iter().filter(|c| !own.contains(&key_u64(c.key))).cloned().collect();
    // roofs on this level: the eave (dashed), ridges and hips (thin)
    for r in sn.derived.roofs.iter().filter(|x| x.storey == Some(level)) {
        let Some(wl) = sn.levels.iter().find(|l| l.guid.as_deref() == Some(r.walls.as_str())) else { continue };
        let other;
        let lps = if wl.id == level {
            loops
        } else {
            match loops_of(wl.id) {
                Some(x) => {
                    other = x.loops;
                    &other
                }
                None => continue,
            }
        };
        let Some(o) = loop_at(&lps.outlines, r.seed) else { continue };
        let ws: Vec<Wall> = sn.walls.iter().filter(|w| w.storey == Some(wl.id)).cloned().collect();
        let kinds = edge_kinds(&o.poly, &ws, &r.set());
        let Ok(plan) = roof_geometry(&o.poly, &kinds, r.thickness, 0.0, r.fascia, roof_defaults().fascia_t, r.soffit) else { continue };
        let ne = plan.eave.len();
        let dashed = (0..ne)
            .map(|i| [at(&plan.eave, i)[0], at(&plan.eave, i)[1], at(&plan.eave, nx(i, ne))[0], at(&plan.eave, nx(i, ne))[1]])
            .collect();
        let sym = Symbol { dashed, thin: plan.ridge.clone(), poly: plan.eave.clone(), ..Symbol::default() };
        scene.symbols.push(SSym { key: key_base + r.id, guid: None, sym, roof: true });
    }
    let pieces: Vec<Vec<V2>> = scene.walls.iter().flat_map(|w| w.pieces.iter().cloned()).collect();
    if !pieces.is_empty() {
        let l = plan_loops(&pieces);
        let mut e: Vec<Vec<V2>> = Vec::new();
        for r in &l.rooms {
            e.push(r.poly.clone());
            e.extend(r.holes.iter().cloned());
        }
        e.extend(l.outlines.iter().map(|o| o.poly.clone()));
        scene.wall_edges = Some(e);
    }
    if dims {
        let ow: Vec<Wall> = walls.iter().map(|w| (*w).clone()).collect();
        let oo: Vec<crate::draw::openings::Opening> = ops.iter().map(|o| (*o).clone()).collect();
        scene.dims = wall_dims(&ow, &outlines, &oo, units);
    }
    scene
}
